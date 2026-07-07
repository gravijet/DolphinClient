//! egui HUD + menus.
//!
//! In game: crosshair, hotbar (9 slots + selection), chat log (last 10 lines,
//! fading) + chat input, F3 debug, and an Esc pause menu.
//!
//! Out of game: a Minecraft-style title screen (Singleplayer is greyed out —
//! this is a multiplayer-only client), a Multiplayer connect screen, and an
//! Options screen (FOV / sensitivity / render distance). A "Connecting…" and a
//! disconnect overlay bridge the states.
//!
//! Pure egui — no wgpu here. The app calls `run()` each frame and forwards the
//! returned actions (chat, connect, option changes, resume/disconnect/quit) as
//! Commands / state changes.

use crate::assets::items::ItemIcons;
use crate::bridge::events::ItemSnapshot;
use crate::settings::GameSettings;
use egui::{
    Align2, Area, Color32, FontId, Id, Key, LayerId, Order, Rect, ScrollArea, Sense, Slider,
    Stroke, StrokeKind, TextEdit, TextureId, Window, pos2, vec2,
};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

#[derive(Default)]
pub struct HudState {
    pub fps: f32,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub food: u32,
    pub hotbar: Vec<Option<ItemSnapshot>>,
    pub selected_slot: u8,
    /// Item-icon atlas (egui texture id + lookup); None until it loads.
    pub icons: Option<(TextureId, Arc<ItemIcons>)>,
    pub sections_drawn: usize,
    pub sections_total: usize,
    pub mesh_queue: usize,
    pub connected: bool,
    /// A connect attempt is in flight (bridge spawned, not yet Connected).
    pub connecting: bool,
    pub disconnect_reason: Option<String>,
    /// Seconds since start — drives the title splash wobble.
    pub menu_time: f32,
}

pub enum HudAction {
    SendChat(String),
    Connect { address: String, username: String },
    /// An Options screen changed a setting: persist it and apply
    /// vsync/fullscreen/GUI-scale/render-distance.
    SettingsChanged,
    /// Pause menu → "Back to Game": re-grab the mouse.
    Resume,
    /// Pause menu → "Disconnect": leave the server, return to the title screen.
    Disconnect,
    /// Disconnect overlay → "Back to title": clear the error, show the menu.
    BackToMenu,
    Quit,
}

/// Which pre-game screen is showing (only when not connected).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Title,
    Multiplayer,
    Options,
}

/// In-game pause state (only when connected).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pause {
    None,
    Menu,
    Options,
}

/// Which category of the Options screen is showing (pre-game and in-game share
/// this). Root is the top-level list that links to the sub-screens.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionsTab {
    Root,
    Video,
    Controls,
    Chat,
    Sound,
}

/// Chat lines older than this are hidden (unless the chat input is open).
const CHAT_VISIBLE_SECS: f32 = 15.0;
const CHAT_MAX_LINES: usize = 10;

/// Minecraft GUI buttons are 200×20 px; at 2× GUI scale that is 400×40.
const BUTTON_W: f32 = 400.0;
const BUTTON_H: f32 = 40.0;
const BUTTON_GAP: f32 = 8.0;

pub struct Hud {
    pub show_debug: bool,
    pub chat_open: bool,
    /// (line, arrival time) — newest last, capped at CHAT_MAX_LINES.
    chat_lines: VecDeque<(String, Instant)>,
    chat_input: String,

    screen: Screen,
    pause: Pause,
    /// Which Options sub-screen is showing (shared pre-game / in-game).
    options_tab: OptionsTab,
    /// Connect-screen fields (persist across frames).
    address: String,
    username: String,
    /// Offline mode → the Multiplayer screen shows an editable username.
    offline: bool,
    /// Set while a menu text field wants the keyboard (app must not treat keys
    /// as movement). Recomputed every `run`.
    menu_wants_keyboard: bool,
    /// Address of the current connect attempt (shown in the Connecting overlay).
    connecting_to: String,
}

impl Hud {
    pub fn new(default_server: String, offline: bool, player_name: String) -> Self {
        let address = if default_server.trim().is_empty() {
            "localhost".into()
        } else {
            default_server
        };
        Self {
            show_debug: false,
            chat_open: false,
            chat_lines: VecDeque::new(),
            chat_input: String::new(),
            screen: Screen::Title,
            pause: Pause::None,
            options_tab: OptionsTab::Root,
            address,
            username: if player_name.trim().is_empty() { "Dolphin".into() } else { player_name },
            offline,
            menu_wants_keyboard: false,
            connecting_to: String::new(),
        }
    }

