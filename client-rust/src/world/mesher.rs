//! Section meshing: PaddedSnapshot + BakedModelStore → MeshData.
//! Pure function, runs on rayon threads. No locks, no allocation reuse needed v1.
//!
//! Per non-air block:
//! - Fluid states (water/lava via `BlockTable::fluid_kind`) and waterlogged
//!   blocks: emit the fluid box (14/16 height when the block above isn't the
//!   same fluid, else full height; still texture; water → Translucent +
//!   TintKind::Water tint, lava → Opaque, fullbright block light 15).
//!   Fluid faces cull against same-fluid neighbors and occluding solids.
//!   Waterlogged blocks emit fluid box PLUS their model quads.
//! - Model quads: skip quad if `cull=Some(d)` and neighbor at d occludes
//!   (`store.occludes`), except: never cull between two DIFFERENT translucent
//!   states; DO cull between identical states for glass-like blocks (same id).
//! - Light: sample the padded cell the face points into (pos + normal) for
//!   axis-aligned border quads; interior quads sample the block itself.
//! - AO (Up/Down/N/S/W/E full faces only): classic 3-neighbor corner test →
//!   ao byte 255/204/153/102 per vertex. Non-full quads: ao=255.
//! - shade byte: `Face::shade() * 255`.
//! - tint: TintKind → types::tint constants (v1 constant biome colors).
//! - Vertex positions: model unit coords + in-section block offset (f32).

use crate::assets::blockmap::BlockTable;
use crate::models::{BakedModelStore, BakedQuad, TintKind};
use crate::types::{Face, MeshData, MeshVertex, PaddedSnapshot, RenderLayer, StateId, tint};

const EPS: f32 = 1e-4;
/// Fluid surface height when the block above is not the same fluid.
const FLUID_SURFACE: f32 = 14.0 / 16.0;

pub fn mesh_section(
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    table: &BlockTable,
) -> MeshData {
    let mut mesh = MeshData::new(snap.pos);
    let fluid_uvs = FluidUvs { water: store.water_still_uv(), lava: store.lava_still_uv() };

    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                let id = snap.get(x, y, z);
                if table.is_air(id) {
                    continue;
                }
                if let Some(kind) = fluid_at(table, id) {
                    emit_fluid(&mut mesh, snap, store, table, &fluid_uvs, (x, y, z), kind);
                    if table.fluid_kind(id).is_some() {
                        continue; // pure fluid state: no block model
                    }
                    // waterlogged: fall through and emit the model too
                }
                emit_model(&mut mesh, snap, store, (x, y, z), id);
            }
        }
    }
    mesh
}

// ---------------------------------------------------------------------------
// Fluids
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FluidKind {
    Water,
    Lava,
}

/// The fluid occupying this state's cell, if any (fluid blocks + waterlogged).
fn fluid_at(table: &BlockTable, id: StateId) -> Option<FluidKind> {
    match table.fluid_kind(id) {
        Some("water") => Some(FluidKind::Water),
        Some("lava") => Some(FluidKind::Lava),
        Some(other) => {
            tracing::warn!("unknown fluid kind {other:?} for state {id}; treating as water");
            Some(FluidKind::Water)
        }
        None => {
            if table.entry(id).is_some_and(|e| e.is_waterlogged()) {
                Some(FluidKind::Water)
            } else {
                None
            }
        }
    }
}

/// Fluid sprite UVs. The atlas is not reachable from the mesher yet (see
/// contract note): default is a degenerate rect so geometry/culling/lighting
/// work now and the integrator swaps real `block/water_still` /
/// `block/lava_still` sprite rects in this one place.
/// Corner order: (0,0), (1,0), (1,1), (0,1) in face-local (s, t).
struct FluidUvs {
    water: [[f32; 2]; 4],
    lava: [[f32; 2]; 4],
}

impl Default for FluidUvs {
    fn default() -> Self {
        let degenerate = [[0.0, 0.0], [0.001, 0.0], [0.001, 0.001], [0.0, 0.001]];
        Self { water: degenerate, lava: degenerate }
    }
}

