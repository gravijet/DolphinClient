//! Lightweight launcher update check against the backend feed.
//!
//! The backend exposes `GET /v1/updates/:channel` returning
//! `{ launcher: { version }, client: { version, minecraft } }`. If the
//! advertised launcher version differs from ours, we surface a note in the UI.
//! (Actual binary self-update happens through the OS installer / release feed.)

use std::time::Duration;

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const FEED: &str = "https://dolphin.gravijet.net/api/v1/updates/stable";

/// Returns `Some(latest_version)` when a different launcher version is offered.
pub fn check() -> Option<String> {
    let url = std::env::var("DOLPHIN_UPDATE_FEED").unwrap_or_else(|_| FEED.to_string());
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!(
            "DolphinClient-Launcher/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;

    let res = client.get(&url).send().ok()?;
    if !res.status().is_success() {
        return None;
    }
    let json: serde_json::Value = res.json().ok()?;
    let latest = json
        .get("launcher")
        .and_then(|l| l.get("version"))
        .and_then(|v| v.as_str())?;

    if latest != CURRENT {
        Some(latest.to_string())
    } else {
        None
    }
}
