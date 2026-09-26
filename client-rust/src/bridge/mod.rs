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

pub mod blockentity;
pub mod dialog;
pub mod events;
pub mod recipe;
pub mod text;

mod account;
mod convert;
mod plugins;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
    ClientboundAnimate, ClientboundBossEvent, ClientboundGamePacket, ClientboundHurtAnimation,
    ClientboundLevelParticles, ClientboundResetScore,
    ClientboundSetDisplayObjective, ClientboundSetEquipment,
    ClientboundSetObjective, ClientboundPlayerLookAt, ClientboundSetPlayerTeam,
    ClientboundSetScore, ClientboundSetTime, ClientboundWaypoint,
};
use azalea::protocol::packets::game::c_waypoint::{
    WaypointData, WaypointIdentifier, WaypointOperation,
};
use azalea::chat::ChatPacket;
use azalea::protocol::packets::game::c_custom_chat_completions::Action as ChatCompletionActionPacket;
use azalea::protocol::packets::game::c_player_chat::PackedMessageSignature;
use azalea::protocol::common::server_links::{KnownLinkKind, ServerLinkKind};
use azalea::protocol::packets::game::s_player_command;
use azalea::core::sound::CustomSound;
use azalea::registry::Holder;
use azalea::registry::builtin::{EntityKind, GameRule, SoundEvent};
use azalea::inventory::{CloseContainerEvent, ContainerClickEvent};
use azalea::protocol::packets::game::{
    ServerboundBundleItemSelected, ServerboundCommandSuggestion, ServerboundSelectTrade,
};
use azalea::protocol::packets::game::s_recipe_book_change_settings::RecipeBookType;
use azalea::protocol::packets::game::ServerboundRecipeBookChangeSettings;
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
    AccountConfig, AnimalPose, BlockEntityInfo, BossBar, BossBarUpdate, BridgeOptions,
    ChatCompletionAction, ChatSpan, Command, EntityPose, EntitySnapshot, Equipment, GameEvent,
    InstrumentDesc, ItemSnapshot, MsgSig, PackedSig, PlayerSnapshot, RecipeBookKind, ScoreLine,
    ServerLink, SlotClickKind, StonecutterRecipe, FireworkStar, TabPlayer, TitlePart, TradeOffer,
};

/// How long the server may go completely silent before we treat the connection
/// as timed out. azalea's schedule keeps ticking on a dead socket without ever
/// firing `Event::Disconnect`, so without this the client sits frozen forever.
/// Kept below the app-side backstop watchdog so this (with its clean message)
/// wins when the bridge thread itself is still alive. 30s matches vanilla's
/// read-timeout feel and clears two missed 15s keep-alives, so a brief lag
/// spike on a live server never trips it.
const SERVER_SILENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
static PACK_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Handle the app uses to control the game. Dropping it disconnects.
pub struct GameHandle {
    cmd_tx: Sender<Command>,
    /// Resource-pack choices/results must also flow during configuration, when
    /// azalea emits no game ticks and the ordinary command queue is idle.
    pack_cmd_tx: Sender<Command>,
}

