//! Shared plain types — the vocabulary every module speaks.
//! See docs/rust-client/DESIGN.md for conventions (YZX indexing, camera-relative rendering).

use std::ops::{Index, IndexMut};

/// Vanilla global block state id (identical to azalea's `BlockState` id).
pub type StateId = u32;

pub const SECTION_SIZE: usize = 16;
pub const SECTION_VOLUME: usize = 16 * 16 * 16;
/// Padded snapshot edge (one block of neighbor context on each side).
pub const PADDED_SIZE: usize = 18;
pub const PADDED_VOLUME: usize = PADDED_SIZE * PADDED_SIZE * PADDED_SIZE;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct SectionPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl SectionPos {
    pub fn of_block(x: i32, y: i32, z: i32) -> Self {
        Self { x: x >> 4, y: y >> 4, z: z >> 4 }
    }
    pub fn chunk(self) -> ChunkPos {
        ChunkPos { x: self.x, z: self.z }
    }
    /// World-space origin of this section (min corner).
    pub fn origin(self) -> [f64; 3] {
        [self.x as f64 * 16.0, self.y as f64 * 16.0, self.z as f64 * 16.0]
    }
    pub fn center(self) -> [f64; 3] {
        let o = self.origin();
        [o[0] + 8.0, o[1] + 8.0, o[2] + 8.0]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub fn section(self) -> SectionPos {
        SectionPos::of_block(self.x, self.y, self.z)
    }
    /// Index within its section, YZX order.
    pub fn section_index(self) -> usize {
        let (lx, ly, lz) = (
            (self.x & 15) as usize,
            (self.y & 15) as usize,
            (self.z & 15) as usize,
        );
        (ly * 16 + lz) * 16 + lx
    }
    pub fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self { x: self.x + dx, y: self.y + dy, z: self.z + dz }
    }
}

/// The six axis-aligned directions, vanilla order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum Face {
    Down = 0,
    Up = 1,
    North = 2, // -z
    South = 3, // +z
    West = 4,  // -x
    East = 5,  // +x
}

impl Face {
    pub const ALL: [Face; 6] = [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East];
    pub fn normal(self) -> [i32; 3] {
        match self {
            Face::Down => [0, -1, 0],
            Face::Up => [0, 1, 0],
            Face::North => [0, 0, -1],
            Face::South => [0, 0, 1],
            Face::West => [-1, 0, 0],
            Face::East => [1, 0, 0],
        }
    }
    pub fn opposite(self) -> Face {
        match self {
            Face::Down => Face::Up,
            Face::Up => Face::Down,
            Face::North => Face::South,
            Face::South => Face::North,
            Face::West => Face::East,
            Face::East => Face::West,
        }
    }
    /// Vanilla directional face shade (up 1.0, down 0.5, z 0.8, x 0.6).
    pub fn shade(self) -> f32 {
        match self {
            Face::Up => 1.0,
            Face::Down => 0.5,
            Face::North | Face::South => 0.8,
            Face::West | Face::East => 0.6,
        }
    }
    pub fn from_name(s: &str) -> Option<Face> {
        Some(match s {
            "down" | "bottom" => Face::Down,
            "up" | "top" => Face::Up,
            "north" => Face::North,
            "south" => Face::South,
            "west" => Face::West,
            "east" => Face::East,
            _ => return None,
        })
    }
}

/// Copy of one section's contents as sent by the bridge.
#[derive(Clone)]
pub struct SectionData {
    /// 4096 states, YZX order.
    pub blocks: Box<[StateId; SECTION_VOLUME]>,
    /// 2048-byte nibble arrays (vanilla layout: nibble i = block index i, low nibble first).
    pub sky_light: Option<Box<[u8; 2048]>>,
    pub block_light: Option<Box<[u8; 2048]>>,
    /// 4×4×4 biome ids (azalea/vanilla biome registry ids), YZX order.
    pub biomes: Box<[u32; 64]>,
}

impl SectionData {
    pub fn empty() -> Self {
        Self {
            blocks: vec![0u32; SECTION_VOLUME].into_boxed_slice().try_into().unwrap(),
            sky_light: None,
            block_light: None,
            biomes: vec![0u32; 64].into_boxed_slice().try_into().unwrap(),
        }
    }
}

#[inline]
pub fn nibble(arr: &[u8; 2048], idx: usize) -> u8 {
    let b = arr[idx / 2];
    if idx.is_multiple_of(2) { b & 0xF } else { b >> 4 }
}

/// 18³ snapshot: the section plus one block of context on every side.
/// Index with `[x+1][y+1][z+1]` style local coords via `get(x, y, z)`
/// where x/y/z ∈ -1..=16.
pub struct PaddedSnapshot {
    pub pos: SectionPos,
    pub blocks: Box<[StateId; PADDED_VOLUME]>,
    /// Combined light per padded cell: low nibble sky, high nibble block. 0xFF = unknown (fallback to fullbright sky).
    pub light: Box<[u8; PADDED_VOLUME]>,
    /// Biome of the section itself (approx: one biome per section for tinting v1 — dominant biome id).
    pub biome: u32,
}

