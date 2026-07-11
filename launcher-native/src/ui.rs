//! The launcher UI — a modern, dark, sleek layout in the spirit of Lunar
//! Client / NoRisk Client: a slim custom title bar, a left navigation rail, a
//! persistent bottom play bar and card-based content. Everything is drawn with
//! egui's own widgets and painter (no Minecraft textures). All settings save
//! themselves on change — there is no "Save" button.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Rounding, Sense, Stroke,
    ViewportCommand, pos2, vec2,
};

use crate::app::{DolphinApp, LoginMethod, Tab};
use crate::config::{self, TARGET_VERSION};

/* ---------------------------------------------------------------- */
/*  Palette                                                          */
/* ---------------------------------------------------------------- */

// The "Abyss" palette — a deep ocean-black canvas with cool hairlines and one
// aqua accent (supplied at runtime), matching the website 1:1.
pub const BG_0: [u8; 3] = [0x05, 0x07, 0x0D]; // window / floor
const BG_1: [u8; 3] = [0x08, 0x0B, 0x14]; // rails / bars
const BG_2: [u8; 3] = [0x0B, 0x10, 0x19]; // cards
const BG_3: [u8; 3] = [0x10, 0x17, 0x25]; // inputs / hover
const LINE: [u8; 3] = [0x1B, 0x24, 0x33]; // hairline borders
const TEXT: [u8; 3] = [0xEA, 0xF1, 0xFB];
const DIM: [u8; 3] = [0x9A, 0xA8, 0xBE];
const FAINT: [u8; 3] = [0x56, 0x64, 0x7C];
const GREEN: [u8; 3] = [0x45, 0xE0, 0xA0];
const RED: [u8; 3] = [0xFF, 0x6B, 0x6B];
const GOLD: [u8; 3] = [0xF0, 0xB2, 0x3C];

fn c(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
fn soft(rgb: [u8; 3], a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], a)
}
fn accent_soft(a: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), alpha)
}
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), 255)
}
fn lighten(a: Color32, t: f32) -> Color32 {
    lerp_color(a, Color32::WHITE, t)
}

fn accent(app: &DolphinApp) -> Color32 {
    c(config::accent_rgb(&app.settings.accent))
}

const TITLEBAR_H: f32 = 44.0;
const BOTTOM_H: f32 = 80.0;
const NAV_W: f32 = 220.0;
const ROUND: f32 = 11.0;

/* ---------------------------------------------------------------- */
/*  Theme                                                            */
/* ---------------------------------------------------------------- */

pub fn install_theme(ctx: &egui::Context, accent_name: &str) {
    let accent = c(config::accent_rgb(accent_name));
    let round = Rounding::same(12.0);

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
        offset: vec2(0.0, 6.0),
        blur: 24.0,
        spread: 0.0,
        color: Color32::from_black_alpha(120),
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
        (TextStyle::Heading, FontId::new(23.0, FontFamily::Proportional)),
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
    title_bar(app, ctx);
    bottom_bar(app, ctx);
    nav_rail(app, ctx);

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(c(BG_0)))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(22.0);
                    content_column(ui, |ui| match app.tab {
                        Tab::Home => home_view(app, ui),
                        Tab::Accounts => accounts_view(app, ui),
                        Tab::Servers => servers_view(app, ui),
                        Tab::Cosmetics => cosmetics_view(app, ui),
                        Tab::Settings => settings_view(app, ui),
                    });
                    ui.add_space(28.0);
                });
        });
}

/// Centered fixed-max-width content column.
fn content_column(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let w = 760.0_f32.min(ui.available_width() - 48.0);
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
        .frame(egui::Frame::none().fill(c(BG_0)))
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            let painter = ui.painter().clone();

            // Logo + two-tone wordmark (left) — mirrors the website brand.
            let logo = Rect::from_center_size(pos2(bar.left() + 24.0, bar.center().y), vec2(26.0, 26.0));
            painter.image(
                app.logo.id(),
                logo,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            let wordmark = FontId::new(16.0, egui::FontFamily::Proportional);
            let r1 = painter.text(
                pos2(bar.left() + 48.0, bar.center().y),
                Align2::LEFT_CENTER,
                "Dolphin",
                wordmark.clone(),
                c(TEXT),
            );
            painter.text(
                pos2(r1.right(), bar.center().y),
                Align2::LEFT_CENTER,
                "Client",
                wordmark,
                accent(app),
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
    let size = vec2(30.0, 24.0);
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
                .inner_margin(egui::Margin::symmetric(12.0, 14.0)),
        )
        .show(ctx, |ui| {
            // Right border.
            let r = ui.max_rect();
            ui.painter().line_segment(
                [pos2(r.right(), r.top()), pos2(r.right(), r.bottom())],
                Stroke::new(1.0, c(LINE)),
            );

            let n = app.accounts.accounts.len();
            let sv = app.settings.servers.len();
            let items = [
                (Icon::Home, "Start".to_string(), Tab::Home),
                (Icon::User, format!("Konten ({n})"), Tab::Accounts),
                (Icon::Server, format!("Server ({sv})"), Tab::Servers),
                (Icon::Cape, "Cosmetics".to_string(), Tab::Cosmetics),
                (Icon::Gear, "Einstellungen".to_string(), Tab::Settings),
            ];
            for (icon, label, tab) in items {
                if nav_item(ui, icon, &label, app.tab == tab, ac) {
                    app.tab = tab;
                }
                ui.add_space(4.0);
            }

            // Bottom block: dashboard link, bridge state, version.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("Launcher v{}", env!("CARGO_PKG_VERSION")))
                        .color(c(FAINT))
                        .size(11.5),
                );
                ui.horizontal(|ui| {
                    let dot = if app.running.load(std::sync::atomic::Ordering::Relaxed) {
                        c(GREEN)
                    } else {
                        ac
                    };
                    let (rr, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(rr.center(), 4.0, dot);
                    ui.label(
                        egui::RichText::new("Dashboard-Bridge aktiv")
                            .color(c(DIM))
                            .size(11.5),
                    );
                });
                ui.add_space(8.0);
                if ghost_button(ui, "Web-Dashboard öffnen", true, ac) {
                    let _ = open::that("https://dolphinclient.de/dashboard");
                }
                ui.add_space(6.0);
            });
        });
}

