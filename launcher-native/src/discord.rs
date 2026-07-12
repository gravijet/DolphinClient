//! Discord Rich Presence for the **launcher**, over the local Discord IPC
//! socket.
//!
//! While the DolphinClient launcher is open, it broadcasts a "in the launcher"
//! presence to Discord (your name, the app, an elapsed timer). The moment the
//! game itself starts, the launcher clears its presence so the in-game client's
//! richer "playing on <server>" presence takes over — the two never fight.
//!
//! No external crate: the IPC wire format is tiny (a little-endian `u32` opcode
//! + `u32` length + a JSON body over a named pipe / unix socket). All blocking
//! I/O lives on a dedicated worker thread; the launcher only ever hands it the
//! desired [`Activity`] (or `None` to clear). If Discord isn't running the
//! worker retries quietly and never blocks or affects the launcher.
//!
//! This mirrors the client's `discord` module and shares the same registered
//! **Discord Application id**, so both show up as "DolphinClient".

use std::io::Write;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

/// The DolphinClient Discord Application id (same app as the in-game client).
/// Overridable at runtime with `DOLPHIN_DISCORD_APP_ID`. Empty ⇒ disabled.
const APP_ID: &str = "1525101310964269196";

/// Large image shown in the presence card. Discord accepts a full external URL
/// here (proxied by its media server), so this needs no uploaded art assets.
const LARGE_IMAGE: &str = "https://dolphinclient.de/logo.png";

/// One Rich Presence activity. Comparing two of these tells the worker whether a
/// re-send to Discord is actually needed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    /// First line of the presence.
    pub details: Option<String>,
    /// Second line of the presence.
    pub state: Option<String>,
    /// Large image key or URL.
    pub large_image: Option<String>,
    /// Tooltip when hovering the large image.
    pub large_text: Option<String>,
    /// "Elapsed" timer start (unix seconds).
    pub start_unix: Option<u64>,
}

/// Handle to the presence worker thread. The launcher holds exactly one.
pub struct Discord {
    tx: Sender<Option<Activity>>,
}

