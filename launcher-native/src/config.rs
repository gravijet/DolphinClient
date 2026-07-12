//! Persistent launcher settings, playtime stats + well-known Minecraft paths.
//!
//! Everything the user changes in the launcher is saved **immediately** — there
//! is no "Save" button. `Settings::save()` is called by the UI on every change.

use std::path::PathBuf;

use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};

/// The Minecraft version DolphinClient targets.
pub const TARGET_VERSION: &str = "26.1";

/// Accent colours the user can pick for the launcher look. "aqua" is the
/// signature DolphinClient accent and matches the website's "Prism" theme.
/// The launcher now ships a single fixed accent; kept for settings compat.
#[allow(dead_code)]
pub const ACCENTS: &[(&str, [u8; 3])] = &[
    ("aqua", [0x34, 0xE6, 0xD6]),
    ("blau", [0x37, 0xA7, 0xFF]),
    ("violett", [0x8A, 0x5C, 0xFF]),
    ("pink", [0xFF, 0x6A, 0xD5]),
    ("teal", [0x35, 0xE0, 0xC8]),
    ("grün", [0x4D, 0xE3, 0xA4]),
    ("gold", [0xF0, 0xB2, 0x3C]),
];

/// RGB for the named accent (falls back to the signature ocean blue).
#[allow(dead_code)]
pub fn accent_rgb(name: &str) -> [u8; 3] {
    ACCENTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
        .unwrap_or(ACCENTS[0].1)
}

/// A saved server the launcher can quick-join. Stored in [`Settings::servers`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerEntry {
    /// Friendly name shown in the list.
    pub name: String,
    /// `host` or `host:port` the client connects to.
    pub address: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    /// Allocated heap in GB (`-Xmx`, only used for the legacy Java fallback).
    pub ram_gb: u32,
    /// Path to the `java` binary. Empty → use `java` from `PATH`.
    pub java_path: String,
    /// Check for launcher updates on start.
    pub auto_update: bool,
    /// Launch the game in fullscreen.
    pub fullscreen: bool,
    /// Default server the native client joins on launch. Empty → the client
    /// opens its own connect screen so the player can pick a server.
    #[serde(default)]
    pub server: String,
    /// Pinned client version ("" = always the newest). Older versions come
    /// from the `clientVersions` archive in the published manifest.
    #[serde(default)]
    pub client_version: String,
    /// Launcher accent colour (see [`ACCENTS`]).
    #[serde(default = "default_accent")]
    pub accent: String,
    /// Close the launcher window once the game has started.
    #[serde(default)]
    pub close_on_launch: bool,
    /// Selected cape id ("" = none). Shown on the profile; in-game rendering
    /// is on the roadmap.
    #[serde(default)]
    pub cape: String,
    /// Saved servers the launcher can quick-join. The `server` field above is
    /// the currently-selected default address (kept in sync on selection).
    #[serde(default)]
    pub servers: Vec<ServerEntry>,
    /// Broadcast a Discord Rich Presence while the launcher is open.
    #[serde(default = "default_true")]
    pub discord_rpc: bool,
    /// Register DolphinClient to open automatically when the user signs in.
    #[serde(default)]
    pub autostart: bool,
    /// Install a found launcher update automatically, without waiting for a
    /// click (the launcher downloads it and restarts itself).
    #[serde(default = "default_true")]
    pub auto_update_apply: bool,
}

fn default_true() -> bool {
    true
}

fn default_accent() -> String {
    "aqua".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ram_gb: 4,
            java_path: String::new(),
            auto_update: true,
            fullscreen: false,
            server: String::new(),
            client_version: String::new(),
            accent: default_accent(),
            close_on_launch: false,
            cape: String::new(),
            servers: Vec::new(),
            discord_rpc: true,
            autostart: false,
            auto_update_apply: true,
        }
    }
}

