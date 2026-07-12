//! "Start with the system" (autostart) support — dependency-free.
//!
//! - **Windows:** add/remove a per-user `Run` value via `reg.exe`, spawned with
//!   `CREATE_NO_WINDOW` so no console flashes. Stored value is the launcher path.
//! - **Linux:** write/delete a freedesktop `~/.config/autostart/*.desktop` entry.
//! - **macOS:** write/delete a per-user LaunchAgent plist.
//!
//! All operations are best-effort — autostart is a convenience, never required
//! for the launcher to run, so callers can ignore the result.

const APP_NAME: &str = "DolphinClient";

/// Enable or disable launching DolphinClient when the user signs in.
pub fn set(enabled: bool) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let exe = exe.as_path();
    #[cfg(target_os = "windows")]
    {
        set_windows(enabled, exe)
    }
    #[cfg(target_os = "linux")]
    {
        set_linux(enabled, exe)
    }
    #[cfg(target_os = "macos")]
    {
        set_macos(enabled, exe)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = (enabled, exe);
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn set_windows(enabled: bool, exe: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut cmd = Command::new("reg");
    if enabled {
        // Store a quoted path so profiles containing spaces
        // (C:\Users\First Last\…) launch unambiguously at logon.
        cmd.args(["add", RUN_KEY, "/v", APP_NAME, "/t", "REG_SZ", "/d"])
            .arg(format!("\"{}\"", exe.display()))
            .arg("/f");
    } else {
        cmd.args(["delete", RUN_KEY, "/v", APP_NAME, "/f"]);
    }
    cmd.creation_flags(CREATE_NO_WINDOW);
    // `reg delete` exits non-zero when the value was already absent — for us the
    // desired end state ("not present") is reached either way, so status is not
    // treated as an error.
    let _ = cmd.status()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_linux(enabled: bool, exe: &std::path::Path) -> std::io::Result<()> {
    use std::path::PathBuf;
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autostart");
    let file = dir.join("dolphinclient.desktop");
    if enabled {
        std::fs::create_dir_all(&dir)?;
        let content = format!(
            "[Desktop Entry]\nType=Application\nName={APP_NAME}\nExec={}\nX-GNOME-Autostart-enabled=true\nTerminal=false\n",
            exe.display()
        );
        std::fs::write(file, content)?;
    } else if file.exists() {
        std::fs::remove_file(file)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn set_macos(enabled: bool, exe: &std::path::Path) -> std::io::Result<()> {
    use std::path::PathBuf;
    let dir = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/LaunchAgents"))
        .unwrap_or_else(|| PathBuf::from("."));
    let file = dir.join("de.dolphinclient.launcher.plist");
    if enabled {
        std::fs::create_dir_all(&dir)?;
        let content = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>de.dolphinclient.launcher</string><key>ProgramArguments</key><array><string>{}</string></array><key>RunAtLoad</key><true/></dict></plist>\n",
            exe.display()
        );
        std::fs::write(file, content)?;
    } else if file.exists() {
        std::fs::remove_file(file)?;
    }
    Ok(())
}
