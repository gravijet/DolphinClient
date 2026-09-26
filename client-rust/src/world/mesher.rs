//! Section meshing: PaddedSnapshot + BakedModelStore → MeshData.
//! Pure function, runs on rayon threads. No locks, no allocation reuse needed v1.
//!
//! Per non-air block:
//! - Fluid states (water/lava via `BlockTable::fluid_kind`) and waterlogged
//!   blocks: vanilla's liquid renderer — the surface is a *sloped* quad whose
//!   four corner heights are the weighted average of the neighbouring fluid
//!   levels, the top face uses the flow sprite rotated into the flow direction
//!   (still sprite when the fluid isn't moving), and every side uses the flow
//!   sprite at half scale so waterfalls run downwards. Water → Translucent +
//!   TintKind::Water tint, lava → Opaque, fullbright block light 15.
//!   Fluid faces cull against same-fluid neighbors and occluding solids.
//!   Waterlogged blocks emit fluid box PLUS their model quads.
//! - Model quads: skip quad if `cull=Some(d)` and neighbor at d occludes
//!   (`store.occludes`), except: never cull between two DIFFERENT translucent
//!   states; DO cull between identical states for glass-like blocks (same id).
//! - Light: sample the padded cell the face points into (pos + normal) for
//!   axis-aligned border quads; interior quads sample the block itself. With
//!   smooth lighting on, a full border face instead averages the four cells
//!   touching each vertex, so light fades across a face instead of stepping.
//!   Levels leave here scaled 0..255 (15 → 255) so those averages survive.
//! - AO (Up/Down/N/S/W/E full faces only): classic 3-neighbor corner test →
//!   ao byte 255/204/153/102 per vertex. Non-full quads: ao=255.
//! - shade byte: `Face::shade() * 255`.
//! - tint: TintKind → the real biome color, box-averaged over the surrounding
//!   4×4×4 cells so tints fade smoothly across biome borders (see `cell_tint`).
//! - Vertex positions: model unit coords + in-section block offset (f32).

use crate::assets::blockmap::BlockTable;
use crate::models::bake::{FluidSprites, SpriteRect};
use crate::models::{BakedModelStore, BakedQuad, TintKind};
use crate::types::{
    BIOME_CELL_PADDED_VOLUME, BiomeTints, Face, MeshData, MeshVertex, PADDED_VOLUME, PaddedSnapshot,
    RenderLayer, SectionPos, StateId, tint,
};

const EPS: f32 = 1e-4;
/// Vanilla's full-amount fluid surface height (8/9 of a block).
const FLUID_SURFACE: f32 = 8.0 / 9.0;

/// Grass/foliage/water tint colors for one 4×4×4 biome cell, already box-averaged
/// with its neighbors (see `cell_tint`) — vanilla's `biomeBlendRadius` smoothing,
/// approximated at this engine's coarser per-cell (rather than per-block) biome
/// data granularity.
#[derive(Clone, Copy)]
struct SectionTint {
    grass: [u8; 3],
    foliage: [u8; 3],
    water: [u8; 3],
}

/// One blended `SectionTint` per local 4×4×4 cell (YZX, matching `SectionData::biomes`).
type CellTints = [SectionTint; 64];

/// Box-average the biome color at cell `(cx, cy, cz)` (0..=3, this section's own
/// grid) over its full 3×3×3 cell neighborhood — vanilla samples a `(2r+1)²`
/// column of blocks (`r` = `biomeBlendRadius`, default 2) and averages resolved
/// colors, not ids; this does the same over the cells this engine actually has
/// data for; one cell is 4 blocks wide, so a 1-cell radius covers a similar-sized
/// neighborhood to vanilla's real block-radius-2 blend.
fn cell_tint(snap: &PaddedSnapshot, biome_tints: &BiomeTints, (cx, cy, cz): (i32, i32, i32)) -> SectionTint {
    let mut grass = [0u32; 3];
    let mut foliage = [0u32; 3];
    let mut water = [0u32; 3];
    let mut n = 0u32;
    for dy in -1..=1 {
        for dz in -1..=1 {
            for dx in -1..=1 {
                let id = snap.biome_cell(cx + dx, cy + dy, cz + dz);
                let g = biome_tints.grass(id);
                let f = biome_tints.foliage(id);
                let w = biome_tints.water(id);
                for c in 0..3 {
                    grass[c] += g[c] as u32;
                    foliage[c] += f[c] as u32;
                    water[c] += w[c] as u32;
                }
                n += 1;
            }
        }
    }
    SectionTint {
        grass: [(grass[0] / n) as u8, (grass[1] / n) as u8, (grass[2] / n) as u8],
        foliage: [(foliage[0] / n) as u8, (foliage[1] / n) as u8, (foliage[2] / n) as u8],
        water: [(water[0] / n) as u8, (water[1] / n) as u8, (water[2] / n) as u8],
    }
}

