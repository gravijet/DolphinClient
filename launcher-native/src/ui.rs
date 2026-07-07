//! The launcher UI, drawn like the real Minecraft menus: dirt background,
//! nine-sliced stone buttons, the game's bitmap font (once the jar is cached)
//! and a custom Minecraft-styled title bar with minimize/maximize/close —
//! the window itself is frameless.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, Rect, Sense, Stroke, ViewportCommand, pos2, vec2,
};

use crate::app::{DolphinApp, LoginMethod, Tab};
use crate::config::{self, TARGET_VERSION};
use crate::mcui::{BTN_H, S};

fn mcui_btn_h() -> f32 {
    BTN_H
}

/// Vanilla text colors.
const WHITE: Color32 = Color32::WHITE;
const GRAY: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);
const YELLOW: Color32 = Color32::from_rgb(0xFF, 0xFF, 0x55);
const GREEN: Color32 = Color32::from_rgb(0x55, 0xFF, 0x55);
const RED: Color32 = Color32::from_rgb(0xFF, 0x55, 0x55);
const AQUA: Color32 = Color32::from_rgb(0x55, 0xFF, 0xFF);

const TITLEBAR_H: f32 = 32.0;
const BOTTOM_H: f32 = 64.0;

pub fn install_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Color32::from_rgb(0xE0, 0xE0, 0xE0));
    visuals.hyperlink_color = AQUA;
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.selection.bg_fill = Color32::from_rgb(50, 70, 120);
    ctx.set_visuals(visuals);
}

/// Draw the whole frame. Called from `DolphinApp::update`.
pub fn draw(app: &mut DolphinApp, ctx: &egui::Context) {
    title_bar(app, ctx);
    bottom_bar(app, ctx);

    egui::CentralPanel::default()
        .frame(egui::Frame::none())
        .show(ctx, |ui| {
            // Dirt behind everything in the content area.
            app.mc.dirt_background(ui.painter(), ui.max_rect(), S);
            tabs_row(app, ui);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match app.tab {
                    Tab::Home => home_view(app, ui),
                    Tab::Accounts => accounts_view(app, ui),
                    Tab::Settings => settings_view(app, ui),
                });
        });
}

/* ---------------------------------------------------------------- */
/*  Custom title bar (frameless window)                              */
/* ---------------------------------------------------------------- */

fn title_bar(app: &mut DolphinApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("titlebar")
        .exact_height(TITLEBAR_H)
        .frame(egui::Frame::none().fill(Color32::from_rgb(18, 14, 10)))
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            let painter = ui.painter().clone();
            // Subtle bottom edge like an inventory border.
            painter.line_segment(
                [bar.left_bottom(), bar.right_bottom()],
                Stroke::new(2.0, Color32::BLACK),
            );

            // Window buttons (right → left): close, maximize, minimize.
            let mut x = bar.right() - 6.0;
            let close = window_button(ui, &mut x, bar, WindowButton::Close);
            let maxi = window_button(ui, &mut x, bar, WindowButton::Maximize);
            let mini = window_button(ui, &mut x, bar, WindowButton::Minimize);
            if close {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            if maxi {
                let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!is_max));
            }
            if mini {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            }

            // Logo + title (left).
            let logo = Rect::from_center_size(
                pos2(bar.left() + 18.0, bar.center().y),
                vec2(22.0, 22.0),
            );
            painter.image(
                app.mc.logo.id(),
                logo,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            app.mc.text_anchored(
                &painter,
                pos2(bar.left() + 34.0, bar.center().y),
                Align2::LEFT_CENTER,
                "DolphinClient Launcher",
                1.6,
                WHITE,
                true,
            );

            // Drag area: everything left of the window buttons.
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

/// One 26×20 title-bar button, right-aligned at `*x` (which moves left).
/// Returns true on click. Symbols are painted (not font glyphs) so they stay
/// crisp regardless of which font is loaded.
fn window_button(ui: &mut egui::Ui, x: &mut f32, bar: Rect, kind: WindowButton) -> bool {
    let size = vec2(26.0, 20.0);
    let rect = Rect::from_min_size(pos2(*x - size.x, bar.center().y - size.y / 2.0), size);
    *x -= size.x + 4.0;
    let resp = ui.interact(rect, ui.id().with(format!("winbtn{}", *x as i32)), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();
    let fill = match (&kind, hovered) {
        (WindowButton::Close, true) => Color32::from_rgb(170, 40, 40),
        (_, true) => Color32::from_rgb(90, 90, 90),
        _ => Color32::from_rgb(50, 45, 40),
    };
    painter.rect_filled(rect, 2.0, fill);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, Color32::BLACK));
    let c = rect.center();
    let s = Stroke::new(1.6, WHITE);
    match kind {
        WindowButton::Minimize => {
            painter.line_segment([c + vec2(-5.0, 3.0), c + vec2(5.0, 3.0)], s);
        }
        WindowButton::Maximize => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(9.0, 9.0)), 0.0, s);
        }
        WindowButton::Close => {
            painter.line_segment([c + vec2(-4.5, -4.5), c + vec2(4.5, 4.5)], s);
            painter.line_segment([c + vec2(-4.5, 4.5), c + vec2(4.5, -4.5)], s);
        }
    }
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}

