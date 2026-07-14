//! The launcher UI — a modern, animated, dark Minecraft launcher.
//!
//! Layout: a slim custom title bar, a horizontal **text** tab header
//! (Start · Spiel · Konten · Einstellungen) with an animated underline and a
//! live account chip, and a content area drawn over a gently animated aurora
//! backdrop. The Start page is a launch pad — your character on a lit stage,
//! live playtime stats, a glowing Play button, a version picker and one-tap
//! quick-join servers. The Spiel page exposes real in-game quick-settings
//! (render distance, FPS, FoV, GUI scale, brightness, graphics) that are
//! pre-written into the client's `options.json`. Everything is painted with
//! egui's own painter; settings save on change.

use std::f32::consts::TAU;

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Id, Rect, Rounding, Sense, Shape, Stroke,
    ViewportCommand, pos2, vec2,
};
use eframe::egui::epaint::{Mesh, Vertex, WHITE_UV};

use crate::app::{DolphinApp, LoginMethod, Tab};
use crate::config;

/* ---------------------------------------------------------------- */
/*  Palette — deep dark, one aqua accent + cool backdrop hues        */
/* ---------------------------------------------------------------- */

pub const BG_0: [u8; 3] = [0x0A, 0x0D, 0x12]; // window (deepest)
const BG_1: [u8; 3] = [0x10, 0x14, 0x1C]; // header / bars
const BG_2: [u8; 3] = [0x14, 0x19, 0x22]; // cards
const BG_3: [u8; 3] = [0x1D, 0x23, 0x2F]; // inputs / hover
const LINE: [u8; 3] = [0x26, 0x2E, 0x3B]; // hairline borders
const LINE_2: [u8; 3] = [0x35, 0x3F, 0x4F]; // brighter hairline
const TEXT: [u8; 3] = [0xEC, 0xF0, 0xF6];
const DIM: [u8; 3] = [0x9A, 0xA5, 0xB4];
const FAINT: [u8; 3] = [0x5C, 0x67, 0x76];
const ACCENT: [u8; 3] = [0x35, 0xE0, 0xC8]; // the one aqua accent (Play + active)
const GREEN: [u8; 3] = [0x46, 0xD6, 0xA0]; // "in game" state
const RED: [u8; 3] = [0xE0, 0x60, 0x4C];
const GOLD: [u8; 3] = [0xE7, 0xB2, 0x4A];
const INK: [u8; 3] = [0x05, 0x0A, 0x0E]; // text on the accent button
// Backdrop-only hues (never used on interactive elements).
const INDIGO: [u8; 3] = [0x46, 0x5F, 0xF0];
const VIOLET: [u8; 3] = [0x8A, 0x5C, 0xFF];
const PLAY_TOP: [u8; 3] = [0x57, 0xF0, 0xD6];
const PLAY_BOT: [u8; 3] = [0x1C, 0xBF, 0xD9];

fn c(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
fn soft(rgb: [u8; 3], a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], a)
}
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), 255)
}

fn running(app: &DolphinApp) -> bool {
    app.running.load(std::sync::atomic::Ordering::Relaxed)
}

fn seconds(ctx: &egui::Context) -> f32 {
    ctx.input(|i| i.time) as f32
}

const TITLEBAR_H: f32 = 44.0;
const TABSTRIP_H: f32 = 48.0;
const ROUND: f32 = 14.0;

/* ---------------------------------------------------------------- */
/*  Low-level painting helpers (gradients + soft glows)              */
/* ---------------------------------------------------------------- */

/// A vertical top→bottom gradient rectangle (via a 2-triangle vertex-coloured
/// mesh — egui has no native gradient fill).
fn v_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    let i = mesh.vertices.len() as u32;
    for (p, col) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ] {
        mesh.vertices.push(Vertex { pos: p, uv: WHITE_UV, color: col });
    }
    mesh.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    painter.add(Shape::mesh(mesh));
}

/// A soft radial glow (bright centre fading to fully transparent at `radius`),
/// drawn as a triangle fan. Cheap and smooth — the building block of the
/// animated backdrop and the Play-button aura.
fn glow(painter: &egui::Painter, center: egui::Pos2, radius: f32, inner: Color32) {
    const SEG: usize = 44;
    let outer = Color32::from_rgba_unmultiplied(inner.r(), inner.g(), inner.b(), 0);
    let mut mesh = Mesh::default();
    let c_idx = mesh.vertices.len() as u32;
    mesh.vertices.push(Vertex { pos: center, uv: WHITE_UV, color: inner });
    for k in 0..=SEG {
        let a = k as f32 / SEG as f32 * TAU;
        let p = center + vec2(a.cos(), a.sin()) * radius;
        mesh.vertices.push(Vertex { pos: p, uv: WHITE_UV, color: outer });
    }
    for k in 1..=SEG as u32 {
        mesh.indices.extend_from_slice(&[c_idx, c_idx + k, c_idx + k + 1]);
    }
    painter.add(Shape::mesh(mesh));
}

/// The living backdrop: drifting aurora orbs + a faint measured grid + a
/// bottom vignette. Deliberately low-contrast so content stays readable.
fn animated_backdrop(painter: &egui::Painter, rect: Rect, t: f32) {
    let cx = rect.center().x;
    let orbs = [
        (
            pos2(cx - 240.0 + (t * 0.13).sin() * 46.0, rect.top() + 120.0 + (t * 0.11).cos() * 34.0),
            460.0,
            soft(ACCENT, 30),
        ),
        (
            pos2(cx + 300.0 + (t * 0.10).cos() * 52.0, rect.top() + 60.0 + (t * 0.14).sin() * 30.0),
            420.0,
            soft(INDIGO, 26),
        ),
        (
            pos2(cx + 40.0 + (t * 0.08).sin() * 60.0, rect.bottom() - 60.0 + (t * 0.09).cos() * 40.0),
            520.0,
            soft(VIOLET, 20),
        ),
    ];
    for (p, r, col) in orbs {
        glow(painter, p, r, col);
    }

    // Faint measured grid — echoes the website's "Sonar" look.
    let grid = soft([0x3A, 0x45, 0x57], 8);
    let mut x = rect.left() + 40.0;
    while x < rect.right() {
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, grid));
        x += 72.0;
    }

    // Bottom vignette to seat the content.
    let vig = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 220.0), rect.right_bottom());
    v_gradient(painter, vig, Color32::TRANSPARENT, soft(BG_0, 210));
}

