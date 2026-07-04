//! Render-side copy of the world. Single writer (app thread applies GameEvents),
//! meshing reads immutable `PaddedSnapshot` copies on rayon threads.

pub mod mesher;

use crate::bridge::events::GameEvent;
use crate::types::{
    BlockPos, ChunkPos, Face, PADDED_VOLUME, PaddedSnapshot, SectionData, SectionPos, StateId,
    nibble,
};
use std::collections::{HashMap, HashSet};
use tracing::warn;

#[derive(Default)]
pub struct WorldMirror {
    sections: HashMap<SectionPos, SectionData>,
    dirty: HashSet<SectionPos>,
    /// Sections removed since the last `take_removed` (GPU eviction queue).
    removed: Vec<SectionPos>,
}

impl WorldMirror {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a world-affecting event. `Section` inserts/replaces and dirties the
    /// section + its 6 face neighbors (their border faces may change).
    /// `BlockChanged` writes the state and dirties the section (+ neighbors when
    /// the block sits on a border). `ChunkUnloaded` removes that column's
    /// sections and returns them via `take_removed` semantics — implementation:
    /// collect removals internally; app drains with [`take_removed`].
    /// Non-world events are ignored.
    pub fn apply(&mut self, ev: &GameEvent) {
        match ev {
            GameEvent::Section { pos, data } => {
                self.sections.insert(*pos, data.clone());
                self.dirty.insert(*pos);
                for f in Face::ALL {
                    let n = f.normal();
                    self.dirty.insert(SectionPos {
                        x: pos.x + n[0],
                        y: pos.y + n[1],
                        z: pos.z + n[2],
                    });
                }
            }
            GameEvent::BlockChanged { pos, state } => {
                let sp = pos.section();
                let Some(sec) = self.sections.get_mut(&sp) else {
                    warn!(
                        "BlockChanged at {:?} for unloaded section {:?}; skipped",
                        pos, sp
                    );
                    return;
                };
                sec.blocks[pos.section_index()] = *state;
                // Dirty every section whose padded snapshot contains this block:
                // self plus face/edge/corner neighbors the block borders on.
                let (lx, ly, lz) = (pos.x & 15, pos.y & 15, pos.z & 15);
                let deltas = |l: i32| -> &'static [i32] {
                    match l {
                        0 => &[0, -1],
                        15 => &[0, 1],
                        _ => &[0],
                    }
                };
                for &dy in deltas(ly) {
                    for &dz in deltas(lz) {
                        for &dx in deltas(lx) {
                            self.dirty.insert(SectionPos {
                                x: sp.x + dx,
                                y: sp.y + dy,
                                z: sp.z + dz,
                            });
                        }
                    }
                }
            }
            GameEvent::ChunkUnloaded { pos } => {
                let gone: Vec<SectionPos> = self
                    .sections
                    .keys()
                    .copied()
                    .filter(|s| s.x == pos.x && s.z == pos.z)
                    .collect();
                self.remove_sections(&gone);
                self.removed.extend_from_slice(&gone);
            }
            _ => {}
        }
    }

    /// Remove `gone` from the map/dirty set and dirty their surviving face
    /// neighbors (borders that were culled against them become exposed).
    fn remove_sections(&mut self, gone: &[SectionPos]) {
        for p in gone {
            self.sections.remove(p);
            self.dirty.remove(p);
        }
        for p in gone {
            for f in Face::ALL {
                let n = f.normal();
                let np = SectionPos { x: p.x + n[0], y: p.y + n[1], z: p.z + n[2] };
                if self.sections.contains_key(&np) {
                    self.dirty.insert(np);
                }
            }
        }
    }

    /// Sections removed since the last call (for GPU buffer eviction).
    pub fn take_removed(&mut self) -> Vec<SectionPos> {
        std::mem::take(&mut self.removed)
    }

    /// Up to `budget` dirty sections, nearest to `center` first, removed from
    /// the dirty set. Sections whose data is missing are skipped/dropped.
    pub fn take_dirty(&mut self, center: [f64; 3], budget: usize) -> Vec<SectionPos> {
        // Drop dirty marks with no backing data (they'll be re-dirtied when
        // their Section event arrives).
        let sections = &self.sections;
        self.dirty.retain(|p| sections.contains_key(p));
        if budget == 0 || self.dirty.is_empty() {
            return Vec::new();
        }
        let mut all: Vec<SectionPos> = self.dirty.iter().copied().collect();
        let d2 = |p: &SectionPos| -> f64 {
            let c = p.center();
            let (dx, dy, dz) = (c[0] - center[0], c[1] - center[1], c[2] - center[2]);
            dx * dx + dy * dy + dz * dz
        };
        all.sort_by(|a, b| d2(a).total_cmp(&d2(b)));
        all.truncate(budget);
        for p in &all {
            self.dirty.remove(p);
        }
        all
    }

    pub fn is_dirty_empty(&self) -> bool {
        self.dirty.is_empty()
    }

    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// 0 (air) when the section isn't loaded.
    pub fn get_block(&self, pos: BlockPos) -> StateId {
        self.sections
            .get(&pos.section())
            .map_or(0, |s| s.blocks[pos.section_index()])
    }

    /// 18³ copy of `pos` ± 1 with combined light (see PaddedSnapshot docs).
    /// None if the center section isn't loaded. Missing neighbors → air, light
    /// unknown (0xFF).
    pub fn snapshot27(&self, pos: SectionPos) -> Option<PaddedSnapshot> {
        let center = self.sections.get(&pos)?;

        let mut blocks: Box<[StateId; PADDED_VOLUME]> = vec![0u32; PADDED_VOLUME]
            .into_boxed_slice()
            .try_into()
            .expect("PADDED_VOLUME sized vec");
        let mut light: Box<[u8; PADDED_VOLUME]> = vec![0xFFu8; PADDED_VOLUME]
            .into_boxed_slice()
            .try_into()
            .expect("PADDED_VOLUME sized vec");

        // Padded coords covered by a neighbor at section offset o along one axis.
        fn coords(o: i32) -> std::ops::RangeInclusive<i32> {
            match o {
                -1 => -1..=-1,
                0 => 0..=15,
                _ => 16..=16,
            }
        }

        for oy in -1..=1 {
            for oz in -1..=1 {
                for ox in -1..=1 {
                    let np = SectionPos { x: pos.x + ox, y: pos.y + oy, z: pos.z + oz };
                    let Some(sec) = self.sections.get(&np) else {
                        continue; // stays air / 0xFF
                    };
                    for py in coords(oy) {
                        for pz in coords(oz) {
                            for px in coords(ox) {
                                // Local coord inside the source section.
                                let (sx, sy, sz) =
                                    ((px & 15) as usize, (py & 15) as usize, (pz & 15) as usize);
                                let sidx = (sy * 16 + sz) * 16 + sx;
                                let pidx = PaddedSnapshot::idx(px, py, pz);
                                blocks[pidx] = sec.blocks[sidx];
                                light[pidx] = combined_light(sec, sidx);
                            }
                        }
                    }
                }
            }
        }

        Some(PaddedSnapshot { pos, blocks, light, biome: dominant_biome(center) })
    }

    /// Drop sections whose chunk is farther than `radius` chunks (Chebyshev)
    /// from `center`; returns removed positions for GPU eviction.
    pub fn unload_far(&mut self, center: ChunkPos, radius: i32) -> Vec<SectionPos> {
        let far: Vec<SectionPos> = self
            .sections
            .keys()
            .copied()
            .filter(|p| (p.x - center.x).abs().max((p.z - center.z).abs()) > radius)
            .collect();
        self.remove_sections(&far);
        far
    }

    /// Amanatides–Woo voxel walk from `origin` along `dir` (normalized) up to
    /// `max_dist`. Returns first non-air block + entry face. v1 treats every
    /// non-air state as a full cube.
    pub fn raycast(
        &self,
        origin: [f64; 3],
        dir: [f64; 3],
        max_dist: f64,
        is_air: impl Fn(StateId) -> bool,
    ) -> Option<(BlockPos, Face)> {
        let mut cell = [
            origin[0].floor() as i32,
            origin[1].floor() as i32,
            origin[2].floor() as i32,
        ];

        // Origin already inside a solid block: report it, entering "backwards"
        // along the dominant travel axis (best available convention).
        let start = BlockPos { x: cell[0], y: cell[1], z: cell[2] };
        if !is_air(self.get_block(start)) {
            return Some((start, dominant_entry_face(dir)));
        }

        let mut t_max = [f64::INFINITY; 3];
        let mut t_delta = [f64::INFINITY; 3];
        let mut step = [0i32; 3];
        for i in 0..3 {
            if dir[i] > 0.0 {
                step[i] = 1;
                t_delta[i] = 1.0 / dir[i];
                t_max[i] = (cell[i] as f64 + 1.0 - origin[i]) / dir[i];
            } else if dir[i] < 0.0 {
                step[i] = -1;
                t_delta[i] = -1.0 / dir[i];
                t_max[i] = (cell[i] as f64 - origin[i]) / dir[i];
            }
            // dir[i] == 0.0 (or NaN): never step this axis (t stays INFINITY).
        }

        // Normalized dir crosses ≤ ~√3 boundaries per unit distance; the cap
        // only guards against a non-normalized caller looping excessively.
        let max_steps = (max_dist.max(0.0) * 3.0).min(1e7) as usize + 8;
        for _ in 0..max_steps {
            let axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };
            // Also bails when all components are 0/NaN (t_max = INFINITY): the
            // negated `<=` is deliberate so NaN/∞ (never `<= max_dist`) bails.
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            if !(t_max[axis] <= max_dist) {
                return None;
            }
            cell[axis] += step[axis];
            t_max[axis] += t_delta[axis];
            let face = entry_face(axis, step[axis]);
            let bp = BlockPos { x: cell[0], y: cell[1], z: cell[2] };
            if !is_air(self.get_block(bp)) {
                return Some((bp, face));
            }
        }
        None
    }
}

