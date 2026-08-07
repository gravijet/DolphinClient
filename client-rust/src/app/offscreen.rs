//! Headless test mode (works with lavapipe software Vulkan — no window/display):
//! 1. bake assets → renderer (RenderTarget::Offscreen 1280×720)
//! 2. spawn bridge, wait for Connected (timeout 60 s → error)
//! 3. pump events/meshing until ≥ `wait_sections` sections are meshed AND the
//!    mesh queue drains (timeout 120 s)
//! 4. render `frames` frames — camera at player eye pos, yaw sweeps 360°/frames
//!    (pitch 10° down) — writing out/frame_NN.png each time
//! 5. sanity: fail if every pixel of the last frame is one color
//! 6. send one Chat command ("DolphinClient rust ok"), tick once, disconnect.
//!
//! Return Ok(()) only if all steps pass — this is the CI smoke test.

use super::AppOptions;
use super::hud::{Hud, HudState};
use crate::assets::AssetPack;
use crate::assets::blockmap::BlockTable;
use crate::assets::items::ItemIcons;
use crate::bridge::events::{
    ChatSpan, Command, GameEvent, ItemSnapshot, PlayerSnapshot, ScoreLine,
};
use crate::bridge::spawn_bridge;
use crate::models::BakedModelStore;
use crate::render::{EguiFrame, RenderTarget, Renderer, SceneParams};
use crate::types::{MeshData, SectionPos};
use crate::world::WorldMirror;
use crate::world::mesher::mesh_section;
use anyhow::{Context, Result, bail};
use crossbeam_channel::RecvTimeoutError;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;

#[derive(Clone, Debug)]
pub struct OffscreenOptions {
    pub app: AppOptions,
    pub out_dir: PathBuf,
    pub frames: u32,
    /// Meshed-section threshold to consider the world "arrived".
    pub wait_sections: usize,
    /// Chat/server commands sent once after Connected (e.g. "/setblock ...");
    /// the world-ready wait then allows 3 s for the resulting block updates.
    pub exec: Vec<String>,
    /// Draw the egui HUD (crosshair, hotbar with item icons, chat) into the
    /// frames — used to verify the in-game HUD headlessly.
    pub hud_demo: bool,
}

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const MESH_TIMEOUT: Duration = Duration::from_secs(120);