/// Precompute the blended tint for every one of this section's 64 cells, once
/// per mesh build (each cell's own 27-sample average is then just an array read
/// per block, not recomputed per quad).
fn build_cell_tints(snap: &PaddedSnapshot, biome_tints: &BiomeTints) -> CellTints {
    let mut out = [SectionTint { grass: [0; 3], foliage: [0; 3], water: [0; 3] }; 64];
    for cy in 0..4i32 {
        for cz in 0..4i32 {
            for cx in 0..4i32 {
                let (ux, uy, uz) = (cx as usize, cy as usize, cz as usize);
                out[(uy * 4 + uz) * 4 + ux] = cell_tint(snap, biome_tints, (cx, cy, cz));
            }
        }
    }
    out
}

/// The blended tint covering block `(x, y, z)` (0..=15, this section's own coords).
#[inline]
fn tint_at(cell_tints: &CellTints, (x, y, z): (i32, i32, i32)) -> SectionTint {
    let (cx, cy, cz) = ((x / 4) as usize, (y / 4) as usize, (z / 4) as usize);
    cell_tints[(cy * 4 + cz) * 4 + cx]
}

/// A light level (0..=15, possibly fractional after smoothing) as the byte the
/// shader samples the light texture with. 15 → 255.
#[inline]
pub fn light_byte(level: f32) -> u8 {
    (level * 17.0 + 0.5).clamp(0.0, 255.0) as u8
}

pub fn mesh_section(
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    table: &BlockTable,
    biome_tints: &BiomeTints,
    smooth_lighting: bool,
) -> MeshData {
    let mut mesh = MeshData::new(snap.pos);
    let fluid_uvs = store.fluids();
    let cell_tints = build_cell_tints(snap, biome_tints);

    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                let id = snap.get(x, y, z);
                if table.is_air(id) {
                    continue;
                }
                // Chests and shulker boxes move: the app draws them per frame,
                // so all the mesh carries is where they are.
                if let Some(dynamic) = store.dyn_block(id) {
                    mesh.dyn_be.push((
                        crate::types::BlockPos {
                            x: snap.pos.x * 16 + x as i32,
                            y: snap.pos.y * 16 + y as i32,
                            z: snap.pos.z * 16 + z as i32,
                        },
                        id,
                    ));
                    // An enchanting table still has a table to draw; a chest
                    // has nothing but what the client draws itself.
                    if dynamic.replaces_model() {
                        continue;
                    }
                }
                if let Some(kind) = fluid_at(table, id) {
                    emit_fluid(&mut mesh, snap, store, table, fluid_uvs, &cell_tints, (x, y, z), kind);
                    if table.fluid_kind(id).is_some() {
                        continue; // pure fluid state: no block model
                    }
                    // waterlogged: fall through and emit the model too
                }
                if is_end_portal(table, id) {
                    emit_end_portal(&mut mesh, store, table, (x, y, z), id);
                    continue;
                }
                emit_model(&mut mesh, snap, store, &cell_tints, (x, y, z), id, smooth_lighting);
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
            if table.contains_water(id) {
                Some(FluidKind::Water)
            } else {
                None
            }
        }
    }
}

/// Vanilla fluid *amount* (0..8, 8 = a full source) for a fluid state, from the
/// block state's `level` property. `level=0` is a source; 1..7 are the flowing
/// steps (higher = shallower); the 8..15 range is *falling* fluid, which fills
/// its cell. A waterlogged block carries a full water amount.
fn fluid_amount(table: &BlockTable, id: StateId) -> u8 {
    let Some(e) = table.entry(id) else { return 8 };
    match e.prop("level").and_then(|l| l.parse::<u8>().ok()) {
        Some(0) => 8,
        Some(l) if l < 8 => 8 - l,
        Some(_) => 8, // falling: fills the cell
        None => 8,    // waterlogged / bubble column
    }
}

/// Vanilla `FluidState.getOwnHeight()`: the surface height of a fluid cell in
/// isolation (amount/9), before the corner averaging.
fn own_height(amount: u8) -> f32 {
    amount as f32 / 9.0
}

/// Is this fluid state *falling* (block `level` ≥ 8)? Falling fluid renders as
/// a full-height column and its flow points straight down.
fn fluid_falling(table: &BlockTable, id: StateId) -> bool {
    table
        .entry(id)
        .and_then(|e| e.prop("level"))
        .and_then(|l| l.parse::<u8>().ok())
        .is_some_and(|l| l >= 8)
}

/// Vanilla `LiquidBlockRenderer.getHeight`: the surface height contributed by
/// the cell at (x, y, z) to a corner of the fluid at the centre.
/// - same fluid → 1.0 if the same fluid is above it, else its own height
/// - non-solid, non-fluid → 0.0 (drags the corner down)
/// - solid block → -1.0, meaning "ignore me" in the weighted average
fn corner_sample(
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    table: &BlockTable,
    (x, y, z): (i32, i32, i32),
    kind: FluidKind,
) -> f32 {
    let id = snap.get(x, y, z);
    if fluid_at(table, id) == Some(kind) {
        if fluid_at(table, snap.get(x, y + 1, z)) == Some(kind) {
            return 1.0;
        }
        return own_height(fluid_amount(table, id));
    }
    if store.occludes(id, Face::Up) { -1.0 } else { 0.0 }
}

