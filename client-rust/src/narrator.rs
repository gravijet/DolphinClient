//! Narrator: reads UI text aloud through the operating system's own
//! text-to-speech, the same way vanilla's narrator leans on the platform's
//! accessibility stack rather than shipping a voice of its own.
//!
//! No bundled voice, no heavy speech-synthesis dependency: each utterance is
//! handed to whatever the OS already provides — `say` on macOS, `spd-say`
//! (falling back to `espeak-ng`/`espeak`) on Linux, a one-line PowerShell
//! `System.Speech` call on Windows — spawned on a dedicated worker thread so
//! a slow or missing backend can never stall a frame. With none of that
//! installed, the narrator is silently a no-op, same as a bare Linux box
//! with no speech-dispatcher — never a crash, never a stall.

use std::sync::mpsc::{Receiver, Sender};
use tracing::warn;

/// What's being spoken — matches `settings::NarratorMode`'s own split so a
/// mode can filter by category without depending on the settings module.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// Menu/screen titles and UI feedback.
    System,
    /// Incoming chat messages.
    Chat,
    /// Sound subtitles ("Zombie groans") — vanilla only reads these under
    /// `All`, not under the narrower `System`/`Chat` modes.
    Sound,
}

/// Handle to the narrator's worker thread. The app holds exactly one.
pub struct Narrator {
    tx: Option<Sender<String>>,
}

impl Narrator {
    pub fn spawn() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let ok = std::thread::Builder::new()
            .name("narrator-tts".into())
            .spawn(move || worker(rx))
            .is_ok();
        if !ok {
            warn!("narrator: worker thread failed to start — narrator disabled");
        }
        Self { tx: ok.then_some(tx) }
    }

    /// Queue an utterance, if `mode` cares about `category` at all. Never
    /// blocks; if the worker is gone or no OS backend is available the
    /// request is simply dropped.
    pub fn speak(&self, mode: crate::settings::NarratorMode, category: Category, text: &str) {
        let text = text.trim();
        if text.is_empty() || !mode.wants(category) {
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(text.to_string());
        }
    }
}

fn worker(rx: Receiver<String>) {
    // One utterance speaks at a time: a queued backlog would have the
    // narrator babbling over itself, so a new one interrupts whatever's
    // still speaking rather than stacking behind it — matching vanilla,
    // where a fresh focus change cuts the previous announcement short.
    let mut current: Option<std::process::Child> = None;
    while let Ok(mut text) = rx.recv() {
        while let Ok(next) = rx.try_recv() {
            text = next;
        }
        if let Some(mut child) = current.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        current = speak_now(&text);
    }
    if let Some(mut child) = current.take() {
        let _ = child.kill();
    }
}

#[cfg(target_os = "macos")]
fn speak_now(text: &str) -> Option<std::process::Child> {
    std::process::Command::new("say").arg(text).spawn().ok()
}

#[cfg(target_os = "linux")]
fn speak_now(text: &str) -> Option<std::process::Child> {
    // speech-dispatcher respects whatever voice/rate the desktop has
    // configured; espeak-ng/espeak are the common fallback when it (or its
    // daemon) isn't installed.
    if let Ok(child) = std::process::Command::new("spd-say").arg("--").arg(text).spawn() {
        return Some(child);
    }
    for bin in ["espeak-ng", "espeak"] {
        if let Ok(child) = std::process::Command::new(bin).arg(text).spawn() {
            return Some(child);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn speak_now(text: &str) -> Option<std::process::Child> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Single-quoted PowerShell string literal: escape `'` by doubling it.
    let escaped = text.replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Speech; \
         (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak('{escaped}')"
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn speak_now(_text: &str) -> Option<std::process::Child> {
    None
}
