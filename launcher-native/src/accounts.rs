//! Multi-account support.
//!
//! Account *metadata* (uuid, name, origin) is persisted to `accounts.json` in
//! the launcher config dir. *Secrets* (Microsoft refresh tokens and the last
//! known Minecraft access token) live in the OS credential store, keyed per
//! account — never in the JSON. See [`crate::tokens`].
//!
//! Accounts can also be **imported** from other Minecraft launchers already
//! installed on this device (the official launcher, Lunar Client, …). Those
//! launchers keep the signed-in account's Minecraft *access token* in a plain
//! JSON file; we read our own device's files, copy the token, and can play
//! immediately — no re-login. Launchers that encrypt their token store
//! (Badlion, Feather) can't be imported and are skipped.

use std::path::PathBuf;

use anyhow::{bail, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::config;

/// One signed-in / imported account.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub uuid: String,
    pub username: String,
    /// Where this account came from ("Microsoft", "Vanilla Launcher", …).
    #[serde(default)]
    pub source: String,
    /// True when a renewable Microsoft refresh token is stored (our own login).
    /// Imported accounts have only a short-lived access token → false.
    #[serde(default)]
    pub has_refresh: bool,
    /// Offline profiles never contact Microsoft and only work on servers whose
    /// owner deliberately enabled Minecraft's offline mode.
    #[serde(default)]
    pub offline: bool,
    /// Client-local cosmetics. These files are never uploaded and therefore
    /// only affect what this installation renders for its own player.
    #[serde(default)]
    pub skin_path: String,
    #[serde(default)]
    pub cape_path: String,
    /// `false` = classic/Steve arms, `true` = slim/Alex arms.
    #[serde(default)]
    pub skin_slim: bool,
}

impl Account {
    /// Build a Vanilla-compatible offline profile. Java's
    /// `UUID.nameUUIDFromBytes("OfflinePlayer:<name>")` is raw MD5 with UUIDv3
    /// bits (there is deliberately no namespace UUID involved).
    pub fn offline(username: &str) -> Result<Self> {
        validate_offline_name(username)?;
        let username = username.trim().to_string();
        Ok(Self {
            uuid: offline_uuid(&username),
            username,
            source: "Offline profile".to_string(),
            has_refresh: false,
            offline: true,
            skin_path: String::new(),
            cape_path: String::new(),
            skin_slim: false,
        })
    }
}

/// Minecraft usernames are 3–16 ASCII letters, digits or underscores. Keeping
/// this identical to Vanilla prevents profiles that servers can never accept.
pub fn validate_offline_name(name: &str) -> Result<()> {
    let name = name.trim();
    if !(3..=16).contains(&name.len()) {
        bail!("Offline names must be 3 to 16 characters long.");
    }
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        bail!("Offline names may only contain letters, numbers and underscores.");
    }
    Ok(())
}