/// Combined light byte for one cell: low nibble sky, high nibble block.
/// No light data at all → 0xFF (unknown; readers fall back to sky 15).
fn combined_light(sec: &SectionData, idx: usize) -> u8 {
    match (&sec.sky_light, &sec.block_light) {
        (None, None) => 0xFF,
        (s, b) => {
            let sky = s.as_deref().map_or(15, |a| nibble(a, idx));
            let blk = b.as_deref().map_or(0, |a| nibble(a, idx));
            (sky & 0xF) | (blk << 4)
        }
    }
}

/// Most common biome id of a section (v1: one biome per snapshot).
fn dominant_biome(sec: &SectionData) -> u32 {
    let mut counts: Vec<(u32, u32)> = Vec::new();
    for &b in sec.biomes.iter() {
        match counts.iter_mut().find(|(id, _)| *id == b) {
            Some((_, c)) => *c += 1,
            None => counts.push((b, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(_, c)| c)
        .map_or(0, |(id, _)| id)
}

/// Face through which a ray travelling along `axis` with `step` enters a block.
fn entry_face(axis: usize, step: i32) -> Face {
    match (axis, step > 0) {
        (0, true) => Face::West,
        (0, false) => Face::East,
        (1, true) => Face::Down,
        (1, false) => Face::Up,
        (2, true) => Face::North,
        _ => Face::South,
    }
}

/// Entry face for the origin-inside-a-block case: oppose the dominant
/// direction component (arbitrary but stable; Up for a zero direction).
fn dominant_entry_face(dir: [f64; 3]) -> Face {
    let ax = dir[0].abs();
    let ay = dir[1].abs();
    let az = dir[2].abs();
    if ax >= ay && ax >= az && ax > 0.0 {
        entry_face(0, if dir[0] > 0.0 { 1 } else { -1 })
    } else if ay >= az && ay > 0.0 {
        entry_face(1, if dir[1] > 0.0 { 1 } else { -1 })
    } else if az > 0.0 {
        entry_face(2, if dir[2] > 0.0 { 1 } else { -1 })
    } else {
        Face::Up
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SECTION_VOLUME;

    fn section_with(blocks: &[(usize, StateId)]) -> SectionData {
        let mut s = SectionData::empty();
        for &(i, id) in blocks {
            s.blocks[i] = id;
        }
        s
    }

    fn idx(x: usize, y: usize, z: usize) -> usize {
        (y * 16 + z) * 16 + x
    }

    fn sp(x: i32, y: i32, z: i32) -> SectionPos {
        SectionPos { x, y, z }
    }

    fn bp(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos { x, y, z }
    }

    #[test]
    fn section_event_dirties_self_and_face_neighbors() {
        let mut w = WorldMirror::new();
        w.apply(&GameEvent::Section { pos: sp(1, 2, 3), data: SectionData::empty() });
        assert_eq!(w.section_count(), 1);
        assert_eq!(w.dirty.len(), 7);
        for p in [
            sp(1, 2, 3),
            sp(0, 2, 3),
            sp(2, 2, 3),
            sp(1, 1, 3),
            sp(1, 3, 3),
            sp(1, 2, 2),
            sp(1, 2, 4),
        ] {
            assert!(w.dirty.contains(&p), "missing dirty {p:?}");
        }
    }

    #[test]
    fn block_changed_writes_and_dirties_borders() {
        let mut w = WorldMirror::new();
        w.apply(&GameEvent::Section { pos: sp(0, 0, 0), data: SectionData::empty() });
        w.dirty.clear();

        // Interior block: only its own section.
        w.apply(&GameEvent::BlockChanged { pos: bp(8, 8, 8), state: 7 });
        assert_eq!(w.get_block(bp(8, 8, 8)), 7);
        assert_eq!(w.dirty.len(), 1);
        assert!(w.dirty.contains(&sp(0, 0, 0)));

        // Corner block (0,0,0): 8 sections share its padded snapshots.
        w.dirty.clear();
        w.apply(&GameEvent::BlockChanged { pos: bp(0, 0, 0), state: 9 });
        assert_eq!(w.get_block(bp(0, 0, 0)), 9);
        assert_eq!(w.dirty.len(), 8);
        assert!(w.dirty.contains(&sp(-1, -1, -1)));
        assert!(w.dirty.contains(&sp(0, 0, 0)));
        assert!(w.dirty.contains(&sp(-1, 0, 0)));

        // Face-border block (15, 8, 8): self + east neighbor.
        w.dirty.clear();
        w.apply(&GameEvent::BlockChanged { pos: bp(15, 8, 8), state: 3 });
        assert_eq!(w.dirty.len(), 2);
        assert!(w.dirty.contains(&sp(1, 0, 0)));

        // Unloaded section: ignored, nothing dirtied.
        w.dirty.clear();
        w.apply(&GameEvent::BlockChanged { pos: bp(100, 0, 0), state: 5 });
        assert!(w.dirty.is_empty());
        assert_eq!(w.get_block(bp(100, 0, 0)), 0);
    }

    #[test]
    fn chunk_unloaded_removes_column_and_dirties_neighbors() {
        let mut w = WorldMirror::new();
        for y in 0..3 {
            w.apply(&GameEvent::Section { pos: sp(0, y, 0), data: SectionData::empty() });
        }
        w.apply(&GameEvent::Section { pos: sp(1, 0, 0), data: SectionData::empty() });
        w.dirty.clear();
        w.take_removed();

        w.apply(&GameEvent::ChunkUnloaded { pos: ChunkPos { x: 0, z: 0 } });
        assert_eq!(w.section_count(), 1);
        let mut removed = w.take_removed();
        removed.sort();
        assert_eq!(removed, vec![sp(0, 0, 0), sp(0, 1, 0), sp(0, 2, 0)]);
        assert!(w.take_removed().is_empty());
        // Surviving neighbor of a removed section got dirtied; removed ones did not.
        assert!(w.dirty.contains(&sp(1, 0, 0)));
        assert!(!w.dirty.contains(&sp(0, 0, 0)));
    }

    #[test]
    fn take_dirty_nearest_first_drops_missing() {
        let mut w = WorldMirror::new();
        w.apply(&GameEvent::Section { pos: sp(0, 0, 0), data: SectionData::empty() });
        w.apply(&GameEvent::Section { pos: sp(5, 0, 0), data: SectionData::empty() });
        w.apply(&GameEvent::Section { pos: sp(2, 0, 0), data: SectionData::empty() });
        // Dirty set currently contains loaded sections + phantom neighbors.
        let got = w.take_dirty([8.0, 8.0, 8.0], 10);
        assert_eq!(got, vec![sp(0, 0, 0), sp(2, 0, 0), sp(5, 0, 0)]);
        assert!(w.is_dirty_empty());

        // Budget respected; leftovers stay dirty.
        w.dirty.insert(sp(0, 0, 0));
        w.dirty.insert(sp(5, 0, 0));
        let got = w.take_dirty([8.0, 8.0, 8.0], 1);
        assert_eq!(got, vec![sp(0, 0, 0)]);
        assert!(w.dirty.contains(&sp(5, 0, 0)));
    }

    #[test]
    fn snapshot27_center_shell_and_light() {
        let mut w = WorldMirror::new();
        // Center: distinctive block at (1,2,3), sky light 5 / block light 9 there.
        let mut center = section_with(&[(idx(1, 2, 3), 42)]);
        let mut sky = [0u8; 2048];
        let mut blk = [0u8; 2048];
        let li = idx(1, 2, 3);
        sky[li / 2] |= if li.is_multiple_of(2) { 5 } else { 5 << 4 };
        blk[li / 2] |= if li.is_multiple_of(2) { 9 } else { 9 << 4 };
        center.sky_light = Some(Box::new(sky));
        center.block_light = Some(Box::new(blk));
        w.apply(&GameEvent::Section { pos: sp(0, 0, 0), data: center });

        // East neighbor: block 7 at its local x=0 face → padded x=16.
        let east = section_with(&[(idx(0, 4, 5), 7)]);
        w.apply(&GameEvent::Section { pos: sp(1, 0, 0), data: east });

        // Corner neighbor (-1,-1,-1): block 8 at its local (15,15,15) → padded (-1,-1,-1).
        let corner = section_with(&[(idx(15, 15, 15), 8)]);
        w.apply(&GameEvent::Section { pos: sp(-1, -1, -1), data: corner });

        let snap = w.snapshot27(sp(0, 0, 0)).expect("center loaded");
        assert_eq!(snap.pos, sp(0, 0, 0));
        assert_eq!(snap.get(1, 2, 3), 42);
        assert_eq!(snap.light_at(1, 2, 3), (5, 9));
        // Center has light arrays: other cells are (0, 0), not unknown.
        assert_eq!(snap.light_at(0, 0, 0), (0, 0));
        // Shell from the east neighbor.
        assert_eq!(snap.get(16, 4, 5), 7);
        // Corner shell cell.
        assert_eq!(snap.get(-1, -1, -1), 8);
        // Neighbors have no light arrays → unknown → (15, 0) fallback.
        assert_eq!(snap.light_at(16, 4, 5), (15, 0));
        assert_eq!(snap.light[PaddedSnapshot::idx(16, 4, 5)], 0xFF);
        // Missing neighbor (up): air + unknown light.
        assert_eq!(snap.get(5, 16, 5), 0);
        assert_eq!(snap.light[PaddedSnapshot::idx(5, 16, 5)], 0xFF);

        // Missing center → None.
        assert!(w.snapshot27(sp(9, 9, 9)).is_none());
    }

    #[test]
    fn snapshot27_dominant_biome() {
        let mut sec = SectionData::empty();
        for i in 0..64 {
            sec.biomes[i] = if i < 40 { 3 } else { 1 };
        }
        assert_eq!(dominant_biome(&sec), 3);
        let mut w = WorldMirror::new();
        w.apply(&GameEvent::Section { pos: sp(0, 0, 0), data: sec });
        assert_eq!(w.snapshot27(sp(0, 0, 0)).unwrap().biome, 3);
    }

    #[test]
    fn unload_far_chebyshev() {
        let mut w = WorldMirror::new();
        for x in 0..6 {
            w.apply(&GameEvent::Section { pos: sp(x, 0, 0), data: SectionData::empty() });
        }
        let mut gone = w.unload_far(ChunkPos { x: 0, z: 0 }, 2);
        gone.sort();
        assert_eq!(gone, vec![sp(3, 0, 0), sp(4, 0, 0), sp(5, 0, 0)]);
        assert_eq!(w.section_count(), 3);
        // Border survivor got re-dirtied.
        assert!(w.dirty.contains(&sp(2, 0, 0)));
    }

    #[test]
    fn raycast_axis_hits_and_entry_faces() {
        let mut w = WorldMirror::new();
        let sec = section_with(&[
            (idx(5, 8, 8), 1),
            (idx(8, 12, 8), 1),
            (idx(8, 8, 2), 1),
        ]);
        w.apply(&GameEvent::Section { pos: sp(0, 0, 0), data: sec });
        let air = |id: StateId| id == 0;

        // +x → hits west face.
        let (b, f) = w.raycast([0.5, 8.5, 8.5], [1.0, 0.0, 0.0], 10.0, air).unwrap();
        assert_eq!((b, f), (bp(5, 8, 8), Face::West));
        // -x → east face.
        let (b, f) = w.raycast([10.5, 8.5, 8.5], [-1.0, 0.0, 0.0], 10.0, air).unwrap();
        assert_eq!((b, f), (bp(5, 8, 8), Face::East));
        // +y → down face.
        let (b, f) = w.raycast([8.5, 8.5, 8.5], [0.0, 1.0, 0.0], 10.0, air).unwrap();
        assert_eq!((b, f), (bp(8, 12, 8), Face::Down));
        // -z → south face.
        let (b, f) = w.raycast([8.5, 8.5, 8.5], [0.0, 0.0, -1.0], 10.0, air).unwrap();
        assert_eq!((b, f), (bp(8, 8, 2), Face::South));

        // Out of range.
        assert!(w.raycast([0.5, 8.5, 8.5], [1.0, 0.0, 0.0], 3.0, air).is_none());
        // Zero direction, not inside a block.
        assert!(w.raycast([0.5, 8.5, 8.5], [0.0, 0.0, 0.0], 10.0, air).is_none());
        // Miss entirely.
        assert!(w.raycast([0.5, 0.5, 0.5], [0.0, 0.0, 1.0], 64.0, air).is_none());
    }

    #[test]
    fn raycast_starting_inside_and_diagonal() {
        let mut w = WorldMirror::new();
        let mut blocks: Vec<(usize, StateId)> = vec![(idx(3, 3, 3), 1)];
        blocks.push((idx(0, 0, 0), 2));
        let sec = section_with(&blocks);
        w.apply(&GameEvent::Section { pos: sp(0, 0, 0), data: sec });
        let air = |id: StateId| id == 0;

        // Start inside block (0,0,0): reported immediately, face opposes travel.
        let (b, f) = w.raycast([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 5.0, air).unwrap();
        assert_eq!((b, f), (bp(0, 0, 0), Face::West));

        // Diagonal from (1.5,1.5,1.5) toward (3.5,3.5,3.5): hits (3,3,3).
        let n = 1.0 / f64::sqrt(3.0);
        let (b, _f) = w
            .raycast([1.5, 1.5, 1.5], [n, n, n], 10.0, air)
            .unwrap();
        assert_eq!(b, bp(3, 3, 3));

        // Diagonal in negative direction from outside the section into the corner block.
        let (b, _f) = w
            .raycast([2.5, 2.5, 2.5], [-n, -n, -n], 10.0, |id| id != 2)
            .unwrap();
        assert_eq!(b, bp(0, 0, 0));
    }

    #[test]
    fn combined_light_encoding() {
        let mut sec = SectionData::empty();
        assert_eq!(combined_light(&sec, 0), 0xFF);
        let mut sky = [0u8; 2048];
        sky[0] = 0x0A; // idx 0 → sky 10
        sec.sky_light = Some(Box::new(sky));
        // Sky-only: block defaults to 0.
        assert_eq!(combined_light(&sec, 0), 0x0A);
        let mut blk = [0u8; 2048];
        blk[0] = 0x0C; // idx 0 → block 12
        sec.block_light = Some(Box::new(blk));
        assert_eq!(combined_light(&sec, 0), 0xCA);
        // Block-only: sky falls back to 15.
        sec.sky_light = None;
        assert_eq!(combined_light(&sec, 0), 0xCF);
    }

    #[test]
    fn get_block_yzx_and_default_air() {
        let mut w = WorldMirror::new();
        let sec = section_with(&[(idx(1, 2, 3), 77)]);
        w.apply(&GameEvent::Section { pos: sp(0, 0, 0), data: sec });
        assert_eq!(w.get_block(bp(1, 2, 3)), 77);
        assert_eq!(w.get_block(bp(3, 2, 1)), 0);
        assert_eq!(w.get_block(bp(-5, 200, 9)), 0);
        assert_eq!(SECTION_VOLUME, 4096);
    }
}
