//! egui application: login, play, progress and settings — a small native UI.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

use crate::auth::Session;
use crate::config::{self, Settings, TARGET_VERSION};
use crate::events::Event;

const CYAN: egui::Color32 = egui::Color32::from_rgb(56, 225, 196);
const AQUA: egui::Color32 = egui::Color32::from_rgb(74, 198, 255);
const VIOLET: egui::Color32 = egui::Color32::from_rgb(124, 139, 255);
const MINT: egui::Color32 = egui::Color32::from_rgb(126, 240, 212);
const MUTED: egui::Color32 = egui::Color32::from_rgb(147, 167, 196);
const DANGER: egui::Color32 = egui::Color32::from_rgb(255, 154, 154);

#[derive(PartialEq, Eq)]
enum Tab {
    Home,
    Settings,
}

#[derive(Clone, Copy)]
enum LoginMethod {
    /// System browser + loopback redirect — just sign in, no code to type.
    Browser,
    /// Device-code fallback (open a page, type a code).
    Device,
    /// Silent login with the stored refresh token.
    Refresh,
}

pub struct DolphinApp {
    tx: Sender<Event>,
    rx: Receiver<Event>,

    session: Option<Session>,
    settings: Settings,

    status: String,
    progress: f32,
    busy: bool,
    device: Option<(String, String)>, // (url, code)
    auth_url: Option<String>,         // browser-login URL (for re-open)
    log: Vec<String>,
    show_log: bool,

    tab: Tab,
    has_saved_token: bool,
    update_note: Arc<Mutex<Option<String>>>,
}

impl DolphinApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_theme(&cc.egui_ctx);

        let (tx, rx) = channel();
        let settings = Settings::load();
        let has_saved_token = crate::tokens::has_token();

        // Background launcher-update check.
        let update_note = Arc::new(Mutex::new(None));
        if settings.auto_update {
            let note = update_note.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                if let Some(v) = crate::updater::check() {
                    if let Ok(mut n) = note.lock() {
                        *n = Some(v);
                    }
                    ctx.request_repaint();
                }
            });
        }

        Self {
            tx,
            rx,
            session: None,
            settings,
            status: "Nicht angemeldet".to_string(),
            progress: 0.0,
            busy: false,
            device: None,
            auth_url: None,
            log: Vec::new(),
            show_log: false,
            tab: Tab::Home,
            has_saved_token,
            update_note,
        }
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Status(s) => self.status = s,
                Event::Log(s) => {
                    self.log.push(s);
                    if self.log.len() > 200 {
                        self.log.remove(0);
                    }
                }
                Event::Progress(p) => self.progress = p.clamp(0.0, 1.0),
                Event::Device { url, code, message } => {
                    self.device = Some((url, code));
                    self.status = message;
                }
                Event::BrowserOpen { url } => {
                    self.auth_url = Some(url);
                }
                Event::LoggedIn(session) => {
                    self.status = format!("Angemeldet als {}", session.username);
                    self.session = Some(session);
                    self.device = None;
                    self.auth_url = None;
                    self.has_saved_token = true;
                }
                Event::Launched => self.status = "Minecraft läuft — viel Spaß! 🐬".to_string(),
                Event::Error(e) => {
                    self.status = format!("Fehler: {}", e);
                    self.device = None;
                    self.auth_url = None;
                }
                Event::Done => {
                    self.busy = false;
                    self.progress = 0.0;
                    self.auth_url = None;
                }
            }
        }
    }

    fn start_login(&mut self, ctx: &egui::Context, method: LoginMethod) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.device = None;
        self.auth_url = None;
        self.progress = 0.0;
        self.status = match method {
            LoginMethod::Refresh => "Automatische Anmeldung …",
            _ => "Anmeldung wird vorbereitet …",
        }
        .to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = match method {
                LoginMethod::Browser => crate::auth::login_via_browser(&tx),
                LoginMethod::Device => crate::auth::login_device(&tx),
                LoginMethod::Refresh => crate::auth::login_with_refresh(&tx),
            };
            match result {
                Ok(session) => {
                    let _ = tx.send(Event::LoggedIn(session));
                }
                Err(e) => {
                    let _ = tx.send(Event::Error(e.to_string()));
                }
            }
            let _ = tx.send(Event::Done);
            ctx.request_repaint();
        });
    }

    fn start_launch(&mut self, ctx: &egui::Context) {
        let session = match &self.session {
            Some(s) => s.clone(),
            None => return,
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.progress = 0.0;
        self.status = "Spielstart wird vorbereitet …".to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let settings = self.settings.clone();
        std::thread::spawn(move || {
            if let Err(e) = crate::game::launch(&session, &settings, &tx) {
                let _ = tx.send(Event::Error(e.to_string()));
            }
            let _ = tx.send(Event::Done);
            ctx.request_repaint();
        });
    }

    fn logout(&mut self) {
        crate::tokens::clear();
        self.session = None;
        self.has_saved_token = false;
        self.device = None;
        self.auth_url = None;
        self.status = "Abgemeldet.".to_string();
    }
}

