//! The launcher UI — a modern, cinematic client launcher in the spirit of
//! Lunar / Badlion.
//!
//! Layout: a **left icon sidebar** (logo, nav, account) and a large content
//! area. The Start page is an asymmetric hero — your character rendered as
//! key-art on the right, a big title and a prominent LAUNCH button on the left,
//! over a restrained single-hue wash. Everything is drawn with egui's painter
//! and custom widgets (custom sliders, a custom version dropdown), with a
//! bundled geometric typeface (Outfit + Sora) so it never reads as a stock
//! egui app. Settings save on change.

use std::f32::consts::TAU;

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontFamily, FontId, Rect, Rounding, Sense, Shape,
    Stroke, ViewportCommand, pos2, vec2,
};
use eframe::egui::epaint::{Mesh, Vertex, WHITE_UV};

use crate::app::{DolphinApp, LoginMethod, Tab};
use crate::config;
use crate::fonts;

/* ---------------------------------------------------------------- */
/*  Palette — graphite-navy ground, LIGHTER elevated panels, ocean   */
/*  accent. Panels lighter than the ground is what reads as a        */
/*  premium app instead of a flat terminal-dark window.              */
/* ---------------------------------------------------------------- */

pub const BG_0: [u8; 3] = [0x0B, 0x0D, 0x13]; // window ground
const BG_SIDE: [u8; 3] = [0x0E, 0x11, 0x18]; // sidebar
const BG_1: [u8; 3] = [0x14, 0x18, 0x22]; // elevated panel
const BG_2: [u8; 3] = [0x1A, 0x1F, 0x2B]; // card
const BG_3: [u8; 3] = [0x22, 0x28, 0x36]; // input / hover
const BG_4: [u8; 3] = [0x2C, 0x34, 0x45]; // active hover
const LINE: [u8; 3] = [0x24, 0x2B, 0x3A]; // hairline
const LINE_2: [u8; 3] = [0x35, 0x3F, 0x52]; // brighter hairline
const TEXT: [u8; 3] = [0xF3, 0xF5, 0xFA];
const DIM: [u8; 3] = [0xA6, 0xB0, 0xC1];
const FAINT: [u8; 3] = [0x66, 0x70, 0x81];
const ACCENT: [u8; 3] = [0x2F, 0xC7, 0xE0]; // ocean cyan (dolphin)
const ACCENT_2: [u8; 3] = [0x39, 0x8B, 0xF0]; // deep blue (gradient partner)
const GREEN: [u8; 3] = [0x4B, 0xD6, 0x9C];
const RED: [u8; 3] = [0xE9, 0x5C, 0x5C];
const GOLD: [u8; 3] = [0xE9, 0xB4, 0x4C];
const INK: [u8; 3] = [0x04, 0x0A, 0x0F]; // text on the accent button

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

/* ---- fonts ---- */
fn f_disp(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(fonts::DISPLAY.into()))
}
fn f_sb(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(fonts::SEMIBOLD.into()))
}
fn f_md(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(fonts::MEDIUM.into()))
}
fn f_reg(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

const SIDEBAR_W: f32 = 216.0;
const TOPBAR_H: f32 = 52.0;
const ROUND: f32 = 16.0;

/* ---------------------------------------------------------------- */
/*  Mesh helpers — gradients + soft glows                            */
/* ---------------------------------------------------------------- */

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

fn h_gradient(painter: &egui::Painter, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = Mesh::default();
    let i = mesh.vertices.len() as u32;
    for (p, col) in [
        (rect.left_top(), left),
        (rect.right_top(), right),
        (rect.right_bottom(), right),
        (rect.left_bottom(), left),
    ] {
        mesh.vertices.push(Vertex { pos: p, uv: WHITE_UV, color: col });
    }
    mesh.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    painter.add(Shape::mesh(mesh));
}

fn glow(painter: &egui::Painter, center: egui::Pos2, radius: f32, inner: Color32) {
    const SEG: usize = 48;
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
    v.selection.bg_fill = soft(ACCENT, 90);
    v.selection.stroke = Stroke::new(1.0, accent);
    v.popup_shadow = egui::epaint::Shadow {
        offset: vec2(0.0, 10.0),
        blur: 28.0,
        spread: 0.0,
        color: Color32::from_black_alpha(160),
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
    use egui::TextStyle;
    style.text_styles = [
        (TextStyle::Heading, f_disp(24.0)),
        (TextStyle::Body, f_reg(14.0)),
        (TextStyle::Button, f_sb(14.0)),
        (TextStyle::Small, f_reg(12.0)),
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
    side_bar(app, ctx);

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(c(BG_0)).inner_margin(egui::Margin::ZERO))
        .show(ctx, |ui| {
            let full = ui.max_rect();
            let t = seconds(ctx);

            // Page background depends on the tab (Home gets the cinematic wash).
            match app.tab {
                Tab::Home => home_backdrop(app, ui, full, t),
                _ => page_backdrop(ui.painter(), full, t),
            }

            // Top strip: draggable + window controls (drawn over the backdrop).
            top_strip(app, ctx, ui, full);

            // Content.
            let body = Rect::from_min_max(pos2(full.left(), full.top() + TOPBAR_H), full.right_bottom());
            let mut cui = ui.new_child(
                egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)),
            );
            match app.tab {
                Tab::Home => home_view(app, &mut cui),
                Tab::Game => scroll_page(&mut cui, |ui| game_view(app, ui)),
                Tab::Accounts => scroll_page(&mut cui, |ui| accounts_view(app, ui)),
                Tab::Settings => scroll_page(&mut cui, |ui| settings_view(app, ui)),
            }
        });
}

/// Standard scrolling page with a centered max-width column (non-Home tabs).
fn scroll_page(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(30.0);
        let w = 720.0_f32.min(ui.available_width() - 96.0);
        let pad = ((ui.available_width() - w) / 2.0).max(24.0);
        ui.horizontal(|ui| {
            ui.add_space(pad);
            ui.allocate_ui_with_layout(vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), add);
        });
        ui.add_space(40.0);
    });
}

/* ---------------------------------------------------------------- */
/*  Sidebar                                                          */
/* ---------------------------------------------------------------- */

