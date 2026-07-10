//! Persistent launcher settings, playtime stats + well-known Minecraft paths.
//!
//! Everything the user changes in the launcher is saved **immediately** — there
//! is no "Save" button. `Settings::save()` is called by the UI on every change.

use std::path::PathBuf;

use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};

/// The Minecraft version DolphinClient targets.
pub const TARGET_VERSION: &str = "26.1";

/// Accent colours the user can pick for the launcher look.
pub const ACCENTS: &[(&str, [u8; 3])] = &[
    ("teal", [0x35, 0xE0, 0xC8]),
    ("blau", [0x5B, 0x8C, 0xFF]),
    ("violett", [0x9B, 0x7B, 0xFF]),
    ("pink", [0xFF, 0x6F, 0xB3]),
    ("grün", [0x53, 0xE0, 0x8B]),
    ("gold", [0xFF, 0xC4, 0x5A]),
];

/// RGB for the named accent (falls back to teal).
pub fn accent_rgb(name: &str) -> [u8; 3] {
    ACCENTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
        .unwrap_or(ACCENTS[0].1)
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
}

fn default_accent() -> String {
    "teal".to_string()
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

/// Lifetime playtime + launch stats, shown on the profile and exposed to the
/// web dashboard through the local bridge.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Stats {
    #[serde(default)]
    pub playtime_secs: u64,
    #[serde(default)]
    pub launches: u32,
    /// Unix epoch seconds of the last launch (None = never).
    #[serde(default)]
    pub last_played: Option<u64>,
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

    /// Record one finished session (seconds played) and persist.
    pub fn record_session(&mut self, secs: u64) {
        self.playtime_secs = self.playtime_secs.saturating_add(secs);
        self.save();
    }

    /// Record a launch (increments the counter, stamps `last_played`).
    pub fn record_launch(&mut self) {
        self.launches = self.launches.saturating_add(1);
        self.last_played = Some(now_unix());
        self.save();
    }
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
