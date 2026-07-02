//! Microsoft OAuth 2.0 device-code ("link code") flow → Xbox Live → XSTS →
//! Minecraft services → profile. Only legitimate Microsoft login; no cracked
//! accounts (Mojang EULA).
//!
//! The Azure application (client) ID is baked in but can be overridden with
//! `DOLPHIN_MS_CLIENT_ID`. End users never see Azure.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::events::Event;
use crate::tokens;

/// Azure app (client) ID registered for DolphinClient (portal.azure.com).
pub const DEFAULT_CLIENT_ID: &str = "d7c09844-ad46-4930-a39b-ac04ca90d894";

const SCOPE: &str = "XboxLive.signin offline_access";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// OAuth tenant. `consumers` = personal Microsoft accounts (Minecraft default).
/// Override with `DOLPHIN_MS_TENANT` (e.g. `common` if the Azure app is
/// registered for "any org directory and personal Microsoft accounts", or a
/// specific tenant id). The Azure app MUST support personal Microsoft accounts,
/// otherwise Microsoft returns AADSTS700016 (app not found in that directory).
fn tenant() -> String {
    std::env::var("DOLPHIN_MS_TENANT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "consumers".to_string())
}

fn devicecode_url() -> String {
    format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/devicecode",
        tenant()
    )
}

fn token_url() -> String {
    format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
        tenant()
    )
}

/// The result of a successful login — everything the game launch needs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub uuid: String,
    pub username: String,
    pub access_token: String,
}

pub fn client_id() -> String {
    std::env::var("DOLPHIN_MS_CLIENT_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string())
}

fn http() -> Client {
    Client::builder()
        .user_agent(concat!(
            "DolphinClient-Launcher/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build HTTP client")
}

fn post_form(client: &Client, url: &str, form: &[(&str, &str)]) -> Result<Value> {
    let res = client.post(url).form(form).send()?;
    Ok(res.json()?)
}

fn post_json(client: &Client, url: &str, body: &Value) -> Result<Value> {
    let res = client
        .post(url)
        .header("Accept", "application/json")
        .json(body)
        .send()?;
    if !res.status().is_success() {
        bail!("{} -> HTTP {}", url, res.status());
    }
    Ok(res.json()?)
}

fn err_of(v: &Value) -> Option<&str> {
    v.get("error").and_then(|e| e.as_str())
}

fn err_desc(v: &Value) -> String {
    v.get("error_description")
        .and_then(|d| d.as_str())
        .or_else(|| err_of(v))
        .unwrap_or("unbekannter Fehler")
        .to_string()
}

/// Full interactive login via the device-code flow.
pub fn login(tx: &Sender<Event>) -> Result<Session> {
    let client = http();
    let id = client_id();

    // 1. Request a device code and show the user the code + URL.
    let dc = post_form(
        &client,
        &devicecode_url(),
        &[("client_id", &id), ("scope", SCOPE)],
    )?;
    if err_of(&dc).is_some() {
        bail!("devicecode: {}", err_desc(&dc));
    }
    let device_code = dc["device_code"]
        .as_str()
        .context("Antwort ohne device_code")?
        .to_string();
    let user_code = dc["user_code"].as_str().unwrap_or_default().to_string();
    let verification_uri = dc["verification_uri"]
        .as_str()
        .unwrap_or("https://microsoft.com/link")
        .to_string();
    let message = dc["message"]
        .as_str()
        .unwrap_or("Öffne die Seite und gib den Code ein.")
        .to_string();
    let _ = tx.send(Event::Device {
        url: verification_uri,
        code: user_code,
        message,
    });

    // 2. Poll the token endpoint until the user confirms.
    let mut interval = dc["interval"].as_u64().unwrap_or(5);
    let expires_in = dc["expires_in"].as_u64().unwrap_or(900);
    let deadline = Instant::now() + Duration::from_secs(expires_in);
    let mut ms_token = String::new();

    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(interval));
        let tok = post_form(
            &client,
            &token_url(),
            &[
                ("grant_type", DEVICE_GRANT),
                ("client_id", &id),
                ("device_code", &device_code),
            ],
        )?;
        match err_of(&tok) {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += 5;
                continue;
            }
            Some(_) => bail!("token: {}", err_desc(&tok)),
            None => {}
        }
        ms_token = tok["access_token"].as_str().unwrap_or_default().to_string();
        if let Some(rt) = tok["refresh_token"].as_str() {
            tokens::save_refresh(rt);
        }
        break;
    }
    if ms_token.is_empty() {
        bail!("Anmeldung abgelaufen — bitte erneut versuchen.");
    }

    minecraft_session(&client, &ms_token, tx)
}

