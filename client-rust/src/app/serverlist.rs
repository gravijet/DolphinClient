//! Vanilla-style saved server list: persisted entries (`servers.json` in the
//! DolphinClient config dir) + background status pings (MOTD, player count,
//! version, latency, favicon).

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use azalea::protocol::packets::PROTOCOL_VERSION;
use base64::Engine as _;
use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::bridge::events::ChatSpan;
use crate::bridge::text;
use crate::settings::GameSettings;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedServer {
    pub name: String,
    pub address: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ServerListStore {
    pub servers: Vec<SavedServer>,
}

impl ServerListStore {
    fn path() -> std::path::PathBuf {
        GameSettings::config_dir().join("servers.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(dir) = Self::path().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(), s);
        }
    }
}

/// Result of pinging one server.
#[derive(Clone)]
pub struct PingInfo {
    pub motd: Vec<ChatSpan>,
    pub version: String,
    /// Server speaks our protocol (joinable).
    pub protocol_ok: bool,
    pub online: i32,
    pub max: i32,
    pub latency_ms: u32,
    pub favicon: Option<std::sync::Arc<RgbaImage>>,
    pub error: Option<String>,
}

/// Per-row ping progress.
#[derive(Clone, Default)]
pub enum PingState {
    #[default]
    Idle,
    Pending,
    Done(PingInfo),
}

/// Fire-and-forget pinger: `ping()` spawns a worker, results arrive on `poll`.
pub struct Pinger {
    tx: Sender<(u64, String)>,
    rx: Receiver<(u64, PingInfo)>,
    started: Vec<(u64, Instant)>,
}

impl Default for Pinger {
    fn default() -> Self {
        let (tx, worker_rx) = channel::<(u64, String)>();
        let (worker_tx, rx) = channel::<(u64, PingInfo)>();
        std::thread::Builder::new()
            .name("server-ping".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(r) => r,
                    Err(_) => return,
                };
                rt.block_on(async move {
                    while let Ok((token, address)) = worker_rx.recv() {
                        let tx = worker_tx.clone();
                        let started = Instant::now();
                        let result = tokio::time::timeout(
                            Duration::from_secs(6),
                            azalea::ping::ping_server(address.as_str()),
                        )
                        .await;
                        let info = match result {
                            Ok(Ok(status)) => PingInfo {
                                motd: text::spans_of(&status.description),
                                protocol_ok: status.version.protocol == PROTOCOL_VERSION,
                                version: status.version.name,
                                online: status.players.online,
                                max: status.players.max,
                                latency_ms: started.elapsed().as_millis() as u32,
                                favicon: status.favicon.as_deref().and_then(decode_favicon),
                                error: None,
                            },
                            Ok(Err(e)) => ping_error(format!("{e}")),
                            Err(_) => ping_error("Zeitüberschreitung".into()),
                        };
                        let _ = tx.send((token, info));
                    }
                });
            })
            .ok();
        Self { tx, rx, started: Vec::new() }
    }
}

fn ping_error(msg: String) -> PingInfo {
    PingInfo {
        motd: vec![ChatSpan {
            text: format!("Kann Server nicht erreichen: {msg}"),
            color: Some([0xFF, 0x55, 0x55]),
            ..ChatSpan::default()
        }],
        version: String::new(),
        protocol_ok: false,
        online: 0,
        max: 0,
        latency_ms: 0,
        favicon: None,
        error: Some(msg),
    }
}

impl Pinger {
    /// Kick off a ping; the result arrives via `poll` with the same token.
    pub fn ping(&mut self, token: u64, address: &str) {
        self.started.push((token, Instant::now()));
        let _ = self.tx.send((token, address.to_string()));
    }

    pub fn poll(&mut self) -> Vec<(u64, PingInfo)> {
        let mut out = Vec::new();
        while let Ok(r) = self.rx.try_recv() {
            self.started.retain(|(t, _)| *t != r.0);
            out.push(r);
        }
        out
    }
}

/// Decode a `data:image/png;base64,…` favicon into an image.
fn decode_favicon(data: &str) -> Option<std::sync::Arc<RgbaImage>> {
    let b64 = data.strip_prefix("data:image/png;base64,")?;
    // Server MOTD favicons often contain stray newlines.
    let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD.decode(clean).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    Some(std::sync::Arc::new(img))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_roundtrip_json() {
        let store = ServerListStore {
            servers: vec![SavedServer { name: "Home".into(), address: "localhost".into() }],
        };
        let s = serde_json::to_string(&store).unwrap();
        let back: ServerListStore = serde_json::from_str(&s).unwrap();
        assert_eq!(back.servers.len(), 1);
        assert_eq!(back.servers[0].address, "localhost");
    }

    #[test]
    fn favicon_rejects_garbage() {
        assert!(decode_favicon("nope").is_none());
        assert!(decode_favicon("data:image/png;base64,!!!").is_none());
    }
}
