//! Footsteps, landings and splashes — the sounds a body makes moving through
//! the world.
//!
//! The server never sends these: in vanilla every client works them out for
//! itself, from how far a body has travelled and whether it is on the ground,
//! in water, or falling. That is what this does, for the player and for every
//! entity in sight, so a world with other people in it finally has floors that
//! answer back.
//!
//! Vanilla's rule (`Entity.move`): each move adds `horizontalDistance × 0.6` to
//! a counter, and a step plays whenever that counter crosses a whole number —
//! one step per **1 / 0.6 ≈ 1.67 blocks** walked. Sprinting covers that ground
//! faster, so the steps come faster by themselves; nothing extra is needed.

/// What a body just did that makes a noise.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepEvent {
    /// A foot hit the ground — play the block's own step sound.
    Step,
    /// Landed after a fall of `blocks`; vanilla splits small from big at four.
    Land { big: bool },
    /// Broke the surface of water, in either direction.
    Splash,
    /// A swimming stroke.
    Swim,
}

/// Vanilla's counter multiplier: distance travelled counts 0.6 towards a step.
const MOVE_FACTOR: f32 = 0.6;
/// A fall of four blocks or more lands "big" (`entity.*.big_fall`).
const BIG_FALL: f32 = 4.0;

/// Per-body state for the sounds above. One of these rides along with the
/// player and with every tracked entity.
#[derive(Default)]
pub struct StepTracker {
    /// Vanilla's `moveDist`: distance travelled × 0.6.
    move_dist: f32,
    /// The whole number it has to pass for the next step (`nextStep`).
    next_step: f32,
    /// Same idea for swimming strokes.
    next_swim: f32,
    was_on_ground: bool,
    was_in_water: bool,
    /// Highest point since leaving the ground — how far there is to fall.
    fall_top: Option<f64>,
    started: bool,
}

impl StepTracker {
    /// Feed one frame of movement and get back whatever it should sound like.
    /// `pos` is the body's feet. Returns at most one event per frame, in
    /// vanilla's own order of importance (water first — a splash drowns out a
    /// step).
    pub fn update(
        &mut self,
        pos: [f64; 3],
        prev: [f64; 3],
        on_ground: bool,
        in_water: bool,
    ) -> Option<StepEvent> {
        // The first frame has no meaningful "previous", and a teleport would
        // otherwise sound like a sprint across the map.
        let dx = (pos[0] - prev[0]) as f32;
        let dz = (pos[2] - prev[2]) as f32;
        let horizontal = (dx * dx + dz * dz).sqrt();
        let teleported = horizontal > 8.0 || (pos[1] - prev[1]).abs() > 8.0;
        let first = !self.started;
        self.started = true;
        if teleported || first {
            self.move_dist = 0.0;
            self.next_step = 1.0;
            self.next_swim = 1.0;
            self.was_on_ground = on_ground;
            self.was_in_water = in_water;
            self.fall_top = (!on_ground).then_some(pos[1]);
            return None;
        }

        // --- water surface ---------------------------------------------------
        let entered_water = in_water && !self.was_in_water;
        let left_water = !in_water && self.was_in_water;
        self.was_in_water = in_water;

        // --- falling and landing --------------------------------------------
        let landed = on_ground && !self.was_on_ground;
        let fall = match self.fall_top {
            Some(top) if landed => (top - pos[1]).max(0.0) as f32,
            _ => 0.0,
        };
        if on_ground {
            self.fall_top = None;
        } else {
            // Rising resets the top; the fall is measured from the highest point.
            self.fall_top = Some(self.fall_top.map_or(pos[1], |t| t.max(pos[1])));
        }
        self.was_on_ground = on_ground;

        self.move_dist += horizontal * MOVE_FACTOR;

        if entered_water || left_water {
            // Crossing the surface restarts the step counter, like vanilla's
            // separate swim counter taking over.
            self.next_step = self.move_dist + 1.0;
            return Some(StepEvent::Splash);
        }
        // A landing in water is a splash, not a thud; that case is handled above
        // on the frame the surface was crossed.
        if landed && !in_water {
            self.next_step = self.move_dist + 1.0;
            return Some(StepEvent::Land { big: fall >= BIG_FALL });
        }
        if in_water {
            if self.move_dist > self.next_swim {
                self.next_swim = self.move_dist.floor() + 1.0;
                return Some(StepEvent::Swim);
            }
            return None;
        }
        if on_ground && self.move_dist > self.next_step {
            self.next_step = self.move_dist.floor() + 1.0;
            return Some(StepEvent::Step);
        }
        None
    }
}

