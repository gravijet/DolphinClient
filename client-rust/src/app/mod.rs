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

pub mod advancements;
pub mod ambient;
pub mod blockentities;
pub mod blocksound;
pub mod chat;
pub mod container;
pub mod creative;
pub mod dial;
pub mod entitystatus;
pub mod fireworks;
pub mod footsteps;
pub mod gamepad;
pub mod hud;
pub mod lids;
pub mod maps;
pub mod mcui;
pub mod music;
pub mod offscreen;
pub mod pistons;
pub mod recipebook;
pub mod resourcepacks;
pub mod riding;
pub mod serverlist;
pub mod skins;
pub mod statistics;
pub mod tablist;
pub mod toasts;
pub mod viewfx;

use crate::assets::atlas::{Atlas, AtlasAnimator};
use crate::assets::blockmap::BlockTable;
use crate::assets::items::ItemIcons;
use crate::assets::{AssetPack, Lang};
use crate::audio::AudioEngine;
use crate::bridge::events;
use crate::bridge::events::{
    AccountConfig, BlockEntityData, BridgeOptions, ChatSpan, Command, EntitySnapshot, GameEvent,
    ItemSnapshot, ParticleTex, PlayerSnapshot, ScoreLine,
};
use crate::bridge::{GameHandle, spawn_bridge};
use crate::models::BakedModelStore;
use crate::models::bake::{ChestKind, DynBlock};
use crate::render::{
    ArmorMaterial, EguiFrame, EntityDraw, EntityDrawKind, LightmapParams, MobModel, MobPose,
    PlayerPose, RenderTarget, Renderer, SceneParams, camera,
};
use crate::settings::{GameSettings, KeyBinds, ServerResourcePackPolicy, key_id};
use crate::types::{BlockPos, ChunkPos, Face, MeshData, SectionPos, StateId};
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
    /// Private launcher-selected cosmetics. They override Mojang textures only
    /// for the local player and never cross the network bridge.
    pub local_cosmetics: LocalCosmetics,
    /// Bundled Pumpkin server binary for Singleplayer; `None` disables it.
    pub server_binary: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct LocalCosmetics {
    pub skin: Option<PathBuf>,
    pub cape: Option<PathBuf>,
    pub slim: bool,
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
        "Timed out",
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
    NEEDLES
        .iter()
        .any(|n| reason.contains(n) || lower.contains(&n.to_lowercase()))
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

/// Linux only: sets the X11 `WM_CLASS` / Wayland `app_id` to
/// `de.dolphinclient.client`, matching the launcher's own
/// `de.dolphinclient.launcher` (see `desktopicon.rs`). Desktop environments
/// key the taskbar/alt-tab icon off this, not off `with_window_icon` alone —
/// without it, most Wayland compositors fall back to a generic icon for the
/// running game window. A no-op on Windows/macOS, where the icon is already
/// embedded into the executable (see `build.rs`).
#[cfg(target_os = "linux")]
fn with_linux_app_id(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    const APP_ID: &str = "de.dolphinclient.client";
    let attrs =
        winit::platform::x11::WindowAttributesExtX11::with_name(attrs, APP_ID, APP_ID);
    winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs, APP_ID, APP_ID)
}

#[cfg(not(target_os = "linux"))]
fn with_linux_app_id(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attrs
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
    // Local packs are now a selected low-to-high priority stack. On the first
    // migration all existing ZIPs remain enabled, preserving the old behavior.
    let local_packs = resourcepacks::LocalPackStore::load();
    let applied_packs = crate::assets::load_resource_pack_paths(&mut pack, &local_packs.paths());
    if !applied_packs.is_empty() {
        info!(packs = ?applied_packs, "app: applied client resource packs");
    }
    let table = BlockTable::load_or_embedded(opts.blocks_report.as_deref())
        .context("loading block table")?;
    info!(
        states = table.len(),
        elapsed_ms = t0.elapsed().as_millis() as u64,
        "app: block table loaded"
    );
    let t1 = Instant::now();
    let (store, mut atlas) =
        BakedModelStore::bake_all(&mut pack, &table).context("baking models")?;
    let atlas_anim = AtlasAnimator::new(std::mem::take(&mut atlas.animations));
    info!(
        elapsed_ms = t1.elapsed().as_millis() as u64,
        animated = atlas_anim.len(),
        "app: models baked"
    );
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
        warn!(
            found = crack_textures.len(),
            "app: incomplete destroy_stage textures"
        );
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
        // 0.93.0 — Mounts of Mayhem: a desert skeleton variant, same shape.
        ("parched", "entity/skeleton/parched"),
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
    info!(
        count = mob_textures.len(),
        "app: humanoid mob textures loaded"
    );

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
        (
            "chicken",
            "entity/chicken/chicken_temperate",
            MobModel::Chicken,
        ),
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
        (
            "spruce_chest_boat",
            "entity/chest_boat/spruce",
            MobModel::Boat,
        ),
        (
            "birch_chest_boat",
            "entity/chest_boat/birch",
            MobModel::Boat,
        ),
        (
            "jungle_chest_boat",
            "entity/chest_boat/jungle",
            MobModel::Boat,
        ),
        (
            "acacia_chest_boat",
            "entity/chest_boat/acacia",
            MobModel::Boat,
        ),
        (
            "dark_oak_chest_boat",
            "entity/chest_boat/dark_oak",
            MobModel::Boat,
        ),
        (
            "mangrove_chest_boat",
            "entity/chest_boat/mangrove",
            MobModel::Boat,
        ),
        (
            "cherry_chest_boat",
            "entity/chest_boat/cherry",
            MobModel::Boat,
        ),
        (
            "pale_oak_chest_boat",
            "entity/chest_boat/pale_oak",
            MobModel::Boat,
        ),
        (
            "bamboo_chest_raft",
            "entity/chest_boat/bamboo",
            MobModel::Boat,
        ),
        // Minecarts: one open-box model, the shared minecart texture. Typed
        // carts (chest/furnace/tnt/hopper/…) add their content block on top in
        // the draw path.
        ("minecart", "entity/minecart/minecart", MobModel::Minecart),
        (
            "chest_minecart",
            "entity/minecart/minecart",
            MobModel::Minecart,
        ),
        (
            "furnace_minecart",
            "entity/minecart/minecart",
            MobModel::Minecart,
        ),
        (
            "tnt_minecart",
            "entity/minecart/minecart",
            MobModel::Minecart,
        ),
        (
            "hopper_minecart",
            "entity/minecart/minecart",
            MobModel::Minecart,
        ),
        (
            "spawner_minecart",
            "entity/minecart/minecart",
            MobModel::Minecart,
        ),
        (
            "command_block_minecart",
            "entity/minecart/minecart",
            MobModel::Minecart,
        ),
        // Extended roster: real cuboid models for the common overworld mobs that
        // used to fall back to a flat coloured box.
        ("spider", "entity/spider/spider", MobModel::Spider),
        ("cave_spider", "entity/spider/cave_spider", MobModel::Spider),
        ("wolf", "entity/wolf/wolf", MobModel::Wolf),
        ("fox", "entity/fox/fox", MobModel::Fox),
        ("villager", "entity/villager/villager", MobModel::Villager),
        (
            "wandering_trader",
            "entity/wandering_trader/wandering_trader",
            MobModel::Villager,
        ),
        ("enderman", "entity/enderman/enderman", MobModel::Enderman),
        (
            "iron_golem",
            "entity/iron_golem/iron_golem",
            MobModel::IronGolem,
        ),
        ("squid", "entity/squid/squid", MobModel::Squid),
        ("glow_squid", "entity/squid/glow_squid", MobModel::Squid),
        ("bat", "entity/bat/bat", MobModel::Bat),
        ("rabbit", "entity/rabbit/rabbit_brown", MobModel::Rabbit),
        ("horse", "entity/horse/horse_brown", MobModel::Horse),
        ("donkey", "entity/horse/donkey", MobModel::Horse),
        ("mule", "entity/horse/mule", MobModel::Horse),
        (
            "skeleton_horse",
            "entity/horse/horse_skeleton",
            MobModel::Horse,
        ),
        ("zombie_horse", "entity/horse/horse_zombie", MobModel::Horse),
        ("cat", "entity/cat/cat_tabby", MobModel::Cat),
        ("ocelot", "entity/cat/ocelot", MobModel::Cat),
        (
            "snow_golem",
            "entity/snow_golem/snow_golem",
            MobModel::SnowGolem,
        ),
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
        (
            "elder_guardian",
            "entity/guardian/guardian_elder",
            MobModel::Guardian,
        ),
        ("cod", "entity/fish/cod", MobModel::Cod),
        ("salmon", "entity/fish/salmon", MobModel::Salmon),
        ("bee", "entity/bee/bee", MobModel::Bee),
        (
            "silverfish",
            "entity/silverfish/silverfish",
            MobModel::Silverfish,
        ),
        ("parrot", "entity/parrot/parrot_red_blue", MobModel::Parrot),
        ("phantom", "entity/phantom/phantom", MobModel::Phantom),
        // 0.38.0 bestiary expansion.
        ("axolotl", "entity/axolotl/axolotl_lucy", MobModel::Axolotl),
        ("frog", "entity/frog/frog_temperate", MobModel::Frog),
        ("tadpole", "entity/tadpole/tadpole", MobModel::Tadpole),
        ("camel", "entity/camel/camel", MobModel::Camel),
        ("sniffer", "entity/sniffer/sniffer", MobModel::Sniffer),
        (
            "armadillo",
            "entity/armadillo/armadillo",
            MobModel::Armadillo,
        ),
        ("allay", "entity/allay/allay", MobModel::Allay),
        ("vex", "entity/illager/vex", MobModel::Vex),
        (
            "endermite",
            "entity/endermite/endermite",
            MobModel::Endermite,
        ),
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
        (
            "ender_dragon",
            "entity/enderdragon/dragon",
            MobModel::EnderDragon,
        ),
        ("wither", "entity/wither/wither", MobModel::Wither),
        ("shulker", "entity/shulker/shulker", MobModel::Shulker),
        (
            "armor_stand",
            "entity/armorstand/armorstand",
            MobModel::ArmorStand,
        ),
        (
            "end_crystal",
            "entity/end_crystal/end_crystal",
            MobModel::EndCrystal,
        ),
        // 0.93.0 — Mounts of Mayhem. The zombie nautilus shares the plain
        // nautilus's shape (only its rarer warm-ocean coral variant grows
        // extra coral on the shell, not modelled here), and camel husk is a
        // reskin of the ordinary camel — exactly how zoglin above reuses
        // hoglin's own model.
        ("nautilus", "entity/nautilus/nautilus", MobModel::Nautilus),
        (
            "zombie_nautilus",
            "entity/nautilus/zombie_nautilus",
            MobModel::Nautilus,
        ),
        (
            "camel_husk",
            "entity/camel/camel_husk",
            MobModel::Camel,
        ),
        // The copper golem's oxidation stage (unweathered/exposed/weathered/
        // oxidized) is read from its real `WeatherState` metadata and applied
        // as a texture swap below (VARIANT_MOBS) — this entry is just its
        // unweathered default.
        (
            "copper_golem",
            "entity/copper_golem/copper_golem",
            MobModel::CopperGolem,
        ),
        // These three are drawn with a custom per-frame orientation rather
        // than the generic standing-mob dispatch below (a projectile's flight
        // angle, a bullet's tumble, fangs that bite and rise) — this entry
        // only exists to load and key the real texture.
        (
            "evoker_fangs",
            "entity/illager/evoker_fangs",
            MobModel::EvokerFangs,
        ),
        (
            "shulker_bullet",
            "entity/shulker/spark",
            MobModel::ShulkerBullet,
        ),
        ("llama_spit", "entity/llama/llama_spit", MobModel::LlamaSpit),
    ];
    let mut mob_model: HashMap<String, (u64, MobModel)> = HashMap::new();
    for (kind, path, model) in MODEL_MOBS {
        if let Ok(img) = pack.texture_png(path) {
            let key = fnv64(format!("mobmodel:{kind}").as_bytes());
            mob_model.insert((*kind).to_string(), (key, *model));
            mob_textures.push((key, img));
        } else {
            warn!(
                kind,
                path, "app: mob texture missing — will fall back to a box"
            );
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
        // Copper golem oxidation stage (WeatherState): 0 unaffected (the
        // MODEL_MOBS default, listed for clarity), 1 exposed, 2 weathered,
        // 3 oxidized — same four stages as the copper block it's built from.
        ("copper_golem", 0, "entity/copper_golem/copper_golem"),
        ("copper_golem", 1, "entity/copper_golem/copper_golem_exposed"),
        ("copper_golem", 2, "entity/copper_golem/copper_golem_weathered"),
        ("copper_golem", 3, "entity/copper_golem/copper_golem_oxidized"),
        // Zombie nautilus warm-ocean variant: its own coral-mottled texture
        // (the extra coral geometry itself is a separate overlay layer, see
        // MobModel::NautilusCorals and the "corals" draw block below).
        ("zombie_nautilus", 1, "entity/nautilus/zombie_nautilus_coral"),
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
        (
            "cat",
            "british_shorthair",
            "entity/cat/cat_british_shorthair",
        ),
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

    // Villager appearance: vanilla layers three textures — the biome `type`
    // (a body hued for the villager's home biome), a `profession` clothing
    // overlay, and a small `profession_level` trade badge on the chest. We
    // pre-composite every reachable (type, profession, level) combination into
    // one texture and file it under the same `(kind, variant_name)` map the
    // bridge drives: the bridge sends `variant_name = "{type}|{profession}|
    // {level}"` (level 0 for the badgeless none/nitwit). Villagers with no
    // metadata yet fall back to the plain `entity/villager/villager` base.
    {
        const V_TYPES: &[&str] = &[
            "plains", "desert", "jungle", "savanna", "snow", "swamp", "taiga",
        ];
        // (profession, wears a trade badge). none/nitwit never do.
        const V_PROFS: &[(&str, bool)] = &[
            ("none", false),
            ("nitwit", false),
            ("armorer", true),
            ("butcher", true),
            ("cartographer", true),
            ("cleric", true),
            ("farmer", true),
            ("fisherman", true),
            ("fletcher", true),
            ("leatherworker", true),
            ("librarian", true),
            ("mason", true),
            ("shepherd", true),
            ("toolsmith", true),
            ("weaponsmith", true),
        ];
        const V_LEVELS: &[&str] = &["stone", "iron", "gold", "emerald", "diamond"];
        // Decode each shared layer PNG once. `none` has no clothing overlay.
        let type_imgs: HashMap<&str, image::RgbaImage> = V_TYPES
            .iter()
            .filter_map(|t| {
                pack.texture_png(&format!("entity/villager/type/{t}"))
                    .ok()
                    .map(|i| (*t, i))
            })
            .collect();
        let prof_imgs: HashMap<&str, image::RgbaImage> = V_PROFS
            .iter()
            .filter(|(p, _)| *p != "none")
            .filter_map(|(p, _)| {
                pack.texture_png(&format!("entity/villager/profession/{p}"))
                    .ok()
                    .map(|i| (*p, i))
            })
            .collect();
        let level_imgs: Vec<image::RgbaImage> = V_LEVELS
            .iter()
            .filter_map(|l| {
                pack.texture_png(&format!("entity/villager/profession_level/{l}"))
                    .ok()
            })
            .collect();
        let mut built = 0usize;
        for t in V_TYPES {
            let Some(base) = type_imgs.get(*t) else {
                continue;
            };
            for (prof, badge) in V_PROFS {
                // Employed professions build one texture per trade level (1..=5);
                // the badgeless none/nitwit collapse to a single level-0 texture.
                let levels: &[u32] = if *badge { &[1, 2, 3, 4, 5] } else { &[0] };
                for &lvl in levels {
                    let mut img = base.clone();
                    if let Some(p) = prof_imgs.get(*prof) {
                        image::imageops::overlay(&mut img, p, 0, 0);
                    }
                    if *badge && let Some(b) = level_imgs.get((lvl - 1) as usize) {
                        image::imageops::overlay(&mut img, b, 0, 0);
                    }
                    let vname = format!("{t}|{prof}|{lvl}");
                    let key = fnv64(format!("mobvarname:villager:{vname}").as_bytes());
                    mob_named_variant_tex.insert(("villager".to_string(), vname), key);
                    mob_textures.push((key, img));
                    built += 1;
                }
            }
        }
        info!(built, "app: villager appearance composites built");
    }

    // Paintings: register every vanilla painting art texture + the shared
    // wooden back/edge texture, keyed by asset name. The bridge resolves each
    // painting's asset (and size) from the server's `painting_variant` registry;
    // the draw path looks the texture up here.
    const PAINTINGS: &[&str] = &[
        "alban",
        "aztec",
        "aztec2",
        "backyard",
        "baroque",
        "bomb",
        "bouquet",
        "burning_skull",
        "bust",
        "cavebird",
        "changing",
        "cotan",
        "courbet",
        "creebet",
        "dennis",
        "donkey_kong",
        "earth",
        "endboss",
        "fern",
        "fighters",
        "finding",
        "fire",
        "graham",
        "humble",
        "kebab",
        "lowmist",
        "match",
        "meditative",
        "orb",
        "owlemons",
        "passage",
        "pigscene",
        "plant",
        "pointer",
        "pond",
        "pool",
        "prairie_ride",
        "sea",
        "skeleton",
        "skull_and_roses",
        "stage",
        "sunflowers",
        "sunset",
        "tides",
        "unpacked",
        "void",
        "wanderer",
        "wasteland",
        "water",
        "wind",
        "wither",
    ];
    let mut painting_tex: HashMap<String, u64> = HashMap::new();
    for name in PAINTINGS {
        if let Ok(img) = pack.texture_png(&format!("painting/{name}")) {
            let key = fnv64(format!("painting:{name}").as_bytes());
            painting_tex.insert((*name).to_string(), key);
            mob_textures.push((key, img));
        }
    }
    let painting_back_tex = fnv64(b"painting:back");
    if let Ok(img) = pack.texture_png("painting/back") {
        mob_textures.push((painting_back_tex, img));
    }
    info!(
        paintings = painting_tex.len(),
        "app: painting textures loaded"
    );

    // Item-frame face textures (normal + glow). The wooden back/edges reuse the
    // painting back texture.
    let item_frame_tex = fnv64(b"frame:item_frame");
    if let Ok(img) = pack.texture_png("block/item_frame") {
        mob_textures.push((item_frame_tex, img));
    }
    let glow_item_frame_tex = fnv64(b"frame:glow_item_frame");
    if let Ok(img) = pack.texture_png("block/glow_item_frame") {
        mob_textures.push((glow_item_frame_tex, img));
    }

    // Projectile textures (arrows) — drawn on crossed planes, oriented in flight.
    let arrow_tex = fnv64(b"proj:arrow");
    if let Ok(img) = pack.texture_png("entity/projectiles/arrow") {
        mob_textures.push((arrow_tex, img));
    }
    let arrow_spectral_tex = fnv64(b"proj:arrow_spectral");
    if let Ok(img) = pack.texture_png("entity/projectiles/arrow_spectral") {
        mob_textures.push((arrow_spectral_tex, img));
    }

    // Experience orb — the texture is a 4×4 grid of orb frames; take one full
    // orb cell (row 2 is the roundest) as a single billboard sprite.
    let xp_orb_tex = fnv64(b"xp_orb");
    if let Ok(img) = pack.texture_png("entity/experience/experience_orb") {
        let (cw, ch) = (img.width() / 4, img.height() / 4);
        let cell = image::imageops::crop_imm(&img, cw * 2, ch * 2, cw, ch).to_image();
        mob_textures.push((xp_orb_tex, cell));
    }

    // Tropical fish: two body shapes, each a grayscale base body (tinted by the
    // fish's body colour) plus six pattern overlays (tinted by the pattern
    // colour). Both layers already carry alpha, so alpha-discard renders them.
    let mut fish_base_tex = [0u64; 2];
    let mut fish_pat_tex = [[0u64; 6]; 2];
    for (si, shape) in ["a", "b"].iter().enumerate() {
        let bkey = fnv64(format!("fish:base:{shape}").as_bytes());
        if let Ok(img) = pack.texture_png(&format!("entity/fish/tropical_{shape}")) {
            fish_base_tex[si] = bkey;
            mob_textures.push((bkey, img));
        }
        for p in 0..6 {
            let pkey = fnv64(format!("fish:pat:{shape}:{p}").as_bytes());
            if let Ok(img) =
                pack.texture_png(&format!("entity/fish/tropical_{shape}_pattern_{}", p + 1))
            {
                fish_pat_tex[si][p] = pkey;
                mob_textures.push((pkey, img));
            }
        }
    }

    // Pet collars (tamed cats/wolves): a near-white collar mask tinted by the
    // dye colour, drawn as an overlay on the animal model.
    let cat_collar_tex = fnv64(b"cat_collar");
    if let Ok(img) = pack.texture_png("entity/cat/cat_collar") {
        mob_textures.push((cat_collar_tex, img));
    }
    let wolf_collar_tex = fnv64(b"wolf_collar");
    if let Ok(img) = pack.texture_png("entity/wolf/wolf_collar") {
        mob_textures.push((wolf_collar_tex, img));
    }

    // Charged ("powered") creeper: the blue energy-swirl overlay, drawn slightly
    // inflated over the creeper so the green shows through the gaps.
    let creeper_armor_tex = fnv64(b"creeper_armor");
    if let Ok(img) = pack.texture_png("entity/creeper/creeper_armor") {
        mob_textures.push((creeper_armor_tex, img));
    }

    // Fire (on-fire entities): a 16×(16·N) vertical strip of flame frames. The
    // flame sits on a black background, so key near-black pixels to transparent
    // and the alpha-discard billboard pipeline renders a clean flame.
    let fire_tex = fnv64(b"fire");
    let mut fire_frames = 1u32;
    if let Ok(mut img) = pack.texture_png("block/fire_0") {
        for px in img.pixels_mut() {
            let [r, g, b, _] = px.0;
            if (r as u16 + g as u16 + b as u16) < 60 {
                px.0[3] = 0;
            }
        }
        fire_frames = (img.height() / img.width().max(1)).max(1);
        mob_textures.push((fire_tex, img));
    }

    // Elytra wings: a 64x32 sheet, padded out to the 64x64 the cape/skin UV
    // convention assumes.
    let elytra_tex = fnv64(b"elytra");
    if let Ok(img) = pack.texture_png("entity/equipment/wings/elytra") {
        mob_textures.push((elytra_tex, pad_to_square(&img)));
    }

    // Animal equipment: saddles, horse armour, llama carpets and wolf armour.
    // Each is drawn as its own layer over the animal's own model, so all we
    // need is the texture, keyed by the path it came from.
    let mut animal_equipment: HashMap<String, u64> = HashMap::new();
    let mut want = |path: String| {
        let key = fnv64(path.as_bytes());
        if let Ok(img) = pack.texture_png(&path) {
            mob_textures.push((key, img));
            animal_equipment.insert(path, key);
        }
    };
    for species in [
        "pig",
        "strider",
        "horse",
        "donkey",
        "mule",
        "camel",
        "skeleton_horse",
        "zombie_horse",
        // 0.93.0 — Mounts of Mayhem: both ride with a saddle too.
        "camel_husk",
        "nautilus",
    ] {
        want(format!("entity/equipment/{species}_saddle/saddle"));
    }
    for material in ["leather", "iron", "gold", "diamond", "copper", "netherite"] {
        want(format!("entity/equipment/horse_body/{material}"));
        // Nautilus armour comes in the same tiers minus leather.
        want(format!("entity/equipment/nautilus_body/{material}"));
    }
    want("entity/equipment/wolf_body/armadillo_scute".to_string());

    // Sheep fleece: drawn as an inflated layer over the bare sheep body, and
    // skipped once the sheep has been sheared (vanilla's wool model).
    let sheep_wool_tex = fnv64(b"sheep_wool");
    if let Ok(img) = pack.texture_png("entity/sheep/sheep_wool") {
        mob_textures.push((sheep_wool_tex, img));
    }
    // A ghast winding up a fireball turns red-eyed: vanilla swaps its whole
    // texture for the shooting one.
    let ghast_shooting_tex = fnv64(b"ghast_shooting");
    if let Ok(img) = pack.texture_png("entity/ghast/ghast_shooting") {
        mob_textures.push((ghast_shooting_tex, img));
    }
    // Weather: vanilla's own falling-rain and falling-snow sheets. Both tile
    // vertically, so they go in with a wrapping sampler.
    let rain_tex = fnv64(b"environment_rain");
    let snow_tex = fnv64(b"environment_snow");
    // Fishing bobber: the float itself; the line to the rod is drawn as a rope.
    let bobber_tex = fnv64(b"fishing_hook");
    if let Ok(img) = pack.texture_png("entity/fishing/fishing_hook") {
        mob_textures.push((bobber_tex, img));
    }
    // Beacon beam: tiled vertically up the whole column, so it is uploaded with
    // a wrapping sampler (see `tiled_textures`).
    let beam_tex = fnv64(b"beacon_beam");
    let mut tiled_textures = Vec::new();
    if let Ok(img) = pack.texture_png("entity/beacon/beacon_beam") {
        tiled_textures.push((beam_tex, img));
    }
    // Rain and snow tile vertically as they scroll past, so they need the same
    // wrapping sampler as the beam.
    for (key, path) in [
        (rain_tex, "environment/rain"),
        (snow_tex, "environment/snow"),
    ] {
        if let Ok(img) = pack.texture_png(path) {
            tiled_textures.push((key, img));
        }
    }
    // The world border's wall: the same tiled treatment, scrolling upward.
    let border_tex = fnv64(b"forcefield");
    if let Ok(img) = pack.texture_png("misc/forcefield") {
        tiled_textures.push((border_tex, img));
    }

    // Entity shadow blob (`misc/shadow.png`): a white radial gradient whose
    // alpha is the whole shape. Drawn black, so it darkens the ground exactly
    // like vanilla's per-block shadow projection.
    let shadow_tex = fnv64(b"shadow");
    if let Ok(img) = pack.texture_png("misc/shadow") {
        mob_textures.push((shadow_tex, img));
    }

    // Filled maps: the wooden background sheet plus every marker sprite. The
    // map contents themselves come from the server, one patch at a time.
    let mut map_store = maps::MapStore::default();
    {
        let background = pack.texture_png("map/map_background").ok();
        let mut decorations = HashMap::new();
        const DECORATIONS: &[&str] = &[
            "player",
            "frame",
            "red_marker",
            "blue_marker",
            "target_x",
            "target_point",
            "player_off_map",
            "player_off_limits",
            "woodland_mansion",
            "ocean_monument",
            "red_x",
            "desert_village",
            "plains_village",
            "savanna_village",
            "snowy_village",
            "taiga_village",
            "jungle_temple",
            "swamp_hut",
            "trial_chambers",
            "white_banner",
            "orange_banner",
            "magenta_banner",
            "light_blue_banner",
            "yellow_banner",
            "lime_banner",
            "pink_banner",
            "gray_banner",
            "light_gray_banner",
            "cyan_banner",
            "purple_banner",
            "blue_banner",
            "brown_banner",
            "green_banner",
            "red_banner",
            "black_banner",
        ];
        for name in DECORATIONS {
            if let Ok(img) = pack.texture_png(&format!("map/decorations/{name}")) {
                decorations.insert((*name).to_string(), img);
            }
        }
        info!(
            background = background.is_some(),
            markers = decorations.len(),
            "app: map textures loaded"
        );
        map_store.set_textures(background, decorations);
    }

    // The vanilla bitmap font, for sign text drawn onto a texture.
    let font = crate::assets::font::Font::load(&mut pack);
    info!(loaded = font.is_loaded(), "app: vanilla bitmap font");

    // Particle sprite atlas + per-family frame UVs (billboarded at draw time).
    let (particle_atlas, particle_atlas_uv) = build_particle_atlas(&mut pack);
    info!(
        families = particle_atlas_uv.len(),
        "app: particle atlas built"
    );

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

    let mut skins = SkinManager::new(opts.assets_dir.as_deref());
    let local_skin =
        opts.local_cosmetics
            .skin
            .as_deref()
            .and_then(|path| match skins.load_local_skin(path) {
                Ok(url) => Some((url, opts.local_cosmetics.slim)),
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "app: local skin rejected");
                    None
                }
            });
    let local_cape =
        opts.local_cosmetics
            .cape
            .as_deref()
            .and_then(|path| match skins.load_local_cape(path) {
                Ok(url) => Some(url),
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "app: local cape rejected");
                    None
                }
            });

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
            block_state_by_name
                .entry(e.short_name.clone())
                .or_insert(id);
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
        local_packs,
        server_packs: Vec::new(),
        resource_pack_status: None,
        table: Arc::new(table),
        store: Arc::new(store),
        atlas,
        atlas_anim,
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
        painting_tex,
        painting_back_tex,
        item_frame_tex,
        glow_item_frame_tex,
        arrow_tex,
        arrow_spectral_tex,
        xp_orb_tex,
        fish_base_tex,
        fish_pat_tex,
        cat_collar_tex,
        wolf_collar_tex,
        creeper_armor_tex,
        elytra_tex,
        animal_equipment,
        fire_tex,
        fire_frames,
        sheep_wool_tex,
        ghast_shooting_tex,
        rain_tex,
        snow_tex,
        bobber_tex,
        beam_tex,
        tiled_textures,
        beacons: Vec::new(),
        block_entities: blockentities::BlockEntities::default(),
        open_containers: HashMap::new(),
        lids: lids::Lids::default(),
        pistons: pistons::Pistons::default(),
        font,
        lightning: Vec::new(),
        spawn_pos: None,
        shadow_tex,
        particle_atlas: Some(particle_atlas),
        particle_atlas_uv,
        mob_textures,
        panorama,
        panorama_loaded: false,
        biome_tints: Arc::new(crate::types::BiomeTints::default()),
        biomes: Vec::new(),
        boss_bars: Vec::new(),
        maps: map_store,
        map_tex: HashMap::new(),
        map_egui: HashMap::new(),
        border: Default::default(),
        border_tex,
        border_since: Instant::now(),
        block_destruction: HashMap::new(),
        camera_entity: None,
        container_data: HashMap::new(),
        container_data_id: -1,
        enchantments: Arc::new(Vec::new()),
        trim_patterns: Arc::new(Vec::new()),
        trim_materials: Arc::new(Vec::new()),
        instruments: Arc::new(Vec::new()),
        trim_tex: HashMap::new(),
        pending_trims: Vec::new(),
        ambient_rng: ambient::Rng::new(0x5EED_1234_ABCD_0001),
        ambient_accum: 0.0,
        music: music::MusicDirector::default(),
        mood: music::MoodMeter::default(),
        ambient_accum_mood: 0.0,
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
        local_skin,
        local_cape,
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
        smooth_lighting_meshed: settings.smooth_lighting,
        settings,
        settings_dirty: true,
        last_frame_end: Instant::now(),
        last_frame: Instant::now(),
        bob_phase: 0.0,
        fov_mult: 1.0,
        dim_skylight: true,
        dim_ultrawarm: false,
        dim_ambient: 0.0,
        dim_name: "overworld".to_string(),
        mining_target: None,
        mining_recent: None,
        mine_hit_counter: 0,
        pending_place: None,
        air_seen: false,
        keys: HashSet::new(),
        last_move: (0, 0, false),
        own_steps: footsteps::StepTracker::default(),
        totem_flash: None,
        sneaking: false,
        sneak_latch: false,
        sprint_latch: false,
        forward_since: None,
        last_jump_tap: None,
        ride_jump: riding::RideJump::default(),
        sleep_since: None,
        show_hitboxes: false,
        show_chunk_borders: false,
        mount: None,
        auto_jump_until: None,
        yaw: 0.0,
        pitch: 20.0,
        dir_synced: false,
        last_sent_dir: None,
        pending_mouse: (0.0, 0.0),
        gamepad: gamepad::GamepadInput::new(),
        narrator: crate::narrator::Narrator::spawn(),
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
        waypoints: HashMap::new(),
        server_reduced_debug_info: false,
        rain_level: 0.0,
        thunder_level: 0.0,
        active_effects: HashMap::new(),
        cooldowns: HashMap::new(),
        effect_tex: HashMap::new(),
        loom_tex: HashMap::new(),
        stonecutter: Arc::new(Vec::new()),
        rain_drops: Vec::new(),
        particle_rng: 0x9E37_79B9_7F4A_7C15,
        light_flicker: 0.0,
        cloud_accum: 0.0,
        delayed_sounds: Vec::new(),
        flicker_accum: 0.0,
        last_health: -1.0,
        hurt_flash_until: None,
        hurt_at: None,
        hurt_from_yaw: f32::NAN,
        nausea_mix: 0.0,
        frame_times: VecDeque::with_capacity(FPS_WINDOW + 1),
        fps_display: 0.0,
        fps_updated: Instant::now(),
        start: Instant::now(),
        last_stats: (0, 0),
        frame_counter: 0,
        fatal: None,
        singleplayer: None,
        singleplayer_starting: None,
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
/// Per-tick lag-follow + wobble accumulator for cape sway — ported from real
/// vanilla `ClientAvatarState` (`moveCloak`/`updateBob`, decompiled from the
/// 26.1 client jar) and `AvatarRenderer.extractCapeState`. Shared by remote
/// players (`EntityTrack`) and the local player (`CamTrack`); every real
/// player-shaped entity in vanilla carries exactly one of these.
#[derive(Clone, Copy, Debug)]
struct CapeLag {
    lag: [f64; 3],
    lag_prev: [f64; 3],
    actual: [f64; 3],
    actual_prev: [f64; 3],
    bob: f32,
    bob_prev: f32,
    last_tick_at: Instant,
}

impl CapeLag {
    fn at(pos: [f64; 3], now: Instant) -> Self {
        Self {
            lag: pos,
            lag_prev: pos,
            actual: pos,
            actual_prev: pos,
            bob: 0.0,
            bob_prev: 0.0,
            last_tick_at: now,
        }
    }

    /// One real tick: real `ClientAvatarState.moveCloak` — per-axis 0.25
    /// lag-follow with a ±10-block-per-axis teleport snap (NOT a combined
    /// Euclidean threshold; confirmed by decompile, this is what actually
    /// gives a fast-teleporting player's cape a clean cut instead of a
    /// glide) — plus a bob accumulator standing in for
    /// `AbstractClientPlayer.updateBob`. Real vanilla gates bob's target on
    /// onGround/dead/swimming; this uses tick-to-tick horizontal distance
    /// alone (already in blocks/tick, the same unit as vanilla's own
    /// `getDeltaMovement().horizontalDistance()`) — a documented
    /// approximation for this secondary wobble term only. The primary
    /// lag-follow lean/flap computed from `lag`/`actual` below is an exact
    /// port with no approximation.
    fn tick(&mut self, pos: [f64; 3], now: Instant) {
        self.actual_prev = self.actual;
        self.lag_prev = self.lag;
        for i in 0..3 {
            let d = pos[i] - self.lag[i];
            if !(-10.0..=10.0).contains(&d) {
                self.lag[i] = pos[i];
                self.lag_prev[i] = pos[i];
            } else {
                self.lag[i] += d * 0.25;
            }
        }
        let horiz =
            ((pos[0] - self.actual[0]).powi(2) + (pos[2] - self.actual[2]).powi(2)).sqrt();
        self.actual = pos;
        self.bob_prev = self.bob;
        let target = (horiz as f32).min(0.1);
        self.bob += (target - self.bob) * 0.4;
        self.last_tick_at = now;
    }

    /// How far into the current tick interval `now` is, 0..1 — vanilla's own
    /// `partialTicks` concept (elapsed-since-last-tick over a nominal 50 ms
    /// tick), used to interpolate every tick-stepped quantity above.
    fn frac(&self, now: Instant) -> f32 {
        (now.duration_since(self.last_tick_at).as_secs_f32() / 0.05).clamp(0.0, 1.0)
    }
}

/// Real `AvatarRenderer.extractCapeState`'s flap/lean/lean2, in degrees, fed
/// straight into `PlayerCapeModel.setupAnim`'s rotation. `wobble_phase` stands
/// in for `sin(walkDistance * 6)` — this reuses the engine's existing
/// per-entity leg-swing phase (the same walked-distance concept vanilla's own
/// `walkDistance` is, already tracked for limb swing) rather than adding a
/// second, parallel accumulator whose exact real scale constant was not
/// findable within this decompile pass. `fallFlyingScale` is always treated
/// as 0 here: this client never draws a cape while the elytra wings are out
/// (the draw code already prefers wings over the cape, so this branch is
/// simply never reached while it would matter).
/// Vanilla's exact wall-proximity fade (`WorldBorderRenderer.extract`):
/// `pow(1 - distanceToBorder / renderDistance, 4)`, clamped 0..1 — the wall
/// fades smoothly into view as you approach it and disappears once you're
/// well clear of it, instead of popping fully opaque into existence at a
/// fixed distance.
fn border_wall_alpha(dist_to_border: f64, render_distance_blocks: f64) -> f32 {
    (1.0 - dist_to_border / render_distance_blocks)
        .powi(4)
        .clamp(0.0, 1.0) as f32
}

/// Milliseconds per real Minecraft server tick (a constant 20 TPS).
const MC_TICK_MS: f64 = 50.0;

/// The world border's size mid-lerp. `lerp_time_ticks` is the raw tick count
/// straight off `ClientboundSetBorderLerpSize`/`ClientboundInitializeBorder`
/// (confirmed via decompile of `WorldBorder.MovingBorderExtent`: its
/// `lerpProgress` is decremented once per call to `WorldBorder.tick()`, which
/// runs once per server tick — the packet's `lerpTime` field is that same
/// counter's value at the moment the move starts) — real time is
/// `lerp_time_ticks * 50ms`, not `lerp_time_ticks` milliseconds directly.
fn border_interpolated_size(old_size: f64, new_size: f64, lerp_time_ticks: u64, elapsed_ms: f64) -> f64 {
    if lerp_time_ticks == 0 {
        return new_size;
    }
    let duration_ms = lerp_time_ticks as f64 * MC_TICK_MS;
    let t = (elapsed_ms / duration_ms).clamp(0.0, 1.0);
    old_size + (new_size - old_size) * t
}

/// Vanilla's border-move speed, in blocks per tick (`WorldBorder.getLerpSpeed`
/// on a moving border: `|from-to| / durationTicks`; 0 while stationary).
fn border_lerp_speed(old_size: f64, new_size: f64, lerp_time_ticks: u64) -> f64 {
    if lerp_time_ticks == 0 {
        0.0
    } else {
        (old_size - new_size).abs() / lerp_time_ticks as f64
    }
}

/// Vanilla's real warning-vignette trigger distance (`Gui.extractVignette`,
/// decompiled): a border that's about to move a lot in the next
/// `warning_time` warns further out than the plain `warning_blocks` radius
/// alone would. Note `lerp_speed` is blocks-PER-TICK multiplied directly by
/// `warning_time` (a raw count, not converted to ticks) — that mismatch is
/// vanilla's own real formula, confirmed byte-for-byte in the decompiled
/// class, not a unit bug to "fix": porting it faithfully means keeping it.
fn border_warning_distance(
    warning_blocks: u32,
    lerp_speed_blocks_per_tick: f64,
    warning_time: u32,
    lerp_target: f64,
    current_size: f64,
) -> f64 {
    let moving_blocks_threshold =
        (lerp_speed_blocks_per_tick * warning_time as f64).min((lerp_target - current_size).abs());
    (warning_blocks as f64).max(moving_blocks_threshold)
}

/// Vanilla's exact `BorderStatus` colours (`BorderStatus.getColor()`, a
/// 24-bit packed RGB int per status) — blue while it sits still, green
/// while it grows, red while it closes in.
fn border_status_color(growing: bool, shrinking: bool) -> [f32; 3] {
    if growing {
        [64.0 / 255.0, 255.0 / 255.0, 128.0 / 255.0] // 0x40FF80
    } else if shrinking {
        [255.0 / 255.0, 48.0 / 255.0, 48.0 / 255.0] // 0xFF3030
    } else {
        [32.0 / 255.0, 160.0 / 255.0, 255.0 / 255.0] // 0x20A0FF
    }
}

fn cape_flap_lean(cape: &CapeLag, now: Instant, body_yaw: f32, wobble_phase: f32) -> (f32, f32, f32) {
    let t = cape.frac(now) as f64;
    let lerp3 = |a: [f64; 3], b: [f64; 3]| {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    let lag = lerp3(cape.lag_prev, cape.lag);
    let actual = lerp3(cape.actual_prev, cape.actual);
    let (dx, dy, dz) = (lag[0] - actual[0], lag[1] - actual[1], lag[2] - actual[2]);
    let yaw_r = body_yaw.to_radians();
    let (fx, fz) = (yaw_r.sin(), -yaw_r.cos());
    let flap0 = (dy as f32 * 10.0).clamp(-6.0, 32.0);
    let lean = ((dx as f32 * fx + dz as f32 * fz) * 100.0).clamp(0.0, 150.0);
    let lean2 = ((dx as f32 * fz - dz as f32 * fx) * 100.0).clamp(-20.0, 20.0);
    let bob = cape.bob_prev + (cape.bob - cape.bob_prev) * t as f32;
    let flap = flap0 + wobble_phase.sin() * 32.0 * bob;
    (flap, lean, lean2)
}

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
    /// When this entity died. Vanilla tips a dying mob onto its side over
    /// 20 ticks before the server removes it.
    death_start: Option<Instant>,
    /// Somebody picked this item up: when, and which entity took it. Vanilla
    /// keeps the item on screen for three more ticks and flies it into the
    /// collector (its `ItemPickupParticle`).
    pickup: Option<(Instant, u64)>,
    /// When this creeper's fuse was lit, so the swell and the white flash can
    /// ramp from there.
    swell_start: Option<Instant>,
    /// How stretched this body is right now (positive) or how flattened
    /// (negative) — a slime springing up and landing again.
    squish: f32,
    /// Its footsteps: the same counter vanilla runs for every entity, so other
    /// people and animals are heard walking around.
    steps: footsteps::StepTracker,
    /// When this entity was first seen — a stand-in for vanilla's per-entity
    /// age, for the handful of things (a shulker bullet's tumble, an ominous
    /// item spawner's grow-in) whose real formula runs off age rather than
    /// anything the server keeps re-sending.
    spawned_at: Instant,
    /// When an evoker's fangs got the "attack" entity-event (status 4):
    /// vanilla renders nothing until this fires, then bites shut and bursts
    /// upward over the following second.
    bite_start: Option<Instant>,
    /// Cape lag-follow physics (players only; harmless to compute for every
    /// entity, cape is only ever drawn when the snapshot actually has one).
    cape: CapeLag,
    /// An allay's client-tick dance/spin counters (`Allay.tick()`'s
    /// client-side branch, decompiled 0.119.0): `dance_ticks` counts up every
    /// tick while `Dancing` holds and resets to 0 the instant it doesn't;
    /// `spin_ticks` eases 0..15 over the first/last third of each 55-tick
    /// dance cycle (`spin_ticks % 55.0 < 15.0`). `_prev` pairs are the prior
    /// tick's value, for the same tick-to-frame interpolation `CapeLag` uses.
    dance_ticks: f32,
    spin_ticks: f32,
    spin_ticks_prev: f32,
    /// A panda's roll/on-back ease-in-ease-out amounts
    /// (`Panda.updateRollAmount`/`updateOnBackAnimation`, decompiled
    /// 0.119.0): +0.15/tick toward 1 while the flag holds, -0.19/tick toward
    /// 0 otherwise, clamped to [0, 1]. `_prev` for interpolation.
    roll_amount: f32,
    roll_amount_prev: f32,
    on_back_amount: f32,
    on_back_amount_prev: f32,
    /// A fox's faceplant leg-scramble phase (`FoxModel`'s `legMotionPos`,
    /// decompiled 0.119.0). Real vanilla advances this by a fixed 0.67 once
    /// per RENDER frame (not per tick) — approximated here as a real-time
    /// rate assuming a 60 fps reference (`+= dt * 40.2`) so the wiggle's
    /// speed doesn't literally depend on this client's own frame rate the
    /// way vanilla's does; documented simplification, not a bug.
    leg_motion_pos: f32,
}

/// Composite one armour trim: the pattern sheet with vanilla's greyscale key
/// palette swapped for the material's own colours.
fn build_trim(
    pack: &mut AssetPack,
    pattern: &str,
    material: &str,
    leggings: bool,
) -> Option<image::RgbaImage> {
    let layer = if leggings {
        "humanoid_leggings"
    } else {
        "humanoid"
    };
    let mut sheet = pack
        .texture_png(&format!("trims/entity/{layer}/{pattern}"))
        .ok()?;
    let key_palette = pack.texture_png("trims/color_palettes/trim_palette").ok()?;
    let colors = pack
        .texture_png(&format!("trims/color_palettes/{material}"))
        .ok()?;
    // The palettes are 8×1 strips: the Nth key colour maps to the Nth material
    // colour, which is the whole of vanilla's trim recolouring.
    let keys: Vec<[u8; 4]> = key_palette.pixels().map(|p| p.0).collect();
    let values: Vec<[u8; 4]> = colors.pixels().map(|p| p.0).collect();
    if keys.is_empty() || values.len() < keys.len() {
        return None;
    }
    for px in sheet.pixels_mut() {
        if px.0[3] == 0 {
            continue;
        }
        match keys.iter().position(|k| k[..3] == px.0[..3]) {
            Some(i) => {
                let v = values[i];
                px.0 = [v[0], v[1], v[2], px.0[3]];
            }
            // A pixel outside the key palette is not part of the trim.
            None => px.0[3] = 0,
        }
    }
    Some(sheet)
}

/// Look up the four composited trim textures for one set of armour trims.
fn trim_keys(
    lookup: &HashMap<(String, String, bool), Option<u64>>,
    trims: &[Option<(String, String)>; 4],
) -> [Option<u64>; 4] {
    // Only leggings use the second armour layer; head/chest/feet share the first.
    let mut out = [None; 4];
    for (slot, trim) in trims.iter().enumerate() {
        let Some((pattern, material)) = trim else {
            continue;
        };
        let key = (pattern.clone(), material.clone(), slot == 2);
        out[slot] = lookup.get(&key).copied().flatten();
    }
    out
}

/// The colour of the dust a body kicks up off this block, so a sprint across
/// sand puffs yellow and one across grass puffs green. Vanilla shows actual
/// pieces of the block's texture; a puff in its colour reads the same at
/// playing distance, and is what our particle atlas can draw.
fn block_dust_color(short_name: &str) -> [f32; 3] {
    match blocksound::group(short_name) {
        "grass" | "vine" | "nether_sprouts" => [0.42, 0.55, 0.28],
        "sand" => [0.83, 0.78, 0.55],
        "gravel" => [0.50, 0.46, 0.43],
        "snow" | "powder_snow" => [0.92, 0.94, 0.98],
        "wood" | "bamboo" | "ladder" | "wood_hanging_sign" => [0.55, 0.43, 0.27],
        "wool" => [0.85, 0.85, 0.85],
        "netherrack" | "nether_bricks" | "nylium" => [0.42, 0.20, 0.20],
        "soul_sand" | "soul_soil" => [0.36, 0.28, 0.22],
        "basalt" | "deepslate" => [0.30, 0.30, 0.33],
        "glass" | "amethyst_block" => [0.75, 0.82, 0.88],
        _ => [0.55, 0.50, 0.45],
    }
}

/// Vanilla's death animation length: 20 ticks, i.e. one second.
const DEATH_ANIM: Duration = Duration::from_millis(1000);

/// How long a picked-up item flies into its collector (vanilla's
/// `ItemPickupParticle` lives three ticks).
const PICKUP_ANIM: Duration = Duration::from_millis(150);

impl EntityTrack {
    fn new(snap: EntitySnapshot, now: Instant) -> Self {
        let mut hist = VecDeque::with_capacity(8);
        let cape = CapeLag::at(snap.pos, now);
        hist.push_back((now, snap.pos, snap.yaw, snap.pitch));
        Self {
            hist,
            snap,
            phase: 0.0,
            amp: 0.0,
            last_render: None,
            hurt_until: None,
            swing_start: None,
            death_start: None,
            pickup: None,
            swell_start: None,
            squish: 0.0,
            steps: footsteps::StepTracker::default(),
            spawned_at: now,
            bite_start: None,
            cape,
            dance_ticks: 0.0,
            spin_ticks: 0.0,
            spin_ticks_prev: 0.0,
            roll_amount: 0.0,
            roll_amount_prev: 0.0,
            on_back_amount: 0.0,
            on_back_amount_prev: 0.0,
            leg_motion_pos: 0.0,
        }
    }

    /// How far into the death animation this entity is, 0..1 (0 = alive).
    fn death_progress(&self, now: Instant) -> f32 {
        match self.death_start {
            Some(t) => {
                (now.duration_since(t).as_secs_f32() / DEATH_ANIM.as_secs_f32()).clamp(0.0, 1.0)
            }
            None => 0.0,
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
        self.cape.tick(snap.pos, now);
        // Allay dance/spin: real `Allay.tick()`'s client-side branch.
        if matches!(snap.pose_kind, crate::bridge::events::AnimalPose::Dancing) {
            self.dance_ticks += 1.0;
            let is_spinning = self.dance_ticks % 55.0 < 15.0;
            self.spin_ticks_prev = self.spin_ticks;
            self.spin_ticks += if is_spinning { 1.0 } else { -1.0 };
            self.spin_ticks = self.spin_ticks.clamp(0.0, 15.0);
        } else {
            self.dance_ticks = 0.0;
            self.spin_ticks = 0.0;
            self.spin_ticks_prev = 0.0;
        }
        // Panda roll/on-back: real `Panda.updateRollAmount`/`updateOnBackAnimation`.
        let is_rolling = matches!(snap.pose_kind, crate::bridge::events::AnimalPose::Rolling);
        let is_on_back = matches!(snap.pose_kind, crate::bridge::events::AnimalPose::OnBack);
        self.roll_amount_prev = self.roll_amount;
        self.roll_amount = if is_rolling {
            (self.roll_amount + 0.15).min(1.0)
        } else {
            (self.roll_amount - 0.19).max(0.0)
        };
        self.on_back_amount_prev = self.on_back_amount;
        self.on_back_amount = if is_on_back {
            (self.on_back_amount + 0.15).min(1.0)
        } else {
            (self.on_back_amount - 0.19).max(0.0)
        };
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
                return (
                    pos,
                    lerp_angle(y0, y1, a as f32),
                    pi0 + (pi1 - pi0) * a as f32,
                );
            }
        }
        (last.1, last.2, last.3)
    }
}

/// One falling raindrop or snowflake around the player.
struct RainDrop {
    pos: [f64; 3],
    /// Fall speed in blocks/second (snow uses a fraction of it).
    speed: f32,
    /// Snow rather than rain: this column's biome is cold enough.
    snow: bool,
    /// Scrolls the streak (rain) or drives the drift (snow).
    phase: f32,
}

/// One live particle, simulated on the CPU and drawn as a camera-facing
/// textured billboard.
struct Particle {
    pos: [f64; 3],
    vel: [f64; 3],
    /// Texture family (→ atlas UVs); animated families pick a frame by age.
    tex: ParticleTex,
    /// RGB tint (multiplies the texture; white for most textured particles).
    color: [f32; 3],
    size: f32,
    age: f32,
    life: f32,
    /// Downward acceleration (blocks/s²); can be negative for floaty particles.
    gravity: f32,
    /// A colour to cross into over the second half of life. Firework stars are
    /// the only thing that uses it — a star made with a fade dye changes colour
    /// in the air, which is most of what makes fireworks look like fireworks.
    fade_to: Option<[f32; 3]>,
    /// Set for `Item`/`ItemSlime`/`ItemCobweb`/`ItemSnowball` particles: the
    /// real item's icon UV in the item atlas, resolved once at spawn time.
    /// When set, this billboards that icon instead of `tex`'s atlas frame.
    item_uv: Option<[f32; 4]>,
}

/// Pack the vanilla particle sprites into one grid atlas and record, per
/// texture family, the UV rect of each animation frame (in atlas 0..1 space,
/// top-left origin). Each source sprite is scaled to a fixed cell so every UV is
/// a full cell. Missing sprites become transparent cells (drawn as nothing).
fn build_particle_atlas(
    pack: &mut AssetPack,
) -> (image::RgbaImage, HashMap<ParticleTex, Vec<[f32; 4]>>) {
    use image::imageops::{FilterType, resize};
    const CELL: u32 = 16;
    const COLS: u32 = 12;
    // (family, frame sprite names under textures/particle/).
    let families: &[(ParticleTex, &[&str])] = &[
        (
            ParticleTex::Generic,
            &[
                "generic_0",
                "generic_1",
                "generic_2",
                "generic_3",
                "generic_4",
                "generic_5",
                "generic_6",
                "generic_7",
            ],
        ),
        (ParticleTex::Flame, &["flame"]),
        (ParticleTex::SoulFlame, &["soul_fire_flame"]),
        (ParticleTex::Lava, &["lava"]),
        (
            ParticleTex::Smoke,
            &[
                "big_smoke_0",
                "big_smoke_1",
                "big_smoke_2",
                "big_smoke_3",
                "big_smoke_4",
                "big_smoke_5",
                "big_smoke_6",
                "big_smoke_7",
                "big_smoke_8",
                "big_smoke_9",
                "big_smoke_10",
                "big_smoke_11",
            ],
        ),
        (ParticleTex::Crit, &["critical_hit"]),
        (ParticleTex::EnchantedHit, &["enchanted_hit"]),
        (ParticleTex::Damage, &["damage"]),
        (ParticleTex::Heart, &["heart"]),
        (ParticleTex::Angry, &["angry"]),
        (ParticleTex::Happy, &["glint"]),
        (
            ParticleTex::Effect,
            &[
                "effect_0", "effect_1", "effect_2", "effect_3", "effect_4", "effect_5", "effect_6",
                "effect_7",
            ],
        ),
        (ParticleTex::Note, &["note"]),
        (ParticleTex::Bubble, &["bubble"]),
        (
            ParticleTex::Splash,
            &["splash_0", "splash_1", "splash_2", "splash_3"],
        ),
        (ParticleTex::Drip, &["drip_hang"]),
        (
            ParticleTex::Explosion,
            &[
                "explosion_0",
                "explosion_1",
                "explosion_2",
                "explosion_3",
                "explosion_4",
                "explosion_5",
                "explosion_6",
                "explosion_7",
                "explosion_8",
                "explosion_9",
                "explosion_10",
                "explosion_11",
                "explosion_12",
                "explosion_13",
                "explosion_14",
                "explosion_15",
            ],
        ),
        (ParticleTex::Flash, &["flash"]),
        (ParticleTex::Glow, &["glow"]),
        // No dedicated sprite — reuse a soft generic blob (tinted at draw time).
        (ParticleTex::Portal, &["generic_0"]),
        (ParticleTex::Dust, &["generic_0"]),
        (
            ParticleTex::Cherry,
            &["cherry_0", "cherry_1", "cherry_2", "cherry_3"],
        ),
        (ParticleTex::Leaf, &["leaf_0", "leaf_1", "leaf_2", "leaf_3"]),
        (
            ParticleTex::PaleOak,
            &["pale_oak_0", "pale_oak_1", "pale_oak_2", "pale_oak_3"],
        ),
        (ParticleTex::Nautilus, &["nautilus"]),
        (
            ParticleTex::SculkSoul,
            &["sculk_soul_0", "sculk_soul_1", "sculk_soul_2"],
        ),
        (ParticleTex::Soul, &["soul_0", "soul_1", "soul_2", "soul_3"]),
        (
            ParticleTex::Spark,
            &[
                "spark_0", "spark_1", "spark_2", "spark_3", "spark_4", "spark_5", "spark_6",
                "spark_7",
            ],
        ),
        (ParticleTex::Firefly, &["firefly"]),
        // 0.98.0 — particle-accuracy pass: real textures for particle kinds
        // that were previously falling back to a plain grey dot.
        (
            ParticleTex::Glitter,
            &[
                "glitter_0", "glitter_1", "glitter_2", "glitter_3", "glitter_4", "glitter_5",
                "glitter_6", "glitter_7",
            ],
        ),
        (
            ParticleTex::Spell,
            &[
                "spell_0", "spell_1", "spell_2", "spell_3", "spell_4", "spell_5", "spell_6",
                "spell_7",
            ],
        ),
        (
            ParticleTex::Gust,
            &[
                "gust_0", "gust_1", "gust_2", "gust_3", "gust_4", "gust_5", "gust_6", "gust_7",
                "gust_8", "gust_9", "gust_10", "gust_11",
            ],
        ),
        (
            ParticleTex::SmallGust,
            &[
                "small_gust_0", "small_gust_1", "small_gust_2", "small_gust_3", "small_gust_4",
                "small_gust_5", "small_gust_6",
            ],
        ),
        (
            ParticleTex::SonicBoom,
            &[
                "sonic_boom_0", "sonic_boom_1", "sonic_boom_2", "sonic_boom_3", "sonic_boom_4",
                "sonic_boom_5", "sonic_boom_6", "sonic_boom_7", "sonic_boom_8", "sonic_boom_9",
                "sonic_boom_10", "sonic_boom_11", "sonic_boom_12", "sonic_boom_13",
                "sonic_boom_14", "sonic_boom_15",
            ],
        ),
        (
            ParticleTex::SculkCharge,
            &[
                "sculk_charge_0", "sculk_charge_1", "sculk_charge_2", "sculk_charge_3",
                "sculk_charge_4", "sculk_charge_5", "sculk_charge_6",
            ],
        ),
        (
            ParticleTex::SculkChargePop,
            &["sculk_charge_pop_0", "sculk_charge_pop_1", "sculk_charge_pop_2", "sculk_charge_pop_3"],
        ),
        (
            ParticleTex::Sweep,
            &[
                "sweep_0", "sweep_1", "sweep_2", "sweep_3", "sweep_4", "sweep_5", "sweep_6",
                "sweep_7",
            ],
        ),
        (
            ParticleTex::BubblePop,
            &["bubble_pop_0", "bubble_pop_1", "bubble_pop_2", "bubble_pop_3", "bubble_pop_4"],
        ),
        (ParticleTex::Infested, &["infested"]),
        (ParticleTex::Vibration, &["vibration"]),
        (ParticleTex::Shriek, &["shriek"]),
        (ParticleTex::VaultConnection, &["vault_connection"]),
        (ParticleTex::RaidOmen, &["raid_omen"]),
        (ParticleTex::TrialOmen, &["trial_omen"]),
        (ParticleTex::OminousSpawning, &["ominous_spawning"]),
        (
            ParticleTex::TrialSpawnerDetection,
            &[
                "trial_spawner_detection_0", "trial_spawner_detection_1",
                "trial_spawner_detection_2", "trial_spawner_detection_3",
                "trial_spawner_detection_4",
            ],
        ),
        (
            ParticleTex::TrialSpawnerDetectionOminous,
            &[
                "trial_spawner_detection_ominous_0", "trial_spawner_detection_ominous_1",
                "trial_spawner_detection_ominous_2", "trial_spawner_detection_ominous_3",
                "trial_spawner_detection_ominous_4",
            ],
        ),
        (
            ParticleTex::Enchant,
            &[
                "sga_a", "sga_b", "sga_c", "sga_d", "sga_e", "sga_f", "sga_g", "sga_h", "sga_i",
                "sga_j", "sga_k", "sga_l", "sga_m", "sga_n", "sga_o", "sga_p", "sga_q", "sga_r",
                "sga_s", "sga_t", "sga_u", "sga_v", "sga_w", "sga_x", "sga_y", "sga_z",
            ],
        ),
    ];
    let mut cells: Vec<image::RgbaImage> = Vec::new();
    let mut idx_map: HashMap<ParticleTex, Vec<u32>> = HashMap::new();
    for (tex, frames) in families {
        let mut idxs = Vec::new();
        for name in *frames {
            let cell = match pack.texture_png(&format!("particle/{name}")) {
                Ok(im) => resize(&im, CELL, CELL, FilterType::Nearest),
                Err(_) => image::RgbaImage::new(CELL, CELL),
            };
            idxs.push(cells.len() as u32);
            cells.push(cell);
        }
        idx_map.insert(*tex, idxs);
    }
    let rows = cells.len().div_ceil(COLS as usize).max(1) as u32;
    let mut atlas = image::RgbaImage::new(COLS * CELL, rows * CELL);
    for (i, cell) in cells.iter().enumerate() {
        let (cx, cy) = (i as u32 % COLS * CELL, i as u32 / COLS * CELL);
        image::imageops::overlay(&mut atlas, cell, cx as i64, cy as i64);
    }
    let (aw, ah) = (COLS * CELL, rows * CELL);
    let uv_of = |idx: u32| -> [f32; 4] {
        let (col, row) = (idx % COLS, idx / COLS);
        [
            (col * CELL) as f32 / aw as f32,
            (row * CELL) as f32 / ah as f32,
            ((col + 1) * CELL) as f32 / aw as f32,
            ((row + 1) * CELL) as f32 / ah as f32,
        ]
    };
    let map = idx_map
        .into_iter()
        .map(|(tex, idxs)| (tex, idxs.into_iter().map(uv_of).collect()))
        .collect();
    (atlas, map)
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
    /// Our own cape's lag-follow physics (see `CapeLag`).
    cape: CapeLag,
}

/// The mount inventory the server opened for the animal under us. It never
/// goes through azalea's menus (there is no horse menu), so the app keeps the
/// little it needs to click and close it.
#[derive(Clone, Debug)]
struct MountScreen {
    container_id: i32,
    /// The animal's registry name, which decides whether the armour slot
    /// shows a llama's carpet or a horse's barding.
    kind: String,
}

struct App {
    opts: AppOptions,
    /// Kept open for language reloads.
    pack: AssetPack,
    /// User-selected local pack stack (low → high priority).
    local_packs: resourcepacks::LocalPackStore,
    /// Server stack in push order. A later push has higher priority.
    server_packs: Vec<(uuid::Uuid, PathBuf)>,
    /// Human-readable download state for the connecting screen.
    resource_pack_status: Option<String>,
    table: Arc<BlockTable>,
    store: Arc<BakedModelStore>,
    atlas: Atlas,
    /// Drives the 50-odd animated block sprites (water, lava, fire, portal,
    /// sea lantern, sculk, command blocks, …) by rewriting their rectangles in
    /// the atlas texture once per game tick, exactly like vanilla.
    atlas_anim: AtlasAnimator,
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
    /// Painting art texture key by asset name (e.g. "kebab"); the wooden back is
    /// `painting_back_tex`. Both are uploaded via `mob_textures`.
    painting_tex: HashMap<String, u64>,
    painting_back_tex: u64,
    /// Item-frame face textures (normal + glow); the back reuses `painting_back_tex`.
    item_frame_tex: u64,
    glow_item_frame_tex: u64,
    /// Projectile textures (arrow, spectral arrow), drawn on crossed planes.
    arrow_tex: u64,
    arrow_spectral_tex: u64,
    xp_orb_tex: u64,
    /// Tropical-fish base body textures by shape index (0 = A/small, 1 = B/large),
    /// tinted per-fish by the body colour.
    fish_base_tex: [u64; 2],
    /// Tropical-fish pattern overlays `[shape][pattern 0..5]`, tinted by the
    /// pattern colour and drawn over the body.
    fish_pat_tex: [[u64; 6]; 2],
    /// Collar overlay textures for tamed cats/wolves, tinted by the dye colour.
    cat_collar_tex: u64,
    wolf_collar_tex: u64,
    /// Charged-creeper energy-swirl overlay.
    creeper_armor_tex: u64,
    /// Elytra wing texture key (0 if the jar had no wings sheet).
    elytra_tex: u64,
    /// Saddle / horse-armour / llama-carpet / wolf-armour textures by the jar
    /// path they were loaded from.
    animal_equipment: HashMap<String, u64>,
    /// Fire billboard strip (alpha-keyed) + its animation frame count.
    fire_tex: u64,
    fire_frames: u32,
    /// Sheep fleece overlay + the fishing bobber float.
    sheep_wool_tex: u64,
    /// The ghast's other face, worn while it is charging a shot.
    ghast_shooting_tex: u64,
    /// Vanilla's falling-rain and falling-snow sheets.
    rain_tex: u64,
    snow_tex: u64,
    bobber_tex: u64,
    /// Beacon beam texture, and the textures that need a wrapping sampler
    /// (taken by the renderer on first upload, like `mob_textures`).
    beam_tex: u64,
    tiled_textures: Vec<(u64, image::RgbaImage)>,
    /// Every beacon block in the loaded world, kept up to date as chunks and
    /// block updates arrive — beams are drawn from these.
    beacons: Vec<BlockPos>,
    /// Block entities (sign text, banner patterns, heads, bells, conduits,
    /// pots) and the textures composited for them.
    block_entities: blockentities::BlockEntities,
    /// Where the containers that open are, by section — collected by the mesher
    /// (which walks every block anyway) instead of by meshing them, because the
    /// client draws these itself so their lids can move.
    open_containers: HashMap<SectionPos, Vec<(BlockPos, StateId)>>,
    /// How far each of those lids has swung.
    lids: lids::Lids,
    /// Pistons mid-stroke. The server sends one "it fired" event and nothing
    /// else until the blocks land, so the client works out what moves and
    /// animates it — the same two-tick slide vanilla runs.
    pistons: pistons::Pistons,
    /// The vanilla bitmap font, used to render sign text onto a texture.
    font: crate::assets::font::Font,
    /// Lightning strikes still playing: `(position, jitter seed, struck at)`.
    lightning: Vec<([f64; 3], u64, Instant)>,
    /// World/bed spawn point, world-space X/Z — the compass needle's target.
    /// `None` until the server's first spawn-position packet arrives.
    spawn_pos: Option<[f64; 2]>,
    /// Round blob texture every entity's ground shadow is drawn with.
    shadow_tex: u64,
    /// Particle sprite atlas, taken by the renderer on first upload.
    particle_atlas: Option<image::RgbaImage>,
    /// Per-family particle frame UV rects into the atlas.
    particle_atlas_uv: HashMap<ParticleTex, Vec<[f32; 4]>>,
    /// Mob textures (skin key, image) waiting for the renderer (uploaded once).
    mob_textures: Vec<(u64, image::RgbaImage)>,
    /// Panorama faces waiting for the renderer (taken on upload).
    panorama: Option<[image::RgbaImage; 6]>,
    panorama_loaded: bool,
    /// Per-biome grass/foliage/water tint colors, built from the server biome
    /// registry (`GameEvent::Biomes`). Empty until then → plains fallback.
    biome_tints: Arc<crate::types::BiomeTints>,
    /// The server's biome registry, indexed by protocol id — kept whole for the
    /// fog/sky colours, which are looked up per frame rather than baked in.
    biomes: Vec<crate::bridge::events::BiomeInfo>,
    /// Active boss bars in the order the server announced them, keyed by its
    /// uuid. A `Vec` rather than a map because that order is what gets drawn.
    boss_bars: Vec<(u128, crate::bridge::events::BossBar)>,
    /// Every filled map the session has seen, and the composited image each
    /// one turns into.
    maps: maps::MapStore,
    /// Map id → renderer texture key, once the composite has been uploaded.
    map_tex: HashMap<u32, u64>,
    /// The same composites as egui textures, for the cartography table.
    map_egui: HashMap<u32, egui::TextureHandle>,
    /// The world border: where it is, where it is heading, and since when.
    border: crate::bridge::events::WorldBorderUpdate,
    /// Texture key of `misc/forcefield` (tiled sampler).
    border_tex: u64,
    border_since: Instant,
    /// Blocks other players are mining: entity id → (block, crack stage 0..=9).
    block_destruction: HashMap<u64, (BlockPos, u8)>,
    /// The entity the camera is attached to (`/spectate`); `None` = own body.
    camera_entity: Option<u64>,
    /// Properties of the open container (`ClientboundContainerSetData`), by
    /// property id. Cleared whenever a different container opens.
    container_data: HashMap<u16, u16>,
    /// Which container id `container_data` belongs to.
    container_data_id: i32,
    /// The server's enchantment registry, indexed by protocol id.
    enchantments: Arc<Vec<String>>,
    /// The trim registries, indexed by protocol id, for item tooltips.
    trim_patterns: Arc<Vec<String>>,
    trim_materials: Arc<Vec<String>>,
    /// The instrument registry, indexed by protocol id, for a goat horn's
    /// tooltip.
    instruments: Arc<Vec<String>>,
    /// Composited armour-trim textures, keyed by `(pattern, material, leggings
    /// layer)`. `None` means the jar had no such pattern, so we stop retrying.
    trim_tex: HashMap<(String, String, bool), Option<u64>>,
    /// Trim images composited this frame, waiting for the GPU.
    pending_trims: Vec<(u64, image::RgbaImage)>,
    /// Block-ambience state: its own RNG (so the world's idle look is
    /// reproducible) and the leftover of the frame→tick conversion.
    ambient_rng: ambient::Rng,
    ambient_accum: f32,
    /// When the next piece of music may start, and how dark it has been.
    music: music::MusicDirector,
    mood: music::MoodMeter,
    /// Frame→tick leftover for the cave-mood sampler.
    ambient_accum_mood: f32,
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
    /// Launcher-selected overrides for our own model only.
    local_skin: Option<(String, bool)>,
    local_cape: Option<String>,
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
    connect_target: Option<(String, String, ServerResourcePackPolicy)>,
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
    /// The dimension's `ambient_light` — the floor of the light ramp.
    dim_ambient: f32,
    /// Dimension-type name ("overworld", "the_nether", "the_end"), for the
    /// F3 overlay and the End's own sky.
    dim_name: String,
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
    gamepad: gamepad::GamepadInput,
    narrator: crate::narrator::Narrator,
    last_move: (i8, i8, bool),
    /// Our own step/land/splash counter (the server sends none of these).
    own_steps: footsteps::StepTracker,
    /// When a totem of undying last saved somebody in sight — vanilla flashes
    /// the totem across the whole screen for a second.
    totem_flash: Option<Instant>,
    sneaking: bool,
    /// Toggle-mode latches (Sneak/Sprint options).
    sneak_latch: bool,
    sprint_latch: bool,
    /// Since when the forward key is held (auto-jump needs "pushing a wall").
    forward_since: Option<Instant>,
    /// When the jump key last went down — two taps in a row start flying.
    last_jump_tap: Option<Instant>,
    /// The horse jump being charged by holding that same key.
    ride_jump: riding::RideJump,
    /// When we got into bed — vanilla's sleep counter, which is what the
    /// screen fades with.
    sleep_since: Option<Instant>,
    /// F3+B: every entity's bounding box and the line it is looking along.
    show_hitboxes: bool,
    /// F3+G: the borders of the chunk under the camera.
    show_chunk_borders: bool,
    /// The mount inventory the server opened: its container id, its chest
    /// columns and whose inventory it is.
    mount: Option<MountScreen>,
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
    /// Server-tracked waypoints (the locator bar), keyed by the server's own
    /// identifier — mirrors the `HashMap<u128, BossBar>` boss-bar pattern.
    waypoints: HashMap<events::WaypointKey, events::TrackedWaypointInfo>,
    /// The server's own `reducedDebugInfo` gamerule (from `ClientboundLogin`,
    /// kept live by any later `ClientboundGameRuleValues`) — vanilla's F3
    /// screen reduces itself when EITHER this or the local settings toggle
    /// is on (`Minecraft.showOnlyReducedInfo`, decompiled), not just the
    /// local one, so a server can genuinely hide coordinates from everyone.
    server_reduced_debug_info: bool,
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
    /// Loom pattern previews, keyed by (banner colour, dye colour, pattern).
    loom_tex: HashMap<String, egui::TextureHandle>,
    /// Every stonecutter recipe the server sent on join.
    stonecutter: Arc<Vec<crate::bridge::events::StonecutterRecipe>>,
    /// Falling rain and snow, recycled around the player while it is raining.
    rain_drops: Vec<RainDrop>,
    /// Cheap xorshift state for particle jitter (Math::random is fine here, but
    /// a tiny PRNG keeps spawns deterministic and dependency-free).
    particle_rng: u64,
    /// The smooth-lighting setting the loaded meshes were built with; a change
    /// forces a re-mesh.
    smooth_lighting_meshed: bool,
    /// Vanilla's block-light flicker, 0..1, eased toward a fresh random value
    /// every tick.
    light_flicker: f32,
    /// Seconds carried toward the next area-effect-cloud puff.
    cloud_accum: f32,
    /// Sounds waiting for their moment: a firework's bang arrives after the
    /// light does, because sound is slow. (when, event name, where).
    delayed_sounds: Vec<(Instant, String, [f64; 3])>,
    /// Seconds carried over toward the next flicker tick.
    flicker_accum: f32,
    /// Last seen local-player health, to detect damage (hurt flash + sound).
    last_health: f32,
    /// Until when the red damage vignette is shown; drives its fade.
    hurt_flash_until: Option<Instant>,
    /// When we were last hit, and the world-degrees direction it came from —
    /// vanilla's damage tilt rolls the camera towards it and springs back.
    hurt_at: Option<Instant>,
    hurt_from_yaw: f32,
    /// How far the nausea warp has faded in (0..1), so it ramps like vanilla's
    /// rather than snapping on with the effect.
    nausea_mix: f32,

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

    /// The local server currently running, if any (Singleplayer only).
    singleplayer: Option<crate::singleplayer::SingleplayerServer>,
    /// Set while waiting for the just-spawned local server's port to open.
    singleplayer_starting: Option<Instant>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Poll);
        if self.window.is_some() {
            return; // redundant Resumed
        }
        let attrs = with_linux_app_id(
            Window::default_attributes()
                .with_title("DolphinClient")
                .with_window_icon(load_window_icon())
                .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0)),
        );
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
                for (key, img) in &self.tiled_textures {
                    r.ensure_skin_tiled(*key, img);
                }
                if !self.item_icons.is_empty() {
                    r.ensure_item_atlas(&self.item_icons.image);
                }
                if let Some(atlas) = self.particle_atlas.take() {
                    r.ensure_particle_atlas(&atlas);
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
            WindowEvent::CloseRequested => {
                self.shutdown_singleplayer_blocking();
                event_loop.exit();
            }
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
        self.hud.close_creative();
    }

    /// Opens the right screen for the inventory key (E by default, or the
    /// gamepad's North button): the mount's own screen while riding something
    /// that has one (vanilla's `isServerControlledInventory`), the creative
    /// menu in creative, the ordinary survival inventory otherwise.
    fn open_inventory_screen(&mut self) {
        if self
            .player
            .as_ref()
            .and_then(|p| p.vehicle_kind.as_deref())
            .is_some_and(riding::has_inventory)
        {
            self.send_cmd(Command::OpenMountInventory);
            self.keys.clear();
            self.push_move_if_changed();
            return;
        }
        if self.player.as_ref().is_some_and(|p| p.game_mode == 1) {
            self.hud.open_creative();
        } else {
            self.hud.open_own_inventory();
        }
        self.keys.clear();
        self.push_move_if_changed();
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
        // Vanilla's F3 chords: B draws every entity's hitbox, G the borders of
        // the chunk you are standing in. Holding F3 swallows the letter.
        if pressed && !repeat && key_down(&self.keys, &self.settings.keys.debug) {
            match code {
                KeyCode::KeyB => {
                    self.show_hitboxes = !self.show_hitboxes;
                    return;
                }
                KeyCode::KeyG => {
                    self.show_chunk_borders = !self.show_chunk_borders;
                    return;
                }
                _ => {}
            }
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
                if self.hud.dialog_open() {
                    // A server dialog sits above everything else (even the
                    // in-bed screen) — same priority `dialog_screen` renders
                    // it at.
                    let ctx = self.egui_ctx.clone();
                    let mut dlg_actions = Vec::new();
                    self.hud.dialog_escape(&ctx, &mut dlg_actions);
                    for action in dlg_actions {
                        match action {
                            HudAction::SendChat(msg) => self.send_cmd(Command::Chat(msg)),
                            HudAction::OpenUrl(url) => {
                                if let Err(e) = open::that(&url) {
                                    warn!(url, error = %e, "app: failed to open URL");
                                }
                            }
                            _ => {}
                        }
                    }
                } else if self.sleep_since.is_some() {
                    // Vanilla's in-bed screen: Esc gets you out of bed, it
                    // does not pause the game.
                    self.sleep_since = None;
                    self.send_cmd(Command::StopSleeping);
                } else if self.hud.container_open() {
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
        // Except while a text field in it has the keyboard — the anvil's name
        // and the recipe book's search box both take letters, and "e" must go
        // into them rather than shutting the screen.
        if self.hud.container_open() {
            if pressed
                && !repeat
                && KeyBinds::matches(&self.settings.keys.inventory, code)
                && !self.hud.container_typing()
            {
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
                self.jump_pressed();
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
                    self.open_inventory_screen();
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
            // Letting go is what actually makes a horse jump, and how hard.
            if let Some(power) = self.ride_jump.release(Instant::now()) {
                self.send_cmd(Command::RideJump { power });
            }
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
                    self.play_attack_sound(id);
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
                // A Book & Quill opens its own editor entirely client-side —
                // real vanilla's `LocalPlayer.openItemGui` does this straight
                // off the held stack's `WritableBookContent`, no server round
                // trip to *open* it (only Done/Sign send a real packet).
                // Gated on "looking at air" like the rest of this branch's own
                // block-vs-item-use framing below.
                if hit.is_none() && !self.hud.book_editor_open() {
                    let main =
                        self.hotbar.get(self.selected_slot as usize).and_then(|s| s.as_ref());
                    if let Some(pages) = main.and_then(|i| i.writable_pages.clone()) {
                        self.hud.open_book_editor(pages, false);
                    } else if let Some(pages) =
                        self.offhand.as_ref().and_then(|i| i.writable_pages.clone())
                    {
                        self.hud.open_book_editor(pages, true);
                    }
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
            MouseButton::Middle => {
                // Real vanilla (`Minecraft.pickBlockOrEntity`) always sends
                // the real pick packet regardless of game mode or hotbar
                // state — Ctrl held is `includeData` (pick block/entity with
                // its full NBT, e.g. a written book or a named mob).
                let include_data = self.keys.contains(&KeyCode::ControlLeft)
                    || self.keys.contains(&KeyCode::ControlRight);
                // Vanilla picks the entity first when one is closer than the
                // block behind it — a mob hands over its spawn egg.
                if let Some((id, t)) = self.entity_hit(eye, dir, 5.0)
                    && block_t.is_none_or(|bt| t < bt)
                {
                    self.send_cmd(Command::PickItemFromEntity { id, include_data });
                    if let Some(track) = self.tracks.get(&id) {
                        let egg = format!("{}_spawn_egg", track.snap.kind);
                        self.pick_up(egg);
                    }
                } else if let Some((bpos, _)) = hit {
                    self.send_cmd(Command::PickItemFromBlock { pos: bpos, include_data });
                    self.pick_block(bpos);
                }
            }
            _ => {}
        }
    }

    /// Middle-click: take the block under the crosshair. The real pick
    /// packet (sent by the caller) is what actually gets the server to swap
    /// a copy into the selected slot for any item the player already owns
    /// anywhere in their inventory — this is just the immediate local
    /// feedback: switch to it if it's already in the hotbar, or (creative
    /// only) conjure a fresh stack, without waiting on the round trip.
    fn pick_block(&mut self, pos: BlockPos) {
        let Some(entry) = self.table.entry(self.mirror.get_block(pos)) else {
            return;
        };
        let icons = self.item_icons.clone();
        let Some(item) = creative::pick_item(&entry.short_name, |n| icons.uv(n).is_some()) else {
            return;
        };
        self.pick_up(item);
    }

    /// Reach for an item by name: the hotbar slot that already holds it, or —
    /// in creative, where the game is allowed to conjure things — a fresh
    /// stack in the selected slot.
    fn pick_up(&mut self, item: String) {
        if self.item_icons.uv(&item).is_none() {
            return;
        }
        // Already in the hotbar? Just switch to it — that is what vanilla does
        // first, in every game mode.
        let in_hotbar = self
            .hotbar
            .iter()
            .position(|s| s.as_ref().is_some_and(|i| i.item == item))
            .map(|i| i as u8);
        if let Some(slot) = in_hotbar {
            self.selected_slot = slot;
            self.send_cmd(Command::SelectHotbar(slot));
            return;
        }
        if self
            .player
            .as_ref()
            .is_some_and(|p| p.abilities.instant_build)
        {
            self.send_cmd(Command::CreativeSlot {
                slot: 36 + self.selected_slot as u16,
                count: creative::stack_size(&item),
                item,
            });
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

    /// Discrete (edge-triggered) gamepad buttons — everything that isn't a
    /// continuous axis (those are read directly in `push_move_if_changed`/
    /// `apply_mouse_look`). Mirrors the equivalent keyboard/mouse action
    /// exactly, by calling the very same methods, so gamepad and
    /// keyboard+mouse players see identical behaviour. Button layout
    /// (Bedrock-style, since that's the mapping most pad players already
    /// know): LT/RT analog triggers attack/use like left/right click, the
    /// bumpers cycle the hotbar like the scroll wheel, A jumps, B sneaks,
    /// X swaps the offhand item, Y opens the inventory, clicking the left
    /// stick sprints, clicking the right stick cycles the camera
    /// perspective, and Start is Escape.
    fn apply_gamepad_actions(&mut self) {
        use gilrs::Button;

        // Attack/use mirror a real mouse click+hold. `on_click`/
        // `on_mouse_button` already no-op unless the pointer is grabbed
        // (i.e. actually in-game with no menu open), so these are safe to
        // call unconditionally every frame.
        if self.gamepad.just_pressed(Button::LeftTrigger2) {
            self.on_click(MouseButton::Left);
        }
        if self.gamepad.just_pressed(Button::LeftTrigger2) || self.gamepad.just_released(Button::LeftTrigger2)
        {
            let held = self.gamepad.held(Button::LeftTrigger2);
            self.on_mouse_button(MouseButton::Left, held, false);
        }
        if self.gamepad.just_pressed(Button::RightTrigger2) {
            self.on_click(MouseButton::Right);
        }
        if self.gamepad.just_pressed(Button::RightTrigger2)
            || self.gamepad.just_released(Button::RightTrigger2)
        {
            let held = self.gamepad.held(Button::RightTrigger2);
            self.on_mouse_button(MouseButton::Right, held, false);
        }

        // Hotbar cycle (bumpers) mirrors the scroll wheel exactly.
        if self.gamepad.just_pressed(Button::LeftTrigger) {
            self.on_scroll(1.0);
        }
        if self.gamepad.just_pressed(Button::RightTrigger) {
            self.on_scroll(-1.0);
        }

        // Start = Escape: identical to a real Escape key press, so chat,
        // containers, the in-bed screen and the pause menu all handle it via
        // the exact same branch `on_key` already has for the keyboard.
        if self.gamepad.just_pressed(Button::Start) {
            self.on_key(KeyCode::Escape, ElementState::Pressed, false, false);
        }

        // Everything below only makes sense mid-game with no menu or text
        // field capturing input — the same guard the keyboard path applies
        // before it ever reaches its own jump/sneak/sprint/offhand/inventory
        // branches.
        if self.hud.container_open() || self.hud.wants_keyboard() || self.hud.is_paused() {
            return;
        }

        if self.gamepad.just_pressed(Button::South) {
            self.send_cmd(Command::Jump(true));
            self.jump_pressed();
        }
        if self.gamepad.just_released(Button::South) {
            self.send_cmd(Command::Jump(false));
            if let Some(power) = self.ride_jump.release(Instant::now()) {
                self.send_cmd(Command::RideJump { power });
            }
        }
        if self.connected && self.gamepad.just_pressed(Button::East) && self.settings.sneak_toggle {
            self.sneak_latch = !self.sneak_latch;
        }
        if self.connected
            && self.gamepad.just_pressed(Button::LeftThumb)
            && self.settings.sprint_toggle
        {
            self.sprint_latch = !self.sprint_latch;
        }
        if self.connected {
            if self.gamepad.just_pressed(Button::West) {
                self.send_cmd(Command::SwapOffhand);
            }
            if self.gamepad.just_pressed(Button::North) {
                self.open_inventory_screen();
            }
            if self.gamepad.just_pressed(Button::RightThumb) && !self.hud.wants_keyboard() {
                self.perspective = (self.perspective + 1) % 3;
            }
        }
    }

    /// Menu-only gamepad navigation. Outside of grabbed gameplay — i.e.
    /// whenever a screen (title, connect, pause, options, a container, …) is
    /// actually what's on display — the d-pad and A/B are synthesized as the
    /// same Tab/Shift+Tab/Enter/Escape key events a keyboard would send.
    /// Every existing egui screen already supports keyboard focus traversal
    /// (see the Tab handling in `on_key`), so this gets gamepad navigation
    /// for free instead of needing a bespoke focus system per screen.
    fn inject_gamepad_menu_nav(&mut self, raw_input: &mut egui::RawInput) {
        use gilrs::Button;
        let mut send_key = |key: egui::Key, modifiers: egui::Modifiers| {
            raw_input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            });
        };
        if self.gamepad.just_pressed(Button::DPadDown) || self.gamepad.just_pressed(Button::DPadRight)
        {
            send_key(egui::Key::Tab, egui::Modifiers::NONE);
        }
        if self.gamepad.just_pressed(Button::DPadUp) || self.gamepad.just_pressed(Button::DPadLeft) {
            send_key(egui::Key::Tab, egui::Modifiers::SHIFT);
        }
        if self.gamepad.just_pressed(Button::South) {
            send_key(egui::Key::Enter, egui::Modifiers::NONE);
        }
        if self.gamepad.just_pressed(Button::East) {
            send_key(egui::Key::Escape, egui::Modifiers::NONE);
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
        let Some((_, t)) = self.entity_hit(eye, dir, 3.0) else {
            return false;
        };
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
    #[allow(clippy::too_many_arguments)]
    fn spawn_particles(
        &mut self,
        origin: [f64; 3],
        tex: ParticleTex,
        color: [f32; 3],
        size: f32,
        count: u32,
        spread: [f32; 3],
        speed: f32,
        gravity: f32,
        item: Option<&str>,
    ) {
        // Bounded: each live particle is one draw call, so keep the ceiling
        // modest even during explosion/firework spam. The Particles option
        // scales the count (All / Decreased / Minimal), like vanilla.
        const CAP: usize = 1500;
        let factor = self.settings.particles.factor();
        let count = (count as f32 * factor).round() as usize;
        let count = count.min(CAP - self.particles.len().min(CAP));
        // Resolved once per burst, not per particle — every particle in one
        // burst is the same item.
        let item_uv = item.and_then(|name| self.item_icons.uv(name));
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
                pos: [
                    origin[0] + jx as f64,
                    origin[1] + jy as f64,
                    origin[2] + jz as f64,
                ],
                vel: [vx as f64, vy as f64, vz as f64],
                tex,
                color,
                size,
                age: 0.0,
                life,
                gravity,
                fade_to: None,
                item_uv,
            });
        }
    }

    /// Lingering potions and the dragon's breath: the puddle they leave.
    ///
    /// Vanilla gives an area-effect cloud no model at all — what you see is
    /// particles, sprayed at random points inside its circle, as many as the
    /// circle is wide. The cloud grows and shrinks on the server and the
    /// radius rides along in its metadata, so the puddle spreads and dies back
    /// on its own.
    fn tick_area_clouds(&mut self, dt: f32) {
        if !self.connected || !self.settings.particles.ambient() {
            return;
        }
        // One vanilla tick.
        self.cloud_accum += dt;
        if self.cloud_accum < 0.05 {
            return;
        }
        self.cloud_accum = 0.0;
        let clouds: Vec<([f64; 3], f32)> = self
            .tracks
            .values()
            .filter_map(|t| t.snap.cloud_radius.map(|r| (t.snap.pos, r)))
            .filter(|(_, r)| *r > 0.05)
            .collect();
        for (pos, radius) in clouds {
            // Vanilla scales the count with the area, so a fresh potion is a
            // thick cloud and the last of it is a wisp.
            let count = ((radius * radius * 2.0) as u32).clamp(1, 20);
            for _ in 0..count {
                let a = self.rand01() * std::f32::consts::TAU;
                let d = radius * self.rand01().sqrt();
                let (sa, ca) = a.sin_cos();
                let rise = 0.4 + self.rand01() as f64 * 0.3;
                let life = 0.6 + self.rand01() * 0.4;
                self.particles.push(Particle {
                    pos: [
                        pos[0] + (ca * d) as f64,
                        pos[1] + 0.05,
                        pos[2] + (sa * d) as f64,
                    ],
                    vel: [0.0, rise, 0.0],
                    tex: ParticleTex::Effect,
                    // The potion's own colour lives in a data component the
                    // server does not send with the entity, so this is
                    // vanilla's plain effect swirl rather than a guess at the
                    // brew.
                    color: [0.85, 0.85, 0.95],
                    size: 0.14,
                    age: 0.0,
                    life,
                    gravity: -0.15,
                    fade_to: None,
                    item_uv: None,
                });
            }
        }
    }

    /// A rocket bursting: paint every star it was made with.
    ///
    /// Vanilla builds each explosion out of particles whose directions come
    /// from the star's shape — a hollow ball, or the outline of a star or a
    /// creeper face — then colours them with the dyes the star was crafted
    /// from and lets them fade to the second set. The bang is deliberately
    /// late: sound takes about a third of a second to cross a hundred blocks,
    /// and fireworks look wrong without that gap.
    fn explode_firework(&mut self, stars: &[crate::bridge::events::FireworkStar], at: [f64; 3]) {
        use crate::app::fireworks::{Shape, Star};
        let factor = self.settings.particles.factor();
        if factor <= 0.0 {
            return;
        }
        // The flash at the middle, before the sparks.
        self.spawn_particles(
            at,
            ParticleTex::Flash,
            [1.0, 1.0, 1.0],
            0.8,
            1,
            [0.0; 3],
            0.0,
            0.0,
            None,
        );

        for raw in stars {
            let star = Star {
                shape: match raw.shape {
                    1 => Shape::LargeBall,
                    2 => Shape::Star,
                    3 => Shape::Creeper,
                    4 => Shape::Burst,
                    _ => Shape::SmallBall,
                },
                colors: raw.colors.iter().map(|&c| fireworks::rgb(c)).collect(),
                fade: raw.fade_colors.iter().map(|&c| fireworks::rgb(c)).collect(),
                trail: raw.trail,
                twinkle: raw.twinkle,
            };
            let dirs = {
                let mut rng = || self.rand01();
                fireworks::directions(&star, &mut rng)
            };
            // One spark per direction, capped by the Particles option like
            // everything else that can spam the screen.
            let keep = (dirs.len() as f32 * factor).round() as usize;
            for (i, d) in dirs.iter().take(keep.max(1)).enumerate() {
                let color = fireworks::spark_color(&star, i, 0.0);
                let fade_to =
                    (!star.fade.is_empty()).then(|| fireworks::spark_color(&star, i, 1.0));
                // A trail star hangs in the air longer and falls further.
                let life = (if star.trail { 1.4 } else { 0.9 }) + self.rand01() * 0.3;
                self.particles.push(Particle {
                    pos: at,
                    vel: [d[0] as f64 * 12.0, d[1] as f64 * 12.0, d[2] as f64 * 12.0],
                    tex: ParticleTex::Glow,
                    color,
                    size: 0.16,
                    age: 0.0,
                    life,
                    gravity: if star.trail { 2.2 } else { 1.2 },
                    fade_to,
                    item_uv: None,
                });
            }
            let dist = self
                .player
                .as_ref()
                .map(|p| {
                    let d = [at[0] - p.pos[0], at[1] - p.pos[1], at[2] - p.pos[2]];
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                })
                .unwrap_or(0.0);
            let (blast, twinkle, delay) = fireworks::boom(&star, dist);
            self.delayed_sounds.push((
                Instant::now() + Duration::from_secs_f32(delay),
                blast.to_string(),
                at,
            ));
            if let Some(tw) = twinkle {
                self.delayed_sounds.push((
                    Instant::now() + Duration::from_secs_f32(delay + 0.25),
                    tw.to_string(),
                    at,
                ));
            }
        }
    }

    /// How far the nether-portal swirl has faded in, 0..1. Vanilla ramps it
    /// while you stand in the portal and drops it the moment you step out.
    fn portal_amount(&self) -> f32 {
        if !self.connected {
            return 0.0;
        }
        let Some(p) = self.player.as_ref() else {
            return 0.0;
        };
        let eye = BlockPos {
            x: p.pos[0].floor() as i32,
            y: (p.pos[1] + p.eye_height as f64).floor() as i32,
            z: p.pos[2].floor() as i32,
        };
        let name = self
            .table
            .entry(self.mirror.get_block(eye))
            .map(|e| e.short_name.as_str())
            .unwrap_or("");
        if name == "nether_portal" || name == "end_gateway" {
            1.0
        } else {
            0.0
        }
    }

    /// The world border wall, if the camera is anywhere near it. Vanilla only
    /// draws the wall when you are close enough for it to matter, and colours
    /// it by whether the border is standing still, growing or closing in.
    fn border_params(&self) -> Option<crate::render::BorderParams> {
        if !self.connected {
            return None;
        }
        let b = self.border;
        // A border in mid-move interpolates between the two sizes.
        let size = border_interpolated_size(
            b.old_size,
            b.new_size,
            b.lerp_time,
            self.border_since.elapsed().as_millis() as f64,
        );
        let radius = size / 2.0;
        // The default border is 30 million blocks wide; never draw that.
        if radius > 2.9e7 {
            return None;
        }
        let p = self.player.as_ref()?;
        let reach = (self.settings.render_distance as f64 * 16.0) + 32.0;
        let dx = (p.pos[0] - b.center_x).abs();
        let dz = (p.pos[2] - b.center_z).abs();
        let dist_to_border = (radius - dx).min(radius - dz);
        if dist_to_border > reach {
            return None;
        }
        let color = border_status_color(b.new_size > b.old_size, b.new_size < b.old_size);
        // `renderDistance` here is the plain block-radius (`render_distance * 16`),
        // not this client's own `reach` above (which adds a 32-block draw-cutoff
        // margin on top — a local invention, not something vanilla has).
        let render_distance_blocks = self.settings.render_distance as f64 * 16.0;
        let alpha = border_wall_alpha(dist_to_border, render_distance_blocks);
        Some(crate::render::BorderParams {
            center_x: b.center_x,
            center_z: b.center_z,
            radius,
            color,
            // Vanilla scrolls the wall on a three-second loop.
            phase: (self.start.elapsed().as_secs_f32() / 3.0).fract(),
            tex: self.border_tex,
            alpha,
        })
    }

    /// How hard the world-border warning should flash right now, 0 (fine) ..
    /// 1 (about to hit the wall) — vanilla's `getDistanceToBorder`-driven red
    /// screen pulse. Includes the time-based term for a fast-moving border
    /// (`Gui.extractVignette`'s `movingBlocksThreshold`), not just the plain
    /// `warning_blocks` radius — a border about to sweep over the player
    /// warns further out, in proportion to how fast it's currently moving.
    fn border_warning(&self) -> f32 {
        if !self.connected || self.border.warning_blocks == 0 {
            return 0.0;
        }
        let b = self.border;
        let size = border_interpolated_size(
            b.old_size,
            b.new_size,
            b.lerp_time,
            self.border_since.elapsed().as_millis() as f64,
        );
        let radius = size / 2.0;
        if radius > 2.9e7 {
            return 0.0;
        }
        let Some(p) = &self.player else { return 0.0 };
        let dx = (p.pos[0] - b.center_x).abs();
        let dz = (p.pos[2] - b.center_z).abs();
        let distance = radius - dx.max(dz);
        let lerp_speed = border_lerp_speed(b.old_size, b.new_size, b.lerp_time);
        let warn = border_warning_distance(b.warning_blocks, lerp_speed, b.warning_time, b.new_size, size);
        (1.0 - (distance / warn).clamp(0.0, 1.0)) as f32
    }

    /// An explosion went off. The server sends no particles and no sound for
    /// one — the client is expected to make both, so this is what actually
    /// puts the fireball on screen when TNT or a creeper goes off.
    fn on_explosion(&mut self, pos: [f64; 3], radius: f32, sound: &str) {
        // Vanilla: a big blast gets `explosion_emitter` (a handful of large
        // puffs spread over the radius), a small one a single `explosion`.
        let big = radius >= 2.0;
        let puffs = if big { (radius * 3.0) as u32 } else { 1 };
        self.spawn_particles(
            pos,
            ParticleTex::Explosion,
            [1.0, 1.0, 1.0],
            if big { 1.8 } else { 1.0 },
            puffs.clamp(1, 32),
            [radius * 0.5, radius * 0.5, radius * 0.5],
            0.0,
            0.0,
            None,
        );
        // …plus the smoke that hangs around after it.
        self.spawn_particles(
            pos,
            ParticleTex::Smoke,
            [0.6, 0.6, 0.6],
            0.6,
            (radius * 4.0).clamp(4.0, 48.0) as u32,
            [radius * 0.6, radius * 0.4, radius * 0.6],
            0.06,
            0.0,
            None,
        );
        let name = if sound.is_empty() {
            "entity.generic.explode"
        } else {
            sound
        };
        self.play_world_sound(name, pos, 4.0, 1.0);
    }

    /// Play a positional sound the client itself decides to make (explosions,
    /// item pickups) — the server never sends these.
    fn play_world_sound(&self, name: &str, pos: [f64; 3], volume: f32, pitch: f32) {
        self.play_world_sound_in(
            name,
            pos,
            volume,
            pitch,
            crate::settings::SoundCategory::Blocks,
        );
    }

    /// The same, under a specific volume slider (a note block counts as a
    /// record in vanilla, not as a block).
    fn play_world_sound_in(
        &self,
        name: &str,
        pos: [f64; 3],
        volume: f32,
        pitch: f32,
        category: crate::settings::SoundCategory,
    ) {
        let Some(audio) = &self.audio else { return };
        if !audio.has(name) {
            return;
        }
        let ear = self.listener_pos();
        let (dx, dy, dz) = (pos[0] - ear[0], pos[1] - ear[1], pos[2] - ear[2]);
        let distance = ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
        let gain = self.settings.category_volume(category);
        audio.play_positional(name, gain, volume, pitch, distance, audio.local_seed());
    }

    /// A piston fired. The server sends this one event and then says nothing
    /// until the blocks have landed two ticks later, so the client works out
    /// what travels — vanilla's structure resolver, run against our own copy of
    /// the world — and animates it. Nothing here touches the world: if our
    /// answer ever differed from the server's, the worst case is a ghost that
    /// fades in a tenth of a second.
    fn start_piston(&mut self, pos: BlockPos, action: u8, param: u8) {
        use crate::world::piston::Resolver;
        let facing = match param & 7 {
            0 => Face::Down,
            1 => Face::Up,
            2 => Face::North,
            3 => Face::South,
            4 => Face::West,
            _ => Face::East,
        };
        // 0 = push out, 1 = pull back, 2 = pull back without taking anything.
        let extending = action == 0;
        let sticky = self
            .table
            .entry(self.mirror.get_block(pos))
            .is_some_and(|e| e.short_name == "sticky_piston");
        let table = self.table.clone();
        let moved = {
            let mirror = &self.mirror;
            Resolver::new(&table, pos, facing, extending, |p| mirror.get_block(p)).resolve()
        };
        // A blocked piston does not move at all — and neither does the head.
        let Some(moved) = moved else { return };
        // A "drop" retraction leaves whatever was in front of it behind.
        let riders: Vec<pistons::Rider> = if action == 2 {
            Vec::new()
        } else {
            moved
                .push
                .iter()
                .map(|&src| pistons::Rider {
                    src,
                    state: self.mirror.get_block(src),
                })
                .collect()
        };
        let head_state = table
            .find_state(
                "piston_head",
                &[
                    ("facing", face_name(facing)),
                    ("short", "false"),
                    ("type", if sticky { "sticky" } else { "normal" }),
                ],
            )
            .unwrap_or(0);
        // Coming back with a block in tow, the server leaves the old head
        // standing until the stroke ends; drawing a second one over it looks
        // worse than drawing none.
        let head = extending || riders.is_empty();
        tracing::debug!(
            x = pos.x,
            y = pos.y,
            z = pos.z,
            ?facing,
            extending,
            riders = riders.len(),
            "piston fired"
        );
        self.pistons.start(pistons::Stroke::new(
            pos,
            facing,
            extending,
            riders,
            head,
            head_state,
            Instant::now(),
        ));
    }

    /// A note block was struck. The event carries nothing: the instrument and
    /// the note are properties of the block, and the pitch follows from the
    /// note, exactly as vanilla works it out.
    fn play_note_block(&mut self, pos: BlockPos) {
        let Some(entry) = self.table.entry(self.mirror.get_block(pos)) else {
            return;
        };
        if entry.short_name != "note_block" {
            return;
        }
        let instrument = entry.prop("instrument").unwrap_or("harp").to_string();
        let note: i32 = entry.prop("note").and_then(|n| n.parse().ok()).unwrap_or(0);
        // The mob-head instruments play one flat sample and make no particle.
        let tunable = !matches!(
            instrument.as_str(),
            "zombie"
                | "skeleton"
                | "creeper"
                | "dragon"
                | "wither_skeleton"
                | "piglin"
                | "custom_head"
        );
        let pitch = if tunable {
            2f32.powf((note - 12) as f32 / 12.0)
        } else {
            1.0
        };
        tracing::debug!(instrument = %instrument, note, pitch, "note block struck");
        let center = [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5];
        self.play_world_sound_in(
            &format!("block.note_block.{instrument}"),
            center,
            3.0,
            pitch,
            crate::settings::SoundCategory::Records,
        );
        if !tunable || !self.settings.particles.ambient() {
            return;
        }
        // Vanilla's note particle: one sprite drifting up out of the block,
        // coloured by where the note sits in the octave.
        let f = note as f32 / 24.0;
        let comp = |o: f32| (((f + o) * std::f32::consts::TAU).sin() * 0.65 + 0.35).max(0.0);
        self.particles.push(Particle {
            pos: [center[0], pos.y as f64 + 1.2, center[2]],
            vel: [0.0, 4.0, 0.0],
            tex: ParticleTex::Note,
            color: [comp(0.0), comp(1.0 / 3.0), comp(2.0 / 3.0)],
            size: 0.22,
            age: 0.0,
            life: 0.3,
            gravity: 0.0,
            fade_to: None,
            item_uv: None,
        });
    }

    /// Vanilla's `animateTick`: every tick, pick a few hundred random blocks
    /// around the player and let each one make its own ambience. This is where
    /// torch smoke, campfire columns, lava pops, portal drift, falling petals
    /// and drips come from — the server sends none of it.
    fn tick_ambient(&mut self, dt: f32) {
        if !self.connected || !self.settings.particles.ambient() {
            return;
        }
        let Some(player) = self.player.as_ref() else {
            return;
        };
        let eye = [
            player.pos[0],
            player.pos[1] + player.eye_height as f64,
            player.pos[2],
        ];
        // Run on the game tick, not the frame, so the rate doesn't ride on FPS.
        self.ambient_accum += dt;
        let ticks = (self.ambient_accum * 20.0) as u32;
        if ticks == 0 {
            return;
        }
        self.ambient_accum -= ticks as f32 / 20.0;
        // More than a couple of ticks of backlog is a stall, not a debt.
        let ticks = ticks.min(2);

        // Vanilla samples 667 blocks in a ±16 box per tick; 400 looks the same
        // and leaves the budget for everything else.
        const SAMPLES: u32 = 400;
        const RANGE: i32 = 16;
        let scale = self.settings.particles.factor();
        let samples = (SAMPLES as f32 * scale) as u32;
        let mut emissions = Vec::new();
        for _ in 0..ticks {
            for _ in 0..samples {
                let pick =
                    |r: &mut ambient::Rng| ((r.next_f32() * (RANGE * 2 + 1) as f32) as i32) - RANGE;
                let pos = BlockPos {
                    x: eye[0].floor() as i32 + pick(&mut self.ambient_rng),
                    y: eye[1].floor() as i32 + pick(&mut self.ambient_rng),
                    z: eye[2].floor() as i32 + pick(&mut self.ambient_rng),
                };
                let Some(entry) = self.table.entry(self.mirror.get_block(pos)) else {
                    continue;
                };
                // Air is the overwhelming majority of every sample; skip it
                // before touching the neighbours.
                if entry.short_name == "air" {
                    continue;
                }
                let above = self
                    .table
                    .entry(self.mirror.get_block(BlockPos {
                        y: pos.y + 1,
                        ..pos
                    }))
                    .map(|e| e.short_name.as_str())
                    .unwrap_or("air");
                let below = self
                    .table
                    .entry(self.mirror.get_block(BlockPos {
                        y: pos.y - 1,
                        ..pos
                    }))
                    .map(|e| e.short_name.as_str())
                    .unwrap_or("air");
                ambient::emissions(
                    &entry.short_name,
                    &entry.props,
                    pos,
                    &ambient::Neighbours { above, below },
                    &mut self.ambient_rng,
                    &mut emissions,
                );
            }
            // Rain stipples the ground it can actually reach.
            if self.rain_level > 0.05 {
                let splashes = (12.0 * self.rain_level * scale) as u32;
                for _ in 0..splashes {
                    let pos = BlockPos {
                        x: eye[0].floor() as i32
                            + ((self.ambient_rng.next_f32() * 17.0) as i32 - 8),
                        y: eye[1].floor() as i32,
                        z: eye[2].floor() as i32
                            + ((self.ambient_rng.next_f32() * 17.0) as i32 - 8),
                    };
                    // Walk down to the first solid block and only splash there
                    // if the sky can see it.
                    let Some(ground) = self.ground_under(pos) else {
                        continue;
                    };
                    let (sky, _) = self.mirror.light_at(BlockPos {
                        y: ground.y + 1,
                        ..ground
                    });
                    if sky < 15 {
                        continue;
                    }
                    emissions.push(ambient::rain_splash(ground, &mut self.ambient_rng));
                }
            }
        }
        for e in emissions {
            self.spawn_particles(
                e.pos, e.tex, e.color, e.size, e.count, e.spread, e.speed, e.gravity, None,
            );
        }
    }

    /// Music and cave ambience — both of which the client owns outright: the
    /// server never sends a note.
    fn tick_music(&mut self, dt: f32) {
        let Some(audio) = &self.audio else { return };
        // Which score fits where we are.
        let scene = if !self.connected {
            music::MusicScene::Menu
        } else if self.player.as_ref().is_some_and(|p| p.eyes_in_water) {
            music::MusicScene::Underwater
        } else if self.dim_name.contains("nether") {
            music::MusicScene::Nether
        } else if self.dim_name.contains("end") {
            music::MusicScene::End
        } else {
            music::MusicScene::Overworld
        };
        if let Some(scene) = self.music.tick(dt, scene) {
            let gain = self
                .settings
                .category_volume(crate::settings::SoundCategory::Music);
            if gain > 0.0
                && let Some(name) = scene.candidates().iter().find(|n| audio.has(n))
            {
                info!(track = name, "app: starting music");
                audio.play_ui(name, gain);
            }
        }

        // Cave mood: vanilla samples one random block near you per tick and
        // lets the dark ones build toward a noise.
        if !self.connected {
            self.mood.reset();
            return;
        }
        let Some(player) = self.player.as_ref() else {
            return;
        };
        let eye = [
            player.pos[0],
            player.pos[1] + player.eye_height as f64,
            player.pos[2],
        ];
        self.ambient_accum_mood += dt;
        let ticks = ((self.ambient_accum_mood * 20.0) as u32).min(4);
        if ticks == 0 {
            return;
        }
        self.ambient_accum_mood -= ticks as f32 / 20.0;
        for _ in 0..ticks {
            let jitter = |r: &mut ambient::Rng| ((r.next_f32() * 17.0) as i32) - 8;
            let pos = BlockPos {
                x: eye[0].floor() as i32 + jitter(&mut self.ambient_rng),
                y: eye[1].floor() as i32 + jitter(&mut self.ambient_rng),
                z: eye[2].floor() as i32 + jitter(&mut self.ambient_rng),
            };
            let (sky, block) = self.mirror.light_at(pos);
            if self.mood.tick(block == 0, sky > 0) {
                // Vanilla puts the noise a little away from you, never on top.
                let offset = |r: &mut ambient::Rng| (r.next_f32() as f64 - 0.5) * 16.0;
                let at = [
                    eye[0] + offset(&mut self.ambient_rng),
                    eye[1] + offset(&mut self.ambient_rng) * 0.5,
                    eye[2] + offset(&mut self.ambient_rng),
                ];
                let gain = self
                    .settings
                    .category_volume(crate::settings::SoundCategory::Ambient);
                if gain > 0.0 && audio.has("ambient.cave") {
                    let distance = ((at[0] - eye[0]).powi(2)
                        + (at[1] - eye[1]).powi(2)
                        + (at[2] - eye[2]).powi(2))
                    .sqrt() as f32;
                    audio.play_positional(
                        "ambient.cave",
                        gain,
                        1.0,
                        1.0,
                        distance,
                        audio.local_seed(),
                    );
                }
            }
        }
    }

    /// The topmost solid block at or below `pos`, within a few blocks — what
    /// rain lands on.
    fn ground_under(&self, pos: BlockPos) -> Option<BlockPos> {
        for dy in 0..12 {
            let p = BlockPos {
                y: pos.y - dy,
                ..pos
            };
            let entry = self.table.entry(self.mirror.get_block(p))?;
            if entry.short_name != "air" {
                return Some(p);
            }
        }
        None
    }

    /// Advance and cull particles (Euler step with a little drag).
    fn tick_particles(&mut self, dt: f32) {
        self.tick_area_clouds(dt);
        // Sounds that were waiting for their travel time — the bang of a
        // firework that went off a hundred blocks away.
        if !self.delayed_sounds.is_empty() {
            let now = Instant::now();
            let due: Vec<_> = self
                .delayed_sounds
                .iter()
                .filter(|(at, _, _)| *at <= now)
                .cloned()
                .collect();
            self.delayed_sounds.retain(|(at, _, _)| *at > now);
            for (_, name, pos) in due {
                self.play_world_sound(&name, pos, 2.0, 1.0);
            }
        }
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

    /// The locator bar's dots for this frame: one per tracked waypoint that's
    /// currently within the visible ±60° yaw window, positioned, sprited and
    /// coloured exactly like vanilla's `LocatorBarRenderer`. Empty when there
    /// are no waypoints, no local player yet, or none are currently in view.
    fn locator_dots(&self) -> Vec<hud::LocatorDot> {
        let Some(player) = self.player.as_ref().filter(|_| !self.waypoints.is_empty()) else {
            return Vec::new();
        };
        let cam = player.pos;
        let cam_yaw = self.yaw;
        let cam_pitch = self.pitch;
        let fov = self.settings.fov;
        let mut out = Vec::new();
        for (key, info) in &self.waypoints {
            let (angle, distance, elevation) = match info.pos {
                events::WaypointPos::Empty => continue,
                events::WaypointPos::Pos(p) => {
                    let target = [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5];
                    let (dx, dy, dz) = (target[0] - cam[0], target[1] - cam[1], target[2] - cam[2]);
                    let horiz = (dx * dx + dz * dz).sqrt();
                    let elevation = dy.atan2(horiz).to_degrees() as f32;
                    let dist = (dx * dx + dy * dy + dz * dz).sqrt() as f32;
                    (waypoint_yaw_angle(cam, cam_yaw, target), dist, elevation)
                }
                events::WaypointPos::Chunk { x, z } => {
                    // Chunk waypoints carry no Y — project onto the camera's
                    // own height, exactly like vanilla.
                    let target = [x as f64 * 16.0 + 8.0, cam[1], z as f64 * 16.0 + 8.0];
                    let (dx, dz) = (target[0] - cam[0], target[2] - cam[2]);
                    let dist = (dx * dx + dz * dz).sqrt() as f32;
                    (waypoint_yaw_angle(cam, cam_yaw, target), dist, 0.0)
                }
                events::WaypointPos::Azimuth(rad) => {
                    (wrap_degrees(rad.to_degrees() - cam_yaw), f32::INFINITY, 0.0)
                }
            };
            // Real vanilla's own visibility window: `(-60, 60]`.
            if angle <= -60.0 || angle > 60.0 {
                continue;
            }
            let style = waypoint_style(&info.style);
            out.push(hud::LocatorDot {
                offset_px: locator_dot_offset(angle),
                sprite: waypoint_sprite(style, distance),
                color: info.color.unwrap_or_else(|| hashed_waypoint_color(key)),
                arrow_down: pitch_direction(cam_pitch, fov, elevation),
            });
        }
        out
    }

    /// Recycle the falling-rain field around the player: grow/shrink to a count
    /// scaled by rain strength (and the particles setting), fall each drop, and
    /// respawn drops that fell past the player or drifted too far.
    /// Torchlight never sits still in vanilla: every tick the flicker eases a
    /// tenth of the way toward a fresh random value, which the light texture
    /// turns into a barely-there wobble in block light.
    fn tick_light_flicker(&mut self, dt: f32) {
        self.flicker_accum += dt;
        while self.flicker_accum >= 0.05 {
            self.flicker_accum -= 0.05;
            let r = self.rand01();
            self.light_flicker += (r - self.light_flicker) * 0.1;
        }
    }

    /// What falls out of the sky in the column at `pos`: nothing in a desert,
    /// snow where it is cold enough, rain everywhere else. Vanilla asks the
    /// biome the same two questions (`hasPrecipitation`, `coldEnoughToSnow`).
    fn precipitation_at(&self, pos: [f64; 3]) -> Option<bool> {
        if !self.dim_skylight {
            return None; // no weather in the Nether or the End
        }
        let bp = BlockPos {
            x: pos[0].floor() as i32,
            y: pos[1].floor() as i32,
            z: pos[2].floor() as i32,
        };
        let id = self.mirror.biome_at(bp)?;
        // No biome registry yet (a server that has not sent one): plain rain,
        // rather than a world where the weather quietly never falls.
        let Some(biome) = self.biomes.get(id as usize) else {
            return Some(false);
        };
        precipitation_kind(biome.downfall, biome.temperature)
    }

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
        // Spawning needs the biome and the sky, both of which want `&self`, so
        // the new drops are chosen before the list is borrowed.
        let wanted = target.saturating_sub(self.rain_drops.len());
        let mut fresh: Vec<RainDrop> = Vec::with_capacity(wanted);
        for _ in 0..wanted {
            let mut d = spawn_raindrop(&mut rng, center, R);
            match self.precipitation_at(d.pos) {
                Some(snow) => d.snow = snow,
                // Nothing falls here (a desert, or under a roof): drop it and
                // let the next tick try somewhere else.
                None => continue,
            }
            if snow_or_rain_is_indoors(&self.mirror, d.pos) {
                continue;
            }
            fresh.push(d);
        }
        let drops = &mut self.rain_drops;
        drops.extend(fresh);
        drops.truncate(target);
        let dt64 = dt as f64;
        for d in drops.iter_mut() {
            // Snow drifts down at a fraction of rain's speed and sways as it
            // falls; rain simply falls.
            if d.snow {
                d.pos[1] -= d.speed as f64 * 0.18 * dt64;
                d.phase += dt * 1.4;
                d.pos[0] += (d.phase.sin() * 0.35 * dt) as f64;
                d.pos[2] += (d.phase.cos() * 0.35 * dt) as f64;
            } else {
                d.pos[1] -= d.speed as f64 * dt64;
                d.phase += dt * d.speed * 0.6;
            }
            let (dx, dz) = (d.pos[0] - center[0], d.pos[2] - center[2]);
            if d.pos[1] < center[1] - 4.0 || dx * dx + dz * dz > (R + 5.0) * (R + 5.0) {
                let snow = d.snow;
                *d = spawn_raindrop(&mut rng, center, R);
                d.snow = snow;
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
        self.own_player_draw()
    }

    /// Our own model, however it happens to be looked at — over our shoulder in
    /// third person, or standing in the inventory's preview panel.
    fn own_player_draw(&self) -> Option<EntityDraw> {
        let pos = self
            .cam
            .as_ref()
            .map(|c| c.render_pos)
            .or(self.player.as_ref().map(|p| p.pos))?;
        // Own skin: look ourselves up in the tab list by name, else Steve (0).
        let (mut skin, mut slim) = (0u64, false);
        if let Some((url, sl)) = self.own_skin_url() {
            let key = fnv64(key_of_url(&url).as_bytes());
            if self.renderer.as_ref().is_some_and(|r| r.has_skin(key)) {
                skin = key;
                slim = sl;
            }
        }
        // Gentle walk swing while moving (reuse the view-bob phase). A rider's
        // legs are over the saddle, not walking, so they never swing.
        let riding = self.player.as_ref().is_some_and(|p| p.riding);
        let moving = !riding && (self.last_move.0 != 0 || self.last_move.1 != 0);
        let swing = if moving {
            self.bob_phase.sin() * 0.6
        } else {
            0.0
        };
        // One-shot attack/use arm swing over ~300 ms.
        let attack_swing = match self.hand_swing_start {
            Some(start) => {
                let t = start.elapsed().as_secs_f32() / 0.30;
                if t >= 1.0 {
                    0.0
                } else {
                    (t * std::f32::consts::PI).sin() * 1.4
                }
            }
            None => 0.0,
        };
        let main_hand = self
            .hotbar
            .get(self.selected_slot as usize)
            .and_then(|s| s.as_ref())
            .and_then(|i| self.item_icons.uv(&i.item));
        let off_hand = self
            .offhand
            .as_ref()
            .and_then(|i| self.item_icons.uv(&i.item));
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
        let (cape_flap, cape_lean, cape_lean2) = match &self.cam {
            Some(c) => cape_flap_lean(&c.cape, Instant::now(), self.yaw, self.bob_phase),
            None => (0.0, 0.0, 0.0),
        };
        Some(EntityDraw {
            pos,
            yaw: self.yaw,
            light: [1.0, 1.0],
            tint: [1.0, 1.0, 1.0],
            roll: 0.0,
            kind: EntityDrawKind::Player {
                skin,
                slim,
                swing,
                attack_swing,
                pose: if riding {
                    // Everyone else's riders sit; so do we, seen from behind.
                    PlayerPose::Sitting
                } else if self.sneaking {
                    PlayerPose::Sneaking
                } else {
                    player_pose(
                        self.player.as_ref().map(|p| p.pose).unwrap_or_default(),
                        self.start.elapsed().as_secs_f32(),
                    )
                },
                skin_layers: self.settings.skin_layer_mask(),
                head_pitch: self.pitch,
                head_yaw: 0.0,
                armor,
                trims: [None; 4],
                main_hand,
                off_hand,
                cape: self.own_cape(),
                elytra: match self
                    .player
                    .as_ref()
                    .and_then(|p| p.equipment.chest.as_deref())
                {
                    Some("elytra") => self.elytra_tex,
                    _ => 0,
                },
                cape_flap,
                cape_lean,
                cape_lean2,
            },
        })
    }

    /// Ask the renderer for the little textures the open screen's entity panels
    /// draw into, sized to the panel's real pixels so the model comes out crisp.
    fn preview_textures(&mut self) -> [Option<egui::TextureId>; 2] {
        let mut out = [None, None];
        let Some(kind) = self.hud.container_kind().map(str::to_owned) else {
            return out;
        };
        let Some((slot, panel)) = container::PreviewPanel::of_kind(&kind) else {
            return out;
        };
        let s = self.mcui.gui_scale(&self.egui_ctx, &self.settings);
        let ppp = self.egui_ctx.pixels_per_point();
        let w = (panel.width() * s * ppp).round() as u32;
        let h = (panel.height() * s * ppp).round() as u32;
        if let Some(r) = &mut self.renderer {
            out[slot] = Some(r.gui_entity_texture(slot as u32, w, h));
        }
        out
    }

    /// The entities the open screen wants drawn inside its panels, posed the way
    /// vanilla poses them: turned toward the cursor, the head leading the body
    /// by twice as much, and the whole model tipped by how high the mouse is.
    fn gui_entities(&self) -> Vec<crate::render::GuiEntity> {
        let mut out = Vec::new();
        for (slot, mouse) in self.hud.preview_mouse.iter().enumerate() {
            let Some(m) = *mouse else { continue };
            let panel = match slot {
                0 => container::PreviewPanel::PLAYER,
                _ => container::PreviewPanel::MOUNT,
            };
            // Vanilla takes the arctangent of the offset over 40 px and then
            // uses that radian value as if it were degrees — a shallow curve
            // that never lets the model spin right round.
            let h = (m[0] / 40.0).atan();
            let v = (m[1] / 40.0).atan();
            let (yaw, head_pitch, tilt) = (h * 20.0, -v * 20.0, (v * 20.0).to_radians());
            let mut draws = Vec::new();
            let mut height = 1.8;
            if slot == 0 {
                let Some(mut d) = self.own_player_draw() else {
                    continue;
                };
                d.yaw = yaw;
                d.roll = 0.0;
                if let EntityDrawKind::Player {
                    head_yaw: hy,
                    head_pitch: hp,
                    swing,
                    ..
                } = &mut d.kind
                {
                    // The head leads the body by the same again (vanilla turns
                    // the head twice as far as the shoulders).
                    *hy = yaw;
                    *hp = head_pitch;
                    *swing = 0.0;
                }
                height = if self.sneaking { 1.5 } else { 1.8 };
                draws.push(d);
            } else if let Some(snap) = self.mount_snapshot() {
                height = snap.height.max(0.5);
                for mut d in self.gui_mob_draws(snap) {
                    d.yaw = yaw;
                    if let EntityDrawKind::Mob {
                        head_yaw: hy,
                        head_pitch: hp,
                        ..
                    } = &mut d.kind
                    {
                        *hy = yaw;
                        *hp = head_pitch;
                    }
                    draws.push(d);
                }
            }
            for entity in draws {
                out.push(crate::render::GuiEntity {
                    slot: slot as u32,
                    entity,
                    half_w: panel.width() / (2.0 * panel.scale),
                    half_h: panel.height() / (2.0 * panel.scale),
                    center_y: height / 2.0 + panel.y_offset,
                    tilt,
                });
            }
        }
        out
    }

    /// The animal whose inventory screen is open (we are always riding it).
    fn mount_snapshot(&self) -> Option<&crate::bridge::events::EntitySnapshot> {
        let id = self.player.as_ref()?.vehicle_id?;
        Some(&self.tracks.get(&id)?.snap)
    }

    /// A mob's model and everything worn over it (variant coat, saddle, barding
    /// or carpet, collar, fleece), for a GUI panel. The world loop builds the
    /// same layers around a great deal of movement state a still panel has no
    /// use for.
    fn gui_mob_draws(&self, snap: &crate::bridge::events::EntitySnapshot) -> Vec<EntityDraw> {
        let mut out = Vec::new();
        let Some(&(base_tex, model)) = self.mob_model.get(&snap.kind) else {
            return out;
        };
        let tex = snap
            .variant_name
            .as_ref()
            .and_then(|n| {
                self.mob_named_variant_tex
                    .get(&(snap.kind.clone(), n.clone()))
                    .copied()
            })
            .or_else(|| {
                self.mob_variant_tex
                    .get(&(snap.kind.clone(), snap.variant))
                    .copied()
            })
            .unwrap_or(base_tex);
        let scale = if snap.baby { 0.55 } else { 1.0 };
        let layer = |tex: u64, scale: f32, tint: [f32; 3]| EntityDraw {
            pos: [0.0, 0.0, 0.0],
            yaw: 0.0,
            light: [1.0, 1.0],
            tint,
            roll: 0.0,
            kind: EntityDrawKind::Mob {
                tex,
                model,
                swing: 0.0,
                head_pitch: 0.0,
                head_yaw: 0.0,
                scale,
                anim: 0.0,
                pose: MobPose::None,
            },
        };
        let has = |key: u64| self.renderer.as_ref().is_some_and(|r| r.has_skin(key));
        out.push(layer(tex, scale, [1.0, 1.0, 1.0]));
        if snap.kind == "sheep" && !snap.sheared && has(self.sheep_wool_tex) {
            out.push(layer(self.sheep_wool_tex, scale * 1.12, [1.0, 1.0, 1.0]));
        }
        if let Some(col) = snap.collar {
            let collar = match snap.kind.as_str() {
                "cat" => self.cat_collar_tex,
                "wolf" => self.wolf_collar_tex,
                _ => 0,
            };
            if has(collar) {
                out.push(layer(collar, scale * 1.02, dye_rgb(col)));
            }
        }
        for path in [
            animal_saddle_texture(&snap.kind, snap.equipment.saddle.as_deref()),
            animal_body_texture(&snap.kind, snap.equipment.body.as_deref()),
        ]
        .into_iter()
        .flatten()
        {
            let key = fnv64(path.as_bytes());
            if has(key) {
                out.push(layer(key, scale * 1.02, [1.0, 1.0, 1.0]));
            }
        }
        if snap.kind == "zombie_nautilus" && snap.variant == 1 && snap.equipment.body.is_none()
            && let Some(&coral_tex) = self.mob_variant_tex.get(&("zombie_nautilus".to_string(), 1))
            && has(coral_tex)
        {
            out.push(EntityDraw {
                pos: [0.0, 0.0, 0.0],
                yaw: 0.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Mob {
                    tex: coral_tex,
                    model: MobModel::NautilusCorals,
                    swing: 0.0,
                    head_pitch: 0.0,
                    head_yaw: 0.0,
                    scale,
                    anim: 0.0,
                    pose: MobPose::None,
                },
            });
        }
        out
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
        if let Some((url, sl)) = self.own_skin_url() {
            let key = fnv64(key_of_url(&url).as_bytes());
            if self.renderer.as_ref().is_some_and(|r| r.has_skin(key)) {
                skin = key;
                slim = sl;
            }
        }
        // The selected hotbar item is always what the main hand holds; the
        // `left_handed` flag only mirrors which side it's drawn on (handled in
        // the renderer), it does not change which item is shown.
        let held = self
            .hotbar
            .get(self.selected_slot as usize)
            .and_then(|s| s.as_ref());
        let item_name = held.map(|i| i.item.clone());
        let item_uv = item_name.as_deref().and_then(|n| self.item_icons.uv(n));
        let item_is_block = item_name
            .as_deref()
            .is_some_and(|n| self.block_names.contains(n));
        // A real 3D block model for a held block (bedwars: blocks in hand).
        let block_quads = if item_is_block {
            item_name
                .as_deref()
                .and_then(|n| self.held_block_geometry(n))
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
        // Which stance the use pose takes, and how far a bow/crossbow is drawn.
        // Vanilla measures the draw from when the use began, not from the item.
        let held_name = held.map(|i| i.item.as_str()).unwrap_or("");
        let use_kind = match held_name {
            "bow" => crate::render::UseKind::Bow,
            "crossbow" => crate::render::UseKind::Crossbow,
            "shield" => crate::render::UseKind::Shield,
            "trident" => crate::render::UseKind::Trident,
            // All seven material tiers of the real 26.1 kinetic-charge spear
            // (`wooden_spear` .. `netherite_spear`, confirmed in azalea's
            // `ItemKind` registry and the client jar's own item textures) —
            // without this they fell through to the generic eat/drink raise.
            "wooden_spear" | "stone_spear" | "golden_spear" | "iron_spear" | "copper_spear"
            | "diamond_spear" | "netherite_spear" => crate::render::UseKind::Spear,
            _ => crate::render::UseKind::Generic,
        };
        // A bow draws over 20 ticks; vanilla shows three sprites across it.
        let draw_secs = self
            .use_start
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.0);
        let item_uv = match use_kind {
            crate::render::UseKind::Bow | crate::render::UseKind::Crossbow if using > 0.0 => {
                let stage = match draw_secs {
                    t if t < 0.30 => 0,
                    t if t < 0.65 => 1,
                    _ => 2,
                };
                self.item_icons
                    .uv(&format!("{held_name}_pulling_{stage}"))
                    .or(item_uv)
            }
            _ => item_uv,
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
            light: self
                .player
                .as_ref()
                .map(|p| self.light_at_pos([p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]]))
                .unwrap_or([1.0, 1.0]),
            // What the held item is being used for decides the stance, and a
            // drawn bow or crossbow swaps to its own pulling sprite.
            use_kind,
            // Holding a filled map switches to vanilla's two-handed map pose.
            map: held
                .filter(|i| i.item == "filled_map")
                .and_then(|i| i.map_id)
                .and_then(|id| self.map_tex.get(&id).copied()),
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
        match self
            .hotbar
            .get(self.selected_slot as usize)
            .and_then(|s| s.as_ref())
        {
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

    /// The biome the camera is standing in, or the registry's first entry (and
    /// failing that plains-ish defaults) before any chunks have arrived.
    fn biome_here(&self) -> crate::bridge::events::BiomeInfo {
        let id = self
            .player
            .as_ref()
            .and_then(|p| {
                self.mirror.biome_at(BlockPos {
                    x: p.pos[0].floor() as i32,
                    y: (p.pos[1] + p.eye_height as f64).floor() as i32,
                    z: p.pos[2].floor() as i32,
                })
            })
            .unwrap_or(0);
        self.biomes.get(id as usize).cloned().unwrap_or_default()
    }

    /// The camera's biome, namespaced like vanilla's F3 line.
    fn biome_name_here(&self) -> String {
        format!("minecraft:{}", self.biome_here().name)
    }

    /// The block under the crosshair with its state properties — vanilla's
    /// right-hand F3 column. `None` when nothing is in reach.
    fn targeted_block(&self) -> Option<(BlockPos, String, Vec<(String, String)>)> {
        let p = self.player.as_ref()?;
        let eye = [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]];
        let d = camera::view_dir(self.yaw, self.pitch);
        let table = &self.table;
        let (pos, _) =
            self.mirror
                .raycast(eye, [d.x as f64, d.y as f64, d.z as f64], 5.0, |id| {
                    table.is_air(id)
                })?;
        let entry = table.entry(self.mirror.get_block(pos))?;
        Some((pos, entry.name.clone(), entry.props.clone()))
    }

    /// This frame's inputs to vanilla's light texture. `daylight` is the sky
    /// strength the sky colour was built from.
    fn lightmap_params(&self, daylight: f32) -> LightmapParams {
        // A lightning bolt washes the sky out to full daylight for a moment,
        // then fades — vanilla's `skyFlashTime`. The accessibility toggle
        // only suppresses this flash; the bolt model itself still renders.
        let flash = if self.settings.hide_lightning_flash {
            0.0
        } else {
            self.lightning
                .iter()
                .map(|(_, _, at)| at.elapsed().as_secs_f32())
                .fold(0.0f32, |best, age| {
                    best.max((1.0 - age / 0.4).clamp(0.0, 1.0))
                })
        };
        // Darkness (warden / sculk shrieker) pulls the whole ramp down in
        // waves by default; the accessibility toggle holds it at the wave's
        // average instead of animating.
        let darkness = if self.active_effects.contains_key("darkness") {
            if self.settings.darkness_pulsing {
                let t = self.start.elapsed().as_secs_f32();
                0.35 + 0.30 * (t * 2.2).sin().max(0.0)
            } else {
                0.5
            }
        } else {
            0.0
        };
        LightmapParams {
            // Skylight-less dimensions have no day cycle, and their chunks
            // carry no sky-light array at all — what light there is comes from
            // the dimension's ambient floor and nearby block light.
            daylight: if !self.connected || self.dim_skylight {
                daylight
            } else {
                0.0
            },
            ambient: self.dim_ambient,
            flicker: self.light_flicker,
            night_vision: if self.active_effects.contains_key("night_vision") {
                1.0
            } else {
                0.0
            },
            gamma: self.settings.brightness,
            end: self.connected && !self.dim_skylight && !self.dim_ultrawarm,
            flash,
            darkness,
        }
    }

    /// `(block, sky)` light at a world position, normalised to 0..1 for the
    /// light texture. Entities, block entities and the held item all use this.
    fn light_at_pos(&self, pos: [f64; 3]) -> [f32; 2] {
        if !self.connected {
            return [1.0, 1.0];
        }
        let bp = BlockPos {
            x: pos[0].floor() as i32,
            y: pos[1].floor() as i32,
            z: pos[2].floor() as i32,
        };
        let (sky, blk) = self.mirror.light_at(bp);
        [blk as f32 / 15.0, sky as f32 / 15.0]
    }

    /// A picture of every pattern the open loom could weave, drawn onto the
    /// banner that is actually in it. Vanilla renders the banner model into
    /// each button; we composite the cloth once per (banner, pattern) pair and
    /// keep it, so a loom costs nothing to look at after the first frame.
    fn loom_previews(&mut self) -> Vec<egui::TextureId> {
        let Some(view) = self.hud.container.as_ref() else {
            return Vec::new();
        };
        if view.kind != "loom" {
            return Vec::new();
        }
        // The banner in the first slot decides the base colour; its existing
        // patterns are under everything the loom would add.
        let Some(banner) = view.slots.first().and_then(|s| s.as_ref()) else {
            return Vec::new();
        };
        let Some((base, _)) = blockentities::banner_base(&banner.item) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(blockentities::LOOM_PATTERNS.len());
        // The dye in the second slot is the colour the new pattern is woven in.
        let dye = view
            .slots
            .get(1)
            .and_then(|s| s.as_ref())
            .and_then(|i| blockentities::dye_id(&i.item))
            .unwrap_or(base);
        for pattern in blockentities::LOOM_PATTERNS {
            let key = format!("loom:{base}:{dye}:{pattern}");
            if !self.loom_tex.contains_key(&key) {
                let layers = vec![(pattern.to_owned(), dye)];
                let Some(img) = blockentities::banner_preview(&mut self.pack, base, &layers) else {
                    continue;
                };
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                );
                let tex = self
                    .egui_ctx
                    .load_texture(&key, color, egui::TextureOptions::NEAREST);
                self.loom_tex.insert(key.clone(), tex);
            }
            if let Some(tex) = self.loom_tex.get(&key) {
                out.push(tex.id());
            }
        }
        out
    }

    /// The icons the beacon's buttons need, loaded once each.
    fn beacon_effect_icons(&mut self) -> HashMap<String, egui::TextureId> {
        const BEACON_EFFECTS: [&str; 6] = [
            "speed",
            "haste",
            "resistance",
            "jump_boost",
            "strength",
            "regeneration",
        ];
        if self
            .hud
            .container
            .as_ref()
            .is_none_or(|v| v.kind != "beacon")
        {
            return HashMap::new();
        }
        let mut out = HashMap::new();
        for name in BEACON_EFFECTS {
            if !self.effect_tex.contains_key(name)
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
                self.effect_tex.insert(name.to_owned(), tex);
            }
            if let Some(tex) = self.effect_tex.get(name) {
                out.insert(name.to_owned(), tex.id());
            }
        }
        out
    }

    /// Prune expired potion effects and build the top-right HUD list, lazily
    /// loading each effect's `mob_effect/<name>` icon texture.
    fn active_effect_hud(&mut self) -> Vec<hud::EffectHud> {
        let now = Instant::now();
        self.active_effects
            .retain(|_, (_, expiry)| expiry.is_none_or(|e| e > now));
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
        // The gamepad's left stick is an additive input source: it OR's into
        // the same booleans the keyboard produces, so both can be used
        // interchangeably (or together) frame to frame.
        let pad = self.settings.gamepad.enabled;
        let (fw, bk, lt, rt, sprint_key, sneak_key) = {
            let kb = &self.settings.keys;
            (
                key_down(&self.keys, &kb.forward) || (pad && self.gamepad.forward()),
                key_down(&self.keys, &kb.back) || (pad && self.gamepad.back()),
                key_down(&self.keys, &kb.left) || (pad && self.gamepad.left()),
                key_down(&self.keys, &kb.right) || (pad && self.gamepad.right()),
                key_down(&self.keys, &kb.sprint) || (pad && self.gamepad.sprint_held()),
                key_down(&self.keys, &kb.sneak) || (pad && self.gamepad.sneak_held()),
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
            && if self.settings.sprint_toggle {
                self.sprint_latch
            } else {
                sprint_key
            };
        let mv = (forward, strafe, sprint);
        if mv != self.last_move {
            self.last_move = mv;
            self.send_cmd(Command::Move {
                forward,
                strafe,
                sprint,
            });
        }
        // Sneak state, also change-triggered.
        let sneak = active
            && if self.settings.sneak_toggle {
                self.sneak_latch
            } else {
                sneak_key
            };
        if sneak != self.sneaking {
            self.sneaking = sneak;
            self.send_cmd(Command::Sneak(sneak));
        }
    }

    /// The jump key went down. In the saddle it charges the mount's jump, in
    /// the air over an elytra it opens the wings, and on foot two taps in a
    /// row start flying.
    fn jump_pressed(&mut self) {
        let now = Instant::now();
        if let Some(kind) = self.player.as_ref().and_then(|p| p.vehicle_kind.clone()) {
            if riding::jumpable(&kind) {
                self.ride_jump.press(now);
            }
            return; // riding: no flight toggle, no elytra
        }
        if self.try_start_gliding() {
            return;
        }
        self.jump_tapped();
    }

    /// The F3 chord wireframes: entity hitboxes (F3+B) and the borders of the
    /// chunk under the camera (F3+G). Both are off unless asked for, and both
    /// are plain line boxes, so they cost nothing when they are.
    fn debug_boxes(&self, cam_pos: [f64; 3]) -> Vec<([f64; 3], [f64; 3], [f32; 4])> {
        let mut out = Vec::new();
        if self.show_hitboxes {
            // Vanilla's white box around the collision shape, plus the red
            // line the eyes look along, drawn two blocks out.
            let now = Instant::now();
            let render_t = now;
            for track in self.tracks.values() {
                let (pos, yaw, pitch) = track.sample(render_t);
                let (w, h) = (track.snap.width as f64 * 0.5, track.snap.height as f64);
                if (pos[0] - cam_pos[0]).abs() > 64.0 || (pos[2] - cam_pos[2]).abs() > 64.0 {
                    continue;
                }
                out.push((
                    [pos[0] - w, pos[1], pos[2] - w],
                    [pos[0] + w, pos[1] + h, pos[2] + w],
                    [1.0, 1.0, 1.0, 0.85],
                ));
                // The look vector as a hair-thin box, from the eyes outward.
                let eye = [pos[0], pos[1] + track.snap.height as f64 * 0.85, pos[2]];
                let (sy, cy) = (-yaw.to_radians() as f64).sin_cos();
                let cp = (pitch.to_radians() as f64).cos();
                let dir = [sy * cp, -(pitch.to_radians() as f64).sin(), cy * cp];
                let tip = [
                    eye[0] + dir[0] * 2.0,
                    eye[1] + dir[1] * 2.0,
                    eye[2] + dir[2] * 2.0,
                ];
                out.push((
                    [
                        eye[0].min(tip[0]) - 0.01,
                        eye[1].min(tip[1]) - 0.01,
                        eye[2].min(tip[2]) - 0.01,
                    ],
                    [
                        eye[0].max(tip[0]) + 0.01,
                        eye[1].max(tip[1]) + 0.01,
                        eye[2].max(tip[2]) + 0.01,
                    ],
                    [0.0, 0.0, 1.0, 0.85],
                ));
            }
        }
        if self.show_chunk_borders {
            // The column you are standing in, in yellow, and its eight
            // neighbours in a dimmer blue — vanilla's own colour split.
            let (cx, cz) = (
                (cam_pos[0].floor() as i32) >> 4,
                (cam_pos[2].floor() as i32) >> 4,
            );
            let (bottom, top) = (cam_pos[1] - 64.0, cam_pos[1] + 64.0);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let (x, z) = (((cx + dx) * 16) as f64, ((cz + dz) * 16) as f64);
                    let own = dx == 0 && dz == 0;
                    out.push((
                        [x, bottom, z],
                        [x + 16.0, top, z + 16.0],
                        if own {
                            [1.0, 1.0, 0.0, 0.9]
                        } else {
                            [0.25, 0.5, 1.0, 0.4]
                        },
                    ));
                }
            }
        }
        out
    }

    /// How dark the sleep screen is, `None` while awake. Vanilla counts a
    /// hundred ticks from the moment you lie down and washes the screen over
    /// in that time.
    fn sleep_fade(&self) -> Option<f32> {
        let since = self.sleep_since?;
        Some((since.elapsed().as_secs_f32() / 5.0).clamp(0.0, 1.0))
    }

    /// Vanilla's elytra deploy check (`LocalPlayer.aiStep`): falling, off the
    /// ground, out of the water, wearing an elytra, and not already gliding.
    /// We only ever ask — the server owns the flag, so a broken elytra or a
    /// server that says no simply leaves us falling.
    fn try_start_gliding(&mut self) -> bool {
        let Some(p) = &self.player else { return false };
        let wearing_elytra = p.equipment.chest.as_deref() == Some("elytra");
        if !wearing_elytra
            || p.gliding
            || p.on_ground
            || p.abilities.flying
            || p.eyes_in_water
            || p.velocity[1] >= 0.0
        {
            return false;
        }
        self.send_cmd(Command::StartGliding);
        true
    }

    /// The jump key went down on foot. Two taps in quick succession start or
    /// stop flying, exactly as in vanilla — and only when the server has said
    /// we may (creative and spectator).
    fn jump_tapped(&mut self) {
        let Some(p) = &self.player else { return };
        if !p.abilities.may_fly {
            self.last_jump_tap = Some(Instant::now());
            return;
        }
        // Vanilla's window is 7 ticks.
        let double = self
            .last_jump_tap
            .is_some_and(|t| t.elapsed() < Duration::from_millis(350));
        self.last_jump_tap = Some(Instant::now());
        if !double {
            return;
        }
        let now_flying = !p.abilities.flying;
        self.last_jump_tap = None;
        self.send_cmd(Command::SetFlying(now_flying));
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
        let pushing = self
            .forward_since
            .is_some_and(|t| t.elapsed().as_millis() > 250);
        let on_ground = self.player.as_ref().is_some_and(|p| p.on_ground);
        let speed = self.cam.as_ref().map_or(f64::MAX, |c| {
            (c.vel[0] * c.vel[0] + c.vel[2] * c.vel[2]).sqrt()
        });
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
        self.tick_ambient(frame_dt as f32);
        self.lids.tick(frame_dt as f32);
        self.tick_music(frame_dt as f32);
        self.tick_rain(frame_dt as f32);
        self.tick_light_flicker(frame_dt as f32);
        self.pump_meshing();
        if self.settings.gamepad.enabled {
            self.gamepad.poll(self.settings.gamepad.deadzone);
            self.apply_gamepad_actions();
        }
        self.hud.gamepad_connected = self.settings.gamepad.enabled && self.gamepad.connected();
        self.apply_mouse_look(frame_dt);
        self.push_move_if_changed();
        self.auto_jump_tick();
        // Thrown off mid-charge: the horse is gone, so is the jump.
        if self.ride_jump.charging() && !self.player.as_ref().is_some_and(|p| p.riding) {
            self.ride_jump.cancel();
        }
        self.skins.poll();
        self.upload_skins();
        self.tick_atlas_animations();
        self.tick_dial_items();
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

        let show_tab_list = self.tab_held && self.connected && !self.hud.overlay_open();
        // Refresh the debug FPS at most ~3×/second and round it, so it reads
        // as a steady number instead of churning every frame.
        if self.fps_updated.elapsed() >= Duration::from_millis(333) {
            self.fps_display = fps_of(&self.frame_times).round();
            self.fps_updated = Instant::now();
        }

        // View bobbing: a subtle vertical sway while walking (vanilla-style).
        // Vanilla stops it dead the moment you are carried: a rider's head
        // does not bob with their own footsteps.
        let moving = (self.last_move.0 != 0 || self.last_move.1 != 0)
            && !self.player.as_ref().is_some_and(|p| p.riding || p.gliding);
        if self.settings.view_bobbing && moving {
            let step = if self.last_move.2 { 0.42 } else { 0.30 };
            self.bob_phase =
                (self.bob_phase + step * (frame_dt * 60.0) as f32) % std::f32::consts::TAU;
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
        // Dynamic FOV, vanilla's `getFovModifier`: the view widens with the
        // player's movement-speed attribute (so Speed opens it up and Slowness
        // closes it in), widens again for sprinting and for creative flight,
        // and pulls IN while a bow is drawn. The FOV Effects option scales the
        // whole thing, and 0 pins it — exactly like the slider. The multiplier
        // eases toward its target (~vanilla's half-way-per-tick) rather than
        // snapping.
        let fov_target = if self.connected {
            let ab = self.player.as_ref().map(|p| &p.abilities);
            // How far a bow/crossbow is drawn right now (20 ticks = 1 s).
            let held = self
                .hotbar
                .get(self.selected_slot as usize)
                .and_then(|s| s.as_ref());
            let pull = match held.map(|i| i.item.as_str()) {
                Some("bow") | Some("crossbow") => self
                    .use_start
                    .map(|t| (t.elapsed().as_secs_f32() / 1.0).clamp(0.0, 1.0))
                    .unwrap_or(0.0),
                _ => 0.0,
            };
            viewfx::fov_multiplier(
                ab.map(|a| a.walk_speed).unwrap_or(0.1),
                self.last_move.2,
                ab.is_some_and(|a| a.flying),
                pull,
                self.settings.fov_effects,
            )
        } else {
            1.0
        };
        let ease = 1.0 - (-frame_dt as f32 * 14.0).exp();
        self.fov_mult += (fov_target - self.fov_mult) * ease;
        if (self.fov_mult - fov_target).abs() < 1e-4 {
            self.fov_mult = fov_target;
        }
        fov *= self.fov_mult;
        // Nausea (and standing in a portal) makes the world swim: vanilla
        // breathes the FOV and rolls the view slightly, ramping the effect in
        // and out rather than switching it on.
        let nausea_target = if self.connected && self.active_effects.contains_key("nausea") {
            1.0
        } else {
            0.0
        };
        let n_ease = 1.0 - (-frame_dt as f32 * 1.6).exp();
        self.nausea_mix += (nausea_target - self.nausea_mix) * n_ease;
        if (self.nausea_mix - nausea_target).abs() < 1e-3 {
            self.nausea_mix = nausea_target;
        }
        let nausea = viewfx::nausea_amount(self.nausea_mix, self.portal_amount());
        let (nausea_fov, nausea_roll) =
            viewfx::nausea_warp(nausea, self.start.elapsed().as_secs_f32());
        fov *= nausea_fov;
        // The camera only ever rolls for these two: the flinch when something
        // hits you, and that swim.
        let damage_roll = match self.hurt_at {
            Some(at) if self.settings.damage_tilt => viewfx::damage_tilt(
                at.elapsed().as_secs_f32(),
                yaw,
                if self.hurt_from_yaw.is_finite() {
                    self.hurt_from_yaw
                } else {
                    yaw
                },
                1.0,
            ),
            _ => 0.0,
        };
        let roll_deg = damage_roll + nausea_roll;
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
        let loom_previews = self.loom_previews();
        let effect_icons = self.beacon_effect_icons();

        // The totem flash runs for two seconds and then forgets itself.
        let totem_flash = self.totem_flash.and_then(|at| {
            let t = at.elapsed().as_secs_f32() / 2.0;
            (t < 1.0).then_some(t)
        });
        if totem_flash.is_none() {
            self.totem_flash = None;
        }
        let hud_state = HudState {
            totem_flash,
            fps: self.fps_display,
            pos: self.player.as_ref().map_or([0.0; 3], |p| p.pos),
            yaw: self.yaw,
            pitch: self.pitch,
            health: self.player.as_ref().map_or(0.0, |p| p.health),
            game_mode: self.player.as_ref().map_or(0, |p| p.game_mode),
            flying: self.player.as_ref().is_some_and(|p| p.abilities.flying),
            gliding: self.player.as_ref().is_some_and(|p| p.gliding),
            jump_charge: self.ride_jump.charge(),
            locator: self.locator_dots(),
            sleeping: self.sleep_fade(),
            vehicle: self.player.as_ref().and_then(|p| p.vehicle_kind.clone()),
            mount_kind: self.mount.as_ref().map(|m| m.kind.clone()),
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
                if self.settings.darkness_pulsing {
                    // Vanilla darkness pulses the screen darker in waves.
                    let t = self.start.elapsed().as_secs_f32();
                    0.30 + 0.28 * (t * 2.2).sin().max(0.0)
                } else {
                    0.44 // the wave's average, held steady
                }
            } else {
                0.0
            },
            border_warning: self.border_warning(),
            poisoned: self.active_effects.contains_key("poison"),
            withered: self.active_effects.contains_key("wither"),
            freeze: self.player.as_ref().map_or(0.0, |p| p.freeze),
            pumpkin: self
                .player
                .as_ref()
                .is_some_and(|p| p.equipment.head.as_deref() == Some("carved_pumpkin")),
            spyglass: spyglass_active,
            portal: self.portal_amount(),
            // Vanilla's nausea overlay ramps with the effect's remaining time;
            // a plain "is it active" flag is enough to drive it here.
            nausea: if self.active_effects.contains_key("nausea") {
                1.0
            } else {
                0.0
            },
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
            connecting: (self.bridge.is_some() || self.reconnect_at.is_some()) && !self.connected,
            singleplayer_available: self.opts.server_binary.is_some(),
            singleplayer_starting: self.singleplayer_starting.is_some(),
            connect_attempt: self.connect_attempt,
            resource_pack_status: self.resource_pack_status.clone(),
            disconnect_reason: self.disconnect_reason.clone(),
            menu_time: self.start.elapsed().as_secs_f32(),
            show_tab_list,
            nametags,
            entities_count,
            render_distance: self.settings.render_distance,
            sidebar_title: self.sidebar_title.clone(),
            sidebar_lines: self.sidebar_lines.clone(),
            boss_bars: self
                .boss_bars
                .iter()
                .map(|(_, b)| hud::BossBarHud {
                    name: b.name.clone(),
                    progress: b.progress,
                    color: boss_bar_color(b.color),
                    notches: boss_bar_notches(b.overlay),
                })
                .collect(),
            hud_hidden: self.hud_hidden,
            hurt_flash: self
                .hurt_flash_until
                .and_then(|t| t.checked_duration_since(Instant::now()))
                .map_or(0.0, |d| (d.as_secs_f32() / 0.5).clamp(0.0, 1.0)),
            own_skin: self.own_skin_url(),
            attack_indicator: self.settings.attack_indicator,
            reduced_debug_info: self.settings.reduced_debug_info || self.server_reduced_debug_info,
            biome: self.biome_name_here(),
            dimension: format!("minecraft:{}", self.dim_name),
            light_here: self
                .player
                .as_ref()
                .map(|p| {
                    self.mirror.light_at(BlockPos {
                        x: p.pos[0].floor() as i32,
                        y: p.pos[1].floor() as i32,
                        z: p.pos[2].floor() as i32,
                    })
                })
                .unwrap_or((15, 0)),
            world_time: self.world_time,
            targeted: self.targeted_block(),
            text_bg_opacity: self.settings.text_background_opacity,
            server_address: self
                .connect_target
                .as_ref()
                .map(|(a, _, _)| a.clone())
                .unwrap_or_default(),
            session_secs: self
                .session_start
                .map_or(0.0, |t| t.elapsed().as_secs_f32()),
            maps: self.map_egui.iter().map(|(id, h)| (*id, h.id())).collect(),
            container_data: self.container_data.clone(),
            enchantments: self.enchantments.clone(),
            trim_patterns: self.trim_patterns.clone(),
            trim_materials: self.trim_materials.clone(),
            instruments: self.instruments.clone(),
            stonecutter: self.stonecutter.clone(),
            loom_previews,
            effect_icons,
            previews: self.preview_textures(),
            // Riding something alive shows its health instead of our hunger.
            mount_health: self
                .mount_snapshot()
                .and_then(|m| Some((m.health?, m.max_health?)))
                .filter(|(_, max)| *max > 0.0),
        };
        let mut raw_input = self
            .egui_state
            .as_mut()
            .expect("egui_state present")
            .take_egui_input(&window);
        if self.settings.gamepad.enabled && !self.grabbed {
            self.inject_gamepad_menu_nav(&mut raw_input);
        }
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
            primitives: self
                .egui_ctx
                .tessellate(output.shapes, output.pixels_per_point),
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
        // The biome under the camera drives the fog (and, where there is no sky,
        // the sky colour too) — this is what makes the crimson forest red and
        // the warped forest teal.
        let here = self.biome_here();
        let (mut sky_color, mut daylight) = if !self.connected {
            ([0.08, 0.09, 0.12], self.daylight) // menu (or panorama override below)
        } else if self.dim_skylight {
            (overworld_sky_color(self.world_time), self.daylight)
        } else if self.dim_ultrawarm {
            // The Nether's colour is the biome's, not one flat red.
            (rgb_f32(here.fog), 0.0)
        } else {
            (rgb_f32(here.sky), 0.0) // The End
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
        let lightmap = self.lightmap_params(daylight);
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
        // Other players' mining: the server streams a 0..9 crack stage per
        // miner, so several blocks can be cracking at once.
        let other_cracks: Vec<([f64; 3], u32)> = self
            .block_destruction
            .values()
            .map(|(pos, stage)| ([pos.x as f64, pos.y as f64, pos.z as f64], *stage as u32))
            .collect();
        let mut sky_color = if self.connected || show_panorama {
            if self.connected {
                sky_color
            } else {
                [0.47, 0.65, 1.0]
            }
        } else {
            [0.08, 0.09, 0.12] // panorama missing: keep the title moody
        };
        let mut fog_start = if self.settings.fog {
            fog_end * 0.75
        } else {
            fog_end - 1.0
        };
        let mut fog_end = fog_end;
        // A boss that asks for it darkens the sky and closes the world in —
        // vanilla's Wither and ender dragon both do, and it is most of what
        // makes those fights feel like fights.
        if self.boss_bars.iter().any(|(_, b)| b.darken_screen) {
            for c in &mut sky_color {
                *c *= 0.45;
            }
            daylight *= 0.55;
        }
        if self.boss_bars.iter().any(|(_, b)| b.world_fog) {
            fog_end = fog_end.min(56.0);
            fog_start = fog_start.min(fog_end * 0.35);
        }
        // In a skylight dimension the distant haze takes the biome's fog colour
        // rather than the sky's, which is what gives a swamp its murk and a
        // badlands its dust.
        if self.connected && self.dim_skylight && self.settings.fog {
            let biome_fog = rgb_f32(here.fog);
            // The fog follows the day: at night it is the sky, not the biome.
            let k = 0.5 * self.daylight.clamp(0.0, 1.0);
            for c in 0..3 {
                sky_color[c] += (biome_fog[c] - sky_color[c]) * k;
            }
        }
        // Vanilla's submerged fog: water closes the view to the biome's own
        // water-fog colour, lava blinds you almost completely, and powder snow
        // leaves barely more than arm's reach.
        if self.connected {
            let p = self.player.as_ref();
            if p.is_some_and(|p| p.eyes_in_lava) {
                sky_color = [0.6, 0.1, 0.0];
                fog_start = 0.25;
                fog_end = 2.0;
            } else if p.is_some_and(|p| p.freeze >= 1.0) {
                sky_color = [0.62, 0.73, 0.80];
                fog_start = 0.0;
                fog_end = 4.0;
            } else if p.is_some_and(|p| p.eyes_in_water) {
                sky_color = rgb_f32(here.water_fog);
                fog_start = 0.0;
                fog_end = fog_end.min(48.0);
            }
            // Blindness closes the world down to a few blocks; darkness does the
            // same more gently, and both ignore the render distance.
            if self.active_effects.contains_key("blindness") {
                fog_start = 0.0;
                fog_end = fog_end.min(5.0);
            } else if self.active_effects.contains_key("darkness") {
                fog_start = fog_start.min(2.0);
                fog_end = fog_end.min(15.0);
            }
        }
        let scene = SceneParams {
            cam_pos,
            yaw,
            pitch,
            fov_deg: fov,
            roll_deg,
            daylight: (daylight * gamma).clamp(0.05, 1.0),
            fog_start,
            fog_end,
            sky_color,
            panorama: show_panorama,
            outline,
            debug_boxes: self.debug_boxes(cam_pos),
            crack,
            other_cracks,
            border: self.border_params(),
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
            lightmap,
            end_sky: self.connected && !self.dim_skylight && !self.dim_ultrawarm,
            gui_entities: self.gui_entities(),
        };

        let entities = self.entity_draws(scene.cam_pos);
        if let Some(renderer) = &mut self.renderer {
            let stats = renderer.frame(&scene, &entities, Some(egui_frame))?;
            self.last_stats = (stats.sections_drawn, stats.sections_total);
        }

        // --- hud actions ---------------------------------------------------------
        for action in actions {
            match action {
                HudAction::Narrate(label) => {
                    self.narrator.speak(self.settings.narrator, crate::narrator::Category::System, &label)
                }
                HudAction::SendChat(msg) => self.send_cmd(Command::Chat(msg)),
                HudAction::ChatClosed => {} // grab restores automatically
                HudAction::TabComplete { id, text } => {
                    self.send_cmd(Command::TabComplete { id, text });
                }
                HudAction::SlotClick {
                    window_id,
                    slot,
                    kind,
                } => {
                    // The mount screen is ours, not azalea's, so its clicks
                    // take the hand-written packet.
                    if self
                        .mount
                        .as_ref()
                        .is_some_and(|m| m.container_id == window_id)
                    {
                        self.send_cmd(Command::MountClick {
                            container_id: window_id,
                            slot,
                            kind,
                        });
                    } else {
                        self.send_cmd(Command::ContainerClick {
                            window_id,
                            slot,
                            kind,
                        });
                    }
                }
                HudAction::SelectTrade { index } => {
                    self.send_cmd(Command::SelectTrade { index });
                }
                HudAction::RecipeBookChangeSettings { kind, open, filtering } => {
                    self.send_cmd(Command::RecipeBookChangeSettings { kind, open, filtering });
                }
                HudAction::PlaceRecipe { container_id, recipe, use_max_items } => {
                    self.send_cmd(Command::PlaceRecipe { container_id, recipe, use_max_items });
                    self.send_cmd(Command::RecipeBookSeenRecipe { recipe });
                }
                HudAction::BundleSelectItem { window_id, slot, selected } => {
                    self.send_cmd(Command::BundleSelectItem { window_id, slot, selected });
                }
                HudAction::ContainerSlotStateChanged { window_id, slot, enabled } => {
                    self.send_cmd(Command::ContainerSlotStateChanged { window_id, slot, enabled });
                }
                HudAction::LeaveBed => {
                    self.sleep_since = None;
                    self.send_cmd(Command::StopSleeping);
                }
                HudAction::Connect { address, username, resource_pack_policy } => {
                    self.start_connect(address, username, resource_pack_policy, 1);
                }
                HudAction::PlaySingleplayer { id } => {
                    self.start_singleplayer(id);
                }
                HudAction::SettingsChanged => {
                    self.settings.clamp();
                    self.settings.save();
                    // vsync / fullscreen / GUI scale are applied next frame.
                    self.settings_dirty = true;
                    // Smooth lighting is baked into the vertices, so switching
                    // it re-meshes everything that is loaded.
                    if self.settings.smooth_lighting != self.smooth_lighting_meshed {
                        self.smooth_lighting_meshed = self.settings.smooth_lighting;
                        self.mirror.mark_all_dirty();
                    }
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
                    self.shutdown_singleplayer();
                }
                HudAction::BackToMenu => {
                    self.disconnect_reason = None;
                    self.connected = false;
                    self.bridge = None;
                    self.reset_world_state();
                    self.shutdown_singleplayer();
                }
                HudAction::Quit => {
                    self.shutdown_singleplayer_blocking();
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
                HudAction::Respawn => {
                    self.send_cmd(Command::Respawn);
                    self.hud.clear_death_screen();
                }
                HudAction::RequestStats => self.send_cmd(Command::RequestStats),
                HudAction::ContainerButton { window_id, button } => {
                    self.send_cmd(Command::ContainerButton { window_id, button });
                }
                HudAction::RenameItem { name } => self.send_cmd(Command::RenameItem { name }),
                HudAction::SetBeacon { primary, secondary } => {
                    self.send_cmd(Command::SetBeacon { primary, secondary });
                }
                HudAction::CloseContainer { id } => {
                    self.hud.container_closed(id);
                    if self.mount.take().is_some_and(|m| m.container_id == id) {
                        self.send_cmd(Command::CloseMount { container_id: id });
                    } else {
                        self.send_cmd(Command::CloseContainer { id });
                    }
                }
                HudAction::SignUpdate { pos, front, lines } => {
                    self.send_cmd(Command::SignUpdate { pos, front, lines });
                }
                HudAction::EditBook { off_hand, pages, title } => {
                    // Raw `Inventory` slot vanilla's `ServerboundEditBook`
                    // expects: the selected hotbar slot (0-8), or 40 off hand.
                    let slot = if off_hand { 40 } else { self.selected_slot as u32 };
                    self.send_cmd(Command::EditBook { slot, pages, title });
                }
                HudAction::CreativeSet { slot, item, count } => {
                    self.send_cmd(Command::CreativeSlot { slot, item, count });
                }
                HudAction::ResourcePackResponse { id, accept } => {
                    self.resource_pack_status = accept.then(|| "Preparing server resource pack...".into());
                    if !self.connected {
                        self.connect_deadline = Some(
                            Instant::now()
                                + if accept { Duration::from_secs(90) } else { Duration::from_secs(15) },
                        );
                    }
                    self.send_cmd(Command::ResourcePackResponse { id, accept });
                }
                HudAction::ReloadResourcePacks { enabled } => {
                    self.local_packs.enabled = enabled;
                    self.local_packs.save();
                    match self.rebuild_resource_pack_stack() {
                        Ok(()) => {
                            self.hud.push_chat(vec![ChatSpan::plain("Resource packs reloaded.")], true);
                        }
                        Err(e) => {
                            warn!("app: local resource-pack reload failed: {e:#}");
                            self.hud.push_chat(
                                vec![ChatSpan::plain(format!("Resource-pack reload failed: {e}"))],
                                true,
                            );
                        }
                    }
                }
                HudAction::ClearResourcePackCache => {
                    if self.server_packs.is_empty() {
                        match resourcepacks::clear_server_cache() {
                            Ok(removed) => {
                                self.hud.push_chat(
                                    vec![ChatSpan::plain(format!(
                                        "Cleared {removed} server-pack cache files."
                                    ))],
                                    true,
                                );
                            }
                            Err(e) => warn!("app: clearing server-pack cache failed: {e}"),
                        }
                    } else {
                        self.hud.push_chat(
                            vec![ChatSpan::plain(
                                "Disconnect before clearing active server packs.",
                            )],
                            true,
                        );
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
    fn start_connect(
        &mut self,
        address: String,
        username: String,
        resource_pack_policy: ServerResourcePackPolicy,
        attempt: u32,
    ) {
        info!(address, username, attempt, "app: connect attempt");
        self.clear_server_resource_packs();
        // Use the account resolved at startup (launcher session / Microsoft).
        // Only offline mode takes the username field.
        let account = match &self.opts.bridge.account {
            AccountConfig::Offline(_) => AccountConfig::Offline(username.clone()),
            other => other.clone(),
        };
        self.connect_target = Some((address.clone(), username, resource_pack_policy));
        self.connect_attempt = attempt;
        self.reconnect_at = None;
        match spawn_bridge(BridgeOptions {
            account,
            address,
            view_distance: self.settings.render_distance.clamp(2, 32) as u8,
            resource_pack_policy,
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

    /// `ClientboundTransfer`: the server is redirecting us to a different
    /// host:port. Decompiled `ClientCommonPacketListenerImpl.handleTransfer`
    /// shows real vanilla does this with **no confirmation prompt** — it
    /// disconnects and immediately reconnects via `ConnectScreen.startConnecting`
    /// under the *same already-authenticated* `Minecraft.getUser()`, only
    /// `ServerAddress` changes. `start_connect` already resolves the account
    /// from `self.opts.bridge.account` the same way on every connect, so
    /// reusing it here for the new address is a correct transcription, not a
    /// shortcut. Username/resource-pack policy carry over from the
    /// connection being replaced.
    fn start_transfer(&mut self, host: String, port: u32) {
        let (username, resource_pack_policy) = self
            .connect_target
            .as_ref()
            .map(|(_, username, policy)| (username.clone(), *policy))
            .unwrap_or_default();
        info!(host, port, "app: server requested a transfer");
        // Best-effort close of the connection being replaced, same as
        // `HudAction::Disconnect` — never wait for it to confirm.
        self.send_cmd(Command::Disconnect);
        self.start_connect(format!("{host}:{port}"), username, resource_pack_policy, 1);
    }

    /// Singleplayer "Play": spawn the bundled local server for this world,
    /// then wait for its port to open (`poll_singleplayer_starting` finishes
    /// the job by handing off to the normal `start_connect`).
    fn start_singleplayer(&mut self, world_id: String) {
        self.shutdown_singleplayer();
        let Some(binary) = self.opts.server_binary.clone() else {
            warn!("app: singleplayer requested but no server binary is bundled");
            self.hud.reset_to_title();
            self.disconnect_reason = Some("No local server was found.".to_string());
            return;
        };
        match crate::singleplayer::SingleplayerServer::spawn(&binary, &world_id) {
            Ok(server) => {
                info!(world_id, port = server.port(), "app: local server starting");
                self.singleplayer = Some(server);
                self.singleplayer_starting = Some(Instant::now());
            }
            Err(e) => {
                warn!("app: failed to start local server: {e:#}");
                self.hud.reset_to_title();
                self.disconnect_reason = Some(format!("failed to start local server: {e:#}"));
            }
        }
    }

    /// Once-per-frame: while a local server is booting, poll its port; once
    /// it opens, connect to it exactly like a normal `HudAction::Connect`.
    fn poll_singleplayer_starting(&mut self) {
        let Some(since) = self.singleplayer_starting else { return };
        let Some(server) = &mut self.singleplayer else {
            self.singleplayer_starting = None;
            return;
        };
        if server.has_exited() {
            warn!("app: local server exited before it finished starting");
            self.singleplayer_starting = None;
            self.singleplayer = None;
            self.hud.reset_to_title();
            self.disconnect_reason = Some("The local server stopped unexpectedly.".to_string());
            return;
        }
        if since.elapsed() > Duration::from_secs(30) {
            warn!("app: local server did not open its port in time");
            self.singleplayer_starting = None;
            self.shutdown_singleplayer();
            self.hud.reset_to_title();
            self.disconnect_reason = Some("The local server took too long to start.".to_string());
            return;
        }
        if server.is_ready() {
            let address = server.address();
            self.singleplayer_starting = None;
            let username = match &self.opts.bridge.account {
                AccountConfig::Offline(name) if !name.trim().is_empty() => name.clone(),
                _ => "Dolphin".to_string(),
            };
            self.start_connect(address, username, ServerResourcePackPolicy::Disabled, 1);
        }
    }

    /// Stop any running local server (Singleplayer) without blocking the UI
    /// thread. Safe to call when none is running.
    fn shutdown_singleplayer(&mut self) {
        self.singleplayer_starting = None;
        if let Some(server) = self.singleplayer.take() {
            server.shutdown_async();
        }
    }

    /// Stop any running local server and wait for it. Only call this right
    /// before the app itself exits (Quit / window close).
    fn shutdown_singleplayer_blocking(&mut self) {
        self.singleplayer_starting = None;
        if let Some(server) = self.singleplayer.take() {
            server.shutdown_blocking();
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
            info!(
                reason,
                attempt = self.connect_attempt,
                "app: transient connect failure; retrying"
            );
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
        self.boss_bars.clear();
        self.waypoints.clear();
        self.server_reduced_debug_info = false;
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
        self.clear_server_resource_packs();
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
                .map(|(addr, _, _)| crate::discord::server_host(addr))
                .filter(|h| !crate::discord::is_raw_ip(h));
            let details = match host {
                Some(h) => format!("Playing on {h}"),
                None => "Playing on a server".to_string(),
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
                details: Some("In the main menu".to_string()),
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
        let gain = self
            .settings
            .category_volume(crate::settings::SoundCategory::Blocks);
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

    /// Is this position inside water? (Feet-level check, for splashes.)
    fn in_water(&self, pos: [f64; 3]) -> bool {
        let bp = BlockPos {
            x: pos[0].floor() as i32,
            y: (pos[1] + 0.1).floor() as i32,
            z: pos[2].floor() as i32,
        };
        self.table.fluid_kind(self.mirror.get_block(bp)) == Some("water")
    }

    /// The block a body standing here has under its feet, if any.
    fn floor_block(&self, pos: [f64; 3]) -> Option<(BlockPos, StateId)> {
        let bp = BlockPos {
            x: pos[0].floor() as i32,
            y: (pos[1] - 0.2).floor() as i32,
            z: pos[2].floor() as i32,
        };
        let state = self.mirror.get_block(bp);
        (!self.table.is_air(state)).then_some((bp, state))
    }

    /// Play one movement sound at a world position.
    fn play_step_event(&self, ev: footsteps::StepEvent, pos: [f64; 3], kind: &str) {
        match ev {
            footsteps::StepEvent::Step => {
                // Vanilla plays the *block's* step sound at 15 % volume.
                if let Some((bp, state)) = self.floor_block(pos) {
                    self.play_block_sound("step", state, bp, 0.15, 1.0);
                }
            }
            footsteps::StepEvent::Land { big } => {
                // A landing is the fall sound plus the block underfoot.
                self.play_world_sound(&footsteps::fall_sound(kind, big), pos, 1.0, 1.0);
                if let Some((bp, state)) = self.floor_block(pos) {
                    self.play_block_sound("step", state, bp, 0.5, 0.75);
                }
            }
            footsteps::StepEvent::Splash => {
                self.play_world_sound(&footsteps::splash_sound(kind), pos, 1.0, 1.0);
            }
            footsteps::StepEvent::Swim => {
                self.play_world_sound(&footsteps::swim_sound(kind), pos, 0.4, 1.0);
            }
        }
    }

    /// The sound of our own swing landing. The server never sends this: in
    /// vanilla `Player.attack` picks one of five sounds from how charged the
    /// swing was and what the player was doing, and plays it locally.
    fn play_attack_sound(&mut self, target: u64) {
        let Some(p) = &self.player else { return };
        let strength = p.attack_strength;
        let sprinting = self.last_move.2;
        // Vanilla's crit: falling, not on the ground, not on a ladder, not in
        // water, not riding, and swinging at full strength.
        let crit =
            strength > 0.9 && !p.on_ground && p.velocity[1] < 0.0 && !p.riding && !p.eyes_in_water;
        let name = if strength <= 0.9 {
            "entity.player.attack.weak"
        } else if crit {
            "entity.player.attack.crit"
        } else if sprinting {
            "entity.player.attack.knockback"
        } else {
            "entity.player.attack.strong"
        };
        let at = self
            .tracks
            .get(&target)
            .map(|t| t.snap.pos)
            .unwrap_or(p.pos);
        self.play_world_sound(name, at, 1.0, 1.0);
        // A full-strength hit throws vanilla's crit sparks over the victim.
        if crit && let Some(t) = self.tracks.get(&target) {
            let (h, w) = (t.snap.height.max(0.5), t.snap.width.max(0.4));
            let mid = [at[0], at[1] + h as f64 * 0.5, at[2]];
            self.spawn_particles(
                mid,
                ParticleTex::Crit,
                [1.0, 1.0, 1.0],
                0.10,
                10,
                [w * 0.5, h * 0.4, w * 0.5],
                0.6,
                1.5,
                None,
            );
        }
    }

    /// Our own steps, landings and splashes. The server sends none of these —
    /// in vanilla the client makes its own noise as it moves.
    fn own_footsteps(&mut self, p: &PlayerSnapshot) {
        let prev = self.player.as_ref().map(|old| old.pos).unwrap_or(p.pos);
        let in_water = self.in_water(p.pos);
        let Some(ev) = self.own_steps.update(p.pos, prev, p.on_ground, in_water) else {
            return;
        };
        self.play_step_event(ev, p.pos, "player");
        // Sprinting kicks up the block underfoot, and so does a hard landing.
        match ev {
            // `last_move.2` is what we last told the server: are we sprinting?
            footsteps::StepEvent::Step if self.last_move.2 => self.spawn_step_dust(p.pos, 2),
            footsteps::StepEvent::Land { big: true } => self.spawn_step_dust(p.pos, 10),
            _ => {}
        }
    }

    /// The little puff a sprinting or landing body throws up behind it.
    fn spawn_step_dust(&mut self, pos: [f64; 3], count: u32) {
        let Some((_, state)) = self.floor_block(pos) else {
            return;
        };
        // Take the block's own colour so sand puffs yellow and grass green.
        let color = self
            .table
            .entry(state)
            .map(|e| block_dust_color(&e.short_name))
            .unwrap_or([0.55, 0.50, 0.45]);
        self.spawn_particles(
            [pos[0], pos[1] + 0.1, pos[2]],
            ParticleTex::Generic,
            color,
            0.10,
            count,
            [0.25, 0.05, 0.25],
            0.6,
            3.0,
            None,
        );
    }

    /// A quick gray-brown puff where a block broke (vanilla shows textured
    /// chunks; a neutral puff reads the same at gameplay distance).
    fn spawn_block_break_particles(&mut self, pos: BlockPos) {
        let center = [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5];
        self.spawn_particles(
            center,
            ParticleTex::Generic,
            [0.55, 0.50, 0.45],
            0.12,
            16,
            [0.35, 0.35, 0.35],
            0.15,
            5.0,
            None,
        );
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
            r.set_high_contrast(self.settings.high_contrast);
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

    /// Rebuild the complete selected stack from the pristine jar. Reopening is
    /// what makes server `pop` and local enable/disable truthful: a removed
    /// overlay cannot leak through the zip handles held by the previous pack.
    fn rebuild_resource_pack_stack(&mut self) -> Result<()> {
        let mut pack = AssetPack::open(&self.opts.mc_jar)?;
        crate::assets::load_resource_pack_paths(&mut pack, &self.local_packs.paths());
        for (_, path) in &self.server_packs {
            pack.add_overlay_zip(path)?;
        }
        let (store, mut atlas) = BakedModelStore::bake_all(&mut pack, &self.table)?;
        let item_icons = ItemIcons::bake(&mut pack, &self.table, &store, &atlas);
        self.lang = Lang::load(
            &mut pack,
            self.opts.assets_dir.as_deref(),
            self.opts.asset_index.as_deref(),
            &self.lang_code,
        );
        self.pack = pack;
        self.store = Arc::new(store);
        self.atlas_anim = AtlasAnimator::new(std::mem::take(&mut atlas.animations));
        self.atlas = atlas;
        self.item_icons = Arc::new(item_icons);
        self.icon_tex = None;
        if let Some(r) = &mut self.renderer {
            r.set_atlas(&self.atlas);
            r.set_item_atlas(&self.item_icons.image);
            r.clear_meshes();
        }
        self.mirror.mark_all_dirty();
        Ok(())
    }

    /// Insert or replace one UUID-keyed server pack and only return success
    /// once model baking and GPU atlas replacement have both completed.
    fn apply_server_resource_pack(&mut self, id: uuid::Uuid, path: PathBuf) -> bool {
        info!(%id, path = %path.display(), "app: applying server resource pack");
        let previous = self.server_packs.clone();
        if let Some((_, current)) = self.server_packs.iter_mut().find(|(pack_id, _)| *pack_id == id) {
            *current = path;
        } else {
            self.server_packs.push((id, path));
        }
        if let Err(e) = self.rebuild_resource_pack_stack() {
            self.server_packs = previous;
            warn!(%id, "app: resource-pack reload failed: {e:#}");
            return false;
        }
        self.resource_pack_status = None;
        self.hud.push_chat(vec![ChatSpan::plain("Server resource pack loaded.")], true);
        true
    }

    fn pop_server_resource_pack(&mut self, id: Option<uuid::Uuid>) {
        let old_len = self.server_packs.len();
        match id {
            Some(id) => self.server_packs.retain(|(pack_id, _)| *pack_id != id),
            None => self.server_packs.clear(),
        }
        if self.server_packs.len() != old_len {
            if let Err(e) = self.rebuild_resource_pack_stack() {
                warn!("app: rebuilding after server resource-pack pop failed: {e:#}");
            } else {
                self.hud.push_chat(vec![ChatSpan::plain("Server resource pack removed.")], true);
            }
        }
        self.resource_pack_status = None;
    }

    fn clear_server_resource_packs(&mut self) {
        if self.server_packs.is_empty() {
            self.resource_pack_status = None;
            return;
        }
        self.server_packs.clear();
        if let Err(e) = self.rebuild_resource_pack_stack() {
            warn!("app: restoring local resource-pack stack failed: {e:#}");
        }
        self.resource_pack_status = None;
    }

    fn drain_game_events(&mut self) {
        self.poll_singleplayer_starting();

        // Auto-retry backoff: re-spawn the bridge once the delay elapses.
        if let Some(at) = self.reconnect_at
            && Instant::now() >= at
        {
            self.reconnect_at = None;
            if let Some((address, username, resource_pack_policy)) = self.connect_target.clone() {
                let attempt = self.connect_attempt + 1;
                self.start_connect(address, username, resource_pack_policy, attempt);
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
                "Connecting timed out — the server never finished the login. Try \
                 again; if it keeps happening, restart the launcher."
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
            warn!(
                ?CONNECTION_WATCHDOG,
                "app: connection watchdog fired (bridge went silent); leaving"
            );
            self.connected = false;
            self.bridge = None;
            self.connect_target = None;
            self.connect_deadline = None;
            self.reconnect_at = None;
            self.returning_to_menu = false;
            self.shutdown_singleplayer();
            // Clean up any open pause menu/container/chat so dismissing the
            // timeout error lands on a tidy title screen.
            self.hud.reset_to_title();
            self.disconnect_reason = Some("Lost connection to the server (timed out).".into());
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
            // Remember where the beacons are as the world streams in; scanning
            // every loaded section for them at draw time would be far too slow.
            match &ev {
                GameEvent::Section { pos, data } => {
                    let base = (pos.x * 16, pos.y * 16, pos.z * 16);
                    self.beacons
                        .retain(|b| !(b.x >> 4 == pos.x && b.y >> 4 == pos.y && b.z >> 4 == pos.z));
                    for (i, &id) in data.blocks.iter().enumerate() {
                        if self.is_beacon(id) {
                            self.beacons.push(BlockPos {
                                x: base.0 + (i & 15) as i32,
                                y: base.1 + (i >> 8) as i32,
                                z: base.2 + ((i >> 4) & 15) as i32,
                            });
                        }
                    }
                }
                GameEvent::BlockChanged { pos, state } => {
                    self.beacons.retain(|b| b != pos);
                    if self.is_beacon(*state) {
                        self.beacons.push(*pos);
                    }
                    // Whatever block entity used to be here is gone unless the
                    // new block still has one (a sign being edited keeps its
                    // state and gets fresh NBT on its own packet).
                    if !blockentities::draws_block_entity(&self.table, *state) {
                        self.block_entities.remove(*pos);
                    }
                }
                GameEvent::ChunkUnloaded { pos } => {
                    let (cx, cz) = (pos.x, pos.z);
                    self.block_entities
                        .retain_chunks(|x, z| !(x == cx && z == cz));
                    self.lids.retain_chunks(|x, z| !(x == cx && z == cz));
                    self.open_containers
                        .retain(|p, _| !(p.x == cx && p.z == cz));
                }
                _ => {}
            }
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
                    // The fog/sky/water-fog colours stay per biome and are
                    // looked up live from wherever the camera is.
                    self.biomes = (*infos).clone();
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
                    self.hud.push_chat(
                        vec![ChatSpan::plain(format!("Connected as {username}"))],
                        true,
                    );
                }
                GameEvent::Disconnected { reason } => {
                    self.music.reset();
                    self.mood.reset();
                    self.maps.clear();
                    self.map_tex.clear();
                    self.map_egui.clear();
                    self.block_destruction.clear();
                    self.container_data.clear();
                    self.border = Default::default();
                    warn!(reason, "app: disconnected");
                    let was_connected = self.connected;
                    self.connected = false;
                    self.connect_deadline = None;
                    self.bridge = None;
                    self.clear_server_resource_packs();
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
                GameEvent::Respawn {
                    dimension,
                    has_skylight,
                    ultrawarm,
                    ambient_light,
                } => {
                    // Respawning or changing dimension takes the death screen
                    // down and drops the cracks other players were making.
                    self.hud.clear_death_screen();
                    self.block_destruction.clear();
                    info!(
                        dimension,
                        has_skylight, ultrawarm, "app: dimension change / respawn"
                    );
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
                    self.block_entities.clear();
                    self.open_containers.clear();
                    self.lids.clear();
                    self.pistons.clear();
                    self.lightning.clear();
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
                    self.dim_ambient = ambient_light;
                    self.dim_name = dimension;
                }
                GameEvent::Chat { spans, system, signature, last_seen } => {
                    let text = self.hud.push_signed_chat(spans, system, signature, last_seen);
                    self.narrator.speak(self.settings.narrator, crate::narrator::Category::Chat, &text);
                }
                GameEvent::DeleteChat { signature } => {
                    self.hud.chat.delete_message(&signature);
                }
                GameEvent::LowDiskSpaceWarning => {
                    let title = self
                        .lang
                        .get("chunk.toast.lowDiskSpace")
                        .unwrap_or("Low disk space!")
                        .to_string();
                    let body = self
                        .lang
                        .get("chunk.toast.lowDiskSpace.description")
                        .unwrap_or("Might not be able to save the world.")
                        .to_string();
                    self.hud.toasts.push(toasts::Toast::system(title, body));
                }
                GameEvent::ServerLinks(links) => {
                    self.hud.server_links = links;
                }
                GameEvent::ShowDialog(data) => {
                    self.hud.show_dialog(data);
                }
                GameEvent::ClearDialog => {
                    self.hud.clear_dialog();
                }
                GameEvent::Transfer { host, port } => {
                    self.start_transfer(host, port);
                }
                GameEvent::ChatCompletions { action, entries } => {
                    self.hud.chat.apply_completions(action, entries);
                }
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
                    if self.last_health >= 0.0
                        && p.health > 0.0
                        && p.health < self.last_health - 0.01
                    {
                        // The red flash + hit particles are the "Damage Tilt"
                        // feedback. The hurt SOUND comes from the server's
                        // SoundEntity packet (handled below) like vanilla —
                        // playing it here too would double it.
                        if self.settings.damage_tilt {
                            self.hurt_flash_until =
                                Some(Instant::now() + Duration::from_millis(500));
                            let eye = [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]];
                            self.spawn_particles(
                                eye,
                                ParticleTex::Damage,
                                [1.0, 1.0, 1.0],
                                0.16,
                                8,
                                [0.3, 0.3, 0.3],
                                0.25,
                                2.0,
                                None,
                            );
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
                                self.tracks
                                    .insert(snap.id, EntityTrack::new(snap.clone(), now));
                            }
                        }
                    }
                    // Items somebody just picked up stay for three more ticks
                    // so they can fly into the collector, like vanilla.
                    self.tracks.retain(|id, track| {
                        seen.contains(id)
                            || track
                                .pickup
                                .is_some_and(|(t, _)| now.duration_since(t) < PICKUP_ANIM)
                    });
                }
                GameEvent::Hotbar {
                    slots,
                    offhand,
                    selected,
                } => {
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
                GameEvent::EffectUpdate {
                    name,
                    amplifier,
                    duration_ticks,
                } => {
                    let expiry = (duration_ticks >= 0).then(|| {
                        Instant::now() + Duration::from_secs_f32(duration_ticks as f32 / 20.0)
                    });
                    self.active_effects.insert(name, (amplifier, expiry));
                }
                GameEvent::EffectRemove { name } => {
                    self.active_effects.remove(&name);
                }
                GameEvent::Cooldown {
                    name,
                    duration_ticks,
                } => {
                    if duration_ticks == 0 {
                        self.cooldowns.remove(&name);
                    } else {
                        let secs = duration_ticks as f32 / 20.0;
                        self.cooldowns
                            .insert(name, (Instant::now() + Duration::from_secs_f32(secs), secs));
                    }
                }
                GameEvent::Sound {
                    name,
                    category,
                    pos,
                    volume,
                    pitch,
                    seed,
                } => {
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
                            audio.set_category(Some(category));
                            audio.play_positional(&name, gain, volume, pitch, distance, seed);
                            audio.set_category(None);
                        }
                        if self.settings.subtitles
                            && let Some(text) = self.lang.get(&format!("subtitles.{name}"))
                        {
                            let text = self.hud.push_subtitle(text.to_string());
                            self.narrator.speak(self.settings.narrator, crate::narrator::Category::Sound, &text);
                        }
                    }
                }
                GameEvent::TabList(mut players) => {
                    // The private skin also owns our local tab-list head. The
                    // bridge/server data remains untouched and nobody else is
                    // told about this synthetic URL.
                    if let (Some(name), Some((url, slim))) = (&self.own_name, &self.local_skin)
                        && let Some(own) = players.iter_mut().find(|p| &p.name == name)
                    {
                        own.skin_url = Some(url.clone());
                        own.skin_slim = *slim;
                    }
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
                GameEvent::TabSuggestions {
                    id,
                    start,
                    length,
                    entries,
                } => {
                    self.hud.chat.on_suggestions(id, start, length, entries);
                }
                GameEvent::ContainerOpened {
                    id,
                    kind,
                    title,
                    slots,
                } => {
                    self.hud.container_opened(id, kind, title, slots);
                }
                GameEvent::ContainerContent { id, slots, carried } => {
                    self.hud.container_content(id, slots, carried);
                }
                GameEvent::ContainerClosed { id } => {
                    if self.mount.as_ref().is_some_and(|m| m.container_id == id) {
                        self.mount = None;
                    }
                    // Stale furnace/brewing/enchantment properties must never
                    // bleed into the next screen with the same window id.
                    if self.container_data_id == id {
                        self.container_data.clear();
                        self.container_data_id = -1;
                    }
                    self.hud.container_closed(id);
                }
                GameEvent::MerchantOffers {
                    container_id,
                    offers,
                    villager_level,
                    villager_xp,
                    show_progress,
                } => {
                    self.hud.merchant_offers(
                        container_id,
                        offers,
                        villager_level,
                        villager_xp,
                        show_progress,
                    );
                }
                GameEvent::MountScreen {
                    container_id,
                    columns,
                    entity_id,
                } => {
                    let _ = (columns, entity_id); // the slot count carries both
                    // Vanilla titles this screen with the animal's own name.
                    let kind = self
                        .player
                        .as_ref()
                        .and_then(|p| p.vehicle_kind.clone())
                        .unwrap_or_else(|| "horse".into());
                    let title = self
                        .lang
                        .get(&format!("entity.minecraft.{kind}"))
                        .unwrap_or("Horse")
                        .to_string();
                    self.mount = Some(MountScreen { container_id, kind });
                    self.hud.container_opened(
                        container_id,
                        "horse".into(),
                        vec![ChatSpan::plain(title)],
                        Vec::new(),
                    );
                }
                GameEvent::EntityHurt { id, yaw: _ } => {
                    if let Some(track) = self.tracks.get_mut(&id) {
                        // Vanilla hurtTime is 10 ticks = 500 ms.
                        track.hurt_until = Some(Instant::now() + Duration::from_millis(500));
                    }
                }
                GameEvent::OwnHurt { yaw } => {
                    // Vanilla rolls the view towards whatever hit you for the
                    // length of the hurt animation. The red vignette is driven
                    // by the health drop (below) so a hit that costs no health
                    // — a shielded blow — still flinches without flashing.
                    if self.settings.damage_tilt {
                        self.hurt_at = Some(Instant::now());
                        self.hurt_from_yaw = yaw;
                    }
                }
                GameEvent::EntityDeath { id } => {
                    if let Some(track) = self.tracks.get_mut(&id) {
                        track.death_start.get_or_insert_with(Instant::now);
                    }
                }
                GameEvent::BlockEntities(entries) => {
                    self.block_entities.insert_all(entries);
                }
                GameEvent::BlockAction {
                    pos,
                    block,
                    action,
                    param,
                } => {
                    // Bells: action 1 is "rung", with the struck face in the
                    // parameter.
                    if block == "bell" && action == 1 {
                        self.block_entities.ring_bell(pos, param);
                    }
                    // Pistons: "the one at P fired, facing D". Everything else
                    // about the stroke — which blocks travel, which break — the
                    // client works out for itself.
                    if block == "piston" || block == "sticky_piston" {
                        self.start_piston(pos, action, param);
                    }
                    // Note blocks: struck. The tune is in the block state.
                    if block == "note_block" {
                        self.play_note_block(pos);
                    }
                    // Containers: action 1 carries how many players have it
                    // open. That is all the server ever says about a lid — how
                    // far it has actually swung is ours to work out.
                    if action == 1 && opens_a_lid(&block) {
                        self.lids.set_viewers(pos, param);
                        // A double chest is two block entities, and the server
                        // does not reliably speak for both: move the other half
                        // ourselves so the two lids never part company.
                        if let Some(DynBlock::Chest {
                            partner: Some(d), ..
                        }) = self.store.dyn_block(self.mirror.get_block(pos))
                        {
                            let other = BlockPos {
                                x: pos.x + d[0],
                                y: pos.y + d[1],
                                z: pos.z + d[2],
                            };
                            self.lids.set_viewers(other, param);
                        }
                    }
                }
                GameEvent::Lightning { pos } => {
                    // Vanilla's bolt lives for 10 ticks (half a second).
                    let seed = fnv64(&pos[0].to_bits().to_le_bytes())
                        ^ fnv64(&pos[2].to_bits().to_le_bytes());
                    self.lightning.push((pos, seed, Instant::now()));
                }
                GameEvent::BossBar(update) => {
                    use crate::bridge::events::BossBarUpdate as U;
                    match update {
                        // Server order matters: vanilla stacks the bars in the
                        // order it was told about them.
                        U::Set { id, bar } => {
                            match self.boss_bars.iter_mut().find(|(k, _)| *k == id) {
                                Some(slot) => slot.1 = bar,
                                None => self.boss_bars.push((id, bar)),
                            }
                        }
                        U::Remove { id } => self.boss_bars.retain(|(k, _)| *k != id),
                        U::Progress { id, progress } => {
                            if let Some((_, b)) = self.boss_bars.iter_mut().find(|(k, _)| *k == id)
                            {
                                b.progress = progress;
                            }
                        }
                        U::Name { id, name } => {
                            if let Some((_, b)) = self.boss_bars.iter_mut().find(|(k, _)| *k == id)
                            {
                                b.name = name;
                            }
                        }
                        U::Style { id, color, overlay } => {
                            if let Some((_, b)) = self.boss_bars.iter_mut().find(|(k, _)| *k == id)
                            {
                                b.color = color;
                                b.overlay = overlay;
                            }
                        }
                    }
                }
                GameEvent::Waypoint(update) => {
                    use crate::bridge::events::WaypointUpdate as U;
                    match update {
                        U::Set { id, waypoint } => {
                            self.waypoints.insert(id, waypoint);
                        }
                        U::Remove { id } => {
                            self.waypoints.remove(&id);
                        }
                    }
                }
                GameEvent::EntitySound {
                    id,
                    name,
                    category,
                    volume,
                    pitch,
                    seed,
                } => {
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
                                    let (dx, dy, dz) = (p[0] - e[0], p[1] - e[1], p[2] - e[2]);
                                    ((dx * dx + dy * dy + dz * dz) as f32).sqrt()
                                }
                                None => 0.0,
                            };
                            audio.set_category(Some(category));
                            audio.play_positional(&name, gain, volume, pitch, distance, seed);
                            audio.set_category(None);
                        }
                        if self.settings.subtitles
                            && let Some(text) = self.lang.get(&format!("subtitles.{name}"))
                        {
                            let text = self.hud.push_subtitle(text.to_string());
                            self.narrator.speak(self.settings.narrator, crate::narrator::Category::Sound, &text);
                        }
                    }
                }
                GameEvent::EntityCrit { id, magic } => {
                    // Vanilla scatters the sparks over the whole body of
                    // whoever was hit, not at one point.
                    if let Some(t) = self.tracks.get(&id) {
                        let (pos, h, w) =
                            (t.snap.pos, t.snap.height.max(0.5), t.snap.width.max(0.4));
                        let tex = if magic {
                            ParticleTex::EnchantedHit
                        } else {
                            ParticleTex::Crit
                        };
                        self.spawn_particles(
                            [pos[0], pos[1] + h as f64 * 0.5, pos[2]],
                            tex,
                            [1.0, 1.0, 1.0],
                            0.10,
                            if magic { 16 } else { 10 },
                            [w * 0.5, h * 0.4, w * 0.5],
                            0.6,
                            1.5,
                            None,
                        );
                    }
                }
                GameEvent::EntityStatus { id, status } => {
                    // 4 on a set of evoker fangs is the real "start biting"
                    // trigger — vanilla renders nothing until it fires, then
                    // bites shut and bursts upward over the following second.
                    if status == 4
                        && let Some(t) = self.tracks.get_mut(&id)
                        && t.snap.kind == "evoker_fangs"
                    {
                        t.bite_start.get_or_insert(Instant::now());
                    }
                    // 17 on a rocket is the firework going off. Vanilla reads
                    // the stars out of the rocket's own item and paints each
                    // one; only a rocket with no star in it falls through to
                    // the plain puff below.
                    let stars = if status == 17 {
                        self.tracks
                            .get(&id)
                            .map(|t| t.snap.firework.clone())
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    if !stars.is_empty() {
                        let at = self.tracks.get(&id).map(|t| t.snap.pos).unwrap_or_default();
                        self.explode_firework(&stars, at);
                        continue;
                    }
                    // Vanilla's small moments: taming smoke, breeding hearts,
                    // a shield taking a hit, a totem going off.
                    if let Some(fx) = entitystatus::status_fx(status) {
                        let (pos, height) = match self.tracks.get(&id) {
                            Some(t) => (t.snap.pos, t.snap.height.max(0.5)),
                            // The local player is never tracked; its own status
                            // (a totem, a shield) belongs at the camera.
                            None => (self.player.as_ref().map(|p| p.pos).unwrap_or_default(), 1.8),
                        };
                        let at = [
                            pos[0],
                            pos[1]
                                + if fx.above {
                                    height as f64 + 0.4
                                } else {
                                    height as f64 * 0.5
                                },
                            pos[2],
                        ];
                        if let Some((tex, color, count, spread)) = fx.particles {
                            self.spawn_particles(
                                at,
                                tex,
                                color,
                                0.12,
                                count,
                                [spread, spread * 0.6, spread],
                                0.35,
                                if fx.above { -0.4 } else { 1.0 },
                                None,
                            );
                        }
                        if let Some(name) = fx.sound {
                            self.play_world_sound(name, at, 1.0, 1.0);
                        }
                        if fx.totem {
                            self.totem_flash = Some(Instant::now());
                        }
                    }
                }
                GameEvent::StopSound { name, category } => {
                    if let Some(audio) = &self.audio {
                        audio.stop(name.as_deref(), category);
                    }
                }
                GameEvent::Title(part) => {
                    use crate::bridge::events::TitlePart;
                    match part {
                        TitlePart::Title(spans) => self.hud.set_title(spans),
                        TitlePart::Subtitle(spans) => self.hud.set_subtitle(spans),
                        TitlePart::ActionBar(spans) => self.hud.set_action_bar(spans),
                        TitlePart::Times {
                            fade_in,
                            stay,
                            fade_out,
                        } => self.hud.set_title_times(fade_in, stay, fade_out),
                        TitlePart::Clear { reset } => self.hud.clear_titles(reset),
                    }
                }
                GameEvent::LookAt { yaw, pitch } => {
                    // The server turned us: the camera follows immediately, and
                    // the server is told where we ended up looking.
                    self.yaw = yaw;
                    self.pitch = pitch.clamp(-90.0, 90.0);
                    self.send_cmd(Command::SetDirection {
                        yaw: self.yaw,
                        pitch: self.pitch,
                    });
                }
                GameEvent::EntitySwing { id } => {
                    if let Some(track) = self.tracks.get_mut(&id) {
                        track.swing_start = Some(Instant::now());
                    }
                }
                GameEvent::Particles {
                    pos,
                    tex,
                    color,
                    size,
                    count,
                    spread,
                    speed,
                    gravity,
                    item,
                } => {
                    self.spawn_particles(
                        pos,
                        tex,
                        color,
                        size,
                        count,
                        spread,
                        speed,
                        gravity,
                        item.as_deref(),
                    );
                }
                GameEvent::ResourcePackPrompt { id, required, prompt } => {
                    self.hud.queue_resource_pack_prompt(id, required, prompt);
                    self.resource_pack_status = None;
                    // User decisions have no network timeout in vanilla. The
                    // ordinary 45-second login watchdog resumes after Proceed.
                    self.connect_deadline = None;
                }
                GameEvent::ResourcePackProgress { downloaded, total, .. } => {
                    if !self.connected {
                        self.connect_deadline = Some(Instant::now() + Duration::from_secs(60));
                    }
                    self.resource_pack_status = Some(match total.filter(|total| *total > 0) {
                        Some(total) => format!(
                            "Downloading server resource pack... {:.0}% ({:.1}/{:.1} MiB)",
                            downloaded as f64 * 100.0 / total as f64,
                            downloaded as f64 / 1_048_576.0,
                            total as f64 / 1_048_576.0,
                        ),
                        None => format!(
                            "Downloading server resource pack... {:.1} MiB",
                            downloaded as f64 / 1_048_576.0,
                        ),
                    });
                }
                GameEvent::ResourcePackReady { id, path } => {
                    self.resource_pack_status = Some("Applying server resource pack...".into());
                    let loaded = self.apply_server_resource_pack(id, path);
                    self.send_cmd(Command::ResourcePackApplied { id, loaded });
                }
                GameEvent::ResourcePackPop { id } => {
                    self.hud.cancel_resource_pack_prompt(id);
                    self.pop_server_resource_pack(id);
                }
                GameEvent::ResourcePackFailed { reason, .. } => {
                    self.resource_pack_status = None;
                    warn!(reason, "app: server resource pack failed");
                    self.hud.push_chat(
                        vec![ChatSpan::plain(format!("Server resource pack failed: {reason}"))],
                        true,
                    );
                }
                GameEvent::MapData(update) => {
                    self.maps.apply(&update);
                }
                GameEvent::ContainerData {
                    id,
                    property,
                    value,
                } => {
                    if self.container_data_id != id {
                        self.container_data.clear();
                        self.container_data_id = id;
                    }
                    self.container_data.insert(property, value);
                }
                GameEvent::SelectAdvancementsTab(tab) => {
                    self.hud.select_advancements_tab(tab);
                }
                GameEvent::RecipeBookSettings { crafting, furnace, blast_furnace, smoker } => {
                    self.hud.recipe_book_settings(crafting, furnace, blast_furnace, smoker);
                }
                GameEvent::ServerData { motd, icon_bytes } => {
                    if let Some((address, _, _)) = self.connect_target.clone() {
                        self.hud.apply_server_data(&address, motd, icon_bytes);
                    }
                }
                GameEvent::Advancements(update) => {
                    let completed = self.hud.advancements.apply(&update);
                    for id in completed {
                        let Some(display) = self
                            .hud
                            .advancements
                            .nodes
                            .get(&id)
                            .and_then(|n| n.display.as_ref())
                        else {
                            continue;
                        };
                        if !display.show_toast {
                            continue;
                        }
                        let key = match display.frame {
                            1 => "advancements.toast.challenge",
                            2 => "advancements.toast.goal",
                            _ => "advancements.toast.task",
                        };
                        let title = self
                            .lang
                            .get(key)
                            .unwrap_or("Advancement Made!")
                            .to_string();
                        self.hud.toasts.push(toasts::Toast::advancement(
                            display.frame,
                            display.title.clone(),
                            display.icon.clone(),
                            &title,
                        ));
                    }
                }
                GameEvent::Statistics(entries) => {
                    self.hud.statistics.apply(&entries);
                }
                GameEvent::BlockDestruction { id, pos, stage } => match stage {
                    Some(stage) => {
                        self.block_destruction.insert(id, (pos, stage.min(9)));
                    }
                    None => {
                        self.block_destruction.remove(&id);
                    }
                },
                GameEvent::Explosion { pos, radius, sound } => {
                    self.on_explosion(pos, radius, &sound);
                }
                GameEvent::ItemPickedUp { item, collector } => {
                    let pos = self.tracks.get(&item).map(|t| t.snap.pos);
                    if let Some(track) = self.tracks.get_mut(&item) {
                        track.pickup = Some((Instant::now(), collector));
                    }
                    // Vanilla plays the pickup blip client-side; the server
                    // never sends one.
                    if let Some(pos) = pos {
                        let pitch = 1.4 + self.rand01() * 0.4;
                        self.play_world_sound("entity.item.pickup", pos, 0.2, pitch);
                    }
                }
                GameEvent::Died { message } => {
                    self.hud.show_death_screen(message);
                    self.set_grab(false);
                }
                GameEvent::OpenSignEditor { pos, front } => {
                    self.hud.open_sign_editor(pos, front);
                    self.set_grab(false);
                }
                GameEvent::OpenBook { off_hand } => {
                    let held = if off_hand {
                        self.offhand.clone()
                    } else {
                        self.hotbar
                            .get(self.selected_slot as usize)
                            .cloned()
                            .flatten()
                    };
                    if let Some(item) = held {
                        self.hud.open_book(&item);
                        self.set_grab(false);
                    }
                }
                GameEvent::WorldBorder(border) => {
                    self.border = border;
                    self.border_since = Instant::now();
                }
                GameEvent::SpawnPosition(pos) => {
                    self.spawn_pos = Some(pos);
                }
                GameEvent::Camera { id } => {
                    self.camera_entity = id;
                }
                GameEvent::Enchantments(list) => {
                    self.enchantments = list;
                }
                GameEvent::TrimRegistries {
                    patterns,
                    materials,
                } => {
                    self.trim_patterns = patterns;
                    self.trim_materials = materials;
                }
                GameEvent::Instruments(list) => {
                    self.instruments = list;
                }
                GameEvent::StonecutterRecipes(list) => {
                    self.stonecutter = list;
                }
                GameEvent::RecipeBook { entries, replace } => {
                    self.hud.recipes.add(entries, replace);
                }
                GameEvent::RecipesForgotten(ids) => {
                    self.hud.recipes.remove(&ids);
                }
                GameEvent::GhostRecipe {
                    container_id,
                    recipe,
                } => {
                    self.hud.set_ghost_recipe(container_id, recipe);
                }
                GameEvent::ReducedDebugInfo(v) => {
                    self.server_reduced_debug_info = v;
                }
                GameEvent::RecipesUnlocked { count } => {
                    let title = self
                        .lang
                        .get("recipe.toast.title")
                        .unwrap_or("New Recipes Unlocked!")
                        .to_string();
                    let body = self
                        .lang
                        .get("recipe.toast.description")
                        .unwrap_or("Check your recipe book")
                        .to_string();
                    let _ = count;
                    self.hud.toasts.push(toasts::Toast::recipe(title, body));
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
        self.own_footsteps(p);
        // Getting into bed starts vanilla's sleep counter; leaving it stops it.
        match p.sleeping_at {
            Some(_) => {
                self.sleep_since.get_or_insert(now);
            }
            None => self.sleep_since = None,
        }
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
                    c.vel = [
                        p.velocity[0] * 20.0,
                        p.velocity[1] * 20.0,
                        p.velocity[2] * 20.0,
                    ];
                }
                c.snap_pos = p.pos;
                c.snap_t = now;
                c.cape.tick(p.pos, now);
            }
            None => {
                self.cam = Some(CamTrack {
                    snap_pos: p.pos,
                    snap_t: now,
                    vel: [0.0; 3],
                    render_pos: p.pos,
                    cape: CapeLag::at(p.pos, now),
                });
            }
        }
    }

    /// Per-frame camera smoothing: chase the velocity-extrapolated position.
    /// Advance every animated block sprite and push the frames that changed
    /// into the atlas texture. The clock is the vanilla 20 ticks/second, so a
    /// `frametime: 2` sprite (water, lava, fire) runs at 10 fps no matter what
    /// the frame rate is, and nothing uploads on frames where nothing moved.
    fn tick_atlas_animations(&mut self) {
        if self.atlas_anim.is_empty() {
            return;
        }
        let Some(r) = self.renderer.as_ref() else {
            return;
        };
        let tick = (self.start.elapsed().as_secs_f64() * 20.0) as u64;
        self.atlas_anim
            .tick(tick, |u| r.update_atlas_rect(u.x, u.y, u.w, u.h, u.rgba));
    }

    /// Refreshes the compass needle / clock hand frames shown everywhere an
    /// item icon is drawn (hotbar, hand, inventories, chests, …) — see
    /// `dial.rs` for the math and `ItemIcons::resolve` for how it's applied.
    fn tick_dial_items(&mut self) {
        let clock = dial::clock_frame(self.world_time);
        let compass = match &self.player {
            Some(p) => {
                // A lodestone compass in the main hand overrides the plain
                // compass's world-spawn target with wherever it's linked —
                // and, like vanilla, gives up and spins if that's in a
                // different dimension than the one we're standing in.
                let held = self.hotbar.get(self.selected_slot as usize).and_then(|s| s.as_ref());
                let (target, no_signal) = match held.and_then(|i| i.lodestone.as_ref()) {
                    Some((pos, dim)) => (Some(*pos), !dim.ends_with(&self.dim_name)),
                    None => (self.spawn_pos, self.dim_name.contains("nether")),
                };
                dial::compass_frame(
                    [p.pos[0], p.pos[2]],
                    self.yaw,
                    target,
                    no_signal,
                    self.start.elapsed().as_secs_f32(),
                )
            }
            None => 0,
        };
        self.item_icons.set_dial_frames(compass, clock);
    }

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
        // Composite any new armour trims first: it needs `&mut self.pack`, so
        // it cannot happen while the renderer is borrowed below.
        self.ensure_trims();
        // Our own skin URL (bridge never sends our own entity, so it isn't in
        // `tracks`) — resolved before borrowing the renderer to keep borrows
        // disjoint. Drives third-person (F5) and the inventory paper-doll.
        let own_url = self.own_skin_url().map(|(u, _)| u);
        let own_cape = self.own_cape_url();
        let Some(renderer) = &mut self.renderer else {
            return;
        };
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
            let Some(uuid) = &track.snap.uuid else {
                continue;
            };
            let Some((url, _)) = self.skin_by_uuid.get(uuid) else {
                continue;
            };
            upload(&mut self.skins, url);
        }
        if let Some(url) = &own_url {
            upload(&mut self.skins, url);
        }
        // Capes come off the same texture server as skins, so they ride the
        // same download + cache + upload path.
        let cape_urls: Vec<String> = self
            .tracks
            .values()
            .filter_map(|t| t.snap.cape_url.clone())
            .collect();
        for url in &cape_urls {
            upload(&mut self.skins, url);
        }
        if let Some(url) = &own_cape {
            upload(&mut self.skins, url);
        }
        // Player heads wear their owner's skin, downloaded the same way.
        let head_urls: Vec<String> = self
            .block_entities
            .map
            .values()
            .filter_map(|d| match d {
                crate::bridge::events::BlockEntityData::Skull { texture_url, .. } => {
                    texture_url.clone()
                }
                _ => None,
            })
            .collect();
        for url in &head_urls {
            upload(&mut self.skins, url);
        }
        // Block-entity textures composited this frame (banner patterns, sign
        // text, pot sherds) — uploaded once, then cached by key.
        for (key, img) in self.block_entities.take_pending() {
            renderer.ensure_skin(key, &img);
        }
        // Armour trims composited above this frame: hand them to the GPU.
        for (key, img) in std::mem::take(&mut self.pending_trims) {
            renderer.ensure_skin(key, &img);
        }
        // Filled maps: re-composite and re-upload whichever ones the server
        // just sent patches for. A map's texture is not load-once — it grows
        // as you walk, so `replace_skin` overwrites the old one.
        for id in self.maps.dirty_ids() {
            let Some(img) = self.maps.compose(id) else {
                continue;
            };
            let key = fnv64(format!("map:{id}").as_bytes());
            renderer.replace_skin(key, &img);
            self.map_tex.insert(id, key);
            // The same picture again as an egui texture, for the cartography
            // table's preview panel.
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            );
            let handle = self.egui_ctx.load_texture(
                format!("map-{id}"),
                color,
                egui::TextureOptions::NEAREST,
            );
            self.map_egui.insert(id, handle);
            self.maps.mark_clean(id);
        }
    }

    /// Build any armour-trim texture that is on screen but not composited yet.
    ///
    /// Vanilla paints a trim by taking the pattern's greyscale sheet and
    /// swapping its palette for the trim material's — that is why a gold trim
    /// on iron armour and on diamond armour are the same shapes in the same
    /// colours. Same here: one palette swap per (pattern, material, layer).
    fn ensure_trims(&mut self) {
        // Everything visible right now, our own armour included.
        let mut wanted: Vec<(String, String, bool)> = Vec::new();
        let mut collect = |trims: &[Option<(String, String)>; 4]| {
            for (slot, trim) in trims.iter().enumerate() {
                if let Some((pattern, material)) = trim {
                    wanted.push((pattern.clone(), material.clone(), slot == 2));
                }
            }
        };
        if let Some(p) = &self.player {
            collect(&p.equipment.trims);
        }
        for track in self.tracks.values() {
            collect(&track.snap.equipment.trims);
        }
        for key in wanted {
            if self.trim_tex.contains_key(&key) {
                continue;
            }
            let (pattern, material, leggings) = key.clone();
            let built = build_trim(&mut self.pack, &pattern, &material, leggings);
            match built {
                Some(img) => {
                    let tex = fnv64(format!("trim:{pattern}:{material}:{leggings}").as_bytes());
                    self.pending_trims.push((tex, img));
                    self.trim_tex.insert(key, Some(tex));
                }
                None => {
                    // Remember the miss so a pack without this pattern doesn't
                    // make us re-read the jar every frame.
                    self.trim_tex.insert(key, None);
                }
            }
        }
    }

    /// Our own cape url, if the account has one.
    /// Our own cape url, if the account has one. Our own entity is never sent
    /// to us, so it comes from whichever track shares our uuid — in third
    /// person that is the only place it could come from anyway.
    fn own_cape_url(&self) -> Option<String> {
        if let Some(url) = &self.local_cape {
            return Some(url.clone());
        }
        let name = self.own_name.as_ref()?;
        let tp = self.hud.tab.players.iter().find(|p| &p.name == name)?;
        self.tracks
            .values()
            .find(|t| t.snap.uuid.as_deref() == Some(tp.uuid.as_str()))
            .and_then(|t| t.snap.cape_url.clone())
    }

    /// Our own cape's renderer key, or 0 if we have none (or it is not on the
    /// GPU yet).
    fn own_cape(&self) -> u64 {
        let Some(url) = self.own_cape_url() else {
            return 0;
        };
        let key = fnv64(key_of_url(&url).as_bytes());
        if self.renderer.as_ref().is_some_and(|r| r.has_skin(key)) {
            key
        } else {
            0
        }
    }

    /// Our own skin `(url, slim)`, looked up in the tab list by our username.
    /// `None` until the tab list arrives (or in offline mode with no skin).
    fn own_skin_url(&self) -> Option<(String, bool)> {
        if let Some(local) = &self.local_skin {
            return Some(local.clone());
        }
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
            let cc = ChunkPos {
                x: (p.pos[0].floor() as i32) >> 4,
                z: (p.pos[2].floor() as i32) >> 4,
            };
            for pos in self
                .mirror
                .unload_far(cc, self.settings.render_distance + 2)
            {
                self.open_containers.remove(&pos);
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
                let smooth = self.settings.smooth_lighting;
                self.in_flight += 1;
                rayon::spawn(move || {
                    let mesh = mesh_section(&snap, &store, &table, &biome_tints, smooth);
                    let _ = tx.send((pos, mesh));
                });
            }
        }

        // Upload finished meshes (bounded per frame to keep frame time stable).
        for _ in 0..MESH_BUDGET_PER_FRAME {
            let Ok((_pos, mut mesh)) = self.mesh_rx.try_recv() else {
                break;
            };
            self.in_flight = self.in_flight.saturating_sub(1);
            // The containers this section holds come along with its mesh, so
            // they follow block changes and chunk loads for free.
            let containers = std::mem::take(&mut mesh.dyn_be);
            if containers.is_empty() {
                self.open_containers.remove(&mesh.pos);
            } else {
                self.open_containers.insert(mesh.pos, containers);
            }
            if let Some(r) = &mut self.renderer {
                r.upload_mesh(mesh);
            }
        }
        for pos in self.mirror.take_removed() {
            self.open_containers.remove(&pos);
            if let Some(r) = &mut self.renderer {
                r.remove_mesh(pos);
            }
        }
    }

    fn apply_mouse_look(&mut self, frame_dt: f64) {
        let (mut dx, mut dy) = std::mem::take(&mut self.pending_mouse);
        // Right stick, folded into the same delta mouse motion arrives as —
        // frame-rate independent via `frame_dt`, since unlike a mouse a
        // stick reports a held *position*, not a per-frame movement.
        if self.settings.gamepad.enabled && self.grabbed {
            let (lx, ly) = self.gamepad.look_delta();
            if lx != 0.0 || ly != 0.0 {
                const DEG_PER_SEC: f64 = 120.0;
                let rate = self.settings.gamepad.look_sensitivity as f64 * DEG_PER_SEC * frame_dt;
                dx += lx as f64 * rate;
                // Pushing the stick up looks up (pitch decreases), matching
                // moving the mouse up with `invert_y` off — same convention
                // `invert_mouse` already uses for the real mouse below.
                let ly = if self.settings.gamepad.invert_y { ly } else { -ly };
                dy += ly as f64 * rate;
            }
        }
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
                self.send_cmd(Command::SetDirection {
                    yaw: self.yaw,
                    pitch: self.pitch,
                });
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
            tags.push(NameTag {
                ndc,
                dist,
                scale,
                spans,
            });
        }
        // Far tags first so nearer ones paint on top.
        tags.sort_by(|a, b| b.dist.total_cmp(&a.dist));
        tags
    }

    fn entity_draws(&mut self, cam_pos: [f64; 3]) -> Vec<EntityDraw> {
        let now = Instant::now();
        let render_t = now.checked_sub(ENTITY_LERP_DELAY).unwrap_or(now);
        let renderer = self.renderer.as_ref();
        // Dropped items spin around Y like vanilla.
        let spin = (self.start.elapsed().as_secs_f32() * 60.0) % 360.0;
        // The clock the self-animating mob parts run on. Offset per entity
        // below so a pen full of bees doesn't beat in lockstep.
        let clock = self.start.elapsed().as_secs_f32();
        let mut out = Vec::with_capacity(self.tracks.len());
        // (position, radius) per shadow-casting entity. The ground under each is
        // looked up after the loop, which needs `&self` while `tracks` is
        // borrowed mutably here.
        let mut shadows: Vec<([f64; 3], f32)> = Vec::new();
        // Ropes to resolve after the loop, for the same reason: the other end is
        // another entity. (rope start, other entity id, fishing line?).
        let mut ropes: Vec<([f64; 3], u64, bool)> = Vec::new();
        // Same reason the shadows are deferred: `tracks` is borrowed mutably
        // below, so the world light lookup takes the mirror directly.
        let (mirror, connected) = (&self.mirror, self.connected);
        // Footsteps of everything in sight are worked out inside the loop and
        // played after it — `tracks` is held mutably here.
        let table = &self.table;
        let mut step_sounds: Vec<(footsteps::StepEvent, [f64; 3], String)> = Vec::new();
        // Trim textures are composited before this loop (`ensure_trims`), so
        // the draw side is a plain lookup that needs no `&mut self`.
        let trim_lookup = &self.trim_tex;
        let light_at = |p: [f64; 3]| -> [f32; 2] {
            if !connected {
                return [1.0, 1.0];
            }
            let bp = BlockPos {
                x: p[0].floor() as i32,
                y: p[1].floor() as i32,
                z: p[2].floor() as i32,
            };
            let (sky, blk) = mirror.light_at(bp);
            [blk as f32 / 15.0, sky as f32 / 15.0]
        };
        // Riders sit on their vehicle, so their draw position comes from the
        // vehicle's — sampled up front because the loop below holds `tracks`
        // mutably. Skipped entirely when nobody is riding anything.
        let seats: HashMap<u64, ([f64; 3], f32, String, f32)> =
            if self.tracks.values().any(|t| t.snap.riding_on.is_some()) {
                self.tracks
                    .values()
                    .map(|t| {
                        let (p, y, _) = t.sample(render_t);
                        (t.snap.id, (p, y, t.snap.kind.clone(), t.snap.height))
                    })
                    .collect()
            } else {
                HashMap::new()
            };
        // Where every entity is right now, needed only when something was just
        // picked up (the item flies into whoever took it).
        let collectors: HashMap<u64, [f64; 3]> = if self.tracks.values().any(|t| t.pickup.is_some())
        {
            self.tracks
                .values()
                .map(|t| (t.snap.id, t.sample(render_t).0))
                .collect()
        } else {
            HashMap::new()
        };
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
            let (mut pos, yaw, pitch) = track.sample(render_t);

            // Just picked up: vanilla flies the item into the collector's chest
            // over three ticks instead of making it blink out of existence.
            if let Some((start, collector)) = track.pickup {
                let t = (now.duration_since(start).as_secs_f32() / PICKUP_ANIM.as_secs_f32())
                    .clamp(0.0, 1.0);
                if let Some(target) = collectors.get(&collector) {
                    // Aim at the middle of the body, not its feet.
                    let to = [target[0], target[1] + 0.8, target[2]];
                    for i in 0..3 {
                        pos[i] += (to[i] - pos[i]) * t as f64;
                    }
                }
            }

            // Riding: sit on the vehicle's seat instead of standing wherever the
            // server last placed us. Vanilla drives the rider's position from
            // the vehicle every tick, which is why an un-seated rider visibly
            // lags a moving boat.
            let mut sitting = false;
            if let Some((vehicle, seat)) = snap.riding_on
                && let Some((vpos, vyaw, vkind, vheight)) = seats.get(&vehicle)
            {
                let off = seat_offset(vkind, *vheight, seat);
                let (s, c) = (-vyaw.to_radians()).sin_cos();
                pos = [
                    vpos[0] + (off[0] * c + off[2] * s) as f64,
                    vpos[1] + off[1] as f64,
                    vpos[2] + (off[2] * c - off[0] * s) as f64,
                ];
                sitting = true;
            }

            // Death: vanilla tips the body onto its side over 20 ticks.
            let roll = track.death_progress(now) * 90.0;
            // Riding beats the metadata pose: a player in a boat is "standing"
            // as far as the server is concerned.
            let pose = if sitting {
                PlayerPose::Sitting
            } else {
                player_pose(snap.pose, self.start.elapsed().as_secs_f32())
            };
            // Vanilla splits head and body rotation; the head may lead by 50°.
            let head_yaw = snap
                .head_yaw
                .map(|h| lerp_angle(yaw, h, 1.0) - yaw)
                .unwrap_or(0.0);

            let radius = shadow_radius(&snap.kind, snap.width);
            if radius > 0.0 {
                shadows.push((pos, radius));
            }

            // Walk cycle from actual rendered movement (players + humanoid mobs).
            // The same measurement gives the vertical speed a slime squashes and
            // stretches with — vanilla works it out on the client too, since the
            // server never says anything about it.
            if let Some((lt, lp)) = track.last_render {
                let dt = now.duration_since(lt).as_secs_f32().max(1e-3);
                let dist = (((pos[0] - lp[0]).powi(2) + (pos[2] - lp[2]).powi(2)) as f32).sqrt();
                let target = (dist / dt / 3.5).clamp(0.0, 1.0);
                track.amp += (target - track.amp) * (dt * 8.0).min(1.0);
                track.phase = (track.phase + dist * 2.6) % std::f32::consts::TAU;
                let rise = ((pos[1] - lp[1]) as f32 / dt).clamp(-8.0, 8.0);
                track.squish += (rise * 0.045 - track.squish) * (dt * 9.0).min(1.0);
                // A faceplanted fox's leg-scramble phase: see `leg_motion_pos`'s
                // doc comment for why this uses real elapsed time rather than
                // vanilla's literal per-render-frame `+= 0.67`.
                if matches!(snap.pose_kind, crate::bridge::events::AnimalPose::Faceplanted) {
                    track.leg_motion_pos += dt * 40.2;
                }
                // Footsteps. The server never says whether an entity is on the
                // ground, so read the world: something solid underfoot and no
                // real vertical movement is standing on it.
                let below = BlockPos {
                    x: pos[0].floor() as i32,
                    y: (pos[1] - 0.2).floor() as i32,
                    z: pos[2].floor() as i32,
                };
                let on_ground =
                    !table.is_air(mirror.get_block(below)) && (pos[1] - lp[1]).abs() < 0.02;
                let feet = BlockPos {
                    y: (pos[1] + 0.1).floor() as i32,
                    ..below
                };
                let wet = table.fluid_kind(mirror.get_block(feet)) == Some("water");
                if let Some(ev) = track.steps.update(pos, lp, on_ground, wet) {
                    let kind = if snap.is_player {
                        "player".to_string()
                    } else {
                        snap.kind.clone()
                    };
                    step_sounds.push((ev, pos, kind));
                }
            }
            track.last_render = Some((now, pos));
            // Sprinting widens the limb swing, like vanilla's run animation.
            let swing_gain = if snap.sprinting { 1.35 } else { 1.0 };
            let swing = track.phase.sin() * track.amp * 0.8 * swing_gain;
            let squish = track.squish;
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
            let mut tint = if track.hurt_until.is_some_and(|t| now < t) {
                [1.0, 0.45, 0.45]
            } else {
                [1.0, 1.0, 1.0]
            };
            // A creeper with its fuse lit flashes white faster and faster, and
            // swells as it goes — vanilla's tell that you have about a second.
            let mut swell = 0.0f32;
            if snap.swelling {
                let start = *track.swell_start.get_or_insert(now);
                // Vanilla's fuse is 30 ticks; the flash speeds up across it.
                swell = (now.duration_since(start).as_secs_f32() / 1.5).clamp(0.0, 1.0);
                let flash = ((swell * swell * 24.0).sin() * 0.5 + 0.5) * swell;
                for c in &mut tint {
                    *c += flash * 1.6;
                }
            } else {
                track.swell_start = None;
            }
            // Vanilla lights an entity by the block its eyes are in, so a mob
            // standing in an unlit cave is as dark as the cave.
            let light = light_at([pos[0], pos[1] + snap.height as f64 * 0.5, pos[2]]);

            // --- on fire: an upright flame billboard over any burning entity ---
            if snap.on_fire
                && self.fire_frames > 0
                && renderer.is_some_and(|r| r.has_skin(self.fire_tex))
            {
                // ~12 fps flame animation, phased per entity so a pile doesn't
                // flicker in lockstep.
                let t = self.start.elapsed().as_secs_f32();
                let f = ((t * 12.0) as u32 + snap.id as u32) % self.fire_frames;
                let n = self.fire_frames as f32;
                let uv = [0.0, f as f32 / n, 1.0, (f + 1) as f32 / n];
                out.push(EntityDraw {
                    pos,
                    yaw,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Fire {
                        tex: self.fire_tex,
                        w: snap.width.max(0.4) * 1.4 + 0.1,
                        h: snap.height.max(0.5) + 0.3,
                        uv,
                    },
                });
            }

            // --- arrows and stingers left sticking in a body ------------------
            // Vanilla's ArrowLayer: one little arrow per hit that is still in
            // there, poking out of the body at a fixed angle. The angles are
            // derived from the entity id and the index, so they stay put
            // between frames instead of shimmering.
            let stuck = snap.arrows as u32 + snap.stingers as u32;
            if stuck > 0 && renderer.is_some_and(|r| r.has_skin(self.arrow_tex)) {
                let bw = snap.width.max(0.4);
                let bh = snap.height.max(0.5);
                for i in 0..stuck.min(8) {
                    // Two independent pseudo-random angles per shaft.
                    let h = fnv64(&[(snap.id as u32) as u8, (snap.id >> 8) as u8, i as u8]);
                    let a = (h % 360) as f32;
                    let up = ((h >> 9) % 100) as f32 / 100.0;
                    let r = bw * 0.42;
                    let (sa, ca) = a.to_radians().sin_cos();
                    out.push(EntityDraw {
                        pos: [
                            pos[0] + (r * ca) as f64,
                            pos[1] + (bh * (0.25 + 0.5 * up)) as f64,
                            pos[2] + (r * sa) as f64,
                        ],
                        yaw: 0.0,
                        tint,
                        light,
                        roll: 0.0,
                        // Pointing inwards and slightly down, like a hit that
                        // came from outside the body.
                        kind: EntityDrawKind::Projectile {
                            tex: self.arrow_tex,
                            yaw: a + 180.0,
                            pitch: -10.0 + 20.0 * up,
                        },
                    });
                }
            }

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
                let trims = trim_keys(&trim_lookup, &eq.trims);
                let main_hand = eq.main_hand.as_deref().and_then(|n| self.item_icons.uv(n));
                let off_hand = eq.off_hand.as_deref().and_then(|n| self.item_icons.uv(n));
                // Elytra wings replace the cape; both need their texture on
                // the GPU already, or we simply draw neither.
                let elytra = match eq.chest.as_deref() {
                    Some("elytra") => self.elytra_tex,
                    _ => 0,
                };
                let cape = snap
                    .cape_url
                    .as_deref()
                    .map(|u| fnv64(key_of_url(u).as_bytes()))
                    .unwrap_or(0);
                let cape = if renderer.is_some_and(|r| r.has_skin(cape)) {
                    cape
                } else {
                    0
                };
                let elytra = if renderer.is_some_and(|r| r.has_skin(elytra)) {
                    elytra
                } else {
                    0
                };
                let (cape_flap, cape_lean, cape_lean2) =
                    cape_flap_lean(&track.cape, now, yaw, track.phase);
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    light,
                    roll,
                    kind: EntityDrawKind::Player {
                        skin,
                        slim,
                        swing,
                        attack_swing,
                        pose,
                        skin_layers: 0xFF,
                        head_pitch: pitch,
                        head_yaw,
                        armor,
                        trims,
                        main_hand,
                        off_hand,
                        cape,
                        elytra,
                        cape_flap,
                        cape_lean,
                        cape_lean2,
                    },
                });
                // A tamed parrot rides its owner's shoulder, one per side.
                for (side, variant) in snap.shoulders.iter().enumerate() {
                    let Some(v) = *variant else { continue };
                    let tex = self
                        .mob_variant_tex
                        .get(&("parrot".to_string(), v))
                        .copied()
                        .or_else(|| self.mob_model.get("parrot").map(|(t, _)| *t));
                    let Some(tex) = tex.filter(|t| renderer.is_some_and(|r| r.has_skin(*t))) else {
                        continue;
                    };
                    // Vanilla perches it beside the neck, and lower when its
                    // owner is sneaking.
                    let x = if side == 0 { 0.35 } else { -0.35 };
                    let y = if pose == PlayerPose::Sneaking {
                        1.16
                    } else {
                        1.36
                    };
                    out.push(EntityDraw {
                        pos: rotate_offset(pos, [x, y, 0.0], yaw),
                        yaw,
                        tint,
                        light,
                        roll: 0.0,
                        kind: EntityDrawKind::Mob {
                            tex,
                            model: MobModel::Parrot,
                            swing: 0.0,
                            head_pitch: pitch,
                            head_yaw,
                            scale: 1.0,
                            anim: clock + (snap.id % 1000) as f32 * 0.017,
                            pose: MobPose::None,
                        },
                    });
                }
                continue;
            }

            // --- leads: a rope from this mob up to whatever holds it ----------
            if let Some(holder) = snap.leashed_to {
                // Vanilla attaches the lead near the mob's shoulders.
                ropes.push((
                    [pos[0], pos[1] + snap.height as f64 * 0.8, pos[2]],
                    holder,
                    false,
                ));
            }

            // --- fishing bobber: the float, plus the line back to the rod -----
            if snap.kind == "fishing_bobber" {
                if renderer.is_some_and(|r| r.has_skin(self.bobber_tex)) {
                    out.push(EntityDraw {
                        pos,
                        yaw,
                        tint,
                        light,
                        roll: 0.0,
                        kind: EntityDrawKind::Orb {
                            tex: self.bobber_tex,
                            size: 0.25,
                            color: [1.0, 1.0, 1.0],
                        },
                    });
                }
                // The spawn packet's object data is the owner's entity id.
                ropes.push((pos, snap.spawn_data.max(0) as u64, true));
                continue;
            }

            // --- falling blocks (gravel, sand, anvils): the real block cube ---
            // The state id rides in the spawn packet's object data; vanilla
            // draws the block un-spun, centred on the hitbox.
            if snap.kind == "falling_block" {
                if let Some(quads) =
                    block_geometry_centred(&self.store, snap.spawn_data.max(0) as StateId)
                {
                    out.push(EntityDraw {
                        pos,
                        yaw: 0.0,
                        tint,
                        light,
                        roll: 0.0,
                        kind: EntityDrawKind::StaticBlock {
                            quads,
                            y_off: 0.5,
                            scale: 1.0,
                            flash: 0.0,
                        },
                    });
                    continue;
                }
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
                // Vanilla stacks a bigger pile out of 2–5 copies of the model,
                // each nudged by a per-copy pseudo-random offset.
                let copies = render_amount(snap.item_count);
                // A dropped chest or shulker box has no baked block model any
                // more — it is one of the ones the client draws itself — so it
                // gets its real model here rather than a flat icon.
                let container_model = item
                    .and_then(|n| self.block_state_by_name.get(n))
                    .and_then(|&sid| self.store.dyn_block(sid))
                    .and_then(|d| match d {
                        DynBlock::Chest { tex, .. } => Some((tex.to_string(), MobModel::Chest)),
                        DynBlock::Shulker { tex, .. } => {
                            Some((tex.to_string(), MobModel::ShulkerBox))
                        }
                        DynBlock::Book { .. } => None,
                    })
                    .filter(|(tex, _)| {
                        let key = fnv64(tex.as_bytes());
                        renderer.is_some_and(|r| r.has_skin(key))
                    });
                if let Some((tex, model)) = container_model {
                    for c in 0..copies {
                        let (dx, dy, dz) = stack_offset(snap.id, c, true);
                        out.push(EntityDraw {
                            pos: [pos[0] + dx, pos[1] + dy - 0.1, pos[2] + dz],
                            yaw: spin,
                            tint,
                            light,
                            roll: 0.0,
                            kind: EntityDrawKind::Mob {
                                tex: fnv64(tex.as_bytes()),
                                model,
                                swing: 0.0,
                                head_pitch: 0.0,
                                head_yaw: 0.0,
                                scale: 0.30,
                                anim: 0.0,
                                pose: MobPose::None,
                            },
                        });
                    }
                } else if let Some(quads) = block_quads {
                    for c in 0..copies {
                        let (dx, dy, dz) = stack_offset(snap.id, c, true);
                        out.push(EntityDraw {
                            pos: [pos[0] + dx, pos[1] + dy, pos[2] + dz],
                            yaw: spin,
                            tint,
                            light,
                            roll: 0.0,
                            kind: EntityDrawKind::ItemBlock {
                                quads: quads.clone(),
                            },
                        });
                    }
                } else if let Some(uv) = item.and_then(|n| self.item_icons.uv(n)) {
                    for c in 0..copies {
                        let (dx, dy, dz) = stack_offset(snap.id, c, false);
                        out.push(EntityDraw {
                            pos: [pos[0] + dx, pos[1] + dy, pos[2] + dz],
                            yaw: spin,
                            tint,
                            light,
                            roll: 0.0,
                            kind: EntityDrawKind::Item { uv, scale: 1.0 },
                        });
                    }
                } else {
                    out.push(EntityDraw {
                        pos,
                        yaw: spin,
                        tint,
                        light,
                        roll: 0.0,
                        kind: EntityDrawKind::Box {
                            w: 0.25,
                            h: 0.25,
                            color: [0.85, 0.85, 0.85],
                        },
                    });
                }
                continue;
            }

            // --- paintings: a flat wall slab showing the real artwork ---------
            if snap.kind == "painting"
                && let Some(info) = &snap.painting
                && let Some(&art_tex) = self.painting_tex.get(&info.asset)
            {
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    light,
                    roll: 0.0,
                    kind: EntityDrawKind::Painting {
                        art_tex,
                        back_tex: self.painting_back_tex,
                        w: info.width.max(1) as f32,
                        h: info.height.max(1) as f32,
                        facing: info.facing,
                    },
                });
                continue;
            }

            // --- item frames: the frame + its held item, rotated on the wall --
            if (snap.kind == "item_frame" || snap.kind == "glow_item_frame")
                && let Some(info) = &snap.frame
            {
                let frame_tex = if info.glow {
                    self.glow_item_frame_tex
                } else {
                    self.item_frame_tex
                };
                // Block items render as a small 3D block; everything else as its
                // flat icon.
                let block_quads = info
                    .item
                    .as_deref()
                    .filter(|n| self.block_names.contains(*n))
                    .and_then(|n| block_geometry(&self.store, &self.block_state_by_name, n))
                    .unwrap_or_default();
                let item_uv = if block_quads.is_empty() {
                    info.item.as_deref().and_then(|n| self.item_icons.uv(n))
                } else {
                    None
                };
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    light,
                    roll: 0.0,
                    kind: EntityDrawKind::ItemFrame {
                        frame_tex,
                        back_tex: self.painting_back_tex,
                        facing: info.facing,
                        rot: info.rot,
                        item_uv,
                        block_quads,
                        map_tex: info.map_id.and_then(|id| self.map_tex.get(&id).copied()),
                    },
                });
                continue;
            }

            // --- arrows: their real texture on crossed planes, flight-oriented -
            if snap.kind == "arrow" || snap.kind == "spectral_arrow" {
                let tex = if snap.kind == "spectral_arrow" {
                    self.arrow_spectral_tex
                } else {
                    self.arrow_tex
                };
                if renderer.is_some_and(|r| r.has_skin(tex)) {
                    out.push(EntityDraw {
                        pos,
                        yaw,
                        tint,
                        light,
                        roll: 0.0,
                        kind: EntityDrawKind::Projectile {
                            tex,
                            yaw: snap.yaw,
                            pitch: snap.pitch,
                        },
                    });
                    continue;
                }
            }

            // --- evoker fangs: real geometry, but only from the moment the real
            //     "attack" event fires — vanilla itself draws nothing before that,
            //     then bites shut and bursts up out of the ground over ~1s -------
            if snap.kind == "evoker_fangs" {
                let Some(bite_start) = track.bite_start else {
                    continue;
                };
                // Vanilla's own timeline: a full second from the attack event to
                // fully bitten (`getAnimationProgress`'s 20-tick ramp).
                let progress = (now.duration_since(bite_start).as_secs_f32()).clamp(0.0, 1.0);
                // The last 10% shrinks the whole thing away before the fangs
                // despawn (vanilla's `preScale`).
                let pre_scale = if progress > 0.9 {
                    ((1.0 - progress) / 0.1).max(0.0)
                } else {
                    1.0
                };
                if pre_scale <= 0.0 {
                    continue;
                }
                // The jaws snap shut over the first half-second: a cubic
                // ease-out from wide open (1) to clamped shut (0).
                let bite_t = (progress * 2.0).min(1.0);
                let bite_amount = 1.0 - bite_t * bite_t * bite_t;
                // Vanilla's own root+base vertical math, in its raw pixel units
                // (16px/block), run back through its renderer's own -1.501-block
                // translate and Y-flip to land in this engine's Y-up, feet-at-0
                // world space.
                let raw_root_y = 24.0 - 20.0 * pre_scale;
                let raw_base_y = 24.0 - (progress + (progress * 2.7).sin()) * 7.2;
                let y_shift = 1.501 - (raw_root_y + raw_base_y) / 16.0;
                if let Some(&(tex, _)) = self.mob_model.get("evoker_fangs") {
                    out.push(EntityDraw {
                        pos: [pos[0], pos[1] + y_shift as f64, pos[2]],
                        // Vanilla orients these with its own `90 - yRot`, not the
                        // generic mob body-facing angle.
                        yaw: 90.0 - yaw,
                        tint,
                        light,
                        roll: 0.0,
                        kind: EntityDrawKind::Mob {
                            tex,
                            model: MobModel::EvokerFangs,
                            swing: bite_amount,
                            head_pitch: 0.0,
                            head_yaw: 0.0,
                            scale: pre_scale,
                            anim: 0.0,
                            pose: MobPose::None,
                        },
                    });
                }
                continue;
            }

            // --- shulker bullet: always lit, tumbling on all three axes as it
            //     homes in, with its own real "spark" geometry ------------------
            if snap.kind == "shulker_bullet"
                && let Some(&(tex, _)) = self.mob_model.get("shulker_bullet")
            {
                // Vanilla drives the tumble off `ageInTicks`; a per-entity phase
                // (rather than true spawn-synced ticks) keeps a swarm of bullets
                // from all tumbling in lockstep.
                let tc = (now.duration_since(track.spawned_at).as_secs_f32()
                    + snap.id as f32 * 0.7)
                    * 20.0;
                out.push(EntityDraw {
                    pos,
                    yaw: 0.0,
                    tint,
                    // Vanilla forces full brightness on these regardless of
                    // where they fly.
                    light: [1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::OrientedMob {
                        tex,
                        model: MobModel::ShulkerBullet,
                        yaw: (tc * 0.1).sin() * 180.0,
                        pitch: (tc * 0.1).cos() * 180.0,
                        roll: (tc * 0.15).sin() * 360.0,
                        y_off: 0.15,
                        scale: 0.5,
                    },
                });
                continue;
            }

            // --- llama spit: a real cluster of cubes nosed along its flight path
            if snap.kind == "llama_spit"
                && let Some(&(tex, _)) = self.mob_model.get("llama_spit")
            {
                out.push(EntityDraw {
                    pos,
                    yaw: 0.0,
                    tint,
                    light,
                    roll: 0.0,
                    kind: EntityDrawKind::OrientedMob {
                        tex,
                        model: MobModel::LlamaSpit,
                        yaw: yaw - 90.0,
                        pitch: 0.0,
                        roll: pitch,
                        y_off: 0.15,
                        scale: 1.0,
                    },
                });
                continue;
            }

            // --- ominous item spawner: the real item it's about to spawn,
            //     spinning and growing in over its first 2.5s ------------------
            if snap.kind == "ominous_item_spawner" {
                let age_ticks = now.duration_since(track.spawned_at).as_secs_f32() * 20.0;
                let spin = age_ticks * 40.0 % 360.0;
                let grow = (age_ticks / 50.0).clamp(0.0, 1.0);
                if let Some(uv) = snap.item.as_deref().and_then(|n| self.item_icons.uv(n)) {
                    out.push(EntityDraw {
                        pos,
                        yaw: spin,
                        tint,
                        light,
                        roll: 0.0,
                        kind: EntityDrawKind::Item { uv, scale: grow },
                    });
                }
                continue;
            }

            // --- primed TNT: the block cube at full size, flashing white -------
            if snap.kind == "tnt"
                && let Some(quads) = block_geometry(&self.store, &self.block_state_by_name, "tnt")
            {
                // A ~2 Hz white pulse reads as "primed" (server doesn't expose
                // the fuse here); vanilla flashes faster near detonation.
                let t = self.start.elapsed().as_secs_f32();
                let flash = (0.5 + 0.5 * (t * 12.0).sin()).powi(2) * 0.9;
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    light,
                    roll: 0.0,
                    kind: EntityDrawKind::StaticBlock {
                        quads,
                        y_off: 0.5,
                        scale: 1.0,
                        flash,
                    },
                });
                continue;
            }

            // --- thrown items (snowball, egg, pearl, potions, fireballs…): the
            //     real item icon as a spinning sprite, like a dropped item ------
            if let Some(item_name) = projectile_item(&snap.kind)
                && let Some(uv) = self.item_icons.uv(item_name)
            {
                out.push(EntityDraw {
                    pos,
                    yaw: spin,
                    tint,
                    light,
                    roll: 0.0,
                    kind: EntityDrawKind::Item { uv, scale: 1.0 },
                });
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
                let trims = trim_keys(&trim_lookup, &eq.trims);
                let main_hand = eq.main_hand.as_deref().and_then(|n| self.item_icons.uv(n));
                let off_hand = eq.off_hand.as_deref().and_then(|n| self.item_icons.uv(n));
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    light,
                    roll,
                    kind: EntityDrawKind::Player {
                        skin: key,
                        slim: false,
                        swing,
                        attack_swing,
                        pose,
                        skin_layers: 0xFF,
                        head_pitch: pitch,
                        head_yaw,
                        armor,
                        trims,
                        main_hand,
                        off_hand,
                        cape: 0,
                        elytra: 0,
                        cape_flap: 0.0,
                        cape_lean: 0.0,
                        cape_lean2: 0.0,
                    },
                });
                continue;
            }

            // --- experience orbs: a small glowing billboard, bobbing + pulsing
            //     green↔yellow like vanilla ---------------------------------------
            if snap.kind == "experience_orb" {
                let t = self.start.elapsed().as_secs_f32();
                // A ~1.5 Hz green↔yellow shimmer (vanilla cycles the orb colour).
                let k = 0.5 + 0.5 * (t * 3.1 + (snap.id as f32) * 0.7).sin();
                let color = [0.30 + 0.70 * k, 1.0, 0.30 * (1.0 - k)];
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    light,
                    roll: 0.0,
                    kind: EntityDrawKind::Orb {
                        tex: self.xp_orb_tex,
                        size: 0.45,
                        color,
                    },
                });
                continue;
            }

            // --- armor stands: the real model, posed per the server metadata ---
            if snap.kind == "armor_stand"
                && let Some(a) = &snap.armor_stand
            {
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    light,
                    roll: 0.0,
                    kind: EntityDrawKind::ArmorStandPosed {
                        tex: self
                            .mob_model
                            .get("armor_stand")
                            .map(|&(t, _)| t)
                            .unwrap_or(0),
                        scale: if a.small { 0.5 } else { 1.0 },
                        show_arms: a.show_arms,
                        show_base: a.show_base,
                        poses: [
                            a.head,
                            a.body,
                            a.right_arm,
                            a.left_arm,
                            a.right_leg,
                            a.left_leg,
                        ],
                    },
                });
                continue;
            }

            // --- display entities: block/item/text with a free transform -----
            //     text_display renders as a floating label (via name_spans, set
            //     by the bridge) — no body, so just skip the box fallback.
            if snap.kind == "text_display" {
                continue;
            }
            if let Some(d) = &snap.display {
                // block_display: the block's geometry (corner-origin) under the
                // vanilla display transform T·Lrot·S·Rrot about the entity pos.
                if let Some(sid) = d.block_state
                    && let Some(quads) = block_geometry_by_state(&self.store, sid as StateId)
                {
                    out.push(EntityDraw {
                        pos,
                        yaw,
                        tint,
                        light,
                        roll: 0.0,
                        kind: EntityDrawKind::DisplayBlock {
                            quads,
                            translation: d.translation,
                            scale: d.scale,
                            left_rot: d.left_rot,
                            right_rot: d.right_rot,
                        },
                    });
                    continue;
                }
                // item_display: the item icon on a flat quad, same transform.
                if let Some(item) = &d.item
                    && let Some(uv) = self.item_icons.uv(item)
                {
                    out.push(EntityDraw {
                        pos,
                        yaw,
                        tint,
                        light,
                        roll: 0.0,
                        kind: EntityDrawKind::DisplayItem {
                            uv,
                            translation: d.translation,
                            scale: d.scale,
                            left_rot: d.left_rot,
                            right_rot: d.right_rot,
                        },
                    });
                    continue;
                }
            }

            // --- typed minecarts: the cart model + its content block ----------
            if let Some(content) = minecart_content(&snap.kind)
                && let Some(&(cart_tex, model)) = self.mob_model.get(&snap.kind)
            {
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    light,
                    roll,
                    kind: EntityDrawKind::Mob {
                        tex: cart_tex,
                        model,
                        swing: 0.0,
                        head_pitch: 0.0,
                        head_yaw: 0.0,
                        scale: 1.0,
                        anim: 0.0,
                        pose: MobPose::None,
                    },
                });
                if let Some(quads) = block_geometry(&self.store, &self.block_state_by_name, content)
                {
                    out.push(EntityDraw {
                        pos,
                        yaw,
                        tint,
                        light,
                        roll,
                        kind: EntityDrawKind::StaticBlock {
                            quads,
                            y_off: 0.5,
                            scale: 0.68,
                            flash: 0.0,
                        },
                    });
                }
                continue;
            }

            // --- tropical fish: two-pass tinted body + pattern ----------------
            //     The packed variant encodes: shape (A/B), pattern index (0..5),
            //     body colour and pattern colour (both DyeColor ids). We draw the
            //     shape's grayscale body tinted by the body colour, then the
            //     pattern overlay tinted by the pattern colour on the same model.
            if snap.kind == "tropical_fish" {
                let v = snap.variant;
                let shape = (v & 0xFF).clamp(0, 1) as usize;
                let pattern = ((v >> 8) & 0xFF).clamp(0, 5) as usize;
                let body_col = (v >> 16) & 0xFF;
                let pat_col = (v >> 24) & 0xFF;
                let model = if shape == 0 {
                    MobModel::TropicalFishA
                } else {
                    MobModel::TropicalFishB
                };
                let base_tex = self.fish_base_tex[shape];
                if renderer.is_some_and(|r| r.has_skin(base_tex)) {
                    let mul = |c: [f32; 3]| [c[0] * tint[0], c[1] * tint[1], c[2] * tint[2]];
                    // Body layer, tinted by the body colour.
                    out.push(EntityDraw {
                        pos,
                        yaw,
                        light: [1.0, 1.0],
                        tint: mul(dye_rgb(body_col)),
                        roll,
                        kind: EntityDrawKind::Mob {
                            tex: base_tex,
                            model,
                            swing: 0.0,
                            head_pitch: pitch,
                            head_yaw,
                            scale: 1.0,
                            anim: 0.0,
                            pose: MobPose::None,
                        },
                    });
                    // Pattern overlay, tinted by the pattern colour, a hair larger
                    // so it sits just proud of the body (no z-fighting).
                    let pat_tex = self.fish_pat_tex[shape][pattern];
                    if renderer.is_some_and(|r| r.has_skin(pat_tex)) {
                        out.push(EntityDraw {
                            pos,
                            yaw,
                            light: [1.0, 1.0],
                            tint: mul(dye_rgb(pat_col)),
                            roll,
                            kind: EntityDrawKind::Mob {
                                tex: pat_tex,
                                model,
                                swing: 0.0,
                                head_pitch: pitch,
                                head_yaw,
                                scale: 1.006,
                                anim: 0.0,
                                pose: MobPose::None,
                            },
                        });
                    }
                    continue;
                }
            }

            // --- non-humanoid mobs with a real cuboid model + texture ---------
            if let Some(&(base_tex, model)) = self.mob_model.get(&snap.kind) {
                // Dance/roll/on-back progress, interpolated between the last
                // two ticks the same way `cape_flap_lean` interpolates
                // `CapeLag` — all three accumulators tick alongside the cape
                // in `EntityTrack::push`, so they share its tick-boundary
                // clock.
                let pose_frac = track.cape.frac(now);
                let is_spinning = track.dance_ticks % 55.0 < 15.0;
                let spin_progress = (track.spin_ticks_prev
                    + (track.spin_ticks - track.spin_ticks_prev) * pose_frac)
                    / 15.0;
                let roll_amount = track.roll_amount_prev
                    + (track.roll_amount - track.roll_amount_prev) * pose_frac;
                let on_back_amount = track.on_back_amount_prev
                    + (track.on_back_amount - track.on_back_amount_prev) * pose_frac;
                let pose = mob_pose(
                    snap.pose_kind,
                    is_spinning,
                    spin_progress,
                    roll_amount,
                    on_back_amount,
                );
                // Colour/type variants override the default texture: a
                // registry-resolved name first (cat/wolf/cow/chicken/pig/frog),
                // then an index variant (rabbit/parrot/…), else the default.
                let tex = snap
                    .variant_name
                    .as_ref()
                    .and_then(|n| {
                        self.mob_named_variant_tex
                            .get(&(snap.kind.clone(), n.clone()))
                            .copied()
                    })
                    .or_else(|| {
                        self.mob_variant_tex
                            .get(&(snap.kind.clone(), snap.variant))
                            .copied()
                    })
                    .unwrap_or(base_tex);
                // A ghast about to fire wears its red-eyed face.
                let tex = if snap.kind == "ghast"
                    && snap.charging
                    && renderer.is_some_and(|r| r.has_skin(self.ghast_shooting_tex))
                {
                    self.ghast_shooting_tex
                } else {
                    tex
                };
                // Slimes/magma cubes scale with their size; the cube model is
                // authored at the size-1 (0.5-block) scale. Salmon come in three
                // sizes (variant 0 small, 1 medium, 2 large).
                let base = if matches!(snap.kind.as_str(), "slime" | "magma_cube") {
                    (snap.height / 0.5).clamp(0.4, 5.0)
                } else if snap.kind == "salmon" {
                    match snap.variant {
                        0 => 0.6,
                        2 => 1.4,
                        _ => 1.0,
                    }
                } else {
                    1.0
                };
                // Babies render about half size (vanilla also enlarges the head;
                // a uniform shrink is a close approximation).
                let scale = if snap.baby { base * 0.55 } else { base };
                // A sneezing panda's head rears back on its own, replacing the
                // usual look-based pitch entirely (see `sneeze_head_pitch`'s
                // doc comment) — vanilla does the same full override.
                let head_pitch = snap.sneeze_head_pitch.unwrap_or(pitch);
                out.push(EntityDraw {
                    pos,
                    yaw,
                    tint,
                    light,
                    roll,
                    kind: EntityDrawKind::Mob {
                        tex,
                        model,
                        // A slime has no legs to swing: the channel carries how
                        // far it is stretched instead. A shulker has no legs
                        // either — the channel drives its lid instead, straight
                        // off the server's `Peek` metadata (0..100).
                        swing: if model == MobModel::Slime {
                            squish
                        } else if model == MobModel::Shulker {
                            snap.peek as f32 / 100.0
                        } else {
                            swing
                        },
                        head_pitch,
                        head_yaw,
                        // Vanilla puffs the creeper up as the fuse burns down.
                        scale: scale * (1.0 + swell * 0.10),
                        // A faceplanted fox's legs scramble off its own
                        // `leg_motion_pos` phase, not the generic per-entity
                        // clock (see that field's doc comment).
                        anim: if matches!(pose, MobPose::Faceplanted) {
                            track.leg_motion_pos
                        } else {
                            clock + (snap.id % 1000) as f32 * 0.017
                        },
                        pose,
                    },
                });
                // Sheep wool: vanilla draws the fleece as its own inflated layer
                // over the bare body, and drops it entirely once the sheep is
                // sheared. (The wool colour is a server-side data component in
                // 26.1, not entity metadata, so the fleece stays undyed.)
                if snap.kind == "sheep"
                    && !snap.sheared
                    && renderer.is_some_and(|r| r.has_skin(self.sheep_wool_tex))
                {
                    out.push(EntityDraw {
                        pos,
                        yaw,
                        tint,
                        light,
                        roll,
                        kind: EntityDrawKind::Mob {
                            tex: self.sheep_wool_tex,
                            model,
                            swing,
                            head_pitch: pitch,
                            head_yaw,
                            scale: scale * 1.12,
                            anim: 0.0,
                            pose,
                        },
                    });
                }
                // Tamed cat/wolf collar: the collar mask on the same model,
                // tinted by the dye colour, a hair larger so it sits proud.
                if let Some(col) = snap.collar {
                    let collar_tex = match snap.kind.as_str() {
                        "cat" => self.cat_collar_tex,
                        "wolf" => self.wolf_collar_tex,
                        _ => 0,
                    };
                    if renderer.is_some_and(|r| r.has_skin(collar_tex)) {
                        let d = dye_rgb(col);
                        out.push(EntityDraw {
                            pos,
                            yaw,
                            light,
                            tint: [d[0] * tint[0], d[1] * tint[1], d[2] * tint[2]],
                            roll,
                            kind: EntityDrawKind::Mob {
                                tex: collar_tex,
                                model,
                                swing,
                                head_pitch: pitch,
                                head_yaw,
                                scale: scale * 1.02,
                                anim: 0.0,
                                pose,
                            },
                        });
                    }
                }
                // Saddles, horse armour, llama carpets and wolf armour: the
                // same model again wearing the equipment's texture, a hair
                // proud of the body so it never z-fights.
                for path in [
                    animal_saddle_texture(&snap.kind, snap.equipment.saddle.as_deref()),
                    animal_body_texture(&snap.kind, snap.equipment.body.as_deref()),
                ]
                .into_iter()
                .flatten()
                {
                    let Some(&eq_tex) = self.animal_equipment.get(&path) else {
                        continue;
                    };
                    if !renderer.is_some_and(|r| r.has_skin(eq_tex)) {
                        continue;
                    }
                    out.push(EntityDraw {
                        pos,
                        yaw,
                        tint,
                        light,
                        roll,
                        kind: EntityDrawKind::Mob {
                            tex: eq_tex,
                            model,
                            swing,
                            head_pitch: pitch,
                            head_yaw,
                            scale: scale * 1.03,
                            anim: 0.0,
                            pose,
                        },
                    });
                }
                // A warm-ocean zombie nautilus grows extra coral on its
                // shell (`ZombieNautilusCoralModel`) — hidden the moment it's
                // wearing body armour, exactly like vanilla.
                if snap.kind == "zombie_nautilus"
                    && snap.variant == 1
                    && snap.equipment.body.is_none()
                    && let Some(&coral_tex) =
                        self.mob_variant_tex.get(&("zombie_nautilus".to_string(), 1))
                    && renderer.is_some_and(|r| r.has_skin(coral_tex))
                {
                    out.push(EntityDraw {
                        pos,
                        yaw,
                        tint,
                        light,
                        roll,
                        kind: EntityDrawKind::Mob {
                            tex: coral_tex,
                            model: MobModel::NautilusCorals,
                            swing: 0.0,
                            head_pitch: 0.0,
                            head_yaw: 0.0,
                            scale,
                            anim: 0.0,
                            pose: MobPose::None,
                        },
                    });
                }
                // A goat's horns fall off independently when it rams into
                // something — each one is its own layer over the base head,
                // hidden the moment its `HasLeftHorn`/`HasRightHorn` flag
                // goes false, exactly like vanilla's `GoatModel.setupAnim`.
                if snap.kind == "goat" && renderer.is_some_and(|r| r.has_skin(tex)) {
                    for (present, model) in [
                        (snap.goat_left_horn, MobModel::GoatLeftHorn),
                        (snap.goat_right_horn, MobModel::GoatRightHorn),
                    ] {
                        if !present {
                            continue;
                        }
                        out.push(EntityDraw {
                            pos,
                            yaw,
                            tint,
                            light,
                            roll,
                            kind: EntityDrawKind::Mob {
                                tex,
                                model,
                                swing: 0.0,
                                head_pitch: pitch,
                                head_yaw,
                                scale,
                                anim: 0.0,
                                pose: MobPose::None,
                            },
                        });
                    }
                }
                // Charged ("powered") creeper: the blue energy-swirl overlay,
                // inflated a little so the green body shows through the gaps.
                if snap.powered
                    && snap.kind == "creeper"
                    && renderer.is_some_and(|r| r.has_skin(self.creeper_armor_tex))
                {
                    out.push(EntityDraw {
                        pos,
                        yaw,
                        tint,
                        light,
                        roll,
                        kind: EntityDrawKind::Mob {
                            tex: self.creeper_armor_tex,
                            model,
                            swing,
                            head_pitch: pitch,
                            head_yaw,
                            scale: scale * 1.08,
                            anim: 0.0,
                            pose,
                        },
                    });
                }
                continue;
            }

            // --- everything else: a per-type tinted box (never uniform yellow) -
            let (w, h) = (snap.width.max(0.1), snap.height.max(0.1));
            out.push(EntityDraw {
                pos,
                yaw,
                tint,
                light,
                roll,
                kind: EntityDrawKind::Box {
                    w,
                    h,
                    color: mob_color(&snap.kind),
                },
            });
        }
        if let Some(me) = self.local_player_draw() {
            shadows.push((me.pos, 0.5));
            out.push(me);
        }
        // Leads and fishing lines, now that every entity's position is known.
        // The far end is either another tracked entity or — for our own fishing
        // rod, whose owner the bridge never reports as a remote entity — the
        // local player's hand.
        for (from, other, line) in ropes {
            let anchor = match self.tracks.get(&other) {
                Some(t) => {
                    let p = t.snap.pos;
                    // Held ropes hang from the holder's hand, not their feet.
                    [p[0], p[1] + t.snap.height as f64 * 0.7, p[2]]
                }
                None if line => match self.player.as_ref() {
                    // Roughly where the rod's tip sits in first person.
                    Some(p) => [p.pos[0], p.pos[1] + 1.25, p.pos[2]],
                    None => continue,
                },
                None => continue,
            };
            let to = [
                (anchor[0] - from[0]) as f32,
                (anchor[1] - from[1]) as f32,
                (anchor[2] - from[2]) as f32,
            ];
            let dist = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
            if dist > 40.0 {
                continue; // stale pairing; don't draw a rope across the map
            }
            out.push(EntityDraw {
                pos: from,
                yaw: 0.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Rope {
                    to,
                    // A fishing line is taut; a lead droops with its length.
                    sag: if line { 0.02 } else { dist * 0.12 },
                    thickness: if line { 0.02 } else { 0.05 },
                    color: if line {
                        [0.04, 0.04, 0.04]
                    } else {
                        [0.35, 0.27, 0.20]
                    },
                },
            });
        }
        // Entity shadows, vanilla-style: a blob projected onto the full blocks
        // under each entity, fading with camera distance and gone past 16 m.
        if renderer.is_some_and(|r| r.has_skin(self.shadow_tex)) {
            for (pos, radius) in shadows {
                let d2 = (pos[0] - cam_pos[0]).powi(2)
                    + (pos[1] - cam_pos[1]).powi(2)
                    + (pos[2] - cam_pos[2]).powi(2);
                // Vanilla: strength = 1 − d²/256, halved when the quads are built.
                let alpha = ((1.0 - d2 / 256.0) * 0.5).clamp(0.0, 0.5) as f32;
                if alpha <= 0.002 {
                    continue;
                }
                let patches = self.shadow_patches(pos, radius);
                if patches.is_empty() {
                    continue;
                }
                out.push(EntityDraw {
                    pos,
                    yaw: 0.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Shadow {
                        tex: self.shadow_tex,
                        radius,
                        alpha,
                        patches,
                    },
                });
            }
        }
        // Particles: camera-facing textured billboards, centered on their
        // position. Animated families step through their frames over lifetime.
        for p in &self.particles {
            let frac = (p.age / p.life).clamp(0.0, 1.0);
            // Shrink a touch toward end-of-life so they fade out instead of popping.
            let size = p.size * (0.5 + 0.5 * (1.0 - frac));
            let color = match p.fade_to {
                Some(to) if frac > 0.5 => {
                    let k = ((frac - 0.5) * 2.0).clamp(0.0, 1.0);
                    [
                        p.color[0] + (to[0] - p.color[0]) * k,
                        p.color[1] + (to[1] - p.color[1]) * k,
                        p.color[2] + (to[2] - p.color[2]) * k,
                    ]
                }
                _ => p.color,
            };
            // An item-icon particle billboards the real item's icon (from the
            // item atlas) instead of stepping through a particle-atlas family.
            let kind = if let Some(uv) = p.item_uv {
                EntityDrawKind::ItemParticle { uv, color, size }
            } else {
                let Some(frames) = self.particle_atlas_uv.get(&p.tex) else {
                    continue;
                };
                let Some(&uv) = frames.get(
                    ((frac * frames.len() as f32) as usize).min(frames.len().saturating_sub(1)),
                ) else {
                    continue;
                };
                EntityDrawKind::Particle { uv, color, size }
            };
            out.push(EntityDraw {
                pos: p.pos,
                yaw: 0.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind,
            });
        }
        // Rain: thin tall streaks, a desaturated blue-gray, slightly dimmer at
        // night (the sky darkening handles most of the mood).
        // Weather: vanilla's own rain and snow textures on upright sheets,
        // dimmed with the daylight so a storm at night is not a light show.
        if !self.rain_drops.is_empty() {
            let d = 0.55 + 0.45 * self.daylight;
            let color = [0.75 * d, 0.80 * d, 0.95 * d];
            for drop in &self.rain_drops {
                let tex = if drop.snow {
                    self.snow_tex
                } else {
                    self.rain_tex
                };
                if !self.renderer.as_ref().is_some_and(|r| r.has_skin(tex)) {
                    continue;
                }
                out.push(EntityDraw {
                    pos: drop.pos,
                    yaw: 0.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Precip {
                        tex,
                        w: if drop.snow { 0.30 } else { 0.32 },
                        h: if drop.snow { 0.30 } else { 1.4 },
                        uv: precip_uv(drop),
                        alpha: (0.55 + 0.35 * self.rain_level).min(0.95),
                        color,
                    },
                });
            }
        }
        // Lightning: vanilla's bolt lives for 10 ticks and fades out.
        self.lightning
            .retain(|(_, _, at)| at.elapsed() < Duration::from_millis(500));
        for &(pos, seed, at) in &self.lightning {
            let alpha = 1.0 - at.elapsed().as_secs_f32() / 0.5;
            out.push(EntityDraw {
                pos,
                yaw: 0.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Lightning {
                    seed,
                    alpha: alpha.clamp(0.0, 1.0),
                },
            });
        }
        // Now that `tracks` is free again, let everything that moved be heard.
        for (ev, pos, kind) in step_sounds {
            self.play_step_event(ev, pos, &kind);
        }
        self.beacon_beams(&mut out, cam_pos);
        self.block_entity_draws(&mut out, cam_pos);
        self.open_container_draws(&mut out, cam_pos);
        self.piston_draws(&mut out, cam_pos);
        out
    }

    /// Everything a piston has in the air right now: the blocks it is moving,
    /// each drawn as its own geometry sliding from one cell to the next, and
    /// the head growing out of (or sinking back into) the piston itself.
    fn piston_draws(&mut self, out: &mut Vec<EntityDraw>, cam: [f64; 3]) {
        let now = Instant::now();
        self.pistons.tick(now);
        if self.pistons.is_empty() {
            return;
        }
        const RANGE: f64 = 96.0;
        for stroke in self.pistons.iter() {
            let d = [
                stroke.piston.x as f64 - cam[0],
                stroke.piston.y as f64 - cam[1],
                stroke.piston.z as f64 - cam[2],
            ];
            if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > RANGE * RANGE {
                continue;
            }
            let p = stroke.progress(now);
            // Lit like the piston itself: the cells the blocks pass through are
            // in motion, so there is nothing stable to sample.
            let light = self.light_at_pos([
                stroke.piston.x as f64 + 0.5,
                stroke.piston.y as f64 + 0.5,
                stroke.piston.z as f64 + 0.5,
            ]);
            let biome = self.mirror.biome_at(stroke.piston).unwrap_or(0);
            for rider in &stroke.blocks {
                // Once the server's own copy of the block has landed, stop
                // drawing ours over the top of it.
                let dest = stroke.rider_dest(rider);
                if self.mirror.get_block(dest) == rider.state {
                    continue;
                }
                let pos = stroke.rider_pos(rider, p);
                self.push_block_geometry(out, rider.state, pos, light, biome);
            }
            if stroke.head && stroke.head_state != 0 {
                let pos = stroke.head_pos(p);
                self.push_block_geometry(out, stroke.head_state, pos, light, biome);
            }
        }
    }

    /// Draw one block's baked geometry loose in the world at `pos` (its lower
    /// corner). Tinted quads — grass, leaves, water — go out as a second draw
    /// so the biome colour lands on them and only them.
    fn push_block_geometry(
        &self,
        out: &mut Vec<EntityDraw>,
        state: StateId,
        pos: [f64; 3],
        light: [f32; 2],
        biome: u32,
    ) {
        let (plain, tinted, kind) = block_geometry_split(&self.store, state);
        let mut emit = |quads: Vec<([f32; 3], [f32; 2])>, tint: [f32; 3]| {
            if quads.is_empty() {
                return;
            }
            out.push(EntityDraw {
                pos,
                yaw: 0.0,
                tint,
                light,
                roll: 0.0,
                kind: EntityDrawKind::DisplayBlock {
                    quads,
                    translation: [0.0; 3],
                    scale: [1.0; 3],
                    left_rot: [0.0, 0.0, 0.0, 1.0],
                    right_rot: [0.0, 0.0, 0.0, 1.0],
                },
            });
        };
        emit(plain, [1.0, 1.0, 1.0]);
        if let Some(kind) = kind {
            let rgb = match kind {
                crate::models::TintKind::Grass => self.biome_tints.grass(biome),
                crate::models::TintKind::Foliage => self.biome_tints.foliage(biome),
                crate::models::TintKind::Water => self.biome_tints.water(biome),
                // `redstone_wire` has no item/entity icon of its own (the dust
                // item is a separate, untinted sprite) — unreachable in
                // practice, kept only for match exhaustiveness.
                crate::models::TintKind::Redstone => {
                    crate::world::mesher::redstone_color(0)
                }
            };
            emit(
                tinted,
                [
                    rgb[0] as f32 / 255.0,
                    rgb[1] as f32 / 255.0,
                    rgb[2] as f32 / 255.0,
                ],
            );
        }
    }

    /// Draws for every container in range whose lid can move: chests (single
    /// and both halves of a double), ender and trapped and copper chests, and
    /// shulker boxes. The mesher deliberately leaves these out of the terrain,
    /// so this is the only thing drawing them — exactly as in vanilla, where a
    /// chest is a block entity and disappears at the block-entity view distance.
    fn open_container_draws(&mut self, out: &mut Vec<EntityDraw>, cam: [f64; 3]) {
        if self.open_containers.is_empty() {
            return;
        }
        const RANGE: f64 = 64.0;
        // Resolve everything that needs `&self` up front: the loop below hands
        // the texture store and the asset pack out mutably.
        struct Job {
            pos: BlockPos,
            block: DynBlock,
            light: [f32; 2],
            progress: f32,
        }
        let mut jobs: Vec<Job> = Vec::new();
        for (spos, list) in &self.open_containers {
            // Cheap section cull first: a section is 16 blocks across, so its
            // centre can be that much closer than any block in it.
            let sd = [
                (spos.x * 16 + 8) as f64 - cam[0],
                (spos.y * 16 + 8) as f64 - cam[1],
                (spos.z * 16 + 8) as f64 - cam[2],
            ];
            if sd[0] * sd[0] + sd[1] * sd[1] + sd[2] * sd[2] > (RANGE + 16.0) * (RANGE + 16.0) {
                continue;
            }
            for &(pos, state) in list {
                let d = [
                    pos.x as f64 + 0.5 - cam[0],
                    pos.y as f64 + 0.5 - cam[1],
                    pos.z as f64 + 0.5 - cam[2],
                ];
                if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > RANGE * RANGE {
                    continue;
                }
                let Some(block) = self.store.dyn_block(state) else {
                    continue;
                };
                jobs.push(Job {
                    pos,
                    block: block.clone(),
                    // Lit from the cell above, like every other block entity —
                    // its own cell is shadowed by the container standing in it.
                    light: self.light_at_pos([
                        pos.x as f64 + 0.5,
                        pos.y as f64 + 1.0,
                        pos.z as f64 + 0.5,
                    ]),
                    progress: self.lids.progress(pos),
                });
            }
        }
        let time = self.start.elapsed().as_secs_f32();
        let be = &mut self.block_entities;
        let pack = &mut self.pack;
        for job in jobs {
            let (tex_name, model, pos, yaw, roll, swing) = match &job.block {
                DynBlock::Chest { tex, kind, yaw, .. } => {
                    let (suffix, model) = match kind {
                        ChestKind::Single => ("", MobModel::Chest),
                        ChestKind::Left => ("_left", MobModel::ChestLeft),
                        ChestKind::Right => ("_right", MobModel::ChestRight),
                    };
                    (
                        format!("{tex}{suffix}"),
                        model,
                        [
                            job.pos.x as f64 + 0.5,
                            job.pos.y as f64,
                            job.pos.z as f64 + 0.5,
                        ],
                        *yaw,
                        0.0,
                        lids::chest_angle(job.progress),
                    )
                }
                // A shulker box is authored around the block centre so it can
                // be tipped onto whichever face it is stuck to.
                DynBlock::Shulker { tex, yaw, roll } => (
                    tex.to_string(),
                    MobModel::ShulkerBox,
                    [
                        job.pos.x as f64 + 0.5,
                        job.pos.y as f64 + 0.5,
                        job.pos.z as f64 + 0.5,
                    ],
                    *yaw,
                    *roll,
                    job.progress,
                ),
                // The enchanting table's book hangs over the table, bobbing and
                // turning to face whoever comes near it — vanilla turns it
                // toward the closest player within three blocks. A lectern's
                // book lies flat on the stand instead, angled toward its front.
                DynBlock::Book { lectern, yaw } => {
                    let centre = [
                        job.pos.x as f64 + 0.5,
                        job.pos.y as f64,
                        job.pos.z as f64 + 0.5,
                    ];
                    if *lectern {
                        (
                            "entity/enchantment/enchanting_table_book".to_string(),
                            MobModel::Book,
                            [centre[0], centre[1] + 1.06, centre[2]],
                            *yaw,
                            -68.0,
                            0.0,
                        )
                    } else {
                        let near = self.player.as_ref().map(|p| {
                            let d = [p.pos[0] - centre[0], p.pos[2] - centre[2]];
                            (d[0] * d[0] + d[1] * d[1], d)
                        });
                        // Face the player while they are close enough to read
                        // it, and drift back to square on when they leave.
                        let yaw = match near {
                            Some((dist2, d)) if dist2 < 9.0 => {
                                (d[0].atan2(d[1]) as f32).to_degrees()
                            }
                            _ => t_book_idle(time),
                        };
                        let bob = (time * 0.6).sin() * 0.02;
                        (
                            "entity/enchantment/enchanting_table_book".to_string(),
                            MobModel::Book,
                            [centre[0], centre[1] + 0.79 + bob as f64, centre[2]],
                            yaw,
                            // Vanilla lays the book back at 80° so its pages
                            // face up and out over the table.
                            80.0,
                            0.0,
                        )
                    }
                }
            };
            let key = fnv64(tex_name.as_bytes());
            if !be.build(key, || pack.texture_png(&tex_name).ok()) {
                continue;
            }
            out.push(EntityDraw {
                pos,
                yaw,
                light: job.light,
                tint: [1.0, 1.0, 1.0],
                roll,
                kind: EntityDrawKind::Mob {
                    tex: key,
                    model,
                    swing,
                    head_pitch: 0.0,
                    head_yaw: 0.0,
                    scale: 1.0,
                    anim: 0.0,
                    pose: MobPose::None,
                },
            });
        }
    }

    /// Draws for every block entity in range: sign text, banner cloth, heads,
    /// bells, conduits and decorated pots. All of these have particle-only
    /// block models, so nothing here is drawn twice.
    fn block_entity_draws(&mut self, out: &mut Vec<EntityDraw>, cam: [f64; 3]) {
        if self.block_entities.map.is_empty() {
            return;
        }
        // Vanilla stops drawing block entities well before the terrain fades;
        // sign text in particular is unreadable long before that.
        const RANGE: f64 = 64.0;
        let now = Instant::now();
        let time = self.start.elapsed().as_secs_f32();
        // Everything that needs `&self` is resolved up front, because the loop
        // below hands `&mut self.block_entities` to the compositor.
        struct Job {
            pos: BlockPos,
            short: String,
            rotation: Option<f32>,
            facing: Option<String>,
            player_head: Option<u64>,
            conduit_active: bool,
            struck: Option<(f32, u8)>,
            light: [f32; 2],
        }
        let mut jobs: Vec<Job> = Vec::new();
        for (&pos, data) in &self.block_entities.map {
            let d = [
                pos.x as f64 + 0.5 - cam[0],
                pos.y as f64 + 0.5 - cam[1],
                pos.z as f64 + 0.5 - cam[2],
            ];
            if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > RANGE * RANGE {
                continue;
            }
            let Some(entry) = self.table.entry(self.mirror.get_block(pos)) else {
                continue;
            };
            jobs.push(Job {
                pos,
                short: entry.short_name.clone(),
                rotation: entry.prop("rotation").and_then(|r| r.parse::<f32>().ok()),
                facing: entry.prop("facing").map(str::to_owned),
                // A player head needs its owner's skin downloaded first.
                player_head: match data {
                    BlockEntityData::Skull {
                        texture_url: Some(url),
                        ..
                    } => {
                        let key = fnv64(key_of_url(url).as_bytes());
                        self.renderer
                            .as_ref()
                            .is_some_and(|r| r.has_skin(key))
                            .then_some(key)
                    }
                    _ => None,
                },
                conduit_active: matches!(data, BlockEntityData::Conduit)
                    && self.conduit_active(pos),
                struck: self.block_entities.struck(pos, now),
                // Vanilla lights a block entity from the cell above it — the
                // one its own block would otherwise shadow.
                light: self.light_at_pos([
                    pos.x as f64 + 0.5,
                    pos.y as f64 + 1.0,
                    pos.z as f64 + 0.5,
                ]),
            });
        }
        // Lift the map out so the payloads can be read while the compositor
        // holds the rest of the store mutably — no per-frame cloning.
        let map = std::mem::take(&mut self.block_entities.map);
        for Job {
            pos,
            short,
            rotation,
            facing,
            player_head,
            conduit_active,
            struck,
            light,
        } in jobs
        {
            let Some(data) = map.get(&pos) else { continue };
            let st = blockentities::BeState {
                short: &short,
                rotation,
                facing: facing.as_deref(),
                player_head,
                conduit_active,
                time,
                struck,
            };
            let draw = blockentities::draw_for(
                &mut self.block_entities,
                &mut self.pack,
                &self.font,
                data,
                &st,
            );
            let origin = [pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5];
            for part in draw.parts {
                let p = rotate_offset(origin, part.offset, part.yaw);
                out.push(EntityDraw {
                    pos: p,
                    yaw: part.yaw,
                    light,
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Mob {
                        tex: part.tex,
                        model: part.model,
                        swing: part.swing,
                        head_pitch: 0.0,
                        head_yaw: 0.0,
                        scale: part.scale,
                        anim: 0.0,
                        pose: MobPose::None,
                    },
                });
            }
            for text in draw.texts {
                let p = rotate_offset(origin, text.offset, text.yaw);
                out.push(EntityDraw {
                    pos: p,
                    yaw: text.yaw,
                    light,
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Decal {
                        tex: text.tex,
                        w: text.size[0],
                        h: text.size[1],
                        glowing: text.glowing,
                    },
                });
            }
            // Campfire food: a flat item icon lying on the fire. The display
            // transform is the one item-display entities use, laid flat.
            for it in draw.items {
                let Some(uv) = self.item_icons.uv(&it.item) else {
                    continue;
                };
                let half = (-it.yaw.to_radians() * 0.5).sin_cos();
                out.push(EntityDraw {
                    pos: [
                        origin[0] + it.offset[0] as f64,
                        origin[1] + it.offset[1] as f64,
                        origin[2] + it.offset[2] as f64,
                    ],
                    yaw: 0.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::DisplayItem {
                        uv,
                        translation: [0.0; 3],
                        scale: [0.5, 0.5, 0.5],
                        // Lie the quad flat (a quarter turn about X), then spin
                        // it about the vertical by the slot's yaw.
                        left_rot: [half.0, 0.0, 0.0, half.1],
                        right_rot: [
                            -std::f32::consts::FRAC_1_SQRT_2,
                            0.0,
                            0.0,
                            std::f32::consts::FRAC_1_SQRT_2,
                        ],
                    },
                });
            }
        }
        // Anything the server sent while we were drawing wins over the old map.
        let fresh = std::mem::replace(&mut self.block_entities.map, map);
        self.block_entities.map.extend(fresh);
    }

    /// Vanilla's conduit activation test: the 3×3×3 around it must be water and
    /// at least one prismarine frame block must sit on the surrounding rings.
    fn conduit_active(&self, pos: BlockPos) -> bool {
        let water = |p: BlockPos| self.table.contains_water(self.mirror.get_block(p));
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let p = BlockPos {
                        x: pos.x + dx,
                        y: pos.y + dy,
                        z: pos.z + dz,
                    };
                    if p != pos && !water(p) {
                        return false;
                    }
                }
            }
        }
        blockentities::conduit_frame_offsets()
            .into_iter()
            .any(|[dx, dy, dz]| {
                let p = BlockPos {
                    x: pos.x + dx,
                    y: pos.y + dy,
                    z: pos.z + dz,
                };
                self.table
                    .entry(self.mirror.get_block(p))
                    .is_some_and(|e| blockentities::is_conduit_frame(&e.short_name))
            })
    }

    /// Vanilla's per-block shadow projection: the ground surfaces under `pos`
    /// inside the shadow square, as `[dx0, dz0, dx1, dz1, dy]` offsets from the
    /// entity. Only full cubes catch a shadow, and only within `radius` blocks
    /// below the entity — which is why a vanilla shadow shrinks away as its
    /// owner jumps. Footprints are clipped to the square so the blob's UVs stay
    /// inside the sprite.
    fn shadow_patches(&self, pos: [f64; 3], radius: f32) -> Vec<[f32; 5]> {
        shadow_patches_with(pos, radius, |x, y, z| {
            self.store
                .occludes(self.mirror.get_block(BlockPos { x, y, z }), Face::Up)
        })
    }

    /// Is this state a beacon block?
    fn is_beacon(&self, id: StateId) -> bool {
        self.table
            .entry(id)
            .is_some_and(|e| e.short_name == "beacon")
    }

    /// Vanilla beacon beams. A beacon shoots its beam when it has at least a
    /// level-1 base (a full 3x3 of beacon-base blocks right beneath it) and
    /// nothing solid in the way; stained glass in the column tints the beam,
    /// multiplying one colour into the next exactly like the server does. The
    /// beam is drawn twice: an opaque spinning core and a wide, faint glow.
    fn beacon_beams(&mut self, out: &mut Vec<EntityDraw>, cam: [f64; 3]) {
        if self.beacons.is_empty()
            || !self
                .renderer
                .as_ref()
                .is_some_and(|r| r.has_skin(self.beam_tex))
        {
            return;
        }
        // Vanilla scrolls the beam texture at 0.2 tiles/tick and spins the core
        // 2.25 degrees/tick (20 ticks a second).
        let ticks = self.start.elapsed().as_secs_f32() * 20.0;
        let v_off = -1.0 + (ticks * 0.2 - (ticks * 0.1).floor()).fract();
        let core_spin = ticks * 2.25 - 45.0;
        let table = self.table.clone();
        let store = self.store.clone();
        let mirror = &self.mirror;
        let mut beams: Vec<(BlockPos, [f32; 3])> = Vec::new();
        self.beacons.retain(|&pos| {
            // Prune beacons that were replaced while their section stayed loaded.
            let here = mirror.get_block(pos);
            if !table.entry(here).is_some_and(|e| e.short_name == "beacon") {
                return false;
            }
            let dx = pos.x as f64 + 0.5 - cam[0];
            let dz = pos.z as f64 + 0.5 - cam[2];
            if dx * dx + dz * dz > 256.0 * 256.0 {
                return true; // keep tracking it, just don't draw it
            }
            let base_ok = (-1..=1).all(|ox| {
                (-1..=1).all(|oz| {
                    let id = mirror.get_block(BlockPos {
                        x: pos.x + ox,
                        y: pos.y - 1,
                        z: pos.z + oz,
                    });
                    table
                        .entry(id)
                        .is_some_and(|e| BEACON_BASE.contains(&e.short_name.as_str()))
                })
            });
            if !base_ok {
                return true;
            }
            // Tint the beam with the stained glass above it, and drop the beam
            // entirely if something solid caps the column.
            let mut color = [1.0f32, 1.0, 1.0];
            for y in (pos.y + 1)..(pos.y + 65) {
                let id = mirror.get_block(BlockPos {
                    x: pos.x,
                    y,
                    z: pos.z,
                });
                let Some(e) = table.entry(id) else { break };
                if let Some(dye) = stained_glass_dye(&e.short_name) {
                    let d = dye_rgb(dye);
                    for c in 0..3 {
                        color[c] *= d[c];
                    }
                } else if !table.is_air(id) && store.occludes(id, Face::Up) {
                    return true;
                }
            }
            beams.push((pos, color));
            true
        });
        for (pos, color) in beams {
            let p = [pos.x as f64 + 0.5, pos.y as f64 + 1.0, pos.z as f64 + 0.5];
            // Vanilla runs the beam to the top of the world.
            let height = (320 - pos.y).max(16) as f32;
            for (width, alpha, spin) in [(0.2, 1.0, core_spin), (0.25, 0.125, 0.0)] {
                out.push(EntityDraw {
                    pos: p,
                    yaw: 0.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Beam {
                        tex: self.beam_tex,
                        height,
                        width,
                        alpha,
                        color,
                        spin,
                        v_off,
                    },
                });
            }
        }
    }
}

/// Blocks a beacon pyramid may be built from (vanilla's `BlockTags.BEACON_BASE_BLOCKS`).
const BEACON_BASE: [&str; 5] = [
    "iron_block",
    "gold_block",
    "diamond_block",
    "emerald_block",
    "netherite_block",
];

/// Dye index of a stained-glass block or pane, for the beacon beam tint.
fn stained_glass_dye(name: &str) -> Option<i32> {
    let base = name
        .strip_suffix("_stained_glass_pane")
        .or_else(|| name.strip_suffix("_stained_glass"))?;
    const DYES: [&str; 16] = [
        "white",
        "orange",
        "magenta",
        "light_blue",
        "yellow",
        "lime",
        "pink",
        "gray",
        "light_gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ];
    DYES.iter().position(|d| *d == base).map(|i| i as i32)
}

/// The projection itself, with the world behind a predicate (`is_full`) so the
/// offscreen previews can run it over a hand-built scene.
pub(crate) fn shadow_patches_with(
    pos: [f64; 3],
    radius: f32,
    is_full: impl Fn(i32, i32, i32) -> bool,
) -> Vec<[f32; 5]> {
    let r = radius as f64;
    let (x0, x1) = (pos[0] - r, pos[0] + r);
    let (z0, z1) = (pos[2] - r, pos[2] + r);
    let top = pos[1].floor() as i32;
    let bottom = (pos[1] - r).floor() as i32;
    let mut out = Vec::new();
    for bx in x0.floor() as i32..=x1.floor() as i32 {
        for bz in z0.floor() as i32..=z1.floor() as i32 {
            // Highest full block in range: its top face takes the shadow.
            let Some(y) = (bottom..=top).rev().find(|&by| is_full(bx, by - 1, bz)) else {
                continue;
            };
            let (cx0, cx1) = ((bx as f64).max(x0), (bx as f64 + 1.0).min(x1));
            let (cz0, cz1) = ((bz as f64).max(z0), (bz as f64 + 1.0).min(z1));
            if cx1 <= cx0 || cz1 <= cz0 {
                continue;
            }
            out.push([
                (cx0 - pos[0]) as f32,
                (cz0 - pos[2]) as f32,
                (cx1 - pos[0]) as f32,
                (cz1 - pos[2]) as f32,
                // A hair above the surface so it never z-fights the block.
                (y as f64 - pos[1]) as f32 + 0.015,
            ]);
        }
    }
    out
}

/// Vanilla's `ItemEntityRenderer.getRenderAmount`: how many copies of the model
/// a dropped stack is drawn from, so a big pile actually looks big.
fn render_amount(count: u32) -> u32 {
    match count {
        0..=1 => 1,
        2..=16 => 2,
        17..=32 => 3,
        33..=48 => 4,
        _ => 5,
    }
}

/// Per-copy offset for a stacked dropped item. Vanilla seeds a `Random` from
/// the stack and nudges each extra copy by ±0.15 (blocks) or ±0.075 in x/y only
/// (flat sprites); this is the same shape with a deterministic hash so a pile
/// doesn't jitter from frame to frame.
fn stack_offset(id: u64, copy: u32, block: bool) -> (f64, f64, f64) {
    if copy == 0 {
        return (0.0, 0.0, 0.0);
    }
    let mut h =
        id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (copy as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let mut next = || {
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h ^= h >> 29;
        (h >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    };
    if block {
        (next() * 0.15, next() * 0.15, next() * 0.15)
    } else {
        (next() * 0.075, next() * 0.075, 0.0)
    }
}

/// Vanilla's shadow radius for an entity. Most renderers pass roughly three
/// quarters of the hitbox width (player 0.5 at 0.6 wide, pig 0.7 at 0.9,
/// chicken 0.3 at 0.4); flat/wall entities and projectiles cast none at all.
fn shadow_radius(kind: &str, width: f32) -> f32 {
    const NONE: [&str; 20] = [
        "painting",
        "item_frame",
        "glow_item_frame",
        "block_display",
        "item_display",
        "text_display",
        "interaction",
        "marker",
        "arrow",
        "spectral_arrow",
        "trident",
        "fishing_bobber",
        "leash_knot",
        "end_crystal",
        "lightning_bolt",
        "area_effect_cloud",
        // All four extend plain `EntityRenderer` rather than
        // `LivingEntityRenderer`, so — like the projectiles above — vanilla
        // never gives them the generic shadow the base class adds.
        "evoker_fangs",
        "shulker_bullet",
        "llama_spit",
        "ominous_item_spawner",
    ];
    if NONE.contains(&kind) {
        return 0.0;
    }
    (width * 0.75).clamp(0.12, 1.0)
}

/// Vanilla `DyeColor` → linear-ish RGB (0..1), in id order (white=0 … black=15).
/// Used to tint dyeable overlays (tropical-fish body/pattern, pet collars).
const DYE_RGB: [[f32; 3]; 16] = [
    [0.976, 1.000, 0.996], // white
    [0.976, 0.502, 0.114], // orange
    [0.780, 0.306, 0.741], // magenta
    [0.227, 0.702, 0.855], // light_blue
    [0.996, 0.847, 0.239], // yellow
    [0.502, 0.780, 0.122], // lime
    [0.953, 0.545, 0.667], // pink
    [0.278, 0.310, 0.322], // gray
    [0.616, 0.616, 0.592], // light_gray
    [0.086, 0.612, 0.612], // cyan
    [0.537, 0.196, 0.722], // purple
    [0.235, 0.267, 0.667], // blue
    [0.514, 0.329, 0.196], // brown
    [0.369, 0.486, 0.086], // green
    [0.690, 0.180, 0.149], // red
    [0.114, 0.114, 0.129], // black
];

/// The saddle texture an animal wears, if it is wearing one. Every rideable
/// species has its own sheet cut for its own model.
fn animal_saddle_texture(kind: &str, saddle: Option<&str>) -> Option<String> {
    saddle?;
    let species = match kind {
        "pig" | "strider" | "horse" | "donkey" | "mule" | "camel" | "skeleton_horse"
        | "zombie_horse" | "camel_husk" | "nautilus" => kind,
        // A zombie nautilus rides the same saddle sheet as its tamed cousin.
        "zombie_nautilus" => "nautilus",
        _ => return None,
    };
    Some(format!("entity/equipment/{species}_saddle/saddle"))
}

/// The body-armour texture an animal wears: horse armour by material, or wolf
/// armour. (A llama's carpet is cut for vanilla's separate decor model, not for
/// the llama itself, so painting it onto the llama bleeds onto the neck — it is
/// left out rather than drawn wrong.)
fn animal_body_texture(kind: &str, body: Option<&str>) -> Option<String> {
    let item = body?;
    match kind {
        "horse" | "donkey" | "mule" | "skeleton_horse" | "zombie_horse" => {
            // `iron_horse_armor` → `iron`, and likewise for the rest.
            let material = item.strip_suffix("_horse_armor")?;
            Some(format!("entity/equipment/horse_body/{material}"))
        }
        "wolf" => {
            (item == "wolf_armor").then(|| "entity/equipment/wolf_body/armadillo_scute".to_string())
        }
        "nautilus" | "zombie_nautilus" => {
            // `copper_nautilus_armor` → `copper`, and likewise for the rest.
            let material = item.strip_suffix("_nautilus_armor")?;
            Some(format!("entity/equipment/nautilus_body/{material}"))
        }
        _ => None,
    }
}

/// Pad a 64×32 entity sheet out to 64×64. Capes and elytra ship at half height,
/// but the skin UV maths divides by 64 on both axes.
fn pad_to_square(img: &image::RgbaImage) -> image::RgbaImage {
    if img.height() >= img.width() {
        return img.clone();
    }
    let mut out = image::RgbaImage::new(img.width(), img.width());
    image::imageops::overlay(&mut out, img, 0, 0);
    out
}

/// Look up a dye colour by id, clamped; unknown ids fall back to white.
fn dye_rgb(id: i32) -> [f32; 3] {
    *DYE_RGB
        .get(id.rem_euclid(16) as usize)
        .unwrap_or(&[1.0, 1.0, 1.0])
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

/// Where an enchanting table's book points with nobody near it: vanilla leaves
/// it where the last reader left it, which on a fresh client is a slow drift.
fn t_book_idle(time: f32) -> f32 {
    time * 12.0
}

/// Does this block have a lid the client animates? The server reports viewer
/// counts for a few more containers than that (barrels, for instance, say how
/// many people are in them even though their "open" is a block state), so the
/// list is exactly the blocks whose lid we draw ourselves.
fn opens_a_lid(block: &str) -> bool {
    block.ends_with("chest") || block.ends_with("shulker_box")
}

/// Shortest-arc interpolation between two angles in degrees.
/// Apply an offset given in a model's own frame (+Z = the way it faces, yaw in
/// vanilla degrees) to a world position.
fn rotate_offset(origin: [f64; 3], offset: [f32; 3], yaw: f32) -> [f64; 3] {
    let (s, c) = (-yaw.to_radians()).sin_cos();
    [
        origin[0] + (offset[0] * c + offset[2] * s) as f64,
        origin[1] + offset[1] as f64,
        origin[2] + (offset[2] * c - offset[0] * s) as f64,
    ]
}

/// Where a passenger sits on its vehicle, in the vehicle's own frame (+Z =
/// forward). Boats seat two, one ahead of the other; minecarts seat one in the
/// middle; riding a mob puts you on its back.
fn seat_offset(vehicle_kind: &str, vehicle_height: f32, seat: u8) -> [f32; 3] {
    if vehicle_kind.ends_with("boat") || vehicle_kind.ends_with("raft") {
        // Vanilla's two boat seats: the front one ahead of centre, the back one
        // behind it.
        return [0.0, -0.05, if seat == 0 { 0.2 } else { -0.6 }];
    }
    if vehicle_kind.contains("minecart") {
        return [0.0, 0.0, 0.0];
    }
    // A ridden mob: sit on its back, a little above the shoulders.
    [0.0, vehicle_height * 0.75, -0.1]
}

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
    let span = times[times.len() - 1]
        .duration_since(times[0])
        .as_secs_f32();
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
fn spawn_raindrop(rng: &mut u64, center: [f64; 3], r: f64) -> RainDrop {
    let ang = xorshift01(rng) as f64 * std::f64::consts::TAU;
    let rad = (xorshift01(rng) as f64).sqrt() * r;
    let x = center[0] + ang.cos() * rad;
    let z = center[2] + ang.sin() * rad;
    let y = center[1] + 6.0 + xorshift01(rng) as f64 * 12.0;
    let speed = 18.0 + xorshift01(rng) * 9.0;
    RainDrop {
        pos: [x, y, z],
        speed,
        snow: false,
        phase: xorshift01(rng) * std::f32::consts::TAU,
    }
}

/// The patch of the weather sheet one drop wears. Vanilla's rain and snow
/// textures are 64×256 sheets of many streaks and flakes; a drop takes a narrow
/// column of them (a few streaks) or one small cell (a single flake), and lets
/// it scroll past as it falls.
fn precip_uv(drop: &RainDrop) -> [f32; 4] {
    // A narrow slice, stable per drop so a streak never jumps sideways. Small
    // patches matter: a whole sheet squeezed onto a hand-sized quad turns into
    // a lattice as soon as you get close to it.
    let col = ((drop.phase.abs() * 7.0) as u32 % 5) as f32 * 0.2;
    if drop.snow {
        let row = (drop.phase * 3.0).fract() * 0.9;
        [col, row, col + 0.2, row + 0.05]
    } else {
        let scroll = drop.phase.fract();
        [col, scroll, col + 0.2, scroll + 0.12]
    }
}

/// What a biome's climate makes fall out of its sky: `None` where it never
/// rains at all (a desert, the badlands, the dry savannas), `Some(true)` for
/// snow where it is cold enough, `Some(false)` for rain. Vanilla asks the biome
/// the same two questions.
fn precipitation_kind(downfall: f32, temperature: f32) -> Option<bool> {
    if downfall <= 0.0 {
        return None;
    }
    Some(temperature < 0.15)
}

/// True when the sky cannot reach this spot, so nothing should be falling on
/// it. Vanilla asks its heightmap; the sky light we already track answers the
/// same question for anywhere a player can see.
fn snow_or_rain_is_indoors(mirror: &WorldMirror, pos: [f64; 3]) -> bool {
    let bp = BlockPos {
        x: pos[0].floor() as i32,
        y: pos[1].floor() as i32,
        z: pos[2].floor() as i32,
    };
    mirror.light_at(bp).0 == 0
}

/// Baked geometry `(pos centered at origin in unit-cube space, atlas uv)` of a
/// block's representative state, for the 3D block-in-hand and dropped blocks.
/// `None` for non-blocks or blocks with no drawable model (air/fluids/fallbacks).
/// A free function so callers can pass disjoint field borrows (e.g. inside a
/// `self.tracks.values_mut()` loop).
/// Thrown/projectile entity kind → the item icon to draw for it (the kind
/// implies the item, so no metadata is needed). Returns `None` for kinds that
/// have their own render path (arrows, TNT) or aren't thrown items. A missing
/// icon simply falls through to the tinted-box fallback.
fn projectile_item(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "snowball" => "snowball",
        "egg" => "egg",
        "ender_pearl" => "ender_pearl",
        "eye_of_ender" => "ender_eye",
        "experience_bottle" => "experience_bottle",
        "splash_potion" | "potion" => "splash_potion",
        "lingering_potion" => "lingering_potion",
        "fireball" | "small_fireball" | "dragon_fireball" => "fire_charge",
        "wither_skull" => "wither_skeleton_skull",
        "firework_rocket" => "firework_rocket",
        "wind_charge" | "breeze_wind_charge" => "wind_charge",
        _ => return None,
    })
}

/// Typed minecart kind → the block it carries (drawn sitting in the cart).
/// `None` for the plain minecart (just the cart).
fn minecart_content(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "chest_minecart" => "chest",
        "furnace_minecart" => "furnace",
        "tnt_minecart" => "tnt",
        "hopper_minecart" => "hopper",
        "spawner_minecart" => "spawner",
        "command_block_minecart" => "command_block",
        _ => return None,
    })
}

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

/// Block geometry keyed by state id, centred on the origin like
/// `block_geometry` — for falling blocks, which carry their state in the spawn
/// packet instead of a name.
fn block_geometry_centred(
    store: &BakedModelStore,
    sid: StateId,
) -> Option<Vec<([f32; 3], [f32; 2])>> {
    let model = store.get(sid);
    if model.quads.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(model.quads.len() * 6);
    for q in &model.quads {
        for &i in &[0usize, 1, 2, 0, 2, 3] {
            let v = q.verts[i];
            out.push(([v[0] - 0.5, v[1] - 0.5, v[2] - 0.5], q.uvs[i]));
        }
    }
    Some(out)
}

/// Block geometry keyed directly by (global) state id, corner at the origin
/// (0..1) rather than centred — for block-display entities, whose transform is
/// applied about the block's origin like vanilla.
/// The same geometry, split into the quads that take a biome colour and the
/// ones that don't, with the colour they want. A grass block is mostly plain
/// dirt sides with one tinted top; leaves are tinted through and through.
fn block_geometry_split(
    store: &BakedModelStore,
    sid: StateId,
) -> (
    Vec<([f32; 3], [f32; 2])>,
    Vec<([f32; 3], [f32; 2])>,
    Option<crate::models::TintKind>,
) {
    let model = store.get(sid);
    let mut plain = Vec::new();
    let mut tinted = Vec::new();
    let mut kind = None;
    for q in &model.quads {
        let out = if q.tint.is_some() {
            kind = kind.or(q.tint);
            &mut tinted
        } else {
            &mut plain
        };
        for &i in &[0usize, 1, 2, 0, 2, 3] {
            out.push((q.verts[i], q.uvs[i]));
        }
    }
    (plain, tinted, kind)
}

/// The vanilla name of a face, as it appears in block-state properties.
fn face_name(f: Face) -> &'static str {
    match f {
        Face::Down => "down",
        Face::Up => "up",
        Face::North => "north",
        Face::South => "south",
        Face::West => "west",
        Face::East => "east",
    }
}

fn block_geometry_by_state(
    store: &BakedModelStore,
    sid: StateId,
) -> Option<Vec<([f32; 3], [f32; 2])>> {
    let model = store.get(sid);
    if model.quads.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(model.quads.len() * 6);
    for q in &model.quads {
        for &i in &[0usize, 1, 2, 0, 2, 3] {
            out.push((q.verts[i], q.uvs[i]));
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
    let end_sky = pack.texture_png("environment/end_sky").ok();
    renderer.set_sky_textures(&sun, &moons, clouds.as_ref(), end_sky.as_ref());
    info!("app: celestial sky textures loaded");
}

/// The bridge's animal pose as the renderer's. A pose replaces the walk cycle
/// on the parts it touches, which is what lets a sitting dog keep its head.
///
/// `is_spinning`/`spin_progress` are an allay's dance state, already
/// tick-interpolated from its `EntityTrack` accumulators; `roll_amount`/
/// `on_back_amount` are a panda's, the same way. All four are ignored for
/// every pose but the one that uses them.
fn mob_pose(
    pose: crate::bridge::events::AnimalPose,
    is_spinning: bool,
    spin_progress: f32,
    roll_amount: f32,
    on_back_amount: f32,
) -> MobPose {
    use crate::bridge::events::AnimalPose as A;
    match pose {
        A::Standing => MobPose::None,
        A::Sitting => MobPose::Sitting,
        A::Lying => MobPose::Lying,
        A::Rearing => MobPose::Rearing,
        A::Crouching => MobPose::Crouching,
        A::Rowing { left, right } => MobPose::Rowing { left, right },
        A::Celebrating => MobPose::Celebrating,
        A::Dancing => MobPose::Dancing { is_spinning, spin_progress },
        A::Rolling => MobPose::Rolling { amount: roll_amount },
        A::OnBack => MobPose::OnBack { amount: on_back_amount },
        A::Faceplanted => MobPose::Faceplanted,
    }
}

/// The bridge's pose as the renderer's, giving the riptide spin its angle from
/// the frame clock (vanilla spins the model, it does not hold it still).
fn player_pose(pose: crate::bridge::events::EntityPose, time: f32) -> PlayerPose {
    use crate::bridge::events::EntityPose as P;
    match pose {
        P::Crouching => PlayerPose::Sneaking,
        P::FallFlying => PlayerPose::FallFlying,
        P::Swimming => PlayerPose::Swimming,
        P::SpinAttack => PlayerPose::SpinAttack(time * 20.0),
        P::Sleeping => PlayerPose::Sleeping,
        P::Sitting => PlayerPose::Sitting,
        P::Standing => PlayerPose::Standing,
    }
}

/// Boss-bar colour id → the sprite name vanilla uses for it.
fn boss_bar_color(id: u8) -> &'static str {
    match id {
        0 => "pink",
        1 => "blue",
        2 => "red",
        3 => "green",
        4 => "yellow",
        5 => "purple",
        _ => "white",
    }
}

/// Boss-bar overlay id → the notch sprite prefix, or `None` for a plain bar.
fn boss_bar_notches(id: u8) -> Option<&'static str> {
    match id {
        1 => Some("notched_6"),
        2 => Some("notched_10"),
        3 => Some("notched_12"),
        4 => Some("notched_20"),
        _ => None,
    }
}

/// A registry colour (0..255 per channel) as a linear 0..1 triple.
fn rgb_f32(c: [u8; 3]) -> [f32; 3] {
    [
        c[0] as f32 / 255.0,
        c[1] as f32 / 255.0,
        c[2] as f32 / 255.0,
    ]
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

/// One real waypoint style asset (`assets/minecraft/waypoint_style/*.json`):
/// which dot sprites to cycle through as distance grows, and where the near/
/// far thresholds sit. Hardcoded rather than read from the resource pack at
/// runtime — same convention as `PAINTINGS` above, a small fixed real list
/// rather than generic directory scanning this asset pack doesn't support.
struct WaypointStyleDef {
    near: f32,
    far: f32,
    sprites: &'static [&'static str],
}

/// Real vanilla ships exactly two waypoint styles. An unrecognized style id
/// (a resource pack could add more) falls back to `"default"`, matching
/// vanilla's own graceful behavior for a style with no client asset loaded.
const WAYPOINT_STYLES: &[(&str, WaypointStyleDef)] = &[
    (
        "default",
        WaypointStyleDef { near: 128.0, far: 332.0, sprites: &["default_0", "default_1", "default_2", "default_3"] },
    ),
    (
        "bowtie",
        WaypointStyleDef {
            near: 64.0,
            far: 332.0,
            sprites: &["bowtie", "default_0", "default_1", "default_2", "default_3"],
        },
    ),
];

fn waypoint_style(id: &str) -> &'static WaypointStyleDef {
    WAYPOINT_STYLES
        .iter()
        .find(|(name, _)| *name == id)
        .map(|(_, def)| def)
        .unwrap_or(&WAYPOINT_STYLES[0].1)
}

/// Real `WaypointStyle.sprite(distance)`: the nearest/farthest sprite below/
/// at their thresholds, otherwise a size chosen by how far between the two
/// this distance sits (real vanilla's `Mth.lerpInt`; this floors the same
/// fraction, which may pick a neighbouring frame right at a boundary distance
/// but always lands on a real sprite from the style's own list).
fn waypoint_sprite(style: &WaypointStyleDef, distance: f32) -> &'static str {
    let sprites = style.sprites;
    if distance < style.near {
        return sprites[0];
    }
    if distance >= style.far {
        return sprites[sprites.len() - 1];
    }
    if sprites.len() == 1 {
        return sprites[0];
    }
    if sprites.len() == 3 {
        return sprites[1];
    }
    let frac = (distance - style.near) / (style.far - style.near);
    let idx = 1 + (frac * (sprites.len() - 2) as f32).floor() as usize;
    sprites[idx.min(sprites.len() - 1)]
}

/// Real `Mth.wrapDegrees`: wraps to `(-180, 180]`.
fn wrap_degrees(deg: f32) -> f32 {
    let mut d = deg % 360.0;
    if d >= 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d
}

/// Real `Vec3iWaypoint`/`ChunkWaypoint.yawAngleToCamera`: the camera-relative
/// bearing to a world point, in degrees, wrapped to `(-180, 180]` — 0 means
/// dead ahead, positive is to the right (matching vanilla yaw convention).
fn waypoint_yaw_angle(cam: [f64; 3], cam_yaw: f32, target: [f64; 3]) -> f32 {
    let dx = cam[0] - target[0];
    let dz = cam[2] - target[2];
    // (cam - target).rotateClockwise90() = (-dz, _, dx); atan2(z, x) of that.
    let waypoint_angle = (dx as f32).atan2(-dz as f32).to_degrees();
    wrap_degrees(waypoint_angle - cam_yaw)
}

/// Real `Vec3iWaypoint.pitchDirectionToCamera`, generalized: vanilla projects
/// the exact point through the camera's view-projection matrix and checks the
/// resulting screen-space Y. This instead compares the *angular* elevation
/// difference between the camera's own pitch and the target against half the
/// vertical FOV — algebraically the same test for a point straight ahead, and
/// a close approximation elsewhere (this engine's HUD needs a hint arrow, not
/// a pixel-exact projection). For a `Chunk`/`Azimuth` waypoint (no real
/// elevation, always vanilla's `projectHorizonToScreen`), pass
/// `target_elevation_deg = 0.0` and this reduces to that exact formula.
fn pitch_direction(cam_pitch_deg: f32, fov_deg: f32, target_elevation_deg: f32) -> Option<bool> {
    let ndc_y = (cam_pitch_deg - target_elevation_deg).to_radians().tan()
        / (fov_deg * 0.5).to_radians().tan();
    if ndc_y < -1.0 {
        Some(true) // arrow down
    } else if ndc_y > 1.0 {
        Some(false) // arrow up
    } else {
        None
    }
}

/// Real `LocatorBarRenderer`'s `dotPosition`: an offset in GUI px from the
/// bar's own centre.
fn locator_dot_offset(angle_deg: f32) -> f32 {
    (angle_deg * 173.0 / 2.0 / 60.0).floor()
}

/// Java's `UUID.hashCode()`: XOR-fold the 64-bit halves, then XOR-fold that
/// into 32 bits. Needed to reproduce vanilla's un-tinted waypoint colour bit
/// for bit — that colour is genuinely just this hash's low 24 bits, brightened.
fn java_uuid_hash(uuid: u128) -> i32 {
    let msb = (uuid >> 64) as u64;
    let lsb = uuid as u64;
    let hilo = msb ^ lsb;
    ((hilo >> 32) ^ hilo) as u32 as i32
}

/// Java's `String.hashCode()`: `31*h + c` over UTF-16 code units.
fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for c in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(c as i32);
    }
    h
}

/// Real `ARGB.setBrightness`: RGB → HSB, force the brightness (V) channel,
/// HSB → RGB — vanilla's own algorithm (a standard HSV round-trip), used to
/// turn a raw colour into one that reads clearly against the dark locator bar.
fn set_brightness(rgb: [u8; 3], brightness: f32) -> [u8; 3] {
    let (r, g, b) = (rgb[0] as i32, rgb[1] as i32, rgb[2] as i32);
    let rgb_max = r.max(g).max(b);
    let rgb_min = r.min(g).min(b);
    let range = (rgb_max - rgb_min) as f32;
    let saturation = if rgb_max != 0 { range / rgb_max as f32 } else { 0.0 };
    if saturation == 0.0 {
        let v = (brightness * 255.0).round() as u8;
        return [v, v, v];
    }
    let cr = (rgb_max - r) as f32 / range;
    let cg = (rgb_max - g) as f32 / range;
    let cb = (rgb_max - b) as f32 / range;
    let mut hue = if r == rgb_max {
        cb - cg
    } else if g == rgb_max {
        2.0 + cr - cb
    } else {
        4.0 + cg - cr
    };
    hue /= 6.0;
    if hue < 0.0 {
        hue += 1.0;
    }
    let segment = (hue - hue.floor()) * 6.0;
    let offset = segment - segment.floor();
    let primary = brightness * (1.0 - saturation);
    let secondary = brightness * (1.0 - saturation * offset);
    let tertiary = brightness * (1.0 - saturation * (1.0 - offset));
    let (rf, gf, bf) = match segment as i32 {
        0 => (brightness, tertiary, primary),
        1 => (secondary, brightness, primary),
        2 => (primary, brightness, tertiary),
        3 => (primary, secondary, brightness),
        4 => (tertiary, primary, brightness),
        _ => (brightness, primary, secondary),
    };
    [(rf * 255.0).round() as u8, (gf * 255.0).round() as u8, (bf * 255.0).round() as u8]
}

/// Real vanilla's un-tinted waypoint colour: `ARGB.setBrightness(ARGB.color(255, hash), 0.9)`.
fn hashed_waypoint_color(id: &events::WaypointKey) -> [u8; 3] {
    let hash = match id {
        events::WaypointKey::Uuid(u) => java_uuid_hash(*u),
        events::WaypointKey::Name(s) => java_string_hash(s),
    };
    let rgb24 = hash as u32 & 0xFF_FFFF;
    let raw = [((rgb24 >> 16) & 0xFF) as u8, ((rgb24 >> 8) & 0xFF) as u8, (rgb24 & 0xFF) as u8];
    set_brightness(raw, 0.9)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The three climates that matter: a desert never rains, a taiga snows,
    /// a plain rains.
    #[test]
    fn a_biome_decides_its_own_weather() {
        assert_eq!(precipitation_kind(0.0, 2.0), None, "a desert stays dry");
        assert_eq!(
            precipitation_kind(0.4, 0.05),
            Some(true),
            "a snowy taiga snows"
        );
        assert_eq!(precipitation_kind(0.4, 0.8), Some(false), "plains rain");
        // Vanilla's threshold: 0.15 is still rain, anything under it is snow.
        assert_eq!(precipitation_kind(0.5, 0.15), Some(false));
        assert_eq!(precipitation_kind(0.5, 0.149), Some(true));
    }

    #[test]
    fn the_pose_of_an_animal_survives_the_trip_to_the_renderer() {
        use crate::bridge::events::AnimalPose;
        let p = |pose| mob_pose(pose, false, 0.0, 0.0, 0.0);
        assert_eq!(p(AnimalPose::Standing), MobPose::None);
        assert_eq!(p(AnimalPose::Sitting), MobPose::Sitting);
        assert_eq!(p(AnimalPose::Lying), MobPose::Lying);
        assert_eq!(p(AnimalPose::Rearing), MobPose::Rearing);
        assert_eq!(
            p(AnimalPose::Rowing {
                left: true,
                right: false
            }),
            MobPose::Rowing {
                left: true,
                right: false
            }
        );
        assert_eq!(
            mob_pose(AnimalPose::Dancing, true, 0.6, 0.0, 0.0),
            MobPose::Dancing { is_spinning: true, spin_progress: 0.6 }
        );
        assert_eq!(
            mob_pose(AnimalPose::Rolling, false, 0.0, 0.4, 0.0),
            MobPose::Rolling { amount: 0.4 }
        );
        assert_eq!(
            mob_pose(AnimalPose::OnBack, false, 0.0, 0.0, 0.8),
            MobPose::OnBack { amount: 0.8 }
        );
        assert_eq!(p(AnimalPose::Faceplanted), MobPose::Faceplanted);
    }

    #[test]
    fn pitch_clamps_to_just_under_vertical() {
        assert_eq!(clamp_pitch(120.0), 89.9);
        assert_eq!(clamp_pitch(-1000.0), -89.9);
        assert_eq!(clamp_pitch(15.5), 15.5);
    }

    #[test]
    fn rotate_offset_turns_with_the_model() {
        let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        let o = [10.0, 64.0, 20.0];
        // Facing south (yaw 0): the model's forward is world +Z.
        assert!(near(
            rotate_offset(o, [0.0, 0.0, 0.5], 0.0),
            [10.0, 64.0, 20.5]
        ));
        // Facing north (yaw 180): forward is world −Z.
        assert!(near(
            rotate_offset(o, [0.0, 0.0, 0.5], 180.0),
            [10.0, 64.0, 19.5]
        ));
        // Facing west (yaw 90): forward is world −X.
        assert!(near(
            rotate_offset(o, [0.0, 0.0, 0.5], 90.0),
            [9.5, 64.0, 20.0]
        ));
        // The vertical component never rotates.
        assert!(near(
            rotate_offset(o, [0.0, 1.5, 0.0], 123.0),
            [10.0, 65.5, 20.0]
        ));
    }

    #[test]
    fn boats_seat_two_riders_and_mobs_carry_them_on_top() {
        let front = seat_offset("oak_boat", 0.6, 0);
        let back = seat_offset("oak_boat", 0.6, 1);
        assert!(front[2] > back[2], "seat 0 sits ahead of seat 1");
        assert_eq!(seat_offset("minecart", 0.7, 0), [0.0, 0.0, 0.0]);
        // Riding a horse puts you above its back, not inside it.
        assert!(seat_offset("horse", 1.6, 0)[1] > 1.0);
    }

    #[test]
    fn death_animation_runs_for_one_second_then_holds() {
        let snap = EntitySnapshot {
            id: 1,
            kind: "zombie".into(),
            pos: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            width: 0.6,
            height: 1.8,
            name: None,
            name_spans: None,
            is_player: false,
            sneaking: false,
            pose: Default::default(),
            cape_url: None,
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
            painting: None,
            frame: None,
            display: None,
            armor_stand: None,
            on_fire: false,
            collar: None,
            powered: false,
            goat_left_horn: true,
            goat_right_horn: true,
            sneeze_head_pitch: None,
            item_count: 1,
            spawn_data: 0,
            sheared: false,
            pose_kind: Default::default(),
            health: None,
            max_health: None,
            cloud_radius: None,
            firework: Vec::new(),
            shoulders: [None; 2],
            arrows: 0,
            stingers: 0,
            swelling: false,
            charging: false,
            peek: 0,
            leashed_to: None,
            head_yaw: None,
            riding_on: None,
        };
        let now = Instant::now();
        let mut track = EntityTrack::new(snap, now);
        assert_eq!(track.death_progress(now), 0.0);
        track.death_start = Some(now);
        assert_eq!(track.death_progress(now), 0.0);
        let half = now + DEATH_ANIM / 2;
        assert!((track.death_progress(half) - 0.5).abs() < 1e-3);
        // Past the end it stays flat on the ground instead of spinning on.
        assert_eq!(track.death_progress(now + DEATH_ANIM * 3), 1.0);
    }

    #[test]
    fn armor_material_maps_items() {
        assert_eq!(
            armor_material("diamond_chestplate"),
            Some(ArmorMaterial::Diamond)
        );
        assert_eq!(armor_material("golden_boots"), Some(ArmorMaterial::Gold));
        assert_eq!(
            armor_material("netherite_helmet"),
            Some(ArmorMaterial::Netherite)
        );
        assert_eq!(
            armor_material("chainmail_leggings"),
            Some(ArmorMaterial::Chainmail)
        );
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
    fn boss_bar_sprites_cover_every_id() {
        // The seven vanilla colours, in protocol order.
        let names: Vec<&str> = (0..7).map(boss_bar_color).collect();
        assert_eq!(
            names,
            ["pink", "blue", "red", "green", "yellow", "purple", "white"]
        );
        // Anything out of range still names a real sprite rather than panicking.
        assert_eq!(boss_bar_color(200), "white");
        assert_eq!(boss_bar_notches(0), None);
        assert_eq!(boss_bar_notches(1), Some("notched_6"));
        assert_eq!(boss_bar_notches(4), Some("notched_20"));
        assert_eq!(boss_bar_notches(9), None);
    }

    #[test]
    fn animal_equipment_paths() {
        // A saddle names the species' own sheet...
        assert_eq!(
            animal_saddle_texture("pig", Some("saddle")).as_deref(),
            Some("entity/equipment/pig_saddle/saddle")
        );
        assert_eq!(
            animal_saddle_texture("camel", Some("saddle")).as_deref(),
            Some("entity/equipment/camel_saddle/saddle")
        );
        // ...and nothing at all when the slot is empty or the animal can't wear one.
        assert_eq!(animal_saddle_texture("pig", None), None);
        assert_eq!(animal_saddle_texture("cow", Some("saddle")), None);

        // Mounts of Mayhem: a camel husk rides its own sheet, and a zombie
        // nautilus saddles up on the tamed nautilus's own sheet.
        assert_eq!(
            animal_saddle_texture("camel_husk", Some("saddle")).as_deref(),
            Some("entity/equipment/camel_husk_saddle/saddle")
        );
        assert_eq!(
            animal_saddle_texture("nautilus", Some("saddle")).as_deref(),
            Some("entity/equipment/nautilus_saddle/saddle")
        );
        assert_eq!(
            animal_saddle_texture("zombie_nautilus", Some("saddle")).as_deref(),
            Some("entity/equipment/nautilus_saddle/saddle")
        );

        // Horse armour is named by its material, wolf armour is one texture.
        assert_eq!(
            animal_body_texture("horse", Some("diamond_horse_armor")).as_deref(),
            Some("entity/equipment/horse_body/diamond")
        );
        assert_eq!(
            animal_body_texture("wolf", Some("wolf_armor")).as_deref(),
            Some("entity/equipment/wolf_body/armadillo_scute")
        );
        // Nautilus armour follows the same by-material naming, worn by either
        // the tamed nautilus or its undead cousin.
        assert_eq!(
            animal_body_texture("nautilus", Some("copper_nautilus_armor")).as_deref(),
            Some("entity/equipment/nautilus_body/copper")
        );
        assert_eq!(
            animal_body_texture("zombie_nautilus", Some("netherite_nautilus_armor")).as_deref(),
            Some("entity/equipment/nautilus_body/netherite")
        );
        // A llama's carpet is deliberately not drawn (wrong model's UVs).
        assert_eq!(animal_body_texture("llama", Some("red_carpet")), None);
        // Junk in the slot never invents a path.
        assert_eq!(animal_body_texture("horse", Some("stone")), None);
    }

    #[test]
    fn cape_sheets_are_padded_square() {
        let wide = image::RgbaImage::new(64, 32);
        let out = pad_to_square(&wide);
        assert_eq!(out.dimensions(), (64, 64));
        // An already-square sheet is left alone.
        let square = image::RgbaImage::new(64, 64);
        assert_eq!(pad_to_square(&square).dimensions(), (64, 64));
    }

    #[test]
    fn poses_map_across_the_bridge() {
        use crate::bridge::events::EntityPose as P;
        assert_eq!(player_pose(P::Standing, 0.0), PlayerPose::Standing);
        assert_eq!(player_pose(P::Crouching, 0.0), PlayerPose::Sneaking);
        assert_eq!(player_pose(P::Swimming, 0.0), PlayerPose::Swimming);
        assert_eq!(player_pose(P::Sleeping, 0.0), PlayerPose::Sleeping);
        // The riptide spin carries an angle that moves with the clock.
        let a = player_pose(P::SpinAttack, 1.0);
        let b = player_pose(P::SpinAttack, 2.0);
        assert!(matches!(a, PlayerPose::SpinAttack(_)));
        assert_ne!(a, b);
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
            pose: Default::default(),
            cape_url: None,
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
            painting: None,
            frame: None,
            display: None,
            armor_stand: None,
            on_fire: false,
            collar: None,
            powered: false,
            goat_left_horn: true,
            goat_right_horn: true,
            sneeze_head_pitch: None,
            item_count: 1,
            spawn_data: 0,
            sheared: false,
            pose_kind: Default::default(),
            health: None,
            max_health: None,
            cloud_radius: None,
            firework: Vec::new(),
            shoulders: [None; 2],
            arrows: 0,
            stingers: 0,
            swelling: false,
            charging: false,
            peek: 0,
            leashed_to: None,
            head_yaw: None,
            riding_on: None,
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
            pose: Default::default(),
            cape_url: None,
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
            painting: None,
            frame: None,
            display: None,
            armor_stand: None,
            on_fire: false,
            collar: None,
            powered: false,
            goat_left_horn: true,
            goat_right_horn: true,
            sneeze_head_pitch: None,
            item_count: 1,
            spawn_data: 0,
            sheared: false,
            pose_kind: Default::default(),
            health: None,
            max_health: None,
            cloud_radius: None,
            firework: Vec::new(),
            shoulders: [None; 2],
            arrows: 0,
            stingers: 0,
            swelling: false,
            charging: false,
            peek: 0,
            leashed_to: None,
            head_yaw: None,
            riding_on: None,
        };
        let t0 = Instant::now();
        let mut track = EntityTrack::new(snap(0.0), t0);
        track.push(snap(100.0), t0 + Duration::from_millis(50));
        // No gliding across 100 blocks: history restarts at the new spot.
        assert_eq!(track.sample(t0 + Duration::from_millis(25)).0[0], 100.0);
    }

    #[test]
    fn shadow_radius_matches_vanilla_shapes() {
        // Vanilla passes 0.5 for players, 0.7 for pigs/cows, 0.3 for chickens.
        assert!((shadow_radius("player", 0.6) - 0.45).abs() < 0.06);
        assert!((shadow_radius("pig", 0.9) - 0.7).abs() < 0.03);
        assert!((shadow_radius("chicken", 0.4) - 0.3).abs() < 0.01);
        // Flat wall entities and projectiles cast none.
        for kind in [
            "painting",
            "item_frame",
            "arrow",
            "text_display",
            "end_crystal",
        ] {
            assert_eq!(
                shadow_radius(kind, 1.0),
                0.0,
                "{kind} should have no shadow"
            );
        }
    }

    #[test]
    fn shadow_patches_cover_the_square_and_stay_in_uv_range() {
        // Flat ground at y = 64: the block below every column is full.
        let patches = shadow_patches_with([8.3, 64.0, 8.7], 0.5, |_, y, _| y == 63);
        assert!(!patches.is_empty());
        let mut area = 0.0f32;
        for [x0, z0, x1, z1, dy] in &patches {
            assert!(x1 > x0 && z1 > z0);
            // Clipped to the shadow square, so the blob's UVs stay inside 0..1.
            for v in [x0, x1, z0, z1] {
                assert!(
                    v.abs() <= 0.5 + 1e-4,
                    "patch reaches outside the square: {v}"
                );
            }
            assert!(
                (*dy - 0.015).abs() < 1e-4,
                "shadow should sit on the surface"
            );
            area += (x1 - x0) * (z1 - z0);
        }
        // The patches tile the whole 1x1 footprint of the shadow square.
        assert!(
            (area - 1.0).abs() < 1e-3,
            "patches cover {area} instead of 1.0"
        );
    }

    #[test]
    fn shadow_needs_ground_within_its_radius() {
        // Standing on the ground: a shadow. One block up: still inside 0.5 blocks
        // of the surface at y=64, so vanilla keeps it...
        assert!(!shadow_patches_with([0.5, 64.0, 0.5], 0.5, |_, y, _| y == 63).is_empty());
        // ...but jump well clear and it is gone, like vanilla.
        assert!(shadow_patches_with([0.5, 66.0, 0.5], 0.5, |_, y, _| y == 63).is_empty());
    }

    #[test]
    fn dropped_stacks_grow_with_their_count() {
        assert_eq!(render_amount(1), 1);
        assert_eq!(render_amount(2), 2);
        assert_eq!(render_amount(16), 2);
        assert_eq!(render_amount(17), 3);
        assert_eq!(render_amount(33), 4);
        assert_eq!(render_amount(64), 5);
        // The first copy is always dead centre; the rest scatter deterministically.
        assert_eq!(stack_offset(7, 0, true), (0.0, 0.0, 0.0));
        assert_eq!(stack_offset(7, 2, true), stack_offset(7, 2, true));
        let (dx, dy, dz) = stack_offset(7, 3, true);
        for d in [dx, dy, dz] {
            assert!(d.abs() <= 0.15, "block copy strays too far: {d}");
        }
        // Flat sprites only spread in x/y, at half the distance.
        let (fx, fy, fz) = stack_offset(7, 3, false);
        assert_eq!(fz, 0.0);
        assert!(fx.abs() <= 0.075 && fy.abs() <= 0.075);
    }

    #[test]
    fn beacon_beam_tint_comes_from_stained_glass() {
        assert_eq!(stained_glass_dye("white_stained_glass"), Some(0));
        assert_eq!(stained_glass_dye("light_blue_stained_glass"), Some(3));
        assert_eq!(stained_glass_dye("black_stained_glass_pane"), Some(15));
        assert_eq!(stained_glass_dye("glass"), None);
        assert_eq!(stained_glass_dye("stone"), None);
        // Every base beacon block is a real vanilla block name.
        assert!(BEACON_BASE.iter().all(|b| b.ends_with("_block")));
    }

    #[test]
    fn wrap_degrees_stays_in_range() {
        assert_eq!(wrap_degrees(0.0), 0.0);
        assert_eq!(wrap_degrees(350.0), -10.0);
        assert_eq!(wrap_degrees(-350.0), 10.0);
        assert_eq!(wrap_degrees(180.0), -180.0);
        assert_eq!(wrap_degrees(-180.0), -180.0);
    }

    /// A waypoint straight ahead of the camera reads angle 0 regardless of
    /// which way the camera actually faces.
    #[test]
    fn a_waypoint_dead_ahead_reads_zero() {
        // Facing south (yaw 0), a target further south is dead ahead.
        assert!(waypoint_yaw_angle([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 10.0]).abs() < 1e-3);
        // Facing east (yaw -90 / 270), a target further east is dead ahead.
        assert!(waypoint_yaw_angle([0.0, 0.0, 0.0], -90.0, [10.0, 0.0, 0.0]).abs() < 1e-3);
    }

    /// Transcription check against a hand-computed value from the same real
    /// formula (`atan2(dx, -dz)` then wrapped against camera yaw) — verifies
    /// the Rust code matches the decompiled Java arithmetic, not a re-derived
    /// notion of "which side should be positive".
    #[test]
    fn waypoint_yaw_angle_matches_the_real_formula() {
        let angle = waypoint_yaw_angle([0.0, 0.0, 0.0], 0.0, [10.0, 0.0, 0.0]);
        // dx = 0-10 = -10, dz = 0-0 = 0 -> atan2(-10, 0) = -90
        assert!((angle - (-90.0)).abs() < 1e-3, "got {angle}");
    }

    #[test]
    fn pitch_direction_is_none_when_level_with_the_target() {
        assert_eq!(pitch_direction(0.0, 70.0, 0.0), None);
    }

    #[test]
    fn pitch_direction_flags_steep_look_up_and_down() {
        // Looking far down at something level with the camera: the target
        // sits high on screen -> UP arrow (matches vanilla's own polarity:
        // pitching toward -90, i.e. looking up, drives the horizon to
        // NEGATIVE_INFINITY -> DOWN, so pitching toward +90 must be the
        // opposite).
        assert_eq!(pitch_direction(89.0, 70.0, 0.0), Some(false));
        assert_eq!(pitch_direction(-89.0, 70.0, 0.0), Some(true));
    }

    #[test]
    fn locator_dot_offset_scales_and_floors() {
        assert_eq!(locator_dot_offset(0.0), 0.0);
        // 60 * 173/2/60 = 86.5 -> floors to 86.
        assert_eq!(locator_dot_offset(60.0), 86.0);
        assert_eq!(locator_dot_offset(-60.0), -87.0);
    }

    #[test]
    fn waypoint_sprite_picks_near_far_and_middle() {
        let style = waypoint_style("default");
        assert_eq!(waypoint_sprite(style, 0.0), "default_0");
        assert_eq!(waypoint_sprite(style, 1000.0), "default_3");
        // Somewhere in the middle picks a middle frame, not either extreme.
        let mid = waypoint_sprite(style, 200.0);
        assert!(mid == "default_1" || mid == "default_2", "got {mid}");
    }

    #[test]
    fn unknown_waypoint_style_falls_back_to_default() {
        let fallback = waypoint_style("something_a_resource_pack_added");
        let default = waypoint_style("default");
        assert_eq!(fallback.sprites, default.sprites);
        assert_eq!((fallback.near, fallback.far), (default.near, default.far));
    }

    /// Known Java `String.hashCode()` values — "" and "ab" are commonly-cited
    /// reference vectors for this exact algorithm.
    #[test]
    fn java_string_hash_matches_known_vectors() {
        assert_eq!(java_string_hash(""), 0);
        assert_eq!(java_string_hash("a"), 97);
        assert_eq!(java_string_hash("ab"), 3105);
    }

    #[test]
    fn java_uuid_hash_matches_hand_computed_cases() {
        assert_eq!(java_uuid_hash(0), 0);
        // msb=0, lsb=1 -> hilo=1 -> (1>>32)^1 = 0^1 = 1.
        assert_eq!(java_uuid_hash(1), 1);
    }

    #[test]
    fn set_brightness_of_grey_is_flat() {
        assert_eq!(set_brightness([0, 0, 0], 0.9), [230, 230, 230]);
    }

    #[test]
    fn set_brightness_keeps_pure_red_pure() {
        assert_eq!(set_brightness([255, 0, 0], 1.0), [255, 0, 0]);
    }

    /// An un-tinted waypoint's colour is fully determined by its identifier —
    /// same id, same colour, every time.
    #[test]
    fn hashed_waypoint_color_is_deterministic() {
        let id = events::WaypointKey::Name("spawn".to_string());
        assert_eq!(hashed_waypoint_color(&id), hashed_waypoint_color(&id));
    }

    // -- Cape physics (real `ClientAvatarState`/`AvatarRenderer` port) -------

    #[test]
    fn cape_lag_converges_toward_a_steady_walk_at_the_real_rate() {
        let now = Instant::now();
        let mut cape = CapeLag::at([0.0, 0.0, 0.0], now);
        // A steady 0.2 blocks/tick walk in +x: each tick the lag should close
        // exactly a quarter of that tick's own gap (real `xCloak += dx*0.25`).
        let mut pos = [0.0, 0.0, 0.0];
        for _ in 0..5 {
            pos[0] += 0.2;
            cape.tick(pos, now);
        }
        // After 5 ticks the lag has NOT caught up to the walker (it's a
        // continuous lag-follow, never equal while still moving).
        assert!(cape.lag[0] < pos[0]);
        assert!(cape.lag[0] > 0.0);
    }

    #[test]
    fn cape_lag_snaps_per_axis_past_a_ten_block_jump_not_by_euclidean_distance() {
        let now = Instant::now();
        // A diagonal jump whose combined distance exceeds 10 but whose own
        // per-axis deltas do not (real vanilla checks x/y/z independently,
        // not `dx*dx+dy*dy+dz*dz > 100`).
        let mut cape = CapeLag::at([0.0, 0.0, 0.0], now);
        cape.tick([7.5, 0.0, 7.5], now);
        // Neither axis alone crossed ±10, so both still lag-follow (0.25 of
        // the delta), not snap outright.
        assert!((cape.lag[0] - 7.5 * 0.25).abs() < 1e-9);
        assert!((cape.lag[2] - 7.5 * 0.25).abs() < 1e-9);

        // Now a real single-axis teleport: x jumps by exactly 11.
        let mut cape2 = CapeLag::at([0.0, 0.0, 0.0], now);
        cape2.tick([11.0, 0.0, 0.0], now);
        assert_eq!(cape2.lag[0], 11.0, "a >10 per-axis delta snaps outright");
        assert_eq!(cape2.lag_prev[0], 11.0, "the snap has no glide, old==new");
    }

    #[test]
    fn cape_flap_lean_is_zero_for_a_stationary_player() {
        let now = Instant::now();
        let cape = CapeLag::at([0.0, 0.0, 0.0], now);
        let (flap, lean, lean2) = cape_flap_lean(&cape, now, 0.0, 0.0);
        assert_eq!((flap, lean, lean2), (0.0, 0.0, 0.0));
    }

    #[test]
    fn cape_flap_lean_leans_forward_when_the_body_outruns_the_cape() {
        let now = Instant::now();
        // The cape is still at the origin; the body has already moved 1
        // block in +z while facing yaw=0 (real vanilla: yaw 0 south/+z), so
        // the cape should show a forward lean (it's dragging behind).
        let mut cape = CapeLag::at([0.0, 0.0, 0.0], now);
        cape.actual = [0.0, 0.0, 1.0];
        // Query well past this tick so `frac` clamps to 1.0 and the
        // just-set `actual`/`lag` (not their `_prev` counterparts) are used.
        let query_t = now + Duration::from_millis(100);
        let (_, lean, lean2) = cape_flap_lean(&cape, query_t, 0.0, 0.0);
        assert!(lean > 0.0, "cape should lean forward, got {lean}");
        assert!((lean2).abs() < 1e-6, "no sideways component for a pure +z move");
    }

    // -- World border wall (real `WorldBorderRenderer`/`BorderStatus` ports) -

    #[test]
    fn border_wall_alpha_matches_the_real_vanilla_fade() {
        // Right at the edge of visibility (distance == renderDistance): the
        // term is 0, so the wall is fully invisible.
        assert_eq!(border_wall_alpha(64.0, 64.0), 0.0);
        // Standing right at the wall (distance == 0): fully opaque.
        assert_eq!(border_wall_alpha(0.0, 64.0), 1.0);
        // Already past the wall (negative distance): still fully opaque,
        // never goes translucent again on the far side.
        assert_eq!(border_wall_alpha(-10.0, 64.0), 1.0);
        // Halfway to the wall: `(1 - 0.5)^4 = 0.0625`, not a linear 0.5 fade.
        assert!((border_wall_alpha(32.0, 64.0) - 0.0625).abs() < 1e-6);
        // Monotonic: closer to the wall never fades the wall out further.
        assert!(border_wall_alpha(16.0, 64.0) > border_wall_alpha(48.0, 64.0));
    }

    #[test]
    fn border_status_color_matches_the_real_vanilla_enum() {
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        assert!(close(
            border_status_color(true, false),
            [64.0 / 255.0, 255.0 / 255.0, 128.0 / 255.0]
        ));
        assert!(close(
            border_status_color(false, true),
            [255.0 / 255.0, 48.0 / 255.0, 48.0 / 255.0]
        ));
        assert!(close(
            border_status_color(false, false),
            [32.0 / 255.0, 160.0 / 255.0, 255.0 / 255.0]
        ));
    }

    #[test]
    fn border_interpolated_size_treats_lerp_time_as_ticks_not_millis() {
        // Regression test for a real pre-existing bug: `lerp_time` off the
        // wire is a raw tick count (`WorldBorder.getLerpTime()`), not
        // milliseconds — a 600-tick (30s) move must still be in progress
        // after 1000ms elapsed, not already finished (1000 > 600 would have
        // clamped `t` to 1.0 under the old, wrong, ms-per-tick-less formula).
        let mid = border_interpolated_size(0.0, 1200.0, 600, 1000.0);
        assert!(mid > 0.0 && mid < 1200.0, "expected still mid-move, got {mid}");
        // At exactly `lerp_time_ticks * 50` ms, the move is exactly done.
        assert_eq!(border_interpolated_size(0.0, 1200.0, 600, 30_000.0), 1200.0);
        // Long past done: stays at the target, never overshoots.
        assert_eq!(border_interpolated_size(0.0, 1200.0, 600, 999_999.0), 1200.0);
        // Static border (lerp_time 0): always at the target size immediately.
        assert_eq!(border_interpolated_size(500.0, 500.0, 0, 12_345.0), 500.0);
    }

    #[test]
    fn border_lerp_speed_is_blocks_per_tick() {
        assert_eq!(border_lerp_speed(0.0, 1200.0, 600), 2.0);
        assert_eq!(border_lerp_speed(1200.0, 0.0, 600), 2.0); // direction-agnostic
        assert_eq!(border_lerp_speed(0.0, 1200.0, 0), 0.0); // static: no speed
    }

    #[test]
    fn border_warning_distance_matches_the_real_vanilla_formula() {
        // Static border: falls back to the plain `warning_blocks` radius,
        // matching this client's pre-fix behaviour exactly (regression-safe).
        assert_eq!(border_warning_distance(5, 0.0, 15, 1000.0, 1000.0), 5.0);
        // Fast-moving border: the moving-blocks term can dominate.
        // speed=2 blocks/tick * warning_time=15 = 30, vs a plain radius of 5.
        assert_eq!(border_warning_distance(5, 2.0, 15, 1000.0, 940.0), 30.0);
        // But it's capped at the actual remaining distance to the target —
        // a border 10 blocks from finishing its move can't warn 30 blocks out.
        assert_eq!(border_warning_distance(5, 2.0, 15, 1000.0, 990.0), 10.0);
    }

    #[test]
    fn border_warning_static_case_matches_the_pre_fix_plain_formula() {
        // With lerp_speed=0 (static), border_warning_distance always returns
        // plain warning_blocks — the exact behaviour this client already had
        // before this release, for any static border regardless of size.
        for size in [100.0, 5_000.0, 59_999_968.0] {
            assert_eq!(border_warning_distance(5, 0.0, 15, size, size), 5.0);
        }
    }

    // -- Allay/panda animation timers (real `Allay.tick`/`Panda.tick` ports) -

    fn pose_kind_snap(pose_kind: crate::bridge::events::AnimalPose) -> EntitySnapshot {
        EntitySnapshot {
            id: 1,
            kind: "test".into(),
            pos: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            width: 0.6,
            height: 1.8,
            name: None,
            name_spans: None,
            is_player: false,
            sneaking: false,
            pose: Default::default(),
            cape_url: None,
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
            painting: None,
            frame: None,
            display: None,
            armor_stand: None,
            on_fire: false,
            collar: None,
            powered: false,
            goat_left_horn: true,
            goat_right_horn: true,
            sneeze_head_pitch: None,
            item_count: 1,
            spawn_data: 0,
            sheared: false,
            pose_kind,
            health: None,
            max_health: None,
            cloud_radius: None,
            firework: Vec::new(),
            shoulders: [None; 2],
            arrows: 0,
            stingers: 0,
            swelling: false,
            charging: false,
            peek: 0,
            leashed_to: None,
            head_yaw: None,
            riding_on: None,
        }
    }

    fn dancing_snap(dancing: bool) -> EntitySnapshot {
        pose_kind_snap(if dancing {
            crate::bridge::events::AnimalPose::Dancing
        } else {
            crate::bridge::events::AnimalPose::Standing
        })
    }

    #[test]
    fn allay_dance_ticks_reset_the_instant_dancing_stops() {
        let now = Instant::now();
        let mut t = EntityTrack::new(dancing_snap(true), now);
        for _ in 0..10 {
            t.push(dancing_snap(true), now);
        }
        assert_eq!(t.dance_ticks, 10.0);
        assert!(t.spin_ticks > 0.0, "the first third of the cycle is spinning");
        t.push(dancing_snap(false), now);
        assert_eq!((t.dance_ticks, t.spin_ticks, t.spin_ticks_prev), (0.0, 0.0, 0.0));
    }

    #[test]
    fn allay_spin_ticks_never_leave_the_real_zero_to_fifteen_window() {
        let now = Instant::now();
        let mut t = EntityTrack::new(dancing_snap(true), now);
        // Run several full 55-tick dance cycles — real vanilla's own
        // `Mth.clamp` keeps `spinningAnimationTicks` in [0, 15] the whole
        // time, briefly touching 15 once near the end of each cycle's
        // 15-tick spinning window (dance_ticks % 55 < 15) before easing back
        // down through the 40-tick non-spinning remainder.
        let mut saw_the_ceiling = false;
        for _ in 0..220 {
            t.push(dancing_snap(true), now);
            assert!((0.0..=15.0).contains(&t.spin_ticks));
            if t.spin_ticks == 15.0 {
                saw_the_ceiling = true;
            }
        }
        assert!(saw_the_ceiling, "a long enough dance should reach the real 15-tick ceiling");
    }

    fn panda_snap(pose: crate::bridge::events::AnimalPose) -> EntitySnapshot {
        pose_kind_snap(pose)
    }

    #[test]
    fn panda_roll_amount_eases_in_and_out_at_the_real_rate() {
        use crate::bridge::events::AnimalPose;
        let now = Instant::now();
        let mut t = EntityTrack::new(panda_snap(AnimalPose::Rolling), now);
        t.push(panda_snap(AnimalPose::Rolling), now);
        assert!((t.roll_amount - 0.15).abs() < 1e-6);
        t.push(panda_snap(AnimalPose::Rolling), now);
        assert!((t.roll_amount - 0.30).abs() < 1e-6);
        t.push(panda_snap(AnimalPose::Standing), now);
        assert!((t.roll_amount - (0.30 - 0.19)).abs() < 1e-6, "eases back out at its own rate");
        // Never dips below 0 even after many more idle ticks.
        for _ in 0..10 {
            t.push(panda_snap(AnimalPose::Standing), now);
        }
        assert_eq!(t.roll_amount, 0.0);
    }

    #[test]
    fn panda_on_back_amount_is_independent_of_rolling() {
        use crate::bridge::events::AnimalPose;
        let now = Instant::now();
        let mut t = EntityTrack::new(panda_snap(AnimalPose::OnBack), now);
        t.push(panda_snap(AnimalPose::OnBack), now);
        assert!((t.on_back_amount - 0.15).abs() < 1e-6);
        assert_eq!(t.roll_amount, 0.0, "rolling never engaged, so its amount stays put");
        // Never climbs above 1 even after many more ticks.
        for _ in 0..10 {
            t.push(panda_snap(AnimalPose::OnBack), now);
        }
        assert_eq!(t.on_back_amount, 1.0);
    }
}
