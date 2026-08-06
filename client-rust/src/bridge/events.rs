//! The complete language between the network bridge and the app.
//! Everything here is plain data — no azalea types leak past this boundary.

use crate::types::{BlockPos, ChunkPos, SectionData, SectionPos, StateId};

/// One styled run of chat text. A chat line is a `Vec<ChatSpan>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

/// One row of the sidebar scoreboard: styled text on the left, score on the
/// right (drawn in red like vanilla).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreLine {
    pub text: Vec<ChatSpan>,
    pub score: i32,
    /// The server hid this row's number (number_format = blank). Most minigame
    /// servers do this — the red integer on the right is noise, not content.
    pub hide_number: bool,
}

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
    /// The player respawned or changed dimension (death, Nether/End portal,
    /// server world switch). azalea already swapped its own world; the app
    /// must drop its world mirror and re-render from the fresh chunk stream.
    /// Also emitted once on login with the starting dimension.
    Respawn {
        /// Namespace-stripped dimension-type name ("overworld",
        /// "the_nether", "the_end", or a custom name).
        dimension: String,
        /// False in the Nether/End: no sky light — drives sky color and
        /// ambient brightness.
        has_skylight: bool,
        /// Nether-style dimension (red fog / dark red sky).
        ultrawarm: bool,
    },
    /// Connection ended (kick, error, or requested disconnect).
    Disconnected { reason: String },
    /// The server's biome registry, indexed by protocol id. Sent once after the
    /// registries arrive; the app turns each biome's climate + effect overrides
    /// into grass/foliage/water tint colours for the mesher.
    Biomes(std::sync::Arc<Vec<BiomeInfo>>),
    /// A full chunk arrived; one event per non-empty section.
    Section { pos: SectionPos, data: SectionData },
    ChunkUnloaded { pos: ChunkPos },
    /// Single block change (also emitted for each block of multi-block updates).
    BlockChanged { pos: BlockPos, state: StateId },
    /// LevelEvent 2001: a block broke nearby (another player or the server —
    /// our own breaks are excluded by the server and synthesized locally).
    /// `state` is the broken block's state id for sound + particle color.
    BlockBreakEffect { pos: BlockPos, state: StateId },
    /// Local player state, once per tick (20/s).
    PlayerState(Box<PlayerSnapshot>),
    /// Full snapshot of visible remote entities, once per tick.
    Entities(Vec<EntitySnapshot>),
    /// Chat/system message as styled spans (colors, formatting, click events).
    /// `system` = not a player chat message (command feedback etc.), for the
    /// "Commands Only" chat visibility.
    Chat { spans: Vec<ChatSpan>, system: bool },
    /// Hotbar contents (slot 0-8) + offhand + selected slot, sent when it changes.
    Hotbar {
        slots: Box<[Option<ItemSnapshot>; 9]>,
        offhand: Option<ItemSnapshot>,
        selected: u8,
    },
    /// World time for the daylight factor (ticks, 0..24000 cycle; negative = frozen).
    TimeOfDay { time_of_day: i64 },
    /// Weather state: rain and thunder strength (0..1), from the server's game
    /// events (start/stop raining + rain/thunder level changes).
    Weather { rain: f32, thunder: f32 },
    /// The local player gained/refreshed a potion effect (only ones with the
    /// icon flag set). `name` is the effect id (namespace stripped, e.g.
    /// `speed`), `amplifier` 0-based, `duration_ticks` (< 0 = infinite).
    EffectUpdate { name: String, amplifier: u32, duration_ticks: i32 },
    /// The local player lost a potion effect.
    EffectRemove { name: String },
    /// The server set (`duration_ticks > 0`) or cleared (`0`) a use-cooldown on
    /// an item type (`name` registry id, `minecraft:` stripped) — drives the
    /// shrinking white sweep over matching hotbar/off-hand slots.
    Cooldown { name: String, duration_ticks: u32 },
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
    /// A sound attached to an entity (`ClientboundSoundEntity`, e.g. hurt and
    /// attack sounds). The app resolves the entity's current position from its
    /// tracks (the bridge would need an extra ECS lock for it); an unknown id
    /// (usually the local player) plays at the listener.
    EntitySound {
        id: u64,
        name: String,
        category: crate::settings::SoundCategory,
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
    /// Sidebar scoreboard: title + rows (already sorted, highest score first,
    /// at most 15 rows). Empty `title` and `lines` = hide the sidebar.
    Scoreboard { title: Vec<ChatSpan>, lines: Vec<ScoreLine> },
    /// An entity played its hurt animation (took damage) — flash it red.
    EntityHurt { id: u64 },
    /// An entity swung its arm (attacked / mined) — play the swing animation.
    EntitySwing { id: u64 },
    /// A server resource pack finished downloading to `path` (a local .zip).
    /// The app overlays it and re-bakes so its textures actually apply.
    ResourcePackReady { path: std::path::PathBuf },
    /// A particle effect to spawn: `count` particles around `pos`, jittered
    /// within `±spread` and given a random velocity up to `speed` blocks/tick.
    Particles {
        pos: [f64; 3],
        /// Which particle texture to billboard (the app maps it to atlas UVs).
        tex: ParticleTex,
        /// RGB tint (multiplies the texture; used for coloured dust — white for
        /// most textured particles).
        color: [f32; 3],
        /// Sprite edge length in blocks.
        size: f32,
        count: u32,
        spread: [f32; 3],
        speed: f32,
        /// Downward acceleration (blocks/s²); 0 = floaty (smoke/heart).
        gravity: f32,
    },
}