    /// True while a text field wants keyboard focus (app must not treat keys
    /// as movement).
    pub fn wants_keyboard(&self) -> bool {
        self.chat_open || self.menu_wants_keyboard
    }

    pub fn is_paused(&self) -> bool {
        !matches!(self.pause, Pause::None)
    }

    /// Esc while in game. Returns whether the mouse should be grabbed after
    /// (true = back to playing). None→Menu, Menu→None, Options→Menu.
    pub fn toggle_pause(&mut self) -> bool {
        self.pause = match self.pause {
            Pause::None => Pause::Menu,
            Pause::Menu => Pause::None,
            Pause::Options => Pause::Menu,
        };
        matches!(self.pause, Pause::None)
    }

    /// Return to the title screen (after a disconnect / user quit-to-menu).
    pub fn reset_to_title(&mut self) {
        self.screen = Screen::Title;
        self.pause = Pause::None;
        self.options_tab = OptionsTab::Root;
        self.chat_open = false;
        self.chat_lines.clear();
        self.connecting_to.clear();
    }

    /// Screenshot/debug helper: jump straight to a menu screen
    /// (0=Title, 1=Multiplayer, 2=Options) and optionally the in-game pause menu.
    pub fn debug_force(&mut self, screen: u8, pause_menu: bool) {
        self.screen = match screen {
            1 => Screen::Multiplayer,
            2 => Screen::Options,
            _ => Screen::Title,
        };
        self.pause = if pause_menu { Pause::Menu } else { Pause::None };
    }

    pub fn push_chat(&mut self, line: String) {
        self.chat_lines.push_back((line, Instant::now()));
        while self.chat_lines.len() > CHAT_MAX_LINES {
            self.chat_lines.pop_front();
        }
    }

    /// Build the frame's UI. Called inside `egui::Context::run`.
    pub fn run(
        &mut self,
        ctx: &egui::Context,
        state: &HudState,
        settings: &mut GameSettings,
    ) -> Vec<HudAction> {
        let mut actions = Vec::new();
        self.menu_wants_keyboard = false;

        if let Some(reason) = &state.disconnect_reason {
            self.disconnect_overlay(ctx, reason, &mut actions);
            return actions;
        }
        if !state.connected {
            self.menu_backdrop(ctx, Order::Background);
            if state.connecting {
                self.connecting_overlay(ctx);
            } else {
                match self.screen {
                    Screen::Title => self.title_screen(ctx, state, &mut actions),
                    Screen::Multiplayer => self.multiplayer_screen(ctx, &mut actions),
                    Screen::Options => self.options_screen(ctx, settings, &mut actions, false),
                }
            }
            return actions;
        }

        // In game.
        self.crosshair(ctx);
        self.hotbar(ctx, state);
        self.chat(ctx, &mut actions, settings);
        if self.show_debug {
            self.debug_overlay(ctx, state);
        }
        match self.pause {
            Pause::None => {}
            Pause::Menu => self.pause_menu(ctx, &mut actions),
            Pause::Options => self.options_screen(ctx, settings, &mut actions, true),
        }
        actions
    }

    // -- in-game HUD ---------------------------------------------------------

