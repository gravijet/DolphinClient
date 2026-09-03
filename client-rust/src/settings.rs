//! Persistent, vanilla-style game settings (Options screens write these).
//!
//! Only options that actually change the running client are stored here — no
//! dead toggles. They persist to a per-user `options.json` next to where the
//! vanilla launcher keeps its config, so tweaks survive restarts.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use winit::keyboard::KeyCode;

/// Graphics quality preset. Affects fog/leaves/cloud detail in the renderer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Graphics {
    Fast,
    Fancy,
}

impl Graphics {
    pub fn label(self) -> &'static str {
        match self {
            Graphics::Fast => "Fast",
            Graphics::Fancy => "Fancy",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Graphics::Fast => Graphics::Fancy,
            Graphics::Fancy => Graphics::Fast,
        }
    }
}

/// How many particles the client spawns (scales server + local bursts), like
/// vanilla's Particles option.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ParticleLevel {
    All,
    Decreased,
    Minimal,
}

impl ParticleLevel {
    pub fn label(self) -> &'static str {
        match self {
            ParticleLevel::All => "All",
            ParticleLevel::Decreased => "Decreased",
            ParticleLevel::Minimal => "Minimal",
        }
    }
    pub fn next(self) -> Self {
        match self {
            ParticleLevel::All => ParticleLevel::Decreased,
            ParticleLevel::Decreased => ParticleLevel::Minimal,
            ParticleLevel::Minimal => ParticleLevel::All,
        }
    }
    /// Fraction of requested particles to actually spawn.
    pub fn factor(self) -> f32 {
        match self {
            ParticleLevel::All => 1.0,
            ParticleLevel::Decreased => 0.5,
            ParticleLevel::Minimal => 0.15,
        }
    }

    /// Whether blocks make their own ambience (torch smoke, campfire columns,
    /// falling petals). Vanilla keeps these on at "Decreased" and drops them
    /// entirely at "Minimal".
    pub fn ambient(self) -> bool {
        !matches!(self, ParticleLevel::Minimal)
    }
}

/// Where the melee attack-strength indicator is drawn (vanilla option).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum AttackIndicator {
    Off,
    #[default]
    Crosshair,
    Hotbar,
}

impl AttackIndicator {
    pub fn label(self) -> &'static str {
        match self {
            AttackIndicator::Off => "Off",
            AttackIndicator::Crosshair => "Crosshair",
            AttackIndicator::Hotbar => "Hotbar",
        }
    }
    pub fn next(self) -> Self {
        match self {
            AttackIndicator::Off => AttackIndicator::Crosshair,
            AttackIndicator::Crosshair => AttackIndicator::Hotbar,
            AttackIndicator::Hotbar => AttackIndicator::Off,
        }
    }
}

/// Vanilla chat visibility modes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ChatVisibility {
    Full,
    /// Only command feedback / system messages.
    System,
    Hidden,
}

/// How a multiplayer entry handles packs offered by that server. This is the
/// same three-way choice shown by vanilla's Edit Server screen: ask every
/// time, always accept, or always decline.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ServerResourcePackPolicy {
    #[default]
    Prompt,
    Enabled,
    Disabled,
}

impl ServerResourcePackPolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Prompt => "Prompt",
            Self::Enabled => "Enabled",
            Self::Disabled => "Disabled",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Prompt => Self::Enabled,
            Self::Enabled => Self::Disabled,
            Self::Disabled => Self::Prompt,
        }
    }
}

impl ChatVisibility {
    pub fn label(self) -> &'static str {
        match self {
            ChatVisibility::Full => "Shown",
            ChatVisibility::System => "Commands Only",
            ChatVisibility::Hidden => "Hidden",
        }
    }
    pub fn next(self) -> Self {
        match self {
            ChatVisibility::Full => ChatVisibility::System,
            ChatVisibility::System => ChatVisibility::Hidden,
            ChatVisibility::Hidden => ChatVisibility::Full,
        }
    }
}

// ---------------------------------------------------------------------------
// Key binds
// ---------------------------------------------------------------------------

