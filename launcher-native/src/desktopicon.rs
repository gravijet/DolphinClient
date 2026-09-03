//! Linux application-menu integration.
//!
//! On Windows and macOS the icon is embedded straight into the executable
//! (see `build.rs`) and the OS shows it everywhere on its own. Linux desktop
//! environments work differently: the taskbar/dock/app-grid icon comes from a
//! `.desktop` file's `Icon=` key, matched to the running window via
//! `StartupWMClass` — not from any pixel buffer the app hands the window
//! system. Without one, DolphinClient shows a generic fallback icon even
//! though the binary itself is fine.
//!
//! This installs (or refreshes) a standard XDG `.desktop` entry plus the
//! icon in every size vanilla apps ship, under the user's own
//! `~/.local/share` — no root needed, nothing outside the user's profile.
//! Best-effort and idempotent: called once at startup, failures are ignored,
//! since a missing menu icon is cosmetic and must never block the app.

/// Must match the `with_app_id` passed to the eframe viewport in `main.rs`
/// (and ideally the client's window `with_name`/`with_class`), so the window
/// manager can associate a running window with this `.desktop` entry.
pub const APP_ID: &str = "de.dolphinclient.launcher";

pub fn ensure_installed() {
    #[cfg(target_os = "linux")]
    {
        let _ = install();
    }
}

#[cfg(target_os = "linux")]
fn install() -> std::io::Result<()> {
    use std::path::PathBuf;

    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| std::io::Error::other("neither XDG_DATA_HOME nor HOME is set"))?;

    // One embedded PNG per hicolor size — same artwork the Windows .ico and
    // macOS .icns already carry, just split by size for the icon theme.
    const ICONS: &[(u32, &[u8])] = &[
        (16, include_bytes!("../../assets/brand/dolphin-client-16.png")),
        (32, include_bytes!("../../assets/brand/dolphin-client-32.png")),
        (48, include_bytes!("../../assets/brand/dolphin-client-48.png")),
        (64, include_bytes!("../../assets/brand/dolphin-client-64.png")),
        (128, include_bytes!("../../assets/brand/dolphin-client-128.png")),
        (256, include_bytes!("../../assets/brand/dolphin-client-256.png")),
        (512, include_bytes!("../../assets/brand/dolphin-512.png")),
    ];
    for (size, bytes) in ICONS {
        let dir = data_home.join(format!("icons/hicolor/{size}x{size}/apps"));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(format!("{}.png", APP_ID)), bytes)?;
    }

    let apps_dir = data_home.join("applications");
    std::fs::create_dir_all(&apps_dir)?;
    let exe = std::env::current_exe()?;
    // Quote the path: XDG desktop entries use the same word-splitting rules
    // as a shell, and install locations containing spaces are common enough
    // (e.g. "~/Downloads/DolphinClient (1)/dolphinclient-launcher").
    let desktop = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=DolphinClient\n\
         Comment=Native Minecraft-Client & Launcher\n\
         Exec=\"{}\"\n\
         Icon={}\n\
         Terminal=false\n\
         Categories=Game;\n\
         StartupNotify=true\n\
         StartupWMClass={}\n",
        exe.display(),
        APP_ID,
        APP_ID,
    );
    let dest = apps_dir.join(format!("{}.desktop", APP_ID));
    // Skip the write if the content is already identical — avoids bumping the
    // file's mtime (and re-triggering menu-cache rebuilds) on every launch.
    if std::fs::read_to_string(&dest).ok().as_deref() != Some(desktop.as_str()) {
        std::fs::write(&dest, desktop)?;
    }

    // Best-effort cache refresh so the new/updated entry shows up without a
    // re-login — neither tool is guaranteed to be installed.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .status();
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .args(["-f", "-t"])
        .arg(data_home.join("icons/hicolor"))
        .status();
    Ok(())
}
