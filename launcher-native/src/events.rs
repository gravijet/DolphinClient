//! Messages worker threads send back to the UI thread.

use crate::auth::Session;

#[derive(Debug)]
pub enum Event {
    /// One-line status update.
    Status(String),
    /// Append a line to the detailed log.
    Log(String),
    /// Overall progress, 0.0 ..= 1.0.
    Progress(f32),
    /// Show the Microsoft device-code prompt (link code).
    Device {
        /// Verification URL with the code pre-filled (one click → sign in).
        complete: String,
        code: String,
        message: String,
    },
    /// A browser window was opened for sign-in (auth-code flow); keep the URL
    /// so the UI can offer to re-open it.
    BrowserOpen { url: String },
    /// Login succeeded.
    LoggedIn(Session),
    /// The game process was spawned.
    Launched,
    /// Something went wrong.
    Error(String),
    /// Worker finished (success or failure) — clear the busy flag.
    Done,
}
