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
        /// The dimension's `ambient_light` (0 in the Overworld, 0.1 in the
        /// Nether): the floor of the light ramp, so nothing there is ever
        /// truly black.
        ambient_light: f32,
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
    /// Block entities decoded from a chunk, or a single one that changed.
    /// Each entry replaces whatever the app had at that position; positions the
    /// app knows about but that aren't listed are left alone (block changes and
    /// chunk unloads prune them instead).
    BlockEntities(Vec<BlockEntityInfo>),
    /// A block-entity animation trigger (`ClientboundBlockEvent`): chest and
    /// shulker "viewers" counts (`action` 1, `param` = viewers) and bell rings
    /// (`action` 1, `param` = the struck Direction). `block` is the block's
    /// registry name with the namespace stripped.
    BlockAction { pos: BlockPos, block: String, action: u8, param: u8 },
    /// A lightning bolt struck at `pos` — drawn for vanilla's half-second and
    /// then dropped (the strike is an entity the server never updates again).
    Lightning { pos: [f64; 3] },
    /// A boss bar appeared, changed or went away. The app keeps the set and
    /// draws them stacked at the top of the screen.
    BossBar(BossBarUpdate),
    /// An entity played its hurt animation (took damage) — flash it red.
    EntityHurt { id: u64 },
    /// An entity died (`EntityEvent` 3): play the vanilla death spin-and-fall
    /// before it despawns.
    EntityDeath { id: u64 },
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
    /// New contents for one filled map. The server sends a *patch*: a rectangle
    /// of colour indices, plus (sometimes) the full decoration list.
    MapData(Box<MapUpdate>),
    /// One property of the open container changed (`ClientboundContainerSetData`):
    /// furnace burn/cook time, brewing progress, the enchantment offers, the
    /// anvil's level cost, a beacon's effects. Meaning is per menu kind.
    ContainerData { id: i32, property: u16, value: u16 },
    /// The server's advancement tree changed (sent once on join, then on every
    /// criterion the player completes).
    Advancements(Box<AdvancementUpdate>),
    /// The player's server-side statistics, in reply to asking for them
    /// (vanilla asks whenever the Statistics screen opens).
    Statistics(Vec<StatEntry>),
    /// Somebody is mining a block: `stage` 0..=9 is how far the cracks have
    /// spread, `None` means they stopped. Keyed by the mining entity, exactly
    /// like vanilla (one player can only crack one block at a time).
    BlockDestruction { id: u64, pos: BlockPos, stage: Option<u8> },
    /// An explosion went off: TNT, a creeper, a bed in the Nether. The client
    /// owns the particle burst and the sound — the server sends neither.
    Explosion { pos: [f64; 3], radius: f32, sound: String },
    /// An item entity was picked up — vanilla flies it into the collector for
    /// a few ticks instead of making it vanish.
    ItemPickedUp { item: u64, collector: u64 },
    /// The local player died; `message` is the server's death message. Opens
    /// the "You died!" screen.
    Died { message: Vec<ChatSpan> },
    /// The server asked to open the written book held in this hand.
    OpenBook { off_hand: bool },
    /// A sign was just placed: vanilla opens its editor straight away.
    OpenSignEditor { pos: BlockPos, front: bool },
    /// The world border moved, resized or changed its warning distance.
    WorldBorder(WorldBorderUpdate),
    /// The camera now follows this entity (`/spectate`, or dying as a
    /// spectator). `None` = back to the player's own body.
    Camera { id: Option<u64> },
    /// The server's enchantment registry, indexed by protocol id. The
    /// enchanting table sends its three offers as ids into this.
    Enchantments(std::sync::Arc<Vec<String>>),
    /// New recipes were unlocked — vanilla pops a toast for them.
    RecipesUnlocked { count: u32 },
    /// The server's trim-pattern and trim-material registries, indexed by
    /// protocol id, so item tooltips can name a trim.
    TrimRegistries {
        patterns: std::sync::Arc<Vec<String>>,
        materials: std::sync::Arc<Vec<String>>,
    },
}