/// Silent login using a stored refresh token, if any.
pub fn login_with_refresh(tx: &Sender<Event>) -> Result<Session> {
    let client = http();
    let id = client_id();
    let refresh = tokens::load_refresh().context("Kein gespeichertes Token vorhanden.")?;

    let _ = tx.send(Event::Status("Sitzung wird erneuert …".into()));
    let tok = post_form(
        &client,
        &token_url(),
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &id),
            ("refresh_token", &refresh),
            ("scope", SCOPE),
        ],
    )?;
    if err_of(&tok).is_some() {
        tokens::clear();
        bail!("refresh: {}", err_desc(&tok));
    }
    if let Some(rt) = tok["refresh_token"].as_str() {
        tokens::save_refresh(rt);
    }
    let ms_token = tok["access_token"].as_str().unwrap_or_default().to_string();
    if ms_token.is_empty() {
        bail!("Konnte Sitzung nicht erneuern.");
    }

    minecraft_session(&client, &ms_token, tx)
}

/// Random URL-safe base64 string of `bytes` random bytes (for PKCE / state).
fn random_b64url(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("OS randomness unavailable");
    URL_SAFE_NO_PAD.encode(buf)
}

/// Parse `code` / `state` / `error` out of a redirect path like `/?code=…&state=…`.
fn parse_query(path: &str) -> (Option<String>, Option<String>, Option<String>) {
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let (mut code, mut state, mut error) = (None, None, None);
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let val = urlencoding::decode(v)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| v.to_string());
        match k {
            "code" => code = Some(val),
            "state" => state = Some(val),
            "error" => error = error.or(Some(val)),
            "error_description" => error = Some(val),
            _ => {}
        }
    }
    (code, state, error)
}

