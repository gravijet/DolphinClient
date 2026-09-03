//! Local worlds: a bundled [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin)
//! server binary (GPLv3, run unmodified as a separate subprocess — never
//! linked into DolphinClient, see `THIRD_PARTY_NOTICES.md`) spawned on
//! `127.0.0.1`, so "Singleplayer" is really "host a local server and connect
//! to it", without requiring a Java install.

use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::settings::GameSettings;

const PUMPKIN_TOML_TEMPLATE: &str = include_str!("../assets/pumpkin.toml.template");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gamemode {
    Survival,
    Creative,
    Adventure,
}

impl Gamemode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Survival => "Survival",
            Self::Creative => "Creative",
            Self::Adventure => "Adventure",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Survival => Self::Creative,
            Self::Creative => Self::Adventure,
            Self::Adventure => Self::Survival,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    pub fn label(self) -> &'static str {
        match self {
            Self::Peaceful => "Peaceful",
            Self::Easy => "Easy",
            Self::Normal => "Normal",
            Self::Hard => "Hard",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Peaceful => Self::Easy,
            Self::Easy => Self::Normal,
            Self::Normal => Self::Hard,
            Self::Hard => Self::Peaceful,
        }
    }
}

/// Sidecar metadata next to each world's `pumpkin.toml` — Pumpkin doesn't
/// need this, it's how the world-select screen lists worlds without booting
/// a server for each one.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldMeta {
    #[serde(skip)]
    pub id: String,
    pub display_name: String,
    pub seed: String,
    pub gamemode: Gamemode,
    pub difficulty: Difficulty,
    pub hardcore: bool,
    pub created_at: u64,
    pub last_played: u64,
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn saves_dir() -> PathBuf {
    GameSettings::config_dir().join("saves")
}

fn world_dir(id: &str) -> PathBuf {
    saves_dir().join(id)
}

fn meta_path(id: &str) -> PathBuf {
    world_dir(id).join("dolphin_world.json")
}

/// Turn a display name into a filesystem-safe id, disambiguating collisions.
fn slugify(display_name: &str) -> String {
    let base: String = display_name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let base = if base.is_empty() { "world".to_string() } else { base };
    let mut candidate = base.clone();
    let mut n = 1;
    while world_dir(&candidate).exists() {
        n += 1;
        candidate = format!("{base}-{n}");
    }
    candidate
}

/// List saved worlds, most-recently-played first.
pub fn list_worlds() -> Vec<WorldMeta> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(saves_dir()) else {
        return out;
    };
    for entry in entries.flatten() {
        let id = entry.file_name().to_string_lossy().to_string();
        let Ok(s) = std::fs::read_to_string(meta_path(&id)) else {
            continue;
        };
        let Ok(mut meta) = serde_json::from_str::<WorldMeta>(&s) else {
            continue;
        };
        meta.id = id;
        out.push(meta);
    }
    out.sort_by(|a, b| b.last_played.cmp(&a.last_played));
    out
}

/// Create a new world directory: `pumpkin.toml`, `eula.txt`, and the sidecar
/// metadata. The port is filled in fresh at every `spawn` (a saved world may
/// be replayed on a different free port each session).
pub fn create_world(
    display_name: &str,
    seed: &str,
    gamemode: Gamemode,
    difficulty: Difficulty,
    hardcore: bool,
) -> Result<String> {
    let id = slugify(display_name);
    let dir = world_dir(&id);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(dir.join("eula.txt"), "eula=true\n").context("writing eula.txt")?;

    let name = if display_name.trim().is_empty() { "New World" } else { display_name.trim() };
    let meta = WorldMeta {
        id: id.clone(),
        display_name: name.to_string(),
        seed: seed.trim().to_string(),
        gamemode,
        difficulty: if hardcore { Difficulty::Hard } else { difficulty },
        hardcore,
        created_at: now_millis(),
        last_played: now_millis(),
    };
    write_meta(&dir, &meta)?;
    write_config(&dir, &meta, 0)?;
    Ok(id)
}

pub fn delete_world(id: &str) -> Result<()> {
    let dir = world_dir(id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("deleting {}", dir.display()))?;
    }
    Ok(())
}

fn write_meta(dir: &Path, meta: &WorldMeta) -> Result<()> {
    let s = serde_json::to_string_pretty(meta)?;
    std::fs::write(dir.join("dolphin_world.json"), s)?;
    Ok(())
}

