//! egui HUD + menus, drawn with the real Minecraft assets (see `mcui`).
//!
//! In game: sprite crosshair, the vanilla hotbar with item icons, hearts /
//! food / XP bar, colored/clickable chat (see `chat`), tab list overlay,
//! container screens (see `container`), subtitles, F3 debug, and an Esc pause
//! menu over the translucent in-world tile.
//!
//! Out of game: a Minecraft-style title screen over the rotating panorama, a
//! vanilla server list with live pings, Direct Connect / Add Server screens
//! and an Options screen with rebindable controls.

use crate::app::chat::ChatState;
use crate::app::container::{self, ContainerView};
use crate::app::mcui::{self, BTN_GAP, BTN_W, COL_W, LINE_H, McUi, ROW_W};
use crate::app::serverlist::{PingState, Pinger, SavedServer, ServerListStore};
use crate::app::skins::SkinManager;
use crate::app::tablist::{self, TabListState};
use crate::assets::Lang;
use crate::assets::items::ItemIcons;
use crate::bridge::events::{ChatSpan, ItemSnapshot, ScoreLine, SlotClickKind, TradeOffer};
use crate::settings::{GameSettings, KeyBinds, key_label};
use egui::{
    Align2, Area, Color32, Id, Key, LayerId, Order, Rect, ScrollArea, Sense, TextureHandle,
    TextureId, pos2, vec2,
};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

#[derive(Default)]
pub struct HudState {
    pub fps: f32,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub food: u32,
    pub xp_level: u32,
    /// 0.0..1.0 — the XP bar fill.
    pub xp_progress: f32,
    /// 0.0..1.0 — attack cooldown recharge (drives the crosshair indicator).
    pub attack_strength: f32,
    /// An attackable entity is under the crosshair within melee reach — shows
    /// the full-charge indicator like vanilla.
    pub target_in_reach: bool,
    pub hotbar: Vec<Option<ItemSnapshot>>,
    /// Off-hand item (drawn in its own box beside the hotbar), if any.
    pub offhand: Option<ItemSnapshot>,
    pub selected_slot: u8,
    /// Display name of the just-selected item, shown above the hotbar and fading
    /// out (vanilla). Empty = nothing to show.
    pub item_name: Vec<ChatSpan>,
    /// Fade alpha 0..1 for `item_name` (0 = fully faded / hidden).
    pub item_name_alpha: f32,
    /// Item-icon atlas (egui texture id + lookup); None until it loads.
    pub icons: Option<(TextureId, Arc<ItemIcons>)>,
    pub sections_drawn: usize,
    pub sections_total: usize,
    pub mesh_queue: usize,
    pub connected: bool,
    /// A connect attempt is in flight (bridge spawned, not yet Connected).
    pub connecting: bool,
    /// 1-based attempt number; > 1 while auto-retrying a transient failure.
    pub connect_attempt: u32,
    pub disconnect_reason: Option<String>,
    /// Seconds since start — drives the title splash wobble.
    pub menu_time: f32,
    /// The player-list key is held: show the tab list overlay.
    pub show_tab_list: bool,
    /// Floating nametags to draw over the scene (already projected to NDC).
    pub nametags: Vec<NameTag>,
    /// Number of tracked remote entities (F3 line).
    pub entities_count: usize,
    /// Render distance in chunks (F3 line).
    pub render_distance: i32,
    /// Sidebar scoreboard title (empty = no sidebar).
    pub sidebar_title: Vec<ChatSpan>,
    /// Sidebar rows, highest score first (already sorted/truncated).
    pub sidebar_lines: Vec<ScoreLine>,
    /// F1 hides the whole in-game HUD (world stays visible).
    pub hud_hidden: bool,
    /// Red damage-flash intensity, 0.0 (none) .. 1.0 (just hit). Drawn even when
    /// the HUD is hidden, like vanilla.
    pub hurt_flash: f32,
    /// Air supply in ticks (0..=300) — drives the bubble row.
    pub air: i32,
    /// Eyes are underwater (bubble row shows even at full air, like vanilla
    /// shows it the moment you dive).
    pub eyes_in_water: bool,
    /// The player is burning: draw the first-person fire overlay.
    pub on_fire: bool,
    /// Our own skin `(url, slim)` for the inventory paper-doll; `None` = Steve.
    pub own_skin: Option<(String, bool)>,
    /// Where to draw the attack-cooldown indicator.
    pub attack_indicator: crate::settings::AttackIndicator,
    /// Trim the F3 overlay to the essentials.
    pub reduced_debug_info: bool,
    /// Opacity of nametag backdrops, 0..=1 (Accessibility setting).
    pub text_bg_opacity: f32,
    /// The connected server's address (for the pause menu's "Copy Server IP").
    pub server_address: String,
    /// Seconds since the current server session started (for Statistics).
    pub session_secs: f32,
}

/// One projected nametag: normalized device coords (x/y ∈ [-1, 1], origin at
/// screen center, +y up) plus the camera distance (for sizing/ordering).
#[derive(Clone)]
pub struct NameTag {
    pub ndc: [f32; 2],
    pub dist: f32,
    /// Text line height as a fraction of the viewport height (perspective
    /// size: shrinks with distance like vanilla's world-space billboards).
    pub scale: f32,
    /// Styled name spans (team colors / custom-name formatting; no raw `§`).
    pub spans: Vec<ChatSpan>,
}

pub enum HudAction {
    SendChat(String),
    Connect { address: String, username: String },
    /// An Options screen changed a setting: persist it and apply
    /// vsync/fullscreen/GUI-scale/render-distance.
    SettingsChanged,
    /// Pause menu → "Back to Game": re-grab the mouse.
    Resume,
    /// Pause menu → "Disconnect": leave the server, return to the title screen.
    Disconnect,
    /// Disconnect overlay → "Back to title": clear the error, show the menu.
    BackToMenu,
    Quit,
    /// Chat closed (Enter/Esc) — the app re-grabs the mouse.
    ChatClosed,
    /// Ask the server for command completions.
    TabComplete { id: u32, text: String },
    /// Container slot interaction.
    SlotClick { window_id: i32, slot: u16, kind: SlotClickKind },
    SelectTrade { index: u32 },
    /// Open a URL in the system browser (pause-menu Feedback / Report Bugs).
    OpenUrl(String),
    /// Open the DolphinClient config/game folder in the file manager.
    OpenGameFolder,
}

/// Which pre-game screen is showing (only when not connected).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Title,
    /// The vanilla server list.
    Multiplayer,
    DirectConnect,
    /// Add (`None`) or edit (`Some(index)`) a saved server.
    EditServer(Option<usize>),
    Options,
}

/// In-game pause state (only when connected).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pause {
    None,
    Menu,
    Options,
    /// The (read-only) Advancements screen reached from the pause menu.
    Advancements,
    /// The Statistics screen (session stats) reached from the pause menu.
    Statistics,
}

/// Which category of the Options screen is showing (pre-game and in-game share
/// this). Root is the top-level list that links to the sub-screens.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionsTab {
    Root,
    Video,
    Controls,
    Chat,
    Sound,
    Skin,
    Language,
    Accessibility,
    ResourcePacks,
}

/// Rebindable action currently listening for a key press.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BindField {
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Sneak,
    Sprint,
    Chat,
    Command,
    Inventory,
    Drop,
    SwapOffhand,
    PlayerList,
    Hotbar1,
    Hotbar2,
    Hotbar3,
    Hotbar4,
    Hotbar5,
    Hotbar6,
    Hotbar7,
    Hotbar8,
    Hotbar9,
    Perspective,
    HideHud,
    Zoom,
    Fullscreen,
    Debug,
}

impl BindField {
    pub const ALL: [BindField; 27] = [
        BindField::Forward,
        BindField::Back,
        BindField::Left,
        BindField::Right,
        BindField::Jump,
        BindField::Sneak,
        BindField::Sprint,
        BindField::Chat,
        BindField::Command,
        BindField::Inventory,
        BindField::Drop,
        BindField::SwapOffhand,
        BindField::PlayerList,
        BindField::Hotbar1,
        BindField::Hotbar2,
        BindField::Hotbar3,
        BindField::Hotbar4,
        BindField::Hotbar5,
        BindField::Hotbar6,
        BindField::Hotbar7,
        BindField::Hotbar8,
        BindField::Hotbar9,
        BindField::Perspective,
        BindField::HideHud,
        BindField::Zoom,
        BindField::Fullscreen,
        BindField::Debug,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BindField::Forward => "Vorwärts",
            BindField::Back => "Rückwärts",
            BindField::Left => "Links",
            BindField::Right => "Rechts",
            BindField::Jump => "Springen",
            BindField::Sneak => "Schleichen",
            BindField::Sprint => "Sprinten",
            BindField::Chat => "Chat öffnen",
            BindField::Command => "Befehl eingeben",
            BindField::Inventory => "Inventar",
            BindField::Drop => "Gegenstand fallen lassen",
            BindField::SwapOffhand => "Hände tauschen",
            BindField::PlayerList => "Spielerliste",
            BindField::Hotbar1 => "Hotbar-Slot 1",
            BindField::Hotbar2 => "Hotbar-Slot 2",
            BindField::Hotbar3 => "Hotbar-Slot 3",
            BindField::Hotbar4 => "Hotbar-Slot 4",
            BindField::Hotbar5 => "Hotbar-Slot 5",
            BindField::Hotbar6 => "Hotbar-Slot 6",
            BindField::Hotbar7 => "Hotbar-Slot 7",
            BindField::Hotbar8 => "Hotbar-Slot 8",
            BindField::Hotbar9 => "Hotbar-Slot 9",
            BindField::Perspective => "Perspektive (F5)",
            BindField::HideHud => "HUD ausblenden",
            BindField::Zoom => "Zoom (halten)",
            BindField::Fullscreen => "Vollbild",
            BindField::Debug => "Debug-Overlay",
        }
    }

    pub fn get(self, keys: &KeyBinds) -> &str {
        match self {
            BindField::Forward => &keys.forward,
            BindField::Back => &keys.back,
            BindField::Left => &keys.left,
            BindField::Right => &keys.right,
            BindField::Jump => &keys.jump,
            BindField::Sneak => &keys.sneak,
            BindField::Sprint => &keys.sprint,
            BindField::Chat => &keys.chat,
            BindField::Command => &keys.command,
            BindField::Inventory => &keys.inventory,
            BindField::Drop => &keys.drop,
            BindField::SwapOffhand => &keys.swap_offhand,
            BindField::PlayerList => &keys.player_list,
            BindField::Hotbar1 => &keys.hotbar_1,
            BindField::Hotbar2 => &keys.hotbar_2,
            BindField::Hotbar3 => &keys.hotbar_3,
            BindField::Hotbar4 => &keys.hotbar_4,
            BindField::Hotbar5 => &keys.hotbar_5,
            BindField::Hotbar6 => &keys.hotbar_6,
            BindField::Hotbar7 => &keys.hotbar_7,
            BindField::Hotbar8 => &keys.hotbar_8,
            BindField::Hotbar9 => &keys.hotbar_9,
            BindField::Perspective => &keys.perspective,
            BindField::HideHud => &keys.hide_hud,
            BindField::Zoom => &keys.zoom,
            BindField::Fullscreen => &keys.fullscreen,
            BindField::Debug => &keys.debug,
        }
    }

    pub fn set(self, keys: &mut KeyBinds, id: String) {
        match self {
            BindField::Forward => keys.forward = id,
            BindField::Back => keys.back = id,
            BindField::Left => keys.left = id,
            BindField::Right => keys.right = id,
            BindField::Jump => keys.jump = id,
            BindField::Sneak => keys.sneak = id,
            BindField::Sprint => keys.sprint = id,
            BindField::Chat => keys.chat = id,
            BindField::Command => keys.command = id,
            BindField::Inventory => keys.inventory = id,
            BindField::Drop => keys.drop = id,
            BindField::SwapOffhand => keys.swap_offhand = id,
            BindField::PlayerList => keys.player_list = id,
            BindField::Hotbar1 => keys.hotbar_1 = id,
            BindField::Hotbar2 => keys.hotbar_2 = id,
            BindField::Hotbar3 => keys.hotbar_3 = id,
            BindField::Hotbar4 => keys.hotbar_4 = id,
            BindField::Hotbar5 => keys.hotbar_5 = id,
            BindField::Hotbar6 => keys.hotbar_6 = id,
            BindField::Hotbar7 => keys.hotbar_7 = id,
            BindField::Hotbar8 => keys.hotbar_8 = id,
            BindField::Hotbar9 => keys.hotbar_9 = id,
            BindField::Perspective => keys.perspective = id,
            BindField::HideHud => keys.hide_hud = id,
            BindField::Zoom => keys.zoom = id,
            BindField::Fullscreen => keys.fullscreen = id,
            BindField::Debug => keys.debug = id,
        }
    }
}

