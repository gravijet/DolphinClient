//! The launcher UI — a modern, cinematic client launcher in the spirit of
//! Lunar / Badlion.
//!
//! Layout: a **left icon sidebar** (logo, nav, account) and a large content
//! area. The Start page is an asymmetric hero — your character rendered as
//! key-art on the right, a big title and a prominent PLAY button on the left,
//! over a restrained single-hue wash. Everything is drawn with egui's painter
//! and custom widgets (custom sliders, a custom version dropdown), with a
//! bundled geometric typeface (Outfit + Sora) so it never reads as a stock
//! egui app. Settings save on change.

use std::f32::consts::TAU;

use eframe::egui::epaint::{Mesh, Vertex, WHITE_UV};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontFamily, FontId, Rect, Rounding, Sense,
    Shape, Stroke, ViewportCommand,
};

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

/* Rounding scale — generous, soft corners so nothing reads as "angular". */
const R_CARD: f32 = 18.0; // cards / panels / window
const R_BTN: f32 = 16.0; // primary buttons / big controls
const R_CTL: f32 = 13.0; // inputs, chips, nav items, dropdowns
const R_SM: f32 = 11.0; // small buttons / avatars

/* ---------------------------------------------------------------- */
/*  Mesh helpers — gradients + soft ambient light                    */
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
        mesh.vertices.push(Vertex {
            pos: p,
            uv: WHITE_UV,
            color: col,
        });
    }
    mesh.indices
        .extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
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
        mesh.vertices.push(Vertex {
            pos: p,
            uv: WHITE_UV,
            color: col,
        });
    }
    mesh.indices
        .extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    painter.add(Shape::mesh(mesh));
}

/// A soft radial ambient light — used ONLY for backdrop lighting behind the
/// character, never around buttons or controls.
fn ambient(painter: &egui::Painter, center: egui::Pos2, radius: f32, inner: Color32) {
    const SEG: usize = 48;
    let outer = Color32::from_rgba_unmultiplied(inner.r(), inner.g(), inner.b(), 0);
    let mut mesh = Mesh::default();
    let c_idx = mesh.vertices.len() as u32;
    mesh.vertices.push(Vertex {
        pos: center,
        uv: WHITE_UV,
        color: inner,
    });
    for k in 0..=SEG {
        let a = k as f32 / SEG as f32 * TAU;
        let p = center + vec2(a.cos(), a.sin()) * radius;
        mesh.vertices.push(Vertex {
            pos: p,
            uv: WHITE_UV,
            color: outer,
        });
    }
    for k in 1..=SEG as u32 {
        mesh.indices
            .extend_from_slice(&[c_idx, c_idx + k, c_idx + k + 1]);
    }
    painter.add(Shape::mesh(mesh));
}

/* ---------------------------------------------------------------- */
/*  Theme                                                            */
/* ---------------------------------------------------------------- */

pub fn install_theme(ctx: &egui::Context, _accent_name: &str) {
    let accent = c(ACCENT);
    let round = Rounding::same(R_CTL);
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(c(TEXT));
    v.panel_fill = c(BG_0);
    v.window_fill = c(BG_1);
    v.window_stroke = Stroke::new(1.0, c(LINE));
    v.window_rounding = Rounding::same(R_CARD);
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
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
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
        .frame(
            egui::Frame::none()
                .fill(c(BG_0))
                .inner_margin(egui::Margin::ZERO),
        )
        .show(ctx, |ui| {
            let full = ui.max_rect();
            let t = seconds(ctx);

            // Page background depends on the tab (Home gets the cinematic wash).
            match app.tab {
                Tab::Home => home_backdrop(app, ui, full, t),
                _ => page_backdrop(ui.painter(), full),
            }

            // Top strip: draggable + window controls (drawn over the backdrop).
            top_strip(app, ctx, ui, full);

            // Content.
            let body = Rect::from_min_max(
                pos2(full.left(), full.top() + TOPBAR_H),
                full.right_bottom(),
            );
            let mut cui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(body)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            match app.tab {
                Tab::Home => home_view(app, &mut cui),
                Tab::Game => scroll_page(&mut cui, |ui| game_view(app, ui)),
                Tab::Cosmetics => scroll_page(&mut cui, |ui| cosmetics_view(app, ui)),
                Tab::Mods => coming_soon_view(&mut cui, SoonPage::Mods),
                Tab::Friends => coming_soon_view(&mut cui, SoonPage::Friends),
                Tab::Accounts => scroll_page(&mut cui, |ui| accounts_view(app, ui)),
                Tab::Settings => scroll_page(&mut cui, |ui| settings_view(app, ui)),
            }
        });
}