/// A particle's billboard texture family. The app maps each to one or more
/// frames in the particle atlas; the bridge picks it from the server's particle
/// kind (see `particle_style`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ParticleTex {
    #[default]
    Generic,
    Flame,
    SoulFlame,
    Lava,
    Smoke,
    Crit,
    EnchantedHit,
    Damage,
    Heart,
    Angry,
    Happy,
    Effect,
    Note,
    Bubble,
    Splash,
    Drip,
    Explosion,
    Flash,
    Glow,
    Portal,
    Dust,
}

/// One biome's climate + colour data, as read from the server's biome registry.
/// The app turns this into grass/foliage/water tint colours (sampling the grass
/// and foliage colormaps for biomes with no explicit override).
#[derive(Clone, Copy, Debug)]
pub struct BiomeInfo {
    pub temperature: f32,
    pub downfall: f32,
    /// `effects.grass_color` / `foliage_color` if the biome overrides them.
    pub grass_override: Option<[u8; 3]>,
    pub foliage_override: Option<[u8; 3]>,
    /// `effects.water_color` (default `0x3F76E4`).
    pub water: [u8; 3],
    /// `effects.grass_color_modifier`: 0 = none, 1 = dark_forest, 2 = swamp.
    pub grass_modifier: u8,
}

impl Default for BiomeInfo {
    fn default() -> Self {
        Self {
            temperature: 0.5,
            downfall: 0.5,
            grass_override: None,
            foliage_override: None,
            water: [0x3F, 0x76, 0xE4],
            grass_modifier: 0,
        }
    }
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
    /// Absorption health (yellow "shield" hearts), in half-heart *points* (2 per
    /// heart). 0 when the player has no absorption. Drawn as gold hearts.
    pub absorption: f32,
    pub food: u32,
    pub xp_level: u32,
    /// Progress toward the next level, 0.0..1.0 (drives the XP bar fill).
    pub xp_progress: f32,
    /// Attack cooldown recharge, 0.0..1.0 (1.0 = fully charged). Drives the
    /// vanilla attack-strength indicator under the crosshair.
    pub attack_strength: f32,
    /// Air supply in ticks (max 300) — drives the bubble bar. Servers only
    /// send this once it changes; treat an unchanged 0 outside water as full.
    pub air: i32,
    /// Eyes are below the water surface (azalea FluidOnEyes).
    pub eyes_in_water: bool,
    /// Eyes are inside lava (azalea FluidOnEyes == Lava) — dense orange overlay.
    pub eyes_in_lava: bool,
    /// The player is actively using an item (eating, drinking, drawing a bow,
    /// blocking with a shield, spyglass) — drives the first-person use pose.
    pub using_item: bool,
    /// The player is burning (shared entity flag) — fire screen overlay.
    pub on_fire: bool,
    /// Powder-snow freeze progress, 0.0 (warm) .. 1.0 (fully frozen). Drives the
    /// frost screen vignette and the cyan frozen hearts.
    pub freeze: f32,
    /// Swim pose active (sprint-swimming).
    pub swimming: bool,
    /// Mounted on a vehicle (boat, horse, minecart): movement keys steer the
    /// vehicle instead of walking; no auto-jump.
    pub riding: bool,
    /// Block currently being mined + progress 0.0..1.0 (crack overlay,
    /// mining sounds). `None` while not mining.
    pub mining: Option<(BlockPos, f32)>,
    /// The local player's own worn armor (from the inventory armor slots), so
    /// the third-person model shows it. Hands come from the hotbar.
    pub equipment: Equipment,
}