/// How long a subtitle stays visible.
const SUBTITLE_SECS: f32 = 3.0;

pub struct Hud {
    pub show_debug: bool,
    pub chat: ChatState,
    pub tab: TabListState,
    /// Currently open container screen (id 0 = own inventory, opened locally).
    container: Option<ContainerView>,
    /// Latest own-inventory content (window id 0) for the E screen.
    own_slots: Vec<Option<ItemSnapshot>>,
    own_carried: Option<ItemSnapshot>,

    /// A Controls row is waiting for a key press.
    pub rebinding: Option<BindField>,

    /// Recent sound subtitles (text, arrival).
    subtitles: VecDeque<(String, Instant)>,

    screen: Screen,
    pause: Pause,
    options_tab: OptionsTab,

    // --- server list -------------------------------------------------------
    store: ServerListStore,
    pings: Vec<PingState>,
    icons: Vec<Option<TextureHandle>>,
    selected: Option<usize>,
    pinger: Pinger,
    /// Generation counter — stale ping results are dropped.
    ping_gen: u64,
    /// Edit-screen fields.
    edit_name: String,
    edit_address: String,

    /// Direct-connect fields (persist across frames).
    address: String,
    username: String,
    /// Offline mode → the connect screens show an editable username.
    offline: bool,
    /// Set while a menu text field wants the keyboard (app must not treat keys
    /// as movement). Recomputed every `run`.
    menu_wants_keyboard: bool,
    /// Address of the current connect attempt (shown in the Connecting overlay).
    connecting_to: String,
}

impl Hud {
    pub fn new(default_server: String, offline: bool, player_name: String) -> Self {
        let store = ServerListStore::load();
        let n = store.servers.len();
        Self {
            show_debug: false,
            chat: ChatState::default(),
            tab: TabListState::default(),
            container: None,
            own_slots: Vec::new(),
            own_carried: None,
            rebinding: None,
            subtitles: VecDeque::new(),
            screen: Screen::Title,
            pause: Pause::None,
            options_tab: OptionsTab::Root,
            store,
            pings: vec![PingState::Idle; n],
            icons: vec![None; n],
            selected: None,
            pinger: Pinger::default(),
            ping_gen: 0,
            edit_name: String::new(),
            edit_address: String::new(),
            address: if default_server.trim().is_empty() {
                "localhost".into()
            } else {
                default_server
            },
            username: if player_name.trim().is_empty() { "Dolphin".into() } else { player_name },
            offline,
            menu_wants_keyboard: false,
            connecting_to: String::new(),
        }
    }

    /// True while a text field wants keyboard focus (app must not treat keys
    /// as movement).
    pub fn wants_keyboard(&self) -> bool {
        self.chat.open || self.menu_wants_keyboard || self.rebinding.is_some()
    }

    pub fn is_paused(&self) -> bool {
        !matches!(self.pause, Pause::None)
    }

    pub fn container_open(&self) -> bool {
        self.container.is_some()
    }

    /// Id of the open server-side container window, if any.
    pub fn open_container_id(&self) -> Option<i32> {
        self.container.as_ref().map(|c| c.id)
    }

    /// Anything that should release the mouse is up.
    pub fn overlay_open(&self) -> bool {
        self.is_paused() || self.chat.open || self.container.is_some()
    }

    /// Esc while in game. Returns whether the mouse should be grabbed after
    /// (true = back to playing). None→Menu, Menu→None, Options→Menu.
    pub fn toggle_pause(&mut self) -> bool {
        self.pause = match self.pause {
            Pause::None => Pause::Menu,
            Pause::Menu => Pause::None,
            // Any sub-screen backs out to the pause menu, like vanilla Esc.
            Pause::Options | Pause::Advancements | Pause::Statistics => Pause::Menu,
        };
        matches!(self.pause, Pause::None)
    }

    /// Return to the title screen (after a disconnect / user quit-to-menu).
    pub fn reset_to_title(&mut self) {
        self.screen = Screen::Title;
        self.pause = Pause::None;
        self.options_tab = OptionsTab::Root;
        self.chat.clear();
        self.container = None;
        self.subtitles.clear();
        self.tab = TabListState::default();
        self.connecting_to.clear();
        self.rebinding = None;
    }

    /// Screenshot/debug helper: jump straight to a menu screen
    /// (0=Title, 1=Multiplayer, 2=Options) and optionally the in-game pause menu.
    pub fn debug_force(&mut self, screen: u8, pause_menu: bool) {
        self.screen = match screen {
            1 => Screen::Multiplayer,
            2 => Screen::Options,
            _ => Screen::Title,
        };
        self.pause = if pause_menu { Pause::Menu } else { Pause::None };
    }

    /// Test helper: jump straight to a pause sub-screen (1=Advancements,
    /// 2=Statistics), for headless screenshots.
    pub fn debug_pause_sub(&mut self, which: u8) {
        self.pause = match which {
            1 => Pause::Advancements,
            2 => Pause::Statistics,
            _ => Pause::Menu,
        };
    }

    pub fn push_chat(&mut self, spans: Vec<ChatSpan>, system: bool) {
        self.chat.push(spans, system);
    }

    /// Subtitle for a played sound (already translated).
    pub fn push_subtitle(&mut self, text: String) {
        // Refresh an identical subtitle instead of stacking duplicates.
        if let Some(e) = self.subtitles.iter_mut().find(|(t, _)| *t == text) {
            e.1 = Instant::now();
            return;
        }
        self.subtitles.push_back((text, Instant::now()));
        while self.subtitles.len() > 5 {
            self.subtitles.pop_front();
        }
    }

    // --- container plumbing (app → hud) --------------------------------------

    pub fn container_opened(
        &mut self,
        id: i32,
        kind: String,
        title: Vec<ChatSpan>,
        slots: Vec<Option<ItemSnapshot>>,
    ) {
        self.container = Some(ContainerView {
            id,
            kind,
            title,
            slots,
            carried: None,
            offers: Vec::new(),
            trade_scroll: 0,
        });
    }

    pub fn container_content(
        &mut self,
        id: i32,
        slots: Vec<Option<ItemSnapshot>>,
        carried: Option<ItemSnapshot>,
    ) {
        if id == 0 {
            self.own_slots = slots.clone();
            self.own_carried = carried.clone();
        }
        if let Some(view) = &mut self.container
            && view.id == id
        {
            view.slots = slots;
            view.carried = carried;
        }
    }

    pub fn container_closed(&mut self, id: i32) {
        if self.container.as_ref().is_some_and(|c| c.id == id) {
            self.container = None;
        }
    }

    /// E pressed: open the local player-inventory screen.
    pub fn open_own_inventory(&mut self) {
        self.container = Some(ContainerView::own_inventory(
            self.own_slots.clone(),
            self.own_carried.clone(),
        ));
    }

    /// Close whatever container screen is up (local view only).
    pub fn close_container_view(&mut self) {
        self.container = None;
    }

    pub fn merchant_offers(&mut self, container_id: i32, offers: Vec<TradeOffer>) {
        if let Some(view) = &mut self.container
            && view.id == container_id
        {
            view.offers = offers;
        }
    }

