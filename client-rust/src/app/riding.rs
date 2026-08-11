//! Riding a mount: what the animal under you can do, and the jump you charge
//! by holding the jump key.
//!
//! Vanilla keeps this on the client: the horse does not jump because you
//! pressed space, it jumps because the client watched you hold space, worked
//! out how hard, and told the server once, on release
//! (`ServerboundPlayerCommand::StartRidingJump`). Everything here is that
//! bookkeeping, kept away from the app so it can be tested on its own.

use std::time::{Duration, Instant};

/// One vanilla tick, which is the unit the jump charge is counted in.
const TICK: Duration = Duration::from_millis(50);

/// Mounts that rear up and jump — vanilla's `PlayerRideableJumping`.
///
/// Llamas are the exception in the horse family: they carry chests and spit,
/// but `Llama.canJump` says no.
pub fn jumpable(kind: &str) -> bool {
    matches!(
        kind,
        "horse" | "donkey" | "mule" | "skeleton_horse" | "zombie_horse" | "camel"
    )
}

/// Mounts with an inventory screen of their own — vanilla's
/// `HasCustomInventoryScreen`, which is what makes E open the saddle slots
/// instead of your own backpack.
pub fn has_inventory(kind: &str) -> bool {
    jumpable(kind) || matches!(kind, "llama" | "trader_llama")
}

/// Vanilla's jump strength curve (`LocalPlayer.aiStep`): ten ticks of holding
/// wind it up to full, and holding longer than that bleeds back down toward
/// four fifths — you cannot simply lean on the key.
pub fn charge_for_ticks(ticks: u32) -> f32 {
    if ticks < 10 {
        ticks as f32 * 0.1
    } else {
        0.8 + 2.0 / (ticks - 9) as f32 * 0.1
    }
}

/// The jump key, seen from the saddle.
#[derive(Debug, Default)]
pub struct RideJump {
    /// When the key went down, while it still is.
    held_since: Option<Instant>,
    /// Cleared on release; kept so the bar can be drawn one frame longer.
    last: f32,
}

impl RideJump {
    /// The jump key went down while riding something that can jump.
    pub fn press(&mut self, now: Instant) {
        if self.held_since.is_none() {
            self.held_since = Some(now);
        }
    }

    /// The jump key came up. Returns the power to send, on vanilla's 0..100
    /// scale, or `None` if we were not charging at all.
    pub fn release(&mut self, now: Instant) -> Option<u32> {
        let since = self.held_since.take()?;
        let charge = charge_for_ticks(ticks_since(since, now));
        self.last = 0.0;
        Some((charge * 100.0).floor().clamp(0.0, 100.0) as u32)
    }

    /// Drop the charge without sending anything (dismounted mid-hold).
    pub fn cancel(&mut self) {
        self.held_since = None;
        self.last = 0.0;
    }

    /// How full the jump bar is right now, 0..1.
    pub fn charge(&self) -> f32 {
        match self.held_since {
            Some(since) => charge_for_ticks(ticks_since(since, Instant::now())),
            None => self.last,
        }
    }

    /// Whether the key is being held.
    pub fn charging(&self) -> bool {
        self.held_since.is_some()
    }
}

fn ticks_since(since: Instant, now: Instant) -> u32 {
    (now.saturating_duration_since(since).as_secs_f64() / TICK.as_secs_f64()) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horses_jump_llamas_do_not() {
        assert!(jumpable("horse"));
        assert!(jumpable("camel"));
        assert!(!jumpable("llama"));
        assert!(!jumpable("pig"));
        assert!(!jumpable("boat"));
    }

    #[test]
    fn the_whole_horse_family_has_an_inventory() {
        for kind in ["horse", "donkey", "mule", "llama", "trader_llama", "camel"] {
            assert!(has_inventory(kind), "{kind}");
        }
        assert!(!has_inventory("pig"));
        assert!(!has_inventory("strider"));
    }

    #[test]
    fn charge_winds_up_then_bleeds_back() {
        assert_eq!(charge_for_ticks(0), 0.0);
        assert!((charge_for_ticks(5) - 0.5).abs() < 1e-6);
        // Ten ticks is the peak.
        assert!((charge_for_ticks(10) - 1.0).abs() < 1e-6);
        // Holding on gives less, never below vanilla's floor of 0.8.
        assert!(charge_for_ticks(20) < 1.0);
        assert!(charge_for_ticks(200) > 0.8);
        assert!(charge_for_ticks(20) > charge_for_ticks(40));
    }

    #[test]
    fn a_full_hold_sends_a_hundred() {
        let mut jump = RideJump::default();
        let t0 = Instant::now();
        jump.press(t0);
        assert!(jump.charging());
        let power = jump.release(t0 + Duration::from_millis(500)).unwrap();
        assert_eq!(power, 100);
        assert!(!jump.charging());
        assert_eq!(jump.charge(), 0.0);
    }

    #[test]
    fn a_tap_sends_almost_nothing() {
        let mut jump = RideJump::default();
        let t0 = Instant::now();
        jump.press(t0);
        assert_eq!(jump.release(t0 + Duration::from_millis(60)).unwrap(), 10);
    }

    #[test]
    fn releasing_without_pressing_sends_nothing() {
        let mut jump = RideJump::default();
        assert_eq!(jump.release(Instant::now()), None);
    }

    #[test]
    fn cancelling_forgets_the_charge() {
        let mut jump = RideJump::default();
        jump.press(Instant::now());
        jump.cancel();
        assert!(!jump.charging());
        assert_eq!(jump.release(Instant::now()), None);
    }
}