/* ---------------------------------------------------------------- */
/*  Tabs                                                             */
/* ---------------------------------------------------------------- */

fn tabs_row(app: &mut DolphinApp, ui: &mut egui::Ui) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let total = 3.0 * 98.0 * S + 2.0 * 8.0;
        ui.add_space((ui.available_width() - total).max(0.0) / 2.0);
        ui.spacing_mut().item_spacing.x = 8.0;
        let n = app.accounts.accounts.len();
        let tabs: [(Tab, String); 3] = [
            (Tab::Home, "Spielen".to_string()),
            (Tab::Accounts, format!("Konten ({n})")),
            (Tab::Settings, "Einstellungen".to_string()),
        ];
        for (tab, label) in tabs {
            let active = app.tab == tab;
            let resp = app.mc.button_sized(ui, 98.0, mcui_btn_h(), S, &label, true);
            if active {
                // Yellow underline marks the selected tab.
                let r = resp.rect;
                ui.painter().rect_filled(
                    Rect::from_min_max(
                        pos2(r.left() + 2.0, r.bottom() - 4.0),
                        pos2(r.right() - 2.0, r.bottom() - 2.0),
                    ),
                    0.0,
                    YELLOW,
                );
            } else if resp.clicked() {
                app.tab = tab;
            }
        }
    });
    ui.add_space(8.0);
}

/* ---------------------------------------------------------------- */
/*  Bottom play bar                                                  */
/* ---------------------------------------------------------------- */

fn bottom_bar(app: &mut DolphinApp, ctx: &egui::Context) {
    let active = app.accounts.active_account().cloned();
    egui::TopBottomPanel::bottom("play")
        .exact_height(BOTTOM_H)
        .frame(egui::Frame::none().fill(Color32::from_rgb(18, 14, 10)))
        .show(ctx, |ui| {
            let bar = ui.max_rect();
            let painter = ui.painter().clone();
            painter.line_segment(
                [bar.left_top(), bar.right_top()],
                Stroke::new(2.0, Color32::BLACK),
            );

            // Right: the big play button (Minecraft button, extra tall).
            let btn_w = 130.0;
            let btn_h = 24.0;
            let label = if active.is_some() { "Spielen" } else { "Anmelden" };
            let btn_rect = Rect::from_min_size(
                pos2(
                    bar.right() - btn_w * S - 14.0,
                    bar.center().y - btn_h * S / 2.0,
                ),
                vec2(btn_w * S, btn_h * S),
            );
            let clicked = ui
                .allocate_new_ui(egui::UiBuilder::new().max_rect(btn_rect), |ui| {
                    app.mc
                        .button_sized(ui, btn_w, btn_h, S, label, !app.busy)
                        .clicked()
                })
                .inner;
            if clicked && !app.busy {
                if active.is_some() {
                    app.start_launch(ctx);
                } else {
                    app.add_microsoft(ctx);
                }
            }

            // Left: account line + status + progress.
            let left = bar.left() + 14.0;
            let right = btn_rect.left() - 16.0;
            let acct = match &active {
                Some(a) => format!("Konto: {}", a.username),
                None => "Kein Konto angemeldet".to_string(),
            };
            app.mc.text(
                &painter,
                pos2(left, bar.top() + 9.0),
                &acct,
                1.4,
                if active.is_some() { GREEN } else { GRAY },
                true,
            );
            let status_color = if app.status.starts_with("Fehler") { RED } else { WHITE };
            app.mc.text(
                &painter,
                pos2(left, bar.top() + 25.0),
                &app.status,
                1.4,
                status_color,
                true,
            );
            if app.busy || app.progress > 0.0 {
                let pr = Rect::from_min_max(
                    pos2(left, bar.bottom() - 16.0),
                    pos2(right.max(left + 60.0), bar.bottom() - 8.0),
                );
                mc_progress(&painter, pr, app.progress);
            }
        });
}