/// Rebindable actions, each stored as the winit `KeyCode` debug name
/// ("KeyW", "Space", "ShiftLeft", …) so the config stays readable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyBinds {
    pub forward: String,
    pub back: String,
    pub left: String,
    pub right: String,
    pub jump: String,
    pub sneak: String,
    pub sprint: String,
    pub chat: String,
    pub command: String,
    pub inventory: String,
    pub drop: String,
    pub player_list: String,
    /// Swap the main-hand and off-hand items (vanilla F).
    pub swap_offhand: String,
    /// Nine hotbar slot selectors (default Digit1..Digit9).
    pub hotbar_1: String,
    pub hotbar_2: String,
    pub hotbar_3: String,
    pub hotbar_4: String,
    pub hotbar_5: String,
    pub hotbar_6: String,
    pub hotbar_7: String,
    pub hotbar_8: String,
    pub hotbar_9: String,
    /// Cycle camera perspective (vanilla F5).
    pub perspective: String,
    /// Hide/show the HUD (vanilla F1).
    pub hide_hud: String,
    /// Hold to zoom the view (Optifine-style, default C).
    pub zoom: String,
    /// Toggle fullscreen (vanilla F11).
    pub fullscreen: String,
    /// Toggle the debug overlay (vanilla F3).
    pub debug: String,
}

impl Default for KeyBinds {
    fn default() -> Self {
        Self {
            forward: "KeyW".into(),
            back: "KeyS".into(),
            left: "KeyA".into(),
            right: "KeyD".into(),
            jump: "Space".into(),
            sneak: "ShiftLeft".into(),
            sprint: "ControlLeft".into(),
            chat: "KeyT".into(),
            command: "Slash".into(),
            inventory: "KeyE".into(),
            drop: "KeyQ".into(),
            player_list: "Tab".into(),
            swap_offhand: "KeyF".into(),
            hotbar_1: "Digit1".into(),
            hotbar_2: "Digit2".into(),
            hotbar_3: "Digit3".into(),
            hotbar_4: "Digit4".into(),
            hotbar_5: "Digit5".into(),
            hotbar_6: "Digit6".into(),
            hotbar_7: "Digit7".into(),
            hotbar_8: "Digit8".into(),
            hotbar_9: "Digit9".into(),
            perspective: "F5".into(),
            hide_hud: "F1".into(),
            zoom: "KeyC".into(),
            fullscreen: "F11".into(),
            debug: "F3".into(),
        }
    }
}

impl KeyBinds {
    /// The nine hotbar-slot binds in order (slot 0..8).
    pub fn hotbar(&self) -> [&str; 9] {
        [
            &self.hotbar_1,
            &self.hotbar_2,
            &self.hotbar_3,
            &self.hotbar_4,
            &self.hotbar_5,
            &self.hotbar_6,
            &self.hotbar_7,
            &self.hotbar_8,
            &self.hotbar_9,
        ]
    }
}

/// The stable identifier of a `KeyCode` used in the config ("KeyW", "F5", …).
pub fn key_id(code: KeyCode) -> String {
    format!("{code:?}")
}

/// Human-readable key label for the Controls screen ("W", "Left Shift", …).
pub fn key_label(id: &str) -> String {
    if let Some(rest) = id.strip_prefix("Key") {
        return rest.to_string();
    }
    if let Some(rest) = id.strip_prefix("Digit") {
        return rest.to_string();
    }
    match id {
        "ShiftLeft" => "Left Shift".into(),
        "ShiftRight" => "Right Shift".into(),
        "ControlLeft" => "Left Ctrl".into(),
        "ControlRight" => "Right Ctrl".into(),
        "AltLeft" => "Left Alt".into(),
        "AltRight" => "Right Alt".into(),
        "Slash" => "/".into(),
        "Backslash" => "\\".into(),
        "Comma" => ",".into(),
        "Period" => ".".into(),
        "Semicolon" => ";".into(),
        "Quote" => "'".into(),
        "Minus" => "-".into(),
        "Equal" => "=".into(),
        "BracketLeft" => "[".into(),
        "BracketRight" => "]".into(),
        "ArrowUp" => "Up".into(),
        "ArrowDown" => "Down".into(),
        "ArrowLeft" => "Left".into(),
        "ArrowRight" => "Right".into(),
        "Backquote" => "`".into(),
        other => other.to_string(),
    }
}

