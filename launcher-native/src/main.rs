// Hide the console window on Windows release builds (keep it in debug for logs).
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod accounts;
mod app;
mod auth;
mod client;
mod config;
mod events;
mod game;
mod tokens;
mod updater;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([980.0, 680.0])
            .with_min_inner_size([860.0, 600.0])
            .with_title("DolphinClient"),
        ..Default::default()
    };

    eframe::run_native(
        "DolphinClient",
        options,
        Box::new(|cc| Ok(Box::new(app::DolphinApp::new(cc)))),
    )
}