fn side_bar(app: &mut DolphinApp, ctx: &egui::Context) {
    egui::SidePanel::left("nav")
        .exact_width(SIDEBAR_W)
        .resizable(false)
        .frame(egui::Frame::none().fill(c(BG_SIDE)))
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            let p = ui.painter().clone();
            // Subtle top-down sheen + right hairline.
            v_gradient(&p, rect, lerp_color(c(BG_SIDE), c(BG_1), 0.4), c(BG_SIDE));
            p.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0, c(LINE)));

            // Logo + wordmark.
            let logo = Rect::from_min_size(pos2(rect.left() + 22.0, rect.top() + 24.0), vec2(30.0, 30.0));
            p.image(app.logo.id(), logo, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            let r = p.text(pos2(logo.right() + 12.0, logo.center().y - 1.0), Align2::LEFT_CENTER, "Dolphin", f_disp(17.0), c(TEXT));
            p.text(pos2(r.right(), logo.center().y - 1.0), Align2::LEFT_CENTER, "Client", f_disp(17.0), c(ACCENT));

            // Nav.
            let mut y = rect.top() + 92.0;
            for (label, tab, icon) in [
                ("Start", Tab::Home, NavIcon::Home),
                ("Spiel", Tab::Game, NavIcon::Game),
                ("Konten", Tab::Accounts, NavIcon::Accounts),
                ("Einstellungen", Tab::Settings, NavIcon::Settings),
            ] {
                if nav_item(ui, &p, rect, &mut y, label, icon, app.tab == tab) {
                    app.tab = tab;
                }
            }

            // Account card pinned to the bottom.
            account_card(app, ui, &p, rect);

            // Thin activity bar under the card while busy.
            if app.busy || app.progress > 0.001 {
                let w = (SIDEBAR_W - 32.0) * app.progress.clamp(0.05, 1.0);
                let y = rect.bottom() - 84.0;
                p.rect_filled(Rect::from_min_size(pos2(rect.left() + 16.0, y), vec2(SIDEBAR_W - 32.0, 3.0)), Rounding::same(2.0), c(BG_3));
                p.rect_filled(Rect::from_min_size(pos2(rect.left() + 16.0, y), vec2(w, 3.0)), Rounding::same(2.0), c(ACCENT));
            }
        });
}

#[derive(Clone, Copy)]
enum NavIcon {
    Home,
    Game,
    Accounts,
    Settings,
}

fn nav_item(
    ui: &mut egui::Ui,
    p: &egui::Painter,
    area: Rect,
    y: &mut f32,
    label: &str,
    icon: NavIcon,
    active: bool,
) -> bool {
    let h = 46.0;
    let rect = Rect::from_min_size(pos2(area.left() + 12.0, *y), vec2(SIDEBAR_W - 24.0, h));
    *y += h + 6.0;
    let resp = ui.interact(rect, ui.id().with(("nav", label)), Sense::click());
    let hov = resp.hovered();
    if active {
        p.rect_filled(rect, Rounding::same(12.0), soft(ACCENT, 26));
        // Accent marker on the left.
        p.rect_filled(Rect::from_min_size(pos2(rect.left() - 4.0, rect.center().y - 9.0), vec2(4.0, 18.0)), Rounding::same(2.0), c(ACCENT));
    } else if hov {
        p.rect_filled(rect, Rounding::same(12.0), soft(LINE_2, 40));
    }
    let col = if active { c(ACCENT) } else if hov { c(TEXT) } else { c(DIM) };
    let icx = rect.left() + 20.0;
    let icy = rect.center().y;
    draw_nav_icon(p, icon, pos2(icx, icy), col);
    p.text(pos2(rect.left() + 44.0, icy), Align2::LEFT_CENTER, label, f_sb(14.5), col);
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Minimal stroke icons drawn by hand so nav doesn't rely on a symbol font.
fn draw_nav_icon(p: &egui::Painter, icon: NavIcon, ctr: egui::Pos2, col: Color32) {
    let s = Stroke::new(1.7, col);
    match icon {
        NavIcon::Home => {
            let x = ctr.x;
            let y = ctr.y;
            // Roof.
            p.add(Shape::line(vec![pos2(x - 8.0, y + 1.0), pos2(x, y - 8.0), pos2(x + 8.0, y + 1.0)], s));
            // Body.
            p.rect_stroke(Rect::from_min_max(pos2(x - 6.0, y + 1.0), pos2(x + 6.0, y + 8.0)), Rounding::same(1.5), s);
        }
        NavIcon::Game => {
            // Three sliders.
            for (i, kx) in [(-6.0_f32, 3.0_f32), (0.0, -3.0), (6.0, 4.0)].iter().enumerate() {
                let (dy, knob) = (kx.0, kx.1);
                let yy = ctr.y + dy;
                p.line_segment([pos2(ctr.x - 8.0, yy), pos2(ctr.x + 8.0, yy)], Stroke::new(1.6, col));
                p.circle_filled(pos2(ctr.x + knob, yy), 2.4, col);
                let _ = i;
            }
        }
        NavIcon::Accounts => {
            p.circle_stroke(pos2(ctr.x, ctr.y - 3.0), 3.6, s);
            // Shoulders arc approximated with a stroked half-rounded rect.
            let b = Rect::from_min_max(pos2(ctr.x - 7.0, ctr.y + 2.0), pos2(ctr.x + 7.0, ctr.y + 11.0));
            p.add(Shape::line(
                vec![pos2(b.left(), b.bottom()), pos2(b.left(), b.top() + 2.0), pos2(b.right(), b.top() + 2.0), pos2(b.right(), b.bottom())],
                s,
            ));
        }
        NavIcon::Settings => {
            p.circle_stroke(ctr, 4.0, s);
            for k in 0..8 {
                let a = k as f32 / 8.0 * TAU;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([ctr + d * 6.0, ctr + d * 8.5], Stroke::new(1.6, col));
            }
        }
    }
}

fn account_card(app: &mut DolphinApp, ui: &mut egui::Ui, p: &egui::Painter, area: Rect) {
    let h = 62.0;
    let rect = Rect::from_min_size(pos2(area.left() + 12.0, area.bottom() - h - 16.0), vec2(SIDEBAR_W - 24.0, h));
    let resp = ui.interact(rect, ui.id().with("acctcard"), Sense::click());
    let hov = resp.hovered();
    p.rect_filled(rect, Rounding::same(13.0), if hov { c(BG_3) } else { c(BG_2) });
    p.rect_stroke(rect, Rounding::same(13.0), Stroke::new(1.0, c(LINE)));
    let av = Rect::from_min_size(pos2(rect.left() + 10.0, rect.center().y - 17.0), vec2(34.0, 34.0));
    draw_avatar(p, app, av);
    let active = app.accounts.active_account().cloned();
    let name = active.as_ref().map(|a| a.username.clone()).unwrap_or_else(|| "Kein Konto".to_string());
    let (state, scol) = if app.relogin_for.is_some() {
        ("Anmeldung nötig", c(RED))
    } else if running(app) {
        ("im Spiel", c(GREEN))
    } else if active.is_some() {
        ("bereit", c(DIM))
    } else {
        ("anmelden", c(FAINT))
    };
    p.text(pos2(av.right() + 10.0, rect.center().y - 8.0), Align2::LEFT_CENTER, truncate(&name, 13), f_sb(13.5), c(TEXT));
    p.text(pos2(av.right() + 10.0, rect.center().y + 8.0), Align2::LEFT_CENTER, state, f_md(11.0), scol);
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.clicked() {
        app.tab = Tab::Accounts;
    }
}

/* ---------------------------------------------------------------- */
/*  Top strip — window drag + controls                               */
/* ---------------------------------------------------------------- */

fn top_strip(app: &DolphinApp, ctx: &egui::Context, ui: &mut egui::Ui, full: Rect) {
    let bar = Rect::from_min_max(full.left_top(), pos2(full.right(), full.top() + TOPBAR_H));
    let p = ui.painter().clone();

    // Page title (left).
    let title = match app.tab {
        Tab::Home => "",
        Tab::Game => "Spiel",
        Tab::Accounts => "Konten",
        Tab::Settings => "Einstellungen",
    };
    if !title.is_empty() {
        p.text(pos2(bar.left() + 30.0, bar.center().y), Align2::LEFT_CENTER, title, f_disp(18.0), c(TEXT));
    }

    // Window buttons (right → left).
    let mut x = bar.right() - 10.0;
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

    let drag = Rect::from_min_max(bar.left_top(), pos2(x - 6.0, bar.bottom()));
    let resp = ui.interact(drag, ui.id().with("wdrag"), Sense::click_and_drag());
    if resp.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!is_max));
    }
}

enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

fn window_button(ui: &mut egui::Ui, x: &mut f32, bar: Rect, kind: WindowButton) -> bool {
    let size = vec2(32.0, 28.0);
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
    painter.rect_filled(rect, Rounding::same(8.0), fill);
    let ctr = rect.center();
    let col = if hovered { Color32::WHITE } else { c(DIM) };
    let s = Stroke::new(1.5, col);
    match kind {
        WindowButton::Minimize => {
            painter.line_segment([ctr + vec2(-5.0, 2.0), ctr + vec2(5.0, 2.0)], s);
        }
        WindowButton::Maximize => {
            painter.rect_stroke(Rect::from_center_size(ctr, vec2(9.0, 9.0)), Rounding::same(1.5), s);
        }
        WindowButton::Close => {
            painter.line_segment([ctr + vec2(-4.5, -4.5), ctr + vec2(4.5, 4.5)], s);
            painter.line_segment([ctr + vec2(-4.5, 4.5), ctr + vec2(4.5, -4.5)], s);
        }
    }
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/* ---------------------------------------------------------------- */
/*  Backdrops                                                        */
/* ---------------------------------------------------------------- */

/// Quiet backdrop for the non-Home tabs: a single soft top glow, no orbs.
fn page_backdrop(painter: &egui::Painter, rect: Rect, t: f32) {
    let breathe = 0.5 + 0.5 * (t * 0.4).sin();
    glow(painter, pos2(rect.left() + rect.width() * 0.7, rect.top() - 40.0), 520.0, soft(ACCENT_2, (14.0 + breathe * 8.0) as u8));
}

/// Cinematic Home backdrop: a horizontal wash from a deep accent-tinted left to
/// the ground, a large soft glow behind the character, and the character
/// key-art bleeding off the right edge.
fn home_backdrop(app: &DolphinApp, ui: &mut egui::Ui, rect: Rect, t: f32) {
    let p = ui.painter().clone();
    // Base wash.
    h_gradient(&p, rect, lerp_color(c(BG_0), c(ACCENT_2), 0.10), c(BG_0));
    v_gradient(&p, rect, soft(BG_0, 0), soft(BG_0, 150));

    // Character key-art on the right (only when signed in).
    if app.accounts.active_account().is_some() {
        if let Some(tex) = app.body.lock().ok().and_then(|b| b.clone()) {
            let [tw, th] = tex.size();
            if tw > 0 && th > 0 {
                let target_h = rect.height() * 0.82;
                let target_w = target_h * tw as f32 / th as f32;
                let cx = rect.left() + rect.width() * 0.72;
                let bob = (t * 1.0).sin() * 5.0;
                let top = rect.bottom() - target_h - 30.0 + bob;
                // Glow behind the character.
                glow(&p, pos2(cx, rect.center().y + 30.0), target_h * 0.62, soft(ACCENT, 46));
                p.image(
                    tex.id(),
                    Rect::from_min_size(pos2(cx - target_w / 2.0, top), vec2(target_w, target_h)),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
    } else {
        // Ambient glow for the signed-out state.
        glow(&p, pos2(rect.left() + rect.width() * 0.7, rect.center().y), 440.0, soft(ACCENT, 40));
    }
}

/* ---------------------------------------------------------------- */
/*  Home — asymmetric hero                                           */
/* ---------------------------------------------------------------- */

fn home_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let area = ui.max_rect();
    let active = app.accounts.active_account().cloned();
    let is_running = running(app);
    let t = seconds(&ctx);

    // Floating notices (updates / relogin / device-login) top-right.
    notices(app, ui, area);

    match active {
        None => signed_out_hero(app, ui, area),
        Some(acc) => {
            let left = area.left() + 44.0;
            let p = ui.painter().clone();

            // Eyebrow.
            let mut y = area.top() + 40.0;
            p.text(pos2(left, y), Align2::LEFT_TOP, "WILLKOMMEN ZURÜCK", f_sb(12.5), c(ACCENT));
            y += 34.0;

            // Big name.
            p.text(pos2(left, y), Align2::LEFT_TOP, truncate(&acc.username, 16), f_disp(52.0), c(TEXT));
            y += 70.0;

            // Subline.
            let ver = if app.settings.client_version.is_empty() { "Neueste".to_string() } else { app.settings.client_version.clone() };
            p.text(pos2(left, y), Align2::LEFT_TOP, &format!("DolphinClient {ver}   ·   Minecraft {}", config::TARGET_VERSION), f_md(14.5), c(DIM));
            y += 40.0;

            // Stat strip.
            let stats = app.stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
            stat_strip(&p, pos2(left, y), &stats);
            y += 78.0;

            // LAUNCH row (button + version pill) — placed with real widgets.
            let mut row = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_max(pos2(left, y), pos2(left + 560.0, y + 66.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            if launch_button(&mut row, is_running, app.busy, t) && !app.busy && !is_running {
                app.start_launch(&ctx);
            }
            row.add_space(14.0);
            version_dropdown(app, &mut row);
            y += 66.0 + 16.0;

            // Status line.
            if !app.status.is_empty() {
                let scol = if app.status.starts_with("Fehler") || app.relogin_for.is_some() {
                    c(RED)
                } else if is_running {
                    c(GREEN)
                } else {
                    c(DIM)
                };
                ui.painter().text(pos2(left, y), Align2::LEFT_TOP, truncate(&app.status, 70), f_md(12.5), scol);
                y += 26.0;
            }

            // Quick-join servers (chips) along the bottom-left.
            if !app.settings.servers.is_empty() {
                let sy = (area.bottom() - 96.0).max(y + 12.0);
                ui.painter().text(pos2(left, sy - 22.0), Align2::LEFT_BOTTOM, "SCHNELL BEITRETEN", f_sb(11.5), c(FAINT));
                let servers = app.settings.servers.clone();
                let mut sx = left;
                let mut join: Option<String> = None;
                for s in servers.iter().take(4) {
                    if server_chip(ui, &mut sx, sy, &s.name, &s.address) && !app.busy && !is_running {
                        join = Some(s.address.clone());
                    }
                }
                if let Some(addr) = join {
                    app.launch_server(&ctx, addr);
                }
            }
        }
    }
}

fn signed_out_hero(app: &mut DolphinApp, ui: &mut egui::Ui, area: Rect) {
    let ctx = ui.ctx().clone();
    let left = area.left() + 44.0;
    let p = ui.painter().clone();
    let mut y = area.top() + 90.0;
    p.text(pos2(left, y), Align2::LEFT_TOP, "DEIN MINECRAFT, AUFGEWERTET", f_sb(12.5), c(ACCENT));
    y += 34.0;
    p.text(pos2(left, y), Align2::LEFT_TOP, "Willkommen.", f_disp(54.0), c(TEXT));
    y += 76.0;
    p.text(pos2(left, y), Align2::LEFT_TOP, "Melde dich mit Microsoft an oder übernimm ein", f_md(15.0), c(DIM));
    y += 24.0;
    p.text(pos2(left, y), Align2::LEFT_TOP, "bestehendes Konto von diesem PC.", f_md(15.0), c(DIM));
    y += 44.0;

    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(pos2(left, y), pos2(left + 600.0, y + 52.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    if primary_button(&mut row, "Mit Microsoft anmelden", !app.busy) {
        app.add_microsoft(&ctx);
    }
    row.add_space(10.0);
    if ghost_button(&mut row, "Konto übernehmen", true) {
        app.import_accounts();
    }
    if crate::tokens::has_token() {
        row.add_space(10.0);
        if ghost_button(&mut row, "Vorheriges Konto", true) {
            app.start_login(&ctx, LoginMethod::Refresh);
        }
    }
    if let Some(note) = &app.import_note {
        ui.painter().text(pos2(left, y + 66.0), Align2::LEFT_TOP, note, f_md(13.0), c(GREEN));
    }
    login_prompts_floating(app, ui, area);
}

/// Inline stat strip: "value label   ·   value label …" drawn by the painter.
fn stat_strip(p: &egui::Painter, at: egui::Pos2, stats: &config::Stats) {
    let items = [
        (fmt_playtime(stats.playtime_secs), "Spielzeit"),
        (stats.launches.to_string(), "Starts"),
        (fmt_relative(stats.last_played), "Zuletzt"),
    ];
    let mut x = at.x;
    for (i, (value, label)) in items.iter().enumerate() {
        if i > 0 {
            p.text(pos2(x, at.y + 14.0), Align2::LEFT_CENTER, "·", f_md(18.0), c(FAINT));
            x += 26.0;
        }
        let r = p.text(pos2(x, at.y + 6.0), Align2::LEFT_TOP, value, f_disp(20.0), c(TEXT));
        let r2 = p.text(pos2(x, at.y + 34.0), Align2::LEFT_TOP, label, f_md(11.5), c(DIM));
        x = r.right().max(r2.right()) + 26.0;
    }
}

/// The big LAUNCH button — gradient, breathing aura, glass sheen.
fn launch_button(ui: &mut egui::Ui, is_running: bool, busy: bool, t: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(268.0, 62.0), Sense::click());
    let enabled = !busy && !is_running;
    let hov = enabled && resp.hovered();
    let painter = ui.painter();
    let rounding = Rounding::same(15.0);
    if enabled {
        let pulse = 0.5 + 0.5 * (t * 1.5).sin();
        let aura = (40.0 + pulse * 26.0 + if hov { 40.0 } else { 0.0 }) as u8;
        glow(painter, rect.center() + vec2(0.0, 4.0), rect.width() * 0.6, soft(ACCENT, aura));
        let (l, r) = if hov {
            (lerp_color(c(ACCENT), Color32::WHITE, 0.12), lerp_color(c(ACCENT_2), Color32::WHITE, 0.06))
        } else {
            (c(ACCENT), c(ACCENT_2))
        };
        painter.rect_filled(rect, rounding, r);
        h_gradient(painter, rect, l, r);
        let sheen = Rect::from_min_max(rect.left_top(), pos2(rect.right(), rect.center().y + 2.0));
        v_gradient(painter, sheen, soft([0xFF, 0xFF, 0xFF], 42), Color32::TRANSPARENT);
    } else {
        painter.rect_filled(rect, rounding, c(BG_3));
        painter.rect_stroke(rect, rounding, Stroke::new(1.0, c(LINE)));
    }
    let (label, tri) = if is_running { ("IM SPIEL", false) } else if busy { ("…", false) } else { ("SPIELEN", true) };
    let fg = if enabled { c(INK) } else { c(DIM) };
    let mut tx = rect.center().x;
    if tri {
        tx += 14.0;
        let cy = rect.center().y;
        let lx = rect.center().x - 62.0;
        painter.add(Shape::convex_polygon(vec![pos2(lx, cy - 9.0), pos2(lx + 15.0, cy), pos2(lx, cy + 9.0)], fg, Stroke::NONE));
    }
    painter.text(pos2(tx, rect.center().y), Align2::CENTER_CENTER, label, f_disp(19.0), fg);
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked() && enabled
}

/// Custom version dropdown pill (no eguiish ComboBox).
fn version_dropdown(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let versions = app.versions.lock().map(|v| v.clone()).unwrap_or_default();
    let sel = app.settings.client_version.clone();
    let label = if sel.is_empty() { "Neueste".to_string() } else { sel.clone() };
    let (rect, resp) = ui.allocate_exact_size(vec2(158.0, 50.0), Sense::click());
    let hov = resp.hovered();
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(13.0), if hov { c(BG_3) } else { c(BG_2) });
    p.rect_stroke(rect, Rounding::same(13.0), Stroke::new(1.0, if hov { c(LINE_2) } else { c(LINE) }));
    p.text(pos2(rect.left() + 14.0, rect.center().y - 8.0), Align2::LEFT_CENTER, "VERSION", f_sb(9.5), c(FAINT));
    p.text(pos2(rect.left() + 14.0, rect.center().y + 8.0), Align2::LEFT_CENTER, truncate(&label, 14), f_sb(13.5), c(TEXT));
    // Chevron.
    let cx = rect.right() - 16.0;
    let cy = rect.center().y;
    p.add(Shape::line(vec![pos2(cx - 4.0, cy - 2.0), pos2(cx, cy + 2.5), pos2(cx + 4.0, cy - 2.0)], Stroke::new(1.6, c(DIM))));
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let popup_id = ui.id().with("verpop");
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    let mut pick: Option<String> = None;
    egui::popup_below_widget(ui, popup_id, &resp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(rect.width());
        ui.spacing_mut().item_spacing.y = 2.0;
        if dropdown_row(ui, "Neueste (empfohlen)", sel.is_empty()) {
            pick = Some(String::new());
        }
        for v in &versions {
            if dropdown_row(ui, v, &sel == v) {
                pick = Some(v.clone());
            }
        }
    });
    if let Some(v) = pick {
        app.settings.client_version = v;
        app.settings.save();
        ui.memory_mut(|m| m.close_popup());
    }
}

fn dropdown_row(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(150.0), 30.0), Sense::click());
    let hov = resp.hovered();
    let p = ui.painter();
    if hov || selected {
        p.rect_filled(rect, Rounding::same(7.0), if selected { soft(ACCENT, 28) } else { soft(LINE_2, 40) });
    }
    let col = if selected { c(ACCENT) } else { c(TEXT) };
    p.text(pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, label, f_md(13.0), col);
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn server_chip(ui: &mut egui::Ui, x: &mut f32, y_bottom: f32, name: &str, addr: &str) -> bool {
    let label = truncate(name, 18);
    let galley = ui.painter().layout_no_wrap(label.clone(), f_sb(13.0), c(TEXT));
    let w = (galley.size().x + 46.0).clamp(120.0, 220.0);
    let h = 46.0;
    let rect = Rect::from_min_size(pos2(*x, y_bottom - h), vec2(w, h));
    *x += w + 10.0;
    let resp = ui.interact(rect, ui.id().with(("srv", name, addr)), Sense::click());
    let hov = resp.hovered();
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(12.0), if hov { c(BG_3) } else { c(BG_2) });
    p.rect_stroke(rect, Rounding::same(12.0), Stroke::new(1.0, if hov { c(ACCENT) } else { c(LINE) }));
    let cy = rect.center().y;
    let lx = rect.left() + 15.0;
    p.add(Shape::convex_polygon(vec![pos2(lx, cy - 6.0), pos2(lx + 9.0, cy), pos2(lx, cy + 6.0)], c(ACCENT), Stroke::NONE));
    p.text(pos2(lx + 16.0, cy - 7.0), Align2::LEFT_CENTER, label, f_sb(13.0), c(TEXT));
    p.text(pos2(lx + 16.0, cy + 8.0), Align2::LEFT_CENTER, truncate(addr, 22), f_md(10.5), c(DIM));
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/* ---------------------------------------------------------------- */
/*  Notices (floating cards, top-right of the Home hero)             */
/* ---------------------------------------------------------------- */

fn notices(app: &mut DolphinApp, ui: &mut egui::Ui, area: Rect) {
    let ctx = ui.ctx().clone();
    // Positioned as a right-aligned column via a child UI.
    let col_w = 320.0;
    let x = area.right() - col_w - 28.0;
    if x < area.left() + 380.0 {
        return; // window too narrow — skip (rare)
    }
    let mut cui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(pos2(x, area.top() + 20.0), pos2(x + col_w, area.bottom() - 20.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    if let Some(name) = app.relogin_for.clone() {
        card_tinted(&mut cui, RED, |ui| {
            ui.label(rt(&format!("Anmeldung für {name} nötig"), f_sb(14.0), c(TEXT)));
            ui.add_space(2.0);
            ui.label(rt("Sitzung nicht übernehmbar. Bitte neu anmelden.", f_md(12.0), c(DIM)));
            ui.add_space(10.0);
            if primary_button(ui, "Neu anmelden", !app.busy) {
                app.add_microsoft(&ctx);
            }
        });
        cui.add_space(12.0);
    }
    if let Some(info) = app.update_note.lock().ok().and_then(|n| n.clone()) {
        card_tinted(&mut cui, GOLD, |ui| {
            ui.label(rt(&format!("Update {} verfügbar", info.version), f_sb(14.0), c(TEXT)));
            ui.add_space(10.0);
            if primary_button(ui, "Jetzt aktualisieren", !app.busy) {
                app.start_self_update(&ctx, info.clone());
            }
        });
        cui.add_space(12.0);
    }
    login_prompts_in(app, &mut cui);
}

/* ---------------------------------------------------------------- */
/*  Spiel — quick-settings (custom sliders) + servers                */
/* ---------------------------------------------------------------- */

fn game_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    ui.label(rt("Grafik- und Leistungseinstellungen — sie greifen beim nächsten Spielstart.", f_md(14.0), c(DIM)));
    ui.add_space(16.0);

    let is_running = running(app);
    if is_running {
        card_tinted(ui, GOLD, |ui| {
            ui.label(rt("Das Spiel läuft gerade — Änderungen sind erst nach dem Beenden möglich.", f_md(12.5), c(TEXT)));
        });
        ui.add_space(12.0);
    }

    ui.add_enabled_ui(!is_running, |ui| {
        card(ui, |ui| {
            section_title(ui, "Leistung");
            let mut rd = app.gameopts.render_distance();
            if slider_row(ui, "Sichtweite", &format!("{rd} Chunks"), &mut rd, 2, 32) {
                app.gameopts.set_render_distance(rd);
                app.gameopts.save();
            }
            let mut fps = app.gameopts.max_fps() as i32;
            let fl = if fps == 0 { "Unbegrenzt".to_string() } else { format!("{fps} FPS") };
            if slider_row_step(ui, "Bildrate-Limit", &fl, &mut fps, 0, 360, 10) {
                app.gameopts.set_max_fps(fps as u32);
                app.gameopts.save();
            }
            let mut vsync = app.gameopts.vsync();
            if toggle_row(ui, "VSync", "Bildrate an die Bildwiederholrate des Monitors koppeln.", &mut vsync) {
                app.gameopts.set_vsync(vsync);
                app.gameopts.save();
            }
        });
        ui.add_space(14.0);

        card(ui, |ui| {
            section_title(ui, "Anzeige");
            let mut fovf = app.gameopts.fov();
            let mut fov = fovf.round() as i32;
            if slider_row(ui, "Sichtfeld (FoV)", &format!("{fov}°"), &mut fov, 30, 110) {
                fovf = fov as f32;
                app.gameopts.set_fov(fovf);
                app.gameopts.save();
            }
            let mut brp = (app.gameopts.brightness() * 100.0).round() as i32;
            if slider_row(ui, "Helligkeit", &format!("{brp}%"), &mut brp, 0, 100) {
                app.gameopts.set_brightness(brp as f32 / 100.0);
                app.gameopts.save();
            }
            row_head(ui, "GUI-Größe", "0 = automatisch an die Fenstergröße anpassen.");
            let gs = app.gameopts.gui_scale();
            if let Some(sel) = segmented(ui, &["Auto", "1", "2", "3", "4"], gs as usize) {
                app.gameopts.set_gui_scale(sel as u32);
                app.gameopts.save();
            }
            ui.add_space(8.0);
            row_head(ui, "Grafik", "Fancy sieht besser aus, Fast bringt mehr FPS.");
            let cur = if app.gameopts.graphics() == crate::gameopts::Graphics::Fast { 0 } else { 1 };
            if let Some(sel) = segmented(ui, &["Fast", "Fancy"], cur) {
                let ng = if sel == 0 { crate::gameopts::Graphics::Fast } else { crate::gameopts::Graphics::Fancy };
                app.gameopts.set_graphics(ng);
                app.gameopts.save();
            }
            ui.add_space(6.0);
            if toggle_row(ui, "Vollbild starten", "Das Spiel direkt im Vollbild öffnen.", &mut app.settings.fullscreen) {
                app.settings.save();
                app.gameopts.set_fullscreen(app.settings.fullscreen);
                app.gameopts.save();
            }
        });
    });
    ui.add_space(14.0);

    // Playtime history.
    let stats = app.stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
    if !stats.sessions.is_empty() {
        playtime_card(ui, &stats);
        ui.add_space(14.0);
    }

    servers_card(app, ui);
}

fn servers_card(app: &mut DolphinApp, ui: &mut egui::Ui) {
    card(ui, |ui| {
        section_title(ui, "Server");
        ui.label(rt("Speichere deine Lieblingsserver für den Ein-Klick-Beitritt auf der Startseite.", f_md(12.0), c(DIM)));
        ui.add_space(12.0);
        let servers = app.settings.servers.clone();
        let default_addr = app.settings.server.clone();
        let mut remove: Option<usize> = None;
        let mut make_default: Option<String> = None;
        for (i, s) in servers.iter().enumerate() {
            let is_default = !default_addr.is_empty() && default_addr == s.address;
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(rect, Rounding::same(11.0), c(BG_3));
            p.rect_stroke(rect, Rounding::same(11.0), Stroke::new(1.0, if is_default { c(ACCENT) } else { c(LINE) }));
            p.text(pos2(rect.left() + 14.0, rect.center().y - 8.0), Align2::LEFT_CENTER, truncate(&s.name, 30), f_sb(14.0), c(TEXT));
            p.text(pos2(rect.left() + 14.0, rect.center().y + 9.0), Align2::LEFT_CENTER, truncate(&s.address, 40), f_md(11.5), c(DIM));
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
                bx.label(rt("Standard", f_sb(12.5), c(ACCENT)));
            } else if small_button(&mut bx, "Als Standard", c(ACCENT)) {
                make_default = Some(s.address.clone());
            }
            ui.add_space(8.0);
        }
        if let Some(i) = remove {
            app.remove_server(i);
        }
        if let Some(a) = make_default {
            app.set_default_server(&a);
        }
        if !servers.is_empty() {
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.new_server_name).hint_text("Name (optional)").desired_width(150.0));
            ui.add(egui::TextEdit::singleline(&mut app.new_server_addr).hint_text("Adresse, z. B. play.example.net").desired_width(f32::INFINITY));
        });
        ui.add_space(10.0);
        if primary_button(ui, "Server hinzufügen", !app.new_server_addr.trim().is_empty()) {
            app.add_server();
        }
    });
}