    fn crosshair(&self, ctx: &egui::Context) {
        let painter =
            ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("crosshair")));
        let c = ctx.content_rect().center();
        let stroke = Stroke::new(2.0, Color32::from_white_alpha(200));
        painter.line_segment([c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)], stroke);
        painter.line_segment([c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)], stroke);
    }

    fn hotbar(&self, ctx: &egui::Context, state: &HudState) {
        const SLOT: f32 = 40.0;
        Area::new(Id::new("hotbar"))
            .order(Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -6.0))
            .show(ctx, |ui| {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(9.0 * SLOT, SLOT), Sense::hover());
                let painter = ui.painter();
                for i in 0..9usize {
                    let r = Rect::from_min_size(
                        pos2(rect.min.x + i as f32 * SLOT, rect.min.y),
                        vec2(SLOT, SLOT),
                    )
                    .shrink(1.0);
                    painter.rect_filled(r, 2.0, Color32::from_black_alpha(160));
                    let stroke = if i as u8 == state.selected_slot {
                        Stroke::new(2.0, Color32::WHITE)
                    } else {
                        Stroke::new(1.0, Color32::from_gray(120))
                    };
                    painter.rect_stroke(r, 2.0, stroke, StrokeKind::Inside);
                    if let Some(Some(item)) = state.hotbar.get(i) {
                        let drawn = state.icons.as_ref().and_then(|(tex, icons)| {
                            let uv = icons.uv(&item.item)?;
                            let uv_rect =
                                Rect::from_min_max(pos2(uv[0], uv[1]), pos2(uv[2], uv[3]));
                            painter.image(*tex, r.shrink(3.0), uv_rect, Color32::WHITE);
                            Some(())
                        });
                        if drawn.is_none() {
                            // No baked icon (entity-rendered item, unknown name):
                            // fall back to a short text label.
                            let name: String = item.item.chars().take(8).collect();
                            painter.text(
                                r.center() - vec2(0.0, 5.0),
                                Align2::CENTER_CENTER,
                                name,
                                FontId::proportional(9.0),
                                Color32::WHITE,
                            );
                        }
                        if item.count > 1 {
                            painter.text(
                                r.right_bottom() - vec2(3.0, 1.0),
                                Align2::RIGHT_BOTTOM,
                                item.count.to_string(),
                                FontId::proportional(11.0),
                                Color32::WHITE,
                            );
                        }
                    }
                }
            });
    }

    fn chat(&mut self, ctx: &egui::Context, actions: &mut Vec<HudAction>, settings: &GameSettings) {
        let chat_open = self.chat_open;
        let scale = settings.chat_scale.clamp(0.5, 2.0);
        let opacity = settings.chat_opacity.clamp(0.0, 1.0);
        let font_size = 13.0 * scale;
        Area::new(Id::new("chat"))
            .order(Order::Foreground)
            .anchor(Align2::LEFT_BOTTOM, vec2(8.0, -64.0))
            .show(ctx, |ui| {
                ui.set_max_width(ctx.content_rect().width() * 0.5);
                for (line, when) in &self.chat_lines {
                    let age = when.elapsed().as_secs_f32();
                    let alpha = if chat_open {
                        1.0
                    } else if age >= CHAT_VISIBLE_SECS {
                        continue;
                    } else {
                        // Fade out over the final 2 seconds.
                        ((CHAT_VISIBLE_SECS - age) / 2.0).clamp(0.0, 1.0)
                    };
                    let bg = Color32::from_black_alpha((200.0 * opacity * alpha) as u8);
                    let fg = Color32::WHITE.gamma_multiply(alpha);
                    egui::Frame::NONE.fill(bg).inner_margin(2.0).show(ui, |ui| {
                        ui.label(egui::RichText::new(line).color(fg).size(font_size));
                    });
                }
                if chat_open {
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.chat_input)
                            .desired_width(400.0)
                            .font(FontId::proportional(font_size))
                            .hint_text("chat…"),
                    );
                    resp.request_focus();
                }
            });
        if self.chat_open {
            if ctx.input(|i| i.key_pressed(Key::Enter)) {
                let msg = std::mem::take(&mut self.chat_input);
                if !msg.trim().is_empty() {
                    actions.push(HudAction::SendChat(msg));
                }
                self.chat_open = false;
            } else if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.chat_input.clear();
                self.chat_open = false;
            }
        }
    }

    fn debug_overlay(&self, ctx: &egui::Context, s: &HudState) {
        Area::new(Id::new("debug"))
            .order(Order::Foreground)
            .anchor(Align2::LEFT_TOP, vec2(6.0, 6.0))
            .show(ctx, |ui| {
                let lines = [
                    format!("DolphinClient | {:5.1} fps", s.fps),
                    format!("xyz: {:.2} / {:.2} / {:.2}", s.pos[0], s.pos[1], s.pos[2]),
                    format!("yaw: {:.1}  pitch: {:.1}", s.yaw, s.pitch),
                    format!("health: {:.1}  food: {}", s.health, s.food),
                    format!("sections: {} drawn / {} total", s.sections_drawn, s.sections_total),
                    format!("mesh queue: {}", s.mesh_queue),
                ];
                for l in lines {
                    egui::Frame::NONE
                        .fill(Color32::from_black_alpha(120))
                        .inner_margin(2.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(l)
                                    .monospace()
                                    .size(12.0)
                                    .color(Color32::WHITE),
                            );
                        });
                }
            });
    }

    // -- pre-game menus ------------------------------------------------------

    /// Full-screen dark wash so menu text reads over the 3D world behind it.
    fn menu_backdrop(&self, ctx: &egui::Context, order: Order) {
        let painter = ctx.layer_painter(LayerId::new(order, Id::new(("menu-backdrop", order))));
        let r = ctx.content_rect();
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 225));
    }

    fn title_screen(&mut self, ctx: &egui::Context, state: &HudState, actions: &mut Vec<HudAction>) {
        self.title_logo(ctx, state.menu_time);

        let mut goto: Option<Screen> = None;
        let mut quit = false;
        Area::new(Id::new("title-buttons"))
            .order(Order::Middle)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 44.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = BUTTON_GAP;
                // Singleplayer is deliberately disabled — DolphinClient is a
                // multiplayer-only client (no world generation, no saves).
                mc_button(ui, BUTTON_W, "Singleplayer", false);
                if mc_button(ui, BUTTON_W, "Multiplayer", true) {
                    goto = Some(Screen::Multiplayer);
                }
                if mc_button(ui, BUTTON_W, "Options…", true) {
                    goto = Some(Screen::Options);
                }
                if mc_button(ui, BUTTON_W, "Quit Game", true) {
                    quit = true;
                }
            });
        if let Some(s) = goto {
            if s == Screen::Options {
                self.options_tab = OptionsTab::Root;
            }
            self.screen = s;
        }
        if quit {
            actions.push(HudAction::Quit);
        }

        // Corner labels, like vanilla.
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("title-corners")));
        let r = ctx.content_rect();
        painter.text(
            r.left_bottom() + vec2(4.0, -4.0),
            Align2::LEFT_BOTTOM,
            "DolphinClient 26.1 — native Rust client",
            FontId::proportional(13.0),
            Color32::from_gray(220),
        );
        painter.text(
            r.right_bottom() + vec2(-4.0, -4.0),
            Align2::RIGHT_BOTTOM,
            "Multiplayer only • Not affiliated with Mojang",
            FontId::proportional(13.0),
            Color32::from_gray(220),
        );
    }

    /// The big "DolphinClient" wordmark + a wobbling yellow splash.
    fn title_logo(&self, ctx: &egui::Context, time: f32) {
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("title-logo")));
        let r = ctx.content_rect();
        let cx = r.center().x;
        let ty = r.top() + r.height() * 0.20;
        let font = FontId::proportional(56.0);
        painter.text(
            pos2(cx + 4.0, ty + 4.0),
            Align2::CENTER_CENTER,
            "DolphinClient",
            font.clone(),
            Color32::from_black_alpha(160),
        );
        painter.text(
            pos2(cx, ty),
            Align2::CENTER_CENTER,
            "DolphinClient",
            font,
            Color32::from_rgb(0xE8, 0xF4, 0xFF),
        );
        // Splash: pulses in size like the vanilla title splash.
        let wob = 1.0 + 0.08 * (time * 3.5).sin();
        let splash = FontId::proportional(18.0 * wob);
        let sp = pos2(cx + 205.0, ty + 40.0);
        painter.text(sp + vec2(2.0, 2.0), Align2::CENTER_CENTER, "100% Rust!", splash.clone(), Color32::from_black_alpha(160));
        painter.text(sp, Align2::CENTER_CENTER, "100% Rust!", splash, Color32::from_rgb(0xFF, 0xFF, 0x40));
    }

    fn multiplayer_screen(&mut self, ctx: &egui::Context, actions: &mut Vec<HudAction>) {
        self.menu_wants_keyboard = true;
        self.menu_heading(ctx, "Play Multiplayer");

        let address = &mut self.address;
        let username = &mut self.username;
        let offline = self.offline;
        let mut join = false;
        let mut back = false;
        Area::new(Id::new("mp-screen"))
            .order(Order::Middle)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 10.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Server Address").color(Color32::from_gray(220)));
                    ui.add(
                        TextEdit::singleline(address)
                            .desired_width(BUTTON_W)
                            .hint_text("host or host:port"),
                    );
                    if offline {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Username (offline)").color(Color32::from_gray(220)));
                        ui.add(TextEdit::singleline(username).desired_width(BUTTON_W));
                    }
                });
                ui.add_space(6.0);
                let can_join = !address.trim().is_empty()
                    && (!offline || !username.trim().is_empty());
                if mc_button(ui, BUTTON_W, "Join Server", can_join)
                    || (can_join && ctx.input(|i| i.key_pressed(Key::Enter)))
                {
                    join = true;
                }
                if mc_button(ui, BUTTON_W, "Back", true) {
                    back = true;
                }
            });

        if join {
            let addr = self.address.trim().to_string();
            self.connecting_to = addr.clone();
            actions.push(HudAction::Connect {
                address: addr,
                username: self.username.trim().to_string(),
            });
        }
        if back || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.screen = Screen::Title;
        }
    }

    fn options_screen(
        &mut self,
        ctx: &egui::Context,
        settings: &mut GameSettings,
        actions: &mut Vec<HudAction>,
        in_game: bool,
    ) {
        if in_game {
            self.menu_backdrop(ctx, Order::Foreground);
        }
        let order = if in_game { Order::Tooltip } else { Order::Middle };
        let tab = self.options_tab;
        let title = match tab {
            OptionsTab::Root => "Options",
            OptionsTab::Video => "Video Settings",
            OptionsTab::Controls => "Controls",
            OptionsTab::Chat => "Chat Settings",
            OptionsTab::Sound => "Music & Sound",
        };
        self.menu_heading_ordered(ctx, title, order);

        let mut changed = false;
        let mut done = false;
        let mut goto: Option<OptionsTab> = None;
        Area::new(Id::new(("options-screen", in_game)))
            .order(order)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 30.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.style_mut().spacing.slider_width = BUTTON_W - 150.0;
                ui.set_width(BUTTON_W);
                ScrollArea::vertical()
                    .max_height(340.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| match tab {
                            OptionsTab::Root => {
                                let (c, g) = root_tab(ui, settings);
                                changed |= c;
                                goto = g;
                            }
                            OptionsTab::Video => changed |= video_tab(ui, settings),
                            OptionsTab::Controls => changed |= controls_tab(ui, settings),
                            OptionsTab::Chat => changed |= chat_tab(ui, settings),
                            OptionsTab::Sound => changed |= sound_tab(ui, settings),
                        });
                    });
                ui.add_space(8.0);
                if mc_button(ui, BUTTON_W, "Done", true) {
                    done = true;
                }
            });

        if let Some(t) = goto {
            self.options_tab = t;
        }
        if changed {
            actions.push(HudAction::SettingsChanged);
        }
        // Esc: only handled here for the pre-game menu (in-game Esc is the app's
        // pause toggle). Sub-tab → Root; Root → leave Options.
        let esc = !in_game && ctx.input(|i| i.key_pressed(Key::Escape));
        if done || esc {
            match self.options_tab {
                OptionsTab::Root => {
                    if in_game {
                        self.pause = Pause::Menu;
                    } else {
                        self.screen = Screen::Title;
                    }
                }
                _ => self.options_tab = OptionsTab::Root,
            }
        }
    }

    fn connecting_overlay(&self, ctx: &egui::Context) {
        self.menu_heading(ctx, "Connecting to server");
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("connecting")));
        let c = ctx.content_rect().center();
        painter.text(
            c,
            Align2::CENTER_CENTER,
            format!("Connecting to {} …", self.connecting_to),
            FontId::proportional(20.0),
            Color32::from_gray(230),
        );
    }

    // -- in-game pause menu --------------------------------------------------

    fn pause_menu(&mut self, ctx: &egui::Context, actions: &mut Vec<HudAction>) {
        self.menu_backdrop(ctx, Order::Foreground);
        self.menu_heading_ordered(ctx, "Game Paused", Order::Tooltip);

        let mut resume = false;
        let mut options = false;
        let mut disconnect = false;
        Area::new(Id::new("pause-menu"))
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 20.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = BUTTON_GAP;
                if mc_button(ui, BUTTON_W, "Back to Game", true) {
                    resume = true;
                }
                if mc_button(ui, BUTTON_W, "Options…", true) {
                    options = true;
                }
                if mc_button(ui, BUTTON_W, "Disconnect", true) {
                    disconnect = true;
                }
            });
        if resume {
            self.pause = Pause::None;
            actions.push(HudAction::Resume);
        }
        if options {
            self.options_tab = OptionsTab::Root;
            self.pause = Pause::Options;
        }
        if disconnect {
            actions.push(HudAction::Disconnect);
        }
    }

    // -- overlays ------------------------------------------------------------

    fn menu_heading(&self, ctx: &egui::Context, text: &str) {
        self.menu_heading_ordered(ctx, text, Order::Middle);
    }

    fn menu_heading_ordered(&self, ctx: &egui::Context, text: &str, order: Order) {
        let painter = ctx.layer_painter(LayerId::new(order, Id::new(("menu-heading", order))));
        let r = ctx.content_rect();
        let p = pos2(r.center().x, r.top() + r.height() * 0.14);
        painter.text(p + vec2(2.0, 2.0), Align2::CENTER_CENTER, text, FontId::proportional(30.0), Color32::from_black_alpha(160));
        painter.text(p, Align2::CENTER_CENTER, text, FontId::proportional(30.0), Color32::WHITE);
    }

    fn disconnect_overlay(
        &mut self,
        ctx: &egui::Context,
        reason: &str,
        actions: &mut Vec<HudAction>,
    ) {
        self.menu_backdrop(ctx, Order::Background);
        Window::new("Disconnected")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_min_width(BUTTON_W);
                ui.label(reason);
                ui.add_space(12.0);
                let mut back = false;
                let mut quit = false;
                ui.vertical_centered(|ui| {
                    if mc_button(ui, BUTTON_W, "Back to Title", true) {
                        back = true;
                    }
                    ui.add_space(BUTTON_GAP);
                    if mc_button(ui, BUTTON_W, "Quit Game", true) {
                        quit = true;
                    }
                });
                if back {
                    self.reset_to_title();
                    actions.push(HudAction::BackToMenu);
                }
                if quit {
                    actions.push(HudAction::Quit);
                }
            });
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "ON" } else { "OFF" }
}