/// A Minecraft-style progress bar: black box, grey border, XP-green fill.
fn mc_progress(painter: &egui::Painter, rect: Rect, t: f32) {
    painter.rect_filled(rect, 0.0, Color32::BLACK);
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, GRAY));
    let w = (rect.width() - 4.0) * t.clamp(0.0, 1.0);
    if w > 0.5 {
        painter.rect_filled(
            Rect::from_min_size(rect.min + vec2(2.0, 2.0), vec2(w, rect.height() - 4.0)),
            0.0,
            Color32::from_rgb(0x80, 0xFF, 0x20),
        );
    }
}

/* ---------------------------------------------------------------- */
/*  Panels & shared bits                                             */
/* ---------------------------------------------------------------- */

/// A translucent black panel like vanilla's list backgrounds.
fn mc_panel(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(Color32::from_black_alpha(140))
        .stroke(Stroke::new(2.0, Color32::from_black_alpha(190)))
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, add);
}

/// Device-code + browser sign-in prompts (shared by Home and Accounts).
fn login_prompts(app: &mut DolphinApp, ui: &mut egui::Ui) {
    if let Some((link, code)) = app.device.clone() {
        mc_panel(ui, |ui| {
            app.mc.label(ui, 1.4, "Ein Browser-Fenster wurde geöffnet — dort anmelden.", GRAY);
            ui.add_space(4.0);
            app.mc.label(ui, 1.4, "Code (im Link bereits enthalten):", GRAY);
            app.mc.label(ui, 2.4, &code, AQUA);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if app.mc.button(ui, 110.0, 1.6, "Link erneut öffnen", true) {
                    let _ = open::that(&link);
                }
                if app.mc.button(ui, 90.0, 1.6, "Code kopieren", true) {
                    ui.output_mut(|o| o.copied_text = code.clone());
                }
            });
        });
        ui.add_space(8.0);
    }
    if let Some(url) = app.auth_url.clone() {
        mc_panel(ui, |ui| {
            app.mc.label(ui, 1.4, "Ein Browser-Fenster wurde geöffnet — dort anmelden.", GRAY);
            ui.add_space(4.0);
            if app.mc.button(ui, 130.0, 1.6, "Browser erneut öffnen", true) {
                let _ = open::that(&url);
            }
        });
        ui.add_space(8.0);
    }
}

/// Cycle button through "Neueste" and every archived client version.
/// Click steps forward; the choice is saved immediately.
fn version_picker(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let versions = app
        .versions
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();
    let current = app.settings.client_version.clone();
    let label = if current.is_empty() {
        "Client-Version: Neueste".to_string()
    } else {
        format!("Client-Version: {current}")
    };
    ui.horizontal(|ui| {
        if app.mc.button(ui, 170.0, 1.8, &label, true) {
            // "" → versions[0] → versions[1] → … → "" (wrap around).
            let next = if current.is_empty() {
                versions.first().cloned().unwrap_or_default()
            } else {
                match versions.iter().position(|v| *v == current) {
                    Some(i) if i + 1 < versions.len() => versions[i + 1].clone(),
                    _ => String::new(),
                }
            };
            app.settings.client_version = next;
            app.settings.save();
        }
        if !current.is_empty() {
            app.mc.label(ui, 1.3, "(ältere Version angepinnt)", YELLOW);
        }
    });
    if versions.is_empty() && !current.is_empty() {
        app.mc.label(ui, 1.3, "Archiv offline - nutzt lokalen Cache, falls vorhanden.", GRAY);
    }
}

fn log_box(app: &DolphinApp, ui: &mut egui::Ui) {
    egui::Frame::none()
        .fill(Color32::from_black_alpha(200))
        .stroke(Stroke::new(1.0, Color32::BLACK))
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(150.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if app.log.is_empty() {
                        app.mc.label(ui, 1.3, "- noch keine Ausgaben -", GRAY);
                    }
                    for line in &app.log {
                        ui.label(
                            egui::RichText::new(line)
                                .monospace()
                                .size(11.0)
                                .color(Color32::from_rgb(0xd0, 0xd0, 0xd0)),
                        );
                    }
                });
        });
}

