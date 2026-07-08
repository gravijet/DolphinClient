//! The complete language between the network bridge and the app.
//! Everything here is plain data — no azalea types leak past this boundary.

use crate::types::{BlockPos, ChunkPos, SectionData, SectionPos, StateId};

/// One styled run of chat text. A chat line is a `Vec<ChatSpan>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatSpan {
    pub text: String,
    /// RGB text color; `None` = default (white).
    pub color: Option<[u8; 3]>,
    pub bold: bool,
    pub italic: bool,
    pub underlined: bool,
    pub strikethrough: bool,
    pub obfuscated: bool,
    /// What clicking this span does (links, commands, …).
    pub click: Option<ChatClick>,
    /// Plain-text hover tooltip, if the span has one.
    pub hover: Option<String>,
}

impl ChatSpan {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), ..Self::default() }
    }
}

/// Flatten spans to plain text (for logs / dedupe / fallbacks).
pub fn spans_to_plain(spans: &[ChatSpan]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatClick {
    OpenUrl(String),
    RunCommand(String),
    SuggestCommand(String),
    CopyToClipboard(String),
}

/// One row of the player tab list.
#[derive(Clone, Debug)]
pub struct TabPlayer {
    pub uuid: String,
    /// Account name (sort key).
    pub name: String,
    /// Styled display name (falls back to the plain name).
    pub display: Vec<ChatSpan>,
    /// Latency in ms (drives the ping bars).
    pub latency: i32,
    /// Skin texture URL (from the profile's `textures` property).
    pub skin_url: Option<String>,
    /// True for the slim ("Alex") arm model.
    pub skin_slim: bool,
}

/// A container/inventory slot as plain data.
pub type Slots = Vec<Option<ItemSnapshot>>;

/// One villager/wandering-trader trade.
#[derive(Clone, Debug)]
pub struct TradeOffer {
    pub input_a: ItemSnapshot,
    pub input_b: Option<ItemSnapshot>,
    pub output: ItemSnapshot,
    pub disabled: bool,
}

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
    /// Chat/system message as styled spans (colors, formatting, click events).
    /// `system` = not a player chat message (command feedback etc.), for the
    /// "Commands Only" chat visibility.
    Chat { spans: Vec<ChatSpan>, system: bool },
    /// Hotbar contents (slot 0-8) + selected slot, sent when it changes.
    Hotbar { slots: Box<[Option<ItemSnapshot>; 9]>, selected: u8 },
    /// World time for the daylight factor (ticks, 0..24000 cycle; negative = frozen).
    TimeOfDay { time_of_day: i64 },
    /// A sound to play, from a server sound packet: event name
    /// (`entity.zombie.ambient`, namespace stripped), category, optional world
    /// position (`None` = non-positional), and volume/pitch/seed.
    Sound {
        name: String,
        category: crate::settings::SoundCategory,
        pos: Option<[f64; 3]>,
        volume: f32,
        pitch: f32,
        seed: u64,
    },
    /// Current tab list (sent when it changes, at most once per second).
    TabList(Vec<TabPlayer>),
    /// Tab-list header/footer lines set by the server.
    TabHeaderFooter { header: Vec<ChatSpan>, footer: Vec<ChatSpan> },
    /// Command tab-completion result (response to `Command::TabComplete`).
    TabSuggestions {
        /// Echoes the request id.
        id: u32,
        /// Byte range of the input the suggestions replace.
        start: usize,
        length: usize,
        entries: Vec<String>,
    },
    /// The server opened a container screen (chest, furnace, villager, …).
    ContainerOpened {
        id: i32,
        /// Vanilla menu kind, e.g. "generic_9x3", "crafting", "merchant".
        kind: String,
        title: Vec<ChatSpan>,
        slots: Slots,
    },
    /// Slot contents of the open container changed (includes the player rows).
    ContainerContent {
        id: i32,
        slots: Slots,
        /// Item on the cursor (picked up mid-click).
        carried: Option<ItemSnapshot>,
    },
    /// The open container was closed (by the server or as click feedback).
    ContainerClosed { id: i32 },
    /// Trades for the open merchant container.
    MerchantOffers { container_id: i32, offers: Vec<TradeOffer> },
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
    /// Progress toward the next level, 0.0..1.0 (drives the XP bar fill).
    pub xp_progress: f32,
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
    /// Hitbox size (vanilla dimensions; the box is centered on `pos` in x/z
    /// and extends up from `pos[1]`).
    pub width: f32,
    pub height: f32,
    /// Display/profile name for players and named entities.
    pub name: Option<String>,
    pub is_player: bool,
    /// Player UUID (players only) — used to look up the skin.
    pub uuid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSnapshot {
    /// Registry name, e.g. "diamond_sword".
    pub item: String,
    pub count: u32,
}

/// Mouse button used for a container click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotClickKind {
    /// Plain left click (pick up / place).
    Left,
    /// Plain right click (half / single).
    Right,
    /// Shift-left click (quick move).
    QuickMove,
    /// Q — throw one item.
    Throw,
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
    /// Ask the server for command completions of `text` (id echoes back).
    TabComplete { id: u32, text: String },
    /// Click slot `slot` of the open container `window_id`.
    ContainerClick { window_id: i32, slot: u16, kind: SlotClickKind },
    /// Close the open container (Esc/E in a container screen).
    CloseContainer { id: i32 },
    /// Select the trade at `index` in the open merchant screen.
    SelectTrade { index: u32 },
    /// Q — drop the held item (`all` = whole stack, Ctrl+Q).
    DropItem { all: bool },
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
