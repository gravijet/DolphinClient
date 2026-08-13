//! The three things vanilla does to the camera that you never notice until
//! they are missing: the field of view breathing with your speed, the view
//! rolling towards whatever just hit you, and the world swimming under nausea.
//!
//! All of it is pure arithmetic over the player's state, so it lives here with
//! its own tests rather than inside the render loop.

/// How far the world may lean when you are hit (degrees). Vanilla's damage
/// tilt is small — it reads as a flinch, not a barrel roll.
const TILT_DEGREES: f32 = 14.0;

/// Vanilla's hurt animation runs for 10 ticks.
pub const HURT_SECS: f32 = 0.5;

/// The field-of-view multiplier for the way the player is moving.
///
/// Vanilla multiplies the base FOV by the player's movement-speed attribute
/// (so Speed widens the view and Slowness narrows it), adds a fixed amount for
/// sprinting and for creative flight, and folds the whole thing towards 1 by
/// the "FOV Effects" slider. `pull` is how far a bow is drawn (0..1), which
/// zooms *in* instead — vanilla's `getFovModifier`.
pub fn fov_multiplier(
    walk_speed: f32,
    sprinting: bool,
    flying: bool,
    pull: f32,
    fov_effects: f32,
) -> f32 {
    // The default walking speed; anything else is a potion or an attribute.
    const BASE_SPEED: f32 = 0.1;
    let mut m = 1.0;
    if walk_speed > 0.0 {
        // Vanilla: (speed / base + 1) / 2 — half the difference, so Speed II
        // does not turn the screen inside out.
        m *= (walk_speed / BASE_SPEED + 1.0) / 2.0;
    }
    if !m.is_finite() || m <= 0.0 {
        m = 1.0;
    }
    if sprinting {
        m *= 1.15;
    }
    if flying {
        m *= 1.1;
    }
    // The slider only scales how far the effect goes, never its direction.
    m = 1.0 + (m - 1.0) * fov_effects.clamp(0.0, 1.0);

    if pull > 0.0 {
        // Drawing a bow pulls the view in to 85% over the full draw; vanilla
        // eases it so the last part of the draw barely moves.
        let p = pull.clamp(0.0, 1.0);
        let eased = if p > 0.99 { 1.0 } else { 1.0 - (1.0 - p) * (1.0 - p) };
        m *= 1.0 - eased * 0.15 * fov_effects.clamp(0.0, 1.0);
    }
    m
}

/// The camera roll, in degrees, `t` seconds into being hit from `from_yaw`
/// (the direction the damage came from, in world degrees) while the player
/// faces `facing_yaw`.
///
/// Vanilla rolls the view towards the attacker and lets it spring back over
/// the hurt animation: `sin(progress·π)² · 14°`, signed by which side the hit
/// came from. Being hit from straight ahead or behind barely tilts at all,
/// which is why the flinch reads as direction and not just as noise.
pub fn damage_tilt(t: f32, facing_yaw: f32, from_yaw: f32, strength: f32) -> f32 {
    if t < 0.0 || t >= HURT_SECS || strength <= 0.0 {
        return 0.0;
    }
    let progress = t / HURT_SECS;
    let curve = (progress * std::f32::consts::PI).sin();
    // Angle between where you look and where it came from, wrapped to ±180.
    let mut rel = from_yaw - facing_yaw;
    while rel > 180.0 {
        rel -= 360.0;
    }
    while rel < -180.0 {
        rel += 360.0;
    }
    let side = rel.to_radians().sin();
    curve * curve * TILT_DEGREES * side * strength.clamp(0.0, 1.0)
}

/// The screen wobble under nausea (and inside a portal): vanilla's
/// `portalTime`/`confusion` warp. Returns a factor 0..1 the renderer can push
/// through its FOV and roll — 0 is a still world.
///
/// The two sources add up but are held to 1, because a player in a portal
/// holding nausea should not have the world turn inside out.
pub fn nausea_amount(nausea: f32, portal: f32) -> f32 {
    (nausea.clamp(0.0, 1.0) * 0.6 + portal.clamp(0.0, 1.0) * 0.8).min(1.0)
}

