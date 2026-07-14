//! A custom azalea [`Account`] backed by a Minecraft session the launcher
//! already obtained (username + UUID + Minecraft access token). It lets the
//! native client join online-mode servers without running its own Microsoft
//! login: the launcher does the Microsoft/Xbox/Minecraft handshake once and
//! hands the resulting session over (see `bridge::events::AccountConfig::Session`).

use std::future::Future;
use std::pin::Pin;

use azalea::account::{Account, AccountTrait};
use azalea::auth::certs::Certificates;
use azalea::auth::sessionserver::{self, ClientSessionServerError, SessionServerJoinOpts};
use parking_lot::Mutex;
use uuid::Uuid;

/// Minecraft session forwarded by the launcher.
pub struct SessionAccount {
    username: String,
    uuid: Uuid,
    access_token: String,
    /// Chat-signing certificates, fetched and stored by azalea's
    /// ChatSigningPlugin. Without this storage the plugin's `set_certs` is a
    /// no-op and azalea later panics on `certs().expect(...)` when sending a
    /// signed (normal, non-command) chat message — the whole schedule loop
    /// died and the game froze. Mirrors azalea's own MicrosoftAccount.
    certs: Mutex<Option<Certificates>>,
}

impl std::fmt::Debug for SessionAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionAccount")
            .field("username", &self.username)
            .field("uuid", &self.uuid)
            .finish_non_exhaustive()
    }
}

impl SessionAccount {
    /// Build an [`Account`] from a launcher session. `uuid` is the undashed or
    /// dashed Minecraft profile id; parsing failures fall back to nil (which
    /// only matters for offline servers, where the token is unused anyway).
    pub fn account(username: String, uuid: &str, access_token: String) -> Account {
        let uuid = Uuid::parse_str(uuid).unwrap_or(Uuid::nil());
        Account::from(SessionAccount {
            username,
            uuid,
            access_token,
            certs: Mutex::new(None),
        })
    }
}

impl AccountTrait for SessionAccount {
    fn username(&self) -> &str {
        &self.username
    }

    fn uuid(&self) -> Uuid {
        self.uuid
    }

    fn access_token(&self) -> Option<String> {
        Some(self.access_token.clone())
    }

    fn certs(&self) -> Option<Certificates> {
        self.certs.lock().as_ref().cloned()
    }

    fn set_certs(&self, certs: Certificates) {
        *self.certs.lock() = Some(certs);
    }

    // The launcher token is short-lived but the launcher refreshes it on every
    // start, so we don't refresh here — the default no-op impl is inherited.

    fn join<'a>(
        &'a self,
        public_key: &'a [u8],
        private_key: &'a [u8; 16],
        server_id: &'a str,
        proxy: Option<reqwest::Proxy>,
    ) -> Pin<Box<dyn Future<Output = Result<(), ClientSessionServerError>> + Send + 'a>> {
        Box::pin(async move {
            sessionserver::join(SessionServerJoinOpts {
                access_token: &self.access_token,
                public_key,
                private_key,
                uuid: &self.uuid,
                server_id,
                proxy,
            })
            .await
        })
    }
}