fn nav_item(ui: &mut egui::Ui, icon: Icon, label: &str, active: bool, ac: Color32) -> bool {
    let h = 44.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let hov = resp.hovered();
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, Rounding::same(10.0), accent_soft(ac, 34));
        painter.rect_filled(
            Rect::from_min_max(rect.left_top(), pos2(rect.left() + 3.5, rect.bottom())),
            Rounding::same(2.0),
            ac,
        );
    } else if hov {
        painter.rect_filled(rect, Rounding::same(10.0), c(BG_2));
    }
    let icon_col = if active {
        ac
    } else if hov {
        c(TEXT)
    } else {
        c(DIM)
    };
    paint_icon(painter, pos2(rect.left() + 24.0, rect.center().y), 9.0, icon, icon_col);
    painter.text(
        pos2(rect.left() + 46.0, rect.center().y),
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
                .inner_margin(egui::Margin::symmetric(16.0, 0.0)),
        )
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            ui.painter().line_segment(
                [bar.left_top(), bar.right_top()],
                Stroke::new(1.0, c(LINE)),
            );

            // ----- Right: PLAY button + version picker -----
            let btn_w = 190.0;
            let btn_h = 50.0;
            let btn_rect = Rect::from_min_size(
                pos2(bar.right() - btn_w, bar.center().y - btn_h / 2.0),
                vec2(btn_w, btn_h),
            );
            let can_play = active.is_some() && !app.busy && !running;
            let clicked = play_button(ui, btn_rect, &active, running, app.busy, ac);
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
                    pos2(btn_rect.left() - 178.0, bar.center().y - 17.0),
                    vec2(166.0, 34.0),
                );
                let mut ui2 = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(combo_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                version_combo(app, &mut ui2);
            }

            // ----- Left: avatar + account + status/progress -----
            let av_rect = Rect::from_min_size(pos2(bar.left(), bar.center().y - 23.0), vec2(46.0, 46.0));
            draw_avatar(ui, app, av_rect, ac);

            let tx = bar.left() + 60.0;
            let name = match &active {
                Some(a) => a.username.clone(),
                None => "Kein Konto".to_string(),
            };
            ui.painter().text(
                pos2(tx, bar.center().y - 12.0),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(15.0),
                c(TEXT),
            );
            let status_col = if app.status.starts_with("Fehler") {
                c(RED)
            } else if running {
                c(GREEN)
            } else {
                c(DIM)
            };
            let sub = truncate(&app.status, 52);
            ui.painter().text(
                pos2(tx, bar.center().y + 9.0),
                Align2::LEFT_CENTER,
                sub,
                FontId::proportional(12.5),
                status_col,
            );
            if app.busy || app.progress > 0.001 {
                let pr = Rect::from_min_size(pos2(tx, bar.center().y + 20.0), vec2(230.0, 5.0));
                progress_bar(ui.painter(), pr, app.progress, ac);
            }
        });
}