/* ---------------------------------------------------------------- */
/*  Accounts                                                         */
/* ---------------------------------------------------------------- */

fn accounts_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    ui.label(rt("Mehrere Microsoft-Konten verwalten oder ein bestehendes Konto von diesem PC übernehmen.", f_md(14.0), c(DIM)));
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        if primary_button(ui, "Microsoft-Konto hinzufügen", !app.busy) {
            app.add_microsoft(&ctx);
        }
        if ghost_button(ui, "Konto übernehmen", true) {
            app.import_accounts();
        }
    });
    if let Some(note) = &app.import_note {
        ui.add_space(8.0);
        ui.label(rt(note, f_md(13.0), c(GREEN)));
    }
    ui.add_space(14.0);
    login_prompts_in(app, ui);

    let accounts = app.accounts.accounts.clone();
    if accounts.is_empty() {
        card(ui, |ui| {
            ui.label(rt("Noch keine Konten. Füge eines hinzu oder übernimm ein bestehendes.", f_md(13.5), c(DIM)));
        });
    }
    let mut switch_to: Option<String> = None;
    let mut remove: Option<String> = None;
    for a in &accounts {
        let is_active = app.accounts.is_active(&a.uuid);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(rect, Rounding::same(ROUND), c(BG_2));
        p.rect_stroke(rect, Rounding::same(ROUND), if is_active { Stroke::new(1.5, c(ACCENT)) } else { Stroke::new(1.0, c(LINE)) });
        let av = Rect::from_min_size(pos2(rect.left() + 15.0, rect.center().y - 19.0), vec2(38.0, 38.0));
        let tex = if is_active { app.avatar.lock().ok().and_then(|a| a.clone()) } else { None };
        p.rect_filled(av, Rounding::same(10.0), c(BG_3));
        if let Some(tex) = tex {
            p.image(tex.id(), av.shrink(3.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            p.text(av.center(), Align2::CENTER_CENTER, initials(&a.username), f_sb(15.0), c(TEXT));
        }
        p.rect_stroke(av, Rounding::same(10.0), Stroke::new(1.0, c(LINE_2)));
        p.text(pos2(rect.left() + 66.0, rect.center().y - 8.0), Align2::LEFT_CENTER, &a.username, f_sb(15.0), c(TEXT));
        p.text(pos2(rect.left() + 66.0, rect.center().y + 9.0), Align2::LEFT_CENTER, truncate(&a.source, 30), f_md(11.0), c(DIM));
        let mut bx = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.right() - 220.0, rect.top()), rect.right_bottom()))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        bx.add_space(15.0);
        if small_button(&mut bx, "Entfernen", c(RED)) {
            remove = Some(a.uuid.clone());
        }
        if is_active {
            bx.label(rt("● Aktiv", f_sb(13.0), c(ACCENT)));
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
    ui.label(rt("Änderungen werden sofort gespeichert.", f_md(14.0), c(DIM)));
    ui.add_space(16.0);
    card(ui, |ui| {
        section_title(ui, "Start & Updates");
        if toggle_row(ui, "Mit dem System starten", "DolphinClient öffnet sich automatisch bei der Anmeldung am PC.", &mut app.settings.autostart) {
            app.settings.save();
            let _ = crate::autostart::set(app.settings.autostart);
        }
        toggle_row(ui, "Launcher nach Start schließen", "Das Fenster schließen, sobald das Spiel läuft.", &mut app.settings.close_on_launch).then(|| app.settings.save());
        toggle_row(ui, "Nach Updates suchen", "Beim Start prüfen, ob eine neuere Version bereitsteht.", &mut app.settings.auto_update).then(|| app.settings.save());
        toggle_row(ui, "Updates automatisch installieren", "Gefundene Updates ohne Nachfrage einspielen (Neustart).", &mut app.settings.auto_update_apply).then(|| app.settings.save());
    });
    ui.add_space(14.0);
    card(ui, |ui| {
        section_title(ui, "Discord");
        toggle_row(ui, "Rich Presence", "Zeigt in deinem Discord-Profil, dass du DolphinClient offen hast.", &mut app.settings.discord_rpc).then(|| app.settings.save());
    });
    ui.add_space(16.0);
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
        ui.add_space(12.0);
        log_box(app, ui);
    }
}