/// Vanilla `addWeightedHeight`: near-full cells dominate the average tenfold,
/// so a source next to a trickle still reads as a flat surface.
fn add_weighted(acc: &mut (f32, f32), h: f32) {
    if h >= 0.8 {
        acc.0 += h * 10.0;
        acc.1 += 10.0;
    } else if h >= 0.0 {
        acc.0 += h;
        acc.1 += 1.0;
    }
}

/// Vanilla `calculateAverageHeight` for one corner: blend the cell's own height
/// with its two edge neighbours and (only when one of them carries fluid) the
/// diagonal.
#[allow(clippy::too_many_arguments)]
fn corner_height(
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    table: &BlockTable,
    (x, y, z): (i32, i32, i32),
    own: f32,
    dx: i32,
    dz: i32,
    kind: FluidKind,
) -> f32 {
    let side_x = corner_sample(snap, store, table, (x + dx, y, z), kind);
    let side_z = corner_sample(snap, store, table, (x, y, z + dz), kind);
    if side_x >= 1.0 || side_z >= 1.0 {
        return 1.0;
    }
    let mut acc = (0.0f32, 0.0f32);
    if side_x > 0.0 || side_z > 0.0 {
        let diag = corner_sample(snap, store, table, (x + dx, y, z + dz), kind);
        if diag >= 1.0 {
            return 1.0;
        }
        add_weighted(&mut acc, diag);
    }
    add_weighted(&mut acc, own);
    add_weighted(&mut acc, side_x);
    add_weighted(&mut acc, side_z);
    if acc.1 <= 0.0 { own } else { acc.0 / acc.1 }
}

/// The four corner heights of a fluid cell's surface, indexed
/// `[north-west, north-east, south-west, south-east]` — i.e. `(x, z)`,
/// `(x+1, z)`, `(x, z+1)`, `(x+1, z+1)`.
fn surface_heights(
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    table: &BlockTable,
    (x, y, z): (i32, i32, i32),
    kind: FluidKind,
) -> [f32; 4] {
    if fluid_at(table, snap.get(x, y + 1, z)) == Some(kind) {
        return [1.0; 4]; // covered by more fluid: the cell is full
    }
    let own = own_height(fluid_amount(table, snap.get(x, y, z)));
    [
        corner_height(snap, store, table, (x, y, z), own, -1, -1, kind),
        corner_height(snap, store, table, (x, y, z), own, 1, -1, kind),
        corner_height(snap, store, table, (x, y, z), own, -1, 1, kind),
        corner_height(snap, store, table, (x, y, z), own, 1, 1, kind),
    ]
}

/// Vanilla `FlowingFluid.getFlow`, reduced to what rendering needs: the
/// horizontal direction the fluid runs in, or `None` when it is still.
/// A falling fluid always reads as still (its surface is hidden anyway).
fn flow_vector(
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    table: &BlockTable,
    (x, y, z): (i32, i32, i32),
    kind: FluidKind,
) -> Option<(f32, f32)> {
    let id = snap.get(x, y, z);
    let own = own_height(fluid_amount(table, id));
    let (mut fx, mut fz) = (0.0f32, 0.0f32);
    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let nid = snap.get(x + dx, y, z + dz);
        let mut drop = 0.0f32;
        if fluid_at(table, nid) == Some(kind) {
            drop = own - own_height(fluid_amount(table, nid));
        } else if !store.occludes(nid, Face::Up) {
            // Empty neighbour: the fluid one below it still pulls the flow
            // (this is what makes water bend toward a ledge before it falls).
            let bid = snap.get(x + dx, y - 1, z + dz);
            if fluid_at(table, bid) == Some(kind) {
                let below = own_height(fluid_amount(table, bid));
                if below > 0.0 {
                    drop = own - (below - FLUID_SURFACE);
                }
            }
        }
        if drop != 0.0 {
            fx += dx as f32 * drop;
            fz += dz as f32 * drop;
        }
    }
    let len = (fx * fx + fz * fz).sqrt();
    if len < 1e-4 { None } else { Some((fx / len, fz / len)) }
}

/// A fluid face is dropped against same-fluid neighbors and occluding solids.
fn fluid_face_culled(neighbor_same_fluid: bool, neighbor_occludes: bool) -> bool {
    neighbor_same_fluid || neighbor_occludes
}

/// The four top-surface UVs for a flowing fluid: vanilla samples the middle half
/// of the flow sprite along an axis rotated into the flow direction, which is
/// what makes the surface visibly stream downhill.
fn flow_top_uvs(sprite: SpriteRect, flow: (f32, f32)) -> [[f32; 2]; 4] {
    let angle = flow.1.atan2(flow.0) - std::f32::consts::FRAC_PI_2;
    let (s, c) = (angle.sin() * 0.25, angle.cos() * 0.25);
    [
        sprite.at(0.5 + (-c - s), 0.5 + (-c + s)),
        sprite.at(0.5 + (-c + s), 0.5 + (c + s)),
        sprite.at(0.5 + (c + s), 0.5 + (c - s)),
        sprite.at(0.5 + (c - s), 0.5 + (-c - s)),
    ]
}