    /// Build the frame's UI. Called inside `egui::Context::run`.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        state: &HudState,
        settings: &mut GameSettings,
        skins: &mut SkinManager,
        lang: &Lang,
    ) -> Vec<HudAction> {
        let mut actions = Vec::new();
        self.menu_wants_keyboard = false;
        let s = mc.gui_scale(ctx, settings);

        // Server ping results (arrive any time).
        for (token, info) in self.pinger.poll() {
            let (generation, idx) = (token >> 32, (token & 0xFFFF_FFFF) as usize);
            if generation == self.ping_gen && idx < self.pings.len() {
                if let Some(fav) = &info.favicon {
                    let img = egui::ColorImage::from_rgba_unmultiplied(
                        [fav.width() as usize, fav.height() as usize],
                        fav.as_raw(),
                    );
                    self.icons[idx] = Some(ctx.load_texture(
                        format!("favicon-{idx}"),
                        img,
                        egui::TextureOptions::NEAREST,
                    ));
                }
                self.pings[idx] = PingState::Done(info);
            }
        }

        if let Some(reason) = &state.disconnect_reason {
            let reason = reason.clone();
            self.disconnect_screen(ctx, mc, s, &reason, &mut actions);
            return actions;
        }
        if !state.connected {
            if state.connecting {
                self.connecting_screen(ctx, mc, s, state.connect_attempt);
            } else {
                match self.screen {
                    Screen::Title => self.title_screen(ctx, mc, s, state, &mut actions),
                    Screen::Multiplayer => self.server_list_screen(ctx, mc, s, &mut actions),
                    Screen::DirectConnect => self.direct_connect_screen(ctx, mc, s, &mut actions),
                    Screen::EditServer(idx) => self.edit_server_screen(ctx, mc, s, idx),
                    Screen::Options => {
                        self.options_screen(ctx, mc, s, settings, &mut actions, false)
                    }
                }
            }
            return actions;
        }

        // Damage vignette: a red border flash on taking damage (over the world,
        // under menus, shown even when the HUD is hidden — like vanilla).
        if state.hurt_flash > 0.0 {
            self.hurt_vignette(ctx, state.hurt_flash);
        }
        // Burning: animated flames along the bottom of the view (also drawn
        // with the HUD hidden, like vanilla's first-person fire).
        if state.on_fire && state.connected {
            self.fire_overlay(ctx, mc);
        }

        // In game. F1 hides the HUD entirely (except open menus/containers).
        if !state.hud_hidden {
            if self.container.is_none() {
                self.nametags(ctx, mc, s, state);
                self.crosshair(ctx, mc, s, state);
            }
            self.hotbar(ctx, mc, s, state);
            self.status_bars(ctx, mc, s, state);
            self.scoreboard_sidebar(ctx, mc, s, state);
            self.chat.run(ctx, mc, s, settings, &mut actions);
            if settings.subtitles {
                self.subtitle_overlay(ctx, mc, s);
            }
        }
        if let Some(view) = &mut self.container {
            // The inventory ('E') screen shows our own skin as a paper-doll in
            // the recessed preview panel, like vanilla.
            let player_body = if view.kind == "player" {
                state
                    .own_skin
                    .as_ref()
                    .and_then(|(url, slim)| skins.body(ctx, url, *slim))
                    .map(|h| h.id())
            } else {
                None
            };
            container::draw(ctx, mc, s, view, &state.icons, lang, player_body, &mut actions);
        }
        if state.show_tab_list {
            tablist::draw(ctx, mc, s, &self.tab, skins);
        }
        if self.show_debug {
            self.debug_overlay(ctx, mc, state);
        }
        match self.pause {
            Pause::None => {}
            Pause::Menu => self.pause_menu(ctx, mc, s, state, &mut actions),
            Pause::Options => self.options_screen(ctx, mc, s, settings, &mut actions, true),
            Pause::Advancements => self.advancements_screen(ctx, mc, s),
            Pause::Statistics => self.statistics_screen(ctx, mc, s, state),
        }
        actions
    }

    // -- in-game HUD ---------------------------------------------------------

    /// A red border vignette flashed on taking damage. `intensity` 0..1 fades it.
    fn hurt_vignette(&self, ctx: &egui::Context, intensity: f32) {
        let painter = ctx.layer_painter(LayerId::new(Order::Background, Id::new("hurt-flash")));
        let r = ctx.content_rect();
        let a = (intensity.clamp(0.0, 1.0) * 150.0) as u8;
        let red = Color32::from_rgba_unmultiplied(0xB0, 0x00, 0x00, a);
        // Four edge bands (thicker at the corners like the vanilla overlay).
        let t = (r.width().min(r.height()) * 0.16).max(24.0);
        painter.rect_filled(Rect::from_min_max(r.min, pos2(r.right(), r.top() + t)), 0.0, red);
        painter.rect_filled(Rect::from_min_max(pos2(r.left(), r.bottom() - t), r.max), 0.0, red);
        painter.rect_filled(Rect::from_min_max(r.min, pos2(r.left() + t, r.bottom())), 0.0, red);
        painter.rect_filled(Rect::from_min_max(pos2(r.right() - t, r.top()), r.max), 0.0, red);
    }

    /// First-person burning feedback: the animated vanilla fire texture tiled
    /// along the bottom of the screen (two mirrored layers, like vanilla's
    /// ScreenEffectRenderer).
    fn fire_overlay(&self, ctx: &egui::Context, mc: &McUi) {
        let Some(fire) = &mc.tex.fire else { return };
        let painter = ctx.layer_painter(LayerId::new(Order::Background, Id::new("fire-overlay")));
        let r = ctx.content_rect();
        // fire_1.png is a vertical strip of 16×16 frames animated at 20 fps.
        let frames = (fire.size()[1] / fire.size()[0]).max(1);
        let frame = (ctx.input(|i| i.time) * 20.0) as usize % frames;
        let v0 = frame as f32 / frames as f32;
        let v1 = (frame + 1) as f32 / frames as f32;
        let h = r.height() * 0.38;
        let tile_w = h; // square tiles
        let tint = Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 0xB8);
        let n = (r.width() / tile_w).ceil() as i32 + 1;
        for i in 0..n {
            let x = r.left() + i as f32 * tile_w;
            let rect = Rect::from_min_size(pos2(x, r.bottom() - h), vec2(tile_w, h));
            let uv = if i % 2 == 0 {
                Rect::from_min_max(pos2(0.0, v0), pos2(1.0, v1))
            } else {
                // Mirror every other tile so the tiling doesn't read as a loop.
                Rect::from_min_max(pos2(1.0, v0), pos2(0.0, v1))
            };
            painter.image(fire.id(), rect, uv, tint);
        }
        ctx.request_repaint(); // keep the flames animating
    }

    fn crosshair(&self, ctx: &egui::Context, mc: &McUi, s: f32, state: &HudState) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("crosshair")));
        let c = ctx.content_rect().center();
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let sz = mc.tex.crosshair.size_vec2() * s;
        let rect = Rect::from_center_size(c, sz);
        painter.image(mc.tex.crosshair.id(), rect, full, Color32::from_white_alpha(220));

        // Attack-strength indicator: a 16×4 bar just below the crosshair while
        // the melee cooldown is recharging (vanilla "crosshair" indicator).
        // The Hotbar/Off placements are handled in `hotbar`.
        let strength = state.attack_strength.clamp(0.0, 1.0);
        if state.attack_indicator != crate::settings::AttackIndicator::Crosshair {
            return;
        }
        // Fully recharged + an entity in reach: the crossed-swords "ready"
        // sprite (vanilla behavior), so you always see the cooldown state
        // while a target is under the crosshair.
        if strength >= 1.0 && state.target_in_reach {
            if let Some(fullt) = &mc.tex.attack_full {
                let sz = fullt.size_vec2() * s;
                let rect =
                    Rect::from_center_size(pos2(c.x, c.y + 9.0 * s + 2.0 * s), sz);
                painter.image(fullt.id(), rect, full, Color32::from_white_alpha(220));
            }
            return;
        }
        if strength < 1.0 {
            let (bar_w, bar_h) = (16.0 * s, 4.0 * s);
            let bg = Rect::from_min_size(
                pos2(c.x - bar_w / 2.0, c.y + 9.0 * s),
                vec2(bar_w, bar_h),
            );
            match (&mc.tex.attack_bg, &mc.tex.attack_progress) {
                (Some(bgt), Some(prt)) => {
                    painter.image(bgt.id(), bg, full, Color32::WHITE);
                    painter.image(
                        prt.id(),
                        Rect::from_min_size(bg.min, vec2(bar_w * strength, bar_h)),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(strength, 1.0)),
                        Color32::WHITE,
                    );
                }
                _ => {
                    painter.rect_filled(bg, 0.0, Color32::from_black_alpha(150));
                    painter.rect_filled(
                        Rect::from_min_size(bg.min, vec2(bar_w * strength, bar_h)),
                        0.0,
                        Color32::from_rgb(0xC8, 0xC8, 0xC8),
                    );
                }
            }
        }
    }

    /// Floating entity nametags. The app projects each named entity's head to
    /// normalized device coords; here we map NDC → screen points and draw the
    /// name centered over a translucent dark box (vanilla look), shadowed.
    fn nametags(&self, ctx: &egui::Context, mc: &McUi, s: f32, state: &HudState) {
        if state.nametags.is_empty() {
            return;
        }
        let painter = ctx.layer_painter(LayerId::new(Order::Background, Id::new("nametags")));
        let r = ctx.content_rect();
        let (cx0, cy0) = (r.center().x, r.center().y);
        let (hw, hh) = (r.width() * 0.5, r.height() * 0.5);
        let white = Color32::from_rgb(0xFF, 0xFF, 0xFF);
        let time = ctx.input(|i| i.time);
        for tag in &state.nametags {
            // Perspective size: `scale` is the line height as a fraction of
            // the viewport, capped at the GUI-scale size so close-up tags
            // don't balloon. Skip tags that would be under a pixel tall.
            let line_h = (tag.scale * r.height()).min(8.0 * s);
            if line_h < 1.0 {
                continue;
            }
            let k = line_h / 8.0; // font scale factor equivalent
            let pad = 2.0 * k;
            // NDC (+y up) → screen points (+y down).
            let cx = cx0 + tag.ndc[0] * hw;
            let cy = cy0 - tag.ndc[1] * hh;
            let w = mc.font.spans_width(&tag.spans, k);
            let bg = Rect::from_min_max(
                pos2(cx - w * 0.5 - pad, cy - line_h * 0.5 - pad),
                pos2(cx + w * 0.5 + pad, cy + line_h * 0.5 + pad),
            );
            let bg_alpha = (state.text_bg_opacity.clamp(0.0, 1.0) * 255.0) as u8;
            painter.rect_filled(bg, 1.0 * k, Color32::from_black_alpha(bg_alpha));
            mc.font.draw_spans(
                &painter,
                pos2(cx - w * 0.5, cy - line_h * 0.5),
                &tag.spans,
                k,
                white,
                1.0,
                true,
                time,
            );
        }
    }

    /// Vanilla-style sidebar scoreboard: right-aligned and vertically centered,
    /// a centered title over rows that show entry text on the left and the score
    /// in red on the right, over a translucent black panel.
    fn scoreboard_sidebar(&self, ctx: &egui::Context, mc: &McUi, s: f32, state: &HudState) {
        if state.sidebar_lines.is_empty() && state.sidebar_title.is_empty() {
            return;
        }
        let painter = ctx.layer_painter(LayerId::new(Order::Background, Id::new("scoreboard")));
        let r = ctx.content_rect();
        let rows = &state.sidebar_lines;
        let line_h = 9.0 * s;
        let pad = 3.0 * s;
        let gap = 8.0 * s; // minimum space between an entry and its score

        // Panel width fits the widest of the title and any "entry + score" row.
        let title_w = mc.font.spans_width(&state.sidebar_title, s);
        let mut content_w = title_w;
        for row in rows {
            let name_w = mc.font.spans_width(&row.text, s);
            let score_w = if row.hide_number {
                0.0
            } else {
                gap + mc.font.width(&row.score.to_string(), s)
            };
            content_w = content_w.max(name_w + score_w);
        }
        let panel_w = content_w + pad * 2.0;
        let title_h = line_h + pad;
        let total_h = title_h + line_h * rows.len() as f32 + pad;
        let right = r.right() - 2.0 * s;
        let left = right - panel_w;
        let top = (r.center().y - total_h * 0.5).max(r.top() + 2.0 * s);

        let title_rect = Rect::from_min_size(pos2(left, top), vec2(panel_w, title_h));
        let body_rect =
            Rect::from_min_max(pos2(left, title_rect.bottom()), pos2(right, top + total_h));
        painter.rect_filled(body_rect, 0.0, Color32::from_black_alpha(100));
        painter.rect_filled(title_rect, 0.0, Color32::from_black_alpha(140));

        // Centered title.
        mc.font.draw_spans(
            &painter,
            pos2(left + (panel_w - title_w) * 0.5, top + pad * 0.5),
            &state.sidebar_title,
            s,
            Color32::WHITE,
            1.0,
            true,
            0.0,
        );

        // Rows: entry text on the left, score in red on the right.
        let red = Color32::from_rgb(0xFF, 0x55, 0x55);
        let mut y = title_rect.bottom() + pad * 0.5;
        for row in rows {
            mc.font.draw_spans(
                &painter,
                pos2(left + pad, y),
                &row.text,
                s,
                Color32::from_gray(0xE0),
                1.0,
                false,
                0.0,
            );
            if !row.hide_number {
                let sc = row.score.to_string();
                let sw = mc.font.width(&sc, s);
                mc.font.draw(&painter, pos2(right - pad - sw, y), &sc, s, red, false);
            }
            y += line_h;
        }
    }

    /// The vanilla hotbar: 182×22 sprite, 24×23 selection frame, item icons
    /// and stack counts. Bottom-centered like the real HUD.
    fn hotbar(&self, ctx: &egui::Context, mc: &McUi, s: f32, state: &HudState) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("hotbar")));
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let r = ctx.content_rect();
        let bar_size = mc.tex.hotbar.size_vec2() * s; // 182×22 GUI px
        let bar = Rect::from_min_size(
            pos2(r.center().x - bar_size.x / 2.0, r.bottom() - bar_size.y),
            bar_size,
        );
        painter.image(mc.tex.hotbar.id(), bar, full, Color32::WHITE);

        // Selection frame around the active slot (slot pitch 20 GUI px).
        let sel_size = mc.tex.hotbar_selection.size_vec2() * s;
        let sel_x = bar.left() + (state.selected_slot as f32 * 20.0 - 1.0) * s;
        let sel = Rect::from_min_size(pos2(sel_x, bar.top() - 1.0 * s), sel_size);
        painter.image(mc.tex.hotbar_selection.id(), sel, full, Color32::WHITE);

        // Items: 16×16 at x = 3 + i*20, y = 3 (GUI px inside the bar).
        for i in 0..9usize {
            let Some(Some(item)) = state.hotbar.get(i) else { continue };
            let cell = Rect::from_min_size(
                pos2(bar.left() + (3.0 + i as f32 * 20.0) * s, bar.top() + 3.0 * s),
                vec2(16.0 * s, 16.0 * s),
            );
            container::draw_item(&painter, mc, &state.icons, cell, item, s);
        }

        // Off-hand slot: its own box just left of the hotbar (vanilla
        // right-handed layout). Only shown when the off-hand holds something.
        if let Some(item) = &state.offhand {
            let box_size = mc
                .tex
                .hotbar_offhand
                .as_ref()
                .map(|t| t.size_vec2() * s)
                .unwrap_or_else(|| vec2(22.0 * s, 22.0 * s));
            let box_rect = Rect::from_min_size(
                pos2(bar.left() - box_size.x - 1.0 * s, bar.bottom() - box_size.y),
                box_size,
            );
            if let Some(tex) = &mc.tex.hotbar_offhand {
                painter.image(tex.id(), box_rect, full, Color32::WHITE);
            }
            let cell = Rect::from_center_size(box_rect.center(), vec2(16.0 * s, 16.0 * s));
            container::draw_item(&painter, mc, &state.icons, cell, item, s);
        }

        // Just-selected item name, centered above the status bars, fading out.
        if state.item_name_alpha > 0.01 && !state.item_name.is_empty() {
            let name_w = mc.font.spans_width(&state.item_name, s);
            let x = bar.center().x - name_w * 0.5;
            let y = bar.top() - 42.0 * s;
            mc.font.draw_spans(
                &painter,
                pos2(x, y),
                &state.item_name,
                s,
                Color32::WHITE,
                state.item_name_alpha,
                true,
                0.0,
            );
        }

        // Attack indicator in Hotbar mode: a vertical recharge bar just to the
        // right of the hotbar (vanilla's alternative placement).
        let strength = state.attack_strength.clamp(0.0, 1.0);
        if strength < 1.0 && state.attack_indicator == crate::settings::AttackIndicator::Hotbar {
            let (w, h) = (4.0 * s, 18.0 * s);
            let x = bar.right() + 3.0 * s;
            let bg = Rect::from_min_size(pos2(x, bar.center().y - h / 2.0), vec2(w, h));
            painter.rect_filled(bg, 0.0, Color32::from_black_alpha(150));
            let fill_h = h * strength;
            painter.rect_filled(
                Rect::from_min_size(pos2(x, bg.bottom() - fill_h), vec2(w, fill_h)),
                0.0,
                Color32::from_rgb(0xC8, 0xC8, 0xC8),
            );
        }
    }

    /// Hearts, hunger and the XP bar in their vanilla positions above the
    /// hotbar.
    fn status_bars(&self, ctx: &egui::Context, mc: &McUi, s: f32, state: &HudState) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("status-bars")));
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let r = ctx.content_rect();
        let cx = r.center().x;
        let hotbar_top = r.bottom() - 22.0 * s;

        // XP bar: 182×5, sitting 7 GUI px above the hotbar top edge.
        let bar_w = 182.0 * s;
        let xp_rect = Rect::from_min_size(
            pos2(cx - bar_w / 2.0, hotbar_top - 7.0 * s),
            vec2(bar_w, 5.0 * s),
        );
        painter.image(mc.tex.xp_bg.id(), xp_rect, full, Color32::WHITE);
        let fill = state.xp_progress.clamp(0.0, 1.0);
        if fill > 0.0 {
            // Progress sprite, clipped to the fill fraction like vanilla.
            painter.image(
                mc.tex.xp_progress.id(),
                Rect::from_min_size(xp_rect.min, vec2(xp_rect.width() * fill, xp_rect.height())),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(fill, 1.0)),
                Color32::WHITE,
            );
        }
        if state.xp_level > 0 {
            mc.font.draw_anchored(
                &painter,
                pos2(cx, xp_rect.top() - 8.0 * s),
                Align2::CENTER_CENTER,
                &state.xp_level.to_string(),
                s,
                Color32::from_rgb(0x80, 0xFF, 0x20),
                true,
            );
        }

        // Hearts (left) and hunger (right), 9×9 sprites, row 10 px above XP.
        let row_y = hotbar_top - 17.0 * s;
        let icon = vec2(9.0 * s, 9.0 * s);
        let health = state.health.clamp(0.0, 20.0);
        for i in 0..10 {
            let x = cx - 91.0 * s + i as f32 * 8.0 * s;
            let rect = Rect::from_min_size(pos2(x, row_y), icon);
            painter.image(mc.tex.heart_container.id(), rect, full, Color32::WHITE);
            let v = health - (i * 2) as f32;
            if v >= 2.0 {
                painter.image(mc.tex.heart_full.id(), rect, full, Color32::WHITE);
            } else if v >= 1.0 {
                painter.image(mc.tex.heart_half.id(), rect, full, Color32::WHITE);
            }
        }
        let food = state.food.min(20);
        for i in 0..10 {
            let x = cx + 91.0 * s - (i as f32 + 1.0) * 8.0 * s - 1.0 * s;
            let rect = Rect::from_min_size(pos2(x, row_y), icon);
            painter.image(mc.tex.food_empty.id(), rect, full, Color32::WHITE);
            let v = food as i32 - (i * 2) as i32;
            if v >= 2 {
                painter.image(mc.tex.food_full.id(), rect, full, Color32::WHITE);
            } else if v >= 1 {
                painter.image(mc.tex.food_half.id(), rect, full, Color32::WHITE);
            }
        }

        // Air bubbles: right side, one row above hunger, only while diving or
        // recovering air (vanilla math: ceil(air·10/300), popping bubble for
        // the partial one).
        if state.eyes_in_water || state.air < 300 {
            let air = state.air.clamp(0, 300);
            let full_bubbles = (((air - 2).max(0) * 10) as f32 / 300.0).ceil() as i32;
            let total = ((air * 10) as f32 / 300.0).ceil() as i32;
            let bubble_y = row_y - 10.0 * s;
            for i in 0..10 {
                let x = cx + 91.0 * s - (i as f32 + 1.0) * 8.0 * s - 1.0 * s;
                let rect = Rect::from_min_size(pos2(x, bubble_y), icon);
                if i < full_bubbles {
                    if let Some(t) = &mc.tex.air {
                        painter.image(t.id(), rect, full, Color32::WHITE);
                    }
                } else if i < total {
                    if let Some(t) = &mc.tex.air_bursting {
                        painter.image(t.id(), rect, full, Color32::WHITE);
                    }
                } else if let Some(t) = &mc.tex.air_empty {
                    painter.image(t.id(), rect, full, Color32::WHITE);
                }
            }
        }
    }

    /// Sound subtitles, bottom-right like vanilla.
    fn subtitle_overlay(&mut self, ctx: &egui::Context, mc: &McUi, s: f32) {
        self.subtitles
            .retain(|(_, when)| when.elapsed().as_secs_f32() < SUBTITLE_SECS);
        if self.subtitles.is_empty() {
            return;
        }
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("subtitles")));
        let r = ctx.content_rect();
        let mut y = r.bottom() - 60.0 * s;
        for (text, when) in self.subtitles.iter().rev() {
            let alpha =
                (((SUBTITLE_SECS - when.elapsed().as_secs_f32()) / 0.5).clamp(0.0, 1.0) * 255.0)
                    as u8;
            let w = mc.font.width(text, s) + 6.0 * s;
            let rect = Rect::from_min_size(
                pos2(r.right() - w - 4.0 * s, y - LINE_H * s),
                vec2(w, LINE_H * s + 1.0 * s),
            );
            painter.rect_filled(rect, 2.0, Color32::from_black_alpha(alpha / 2 + 60));
            mc.font.draw(
                &painter,
                rect.min + vec2(3.0 * s, 1.0 * s),
                text,
                s,
                Color32::from_rgba_unmultiplied(255, 255, 255, alpha),
                false,
            );
            y -= LINE_H * s + 2.0 * s;
        }
        ctx.request_repaint();
    }

    fn debug_overlay(&self, ctx: &egui::Context, mc: &McUi, s: &HudState) {
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("debug")));
        let r = ctx.content_rect();
        let fs = 1.5; // F3 text is small in vanilla too
        let bx = s.pos[0].floor() as i64;
        let by = s.pos[1].floor() as i64;
        let bz = s.pos[2].floor() as i64;
        let (cx, cz) = (bx >> 4, bz >> 4);
        let (rx, rz) = (bx.rem_euclid(16), bz.rem_euclid(16));
        let (facing, axis) = facing_of(s.yaw);
        // "Reduced Debug Info" (F3+Q in vanilla) keeps just the essentials.
        let lines: Vec<String> = if s.reduced_debug_info {
            vec![
                format!("DolphinClient {} ({:.0} fps)", env!("CARGO_PKG_VERSION"), s.fps),
                format!("XYZ: {:.3} / {:.5} / {:.3}", s.pos[0], s.pos[1], s.pos[2]),
                format!("Facing: {facing}"),
            ]
        } else {
            vec![
                format!("DolphinClient {} ({:.0} fps)", env!("CARGO_PKG_VERSION"), s.fps),
                format!("XYZ: {:.3} / {:.5} / {:.3}", s.pos[0], s.pos[1], s.pos[2]),
                format!("Block: {} {} {}", bx, by, bz),
                format!("Chunk: {} {} {} in {} {}", rx, by, rz, cx, cz),
                format!("Facing: {} ({})  yaw {:.1} / pitch {:.1}", facing, axis, s.yaw, s.pitch),
                format!("Health: {:.1}  Food: {}", s.health, s.food),
                format!("Entities: {}", s.entities_count),
                format!(
                    "C: {}/{} sections  RD: {}",
                    s.sections_drawn, s.sections_total, s.render_distance
                ),
                format!("Mesh queue: {}", s.mesh_queue),
            ]
        };
        let mut y = r.top() + 2.0;
        for l in lines {
            let w = mc.font.width(&l, fs) + 2.0;
            painter.rect_filled(
                Rect::from_min_size(pos2(r.left() + 1.0, y), vec2(w, LINE_H * fs)),
                0.0,
                Color32::from_rgba_unmultiplied(80, 80, 80, 90),
            );
            mc.font.draw(
                &painter,
                pos2(r.left() + 2.0, y + 0.5),
                &l,
                fs,
                Color32::from_rgb(0xE0, 0xE0, 0xE0),
                false,
            );
            y += LINE_H * fs;
        }
    }

    // -- pre-game menus ------------------------------------------------------

    /// Tiled vanilla background: the classic dark dirt out of game, the
    /// translucent `inworld_menu_background` over the running world.
    fn menu_background(&self, ctx: &egui::Context, mc: &McUi, s: f32, order: Order, in_world: bool) {
        let painter = ctx.layer_painter(LayerId::new(order, Id::new(("menu-bg", order))));
        if in_world {
            mcui::tile_background(
                &painter,
                &mc.tex.inworld_bg,
                ctx.content_rect(),
                s,
                Color32::WHITE,
            );
        } else {
            // The iconic dirt screen: dirt tiles multiplied to 25 % grey.
            mcui::tile_background(
                &painter,
                &mc.tex.dirt,
                ctx.content_rect(),
                s,
                Color32::from_gray(64),
            );
        }
    }

    fn title_screen(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        state: &HudState,
        actions: &mut Vec<HudAction>,
    ) {
        // No background here: the renderer draws the rotating panorama.
        self.title_logo(ctx, mc, s, state.menu_time);

        let mut goto: Option<Screen> = None;
        let mut quit = false;
        Area::new(Id::new("title-buttons"))
            .order(Order::Middle)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 10.0 * s))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = BTN_GAP * s;
                // Singleplayer is deliberately disabled — DolphinClient is a
                // multiplayer-only client (no world generation, no saves).
                mcui::button(ui, mc, BTN_W, s, "Singleplayer", false);
                if mcui::button(ui, mc, BTN_W, s, "Multiplayer", true) {
                    goto = Some(Screen::Multiplayer);
                }
                ui.add_space(4.0 * s);
                if mcui::button(ui, mc, BTN_W, s, "Options...", true) {
                    goto = Some(Screen::Options);
                }
                if mcui::button(ui, mc, BTN_W, s, "Quit Game", true) {
                    quit = true;
                }
            });
        if let Some(sc) = goto {
            if sc == Screen::Options {
                self.options_tab = OptionsTab::Root;
            }
            if sc == Screen::Multiplayer {
                self.refresh_pings();
            }
            self.screen = sc;
        }
        if quit {
            actions.push(HudAction::Quit);
        }

        // Corner labels, like vanilla.
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("title-corners")));
        let r = ctx.content_rect();
        mc.font.draw(
            &painter,
            pos2(r.left() + 2.0, r.bottom() - LINE_H * s),
            &format!("DolphinClient {} (Minecraft 26.1)", env!("CARGO_PKG_VERSION")),
            s,
            Color32::WHITE,
            true,
        );
        let right_text = "Not affiliated with Mojang";
        mc.font.draw(
            &painter,
            pos2(
                r.right() - mc.font.width(right_text, s) - 2.0,
                r.bottom() - LINE_H * s,
            ),
            right_text,
            s,
            Color32::WHITE,
            true,
        );
    }

    /// The DolphinClient "logo": pixel dolphin + chunky wordmark + splash.
    fn title_logo(&self, ctx: &egui::Context, mc: &McUi, s: f32, time: f32) {
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("title-logo")));
        let r = ctx.content_rect();
        let cx = r.center().x;
        let ty = r.top() + 30.0 * s;

        let word = "DolphinClient";
        let word_s = s * 2.0; // chunky 16-GUI-px letters
        let word_w = mc.font.width(word, word_s);
        let icon = 32.0 * s;
        let total = icon + 6.0 * s + word_w;
        let left = cx - total / 2.0;

        let icon_rect = Rect::from_min_size(
            pos2(left, ty - icon * 0.30),
            vec2(icon, icon),
        );
        painter.image(
            mc.tex.logo.id(),
            icon_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        mc.font.draw(
            &painter,
            pos2(left + icon + 6.0 * s, ty),
            word,
            word_s,
            Color32::WHITE,
            true,
        );

        // Splash: pulses like the vanilla title splash.
        let wob = 1.0 + 0.06 * (time * 6.0).sin();
        let splash = "100% Rust!";
        let sp = pos2(left + total - 6.0 * s, ty + 18.0 * s);
        mc.font.draw_anchored(
            &painter,
            sp,
            Align2::CENTER_CENTER,
            splash,
            s * wob,
            Color32::from_rgb(0xFF, 0xFF, 0x00),
            true,
        );
    }

    // -- server list -----------------------------------------------------------

    fn refresh_pings(&mut self) {
        self.ping_gen += 1;
        self.pings = vec![PingState::Idle; self.store.servers.len()];
        self.icons = vec![None; self.store.servers.len()];
        for i in 0..self.store.servers.len() {
            self.pings[i] = PingState::Pending;
            let token = (self.ping_gen << 32) | i as u64;
            let address = self.store.servers[i].address.clone();
            self.pinger.ping(token, &address);
        }
    }

    fn server_list_screen(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        actions: &mut Vec<HudAction>,
    ) {
        self.menu_background(ctx, mc, s, Order::Background, false);
        self.menu_heading(ctx, mc, s, "Play Multiplayer", Order::Middle);
        let r = ctx.content_rect();
        let time = ctx.input(|i| i.time);

        // Use as much width and height as the window allows: wider rows on wide
        // screens, and a list band that spans everything between the heading and
        // the two button rows so as many servers as possible show at once.
        let list_w = 420.0f32.min(r.width() / s - 20.0).max(200.0) * s;
        let row_h = 36.0 * s;
        let list_top = r.top() + 28.0 * s;
        let list_bottom = r.bottom() - 52.0 * s;

        let mut join_now: Option<usize> = None;
        Area::new(Id::new("server-list"))
            .order(Order::Middle)
            .fixed_pos(pos2(r.center().x - list_w / 2.0, list_top))
            .show(ctx, |ui| {
                ui.set_width(list_w);
                ScrollArea::vertical()
                    .max_height((list_bottom - list_top).max(row_h))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for i in 0..self.store.servers.len() {
                            let (rect, resp) = ui
                                .allocate_exact_size(vec2(list_w, row_h), Sense::click());
                            if resp.clicked() {
                                self.selected = Some(i);
                                mc.click();
                            }
                            if resp.double_clicked() {
                                join_now = Some(i);
                            }
                            self.server_row(ui.painter(), mc, s, i, rect, time);
                            ui.add_space(2.0 * s);
                        }
                        if self.store.servers.is_empty() {
                            let (rect, _) = ui
                                .allocate_exact_size(vec2(list_w, row_h), Sense::hover());
                            mc.font.draw_anchored(
                                ui.painter(),
                                rect.center(),
                                Align2::CENTER_CENTER,
                                "Noch keine Server — füge einen hinzu!",
                                s,
                                Color32::from_rgb(0xA0, 0xA0, 0xA0),
                                true,
                            );
                        }
                    });
            });

        // Buttons (two vanilla rows at the bottom).
        let sel_ok = self.selected.is_some_and(|i| i < self.store.servers.len());
        let mut goto: Option<Screen> = None;
        let mut delete = false;
        let mut refresh = false;
        Area::new(Id::new("server-buttons"))
            .order(Order::Middle)
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -10.0 * s))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(4.0 * s, BTN_GAP * s);
                ui.horizontal(|ui| {
                    if mcui::button(ui, mc, 100.0, s, "Join Server", sel_ok) {
                        join_now = self.selected;
                    }
                    if mcui::button(ui, mc, 100.0, s, "Direct Connect", true) {
                        goto = Some(Screen::DirectConnect);
                    }
                    if mcui::button(ui, mc, 100.0, s, "Add Server", true) {
                        self.edit_name = "Minecraft Server".into();
                        self.edit_address.clear();
                        goto = Some(Screen::EditServer(None));
                    }
                });
                ui.horizontal(|ui| {
                    if mcui::button(ui, mc, 74.0, s, "Edit", sel_ok)
                        && let Some(i) = self.selected
                    {
                        self.edit_name = self.store.servers[i].name.clone();
                        self.edit_address = self.store.servers[i].address.clone();
                        goto = Some(Screen::EditServer(Some(i)));
                    }
                    if mcui::button(ui, mc, 74.0, s, "Delete", sel_ok) {
                        delete = true;
                    }
                    if mcui::button(ui, mc, 74.0, s, "Refresh", true) {
                        refresh = true;
                    }
                    if mcui::button(ui, mc, 74.0, s, "Back", true) {
                        goto = Some(Screen::Title);
                    }
                });
            });

        if delete && let Some(i) = self.selected {
            self.store.servers.remove(i);
            self.store.save();
            self.selected = None;
            self.refresh_pings();
        }
        if refresh {
            self.refresh_pings();
        }
        if let Some(i) = join_now
            && i < self.store.servers.len()
        {
            let addr = self.store.servers[i].address.trim().to_string();
            self.connecting_to = addr.clone();
            actions.push(HudAction::Connect {
                address: addr,
                username: self.username.trim().to_string(),
            });
        }
        if let Some(g) = goto {
            self.screen = g;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.screen = Screen::Title;
        }
    }

    /// One server row: icon, name, MOTD spans, players + ping icon.
    fn server_row(
        &self,
        painter: &egui::Painter,
        mc: &McUi,
        s: f32,
        i: usize,
        rect: Rect,
        time: f64,
    ) {
        let selected = self.selected == Some(i);
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(90));
        if selected {
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0f32.max(s * 0.5), Color32::from_gray(160)),
                egui::StrokeKind::Inside,
            );
        }
        let icon_rect = Rect::from_min_size(
            rect.min + vec2(2.0 * s, 2.0 * s),
            vec2(32.0 * s, 32.0 * s),
        );
        let icon = self.icons.get(i).and_then(|t| t.as_ref()).unwrap_or(&mc.tex.unknown_server);
        painter.image(
            icon.id(),
            icon_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        let name = &self.store.servers[i].name;
        let text_x = icon_rect.right() + 3.0 * s;
        mc.font.draw(
            painter,
            pos2(text_x, rect.top() + 2.0 * s),
            name,
            s,
            Color32::WHITE,
            true,
        );

        match self.pings.get(i) {
            Some(PingState::Done(info)) => {
                // MOTD: up to 2 wrapped lines.
                let motd_w = rect.right() - text_x - 34.0 * s;
                let lines = crate::app::chat::wrap_spans(mc, &info.motd, s, motd_w);
                for (li, line) in lines.iter().take(2).enumerate() {
                    mc.font.draw_spans(
                        painter,
                        pos2(text_x, rect.top() + (12.0 + li as f32 * 10.0) * s),
                        line,
                        s,
                        Color32::from_rgb(0xA0, 0xA0, 0xA0),
                        1.0,
                        true,
                        time,
                    );
                }
                if info.error.is_none() {
                    // Players + ping icon, top-right.
                    let players = format!("{}/{}", info.online, info.max);
                    let pw = mc.font.width(&players, s);
                    mc.font.draw(
                        painter,
                        pos2(rect.right() - pw - 15.0 * s, rect.top() + 2.0 * s),
                        &players,
                        s,
                        Color32::from_rgb(0xA0, 0xA0, 0xA0),
                        true,
                    );
                    let bars = if !info.protocol_ok {
                        5
                    } else {
                        match info.latency_ms {
                            0..150 => 4usize,
                            150..300 => 3,
                            300..600 => 2,
                            600..1000 => 1,
                            _ => 0,
                        }
                    };
                    painter.image(
                        mc.tex.ping[bars].id(),
                        Rect::from_min_size(
                            pos2(rect.right() - 12.0 * s, rect.top() + 2.0 * s),
                            vec2(10.0 * s, 7.0 * s),
                        ),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    if !info.protocol_ok && !info.version.is_empty() {
                        let v = format!("Version: {}", info.version);
                        let vw = mc.font.width(&v, s);
                        mc.font.draw(
                            painter,
                            pos2(rect.right() - vw - 15.0 * s, rect.top() + 12.0 * s),
                            &v,
                            s,
                            Color32::from_rgb(0xFF, 0x55, 0x55),
                            true,
                        );
                    }
                }
            }
            Some(PingState::Pending) => {
                mc.font.draw(
                    painter,
                    pos2(text_x, rect.top() + 12.0 * s),
                    "Pinging...",
                    s,
                    Color32::from_rgb(0x80, 0x80, 0x80),
                    true,
                );
            }
            _ => {}
        }
        // Address in small gray at the bottom.
        mc.font.draw(
            painter,
            pos2(text_x, rect.bottom() - 10.0 * s),
            &self.store.servers[i].address,
            s * 0.9,
            Color32::from_gray(110),
            false,
        );
    }

    fn edit_server_screen(&mut self, ctx: &egui::Context, mc: &McUi, s: f32, idx: Option<usize>) {
        self.menu_background(ctx, mc, s, Order::Background, false);
        let title = if idx.is_some() { "Edit Server Info" } else { "Add Server" };
        self.menu_heading(ctx, mc, s, title, Order::Middle);
        self.menu_wants_keyboard = true;

        let mut done = false;
        let mut cancel = false;
        Area::new(Id::new("edit-server"))
            .order(Order::Middle)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, -10.0 * s))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(4.0 * s, 4.0 * s);
                ui.vertical_centered(|ui| {
                    mcui::label(ui, mc, s, "Server Name", Color32::from_rgb(0xA0, 0xA0, 0xA0));
                    mcui::text_field(ui, mc, BTN_W, s, &mut self.edit_name, "");
                    ui.add_space(4.0 * s);
                    mcui::label(ui, mc, s, "Server Address", Color32::from_rgb(0xA0, 0xA0, 0xA0));
                    mcui::text_field(ui, mc, BTN_W, s, &mut self.edit_address, "host oder host:port");
                    ui.add_space(8.0 * s);
                    let ok = !self.edit_address.trim().is_empty();
                    if mcui::button(ui, mc, BTN_W, s, "Done", ok) {
                        done = true;
                    }
                    if mcui::button(ui, mc, BTN_W, s, "Cancel", true) {
                        cancel = true;
                    }
                });
            });
        if done {
            let name = if self.edit_name.trim().is_empty() {
                "Minecraft Server".to_string()
            } else {
                self.edit_name.trim().to_string()
            };
            let server = SavedServer { name, address: self.edit_address.trim().to_string() };
            match idx {
                Some(i) if i < self.store.servers.len() => self.store.servers[i] = server,
                _ => self.store.servers.push(server),
            }
            self.store.save();
            self.screen = Screen::Multiplayer;
            self.refresh_pings();
        }
        if cancel || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.screen = Screen::Multiplayer;
        }
    }

    fn direct_connect_screen(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        actions: &mut Vec<HudAction>,
    ) {
        self.menu_background(ctx, mc, s, Order::Background, false);
        self.menu_wants_keyboard = true;
        self.menu_heading(ctx, mc, s, "Direct Connect", Order::Middle);

        let offline = self.offline;
        let mut join = false;
        let mut back = false;
        Area::new(Id::new("direct-connect"))
            .order(Order::Middle)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(4.0 * s, 4.0 * s);
                ui.vertical_centered(|ui| {
                    mcui::label(ui, mc, s, "Server Address", Color32::from_rgb(0xA0, 0xA0, 0xA0));
                    mcui::text_field(ui, mc, BTN_W, s, &mut self.address, "host oder host:port");
                    if offline {
                        ui.add_space(2.0 * s);
                        mcui::label(ui, mc, s, "Username (offline)", Color32::from_rgb(0xA0, 0xA0, 0xA0));
                        mcui::text_field(ui, mc, BTN_W, s, &mut self.username, "");
                    }
                    ui.add_space(6.0 * s);
                    let can_join = !self.address.trim().is_empty()
                        && (!offline || !self.username.trim().is_empty());
                    if mcui::button(ui, mc, BTN_W, s, "Join Server", can_join)
                        || (can_join && ctx.input(|i| i.key_pressed(Key::Enter)))
                    {
                        join = true;
                    }
                    if mcui::button(ui, mc, BTN_W, s, "Back", true) {
                        back = true;
                    }
                });
            });

        if join {
            let addr = self.address.trim().to_string();
            self.connecting_to = addr.clone();
            actions.push(HudAction::Connect {
                address: addr,
                username: self.username.trim().to_string(),
            });
        }
        if back || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.screen = Screen::Multiplayer;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn options_screen(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        settings: &mut GameSettings,
        actions: &mut Vec<HudAction>,
        in_game: bool,
    ) {
        let order = if in_game { Order::Tooltip } else { Order::Middle };
        if in_game {
            self.menu_background(ctx, mc, s, Order::Foreground, true);
        } else {
            self.menu_background(ctx, mc, s, Order::Background, false);
        }
        let tab = self.options_tab;
        let title = match tab {
            OptionsTab::Root => "Options",
            OptionsTab::Video => "Video Settings",
            OptionsTab::Controls => "Controls",
            OptionsTab::Chat => "Chat Settings",
            OptionsTab::Sound => "Music & Sounds",
            OptionsTab::Skin => "Skin Customization",
            OptionsTab::Language => "Language",
            OptionsTab::Accessibility => "Accessibility Settings",
            OptionsTab::ResourcePacks => "Resource Packs",
        };
        self.menu_heading(ctx, mc, s, title, order);

        let mut changed = false;
        let mut done = false;
        let mut goto: Option<OptionsTab> = None;
        let mut rebind: Option<Option<BindField>> = None;
        let max_h = (ctx.content_rect().height() - 100.0 * s).max(120.0);
        let rebinding = self.rebinding;
        Area::new(Id::new(("options-screen", in_game)))
            .order(order)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 8.0 * s))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(10.0 * s, BTN_GAP * s);
                ui.set_width(ROW_W * s + 16.0);
                ScrollArea::vertical()
                    .max_height(max_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| match tab {
                            OptionsTab::Root => {
                                let (c, g) = root_tab(ui, mc, s, settings);
                                changed |= c;
                                goto = g;
                            }
                            OptionsTab::Video => changed |= video_tab(ui, mc, s, settings),
                            OptionsTab::Controls => {
                                let (c, r) = controls_tab(ui, mc, s, settings, rebinding);
                                changed |= c;
                                if let Some(r) = r {
                                    rebind = Some(r);
                                }
                            }
                            OptionsTab::Chat => changed |= chat_tab(ui, mc, s, settings),
                            OptionsTab::Sound => changed |= sound_tab(ui, mc, s, settings),
                            OptionsTab::Skin => changed |= skin_tab(ui, mc, s, settings),
                            OptionsTab::Language => changed |= language_tab(ui, mc, s, settings),
                            OptionsTab::Accessibility => {
                                changed |= accessibility_tab(ui, mc, s, settings)
                            }
                            OptionsTab::ResourcePacks => resource_packs_tab(ui, mc, s),
                        });
                    });
                ui.add_space(6.0 * s);
                ui.vertical_centered(|ui| {
                    if mcui::button(ui, mc, BTN_W, s, "Done", true) {
                        done = true;
                    }
                });
            });

        if let Some(r) = rebind {
            self.rebinding = r;
        }
        if let Some(t) = goto {
            self.options_tab = t;
            self.rebinding = None;
        }
        if changed {
            actions.push(HudAction::SettingsChanged);
        }
        // Esc: only handled here for the pre-game menu (in-game Esc is the app's
        // pause toggle). Sub-tab → Root; Root → leave Options.
        let esc =
            !in_game && self.rebinding.is_none() && ctx.input(|i| i.key_pressed(Key::Escape));
        if done || esc {
            self.rebinding = None;
            match self.options_tab {
                OptionsTab::Root => {
                    if in_game {
                        self.pause = Pause::Menu;
                    } else {
                        self.screen = Screen::Title;
                    }
                }
                _ => self.options_tab = OptionsTab::Root,
            }
        }
    }

    fn connecting_screen(&self, ctx: &egui::Context, mc: &McUi, s: f32, attempt: u32) {
        self.menu_background(ctx, mc, s, Order::Background, false);
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("connecting")));
        let c = ctx.content_rect().center();
        // On a retry, tell the user we're trying again rather than looking stuck.
        let heading = if attempt > 1 {
            format!("Connecting to the server... (try {attempt})")
        } else {
            "Connecting to the server...".to_string()
        };
        mc.font.draw_anchored(
            &painter,
            c - vec2(0.0, 6.0 * s),
            Align2::CENTER_CENTER,
            &heading,
            s,
            Color32::WHITE,
            true,
        );
        mc.font.draw_anchored(
            &painter,
            c + vec2(0.0, 6.0 * s),
            Align2::CENTER_CENTER,
            &self.connecting_to,
            s,
            Color32::from_rgb(0xA0, 0xA0, 0xA0),
            true,
        );
    }

    // -- in-game pause menu --------------------------------------------------

    /// URLs the pause menu opens (feedback / bug report), mirroring vanilla's
    /// link buttons but pointed at the DolphinClient project.
    const FEEDBACK_URL: &'static str = "https://example.invalid/feedback";
    const BUGS_URL: &'static str = "https://github.com/gravijet/DolphinClient/issues";

    fn pause_menu(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        state: &HudState,
        actions: &mut Vec<HudAction>,
    ) {
        self.menu_background(ctx, mc, s, Order::Foreground, true);
        self.menu_heading(ctx, mc, s, "Game Menu", Order::Tooltip);

        // What the user pressed this frame (evaluated after the closure).
        #[derive(Default)]
        struct Pressed {
            resume: bool,
            advancements: bool,
            statistics: bool,
            feedback: bool,
            bugs: bool,
            copy_ip: bool,
            folder: bool,
            options: bool,
            disconnect: bool,
        }
        let mut p = Pressed::default();

        Area::new(Id::new("pause-menu"))
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(8.0 * s, BTN_GAP * s);
                // Row 1: Back to Game (full width), like vanilla.
                ui.vertical_centered(|ui| {
                    if mcui::button(ui, mc, BTN_W, s, "Back to Game", true) {
                        p.resume = true;
                    }
                });
                // A two-column grid of the remaining options.
                let mut row = |ui: &mut egui::Ui, left: &str, right: &str| -> (bool, bool) {
                    let mut l = false;
                    let mut r = false;
                    ui.horizontal(|ui| {
                        l = mcui::button(ui, mc, COL_W, s, left, true);
                        r = mcui::button(ui, mc, COL_W, s, right, true);
                    });
                    (l, r)
                };
                ui.vertical_centered(|ui| {
                    let (a, b) = row(ui, "Advancements", "Statistics");
                    p.advancements |= a;
                    p.statistics |= b;
                    let (a, b) = row(ui, "Send Feedback", "Report Bugs");
                    p.feedback |= a;
                    p.bugs |= b;
                    let (a, b) = row(ui, "Copy Server IP", "Open Game Folder");
                    p.copy_ip |= a;
                    p.folder |= b;
                    let (a, b) = row(ui, "Options...", "Disconnect");
                    p.options |= a;
                    p.disconnect |= b;
                });
            });

        if p.resume {
            self.pause = Pause::None;
            actions.push(HudAction::Resume);
        }
        if p.advancements {
            self.pause = Pause::Advancements;
        }
        if p.statistics {
            self.pause = Pause::Statistics;
        }
        if p.feedback {
            actions.push(HudAction::OpenUrl(Self::FEEDBACK_URL.to_string()));
        }
        if p.bugs {
            actions.push(HudAction::OpenUrl(Self::BUGS_URL.to_string()));
        }
        if p.copy_ip && !state.server_address.is_empty() {
            ctx.copy_text(state.server_address.clone());
        }
        if p.folder {
            actions.push(HudAction::OpenGameFolder);
        }
        if p.options {
            self.options_tab = OptionsTab::Root;
            self.pause = Pause::Options;
        }
        if p.disconnect {
            actions.push(HudAction::Disconnect);
        }
    }

    /// Read-only Advancements screen. We don't yet sync server advancement
    /// progress, so this shows the vanilla-style panel with an explanatory note
    /// and a Done button — reachable and styled, never a dead button.
    fn advancements_screen(&mut self, ctx: &egui::Context, mc: &McUi, s: f32) {
        self.menu_background(ctx, mc, s, Order::Foreground, true);
        self.menu_heading(ctx, mc, s, "Advancements", Order::Tooltip);
        let lines = [
            "Deine Erfolge erscheinen hier, sobald der Server sie sendet.",
            "Spiele weiter, um Fortschritte freizuschalten!",
        ];
        self.info_panel(ctx, mc, s, &lines);
    }

    /// Statistics screen: live session stats (play time, position, health, …).
    fn statistics_screen(&mut self, ctx: &egui::Context, mc: &McUi, s: f32, state: &HudState) {
        self.menu_background(ctx, mc, s, Order::Foreground, true);
        self.menu_heading(ctx, mc, s, "Statistics", Order::Tooltip);
        let mins = (state.session_secs / 60.0) as u32;
        let secs = (state.session_secs as u32) % 60;
        let (facing, _) = facing_of(state.yaw);
        let lines = [
            format!("Zeit auf dem Server: {mins} min {secs} s"),
            format!("Position: {:.0} / {:.0} / {:.0}", state.pos[0], state.pos[1], state.pos[2]),
            format!("Blickrichtung: {facing}"),
            format!("Leben: {:.0} / 20", state.health),
            format!("Hunger: {} / 20", state.food),
            format!("Erfahrungslevel: {}", state.xp_level),
            format!("Sichtbare Wesen: {}", state.entities_count),
            format!("Bilder pro Sekunde: {:.0}", state.fps),
            format!("Sichtweite: {} Chunks", state.render_distance),
        ];
        let refs: Vec<&str> = lines.iter().map(|l| l.as_str()).collect();
        self.info_panel(ctx, mc, s, &refs);
    }

    /// Shared body for the Advancements/Statistics screens: a centered column of
    /// text lines over the pause background, plus a Done button that returns to
    /// the pause menu.
    fn info_panel(&mut self, ctx: &egui::Context, mc: &McUi, s: f32, lines: &[&str]) {
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("info-panel-text")));
        let r = ctx.content_rect();
        let block_h = lines.len() as f32 * LINE_H * s;
        // Sit the text block above center; the Done button goes below it.
        let mut y = r.center().y - 40.0 * s - block_h / 2.0;
        for &line in lines {
            let w = mc.font.width(line, s);
            mc.font.draw(
                &painter,
                pos2(r.center().x - w / 2.0, y),
                line,
                s,
                Color32::from_gray(0xE0),
                true,
            );
            y += LINE_H * s;
        }

        let mut done = false;
        Area::new(Id::new("info-panel-done"))
            .order(Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, block_h / 2.0 + 8.0 * s))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if mcui::button(ui, mc, BTN_W, s, "Done", true) {
                        done = true;
                    }
                });
            });
        if done || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.pause = Pause::Menu;
        }
    }

    // -- overlays ------------------------------------------------------------

    fn menu_heading(&self, ctx: &egui::Context, mc: &McUi, s: f32, text: &str, order: Order) {
        let painter = ctx.layer_painter(LayerId::new(order, Id::new(("menu-heading", order))));
        let r = ctx.content_rect();
        mc.font.draw_anchored(
            &painter,
            pos2(r.center().x, r.top() + 16.0 * s),
            Align2::CENTER_CENTER,
            text,
            s,
            Color32::WHITE,
            true,
        );
    }

    fn disconnect_screen(
        &mut self,
        ctx: &egui::Context,
        mc: &McUi,
        s: f32,
        reason: &str,
        actions: &mut Vec<HudAction>,
    ) {
        self.menu_background(ctx, mc, s, Order::Background, false);
        self.menu_heading(ctx, mc, s, "Connection Lost", Order::Middle);

        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("disconnect-text")));
        let r = ctx.content_rect();
        let max_w = (BTN_W + 110.0) * s;
        let lines = crate::app::chat::wrap_spans(mc, &[ChatSpan::plain(reason)], s, max_w);
        let block_h = lines.len() as f32 * LINE_H * s;
        let mut y = r.center().y - 30.0 * s - block_h / 2.0;
        for line in &lines {
            let w = mc.font.spans_width(line, s);
            mc.font.draw_spans(
                &painter,
                pos2(r.center().x - w / 2.0, y),
                line,
                s,
                Color32::from_rgb(0xE0, 0xE0, 0xE0),
                1.0,
                true,
                0.0,
            );
            y += LINE_H * s;
        }

        let mut back = false;
        let mut quit = false;
        Area::new(Id::new("disconnect-buttons"))
            .order(Order::Middle)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 30.0 * s))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = BTN_GAP * s;
                if mcui::button(ui, mc, BTN_W, s, "Back to Title Screen", true) {
                    back = true;
                }
                if mcui::button(ui, mc, BTN_W, s, "Quit Game", true) {
                    quit = true;
                }
            });
        if back {
            self.reset_to_title();
            actions.push(HudAction::BackToMenu);
        }
        if quit {
            actions.push(HudAction::Quit);
        }
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "ON" } else { "OFF" }
}

