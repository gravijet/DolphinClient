//! Microsoft login → Xbox Live → XSTS → Minecraft services → profile.
//! Only legitimate Microsoft login; no cracked accounts (Mojang EULA).
//!
//! Two backends:
//!   * **Live (default):** the official Minecraft launcher client id via
//!     `login.live.com` — already allowlisted for the Minecraft API, so **no
//!     custom Azure app and no approval are needed** (the same approach
//!     prismarine-auth / MCProtocolLib use). Device-code flow.
//!   * **Azure/AAD:** set `DOLPHIN_MS_CLIENT_ID` to your own *approved* Azure app
//!     to use `login.microsoftonline.com` — enables the browser (loopback) login.

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

/// Official Minecraft launcher client id (allowlisted for the Minecraft API).
/// Used with `login.live.com` — no custom Azure app needed.
pub const DEFAULT_CLIENT_ID: &str = "00000000402b5328";

const AAD_SCOPE: &str = "XboxLive.signin offline_access";
const LIVE_SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// The result of a successful login — everything the game launch needs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub uuid: String,
    pub username: String,
    pub access_token: String,
}

/// Cheap check whether a cached Minecraft access token is still valid (they
/// expire after ~24 h). A stale token would otherwise only fail deep inside
/// the game client. Network errors count as "valid" — being offline must not
/// block a launch that might still work.
pub fn access_token_valid(access: &str) -> bool {
    let client = match Client::builder().timeout(Duration::from_secs(8)).build() {
        Ok(c) => c,
        Err(_) => return true,
    };
    match client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(access)
        .send()
    {
        Ok(res) => res.status() != reqwest::StatusCode::UNAUTHORIZED,
        Err(_) => true,
    }
}

/// AAD tenant for the Azure backend (default personal accounts).
fn tenant() -> String {
    std::env::var("DOLPHIN_MS_TENANT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "consumers".to_string())
}

/// Use the Azure/AAD backend? True when a custom client id is provided (or when
/// `DOLPHIN_MS_MODE` says so). Default is the Live backend (no Azure app needed).
pub fn is_azure() -> bool {
    if let Ok(m) = std::env::var("DOLPHIN_MS_MODE") {
        if !m.trim().is_empty() {
            return m.eq_ignore_ascii_case("azure") || m.eq_ignore_ascii_case("aad");
        }
    }
    std::env::var("DOLPHIN_MS_CLIENT_ID")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

pub fn client_id() -> String {
    std::env::var("DOLPHIN_MS_CLIENT_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string())
}

fn scope() -> &'static str {
    if is_azure() {
        AAD_SCOPE
    } else {
        LIVE_SCOPE
    }
}

fn devicecode_url() -> String {
    if is_azure() {
        format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/devicecode",
            tenant()
        )
    } else {
        "https://login.live.com/oauth20_connect.srf".to_string()
    }
}

fn token_url() -> String {
    if is_azure() {
        format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            tenant()
        )
    } else {
        "https://login.live.com/oauth20_token.srf".to_string()
    }
}

/// Authorize endpoint — only used by the AAD loopback browser flow.
fn authorize_url() -> String {
    format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0/authorize",
        tenant()
    )
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

/// Device-code login (works for both backends). The user opens a page and types
/// a short code — this is the method prismarine-auth / mineflayer use by default.
pub fn login_device(tx: &Sender<Event>) -> Result<Session> {
    let client = http();
    let id = client_id();
    let azure = is_azure();

    let mut form: Vec<(&str, &str)> = vec![("client_id", &id), ("scope", scope())];
    if !azure {
        // login.live.com requires this parameter.
        form.push(("response_type", "device_code"));
    }
    let dc = post_form(&client, &devicecode_url(), &form)?;
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
        .or_else(|| dc["verification_url"].as_str())
        .unwrap_or("https://www.microsoft.com/link")
        .to_string();
    // A "complete" URL with the one-time code pre-filled → one click, sign in.
    let complete = dc["verification_uri_complete"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let sep = if verification_uri.contains('?') {
                '&'
            } else {
                '?'
            };
            format!(
                "{}{}otc={}",
                verification_uri,
                sep,
                urlencoding::encode(&user_code)
            )
        });
    let message =
        "Ein Browser-Fenster wurde geöffnet — melde dich dort mit Microsoft an.".to_string();
    let _ = tx.send(Event::Device {
        complete: complete.clone(),
        code: user_code,
        message,
    });
    // Open the pre-filled sign-in page automatically.
    let _ = open::that(&complete);

    let mut interval = dc["interval"].as_u64().unwrap_or(5).max(1);
    let expires_in = dc["expires_in"].as_u64().unwrap_or(900);
    let deadline = Instant::now() + Duration::from_secs(expires_in);
    let mut ms_token = String::new();
    let mut refresh_token = String::new();

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
            refresh_token = rt.to_string();
        }
        break;
    }
    if ms_token.is_empty() {
        bail!("Anmeldung abgelaufen — bitte erneut versuchen.");
    }

    let refresh = (!refresh_token.is_empty()).then_some(refresh_token.as_str());
    minecraft_session(&client, &ms_token, azure, tx, refresh)
}

