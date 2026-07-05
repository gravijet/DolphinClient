//! Secure refresh-token storage via the OS credential store
//! (Windows Credential Manager / macOS Keychain / Linux kernel keyutils).
//! The token is never written to disk in plaintext.

use keyring::Entry;

const SERVICE: &str = "DolphinClient";
/// Legacy single-account key (pre-multi-account). Still read for migration.
const USER: &str = "microsoft-refresh-token";

fn entry_for(user: &str) -> Option<Entry> {
    Entry::new(SERVICE, user).ok()
}

fn entry() -> Option<Entry> {
    entry_for(USER)
}

fn set(user: &str, token: &str) {
    if let Some(e) = entry_for(user) {
        let _ = e.set_password(token);
    }
}

fn get(user: &str) -> Option<String> {
    entry_for(user).and_then(|e| e.get_password().ok())
}

fn del(user: &str) {
    if let Some(e) = entry_for(user) {
        let _ = e.delete_credential();
    }
}

// -- legacy single-account API (kept for migration) --------------------------

pub fn save_refresh(token: &str) {
    set(USER, token);
}

pub fn load_refresh() -> Option<String> {
    entry().and_then(|e| e.get_password().ok())
}

pub fn clear() {
    del(USER);
}

pub fn has_token() -> bool {
    load_refresh().is_some()
}

// -- per-account API ---------------------------------------------------------

/// Renewable Microsoft refresh token for one account (our own login).
pub fn save_refresh_for(uuid: &str, token: &str) {
    set(&format!("refresh:{uuid}"), token);
}

pub fn load_refresh_for(uuid: &str) -> Option<String> {
    get(&format!("refresh:{uuid}"))
}

/// Last known Minecraft access token for one account (used to launch, and the
/// only credential we have for imported accounts).
pub fn save_access_for(uuid: &str, token: &str) {
    set(&format!("access:{uuid}"), token);
}

pub fn load_access_for(uuid: &str) -> Option<String> {
    get(&format!("access:{uuid}"))
}

/// Wipe every stored secret for one account.
pub fn clear_for(uuid: &str) {
    del(&format!("refresh:{uuid}"));
    del(&format!("access:{uuid}"));
}