/// Standard scrolling page with a centered max-width column (non-Home tabs).
fn scroll_page(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(30.0);
            let w = 720.0_f32.min(ui.available_width() - 96.0);
            let pad = ((ui.available_width() - w) / 2.0).max(24.0);
            ui.horizontal(|ui| {
                ui.add_space(pad);
                ui.allocate_ui_with_layout(
                    vec2(w, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    add,
                );
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
            p.line_segment(
                [rect.right_top(), rect.right_bottom()],
                Stroke::new(1.0, c(LINE)),
            );

            // Logo + wordmark.
            let logo = Rect::from_min_size(
                pos2(rect.left() + 22.0, rect.top() + 24.0),
                vec2(30.0, 30.0),
            );
            p.image(
                app.logo.id(),
                logo,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            let r = p.text(
                pos2(logo.right() + 12.0, logo.center().y - 1.0),
                Align2::LEFT_CENTER,
                "Dolphin",
                f_disp(17.0),
                c(TEXT),
            );
            p.text(
                pos2(r.right(), logo.center().y - 1.0),
                Align2::LEFT_CENTER,
                "Client",
                f_disp(17.0),
                c(ACCENT),
            );

            // Nav — grouped: play, then the upcoming areas, then account/config.
            let mut y = rect.top() + 88.0;
            for (label, tab, icon) in [
                ("Home", Tab::Home, NavIcon::Home),
                ("Game", Tab::Game, NavIcon::Game),
            ] {
                if nav_item(ui, &p, rect, &mut y, label, icon, app.tab == tab, false) {
                    app.tab = tab;
                }
            }
            nav_section(&p, rect, &mut y, "DISCOVER");
            for (label, tab, icon) in [
                ("Cosmetics", Tab::Cosmetics, NavIcon::Cosmetics),
                ("Mods", Tab::Mods, NavIcon::Mods),
                ("Friends", Tab::Friends, NavIcon::Friends),
            ] {
                if nav_item(ui, &p, rect, &mut y, label, icon, app.tab == tab, true) {
                    app.tab = tab;
                }
            }
            nav_section(&p, rect, &mut y, "ACCOUNT");
            for (label, tab, icon) in [
                ("Accounts", Tab::Accounts, NavIcon::Accounts),
                ("Settings", Tab::Settings, NavIcon::Settings),
            ] {
                if nav_item(ui, &p, rect, &mut y, label, icon, app.tab == tab, false) {
                    app.tab = tab;
                }
            }

            // Account card pinned to the bottom.
            account_card(app, ui, &p, rect);

            // Thin activity bar under the card while busy.
            if app.busy || app.progress > 0.001 {
                let w = (SIDEBAR_W - 32.0) * app.progress.clamp(0.05, 1.0);
                let y = rect.bottom() - 84.0;
                p.rect_filled(
                    Rect::from_min_size(pos2(rect.left() + 16.0, y), vec2(SIDEBAR_W - 32.0, 3.0)),
                    Rounding::same(2.0),
                    c(BG_3),
                );
                p.rect_filled(
                    Rect::from_min_size(pos2(rect.left() + 16.0, y), vec2(w, 3.0)),
                    Rounding::same(2.0),
                    c(ACCENT),
                );
            }
        });
}

#[derive(Clone, Copy)]
enum NavIcon {
    Home,
    Game,
    Cosmetics,
    Mods,
    Friends,
    Accounts,
    Settings,
}

/// Small uppercase group heading between nav clusters.
fn nav_section(p: &egui::Painter, area: Rect, y: &mut f32, label: &str) {
    *y += 14.0;
    p.text(
        pos2(area.left() + 22.0, *y),
        Align2::LEFT_TOP,
        label,
        f_sb(10.5),
        c(FAINT),
    );
    *y += 22.0;
}

fn nav_item(
    ui: &mut egui::Ui,
    p: &egui::Painter,
    area: Rect,
    y: &mut f32,
    label: &str,
    icon: NavIcon,
    active: bool,
    soon: bool,
) -> bool {
    let h = 44.0;
    let rect = Rect::from_min_size(pos2(area.left() + 12.0, *y), vec2(SIDEBAR_W - 24.0, h));
    *y += h + 5.0;
    let resp = ui.interact(rect, ui.id().with(("nav", label)), Sense::click());
    let hov = resp.hovered();
    if active {
        // Soft filled pill only — no left indicator bar.
        p.rect_filled(rect, Rounding::same(R_CTL), soft(ACCENT, 26));
    } else if hov {
        p.rect_filled(rect, Rounding::same(R_CTL), soft(LINE_2, 40));
    }
    let col = if active {
        c(ACCENT)
    } else if hov {
        c(TEXT)
    } else {
        c(DIM)
    };
    let icx = rect.left() + 20.0;
    let icy = rect.center().y;
    draw_nav_icon(p, icon, pos2(icx, icy), col);
    p.text(
        pos2(rect.left() + 44.0, icy),
        Align2::LEFT_CENTER,
        label,
        f_sb(14.5),
        col,
    );
    // "Soon" pill on the right for upcoming sections.
    if soon {
        let pill = Rect::from_min_size(pos2(rect.right() - 50.0, icy - 9.0), vec2(40.0, 18.0));
        p.rect_filled(
            pill,
            Rounding::same(9.0),
            soft(ACCENT, if active || hov { 34 } else { 20 }),
        );
        p.text(
            pill.center(),
            Align2::CENTER_CENTER,
            "Soon",
            f_sb(10.0),
            c(ACCENT),
        );
    }
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
            p.add(Shape::line(
                vec![
                    pos2(x - 8.0, y + 1.0),
                    pos2(x, y - 8.0),
                    pos2(x + 8.0, y + 1.0),
                ],
                s,
            ));
            // Body.
            p.rect_stroke(
                Rect::from_min_max(pos2(x - 6.0, y + 1.0), pos2(x + 6.0, y + 8.0)),
                Rounding::same(2.0),
                s,
            );
        }
        NavIcon::Game => {
            // Three sliders.
            for (i, kx) in [(-6.0_f32, 3.0_f32), (0.0, -3.0), (6.0, 4.0)]
                .iter()
                .enumerate()
            {
                let (dy, knob) = (kx.0, kx.1);
                let yy = ctr.y + dy;
                p.line_segment(
                    [pos2(ctr.x - 8.0, yy), pos2(ctr.x + 8.0, yy)],
                    Stroke::new(1.6, col),
                );
                p.circle_filled(pos2(ctr.x + knob, yy), 2.4, col);
                let _ = i;
            }
        }
        NavIcon::Accounts => {
            p.circle_stroke(pos2(ctr.x, ctr.y - 3.0), 3.6, s);
            // Shoulders arc approximated with a stroked half-rounded rect.
            let b = Rect::from_min_max(
                pos2(ctr.x - 7.0, ctr.y + 2.0),
                pos2(ctr.x + 7.0, ctr.y + 11.0),
            );
            p.add(Shape::line(
                vec![
                    pos2(b.left(), b.bottom()),
                    pos2(b.left(), b.top() + 2.0),
                    pos2(b.right(), b.top() + 2.0),
                    pos2(b.right(), b.bottom()),
                ],
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
        NavIcon::Cosmetics => {
            // A four-point sparkle (shine / cosmetics).
            let (bg, sm) = (8.5_f32, 2.6_f32);
            p.add(Shape::convex_polygon(
                vec![
                    pos2(ctr.x, ctr.y - bg),
                    pos2(ctr.x + sm, ctr.y - sm),
                    pos2(ctr.x + bg, ctr.y),
                    pos2(ctr.x + sm, ctr.y + sm),
                    pos2(ctr.x, ctr.y + bg),
                    pos2(ctr.x - sm, ctr.y + sm),
                    pos2(ctr.x - bg, ctr.y),
                    pos2(ctr.x - sm, ctr.y - sm),
                ],
                col,
                Stroke::NONE,
            ));
        }
        NavIcon::Mods => {
            // 2×2 grid of rounded modules.
            for (dx, dy) in [(-7.0_f32, -7.0_f32), (1.0, -7.0), (-7.0, 1.0), (1.0, 1.0)] {
                p.rect_stroke(
                    Rect::from_min_size(pos2(ctr.x + dx, ctr.y + dy), vec2(6.0, 6.0)),
                    Rounding::same(1.8),
                    Stroke::new(1.6, col),
                );
            }
        }
        NavIcon::Friends => {
            // Two overlapping people.
            p.circle_stroke(pos2(ctr.x + 4.5, ctr.y - 3.5), 2.8, Stroke::new(1.5, col));
            let b1 = Rect::from_min_max(
                pos2(ctr.x + 0.5, ctr.y + 0.5),
                pos2(ctr.x + 8.5, ctr.y + 7.5),
            );
            p.add(Shape::line(
                vec![
                    pos2(b1.left(), b1.bottom()),
                    pos2(b1.left(), b1.top()),
                    pos2(b1.right(), b1.top()),
                    pos2(b1.right(), b1.bottom()),
                ],
                Stroke::new(1.5, col),
            ));
            p.circle_stroke(pos2(ctr.x - 4.0, ctr.y - 3.0), 3.2, s);
            let b2 = Rect::from_min_max(
                pos2(ctr.x - 8.5, ctr.y + 1.5),
                pos2(ctr.x + 0.5, ctr.y + 9.0),
            );
            p.add(Shape::line(
                vec![
                    pos2(b2.left(), b2.bottom()),
                    pos2(b2.left(), b2.top()),
                    pos2(b2.right(), b2.top()),
                    pos2(b2.right(), b2.bottom()),
                ],
                s,
            ));
        }
    }
}

fn account_card(app: &mut DolphinApp, ui: &mut egui::Ui, p: &egui::Painter, area: Rect) {
    let h = 62.0;
    let rect = Rect::from_min_size(
        pos2(area.left() + 12.0, area.bottom() - h - 16.0),
        vec2(SIDEBAR_W - 24.0, h),
    );
    let resp = ui.interact(rect, ui.id().with("acctcard"), Sense::click());
    let hov = resp.hovered();
    p.rect_filled(
        rect,
        Rounding::same(R_CTL),
        if hov { c(BG_3) } else { c(BG_2) },
    );
    p.rect_stroke(rect, Rounding::same(R_CTL), Stroke::new(1.0, c(LINE)));
    let av = Rect::from_min_size(
        pos2(rect.left() + 10.0, rect.center().y - 17.0),
        vec2(34.0, 34.0),
    );
    draw_avatar(p, app, av);
    let active = app.accounts.active_account().cloned();
    let name = active
        .as_ref()
        .map(|a| a.username.clone())
        .unwrap_or_else(|| "No account".to_string());
    let (state, scol) = if app.relogin_for.is_some() {
        ("Sign-in needed", c(RED))
    } else if running(app) {
        ("in game", c(GREEN))
    } else if active.is_some() {
        ("ready", c(DIM))
    } else {
        ("sign in", c(FAINT))
    };
    p.text(
        pos2(av.right() + 10.0, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        truncate(&name, 13),
        f_sb(13.5),
        c(TEXT),
    );
    p.text(
        pos2(av.right() + 10.0, rect.center().y + 8.0),
        Align2::LEFT_CENTER,
        state,
        f_md(11.0),
        scol,
    );
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
        Tab::Game => "Game",
        Tab::Cosmetics => "Cosmetics",
        Tab::Mods => "Mods",
        Tab::Friends => "Friends",
        Tab::Accounts => "Accounts",
        Tab::Settings => "Settings",
    };
    if !title.is_empty() {
        p.text(
            pos2(bar.left() + 30.0, bar.center().y),
            Align2::LEFT_CENTER,
            title,
            f_disp(18.0),
            c(TEXT),
        );
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
    let resp = ui.interact(
        rect,
        ui.id().with(format!("winbtn{}", *x as i32)),
        Sense::click(),
    );
    let hovered = resp.hovered();
    let painter = ui.painter();
    let fill = match (&kind, hovered) {
        (WindowButton::Close, true) => c(RED),
        (_, true) => c(BG_3),
        _ => Color32::TRANSPARENT,
    };
    painter.rect_filled(rect, Rounding::same(9.0), fill);
    let ctr = rect.center();
    let col = if hovered { Color32::WHITE } else { c(DIM) };
    let s = Stroke::new(1.5, col);
    match kind {
        WindowButton::Minimize => {
            painter.line_segment([ctr + vec2(-5.0, 2.0), ctr + vec2(5.0, 2.0)], s);
        }
        WindowButton::Maximize => {
            painter.rect_stroke(
                Rect::from_center_size(ctr, vec2(9.0, 9.0)),
                Rounding::same(2.0),
                s,
            );
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

/// Quiet backdrop for the non-Home tabs: a single soft top wash, no motion.
fn page_backdrop(painter: &egui::Painter, rect: Rect) {
    ambient(
        painter,
        pos2(rect.left() + rect.width() * 0.7, rect.top() - 40.0),
        520.0,
        soft(ACCENT_2, 16),
    );
}

/// Cinematic Home backdrop: a horizontal wash from a deep accent-tinted left to
/// the ground, a large soft light behind the character, and the character
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
                // Soft light behind the character.
                ambient(
                    &p,
                    pos2(cx, rect.center().y + 30.0),
                    target_h * 0.62,
                    soft(ACCENT, 40),
                );
                p.image(
                    tex.id(),
                    Rect::from_min_size(pos2(cx - target_w / 2.0, top), vec2(target_w, target_h)),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
    } else {
        // Ambient light for the signed-out state.
        ambient(
            &p,
            pos2(rect.left() + rect.width() * 0.7, rect.center().y),
            440.0,
            soft(ACCENT, 34),
        );
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

    // Floating notices (updates / relogin / device-login) top-right.
    notices(app, ui, area);

    match active {
        None => signed_out_hero(app, ui, area),
        Some(acc) => {
            let left = area.left() + 44.0;
            let p = ui.painter().clone();

            // Eyebrow.
            let mut y = area.top() + 40.0;
            p.text(
                pos2(left, y),
                Align2::LEFT_TOP,
                "WELCOME BACK",
                f_sb(12.5),
                c(ACCENT),
            );
            y += 34.0;

            // Big name.
            p.text(
                pos2(left, y),
                Align2::LEFT_TOP,
                truncate(&acc.username, 16),
                f_disp(52.0),
                c(TEXT),
            );
            y += 70.0;

            // Subline.
            let ver = if app.settings.client_version.is_empty() {
                "Latest".to_string()
            } else {
                app.settings.client_version.clone()
            };
            p.text(
                pos2(left, y),
                Align2::LEFT_TOP,
                &format!(
                    "DolphinClient {ver}   ·   Minecraft {}",
                    config::TARGET_VERSION
                ),
                f_md(14.5),
                c(DIM),
            );
            y += 40.0;

            // Stat strip.
            let stats = app.stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
            stat_strip(&p, pos2(left, y), &stats);
            y += 78.0;

            // PLAY row (button + version pill) — placed with real widgets.
            let mut row = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_max(
                        pos2(left, y),
                        pos2(left + 560.0, y + 66.0),
                    ))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            if launch_button(&mut row, is_running, app.busy) && !app.busy && !is_running {
                app.start_launch(&ctx);
            }
            row.add_space(14.0);
            version_dropdown(app, &mut row);
            y += 66.0 + 16.0;

            // Status line.
            if !app.status.is_empty() {
                let scol = if app.status.starts_with("Error") || app.relogin_for.is_some() {
                    c(RED)
                } else if is_running {
                    c(GREEN)
                } else {
                    c(DIM)
                };
                ui.painter().text(
                    pos2(left, y),
                    Align2::LEFT_TOP,
                    truncate(&app.status, 70),
                    f_md(12.5),
                    scol,
                );
                y += 26.0;
            }

            // Quick-join servers (chips) along the bottom-left.
            if !app.settings.servers.is_empty() {
                let sy = (area.bottom() - 96.0).max(y + 12.0);
                ui.painter().text(
                    pos2(left, sy - 22.0),
                    Align2::LEFT_BOTTOM,
                    "QUICK JOIN",
                    f_sb(11.5),
                    c(FAINT),
                );
                let servers = app.settings.servers.clone();
                let mut sx = left;
                let mut join: Option<String> = None;
                for s in servers.iter().take(4) {
                    if server_chip(ui, &mut sx, sy, &s.name, &s.address) && !app.busy && !is_running
                    {
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
    p.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "YOUR MINECRAFT, UPGRADED",
        f_sb(12.5),
        c(ACCENT),
    );
    y += 34.0;
    p.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "Welcome.",
        f_disp(54.0),
        c(TEXT),
    );
    y += 76.0;
    p.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "Sign in with Microsoft, or import an existing",
        f_md(15.0),
        c(DIM),
    );
    y += 24.0;
    p.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "account already on this PC.",
        f_md(15.0),
        c(DIM),
    );
    y += 44.0;

    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(left, y),
                pos2(left + 600.0, y + 52.0),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    if primary_button(&mut row, "Sign in with Microsoft", !app.busy) {
        app.add_microsoft(&ctx);
    }
    row.add_space(10.0);
    if ghost_button(&mut row, "Import account", true) {
        app.import_accounts();
    }
    if crate::tokens::has_token() {
        row.add_space(10.0);
        if ghost_button(&mut row, "Previous account", true) {
            app.start_login(&ctx, LoginMethod::Refresh);
        }
    }
    if let Some(note) = &app.import_note {
        ui.painter().text(
            pos2(left, y + 66.0),
            Align2::LEFT_TOP,
            note,
            f_md(13.0),
            c(GREEN),
        );
    }
    login_prompts_floating(app, ui, area);
}

/// Inline stat strip: "value label   ·   value label …" drawn by the painter.
fn stat_strip(p: &egui::Painter, at: egui::Pos2, stats: &config::Stats) {
    let items = [
        (fmt_playtime(stats.playtime_secs), "Playtime"),
        (stats.launches.to_string(), "Launches"),
        (fmt_relative(stats.last_played), "Last played"),
    ];
    let mut x = at.x;
    for (i, (value, label)) in items.iter().enumerate() {
        if i > 0 {
            p.text(
                pos2(x, at.y + 14.0),
                Align2::LEFT_CENTER,
                "·",
                f_md(18.0),
                c(FAINT),
            );
            x += 26.0;
        }
        let r = p.text(
            pos2(x, at.y + 6.0),
            Align2::LEFT_TOP,
            value,
            f_disp(20.0),
            c(TEXT),
        );
        let r2 = p.text(
            pos2(x, at.y + 34.0),
            Align2::LEFT_TOP,
            label,
            f_md(11.5),
            c(DIM),
        );
        x = r.right().max(r2.right()) + 26.0;
    }
}

/// The big PLAY button — a clean accent gradient with a subtle top sheen. No
/// glow (the user dislikes it), just a solid, confident, rounded button.
fn launch_button(ui: &mut egui::Ui, is_running: bool, busy: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(268.0, 62.0), Sense::click());
    let enabled = !busy && !is_running;
    let hov = enabled && resp.hovered();
    let painter = ui.painter();
    let rounding = Rounding::same(R_BTN);
    if enabled {
        let (l, r) = if hov {
            (
                lerp_color(c(ACCENT), Color32::WHITE, 0.12),
                lerp_color(c(ACCENT_2), Color32::WHITE, 0.06),
            )
        } else {
            (c(ACCENT), c(ACCENT_2))
        };
        painter.rect_filled(rect, rounding, r);
        h_gradient(painter, rect, l, r);
        // A whisper-thin top sheen for a little depth (not a glow).
        let sheen = Rect::from_min_max(rect.left_top(), pos2(rect.right(), rect.center().y));
        v_gradient(
            painter,
            sheen,
            soft([0xFF, 0xFF, 0xFF], 16),
            Color32::TRANSPARENT,
        );
    } else {
        painter.rect_filled(rect, rounding, c(BG_3));
        painter.rect_stroke(rect, rounding, Stroke::new(1.0, c(LINE)));
    }
    let (label, tri) = if is_running {
        ("IN GAME", false)
    } else if busy {
        ("…", false)
    } else {
        ("PLAY", true)
    };
    let fg = if enabled { c(INK) } else { c(DIM) };
    let mut tx = rect.center().x;
    if tri {
        tx += 14.0;
        let cy = rect.center().y;
        let lx = rect.center().x - 52.0;
        painter.add(Shape::convex_polygon(
            vec![pos2(lx, cy - 9.0), pos2(lx + 15.0, cy), pos2(lx, cy + 9.0)],
            fg,
            Stroke::NONE,
        ));
    }
    painter.text(
        pos2(tx, rect.center().y),
        Align2::CENTER_CENTER,
        label,
        f_disp(19.0),
        fg,
    );
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked() && enabled
}

/// Custom version dropdown pill (no eguiish ComboBox).
fn version_dropdown(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let versions = app.versions.lock().map(|v| v.clone()).unwrap_or_default();
    let sel = app.settings.client_version.clone();
    let label = if sel.is_empty() {
        "Latest".to_string()
    } else {
        sel.clone()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(158.0, 52.0), Sense::click());
    let hov = resp.hovered();
    let p = ui.painter();
    p.rect_filled(
        rect,
        Rounding::same(R_BTN),
        if hov { c(BG_3) } else { c(BG_2) },
    );
    p.rect_stroke(
        rect,
        Rounding::same(R_BTN),
        Stroke::new(1.0, if hov { c(LINE_2) } else { c(LINE) }),
    );
    p.text(
        pos2(rect.left() + 14.0, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        "VERSION",
        f_sb(9.5),
        c(FAINT),
    );
    p.text(
        pos2(rect.left() + 14.0, rect.center().y + 8.0),
        Align2::LEFT_CENTER,
        truncate(&label, 14),
        f_sb(13.5),
        c(TEXT),
    );
    // Chevron.
    let cx = rect.right() - 16.0;
    let cy = rect.center().y;
    p.add(Shape::line(
        vec![
            pos2(cx - 4.0, cy - 2.0),
            pos2(cx, cy + 2.5),
            pos2(cx + 4.0, cy - 2.0),
        ],
        Stroke::new(1.6, c(DIM)),
    ));
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let popup_id = ui.id().with("verpop");
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    let mut pick: Option<String> = None;
    egui::popup_below_widget(
        ui,
        popup_id,
        &resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(rect.width());
            ui.spacing_mut().item_spacing.y = 2.0;
            if dropdown_row(ui, "Latest (recommended)", sel.is_empty()) {
                pick = Some(String::new());
            }
            for v in &versions {
                if dropdown_row(ui, v, &sel == v) {
                    pick = Some(v.clone());
                }
            }
        },
    );
    if let Some(v) = pick {
        app.settings.client_version = v;
        app.settings.save();
        ui.memory_mut(|m| m.close_popup());
    }
}

fn dropdown_row(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width().max(150.0), 30.0), Sense::click());
    let hov = resp.hovered();
    let p = ui.painter();
    if hov || selected {
        p.rect_filled(
            rect,
            Rounding::same(9.0),
            if selected {
                soft(ACCENT, 28)
            } else {
                soft(LINE_2, 40)
            },
        );
    }
    let col = if selected { c(ACCENT) } else { c(TEXT) };
    p.text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        f_md(13.0),
        col,
    );
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn server_chip(ui: &mut egui::Ui, x: &mut f32, y_bottom: f32, name: &str, addr: &str) -> bool {
    let label = truncate(name, 18);
    let galley = ui
        .painter()
        .layout_no_wrap(label.clone(), f_sb(13.0), c(TEXT));
    let w = (galley.size().x + 46.0).clamp(120.0, 220.0);
    let h = 46.0;
    let rect = Rect::from_min_size(pos2(*x, y_bottom - h), vec2(w, h));
    *x += w + 10.0;
    let resp = ui.interact(rect, ui.id().with(("srv", name, addr)), Sense::click());
    let hov = resp.hovered();
    let p = ui.painter();
    p.rect_filled(
        rect,
        Rounding::same(R_CTL),
        if hov { c(BG_3) } else { c(BG_2) },
    );
    p.rect_stroke(
        rect,
        Rounding::same(R_CTL),
        Stroke::new(1.0, if hov { c(ACCENT) } else { c(LINE) }),
    );
    let cy = rect.center().y;
    let lx = rect.left() + 15.0;
    p.add(Shape::convex_polygon(
        vec![pos2(lx, cy - 6.0), pos2(lx + 9.0, cy), pos2(lx, cy + 6.0)],
        c(ACCENT),
        Stroke::NONE,
    ));
    p.text(
        pos2(lx + 16.0, cy - 7.0),
        Align2::LEFT_CENTER,
        label,
        f_sb(13.0),
        c(TEXT),
    );
    p.text(
        pos2(lx + 16.0, cy + 8.0),
        Align2::LEFT_CENTER,
        truncate(addr, 22),
        f_md(10.5),
        c(DIM),
    );
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
            .max_rect(Rect::from_min_max(
                pos2(x, area.top() + 20.0),
                pos2(x + col_w, area.bottom() - 20.0),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    if let Some(name) = app.relogin_for.clone() {
        card_tinted(&mut cui, RED, |ui| {
            ui.label(rt(
                &format!("Sign-in required for {name}"),
                f_sb(14.0),
                c(TEXT),
            ));
            ui.add_space(2.0);
            ui.label(rt(
                "Session couldn't be restored. Please sign in again.",
                f_md(12.0),
                c(DIM),
            ));
            ui.add_space(10.0);
            if primary_button(ui, "Sign in again", !app.busy) {
                app.add_microsoft(&ctx);
            }
        });
        cui.add_space(12.0);
    }
    if let Some(info) = app.update_note.lock().ok().and_then(|n| n.clone()) {
        card_tinted(&mut cui, GOLD, |ui| {
            ui.label(rt(
                &format!("Update {} available", info.version),
                f_sb(14.0),
                c(TEXT),
            ));
            ui.add_space(10.0);
            if primary_button(ui, "Update now", !app.busy) {
                app.start_self_update(&ctx, info.clone());
            }
        });
        cui.add_space(12.0);
    }
    login_prompts_in(app, &mut cui);
}

/* ---------------------------------------------------------------- */
/*  Game — quick-settings (custom sliders) + servers                 */
/* ---------------------------------------------------------------- */

fn game_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    ui.label(rt(
        "Graphics and performance settings — applied the next time you play.",
        f_md(14.0),
        c(DIM),
    ));
    ui.add_space(16.0);

    let is_running = running(app);
    if is_running {
        card_tinted(ui, GOLD, |ui| {
            ui.label(rt(
                "The game is running — changes apply after you close it.",
                f_md(12.5),
                c(TEXT),
            ));
        });
        ui.add_space(12.0);
    }

    ui.add_enabled_ui(!is_running, |ui| {
        card(ui, |ui| {
            section_title(ui, "Performance");
            let mut rd = app.gameopts.render_distance();
            if slider_row(
                ui,
                "Render distance",
                &format!("{rd} chunks"),
                &mut rd,
                2,
                32,
            ) {
                app.gameopts.set_render_distance(rd);
                app.gameopts.save();
            }
            let mut fps = app.gameopts.max_fps() as i32;
            let fl = if fps == 0 {
                "Unlimited".to_string()
            } else {
                format!("{fps} FPS")
            };
            if slider_row_step(ui, "Frame rate limit", &fl, &mut fps, 0, 360, 10) {
                app.gameopts.set_max_fps(fps as u32);
                app.gameopts.save();
            }
            let mut vsync = app.gameopts.vsync();
            if toggle_row(
                ui,
                "VSync",
                "Sync the frame rate to your monitor's refresh rate.",
                &mut vsync,
            ) {
                app.gameopts.set_vsync(vsync);
                app.gameopts.save();
            }
        });
        ui.add_space(14.0);

        card(ui, |ui| {
            section_title(ui, "Display");
            let mut fovf = app.gameopts.fov();
            let mut fov = fovf.round() as i32;
            if slider_row(
                ui,
                "Field of view (FOV)",
                &format!("{fov}°"),
                &mut fov,
                30,
                110,
            ) {
                fovf = fov as f32;
                app.gameopts.set_fov(fovf);
                app.gameopts.save();
            }
            let mut brp = (app.gameopts.brightness() * 100.0).round() as i32;
            if slider_row(ui, "Brightness", &format!("{brp}%"), &mut brp, 0, 100) {
                app.gameopts.set_brightness(brp as f32 / 100.0);
                app.gameopts.save();
            }
            row_head(
                ui,
                "GUI scale",
                "0 = scale automatically to the window size.",
            );
            let gs = app.gameopts.gui_scale();
            if let Some(sel) = segmented(ui, &["Auto", "1", "2", "3", "4"], gs as usize) {
                app.gameopts.set_gui_scale(sel as u32);
                app.gameopts.save();
            }
            ui.add_space(8.0);
            row_head(ui, "Graphics", "Fancy looks better, Fast gives more FPS.");
            let cur = if app.gameopts.graphics() == crate::gameopts::Graphics::Fast {
                0
            } else {
                1
            };
            if let Some(sel) = segmented(ui, &["Fast", "Fancy"], cur) {
                let ng = if sel == 0 {
                    crate::gameopts::Graphics::Fast
                } else {
                    crate::gameopts::Graphics::Fancy
                };
                app.gameopts.set_graphics(ng);
                app.gameopts.save();
            }
            ui.add_space(6.0);
            if toggle_row(
                ui,
                "Start in fullscreen",
                "Open the game directly in fullscreen.",
                &mut app.settings.fullscreen,
            ) {
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
        section_title(ui, "Servers");
        ui.label(rt(
            "Save your favorite servers to join them in one click from the home screen.",
            f_md(12.0),
            c(DIM),
        ));
        ui.add_space(12.0);
        let servers = app.settings.servers.clone();
        let default_addr = app.settings.server.clone();
        let mut remove: Option<usize> = None;
        let mut make_default: Option<String> = None;
        for (i, s) in servers.iter().enumerate() {
            let is_default = !default_addr.is_empty() && default_addr == s.address;
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(rect, Rounding::same(R_CTL), c(BG_3));
            p.rect_stroke(
                rect,
                Rounding::same(R_CTL),
                Stroke::new(1.0, if is_default { c(ACCENT) } else { c(LINE) }),
            );
            p.text(
                pos2(rect.left() + 14.0, rect.center().y - 8.0),
                Align2::LEFT_CENTER,
                truncate(&s.name, 30),
                f_sb(14.0),
                c(TEXT),
            );
            p.text(
                pos2(rect.left() + 14.0, rect.center().y + 9.0),
                Align2::LEFT_CENTER,
                truncate(&s.address, 40),
                f_md(11.5),
                c(DIM),
            );
            let mut bx = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_max(
                        pos2(rect.right() - 250.0, rect.top()),
                        rect.right_bottom(),
                    ))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            bx.add_space(14.0);
            if small_button(&mut bx, "Remove", c(RED)) {
                remove = Some(i);
            }
            if is_default {
                bx.label(rt("Default", f_sb(12.5), c(ACCENT)));
            } else if small_button(&mut bx, "Set default", c(ACCENT)) {
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
            ui.add(
                egui::TextEdit::singleline(&mut app.new_server_name)
                    .hint_text("Name (optional)")
                    .desired_width(150.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut app.new_server_addr)
                    .hint_text("Address, e.g. play.example.net")
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(10.0);
        if primary_button(ui, "Add server", !app.new_server_addr.trim().is_empty()) {
            app.add_server();
        }
    });
}

/* ---------------------------------------------------------------- */
/*  Cosmetics                                                       */
/* ---------------------------------------------------------------- */

fn cosmetics_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    ui.label(rt(
        "Private cosmetics are rendered only by your DolphinClient. Nothing is uploaded and other players still see your official Mojang skin and cape.",
        f_md(14.0),
        c(DIM),
    ));
    ui.add_space(16.0);

    let Some(account) = app.accounts.active_account().cloned() else {
        card(ui, |ui| {
            section_title(ui, "No active profile");
            ui.label(rt(
                "Add or select an account before choosing cosmetics.",
                f_md(13.5),
                c(DIM),
            ));
        });
        return;
    };

    card(ui, |ui| {
        section_title(ui, &format!("{}'s local look", account.username));
        ui.horizontal(|ui| {
            let (preview, _) = ui.allocate_exact_size(vec2(90.0, 180.0), Sense::hover());
            ui.painter()
                .rect_filled(preview, Rounding::same(R_CARD), c(BG_3));
            if let Some(tex) = app.body.lock().ok().and_then(|b| b.clone()) {
                let inset = preview.shrink(12.0);
                ui.painter().image(
                    tex.id(),
                    inset,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                ui.painter().text(
                    preview.center(),
                    Align2::CENTER_CENTER,
                    initials(&account.username),
                    f_disp(24.0),
                    c(TEXT),
                );
            }
            ui.add_space(18.0);
            ui.vertical(|ui| {
                ui.add_space(18.0);
                ui.label(rt("CLIENT-SIDE PREVIEW", f_sb(11.0), c(ACCENT)));
                ui.add_space(8.0);
                ui.label(rt(
                    if account.offline {
                        "Offline profile · offline-mode servers only"
                    } else {
                        "Microsoft profile · online and offline-mode servers"
                    },
                    f_md(13.0),
                    c(DIM),
                ));
                ui.add_space(8.0);
                ui.label(rt(
                    "Skin: first person, F5, inventory and player list",
                    f_md(12.5),
                    c(FAINT),
                ));
                ui.label(rt("Cape: F5 and inventory model", f_md(12.5), c(FAINT)));
            });
        });
    });

    ui.add_space(14.0);
    card(ui, |ui| {
        section_title(ui, "Local skin");
        let skin_name = std::path::Path::new(&account.skin_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Vanilla / Mojang skin");
        ui.label(rt(
            skin_name,
            f_md(13.0),
            if account.skin_path.is_empty() {
                c(DIM)
            } else {
                c(GREEN)
            },
        ));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if primary_button(ui, "Choose skin PNG", true) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG skin", &["png"])
                    .pick_file()
                {
                    app.import_active_skin(&path);
                }
            }
            if !account.skin_path.is_empty() && ghost_button(ui, "Use Mojang skin", true) {
                app.clear_active_skin();
            }
        });
        ui.add_space(14.0);
        ui.label(rt("PLAYER MODEL", f_sb(11.0), c(FAINT)));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if small_button(
                ui,
                if account.skin_slim {
                    "Classic"
                } else {
                    "● Classic"
                },
                c(ACCENT),
            ) {
                app.set_active_skin_slim(false);
            }
            if small_button(
                ui,
                if account.skin_slim {
                    "● Slim"
                } else {
                    "Slim"
                },
                c(ACCENT),
            ) {
                app.set_active_skin_slim(true);
            }
        });
        ui.add_space(8.0);
        ui.label(rt(
            "Vanilla 64×64 and legacy 64×32 PNGs are supported, including HD multiples.",
            f_md(12.0),
            c(FAINT),
        ));
    });

    ui.add_space(14.0);
    card(ui, |ui| {
        section_title(ui, "Local cape");
        let cape_name = std::path::Path::new(&account.cape_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("No local cape");
        ui.label(rt(
            cape_name,
            f_md(13.0),
            if account.cape_path.is_empty() {
                c(DIM)
            } else {
                c(GREEN)
            },
        ));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if primary_button(ui, "Choose cape PNG", true) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG cape", &["png"])
                    .pick_file()
                {
                    app.import_active_cape(&path);
                }
            }
            if !account.cape_path.is_empty() && ghost_button(ui, "Disable local cape", true) {
                app.clear_active_cape();
            }
        });
        ui.add_space(8.0);
        ui.label(rt(
            "Uses the Vanilla 64×32 cape layout; HD multiples are downscaled pixel-perfectly.",
            f_md(12.0),
            c(FAINT),
        ));
    });
}

/* ---------------------------------------------------------------- */
/*  Accounts                                                         */
/* ---------------------------------------------------------------- */

fn accounts_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    ui.label(rt("Manage Microsoft accounts and local profiles for servers that explicitly allow offline mode.", f_md(14.0), c(DIM)));
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        if primary_button(ui, "Add Microsoft account", !app.busy) {
            app.add_microsoft(&ctx);
        }
        if ghost_button(ui, "Import account", true) {
            app.import_accounts();
        }
    });
    if let Some(note) = &app.import_note {
        ui.add_space(8.0);
        ui.label(rt(note, f_md(13.0), c(GREEN)));
    }
    ui.add_space(14.0);
    login_prompts_in(app, ui);

    card(ui, |ui| {
        section_title(ui, "Add offline profile");
        ui.label(rt(
            "No Microsoft login. This profile cannot join online-mode servers and does not bypass ownership checks.",
            f_md(12.5), c(DIM),
        ));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.offline_name)
                    .hint_text("Minecraft username")
                    .char_limit(16)
                    .desired_width(240.0),
            );
            let valid = crate::accounts::validate_offline_name(&app.offline_name).is_ok();
            if primary_button(ui, "Create offline profile", valid) {
                app.add_offline();
            }
        });
    });
    ui.add_space(14.0);

    let accounts = app.accounts.accounts.clone();
    if accounts.is_empty() {
        card(ui, |ui| {
            ui.label(rt(
                "No accounts yet. Add one, or import an existing account.",
                f_md(13.5),
                c(DIM),
            ));
        });
    }
    let mut switch_to: Option<String> = None;
    let mut remove: Option<String> = None;
    for a in &accounts {
        let is_active = app.accounts.is_active(&a.uuid);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(rect, Rounding::same(R_CARD), c(BG_2));
        p.rect_stroke(
            rect,
            Rounding::same(R_CARD),
            if is_active {
                Stroke::new(1.5, c(ACCENT))
            } else {
                Stroke::new(1.0, c(LINE))
            },
        );
        let av = Rect::from_min_size(
            pos2(rect.left() + 15.0, rect.center().y - 19.0),
            vec2(38.0, 38.0),
        );
        let tex = if is_active {
            app.avatar.lock().ok().and_then(|a| a.clone())
        } else {
            None
        };
        p.rect_filled(av, Rounding::same(R_SM), c(BG_3));
        if let Some(tex) = tex {
            p.image(
                tex.id(),
                av.shrink(3.0),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            p.text(
                av.center(),
                Align2::CENTER_CENTER,
                initials(&a.username),
                f_sb(15.0),
                c(TEXT),
            );
        }
        p.rect_stroke(av, Rounding::same(R_SM), Stroke::new(1.0, c(LINE_2)));
        p.text(
            pos2(rect.left() + 66.0, rect.center().y - 8.0),
            Align2::LEFT_CENTER,
            &a.username,
            f_sb(15.0),
            c(TEXT),
        );
        let source = if a.offline {
            format!("{} · offline servers only", a.source)
        } else {
            a.source.clone()
        };
        p.text(
            pos2(rect.left() + 66.0, rect.center().y + 9.0),
            Align2::LEFT_CENTER,
            truncate(&source, 38),
            f_md(11.0),
            c(DIM),
        );
        let mut bx = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(rect.right() - 220.0, rect.top()),
                    rect.right_bottom(),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        bx.add_space(15.0);
        if small_button(&mut bx, "Remove", c(RED)) {
            remove = Some(a.uuid.clone());
        }
        if is_active {
            bx.label(rt("● Active", f_sb(13.0), c(ACCENT)));
        } else if small_button(&mut bx, "Select", c(ACCENT)) {
            switch_to = Some(a.uuid.clone());
        }
        ui.add_space(10.0);
    }
    if let Some(uuid) = switch_to {
        app.accounts.set_active(&uuid);
        if let Some(a) = app.accounts.active_account() {
            app.status = format!("Active account: {}", a.username);
        }
    }
    if let Some(uuid) = remove {
        app.accounts.remove(&uuid);
        app.status = "Account removed.".to_string();
    }
}

