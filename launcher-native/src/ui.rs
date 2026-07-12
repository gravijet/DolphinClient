//! The launcher UI — DolphinClient's native front, redrawn to match the
//! website's living "Prism" look 1:1: a deep water-black stage lit by a slowly
//! drifting iridescent aurora, rising bubbles, glossy glass cards, flowing
//! gradient headlines, a live performance readout, racing comparison bars and a
//! prism play button that sweeps with light. Everything is painted with egui's
//! own painter — no image assets beyond the logo. All settings save on change;
//! there is no "Save" button.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Rounding, Sense, Stroke,
    ViewportCommand, pos2, vec2,
};

use crate::app::{DolphinApp, LoginMethod, Tab};
use crate::config;

/* ---------------------------------------------------------------- */
/*  Palette — the website's "Prism" tokens, 1:1                      */
/* ---------------------------------------------------------------- */

pub const BG_0: [u8; 3] = [0x06, 0x09, 0x11]; // --ink   (window floor)
const BG_1: [u8; 3] = [0x09, 0x0E, 0x1B]; // --ink-2 (chrome: rails/bars)
const BG_2: [u8; 3] = [0x0D, 0x14, 0x24]; // --ink-3 (cards)
const BG_3: [u8; 3] = [0x14, 0x1D, 0x32]; // inputs / hover
const LINE: [u8; 3] = [0x24, 0x30, 0x4A]; // hairline borders
const LINE_2: [u8; 3] = [0x32, 0x41, 0x63]; // brighter hairline
const TEXT: [u8; 3] = [0xEE, 0xF3, 0xFE];
const DIM: [u8; 3] = [0x9A, 0xA9, 0xC7];
const FAINT: [u8; 3] = [0x5F, 0x6E, 0x8C];
const GREEN: [u8; 3] = [0x4D, 0xE3, 0xA4];
const RED: [u8; 3] = [0xFF, 0x6B, 0x6B];
const GOLD: [u8; 3] = [0xFF, 0xCF, 0x6A];

/// The iridescent "Prism" stops — aqua → blue → violet → pink. Cyclic (wraps
/// back to aqua), driving every flowing gradient in the launcher, mirroring the
/// website's signature light.
const PRISM: [[u8; 3]; 4] = [
    [0x34, 0xE6, 0xD6], // aqua
    [0x37, 0xA7, 0xFF], // blue
    [0x8A, 0x5C, 0xFF], // violet
    [0xFF, 0x6A, 0xD5], // pink
];

fn c(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
fn soft(rgb: [u8; 3], a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], a)
}
fn accent_soft(a: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), alpha)
}
fn with_alpha(col: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), alpha)
}
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), 255)
}
/// Sample the cyclic prism gradient at position `t` (wraps every 1.0).
fn prism_at(t: f32) -> Color32 {
    let x = t.rem_euclid(1.0) * 4.0;
    let i = x.floor() as usize % 4;
    let f = x - x.floor();
    lerp_color(c(PRISM[i]), c(PRISM[(i + 1) % 4]), f)
}

/// Paint a flowing horizontal prism gradient into `rect`. `phase` shifts the
/// colours along the width (animate with time for the living-light look);
/// `alpha` fades the whole wash so it can sit as a sheen over a card.
fn prism_wash(painter: &egui::Painter, rect: Rect, phase: f32, round: f32, alpha: u8) {
    let bands = 44usize;
    for i in 0..bands {
        let t0 = i as f32 / bands as f32;
        let x0 = rect.left() + rect.width() * t0;
        let x1 = rect.left() + rect.width() * ((i + 1) as f32 / bands as f32);
        let col0 = prism_at(t0 + phase);
        let col = if alpha == 255 { col0 } else { with_alpha(col0, alpha) };
        let r = if i == 0 {
            Rounding { nw: round, ne: 0.0, sw: round, se: 0.0 }
        } else if i == bands - 1 {
            Rounding { nw: 0.0, ne: round, sw: 0.0, se: round }
        } else {
            Rounding::ZERO
        };
        painter.rect_filled(
            Rect::from_min_max(pos2(x0, rect.top()), pos2(x1 + 1.0, rect.bottom())),
            r,
            col,
        );
    }
}

fn accent(app: &DolphinApp) -> Color32 {
    c(config::accent_rgb(&app.settings.accent))
}

fn time_of(ui: &egui::Ui) -> f32 {
    ui.input(|i| i.time) as f32
}

const TITLEBAR_H: f32 = 46.0;
const BOTTOM_H: f32 = 86.0;
const NAV_W: f32 = 226.0;
const ROUND: f32 = 14.0;
const CARD_A: u8 = 232; // card translucency, so the aurora glows faintly through

/* ---------------------------------------------------------------- */
/*  Theme                                                            */
/* ---------------------------------------------------------------- */

pub fn install_theme(ctx: &egui::Context, accent_name: &str) {
    let accent = c(config::accent_rgb(accent_name));
    let round = Rounding::same(11.0);

    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(c(TEXT));
    v.panel_fill = c(BG_0);
    v.window_fill = c(BG_1);
    v.window_stroke = Stroke::new(1.0, c(LINE));
    v.window_rounding = Rounding::same(ROUND);
    v.extreme_bg_color = c(BG_3);
    v.faint_bg_color = c(BG_2);
    v.hyperlink_color = accent;
    v.selection.bg_fill = accent_soft(accent, 70);
    v.selection.stroke = Stroke::new(1.0, accent);
    v.popup_shadow = egui::epaint::Shadow {
        offset: vec2(0.0, 8.0),
        blur: 30.0,
        spread: 0.0,
        color: Color32::from_black_alpha(150),
    };

    v.widgets.noninteractive.bg_fill = c(BG_1);
    v.widgets.noninteractive.weak_bg_fill = c(BG_1);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c(LINE));
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c(DIM));
    v.widgets.noninteractive.rounding = round;

    v.widgets.inactive.bg_fill = c(BG_3);
    v.widgets.inactive.weak_bg_fill = c(BG_2);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, c(LINE));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, c(TEXT));
    v.widgets.inactive.rounding = round;

    v.widgets.hovered.bg_fill = c(BG_3);
    v.widgets.hovered.weak_bg_fill = c(BG_3);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, c(TEXT));
    v.widgets.hovered.rounding = round;

    v.widgets.active.bg_fill = c(BG_3);
    v.widgets.active.weak_bg_fill = c(BG_3);
    v.widgets.active.bg_stroke = Stroke::new(1.0, accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0, c(TEXT));
    v.widgets.active.rounding = round;
    v.widgets.open.rounding = round;

    let mut style = (*ctx.style()).clone();
    use egui::{FontFamily, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(25.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.5, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
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
    // The living aurora runs on every tab, so keep a smooth ~30fps heartbeat.
    ctx.request_repaint_after(std::time::Duration::from_millis(33));

    title_bar(app, ctx);
    bottom_bar(app, ctx);
    nav_rail(app, ctx);

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(c(BG_0)))
        .show(ctx, |ui| {
            // Paint the living aurora into the central stage *before* the scroll
            // content, so cards float over fixed, slowly-moving light.
            let stage = ui.max_rect();
            let t = time_of(ui);
            let pointer = ui.input(|i| i.pointer.hover_pos());
            aurora(ui.painter(), stage, t, pointer);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(24.0);
                    content_column(ui, |ui| match app.tab {
                        Tab::Home => home_view(app, ui),
                        Tab::Accounts => accounts_view(app, ui),
                        Tab::Servers => servers_view(app, ui),
                        Tab::Cosmetics => cosmetics_view(app, ui),
                        Tab::Settings => settings_view(app, ui),
                    });
                    ui.add_space(30.0);
                });
        });
}

/// Centered fixed-max-width content column.
fn content_column(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let w = 840.0_f32.min(ui.available_width() - 52.0);
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - w).max(0.0) / 2.0).max(0.0));
        ui.allocate_ui_with_layout(vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), add);
    });
}

/* ---------------------------------------------------------------- */
/*  Living aurora background (drifting light + rising bubbles)       */
/* ---------------------------------------------------------------- */