fn gui_scale_label(n: u32) -> String {
    if n == 0 { "Auto".to_string() } else { format!("{n}x") }
}

/// Cardinal facing + axis hint from a vanilla yaw (0 = south/+Z, 90 = west/−X).
fn facing_of(yaw: f32) -> (&'static str, &'static str) {
    let y = yaw.rem_euclid(360.0);
    if !(45.0..315.0).contains(&y) {
        ("south", "Towards positive Z")
    } else if y < 135.0 {
        ("west", "Towards negative X")
    } else if y < 225.0 {
        ("north", "Towards negative Z")
    } else {
        ("east", "Towards positive X")
    }
}

/// A vanilla-style option slider over an f32 range at the given width. The
/// label is rebuilt from the current value every frame ("FOV: 90").
fn opt_slider_w(
    ui: &mut egui::Ui,
    mc: &McUi,
    w: f32,
    s: f32,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    label: impl Fn(f32) -> String,
) -> bool {
    let (min, max) = (*range.start(), *range.end());
    let mut t = ((*value - min) / (max - min)).clamp(0.0, 1.0);
    let text = label(*value);
    let changed = mcui::slider(ui, mc, w, s, &text, &mut t);
    if changed {
        *value = min + t * (max - min);
    }
    changed
}

fn fov_label(v: f32) -> String {
    let r = v.round();
    if (r - 70.0).abs() < 0.5 {
        "FOV: Normal".to_string()
    } else if (r - 110.0).abs() < 0.5 {
        "FOV: Quake Pro".to_string()
    } else {
        format!("FOV: {r:.0}")
    }
}

