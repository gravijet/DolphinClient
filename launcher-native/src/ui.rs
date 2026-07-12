//! The launcher UI — a calm, dark, professional Minecraft launcher in the
//! spirit of Lunar / Feather / Badlion: flat dark surfaces, ONE aqua accent used
//! only on the primary "Spielen" button and the active nav item, a short left
//! nav rail, a launch-pad home (your character + one clear action) and a
//! persistent bottom bar with account, version and the play button. No animated
//! backgrounds, no gimmicks — real controls only. Everything is painted with
//! egui's own widgets and painter; settings save on change.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Rounding, Sense, Stroke,
    ViewportCommand, pos2, vec2,
};

use crate::app::{DolphinApp, LoginMethod, Tab};
use crate::config;

/* ---------------------------------------------------------------- */
/*  Palette — flat, dark, one aqua accent                            */
/* ---------------------------------------------------------------- */

pub const BG_0: [u8; 3] = [0x0E, 0x11, 0x17]; // window
const BG_1: [u8; 3] = [0x12, 0x16, 0x1D]; // rails / bottom bar
const BG_2: [u8; 3] = [0x16, 0x1B, 0x23]; // cards
const BG_3: [u8; 3] = [0x1E, 0x24, 0x2E]; // inputs / hover
const LINE: [u8; 3] = [0x27, 0x2E, 0x39]; // hairline borders
const LINE_2: [u8; 3] = [0x33, 0x3C, 0x49]; // brighter hairline
const TEXT: [u8; 3] = [0xE7, 0xEC, 0xF3];
const DIM: [u8; 3] = [0x98, 0xA2, 0xB0];
const FAINT: [u8; 3] = [0x5E, 0x68, 0x75];
const ACCENT: [u8; 3] = [0x35, 0xE0, 0xC8]; // the one aqua accent (Play + active)
const GREEN: [u8; 3] = [0x46, 0xD6, 0xA0]; // "in game" state
const RED: [u8; 3] = [0xE0, 0x60, 0x4C];
const GOLD: [u8; 3] = [0xE7, 0xB2, 0x4A];
const INK: [u8; 3] = [0x08, 0x0C, 0x12]; // text on the accent button

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

const TITLEBAR_H: f32 = 44.0;
const BOTTOM_H: f32 = 84.0;
const NAV_W: f32 = 212.0;
const ROUND: f32 = 12.0;

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
    v.selection.bg_fill = soft(ACCENT, 60);
    v.selection.stroke = Stroke::new(1.0, accent);
    v.popup_shadow = egui::epaint::Shadow {
        offset: vec2(0.0, 6.0),
        blur: 20.0,
        spread: 0.0,
        color: Color32::from_black_alpha(140),
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
        .frame(egui::Frame::none().fill(c(BG_0)).inner_margin(egui::Margin::ZERO))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(26.0);
                    content_column(ui, |ui| match app.tab {
                        Tab::Home => home_view(app, ui),
                        Tab::Accounts => accounts_view(app, ui),
                        Tab::Cosmetics => cosmetics_view(app, ui),
                        Tab::Settings => settings_view(app, ui),
                    });
                    ui.add_space(30.0);
                });
        });
}

/// Centered fixed-max-width content column.
fn content_column(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let w = 720.0_f32.min(ui.available_width() - 56.0);
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
            painter.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, c(LINE)));

            let logo = Rect::from_center_size(pos2(bar.left() + 22.0, bar.center().y), vec2(24.0, 24.0));
            painter.image(
                app.logo.id(),
                logo,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            let wordmark = FontId::new(15.5, egui::FontFamily::Proportional);
            let r1 = painter.text(
                pos2(bar.left() + 44.0, bar.center().y),
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
    egui::SidePanel::left("nav")
        .exact_width(NAV_W)
        .resizable(false)
        .frame(egui::Frame::none().fill(c(BG_1)).inner_margin(egui::Margin::symmetric(12.0, 14.0)))
        .show(ctx, |ui| {
            let r = ui.max_rect();
            ui.painter().line_segment([pos2(r.right(), r.top()), pos2(r.right(), r.bottom())], Stroke::new(1.0, c(LINE)));

            let n = app.accounts.accounts.len();
            let items = [
                (Icon::Home, "Start".to_string(), Tab::Home),
                (Icon::User, format!("Konten · {n}"), Tab::Accounts),
                (Icon::Cape, "Cosmetics".to_string(), Tab::Cosmetics),
                (Icon::Gear, "Einstellungen".to_string(), Tab::Settings),
            ];
            for (icon, label, tab) in items {
                if nav_item(ui, icon, &label, app.tab == tab) {
                    app.tab = tab;
                }
                ui.add_space(4.0);
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .color(c(FAINT))
                        .size(11.5),
                );
                ui.add_space(6.0);
            });
        });
}