/* ---------------------------------------------------------------- */
/*  Theme                                                            */
/* ---------------------------------------------------------------- */

pub fn install_theme(ctx: &egui::Context, _accent_name: &str) {
    let accent = c(ACCENT);
    let round = Rounding::same(10.0);

    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(c(TEXT));
    v.panel_fill = c(BG_0);
    v.window_fill = c(BG_1);
    v.window_stroke = Stroke::new(1.0, c(LINE));
    v.window_rounding = Rounding::same(ROUND);
    v.extreme_bg_color = c(BG_3);
    v.faint_bg_color = c(BG_2);
    v.hyperlink_color = accent;
    // Slider trailing fill + selections use the accent.
    v.selection.bg_fill = accent;
    v.selection.stroke = Stroke::new(1.0, accent);
    v.popup_shadow = egui::epaint::Shadow {
        offset: vec2(0.0, 8.0),
        blur: 24.0,
        spread: 0.0,
        color: Color32::from_black_alpha(150),
    };

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.rounding = round;
    }
    v.widgets.noninteractive.bg_fill = c(BG_1);
    v.widgets.noninteractive.weak_bg_fill = c(BG_1);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c(LINE));
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c(DIM));

    v.widgets.inactive.bg_fill = c(BG_3);
    v.widgets.inactive.weak_bg_fill = c(BG_2);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, c(LINE));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, c(TEXT));

    v.widgets.hovered.bg_fill = c(BG_3);
    v.widgets.hovered.weak_bg_fill = c(BG_3);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, c(LINE_2));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, c(TEXT));

    v.widgets.active.bg_fill = c(BG_3);
    v.widgets.active.weak_bg_fill = c(BG_3);
    v.widgets.active.bg_stroke = Stroke::new(1.0, accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0, c(TEXT));

    let mut style = (*ctx.style()).clone();
    use egui::{FontFamily, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(22.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.button_padding = vec2(14.0, 8.0);
    style.spacing.interact_size = vec2(0.0, 30.0);
    style.visuals = v;
    ctx.set_style(style);
}

/* ---------------------------------------------------------------- */
/*  Frame                                                            */
/* ---------------------------------------------------------------- */

pub fn draw(app: &mut DolphinApp, ctx: &egui::Context) {
    title_bar(app, ctx);
    tab_strip(app, ctx);

    let t = seconds(ctx);
    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(c(BG_0)).inner_margin(egui::Margin::ZERO))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            animated_backdrop(ui.painter(), full, t);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(26.0);
                    content_column(ui, |ui| match app.tab {
                        Tab::Home => home_view(app, ui),
                        Tab::Game => game_view(app, ui),
                        Tab::Accounts => accounts_view(app, ui),
                        Tab::Settings => settings_view(app, ui),
                    });
                    ui.add_space(36.0);
                });
        });
}

/// Centered fixed-max-width content column.
fn content_column(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let w = 740.0_f32.min(ui.available_width() - 56.0);
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - w).max(0.0) / 2.0).max(0.0));
        ui.allocate_ui_with_layout(vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), add);
    });
}

/* ---------------------------------------------------------------- */
/*  Title bar                                                        */
/* ---------------------------------------------------------------- */

fn title_bar(app: &mut DolphinApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("titlebar")
        .exact_height(TITLEBAR_H)
        .frame(egui::Frame::none().fill(c(BG_1)))
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            let painter = ui.painter().clone();
            v_gradient(&painter, bar, c(BG_1), lerp_color(c(BG_1), c(BG_0), 0.6));

            let logo = Rect::from_center_size(pos2(bar.left() + 22.0, bar.center().y), vec2(22.0, 22.0));
            painter.image(
                app.logo.id(),
                logo,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            let wordmark = FontId::new(15.0, egui::FontFamily::Proportional);
            let r1 = painter.text(
                pos2(bar.left() + 42.0, bar.center().y),
                Align2::LEFT_CENTER,
                "Dolphin",
                wordmark.clone(),
                c(TEXT),
            );
            painter.text(pos2(r1.right(), bar.center().y), Align2::LEFT_CENTER, "Client", wordmark, c(ACCENT));

            // Window buttons (right → left).
            let mut x = bar.right() - 6.0;
            if window_button(ui, &mut x, bar, WindowButton::Close) {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            if window_button(ui, &mut x, bar, WindowButton::Maximize) {
                let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!is_max));
            }
            if window_button(ui, &mut x, bar, WindowButton::Minimize) {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            }

            let drag_zone = Rect::from_min_max(pos2(bar.left() + 150.0, bar.top()), pos2(x - 4.0, bar.bottom()));
            let resp = ui.interact(drag_zone, ui.id().with("drag"), Sense::click_and_drag());
            if resp.drag_started() {
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
            if resp.double_clicked() {
                let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!is_max));
            }
        });
}

enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

