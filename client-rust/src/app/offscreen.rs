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
use crate::bridge::events::{Command, GameEvent, ItemSnapshot, PlayerSnapshot};
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
    let _ = store;

    let mut renderer = Renderer::new(RenderTarget::Offscreen { width: WIDTH, height: HEIGHT })
        .context("creating offscreen renderer")?;
    renderer.set_atlas(&atlas);
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("creating {}", out_dir.display()))?;

    // A calm dusk-ish sky behind the menu.
    let scene = SceneParams {
        cam_pos: [8.0, 80.0, 8.0],
        yaw: 30.0,
        pitch: 8.0,
        fov_deg: 70.0,
        daylight: 0.9,
        fog_start: 96.0,
        fog_end: 192.0,
        sky_color: [0.47, 0.65, 1.0],
    };

    let ctx = egui::Context::default();
    // Headless RawInput has no clock, so egui's Area fade-in animation would be
    // stuck at 0 opacity (transparent). Disable animations so a single render
    // shows the menu at full opacity, exactly as the live app does after its
    // first few frames.
    ctx.all_styles_mut(|s| s.animation_time = 0.0);
    // (name, screen index, in-game pause menu?)
    let shots: [(&str, u8, bool); 4] = [
        ("title", 0, false),
        ("multiplayer", 1, false),
        ("options", 2, false),
        ("pause", 0, true),
    ];
    for (name, screen, pause) in shots {
        let mut hud = Hud::default();
        hud.debug_force(screen, pause);
        let state = HudState {
            connected: pause,
            fov: 70.0,
            sensitivity: 0.15,
            render_distance: 12,
            menu_time: 0.6,
            hotbar: vec![None; 9],
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
            let _ = hud.run(&ctx, &state);
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
                    GameEvent::Hotbar { slots, selected } => {
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
    let mut hud = opts.hud_demo.then(Hud::default);
    let icon_tex = egui_ctx.as_ref().map(|ctx| {
        let img = &item_icons.image;
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [img.width() as usize, img.height() as usize],
            img.as_raw(),
        );
        ctx.load_texture("item-icons", color, egui::TextureOptions::NEAREST)
    });

    let mut last_frame: Option<image::RgbaImage> = None;
    for i in 0..opts.frames {
        // Keep the HUD state live (hotbar can arrive after world-ready).
        while let Ok(ev) = rx.try_recv() {
            if let GameEvent::Hotbar { slots, selected } = &ev {
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
            sky_color: [0.47, 0.65, 1.0],
        };
        let egui_frame = match (&egui_ctx, &mut hud, &icon_tex) {
            (Some(ctx), Some(hud), Some(tex)) => {
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
                    hotbar: hotbar.clone(),
                    selected_slot,
                    icons: Some((tex.id(), item_icons.clone())),
                    ..Default::default()
                };
                let _ = hud.run(ctx, &hud_state);
                let output = ctx.end_pass();
                Some(EguiFrame {
                    textures_delta: output.textures_delta,
                    primitives: ctx.tessellate(output.shapes, output.pixels_per_point),
                    pixels_per_point: output.pixels_per_point,
                })
            }
            _ => None,
        };
        let stats = renderer
            .frame(&scene, &[], egui_frame)
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
