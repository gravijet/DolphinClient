//! Native-client launch: fetch the vanilla client jar (models + textures) and
//! the DolphinClient native client binary, then spawn the client with the
//! player's Minecraft session. No Java, libraries, natives or Fabric — the
//! native client renders the world itself and has the block report baked in.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

use anyhow::{bail, Context, Result};

use crate::auth::Session;
use crate::config;
use crate::events::Event;
use crate::game;

/// Origin the native client binary is served from (same host as the website
/// downloads). Overridable via `DOLPHIN_CLIENT_URL` (base) for testing.
const CLIENT_BASE_URL: &str = "https://dolphin.gravijet.net/downloads";

/// Release asset name for this OS (matches the CI `release.yml` client job).
fn client_asset_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "DolphinClient-Client-windows-x64.exe"
    } else if cfg!(target_os = "macos") {
        "DolphinClient-Client-macos-arm64"
    } else {
        "DolphinClient-Client-linux-x64"
    }
}

/// Local filename for the cached client binary.
fn local_bin_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "dolphinclient.exe"
    } else {
        "dolphinclient"
    }
}

/// Ensure the native client binary is present locally and return its path.
///
/// - `DOLPHIN_CLIENT_BIN=<path>` uses that binary directly (dev/testing).
/// - otherwise download `{DOLPHIN_CLIENT_URL|CLIENT_BASE_URL}/<asset>` into the
///   launcher data dir, skipping the download when it is already cached.
fn ensure_client_bin(client: &reqwest::blocking::Client, tx: &Sender<Event>) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("DOLPHIN_CLIENT_BIN") {
        let p = PathBuf::from(p);
        if p.exists() {
            let _ = tx.send(Event::Log(format!("Client-Binary (lokal): {}", p.display())));
            return Ok(p);
        }
        bail!("DOLPHIN_CLIENT_BIN gesetzt, aber Datei fehlt: {}", p.display());
    }

    let dir = config::data_dir().join("bin");
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(local_bin_name());
    if !dest.exists() {
        let base =
            std::env::var("DOLPHIN_CLIENT_URL").unwrap_or_else(|_| CLIENT_BASE_URL.to_string());
        let url = format!("{}/{}", base.trim_end_matches('/'), client_asset_name());
        let _ = tx.send(Event::Status("DolphinClient-Client laden …".into()));
        game::download_file(client, &url, &dest)
            .with_context(|| format!("Client-Download fehlgeschlagen: {url}"))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&dest) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&dest, perms);
        }
    }
    Ok(dest)
}

/// Download the vanilla jar, fetch the client binary and spawn it. `server` may
/// be empty (the client then shows its own connect screen). The session is
/// passed via environment variables — never on the command line — so the access
/// token never appears in a process listing. Returns once the client started.
pub fn launch(session: &Session, server: &str, tx: &Sender<Event>) -> Result<()> {
    let client = game::http();

    // 1. Vanilla client jar — the only thing the native client needs from Mojang.
    let jar = game::ensure_client_jar(&client, tx)?;

    // 2. The native client binary itself.
    let bin = ensure_client_bin(&client, tx)?;
    let _ = tx.send(Event::Progress(0.9));

    // 3. Spawn it. Redirect output to a log so crashes are diagnosable in the
    //    windowed (no-console) build.
    let log_path = config::minecraft_dir().join("dolphinclient-native.log");
    let log = std::fs::File::create(&log_path).ok();
    let _ = tx.send(Event::Status("DolphinClient starten …".into()));
    let _ = tx.send(Event::Log(format!("Client: {}", bin.display())));
    let _ = tx.send(Event::Log(format!("Client-JAR: {}", jar.display())));
    let _ = tx.send(Event::Log(format!("Log-Datei: {}", log_path.display())));

    let mut cmd = Command::new(&bin);
    cmd.arg("--mc-jar").arg(&jar);
    if !server.trim().is_empty() {
        cmd.arg("--server").arg(server.trim());
    }
    cmd.env("DOLPHIN_MC_TOKEN", &session.access_token)
        .env("DOLPHIN_MC_UUID", &session.uuid)
        .env("DOLPHIN_MC_NAME", &session.username);
    if let Some(f) = log {
        if let Ok(err) = f.try_clone() {
            cmd.stderr(Stdio::from(err));
        }
        cmd.stdout(Stdio::from(f));
    }

    let _ = tx.send(Event::Progress(1.0));
    cmd.spawn()
        .with_context(|| format!("Client-Start fehlgeschlagen: {}", bin.display()))?;
    let _ = tx.send(Event::Launched);
    Ok(())
}