impl Discord {
    /// Spawn the presence worker, unless no Application id is configured (in
    /// which case this returns `None` and the feature stays dormant).
    pub fn spawn() -> Option<Self> {
        let app_id = std::env::var("DOLPHIN_DISCORD_APP_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| APP_ID.to_string());
        if app_id.trim().is_empty() {
            return None;
        }
        let (tx, rx) = std::sync::mpsc::channel::<Option<Activity>>();
        std::thread::Builder::new()
            .name("discord-rpc".into())
            .spawn(move || worker(app_id, rx))
            .ok()?;
        Some(Self { tx })
    }

    /// Request a presence update (or `None` to clear it). Never blocks; if the
    /// worker is gone the request is dropped silently.
    pub fn set(&self, activity: Option<Activity>) {
        let _ = self.tx.send(activity);
    }
}

impl Drop for Discord {
    fn drop(&mut self) {
        // Dropping the sender ends the worker's recv loop, which clears the
        // presence on its way out.
        let _ = &self.tx;
    }
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

fn worker(app_id: String, rx: Receiver<Option<Activity>>) {
    let mut conn: Option<Conn> = None;
    let mut desired: Option<Activity> = None;
    let mut applied: Option<Activity> = None;
    let mut next_connect = Instant::now();
    let mut nonce: u64 = 0;

    loop {
        // Block for the next command, but wake periodically to retry the
        // connection when Discord isn't up yet.
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(next) => {
                desired = next;
                // Coalesce a burst of updates to the most recent one.
                while let Ok(next) = rx.try_recv() {
                    desired = next;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            // Launcher dropped the handle: clear the presence and exit.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(c) = &mut conn {
                    nonce += 1;
                    let _ = c.clear_activity(nonce);
                }
                return;
            }
        }

        // (Re)connect if needed, throttled so a missing Discord doesn't spin.
        if conn.is_none() && Instant::now() >= next_connect {
            match Conn::connect(&app_id) {
                Ok(c) => {
                    conn = Some(c);
                    applied = None; // force a resend of the current activity
                }
                Err(_) => {
                    next_connect = Instant::now() + Duration::from_secs(15);
                }
            }
        }

        // Push the activity if it changed (or we just reconnected).
        if let Some(c) = &mut conn {
            if applied != desired {
                nonce += 1;
                let res = match &desired {
                    Some(a) => c.set_activity(a, nonce),
                    None => c.clear_activity(nonce),
                };
                match res {
                    Ok(()) => applied = desired.clone(),
                    Err(_) => {
                        // Discord went away (closed/restarted): drop the pipe and
                        // reconnect shortly, keeping `desired` so it re-applies.
                        conn = None;
                        applied = None;
                        next_connect = Instant::now() + Duration::from_secs(5);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// One IPC connection
// ---------------------------------------------------------------------------

struct Conn {
    pipe: Pipe,
}

impl Conn {
    fn connect(app_id: &str) -> std::io::Result<Self> {
        let pipe = Pipe::open()?;
        let mut conn = Self { pipe };
        // Handshake: opcode 0, `{ v: 1, client_id }`.
        let handshake = serde_json::json!({ "v": 1, "client_id": app_id });
        conn.send(0, &serde_json::to_vec(&handshake)?)?;
        Ok(conn)
    }

    fn set_activity(&mut self, a: &Activity, nonce: u64) -> std::io::Result<()> {
        let mut activity = serde_json::Map::new();
        if let Some(d) = &a.details {
            activity.insert("details".into(), d.clone().into());
        }
        if let Some(s) = &a.state {
            activity.insert("state".into(), s.clone().into());
        }
        let mut assets = serde_json::Map::new();
        if let Some(li) = &a.large_image {
            assets.insert("large_image".into(), li.clone().into());
        }
        if let Some(lt) = &a.large_text {
            assets.insert("large_text".into(), lt.clone().into());
        }
        if !assets.is_empty() {
            activity.insert("assets".into(), assets.into());
        }
        if let Some(start) = a.start_unix {
            activity.insert("timestamps".into(), serde_json::json!({ "start": start }));
        }
        let frame = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "nonce": nonce.to_string(),
            "args": { "pid": std::process::id(), "activity": activity },
        });
        self.send(1, &serde_json::to_vec(&frame)?)
    }

    fn clear_activity(&mut self, nonce: u64) -> std::io::Result<()> {
        // `activity: null` clears the presence.
        let frame = serde_json::json!({
            "cmd": "SET_ACTIVITY",
            "nonce": nonce.to_string(),
            "args": { "pid": std::process::id(), "activity": serde_json::Value::Null },
        });
        self.send(1, &serde_json::to_vec(&frame)?)
    }

    /// Write one IPC frame: LE opcode, LE length, then the JSON body.
    fn send(&mut self, opcode: u32, payload: &[u8]) -> std::io::Result<()> {
        let mut buf = Vec::with_capacity(8 + payload.len());
        buf.extend_from_slice(&opcode.to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(payload);
        self.pipe.write_all(&buf)
    }
}

// ---------------------------------------------------------------------------
// Cross-platform pipe (unix socket / windows named pipe)
// ---------------------------------------------------------------------------

#[cfg(unix)]
struct Pipe(std::os::unix::net::UnixStream);

#[cfg(windows)]
struct Pipe(std::fs::File);

#[cfg(not(any(unix, windows)))]
struct Pipe(std::io::Sink);

impl Pipe {
    #[cfg(unix)]
    fn open() -> std::io::Result<Self> {
        use std::os::unix::net::UnixStream;
        // Discord may live under the plain runtime dir, or namespaced under a
        // snap/flatpak subdir. Try each base × ipc slot 0..9.
        let mut bases: Vec<std::path::PathBuf> = Vec::new();
        for var in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
            if let Ok(dir) = std::env::var(var) {
                if !dir.is_empty() {
                    bases.push(std::path::PathBuf::from(dir));
                }
            }
        }
        bases.push(std::path::PathBuf::from("/tmp"));
        let mut roots = Vec::new();
        for base in &bases {
            roots.push(base.clone());
            roots.push(base.join("app/com.discordapp.Discord"));
            roots.push(base.join("snap.discord"));
        }
        for root in roots {
            for i in 0..10 {
                let path = root.join(format!("discord-ipc-{i}"));
                if let Ok(stream) = UnixStream::connect(&path) {
                    return Ok(Pipe(stream));
                }
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no discord-ipc socket",
        ))
    }

    #[cfg(windows)]
    fn open() -> std::io::Result<Self> {
        use std::fs::OpenOptions;
        for i in 0..10 {
            let path = format!(r"\\.\pipe\discord-ipc-{i}");
            if let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) {
                return Ok(Pipe(file));
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no discord-ipc pipe",
        ))
    }

    #[cfg(not(any(unix, windows)))]
    fn open() -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "unsupported platform",
        ))
    }

    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.0.write_all(buf)?;
        self.0.flush()
    }
}

// ---------------------------------------------------------------------------
// Helpers used by the launcher to build activities.
// ---------------------------------------------------------------------------

/// The image URL/key the launcher should use for the large presence image.
pub fn large_image() -> String {
    LARGE_IMAGE.to_string()
}