fn play_button(
    ui: &mut egui::Ui,
    rect: Rect,
    active: &Option<crate::accounts::Account>,
    running: bool,
    busy: bool,
    ac: Color32,
) -> bool {
    let enabled = !busy && !running;
    let resp = ui.interact(rect, ui.id().with("playbtn"), Sense::click());
    let hov = enabled && resp.hovered();
    let painter = ui.painter();
    let fill = if !enabled {
        c(BG_3)
    } else if hov {
        lighten(ac, 0.10)
    } else {
        ac
    };
    painter.rect_filled(rect, Rounding::same(ROUND), fill);
    let (label, show_tri) = if running {
        ("LÄUFT", false)
    } else if busy {
        ("…", false)
    } else if active.is_some() {
        ("SPIELEN", true)
    } else {
        ("ANMELDEN", false)
    };
    let fg = if enabled { c(BG_0) } else { c(DIM) };
    let mut tx = rect.center().x;
    if show_tri {
        tx += 12.0;
        let cy = rect.center().y;
        let lx = rect.center().x - 58.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(lx, cy - 7.0),
                pos2(lx + 12.0, cy),
                pos2(lx, cy + 7.0),
            ],
            fg,
            Stroke::NONE,
        ));
    }
    painter.text(
        pos2(tx, rect.center().y),
        Align2::CENTER_CENTER,
        label,
        FontId::new(16.0, egui::FontFamily::Proportional),
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
        "Version: Neueste".to_string()
    } else {
        format!("Version: {sel}")
    };
    egui::ComboBox::from_id_salt("verpick")
        .selected_text(egui::RichText::new(label).size(13.0))
        .width(160.0)
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

    // ---- Hero ----
    let hero_h = 180.0;
    let (hero_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), hero_h), Sense::hover());
    let hero_round = Rounding::same(18.0);
    let p = ui.painter();
    p.rect_filled(hero_rect, hero_round, c(BG_2));
    // A gentle brand wash + a soft accent glow toward the top-right corner.
    p.rect_filled(hero_rect, hero_round, accent_soft(ac, 14));
    let glow = Rect::from_center_size(hero_rect.right_top() + vec2(-40.0, 30.0), vec2(260.0, 200.0));
    p.rect_filled(glow, Rounding::same(120.0), accent_soft(ac, 16));
    p.rect_stroke(hero_rect, hero_round, Stroke::new(1.0, c(LINE)));
    let pad = 24.0;
    let greet = match &active {
        Some(a) => format!("Willkommen zurück, {}", a.username),
        None => "Willkommen bei DolphinClient".to_string(),
    };

    // Logo badge to the left of the greeting (brand-forward, matches the site).
    let badge = Rect::from_min_size(pos2(hero_rect.left() + pad, hero_rect.top() + 22.0), vec2(48.0, 48.0));
    p.rect_filled(badge, Rounding::same(13.0), accent_soft(ac, 38));
    p.rect_stroke(badge, Rounding::same(13.0), Stroke::new(1.0, accent_soft(ac, 90)));
    p.image(
        app.logo.id(),
        badge.shrink(7.0),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    let text_x = badge.right() + 15.0;
    p.text(
        pos2(text_x, hero_rect.top() + 36.0),
        Align2::LEFT_CENTER,
        greet,
        FontId::new(23.0, egui::FontFamily::Proportional),
        c(TEXT),
    );
    p.text(
        pos2(text_x, hero_rect.top() + 60.0),
        Align2::LEFT_CENTER,
        format!("Native Minecraft-{TARGET_VERSION}-Engine · Rust · wgpu · kein Java"),
        egui::FontId::new(12.5, egui::FontFamily::Monospace),
        c(DIM),
    );

    // Quick stat chips inside the hero.
    let stats = app.stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
    let chips = [
        ("Spielzeit", fmt_playtime(stats.playtime_secs)),
        ("Starts", stats.launches.to_string()),
        (
            "Client",
            if app.settings.client_version.is_empty() {
                "Neueste".to_string()
            } else {
                app.settings.client_version.clone()
            },
        ),
    ];
    let chip_w = 150.0;
    let chip_h = 60.0;
    let mut cx = hero_rect.left() + pad;
    let cy = hero_rect.bottom() - pad - chip_h;
    for (label, value) in chips {
        let r = Rect::from_min_size(pos2(cx, cy), vec2(chip_w, chip_h));
        let pp = ui.painter();
        pp.rect_filled(r, Rounding::same(10.0), c(BG_3));
        pp.text(
            pos2(r.left() + 14.0, r.top() + 20.0),
            Align2::LEFT_CENTER,
            value,
            FontId::new(19.0, egui::FontFamily::Proportional),
            c(TEXT),
        );
        pp.text(
            pos2(r.left() + 14.0, r.top() + 42.0),
            Align2::LEFT_CENTER,
            label.to_uppercase(),
            egui::FontId::new(10.5, egui::FontFamily::Monospace),
            c(FAINT),
        );
        cx += chip_w + 12.0;
    }

    // Full-body skin preview on the right of the hero (once fetched).
    if let Some(tex) = app.body.lock().ok().and_then(|b| b.clone()) {
        let [tw, th] = tex.size();
        if tw > 0 && th > 0 {
            let target_h = (hero_h - 28.0).min(150.0);
            let target_w = target_h * tw as f32 / th as f32;
            let bx = hero_rect.right() - 28.0 - target_w;
            let by = hero_rect.center().y - target_h / 2.0;
            let brect = Rect::from_min_size(pos2(bx, by), vec2(target_w, target_h));
            ui.painter().image(
                tex.id(),
                brect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    ui.add_space(16.0);

    // ---- Sign-in / play prompt ----
    if active.is_none() {
        card(ui, |ui| {
            ui.label(egui::RichText::new("Anmelden").heading().color(c(TEXT)));
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Melde dich mit deinem Microsoft-Konto an, um zu spielen.")
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
                if crate::tokens::has_token()
                    && ghost_button(ui, "Vorheriges Konto", true, ac)
                {
                    app.start_login(&ctx, LoginMethod::Refresh);
                }
            });
            if let Some(note) = &app.import_note {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(note).color(c(GREEN)));
            }
        });
        ui.add_space(16.0);
    }

    // ---- Feature highlights ----
    ui.label(egui::RichText::new("Was drinsteckt").strong().color(c(TEXT)).size(15.0));
    ui.add_space(8.0);
    let features = [
        (Icon::Bolt, "Eigener Renderer", "wgpu in Rust — spricht Vulkan, DX12 und Metal direkt an."),
        (Icon::Sound, "Echter Sound", "Originale Mojang-Sounds, pro Kategorie regelbar."),
        (Icon::Globe, "1:1 Multiplayer", "Echte 26.1-Server. Kein Singleplayer, keine Cheats."),
        (Icon::Refresh, "Hält sich aktuell", "SHA-256-Abgleich bei jedem Start — nie veraltet."),
    ];
    feature_grid(ui, &features, ac);

    ui.add_space(16.0);

    // ---- Activity (session history sparkline) ----
    activity_card(app, ui, ac);
    ui.add_space(16.0);

    // ---- What's new ----
    news_card(ui, ac);
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

fn feature_grid(ui: &mut egui::Ui, items: &[(Icon, &str, &str)], ac: Color32) {
    let gap = 12.0;
    let cols = 2;
    let cell_w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
    let mut i = 0;
    while i < items.len() {
        ui.horizontal(|ui| {
            for j in 0..cols {
                if let Some((icon, title, body)) = items.get(i + j) {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(cell_w, 92.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(rect, Rounding::same(14.0), c(BG_2));
                    p.rect_stroke(rect, Rounding::same(14.0), Stroke::new(1.0, c(LINE)));
                    let ic = Rect::from_min_size(pos2(rect.left() + 16.0, rect.top() + 16.0), vec2(30.0, 30.0));
                    p.rect_filled(ic, Rounding::same(8.0), accent_soft(ac, 34));
                    paint_icon(p, ic.center(), 8.0, *icon, ac);
                    p.text(
                        pos2(rect.left() + 58.0, rect.top() + 24.0),
                        Align2::LEFT_CENTER,
                        *title,
                        FontId::new(15.0, egui::FontFamily::Proportional),
                        c(TEXT),
                    );
                    paint_wrapped(
                        p,
                        pos2(rect.left() + 58.0, rect.top() + 44.0),
                        rect.right() - 16.0,
                        body,
                        c(DIM),
                    );
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
    ("Neues „Abyss“-Design", "Dunkle Instrument-Panel-Optik, feine Linien, ein Aqua-Akzent."),
    ("Server-Liste & Quick-Settings", "Server speichern und beitreten, Spiel-Optionen vorab setzen."),
    ("Skin-Vorschau & Aktivität", "Ganzkörper-Skin im Profil plus eine Spielzeit-Historie."),
];

/// A "what's new in this version" card.
fn news_card(ui: &mut egui::Ui, ac: Color32) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Neu in dieser Version").strong().color(c(TEXT)).size(16.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(60.0, 22.0), Sense::hover());
                ui.painter().rect_filled(rect, Rounding::same(11.0), accent_soft(ac, 40));
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    format!("v{}", env!("CARGO_PKG_VERSION")),
                    FontId::proportional(12.5),
                    ac,
                );
            });
        });
        ui.add_space(8.0);
        for (title, body) in NEWS {
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(12.0, 20.0), Sense::hover());
                ui.painter().circle_filled(pos2(dot.left() + 4.0, dot.top() + 9.0), 3.0, ac);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(*title).strong().color(c(TEXT)));
                    ui.label(egui::RichText::new(*body).color(c(DIM)).size(12.5));
                });
            });
            ui.add_space(4.0);
        }
    });
}

