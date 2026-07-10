//! Native-client launch: fetch the vanilla client jar (models + textures) and
//! the DolphinClient native client binary, then spawn the client with the
//! player's Minecraft session. No Java, libraries, natives or Fabric — the
//! native client renders the world itself and has the block report baked in.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::auth::Session;
use crate::config;
use crate::events::Event;
use crate::game;

/// Origin the native client binary is served from (same host as the website
/// downloads). Overridable via `DOLPHIN_CLIENT_URL` (base) for testing.
const CLIENT_BASE_URL: &str = "https://example.invalid/downloads";

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

/// Manifest key for this OS (matches `deploy/gen-manifest.mjs`).
fn os_key() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// SHA-256 of a file as lowercase hex, or `None` if it can't be read.
fn sha256_file(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// The published manifest, or `None` on any network / parse error (an offline
/// launch then falls back to whatever is already cached).
fn fetch_manifest(client: &reqwest::blocking::Client, base: &str) -> Option<serde_json::Value> {
    let url = format!("{}/manifest.json", base.trim_end_matches('/'));
    let res = client.get(&url).send().ok()?;
    if !res.status().is_success() {
        return None;
    }
    res.json().ok()
}

/// All client versions offered by the manifest archive (newest first).
pub fn available_versions(client: &reqwest::blocking::Client) -> Vec<String> {
    let base = std::env::var("DOLPHIN_CLIENT_URL").unwrap_or_else(|_| CLIENT_BASE_URL.to_string());
    let Some(manifest) = fetch_manifest(client, &base) else {
        return Vec::new();
    };
    manifest
        .get("clientVersions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("version").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Resolve a relative manifest URL (`/downloads/...`) against the site origin.
fn absolute_url(base: &str, url: &str) -> String {
    if url.starts_with("http") {
        url.to_string()
    } else {
        // base is ".../downloads"; strip the path down to the origin.
        let origin = base
            .find("://")
            .and_then(|i| base[i + 3..].find('/').map(|j| &base[..i + 3 + j]))
            .unwrap_or(base);
        format!("{}{}", origin.trim_end_matches('/'), url)
    }
}

/// Download `url` to `dest` (via a temp file), verifying the SHA-256 when the
/// manifest advertised one. Always overwrites — this is the update path.
fn download_verified(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    expected: Option<&str>,
) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let res = client.get(url).send()?;
    if !res.status().is_success() {
        bail!("Download fehlgeschlagen: {} (HTTP {})", url, res.status());
    }
    let bytes = res.bytes()?;
    if let Some(exp) = expected {
        let got = format!("{:x}", Sha256::digest(&bytes));
        if !got.eq_ignore_ascii_case(exp) {
            bail!(
                "Client-Download beschädigt: SHA-256 erwartet {exp}, erhalten {got}."
            );
        }
    }
    let tmp = dest.with_extension("part");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// Ensure the native client binary is present AND up to date, then return its
/// path. `want_version` pins an archived version ("" = newest).
///
/// - `DOLPHIN_CLIENT_BIN=<path>` uses that binary directly (dev/testing).
/// - otherwise the launcher compares the SHA-256 of the cached binary against
///   the published manifest and re-downloads whenever they differ. This is
///   what makes a freshly published client actually reach users instead of a
///   stale, forever-cached copy.
fn ensure_client_bin(
    client: &reqwest::blocking::Client,
    tx: &Sender<Event>,
    want_version: &str,
) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("DOLPHIN_CLIENT_BIN") {
        let p = PathBuf::from(p);
        if p.exists() {
            let _ = tx.send(Event::Log(format!("Client-Binary (lokal): {}", p.display())));
            return Ok(p);
        }
        bail!("DOLPHIN_CLIENT_BIN gesetzt, aber Datei fehlt: {}", p.display());
    }

    let base = std::env::var("DOLPHIN_CLIENT_URL").unwrap_or_else(|_| CLIENT_BASE_URL.to_string());
    let manifest = fetch_manifest(client, &base);
    let want = want_version.trim();

    // Where to cache + where to download from.
    let (dest, url, expected) = if want.is_empty() {
        // Newest: flat cache dir, flat download URL (as always).
        let dir = config::data_dir().join("bin");
        std::fs::create_dir_all(&dir)?;
        let expected = manifest
            .as_ref()
            .and_then(|m| m.get("client")?.get(os_key())?.get("sha256")?.as_str())
            .map(String::from);
        let url = format!("{}/{}", base.trim_end_matches('/'), client_asset_name());
        (dir.join(local_bin_name()), url, expected)
    } else {
        // Pinned version from the archive. Requires the manifest (online).
        let entry = manifest
            .as_ref()
            .and_then(|m| m.get("clientVersions")?.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|e| e.get("version").and_then(|v| v.as_str()) == Some(want))
            })
            .cloned();
        let dir = config::data_dir().join("bin").join(want);
        std::fs::create_dir_all(&dir)?;
        let dest = dir.join(local_bin_name());
        match entry.as_ref().and_then(|e| e.get(os_key())) {
            Some(os_entry) => {
                let rel = os_entry
                    .get("url")
                    .and_then(|u| u.as_str())
                    .with_context(|| format!("Version {want}: keine Download-URL im Manifest"))?;
                let expected =
                    os_entry.get("sha256").and_then(|s| s.as_str()).map(String::from);
                (dest, absolute_url(&base, rel), expected)
            }
            None if dest.exists() => {
                // Offline, but this version is already cached — use it.
                let _ = tx.send(Event::Log(format!(
                    "Version {want} aus dem lokalen Cache (offline)."
                )));
                return Ok(dest);
            }
            None => bail!(
                "Client-Version {want} ist im Download-Archiv nicht (mehr) verfügbar. \
                 Bitte in den Einstellungen „Neueste“ wählen."
            ),
        }
    };

    let have = if dest.exists() { sha256_file(&dest) } else { None };
    // Up to date only when we can prove the hash matches; if we're offline but
    // already have a copy, use it rather than failing the launch.
    let up_to_date = match (&expected, &have) {
        (Some(exp), Some(got)) => exp.eq_ignore_ascii_case(got),
        (None, Some(_)) => true,
        _ => false,
    };

    if up_to_date {
        let _ = tx.send(Event::Log("Client-Binary ist aktuell (SHA-256 geprüft).".into()));
    } else {
        if have.is_some() {
            let _ = tx.send(Event::Status(
                "Client-Version wird aktualisiert — wird geladen …".into(),
            ));
        } else if want.is_empty() {
            let _ = tx.send(Event::Status("DolphinClient-Client laden …".into()));
        } else {
            let _ = tx.send(Event::Status(format!("Client-Version {want} laden …")));
        }
        download_verified(client, &url, &dest, expected.as_deref())
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
/// be empty (the client then shows its own connect screen); `client_version`
/// pins an archived client build ("" = newest). The session is passed via
/// environment variables — never on the command line — so the access token
/// never appears in a process listing. Returns once the client started.
pub fn launch(
    session: &Session,
    server: &str,
    client_version: &str,
    tx: &Sender<Event>,
) -> Result<std::process::Child> {
    let client = game::http();

    // 1. Vanilla client jar — the only thing the native client needs from Mojang.
    let jar = game::ensure_client_jar(&client, tx)?;

    // 2. The tiny sound index + sounds.json (OGGs stream in from the client on
    //    demand). Best-effort: if it fails, the game just starts without sound.
    let sound = match game::ensure_sound_index(&client, tx) {
        Ok(pair) => Some(pair),
        Err(e) => {
            let _ = tx.send(Event::Log(format!("Sound-Index nicht verfügbar: {e:#}")));
            None
        }
    };

    // 3. The native client binary itself.
    let bin = ensure_client_bin(&client, tx, client_version)?;
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
    if let Some((assets, id)) = &sound {
        cmd.arg("--assets-dir").arg(assets).arg("--asset-index").arg(id);
        let _ = tx.send(Event::Log(format!("Sound aktiv (assets: {})", assets.display())));
    }
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
    let child = cmd
        .spawn()
        .with_context(|| format!("Client-Start fehlgeschlagen: {}", bin.display()))?;
    // `Event::Launched` + playtime tracking are handled by the caller, which
    // owns the returned child handle.
    Ok(child)
}
