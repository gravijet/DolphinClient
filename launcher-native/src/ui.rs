//! The launcher UI — a calm, dark, serious Minecraft launcher.
//!
//! Layout: a slim title bar, a horizontal **text** tab header (Start · Konten ·
//! Einstellungen) with a live account chip on the right, and a content area. The
//! primary "Spielen" action lives on the Start page next to your character — no
//! left rail, no icon strip, no persistent bottom bar. Flat dark surfaces, one
//! aqua accent used only on the Play button and the active tab. Everything is
//! painted with egui's own widgets/painter; settings save on change.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Rect, Rounding, Sense, Stroke, ViewportCommand,
    pos2, vec2,
};

use crate::app::{DolphinApp, LoginMethod, Tab};
use crate::config;

/* ---------------------------------------------------------------- */
/*  Palette — flat, dark, one aqua accent                            */
/* ---------------------------------------------------------------- */

pub const BG_0: [u8; 3] = [0x0E, 0x11, 0x17]; // window
const BG_1: [u8; 3] = [0x12, 0x16, 0x1D]; // header / bars
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

fn running(app: &DolphinApp) -> bool {
    app.running.load(std::sync::atomic::Ordering::Relaxed)
}

const TITLEBAR_H: f32 = 46.0;
const TABSTRIP_H: f32 = 46.0;
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
    tab_strip(app, ctx);

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(c(BG_0)).inner_margin(egui::Margin::ZERO))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(24.0);
                    content_column(ui, |ui| match app.tab {
                        Tab::Home => home_view(app, ui),
                        Tab::Accounts => accounts_view(app, ui),
                        Tab::Settings => settings_view(app, ui),
                    });
                    ui.add_space(32.0);
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

            let drag_zone = Rect::from_min_max(pos2(bar.left() + 160.0, bar.top()), pos2(x - 4.0, bar.bottom()));
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
/*  Tab header — text tabs + account chip                            */
/* ---------------------------------------------------------------- */

fn tab_strip(app: &mut DolphinApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("tabs")
        .exact_height(TABSTRIP_H)
        .frame(egui::Frame::none().fill(c(BG_1)))
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            let painter = ui.painter().clone();
            painter.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, c(LINE)));

            let mut x = bar.left() + 18.0;
            for (label, tab) in [
                ("Start", Tab::Home),
                ("Konten", Tab::Accounts),
                ("Einstellungen", Tab::Settings),
            ] {
                if tab_button(ui, &painter, &mut x, bar, label, app.tab == tab) {
                    app.tab = tab;
                }
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
) -> bool {
    let font = FontId::proportional(14.5);
    let galley = painter.layout_no_wrap(label.to_string(), font.clone(), c(TEXT));
    let pad = 11.0;
    let w = galley.size().x + pad * 2.0;
    let rect = Rect::from_min_size(pos2(*x, bar.top()), vec2(w, bar.height()));
    *x += w + 6.0;
    let resp = ui.interact(rect, ui.id().with(("tab", label)), Sense::click());
    let hov = resp.hovered();
    let col = if active || hov { c(TEXT) } else { c(DIM) };
    painter.text(rect.center(), Align2::CENTER_CENTER, label, font, col);
    if active {
        let y = rect.bottom() - 2.0;
        painter.line_segment(
            [pos2(rect.left() + pad * 0.6, y), pos2(rect.right() - pad * 0.6, y)],
            Stroke::new(2.0, c(ACCENT)),
        );
    }
    if hov {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// The live account chip on the right of the tab header (avatar + name +
/// one-word state). Clicking it jumps to the Konten tab. Deliberately shows no
/// login origin — just who is signed in and whether they're ready to play.
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
/*  Home — a launch pad: your character + the Play action            */
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
            ui.add_space(4.0);
            // Character render, centered — no floor shadow.
            let body = app.body.lock().ok().and_then(|b| b.clone());
            let stage_h = 286.0;
            let (stage, _) = ui.allocate_exact_size(vec2(ui.available_width(), stage_h), Sense::hover());
            let p = ui.painter().clone();
            if let Some(tex) = body {
                let [tw, th] = tex.size();
                if tw > 0 && th > 0 {
                    let target_h = stage_h;
                    let target_w = target_h * tw as f32 / th as f32;
                    let bx = stage.center().x - target_w / 2.0;
                    p.image(
                        tex.id(),
                        Rect::from_min_size(pos2(bx, stage.top()), vec2(target_w, target_h)),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            } else {
                p.text(stage.center(), Align2::CENTER_CENTER, "…", FontId::proportional(20.0), c(FAINT));
            }

            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(&acc.username).size(26.0).strong().color(c(TEXT)));
                ui.add_space(3.0);
                let ver = if app.settings.client_version.is_empty() {
                    "Neueste".to_string()
                } else {
                    app.settings.client_version.clone()
                };
                let stats = app.stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
                let line = format!(
                    "Minecraft {}  ·  {}  ·  Spielzeit {}",
                    config::TARGET_VERSION,
                    ver,
                    fmt_playtime(stats.playtime_secs),
                );
                ui.label(egui::RichText::new(line).size(12.5).color(c(DIM)));

                ui.add_space(22.0);
                if play_button(ui, is_running, app.busy) && !app.busy && !is_running {
                    app.start_launch(&ctx);
                }

                ui.add_space(12.0);
                ui.allocate_ui_with_layout(vec2(210.0, 34.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                    version_combo(app, ui);
                });

                if !app.status.is_empty() {
                    ui.add_space(8.0);
                    let scol = if app.status.starts_with("Fehler") || app.relogin_for.is_some() {
                        c(RED)
                    } else if is_running {
                        c(GREEN)
                    } else {
                        c(DIM)
                    };
                    ui.label(egui::RichText::new(truncate(&app.status, 64)).size(12.0).color(scol));
                }
            });
        }
    }
}