/// One filled map's new state, straight off `ClientboundMapItemData`.
#[derive(Clone, Debug, PartialEq)]
pub struct MapUpdate {
    /// Map id — the `map_id` component of every `filled_map` stack.
    pub id: u32,
    /// Zoom level 0..=4: one pixel covers `1 << scale` blocks.
    pub scale: u8,
    /// A locked map (copied in a cartography table) never updates again.
    pub locked: bool,
    /// The full decoration list, when the server sent one. `None` = unchanged.
    pub decorations: Option<Vec<MapDecoration>>,
    /// A rectangle of colour indices to blit into the 128×128 map. `None` when
    /// only the decorations moved.
    pub patch: Option<MapPatch>,
}

/// A rectangle of map colour indices (vanilla's `MapPatch`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapPatch {
    pub start_x: u8,
    pub start_y: u8,
    pub width: u8,
    pub height: u8,
    /// `width * height` colour indices, row-major. Index `>> 2` is the base
    /// colour, `& 3` the shade.
    pub colors: Vec<u8>,
}

/// One marker drawn on top of a map: the white player arrow, an item frame, a
/// coloured banner, a woodland mansion…
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapDecoration {
    /// Sprite name under `textures/map/decorations/`, e.g. "player",
    /// "red_banner", "woodland_mansion".
    pub sprite: &'static str,
    /// Position in map space, -128..=127 across the whole map.
    pub x: i8,
    pub y: i8,
    /// Rotation in sixteenths of a turn.
    pub rot: i8,
    /// A named banner shows its name under the marker.
    pub name: Option<String>,
}

/// The advancement tree as the server describes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdvancementUpdate {
    /// Throw away everything known so far before applying this.
    pub reset: bool,
    pub added: Vec<AdvancementNode>,
    /// Advancement ids that went away.
    pub removed: Vec<String>,
    /// `(advancement id, obtained criteria)`. A criterion counts as obtained
    /// once the server stamps it with a date.
    pub progress: Vec<(String, Vec<String>)>,
}

/// One advancement: where it sits in the tree and how it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct AdvancementNode {
    /// Full id including the namespace, e.g. `minecraft:story/mine_stone`.
    pub id: String,
    /// The advancement this one hangs off. Roots have none.
    pub parent: Option<String>,
    /// `None` for the invisible "glue" advancements servers use for logic.
    pub display: Option<AdvancementDisplay>,
    /// Criterion groups: every group must have at least one obtained criterion
    /// for the advancement to count as done (vanilla's AND of ORs).
    pub requirements: Vec<Vec<String>>,
}

/// How one advancement is drawn in the tree and in its toast.
#[derive(Clone, Debug, PartialEq)]
pub struct AdvancementDisplay {
    pub title: Vec<ChatSpan>,
    pub description: Vec<ChatSpan>,
    /// The item shown in the frame. `None` = the server sent an empty stack.
    pub icon: Option<ItemSnapshot>,
    /// 0 = task (square), 1 = challenge (spiky), 2 = goal (rounded).
    pub frame: u8,
    /// Show a toast when it is completed.
    pub show_toast: bool,
    /// Hidden until its parent is done.
    pub hidden: bool,
    /// Background texture of the tab this advancement roots, e.g.
    /// `minecraft:textures/gui/advancements/backgrounds/stone.png`.
    pub background: Option<String>,
    /// Position in the tree, in advancement cells (1 cell = 28 GUI px).
    pub x: f32,
    pub y: f32,
}

/// One server-side statistic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatEntry {
    /// Which family it belongs to: "custom", "mined", "crafted", "used",
    /// "broken", "picked_up", "dropped", "killed", "killed_by".
    pub category: &'static str,
    /// The block/item/entity/custom-stat name, namespace stripped.
    pub key: String,
    pub value: i32,
}

/// The world border, as the six border packets describe it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldBorderUpdate {
    pub center_x: f64,
    pub center_z: f64,
    /// Diameter in blocks the border is moving *from* and *to*, and how many
    /// milliseconds the move takes (0 = instant).
    pub old_size: f64,
    pub new_size: f64,
    pub lerp_time: u64,
    /// How close you have to get before the red warning tint appears.
    pub warning_blocks: u32,
    /// …or how many seconds away it is at your current speed.
    pub warning_time: u32,
}

