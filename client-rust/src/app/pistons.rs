//! Piston animations in flight.
//!
//! One entry per piston that has just fired. The blocks it moves are drawn by
//! the client as loose geometry sliding from their old cell to their new one —
//! the server sends no updates at all while they are in the air, exactly as in
//! vanilla, where both sides run the same structure resolver and animate for
//! two ticks.

use std::time::Instant;

use crate::types::{BlockPos, Face, StateId};

/// Vanilla moves a piston structure by 0.5 per tick: two ticks, 100 ms.
pub const MOVE_SECS: f32 = 0.1;
/// The real blocks land a tick after the movement finishes. Keep the animation
/// on screen a little past that so there is never an empty frame between the
/// two; individual blocks drop out sooner once the server's version turns up.
pub const LIFE_SECS: f32 = 0.25;

/// One block riding along with a piston.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rider {
    /// Where it started.
    pub src: BlockPos,
    /// What it looks like.
    pub state: StateId,
}

/// One piston mid-stroke.
#[derive(Clone, Debug)]
pub struct Stroke {
    pub piston: BlockPos,
    /// Which way the piston points (not necessarily where the blocks go).
    pub facing: Face,
    pub extending: bool,
    pub blocks: Vec<Rider>,
    /// Whether to draw the head sliding. Suppressed while retracting with a
    /// block in tow, because the server leaves the old head block standing
    /// until the stroke finishes and two heads look worse than none.
    pub head: bool,
    /// The `piston_head` state to draw for it.
    pub head_state: StateId,
    started: Instant,
}

impl Stroke {
    pub fn new(
        piston: BlockPos,
        facing: Face,
        extending: bool,
        blocks: Vec<Rider>,
        head: bool,
        head_state: StateId,
        now: Instant,
    ) -> Self {
        Self { piston, facing, extending, blocks, head, head_state, started: now }
    }

    /// Which way the structure travels.
    pub fn travel(&self) -> Face {
        if self.extending { self.facing } else { self.facing.opposite() }
    }

    /// 0 at the start of the stroke, 1 once everything has arrived.
    pub fn progress(&self, now: Instant) -> f32 {
        (now.duration_since(self.started).as_secs_f32() / MOVE_SECS).clamp(0.0, 1.0)
    }

    pub fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.started).as_secs_f32() > LIFE_SECS
    }

    /// Where a rider is drawn: from its own cell to the next one along.
    pub fn rider_pos(&self, r: &Rider, progress: f32) -> [f64; 3] {
        let d = self.travel().normal();
        [
            r.src.x as f64 + d[0] as f64 * progress as f64,
            r.src.y as f64 + d[1] as f64 * progress as f64,
            r.src.z as f64 + d[2] as f64 * progress as f64,
        ]
    }

    /// Where the head is drawn. Extending it grows out of the piston, and
    /// retracting it slides back in.
    pub fn head_pos(&self, progress: f32) -> [f64; 3] {
        let d = self.facing.normal();
        let t = if self.extending { progress } else { 1.0 - progress };
        [
            self.piston.x as f64 + d[0] as f64 * t as f64,
            self.piston.y as f64 + d[1] as f64 * t as f64,
            self.piston.z as f64 + d[2] as f64 * t as f64,
        ]
    }

    /// Where a rider ends up — the cell the server will fill in.
    pub fn rider_dest(&self, r: &Rider) -> BlockPos {
        let d = self.travel().normal();
        BlockPos { x: r.src.x + d[0], y: r.src.y + d[1], z: r.src.z + d[2] }
    }
}

/// Every stroke currently in flight.
#[derive(Default)]
pub struct Pistons {
    strokes: Vec<Stroke>,
}

impl Pistons {
    /// Start a stroke, replacing any older one from the same piston.
    pub fn start(&mut self, stroke: Stroke) {
        self.strokes.retain(|s| s.piston != stroke.piston);
        self.strokes.push(stroke);
    }

    /// Drop everything that has run its course.
    pub fn tick(&mut self, now: Instant) {
        self.strokes.retain(|s| !s.expired(now));
    }

    pub fn iter(&self) -> impl Iterator<Item = &Stroke> {
        self.strokes.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty()
    }

    pub fn clear(&mut self) {
        self.strokes.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.strokes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn stroke(now: Instant, extending: bool) -> Stroke {
        Stroke::new(
            BlockPos { x: 0, y: 0, z: 0 },
            Face::East,
            extending,
            vec![Rider { src: BlockPos { x: 1, y: 0, z: 0 }, state: 1 }],
            true,
            2,
            now,
        )
    }

    #[test]
    fn a_stroke_runs_from_nought_to_one_in_two_ticks() {
        let now = Instant::now();
        let s = stroke(now, true);
        assert_eq!(s.progress(now), 0.0);
        assert!((s.progress(now + Duration::from_millis(50)) - 0.5).abs() < 0.05);
        assert_eq!(s.progress(now + Duration::from_millis(200)), 1.0);
    }

    #[test]
    fn a_rider_slides_exactly_one_cell() {
        let now = Instant::now();
        let s = stroke(now, true);
        let r = s.blocks[0];
        assert_eq!(s.rider_pos(&r, 0.0), [1.0, 0.0, 0.0]);
        assert_eq!(s.rider_pos(&r, 1.0), [2.0, 0.0, 0.0]);
        assert_eq!(s.rider_dest(&r), BlockPos { x: 2, y: 0, z: 0 });
    }

    #[test]
    fn retracting_pulls_the_other_way() {
        let now = Instant::now();
        let s = stroke(now, false);
        let r = Rider { src: BlockPos { x: 2, y: 0, z: 0 }, state: 1 };
        assert_eq!(s.travel(), Face::West);
        assert_eq!(s.rider_pos(&r, 1.0), [1.0, 0.0, 0.0]);
        // The head starts fully out and ends up inside the piston.
        assert_eq!(s.head_pos(0.0), [1.0, 0.0, 0.0]);
        assert_eq!(s.head_pos(1.0), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_second_stroke_from_the_same_piston_replaces_the_first() {
        let now = Instant::now();
        let mut p = Pistons::default();
        p.start(stroke(now, true));
        p.start(stroke(now, false));
        assert_eq!(p.len(), 1);
        assert!(!p.iter().next().unwrap().extending);
    }

    #[test]
    fn strokes_expire() {
        let now = Instant::now();
        let mut p = Pistons::default();
        p.start(stroke(now, true));
        p.tick(now + Duration::from_millis(100));
        assert_eq!(p.len(), 1);
        p.tick(now + Duration::from_millis(400));
        assert!(p.is_empty());
    }
}
