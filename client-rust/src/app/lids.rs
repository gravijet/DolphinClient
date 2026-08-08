//! Containers that open.
//!
//! A chest is not part of the terrain: vanilla's chest block model is
//! particle-only, and the client draws the box and its lid every frame so the
//! lid can swing. The server only ever says *how many players have this open*
//! (a block event with the viewer count); how far the lid has actually travelled
//! is the client's business, and it is the same on every client:
//!
//! * the lid moves a tenth of the way each tick — half a second, open or shut;
//! * the angle it is drawn at is not that fraction but `1-(1-t)³` of a quarter
//!   turn, so it leaps open and settles closed;
//! * a shulker box has no hinge — its lid rises half a block and turns three
//!   quarters of a turn as it goes.
//!
//! Both halves of a double chest move together, which matters because the
//! server does not always tell us about both.

use std::collections::HashMap;

use crate::types::BlockPos;

/// Vanilla's lid speed: a tenth of the way per tick, i.e. half a second end to
/// end at 20 ticks a second.
const RATE: f32 = 2.0;

/// One container's lid.
#[derive(Clone, Copy, Debug, Default)]
struct Lid {
    /// How many players the server says have it open.
    viewers: u8,
    /// How far it has actually travelled, 0 (shut) .. 1 (open).
    progress: f32,
}

/// Every lid the client is currently animating. Entries appear when a container
/// is opened and are dropped again once they are shut and still, so a world full
/// of chests costs nothing.
#[derive(Default)]
pub struct Lids {
    map: HashMap<BlockPos, Lid>,
}

impl Lids {
    /// The server's viewer count for a container changed.
    pub fn set_viewers(&mut self, pos: BlockPos, viewers: u8) {
        self.map.entry(pos).or_default().viewers = viewers;
    }

    /// Move every lid on by `dt` seconds and forget the ones that have finished
    /// closing.
    pub fn tick(&mut self, dt: f32) {
        let step = dt * RATE;
        self.map.retain(|_, lid| {
            if lid.viewers > 0 {
                lid.progress = (lid.progress + step).min(1.0);
            } else {
                lid.progress = (lid.progress - step).max(0.0);
            }
            lid.viewers > 0 || lid.progress > 0.0
        });
    }

    /// How far this container's lid has travelled, 0..1.
    pub fn progress(&self, pos: BlockPos) -> f32 {
        self.map.get(&pos).map_or(0.0, |l| l.progress)
    }

    /// Is anything moving at all? Used to skip the whole pass on a still world.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Drop lids in chunk columns that unloaded.
    pub fn retain_chunks(&mut self, keep: impl Fn(i32, i32) -> bool) {
        self.map.retain(|p, _| keep(p.x >> 4, p.z >> 4));
    }
}

/// Vanilla's eased lid angle in radians: `1-(1-t)³` of a quarter turn, so the
/// lid throws itself open and eases shut instead of moving at a constant rate.
pub fn chest_angle(progress: f32) -> f32 {
    let t = 1.0 - progress.clamp(0.0, 1.0);
    (1.0 - t * t * t) * std::f32::consts::FRAC_PI_2
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: BlockPos = BlockPos { x: 1, y: 2, z: 3 };

    #[test]
    fn a_lid_takes_half_a_second_each_way() {
        let mut lids = Lids::default();
        lids.set_viewers(P, 1);
        // 10 ticks at vanilla's tenth-per-tick.
        for _ in 0..10 {
            lids.tick(0.05);
        }
        assert!((lids.progress(P) - 1.0).abs() < 1e-4, "{}", lids.progress(P));
        lids.set_viewers(P, 0);
        for _ in 0..10 {
            lids.tick(0.05);
        }
        assert_eq!(lids.progress(P), 0.0);
    }

    #[test]
    fn a_shut_still_lid_is_forgotten() {
        let mut lids = Lids::default();
        lids.set_viewers(P, 1);
        lids.tick(0.5);
        lids.set_viewers(P, 0);
        assert!(!lids.is_empty());
        lids.tick(0.5);
        assert!(lids.is_empty(), "a closed chest should not be tracked forever");
    }

    #[test]
    fn a_lid_never_overshoots() {
        let mut lids = Lids::default();
        lids.set_viewers(P, 2);
        lids.tick(10.0);
        assert_eq!(lids.progress(P), 1.0);
        lids.set_viewers(P, 0);
        lids.tick(10.0);
        assert_eq!(lids.progress(P), 0.0);
    }

    #[test]
    fn the_lid_angle_is_a_quarter_turn_eased() {
        assert_eq!(chest_angle(0.0), 0.0);
        assert!((chest_angle(1.0) - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        // Vanilla's cubic ease-out: halfway through the animation the lid is
        // already most of the way open.
        assert!(chest_angle(0.5) > 0.8 * std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn unloading_a_chunk_drops_its_lids() {
        let mut lids = Lids::default();
        lids.set_viewers(P, 1);
        lids.set_viewers(BlockPos { x: 400, y: 64, z: 0 }, 1);
        lids.retain_chunks(|x, _| x == 0);
        assert_eq!(lids.progress(P), 0.0);
        lids.tick(0.05);
        assert!(lids.progress(P) > 0.0, "the near chest is still tracked");
    }
}