impl Default for WorldBorderUpdate {
    fn default() -> Self {
        Self {
            center_x: 0.0,
            center_z: 0.0,
            old_size: 5.9999968e7,
            new_size: 5.9999968e7,
            lerp_time: 0,
            warning_blocks: 5,
            warning_time: 15,
        }
    }
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
    /// Cherry blossom petals drifting down from cherry leaves.
    Cherry,
    /// Ordinary falling leaves, and the pale-oak ones the creaking sheds.
    Leaf,
    PaleOak,
    /// The conduit's swirling nautilus shells.
    Nautilus,
    /// Sculk's blue soul wisps.
    SculkSoul,
    /// Soul sand / soul fire's pale face.
    Soul,
    /// A bright pinpoint: end rods, and the sparks off a firework.
    Spark,
    /// The firefly bush's little green lights.
    Firefly,
}

/// The vanilla poses that change how an entity is drawn. Anything we do not
/// draw differently (croaking, digging, …) collapses into `Standing`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EntityPose {
    #[default]
    Standing,
    Crouching,
    /// Flying on an elytra.
    FallFlying,
    /// Swimming, or crawling through a one-block gap.
    Swimming,
    /// Riptide trident spin.
    SpinAttack,
    Sleeping,
    /// Sitting: a tamed pet, or a mob riding something.
    Sitting,
}

/// A change to one boss bar, keyed by the server's bar uuid.
#[derive(Clone, Debug)]
pub enum BossBarUpdate {
    /// The bar appeared, or every field of it was replaced.
    Set { id: u128, bar: BossBar },
    /// The bar went away (boss died, player left the area).
    Remove { id: u128 },
    /// Health changed: 0.0..=1.0 of the bar filled.
    Progress { id: u128, progress: f32 },
    /// The styled title above the bar changed.
    Name { id: u128, name: Vec<ChatSpan> },
    /// The colour and/or the notch pattern changed.
    Style { id: u128, color: u8, overlay: u8 },
}

/// One boss bar as vanilla draws it.
#[derive(Clone, Debug)]
pub struct BossBar {
    /// Styled title, drawn centred above the bar.
    pub name: Vec<ChatSpan>,
    /// How full the bar is, 0.0..=1.0.
    pub progress: f32,
    /// 0 pink, 1 blue, 2 red, 3 green, 4 yellow, 5 purple, 6 white — the sprite
    /// name is derived from this.
    pub color: u8,
    /// 0 = plain, 1..=4 = notched into 6 / 10 / 12 / 20 segments.
    pub overlay: u8,
    /// The server asked for a darkened sky (the Wither and the dragon do).
    pub darken_screen: bool,
    /// The server asked for boss fog.
    pub world_fog: bool,
}

/// One biome's climate + colour data, as read from the server's biome registry.
/// The app turns this into grass/foliage/water tint colours (sampling the grass
/// and foliage colormaps for biomes with no explicit override).
#[derive(Clone, Debug)]
pub struct BiomeInfo {
    /// Registry name without the namespace, e.g. `plains` — the F3 line.
    pub name: String,
    pub temperature: f32,
    pub downfall: f32,
    /// `effects.grass_color` / `foliage_color` if the biome overrides them.
    pub grass_override: Option<[u8; 3]>,
    pub foliage_override: Option<[u8; 3]>,
    /// `effects.water_color` (default `0x3F76E4`).
    pub water: [u8; 3],
    /// `effects.grass_color_modifier`: 0 = none, 1 = dark_forest, 2 = swamp.
    pub grass_modifier: u8,
    /// `effects.fog_color` — the haze in the distance. This is what makes the
    /// crimson forest red and the warped forest teal.
    pub fog: [u8; 3],
    /// `effects.sky_color`, the flat colour above the horizon. Derived from
    /// temperature in the Overworld, explicit in the Nether/End.
    pub sky: [u8; 3],
    /// `effects.water_fog_color` — the colour of being underwater.
    pub water_fog: [u8; 3],
}