/// Top-level Options: quick FOV/Brightness + links to the sub-screens, laid
/// out in vanilla's two-column rows.
fn root_tab(
    ui: &mut egui::Ui,
    mc: &McUi,
    s: f32,
    st: &mut GameSettings,
) -> (bool, Option<OptionsTab>) {
    let mut changed = false;
    let mut goto = None;
    ui.horizontal(|ui| {
        changed |= {
            let c = opt_slider_w(ui, mc, COL_W, s, &mut st.fov, 30.0..=110.0, fov_label);
            if c {
                st.fov = st.fov.round();
            }
            c
        };
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.brightness, 0.0..=1.0, |v| {
            if v <= 0.005 {
                "Brightness: Moody".to_string()
            } else if v >= 0.995 {
                "Brightness: Bright".to_string()
            } else {
                format!("Brightness: {:.0}%", v * 100.0)
            }
        });
    });
    ui.add_space(4.0 * s);
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, "Video Settings...", true) {
            goto = Some(OptionsTab::Video);
        }
        if mcui::button(ui, mc, COL_W, s, "Controls...", true) {
            goto = Some(OptionsTab::Controls);
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, "Music & Sounds...", true) {
            goto = Some(OptionsTab::Sound);
        }
        if mcui::button(ui, mc, COL_W, s, "Chat Settings...", true) {
            goto = Some(OptionsTab::Chat);
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, "Skin Customization...", true) {
            goto = Some(OptionsTab::Skin);
        }
        if mcui::button(ui, mc, COL_W, s, "Language...", true) {
            goto = Some(OptionsTab::Language);
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, "Accessibility Settings...", true) {
            goto = Some(OptionsTab::Accessibility);
        }
        if mcui::button(ui, mc, COL_W, s, "Resource Packs...", true) {
            goto = Some(OptionsTab::ResourcePacks);
        }
    });
    (changed, goto)
}