/* ---------------------------------------------------------------- */
/*  Settings — launcher only                                         */
/* ---------------------------------------------------------------- */

fn settings_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    ui.label(rt("Changes are saved automatically.", f_md(14.0), c(DIM)));
    ui.add_space(16.0);
    card(ui, |ui| {
        section_title(ui, "Startup & updates");
        if toggle_row(
            ui,
            "Start with the system",
            "DolphinClient opens automatically when you sign in to your PC.",
            &mut app.settings.autostart,
        ) {
            app.settings.save();
            let _ = crate::autostart::set(app.settings.autostart);
        }
        toggle_row(
            ui,
            "Close launcher after launch",
            "Close the window once the game is running.",
            &mut app.settings.close_on_launch,
        )
        .then(|| app.settings.save());
        toggle_row(
            ui,
            "Check for updates",
            "Check for a newer version at startup.",
            &mut app.settings.auto_update,
        )
        .then(|| app.settings.save());
        toggle_row(
            ui,
            "Install updates automatically",
            "Apply available updates without asking (the launcher restarts).",
            &mut app.settings.auto_update_apply,
        )
        .then(|| app.settings.save());
    });
    ui.add_space(14.0);
    card(ui, |ui| {
        section_title(ui, "Discord");
        toggle_row(
            ui,
            "Rich Presence",
            "Show in your Discord profile that you have DolphinClient open.",
            &mut app.settings.discord_rpc,
        )
        .then(|| app.settings.save());
    });
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        if app.accounts.active_account().is_some()
            && ghost_button(ui, "Sign out active account", true)
        {
            app.remove_active();
        }
        if ghost_button(ui, "Open game folder", true) {
            let _ = open::that(config::minecraft_dir());
        }
        if ghost_button(ui, "Log", true) {
            app.show_log = !app.show_log;
        }
    });
    if app.show_log {
        ui.add_space(12.0);
        log_box(app, ui);
    }
}

