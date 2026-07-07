//! Windowed app: winit event loop, input → Commands, per-frame pipeline:
//! drain GameEvents → WorldMirror → schedule meshing (rayon, nearest-first,
//! ≤8/frame) → upload finished meshes → render → egui HUD.
//!
//! Input map (v1): WASD move, Space jump, Shift sneak, Ctrl sprint, mouse look
//! (pointer-lock while focused; Esc releases + opens chat-less pause overlay),
//! left click mine (raycast ≤ 5 blocks), right click interact, 1-9 hotbar,
//! T/Enter chat, F3 debug overlay, F2 screenshot (windowed: read via surface —
//! v1 skip, log a note).

pub mod hud;
pub mod mcui;
pub mod offscreen;

use crate::assets::AssetPack;
use crate::assets::atlas::Atlas;
use crate::audio::AudioEngine;
use crate::assets::blockmap::BlockTable;
use crate::assets::items::ItemIcons;
use crate::bridge::events::{
    AccountConfig, BridgeOptions, Command, EntitySnapshot, GameEvent, ItemSnapshot, PlayerSnapshot,
};
use crate::bridge::{GameHandle, spawn_bridge};
use crate::models::BakedModelStore;
use crate::render::{
    EguiFrame, EntityDraw, EntityDrawKind, RenderTarget, Renderer, SceneParams, camera,
};
use crate::settings::GameSettings;
use crate::types::{ChunkPos, MeshData, SectionPos};
use crate::world::WorldMirror;
use crate::world::mesher::mesh_section;
use anyhow::{Context as _, Result};
use crossbeam_channel::{Receiver, Sender};
use hud::{Hud, HudAction, HudState};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
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
    /// `.minecraft/assets` dir enabling sound (`None` = silent).
    pub assets_dir: Option<PathBuf>,
    /// Asset-index id paired with `assets_dir`.
    pub asset_index: Option<String>,
}

const MESH_BUDGET_PER_FRAME: usize = 8;
const FPS_WINDOW: usize = 30;