/// Video Settings: everything that affects the renderer & window.
fn video_tab(ui: &mut egui::Ui, mc: &McUi, s: f32, st: &mut GameSettings) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        {
            let mut rd = st.render_distance as f32;
            if opt_slider_w(ui, mc, COL_W, s, &mut rd, 2.0..=32.0, |v| {
                format!("Render Distance: {:.0}", v.round())
            }) {
                st.render_distance = rd.round() as i32;
                changed = true;
            }
        }
        {
            let mut fps = st.max_fps as f32;
            if opt_slider_w(ui, mc, COL_W, s, &mut fps, 0.0..=260.0, |v| {
                let stepped = (v / 5.0).round() * 5.0;
                if stepped < 1.0 {
                    "Max FPS: Unlimited".to_string()
                } else {
                    format!("Max FPS: {stepped:.0}")
                }
            }) {
                st.max_fps = ((fps / 5.0).round() * 5.0) as u32;
                changed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.brightness, 0.0..=1.0, |v| {
            format!("Brightness: {:.0}%", v * 100.0)
        });
        if mcui::button(ui, mc, COL_W, s, &format!("GUI Scale: {}", gui_scale_label(st.gui_scale)), true) {
            st.gui_scale = (st.gui_scale + 1) % 5;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("VSync: {}", on_off(st.vsync)), true) {
            st.vsync = !st.vsync;
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Fullscreen: {}", on_off(st.fullscreen)), true) {
            st.fullscreen = !st.fullscreen;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Graphics: {}", st.graphics.label()), true) {
            st.graphics = st.graphics.next();
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Fog: {}", on_off(st.fog)), true) {
            st.fog = !st.fog;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("View Bobbing: {}", on_off(st.view_bobbing)), true) {
            st.view_bobbing = !st.view_bobbing;
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Particles: {}", st.particles.label()), true) {
            st.particles = st.particles.next();
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.fov_effects, 0.0..=1.0, |v| {
            if v <= 0.005 {
                "FOV Effects: OFF".to_string()
            } else {
                format!("FOV Effects: {:.0}%", v * 100.0)
            }
        });
        if mcui::button(ui, mc, COL_W, s, &format!("Attack Indicator: {}", st.attack_indicator.label()), true) {
            st.attack_indicator = st.attack_indicator.next();
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Damage Tilt: {}", on_off(st.damage_tilt)), true) {
            st.damage_tilt = !st.damage_tilt;
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Reduced Debug Info: {}", on_off(st.reduced_debug_info)), true) {
            st.reduced_debug_info = !st.reduced_debug_info;
            changed = true;
        }
    });
    changed
}

