//! egui application: multi-account login, play, live status and auto-saving
//! settings — a modern native launcher (Home / Konten / Cosmetics /
//! Einstellungen) in the spirit of Lunar Client / NoRisk Client. Accounts can
//! be added via Microsoft or imported from other launchers on this device.
//!
//! The launcher also runs a local [`crate::bridge`] server so the website
//! dashboard can read its live state (active account, version, playtime).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, ViewportCommand};

use crate::accounts::{Account, AccountStore};
use crate::auth::Session;
use crate::config::{self, Settings, Stats};
use crate::events::Event;

#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum Tab {
    Home,
    Accounts,
    Cosmetics,
    Settings,
}

#[derive(Clone, Copy)]
pub(crate) enum LoginMethod {
    /// System browser + loopback redirect — just sign in, no code to type.
    Browser,
    /// Device-code fallback (open a page, type a code).
    Device,
    /// Silent login with the stored refresh token (legacy single-account).
    Refresh,
}

pub struct DolphinApp {
    pub(crate) tx: Sender<Event>,
    pub(crate) rx: Receiver<Event>,

    /// Last resolved session (for the status line only; play resolves fresh).
    pub(crate) session: Option<Session>,
    pub(crate) settings: Settings,
    pub(crate) accounts: AccountStore,

    pub(crate) status: String,
    pub(crate) progress: f32,
    pub(crate) busy: bool,
    pub(crate) device: Option<(String, String)>, // (url, code)
    pub(crate) auth_url: Option<String>,          // browser-login URL (for re-open)
    pub(crate) log: Vec<String>,
    pub(crate) show_log: bool,
    pub(crate) import_note: Option<String>,

    pub(crate) tab: Tab,
    pub(crate) update_note: Arc<Mutex<Option<crate::updater::UpdateInfo>>>,
    /// Logo texture (loaded from the embedded brand PNG).
    pub(crate) logo: egui::TextureHandle,
    /// Client versions offered by the download archive (newest first).
    pub(crate) versions: Arc<Mutex<Vec<String>>>,

    /// Live state shared with the web dashboard.
    pub(crate) bridge: Arc<crate::bridge::Bridge>,
    pub(crate) stats: Arc<Mutex<Stats>>,
    /// True while the game process is running.
    pub(crate) running: Arc<AtomicBool>,

    /// Player-head avatar for the active account (fetched in the background).
    pub(crate) avatar: Arc<Mutex<Option<egui::TextureHandle>>>,
    avatar_for: Option<String>,
}

