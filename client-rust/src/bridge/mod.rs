//! Network bridge: owns the azalea client on a dedicated tokio runtime thread.
//! Translates azalea events → [`GameEvent`] and applies [`Command`]s.
//! No azalea types cross this module boundary (except inside `events.rs` plain data).
//!
//! Implementation notes (see docs/rust-client/api-notes/):
//! - `spawn_bridge` starts a std thread running a current-thread tokio runtime
//!   with azalea's `ClientBuilder` (its `start()` future is `!Send` and only
//!   returns after `Client::exit`). Auto-reconnect is disabled — the app owns
//!   the reconnect policy.
//! - azalea does NOT store light, so `LevelChunkWithLight`/`LightUpdate`
//!   packets are parsed into a per-chunk light cache (`convert::ChunkLight`)
//!   and merged into the `SectionData` snapshots emitted on
//!   `Event::ReceiveChunk` (and re-emitted on later light updates).
//! - Section snapshots copy block states (4096 × u32, YZX), biomes (4×4×4)
//!   and light straight out of azalea's world; empty sections (block_count 0)
//!   are skipped — the mirror treats missing sections as air.
//! - `GameEvent::BlockChanged` comes from BlockUpdate + SectionBlocksUpdate
//!   packets (azalea already applied them to its own world).
//! - Player/entity/hotbar snapshots once per `Event::Tick` (entities every
//!   2nd tick); commands are drained and applied at the start of each tick.
//! - Reconnect is NOT handled here; app decides (v1: exit to connect screen).

pub mod events;
pub mod text;

mod account;
mod convert;
mod plugins;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context as _;
use azalea::core::entity_id::MinecraftEntityId;
use azalea::protocol::address::ResolvedAddr;
use azalea::protocol::packets::PROTOCOL_VERSION;
use azalea::core::hit_result::HitResult;
use azalea::core::position::{BlockPos as AzBlockPos, ChunkPos as AzChunkPos, Vec3};
use azalea::ecs::entity::Entity;
use azalea::entity::dimensions::EntityDimensions;
use azalea::entity::inventory::Inventory;
use azalea::entity::Pose;
use azalea::entity::metadata::{CustomName, Health, Invisible, Sprinting};
use azalea::entity::{EntityKindComponent, LocalEntity, LookDirection, Physics, Position};
use azalea::local_player::{Experience, Hunger};
use azalea::player::GameProfileComponent;
use azalea::app::PluginGroup;
use azalea::prelude::*;
use azalea::protocol::packets::game::{
    ClientboundAnimate, ClientboundGamePacket, ClientboundHurtAnimation, ClientboundLevelParticles,
    ClientboundResetScore, ClientboundResourcePackPush, ClientboundSetDisplayObjective,
    ClientboundSetEquipment, ClientboundSetObjective, ClientboundSetPlayerTeam, ClientboundSetScore,
    ClientboundSetTime,
};
use azalea::core::sound::CustomSound;
use azalea::registry::Holder;
use azalea::registry::builtin::{EntityKind, SoundEvent};
use azalea::inventory::{CloseContainerEvent, ContainerClickEvent};
use azalea::protocol::packets::game::{ServerboundCommandSuggestion, ServerboundSelectTrade};
use azalea::world::{Section, WorldName};
use azalea::{SprintDirection, WalkDirection};
use azalea_inventory::operations::{ClickOperation, PickupClick, QuickMoveClick, ThrowClick};
use azalea_inventory::{ItemStack, Menu, Player, components};
use base64::Engine as _;
use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use tracing::{debug, error, info, warn};

use crate::types::{BlockPos, ChunkPos, SectionData, SectionPos, StateId};
use convert::{ChunkLight, SectionLight};
use events::{
    AccountConfig, BridgeOptions, ChatSpan, Command, EntitySnapshot, Equipment, GameEvent,
    ItemSnapshot, PlayerSnapshot, ScoreLine, SlotClickKind, TabPlayer, TradeOffer,
};

/// How long the server may go completely silent before we treat the connection
/// as timed out. azalea's schedule keeps ticking on a dead socket without ever
/// firing `Event::Disconnect`, so without this the client sits frozen forever.
/// Kept below the app-side backstop watchdog so this (with its clean message)
/// wins when the bridge thread itself is still alive. 30s matches vanilla's
/// read-timeout feel and clears two missed 15s keep-alives, so a brief lag
/// spike on a live server never trips it.
const SERVER_SILENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Handle the app uses to control the game. Dropping it disconnects.
pub struct GameHandle {
    cmd_tx: Sender<Command>,
}

impl GameHandle {
    /// Queue a command; applied on the next tick. Never blocks.
    pub fn send(&self, cmd: Command) {
        // Unbounded channel: send never blocks. If the bridge thread is gone
        // the command is meaningless anyway — drop it silently.
        let _ = self.cmd_tx.send(cmd);
    }
}

impl Drop for GameHandle {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Disconnect);
    }
}

/// Pre-flight checks before the real join, each with a short timeout and a
/// precise, user-readable (German) error. Without this, azalea's `start()`
/// swallows every failure mode into a silent hang or a generic message:
/// - DNS/SRV failure → start() returns immediately with no event,
/// - unreachable host → long OS connect timeout, no event,
/// - wrong server version → login kick with a cryptic reason,
/// - expired session token → azalea logs an error and hangs forever.
///
/// On success it returns the fully [`ResolvedAddr`] (SRV redirect already
/// applied, IP already chosen) so the caller hands it straight to azalea and
/// the join path does no further DNS work.
async fn preflight(address: &str, account: &AccountConfig) -> Result<ResolvedAddr, String> {
    use std::time::Duration;
    use tokio::time::timeout;

    // 1 + 2. Parse and resolve (SRV → A/AAAA) with our own tuned resolver.
    //    Retried a few times: a freshly-launched process can see the very first
    //    resolver query fail (cold OS cache, a VPN/network still coming up);
    //    the next try then succeeds. This is what makes "the first 1-2 connects
    //    after start fail" invisible. Our resolver caches the win, so a retry is
    //    cheap and later connects are instant.
    let mut resolved = None;
    let mut last_err = String::new();
    for attempt in 0..4u32 {
        match timeout(Duration::from_secs(5), crate::net::resolve(address)).await {
            Ok(Ok(r)) => {
                resolved = Some(r);
                break;
            }
            Ok(Err(e)) => last_err = e,
            Err(_) => {
                last_err =
                    format!("DNS-Auflösung für „{address}“ dauert zu lange (Timeout).");
            }
        }
        warn!(attempt, %address, "preflight: DNS resolve failed, retrying");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let Some(resolved) = resolved else {
        return Err(last_err);
    };
    let socket = resolved.socket;
    info!(%socket, host = %resolved.server.host, "preflight: resolved");

    // 3. Raw TCP reachability (fast fail instead of a minutes-long OS timeout).
    match timeout(Duration::from_secs(6), tokio::net::TcpStream::connect(socket)).await {
        Err(_) => {
            return Err(format!(
                "Server {socket} antwortet nicht (Timeout). Ist der Server online?"
            ));
        }
        Ok(Err(e)) => {
            return Err(format!("Server {socket} nicht erreichbar: {e}"));
        }
        Ok(Ok(stream)) => drop(stream),
    }

    // 4. Status-Ping: informational only. The protocol number a server reports
    //    is its *native* version — but the vast majority of public servers run
    //    ViaVersion/ViaBackwards and happily accept a 26.1 client even while
    //    advertising an older protocol. Hard-blocking on a mismatch is exactly
    //    what made "manche Server gehen gar nicht" — so we NEVER block here.
    //    We attempt the join regardless; if the server truly can't speak our
    //    protocol, azalea surfaces the login kick with the real reason.
    //    Pass the already-resolved address so the ping reuses our lookup.
    match timeout(Duration::from_secs(6), azalea::ping::ping_server(&resolved)).await {
        Ok(Ok(status)) => {
            info!(
                version = %status.version.name,
                protocol = status.version.protocol,
                our_protocol = PROTOCOL_VERSION,
                players = status.players.online,
                "preflight: server status"
            );
            if status.version.protocol != PROTOCOL_VERSION {
                warn!(
                    server = %status.version.name,
                    server_protocol = status.version.protocol,
                    "preflight: server advertises a different protocol; joining anyway (ViaVersion?)"
                );
            }
        }
        Ok(Err(e)) => warn!("preflight: status ping failed (joining anyway): {e}"),
        Err(_) => warn!("preflight: status ping timed out (joining anyway)"),
    }

    // 5. Session-token sanity check (launcher sessions only). An expired token
    //    would otherwise make azalea hang silently during encryption. Network
    //    errors don't block — Mojang being down shouldn't stop offline play.
    if let AccountConfig::Session { access_token, .. } = account {
        let profile = reqwest::Client::new()
            .get("https://api.minecraftservices.com/minecraft/profile")
            .bearer_auth(access_token)
            .timeout(Duration::from_secs(8))
            .send()
            .await;
        match profile {
            Ok(res) if res.status().as_u16() == 401 => {
                return Err(
                    "Deine Minecraft-Session ist abgelaufen. Bitte starte den Launcher neu \
                     (er erneuert die Anmeldung automatisch)."
                        .to_string(),
                );
            }
            Ok(res) if !res.status().is_success() => {
                warn!(status = %res.status(), "preflight: profile check failed (joining anyway)");
            }
            Ok(_) => info!("preflight: session token OK"),
            Err(e) => warn!("preflight: profile check unreachable (joining anyway): {e}"),
        }
    }

    Ok(resolved)
}

/// Spawn the azalea client. Returns immediately; connection progress arrives
/// as `GameEvent::Connected` / `GameEvent::Disconnected` on the receiver.
pub fn spawn_bridge(opts: BridgeOptions) -> anyhow::Result<(GameHandle, Receiver<GameEvent>)> {
    anyhow::ensure!(!opts.address.trim().is_empty(), "empty server address");

    let (event_tx, event_rx) = crossbeam_channel::unbounded::<GameEvent>();
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<Command>();

    let state = BridgeState {
        event_tx: event_tx.clone(),
        cmd_rx,
        shared: Arc::new(Mutex::new(Shared::default())),
        dead: Arc::new(AtomicBool::new(false)),
        reported_end: Arc::new(AtomicBool::new(false)),
        disconnecting: Arc::new(AtomicBool::new(false)),
        exited: Arc::new(AtomicBool::new(false)),
        exit_task_spawned: Arc::new(AtomicBool::new(false)),
        view_distance: opts.view_distance.clamp(2, 32),
    };
    let reported_end = state.reported_end.clone();
    let disconnecting = state.disconnecting.clone();
    let exited = state.exited.clone();

    std::thread::Builder::new()
        .name("bridge-azalea".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = event_tx.send(GameEvent::Disconnected {
                        reason: format!("failed to start bridge runtime: {e}"),
                    });
                    return;
                }
            };
            // A panic inside azalea's schedule loop propagates out of
            // `start()`/`block_on` (its exit path double-panics on the dropped
            // appexit channel). Without this net the bridge thread dies
            // silently: no Disconnected event, the render loop keeps going and
            // the world just freezes. Catch it and surface a real disconnect.
            let panic_event_tx = event_tx.clone();
            let panic_reported_end = reported_end.clone();
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rt.block_on(async move {
                // Fail fast with a precise message instead of azalea's silent
                // failure modes (see `preflight`). It hands back the resolved
                // address so azalea joins without repeating the DNS/SRV lookup.
                let resolved = match preflight(&opts.address, &opts.account).await {
                    Ok(r) => r,
                    Err(reason) => {
                        warn!(reason, "bridge: preflight failed");
                        let _ = event_tx.send(GameEvent::Disconnected { reason });
                        return;
                    }
                };
                let account = match &opts.account {
                    AccountConfig::Offline(name) => Account::offline(name),
                    AccountConfig::Microsoft(email) => match Account::microsoft(email).await {
                        Ok(a) => a,
                        Err(e) => {
                            let _ = event_tx.send(GameEvent::Disconnected {
                                reason: format!("Microsoft login failed: {e}"),
                            });
                            return;
                        }
                    },
                    AccountConfig::Session { username, uuid, access_token } => {
                        account::SessionAccount::account(username.clone(), uuid, access_token.clone())
                    }
                };
                info!(address = %opts.address, socket = %resolved.socket, "bridge: connecting");
                // `start()` runs the whole client lifecycle; it returns only
                // once `Client::exit` fires (we call it on disconnect). We pass
                // the pre-resolved address, so it dials our socket directly and
                // does no further DNS.
                // Like ClientBuilder::new(), but with azalea's BrandPlugin
                // (which pretends to be "vanilla") swapped for our own brand
                // and our physics/behavior fixes added.
                ClientBuilder::new_without_plugins()
                    .add_plugins(
                        azalea::DefaultPlugins.build().disable::<azalea::brand::BrandPlugin>(),
                    )
                    .add_plugins(azalea::bot::DefaultBotPlugins)
                    .add_plugins(plugins::DolphinBrandPlugin)
                    .add_plugins(plugins::DolphinPhysicsPlugin)
                    .add_plugins(plugins::DolphinVehiclePlugin)
                    .set_handler(handle)
                    .set_state(state)
                    .reconnect_after(None::<std::time::Duration>) // app owns reconnects
                    .start(account, &resolved)
                    .await;
                info!("bridge: azalea client exited");
                exited.store(true, Ordering::SeqCst);
                // If nothing was reported yet, tell the app we're done. An
                // app-requested exit (Command::Disconnect) reports cleanly;
                // otherwise start() returned without an Event::Disconnect,
                // which usually means the address failed to resolve/connect.
                if !reported_end.swap(true, Ordering::SeqCst) {
                    let reason = if disconnecting.load(Ordering::SeqCst) {
                        "disconnected".into()
                    } else {
                        // Preflight passed, so the server was reachable — this
                        // is azalea giving up mid-login without an event.
                        "Verbindung wurde unerwartet beendet (Details im Log)".into()
                    };
                    let _ = event_tx.send(GameEvent::Disconnected { reason });
                }
            });
            }));
            if let Err(payload) = ran {
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unbekannter Fehler".into());
                error!(msg, "bridge: azalea panicked");
                if !panic_reported_end.swap(true, Ordering::SeqCst) {
                    let _ = panic_event_tx.send(GameEvent::Disconnected {
                        reason: format!("Interner Fehler im Netzwerk-Thread: {msg}"),
                    });
                }
            }
        })
        .context("spawning bridge thread")?;
    // The JoinHandle is intentionally dropped: the thread exits on its own
    // once the client exits.

    Ok((GameHandle { cmd_tx }, event_rx))
}

