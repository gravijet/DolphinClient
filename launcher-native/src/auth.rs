//! Microsoft OAuth 2.0 device-code ("link code") flow → Xbox Live → XSTS →
//! Minecraft services → profile. Only legitimate Microsoft login; no cracked
//! accounts (Mojang EULA).
//!
//! The Azure application (client) ID is baked in but can be overridden with
//! `DOLPHIN_MS_CLIENT_ID`. End users never see Azure.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::events::Event;
use crate::tokens;

/// Azure app (client) ID registered for DolphinClient (portal.azure.com).
pub const DEFAULT_CLIENT_ID: &str = "fee9e26b-cfdd-4c9d-b15a-294f01172f66";

const DEVICECODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

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
        DEVICECODE_URL,
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
            TOKEN_URL,
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
        TOKEN_URL,
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