fn window_button(ui: &mut egui::Ui, x: &mut f32, bar: Rect, kind: WindowButton) -> bool {
    let size = vec2(32.0, 26.0);
    let rect = Rect::from_min_size(pos2(*x - size.x, bar.center().y - size.y / 2.0), size);
    *x -= size.x + 4.0;
    let resp = ui.interact(rect, ui.id().with(format!("winbtn{}", *x as i32)), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();
    let fill = match (&kind, hovered) {
        (WindowButton::Close, true) => c(RED),
        (_, true) => c(BG_3),
        _ => Color32::TRANSPARENT,
    };
    painter.rect_filled(rect, Rounding::same(7.0), fill);
    let center = rect.center();
    let col = if hovered { Color32::WHITE } else { c(DIM) };
    let s = Stroke::new(1.5, col);
    match kind {
        WindowButton::Minimize => {
            painter.line_segment([center + vec2(-5.0, 2.0), center + vec2(5.0, 2.0)], s);
        }
        WindowButton::Maximize => {
            painter.rect_stroke(Rect::from_center_size(center, vec2(9.0, 9.0)), Rounding::same(1.5), s);
        }
        WindowButton::Close => {
            painter.line_segment([center + vec2(-4.5, -4.5), center + vec2(4.5, 4.5)], s);
            painter.line_segment([center + vec2(-4.5, 4.5), center + vec2(4.5, -4.5)], s);
        }
    }
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/* ---------------------------------------------------------------- */
/*  Tab header — text tabs + animated underline + account chip       */
/* ---------------------------------------------------------------- */

fn tab_strip(app: &mut DolphinApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("tabs")
        .exact_height(TABSTRIP_H)
        .frame(egui::Frame::none().fill(c(BG_1)))
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            let painter = ui.painter().clone();
            v_gradient(&painter, bar, c(BG_1), c(BG_0));
            painter.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, c(LINE)));

            let mut x = bar.left() + 18.0;
            let mut active_span: Option<(f32, f32)> = None; // (center_x, width)
            for (label, tab) in [
                ("Start", Tab::Home),
                ("Spiel", Tab::Game),
                ("Konten", Tab::Accounts),
                ("Einstellungen", Tab::Settings),
            ] {
                let (clicked, rect) = tab_button(ui, &painter, &mut x, bar, label, app.tab == tab);
                if app.tab == tab {
                    active_span = Some((rect.center().x, rect.width() - 22.0));
                }
                if clicked {
                    app.tab = tab;
                }
            }

            // Underline slides smoothly to the active tab.
            if let Some((cx, w)) = active_span {
                let ax = ctx.animate_value_with_time(Id::new("ul_x"), cx, 0.16);
                let aw = ctx.animate_value_with_time(Id::new("ul_w"), w, 0.16);
                let y = bar.bottom() - 2.5;
                let ul = Rect::from_min_max(pos2(ax - aw / 2.0, y - 1.5), pos2(ax + aw / 2.0, y + 1.5));
                painter.rect_filled(ul, Rounding::same(2.0), c(ACCENT));
            }

            account_chip(ui, &painter, app, bar);

            if app.busy || app.progress > 0.001 {
                let w = bar.width() * app.progress.clamp(0.03, 1.0);
                painter.rect_filled(
                    Rect::from_min_size(pos2(bar.left(), bar.bottom() - 2.0), vec2(w, 2.0)),
                    Rounding::ZERO,
                    c(ACCENT),
                );
            }
        });
}

fn tab_button(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    x: &mut f32,
    bar: Rect,
    label: &str,
    active: bool,
) -> (bool, Rect) {
    let font = FontId::proportional(14.5);
    let galley = painter.layout_no_wrap(label.to_string(), font.clone(), c(TEXT));
    let pad = 12.0;
    let w = galley.size().x + pad * 2.0;
    let rect = Rect::from_min_size(pos2(*x, bar.top()), vec2(w, bar.height()));
    *x += w + 4.0;
    let resp = ui.interact(rect, ui.id().with(("tab", label)), Sense::click());
    let hov = resp.hovered();
    if hov && !active {
        painter.rect_filled(rect.shrink2(vec2(3.0, 9.0)), Rounding::same(8.0), soft(LINE_2, 40));
    }
    let col = if active {
        c(TEXT)
    } else if hov {
        c(TEXT)
    } else {
        c(DIM)
    };
    painter.text(rect.center(), Align2::CENTER_CENTER, label, font, col);
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    (resp.clicked(), rect)
}

/// The live account chip on the right of the tab header (avatar + name +
/// one-word state). Clicking it jumps to the Konten tab.
fn account_chip(ui: &mut egui::Ui, painter: &egui::Painter, app: &mut DolphinApp, bar: Rect) {
    let is_running = running(app);
    let active = app.accounts.active_account().cloned();
    let cy = bar.center().y;
    let av = Rect::from_min_size(pos2(bar.right() - 18.0 - 30.0, cy - 15.0), vec2(30.0, 30.0));
    draw_avatar(painter, app, av);

    let tx = av.left() - 10.0;
    let name = active
        .as_ref()
        .map(|a| a.username.clone())
        .unwrap_or_else(|| "Kein Konto".to_string());
    let (state, scol) = if app.relogin_for.is_some() {
        ("Anmeldung nötig", c(RED))
    } else if is_running {
        ("im Spiel", c(GREEN))
    } else if active.is_some() {
        ("bereit zum Spielen", c(DIM))
    } else {
        ("nicht angemeldet", c(FAINT))
    };
    painter.text(pos2(tx, cy - 8.0), Align2::RIGHT_CENTER, truncate(&name, 22), FontId::proportional(13.5), c(TEXT));
    painter.text(pos2(tx, cy + 8.0), Align2::RIGHT_CENTER, state, FontId::proportional(11.0), scol);

    let hit = Rect::from_min_max(pos2(tx - 150.0, bar.top()), pos2(av.right(), bar.bottom()));
    let resp = ui.interact(hit, ui.id().with("chip"), Sense::click());
    if resp.clicked() {
        app.tab = Tab::Accounts;
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
}

/* ---------------------------------------------------------------- */
/*  Home — a launch pad: character, live stats, Play, quick servers  */
/* ---------------------------------------------------------------- */

fn home_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let active = app.accounts.active_account().cloned();
    let is_running = running(app);

    // Update banner (only shown when auto-install is off, or while it works).
    if let Some(info) = app.update_note.lock().ok().and_then(|n| n.clone()) {
        card_tinted(ui, GOLD, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Update {} verfügbar", info.version)).strong().color(c(TEXT)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if accent_button(ui, "Jetzt aktualisieren", !app.busy) {
                        app.start_self_update(&ctx, info.clone());
                    }
                });
            });
        });
        ui.add_space(14.0);
    }

    relogin_banner(app, ui);
    login_prompts(app, ui);

    match &active {
        None => {
            ui.add_space(28.0);
            signed_out_card(app, ui);
        }
        Some(acc) => {
            let t = seconds(&ctx);
            ui.add_space(4.0);

            // Character render on a lit stage — soft floor glow, no hard shadow.
            let body = app.body.lock().ok().and_then(|b| b.clone());
            let stage_h = 292.0;
            let (stage, _) = ui.allocate_exact_size(vec2(ui.available_width(), stage_h), Sense::hover());
            let p = ui.painter().clone();
            // Floor glow under the character.
            glow(&p, pos2(stage.center().x, stage.bottom() - 6.0), 150.0, soft(ACCENT, 34));
            p.line_segment(
                [pos2(stage.center().x - 120.0, stage.bottom()), pos2(stage.center().x + 120.0, stage.bottom())],
                Stroke::new(1.0, soft(ACCENT, 70)),
            );
            if let Some(tex) = body {
                let [tw, th] = tex.size();
                if tw > 0 && th > 0 {
                    let target_h = stage_h - 10.0;
                    let target_w = target_h * tw as f32 / th as f32;
                    // Gentle idle bob.
                    let bob = (t * 1.1).sin() * 4.0;
                    let bx = stage.center().x - target_w / 2.0;
                    p.image(
                        tex.id(),
                        Rect::from_min_size(pos2(bx, stage.top() + bob), vec2(target_w, target_h)),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            } else {
                p.text(stage.center(), Align2::CENTER_CENTER, "…", FontId::proportional(20.0), c(FAINT));
            }

            ui.add_space(2.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(&acc.username).size(28.0).strong().color(c(TEXT)));
                ui.add_space(2.0);
                let ver = if app.settings.client_version.is_empty() {
                    "Neueste".to_string()
                } else {
                    app.settings.client_version.clone()
                };
                let line = format!("DolphinClient {ver}  ·  Minecraft {}", config::TARGET_VERSION);
                ui.label(egui::RichText::new(line).size(12.5).color(c(DIM)));
            });

            ui.add_space(16.0);
            // Live stat pills.
            let stats = app.stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
            stat_pills(ui, &stats);

            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                if play_button(ui, is_running, app.busy, t) && !app.busy && !is_running {
                    app.start_launch(&ctx);
                }
                ui.add_space(12.0);
                ui.allocate_ui_with_layout(vec2(230.0, 34.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                    version_combo(app, ui);
                });

                if !app.status.is_empty() {
                    ui.add_space(10.0);
                    let scol = if app.status.starts_with("Fehler") || app.relogin_for.is_some() {
                        c(RED)
                    } else if is_running {
                        c(GREEN)
                    } else {
                        c(DIM)
                    };
                    ui.label(egui::RichText::new(truncate(&app.status, 72)).size(12.0).color(scol));
                }
            });

            // One-tap quick-join servers.
            if !app.settings.servers.is_empty() {
                ui.add_space(24.0);
                quick_servers(app, ui, is_running);
            }

            // Playtime history sparkline.
            if !stats.sessions.is_empty() {
                ui.add_space(18.0);
                playtime_card(ui, &stats);
            }
        }
    }
}