/* ---------------------------------------------------------------- */
/*  Coming soon — upcoming feature areas                             */
/* ---------------------------------------------------------------- */

#[derive(Clone, Copy)]
enum SoonPage {
    Mods,
    Friends,
}

/// A clean, honest placeholder for a section that isn't ready yet: a rounded
/// gradient icon tile, the section name and simply "Coming soon". No promises,
/// no list of planned features.
fn coming_soon_view(ui: &mut egui::Ui, page: SoonPage) {
    let area = ui.max_rect();
    let p = ui.painter().clone();

    let title = match page {
        SoonPage::Mods => "Mods",
        SoonPage::Friends => "Friends",
    };

    let cx = area.center().x;
    let cy = area.center().y - 20.0;

    // Icon tile — rounded, soft accent gradient, hairline. No glow.
    let tile = Rect::from_center_size(pos2(cx, cy - 46.0), vec2(88.0, 88.0));
    p.rect_filled(tile, Rounding::same(R_CARD + 4.0), c(BG_2));
    h_gradient(&p, tile, soft(ACCENT, 34), soft(ACCENT_2, 26));
    p.rect_stroke(
        tile,
        Rounding::same(R_CARD + 4.0),
        Stroke::new(1.0, soft(ACCENT, 90)),
    );
    soon_hero_icon(&p, page, tile.center());

    // Section name.
    p.text(
        pos2(cx, cy + 24.0),
        Align2::CENTER_TOP,
        title,
        f_disp(28.0),
        c(TEXT),
    );

    // "Coming soon" pill — that's all, no roadmap.
    let pill_txt = "COMING SOON";
    let pg = p.layout_no_wrap(pill_txt.to_string(), f_sb(11.5), c(ACCENT));
    let pill = Rect::from_center_size(pos2(cx, cy + 78.0), vec2(pg.size().x + 30.0, 26.0));
    p.rect_filled(pill, Rounding::same(13.0), soft(ACCENT, 24));
    p.rect_stroke(
        pill,
        Rounding::same(13.0),
        Stroke::new(1.0, soft(ACCENT, 80)),
    );
    p.text(
        pill.center(),
        Align2::CENTER_CENTER,
        pill_txt,
        f_sb(11.5),
        c(ACCENT),
    );
}