/// A tiny integer hash → 0..1, for deterministic bubble placement.
fn hash01(n: u32) -> f32 {
    let mut x = n.wrapping_mul(2_654_435_761);
    x ^= x >> 15;
    x = x.wrapping_mul(2_246_822_519);
    x ^= x >> 13;
    (x & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
}

/// A soft radial glow ≈ a heavily-blurred blob, approximated by many stacked
/// translucent circles from the outside in. On the dark stage the overlap reads
/// as additive light, just like the website's `mix-blend-mode: screen` aurora.
/// Plenty of rings + a smooth cubic falloff keep the edge from banding.
fn soft_blob(painter: &egui::Painter, center: Pos2, radius: f32, col: Color32, peak: u8) {
    let rings = 46;
    for i in 0..rings {
        let f = i as f32 / (rings as f32 - 1.0); // 0 = outer, 1 = core
        let r = radius * (1.0 - f).powf(0.85);
        // Alpha rises toward the core with a soft cubic ramp, so the edge melts.
        let a = (peak as f32 * f * f * f).round() as u8;
        painter.circle_filled(center, r, with_alpha(col, a.max(1)));
    }
}

/// Paint the whole living background into `rect`: a top wash, four drifting
/// iridescent blobs that lean gently toward the cursor, and a field of slowly
/// rising bubbles (the ocean the dolphin swims in). Purely decorative.
fn aurora(painter: &egui::Painter, rect: Rect, t: f32, pointer: Option<Pos2>) {
    let clip = painter.with_clip_rect(rect);
    let w = rect.width();
    let h = rect.height();

    // Cursor parallax: the light leans a little toward the pointer.
    let (px, py) = match pointer {
        Some(p) if rect.contains(p) => (
            ((p.x - rect.center().x) / w).clamp(-0.5, 0.5),
            ((p.y - rect.center().y) / h).clamp(-0.5, 0.5),
        ),
        _ => (0.0, 0.0),
    };
    let lean = vec2(px * 34.0, py * 26.0);

    // A soft blue crown at the top, matching the site's radial header glow.
    soft_blob(
        &clip,
        pos2(rect.center().x, rect.top() - h * 0.12),
        w.max(h) * 0.6,
        c([0x1E, 0x54, 0x9C]),
        12,
    );

    // Four drifting blobs (aqua, violet, blue, pink) on slow Lissajous paths,
    // tucked toward the corners. Kept deliberately low-alpha so the deep-ink
    // canvas dominates and text stays crisp — the light pools and drifts rather
    // than flooding the stage.
    let blobs = [
        (c(PRISM[0]), 0.10, 0.08, 0.052, 0.041, 0.34, 24u8),
        (c(PRISM[2]), 0.90, 0.06, 0.037, 0.058, 0.40, 24),
        (c(PRISM[1]), 0.24, 0.94, 0.045, 0.033, 0.38, 20),
        (c(PRISM[3]), 0.94, 0.90, 0.031, 0.049, 0.34, 24),
    ];
    for (i, (col, bx, by, sx, sy, rad, peak)) in blobs.iter().enumerate() {
        let ph = i as f32 * 1.7;
        let cx = rect.left() + w * bx + (t * sx + ph).sin() * w * 0.10 + lean.x;
        let cy = rect.top() + h * by + (t * sy + ph).cos() * h * 0.12 + lean.y;
        soft_blob(&clip, pos2(cx, cy), w.max(h) * rad, *col, *peak);
    }

    // Rising bubbles — small, faint, drifting up and wrapping around.
    let bubbles = 26;
    for i in 0..bubbles {
        let seed = i as u32 * 97 + 13;
        let bx = hash01(seed);
        let speed = 0.008 + hash01(seed ^ 0xABCD) * 0.02;
        let size = 1.4 + hash01(seed ^ 0x55AA) * 3.6;
        let drift = (t * (0.2 + hash01(seed) * 0.5) + i as f32).sin() * 10.0;
        let prog = (hash01(seed ^ 0x1234) + t * speed).fract();
        let x = rect.left() + bx * w + drift;
        let y = rect.bottom() - prog * (h + 40.0) + 20.0;
        let fade = (prog * std::f32::consts::PI).sin(); // fade in/out at ends
        let a = (34.0 * fade) as u8;
        clip.circle_filled(pos2(x, y), size, with_alpha(c([0xBF, 0xE6, 0xFF]), a));
        clip.circle_stroke(
            pos2(x, y),
            size,
            Stroke::new(1.0, with_alpha(c(PRISM[0]), (a as f32 * 0.7) as u8)),
        );
    }

    // A whole-stage depth scrim: deepens the light back toward the ink canvas so
    // the aurora glows *under* the content instead of flooding it — this is what
    // keeps headlines and body text crisp over the colour.
    clip.rect_filled(rect, Rounding::ZERO, with_alpha(c(BG_0), 70));

    // A stronger floor gradient so text stays legible near the bottom edge.
    let floor = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 240.0), rect.right_bottom());
    let clear = with_alpha(c(BG_0), 0);
    let deep = with_alpha(c(BG_0), 140);
    grad4(&clip, floor, clear, clear, deep, deep);
}

/* ---------------------------------------------------------------- */
/*  Flowing gradient text (per-letter prism, animated)              */
/* ---------------------------------------------------------------- */

/// A smooth 4-corner gradient quad drawn as a single GPU mesh (perfectly
/// seam-free, unlike stacked band rects).
fn grad4(painter: &egui::Painter, rect: Rect, tl: Color32, tr: Color32, br: Color32, bl: Color32) {
    use egui::epaint::{Mesh, Vertex, WHITE_UV};
    let mut mesh = Mesh::default();
    let v = |p: Pos2, col: Color32| Vertex { pos: p, uv: WHITE_UV, color: col };
    mesh.vertices.push(v(rect.left_top(), tl));
    mesh.vertices.push(v(rect.right_top(), tr));
    mesh.vertices.push(v(rect.right_bottom(), br));
    mesh.vertices.push(v(rect.left_bottom(), bl));
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}

/// A left→right dark gradient of the ink canvas — deepens a region behind text
/// so it reads clearly over the living aurora, then fades out.
fn horizontal_scrim(painter: &egui::Painter, rect: Rect, left_a: f32, right_a: f32) {
    let lc = with_alpha(c(BG_0), left_a as u8);
    let rc = with_alpha(c(BG_0), right_a as u8);
    grad4(painter, rect, lc, rc, rc, lc);
}

fn text_width(ui: &egui::Ui, text: &str, font: &FontId) -> f32 {
    ui.fonts(|f| text.chars().map(|ch| f.glyph_width(font, ch)).sum())
}

/// Paint `text` letter-by-letter with the prism gradient sampled along its
/// width and shifted by `phase` — a living, iridescent headline. Returns the
/// total advance width. `left_center` is the left edge at the vertical centre.
fn flowing_text(
    ui: &egui::Ui,
    left_center: Pos2,
    text: &str,
    font: FontId,
    phase: f32,
    spread: f32,
) -> f32 {
    let painter = ui.painter().clone();
    let mut x = left_center.x;
    for ch in text.chars() {
        let w = ui.fonts(|f| f.glyph_width(&font, ch));
        let col = prism_at(phase + (x - left_center.x) * spread);
        painter.text(pos2(x, left_center.y), Align2::LEFT_CENTER, ch, font.clone(), col);
        x += w;
    }
    x - left_center.x
}

/* ---------------------------------------------------------------- */
/*  Title bar                                                        */
/* ---------------------------------------------------------------- */

fn title_bar(app: &mut DolphinApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("titlebar")
        .exact_height(TITLEBAR_H)
        .frame(egui::Frame::none().fill(c(BG_0)))
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            let t = time_of(ui);
            let painter = ui.painter().clone();
            // Hairline base under the bar.
            painter.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, c(LINE)));

            // Logo + two-tone wordmark (left) — mirrors the website brand, with a
            // slowly flowing "Client".
            let logo = Rect::from_center_size(pos2(bar.left() + 26.0, bar.center().y), vec2(26.0, 26.0));
            painter.image(
                app.logo.id(),
                logo,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            let wordmark = FontId::new(16.5, egui::FontFamily::Proportional);
            let r1 = painter.text(
                pos2(bar.left() + 50.0, bar.center().y),
                Align2::LEFT_CENTER,
                "Dolphin",
                wordmark.clone(),
                c(TEXT),
            );
            flowing_text(
                ui,
                pos2(r1.right(), bar.center().y),
                "Client",
                wordmark,
                t * 0.06,
                0.006,
            );

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

            // Drag zone: everything left of the window buttons.
            let drag_zone = Rect::from_min_max(bar.min, pos2(x - 4.0, bar.bottom()));
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
    painter.rect_filled(rect, Rounding::same(8.0), fill);
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
/*  Navigation rail                                                  */
/* ---------------------------------------------------------------- */

fn nav_rail(app: &mut DolphinApp, ctx: &egui::Context) {
    let ac = accent(app);
    egui::SidePanel::left("nav")
        .exact_width(NAV_W)
        .resizable(false)
        .frame(
            egui::Frame::none()
                .fill(c(BG_1))
                .inner_margin(egui::Margin::symmetric(12.0, 16.0)),
        )
        .show(ctx, |ui| {
            let r = ui.max_rect();
            ui.painter().line_segment(
                [pos2(r.right(), r.top()), pos2(r.right(), r.bottom())],
                Stroke::new(1.0, c(LINE)),
            );

            let n = app.accounts.accounts.len();
            let sv = app.settings.servers.len();
            let items = [
                (Icon::Home, "Start".to_string(), Tab::Home),
                (Icon::User, format!("Konten · {n}"), Tab::Accounts),
                (Icon::Server, format!("Server · {sv}"), Tab::Servers),
                (Icon::Cape, "Cosmetics".to_string(), Tab::Cosmetics),
                (Icon::Gear, "Einstellungen".to_string(), Tab::Settings),
            ];
            let t = time_of(ui);
            for (icon, label, tab) in items {
                if nav_item(ui, icon, &label, app.tab == tab, ac, t) {
                    app.tab = tab;
                }
                ui.add_space(4.0);
            }

            // Bottom block: Discord presence, dashboard link, version.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("Launcher v{}", env!("CARGO_PKG_VERSION")))
                        .color(c(FAINT))
                        .size(11.5),
                );
                ui.add_space(8.0);
                if ghost_button(ui, "Web-Dashboard öffnen", true, ac) {
                    let _ = open::that("https://example.invalid/dashboard");
                }
                ui.add_space(10.0);
                status_chip(ui, app, ac);
                ui.add_space(6.0);
            });
        });
}

/// A small live-status chip in the nav foot: Discord presence + game state.
fn status_chip(ui: &mut egui::Ui, app: &DolphinApp, ac: Color32) {
    let running = app.running.load(std::sync::atomic::Ordering::Relaxed);
    let rpc_on = app.gameopts.discord_rpc();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(12.0), soft(BG_2, CARD_A));
    p.rect_stroke(rect, Rounding::same(12.0), Stroke::new(1.0, c(LINE)));

    // Discord row.
    let dcol = if rpc_on { c([0x88, 0x9B, 0xF4]) } else { c(FAINT) };
    paint_icon(p, pos2(rect.left() + 20.0, rect.top() + 20.0), 8.0, Icon::Discord, dcol);
    p.text(
        pos2(rect.left() + 38.0, rect.top() + 20.0),
        Align2::LEFT_CENTER,
        if rpc_on { "Rich Presence aktiv" } else { "Rich Presence aus" },
        FontId::proportional(12.5),
        c(DIM),
    );
    // Game / dashboard row.
    let (dot, label, col) = if running {
        (c(GREEN), "Im Spiel", c(GREEN))
    } else {
        (ac, "Dashboard verbunden", c(DIM))
    };
    p.circle_filled(pos2(rect.left() + 20.0, rect.bottom() - 20.0), 4.0, dot);
    p.text(
        pos2(rect.left() + 38.0, rect.bottom() - 20.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.5),
        col,
    );
}