/// A row of small stat pills (playtime · launches · avg session · last played).
fn stat_pills(ui: &mut egui::Ui, stats: &config::Stats) {
    let items = [
        (fmt_playtime(stats.playtime_secs), "Gesamt-Spielzeit"),
        (stats.launches.to_string(), "Starts"),
        (
            if stats.avg_session_secs() > 0 { fmt_playtime(stats.avg_session_secs()) } else { "—".into() },
            "Ø Sitzung",
        ),
        (fmt_relative(stats.last_played), "Zuletzt gespielt"),
    ];
    let gap = 12.0;
    let pill_w = ((ui.available_width() - gap * 3.0) / 4.0).clamp(120.0, 180.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (value, label) in items {
            stat_pill(ui, pill_w, &value, label);
        }
    });
}

fn stat_pill(ui: &mut egui::Ui, w: f32, value: &str, label: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(w, 66.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(12.0), c(BG_2));
    p.rect_stroke(rect, Rounding::same(12.0), Stroke::new(1.0, c(LINE)));
    // Subtle top highlight for depth.
    p.line_segment(
        [pos2(rect.left() + 12.0, rect.top() + 1.0), pos2(rect.right() - 12.0, rect.top() + 1.0)],
        Stroke::new(1.0, soft(LINE_2, 90)),
    );
    p.text(pos2(rect.left() + 15.0, rect.top() + 24.0), Align2::LEFT_CENTER, value, FontId::proportional(19.0), c(TEXT));
    p.text(pos2(rect.left() + 15.0, rect.bottom() - 17.0), Align2::LEFT_CENTER, label, FontId::proportional(11.0), c(DIM));
}

/// The big glowing Play button — gradient fill, breathing aura, top sheen.
fn play_button(ui: &mut egui::Ui, is_running: bool, busy: bool, t: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(248.0, 58.0), Sense::click());
    let enabled = !busy && !is_running;
    let hov = enabled && resp.hovered();
    let painter = ui.painter();
    let rounding = Rounding::same(ROUND);

    if enabled {
        // Breathing aura behind the button.
        let pulse = 0.5 + 0.5 * (t * 1.6).sin();
        let aura_a = (34.0 + pulse * 26.0 + if hov { 34.0 } else { 0.0 }) as u8;
        glow(painter, rect.center() + vec2(0.0, 4.0), rect.width() * 0.62, soft(ACCENT, aura_a));
        // Gradient fill.
        let (top, bot) = if hov {
            (lerp_color(c(PLAY_TOP), Color32::WHITE, 0.14), lerp_color(c(PLAY_BOT), Color32::WHITE, 0.06))
        } else {
            (c(PLAY_TOP), c(PLAY_BOT))
        };
        painter.rect_filled(rect, rounding, bot);
        v_gradient(painter, rect, top, bot);
        // Glass sheen on the top half.
        let sheen = Rect::from_min_max(rect.left_top(), pos2(rect.right(), rect.center().y + 4.0));
        v_gradient(painter, sheen, soft([0xFF, 0xFF, 0xFF], 40), Color32::TRANSPARENT);
    } else {
        painter.rect_filled(rect, rounding, c(BG_3));
        painter.rect_stroke(rect, rounding, Stroke::new(1.0, c(LINE)));
    }

    let (label, show_tri) = if is_running {
        ("LÄUFT", false)
    } else if busy {
        ("…", false)
    } else {
        ("SPIELEN", true)
    };
    let fg = if enabled { c(INK) } else { c(DIM) };
    let mut txp = rect.center().x;
    if show_tri {
        txp += 12.0;
        let cy = rect.center().y;
        let lx = rect.center().x - 58.0;
        painter.add(egui::Shape::convex_polygon(
            vec![pos2(lx, cy - 8.5), pos2(lx + 14.0, cy), pos2(lx, cy + 8.5)],
            fg,
            Stroke::NONE,
        ));
    }
    painter.text(pos2(txp, rect.center().y), Align2::CENTER_CENTER, label, FontId::new(17.5, egui::FontFamily::Proportional), fg);
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked() && enabled
}

fn version_combo(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let versions = app.versions.lock().map(|v| v.clone()).unwrap_or_default();
    let mut sel = app.settings.client_version.clone();
    let label = if sel.is_empty() {
        "Version · Neueste".to_string()
    } else {
        format!("Version · {sel}")
    };
    egui::ComboBox::from_id_salt("verpick")
        .selected_text(egui::RichText::new(label).size(13.0))
        .width(226.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut sel, String::new(), "Neueste (empfohlen)");
            for v in &versions {
                ui.selectable_value(&mut sel, v.clone(), v);
            }
        });
    if sel != app.settings.client_version {
        app.settings.client_version = sel;
        app.settings.save();
    }
}