pub fn offline_uuid(name: &str) -> String {
    let mut bytes = md5::compute(format!("OfflinePlayer:{name}")).0;
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h = bytes.map(|b| format!("{b:02x}")).concat();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Persistent list of accounts + which one is active.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountStore {
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// UUID of the active account (the one that plays).
    #[serde(default)]
    pub active: Option<String>,
}

fn store_path() -> PathBuf {
    config::config_dir().join("accounts.json")
}

impl AccountStore {
    pub fn load() -> Self {
        std::fs::read_to_string(store_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(config::config_dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(store_path(), s);
        }
    }

    pub fn active_account(&self) -> Option<&Account> {
        let id = self.active.as_ref()?;
        self.accounts.iter().find(|a| &a.uuid == id)
    }

    pub fn is_active(&self, uuid: &str) -> bool {
        self.active.as_deref() == Some(uuid)
    }

    /// Insert or update an account (matched by uuid). Makes it active if it's
    /// the first account. Persists.
    pub fn upsert(&mut self, account: Account) {
        let uuid = account.uuid.clone();
        if let Some(existing) = self.accounts.iter_mut().find(|a| a.uuid == uuid) {
            existing.username = account.username;
            existing.source = account.source;
            existing.has_refresh = existing.has_refresh || account.has_refresh;
            existing.offline = account.offline;
            // Re-authentication/import refreshes identity metadata, never the
            // user's private per-profile cosmetic choices.
        } else {
            self.accounts.push(account);
        }
        if self.active.is_none() {
            self.active = Some(uuid);
        }
        self.save();
    }

    pub fn set_active(&mut self, uuid: &str) {
        if self.accounts.iter().any(|a| a.uuid == uuid) {
            self.active = Some(uuid.to_string());
            self.save();
        }
    }

    /// Remove an account and wipe its stored secrets. Re-points `active` if it
    /// removed the active one.
    pub fn remove(&mut self, uuid: &str) {
        self.accounts.retain(|a| a.uuid != uuid);
        crate::tokens::clear_for(uuid);
        if self.active.as_deref() == Some(uuid) {
            self.active = self.accounts.first().map(|a| a.uuid.clone());
        }
        self.save();
    }
}

/// An account discovered in another launcher's config on this device.
pub struct Imported {
    pub uuid: String,
    pub username: String,
    pub access_token: String,
    pub source: String,
}

/// Normalize a bare-hex uuid (32 chars) into canonical 8-4-4-4-12 form; leave
/// already-hyphenated ids untouched.
fn dash_uuid(id: &str) -> String {
    let hex: String = id.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() == 32 {
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    } else {
        id.to_string()
    }
}

/// The on-disk JSON layout a launcher uses for its stored accounts.
#[derive(Clone, Copy)]
enum Shape {
    /// Mojang / Lunar: `{ "accounts": { "<id>": { "accessToken", "minecraftProfile": { "id", "name" } } } }`.
    Mojang,
    /// Prism / PolyMC / MultiMC: `{ "accounts": [ { "profile": { "id", "name" }, "ygg": { "token" } } ] }`.
    Prism,
}

/// Parse a launcher `accounts.json` that uses the Mojang shape.
fn parse_mojang_shape(text: &str, source: &str, out: &mut Vec<Imported>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let Some(map) = v.get("accounts").and_then(|a| a.as_object()) else {
        return;
    };
    for entry in map.values() {
        let token = entry.get("accessToken").and_then(|t| t.as_str());
        let profile = entry.get("minecraftProfile");
        let id = profile.and_then(|p| p.get("id")).and_then(|i| i.as_str());
        let name = profile
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .or_else(|| entry.get("username").and_then(|u| u.as_str()));
        if let (Some(token), Some(id), Some(name)) = (token, id, name) {
            if token.is_empty() || id.is_empty() {
                continue;
            }
            out.push(Imported {
                uuid: dash_uuid(id),
                username: name.to_string(),
                access_token: token.to_string(),
                source: source.to_string(),
            });
        }
    }
}

/// Parse a Prism/PolyMC/MultiMC `accounts.json` (accounts as an array, each with
/// a `profile` and the Minecraft access token under `ygg.token`).
fn parse_prism_shape(text: &str, source: &str, out: &mut Vec<Imported>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let Some(list) = v.get("accounts").and_then(|a| a.as_array()) else {
        return;
    };
    for entry in list {
        let profile = entry.get("profile");
        let id = profile.and_then(|p| p.get("id")).and_then(|i| i.as_str());
        let name = profile.and_then(|p| p.get("name")).and_then(|n| n.as_str());
        let token = entry
            .get("ygg")
            .and_then(|y| y.get("token"))
            .and_then(|t| t.as_str());
        if let (Some(token), Some(id), Some(name)) = (token, id, name) {
            if token.is_empty() || id.is_empty() {
                continue;
            }
            out.push(Imported {
                uuid: dash_uuid(id),
                username: name.to_string(),
                access_token: token.to_string(),
                source: source.to_string(),
            });
        }
    }
}

/// Candidate account files from launchers installed on this device, with the
/// JSON shape each one uses. Launchers that encrypt their token store (Feather,
/// Badlion) can't be read and are intentionally absent; LabyMod signs in through
/// the official launcher, so it is covered by the Vanilla entry.
fn import_sources() -> Vec<(PathBuf, &'static str, Shape)> {
    let mut sources = Vec::new();
    // Official Minecraft launcher (also covers LabyMod on most setups).
    sources.push((
        config::minecraft_dir().join("launcher_accounts.json"),
        "Vanilla Launcher",
        Shape::Mojang,
    ));
    if let Some(base) = BaseDirs::new() {
        let home = base.home_dir();
        let data = base.data_dir();
        // Lunar Client (same path on every OS).
        sources.push((
            home.join(".lunarclient/settings/game/accounts.json"),
            "Lunar Client",
            Shape::Mojang,
        ));
        // Prism / PolyMC / MultiMC family (OS-correct data dir).
        sources.push((
            data.join("PrismLauncher/accounts.json"),
            "Prism Launcher",
            Shape::Prism,
        ));
        sources.push((data.join("PolyMC/accounts.json"), "PolyMC", Shape::Prism));
        sources.push((data.join("multimc/accounts.json"), "MultiMC", Shape::Prism));
    }
    sources
}

/// Scan installed launchers for signed-in accounts. Deduplicates by uuid.
pub fn discover() -> Vec<Imported> {
    let mut out: Vec<Imported> = Vec::new();
    for (path, source, shape) in import_sources() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            match shape {
                Shape::Mojang => parse_mojang_shape(&text, source, &mut out),
                Shape::Prism => parse_prism_shape(&text, source, &mut out),
            }
        }
    }
    // Dedup by uuid, keeping the first (highest-priority) source.
    let mut seen = std::collections::HashSet::new();
    out.retain(|i| seen.insert(i.uuid.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_profile_matches_vanilla_uuid() {
        assert_eq!(
            offline_uuid("Notch"),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert!(validate_offline_name("Dolphin_26").is_ok());
        assert!(validate_offline_name("no spaces").is_err());
        assert!(validate_offline_name("xy").is_err());
    }

    #[test]
    fn dash_uuid_inserts_hyphens() {
        assert_eq!(
            dash_uuid("069a79f444e94726a5befca90e38aaf5"),
            "069a79f4-44e9-4726-a5be-fca90e38aaf5"
        );
        // Already hyphenated → unchanged.
        let hy = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
        assert_eq!(dash_uuid(hy), hy);
    }

    #[test]
    fn parses_mojang_shape() {
        let text = r#"{
            "accounts": {
                "abc": {
                    "accessToken": "TOKEN123",
                    "minecraftProfile": { "id": "069a79f444e94726a5befca90e38aaf5", "name": "Notch" },
                    "username": "notch@example.com"
                }
            },
            "activeAccountLocalId": "abc"
        }"#;
        let mut out = Vec::new();
        parse_mojang_shape(text, "Vanilla Launcher", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].username, "Notch");
        assert_eq!(out[0].access_token, "TOKEN123");
        assert_eq!(out[0].uuid, "069a79f4-44e9-4726-a5be-fca90e38aaf5");
    }

    #[test]
    fn skips_entries_without_token_or_profile() {
        let text = r#"{ "accounts": { "x": { "username": "no-profile" } } }"#;
        let mut out = Vec::new();
        parse_mojang_shape(text, "Vanilla Launcher", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn parses_prism_shape() {
        let text = r#"{
            "accounts": [
                {
                    "profile": { "id": "069a79f444e94726a5befca90e38aaf5", "name": "Notch" },
                    "type": "MSA",
                    "ygg": { "token": "PRISMTOKEN", "iat": 1 }
                },
                { "profile": { "name": "no-token" } }
            ],
            "formatVersion": 3
        }"#;
        let mut out = Vec::new();
        parse_prism_shape(text, "Prism Launcher", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].username, "Notch");
        assert_eq!(out[0].access_token, "PRISMTOKEN");
        assert_eq!(out[0].uuid, "069a79f4-44e9-4726-a5be-fca90e38aaf5");
        assert_eq!(out[0].source, "Prism Launcher");
    }
}