fn gui_scale_label(n: u32) -> String {
    if n == 0 { "Auto".to_string() } else { n.to_string() }
}

/// Fixed vanilla key binds, shown read-only on the Controls screen.
const KEY_BINDS: &[(&str, &str)] = &[
    ("Move", "W A S D"),
    ("Jump", "Space"),
    ("Sneak", "Left Shift"),
    ("Sprint", "Left Ctrl"),
    ("Hotbar", "1 – 9"),
    ("Attack / Mine", "Left Mouse"),
    ("Use / Interact", "Right Mouse"),
    ("Chat", "T / Enter"),
    ("Pause", "Esc"),
    ("Debug Overlay", "F3"),
];

/// Top-level Options: quick FOV/Brightness + links to the sub-screens.
fn root_tab(ui: &mut egui::Ui, s: &mut GameSettings) -> (bool, Option<OptionsTab>) {
    let mut changed = false;
    let mut goto = None;
    changed |= ui
        .add(Slider::new(&mut s.fov, 30.0..=110.0).text("FOV").fixed_decimals(0))
        .changed();
    changed |= ui
        .add(
            Slider::new(&mut s.brightness, 0.0..=1.0)
                .text("Brightness")
                .custom_formatter(|n, _| format!("{:.0}%", n * 100.0)),
        )
        .changed();
    ui.add_space(8.0);
    if mc_button(ui, BUTTON_W, "Video Settings…", true) {
        goto = Some(OptionsTab::Video);
    }
    if mc_button(ui, BUTTON_W, "Controls…", true) {
        goto = Some(OptionsTab::Controls);
    }
    if mc_button(ui, BUTTON_W, "Music & Sound…", true) {
        goto = Some(OptionsTab::Sound);
    }
    if mc_button(ui, BUTTON_W, "Chat Settings…", true) {
        goto = Some(OptionsTab::Chat);
    }
    (changed, goto)
}

