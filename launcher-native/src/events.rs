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
        url: String,
        code: String,
        message: String,
    },
    /// Login succeeded.
    LoggedIn(Session),
    /// The game process was spawned.
    Launched,
    /// Something went wrong.
    Error(String),
    /// Worker finished (success or failure) — clear the busy flag.
    Done,
}