/// Centered fixed-width column for all content views.
fn content_column(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let w = 620.0_f32.min(ui.available_width() - 24.0);
    ui.horizontal(|ui| {
        ui.add_space((ui.available_width() - w).max(0.0) / 2.0);
        ui.allocate_ui_with_layout(
            vec2(w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            add,
        );
    });
}

/* ---------------------------------------------------------------- */
/*  Home                                                             */
/* ---------------------------------------------------------------- */

fn home_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let active = app.accounts.active_account().cloned();
    let ctx = ui.ctx().clone();
    content_column(ui, |ui| {
        // Logo + wordmark, like the game's title screen.
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(96.0, 96.0), Sense::hover());
            ui.painter().image(
                app.mc.logo.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            ui.add_space(6.0);
            app.mc.label(ui, 3.0, "DolphinClient", WHITE);
            ui.add_space(2.0);
            app.mc.label(
                ui,
                1.5,
                &format!("Nativer Minecraft-{TARGET_VERSION}-Client - maximale FPS"),
                GRAY,
            );
            let note = app.update_note.lock().ok().and_then(|n| n.clone());
            if let Some(info) = note {
                ui.add_space(4.0);
                app.mc.label(ui, 1.5, &format!("Update {} verfügbar!", info.version), YELLOW);
                ui.add_space(2.0);
                if app
                    .mc
                    .button(ui, 180.0, 1.8, &format!("Update {} installieren", info.version), !app.busy)
                {
                    app.start_self_update(&ctx, info);
                }
            }
        });
        ui.add_space(14.0);

        login_prompts(app, ui);

        if active.is_none() {
            mc_panel(ui, |ui| {
                app.mc.label(ui, 1.6, "Anmelden", WHITE);
                app.mc.label(
                    ui,
                    1.4,
                    "Mit deinem Microsoft-Konto anmelden, um online zu spielen.",
                    GRAY,
                );
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    if app.mc.button(ui, 200.0, S, "Mit Microsoft anmelden", !app.busy) {
                        app.add_microsoft(&ctx);
                    }
                    ui.add_space(2.0);
                    if app.mc.button(ui, 200.0, S, "Aus anderen Launchern importieren", true) {
                        app.import_accounts();
                    }
                    if crate::tokens::has_token()
                        && app.mc.button(ui, 200.0, S, "Vorheriges Konto wiederherstellen", true)
                    {
                        app.start_login(&ctx, LoginMethod::Refresh);
                    }
                });
                if let Some(note) = &app.import_note {
                    ui.add_space(4.0);
                    app.mc.label(ui, 1.4, note, GREEN);
                }
            });
        } else if let Some(a) = &active {
            mc_panel(ui, |ui| {
                app.mc.label(ui, 1.6, &format!("Willkommen zurück, {}!", a.username), WHITE);
                app.mc.label(
                    ui,
                    1.4,
                    "Unten rechts auf Spielen klicken - der Client verbindet sich \
                     mit deiner Minecraft-Session.",
                    GRAY,
                );
                ui.add_space(6.0);
                version_picker(app, ui);
            });
        }

        ui.add_space(10.0);

        // Feature list, kept in the pixel look.
        mc_panel(ui, |ui| {
            app.mc.label(ui, 1.6, "Warum DolphinClient?", WHITE);
            ui.add_space(4.0);
            for (title, desc) in [
                ("Maximale FPS", "Nativer Rust-Client mit wgpu - kein Java, kein Limit."),
                ("Echter Vanilla-Look", "Originale Texturen, Sounds und Menüs wie im Spiel."),
                ("Multiplayer 26.1", "Direkt auf jeden 26.1-Server verbinden."),
                ("Auto-Update", "Launcher und Client halten sich selbst aktuell."),
            ] {
                ui.horizontal(|ui| {
                    app.mc.label(ui, 1.4, "*", YELLOW);
                    app.mc.label(ui, 1.4, title, WHITE);
                    app.mc.label(ui, 1.4, "-", GRAY);
                    app.mc.label(ui, 1.4, desc, GRAY);
                });
            }
        });

        ui.add_space(10.0);
        let log_label = if app.show_log { "Protokoll ausblenden" } else { "Protokoll anzeigen" };
        if app.mc.button(ui, 130.0, 1.6, log_label, true) {
            app.show_log = !app.show_log;
        }
        if app.show_log {
            ui.add_space(4.0);
            log_box(app, ui);
        }
        ui.add_space(16.0);
    });
}

/* ---------------------------------------------------------------- */
/*  Accounts                                                         */
/* ---------------------------------------------------------------- */

