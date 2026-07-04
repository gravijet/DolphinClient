//! Persistent launcher settings + well-known Minecraft paths.

use std::path::PathBuf;

use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};

/// The Minecraft version DolphinClient targets.
pub const TARGET_VERSION: &str = "26.1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    /// Allocated heap in GB (`-Xmx`).
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ram_gb: 4,
            java_path: String::new(),
            auto_update: true,
            fullscreen: false,
            server: String::new(),
        }
    }
}

fn project_dirs() -> Option<ProjectDirs> {
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