/// Home quick-join: a wrapped row of server chips that launch straight in.
fn quick_servers(app: &mut DolphinApp, ui: &mut egui::Ui, is_running: bool) {
    let servers = app.settings.servers.clone();
    let ctx = ui.ctx().clone();
    section_label(ui, "Schnell beitreten");
    ui.add_space(8.0);
    let mut join: Option<String> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        for s in &servers {
            if server_chip(ui, &s.name, &s.address) && !app.busy && !is_running {
                join = Some(s.address.clone());
            }
        }
    });
    if let Some(addr) = join {
        app.launch_server(&ctx, addr);
    }
}

fn server_chip(ui: &mut egui::Ui, name: &str, addr: &str) -> bool {
    let label = truncate(name, 22);
    let font = FontId::proportional(13.5);
    let galley = ui.painter().layout_no_wrap(label.clone(), font.clone(), c(TEXT));
    let w = (galley.size().x + 44.0).max(120.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 44.0), Sense::click());
    let hov = resp.hovered();
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(10.0), if hov { c(BG_3) } else { c(BG_2) });
    p.rect_stroke(rect, Rounding::same(10.0), Stroke::new(1.0, if hov { c(ACCENT) } else { c(LINE) }));
    // Play triangle.
    let cy = rect.center().y;
    let lx = rect.left() + 15.0;
    p.add(egui::Shape::convex_polygon(
        vec![pos2(lx, cy - 6.0), pos2(lx + 9.0, cy), pos2(lx, cy + 6.0)],
        c(ACCENT),
        Stroke::NONE,
    ));
    p.text(pos2(lx + 18.0, cy - 6.0), Align2::LEFT_CENTER, label, font, c(TEXT));
    p.text(pos2(lx + 18.0, cy + 9.0), Align2::LEFT_CENTER, truncate(addr, 26), FontId::proportional(10.5), c(DIM));
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// A card with the recent-sessions sparkline (bar chart of session lengths).
fn playtime_card(ui: &mut egui::Ui, stats: &config::Stats) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Spielverlauf").strong().color(c(TEXT)).size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("letzte {} Sitzungen", stats.sessions.len()))
                        .color(c(DIM))
                        .size(12.0),
                );
            });
        });
        ui.add_space(12.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
        let p = ui.painter();
        let max = stats.sessions.iter().map(|s| s.secs).max().unwrap_or(1).max(1) as f32;
        let n = stats.sessions.len().max(1);
        let gap = 4.0;
        let bw = ((rect.width() - gap * (n as f32 - 1.0)) / n as f32).clamp(3.0, 26.0);
        let mut x = rect.left();
        for s in &stats.sessions {
            let h = (s.secs as f32 / max * rect.height()).max(3.0);
            let bar = Rect::from_min_max(pos2(x, rect.bottom() - h), pos2(x + bw, rect.bottom()));
            v_gradient(p, bar, c(PLAY_TOP), c(PLAY_BOT));
            x += bw + gap;
        }
    });
}

/// The signed-out prompt (used on Home when no account exists).
fn signed_out_card(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    card(ui, |ui| {
        ui.label(egui::RichText::new("Melde dich an").heading().color(c(TEXT)));
        ui.add_space(2.0);
        ui.label(egui::RichText::new("Mit deinem Microsoft-Konto anmelden — oder ein bestehendes Konto von diesem PC übernehmen.").color(c(DIM)));
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if accent_button(ui, "Mit Microsoft anmelden", !app.busy) {
                app.add_microsoft(&ctx);
            }
            if ghost_button(ui, "Konto übernehmen", true) {
                app.import_accounts();
            }
            if crate::tokens::has_token() && ghost_button(ui, "Vorheriges Konto", true) {
                app.start_login(&ctx, LoginMethod::Refresh);
            }
        });
        if let Some(note) = &app.import_note {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(note).color(c(GREEN)));
        }
    });
}

/// Prominent re-login prompt shown when an account's session could not be
/// resolved at launch (even after re-importing from other launchers).
fn relogin_banner(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let Some(name) = app.relogin_for.clone() else { return };
    let ctx = ui.ctx().clone();
    card_tinted(ui, RED, |ui| {
        ui.label(egui::RichText::new(format!("Anmeldung für {name} nötig")).strong().color(c(TEXT)));
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("Die Sitzung konnte nicht automatisch übernommen werden. Bitte melde dich neu an.")
                .color(c(DIM))
                .size(12.5),
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if accent_button(ui, "Neu anmelden", !app.busy) {
                app.add_microsoft(&ctx);
            }
            if ghost_button(ui, "Konto übernehmen", true) {
                app.import_accounts();
            }
        });
    });
    ui.add_space(14.0);
}

/* ---------------------------------------------------------------- */
/*  Spiel — real in-game quick-settings + saved servers              */
/* ---------------------------------------------------------------- */