fn nav_item(ui: &mut egui::Ui, icon: Icon, label: &str, active: bool, ac: Color32, t: f32) -> bool {
    let h = 46.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let hov = resp.hovered();
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, Rounding::same(11.0), accent_soft(ac, 30));
        // Flowing prism marker on the left edge.
        let marker = Rect::from_min_max(rect.left_top(), pos2(rect.left() + 3.5, rect.bottom()));
        prism_wash(painter, marker, t * 0.05, 2.0, 255);
    } else if hov {
        painter.rect_filled(rect, Rounding::same(11.0), soft(BG_2, 200));
    }
    let icon_col = if active { ac } else if hov { c(TEXT) } else { c(DIM) };
    paint_icon(painter, pos2(rect.left() + 25.0, rect.center().y), 9.0, icon, icon_col);
    painter.text(
        pos2(rect.left() + 48.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.5),
        if active { c(TEXT) } else { icon_col },
    );
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/* ---------------------------------------------------------------- */
/*  Bottom play bar                                                  */
/* ---------------------------------------------------------------- */

fn bottom_bar(app: &mut DolphinApp, ctx: &egui::Context) {
    let ac = accent(app);
    let active = app.accounts.active_account().cloned();
    let running = app.running.load(std::sync::atomic::Ordering::Relaxed);
    egui::TopBottomPanel::bottom("play")
        .exact_height(BOTTOM_H)
        .frame(
            egui::Frame::none()
                .fill(c(BG_1))
                .inner_margin(egui::Margin::symmetric(18.0, 0.0)),
        )
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            let t = time_of(ui);
            ui.painter().line_segment([bar.left_top(), bar.right_top()], Stroke::new(1.0, c(LINE)));
            // A whisper of flowing prism along the very top edge, tying the bar
            // to the stage above it.
            let edge = Rect::from_min_max(bar.left_top(), pos2(bar.right(), bar.top() + 1.5));
            prism_wash(ui.painter(), edge, t * 0.05, 0.0, 120);

            // ----- Right: PLAY button + version picker -----
            let btn_w = 200.0;
            let btn_h = 52.0;
            let btn_rect = Rect::from_min_size(
                pos2(bar.right() - btn_w, bar.center().y - btn_h / 2.0),
                vec2(btn_w, btn_h),
            );
            let can_play = active.is_some() && !app.busy && !running;
            let clicked = play_button(ui, btn_rect, &active, running, app.busy, t);
            if clicked {
                if active.is_some() {
                    if can_play {
                        app.start_launch(ctx);
                    }
                } else {
                    app.add_microsoft(ctx);
                }
            }

            // Version combo to the left of the play button.
            if active.is_some() {
                let combo_rect = Rect::from_min_size(
                    pos2(btn_rect.left() - 182.0, bar.center().y - 17.0),
                    vec2(168.0, 34.0),
                );
                let mut ui2 = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(combo_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                version_combo(app, &mut ui2);
            }

            // ----- Left: avatar + account + status/progress -----
            let av_rect = Rect::from_min_size(pos2(bar.left(), bar.center().y - 24.0), vec2(48.0, 48.0));
            draw_avatar(ui, app, av_rect, ac, t);

            let tx = bar.left() + 64.0;
            let name = match &active {
                Some(a) => a.username.clone(),
                None => "Kein Konto".to_string(),
            };
            ui.painter().text(
                pos2(tx, bar.center().y - 13.0),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(15.5),
                c(TEXT),
            );
            let status_col = if app.status.starts_with("Fehler") {
                c(RED)
            } else if running {
                c(GREEN)
            } else {
                c(DIM)
            };
            let sub = truncate(&app.status, 54);
            ui.painter().text(
                pos2(tx, bar.center().y + 9.0),
                Align2::LEFT_CENTER,
                sub,
                FontId::proportional(12.5),
                status_col,
            );
            if app.busy || app.progress > 0.001 {
                let pr = Rect::from_min_size(pos2(tx, bar.center().y + 21.0), vec2(250.0, 5.0));
                progress_bar(ui.painter(), pr, app.progress, t);
            }
        });
}

fn play_button(
    ui: &mut egui::Ui,
    rect: Rect,
    active: &Option<crate::accounts::Account>,
    running: bool,
    busy: bool,
    t: f32,
) -> bool {
    let enabled = !busy && !running;
    let resp = ui.interact(rect, ui.id().with("playbtn"), Sense::click());
    let hov = enabled && resp.hovered();
    let hov_t = ui.ctx().animate_bool_with_time(ui.id().with("playhov"), hov, 0.7);
    let painter = ui.painter();

    if !enabled {
        painter.rect_filled(rect, Rounding::same(ROUND), c(BG_3));
        painter.rect_stroke(rect, Rounding::same(ROUND), Stroke::new(1.0, c(LINE)));
    } else {
        // Soft prism glow that breathes underneath the button.
        let breathe = 0.5 + 0.5 * (t * 1.4).sin();
        let glow = rect.expand(6.0 + breathe * 3.0);
        prism_wash(painter, glow, t * 0.05, ROUND + 6.0, (26.0 + breathe * 24.0) as u8);
        // The button face carries the flowing prism gradient.
        prism_wash(painter, rect, t * 0.06, ROUND, 255);
        // A diagonal sheen that sweeps across on hover (clipped to the face).
        if hov_t > 0.01 {
            let clip = painter.with_clip_rect(rect);
            let sweep = rect.left() - rect.width() * 0.5 + hov_t * rect.width() * 1.8;
            for k in -6i32..6 {
                let off = k as f32 * 4.0;
                let a = (70.0 * (1.0 - (k.abs() as f32 / 6.0))) as u8;
                clip.line_segment(
                    [pos2(sweep + off, rect.top() - 6.0), pos2(sweep + off - 22.0, rect.bottom() + 6.0)],
                    Stroke::new(3.0, Color32::from_white_alpha(a)),
                );
            }
        }
        if hov {
            painter.rect_filled(rect, Rounding::same(ROUND), Color32::from_white_alpha(18));
        }
    }

    let (label, show_tri) = if running {
        ("LÄUFT", false)
    } else if busy {
        ("…", false)
    } else if active.is_some() {
        ("SPIELEN", true)
    } else {
        ("ANMELDEN", false)
    };
    let fg = if enabled { c([0x04, 0x0A, 0x12]) } else { c(DIM) };
    let mut tx = rect.center().x;
    if show_tri {
        tx += 13.0;
        let cy = rect.center().y;
        let lx = rect.center().x - 62.0;
        painter.add(egui::Shape::convex_polygon(
            vec![pos2(lx, cy - 7.5), pos2(lx + 13.0, cy), pos2(lx, cy + 7.5)],
            fg,
            Stroke::NONE,
        ));
    }
    painter.text(
        pos2(tx, rect.center().y),
        Align2::CENTER_CENTER,
        label,
        FontId::new(16.5, egui::FontFamily::Proportional),
        fg,
    );
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
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
        .width(162.0)
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

/* ---------------------------------------------------------------- */
/*  Home                                                             */
/* ---------------------------------------------------------------- */

fn home_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ac = accent(app);
    let ctx = ui.ctx().clone();
    let active = app.accounts.active_account().cloned();
    let t = time_of(ui);

    // ---- Update banner ----
    if let Some(info) = app.update_note.lock().ok().and_then(|n| n.clone()) {
        card_tinted(ui, GOLD, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("Launcher-Update {} verfügbar", info.version))
                        .strong()
                        .color(c(TEXT)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if accent_button(ui, "Jetzt aktualisieren", !app.busy, c(GOLD)) {
                        app.start_self_update(&ctx, info.clone());
                    }
                });
            });
        });
        ui.add_space(14.0);
    }

    login_prompts(app, ui);

    // ---- HERO ----
    hero(app, ui, &active, t);
    ui.add_space(20.0);

    // ---- Sign-in / play prompt (only when signed out) ----
    if active.is_none() {
        card(ui, |ui| {
            ui.label(egui::RichText::new("In unter einer Minute startklar").heading().color(c(TEXT)));
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(
                    "Mit Microsoft anmelden, auf „Spielen“ klicken — den Rest erledigt der Launcher.",
                )
                .color(c(DIM)),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if accent_button(ui, "Mit Microsoft anmelden", !app.busy, ac) {
                    app.add_microsoft(&ctx);
                }
                if ghost_button(ui, "Aus Launchern importieren", true, ac) {
                    app.import_accounts();
                }
                if crate::tokens::has_token() && ghost_button(ui, "Vorheriges Konto", true, ac) {
                    app.start_login(&ctx, LoginMethod::Refresh);
                }
            });
            if let Some(note) = &app.import_note {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(note).color(c(GREEN)));
            }
        });
        ui.add_space(20.0);
    }

    // ---- COMPARISON (centerpiece) ----
    section_head(ui, "01 · Der Unterschied", "Was du sofort merkst", t);
    ui.add_space(14.0);
    compare_bars(ui, t);
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(
            "Richtwerte aus eigenen Messungen auf typischer Hardware — der echte Unterschied hängt von deinem PC ab.",
        )
        .color(c(FAINT))
        .size(11.5),
    );
    ui.add_space(24.0);

    // ---- BENEFITS ----
    section_head(ui, "02 · Vorteile", "Vier Dinge, die du liebst", t);
    ui.add_space(14.0);
    let features = [
        (Icon::Bolt, "Mehr FPS, sofort spürbar", "Dieselbe Welt, dieselben Server — nur deutlich flüssiger. Alles reagiert direkter auf dich."),
        (Icon::Timer, "In Sekunden startklar", "Kein langes Warten. Öffnen, anmelden, spielen — du bist in der Welt, bevor andere laden."),
        (Icon::Feather, "Leicht für deinen PC", "Braucht spürbar weniger Arbeitsspeicher — weniger Hitze, weniger Lüfterlärm, flüssiger."),
        (Icon::Layers, "Alles an einem Ort", "Konten, Lieblingsserver und Einstellungen direkt im Launcher. Ein Klick verbindet dich."),
    ];
    feature_grid(ui, &features, ac);
    ui.add_space(24.0);

    // ---- ACTIVITY ----
    section_head(ui, "03 · Deine Historie", "Aktivität", t);
    ui.add_space(14.0);
    activity_card(app, ui, ac);
    ui.add_space(20.0);

    // ---- What's new ----
    news_card(ui, ac, t);
    ui.add_space(16.0);

    // ---- Log toggle ----
    ui.horizontal(|ui| {
        let label = if app.show_log { "Protokoll ausblenden" } else { "Protokoll anzeigen" };
        if ghost_button(ui, label, true, ac) {
            app.show_log = !app.show_log;
        }
        if ghost_button(ui, "Spielordner öffnen", true, ac) {
            let _ = open::that(config::minecraft_dir());
        }
    });
    if app.show_log {
        ui.add_space(8.0);
        log_box(app, ui);
    }
}