/* ---------------------------------------------------------------- */
/*  Login prompts                                                    */
/* ---------------------------------------------------------------- */

fn login_prompts_in(app: &mut DolphinApp, ui: &mut egui::Ui) {
    if let Some((link, code)) = app.device.clone() {
        card(ui, |ui| {
            ui.label(rt("Anmeldung im Browser", f_sb(14.0), c(TEXT)));
            ui.label(rt("Ein Browser-Fenster wurde geöffnet — dort anmelden.", f_md(12.5), c(DIM)));
            ui.add_space(6.0);
            ui.label(rt(&code, f_disp(24.0), c(ACCENT)));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ghost_button(ui, "Link öffnen", true) {
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
            ui.label(rt("Ein Browser-Fenster wurde geöffnet — dort anmelden.", f_md(12.5), c(DIM)));
            ui.add_space(8.0);
            if ghost_button(ui, "Browser erneut öffnen", true) {
                let _ = open::that(&url);
            }
        });
        ui.add_space(12.0);
    }
}

/// Signed-out variant: render prompts in a right-hand floating column.
fn login_prompts_floating(app: &mut DolphinApp, ui: &mut egui::Ui, area: Rect) {
    if app.device.is_none() && app.auth_url.is_none() {
        return;
    }
    let col_w = 320.0;
    let x = area.right() - col_w - 28.0;
    if x < area.left() + 380.0 {
        return;
    }
    let mut cui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(pos2(x, area.top() + 20.0), pos2(x + col_w, area.bottom() - 20.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    login_prompts_in(app, &mut cui);
}

/* ---------------------------------------------------------------- */
/*  Custom widgets                                                   */
/* ---------------------------------------------------------------- */

fn slider_row(ui: &mut egui::Ui, title: &str, value: &str, val: &mut i32, min: i32, max: i32) -> bool {
    slider_row_step(ui, title, value, val, min, max, 1)
}

fn slider_row_step(ui: &mut egui::Ui, title: &str, value: &str, val: &mut i32, min: i32, max: i32, step: i32) -> bool {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.label(rt(title, f_sb(14.0), c(TEXT)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(rt(value, f_sb(13.0), c(ACCENT)));
        });
    });
    ui.add_space(8.0);
    let changed = slider(ui, val, min, max, step);
    ui.add_space(2.0);
    changed
}

/// A custom horizontal slider (track + accent fill + white knob).
fn slider(ui: &mut egui::Ui, val: &mut i32, min: i32, max: i32, step: i32) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click_and_drag());
    let cy = rect.center().y;
    let x0 = rect.left() + 10.0;
    let x1 = rect.right() - 10.0;
    let span = (x1 - x0).max(1.0);
    let mut changed = false;
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            let tt = ((pos.x - x0) / span).clamp(0.0, 1.0);
            let raw = min as f32 + tt * (max - min) as f32;
            let stepped = (raw / step as f32).round() as i32 * step;
            let nv = stepped.clamp(min, max);
            if nv != *val {
                *val = nv;
                changed = true;
            }
        }
    }
    let t = ((*val - min) as f32 / (max - min).max(1) as f32).clamp(0.0, 1.0);
    let kx = x0 + span * t;
    let p = ui.painter();
    // Track.
    p.rect_filled(Rect::from_min_max(pos2(x0, cy - 3.0), pos2(x1, cy + 3.0)), Rounding::same(3.0), c(BG_4));
    // Fill (gradient).
    if kx > x0 + 1.0 {
        h_gradient(p, Rect::from_min_max(pos2(x0, cy - 3.0), pos2(kx, cy + 3.0)), c(ACCENT_2), c(ACCENT));
    }
    // Knob.
    let hov = resp.hovered() || resp.dragged();
    if hov {
        glow(p, pos2(kx, cy), 16.0, soft(ACCENT, 90));
    }
    p.circle_filled(pos2(kx, cy), if hov { 9.5 } else { 8.5 }, Color32::WHITE);
    p.circle_stroke(pos2(kx, cy), if hov { 9.5 } else { 8.5 }, Stroke::new(1.5, c(ACCENT)));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    changed
}