impl Default for BiomeInfo {
    fn default() -> Self {
        Self {
            name: "plains".to_string(),
            temperature: 0.5,
            downfall: 0.5,
            grass_override: None,
            foliage_override: None,
            water: [0x3F, 0x76, 0xE4],
            grass_modifier: 0,
            fog: [0xC0, 0xD8, 0xFF],
            sky: [0x78, 0xA7, 0xFF],
            water_fog: [0x05, 0x0D, 0x33],
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
    /// The entity's vanilla pose, as the metadata reports it. Drives the
    /// swimming/crawling, elytra, riptide-spin and sleeping poses.
    pub pose: EntityPose,
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
    /// Animal armour: horse armour, a llama's carpet, wolf armour.
    pub body: Option<String>,
    /// The saddle slot (pigs, striders, horses, camels).
    pub saddle: Option<String>,
    /// Armour trim per armour slot `[head, chest, legs, feet]`, as
    /// `(pattern, material)` names resolved from the server's trim registries.
    pub trims: [Option<(String, String)>; 4],
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
    /// The entity's vanilla pose — swimming/crawling, elytra flight, the
    /// riptide spin, sleeping.
    pub pose: EntityPose,
    /// Cape texture URL from the player's profile, if they have one.
    pub cape_url: Option<String>,
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
    /// Armor-stand appearance + pose (`kind == "armor_stand"`): size, whether
    /// arms/base plate show, and the six part rotations. `None` for everything else.
    pub armor_stand: Option<ArmorStandInfo>,
    /// The entity is burning (`OnFire` shared flag) — the app draws a flame
    /// billboard over it, like vanilla's on-fire effect.
    pub on_fire: bool,
    /// Dye-collar colour (0..15) for a *tamed* cat or wolf; `None` when untamed
    /// or not a pet. The app draws the collar as a tinted overlay on the model.
    pub collar: Option<i32>,
    /// Charged/"powered" creeper (`IsPowered`) — the app draws the blue
    /// energy-swirl overlay. `false` for everything else.
    pub powered: bool,
    /// Stack size of a dropped-item entity (`kind == "item"`), so vanilla's
    /// "bigger piles look bigger" rule can draw 2–5 stacked sprites.
    pub item_count: u32,
    /// The spawn packet's "object data", kept because azalea drops it: a
    /// falling block's block state id, a fishing bobber's / projectile's owner
    /// entity id. `0` when the entity type doesn't use it.
    pub spawn_data: i32,
    /// A sheared sheep (`SheepSheared`) — drawn without its wool layer.
    pub sheared: bool,
    /// A creeper with its fuse lit: it swells and flashes white before it goes.
    pub swelling: bool,
    /// A ghast or blaze winding up a shot.
    pub charging: bool,
    /// The entity this one is leashed to (`SetEntityLink`), if any. The app
    /// draws the lead as a hanging rope between the two.
    pub leashed_to: Option<u64>,
    /// Head yaw in vanilla degrees (`ClientboundRotateHead`). Vanilla turns the
    /// head up to 50° away from the body before the body follows; `None` means
    /// the server never sent one, so the head just follows the body.
    pub head_yaw: Option<f32>,
    /// The vehicle this entity rides and its seat index (`SetPassengers`). The
    /// app moves the rider onto the vehicle's seat and poses its legs.
    pub riding_on: Option<(u64, u8)>,
}

/// An armor stand's appearance and pose. The six rotations are Euler angles in
/// degrees (x,y,z), applied per part exactly like vanilla's armor-stand pose.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArmorStandInfo {
    /// Small (half-size) stand.
    pub small: bool,
    /// Draw the arms (a stand only shows arms when this is set).
    pub show_arms: bool,
    /// Draw the stone base plate.
    pub show_base: bool,
    /// Per-part pose rotations (degrees): head, body, left arm, right arm,
    /// left leg, right leg.
    pub head: [f32; 3],
    pub body: [f32; 3],
    pub left_arm: [f32; 3],
    pub right_arm: [f32; 3],
    pub left_leg: [f32; 3],
    pub right_leg: [f32; 3],
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
    /// The map id, when the frame holds a filled map — a framed map is drawn
    /// as the map itself, filling the frame.
    pub map_id: Option<u32>,
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

/// One side of a sign: four lines of styled text, a dye colour and the
/// glowing-ink flag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SignFace {
    /// The four lines, each a run of styled spans (empty = blank line).
    pub lines: [Vec<ChatSpan>; 4],
    /// Dye colour name applied to the whole side ("black" by default). Spans
    /// with their own colour win over it, exactly like vanilla.
    pub color: String,
    /// Glowing ink: the text is drawn full-bright with a dark outline.
    pub glowing: bool,
}

/// A block entity's renderable payload, decoded from the server's NBT. Only the
/// kinds the renderer actually draws are decoded; everything else is ignored.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockEntityData {
    /// Sign text (both sides). Applies to standing, wall and hanging signs.
    Sign { front: SignFace, back: SignFace },
    /// Banner pattern layers as `(pattern asset, dye colour 0..15)`, painted in
    /// order over the base colour (which comes from the block state, not NBT).
    Banner { layers: Vec<(String, u8)> },
    /// A player head's profile: the skin texture URL and the owner's name.
    /// Mob skulls carry no NBT and never produce this.
    Skull { texture_url: Option<String>, owner: Option<String> },
    /// A decorated pot's four faces, in vanilla's `sherds` order (back, left,
    /// right, front). Each entry is a pottery-pattern asset name, or `None` for
    /// a plain brick side.
    DecoratedPot { sherds: [Option<String>; 4] },
    /// Items cooking on a campfire, by slot (registry names, no namespace).
    Campfire { items: [Option<String>; 4] },
    /// A bell. Carries no data of its own — the marker is what matters, since
    /// only the block entity draws the gold bell body.
    Bell,
    /// A conduit. Also data-free: the shell's open/closed state is derived from
    /// the blocks around it, exactly like vanilla does client-side.
    Conduit,
}

