//! Windowed app: winit event loop, input → Commands, per-frame pipeline:
//! drain GameEvents → WorldMirror → schedule meshing (rayon, nearest-first,
//! ≤8/frame) → upload finished meshes → render → egui HUD.
//!
//! Input goes through the rebindable `settings.keys` map (WASD defaults).
//! Mouse capture is centralized: every frame the app computes whether the
//! game *should* own the pointer (connected, no overlay, window focused) and
//! applies it — closing chat/menus/containers re-grabs automatically.
//!
//! Smoothness: the local player position is extrapolated from the last two
//! 20 Hz snapshots and exponentially smoothed; remote entities render ~100 ms
//! in the past, interpolated between their per-tick snapshots.

pub mod blocksound;
pub mod chat;
pub mod container;
pub mod hud;
pub mod mcui;
pub mod offscreen;
pub mod serverlist;
pub mod skins;
pub mod tablist;

use crate::assets::atlas::Atlas;
use crate::assets::blockmap::BlockTable;
use crate::assets::items::ItemIcons;
use crate::assets::{AssetPack, Lang};
use crate::audio::AudioEngine;
use crate::bridge::events::{
    AccountConfig, BridgeOptions, ChatSpan, Command, EntitySnapshot, GameEvent, ItemSnapshot,
    PlayerSnapshot, ScoreLine,
};
use crate::bridge::{GameHandle, spawn_bridge};
use crate::models::BakedModelStore;
use crate::render::{
    ArmorMaterial, EguiFrame, EntityDraw, EntityDrawKind, MobModel, RenderTarget, Renderer,
    SceneParams, camera,
};
use crate::settings::{GameSettings, KeyBinds, key_id};
use crate::types::{BlockPos, ChunkPos, MeshData, SectionPos, StateId};
use crate::world::WorldMirror;
use crate::world::mesher::mesh_section;
use anyhow::{Context as _, Result};
use crossbeam_channel::{Receiver, Sender};
use hud::{Hud, HudAction, HudState, NameTag};
use skins::{SkinManager, fnv64, key_of_url, normalize_skin};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

#[derive(Clone, Debug)]
pub struct AppOptions {
    pub bridge: BridgeOptions,
    /// Vanilla 26.1 client jar (assets source).
    pub mc_jar: PathBuf,
    /// Optional external blocks.json report (plain or .gz). `None` = use the
    /// 26.1 report embedded in the binary (the normal case).
    pub blocks_report: Option<PathBuf>,
    /// Chunks (Chebyshev radius) to keep/mesh around the player.
    pub render_distance: i32,
    /// `.minecraft/assets` dir enabling sound, unifont, panorama, skins cache.
    pub assets_dir: Option<PathBuf>,
    /// Asset-index id paired with `assets_dir`.
    pub asset_index: Option<String>,
}

const MESH_BUDGET_PER_FRAME: usize = 8;
const FPS_WINDOW: usize = 60;
/// How many connect attempts to make before surfacing an error. Cold DNS
/// resolvers / SRV flakiness and servers that drop the very first login
/// handshake make the initial tries after launch fail; several silent retries
/// make that invisible to the user (the common "took a few tries to join").
const MAX_CONNECT_ATTEMPTS: u32 = 8;

/// Whether a connect-phase failure is a transient network/DNS hiccup worth
/// retrying automatically — as opposed to a ban, whitelist, version mismatch,
/// or expired session, which should surface to the user immediately.
fn is_transient_connect_error(reason: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "DNS",
        "Timeout",
        "Zeitüberschreitung",
        "zu lange",
        "antwortet nicht",
        "nicht erreichbar",
        "nicht gefunden",
        "connection failed",
        "connection closed",
        "unerwartet beendet",
        "reset",
        "refused",
        "os error",
        // Server dropped the handshake / half-open login — common on the first
        // try after launch and virtually always fixed by a quick reconnect.
        "eof",
        "EOF",
        "broken pipe",
        "closed",
        "aborted",
        "unexpected end",
        "handshake",
        "read error",
        "write error",
        "verbindung",
    ];
    let lower = reason.to_lowercase();
    NEEDLES.iter().any(|n| reason.contains(n) || lower.contains(&n.to_lowercase()))
}
/// Remote entities render this far in the past, interpolated between their
/// per-tick snapshots (2 ticks — smooth even when one snapshot arrives late).
const ENTITY_LERP_DELAY: Duration = Duration::from_millis(100);

/// In-game, the bridge streams events continuously (a player snapshot every
/// tick, 20×/s). If it emits nothing at all for this long the connection is
/// dead — a silent server timeout or a frozen azalea schedule loop (the case
/// the user hit: "you time out and nothing happens; even Disconnect does
/// nothing"). The watchdog then leaves the server locally, without waiting on
/// the (possibly frozen) bridge thread to confirm. Sits above the bridge's own
/// 30s silence timeout so the bridge's cleaner "server not responding" message
/// wins whenever it can still emit.
const CONNECTION_WATCHDOG: Duration = Duration::from_secs(45);

/// The dolphin-with-controller logo as the window/taskbar icon: visibly
/// distinct from the plain launcher dolphin. Falls back to the launcher logo.
fn load_window_icon() -> Option<winit::window::Icon> {
    let bytes: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../assets/brand/dolphin-client-64.png"
    ));
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = (img.width(), img.height());
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// Load the six title-panorama faces from the asset-object store (the jar only
/// ships 1×1 stubs).
pub(crate) fn load_panorama(
    assets_dir: Option<&Path>,
    index_id: Option<&str>,
) -> Option<[image::RgbaImage; 6]> {
    let (dir, id) = (assets_dir?, index_id?);
    let raw = std::fs::read(dir.join("indexes").join(format!("{id}.json"))).ok()?;
    let index: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let objects = index.get("objects")?;
    let mut faces = Vec::with_capacity(6);
    for i in 0..6 {
        let key = format!("minecraft/textures/gui/title/background/panorama_{i}.png");
        let hash = objects.get(key.as_str())?.get("hash")?.as_str()?;
        let path = dir.join("objects").join(&hash[0..2]).join(hash);
        // Asset-store objects are hash-named (no extension), so decode from the
        // bytes — `image::open` would guess the format from the extension.
        let bytes = std::fs::read(path).ok()?;
        faces.push(image::load_from_memory(&bytes).ok()?.to_rgba8());
    }
    faces.try_into().ok()
}