fn nav_item(ui: &mut egui::Ui, icon: Icon, label: &str, active: bool) -> bool {
    let h = 44.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let hov = resp.hovered();
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, Rounding::same(10.0), c(BG_3));
        painter.rect_filled(
            Rect::from_min_max(rect.left_top(), pos2(rect.left() + 3.0, rect.bottom())),
            Rounding::same(2.0),
            c(ACCENT),
        );
    } else if hov {
        painter.rect_filled(rect, Rounding::same(10.0), c(BG_2));
    }
    let fg = if active { c(ACCENT) } else if hov { c(TEXT) } else { c(DIM) };
    paint_icon(painter, pos2(rect.left() + 22.0, rect.center().y), 9.0, icon, fg);
    painter.text(
        pos2(rect.left() + 44.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        if active { c(TEXT) } else { fg },
    );
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/* ---------------------------------------------------------------- */
/*  Bottom bar — account · version · Play                            */
/* ---------------------------------------------------------------- */

fn bottom_bar(app: &mut DolphinApp, ctx: &egui::Context) {
    let active = app.accounts.active_account().cloned();
    let running = app.running.load(std::sync::atomic::Ordering::Relaxed);
    egui::TopBottomPanel::bottom("play")
        .exact_height(BOTTOM_H)
        .frame(egui::Frame::none().fill(c(BG_1)).inner_margin(egui::Margin::symmetric(18.0, 0.0)))
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            ui.painter().line_segment([bar.left_top(), bar.right_top()], Stroke::new(1.0, c(LINE)));

            // Right: PLAY button + version picker.
            let btn_w = 196.0;
            let btn_h = 50.0;
            let btn_rect = Rect::from_min_size(pos2(bar.right() - btn_w, bar.center().y - btn_h / 2.0), vec2(btn_w, btn_h));
            let can_play = active.is_some() && !app.busy && !running;
            if play_button(ui, btn_rect, &active, running, app.busy) {
                if active.is_some() {
                    if can_play {
                        app.start_launch(ctx);
                    }
                } else {
                    app.add_microsoft(ctx);
                }
            }

            if active.is_some() {
                let combo_rect = Rect::from_min_size(pos2(btn_rect.left() - 184.0, bar.center().y - 17.0), vec2(170.0, 34.0));
                let mut ui2 = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(combo_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                version_combo(app, &mut ui2);
            }

            // Left: avatar + account + status.
            let av_rect = Rect::from_min_size(pos2(bar.left(), bar.center().y - 22.0), vec2(44.0, 44.0));
            draw_avatar(ui, app, av_rect);

            let tx = bar.left() + 58.0;
            let name = match &active {
                Some(a) => a.username.clone(),
                None => "Kein Konto".to_string(),
            };
            ui.painter().text(pos2(tx, bar.center().y - 12.0), Align2::LEFT_CENTER, name, FontId::proportional(15.0), c(TEXT));
            let status_col = if app.status.starts_with("Fehler") || app.relogin_for.is_some() {
                c(RED)
            } else if running {
                c(GREEN)
            } else {
                c(DIM)
            };
            ui.painter().text(
                pos2(tx, bar.center().y + 8.0),
                Align2::LEFT_CENTER,
                truncate(&app.status, 50),
                FontId::proportional(12.0),
                status_col,
            );
            if app.busy || app.progress > 0.001 {
                let pr = Rect::from_min_size(pos2(tx, bar.center().y + 20.0), vec2(240.0, 4.0));
                progress_bar(ui.painter(), pr, app.progress);
            }
        });
}