impl KeyBinds {
    /// Does `code` match the bind stored in `id`?
    pub fn matches(id: &str, code: KeyCode) -> bool {
        key_id(code) == id
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GameSettings {
    // --- Video ---------------------------------------------------------------
    /// Field of view in degrees (30..=110). 70 is vanilla "Normal".
    pub fov: f32,
    /// Chunk render distance (Chebyshev radius), 2..=32.
    pub render_distance: i32,
    /// Frame cap. 0 = unlimited. Ignored while `vsync` is on.
    pub max_fps: u32,
    /// Vertical sync (FIFO present). Off = uncapped (Immediate) for max FPS.
    pub vsync: bool,
    /// GUI scale. 0 = Auto (follow the OS scale factor), else 1..=4.
    pub gui_scale: u32,
    /// Screen brightness / gamma, 0.0 (Moody) .. 1.0 (Bright).
    pub brightness: f32,
    /// Launch/run in borderless fullscreen.
    pub fullscreen: bool,
    /// Fast vs Fancy graphics preset.
    pub graphics: Graphics,
    /// Distance fog. Off keeps far chunks crisp (and slightly faster).
    pub fog: bool,
    /// Camera bob while walking.
    pub view_bobbing: bool,
    /// How many particles to spawn (All / Decreased / Minimal).
    pub particles: ParticleLevel,
    /// Dynamic FOV effect strength (0 = fixed FOV, 1 = full sprint zoom).
    pub fov_effects: f32,
    /// Where the attack-cooldown indicator is drawn.
    pub attack_indicator: AttackIndicator,
    /// Red screen flash + camera-shake feedback when taking damage.
    pub damage_tilt: bool,
    /// Smooth lighting: light is averaged across each block face instead of
    /// being flat per face. Off is the old blocky look (and meshes marginally
    /// faster).
    pub smooth_lighting: bool,

    // --- Controls ------------------------------------------------------------
    /// Mouse sensitivity as a vanilla 0..=200 percentage (100 = default).
    pub sensitivity_pct: f32,
    /// Invert the vertical mouse axis.
    pub invert_mouse: bool,
    /// Sneak is a toggle instead of hold.
    pub sneak_toggle: bool,
    /// Sprint is a toggle instead of hold.
    pub sprint_toggle: bool,
    /// Jump automatically when walking into a block.
    pub auto_jump: bool,
    /// Rebindable keys.
    pub keys: KeyBinds,
    /// Gamepad/controller settings.
    pub gamepad: GamepadSettings,

    // --- Chat ----------------------------------------------------------------
    /// Chat text scale (0.5..=2.0).
    pub chat_scale: f32,
    /// Chat background opacity (0..=1).
    pub chat_opacity: f32,
    /// Chat width in GUI px (vanilla 40..=320).
    pub chat_width: f32,
    /// Chat line spacing multiplier (1.0 = vanilla).
    pub chat_line_spacing: f32,
    /// What chat shows: everything / commands only / nothing.
    pub chat_visibility: ChatVisibility,
    /// Render server chat/formatting colors (off = plain white text).
    pub chat_colors: bool,
    /// Make chat links clickable (off = links are inert plain text).
    pub chat_links: bool,
    /// Show the command auto-complete suggestion box while typing `/…`.
    pub command_suggestions: bool,
    /// Show subtitles ("Zombie groans") for nearby sounds.
    pub subtitles: bool,
    /// Trim the F3 debug overlay to the essentials.
    pub reduced_debug_info: bool,
    /// Item-name language ("de_de" / "en_us"). Applied on restart.
    pub language: String,
    /// Show a Discord Rich Presence with the current server. A raw server IP
    /// (as opposed to a domain) is never shown, for privacy.
    pub discord_rpc: bool,

    // --- Skin customization (own model overlay layers + main hand) -----------
    /// Show the hat (head) overlay layer on your own model.
    pub skin_hat: bool,
    /// Show the jacket (body) overlay layer.
    pub skin_jacket: bool,
    /// Show the right-sleeve overlay layer.
    pub skin_right_sleeve: bool,
    /// Show the left-sleeve overlay layer.
    pub skin_left_sleeve: bool,
    /// Show the right pants-leg overlay layer.
    pub skin_right_pants: bool,
    /// Show the left pants-leg overlay layer.
    pub skin_left_pants: bool,
    /// Left-handed: your held item is drawn in the left hand (vanilla Main Hand).
    pub left_handed: bool,

    // --- Accessibility -------------------------------------------------------
    /// Opacity of floating text backdrops (nametags), 0..=1.
    pub text_background_opacity: f32,

    // --- Sound (all 0..=1) ---------------------------------------------------
    /// Master volume — scales every category, exactly like vanilla.
    pub master_volume: f32,
    pub music_volume: f32,
    pub records_volume: f32,
    pub weather_volume: f32,
    pub blocks_volume: f32,
    pub hostile_volume: f32,
    pub neutral_volume: f32,
    pub players_volume: f32,
    pub ambient_volume: f32,
    pub voice_volume: f32,
}

/// The vanilla sound categories, matching the server's `SoundSource`. Each has
/// its own volume slider; the effective gain is `master * category`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundCategory {
    Master,
    Music,
    Records,
    Weather,
    Blocks,
    Hostile,
    Neutral,
    Players,
    Ambient,
    Voice,
}

/// Controller options. A pad is fully optional and additive over
/// keyboard+mouse — see `app/gamepad.rs` for the button/axis mapping this
/// configures.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GamepadSettings {
    /// Master switch — off ignores any connected pad entirely.
    pub enabled: bool,
    /// Right-stick look speed multiplier (0.1..=3.0, 1.0 = default rate).
    pub look_sensitivity: f32,
    /// Invert the right stick's vertical axis.
    pub invert_y: bool,
    /// Stick deadzone (0..=0.9) — the fraction of travel from center that is
    /// ignored, so a worn or imprecise pad doesn't drift the camera or walk
    /// on its own.
    pub deadzone: f32,
}