/// Headless screenshot of the menus (title / multiplayer / options / pause) —
/// no server, no window. Renders the sky backdrop + egui menu and writes one
/// PNG per screen into `out_dir`. Used to eyeball the Minecraft-style UI.
pub fn dump_menu(app: AppOptions, out_dir: PathBuf) -> Result<()> {
    use super::hud::Hud;

    let mut pack = AssetPack::open(&app.mc_jar)?;
    let table = BlockTable::load_or_embedded(app.blocks_report.as_deref())
        .context("loading block table")?;
    let (store, mut atlas) =
        BakedModelStore::bake_all(&mut pack, &table).context("baking models")?;
    let item_icons = ItemIcons::bake(&mut pack, &table, &store, &atlas);

    let mut renderer = Renderer::new(RenderTarget::Offscreen { width: WIDTH, height: HEIGHT })
        .context("creating offscreen renderer")?;
    renderer.set_atlas(&atlas);
    if !item_icons.is_empty() {
        renderer.ensure_item_atlas(&item_icons.image);
    }
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("creating {}", out_dir.display()))?;

    // Title-screen panorama, if the asset store has it — verifies the panorama
    // pipeline behind the menus.
    let has_panorama =
        match super::load_panorama(app.assets_dir.as_deref(), app.asset_index.as_deref()) {
            Some(faces) => {
                info!(
                    dims = format!("{}x{}", faces[0].width(), faces[0].height()),
                    "panorama: loaded 6 faces"
                );
                renderer.set_panorama(&faces);
                true
            }
            None => {
                info!(
                    assets = ?app.assets_dir,
                    index = ?app.asset_index,
                    "panorama: not available"
                );
                false
            }
        };

    // The panorama behind the title; a plain sky behind the other screens.
    let scene = SceneParams {
        cam_pos: [8.0, 80.0, 8.0],
        yaw: 30.0,
        pitch: 8.0,
        fov_deg: 85.0,
        daylight: 0.9,
        fog_start: 96.0,
        fog_end: 192.0,
        sky_color: [0.47, 0.65, 1.0],
        panorama: has_panorama,
        outline: Vec::new(),
        crack: None,
        view_model: None,
        sky: None,
        lightmap: Default::default(),
        end_sky: false,
    };

    let ctx = egui::Context::default();
    // Headless RawInput has no clock, so egui's Area fade-in animation would be
    // stuck at 0 opacity (transparent). Disable animations so a single render
    // shows the menu at full opacity, exactly as the live app does after its
    // first few frames.
    ctx.all_styles_mut(|s| s.animation_time = 0.0);
    let mcui = super::mcui::McUi::load(
        &mut pack,
        &ctx,
        app.assets_dir.as_deref(),
        app.asset_index.as_deref(),
    )
    .context("loading vanilla GUI assets")?;
    let lang = crate::assets::Lang::load(
        &mut pack,
        app.assets_dir.as_deref(),
        app.asset_index.as_deref(),
        "en_us",
    );
    let mut skins = super::skins::SkinManager::new(app.assets_dir.as_deref());
    // The item-icon atlas as an egui texture, so the in-game HUD shot draws real
    // hotbar icons (and the enchantment glint composited over them).
    let item_icons = Arc::new(item_icons);
    let icon_handle = (!item_icons.is_empty()).then(|| {
        let img = &item_icons.image;
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [img.width() as usize, img.height() as usize],
            img.as_raw(),
        );
        ctx.load_texture("item-icons", color, egui::TextureOptions::NEAREST)
    });
    let icon_tex = icon_handle.as_ref().map(|h| (h.id(), item_icons.clone()));
    // (name, screen index, in-game pause menu?, pause sub-screen)
    // "ingame" is special: connected with no menu open, so the live HUD
    // (hotbar, status bars, scoreboard sidebar) renders for verification.
    let shots: [(&str, u8, bool, u8); 7] = [
        ("title", 0, false, 0),
        ("multiplayer", 1, false, 0),
        ("options", 2, false, 0),
        ("pause", 0, true, 0),
        ("advancements", 0, true, 1),
        ("statistics", 0, true, 2),
        ("ingame", 0, false, 0),
    ];
    for (name, screen, pause, sub) in shots {
        let ingame = name == "ingame";
        let mut hud = Hud::new(String::new(), true, "Dolphin".into());
        hud.debug_force(screen, pause);
        if sub != 0 {
            hud.debug_pause_sub(sub);
        }
        let mut settings = crate::settings::GameSettings::default();
        let sb_row = |t: &str, sc: i32, hide: bool| ScoreLine {
            text: vec![ChatSpan::plain(t)],
            score: sc,
            hide_number: hide,
        };
        // A hotbar with real icons: two of them enchanted (scrolling glint) and
        // two worn (vanilla durability bar), so the shot proves both.
        let stack = |name: &str, count: u32, enchanted: bool, damage: u32, max: u32| {
            Some(ItemSnapshot {
                item: name.into(),
                count,
                enchanted,
                damage,
                max_damage: max,
                ..Default::default()
            })
        };
        let state = HudState {
            connected: pause || ingame,
            menu_time: 0.6,
            // Two boss bars on the in-game shot: a plain purple dragon bar and
            // a notched red one, so both sprite families get eyeballed.
            boss_bars: if ingame {
                vec![
                    crate::app::hud::BossBarHud {
                        name: vec![ChatSpan::plain("Ender Dragon")],
                        progress: 0.72,
                        color: "purple",
                        notches: None,
                    },
                    crate::app::hud::BossBarHud {
                        name: vec![ChatSpan::plain("Raid")],
                        progress: 0.35,
                        color: "red",
                        notches: Some("notched_10"),
                    },
                ]
            } else {
                Vec::new()
            },
            icons: icon_tex.clone(),
            hotbar: vec![
                stack("diamond_sword", 1, true, 900, 1561),
                stack("diamond_pickaxe", 1, true, 0, 1561),
                stack("iron_axe", 1, false, 120, 250),
                stack("bow", 1, true, 0, 384),
                stack("cooked_beef", 32, false, 0, 0),
                stack("golden_apple", 3, false, 0, 0),
                stack("oak_planks", 64, false, 0, 0),
                stack("torch", 17, false, 0, 0),
                stack("enchanted_book", 1, true, 0, 0),
            ],
            health: 16.0,
            food: 18,
            xp_level: 7,
            pos: [128.5, 64.0, -240.5],
            entities_count: 12,
            fps: 244.0,
            render_distance: 12,
            session_secs: 372.0,
            sidebar_title: if ingame {
                vec![ChatSpan::plain("DolphinClient")]
            } else {
                vec![]
            },
            sidebar_lines: if ingame {
                vec![
                    sb_row("Kills:", 12, false),
                    sb_row("Deaths:", 3, false),
                    sb_row("Rank: MVP+", 0, true),
                    sb_row("Map: Skywars", 0, true),
                ]
            } else {
                vec![]
            },
            ..Default::default()
        };
        // egui anchors an Area from its previous-frame size, so a single pass
        // renders the title but not yet the button column. Render two full
        // frames (warm-up + capture) so the second knows the layout — the real
        // app renders continuously and never sees this one-frame lag.
        for _ in 0..2 {
            ctx.set_pixels_per_point(1.0);
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(WIDTH as f32, HEIGHT as f32),
                )),
                ..Default::default()
            };
            ctx.begin_pass(raw);
            let _ = hud.run(&ctx, &mcui, &state, &mut settings, &mut skins, &lang);
            let output = ctx.end_pass();
            let egui_frame = EguiFrame {
                textures_delta: output.textures_delta,
                primitives: ctx.tessellate(output.shapes, output.pixels_per_point),
                pixels_per_point: output.pixels_per_point,
            };
            renderer
                .frame(&scene, &[], Some(egui_frame))
                .with_context(|| format!("rendering menu {name}"))?;
        }
        let img = renderer
            .read_screenshot()
            .with_context(|| format!("reading back menu {name}"))?;
        let path = out_dir.join(format!("menu_{name}.png"));
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(screen = name, path = %path.display(), "menu shot written");
    }

    // Skin pipeline check: a Steve model in front of the panorama.
    if let Ok(steve) = pack.texture_png_raw("entity/player/wide/steve") {
        use crate::render::{ArmorMaterial, EntityDraw, EntityDrawKind};
        renderer.ensure_skin(0, &super::skins::normalize_skin(steve));
        // Upload armor textures so the armored test models render.
        for mat in ArmorMaterial::all() {
            let n = mat.tex_name();
            if let Ok(img) = pack.texture_png(&format!("entity/equipment/humanoid/{n}")) {
                renderer.ensure_armor(mat, false, &img);
            }
            if let Ok(img) = pack.texture_png(&format!("entity/equipment/humanoid_leggings/{n}")) {
                renderer.ensure_armor(mat, true, &img);
            }
        }
        let scene = SceneParams {
            cam_pos: [0.0, 65.6, 0.0],
            yaw: 0.0, // look +z (vanilla south)
            pitch: 3.0,
            fov_deg: 70.0,
            daylight: 1.0,
            fog_start: 90.0,
            fog_end: 192.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        // Two players 3 blocks ahead: one facing the camera, one turned, mid-step.
        let players = [
            EntityDraw {
                pos: [-0.6, 64.0, 3.0],
                yaw: 180.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Player {
                    skin: 0,
                    slim: false,
                    swing: 0.6,
                    attack_swing: 0.0,
                    pose: crate::render::PlayerPose::Standing,
                    skin_layers: 0xFF,
                    head_pitch: 0.0,
                    head_yaw: 0.0,
                    // Full diamond armor to eyeball all four layers.
                    armor: [
                        Some(ArmorMaterial::Diamond),
                        Some(ArmorMaterial::Diamond),
                        Some(ArmorMaterial::Diamond),
                        Some(ArmorMaterial::Diamond),
                    ],
                    main_hand: item_icons.uv("diamond_sword"),
                    off_hand: item_icons.uv("shield"),
                    cape: 0,
                    elytra: 0,
                },
            },
            EntityDraw {
                pos: [0.7, 64.0, 3.2],
                yaw: 150.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Player {
                    skin: 0,
                    slim: true,
                    swing: -0.4,
                    attack_swing: 0.8,
                    pose: crate::render::PlayerPose::Sneaking,
                    skin_layers: 0xFF,
                    head_pitch: 10.0,
                    head_yaw: 0.0,
                    // Iron helmet + chestplate only (partial armor).
                    armor: [
                        Some(ArmorMaterial::Iron),
                        Some(ArmorMaterial::Iron),
                        None,
                        None,
                    ],
                    main_hand: item_icons.uv("bow"),
                    off_hand: None,
                    cape: 0,
                    elytra: 0,
                },
            },
        ];
        renderer.frame(&scene, &players, None).context("rendering skin check")?;
        let img = renderer.read_screenshot().context("reading back skin check")?;
        let path = out_dir.join("menu_skins.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "skin check written");
    }

    // Mob-model check: the textured non-humanoid models in a row, so their
    // texture mapping and proportions can be eyeballed headlessly.
    {
        use crate::render::{EntityDraw, EntityDrawKind, MobModel};
        let mobs: &[(&str, MobModel)] = &[
            ("entity/creeper/creeper", MobModel::Creeper),
            ("entity/pig/pig_temperate", MobModel::Pig),
            ("entity/slime/slime", MobModel::Slime),
            ("entity/chicken/chicken_temperate", MobModel::Chicken),
            ("entity/cow/cow_temperate", MobModel::Cow),
            ("entity/sheep/sheep", MobModel::Sheep),
            ("entity/spider/spider", MobModel::Spider),
            ("entity/wolf/wolf", MobModel::Wolf),
            ("entity/fox/fox", MobModel::Fox),
            ("entity/villager/villager", MobModel::Villager),
            ("entity/enderman/enderman", MobModel::Enderman),
            ("entity/iron_golem/iron_golem", MobModel::IronGolem),
            ("entity/squid/squid", MobModel::Squid),
            ("entity/bat/bat", MobModel::Bat),
            ("entity/rabbit/rabbit_brown", MobModel::Rabbit),
            ("entity/horse/horse_brown", MobModel::Horse),
            ("entity/cat/cat_tabby", MobModel::Cat),
            ("entity/snow_golem/snow_golem", MobModel::SnowGolem),
            ("entity/turtle/turtle", MobModel::Turtle),
            ("entity/goat/goat", MobModel::Goat),
            ("entity/panda/panda", MobModel::Panda),
            ("entity/bear/polarbear", MobModel::PolarBear),
            ("entity/llama/llama_creamy", MobModel::Llama),
            ("entity/ghast/ghast", MobModel::Ghast),
            ("entity/blaze/blaze", MobModel::Blaze),
            ("entity/dolphin/dolphin", MobModel::Dolphin),
            ("entity/guardian/guardian", MobModel::Guardian),
            ("entity/fish/cod", MobModel::Cod),
            ("entity/fish/salmon", MobModel::Salmon),
            ("entity/bee/bee", MobModel::Bee),
            ("entity/silverfish/silverfish", MobModel::Silverfish),
            ("entity/parrot/parrot_red_blue", MobModel::Parrot),
            ("entity/phantom/phantom", MobModel::Phantom),
            ("entity/axolotl/axolotl_lucy", MobModel::Axolotl),
            ("entity/frog/frog_temperate", MobModel::Frog),
            ("entity/tadpole/tadpole", MobModel::Tadpole),
            ("entity/camel/camel", MobModel::Camel),
            ("entity/sniffer/sniffer", MobModel::Sniffer),
            ("entity/armadillo/armadillo", MobModel::Armadillo),
            ("entity/allay/allay", MobModel::Allay),
            ("entity/illager/vex", MobModel::Vex),
            ("entity/endermite/endermite", MobModel::Endermite),
            ("entity/fish/pufferfish", MobModel::Pufferfish),
            ("entity/illager/pillager", MobModel::Illager),
            ("entity/witch/witch", MobModel::Witch),
            ("entity/strider/strider", MobModel::Strider),
            ("entity/hoglin/hoglin", MobModel::Hoglin),
            ("entity/illager/ravager", MobModel::Ravager),
            ("entity/warden/warden", MobModel::Warden),
            ("entity/creaking/creaking", MobModel::Creaking),
        ];
        // Lay the roster out as a front-facing grid (columns in X, rows stacked
        // in Y at a fixed depth) so every model is eyeballable without the rows
        // receding into perspective and overlapping.
        let cols = 6usize;
        let rows = mobs.len().div_ceil(cols);
        let mut draws = Vec::new();
        for (i, (path, model)) in mobs.iter().enumerate() {
            if let Ok(img) = pack.texture_png(path) {
                let key = 100 + i as u64;
                renderer.ensure_skin(key, &img);
                let (col, row) = (i % cols, i / cols);
                let x = -6.5 + col as f32 * 2.6;
                // Top row highest; generous row spacing so tall mobs (golem,
                // enderman, ~3 blocks) never overlap the row above.
                let y = 60.0 + (rows - 1 - row) as f32 * 4.6;
                // Slimes are authored at 0.5 block; show a size-2 one here.
                let scale = if matches!(model, MobModel::Slime) { 2.0 } else { 1.0 };
                draws.push(EntityDraw {
                    pos: [x as f64, y as f64, 6.0],
                    yaw: 150.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Mob { tex: key, model: *model, swing: 0.3, head_pitch: 0.0, head_yaw: 0.0, scale },
                });
            }
        }
        let mid_y = 60.0 + (rows as f32 - 1.0) * 4.6 * 0.5 + 1.0;
        let scene = SceneParams {
            cam_pos: [0.0, mid_y as f64, -16.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 82.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering mob check")?;
        let img = renderer.read_screenshot().context("reading back mob check")?;
        let path = out_dir.join("menu_mobs.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "mob check written");
    }

    // Focused, larger preview of the 0.38.0 bestiary additions — each model
    // scaled to a similar apparent height and spread out so nothing overlaps,
    // for close headless inspection.
    {
        use crate::render::{EntityDraw, EntityDrawKind, MobModel};
        let mobs: &[(&str, MobModel, f32)] = &[
            ("entity/axolotl/axolotl_lucy", MobModel::Axolotl, 1.4),
            ("entity/frog/frog_temperate", MobModel::Frog, 1.4),
            ("entity/tadpole/tadpole", MobModel::Tadpole, 2.5),
            ("entity/camel/camel", MobModel::Camel, 0.6),
            ("entity/sniffer/sniffer", MobModel::Sniffer, 0.5),
            ("entity/armadillo/armadillo", MobModel::Armadillo, 1.4),
            ("entity/allay/allay", MobModel::Allay, 2.0),
            ("entity/illager/vex", MobModel::Vex, 2.0),
            ("entity/endermite/endermite", MobModel::Endermite, 2.5),
            ("entity/fish/pufferfish", MobModel::Pufferfish, 2.0),
            ("entity/illager/pillager", MobModel::Illager, 1.0),
            ("entity/witch/witch", MobModel::Witch, 1.0),
            ("entity/strider/strider", MobModel::Strider, 0.8),
            ("entity/hoglin/hoglin", MobModel::Hoglin, 0.9),
            ("entity/illager/ravager", MobModel::Ravager, 0.55),
            ("entity/warden/warden", MobModel::Warden, 0.55),
            ("entity/creaking/creaking", MobModel::Creaking, 0.7),
            ("entity/breeze/breeze", MobModel::Breeze, 1.2),
            ("entity/enderdragon/dragon", MobModel::EnderDragon, 0.35),
            ("entity/wither/wither", MobModel::Wither, 0.7),
            ("entity/shulker/shulker", MobModel::Shulker, 1.1),
            ("entity/armorstand/armorstand", MobModel::ArmorStand, 1.1),
            ("entity/end_crystal/end_crystal", MobModel::EndCrystal, 1.0),
        ];
        let cols = 5usize;
        let rows = mobs.len().div_ceil(cols);
        let (dx, dy) = (3.4f32, 3.4f32);
        let mut draws = Vec::new();
        for (i, (path, model, scale)) in mobs.iter().enumerate() {
            if let Ok(img) = pack.texture_png(path) {
                let key = 300 + i as u64;
                renderer.ensure_skin(key, &img);
                let (col, row) = (i % cols, i / cols);
                let x = -(cols as f32 - 1.0) * 0.5 * dx + col as f32 * dx;
                let y = 60.0 + (rows - 1 - row) as f32 * dy;
                draws.push(EntityDraw {
                    pos: [x as f64, y as f64, 4.0],
                    yaw: 150.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Mob { tex: key, model: *model, swing: 0.35, head_pitch: 0.0, head_yaw: 0.0, scale: *scale },
                });
            }
        }
        let mid_y = 60.0 + (rows as f32 - 1.0) * dy * 0.5 + 1.0;
        let scene = SceneParams {
            cam_pos: [0.0, mid_y as f64, -9.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 70.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering new-mob check")?;
        let img = renderer.read_screenshot().context("reading back new-mob check")?;
        let path = out_dir.join("menu_mobs_new.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "new-mob check written");
    }

    // Mob variant check (0.40.0): each species' colour/type variants on its
    // model, so the variant textures can be eyeballed headlessly.
    {
        use crate::render::{EntityDraw, EntityDrawKind, MobModel};
        let mobs: &[(&str, MobModel, f32)] = &[
            ("entity/rabbit/rabbit_brown", MobModel::Rabbit, 1.4),
            ("entity/rabbit/rabbit_white", MobModel::Rabbit, 1.4),
            ("entity/rabbit/rabbit_black", MobModel::Rabbit, 1.4),
            ("entity/rabbit/rabbit_gold", MobModel::Rabbit, 1.4),
            ("entity/rabbit/rabbit_salt", MobModel::Rabbit, 1.4),
            ("entity/parrot/parrot_red_blue", MobModel::Parrot, 1.6),
            ("entity/parrot/parrot_blue", MobModel::Parrot, 1.6),
            ("entity/parrot/parrot_green", MobModel::Parrot, 1.6),
            ("entity/parrot/parrot_yellow_blue", MobModel::Parrot, 1.6),
            ("entity/parrot/parrot_grey", MobModel::Parrot, 1.6),
            ("entity/axolotl/axolotl_lucy", MobModel::Axolotl, 1.4),
            ("entity/axolotl/axolotl_wild", MobModel::Axolotl, 1.4),
            ("entity/axolotl/axolotl_gold", MobModel::Axolotl, 1.4),
            ("entity/axolotl/axolotl_cyan", MobModel::Axolotl, 1.4),
            ("entity/axolotl/axolotl_blue", MobModel::Axolotl, 1.4),
            ("entity/horse/horse_white", MobModel::Horse, 0.75),
            ("entity/horse/horse_chestnut", MobModel::Horse, 0.75),
            ("entity/horse/horse_brown", MobModel::Horse, 0.75),
            ("entity/horse/horse_black", MobModel::Horse, 0.75),
            ("entity/horse/horse_gray", MobModel::Horse, 0.75),
            ("entity/llama/llama_creamy", MobModel::Llama, 0.7),
            ("entity/llama/llama_white", MobModel::Llama, 0.7),
            ("entity/llama/llama_brown", MobModel::Llama, 0.7),
            ("entity/llama/llama_gray", MobModel::Llama, 0.7),
            ("entity/fox/fox_snow", MobModel::Fox, 1.1),
            ("entity/cow/mooshroom_brown", MobModel::Cow, 0.9),
            ("entity/shulker/shulker_red", MobModel::Shulker, 1.1),
            ("entity/shulker/shulker_lime", MobModel::Shulker, 1.1),
            ("entity/shulker/shulker_blue", MobModel::Shulker, 1.1),
            ("entity/shulker/shulker_yellow", MobModel::Shulker, 1.1),
            // Registry-driven variants (0.41.0): cat / wolf / cow / frog.
            ("entity/cat/cat_tabby", MobModel::Cat, 1.3),
            ("entity/cat/cat_calico", MobModel::Cat, 1.3),
            ("entity/cat/cat_siamese", MobModel::Cat, 1.3),
            ("entity/cat/cat_red", MobModel::Cat, 1.3),
            ("entity/cat/cat_white", MobModel::Cat, 1.3),
            ("entity/wolf/wolf_ashen", MobModel::Wolf, 1.1),
            ("entity/wolf/wolf_chestnut", MobModel::Wolf, 1.1),
            ("entity/wolf/wolf_snowy", MobModel::Wolf, 1.1),
            ("entity/wolf/wolf_spotted", MobModel::Wolf, 1.1),
            ("entity/wolf/wolf_striped", MobModel::Wolf, 1.1),
            ("entity/cow/cow_cold", MobModel::Cow, 0.9),
            ("entity/cow/cow_warm", MobModel::Cow, 0.9),
            ("entity/frog/frog_cold", MobModel::Frog, 1.4),
            ("entity/frog/frog_warm", MobModel::Frog, 1.4),
        ];
        let cols = 5usize;
        let rows = mobs.len().div_ceil(cols);
        let (dx, dy) = (3.4f32, 3.4f32);
        let mut draws = Vec::new();
        for (i, (path, model, scale)) in mobs.iter().enumerate() {
            if let Ok(img) = pack.texture_png(path) {
                let key = 400 + i as u64;
                renderer.ensure_skin(key, &img);
                let (col, row) = (i % cols, i / cols);
                let x = -(cols as f32 - 1.0) * 0.5 * dx + col as f32 * dx;
                let y = 60.0 + (rows - 1 - row) as f32 * dy;
                draws.push(EntityDraw {
                    pos: [x as f64, y as f64, 4.0],
                    yaw: 150.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Mob { tex: key, model: *model, swing: 0.3, head_pitch: 0.0, head_yaw: 0.0, scale: *scale },
                });
            }
        }
        let mid_y = 60.0 + (rows as f32 - 1.0) * dy * 0.5 + 1.0;
        let scene = SceneParams {
            cam_pos: [0.0, mid_y as f64, -9.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 70.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering variant check")?;
        let img = renderer.read_screenshot().context("reading back variant check")?;
        let path = out_dir.join("menu_variants.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "variant check written");
    }

    // Villager appearance check (0.42.0): composite biome type + profession +
    // level badge (the same three layers the app pre-builds) and render a
    // sampling on the villager model, so the composites can be eyeballed.
    {
        use crate::render::{EntityDraw, EntityDrawKind, MobModel};
        // (biome type, profession, badge level or None for none/nitwit).
        let samples: &[(&str, &str, Option<&str>)] = &[
            ("plains", "none", None),
            ("plains", "nitwit", None),
            ("plains", "farmer", Some("stone")),
            ("desert", "farmer", Some("diamond")),
            ("savanna", "cleric", Some("gold")),
            ("snow", "librarian", Some("emerald")),
            ("jungle", "armorer", Some("iron")),
            ("swamp", "fisherman", Some("stone")),
            ("taiga", "weaponsmith", Some("diamond")),
            ("desert", "toolsmith", Some("emerald")),
        ];
        let cols = 5usize;
        let rows = samples.len().div_ceil(cols);
        let (dx, dy) = (2.6f32, 3.4f32);
        let mut draws = Vec::new();
        for (i, (vtype, prof, badge)) in samples.iter().enumerate() {
            let Ok(mut img) = pack.texture_png(&format!("entity/villager/type/{vtype}")) else {
                continue;
            };
            if *prof != "none"
                && let Ok(p) = pack.texture_png(&format!("entity/villager/profession/{prof}"))
            {
                image::imageops::overlay(&mut img, &p, 0, 0);
            }
            if let Some(b) = badge
                && let Ok(bi) = pack.texture_png(&format!("entity/villager/profession_level/{b}"))
            {
                image::imageops::overlay(&mut img, &bi, 0, 0);
            }
            let key = 500 + i as u64;
            renderer.ensure_skin(key, &img);
            let (col, row) = (i % cols, i / cols);
            let x = -(cols as f32 - 1.0) * 0.5 * dx + col as f32 * dx;
            let y = 60.0 + (rows - 1 - row) as f32 * dy;
            draws.push(EntityDraw {
                pos: [x as f64, y as f64, 4.0],
                yaw: 20.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Mob { tex: key, model: MobModel::Villager, swing: 0.15, head_pitch: 0.0, head_yaw: 0.0, scale: 1.3 },
            });
        }
        let mid_y = 60.0 + (rows as f32 - 1.0) * dy * 0.5 + 1.0;
        let scene = SceneParams {
            cam_pos: [0.0, mid_y as f64, -8.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 70.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering villager check")?;
        let img = renderer.read_screenshot().context("reading back villager check")?;
        let path = out_dir.join("menu_villagers.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "villager check written");
    }

    // Painting check (0.43.0): a sampling of artworks at their real aspect
    // ratios + a couple of different facings, to eyeball the flat-quad path.
    {
        use crate::render::{EntityDraw, EntityDrawKind};
        // (asset, width, height, facing, x). All North-facing (2) so the art
        // turns toward the camera on the −Z side; explicit x keeps each fully
        // in frame. X is mirrored in the shot (−x renders on the right).
        let samples: &[(&str, i32, i32, u8, f32)] = &[
            ("kebab", 1, 1, 2, -9.0),
            ("wanderer", 1, 2, 2, -6.0),
            ("pool", 2, 1, 2, -2.5),
            ("skull_and_roses", 2, 2, 2, 1.5),
            ("fighters", 4, 2, 2, 6.0),
            ("pointer", 4, 4, 2, 11.5),
        ];
        let back_tex = 599u64;
        if let Ok(back) = pack.texture_png("painting/back") {
            renderer.ensure_skin(back_tex, &back);
        }
        let mut draws = Vec::new();
        for (i, (asset, w, h, facing, x)) in samples.iter().enumerate() {
            let Ok(img) = pack.texture_png(&format!("painting/{asset}")) else { continue };
            let key = 600 + i as u64;
            renderer.ensure_skin(key, &img);
            draws.push(EntityDraw {
                pos: [*x as f64, 64.0, 9.0],
                yaw: 0.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Painting {
                    art_tex: key,
                    back_tex,
                    w: *w as f32,
                    h: *h as f32,
                    facing: *facing,
                },
            });
        }
        let scene = SceneParams {
            cam_pos: [0.0, 64.5, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 75.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering painting check")?;
        let img = renderer.read_screenshot().context("reading back painting check")?;
        let path = out_dir.join("menu_paintings.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "painting check written");
    }

    // Item-frame check (0.44.0): frames holding flat items at various rotations,
    // an empty frame and a glow frame. All North-facing so the art faces us.
    {
        use crate::render::{EntityDraw, EntityDrawKind};
        let frame_tex = 591u64;
        let glow_tex = 592u64;
        let back_tex = 590u64;
        if let Ok(img) = pack.texture_png("block/item_frame") {
            renderer.ensure_skin(frame_tex, &img);
        }
        if let Ok(img) = pack.texture_png("block/glow_item_frame") {
            renderer.ensure_skin(glow_tex, &img);
        }
        if let Ok(img) = pack.texture_png("painting/back") {
            renderer.ensure_skin(back_tex, &img);
        }
        // (item, rotation, glow, x).
        let samples: &[(Option<&str>, u8, bool, f32)] = &[
            (None, 0, false, -4.5),
            (Some("apple"), 0, false, -2.7),
            (Some("diamond"), 2, false, -0.9),
            (Some("golden_apple"), 4, false, 0.9),
            (Some("compass"), 6, false, 2.7),
            (Some("netherite_ingot"), 0, true, 4.5),
        ];
        let mut draws = Vec::new();
        for (item, rot, glow, x) in samples {
            let item_uv = item.and_then(|n| item_icons.uv(n));
            draws.push(EntityDraw {
                pos: [*x as f64, 64.0, 4.0],
                yaw: 0.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::ItemFrame {
                    frame_tex: if *glow { glow_tex } else { frame_tex },
                    back_tex,
                    facing: 2,
                    rot: *rot,
                    item_uv,
                    block_quads: Vec::new(),
                },
            });
        }
        let scene = SceneParams {
            cam_pos: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 70.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering frame check")?;
        let img = renderer.read_screenshot().context("reading back frame check")?;
        let path = out_dir.join("menu_frames.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "frame check written");
    }

    // Particle check (0.45.0): one billboard per texture family (first frame),
    // laid in a grid so each real particle sprite can be eyeballed.
    {
        use crate::bridge::events::ParticleTex as T;
        use crate::render::{EntityDraw, EntityDrawKind};
        let (atlas, uv_map) = super::build_particle_atlas(&mut pack);
        renderer.ensure_particle_atlas(&atlas);
        // (family, tint) — coloured families are tinted like the live styles.
        let fams: &[(T, [f32; 3])] = &[
            (T::Flame, [1.0, 1.0, 1.0]),
            (T::SoulFlame, [1.0, 1.0, 1.0]),
            (T::Lava, [1.0, 1.0, 1.0]),
            (T::Smoke, [1.0, 1.0, 1.0]),
            (T::Generic, [1.0, 1.0, 1.0]),
            (T::Crit, [1.0, 1.0, 1.0]),
            (T::EnchantedHit, [1.0, 1.0, 1.0]),
            (T::Damage, [1.0, 1.0, 1.0]),
            (T::Heart, [1.0, 1.0, 1.0]),
            (T::Angry, [1.0, 1.0, 1.0]),
            (T::Happy, [1.0, 1.0, 1.0]),
            (T::Effect, [1.0, 1.0, 1.0]),
            (T::Note, [1.0, 1.0, 1.0]),
            (T::Bubble, [1.0, 1.0, 1.0]),
            (T::Splash, [1.0, 1.0, 1.0]),
            (T::Drip, [0.30, 0.45, 0.85]),
            (T::Explosion, [1.0, 1.0, 1.0]),
            (T::Flash, [1.0, 1.0, 1.0]),
            (T::Glow, [1.0, 1.0, 1.0]),
            (T::Portal, [0.55, 0.25, 0.85]),
            (T::Dust, [0.85, 0.45, 0.45]),
        ];
        let cols = 7usize;
        let (dx, dy) = (1.3f32, 1.3f32);
        let rows = fams.len().div_ceil(cols);
        let mut draws = Vec::new();
        for (i, (tex, color)) in fams.iter().enumerate() {
            let Some(uv) = uv_map.get(tex).and_then(|f| f.first()).copied() else { continue };
            let (col, row) = (i % cols, i / cols);
            let x = -(cols as f32 - 1.0) * 0.5 * dx + col as f32 * dx;
            let y = 64.0 + (rows as f32 - 1.0) * 0.5 * dy - row as f32 * dy;
            draws.push(EntityDraw {
                pos: [x as f64, y as f64, 3.0],
                yaw: 0.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Particle { uv, color: *color, size: 0.9 },
            });
        }
        let scene = SceneParams {
            cam_pos: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 70.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.20, 0.22, 0.28],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering particle check")?;
        let img = renderer.read_screenshot().context("reading back particle check")?;
        let path = out_dir.join("menu_particles.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "particle check written");
    }

    // Projectile check (0.46.0): arrows (oriented by pitch) + a few thrown-item
    // sprites, to eyeball the crossed-plane arrow model and the item icons.
    {
        use crate::render::{EntityDraw, EntityDrawKind};
        let arrow_tex = 700u64;
        let spectral_tex = 701u64;
        if let Ok(img) = pack.texture_png("entity/projectiles/arrow") {
            renderer.ensure_skin(arrow_tex, &img);
        }
        if let Ok(img) = pack.texture_png("entity/projectiles/arrow_spectral") {
            renderer.ensure_skin(spectral_tex, &img);
        }
        let mut draws = Vec::new();
        // Arrows at a few pitches (yaw 90 → broadside to the −Z camera).
        let arrows: &[(u64, f32, f32)] = &[
            (arrow_tex, 90.0, 0.0),
            (arrow_tex, 90.0, -40.0),
            (arrow_tex, 90.0, 40.0),
            (spectral_tex, 90.0, 0.0),
        ];
        for (i, (tex, yaw, pitch)) in arrows.iter().enumerate() {
            draws.push(EntityDraw {
                pos: [-4.5 + i as f64 * 1.5, 64.5, 3.0],
                yaw: 0.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Projectile { tex: *tex, yaw: *yaw, pitch: *pitch },
            });
        }
        // Thrown-item sprites.
        let items = ["snowball", "egg", "ender_pearl", "splash_potion", "fire_charge", "firework_rocket"];
        for (i, name) in items.iter().enumerate() {
            if let Some(uv) = item_icons.uv(name) {
                draws.push(EntityDraw {
                    pos: [-3.75 + i as f64 * 1.5, 63.2, 3.0],
                    yaw: 30.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Item { uv },
                });
            }
        }
        let scene = SceneParams {
            cam_pos: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 75.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.30, 0.34, 0.42],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering projectile check")?;
        let img = renderer.read_screenshot().context("reading back projectile check")?;
        let path = out_dir.join("menu_projectiles.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "projectile check written");
    }

    // Vehicle check (0.47.0): boats (verify the hull) + the new minecart model.
    {
        use crate::render::{EntityDraw, EntityDrawKind, MobModel};
        let vehicles: &[(&str, MobModel, f32)] = &[
            ("entity/boat/oak", MobModel::Boat, 0.7),
            ("entity/boat/birch", MobModel::Boat, 0.7),
            ("entity/boat/bamboo", MobModel::Boat, 0.7),
            ("entity/chest_boat/oak", MobModel::Boat, 0.7),
            ("entity/minecart/minecart", MobModel::Minecart, 0.9),
        ];
        let cols = 5usize;
        let (dx, dy) = (3.4f32, 3.4f32);
        let rows = vehicles.len().div_ceil(cols);
        let mut draws = Vec::new();
        for (i, (path, model, scale)) in vehicles.iter().enumerate() {
            if let Ok(img) = pack.texture_png(path) {
                let key = 800 + i as u64;
                renderer.ensure_skin(key, &img);
                let (col, row) = (i % cols, i / cols);
                let x = -(cols as f32 - 1.0) * 0.5 * dx + col as f32 * dx;
                let y = 60.0 + (rows - 1 - row) as f32 * dy;
                draws.push(EntityDraw {
                    pos: [x as f64, y as f64, 4.0],
                    yaw: 150.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Mob { tex: key, model: *model, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: *scale },
                });
            }
        }
        let mid_y = 60.0 + (rows as f32 - 1.0) * dy * 0.5 + 1.0;
        let scene = SceneParams {
            cam_pos: [0.0, mid_y as f64, -9.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 70.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering vehicle check")?;
        let img = renderer.read_screenshot().context("reading back vehicle check")?;
        let path = out_dir.join("menu_vehicles.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "vehicle check written");
    }

    // Display-entity check (0.48.0): block-displays (full-size, half-scale, and
    // rotated) and item-displays under the vanilla T·Lrot·S·Rrot transform.
    // text_display rides the proven nametag path, so it isn't re-verified here.
    {
        use crate::render::{EntityDraw, EntityDrawKind};
        // Corner-origin (0..1) geometry for a named block, matching the live
        // block_geometry_by_state path used for real block_display entities.
        let block_quads = |name: &str| -> Option<Vec<([f32; 3], [f32; 2])>> {
            let sid = (0..table.len() as crate::types::StateId)
                .find(|&id| table.entry(id).map(|e| e.short_name == name).unwrap_or(false))?;
            let model = store.get(sid);
            if model.quads.is_empty() {
                return None;
            }
            let mut out = Vec::new();
            for q in &model.quads {
                for &k in &[0usize, 1, 2, 0, 2, 3] {
                    out.push((q.verts[k], q.uvs[k]));
                }
            }
            Some(out)
        };
        let no_rot = [0.0, 0.0, 0.0, 1.0];
        // 45° about Y as a quaternion (x, y, z, w).
        let yaw45 = [0.0, 0.382_683_43, 0.0, 0.923_879_5];
        let mut draws = Vec::new();
        // (block, translation, scale, left_rot, x). Translation re-centres each
        // corner-origin block roughly in front of the camera.
        let blocks: &[(&str, [f32; 3], [f32; 3], [f32; 4], f32)] = &[
            ("diamond_block", [-0.5, -0.5, -0.5], [1.0, 1.0, 1.0], no_rot, -6.0),
            ("gold_block", [-0.25, -0.25, -0.25], [0.5, 0.5, 0.5], no_rot, -3.0),
            ("emerald_block", [-0.5, -0.5, -0.5], [1.0, 1.0, 1.0], yaw45, 0.0),
        ];
        for (name, translation, scale, rot, x) in blocks {
            if let Some(quads) = block_quads(name) {
                draws.push(EntityDraw {
                    pos: [*x as f64, 64.0, 5.0],
                    yaw: 0.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::DisplayBlock {
                        quads,
                        translation: *translation,
                        scale: *scale,
                        left_rot: *rot,
                        right_rot: no_rot,
                    },
                });
            }
        }
        // (item, scale, x). Item-displays draw the flat icon under the transform.
        let items: &[(&str, [f32; 3], f32)] = &[
            ("diamond_sword", [3.0, 3.0, 3.0], 3.0),
            ("apple", [1.5, 1.5, 1.5], 6.0),
        ];
        for (name, scale, x) in items {
            if let Some(uv) = item_icons.uv(name) {
                draws.push(EntityDraw {
                    pos: [*x as f64, 64.5, 5.0],
                    yaw: 0.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::DisplayItem {
                        uv,
                        translation: [0.0, 0.0, 0.0],
                        scale: *scale,
                        left_rot: no_rot,
                        right_rot: no_rot,
                    },
                });
            }
        }
        let scene = SceneParams {
            cam_pos: [0.0, 64.8, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 75.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering display check")?;
        let img = renderer.read_screenshot().context("reading back display check")?;
        let path = out_dir.join("menu_displays.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "display check written");
    }

    // Armor-stand pose + XP orb check (0.49.0): several stands with different
    // poses (rest, T-pose, waving, small, no arms) plus a few floating orbs.
    {
        use crate::render::{EntityDraw, EntityDrawKind};
        let as_tex = 900u64;
        if let Ok(img) = pack.texture_png("entity/armorstand/armorstand") {
            renderer.ensure_skin(as_tex, &img);
        }
        let orb_tex = 901u64;
        if let Ok(img) = pack.texture_png("entity/experience/experience_orb") {
            let (cw, ch) = (img.width() / 4, img.height() / 4);
            let cell = image::imageops::crop_imm(&img, cw * 2, ch * 2, cw, ch).to_image();
            renderer.ensure_skin(orb_tex, &cell);
        }
        // (poses[head,body,rArm,lArm,rLeg,lLeg], small, arms, base, x).
        let rest: [[f32; 3]; 6] = [
            [0.0, 0.0, 0.0], [0.0, 0.0, 0.0],
            [-10.0, 0.0, -10.0], [-15.0, 0.0, 10.0],
            [-1.0, 0.0, -1.0], [1.0, 0.0, 1.0],
        ];
        let tpose: [[f32; 3]; 6] = [
            [0.0, 0.0, 0.0], [0.0, 0.0, 0.0],
            [0.0, 0.0, -90.0], [0.0, 0.0, 90.0],
            [0.0, 0.0, 0.0], [0.0, 0.0, 0.0],
        ];
        let wave: [[f32; 3]; 6] = [
            [0.0, 0.0, 12.0], [0.0, 0.0, 0.0],
            [0.0, 0.0, -160.0], [-15.0, 0.0, 10.0],
            [-1.0, 0.0, -1.0], [1.0, 0.0, 1.0],
        ];
        let noarms: [[f32; 3]; 6] = [
            [15.0, 20.0, 0.0], [8.0, 0.0, 0.0],
            [0.0, 0.0, 0.0], [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0], [0.0, 0.0, 0.0],
        ];
        let stands: &[([[f32; 3]; 6], bool, bool, bool, f32)] = &[
            (rest, false, true, true, -6.0),
            (tpose, false, true, true, -3.0),
            (wave, false, true, true, 0.0),
            (rest, true, true, false, 3.0),
            (noarms, false, false, true, 6.0),
        ];
        let mut draws = Vec::new();
        for (poses, small, arms, base, x) in stands {
            draws.push(EntityDraw {
                pos: [*x as f64, 63.6, 4.0],
                yaw: 150.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::ArmorStandPosed {
                    tex: as_tex,
                    scale: if *small { 0.5 } else { 1.0 },
                    show_arms: *arms,
                    show_base: *base,
                    poses: *poses,
                },
            });
        }
        for x in [-4.5f32, 1.5, 4.5] {
            draws.push(EntityDraw {
                pos: [x as f64, 64.6, 4.0],
                yaw: 0.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Orb { tex: orb_tex, size: 0.6, color: [0.6, 1.0, 0.2] },
            });
        }
        let scene = SceneParams {
            cam_pos: [0.0, 64.4, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            fov_deg: 75.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.30, 0.34, 0.40],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering pose check")?;
        let img = renderer.read_screenshot().context("reading back pose check")?;
        let path = out_dir.join("menu_posed.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "pose check written");
    }

    // Tropical fish + dyed pet collars + charged creeper + on-fire check
    // (0.50.0). Fish are drawn as a tinted base body + a tinted pattern overlay
    // on one of the two body-shape models; collars/creeper-swirl are overlays on
    // the animal model; fire is an upright billboard.
    {
        use crate::render::entity_models::MobModel;
        use crate::render::{EntityDraw, EntityDrawKind};

        // --- register textures with fixed keys ---
        let (fa, fb) = (910u64, 911u64);
        if let Ok(img) = pack.texture_png("entity/fish/tropical_a") { renderer.ensure_skin(fa, &img); }
        if let Ok(img) = pack.texture_png("entity/fish/tropical_b") { renderer.ensure_skin(fb, &img); }
        let mut pat = [[0u64; 6]; 2];
        for (si, shape) in ["a", "b"].iter().enumerate() {
            for p in 0..6 {
                let key = 920 + (si * 6 + p) as u64;
                pat[si][p] = key;
                if let Ok(img) =
                    pack.texture_png(&format!("entity/fish/tropical_{shape}_pattern_{}", p + 1))
                {
                    renderer.ensure_skin(key, &img);
                }
            }
        }
        let (cat_t, cat_c) = (940u64, 941u64);
        if let Ok(img) = pack.texture_png("entity/cat/cat_tabby") { renderer.ensure_skin(cat_t, &img); }
        if let Ok(img) = pack.texture_png("entity/cat/cat_collar") { renderer.ensure_skin(cat_c, &img); }
        let (wolf_t, wolf_c) = (942u64, 943u64);
        if let Ok(img) = pack.texture_png("entity/wolf/wolf") { renderer.ensure_skin(wolf_t, &img); }
        if let Ok(img) = pack.texture_png("entity/wolf/wolf_collar") { renderer.ensure_skin(wolf_c, &img); }
        let (creep_t, creep_a) = (944u64, 945u64);
        if let Ok(img) = pack.texture_png("entity/creeper/creeper") { renderer.ensure_skin(creep_t, &img); }
        if let Ok(img) = pack.texture_png("entity/creeper/creeper_armor") { renderer.ensure_skin(creep_a, &img); }
        let pig_t = 946u64;
        if let Ok(img) = pack.texture_png("entity/pig/pig_temperate") { renderer.ensure_skin(pig_t, &img); }
        let fire_t = 947u64;
        let mut fire_frames = 1u32;
        if let Ok(mut img) = pack.texture_png("block/fire_0") {
            for px in img.pixels_mut() {
                let [r, g, b, _] = px.0;
                if (r as u16 + g as u16 + b as u16) < 60 { px.0[3] = 0; }
            }
            fire_frames = (img.height() / img.width().max(1)).max(1);
            renderer.ensure_skin(fire_t, &img);
        }

        let mut draws: Vec<EntityDraw> = Vec::new();
        // --- a row of tropical fish: (shape, pattern, body colour, pattern colour) ---
        let fish: &[(usize, usize, i32, i32)] = &[
            (0, 0, 1, 0),   // small, orange body / white pattern
            (0, 1, 11, 8),  // small, blue / light-gray
            (0, 4, 14, 0),  // small, red / white
            (1, 0, 4, 14),  // large, yellow / red
            (1, 2, 3, 11),  // large, light-blue / blue
            (1, 5, 13, 4),  // large, green / yellow
        ];
        for (i, &(shape, pattern, body, patc)) in fish.iter().enumerate() {
            let x = -6.0 + i as f64 * 2.4;
            let model = if shape == 0 { MobModel::TropicalFishA } else { MobModel::TropicalFishB };
            let base = if shape == 0 { fa } else { fb };
            let s = 2.4;
            draws.push(EntityDraw {
                pos: [x, 64.2, 4.0],
                yaw: 90.0,
                light: [1.0, 1.0],
                tint: super::dye_rgb(body),
                roll: 0.0,
                kind: EntityDrawKind::Mob { tex: base, model, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: s },
            });
            draws.push(EntityDraw {
                pos: [x, 64.2, 4.0],
                yaw: 90.0,
                light: [1.0, 1.0],
                tint: super::dye_rgb(patc),
                roll: 0.0,
                kind: EntityDrawKind::Mob { tex: pat[shape][pattern], model, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: s * 1.006 },
            });
        }

        // --- animals: burning pig, charged creeper, collared cat + wolf ---
        // Burning pig: the pig + an upright flame billboard over it.
        draws.push(EntityDraw {
            pos: [-6.0, 62.4, 7.5], yaw: 200.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Mob { tex: pig_t, model: MobModel::Pig, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.0 },
        });
        let f = 8u32.min(fire_frames.saturating_sub(1));
        let n = fire_frames as f32;
        draws.push(EntityDraw {
            pos: [-6.0, 62.4, 7.5], yaw: 0.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Fire { tex: fire_t, w: 1.3, h: 1.3, uv: [0.0, f as f32 / n, 1.0, (f + 1) as f32 / n] },
        });
        // Charged creeper: creeper + inflated energy-swirl overlay.
        draws.push(EntityDraw {
            pos: [-2.0, 62.4, 7.5], yaw: 200.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Mob { tex: creep_t, model: MobModel::Creeper, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.0 },
        });
        draws.push(EntityDraw {
            pos: [-2.0, 62.4, 7.5], yaw: 200.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Mob { tex: creep_a, model: MobModel::Creeper, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.08 },
        });
        // Tamed cat with a red collar.
        draws.push(EntityDraw {
            pos: [2.0, 62.4, 7.5], yaw: 200.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Mob { tex: cat_t, model: MobModel::Cat, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.0 },
        });
        draws.push(EntityDraw {
            pos: [2.0, 62.4, 7.5], yaw: 200.0, tint: super::dye_rgb(14), roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Mob { tex: cat_c, model: MobModel::Cat, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.02 },
        });
        // Tamed wolf with a blue collar.
        draws.push(EntityDraw {
            pos: [6.0, 62.4, 7.5], yaw: 200.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Mob { tex: wolf_t, model: MobModel::Wolf, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.0 },
        });
        draws.push(EntityDraw {
            pos: [6.0, 62.4, 7.5], yaw: 200.0, tint: super::dye_rgb(11), roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Mob { tex: wolf_c, model: MobModel::Wolf, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.02 },
        });

        let scene = SceneParams {
            cam_pos: [0.0, 64.2, 0.0],
            yaw: 0.0,
            pitch: 8.0,
            fov_deg: 75.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.30, 0.34, 0.40],
            panorama: has_panorama,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering fish check")?;
        let img = renderer.read_screenshot().context("reading back fish check")?;
        let path = out_dir.join("menu_fish.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "fish check written");
    }

    // Fluids + animated-texture check (0.51.0). A hand-built world — no server,
    // no world generation — meshed straight through `mesh_section`, so it always
    // frames the exact same scene: water sources, a 7→1 flowing channel with its
    // sloped surface, a falling column, lava, and a row of animated blocks. Two
    // shots at different animation ticks prove the sprite ticker actually runs.
    {
        let mut world: std::collections::HashMap<(i32, i32, i32), crate::types::StateId> =
            std::collections::HashMap::new();
        let id = |name: &str, props: &[(&str, &str)]| -> crate::types::StateId {
            table.find_state(name, props).unwrap_or(0)
        };
        let air = id("air", &[]);
        let stone = id("stone", &[]);
        let sand = id("sand", &[]);
        let glass = id("glass", &[]);
        let water = |lvl: u32| id("water", &[("level", &lvl.to_string())]);
        let lava = |lvl: u32| id("lava", &[("level", &lvl.to_string())]);
        let mut set = |x: i32, y: i32, z: i32, s: crate::types::StateId| {
            world.insert((x, y, z), s);
        };

        // Ground slab everything sits on.
        for x in -10..=10 {
            for z in -6..=14 {
                set(x, 60, z, stone);
                set(x, 61, z, stone);
            }
        }
        // Water basin (sunk one deep) with source blocks — a flat, full surface.
        for x in -9..=-3 {
            for z in 0..=6 {
                set(x, 61, z, sand);
                set(x, 62, z, water(0));
            }
        }
        // Flowing channel: level 1..7 steps down eastwards, each with a lower
        // surface than the last, so the sloped tops and side faces show.
        for (i, lvl) in (1..=7).enumerate() {
            let x = -2 + i as i32;
            for z in 0..=6 {
                set(x, 62, z, water(lvl));
            }
        }
        // Falling water: level 8 (falling) fills its whole cell top to bottom.
        for y in 63..=70 {
            set(6, y, 3, water(8));
            set(6, y, 4, water(8));
        }
        // Backing wall on the far side only, so the camera (which sits at +x,
        // −z) looks straight at the falling column instead of at stone.
        for y in 62..=71 {
            set(6, y, 5, stone);
            set(5, y, 3, stone);
            set(5, y, 4, stone);
        }
        // Lava pool + a short lava fall, next to the water for contrast.
        for x in -9..=-4 {
            for z in 9..=13 {
                set(x, 61, z, sand);
                set(x, 62, z, lava(0));
            }
        }
        for (i, lvl) in [2u32, 4, 6].into_iter().enumerate() {
            let x = -3 + i as i32;
            for z in 9..=13 {
                set(x, 62, z, lava(lvl));
            }
        }
        // Animated block row, raised on a shelf behind the pools: every one of
        // these has an `.mcmeta` animation in the vanilla jar.
        let animated = [
            "sea_lantern",
            "magma_block",
            "prismarine",
            "command_block",
            "respawn_anchor",
        ];
        for (i, name) in animated.iter().enumerate() {
            let x = 0 + i as i32 * 2;
            set(x, 63, 11, id(name, &[]));
            set(x, 62, 11, stone);
        }
        // A nether portal frame (its texture is animated too).
        for y in 63..=66 {
            set(-1, y, 11, id("obsidian", &[]));
        }
        // Glass, so translucency over the water reads correctly in the shot.
        for z in 0..=6 {
            set(-10, 62, z, glass);
            set(-10, 63, z, glass);
        }

        // Mesh every section the scene touches. `snapshot27` on a live mirror
        // does this from chunk data; here the padded neighbourhood is sampled
        // straight out of the map (missing cells = air).
        let biome_tints = crate::types::BiomeTints::default();
        renderer.clear_meshes();
        let mut quads = 0usize;
        for sy in 3..5 {
            for sz in -1..1 {
                for sx in -1..1 {
                    let pos = SectionPos { x: sx, y: sy, z: sz };
                    let mut blocks = Box::new([air; crate::types::PADDED_VOLUME]);
                    for y in -1..=16i32 {
                        for z in -1..=16i32 {
                            for x in -1..=16i32 {
                                let key = (pos.x * 16 + x, pos.y * 16 + y, pos.z * 16 + z);
                                blocks[crate::types::PaddedSnapshot::idx(x, y, z)] =
                                    world.get(&key).copied().unwrap_or(air);
                            }
                        }
                    }
                    let snap = crate::types::PaddedSnapshot {
                        pos,
                        blocks,
                        light: Box::new([0xFF; crate::types::PADDED_VOLUME]),
                        biome: 0,
                    };
                    let mesh = mesh_section(&snap, &store, &table, &biome_tints, true);
                    quads += mesh.layers.iter().map(|l| l.indices.len() / 6).sum::<usize>();
                    renderer.upload_mesh(mesh);
                }
            }
        }
        info!(quads, "fluid check meshed");

        let scene = SceneParams {
            cam_pos: [9.5, 69.5, -5.5],
            yaw: 47.0,
            pitch: 22.0,
            fov_deg: 80.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: false,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        // Entity shadows over the same scene: three pigs at rising heights, so
        // the blob shrinks with the gap to the ground and vanishes once the
        // entity is more than its own radius above it — exactly like vanilla.
        let mut draws: Vec<crate::render::EntityDraw> = Vec::new();
        {
            use crate::render::entity_models::MobModel;
            use crate::render::{EntityDraw, EntityDrawKind};
            let pig_t = 946u64;
            if let Ok(img) = pack.texture_png("entity/pig/pig_temperate") {
                renderer.ensure_skin(pig_t, &img);
            }
            let shadow_t = 948u64;
            if let Ok(img) = pack.texture_png("misc/shadow") {
                renderer.ensure_skin(shadow_t, &img);
            }
            let solid = |x: i32, y: i32, z: i32| {
                let s = world.get(&(x, y, z)).copied().unwrap_or(air);
                store.occludes(s, crate::types::Face::Up)
            };
            for (i, lift) in [0.0f64, 0.35, 0.9].into_iter().enumerate() {
                let p = [0.5 + i as f64 * 2.2, 62.0 + lift, -2.5];
                draws.push(EntityDraw {
                    pos: p,
                    yaw: 210.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Mob {
                        tex: pig_t,
                        model: MobModel::Pig,
                        swing: 0.0,
                        head_pitch: 0.0,
                        head_yaw: 0.0,
                        scale: 1.0,
                    },
                });
                let radius = super::shadow_radius("pig", 0.9);
                let patches = super::shadow_patches_with(p, radius, solid);
                if !patches.is_empty() {
                    draws.push(EntityDraw {
                        pos: p,
                        yaw: 0.0,
                        light: [1.0, 1.0],
                        tint: [1.0, 1.0, 1.0],
                        roll: 0.0,
                        kind: EntityDrawKind::Shadow {
                            tex: shadow_t,
                            radius,
                            alpha: 0.5,
                            patches,
                        },
                    });
                }
            }
        }

        let mut anim = crate::assets::atlas::AtlasAnimator::new(std::mem::take(&mut atlas.animations));
        for (tick, name) in [(0u64, "menu_fluids.png"), (37, "menu_fluids_anim.png")] {
            anim.tick(tick, |u| renderer.update_atlas_rect(u.x, u.y, u.w, u.h, u.rgba));
            renderer.frame(&scene, &draws, None).context("rendering fluid check")?;
            let img = renderer.read_screenshot().context("reading back fluid check")?;
            let path = out_dir.join(name);
            img.save(&path).with_context(|| format!("saving {}", path.display()))?;
            info!(path = %path.display(), tick, animated = anim.len(), "fluid check written");
        }
        renderer.clear_meshes();
    }

    // Beacon beams, leads, sheep fleece, falling blocks and stacked item drops
    // (0.51.0) — one deterministic scene, again meshed straight through
    // `mesh_section` with no server involved.
    {
        use crate::render::entity_models::MobModel;
        use crate::render::{EntityDraw, EntityDrawKind};

        let id = |name: &str| -> crate::types::StateId { table.find_state(name, &[]).unwrap_or(0) };
        let air = id("air");
        let mut world: std::collections::HashMap<(i32, i32, i32), crate::types::StateId> =
            std::collections::HashMap::new();
        let stone = id("stone");
        for x in -12..=12 {
            for z in -8..=12 {
                world.insert((x, 62, z), stone);
                world.insert((x, 61, z), stone);
            }
        }
        // An End portal pool and an End gateway — both block entities in vanilla,
        // both invisible without the starfield surface.
        for ox in 0..3 {
            for oz in 0..2 {
                world.insert((-11 + ox, 62, -4 + oz), id("end_portal"));
            }
        }
        world.insert((-6, 63, -4), id("end_gateway"));

        // Two beacons: one plain (white beam), one under blue stained glass.
        for (i, glass) in [None, Some("blue_stained_glass")].into_iter().enumerate() {
            let bx = -6 + i as i32 * 8;
            for ox in -1..=1 {
                for oz in -1..=1 {
                    world.insert((bx + ox, 63, 6 + oz), id("iron_block"));
                }
            }
            world.insert((bx, 64, 6), id("beacon"));
            if let Some(g) = glass {
                world.insert((bx, 65, 6), id(g));
            }
        }

        let biome_tints = crate::types::BiomeTints::default();
        renderer.clear_meshes();
        for sy in 3..5 {
            for sz in -1..1 {
                for sx in -1..1 {
                    let pos = SectionPos { x: sx, y: sy, z: sz };
                    let mut blocks = Box::new([air; crate::types::PADDED_VOLUME]);
                    for y in -1..=16i32 {
                        for z in -1..=16i32 {
                            for x in -1..=16i32 {
                                let key = (pos.x * 16 + x, pos.y * 16 + y, pos.z * 16 + z);
                                blocks[crate::types::PaddedSnapshot::idx(x, y, z)] =
                                    world.get(&key).copied().unwrap_or(air);
                            }
                        }
                    }
                    let snap = crate::types::PaddedSnapshot {
                        pos,
                        blocks,
                        light: Box::new([0xFF; crate::types::PADDED_VOLUME]),
                        biome: 0,
                    };
                    renderer.upload_mesh(mesh_section(&snap, &store, &table, &biome_tints, true));
                }
            }
        }

        let mut draws: Vec<EntityDraw> = Vec::new();
        // Beam textures need the wrapping sampler, like the live client.
        let beam_t = 960u64;
        if let Ok(img) = pack.texture_png("entity/beacon/beacon_beam") {
            renderer.ensure_skin_tiled(beam_t, &img);
        }
        for (i, color) in [[1.0f32, 1.0, 1.0], super::dye_rgb(11)].into_iter().enumerate() {
            let p = [(-6 + i as i32 * 8) as f64 + 0.5, 65.0, 6.5];
            for (width, alpha, spin) in [(0.2, 1.0, 25.0), (0.25, 0.125, 0.0)] {
                draws.push(EntityDraw {
                    pos: p,
                    yaw: 0.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: EntityDrawKind::Beam {
                        tex: beam_t,
                        height: 40.0,
                        width,
                        alpha,
                        color,
                        spin,
                        v_off: -0.4,
                    },
                });
            }
        }
        // A woolly sheep beside a sheared one.
        let (sheep_t, wool_t) = (961u64, 962u64);
        if let Ok(img) = pack.texture_png("entity/sheep/sheep") { renderer.ensure_skin(sheep_t, &img); }
        if let Ok(img) = pack.texture_png("entity/sheep/sheep_wool") { renderer.ensure_skin(wool_t, &img); }
        for (i, woolly) in [true, false].into_iter().enumerate() {
            let p = [-10.0 + i as f64 * 2.2, 63.0, 1.0];
            draws.push(EntityDraw {
                pos: p, yaw: 200.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
                kind: EntityDrawKind::Mob { tex: sheep_t, model: MobModel::Sheep, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.0 },
            });
            if woolly {
                draws.push(EntityDraw {
                    pos: p, yaw: 200.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
                    kind: EntityDrawKind::Mob { tex: wool_t, model: MobModel::Sheep, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.12 },
                });
            }
        }
        // A leashed pig: the mob plus the lead up to a fence-post-height anchor.
        let pig_t = 946u64;
        if let Ok(img) = pack.texture_png("entity/pig/pig_temperate") { renderer.ensure_skin(pig_t, &img); }
        let pig = [-4.0, 63.0, 1.0];
        draws.push(EntityDraw {
            pos: pig, yaw: 150.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Mob { tex: pig_t, model: MobModel::Pig, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.0 },
        });
        draws.push(EntityDraw {
            pos: [pig[0], pig[1] + 0.7, pig[2]], yaw: 0.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Rope { to: [2.6, 1.1, 0.4], sag: 0.35, thickness: 0.05, color: [0.35, 0.27, 0.20] },
        });
        // A fishing bobber on its line.
        let bob_t = 963u64;
        if let Ok(img) = pack.texture_png("entity/fishing/fishing_hook") { renderer.ensure_skin(bob_t, &img); }
        let bob = [2.0, 63.6, 0.0];
        draws.push(EntityDraw {
            pos: bob, yaw: 0.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Orb { tex: bob_t, size: 0.25, color: [1.0, 1.0, 1.0] },
        });
        draws.push(EntityDraw {
            pos: bob, yaw: 0.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
            kind: EntityDrawKind::Rope { to: [3.0, 1.2, -1.0], sag: 0.02, thickness: 0.02, color: [0.04, 0.04, 0.04] },
        });
        // A falling anvil, drawn from its real block model.
        if let Some(quads) = super::block_geometry_centred(&store, id("anvil")) {
            draws.push(EntityDraw {
                pos: [5.5, 65.5, 1.0], yaw: 0.0, tint: [1.0, 1.0, 1.0], roll: 0.0, light: [1.0, 1.0],
                kind: EntityDrawKind::StaticBlock { quads, y_off: 0.5, scale: 1.0, flash: 0.0 },
            });
        }
        // Stacked item drops: 1, 17 and 64 diamonds — 1, 3 and 5 sprites.
        if let Some(uv) = item_icons.uv("diamond") {
            for (i, count) in [1u32, 17, 64].into_iter().enumerate() {
                let p = [7.5 + i as f64 * 1.4, 63.2, 1.0];
                for c in 0..super::render_amount(count) {
                    let (dx, dy, dz) = super::stack_offset(1234 + i as u64, c, false);
                    draws.push(EntityDraw {
                        pos: [p[0] + dx, p[1] + dy, p[2] + dz],
                        yaw: 35.0,
                        light: [1.0, 1.0],
                        tint: [1.0, 1.0, 1.0],
                        roll: 0.0,
                        kind: EntityDrawKind::Item { uv },
                    });
                }
            }
        }

        let scene = SceneParams {
            cam_pos: [3.0, 68.0, -13.0],
            yaw: 14.0,
            pitch: 16.0,
            fov_deg: 80.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.47, 0.65, 1.0],
            panorama: false,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering beacon check")?;
        let img = renderer.read_screenshot().context("reading back beacon check")?;
        let path = out_dir.join("menu_beacons.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), draws = draws.len(), "beacon check written");
        renderer.clear_meshes();
    }

    // Block-entity check (0.52.0): sign text in the vanilla bitmap font, banner
    // pattern stacks, every mob head, a bell, a conduit and a decorated pot —
    // all built through the real `blockentities` path, so the preview exercises
    // the same compositing and placement code the live client runs. Two shots:
    // the flat, text-bearing pieces and the solid ones.
    {
        use crate::app::blockentities::{self, BeState, BlockEntities};
        use crate::bridge::events::{BlockEntityData, ChatSpan, SignFace};
        use crate::render::{EntityDraw, EntityDrawKind};

        let id = |name: &str, props: &[(&str, &str)]| -> crate::types::StateId {
            table.find_state(name, props).unwrap_or(0)
        };
        let air = id("air", &[]);
        let stone = id("stone", &[]);
        let sign_face = |lines: [&str; 4], color: &str, glowing: bool| SignFace {
            lines: lines.map(|l| if l.is_empty() { Vec::new() } else { vec![ChatSpan::plain(l)] }),
            color: color.to_owned(),
            glowing,
        };
        let banner = |layers: &[(&str, u8)]| BlockEntityData::Banner {
            layers: layers.iter().map(|(a, c)| ((*a).to_owned(), *c)).collect(),
        };
        let blank = BlockEntityData::Skull { texture_url: None, owner: None };

        // One block entity to place: position, block state, decoded NBT.
        type Placed = ((i32, i32, i32), crate::types::StateId, BlockEntityData);

        // Both shots share this: build the world, mesh it, run the real draw
        // builder over every entity, upload what it composited, shoot.
        let shot = |renderer: &mut Renderer,
                        pack: &mut AssetPack,
                        entities: &[Placed],
                        extra_blocks: &[((i32, i32, i32), crate::types::StateId)],
                        cam: [f64; 3],
                        yaw: f32,
                        pitch: f32,
                        name: &str|
         -> Result<()> {
            let mut world: std::collections::HashMap<(i32, i32, i32), crate::types::StateId> =
                std::collections::HashMap::new();
            for z in -4..14 {
                for x in -8..16 {
                    world.insert((x, 62, z), stone);
                }
            }
            for (p, s) in extra_blocks {
                world.insert(*p, *s);
            }
            for (p, s, _) in entities {
                world.insert(*p, *s);
            }
            let biome_tints = crate::types::BiomeTints::default();
            renderer.clear_meshes();
            for sy in 3..5 {
                for sz in -1..1 {
                    for sx in -1..1 {
                        let pos = SectionPos { x: sx, y: sy, z: sz };
                        let mut blocks = Box::new([air; crate::types::PADDED_VOLUME]);
                        for y in -1..=16i32 {
                            for z in -1..=16i32 {
                                for x in -1..=16i32 {
                                    let key = (pos.x * 16 + x, pos.y * 16 + y, pos.z * 16 + z);
                                    blocks[crate::types::PaddedSnapshot::idx(x, y, z)] =
                                        world.get(&key).copied().unwrap_or(air);
                                }
                            }
                        }
                        let snap = crate::types::PaddedSnapshot {
                            pos,
                            blocks,
                            light: Box::new([0xFF; crate::types::PADDED_VOLUME]),
                            biome: 0,
                        };
                        renderer.upload_mesh(mesh_section(&snap, &store, &table, &biome_tints, true));
                    }
                }
            }

            let mut be = BlockEntities::default();
            let font = crate::assets::font::Font::load(pack);
            let mut draws: Vec<EntityDraw> = Vec::new();
            for ((x, y, z), state, data) in entities {
                let entry = table.entry(*state);
                let short = entry.map(|e| e.short_name.clone()).unwrap_or_default();
                let st = BeState {
                    short: &short,
                    rotation: entry.and_then(|e| e.prop("rotation")).and_then(|r| r.parse().ok()),
                    facing: entry.and_then(|e| e.prop("facing")),
                    player_head: None,
                    conduit_active: matches!(data, BlockEntityData::Conduit),
                    // A fixed clock keeps the shot byte-identical between runs.
                    time: 0.0,
                    struck: matches!(data, BlockEntityData::Bell).then_some((0.12, 3)),
                };
                let d = blockentities::draw_for(&mut be, pack, &font, data, &st);
                let origin = [*x as f64 + 0.5, *y as f64, *z as f64 + 0.5];
                for part in d.parts {
                    draws.push(EntityDraw {
                        pos: super::rotate_offset(origin, part.offset, part.yaw),
                        yaw: part.yaw,
                        light: [1.0, 1.0],
                        tint: [1.0, 1.0, 1.0],
                        roll: 0.0,
                        kind: EntityDrawKind::Mob {
                            tex: part.tex,
                            model: part.model,
                            swing: part.swing,
                            head_pitch: 0.0,
                            head_yaw: 0.0,
                            scale: part.scale,
                        },
                    });
                }
                for text in d.texts {
                    draws.push(EntityDraw {
                        pos: super::rotate_offset(origin, text.offset, text.yaw),
                        yaw: text.yaw,
                        light: [1.0, 1.0],
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
            }
            let composited = be.take_pending();
            info!(shot = name, textures = composited.len(), draws = draws.len(), "block-entity check built");
            for (key, img) in composited {
                renderer.ensure_skin(key, &img);
            }
            let scene = SceneParams {
                cam_pos: cam,
                yaw,
                pitch,
                fov_deg: 70.0,
                daylight: 1.0,
                fog_start: 200.0,
                fog_end: 400.0,
                sky_color: [0.47, 0.65, 1.0],
                panorama: false,
                outline: Vec::new(),
                crack: None,
                view_model: None,
                sky: None,
            lightmap: Default::default(),
            end_sky: false,
            };
            renderer.frame(&scene, &draws, None).context("rendering block-entity check")?;
            let img = renderer.read_screenshot().context("reading back block-entity check")?;
            let path = out_dir.join(name);
            img.save(&path).with_context(|| format!("saving {}", path.display()))?;
            info!(path = %path.display(), "block-entity check written");
            renderer.clear_meshes();
            Ok(())
        };

        // --- signs + banners ------------------------------------------------
        // The camera looks south (+Z), so anything meant to be read faces north.
        // Each kind appears at both rotation 0 and rotation 8, which proves the
        // rotation → yaw mapping matches vanilla instead of being 180° out.
        let wall: Vec<((i32, i32, i32), crate::types::StateId)> = (-8..12)
            .flat_map(|x| (63..66).map(move |y| ((x, y, 10), stone)))
            .collect();
        let signs: Vec<Placed> = vec![
            (
                (-4, 63, 2),
                id("oak_sign", &[("rotation", "0"), ("waterlogged", "false")]),
                BlockEntityData::Sign {
                    front: sign_face(["rotation 0", "front", "", ""], "black", false),
                    back: sign_face(["rotation 0", "back", "", ""], "red", false),
                },
            ),
            (
                (-1, 63, 2),
                id("oak_sign", &[("rotation", "8"), ("waterlogged", "false")]),
                BlockEntityData::Sign {
                    front: sign_face(["Dolphin", "Client", "0.53.0", "signs!"], "black", false),
                    back: SignFace::default(),
                },
            ),
            (
                (2, 63, 2),
                id("oak_hanging_sign", &[("rotation", "8"), ("waterlogged", "false")]),
                BlockEntityData::Sign {
                    front: sign_face(["glowing", "ink", "", ""], "lime", true),
                    back: SignFace::default(),
                },
            ),
            (
                (5, 64, 9),
                id("birch_wall_sign", &[("facing", "north"), ("waterlogged", "false")]),
                BlockEntityData::Sign {
                    front: sign_face(["wall sign", "on the wall", "", ""], "blue", false),
                    back: SignFace::default(),
                },
            ),
            ((-5, 63, 7), id("white_banner", &[("rotation", "0")]), banner(&[])),
            (
                (-2, 63, 7),
                id("red_banner", &[("rotation", "8")]),
                banner(&[("stripe_bottom", 0), ("cross", 4), ("border", 15)]),
            ),
            (
                (1, 63, 7),
                id("blue_banner", &[("rotation", "8")]),
                banner(&[("gradient", 0), ("skull", 15)]),
            ),
            (
                (4, 64, 9),
                id("green_wall_banner", &[("facing", "north")]),
                banner(&[("half_horizontal", 4)]),
            ),
        ];
        shot(&mut renderer, &mut pack, &signs, &wall, [-1.0, 64.4, -1.5], 0.0, 3.0, "menu_signs.png")?;

        // --- heads, pot, bell, conduit ---------------------------------------
        // The conduit needs its water box and prismarine frame to read "active".
        let mut solids_blocks: Vec<((i32, i32, i32), crate::types::StateId)> = Vec::new();
        for dx in -2..=2i32 {
            for dy in -2..=2i32 {
                for dz in -2..=2i32 {
                    solids_blocks.push(((-9 + dx, 65 + dy, 4 + dz), id("water", &[("level", "0")])));
                }
            }
        }
        for [dx, dy, dz] in blockentities::conduit_frame_offsets() {
            solids_blocks.push(((-9 + dx, 65 + dy, 4 + dz), id("prismarine", &[])));
        }
        // The bell hangs off its support block, which is a real block model.
        let heads: Vec<Placed> = vec![
            ((-6, 63, 4), id("skeleton_skull", &[("rotation", "8")]), blank.clone()),
            ((-4, 63, 4), id("wither_skeleton_skull", &[("rotation", "8")]), blank.clone()),
            ((-2, 63, 4), id("zombie_head", &[("rotation", "8")]), blank.clone()),
            ((0, 63, 4), id("creeper_head", &[("rotation", "8")]), blank.clone()),
            ((2, 63, 4), id("piglin_head", &[("rotation", "8")]), blank.clone()),
            ((4, 63, 4), id("dragon_head", &[("rotation", "8")]), blank),
            (
                (6, 63, 4),
                id("decorated_pot", &[("facing", "north"), ("waterlogged", "false")]),
                BlockEntityData::DecoratedPot {
                    sherds: [
                        Some("angler_pottery_pattern".into()),
                        Some("heart_pottery_pattern".into()),
                        Some("explorer_pottery_pattern".into()),
                        Some("howl_pottery_pattern".into()),
                    ],
                },
            ),
            (
                (8, 63, 4),
                id("bell", &[("facing", "north"), ("attachment", "floor")]),
                BlockEntityData::Bell,
            ),
            ((-9, 65, 4), id("conduit", &[("waterlogged", "true")]), BlockEntityData::Conduit),
        ];
        shot(&mut renderer, &mut pack, &heads, &solids_blocks, [0.0, 65.2, -7.0], 0.0, 7.0, "menu_heads.png")?;
    }

    // Entity-fidelity check (0.52.0): a lightning bolt, a mob mid-death, a mob
    // whose head is turned away from its body, and a rider seated in a boat.
    {
        use crate::render::{EntityDraw, EntityDrawKind, MobModel};

        let mut world: std::collections::HashMap<(i32, i32, i32), crate::types::StateId> =
            std::collections::HashMap::new();
        let id = |name: &str, props: &[(&str, &str)]| -> crate::types::StateId {
            table.find_state(name, props).unwrap_or(0)
        };
        let air = id("air", &[]);
        let stone = id("stone", &[]);
        for z in -4..14 {
            for x in -10..12 {
                world.insert((x, 62, z), stone);
            }
        }
        let biome_tints = crate::types::BiomeTints::default();
        renderer.clear_meshes();
        for sy in 3..5 {
            for sz in -1..1 {
                for sx in -1..1 {
                    let pos = SectionPos { x: sx, y: sy, z: sz };
                    let mut blocks = Box::new([air; crate::types::PADDED_VOLUME]);
                    for y in -1..=16i32 {
                        for z in -1..=16i32 {
                            for x in -1..=16i32 {
                                let key = (pos.x * 16 + x, pos.y * 16 + y, pos.z * 16 + z);
                                blocks[crate::types::PaddedSnapshot::idx(x, y, z)] =
                                    world.get(&key).copied().unwrap_or(air);
                            }
                        }
                    }
                    let snap = crate::types::PaddedSnapshot {
                        pos,
                        blocks,
                        light: Box::new([0xFF; crate::types::PADDED_VOLUME]),
                        biome: 0,
                    };
                    renderer.upload_mesh(mesh_section(&snap, &store, &table, &biome_tints, true));
                }
            }
        }

        let (pig_t, boat_t) = (971u64, 972u64);
        if let Ok(img) = pack.texture_png("entity/pig/pig_temperate") {
            renderer.ensure_skin(pig_t, &img);
        }
        if let Ok(img) = pack.texture_png("entity/boat/oak") {
            renderer.ensure_skin(boat_t, &img);
        }
        let mut draws: Vec<EntityDraw> = Vec::new();
        // One player standing, one 70 % through vanilla's death fall.
        for (i, roll) in [0.0f32, 63.0].into_iter().enumerate() {
            draws.push(EntityDraw {
                pos: [-5.0 + i as f64 * 2.0, 63.0, 4.0],
                yaw: 180.0,
                light: [1.0, 1.0],
                tint: if roll > 0.0 { [1.0, 0.45, 0.45] } else { [1.0, 1.0, 1.0] },
                roll,
                kind: EntityDrawKind::Player {
                    skin: 0,
                    slim: false,
                    swing: 0.0,
                    attack_swing: 0.0,
                    pose: crate::render::PlayerPose::Standing,
                    skin_layers: 0xFF,
                    head_pitch: 0.0,
                    head_yaw: 0.0,
                    armor: [None; 4],
                    main_hand: None,
                    off_hand: None,
                    cape: 0,
                    elytra: 0,
                },
            });
        }
        // Two pigs: one looking straight ahead, one with its head turned the
        // vanilla maximum of 50° while the body stays put.
        for (i, head_yaw) in [0.0f32, 50.0].into_iter().enumerate() {
            draws.push(EntityDraw {
                pos: [0.0 + i as f64 * 2.0, 63.0, 4.0],
                yaw: 180.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Mob {
                    tex: pig_t,
                    model: MobModel::Pig,
                    swing: 0.0,
                    head_pitch: 0.0,
                    head_yaw,
                    scale: 1.0,
                },
            });
        }
        // A boat with a rider in the front seat, placed by the same
        // `seat_offset` the live client uses.
        let boat = [5.0f64, 63.0, 4.0];
        draws.push(EntityDraw {
            pos: boat,
            yaw: 180.0,
            light: [1.0, 1.0],
            tint: [1.0, 1.0, 1.0],
            roll: 0.0,
            kind: EntityDrawKind::Mob {
                tex: boat_t,
                model: MobModel::Boat,
                swing: 0.0,
                head_pitch: 0.0,
                head_yaw: 0.0,
                scale: 1.0,
            },
        });
        if renderer.has_skin(0) {
            let off = super::seat_offset("oak_boat", 0.6, 0);
            draws.push(EntityDraw {
                pos: super::rotate_offset(boat, off, 180.0),
                yaw: 180.0,
                light: [1.0, 1.0],
                tint: [1.0, 1.0, 1.0],
                roll: 0.0,
                kind: EntityDrawKind::Player {
                    skin: 0,
                    slim: false,
                    swing: 0.0,
                    attack_swing: 0.0,
                    pose: crate::render::PlayerPose::Sitting,
                    skin_layers: 0xFF,
                    head_pitch: 0.0,
                    head_yaw: -35.0,
                    armor: [None; 4],
                    main_hand: None,
                    off_hand: None,
                    cape: 0,
                    elytra: 0,
                },
            });
        }
        // A lightning bolt striking behind them.
        draws.push(EntityDraw {
            pos: [-9.0, 63.0, 13.0],
            yaw: 0.0,
            light: [1.0, 1.0],
            tint: [1.0, 1.0, 1.0],
            roll: 0.0,
            kind: EntityDrawKind::Lightning { seed: 0x5EED_1234, alpha: 1.0 },
        });

        let scene = SceneParams {
            cam_pos: [0.0, 64.8, -2.0],
            yaw: 0.0,
            pitch: 6.0,
            fov_deg: 75.0,
            daylight: 0.45,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.30, 0.34, 0.42],
            panorama: false,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering entity check")?;
        let img = renderer.read_screenshot().context("reading back entity check")?;
        let path = out_dir.join("menu_entities.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), draws = draws.len(), "entity check written");
        renderer.clear_meshes();
    }

    // Lighting check (0.53.0): a stone room lit only by a torch, so the light
    // ramp, the warm colour of block light and the smooth gradient across each
    // face are all visible in one shot. Rendered twice, smooth and flat, to
    // show what the setting actually changes.
    for (smooth, name) in [(true, "menu_light.png"), (false, "menu_light_flat.png")] {
        use crate::render::{EntityDraw, EntityDrawKind, MobModel};

        let mut world: std::collections::HashMap<(i32, i32, i32), crate::types::StateId> =
            std::collections::HashMap::new();
        let id = |name: &str, props: &[(&str, &str)]| -> crate::types::StateId {
            table.find_state(name, props).unwrap_or(0)
        };
        let air = id("air", &[]);
        let stone = id("stone", &[]);
        // A 15x7x15 room: floor, back wall and side walls, open toward the camera.
        for x in -7..8 {
            for z in -7..8 {
                world.insert((x, 62, z), stone);
                world.insert((x, 69, z), stone);
            }
        }
        for y in 63..69 {
            for x in -7..8 {
                world.insert((x, y, 7), stone);
            }
            for z in -7..8 {
                world.insert((-7, y, z), stone);
                world.insert((7, y, z), stone);
            }
        }
        // A torch on the back wall, and the block light it casts (a real client
        // would get these levels from the server; here they are the vanilla
        // falloff computed by hand).
        world.insert((0, 65, 6), id("wall_torch", &[("facing", "south")]));
        let mut light = std::collections::HashMap::new();
        for x in -7..8i32 {
            for y in 62..70i32 {
                for z in -7..8i32 {
                    let d = (x - 0).abs() + (y - 65).abs() + (z - 6).abs();
                    let level = (14 - d).clamp(0, 15) as u8;
                    light.insert((x, y, z), level << 4); // block light, no sky
                }
            }
        }
        let biome_tints = crate::types::BiomeTints::default();
        renderer.clear_meshes();
        for sy in 3..5 {
            for sz in -1..1 {
                for sx in -1..1 {
                    let pos = SectionPos { x: sx, y: sy, z: sz };
                    let mut blocks = Box::new([air; crate::types::PADDED_VOLUME]);
                    let mut lit = Box::new([0u8; crate::types::PADDED_VOLUME]);
                    for y in -1..=16i32 {
                        for z in -1..=16i32 {
                            for x in -1..=16i32 {
                                let key = (pos.x * 16 + x, pos.y * 16 + y, pos.z * 16 + z);
                                let i = crate::types::PaddedSnapshot::idx(x, y, z);
                                blocks[i] = world.get(&key).copied().unwrap_or(air);
                                lit[i] = light.get(&key).copied().unwrap_or(0);
                            }
                        }
                    }
                    let snap = crate::types::PaddedSnapshot { pos, blocks, light: lit, biome: 0 };
                    renderer.upload_mesh(mesh_section(&snap, &store, &table, &biome_tints, smooth));
                }
            }
        }
        // A pig in the middle, lit by the same torch as the room around it.
        let pig_t = 981u64;
        if let Ok(img) = pack.texture_png("entity/pig/pig_temperate") {
            renderer.ensure_skin(pig_t, &img);
        }
        let draws = vec![EntityDraw {
            pos: [0.0, 63.0, 2.0],
            yaw: 180.0,
            tint: [1.0, 1.0, 1.0],
            roll: 0.0,
            light: [10.0 / 15.0, 0.0],
            kind: EntityDrawKind::Mob {
                tex: pig_t,
                model: MobModel::Pig,
                swing: 0.0,
                head_pitch: 0.0,
                head_yaw: 0.0,
                scale: 1.0,
            },
        }];
        let scene = SceneParams {
            cam_pos: [0.0, 66.0, -8.0],
            yaw: 0.0,
            pitch: 12.0,
            fov_deg: 75.0,
            daylight: 0.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.02, 0.02, 0.03],
            panorama: false,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            // Night, no sky light: everything you see is the torch.
            lightmap: crate::render::LightmapParams {
                daylight: 0.0,
                gamma: 0.5,
                ..Default::default()
            },
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering light check")?;
        let img = renderer.read_screenshot().context("reading back light check")?;
        let path = out_dir.join(name);
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), smooth, "light check written");
        renderer.clear_meshes();
    }

    // Player check (0.53.0): every pose side by side, a cape, elytra wings, and
    // the animal equipment layers.
    {
        use crate::render::{EntityDraw, EntityDrawKind, MobModel, PlayerPose};

        let mut world: std::collections::HashMap<(i32, i32, i32), crate::types::StateId> =
            std::collections::HashMap::new();
        let id = |name: &str, props: &[(&str, &str)]| -> crate::types::StateId {
            table.find_state(name, props).unwrap_or(0)
        };
        let air = id("air", &[]);
        let stone = id("stone", &[]);
        for x in -14..15 {
            for z in -4..8 {
                world.insert((x, 62, z), stone);
            }
        }
        let biome_tints = crate::types::BiomeTints::default();
        renderer.clear_meshes();
        for sy in 3..5 {
            for sz in -1..1 {
                for sx in -1..1 {
                    let pos = SectionPos { x: sx, y: sy, z: sz };
                    let mut blocks = Box::new([air; crate::types::PADDED_VOLUME]);
                    for y in -1..=16i32 {
                        for z in -1..=16i32 {
                            for x in -1..=16i32 {
                                let key = (pos.x * 16 + x, pos.y * 16 + y, pos.z * 16 + z);
                                blocks[crate::types::PaddedSnapshot::idx(x, y, z)] =
                                    world.get(&key).copied().unwrap_or(air);
                            }
                        }
                    }
                    let snap = crate::types::PaddedSnapshot {
                        pos,
                        blocks,
                        light: Box::new([0xFF; crate::types::PADDED_VOLUME]),
                        biome: 0,
                    };
                    renderer.upload_mesh(mesh_section(&snap, &store, &table, &biome_tints, true));
                }
            }
        }
        // A stand-in cape sheet: the elytra texture doubles as a cape here so
        // the preview does not need a Mojang account's cape.
        let (cape_t, elytra_t) = (982u64, 983u64);
        if let Ok(img) = pack.texture_png("entity/equipment/wings/elytra") {
            renderer.ensure_skin(elytra_t, &super::pad_to_square(&img));
        }
        // Stand-in cape sheet: a real cape comes off a Mojang account, so the
        // preview paints its own into the cape's 64x32 UV region — a red cloth
        // with a gold border, which makes the shape and hang obvious.
        let cape_img = image::RgbaImage::from_fn(64, 64, |x, y| {
            let inside = (1..=22).contains(&x) && (1..=17).contains(&y);
            let border = inside && (x <= 2 || x >= 21 || y <= 2 || y >= 16);
            match (inside, border) {
                (true, true) => image::Rgba([230, 190, 60, 255]),
                (true, false) => image::Rgba([170, 30, 40, 255]),
                _ => image::Rgba([0, 0, 0, 0]),
            }
        });
        renderer.ensure_skin(cape_t, &cape_img);
        let mut draws: Vec<EntityDraw> = Vec::new();
        // Anything worn on the back is shown from behind (yaw 0 faces away
        // from the camera); the poses are shown from the front.
        let player = |x: f64, pose: PlayerPose, cape: u64, elytra: u64| EntityDraw {
            pos: [x, 63.0, 3.0],
            yaw: if cape != 0 || elytra != 0 { 0.0 } else { 180.0 },
            tint: [1.0, 1.0, 1.0],
            roll: 0.0,
            light: [1.0, 1.0],
            kind: EntityDrawKind::Player {
                skin: 0,
                slim: false,
                swing: 0.35,
                attack_swing: 0.0,
                pose,
                skin_layers: 0xFF,
                head_pitch: 0.0,
                head_yaw: 0.0,
                armor: [None; 4],
                main_hand: None,
                off_hand: None,
                cape,
                elytra,
            },
        };
        // Standing with a cape, then each flat pose, then elytra wings.
        draws.push(player(-12.0, PlayerPose::Standing, cape_t, 0));
        draws.push(player(-9.0, PlayerPose::Sneaking, cape_t, 0));
        draws.push(player(-6.0, PlayerPose::Swimming, 0, 0));
        draws.push(player(-3.0, PlayerPose::SpinAttack(0.8), 0, 0));
        draws.push(player(0.0, PlayerPose::Sleeping, 0, 0));
        draws.push(player(3.0, PlayerPose::FallFlying, 0, elytra_t));
        draws.push(player(6.0, PlayerPose::Standing, 0, elytra_t));

        // Animal equipment: a saddled pig and a carpeted llama, each drawn as
        // the animal plus its equipment layer, exactly like the live path.
        let mut layer = |x: f64, model: MobModel, base: &str, over: &str, key: u64| {
            if let Ok(img) = pack.texture_png(base) {
                renderer.ensure_skin(key, &img);
                draws.push(EntityDraw {
                    pos: [x, 63.0, 3.0],
                    yaw: 180.0,
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    light: [1.0, 1.0],
                    kind: EntityDrawKind::Mob {
                        tex: key, model, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0, scale: 1.0,
                    },
                });
            }
            if let Ok(img) = pack.texture_png(over) {
                renderer.ensure_skin(key + 1, &img);
                draws.push(EntityDraw {
                    pos: [x, 63.0, 3.0],
                    yaw: 180.0,
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    light: [1.0, 1.0],
                    kind: EntityDrawKind::Mob {
                        tex: key + 1, model, swing: 0.0, head_pitch: 0.0, head_yaw: 0.0,
                        scale: 1.03,
                    },
                });
            }
        };
        layer(9.0, MobModel::Pig, "entity/pig/pig_temperate",
              "entity/equipment/pig_saddle/saddle", 990);
        layer(12.0, MobModel::Wolf, "entity/wolf/wolf",
              "entity/equipment/wolf_body/armadillo_scute", 992);

        let scene = SceneParams {
            cam_pos: [0.0, 64.6, -6.0],
            yaw: 0.0,
            pitch: 6.0,
            fov_deg: 90.0,
            daylight: 1.0,
            fog_start: 200.0,
            fog_end: 400.0,
            sky_color: [0.30, 0.34, 0.42],
            panorama: false,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            lightmap: Default::default(),
            end_sky: false,
        };
        renderer.frame(&scene, &draws, None).context("rendering player check")?;
        let img = renderer.read_screenshot().context("reading back player check")?;
        let path = out_dir.join("menu_players.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), draws = draws.len(), "player check written");
        renderer.clear_meshes();
    }

    // End-sky check (0.53.0): an end-stone island under the End's own starfield
    // box, with no sun, moon or stars — the sky the dimension actually has.
    {
        let mut world: std::collections::HashMap<(i32, i32, i32), crate::types::StateId> =
            std::collections::HashMap::new();
        let id = |name: &str, props: &[(&str, &str)]| -> crate::types::StateId {
            table.find_state(name, props).unwrap_or(0)
        };
        // The star box needs its texture; the menu dump does not otherwise
        // load the sky.
        super::load_sky_textures(&mut pack, &mut renderer);
        let air = id("air", &[]);
        let end_stone = id("end_stone", &[]);
        let obsidian = id("obsidian", &[]);
        for x in -6..7 {
            for z in -6..7 {
                if x * x + z * z <= 36 {
                    world.insert((x, 62, z), end_stone);
                }
            }
        }
        for y in 63..68 {
            world.insert((0, y, 3), obsidian);
        }
        let biome_tints = crate::types::BiomeTints::default();
        renderer.clear_meshes();
        for sy in 3..5 {
            for sz in -1..1 {
                for sx in -1..1 {
                    let pos = SectionPos { x: sx, y: sy, z: sz };
                    let mut blocks = Box::new([air; crate::types::PADDED_VOLUME]);
                    for y in -1..=16i32 {
                        for z in -1..=16i32 {
                            for x in -1..=16i32 {
                                let key = (pos.x * 16 + x, pos.y * 16 + y, pos.z * 16 + z);
                                blocks[crate::types::PaddedSnapshot::idx(x, y, z)] =
                                    world.get(&key).copied().unwrap_or(air);
                            }
                        }
                    }
                    let snap = crate::types::PaddedSnapshot {
                        pos,
                        blocks,
                        light: Box::new([0xFF; crate::types::PADDED_VOLUME]),
                        biome: 0,
                    };
                    renderer.upload_mesh(mesh_section(&snap, &store, &table, &biome_tints, true));
                }
            }
        }
        let scene = SceneParams {
            cam_pos: [0.0, 65.0, -11.0],
            yaw: 0.0,
            pitch: 4.0,
            fov_deg: 80.0,
            daylight: 0.0,
            fog_start: 200.0,
            fog_end: 400.0,
            // The End's flat sky colour, behind the star box.
            sky_color: [0.0, 0.0, 0.0],
            panorama: false,
            outline: Vec::new(),
            crack: None,
            view_model: None,
            sky: None,
            // No sky light, and the End's own pale green-grey ramp.
            lightmap: crate::render::LightmapParams {
                daylight: 0.0,
                end: true,
                ..Default::default()
            },
            end_sky: true,
        };
        renderer.frame(&scene, &[], None).context("rendering end-sky check")?;
        let img = renderer.read_screenshot().context("reading back end-sky check")?;
        let path = out_dir.join("menu_endsky.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "end-sky check written");
        renderer.clear_meshes();
    }
    Ok(())
}

pub fn run_offscreen(opts: OffscreenOptions) -> Result<()> {
    // --- 1. Bake assets + renderer -----------------------------------------
    let t0 = Instant::now();
    info!(jar = %opts.app.mc_jar.display(), "offscreen: opening asset pack");
    let mut pack = AssetPack::open(&opts.app.mc_jar)?;
    let table = BlockTable::load_or_embedded(opts.app.blocks_report.as_deref())
        .context("loading block table")?;
    info!(states = table.len(), "offscreen: block table loaded");
    let (store, mut atlas) =
        BakedModelStore::bake_all(&mut pack, &table).context("baking models")?;
    // Animated block sprites (water, lava, fire, portal, …). Offscreen advances
    // them one tick per rendered frame, so a frame sequence shows the animation
    // actually running instead of 12 copies of frame 0.
    let mut atlas_anim =
        crate::assets::atlas::AtlasAnimator::new(std::mem::take(&mut atlas.animations));
    info!(
        elapsed_ms = t0.elapsed().as_millis() as u64,
        animated = atlas_anim.len(),
        "offscreen: models baked"
    );
    let item_icons = Arc::new(ItemIcons::bake(&mut pack, &table, &store, &atlas));

    let mut renderer =
        Renderer::new(RenderTarget::Offscreen { width: WIDTH, height: HEIGHT })
            .context("creating offscreen renderer")?;
    renderer.set_atlas(&atlas);
    // For the --hud-demo first-person view model: the held item needs the item
    // atlas and the arm needs the default Steve skin (key 0).
    if opts.hud_demo {
        renderer.ensure_item_atlas(&item_icons.image);
        if let Ok(steve) = pack.texture_png("entity/player/wide/steve")
            .or_else(|_| pack.texture_png("entity/steve"))
        {
            renderer.ensure_skin(0, &super::skins::normalize_skin(steve));
        }
        super::load_sky_textures(&mut pack, &mut renderer);
    }
    info!("offscreen: renderer ready ({WIDTH}x{HEIGHT})");

    let store = Arc::new(store);
    let table = Arc::new(table);
    // Biome tint table (built when the biome registry arrives) + climate maps.
    let grass_cm = pack.texture_png("colormap/grass").ok();
    let foliage_cm = pack.texture_png("colormap/foliage").ok();
    let mut biome_tints = Arc::new(crate::types::BiomeTints::default());

    // --- 2. Bridge ----------------------------------------------------------
    info!(address = %opts.app.bridge.address, "offscreen: spawning bridge");
    let (handle, rx) = spawn_bridge(opts.app.bridge.clone()).context("spawning bridge")?;

    // --- 3. Pump events + meshing until the world has arrived ---------------
    let (mesh_tx, mesh_rx) = crossbeam_channel::unbounded::<(SectionPos, MeshData)>();
    let mut mirror = WorldMirror::new();
    let mut player: Option<PlayerSnapshot> = None;
    let mut connected = false;
    let mut meshed_sections = 0usize; // uploads with a non-empty mesh
    let mut in_flight = 0usize;
    let mut hotbar: Vec<Option<ItemSnapshot>> = vec![None; 9];
    let mut selected_slot = 0u8;

    let start = Instant::now();
    let connect_deadline = start + CONNECT_TIMEOUT;
    let mesh_deadline = start + MESH_TIMEOUT;
    let mut last_log = Instant::now();
    let mut exec_sent = false;
    let mut settle_until = start;

    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(ev) => {
                mirror.apply(&ev);
                match &ev {
                    GameEvent::Connected { username } => {
                        info!(username, "offscreen: connected");
                        connected = true;
                        if !opts.exec.is_empty() && !exec_sent {
                            exec_sent = true;
                            for cmd in &opts.exec {
                                info!(cmd, "offscreen: exec");
                                handle.send(Command::Chat(cmd.clone()));
                            }
                            settle_until = Instant::now() + Duration::from_secs(3);
                        }
                    }
                    GameEvent::Disconnected { reason } => {
                        bail!("disconnected before rendering: {reason}");
                    }
                    GameEvent::PlayerState(p) => player = Some((**p).clone()),
                    GameEvent::Hotbar { slots, selected, .. } => {
                        hotbar = slots.to_vec();
                        selected_slot = *selected;
                    }
                    GameEvent::Biomes(infos) => {
                        biome_tints = Arc::new(crate::world::biome::build_biome_tints(
                            infos,
                            grass_cm.as_ref(),
                            foliage_cm.as_ref(),
                        ));
                        info!(biomes = infos.len(), "offscreen: biome tints built");
                        mirror.mark_all_dirty();
                    }
                    _ => {}
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                bail!("bridge event channel closed before the world was ready");
            }
        }

        // Schedule meshing for dirty sections, nearest to the player first.
        let center = player.as_ref().map_or([0.0, 80.0, 0.0], |p| p.pos);
        for pos in mirror.take_dirty(center, 16) {
            if let Some(snap) = mirror.snapshot27(pos) {
                let store = store.clone();
                let table = table.clone();
                let bt = biome_tints.clone();
                let tx = mesh_tx.clone();
                in_flight += 1;
                rayon::spawn(move || {
                    let mesh = mesh_section(&snap, &store, &table, &bt, true);
                    let _ = tx.send((pos, mesh));
                });
            }
        }
        // Drain finished meshes.
        while let Ok((_pos, mesh)) = mesh_rx.try_recv() {
            in_flight -= 1;
            if !mesh.is_empty() {
                meshed_sections += 1;
            }
            renderer.upload_mesh(mesh);
        }
        for pos in mirror.take_removed() {
            renderer.remove_mesh(pos);
        }

        let now = Instant::now();
        if last_log.elapsed() > Duration::from_secs(5) {
            last_log = now;
            info!(
                connected,
                meshed_sections,
                in_flight,
                sections = mirror.section_count(),
                player_seen = player.is_some(),
                "offscreen: waiting for world"
            );
        }

        if !connected {
            if now > connect_deadline {
                bail!(
                    "timed out ({}s) waiting for Connected from {}",
                    CONNECT_TIMEOUT.as_secs(),
                    opts.app.bridge.address
                );
            }
            continue;
        }
        if meshed_sections >= opts.wait_sections
            && mirror.is_dirty_empty()
            && in_flight == 0
            && player.is_some()
            && now >= settle_until
        {
            info!(
                meshed_sections,
                sections = mirror.section_count(),
                elapsed_ms = start.elapsed().as_millis() as u64,
                "offscreen: world ready"
            );
            break;
        }
        if now > mesh_deadline {
            bail!(
                "timed out ({}s) waiting for the world: meshed {}/{} sections{}{}{}",
                MESH_TIMEOUT.as_secs(),
                meshed_sections,
                opts.wait_sections,
                if meshed_sections < opts.wait_sections { " (below threshold)" } else { "" },
                if !mirror.is_dirty_empty() || in_flight > 0 {
                    " (mesh queue not drained)"
                } else {
                    ""
                },
                if player.is_none() { " (no PlayerState received)" } else { "" },
            );
        }
    }

    // --- 4. Render orbit frames ---------------------------------------------
    let p = player.as_ref().context("no player snapshot after world wait")?;
    let cam_pos = [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]];
    std::fs::create_dir_all(&opts.out_dir)
        .with_context(|| format!("creating {}", opts.out_dir.display()))?;

    // Optional egui HUD (crosshair, hotbar with item icons, chat) for headless
    // verification of the in-game overlay.
    let egui_ctx = opts.hud_demo.then(egui::Context::default);
    let mut hud = opts
        .hud_demo
        .then(|| Hud::new(String::new(), true, "Dolphin".into()));
    let lang = crate::assets::Lang::load(
        &mut pack,
        opts.app.assets_dir.as_deref(),
        opts.app.asset_index.as_deref(),
        "en_us",
    );
    let mut skins = super::skins::SkinManager::new(opts.app.assets_dir.as_deref());
    let mcui = match &egui_ctx {
        Some(ctx) => Some(
            super::mcui::McUi::load(
                &mut pack,
                ctx,
                opts.app.assets_dir.as_deref(),
                opts.app.asset_index.as_deref(),
            )
            .context("loading vanilla GUI assets")?,
        ),
        None => None,
    };
    let icon_tex = egui_ctx.as_ref().map(|ctx| {
        let img = &item_icons.image;
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [img.width() as usize, img.height() as usize],
            img.as_raw(),
        );
        ctx.load_texture("item-icons", color, egui::TextureOptions::NEAREST)
    });

    // Demo potion-effect icons for the --hud-demo effects row.
    // The handles must outlive the frames that use them: egui frees a texture
    // as soon as its last handle drops, which used to leave the demo effect
    // icons missing (the renderer logged "Missing texture" every frame).
    let mut effect_handles: Vec<egui::TextureHandle> = Vec::new();
    let effect_demo: Vec<super::hud::EffectHud> = match &egui_ctx {
        Some(ctx) => [("speed", 1u32, Some(52i32)), ("strength", 0, Some(600)), ("regeneration", 2, Some(8))]
            .iter()
            .map(|&(name, amp, secs)| {
                let icon = pack.texture_png(&format!("mob_effect/{name}")).ok().map(|img| {
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [img.width() as usize, img.height() as usize],
                        img.as_raw(),
                    );
                    let handle =
                        ctx.load_texture(format!("effect-{name}"), color, egui::TextureOptions::NEAREST);
                    let id = handle.id();
                    effect_handles.push(handle);
                    id
                });
                super::hud::EffectHud { icon, amplifier: amp, remaining_secs: secs }
            })
            .collect(),
        None => Vec::new(),
    };

    // For the --hud-demo view model: 3D geometry of a held block (stone).
    let demo_block_quads: Option<Vec<([f32; 3], [f32; 2])>> = opts.hud_demo.then(|| {
        let sid = (0..table.len() as crate::types::StateId)
            .find(|&id| table.entry(id).map(|e| e.short_name == "stone").unwrap_or(false));
        sid.map(|sid| {
            let model = store.get(sid);
            let mut out = Vec::new();
            for q in &model.quads {
                for &k in &[0usize, 1, 2, 0, 2, 3] {
                    let v = q.verts[k];
                    out.push(([v[0] - 0.5, v[1] - 0.5, v[2] - 0.5], q.uvs[k]));
                }
            }
            out
        })
    }).flatten();

    let mut last_frame: Option<image::RgbaImage> = None;
    for i in 0..opts.frames {
        // One animation tick per frame.
        atlas_anim.tick(i as u64 * 2, |u| renderer.update_atlas_rect(u.x, u.y, u.w, u.h, u.rgba));
        // Keep the HUD state live (hotbar can arrive after world-ready).
        while let Ok(ev) = rx.try_recv() {
            if let GameEvent::Hotbar { slots, selected, .. } = &ev {
                hotbar = slots.to_vec();
                selected_slot = *selected;
            }
        }
        let scene = SceneParams {
            cam_pos,
            yaw: i as f32 * 360.0 / opts.frames.max(1) as f32,
            pitch: 10.0,
            fov_deg: 70.0,
            daylight: 1.0,
            fog_start: 96.0,
            fog_end: 192.0,
            sky_color: if opts.hud_demo {
                super::overworld_sky_color(6000 + i as i64 * 3000)
            } else {
                [0.47, 0.65, 1.0]
            },
            panorama: false,
            outline: Vec::new(),
            crack: None,
            // Demo the first-person hand + held item (selected hotbar slot).
            view_model: opts.hud_demo.then(|| crate::render::ViewModel {
                skin: 0,
                slim: false,
                item_uv: hotbar
                    .get(selected_slot as usize)
                    .and_then(|s| s.as_ref())
                    .and_then(|it| item_icons.uv(&it.item)),
                item_is_block: demo_block_quads.is_some(),
                block_quads: demo_block_quads.clone(),
                off_hand_uv: item_icons.uv("shield"),
                off_hand_is_block: false,
                swing: (i as f32 / opts.frames.max(1) as f32).fract(),
                equip: 1.0,
                bob_phase: i as f32 * 0.6,
                bob: 1.0,
                // Demo the eat/use pose on the back half of the frame sweep.
                using: if i * 2 >= opts.frames { 1.0 } else { 0.0 },
                use_phase: i as f32 * 0.15,
                left_handed: false,
                light: [1.0, 1.0],
            }),
            // Demo the celestial sky, sweeping time across frames (noon → night)
            // so the sun/moon/stars and sky color can be eyeballed headlessly.
            sky: opts
                .hud_demo
                .then(|| super::sky_params_of(6000 + i as i64 * 3000, i as f32 * 2.0)),
            // The orbit shot walks the day cycle, so the light ramp follows it.
            lightmap: crate::render::LightmapParams {
                daylight: super::daylight_factor(6000 + i as i64 * 3000),
                ..Default::default()
            },
            end_sky: false,
        };
        let egui_frame = match (&egui_ctx, &mut hud, &icon_tex, &mcui) {
            (Some(ctx), Some(hud), Some(tex), Some(mcui)) => {
                ctx.set_pixels_per_point(1.0);
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::pos2(0.0, 0.0),
                        egui::vec2(WIDTH as f32, HEIGHT as f32),
                    )),
                    ..Default::default()
                };
                ctx.begin_pass(raw);
                let hud_state = HudState {
                    fps: 60.0,
                    connected: true,
                    health: 20.0,
                    food: 18,
                    xp_level: 3,
                    hotbar: hotbar.clone(),
                    selected_slot,
                    // Demo the just-selected item-name popup above the hotbar.
                    item_name: vec![ChatSpan::plain("Stone")],
                    item_name_alpha: 1.0,
                    effects: effect_demo.clone(),
                    icons: Some((tex.id(), item_icons.clone())),
                    // Cycle the new screen overlays across frames so each can be
                    // eyeballed: lava on frame%4==1, blindness on frame%4==2.
                    eyes_in_lava: i % 4 == 1,
                    dark_vignette: if i % 4 == 2 { 0.7 } else { 0.0 },
                    // Demo effect-tinted hearts: poison on frame%3==1, wither==2.
                    poisoned: i % 3 == 1,
                    withered: i % 3 == 2,
                    // Demo absorption (gold) hearts: a few points on even frames.
                    absorption: if i % 2 == 0 { 6.0 } else { 0.0 },
                    // Demo the freeze frost overlay (frame%5==3) + fully-frozen
                    // cyan hearts, and the pumpkin overlay (frame%5==4).
                    freeze: if i % 5 == 3 { 1.0 } else { 0.0 },
                    pumpkin: i % 5 == 4,
                    // Demo the spyglass scope on frame%5==0 (skip frame 0 itself).
                    spyglass: i > 0 && i % 5 == 0,
                    // Demo the hotbar cooldown sweep on the stone slot, shrinking
                    // across the frame sweep.
                    cooldowns: [(
                        "stone".to_string(),
                        1.0 - (i as f32 / opts.frames.max(1) as f32),
                    )]
                    .into_iter()
                    .collect(),
                    ..Default::default()
                };
                let mut settings = crate::settings::GameSettings::default();
                let _ = hud.run(ctx, mcui, &hud_state, &mut settings, &mut skins, &lang);
                let output = ctx.end_pass();
                Some(EguiFrame {
                    textures_delta: output.textures_delta,
                    primitives: ctx.tessellate(output.shapes, output.pixels_per_point),
                    pixels_per_point: output.pixels_per_point,
                })
            }
            _ => None,
        };
        // Demo rain: thin falling streaks around the camera (verifies the
        // weather look; the live app drives these from server weather events).
        let rain_demo: Vec<crate::render::EntityDraw> = if opts.hud_demo {
            use crate::render::{EntityDraw, EntityDrawKind};
            let mut rng = 0x1234_5678_9abc_def0u64 ^ (i as u64).wrapping_mul(0x9E37_79B9);
            let mut r = || {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                ((rng >> 40) as f32) / (1u64 << 24) as f32
            };
            (0..160)
                .map(|_| {
                    let ang = r() as f64 * std::f64::consts::TAU;
                    let rad = (r() as f64).sqrt() * 12.0;
                    EntityDraw {
                        pos: [
                            cam_pos[0] + ang.cos() * rad,
                            cam_pos[1] - 2.0 + r() as f64 * 12.0,
                            cam_pos[2] + ang.sin() * rad,
                        ],
                        yaw: 0.0,
                        light: [1.0, 1.0],
                        tint: [1.0, 1.0, 1.0],
                        roll: 0.0,
                        kind: EntityDrawKind::Box { w: 0.02, h: 0.7, color: [0.55, 0.60, 0.72] },
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        // Demo a spinning dropped 3D block (stone) a few blocks ahead.
        let mut demo_entities = rain_demo;
        if opts.hud_demo {
            if let Some(quads) = &demo_block_quads {
                demo_entities.push(crate::render::EntityDraw {
                    pos: [cam_pos[0], cam_pos[1] + 0.3, cam_pos[2] + 2.5],
                    yaw: i as f32 * 45.0,
                    light: [1.0, 1.0],
                    tint: [1.0, 1.0, 1.0],
                    roll: 0.0,
                    kind: crate::render::EntityDrawKind::ItemBlock { quads: quads.clone() },
                });
            }
        }
        let stats = renderer
            .frame(&scene, &demo_entities, egui_frame)
            .with_context(|| format!("rendering frame {i}"))?;
        let img = renderer
            .read_screenshot()
            .with_context(|| format!("reading back frame {i}"))?;
        let path = opts.out_dir.join(format!("frame_{i:02}.png"));
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(
            frame = i,
            yaw = scene.yaw,
            sections_drawn = stats.sections_drawn,
            sections_total = stats.sections_total,
            draw_calls = stats.draw_calls,
            path = %path.display(),
            "offscreen: frame written"
        );
        last_frame = Some(img);
    }

    // --- 5. Sanity: the last frame must not be a single flat color ----------
    let img = last_frame.context("no frames rendered (--frames 0?)")?;
    let mut first: Option<[u8; 4]> = None;
    let mut distinct = false;
    for px in img.pixels() {
        match first {
            None => first = Some(px.0),
            Some(f) if f != px.0 => {
                distinct = true;
                break;
            }
            _ => {}
        }
    }
    if !distinct {
        bail!("uniform frame: every pixel of the last frame is {:?}", first);
    }
    info!("offscreen: last frame is non-uniform (sanity ok)");

    // --- 6. Chat + disconnect ------------------------------------------------
    handle.send(Command::Chat("DolphinClient rust ok".into()));
    std::thread::sleep(Duration::from_millis(500));
    handle.send(Command::Disconnect);
    drop(handle);
    info!("offscreen: done");
    Ok(())
}