impl eframe::App for DolphinApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        if self.busy {
            // Poll the worker channel while work is in flight.
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        top_bar(self, ctx);

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Home => home_view(self, ui, ctx),
            Tab::Settings => settings_view(self, ui),
        });
    }
}

/* ---------------------------------------------------------------- */
/*  Theme                                                            */
/* ---------------------------------------------------------------- */

fn install_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.hyperlink_color = CYAN;
    visuals.override_text_color = Some(egui::Color32::from_rgb(234, 243, 255));
    visuals.panel_fill = egui::Color32::from_rgb(6, 12, 22);
    visuals.window_fill = egui::Color32::from_rgb(9, 18, 31);
    visuals.extreme_bg_color = egui::Color32::from_rgb(4, 9, 16);
    visuals.selection.bg_fill = egui::Color32::from_rgb(24, 60, 78);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(20, 40, 56);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(26, 52, 70);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(16.0, 9.0);
    ctx.set_style(style);
}

/* ---------------------------------------------------------------- */
/*  Top bar                                                         */
/* ---------------------------------------------------------------- */

fn top_bar(app: &mut DolphinApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("top")
        .exact_height(58.0)
        .show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("🐬").size(24.0));
                ui.label(
                    egui::RichText::new("DolphinClient")
                        .size(20.0)
                        .strong()
                        .color(CYAN),
                );
                ui.label(
                    egui::RichText::new(format!("Minecraft {}", TARGET_VERSION))
                        .color(MUTED)
                        .size(13.0),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(6.0);
                    if ui
                        .selectable_label(app.tab == Tab::Settings, "  ⚙ Einstellungen  ")
                        .clicked()
                    {
                        app.tab = Tab::Settings;
                    }
                    if ui
                        .selectable_label(app.tab == Tab::Home, "  🏠 Start  ")
                        .clicked()
                    {
                        app.tab = Tab::Home;
                    }

                    let note = app.update_note.lock().ok().and_then(|n| n.clone());
                    if let Some(v) = note {
                        ui.label(
                            egui::RichText::new(format!("⬆ Update {}", v))
                                .color(MINT)
                                .size(12.0),
                        );
                    }
                });
            });
        });
}

/* ---------------------------------------------------------------- */
/*  Home view                                                       */
/* ---------------------------------------------------------------- */

