//! Brand assets for the launcher.
//!
//! The modern launcher UI is drawn with egui's own widgets (see
//! [`crate::ui`]); the only bundled asset it needs is the DolphinClient logo,
//! embedded as a PNG and used both for the window icon and the in-app logo.

use eframe::egui::{self, TextureHandle, TextureOptions};

/// The DolphinClient logo, embedded (PNG, transparent circle).
pub const LOGO_PNG: &[u8] = include_bytes!("../../assets/brand/dolphin-256.png");

/// Decode the embedded logo into an egui texture.
pub fn load_logo(ctx: &egui::Context) -> TextureHandle {
    let img = image::load_from_memory(LOGO_PNG)
        .expect("embedded logo PNG is valid")
        .to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    ctx.load_texture("dolphin-logo", color, TextureOptions::LINEAR)
}
