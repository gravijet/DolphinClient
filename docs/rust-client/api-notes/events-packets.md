# azalea 0.16.0+mc26.1 — Event system & raw packet access

Verified against crate sources in
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/azalea{,-client,-protocol,-core}-0.16.0+mc26.1`.
NOTE: azalea 0.16 requires **nightly Rust** (`azalea` uses `#![feature(type_changing_struct_update)]`,
`azalea-client` uses `#![feature(error_generic_member_access)]`).

---

## 1. The full `azalea::Event` enum

Defined in `azalea-0.16.0/src/events.rs`, re-exported as `azalea::Event` (also `azalea::events::Event`,
and in `azalea::prelude`). It is `#[derive(Clone, Debug)]` and **`#[non_exhaustive]`** — always include a
`_ => {}` arm.

```rust
// azalea::events::Event  (exact source, 0.16.0+mc26.1)
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Event {
    /// Right after switching into the Game state, before spawn. Good place for
    /// Client::set_client_information.
    Init,
    /// Fired on receiving a login packet (after Init, before Spawn). Position may
    /// still be Vec3::ZERO. Can fire multiple times (world switches).
    Login,
    /// Player is fully in a loaded chunk and ready. Fires again on respawn/world switch.
    Spawn,
    /// A chat message (system/player/disguised) was received.
    Chat(ChatPacket),                                   // azalea_client::chat::ChatPacket (see below)
    /// 20x/second, only while a world is loaded.
    Tick,
    /// Every clientbound game-state packet. Only with the "packet-event" cargo
    /// feature, which is ON by default.
    #[cfg(feature = "packet-event")]
    Packet(Arc<azalea_protocol::packets::game::ClientboundGamePacket>),
    /// Player added to tab list.
    AddPlayer(PlayerInfo),                              // azalea_client::player::PlayerInfo
    /// Player removed from tab list.
    RemovePlayer(PlayerInfo),
    /// Tab-list entry updated (gamemode / display name / latency).
    UpdatePlayer(PlayerInfo),
    /// Our client died. Payload is the combat-kill packet if the server sent one.
    Death(Option<Arc<ClientboundPlayerCombatKill>>),    // azalea_protocol::packets::game::c_player_combat_kill::ClientboundPlayerCombatKill
    /// Server sent a KeepAlive; payload is its id. (azalea already replied automatically.)
    KeepAlive(u64),
    /// Disconnected; payload is the kick reason if known.
    Disconnect(Option<FormattedText>),                  // azalea_chat::FormattedText
    /// Initial connection failed.
    ConnectionFailed(Arc<ConnectionError>),             // azalea_protocol::connect::ConnectionError (Io(io::Error) only)
    /// A full chunk packet was received (fires AFTER azalea queued it for parsing).
    ReceiveChunk(ChunkPos),                             // azalea_core::position::ChunkPos { x: i32, z: i32 }
}
```

Payload type locations:

| Type | Path |
|---|---|
| `ChatPacket` | `azalea_client::chat::ChatPacket` (re-exported `azalea::chat::ChatPacket`) — `enum { System(Arc<ClientboundSystemChat>), Player(Arc<ClientboundPlayerChat>), Disguised(Arc<ClientboundDisguisedChat>) }`; helpers: `.message() -> FormattedText`, `.content() -> String`, `.sender() -> Option<String>`, `.sender_uuid() -> Option<Uuid>` |
| `PlayerInfo` | `azalea_client::player::PlayerInfo` — `{ profile: GameProfile, uuid: Uuid, gamemode: GameMode, latency: i32, display_name: Option<Box<FormattedText>> }` |
| `ClientboundGamePacket` | `azalea_protocol::packets::game::ClientboundGamePacket` (also via `azalea::protocol::...`) |
| `ConnectionError` | `azalea_protocol::connect::ConnectionError` — currently only variant `Io(std::io::Error)` |
| `ChunkPos` | `azalea_core::position::ChunkPos` |