/// Controls: mouse settings, toggles + rebindable keys.
fn controls_tab(
    ui: &mut egui::Ui,
    mc: &McUi,
    s: f32,
    st: &mut GameSettings,
    rebinding: Option<BindField>,
) -> (bool, Option<Option<BindField>>) {
    let mut changed = false;
    let mut rebind: Option<Option<BindField>> = None;
    ui.horizontal(|ui| {
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.sensitivity_pct, 0.0..=200.0, |v| {
            if v <= 0.5 {
                "Sensitivity: *yawn*".to_string()
            } else if (v - 100.0).abs() < 0.5 {
                "Sensitivity: 100%".to_string()
            } else {
                format!("Sensitivity: {v:.0}%")
            }
        });
        if mcui::button(ui, mc, COL_W, s, &format!("Invert Mouse: {}", on_off(st.invert_mouse)), true) {
            st.invert_mouse = !st.invert_mouse;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Sneak: {}", if st.sneak_toggle { "Toggle" } else { "Hold" }), true) {
            st.sneak_toggle = !st.sneak_toggle;
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Sprint: {}", if st.sprint_toggle { "Toggle" } else { "Hold" }), true) {
            st.sprint_toggle = !st.sprint_toggle;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Auto Jump: {}", on_off(st.auto_jump)), true) {
            st.auto_jump = !st.auto_jump;
            changed = true;
        }
    });
    ui.add_space(6.0 * s);
    mcui::label(ui, mc, s, "Key Binds (klicken zum Ändern)", Color32::WHITE);
    ui.add_space(2.0 * s);
    for field in BindField::ALL {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(ROW_W * s - 104.0 * s, 20.0 * s), Sense::hover());
            mc.font.draw(
                ui.painter(),
                pos2(rect.left(), rect.center().y - 4.0 * s),
                field.label(),
                s,
                Color32::from_rgb(0xA0, 0xA0, 0xA0),
                true,
            );
            let listening = rebinding == Some(field);
            let label = if listening {
                "> ??? <".to_string()
            } else {
                key_label(field.get(&st.keys))
            };
            if mcui::button(ui, mc, 100.0, s, &label, true) {
                rebind = Some(if listening { None } else { Some(field) });
            }
        });
    }
    if rebinding.is_some() {
        ui.add_space(2.0 * s);
        mcui::label(
            ui,
            mc,
            s,
            "Drücke eine Taste... (Esc bricht ab)",
            Color32::from_rgb(0xFF, 0xFF, 0x55),
        );
    }
    (changed, rebind)
}

