//! DolphinClient — native Rust Minecraft 26.1 client.
//! azalea (protocol/physics) + wgpu (rendering) + egui (HUD).

// Hide the console window on Windows release builds (like vanilla Minecraft's
// javaw). Debug keeps it so logs are visible during development.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod assets;
mod audio;
mod bridge;
mod models;
mod render;
mod settings;
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

    /// `.minecraft/assets` directory (indexes/ + objects/). Enables sound —
    /// OGGs stream in on demand from here. The launcher passes this.
    #[arg(long)]
    assets_dir: Option<PathBuf>,

    /// Asset-index id (e.g. `30` for 26.1). Required with `--assets-dir`.
    #[arg(long)]
    asset_index: Option<String>,

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

    /// Debug: bake the item-icon atlas and write it to this PNG, then exit.
    #[arg(long)]
    dump_item_icons: Option<PathBuf>,

    /// Debug: render the menus (title/multiplayer/options/pause) to PNGs in this
    /// directory, then exit. No server or window needed.
    #[arg(long)]
    dump_menu: Option<PathBuf>,

    /// Offscreen: draw the egui HUD (crosshair, hotbar icons, chat) into frames.
    #[arg(long)]
    hud_demo: bool,
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

    // Debug: bake the item-icon atlas to a PNG and exit (no window/server).
    if let Some(out) = &cli.dump_item_icons {
        use assets::AssetPack;
        use assets::blockmap::BlockTable;
        use assets::items::ItemIcons;
        use models::BakedModelStore;
        let mut pack = AssetPack::open(&mc_jar)?;
        let table = BlockTable::load_or_embedded(blocks_report.as_deref())?;
        let (store, atlas) = BakedModelStore::bake_all(&mut pack, &table)?;
        let icons = ItemIcons::bake(&mut pack, &table, &store, &atlas);
        icons.image.save(out).with_context(|| format!("writing {}", out.display()))?;
        // A curated, labeled-by-position preview for eyeballing correctness.
        let curated = [
            "stone", "cobblestone", "oak_planks", "oak_log", "grass_block", "dirt",
            "glass", "white_wool", "oak_stairs", "oak_slab", "oak_fence", "cobblestone_wall",
            "crafting_table", "furnace", "chest", "bookshelf", "pumpkin", "hay_block",
            "diamond_block", "gold_block", "redstone_block", "bricks", "sandstone", "tnt",
            "diamond_sword", "iron_pickaxe", "apple", "golden_apple", "bread", "arrow",
            "stick", "coal", "iron_ingot", "diamond", "redstone", "ender_pearl",
            "oak_leaves", "poppy", "dandelion", "torch", "ladder", "water_bucket",
            "bow", "shield", "cake", "cobweb", "sea_lantern", "glowstone",
        ];
        let preview = icons.preview_montage(&curated, 4);
        let ppath = out.with_extension("preview.png");
        preview.save(&ppath).with_context(|| format!("writing {}", ppath.display()))?;
        eprintln!(
            "wrote {} item icons to {} (+ preview {})",
            icons.len(),
            out.display(),
            ppath.display()
        );
        return Ok(());
    }

    let opts = app::AppOptions {
        bridge: bridge::events::BridgeOptions {
            account,
            address: cli.server.clone().unwrap_or_default(),
        },
        mc_jar,
        blocks_report,
        render_distance: cli.render_distance,
        assets_dir: cli.assets_dir,
        asset_index: cli.asset_index,
    };

    if let Some(dir) = cli.dump_menu {
        return app::offscreen::dump_menu(opts, dir);
    }

    if cli.offscreen {
        let server = cli.server.as_deref().unwrap_or("");
        anyhow::ensure!(!server.is_empty(), "--offscreen requires --server");
        app::offscreen::run_offscreen(app::offscreen::OffscreenOptions {
            app: opts,
            out_dir: cli.out,
            frames: cli.frames,
            wait_sections: cli.wait_sections,
            exec: cli.exec,
            hud_demo: cli.hud_demo,
        })
    } else {
        app::run_windowed(opts)
    }
}