/// Video Settings: everything that affects the renderer & window.
fn video_tab(ui: &mut egui::Ui, s: &mut GameSettings) -> bool {
    let mut changed = false;
    changed |= ui
        .add(Slider::new(&mut s.fov, 30.0..=110.0).text("FOV").fixed_decimals(0))
        .changed();
    changed |= ui
        .add(
            Slider::new(&mut s.render_distance, 2..=32)
                .text("Render Distance")
                .suffix(" chunks"),
        )
        .changed();
    changed |= ui
        .add(
            Slider::new(&mut s.max_fps, 0..=260)
                .text("Max Framerate")
                .step_by(5.0)
                .custom_formatter(|n, _| {
                    if n < 1.0 {
                        "Unlimited".to_string()
                    } else {
                        format!("{n:.0} fps")
                    }
                }),
        )
        .changed();
    changed |= ui
        .add(
            Slider::new(&mut s.brightness, 0.0..=1.0)
                .text("Brightness")
                .custom_formatter(|n, _| format!("{:.0}%", n * 100.0)),
        )
        .changed();
    ui.add_space(6.0);
    if mc_button(ui, BUTTON_W, &format!("VSync: {}", on_off(s.vsync)), true) {
        s.vsync = !s.vsync;
        changed = true;
    }
    if mc_button(ui, BUTTON_W, &format!("Fullscreen: {}", on_off(s.fullscreen)), true) {
        s.fullscreen = !s.fullscreen;
        changed = true;
    }
    if mc_button(ui, BUTTON_W, &format!("Graphics: {}", s.graphics.label()), true) {
        s.graphics = s.graphics.next();
        changed = true;
    }
    if mc_button(ui, BUTTON_W, &format!("GUI Scale: {}", gui_scale_label(s.gui_scale)), true) {
        s.gui_scale = (s.gui_scale + 1) % 5;
        changed = true;
    }
    if mc_button(ui, BUTTON_W, &format!("Fog: {}", on_off(s.fog)), true) {
        s.fog = !s.fog;
        changed = true;
    }
    if mc_button(ui, BUTTON_W, &format!("View Bobbing: {}", on_off(s.view_bobbing)), true) {
        s.view_bobbing = !s.view_bobbing;
        changed = true;
    }
    changed
}

