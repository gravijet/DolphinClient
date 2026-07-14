//! egui application: multi-account login, play, live status and auto-saving
//! settings — a serious native launcher (Home / Konten / Einstellungen) in the
//! spirit of Lunar Client / NoRisk Client. Accounts can be added via Microsoft
//! or imported from other launchers on this device.

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
    Game,
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
    /// Set to an account's name when its session could not be resolved at launch
    /// (even after re-import) — the UI shows a "sign in again" prompt for it.
    pub(crate) relogin_for: Option<String>,
    pub(crate) device: Option<(String, String)>, // (url, code)
    pub(crate) auth_url: Option<String>,          // browser-login URL (for re-open)
    pub(crate) log: Vec<String>,
    pub(crate) show_log: bool,
    pub(crate) import_note: Option<String>,

    /// Draft fields for the "add saved server" form on the Spiel tab.
    pub(crate) new_server_name: String,
    pub(crate) new_server_addr: String,

    pub(crate) tab: Tab,
    pub(crate) update_note: Arc<Mutex<Option<crate::updater::UpdateInfo>>>,
    /// Guards against kicking off the same automatic update install twice.
    auto_update_started: bool,
    /// Logo texture (loaded from the embedded brand PNG).
    pub(crate) logo: egui::TextureHandle,
    /// Client versions offered by the download archive (newest first).
    pub(crate) versions: Arc<Mutex<Vec<String>>>,

    pub(crate) stats: Arc<Mutex<Stats>>,
    /// True while the game process is running.
    pub(crate) running: Arc<AtomicBool>,

    /// Player-head avatar for the active account (fetched in the background).
    pub(crate) avatar: Arc<Mutex<Option<egui::TextureHandle>>>,
    /// Full-body skin render for the active account (fetched in the background).
    pub(crate) body: Arc<Mutex<Option<egui::TextureHandle>>>,
    avatar_for: Option<String>,

    /// Cached view over the client's `options.json` (game quick-settings).
    pub(crate) gameopts: crate::gameopts::GameOpts,
    /// Track game-running edge + tab changes so we can reload `gameopts` from
    /// disk after the client (which owns `options.json`) may have rewritten it.
    was_running: bool,
    prev_tab: Tab,

    /// Discord Rich Presence worker for the launcher (`None` when Discord isn't
    /// running or no Application id is configured). Broadcasts an "in the
    /// launcher" presence while open, and clears it while the game is running so
    /// the in-game client's own presence takes over.
    discord: Option<crate::discord::Discord>,
    /// Last activity pushed to Discord — diffed so we only re-send on change.
    discord_state: Option<crate::discord::Activity>,
    /// Unix seconds the launcher opened (Discord's "elapsed" timer).
    launcher_start_unix: u64,
}

/// Download an image in the background and store it as an egui texture in `slot`.
fn fetch_texture(
    ctx: &egui::Context,
    slot: Arc<Mutex<Option<egui::TextureHandle>>>,
    name: &'static str,
    url: String,
) {
    let ctx = ctx.clone();
    std::thread::spawn(move || {
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
                let tex = ctx.load_texture(name, color, egui::TextureOptions::NEAREST);
                if let Ok(mut s) = slot.lock() {
                    *s = Some(tex);
                }
                ctx.request_repaint();
            }
        }
    });
}

