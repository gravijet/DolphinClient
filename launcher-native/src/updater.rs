//! Real launcher self-update against `downloads/manifest.json`.
//!
//! `check()` compares the published launcher version with ours and returns
//! the download info. `apply()` performs the update:
//! - **Windows:** downloads the new Setup (SHA-256-verified), launches it
//!   silently (`/S`) and exits — the installer replaces the app and restarts it.
//! - **Linux/macOS:** downloads the new binary, atomically swaps it over the
//!   running executable and restarts the launcher.

use std::io::Read;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::events::Event;

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const MANIFEST: &str = "https://example.invalid/downloads/manifest.json";

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    pub url: String,
    pub sha256: Option<String>,
}

fn manifest_url() -> String {
    std::env::var("DOLPHIN_UPDATE_MANIFEST").unwrap_or_else(|_| MANIFEST.to_string())
}

fn os_key() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn http() -> Option<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("DolphinClient-Launcher/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(120))
        .build()
        .ok()
}

/// Returns update info when the published launcher version differs from ours.
pub fn check() -> Option<UpdateInfo> {
    let client = http()?;
    let res = client.get(manifest_url()).send().ok()?;
    if !res.status().is_success() {
        return None;
    }
    let json: serde_json::Value = res.json().ok()?;
    let version = json.get("version")?.as_str()?.to_string();
    if version == CURRENT {
        return None;
    }
    let plat = json.get("platforms")?.get(os_key())?;
    if plat.get("available").and_then(|a| a.as_bool()) != Some(true) {
        return None;
    }
    let file_url = plat.get("url")?.as_str()?;
    let url = if file_url.starts_with("http") {
        file_url.to_string()
    } else {
        format!("https://example.invalid{file_url}")
    };
    Some(UpdateInfo {
        version,
        url,
        sha256: plat.get("sha256").and_then(|s| s.as_str()).map(String::from),
    })
}

/// Download + verify the update payload.
fn download(info: &UpdateInfo, tx: &Sender<Event>) -> Result<Vec<u8>> {
    let client = http().context("HTTP-Client")?;
    let _ = tx.send(Event::Status(format!(
        "Launcher-Update {} wird geladen …",
        info.version
    )));
    let mut res = client
        .get(&info.url)
        .send()
        .with_context(|| format!("Update-Download fehlgeschlagen: {}", info.url))?;
    if !res.status().is_success() {
        bail!("Update-Download: HTTP {}", res.status());
    }
    let total = res.content_length().unwrap_or(0);
    let mut bytes = Vec::with_capacity(total as usize);
    let mut buf = [0u8; 65536];
    loop {
        let n = res.read(&mut buf)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        if total > 0 {
            let _ = tx.send(Event::Progress(bytes.len() as f32 / total as f32));
        }
    }
    if let Some(exp) = &info.sha256 {
        let got = format!("{:x}", Sha256::digest(&bytes));
        if !got.eq_ignore_ascii_case(exp) {
            bail!("Update beschädigt: SHA-256 erwartet {exp}, erhalten {got}.");
        }
        let _ = tx.send(Event::Log("Update-SHA-256 geprüft.".into()));
    }
    Ok(bytes)
}

/// Perform the self-update. On success this function does not return — the
/// process restarts (Unix) or hands over to the installer and exits (Windows).
pub fn apply(info: &UpdateInfo, tx: &Sender<Event>) -> Result<()> {
    let bytes = download(info, tx)?;

    #[cfg(target_os = "windows")]
    {
        // The payload is the NSIS installer: run it silently and exit. The
        // installer waits for this process to die, replaces the files and
        // restarts the launcher.
        let dir = crate::config::data_dir().join("update");
        std::fs::create_dir_all(&dir)?;
        let setup = dir.join(format!("DolphinClient-Setup-{}.exe", info.version));
        std::fs::write(&setup, &bytes)?;
        let _ = tx.send(Event::Status("Update wird installiert — der Launcher startet gleich neu …".into()));
        Command::new(&setup)
            .arg("/S")
            .spawn()
            .with_context(|| format!("Installer-Start fehlgeschlagen: {}", setup.display()))?;
        std::thread::sleep(Duration::from_millis(400));
        std::process::exit(0);
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Atomic swap over the running binary, then restart ourselves.
        let exe = std::env::current_exe().context("eigener Pfad unbekannt")?;
        let tmp = exe.with_extension("update");
        std::fs::write(&tmp, &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&tmp, &exe)
            .with_context(|| format!("Konnte {} nicht ersetzen", exe.display()))?;
        let _ = tx.send(Event::Status("Update installiert — Neustart …".into()));
        Command::new(&exe)
            .spawn()
            .with_context(|| format!("Neustart fehlgeschlagen: {}", exe.display()))?;
        std::thread::sleep(Duration::from_millis(200));
        std::process::exit(0);
    }
}
