//! Secure refresh-token storage via the OS credential store
//! (Windows Credential Manager / macOS Keychain / Linux kernel keyutils).
//! The token is never written to disk in plaintext.

use keyring::Entry;

const SERVICE: &str = "DolphinClient";
const USER: &str = "microsoft-refresh-token";

fn entry() -> Option<Entry> {
    Entry::new(SERVICE, USER).ok()
}

pub fn save_refresh(token: &str) {
    if let Some(e) = entry() {
        let _ = e.set_password(token);
    }
}

pub fn load_refresh() -> Option<String> {
    entry().and_then(|e| e.get_password().ok())
}

pub fn clear() {
    if let Some(e) = entry() {
        let _ = e.delete_credential();
    }
}

pub fn has_token() -> bool {
    load_refresh().is_some()
}
