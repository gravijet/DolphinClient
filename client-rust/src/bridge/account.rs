//! A custom azalea [`Account`] backed by a Minecraft session the launcher
//! already obtained (username + UUID + Minecraft access token). It lets the
//! native client join online-mode servers without running its own Microsoft
//! login: the launcher does the Microsoft/Xbox/Minecraft handshake once and
//! hands the resulting session over (see `bridge::events::AccountConfig::Session`).

use std::future::Future;
use std::pin::Pin;

use azalea::account::{Account, AccountTrait};
use azalea::auth::sessionserver::{self, ClientSessionServerError, SessionServerJoinOpts};
use uuid::Uuid;

/// Minecraft session forwarded by the launcher.
#[derive(Debug)]
pub struct SessionAccount {
    username: String,
    uuid: Uuid,
    access_token: String,
}

impl SessionAccount {
    /// Build an [`Account`] from a launcher session. `uuid` is the undashed or
    /// dashed Minecraft profile id; parsing failures fall back to nil (which
    /// only matters for offline servers, where the token is unused anyway).
    pub fn account(username: String, uuid: &str, access_token: String) -> Account {
        let uuid = Uuid::parse_str(uuid).unwrap_or(Uuid::nil());
        Account::from(SessionAccount { username, uuid, access_token })
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