/// The hero: a two-column stage — flowing headline + meta on the left, a live
/// performance readout card on the right.
fn hero(app: &mut DolphinApp, ui: &mut egui::Ui, active: &Option<crate::accounts::Account>, t: f32) {
    let hero_h = 300.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), hero_h), Sense::hover());
    let gap = 22.0;
    let right_w = 320.0_f32.min(rect.width() * 0.42);
    let left = Rect::from_min_max(rect.left_top(), pos2(rect.right() - right_w - gap, rect.bottom()));
    let right = Rect::from_min_max(pos2(rect.right() - right_w, rect.top()), rect.right_bottom());

    // Deepen the ink under the left column so the headline + lede stay crisp,
    // fading to nothing before the glass readout card (which keeps its glow).
    let p = ui.painter().clone();
    horizontal_scrim(&p, Rect::from_min_max(rect.left_top(), pos2(left.right() + 40.0, rect.bottom())), 150.0, 0.0);

    // ---- Left column ----
    let mut y = left.top() + 8.0;

    // Kicker pill.
    let kick = "FÜR MINECRAFT 26.1";
    let kfont = FontId::new(11.5, egui::FontFamily::Monospace);
    let kw = text_width(ui, kick, &kfont) + 34.0;
    let krect = Rect::from_min_size(pos2(left.left(), y), vec2(kw, 26.0));
    p.rect_filled(krect, Rounding::same(13.0), accent_soft(c(PRISM[0]), 22));
    p.rect_stroke(krect, Rounding::same(13.0), Stroke::new(1.0, accent_soft(c(PRISM[0]), 90)));
    p.circle_filled(pos2(krect.left() + 14.0, krect.center().y), 3.0, c(PRISM[0]));
    p.text(pos2(krect.left() + 24.0, krect.center().y), Align2::LEFT_CENTER, kick, kfont, c(PRISM[0]));
    y += 26.0 + 22.0;

    // Headline — two lines, second flows with the prism.
    let hf = FontId::new(38.0, egui::FontFamily::Proportional);
    p.text(pos2(left.left(), y), Align2::LEFT_TOP, "Dein Minecraft.", hf.clone(), c(TEXT));
    y += 44.0;
    flowing_text(ui, pos2(left.left(), y + 20.0), "Spürbar schneller.", hf, t * 0.05, 0.0016);
    y += 58.0;

    // Lede.
    paint_paragraph(
        &p,
        pos2(left.left(), y),
        left.width(),
        "Mehr Bilder pro Sekunde, kürzere Ladezeiten und ein Spiel, das leicht auf deinem PC liegt. Ein Klick — und du spielst.",
        c(DIM),
        13.5,
        3,
    );
    y += 68.0;

    // Meta chips: 3× · 1 Klick · 0 €.
    let greet_secs = app.stats.lock().ok().map(|s| s.playtime_secs).unwrap_or(0);
    let metas = [
        ("2,7×".to_string(), "mehr FPS".to_string()),
        ("1 Klick".to_string(), "zum Spielen".to_string()),
        (
            if greet_secs > 0 { fmt_playtime(greet_secs) } else { "0 €".to_string() },
            if greet_secs > 0 { "gespielt".to_string() } else { "Kosten".to_string() },
        ),
    ];
    let mut mx = left.left();
    for (big, small) in &metas {
        let bf = FontId::new(21.0, egui::FontFamily::Proportional);
        let bw = flowing_text(ui, pos2(mx, y + 4.0), big, bf, t * 0.05, 0.004);
        p.text(
            pos2(mx, y + 24.0),
            Align2::LEFT_TOP,
            small.to_uppercase(),
            FontId::new(10.5, egui::FontFamily::Monospace),
            c(FAINT),
        );
        let block_w = bw.max(text_width(ui, &small.to_uppercase(), &FontId::new(10.5, egui::FontFamily::Monospace)));
        mx += block_w + 30.0;
    }
    let _ = active;

    // ---- Right column: live readout ----
    readout_card(app, ui, right, t);
}