/// Blocks until the window closes or the connection drops fatally.
pub fn run_windowed(opts: AppOptions) -> Result<()> {
    // Bake assets before opening the window (slow, one-off).
    let t0 = Instant::now();
    info!(jar = %opts.mc_jar.display(), "app: opening asset pack");
    let mut pack = AssetPack::open(&opts.mc_jar)?;
    // Client-side resource packs: any .zip in `<config>/resourcepacks/` overlays
    // the vanilla jar (later name wins), applied before anything is baked.
    let rp_dir = GameSettings::config_dir().join("resourcepacks");
    let applied_packs = crate::assets::load_resource_packs(&mut pack, &rp_dir);
    if !applied_packs.is_empty() {
        info!(packs = ?applied_packs, "app: applied client resource packs");
    }
    let table = BlockTable::load_or_embedded(opts.blocks_report.as_deref())
        .context("loading block table")?;
    info!(states = table.len(), elapsed_ms = t0.elapsed().as_millis() as u64, "app: block table loaded");
    let t1 = Instant::now();
    let (store, atlas) = BakedModelStore::bake_all(&mut pack, &table).context("baking models")?;
    info!(elapsed_ms = t1.elapsed().as_millis() as u64, "app: models baked");
    let t2 = Instant::now();
    let item_icons = Arc::new(ItemIcons::bake(&mut pack, &table, &store, &atlas));
    info!(
        icons = item_icons.len(),
        elapsed_ms = t2.elapsed().as_millis() as u64,
        "app: item icons baked"
    );

    // Vanilla GUI assets (bitmap font + unifont fallback, widget sprites) for
    // the Minecraft-look menus.
    let egui_ctx = egui::Context::default();
    let mcui = Arc::new(
        mcui::McUi::load(
            &mut pack,
            &egui_ctx,
            opts.assets_dir.as_deref(),
            opts.asset_index.as_deref(),
        )
        .context("loading vanilla GUI assets")?,
    );

    let mut settings = GameSettings::load_or_seed(opts.render_distance);
    settings.clamp();
    let lang_code = settings.language.clone();
    let lang = Lang::load(
        &mut pack,
        opts.assets_dir.as_deref(),
        opts.asset_index.as_deref(),
        &lang_code,
    );

    // Default Steve skin (renderer key 0) + title panorama, both best-effort.
    let steve = pack
        .texture_png("entity/player/wide/steve")
        .ok()
        .map(normalize_skin);
    if steve.is_none() {
        warn!("app: no Steve skin in the jar — unskinned players render as boxes");
    }

    // Armor textures for rendering other players' equipment (best-effort per
    // material and layer; a missing file just means that piece isn't drawn).
    let mut armor_textures: Vec<(ArmorMaterial, bool, image::RgbaImage)> = Vec::new();
    for mat in ArmorMaterial::all() {
        let name = mat.tex_name();
        if let Ok(img) = pack.texture_png(&format!("entity/equipment/humanoid/{name}")) {
            armor_textures.push((mat, false, img));
        }
        if let Ok(img) = pack.texture_png(&format!("entity/equipment/humanoid_leggings/{name}")) {
            armor_textures.push((mat, true, img));
        }
    }
    info!(count = armor_textures.len(), "app: armor textures loaded");

    // Mining crack frames (block/destroy_stage_0..9) for the break animation.
    let mut crack_textures: Vec<image::RgbaImage> = Vec::new();
    for i in 0..10 {
        match pack.texture_png(&format!("block/destroy_stage_{i}")) {
            Ok(img) => crack_textures.push(img),
            Err(_) => break, // keep the stages contiguous
        }
    }
    if crack_textures.len() < 10 {
        warn!(found = crack_textures.len(), "app: incomplete destroy_stage textures");
    }

    // Humanoid mobs share the player skin layout (64×64), so we render them with
    // the player model using their real entity texture — a real texture instead
    // of a yellow box, at near-zero extra cost. (registry kind, jar texture path)
    const HUMANOID_MOBS: &[(&str, &str)] = &[
        ("zombie", "entity/zombie/zombie"),
        ("husk", "entity/zombie/husk"),
        ("drowned", "entity/zombie/drowned"),
        ("giant", "entity/zombie/zombie"),
        ("skeleton", "entity/skeleton/skeleton"),
        ("stray", "entity/skeleton/stray"),
        ("wither_skeleton", "entity/skeleton/wither_skeleton"),
        ("bogged", "entity/skeleton/bogged"),
        ("zombified_piglin", "entity/piglin/zombified_piglin"),
        ("piglin", "entity/piglin/piglin"),
        ("piglin_brute", "entity/piglin/piglin_brute"),
        ("zombie_villager", "entity/zombie_villager/zombie_villager"),
    ];
    let mut mob_skin_key: HashMap<String, u64> = HashMap::new();
    let mut mob_textures: Vec<(u64, image::RgbaImage)> = Vec::new();
    for (kind, path) in HUMANOID_MOBS {
        if let Ok(img) = pack.texture_png(path) {
            let key = fnv64(format!("mob:{kind}").as_bytes());
            mob_skin_key.insert((*kind).to_string(), key);
            mob_textures.push((key, normalize_skin(img)));
        }
    }
    info!(count = mob_textures.len(), "app: humanoid mob textures loaded");

    // Non-humanoid mobs rendered with their own prebuilt cuboid model + real
    // texture (registry kind, jar texture path, model). Mesh geometry lives in
    // the renderer; here we just load and key the texture. Mob textures are NOT
    // normalized (they aren't 64×64 skins) — uploaded at their native size.
    const MODEL_MOBS: &[(&str, &str, MobModel)] = &[
        ("creeper", "entity/creeper/creeper", MobModel::Creeper),
        // Slime + magma cube: one cube model, per-mob texture; the app scales it
        // by the entity's size (size 1/2/4 → 0.5/1/2 blocks).
        ("slime", "entity/slime/slime", MobModel::Slime),
        ("magma_cube", "entity/slime/magmacube", MobModel::Slime),
        ("pig", "entity/pig/pig_temperate", MobModel::Pig),
        ("sheep", "entity/sheep/sheep", MobModel::Sheep),
        ("chicken", "entity/chicken/chicken_temperate", MobModel::Chicken),
        ("cow", "entity/cow/cow_temperate", MobModel::Cow),
        ("mooshroom", "entity/cow/mooshroom_red", MobModel::Cow),
        // Boats: one hull model, per-wood texture (the raft shares the hull —
        // approximate, but far better than a box). Chest variants use the
        // chest_boat texture whose hull region matches.
        ("oak_boat", "entity/boat/oak", MobModel::Boat),
        ("spruce_boat", "entity/boat/spruce", MobModel::Boat),
        ("birch_boat", "entity/boat/birch", MobModel::Boat),
        ("jungle_boat", "entity/boat/jungle", MobModel::Boat),
        ("acacia_boat", "entity/boat/acacia", MobModel::Boat),
        ("dark_oak_boat", "entity/boat/dark_oak", MobModel::Boat),
        ("mangrove_boat", "entity/boat/mangrove", MobModel::Boat),
        ("cherry_boat", "entity/boat/cherry", MobModel::Boat),
        ("pale_oak_boat", "entity/boat/pale_oak", MobModel::Boat),
        ("bamboo_raft", "entity/boat/bamboo", MobModel::Boat),
        ("oak_chest_boat", "entity/chest_boat/oak", MobModel::Boat),
        ("spruce_chest_boat", "entity/chest_boat/spruce", MobModel::Boat),
        ("birch_chest_boat", "entity/chest_boat/birch", MobModel::Boat),
        ("jungle_chest_boat", "entity/chest_boat/jungle", MobModel::Boat),
        ("acacia_chest_boat", "entity/chest_boat/acacia", MobModel::Boat),
        ("dark_oak_chest_boat", "entity/chest_boat/dark_oak", MobModel::Boat),
        ("mangrove_chest_boat", "entity/chest_boat/mangrove", MobModel::Boat),
        ("cherry_chest_boat", "entity/chest_boat/cherry", MobModel::Boat),
        ("pale_oak_chest_boat", "entity/chest_boat/pale_oak", MobModel::Boat),
        ("bamboo_chest_raft", "entity/chest_boat/bamboo", MobModel::Boat),
        // Extended roster: real cuboid models for the common overworld mobs that
        // used to fall back to a flat coloured box.
        ("spider", "entity/spider/spider", MobModel::Spider),
        ("cave_spider", "entity/spider/cave_spider", MobModel::Spider),
        ("wolf", "entity/wolf/wolf", MobModel::Wolf),
        ("fox", "entity/fox/fox", MobModel::Fox),
        ("villager", "entity/villager/villager", MobModel::Villager),
        ("wandering_trader", "entity/wandering_trader/wandering_trader", MobModel::Villager),
        ("enderman", "entity/enderman/enderman", MobModel::Enderman),
        ("iron_golem", "entity/iron_golem/iron_golem", MobModel::IronGolem),
        ("squid", "entity/squid/squid", MobModel::Squid),
        ("glow_squid", "entity/squid/glow_squid", MobModel::Squid),
        ("bat", "entity/bat/bat", MobModel::Bat),
        ("rabbit", "entity/rabbit/rabbit_brown", MobModel::Rabbit),
        ("horse", "entity/horse/horse_brown", MobModel::Horse),
        ("donkey", "entity/horse/donkey", MobModel::Horse),
        ("mule", "entity/horse/mule", MobModel::Horse),
        ("skeleton_horse", "entity/horse/horse_skeleton", MobModel::Horse),
        ("zombie_horse", "entity/horse/horse_zombie", MobModel::Horse),
        ("cat", "entity/cat/cat_tabby", MobModel::Cat),
        ("ocelot", "entity/cat/ocelot", MobModel::Cat),
        ("snow_golem", "entity/snow_golem/snow_golem", MobModel::SnowGolem),
        ("turtle", "entity/turtle/turtle", MobModel::Turtle),
        ("goat", "entity/goat/goat", MobModel::Goat),
        ("panda", "entity/panda/panda", MobModel::Panda),
        ("polar_bear", "entity/bear/polarbear", MobModel::PolarBear),
        ("llama", "entity/llama/llama_creamy", MobModel::Llama),
        ("trader_llama", "entity/llama/llama_creamy", MobModel::Llama),
        ("ghast", "entity/ghast/ghast", MobModel::Ghast),
        ("happy_ghast", "entity/ghast/happy_ghast", MobModel::Ghast),
        ("blaze", "entity/blaze/blaze", MobModel::Blaze),
        ("dolphin", "entity/dolphin/dolphin", MobModel::Dolphin),
        ("guardian", "entity/guardian/guardian", MobModel::Guardian),
        ("elder_guardian", "entity/guardian/guardian_elder", MobModel::Guardian),
        ("cod", "entity/fish/cod", MobModel::Cod),
        ("salmon", "entity/fish/salmon", MobModel::Salmon),
        ("bee", "entity/bee/bee", MobModel::Bee),
        ("silverfish", "entity/silverfish/silverfish", MobModel::Silverfish),
        ("parrot", "entity/parrot/parrot_red_blue", MobModel::Parrot),
        ("phantom", "entity/phantom/phantom", MobModel::Phantom),
        // 0.38.0 bestiary expansion.
        ("axolotl", "entity/axolotl/axolotl_lucy", MobModel::Axolotl),
        ("frog", "entity/frog/frog_temperate", MobModel::Frog),
        ("tadpole", "entity/tadpole/tadpole", MobModel::Tadpole),
        ("camel", "entity/camel/camel", MobModel::Camel),
        ("sniffer", "entity/sniffer/sniffer", MobModel::Sniffer),
        ("armadillo", "entity/armadillo/armadillo", MobModel::Armadillo),
        ("allay", "entity/allay/allay", MobModel::Allay),
        ("vex", "entity/illager/vex", MobModel::Vex),
        ("endermite", "entity/endermite/endermite", MobModel::Endermite),
        ("pufferfish", "entity/fish/pufferfish", MobModel::Pufferfish),
        ("pillager", "entity/illager/pillager", MobModel::Illager),
        ("vindicator", "entity/illager/vindicator", MobModel::Illager),
        ("evoker", "entity/illager/evoker", MobModel::Illager),
        ("illusioner", "entity/illager/illusioner", MobModel::Illager),
        ("witch", "entity/witch/witch", MobModel::Witch),
        ("strider", "entity/strider/strider", MobModel::Strider),
        ("hoglin", "entity/hoglin/hoglin", MobModel::Hoglin),
        ("zoglin", "entity/hoglin/zoglin", MobModel::Hoglin),
        ("ravager", "entity/illager/ravager", MobModel::Ravager),
        ("warden", "entity/warden/warden", MobModel::Warden),
        ("creaking", "entity/creaking/creaking", MobModel::Creaking),
        ("breeze", "entity/breeze/breeze", MobModel::Breeze),
        // 0.39.0 — the last entities: bosses + specials.
        ("ender_dragon", "entity/enderdragon/dragon", MobModel::EnderDragon),
        ("wither", "entity/wither/wither", MobModel::Wither),
        ("shulker", "entity/shulker/shulker", MobModel::Shulker),
        ("armor_stand", "entity/armorstand/armorstand", MobModel::ArmorStand),
        ("end_crystal", "entity/end_crystal/end_crystal", MobModel::EndCrystal),
    ];
    let mut mob_model: HashMap<String, (u64, MobModel)> = HashMap::new();
    for (kind, path, model) in MODEL_MOBS {
        if let Ok(img) = pack.texture_png(path) {
            let key = fnv64(format!("mobmodel:{kind}").as_bytes());
            mob_model.insert((*kind).to_string(), (key, *model));
            mob_textures.push((key, img));
        } else {
            warn!(kind, path, "app: mob texture missing — will fall back to a box");
        }
    }

    // Per-species colour/type variants: the server sends a variant index in the
    // entity metadata (see the bridge query); pick the real texture for
    // `(kind, index)`. Missing → the mob keeps its default MODEL_MOBS texture.
    // Index meanings follow vanilla's enum order for each species.
    const VARIANT_MOBS: &[(&str, i32, &str)] = &[
        // Rabbit (RabbitKind): 0 brown,1 white,2 black,3 white_splotched,4 gold,5 salt,99 killer.
        ("rabbit", 0, "entity/rabbit/rabbit_brown"),
        ("rabbit", 1, "entity/rabbit/rabbit_white"),
        ("rabbit", 2, "entity/rabbit/rabbit_black"),
        ("rabbit", 3, "entity/rabbit/rabbit_white_splotched"),
        ("rabbit", 4, "entity/rabbit/rabbit_gold"),
        ("rabbit", 5, "entity/rabbit/rabbit_salt"),
        ("rabbit", 99, "entity/rabbit/rabbit_caerbannog"),
        // Fox (FoxKind): 0 red, 1 snow.
        ("fox", 0, "entity/fox/fox"),
        ("fox", 1, "entity/fox/fox_snow"),
        // Parrot (ParrotVariant): 0 red/blue,1 blue,2 green,3 yellow/blue,4 grey.
        ("parrot", 0, "entity/parrot/parrot_red_blue"),
        ("parrot", 1, "entity/parrot/parrot_blue"),
        ("parrot", 2, "entity/parrot/parrot_green"),
        ("parrot", 3, "entity/parrot/parrot_yellow_blue"),
        ("parrot", 4, "entity/parrot/parrot_grey"),
        // Llama + trader llama (LlamaVariant): 0 creamy,1 white,2 brown,3 gray.
        ("llama", 0, "entity/llama/llama_creamy"),
        ("llama", 1, "entity/llama/llama_white"),
        ("llama", 2, "entity/llama/llama_brown"),
        ("llama", 3, "entity/llama/llama_gray"),
        ("trader_llama", 0, "entity/llama/llama_creamy"),
        ("trader_llama", 1, "entity/llama/llama_white"),
        ("trader_llama", 2, "entity/llama/llama_brown"),
        ("trader_llama", 3, "entity/llama/llama_gray"),
        // Axolotl (AxolotlVariant): 0 lucy,1 wild,2 gold,3 cyan,4 blue.
        ("axolotl", 0, "entity/axolotl/axolotl_lucy"),
        ("axolotl", 1, "entity/axolotl/axolotl_wild"),
        ("axolotl", 2, "entity/axolotl/axolotl_gold"),
        ("axolotl", 3, "entity/axolotl/axolotl_cyan"),
        ("axolotl", 4, "entity/axolotl/axolotl_blue"),
        // Horse (HorseTypeVariant): 0 white,1 creamy,2 chestnut,3 brown,4 black,5 gray,6 dark brown.
        ("horse", 0, "entity/horse/horse_white"),
        ("horse", 1, "entity/horse/horse_creamy"),
        ("horse", 2, "entity/horse/horse_chestnut"),
        ("horse", 3, "entity/horse/horse_brown"),
        ("horse", 4, "entity/horse/horse_black"),
        ("horse", 5, "entity/horse/horse_gray"),
        ("horse", 6, "entity/horse/horse_darkbrown"),
        // Mooshroom (MooshroomKind): 0 red, 1 brown.
        ("mooshroom", 0, "entity/cow/mooshroom_red"),
        ("mooshroom", 1, "entity/cow/mooshroom_brown"),
        // Shulker (dye Color 0..15). 16/none → the default purple texture.
        ("shulker", 0, "entity/shulker/shulker_white"),
        ("shulker", 1, "entity/shulker/shulker_orange"),
        ("shulker", 2, "entity/shulker/shulker_magenta"),
        ("shulker", 3, "entity/shulker/shulker_light_blue"),
        ("shulker", 4, "entity/shulker/shulker_yellow"),
        ("shulker", 5, "entity/shulker/shulker_lime"),
        ("shulker", 6, "entity/shulker/shulker_pink"),
        ("shulker", 7, "entity/shulker/shulker_gray"),
        ("shulker", 8, "entity/shulker/shulker_light_gray"),
        ("shulker", 9, "entity/shulker/shulker_cyan"),
        ("shulker", 10, "entity/shulker/shulker_purple"),
        ("shulker", 11, "entity/shulker/shulker_blue"),
        ("shulker", 12, "entity/shulker/shulker_brown"),
        ("shulker", 13, "entity/shulker/shulker_green"),
        ("shulker", 14, "entity/shulker/shulker_red"),
        ("shulker", 15, "entity/shulker/shulker_black"),
    ];
    let mut mob_variant_tex: HashMap<(String, i32), u64> = HashMap::new();
    for (kind, idx, path) in VARIANT_MOBS {
        if let Ok(img) = pack.texture_png(path) {
            let key = fnv64(format!("mobvar:{kind}:{idx}").as_bytes());
            mob_variant_tex.insert(((*kind).to_string(), *idx), key);
            mob_textures.push((key, img));
        }
    }

    // Registry-driven variants selected by *name* (the bridge resolves the
    // metadata id → name via the server registry): cat / wolf / cow / chicken /
    // pig / frog. `(kind, variant_name)` → texture.
    const NAMED_VARIANT_MOBS: &[(&str, &str, &str)] = &[
        // Cat (cat_variant registry).
        ("cat", "all_black", "entity/cat/cat_all_black"),
        ("cat", "black", "entity/cat/cat_black"),
        ("cat", "british_shorthair", "entity/cat/cat_british_shorthair"),
        ("cat", "calico", "entity/cat/cat_calico"),
        ("cat", "jellie", "entity/cat/cat_jellie"),
        ("cat", "persian", "entity/cat/cat_persian"),
        ("cat", "ragdoll", "entity/cat/cat_ragdoll"),
        ("cat", "red", "entity/cat/cat_red"),
        ("cat", "siamese", "entity/cat/cat_siamese"),
        ("cat", "tabby", "entity/cat/cat_tabby"),
        ("cat", "white", "entity/cat/cat_white"),
        // Wolf (wolf_variant registry). "pale" is the default wolf.png.
        ("wolf", "pale", "entity/wolf/wolf"),
        ("wolf", "ashen", "entity/wolf/wolf_ashen"),
        ("wolf", "black", "entity/wolf/wolf_black"),
        ("wolf", "chestnut", "entity/wolf/wolf_chestnut"),
        ("wolf", "rusty", "entity/wolf/wolf_rusty"),
        ("wolf", "snowy", "entity/wolf/wolf_snowy"),
        ("wolf", "spotted", "entity/wolf/wolf_spotted"),
        ("wolf", "striped", "entity/wolf/wolf_striped"),
        ("wolf", "woods", "entity/wolf/wolf_woods"),
        // Cow / chicken / pig / frog temperature variants (temperate = default).
        ("cow", "cold", "entity/cow/cow_cold"),
        ("cow", "temperate", "entity/cow/cow_temperate"),
        ("cow", "warm", "entity/cow/cow_warm"),
        ("chicken", "cold", "entity/chicken/chicken_cold"),
        ("chicken", "temperate", "entity/chicken/chicken_temperate"),
        ("chicken", "warm", "entity/chicken/chicken_warm"),
        ("pig", "cold", "entity/pig/pig_cold"),
        ("pig", "temperate", "entity/pig/pig_temperate"),
        ("pig", "warm", "entity/pig/pig_warm"),
        ("frog", "cold", "entity/frog/frog_cold"),
        ("frog", "temperate", "entity/frog/frog_temperate"),
        ("frog", "warm", "entity/frog/frog_warm"),
    ];
    let mut mob_named_variant_tex: HashMap<(String, String), u64> = HashMap::new();
    for (kind, vname, path) in NAMED_VARIANT_MOBS {
        if let Ok(img) = pack.texture_png(path) {
            let key = fnv64(format!("mobvarname:{kind}:{vname}").as_bytes());
            mob_named_variant_tex.insert(((*kind).to_string(), (*vname).to_string()), key);
            mob_textures.push((key, img));
        }
    }
    info!(
        models = mob_model.len(),
        variants = mob_variant_tex.len(),
        named_variants = mob_named_variant_tex.len(),
        textures = mob_textures.len(),
        "app: mob textures loaded (humanoid + models + variants)"
    );

    let panorama = load_panorama(opts.assets_dir.as_deref(), opts.asset_index.as_deref());
    if panorama.is_none() {
        info!("app: no panorama in the asset store — plain title background");
    }

    let skins = SkinManager::new(opts.assets_dir.as_deref());

    // Always start on the title screen (like vanilla Minecraft) — the menu
    // drives the connect. The Multiplayer screen is pre-filled with the
    // launcher/CLI `--server` and (offline only) the username.
    let (offline, player_name) = match &opts.bridge.account {
        AccountConfig::Offline(name) => (true, name.clone()),
        AccountConfig::Session { username, .. } => (false, username.clone()),
        AccountConfig::Microsoft(email) => (false, email.clone()),
    };
    let default_server = opts.bridge.address.clone();

    // Audio is best-effort: no device or no assets → the game runs silently.
    let audio = match (&opts.assets_dir, &opts.asset_index) {
        (Some(dir), Some(id)) => match AudioEngine::new(dir, id) {
            Ok(a) => {
                info!("app: audio enabled");
                Some(a)
            }
            Err(e) => {
                warn!(error = %format!("{e:#}"), "app: audio disabled");
                None
            }
        },
        _ => {
            info!("app: no assets dir — sound disabled");
            None
        }
    };

    // Placeable-block short-names + a representative state id each (for the
    // first-person view model: tilt tools vs blocks, and render held blocks 3D).
    let mut block_names: HashSet<String> = HashSet::new();
    let mut block_state_by_name: HashMap<String, StateId> = HashMap::new();
    for id in 0..table.len() as StateId {
        if let Some(e) = table.entry(id) {
            block_names.insert(e.short_name.clone());
            block_state_by_name.entry(e.short_name.clone()).or_insert(id);
        }
    }

    // Climate colormaps for biome tinting (grass/foliage color from a biome's
    // temperature + downfall); best-effort — plains fallback if absent.
    let grass_colormap = pack.texture_png("colormap/grass").ok();
    let foliage_colormap = pack.texture_png("colormap/foliage").ok();

    let (mesh_tx, mesh_rx) = crossbeam_channel::unbounded::<(SectionPos, MeshData)>();
    let mut app = App {
        opts,
        pack,
        table: Arc::new(table),
        store: Arc::new(store),
        atlas,
        item_icons,
        icon_tex: None,
        lang,
        lang_code,
        window: None,
        renderer: None,
        egui_ctx,
        mcui,
        egui_state: None,
        hud: Hud::new(default_server, offline, player_name),
        skins,
        steve,
        armor_textures,
        crack_textures,
        block_names,
        block_state_by_name,
        mob_skin_key,
        mob_model,
        mob_variant_tex,
        mob_named_variant_tex,
        mob_textures,
        panorama,
        panorama_loaded: false,
        biome_tints: Arc::new(crate::types::BiomeTints::default()),
        grass_colormap,
        foliage_colormap,
        mirror: WorldMirror::new(),
        bridge: None,
        mesh_tx,
        mesh_rx,
        in_flight: 0,
        player: None,
        tracks: HashMap::new(),
        cam: None,
        skin_by_uuid: HashMap::new(),
        own_name: None,
        connected: false,
        session_start: None,
        last_activity: Instant::now(),
        disconnect_reason: None,
        connect_deadline: None,
        connect_target: None,
        connect_attempt: 0,
        reconnect_at: None,
        returning_to_menu: false,
        hotbar: vec![None; 9],
        offhand: None,
        selected_slot: 0,
        last_shown_item: None,
        item_name_spans: Vec::new(),
        item_name_until: None,
        sidebar_title: Vec::new(),
        sidebar_lines: Vec::new(),
        daylight: 1.0,
        world_time: 6000,
        discord: crate::discord::Discord::spawn(),
        discord_state: None,
        session_unix_start: None,
        audio,
        settings,
        settings_dirty: true,
        last_frame_end: Instant::now(),
        last_frame: Instant::now(),
        bob_phase: 0.0,
        fov_mult: 1.0,
        dim_skylight: true,
        dim_ultrawarm: false,
        mining_target: None,
        mining_recent: None,
        mine_hit_counter: 0,
        pending_place: None,
        air_seen: false,
        keys: HashSet::new(),
        last_move: (0, 0, false),
        sneaking: false,
        sneak_latch: false,
        sprint_latch: false,
        forward_since: None,
        auto_jump_until: None,
        yaw: 0.0,
        pitch: 20.0,
        dir_synced: false,
        last_sent_dir: None,
        pending_mouse: (0.0, 0.0),
        grabbed: false,
        left_held: false,
        right_held: false,
        hand_swing_start: None,
        view_equip_start: Instant::now(),
        view_last_item: None,
        use_start: None,
        use_repeat_at: None,
        grab_retry_at: None,
        focused: true,
        tab_held: false,
        perspective: 0,
        hud_hidden: false,
        particles: Vec::new(),
        rain_level: 0.0,
        thunder_level: 0.0,
        active_effects: HashMap::new(),
        cooldowns: HashMap::new(),
        effect_tex: HashMap::new(),
        rain_drops: Vec::new(),
        particle_rng: 0x9E37_79B9_7F4A_7C15,
        last_health: -1.0,
        hurt_flash_until: None,
        frame_times: VecDeque::with_capacity(FPS_WINDOW + 1),
        fps_display: 0.0,
        fps_updated: Instant::now(),
        start: Instant::now(),
        last_stats: (0, 0),
        frame_counter: 0,
        fatal: None,
    };

    let event_loop = EventLoop::new().context("creating winit event loop")?;
    event_loop.run_app(&mut app).context("running event loop")?;

    match app.fatal.take() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Movement history of one remote entity, for delayed interpolation +
/// walk-cycle animation.
struct EntityTrack {
    /// (arrival, pos, yaw, pitch) — oldest first, bounded.
    hist: VecDeque<(Instant, [f64; 3], f32, f32)>,
    snap: EntitySnapshot,
    /// Walk-cycle phase/amplitude (players only).
    phase: f32,
    amp: f32,
    last_render: Option<(Instant, [f64; 3])>,
    /// Until when this entity flashes red (took damage).
    hurt_until: Option<Instant>,
    /// When the current arm-swing (attack/mine) animation started; drives a
    /// one-shot swing arc that decays over ~250 ms like vanilla.
    swing_start: Option<Instant>,
}

impl EntityTrack {
    fn new(snap: EntitySnapshot, now: Instant) -> Self {
        let mut hist = VecDeque::with_capacity(8);
        hist.push_back((now, snap.pos, snap.yaw, snap.pitch));
        Self {
            hist,
            snap,
            phase: 0.0,
            amp: 0.0,
            last_render: None,
            hurt_until: None,
            swing_start: None,
        }
    }

    fn push(&mut self, snap: EntitySnapshot, now: Instant) {
        if let Some((_, last, _, _)) = self.hist.back() {
            let d2 = (snap.pos[0] - last[0]).powi(2)
                + (snap.pos[1] - last[1]).powi(2)
                + (snap.pos[2] - last[2]).powi(2);
            if d2 > 64.0 {
                self.hist.clear(); // teleport: don't glide across the world
            }
        }
        self.hist.push_back((now, snap.pos, snap.yaw, snap.pitch));
        while self.hist.len() > 8 {
            self.hist.pop_front();
        }
        self.snap = snap;
    }

    /// Position/rotation at `t`, interpolated between bracketing snapshots.
    fn sample(&self, t: Instant) -> ([f64; 3], f32, f32) {
        let h = &self.hist;
        let last = h.back().expect("hist never empty");
        if h.len() == 1 || t >= last.0 {
            return (last.1, last.2, last.3);
        }
        let first = h.front().expect("hist never empty");
        if t <= first.0 {
            return (first.1, first.2, first.3);
        }
        for w in 0..h.len() - 1 {
            let (t0, p0, y0, pi0) = h[w];
            let (t1, p1, y1, pi1) = h[w + 1];
            if t >= t0 && t <= t1 {
                let span = t1.duration_since(t0).as_secs_f64().max(1e-6);
                let a = (t.duration_since(t0).as_secs_f64() / span).clamp(0.0, 1.0);
                let pos = [
                    p0[0] + (p1[0] - p0[0]) * a,
                    p0[1] + (p1[1] - p0[1]) * a,
                    p0[2] + (p1[2] - p0[2]) * a,
                ];
                return (pos, lerp_angle(y0, y1, a as f32), pi0 + (pi1 - pi0) * a as f32);
            }
        }
        (last.1, last.2, last.3)
    }
}

/// One live particle, simulated on the CPU and drawn as a tiny colored cube.
struct Particle {
    pos: [f64; 3],
    vel: [f64; 3],
    color: [f32; 3],
    size: f32,
    age: f32,
    life: f32,
    /// Downward acceleration (blocks/s²); can be negative for floaty particles.
    gravity: f32,
}

/// Local-player camera smoothing: extrapolate from the last 20 Hz snapshot
/// using the observed velocity, then chase it exponentially.
struct CamTrack {
    snap_pos: [f64; 3],
    snap_t: Instant,
    /// Blocks per second, from the last two snapshots.
    vel: [f64; 3],
    /// The smoothed position frames actually render from.
    render_pos: [f64; 3],
}

struct App {
    opts: AppOptions,
    /// Kept open for language reloads.
    pack: AssetPack,
    table: Arc<BlockTable>,
    store: Arc<BakedModelStore>,
    atlas: Atlas,
    item_icons: Arc<ItemIcons>,
    /// egui texture for the item-icon atlas; created lazily on the first frame.
    icon_tex: Option<egui::TextureHandle>,
    lang: Lang,
    lang_code: String,

    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    egui_ctx: egui::Context,
    /// Vanilla GUI assets (font/sprites) shared by all menu drawing.
    mcui: Arc<mcui::McUi>,
    egui_state: Option<egui_winit::State>,
    hud: Hud,
    skins: SkinManager,
    /// Default skin, uploaded as renderer key 0 once the renderer exists.
    steve: Option<image::RgbaImage>,
    /// Armor textures (material, is-leggings-layer, image), uploaded to the
    /// renderer once it exists. Drives armor on other players.
    armor_textures: Vec<(ArmorMaterial, bool, image::RgbaImage)>,
    /// destroy_stage_0..9 for the mining crack overlay.
    crack_textures: Vec<image::RgbaImage>,
    /// Short-names of all placeable blocks (from the block table), so the
    /// first-person view model can tilt a held block differently from a tool.
    block_names: HashSet<String>,
    /// Block short-name → a representative state id, for rendering a held block
    /// as a real 3D cube in the first-person view.
    block_state_by_name: HashMap<String, StateId>,
    /// Humanoid mob registry kind → renderer skin key (their real texture).
    mob_skin_key: HashMap<String, u64>,
    /// Non-humanoid mob registry kind → (texture key, cuboid model).
    mob_model: HashMap<String, (u64, MobModel)>,
    /// Per-species variant texture key, by `(kind, variant index)` — overrides
    /// the default `mob_model` texture (colour/type variants: rabbit, fox,
    /// parrot, llama, axolotl, horse, mooshroom, shulker colour…).
    mob_variant_tex: HashMap<(String, i32), u64>,
    /// Registry-driven variant texture key, by `(kind, variant name)` — cat,
    /// wolf, cow, chicken, pig, frog (name resolved server-side).
    mob_named_variant_tex: HashMap<(String, String), u64>,
    /// Mob textures (skin key, image) waiting for the renderer (uploaded once).
    mob_textures: Vec<(u64, image::RgbaImage)>,
    /// Panorama faces waiting for the renderer (taken on upload).
    panorama: Option<[image::RgbaImage; 6]>,
    panorama_loaded: bool,
    /// Per-biome grass/foliage/water tint colors, built from the server biome
    /// registry (`GameEvent::Biomes`). Empty until then → plains fallback.
    biome_tints: Arc<crate::types::BiomeTints>,
    /// The grass/foliage climate colormaps from the jar, for biomes with no
    /// explicit color override.
    grass_colormap: Option<image::RgbaImage>,
    foliage_colormap: Option<image::RgbaImage>,

    mirror: WorldMirror,
    bridge: Option<(GameHandle, Receiver<GameEvent>)>,
    mesh_tx: Sender<(SectionPos, MeshData)>,
    mesh_rx: Receiver<(SectionPos, MeshData)>,
    in_flight: usize,

    player: Option<PlayerSnapshot>,
    /// Interpolation + animation state per entity id (also drives hit tests).
    tracks: HashMap<u64, EntityTrack>,
    cam: Option<CamTrack>,
    /// uuid → (skin url, slim), from the tab list.
    skin_by_uuid: HashMap<String, (String, bool)>,
    own_name: Option<String>,
    connected: bool,
    /// When the current server session began (for the Statistics screen).
    session_start: Option<Instant>,
    /// Last time the bridge emitted *any* event while in-game. If it goes
    /// stale the connection is dead (silent server timeout, or azalea's
    /// schedule loop froze) — the watchdog then tears the session down locally
    /// so the player is never stuck on a frozen world with a dead menu.
    last_activity: Instant,
    disconnect_reason: Option<String>,
    /// Backstop: give up on a connect attempt that produces no event at all
    /// (e.g. azalea hanging silently after a failed session-server auth).
    connect_deadline: Option<Instant>,
    /// The (address, username) of the in-flight/last connect, kept so a
    /// transient first-attempt failure can be retried automatically.
    connect_target: Option<(String, String)>,
    /// 1-based attempt number of the current connect (for the retry UI).
    connect_attempt: u32,
    /// When set, re-spawn the bridge at this instant (auto-retry backoff).
    reconnect_at: Option<Instant>,
    /// Set when the user chose "Disconnect" from the pause menu: the resulting
    /// Disconnected event returns to the title screen instead of the error box.
    returning_to_menu: bool,
    hotbar: Vec<Option<ItemSnapshot>>,
    offhand: Option<ItemSnapshot>,
    selected_slot: u8,
    /// The (slot, item name) last shown as the held-item name popup, to detect a
    /// switch; the popup's spans + when it should finish fading.
    last_shown_item: Option<(u8, String)>,
    item_name_spans: Vec<ChatSpan>,
    item_name_until: Option<Instant>,
    /// Sidebar scoreboard title + rows (empty = no sidebar).
    sidebar_title: Vec<ChatSpan>,
    sidebar_lines: Vec<ScoreLine>,
    daylight: f32,
    /// Raw world time-of-day (ticks), for the sun/moon angle and star fade.
    /// Negative when the daylight cycle is frozen (rate 0); the sky uses `abs`.
    world_time: i64,

    /// Discord Rich Presence worker (`None` when disabled or unavailable).
    discord: Option<crate::discord::Discord>,
    /// Last activity pushed to Discord — diffed so we only write on change.
    discord_state: Option<crate::discord::Activity>,
    /// Unix seconds the current session connected (Discord's "elapsed" timer).
    session_unix_start: Option<u64>,

    /// Sound engine (rodio). `None` when there's no audio device or no assets.
    audio: Option<AudioEngine>,

    /// Persistent, vanilla-style options. Drives the camera, renderer, GUI
    /// scale, FPS cap, keybinds and more each frame.
    settings: GameSettings,
    /// Set when a setting that the renderer/window must apply changed
    /// (vsync, fullscreen); applied at the top of the next frame.
    settings_dirty: bool,
    /// End-of-frame instant, for the software FPS cap when vsync is off.
    last_frame_end: Instant,
    /// Start of the previous frame (smoothing time step).
    last_frame: Instant,
    /// Accumulated view-bob phase (advances while walking).
    bob_phase: f32,
    /// Smoothed dynamic-FOV multiplier (1.0 idle, eases toward 1.10 while
    /// sprinting like vanilla — never an instant snap).
    fov_mult: f32,
    /// Current dimension has sky light (false in Nether/End) — drives the
    /// sky color and ambient brightness.
    dim_skylight: bool,
    /// Nether-style dimension (red sky/fog).
    dim_ultrawarm: bool,
    /// Block being mined + progress 0..1 (crack overlay), from the snapshot.
    mining_target: Option<(BlockPos, f32)>,
    /// Last mined position + when — the break confirmation (BlockChanged to
    /// air) arrives after azalea already dropped its mining state.
    mining_recent: Option<(BlockPos, Instant)>,
    /// Snapshot counter while mining (hit sound every 4th, like vanilla).
    mine_hit_counter: u32,
    /// Right-click placement prediction: (clicked block, offset block, when).
    /// The place sound plays when either position turns non-air shortly after.
    pending_place: Option<(BlockPos, BlockPos, Instant)>,
    /// The server sent a real air-supply value at least once. azalea's
    /// component defaults to 0 and vanilla servers stay silent until it
    /// changes — without this the bubble bar would read "drowned" on join.
    air_seen: bool,

    keys: HashSet<KeyCode>,
    last_move: (i8, i8, bool),
    sneaking: bool,
    /// Toggle-mode latches (Sneak/Sprint options).
    sneak_latch: bool,
    sprint_latch: bool,
    /// Since when the forward key is held (auto-jump needs "pushing a wall").
    forward_since: Option<Instant>,
    /// An auto-jump pulse is active until this instant (then Jump(false)).
    auto_jump_until: Option<Instant>,
    yaw: f32,
    pitch: f32,
    /// Camera direction initialized from the first PlayerState.
    dir_synced: bool,
    last_sent_dir: Option<(f32, f32)>,
    pending_mouse: (f64, f64),
    grabbed: bool,
    /// Left/right mouse held — drives hold-to-mine and bow/crossbow charge.
    left_held: bool,
    right_held: bool,
    /// When our own hand last started an attack/use swing, for the third-person
    /// arm animation (vanilla swings your arm on left/right click).
    hand_swing_start: Option<Instant>,
    /// When the currently held item was equipped, for the first-person raise
    /// animation (vanilla slides the new item up when you switch).
    view_equip_start: Instant,
    /// The item name last shown in the first-person hand, to detect a switch.
    view_last_item: Option<String>,
    /// When the current item-use (eat/drink/bow/shield) began — raises the
    /// first-person item toward the mouth. `None` while not using.
    use_start: Option<Instant>,
    /// Throttle for hold-to-place: earliest instant the next held right-click
    /// `UseItem` may fire (vanilla repeats block placement while held).
    use_repeat_at: Option<Instant>,
    /// Backoff after a failed grab (X11 can refuse while a popup is up).
    grab_retry_at: Option<Instant>,
    focused: bool,
    /// Player-list key held. Tracked explicitly because egui-winit *always*
    /// reports Tab as consumed, so it never reaches the `keys` set.
    tab_held: bool,
    /// Camera perspective: 0 = first person, 1 = third person (behind),
    /// 2 = third person (front). Cycled with the perspective key (F5).
    perspective: u8,
    /// F1 hides the whole in-game HUD (world stays visible).
    hud_hidden: bool,
    /// Live particles (cube sprites), simulated each frame.
    particles: Vec<Particle>,
    /// Rain/thunder strength (0..1) from the server's weather events.
    rain_level: f32,
    thunder_level: f32,
    /// Active potion effects on the local player: name → (amplifier, expiry).
    /// `None` expiry = infinite (beacon/spawn effects).
    active_effects: HashMap<String, (u32, Option<Instant>)>,
    /// Item use-cooldowns by registry name → (end instant, total seconds), for
    /// the vanilla shrinking sweep over hotbar/off-hand slots.
    cooldowns: HashMap<String, (Instant, f32)>,
    /// egui textures for the effect icons (`mob_effect/<name>`), loaded lazily.
    effect_tex: HashMap<String, egui::TextureHandle>,
    /// Falling rain streaks: `(world pos, fall speed)`, recycled around the
    /// player while it rains.
    rain_drops: Vec<([f64; 3], f32)>,
    /// Cheap xorshift state for particle jitter (Math::random is fine here, but
    /// a tiny PRNG keeps spawns deterministic and dependency-free).
    particle_rng: u64,
    /// Last seen local-player health, to detect damage (hurt flash + sound).
    last_health: f32,
    /// Until when the red damage vignette is shown; drives its fade.
    hurt_flash_until: Option<Instant>,

    frame_times: VecDeque<Instant>,
    /// Debug-overlay FPS reading, refreshed on a slow cadence so the number
    /// reads steadily instead of flickering every frame.
    fps_display: f32,
    fps_updated: Instant,
    start: Instant,
    /// (sections_drawn, sections_total) of the last rendered frame.
    last_stats: (usize, usize),
    frame_counter: u64,
    fatal: Option<anyhow::Error>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Poll);
        if self.window.is_some() {
            return; // redundant Resumed
        }
        let attrs = Window::default_attributes()
            .with_title("DolphinClient")
            .with_window_icon(load_window_icon())
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.fatal = Some(anyhow::anyhow!(e).context("creating window"));
                event_loop.exit();
                return;
            }
        };
        match Renderer::new(RenderTarget::Window(window.clone())) {
            Ok(mut r) => {
                r.set_atlas(&self.atlas);
                if let Some(steve) = &self.steve {
                    r.ensure_skin(0, steve);
                }
                for (mat, leggings, img) in &self.armor_textures {
                    r.ensure_armor(*mat, *leggings, img);
                }
                r.set_crack_textures(&self.crack_textures);
                for (key, img) in &self.mob_textures {
                    r.ensure_skin(*key, img);
                }
                if !self.item_icons.is_empty() {
                    r.ensure_item_atlas(&self.item_icons.image);
                }
                load_sky_textures(&mut self.pack, &mut r);
                if let Some(faces) = self.panorama.take() {
                    r.set_panorama(&faces);
                    self.panorama_loaded = true;
                }
                self.renderer = Some(r);
            }
            Err(e) => {
                self.fatal = Some(e.context("creating renderer"));
                event_loop.exit();
                return;
            }
        }
        self.egui_state = Some(egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            None,
        ));
        info!("app: window + renderer ready");
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let consumed = match (&self.egui_state, &self.window) {
            (Some(_), Some(_)) => {
                // Both exist; split borrows.
                let window = self.window.clone().expect("checked");
                self.egui_state
                    .as_mut()
                    .expect("checked")
                    .on_window_event(&window, &event)
                    .consumed
            }
            _ => false,
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer
                    && size.width > 0
                    && size.height > 0
                {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                if !focused {
                    // Drop all movement state; keys released while unfocused
                    // are lost. The next focused frame re-grabs automatically.
                    self.keys.clear();
                    self.tab_held = false;
                    self.set_grab(false);
                    self.push_move_if_changed();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.on_key(code, event.state, event.repeat, consumed);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                // While the pointer is grabbed the game owns the mouse. egui
                // derives `consumed` from a cursor position it can no longer
                // see — it stops getting `CursorMoved` the moment we lock the
                // pointer, so its last-known position is stale and it can
                // wrongly claim clicks/scrolls, suppressing attack/mine/use and
                // hotbar scrolling. Treat "grabbed" as never-consumed.
                let consumed = consumed && !self.grabbed;
                if pressed && !consumed {
                    self.on_click(button);
                }
                self.on_mouse_button(button, pressed, consumed);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let consumed = consumed && !self.grabbed;
                if !consumed {
                    let dy = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                        winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32,
                    };
                    self.on_scroll(dy);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.frame(event_loop) {
                    self.fatal = Some(e.context("rendering frame"));
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event
            && self.grabbed
        {
            self.pending_mouse.0 += delta.0;
            self.pending_mouse.1 += delta.1;
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

impl App {
    fn send_cmd(&self, cmd: Command) {
        if let Some((handle, _)) = &self.bridge {
            handle.send(cmd);
        }
    }

    /// Whether the game should own the pointer right now. Applied every frame
    /// — closing chat/pause/container automatically re-captures the mouse.
    fn desired_grab(&self) -> bool {
        self.focused
            && self.connected
            && self.disconnect_reason.is_none()
            && !self.hud.overlay_open()
    }

    fn set_grab(&mut self, on: bool) {
        let Some(window) = &self.window else { return };
        if on == self.grabbed {
            return;
        }
        if on {
            if self.grab_retry_at.is_some_and(|t| Instant::now() < t) {
                return;
            }
            let res = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            match res {
                Ok(()) => {
                    window.set_cursor_visible(false);
                    self.grabbed = true;
                    self.grab_retry_at = None;
                }
                Err(e) => {
                    warn!("app: cursor grab failed: {e}");
                    self.grab_retry_at = Some(Instant::now() + Duration::from_secs(1));
                }
            }
        } else {
            if let Err(e) = window.set_cursor_grab(CursorGrabMode::None) {
                warn!("app: cursor ungrab failed: {e}");
            }
            window.set_cursor_visible(true);
            self.grabbed = false;
            // Losing the pointer ends any hold-to-mine and fires a charged bow
            // (we won't get the physical release event once ungrabbed).
            if self.right_held {
                self.send_cmd(Command::ReleaseUseItem);
            }
            if self.left_held {
                self.send_cmd(Command::SetMining(false));
            }
            self.left_held = false;
            self.right_held = false;
        }
    }

    /// Close the open container screen (Esc/E): tell the server for real
    /// windows, drop the local view either way.
    fn close_container(&mut self) {
        if let Some(id) = self.hud.open_container_id()
            && id != 0
        {
            self.send_cmd(Command::CloseContainer { id });
        }
        self.hud.close_container_view();
    }

    // -- input -----------------------------------------------------------------

    fn on_key(&mut self, code: KeyCode, state: ElementState, repeat: bool, consumed: bool) {
        let pressed = state.is_pressed();

        // A Controls row is listening: the next key press becomes the binding.
        if pressed && self.hud.rebinding.is_some() {
            if let Some(field) = self.hud.rebinding.take()
                && code != KeyCode::Escape
            {
                field.set(&mut self.settings.keys, key_id(code));
                self.settings.save();
            }
            return;
        }

        // Player-list key (default Tab). egui-winit *always* reports Tab as
        // consumed (it steals Tab for focus traversal), so it can never reach
        // the `keys` set below — and letting it fall through would `keys.clear()`
        // and freeze movement every time you glance at the list. Handle it here,
        // ahead of the consumed early-return, tracking a hold flag directly.
        // Must catch key *repeats* too (held Tab emits them), or a held glance
        // would re-hit the clear-and-freeze path.
        if KeyBinds::matches(&self.settings.keys.player_list, code) {
            self.tab_held = pressed
                && self.connected
                && !self.hud.wants_keyboard()
                && !self.hud.is_paused()
                && !self.hud.container_open();
            return;
        }

        // Overlay-independent toggles (never text input keys).
        if pressed && !repeat && KeyBinds::matches(&self.settings.keys.debug, code) {
            self.hud.show_debug = !self.hud.show_debug;
            return;
        }
        if pressed && !repeat && KeyBinds::matches(&self.settings.keys.fullscreen, code) {
            self.settings.fullscreen = !self.settings.fullscreen;
            self.settings.save();
            self.settings_dirty = true;
            return;
        }
        if pressed
            && !repeat
            && KeyBinds::matches(&self.settings.keys.hide_hud, code)
            && self.connected
        {
            self.hud_hidden = !self.hud_hidden;
            return;
        }
        if pressed
            && !repeat
            && KeyBinds::matches(&self.settings.keys.perspective, code)
            && self.connected
            && !self.hud.wants_keyboard()
        {
            self.perspective = (self.perspective + 1) % 3;
            return;
        }
        if pressed && !repeat && code == KeyCode::Escape {
            // Chat handles Esc itself; pre-game screens handle it themselves.
            // In game: container first, then the pause menu.
            if !self.hud.chat.open && self.connected && self.disconnect_reason.is_none() {
                if self.hud.container_open() {
                    self.close_container();
                } else {
                    self.hud.toggle_pause();
                    if self.hud.is_paused() {
                        self.keys.clear();
                        self.push_move_if_changed();
                    }
                }
            }
            return;
        }

        // Container screen: inventory key closes it; everything else is UI.
        if self.hud.container_open() {
            if pressed && !repeat && KeyBinds::matches(&self.settings.keys.inventory, code) {
                self.close_container();
            }
            self.keys.clear();
            self.push_move_if_changed();
            return;
        }

        if consumed || self.hud.wants_keyboard() || self.hud.is_paused() {
            // A text field owns the keyboard, or the game is paused: no game
            // keys (movement, jump, hotbar) may stick or fire.
            self.keys.clear();
            self.push_move_if_changed();
            return;
        }

        if pressed {
            self.keys.insert(code);
        } else {
            self.keys.remove(&code);
        }

        if pressed && !repeat {
            if let Some(slot) = hotbar_slot(&self.settings.keys, code) {
                self.selected_slot = slot;
                self.send_cmd(Command::SelectHotbar(slot));
                return;
            }
            if KeyBinds::matches(&self.settings.keys.jump, code) {
                self.send_cmd(Command::Jump(true));
            }
            if KeyBinds::matches(&self.settings.keys.sneak, code) && self.settings.sneak_toggle {
                self.sneak_latch = !self.sneak_latch;
            }
            if KeyBinds::matches(&self.settings.keys.sprint, code) && self.settings.sprint_toggle {
                self.sprint_latch = !self.sprint_latch;
            }
            if self.connected {
                if KeyBinds::matches(&self.settings.keys.chat, code) {
                    self.hud.chat.open_with("");
                    self.keys.clear();
                    self.push_move_if_changed();
                } else if KeyBinds::matches(&self.settings.keys.command, code) {
                    self.hud.chat.open_with("/");
                    self.keys.clear();
                    self.push_move_if_changed();
                } else if KeyBinds::matches(&self.settings.keys.inventory, code) {
                    self.hud.open_own_inventory();
                    self.keys.clear();
                    self.push_move_if_changed();
                } else if KeyBinds::matches(&self.settings.keys.drop, code) {
                    let all = self.keys.contains(&KeyCode::ControlLeft)
                        || self.keys.contains(&KeyCode::ControlRight);
                    self.send_cmd(Command::DropItem { all });
                } else if KeyBinds::matches(&self.settings.keys.swap_offhand, code) {
                    self.send_cmd(Command::SwapOffhand);
                }
            }
        } else if !pressed && KeyBinds::matches(&self.settings.keys.jump, code) {
            self.send_cmd(Command::Jump(false));
        }
    }

    fn on_click(&mut self, button: MouseButton) {
        if !self.grabbed {
            return; // menus/overlays: egui owns the mouse
        }
        // Swing our own arm on any click (attack / mine / use), like vanilla.
        self.hand_swing_start = Some(Instant::now());
        let Some(p) = &self.player else { return };
        let eye = [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]];
        let d = camera::view_dir(self.yaw, self.pitch);
        let dir = [d.x as f64, d.y as f64, d.z as f64];
        let table = self.table.clone();
        let hit = self.mirror.raycast(eye, dir, 5.0, |id| table.is_air(id));
        // Distance to the block hit, for entity-vs-block priority. The block
        // is a unit cube, so reuse the same slab test the entities use.
        let block_t = hit.and_then(|(pos, _)| {
            let min = [pos.x as f64, pos.y as f64, pos.z as f64];
            ray_aabb(eye, dir, min, [min[0] + 1.0, min[1] + 1.0, min[2] + 1.0])
        });
        match button {
            MouseButton::Left => {
                // Vanilla-style: an entity within attack reach (3.0) that is
                // not occluded by a nearer block wins over mining.
                if let Some((id, t)) = self.entity_hit(eye, dir, 3.0)
                    && block_t.is_none_or(|bt| t < bt)
                {
                    self.send_cmd(Command::Attack(id));
                    return;
                }
                // Block mining is driven by the hold-to-mine toggle in
                // `on_mouse_button` (azalea's native `left_click_mine`), which
                // starts breaking the crosshair block on the next tick and
                // handles survival progress/target-changes correctly — the
                // reliable path that makes Survival digging actually work.
                let _ = hit;
            }
            MouseButton::Right => {
                // Vanilla priority: an entity within interact reach (3.0) that
                // isn't occluded by a nearer block gets right-clicked — trade
                // with a villager, mount a boat/horse, name-tag a mob, dye a
                // sheep, and so on.
                if let Some((id, t)) = self.entity_hit(eye, dir, 3.0)
                    && block_t.is_none_or(|bt| t < bt)
                {
                    self.send_cmd(Command::InteractEntity(id));
                    return;
                }
                // Otherwise azalea decides: place/use the block under the
                // crosshair, or use the held item (bow, crossbow, ender
                // pearl/snowball, eat food) when looking at air.
                self.send_cmd(Command::UseItem);
                // Predict where a block would land so its confirmation
                // (BlockChanged) plays the place sound: either the clicked
                // block itself (replaceables like grass) or face-adjacent.
                if let Some((bpos, face)) = hit {
                    let n = face.normal();
                    self.pending_place =
                        Some((bpos, bpos.offset(n[0], n[1], n[2]), Instant::now()));
                }
                // Arm the hold-to-place throttle so the next repeat waits.
                self.use_repeat_at = Some(Instant::now() + Duration::from_millis(220));
            }
            _ => {}
        }
    }

    /// While the right button is held and the crosshair is on a block, keep
    /// re-issuing `UseItem` on a throttle so a row of blocks can be placed by
    /// dragging (vanilla behaviour). Skipped when looking at air or an entity,
    /// so charged items (bow/crossbow) and eating aren't retriggered.
    fn continue_using(&mut self) {
        if !self.right_held || !self.grabbed || self.player.is_none() {
            return;
        }
        let now = Instant::now();
        if self.use_repeat_at.is_some_and(|t| now < t) {
            return;
        }
        let p = self.player.as_ref().unwrap();
        let eye = [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]];
        let d = camera::view_dir(self.yaw, self.pitch);
        let dir = [d.x as f64, d.y as f64, d.z as f64];
        if self.entity_hit(eye, dir, 3.0).is_some() {
            return;
        }
        let table = self.table.clone();
        if let Some((bpos, face)) = self.mirror.raycast(eye, dir, 5.0, |id| table.is_air(id)) {
            self.send_cmd(Command::UseItem);
            let n = face.normal();
            self.pending_place = Some((bpos, bpos.offset(n[0], n[1], n[2]), Instant::now()));
            self.use_repeat_at = Some(now + Duration::from_millis(220));
        }
    }

    /// Mouse-wheel over the hotbar: cycle the selected slot like vanilla
    /// (scroll up → previous, scroll down → next; wraps 0..8).
    fn on_scroll(&mut self, dy: f32) {
        if !self.grabbed || dy == 0.0 {
            return; // menus/overlays own the wheel (chat scroll, sliders, …)
        }
        // +8 ≡ -1 (mod 9): scrolling up moves to the previous slot.
        let step: u8 = if dy > 0.0 { 8 } else { 1 };
        let next = (self.selected_slot + step) % 9;
        self.selected_slot = next;
        self.send_cmd(Command::SelectHotbar(next));
    }

    /// Track mouse-button hold state for hold-to-mine and bow charge/release.
    fn on_mouse_button(&mut self, button: MouseButton, pressed: bool, consumed: bool) {
        match button {
            MouseButton::Left => {
                let want = pressed && self.grabbed && !consumed;
                if want != self.left_held {
                    self.left_held = want;
                    // Toggle azalea's continuous mining: it breaks whatever
                    // block is under the crosshair while held, like vanilla.
                    self.send_cmd(Command::SetMining(want));
                }
            }
            MouseButton::Right => {
                let was = self.right_held;
                self.right_held = pressed && self.grabbed && !consumed;
                // Releasing after a use fires a charged item (bow/crossbow/trident).
                if !pressed && was {
                    self.send_cmd(Command::ReleaseUseItem);
                }
            }
            _ => {}
        }
    }

    /// True when an attackable entity is under the crosshair within melee
    /// reach and not hidden behind a nearer block — drives the vanilla
    /// full-charge attack indicator.
    fn crosshair_target_in_reach(&self) -> bool {
        let Some(p) = &self.player else { return false };
        let eye = [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]];
        let d = camera::view_dir(self.yaw, self.pitch);
        let dir = [d.x as f64, d.y as f64, d.z as f64];
        let Some((_, t)) = self.entity_hit(eye, dir, 3.0) else { return false };
        let table = &self.table;
        let block_t = self
            .mirror
            .raycast(eye, dir, 5.0, |id| table.is_air(id))
            .and_then(|(pos, _)| {
                let min = [pos.x as f64, pos.y as f64, pos.z as f64];
                ray_aabb(eye, dir, min, [min[0] + 1.0, min[1] + 1.0, min[2] + 1.0])
            });
        block_t.is_none_or(|bt| t < bt)
    }

    /// Nearest remote entity whose hitbox the view ray enters within `reach`
    /// blocks: `(bridge id, ray distance)`.
    ///
    /// Hit-tested against each entity's *interpolated, rendered* position (the
    /// same ~100 ms-delayed sample the model is drawn at), not its raw latest
    /// snapshot — otherwise the crosshair and the visible mob disagree and
    /// attacks on moving targets miss.
    fn entity_hit(&self, eye: [f64; 3], dir: [f64; 3], reach: f64) -> Option<(u64, f64)> {
        let now = Instant::now();
        let render_t = now.checked_sub(ENTITY_LERP_DELAY).unwrap_or(now);
        let mut best: Option<(u64, f64)> = None;
        for track in self.tracks.values() {
            let e = &track.snap;
            if e.is_player && e.name.is_some() && e.name == self.own_name {
                continue;
            }
            let (pos, _, _) = track.sample(render_t);
            let hw = e.width as f64 / 2.0;
            let min = [pos[0] - hw, pos[1], pos[2] - hw];
            let max = [pos[0] + hw, pos[1] + e.height as f64, pos[2] + hw];
            if let Some(t) = ray_aabb(eye, dir, min, max)
                && t <= reach
                && best.is_none_or(|(_, bt)| t < bt)
            {
                best = Some((e.id, t));
            }
        }
        best
    }

    /// One xorshift step in [0,1) for particle jitter.
    fn rand01(&mut self) -> f32 {
        let mut x = self.particle_rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.particle_rng = x;
        // Top 24 bits → [0,1).
        ((x >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Spawn `count` particles around `origin`, jittered within `±spread` and
    /// given a random velocity up to `speed` blocks/tick. Bounded so bursts
    /// can't grow the pool without limit.
    #[allow(clippy::too_many_arguments)]
    fn spawn_particles(
        &mut self,
        origin: [f64; 3],
        color: [f32; 3],
        size: f32,
        count: u32,
        spread: [f32; 3],
        speed: f32,
        gravity: f32,
    ) {
        // Bounded: each live particle is one draw call, so keep the ceiling
        // modest even during explosion/firework spam. The Particles option
        // scales the count (All / Decreased / Minimal), like vanilla.
        const CAP: usize = 1500;
        let factor = self.settings.particles.factor();
        let count = (count as f32 * factor).round() as usize;
        let count = count.min(CAP - self.particles.len().min(CAP));
        for _ in 0..count {
            let jx = (self.rand01() * 2.0 - 1.0) * spread[0];
            let jy = (self.rand01() * 2.0 - 1.0) * spread[1];
            let jz = (self.rand01() * 2.0 - 1.0) * spread[2];
            // Velocity: server `max_speed` is blocks/tick → blocks/second.
            let vx = (self.rand01() * 2.0 - 1.0) * speed * 20.0;
            let vy = (self.rand01() * 2.0 - 1.0) * speed * 20.0;
            let vz = (self.rand01() * 2.0 - 1.0) * speed * 20.0;
            let life = 0.6 + self.rand01() * 0.9;
            self.particles.push(Particle {
                pos: [origin[0] + jx as f64, origin[1] + jy as f64, origin[2] + jz as f64],
                vel: [vx as f64, vy as f64, vz as f64],
                color,
                size,
                age: 0.0,
                life,
                gravity,
            });
        }
    }

    /// Advance and cull particles (Euler step with a little drag).
    fn tick_particles(&mut self, dt: f32) {
        if self.particles.is_empty() {
            return;
        }
        let dt64 = dt as f64;
        let drag = (1.0 - 1.6 * dt64).clamp(0.0, 1.0);
        self.particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.life {
                return false;
            }
            p.vel[1] -= p.gravity as f64 * dt64;
            p.vel[0] *= drag;
            p.vel[1] *= drag;
            p.vel[2] *= drag;
            p.pos[0] += p.vel[0] * dt64;
            p.pos[1] += p.vel[1] * dt64;
            p.pos[2] += p.vel[2] * dt64;
            true
        });
    }

    /// Recycle the falling-rain field around the player: grow/shrink to a count
    /// scaled by rain strength (and the particles setting), fall each drop, and
    /// respawn drops that fell past the player or drifted too far.
    fn tick_rain(&mut self, dt: f32) {
        let center = match &self.player {
            Some(p) if self.rain_level > 0.01 => p.pos,
            _ => {
                self.rain_drops.clear();
                return;
            }
        };
        const R: f64 = 12.0;
        const MAX: usize = 220;
        let factor = self.settings.particles.factor().max(0.25);
        let target = (self.rain_level.clamp(0.0, 1.0) * MAX as f32 * factor) as usize;
        let mut rng = self.particle_rng;
        let drops = &mut self.rain_drops;
        while drops.len() < target {
            drops.push(spawn_raindrop(&mut rng, center, R));
        }
        drops.truncate(target);
        let dt64 = dt as f64;
        for d in drops.iter_mut() {
            d.0[1] -= d.1 as f64 * dt64;
            let (dx, dz) = (d.0[0] - center[0], d.0[2] - center[2]);
            if d.0[1] < center[1] - 4.0 || dx * dx + dz * dz > (R + 5.0) * (R + 5.0) {
                *d = spawn_raindrop(&mut rng, center, R);
            }
        }
        self.particle_rng = rng;
    }

    /// How far the third-person camera may sit from the eye along `dir` before a
    /// solid block would clip it — capped at `max`, with a small wall margin.
    fn third_person_distance(&self, eye: [f64; 3], dir: [f64; 3], max: f64) -> f64 {
        let table = self.table.clone();
        if let Some((pos, _)) = self.mirror.raycast(eye, dir, max, |id| table.is_air(id)) {
            let min = [pos.x as f64, pos.y as f64, pos.z as f64];
            let far = [min[0] + 1.0, min[1] + 1.0, min[2] + 1.0];
            if let Some(t) = ray_aabb(eye, dir, min, far) {
                return (t - 0.25).clamp(0.0, max);
            }
        }
        max
    }

    /// The local player's own model, drawn only in third-person perspective
    /// (the bridge never sends our own entity, so we synthesize it here).
    fn local_player_draw(&self) -> Option<EntityDraw> {
        if self.perspective == 0 {
            return None;
        }
        let pos = self.cam.as_ref().map(|c| c.render_pos).or(self.player.as_ref().map(|p| p.pos))?;
        // Own skin: look ourselves up in the tab list by name, else Steve (0).
        let (mut skin, mut slim) = (0u64, false);
        if let Some(name) = &self.own_name
            && let Some(tp) = self.hud.tab.players.iter().find(|p| &p.name == name)
            && let Some((url, sl)) = self.skin_by_uuid.get(&tp.uuid)
        {
            let key = fnv64(key_of_url(url).as_bytes());
            if self.renderer.as_ref().is_some_and(|r| r.has_skin(key)) {
                skin = key;
                slim = *sl;
            }
        }
        // Gentle walk swing while moving (reuse the view-bob phase).
        let moving = self.last_move.0 != 0 || self.last_move.1 != 0;
        let swing = if moving { self.bob_phase.sin() * 0.6 } else { 0.0 };
        // One-shot attack/use arm swing over ~300 ms.
        let attack_swing = match self.hand_swing_start {
            Some(start) => {
                let t = start.elapsed().as_secs_f32() / 0.30;
                if t >= 1.0 { 0.0 } else { (t * std::f32::consts::PI).sin() * 1.4 }
            }
            None => 0.0,
        };
        let main_hand = self
            .hotbar
            .get(self.selected_slot as usize)
            .and_then(|s| s.as_ref())
            .and_then(|i| self.item_icons.uv(&i.item));
        let off_hand = self.offhand.as_ref().and_then(|i| self.item_icons.uv(&i.item));
        // Left-handed players hold the selected item in the left hand (vanilla
        // "Main Hand: Left"); swap the drawn hands.
        let (main_hand, off_hand) = if self.settings.left_handed {
            (off_hand, main_hand)
        } else {
            (main_hand, off_hand)
        };
        // Our own worn armor (from the inventory armor slots the bridge reads).
        let armor = self
            .player
            .as_ref()
            .map(|p| {
                let eq = &p.equipment;
                [
                    eq.head.as_deref().and_then(armor_material),
                    eq.chest.as_deref().and_then(armor_material),
                    eq.legs.as_deref().and_then(armor_material),
                    eq.feet.as_deref().and_then(armor_material),
                ]
            })
            .unwrap_or([None; 4]);
        Some(EntityDraw {
            pos,
            yaw: self.yaw,
            tint: [1.0, 1.0, 1.0],
            kind: EntityDrawKind::Player {
                skin,
                slim,
                swing,
                attack_swing,
                sneaking: self.sneaking,
                skin_layers: self.settings.skin_layer_mask(),
                head_pitch: self.pitch,
                armor,
                main_hand,
                off_hand,
            },
        })
    }

    /// The first-person view model (own hand + held item), shown only in first
    /// person with no overlay up and the HUD visible — exactly like vanilla.
    fn view_model(&mut self) -> Option<crate::render::ViewModel> {
        if self.perspective != 0 || !self.connected || self.hud_hidden || self.hud.overlay_open() {
            self.view_last_item = None; // reset equip so re-showing raises again
            return None;
        }
        // Own skin (Steve = 0 fallback), same lookup as the third-person body.
        let (mut skin, mut slim) = (0u64, false);
        if let Some(name) = &self.own_name
            && let Some(tp) = self.hud.tab.players.iter().find(|p| &p.name == name)
            && let Some((url, sl)) = self.skin_by_uuid.get(&tp.uuid)
        {
            let key = fnv64(key_of_url(url).as_bytes());
            if self.renderer.as_ref().is_some_and(|r| r.has_skin(key)) {
                skin = key;
                slim = *sl;
            }
        }
        // The selected hotbar item is always what the main hand holds; the
        // `left_handed` flag only mirrors which side it's drawn on (handled in
        // the renderer), it does not change which item is shown.
        let held = self.hotbar.get(self.selected_slot as usize).and_then(|s| s.as_ref());
        let item_name = held.map(|i| i.item.clone());
        let item_uv = item_name.as_deref().and_then(|n| self.item_icons.uv(n));
        let item_is_block = item_name.as_deref().is_some_and(|n| self.block_names.contains(n));
        // A real 3D block model for a held block (bedwars: blocks in hand).
        let block_quads = if item_is_block {
            item_name.as_deref().and_then(|n| self.held_block_geometry(n))
        } else {
            None
        };
        // Off-hand item (shield/torch/map), shown in the other hand.
        let off = self.offhand.as_ref();
        let off_hand_uv = off.and_then(|i| self.item_icons.uv(&i.item));
        let off_hand_is_block = off.is_some_and(|i| self.block_names.contains(&i.item));

        // Equip raise when the held item changes.
        if item_name != self.view_last_item {
            self.view_last_item = item_name.clone();
            self.view_equip_start = Instant::now();
        }
        let equip = (self.view_equip_start.elapsed().as_secs_f32() / 0.18).clamp(0.0, 1.0);

        // Swing: a repeating arc while mining, else the one-shot click swing.
        let swing = if self.left_held && self.mining_target.is_some() {
            (self.start.elapsed().as_secs_f32() * 3.0).fract()
        } else {
            match self.hand_swing_start {
                Some(start) => {
                    let t = start.elapsed().as_secs_f32() / 0.30;
                    if t >= 1.0 { 0.0 } else { t }
                }
                None => 0.0,
            }
        };

        let moving = self.last_move.0 != 0 || self.last_move.1 != 0;
        // Item-use raise: ease in over ~150 ms from when using began. Only the
        // main hand raises (eat/drink/bow), so require a held main-hand item —
        // this avoids lifting an empty hand while blocking with an off-hand
        // shield (azalea doesn't expose which hand is using).
        let using = if held.is_some() {
            self.use_start
                .map(|t| (t.elapsed().as_secs_f32() / 0.15).clamp(0.0, 1.0))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        Some(crate::render::ViewModel {
            skin,
            slim,
            item_uv,
            item_is_block,
            block_quads,
            off_hand_uv,
            off_hand_is_block,
            swing,
            equip,
            bob_phase: self.bob_phase,
            bob: if moving { 1.0 } else { 0.0 },
            using,
            use_phase: self.start.elapsed().as_secs_f32(),
            left_handed: self.settings.left_handed,
        })
    }

    /// Baked geometry of a held/dropped block's representative state (see
    /// [`block_geometry`]).
    fn held_block_geometry(&self, name: &str) -> Option<Vec<([f32; 3], [f32; 2])>> {
        block_geometry(&self.store, &self.block_state_by_name, name)
    }

    /// Track the selected hotbar item; when it changes to a new item, arm the
    /// "held item name" popup (vanilla shows it above the hotbar, fading out).
    /// Returns `(spans, alpha)` for the HUD.
    fn item_name_popup(&mut self) -> (Vec<ChatSpan>, f32) {
        let now = Instant::now();
        match self.hotbar.get(self.selected_slot as usize).and_then(|s| s.as_ref()) {
            Some(item) => {
                let key = (self.selected_slot, item.item.clone());
                if self.last_shown_item.as_ref() != Some(&key) {
                    // A custom (anvil/NBT) name wins; else the translated item name.
                    let spans = item
                        .name
                        .clone()
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| vec![ChatSpan::plain(self.lang.item_name(&item.item))]);
                    self.last_shown_item = Some(key);
                    self.item_name_spans = spans;
                    self.item_name_until = Some(now + Duration::from_millis(2500));
                }
            }
            // Empty slot: remember it (so re-selecting an item re-shows) — no popup.
            None => self.last_shown_item = Some((self.selected_slot, String::new())),
        }
        let alpha = match self.item_name_until {
            Some(t) if t > now => ((t - now).as_secs_f32() / 0.5).clamp(0.0, 1.0),
            _ => 0.0,
        };
        (self.item_name_spans.clone(), alpha)
    }

    /// Prune expired potion effects and build the top-right HUD list, lazily
    /// loading each effect's `mob_effect/<name>` icon texture.
    fn active_effect_hud(&mut self) -> Vec<hud::EffectHud> {
        let now = Instant::now();
        self.active_effects.retain(|_, (_, expiry)| expiry.is_none_or(|e| e > now));
        if self.active_effects.is_empty() {
            return Vec::new();
        }
        let mut names: Vec<String> = self.active_effects.keys().cloned().collect();
        names.sort();
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let (amplifier, expiry) = self.active_effects[&name];
            if !self.effect_tex.contains_key(&name)
                && let Ok(img) = self.pack.texture_png(&format!("mob_effect/{name}"))
            {
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                );
                let tex = self.egui_ctx.load_texture(
                    format!("effect-{name}"),
                    color,
                    egui::TextureOptions::NEAREST,
                );
                self.effect_tex.insert(name.clone(), tex);
            }
            out.push(hud::EffectHud {
                icon: self.effect_tex.get(&name).map(|t| t.id()),
                amplifier,
                remaining_secs: expiry.map(|e| e.saturating_duration_since(now).as_secs() as i32),
            });
        }
        out
    }

    /// Prune finished item cooldowns and return `name → remaining fraction`
    /// (1.0 just triggered → 0.0 ready) for the hotbar sweep overlay.
    fn cooldown_fractions(&mut self) -> HashMap<String, f32> {
        let now = Instant::now();
        self.cooldowns.retain(|_, (end, _)| *end > now);
        self.cooldowns
            .iter()
            .map(|(name, (end, total))| {
                let rem = end.saturating_duration_since(now).as_secs_f32();
                (name.clone(), (rem / total.max(0.05)).clamp(0.0, 1.0))
            })
            .collect()
    }

    /// Compute the Move command from held keys; send only on change.
    fn push_move_if_changed(&mut self) {
        // No movement while a text field owns the keyboard, a container is up
        // or the game is paused — same as vanilla.
        let active =
            !self.hud.wants_keyboard() && !self.hud.is_paused() && !self.hud.container_open();
        let (fw, bk, lt, rt, sprint_key, sneak_key) = {
            let kb = &self.settings.keys;
            (
                key_down(&self.keys, &kb.forward),
                key_down(&self.keys, &kb.back),
                key_down(&self.keys, &kb.left),
                key_down(&self.keys, &kb.right),
                key_down(&self.keys, &kb.sprint),
                key_down(&self.keys, &kb.sneak),
            )
        };
        let mut forward = 0i8;
        let mut strafe = 0i8;
        if active {
            forward = fw as i8 - bk as i8;
            strafe = lt as i8 - rt as i8;
        }
        if forward > 0 {
            if self.forward_since.is_none() {
                self.forward_since = Some(Instant::now());
            }
        } else {
            self.forward_since = None;
            self.sprint_latch = false;
        }
        let sprint = forward > 0
            && if self.settings.sprint_toggle { self.sprint_latch } else { sprint_key };
        let mv = (forward, strafe, sprint);
        if mv != self.last_move {
            self.last_move = mv;
            self.send_cmd(Command::Move { forward, strafe, sprint });
        }
        // Sneak state, also change-triggered.
        let sneak = active
            && if self.settings.sneak_toggle { self.sneak_latch } else { sneak_key };
        if sneak != self.sneaking {
            self.sneaking = sneak;
            self.send_cmd(Command::Sneak(sneak));
        }
    }

    /// Auto-jump: pushing forward on the ground but barely moving for a while
    /// → hop. A short Jump pulse, never while the jump key is held.
    fn auto_jump_tick(&mut self) {
        if let Some(until) = self.auto_jump_until {
            if Instant::now() >= until {
                self.auto_jump_until = None;
                self.send_cmd(Command::Jump(false));
            }
            return;
        }
        if !self.settings.auto_jump || !self.connected || self.last_move.0 <= 0 {
            return;
        }
        if self.player.as_ref().is_some_and(|p| p.riding) {
            return; // movement keys steer the vehicle — never auto-jump
        }
        if key_down(&self.keys, &self.settings.keys.jump) {
            return; // player controls jumping
        }
        let pushing = self.forward_since.is_some_and(|t| t.elapsed().as_millis() > 250);
        let on_ground = self.player.as_ref().is_some_and(|p| p.on_ground);
        let speed = self
            .cam
            .as_ref()
            .map_or(f64::MAX, |c| (c.vel[0] * c.vel[0] + c.vel[2] * c.vel[2]).sqrt());
        if pushing && on_ground && speed < 1.0 {
            self.send_cmd(Command::Jump(true));
            self.auto_jump_until = Some(Instant::now() + Duration::from_millis(150));
        }
    }

    // -- per-frame pipeline ------------------------------------------------------

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        self.frame_counter += 1;
        let frame_dt = self.last_frame.elapsed().as_secs_f64().clamp(0.0, 0.25);
        self.last_frame = Instant::now();
        if self.settings_dirty {
            self.apply_settings();
            self.settings_dirty = false;
        }
        self.drain_game_events();
        self.update_discord();
        self.continue_using();
        self.tick_particles(frame_dt as f32);
        self.tick_rain(frame_dt as f32);
        self.pump_meshing();
        self.apply_mouse_look();
        self.push_move_if_changed();
        self.auto_jump_tick();
        self.skins.poll();
        self.upload_skins();
        self.smooth_camera(frame_dt);

        let (Some(window), true) = (self.window.clone(), self.egui_state.is_some()) else {
            return Ok(());
        };

        // --- egui pass -------------------------------------------------------
        // Upload the item-icon atlas to egui once (nearest-filtered, crisp).
        if self.icon_tex.is_none() && !self.item_icons.is_empty() {
            let img = &self.item_icons.image;
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            );
            self.icon_tex = Some(self.egui_ctx.load_texture(
                "item-icons",
                color,
                egui::TextureOptions::NEAREST,
            ));
        }
        let icons = self
            .icon_tex
            .as_ref()
            .map(|t| (t.id(), self.item_icons.clone()));

        let show_tab_list =
            self.tab_held && self.connected && !self.hud.overlay_open();
        // Refresh the debug FPS at most ~3×/second and round it, so it reads
        // as a steady number instead of churning every frame.
        if self.fps_updated.elapsed() >= Duration::from_millis(333) {
            self.fps_display = fps_of(&self.frame_times).round();
            self.fps_updated = Instant::now();
        }

        // View bobbing: a subtle vertical sway while walking (vanilla-style).
        let moving = self.last_move.0 != 0 || self.last_move.1 != 0;
        if self.settings.view_bobbing && moving {
            let step = if self.last_move.2 { 0.42 } else { 0.30 };
            self.bob_phase = (self.bob_phase + step * (frame_dt * 60.0) as f32)
                % std::f32::consts::TAU;
        }
        let bob_y = if self.settings.view_bobbing && moving {
            (self.bob_phase.sin() * 0.045) as f64
        } else {
            0.0
        };
        let (mut cam_pos, mut yaw, mut pitch, mut fov) = if !self.connected {
            // Title screens: slow panorama rotation (position is irrelevant).
            (
                [0.0, 64.0, 0.0],
                (self.start.elapsed().as_secs_f32() * 2.25) % 360.0,
                0.0,
                85.0,
            )
        } else {
            match (&self.cam, &self.player) {
                (Some(c), Some(p)) => (
                    [
                        c.render_pos[0],
                        c.render_pos[1] + p.eye_height as f64 + bob_y,
                        c.render_pos[2],
                    ],
                    self.yaw,
                    self.pitch,
                    self.settings.fov,
                ),
                // Spectator wait: slow orbit above spawn until player data arrives.
                _ => (
                    [8.0, 80.0, 8.0],
                    (self.start.elapsed().as_secs_f32() * 10.0) % 360.0,
                    20.0,
                    self.settings.fov,
                ),
            }
        };
        // Dynamic FOV: a gentle zoom-out while sprinting, scaled by the FOV
        // Effects option (0 = fixed FOV, like vanilla's slider). Vanilla-like:
        // walking never changes the FOV, sprinting targets +10%, and the
        // multiplier eases toward its target (~vanilla's half-way-per-tick)
        // instead of snapping.
        let fov_target = if self.connected && self.last_move.2 {
            1.0 + 0.10 * self.settings.fov_effects
        } else {
            1.0
        };
        let ease = 1.0 - (-frame_dt as f32 * 14.0).exp();
        self.fov_mult += (fov_target - self.fov_mult) * ease;
        if (self.fov_mult - fov_target).abs() < 1e-4 {
            self.fov_mult = fov_target;
        }
        fov *= self.fov_mult;
        // Hold-to-zoom (Optifine-style): narrow the FOV while the zoom key is down.
        let zoom_active = self.connected
            && key_down(&self.keys, &self.settings.keys.zoom)
            && !self.hud.wants_keyboard()
            && !self.hud.is_paused();
        if zoom_active {
            fov = (fov * 0.28).max(5.0);
        }
        // Spyglass: while actively using a spyglass (either hand), zoom the view
        // hard (vanilla's ~0.1 FOV scale) and show the round scope overlay.
        let spyglass_active = self.connected
            && self.player.as_ref().is_some_and(|p| p.using_item)
            && (self
                .hotbar
                .get(self.selected_slot as usize)
                .and_then(|s| s.as_ref())
                .is_some_and(|i| i.item == "spyglass")
                || self.offhand.as_ref().is_some_and(|i| i.item == "spyglass"));
        if spyglass_active {
            fov = (fov * 0.10).max(5.0);
        }
        // Third-person (F5): pull the eye back behind the player (perspective 1)
        // or in front looking back (perspective 2), stopping short of walls.
        if self.connected && self.perspective != 0 && self.player.is_some() {
            let front = self.perspective == 2;
            let vd = camera::view_dir(yaw, pitch);
            let away = if front { vd } else { -vd };
            let away = [away.x as f64, away.y as f64, away.z as f64];
            let dist = self.third_person_distance(cam_pos, away, 4.0);
            cam_pos = [
                cam_pos[0] + away[0] * dist,
                cam_pos[1] + away[1] * dist,
                cam_pos[2] + away[2] * dist,
            ];
            if front {
                yaw = (yaw + 180.0).rem_euclid(360.0);
                pitch = -pitch;
            }
        }
        // Nametags: project each named entity's head to screen space (needs the
        // camera params above, so it must run before the HUD is built).
        let win_size = window.inner_size();
        let aspect = win_size.width.max(1) as f32 / win_size.height.max(1) as f32;
        let nametags = if self.connected {
            self.compute_nametags(cam_pos, yaw, pitch, fov, aspect)
        } else {
            Vec::new()
        };
        let entities_count = self.tracks.len();
        let (item_name, item_name_alpha) = self.item_name_popup();
        let effects = self.active_effect_hud();
        let cooldowns = self.cooldown_fractions();

        let hud_state = HudState {
            fps: self.fps_display,
            pos: self.player.as_ref().map_or([0.0; 3], |p| p.pos),
            yaw: self.yaw,
            pitch: self.pitch,
            health: self.player.as_ref().map_or(0.0, |p| p.health),
            absorption: self.player.as_ref().map_or(0.0, |p| p.absorption),
            food: self.player.as_ref().map_or(0, |p| p.food),
            xp_level: self.player.as_ref().map_or(0, |p| p.xp_level),
            xp_progress: self.player.as_ref().map_or(0.0, |p| p.xp_progress),
            attack_strength: self.player.as_ref().map_or(1.0, |p| p.attack_strength),
            target_in_reach: self.connected && self.crosshair_target_in_reach(),
            air: if self.air_seen {
                self.player.as_ref().map_or(300, |p| p.air)
            } else {
                300
            },
            eyes_in_water: self.player.as_ref().is_some_and(|p| p.eyes_in_water),
            eyes_in_lava: self.player.as_ref().is_some_and(|p| p.eyes_in_lava),
            on_fire: self.player.as_ref().is_some_and(|p| p.on_fire),
            dark_vignette: if self.active_effects.contains_key("blindness") {
                0.92
            } else if self.active_effects.contains_key("darkness") {
                // Vanilla darkness pulses the screen darker in waves.
                let t = self.start.elapsed().as_secs_f32();
                0.30 + 0.28 * (t * 2.2).sin().max(0.0)
            } else {
                0.0
            },
            poisoned: self.active_effects.contains_key("poison"),
            withered: self.active_effects.contains_key("wither"),
            freeze: self.player.as_ref().map_or(0.0, |p| p.freeze),
            pumpkin: self
                .player
                .as_ref()
                .is_some_and(|p| p.equipment.head.as_deref() == Some("carved_pumpkin")),
            spyglass: spyglass_active,
            hotbar: self.hotbar.clone(),
            offhand: self.offhand.clone(),
            cooldowns,
            selected_slot: self.selected_slot,
            item_name,
            item_name_alpha,
            icons,
            effects,
            sections_drawn: self.last_stats.0,
            sections_total: self.last_stats.1,
            mesh_queue: self.in_flight,
            connected: self.connected,
            connecting: (self.bridge.is_some() || self.reconnect_at.is_some())
                && !self.connected,
            connect_attempt: self.connect_attempt,
            disconnect_reason: self.disconnect_reason.clone(),
            menu_time: self.start.elapsed().as_secs_f32(),
            show_tab_list,
            nametags,
            entities_count,
            render_distance: self.settings.render_distance,
            sidebar_title: self.sidebar_title.clone(),
            sidebar_lines: self.sidebar_lines.clone(),
            hud_hidden: self.hud_hidden,
            hurt_flash: self
                .hurt_flash_until
                .and_then(|t| t.checked_duration_since(Instant::now()))
                .map_or(0.0, |d| (d.as_secs_f32() / 0.5).clamp(0.0, 1.0)),
            own_skin: self.own_skin_url(),
            attack_indicator: self.settings.attack_indicator,
            reduced_debug_info: self.settings.reduced_debug_info,
            text_bg_opacity: self.settings.text_background_opacity,
            server_address: self
                .connect_target
                .as_ref()
                .map(|(a, _)| a.clone())
                .unwrap_or_default(),
            session_secs: self.session_start.map_or(0.0, |t| t.elapsed().as_secs_f32()),
        };
        let raw_input = self
            .egui_state
            .as_mut()
            .expect("egui_state present")
            .take_egui_input(&window);
        self.egui_ctx.begin_pass(raw_input);
        let actions = self.hud.run(
            &self.egui_ctx,
            &self.mcui,
            &hud_state,
            &mut self.settings,
            &mut self.skins,
            &self.lang,
        );
        let output = self.egui_ctx.end_pass();
        if let Some(egui_state) = self.egui_state.as_mut() {
            egui_state.handle_platform_output(&window, output.platform_output);
        }
        let egui_frame = EguiFrame {
            textures_delta: output.textures_delta,
            primitives: self.egui_ctx.tessellate(output.shapes, output.pixels_per_point),
            pixels_per_point: output.pixels_per_point,
        };

        // Widget clicks this frame → the vanilla button sound.
        if self.mcui.take_clicks() > 0 {
            self.play_click();
        }

        // --- scene -------------------------------------------------------------
        let fog_end = (self.settings.render_distance.max(2) * 16) as f32;
        // Brightness maps 0.5 → neutral, up → brighter, down → moody.
        let gamma = 0.6 + 0.8 * self.settings.brightness;
        let show_panorama = !self.connected && self.panorama_loaded;
        // Dimension look: skylight-less dimensions ignore the day cycle and
        // use a fixed ambient (sections there carry no sky-light data, which
        // would otherwise render as full daylight) plus their own sky color.
        let (mut sky_color, mut daylight) = if !self.connected {
            ([0.08, 0.09, 0.12], self.daylight) // menu (or panorama override below)
        } else if self.dim_skylight {
            (overworld_sky_color(self.world_time), self.daylight)
        } else if self.dim_ultrawarm {
            ([0.16, 0.04, 0.04], 0.18) // Nether: red haze, dim ambient
        } else {
            ([0.03, 0.03, 0.06], 0.28) // The End: dark purple-ish sky
        };
        // Rain/storm: dim the light and desaturate the sky toward overcast gray.
        if self.connected && self.dim_skylight && self.rain_level > 0.01 {
            let storm = (self.rain_level + self.thunder_level * 0.6).min(1.0);
            daylight *= 1.0 - 0.45 * storm;
            let gray = 0.55 * daylight.max(0.25);
            let k = 0.7 * storm;
            for c in 0..3 {
                sky_color[c] += (gray - sky_color[c]) * k;
            }
        }
        // Targeted-block selection outline (vanilla black box) — exact per-state
        // shape from azalea's generated outline data. This is static block-shape
        // data, not live ECS state, so reading it here doesn't cross the
        // app/bridge boundary in spirit.
        let mut outline: Vec<([f64; 3], [f64; 3])> = Vec::new();
        if self.connected
            && let Some(p) = &self.player
        {
            let eye = [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]];
            let d = camera::view_dir(self.yaw, self.pitch);
            let dir = [d.x as f64, d.y as f64, d.z as f64];
            let table = &self.table;
            if let Some((bpos, _)) = self.mirror.raycast(eye, dir, 5.0, |id| table.is_air(id)) {
                let state = self.mirror.get_block(bpos);
                let base = [bpos.x as f64, bpos.y as f64, bpos.z as f64];
                match azalea::block::BlockState::try_from(state) {
                    Ok(bs) => {
                        use azalea::physics::collision::BlockWithShape;
                        // An empty shape (fluids) legitimately draws nothing.
                        for a in bs.outline_shape().to_aabbs() {
                            outline.push((
                                [base[0] + a.min.x, base[1] + a.min.y, base[2] + a.min.z],
                                [base[0] + a.max.x, base[1] + a.max.y, base[2] + a.max.z],
                            ));
                        }
                    }
                    Err(_) => {
                        outline.push((base, [base[0] + 1.0, base[1] + 1.0, base[2] + 1.0]));
                    }
                }
            }
        }
        // Mining crack overlay: stage from azalea's live mining progress.
        let crack = self.mining_target.and_then(|(bpos, progress)| {
            (progress > 0.0).then(|| {
                (
                    [bpos.x as f64, bpos.y as f64, bpos.z as f64],
                    ((progress * 10.0) as u32).min(9),
                )
            })
        });
        let scene = SceneParams {
            cam_pos,
            yaw,
            pitch,
            fov_deg: fov,
            daylight: (daylight * gamma).clamp(0.05, 1.0),
            fog_start: if self.settings.fog { fog_end * 0.75 } else { fog_end - 1.0 },
            fog_end,
            sky_color: if self.connected || show_panorama {
                if self.connected { sky_color } else { [0.47, 0.65, 1.0] }
            } else {
                [0.08, 0.09, 0.12] // panorama missing: keep the title moody
            },
            panorama: show_panorama,
            outline,
            crack,
            view_model: self.view_model(),
            // Sun/moon/stars/clouds only in the overworld (skylight dimensions).
            // Rain hides the celestial bodies behind the overcast.
            sky: (self.connected && self.dim_skylight).then(|| {
                let mut s = sky_params_of(self.world_time, self.start.elapsed().as_secs_f32());
                let clear = (1.0 - self.rain_level).clamp(0.0, 1.0);
                s.sun_alpha *= clear;
                s.moon_alpha *= clear;
                s.star_brightness *= clear;
                s.glow_color[3] *= clear;
                s
            }),
        };

        let entities = self.entity_draws();
        if let Some(renderer) = &mut self.renderer {
            let stats = renderer.frame(&scene, &entities, Some(egui_frame))?;
            self.last_stats = (stats.sections_drawn, stats.sections_total);
        }

        // --- hud actions ---------------------------------------------------------
        for action in actions {
            match action {
                HudAction::SendChat(msg) => self.send_cmd(Command::Chat(msg)),
                HudAction::ChatClosed => {} // grab restores automatically
                HudAction::TabComplete { id, text } => {
                    self.send_cmd(Command::TabComplete { id, text });
                }
                HudAction::SlotClick { window_id, slot, kind } => {
                    self.send_cmd(Command::ContainerClick { window_id, slot, kind });
                }
                HudAction::SelectTrade { index } => {
                    self.send_cmd(Command::SelectTrade { index });
                }
                HudAction::Connect { address, username } => {
                    self.start_connect(address, username, 1);
                }
                HudAction::SettingsChanged => {
                    self.settings.clamp();
                    self.settings.save();
                    // vsync / fullscreen / GUI scale are applied next frame.
                    self.settings_dirty = true;
                    if self.settings.language != self.lang_code {
                        self.lang_code = self.settings.language.clone();
                        self.lang = Lang::load(
                            &mut self.pack,
                            self.opts.assets_dir.as_deref(),
                            self.opts.asset_index.as_deref(),
                            &self.lang_code,
                        );
                    }
                }
                HudAction::Resume => {} // grab restores automatically
                HudAction::Disconnect => {
                    // Leave immediately and locally. The bridge's own teardown
                    // is best-effort — azalea's exit path can freeze the
                    // schedule loop, and a timed-out connection may already have
                    // stopped ticking — so we must NEVER wait for it to confirm.
                    // Waiting is exactly the "Disconnect button does nothing"
                    // hang. Fire a best-effort close, then drop the connection
                    // and return to the title screen ourselves.
                    self.send_cmd(Command::Disconnect);
                    self.leave_to_title();
                }
                HudAction::BackToMenu => {
                    self.disconnect_reason = None;
                    self.connected = false;
                    self.bridge = None;
                    self.reset_world_state();
                }
                HudAction::Quit => {
                    event_loop.exit();
                }
                HudAction::OpenUrl(url) => {
                    if let Err(e) = open::that(&url) {
                        warn!(url, error = %e, "app: failed to open URL");
                    }
                }
                HudAction::OpenGameFolder => {
                    let dir = GameSettings::config_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    if let Err(e) = open::that(&dir) {
                        warn!(dir = %dir.display(), error = %e, "app: failed to open game folder");
                    }
                }
            }
        }

        // Centralized mouse capture (auto re-grab after chat/menu/container).
        let want = self.desired_grab();
        self.set_grab(want);

        // --- fps ------------------------------------------------------------------
        // Software frame cap: only when vsync is off (present mode is uncapped)
        // and a finite limit is set. Unlimited (max_fps == 0) runs wide open.
        if !self.settings.vsync && self.settings.max_fps > 0 {
            let target = Duration::from_secs_f64(1.0 / self.settings.max_fps as f64);
            let elapsed = self.last_frame_end.elapsed();
            if elapsed < target {
                std::thread::sleep(target - elapsed);
            }
        }
        self.last_frame_end = Instant::now();

        self.frame_times.push_back(Instant::now());
        while self.frame_times.len() > FPS_WINDOW + 1 {
            self.frame_times.pop_front();
        }
        Ok(())
    }

    /// Spawn the bridge for one connect attempt. `attempt` is 1-based and drives
    /// the auto-retry UI. Cold DNS resolvers / SRV flakiness make the first
    /// tries after launch fail; [`drain_game_events`] retries transient failures
    /// automatically up to [`MAX_CONNECT_ATTEMPTS`] before surfacing an error.
    fn start_connect(&mut self, address: String, username: String, attempt: u32) {
        info!(address, username, attempt, "app: connect attempt");
        // Use the account resolved at startup (launcher session / Microsoft).
        // Only offline mode takes the username field.
        let account = match &self.opts.bridge.account {
            AccountConfig::Offline(_) => AccountConfig::Offline(username.clone()),
            other => other.clone(),
        };
        self.connect_target = Some((address.clone(), username));
        self.connect_attempt = attempt;
        self.reconnect_at = None;
        match spawn_bridge(BridgeOptions {
            account,
            address,
            view_distance: self.settings.render_distance.clamp(2, 32) as u8,
        }) {
            Ok(pair) => {
                self.reset_world_state();
                self.disconnect_reason = None;
                self.returning_to_menu = false;
                self.last_activity = Instant::now();
                // Preflight + login should finish well within this.
                self.connect_deadline = Some(Instant::now() + Duration::from_secs(45));
                self.bridge = Some(pair);
            }
            Err(e) => {
                warn!("app: connect failed: {e:#}");
                self.hud.reset_to_title();
                self.connect_target = None;
                self.disconnect_reason = Some(format!("connect failed: {e:#}"));
            }
        }
    }

    /// Schedule an auto-retry of the current connect target after a short
    /// backoff, or surface `reason` if we're out of attempts / it isn't
    /// transient. Returns `true` when a retry was scheduled. `was_connected`
    /// guards against reconnecting into a server that kicked us mid-game.
    fn maybe_retry_connect(&mut self, reason: String, was_connected: bool) -> bool {
        if !was_connected
            && self.connect_target.is_some()
            && self.connect_attempt < MAX_CONNECT_ATTEMPTS
            && is_transient_connect_error(&reason)
        {
            info!(reason, attempt = self.connect_attempt, "app: transient connect failure; retrying");
            self.connect_deadline = None;
            self.reconnect_at = Some(Instant::now() + Duration::from_millis(600));
            self.reset_world_state();
            true
        } else {
            self.connect_target = None;
            self.reconnect_at = None;
            self.disconnect_reason = Some(reason);
            self.reset_world_state();
            false
        }
    }

    /// Clear per-world state (on connect and when returning to the menu).
    fn reset_world_state(&mut self) {
        self.mirror = WorldMirror::new();
        if let Some(r) = &mut self.renderer {
            r.clear_meshes();
        }
        self.player = None;
        self.tracks.clear();
        self.cam = None;
        self.dir_synced = false;
        self.sneak_latch = false;
        self.sprint_latch = false;
        self.auto_jump_until = None;
        self.hotbar = vec![None; 9];
        self.offhand = None;
        self.sidebar_title.clear();
        self.sidebar_lines.clear();
        self.perspective = 0;
        self.hud_hidden = false;
        self.tab_held = false;
        self.particles.clear();
        self.rain_level = 0.0;
        self.thunder_level = 0.0;
        self.rain_drops.clear();
        self.active_effects.clear();
        self.cooldowns.clear();
        self.last_shown_item = None;
        self.item_name_until = None;
        self.item_name_spans.clear();
        self.last_health = -1.0;
        self.hurt_flash_until = None;
        self.use_start = None;
        self.dim_skylight = true;
        self.dim_ultrawarm = false;
        self.mining_target = None;
        self.mining_recent = None;
        self.mine_hit_counter = 0;
        self.pending_place = None;
        self.air_seen = false;
    }

    /// Drop the server connection and return to the title screen *now*, without
    /// waiting on the bridge thread (which can freeze on a dead connection).
    /// Used by the pause-menu Disconnect button. A best-effort `Disconnect`
    /// command should be sent first so the socket closes cleanly when the
    /// bridge is still healthy; dropping `self.bridge` also queues one via the
    /// handle's `Drop`, and closes the event channel so a frozen bridge stops.
    fn leave_to_title(&mut self) {
        self.connected = false;
        self.bridge = None;
        self.disconnect_reason = None;
        self.connect_target = None;
        self.connect_deadline = None;
        self.reconnect_at = None;
        self.returning_to_menu = false;
        self.hud.reset_to_title();
        self.reset_world_state();
    }

    /// Push the current Rich Presence to Discord, re-sending only when it
    /// changed. A raw server IP is deliberately hidden — only a domain name is
    /// shown, so you don't broadcast a friend's server IP on your profile.
    fn update_discord(&mut self) {
        let Some(discord) = &self.discord else { return };
        let version = env!("CARGO_PKG_VERSION");
        let want: Option<crate::discord::Activity> = if !self.settings.discord_rpc {
            None
        } else if self.connected {
            let host = self
                .connect_target
                .as_ref()
                .map(|(addr, _)| crate::discord::server_host(addr))
                .filter(|h| !crate::discord::is_raw_ip(h));
            let details = match host {
                Some(h) => format!("Spielt auf {h}"),
                None => "Spielt auf einem Server".to_string(),
            };
            Some(crate::discord::Activity {
                details: Some(details),
                state: self.own_name.clone(),
                large_image: Some(crate::discord::large_image()),
                large_text: Some(format!("DolphinClient {version}")),
                start_unix: self.session_unix_start,
            })
        } else {
            Some(crate::discord::Activity {
                details: Some("Im Hauptmenü".to_string()),
                state: None,
                large_image: Some(crate::discord::large_image()),
                large_text: Some(format!("DolphinClient {version}")),
                start_unix: None,
            })
        };
        if want != self.discord_state {
            discord.set(want.clone());
            self.discord_state = want;
        }
    }

    /// The listener (ear) position for sound attenuation — the player's eyes,
    /// or the origin before the first player snapshot.
    fn listener_pos(&self) -> [f64; 3] {
        match &self.player {
            Some(p) => [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]],
            None => [0.0, 0.0, 0.0],
        }
    }

    /// A block changed under us: synthesize the vanilla sounds the server
    /// never sends for our own actions (break confirmed by the mined block
    /// turning to air, place confirmed by the predicted position filling).
    fn on_block_changed(&mut self, pos: BlockPos, prev: StateId, state: StateId) {
        let now = Instant::now();
        let became_air = self.table.is_air(state);
        let was_air = self.table.is_air(prev);
        if became_air
            && !was_air
            && let Some((mpos, at)) = self.mining_recent
            && mpos == pos
            && now.duration_since(at) < Duration::from_millis(1000)
        {
            self.play_block_sound("break", prev, pos, 1.0, 0.8);
            self.spawn_block_break_particles(pos);
            self.mining_recent = None;
        }
        if !became_air
            && let Some((a, b, at)) = self.pending_place
            && (a == pos || b == pos)
            && now.duration_since(at) < Duration::from_millis(400)
        {
            self.play_block_sound("place", state, pos, 1.0, 0.8);
            self.pending_place = None;
        }
    }

    /// Play `block.<group>.<verb>` positionally at a block. Unknown groups
    /// fall back to stone so a wrong mapping degrades to a plausible sound.
    fn play_block_sound(&self, verb: &str, state: StateId, pos: BlockPos, volume: f32, pitch: f32) {
        let gain = self.settings.category_volume(crate::settings::SoundCategory::Blocks);
        if gain <= 0.0 {
            return;
        }
        let Some(audio) = &self.audio else { return };
        let group = self
            .table
            .entry(state)
            .map(|e| blocksound::group(&e.short_name))
            .unwrap_or("stone");
        let mut name = format!("block.{group}.{verb}");
        if !audio.has(&name) {
            name = format!("block.stone.{verb}");
        }
        let center = [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5];
        let e = self.listener_pos();
        let (dx, dy, dz) = (center[0] - e[0], center[1] - e[1], center[2] - e[2]);
        let dist = ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
        audio.play_positional(&name, gain, volume, pitch, dist, audio.local_seed());
    }

    /// A quick gray-brown puff where a block broke (vanilla shows textured
    /// chunks; a neutral puff reads the same at gameplay distance).
    fn spawn_block_break_particles(&mut self, pos: BlockPos) {
        let center = [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5];
        self.spawn_particles(center, [0.55, 0.50, 0.45], 0.12, 16, [0.35, 0.35, 0.35], 0.15, 5.0);
    }

    /// Play the vanilla button-click sound at master volume (menu feedback).
    fn play_click(&self) {
        if let Some(audio) = &self.audio {
            let g = self.settings.master_volume.clamp(0.0, 1.0);
            if g > 0.0 {
                audio.play_ui("ui.button.click", g);
            }
        }
    }

    /// Apply window/renderer-affecting settings (vsync, fullscreen, GUI scale).
    fn apply_settings(&mut self) {
        self.settings.clamp();
        if let Some(r) = &mut self.renderer {
            r.set_vsync(self.settings.vsync);
        }
        if let Some(w) = &self.window {
            let want = if self.settings.fullscreen {
                Some(winit::window::Fullscreen::Borderless(None))
            } else {
                None
            };
            w.set_fullscreen(want);
            let os_scale = w.scale_factor() as f32;
            let zoom = if self.settings.gui_scale == 0 {
                1.0
            } else {
                (self.settings.gui_scale as f32 / os_scale.max(1.0)).clamp(0.5, 4.0)
            };
            self.egui_ctx.set_zoom_factor(zoom);
        }
    }

    /// Overlay a downloaded server resource pack onto the asset pack and re-bake
    /// models/atlas/item-icons live so its textures actually take effect. This
    /// hitches once (~half a second) but only when a server pushes a pack.
    fn apply_server_resource_pack(&mut self, path: PathBuf) {
        info!(path = %path.display(), "app: applying server resource pack");
        if let Err(e) = self.pack.add_overlay_zip(&path) {
            warn!("app: could not open server resource pack: {e:#}");
            return;
        }
        let (store, atlas) = match BakedModelStore::bake_all(&mut self.pack, &self.table) {
            Ok(v) => v,
            Err(e) => {
                warn!("app: re-bake after resource pack failed: {e:#}");
                return;
            }
        };
        let item_icons = ItemIcons::bake(&mut self.pack, &self.table, &store, &atlas);
        self.store = Arc::new(store);
        self.atlas = atlas;
        self.item_icons = Arc::new(item_icons);
        self.icon_tex = None; // re-upload the egui item atlas next frame
        if let Some(r) = &mut self.renderer {
            r.set_atlas(&self.atlas);
            r.set_item_atlas(&self.item_icons.image);
            r.clear_meshes();
        }
        // Re-mesh every loaded section against the new atlas.
        self.mirror.mark_all_dirty();
        self.hud.push_chat(
            vec![ChatSpan::plain("Server-Resource-Pack geladen.")],
            true,
        );
    }

    fn drain_game_events(&mut self) {
        // Auto-retry backoff: re-spawn the bridge once the delay elapses.
        if let Some(at) = self.reconnect_at
            && Instant::now() >= at
        {
            self.reconnect_at = None;
            if let Some((address, username)) = self.connect_target.clone() {
                let attempt = self.connect_attempt + 1;
                self.start_connect(address, username, attempt);
            }
        }

        // Backstop: a connect attempt that never produced any event (azalea
        // can hang silently, e.g. after a failed session-server auth).
        if !self.connected
            && let Some(deadline) = self.connect_deadline
            && Instant::now() > deadline
        {
            warn!("app: connect timed out without any bridge event");
            self.connect_deadline = None;
            self.bridge = None; // dropping the handle disconnects
            self.maybe_retry_connect(
                "Zeitüberschreitung beim Verbinden — der Server hat den Login nicht \
                 abgeschlossen. Bitte erneut versuchen; falls es bleibt, Launcher neu starten."
                    .to_string(),
                false,
            );
        }
        // Watchdog: in-game, but the bridge has gone completely silent (no
        // player snapshots, no packets) for too long → the connection is dead
        // and possibly the bridge thread is frozen. Leave locally so the user
        // isn't stuck on a frozen world with an unresponsive menu (the exact
        // "you time out and nothing happens" report). Show it as an error so
        // they know why, and don't auto-reconnect into a server that dropped us.
        if self.connected
            && self.bridge.is_some()
            && self.last_activity.elapsed() > CONNECTION_WATCHDOG
        {
            warn!(?CONNECTION_WATCHDOG, "app: connection watchdog fired (bridge went silent); leaving");
            self.connected = false;
            self.bridge = None;
            self.connect_target = None;
            self.connect_deadline = None;
            self.reconnect_at = None;
            self.returning_to_menu = false;
            // Clean up any open pause menu/container/chat so dismissing the
            // timeout error lands on a tidy title screen.
            self.hud.reset_to_title();
            self.disconnect_reason =
                Some("Verbindung zum Server unterbrochen (Zeitüberschreitung).".into());
            self.reset_world_state();
            return;
        }

        let Some((_, rx)) = &self.bridge else { return };
        let mut events = Vec::new();
        let mut channel_dead = false;
        loop {
            match rx.try_recv() {
                Ok(ev) => events.push(ev),
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    channel_dead = true;
                    break;
                }
            }
        }
        // Any event at all means the bridge (and connection) is alive — reset
        // the watchdog clock.
        if !events.is_empty() {
            self.last_activity = Instant::now();
        }
        for ev in events {
            // The changed block's previous state, captured before the mirror
            // applies the update (own break/place sounds need it).
            let prev_state = match &ev {
                GameEvent::BlockChanged { pos, .. } => Some(self.mirror.get_block(*pos)),
                _ => None,
            };
            self.mirror.apply(&ev);
            match ev {
                GameEvent::BlockChanged { pos, state } => {
                    self.on_block_changed(pos, prev_state.unwrap_or(0), state);
                }
                GameEvent::BlockBreakEffect { pos, state } => {
                    // Another player's (or the server's) block break nearby.
                    self.play_block_sound("break", state, pos, 1.0, 0.8);
                    self.spawn_block_break_particles(pos);
                }
                GameEvent::Biomes(infos) => {
                    // Build per-biome grass/foliage/water tints and re-mesh the
                    // whole world so the new colors apply.
                    let tints = crate::world::biome::build_biome_tints(
                        &infos,
                        self.grass_colormap.as_ref(),
                        self.foliage_colormap.as_ref(),
                    );
                    info!(biomes = infos.len(), "app: biome tint table built");
                    self.biome_tints = Arc::new(tints);
                    self.mirror.mark_all_dirty();
                }
                GameEvent::Connected { username } => {
                    info!(username, "app: connected");
                    self.connected = true;
                    self.session_start = Some(Instant::now());
                    self.session_unix_start = Some(unix_now());
                    self.connect_deadline = None;
                    self.disconnect_reason = None;
                    self.own_name = Some(username.clone());
                    self.hud
                        .push_chat(vec![ChatSpan::plain(format!("Connected as {username}"))], true);
                }
                GameEvent::Disconnected { reason } => {
                    warn!(reason, "app: disconnected");
                    let was_connected = self.connected;
                    self.connected = false;
                    self.connect_deadline = None;
                    self.bridge = None;
                    // A user-requested disconnect (pause menu) returns to the
                    // title screen; a transient first-attempt failure is retried
                    // silently; a kick/error shows the disconnect overlay.
                    if self.returning_to_menu {
                        self.returning_to_menu = false;
                        self.disconnect_reason = None;
                        self.connect_target = None;
                        self.reconnect_at = None;
                        self.hud.reset_to_title();
                        self.reset_world_state();
                    } else {
                        self.maybe_retry_connect(reason, was_connected);
                    }
                    return; // bridge is gone; stop draining
                }
                GameEvent::Respawn { dimension, has_skylight, ultrawarm } => {
                    info!(dimension, has_skylight, ultrawarm, "app: dimension change / respawn");
                    // azalea swapped its world — drop ours and re-render from
                    // the fresh chunk stream. Deliberately NOT reset_world_state:
                    // hotbar, health, chat and scoreboard survive a dimension
                    // change like in vanilla.
                    self.mirror = WorldMirror::new();
                    if let Some(r) = &mut self.renderer {
                        r.clear_meshes();
                    }
                    self.particles.clear();
                    self.tracks.clear();
                    // azalea resets the player position to (0,0,0) until the
                    // server's teleport arrives; dropping player/cam keeps
                    // unload_far and the camera from acting on that stale
                    // center (it purged freshly streamed chunks — the
                    // "invisible ground after respawn" bug).
                    self.player = None;
                    self.cam = None;
                    self.dir_synced = false;
                    self.dim_skylight = has_skylight;
                    self.dim_ultrawarm = ultrawarm;
                }
                GameEvent::Chat { spans, system } => self.hud.push_chat(spans, system),
                GameEvent::PlayerState(p) => {
                    if !self.dir_synced {
                        self.yaw = p.yaw;
                        self.pitch = clamp_pitch(p.pitch);
                        self.dir_synced = true;
                    }
                    // Took damage: red screen flash + hurt sound + a small burst
                    // of red particles at the eyes (vanilla-style feedback). The
                    // `>= 0.0` guard skips the first snapshot so spawning with
                    // partial health doesn't read as a hit.
                    if self.last_health >= 0.0 && p.health > 0.0 && p.health < self.last_health - 0.01
                    {
                        // The red flash + hit particles are the "Damage Tilt"
                        // feedback. The hurt SOUND comes from the server's
                        // SoundEntity packet (handled below) like vanilla —
                        // playing it here too would double it.
                        if self.settings.damage_tilt {
                            self.hurt_flash_until =
                                Some(Instant::now() + Duration::from_millis(500));
                            let eye = [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]];
                            self.spawn_particles(eye, [0.80, 0.10, 0.10], 0.16, 8, [0.3, 0.3, 0.3], 0.25, 2.0);
                        }
                    }
                    self.last_health = p.health;
                    if p.air > 0 {
                        self.air_seen = true;
                    }
                    // Mining state → crack overlay + periodic hit sounds
                    // (vanilla plays block.<group>.hit every 4th mine tick).
                    match p.mining {
                        Some((pos, progress)) => {
                            self.mining_recent = Some((pos, Instant::now()));
                            if progress > 0.0 {
                                self.mine_hit_counter += 1;
                                if self.mine_hit_counter % 4 == 1 {
                                    let state = self.mirror.get_block(pos);
                                    self.play_block_sound("hit", state, pos, 0.25, 0.5);
                                }
                            }
                            self.mining_target = Some((pos, progress));
                        }
                        None => {
                            self.mining_target = None;
                            self.mine_hit_counter = 0;
                        }
                    }
                    // Item-use pose: latch the start instant so the first-person
                    // hand eases up to the mouth (eat/drink/bow/shield).
                    if p.using_item {
                        self.use_start.get_or_insert_with(Instant::now);
                    } else {
                        self.use_start = None;
                    }
                    self.on_player_snapshot(&p);
                    self.player = Some(*p);
                }
                GameEvent::Entities(list) => {
                    let now = Instant::now();
                    let mut seen = HashSet::with_capacity(list.len());
                    for snap in &list {
                        seen.insert(snap.id);
                        // Learn the skin from the entity's own profile (server
                        // NPCs never appear in the tab list). The tab list still
                        // wins if it already has an entry for this uuid.
                        if let (Some(uuid), Some(url)) = (&snap.uuid, &snap.skin_url) {
                            self.skin_by_uuid
                                .entry(uuid.clone())
                                .or_insert_with(|| (url.clone(), snap.skin_slim));
                        }
                        match self.tracks.get_mut(&snap.id) {
                            Some(track) => track.push(snap.clone(), now),
                            None => {
                                self.tracks.insert(snap.id, EntityTrack::new(snap.clone(), now));
                            }
                        }
                    }
                    self.tracks.retain(|id, _| seen.contains(id));
                }
                GameEvent::Hotbar { slots, offhand, selected } => {
                    self.hotbar = slots.to_vec();
                    self.offhand = offhand;
                    self.selected_slot = selected;
                }
                GameEvent::Scoreboard { title, lines } => {
                    self.sidebar_title = title;
                    self.sidebar_lines = lines;
                }
                GameEvent::TimeOfDay { time_of_day } => {
                    self.daylight = daylight_factor(time_of_day);
                    self.world_time = time_of_day;
                }
                GameEvent::Weather { rain, thunder } => {
                    self.rain_level = rain;
                    self.thunder_level = thunder;
                    if rain <= 0.01 {
                        self.rain_drops.clear();
                    }
                }
                GameEvent::EffectUpdate { name, amplifier, duration_ticks } => {
                    let expiry = (duration_ticks >= 0).then(|| {
                        Instant::now() + Duration::from_secs_f32(duration_ticks as f32 / 20.0)
                    });
                    self.active_effects.insert(name, (amplifier, expiry));
                }
                GameEvent::EffectRemove { name } => {
                    self.active_effects.remove(&name);
                }
                GameEvent::Cooldown { name, duration_ticks } => {
                    if duration_ticks == 0 {
                        self.cooldowns.remove(&name);
                    } else {
                        let secs = duration_ticks as f32 / 20.0;
                        self.cooldowns.insert(
                            name,
                            (Instant::now() + Duration::from_secs_f32(secs), secs),
                        );
                    }
                }
                GameEvent::Sound { name, category, pos, volume, pitch, seed } => {
                    let gain = self.settings.category_volume(category);
                    if gain > 0.0 {
                        if let Some(audio) = &self.audio {
                            let distance = match pos {
                                Some(p) => {
                                    let e = self.listener_pos();
                                    let (dx, dy, dz) = (p[0] - e[0], p[1] - e[1], p[2] - e[2]);
                                    ((dx * dx + dy * dy + dz * dz) as f32).sqrt()
                                }
                                None => 0.0,
                            };
                            audio.play_positional(&name, gain, volume, pitch, distance, seed);
                        }
                        if self.settings.subtitles
                            && let Some(text) = self.lang.get(&format!("subtitles.{name}"))
                        {
                            self.hud.push_subtitle(text.to_string());
                        }
                    }
                }
                GameEvent::TabList(players) => {
                    for p in &players {
                        if let Some(url) = &p.skin_url {
                            self.skin_by_uuid
                                .insert(p.uuid.clone(), (url.clone(), p.skin_slim));
                        }
                    }
                    self.hud.tab.players = players;
                }
                GameEvent::TabHeaderFooter { header, footer } => {
                    self.hud.tab.header = header;
                    self.hud.tab.footer = footer;
                }
                GameEvent::TabSuggestions { id, start, length, entries } => {
                    self.hud.chat.on_suggestions(id, start, length, entries);
                }
                GameEvent::ContainerOpened { id, kind, title, slots } => {
                    self.hud.container_opened(id, kind, title, slots);
                }
                GameEvent::ContainerContent { id, slots, carried } => {
                    self.hud.container_content(id, slots, carried);
                }
                GameEvent::ContainerClosed { id } => {
                    self.hud.container_closed(id);
                }
                GameEvent::MerchantOffers { container_id, offers } => {
                    self.hud.merchant_offers(container_id, offers);
                }
                GameEvent::EntityHurt { id } => {
                    if let Some(track) = self.tracks.get_mut(&id) {
                        // Vanilla hurtTime is 10 ticks = 500 ms.
                        track.hurt_until = Some(Instant::now() + Duration::from_millis(500));
                    }
                }
                GameEvent::EntitySound { id, name, category, volume, pitch, seed } => {
                    let gain = self.settings.category_volume(category);
                    if gain > 0.0 {
                        if let Some(audio) = &self.audio {
                            // Sound follows the entity: use its tracked render
                            // position; an unknown id (the local player — never
                            // tracked) plays at the ear.
                            let distance = match self.tracks.get(&id) {
                                Some(t) => {
                                    let p = t.snap.pos;
                                    let e = self.listener_pos();
                                    let (dx, dy, dz) =
                                        (p[0] - e[0], p[1] - e[1], p[2] - e[2]);
                                    ((dx * dx + dy * dy + dz * dz) as f32).sqrt()
                                }
                                None => 0.0,
                            };
                            audio.play_positional(&name, gain, volume, pitch, distance, seed);
                        }
                        if self.settings.subtitles
                            && let Some(text) = self.lang.get(&format!("subtitles.{name}"))
                        {
                            self.hud.push_subtitle(text.to_string());
                        }
                    }
                }
                GameEvent::EntitySwing { id } => {
                    if let Some(track) = self.tracks.get_mut(&id) {
                        track.swing_start = Some(Instant::now());
                    }
                }
                GameEvent::Particles { pos, color, size, count, spread, speed, gravity } => {
                    self.spawn_particles(pos, color, size, count, spread, speed, gravity);
                }
                GameEvent::ResourcePackReady { path } => {
                    self.apply_server_resource_pack(path);
                }
                // World events (Section/BlockChanged/ChunkUnloaded) were fully
                // handled by mirror.apply above.
                _ => {}
            }
        }
        if channel_dead && self.connected {
            warn!("app: bridge channel closed without a Disconnected event");
            self.connected = false;
            self.disconnect_reason = Some("connection lost (bridge thread died)".into());
            self.bridge = None;
        }
    }

    /// Feed a 20 Hz player snapshot into the camera smoother.
    fn on_player_snapshot(&mut self, p: &PlayerSnapshot) {
        let now = Instant::now();
        match &mut self.cam {
            Some(c) => {
                let dt = now.duration_since(c.snap_t).as_secs_f64();
                let dx = p.pos[0] - c.snap_pos[0];
                let dy = p.pos[1] - c.snap_pos[1];
                let dz = p.pos[2] - c.snap_pos[2];
                if dx * dx + dy * dy + dz * dz > 16.0 * 16.0 {
                    // Teleport: cut, don't glide.
                    c.vel = [0.0; 3];
                    c.render_pos = p.pos;
                } else if (0.005..0.5).contains(&dt) {
                    c.vel = [dx / dt, dy / dt, dz / dt];
                } else {
                    // Odd snapshot spacing: fall back to the physics velocity
                    // (blocks/tick → blocks/second).
                    c.vel = [p.velocity[0] * 20.0, p.velocity[1] * 20.0, p.velocity[2] * 20.0];
                }
                c.snap_pos = p.pos;
                c.snap_t = now;
            }
            None => {
                self.cam = Some(CamTrack {
                    snap_pos: p.pos,
                    snap_t: now,
                    vel: [0.0; 3],
                    render_pos: p.pos,
                });
            }
        }
    }

    /// Per-frame camera smoothing: chase the velocity-extrapolated position.
    fn smooth_camera(&mut self, frame_dt: f64) {
        let Some(c) = &mut self.cam else { return };
        let ahead = c.snap_t.elapsed().as_secs_f64().min(0.15);
        let predicted = [
            c.snap_pos[0] + c.vel[0] * ahead,
            c.snap_pos[1] + c.vel[1] * ahead,
            c.snap_pos[2] + c.vel[2] * ahead,
        ];
        let dx = predicted[0] - c.render_pos[0];
        let dy = predicted[1] - c.render_pos[1];
        let dz = predicted[2] - c.render_pos[2];
        if dx * dx + dy * dy + dz * dz > 16.0 {
            c.render_pos = predicted; // way off (teleport/lag spike): snap
            return;
        }
        // Exponential chase, ~50 ms time constant: fast but never steppy.
        let a = 1.0 - (-frame_dt / 0.05).exp();
        c.render_pos[0] += dx * a;
        c.render_pos[1] += dy * a;
        c.render_pos[2] += dz * a;
    }

    /// Request/download/upload skins for the players currently around.
    fn upload_skins(&mut self) {
        // Our own skin URL (bridge never sends our own entity, so it isn't in
        // `tracks`) — resolved before borrowing the renderer to keep borrows
        // disjoint. Drives third-person (F5) and the inventory paper-doll.
        let own_url = self.own_skin_url().map(|(u, _)| u);
        let Some(renderer) = &mut self.renderer else { return };
        let mut upload = |skins: &mut SkinManager, url: &str| {
            skins.request(url);
            let key = fnv64(key_of_url(url).as_bytes());
            if !renderer.has_skin(key)
                && let Some(img) = skins.skin(url)
            {
                renderer.ensure_skin(key, &img);
            }
        };
        for track in self.tracks.values() {
            let Some(uuid) = &track.snap.uuid else { continue };
            let Some((url, _)) = self.skin_by_uuid.get(uuid) else { continue };
            upload(&mut self.skins, url);
        }
        if let Some(url) = &own_url {
            upload(&mut self.skins, url);
        }
    }

    /// Our own skin `(url, slim)`, looked up in the tab list by our username.
    /// `None` until the tab list arrives (or in offline mode with no skin).
    fn own_skin_url(&self) -> Option<(String, bool)> {
        let name = self.own_name.as_ref()?;
        let tp = self.hud.tab.players.iter().find(|p| &p.name == name)?;
        self.skin_by_uuid.get(&tp.uuid).cloned()
    }

    fn pump_meshing(&mut self) {
        let center = self
            .player
            .as_ref()
            .map_or([8.0, 80.0, 8.0], |p| [p.pos[0], p.pos[1], p.pos[2]]);

        // Periodically drop sections beyond render distance.
        if self.frame_counter.is_multiple_of(32)
            && let Some(p) = &self.player
        {
            let cc = ChunkPos { x: (p.pos[0].floor() as i32) >> 4, z: (p.pos[2].floor() as i32) >> 4 };
            for pos in self.mirror.unload_far(cc, self.settings.render_distance + 2) {
                if let Some(r) = &mut self.renderer {
                    r.remove_mesh(pos);
                }
            }
        }

        // Schedule nearest-first, bounded per frame.
        for pos in self.mirror.take_dirty(center, MESH_BUDGET_PER_FRAME) {
            if let Some(snap) = self.mirror.snapshot27(pos) {
                let store = self.store.clone();
                let table = self.table.clone();
                let biome_tints = self.biome_tints.clone();
                let tx = self.mesh_tx.clone();
                self.in_flight += 1;
                rayon::spawn(move || {
                    let mesh = mesh_section(&snap, &store, &table, &biome_tints);
                    let _ = tx.send((pos, mesh));
                });
            }
        }

        // Upload finished meshes (bounded per frame to keep frame time stable).
        for _ in 0..MESH_BUDGET_PER_FRAME {
            let Ok((_pos, mesh)) = self.mesh_rx.try_recv() else { break };
            self.in_flight = self.in_flight.saturating_sub(1);
            if let Some(r) = &mut self.renderer {
                r.upload_mesh(mesh);
            }
        }
        for pos in self.mirror.take_removed() {
            if let Some(r) = &mut self.renderer {
                r.remove_mesh(pos);
            }
        }
    }

    fn apply_mouse_look(&mut self) {
        let (dx, dy) = std::mem::take(&mut self.pending_mouse);
        if dx != 0.0 || dy != 0.0 {
            let sens = self.settings.sensitivity();
            let dy = if self.settings.invert_mouse { -dy } else { dy };
            self.yaw = (self.yaw + dx as f32 * sens).rem_euclid(360.0);
            self.pitch = clamp_pitch(self.pitch + dy as f32 * sens);
        }
        if self.connected && self.dir_synced {
            let dir = (self.yaw, self.pitch);
            if self.last_sent_dir != Some(dir) {
                self.last_sent_dir = Some(dir);
                self.send_cmd(Command::SetDirection { yaw: self.yaw, pitch: self.pitch });
            }
        }
    }

    /// Interpolated draw list: skinned players, boxes for everything else.
    /// Project each named entity's nametag to normalized device coords for the
    /// HUD to draw over the 3D scene. Camera-relative to match the renderer:
    /// world positions are offset by `cam_pos` before projection.
    fn compute_nametags(
        &self,
        cam_pos: [f64; 3],
        yaw: f32,
        pitch: f32,
        fov: f32,
        aspect: f32,
    ) -> Vec<NameTag> {
        use glam::Vec4;
        let now = Instant::now();
        let render_t = now.checked_sub(ENTITY_LERP_DELAY).unwrap_or(now);
        let vp = camera::view_proj(yaw, pitch, fov, aspect, 512.0);
        let mut tags = Vec::new();
        for track in self.tracks.values() {
            let snap = &track.snap;
            let Some(name) = &snap.name else { continue };
            if name.is_empty() {
                continue;
            }
            // Belt-and-braces: never tag the local player (bridge already skips it).
            if snap.is_player && snap.name == self.own_name {
                continue;
            }
            let (pos, _, _) = track.sample(render_t);
            // The tag floats just above the entity's head.
            let head = [
                (pos[0] - cam_pos[0]) as f32,
                (pos[1] + snap.height as f64 + 0.5 - cam_pos[1]) as f32,
                (pos[2] - cam_pos[2]) as f32,
            ];
            let dist = (head[0] * head[0] + head[1] * head[1] + head[2] * head[2]).sqrt();
            // Vanilla range: 64 blocks, 32 for sneaking entities.
            let max_dist = if snap.sneaking { 32.0 } else { 64.0 };
            if dist > max_dist {
                continue;
            }
            let clip = vp * Vec4::new(head[0], head[1], head[2], 1.0);
            if clip.w <= 0.05 {
                continue; // behind the camera
            }
            let ndc = [clip.x / clip.w, clip.y / clip.w];
            if !(-1.2..=1.2).contains(&ndc[0]) || !(-1.2..=1.2).contains(&ndc[1]) {
                continue; // off-screen (small margin so edge tags don't pop hard)
            }
            // Styled spans (team colors / custom-name formatting); fall back to
            // the plain name so a tag always renders.
            let spans = snap
                .name_spans
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| vec![crate::bridge::events::ChatSpan::plain(name.clone())]);
            // Perspective size like vanilla's world-space billboard: one text
            // line is 8 px · 0.025 blocks/px = 0.2 blocks tall, so its screen
            // height is that over the frustum height at `dist` — expressed
            // here as a fraction of the viewport height. Tags shrink with
            // distance instead of painting full-size across the screen.
            let scale = 0.2 / (2.0 * dist * (fov.to_radians() * 0.5).tan());
            tags.push(NameTag { ndc, dist, scale, spans });
        }
        // Far tags first so nearer ones paint on top.
        tags.sort_by(|a, b| b.dist.total_cmp(&a.dist));
        tags
    }

    fn entity_draws(&mut self) -> Vec<EntityDraw> {
        let now = Instant::now();
        let render_t = now.checked_sub(ENTITY_LERP_DELAY).unwrap_or(now);
        let renderer = self.renderer.as_ref();
        // Dropped items spin around Y like vanilla.
        let spin = (self.start.elapsed().as_secs_f32() * 60.0) % 360.0;
        let mut out = Vec::with_capacity(self.tracks.len());
        for track in self.tracks.values_mut() {
            let snap = &track.snap;
            // The bridge already skips the local player; belt-and-braces by name.
            if snap.is_player && snap.name.is_some() && snap.name == self.own_name {
                continue;
            }
            // Invisible entities (armor stands used as holograms, invisibility
            // potion) draw no body — vanilla shows nothing but the nametag, which
            // is computed separately, so a hidden entity keeps its floating name.
            if snap.invisible {
                continue;
            }
            let (pos, yaw, pitch) = track.sample(render_t);

            // Walk cycle from actual rendered movement (players + humanoid mobs).
            if let Some((lt, lp)) = track.last_render {
                let dt = now.duration_since(lt).as_secs_f32().max(1e-3);
                let dist = (((pos[0] - lp[0]).powi(2) + (pos[2] - lp[2]).powi(2)) as f32).sqrt();
                let target = (dist / dt / 3.5).clamp(0.0, 1.0);
                track.amp += (target - track.amp) * (dt * 8.0).min(1.0);
                track.phase = (track.phase + dist * 2.6) % std::f32::consts::TAU;
            }
            track.last_render = Some((now, pos));
            // Sprinting widens the limb swing, like vanilla's run animation.
            let swing_gain = if snap.sprinting { 1.35 } else { 1.0 };
            let swing = track.phase.sin() * track.amp * 0.8 * swing_gain;
            // One-shot attack/mine arm swing: a single forward sweep over ~300 ms.
            let attack_swing = match track.swing_start {
                Some(start) => {
                    let t = now.duration_since(start).as_secs_f32() / 0.30;
                    if t >= 1.0 {
                        track.swing_start = None;
                        0.0
                    } else {
                        (t * std::f32::consts::PI).sin() * 1.4
                    }
                }
                None => 0.0,
            };
            // Damage flash: tint the whole model red for a short window.
            let tint = if track.hurt_until.is_some_and(|t| now < t) {
                [1.0, 0.45, 0.45]
            } else {
                [1.0, 1.0, 1.0]
            };

            // --- players ------------------------------------------------------
            if snap.is_player {
                let mut skin = 0u64;
                let mut slim = false;
                if let Some(uuid) = &snap.uuid
                    && let Some((url, sl)) = self.skin_by_uuid.get(uuid)
                {
                    slim = *sl;
                    let key = fnv64(key_of_url(url).as_bytes());
                    if renderer.is_some_and(|r| r.has_skin(key)) {
                        skin = key;
                    }
                }
                let eq = &snap.equipment;
                let armor = [
                    eq.head.as_deref().and_then(armor_material),
                    eq.chest.as_deref().and_then(armor_material),
                    eq.legs.as_deref().and_then(armor_material),
                    eq.feet.as_deref().and_then(armor_material),
                ];
                let main_hand = eq.main_hand.as_deref().and_then(|n| self.item_icons.uv(n));
                let off_hand = eq.off_hand.as_deref().and_then(|n| self.item_icons.uv(n));
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    kind: EntityDrawKind::Player {
                        skin, slim, swing, attack_swing, sneaking: snap.sneaking, skin_layers: 0xFF,
                        head_pitch: pitch, armor, main_hand, off_hand,
                    },
                });
                continue;
            }

            // --- dropped items: their real icon, spinning + bobbing -----------
            if snap.kind == "item" {
                let item = snap.item.as_deref();
                // Vanilla dropped items bob up and down; phase per entity id so a
                // pile doesn't bob in lockstep.
                let bob = (self.start.elapsed().as_secs_f32() * 1.8 + snap.id as f32 * 0.7).sin()
                    as f64
                    * 0.06;
                let pos = [pos[0], pos[1] + 0.1 + bob, pos[2]];
                // Dropped blocks spin as a real 3D cube (vanilla); flat items keep
                // their sprite.
                let block_quads = item
                    .filter(|n| self.block_names.contains(*n))
                    .and_then(|n| block_geometry(&self.store, &self.block_state_by_name, n));
                if let Some(quads) = block_quads {
                    out.push(EntityDraw { pos, yaw: spin, tint, kind: EntityDrawKind::ItemBlock { quads } });
                } else if let Some(uv) = item.and_then(|n| self.item_icons.uv(n)) {
                    out.push(EntityDraw { pos, yaw: spin, tint, kind: EntityDrawKind::Item { uv } });
                } else {
                    out.push(EntityDraw {
                        pos,
                        yaw: spin,
                        tint,
                        kind: EntityDrawKind::Box { w: 0.25, h: 0.25, color: [0.85, 0.85, 0.85] },
                    });
                }
                continue;
            }

            // --- humanoid mobs: their real texture on the player model --------
            if let Some(&key) = self.mob_skin_key.get(&snap.kind)
                && renderer.is_some_and(|r| r.has_skin(key))
            {
                let eq = &snap.equipment;
                let armor = [
                    eq.head.as_deref().and_then(armor_material),
                    eq.chest.as_deref().and_then(armor_material),
                    eq.legs.as_deref().and_then(armor_material),
                    eq.feet.as_deref().and_then(armor_material),
                ];
                let main_hand = eq.main_hand.as_deref().and_then(|n| self.item_icons.uv(n));
                let off_hand = eq.off_hand.as_deref().and_then(|n| self.item_icons.uv(n));
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    kind: EntityDrawKind::Player {
                        skin: key, slim: false, swing, attack_swing, sneaking: snap.sneaking,
                        skin_layers: 0xFF, head_pitch: pitch, armor, main_hand, off_hand,
                    },
                });
                continue;
            }

            // --- non-humanoid mobs with a real cuboid model + texture ---------
            if let Some(&(base_tex, model)) = self.mob_model.get(&snap.kind) {
                // Colour/type variants override the default texture: a
                // registry-resolved name first (cat/wolf/cow/chicken/pig/frog),
                // then an index variant (rabbit/parrot/…), else the default.
                let tex = snap
                    .variant_name
                    .as_ref()
                    .and_then(|n| self.mob_named_variant_tex.get(&(snap.kind.clone(), n.clone())).copied())
                    .or_else(|| self.mob_variant_tex.get(&(snap.kind.clone(), snap.variant)).copied())
                    .unwrap_or(base_tex);
                // Slimes/magma cubes scale with their size; the cube model is
                // authored at the size-1 (0.5-block) scale.
                let base = if matches!(snap.kind.as_str(), "slime" | "magma_cube") {
                    (snap.height / 0.5).clamp(0.4, 5.0)
                } else {
                    1.0
                };
                // Babies render about half size (vanilla also enlarges the head;
                // a uniform shrink is a close approximation).
                let scale = if snap.baby { base * 0.55 } else { base };
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    kind: EntityDrawKind::Mob { tex, model, swing, head_pitch: pitch, scale },
                });
                continue;
            }

            // --- everything else: a per-type tinted box (never uniform yellow) -
            let (w, h) = (snap.width.max(0.1), snap.height.max(0.1));
            out.push(EntityDraw {
                pos,
                yaw,
                tint,
                kind: EntityDrawKind::Box { w, h, color: mob_color(&snap.kind) },
            });
        }
        if let Some(me) = self.local_player_draw() {
            out.push(me);
        }
        // Particles: tiny colored cubes, centered on their position.
        for p in &self.particles {
            // Shrink toward end-of-life so they fade out instead of popping.
            let k = 1.0 - (p.age / p.life).clamp(0.0, 1.0);
            let size = p.size * (0.4 + 0.6 * k);
            out.push(EntityDraw {
                pos: [p.pos[0], p.pos[1] - size as f64 / 2.0, p.pos[2]],
                yaw: 0.0,
                tint: [1.0, 1.0, 1.0],
                kind: EntityDrawKind::Box { w: size, h: size, color: p.color },
            });
        }
        // Rain: thin tall streaks, a desaturated blue-gray, slightly dimmer at
        // night (the sky darkening handles most of the mood).
        if !self.rain_drops.is_empty() {
            let d = 0.55 + 0.45 * self.daylight;
            let color = [0.55 * d, 0.60 * d, 0.72 * d];
            for (pos, _) in &self.rain_drops {
                out.push(EntityDraw {
                    pos: *pos,
                    yaw: 0.0,
                    tint: [1.0, 1.0, 1.0],
                    kind: EntityDrawKind::Box { w: 0.02, h: 0.7, color },
                });
            }
        }
        out
    }
}