fn play_button(ui: &mut egui::Ui, rect: Rect, active: &Option<crate::accounts::Account>, running: bool, busy: bool) -> bool {
    let enabled = !busy && !running;
    let resp = ui.interact(rect, ui.id().with("playbtn"), Sense::click());
    let hov = enabled && resp.hovered();
    let painter = ui.painter();
    let fill = if !enabled {
        c(BG_3)
    } else if hov {
        lerp_color(c(ACCENT), Color32::WHITE, 0.12)
    } else {
        c(ACCENT)
    };
    painter.rect_filled(rect, Rounding::same(ROUND), fill);
    if !enabled {
        painter.rect_stroke(rect, Rounding::same(ROUND), Stroke::new(1.0, c(LINE)));
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
    let fg = if enabled { c(INK) } else { c(DIM) };
    let mut txp = rect.center().x;
    if show_tri {
        txp += 11.0;
        let cy = rect.center().y;
        let lx = rect.center().x - 54.0;
        painter.add(egui::Shape::convex_polygon(
            vec![pos2(lx, cy - 7.0), pos2(lx + 12.0, cy), pos2(lx, cy + 7.0)],
            fg,
            Stroke::NONE,
        ));
    }
    painter.text(pos2(txp, rect.center().y), Align2::CENTER_CENTER, label, FontId::new(15.5, egui::FontFamily::Proportional), fg);
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
        .width(168.0)
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
/*  Home — a launch pad: your character + identity                   */
/* ---------------------------------------------------------------- */

fn home_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let active = app.accounts.active_account().cloned();

    // Update banner.
    if let Some(info) = app.update_note.lock().ok().and_then(|n| n.clone()) {
        card_tinted(ui, GOLD, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Update {} verfügbar", info.version)).strong().color(c(TEXT)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if accent_button(ui, "Aktualisieren", !app.busy) {
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
            ui.add_space(30.0);
            signed_out_card(app, ui);
        }
        Some(acc) => {
            ui.add_space(8.0);
            // Character render, centered.
            let body = app.body.lock().ok().and_then(|b| b.clone());
            let stage_h = 300.0;
            let (stage, _) = ui.allocate_exact_size(vec2(ui.available_width(), stage_h), Sense::hover());
            let p = ui.painter().clone();
            // A very subtle floor shadow under the character (no glow, no gradient wash).
            p.circle_filled(pos2(stage.center().x, stage.bottom() - 18.0), 66.0, soft([0x00, 0x00, 0x00], 60));
            if let Some(tex) = body {
                let [tw, th] = tex.size();
                if tw > 0 && th > 0 {
                    let target_h = stage_h - 30.0;
                    let target_w = target_h * tw as f32 / th as f32;
                    let bx = stage.center().x - target_w / 2.0;
                    let by = stage.top();
                    p.image(
                        tex.id(),
                        Rect::from_min_size(pos2(bx, by), vec2(target_w, target_h)),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            } else {
                p.text(stage.center(), Align2::CENTER_CENTER, "…", FontId::proportional(20.0), c(FAINT));
            }
            ui.add_space(2.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(&acc.username).size(24.0).strong().color(c(TEXT)));
                ui.add_space(2.0);
                let ver = if app.settings.client_version.is_empty() {
                    "Neueste".to_string()
                } else {
                    app.settings.client_version.clone()
                };
                let stats = app.stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
                let line = format!(
                    "{}  ·  Minecraft {}  ·  {}  ·  Spielzeit {}",
                    acc.source,
                    config::TARGET_VERSION,
                    ver,
                    fmt_playtime(stats.playtime_secs),
                );
                ui.label(egui::RichText::new(line).size(12.5).color(c(DIM)));
            });
        }
    }
}

/// The signed-out prompt (used on Home when no account exists).
fn signed_out_card(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    card(ui, |ui| {
        ui.label(egui::RichText::new("Melde dich an").heading().color(c(TEXT)));
        ui.add_space(2.0);
        ui.label(egui::RichText::new("Mit deinem Microsoft-Konto anmelden — oder aus einem anderen Launcher übernehmen.").color(c(DIM)));
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if accent_button(ui, "Mit Microsoft anmelden", !app.busy) {
                app.add_microsoft(&ctx);
            }
            if ghost_button(ui, "Aus Launchern importieren", true) {
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
            egui::RichText::new("Die Sitzung konnte nicht automatisch übernommen werden. Bitte neu anmelden.")
                .color(c(DIM))
                .size(12.5),
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if accent_button(ui, "Neu anmelden", !app.busy) {
                app.add_microsoft(&ctx);
            }
            if ghost_button(ui, "Aus Launchern importieren", true) {
                app.import_accounts();
            }
        });
    });
    ui.add_space(14.0);
}