impl PaddedSnapshot {
    #[inline]
    pub fn idx(x: i32, y: i32, z: i32) -> usize {
        debug_assert!((-1..=16).contains(&x) && (-1..=16).contains(&y) && (-1..=16).contains(&z));
        (((y + 1) as usize) * PADDED_SIZE + ((z + 1) as usize)) * PADDED_SIZE + ((x + 1) as usize)
    }
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> StateId {
        self.blocks[Self::idx(x, y, z)]
    }
    /// (sky, block) light at padded cell; (15, 0) when unknown.
    #[inline]
    pub fn light_at(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        let v = self.light[Self::idx(x, y, z)];
        if v == 0xFF { (15, 0) } else { (v & 0xF, v >> 4) }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum RenderLayer {
    Opaque = 0,
    Cutout = 1,
    Translucent = 2,
}

impl RenderLayer {
    pub const ALL: [RenderLayer; 3] = [RenderLayer::Opaque, RenderLayer::Cutout, RenderLayer::Translucent];
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug)]
pub struct MeshVertex {
    /// Relative to section origin.
    pub pos: [f32; 3],
    /// Normalized atlas UV.
    pub uv: [f32; 2],
    /// rgb tint (255,255,255 = none), a unused.
    pub color: [u8; 4],
    /// [sky 0-15, block 0-15, shade 0-255, ao 0-255]
    pub light: [u8; 4],
}

/// CPU-side mesh for one section, split by layer. Indices are u32 into `vertices`.
#[derive(Default)]
pub struct LayerMesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

impl LayerMesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
    /// Push a quad (4 verts, CCW when viewed from outside) as two triangles.
    pub fn push_quad(&mut self, verts: [MeshVertex; 4]) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&verts);
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

pub struct MeshData {
    pub pos: SectionPos,
    pub layers: [LayerMesh; 3],
}

impl Index<RenderLayer> for MeshData {
    type Output = LayerMesh;
    fn index(&self, l: RenderLayer) -> &LayerMesh {
        &self.layers[l as usize]
    }
}
impl IndexMut<RenderLayer> for MeshData {
    fn index_mut(&mut self, l: RenderLayer) -> &mut LayerMesh {
        &mut self.layers[l as usize]
    }
}

impl MeshData {
    pub fn new(pos: SectionPos) -> Self {
        Self { pos, layers: [LayerMesh::default(), LayerMesh::default(), LayerMesh::default()] }
    }
    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(|l| l.is_empty())
    }
}

/// Fallback tint colors (plains biome), used before the biome table arrives and
/// for biome ids the server didn't send.
pub mod tint {
    pub const GRASS: [u8; 3] = [0x91, 0xBD, 0x59];
    pub const FOLIAGE: [u8; 3] = [0x77, 0xAB, 0x2F];
    pub const WATER: [u8; 3] = [0x3F, 0x76, 0xE4];
    pub const NONE: [u8; 3] = [0xFF, 0xFF, 0xFF];
}

/// Per-biome grass / foliage / water tint colors, indexed by protocol biome id.
/// Built from the server biome registry + the grass/foliage colormaps. Shared
/// with the rayon meshing threads via `Arc`; the mesher looks colors up by the
/// snapshot's dominant biome id.
#[derive(Clone, Debug, Default)]
pub struct BiomeTints {
    /// `[grass, foliage, water]` per biome id.
    tints: Vec<[[u8; 3]; 3]>,
}

impl BiomeTints {
    pub fn from_rows(tints: Vec<[[u8; 3]; 3]>) -> Self {
        Self { tints }
    }
    #[inline]
    pub fn grass(&self, id: u32) -> [u8; 3] {
        self.tints.get(id as usize).map_or(tint::GRASS, |t| t[0])
    }
    #[inline]
    pub fn foliage(&self, id: u32) -> [u8; 3] {
        self.tints.get(id as usize).map_or(tint::FOLIAGE, |t| t[1])
    }
    #[inline]
    pub fn water(&self, id: u32) -> [u8; 3] {
        self.tints.get(id as usize).map_or(tint::WATER, |t| t[2])
    }
    pub fn is_empty(&self) -> bool {
        self.tints.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_index_yzx() {
        let p = BlockPos { x: 1, y: 2, z: 3 };
        assert_eq!(p.section_index(), (2 * 16 + 3) * 16 + 1);
        let neg = BlockPos { x: -1, y: -1, z: -1 };
        assert_eq!(neg.section_index(), (15 * 16 + 15) * 16 + 15);
        assert_eq!(neg.section(), SectionPos { x: -1, y: -1, z: -1 });
    }

    #[test]
    fn padded_idx_bounds() {
        assert_eq!(PaddedSnapshot::idx(-1, -1, -1), 0);
        assert_eq!(PaddedSnapshot::idx(16, 16, 16), PADDED_VOLUME - 1);
    }

    #[test]
    fn nibble_order() {
        let mut arr = [0u8; 2048];
        arr[0] = 0xBA; // idx0 = A (low), idx1 = B (high)
        assert_eq!(nibble(&arr, 0), 0xA);
        assert_eq!(nibble(&arr, 1), 0xB);
    }
}