impl DolphinApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();
        crate::ui::install_theme(&cc.egui_ctx, &settings.accent);

        let (tx, rx) = channel();
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

        // Fetch the client-version archive for the version picker.
        let versions = Arc::new(Mutex::new(Vec::new()));
        {
            let versions = versions.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let list = crate::client::available_versions(&crate::game::http());
                if let Ok(mut v) = versions.lock() {
                    *v = list;
                }
                ctx.request_repaint();
            });
        }

        // First start: warm the vanilla jar in the background (needed to play).
        let jar_path = config::minecraft_dir()
            .join("versions")
            .join(config::TARGET_VERSION)
            .join(format!("{}.jar", config::TARGET_VERSION));
        if !jar_path.exists() {
            let tx2 = tx.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let client = crate::game::http();
                let _ = crate::game::ensure_client_jar(&client, &tx2);
                ctx.request_repaint();
            });
        }

        // Start the local dashboard bridge.
        let bridge = crate::bridge::Bridge::new();
        crate::bridge::start(bridge.clone());

        let logo = crate::mcui::load_logo(&cc.egui_ctx);

        let app = Self {
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
            logo,
            versions,
            bridge,
            stats: Arc::new(Mutex::new(Stats::load())),
            running: Arc::new(AtomicBool::new(false)),
            avatar: Arc::new(Mutex::new(None)),
            avatar_for: None,
        };
        app
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

    /// Kick off fetching the active account's player-head avatar when it changes.
    fn refresh_avatar(&mut self, ctx: &egui::Context) {
        let want = self.accounts.active_account().map(|a| a.username.clone());
        if want == self.avatar_for {
            return;
        }
        self.avatar_for = want.clone();
        let Some(name) = want else {
            if let Ok(mut a) = self.avatar.lock() {
                *a = None;
            }
            return;
        };
        let slot = self.avatar.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let url = format!("https://minotar.net/helm/{}/64.png", name);
            let bytes = crate::game::http()
                .get(&url)
                .send()
                .ok()
                .and_then(|r| r.bytes().ok());
            if let Some(bytes) = bytes {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let img = img.to_rgba8();
                    let size = [img.width() as usize, img.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    let tex =
                        ctx.load_texture("avatar", color, egui::TextureOptions::NEAREST);
                    if let Ok(mut a) = slot.lock() {
                        *a = Some(tex);
                    }
                    ctx.request_repaint();
                }
            }
        });
    }

    /// The live status the web dashboard reads over the bridge.
    fn status_json(&self) -> String {
        let account = self.accounts.active_account().map(|a| {
            serde_json::json!({
                "name": a.username,
                "uuid": a.uuid,
                "source": a.source,
            })
        });
        let stats = self.stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
        serde_json::json!({
            "connected": true,
            "product": "DolphinClient",
            "launcherVersion": env!("CARGO_PKG_VERSION"),
            "minecraft": config::TARGET_VERSION,
            "clientVersion": if self.settings.client_version.is_empty() {
                "neueste".to_string()
            } else {
                self.settings.client_version.clone()
            },
            "accent": self.settings.accent,
            "running": self.running.load(Ordering::Relaxed),
            "busy": self.busy,
            "status": self.status,
            "account": account,
            "accounts": self.accounts.accounts.len(),
            "settings": {
                "server": self.settings.server,
                "ramGb": self.settings.ram_gb,
                "fullscreen": self.settings.fullscreen,
                "autoUpdate": self.settings.auto_update,
                "closeOnLaunch": self.settings.close_on_launch,
                "cape": self.settings.cape,
            },
            "stats": {
                "playtimeSecs": stats.playtime_secs,
                "launches": stats.launches,
                "lastPlayed": stats.last_played,
            },
        })
        .to_string()
    }

    pub(crate) fn start_login(&mut self, ctx: &egui::Context, method: LoginMethod) {
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
    pub(crate) fn add_microsoft(&mut self, ctx: &egui::Context) {
        let method = if crate::auth::is_azure() {
            LoginMethod::Browser
        } else {
            LoginMethod::Device
        };
        self.start_login(ctx, method);
    }

    /// Import signed-in accounts from other launchers on this device.
    pub(crate) fn import_accounts(&mut self) {
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
            "Keine importierbaren Konten gefunden (nur unverschlüsselte Launcher wie Vanilla/Lunar)."
                .to_string()
        } else {
            format!("{imported} Konto(en) importiert.")
        };
        self.import_note = Some(msg.clone());
        self.status = msg;
    }

    pub(crate) fn start_launch(&mut self, ctx: &egui::Context) {
        let Some(account) = self.accounts.active_account().cloned() else {
            self.status = "Kein aktives Konto — bitte hinzufügen.".to_string();
            return;
        };
        if self.busy || self.running.load(Ordering::Relaxed) {
            return;
        }
        self.busy = true;
        self.progress = 0.0;
        self.status = "Spielstart wird vorbereitet …".to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let server = self.settings.server.clone();
        let client_version = self.settings.client_version.clone();
        let running = self.running.clone();
        let stats = self.stats.clone();
        let close_on_launch = self.settings.close_on_launch;
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<std::process::Child> {
                let session = crate::auth::resolve_session(
                    &account.uuid,
                    &account.username,
                    account.has_refresh,
                    &tx,
                )?;
                crate::client::launch(&session, &server, &client_version, &tx)
            })();
            match result {
                Ok(child) => {
                    if let Ok(mut s) = stats.lock() {
                        s.record_launch();
                    }
                    running.store(true, Ordering::Relaxed);
                    let _ = tx.send(Event::Launched);
                    // Track playtime + clear the running flag when the game exits.
                    let stats2 = stats.clone();
                    let running2 = running.clone();
                    let ctx2 = ctx.clone();
                    let start = Instant::now();
                    std::thread::spawn(move || {
                        let mut child = child;
                        let _ = child.wait();
                        let secs = start.elapsed().as_secs();
                        if let Ok(mut s) = stats2.lock() {
                            s.record_session(secs);
                        }
                        running2.store(false, Ordering::Relaxed);
                        ctx2.request_repaint();
                    });
                    if close_on_launch {
                        std::thread::sleep(Duration::from_millis(600));
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                }
                Err(e) => {
                    let _ = tx.send(Event::Error(e.to_string()));
                }
            }
            let _ = tx.send(Event::Done);
            ctx.request_repaint();
        });
    }

    /// Download and install the launcher update in the background. On success
    /// the process restarts itself (the thread never reports back).
    pub(crate) fn start_self_update(
        &mut self,
        ctx: &egui::Context,
        info: crate::updater::UpdateInfo,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.progress = 0.0;
        self.status = format!("Update {} wird geladen …", info.version);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Err(e) = crate::updater::apply(&info, &tx) {
                let _ = tx.send(Event::Error(format!("Update fehlgeschlagen: {e:#}")));
                let _ = tx.send(Event::Done);
            }
            ctx.request_repaint();
        });
    }

    pub(crate) fn remove_active(&mut self) {
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
        self.refresh_avatar(ctx);
        // Keep the web dashboard's view of the launcher fresh.
        self.bridge.set(self.status_json());
        if self.busy || self.running.load(Ordering::Relaxed) {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        crate::ui::draw(self, ctx);
    }

    /// Frameless window: a dark clear colour avoids white flashes on resize.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let [r, g, b] = crate::ui::BG_0;
        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
    }
}
