//! The complete language between the network bridge and the app.
//! Everything here is plain data — no azalea types leak past this boundary.

use crate::types::{BlockPos, ChunkPos, SectionData, SectionPos, StateId};

/// Bridge → app. Drained by the app every frame.
pub enum GameEvent {
    /// Login finished, player is in the world.
    Connected { username: String },
    /// Connection ended (kick, error, or requested disconnect).
    Disconnected { reason: String },
    /// A full chunk arrived; one event per non-empty section.
    Section { pos: SectionPos, data: SectionData },
    ChunkUnloaded { pos: ChunkPos },
    /// Single block change (also emitted for each block of multi-block updates).
    BlockChanged { pos: BlockPos, state: StateId },
    /// Local player state, once per tick (20/s).
    PlayerState(Box<PlayerSnapshot>),
    /// Full snapshot of visible remote entities, once per tick.
    Entities(Vec<EntitySnapshot>),
    /// Chat/system message, already flattened to plain text (ANSI stripped).
    Chat { text: String },
    /// Hotbar contents (slot 0-8) + selected slot, sent when it changes.
    Hotbar { slots: Box<[Option<ItemSnapshot>; 9]>, selected: u8 },
    /// World time for the daylight factor (ticks, 0..24000 cycle; negative = frozen).
    TimeOfDay { time_of_day: i64 },
}

#[derive(Clone, Debug)]
pub struct PlayerSnapshot {
    /// Feet position (vanilla convention).
    pub pos: [f64; 3],
    pub velocity: [f64; 3],
    /// Degrees, vanilla convention (yaw: 0 = south/+z, 90 = west/-x).
    pub yaw: f32,
    pub pitch: f32,
    pub eye_height: f32,
    pub on_ground: bool,
    pub health: f32,
    pub food: u32,
    pub xp_level: u32,
}

#[derive(Clone, Debug)]
pub struct EntitySnapshot {
    /// azalea ECS entity id (stable per entity while loaded).
    pub id: u64,
    /// Registry name, e.g. "player", "zombie", "item".
    pub kind: String,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// Display/profile name for players and named entities.
    pub name: Option<String>,
    pub is_player: bool,
}

#[derive(Clone, Debug)]
pub struct ItemSnapshot {
    /// Registry name, e.g. "diamond_sword".
    pub item: String,
    pub count: u32,
}

/// App → bridge. Applied on the next tick.
pub enum Command {
    /// Look direction in vanilla degrees (yaw, pitch).
    SetDirection { yaw: f32, pitch: f32 },
    /// forward/strafe ∈ {-1, 0, 1}; mapped to azalea WalkDirection (+ sprint).
    Move { forward: i8, strafe: i8, sprint: bool },
    Jump(bool),
    Sneak(bool),
    Chat(String),
    /// Fire-and-forget: azalea mines the block to completion (auto-swaps nothing).
    Mine(BlockPos),
    /// Right-click a block.
    Interact(BlockPos),
    /// Left-click an entity by bridge id (from EntitySnapshot::id).
    Attack(u64),
    SelectHotbar(u8),
    Disconnect,
}

/// How the bridge should authenticate.
#[derive(Clone, Debug)]
pub enum AccountConfig {
    /// Offline-mode username (dev/test servers).
    Offline(String),
    /// Microsoft account via azalea's cached MSA flow (email as cache key).
    Microsoft(String),
    /// A ready Minecraft session handed over by the launcher: it already ran
    /// the Microsoft/Xbox/Minecraft handshake, so we join online servers using
    /// this Minecraft access token directly (no second login).
    Session {
        username: String,
        uuid: String,
        access_token: String,
    },
}

#[derive(Clone, Debug)]
pub struct BridgeOptions {
    pub account: AccountConfig,
    /// "host" or "host:port".
    pub address: String,
}