fn segmented(ui: &mut egui::Ui, options: &[&str], selected: usize) -> Option<usize> {
    let h = 38.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, Rounding::same(11.0), c(BG_3));
    p.rect_stroke(rect, Rounding::same(11.0), Stroke::new(1.0, c(LINE)));
    let n = options.len().max(1);
    let seg_w = rect.width() / n as f32;
    let mut chosen = None;
    for (i, label) in options.iter().enumerate() {
        let sr = Rect::from_min_size(pos2(rect.left() + seg_w * i as f32, rect.top()), vec2(seg_w, h));
        let resp = ui.interact(sr, ui.id().with(("seg", label, i)), Sense::click());
        let is_sel = i == selected;
        if is_sel {
            p.rect_filled(sr.shrink(4.0), Rounding::same(8.0), soft(ACCENT, 40));
            p.rect_stroke(sr.shrink(4.0), Rounding::same(8.0), Stroke::new(1.0, c(ACCENT)));
        } else if resp.hovered() {
            p.rect_filled(sr.shrink(4.0), Rounding::same(8.0), soft(LINE_2, 40));
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let col = if is_sel { c(TEXT) } else { c(DIM) };
        p.text(sr.center(), Align2::CENTER_CENTER, *label, f_sb(13.5), col);
        if resp.clicked() && !is_sel {
            chosen = Some(i);
        }
    }
    chosen
}