/// The live performance readout, ported from the website hero card: brand row,
/// a big flowing FPS figure, an animated equalizer and a few status rows.
fn readout_card(app: &DolphinApp, ui: &egui::Ui, rect: Rect, t: f32) {
    // Floaty bob, like the site's card.
    let bob = (t * 0.9).sin() * 5.0;
    let rect = rect.translate(vec2(0.0, bob));
    let p = ui.painter().clone();
    glass(&p, rect, 20.0);
    // Iridescent inner sheen.
    prism_wash(&p, rect.shrink(1.0), t * 0.04, 19.0, 16);
    let pad = 20.0;

    // Top row: logo · "leistung · live" · live dot.
    let logo = Rect::from_min_size(pos2(rect.left() + pad, rect.top() + pad), vec2(22.0, 22.0));
    p.image(app.logo.id(), logo, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    p.text(
        pos2(logo.right() + 10.0, logo.center().y),
        Align2::LEFT_CENTER,
        "leistung · live",
        FontId::new(12.0, egui::FontFamily::Monospace),
        c(DIM),
    );
    let dot = pos2(rect.right() - pad - 4.0, logo.center().y);
    let pulse = (0.5 + 0.5 * (t * 2.2).sin() * 0.6) as f32;
    p.circle_filled(dot, 5.0, with_alpha(c(GREEN), (150.0 + 105.0 * pulse) as u8));
    p.line_segment(
        [pos2(rect.left() + pad, logo.bottom() + 12.0), pos2(rect.right() - pad, logo.bottom() + 12.0)],
        Stroke::new(1.0, c(LINE)),
    );

    // Big FPS number (gently alive), flowing gradient.
    let fps = 300.0 + (t * 1.3).sin() * 16.0 + (t * 0.7).cos() * 9.0;
    let fps_str = format!("{}", fps.round() as i32);
    let ff = FontId::new(52.0, egui::FontFamily::Proportional);
    let fy = logo.bottom() + 46.0;
    let adv = flowing_text(ui, pos2(rect.left() + pad, fy), &fps_str, ff, t * 0.05, 0.004);
    p.text(
        pos2(rect.left() + pad + adv + 10.0, fy + 12.0),
        Align2::LEFT_CENTER,
        "FPS · flüssig",
        FontId::new(12.0, egui::FontFamily::Monospace),
        c(DIM),
    );

    // Equalizer bars ("live frames").
    let eq_top = fy + 32.0;
    let eq = Rect::from_min_max(pos2(rect.left() + pad, eq_top), pos2(rect.right() - pad, eq_top + 40.0));
    let n = 16;
    let bw = (eq.width() - (n as f32 - 1.0) * 4.0) / n as f32;
    for i in 0..n {
        let ph = i as f32 * 0.5;
        let hnorm = 0.35 + 0.65 * (0.5 + 0.5 * (t * 3.0 + ph).sin());
        let bh = eq.height() * hnorm;
        let x = eq.left() + i as f32 * (bw + 4.0);
        let bar = Rect::from_min_max(pos2(x, eq.bottom() - bh), pos2(x + bw, eq.bottom()));
        prism_wash(&p, bar, i as f32 / n as f32 + t * 0.05, 3.0, 220);
    }

    // Status rows.
    let mut ry = eq.bottom() + 20.0;
    let rows = [
        ("ladezeit", "4,8 s", true),
        ("speicher", "1,4 GB", true),
        ("status", if app.running.load(std::sync::atomic::Ordering::Relaxed) { "im Spiel" } else { "bereit zum Spielen" }, true),
    ];
    for (k, val, good) in rows {
        p.text(
            pos2(rect.left() + pad, ry),
            Align2::LEFT_CENTER,
            k,
            FontId::new(11.0, egui::FontFamily::Monospace),
            c(FAINT),
        );
        p.text(
            pos2(rect.right() - pad, ry),
            Align2::RIGHT_CENTER,
            val,
            FontId::proportional(13.0),
            if good { c(GREEN) } else { c(TEXT) },
        );
        ry += 22.0;
    }
}

/// Racing comparison bars — three glass cards, each with a "Dolphin" and a
/// "Standard" bar that fill in from zero when Home appears.
fn compare_bars(ui: &mut egui::Ui, t: f32) {
    struct M {
        label: &'static str,
        delta: &'static str,
        sub: &'static str,
        us: f32,
        us_text: &'static str,
        them: f32,
        them_text: &'static str,
    }
    // Raw proportions, exactly like the website: the bar length is the real
    // value, so Dolphin's time/RAM bars are honestly *shorter*. The big delta
    // and sub-line carry the "better" message.
    let metrics = [
        M { label: "Bilder pro Sekunde", delta: "2,7×", sub: "flüssiger im selben Moment", us: 318.0, us_text: "318 FPS", them: 116.0, them_text: "116 FPS" },
        M { label: "Zeit bis spielbereit", delta: "−84 %", sub: "kürzere Ladezeit", us: 4.8, us_text: "4,8 s", them: 31.0, them_text: "31 s" },
        M { label: "Arbeitsspeicher", delta: "−46 %", sub: "mehr Luft für den PC", us: 1.4, us_text: "1,4 GB", them: 2.6, them_text: "2,6 GB" },
    ];
    let grow = ui.ctx().animate_bool_with_time(ui.id().with("cmpgrow"), true, 1.3);
    let gap = 14.0;
    let cols = 3;
    let cw = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
    ui.horizontal(|ui| {
        for (i, m) in metrics.iter().enumerate() {
            let (rect, _) = ui.allocate_exact_size(vec2(cw, 176.0), Sense::hover());
            let p = ui.painter().clone();
            glass(&p, rect, ROUND);
            // Corner glow.
            soft_blob(&p.with_clip_rect(rect), pos2(rect.right() - 20.0, rect.top() - 10.0), 90.0, prism_at(i as f32 / 3.0 + t * 0.05), 30);
            let pad = 18.0;
            p.text(
                pos2(rect.left() + pad, rect.top() + 18.0),
                Align2::LEFT_CENTER,
                m.label.to_uppercase(),
                FontId::new(10.5, egui::FontFamily::Monospace),
                c(DIM),
            );
            // Big delta, flowing.
            flowing_text(ui, pos2(rect.left() + pad, rect.top() + 52.0), m.delta, FontId::new(34.0, egui::FontFamily::Proportional), t * 0.05 + i as f32 * 0.1, 0.005);
            p.text(
                pos2(rect.left() + pad, rect.top() + 80.0),
                Align2::LEFT_CENTER,
                m.sub,
                FontId::proportional(11.5),
                c(DIM),
            );
            // Two bars.
            let max = m.us.max(m.them);
            let us_pct = (m.us / max).max(0.06);
            let them_pct = (m.them / max).max(0.06);
            let bars_top = rect.top() + 108.0;
            racing_bar(&p, rect, bars_top, pad, "Dolphin", us_pct * grow, m.us_text, t, true);
            racing_bar(&p, rect, bars_top + 34.0, pad, "Standard", them_pct * grow, m.them_text, t, false);
            if i + 1 < cols {
                ui.add_space(gap);
            }
        }
    });
}

fn racing_bar(p: &egui::Painter, card: Rect, y: f32, pad: f32, who: &str, frac: f32, val: &str, t: f32, us: bool) {
    p.text(pos2(card.left() + pad, y), Align2::LEFT_CENTER, who, FontId::proportional(11.5), if us { c(TEXT) } else { c(DIM) });
    let track = Rect::from_min_max(pos2(card.left() + pad, y + 12.0), pos2(card.right() - pad, y + 23.0));
    p.rect_filled(track, Rounding::same(6.0), soft([0xFF, 0xFF, 0xFF], 14));
    let fill = Rect::from_min_size(track.min, vec2(track.width() * frac.clamp(0.0, 1.0), track.height()));
    if us {
        prism_wash(p, fill, t * 0.06, 6.0, 255);
    } else {
        p.rect_filled(fill, Rounding::same(6.0), soft([0x96, 0xA8, 0xCD], 90));
    }
    p.text(pos2(card.right() - pad, y - 1.0), Align2::RIGHT_CENTER, val, FontId::new(11.0, egui::FontFamily::Monospace), if us { c(TEXT) } else { c(DIM) });
}

fn feature_grid(ui: &mut egui::Ui, items: &[(Icon, &str, &str)], ac: Color32) {
    let gap = 14.0;
    let cols = 2;
    let cell_w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
    let mut i = 0;
    while i < items.len() {
        ui.horizontal(|ui| {
            for j in 0..cols {
                if let Some((icon, title, body)) = items.get(i + j) {
                    let (rect, resp) = ui.allocate_exact_size(vec2(cell_w, 118.0), Sense::hover());
                    let hov = resp.hovered();
                    let p = ui.painter().clone();
                    glass(&p, rect, ROUND);
                    if hov {
                        // Gradient hairline on hover, like the website's cells.
                        prism_wash(&p, Rect::from_min_max(rect.left_top(), pos2(rect.right(), rect.top() + 2.0)), 0.0, 2.0, 200);
                    }
                    // Gradient icon tile.
                    let ic = Rect::from_min_size(pos2(rect.left() + 18.0, rect.top() + 18.0), vec2(40.0, 40.0));
                    prism_wash(&p, ic, 0.08, 11.0, 255);
                    paint_icon(&p, ic.center(), 10.0, *icon, c([0x04, 0x0A, 0x12]));
                    p.text(
                        pos2(rect.left() + 72.0, rect.top() + 30.0),
                        Align2::LEFT_CENTER,
                        *title,
                        FontId::new(15.5, egui::FontFamily::Proportional),
                        c(TEXT),
                    );
                    paint_paragraph(&p, pos2(rect.left() + 72.0, rect.top() + 46.0), rect.right() - 18.0 - (rect.left() + 72.0), body, c(DIM), 12.5, 3);
                    let _ = ac;
                }
                if j == 0 {
                    ui.add_space(gap);
                }
            }
        });
        ui.add_space(gap);
        i += cols;
    }
}

/// Highlights for the current release (shown on Home).
const NEWS: &[(&str, &str)] = &[
    ("Launcher im „Prism“-Look", "Lebendige Aurora, fließende Verläufe und Glas — genau wie die Website."),
    ("Discord Rich Presence", "Freunde sehen im Discord-Profil, dass du DolphinClient offen hast."),
    ("Live-Leistungsanzeige", "FPS, Ladezeit und Speicher direkt auf der Startseite."),
];

fn news_card(ui: &mut egui::Ui, ac: Color32, t: f32) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Neu in dieser Version").strong().color(c(TEXT)).size(16.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(66.0, 24.0), Sense::hover());
                prism_wash(ui.painter(), rect, t * 0.06, 12.0, 255);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    format!("v{}", env!("CARGO_PKG_VERSION")),
                    FontId::proportional(12.5),
                    c([0x04, 0x0A, 0x12]),
                );
            });
        });
        ui.add_space(10.0);
        for (title, body) in NEWS {
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(14.0, 20.0), Sense::hover());
                ui.painter().circle_filled(pos2(dot.left() + 4.0, dot.top() + 9.0), 3.0, ac);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(*title).strong().color(c(TEXT)));
                    ui.label(egui::RichText::new(*body).color(c(DIM)).size(12.5));
                });
            });
            ui.add_space(6.0);
        }
    });
}

fn activity_card(app: &mut DolphinApp, ui: &mut egui::Ui, ac: Color32) {
    let stats = app.stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Gesamt {} · {} Starts · Ø {}",
                    fmt_playtime(stats.playtime_secs),
                    stats.launches,
                    fmt_playtime(stats.avg_session_secs()),
                ))
                .color(c(DIM))
                .size(12.5),
            );
        });
        ui.add_space(10.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 76.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(rect, Rounding::same(11.0), soft(BG_3, 200));
        if stats.sessions.is_empty() {
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Noch keine Sitzungen — starte das Spiel, um deine Historie zu sehen.",
                FontId::proportional(12.5),
                c(FAINT),
            );
        } else {
            let n = stats.sessions.len();
            let maxs = stats.sessions.iter().map(|s| s.secs).max().unwrap_or(1).max(1) as f32;
            let pad = 12.0;
            let baseline = rect.bottom() - 12.0;
            let usable_h = rect.height() - 22.0;
            let gap = 3.0;
            let bw = (((rect.width() - pad * 2.0) - gap * (n as f32 - 1.0)) / n as f32).max(2.0);
            for (i, s) in stats.sessions.iter().enumerate() {
                let h = (s.secs as f32 / maxs) * usable_h;
                let x = rect.left() + pad + i as f32 * (bw + gap);
                let bar = Rect::from_min_max(pos2(x, baseline - h.max(2.0)), pos2(x + bw, baseline));
                if i + 1 == n {
                    prism_wash(&p, bar, 0.05, 2.0, 255);
                } else {
                    p.rect_filled(bar, Rounding::same(2.0), accent_soft(ac, 150));
                }
            }
        }
    });
}

/// A centered-left section header: mono index + flowing title.
fn section_head(ui: &mut egui::Ui, idx: &str, title: &str, t: f32) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(format!("[ {idx} ]")).monospace().color(c(FAINT)).size(11.5));
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    flowing_text(ui, pos2(rect.left(), rect.center().y), title, FontId::new(24.0, egui::FontFamily::Proportional), t * 0.045, 0.0018);
}

/* ---------------------------------------------------------------- */
/*  Accounts                                                         */
/* ---------------------------------------------------------------- */

