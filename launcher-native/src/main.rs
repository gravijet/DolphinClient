// Hide the console window on Windows release builds (keep it in debug for logs).
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod accounts;
mod app;
mod auth;
mod bridge;
mod client;
mod config;
mod events;
mod game;
mod gameopts;
mod mcui;
mod tokens;
mod ui;
mod updater;

/// Decode the embedded logo PNG into the window icon.
fn window_icon() -> Option<eframe::egui::IconData> {
    let img = image::load_from_memory(mcui::LOGO_PNG).ok()?.to_rgba8();
    Some(eframe::egui::IconData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

fn main() -> eframe::Result<()> {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1120.0, 720.0])
        .with_min_inner_size([940.0, 620.0])
        // Frameless: the app draws its own title bar with minimize/maximize/close.
        .with_decorations(false)
        .with_transparent(false)
        .with_title("DolphinClient");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "DolphinClient",
        options,
        Box::new(|cc| Ok(Box::new(app::DolphinApp::new(cc)))),
    )
}