/// The sound a body of this kind makes when it lands, in vanilla's naming.
/// Players have their own; everything else uses the generic pair.
pub fn fall_sound(kind: &str, big: bool) -> String {
    let family = if kind == "player" { "player" } else { "generic" };
    format!("entity.{family}.{}_fall", if big { "big" } else { "small" })
}

/// The sound of breaking the water's surface.
pub fn splash_sound(kind: &str) -> String {
    if kind == "player" {
        "entity.player.splash".to_string()
    } else {
        "entity.generic.splash".to_string()
    }
}

/// One swimming stroke.
pub fn swim_sound(kind: &str) -> String {
    if kind == "player" {
        "entity.player.swim".to_string()
    } else {
        "entity.generic.swim".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk in a straight line and count the steps. Ten blocks make a counter
    /// of 10 × 0.6 = 6.0, which *crosses* the whole numbers 1 to 5 — the sixth
    /// step lands just after the tenth block, exactly as in vanilla.
    #[test]
    fn one_step_every_one_and_two_thirds_blocks() {
        let mut t = StepTracker::default();
        let mut pos = [0.0, 64.0, 0.0];
        t.update(pos, pos, true, false); // first frame primes it
        let mut steps = 0;
        for _ in 0..100 {
            let prev = pos;
            pos[0] += 0.1;
            if t.update(pos, prev, true, false) == Some(StepEvent::Step) {
                steps += 1;
            }
        }
        assert_eq!(steps, 5, "ten blocks of walking should sound five times");
    }

    #[test]
    fn no_steps_in_mid_air() {
        let mut t = StepTracker::default();
        let mut pos = [0.0, 64.0, 0.0];
        t.update(pos, pos, false, false);
        for _ in 0..50 {
            let prev = pos;
            pos[0] += 0.2;
            assert_ne!(t.update(pos, prev, false, false), Some(StepEvent::Step));
        }
    }

    #[test]
    fn landing_is_measured_from_the_highest_point() {
        let mut t = StepTracker::default();
        let mut pos = [0.0, 70.0, 0.0];
        t.update(pos, pos, false, false);
        // Fall six blocks.
        for _ in 0..12 {
            let prev = pos;
            pos[1] -= 0.5;
            t.update(pos, prev, false, false);
        }
        let prev = pos;
        pos[1] -= 0.1;
        assert_eq!(t.update(pos, prev, true, false), Some(StepEvent::Land { big: true }));

        // A short hop is a small landing.
        let mut t = StepTracker::default();
        let mut pos = [0.0, 64.0, 0.0];
        t.update(pos, pos, true, false);
        let prev = pos;
        pos[1] += 1.2;
        t.update(pos, prev, false, false);
        let prev = pos;
        pos[1] -= 1.2;
        assert_eq!(t.update(pos, prev, true, false), Some(StepEvent::Land { big: false }));
    }

    #[test]
    fn crossing_the_surface_splashes_once_in_each_direction() {
        let mut t = StepTracker::default();
        let pos = [0.0, 62.0, 0.0];
        t.update(pos, pos, false, false);
        assert_eq!(t.update(pos, pos, false, true), Some(StepEvent::Splash));
        assert_eq!(t.update(pos, pos, false, true), None, "still under water");
        assert_eq!(t.update(pos, pos, false, false), Some(StepEvent::Splash));
    }

    #[test]
    fn a_teleport_makes_no_sound() {
        let mut t = StepTracker::default();
        let pos = [0.0, 64.0, 0.0];
        t.update(pos, pos, true, false);
        let far = [900.0, 64.0, 900.0];
        assert_eq!(t.update(far, pos, true, false), None);
        // …and the counter starts again from there.
        let mut p = far;
        let mut steps = 0;
        for _ in 0..20 {
            let prev = p;
            p[0] += 0.1;
            if t.update(p, prev, true, false) == Some(StepEvent::Step) {
                steps += 1;
            }
        }
        assert_eq!(steps, 1);
    }

    #[test]
    fn players_and_mobs_use_their_own_sound_names() {
        assert_eq!(fall_sound("player", true), "entity.player.big_fall");
        assert_eq!(fall_sound("cow", false), "entity.generic.small_fall");
        assert_eq!(splash_sound("player"), "entity.player.splash");
        assert_eq!(swim_sound("cow"), "entity.generic.swim");
    }
}
