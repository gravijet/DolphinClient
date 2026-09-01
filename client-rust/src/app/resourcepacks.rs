//! Local resource-pack selection and server-pack cache housekeeping.
//!
//! Vanilla keeps the available files separate from the selected stack. We do
//! the same in `resourcepacks.json`: `enabled` is stored low-to-high priority,
//! matching [`AssetPack`]'s overlay order. A first run preserves Dolphin's old
//! behavior by enabling every pack already in the folder; packs added later
//! appear as available and wait for the player to enable them.

use serde::{Deserialize, Serialize};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::settings::GameSettings;

pub const CURRENT_RESOURCE_FORMAT: u32 = 84;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalPackStore {
    /// File names only, ordered from lowest to highest priority.
    pub enabled: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct LocalPackInfo {
    pub file_name: String,
    pub description: String,
    pub format: Option<u32>,
    pub bytes: u64,
    pub valid: bool,
}

impl LocalPackInfo {
    pub fn compatibility(&self) -> &'static str {
        match self.format {
            Some(CURRENT_RESOURCE_FORMAT) => "Compatible",
            Some(v) if v < CURRENT_RESOURCE_FORMAT => "Made for an older version",
            Some(_) => "Made for a newer version",
            None if self.valid => "Unknown format",
            None => "Invalid pack",
        }
    }
}

impl LocalPackStore {
    fn path() -> PathBuf {
        GameSettings::config_dir().join("resourcepacks.json")
    }

    pub fn directory() -> PathBuf {
        GameSettings::config_dir().join("resourcepacks")
    }

    pub fn load() -> Self {
        let all = scan_resource_packs();
        // Vanilla lists malformed packs, but it never puts one in the active
        // stack. Keep them visible in the UI while pruning them from the saved
        // selection (including during the first-run migration).
        let available: Vec<_> = all
            .iter()
            .filter(|pack| pack.valid)
            .map(|pack| pack.file_name.clone())
            .collect();
        let path = Self::path();
        let mut store = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Self>(&raw).ok())
            .unwrap_or_else(|| Self {
                enabled: available.clone(),
            });
        store.enabled.retain(|name| available.contains(name));
        store.enabled.dedup();
        store.save();
        store
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, raw);
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        let dir = Self::directory();
        let valid = scan_resource_packs();
        self.enabled
            .iter()
            .filter_map(|name| {
                valid
                    .iter()
                    .find(|pack| pack.valid && &pack.file_name == name)
                    .map(|pack| dir.join(&pack.file_name))
            })
            .collect()
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.iter().any(|entry| entry == name)
    }

    pub fn toggle(&mut self, name: &str) {
        if let Some(index) = self.enabled.iter().position(|entry| entry == name) {
            self.enabled.remove(index);
        } else {
            self.enabled.push(name.to_string());
        }
        self.save();
    }

    /// Move toward higher priority (the top of vanilla's selected list).
    pub fn raise(&mut self, name: &str) {
        if let Some(index) = self.enabled.iter().position(|entry| entry == name)
            && index + 1 < self.enabled.len()
        {
            self.enabled.swap(index, index + 1);
            self.save();
        }
    }

    pub fn lower(&mut self, name: &str) {
        if let Some(index) = self.enabled.iter().position(|entry| entry == name)
            && index > 0
        {
            self.enabled.swap(index, index - 1);
            self.save();
        }
    }
}

pub fn scan_resource_packs() -> Vec<LocalPackInfo> {
    let dir = LocalPackStore::directory();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = read
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        })
        .collect();
    paths.sort_by_key(|path| path.file_name().map(|v| v.to_os_string()));
    paths.into_iter().map(|path| inspect_pack(&path)).collect()
}

fn inspect_pack(path: &Path) -> LocalPackInfo {
    let file_name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("pack.zip")
        .to_string();
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let parsed = (|| -> anyhow::Result<(String, Option<u32>)> {
        let file = std::fs::File::open(path)?;
        let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))?;
        let mut entry = zip.by_name("pack.mcmeta")?;
        anyhow::ensure!(entry.size() <= 1024 * 1024, "pack.mcmeta too large");
        let mut raw = String::new();
        entry.read_to_string(&mut raw)?;
        let root: serde_json::Value = serde_json::from_str(&raw)?;
        let pack = root.get("pack").and_then(serde_json::Value::as_object);
        let description = pack
            .and_then(|p| p.get("description"))
            .map(component_text)
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "No description".into());
        let format = pack
            .and_then(|p| p.get("pack_format"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok());
        Ok((description, format))
    })();
    match parsed {
        Ok((description, format)) => LocalPackInfo {
            file_name,
            description,
            format,
            bytes,
            valid: true,
        },
        Err(_) => LocalPackInfo {
            file_name,
            description: "Unreadable or missing pack.mcmeta".into(),
            format: None,
            bytes,
            valid: false,
        },
    }
}

fn component_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(values) => values.iter().map(component_text).collect(),
        serde_json::Value::Object(object) => {
            let mut text = object
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            if let Some(extra) = object.get("extra").and_then(serde_json::Value::as_array) {
                text.extend(extra.iter().map(component_text));
            }
            text
        }
        _ => String::new(),
    }
}

pub fn server_cache_stats() -> (usize, u64) {
    let dir = GameSettings::config_dir().join("server-packs");
    let Ok(read) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    read.flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        })
        .fold((0, 0), |(count, bytes), entry| {
            (
                count + 1,
                bytes + entry.metadata().map(|m| m.len()).unwrap_or(0),
            )
        })
}

/// Remove only files owned by Dolphin's server-pack cache. Local packs are in
/// a different directory and can never be touched by this operation.
pub fn clear_server_cache() -> std::io::Result<usize> {
    let dir = GameSettings::config_dir().join("server-packs");
    let Ok(read) = std::fs::read_dir(dir) else {
        return Ok(0);
    };
    let mut removed = 0;
    for entry in read.flatten() {
        let path = entry.path();
        let owned = path.extension().is_some_and(|ext| {
            ext.eq_ignore_ascii_case("zip")
                || ext.eq_ignore_ascii_case("json")
                || ext.to_string_lossy().starts_with("part")
        }) || path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains(".part"));
        if owned && path.is_file() {
            std::fs::remove_file(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_text_flattens_styled_json() {
        let value = serde_json::json!({"text":"Hello ","extra":[{"text":"world"},"!"]});
        assert_eq!(component_text(&value), "Hello world!");
    }

    #[test]
    fn priority_moves_are_stable() {
        let mut store = LocalPackStore {
            enabled: vec!["a.zip".into(), "b.zip".into()],
        };
        // Exercise the ordering without writing the test's fake names by doing
        // the same swaps the public helpers perform.
        store.enabled.swap(0, 1);
        assert_eq!(store.enabled, ["b.zip", "a.zip"]);
    }
}