/// Controls: mouse settings + read-only key-bind reference.
fn controls_tab(ui: &mut egui::Ui, s: &mut GameSettings) -> bool {
    let mut changed = false;
    changed |= ui
        .add(
            Slider::new(&mut s.sensitivity_pct, 0.0..=200.0)
                .text("Sensitivity")
                .custom_formatter(|n, _| {
                    if n <= 0.5 {
                        "*yawn*".to_string()
                    } else if (n - 100.0).abs() < 0.5 {
                        "100% (default)".to_string()
                    } else {
                        format!("{n:.0}%")
                    }
                }),
        )
        .changed();
    if mc_button(ui, BUTTON_W, &format!("Invert Mouse: {}", on_off(s.invert_mouse)), true) {
        s.invert_mouse = !s.invert_mouse;
        changed = true;
    }
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new("Key Binds")
            .color(Color32::from_gray(210))
            .strong(),
    );
    ui.add_space(2.0);
    for (action, key) in KEY_BINDS {
        ui.horizontal(|ui| {
            ui.add_sized([BUTTON_W * 0.5, 18.0], egui::Label::new(
                egui::RichText::new(*action).color(Color32::from_gray(200)),
            ));
            ui.label(egui::RichText::new(*key).monospace().color(Color32::WHITE));
        });
    }
    changed
}