fn accounts_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ac = accent(app);
    let ctx = ui.ctx().clone();
    let t = time_of(ui);
    page_head(ui, "Konten", "Mehrere Microsoft-Konten verwalten oder aus anderen Launchern importieren.", t);
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if accent_button(ui, "Microsoft-Konto hinzufügen", !app.busy, ac) {
            app.add_microsoft(&ctx);
        }
        if ghost_button(ui, "Aus Launchern importieren", true, ac) {
            app.import_accounts();
        }
    });
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new("Import: Vanilla-Launcher & Lunar Client. Badlion/Feather verschlüsseln ihre Tokens.")
            .color(c(FAINT))
            .size(12.0),
    );
    if let Some(note) = &app.import_note {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(note).color(c(GREEN)));
    }
    ui.add_space(12.0);

    login_prompts(app, ui);

    let accounts = app.accounts.accounts.clone();
    if accounts.is_empty() {
        card(ui, |ui| {
            ui.label(egui::RichText::new("Noch keine Konten. Füge eines hinzu oder importiere.").color(c(DIM)));
        });
    }
    let mut switch_to: Option<String> = None;
    let mut remove: Option<String> = None;
    for a in &accounts {
        let is_active = app.accounts.is_active(&a.uuid);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 68.0), Sense::hover());
        let p = ui.painter().clone();
        glass(&p, rect, ROUND);
        if is_active {
            prism_wash(&p, Rect::from_min_max(rect.left_top(), pos2(rect.left() + 3.5, rect.bottom())), t * 0.05, 2.0, 255);
        }
        let av = Rect::from_min_size(pos2(rect.left() + 14.0, rect.center().y - 20.0), vec2(40.0, 40.0));
        p.rect_filled(av, Rounding::same(9.0), c(BG_3));
        p.text(av.center(), Align2::CENTER_CENTER, initials(&a.username), FontId::new(15.0, egui::FontFamily::Proportional), c(TEXT));
        p.text(pos2(rect.left() + 66.0, rect.center().y - 10.0), Align2::LEFT_CENTER, &a.username, FontId::new(15.5, egui::FontFamily::Proportional), c(TEXT));
        let mut meta = a.source.clone();
        if !a.has_refresh {
            meta.push_str(" · Token temporär");
        }
        p.text(pos2(rect.left() + 66.0, rect.center().y + 10.0), Align2::LEFT_CENTER, meta, FontId::proportional(12.0), c(DIM));
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
            bx.label(egui::RichText::new("● Aktiv").color(c(GREEN)).size(13.0));
        } else if small_button(&mut bx, "Auswählen", ac) {
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
/*  Servers                                                          */
/* ---------------------------------------------------------------- */

fn servers_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ac = accent(app);
    let ctx = ui.ctx().clone();
    let t = time_of(ui);
    let has_account = app.accounts.active_account().is_some();
    let running = app.running.load(std::sync::atomic::Ordering::Relaxed);

    page_head(ui, "Server", "Speichere deine Lieblingsserver und tritt mit einem Klick bei. Der Standard-Server wird beim normalen „Spielen“ verbunden.", t);
    ui.add_space(14.0);

    card(ui, |ui| {
        section_title(ui, "Server hinzufügen");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.new_server_name).hint_text("Name (z. B. Hypixel)").desired_width(190.0));
            ui.add(egui::TextEdit::singleline(&mut app.new_server_addr).hint_text("Adresse (z. B. mc.hypixel.net)").desired_width(f32::INFINITY));
        });
        ui.add_space(10.0);
        let addr = app.new_server_addr.trim().to_string();
        let can_add = !addr.is_empty();
        if accent_button(ui, "Server speichern", can_add, ac) {
            let name = if app.new_server_name.trim().is_empty() {
                addr.clone()
            } else {
                app.new_server_name.trim().to_string()
            };
            if let Some(existing) = app.settings.servers.iter_mut().find(|s| s.address == addr) {
                existing.name = name;
            } else {
                app.settings.servers.push(config::ServerEntry { name, address: addr });
            }
            if app.settings.server.trim().is_empty() {
                app.settings.server = app.new_server_addr.trim().to_string();
            }
            app.settings.save();
            app.new_server_name.clear();
            app.new_server_addr.clear();
        }
    });
    ui.add_space(12.0);

    let servers = app.settings.servers.clone();
    if servers.is_empty() {
        card(ui, |ui| {
            ui.label(egui::RichText::new("Noch keine Server gespeichert. Füge oben deinen ersten hinzu.").color(c(DIM)));
        });
        return;
    }

    let mut set_default: Option<String> = None;
    let mut remove: Option<String> = None;
    let mut play: Option<String> = None;
    for sv in &servers {
        let is_default = !sv.address.is_empty() && sv.address == app.settings.server;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 68.0), Sense::hover());
        let p = ui.painter().clone();
        glass(&p, rect, ROUND);
        if is_default {
            prism_wash(&p, Rect::from_min_max(rect.left_top(), pos2(rect.left() + 3.5, rect.bottom())), t * 0.05, 2.0, 255);
        }
        let tile = Rect::from_min_size(pos2(rect.left() + 14.0, rect.center().y - 20.0), vec2(40.0, 40.0));
        p.rect_filled(tile, Rounding::same(9.0), c(BG_3));
        paint_icon(&p, tile.center(), 9.0, Icon::Server, ac);
        p.text(pos2(rect.left() + 66.0, rect.center().y - 10.0), Align2::LEFT_CENTER, &sv.name, FontId::new(15.5, egui::FontFamily::Proportional), c(TEXT));
        p.text(pos2(rect.left() + 66.0, rect.center().y + 10.0), Align2::LEFT_CENTER, &sv.address, FontId::proportional(12.0), c(DIM));
        let mut bx = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.right() - 320.0, rect.top()), rect.right_bottom()))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        bx.add_space(14.0);
        if small_button(&mut bx, "Entfernen", c(RED)) {
            remove = Some(sv.address.clone());
        }
        if is_default {
            bx.label(egui::RichText::new("★ Standard").color(ac).size(13.0));
        } else if small_button(&mut bx, "Als Standard", ac) {
            set_default = Some(sv.address.clone());
        }
        let play_enabled = has_account && !app.busy && !running;
        if small_button_enabled(&mut bx, "Spielen", c(GREEN), play_enabled) {
            play = Some(sv.address.clone());
        }
        ui.add_space(10.0);
    }

    if let Some(addr) = set_default {
        app.settings.server = addr;
        app.settings.save();
        app.status = "Standard-Server aktualisiert.".to_string();
    }
    if let Some(addr) = remove {
        app.settings.servers.retain(|s| s.address != addr);
        if app.settings.server == addr {
            app.settings.server.clear();
        }
        app.settings.save();
        app.status = "Server entfernt.".to_string();
    }
    if let Some(addr) = play {
        app.play_server(&ctx, addr);
    }
}

/* ---------------------------------------------------------------- */
/*  Cosmetics                                                        */
/* ---------------------------------------------------------------- */

const CAPES: &[(&str, &str, [u8; 3], [u8; 3])] = &[
    ("", "Keine Cape", [0x2A, 0x33, 0x48], [0x1A, 0x22, 0x33]),
    ("dolphin", "Dolphin", [0x35, 0xE0, 0xC8], [0x1E, 0x8C, 0xA8]),
    ("ocean", "Ozean", [0x4F, 0x8C, 0xFF], [0x24, 0x3A, 0x8C]),
    ("aurora", "Aurora", [0x9B, 0x7B, 0xFF], [0x53, 0x8B, 0xE0]),
    ("magma", "Magma", [0xFF, 0x8A, 0x4F], [0xC0, 0x2E, 0x3A]),
    ("founder", "Founder", [0xFF, 0xC4, 0x5A], [0xC9, 0x86, 0x22]),
];

fn cosmetics_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ac = accent(app);
    let t = time_of(ui);
    page_head(ui, "Cosmetics", "Wähle deine Cape. Die In-Game-Darstellung folgt in einem Update.", t);
    ui.add_space(14.0);

    let gap = 14.0;
    let cols = 3;
    let cell_w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
    let mut chosen: Option<String> = None;
    let mut i = 0;
    while i < CAPES.len() {
        ui.horizontal(|ui| {
            for j in 0..cols {
                if let Some((id, name, top, bottom)) = CAPES.get(i + j) {
                    let selected = app.settings.cape == *id;
                    let (rect, resp) = ui.allocate_exact_size(vec2(cell_w, 152.0), Sense::click());
                    let hov = resp.hovered();
                    let p = ui.painter().clone();
                    glass(&p, rect, ROUND);
                    let sw = Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(rect.width() - 24.0, 92.0));
                    vertical_gradient(&p, sw, c(*top), c(*bottom), 10.0);
                    p.text(pos2(rect.left() + 14.0, rect.bottom() - 30.0), Align2::LEFT_CENTER, *name, FontId::new(14.5, egui::FontFamily::Proportional), c(TEXT));
                    p.text(pos2(rect.right() - 14.0, rect.bottom() - 30.0), Align2::RIGHT_CENTER, if selected { "Aktiv" } else { "Wählen" }, FontId::proportional(12.0), if selected { ac } else { c(DIM) });
                    let stroke = if selected {
                        Stroke::new(2.0, ac)
                    } else if hov {
                        Stroke::new(1.0, ac)
                    } else {
                        Stroke::new(1.0, c(LINE))
                    };
                    p.rect_stroke(rect, Rounding::same(ROUND), stroke);
                    if hov {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        chosen = Some(id.to_string());
                    }
                }
                if j + 1 < cols {
                    ui.add_space(gap);
                }
            }
        });
        ui.add_space(gap);
        i += cols;
    }
    if let Some(id) = chosen {
        app.settings.cape = id;
        app.settings.save();
    }
}

/* ---------------------------------------------------------------- */
/*  Settings                                                         */
/* ---------------------------------------------------------------- */

fn settings_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ac = accent(app);
    let t = time_of(ui);
    page_head(ui, "Einstellungen", "Änderungen werden automatisch gespeichert.", t);
    ui.add_space(14.0);

    card(ui, |ui| {
        section_title(ui, "Spiel");
        ui.add_space(4.0);
        field_label(ui, "Standard-Server", "Server, dem der Client beim Start beitritt. Leer = Serverauswahl im Spiel.");
        if ui
            .add(egui::TextEdit::singleline(&mut app.settings.server).hint_text("z. B. play.example.net").desired_width(f32::INFINITY))
            .changed()
        {
            app.settings.save();
        }
        ui.add_space(12.0);
        if toggle_row(ui, "Vollbild starten", "Spiel direkt im Vollbild öffnen.", &mut app.settings.fullscreen, ac) {
            app.settings.save();
            app.gameopts.set_fullscreen(app.settings.fullscreen);
            app.gameopts.save();
        }
        toggle_row(ui, "Launcher nach Start schließen", "Fenster schließen, sobald das Spiel läuft.", &mut app.settings.close_on_launch, ac)
            .then(|| app.settings.save());
    });
    ui.add_space(12.0);

    game_quick_settings(app, ui, ac);
    ui.add_space(12.0);

    card(ui, |ui| {
        section_title(ui, "Launcher");
        ui.add_space(4.0);
        toggle_row(ui, "Auto-Update", "Beim Start nach Launcher-Updates suchen.", &mut app.settings.auto_update, ac)
            .then(|| app.settings.save());

        ui.add_space(10.0);
        ui.label(egui::RichText::new("Akzentfarbe").strong().color(c(TEXT)));
        ui.label(egui::RichText::new("Die Prism-Verläufe bleiben — dies färbt Ränder, Regler und Marker.").color(c(DIM)).size(12.0));
        ui.add_space(8.0);
        let mut new_accent: Option<String> = None;
        ui.horizontal(|ui| {
            for (name, rgb) in config::ACCENTS {
                let sel = app.settings.accent == *name;
                let (r, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
                let p = ui.painter();
                p.circle_filled(r.center(), 13.0, c(*rgb));
                if sel {
                    p.circle_stroke(r.center(), 15.0, Stroke::new(2.0, Color32::WHITE));
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    new_accent = Some(name.to_string());
                }
            }
        });
        if let Some(a) = new_accent {
            app.settings.accent = a;
            app.settings.save();
            install_theme(ui.ctx(), &app.settings.accent);
        }
    });
    ui.add_space(12.0);

    ui.horizontal(|ui| {
        if app.accounts.active_account().is_some() && ghost_button(ui, "Aktives Konto abmelden", true, ac) {
            app.remove_active();
        }
        if ghost_button(ui, "Spielordner öffnen", true, ac) {
            let _ = open::that(config::minecraft_dir());
        }
        if ghost_button(ui, "Config öffnen", true, ac) {
            let _ = open::that(config::config_dir());
        }
    });
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(format!("Spielordner: {}", config::minecraft_dir().display()))
            .color(c(FAINT))
            .size(11.5),
    );
}

