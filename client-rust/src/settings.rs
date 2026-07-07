//! Persistent, vanilla-style game settings (Options screens write these).
//!
//! Only options that actually change the running client are stored here — no
//! dead toggles. They persist to a per-user `options.json` next to where the
//! vanilla launcher keeps its config, so tweaks survive restarts.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

    // --- Chat ----------------------------------------------------------------
    /// Chat text scale (0.5..=2.0).
    pub chat_scale: f32,
    /// Chat background opacity (0..=1).
    pub chat_opacity: f32,

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
            chat_scale: 1.0,
            chat_opacity: 0.5,
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

    /// Config file path (`…/DolphinClient/options.json`), per-OS.
    pub fn path() -> PathBuf {
        let base = if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        };
        base.unwrap_or_else(|| PathBuf::from("."))
            .join("DolphinClient")
            .join("options.json")
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
        s.clamp();
        assert_eq!(s.fov, 110.0);
        assert_eq!(s.render_distance, 32);
        assert_eq!(s.sensitivity_pct, 0.0);
    }
}