impl GameHandle {
    /// Queue a command; applied on the next tick. Never blocks.
    pub fn send(&self, cmd: Command) {
        // Unbounded channel: send never blocks. If the bridge thread is gone
        // the command is meaningless anyway — drop it silently.
        if matches!(
            &cmd,
            Command::ResourcePackResponse { .. } | Command::ResourcePackApplied { .. }
        ) {
            let _ = self.pack_cmd_tx.send(cmd);
        } else {
            let _ = self.cmd_tx.send(cmd);
        }
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
                    format!("DNS lookup for \"{address}\" timed out.");
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
    let (pack_cmd_tx, pack_cmd_rx) = crossbeam_channel::unbounded::<Command>();
    let (pack_download_tx, pack_download_rx) =
        crossbeam_channel::unbounded::<PackDownloadResult>();

    let state = BridgeState {
        event_tx: event_tx.clone(),
        cmd_rx,
        pack_cmd_rx,
        pack_download_tx,
        pack_download_rx,
        shared: Arc::new(Mutex::new(Shared::default())),
        dead: Arc::new(AtomicBool::new(false)),
        reported_end: Arc::new(AtomicBool::new(false)),
        disconnecting: Arc::new(AtomicBool::new(false)),
        exited: Arc::new(AtomicBool::new(false)),
        exit_task_spawned: Arc::new(AtomicBool::new(false)),
        view_distance: opts.view_distance.clamp(2, 32),
        resource_pack_policy: opts.resource_pack_policy,
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
                    .add_plugins(
                        azalea::bot::DefaultBotPlugins
                            .build()
                            .disable::<azalea::accept_resource_packs::AcceptResourcePacksPlugin>(),
                    )
                    .add_plugins(plugins::DolphinBrandPlugin)
                    .add_plugins(plugins::DolphinResourcePackPlugin)
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
                        "The connection ended unexpectedly (details in the log)".into()
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

    Ok((GameHandle { cmd_tx, pack_cmd_tx }, event_rx))
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
    /// Teams: name → everything the team says about how its members look.
    /// Modern minigame servers put the visible sidebar text in team
    /// prefix/suffix, keyed by a dummy owner.
    sb_teams: HashMap<String, Team>,
    /// Which team each scoreboard owner (entry) belongs to.
    sb_member_team: HashMap<String, String>,
    /// Dedupe key for the last emitted sidebar (title, rows).
    last_scoreboard: Option<(Vec<ChatSpan>, Vec<ScoreLine>)>,
    /// Per-entity equipment from SetEquipment, keyed by MinecraftEntityId as u64.
    /// Pruned each tick against the live entity set in `entity_snapshots`.
    entity_equipment: HashMap<u64, Equipment>,
    /// AddEntity "object data" per entity id. azalea drops it when it spawns the
    /// entity, but it is the only source for a falling block's block state and
    /// for a projectile's / fishing bobber's owner, so it is captured raw.
    /// Pruned alongside `entity_equipment`.
    spawn_data: HashMap<u64, i32>,
    /// Leashes from SetEntityLink: leashed entity id → holder entity id
    /// (`None` in the packet means the lead was cut, which removes the entry).
    leashes: HashMap<u64, u64>,
    /// Max health per entity, from UpdateAttributes. It is an *attribute*, not
    /// metadata, so azalea never surfaces it — and without it the hearts over a
    /// mount's health bar would all be guesswork.
    max_health: HashMap<u64, f32>,
    /// Head yaw per entity, degrees. Vanilla splits body and head rotation and
    /// sends the head on its own packet; azalea keeps only the body.
    head_yaw: HashMap<u64, f32>,
    /// Riders from SetPassengers: passenger entity id → (vehicle id, seat
    /// index). azalea ignores the packet, so remote riders would otherwise
    /// stand inside their boat instead of on it.
    riders: HashMap<u64, (u64, u8)>,
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
    /// The server's trim-pattern and trim-material registries, by protocol id.
    /// Armour stacks carry only the ids, so this is what turns them into the
    /// texture names an armour trim is drawn from.
    trim_patterns: Vec<String>,
    trim_materials: Vec<String>,
    /// The server's instrument registry, by protocol id — a goat horn's
    /// tooltip needs this to turn its `Holder::Reference` id into a name.
    instruments: Vec<String>,
    /// The world border. Five of the six border packets only change one field
    /// of it, so the whole thing is kept here and re-sent on every change.
    border: events::WorldBorderUpdate,
    /// The open mount (horse/donkey/llama) inventory. azalea has no menu for
    /// it, so its container id, its slots and its click packets are all ours.
    mount: Option<MountScreen>,
}

/// A scoreboard team, as far as anything visible is concerned: what wraps its
/// members' names, what colour they are, and whether their nametag shows.
#[derive(Clone, Debug, Default)]
struct Team {
    prefix: Vec<ChatSpan>,
    suffix: Vec<ChatSpan>,
    /// The team colour applied to the member's own name (`None` = no colour).
    color: Option<[u8; 3]>,
    /// Vanilla's `NameTagVisibility::Never` — the tag is not drawn at all.
    /// The two "hide for …" rules need our own team, so they are resolved
    /// where the tag is built.
    hide_names: NameTagRule,
}

/// Vanilla's four nametag-visibility rules.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum NameTagRule {
    #[default]
    Always,
    Never,
    HideForOtherTeams,
    HideForOwnTeam,
}

/// The mount inventory screen we are keeping ourselves.
#[derive(Clone, Debug)]
struct MountScreen {
    container_id: i32,
    /// Slots as the server last sent them: the mount's own slots followed by
    /// the 36 player slots, exactly the order the click packet uses.
    slots: Vec<Option<ItemSnapshot>>,
    carried: Option<ItemSnapshot>,
    /// The container state id the server last stamped — echoed back on clicks.
    state_id: u32,
}

/// azalea handler state: must be `Default + Clone + Component` (the handler is
/// a plain `fn`, so all context lives here).
#[derive(Clone, Component)]
struct BridgeState {
    event_tx: Sender<GameEvent>,
    cmd_rx: Receiver<Command>,
    pack_cmd_rx: Receiver<Command>,
    pack_download_tx: Sender<PackDownloadResult>,
    pack_download_rx: Receiver<PackDownloadResult>,
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
    resource_pack_policy: crate::settings::ServerResourcePackPolicy,
}

struct PackDownloadResult {
    id: uuid::Uuid,
    result: Result<std::path::PathBuf, String>,
}

impl Default for BridgeState {
    fn default() -> Self {
        // Only exists to satisfy azalea's `S: Default` bound; `spawn_bridge`
        // always installs a real state via `set_state`.
        let (event_tx, _) = crossbeam_channel::unbounded();
        let (_, cmd_rx) = crossbeam_channel::unbounded();
        let (_, pack_cmd_rx) = crossbeam_channel::unbounded();
        let (pack_download_tx, pack_download_rx) = crossbeam_channel::unbounded();
        Self {
            event_tx,
            cmd_rx,
            pack_cmd_rx,
            pack_download_tx,
            pack_download_rx,
            shared: Arc::new(Mutex::new(Shared::default())),
            dead: Arc::new(AtomicBool::new(false)),
            reported_end: Arc::new(AtomicBool::new(false)),
            disconnecting: Arc::new(AtomicBool::new(false)),
            exited: Arc::new(AtomicBool::new(false)),
            exit_task_spawned: Arc::new(AtomicBool::new(false)),
            view_distance: 12,
            resource_pack_policy: crate::settings::ServerResourcePackPolicy::Prompt,
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
            let (signature, last_seen) = match &packet {
                ChatPacket::Player(p) => (
                    p.signature.as_ref().map(|s| MsgSig(s.bytes)),
                    p.body.last_seen.entries.iter().map(pack_signature_ref).collect(),
                ),
                ChatPacket::System(_) | ChatPacket::Disguised(_) => (None, Vec::new()),
            };
            state.emit(&bot, GameEvent::Chat { spans, system, signature, last_seen });
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

    // The enchantment registry, for the same reason: the enchanting table's
    // three offers arrive as registry ids, and the screen needs their names.
    {
        // Armour trims: the stacks carry protocol ids into these two registries.
        let (patterns, materials) = (
            read_variant_registry(bot, "trim_pattern"),
            read_variant_registry(bot, "trim_material"),
        );
        {
            let mut sh = state.shared.lock();
            sh.trim_patterns = patterns.clone();
            sh.trim_materials = materials.clone();
        }
        state.emit(bot, GameEvent::TrimRegistries {
            patterns: std::sync::Arc::new(patterns),
            materials: std::sync::Arc::new(materials),
        });
    }
    // The instrument registry: a goat horn's tooltip names its instrument
    // ("Ponder", "Sing", …) by looking up this list with the protocol id its
    // `Instrument` component carries.
    {
        let instruments = read_variant_registry(bot, "instrument");
        if !instruments.is_empty() {
            state.shared.lock().instruments = instruments.clone();
            state.emit(bot, GameEvent::Instruments(std::sync::Arc::new(instruments)));
        }
    }
    // Enchantments are one of the few registries azalea parses into a typed
    // field rather than the generic `extra` bag.
    let enchantments: Vec<String> = {
        let world = bot.world();
        let world = world.read();
        world
            .registries
            .enchantment
            .map
            .keys()
            .map(|id| id.path().to_string())
            .collect()
    };
    if !enchantments.is_empty() {
        info!(count = enchantments.len(), "bridge: enchantment registry read");
        state.emit(bot, GameEvent::Enchantments(std::sync::Arc::new(enchantments)));
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
        .iter()
        .map(|(id, nbt)| {
            let effects = nbt.compound("effects");
            events::BiomeInfo {
                name: strip_minecraft_ns(&id.to_string()),
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
                fog: effects
                    .and_then(|e| e.int("fog_color"))
                    .map(rgb)
                    .unwrap_or([0xC0, 0xD8, 0xFF]),
                sky: effects
                    .and_then(|e| e.int("sky_color"))
                    .map(rgb)
                    .unwrap_or([0x78, 0xA7, 0xFF]),
                water_fog: effects
                    .and_then(|e| e.int("water_fog_color"))
                    .map(rgb)
                    .unwrap_or([0x05, 0x0D, 0x33]),
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
    state: &BridgeState,
    p: &azalea::protocol::packets::game::c_set_passengers::ClientboundSetPassengers,
) {
    // Remote riders, for the renderer: this list is authoritative, so first
    // drop everyone who used to sit on this vehicle, then seat the new list.
    {
        let vehicle = p.vehicle.0 as u32 as u64;
        let mut sh = state.shared.lock();
        sh.riders.retain(|_, (v, _)| *v != vehicle);
        for (seat, id) in p.passengers.iter().enumerate() {
            sh.riders.insert(id.0 as u32 as u64, (vehicle, seat.min(255) as u8));
        }
    }
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
        let seat = p.passengers.iter().position(|id| *id == my_id).unwrap_or(0);
        ecs.entity_mut(bot.entity).insert(plugins::RidingVehicle {
            vehicle,
            is_boat,
            delta_rotation: 0.0,
            seat: seat.min(255) as u8,
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
            state.shared.lock().mount = None;
        }
    }
}

/// Contents of the mount inventory. azalea drops these (its `Inventory` never
/// heard of this container), so the screen's slots are tracked here and pushed
/// to the app as an ordinary container.
fn on_mount_content(
    bot: &Client,
    state: &BridgeState,
    p: &azalea::protocol::packets::game::c_container_set_content::ClientboundContainerSetContent,
) {
    let (slots, carried) = {
        let mut sh = state.shared.lock();
        let Some(mount) = sh.mount.as_mut() else { return };
        if mount.container_id != p.container_id {
            return;
        }
        mount.state_id = p.state_id;
        mount.slots = p.items.iter().map(slot_snapshot).collect();
        mount.carried = slot_snapshot(&p.carried_item);
        (mount.slots.clone(), mount.carried.clone())
    };
    state.emit(bot, GameEvent::ContainerContent { id: p.container_id, slots, carried });
}

/// One slot of the mount inventory changed. Container id -1 is vanilla's
/// "this is what is on your cursor".
fn on_mount_slot(
    bot: &Client,
    state: &BridgeState,
    p: &azalea::protocol::packets::game::c_container_set_slot::ClientboundContainerSetSlot,
) {
    let (id, slots, carried) = {
        let mut sh = state.shared.lock();
        let Some(mount) = sh.mount.as_mut() else { return };
        if p.container_id == -1 {
            mount.carried = slot_snapshot(&p.item_stack);
        } else if mount.container_id == p.container_id {
            mount.state_id = p.state_id;
            let idx = p.slot as usize;
            if idx >= mount.slots.len() {
                mount.slots.resize(idx + 1, None);
            }
            mount.slots[idx] = slot_snapshot(&p.item_stack);
        } else {
            return;
        }
        (mount.container_id, mount.slots.clone(), mount.carried.clone())
    };
    state.emit(bot, GameEvent::ContainerContent { id, slots, carried });
}

/// Mirror a boss bar. azalea keeps no boss-bar state of its own, so the app's
/// set is built entirely from these packets.
fn on_boss_event(bot: &Client, state: &BridgeState, p: &ClientboundBossEvent) {
    use azalea::protocol::packets::game::c_boss_event::{BossBarColor, BossBarOverlay, Operation};
    fn color_id(c: BossBarColor) -> u8 {
        match c {
            BossBarColor::Pink => 0,
            BossBarColor::Blue => 1,
            BossBarColor::Red => 2,
            BossBarColor::Green => 3,
            BossBarColor::Yellow => 4,
            BossBarColor::Purple => 5,
            BossBarColor::White => 6,
        }
    }
    fn overlay_id(o: BossBarOverlay) -> u8 {
        match o {
            BossBarOverlay::Progress => 0,
            BossBarOverlay::Notched6 => 1,
            BossBarOverlay::Notched10 => 2,
            BossBarOverlay::Notched12 => 3,
            BossBarOverlay::Notched20 => 4,
        }
    }
    let id = p.id.as_u128();
    let update = match &p.operation {
        Operation::Add(a) => BossBarUpdate::Set {
            id,
            bar: BossBar {
                name: text::spans_of(&a.name),
                progress: a.progress,
                color: color_id(a.style.color),
                overlay: overlay_id(a.style.overlay),
                darken_screen: a.properties.darken_screen,
                world_fog: a.properties.create_world_fog,
            },
        },
        Operation::Remove => BossBarUpdate::Remove { id },
        Operation::UpdateProgress(v) => BossBarUpdate::Progress { id, progress: *v },
        Operation::UpdateName(n) => BossBarUpdate::Name { id, name: text::spans_of(n) },
        Operation::UpdateStyle(s) => {
            BossBarUpdate::Style { id, color: color_id(s.color), overlay: overlay_id(s.overlay) }
        }
        // Only the flags changed; nothing we draw depends on them alone.
        Operation::UpdateProperties(_) => return,
    };
    state.emit(bot, GameEvent::BossBar(update));
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
            // The Nether's 0.1 keeps its caves gloomy rather than black. Like
            // has_skylight this lives in the non-strict registry's extra bag.
            let ambient = data
                ._extra
                .get("ambient_light")
                .and_then(|tag| tag.float())
                .unwrap_or(if name.contains("nether") { 0.1 } else { 0.0 });
            (name, has_skylight, ultrawarm, ambient)
        })
    };
    let (dimension, has_skylight, ultrawarm, ambient_light) = resolved.unwrap_or_else(|| {
        // Registry entry missing (ViaVersion edge): guess from the world
        // name — the app must still clear the stale world either way.
        let name = strip_minecraft_ns(&common.dimension.to_string());
        let nether = name.contains("nether");
        let end = name.contains("the_end");
        (name, !(nether || end), nether, if nether { 0.1 } else { 0.0 })
    });
    info!(dimension, has_skylight, ultrawarm, ambient_light, "bridge: dimension change / respawn");
    state.emit(bot, GameEvent::Respawn { dimension, has_skylight, ultrawarm, ambient_light });
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
            {
                let mut shared = state.shared.lock();
                let entry = shared.light.entry((p.x, p.z)).or_default();
                convert::apply_light_data(entry, section_count, &p.light_data);
            }
            // Block entities ride along on the chunk packet: sign text, banner
            // patterns, head profiles… none of which azalea keeps.
            let found: Vec<BlockEntityInfo> = p
                .chunk_data
                .block_entities
                .iter()
                .filter_map(|be| {
                    let data = blockentity::decode(be.kind, &be.data)?;
                    Some(BlockEntityInfo {
                        pos: BlockPos {
                            x: p.x * 16 + (be.packed_xz >> 4) as i32,
                            y: be.y as i16 as i32,
                            z: p.z * 16 + (be.packed_xz & 15) as i32,
                        },
                        data,
                    })
                })
                .collect();
            if !found.is_empty() {
                state.emit(bot, GameEvent::BlockEntities(found));
            }
        }
        ClientboundGamePacket::BlockEntityData(p) => {
            if let Some(data) = blockentity::decode(p.block_entity_type, &p.tag) {
                state.emit(bot, GameEvent::BlockEntities(vec![BlockEntityInfo {
                    pos: BlockPos { x: p.pos.x, y: p.pos.y, z: p.pos.z },
                    data,
                }]));
            }
        }
        ClientboundGamePacket::BlockEvent(p) => {
            state.emit(bot, GameEvent::BlockAction {
                pos: BlockPos { x: p.pos.x, y: p.pos.y, z: p.pos.z },
                block: strip_minecraft_ns(p.block.to_str()),
                action: p.action_id,
                param: p.action_parameter,
            });
        }
        ClientboundGamePacket::BossEvent(p) => on_boss_event(bot, state, p),
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
        // --- vanilla's title system: three lines, four packets -------------
        ClientboundGamePacket::SetTitleText(p) => {
            state.emit(bot, GameEvent::Title(TitlePart::Title(text::spans_of(&p.text))));
        }
        ClientboundGamePacket::SetSubtitleText(p) => {
            state.emit(bot, GameEvent::Title(TitlePart::Subtitle(text::spans_of(&p.text))));
        }
        ClientboundGamePacket::SetActionBarText(p) => {
            state.emit(bot, GameEvent::Title(TitlePart::ActionBar(text::spans_of(&p.text))));
        }
        ClientboundGamePacket::SetTitlesAnimation(p) => {
            state.emit(bot, GameEvent::Title(TitlePart::Times {
                fade_in: p.fade_in as i32,
                stay: p.stay as i32,
                fade_out: p.fade_out as i32,
            }));
        }
        ClientboundGamePacket::ClearTitles(p) => {
            state.emit(bot, GameEvent::Title(TitlePart::Clear { reset: p.reset_times }));
        }
        ClientboundGamePacket::StopSound(p) => {
            state.emit(bot, GameEvent::StopSound {
                name: p.name.as_ref().map(|id| id.path().to_string()),
                category: p.source.map(|s| map_sound_source(s as i32)),
            });
        }
        ClientboundGamePacket::PlayerLookAt(p) => on_look_at(bot, state, p),
        ClientboundGamePacket::SetPassengers(p) => on_set_passengers(bot, state, p),
        ClientboundGamePacket::MountScreenOpen(p) => {
            let entity_id = p.entity_id.0 as u32 as u64;
            {
                let mut sh = state.shared.lock();
                sh.mount = Some(MountScreen {
                    container_id: p.container_id,
                    slots: Vec::new(),
                    carried: None,
                    state_id: 0,
                });
            }
            info!(
                id = p.container_id,
                columns = p.inventory_columns,
                "bridge: mount inventory opened"
            );
            state.emit(bot, GameEvent::MountScreen {
                container_id: p.container_id,
                columns: p.inventory_columns,
                entity_id,
            });
        }
        ClientboundGamePacket::ContainerClose(p) => {
            // The server closed the mount screen (or we were thrown off): drop
            // it, so a chest that reuses the id is never mistaken for a horse.
            let closed = {
                let mut sh = state.shared.lock();
                match sh.mount.as_ref().filter(|m| m.container_id == p.container_id) {
                    Some(_) => sh.mount.take().map(|m| m.container_id),
                    None => None,
                }
            };
            if let Some(id) = closed {
                state.emit(bot, GameEvent::ContainerClosed { id });
            }
        }
        ClientboundGamePacket::MoveVehicle(p) => {
            // The server disagreeing with our boat: it is the authority, so
            // take its position and carry on simulating from there.
            let Some(riding) = bot.get_component::<plugins::RidingVehicle>() else {
                return;
            };
            let mut ecs = bot.ecs.write();
            if let Some(mut pos) = ecs.get_mut::<Position>(riding.vehicle) {
                **pos = p.pos;
            }
            if let Some(mut look) = ecs.get_mut::<LookDirection>(riding.vehicle) {
                *look = p.look_direction;
            }
        }
        ClientboundGamePacket::ContainerSetContent(p) => on_mount_content(bot, state, p),
        ClientboundGamePacket::ContainerSetSlot(p) => on_mount_slot(bot, state, p),
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
        ClientboundGamePacket::Login(p) => {
            on_dimension_change(bot, state, &p.common);
            state.emit(bot, GameEvent::ReducedDebugInfo(p.reduced_debug_info));
        }
        ClientboundGamePacket::GameRuleValues(p) => {
            if let Some(v) = p.values.get(&GameRule::ReducedDebugInfo) {
                state.emit(bot, GameEvent::ReducedDebugInfo(v.as_str() == "true"));
            }
        }
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
            let id = p.entity_id.0 as u32 as u64;
            match p.event_id {
                // Legacy hurt animation (pre-HurtAnimation servers / ViaVersion
                // translations): entity event 2 = hurt.
                2 => state.emit(bot, GameEvent::EntityHurt { id, yaw: f32::NAN }),
                // Status 3 is "died", for every living entity.
                3 => state.emit(bot, GameEvent::EntityDeath { id }),
                // Everything else is one of vanilla's small moments — taming
                // smoke, breeding hearts, a shield blocking, a totem going
                // off. The app decides what each one looks and sounds like.
                other => state.emit(bot, GameEvent::EntityStatus { id, status: other as u8 }),
            }
        }
        ClientboundGamePacket::DamageEvent(p) => {
            // Modern damage event — flash the entity red as well. The app
            // ignores duplicate flashes from HurtAnimation within the window.
            state.emit(bot, GameEvent::EntityHurt { id: p.entity_id.0 as u32 as u64, yaw: f32::NAN });
        }
        ClientboundGamePacket::SetObjective(p) => on_set_objective(bot, state, p),
        ClientboundGamePacket::SetDisplayObjective(p) => on_set_display_objective(bot, state, p),
        ClientboundGamePacket::SetScore(p) => on_set_score(bot, state, p),
        ClientboundGamePacket::ResetScore(p) => on_reset_score(bot, state, p),
        ClientboundGamePacket::SetPlayerTeam(p) => on_set_player_team(bot, state, p),
        ClientboundGamePacket::SetEquipment(p) => on_set_equipment(state, p),
        // Attributes: the only one we need is max health, which is what says
        // how many hearts a horse's health bar has.
        ClientboundGamePacket::UpdateAttributes(p) => {
            for v in &p.values {
                if v.attribute == azalea::registry::builtin::Attribute::MaxHealth {
                    // Vanilla adds the flat modifiers first, then applies the
                    // multiplying ones to the sum.
                    let mut total = v.base;
                    let mut mul = 1.0f64;
                    for m in &v.modifiers {
                        use azalea::core::attribute_modifier_operation::AttributeModifierOperation as Op;
                        match m.operation {
                            Op::AddValue => total += m.amount,
                            Op::AddMultipliedBase => mul += m.amount,
                            Op::AddMultipliedTotal => mul += m.amount,
                        }
                    }
                    state
                        .shared
                        .lock()
                        .max_health
                        .insert(p.entity_id.0 as u32 as u64, (total * mul) as f32);
                }
            }
        }
        // azalea throws the spawn packet's "object data" away, but it is the
        // only place a falling block's block state and a projectile's / fishing
        // bobber's owner ever arrive.
        ClientboundGamePacket::AddEntity(p) => {
            if p.data != 0 {
                state.shared.lock().spawn_data.insert(p.id.0 as u32 as u64, p.data);
            }
            // A lightning bolt is a spawn-and-forget entity: the server never
            // moves or removes it, it just lives for 10 ticks on the client.
            if p.entity_type == EntityKind::LightningBolt {
                state.emit(bot, GameEvent::Lightning {
                    pos: [p.position.x, p.position.y, p.position.z],
                });
            }
            // The spawn packet carries the initial head rotation; without it a
            // standing mob would face body-forward until its first RotateHead.
            state
                .shared
                .lock()
                .head_yaw
                .insert(p.id.0 as u32 as u64, p.y_head_rot as f32 * 360.0 / 256.0);
        }
        // Head rotation is a packet of its own — azalea keeps only the body
        // yaw, so mobs would stare straight ahead forever without this.
        ClientboundGamePacket::RotateHead(p) => {
            state
                .shared
                .lock()
                .head_yaw
                .insert(p.entity_id.0 as u32 as u64, p.y_head_rot as f32 * 360.0 / 256.0);
        }
        // Leads: `dest_id` 0 (vanilla sends -1 → 0 as u32 is not it; the field is
        // an entity id, and a cut lead sends the max value) means "unleashed".
        ClientboundGamePacket::SetEntityLink(p) => {
            let (src, dest) = (p.source_id.0, p.dest_id.0);
            let mut sh = state.shared.lock();
            if dest < 0 {
                sh.leashes.remove(&(src as u32 as u64));
            } else {
                sh.leashes.insert(src as u32 as u64, dest as u32 as u64);
            }
        }
        ClientboundGamePacket::HurtAnimation(p) => on_hurt_animation(bot, state, p),
        ClientboundGamePacket::Animate(p) => on_animate(bot, state, p),
        ClientboundGamePacket::LevelParticles(p) => on_level_particles(bot, state, p),
        ClientboundGamePacket::Waypoint(p) => on_waypoint(bot, state, p),
        ClientboundGamePacket::DeleteChat(p) => {
            let signature = pack_signature_ref(&p.signature);
            state.emit(bot, GameEvent::DeleteChat { signature });
        }
        ClientboundGamePacket::LowDiskSpaceWarning(_) => {
            state.emit(bot, GameEvent::LowDiskSpaceWarning);
        }
        ClientboundGamePacket::ServerLinks(p) => {
            let links = p.links.iter().map(server_link).collect();
            state.emit(bot, GameEvent::ServerLinks(links));
        }
        ClientboundGamePacket::ShowDialog(p) => {
            if let Some(dialog) = dialog::parse_holder(bot, &p.dialog) {
                state.emit(bot, GameEvent::ShowDialog(dialog));
            }
        }
        ClientboundGamePacket::ClearDialog(_) => {
            state.emit(bot, GameEvent::ClearDialog);
        }
        ClientboundGamePacket::Transfer(p) => {
            state.emit(bot, GameEvent::Transfer { host: p.host.clone(), port: p.port });
        }
        ClientboundGamePacket::SelectAdvancementsTab(p) => {
            state.emit(bot, GameEvent::SelectAdvancementsTab(p.tab.as_ref().map(|id| id.to_string())));
        }
        ClientboundGamePacket::RecipeBookSettings(p) => {
            let s = &p.book_settings;
            state.emit(bot, GameEvent::RecipeBookSettings {
                crafting: (s.gui_open, s.filtering_craftable),
                furnace: (s.furnace_gui_open, s.furnace_filtering_craftable),
                blast_furnace: (s.blast_furnace_gui_open, s.blast_furnace_filtering_craftable),
                smoker: (s.smoker_gui_open, s.smoker_filtering_craftable),
            });
        }
        ClientboundGamePacket::ServerData(p) => {
            state.emit(bot, GameEvent::ServerData {
                motd: text::spans_of(&p.motd),
                icon_bytes: p.icon_bytes.clone(),
            });
        }
        ClientboundGamePacket::CustomChatCompletions(p) => {
            let action = match p.action {
                ChatCompletionActionPacket::Add => ChatCompletionAction::Add,
                ChatCompletionActionPacket::Remove => ChatCompletionAction::Remove,
                ChatCompletionActionPacket::Set => ChatCompletionAction::Set,
            };
            state.emit(bot, GameEvent::ChatCompletions { action, entries: p.entries.clone() });
        }
        ClientboundGamePacket::MapItemData(p) => on_map_item_data(bot, state, p),
        ClientboundGamePacket::OpenSignEditor(p) => {
            state.emit(bot, GameEvent::OpenSignEditor {
                pos: BlockPos { x: p.pos.x, y: p.pos.y, z: p.pos.z },
                front: p.is_front_text,
            });
        }
        ClientboundGamePacket::RecipeBookAdd(p) => {
            // Bit 0 of the flags is vanilla's "notification" bit: the recipes
            // that are worth a toast, as opposed to the whole book on join.
            let count = p.entries.iter().filter(|e| e.flags & 1 != 0).count();
            if count > 0 && !p.replace {
                state.emit(bot, GameEvent::RecipesUnlocked { count: count as u32 });
            }
            let entries: Vec<_> =
                p.entries.iter().filter_map(|e| recipe::book_entry(&e.contents)).collect();
            if !entries.is_empty() || p.replace {
                state.emit(bot, GameEvent::RecipeBook { entries, replace: p.replace });
            }
        }
        ClientboundGamePacket::RecipeBookRemove(p) => {
            state.emit(bot, GameEvent::RecipesForgotten(p.recipes.clone()));
        }
        ClientboundGamePacket::PlaceGhostRecipe(p) => {
            if let Some(recipe) = recipe::from_display(0, &p.recipe) {
                state.emit(bot, GameEvent::GhostRecipe {
                    container_id: p.container_id,
                    recipe,
                });
            }
        }
        ClientboundGamePacket::UpdateRecipes(p) => {
            // The stonecutter is the one station whose whole recipe list the
            // server hands over up front — the screen picks from it by hand.
            let cuts: Vec<_> = p
                .stonecutter_recipes
                .iter()
                .map(|e| StonecutterRecipe {
                    inputs: recipe::ingredient_items(&e.input),
                    result: recipe::display_item(&e.recipe.option_display).unwrap_or_default(),
                })
                .collect();
            info!(stonecutter = cuts.len(), "bridge: recipes updated");
            state.emit(bot, GameEvent::StonecutterRecipes(Arc::new(cuts)));
        }
        ClientboundGamePacket::ContainerSetData(p) => {
            state.emit(bot, GameEvent::ContainerData {
                id: p.container_id,
                property: p.id,
                value: p.value,
            });
        }
        ClientboundGamePacket::UpdateAdvancements(p) => on_update_advancements(bot, state, p),
        ClientboundGamePacket::AwardStats(p) => on_award_stats(bot, state, p),
        ClientboundGamePacket::BlockDestruction(p) => {
            // 0..=9 sets a crack stage; vanilla treats anything else as "gone".
            state.emit(bot, GameEvent::BlockDestruction {
                id: p.id.0 as u32 as u64,
                pos: BlockPos { x: p.pos.x, y: p.pos.y, z: p.pos.z },
                stage: (p.progress <= 9).then_some(p.progress),
            });
        }
        ClientboundGamePacket::Explode(p) => {
            state.emit(bot, GameEvent::Explosion {
                pos: [p.center.x, p.center.y, p.center.z],
                radius: p.radius.clamp(0.5, 16.0),
                // The blast sound rides on the packet; the client is the one
                // that plays it (the server sends no separate sound packet).
                sound: strip_minecraft_ns(&p.explosion_sound.to_string()),
            });
        }
        ClientboundGamePacket::TakeItemEntity(p) => {
            state.emit(bot, GameEvent::ItemPickedUp {
                item: p.item_id as u64,
                collector: p.player_id.0 as u32 as u64,
            });
        }
        ClientboundGamePacket::PlayerCombatKill(p) => {
            state.emit(bot, GameEvent::Died { message: text::spans_of(&p.message) });
        }
        ClientboundGamePacket::OpenBook(p) => {
            use azalea::protocol::packets::game::s_interact::InteractionHand;
            state.emit(bot, GameEvent::OpenBook {
                off_hand: matches!(p.hand, InteractionHand::OffHand),
            });
        }
        ClientboundGamePacket::SetCamera(p) => {
            let id = p.camera_id.0 as u32 as u64;
            let own = bot.entity_component::<MinecraftEntityId>(bot.entity).0 as u32 as u64;
            state.emit(bot, GameEvent::Camera { id: (id != own).then_some(id) });
        }
        ClientboundGamePacket::SetDefaultSpawnPosition(p) => {
            let pos = &p.global_pos.pos;
            state.emit(bot, GameEvent::SpawnPosition([pos.x as f64 + 0.5, pos.z as f64 + 0.5]));
        }
        ClientboundGamePacket::InitializeBorder(p) => {
            let border = events::WorldBorderUpdate {
                center_x: p.new_center_x,
                center_z: p.new_center_z,
                old_size: p.old_size,
                new_size: p.new_size,
                lerp_time: p.lerp_time,
                warning_blocks: p.warning_blocks,
                warning_time: p.warning_time,
            };
            state.shared.lock().border = border;
            state.emit(bot, GameEvent::WorldBorder(border));
        }
        // The five incremental border packets each change one field of the
        // border the server last initialized, so the bridge keeps the whole
        // thing and re-sends it.
        ClientboundGamePacket::SetBorderSize(p) => {
            emit_border(bot, state, |b| {
                b.old_size = p.size;
                b.new_size = p.size;
                b.lerp_time = 0;
            });
        }
        ClientboundGamePacket::SetBorderLerpSize(p) => {
            emit_border(bot, state, |b| {
                b.old_size = p.old_size;
                b.new_size = p.new_size;
                b.lerp_time = p.lerp_time;
            });
        }
        ClientboundGamePacket::SetBorderCenter(p) => {
            emit_border(bot, state, |b| {
                b.center_x = p.new_center_x;
                b.center_z = p.new_center_z;
            });
        }
        ClientboundGamePacket::SetBorderWarningDistance(p) => {
            emit_border(bot, state, |b| b.warning_blocks = p.warning_blocks);
        }
        ClientboundGamePacket::SetBorderWarningDelay(p) => {
            emit_border(bot, state, |b| b.warning_time = p.warning_delay);
        }
        _ => {}
    }
}

/// Apply one change to the remembered world border and send the whole thing on.
fn emit_border(bot: &Client, state: &BridgeState, f: impl FnOnce(&mut events::WorldBorderUpdate)) {
    let border = {
        let mut sh = state.shared.lock();
        f(&mut sh.border);
        sh.border
    };
    state.emit(bot, GameEvent::WorldBorder(border));
}

/// A filled map's contents changed. The server sends a rectangular patch of
/// colour indices plus, when they moved, the full marker list.
fn on_map_item_data(
    bot: &Client,
    state: &BridgeState,
    p: &azalea::protocol::packets::game::ClientboundMapItemData,
) {
    use azalea::protocol::packets::game::c_map_item_data::DecorationType;
    let decorations = p.decorations.as_ref().map(|list| {
        list.iter()
            .map(|d| events::MapDecoration {
                sprite: match d.decoration_type {
                    DecorationType::Player => "player",
                    DecorationType::Frame => "frame",
                    DecorationType::RedMarker => "red_marker",
                    DecorationType::BlueMarker => "blue_marker",
                    DecorationType::TargetX => "target_x",
                    DecorationType::TargetPoint => "target_point",
                    DecorationType::PlayerOffMap => "player_off_map",
                    DecorationType::PlayerOffLimits => "player_off_limits",
                    DecorationType::Mansion => "woodland_mansion",
                    DecorationType::Monument => "ocean_monument",
                    DecorationType::BannerWhite => "white_banner",
                    DecorationType::BannerOrange => "orange_banner",
                    DecorationType::BannerMagenta => "magenta_banner",
                    DecorationType::BannerLightBlue => "light_blue_banner",
                    DecorationType::BannerYellow => "yellow_banner",
                    DecorationType::BannerLime => "lime_banner",
                    DecorationType::BannerPink => "pink_banner",
                    DecorationType::BannerGray => "gray_banner",
                    DecorationType::BannerLightGray => "light_gray_banner",
                    DecorationType::BannerCyan => "cyan_banner",
                    DecorationType::BannerPurple => "purple_banner",
                    DecorationType::BannerBlue => "blue_banner",
                    DecorationType::BannerBrown => "brown_banner",
                    DecorationType::BannerGreen => "green_banner",
                    DecorationType::BannerRed => "red_banner",
                    DecorationType::BannerBlack => "black_banner",
                    DecorationType::RedX => "red_x",
                },
                x: d.x,
                y: d.y,
                rot: d.rot,
                name: d.name.as_ref().map(text::plain_text),
            })
            .collect()
    });
    let patch = p.color_patch.0.as_ref().and_then(|m| {
        // A zero-sized patch is the server's way of saying "nothing changed".
        (m.width > 0 && m.height > 0).then(|| events::MapPatch {
            start_x: m.start_x,
            start_y: m.start_y,
            width: m.width,
            height: m.height,
            colors: m.map_colors.clone(),
        })
    });
    state.emit(bot, GameEvent::MapData(Box::new(events::MapUpdate {
        id: p.map_id,
        scale: p.scale,
        locked: p.locked,
        decorations,
        patch,
    })));
}

/// The advancement tree. Sent in full on join and then incrementally; the app
/// keeps the tree and works out what is done from the criteria.
fn on_update_advancements(
    bot: &Client,
    state: &BridgeState,
    p: &azalea::protocol::packets::game::ClientboundUpdateAdvancements,
) {
    use azalea::protocol::packets::game::c_update_advancements::FrameType;
    let added = p
        .added
        .iter()
        .map(|h| events::AdvancementNode {
            id: h.id.to_string(),
            parent: h.value.parent_id.as_ref().map(|id| id.to_string()),
            display: h.value.display.as_ref().map(|d| events::AdvancementDisplay {
                title: text::spans_of(&d.title),
                description: text::spans_of(&d.description),
                icon: slot_snapshot(&d.icon),
                frame: match d.frame {
                    FrameType::Task => 0,
                    FrameType::Challenge => 1,
                    FrameType::Goal => 2,
                },
                show_toast: d.show_toast,
                hidden: d.hidden,
                background: d.background.as_ref().map(|b| b.to_string()),
                x: d.x,
                y: d.y,
            }),
            requirements: h.value.requirements.clone(),
        })
        .collect();
    let progress = p
        .progress
        .iter()
        .map(|(id, criteria)| {
            let obtained = criteria
                .iter()
                .filter(|(_, c)| c.date.is_some())
                .map(|(name, _)| name.clone())
                .collect();
            (id.to_string(), obtained)
        })
        .collect();
    state.emit(bot, GameEvent::Advancements(Box::new(events::AdvancementUpdate {
        reset: p.reset,
        added,
        removed: p.removed.iter().map(|id| id.to_string()).collect(),
        progress,
    })));
}

/// The player's statistics, in reply to a `ClientCommand::RequestStats`.
fn on_award_stats(
    bot: &Client,
    state: &BridgeState,
    p: &azalea::protocol::packets::game::ClientboundAwardStats,
) {
    use azalea::protocol::packets::game::c_award_stats::Stat;
    let mut out: Vec<events::StatEntry> = p
        .stats
        .iter()
        .map(|(stat, value)| {
            let (category, key) = match stat {
                Stat::Mined(b) => ("mined", b.to_str().to_string()),
                Stat::Crafted(i) => ("crafted", i.to_str().to_string()),
                Stat::Used(i) => ("used", i.to_str().to_string()),
                Stat::Broken(i) => ("broken", i.to_str().to_string()),
                Stat::PickedUp(i) => ("picked_up", i.to_str().to_string()),
                Stat::Dropped(i) => ("dropped", i.to_str().to_string()),
                Stat::Killed(e) => ("killed", e.to_str().to_string()),
                Stat::KilledBy(e) => ("killed_by", e.to_str().to_string()),
                Stat::Custom(c) => ("custom", c.to_str().to_string()),
            };
            events::StatEntry { category, key: strip_minecraft_ns(&key), value: *value }
        })
        .collect();
    // Biggest first inside each family — that is the order vanilla's screen
    // opens on, and it puts the interesting rows on the first page.
    out.sort_by(|a, b| a.category.cmp(b.category).then(b.value.cmp(&a.value)));
    state.emit(bot, GameEvent::Statistics(out));
}

/// An entity took damage: flash it red (client-side animation).
fn on_hurt_animation(bot: &Client, state: &BridgeState, p: &ClientboundHurtAnimation) {
    // Our own flinch drives the camera, everyone else's just flashes them red.
    if bot.get_component::<MinecraftEntityId>().map(|id| *id) == Some(p.id) {
        state.emit(bot, GameEvent::OwnHurt { yaw: p.yaw });
    }
    state.emit(bot, GameEvent::EntityHurt { id: p.id.0 as u32 as u64, yaw: p.yaw });
}

/// An entity animated. A main/off-hand swing plays the arm-swing so other
/// players are visibly seen hitting/mining, exactly like vanilla.
/// The server turned the player's head (`/teleport … facing`). Vanilla points
/// the camera at the target from the chosen anchor — feet or eyes — so work out
/// the angles here, where the player's own position is at hand.
fn on_look_at(bot: &Client, state: &BridgeState, p: &ClientboundPlayerLookAt) {
    use azalea::protocol::packets::game::c_player_look_at::Anchor;
    let Some(me): Option<Vec3> = bot.get_component::<Position>().map(|p| **p) else { return };
    let eye = if matches!(p.from_anchor, Anchor::Eyes) { 1.62 } else { 0.0 };
    // The point in the packet is already the target's anchor position (the
    // server resolves feet/eyes before sending), entity or not.
    let target = p.pos;
    let (dx, dy, dz) = (target.x - me.x, target.y - (me.y + eye), target.z - me.z);
    let flat = (dx * dx + dz * dz).sqrt();
    let yaw = (dz.atan2(dx).to_degrees() - 90.0) as f32;
    let pitch = (-dy.atan2(flat).to_degrees()) as f32;
    state.emit(bot, GameEvent::LookAt { yaw, pitch });
}

fn on_animate(bot: &Client, state: &BridgeState, p: &ClientboundAnimate) {
    use azalea::protocol::packets::game::c_animate::AnimationAction;
    let id = p.id.0 as u32 as u64;
    match p.action {
        AnimationAction::SwingMainHand | AnimationAction::SwingOffHand => {
            state.emit(bot, GameEvent::EntitySwing { id });
        }
        // A critical hit landed on this entity: vanilla bursts its own
        // particles over the victim, in two flavours (plain and enchanted).
        AnimationAction::CriticalHit => {
            state.emit(bot, GameEvent::EntityCrit { id, magic: false });
        }
        AnimationAction::MagicCriticalHit => {
            state.emit(bot, GameEvent::EntityCrit { id, magic: true });
        }
        _ => {}
    }
}

/// Translate a server particle burst into a spawn request the app can render.
fn on_level_particles(bot: &Client, state: &BridgeState, p: &ClientboundLevelParticles) {
    // Cap the count so a firework/explosion burst can't flood the sim.
    let count = p.count.min(64);
    if count == 0 {
        return;
    }
    let (tex, mut color, size, gravity) = particle_style(&p.particle);
    let item = item_particle_icon(&p.particle);
    if item.is_some() {
        // The icon already carries its own real colour; don't grey-tint it
        // with the generic fallback's dust colour.
        color = [1.0, 1.0, 1.0];
    }
    state.emit(bot, GameEvent::Particles {
        pos: [p.pos.x, p.pos.y, p.pos.z],
        tex,
        color,
        size,
        count,
        spread: [p.x_dist, p.y_dist, p.z_dist],
        speed: p.max_speed,
        gravity,
        item,
    });
}

/// Translate a server waypoint change into the app's locator-bar state.
/// `Track` and `Update` both carry a full icon + position (see
/// `WaypointUpdate`'s doc comment) so both simply replace the entry.
fn on_waypoint(bot: &Client, state: &BridgeState, p: &ClientboundWaypoint) {
    let id = match &p.waypoint.identifier {
        WaypointIdentifier::Uuid(u) => events::WaypointKey::Uuid(u.as_u128()),
        WaypointIdentifier::String(s) => events::WaypointKey::Name(s.clone()),
    };
    if matches!(p.operation, WaypointOperation::Untrack) {
        state.emit(bot, GameEvent::Waypoint(events::WaypointUpdate::Remove { id }));
        return;
    }
    // Never show an arrow pointing at yourself — the same check vanilla's own
    // `LocatorBarRenderer.extractRenderState` makes before rendering a dot.
    if let events::WaypointKey::Uuid(u) = id
        && bot.get_component::<GameProfileComponent>().is_some_and(|p| p.uuid.as_u128() == u)
    {
        return;
    }
    let style = strip_minecraft_ns(&p.waypoint.icon.style.to_string()).to_string();
    let color = p.waypoint.icon.color.map(|c| [c.red(), c.green(), c.blue()]);
    let pos = match &p.waypoint.data {
        WaypointData::Empty => events::WaypointPos::Empty,
        WaypointData::Vec3i(v) => events::WaypointPos::Pos([v.x, v.y, v.z]),
        WaypointData::Chunk { x, z } => events::WaypointPos::Chunk { x: *x, z: *z },
        WaypointData::Azimuth { angle } => events::WaypointPos::Azimuth(*angle),
    };
    state.emit(bot, GameEvent::Waypoint(events::WaypointUpdate::Set {
        id,
        waypoint: events::TrackedWaypointInfo { style, color, pos },
    }));
}

/// A panda's sneeze rears its head back over 15 ticks then releases it over
/// the next 5 — a pure function of the real tick counter the server sends
/// (`SneezeCounter`), transcribed exactly from `PandaModel.setupAnim`
/// (`-45°` is vanilla's `-0.7853982` rad, i.e. `-π/4`). Returns degrees, the
/// convention `EntityDrawKind::Mob::head_pitch` already uses.
fn panda_sneeze_head_pitch(sneeze_time: i32) -> f32 {
    let t = sneeze_time.clamp(0, 19);
    if t < 15 {
        -45.0 * t as f32 / 14.0
    } else {
        let p = (t - 15) as f32 / 5.0;
        -45.0 + 45.0 * p
    }
}

/// Resolve a wire-format signature reference into this client's own `PackedSig`.
/// The `Id` case (a compact index into the sender's rolling 128-entry
/// signature cache) is passed through unresolved — this client doesn't build
/// that cache, see `GameEvent::DeleteChat`'s doc comment for why.
fn pack_signature_ref(p: &PackedMessageSignature) -> PackedSig {
    match p {
        PackedMessageSignature::Signature(sig) => PackedSig::Direct(MsgSig(sig.bytes)),
        PackedMessageSignature::Id(id) => PackedSig::Id(*id),
    }
}

/// A `ClientboundServerLinks` entry's real English label: either the
/// server's own text, or vanilla's real `known_server_link.*` translation for
/// one of the well-known kinds (confirmed against `en_us.json` — these are
/// not this client's own wording).
fn server_link(entry: &azalea::protocol::common::server_links::ServerLinkEntry) -> ServerLink {
    let label = match &entry.kind {
        ServerLinkKind::Component(text) => text::plain_text(text),
        ServerLinkKind::Known(kind) => match kind {
            KnownLinkKind::BugReport => "Report Server Bug",
            KnownLinkKind::CommunityGuidelines => "Community Guidelines",
            KnownLinkKind::Support => "Support",
            KnownLinkKind::Status => "Status",
            KnownLinkKind::Feedback => "Feedback",
            KnownLinkKind::Community => "Community",
            KnownLinkKind::Website => "Website",
            KnownLinkKind::Forums => "Forums",
            KnownLinkKind::News => "News",
            KnownLinkKind::Announcements => "Announcements",
        }
        .to_string(),
    };
    ServerLink { label, url: entry.link.clone() }
}

/// Real vanilla renders `Item`/`ItemSlime`/`ItemCobweb`/`ItemSnowball`
/// particles using the actual item's own icon rather than any fixed sprite —
/// confirmed by the absence of an `item*.json` under the client jar's
/// `assets/minecraft/particles/`, unlike every other particle kind. `Item`
/// carries the real thrown/broken `ItemStack` in the packet; the other three
/// are always the same specific item (a slimeball, a cobweb, a snowball —
/// there is no "cobweb" throwable, but the particle exists for the block
/// breaking effect) with no payload, so those three are hardcoded.
fn item_particle_icon(particle: &azalea::entity::particle::Particle) -> Option<String> {
    use azalea::entity::particle::Particle as P;
    match particle {
        P::Item(ip) => match &ip.item {
            ItemStack::Present(d) => Some(strip_minecraft_ns(d.kind.to_str())),
            ItemStack::Empty => None,
        },
        P::ItemSlime => Some("slime_ball".to_string()),
        P::ItemCobweb => Some("cobweb".to_string()),
        P::ItemSnowball => Some("snowball".to_string()),
        _ => None,
    }
}

/// `RgbColor`'s 0..255 channels as the 0.0..1.0 floats this renderer's tints
/// use everywhere else.
fn rgb(c: azalea::core::color::RgbColor) -> [f32; 3] {
    [c.red() as f32 / 255.0, c.green() as f32 / 255.0, c.blue() as f32 / 255.0]
}

/// Map a particle kind to a flat color, cube size, and gravity for the app's
/// lightweight cube-particle renderer. Dust and the entity-effect swirl read
/// their real per-instance colour (and dust its real scale) from the packet;
/// block-state-carrying variants (a falling block's dust, block-marker) are
/// still approximated by a representative color rather than sampling that
/// block's own texture — a bigger change than a flat color/scale read.
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
        P::Flame | P::CopperFireFlame | P::SmallFlame => (T::Flame, w, 0.12, -0.5),
        P::SoulFireFlame => (T::SoulFlame, w, 0.12, -0.5),
        P::Lava | P::FallingLava | P::LandingLava | P::DrippingLava => (T::Lava, w, 0.14, 4.0),
        P::Smoke | P::LargeSmoke => (T::Smoke, w, 0.14, -0.4),
        // Real texture is the plain "generic" sheet, not big_smoke — a lighter,
        // faster-fading puff (e.g. a snuffed campfire) rather than a thick one.
        P::WhiteSmoke => (T::Generic, [0.95, 0.95, 0.95], 0.13, -0.5),
        P::CampfireCosySmoke | P::CampfireSignalSmoke => (T::Smoke, w, 0.16, -0.35),
        P::Cloud | P::Poof => (T::Generic, w, 0.16, -0.2),
        P::Explosion | P::ExplosionEmitter => (T::Explosion, w, 0.55, -0.3),
        P::Bubble | P::BubbleColumnUp | P::CurrentDown => (T::Bubble, w, 0.10, -1.0),
        P::BubblePop => (T::BubblePop, w, 0.10, -0.6),
        P::Splash | P::Rain | P::Fishing => (T::Splash, w, 0.12, -1.0),
        P::DrippingWater | P::FallingWater => (T::Drip, [0.30, 0.45, 0.85], 0.10, 5.0),
        // Honey shares the drip family's shape but reads amber, not blue.
        P::DrippingHoney | P::FallingHoney | P::LandingHoney => {
            (T::Drip, [0.90, 0.65, 0.10], 0.10, 3.0)
        }
        // Dripstone drips are lava- or water-coloured but keep the drip shape.
        P::DrippingDripstoneLava | P::FallingDripstoneLava => (T::Drip, [0.95, 0.45, 0.10], 0.11, 4.0),
        P::DrippingDripstoneWater | P::FallingDripstoneWater => {
            (T::Drip, [0.30, 0.45, 0.85], 0.10, 5.0)
        }
        P::DrippingObsidianTear | P::FallingObsidianTear | P::LandingObsidianTear => {
            (T::Drip, [0.15, 0.05, 0.20], 0.10, 3.5)
        }
        P::FallingNectar => (T::Drip, [0.95, 0.80, 0.20], 0.09, 2.0),
        P::FallingSporeBlossom | P::SporeBlossomAir => (T::Drip, [0.90, 0.45, 0.65], 0.09, 0.6),
        P::HappyVillager | P::Composter | P::EggCrack | P::PauseMobGrowth | P::ResetMobGrowth => {
            (T::Happy, w, 0.16, -0.2)
        }
        P::AngryVillager => (T::Angry, w, 0.18, -0.2),
        P::Portal | P::ReversePortal => (T::Portal, [0.55, 0.25, 0.85], 0.12, 0.0),
        P::Effect => (T::Effect, w, 0.14, 0.0),
        // Real vanilla colours this swirl from the entity's actual active
        // potion effects (server-computed ARGB blend) rather than a fixed
        // tint — e.g. a poisoned mob's swirl reads green, not white.
        P::EntityEffect(c) => (T::Effect, rgb(c.color), 0.14, 0.0),
        P::Note => (T::Note, w, 0.18, -0.1),
        // Real vanilla textures: a firework's own spark trail is the `spark`
        // sheet; `Flash` (the single burst frame) is a separate particle kind.
        P::Firework => (T::Spark, w, 0.14, 0.3),
        P::Flash => (T::Flash, w, 0.16, 1.0),
        P::Glow | P::GlowSquidInk | P::WaxOn | P::WaxOff | P::Scrape | P::ElectricSpark => {
            (T::Glow, w, 0.12, 0.0)
        }
        P::Block(_) | P::BlockMarker(_) | P::FallingDust(_) | P::DustPlume => {
            (T::Generic, [0.55, 0.52, 0.48], 0.12, 6.0)
        }
        // Redstone dust's real colour and size ride in the packet (dyed dust
        // and each wire's power level both change it) — read them instead of
        // guessing one fixed reddish dot for every power level and colour.
        P::Dust(d) => (T::Dust, rgb(d.color), 0.12 * d.scale.clamp(0.01, 4.0), 0.0),
        // This fades from `from` to `to` over its life in real vanilla; shown
        // here at its starting colour rather than plumbing a second fade-to
        // colour through the particle-spawn pipeline for what is normally a
        // short-lived mote.
        P::DustColorTransition(d) => (T::Dust, rgb(d.from), 0.12 * d.scale.clamp(0.01, 4.0), 0.0),
        // Real texture is `glitter`, brighter and sharper than the `spark`
        // family above — matches vanilla giving the totem burst its own look.
        P::TotemOfUndying | P::EndRod => (T::Glitter, [0.95, 0.85, 0.35], 0.14, -0.4),
        P::Snowflake => (T::Generic, [0.92, 0.94, 0.98], 0.12, 1.5),
        // Real vanilla leaf particles, previously only spawned client-side as
        // ambient block effects — now also handled if the server sends one.
        P::CherryLeaves => (T::Cherry, w, 0.11, 0.8),
        P::PaleOakLeaves => (T::PaleOak, w, 0.11, 0.8),
        P::TintedLeaves => (T::Leaf, w, 0.11, 0.8),
        P::Nautilus => (T::Nautilus, w, 0.14, 0.0),
        P::SculkSoul => (T::SculkSoul, w, 0.12, -0.3),
        P::Soul => (T::Soul, w, 0.12, -0.3),
        P::Firefly => (T::Firefly, w, 0.10, -0.1),
        // A witch's brew swirl and an instant-effect potion's burst share the
        // real `spell` sheet (distinct from the ambient `effect` sheet above).
        P::Witch | P::InstantEffect => (T::Spell, [0.65, 0.30, 0.85], 0.14, -0.2),
        P::Gust => (T::Gust, w, 0.30, 0.0),
        P::SmallGust | P::GustEmitterSmall | P::GustEmitterLarge => (T::SmallGust, w, 0.20, 0.0),
        P::SonicBoom => (T::SonicBoom, w, 0.60, 0.0),
        P::SculkCharge(_) => (T::SculkCharge, [0.30, 0.75, 0.75], 0.18, 0.0),
        P::SculkChargePop => (T::SculkChargePop, [0.30, 0.75, 0.75], 0.16, 0.0),
        P::SweepAttack => (T::Sweep, w, 0.35, 0.0),
        P::Infested => (T::Infested, w, 0.13, 6.0),
        P::Vibration(_) => (T::Vibration, [0.55, 0.85, 0.80], 0.10, 0.0),
        P::Shriek(_) => (T::Shriek, [0.85, 0.20, 0.25], 0.30, 0.0),
        P::VaultConnection => (T::VaultConnection, [0.95, 0.75, 0.25], 0.12, 0.0),
        P::RaidOmen => (T::RaidOmen, w, 0.16, -0.3),
        P::TrialOmen => (T::TrialOmen, w, 0.16, -0.3),
        P::OminousSpawning => (T::OminousSpawning, w, 0.20, -0.3),
        P::TrialSpawnerDetection => (T::TrialSpawnerDetection, w, 0.16, -0.2),
        P::TrialSpawnerDetectionOminous => (T::TrialSpawnerDetectionOminous, w, 0.16, -0.2),
        P::Enchant => (T::Enchant, [0.75, 0.35, 0.95], 0.14, -0.4),
        // Ambient/rare kinds whose real texture is the plain generic sheet —
        // distinguished from each other and from the unhandled fallback below
        // by tint alone, same as the dust/snowflake cases above.
        P::Dolphin => (T::Generic, [0.75, 0.90, 0.98], 0.09, -0.6),
        P::Mycelium => (T::Generic, [0.55, 0.45, 0.55], 0.08, -0.1),
        P::Ash => (T::Generic, [0.45, 0.45, 0.45], 0.10, 0.3),
        P::WhiteAsh => (T::Generic, [0.90, 0.90, 0.88], 0.10, 0.2),
        P::CrimsonSpore => (T::Generic, [0.80, 0.15, 0.25], 0.09, -0.2),
        P::WarpedSpore => (T::Generic, [0.15, 0.60, 0.65], 0.09, -0.2),
        P::DragonBreath => (T::Generic, [0.55, 0.75, 0.30], 0.20, -0.1),
        P::Sneeze => (T::Generic, [0.55, 0.75, 0.35], 0.10, 3.0),
        P::Spit => (T::Generic, [0.85, 0.85, 0.75], 0.10, 4.0),
        P::SquidInk => (T::Generic, [0.08, 0.08, 0.10], 0.20, -0.4),
        P::Underwater => (T::Generic, [0.35, 0.55, 0.85], 0.10, 0.0),
        P::Trail => (T::Generic, [0.90, 0.75, 0.20], 0.10, 0.0),
        _ => (T::Generic, [0.85, 0.85, 0.88], 0.12, 0.5),
    }
}

