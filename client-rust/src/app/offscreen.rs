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
    let (store, atlas) = BakedModelStore::bake_all(&mut pack, &table).context("baking models")?;
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
        let state = HudState {
            connected: pause || ingame,
            menu_time: 0.6,
            hotbar: vec![None; 9],
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
        };
        // Two players 3 blocks ahead: one facing the camera, one turned, mid-step.
        let players = [
            EntityDraw {
                pos: [-0.6, 64.0, 3.0],
                yaw: 180.0,
                tint: [1.0, 1.0, 1.0],
                kind: EntityDrawKind::Player {
                    skin: 0,
                    slim: false,
                    swing: 0.6,
                    attack_swing: 0.0,
                    sneaking: false,
                    skin_layers: 0xFF,
                    head_pitch: 0.0,
                    // Full diamond armor to eyeball all four layers.
                    armor: [
                        Some(ArmorMaterial::Diamond),
                        Some(ArmorMaterial::Diamond),
                        Some(ArmorMaterial::Diamond),
                        Some(ArmorMaterial::Diamond),
                    ],
                    main_hand: item_icons.uv("diamond_sword"),
                    off_hand: item_icons.uv("shield"),
                },
            },
            EntityDraw {
                pos: [0.7, 64.0, 3.2],
                yaw: 150.0,
                tint: [1.0, 1.0, 1.0],
                kind: EntityDrawKind::Player {
                    skin: 0,
                    slim: true,
                    swing: -0.4,
                    attack_swing: 0.8,
                    sneaking: true,
                    skin_layers: 0xFF,
                    head_pitch: 10.0,
                    // Iron helmet + chestplate only (partial armor).
                    armor: [
                        Some(ArmorMaterial::Iron),
                        Some(ArmorMaterial::Iron),
                        None,
                        None,
                    ],
                    main_hand: item_icons.uv("bow"),
                    off_hand: None,
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
        let mobs: [(&str, MobModel); 5] = [
            ("entity/creeper/creeper", MobModel::Creeper),
            ("entity/pig/pig_temperate", MobModel::Pig),
            ("entity/sheep/sheep", MobModel::Sheep),
            ("entity/chicken/chicken_temperate", MobModel::Chicken),
            ("entity/cow/cow_temperate", MobModel::Cow),
        ];
        let mut draws = Vec::new();
        for (i, (path, model)) in mobs.iter().enumerate() {
            if let Ok(img) = pack.texture_png(path) {
                let key = 100 + i as u64;
                renderer.ensure_skin(key, &img);
                let x = -3.2 + i as f32 * 1.6;
                draws.push(EntityDraw {
                    pos: [x as f64, 64.0, 4.0],
                    yaw: 160.0,
                    tint: [1.0, 1.0, 1.0],
                    kind: EntityDrawKind::Mob { tex: key, model: *model, swing: 0.5, head_pitch: 0.0 },
                });
            }
        }
        let scene = SceneParams {
            cam_pos: [0.0, 65.0, 0.0],
            yaw: 0.0,
            pitch: 6.0,
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
        };
        renderer.frame(&scene, &draws, None).context("rendering mob check")?;
        let img = renderer.read_screenshot().context("reading back mob check")?;
        let path = out_dir.join("menu_mobs.png");
        img.save(&path).with_context(|| format!("saving {}", path.display()))?;
        info!(path = %path.display(), "mob check written");
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
    let (store, atlas) = BakedModelStore::bake_all(&mut pack, &table).context("baking models")?;
    info!(elapsed_ms = t0.elapsed().as_millis() as u64, "offscreen: models baked");
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
                let tx = mesh_tx.clone();
                in_flight += 1;
                rayon::spawn(move || {
                    let mesh = mesh_section(&snap, &store, &table);
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
                left_handed: false,
            }),
            // Demo the celestial sky, sweeping time across frames (noon → night)
            // so the sun/moon/stars and sky color can be eyeballed headlessly.
            sky: opts
                .hud_demo
                .then(|| super::sky_params_of(6000 + i as i64 * 3000, i as f32 * 2.0)),
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
                    icons: Some((tex.id(), item_icons.clone())),
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
                        tint: [1.0, 1.0, 1.0],
                        kind: EntityDrawKind::Box { w: 0.02, h: 0.7, color: [0.55, 0.60, 0.72] },
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let stats = renderer
            .frame(&scene, &rain_demo, egui_frame)
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