/// (Re)write `pumpkin.toml` for this world with a specific port. Called once
/// right before every spawn, so a world can move to a fresh free port even if
/// the previous session's port is now taken.
fn write_config(dir: &Path, meta: &WorldMeta, port: u16) -> Result<()> {
    let seed = if meta.seed.trim().is_empty() {
        // Pumpkin accepts a numeric or string seed; a random u64 as text
        // matches what it would have generated itself for an empty seed.
        (uuid::Uuid::new_v4().as_u128() as u64).to_string()
    } else {
        meta.seed.trim().to_string()
    };
    let toml = PUMPKIN_TOML_TEMPLATE
        .replace("__SEED__", &seed)
        .replace("__DIFFICULTY__", meta.difficulty.label())
        .replace("__HARDCORE__", if meta.hardcore { "true" } else { "false" })
        .replace("__GAMEMODE__", meta.gamemode.label())
        .replace("__LEVEL_NAME__", "world")
        .replace("__PORT__", &port.to_string());
    std::fs::write(dir.join("pumpkin.toml"), toml)?;
    Ok(())
}

/// Bind an ephemeral local port and immediately release it. There is a small
/// window where another process could grab it before Pumpkin binds — fine
/// for a single local user starting one world at a time.
pub fn find_free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("binding an ephemeral port")?;
    Ok(listener.local_addr()?.port())
}

/// Locate the bundled server binary: an explicit path, else `.mc-cache/` (dev
/// convenience, searched upward from CWD), else next to the running exe
/// (the installed layout).
pub fn find_server_binary(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit
        && p.is_file()
    {
        return Some(p.to_path_buf());
    }
    let name = if cfg!(target_os = "windows") { "pumpkin-server.exe" } else { "pumpkin-server" };
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(".mc-cache").join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join(name);
    candidate.is_file().then_some(candidate)
}

/// A running local server. Dropping this without calling `shutdown` first
/// still best-effort kills the process — never leaves an orphan behind.
pub struct SingleplayerServer {
    child: Child,
    port: u16,
    shut_down: bool,
}

impl SingleplayerServer {
    /// Write a fresh `pumpkin.toml` (picking a new free port) and spawn the
    /// bundled binary with the world directory as its working directory —
    /// that's how Pumpkin finds its config and save data.
    pub fn spawn(binary: &Path, world_id: &str) -> Result<Self> {
        let dir = world_dir(world_id);
        let s = std::fs::read_to_string(meta_path(world_id))
            .with_context(|| format!("reading world metadata for {world_id}"))?;
        let mut meta: WorldMeta = serde_json::from_str(&s).context("parsing world metadata")?;
        meta.id = world_id.to_string();
        meta.last_played = now_millis();
        write_meta(&dir, &meta)?;

        let port = find_free_port()?;
        write_config(&dir, &meta, port)?;

        let child = Command::new(binary)
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawning {}", binary.display()))?;
        Ok(Self { child, port, shut_down: false })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// Non-blocking readiness check: is the Java port accepting connections
    /// yet? Polled once per frame from the "Starting world…" screen.
    pub fn is_ready(&self) -> bool {
        TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", self.port).parse().expect("valid loopback addr"),
            Duration::from_millis(150),
        )
        .is_ok()
    }

    /// True if the child process has exited on its own (crashed, or already
    /// stopped) — checked so a dead server doesn't spin the "Starting…"
    /// screen forever waiting for a port that will never open.
    pub fn has_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    fn send_stop(&mut self) {
        self.shut_down = true;
        if let Some(stdin) = self.child.stdin.as_mut() {
            let _ = stdin.write_all(b"stop\n");
            let _ = stdin.flush();
        }
    }

    /// Give the process a moment to flush the world to disk after `stop`,
    /// then kill it outright if it hasn't exited by itself.
    fn wait_or_kill(&mut self, timeout: Duration) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                _ => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Ask Pumpkin to stop, then hand the wait-and-kill off to a background
    /// thread so the caller (the UI thread) never blocks on it — used when
    /// returning to the title screen while the app keeps running.
    pub fn shutdown_async(mut self) {
        self.send_stop();
        std::thread::spawn(move || self.wait_or_kill(Duration::from_secs(10)));
    }

    /// Ask Pumpkin to stop and wait for it, blocking the caller. Only used
    /// right before the whole app exits — a short pause there is expected
    /// (the same way vanilla shows "Saving world" on quit), and it's the only
    /// way to guarantee the process is gone before the app's own process
    /// tree disappears (a detached background thread would be killed too).
    pub fn shutdown_blocking(mut self) {
        self.send_stop();
        self.wait_or_kill(Duration::from_secs(10));
    }
}

impl Drop for SingleplayerServer {
    fn drop(&mut self) {
        if !self.shut_down {
            let _ = self.child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_sanitizes_and_disambiguates() {
        assert_eq!(slugify(""), "world");
        assert!(slugify("My World!").starts_with("My_World_"));
    }
}