#[cfg(test)]
mod particle_style_tests {
    use super::*;
    use azalea::entity::particle::Particle as P;
    use events::ParticleTex as T;

    /// A firework's own spark trail and its single burst flash are two real,
    /// differently-textured particles — a prior version of this match lumped
    /// them into one arm and always drew the flash sprite for both.
    #[test]
    fn firework_uses_its_real_spark_texture_not_the_flash() {
        assert_eq!(particle_style(&P::Firework).0, T::Spark);
        assert_eq!(particle_style(&P::Flash).0, T::Flash);
    }

    /// End rods and the totem burst use the real `glitter` sheet, distinct
    /// from the plainer `spark` sheet firework trails use.
    #[test]
    fn end_rod_and_totem_use_glitter_not_spark() {
        assert_eq!(particle_style(&P::EndRod).0, T::Glitter);
        assert_eq!(particle_style(&P::TotemOfUndying).0, T::Glitter);
    }

    /// These families were already loaded into the particle atlas (for
    /// client-side ambient block effects) but never wired to the matching
    /// real server-sent `Particle` kind — a plain grey dot until now.
    #[test]
    fn previously_unwired_ambient_families_now_match_their_particle() {
        assert_eq!(particle_style(&P::CherryLeaves).0, T::Cherry);
        assert_eq!(particle_style(&P::PaleOakLeaves).0, T::PaleOak);
        assert_eq!(particle_style(&P::TintedLeaves).0, T::Leaf);
        assert_eq!(particle_style(&P::Nautilus).0, T::Nautilus);
        assert_eq!(particle_style(&P::SculkSoul).0, T::SculkSoul);
        assert_eq!(particle_style(&P::Soul).0, T::Soul);
        assert_eq!(particle_style(&P::Firefly).0, T::Firefly);
    }

