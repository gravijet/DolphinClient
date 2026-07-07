//! egui application: multi-account login, play, progress and settings — a small
//! native UI in the spirit of a modern client launcher (Home / Accounts /
//! Settings). Accounts can be added via Microsoft or imported from other
//! launchers already installed on this device.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

use crate::accounts::{Account, AccountStore};
use crate::auth::Session;
use crate::config::{self, Settings, TARGET_VERSION};
use crate::events::Event;

const CYAN: egui::Color32 = egui::Color32::from_rgb(56, 225, 196);
const AQUA: egui::Color32 = egui::Color32::from_rgb(74, 198, 255);
const VIOLET: egui::Color32 = egui::Color32::from_rgb(124, 139, 255);
const MINT: egui::Color32 = egui::Color32::from_rgb(126, 240, 212);
const MUTED: egui::Color32 = egui::Color32::from_rgb(150, 165, 190);
const DANGER: egui::Color32 = egui::Color32::from_rgb(255, 154, 154);
const CARD_FILL: egui::Color32 = egui::Color32::from_rgb(18, 25, 40);
const CARD_SOFT: egui::Color32 = egui::Color32::from_rgb(21, 29, 46);
const CARD_STROKE: egui::Color32 = egui::Color32::from_rgb(40, 52, 76);
const SIDEBAR: egui::Color32 = egui::Color32::from_rgb(11, 15, 25);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(56, 189, 248);
const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(96, 208, 255);
const ACCENT_SOFT: egui::Color32 = egui::Color32::from_rgb(24, 44, 68);
const INK: egui::Color32 = egui::Color32::from_rgb(6, 14, 22);
const WHITE: egui::Color32 = egui::Color32::from_rgb(236, 243, 255);

#[derive(PartialEq, Eq)]
enum Tab {
    Home,
    Accounts,
    Settings,
}

#[derive(Clone, Copy)]
enum LoginMethod {
    /// System browser + loopback redirect — just sign in, no code to type.
    Browser,
    /// Device-code fallback (open a page, type a code).
    Device,
    /// Silent login with the stored refresh token (legacy single-account).
    Refresh,
}

pub struct DolphinApp {
    tx: Sender<Event>,
    rx: Receiver<Event>,

    /// Last resolved session (for the status line only; play resolves fresh).
    session: Option<Session>,
    settings: Settings,
    accounts: AccountStore,

    status: String,
    progress: f32,
    busy: bool,
    device: Option<(String, String)>, // (url, code)
    auth_url: Option<String>,         // browser-login URL (for re-open)
    log: Vec<String>,
    show_log: bool,
    import_note: Option<String>,

    tab: Tab,
    update_note: Arc<Mutex<Option<String>>>,
}