// ---------------------------------------------------------------------------
// Handler state
// ---------------------------------------------------------------------------

/// Mutable bridge state shared across handler invocations.
#[derive(Default)]
struct Shared {
    /// Light per chunk column, captured from raw packets (azalea drops light).
    light: HashMap<(i32, i32), ChunkLight>,
    /// Tick counter for the every-2nd-tick entity cadence.
    tick: u64,
    /// Last emitted `TimeOfDay` value.
    last_time: Option<i64>,
    /// Cheap comparable key of the last emitted hotbar (slots, offhand, selected).
    last_hotbar: Option<(Vec<Option<(String, u32)>>, Option<(String, u32)>, u8)>,
    /// Container id last seen on the Inventory component (0 = none open).
    container_id: i32,
    /// Cheap comparable key of the last emitted container/inventory content.
    last_content: Option<(i32, Vec<Option<(String, u32)>>, Option<(String, u32)>)>,
    /// Scoreboard objectives: name → display-title spans.
    sb_objectives: HashMap<String, Vec<ChatSpan>>,
    /// The objective currently shown in the `Sidebar` display slot, if any.
    sb_sidebar: Option<String>,
    /// Per-objective scores: objective name → (owner → (score, custom display,
    /// per-row "hide number" from number_format)).
    sb_scores: HashMap<String, HashMap<String, (i32, Option<Vec<ChatSpan>>, Option<bool>)>>,
    /// Per-objective default "hide number" (objective number_format = blank).
    sb_obj_blank: HashMap<String, bool>,
    /// Teams: name → (prefix spans, suffix spans). Modern minigame servers put
    /// the visible sidebar text in team prefix/suffix, keyed by a dummy owner.
    sb_teams: HashMap<String, (Vec<ChatSpan>, Vec<ChatSpan>)>,
    /// Which team each scoreboard owner (entry) belongs to.
    sb_member_team: HashMap<String, String>,
    /// Dedupe key for the last emitted sidebar (title, rows).
    last_scoreboard: Option<(Vec<ChatSpan>, Vec<ScoreLine>)>,
    /// Per-entity equipment from SetEquipment, keyed by MinecraftEntityId as u64.
    /// Pruned each tick against the live entity set in `entity_snapshots`.
    entity_equipment: HashMap<u64, Equipment>,
    /// Last time a real server packet/event arrived (`None` until the first).
    /// azalea's schedule keeps ticking on a dead connection without ever
    /// firing `Event::Disconnect`, so we watch server silence ourselves and
    /// surface a clean timeout (see `SERVER_SILENCE_TIMEOUT`).
    last_packet: Option<std::time::Instant>,
    /// Chunks whose `ReceiveChunk` raced azalea's world insert (the event
    /// listener and the chunk-apply system have no ordering guarantee).
    /// Retried once on the next tick instead of being dropped forever.
    retry_chunks: Vec<(i32, i32)>,
    /// Last emitted weather (rain, thunder) strengths, to dedupe re-emits.
    weather: (f32, f32),
}

/// azalea handler state: must be `Default + Clone + Component` (the handler is
/// a plain `fn`, so all context lives here).
#[derive(Clone, Component)]
struct BridgeState {
    event_tx: Sender<GameEvent>,
    cmd_rx: Receiver<Command>,
    shared: Arc<Mutex<Shared>>,
    /// Set when the app dropped the event receiver — stop all work.
    dead: Arc<AtomicBool>,
    /// Set once a terminal `Disconnected` has been emitted (dedupe).
    reported_end: Arc<AtomicBool>,
    /// Set when `Command::Disconnect` was applied: azalea starts tearing the
    /// client entity down, so component queries (snapshots) must stop —
    /// they panic on a half-despawned client.
    disconnecting: Arc<AtomicBool>,
    /// Set once azalea's `start()` actually returned (stops the exit retry).
    exited: Arc<AtomicBool>,
    /// Dedupe for the `request_exit` retry task.
    exit_task_spawned: Arc<AtomicBool>,
    /// The app's render distance, sent as the client view distance on init.
    view_distance: u8,
}

impl Default for BridgeState {
    fn default() -> Self {
        // Only exists to satisfy azalea's `S: Default` bound; `spawn_bridge`
        // always installs a real state via `set_state`.
        let (event_tx, _) = crossbeam_channel::unbounded();
        let (_, cmd_rx) = crossbeam_channel::unbounded();
        Self {
            event_tx,
            cmd_rx,
            shared: Arc::new(Mutex::new(Shared::default())),
            dead: Arc::new(AtomicBool::new(false)),
            reported_end: Arc::new(AtomicBool::new(false)),
            disconnecting: Arc::new(AtomicBool::new(false)),
            exited: Arc::new(AtomicBool::new(false)),
            exit_task_spawned: Arc::new(AtomicBool::new(false)),
            view_distance: 12,
        }
    }
}

impl BridgeState {
    /// Send an event to the app. If the app is gone, shut the client down.
    /// Must NOT be called while holding `shared` or any ECS lock.
    fn emit(&self, bot: &Client, event: GameEvent) {
        if self.event_tx.send(event).is_err() && !self.dead.swap(true, Ordering::SeqCst) {
            info!("bridge: app dropped the event receiver; disconnecting");
            self.disconnecting.store(true, Ordering::SeqCst);
            self.request_exit(bot);
        }
    }

    /// End `start()` (and with it the bridge thread) — BEST-EFFORT ONLY.
    ///
    /// azalea 0.16's exit path is unreliable: `Client::exit` writes bevy's
    /// `AppExit` message once, and (timing-dependent, reproduced ~30% of
    /// disconnects against a local server) the schedule loop never acts on it
    /// — the bridge thread then freezes solid, even ignoring further writes.
    /// Re-writing the message every 50ms until the loop actually dies
    /// (`exited` is set right after `start()` returns) rescues the benign
    /// cases; the frozen case leaks the thread, which is why callers must
    /// NEVER gate app-facing behavior on the thread exiting: emit
    /// `GameEvent::Disconnected` and close the connection (`bot.disconnect()`)
    /// *before* calling this.
    fn request_exit(&self, bot: &Client) {
        if self.exit_task_spawned.swap(true, Ordering::SeqCst) {
            return; // retry task already running
        }
        let bot = bot.clone();
        let exited = self.exited.clone();
        tokio::spawn(async move {
            // Give the schedule loop a couple of cycles to process a pending
            // DisconnectEvent (connection close) before tearing the loop down.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            for _ in 0..200 {
                if exited.load(Ordering::SeqCst) {
                    return;
                }
                bot.exit();
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            warn!("bridge: azalea schedule loop ignored exit for 10s; giving up");
        });
    }
}

// ---------------------------------------------------------------------------
// Event handler
// ---------------------------------------------------------------------------

async fn handle(bot: Client, event: Event, state: BridgeState) {
    if state.dead.load(Ordering::Relaxed) {
        return;
    }
    // Any real inbound traffic resets the server-silence watchdog. `Event::Tick`
    // is local (azalea keeps ticking on a dead socket), so it must NOT count.
    if matches!(
        event,
        Event::Login | Event::Chat(_) | Event::ReceiveChunk(_) | Event::Packet(_)
    ) {
        state.shared.lock().last_packet = Some(std::time::Instant::now());
    }
    match event {
        Event::Init => {
            // Tell the server (and azalea's own chunk storage, which sizes
            // itself from this) the app's real render distance — the default
            // of 8 silently capped chunks at ~11 even on higher settings.
            bot.set_client_information(azalea::ClientInformation {
                view_distance: state.view_distance,
                ..Default::default()
            });
        }
        Event::Login => on_login(&bot, &state),
        Event::Disconnect(reason) => {
            let reason = reason
                .map(|r| convert::strip_legacy_codes(&r.to_string()))
                .unwrap_or_else(|| "connection closed".into());
            info!(reason, "bridge: disconnected");
            if !state.reported_end.swap(true, Ordering::SeqCst) {
                state.emit(&bot, GameEvent::Disconnected { reason });
            }
            // Auto-reconnect is disabled; free the thread.
            state.request_exit(&bot);
        }
        Event::ConnectionFailed(err) => {
            let reason = format!("connection failed: {err}");
            warn!(reason, "bridge: connection failed");
            if !state.reported_end.swap(true, Ordering::SeqCst) {
                state.emit(&bot, GameEvent::Disconnected { reason });
            }
            state.request_exit(&bot);
        }
        Event::Chat(packet) => {
            let spans = text::spans_of(&packet.message());
            let system = packet.sender().is_none();
            state.emit(&bot, GameEvent::Chat { spans, system });
        }
        Event::ReceiveChunk(pos) => on_receive_chunk(&bot, &state, pos),
        Event::Packet(packet) => on_packet(&bot, &state, &packet),
        Event::Tick => on_tick(&bot, &state),
        _ => {}
    }
}

fn on_login(bot: &Client, state: &BridgeState) {
    // NOTE: Event::Login fires exactly once per connection (azalea keys it on
    // Added<MinecraftEntityId>, which respawn never re-inserts). Dimension
    // changes clear the light cache in `on_dimension_change` instead.
    state.shared.lock().light.clear();
    let username = bot
        .get_component::<GameProfileComponent>()
        .map(|p| p.name.clone())
        .unwrap_or_default();
    info!(username, "bridge: logged in");
    state.emit(bot, GameEvent::Connected { username });

    // The biome registry (climate + colour effects) arrives during the config
    // phase, so it's ready by login. Read it once and hand it to the app, which
    // turns it into per-biome grass/foliage/water tint colours for the mesher.
    let biomes = read_biomes(bot);
    if !biomes.is_empty() {
        info!(count = biomes.len(), "bridge: biome registry read");
        state.emit(bot, GameEvent::Biomes(std::sync::Arc::new(biomes)));
    }
}