impl Default for GamepadSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            look_sensitivity: 1.0,
            invert_y: false,
            deadzone: 0.2,
        }
    }
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            fov: 70.0,
            render_distance: 12,
            max_fps: 0,
            vsync: true,
            gui_scale: 0,
            brightness: 0.5,
            fullscreen: false,
            graphics: Graphics::Fancy,
            fog: true,
            view_bobbing: true,
            particles: ParticleLevel::All,
            fov_effects: 1.0,
            attack_indicator: AttackIndicator::Crosshair,
            damage_tilt: true,
            smooth_lighting: true,
            sensitivity_pct: 100.0,
            invert_mouse: false,
            sneak_toggle: false,
            sprint_toggle: false,
            auto_jump: false,
            keys: KeyBinds::default(),
            gamepad: GamepadSettings::default(),
            chat_scale: 1.0,
            chat_opacity: 0.5,
            chat_width: 320.0,
            chat_line_spacing: 1.0,
            chat_visibility: ChatVisibility::Full,
            chat_colors: true,
            chat_links: true,
            command_suggestions: true,
            subtitles: false,
            reduced_debug_info: false,
            language: "de_de".into(),
            discord_rpc: true,
            skin_hat: true,
            skin_jacket: true,
            skin_right_sleeve: true,
            skin_left_sleeve: true,
            skin_right_pants: true,
            skin_left_pants: true,
            left_handed: false,
            text_background_opacity: 0.4,
            master_volume: 1.0,
            music_volume: 1.0,
            records_volume: 1.0,
            weather_volume: 1.0,
            blocks_volume: 1.0,
            hostile_volume: 1.0,
            neutral_volume: 1.0,
            players_volume: 1.0,
            ambient_volume: 1.0,
            voice_volume: 1.0,
        }
    }
}

impl GameSettings {
    /// The raw per-pixel mouse multiplier used by the camera (matches the old
    /// hard-coded 0.15 at 100%).
    pub fn sensitivity(&self) -> f32 {
        0.15 * (self.sensitivity_pct / 100.0)
    }

    /// Skin overlay-layer visibility as a bit-per-part mask, ordered to match
    /// the renderer's part indices (0 head, 1 body, 2 right arm, 3 left arm,
    /// 4 right leg, 5 left leg). Applied to the local player's own model.
    pub fn skin_layer_mask(&self) -> u8 {
        (self.skin_hat as u8)
            | (self.skin_jacket as u8) << 1
            | (self.skin_right_sleeve as u8) << 2
            | (self.skin_left_sleeve as u8) << 3
            | (self.skin_right_pants as u8) << 4
            | (self.skin_left_pants as u8) << 5
    }

    /// Effective gain for a sound category: `master * category`, clamped 0..=1.
    /// UI/menu sounds use `Master`.
    pub fn category_volume(&self, cat: SoundCategory) -> f32 {
        let c = match cat {
            SoundCategory::Master => 1.0,
            SoundCategory::Music => self.music_volume,
            SoundCategory::Records => self.records_volume,
            SoundCategory::Weather => self.weather_volume,
            SoundCategory::Blocks => self.blocks_volume,
            SoundCategory::Hostile => self.hostile_volume,
            SoundCategory::Neutral => self.neutral_volume,
            SoundCategory::Players => self.players_volume,
            SoundCategory::Ambient => self.ambient_volume,
            SoundCategory::Voice => self.voice_volume,
        };
        (self.master_volume * c).clamp(0.0, 1.0)
    }