/// A larger hand-drawn icon for the coming-soon tile.
fn soon_hero_icon(p: &egui::Painter, page: SoonPage, ctr: egui::Pos2) {
    let col = c(TEXT);
    match page {
        SoonPage::Mods => {
            // 2×2 grid of rounded modules; one accent-filled.
            let sz = 13.0;
            for (i, (dx, dy)) in [
                (-15.0_f32, -15.0_f32),
                (2.0, -15.0),
                (-15.0, 2.0),
                (2.0, 2.0),
            ]
            .iter()
            .enumerate()
            {
                let r = Rect::from_min_size(pos2(ctr.x + dx, ctr.y + dy), vec2(sz, sz));
                if i == 3 {
                    p.rect_filled(r, Rounding::same(3.0), col);
                } else {
                    p.rect_stroke(r, Rounding::same(3.0), Stroke::new(2.0, col));
                }
            }
        }
        SoonPage::Friends => {
            let s = Stroke::new(2.0, col);
            // Back person.
            p.circle_stroke(ctr + vec2(9.0, -7.0), 5.5, s);
            let b1 = Rect::from_min_max(ctr + vec2(1.0, 1.0), ctr + vec2(17.0, 15.0));
            p.add(Shape::line(
                vec![
                    pos2(b1.left(), b1.bottom()),
                    pos2(b1.left(), b1.top()),
                    pos2(b1.right(), b1.top()),
                    pos2(b1.right(), b1.bottom()),
                ],
                s,
            ));
            // Front person.
            p.circle_stroke(ctr + vec2(-8.0, -5.0), 6.5, s);
            let b2 = Rect::from_min_max(ctr + vec2(-17.0, 3.0), ctr + vec2(1.0, 17.0));
            p.add(Shape::line(
                vec![
                    pos2(b2.left(), b2.bottom()),
                    pos2(b2.left(), b2.top()),
                    pos2(b2.right(), b2.top()),
                    pos2(b2.right(), b2.bottom()),
                ],
                s,
            ));
        }
    }
}