/// A representative flat color for a mob type, so non-modelled entities read as
/// distinct silhouettes instead of a single yellow box.
fn mob_color(kind: &str) -> [f32; 3] {
    match kind {
        "creeper" => [0.30, 0.75, 0.30],
        "spider" | "cave_spider" => [0.25, 0.20, 0.20],
        "cow" | "mooshroom" => [0.40, 0.28, 0.18],
        "pig" => [0.94, 0.66, 0.66],
        "sheep" => [0.90, 0.90, 0.88],
        "chicken" => [0.95, 0.95, 0.85],
        "wolf" | "fox" => [0.80, 0.78, 0.72],
        "villager" | "wandering_trader" => [0.55, 0.42, 0.30],
        "enderman" => [0.10, 0.10, 0.14],
        "slime" | "magma_cube" => [0.45, 0.80, 0.40],
        "blaze" => [0.95, 0.70, 0.15],
        "horse" | "donkey" | "mule" => [0.55, 0.40, 0.25],
        "iron_golem" => [0.72, 0.70, 0.62],
        "squid" | "glow_squid" => [0.35, 0.30, 0.45],
        "bat" => [0.30, 0.24, 0.20],
        "armor_stand" => [0.78, 0.72, 0.58],
        // Boats & minecarts: wooden brown / cart grey.
        k if k.contains("boat") => [0.63, 0.48, 0.30],
        k if k.contains("minecart") => [0.42, 0.42, 0.46],
        "item_frame" | "glow_item_frame" | "painting" => [0.55, 0.42, 0.28],
        "end_crystal" => [0.85, 0.55, 0.95],
        "falling_block" => [0.55, 0.52, 0.50],
        // Projectiles / small things.
        "arrow" | "spectral_arrow" => [0.75, 0.75, 0.75],
        "experience_orb" => [0.55, 0.95, 0.35],
        "tnt" => [0.85, 0.20, 0.20],
        _ => [0.60, 0.62, 0.66], // neutral grey default
    }
}