    /// A sample of the 0.98.0 batch: real, previously-unhandled particle
    /// kinds now on their own verified real texture family instead of the
    /// generic fallback.
    #[test]
    fn newly_added_families_are_wired_up() {
        assert_eq!(particle_style(&P::SonicBoom).0, T::SonicBoom);
        assert_eq!(particle_style(&P::SweepAttack).0, T::Sweep);
        assert_eq!(particle_style(&P::Enchant).0, T::Enchant);
        assert_eq!(particle_style(&P::Shriek(Default::default())).0, T::Shriek);
        assert_eq!(particle_style(&P::Witch).0, T::Spell);
        assert_eq!(particle_style(&P::WaxOn).0, T::Glow);
    }

    /// A kind with no real per-type texture in the client jar (checked via
    /// the extracted `assets/minecraft/particles/*.json`) correctly falls
    /// back to the generic approximation rather than guessing one.
    #[test]
    fn kinds_with_no_real_texture_fall_back_to_generic() {
        assert_eq!(particle_style(&P::ElderGuardian).0, T::Generic);
        assert_eq!(particle_style(&P::DustPillar).0, T::Generic);
        assert_eq!(particle_style(&P::BlockCrumble).0, T::Generic);
    }

    /// `ItemSlime`/`ItemCobweb`/`ItemSnowball` carry no payload — real vanilla
    /// always shows the same specific item for each, confirmed by there being
    /// no `item*.json` under the client jar's `assets/minecraft/particles/`
    /// (unlike every other kind, which does have one).
    #[test]
    fn fixed_item_particles_map_to_their_real_item() {
        assert_eq!(item_particle_icon(&P::ItemSlime).as_deref(), Some("slime_ball"));
        assert_eq!(item_particle_icon(&P::ItemCobweb).as_deref(), Some("cobweb"));
        assert_eq!(item_particle_icon(&P::ItemSnowball).as_deref(), Some("snowball"));
    }

