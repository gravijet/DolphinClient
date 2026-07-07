//! egui HUD + menus, drawn with the real Minecraft assets (see `mcui`).
//!
//! In game: sprite crosshair, the vanilla hotbar with item icons, hearts /
//! food / XP bar, chat log (last 10 lines, fading) + chat input, F3 debug,
//! and an Esc pause menu over the translucent in-world tile.
//!
//! Out of game: a Minecraft-style title screen (Singleplayer greyed out —
//! this is a multiplayer-only client), a Multiplayer connect screen and an
//! Options screen, all on the tiled vanilla menu background with the real
//! bitmap font and nine-sliced button/slider textures.
//!
//! Pure egui — no wgpu here. The app calls `run()` each frame and forwards the
//! returned actions (chat, connect, option changes, resume/disconnect/quit) as
//! Commands / state changes.

use crate::app::mcui::{self, BTN_GAP, BTN_W, COL_W, LINE_H, ROW_W, McUi};
use crate::assets::items::ItemIcons;
use crate::bridge::events::ItemSnapshot;
use crate::settings::GameSettings;
use egui::{
    Align2, Area, Color32, FontId, Id, Key, LayerId, Order, Rect, ScrollArea, Sense, TextEdit,
    TextureId, pos2, vec2,
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
    pub xp_level: u32,
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
        mc: &McUi,
        state: &HudState,
        settings: &mut GameSettings,
    ) -> Vec<HudAction> {
        let mut actions = Vec::new();
        self.menu_wants_keyboard = false;
        let s = mc.gui_scale(ctx, settings);

        if let Some(reason) = &state.disconnect_reason {
            let reason = reason.clone();
            self.disconnect_screen(ctx, mc, s, &reason, &mut actions);
            return actions;
        }
        if !state.connected {
            if state.connecting {
                self.connecting_screen(ctx, mc, s);
            } else {
                match self.screen {
                    Screen::Title => self.title_screen(ctx, mc, s, state, &mut actions),
                    Screen::Multiplayer => self.multiplayer_screen(ctx, mc, s, &mut actions),
                    Screen::Options => self.options_screen(ctx, mc, s, settings, &mut actions, false),
                }
            }
            return actions;
        }

        // In game.
        self.crosshair(ctx, mc, s);
        self.hotbar(ctx, mc, s, state);
        self.status_bars(ctx, mc, s, state);
        self.chat(ctx, mc, s, &mut actions, settings);
        if self.show_debug {
            self.debug_overlay(ctx, mc, state);
        }
        match self.pause {
            Pause::None => {}
            Pause::Menu => self.pause_menu(ctx, mc, s, &mut actions),
            Pause::Options => self.options_screen(ctx, mc, s, settings, &mut actions, true),
        }
        actions
    }

    // -- in-game HUD ---------------------------------------------------------

    fn crosshair(&self, ctx: &egui::Context, mc: &McUi, s: f32) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("crosshair")));
        let c = ctx.content_rect().center();
        let sz = mc.tex.crosshair.size_vec2() * s;
        let rect = Rect::from_center_size(c, sz);
        painter.image(
            mc.tex.crosshair.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::from_white_alpha(220),
        );
    }

    /// The vanilla hotbar: 182×22 sprite, 24×23 selection frame, item icons
    /// and stack counts. Bottom-centered like the real HUD.
    fn hotbar(&self, ctx: &egui::Context, mc: &McUi, s: f32, state: &HudState) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("hotbar")));
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let r = ctx.content_rect();
        let bar_size = mc.tex.hotbar.size_vec2() * s; // 182×22 GUI px
        let bar = Rect::from_min_size(
            pos2(r.center().x - bar_size.x / 2.0, r.bottom() - bar_size.y),
            bar_size,
        );
        painter.image(mc.tex.hotbar.id(), bar, full, Color32::WHITE);

        // Selection frame around the active slot (slot pitch 20 GUI px).
        let sel_size = mc.tex.hotbar_selection.size_vec2() * s;
        let sel_x = bar.left() + (state.selected_slot as f32 * 20.0 - 1.0) * s;
        let sel = Rect::from_min_size(pos2(sel_x, bar.top() - 1.0 * s), sel_size);
        painter.image(mc.tex.hotbar_selection.id(), sel, full, Color32::WHITE);

        // Items: 16×16 at x = 3 + i*20, y = 3 (GUI px inside the bar).
        for i in 0..9usize {
            let Some(Some(item)) = state.hotbar.get(i) else { continue };
            let cell = Rect::from_min_size(
                pos2(bar.left() + (3.0 + i as f32 * 20.0) * s, bar.top() + 3.0 * s),
                vec2(16.0 * s, 16.0 * s),
            );
            let drawn = state.icons.as_ref().and_then(|(tex, icons)| {
                let uv = icons.uv(&item.item)?;
                let uv_rect = Rect::from_min_max(pos2(uv[0], uv[1]), pos2(uv[2], uv[3]));
                painter.image(*tex, cell, uv_rect, Color32::WHITE);
                Some(())
            });
            if drawn.is_none() {
                // No baked icon (entity-rendered item, unknown name): initials.
                let name: String = item.item.chars().take(3).collect();
                mc.font.draw_anchored(
                    &painter,
                    cell.center(),
                    Align2::CENTER_CENTER,
                    &name,
                    s * 0.75,
                    Color32::WHITE,
                    true,
                );
            }
            if item.count > 1 {
                mc.font.draw_anchored(
                    &painter,
                    cell.right_bottom() + vec2(1.0 * s, 1.0 * s),
                    Align2::RIGHT_BOTTOM,
                    &item.count.to_string(),
                    s,
                    Color32::WHITE,
                    true,
                );
            }
        }
    }

    /// Hearts, hunger and the XP bar in their vanilla positions above the
    /// hotbar.
    fn status_bars(&self, ctx: &egui::Context, mc: &McUi, s: f32, state: &HudState) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("status-bars")));
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let r = ctx.content_rect();
        let cx = r.center().x;
        let hotbar_top = r.bottom() - 22.0 * s;

        // XP bar: 182×5, sitting 7 GUI px above the hotbar top edge.
        let bar_w = 182.0 * s;
        let xp_rect = Rect::from_min_size(
            pos2(cx - bar_w / 2.0, hotbar_top - 7.0 * s),
            vec2(bar_w, 5.0 * s),
        );
        painter.image(mc.tex.xp_bg.id(), xp_rect, full, Color32::WHITE);
        if state.xp_level > 0 {
            mc.font.draw_anchored(
                &painter,
                pos2(cx, xp_rect.top() - 8.0 * s),
                Align2::CENTER_CENTER,
                &state.xp_level.to_string(),
                s,
                Color32::from_rgb(0x80, 0xFF, 0x20),
                true,
            );
        }

        // Hearts (left) and hunger (right), 9×9 sprites, row 10 px above XP.
        let row_y = hotbar_top - 17.0 * s;
        let icon = vec2(9.0 * s, 9.0 * s);
        let health = state.health.clamp(0.0, 20.0);
        for i in 0..10 {
            let x = cx - 91.0 * s + i as f32 * 8.0 * s;
            let rect = Rect::from_min_size(pos2(x, row_y), icon);
            painter.image(mc.tex.heart_container.id(), rect, full, Color32::WHITE);
            let v = health - (i * 2) as f32;
            if v >= 2.0 {
                painter.image(mc.tex.heart_full.id(), rect, full, Color32::WHITE);
            } else if v >= 1.0 {
                painter.image(mc.tex.heart_half.id(), rect, full, Color32::WHITE);
            }
        }
        let food = state.food.min(20);
        for i in 0..10 {
            let x = cx + 91.0 * s - (i as f32 + 1.0) * 8.0 * s - 1.0 * s;
            let rect = Rect::from_min_size(pos2(x, row_y), icon);
            painter.image(mc.tex.food_empty.id(), rect, full, Color32::WHITE);
            let v = food as i32 - (i * 2) as i32;
            if v >= 2 {
                painter.image(mc.tex.food_full.id(), rect, full, Color32::WHITE);
            } else if v >= 1 {
                painter.image(mc.tex.food_half.id(), rect, full, Color32::WHITE);
            }
        }
    }

    fn chat(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        actions: &mut Vec<HudAction>,
        settings: &GameSettings,
    ) {
        let chat_open = self.chat_open;
        let cs = s * settings.chat_scale.clamp(0.5, 2.0);
        let opacity = settings.chat_opacity.clamp(0.0, 1.0);
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("chat-log")));
        let r = ctx.content_rect();
        let max_w = (r.width() * 0.5).max(160.0 * cs);
        let line_h = LINE_H * cs;

        // Collect visible (possibly wrapped) lines, newest last.
        let mut rows: Vec<(String, f32)> = Vec::new();
        for (line, when) in &self.chat_lines {
            let age = when.elapsed().as_secs_f32();
            let alpha = if chat_open {
                1.0
            } else if age >= CHAT_VISIBLE_SECS {
                continue;
            } else {
                ((CHAT_VISIBLE_SECS - age) / 2.0).clamp(0.0, 1.0)
            };
            for piece in wrap_text(mc, line, cs, max_w - 4.0 * cs) {
                rows.push((piece, alpha));
            }
        }
        let base_y = r.bottom() - 48.0 * s;
        for (i, (line, alpha)) in rows.iter().rev().enumerate() {
            let y = base_y - (i as f32 + 1.0) * line_h;
            if y < r.top() {
                break;
            }
            let w = mc.font.width(line, cs) + 4.0 * cs;
            let bg = Color32::from_black_alpha((128.0 * opacity * alpha) as u8);
            painter.rect_filled(
                Rect::from_min_size(pos2(r.left(), y), vec2(w, line_h)),
                0.0,
                bg,
            );
            mc.font.draw(
                &painter,
                pos2(r.left() + 2.0 * cs, y + 0.5 * cs),
                line,
                cs,
                Color32::WHITE.gamma_multiply(*alpha),
                true,
            );
        }

        // Input row: full-width black bar at the very bottom, like vanilla.
        if chat_open {
            let h = 12.0 * s;
            let bar = Rect::from_min_size(
                pos2(r.left() + 2.0 * s, r.bottom() - h - 2.0 * s),
                vec2(r.width() - 4.0 * s, h),
            );
            painter.rect_filled(bar, 0.0, Color32::from_black_alpha(128));
            let mut resp = None;
            Area::new(Id::new("chat-input"))
                .order(Order::Foreground)
                .fixed_pos(bar.min + vec2(2.0 * s, 0.0))
                .show(ctx, |ui| {
                    let r = ui.add(
                        TextEdit::singleline(&mut self.chat_input)
                            .desired_width(bar.width() - 8.0 * s)
                            .frame(egui::Frame::NONE)
                            .font(FontId::monospace(7.5 * s))
                            .text_color(Color32::WHITE),
                    );
                    r.request_focus();
                    resp = Some(r);
                });
            let _ = resp;
        }
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

    fn debug_overlay(&self, ctx: &egui::Context, mc: &McUi, s: &HudState) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("debug")));
        let r = ctx.content_rect();
        let fs = 1.5; // F3 text is small in vanilla too
        let lines = [
            format!("DolphinClient {} ({:.0} fps)", env!("CARGO_PKG_VERSION"), s.fps),
            format!("XYZ: {:.3} / {:.5} / {:.3}", s.pos[0], s.pos[1], s.pos[2]),
            format!("Facing: yaw {:.1} / pitch {:.1}", s.yaw, s.pitch),
            format!("Health: {:.1}  Food: {}", s.health, s.food),
            format!("C: {}/{} sections", s.sections_drawn, s.sections_total),
            format!("Mesh queue: {}", s.mesh_queue),
        ];
        let mut y = r.top() + 2.0;
        for l in lines {
            let w = mc.font.width(&l, fs) + 2.0;
            painter.rect_filled(
                Rect::from_min_size(pos2(r.left() + 1.0, y), vec2(w, LINE_H * fs)),
                0.0,
                Color32::from_rgba_unmultiplied(80, 80, 80, 90),
            );
            mc.font.draw(
                &painter,
                pos2(r.left() + 2.0, y + 0.5),
                &l,
                fs,
                Color32::from_rgb(0xE0, 0xE0, 0xE0),
                false,
            );
            y += LINE_H * fs;
        }
    }

    // -- pre-game menus ------------------------------------------------------

    /// Tiled vanilla background: the classic dark dirt out of game, the
    /// translucent `inworld_menu_background` over the running world.
    fn menu_background(&self, ctx: &egui::Context, mc: &McUi, s: f32, order: Order, in_world: bool) {
        let painter = ctx.layer_painter(LayerId::new(order, Id::new(("menu-bg", order))));
        if in_world {
            mcui::tile_background(
                &painter,
                &mc.tex.inworld_bg,
                ctx.content_rect(),
                s,
                Color32::WHITE,
            );
        } else {
            // The iconic dirt screen: dirt tiles multiplied to 25 % grey.
            mcui::tile_background(
                &painter,
                &mc.tex.dirt,
                ctx.content_rect(),
                s,
                Color32::from_gray(64),
            );
        }
    }

    fn title_screen(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        state: &HudState,
        actions: &mut Vec<HudAction>,
    ) {
        // No dirt here: the renderer's sky acts as the title "panorama".
        self.title_logo(ctx, mc, s, state.menu_time);

        let mut goto: Option<Screen> = None;
        let mut quit = false;
        Area::new(Id::new("title-buttons"))
            .order(Order::Middle)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 10.0 * s))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = BTN_GAP * s;
                // Singleplayer is deliberately disabled — DolphinClient is a
                // multiplayer-only client (no world generation, no saves).
                mcui::button(ui, mc, BTN_W, s, "Singleplayer", false);
                if mcui::button(ui, mc, BTN_W, s, "Multiplayer", true) {
                    goto = Some(Screen::Multiplayer);
                }
                ui.add_space(4.0 * s);
                if mcui::button(ui, mc, BTN_W, s, "Options...", true) {
                    goto = Some(Screen::Options);
                }
                if mcui::button(ui, mc, BTN_W, s, "Quit Game", true) {
                    quit = true;
                }
            });
        if let Some(sc) = goto {
            if sc == Screen::Options {
                self.options_tab = OptionsTab::Root;
            }
            self.screen = sc;
        }
        if quit {
            actions.push(HudAction::Quit);
        }

        // Corner labels, like vanilla.
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("title-corners")));
        let r = ctx.content_rect();
        mc.font.draw(
            &painter,
            pos2(r.left() + 2.0, r.bottom() - LINE_H * s),
            &format!("DolphinClient {} (Minecraft 26.1)", env!("CARGO_PKG_VERSION")),
            s,
            Color32::WHITE,
            true,
        );
        let right_text = "Not affiliated with Mojang";
        mc.font.draw(
            &painter,
            pos2(
                r.right() - mc.font.width(right_text, s) - 2.0,
                r.bottom() - LINE_H * s,
            ),
            right_text,
            s,
            Color32::WHITE,
            true,
        );
    }

    /// The DolphinClient "logo": pixel dolphin + chunky wordmark + splash.
    fn title_logo(&self, ctx: &egui::Context, mc: &McUi, s: f32, time: f32) {
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("title-logo")));
        let r = ctx.content_rect();
        let cx = r.center().x;
        let ty = r.top() + 30.0 * s;

        let word = "DolphinClient";
        let word_s = s * 2.0; // chunky 16-GUI-px letters
        let word_w = mc.font.width(word, word_s);
        let icon = 32.0 * s;
        let total = icon + 6.0 * s + word_w;
        let left = cx - total / 2.0;

        let icon_rect = Rect::from_min_size(
            pos2(left, ty - icon * 0.30),
            vec2(icon, icon),
        );
        painter.image(
            mc.tex.logo.id(),
            icon_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        mc.font.draw(
            &painter,
            pos2(left + icon + 6.0 * s, ty),
            word,
            word_s,
            Color32::WHITE,
            true,
        );

        // Splash: pulses like the vanilla title splash.
        let wob = 1.0 + 0.06 * (time * 6.0).sin();
        let splash = "100% Rust!";
        let sp = pos2(left + total - 6.0 * s, ty + 18.0 * s);
        mc.font.draw_anchored(
            &painter,
            sp,
            Align2::CENTER_CENTER,
            splash,
            s * wob,
            Color32::from_rgb(0xFF, 0xFF, 0x00),
            true,
        );
    }

    fn multiplayer_screen(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        actions: &mut Vec<HudAction>,
    ) {
        self.menu_background(ctx, mc, s, Order::Background, false);
        self.menu_wants_keyboard = true;
        self.menu_heading(ctx, mc, s, "Play Multiplayer", Order::Middle);

        let offline = self.offline;
        let mut join = false;
        let mut back = false;
        Area::new(Id::new("mp-screen"))
            .order(Order::Middle)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(4.0 * s, 4.0 * s);
                ui.vertical_centered(|ui| {
                    mcui::label(ui, mc, s, "Server Address", Color32::from_rgb(0xA0, 0xA0, 0xA0));
                    mcui::text_field(ui, mc, BTN_W, s, &mut self.address, "host or host:port");
                    if offline {
                        ui.add_space(2.0 * s);
                        mcui::label(ui, mc, s, "Username (offline)", Color32::from_rgb(0xA0, 0xA0, 0xA0));
                        mcui::text_field(ui, mc, BTN_W, s, &mut self.username, "");
                    }
                    ui.add_space(6.0 * s);
                    let can_join = !self.address.trim().is_empty()
                        && (!offline || !self.username.trim().is_empty());
                    if mcui::button(ui, mc, BTN_W, s, "Join Server", can_join)
                        || (can_join && ctx.input(|i| i.key_pressed(Key::Enter)))
                    {
                        join = true;
                    }
                    if mcui::button(ui, mc, BTN_W, s, "Back", true) {
                        back = true;
                    }
                });
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

    #[allow(clippy::too_many_arguments)]
    fn options_screen(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        settings: &mut GameSettings,
        actions: &mut Vec<HudAction>,
        in_game: bool,
    ) {
        let order = if in_game { Order::Tooltip } else { Order::Middle };
        if in_game {
            self.menu_background(ctx, mc, s, Order::Foreground, true);
        } else {
            self.menu_background(ctx, mc, s, Order::Background, false);
        }
        let tab = self.options_tab;
        let title = match tab {
            OptionsTab::Root => "Options",
            OptionsTab::Video => "Video Settings",
            OptionsTab::Controls => "Controls",
            OptionsTab::Chat => "Chat Settings",
            OptionsTab::Sound => "Music & Sounds",
        };
        self.menu_heading(ctx, mc, s, title, order);

        let mut changed = false;
        let mut done = false;
        let mut goto: Option<OptionsTab> = None;
        let max_h = (ctx.content_rect().height() - 100.0 * s).max(120.0);
        Area::new(Id::new(("options-screen", in_game)))
            .order(order)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 8.0 * s))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(10.0 * s, BTN_GAP * s);
                ui.set_width(ROW_W * s + 16.0);
                ScrollArea::vertical()
                    .max_height(max_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| match tab {
                            OptionsTab::Root => {
                                let (c, g) = root_tab(ui, mc, s, settings);
                                changed |= c;
                                goto = g;
                            }
                            OptionsTab::Video => changed |= video_tab(ui, mc, s, settings),
                            OptionsTab::Controls => changed |= controls_tab(ui, mc, s, settings),
                            OptionsTab::Chat => changed |= chat_tab(ui, mc, s, settings),
                            OptionsTab::Sound => changed |= sound_tab(ui, mc, s, settings),
                        });
                    });
                ui.add_space(6.0 * s);
                ui.vertical_centered(|ui| {
                    if mcui::button(ui, mc, BTN_W, s, "Done", true) {
                        done = true;
                    }
                });
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

    fn connecting_screen(&self, ctx: &egui::Context, mc: &McUi, s: f32) {
        self.menu_background(ctx, mc, s, Order::Background, false);
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("connecting")));
        let c = ctx.content_rect().center();
        mc.font.draw_anchored(
            &painter,
            c - vec2(0.0, 6.0 * s),
            Align2::CENTER_CENTER,
            "Connecting to the server...",
            s,
            Color32::WHITE,
            true,
        );
        mc.font.draw_anchored(
            &painter,
            c + vec2(0.0, 6.0 * s),
            Align2::CENTER_CENTER,
            &self.connecting_to,
            s,
            Color32::from_rgb(0xA0, 0xA0, 0xA0),
            true,
        );
    }

    // -- in-game pause menu --------------------------------------------------

    fn pause_menu(&mut self, ctx: &egui::Context, mc: &McUi, s: f32, actions: &mut Vec<HudAction>) {
        self.menu_background(ctx, mc, s, Order::Foreground, true);
        self.menu_heading(ctx, mc, s, "Game Menu", Order::Tooltip);

        let mut resume = false;
        let mut options = false;
        let mut disconnect = false;
        Area::new(Id::new("pause-menu"))
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = BTN_GAP * s;
                if mcui::button(ui, mc, BTN_W, s, "Back to Game", true) {
                    resume = true;
                }
                if mcui::button(ui, mc, BTN_W, s, "Options...", true) {
                    options = true;
                }
                if mcui::button(ui, mc, BTN_W, s, "Disconnect", true) {
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

    fn menu_heading(&self, ctx: &egui::Context, mc: &McUi, s: f32, text: &str, order: Order) {
        let painter = ctx.layer_painter(LayerId::new(order, Id::new(("menu-heading", order))));
        let r = ctx.content_rect();
        mc.font.draw_anchored(
            &painter,
            pos2(r.center().x, r.top() + 16.0 * s),
            Align2::CENTER_CENTER,
            text,
            s,
            Color32::WHITE,
            true,
        );
    }

    fn disconnect_screen(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        reason: &str,
        actions: &mut Vec<HudAction>,
    ) {
        self.menu_background(ctx, mc, s, Order::Background, false);
        self.menu_heading(ctx, mc, s, "Connection Lost", Order::Middle);

        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("disconnect-text")));
        let r = ctx.content_rect();
        let max_w = (BTN_W + 110.0) * s;
        let lines = wrap_text(mc, reason, s, max_w);
        let block_h = lines.len() as f32 * LINE_H * s;
        let mut y = r.center().y - 30.0 * s - block_h / 2.0;
        for line in &lines {
            mc.font.draw_anchored(
                &painter,
                pos2(r.center().x, y + LINE_H * s / 2.0),
                Align2::CENTER_CENTER,
                line,
                s,
                Color32::from_rgb(0xE0, 0xE0, 0xE0),
                true,
            );
            y += LINE_H * s;
        }

        let mut back = false;
        let mut quit = false;
        Area::new(Id::new("disconnect-buttons"))
            .order(Order::Middle)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 30.0 * s))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = BTN_GAP * s;
                if mcui::button(ui, mc, BTN_W, s, "Back to Title Screen", true) {
                    back = true;
                }
                if mcui::button(ui, mc, BTN_W, s, "Quit Game", true) {
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
    }
}