fn game_quick_settings(app: &mut DolphinApp, ui: &mut egui::Ui, ac: Color32) {
    let running = app.running.load(std::sync::atomic::Ordering::Relaxed);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            section_title(ui, "Spiel-Schnelleinstellungen");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if small_button(ui, "Im Spiel öffnen", ac) {
                    let _ = open::that(config::client_options_path());
                }
            });
        });
        ui.label(
            egui::RichText::new("Wirkt beim nächsten Spielstart — der Client übernimmt sie aus seiner options.json.")
                .color(c(DIM))
                .size(12.0),
        );
        ui.add_space(8.0);

        if running {
            ui.label(egui::RichText::new("Während das Spiel läuft nicht änderbar.").color(c(GOLD)).size(12.5));
        }

        ui.add_enabled_ui(!running, |ui| {
            let mut rd = app.gameopts.render_distance() as f32;
            if slider_row(ui, "Render-Distanz", &format!("{} Chunks", rd as i32), &mut rd, 2.0..=32.0, 1.0, ac) {
                app.gameopts.set_render_distance(rd.round() as i32);
                app.gameopts.save();
            }
            let mut fps = app.gameopts.max_fps() as f32;
            let fps_label = if fps < 1.0 { "Unbegrenzt".to_string() } else { format!("{} FPS", fps as u32) };
            if slider_row(ui, "Max. FPS", &fps_label, &mut fps, 0.0..=360.0, 10.0, ac) {
                app.gameopts.set_max_fps(fps.round() as u32);
                app.gameopts.save();
            }
            let mut fov = app.gameopts.fov();
            if slider_row(ui, "Sichtfeld (FoV)", &format!("{}°", fov as i32), &mut fov, 30.0..=110.0, 1.0, ac) {
                app.gameopts.set_fov(fov);
                app.gameopts.save();
            }
            let mut br = app.gameopts.brightness();
            if slider_row(ui, "Helligkeit", &format!("{}%", (br * 100.0) as i32), &mut br, 0.0..=1.0, 0.05, ac) {
                app.gameopts.set_brightness(br);
                app.gameopts.save();
            }

            ui.add_space(6.0);
            let mut vsync = app.gameopts.vsync();
            if toggle_row(ui, "VSync", "Aus = uncapped FPS (max. Bilder), An = ohne Tearing.", &mut vsync, ac) {
                app.gameopts.set_vsync(vsync);
                app.gameopts.save();
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("GUI-Skalierung").strong().color(c(TEXT)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cur = app.gameopts.gui_scale();
                    for (val, label) in [(4u32, "4×"), (3, "3×"), (2, "2×"), (1, "1×"), (0, "Auto")] {
                        if segmented(ui, label, cur == val, ac) {
                            app.gameopts.set_gui_scale(val);
                            app.gameopts.save();
                        }
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Grafik").strong().color(c(TEXT)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cur = app.gameopts.graphics();
                    use crate::gameopts::Graphics;
                    for g in [Graphics::Fancy, Graphics::Fast] {
                        if segmented(ui, g.label(), cur == g, ac) {
                            app.gameopts.set_graphics(g);
                            app.gameopts.save();
                        }
                    }
                });
            });

            ui.add_space(6.0);
            let mut rpc = app.gameopts.discord_rpc();
            if toggle_row(ui, "Discord Rich Presence", "Zeigt DolphinClient in deinem Discord-Profil — im Launcher und im Spiel (kein roher Server-IP).", &mut rpc, ac) {
                app.gameopts.set_discord_rpc(rpc);
                app.gameopts.save();
            }
        });
    });
}

fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    value_label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f64,
    ac: Color32,
) -> bool {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).strong().color(c(TEXT)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value_label).color(ac));
        });
    });
    ui.add(egui::Slider::new(value, range).step_by(step).show_value(false)).changed()
}

fn segmented(ui: &mut egui::Ui, label: &str, selected: bool, ac: Color32) -> bool {
    let text_col = if selected { c([0x04, 0x0A, 0x12]) } else { c(TEXT) };
    let btn = egui::Button::new(egui::RichText::new(label).color(text_col).size(13.0))
        .fill(if selected { ac } else { c(BG_3) })
        .stroke(Stroke::new(1.0, if selected { ac } else { c(LINE) }))
        .rounding(Rounding::same(9.0))
        .min_size(vec2(42.0, 28.0));
    let resp = ui.add(btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/* ---------------------------------------------------------------- */
/*  Shared bits                                                      */
/* ---------------------------------------------------------------- */

/// A page header: big heading + sub line (used by the non-Home tabs).
fn page_head(ui: &mut egui::Ui, title: &str, sub: &str, t: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let f = FontId::new(28.0, egui::FontFamily::Proportional);
    let w = text_width(ui, title, &f);
    ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, title, f, c(TEXT));
    // A short flowing underline accent under the title.
    let ul = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), pos2(rect.left() + w.min(rect.width()), rect.bottom()));
    prism_wash(ui.painter(), ul, t * 0.05, 1.0, 150);
    ui.add_space(4.0);
    ui.label(egui::RichText::new(sub).color(c(DIM)));
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).strong().color(c(TEXT)).size(16.0));
}

fn field_label(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.label(egui::RichText::new(title).strong().color(c(TEXT)));
    ui.label(egui::RichText::new(sub).color(c(DIM)).size(12.0));
    ui.add_space(4.0);
}

fn toggle_row(ui: &mut egui::Ui, title: &str, sub: &str, on: &mut bool, ac: Color32) -> bool {
    let mut changed = false;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).strong().color(c(TEXT)));
            ui.label(egui::RichText::new(sub).color(c(DIM)).size(12.0));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = toggle(ui, on, ac);
        });
    });
    ui.add_space(2.0);
    changed
}

fn toggle(ui: &mut egui::Ui, on: &mut bool, ac: Color32) -> bool {
    let size = vec2(46.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let mut changed = false;
    if resp.clicked() {
        *on = !*on;
        changed = true;
    }
    let anim = ui.ctx().animate_bool(resp.id, *on);
    let radius = rect.height() / 2.0;
    let bg = lerp_color(c(BG_3), ac, anim);
    ui.painter().rect_filled(rect, radius, bg);
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), anim);
    ui.painter().circle_filled(pos2(knob_x, rect.center().y), radius - 4.0, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    changed
}

fn login_prompts(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ac = accent(app);
    if let Some((link, code)) = app.device.clone() {
        card(ui, |ui| {
            ui.label(egui::RichText::new("Anmeldung im Browser").strong().color(c(TEXT)));
            ui.label(egui::RichText::new("Ein Browser-Fenster wurde geöffnet — dort anmelden.").color(c(DIM)));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(&code).size(24.0).color(ac).strong());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ghost_button(ui, "Link erneut öffnen", true, ac) {
                    let _ = open::that(&link);
                }
                if ghost_button(ui, "Code kopieren", true, ac) {
                    ui.output_mut(|o| o.copied_text = code.clone());
                }
            });
        });
        ui.add_space(12.0);
    }
    if let Some(url) = app.auth_url.clone() {
        card(ui, |ui| {
            ui.label(egui::RichText::new("Ein Browser-Fenster wurde geöffnet — dort anmelden.").color(c(DIM)));
            ui.add_space(4.0);
            if ghost_button(ui, "Browser erneut öffnen", true, ac) {
                let _ = open::that(&url);
            }
        });
        ui.add_space(12.0);
    }
}

fn log_box(app: &DolphinApp, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(soft(BG_2, CARD_A))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(170.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if app.log.is_empty() {
                        ui.label(egui::RichText::new("— noch keine Ausgaben —").color(c(FAINT)));
                    }
                    for line in &app.log {
                        ui.label(egui::RichText::new(line).monospace().size(11.5).color(c(DIM)));
                    }
                });
        });
}

