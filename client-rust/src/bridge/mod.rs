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

mod account;
mod convert;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context as _;
use azalea::core::entity_id::MinecraftEntityId;
use azalea::protocol::address::ServerAddr;
use azalea::protocol::packets::PROTOCOL_VERSION;
use azalea::protocol::resolve::resolve_address;
use azalea::core::position::{BlockPos as AzBlockPos, ChunkPos as AzChunkPos, Vec3};
use azalea::ecs::entity::Entity;
use azalea::entity::dimensions::EntityDimensions;
use azalea::entity::inventory::Inventory;
use azalea::entity::metadata::{CustomName, Health};
use azalea::entity::{EntityKindComponent, LocalEntity, LookDirection, Physics, Position};
use azalea::local_player::{Experience, Hunger};
use azalea::player::GameProfileComponent;
use azalea::prelude::*;
use azalea::protocol::packets::game::{ClientboundGamePacket, ClientboundSetTime};
use azalea::core::sound::CustomSound;
use azalea::registry::Holder;
use azalea::registry::builtin::{EntityKind, SoundEvent};
use azalea::world::{Section, WorldName};
use azalea::{SprintDirection, WalkDirection};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use tracing::{debug, info, warn};

use crate::types::{BlockPos, ChunkPos, SectionData, SectionPos, StateId};
use convert::{ChunkLight, SectionLight};
use events::{AccountConfig, BridgeOptions, Command, EntitySnapshot, GameEvent, ItemSnapshot, PlayerSnapshot};

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
async fn preflight(address: &str, account: &AccountConfig) -> Result<(), String> {
    use std::time::Duration;
    use tokio::time::timeout;

    // 1. Parse the address.
    let server_addr = ServerAddr::try_from(address)
        .map_err(|_| format!("Ungültige Serveradresse: „{address}“"))?;

    // 2. Resolve DNS/SRV.
    let socket = timeout(Duration::from_secs(8), resolve_address(&server_addr))
        .await
        .map_err(|_| format!("DNS-Auflösung für „{}“ dauert zu lange (Timeout).", server_addr.host))?
        .map_err(|e| {
            format!(
                "Server „{}“ wurde nicht gefunden (DNS: {e}). Adresse richtig geschrieben?",
                server_addr.host
            )
        })?;
    info!(%socket, "preflight: resolved");

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

    // 4. Status-Ping: catches the most common real-world failure — a server
    //    that runs a different Minecraft version. A failed ping alone does NOT
    //    block the join (some servers hide their status).
    match timeout(Duration::from_secs(6), azalea::ping::ping_server(address)).await {
        Ok(Ok(status)) => {
            info!(
                version = %status.version.name,
                protocol = status.version.protocol,
                players = status.players.online,
                "preflight: server status"
            );
            if status.version.protocol != PROTOCOL_VERSION {
                return Err(format!(
                    "Der Server läuft Minecraft {} (Protokoll {}) — DolphinClient unterstützt nur \
                     Minecraft 26.1 (Protokoll {}).",
                    status.version.name, status.version.protocol, PROTOCOL_VERSION
                ));
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

    Ok(())
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
            rt.block_on(async move {
                // Fail fast with a precise message instead of azalea's silent
                // failure modes (see `preflight`).
                if let Err(reason) = preflight(&opts.address, &opts.account).await {
                    warn!(reason, "bridge: preflight failed");
                    let _ = event_tx.send(GameEvent::Disconnected { reason });
                    return;
                }
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
                info!(address = %opts.address, "bridge: connecting");
                // `start()` runs the whole client lifecycle; it returns only
                // once `Client::exit` fires (we call it on disconnect) or the
                // address fails to parse/resolve.
                ClientBuilder::new()
                    .set_handler(handle)
                    .set_state(state)
                    .reconnect_after(None::<std::time::Duration>) // app owns reconnects
                    .start(account, opts.address.as_str())
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
    /// Cheap comparable key of the last emitted hotbar.
    last_hotbar: Option<(Vec<Option<(String, u32)>>, u8)>,
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
    match event {
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
            let text = convert::strip_legacy_codes(&packet.message().to_string());
            state.emit(&bot, GameEvent::Chat { text });
        }
        Event::ReceiveChunk(pos) => on_receive_chunk(&bot, &state, pos),
        Event::Packet(packet) => on_packet(&bot, &state, &packet),
        Event::Tick => on_tick(&bot, &state),
        _ => {}
    }
}

fn on_login(bot: &Client, state: &BridgeState) {
    // The world (and therefore all cached light) is swapped on respawn or
    // dimension change; Login fires again in that case.
    state.shared.lock().light.clear();
    let username = bot
        .get_component::<GameProfileComponent>()
        .map(|p| p.name.clone())
        .unwrap_or_default();
    info!(username, "bridge: logged in");
    state.emit(bot, GameEvent::Connected { username });
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
        // Chunk may legitimately be all air, but a missing chunk is worth a log.
        let present = {
            let world = bot.world();
            let world = world.read();
            world.chunks.get(&pos).is_some()
        };
        if !present {
            warn!(x = pos.x, z = pos.z, "bridge: ReceiveChunk for a chunk azalea doesn't have; skipping");
        }
        return;
    }
    debug!(x = pos.x, z = pos.z, sections = sections.len(), "bridge: chunk sections");
    for (pos, data) in sections {
        state.emit(bot, GameEvent::Section { pos, data });
    }
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
        _ => {}
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

    // 2. Local player snapshot, every tick.
    if let Some(snap) = player_snapshot(bot) {
        state.emit(bot, GameEvent::PlayerState(Box::new(snap)));
    }

    let tick = {
        let mut shared = state.shared.lock();
        shared.tick += 1;
        shared.tick
    };

    // 3. Entity snapshots, every 2nd tick.
    if tick.is_multiple_of(2) {
        let entities = entity_snapshots(bot);
        state.emit(bot, GameEvent::Entities(entities));
    }

    // 4. Hotbar, when changed.
    maybe_emit_hotbar(bot, state);
}

fn apply_command(bot: &Client, state: &BridgeState, cmd: Command) {
    match cmd {
        Command::SetDirection { yaw, pitch } => bot.set_direction(yaw, pitch),
        Command::Move { forward, strafe, sprint } => apply_move(bot, forward, strafe, sprint),
        Command::Jump(jumping) => bot.set_jumping(jumping),
        Command::Sneak(sneaking) => bot.set_crouching(sneaking),
        Command::Chat(msg) => bot.chat(msg), // leading '/' → command packet
        Command::Mine(pos) => bot.start_mining(AzBlockPos::new(pos.x, pos.y, pos.z)),
        Command::Interact(pos) => bot.block_interact(AzBlockPos::new(pos.x, pos.y, pos.z)),
        Command::Attack(id) => {
            // Bridge entity ids are MinecraftEntityId (i32) round-tripped
            // through u64 (see entity_snapshots).
            match bot.entity_id_by_minecraft_id(MinecraftEntityId(id as u32 as i32)) {
                Some(entity) => bot.attack(entity),
                None => warn!(id, "bridge: Attack for unknown entity id; ignoring"),
            }
        }
        Command::SelectHotbar(slot) => {
            if slot <= 8 {
                bot.set_selected_hotbar_slot(slot);
            } else {
                warn!(slot, "bridge: SelectHotbar slot out of range; ignoring");
            }
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
/// Convention: strafe > 0 = right (D key), strafe < 0 = left (A key).
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
    let food = bot.get_component::<Hunger>().map(|h| h.food).unwrap_or(20);
    let xp_level = bot.get_component::<Experience>().map(|x| x.level).unwrap_or(0);
    Some(PlayerSnapshot {
        pos: [pos.x, pos.y, pos.z],
        velocity: [velocity.x, velocity.y, velocity.z],
        yaw,
        pitch,
        eye_height,
        on_ground,
        health,
        food,
        xp_level,
    })
}

/// Snapshot remote entities with a position within ~128 blocks.
fn entity_snapshots(bot: &Client) -> Vec<EntitySnapshot> {
    const RANGE_SQ: f64 = 128.0 * 128.0;
    // Read our own pos/world BEFORE taking the write lock below
    // (parking_lot RwLock is not reentrant).
    let my_pos: Option<Vec3> = bot.get_component::<Position>().map(|p| **p);
    let my_world: Option<WorldName> = bot.get_component::<WorldName>().map(|w| w.clone());

    let mut out = Vec::new();
    let mut ecs = bot.ecs.write();
    let mut query = ecs.query::<(
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
    )>();
    for (ent, mc_id, kind, pos, look, world_name, dims, custom_name, profile, local) in
        query.iter(&ecs)
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
        let name = profile.map(|p| p.name.clone()).or_else(|| {
            custom_name
                .and_then(|c| c.0.as_ref())
                .map(|t| convert::strip_legacy_codes(&t.to_string()))
        });
        out.push(EntitySnapshot {
            id: mc_id.0 as u32 as u64,
            kind: strip_minecraft_ns(kind.to_str()),
            pos: [pos.x, pos.y, pos.z],
            yaw: look.y_rot(),
            pitch: look.x_rot(),
            width: dims.map(|d| d.width).unwrap_or(0.6),
            height: dims.map(|d| d.height).unwrap_or(1.8),
            name,
            is_player: kind == EntityKind::Player,
        });
    }
    drop(ecs);
    out
}

fn strip_minecraft_ns(s: &str) -> String {
    s.strip_prefix("minecraft:").unwrap_or(s).to_string()
}

fn maybe_emit_hotbar(bot: &Client, state: &BridgeState) {
    let Some((slots, selected)) = read_hotbar(bot) else {
        return;
    };
    let key: (Vec<Option<(String, u32)>>, u8) = (
        slots
            .iter()
            .map(|s| s.as_ref().map(|i| (i.item.clone(), i.count)))
            .collect(),
        selected,
    );
    {
        let mut shared = state.shared.lock();
        if shared.last_hotbar.as_ref() == Some(&key) {
            return;
        }
        shared.last_hotbar = Some(key);
    }
    state.emit(bot, GameEvent::Hotbar { slots, selected });
}

fn read_hotbar(bot: &Client) -> Option<(Box<[Option<ItemSnapshot>; 9]>, u8)> {
    let inv = bot.get_component::<Inventory>()?;
    let selected = inv.selected_hotbar_slot;
    // Always the player inventory (slots 36..=44), not any open container.
    let menu = &inv.inventory_menu;
    let mut slots: Box<[Option<ItemSnapshot>; 9]> = Box::new(std::array::from_fn(|_| None));
    for (i, idx) in menu.hotbar_slots_range().enumerate().take(9) {
        let Some(stack) = menu.slot(idx) else { continue };
        if stack.is_present() {
            slots[i] = Some(ItemSnapshot {
                item: strip_minecraft_ns(stack.kind().to_str()),
                count: stack.count().max(0) as u32,
            });
        }
    }
    Some((slots, selected))
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