/// Read every biome's climate (temperature/downfall) and colour effects from the
/// server's biome registry, indexed by protocol id. Empty if the registry is
/// absent (shouldn't happen post-login).
fn read_biomes(bot: &Client) -> Vec<events::BiomeInfo> {
    use azalea::registry::identifier::Identifier;
    let world = bot.world();
    let world = world.read();
    let key = Identifier::new("minecraft:worldgen/biome");
    let Some(reg) = world.registries.extra.get(&key) else {
        return Vec::new();
    };
    let rgb = |v: i32| [((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8];
    reg.map
        .values()
        .map(|nbt| {
            let effects = nbt.compound("effects");
            events::BiomeInfo {
                temperature: nbt.float("temperature").unwrap_or(0.5),
                downfall: nbt.float("downfall").unwrap_or(0.5),
                grass_override: effects.and_then(|e| e.int("grass_color")).map(rgb),
                foliage_override: effects.and_then(|e| e.int("foliage_color")).map(rgb),
                water: effects
                    .and_then(|e| e.int("water_color"))
                    .map(rgb)
                    .unwrap_or([0x3F, 0x76, 0xE4]),
                grass_modifier: match effects.and_then(|e| e.string("grass_color_modifier")) {
                    Some(s) if s.to_string() == "dark_forest" => 1,
                    Some(s) if s.to_string() == "swamp" => 2,
                    _ => 0,
                },
            }
        })
        .collect()
}

/// Read a data-driven variant registry (e.g. `cat_variant`, `wolf_variant`) into
/// a list of variant names indexed by protocol id — the registry `map` is an
/// `IndexMap`, so its Nth key is protocol id N. Empty if the registry is absent.
fn read_variant_registry(bot: &Client, name: &str) -> Vec<String> {
    use azalea::registry::identifier::Identifier;
    let world = bot.world();
    let world = world.read();
    let key = Identifier::new(&format!("minecraft:{name}"));
    world
        .registries
        .extra
        .get(&key)
        .map(|reg| reg.map.keys().map(|id| id.path().to_string()).collect())
        .unwrap_or_default()
}

/// Read the server's `painting_variant` registry into `(asset, width, height)`
/// indexed by protocol id — each entry's NBT carries the art asset id and the
/// painting's size in blocks (mirrors `read_biomes`' NBT-field reads). Empty if
/// the registry is absent.
fn read_painting_variants(bot: &Client) -> Vec<events::PaintingInfo> {
    use azalea::registry::identifier::Identifier;
    let world = bot.world();
    let world = world.read();
    let key = Identifier::new("minecraft:painting_variant");
    let Some(reg) = world.registries.extra.get(&key) else {
        return Vec::new();
    };
    reg.map
        .values()
        .map(|nbt| {
            let asset = nbt
                .string("asset_id")
                .map(|s| strip_minecraft_ns(&s.to_string()))
                .unwrap_or_default();
            events::PaintingInfo {
                asset,
                width: nbt.int("width").unwrap_or(1),
                height: nbt.int("height").unwrap_or(1),
                facing: 3, // filled in per-entity from PaintingDirection
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// World → SectionData
// ---------------------------------------------------------------------------

/// Number of world sections (height / 16) in the current dimension.
fn world_section_count(bot: &Client) -> usize {
    let world = bot.world();
    let world = world.read();
    (world.chunks.height() / 16) as usize
}

fn section_data(section: &Section, light: Option<&SectionLight>) -> SectionData {
    SectionData {
        blocks: convert::copy_section_blocks(section),
        sky_light: light.and_then(|l| l.sky.clone()),
        block_light: light.and_then(|l| l.block.clone()),
        biomes: convert::copy_section_biomes(section),
    }
}

/// Copy the requested sections (`None` = all non-empty) of a chunk into plain
/// `SectionData`, attaching cached light. Returns owned data; all locks are
/// released before this returns.
fn copy_chunk_sections(
    bot: &Client,
    state: &BridgeState,
    cx: i32,
    cz: i32,
    only: Option<&[usize]>,
) -> Vec<(SectionPos, SectionData)> {
    let (min_y, chunk) = {
        let world = bot.world();
        let world = world.read();
        (world.chunks.min_y(), world.chunks.get(&AzChunkPos::new(cx, cz)))
    };
    let Some(chunk) = chunk else {
        return Vec::new();
    };
    let min_section_y = min_y >> 4;
    let chunk = chunk.read();
    let shared = state.shared.lock();
    let light = shared.light.get(&(cx, cz));
    let mut out = Vec::new();
    let mut push = |i: usize, section: &Section| {
        if section.block_count == 0 {
            return; // mirror treats missing sections as air
        }
        let pos = SectionPos { x: cx, y: min_section_y + i as i32, z: cz };
        out.push((pos, section_data(section, light.and_then(|l| l.sections.get(i)))));
    };
    match only {
        Some(indices) => {
            for &i in indices {
                if let Some(section) = chunk.sections.get(i) {
                    push(i, section);
                }
            }
        }
        None => {
            for (i, section) in chunk.sections.iter().enumerate() {
                push(i, section);
            }
        }
    }
    out
}

fn on_receive_chunk(bot: &Client, state: &BridgeState, pos: AzChunkPos) {
    let sections = copy_chunk_sections(bot, state, pos.x, pos.z, None);
    if sections.is_empty() {
        // Chunk may legitimately be all air, but a missing chunk means the
        // ReceiveChunk event raced azalea's world insert (no ordering
        // guarantee between the two systems) — retry it next tick instead of
        // dropping it forever (invisible terrain, especially after respawn).
        let present = {
            let world = bot.world();
            let world = world.read();
            world.chunks.get(&pos).is_some()
        };
        if !present {
            debug!(x = pos.x, z = pos.z, "bridge: ReceiveChunk raced the world insert; retrying next tick");
            state.shared.lock().retry_chunks.push((pos.x, pos.z));
        }
        return;
    }
    debug!(x = pos.x, z = pos.z, sections = sections.len(), "bridge: chunk sections");
    for (pos, data) in sections {
        state.emit(bot, GameEvent::Section { pos, data });
    }
}

/// Mount/dismount tracking: azalea ignores `SetPassengers` entirely, so the
/// vehicle plugin's `RidingVehicle` marker on our own player is maintained
/// here (it drives the client-side boat simulation and position pinning).
fn on_set_passengers(
    bot: &Client,
    p: &azalea::protocol::packets::game::c_set_passengers::ClientboundSetPassengers,
) {
    let Some(my_id) = bot.get_component::<MinecraftEntityId>().map(|id| *id) else {
        return;
    };
    let am_passenger = p.passengers.contains(&my_id);
    // Resolve the vehicle BEFORE taking the write lock (the lookup locks too).
    let vehicle = bot.entity_id_by_minecraft_id(p.vehicle);
    let mut ecs = bot.ecs.write();
    if am_passenger {
        let Some(vehicle) = vehicle else { return };
        let is_boat = ecs
            .get::<EntityKindComponent>(vehicle)
            .map(|k| {
                let name = k.to_str();
                name.ends_with("boat") || name.ends_with("raft")
            })
            .unwrap_or(false);
        info!(vehicle = ?p.vehicle, is_boat, "bridge: mounted a vehicle");
        ecs.entity_mut(bot.entity).insert(plugins::RidingVehicle {
            vehicle,
            is_boat,
            delta_rotation: 0.0,
        });
    } else {
        // Only dismount when THIS vehicle's passenger list dropped us —
        // other vehicles' passenger updates are none of our business.
        let ours = ecs
            .get::<plugins::RidingVehicle>(bot.entity)
            .is_some_and(|r| vehicle == Some(r.vehicle));
        if ours {
            info!(vehicle = ?p.vehicle, "bridge: dismounted");
            ecs.entity_mut(bot.entity).remove::<plugins::RidingVehicle>();
        }
    }
}

/// The player respawned / changed dimension (or just logged in): reset the
/// per-dimension caches and tell the app which dimension it is in now.
/// azalea has already swapped its own world by the time this runs.
fn on_dimension_change(
    bot: &Client,
    state: &BridgeState,
    common: &azalea::protocol::packets::common::CommonPlayerSpawnInfo,
) {
    // The old dimension's cached light must never bleed into the new one.
    state.shared.lock().light.clear();
    let resolved = {
        let world = bot.world();
        let world = world.read();
        common.dimension_type(&world.registries).map(|(id, data)| {
            let name = strip_minecraft_ns(&id.to_string());
            // Default (non-strict) registry parsing keeps has_skylight in
            // the _extra NBT bag; fall back to the well-known names.
            let has_skylight = data
                ._extra
                .get("has_skylight")
                .and_then(|tag| tag.byte())
                .map(|b| b != 0)
                .unwrap_or_else(|| !(name.contains("nether") || name.contains("the_end")));
            let ultrawarm = data.ultrawarm.unwrap_or_else(|| name.contains("nether"));
            (name, has_skylight, ultrawarm)
        })
    };
    let (dimension, has_skylight, ultrawarm) = resolved.unwrap_or_else(|| {
        // Registry entry missing (ViaVersion edge): guess from the world
        // name — the app must still clear the stale world either way.
        let name = strip_minecraft_ns(&common.dimension.to_string());
        let nether = name.contains("nether");
        let end = name.contains("the_end");
        (name, !(nether || end), nether)
    });
    info!(dimension, has_skylight, ultrawarm, "bridge: dimension change / respawn");
    state.emit(bot, GameEvent::Respawn { dimension, has_skylight, ultrawarm });
}

// ---------------------------------------------------------------------------
// Raw packets: light, block changes, chunk unload, time
// ---------------------------------------------------------------------------

fn on_packet(bot: &Client, state: &BridgeState, packet: &ClientboundGamePacket) {
    match packet {
        ClientboundGamePacket::LevelChunkWithLight(p) => {
            // Cache the light; the matching Event::ReceiveChunk (fired for
            // this same packet, after azalea parsed the blocks) attaches it.
            let section_count = world_section_count(bot);
            if section_count == 0 {
                return;
            }
            let mut shared = state.shared.lock();
            let entry = shared.light.entry((p.x, p.z)).or_default();
            convert::apply_light_data(entry, section_count, &p.light_data);
        }
        ClientboundGamePacket::LightUpdate(p) => {
            let section_count = world_section_count(bot);
            if section_count == 0 {
                return;
            }
            let changed = {
                let mut shared = state.shared.lock();
                let entry = shared.light.entry((p.x, p.z)).or_default();
                convert::apply_light_data(entry, section_count, &p.light_data)
            };
            // Re-emit affected sections that are already loaded so the mirror
            // picks up the new light (blocks re-read from azalea's world).
            let sections = copy_chunk_sections(bot, state, p.x, p.z, Some(&changed));
            for (pos, data) in sections {
                state.emit(bot, GameEvent::Section { pos, data });
            }
        }
        ClientboundGamePacket::ForgetLevelChunk(p) => {
            state.shared.lock().light.remove(&(p.pos.x, p.pos.z));
            state.emit(bot, GameEvent::ChunkUnloaded { pos: ChunkPos { x: p.pos.x, z: p.pos.z } });
        }
        ClientboundGamePacket::BlockUpdate(p) => {
            // azalea already applied it to its world; we just notify the app.
            state.emit(bot, GameEvent::BlockChanged {
                pos: BlockPos { x: p.pos.x, y: p.pos.y, z: p.pos.z },
                state: p.block_state.id() as StateId,
            });
        }
        ClientboundGamePacket::SectionBlocksUpdate(p) => {
            for s in &p.states {
                let abs: AzBlockPos = p.section_pos + s.pos;
                state.emit(bot, GameEvent::BlockChanged {
                    pos: BlockPos { x: abs.x, y: abs.y, z: abs.z },
                    state: s.state.id() as StateId,
                });
            }
        }
        ClientboundGamePacket::SetTime(p) => on_set_time(bot, state, p),
        ClientboundGamePacket::GameEvent(p) => on_game_event(bot, state, p),
        ClientboundGamePacket::UpdateMobEffect(p) => {
            // Only the local player's effects, and only ones the server wants
            // shown as a HUD icon.
            if bot.get_component::<MinecraftEntityId>().map(|id| *id) == Some(p.entity_id)
                && p.data.flags.show_icon
            {
                state.emit(bot, GameEvent::EffectUpdate {
                    name: strip_minecraft_ns(p.mob_effect.to_str()),
                    amplifier: p.data.amplifier.max(0) as u32,
                    duration_ticks: p.data.duration,
                });
            }
        }
        ClientboundGamePacket::RemoveMobEffect(p) => {
            if bot.get_component::<MinecraftEntityId>().map(|id| *id) == Some(p.entity_id) {
                state.emit(bot, GameEvent::EffectRemove {
                    name: strip_minecraft_ns(p.effect.to_str()),
                });
            }
        }
        ClientboundGamePacket::Cooldown(p) => {
            // Item use-cooldown (ender pearl, chorus fruit, shield, …): the
            // server sends duration>0 to start it and 0 to clear it.
            state.emit(bot, GameEvent::Cooldown {
                name: strip_minecraft_ns(p.item.to_str()),
                duration_ticks: p.duration,
            });
        }
        ClientboundGamePacket::TabList(p) => {
            let header = text::spans_of(&p.header);
            let footer = text::spans_of(&p.footer);
            let empty = |s: &[ChatSpan]| s.iter().all(|sp| sp.text.trim().is_empty());
            state.emit(bot, GameEvent::TabHeaderFooter {
                header: if empty(&header) { Vec::new() } else { header },
                footer: if empty(&footer) { Vec::new() } else { footer },
            });
        }
        ClientboundGamePacket::CommandSuggestions(p) => {
            let range = p.suggestions.range();
            state.emit(bot, GameEvent::TabSuggestions {
                id: p.id,
                start: range.start(),
                length: range.end().saturating_sub(range.start()),
                entries: p.suggestions.list().iter().map(|s| s.text()).collect(),
            });
        }
        ClientboundGamePacket::MerchantOffers(p) => {
            let offers = p
                .offers
                .iter()
                .map(|o| TradeOffer {
                    input_a: ItemSnapshot {
                        item: strip_minecraft_ns(o.base_cost_a.item.to_str()),
                        count: o.base_cost_a.count.max(1) as u32,
                        ..Default::default()
                    },
                    input_b: o.cost_b.as_ref().map(|c| ItemSnapshot {
                        item: strip_minecraft_ns(c.item.to_str()),
                        count: c.count.max(1) as u32,
                        ..Default::default()
                    }),
                    output: slot_snapshot(&o.result).unwrap_or(ItemSnapshot {
                        item: "air".into(),
                        count: 0,
                        ..Default::default()
                    }),
                    disabled: o.out_of_stock,
                })
                .collect();
            state.emit(bot, GameEvent::MerchantOffers { container_id: p.container_id, offers });
        }
        ClientboundGamePacket::Sound(p) => {
            // Packet carries a fixed-point position (blockPos * 8).
            state.emit(bot, GameEvent::Sound {
                name: sound_event_name(&p.sound),
                category: map_sound_source(p.source as i32),
                pos: Some([p.x as f64 / 8.0, p.y as f64 / 8.0, p.z as f64 / 8.0]),
                volume: p.volume,
                pitch: p.pitch,
                seed: p.seed,
            });
        }
        ClientboundGamePacket::SetPassengers(p) => on_set_passengers(bot, p),
        ClientboundGamePacket::LevelEvent(p) => {
            // 2001 = block-break effect (sound + particles). The server sends
            // it for everyone EXCEPT the player who broke the block — own
            // breaks are synthesized app-side from the mining snapshot.
            if p.event_type == 2001 {
                state.emit(bot, GameEvent::BlockBreakEffect {
                    pos: BlockPos { x: p.pos.x, y: p.pos.y, z: p.pos.z },
                    state: p.data as StateId,
                });
            }
        }
        ClientboundGamePacket::Respawn(p) => on_dimension_change(bot, state, &p.common),
        ClientboundGamePacket::Login(p) => on_dimension_change(bot, state, &p.common),
        ClientboundGamePacket::SoundEntity(p) => {
            // Entity-attached sounds — hurt/attack/eat/… The app knows every
            // tracked entity's position, so it resolves the sound position.
            state.emit(bot, GameEvent::EntitySound {
                id: p.id.0 as u32 as u64,
                name: sound_event_name(&p.sound),
                category: map_sound_source(p.source as i32),
                volume: p.volume,
                pitch: p.pitch,
                seed: p.seed,
            });
        }
        ClientboundGamePacket::EntityEvent(p) => {
            // Legacy hurt animation (pre-HurtAnimation servers / ViaVersion
            // translations): entity event 2 = hurt.
            if p.event_id == 2 {
                state.emit(bot, GameEvent::EntityHurt { id: p.entity_id.0 as u32 as u64 });
            }
        }
        ClientboundGamePacket::DamageEvent(p) => {
            // Modern damage event — flash the entity red as well. The app
            // ignores duplicate flashes from HurtAnimation within the window.
            state.emit(bot, GameEvent::EntityHurt { id: p.entity_id.0 as u32 as u64 });
        }
        ClientboundGamePacket::SetObjective(p) => on_set_objective(bot, state, p),
        ClientboundGamePacket::SetDisplayObjective(p) => on_set_display_objective(bot, state, p),
        ClientboundGamePacket::SetScore(p) => on_set_score(bot, state, p),
        ClientboundGamePacket::ResetScore(p) => on_reset_score(bot, state, p),
        ClientboundGamePacket::SetPlayerTeam(p) => on_set_player_team(bot, state, p),
        ClientboundGamePacket::ResourcePackPush(p) => on_resource_pack_push(bot, state, p),
        ClientboundGamePacket::SetEquipment(p) => on_set_equipment(state, p),
        ClientboundGamePacket::HurtAnimation(p) => on_hurt_animation(bot, state, p),
        ClientboundGamePacket::Animate(p) => on_animate(bot, state, p),
        ClientboundGamePacket::LevelParticles(p) => on_level_particles(bot, state, p),
        _ => {}
    }
}

/// An entity took damage: flash it red (client-side animation).
fn on_hurt_animation(bot: &Client, state: &BridgeState, p: &ClientboundHurtAnimation) {
    state.emit(bot, GameEvent::EntityHurt { id: p.id.0 as u32 as u64 });
}

/// An entity animated. A main/off-hand swing plays the arm-swing so other
/// players are visibly seen hitting/mining, exactly like vanilla.
fn on_animate(bot: &Client, state: &BridgeState, p: &ClientboundAnimate) {
    use azalea::protocol::packets::game::c_animate::AnimationAction;
    if matches!(p.action, AnimationAction::SwingMainHand | AnimationAction::SwingOffHand) {
        state.emit(bot, GameEvent::EntitySwing { id: p.id.0 as u32 as u64 });
    }
}

/// Translate a server particle burst into a spawn request the app can render.
fn on_level_particles(bot: &Client, state: &BridgeState, p: &ClientboundLevelParticles) {
    // Cap the count so a firework/explosion burst can't flood the sim.
    let count = p.count.min(64);
    if count == 0 {
        return;
    }
    let (tex, color, size, gravity) = particle_style(&p.particle);
    state.emit(bot, GameEvent::Particles {
        pos: [p.pos.x, p.pos.y, p.pos.z],
        tex,
        color,
        size,
        count,
        spread: [p.x_dist, p.y_dist, p.z_dist],
        speed: p.max_speed,
        gravity,
    });
}

/// Map a particle kind to a flat color, cube size, and gravity for the app's
/// lightweight cube-particle renderer. Data-carrying variants (block/dust) are
/// approximated by a representative color.
fn particle_style(particle: &azalea::entity::particle::Particle) -> (events::ParticleTex, [f32; 3], f32, f32) {
    use azalea::entity::particle::Particle as P;
    use events::ParticleTex as T;
    // (texture family, tint, size, gravity). Tint is white for particles whose
    // colour lives in the texture; coloured for dust and a few tinted families.
    let w = [1.0, 1.0, 1.0];
    match particle {
        P::Crit => (T::Crit, w, 0.14, 3.0),
        P::EnchantedHit => (T::EnchantedHit, w, 0.14, 3.0),
        P::DamageIndicator => (T::Damage, w, 0.16, 2.0),
        P::Heart => (T::Heart, w, 0.20, 0.0),
        P::Flame | P::CopperFireFlame => (T::Flame, w, 0.12, -0.5),
        P::SoulFireFlame => (T::SoulFlame, w, 0.12, -0.5),
        P::FallingLava | P::LandingLava | P::DrippingLava => (T::Lava, w, 0.14, 4.0),
        P::Smoke | P::LargeSmoke => (T::Smoke, w, 0.14, -0.4),
        P::Cloud | P::Poof => (T::Generic, w, 0.16, -0.2),
        P::Explosion | P::ExplosionEmitter => (T::Explosion, w, 0.55, -0.3),
        P::Bubble => (T::Bubble, w, 0.10, -1.0),
        P::Splash => (T::Splash, w, 0.12, -1.0),
        P::DrippingWater | P::FallingWater => (T::Drip, [0.30, 0.45, 0.85], 0.10, 5.0),
        P::HappyVillager => (T::Happy, w, 0.16, -0.2),
        P::AngryVillager => (T::Angry, w, 0.18, -0.2),
        P::Portal | P::ReversePortal => (T::Portal, [0.55, 0.25, 0.85], 0.12, 0.0),
        P::Effect | P::EntityEffect(_) => (T::Effect, w, 0.14, 0.0),
        P::Note => (T::Note, w, 0.18, -0.1),
        P::Firework | P::Flash => (T::Flash, w, 0.16, 1.0),
        P::Glow | P::GlowSquidInk => (T::Glow, w, 0.12, 0.0),
        P::Block(_) | P::BlockMarker(_) | P::FallingDust(_) => (T::Generic, [0.55, 0.52, 0.48], 0.12, 6.0),
        P::Dust(_) | P::DustColorTransition(_) => (T::Dust, [0.85, 0.45, 0.45], 0.12, 0.0),
        P::TotemOfUndying => (T::Happy, [0.95, 0.85, 0.35], 0.14, 1.0),
        P::Snowflake => (T::Generic, [0.92, 0.94, 0.98], 0.12, 1.5),
        _ => (T::Generic, [0.85, 0.85, 0.88], 0.12, 0.5),
    }
}

/// Mirror a remote entity's equipment (armor + hands). azalea ignores this
/// packet for non-local entities, so we track it ourselves and attach it to the
/// per-tick entity snapshots (drives armor rendering on other players).
fn on_set_equipment(state: &BridgeState, p: &ClientboundSetEquipment) {
    let id = p.entity_id.0 as u32 as u64;
    let mut sh = state.shared.lock();
    let eq = sh.entity_equipment.entry(id).or_default();
    for (slot, stack) in &p.slots.slots {
        let name = match stack {
            ItemStack::Present(d) => Some(strip_minecraft_ns(d.kind.to_str())),
            ItemStack::Empty => None,
        };
        match slot {
            components::EquipmentSlot::Head => eq.head = name,
            components::EquipmentSlot::Chest => eq.chest = name,
            components::EquipmentSlot::Legs => eq.legs = name,
            components::EquipmentSlot::Feet => eq.feet = name,
            components::EquipmentSlot::Mainhand => eq.main_hand = name,
            components::EquipmentSlot::Offhand => eq.off_hand = name,
            // Body/Saddle are animal armor — not shown on the humanoid model.
            _ => {}
        }
    }
}

/// Respond to a server resource-pack push. azalea does NOT auto-reply, so a
/// server that pushes a *required* pack kicks us the moment we ignore it. We
/// acknowledge acceptance and successful load so play continues; the pack's
/// textures aren't repainted into the world yet, but the connection stays
/// healthy (the common failure the user hit on pack-forcing servers).
fn on_resource_pack_push(bot: &Client, state: &BridgeState, p: &ClientboundResourcePackPush) {
    use azalea::protocol::packets::game::s_resource_pack::{Action, ServerboundResourcePack};
    // ACK acceptance + success right away so a pack-forcing server never kicks
    // us, regardless of how the download below goes.
    bot.write_packet(ServerboundResourcePack { id: p.id, action: Action::Accepted });
    bot.write_packet(ServerboundResourcePack { id: p.id, action: Action::SuccessfullyLoaded });
    info!(url = %p.url, required = p.required, "bridge: server resource pack");

    let url = p.url.clone();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return; // only real HTTP(S) packs can be fetched
    }
    let hash = p.hash.clone();
    let event_tx = state.event_tx.clone();
    // Download in the background (this runs inside azalea's tokio runtime), then
    // hand the local .zip to the app to overlay + re-bake.
    tokio::spawn(async move {
        match download_resource_pack(&url, &hash).await {
            Ok(path) => {
                info!(path = %path.display(), "bridge: server resource pack downloaded");
                let _ = event_tx.send(GameEvent::ResourcePackReady { path });
            }
            Err(e) => warn!("bridge: resource pack download failed: {e:#}"),
        }
    });
}

/// Download a server resource pack to a per-hash cache file, returning its path.
/// Cached by content hash (or URL hash) so re-pushes don't re-download. Capped
/// at 256 MB to bound abuse.
async fn download_resource_pack(url: &str, hash: &str) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Write as _;
    let dir = crate::settings::GameSettings::config_dir().join("server-packs");
    std::fs::create_dir_all(&dir)?;
    // Name by the server-supplied hash when present (stable), else by the URL.
    let key = if hash.len() >= 8 {
        hash.to_string()
    } else {
        format!("{:016x}", crate::app::skins::fnv64(url.as_bytes()))
    };
    let path = dir.join(format!("{key}.zip"));
    if path.is_file() && std::fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(path); // cached
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("DolphinClient/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let resp = client.get(url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    anyhow::ensure!(bytes.len() <= 256 * 1024 * 1024, "resource pack too large");
    // Write atomically via a temp file so a partial download isn't cached.
    let tmp = dir.join(format!("{key}.part"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.flush()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

// -- scoreboard --------------------------------------------------------------
// The vanilla sidebar is assembled from four packets: SetObjective (declares an
// objective + its title), SetDisplayObjective (binds an objective to a display
// slot), SetScore (a row's value/display) and ResetScore (removes a row). We
// mirror that state and re-emit the whole sidebar whenever it changes.

fn on_set_objective(bot: &Client, state: &BridgeState, p: &ClientboundSetObjective) {
    use azalea::protocol::packets::game::c_set_objective::Method;
    use azalea_chat::numbers::NumberFormat;
    {
        let mut sh = state.shared.lock();
        match &p.method {
            Method::Add { display_name, number_format, .. }
            | Method::Change { display_name, number_format, .. } => {
                let title = text::spans_of(display_name);
                sh.sb_objectives.insert(p.objective_name.clone(), title);
                sh.sb_obj_blank.insert(
                    p.objective_name.clone(),
                    matches!(number_format, NumberFormat::Blank),
                );
            }
            Method::Remove => {
                sh.sb_objectives.remove(&p.objective_name);
                sh.sb_scores.remove(&p.objective_name);
                sh.sb_obj_blank.remove(&p.objective_name);
                if sh.sb_sidebar.as_deref() == Some(p.objective_name.as_str()) {
                    sh.sb_sidebar = None;
                }
            }
        }
    }
    emit_scoreboard(bot, state);
}

/// Track team prefix/suffix + membership so the sidebar (and its dummy-owner
/// rows) render the text the server actually intends.
fn on_set_player_team(bot: &Client, state: &BridgeState, p: &ClientboundSetPlayerTeam) {
    use azalea::protocol::packets::game::c_set_player_team::Method;
    {
        let mut sh = state.shared.lock();
        match &p.method {
            Method::Add((params, players)) => {
                sh.sb_teams.insert(
                    p.name.clone(),
                    (text::spans_of(&params.player_prefix), text::spans_of(&params.player_suffix)),
                );
                for m in players {
                    sh.sb_member_team.insert(m.clone(), p.name.clone());
                }
            }
            Method::Change(params) => {
                sh.sb_teams.insert(
                    p.name.clone(),
                    (text::spans_of(&params.player_prefix), text::spans_of(&params.player_suffix)),
                );
            }
            Method::Remove => {
                sh.sb_teams.remove(&p.name);
                sh.sb_member_team.retain(|_, t| t != &p.name);
            }
            Method::Join(players) => {
                for m in players {
                    sh.sb_member_team.insert(m.clone(), p.name.clone());
                }
            }
            Method::Leave(players) => {
                for m in players {
                    if sh.sb_member_team.get(m) == Some(&p.name) {
                        sh.sb_member_team.remove(m);
                    }
                }
            }
        }
    }
    emit_scoreboard(bot, state);
}

fn on_set_display_objective(
    bot: &Client,
    state: &BridgeState,
    p: &ClientboundSetDisplayObjective,
) {
    use azalea::protocol::packets::game::c_set_display_objective::DisplaySlot;
    {
        let mut sh = state.shared.lock();
        if p.slot == DisplaySlot::Sidebar {
            // An empty objective name clears the slot.
            sh.sb_sidebar =
                (!p.objective_name.is_empty()).then(|| p.objective_name.clone());
        } else if sh.sb_sidebar.as_deref() == Some(p.objective_name.as_str()) {
            // This objective was moved off the sidebar into another slot.
            sh.sb_sidebar = None;
        }
    }
    emit_scoreboard(bot, state);
}

fn on_set_score(bot: &Client, state: &BridgeState, p: &ClientboundSetScore) {
    use azalea_chat::numbers::NumberFormat;
    {
        let mut sh = state.shared.lock();
        let display = p.display.as_ref().map(text::spans_of);
        let hide = p.number_format.as_ref().map(|nf| matches!(nf, NumberFormat::Blank));
        sh.sb_scores
            .entry(p.objective_name.clone())
            .or_default()
            .insert(p.owner.clone(), (p.score as i32, display, hide));
    }
    emit_scoreboard(bot, state);
}

fn on_reset_score(bot: &Client, state: &BridgeState, p: &ClientboundResetScore) {
    {
        let mut sh = state.shared.lock();
        match &p.objective_name {
            Some(obj) => {
                if let Some(m) = sh.sb_scores.get_mut(obj) {
                    m.remove(&p.owner);
                }
            }
            // No objective given: drop this owner from every objective.
            None => {
                for m in sh.sb_scores.values_mut() {
                    m.remove(&p.owner);
                }
            }
        }
    }
    emit_scoreboard(bot, state);
}

/// Decorate a scoreboard owner (entry) with its team's prefix + suffix. The
/// owner string is parsed for legacy § codes; blank/dummy owners then contribute
/// nothing and the prefix/suffix carry all the visible text (minigame style).
fn team_decorated(sh: &Shared, owner: &str) -> Vec<ChatSpan> {
    let team = sh.sb_member_team.get(owner).and_then(|t| sh.sb_teams.get(t));
    let mut out = Vec::new();
    if let Some((prefix, _)) = team {
        out.extend(prefix.iter().cloned());
    }
    out.extend(text::spans_of_legacy(owner));
    if let Some((_, suffix)) = team {
        out.extend(suffix.iter().cloned());
    }
    out
}

/// Rebuild the sidebar from mirrored state and emit it if it changed. Must NOT
/// be called while holding `shared` (it takes the lock itself, briefly).
fn emit_scoreboard(bot: &Client, state: &BridgeState) {
    let next = {
        let mut sh = state.shared.lock();
        let (title, lines) = match sh.sb_sidebar.clone() {
            Some(obj) => {
                let title = sh.sb_objectives.get(&obj).cloned().unwrap_or_default();
                let obj_blank = sh.sb_obj_blank.get(&obj).copied().unwrap_or(false);
                let mut lines: Vec<ScoreLine> = sh
                    .sb_scores
                    .get(&obj)
                    .map(|m| {
                        m.iter()
                            .map(|(owner, (score, display, hide))| {
                                // Row text: an explicit per-line display wins;
                                // otherwise decorate the owner with its team's
                                // prefix/suffix (how minigame sidebars work).
                                let text = display.clone().unwrap_or_else(|| {
                                    team_decorated(&sh, owner)
                                });
                                ScoreLine {
                                    text,
                                    score: *score,
                                    hide_number: hide.unwrap_or(obj_blank),
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                // Highest score first (vanilla), stable tie-break on the text.
                lines.sort_by(|a, b| {
                    b.score
                        .cmp(&a.score)
                        .then_with(|| events::spans_to_plain(&a.text).cmp(&events::spans_to_plain(&b.text)))
                });
                lines.truncate(15); // vanilla only renders 15 rows
                (title, lines)
            }
            None => (Vec::new(), Vec::new()),
        };
        let key = (title.clone(), lines.clone());
        if sh.last_scoreboard.as_ref() == Some(&key) {
            None
        } else {
            sh.last_scoreboard = Some(key);
            Some((title, lines))
        }
    };
    if let Some((title, lines)) = next {
        state.emit(bot, GameEvent::Scoreboard { title, lines });
    }
}

/// Map the server `SoundSource` discriminant (0..=9) to our category.
fn map_sound_source(src: i32) -> crate::settings::SoundCategory {
    use crate::settings::SoundCategory::*;
    match src {
        1 => Music,
        2 => Records,
        3 => Weather,
        4 => Blocks,
        5 => Hostile,
        6 => Neutral,
        7 => Players,
        8 => Ambient,
        9 => Voice,
        _ => Master,
    }
}

/// Resolve a sound holder to its event path with the `minecraft:` namespace
/// stripped (e.g. `entity.zombie.ambient`), matching `sounds.json` keys.
fn sound_event_name(holder: &Holder<SoundEvent, CustomSound>) -> String {
    match holder {
        Holder::Reference(ev) => {
            let s = ev.to_str();
            s.split_once(':').map(|(_, p)| p).unwrap_or(s).to_string()
        }
        Holder::Direct(cs) => cs.sound_id.path().to_string(),
    }
}

/// Weather game events: rain start/stop and rain/thunder gradient changes.
/// Tracks the strengths in `shared.weather` and emits on change.
fn on_game_event(
    bot: &Client,
    state: &BridgeState,
    p: &azalea::protocol::packets::game::ClientboundGameEvent,
) {
    use azalea::protocol::packets::game::c_game_event::EventType;
    let emit = {
        let mut sh = state.shared.lock();
        let (mut rain, mut thunder) = sh.weather;
        match p.event {
            EventType::StartRaining => rain = 1.0,
            EventType::StopRaining => rain = 0.0,
            EventType::RainLevelChange => rain = p.param.clamp(0.0, 1.0),
            EventType::ThunderLevelChange => thunder = p.param.clamp(0.0, 1.0),
            _ => return,
        }
        if (rain, thunder) == sh.weather {
            None
        } else {
            sh.weather = (rain, thunder);
            Some((rain, thunder))
        }
    };
    if let Some((rain, thunder)) = emit {
        state.emit(bot, GameEvent::Weather { rain, thunder });
    }
}

fn on_set_time(bot: &Client, state: &BridgeState, p: &ClientboundSetTime) {
    // 26.1 moved time-of-day into per-clock states; rate 0 = frozen daylight
    // cycle (mapped to a negative value per the GameEvent contract). Servers
    // send one clock for the current dimension; fall back to world age.
    let t = p
        .clock_updates
        .values()
        .next()
        .map(|c| convert::time_of_day(c.total_ticks, c.rate))
        .unwrap_or_else(|| convert::time_of_day(p.game_time, 1.0));
    let emit = {
        let mut shared = state.shared.lock();
        let emit = match shared.last_time {
            None => true,
            // Re-emit on >= 100 tick jumps or when frozen-ness flips.
            Some(prev) => (t - prev).abs() >= 100 || (t < 0) != (prev < 0),
        };
        if emit {
            shared.last_time = Some(t);
        }
        emit
    };
    if emit {
        state.emit(bot, GameEvent::TimeOfDay { time_of_day: t });
    }
}

// ---------------------------------------------------------------------------
// Tick: commands + snapshots
// ---------------------------------------------------------------------------

fn on_tick(bot: &Client, state: &BridgeState) {
    // 1. Apply queued commands.
    while let Ok(cmd) = state.cmd_rx.try_recv() {
        apply_command(bot, state, cmd);
    }
    if state.dead.load(Ordering::Relaxed) || state.disconnecting.load(Ordering::Relaxed) {
        return;
    }

    // 1b. Server-silence watchdog. azalea keeps ticking on a dead connection
    // without ever firing Event::Disconnect (the "you time out and nothing
    // happens" freeze). If no server packet has arrived for too long, surface a
    // clean timeout and tear the client down ourselves.
    let silent = {
        let sh = state.shared.lock();
        sh.last_packet.is_some_and(|t| t.elapsed() > SERVER_SILENCE_TIMEOUT)
    };
    if silent {
        warn!("bridge: no server packet for {SERVER_SILENCE_TIMEOUT:?}; treating as timeout");
        state.disconnecting.store(true, Ordering::SeqCst);
        if !state.reported_end.swap(true, Ordering::SeqCst) {
            state.emit(bot, GameEvent::Disconnected {
                reason: "Zeitüberschreitung: Der Server antwortet nicht mehr.".into(),
            });
        }
        bot.disconnect();
        state.request_exit(bot);
        return;
    }

    // 2. Local player snapshot, every tick.
    if let Some(snap) = player_snapshot(bot) {
        state.emit(bot, GameEvent::PlayerState(Box::new(snap)));
    }

    let tick = {
        let mut shared = state.shared.lock();
        shared.tick += 1;
        shared.tick
    };

    // 3. Entity snapshots, every tick (the app interpolates between them).
    let entities = entity_snapshots(bot, state);
    state.emit(bot, GameEvent::Entities(entities));

    // 3b. Chunks whose ReceiveChunk raced azalea's world insert: retry once.
    let retry = std::mem::take(&mut state.shared.lock().retry_chunks);
    for (cx, cz) in retry {
        let sections = copy_chunk_sections(bot, state, cx, cz, None);
        if sections.is_empty() {
            debug!(x = cx, z = cz, "bridge: retried chunk still missing/empty; dropping");
            continue;
        }
        for (pos, data) in sections {
            state.emit(bot, GameEvent::Section { pos, data });
        }
    }

    // 4. Hotbar, when changed.
    maybe_emit_hotbar(bot, state);

    // 5. Open container / inventory contents, when changed.
    track_container(bot, state);

    // 6. Tab list, once per second.
    if tick.is_multiple_of(20) {
        emit_tab_list(bot, state);
    }
}

/// Current tab list → `GameEvent::TabList` (sorted by name).
fn emit_tab_list(bot: &Client, state: &BridgeState) {
    let sh = state.shared.lock();
    let mut list: Vec<TabPlayer> = bot
        .tab_list()
        .into_iter()
        .map(|(uuid, info)| {
            let (skin_url, skin_slim) = skin_of_properties(&info.profile);
            // Vanilla: the server-set display name wins; otherwise the player's
            // scoreboard team decorates the plain account name (prefix + color +
            // suffix). Either way the result is styled spans — no literal `§`.
            let display = info
                .display_name
                .as_deref()
                .map(text::spans_of)
                .unwrap_or_else(|| team_decorated(&sh, &info.profile.name));
            TabPlayer {
                uuid: uuid.to_string(),
                display,
                skin_url,
                skin_slim,
                name: info.profile.name.clone(),
                latency: info.latency,
            }
        })
        .collect();
    drop(sh);
    list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    state.emit(bot, GameEvent::TabList(list));
}

/// Decode `(skin url, slim model?)` out of a profile's base64 `textures`
/// property.
fn skin_of_properties(
    profile: &azalea::auth::game_profile::GameProfile,
) -> (Option<String>, bool) {
    let decode = || -> Option<(String, bool)> {
        let prop = profile.properties.map.get("textures")?;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(prop.value.as_bytes())
            .ok()?;
        let json: serde_json::Value = serde_json::from_slice(&raw).ok()?;
        let skin = json.get("textures")?.get("SKIN")?;
        let url = skin.get("url")?.as_str()?.to_string();
        let slim = skin
            .get("metadata")
            .and_then(|m| m.get("model"))
            .and_then(|m| m.as_str())
            == Some("slim");
        Some((url, slim))
    };
    match decode() {
        Some((url, slim)) => (Some(url), slim),
        None => (None, false),
    }
}

/// `ItemStack` → snapshot with custom name + lore (`None` for empty slots).
fn slot_snapshot(stack: &ItemStack) -> Option<ItemSnapshot> {
    let ItemStack::Present(data) = stack else {
        return None;
    };
    // A player/server-set name (anvil rename or NBT `custom_name`) wins over the
    // item's own `item_name`; both beat the default translated registry name.
    let name = data
        .get_component::<components::CustomName>()
        .map(|c| text::spans_of(&c.name))
        .or_else(|| {
            data.get_component::<components::ItemName>()
                .map(|c| text::spans_of(&c.name))
        });
    let lore = data
        .get_component::<components::Lore>()
        .map(|l| l.lines.iter().map(text::spans_of).collect())
        .unwrap_or_default();
    Some(ItemSnapshot {
        item: strip_minecraft_ns(data.kind.to_str()),
        count: data.count.max(0) as u32,
        name,
        lore,
    })
}

/// Registry-style name for a menu ("generic_9x3", "crafting", "merchant", …).
fn menu_kind_name(menu: &Menu) -> &'static str {
    match menu {
        Menu::Player { .. } => "player",
        Menu::Generic9x1 { .. } => "generic_9x1",
        Menu::Generic9x2 { .. } => "generic_9x2",
        Menu::Generic9x3 { .. } => "generic_9x3",
        Menu::Generic9x4 { .. } => "generic_9x4",
        Menu::Generic9x5 { .. } => "generic_9x5",
        Menu::Generic9x6 { .. } => "generic_9x6",
        Menu::Generic3x3 { .. } => "generic_3x3",
        Menu::Crafter3x3 { .. } => "crafter_3x3",
        Menu::Anvil { .. } => "anvil",
        Menu::Beacon { .. } => "beacon",
        Menu::BlastFurnace { .. } => "blast_furnace",
        Menu::BrewingStand { .. } => "brewing_stand",
        Menu::Crafting { .. } => "crafting",
        Menu::Enchantment { .. } => "enchantment",
        Menu::Furnace { .. } => "furnace",
        Menu::Grindstone { .. } => "grindstone",
        Menu::Hopper { .. } => "hopper",
        Menu::Lectern { .. } => "lectern",
        Menu::Loom { .. } => "loom",
        Menu::Merchant { .. } => "merchant",
        Menu::ShulkerBox { .. } => "shulker_box",
        Menu::Smithing { .. } => "smithing",
        Menu::Smoker { .. } => "smoker",
        Menu::CartographyTable { .. } => "cartography_table",
        Menu::Stonecutter { .. } => "stonecutter",
    }
}

/// Watch the Inventory component: emit ContainerOpened/Closed transitions and
/// ContainerContent whenever the visible slots (or cursor item) change.
fn track_container(bot: &Client, state: &BridgeState) {
    let Some(inv) = bot.get_component::<Inventory>() else {
        return;
    };
    let id = inv.id;
    let prev = {
        let mut shared = state.shared.lock();
        std::mem::replace(&mut shared.container_id, id)
    };
    if prev != id && prev != 0 {
        state.shared.lock().last_content = None;
        state.emit(bot, GameEvent::ContainerClosed { id: prev });
    }
    if prev != id && id != 0 {
        if let Some(menu) = &inv.container_menu {
            let title = inv
                .container_menu_title
                .as_ref()
                .map(text::spans_of)
                .unwrap_or_else(|| vec![ChatSpan::plain(menu_kind_name(menu))]);
            let slots: Vec<Option<ItemSnapshot>> =
                menu.slots().iter().map(slot_snapshot).collect();
            state.shared.lock().last_content = None;
            state.emit(bot, GameEvent::ContainerOpened {
                id,
                kind: menu_kind_name(menu).to_string(),
                title,
                slots,
            });
        }
    }

    // Content updates for whichever menu is visible (container or inventory).
    let menu: &Menu = match (&inv.container_menu, id) {
        (Some(m), n) if n != 0 => m,
        _ => &inv.inventory_menu,
    };
    let slots: Vec<Option<ItemSnapshot>> = menu.slots().iter().map(slot_snapshot).collect();
    let carried = slot_snapshot(&inv.carried);
    let key = (
        id,
        slots
            .iter()
            .map(|s| s.as_ref().map(|i| (i.item.clone(), i.count)))
            .collect::<Vec<_>>(),
        carried.as_ref().map(|i| (i.item.clone(), i.count)),
    );
    {
        let mut shared = state.shared.lock();
        if shared.last_content.as_ref() == Some(&key) {
            return;
        }
        shared.last_content = Some(key);
    }
    state.emit(bot, GameEvent::ContainerContent { id, slots, carried });
}

fn apply_command(bot: &Client, state: &BridgeState, cmd: Command) {
    match cmd {
        Command::SetDirection { yaw, pitch } => bot.set_direction(yaw, pitch),
        Command::Move { forward, strafe, sprint } => apply_move(bot, forward, strafe, sprint),
        Command::Jump(jumping) => bot.set_jumping(jumping),
        Command::Sneak(sneaking) => bot.set_crouching(sneaking),
        Command::Chat(msg) => bot.chat(msg), // leading '/' → command packet
        Command::Mine(pos) => bot.start_mining(AzBlockPos::new(pos.x, pos.y, pos.z)),
        Command::SetMining(on) => bot.left_click_mine(on),
        Command::Interact(pos) => bot.block_interact(AzBlockPos::new(pos.x, pos.y, pos.z)),
        Command::UseItem => {
            // Right-click "use": place/use the block under the crosshair, or use
            // the held item (bow, crossbow, ender pearl/snowball, eat food).
            // We deliberately skip entity interaction: azalea's entity
            // `ServerboundInteract` makes some (ViaVersion-fronted) servers kick
            // us ("Packet … was larger than I expected"). Gate on azalea's own
            // authoritative crosshair hit result so no entity packet is ever sent.
            if !matches!(bot.hit_result(), HitResult::Entity(_)) {
                bot.start_use_item();
            }
        }
        Command::ReleaseUseItem => {
            use azalea::protocol::packets::game::s_player_action::Action;
            bot.write_packet(azalea::protocol::packets::game::ServerboundPlayerAction {
                action: Action::ReleaseUseItem,
                pos: AzBlockPos::new(0, 0, 0),
                direction: azalea::core::direction::Direction::Down,
                seq: 0,
            });
        }
        Command::SwapOffhand => {
            use azalea::protocol::packets::game::s_player_action::Action;
            bot.write_packet(azalea::protocol::packets::game::ServerboundPlayerAction {
                action: Action::SwapItemWithOffhand,
                pos: AzBlockPos::new(0, 0, 0),
                direction: azalea::core::direction::Direction::Down,
                seq: 0,
            });
        }
        Command::Attack(id) => {
            // Bridge entity ids are MinecraftEntityId (i32) round-tripped
            // through u64 (see entity_snapshots).
            match bot.entity_id_by_minecraft_id(MinecraftEntityId(id as u32 as i32)) {
                Some(entity) => bot.attack(entity),
                None => warn!(id, "bridge: Attack for unknown entity id; ignoring"),
            }
        }
        Command::InteractEntity(id) => {
            match bot.entity_id_by_minecraft_id(MinecraftEntityId(id as u32 as i32)) {
                Some(entity) => bot.entity_interact(entity),
                None => warn!(id, "bridge: InteractEntity for unknown entity id; ignoring"),
            }
        }
        Command::SelectHotbar(slot) => {
            if slot <= 8 {
                bot.set_selected_hotbar_slot(slot);
            } else {
                warn!(slot, "bridge: SelectHotbar slot out of range; ignoring");
            }
        }
        Command::TabComplete { id, text } => {
            bot.write_packet(ServerboundCommandSuggestion { id, command: text });
        }
        Command::ContainerClick { window_id, slot, kind } => {
            let operation = match kind {
                SlotClickKind::Left => ClickOperation::Pickup(PickupClick::Left { slot: Some(slot) }),
                SlotClickKind::Right => {
                    ClickOperation::Pickup(PickupClick::Right { slot: Some(slot) })
                }
                SlotClickKind::QuickMove => ClickOperation::QuickMove(QuickMoveClick::Left { slot }),
                SlotClickKind::Throw => ClickOperation::Throw(ThrowClick::Single { slot }),
            };
            bot.ecs.write().trigger(ContainerClickEvent {
                entity: bot.entity,
                window_id,
                operation,
            });
        }
        Command::CloseContainer { id } => {
            bot.ecs.write().trigger(CloseContainerEvent { entity: bot.entity, id });
        }
        Command::SelectTrade { index } => {
            bot.write_packet(ServerboundSelectTrade { item: index });
        }
        Command::DropItem { all } => {
            use azalea::protocol::packets::game::s_player_action::Action;
            bot.write_packet(azalea::protocol::packets::game::ServerboundPlayerAction {
                action: if all { Action::DropAllItems } else { Action::DropItem },
                pos: AzBlockPos::new(0, 0, 0),
                direction: azalea::core::direction::Direction::Down,
                seq: 0,
            });
        }
        Command::Disconnect => {
            info!("bridge: disconnect requested");
            if state.disconnecting.swap(true, Ordering::SeqCst) {
                return; // already tearing down; ignore duplicate (e.g. handle Drop)
            }
            // Report to the app right away: azalea's teardown below is
            // best-effort (its exit path can freeze the schedule loop, see
            // request_exit) and the app must never hang waiting on it.
            if !state.reported_end.swap(true, Ordering::SeqCst) {
                state.emit(bot, GameEvent::Disconnected { reason: "disconnected".into() });
            }
            // Close the TCP connection through azalea's own in-schedule
            // DisconnectEvent path (drops RawConnection) — no ghost player on
            // the server even if the exit below stalls.
            bot.disconnect();
            // Best-effort: end start() so the bridge thread exits too.
            state.request_exit(bot);
        }
    }
}

/// Map forward/strafe signs to azalea walk/sprint directions.
/// Convention: strafe follows vanilla `leftImpulse` — +1 = left (A key),
/// -1 = right (D key). azalea 0.16's WalkDirection::Right/Left names are
/// motion-inverted (Right feeds `left_impulse += 1`, which vanilla's
/// getInputVector math moves LEFT), so mapping +1 → Right below is correct:
/// the two inversions cancel. Don't "fix" either side alone.
fn apply_move(bot: &Client, forward: i8, strafe: i8, sprint: bool) {
    if sprint && forward > 0 {
        // Sprinting only exists in forward-ish directions.
        let dir = match strafe.signum() {
            1 => SprintDirection::ForwardRight,
            -1 => SprintDirection::ForwardLeft,
            _ => SprintDirection::Forward,
        };
        bot.sprint(dir);
        return;
    }
    let dir = match (forward.signum(), strafe.signum()) {
        (1, 0) => WalkDirection::Forward,
        (1, 1) => WalkDirection::ForwardRight,
        (1, -1) => WalkDirection::ForwardLeft,
        (-1, 0) => WalkDirection::Backward,
        (-1, 1) => WalkDirection::BackwardRight,
        (-1, -1) => WalkDirection::BackwardLeft,
        (0, 1) => WalkDirection::Right,
        (0, -1) => WalkDirection::Left,
        _ => WalkDirection::None, // also cancels sprint in azalea
    };
    bot.walk(dir);
}

fn player_snapshot(bot: &Client) -> Option<PlayerSnapshot> {
    // Each accessor takes a short ECS read lock; guards are dropped
    // immediately (clone/copy out) so commands can't deadlock against them.
    let pos: Vec3 = bot.get_component::<Position>().map(|p| **p)?;
    let (velocity, on_ground) = bot
        .get_component::<Physics>()
        .map(|p| (p.velocity, p.on_ground()))
        .unwrap_or((Vec3::ZERO, false));
    let (yaw, pitch) = bot
        .get_component::<LookDirection>()
        .map(|l| (l.y_rot(), l.x_rot()))
        .unwrap_or((0.0, 0.0));
    let eye_height = bot
        .get_component::<EntityDimensions>()
        .map(|d| d.eye_height)
        .unwrap_or(1.62);
    let health = bot.get_component::<Health>().map(|h| h.0).unwrap_or(20.0);
    let absorption = bot
        .get_component::<azalea::entity::metadata::PlayerAbsorption>()
        .map(|a| a.0.max(0.0))
        .unwrap_or(0.0);
    let food = bot.get_component::<Hunger>().map(|h| h.food).unwrap_or(20);
    let (xp_level, xp_progress) = bot
        .get_component::<Experience>()
        .map(|x| (x.level, x.progress))
        .unwrap_or((0, 0.0));
    let attack_strength = bot
        .get_component::<azalea::attack::AttackStrengthScale>()
        .map(|a| a.0)
        .unwrap_or(1.0);
    // Air supply for the bubble bar. azalea defaults the component to 0 and
    // vanilla servers only send it once it changes — the app treats "never
    // saw a change while not diving" as full (see drain_game_events).
    let air = bot
        .get_component::<azalea::entity::metadata::AirSupply>()
        .map(|a| a.0)
        .unwrap_or(300);
    let eyes_in_water = bot
        .get_component::<azalea::entity::FluidOnEyes>()
        .map(|f| **f == azalea::block::fluid_state::FluidKind::Water)
        .unwrap_or(false);
    let eyes_in_lava = bot
        .get_component::<azalea::entity::FluidOnEyes>()
        .map(|f| **f == azalea::block::fluid_state::FluidKind::Lava)
        .unwrap_or(false);
    // The item-use bitflag (eating/drinking/bow draw/shield block/spyglass).
    let using_item = bot
        .get_component::<azalea::entity::metadata::AbstractLivingUsingItem>()
        .map(|u| u.0)
        .unwrap_or(false);
    let on_fire = bot
        .get_component::<azalea::entity::metadata::OnFire>()
        .map(|f| f.0)
        .unwrap_or(false);
    // Powder-snow freeze: vanilla fully freezes at 140 ticks in powder snow.
    let freeze = bot
        .get_component::<azalea::entity::metadata::TicksFrozen>()
        .map(|t| (t.0.max(0) as f32 / 140.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let swimming = bot
        .get_component::<Pose>()
        .map(|p| *p == Pose::Swimming)
        .unwrap_or(false);
    let riding = bot.get_component::<plugins::RidingVehicle>().is_some();
    // Hold-to-mine state (azalea's MiningPlugin): target + progress drive the
    // crack overlay and the mining hit/break sounds app-side.
    let mining = match (
        bot.get_component::<azalea::mining::Mining>(),
        bot.get_component::<azalea::mining::MineProgress>(),
    ) {
        (Some(mining), progress) => Some((
            BlockPos { x: mining.pos.x, y: mining.pos.y, z: mining.pos.z },
            progress.map(|p| p.0).unwrap_or(0.0),
        )),
        _ => None,
    };
    Some(PlayerSnapshot {
        pos: [pos.x, pos.y, pos.z],
        velocity: [velocity.x, velocity.y, velocity.z],
        yaw,
        pitch,
        eye_height,
        on_ground,
        health,
        absorption,
        food,
        xp_level,
        xp_progress,
        attack_strength,
        air,
        eyes_in_water,
        eyes_in_lava,
        using_item,
        on_fire,
        freeze,
        swimming,
        riding,
        mining,
        equipment: read_own_equipment(bot),
    })
}

/// The local player's worn armor + offhand, read from the inventory menu's armor
/// slots (5..=8 = head, chest, legs, feet). Hands come from the hotbar on the
/// app side, so only armor + offhand are filled here.
fn read_own_equipment(bot: &Client) -> Equipment {
    let Some(inv) = bot.get_component::<Inventory>() else {
        return Equipment::default();
    };
    let menu = &inv.inventory_menu;
    let name_at = |idx: usize| -> Option<String> {
        match menu.slot(idx)? {
            ItemStack::Present(d) => Some(strip_minecraft_ns(d.kind.to_str())),
            ItemStack::Empty => None,
        }
    };
    // Armor slots 5..=8 = head, chest, legs, feet (vanilla container order).
    let armor0 = *Player::ARMOR_SLOTS.start();
    Equipment {
        head: name_at(armor0),
        chest: name_at(armor0 + 1),
        legs: name_at(armor0 + 2),
        feet: name_at(armor0 + 3),
        main_hand: None,
        off_hand: name_at(Player::OFFHAND_SLOT),
    }
}

/// Snapshot remote entities with a position within ~128 blocks.
fn entity_snapshots(bot: &Client, state: &BridgeState) -> Vec<EntitySnapshot> {
    const RANGE_SQ: f64 = 128.0 * 128.0;
    // Read our own pos/world BEFORE taking the write lock below
    // (parking_lot RwLock is not reentrant).
    let my_pos: Option<Vec3> = bot.get_component::<Position>().map(|p| **p);
    let my_world: Option<WorldName> = bot.get_component::<WorldName>().map(|w| w.clone());

    // Registry-driven variant names, resolved once per snapshot (tiny maps).
    // Read before taking the ecs write lock (world lock is separate).
    let cat_reg = read_variant_registry(bot, "cat_variant");
    let wolf_reg = read_variant_registry(bot, "wolf_variant");
    let cow_reg = read_variant_registry(bot, "cow_variant");
    let chicken_reg = read_variant_registry(bot, "chicken_variant");
    let pig_reg = read_variant_registry(bot, "pig_variant");
    let frog_reg = read_variant_registry(bot, "frog_variant");
    let painting_reg = read_painting_variants(bot);

    let mut out = Vec::new();
    let mut ecs = bot.ecs.write();
    // The core columns sit at bevy's 15-tuple limit, so the per-species variant
    // columns (rabbit colour, fox type, parrot/llama/axolotl/horse variant,
    // mooshroom/shulker colour…) go in a second, nested tuple.
    let mut query = ecs.query::<(
        (
            Entity,
            &MinecraftEntityId,
            &EntityKindComponent,
            &Position,
            &LookDirection,
            &WorldName,
            Option<&EntityDimensions>,
            Option<&CustomName>,
            Option<&GameProfileComponent>,
            Option<&LocalEntity>,
            Option<&azalea::entity::metadata::ItemItem>,
            Option<&Pose>,
            Option<&Sprinting>,
            Option<&Invisible>,
            Option<&azalea::entity::metadata::AbstractAgeableBaby>,
        ),
        (
            Option<&azalea::entity::metadata::RabbitKind>,
            Option<&azalea::entity::metadata::FoxKind>,
            Option<&azalea::entity::metadata::ParrotVariant>,
            Option<&azalea::entity::metadata::LlamaVariant>,
            Option<&azalea::entity::metadata::AxolotlVariant>,
            Option<&azalea::entity::metadata::HorseTypeVariant>,
            Option<&azalea::entity::metadata::MooshroomKind>,
            Option<&azalea::entity::metadata::Color>,
            Option<&azalea::entity::metadata::SalmonKind>,
            Option<&azalea::entity::metadata::TropicalFishTypeVariant>,
            // Appearance/state extras (0.50.0): on-fire flame, pet collar dye,
            // charged creeper. `Tame` gates the collar so wild pets show none.
            Option<&azalea::entity::metadata::OnFire>,
            Option<&azalea::entity::metadata::CatCollarColor>,
            Option<&azalea::entity::metadata::WolfCollarColor>,
            Option<&azalea::entity::metadata::Tame>,
            Option<&azalea::entity::metadata::IsPowered>,
        ),
        (
            Option<&azalea::entity::metadata::CatVariant>,
            Option<&azalea::entity::metadata::WolfVariant>,
            Option<&azalea::entity::metadata::CowVariant>,
            Option<&azalea::entity::metadata::ChickenVariant>,
            Option<&azalea::entity::metadata::PigVariant>,
            Option<&azalea::entity::metadata::FrogVariant>,
            Option<&azalea::entity::metadata::VillagerVillagerData>,
            Option<&azalea::entity::metadata::PaintingVariant>,
            Option<&azalea::entity::metadata::PaintingDirection>,
            Option<&azalea::entity::metadata::ItemFrameItem>,
            Option<&azalea::entity::metadata::ItemFrameDirection>,
            Option<&azalea::entity::metadata::Rotation>,
        ),
        (
            Option<&azalea::entity::metadata::Text>,
            Option<&azalea::entity::metadata::BlockDisplayBlockState>,
            Option<&azalea::entity::metadata::ItemDisplayItemStack>,
            Option<&azalea::entity::metadata::Translation>,
            Option<&azalea::entity::metadata::Scale>,
            Option<&azalea::entity::metadata::LeftRotation>,
            Option<&azalea::entity::metadata::RightRotation>,
        ),
        (
            Option<&azalea::entity::metadata::Small>,
            Option<&azalea::entity::metadata::ShowArms>,
            Option<&azalea::entity::metadata::ShowBasePlate>,
            Option<&azalea::entity::metadata::HeadPose>,
            Option<&azalea::entity::metadata::BodyPose>,
            Option<&azalea::entity::metadata::LeftArmPose>,
            Option<&azalea::entity::metadata::RightArmPose>,
            Option<&azalea::entity::metadata::LeftLegPose>,
            Option<&azalea::entity::metadata::RightLegPose>,
        ),
    )>();
    for (
        (
            ent,
            mc_id,
            kind,
            pos,
            look,
            world_name,
            dims,
            custom_name,
            profile,
            local,
            item,
            pose,
            sprinting,
            invisible,
            baby,
        ),
        (
            rabbit_v, fox_v, parrot_v, llama_v, axolotl_v, horse_v, mooshroom_v, shulker_color,
            salmon_v, tropical_v,
            on_fire_c, cat_collar, wolf_collar, tame_c, powered_c,
        ),
        (
            cat_v, wolf_v, cow_v, chicken_v, pig_v, frog_v, villager_v, painting_v, painting_dir,
            frame_item, frame_dir, frame_rot,
        ),
        (disp_text, disp_block, disp_item, disp_translation, disp_scale, disp_left, disp_right),
        (
            as_small, as_arms, as_base, as_head, as_body, as_larm, as_rarm, as_lleg, as_rleg,
        ),
    ) in query.iter(&ecs)
    {
        if ent == bot.entity || local.is_some() {
            continue; // our own player (or another local swarm client)
        }
        if let Some(my_world) = &my_world
            && my_world != world_name
        {
            continue; // other dimension
        }
        if let Some(mp) = my_pos {
            let (dx, dy, dz) = (pos.x - mp.x, pos.y - mp.y, pos.z - mp.z);
            if dx * dx + dy * dy + dz * dz > RANGE_SQ {
                continue;
            }
        }
        let kind = kind.0;
        let kind_name = strip_minecraft_ns(kind.to_str());
        // Per-species variant index (default 0). Shulker uses its dye Color
        // (0..15), where 16/None means "no dye" → the default purple texture.
        let variant = match kind_name.as_str() {
            "rabbit" => rabbit_v.map(|v| v.0).unwrap_or(0),
            "fox" => fox_v.map(|v| v.0).unwrap_or(0),
            "parrot" => parrot_v.map(|v| v.0).unwrap_or(0),
            "llama" | "trader_llama" => llama_v.map(|v| v.0).unwrap_or(0),
            "axolotl" => axolotl_v.map(|v| v.0).unwrap_or(0),
            "horse" => horse_v.map(|v| v.0).unwrap_or(0),
            "mooshroom" => mooshroom_v.map(|v| v.0).unwrap_or(0),
            "shulker" => shulker_color.map(|c| c.0 as i32).unwrap_or(16),
            // Salmon: 0 small, 1 medium, 2 large (drives the render scale).
            "salmon" => salmon_v.map(|v| v.0).unwrap_or(1),
            // Tropical fish: the packed variant int (shape|pattern|colours) —
            // the app decodes it to pick the composited body/pattern texture.
            "tropical_fish" => tropical_v.map(|v| v.0).unwrap_or(0),
            _ => 0,
        };
        // Registry-driven variant name (cat/wolf/cow/chicken/pig/frog): the
        // metadata carries a protocol id → look up the name in the registry.
        use azalea::registry::DataRegistry as _;
        let variant_name = match kind_name.as_str() {
            "cat" => cat_v.and_then(|v| cat_reg.get(v.0.protocol_id() as usize).cloned()),
            "wolf" => wolf_v.and_then(|v| wolf_reg.get(v.0.protocol_id() as usize).cloned()),
            "cow" => cow_v.and_then(|v| cow_reg.get(v.0.protocol_id() as usize).cloned()),
            "chicken" => chicken_v.and_then(|v| chicken_reg.get(v.0.protocol_id() as usize).cloned()),
            "pig" => pig_v.and_then(|v| pig_reg.get(v.0.protocol_id() as usize).cloned()),
            "frog" => frog_v.and_then(|v| frog_reg.get(v.0.protocol_id() as usize).cloned()),
            // Villager: encode the three appearance layers (biome type, trade
            // profession, badge level) into one "type|profession|level" key that
            // the app maps to a pre-composited texture. none/nitwit wear no
            // badge, so their level is pinned to 0. VillagerKind/Profession are
            // builtin enums whose `to_str()` already yields the texture name.
            "villager" => villager_v.map(|v| {
                let vt = v.0.kind.to_str();
                let prof = v.0.profession.to_str();
                let employed = prof != "none" && prof != "nitwit";
                let lvl = if employed { v.0.level.clamp(1, 5) } else { 0 };
                format!("{vt}|{prof}|{lvl}")
            }),
            _ => None,
        };
        // Painting: look up its art + size in the painting_variant registry by
        // protocol id, then attach the wall direction it faces.
        let painting = if kind_name == "painting" {
            painting_v.and_then(|v| painting_reg.get(v.0.protocol_id() as usize).cloned()).map(
                |mut info| {
                    info.facing = painting_dir.map(|d| d.0 as u8).unwrap_or(3);
                    info
                },
            )
        } else {
            None
        };
        // Item frame: its held item, rotation and wall direction. The glow
        // variant is a distinct entity kind, so the kind name flags it.
        let frame = if kind_name == "item_frame" || kind_name == "glow_item_frame" {
            let item = frame_item.and_then(|i| match &i.0 {
                ItemStack::Present(d) => Some(strip_minecraft_ns(d.kind.to_str())),
                ItemStack::Empty => None,
            });
            Some(events::FrameInfo {
                item,
                rot: frame_rot.map(|r| (r.0 & 7) as u8).unwrap_or(0),
                facing: frame_dir.map(|d| d.0 as u8).unwrap_or(3),
                glow: kind_name == "glow_item_frame",
            })
        } else {
            None
        };
        // Display entities: block/item carry a transform + payload; text rides
        // on name_spans (rendered like a nametag → a floating hologram).
        let display = if kind_name == "block_display" || kind_name == "item_display" {
            Some(events::DisplayInfo {
                translation: disp_translation.map(|t| [t.0.x, t.0.y, t.0.z]).unwrap_or([0.0; 3]),
                scale: disp_scale.map(|s| [s.0.x, s.0.y, s.0.z]).unwrap_or([1.0; 3]),
                left_rot: disp_left.map(|q| [q.0.x, q.0.y, q.0.z, q.0.w]).unwrap_or([0.0, 0.0, 0.0, 1.0]),
                right_rot: disp_right.map(|q| [q.0.x, q.0.y, q.0.z, q.0.w]).unwrap_or([0.0, 0.0, 0.0, 1.0]),
                block_state: disp_block.map(|b| b.0.id() as u32),
                item: disp_item.and_then(|i| match &i.0 {
                    ItemStack::Present(d) => Some(strip_minecraft_ns(d.kind.to_str())),
                    ItemStack::Empty => None,
                }),
            })
        } else {
            None
        };
        // Armor stand: appearance flags + the six part pose rotations (degrees).
        let armor_stand = if kind_name == "armor_stand" {
            let rot = |r: Option<&azalea::entity::Rotations>, d: [f32; 3]| {
                r.map(|v| [v.x, v.y, v.z]).unwrap_or(d)
            };
            Some(events::ArmorStandInfo {
                small: as_small.map(|s| s.0).unwrap_or(false),
                show_arms: as_arms.map(|s| s.0).unwrap_or(false),
                show_base: as_base.map(|s| s.0).unwrap_or(true),
                head: rot(as_head.map(|p| &p.0), [0.0, 0.0, 0.0]),
                body: rot(as_body.map(|p| &p.0), [0.0, 0.0, 0.0]),
                left_arm: rot(as_larm.map(|p| &p.0), [-10.0, 0.0, -10.0]),
                right_arm: rot(as_rarm.map(|p| &p.0), [-15.0, 0.0, 10.0]),
                left_leg: rot(as_lleg.map(|p| &p.0), [-1.0, 0.0, -1.0]),
                right_leg: rot(as_rleg.map(|p| &p.0), [1.0, 0.0, 1.0]),
            })
        } else {
            None
        };
        // On-fire flame, pet collar dye (tamed cats/wolves only) and charged
        // creeper — small appearance/state extras the app overlays on the model.
        let on_fire = on_fire_c.map(|f| f.0).unwrap_or(false);
        let tamed = tame_c.map(|t| t.0).unwrap_or(false);
        let collar = if tamed {
            match kind_name.as_str() {
                "cat" => cat_collar.map(|c| c.0),
                "wolf" => wolf_collar.map(|c| c.0),
                _ => None,
            }
        } else {
            None
        };
        let powered = kind_name == "creeper" && powered_c.map(|p| p.0).unwrap_or(false);
        let name = profile.map(|p| p.name.clone()).or_else(|| {
            // A text_display's Text is its hologram label (rendered as a nametag).
            disp_text
                .map(|t| convert::strip_legacy_codes(&t.0.to_string()))
                .or_else(|| {
                    custom_name
                        .and_then(|c| c.0.as_ref())
                        .map(|t| convert::strip_legacy_codes(&t.to_string()))
                })
        });
        // Styled name to draw over the entity. A text_display's Text, a server-set
        // custom_name (NPCs, holograms) keeps its component colors; a plain player
        // gets its scoreboard-team prefix/color/suffix (vanilla nametag behaviour).
        // The spans never carry raw `§` — colors live on the span.
        let name_spans = {
            let sh = state.shared.lock();
            disp_text
                .map(|t| text::spans_of(&t.0))
                .or_else(|| custom_name.and_then(|c| c.0.as_ref()).map(|t| text::spans_of(t)))
                .or_else(|| profile.map(|p| team_decorated(&sh, &p.name)))
        };
        // Skin straight off the entity's profile so server NPCs (never in the
        // tab list) still render with their real skin.
        let (skin_url, skin_slim) = profile
            .map(|p| skin_of_properties(p))
            .unwrap_or((None, false));
        let sneaking = matches!(pose, Some(Pose::Crouching));
        let sprinting = sprinting.map(|s| s.0).unwrap_or(false);
        let invisible = invisible.map(|i| i.0).unwrap_or(false);
        // Dropped-item entities carry their stack as metadata; pull the item's
        // registry name so the app can draw its real icon.
        let item = item.and_then(|i| match &i.0 {
            ItemStack::Present(d) => Some(strip_minecraft_ns(d.kind.to_str())),
            ItemStack::Empty => None,
        });
        out.push(EntitySnapshot {
            id: mc_id.0 as u32 as u64,
            kind: kind_name,
            pos: [pos.x, pos.y, pos.z],
            yaw: look.y_rot(),
            pitch: look.x_rot(),
            width: dims.map(|d| d.width).unwrap_or(0.6),
            height: dims.map(|d| d.height).unwrap_or(1.8),
            name,
            name_spans,
            is_player: kind == EntityKind::Player,
            sneaking,
            sprinting,
            invisible,
            baby: baby.map(|b| b.0).unwrap_or(false),
            uuid: profile.map(|p| p.uuid.to_string()),
            skin_url,
            skin_slim,
            equipment: Equipment::default(),
            item,
            variant,
            variant_name,
            painting,
            frame,
            display,
            armor_stand,
            on_fire,
            collar,
            powered,
        });
    }
    drop(ecs);

    // Attach tracked equipment and drop entries for entities that despawned.
    {
        let mut sh = state.shared.lock();
        if !sh.entity_equipment.is_empty() {
            let live: std::collections::HashSet<u64> = out.iter().map(|e| e.id).collect();
            sh.entity_equipment.retain(|k, _| live.contains(k));
            for e in &mut out {
                if let Some(eq) = sh.entity_equipment.get(&e.id) {
                    e.equipment = eq.clone();
                }
            }
        }
    }
    out
}

fn strip_minecraft_ns(s: &str) -> String {
    s.strip_prefix("minecraft:").unwrap_or(s).to_string()
}

fn maybe_emit_hotbar(bot: &Client, state: &BridgeState) {
    let Some((slots, offhand, selected)) = read_hotbar(bot) else {
        return;
    };
    let key: (Vec<Option<(String, u32)>>, Option<(String, u32)>, u8) = (
        slots
            .iter()
            .map(|s| s.as_ref().map(|i| (i.item.clone(), i.count)))
            .collect(),
        offhand.as_ref().map(|i| (i.item.clone(), i.count)),
        selected,
    );
    {
        let mut shared = state.shared.lock();
        if shared.last_hotbar.as_ref() == Some(&key) {
            return;
        }
        shared.last_hotbar = Some(key);
    }
    state.emit(bot, GameEvent::Hotbar { slots, offhand, selected });
}

fn read_hotbar(
    bot: &Client,
) -> Option<(Box<[Option<ItemSnapshot>; 9]>, Option<ItemSnapshot>, u8)> {
    let inv = bot.get_component::<Inventory>()?;
    let selected = inv.selected_hotbar_slot;
    // Always the player inventory (slots 36..=44), not any open container.
    let menu = &inv.inventory_menu;
    let mut slots: Box<[Option<ItemSnapshot>; 9]> = Box::new(std::array::from_fn(|_| None));
    for (i, idx) in menu.hotbar_slots_range().enumerate().take(9) {
        slots[i] = menu.slot(idx).and_then(slot_snapshot);
    }
    let offhand = menu.slot(Player::OFFHAND_SLOT).and_then(slot_snapshot);
    Some((slots, offhand, selected))
}

// ---------------------------------------------------------------------------
// Live end-to-end test (skips when no local server is running)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod live_tests {
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    use super::*;

    /// Connects to the local offline-mode test server, waits for Login and
    /// chunk data, asserts the section copies contain real blocks (and light),
    /// then disconnects. Proves the world-copy path against a real 26.1
    /// server. Skips (passes) when nothing listens on localhost:25565.
    #[test]
    fn live_connect_and_copy_sections() {
        let addr = "127.0.0.1:25565";
        if TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(2)).is_err() {
            eprintln!("skipping live bridge test: no server on {addr}");
            return;
        }

        let (handle, rx) = spawn_bridge(BridgeOptions {
            account: AccountConfig::Offline("BridgeTest".into()),
            address: addr.into(),
            view_distance: 8,
        })
        .expect("spawn_bridge");

        let deadline = Instant::now() + Duration::from_secs(40);
        let mut connected = false;
        let mut sections = 0usize;
        let mut sections_with_blocks = 0usize;
        let mut sections_with_light = 0usize;
        let mut got_player_state = false;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(GameEvent::Connected { username }) => {
                    assert_eq!(username, "BridgeTest");
                    connected = true;
                }
                Ok(GameEvent::Section { pos, data }) => {
                    sections += 1;
                    assert!(pos.y >= -128 && pos.y < 128, "insane section y: {pos:?}");
                    if data.blocks.iter().any(|&b| b != 0) {
                        sections_with_blocks += 1;
                    }
                    if data.sky_light.is_some() || data.block_light.is_some() {
                        sections_with_light += 1;
                    }
                }
                Ok(GameEvent::PlayerState(_)) => got_player_state = true,
                Ok(GameEvent::Disconnected { reason }) => panic!("disconnected early: {reason}"),
                Ok(_) => {}
                Err(_) => {}
            }
            if connected && got_player_state && sections >= 20 {
                break;
            }
        }
        assert!(connected, "never received Connected");
        assert!(got_player_state, "never received PlayerState");
        assert!(sections >= 1, "no Section events received");
        assert!(
            sections_with_blocks >= 1,
            "all {sections} sections were air — block copy is broken"
        );
        eprintln!(
            "live bridge test: {sections} sections ({sections_with_blocks} with blocks, \
             {sections_with_light} with light)"
        );

        handle.send(Command::Disconnect);
        let end = Instant::now() + Duration::from_secs(10);
        let mut disconnected = false;
        while Instant::now() < end {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(GameEvent::Disconnected { .. }) => {
                    disconnected = true;
                    break;
                }
                Ok(_) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Err(_) => {}
            }
        }
        assert!(disconnected, "no Disconnected after Command::Disconnect");
    }

    /// Wait for a `BlockChanged` at `pos`, returning its new state id (or None
    /// on timeout). Panics if the server disconnects first.
    fn wait_block_change(
        rx: &Receiver<GameEvent>,
        pos: BlockPos,
        timeout: Duration,
    ) -> Option<StateId> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(GameEvent::BlockChanged { pos: p, state }) if p == pos => return Some(state),
                Ok(GameEvent::Disconnected { reason }) => panic!("disconnected: {reason}"),
                _ => {}
            }
        }
        None
    }

    /// End-to-end interaction against the local server as the opped creative
    /// player "Dolphin": right-click-place a block, then mine it back to air —
    /// proving `Command::Interact` and `Command::Mine` reach the world. Skips
    /// (passes) when nothing listens on localhost:25565.
    #[test]
    fn live_mine_and_place() {
        let addr = "127.0.0.1:25565";
        if TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(2)).is_err() {
            eprintln!("skipping live interaction test: no server on {addr}");
            return;
        }

        // "Dolphin" is opped in the test server (offline UUID) → creative + commands.
        let (handle, rx) = spawn_bridge(BridgeOptions {
            account: AccountConfig::Offline("Dolphin".into()),
            address: addr.into(),
            view_distance: 8,
        })
        .expect("spawn_bridge");

        // Wait for the first player position.
        let mut pos = None;
        let deadline = Instant::now() + Duration::from_secs(40);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(GameEvent::PlayerState(p)) => {
                    pos = Some(p.pos);
                    break;
                }
                Ok(GameEvent::Disconnected { reason }) => panic!("disconnected early: {reason}"),
                _ => {}
            }
        }
        let pos = pos.expect("never received PlayerState");
        let (px, py, pz) = (pos[0].floor() as i32, pos[1].floor() as i32, pos[2].floor() as i32);

        // A solid block to place against (2 east, at foot level) and the empty
        // cell above it where the placed block will land.
        let ground = BlockPos { x: px + 2, y: py - 1, z: pz };
        let place = BlockPos { x: px + 2, y: py, z: pz };
        // Creative → block placement doesn't consume and mining is instant.
        handle.send(Command::Chat("/gamemode creative Dolphin".into()));
        handle.send(Command::Chat(
            "/item replace entity Dolphin hotbar.0 with minecraft:stone 64".into(),
        ));
        handle.send(Command::Chat(format!(
            "/setblock {} {} {} minecraft:stone",
            ground.x, ground.y, ground.z
        )));
        handle.send(Command::Chat(format!(
            "/setblock {} {} {} minecraft:air",
            place.x, place.y, place.z
        )));
        std::thread::sleep(Duration::from_millis(1500)); // inventory + blocks settle
        while rx.try_recv().is_ok() {} // drain setup events

        // Place: right-clicking the ground block puts stone on its top face.
        handle.send(Command::Interact(ground));
        let placed = wait_block_change(&rx, place, Duration::from_secs(10));
        assert!(
            placed.is_some_and(|s| s != 0),
            "Interact did not place a block at {place:?} (got {placed:?})"
        );

        // Look at the placed block (as the real app does before mining), then
        // break it (creative → instant).
        let eye = [pos[0], pos[1] + 1.62, pos[2]];
        let (dx, dy, dz) = (
            place.x as f64 + 0.5 - eye[0],
            place.y as f64 + 0.5 - eye[1],
            place.z as f64 + 0.5 - eye[2],
        );
        let yaw = (dz.atan2(dx).to_degrees() - 90.0) as f32;
        let pitch = (-dy.atan2((dx * dx + dz * dz).sqrt()).to_degrees()) as f32;
        handle.send(Command::SetDirection { yaw, pitch });
        std::thread::sleep(Duration::from_millis(400));
        while rx.try_recv().is_ok() {} // drop late placement echoes + player states

        // Break it, then wait specifically for the cell to become air (ignoring
        // any lingering non-air confirmations).
        handle.send(Command::Mine(place));
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut mined_air = false;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(GameEvent::BlockChanged { pos: p, state: 0 }) if p == place => {
                    mined_air = true;
                    break;
                }
                Ok(GameEvent::Disconnected { reason }) => panic!("disconnected: {reason}"),
                _ => {}
            }
        }
        assert!(mined_air, "Mine did not clear the placed block to air within 10s");

        eprintln!("live interaction: placed {placed:?} then mined to air at {place:?}");
        handle.send(Command::Disconnect);
    }
}