// -- pure helpers ---------------------------------------------------------------

/// Any currently pressed key matches the binding id.
fn key_down(keys: &HashSet<KeyCode>, id: &str) -> bool {
    keys.iter().any(|c| KeyBinds::matches(id, *c))
}

fn clamp_pitch(pitch: f32) -> f32 {
    pitch.clamp(-89.9, 89.9)
}

/// Current wall-clock time in whole unix seconds (Discord's elapsed timer).
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Map an equipped item's registry name (e.g. "diamond_chestplate") to its
/// armor material for rendering. Returns `None` for non-armor items (a carved
/// pumpkin, a mob head, a held tool …) so no armor layer is drawn.
fn armor_material(item: &str) -> Option<ArmorMaterial> {
    // Only the four armor suffixes count; heads/pumpkins/elytra are not layers.
    if !(item.ends_with("_helmet")
        || item.ends_with("_chestplate")
        || item.ends_with("_leggings")
        || item.ends_with("_boots"))
    {
        return None;
    }
    let mat = if item.starts_with("leather_") {
        ArmorMaterial::Leather
    } else if item.starts_with("chainmail_") {
        ArmorMaterial::Chainmail
    } else if item.starts_with("iron_") {
        ArmorMaterial::Iron
    } else if item.starts_with("golden_") {
        ArmorMaterial::Gold
    } else if item.starts_with("diamond_") {
        ArmorMaterial::Diamond
    } else if item.starts_with("netherite_") {
        ArmorMaterial::Netherite
    } else if item.starts_with("copper_") {
        ArmorMaterial::Copper
    } else if item == "turtle_helmet" {
        ArmorMaterial::Turtle
    } else {
        return None;
    };
    Some(mat)
}