There is **no** `Event` variant for "chunk unloaded" — catch
`ClientboundGamePacket::ForgetLevelChunk` (`{ pos: ChunkPos }`) via `Event::Packet` for that.

---

## 2. Receiving events

### a) ClientBuilder handler (recommended)

Handler is a **plain `fn` pointer**, not a closure: `pub type HandleFn<S, Fut> = fn(Client, Event, S) -> Fut;`
(`azalea/src/lib.rs`). You cannot capture environment — put shared data in the state `S`.

`S: Default + Send + Sync + Clone + Component + 'static`. The return type `R: Send` is arbitrary and
**discarded** (each invocation is `tokio::task::spawn_local`'d; errors are NOT logged). Handler
invocations are spawned concurrently — the handler can be mid-await on one event while the next arrives.

```rust
use azalea::prelude::*; // Client, ClientBuilder, Event, Account, Component, ...

#[derive(Default, Clone, Component)]
pub struct State;

#[tokio::main]
async fn main() {
    let account = Account::offline("dolphin"); // or Account::microsoft("email").await.unwrap()
    ClientBuilder::new()                       // ClientBuilder<NoState, ()>
        .set_state(State)                      // optional; S::default() used otherwise
        .set_handler(handle)                   // -> ClientBuilder<State, R>
        .start(account, "localhost:25565")     // -> AppExit; runs ~forever
        .await;
}

async fn handle(bot: Client, event: Event, _state: State) -> eyre::Result<()> {
    match event {
        Event::Spawn => bot.chat("hello"),
        Event::Chat(m) => println!("{}", m.message().to_ansi()),
        _ => {}
    }
    Ok(())
}
```

### b) Channel/stream without the builder callback: `Client::join`

`azalea::Client` (moved to azalea crate root in 0.16; struct lives in `azalea/src/client_impl/mod.rs`):

```rust
pub struct Client {
    pub entity: bevy_ecs::entity::Entity,
    pub ecs: Arc<parking_lot::RwLock<bevy_ecs::world::World>>,  // RwLock in 0.16 (was Mutex)
}

impl Client {
    pub async fn join(account: Account, address: impl ResolvableAddr)
        -> Result<(Self, tokio::sync::mpsc::UnboundedReceiver<Event>), ResolveError>;
    pub async fn join_with_proxy(...) -> same;
    pub async fn start_client(opts: StartClientOpts) -> Self;
}
```

**Critical caveat:** `Client::join` internally calls `start_ecs_runner`, which uses
`tokio::task::spawn_local` and therefore **panics unless called inside a tokio `LocalSet`**
(documented in `azalea-client/src/client.rs`). The LocalSet must keep being polled or the ECS
(and networking) stops. Working pattern for a wgpu app — dedicated bot thread:

```rust
use azalea::{Client, Event, prelude::Account};
use tokio::{runtime::Builder, sync::mpsc, task::LocalSet};

fn spawn_bot_thread(game_tx: std::sync::mpsc::Sender<Event>) {
    std::thread::spawn(move || {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();
        let local = LocalSet::new();
        local.block_on(&rt, async move {
            let account = Account::offline("dolphin");
            let (client, mut rx): (Client, mpsc::UnboundedReceiver<Event>) =
                Client::join(account, "localhost:25565").await.unwrap();
            // client is Clone + Send + Sync: hand it to the render thread if needed.
            while let Some(event) = rx.recv().await {
                let _ = game_tx.send(event); // forward to the render/main thread
            }
        });
    });
}
```

Note: `Client::join` gives NO auto-reconnect, no pathfinder/bot plugins beyond the defaults it adds
(`DefaultPlugins + DefaultBotPlugins + DefaultSwarmPlugins` via `StartClientOpts::new`), and if the
connection dies the receiver just yields `Event::Disconnect` then closes activity.

### c) ECS-level access (no channel at all)

Inside the ECS you can read `azalea_client::packet::game::ReceiveGamePacketEvent`
(a bevy `Message`) with a system — useful if you write your own bevy `Plugin` and
`ClientBuilder::new().add_plugins(MyPlugin)`:

```rust
pub struct ReceiveGamePacketEvent { pub entity: Entity, pub packet: Arc<ClientboundGamePacket> }
```

Also: `azalea::tick_broadcast::TickBroadcast` / `UpdateBroadcast` resources
(`tokio::sync::broadcast::Sender<()>`) fire every GameTick / every Update;
`client.get_tick_broadcaster()` / `client.wait_ticks(n)` exist on `Client`.

---

## 3. Raw clientbound packets: `Event::Packet`

- Variant: `Event::Packet(Arc<ClientboundGamePacket>)`, gated on cargo feature **`packet-event`
  (in azalea's default features, so on unless you use `default-features = false`)**.
- Only **game-state** packets are surfaced; config/login/status-state packets are not in `Event`.
- Enum: `azalea_protocol::packets::game::ClientboundGamePacket`, one variant per packet in
  PascalCase, each wrapping a struct named `Clientbound<Name>` (structs are re-exported at
  `azalea_protocol::packets::game::*`, and their private-field siblings live in per-packet modules
  like `packets::game::c_level_chunk_with_light`).
- Relevant variants for a renderer: `LevelChunkWithLight`, `LightUpdate`, `BlockUpdate`,
  `SectionBlocksUpdate`, `ForgetLevelChunk`, `SetChunkCacheCenter`, `SetChunkCacheRadius`,
  `ChunksBiomes`, `BlockEntityData`, `Login`, `Respawn`, `SetTime`, `GameEvent`.

### Exact packet struct definitions (verified source)

```rust
// azalea_protocol::packets::game::c_level_chunk_with_light
pub struct ClientboundLevelChunkWithLight {
    pub x: i32,                                       // chunk X (not a ChunkPos: read order x,z)
    pub z: i32,
    pub chunk_data: ClientboundLevelChunkPacketData,
    pub light_data: ClientboundLightUpdatePacketData, // same struct as the LightUpdate packet uses
}

pub struct ClientboundLevelChunkPacketData {
    pub heightmaps: Vec<(HeightmapKind, Box<[u64]>)>, // azalea_core::heightmap_kind::HeightmapKind
    /// RAW, UNPARSED section bytes (the vanilla "Data" field: per-section
    /// block_count:i16 + block-state PalettedContainer + biome PalettedContainer).
    pub data: Arc<Box<[u8]>>,
    pub block_entities: Vec<BlockEntity>,
}

pub struct BlockEntity {          // packets::game::c_level_chunk_with_light::BlockEntity
    pub packed_xz: u8,            // (x&15)<<4 | (z&15)
    pub y: u16,
    pub kind: BlockEntityKind,    // azalea_registry::builtin::BlockEntityKind
    pub data: simdnbt::owned::Nbt,
}

// azalea_protocol::packets::game::c_light_update
pub struct ClientboundLightUpdate {
    pub x: i32,   // #[var] varint
    pub z: i32,   // #[var] varint
    pub light_data: ClientboundLightUpdatePacketData,
}

pub struct ClientboundLightUpdatePacketData {         // Default + Clone + PartialEq
    pub sky_y_mask: BitSet,                           // azalea_core::bitset::BitSet (Java BitSet, u64 words)
    pub block_y_mask: BitSet,
    pub empty_sky_y_mask: BitSet,
    pub empty_block_y_mask: BitSet,
    pub sky_updates: Arc<Box<[Box<[u8]>]>>,           // one 2048-byte nibble array per SET bit in sky_y_mask
    pub block_updates: Arc<Box<[Box<[u8]>]>>,         // ditto for block_y_mask
}
```

Light layout semantics (vanilla protocol; azalea only parses the arrays, it does **not**
interpret or store light anywhere — your renderer must):

- Mask bit `i` covers section `min_section_y - 1 + i`; there are `section_count + 2` bits
  (one extra section below and above the world). For a 26.1 overworld (Y −64..320, 24 sections)
  that's 26 bits, bit 0 = Y section −5 (i.e. y = −80..−65 region below the world).
- `sky_updates[k]` corresponds to the k-th set bit of `sky_y_mask` in ascending bit order.
  Each array is 2048 bytes = 4096 nibbles, index `(y<<8)|(z<<4)|x`, low nibble = even index.
- `empty_*_y_mask` marks sections whose light is all zeros (no array sent for them).
- `BitSet` API: `.index(usize) -> bool` (panics OOB), `.get(usize) -> Option<bool>`, `.len()`.

### Compilable match example

```rust
use std::sync::Arc;

use azalea::Event;
use azalea_protocol::packets::game::ClientboundGamePacket;

fn on_event(event: &Event) {
    if let Event::Packet(packet) = event {
        match &**packet {
            ClientboundGamePacket::LevelChunkWithLight(p) => {
                let (cx, cz) = (p.x, p.z);
                let raw_sections: &Arc<Box<[u8]>> = &p.chunk_data.data;
                let light = &p.light_data;
                // pair the k-th array with the k-th set mask bit:
                let mut k = 0;
                for bit in 0.. {
                    let Some(set) = light.sky_y_mask.get(bit) else { break };
                    if set {
                        let nibbles: &[u8] = &light.sky_updates[k]; // 2048 bytes
                        let _ = (bit, nibbles);
                        k += 1;
                    }
                }
                let _ = (cx, cz, raw_sections);
            }
            ClientboundGamePacket::LightUpdate(p) => {
                let _ = (p.x, p.z, &p.light_data.block_updates);
            }
            ClientboundGamePacket::ForgetLevelChunk(p) => {
                let _ = p.pos; // ChunkPos { x, z }
            }
            _ => {}
        }
    }
}
```

`chunk_data.data` can be parsed with `azalea_world::Chunk::read_with_world_height(&mut Cursor::new(&data), ...)`
— but azalea already does this into its `World` (see §5); for a renderer it's usually better to read
block states from `client.world()` after `Event::ReceiveChunk` than to re-parse the packet.

---

## 4. Block update packets

```rust
// azalea_protocol::packets::game::c_block_update
pub struct ClientboundBlockUpdate {
    pub pos: BlockPos,            // azalea_core::position::BlockPos { x: i32, y: i32, z: i32 }
    pub block_state: BlockState,  // azalea_block::BlockState (global palette id; .id() -> u32)
}

// azalea_protocol::packets::game::c_section_blocks_update
pub struct ClientboundSectionBlocksUpdate {
    pub section_pos: ChunkSectionPos,        // azalea_core::position::ChunkSectionPos { x: i32, y: i32, z: i32 } (section coords)
    pub states: Vec<BlockStateWithPosition>, // module path: packets::game::c_section_blocks_update::BlockStateWithPosition
}
pub struct BlockStateWithPosition {
    pub pos: ChunkSectionBlockPos,  // azalea_core::position::ChunkSectionBlockPos { x: u8, y: u8, z: u8 } (0..15 within section)
    pub state: BlockState,
}
```

Wire encoding of each entry (already decoded for you): varint u64, `state = data >> 12`,
`x = (data>>8)&15, z = (data>>4)&15, y = data&15`.

```rust
use azalea::Event;
use azalea_protocol::packets::game::ClientboundGamePacket;

fn on_block_events(event: &Event) {
    if let Event::Packet(packet) = event {
        match &**packet {
            ClientboundGamePacket::BlockUpdate(p) => {
                println!("set {:?} -> state id {}", p.pos, p.block_state.id());
            }
            ClientboundGamePacket::SectionBlocksUpdate(p) => {
                for e in &p.states {
                    let wx = p.section_pos.x * 16 + e.pos.x as i32;
                    let wy = p.section_pos.y * 16 + e.pos.y as i32;
                    let wz = p.section_pos.z * 16 + e.pos.z as i32;
                    println!("set ({wx},{wy},{wz}) -> {}", e.state.id());
                }
            }
            _ => {}
        }
    }
}
```

Renderer tip: azalea also applies both packets to its own world copy
(`QueuedServerBlockUpdates` component, drained in `Update` by `handle_block_update_event`), so you
can alternatively just re-mesh chunks on `Event::Packet(BlockUpdate/SectionBlocksUpdate)` and read
authoritative state from `client.world()`.

---

## 5. What azalea handles automatically vs what you must do

Handled automatically by `azalea_client::DefaultPlugins` (+ `azalea::bot::DefaultBotPlugins`,
both added by `ClientBuilder::new()`):

- **KeepAlive**: auto-replies `ServerboundKeepAlive` with same id (and still emits `Event::KeepAlive`).
- **Ping/Pong**: `PongPlugin` auto-replies in both game and config states.
- **Chunk batching**: replies `ServerboundChunkBatchReceived` with a computed desired rate.
- **Chunk parsing**: `ChunksPlugin` parses `LevelChunkWithLight.chunk_data` into the shared
  `azalea_world::World` (block states + biomes + heightmaps). Get it via `client.world()`
  (`Arc<RwLock<World>>`; `Instance` was renamed to `World` in 0.16).
- **Block updates**: `BlockUpdatePlugin` applies BlockUpdate/SectionBlocksUpdate to that world.
- **Login/config flow**: full handshake→login→config→game transition, `ServerboundSelectKnownPacks`
  (empty list), brand `ServerboundCustomPayload` ("vanilla"), sends `ClientInformation`, spawns
  player, `ServerboundPlayerLoaded`.
- **Movement/physics**: `PhysicsPlugin` + `MovementPlugin` send position/rotation/input packets
  every tick, apply `player_position` teleports (accepts them), gravity, collisions.
- **Entities**: `EntityPlugin` tracks entity spawn/move/despawn into the ECS.
- **Respawn on death**: `AutoRespawnPlugin` (DefaultBotPlugins) sends the respawn packet on `DeathEvent`.
- **Resource packs**: `AcceptResourcePacksPlugin` auto-accepts (declines download, replies accepted flow).
- **Auto-reconnect**: `AutoReconnectPlugin`, default delay `DEFAULT_RECONNECT_DELAY = 5s`;
  configure/disable with `ClientBuilder::reconnect_after(Option<Duration>)`.
- **Tab list**, health/hunger/xp, inventory state, cookies (config+game `store_cookie`/`cookie_request`).

NOT handled — you must do it yourself:

- **Light**: not parsed, not stored. Consume `LevelChunkWithLight.light_data` / `LightUpdate`
  yourself (feature `packet-event`).
- **Transfer packets**: handler bodies are empty in both game and config states
  (`pub fn transfer(&mut self, _p: &ClientboundTransfer) {}`) — a server `transfer` is ignored;
  you'd have to catch it via `Event::Packet` and rejoin manually.
- Sounds/particles/bossbar/scoreboard/titles/maps: parsed into packets, no side effects — render yourself.
- Biome→color, block models, all rendering concerns.
- Chat *sending* is manual (`client.chat("...")` — sends command if it starts with `/`).

---

## 6. ClientBuilder mechanics for embedding in a GUI app

- `ClientBuilder::new() -> ClientBuilder<NoState, ()>`; `azalea::NoState` is
  `#[derive(Clone, Component, Default)] pub struct NoState;` — used when you never call
  `set_handler`/`set_state`. `set_handler::<S, Fut, R>` retypes the builder.
- Other builder methods: `new_without_plugins()` (then you MUST `.add_plugins(azalea::DefaultPlugins)`
  and `.add_plugins(azalea::bot::DefaultBotPlugins)`), `add_plugins(impl Plugins<M>)`,
  `set_state(S)`, `reconnect_after(impl Into<Option<Duration>>)`,
  `start(Account, impl ResolvableAddr) -> AppExit`,
  `start_with_opts(Account, addr, JoinOpts) -> AppExit`.
- **`start()` never returns** in normal operation (even after disconnect, the swarm lives for
  auto-reconnect) until something sends `AppExit` — i.e. `client.exit()`. It returns
  `bevy_app::AppExit`.
- **The `start()` future is `!Send`**: internally it builds a `tokio::task::LocalSet` and
  `spawn_local`s the ECS loop (Update at 60 Hz, GameTick at 20 Hz — see `run_schedule_loop` in
  `azalea-client/src/client.rs`). Therefore you **cannot `tokio::spawn(builder.start(...))`**.
  Run it either directly under `#[tokio::main]`/`Runtime::block_on`, or on a dedicated thread:

```rust
use std::sync::OnceLock;

use azalea::prelude::*;

static CLIENT: OnceLock<Client> = OnceLock::new();       // Client is Clone + Send + Sync

#[derive(Default, Clone, Component)]
pub struct State;

async fn handle(bot: Client, event: Event, _state: State) -> eyre::Result<()> {
    if let Event::Init = event {
        let _ = CLIENT.set(bot.clone());                 // handle escapes the handler here
    }
    Ok(())
}

pub fn spawn_azalea_thread() -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            ClientBuilder::new()
                .set_handler(handle)
                .start(Account::offline("dolphin"), "127.0.0.1:25565")
                .await;
        });
    })
}
// main thread: run winit/wgpu; poll CLIENT.get() (None until Event::Init) and read
// client.world(), client.component::<Position>() etc. from the render loop.
```

  Because the handler must be a plain `fn`, the two escape hatches for wiring azalea to your
  renderer are (a) a `static OnceLock<Client>`/global channel as above, or (b) put
  `Arc<...>`/`Sender<...>` fields inside your `State` struct and pass it via `.set_state(state)`
  (State is `Clone`d into every handler call).
- `Client.ecs` is `Arc<parking_lot::RwLock<bevy_ecs::world::World>>` in 0.16 (was `Mutex`).
  Locking it from the render thread contends with the 60 Hz schedule loop — keep reads short.
- Alternative without builder: `Client::join` (§2b) — but it needs a `LocalSet` and gives no
  auto-reconnect.
- `JoinOpts { server_proxy: Option<Proxy>, sessionserver_proxy: Option<Proxy>, custom_server_addr:
  Option<ServerAddr>, custom_socket_addr: Option<SocketAddr> }` (all optional; `..Default::default()`).

---

## Open questions

1. **Event::Packet vs world-state ordering**: `packet_listener` (sends `Event::Packet`) and
   `handle_receive_chunk_event` (parses the chunk into `World`) both run in the `Update` schedule
   with no explicit ordering between them; the async handler runs on a separate task anyway. In
   practice the chunk is parsed within the same Update frame, but there is no hard guarantee that
   `client.world()` already contains a chunk when your handler sees its `LevelChunkWithLight`
   packet. `Event::ReceiveChunk` has the same caveat (it's sent from the same frame). If exactness
   matters, parse `chunk_data.data` yourself or re-check `world.chunks.get(&pos).is_some()`.
2. **Light nibble array size**: azalea reads each `sky_updates`/`block_updates` entry as a
   length-prefixed byte array and does not validate the 2048-byte length; the 2048/nibble layout
   and mask-bit⇄section mapping described above is vanilla-protocol knowledge, not enforced by
   azalea types. (Vanilla always sends 2048.)
3. **`Death` payload**: `DeathEvent.packet` is `Option<ClientboundPlayerCombatKill>` but the enum
   variant wraps it into `Option<Arc<...>>` via `.map(|p| p.into())` — confirmed, just noting the
   Arc appears only at the `Event` layer.
4. Whether `Client::join`'s ECS keeps running if the `UnboundedReceiver<Event>` is dropped:
   the sender ignores errors (`let _ = ...send(...)`), so the ECS keeps running; not a leak
   concern but the client will linger until `client.exit()`.
5. `azalea-protocol` packet structs are `PartialEq + Clone + Debug` but not `Serialize`; if you
   need to persist packets, use `azalea_buf::AzBuf` read/write instead (each packet implements it).