/// The dolphin logo as the window/taskbar icon (embedded PNG).
fn load_window_icon() -> Option<winit::window::Icon> {
    let img = image::load_from_memory(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../assets/brand/dolphin-64.png"
    )))
    .ok()?
    .to_rgba8();
    let (w, h) = (img.width(), img.height());
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// Blocks until the window closes or the connection drops fatally.
pub fn run_windowed(opts: AppOptions) -> Result<()> {
    // Bake assets before opening the window (slow, one-off).
    let t0 = Instant::now();
    info!(jar = %opts.mc_jar.display(), "app: opening asset pack");
    let mut pack = AssetPack::open(&opts.mc_jar)?;
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

    // Vanilla GUI assets (bitmap font, widget sprites, backgrounds) for the
    // Minecraft-look menus.
    let egui_ctx = egui::Context::default();
    let mcui = Arc::new(
        mcui::McUi::load(&mut pack, &egui_ctx).context("loading vanilla GUI assets")?,
    );

    // Always start on the title screen (like vanilla Minecraft) — the menu
    // drives the connect. The Multiplayer screen is pre-filled with the
    // launcher/CLI `--server` and (offline only) the username.
    let (offline, player_name) = match &opts.bridge.account {
        AccountConfig::Offline(name) => (true, name.clone()),
        AccountConfig::Session { username, .. } => (false, username.clone()),
        AccountConfig::Microsoft(email) => (false, email.clone()),
    };
    let default_server = opts.bridge.address.clone();
    let mut settings = GameSettings::load_or_seed(opts.render_distance);
    settings.clamp();

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

    let (mesh_tx, mesh_rx) = crossbeam_channel::unbounded::<(SectionPos, MeshData)>();
    let mut app = App {
        opts,
        table: Arc::new(table),
        store: Arc::new(store),
        atlas,
        item_icons,
        icon_tex: None,
        window: None,
        renderer: None,
        egui_ctx,
        mcui,
        egui_state: None,
        hud: Hud::new(default_server, offline, player_name),
        mirror: WorldMirror::new(),
        bridge: None,
        mesh_tx,
        mesh_rx,
        in_flight: 0,
        player: None,
        entities: Vec::new(),
        own_name: None,
        connected: false,
        disconnect_reason: None,
        connect_deadline: None,
        returning_to_menu: false,
        hotbar: vec![None; 9],
        selected_slot: 0,
        daylight: 1.0,
        audio,
        settings,
        settings_dirty: true,
        last_frame_end: Instant::now(),
        bob_phase: 0.0,
        keys: HashSet::new(),
        last_move: (0, 0, false),
        sneaking: false,
        yaw: 0.0,
        pitch: 20.0,
        dir_synced: false,
        last_sent_dir: None,
        pending_mouse: (0.0, 0.0),
        grabbed: false,
        frame_times: VecDeque::with_capacity(FPS_WINDOW + 1),
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

struct App {
    opts: AppOptions,
    table: Arc<BlockTable>,
    store: Arc<BakedModelStore>,
    atlas: Atlas,
    item_icons: Arc<ItemIcons>,
    /// egui texture for the item-icon atlas; created lazily on the first frame.
    icon_tex: Option<egui::TextureHandle>,

    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    egui_ctx: egui::Context,
    /// Vanilla GUI assets (font/sprites) shared by all menu drawing.
    mcui: Arc<mcui::McUi>,
    egui_state: Option<egui_winit::State>,
    hud: Hud,

    mirror: WorldMirror,
    bridge: Option<(GameHandle, Receiver<GameEvent>)>,
    mesh_tx: Sender<(SectionPos, MeshData)>,
    mesh_rx: Receiver<(SectionPos, MeshData)>,
    in_flight: usize,

    player: Option<PlayerSnapshot>,
    entities: Vec<EntitySnapshot>,
    own_name: Option<String>,
    connected: bool,
    disconnect_reason: Option<String>,
    /// Backstop: give up on a connect attempt that produces no event at all
    /// (e.g. azalea hanging silently after a failed session-server auth).
    connect_deadline: Option<Instant>,
    /// Set when the user chose "Disconnect" from the pause menu: the resulting
    /// Disconnected event returns to the title screen instead of the error box.
    returning_to_menu: bool,
    hotbar: Vec<Option<ItemSnapshot>>,
    selected_slot: u8,
    daylight: f32,

    /// Sound engine (rodio). `None` when there's no audio device or no assets.
    audio: Option<AudioEngine>,

    /// Persistent, vanilla-style options (Video/Controls/Chat). Drives the
    /// camera, renderer, GUI scale, FPS cap and more each frame.
    settings: GameSettings,
    /// Set when a setting that the renderer/window must apply changed
    /// (vsync, fullscreen); applied at the top of the next frame.
    settings_dirty: bool,
    /// End-of-frame instant, for the software FPS cap when vsync is off.
    last_frame_end: Instant,
    /// Accumulated view-bob phase (advances while walking).
    bob_phase: f32,

    keys: HashSet<KeyCode>,
    last_move: (i8, i8, bool),
    sneaking: bool,
    yaw: f32,
    pitch: f32,
    /// Camera direction initialized from the first PlayerState.
    dir_synced: bool,
    last_sent_dir: Option<(f32, f32)>,
    pending_mouse: (f64, f64),
    grabbed: bool,

    frame_times: VecDeque<Instant>,
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
        if self.bridge.is_some() {
            self.set_grab(true);
        }
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
            WindowEvent::Focused(false) => {
                // Drop all movement state; keys released while unfocused are lost.
                self.keys.clear();
                self.set_grab(false);
                self.push_move_if_changed();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.on_key(code, event.state, event.repeat, consumed);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed && !consumed {
                    self.on_click(button);
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

    fn set_grab(&mut self, on: bool) {
        let Some(window) = &self.window else { return };
        if on == self.grabbed {
            return;
        }
        if on {
            let res = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            match res {
                Ok(()) => {
                    window.set_cursor_visible(false);
                    self.grabbed = true;
                }
                Err(e) => warn!("app: cursor grab failed: {e}"),
            }
        } else {
            if let Err(e) = window.set_cursor_grab(CursorGrabMode::None) {
                warn!("app: cursor ungrab failed: {e}");
            }
            window.set_cursor_visible(true);
            self.grabbed = false;
        }
    }

    // -- input -----------------------------------------------------------------

    fn on_key(&mut self, code: KeyCode, state: ElementState, repeat: bool, consumed: bool) {
        let pressed = state.is_pressed();

        // Overlay-independent toggles (never text input keys).
        if pressed && !repeat && code == KeyCode::F3 {
            self.hud.show_debug = !self.hud.show_debug;
            return;
        }
        if pressed && !repeat && code == KeyCode::Escape {
            // Chat closes via its own handler in `hud.run`. In a pre-game menu
            // the screens handle Esc themselves. In game, Esc toggles the
            // Minecraft-style pause menu (and releases/grabs the mouse).
            if !self.hud.chat_open && self.connected && self.disconnect_reason.is_none() {
                let grab = self.hud.toggle_pause();
                self.set_grab(grab);
                if !grab {
                    self.keys.clear();
                    self.push_move_if_changed();
                }
            }
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
            if let Some(slot) = hotbar_slot(code) {
                self.selected_slot = slot;
                self.send_cmd(Command::SelectHotbar(slot));
                return;
            }
            match code {
                KeyCode::Space => self.send_cmd(Command::Jump(true)),
                KeyCode::KeyT | KeyCode::Enter if self.connected => {
                    self.hud.chat_open = true;
                    self.keys.clear();
                    self.push_move_if_changed();
                    self.set_grab(false);
                }
                _ => {}
            }
        } else if !pressed && code == KeyCode::Space {
            self.send_cmd(Command::Jump(false));
        }
    }

    fn on_click(&mut self, button: MouseButton) {
        if !self.grabbed {
            // Click into the world: re-capture the mouse (only in game, not
            // while a menu / pause overlay is up).
            if self.connected && self.disconnect_reason.is_none() && !self.hud.is_paused() {
                self.set_grab(true);
            }
            return;
        }
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
                if let Some((pos, _)) = hit {
                    self.send_cmd(Command::Mine(pos));
                }
            }
            MouseButton::Right => {
                if let Some((pos, _)) = hit {
                    self.send_cmd(Command::Interact(pos));
                }
            }
            _ => {}
        }
    }

    /// Nearest remote entity whose hitbox the view ray enters within `reach`
    /// blocks: `(bridge id, ray distance)`.
    fn entity_hit(&self, eye: [f64; 3], dir: [f64; 3], reach: f64) -> Option<(u64, f64)> {
        let mut best: Option<(u64, f64)> = None;
        for e in &self.entities {
            if e.is_player && e.name.is_some() && e.name == self.own_name {
                continue;
            }
            let hw = e.width as f64 / 2.0;
            let min = [e.pos[0] - hw, e.pos[1], e.pos[2] - hw];
            let max = [e.pos[0] + hw, e.pos[1] + e.height as f64, e.pos[2] + hw];
            if let Some(t) = ray_aabb(eye, dir, min, max)
                && t <= reach
                && best.is_none_or(|(_, bt)| t < bt)
            {
                best = Some((e.id, t));
            }
        }
        best
    }

    /// Compute the Move command from held keys; send only on change.
    fn push_move_if_changed(&mut self) {
        // No movement while a text field owns the keyboard or the game is
        // paused (pause menu open) — same as vanilla.
        let active = !self.hud.wants_keyboard() && !self.hud.is_paused();
        let mut forward = 0i8;
        let mut strafe = 0i8;
        if active {
            if self.keys.contains(&KeyCode::KeyW) {
                forward += 1;
            }
            if self.keys.contains(&KeyCode::KeyS) {
                forward -= 1;
            }
            if self.keys.contains(&KeyCode::KeyD) {
                strafe += 1;
            }
            if self.keys.contains(&KeyCode::KeyA) {
                strafe -= 1;
            }
        }
        let sprint = forward > 0
            && (self.keys.contains(&KeyCode::ControlLeft)
                || self.keys.contains(&KeyCode::ControlRight));
        let mv = (forward, strafe, sprint);
        if mv != self.last_move {
            self.last_move = mv;
            self.send_cmd(Command::Move { forward, strafe, sprint });
        }
        // Sneak state (Shift), also change-triggered.
        let sneak = active
            && (self.keys.contains(&KeyCode::ShiftLeft)
                || self.keys.contains(&KeyCode::ShiftRight));
        if sneak != self.sneaking {
            self.sneaking = sneak;
            self.send_cmd(Command::Sneak(sneak));
        }
    }

    // -- per-frame pipeline ------------------------------------------------------

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        self.frame_counter += 1;
        if self.settings_dirty {
            self.apply_settings();
            self.settings_dirty = false;
        }
        self.drain_game_events();
        self.pump_meshing();
        self.apply_mouse_look();
        self.push_move_if_changed();

        let (Some(window), Some(egui_state)) = (self.window.clone(), self.egui_state.as_mut())
        else {
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

        let hud_state = HudState {
            fps: fps_of(&self.frame_times),
            pos: self.player.as_ref().map_or([0.0; 3], |p| p.pos),
            yaw: self.yaw,
            pitch: self.pitch,
            health: self.player.as_ref().map_or(0.0, |p| p.health),
            food: self.player.as_ref().map_or(0, |p| p.food),
            xp_level: self.player.as_ref().map_or(0, |p| p.xp_level),
            hotbar: self.hotbar.clone(),
            selected_slot: self.selected_slot,
            icons,
            sections_drawn: self.last_stats.0,
            sections_total: self.last_stats.1,
            mesh_queue: self.in_flight,
            connected: self.connected,
            connecting: self.bridge.is_some() && !self.connected,
            disconnect_reason: self.disconnect_reason.clone(),
            menu_time: self.start.elapsed().as_secs_f32(),
        };
        let raw_input = egui_state.take_egui_input(&window);
        self.egui_ctx.begin_pass(raw_input);
        let actions = self.hud.run(&self.egui_ctx, &self.mcui, &hud_state, &mut self.settings);
        let output = self.egui_ctx.end_pass();
        if let Some(egui_state) = self.egui_state.as_mut() {
            egui_state.handle_platform_output(&window, output.platform_output);
        }
        let egui_frame = EguiFrame {
            textures_delta: output.textures_delta,
            primitives: self.egui_ctx.tessellate(output.shapes, output.pixels_per_point),
            pixels_per_point: output.pixels_per_point,
        };

        // --- scene -------------------------------------------------------------
        // View bobbing: a subtle vertical sway while walking (vanilla-style).
        let moving = self.last_move.0 != 0 || self.last_move.1 != 0;
        if self.settings.view_bobbing && moving {
            let step = if self.last_move.2 { 0.42 } else { 0.30 };
            self.bob_phase = (self.bob_phase + step) % std::f32::consts::TAU;
        }
        let bob_y = if self.settings.view_bobbing && moving {
            (self.bob_phase.sin() * 0.045) as f64
        } else {
            0.0
        };
        let (cam_pos, yaw, pitch) = match &self.player {
            Some(p) => (
                [p.pos[0], p.pos[1] + p.eye_height as f64 + bob_y, p.pos[2]],
                self.yaw,
                self.pitch,
            ),
            // Spectator wait: slow orbit above spawn until player data arrives.
            None => (
                [8.0, 80.0, 8.0],
                (self.start.elapsed().as_secs_f32() * 10.0) % 360.0,
                20.0,
            ),
        };
        let fog_end = (self.settings.render_distance.max(2) * 16) as f32;
        // Brightness maps 0.5 → neutral, up → brighter, down → moody.
        let gamma = 0.6 + 0.8 * self.settings.brightness;
        let scene = SceneParams {
            cam_pos,
            yaw,
            pitch,
            fov_deg: self.settings.fov,
            daylight: (self.daylight * gamma).clamp(0.05, 1.0),
            fog_start: if self.settings.fog { fog_end * 0.75 } else { fog_end - 1.0 },
            fog_end,
            sky_color: [0.47, 0.65, 1.0],
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
                HudAction::Connect { address, username } => {
                    info!(address, username, "app: connect requested");
                    self.play_click();
                    // Use the account resolved at startup (launcher session /
                    // Microsoft). Only offline mode takes the username field.
                    let account = match &self.opts.bridge.account {
                        AccountConfig::Offline(_) => AccountConfig::Offline(username),
                        other => other.clone(),
                    };
                    match spawn_bridge(BridgeOptions { account, address }) {
                        Ok(pair) => {
                            self.mirror = WorldMirror::new();
                            self.disconnect_reason = None;
                            self.returning_to_menu = false;
                            // Preflight + login should finish well within this.
                            self.connect_deadline =
                                Some(Instant::now() + std::time::Duration::from_secs(45));
                            self.bridge = Some(pair);
                        }
                        Err(e) => {
                            warn!("app: connect failed: {e:#}");
                            self.hud.reset_to_title();
                            self.disconnect_reason = Some(format!("connect failed: {e:#}"));
                        }
                    }
                }
                HudAction::SettingsChanged => {
                    self.settings.clamp();
                    self.settings.save();
                    // vsync / fullscreen / GUI scale are applied next frame.
                    self.settings_dirty = true;
                }
                HudAction::Resume => {
                    self.play_click();
                    self.set_grab(true);
                }
                HudAction::Disconnect => {
                    // User left via the pause menu → return to the title screen
                    // (not the error box) once azalea confirms the disconnect.
                    self.play_click();
                    self.returning_to_menu = true;
                    self.send_cmd(Command::Disconnect);
                }
                HudAction::BackToMenu => {
                    self.play_click();
                    self.disconnect_reason = None;
                    self.connected = false;
                    self.bridge = None;
                    self.player = None;
                    self.entities.clear();
                    self.dir_synced = false;
                }
                HudAction::Quit => {
                    self.play_click();
                    event_loop.exit();
                }
            }
        }

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

    /// The listener (ear) position for sound attenuation — the player's eyes,
    /// or the origin before the first player snapshot.
    fn listener_pos(&self) -> [f64; 3] {
        match &self.player {
            Some(p) => [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]],
            None => [0.0, 0.0, 0.0],
        }
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

    fn drain_game_events(&mut self) {
        // Backstop: a connect attempt that never produced any event (azalea
        // can hang silently, e.g. after a failed session-server auth).
        if !self.connected
            && let Some(deadline) = self.connect_deadline
            && Instant::now() > deadline
        {
            warn!("app: connect timed out without any bridge event");
            self.connect_deadline = None;
            self.bridge = None; // dropping the handle disconnects
            self.disconnect_reason = Some(
                "Zeitüberschreitung beim Verbinden — der Server hat den Login nicht \
                 abgeschlossen. Bitte erneut versuchen; falls es bleibt, Launcher neu starten."
                    .to_string(),
            );
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
        for ev in events {
            self.mirror.apply(&ev);
            match ev {
                GameEvent::Connected { username } => {
                    info!(username, "app: connected");
                    self.connected = true;
                    self.connect_deadline = None;
                    self.disconnect_reason = None;
                    self.own_name = Some(username.clone());
                    self.hud.push_chat(format!("Connected as {username}"));
                    self.set_grab(true);
                }
                GameEvent::Disconnected { reason } => {
                    warn!(reason, "app: disconnected");
                    self.connected = false;
                    self.connect_deadline = None;
                    // A user-requested disconnect (pause menu) returns to the
                    // title screen; a kick/error shows the disconnect overlay.
                    if self.returning_to_menu {
                        self.returning_to_menu = false;
                        self.disconnect_reason = None;
                        self.hud.reset_to_title();
                    } else {
                        self.disconnect_reason = Some(reason);
                    }
                    self.bridge = None;
                    self.player = None;
                    self.entities.clear();
                    self.dir_synced = false;
                    self.set_grab(false);
                    return; // bridge is gone; stop draining
                }
                GameEvent::Chat { text } => self.hud.push_chat(text),
                GameEvent::PlayerState(p) => {
                    if !self.dir_synced {
                        self.yaw = p.yaw;
                        self.pitch = clamp_pitch(p.pitch);
                        self.dir_synced = true;
                    }
                    self.player = Some(*p);
                }
                GameEvent::Entities(list) => self.entities = list,
                GameEvent::Hotbar { slots, selected } => {
                    self.hotbar = slots.to_vec();
                    self.selected_slot = selected;
                }
                GameEvent::TimeOfDay { time_of_day } => {
                    self.daylight = daylight_factor(time_of_day);
                }
                GameEvent::Sound { name, category, pos, volume, pitch, seed } => {
                    if let Some(audio) = &self.audio {
                        let gain = self.settings.category_volume(category);
                        if gain > 0.0 {
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
                    }
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
            self.set_grab(false);
        }
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
                let tx = self.mesh_tx.clone();
                self.in_flight += 1;
                rayon::spawn(move || {
                    let mesh = mesh_section(&snap, &store, &table);
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

    fn entity_draws(&self) -> Vec<EntityDraw> {
        self.entities
            .iter()
            .filter(|e| {
                // The bridge already skips the local player; belt-and-braces by name.
                !(e.is_player && e.name.is_some() && e.name == self.own_name)
            })
            .map(|e| EntityDraw {
                pos: e.pos,
                yaw: e.yaw,
                kind: if e.is_player {
                    EntityDrawKind::Humanoid
                } else {
                    EntityDrawKind::Box { w: e.width, h: e.height }
                },
                color: if e.is_player { [0.3, 0.5, 0.9] } else { [0.9, 0.8, 0.2] },
            })
            .collect()
    }
}

// -- pure helpers ---------------------------------------------------------------

fn clamp_pitch(pitch: f32) -> f32 {
    pitch.clamp(-89.9, 89.9)
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

/// Digit1..Digit9 → hotbar slot 0..8.
fn hotbar_slot(code: KeyCode) -> Option<u8> {
    Some(match code {
        KeyCode::Digit1 => 0,
        KeyCode::Digit2 => 1,
        KeyCode::Digit3 => 2,
        KeyCode::Digit4 => 3,
        KeyCode::Digit5 => 4,
        KeyCode::Digit6 => 5,
        KeyCode::Digit7 => 6,
        KeyCode::Digit8 => 7,
        KeyCode::Digit9 => 8,
        _ => return None,
    })
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
    fn hotbar_keys_map_to_slots() {
        assert_eq!(hotbar_slot(KeyCode::Digit1), Some(0));
        assert_eq!(hotbar_slot(KeyCode::Digit9), Some(8));
        assert_eq!(hotbar_slot(KeyCode::KeyW), None);
        assert_eq!(hotbar_slot(KeyCode::Digit0), None);
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
}