fn accounts_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    content_column(ui, |ui| {
        ui.add_space(8.0);
        app.mc.label(ui, 2.0, "Konten", WHITE);
        app.mc.label(
            ui,
            1.4,
            "Mehrere Microsoft-Konten verwalten oder aus anderen Launchern importieren.",
            GRAY,
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if app.mc.button(ui, 160.0, 1.8, "Microsoft-Konto hinzufügen", !app.busy) {
                app.add_microsoft(&ctx);
            }
            if app.mc.button(ui, 150.0, 1.8, "Aus Launchern importieren", true) {
                app.import_accounts();
            }
        });
        app.mc.label(
            ui,
            1.2,
            "Import: Vanilla-Launcher & Lunar Client. Badlion/Feather verschlüsseln ihre Tokens.",
            GRAY,
        );
        if let Some(note) = &app.import_note {
            app.mc.label(ui, 1.4, note, GREEN);
        }
        ui.add_space(8.0);

        login_prompts(app, ui);

        let accounts = app.accounts.accounts.clone();
        if accounts.is_empty() {
            mc_panel(ui, |ui| {
                app.mc.label(ui, 1.4, "Noch keine Konten. Füge eines hinzu oder importiere.", GRAY);
            });
        }
        let mut switch_to: Option<String> = None;
        let mut remove: Option<String> = None;
        for a in &accounts {
            let is_active = app.accounts.is_active(&a.uuid);
            mc_panel(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    app.mc.label(ui, 1.7, &a.username, if is_active { GREEN } else { WHITE });
                    let mut meta = a.source.clone();
                    if !a.has_refresh {
                        meta.push_str(" (Token temporär)");
                    }
                    app.mc.label(ui, 1.3, &meta, GRAY);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if app.mc.button(ui, 70.0, 1.6, "Entfernen", true) {
                            remove = Some(a.uuid.clone());
                        }
                        if is_active {
                            app.mc.label(ui, 1.4, "Aktiv", GREEN);
                        } else if app.mc.button(ui, 70.0, 1.6, "Auswählen", true) {
                            switch_to = Some(a.uuid.clone());
                        }
                    });
                });
            });
            ui.add_space(4.0);
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
        ui.add_space(16.0);
    });
}

/* ---------------------------------------------------------------- */
/*  Settings                                                         */
/* ---------------------------------------------------------------- */

fn settings_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    content_column(ui, |ui| {
        ui.add_space(8.0);
        app.mc.label(ui, 2.0, "Einstellungen", WHITE);
        ui.add_space(8.0);

        let mut changed = false;
        mc_panel(ui, |ui| {
            app.mc.label(ui, 1.5, "Standard-Server", WHITE);
            app.mc.label(
                ui,
                1.3,
                "Server, dem der Client beim Start beitritt. Leer = Serverauswahl im Spiel.",
                GRAY,
            );
            changed |= app
                .mc
                .text_field(ui, 200.0, S, &mut app.settings.server, "z. B. play.example.net")
                .changed();

            ui.add_space(10.0);
            app.mc.label(ui, 1.5, "Java (nur für den klassischen Java-Start)", WHITE);
            changed |= app
                .mc
                .text_field(ui, 200.0, S, &mut app.settings.java_path, "leer = java aus PATH")
                .changed();

            ui.add_space(10.0);
            {
                let mut t = (app.settings.ram_gb.clamp(2, 16) as f32 - 2.0) / 14.0;
                let label = format!("RAM: {} GB", app.settings.ram_gb);
                if app.mc.slider(ui, 200.0, S, &label, &mut t) {
                    app.settings.ram_gb = (2.0 + t * 14.0).round() as u32;
                    changed = true;
                }
            }

            ui.add_space(10.0);
            let au = format!(
                "Auto-Update: {}",
                if app.settings.auto_update { "AN" } else { "AUS" }
            );
            if app.mc.button(ui, 200.0, S, &au, true) {
                app.settings.auto_update = !app.settings.auto_update;
                changed = true;
            }
            let fs = format!(
                "Vollbild starten: {}",
                if app.settings.fullscreen { "AN" } else { "AUS" }
            );
            if app.mc.button(ui, 200.0, S, &fs, true) {
                app.settings.fullscreen = !app.settings.fullscreen;
                changed = true;
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if app.mc.button(ui, 90.0, 1.8, "Speichern", true) {
                app.settings.save();
                app.status = "Einstellungen gespeichert.".to_string();
            }
            if app.accounts.active_account().is_some()
                && app.mc.button(ui, 140.0, 1.8, "Aktives Konto abmelden", true)
            {
                app.remove_active();
            }
        });
        app.mc.label(
            ui,
            1.2,
            &format!("Spielordner: {}", config::minecraft_dir().display()),
            GRAY,
        );
        if changed {
            app.settings.save();
        }
        ui.add_space(16.0);
    });
}