fn project_dirs() -> Option<ProjectDirs> {
    // Keep the historical qualifier so existing users' accounts/settings are
    // found after an update — this is a filesystem key, not the web domain.
    ProjectDirs::from("net", "DolphinClient", "DolphinClient")
}

/// Where launcher config lives (per-user, per-OS).
pub fn config_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// Per-user data dir for launcher-managed files (e.g. the native client binary).
pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(settings_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(config_dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(settings_path(), s);
        }
    }

    /// Resolved Java binary (`java` if none configured).
    pub fn java_bin(&self) -> String {
        let p = self.java_path.trim();
        if p.is_empty() {
            "java".to_string()
        } else {
            p.to_string()
        }
    }
}

/// One finished play session: when it started and how long it lasted.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    /// Unix epoch seconds the session started.
    pub at: u64,
    /// Duration in seconds.
    pub secs: u64,
}

/// How many recent sessions to keep for the history sparkline.
pub const SESSION_HISTORY: usize = 30;

/// Lifetime playtime + launch stats, shown on the profile.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Stats {
    #[serde(default)]
    pub playtime_secs: u64,
    #[serde(default)]
    pub launches: u32,
    /// Unix epoch seconds of the last launch (None = never).
    #[serde(default)]
    pub last_played: Option<u64>,
    /// Recent sessions (oldest first), capped to [`SESSION_HISTORY`]. Powers the
    /// launcher's playtime sparkline.
    #[serde(default)]
    pub sessions: Vec<Session>,
}

/// Current wall-clock time as Unix epoch seconds.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn stats_path() -> PathBuf {
    config_dir().join("stats.json")
}

impl Stats {
    pub fn load() -> Self {
        std::fs::read_to_string(stats_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(config_dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(stats_path(), s);
        }
    }

    /// Record one finished session (seconds played) and persist. Sessions
    /// shorter than 3s are treated as a failed launch and not charted, but they
    /// still count toward total playtime.
    pub fn record_session(&mut self, secs: u64) {
        self.playtime_secs = self.playtime_secs.saturating_add(secs);
        if secs >= 3 {
            self.sessions.push(Session { at: now_unix(), secs });
            let overflow = self.sessions.len().saturating_sub(SESSION_HISTORY);
            if overflow > 0 {
                self.sessions.drain(0..overflow);
            }
        }
        self.save();
    }

    /// Average session length in seconds over the recorded history (0 if none).
    #[allow(dead_code)]
    pub fn avg_session_secs(&self) -> u64 {
        if self.sessions.is_empty() {
            return 0;
        }
        let total: u64 = self.sessions.iter().map(|s| s.secs).sum();
        total / self.sessions.len() as u64
    }

    /// Record a launch (increments the counter, stamps `last_played`).
    pub fn record_launch(&mut self) {
        self.launches = self.launches.saturating_add(1);
        self.last_played = Some(now_unix());
        self.save();
    }
}

/// Path to the **native client's** `options.json`, mirroring the client's own
/// `GameSettings::config_dir` logic. NOTE: this is deliberately *not* the same
/// as the launcher's [`config_dir`] — on Windows the client uses
/// `%APPDATA%\DolphinClient` while the launcher uses a nested ProjectDirs path.
/// The launcher pre-writes a subset of these options so the client picks them
/// up on its next start (see `gameopts`).
pub fn client_options_path() -> PathBuf {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("DolphinClient")
        .join("options.json")
}

/// The `.minecraft` directory for the current OS.
pub fn minecraft_dir() -> PathBuf {
    let base = BaseDirs::new();
    match base {
        Some(b) => {
            if cfg!(target_os = "windows") {
                // %APPDATA%\.minecraft
                b.data_dir().join(".minecraft")
            } else if cfg!(target_os = "macos") {
                // ~/Library/Application Support/minecraft
                b.data_dir().join("minecraft")
            } else {
                // ~/.minecraft
                b.home_dir().join(".minecraft")
            }
        }
        None => PathBuf::from(".minecraft"),
    }
}