/// Silent login using a stored refresh token, if any.
pub fn login_with_refresh(tx: &Sender<Event>) -> Result<Session> {
    let client = http();
    let id = client_id();
    let azure = is_azure();
    let refresh = tokens::load_refresh().context("Kein gespeichertes Token vorhanden.")?;

    let _ = tx.send(Event::Status("Sitzung wird erneuert …".into()));
    let tok = post_form(
        &client,
        &token_url(),
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &id),
            ("refresh_token", &refresh),
            ("scope", scope()),
        ],
    )?;
    if err_of(&tok).is_some() {
        tokens::clear();
        bail!("refresh: {}", err_desc(&tok));
    }
    let refresh = tok["refresh_token"].as_str().map(str::to_string);
    if let Some(rt) = &refresh {
        tokens::save_refresh(rt);
    }
    let ms_token = tok["access_token"].as_str().unwrap_or_default().to_string();
    if ms_token.is_empty() {
        bail!("Konnte Sitzung nicht erneuern.");
    }

    minecraft_session(&client, &ms_token, azure, tx, refresh.as_deref())
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

/// Browser sign-in (AAD authorization-code flow + PKCE, loopback redirect).
/// Only available with your own Azure app (`DOLPHIN_MS_CLIENT_ID`), because the
/// loopback redirect URI must be registered on the app.
pub fn login_via_browser(tx: &Sender<Event>) -> Result<Session> {
    let client = http();
    let id = client_id();

    let verifier = random_b64url(48);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_b64url(16);

    let listener =
        TcpListener::bind("127.0.0.1:0").context("Konnte lokalen Login-Server nicht starten")?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://localhost:{}", port);

    let auth_url = format!(
        "{authorize}?client_id={client}&response_type=code&redirect_uri={redirect}\
         &response_mode=query&scope={scope}&state={state}\
         &code_challenge={challenge}&code_challenge_method=S256&prompt=select_account",
        authorize = authorize_url(),
        client = urlencoding::encode(&id),
        redirect = urlencoding::encode(&redirect),
        scope = urlencoding::encode(scope()),
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
            ("scope", scope()),
        ],
    )?;
    if err_of(&tok).is_some() {
        bail!("token: {}", err_desc(&tok));
    }
    let refresh = tok["refresh_token"].as_str().map(str::to_string);
    if let Some(rt) = &refresh {
        tokens::save_refresh(rt);
    }
    let ms_token = tok["access_token"].as_str().unwrap_or_default().to_string();
    if ms_token.is_empty() {
        bail!("Kein Zugriffstoken erhalten.");
    }

    minecraft_session(&client, &ms_token, true, tx, refresh.as_deref())
}