fn home_view(app: &mut DolphinApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.add_space(18.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new("Mehr FPS, weniger Aufwand.")
                .size(30.0)
                .strong(),
        );
        ui.label(
            egui::RichText::new("Ein Klick — Login, Fabric-Setup und 26.1-Start.")
                .size(15.0)
                .color(MUTED),
        );
    });
    ui.add_space(22.0);

    let card = egui::Frame::none()
        .fill(egui::Color32::from_rgb(12, 22, 36))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(38, 58, 82)))
        .rounding(16.0)
        .inner_margin(egui::Margin::same(22.0));

    let max_w = 560.0_f32.min(ui.available_width() - 24.0);
    ui.vertical_centered(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(max_w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                card.show(ui, |ui| {
                    ui.set_width(max_w - 44.0);

                    // Player row when logged in.
                    if let Some(session) = &app.session {
                        ui.horizontal(|ui| {
                            avatar(ui, &session.username);
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(&session.username).size(18.0).strong(),
                                );
                                ui.label(
                                    egui::RichText::new("● Angemeldet über Microsoft")
                                        .color(MINT)
                                        .size(12.0),
                                );
                            });
                        });
                        ui.add_space(14.0);
                    }

                    // Device-code prompt.
                    if let Some((url, code)) = app.device.clone() {
                        ui.label(
                            egui::RichText::new("Zum Anmelden diese Seite öffnen:").color(MUTED),
                        );
                        ui.add_space(4.0);
                        ui.hyperlink_to(url.clone(), url.clone());
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("und diesen Code eingeben:").color(MUTED));
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(&code)
                                .size(34.0)
                                .strong()
                                .color(CYAN)
                                .monospace(),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("🌐  Im Browser öffnen").clicked() {
                                let _ = open::that(&url);
                            }
                            if ui.button("📋  Code kopieren").clicked() {
                                ui.output_mut(|o| o.copied_text = code.clone());
                            }
                        });
                        ui.add_space(12.0);
                    }

                    // Browser sign-in in progress.
                    if let Some(url) = app.auth_url.clone() {
                        ui.label(
                            egui::RichText::new(
                                "Ein Browser-Fenster wurde geöffnet — melde dich dort mit Microsoft an.",
                            )
                            .color(MUTED),
                        );
                        ui.add_space(6.0);
                        if ui.button("🌐  Browser erneut öffnen").clicked() {
                            let _ = open::that(&url);
                        }
                        ui.add_space(12.0);
                    }

                    // Primary action.
                    let accent = egui::Color32::from_rgb(50, 210, 200);
                    ui.add_enabled_ui(!app.busy, |ui| {
                        if app.session.is_some() {
                            let btn = egui::Button::new(
                                egui::RichText::new(format!("▶  Spielen ({})", TARGET_VERSION))
                                    .size(18.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(4, 18, 27)),
                            )
                            .fill(accent)
                            .min_size(egui::vec2(ui.available_width(), 46.0));
                            if ui.add(btn).clicked() {
                                app.start_launch(ctx);
                            }
                        } else {
                            let btn = egui::Button::new(
                                egui::RichText::new("Mit Microsoft anmelden")
                                    .size(17.0)
                                    .strong()
                                    .color(egui::Color32::from_rgb(4, 18, 27)),
                            )
                            .fill(accent)
                            .min_size(egui::vec2(ui.available_width(), 46.0));
                            if ui.add(btn).clicked() {
                                // Live backend → device code; own Azure app → browser.
                                let method = if crate::auth::is_azure() {
                                    LoginMethod::Browser
                                } else {
                                    LoginMethod::Device
                                };
                                app.start_login(ctx, method);
                            }
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                if crate::auth::is_azure()
                                    && ui.small_button("Anmeldung per Code").clicked()
                                {
                                    app.start_login(ctx, LoginMethod::Device);
                                }
                                if app.has_saved_token
                                    && ui.small_button("Automatisch anmelden").clicked()
                                {
                                    app.start_login(ctx, LoginMethod::Refresh);
                                }
                            });
                        }
                    });

                    ui.add_space(12.0);

                    // Progress + status.
                    if app.busy || app.progress > 0.0 {
                        ui.add(
                            egui::ProgressBar::new(app.progress)
                                .desired_width(ui.available_width())
                                .animate(app.busy)
                                .show_percentage(),
                        );
                        ui.add_space(6.0);
                    }
                    let status_color = if app.status.starts_with("Fehler") {
                        DANGER
                    } else {
                        MUTED
                    };
                    ui.label(egui::RichText::new(&app.status).color(status_color));

                    if app.session.is_some() {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.small_button("Abmelden").clicked() {
                                app.logout();
                            }
                        });
                    }
                });

                // Log toggle.
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut app.show_log, "Details anzeigen");
                });
                if app.show_log {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(4, 9, 16))
                        .rounding(10.0)
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(140.0)
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    if app.log.is_empty() {
                                        ui.label(
                                            egui::RichText::new("— noch keine Ausgaben —")
                                                .color(MUTED),
                                        );
                                    }
                                    for line in &app.log {
                                        ui.label(egui::RichText::new(line).monospace().size(12.0));
                                    }
                                });
                        });
                }
            },
        );
    });

    // Bottom feature strip.
    ui.add_space(18.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(
                "Nativ in Rust · Original-Dateien von Mojang · Fabric + Mods automatisch",
            )
            .color(MUTED)
            .size(12.0),
        );
    });
}