/// Chat Settings: scale + background opacity.
fn chat_tab(ui: &mut egui::Ui, s: &mut GameSettings) -> bool {
    let mut changed = false;
    changed |= ui
        .add(Slider::new(&mut s.chat_scale, 0.5..=2.0).text("Chat Scale").fixed_decimals(2))
        .changed();
    changed |= ui
        .add(
            Slider::new(&mut s.chat_opacity, 0.0..=1.0)
                .text("Chat Opacity")
                .custom_formatter(|n, _| format!("{:.0}%", n * 100.0)),
        )
        .changed();
    changed
}

/// Volume slider label: `OFF` at zero, otherwise a percentage.
fn vol_fmt(n: f64, _: std::ops::RangeInclusive<usize>) -> String {
    if n <= 0.0 {
        "OFF".to_string()
    } else {
        format!("{:.0}%", n * 100.0)
    }
}

/// Music & Sound: master + per-category volumes, exactly like vanilla's screen.
fn sound_tab(ui: &mut egui::Ui, s: &mut GameSettings) -> bool {
    let mut changed = false;
    // Master first, then each category.
    let rows: [(&str, &mut f32); 10] = [
        ("Master Volume", &mut s.master_volume),
        ("Music", &mut s.music_volume),
        ("Jukebox/Note Blocks", &mut s.records_volume),
        ("Weather", &mut s.weather_volume),
        ("Blocks", &mut s.blocks_volume),
        ("Hostile Creatures", &mut s.hostile_volume),
        ("Friendly Creatures", &mut s.neutral_volume),
        ("Players", &mut s.players_volume),
        ("Ambient/Environment", &mut s.ambient_volume),
        ("Voice/Speech", &mut s.voice_volume),
    ];
    for (label, value) in rows {
        changed |= ui
            .add(
                Slider::new(value, 0.0..=1.0)
                    .text(label)
                    .custom_formatter(vol_fmt),
            )
            .changed();
    }
    changed
}