impl DolphinApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();
        crate::fonts::install(&cc.egui_ctx);
        crate::ui::install_theme(&cc.egui_ctx, &settings.accent);
        // Keep the OS autostart entry consistent with the saved preference — the
        // launcher path can change after an update, so re-apply it on every start.
        let _ = crate::autostart::set(settings.autostart);

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
            relogin_for: None,
            device: None,
            auth_url: None,
            log: Vec::new(),
            show_log: false,
            import_note: None,
            new_server_name: String::new(),
            new_server_addr: String::new(),
            tab: Tab::Home,
            update_note,
            auto_update_started: false,
            logo,
            versions,
            stats: Arc::new(Mutex::new(Stats::load())),
            running: Arc::new(AtomicBool::new(false)),
            avatar: Arc::new(Mutex::new(None)),
            body: Arc::new(Mutex::new(None)),
            avatar_for: None,
            gameopts: crate::gameopts::GameOpts::load(),
            was_running: false,
            prev_tab: Tab::Home,
            discord: crate::discord::Discord::spawn(),
            discord_state: None,
            launcher_start_unix: config::now_unix(),
        };
        app
    }

    /// Push the launcher's Rich Presence to Discord, re-sending only when it
    /// changed. While the game is running we clear our presence so the in-game
    /// client's richer "playing on <server>" presence shows instead — the two
    /// launcher/client processes never fight over the same Discord app.
    fn update_discord(&mut self) {
        let Some(discord) = &self.discord else { return };
        let running = self.running.load(Ordering::Relaxed);
        let want: Option<crate::discord::Activity> = if !self.settings.discord_rpc || running {
            None
        } else {
            let state = match self.accounts.active_account() {
                Some(a) => format!("als {}", a.username),
                None => "Noch nicht angemeldet".to_string(),
            };
            Some(crate::discord::Activity {
                details: Some("Im Launcher · bereit zum Spielen".to_string()),
                state: Some(state),
                large_image: Some(crate::discord::large_image()),
                large_text: Some(format!("DolphinClient · Minecraft {}", config::TARGET_VERSION)),
                start_unix: Some(self.launcher_start_unix),
            })
        };
        if want != self.discord_state {
            discord.set(want.clone());
            self.discord_state = want;
        }
    }

    /// Truly automatic updates: as soon as the background check reports a newer
    /// launcher version and the user hasn't opted out, download and install it
    /// without waiting for a click (the process restarts itself). Runs at most
    /// once, and never while a login/launch is in flight.
    fn maybe_auto_update(&mut self, ctx: &egui::Context) {
        if self.auto_update_started
            || self.busy
            || self.running.load(Ordering::Relaxed)
            || !self.settings.auto_update
            || !self.settings.auto_update_apply
        {
            return;
        }
        if let Some(info) = self.update_note.lock().ok().and_then(|n| n.clone()) {
            self.auto_update_started = true;
            self.start_self_update(ctx, info);
        }
    }

    /// Reload the client's `options.json` into our cached view after the game
    /// exits (it rewrites the file on close) or when the user opens Settings —
    /// so a launcher edit never clobbers the client's newer values.
    fn refresh_gameopts_if_needed(&mut self) {
        let running_now = self.running.load(Ordering::Relaxed);
        let opens_game_tab =
            matches!(self.tab, Tab::Game | Tab::Settings) && self.tab != self.prev_tab;
        if (self.was_running && !running_now) || opens_game_tab {
            self.gameopts = crate::gameopts::GameOpts::load();
        }
        self.was_running = running_now;
        self.prev_tab = self.tab;
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
                    self.relogin_for = None;
                }
                Event::AuthFailed { username } => {
                    self.status = format!("Anmeldung für {username} nötig");
                    self.relogin_for = Some(username);
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

    /// Kick off fetching the active account's player-head avatar and full-body
    /// skin render when the active account changes.
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
            if let Ok(mut b) = self.body.lock() {
                *b = None;
            }
            return;
        };
        // Player head (bottom play bar + profile).
        fetch_texture(
            ctx,
            self.avatar.clone(),
            "avatar",
            format!("https://minotar.net/helm/{name}/64.png"),
        );
        // Full-body skin render (Home hero preview).
        fetch_texture(
            ctx,
            self.body.clone(),
            "body",
            format!("https://minotar.net/armor/body/{name}/128.png"),
        );
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
            "Keine übernehmbaren Konten auf diesem PC gefunden.".to_string()
        } else if imported == 1 {
            "1 Konto übernommen.".to_string()
        } else {
            format!("{imported} Konten übernommen.")
        };
        self.import_note = Some(msg.clone());
        self.status = msg;
    }

    pub(crate) fn start_launch(&mut self, ctx: &egui::Context) {
        self.start_launch_with(ctx, None);
    }

    /// Launch straight into a specific server address (Home quick-join chips).
    pub(crate) fn launch_server(&mut self, ctx: &egui::Context, address: String) {
        self.start_launch_with(ctx, Some(address));
    }

    /// Add the draft server (from the Spiel tab form) to the saved list.
    pub(crate) fn add_server(&mut self) {
        let addr = self.new_server_addr.trim().to_string();
        if addr.is_empty() {
            return;
        }
        let name = {
            let n = self.new_server_name.trim();
            if n.is_empty() { addr.clone() } else { n.to_string() }
        };
        self.settings.servers.push(config::ServerEntry { name, address: addr });
        self.settings.save();
        self.new_server_name.clear();
        self.new_server_addr.clear();
    }

    pub(crate) fn remove_server(&mut self, idx: usize) {
        if idx < self.settings.servers.len() {
            self.settings.servers.remove(idx);
            self.settings.save();
        }
    }

    /// Pin a saved server as the default the client auto-joins on launch.
    pub(crate) fn set_default_server(&mut self, address: &str) {
        self.settings.server = address.to_string();
        self.settings.save();
    }

    fn start_launch_with(&mut self, ctx: &egui::Context, server_override: Option<String>) {
        let Some(account) = self.accounts.active_account().cloned() else {
            self.status = "Kein aktives Konto — bitte hinzufügen.".to_string();
            return;
        };
        if self.busy || self.running.load(Ordering::Relaxed) {
            return;
        }
        self.busy = true;
        self.progress = 0.0;
        self.relogin_for = None;
        self.status = "Spielstart wird vorbereitet …".to_string();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let server = server_override.unwrap_or_else(|| self.settings.server.clone());
        let client_version = self.settings.client_version.clone();
        let running = self.running.clone();
        let stats = self.stats.clone();
        let close_on_launch = self.settings.close_on_launch;
        std::thread::spawn(move || {
            // Resolving the session tries our refresh token, the cached token,
            // then re-importing from other launchers. If it still fails the
            // account genuinely needs a fresh sign-in — surface that distinctly.
            let session = match crate::auth::resolve_session(
                &account.uuid,
                &account.username,
                account.has_refresh,
                &tx,
            ) {
                Ok(s) => s,
                Err(_) => {
                    let _ = tx.send(Event::AuthFailed {
                        username: account.username.clone(),
                    });
                    let _ = tx.send(Event::Done);
                    ctx.request_repaint();
                    return;
                }
            };
            let result = crate::client::launch(&session, &server, &client_version, &tx);
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
        self.refresh_gameopts_if_needed();
        self.maybe_auto_update(ctx);
        self.update_discord();
        // The UI is static — no ambient animation to pump. Only keep a light
        // repaint going while a task is in flight so the progress bar advances
        // smoothly (hover/toggle states schedule their own repaints).
        if self.busy {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        crate::ui::draw(self, ctx);
    }

    /// Frameless window: a dark clear colour avoids white flashes on resize.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let [r, g, b] = crate::ui::BG_0;
        [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
    }
}