/// Greedy word wrap for the Minecraft font at scale `s`.
fn wrap_text(mc: &McUi, text: &str, s: f32, max_w: f32) -> Vec<String> {
    let mut out = Vec::new();
    for hard in text.split('\n') {
        let mut line = String::new();
        for word in hard.split(' ') {
            let cand = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if mc.font.width(&cand, s) <= max_w || line.is_empty() {
                line = cand;
            } else {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            }
        }
        out.push(line);
    }
    out
}

fn on_off(b: bool) -> &'static str {
    if b { "ON" } else { "OFF" }
}

fn gui_scale_label(n: u32) -> String {
    if n == 0 { "Auto".to_string() } else { format!("{n}x") }
}

/// A vanilla-style option slider over an f32 range at the given width. The
/// label is rebuilt from the current value every frame ("FOV: 90").
fn opt_slider_w(
    ui: &mut egui::Ui,
    mc: &McUi,
    w: f32,
    s: f32,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    label: impl Fn(f32) -> String,
) -> bool {
    let (min, max) = (*range.start(), *range.end());
    let mut t = ((*value - min) / (max - min)).clamp(0.0, 1.0);
    let text = label(*value);
    let changed = mcui::slider(ui, mc, w, s, &text, &mut t);
    if changed {
        *value = min + t * (max - min);
    }
    changed
}