fn toggle_row(ui: &mut egui::Ui, title: &str, sub: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(rt(title, f_sb(14.0), c(TEXT)));
            ui.label(rt(sub, f_md(12.0), c(DIM)));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = toggle(ui, on);
        });
    });
    ui.add_space(4.0);
    changed
}

fn toggle(ui: &mut egui::Ui, on: &mut bool) -> bool {
    let size = vec2(46.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let mut changed = false;
    if resp.clicked() {
        *on = !*on;
        changed = true;
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let radius = rect.height() / 2.0;
    let bg = lerp_color(c(BG_4), c(ACCENT), t);
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

/* ---------------------------------------------------------------- */
/*  Shared bits                                                      */
/* ---------------------------------------------------------------- */

fn rt(text: &str, font: FontId, col: Color32) -> egui::RichText {
    egui::RichText::new(text).font(font).color(col)
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(rt(title, f_disp(16.5), c(TEXT)));
    ui.add_space(2.0);
}

fn row_head(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.add_space(12.0);
    ui.label(rt(title, f_sb(14.0), c(TEXT)));
    ui.label(rt(sub, f_md(12.0), c(DIM)));
    ui.add_space(8.0);
}

fn playtime_card(ui: &mut egui::Ui, stats: &config::Stats) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(rt("Spielverlauf", f_disp(16.0), c(TEXT)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(rt(&format!("letzte {} Sitzungen", stats.sessions.len()), f_md(12.0), c(DIM)));
            });
        });
        ui.add_space(14.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
        let p = ui.painter();
        let max = stats.sessions.iter().map(|s| s.secs).max().unwrap_or(1).max(1) as f32;
        let n = stats.sessions.len().max(1);
        let gap = 5.0;
        let bw = ((rect.width() - gap * (n as f32 - 1.0)) / n as f32).clamp(3.0, 26.0);
        let mut x = rect.left();
        for s in &stats.sessions {
            let h = (s.secs as f32 / max * rect.height()).max(3.0);
            let bar = Rect::from_min_max(pos2(x, rect.bottom() - h), pos2(x + bw, rect.bottom()));
            v_gradient(p, bar, c(ACCENT), c(ACCENT_2));
            x += bw + gap;
        }
    });
}

