//! Gamepad/controller input via `gilrs`.
//!
//! Vanilla added native controller support to Java Edition on top of the
//! existing keyboard/mouse scheme rather than replacing it, so this module
//! does the same: it is a second, purely additive input source. With no
//! gamepad plugged in — the overwhelmingly common case, and always true on a
//! headless build box — every axis reads `0.0` and every button reads
//! "not pressed", so keyboard-and-mouse play is bit-for-bit unaffected.
//!
//! Two things are exposed to the rest of the app:
//! - continuous axes (`move_x/move_y/look_x/look_y`), read every frame and
//!   OR'd into the existing WASD/mouse-delta computations;
//! - edge-tracked buttons (`just_pressed`/`just_released`), read every frame
//!   to drive the same calls a real key press or mouse click would.
//!
//! Deliberately out of scope for now: force feedback/rumble, remapping UI
//! beyond the deadzone/sensitivity sliders, and more than one active pad
//! (vanilla itself is single-player-input-focused here too).

use gilrs::{Axis, Button, Gilrs};
use std::collections::HashSet;

pub struct GamepadInput {
    gilrs: Option<Gilrs>,
    move_x: f32,
    move_y: f32,
    look_x: f32,
    look_y: f32,
    down: HashSet<Button>,
    prev_down: HashSet<Button>,
}

/// Buttons we actually read. Polled every frame rather than event-driven so a
/// button already held when the game gains focus is picked up immediately.
const TRACKED_BUTTONS: &[Button] = &[
    Button::South,
    Button::East,
    Button::North,
    Button::West,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger2,
    Button::Select,
    Button::Start,
    Button::LeftThumb,
    Button::RightThumb,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
];

impl GamepadInput {
    pub fn new() -> Self {
        // `Gilrs::new` only errors if the platform backend itself can't be
        // opened (e.g. no udev). Controller support then simply stays off —
        // exactly like it does when no pad is plugged in.
        let gilrs = Gilrs::new().ok();
        Self {
            gilrs,
            move_x: 0.0,
            move_y: 0.0,
            look_x: 0.0,
            look_y: 0.0,
            down: HashSet::new(),
            prev_down: HashSet::new(),
        }
    }

    pub fn connected(&self) -> bool {
        self.gilrs
            .as_ref()
            .is_some_and(|g| g.gamepads().next().is_some())
    }

    /// Drain queued events (so hot-plug/disconnect are noticed) and snapshot
    /// the first active pad's axes/buttons. Call exactly once per frame.
    pub fn poll(&mut self, deadzone: f32) {
        self.prev_down = std::mem::take(&mut self.down);
        let Some(gilrs) = &mut self.gilrs else { return };
        while gilrs.next_event().is_some() {}

        let Some((_, pad)) = gilrs.gamepads().find(|(_, p)| p.is_connected()) else {
            self.move_x = 0.0;
            self.move_y = 0.0;
            self.look_x = 0.0;
            self.look_y = 0.0;
            return;
        };
        let dz = |v: f32| if v.abs() < deadzone { 0.0 } else { v };
        self.move_x = dz(pad.value(Axis::LeftStickX));
        self.move_y = dz(pad.value(Axis::LeftStickY));
        self.look_x = dz(pad.value(Axis::RightStickX));
        self.look_y = dz(pad.value(Axis::RightStickY));
        for &b in TRACKED_BUTTONS {
            if pad.is_pressed(b) {
                self.down.insert(b);
            }
        }
    }

    pub fn just_pressed(&self, b: Button) -> bool {
        self.down.contains(&b) && !self.prev_down.contains(&b)
    }

    pub fn just_released(&self, b: Button) -> bool {
        !self.down.contains(&b) && self.prev_down.contains(&b)
    }

    pub fn held(&self, b: Button) -> bool {
        self.down.contains(&b)
    }

    // -- movement: left stick, treated as four directional "keys" ----------
    // Minecraft's own move command is directional (-1/0/1 per axis), not
    // speed-analog, so a deadzone-thresholded stick matches vanilla exactly
    // rather than approximating it.
    pub fn forward(&self) -> bool {
        self.move_y > 0.5
    }
    pub fn back(&self) -> bool {
        self.move_y < -0.5
    }
    pub fn left(&self) -> bool {
        self.move_x < -0.5
    }
    pub fn right(&self) -> bool {
        self.move_x > 0.5
    }
    /// Left trigger doubles as the sprint modifier (vanilla: click left stick).
    pub fn sprint_held(&self) -> bool {
        self.held(Button::LeftThumb)
    }
    pub fn sneak_held(&self) -> bool {
        self.held(Button::East)
    }

    // -- look: right stick, consumed as synthetic mouse-motion delta --------
    pub fn look_delta(&self) -> (f32, f32) {
        (self.look_x, self.look_y)
    }
}

impl Default for GamepadInput {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The overwhelmingly common case — a build box or a desktop with no pad
    /// plugged in — must never panic, and must report a fully neutral state.
    #[test]
    fn new_and_poll_are_safe_with_no_hardware() {
        let mut pad = GamepadInput::new();
        pad.poll(0.2);
        assert!(!pad.connected());
        assert!(!pad.forward() && !pad.back() && !pad.left() && !pad.right());
        assert!(!pad.sprint_held() && !pad.sneak_held());
        assert_eq!(pad.look_delta(), (0.0, 0.0));
        assert!(!pad.just_pressed(Button::South));
        assert!(!pad.held(Button::South));
    }
}