/// A playtime/session activity card with a small bar sparkline.
fn activity_card(app: &mut DolphinApp, ui: &mut egui::Ui, ac: Color32) {
    let stats = app.stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
    card(ui, |ui| {
        ui.horizontal(|ui| {
            section_title(ui, "Aktivität");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
        });
        ui.add_space(10.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, Rounding::same(10.0), c(BG_3));
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
                // Highlight the most recent session.
                let col = if i + 1 == n { ac } else { accent_soft(ac, 150) };
                p.rect_filled(bar, Rounding::same(2.0), col);
            }
        }
    });
}

/* ---------------------------------------------------------------- */
/*  Accounts                                                         */
/* ---------------------------------------------------------------- */

fn accounts_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ac = accent(app);
    let ctx = ui.ctx().clone();
    ui.label(egui::RichText::new("Konten").heading().color(c(TEXT)));
    ui.label(
        egui::RichText::new(
            "Mehrere Microsoft-Konten verwalten oder aus anderen Launchern importieren.",
        )
        .color(c(DIM)),
    );
    ui.add_space(12.0);
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
        egui::RichText::new(
            "Import: Vanilla-Launcher & Lunar Client. Badlion/Feather verschlüsseln ihre Tokens.",
        )
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
            ui.label(
                egui::RichText::new("Noch keine Konten. Füge eines hinzu oder importiere.")
                    .color(c(DIM)),
            );
        });
    }
    let mut switch_to: Option<String> = None;
    let mut remove: Option<String> = None;
    for a in &accounts {
        let is_active = app.accounts.is_active(&a.uuid);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, Rounding::same(14.0), c(BG_2));
        let stroke = if is_active {
            Stroke::new(1.5, ac)
        } else {
            Stroke::new(1.0, c(LINE))
        };
        p.rect_stroke(rect, Rounding::same(14.0), stroke);
        // avatar box
        let av = Rect::from_min_size(pos2(rect.left() + 12.0, rect.center().y - 20.0), vec2(40.0, 40.0));
        p.rect_filled(av, Rounding::same(8.0), c(BG_3));
        p.text(
            av.center(),
            Align2::CENTER_CENTER,
            initials(&a.username),
            FontId::new(15.0, egui::FontFamily::Proportional),
            c(TEXT),
        );
        p.text(
            pos2(rect.left() + 62.0, rect.center().y - 10.0),
            Align2::LEFT_CENTER,
            &a.username,
            FontId::new(15.5, egui::FontFamily::Proportional),
            if is_active { c(TEXT) } else { c(TEXT) },
        );
        let mut meta = a.source.clone();
        if !a.has_refresh {
            meta.push_str(" · Token temporär");
        }
        p.text(
            pos2(rect.left() + 62.0, rect.center().y + 10.0),
            Align2::LEFT_CENTER,
            meta,
            FontId::proportional(12.0),
            c(DIM),
        );
        // right-side buttons via overlay ui
        let mut bx = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(rect.right() - 210.0, rect.top()),
                    rect.right_bottom(),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        bx.add_space(12.0);
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
    let has_account = app.accounts.active_account().is_some();
    let running = app.running.load(std::sync::atomic::Ordering::Relaxed);

    ui.label(egui::RichText::new("Server").heading().color(c(TEXT)));
    ui.label(
        egui::RichText::new(
            "Speichere deine Lieblingsserver und tritt mit einem Klick bei. Der als \
             Standard markierte Server wird beim normalen „Spielen“ automatisch verbunden.",
        )
        .color(c(DIM)),
    );
    ui.add_space(14.0);

    // ---- Add-server form ----
    card(ui, |ui| {
        section_title(ui, "Server hinzufügen");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.new_server_name)
                    .hint_text("Name (z. B. Hypixel)")
                    .desired_width(190.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut app.new_server_addr)
                    .hint_text("Adresse (z. B. mc.hypixel.net)")
                    .desired_width(f32::INFINITY),
            );
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
            // Avoid duplicates by address; update the name if it already exists.
            if let Some(existing) = app
                .settings
                .servers
                .iter_mut()
                .find(|s| s.address == addr)
            {
                existing.name = name;
            } else {
                app.settings.servers.push(config::ServerEntry { name, address: addr });
            }
            // First server added becomes the default automatically.
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
            ui.label(
                egui::RichText::new(
                    "Noch keine Server gespeichert. Füge oben deinen ersten hinzu.",
                )
                .color(c(DIM)),
            );
        });
        return;
    }

    let mut set_default: Option<String> = None;
    let mut remove: Option<String> = None;
    let mut play: Option<String> = None;
    for sv in &servers {
        let is_default = !sv.address.is_empty() && sv.address == app.settings.server;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, Rounding::same(14.0), c(BG_2));
        let stroke = if is_default {
            Stroke::new(1.5, ac)
        } else {
            Stroke::new(1.0, c(LINE))
        };
        p.rect_stroke(rect, Rounding::same(14.0), stroke);
        // Icon tile.
        let tile = Rect::from_min_size(pos2(rect.left() + 12.0, rect.center().y - 20.0), vec2(40.0, 40.0));
        p.rect_filled(tile, Rounding::same(8.0), c(BG_3));
        paint_icon(p, tile.center(), 9.0, Icon::Server, ac);
        p.text(
            pos2(rect.left() + 64.0, rect.center().y - 10.0),
            Align2::LEFT_CENTER,
            &sv.name,
            FontId::new(15.5, egui::FontFamily::Proportional),
            c(TEXT),
        );
        p.text(
            pos2(rect.left() + 64.0, rect.center().y + 10.0),
            Align2::LEFT_CENTER,
            &sv.address,
            FontId::proportional(12.0),
            c(DIM),
        );
        // Right-side controls.
        let mut bx = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(rect.right() - 310.0, rect.top()),
                    rect.right_bottom(),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        bx.add_space(12.0);
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
    ui.label(egui::RichText::new("Cosmetics").heading().color(c(TEXT)));
    ui.label(
        egui::RichText::new("Wähle deine Cape. Die In-Game-Darstellung folgt in einem Update.")
            .color(c(DIM)),
    );
    ui.add_space(14.0);

    let gap = 12.0;
    let cols = 3;
    let cell_w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
    let mut chosen: Option<String> = None;
    let mut i = 0;
    while i < CAPES.len() {
        ui.horizontal(|ui| {
            for j in 0..cols {
                if let Some((id, name, top, bottom)) = CAPES.get(i + j) {
                    let selected = app.settings.cape == *id;
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(cell_w, 150.0), Sense::click());
                    let hov = resp.hovered();
                    let p = ui.painter();
                    p.rect_filled(rect, Rounding::same(14.0), c(BG_2));
                    // swatch
                    let sw = Rect::from_min_size(
                        rect.min + vec2(12.0, 12.0),
                        vec2(rect.width() - 24.0, 92.0),
                    );
                    vertical_gradient(p, sw, c(*top), c(*bottom), 10.0);
                    p.text(
                        pos2(rect.left() + 14.0, rect.bottom() - 30.0),
                        Align2::LEFT_CENTER,
                        *name,
                        FontId::new(14.5, egui::FontFamily::Proportional),
                        c(TEXT),
                    );
                    p.text(
                        pos2(rect.right() - 14.0, rect.bottom() - 30.0),
                        Align2::RIGHT_CENTER,
                        if selected { "Aktiv" } else { "Wählen" },
                        FontId::proportional(12.0),
                        if selected { ac } else { c(DIM) },
                    );
                    let stroke = if selected {
                        Stroke::new(2.0, ac)
                    } else if hov {
                        Stroke::new(1.0, ac)
                    } else {
                        Stroke::new(1.0, c(LINE))
                    };
                    p.rect_stroke(rect, Rounding::same(14.0), stroke);
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
    ui.label(egui::RichText::new("Einstellungen").heading().color(c(TEXT)));
    ui.label(egui::RichText::new("Änderungen werden automatisch gespeichert.").color(c(DIM)));
    ui.add_space(14.0);

    // ---- Spiel ----
    card(ui, |ui| {
        section_title(ui, "Spiel");
        ui.add_space(4.0);
        field_label(ui, "Standard-Server", "Server, dem der Client beim Start beitritt. Leer = Serverauswahl im Spiel.");
        if ui
            .add(
                egui::TextEdit::singleline(&mut app.settings.server)
                    .hint_text("z. B. play.example.net")
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            app.settings.save();
        }
        ui.add_space(12.0);

        if toggle_row(ui, "Vollbild starten", "Spiel direkt im Vollbild öffnen.", &mut app.settings.fullscreen, ac) {
            app.settings.save();
            // The native client reads fullscreen from its own options.json, so
            // mirror the choice there — otherwise this toggle would do nothing.
            app.gameopts.set_fullscreen(app.settings.fullscreen);
            app.gameopts.save();
        }
        toggle_row(ui, "Launcher nach Start schließen", "Fenster schließen, sobald das Spiel läuft.", &mut app.settings.close_on_launch, ac)
            .then(|| app.settings.save());
    });
    ui.add_space(12.0);

    game_quick_settings(app, ui, ac);
    ui.add_space(12.0);

    // ---- Launcher ----
    card(ui, |ui| {
        section_title(ui, "Launcher");
        ui.add_space(4.0);
        toggle_row(ui, "Auto-Update", "Beim Start nach Launcher-Updates suchen.", &mut app.settings.auto_update, ac)
            .then(|| app.settings.save());

        ui.add_space(10.0);
        ui.label(egui::RichText::new("Akzentfarbe").strong().color(c(TEXT)));
        ui.add_space(6.0);
        let mut new_accent: Option<String> = None;
        ui.horizontal(|ui| {
            for (name, rgb) in config::ACCENTS {
                let sel = app.settings.accent == *name;
                let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
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

    // ---- Erweitert (Java fallback) ----
    card(ui, |ui| {
        section_title(ui, "Erweitert");
        ui.add_space(4.0);
        field_label(ui, "Java-Pfad", "Nur für den klassischen Java-Start. Leer = java aus PATH.");
        if ui
            .add(
                egui::TextEdit::singleline(&mut app.settings.java_path)
                    .hint_text("leer = java aus PATH")
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            app.settings.save();
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Zugewiesener RAM").strong().color(c(TEXT)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("{} GB", app.settings.ram_gb)).color(ac));
            });
        });
        let mut ram = app.settings.ram_gb as f32;
        if ui
            .add(egui::Slider::new(&mut ram, 2.0..=16.0).step_by(1.0).show_value(false))
            .changed()
        {
            app.settings.ram_gb = ram.round() as u32;
            app.settings.save();
        }
    });
    ui.add_space(12.0);

    // ---- Konto / Ordner ----
    ui.horizontal(|ui| {
        if app.accounts.active_account().is_some()
            && ghost_button(ui, "Aktives Konto abmelden", true, ac)
        {
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

/// Game quick-settings: a card that writes a small subset of the native
/// client's `options.json`, so players can tune the game from the launcher.
/// Editing is disabled while the game runs (the client rewrites the file on exit).
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
            egui::RichText::new(
                "Wirkt beim nächsten Spielstart — der Client übernimmt sie aus seiner options.json.",
            )
            .color(c(DIM))
            .size(12.0),
        );
        ui.add_space(8.0);

        if running {
            ui.label(
                egui::RichText::new("Während das Spiel läuft nicht änderbar.")
                    .color(c(GOLD))
                    .size(12.5),
            );
        }

        ui.add_enabled_ui(!running, |ui| {
            // Render distance.
            let mut rd = app.gameopts.render_distance() as f32;
            if slider_row(ui, "Render-Distanz", &format!("{} Chunks", rd as i32), &mut rd, 2.0..=32.0, 1.0, ac) {
                app.gameopts.set_render_distance(rd.round() as i32);
                app.gameopts.save();
            }
            // Max FPS (0 = uncapped).
            let mut fps = app.gameopts.max_fps() as f32;
            let fps_label = if fps < 1.0 { "Unbegrenzt".to_string() } else { format!("{} FPS", fps as u32) };
            if slider_row(ui, "Max. FPS", &fps_label, &mut fps, 0.0..=360.0, 10.0, ac) {
                app.gameopts.set_max_fps(fps.round() as u32);
                app.gameopts.save();
            }
            // FoV.
            let mut fov = app.gameopts.fov();
            if slider_row(ui, "Sichtfeld (FoV)", &format!("{}°", fov as i32), &mut fov, 30.0..=110.0, 1.0, ac) {
                app.gameopts.set_fov(fov);
                app.gameopts.save();
            }
            // Brightness.
            let mut br = app.gameopts.brightness();
            if slider_row(ui, "Helligkeit", &format!("{}%", (br * 100.0) as i32), &mut br, 0.0..=1.0, 0.05, ac) {
                app.gameopts.set_brightness(br);
                app.gameopts.save();
            }

            ui.add_space(6.0);
            // VSync toggle (mirrors client behaviour: off = uncapped).
            let mut vsync = app.gameopts.vsync();
            if toggle_row(ui, "VSync", "Aus = uncapped FPS (max. Bilder), An = ohne Tearing.", &mut vsync, ac) {
                app.gameopts.set_vsync(vsync);
                app.gameopts.save();
            }

            ui.add_space(8.0);
            // GUI scale segmented selector.
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
            // Graphics preset.
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
            // Discord Rich Presence.
            let mut rpc = app.gameopts.discord_rpc();
            if toggle_row(ui, "Discord Rich Presence", "Zeigt deinen Server (kein roher IP) im Discord-Profil.", &mut rpc, ac) {
                app.gameopts.set_discord_rpc(rpc);
                app.gameopts.save();
            }
        });
    });
}

/// A labelled slider row (label left, live value right, full-width slider).
/// Returns true when the value changed.
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

/// A small pill-style segmented button; highlighted when `selected`.
fn segmented(ui: &mut egui::Ui, label: &str, selected: bool, ac: Color32) -> bool {
    let text_col = if selected { c(BG_0) } else { c(TEXT) };
    let btn = egui::Button::new(egui::RichText::new(label).color(text_col).size(13.0))
        .fill(if selected { ac } else { c(BG_3) })
        .stroke(Stroke::new(1.0, if selected { ac } else { c(LINE) }))
        .rounding(Rounding::same(8.0))
        .min_size(vec2(42.0, 28.0));
    let resp = ui.add(btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).strong().color(c(TEXT)).size(16.0));
}

fn field_label(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.label(egui::RichText::new(title).strong().color(c(TEXT)));
    ui.label(egui::RichText::new(sub).color(c(DIM)).size(12.0));
    ui.add_space(4.0);
}

/// A labelled toggle row. Returns true when it changed.
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
    let t = ui.ctx().animate_bool(resp.id, *on);
    let radius = rect.height() / 2.0;
    let bg = lerp_color(c(BG_3), ac, t);
    ui.painter().rect_filled(rect, radius, bg);
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
        .fill(c(BG_2))
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
        .fill(c(BG_2))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(14.0))
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
        .rounding(Rounding::same(14.0))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn accent_button(ui: &mut egui::Ui, label: &str, enabled: bool, ac: Color32) -> bool {
    let text_col = if enabled { c(BG_0) } else { c(DIM) };
    let btn = egui::Button::new(egui::RichText::new(label).color(text_col).strong())
        .fill(if enabled { ac } else { c(BG_3) })
        .rounding(Rounding::same(10.0))
        .min_size(vec2(0.0, 36.0));
    let resp = ui.add_enabled(enabled, btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn ghost_button(ui: &mut egui::Ui, label: &str, enabled: bool, ac: Color32) -> bool {
    let btn = egui::Button::new(egui::RichText::new(label).color(c(TEXT)))
        .fill(c(BG_3))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(10.0))
        .min_size(vec2(0.0, 36.0));
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
        .fill(c(BG_3))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(9.0))
        .min_size(vec2(0.0, 30.0));
    let resp = ui.add_enabled(enabled, btn);
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn draw_avatar(ui: &egui::Ui, app: &DolphinApp, rect: Rect, ac: Color32) {
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(10.0), c(BG_3));
    let tex = app.avatar.lock().ok().and_then(|a| a.clone());
    match (tex, app.accounts.active_account()) {
        (Some(tex), Some(_)) => {
            p.image(
                tex.id(),
                rect.shrink(3.0),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        (_, Some(a)) => {
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                initials(&a.username),
                FontId::new(16.0, egui::FontFamily::Proportional),
                c(TEXT),
            );
        }
        _ => {
            p.text(rect.center(), Align2::CENTER_CENTER, "?", FontId::proportional(16.0), c(DIM));
        }
    }
    p.rect_stroke(rect, Rounding::same(10.0), Stroke::new(1.5, accent_soft(ac, 120)));
}

fn progress_bar(painter: &egui::Painter, rect: Rect, t: f32, ac: Color32) {
    painter.rect_filled(rect, Rounding::same(3.0), c(BG_3));
    let w = (rect.width()) * t.clamp(0.02, 1.0);
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(w, rect.height())),
        Rounding::same(3.0),
        ac,
    );
}

fn vertical_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32, round: f32) {
    // Approximate a vertical gradient with horizontal bands.
    let bands = 24;
    for i in 0..bands {
        let t0 = i as f32 / bands as f32;
        let y0 = rect.top() + rect.height() * t0;
        let y1 = rect.top() + rect.height() * ((i + 1) as f32 / bands as f32);
        let col = lerp_color(top, bottom, t0);
        let r = if i == 0 {
            Rounding { nw: round, ne: round, sw: 0.0, se: 0.0 }
        } else if i == bands - 1 {
            Rounding { nw: 0.0, ne: 0.0, sw: round, se: round }
        } else {
            Rounding::ZERO
        };
        painter.rect_filled(Rect::from_min_max(pos2(rect.left(), y0), pos2(rect.right(), y1)), r, col);
    }
}

/// Very small word-wrap onto two lines for feature bodies.
fn paint_wrapped(painter: &egui::Painter, pos: Pos2, right: f32, text: &str, color: Color32) {
    let font = FontId::proportional(12.5);
    let max_w = right - pos.x;
    let approx_char = 6.4;
    let max_chars = (max_w / approx_char).max(8.0) as usize;
    let mut line = String::new();
    let mut y = pos.y;
    let mut lines = 0;
    for word in text.split_whitespace() {
        if line.len() + word.len() + 1 > max_chars {
            painter.text(pos2(pos.x, y), Align2::LEFT_CENTER, &line, font.clone(), color);
            line.clear();
            y += 16.0;
            lines += 1;
            if lines >= 2 {
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
        painter.text(pos2(pos.x, y), Align2::LEFT_CENTER, &line, font, color);
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
    Sound,
    Globe,
    Refresh,
}

fn paint_icon(p: &egui::Painter, ctr: Pos2, r: f32, icon: Icon, col: Color32) {
    let s = Stroke::new(1.8, col);
    match icon {
        Icon::Home => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(ctr.x - r, ctr.y - 1.0),
                    pos2(ctr.x, ctr.y - r - 2.0),
                    pos2(ctr.x + r, ctr.y - 1.0),
                ],
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
            // Two stacked rack units with a status LED each.
            for k in 0..2 {
                let y = ctr.y - r * 0.5 + k as f32 * r * 1.0;
                let rr = Rect::from_center_size(pos2(ctr.x, y), vec2(r * 2.0, r * 0.72));
                p.rect_stroke(rr, Rounding::same(2.0), s);
                p.circle_filled(pos2(rr.left() + r * 0.35, y), 1.3, col);
            }
        }
        Icon::Cape => {
            p.rect_stroke(
                Rect::from_center_size(ctr, vec2(r * 1.4, r * 2.0)),
                Rounding::same(2.0),
                s,
            );
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
        Icon::Sound => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(ctr.x - r, ctr.y - r * 0.35),
                    pos2(ctr.x - r * 0.3, ctr.y - r * 0.35),
                    pos2(ctr.x + r * 0.2, ctr.y - r),
                    pos2(ctr.x + r * 0.2, ctr.y + r),
                    pos2(ctr.x - r * 0.3, ctr.y + r * 0.35),
                    pos2(ctr.x - r, ctr.y + r * 0.35),
                ],
                Color32::TRANSPARENT,
                s,
            ));
            p.line_segment([pos2(ctr.x + r * 0.55, ctr.y - r * 0.4), pos2(ctr.x + r * 0.9, ctr.y - r * 0.7)], s);
            p.line_segment([pos2(ctr.x + r * 0.55, ctr.y + r * 0.4), pos2(ctr.x + r * 0.9, ctr.y + r * 0.7)], s);
        }
        Icon::Globe => {
            p.circle_stroke(ctr, r, s);
            p.line_segment([pos2(ctr.x - r, ctr.y), pos2(ctr.x + r, ctr.y)], s);
            p.add(egui::Shape::line(
                vec![
                    pos2(ctr.x, ctr.y - r),
                    pos2(ctr.x - r * 0.7, ctr.y),
                    pos2(ctr.x, ctr.y + r),
                    pos2(ctr.x + r * 0.7, ctr.y),
                    pos2(ctr.x, ctr.y - r),
                ],
                s,
            ));
        }
        Icon::Refresh => {
            p.circle_stroke(ctr, r * 0.8, s);
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(ctr.x + r * 0.8, ctr.y - r * 0.9),
                    pos2(ctr.x + r * 0.8, ctr.y - r * 0.1),
                    pos2(ctr.x + r * 1.4, ctr.y - r * 0.5),
                ],
                col,
                Stroke::NONE,
            ));
        }
    }
}