/* ---------------------------------------------------------------- */
/*  Login prompts                                                    */
/* ---------------------------------------------------------------- */

fn login_prompts_in(app: &mut DolphinApp, ui: &mut egui::Ui) {
    if let Some((link, code)) = app.device.clone() {
        card(ui, |ui| {
            ui.label(rt("Sign in via browser", f_sb(14.0), c(TEXT)));
            ui.label(rt(
                "A browser window has opened — sign in there.",
                f_md(12.5),
                c(DIM),
            ));
            ui.add_space(6.0);
            ui.label(rt(&code, f_disp(24.0), c(ACCENT)));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ghost_button(ui, "Open link", true) {
                    let _ = open::that(&link);
                }
                if ghost_button(ui, "Copy code", true) {
                    ui.output_mut(|o| o.copied_text = code.clone());
                }
            });
        });
        ui.add_space(12.0);
    }
    if let Some(url) = app.auth_url.clone() {
        card(ui, |ui| {
            ui.label(rt(
                "A browser window has opened — sign in there.",
                f_md(12.5),
                c(DIM),
            ));
            ui.add_space(8.0);
            if ghost_button(ui, "Reopen browser", true) {
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
            .max_rect(Rect::from_min_max(
                pos2(x, area.top() + 20.0),
                pos2(x + col_w, area.bottom() - 20.0),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    login_prompts_in(app, &mut cui);
}

/* ---------------------------------------------------------------- */
/*  Custom widgets                                                   */
/* ---------------------------------------------------------------- */

fn slider_row(
    ui: &mut egui::Ui,
    title: &str,
    value: &str,
    val: &mut i32,
    min: i32,
    max: i32,
) -> bool {
    slider_row_step(ui, title, value, val, min, max, 1)
}

fn slider_row_step(
    ui: &mut egui::Ui,
    title: &str,
    value: &str,
    val: &mut i32,
    min: i32,
    max: i32,
    step: i32,
) -> bool {
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

/// A custom horizontal slider (track + accent fill + white knob). No glow.
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
    p.rect_filled(
        Rect::from_min_max(pos2(x0, cy - 3.0), pos2(x1, cy + 3.0)),
        Rounding::same(3.0),
        c(BG_4),
    );
    // Fill (gradient).
    if kx > x0 + 1.0 {
        h_gradient(
            p,
            Rect::from_min_max(pos2(x0, cy - 3.0), pos2(kx, cy + 3.0)),
            c(ACCENT_2),
            c(ACCENT),
        );
    }
    // Knob.
    let hov = resp.hovered() || resp.dragged();
    p.circle_filled(pos2(kx, cy), if hov { 9.5 } else { 8.5 }, Color32::WHITE);
    p.circle_stroke(
        pos2(kx, cy),
        if hov { 9.5 } else { 8.5 },
        Stroke::new(1.5, c(ACCENT)),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    changed
}

fn segmented(ui: &mut egui::Ui, options: &[&str], selected: usize) -> Option<usize> {
    let h = 38.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, Rounding::same(R_CTL), c(BG_3));
    p.rect_stroke(rect, Rounding::same(R_CTL), Stroke::new(1.0, c(LINE)));
    let n = options.len().max(1);
    let seg_w = rect.width() / n as f32;
    let mut chosen = None;
    for (i, label) in options.iter().enumerate() {
        let sr = Rect::from_min_size(
            pos2(rect.left() + seg_w * i as f32, rect.top()),
            vec2(seg_w, h),
        );
        let resp = ui.interact(sr, ui.id().with(("seg", label, i)), Sense::click());
        let is_sel = i == selected;
        if is_sel {
            p.rect_filled(sr.shrink(4.0), Rounding::same(9.0), soft(ACCENT, 40));
            p.rect_stroke(
                sr.shrink(4.0),
                Rounding::same(9.0),
                Stroke::new(1.0, c(ACCENT)),
            );
        } else if resp.hovered() {
            p.rect_filled(sr.shrink(4.0), Rounding::same(9.0), soft(LINE_2, 40));
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
        ui.painter()
            .rect_stroke(rect, radius, Stroke::new(1.0, c(LINE_2)));
    }
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    ui.painter()
        .circle_filled(pos2(knob_x, rect.center().y), radius - 4.0, Color32::WHITE);
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
            ui.label(rt("Play history", f_disp(16.0), c(TEXT)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(rt(
                    &format!("last {} sessions", stats.sessions.len()),
                    f_md(12.0),
                    c(DIM),
                ));
            });
        });
        ui.add_space(14.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
        let p = ui.painter();
        let max = stats
            .sessions
            .iter()
            .map(|s| s.secs)
            .max()
            .unwrap_or(1)
            .max(1) as f32;
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
        .rounding(Rounding::same(R_CARD))
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if app.log.is_empty() {
                        ui.label(rt("— no output yet —", f_md(12.0), c(FAINT)));
                    }
                    for line in &app.log {
                        ui.label(
                            egui::RichText::new(line)
                                .monospace()
                                .size(11.5)
                                .color(c(DIM)),
                        );
                    }
                });
        });
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(soft(BG_1, 245))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(R_CARD))
        .shadow(egui::epaint::Shadow {
            offset: vec2(0.0, 6.0),
            blur: 22.0,
            spread: 0.0,
            color: Color32::from_black_alpha(34),
        })
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
        .rounding(Rounding::same(R_CARD))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn primary_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(label.len() as f32 * 8.6 + 40.0, 42.0), Sense::click());
    let hov = enabled && resp.hovered();
    let p = ui.painter();
    let rounding = Rounding::same(R_CTL);
    if enabled {
        let (l, r) = if hov {
            (
                lerp_color(c(ACCENT), Color32::WHITE, 0.12),
                lerp_color(c(ACCENT_2), Color32::WHITE, 0.06),
            )
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
    let (rect, resp) =
        ui.allocate_exact_size(vec2(label.len() as f32 * 8.2 + 34.0, 42.0), Sense::click());
    let hov = enabled && resp.hovered();
    let p = ui.painter();
    let rounding = Rounding::same(R_CTL);
    p.rect_filled(rect, rounding, if hov { c(BG_3) } else { c(BG_2) });
    p.rect_stroke(
        rect,
        rounding,
        Stroke::new(1.0, if hov { c(LINE_2) } else { c(LINE) }),
    );
    let col = if enabled { c(TEXT) } else { c(FAINT) };
    p.text(rect.center(), Align2::CENTER_CENTER, label, f_sb(14.0), col);
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked() && enabled
}

fn small_button(ui: &mut egui::Ui, label: &str, tint: Color32) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(label.len() as f32 * 7.4 + 22.0, 32.0), Sense::click());
    let hov = resp.hovered();
    let p = ui.painter();
    p.rect_filled(
        rect,
        Rounding::same(R_SM),
        if hov { c(BG_4) } else { c(BG_3) },
    );
    p.rect_stroke(rect, Rounding::same(R_SM), Stroke::new(1.0, c(LINE)));
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        f_sb(12.5),
        tint,
    );
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn draw_avatar(painter: &egui::Painter, app: &DolphinApp, rect: Rect) {
    painter.rect_filled(rect, Rounding::same(R_SM), c(BG_3));
    let tex = app.avatar.lock().ok().and_then(|a| a.clone());
    match (tex, app.accounts.active_account()) {
        (Some(tex), Some(_)) => {
            painter.image(
                tex.id(),
                rect.shrink(3.0),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        (_, Some(a)) => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                initials(&a.username),
                f_sb(14.0),
                c(TEXT),
            );
        }
        _ => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "?",
                f_sb(14.0),
                c(DIM),
            );
        }
    }
    painter.rect_stroke(rect, Rounding::same(R_SM), Stroke::new(1.0, c(LINE_2)));
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
    let Some(at) = last else {
        return "—".to_string();
    };
    let now = config::now_unix();
    if now <= at {
        return "just now".to_string();
    }
    let d = now - at;
    if d < 60 {
        "just now".to_string()
    } else if d < 3600 {
        format!("{} min ago", d / 60)
    } else if d < 86_400 {
        format!("{} h ago", d / 3600)
    } else if d < 7 * 86_400 {
        let days = d / 86_400;
        if days == 1 {
            "yesterday".to_string()
        } else {
            format!("{days} d ago")
        }
    } else {
        format!("{} w ago", d / (7 * 86_400))
    }
}