/// A remote entity's visible equipment (registry names, `minecraft:` stripped).
/// Populated from `ClientboundSetEquipment`; empty when the server never sent it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Equipment {
    pub head: Option<String>,
    pub chest: Option<String>,
    pub legs: Option<String>,
    pub feet: Option<String>,
    pub main_hand: Option<String>,
    pub off_hand: Option<String>,
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
    /// Plain display/profile name for players and named entities (logic/dedup).
    pub name: Option<String>,
    /// Styled name to draw over the entity: team prefix/color/suffix for
    /// players, the custom_name component for named entities. `None` = no tag.
    /// Never contains raw `§` codes — the color/formatting is on the spans.
    pub name_spans: Option<Vec<ChatSpan>>,
    pub is_player: bool,
    /// Crouching (Pose::Crouching / shift held): drives the sneak pose.
    pub sneaking: bool,
    /// Sprinting flag (metadata) — a wider limb swing when running.
    pub sprinting: bool,
    /// Invisibility potion / invisible flag — hide the model (armor still shows).
    pub invisible: bool,
    /// Baby animal/monster (`AbstractAgeableBaby`) — drawn about half size.
    pub baby: bool,
    /// Player UUID (players only) — used to look up the skin.
    pub uuid: Option<String>,
    /// Skin texture URL decoded from the entity's own profile (server NPCs
    /// aren't in the tab list, so this is how they get a real skin). `slim` =
    /// Alex model.
    pub skin_url: Option<String>,
    pub skin_slim: bool,
    /// Armor + hand items last seen for this entity (from SetEquipment).
    pub equipment: Equipment,
    /// For dropped-item entities (`kind == "item"`): the item's registry name,
    /// so it can be drawn with its real icon instead of a box.
    pub item: Option<String>,
    /// Species variant index for mobs that come in colours/types (rabbit, fox,
    /// parrot, llama, axolotl, horse, mooshroom, shulker colour…). Meaning is
    /// per-species; the app maps `(kind, variant)` → the real variant texture.
    /// `0` = the default/first variant.
    pub variant: i32,
    /// Registry-driven variant *name* for species whose variant is a data
    /// registry (cat/wolf/cow/chicken/pig/frog) — e.g. "tabby", "ashen",
    /// "warm". Resolved from the server registry; the app maps `(kind, name)` →
    /// texture. `None` for everything else.
    pub variant_name: Option<String>,
    /// Painting appearance (`kind == "painting"`): the art asset name, size in
    /// blocks and the wall direction it faces. `None` for everything else.
    pub painting: Option<PaintingInfo>,
    /// Item-frame contents (`kind == "item_frame"`/`"glow_item_frame"`): the
    /// held item, its rotation and the wall direction. `None` for everything else.
    pub frame: Option<FrameInfo>,
    /// Display-entity transform + payload (`kind == "block_display"`/
    /// `"item_display"`). `text_display` text rides on `name_spans` instead.
    /// `None` for everything else.
    pub display: Option<DisplayInfo>,
}