    /// The general `Item` kind carries the real thrown/broken `ItemStack` in
    /// the packet — this reads whatever item that stack holds, not a fixed one.
    #[test]
    fn item_particle_uses_the_carried_stacks_own_kind() {
        use azalea::entity::particle::ItemParticle;
        use azalea_inventory::{ItemStack, ItemStackData};
        let stack = ItemStack::Present(ItemStackData {
            kind: azalea::registry::builtin::ItemKind::GoldenApple,
            count: 1,
            component_patch: Default::default(),
        });
        let particle = P::Item(ItemParticle { item: stack });
        assert_eq!(item_particle_icon(&particle).as_deref(), Some("golden_apple"));
    }

    /// An empty stack (shouldn't happen for a real `Item` particle, but the
    /// server is not to be trusted) must not panic or fabricate an icon.
    #[test]
    fn item_particle_with_empty_stack_has_no_icon() {
        use azalea::entity::particle::ItemParticle;
        use azalea_inventory::ItemStack;
        let particle = P::Item(ItemParticle { item: ItemStack::Empty });
        assert_eq!(item_particle_icon(&particle), None);
    }

    /// An ordinary particle kind (nothing to do with items) must not somehow
    /// get an icon assigned.
    #[test]
    fn ordinary_particles_have_no_item_icon() {
        assert_eq!(item_particle_icon(&P::Smoke), None);
    }

    /// Redstone dust's real colour (not a fixed reddish guess) and real
    /// scale (not a fixed size) both come from the packet.
    #[test]
    fn dust_reads_its_real_colour_and_scale() {
        use azalea::core::color::RgbColor;
        use azalea::entity::particle::DustParticle;
        let blue = P::Dust(DustParticle { color: RgbColor::new(20, 40, 220), scale: 2.0 });
        let (_, color, size, _) = particle_style(&blue);
        assert!((color[0] - 20.0 / 255.0).abs() < 1e-6);
        assert!((color[2] - 220.0 / 255.0).abs() < 1e-6);
        assert!(size > 0.12, "scale 2.0 should be bigger than the scale-1.0 baseline");
    }

    /// A dust colour transition shows its real starting colour, not the old
    /// fixed reddish placeholder that ignored the packet entirely.
    #[test]
    fn dust_color_transition_reads_its_real_starting_colour() {
        use azalea::core::color::RgbColor;
        use azalea::entity::particle::DustColorTransitionParticle;
        let p = P::DustColorTransition(DustColorTransitionParticle {
            from: RgbColor::new(10, 200, 30),
            to: RgbColor::new(200, 10, 30),
            scale: 1.0,
        });
        let (_, color, _, _) = particle_style(&p);
        assert!((color[1] - 200.0 / 255.0).abs() < 1e-6, "starts green, not the old fixed red");
    }

    /// A mob's status-effect swirl reads the server's real computed potion
    /// colour instead of always drawing white.
    #[test]
    fn entity_effect_reads_its_real_colour() {
        use azalea::core::color::RgbColor;
        use azalea::entity::particle::ColorParticle;
        let poison_green = P::EntityEffect(ColorParticle { color: RgbColor::new(20, 150, 30) });
        let (_, color, _, _) = particle_style(&poison_green);
        assert!((color[1] - 150.0 / 255.0).abs() < 1e-6);
        assert_ne!(color, [1.0, 1.0, 1.0], "must not fall back to the plain white swirl");
    }
}

/// Mirror a remote entity's equipment (armor + hands). azalea ignores this
/// packet for non-local entities, so we track it ourselves and attach it to the
/// per-tick entity snapshots (drives armor rendering on other players).
fn on_set_equipment(state: &BridgeState, p: &ClientboundSetEquipment) {
    let id = p.entity_id.0 as u32 as u64;
    let mut sh = state.shared.lock();
    // Trim ids are only meaningful against the server's registries, so resolve
    // them before the equipment map is borrowed mutably.
    let trims: Vec<Option<(String, String)>> = p
        .slots
        .slots
        .iter()
        .map(|(_, stack)| trim_of(&sh, stack))
        .collect();
    let eq = sh.entity_equipment.entry(id).or_default();
    for (i, (slot, stack)) in p.slots.slots.iter().enumerate() {
        let name = match stack {
            ItemStack::Present(d) => Some(strip_minecraft_ns(d.kind.to_str())),
            ItemStack::Empty => None,
        };
        let trim = trims.get(i).cloned().flatten();
        match slot {
            components::EquipmentSlot::Head => {
                eq.head = name;
                eq.trims[0] = trim;
            }
            components::EquipmentSlot::Chest => {
                eq.chest = name;
                eq.trims[1] = trim;
            }
            components::EquipmentSlot::Legs => {
                eq.legs = name;
                eq.trims[2] = trim;
            }
            components::EquipmentSlot::Feet => {
                eq.feet = name;
                eq.trims[3] = trim;
            }
            components::EquipmentSlot::Mainhand => eq.main_hand = name,
            components::EquipmentSlot::Offhand => eq.off_hand = name,
            // Animal armour and saddles: their own layer over the animal's
            // model rather than anything on the humanoid one.
            components::EquipmentSlot::Body => eq.body = name,
            components::EquipmentSlot::Saddle => eq.saddle = name,
        }
    }
}

/// The `(pattern, material)` names of an armour stack's trim, resolved through
/// the server's trim registries. `None` when the piece carries no trim.
fn trim_of(sh: &Shared, stack: &ItemStack) -> Option<(String, String)> {
    use azalea::registry::DataRegistry as _;
    let ItemStack::Present(data) = stack else { return None };
    let trim = data.get_component::<components::Trim>()?;
    let pattern = sh.trim_patterns.get(trim.pattern.protocol_id() as usize)?;
    let material = sh.trim_materials.get(trim.material.protocol_id() as usize)?;
    Some((pattern.clone(), material.clone()))
}