fn play_button(ui: &mut egui::Ui, is_running: bool, busy: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(232.0, 54.0), Sense::click());
    let enabled = !busy && !is_running;
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
        let lx = rect.center().x - 56.0;
        painter.add(egui::Shape::convex_polygon(
            vec![pos2(lx, cy - 8.0), pos2(lx + 13.0, cy), pos2(lx, cy + 8.0)],
            fg,
            Stroke::NONE,
        ));
    }
    painter.text(pos2(txp, rect.center().y), Align2::CENTER_CENTER, label, FontId::new(16.5, egui::FontFamily::Proportional), fg);
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
        .width(206.0)
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
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::hover());
        let p = ui.painter().clone();
        card_bg(&p, rect, is_active);
        let av = Rect::from_min_size(pos2(rect.left() + 14.0, rect.center().y - 18.0), vec2(36.0, 36.0));
        p.rect_filled(av, Rounding::same(8.0), c(BG_3));
        p.text(av.center(), Align2::CENTER_CENTER, initials(&a.username), FontId::proportional(14.0), c(TEXT));
        p.text(pos2(rect.left() + 62.0, rect.center().y), Align2::LEFT_CENTER, &a.username, FontId::proportional(15.0), c(TEXT));
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
        section_title(ui, "Spiel");
        ui.add_space(10.0);
        field_label(ui, "Standard-Server", "Optional — der Client verbindet sich beim Start direkt. Leer lassen für die Serverauswahl im Spiel.");
        if ui
            .add(egui::TextEdit::singleline(&mut app.settings.server).hint_text("z. B. play.example.net").desired_width(f32::INFINITY))
            .changed()
        {
            app.settings.save();
        }
        if toggle_row(ui, "Vollbild starten", "Das Spiel direkt im Vollbild öffnen.", &mut app.settings.fullscreen) {
            app.settings.save();
            app.gameopts.set_fullscreen(app.settings.fullscreen);
            app.gameopts.save();
        }
        toggle_row(ui, "Launcher nach Start schließen", "Das Fenster schließen, sobald das Spiel läuft.", &mut app.settings.close_on_launch)
            .then(|| app.settings.save());
    });
    ui.add_space(12.0);

    card(ui, |ui| {
        section_title(ui, "Start & Updates");
        ui.add_space(4.0);
        if toggle_row(ui, "Mit dem System starten", "DolphinClient öffnet sich automatisch, sobald du dich am PC anmeldest.", &mut app.settings.autostart) {
            app.settings.save();
            let _ = crate::autostart::set(app.settings.autostart);
        }
        toggle_row(ui, "Nach Updates suchen", "Beim Start prüfen, ob eine neuere Version bereitsteht.", &mut app.settings.auto_update)
            .then(|| app.settings.save());
        toggle_row(ui, "Updates automatisch installieren", "Gefundene Updates ohne Nachfrage einspielen — der Launcher startet dafür kurz neu.", &mut app.settings.auto_update_apply)
            .then(|| app.settings.save());
    });
    ui.add_space(12.0);

    card(ui, |ui| {
        section_title(ui, "Discord");
        ui.add_space(4.0);
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
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}