/// Corner order used for the surface quad, matching `surface_heights`:
/// NW, SW, SE, NE — counter-clockwise seen from above.
const TOP_CORNERS: [(usize, f32, f32); 4] =
    [(0, 0.0, 0.0), (2, 0.0, 1.0), (3, 1.0, 1.0), (1, 1.0, 0.0)];

#[allow(clippy::too_many_arguments)]
fn emit_fluid(
    mesh: &mut MeshData,
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    table: &BlockTable,
    uvs: FluidSprites,
    cell_tints: &CellTints,
    (x, y, z): (i32, i32, i32),
    kind: FluidKind,
) {
    let id = snap.get(x, y, z);
    let above_same = fluid_at(table, snap.get(x, y + 1, z)) == Some(kind);
    let heights = surface_heights(snap, store, table, (x, y, z), kind);
    let bt = tint_at(cell_tints, (x, y, z));
    let (layer, rgb, still, flow_sprite, overlay) = match kind {
        FluidKind::Water => (
            RenderLayer::Translucent,
            bt.water,
            uvs.water_still,
            uvs.water_flow,
            uvs.water_overlay,
        ),
        FluidKind::Lava => {
            (RenderLayer::Opaque, tint::NONE, uvs.lava_still, uvs.lava_flow, uvs.lava_flow)
        }
    };
    let (fx, fy, fz) = (x as f32, y as f32, z as f32);
    let light_of = |cx: i32, cy: i32, cz: i32| {
        let (sky, blk) = snap.light_at(cx, cy, cz);
        if kind == FluidKind::Lava { (sky, 15) } else { (sky, blk) }
    };
    let mut push = |face: Face, corners: [([f32; 3], [f32; 2]); 4], light: (u8, u8)| {
        let shade = (face.shade() * 255.0) as u8;
        let verts = corners.map(|(p, uv)| MeshVertex {
            pos: p,
            uv,
            color: [rgb[0], rgb[1], rgb[2], 255],
            light: [light_byte(light.0 as f32), light_byte(light.1 as f32), shade, 255],
        });
        mesh[layer].push_quad(verts);
    };

    // ---- top surface ------------------------------------------------------
    if !above_same && !store.occludes(snap.get(x, y + 1, z), Face::Down) {
        let uv = match flow_vector(snap, store, table, (x, y, z), kind) {
            Some(f) if !fluid_falling(table, id) => flow_top_uvs(flow_sprite, f),
            // Still surface: the still sprite, laid out axis-aligned.
            _ => [
                still.at(0.0, 0.0),
                still.at(0.0, 1.0),
                still.at(1.0, 1.0),
                still.at(1.0, 0.0),
            ],
        };
        let corners: [([f32; 3], [f32; 2]); 4] = std::array::from_fn(|i| {
            let (hi, cx, cz) = TOP_CORNERS[i];
            ([fx + cx, fy + heights[hi], fz + cz], uv[i])
        });
        // (Vanilla also emits a mirrored copy so the surface is visible from
        // below; our translucent pass is already double-sided, so one quad is
        // enough — a second would blend the water over itself.)
        push(Face::Up, corners, light_of(x, y + 1, z));
    }

    // ---- bottom face ------------------------------------------------------
    let below = snap.get(x, y - 1, z);
    if !fluid_face_culled(fluid_at(table, below) == Some(kind), store.occludes(below, Face::Up)) {
        let uv = [
            still.at(0.0, 0.0),
            still.at(1.0, 0.0),
            still.at(1.0, 1.0),
            still.at(0.0, 1.0),
        ];
        let corners: [([f32; 3], [f32; 2]); 4] = [
            ([fx, fy, fz], uv[0]),
            ([fx + 1.0, fy, fz], uv[1]),
            ([fx + 1.0, fy, fz + 1.0], uv[2]),
            ([fx, fy, fz + 1.0], uv[3]),
        ];
        push(Face::Down, corners, light_of(x, y - 1, z));
    }

    // ---- sides ------------------------------------------------------------
    // Each horizontal face runs between two surface corners, so a sloped
    // surface gets sloped side faces too. The flow sprite is sampled over its
    // left half and from the surface height down, which scrolls the texture
    // downwards exactly like a vanilla waterfall.
    for face in [Face::North, Face::South, Face::West, Face::East] {
        // (corner A index, corner B index) then their in-cell x/z, ordered so
        // the quad winds counter-clockwise seen from outside.
        let (ia, ib, ax, az, bx, bz) = match face {
            Face::North => (0, 1, 0.0, 0.0, 1.0, 0.0),
            Face::South => (3, 2, 1.0, 1.0, 0.0, 1.0),
            Face::West => (2, 0, 0.0, 1.0, 0.0, 0.0),
            _ => (1, 3, 1.0, 0.0, 1.0, 1.0),
        };
        let n = face.normal();
        let (nx, ny, nz) = (x + n[0], y + n[1], z + n[2]);
        let nid = snap.get(nx, ny, nz);
        let same = fluid_at(table, nid) == Some(kind);
        if fluid_face_culled(same, store.occludes(nid, face.opposite())) {
            continue;
        }
        let (ha, hb) = (heights[ia], heights[ib]);
        if ha <= 0.0 && hb <= 0.0 {
            continue;
        }
        // Water against a block that isn't a full cube shows `water_overlay`
        // (no side-texture cut-off), like vanilla's overlay sprite.
        let sprite = if kind == FluidKind::Water && !store.occludes(nid, face.opposite()) && overlay.u1 > overlay.u0 && !table.is_air(nid) {
            overlay
        } else {
            flow_sprite
        };
        let corners: [([f32; 3], [f32; 2]); 4] = [
            ([fx + ax, fy + ha, fz + az], sprite.at(0.0, (1.0 - ha) * 0.5)),
            ([fx + bx, fy + hb, fz + bz], sprite.at(0.5, (1.0 - hb) * 0.5)),
            ([fx + bx, fy, fz + bz], sprite.at(0.5, 0.5)),
            ([fx + ax, fy, fz + az], sprite.at(0.0, 0.5)),
        ];
        push(face, corners, light_of(nx, ny, nz));
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

/// A cell that fills its own volume has no light worth averaging — its stored
/// level is whatever leaked in, not what the face beside it sees.
fn fills_its_cell(store: &BakedModelStore, id: StateId) -> bool {
    Face::ALL.iter().all(|f| store.occludes(id, *f))
}

/// Vanilla's smooth lighting. Each vertex of a full border face averages the
/// light of the four cells that touch it on the lit side, skipping the ones
/// filled by a solid block; if all four are solid the face's own sample stands
/// in. Returned as `(sky, block)` bytes on the same 0..255 scale as `ao_byte`.
fn smooth_light_for_quad(
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    face: Face,
    verts: &[[f32; 3]; 4],
    (x, y, z): (i32, i32, i32),
    fallback: (u8, u8),
) -> [[u8; 2]; 4] {
    let n = face.normal();
    let base = [x + n[0], y + n[1], z + n[2]];
    let (a0, a1) = tangent_axes(face);
    let mut out = [[light_byte(fallback.0 as f32), light_byte(fallback.1 as f32)]; 4];
    for (i, v) in verts.iter().enumerate() {
        let ds = if v[a0] < 0.5 { -1 } else { 1 };
        let dt = if v[a1] < 0.5 { -1 } else { 1 };
        let mut o1 = [0i32; 3];
        o1[a0] = ds;
        let mut o2 = [0i32; 3];
        o2[a1] = dt;
        let corner = [o1[0] + o2[0], o1[1] + o2[1], o1[2] + o2[2]];
        let (mut sky, mut blk, mut count) = (0.0f32, 0.0f32, 0.0f32);
        for d in [[0, 0, 0], o1, o2, corner] {
            let (cx, cy, cz) = (base[0] + d[0], base[1] + d[1], base[2] + d[2]);
            if fills_its_cell(store, snap.get(cx, cy, cz)) {
                continue;
            }
            let (s, b) = snap.light_at(cx, cy, cz);
            sky += s as f32;
            blk += b as f32;
            count += 1.0;
        }
        if count > 0.0 {
            out[i] = [light_byte(sky / count), light_byte(blk / count)];
        }
    }
    out
}

fn tint_color(t: Option<TintKind>, bt: &SectionTint) -> [u8; 3] {
    match t {
        None => tint::NONE,
        Some(TintKind::Grass) => bt.grass,
        Some(TintKind::Foliage) => bt.foliage,
        Some(TintKind::Water) => bt.water,
    }
}

fn emit_model(
    mesh: &mut MeshData,
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    cell_tints: &CellTints,
    (x, y, z): (i32, i32, i32),
    id: StateId,
    smooth: bool,
) {
    let bt = tint_at(cell_tints, (x, y, z));
    let model = store.get(id);
    for quad in &model.quads {
        if let Some(d) = quad.cull {
            let dn = d.normal();
            let nid = snap.get(x + dn[0], y + dn[1], z + dn[2]);
            if model_quad_culled(quad.layer, id, nid, store.occludes(nid, d.opposite())) {
                continue;
            }
        }
        emit_quad(mesh, snap, store, &bt, (x, y, z), quad, smooth);
    }
}

/// The six faces of a unit cube, each wound counter-clockwise as seen from
/// outside (up, down, north, south, west, east).
const CUBE_FACES: [[[f32; 3]; 4]; 6] = [
    [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]],
    [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
    [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
    [[1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0]],
    [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]],
    [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]],
];

/// The End portal and End gateway have no block model — vanilla draws them as
/// block entities. Both are a starfield surface: the portal a single plane at
/// 3/4 height (the height its collision box ends at), the gateway a full cube.
fn is_end_portal(table: &BlockTable, id: StateId) -> bool {
    table
        .entry(id)
        .is_some_and(|e| matches!(e.short_name.as_str(), "end_portal" | "end_gateway"))
}

/// Emit the portal surface: an opaque quad (or cube) showing the starfield
/// sprite, unlit and unshaded so it reads as the void behind the world.
fn emit_end_portal(
    mesh: &mut MeshData,
    store: &BakedModelStore,
    table: &BlockTable,
    (x, y, z): (i32, i32, i32),
    id: StateId,
) {
    let sprite = store.end_portal();
    let gateway = table.entry(id).is_some_and(|e| e.short_name == "end_gateway");
    let vert = |px: f32, py: f32, pz: f32, fu: f32, fv: f32| MeshVertex {
        pos: [x as f32 + px, y as f32 + py, z as f32 + pz],
        uv: sprite.at(fu, fv),
        color: [255, 255, 255, 255],
        // Fullbright and unshaded: the starfield is its own light.
        light: [255, 255, 255, 255],
    };
    if gateway {
        // A full cube of starfield, all six faces wound outward.
        for quad in &CUBE_FACES {
            mesh[RenderLayer::Opaque].push_quad([
                vert(quad[0][0], quad[0][1], quad[0][2], 0.0, 0.0),
                vert(quad[1][0], quad[1][1], quad[1][2], 0.0, 1.0),
                vert(quad[2][0], quad[2][1], quad[2][2], 1.0, 1.0),
                vert(quad[3][0], quad[3][1], quad[3][2], 1.0, 0.0),
            ]);
        }
    } else {
        // Vanilla's portal plane sits at 3/4 of the block, seen from above.
        const H: f32 = 0.75;
        mesh[RenderLayer::Opaque].push_quad([
            vert(0.0, H, 0.0, 0.0, 0.0),
            vert(0.0, H, 1.0, 0.0, 1.0),
            vert(1.0, H, 1.0, 1.0, 1.0),
            vert(1.0, H, 0.0, 1.0, 0.0),
        ]);
    }
}

fn emit_quad(
    mesh: &mut MeshData,
    snap: &PaddedSnapshot,
    store: &BakedModelStore,
    bt: &SectionTint,
    (x, y, z): (i32, i32, i32),
    quad: &BakedQuad,
    smooth: bool,
) {
    let border = quad_on_border(quad.face, &quad.verts);
    let (sky, blk) = if border {
        let n = quad.face.normal();
        snap.light_at(x + n[0], y + n[1], z + n[2])
    } else {
        snap.light_at(x, y, z)
    };
    let shade = (quad.face.shade() * 255.0) as u8;
    let full_border = border && quad_full_face(quad.face, &quad.verts);
    let ao = if full_border {
        ao_for_quad(snap, store, quad.face, &quad.verts, (x, y, z))
    } else {
        [255u8; 4]
    };
    let vlight = if smooth && full_border {
        smooth_light_for_quad(snap, store, quad.face, &quad.verts, (x, y, z), (sky, blk))
    } else {
        [[light_byte(sky as f32), light_byte(blk as f32)]; 4]
    };
    let rgb = tint_color(quad.tint, bt);

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
            light: [vlight[i][0], vlight[i][1], shade, ao[i]],
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
    fn light_levels_scale_to_the_full_byte_range() {
        // 15 must land exactly on 255 or the brightest terrain would never
        // reach the top of the light ramp.
        assert_eq!(light_byte(15.0), 255);
        assert_eq!(light_byte(0.0), 0);
        // Smooth lighting produces fractions between two levels.
        assert_eq!(light_byte(7.5), 128);
        // Out-of-range values clamp rather than wrap.
        assert_eq!(light_byte(-3.0), 0);
        assert_eq!(light_byte(99.0), 255);
    }

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
    fn fluid_cull_rule() {
        assert!(fluid_face_culled(true, false));
        assert!(fluid_face_culled(false, true));
        assert!(fluid_face_culled(true, true));
        assert!(!fluid_face_culled(false, false));
    }

    #[test]
    fn fluid_amount_from_level() {
        // A source (level=0) and falling fluid (level>=8) both fill their cell;
        // levels 1..7 step down by 1/9 of a block each.
        assert_eq!(own_height(8), FLUID_SURFACE);
        assert!((own_height(1) - 1.0 / 9.0).abs() < 1e-6);
        assert!(own_height(7) > own_height(3));
    }

    #[test]
    fn weighted_height_favours_full_cells() {
        // A near-full neighbour outweighs a trickle 10:1, so the surface next
        // to a source stays flat instead of dipping.
        let mut acc = (0.0, 0.0);
        add_weighted(&mut acc, 0.9);
        add_weighted(&mut acc, 0.1);
        let avg = acc.0 / acc.1;
        assert!(avg > 0.8, "expected the full cell to dominate, got {avg}");

        // -1.0 ("solid neighbour") contributes nothing at all.
        let mut solid = (0.0, 0.0);
        add_weighted(&mut solid, -1.0);
        assert_eq!(solid, (0.0, 0.0));
    }

    #[test]
    fn flow_uvs_stay_inside_the_sprite() {
        // The rotated flow sampling must never leave the sprite rect, or the
        // surface would bleed into whatever is packed beside it in the atlas.
        let s = SpriteRect { u0: 0.25, v0: 0.5, u1: 0.5, v1: 0.75 };
        for deg in (0..360).step_by(15) {
            let a = (deg as f32).to_radians();
            for uv in flow_top_uvs(s, (a.cos(), a.sin())) {
                assert!((s.u0..=s.u1).contains(&uv[0]), "u {} out of {deg}°", uv[0]);
                assert!((s.v0..=s.v1).contains(&uv[1]), "v {} out of {deg}°", uv[1]);
            }
        }
    }

    #[test]
    fn fluid_side_quads_wind_outward() {
        // Same winding contract as the model quads: CCW seen from outside.
        for (face, ia, ib, ax, az, bx, bz) in [
            (Face::North, 0, 1, 0.0f32, 0.0f32, 1.0f32, 0.0f32),
            (Face::South, 3, 2, 1.0, 1.0, 0.0, 1.0),
            (Face::West, 2, 0, 0.0, 1.0, 0.0, 0.0),
            (Face::East, 1, 3, 1.0, 0.0, 1.0, 1.0),
        ] {
            let h = [0.9f32, 0.8, 0.7, 0.6];
            let (ha, hb) = (h[ia], h[ib]);
            let c = [
                [ax, ha, az],
                [bx, hb, bz],
                [bx, 0.0, bz],
                [ax, 0.0, az],
            ];
            let e1 = [c[1][0] - c[0][0], c[1][1] - c[0][1], c[1][2] - c[0][2]];
            let e2 = [c[2][0] - c[0][0], c[2][1] - c[0][1], c[2][2] - c[0][2]];
            let cross = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let n = face.normal();
            let dot = cross[0] * n[0] as f32 + cross[1] * n[1] as f32 + cross[2] * n[2] as f32;
            assert!(dot > 0.0, "{face:?} fluid side winds inward (dot {dot})");
        }
    }

    #[test]
    fn cube_faces_wind_outward() {
        // (up, down, north, south, west, east) — the order `CUBE_FACES` uses.
        let normals = [
            [0.0f32, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, -1.0],
            [0.0, 0.0, 1.0],
            [-1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
        ];
        for (q, n) in CUBE_FACES.iter().zip(normals) {
            let e1 = [q[1][0] - q[0][0], q[1][1] - q[0][1], q[1][2] - q[0][2]];
            let e2 = [q[2][0] - q[0][0], q[2][1] - q[0][1], q[2][2] - q[0][2]];
            let cross = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let dot = cross[0] * n[0] + cross[1] * n[1] + cross[2] * n[2];
            assert!(dot > 0.0, "cube face with normal {n:?} winds inward (dot {dot})");
        }
    }

    #[test]
    fn fluid_top_quad_winds_upward() {
        let h = [0.9f32, 0.8, 0.7, 0.6];
        let c: [[f32; 3]; 4] = std::array::from_fn(|i| {
            let (hi, cx, cz) = TOP_CORNERS[i];
            [cx, h[hi], cz]
        });
        let e1 = [c[1][0] - c[0][0], c[1][1] - c[0][1], c[1][2] - c[0][2]];
        let e2 = [c[2][0] - c[0][0], c[2][1] - c[0][1], c[2][2] - c[0][2]];
        let cross_y = e1[2] * e2[0] - e1[0] * e2[2];
        assert!(cross_y > 0.0, "fluid surface winds downward (y {cross_y})");
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
        // A full-cube face is on its border plane and covers it.
        let full_up: [[f32; 3]; 4] =
            [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]];
        assert!(quad_on_border(Face::Up, &full_up));
        assert!(quad_full_face(Face::Up, &full_up));

        // A lowered fluid surface is not on the border at all.
        let low_up = full_up.map(|[x, _, z]| [x, FLUID_SURFACE, z]);
        assert!(!quad_on_border(Face::Up, &low_up));
        assert!(!quad_full_face(Face::Up, &low_up));

        // A half quad (slab top half missing) is on-border but not full.
        let half: [[f32; 3]; 4] =
            [[0.0, 0.0, 0.0], [0.0, 0.5, 0.0], [1.0, 0.5, 0.0], [1.0, 0.0, 0.0]];
        assert!(quad_on_border(Face::North, &half));
        assert!(!quad_full_face(Face::North, &half));
    }

    #[test]
    fn sprite_rect_samples_corners() {
        let s = SpriteRect { u0: 0.0, v0: 0.0, u1: 0.5, v1: 0.25 };
        assert_eq!(s.at(0.0, 0.0), [0.0, 0.0]);
        assert_eq!(s.at(1.0, 0.0), [0.5, 0.0]);
        assert_eq!(s.at(1.0, 1.0), [0.5, 0.25]);
        assert_eq!(s.at(0.5, 0.5), [0.25, 0.125]);
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

    /// A `PaddedSnapshot` with only `biome_cells` set up (blocks/light zeroed —
    /// irrelevant to tinting), split down the middle: cells with cx<2 are biome 0,
    /// cx>=2 are biome 1 (a hard border at the section's own halfway point, so
    /// the padding side contributes the same split biome too).
    fn split_biome_snapshot() -> PaddedSnapshot {
        let blocks: Box<[StateId; PADDED_VOLUME]> =
            vec![0; PADDED_VOLUME].into_boxed_slice().try_into().unwrap();
        let light: Box<[u8; PADDED_VOLUME]> =
            vec![0xFF; PADDED_VOLUME].into_boxed_slice().try_into().unwrap();
        let mut biome_cells: Box<[u32; BIOME_CELL_PADDED_VOLUME]> =
            vec![0u32; BIOME_CELL_PADDED_VOLUME].into_boxed_slice().try_into().unwrap();
        for cy in -1..=4 {
            for cz in -1..=4 {
                for cx in -1..=4 {
                    biome_cells[PaddedSnapshot::cell_idx(cx, cy, cz)] = if cx < 2 { 0 } else { 1 };
                }
            }
        }
        PaddedSnapshot { pos: SectionPos { x: 0, y: 0, z: 0 }, blocks, light, biome_cells }
    }

    /// Two visually distinct grass colors, indexed by biome id 0 and 1 (foliage/
    /// water left at black — this test only checks grass).
    fn two_biome_tints() -> BiomeTints {
        BiomeTints::from_rows(vec![
            [[0xFF, 0x00, 0x00], [0, 0, 0], [0, 0, 0]], // biome 0: pure red
            [[0x00, 0x00, 0xFF], [0, 0, 0], [0, 0, 0]], // biome 1: pure blue
        ])
    }

    #[test]
    fn cell_tint_blends_across_a_biome_border_instead_of_hard_stepping() {
        let snap = split_biome_snapshot();
        let tints = two_biome_tints();

        // Deep inside biome 0's territory (cell 0, neighbors all biome 0 too):
        // no blending needed, stays pure red.
        assert_eq!(cell_tint(&snap, &tints, (0, 0, 0)).grass, [0xFF, 0x00, 0x00]);
        // Deep inside biome 1's territory (cell 3): pure blue.
        assert_eq!(cell_tint(&snap, &tints, (3, 0, 0)).grass, [0x00, 0x00, 0xFF]);

        // Right at the border (cell 1, whose 3×3×3 neighborhood straddles both
        // biomes): must be a real blend, not either hard color.
        let border = cell_tint(&snap, &tints, (1, 0, 0)).grass;
        assert_ne!(border, [0xFF, 0x00, 0x00], "border cell rendered as a hard biome-0 edge");
        assert_ne!(border, [0x00, 0x00, 0xFF], "border cell rendered as a hard biome-1 edge");
        // Red channel present but reduced, blue channel present but reduced —
        // confirms it's an actual mix, not some unrelated third color.
        assert!(border[0] > 0 && border[0] < 0xFF);
        assert!(border[2] > 0 && border[2] < 0xFF);
    }

    #[test]
    fn build_cell_tints_covers_every_local_cell() {
        let snap = split_biome_snapshot();
        let tints = two_biome_tints();
        let table = build_cell_tints(&snap, &tints);
        // Same border-blend guarantee, now going through the precomputed
        // per-section table (and its usize/i32 index conversion) rather than
        // calling cell_tint directly.
        let at = |cx: usize, cy: usize, cz: usize| table[(cy * 4 + cz) * 4 + cx];
        assert_eq!(at(0, 0, 0).grass, [0xFF, 0x00, 0x00]);
        assert_eq!(at(3, 3, 3).grass, [0x00, 0x00, 0xFF]);
        let border = at(1, 2, 2).grass;
        assert!(border[0] > 0 && border[0] < 0xFF);
    }

    #[test]
    fn tint_at_maps_block_coords_to_their_containing_cell() {
        let snap = split_biome_snapshot();
        let tints = two_biome_tints();
        let table = build_cell_tints(&snap, &tints);
        // Blocks 0..3 fall in cell 0 (pure biome 0); blocks 12..15 fall in cell
        // 3 (pure biome 1) — same section, opposite ends.
        assert_eq!(tint_at(&table, (0, 0, 0)).grass, [0xFF, 0x00, 0x00]);
        assert_eq!(tint_at(&table, (3, 0, 0)).grass, [0xFF, 0x00, 0x00]);
        assert_eq!(tint_at(&table, (12, 0, 0)).grass, [0x00, 0x00, 0xFF]);
        assert_eq!(tint_at(&table, (15, 0, 0)).grass, [0x00, 0x00, 0xFF]);
    }
}