/// Fixed vanilla key binds, shown read-only on the Controls screen.
const KEY_BINDS: &[(&str, &str)] = &[
    ("Move", "W A S D"),
    ("Jump", "Space"),
    ("Sneak", "Left Shift"),
    ("Sprint", "Left Ctrl"),
    ("Hotbar", "1 - 9"),
    ("Attack / Mine", "Left Mouse"),
    ("Use / Interact", "Right Mouse"),
    ("Chat", "T / Enter"),
    ("Pause", "Esc"),
    ("Debug Overlay", "F3"),
];

fn fov_label(v: f32) -> String {
    let r = v.round();
    if (r - 70.0).abs() < 0.5 {
        "FOV: Normal".to_string()
    } else if (r - 110.0).abs() < 0.5 {
        "FOV: Quake Pro".to_string()
    } else {
        format!("FOV: {r:.0}")
    }
}

/// Top-level Options: quick FOV/Brightness + links to the sub-screens, laid
/// out in vanilla's two-column rows.
fn root_tab(
    ui: &mut egui::Ui,
    mc: &McUi,
    s: f32,
    st: &mut GameSettings,
) -> (bool, Option<OptionsTab>) {
    let mut changed = false;
    let mut goto = None;
    ui.horizontal(|ui| {
        changed |= {
            let c = opt_slider_w(ui, mc, COL_W, s, &mut st.fov, 30.0..=110.0, fov_label);
            if c {
                st.fov = st.fov.round();
            }
            c
        };
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.brightness, 0.0..=1.0, |v| {
            if v <= 0.005 {
                "Brightness: Moody".to_string()
            } else if v >= 0.995 {
                "Brightness: Bright".to_string()
            } else {
                format!("Brightness: {:.0}%", v * 100.0)
            }
        });
    });
    ui.add_space(4.0 * s);
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, "Video Settings...", true) {
            goto = Some(OptionsTab::Video);
        }
        if mcui::button(ui, mc, COL_W, s, "Controls...", true) {
            goto = Some(OptionsTab::Controls);
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, "Music & Sounds...", true) {
            goto = Some(OptionsTab::Sound);
        }
        if mcui::button(ui, mc, COL_W, s, "Chat Settings...", true) {
            goto = Some(OptionsTab::Chat);
        }
    });
    (changed, goto)
}