/* ---------------------------------------------------------------- */
/*  Accounts                                                         */
/* ---------------------------------------------------------------- */

fn accounts_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    page_head(ui, "Konten", "Mehrere Microsoft-Konten verwalten oder aus anderen Launchern übernehmen.");
    ui.add_space(12.0);

    relogin_banner(app, ui);

    ui.horizontal(|ui| {
        if accent_button(ui, "Microsoft-Konto hinzufügen", !app.busy) {
            app.add_microsoft(&ctx);
        }
        if ghost_button(ui, "Aus Launchern importieren", true) {
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
            ui.label(egui::RichText::new("Noch keine Konten. Füge eines hinzu oder importiere.").color(c(DIM)));
        });
    }
    let mut switch_to: Option<String> = None;
    let mut remove: Option<String> = None;
    for a in &accounts {
        let is_active = app.accounts.is_active(&a.uuid);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
        let p = ui.painter().clone();
        card_bg(&p, rect, is_active);
        let av = Rect::from_min_size(pos2(rect.left() + 14.0, rect.center().y - 19.0), vec2(38.0, 38.0));
        p.rect_filled(av, Rounding::same(8.0), c(BG_3));
        p.text(av.center(), Align2::CENTER_CENTER, initials(&a.username), FontId::proportional(14.0), c(TEXT));
        p.text(pos2(rect.left() + 64.0, rect.center().y - 9.0), Align2::LEFT_CENTER, &a.username, FontId::proportional(15.0), c(TEXT));
        let mut meta = a.source.clone();
        if !a.has_refresh {
            meta.push_str(" · Token temporär");
        }
        p.text(pos2(rect.left() + 64.0, rect.center().y + 10.0), Align2::LEFT_CENTER, meta, FontId::proportional(12.0), c(DIM));
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
/*  Cosmetics                                                        */
/* ---------------------------------------------------------------- */

const CAPES: &[(&str, &str, [u8; 3], [u8; 3])] = &[
    ("", "Keine Cape", [0x2A, 0x31, 0x3D], [0x1A, 0x1F, 0x27]),
    ("dolphin", "Dolphin", [0x35, 0xE0, 0xC8], [0x1E, 0x7E, 0x98]),
    ("ocean", "Ozean", [0x4F, 0x8C, 0xFF], [0x24, 0x3A, 0x8C]),
    ("aurora", "Aurora", [0x7C, 0x6C, 0xE0], [0x3E, 0x6E, 0xC0]),
    ("magma", "Magma", [0xE0, 0x7A, 0x45], [0xA0, 0x2E, 0x36]),
    ("founder", "Founder", [0xE0, 0xB2, 0x52], [0xB0, 0x78, 0x22]),
];

fn cosmetics_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    page_head(ui, "Cosmetics", "Wähle deine Cape. Die In-Game-Darstellung folgt in einem Update.");
    ui.add_space(12.0);

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
                    let (rect, resp) = ui.allocate_exact_size(vec2(cell_w, 150.0), Sense::click());
                    let hov = resp.hovered();
                    let p = ui.painter().clone();
                    p.rect_filled(rect, Rounding::same(ROUND), c(BG_2));
                    let sw = Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(rect.width() - 24.0, 90.0));
                    vertical_gradient(&p, sw, c(*top), c(*bottom), 9.0);
                    p.text(pos2(rect.left() + 14.0, rect.bottom() - 28.0), Align2::LEFT_CENTER, *name, FontId::proportional(14.0), c(TEXT));
                    p.text(
                        pos2(rect.right() - 14.0, rect.bottom() - 28.0),
                        Align2::RIGHT_CENTER,
                        if selected { "Aktiv" } else { "Wählen" },
                        FontId::proportional(12.0),
                        if selected { c(ACCENT) } else { c(DIM) },
                    );
                    let stroke = if selected {
                        Stroke::new(1.5, c(ACCENT))
                    } else if hov {
                        Stroke::new(1.0, c(LINE_2))
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
/*  Settings — launcher only                                         */
/* ---------------------------------------------------------------- */

fn settings_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    page_head(ui, "Einstellungen", "Änderungen werden automatisch gespeichert.");
    ui.add_space(12.0);

    card(ui, |ui| {
        field_label(ui, "Standard-Server", "Optional — der Client verbindet sich beim Start direkt. Leer = Serverauswahl im Spiel.");
        if ui
            .add(egui::TextEdit::singleline(&mut app.settings.server).hint_text("z. B. play.example.net").desired_width(f32::INFINITY))
            .changed()
        {
            app.settings.save();
        }
        ui.add_space(12.0);
        if toggle_row(ui, "Vollbild starten", "Spiel direkt im Vollbild öffnen.", &mut app.settings.fullscreen) {
            app.settings.save();
            app.gameopts.set_fullscreen(app.settings.fullscreen);
            app.gameopts.save();
        }
        toggle_row(ui, "Launcher nach Start schließen", "Fenster schließen, sobald das Spiel läuft.", &mut app.settings.close_on_launch)
            .then(|| app.settings.save());
        toggle_row(ui, "Automatische Updates", "Beim Start nach Launcher-Updates suchen.", &mut app.settings.auto_update)
            .then(|| app.settings.save());
        toggle_row(ui, "Discord Rich Presence", "Zeigt DolphinClient in deinem Discord-Profil, solange der Launcher offen ist.", &mut app.settings.discord_rpc)
            .then(|| app.settings.save());
    });
    ui.add_space(12.0);

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

fn field_label(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.label(egui::RichText::new(title).strong().color(c(TEXT)));
    ui.label(egui::RichText::new(sub).color(c(DIM)).size(12.0));
    ui.add_space(4.0);
}

fn toggle_row(ui: &mut egui::Ui, title: &str, sub: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.add_space(6.0);
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
        .fill(c(BG_2))
        .stroke(Stroke::new(1.0, c(LINE)))
        .rounding(Rounding::same(ROUND))
        .inner_margin(egui::Margin::same(18.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    let _ = section_title; // kept for future sections
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

fn draw_avatar(ui: &egui::Ui, app: &DolphinApp, rect: Rect) {
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(9.0), c(BG_3));
    let tex = app.avatar.lock().ok().and_then(|a| a.clone());
    match (tex, app.accounts.active_account()) {
        (Some(tex), Some(_)) => {
            p.image(tex.id(), rect.shrink(3.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        (_, Some(a)) => {
            p.text(rect.center(), Align2::CENTER_CENTER, initials(&a.username), FontId::proportional(15.0), c(TEXT));
        }
        _ => {
            p.text(rect.center(), Align2::CENTER_CENTER, "?", FontId::proportional(15.0), c(DIM));
        }
    }
    p.rect_stroke(rect, Rounding::same(9.0), Stroke::new(1.0, c(LINE_2)));
}

fn progress_bar(painter: &egui::Painter, rect: Rect, t: f32) {
    painter.rect_filled(rect, Rounding::same(2.0), c(BG_3));
    let w = rect.width() * t.clamp(0.02, 1.0);
    painter.rect_filled(Rect::from_min_size(rect.min, vec2(w, rect.height())), Rounding::same(2.0), c(ACCENT));
}

/// A smooth vertical gradient (single mesh) — used only for cape swatches.
fn vertical_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32, round: f32) {
    use egui::epaint::{Mesh, Vertex, WHITE_UV};
    painter.rect_filled(rect, Rounding::same(round), bottom);
    let mut mesh = Mesh::default();
    let v = |p: Pos2, col: Color32| Vertex { pos: p, uv: WHITE_UV, color: col };
    mesh.vertices.push(v(rect.left_top(), top));
    mesh.vertices.push(v(rect.right_top(), top));
    mesh.vertices.push(v(rect.right_bottom(), bottom));
    mesh.vertices.push(v(rect.left_bottom(), bottom));
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
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
/*  Icons                                                            */
/* ---------------------------------------------------------------- */

#[derive(Clone, Copy)]
enum Icon {
    Home,
    User,
    Cape,
    Gear,
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
    }
}