/// A block/item display entity's transform and payload. The transform matches
/// vanilla: `translation`, then `left_rot`, `scale`, `right_rot` (quaternions
/// xyzw). One of `block`/`item` is set depending on the kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayInfo {
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub left_rot: [f32; 4],
    pub right_rot: [f32; 4],
    /// Block-display block state id (the global vanilla state id, usable
    /// directly with the client's model store).
    pub block_state: Option<u32>,
    /// Item-display item registry name (no namespace).
    pub item: Option<String>,
}

/// An item frame's state: what it holds, how the item is rotated, which way the
/// frame hangs, and whether it is the glowing variant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameInfo {
    /// Held item's registry name (no namespace), e.g. "diamond". `None` = empty.
    pub item: Option<String>,
    /// Rotation step 0..7 (×45°).
    pub rot: u8,
    /// Vanilla Direction index the frame faces (0 Down, 1 Up, 2 N, 3 S, 4 W, 5 E).
    pub facing: u8,
    /// Glowing item frame (brighter frame texture).
    pub glow: bool,
}

/// A painting's appearance, resolved from the server's `painting_variant`
/// registry: which art to show and how big / which way it hangs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaintingInfo {
    /// Art asset name without namespace, e.g. "kebab" → `textures/painting/kebab.png`.
    pub asset: String,
    /// Painting size in blocks.
    pub width: i32,
    pub height: i32,
    /// Vanilla Direction index the art faces (2 N, 3 S, 4 W, 5 E).
    pub facing: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemSnapshot {
    /// Registry name, e.g. "diamond_sword".
    pub item: String,
    pub count: u32,
    /// Server-set display name (custom_name, else item_name), styled spans.
    /// `None` → fall back to the default translated item name.
    pub name: Option<Vec<ChatSpan>>,
    /// Lore lines (styled), each a list of spans. Empty when the item has none.
    pub lore: Vec<Vec<ChatSpan>>,
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
    /// Kept for the live interaction tests / scripted mining; the app drives
    /// normal hold-to-mine through [`Command::SetMining`].
    #[allow(dead_code)]
    Mine(BlockPos),
    /// Hold-to-mine toggle: while enabled, azalea continuously mines whatever
    /// block is under its own (authoritative) crosshair, exactly like vanilla
    /// left-click-hold. This is the reliable survival mining path — it handles
    /// progress accumulation, target changes and instant-mine internally.
    SetMining(bool),
    /// Force a right-click on a specific block (bypasses the crosshair check).
    /// The app uses [`Command::UseItem`] for normal right-clicks; this variant
    /// is kept for the live interaction tests and scripted placement.
    #[allow(dead_code)]
    Interact(BlockPos),
    /// Right-click "use": place/use the block or use the held item (bow,
    /// crossbow, ender pearl, eat food) based on azalea's crosshair hit result.
    /// Entity interaction is intentionally skipped in the bridge.
    UseItem,
    /// Release a charged item (bow/crossbow/trident/spyglass) — fires the arrow.
    /// Sent when the right button is released after a `UseItem`.
    ReleaseUseItem,
    /// F — swap the main-hand and off-hand items.
    SwapOffhand,
    /// Left-click an entity by bridge id (from EntitySnapshot::id).
    Attack(u64),
    /// Right-click (interact with) an entity by bridge id — trade with a
    /// villager, mount a boat/horse, name-tag a mob, etc. Uses azalea's own
    /// `entity_interact`, which emits the modern 26.1 `ServerboundInteract`.
    InteractEntity(u64),
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
    /// Render distance in chunks — sent to the server as the client view
    /// distance so azalea's chunk storage covers what the app can draw
    /// (azalea's default of 8 silently dropped farther chunks).
    pub view_distance: u8,
}