    /// egui points-per-pixel for the configured GUI scale, given the OS scale.
    pub fn pixels_per_point(&self, os_scale: f32) -> f32 {
        if self.gui_scale == 0 {
            os_scale.max(1.0)
        } else {
            self.gui_scale as f32
        }
    }

    /// Config dir (`…/DolphinClient`), per-OS.
    pub fn config_dir() -> PathBuf {
        let base = if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        };
        base.unwrap_or_else(|| PathBuf::from(".")).join("DolphinClient")
    }

    /// Config file path (`…/DolphinClient/options.json`), per-OS.
    pub fn path() -> PathBuf {
        Self::config_dir().join("options.json")
    }

    /// Load saved settings, or defaults seeded with `render_distance` when no
    /// file exists yet (so a CLI `--render-distance` still applies first run).
    pub fn load_or_seed(render_distance: i32) -> Self {
        match std::fs::read_to_string(Self::path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self {
                render_distance,
                ..Self::default()
            },
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }

    pub fn clamp(&mut self) {
        self.fov = self.fov.clamp(30.0, 110.0);
        self.render_distance = self.render_distance.clamp(2, 32);
        self.max_fps = self.max_fps.min(360);
        self.gui_scale = self.gui_scale.min(4);
        self.brightness = self.brightness.clamp(0.0, 1.0);
        self.sensitivity_pct = self.sensitivity_pct.clamp(0.0, 200.0);
        self.chat_scale = self.chat_scale.clamp(0.5, 2.0);
        self.chat_opacity = self.chat_opacity.clamp(0.0, 1.0);
        self.chat_width = self.chat_width.clamp(40.0, 320.0);
        self.chat_line_spacing = self.chat_line_spacing.clamp(1.0, 2.0);
        self.fov_effects = self.fov_effects.clamp(0.0, 1.0);
        for v in [
            &mut self.master_volume,
            &mut self.music_volume,
            &mut self.records_volume,
            &mut self.weather_volume,
            &mut self.blocks_volume,
            &mut self.hostile_volume,
            &mut self.neutral_volume,
            &mut self.players_volume,
            &mut self.ambient_volume,
            &mut self.voice_volume,
        ] {
            *v = v.clamp(0.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitivity_maps_100pct_to_default() {
        let s = GameSettings::default();
        assert!((s.sensitivity() - 0.15).abs() < 1e-6);
    }

    #[test]
    fn gui_scale_auto_follows_os() {
        let mut s = GameSettings::default();
        s.gui_scale = 0;
        assert_eq!(s.pixels_per_point(2.0), 2.0);
        s.gui_scale = 3;
        assert_eq!(s.pixels_per_point(2.0), 3.0);
    }

    #[test]
    fn clamp_bounds_everything() {
        let mut s = GameSettings::default();
        s.fov = 999.0;
        s.render_distance = 999;
        s.sensitivity_pct = -5.0;
        s.chat_width = 9999.0;
        s.clamp();
        assert_eq!(s.fov, 110.0);
        assert_eq!(s.render_distance, 32);
        assert_eq!(s.sensitivity_pct, 0.0);
        assert_eq!(s.chat_width, 320.0);
    }

    #[test]
    fn key_ids_round_trip_labels() {
        assert_eq!(key_id(KeyCode::KeyW), "KeyW");
        assert_eq!(key_label("KeyW"), "W");
        assert_eq!(key_label("ShiftLeft"), "Left Shift");
        assert_eq!(key_label("Digit3"), "3");
        assert!(KeyBinds::matches("Space", KeyCode::Space));
        assert!(!KeyBinds::matches("Space", KeyCode::KeyW));
    }

    #[test]
    fn server_pack_policy_cycles_like_vanilla() {
        let policy = ServerResourcePackPolicy::Prompt;
        assert_eq!(policy.next(), ServerResourcePackPolicy::Enabled);
        assert_eq!(policy.next().next(), ServerResourcePackPolicy::Disabled);
        assert_eq!(policy.next().next().next(), ServerResourcePackPolicy::Prompt);
    }

    #[test]
    fn old_options_json_still_parses() {
        // A pre-keybind config (missing new fields) must load with defaults.
        let old = r#"{ "fov": 90.0, "render_distance": 8 }"#;
        let s: GameSettings = serde_json::from_str(old).expect("parse old config");
        assert_eq!(s.fov, 90.0);
        assert_eq!(s.keys.forward, "KeyW");
        assert_eq!(s.chat_width, 320.0);
    }
}
