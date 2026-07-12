//! In-launcher **game** quick-settings.
//!
//! The native client persists its vanilla-style options to `options.json` and
//! reads them on start (`GameSettings::load_or_seed`). This module lets the
//! launcher pre-write a small, safe subset of those options so a player can set
//! render distance, FPS cap, FoV, etc. without opening the in-game menu — the
//! change takes effect the next time the client launches.
//!
//! We patch the file as a raw `serde_json::Value` object: keys the launcher
//! doesn't understand are preserved untouched, and the client fills in anything
//! missing from its own defaults. Editing is only offered while the game is
//! **not** running (the client rewrites the whole file from memory on exit).
//!
//! The launcher currently only pre-writes `fullscreen`; the other typed
//! accessors are kept as a ready toolbox for future quick-settings.
#![allow(dead_code)]

use serde_json::Value;

use crate::config;

/// Graphics preset, matching the client's `settings::Graphics` JSON encoding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    fn from_json(v: Option<&Value>) -> Self {
        match v.and_then(|v| v.as_str()) {
            Some("Fast") => Graphics::Fast,
            _ => Graphics::Fancy,
        }
    }
}

/// A live view over the client's `options.json`, editable field-by-field.
pub struct GameOpts {
    root: Value,
}

impl GameOpts {
    /// Load the client's options, or an empty object when there is no file yet.
    pub fn load() -> Self {
        let root = std::fs::read_to_string(config::client_options_path())
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .filter(|v| v.is_object())
            .unwrap_or_else(|| Value::Object(Default::default()));
        Self { root }
    }

    /// Persist the patched options back to the client's `options.json`.
    pub fn save(&self) {
        let path = config::client_options_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(&self.root) {
            let _ = std::fs::write(path, s);
        }
    }

    fn set(&mut self, key: &str, value: Value) {
        if let Value::Object(map) = &mut self.root {
            map.insert(key.to_string(), value);
        }
    }
    fn f64(&self, key: &str, default: f64) -> f64 {
        self.root.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
    }
    fn i64(&self, key: &str, default: i64) -> i64 {
        self.root.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
    }
    fn u64(&self, key: &str, default: u64) -> u64 {
        self.root.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
    }
    fn bool(&self, key: &str, default: bool) -> bool {
        self.root.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }

    // ---- Typed accessors (defaults mirror client `GameSettings::default`) ----

    pub fn render_distance(&self) -> i32 {
        self.i64("render_distance", 12).clamp(2, 32) as i32
    }
    pub fn set_render_distance(&mut self, v: i32) {
        self.set("render_distance", Value::from(v.clamp(2, 32)));
    }

    pub fn max_fps(&self) -> u32 {
        self.u64("max_fps", 0).min(360) as u32
    }
    pub fn set_max_fps(&mut self, v: u32) {
        self.set("max_fps", Value::from(v.min(360)));
    }

    pub fn vsync(&self) -> bool {
        self.bool("vsync", true)
    }
    pub fn set_vsync(&mut self, v: bool) {
        self.set("vsync", Value::from(v));
    }

    pub fn fov(&self) -> f32 {
        self.f64("fov", 70.0).clamp(30.0, 110.0) as f32
    }
    pub fn set_fov(&mut self, v: f32) {
        self.set("fov", Value::from(v.clamp(30.0, 110.0)));
    }

    pub fn gui_scale(&self) -> u32 {
        self.u64("gui_scale", 0).min(4) as u32
    }
    pub fn set_gui_scale(&mut self, v: u32) {
        self.set("gui_scale", Value::from(v.min(4)));
    }

    pub fn brightness(&self) -> f32 {
        self.f64("brightness", 0.5).clamp(0.0, 1.0) as f32
    }
    pub fn set_brightness(&mut self, v: f32) {
        self.set("brightness", Value::from(v.clamp(0.0, 1.0)));
    }

    pub fn set_fullscreen(&mut self, v: bool) {
        self.set("fullscreen", Value::from(v));
    }

    pub fn graphics(&self) -> Graphics {
        Graphics::from_json(self.root.get("graphics"))
    }
    pub fn set_graphics(&mut self, g: Graphics) {
        self.set("graphics", Value::from(g.label()));
    }

    pub fn discord_rpc(&self) -> bool {
        self.bool("discord_rpc", true)
    }
    pub fn set_discord_rpc(&mut self, v: bool) {
        self.set("discord_rpc", Value::from(v));
    }
}
