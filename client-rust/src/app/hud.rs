//! egui HUD: crosshair, hotbar (9 slots + selection), chat log (last 10 lines,
//! fading) + chat input line, F3 debug (fps, pos, facing, section counts,
//! meshing queue), death/disconnect overlay, connect screen (server + username
//! when started without --server).
//! Pure egui — no wgpu here. The app calls `run()` each frame and forwards
//! returned actions (chat submit, respawn click, connect click) as Commands.

use crate::assets::items::ItemIcons;
use crate::bridge::events::ItemSnapshot;
use egui::{
    Align2, Area, Color32, FontId, Id, Key, Order, Rect, Sense, Stroke, StrokeKind, TextEdit,
    TextureId, Window, pos2, vec2,
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
    pub disconnect_reason: Option<String>,
}

pub enum HudAction {
    SendChat(String),
    Connect { address: String, username: String },
    Quit,
}

/// Chat lines older than this are hidden (unless the chat input is open).
const CHAT_VISIBLE_SECS: f32 = 15.0;
const CHAT_MAX_LINES: usize = 10;

pub struct Hud {
    pub show_debug: bool,
    pub chat_open: bool,
    _priv: (),
    /// (line, arrival time) — newest last, capped at CHAT_MAX_LINES.
    chat_lines: VecDeque<(String, Instant)>,
    chat_input: String,
    /// Connect-screen fields (persist across frames).
    address: String,
    username: String,
    /// Whether the connect screen was shown on the last `run` (its text fields
    /// want the keyboard).
    connect_shown: bool,
}

impl Hud {
    pub fn new() -> Self {
        Self {
            show_debug: false,
            chat_open: false,
            _priv: (),
            chat_lines: VecDeque::new(),
            chat_input: String::new(),
            address: "localhost".into(),
            username: "Dolphin".into(),
            connect_shown: false,
        }
    }

    /// True while a text field wants keyboard focus (app must not treat keys
    /// as movement).
    pub fn wants_keyboard(&self) -> bool {
        self.chat_open || self.connect_shown
    }

    pub fn push_chat(&mut self, line: String) {
        self.chat_lines.push_back((line, Instant::now()));
        while self.chat_lines.len() > CHAT_MAX_LINES {
            self.chat_lines.pop_front();
        }
    }

    /// Build the frame's UI. Called inside `egui::Context::run`.
    pub fn run(&mut self, ctx: &egui::Context, state: &HudState) -> Vec<HudAction> {
        let mut actions = Vec::new();

        if let Some(reason) = &state.disconnect_reason {
            self.connect_shown = false;
            self.disconnect_overlay(ctx, reason, &mut actions);
            return actions;
        }
        if !state.connected {
            self.connect_screen(ctx, &mut actions);
            return actions;
        }
        self.connect_shown = false;

        self.crosshair(ctx);
        self.hotbar(ctx, state);
        self.chat(ctx, &mut actions);
        if self.show_debug {
            self.debug_overlay(ctx, state);
        }
        actions
    }

    // -- pieces --------------------------------------------------------------

    fn crosshair(&self, ctx: &egui::Context) {
        let painter =
            ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::new("crosshair")));
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

    fn chat(&mut self, ctx: &egui::Context, actions: &mut Vec<HudAction>) {
        let chat_open = self.chat_open;
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
                    let bg = Color32::from_black_alpha((120.0 * alpha) as u8);
                    let fg = Color32::WHITE.gamma_multiply(alpha);
                    egui::Frame::NONE.fill(bg).inner_margin(2.0).show(ui, |ui| {
                        ui.label(egui::RichText::new(line).color(fg).size(13.0));
                    });
                }
                if chat_open {
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.chat_input)
                            .desired_width(400.0)
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

    fn disconnect_overlay(
        &mut self,
        ctx: &egui::Context,
        reason: &str,
        actions: &mut Vec<HudAction>,
    ) {
        Window::new("Disconnected")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(reason);
                ui.add_space(8.0);
                if ui.button("Quit").clicked() {
                    actions.push(HudAction::Quit);
                }
            });
    }

    fn connect_screen(&mut self, ctx: &egui::Context, actions: &mut Vec<HudAction>) {
        self.connect_shown = true;
        Window::new("Connect to server")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Address:");
                    ui.add(TextEdit::singleline(&mut self.address).desired_width(220.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Username:");
                    ui.add(TextEdit::singleline(&mut self.username).desired_width(220.0));
                });
                ui.add_space(8.0);
                let go = ui.button("Connect").clicked()
                    || ctx.input(|i| i.key_pressed(Key::Enter));
                if go && !self.address.trim().is_empty() && !self.username.trim().is_empty() {
                    actions.push(HudAction::Connect {
                        address: self.address.trim().to_string(),
                        username: self.username.trim().to_string(),
                    });
                }
            });
    }
}

impl Default for Hud {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_caps_at_ten_lines() {
        let mut hud = Hud::new();
        for i in 0..25 {
            hud.push_chat(format!("line {i}"));
        }
        assert_eq!(hud.chat_lines.len(), CHAT_MAX_LINES);
        assert_eq!(hud.chat_lines.front().unwrap().0, "line 15");
        assert_eq!(hud.chat_lines.back().unwrap().0, "line 24");
    }

    #[test]
    fn wants_keyboard_follows_chat_and_connect() {
        let mut hud = Hud::new();
        assert!(!hud.wants_keyboard());
        hud.chat_open = true;
        assert!(hud.wants_keyboard());
        hud.chat_open = false;
        hud.connect_shown = true;
        assert!(hud.wants_keyboard());
    }
}