/// Bilinear interpolation across a 4-corner UV rect ((0,0),(1,0),(1,1),(0,1)).
fn bilerp(rect: &[[f32; 2]; 4], s: f32, t: f32) -> [f32; 2] {
    let lerp2 = |a: [f32; 2], b: [f32; 2], k: f32| [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k];
    let top = lerp2(rect[0], rect[1], s);
    let bot = lerp2(rect[3], rect[2], s);
    lerp2(top, bot, t)
}

/// Height of the fluid box in this cell.
fn fluid_height(same_fluid_above: bool) -> f32 {
    if same_fluid_above { 1.0 } else { FLUID_SURFACE }
}

/// A fluid face is dropped against same-fluid neighbors and occluding solids.
fn fluid_face_culled(neighbor_same_fluid: bool, neighbor_occludes: bool) -> bool {
    neighbor_same_fluid || neighbor_occludes
}

/// Unit-box corners for `face` of a fluid box (0,0,0)..(1,h,1), CCW from
/// outside, triangulated 0-1-2 / 0-2-3 by `push_quad`.
fn fluid_face_corners(face: Face, h: f32) -> [[f32; 3]; 4] {
    match face {
        Face::Up => [[0.0, h, 0.0], [0.0, h, 1.0], [1.0, h, 1.0], [1.0, h, 0.0]],
        Face::Down => [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
        Face::North => [[0.0, 0.0, 0.0], [0.0, h, 0.0], [1.0, h, 0.0], [1.0, 0.0, 0.0]],
        Face::South => [[1.0, 0.0, 1.0], [1.0, h, 1.0], [0.0, h, 1.0], [0.0, 0.0, 1.0]],
        Face::West => [[0.0, 0.0, 1.0], [0.0, h, 1.0], [0.0, h, 0.0], [0.0, 0.0, 0.0]],
        Face::East => [[1.0, 0.0, 0.0], [1.0, h, 0.0], [1.0, h, 1.0], [1.0, 0.0, 1.0]],
    }
}

/// Face-local (s, t) texture coordinates for a corner of a fluid face.
fn fluid_face_st(face: Face, c: [f32; 3]) -> (f32, f32) {
    match face {
        Face::Up | Face::Down => (c[0], c[2]),
        Face::North | Face::South => (c[0], 1.0 - c[1]),
        Face::West | Face::East => (c[2], 1.0 - c[1]),
    }
}

fn emit_fluid(
    mesh: &mut MeshData,
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    table: &BlockTable,
    uvs: &FluidUvs,
    (x, y, z): (i32, i32, i32),
    kind: FluidKind,
) {
    let same_above = fluid_at(table, snap.get(x, y + 1, z)) == Some(kind);
    let h = fluid_height(same_above);
    let (layer, rgb, rect) = match kind {
        FluidKind::Water => (RenderLayer::Translucent, tint::WATER, &uvs.water),
        FluidKind::Lava => (RenderLayer::Opaque, tint::NONE, &uvs.lava),
    };

    for face in Face::ALL {
        let n = face.normal();
        let (nx, ny, nz) = (x + n[0], y + n[1], z + n[2]);
        let nid = snap.get(nx, ny, nz);
        let same = fluid_at(table, nid) == Some(kind);
        if fluid_face_culled(same, store.occludes(nid, face.opposite())) {
            continue;
        }

        // Border faces light from the neighbor cell; a lowered top surface
        // (h < 1) is interior and lights from the fluid's own cell.
        let interior_top = face == Face::Up && !same_above;
        let (sky, mut blk) =
            if interior_top { snap.light_at(x, y, z) } else { snap.light_at(nx, ny, nz) };
        if kind == FluidKind::Lava {
            blk = 15;
        }
        let shade = (face.shade() * 255.0) as u8;
        let corners = fluid_face_corners(face, h);
        let verts = corners.map(|c| {
            let (s, t) = fluid_face_st(face, c);
            MeshVertex {
                pos: [c[0] + x as f32, c[1] + y as f32, c[2] + z as f32],
                uv: bilerp(rect, s, t),
                color: [rgb[0], rgb[1], rgb[2], 255],
                light: [sky, blk, shade, 255],
            }
        });
        mesh[layer].push_quad(verts);
    }
}

// ---------------------------------------------------------------------------
// Block models
// ---------------------------------------------------------------------------

/// Cull decision for a model quad with `cull=Some(d)` against the neighbor at d.
/// Translucent quads additionally cull against the identical state (adjacent
/// glass/ice merge) but never against *different* translucent states (which
/// never occlude anyway).
fn model_quad_culled(
    layer: RenderLayer,
    own_id: StateId,
    neighbor_id: StateId,
    neighbor_occludes: bool,
) -> bool {
    if layer == RenderLayer::Translucent && neighbor_id == own_id {
        return true;
    }
    neighbor_occludes
}

/// Tangent axis indices (0=x, 1=y, 2=z) of the plane perpendicular to `face`.
fn tangent_axes(face: Face) -> (usize, usize) {
    match face {
        Face::Up | Face::Down => (0, 2),
        Face::North | Face::South => (0, 1),
        Face::West | Face::East => (2, 1),
    }
}

/// Coordinate value of the border plane `face` sits on (0.0 or 1.0), and the
/// axis it is constant in.
fn face_plane(face: Face) -> (usize, f32) {
    match face {
        Face::Down => (1, 0.0),
        Face::Up => (1, 1.0),
        Face::North => (2, 0.0),
        Face::South => (2, 1.0),
        Face::West => (0, 0.0),
        Face::East => (0, 1.0),
    }
}

/// All four verts lie on the block-border plane of `face`.
fn quad_on_border(face: Face, verts: &[[f32; 3]; 4]) -> bool {
    let (axis, plane) = face_plane(face);
    verts.iter().all(|v| (v[axis] - plane).abs() < EPS)
}

/// Border quad that covers the entire face (0..1 in both tangent axes).
fn quad_full_face(face: Face, verts: &[[f32; 3]; 4]) -> bool {
    if !quad_on_border(face, verts) {
        return false;
    }
    let (a0, a1) = tangent_axes(face);
    for a in [a0, a1] {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for v in verts {
            min = min.min(v[a]);
            max = max.max(v[a]);
        }
        if min > EPS || max < 1.0 - EPS {
            return false;
        }
    }
    true
}

/// Classic AO corner level: 0 (darkest) .. 3 (unoccluded). When both sides are
/// occluded the corner is fully dark regardless of the corner sample.
fn ao_level(side1: bool, side2: bool, corner: bool) -> u8 {
    if side1 && side2 {
        0
    } else {
        3 - (side1 as u8 + side2 as u8 + corner as u8)
    }
}

fn ao_byte(level: u8) -> u8 {
    match level {
        0 => 102,
        1 => 153,
        2 => 204,
        _ => 255,
    }
}

/// Per-vertex AO for a full border face of block (x,y,z). Samples the 8 cells
/// around the cell the face points into; occluder = `store.occludes(state, face)`.
fn ao_for_quad(
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    face: Face,
    verts: &[[f32; 3]; 4],
    (x, y, z): (i32, i32, i32),
) -> [u8; 4] {
    let n = face.normal();
    let base = [x + n[0], y + n[1], z + n[2]];
    let (a0, a1) = tangent_axes(face);
    let occ = |d: [i32; 3]| -> bool {
        store.occludes(snap.get(base[0] + d[0], base[1] + d[1], base[2] + d[2]), face)
    };
    let mut out = [255u8; 4];
    for (i, v) in verts.iter().enumerate() {
        let ds = if v[a0] < 0.5 { -1 } else { 1 };
        let dt = if v[a1] < 0.5 { -1 } else { 1 };
        let mut o1 = [0i32; 3];
        o1[a0] = ds;
        let mut o2 = [0i32; 3];
        o2[a1] = dt;
        let side1 = occ(o1);
        let side2 = occ(o2);
        let corner = occ([o1[0] + o2[0], o1[1] + o2[1], o1[2] + o2[2]]);
        out[i] = ao_byte(ao_level(side1, side2, corner));
    }
    out
}

fn tint_color(t: Option<TintKind>) -> [u8; 3] {
    match t {
        None => tint::NONE,
        Some(TintKind::Grass) => tint::GRASS,
        Some(TintKind::Foliage) => tint::FOLIAGE,
        Some(TintKind::Water) => tint::WATER,
    }
}

fn emit_model(
    mesh: &mut MeshData,
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    (x, y, z): (i32, i32, i32),
    id: StateId,
) {
    let model = store.get(id);
    for quad in &model.quads {
        if let Some(d) = quad.cull {
            let dn = d.normal();
            let nid = snap.get(x + dn[0], y + dn[1], z + dn[2]);
            if model_quad_culled(quad.layer, id, nid, store.occludes(nid, d.opposite())) {
                continue;
            }
        }
        emit_quad(mesh, snap, store, (x, y, z), quad);
    }
}

fn emit_quad(
    mesh: &mut MeshData,
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    (x, y, z): (i32, i32, i32),
    quad: &BakedQuad,
) {
    let border = quad_on_border(quad.face, &quad.verts);
    let (sky, blk) = if border {
        let n = quad.face.normal();
        snap.light_at(x + n[0], y + n[1], z + n[2])
    } else {
        snap.light_at(x, y, z)
    };
    let shade = (quad.face.shade() * 255.0) as u8;
    let ao = if border && quad_full_face(quad.face, &quad.verts) {
        ao_for_quad(snap, store, quad.face, &quad.verts, (x, y, z))
    } else {
        [255u8; 4]
    };
    let rgb = tint_color(quad.tint);

    let mut verts = [MeshVertex {
        pos: [0.0; 3],
        uv: [0.0; 2],
        color: [255; 4],
        light: [0; 4],
    }; 4];
    for i in 0..4 {
        verts[i] = MeshVertex {
            pos: [
                quad.verts[i][0] + x as f32,
                quad.verts[i][1] + y as f32,
                quad.verts[i][2] + z as f32,
            ],
            uv: quad.uvs[i],
            color: [rgb[0], rgb[1], rgb[2], 255],
            light: [sky, blk, shade, ao[i]],
        };
    }
    mesh[quad.layer].push_quad(verts);
}

// ---------------------------------------------------------------------------
// Tests (pure decision logic only — BakedModelStore is not constructible here)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ao_levels_and_bytes() {
        // Free corner.
        assert_eq!(ao_level(false, false, false), 3);
        // Single occluder.
        assert_eq!(ao_level(true, false, false), 2);
        assert_eq!(ao_level(false, true, false), 2);
        assert_eq!(ao_level(false, false, true), 2);
        // Two occluders.
        assert_eq!(ao_level(true, false, true), 1);
        assert_eq!(ao_level(false, true, true), 1);
        // Both sides → darkest even with a free corner.
        assert_eq!(ao_level(true, true, false), 0);
        assert_eq!(ao_level(true, true, true), 0);

        assert_eq!(ao_byte(3), 255);
        assert_eq!(ao_byte(2), 204);
        assert_eq!(ao_byte(1), 153);
        assert_eq!(ao_byte(0), 102);
    }

    #[test]
    fn fluid_height_and_cull() {
        assert_eq!(fluid_height(true), 1.0);
        assert_eq!(fluid_height(false), FLUID_SURFACE);

        assert!(fluid_face_culled(true, false));
        assert!(fluid_face_culled(false, true));
        assert!(fluid_face_culled(true, true));
        assert!(!fluid_face_culled(false, false));
    }

    #[test]
    fn translucent_cull_rule() {
        // Opaque/cutout quads: only the occlusion test matters.
        assert!(model_quad_culled(RenderLayer::Opaque, 1, 2, true));
        assert!(!model_quad_culled(RenderLayer::Opaque, 1, 1, false));
        // Translucent: same id culls even without occlusion...
        assert!(model_quad_culled(RenderLayer::Translucent, 5, 5, false));
        // ...different id doesn't (unless a solid occluder).
        assert!(!model_quad_culled(RenderLayer::Translucent, 5, 6, false));
        assert!(model_quad_culled(RenderLayer::Translucent, 5, 6, true));
    }

    #[test]
    fn border_and_full_face_detection() {
        let full_up = fluid_face_corners(Face::Up, 1.0);
        assert!(quad_on_border(Face::Up, &full_up));
        assert!(quad_full_face(Face::Up, &full_up));

        // Lowered fluid surface is not on the border.
        let low_up = fluid_face_corners(Face::Up, FLUID_SURFACE);
        assert!(!quad_on_border(Face::Up, &low_up));
        assert!(!quad_full_face(Face::Up, &low_up));

        // Side of a lowered fluid box touches the border plane but isn't full.
        let short_north = fluid_face_corners(Face::North, FLUID_SURFACE);
        assert!(quad_on_border(Face::North, &short_north));
        assert!(!quad_full_face(Face::North, &short_north));

        for f in Face::ALL {
            let c = fluid_face_corners(f, 1.0);
            assert!(quad_full_face(f, &c), "{f:?} full box face should be full");
        }

        // A half quad (slab top half missing) is on-border but not full.
        let half: [[f32; 3]; 4] =
            [[0.0, 0.0, 0.0], [0.0, 0.5, 0.0], [1.0, 0.5, 0.0], [1.0, 0.0, 0.0]];
        assert!(quad_on_border(Face::North, &half));
        assert!(!quad_full_face(Face::North, &half));
    }

    #[test]
    fn fluid_corners_wind_outward() {
        // Cross product of the first triangle's edges must point along the
        // face normal (CCW seen from outside).
        for f in Face::ALL {
            for h in [1.0f32, FLUID_SURFACE] {
                let c = fluid_face_corners(f, h);
                let e1 = [c[1][0] - c[0][0], c[1][1] - c[0][1], c[1][2] - c[0][2]];
                let e2 = [c[2][0] - c[0][0], c[2][1] - c[0][1], c[2][2] - c[0][2]];
                let cross = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                let n = f.normal();
                let dot =
                    cross[0] * n[0] as f32 + cross[1] * n[1] as f32 + cross[2] * n[2] as f32;
                assert!(dot > 0.0, "{f:?} h={h} winds inward (dot {dot})");
            }
        }
    }

    #[test]
    fn bilerp_rect_corners() {
        let rect = [[0.0, 0.0], [0.5, 0.0], [0.5, 0.25], [0.0, 0.25]];
        assert_eq!(bilerp(&rect, 0.0, 0.0), [0.0, 0.0]);
        assert_eq!(bilerp(&rect, 1.0, 0.0), [0.5, 0.0]);
        assert_eq!(bilerp(&rect, 1.0, 1.0), [0.5, 0.25]);
        assert_eq!(bilerp(&rect, 0.0, 1.0), [0.0, 0.25]);
        assert_eq!(bilerp(&rect, 0.5, 0.5), [0.25, 0.125]);
    }

    #[test]
    fn fluid_st_ranges() {
        for f in Face::ALL {
            for c in fluid_face_corners(f, FLUID_SURFACE) {
                let (s, t) = fluid_face_st(f, c);
                assert!((0.0..=1.0).contains(&s), "{f:?} s={s}");
                assert!((0.0..=1.0).contains(&t), "{f:?} t={t}");
            }
        }
    }

    #[test]
    fn shade_bytes() {
        assert_eq!((Face::Up.shade() * 255.0) as u8, 255);
        assert_eq!((Face::Down.shade() * 255.0) as u8, 127);
        assert_eq!((Face::North.shade() * 255.0) as u8, 204);
        assert_eq!((Face::East.shade() * 255.0) as u8, 153);
    }

    #[test]
    fn tangent_axes_perpendicular() {
        for f in Face::ALL {
            let (a0, a1) = tangent_axes(f);
            let (plane_axis, _) = face_plane(f);
            assert_ne!(a0, a1);
            assert_ne!(a0, plane_axis);
            assert_ne!(a1, plane_axis);
        }
    }
}