fn game_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    page_head(ui, "Spiel", "Grafik- und Leistungseinstellungen — sie greifen beim nächsten Spielstart.");
    ui.add_space(14.0);

    let is_running = running(app);
    if is_running {
        card_tinted(ui, GOLD, |ui| {
            ui.label(
                egui::RichText::new("Das Spiel läuft gerade — Änderungen sind erst nach dem Beenden möglich.")
                    .color(c(TEXT))
                    .size(12.5),
            );
        });
        ui.add_space(12.0);
    }

    ui.add_enabled_ui(!is_running, |ui| {
        card(ui, |ui| {
            section_title(ui, "Leistung");
            ui.add_space(2.0);

            let mut rd = app.gameopts.render_distance();
            if slider_setting(ui, "Sichtweite", &format!("{rd} Chunks"), |ui| {
                ui.spacing_mut().slider_width = ui.available_width() - 4.0;
                ui.add(egui::Slider::new(&mut rd, 2..=32).trailing_fill(true).show_value(false))
            }) {
                app.gameopts.set_render_distance(rd);
                app.gameopts.save();
            }

            let mut fps = app.gameopts.max_fps();
            let fps_label = if fps == 0 { "Unbegrenzt".to_string() } else { format!("{fps} FPS") };
            if slider_setting(ui, "Bildrate-Limit", &fps_label, |ui| {
                ui.spacing_mut().slider_width = ui.available_width() - 4.0;
                ui.add(egui::Slider::new(&mut fps, 0..=360).trailing_fill(true).show_value(false).step_by(10.0))
            }) {
                app.gameopts.set_max_fps(fps);
                app.gameopts.save();
            }

            let mut vsync = app.gameopts.vsync();
            if toggle_row(ui, "VSync", "Bildrate an die Bildwiederholrate des Monitors koppeln.", &mut vsync) {
                app.gameopts.set_vsync(vsync);
                app.gameopts.save();
            }
        });
        ui.add_space(12.0);

        card(ui, |ui| {
            section_title(ui, "Anzeige");
            ui.add_space(2.0);

            let mut fov = app.gameopts.fov();
            if slider_setting(ui, "Sichtfeld (FoV)", &format!("{:.0}°", fov), |ui| {
                ui.spacing_mut().slider_width = ui.available_width() - 4.0;
                ui.add(egui::Slider::new(&mut fov, 30.0..=110.0).trailing_fill(true).show_value(false))
            }) {
                app.gameopts.set_fov(fov);
                app.gameopts.save();
            }

            let mut br = app.gameopts.brightness();
            if slider_setting(ui, "Helligkeit", &format!("{:.0}%", br * 100.0), |ui| {
                ui.spacing_mut().slider_width = ui.available_width() - 4.0;
                ui.add(egui::Slider::new(&mut br, 0.0..=1.0).trailing_fill(true).show_value(false))
            }) {
                app.gameopts.set_brightness(br);
                app.gameopts.save();
            }

            // GUI scale — 0 = Auto.
            let gs = app.gameopts.gui_scale();
            row_head(ui, "GUI-Größe", "0 = automatisch an die Fenstergröße anpassen.");
            let opts = ["Auto", "1", "2", "3", "4"];
            if let Some(sel) = segmented(ui, &opts, gs as usize) {
                app.gameopts.set_gui_scale(sel as u32);
                app.gameopts.save();
            }
            ui.add_space(6.0);

            // Graphics preset.
            let g = app.gameopts.graphics();
            row_head(ui, "Grafik", "Fancy sieht besser aus, Fast bringt mehr FPS.");
            let cur = if g == crate::gameopts::Graphics::Fast { 0 } else { 1 };
            if let Some(sel) = segmented(ui, &["Fast", "Fancy"], cur) {
                let ng = if sel == 0 { crate::gameopts::Graphics::Fast } else { crate::gameopts::Graphics::Fancy };
                app.gameopts.set_graphics(ng);
                app.gameopts.save();
            }
            ui.add_space(4.0);

            if toggle_row(ui, "Vollbild starten", "Das Spiel direkt im Vollbild öffnen.", &mut app.settings.fullscreen) {
                app.settings.save();
                app.gameopts.set_fullscreen(app.settings.fullscreen);
                app.gameopts.save();
            }
        });
    });
    ui.add_space(12.0);

    servers_card(app, ui);
}

/// Saved-servers editor (add / remove / pin default). The pinned default is the
/// address the client auto-joins on a plain "Spielen".
fn servers_card(app: &mut DolphinApp, ui: &mut egui::Ui) {
    card(ui, |ui| {
        section_title(ui, "Server");
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("Speichere deine Lieblingsserver für den Ein-Klick-Beitritt auf der Startseite.")
                .color(c(DIM))
                .size(12.0),
        );
        ui.add_space(12.0);

        // Existing servers.
        let servers = app.settings.servers.clone();
        let default_addr = app.settings.server.clone();
        let mut remove: Option<usize> = None;
        let mut make_default: Option<String> = None;
        for (i, s) in servers.iter().enumerate() {
            let is_default = !default_addr.is_empty() && default_addr == s.address;
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(rect, Rounding::same(10.0), c(BG_3));
            p.rect_stroke(rect, Rounding::same(10.0), Stroke::new(1.0, if is_default { c(ACCENT) } else { c(LINE) }));
            p.text(pos2(rect.left() + 14.0, rect.center().y - 8.0), Align2::LEFT_CENTER, truncate(&s.name, 30), FontId::proportional(14.5), c(TEXT));
            p.text(pos2(rect.left() + 14.0, rect.center().y + 9.0), Align2::LEFT_CENTER, truncate(&s.address, 40), FontId::proportional(11.5), c(DIM));

            let mut bx = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_max(pos2(rect.right() - 250.0, rect.top()), rect.right_bottom()))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            bx.add_space(14.0);
            if small_button(&mut bx, "Entfernen", c(RED)) {
                remove = Some(i);
            }
            if is_default {
                bx.label(egui::RichText::new("★ Standard").color(c(ACCENT)).size(12.5));
            } else if small_button(&mut bx, "Als Standard", c(ACCENT)) {
                make_default = Some(s.address.clone());
            }
            ui.add_space(8.0);
        }
        if let Some(i) = remove {
            app.remove_server(i);
        }
        if let Some(addr) = make_default {
            app.set_default_server(&addr);
        }

        if !servers.is_empty() {
            ui.add_space(6.0);
        }
        // Add form.
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.new_server_name)
                    .hint_text("Name (optional)")
                    .desired_width(150.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut app.new_server_addr)
                    .hint_text("Adresse, z. B. play.example.net")
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(8.0);
        if accent_button(ui, "Server hinzufügen", !app.new_server_addr.trim().is_empty()) {
            app.add_server();
        }
        if !default_addr.is_empty() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Beim Start wird automatisch {default_addr} beigetreten.")).color(c(DIM)).size(11.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if small_button(ui, "Standard löschen", c(DIM)) {
                        app.set_default_server("");
                    }
                });
            });
        }
    });
}

/* ---------------------------------------------------------------- */
/*  Accounts                                                         */
/* ---------------------------------------------------------------- */

