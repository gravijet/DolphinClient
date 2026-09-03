// Hide the console window on Windows release builds (keep it in debug for logs).
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod accounts;
mod app;
mod auth;
mod autostart;
mod client;
mod config;
mod cosmetics;
mod desktopicon;
mod discord;
mod events;
mod fonts;
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
    // Linux only: install the .desktop entry + hicolor icon set so the app
    // menu/taskbar show DolphinClient's own icon instead of a generic one.
    // No-op on Windows/macOS, where build.rs already embeds the icon.
    desktopicon::ensure_installed();

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([980.0, 640.0])
        // Frameless: the app draws its own title bar with minimize/maximize/close.
        .with_decorations(false)
        .with_transparent(false)
        .with_title("DolphinClient")
        // Matches desktopicon::APP_ID / the .desktop file's StartupWMClass, so
        // Linux window managers can tell this window belongs to that entry.
        .with_app_id(desktopicon::APP_ID);
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