impl DolphinApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_theme(&cc.egui_ctx);

        let (tx, rx) = channel();
        let settings = Settings::load();
        let accounts = AccountStore::load();
        let status = match accounts.active_account() {
            Some(a) => format!("Angemeldet als {}", a.username),
            None => "Kein Konto — melde dich an oder importiere eines".to_string(),
        };

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
            accounts,
            status,
            progress: 0.0,
            busy: false,
            device: None,
            auth_url: None,
            log: Vec::new(),
            show_log: false,
            import_note: None,
            tab: Tab::Home,
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
                Event::Device {
                    complete,
                    code,
                    message,
                } => {
                    self.device = Some((complete, code));
                    self.status = message;
                }
                Event::BrowserOpen { url } => {
                    self.auth_url = Some(url);
                }
                Event::LoggedIn(session) => {
                    self.status = format!("Angemeldet als {}", session.username);
                    // A Microsoft OAuth login always yields a renewable refresh
                    // token (stored per-account by auth::minecraft_session).
                    self.accounts.upsert(Account {
                        uuid: session.uuid.clone(),
                        username: session.username.clone(),
                        source: "Microsoft".to_string(),
                        has_refresh: true,
                    });
                    self.accounts.set_active(&session.uuid);
                    self.session = Some(session);
                    self.device = None;
                    self.auth_url = None;
                }
                Event::Launched => self.status = "Minecraft läuft — viel Spaß! 🐬".to_string(),
                Event::Error(e) => {
                    self.status = format!("Fehler: {}", e);
                    self.device = None;
                    self.auth_url = None;
                    self.show_log = true; // reveal the details log on failure
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

    /// Kick off a Microsoft login (browser when we have an Azure app, else the
    /// device-code flow that needs no custom app).
    fn add_microsoft(&mut self, ctx: &egui::Context) {
        let method = if crate::auth::is_azure() {
            LoginMethod::Browser
        } else {
            LoginMethod::Device
        };
        self.start_login(ctx, method);
    }

    /// Import signed-in accounts from other launchers on this device (synchronous
    /// — just local file reads + credential-store writes).
    fn import_accounts(&mut self) {
        let found = crate::accounts::discover();
        let mut imported = 0;
        for imp in found {
            crate::tokens::save_access_for(&imp.uuid, &imp.access_token);
            self.accounts.upsert(Account {
                uuid: imp.uuid.clone(),
                username: imp.username,
                source: format!("Import · {}", imp.source),
                has_refresh: false,
            });
            imported += 1;
        }
        let msg = if imported == 0 {
            "Keine importierbaren Konten gefunden (nur unverschlüsselte Launcher wie Vanilla/Lunar).".to_string()
        } else {
            format!("{imported} Konto(en) importiert.")
        };
        self.import_note = Some(msg.clone());
        self.status = msg;
    }

    fn start_launch(&mut self, ctx: &egui::Context) {
        let Some(account) = self.accounts.active_account().cloned() else {
            self.status = "Kein aktives Konto — bitte hinzufügen.".to_string();
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.progress = 0.0;
        self.status = "Spielstart wird vorbereitet …".to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let server = self.settings.server.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                // Resolve a live session for the active account.
                let session = if account.has_refresh {
                    crate::auth::login_with_refresh_for(&account.uuid, &tx)?
                } else {
                    let access = crate::tokens::load_access_for(&account.uuid).ok_or_else(|| {
                        anyhow::anyhow!("Kein gültiges Token — bitte Konto neu anmelden.")
                    })?;
                    Session {
                        uuid: account.uuid.clone(),
                        username: account.username.clone(),
                        access_token: access,
                    }
                };
                crate::client::launch(&session, &server, &tx)
            })();
            if let Err(e) = result {
                let _ = tx.send(Event::Error(e.to_string()));
            }
            let _ = tx.send(Event::Done);
            ctx.request_repaint();
        });
    }

    /// Remove the active account (and its stored secrets).
    fn remove_active(&mut self) {
        if let Some(a) = self.accounts.active_account().cloned() {
            self.accounts.remove(&a.uuid);
        }
        self.session = None;
        self.device = None;
        self.auth_url = None;
        self.status = "Konto entfernt.".to_string();
    }
}