/// Video Settings: everything that affects the renderer & window.
fn video_tab(ui: &mut egui::Ui, mc: &McUi, s: f32, st: &mut GameSettings) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        {
            let mut rd = st.render_distance as f32;
            if opt_slider_w(ui, mc, COL_W, s, &mut rd, 2.0..=32.0, |v| {
                format!("Render Distance: {:.0}", v.round())
            }) {
                st.render_distance = rd.round() as i32;
                changed = true;
            }
        }
        {
            let mut fps = st.max_fps as f32;
            if opt_slider_w(ui, mc, COL_W, s, &mut fps, 0.0..=260.0, |v| {
                let stepped = (v / 5.0).round() * 5.0;
                if stepped < 1.0 {
                    "Max FPS: Unlimited".to_string()
                } else {
                    format!("Max FPS: {stepped:.0}")
                }
            }) {
                st.max_fps = ((fps / 5.0).round() * 5.0) as u32;
                changed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.brightness, 0.0..=1.0, |v| {
            format!("Brightness: {:.0}%", v * 100.0)
        });
        if mcui::button(ui, mc, COL_W, s, &format!("GUI Scale: {}", gui_scale_label(st.gui_scale)), true) {
            st.gui_scale = (st.gui_scale + 1) % 5;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("VSync: {}", on_off(st.vsync)), true) {
            st.vsync = !st.vsync;
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Fullscreen: {}", on_off(st.fullscreen)), true) {
            st.fullscreen = !st.fullscreen;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Graphics: {}", st.graphics.label()), true) {
            st.graphics = st.graphics.next();
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Fog: {}", on_off(st.fog)), true) {
            st.fog = !st.fog;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("View Bobbing: {}", on_off(st.view_bobbing)), true) {
            st.view_bobbing = !st.view_bobbing;
            changed = true;
        }
    });
    changed
}