fn log_box(app: &DolphinApp, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(c(BG_2))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(ROUND))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().max_height(180.0).stick_to_bottom(true).show(ui, |ui| {
                if app.log.is_empty() {
                    ui.label(rt("— noch keine Ausgaben —", f_md(12.0), c(FAINT)));
                }
                for line in &app.log {
                    ui.label(egui::RichText::new(line).monospace().size(11.5).color(c(DIM)));
                }
            });
        });
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(soft(BG_1, 245))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(ROUND))
        .inner_margin(egui::Margin::same(20.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn card_tinted(ui: &mut egui::Ui, rgb: [u8; 3], add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(lerp_color(c(BG_1), c(rgb), 0.10))
        .stroke(Stroke::new(1.0, soft(rgb, 120)))
        .rounding(Rounding::same(ROUND))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn primary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(label.len() as f32 * 8.6 + 40.0, 42.0), Sense::click());
    let hov = enabled && resp.hovered();
    let p = ui.painter();
    let rounding = Rounding::same(11.0);
    if enabled {
        let (l, r) = if hov {
            (lerp_color(c(ACCENT), Color32::WHITE, 0.12), lerp_color(c(ACCENT_2), Color32::WHITE, 0.06))
        } else {
            (c(ACCENT), c(ACCENT_2))
        };
        p.rect_filled(rect, rounding, r);
        h_gradient(p, rect, l, r);
    } else {
        p.rect_filled(rect, rounding, c(BG_3));
        p.rect_stroke(rect, rounding, Stroke::new(1.0, c(LINE)));
    }
    let col = if enabled { c(INK) } else { c(DIM) };
    p.text(rect.center(), Align2::CENTER_CENTER, label, f_sb(14.0), col);
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked() && enabled
}

fn ghost_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(label.len() as f32 * 8.2 + 34.0, 42.0), Sense::click());
    let hov = enabled && resp.hovered();
    let p = ui.painter();
    let rounding = Rounding::same(11.0);
    p.rect_filled(rect, rounding, if hov { c(BG_3) } else { c(BG_2) });
    p.rect_stroke(rect, rounding, Stroke::new(1.0, if hov { c(LINE_2) } else { c(LINE) }));
    let col = if enabled { c(TEXT) } else { c(FAINT) };
    p.text(rect.center(), Align2::CENTER_CENTER, label, f_sb(14.0), col);
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked() && enabled
}

fn small_button(ui: &mut egui::Ui, label: &str, tint: Color32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(label.len() as f32 * 7.4 + 22.0, 32.0), Sense::click());
    let hov = resp.hovered();
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(9.0), if hov { c(BG_4) } else { c(BG_3) });
    p.rect_stroke(rect, Rounding::same(9.0), Stroke::new(1.0, c(LINE)));
    p.text(rect.center(), Align2::CENTER_CENTER, label, f_sb(12.5), tint);
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn draw_avatar(painter: &egui::Painter, app: &DolphinApp, rect: Rect) {
    painter.rect_filled(rect, Rounding::same(10.0), c(BG_3));
    let tex = app.avatar.lock().ok().and_then(|a| a.clone());
    match (tex, app.accounts.active_account()) {
        (Some(tex), Some(_)) => {
            painter.image(tex.id(), rect.shrink(3.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        (_, Some(a)) => {
            painter.text(rect.center(), Align2::CENTER_CENTER, initials(&a.username), f_sb(14.0), c(TEXT));
        }
        _ => {
            painter.text(rect.center(), Align2::CENTER_CENTER, "?", f_sb(14.0), c(DIM));
        }
    }
    painter.rect_stroke(rect, Rounding::same(10.0), Stroke::new(1.0, c(LINE_2)));
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

fn fmt_relative(last: Option<u64>) -> String {
    let Some(at) = last else { return "—".to_string() };
    let now = config::now_unix();
    if now <= at {
        return "gerade".to_string();
    }
    let d = now - at;
    if d < 60 {
        "gerade".to_string()
    } else if d < 3600 {
        format!("vor {} min", d / 60)
    } else if d < 86_400 {
        format!("vor {} h", d / 3600)
    } else if d < 7 * 86_400 {
        let days = d / 86_400;
        if days == 1 { "gestern".to_string() } else { format!("vor {days} T") }
    } else {
        format!("vor {} Wo", d / (7 * 86_400))
    }
}