fn accounts_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    page_head(ui, "Konten", "Mehrere Microsoft-Konten verwalten oder ein bestehendes Konto von diesem PC übernehmen.");
    ui.add_space(14.0);

    relogin_banner(app, ui);

    ui.horizontal(|ui| {
        if accent_button(ui, "Microsoft-Konto hinzufügen", !app.busy) {
            app.add_microsoft(&ctx);
        }
        if ghost_button(ui, "Konto übernehmen", true) {
            app.import_accounts();
        }
    });
    if let Some(note) = &app.import_note {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(note).color(c(GREEN)));
    }
    ui.add_space(12.0);

    login_prompts(app, ui);

    let accounts = app.accounts.accounts.clone();
    if accounts.is_empty() {
        card(ui, |ui| {
            ui.label(egui::RichText::new("Noch keine Konten. Füge eines hinzu oder übernimm ein bestehendes.").color(c(DIM)));
        });
    }
    let mut switch_to: Option<String> = None;
    let mut remove: Option<String> = None;
    for a in &accounts {
        let is_active = app.accounts.is_active(&a.uuid);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
        let p = ui.painter().clone();
        card_bg(&p, rect, is_active);
        let av = Rect::from_min_size(pos2(rect.left() + 14.0, rect.center().y - 18.0), vec2(36.0, 36.0));
        // Show the live head for the active account, initials otherwise.
        let tex = if is_active { app.avatar.lock().ok().and_then(|a| a.clone()) } else { None };
        p.rect_filled(av, Rounding::same(9.0), c(BG_3));
        if let Some(tex) = tex {
            p.image(tex.id(), av.shrink(3.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            p.text(av.center(), Align2::CENTER_CENTER, initials(&a.username), FontId::proportional(14.0), c(TEXT));
        }
        p.rect_stroke(av, Rounding::same(9.0), Stroke::new(1.0, c(LINE_2)));
        p.text(pos2(rect.left() + 64.0, rect.center().y - 8.0), Align2::LEFT_CENTER, &a.username, FontId::proportional(15.0), c(TEXT));
        p.text(pos2(rect.left() + 64.0, rect.center().y + 9.0), Align2::LEFT_CENTER, truncate(&a.source, 30), FontId::proportional(11.0), c(DIM));
        let mut bx = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.right() - 220.0, rect.top()), rect.right_bottom()))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        bx.add_space(14.0);
        if small_button(&mut bx, "Entfernen", c(RED)) {
            remove = Some(a.uuid.clone());
        }
        if is_active {
            bx.label(egui::RichText::new("● Aktiv").color(c(ACCENT)).size(13.0));
        } else if small_button(&mut bx, "Auswählen", c(ACCENT)) {
            switch_to = Some(a.uuid.clone());
        }
        ui.add_space(10.0);
    }
    if let Some(uuid) = switch_to {
        app.accounts.set_active(&uuid);
        if let Some(a) = app.accounts.active_account() {
            app.status = format!("Aktives Konto: {}", a.username);
        }
    }
    if let Some(uuid) = remove {
        app.accounts.remove(&uuid);
        app.status = "Konto entfernt.".to_string();
    }
}

/* ---------------------------------------------------------------- */
/*  Settings — launcher only                                         */
/* ---------------------------------------------------------------- */

fn settings_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    page_head(ui, "Einstellungen", "Änderungen werden sofort gespeichert.");
    ui.add_space(14.0);

    card(ui, |ui| {
        section_title(ui, "Start & Updates");
        ui.add_space(2.0);
        if toggle_row(ui, "Mit dem System starten", "DolphinClient öffnet sich automatisch, sobald du dich am PC anmeldest.", &mut app.settings.autostart) {
            app.settings.save();
            let _ = crate::autostart::set(app.settings.autostart);
        }
        toggle_row(ui, "Launcher nach Start schließen", "Das Fenster schließen, sobald das Spiel läuft.", &mut app.settings.close_on_launch)
            .then(|| app.settings.save());
        toggle_row(ui, "Nach Updates suchen", "Beim Start prüfen, ob eine neuere Version bereitsteht.", &mut app.settings.auto_update)
            .then(|| app.settings.save());
        toggle_row(ui, "Updates automatisch installieren", "Gefundene Updates ohne Nachfrage einspielen — der Launcher startet dafür kurz neu.", &mut app.settings.auto_update_apply)
            .then(|| app.settings.save());
    });
    ui.add_space(12.0);

    card(ui, |ui| {
        section_title(ui, "Discord");
        ui.add_space(2.0);
        toggle_row(ui, "Rich Presence", "Zeigt in deinem Discord-Profil, dass du DolphinClient offen hast.", &mut app.settings.discord_rpc)
            .then(|| app.settings.save());
    });
    ui.add_space(14.0);

    ui.horizontal(|ui| {
        if app.accounts.active_account().is_some() && ghost_button(ui, "Aktives Konto abmelden", true) {
            app.remove_active();
        }
        if ghost_button(ui, "Spielordner öffnen", true) {
            let _ = open::that(config::minecraft_dir());
        }
        if ghost_button(ui, "Protokoll", true) {
            app.show_log = !app.show_log;
        }
    });
    if app.show_log {
        ui.add_space(10.0);
        log_box(app, ui);
    }
}

/* ---------------------------------------------------------------- */
/*  Shared bits                                                      */
/* ---------------------------------------------------------------- */

fn page_head(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.label(egui::RichText::new(title).size(26.0).strong().color(c(TEXT)));
    ui.add_space(3.0);
    ui.label(egui::RichText::new(sub).color(c(DIM)));
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).strong().color(c(TEXT)).size(15.5));
}

/// Small dimmed uppercase-ish section label used on the Home page.
fn section_label(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).strong().color(c(DIM)).size(12.5));
}

fn row_head(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong().color(c(TEXT)));
    });
    ui.label(egui::RichText::new(sub).color(c(DIM)).size(12.0));
    ui.add_space(6.0);
}

/// A slider setting row: title + live value on one line, full-width slider below.
fn slider_setting(
    ui: &mut egui::Ui,
    title: &str,
    value: &str,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> bool {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong().color(c(TEXT)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).color(c(ACCENT)).monospace().size(12.5));
        });
    });
    ui.add_space(6.0);
    add(ui).changed()
}

/// A segmented pill control; returns `Some(index)` when a new segment is chosen.
fn segmented(ui: &mut egui::Ui, options: &[&str], selected: usize) -> Option<usize> {
    let h = 34.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, Rounding::same(10.0), c(BG_3));
    p.rect_stroke(rect, Rounding::same(10.0), Stroke::new(1.0, c(LINE)));
    let n = options.len().max(1);
    let seg_w = rect.width() / n as f32;
    let mut chosen = None;
    for (i, label) in options.iter().enumerate() {
        let sr = Rect::from_min_size(pos2(rect.left() + seg_w * i as f32, rect.top()), vec2(seg_w, h));
        let resp = ui.interact(sr, ui.id().with(("seg", label, i)), Sense::click());
        let is_sel = i == selected;
        if is_sel {
            p.rect_filled(sr.shrink(3.0), Rounding::same(8.0), soft(ACCENT, 40));
            p.rect_stroke(sr.shrink(3.0), Rounding::same(8.0), Stroke::new(1.0, c(ACCENT)));
        } else if resp.hovered() {
            p.rect_filled(sr.shrink(3.0), Rounding::same(8.0), soft(LINE_2, 40));
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let col = if is_sel { c(TEXT) } else { c(DIM) };
        p.text(sr.center(), Align2::CENTER_CENTER, *label, FontId::proportional(13.5), col);
        if resp.clicked() && !is_sel {
            chosen = Some(i);
        }
    }
    chosen
}