/// Shortest-arc interpolation between two angles in degrees.
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = (b - a).rem_euclid(360.0);
    if d > 180.0 {
        d -= 360.0;
    }
    a + d * t
}

/// Ray/AABB entry distance (slab method). `None` when the ray misses;
/// `Some(0.0)` when the ray starts inside the box.
fn ray_aabb(eye: [f64; 3], dir: [f64; 3], min: [f64; 3], max: [f64; 3]) -> Option<f64> {
    let mut t0 = 0.0f64;
    let mut t1 = f64::INFINITY;
    for a in 0..3 {
        if dir[a].abs() < 1e-12 {
            if eye[a] < min[a] || eye[a] > max[a] {
                return None;
            }
        } else {
            let inv = 1.0 / dir[a];
            let (ta, tb) = ((min[a] - eye[a]) * inv, (max[a] - eye[a]) * inv);
            t0 = t0.max(ta.min(tb));
            t1 = t1.min(ta.max(tb));
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}

/// A hotbar-select bind (default Digit1..Digit9) → hotbar slot 0..8.
fn hotbar_slot(keys: &KeyBinds, code: KeyCode) -> Option<u8> {
    keys.hotbar()
        .iter()
        .position(|id| KeyBinds::matches(id, code))
        .map(|i| i as u8)
}

/// Rolling-average fps over the stored frame instants (0 until 2 frames exist).
fn fps_of(times: &VecDeque<Instant>) -> f32 {
    if times.len() < 2 {
        return 0.0;
    }
    let span = times[times.len() - 1].duration_since(times[0]).as_secs_f32();
    if span <= 0.0 {
        return 0.0;
    }
    (times.len() - 1) as f32 / span
}

/// Daylight factor from world time: 1.0 at noon (6000), 0.2 at midnight.
/// Negative time = frozen cycle (bridge convention) — use its absolute value.
fn daylight_factor(time_of_day: i64) -> f32 {
    let t = time_of_day.abs().rem_euclid(24000) as f32;
    let raw = ((t / 24000.0 - 0.25) * std::f32::consts::TAU).cos() * 0.5 + 0.5;
    0.2 + 0.8 * raw
}

/// One xorshift step in [0,1) driving a raw `u64` state (used by the rain field,
/// which can't borrow `self` for `rand01` while it holds `&mut rain_drops`).
fn xorshift01(rng: &mut u64) -> f32 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    ((*rng >> 40) as f32) / (1u64 << 24) as f32
}

/// A fresh raindrop at a random spot in the cylinder of radius `r` above the
/// player: `(world pos, fall speed blocks/s)`.
fn spawn_raindrop(rng: &mut u64, center: [f64; 3], r: f64) -> ([f64; 3], f32) {
    let ang = xorshift01(rng) as f64 * std::f64::consts::TAU;
    let rad = (xorshift01(rng) as f64).sqrt() * r;
    let x = center[0] + ang.cos() * rad;
    let z = center[2] + ang.sin() * rad;
    let y = center[1] + 6.0 + xorshift01(rng) as f64 * 12.0;
    let speed = 18.0 + xorshift01(rng) * 9.0;
    ([x, y, z], speed)
}

/// Baked geometry `(pos centered at origin in unit-cube space, atlas uv)` of a
/// block's representative state, for the 3D block-in-hand and dropped blocks.
/// `None` for non-blocks or blocks with no drawable model (air/fluids/fallbacks).
/// A free function so callers can pass disjoint field borrows (e.g. inside a
/// `self.tracks.values_mut()` loop).
fn block_geometry(
    store: &BakedModelStore,
    block_state_by_name: &HashMap<String, StateId>,
    name: &str,
) -> Option<Vec<([f32; 3], [f32; 2])>> {
    let &sid = block_state_by_name.get(name)?;
    let model = store.get(sid);
    if model.quads.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(model.quads.len() * 6);
    for q in &model.quads {
        // Triangulate 0-1-2, 0-2-3 and center the unit cube on the origin.
        for &i in &[0usize, 1, 2, 0, 2, 3] {
            let v = q.verts[i];
            out.push(([v[0] - 0.5, v[1] - 0.5, v[2] - 0.5], q.uvs[i]));
        }
    }
    Some(out)
}

/// Hermite smoothstep. Works for `edge0 < edge1` and (reversed) `edge0 > edge1`.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Celestial rotation angle (radians) from world time — 0 at noon, matching
/// `daylight_factor`. The sun height is `cos(angle)`.
fn sun_angle_of(time_of_day: i64) -> f32 {
    let frac = time_of_day.abs().rem_euclid(24000) as f32 / 24000.0;
    (frac - 0.25) * std::f32::consts::TAU
}

/// Moon phase textures in vanilla phase order (index = `(day / 24000) % 8`).
const MOON_PHASES: [&str; 8] = [
    "full_moon",
    "waning_gibbous",
    "third_quarter",
    "waning_crescent",
    "new_moon",
    "waxing_crescent",
    "first_quarter",
    "waxing_gibbous",
];

/// Load the sun + 8 moon-phase textures from the jar and hand them to the
/// renderer. Best-effort: a missing texture just disables that body.
fn load_sky_textures(pack: &mut AssetPack, renderer: &mut Renderer) {
    let Ok(sun) = pack.texture_png("environment/celestial/sun") else {
        warn!("app: no sun texture in the jar — celestial sky disabled");
        return;
    };
    let moons: Vec<image::RgbaImage> = MOON_PHASES
        .iter()
        .map(|p| {
            pack.texture_png(&format!("environment/celestial/moon/{p}"))
                .unwrap_or_else(|_| image::RgbaImage::new(0, 0))
        })
        .collect();
    let clouds = pack.texture_png("environment/clouds").ok();
    renderer.set_sky_textures(&sun, &moons, clouds.as_ref());
    info!("app: celestial sky textures loaded");
}

/// Overworld sky color from world time: a day↔night lerp with a warm horizon
/// glow around sunrise/sunset.
fn overworld_sky_color(time_of_day: i64) -> [f32; 3] {
    let h = sun_angle_of(time_of_day).cos(); // sun height -1..1
    let day = smoothstep(-0.05, 0.18, h);
    let night = [0.015, 0.02, 0.06];
    let noon = [0.47, 0.65, 1.0];
    let base = [
        night[0] + (noon[0] - night[0]) * day,
        night[1] + (noon[1] - night[1]) * day,
        night[2] + (noon[2] - night[2]) * day,
    ];
    // Warm glow when the sun sits near the horizon (but not in deep night).
    let glow = (1.0 - (h.abs() / 0.16).min(1.0)) * smoothstep(-0.18, 0.02, h) * 0.45;
    let sunset = [0.80, 0.42, 0.26];
    [
        base[0] + (sunset[0] - base[0]) * glow,
        base[1] + (sunset[1] - base[1]) * glow,
        base[2] + (sunset[2] - base[2]) * glow,
    ]
}

/// Sun/moon/star/cloud parameters from world time (overworld only). `elapsed`
/// is a monotonic seconds counter used only for the cloud drift.
fn sky_params_of(time_of_day: i64, elapsed: f32) -> crate::render::SkyParams {
    let angle = sun_angle_of(time_of_day);
    let h = angle.cos();
    // Clouds dim at night (never fully black — moonlit) at ~80% opacity.
    let day = smoothstep(-0.1, 0.2, h);
    let b = 0.35 + 0.6 * day;
    // Warm sunrise/sunset glow, peaking when the sun sits near the horizon.
    let glow = (1.0 - (h.abs() / 0.25).min(1.0)) * smoothstep(-0.22, 0.02, h);
    crate::render::SkyParams {
        sun_angle: angle,
        // Stars fade in below the horizon (reversed edges: 0 above, 0.9 deep night).
        star_brightness: smoothstep(0.08, -0.18, h) * 0.9,
        moon_phase: ((time_of_day.abs() / 24000) % 8) as usize,
        sun_alpha: smoothstep(-0.09, 0.06, h),
        moon_alpha: smoothstep(-0.06, 0.09, -h),
        cloud_scroll: elapsed * 0.6,
        cloud_color: [b, b, b, 0.8],
        glow_color: [1.0, 0.52, 0.28, glow * 0.8],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn pitch_clamps_to_just_under_vertical() {
        assert_eq!(clamp_pitch(120.0), 89.9);
        assert_eq!(clamp_pitch(-1000.0), -89.9);
        assert_eq!(clamp_pitch(15.5), 15.5);
    }

    #[test]
    fn armor_material_maps_items() {
        assert_eq!(armor_material("diamond_chestplate"), Some(ArmorMaterial::Diamond));
        assert_eq!(armor_material("golden_boots"), Some(ArmorMaterial::Gold));
        assert_eq!(armor_material("netherite_helmet"), Some(ArmorMaterial::Netherite));
        assert_eq!(armor_material("chainmail_leggings"), Some(ArmorMaterial::Chainmail));
        assert_eq!(armor_material("turtle_helmet"), Some(ArmorMaterial::Turtle));
        // Non-armor items and non-armor headwear map to nothing.
        assert_eq!(armor_material("diamond_sword"), None);
        assert_eq!(armor_material("carved_pumpkin"), None);
        assert_eq!(armor_material("player_head"), None);
        // Turtle only exists as a helmet.
        assert_eq!(armor_material("turtle_chestplate"), None);
    }

    #[test]
    fn hotbar_keys_map_to_slots() {
        let keys = KeyBinds::default();
        assert_eq!(hotbar_slot(&keys, KeyCode::Digit1), Some(0));
        assert_eq!(hotbar_slot(&keys, KeyCode::Digit9), Some(8));
        assert_eq!(hotbar_slot(&keys, KeyCode::KeyW), None);
        assert_eq!(hotbar_slot(&keys, KeyCode::Digit0), None);
    }

    #[test]
    fn fps_rolling_average() {
        let mut times = VecDeque::new();
        assert_eq!(fps_of(&times), 0.0);
        let t0 = Instant::now();
        times.push_back(t0);
        assert_eq!(fps_of(&times), 0.0);
        // 30 more frames, 10 ms apart → 100 fps.
        for i in 1..=30u32 {
            times.push_back(t0 + Duration::from_millis(10 * i as u64));
        }
        let fps = fps_of(&times);
        assert!((fps - 100.0).abs() < 0.5, "fps = {fps}");
    }

    #[test]
    fn daylight_curve() {
        assert!((daylight_factor(6000) - 1.0).abs() < 1e-4); // noon
        assert!((daylight_factor(18000) - 0.2).abs() < 1e-4); // midnight
        // Frozen (negative) time uses the absolute value.
        assert!((daylight_factor(-6000) - 1.0).abs() < 1e-4);
        // Sunrise/sunset are between the extremes.
        let dawn = daylight_factor(0);
        assert!(dawn > 0.2 && dawn < 1.0);
    }

    #[test]
    fn ray_aabb_hits() {
        let min = [-0.3, 0.0, 4.0];
        let max = [0.3, 1.8, 4.6];
        // Straight ahead (+z) from eye height into the box front face.
        let t = ray_aabb([0.0, 1.6, 0.0], [0.0, 0.0, 1.0], min, max);
        assert!((t.unwrap() - 4.0).abs() < 1e-9);
        // Miss: aiming above the box.
        assert!(ray_aabb([0.0, 1.6, 0.0], [0.0, 0.1, 1.0], min, max).is_none());
        // Behind: the box is at -z, ray goes +z.
        assert!(ray_aabb([0.0, 1.6, 8.0], [0.0, 0.0, 1.0], min, max).is_none());
        // Starting inside → distance 0.
        let t = ray_aabb([0.0, 1.0, 4.3], [1.0, 0.0, 0.0], min, max);
        assert_eq!(t, Some(0.0));
        // Axis-parallel ray offset outside the slab misses.
        assert!(ray_aabb([2.0, 1.0, 0.0], [0.0, 0.0, 1.0], min, max).is_none());
    }

    #[test]
    fn angle_lerp_takes_shortest_arc() {
        assert!((lerp_angle(350.0, 10.0, 0.5) - 360.0).abs() < 1e-4);
        assert!((lerp_angle(10.0, 350.0, 0.5) - (-10.0 + 10.0)).abs() < 1e-4);
        assert!((lerp_angle(0.0, 90.0, 0.5) - 45.0).abs() < 1e-4);
    }

    #[test]
    fn entity_track_interpolates_between_snapshots() {
        let snap = |x: f64| EntitySnapshot {
            id: 1,
            kind: "player".into(),
            pos: [x, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            width: 0.6,
            height: 1.8,
            name: None,
            name_spans: None,
            is_player: true,
            sneaking: false,
            sprinting: false,
            invisible: false,
            baby: false,
            uuid: None,
            skin_url: None,
            skin_slim: false,
            equipment: Default::default(),
            item: None,
            variant: 0,
            variant_name: None,
        };
        let t0 = Instant::now();
        let mut track = EntityTrack::new(snap(0.0), t0);
        track.push(snap(1.0), t0 + Duration::from_millis(50));
        let (pos, _, _) = track.sample(t0 + Duration::from_millis(25));
        assert!((pos[0] - 0.5).abs() < 1e-6, "pos[0] = {}", pos[0]);
        // Before the window → first; after → last.
        assert_eq!(track.sample(t0 - Duration::from_millis(10)).0[0], 0.0);
        assert_eq!(track.sample(t0 + Duration::from_millis(500)).0[0], 1.0);
    }

    #[test]
    fn teleport_clears_interpolation_history() {
        let snap = |x: f64| EntitySnapshot {
            id: 1,
            kind: "player".into(),
            pos: [x, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            width: 0.6,
            height: 1.8,
            name: None,
            name_spans: None,
            is_player: true,
            sneaking: false,
            sprinting: false,
            invisible: false,
            baby: false,
            uuid: None,
            skin_url: None,
            skin_slim: false,
            equipment: Default::default(),
            item: None,
            variant: 0,
            variant_name: None,
        };
        let t0 = Instant::now();
        let mut track = EntityTrack::new(snap(0.0), t0);
        track.push(snap(100.0), t0 + Duration::from_millis(50));
        // No gliding across 100 blocks: history restarts at the new spot.
        assert_eq!(track.sample(t0 + Duration::from_millis(25)).0[0], 100.0);
    }
}