/// Download and validate a server resource pack. The cache is content-addressed
/// by the advertised SHA-1 (or by a SHA-1 of the URL when the server omitted
/// one), and a cache hit is re-verified before it is trusted. Downloads stream
/// to a uniquely named temporary file, so a malicious Content-Length or a lost
/// connection can never leave a valid-looking partial `.zip` behind.
async fn download_resource_pack(
    id: uuid::Uuid,
    url: &str,
    hash: &str,
    event_tx: &Sender<GameEvent>,
) -> anyhow::Result<std::path::PathBuf> {
    use sha1::{Digest as _, Sha1};
    use tokio::io::AsyncWriteExt as _;

    const MAX_PACK_BYTES: u64 = 256 * 1024 * 1024;
    let parsed = reqwest::Url::parse(url).context("invalid resource-pack URL")?;
    anyhow::ensure!(
        matches!(parsed.scheme(), "http" | "https"),
        "resource-pack URL must use HTTP or HTTPS"
    );
    anyhow::ensure!(parsed.username().is_empty(), "resource-pack URL must not contain credentials");

    let expected = if hash.trim().is_empty() {
        None
    } else {
        let normalized = hash.trim().to_ascii_lowercase();
        anyhow::ensure!(
            normalized.len() == 40 && normalized.bytes().all(|b| b.is_ascii_hexdigit()),
            "server supplied an invalid SHA-1"
        );
        Some(normalized)
    };
    let dir = crate::settings::GameSettings::config_dir().join("server-packs");
    std::fs::create_dir_all(&dir)?;
    let key = expected.clone().unwrap_or_else(|| sha1_hex(url.as_bytes()));
    let path = dir.join(format!("{key}.zip"));
    if path.is_file() {
        // Hash-less server pushes are still content-verified: the first
        // successful download records its actual SHA-1 in the sidecar. An old
        // cache entry without that proof is redownloaded instead of trusted.
        let cached_sha1 = expected
            .clone()
            .or_else(|| read_cached_pack_sha1(&dir, &key));
        match cached_sha1
            .as_deref()
            .context("cached resource pack has no integrity metadata")
            .and_then(|hash| validate_pack_archive(&path, Some(hash)))
        {
            Ok((bytes, _)) => {
                write_pack_metadata(&dir, &key, url, cached_sha1.as_deref().unwrap(), bytes);
                let _ = event_tx.send(GameEvent::ResourcePackProgress {
                    id,
                    downloaded: bytes,
                    total: Some(bytes),
                });
                return Ok(path);
            }
            Err(e) => {
                warn!(path = %path.display(), error = %format!("{e:#}"), "bridge: deleting corrupt pack cache entry");
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("DolphinClient/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let mut resp = client.get(parsed).send().await?.error_for_status()?;
    let total = resp.content_length();
    if let Some(total) = total {
        anyhow::ensure!(total <= MAX_PACK_BYTES, "resource pack is larger than 256 MiB");
    }

    // A server may replace a push with the same UUID while the old download is
    // still in flight, so the packet UUID alone is not a unique temporary name.
    let sequence = PACK_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!("{key}.{id}.{sequence}.part"));
    let streamed: anyhow::Result<(u64, String)> = async {
        let mut file = tokio::fs::File::create(&tmp).await?;
        let mut downloaded = 0u64;
        let mut digest = Sha1::new();
        let mut last_report = std::time::Instant::now() - std::time::Duration::from_secs(1);
        while let Some(chunk) = resp.chunk().await? {
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            anyhow::ensure!(downloaded <= MAX_PACK_BYTES, "resource pack is larger than 256 MiB");
            digest.update(&chunk);
            file.write_all(&chunk).await?;
            if last_report.elapsed() >= std::time::Duration::from_millis(100) {
                let _ = event_tx.send(GameEvent::ResourcePackProgress { id, downloaded, total });
                last_report = std::time::Instant::now();
            }
        }
        file.flush().await?;
        drop(file);
        let actual = digest.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>();
        if let Some(expected) = &expected {
            anyhow::ensure!(&actual == expected, "resource pack SHA-1 mismatch");
        }
        let _ = event_tx.send(GameEvent::ResourcePackProgress { id, downloaded, total: Some(total.unwrap_or(downloaded)) });
        Ok((downloaded, actual))
    }
    .await;

    let (bytes, actual_sha1) = match streamed {
        Ok(value) => value,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    if let Err(e) = validate_pack_archive(&tmp, Some(&actual_sha1)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(rename_error) = std::fs::rename(&tmp, &path) {
        // Windows cannot replace an existing file atomically. A concurrent
        // identical download may have won the race; keep it only after the
        // same full validation, otherwise surface the original failure.
        if validate_pack_archive(&path, Some(&actual_sha1)).is_ok() {
            let _ = std::fs::remove_file(&tmp);
        } else {
            let _ = std::fs::remove_file(&tmp);
            return Err(rename_error.into());
        }
    }
    write_pack_metadata(&dir, &key, url, &actual_sha1, bytes);
    Ok(path)
}

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::{Digest as _, Sha1};
    Sha1::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn sha1_file_hex(path: &std::path::Path) -> anyhow::Result<String> {
    use sha1::{Digest as _, Sha1};
    use std::io::Read as _;

    let mut file = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut digest = Sha1::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn read_cached_pack_sha1(dir: &std::path::Path, key: &str) -> Option<String> {
    let raw = std::fs::read(dir.join(format!("{key}.json"))).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let hash = value.get("sha1")?.as_str()?.to_ascii_lowercase();
    (hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(hash)
}

fn write_pack_metadata(
    dir: &std::path::Path,
    key: &str,
    url: &str,
    sha1: &str,
    bytes: u64,
) {
    let metadata = serde_json::json!({
        "url": url,
        "sha1": sha1,
        "bytes": bytes,
        "last_used_unix": chrono::Utc::now().timestamp(),
    });
    if let Ok(raw) = serde_json::to_vec_pretty(&metadata) {
        let _ = std::fs::write(dir.join(format!("{key}.json")), raw);
    }
}

/// Validate both integrity and a few abuse bounds without extracting anything.
/// `pack.mcmeta` is mandatory in vanilla and proves this is a resource pack,
/// not merely an arbitrary ZIP served by a multiplayer host.
fn validate_pack_archive(path: &std::path::Path, expected_sha1: Option<&str>) -> anyhow::Result<(u64, u64)> {
    use std::io::Read as _;
    let bytes = std::fs::metadata(path)?.len();
    anyhow::ensure!(bytes > 0, "resource pack is empty");
    if let Some(expected) = expected_sha1 {
        anyhow::ensure!(
            sha1_file_hex(path)? == expected,
            "cached resource pack SHA-1 mismatch"
        );
    }
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).context("invalid resource-pack ZIP")?;
    anyhow::ensure!(zip.len() <= 65_536, "resource pack contains too many files");
    let mut expanded = 0u64;
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i)?;
        expanded = expanded.saturating_add(entry.size());
        anyhow::ensure!(expanded <= 1024 * 1024 * 1024, "resource pack expands beyond 1 GiB");
    }
    let mut meta = zip.by_name("pack.mcmeta").context("resource pack has no pack.mcmeta")?;
    anyhow::ensure!(meta.size() <= 1024 * 1024, "pack.mcmeta is unreasonably large");
    let mut json = String::new();
    meta.read_to_string(&mut json)?;
    let parsed: serde_json::Value = serde_json::from_str(&json).context("invalid pack.mcmeta JSON")?;
    anyhow::ensure!(parsed.get("pack").is_some_and(serde_json::Value::is_object), "pack.mcmeta has no pack object");
    Ok((bytes, expanded))
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
    use azalea::protocol::packets::game::c_set_player_team::NameTagVisibility;
    let team_of = |params: &azalea::protocol::packets::game::c_set_player_team::Parameters| Team {
        prefix: text::spans_of(&params.player_prefix),
        suffix: text::spans_of(&params.player_suffix),
        color: params.color.color().map(text::rgb),
        hide_names: match params.nametag_visibility {
            NameTagVisibility::Always => NameTagRule::Always,
            NameTagVisibility::Never => NameTagRule::Never,
            NameTagVisibility::HideForOtherTeams => NameTagRule::HideForOtherTeams,
            NameTagVisibility::HideForOwnTeam => NameTagRule::HideForOwnTeam,
        },
    };
    {
        let mut sh = state.shared.lock();
        match &p.method {
            Method::Add((params, players)) => {
                sh.sb_teams.insert(p.name.clone(), team_of(params));
                for m in players {
                    sh.sb_member_team.insert(m.clone(), p.name.clone());
                }
            }
            Method::Change(params) => {
                sh.sb_teams.insert(p.name.clone(), team_of(params));
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
    if let Some(team) = team {
        out.extend(team.prefix.iter().cloned());
    }
    let mut name = text::spans_of_legacy(owner);
    // The team colour paints the member's own name, not the prefix and suffix
    // (those carry their own formatting) — vanilla's `PlayerTeam.getColor`.
    if let Some(color) = team.and_then(|t| t.color) {
        for span in &mut name {
            span.color.get_or_insert(color);
        }
    }
    out.extend(name);
    if let Some(team) = team {
        out.extend(team.suffix.iter().cloned());
    }
    out
}

/// Whether this entry's nametag may be drawn at all, under its team's rule
/// and ours (vanilla's `Team.getNameTagVisibility`).
fn nametag_visible(sh: &Shared, owner: &str, own_name: Option<&str>) -> bool {
    let Some(team_name) = sh.sb_member_team.get(owner) else {
        return true;
    };
    let Some(team) = sh.sb_teams.get(team_name) else {
        return true;
    };
    let same_team = own_name
        .and_then(|me| sh.sb_member_team.get(me))
        .is_some_and(|mine| mine == team_name);
    match team.hide_names {
        NameTagRule::Always => true,
        NameTagRule::Never => false,
        NameTagRule::HideForOtherTeams => same_team,
        NameTagRule::HideForOwnTeam => !same_team,
    }
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

/// Put the `Noclip` marker on the local player exactly while the server has us
/// in spectator mode — vanilla's spectator walks through the world, and
/// bumping into walls is the single most obvious way to get it wrong.
fn sync_noclip(bot: &Client) {
    use azalea::physics::local_player::Noclip;
    let spectator = bot
        .get_component::<azalea::local_player::LocalGameMode>()
        .is_some_and(|g| g.current.to_id() == 3);
    let has = bot.get_component::<Noclip>().is_some();
    if spectator == has {
        return;
    }
    let mut ecs = bot.ecs.write();
    let mut entity = ecs.entity_mut(bot.entity);
    if spectator {
        entity.insert(Noclip);
    } else {
        entity.remove::<Noclip>();
    }
}

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
                reason: "Timed out: the server stopped responding.".into(),
            });
        }
        bot.disconnect();
        state.request_exit(bot);
        return;
    }

    // 1c. Spectator noclip. The physics crate cannot see the game mode
    // (azalea-client depends on it, not the other way round), so the marker it
    // does understand is put on and taken off here.
    sync_noclip(bot);

    // 2. Local player snapshot, every tick.
    if let Some(snap) = player_snapshot(bot, state) {
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
                team: sh.sb_member_team.get(&info.profile.name).cloned(),
            }
        })
        .collect();
    drop(sh);
    // Vanilla's tab list groups teammates together and only then sorts by
    // name, so a server's red and blue sides never interleave.
    list.sort_by(|a, b| {
        a.team
            .cmp(&b.team)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
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

/// The cape url out of the same base64 `textures` property. Most accounts have
/// no `CAPE` entry at all.
fn cape_of_properties(profile: &azalea::auth::game_profile::GameProfile) -> Option<String> {
    let prop = profile.properties.map.get("textures")?;
    let raw = base64::engine::general_purpose::STANDARD.decode(prop.value.as_bytes()).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    Some(json.get("textures")?.get("CAPE")?.get("url")?.as_str()?.to_owned())
}

/// `ItemStack` → snapshot with custom name + lore (`None` for empty slots).
/// Registry names of the blocks an adventure-mode `can_place_on`/`can_break`
/// predicate matches — only the common direct-list form; a predicate that's
/// only a block tag (`#minecraft:...`) or NBT/property match contributes no
/// names rather than a guess.
fn adventure_block_names(pred: &components::AdventureModePredicate) -> Vec<String> {
    pred.predicates
        .iter()
        .filter_map(|p| p.blocks.as_ref())
        .flat_map(|set| match set {
            azalea::registry::HolderSet::Direct { contents } => contents
                .iter()
                .map(|b| strip_minecraft_ns(b.to_str()))
                .collect::<Vec<_>>(),
            azalea::registry::HolderSet::Named { .. } => Vec::new(),
        })
        .collect()
}

fn slot_snapshot(stack: &ItemStack) -> Option<ItemSnapshot> {
    let ItemStack::Present(data) = stack else {
        return None;
    };
    // Which tooltip sections the server explicitly hid via `tooltip_display`
    // (distinct from `hide_tooltip`, which is all-or-nothing) — checked below
    // per section, same as vanilla's own tooltip builder does.
    use components::DataComponentTrait as _;
    let hidden_sections = data
        .get_component::<components::TooltipDisplay>()
        .map(|t| t.hidden_components.clone())
        .unwrap_or_default();
    let hidden = |kind| hidden_sections.contains(&kind);

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
    // Vanilla's glint rule: any enchantment (or stored enchantment, so books in
    // a chest shimmer too), overridable per stack by the server.
    let enchanted = match data.get_component::<components::EnchantmentGlintOverride>() {
        Some(o) => o.show_glint,
        None => {
            data.get_component::<components::Enchantments>()
                .is_some_and(|e| !e.levels.is_empty())
                || data
                    .get_component::<components::StoredEnchantments>()
                    .is_some_and(|e| !e.enchantments.is_empty())
        }
    };
    let max_damage = data
        .get_component::<components::MaxDamage>()
        .map_or(0, |d| d.amount.max(0) as u32);
    // An unbreakable item never shows the bar, exactly like vanilla.
    let damage = if data.get_component::<components::Unbreakable>().is_some() {
        0
    } else {
        data.get_component::<components::Damage>()
            .map_or(0, |d| d.amount.max(0) as u32)
    };
    Some(ItemSnapshot {
        item: strip_minecraft_ns(data.kind.to_str()),
        count: data.count.max(0) as u32,
        name,
        lore,
        enchanted,
        damage,
        max_damage,
        map_id: data
            .get_component::<components::MapId>()
            .map(|m| m.id.max(0) as u32),
        enchantments: if hidden(components::Enchantments::KIND) {
            Vec::new()
        } else {
            use azalea::registry::DataRegistry as _;
            let mut list: Vec<(u32, u32)> = data
                .get_component::<components::Enchantments>()
                .map(|e| {
                    e.levels
                        .iter()
                        .map(|(k, v)| (k.protocol_id(), (*v).max(0) as u32))
                        .collect()
                })
                .unwrap_or_default();
            // A stable order: the server hands them over in hash order.
            list.sort_unstable();
            list
        },
        stored_enchantments: if hidden(components::StoredEnchantments::KIND) {
            Vec::new()
        } else {
            use azalea::registry::DataRegistry as _;
            let mut list: Vec<(u32, u32)> = data
                .get_component::<components::StoredEnchantments>()
                .map(|e| {
                    e.enchantments
                        .iter()
                        .map(|(k, v)| (k.protocol_id(), (*v).max(0) as u32))
                        .collect()
                })
                .unwrap_or_default();
            list.sort_unstable();
            list
        },
        effects: {
            let mut list: Vec<(String, u32, i32)> = data
                .get_component::<components::PotionContents>()
                .map(|p| {
                    p.custom_effects
                        .iter()
                        .map(|e| {
                            (
                                strip_minecraft_ns(e.id.to_str()),
                                e.details.amplifier.max(0) as u32,
                                e.details.duration,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Suspicious stew: its own component, always a single un-amplified
            // effect — the tooltip line vanilla shows is what tells you which
            // one you're about to get.
            if let Some(stew) = data.get_component::<components::SuspiciousStewEffects>() {
                list.extend(
                    stew.effects
                        .iter()
                        .map(|e| (strip_minecraft_ns(e.effect.to_str()), 0, e.duration)),
                );
            }
            // An Ominous Bottle's Bad Omen: always the same 120000-tick
            // (100 minute) duration regardless of amplifier, which is the
            // whole point of bottling it — carry it to a village on your own
            // schedule instead of the few minutes a raid captain grants.
            if let Some(o) = data.get_component::<components::OminousBottleAmplifier>() {
                list.push(("bad_omen".to_string(), o.amplifier.max(0) as u32, 120_000));
            }
            list
        },
        modifiers: if hidden(components::AttributeModifiers::KIND) {
            Vec::new()
        } else {
            data
            .get_component::<components::AttributeModifiers>()
            .map(|m| {
                use azalea::core::attribute_modifier_operation::AttributeModifierOperation as Op;
                use components::AttributeModifierDisplay as Display;
                m.modifiers
                    .iter()
                    // A server can mark a modifier `Hidden` (many vanilla items do,
                    // e.g. ones whose bonus is already implied elsewhere) — showing
                    // it anyway would be a tooltip vanilla never actually prints.
                    .filter(|e| !matches!(e.display, Display::Hidden))
                    .map(|e| {
                        let op = match e.modifier.operation {
                            Op::AddValue => 0,
                            Op::AddMultipliedBase => 1,
                            Op::AddMultipliedTotal => 2,
                        };
                        (strip_minecraft_ns(e.kind.to_str()), e.modifier.amount, op)
                    })
                    .collect()
            })
            .unwrap_or_default()
        },
        unbreakable: !hidden(components::Unbreakable::KIND)
            && data.get_component::<components::Unbreakable>().is_some(),
        dyed: if hidden(components::DyedColor::KIND) {
            None
        } else {
            data.get_component::<components::DyedColor>().map(|d| {
                let rgb = d.rgb as u32;
                [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]
            })
        },
        trim: if hidden(components::Trim::KIND) {
            None
        } else {
            use azalea::registry::DataRegistry as _;
            data.get_component::<components::Trim>()
                .map(|t| (t.pattern.protocol_id(), t.material.protocol_id()))
        },
        bundle_contents: if hidden(components::BundleContents::KIND) {
            Vec::new()
        } else {
            data.get_component::<components::BundleContents>()
                .map(|b| b.items.iter().filter_map(slot_snapshot).collect())
                .unwrap_or_default()
        },
        potion: data
            .get_component::<components::PotionContents>()
            .and_then(|p| p.potion)
            .map(|p| strip_minecraft_ns(p.to_str())),
        book: data.get_component::<components::WrittenBookContent>().map(|b| {
            events::BookContent {
                title: b.title.raw.clone(),
                author: b.author.clone(),
                pages: b.pages.iter().map(|p| text::spans_of(&p.raw)).collect(),
                generation: b.generation.clamp(0, 3) as u8,
            }
        }),
        writable_pages: data
            .get_component::<components::WritableBookContent>()
            .map(|b| b.pages.iter().map(|p| p.raw.clone()).collect()),
        flight_duration: data
            .get_component::<components::Fireworks>()
            .map(|f| f.flight_duration.clamp(0, 255) as u8),
        lodestone: data.get_component::<components::LodestoneTracker>().and_then(|t| {
            let target = t.target.as_ref()?;
            Some((
                [target.pos.x as f64 + 0.5, target.pos.z as f64 + 0.5],
                target.dimension.to_string(),
            ))
        }),
        container_contents: if hidden(components::Container::KIND) {
            Vec::new()
        } else {
            data.get_component::<components::Container>()
                .map(|c| c.items.iter().filter_map(slot_snapshot).collect())
                .unwrap_or_default()
        },
        charged_projectiles: if hidden(components::ChargedProjectiles::KIND) {
            Vec::new()
        } else {
            data.get_component::<components::ChargedProjectiles>()
                .map(|c| c.items.iter().filter_map(slot_snapshot).collect())
                .unwrap_or_default()
        },
        instrument: if hidden(components::Instrument::KIND) {
            None
        } else {
            use azalea::registry::DataRegistry as _;
            data.get_component::<components::Instrument>().map(|i| match &i.value {
                Holder::Reference(id) => InstrumentDesc::Id(id.protocol_id()),
                Holder::Direct(d) => InstrumentDesc::Text(text::plain_text(&d.description)),
            })
        },
        rarity: match data.get_component::<components::Rarity>().as_deref() {
            Some(components::Rarity::Uncommon) => 1,
            Some(components::Rarity::Rare) => 2,
            Some(components::Rarity::Epic) => 3,
            _ => 0,
        },
        hide_tooltip: data
            .get_component::<components::TooltipDisplay>()
            .is_some_and(|t| t.hide_tooltip),
        can_place_on: data
            .get_component::<components::CanPlaceOn>()
            .map(|c| adventure_block_names(&c.predicate))
            .unwrap_or_default(),
        can_break: data
            .get_component::<components::CanBreak>()
            .map(|c| adventure_block_names(&c.predicate))
            .unwrap_or_default(),
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

/// Send one `ServerboundPlayerCommand` — vanilla's catch-all for "the player
/// pressed something the server has to know about": start gliding, charge a
/// horse jump, get out of bed, open the mount's inventory.
fn player_command(bot: &Client, action: s_player_command::Action, data: u32) {
    let Some(id) = bot.get_component::<MinecraftEntityId>() else {
        warn!(?action, "bridge: no entity id yet; player command dropped");
        return;
    };
    bot.write_packet(azalea::protocol::packets::game::ServerboundPlayerCommand {
        id: *id,
        action,
        data,
    });
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
        Command::BundleSelectItem { window_id: _, slot, selected } => {
            // Real vanilla's `int selectedItem` goes over the wire as a raw
            // VarInt of its bit pattern, so -1 (deselect) becomes u32::MAX
            // here — same bytes azalea's `#[var] u32` field would encode for
            // either type, since VarInt encoding doesn't care about signedness.
            bot.write_packet(ServerboundBundleItemSelected {
                slot_id: slot as i32,
                selected_item_index: selected as u32,
            });
        }
        Command::RecipeBookChangeSettings { kind, open, filtering } => {
            let book_type = match kind {
                RecipeBookKind::Crafting => RecipeBookType::Crafting,
                RecipeBookKind::Furnace => RecipeBookType::Furnace,
                RecipeBookKind::BlastFurnace => RecipeBookType::BlastFurnace,
                RecipeBookKind::Smoker => RecipeBookType::Smoker,
            };
            bot.write_packet(ServerboundRecipeBookChangeSettings {
                book_type,
                is_open: open,
                is_filtering: filtering,
            });
        }
        Command::RequestStats => {
            use azalea::protocol::packets::game::s_client_command::Action;
            bot.write_packet(azalea::protocol::packets::game::ServerboundClientCommand {
                action: Action::RequestStats,
            });
        }
        Command::Respawn => {
            use azalea::protocol::packets::game::s_client_command::Action;
            bot.write_packet(azalea::protocol::packets::game::ServerboundClientCommand {
                action: Action::PerformRespawn,
            });
        }
        Command::ContainerButton { window_id, button } => {
            bot.write_packet(azalea::protocol::packets::game::ServerboundContainerButtonClick {
                container_id: window_id,
                button_id: button as u32,
            });
        }
        Command::RenameItem { name } => {
            bot.write_packet(azalea::protocol::packets::game::ServerboundRenameItem { name });
        }
        Command::SetBeacon { primary, secondary } => {
            // The screen speaks in effect names; the packet wants the
            // mob-effect registry ids, which azalea's registry hands over.
            let id = |name: &Option<String>| -> Option<u32> {
                use azalea::registry::Registry as _;
                use std::str::FromStr as _;
                let effect =
                    azalea::registry::builtin::MobEffect::from_str(name.as_deref()?).ok()?;
                Some(effect.to_u32())
            };
            bot.write_packet(azalea::protocol::packets::game::ServerboundSetBeacon {
                primary: id(&primary),
                secondary: id(&secondary),
            });
        }
        Command::SetFlying(flying) => {
            // Vanilla flips the ability client-side and reports it; the flag
            // has to land locally too, because that is what the physics reads.
            {
                let mut ecs = bot.ecs.write();
                if let Some(mut abilities) =
                    ecs.get_mut::<azalea::entity::PlayerAbilities>(bot.entity)
                {
                    if !abilities.can_fly {
                        return;
                    }
                    abilities.flying = flying;
                }
            }
            bot.write_packet(azalea::protocol::packets::game::ServerboundPlayerAbilities {
                is_flying: flying,
            });
        }
        Command::CreativeSlot { slot, item, count } => {
            use std::str::FromStr as _;
            let stack = match azalea::registry::builtin::ItemKind::from_str(&item) {
                Ok(kind) => azalea_inventory::ItemStack::Present(azalea_inventory::ItemStackData {
                    kind,
                    count: count as i32,
                    component_patch: Default::default(),
                }),
                Err(_) => azalea_inventory::ItemStack::Empty,
            };
            bot.write_packet(azalea::protocol::packets::game::ServerboundSetCreativeModeSlot {
                slot_num: slot,
                item_stack: stack,
            });
        }
        Command::StartGliding => {
            // Vanilla only ever asks: the server checks the chest slot and
            // answers by setting the shared flag, which is what the physics
            // and the pose both read.
            player_command(bot, s_player_command::Action::StartFallFlying, 0);
        }
        Command::RideJump { power } => {
            player_command(bot, s_player_command::Action::StartRidingJump, power.min(100));
        }
        Command::StopSleeping => {
            player_command(bot, s_player_command::Action::StopSleeping, 0);
        }
        Command::OpenMountInventory => {
            player_command(bot, s_player_command::Action::OpenInventory, 0);
        }
        Command::MountClick { container_id, slot, kind } => {
            use azalea::protocol::packets::game::s_container_click::HashedStack;
            use azalea_inventory::operations::ClickType;
            let (click_type, button) = match kind {
                SlotClickKind::Left => (ClickType::Pickup, 0),
                SlotClickKind::Right => (ClickType::Pickup, 1),
                SlotClickKind::QuickMove => (ClickType::QuickMove, 0),
                SlotClickKind::Throw => (ClickType::Throw, 0),
            };
            let state_id = state
                .shared
                .lock()
                .mount
                .as_ref()
                .filter(|m| m.container_id == container_id)
                .map(|m| m.state_id)
                .unwrap_or(0);
            // No predicted slots: the server runs the click and re-sends
            // whatever actually changed, cursor included, so the screen is
            // one round trip behind instead of guessing wrong.
            bot.write_packet(azalea::protocol::packets::game::ServerboundContainerClick {
                container_id,
                state_id,
                slot_num: slot as i16,
                button_num: button,
                click_type,
                changed_slots: Default::default(),
                carried_item: HashedStack(None),
            });
        }
        Command::CloseMount { container_id } => {
            state.shared.lock().mount = None;
            bot.write_packet(azalea::protocol::packets::game::ServerboundContainerClose {
                container_id,
            });
        }
        Command::SignUpdate { pos, front, lines } => {
            bot.write_packet(azalea::protocol::packets::game::ServerboundSignUpdate {
                pos: AzBlockPos::new(pos.x, pos.y, pos.z),
                is_front_text: front,
                lines,
            });
        }
        Command::EditBook { slot, pages, title } => {
            bot.write_packet(azalea::protocol::packets::game::ServerboundEditBook {
                slot,
                pages,
                title,
            });
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
        // Routed through GameHandle's dedicated configuration-safe channel;
        // these can never reach the ordinary tick command queue.
        Command::ResourcePackResponse { .. } | Command::ResourcePackApplied { .. } => {}
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

fn player_snapshot(bot: &Client, state: &BridgeState) -> Option<PlayerSnapshot> {
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
    // The same pose the remote entities report, for the third-person view of
    // our own body.
    let own_pose = match bot.get_component::<Pose>().map(|p| *p) {
        Some(Pose::Crouching) => EntityPose::Crouching,
        Some(Pose::FallFlying) => EntityPose::FallFlying,
        Some(Pose::Swimming) => EntityPose::Swimming,
        Some(Pose::SpinAttack) => EntityPose::SpinAttack,
        Some(Pose::Sleeping) => EntityPose::Sleeping,
        _ => EntityPose::Standing,
    };
    // Copied out, not borrowed: the guard would still hold the ECS read lock
    // while the lookups below take it again.
    let riding_on = bot.get_component::<plugins::RidingVehicle>().map(|r| *r);
    let riding = riding_on.is_some();
    // The mount's kind and id: the jump bar only shows for the animals that
    // can jump, and the mount screen has to know whose inventory it draws.
    let (vehicle_kind, vehicle_id) = match riding_on {
        Some(r) => {
            let ecs = bot.ecs.read();
            let kind = ecs
                .get::<EntityKindComponent>(r.vehicle)
                .map(|k| strip_minecraft_ns(k.to_str()));
            let id = ecs.get::<MinecraftEntityId>(r.vehicle).map(|i| i.0 as u32 as u64);
            (kind, id)
        }
        None => (None, None),
    };
    // Gliding and sleeping are both server-owned flags we only ever read.
    let gliding = bot
        .get_component::<azalea::entity::metadata::FallFlying>()
        .map(|f| f.0)
        .unwrap_or(false);
    let sleeping_at = bot
        .get_component::<azalea::entity::metadata::SleepingPos>()
        .and_then(|s| s.0)
        .map(|p| BlockPos { x: p.x, y: p.y, z: p.z });
    // Game mode and abilities: what the HUD shows, whether we can fly, and
    // whether a block breaks the instant we touch it.
    let game_mode = bot
        .get_component::<azalea::local_player::LocalGameMode>()
        .map(|g| g.current.to_id())
        .unwrap_or(0);
    let abilities = bot
        .get_component::<azalea::entity::PlayerAbilities>()
        .map(|a| crate::bridge::events::Abilities {
            invulnerable: a.invulnerable,
            flying: a.flying,
            may_fly: a.can_fly,
            instant_build: a.instant_break,
            fly_speed: a.flying_speed,
            walk_speed: a.walking_speed,
        })
        .unwrap_or_default();
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
        pose: own_pose,
        riding,
        vehicle_kind,
        vehicle_id,
        gliding,
        sleeping_at,
        mining,
        equipment: read_own_equipment(bot, &state.shared.lock()),
        game_mode,
        abilities,
    })
}

/// The local player's worn armor + offhand, read from the inventory menu's armor
/// slots (5..=8 = head, chest, legs, feet). Hands come from the hotbar on the
/// app side, so only armor + offhand are filled here.
fn read_own_equipment(bot: &Client, sh: &Shared) -> Equipment {
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
    let trim_at = |idx: usize| menu.slot(idx).and_then(|s| trim_of(sh, s));
    Equipment {
        head: name_at(armor0),
        chest: name_at(armor0 + 1),
        legs: name_at(armor0 + 2),
        feet: name_at(armor0 + 3),
        main_hand: None,
        off_hand: name_at(Player::OFFHAND_SLOT),
        trims: [
            trim_at(armor0),
            trim_at(armor0 + 1),
            trim_at(armor0 + 2),
            trim_at(armor0 + 3),
        ],
        // A player wears neither.
        body: None,
        saddle: None,
    }
}

/// Snapshot remote entities with a position within ~128 blocks.
fn entity_snapshots(bot: &Client, state: &BridgeState) -> Vec<EntitySnapshot> {
    const RANGE_SQ: f64 = 128.0 * 128.0;
    // Read our own pos/world BEFORE taking the write lock below
    // (parking_lot RwLock is not reentrant).
    let my_pos: Option<Vec3> = bot.get_component::<Position>().map(|p| **p);
    let my_world: Option<WorldName> = bot.get_component::<WorldName>().map(|w| w.clone());
    // Our own name decides the "hide for other teams" nametag rules.
    let own_name: Option<String> =
        bot.get_component::<GameProfileComponent>().map(|p| p.name.clone());

    // Registry-driven variant names, resolved once per snapshot (tiny maps).
    // Read before taking the ecs write lock (world lock is separate).
    let cat_reg = read_variant_registry(bot, "cat_variant");
    let wolf_reg = read_variant_registry(bot, "wolf_variant");
    let cow_reg = read_variant_registry(bot, "cow_variant");
    let chicken_reg = read_variant_registry(bot, "chicken_variant");
    let pig_reg = read_variant_registry(bot, "pig_variant");
    let frog_reg = read_variant_registry(bot, "frog_variant");
    let zombie_nautilus_reg = read_variant_registry(bot, "zombie_nautilus_variant");
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
            // A panda mid-sneeze: real vanilla drives the head-rear-back purely
            // off these two, no other timing state needed.
            Option<&azalea::entity::metadata::Sneezing>,
            Option<&azalea::entity::metadata::SneezeCounter>,
        ),
        (
            Option<&azalea::entity::metadata::Text>,
            Option<&azalea::entity::metadata::BlockDisplayBlockState>,
            Option<&azalea::entity::metadata::ItemDisplayItemStack>,
            Option<&azalea::entity::metadata::Translation>,
            Option<&azalea::entity::metadata::Scale>,
            Option<&azalea::entity::metadata::LeftRotation>,
            Option<&azalea::entity::metadata::RightRotation>,
            // How wide a lingering potion's puddle has spread.
            Option<&azalea::entity::metadata::Radius>,
            // The item an ominous item spawner is about to spawn.
            Option<&azalea::entity::metadata::OminousItemSpawnerItem>,
            // Copper golem oxidation stage, and the zombie nautilus's
            // temperate/warm coral-shell variant.
            Option<&azalea::entity::metadata::WeatherState>,
            Option<&azalea::entity::metadata::ZombieNautilusVariant>,
            // A goat's horns, each knocked off independently by ramming.
            Option<&azalea::entity::metadata::HasLeftHorn>,
            Option<&azalea::entity::metadata::HasRightHorn>,
            // How far a shulker's lid is open, 0..100.
            Option<&azalea::entity::metadata::Peek>,
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
            // Sheared sheep: vanilla drops the wool layer entirely.
            Option<&azalea::entity::metadata::SheepSheared>,
            // A creeper winding up, and a ghast/blaze about to shoot.
            Option<&azalea::entity::metadata::SwellDir>,
            Option<&azalea::entity::metadata::IsCharging>,
            // The rocket a firework is: its item, which carries the stars.
            Option<&azalea::entity::metadata::FireworksItem>,
            // A raid's raiders throw their arms up and cheer once it's won.
            Option<&azalea::entity::metadata::IsCelebrating>,
        ),
        // How the animal is holding itself (0.59.0): a dog told to sit, a cat
        // curled up, a fox asleep or stalking, a horse rearing, a bear standing
        // — and which of a boat's oars are being pulled.
        (
            Option<&azalea::entity::metadata::InSittingPose>,
            Option<&azalea::entity::metadata::IsLying>,
            Option<&azalea::entity::metadata::FoxSitting>,
            Option<&azalea::entity::metadata::FoxCrouching>,
            Option<&azalea::entity::metadata::Sleeping>,
            Option<&azalea::entity::metadata::PandaSitting>,
            Option<&azalea::entity::metadata::PolarBearStanding>,
            Option<&azalea::entity::metadata::AbstractHorseStanding>,
            Option<&azalea::entity::metadata::PaddleLeft>,
            Option<&azalea::entity::metadata::PaddleRight>,
            // Health, for the hearts over a mount's health bar.
            Option<&azalea::entity::metadata::Health>,
            // Parrots riding a player's shoulders (the variant, or nothing).
            Option<&azalea::entity::metadata::ShoulderParrotLeft>,
            Option<&azalea::entity::metadata::ShoulderParrotRight>,
            // Arrows and bee stingers left sticking in a body (0.60.0).
            Option<&azalea::entity::metadata::ArrowCount>,
            Option<&azalea::entity::metadata::StingerCount>,
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
            frame_item, frame_dir, frame_rot, sneezing_c, sneeze_counter_c,
        ),
        (
            disp_text,
            disp_block,
            disp_item,
            disp_translation,
            disp_scale,
            disp_left,
            disp_right,
            cloud_radius_c,
            ominous_item_c,
            weather_c,
            zombie_nautilus_v,
            has_left_horn_c,
            has_right_horn_c,
            peek_c,
        ),
        (
            as_small, as_arms, as_base, as_head, as_body, as_larm, as_rarm, as_lleg, as_rleg,
            sheared_c,
            swell_c,
            charging_c,
            firework_item,
            celebrating_c,
        ),
        (
            sit_c, lying_c, fox_sit_c, fox_crouch_c, sleeping_c, panda_sit_c, bear_stand_c,
            horse_stand_c, paddle_l_c, paddle_r_c, health_c, shoulder_l_c, shoulder_r_c,
            arrows_c, stingers_c,
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
        let is_cloud = kind_name == "area_effect_cloud";
        use azalea::registry::DataRegistry as _;
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
            // Copper golem oxidation stage (WeatherState): 0 unaffected,
            // 1 exposed, 2 weathered, 3 oxidized.
            "copper_golem" => weather_c.map(|w| match w.0 {
                azalea::entity::WeatheringCopperStateKind::Unaffected => 0,
                azalea::entity::WeatheringCopperStateKind::Exposed => 1,
                azalea::entity::WeatheringCopperStateKind::Weathered => 2,
                azalea::entity::WeatheringCopperStateKind::Oxidized => 3,
            }).unwrap_or(0),
            // Zombie nautilus: 0 temperate (plain shell), 1 warm (extra coral
            // growths — ZombieNautilusCoralModel).
            "zombie_nautilus" => zombie_nautilus_v
                .and_then(|v| zombie_nautilus_reg.get(v.0.protocol_id() as usize))
                .map(|n| if n == "warm" { 1 } else { 0 })
                .unwrap_or(0),
            _ => 0,
        };
        // Registry-driven variant name (cat/wolf/cow/chicken/pig/frog): the
        // metadata carries a protocol id → look up the name in the registry.
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
            let map_id = frame_item.and_then(|i| match &i.0 {
                ItemStack::Present(d) => d
                    .get_component::<components::MapId>()
                    .map(|m| m.id.max(0) as u32),
                ItemStack::Empty => None,
            });
            Some(events::FrameInfo {
                map_id,
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
                .or_else(|| {
                    // A team can hide its members' nametags outright, or hide
                    // them from everyone but their own side.
                    let p = profile?;
                    nametag_visible(&sh, &p.name, own_name.as_deref())
                        .then(|| team_decorated(&sh, &p.name))
                })
        };
        // Skin straight off the entity's profile so server NPCs (never in the
        // tab list) still render with their real skin.
        let (skin_url, skin_slim) = profile
            .map(|p| skin_of_properties(p))
            .unwrap_or((None, false));
        let sneaking = matches!(pose, Some(Pose::Crouching));
        // Only the poses that change how we draw the model; everything else
        // (croaking, digging, roaring …) reads as standing.
        let draw_pose = match pose {
            Some(Pose::Crouching) => EntityPose::Crouching,
            Some(Pose::FallFlying) => EntityPose::FallFlying,
            Some(Pose::Swimming) => EntityPose::Swimming,
            Some(Pose::SpinAttack) => EntityPose::SpinAttack,
            Some(Pose::Sleeping) => EntityPose::Sleeping,
            Some(Pose::Sitting) => EntityPose::Sitting,
            _ => EntityPose::Standing,
        };
        let sprinting = sprinting.map(|s| s.0).unwrap_or(false);
        let invisible = invisible.map(|i| i.0).unwrap_or(false);
        // Dropped-item entities carry their stack as metadata; pull the item's
        // registry name so the app can draw its real icon. An ominous item
        // spawner carries its about-to-spawn item the same way, under its own
        // metadata field — the two never coexist on one entity.
        let (item, item_count) = match item
            .map(|i| &i.0)
            .or_else(|| ominous_item_c.map(|i| &i.0))
        {
            Some(ItemStack::Present(d)) => {
                (Some(strip_minecraft_ns(d.kind.to_str())), d.count.max(1) as u32)
            }
            _ => (None, 1),
        };
        let sneeze_head_pitch = sneezing_c
            .is_some_and(|s| **s)
            .then(|| panda_sneeze_head_pitch(sneeze_counter_c.map_or(0, |c| **c)));
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
            // A mannequin is vanilla's own "player avatar without a player" —
            // command-summoned, wears a real skin via the same profile field
            // real players carry, and draws through the identical humanoid
            // pipeline. Without this it falls back to a plain box, since it
            // has no MODEL_MOBS/HUMANOID_MOBS entry of its own and never
            // should — that table is for mobs with a fixed skin, not one
            // that ships its skin over the wire per-entity like a player does.
            is_player: matches!(kind, EntityKind::Player | EntityKind::Mannequin),
            sneaking,
            pose: draw_pose,
            cape_url: profile.and_then(|p| cape_of_properties(p)),
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
            goat_left_horn: has_left_horn_c.map_or(true, |h| **h),
            goat_right_horn: has_right_horn_c.map_or(true, |h| **h),
            sneeze_head_pitch,
            item_count,
            spawn_data: 0,
            sheared: sheared_c.is_some_and(|s| **s),
            health: health_c.map(|h| **h),
            max_health: None,
            // What is still sticking in this body: arrows shot into it and bee
            // stingers left behind. Vanilla draws one of each, up to the count.
            // A lingering potion's puddle, once it has one.
            cloud_radius: is_cloud.then(|| cloud_radius_c.map(|r| **r)).flatten(),
            // A rocket's stars. Vanilla reads exactly this component when the
            // rocket bursts; a rocket with no star in it simply has none.
            firework: firework_item
                .and_then(|f| match &f.0 {
                    ItemStack::Present(data) => data.get_component::<components::Fireworks>(),
                    ItemStack::Empty => None,
                })
                .map(|fw| {
                    fw.explosions
                        .iter()
                        .map(|e| FireworkStar {
                            shape: match e.shape {
                                components::FireworkExplosionShape::SmallBall => 0,
                                components::FireworkExplosionShape::LargeBall => 1,
                                components::FireworkExplosionShape::Star => 2,
                                components::FireworkExplosionShape::Creeper => 3,
                                components::FireworkExplosionShape::Burst => 4,
                            },
                            colors: e.colors.clone(),
                            fade_colors: e.fade_colors.clone(),
                            trail: e.has_trail,
                            twinkle: e.has_twinkle,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            arrows: arrows_c.map_or(0, |a| (**a).clamp(0, 12) as u8),
            stingers: stingers_c.map_or(0, |a| (**a).clamp(0, 12) as u8),
            // A tamed parrot rides its owner's shoulder; the metadata carries
            // the bird's colour, or nothing when that shoulder is empty.
            shoulders: [
                shoulder_l_c.and_then(|c| c.0.0).map(|v| v as i32),
                shoulder_r_c.and_then(|c| c.0.0).map(|v| v as i32),
            ],
            // Vanilla reads each of these off a different flag, and more than
            // one can be set at once (a sleeping fox is also "sitting"), so the
            // order here is the order vanilla's models check them in.
            pose_kind: {
                let sitting = sit_c.is_some_and(|s| **s)
                    || fox_sit_c.is_some_and(|s| **s)
                    || panda_sit_c.is_some_and(|s| **s);
                let rowing = paddle_l_c.is_some() || paddle_r_c.is_some();
                if sleeping_c.is_some_and(|s| **s) || lying_c.is_some_and(|s| **s) {
                    AnimalPose::Lying
                } else if sitting {
                    AnimalPose::Sitting
                } else if bear_stand_c.is_some_and(|s| **s)
                    || horse_stand_c.is_some_and(|s| **s)
                {
                    AnimalPose::Rearing
                } else if fox_crouch_c.is_some_and(|s| **s) {
                    AnimalPose::Crouching
                } else if rowing {
                    AnimalPose::Rowing {
                        left: paddle_l_c.is_some_and(|p| **p),
                        right: paddle_r_c.is_some_and(|p| **p),
                    }
                } else if celebrating_c.is_some_and(|c| **c) {
                    AnimalPose::Celebrating
                } else {
                    AnimalPose::Standing
                }
            },
            // A creeper's fuse is lit (SwellDir counts up), or a ghast/blaze is
            // winding up a shot.
            swelling: swell_c.is_some_and(|s| s.0 > 0),
            charging: charging_c.is_some_and(|c| **c),
            peek: peek_c.map_or(0, |p| p.0),
            leashed_to: None,
            head_yaw: None,
            riding_on: None,
        });
    }
    drop(ecs);

    // Attach the raw-packet side-tables (equipment, spawn data, leads) and drop
    // entries for entities that have despawned.
    {
        let mut sh = state.shared.lock();
        let live: std::collections::HashSet<u64> = out.iter().map(|e| e.id).collect();
        if !sh.entity_equipment.is_empty() {
            sh.entity_equipment.retain(|k, _| live.contains(k));
            for e in &mut out {
                if let Some(eq) = sh.entity_equipment.get(&e.id) {
                    e.equipment = eq.clone();
                }
            }
        }
        if !sh.spawn_data.is_empty() {
            sh.spawn_data.retain(|k, _| live.contains(k));
            for e in &mut out {
                if let Some(&d) = sh.spawn_data.get(&e.id) {
                    e.spawn_data = d;
                }
            }
        }
        if !sh.leashes.is_empty() {
            sh.leashes.retain(|k, _| live.contains(k));
            for e in &mut out {
                e.leashed_to = sh.leashes.get(&e.id).copied();
            }
        }
        if !sh.max_health.is_empty() {
            sh.max_health.retain(|k, _| live.contains(k));
            for e in &mut out {
                e.max_health = sh.max_health.get(&e.id).copied();
            }
        }
        if !sh.head_yaw.is_empty() {
            sh.head_yaw.retain(|k, _| live.contains(k));
            for e in &mut out {
                e.head_yaw = sh.head_yaw.get(&e.id).copied();
            }
        }
        if !sh.riders.is_empty() {
            // A rider whose vehicle went out of range keeps its seat: the
            // vehicle entry is what has to be live, not the rider's own.
            sh.riders.retain(|k, (v, _)| live.contains(k) || live.contains(v));
            for e in &mut out {
                e.riding_on = sh.riders.get(&e.id).copied();
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

#[cfg(test)]
mod resource_pack_tests {
    use super::*;
    use std::io::Write as _;

    fn temp_zip(name: &str, mcmeta: Option<&str>) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dolphin-pack-test-{}-{name}.zip",
            std::process::id()
        ));
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        if let Some(meta) = mcmeta {
            zip.start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(meta.as_bytes()).unwrap();
        }
        zip.start_file(
            "assets/minecraft/textures/block/test.txt",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"test").unwrap();
        zip.finish().unwrap();
        path
    }

    #[test]
    fn sha1_matches_the_standard_vector() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn resource_pack_archive_requires_valid_mcmeta_and_hash() {
        let good = temp_zip("good", Some(r#"{"pack":{"pack_format":84,"description":"ok"}}"#));
        let hash = sha1_hex(&std::fs::read(&good).unwrap());
        assert!(validate_pack_archive(&good, Some(&hash)).is_ok());
        assert!(validate_pack_archive(&good, Some("0000000000000000000000000000000000000000")).is_err());
        std::fs::remove_file(good).unwrap();

        let bad = temp_zip("missing-meta", None);
        assert!(validate_pack_archive(&bad, None).is_err());
        std::fs::remove_file(bad).unwrap();
    }
}

#[cfg(test)]
mod panda_sneeze_tests {
    use super::*;

    #[test]
    fn starts_level_and_rears_back_over_the_first_15_ticks() {
        assert_eq!(panda_sneeze_head_pitch(0), 0.0);
        // Halfway through the rear-back (tick 7): vanilla's own
        // `-0.7853982 * 7.0 / 14.0` rad, in degrees.
        assert!((panda_sneeze_head_pitch(7) - (-22.5)).abs() < 0.01);
        assert!((panda_sneeze_head_pitch(14) - (-45.0)).abs() < 0.01);
    }

    #[test]
    fn releases_back_to_level_over_the_last_5_ticks() {
        assert!((panda_sneeze_head_pitch(15) - (-45.0)).abs() < 0.01);
        assert!((panda_sneeze_head_pitch(17) - (-27.0)).abs() < 0.01);
        assert!((panda_sneeze_head_pitch(19) - (-9.0)).abs() < 0.01);
    }

    #[test]
    fn clamps_out_of_range_ticks() {
        assert_eq!(panda_sneeze_head_pitch(-5), panda_sneeze_head_pitch(0));
        assert_eq!(panda_sneeze_head_pitch(100), panda_sneeze_head_pitch(19));
    }
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
            resource_pack_policy: crate::settings::ServerResourcePackPolicy::Prompt,
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
            resource_pack_policy: crate::settings::ServerResourcePackPolicy::Prompt,
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