/// MS access token → Xbox Live → XSTS → Minecraft services → profile.
/// `azure` selects the RpsTicket preamble (`d=` for AAD, `t=` for live.com).
/// `refresh` (when present) is the renewable Microsoft refresh token; it and the
/// resulting Minecraft access token are stored per-account for multi-account.
fn minecraft_session(
    client: &Client,
    ms_token: &str,
    azure: bool,
    tx: &Sender<Event>,
    refresh: Option<&str>,
) -> Result<Session> {
    let preamble = if azure { "d=" } else { "t=" };

    let _ = tx.send(Event::Status("Xbox-Live-Anmeldung …".into()));
    let xbl = post_json(
        client,
        "https://user.auth.xboxlive.com/user/authenticate",
        &json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("{}{}", preamble, ms_token)
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
            "Minecraft-Profil nicht abrufbar (HTTP {}). Besitzt das Konto Minecraft: Java Edition?",
            prof.status()
        );
    }
    let profile: Value = prof.json()?;
    let session = Session {
        uuid: profile["id"].as_str().unwrap_or_default().to_string(),
        username: profile["name"].as_str().unwrap_or("Spieler").to_string(),
        access_token,
    };
    // Persist per-account secrets so multiple accounts can coexist.
    if !session.uuid.is_empty() {
        if let Some(rt) = refresh {
            tokens::save_refresh_for(&session.uuid, rt);
        }
        tokens::save_access_for(&session.uuid, &session.access_token);
    }
    Ok(session)
}

/// Silent login using the refresh token stored for one specific account.
pub fn login_with_refresh_for(uuid: &str, tx: &Sender<Event>) -> Result<Session> {
    let client = http();
    let id = client_id();
    let azure = is_azure();
    let refresh = tokens::load_refresh_for(uuid)
        .context("Für dieses Konto ist kein erneuerbares Token gespeichert.")?;

    let _ = tx.send(Event::Status("Sitzung wird erneuert …".into()));
    let tok = post_form(
        &client,
        &token_url(),
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &id),
            ("refresh_token", &refresh),
            ("scope", scope()),
        ],
    )?;
    if err_of(&tok).is_some() {
        bail!("refresh: {}", err_desc(&tok));
    }
    let new_refresh = tok["refresh_token"].as_str().map(str::to_string);
    let ms_token = tok["access_token"].as_str().unwrap_or_default().to_string();
    if ms_token.is_empty() {
        bail!("Konnte Sitzung nicht erneuern.");
    }
    minecraft_session(&client, &ms_token, azure, tx, new_refresh.as_deref())
}

/// Resolve a ready-to-launch session for an account, renewing automatically and
/// only demanding a fresh sign-in as a last resort. Tried in order:
///   1. our stored Microsoft refresh token (silent renew),
///   2. the last cached Minecraft access token, if still valid,
///   3. a still-valid token freshly imported from another launcher on this
///      device (Vanilla/Lunar) — this is the "von anderen Clients genommen" case.
/// Only when all three fail does it return an error asking the user to re-login.
pub fn resolve_session(
    uuid: &str,
    username: &str,
    has_refresh: bool,
    tx: &Sender<Event>,
) -> Result<Session> {
    // 1. Silent renew with our own refresh token.
    if has_refresh {
        match login_with_refresh_for(uuid, tx) {
            Ok(session) => return Ok(session),
            Err(e) => {
                let _ = tx.send(Event::Log(format!(
                    "Token-Erneuerung fehlgeschlagen ({e}); versuche zwischengespeicherte Sitzung …"
                )));
            }
        }
    }

    // 2. A cached access token that is still accepted by the Minecraft API.
    if let Some(access) = tokens::load_access_for(uuid) {
        if access_token_valid(&access) {
            let _ =
                tx.send(Event::Status("Zwischengespeicherte Sitzung wird verwendet …".into()));
            return Ok(Session {
                uuid: uuid.to_string(),
                username: username.to_string(),
                access_token: access,
            });
        }
    }

    // 3. Re-import a fresh, valid token from another launcher on this device.
    let _ = tx.send(Event::Status(
        "Sitzung wird von einem anderen Launcher übernommen …".into(),
    ));
    for imported in crate::accounts::discover() {
        if imported.uuid == uuid && access_token_valid(&imported.access_token) {
            tokens::save_access_for(uuid, &imported.access_token);
            let _ = tx.send(Event::Log(format!(
                "Gültige Sitzung aus {} übernommen.",
                imported.source
            )));
            return Ok(Session {
                uuid: uuid.to_string(),
                username: username.to_string(),
                access_token: imported.access_token,
            });
        }
    }

    bail!(
        "Die Anmeldung für {username} konnte nicht automatisch erneuert werden. \
         Bitte das Konto im Konten-Tab neu anmelden."
    )
}