impl eframe::App for DolphinApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        if self.busy {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        top_bar(self, ctx);

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Home => home_view(self, ui, ctx),
            Tab::Accounts => accounts_view(self, ui, ctx),
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

fn card() -> egui::Frame {
    egui::Frame::none()
        .fill(CARD_FILL)
        .stroke(egui::Stroke::new(1.0, CARD_STROKE))
        .rounding(16.0)
        .inner_margin(egui::Margin::same(18.0))
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
                    let acc_count = app.accounts.accounts.len();
                    if ui
                        .selectable_label(app.tab == Tab::Accounts, format!("  👤 Konten ({acc_count})  "))
                        .clicked()
                    {
                        app.tab = Tab::Accounts;
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
            egui::RichText::new("Ein Klick — Login, Dependencies und 26.1-Start.")
                .size(15.0)
                .color(MUTED),
        );
    });
    ui.add_space(22.0);

    let max_w = 560.0_f32.min(ui.available_width() - 24.0);
    ui.vertical_centered(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(max_w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                card().show(ui, |ui| {
                    ui.set_width(max_w - 44.0);

                    let active = app.accounts.active_account().cloned();
                    if let Some(a) = &active {
                        ui.horizontal(|ui| {
                            avatar(ui, &a.username);
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(&a.username).size(18.0).strong());
                                ui.label(
                                    egui::RichText::new(format!("● {}", a.source))
                                        .color(MINT)
                                        .size(12.0),
                                );
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if app.accounts.accounts.len() > 1
                                        && ui.small_button("Konto wechseln").clicked()
                                    {
                                        app.tab = Tab::Accounts;
                                    }
                                },
                            );
                        });
                        ui.add_space(14.0);
                    }

                    device_and_browser_prompts(app, ui);

                    let accent = egui::Color32::from_rgb(50, 210, 200);
                    ui.add_enabled_ui(!app.busy, |ui| {
                        if active.is_some() {
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
                                app.add_microsoft(ctx);
                            }
                            ui.add_space(6.0);
                            if ui
                                .button("⬇  Konten aus anderen Launchern importieren")
                                .clicked()
                            {
                                app.import_accounts();
                            }
                            // Migrate a pre-multi-account saved login, if any.
                            if crate::tokens::has_token()
                                && ui.button("↻  Vorheriges Konto wiederherstellen").clicked()
                            {
                                app.start_login(ctx, LoginMethod::Refresh);
                            }
                        }
                    });

                    ui.add_space(12.0);
                    progress_and_status(app, ui);
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut app.show_log, "Details anzeigen");
                });
                if app.show_log {
                    log_box(app, ui);
                }
            },
        );
    });

    ui.add_space(18.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(
                "Nativ in Rust · Original-Dateien von Mojang · Multi-Account · Auto-Update",
            )
            .color(MUTED)
            .size(12.0),
        );
    });
}

/// Device-code + browser sign-in prompts (shared by Home and Accounts).
fn device_and_browser_prompts(app: &mut DolphinApp, ui: &mut egui::Ui) {
    if let Some((link, code)) = app.device.clone() {
        ui.label(
            egui::RichText::new("Ein Browser-Fenster wurde geöffnet — melde dich dort an.")
                .color(MUTED),
        );
        ui.add_space(6.0);
        ui.hyperlink_to(
            egui::RichText::new("👉  Jetzt bei Microsoft anmelden")
                .size(16.0)
                .strong()
                .color(CYAN),
            link.clone(),
        );
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Code (im Link bereits enthalten):")
                .color(MUTED)
                .size(12.0),
        );
        ui.label(
            egui::RichText::new(&code)
                .size(30.0)
                .strong()
                .color(CYAN)
                .monospace(),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("🌐  Link erneut öffnen").clicked() {
                let _ = open::that(&link);
            }
            if ui.button("📋  Code kopieren").clicked() {
                ui.output_mut(|o| o.copied_text = code.clone());
            }
        });
        ui.add_space(12.0);
    }

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
}

fn progress_and_status(app: &DolphinApp, ui: &mut egui::Ui) {
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
}

fn log_box(app: &DolphinApp, ui: &mut egui::Ui) {
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
                        ui.label(egui::RichText::new("— noch keine Ausgaben —").color(MUTED));
                    }
                    for line in &app.log {
                        ui.label(egui::RichText::new(line).monospace().size(12.0));
                    }
                });
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
/*  Accounts view                                                   */
/* ---------------------------------------------------------------- */