fn avatar(ui: &mut egui::Ui, name: &str) {
    let size = egui::vec2(44.0, 44.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 12.0, VIOLET.linear_multiply(0.9));
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), rect.height() / 2.0)),
        12.0,
        AQUA.linear_multiply(0.9),
    );
    let initials: String = name.chars().take(2).collect::<String>().to_uppercase();
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials,
        egui::FontId::proportional(18.0),
        egui::Color32::from_rgb(4, 18, 27),
    );
}

/* ---------------------------------------------------------------- */
/*  Settings view                                                   */
/* ---------------------------------------------------------------- */

fn settings_view(app: &mut DolphinApp, ui: &mut egui::Ui) {
    ui.add_space(16.0);
    ui.label(egui::RichText::new("Einstellungen").size(24.0).strong());
    ui.add_space(12.0);

    let mut changed = false;

    egui::Frame::none()
        .fill(egui::Color32::from_rgb(12, 22, 36))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(38, 58, 82)))
        .rounding(14.0)
        .inner_margin(egui::Margin::same(18.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Arbeitsspeicher").strong());
            ui.label(
                egui::RichText::new("Dem Spiel zugewiesener Heap (-Xmx).")
                    .color(MUTED)
                    .size(12.0),
            );
            changed |= ui
                .add(
                    egui::Slider::new(&mut app.settings.ram_gb, 2..=16)
                        .suffix(" GB")
                        .text("RAM"),
                )
                .changed();

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);

            ui.label(egui::RichText::new("Java (JDK 25)").strong());
            ui.label(
                egui::RichText::new("Pfad zur java-Binärdatei. Leer = \"java\" aus dem PATH.")
                    .color(MUTED)
                    .size(12.0),
            );
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut app.settings.java_path)
                        .hint_text("z. B. C:\\Program Files\\Java\\jdk-25\\bin\\java.exe")
                        .desired_width(ui.available_width()),
                )
                .changed();

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);

            changed |= ui
                .checkbox(
                    &mut app.settings.auto_update,
                    "Beim Start auf Updates prüfen",
                )
                .changed();
            changed |= ui
                .checkbox(&mut app.settings.fullscreen, "Spiel im Vollbild starten")
                .changed();
        });

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        if ui.button("💾  Speichern").clicked() {
            app.settings.save();
            app.status = "Einstellungen gespeichert.".to_string();
        }
        ui.label(
            egui::RichText::new(format!(
                "Spielordner: {}",
                config::minecraft_dir().display()
            ))
            .color(MUTED)
            .size(12.0),
        );
    });

    if changed {
        // Auto-persist so a crash never loses tweaks.
        app.settings.save();
    }
}
