//! DolphinClient — native Rust Minecraft 26.1 client.
//! azalea (protocol/physics) + wgpu (rendering) + egui (HUD).

mod app;
mod assets;
mod bridge;
mod models;
mod render;
mod types;
mod world;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "dolphinclient", version, about = "DolphinClient — native Minecraft 26.1 client")]
struct Cli {
    /// Server address (host or host:port). Omit for the connect screen.
    #[arg(long)]
    server: Option<String>,

    /// Offline-mode username.
    #[arg(long, default_value = "Dolphin")]
    username: String,

    /// Use a Microsoft account (email); azalea caches tokens on first login.
    #[arg(long)]
    msa: Option<String>,

    /// Vanilla 26.1 client jar (assets). Searched upward from CWD when omitted.
    #[arg(long)]
    mc_jar: Option<PathBuf>,

    /// blocks.json data-generator report (plain or .gz).
    #[arg(long)]
    blocks_report: Option<PathBuf>,

    /// Render distance in chunks.
    #[arg(long, default_value_t = 8)]
    render_distance: i32,

    /// Headless smoke-test mode: render PNGs, no window.
    #[arg(long)]
    offscreen: bool,

    /// Offscreen: output directory for PNG frames.
    #[arg(long, default_value = "shots")]
    out: PathBuf,

    /// Offscreen: number of frames to render.
    #[arg(long, default_value_t = 8)]
    frames: u32,

    /// Offscreen: meshed-section threshold before rendering.
    #[arg(long, default_value_t = 49)]
    wait_sections: usize,

    /// Offscreen: chat/server command sent after connecting (repeatable).
    #[arg(long)]
    exec: Vec<String>,
}

/// Read a ready Minecraft session from the environment, as set by the launcher:
/// `DOLPHIN_MC_TOKEN` (access token), `DOLPHIN_MC_UUID`, `DOLPHIN_MC_NAME`.
/// All three must be present and non-empty.
fn launcher_session() -> Option<bridge::events::AccountConfig> {
    let access_token = std::env::var("DOLPHIN_MC_TOKEN").ok().filter(|s| !s.is_empty())?;
    let uuid = std::env::var("DOLPHIN_MC_UUID").ok().filter(|s| !s.is_empty())?;
    let username = std::env::var("DOLPHIN_MC_NAME").ok().filter(|s| !s.is_empty())?;
    Some(bridge::events::AccountConfig::Session { username, uuid, access_token })
}

/// Search CWD upward for `.mc-cache/<name>` (dev convenience).
fn find_cache_file(name: &str) -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(".mc-cache").join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wgpu_core=warn,wgpu_hal=warn,naga=warn".into()),
        )
        .init();

    let cli = Cli::parse();

    let mc_jar = cli
        .mc_jar
        .or_else(|| find_cache_file("client-26.1.jar"))
        .context("no --mc-jar given and no .mc-cache/client-26.1.jar found")?;
    // An external report is optional: the 26.1 report is embedded in the binary.
    // Prefer an explicit flag, then a dev-cache copy, else fall back to embedded.
    let blocks_report = cli
        .blocks_report
        .or_else(|| find_cache_file("server/generated/reports/blocks.json"))
        .or_else(|| find_cache_file("blocks.json.gz"));

    // Account selection, in priority order:
    // 1. A launcher-provided session (DOLPHIN_MC_TOKEN/UUID/NAME) — the normal
    //    path when started by the DolphinClient launcher; joins online servers.
    // 2. --msa <email> — the client runs azalea's own cached Microsoft login.
    // 3. offline --username — dev/test servers.
    let account = match launcher_session() {
        Some(session) => session,
        None => match &cli.msa {
            Some(email) => bridge::events::AccountConfig::Microsoft(email.clone()),
            None => bridge::events::AccountConfig::Offline(cli.username.clone()),
        },
    };

    let opts = app::AppOptions {
        bridge: bridge::events::BridgeOptions {
            account,
            address: cli.server.clone().unwrap_or_default(),
        },
        mc_jar,
        blocks_report,
        render_distance: cli.render_distance,
    };

    if cli.offscreen {
        let server = cli.server.as_deref().unwrap_or("");
        anyhow::ensure!(!server.is_empty(), "--offscreen requires --server");
        app::offscreen::run_offscreen(app::offscreen::OffscreenOptions {
            app: opts,
            out_dir: cli.out,
            frames: cli.frames,
            wait_sections: cli.wait_sections,
            exec: cli.exec,
        })
    } else {
        app::run_windowed(opts)
    }
}