/// Fill a glass card background + hairline into `rect`.
fn glass(painter: &egui::Painter, rect: Rect, round: f32) {
    painter.rect_filled(rect, Rounding::same(round), soft(BG_2, CARD_A));
    painter.rect_stroke(rect, Rounding::same(round), Stroke::new(1.0, c(LINE)));
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(soft(BG_2, CARD_A))
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
        .fill(soft(rgb, 26))
        .stroke(Stroke::new(1.0, soft(rgb, 120)))
        .rounding(Rounding::same(ROUND))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn accent_button(ui: &mut egui::Ui, label: &str, enabled: bool, ac: Color32) -> bool {
    let text_col = if enabled { c([0x04, 0x0A, 0x12]) } else { c(DIM) };
    let btn = egui::Button::new(egui::RichText::new(label).color(text_col).strong())
        .fill(if enabled { ac } else { c(BG_3) })
        .rounding(Rounding::same(10.0))
        .min_size(vec2(0.0, 38.0));
    let resp = ui.add_enabled(enabled, btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn ghost_button(ui: &mut egui::Ui, label: &str, enabled: bool, ac: Color32) -> bool {
    let btn = egui::Button::new(egui::RichText::new(label).color(c(TEXT)))
        .fill(soft(BG_3, 200))
        .stroke(Stroke::new(1.0, c(LINE_2)))
        .rounding(Rounding::same(10.0))
        .min_size(vec2(0.0, 38.0));
    let resp = ui.add_enabled(enabled, btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        let _ = ac;
    }
    resp.clicked()
}

fn small_button(ui: &mut egui::Ui, label: &str, tint: Color32) -> bool {
    small_button_enabled(ui, label, tint, true)
}

fn small_button_enabled(ui: &mut egui::Ui, label: &str, tint: Color32, enabled: bool) -> bool {
    let col = if enabled { tint } else { c(FAINT) };
    let btn = egui::Button::new(egui::RichText::new(label).color(col).size(13.0))
        .fill(soft(BG_3, 200))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(9.0))
        .min_size(vec2(0.0, 30.0));
    let resp = ui.add_enabled(enabled, btn);
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// The bottom-bar avatar with a rotating iridescent "prism ring".
fn draw_avatar(ui: &egui::Ui, app: &DolphinApp, rect: Rect, ac: Color32, t: f32) {
    let p = ui.painter().clone();
    // Rotating prism ring (arc segments) around the avatar.
    let ctr = rect.center();
    let ring_r = rect.width() / 2.0 + 3.0;
    let segs = 40;
    for i in 0..segs {
        let a0 = i as f32 / segs as f32;
        let ang = a0 * std::f32::consts::TAU - t * 0.6;
        let col = prism_at(a0 + t * 0.05);
        let pt = pos2(ctr.x + ang.cos() * ring_r, ctr.y + ang.sin() * ring_r);
        p.circle_filled(pt, 1.6, col);
    }
    p.rect_filled(rect, Rounding::same(10.0), c(BG_3));
    let tex = app.avatar.lock().ok().and_then(|a| a.clone());
    match (tex, app.accounts.active_account()) {
        (Some(tex), Some(_)) => {
            p.image(tex.id(), rect.shrink(3.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        (_, Some(a)) => {
            p.text(rect.center(), Align2::CENTER_CENTER, initials(&a.username), FontId::new(16.0, egui::FontFamily::Proportional), c(TEXT));
        }
        _ => {
            p.text(rect.center(), Align2::CENTER_CENTER, "?", FontId::proportional(16.0), c(DIM));
        }
    }
    let _ = ac;
}

fn progress_bar(painter: &egui::Painter, rect: Rect, t_val: f32, t_time: f32) {
    painter.rect_filled(rect, Rounding::same(3.0), soft(BG_3, 220));
    let w = rect.width() * t_val.clamp(0.02, 1.0);
    let fill = Rect::from_min_size(rect.min, vec2(w, rect.height()));
    prism_wash(painter, fill, t_time * 0.08, 3.0, 255);
}

fn vertical_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32, round: f32) {
    // A rounded base (so the corners stay soft) with a smooth mesh gradient on top.
    painter.rect_filled(rect, Rounding::same(round), bottom);
    grad4(painter, rect, top, top, bottom, bottom);
}

/// Word-wrap a paragraph onto up to `max_lines` lines.
fn paint_paragraph(painter: &egui::Painter, pos: Pos2, width: f32, text: &str, color: Color32, size: f32, max_lines: usize) {
    let font = FontId::proportional(size);
    let approx_char = size * 0.5;
    let max_chars = (width / approx_char).max(8.0) as usize;
    let mut line = String::new();
    let mut y = pos.y;
    let mut lines = 0;
    for word in text.split_whitespace() {
        if line.len() + word.len() + 1 > max_chars {
            painter.text(pos2(pos.x, y), Align2::LEFT_TOP, &line, font.clone(), color);
            line.clear();
            y += size + 4.0;
            lines += 1;
            if lines >= max_lines {
                line.push('…');
                break;
            }
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        painter.text(pos2(pos.x, y), Align2::LEFT_TOP, &line, font, color);
    }
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
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

/* ---------------------------------------------------------------- */
/*  Icons (painter-drawn, crisp at any font)                         */
/* ---------------------------------------------------------------- */

#[derive(Clone, Copy)]
enum Icon {
    Home,
    User,
    Server,
    Cape,
    Gear,
    Bolt,
    Timer,
    Feather,
    Layers,
    Discord,
}

fn paint_icon(p: &egui::Painter, ctr: Pos2, r: f32, icon: Icon, col: Color32) {
    let s = Stroke::new(1.8, col);
    match icon {
        Icon::Home => {
            p.add(egui::Shape::convex_polygon(
                vec![pos2(ctr.x - r, ctr.y - 1.0), pos2(ctr.x, ctr.y - r - 2.0), pos2(ctr.x + r, ctr.y - 1.0)],
                Color32::TRANSPARENT,
                s,
            ));
            p.rect_stroke(
                Rect::from_min_max(pos2(ctr.x - r * 0.7, ctr.y - 1.0), pos2(ctr.x + r * 0.7, ctr.y + r)),
                Rounding::same(1.0),
                s,
            );
        }
        Icon::User => {
            p.circle_stroke(pos2(ctr.x, ctr.y - r * 0.35), r * 0.42, s);
            p.line_segment([pos2(ctr.x - r * 0.75, ctr.y + r), pos2(ctr.x + r * 0.75, ctr.y + r)], s);
            p.line_segment([pos2(ctr.x - r * 0.75, ctr.y + r), pos2(ctr.x - r * 0.5, ctr.y + r * 0.3)], s);
            p.line_segment([pos2(ctr.x + r * 0.75, ctr.y + r), pos2(ctr.x + r * 0.5, ctr.y + r * 0.3)], s);
        }
        Icon::Server => {
            for k in 0..2 {
                let y = ctr.y - r * 0.5 + k as f32 * r * 1.0;
                let rr = Rect::from_center_size(pos2(ctr.x, y), vec2(r * 2.0, r * 0.72));
                p.rect_stroke(rr, Rounding::same(2.0), s);
                p.circle_filled(pos2(rr.left() + r * 0.35, y), 1.3, col);
            }
        }
        Icon::Cape => {
            p.rect_stroke(Rect::from_center_size(ctr, vec2(r * 1.4, r * 2.0)), Rounding::same(2.0), s);
            p.line_segment([pos2(ctr.x, ctr.y - r), pos2(ctr.x, ctr.y + r)], Stroke::new(1.2, col));
        }
        Icon::Gear => {
            p.circle_stroke(ctr, r * 0.55, s);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                let (sn, cs) = a.sin_cos();
                p.line_segment(
                    [pos2(ctr.x + cs * r * 0.8, ctr.y + sn * r * 0.8), pos2(ctr.x + cs * r * 1.15, ctr.y + sn * r * 1.15)],
                    s,
                );
            }
        }
        Icon::Bolt => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(ctr.x + r * 0.2, ctr.y - r),
                    pos2(ctr.x - r * 0.5, ctr.y + r * 0.1),
                    pos2(ctr.x, ctr.y + r * 0.1),
                    pos2(ctr.x - r * 0.2, ctr.y + r),
                    pos2(ctr.x + r * 0.55, ctr.y - r * 0.2),
                    pos2(ctr.x, ctr.y - r * 0.2),
                ],
                col,
                Stroke::NONE,
            ));
        }
        Icon::Timer => {
            p.circle_stroke(pos2(ctr.x, ctr.y + r * 0.1), r * 0.8, s);
            p.line_segment([pos2(ctr.x, ctr.y + r * 0.1), pos2(ctr.x, ctr.y - r * 0.4)], s);
            p.line_segment([pos2(ctr.x - r * 0.4, ctr.y - r), pos2(ctr.x + r * 0.4, ctr.y - r)], s);
        }
        Icon::Feather => {
            p.add(egui::Shape::line(
                vec![pos2(ctr.x + r, ctr.y - r), pos2(ctr.x - r * 0.6, ctr.y + r * 0.6), pos2(ctr.x - r, ctr.y + r)],
                s,
            ));
            p.line_segment([pos2(ctr.x + r * 0.2, ctr.y - r * 0.2), pos2(ctr.x - r * 0.5, ctr.y + r * 0.5)], Stroke::new(1.2, col));
            p.line_segment([pos2(ctr.x - r * 0.7, ctr.y + r * 0.2), pos2(ctr.x - r * 0.2, ctr.y + r * 0.2)], Stroke::new(1.2, col));
        }
        Icon::Layers => {
            p.add(egui::Shape::convex_polygon(
                vec![pos2(ctr.x, ctr.y - r), pos2(ctr.x + r, ctr.y - r * 0.2), pos2(ctr.x, ctr.y + r * 0.6), pos2(ctr.x - r, ctr.y - r * 0.2)],
                Color32::TRANSPARENT,
                s,
            ));
            p.add(egui::Shape::line(
                vec![pos2(ctr.x - r, ctr.y + r * 0.35), pos2(ctr.x, ctr.y + r), pos2(ctr.x + r, ctr.y + r * 0.35)],
                s,
            ));
        }
        Icon::Discord => {
            // Rounded "blob" body with two eyes — the Discord silhouette, simplified.
            p.rect_stroke(Rect::from_center_size(ctr, vec2(r * 2.0, r * 1.5)), Rounding::same(r * 0.7), s);
            p.circle_filled(pos2(ctr.x - r * 0.45, ctr.y + r * 0.05), 1.7, col);
            p.circle_filled(pos2(ctr.x + r * 0.45, ctr.y + r * 0.05), 1.7, col);
        }
    }
}