/// A decoded block entity at a world position.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockEntityInfo {
    pub pos: BlockPos,
    pub data: BlockEntityData,
}

// `modifiers` carries an f64 amount, so this can only be `PartialEq`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemSnapshot {
    /// Registry name, e.g. "diamond_sword".
    pub item: String,
    pub count: u32,
    /// Server-set display name (custom_name, else item_name), styled spans.
    /// `None` → fall back to the default translated item name.
    pub name: Option<Vec<ChatSpan>>,
    /// Lore lines (styled), each a list of spans. Empty when the item has none.
    pub lore: Vec<Vec<ChatSpan>>,
    /// The stack shows the enchantment glint: it carries enchantments (or
    /// stored ones, like an enchanted book), unless `enchantment_glint_override`
    /// says otherwise.
    pub enchanted: bool,
    /// Durability: points of damage taken and the item's maximum. `max` is 0
    /// for items that don't wear out — the bar only shows when `0 < damage`.
    pub damage: u32,
    pub max_damage: u32,
    /// A filled map's id (its `map_id` component) — which of the world's maps
    /// this stack actually shows. `None` for everything else.
    pub map_id: Option<u32>,
    /// A written book's contents: title, author and one styled page per entry.
    /// `None` for everything that isn't a signed book.
    pub book: Option<BookContent>,
    /// Enchantments as `(registry protocol id, level)` — the id resolves to a
    /// name through the server's enchantment registry.
    pub enchantments: Vec<(u32, u32)>,
    /// Potion effects the stack applies: `(effect name, amplifier, duration in
    /// ticks)`. Empty unless it is a potion, a tipped arrow or suspicious stew.
    pub effects: Vec<(String, u32, i32)>,
    /// Attribute modifiers: `(attribute name, amount, operation)` where the
    /// operation is 0 add, 1 multiply base, 2 multiply total.
    pub modifiers: Vec<(String, f64, u8)>,
    /// The stack never wears out.
    pub unbreakable: bool,
    /// Leather armour's dye colour.
    pub dyed: Option<[u8; 3]>,
    /// An armour trim as `(pattern id, material id)` into the server's trim
    /// registries — the app resolves the names.
    pub trim: Option<(u32, u32)>,
}

/// A written book, as the reader screen needs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BookContent {
    pub title: String,
    pub author: String,
    /// One entry per page, each a run of styled text.
    pub pages: Vec<Vec<ChatSpan>>,
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
    /// Ask the server for the player's statistics (vanilla sends this every
    /// time the Statistics screen opens).
    RequestStats,
    /// Respawn after dying.
    Respawn,
    /// Press a button in the open container: an enchantment offer (0..2), a
    /// loom pattern, a stonecutter recipe.
    ContainerButton { window_id: i32, button: u8 },
    /// Type a new name into the open anvil.
    RenameItem { name: String },
    /// Finish editing a sign: its four lines as typed.
    SignUpdate { pos: BlockPos, front: bool, lines: [String; 4] },
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
