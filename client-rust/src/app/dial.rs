//! Compass needle and clock hand: which of the 32 (compass) / 64 (clock)
//! baked icon frames to show right now. Vanilla's own compass/clock item
//! models pick a frame via an `angle` (0..1) predicate on the item,
//! recomputed client-side every frame — this module is that computation;
//! `assets/items.rs::ItemIcons::resolve` is where the frame index it
//! produces actually gets applied to a name lookup, so every existing
//! render call site (hotbar, hand, inventory, chests, …) picks it up with
//! no changes of its own.

/// Clock hand position, 0..64, from the world time vanilla sends (ticks,
/// 24000 per day). Real vanilla clocks run a quarter-cycle ahead of the raw
/// day-time value (`ClockItemPropertyFunction`) so the hand is already at
/// "6 AM" when the sky is — this matches that offset.
pub fn clock_frame(world_time: i64) -> u8 {
    let day_frac = (world_time.rem_euclid(24000) as f32) / 24000.0;
    let angle = (day_frac + 0.25).rem_euclid(1.0);
    ((angle * 64.0) as u32 % 64) as u8
}

/// Compass needle position, 0..32.
///
/// - `pos`/`yaw`: the holder's eye position (world X/Z) and facing, in the
///   same convention as `render::camera::view_dir` (yaw 0 = +Z).
/// - `target`: the position it should point at (world/bed spawn), or `None`
///   before the server has ever sent one.
/// - `no_signal`: no reliable direction to give (vanilla: the Nether, or any
///   dimension other than the one the tracked position is in) — the needle
///   spins uselessly instead of pointing somewhere meaningless.
/// - `wobble_t`: a free-running seconds counter driving that spin.
pub fn compass_frame(
    pos: [f64; 2],
    yaw: f32,
    target: Option<[f64; 2]>,
    no_signal: bool,
    wobble_t: f32,
) -> u8 {
    let angle01 = match target.filter(|_| !no_signal) {
        None => (wobble_t * 0.6).rem_euclid(1.0),
        Some(t) => {
            let (dx, dz) = (t[0] - pos[0], t[1] - pos[1]);
            // Bearing to the target in the same space `view_dir` builds a
            // forward vector in: `(-sin(a), cos(a))`, so solving for `a`
            // gives `atan2(-dx, dz)`.
            let bearing = (-dx as f32).atan2(dz as f32).to_degrees();
            ((bearing - yaw).rem_euclid(360.0)) / 360.0
        }
    };
    ((angle01 * 32.0) as u32 % 32) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_wraps_once_per_day() {
        let f0 = clock_frame(0);
        let f_full_day = clock_frame(24000);
        assert_eq!(f0, f_full_day, "a full day must land back on the same frame");
        // Monotonic-ish: walking forward in small steps shouldn't jump frames
        // backwards (mod wraparound aside).
        let mut prev = clock_frame(0);
        let mut wrapped = 0;
        for t in (0..24000).step_by(500) {
            let f = clock_frame(t);
            if f < prev {
                wrapped += 1;
            }
            prev = f;
        }
        assert_eq!(wrapped, 1, "the hand should wrap exactly once over a full day");
    }

    #[test]
    fn compass_points_at_target_ahead() {
        // Facing +Z (yaw 0), target due north of the player along +Z: dead
        // ahead, so the needle sits at frame 0.
        let f = compass_frame([0.0, 0.0], 0.0, Some([0.0, 100.0]), false, 0.0);
        assert_eq!(f, 0);
    }

    #[test]
    fn compass_flips_when_target_behind() {
        let ahead = compass_frame([0.0, 0.0], 0.0, Some([0.0, 100.0]), false, 0.0);
        let behind = compass_frame([0.0, 0.0], 0.0, Some([0.0, -100.0]), false, 0.0);
        assert_ne!(ahead, behind);
        // Directly behind is a half-turn (16 frames) from directly ahead.
        assert_eq!((behind as i32 - ahead as i32).rem_euclid(32), 16);
    }

    #[test]
    fn compass_tracks_player_turning() {
        // Same target, player turned 90°: the needle should read a quarter
        // turn away from the straight-ahead case.
        let straight = compass_frame([0.0, 0.0], 0.0, Some([0.0, 100.0]), false, 0.0);
        let turned = compass_frame([0.0, 0.0], 90.0, Some([0.0, 100.0]), false, 0.0);
        // Turning the player right swings the (unmoved) target 90° to the
        // left relative to facing — a quarter-turn the other way, i.e. -8
        // frames, which is +24 mod 32.
        assert_eq!((turned as i32 - straight as i32).rem_euclid(32), 24);
    }

    #[test]
    fn compass_spins_without_a_signal() {
        let a = compass_frame([0.0, 0.0], 0.0, Some([0.0, 100.0]), true, 1.0);
        let b = compass_frame([0.0, 0.0], 0.0, Some([0.0, 100.0]), true, 5.0);
        assert_ne!(a, b, "with no signal the needle must move over time regardless of position");
    }

    #[test]
    fn compass_with_no_target_yet_still_spins() {
        let a = compass_frame([0.0, 0.0], 0.0, None, false, 1.0);
        let b = compass_frame([0.0, 0.0], 0.0, None, false, 5.0);
        assert_ne!(a, b);
    }
}