/// A Minecraft-style button: gray, beveled, hover-highlighted. Disabled buttons
/// are darker with grey text and swallow clicks. Returns true on a click.
fn mc_button(ui: &mut egui::Ui, width: f32, label: &str, enabled: bool) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, BUTTON_H), sense);
    let hovered = enabled && resp.hovered();
    // Minecraft's stone-grey button, top-lit gradient. Values are pre-brightened
    // because egui draws its vertex colours onto an sRGB target (mid-greys land
    // darker than nominal); these are tuned so the *displayed* button reads as
    // vanilla stone-grey over the dark backdrop.
    let (top, bottom, text_col) = if !enabled {
        (Color32::from_rgb(0xA0, 0xA0, 0xA0), Color32::from_rgb(0x86, 0x86, 0x86), Color32::from_rgb(0x64, 0x64, 0x64))
    } else if hovered {
        (Color32::from_rgb(0xDA, 0xDA, 0xAC), Color32::from_rgb(0xBE, 0xBE, 0x92), Color32::from_rgb(0xFF, 0xFF, 0xA0))
    } else {
        (Color32::from_rgb(0xCB, 0xCB, 0xCB), Color32::from_rgb(0xB4, 0xB4, 0xB4), Color32::WHITE)
    };
    let painter = ui.painter();
    let mid = rect.center().y;
    painter.rect_filled(Rect::from_min_max(rect.min, pos2(rect.max.x, mid)), 0.0, top);
    painter.rect_filled(Rect::from_min_max(pos2(rect.min.x, mid), rect.max), 0.0, bottom);
    // Pixel-y bevel: light top/left, dark bottom/right, black outer border.
    painter.line_segment([rect.left_top() + vec2(1.0, 1.0), rect.right_top() + vec2(-1.0, 1.0)], Stroke::new(1.0, Color32::from_white_alpha(45)));
    painter.line_segment([rect.left_bottom() + vec2(1.0, -1.0), rect.right_bottom() + vec2(-1.0, -1.0)], Stroke::new(1.0, Color32::from_black_alpha(90)));
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Inside);
    let c = rect.center();
    let font = FontId::proportional(18.0);
    painter.text(c + vec2(1.0, 1.0), Align2::CENTER_CENTER, label, font.clone(), Color32::from_black_alpha(160));
    painter.text(c, Align2::CENTER_CENTER, label, font, text_col);
    enabled && resp.clicked()
}

impl Default for Hud {
    fn default() -> Self {
        Self::new(String::new(), true, "Dolphin".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_caps_at_ten_lines() {
        let mut hud = Hud::default();
        for i in 0..25 {
            hud.push_chat(format!("line {i}"));
        }
        assert_eq!(hud.chat_lines.len(), CHAT_MAX_LINES);
        assert_eq!(hud.chat_lines.front().unwrap().0, "line 15");
        assert_eq!(hud.chat_lines.back().unwrap().0, "line 24");
    }

    #[test]
    fn wants_keyboard_follows_chat_and_menu() {
        let mut hud = Hud::default();
        assert!(!hud.wants_keyboard());
        hud.chat_open = true;
        assert!(hud.wants_keyboard());
        hud.chat_open = false;
        hud.menu_wants_keyboard = true;
        assert!(hud.wants_keyboard());
    }

    #[test]
    fn pause_toggle_cycles_and_reports_grab() {
        let mut hud = Hud::default();
        assert!(!hud.is_paused());
        // None -> Menu: not playing, don't grab.
        assert!(!hud.toggle_pause());
        assert!(hud.is_paused());
        // Menu -> None: back to playing, grab.
        assert!(hud.toggle_pause());
        assert!(!hud.is_paused());
        // Options -> Menu: still paused, don't grab.
        hud.pause = Pause::Options;
        assert!(!hud.toggle_pause());
        assert!(hud.is_paused());
    }

    #[test]
    fn reset_to_title_clears_state() {
        let mut hud = Hud::default();
        hud.pause = Pause::Menu;
        hud.screen = Screen::Options;
        hud.push_chat("hi".into());
        hud.reset_to_title();
        assert!(!hud.is_paused());
        assert!(matches!(hud.screen, Screen::Title));
        assert!(hud.chat_lines.is_empty());
    }
}