/// The nausea warp at a moment in time: an extra FOV multiplier and a small
/// roll, both breathing slowly. Vanilla drives this off the game clock, so it
/// keeps moving even when the player stands still.
pub fn nausea_warp(amount: f32, seconds: f32) -> (f32, f32) {
    if amount <= 0.0 {
        return (1.0, 0.0);
    }
    let a = amount.clamp(0.0, 1.0);
    // Two waves at different rates, so it never settles into a rhythm.
    let slow = (seconds * 1.7).sin();
    let fast = (seconds * 2.9).sin();
    (1.0 + a * 0.14 * slow, a * 3.5 * fast)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_still_leaves_the_view_alone() {
        assert_eq!(fov_multiplier(0.1, false, false, 0.0, 1.0), 1.0);
    }

    #[test]
    fn sprinting_widens_the_view_and_stopping_gives_it_back() {
        let run = fov_multiplier(0.1, true, false, 0.0, 1.0);
        assert!(run > 1.1 && run < 1.2, "{run}");
        assert_eq!(fov_multiplier(0.1, false, false, 0.0, 1.0), 1.0);
    }

    #[test]
    fn speed_widens_and_slowness_narrows() {
        let fast = fov_multiplier(0.2, false, false, 0.0, 1.0);
        let slow = fov_multiplier(0.05, false, false, 0.0, 1.0);
        assert!(fast > 1.0, "Speed should widen the view ({fast})");
        assert!(slow < 1.0, "Slowness should narrow it ({slow})");
    }

    #[test]
    fn the_slider_scales_the_effect_and_zero_switches_it_off() {
        let full = fov_multiplier(0.1, true, false, 0.0, 1.0);
        let half = fov_multiplier(0.1, true, false, 0.0, 0.5);
        assert!((half - 1.0 - (full - 1.0) / 2.0).abs() < 1e-5, "{half} vs {full}");
        assert_eq!(fov_multiplier(0.2, true, true, 0.7, 0.0), 1.0, "off means off");
    }

    #[test]
    fn drawing_a_bow_zooms_in() {
        let none = fov_multiplier(0.1, false, false, 0.0, 1.0);
        let half = fov_multiplier(0.1, false, false, 0.5, 1.0);
        let full = fov_multiplier(0.1, false, false, 1.0, 1.0);
        assert!(full < half && half < none, "{full} < {half} < {none}");
        assert!((full - 0.85).abs() < 0.001, "a full draw is vanilla's 85% ({full})");
    }

    #[test]
    fn a_broken_speed_attribute_cannot_break_the_camera() {
        assert_eq!(fov_multiplier(0.0, false, false, 0.0, 1.0), 1.0);
        assert!(fov_multiplier(f32::NAN, false, false, 0.0, 1.0).is_finite());
    }

    #[test]
    fn the_tilt_leans_towards_whoever_hit_you() {
        // Hit from the right while facing north.
        let right = damage_tilt(HURT_SECS / 2.0, 0.0, 90.0, 1.0);
        let left = damage_tilt(HURT_SECS / 2.0, 0.0, -90.0, 1.0);
        assert!(right > 0.0 && left < 0.0, "right {right}, left {left}");
        assert!((right + left).abs() < 1e-4, "the two sides should mirror");
        assert!(right <= TILT_DEGREES, "never past vanilla's {TILT_DEGREES}°");
    }

    #[test]
    fn a_hit_from_dead_ahead_barely_tilts() {
        let ahead = damage_tilt(HURT_SECS / 2.0, 0.0, 0.0, 1.0).abs();
        let side = damage_tilt(HURT_SECS / 2.0, 0.0, 90.0, 1.0).abs();
        assert!(ahead < 0.001, "{ahead}");
        assert!(side > ahead);
    }

    #[test]
    fn the_tilt_starts_at_nothing_springs_out_and_comes_back() {
        let at = |t: f32| damage_tilt(t, 0.0, 90.0, 1.0);
        assert_eq!(at(0.0), 0.0);
        assert!(at(HURT_SECS * 0.25) > 0.0);
        assert!(at(HURT_SECS / 2.0) > at(HURT_SECS * 0.25));
        assert_eq!(at(HURT_SECS), 0.0, "over when the hurt animation is over");
        assert_eq!(at(HURT_SECS + 1.0), 0.0);
        assert_eq!(at(-0.1), 0.0);
    }

    #[test]
    fn nausea_and_a_portal_stack_but_stay_bounded() {
        assert_eq!(nausea_amount(0.0, 0.0), 0.0);
        assert!(nausea_amount(1.0, 0.0) > 0.0);
        assert!(nausea_amount(1.0, 1.0) <= 1.0);
        assert!(nausea_amount(0.5, 0.5) > nausea_amount(0.5, 0.0));
    }

    #[test]
    fn a_still_world_when_nothing_is_wrong() {
        assert_eq!(nausea_warp(0.0, 12.3), (1.0, 0.0));
    }

    #[test]
    fn the_warp_keeps_moving_and_stays_small() {
        let (f1, r1) = nausea_warp(1.0, 0.4);
        let (f2, r2) = nausea_warp(1.0, 1.9);
        assert!((f1 - f2).abs() > 1e-3 || (r1 - r2).abs() > 1e-3, "it should breathe");
        for t in 0..200 {
            let (f, r) = nausea_warp(1.0, t as f32 * 0.1);
            assert!((0.85..=1.15).contains(&f), "fov {f}");
            assert!(r.abs() <= 3.5001, "roll {r}");
        }
    }
}
