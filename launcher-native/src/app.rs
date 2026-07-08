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
use crate::config::Settings;
use crate::events::Event;


#[derive(PartialEq, Eq)]
pub(crate) enum Tab {
    Home,
    Accounts,
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
    pub(crate) auth_url: Option<String>,         // browser-login URL (for re-open)
    pub(crate) log: Vec<String>,
    pub(crate) show_log: bool,
    pub(crate) import_note: Option<String>,

    pub(crate) tab: Tab,
    pub(crate) update_note: Arc<Mutex<Option<crate::updater::UpdateInfo>>>,
    /// Minecraft-look UI assets (real game font/textures once the jar is cached).
    pub(crate) mc: crate::mcui::McUi,
    /// Set by the background jar download so `update` reloads the real
    /// Minecraft UI assets (first start only).
    jar_ready: Arc<std::sync::atomic::AtomicBool>,
    /// Client versions offered by the download archive (newest first),
    /// fetched in the background at startup.
    pub(crate) versions: Arc<Mutex<Vec<String>>>,
}

impl DolphinApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::ui::install_theme(&cc.egui_ctx);

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

        // First start: fetch the vanilla jar in the background so the UI can
        // switch to the real Minecraft font/textures (it is needed to play
        // anyway). Progress lands in the normal status/log events.
        let jar_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let jar_path = crate::config::minecraft_dir()
            .join("versions")
            .join(crate::config::TARGET_VERSION)
            .join(format!("{}.jar", crate::config::TARGET_VERSION));
        if !jar_path.exists() {
            let ready = jar_ready.clone();
            let tx2 = tx.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let client = crate::game::http();
                if crate::game::ensure_client_jar(&client, &tx2).is_ok() {
                    ready.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                ctx.request_repaint();
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
            mc: crate::mcui::McUi::load(&cc.egui_ctx),
            jar_ready,
            versions,
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

    /// Import signed-in accounts from other launchers on this device (synchronous
    /// — just local file reads + credential-store writes).
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
            "Keine importierbaren Konten gefunden (nur unverschlüsselte Launcher wie Vanilla/Lunar).".to_string()
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
        if self.busy {
            return;
        }
        self.busy = true;
        self.progress = 0.0;
        self.status = "Spielstart wird vorbereitet …".to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let server = self.settings.server.clone();
        let client_version = self.settings.client_version.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                // Resolve a live session for the active account, renewing
                // automatically (refresh token → cached token → imported from
                // another launcher) and only demanding re-login as a last resort.
                let session = crate::auth::resolve_session(
                    &account.uuid,
                    &account.username,
                    account.has_refresh,
                    &tx,
                )?;
                crate::client::launch(&session, &server, &client_version, &tx)
            })();
            if let Err(e) = result {
                let _ = tx.send(Event::Error(e.to_string()));
            }
            let _ = tx.send(Event::Done);
            ctx.request_repaint();
        });
    }

    /// Remove the active account (and its stored secrets).
    /// Download and install the launcher update in the background. On success
    /// the process restarts itself (the thread never reports back).
    pub(crate) fn start_self_update(&mut self, ctx: &egui::Context, info: crate::updater::UpdateInfo) {
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
        // The background jar download finished → swap in the real game assets.
        if self.jar_ready.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.mc = crate::mcui::McUi::load(ctx);
        }
        if self.busy {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
        crate::ui::draw(self, ctx);
    }

    /// Frameless window: nothing outside our own painting, so a plain black
    /// clear color avoids white flashes on resize.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }
}