/// Controls: mouse settings + read-only key-bind reference.
fn controls_tab(ui: &mut egui::Ui, mc: &McUi, s: f32, st: &mut GameSettings) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.sensitivity_pct, 0.0..=200.0, |v| {
            if v <= 0.5 {
                "Sensitivity: *yawn*".to_string()
            } else if (v - 100.0).abs() < 0.5 {
                "Sensitivity: 100%".to_string()
            } else {
                format!("Sensitivity: {v:.0}%")
            }
        });
        if mcui::button(ui, mc, COL_W, s, &format!("Invert Mouse: {}", on_off(st.invert_mouse)), true) {
            st.invert_mouse = !st.invert_mouse;
            changed = true;
        }
    });
    ui.add_space(6.0 * s);
    mcui::label(ui, mc, s, "Key Binds", Color32::WHITE);
    ui.add_space(2.0 * s);
    for (action, key) in KEY_BINDS {
        let (rect, _) = ui.allocate_exact_size(vec2(ROW_W * s, LINE_H * s), Sense::hover());
        mc.font.draw(
            ui.painter(),
            rect.min,
            action,
            s,
            Color32::from_rgb(0xA0, 0xA0, 0xA0),
            true,
        );
        let w = mc.font.width(key, s);
        mc.font.draw(
            ui.painter(),
            pos2(rect.right() - w, rect.top()),
            key,
            s,
            Color32::WHITE,
            true,
        );
    }
    changed
}

