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

/// Vanilla chat visibility modes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ChatVisibility {
    Full,
    /// Only command feedback / system messages.
    System,
    Hidden,
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
        }
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
    /// Show subtitles ("Zombie groans") for nearby sounds.
    pub subtitles: bool,
    /// Item-name language ("de_de" / "en_us"). Applied on restart.
    pub language: String,

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
            sensitivity_pct: 100.0,
            invert_mouse: false,
            sneak_toggle: false,
            sprint_toggle: false,
            auto_jump: false,
            keys: KeyBinds::default(),
            chat_scale: 1.0,
            chat_opacity: 0.5,
            chat_width: 320.0,
            chat_line_spacing: 1.0,
            chat_visibility: ChatVisibility::Full,
            subtitles: false,
            language: "de_de".into(),
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
    fn old_options_json_still_parses() {
        // A pre-keybind config (missing new fields) must load with defaults.
        let old = r#"{ "fov": 90.0, "render_distance": 8 }"#;
        let s: GameSettings = serde_json::from_str(old).expect("parse old config");
        assert_eq!(s.fov, 90.0);
        assert_eq!(s.keys.forward, "KeyW");
        assert_eq!(s.chat_width, 320.0);
    }
}