fn toggle_row(ui: &mut egui::Ui, title: &str, sub: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).strong().color(c(TEXT)));
            ui.label(egui::RichText::new(sub).color(c(DIM)).size(12.0));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = toggle(ui, on);
        });
    });
    ui.add_space(4.0);
    changed
}

fn toggle(ui: &mut egui::Ui, on: &mut bool) -> bool {
    let size = vec2(44.0, 25.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let mut changed = false;
    if resp.clicked() {
        *on = !*on;
        changed = true;
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let radius = rect.height() / 2.0;
    let bg = lerp_color(c(BG_3), c(ACCENT), t);
    ui.painter().rect_filled(rect, radius, bg);
    if t < 0.5 {
        ui.painter().rect_stroke(rect, radius, Stroke::new(1.0, c(LINE_2)));
    }
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    ui.painter().circle_filled(pos2(knob_x, rect.center().y), radius - 4.0, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    changed
}

fn login_prompts(app: &mut DolphinApp, ui: &mut egui::Ui) {
    if let Some((link, code)) = app.device.clone() {
        card(ui, |ui| {
            ui.label(egui::RichText::new("Anmeldung im Browser").strong().color(c(TEXT)));
            ui.label(egui::RichText::new("Ein Browser-Fenster wurde geöffnet — dort anmelden.").color(c(DIM)));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(&code).size(24.0).color(c(ACCENT)).strong());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ghost_button(ui, "Link erneut öffnen", true) {
                    let _ = open::that(&link);
                }
                if ghost_button(ui, "Code kopieren", true) {
                    ui.output_mut(|o| o.copied_text = code.clone());
                }
            });
        });
        ui.add_space(12.0);
    }
    if let Some(url) = app.auth_url.clone() {
        card(ui, |ui| {
            ui.label(egui::RichText::new("Ein Browser-Fenster wurde geöffnet — dort anmelden.").color(c(DIM)));
            ui.add_space(6.0);
            if ghost_button(ui, "Browser erneut öffnen", true) {
                let _ = open::that(&url);
            }
        });
        ui.add_space(12.0);
    }
}

fn log_box(app: &DolphinApp, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(c(BG_2))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(ROUND))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().max_height(180.0).stick_to_bottom(true).show(ui, |ui| {
                if app.log.is_empty() {
                    ui.label(egui::RichText::new("— noch keine Ausgaben —").color(c(FAINT)));
                }
                for line in &app.log {
                    ui.label(egui::RichText::new(line).monospace().size(11.5).color(c(DIM)));
                }
            });
        });
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(soft(BG_2, 235))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(ROUND))
        .inner_margin(egui::Margin::same(18.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn card_tinted(ui: &mut egui::Ui, rgb: [u8; 3], add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(soft(rgb, 22))
        .stroke(Stroke::new(1.0, soft(rgb, 110)))
        .rounding(Rounding::same(ROUND))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// Flat card background + hairline (brighter when active/selected).
fn card_bg(painter: &egui::Painter, rect: Rect, active: bool) {
    painter.rect_filled(rect, Rounding::same(ROUND), c(BG_2));
    let stroke = if active { Stroke::new(1.5, c(ACCENT)) } else { Stroke::new(1.0, c(LINE)) };
    painter.rect_stroke(rect, Rounding::same(ROUND), stroke);
}

fn accent_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let text_col = if enabled { c(INK) } else { c(DIM) };
    let btn = egui::Button::new(egui::RichText::new(label).color(text_col).strong())
        .fill(if enabled { c(ACCENT) } else { c(BG_3) })
        .rounding(Rounding::same(9.0))
        .min_size(vec2(0.0, 36.0));
    let resp = ui.add_enabled(enabled, btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn ghost_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let btn = egui::Button::new(egui::RichText::new(label).color(c(TEXT)))
        .fill(c(BG_3))
        .stroke(Stroke::new(1.0, c(LINE_2)))
        .rounding(Rounding::same(9.0))
        .min_size(vec2(0.0, 36.0));
    let resp = ui.add_enabled(enabled, btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn small_button(ui: &mut egui::Ui, label: &str, tint: Color32) -> bool {
    let btn = egui::Button::new(egui::RichText::new(label).color(tint).size(13.0))
        .fill(c(BG_3))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(8.0))
        .min_size(vec2(0.0, 30.0));
    let resp = ui.add(btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn draw_avatar(painter: &egui::Painter, app: &DolphinApp, rect: Rect) {
    painter.rect_filled(rect, Rounding::same(9.0), c(BG_3));
    let tex = app.avatar.lock().ok().and_then(|a| a.clone());
    match (tex, app.accounts.active_account()) {
        (Some(tex), Some(_)) => {
            painter.image(tex.id(), rect.shrink(3.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        (_, Some(a)) => {
            painter.text(rect.center(), Align2::CENTER_CENTER, initials(&a.username), FontId::proportional(13.0), c(TEXT));
        }
        _ => {
            painter.text(rect.center(), Align2::CENTER_CENTER, "?", FontId::proportional(13.0), c(DIM));
        }
    }
    painter.rect_stroke(rect, Rounding::same(9.0), Stroke::new(1.0, c(LINE_2)));
}

fn initials(name: &str) -> String {
    name.chars().take(2).collect::<String>().to_uppercase()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn fmt_playtime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h} h {m} min")
    } else if m > 0 {
        format!("{m} min")
    } else {
        format!("{secs} s")
    }
}

/// Human "how long ago" for the last-played stat.
fn fmt_relative(last: Option<u64>) -> String {
    let Some(at) = last else { return "—".to_string() };
    let now = config::now_unix();
    if now <= at {
        return "gerade eben".to_string();
    }
    let d = now - at;
    if d < 60 {
        "gerade eben".to_string()
    } else if d < 3600 {
        format!("vor {} min", d / 60)
    } else if d < 86_400 {
        format!("vor {} h", d / 3600)
    } else if d < 7 * 86_400 {
        let days = d / 86_400;
        if days == 1 { "gestern".to_string() } else { format!("vor {days} Tagen") }
    } else {
        format!("vor {} Wo", d / (7 * 86_400))
    }
}