/// Chat Settings: scale + background opacity.
fn chat_tab(ui: &mut egui::Ui, mc: &McUi, s: f32, st: &mut GameSettings) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.chat_scale, 0.5..=2.0, |v| {
            format!("Chat Text Size: {:.0}%", v * 100.0)
        });
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.chat_opacity, 0.0..=1.0, |v| {
            format!("Chat Opacity: {:.0}%", v * 100.0)
        });
    });
    changed
}

/// Volume label: `OFF` at zero, otherwise a percentage — exactly vanilla.
fn vol_label(name: &str, v: f32) -> String {
    if v <= 0.005 {
        format!("{name}: OFF")
    } else {
        format!("{name}: {:.0}%", v * 100.0)
    }
}

/// Music & Sounds: master on top, then category pairs — like vanilla's screen.
fn sound_tab(ui: &mut egui::Ui, mc: &McUi, s: f32, st: &mut GameSettings) -> bool {
    let mut changed = false;
    ui.vertical_centered(|ui| {
        changed |= opt_slider_w(ui, mc, BTN_W, s, &mut st.master_volume, 0.0..=1.0, |v| {
            vol_label("Master Volume", v)
        });
    });
    ui.add_space(2.0 * s);
    let mut fields: [(&str, &mut f32); 9] = [
        ("Music", &mut st.music_volume),
        ("Jukebox/Note Blocks", &mut st.records_volume),
        ("Weather", &mut st.weather_volume),
        ("Blocks", &mut st.blocks_volume),
        ("Hostile Creatures", &mut st.hostile_volume),
        ("Friendly Creatures", &mut st.neutral_volume),
        ("Players", &mut st.players_volume),
        ("Ambient/Environment", &mut st.ambient_volume),
        ("Voice/Speech", &mut st.voice_volume),
    ];
    for pair in fields.chunks_mut(2) {
        ui.horizontal(|ui| {
            for (name, value) in pair.iter_mut() {
                changed |= opt_slider_w(ui, mc, COL_W, s, value, 0.0..=1.0, |v| {
                    vol_label(name, v)
                });
            }
        });
    }
    changed
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