fn accounts_view(app: &mut DolphinApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.add_space(16.0);
    ui.label(egui::RichText::new("Konten").size(24.0).strong());
    ui.label(
        egui::RichText::new(
            "Mehrere Microsoft-Konten verwalten oder aus anderen Launchern importieren.",
        )
        .color(MUTED)
        .size(13.0),
    );
    ui.add_space(12.0);

    let max_w = 620.0_f32.min(ui.available_width() - 8.0);
    ui.allocate_ui_with_layout(
        egui::vec2(max_w, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            // Action row.
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!app.busy, |ui| {
                    if ui.button("➕  Microsoft-Konto hinzufügen").clicked() {
                        app.add_microsoft(ctx);
                    }
                });
                if ui.button("⬇  Aus Launchern importieren").clicked() {
                    app.import_accounts();
                }
            });
            ui.label(
                egui::RichText::new(
                    "Import unterstützt: Vanilla-Launcher & Lunar Client (unverschlüsselte Token-Ablage). \
                     Badlion/Feather verschlüsseln ihre Token und lassen sich nicht importieren.",
                )
                .color(MUTED)
                .size(11.0),
            );
            if let Some(note) = &app.import_note {
                ui.label(egui::RichText::new(note).color(MINT).size(12.0));
            }
            ui.add_space(8.0);

            device_and_browser_prompts(app, ui);

            // Account list.
            let accounts = app.accounts.accounts.clone();
            if accounts.is_empty() {
                card().show(ui, |ui| {
                    ui.set_width(max_w - 40.0);
                    ui.label(
                        egui::RichText::new("Noch keine Konten. Füge eines hinzu oder importiere.")
                            .color(MUTED),
                    );
                });
            }
            let mut switch_to: Option<String> = None;
            let mut remove: Option<String> = None;
            for a in &accounts {
                let is_active = app.accounts.is_active(&a.uuid);
                let stroke = if is_active {
                    egui::Stroke::new(1.5, CYAN)
                } else {
                    egui::Stroke::new(1.0, CARD_STROKE)
                };
                egui::Frame::none()
                    .fill(CARD_FILL)
                    .stroke(stroke)
                    .rounding(12.0)
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.set_width(max_w - 40.0);
                        ui.horizontal(|ui| {
                            avatar(ui, &a.username);
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(&a.username).size(16.0).strong());
                                let mut meta = a.source.clone();
                                if !a.has_refresh {
                                    meta.push_str("  ·  Token temporär (evtl. neu anmelden)");
                                }
                                ui.label(egui::RichText::new(meta).color(MUTED).size(11.0));
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Entfernen").clicked() {
                                        remove = Some(a.uuid.clone());
                                    }
                                    if is_active {
                                        ui.label(egui::RichText::new("● Aktiv").color(MINT).size(12.0));
                                    } else if ui.button("Auswählen").clicked() {
                                        switch_to = Some(a.uuid.clone());
                                    }
                                },
                            );
                        });
                    });
                ui.add_space(6.0);
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

            ui.add_space(10.0);
            progress_and_status(app, ui);
        },
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
        .fill(CARD_FILL)
        .stroke(egui::Stroke::new(1.0, CARD_STROKE))
        .rounding(14.0)
        .inner_margin(egui::Margin::same(18.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Standard-Server").strong());
            ui.label(
                egui::RichText::new(
                    "Server, dem der Client beim Start beitritt (host oder host:port). \
                     Leer = Verbindungsbildschirm im Client.",
                )
                .color(MUTED)
                .size(12.0),
            );
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut app.settings.server)
                        .hint_text("z. B. play.example.net")
                        .desired_width(ui.available_width()),
                )
                .changed();

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);

            ui.label(egui::RichText::new("Arbeitsspeicher").strong());
            ui.label(
                egui::RichText::new(
                    "Dem Spiel zugewiesener Heap (-Xmx). Nur relevant, falls der \
                     klassische Java-Start genutzt wird.",
                )
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
                .checkbox(&mut app.settings.auto_update, "Beim Start auf Updates prüfen")
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
        if app.accounts.active_account().is_some() && ui.button("Aktives Konto abmelden").clicked() {
            app.remove_active();
        }
    });
    ui.label(
        egui::RichText::new(format!("Spielordner: {}", config::minecraft_dir().display()))
            .color(MUTED)
            .size(12.0),
    );

    if changed {
        app.settings.save();
    }
}