/// Block (up to 5 min) on the loopback listener for the OAuth redirect, answer
/// the browser with a friendly page, and return the authorization code + state.
fn wait_for_redirect(listener: &TcpListener) -> Result<(String, String)> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let first = req.lines().next().unwrap_or("");
                let path = first.split_whitespace().nth(1).unwrap_or("");
                let (code, state, error) = parse_query(path);

                let ok = code.is_some() && error.is_none();
                let inner = if ok {
                    "<h1>Erfolgreich angemeldet ✅</h1><p>Du kannst dieses Fenster schließen und zum DolphinClient-Launcher zurückkehren.</p>".to_string()
                } else {
                    format!(
                        "<h1>Anmeldung fehlgeschlagen</h1><p>{}</p>",
                        error
                            .clone()
                            .unwrap_or_else(|| "Kein Code erhalten.".into())
                    )
                };
                let html = format!(
                    "<!doctype html><html lang=\"de\"><head><meta charset=\"utf-8\"><title>DolphinClient</title>\
                     <style>body{{font-family:system-ui,sans-serif;background:#050b14;color:#eaf3ff;display:grid;place-items:center;height:100vh;margin:0}}\
                     div{{text-align:center;padding:2rem;max-width:32rem}}h1{{color:#38e1c4}}</style></head>\
                     <body><div>{inner}</div></body></html>"
                );
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();

                if let Some(e) = error {
                    bail!("Anmeldung abgebrochen: {}", e);
                }
                let code = code.context("Kein Autorisierungscode in der Antwort.")?;
                return Ok((code, state.unwrap_or_default()));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    bail!("Zeitüberschreitung — keine Anmeldung im Browser erkannt.");
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Sign in via the system browser (OAuth 2.0 authorization-code flow + PKCE,
/// loopback redirect). The user just signs in — no code to type. Requires a
/// redirect URI `http://localhost` on the Azure app ("Mobile and desktop
/// applications" platform).
pub fn login_via_browser(tx: &Sender<Event>) -> Result<Session> {
    let client = http();
    let id = client_id();

    // PKCE (S256) + anti-CSRF state.
    let verifier = random_b64url(48);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_b64url(16);

    // Loopback server on an ephemeral port.
    let listener =
        TcpListener::bind("127.0.0.1:0").context("Konnte lokalen Login-Server nicht starten")?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://localhost:{}", port);

    let auth_url = format!(
        "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize\
         ?client_id={client}&response_type=code&redirect_uri={redirect}\
         &response_mode=query&scope={scope}&state={state}\
         &code_challenge={challenge}&code_challenge_method=S256&prompt=select_account",
        tenant = tenant(),
        client = urlencoding::encode(&id),
        redirect = urlencoding::encode(&redirect),
        scope = urlencoding::encode(SCOPE),
        state = urlencoding::encode(&state),
        challenge = challenge,
    );

    let _ = tx.send(Event::Status(
        "Browser zur Microsoft-Anmeldung geöffnet …".into(),
    ));
    let _ = tx.send(Event::BrowserOpen {
        url: auth_url.clone(),
    });
    let _ = open::that(&auth_url);

    let (code, got_state) = wait_for_redirect(&listener)?;
    if got_state != state {
        bail!("Sicherheitsfehler: state stimmt nicht überein.");
    }

    let _ = tx.send(Event::Status("Anmeldung wird abgeschlossen …".into()));
    let tok = post_form(
        &client,
        &token_url(),
        &[
            ("client_id", &id),
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect),
            ("code_verifier", &verifier),
            ("scope", SCOPE),
        ],
    )?;
    if err_of(&tok).is_some() {
        bail!("token: {}", err_desc(&tok));
    }
    if let Some(rt) = tok["refresh_token"].as_str() {
        tokens::save_refresh(rt);
    }
    let ms_token = tok["access_token"].as_str().unwrap_or_default().to_string();
    if ms_token.is_empty() {
        bail!("Kein Zugriffstoken erhalten.");
    }

    minecraft_session(&client, &ms_token, tx)
}

/// MS access token → Xbox Live → XSTS → Minecraft services → profile.
fn minecraft_session(client: &Client, ms_token: &str, tx: &Sender<Event>) -> Result<Session> {
    let _ = tx.send(Event::Status("Xbox-Live-Anmeldung …".into()));
    let xbl = post_json(
        client,
        "https://user.auth.xboxlive.com/user/authenticate",
        &json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", ms_token)
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }),
    )?;
    let xbl_token = xbl["Token"].as_str().context("kein Xbox-Token")?;

    let _ = tx.send(Event::Status("XSTS-Token …".into()));
    let xsts = post_json(
        client,
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        &json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }),
    )?;
    let uhs = xsts["DisplayClaims"]["xui"][0]["uhs"]
        .as_str()
        .context("kein UHS im XSTS-Token")?;
    let xsts_token = xsts["Token"].as_str().context("kein XSTS-Token")?;

    let _ = tx.send(Event::Status("Minecraft-Services …".into()));
    let mc = post_json(
        client,
        "https://api.minecraftservices.com/authentication/login_with_xbox",
        &json!({ "identityToken": format!("XBL3.0 x={};{}", uhs, xsts_token) }),
    )?;
    let access_token = mc["access_token"]
        .as_str()
        .context("kein Minecraft-Token")?
        .to_string();

    let _ = tx.send(Event::Status("Profil abrufen …".into()));
    let prof = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(&access_token)
        .send()?;
    if !prof.status().is_success() {
        bail!(
            "Minecraft-Profil nicht abrufbar (HTTP {}). Besitzt das Konto Minecraft?",
            prof.status()
        );
    }
    let profile: Value = prof.json()?;
    Ok(Session {
        uuid: profile["id"].as_str().unwrap_or_default().to_string(),
        username: profile["name"].as_str().unwrap_or("Spieler").to_string(),
        access_token,
    })
}