/// Chat Settings: scale, opacity, width, spacing, visibility.
fn chat_tab(ui: &mut egui::Ui, mc: &McUi, s: f32, st: &mut GameSettings) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.chat_scale, 0.5..=2.0, |v| {
            format!("Chat Text Size: {:.0}%", v * 100.0)
        });
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.chat_opacity, 0.0..=1.0, |v| {
            format!("Chat Opacity: {:.0}%", v * 100.0)
        });
    });
    ui.horizontal(|ui| {
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.chat_width, 40.0..=320.0, |v| {
            format!("Chat Width: {v:.0}px")
        });
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.chat_line_spacing, 1.0..=2.0, |v| {
            format!("Line Spacing: {:.0}%", v * 100.0)
        });
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Chat: {}", st.chat_visibility.label()), true) {
            st.chat_visibility = st.chat_visibility.next();
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Colors: {}", on_off(st.chat_colors)), true) {
            st.chat_colors = !st.chat_colors;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Web Links: {}", on_off(st.chat_links)), true) {
            st.chat_links = !st.chat_links;
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Command Suggestions: {}", on_off(st.command_suggestions)), true) {
            st.command_suggestions = !st.command_suggestions;
            changed = true;
        }
    });
    changed
}

/// Volume label: `OFF` at zero, otherwise a percentage — exactly vanilla.
fn vol_label(name: &str, v: f32) -> String {
    if v <= 0.005 {
        format!("{name}: OFF")
    } else {
        format!("{name}: {:.0}%", v * 100.0)
    }
}

/// Music & Sounds: master on top, then category pairs — like vanilla's screen.
fn sound_tab(ui: &mut egui::Ui, mc: &McUi, s: f32, st: &mut GameSettings) -> bool {
    let mut changed = false;
    ui.vertical_centered(|ui| {
        changed |= opt_slider_w(ui, mc, BTN_W, s, &mut st.master_volume, 0.0..=1.0, |v| {
            vol_label("Master Volume", v)
        });
    });
    ui.add_space(2.0 * s);
    let mut fields: [(&str, &mut f32); 9] = [
        ("Music", &mut st.music_volume),
        ("Jukebox/Note Blocks", &mut st.records_volume),
        ("Weather", &mut st.weather_volume),
        ("Blocks", &mut st.blocks_volume),
        ("Hostile Creatures", &mut st.hostile_volume),
        ("Friendly Creatures", &mut st.neutral_volume),
        ("Players", &mut st.players_volume),
        ("Ambient/Environment", &mut st.ambient_volume),
        ("Voice/Speech", &mut st.voice_volume),
    ];
    for pair in fields.chunks_mut(2) {
        ui.horizontal(|ui| {
            for (name, value) in pair.iter_mut() {
                changed |= opt_slider_w(ui, mc, COL_W, s, value, 0.0..=1.0, |v| {
                    vol_label(name, v)
                });
            }
        });
    }
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Show Subtitles: {}", on_off(st.subtitles)), true) {
            st.subtitles = !st.subtitles;
            changed = true;
        }
    });
    changed
}

/// Skin Customization: toggle the model overlay layers on your own body and
/// pick your main hand (vanilla's Skin Customization screen).
fn skin_tab(ui: &mut egui::Ui, mc: &McUi, s: f32, st: &mut GameSettings) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Hat: {}", on_off(st.skin_hat)), true) {
            st.skin_hat = !st.skin_hat;
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Jacket: {}", on_off(st.skin_jacket)), true) {
            st.skin_jacket = !st.skin_jacket;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Right Sleeve: {}", on_off(st.skin_right_sleeve)), true) {
            st.skin_right_sleeve = !st.skin_right_sleeve;
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Left Sleeve: {}", on_off(st.skin_left_sleeve)), true) {
            st.skin_left_sleeve = !st.skin_left_sleeve;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Right Pants Leg: {}", on_off(st.skin_right_pants)), true) {
            st.skin_right_pants = !st.skin_right_pants;
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Left Pants Leg: {}", on_off(st.skin_left_pants)), true) {
            st.skin_left_pants = !st.skin_left_pants;
            changed = true;
        }
    });
    ui.add_space(4.0 * s);
    ui.horizontal(|ui| {
        let hand = if st.left_handed { "Main Hand: Left" } else { "Main Hand: Right" };
        if mcui::button(ui, mc, COL_W, s, hand, true) {
            st.left_handed = !st.left_handed;
            changed = true;
        }
    });
    changed
}

/// Language: pick the item-name / UI language from the packs we ship.
fn language_tab(ui: &mut egui::Ui, mc: &McUi, s: f32, st: &mut GameSettings) -> bool {
    let mut changed = false;
    // (code, display name) — the languages bundled in the asset store.
    const LANGS: &[(&str, &str)] = &[("en_us", "English (US)"), ("de_de", "Deutsch (Deutschland)")];
    ui.vertical_centered(|ui| {
        for (code, name) in LANGS {
            let sel = st.language == *code;
            let label = if sel { format!("» {name} «") } else { (*name).to_string() };
            if mcui::button(ui, mc, BTN_W, s, &label, true) && !sel {
                st.language = (*code).into();
                changed = true;
            }
        }
    });
    ui.add_space(4.0 * s);
    mcui::label(ui, mc, s, "Item- und Menütexte wechseln nach einem Neustart.", Color32::from_rgb(0xA0, 0xA0, 0xA0));
    changed
}

/// Accessibility: text-backdrop opacity + the feedback toggles vanilla groups
/// here (subtitles, damage tilt, dynamic FOV).
fn accessibility_tab(ui: &mut egui::Ui, mc: &McUi, s: f32, st: &mut GameSettings) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.text_background_opacity, 0.0..=1.0, |v| {
            format!("Text Background Opacity: {:.0}%", v * 100.0)
        });
        changed |= opt_slider_w(ui, mc, COL_W, s, &mut st.fov_effects, 0.0..=1.0, |v| {
            if v <= 0.005 {
                "FOV Effects: OFF".to_string()
            } else {
                format!("FOV Effects: {:.0}%", v * 100.0)
            }
        });
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("Show Subtitles: {}", on_off(st.subtitles)), true) {
            st.subtitles = !st.subtitles;
            changed = true;
        }
        if mcui::button(ui, mc, COL_W, s, &format!("Damage Tilt: {}", on_off(st.damage_tilt)), true) {
            st.damage_tilt = !st.damage_tilt;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if mcui::button(ui, mc, COL_W, s, &format!("View Bobbing: {}", on_off(st.view_bobbing)), true) {
            st.view_bobbing = !st.view_bobbing;
            changed = true;
        }
        if mcui::button(
            ui,
            mc,
            COL_W,
            s,
            &format!("Discord Rich Presence: {}", on_off(st.discord_rpc)),
            true,
        ) {
            st.discord_rpc = !st.discord_rpc;
            changed = true;
        }
    });
    changed
}

/// Resource Packs: DolphinClient applies the server's pack automatically; this
/// screen explains that and opens the local cache folder (like vanilla's
/// "Open Pack Folder").
fn resource_packs_tab(ui: &mut egui::Ui, mc: &McUi, s: f32) {
    ui.vertical_centered(|ui| {
        mcui::label(ui, mc, s, "Server-Resource-Packs werden automatisch geladen", Color32::WHITE);
        mcui::label(ui, mc, s, "und angewendet, sobald der Server eins anbietet.", Color32::from_rgb(0xA0, 0xA0, 0xA0));
        ui.add_space(8.0 * s);
        if mcui::button(ui, mc, BTN_W, s, "Pack-Ordner öffnen", true) {
            let dir = crate::settings::GameSettings::config_dir();
            let _ = std::fs::create_dir_all(&dir);
            let _ = open::that(dir);
        }
    });
}

impl Default for Hud {
    fn default() -> Self {
        Self::new(String::new(), true, "Dolphin".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wants_keyboard_follows_chat_and_menu() {
        let mut hud = Hud::default();
        assert!(!hud.wants_keyboard());
        hud.chat.open = true;
        assert!(hud.wants_keyboard());
        hud.chat.open = false;
        hud.menu_wants_keyboard = true;
        assert!(hud.wants_keyboard());
        hud.menu_wants_keyboard = false;
        hud.rebinding = Some(BindField::Jump);
        assert!(hud.wants_keyboard());
    }

    #[test]
    fn pause_toggle_cycles_and_reports_grab() {
        let mut hud = Hud::default();
        assert!(!hud.is_paused());
        // None -> Menu: not playing, don't grab.
        assert!(!hud.toggle_pause());
        assert!(hud.is_paused());
        // Menu -> None: back to playing, grab.
        assert!(hud.toggle_pause());
        assert!(!hud.is_paused());
        // Options -> Menu: still paused, don't grab.
        hud.pause = Pause::Options;
        assert!(!hud.toggle_pause());
        assert!(hud.is_paused());
    }

    #[test]
    fn reset_to_title_clears_state() {
        let mut hud = Hud::default();
        hud.pause = Pause::Menu;
        hud.screen = Screen::Options;
        hud.push_chat(vec![ChatSpan::plain("hi")], false);
        hud.open_own_inventory();
        hud.reset_to_title();
        assert!(!hud.is_paused());
        assert!(matches!(hud.screen, Screen::Title));
        assert_eq!(hud.chat.line_count(), 0);
        assert!(!hud.container_open());
    }

    #[test]
    fn container_plumbing_tracks_ids() {
        let mut hud = Hud::default();
        hud.container_opened(3, "generic_9x3".into(), vec![ChatSpan::plain("Kiste")], vec![None; 63]);
        assert_eq!(hud.open_container_id(), Some(3));
        hud.container_content(3, vec![None; 63], None);
        // Content for another window doesn't clobber the open view.
        hud.container_content(0, vec![None; 46], None);
        assert_eq!(hud.open_container_id(), Some(3));
        hud.container_closed(3);
        assert!(!hud.container_open());
        // Own inventory opens locally from cached id-0 content.
        hud.open_own_inventory();
        assert_eq!(hud.open_container_id(), Some(0));
    }

    #[test]
    fn bind_fields_roundtrip() {
        let mut keys = KeyBinds::default();
        BindField::Jump.set(&mut keys, "KeyJ".into());
        assert_eq!(BindField::Jump.get(&keys), "KeyJ");
        assert_eq!(BindField::Forward.get(&keys), "KeyW");
    }

    #[test]
    fn facing_of_matches_vanilla_cardinals() {
        // Vanilla yaw: 0 = south (+Z), 90 = west (−X), 180 = north (−Z),
        // 270 = east (+X). Boundaries fall on the 45° diagonals.
        assert_eq!(facing_of(0.0).0, "south");
        assert_eq!(facing_of(90.0).0, "west");
        assert_eq!(facing_of(180.0).0, "north");
        assert_eq!(facing_of(270.0).0, "east");
        // Wrapping and negatives normalize.
        assert_eq!(facing_of(360.0).0, "south");
        assert_eq!(facing_of(-90.0).0, "east");
        // Just inside the south wedge on both sides of 0.
        assert_eq!(facing_of(44.0).0, "south");
        assert_eq!(facing_of(316.0).0, "south");
    }
}
