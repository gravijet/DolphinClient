//! Baking: one pass over all block states at startup.
//! Output is immutable and shared with rayon meshing threads via Arc.

use crate::assets::AssetPack;
use crate::assets::atlas::{Atlas, AtlasBuilder};
use crate::assets::blockmap::BlockTable;
use crate::models::{self, ElemRot, ModelRef, ResolvedModel};
use crate::types::{Face, RenderLayer, StateId};
use anyhow::Result;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TintKind {
    Grass,
    Foliage,
    Water,
}

#[derive(Clone, Debug)]
pub struct BakedQuad {
    /// Unit-cube space (0.0..1.0; model coords /16), CCW seen from outside.
    /// Vertex order: consistent with `push_quad` (two tris 0-1-2, 0-2-3).
    pub verts: [[f32; 3]; 4],
    /// Final normalized atlas UVs per vertex.
    pub uvs: [[f32; 2]; 4],
    /// Skip this quad when the neighbor in this direction occludes.
    pub cull: Option<Face>,
    /// Dominant facing (light sampling, face shade, AO plane).
    pub face: Face,
    pub tint: Option<TintKind>,
    pub layer: RenderLayer,
}

#[derive(Clone, Debug)]
pub struct BakedModel {
    pub quads: Vec<BakedQuad>,
    /// Per direction: does this model fully cover that face with opaque texels
    /// (i.e. neighbors may cull faces pointing at it)?
    pub occludes: [bool; 6],
}

impl BakedModel {
    pub fn empty() -> Self {
        Self { quads: Vec::new(), occludes: [false; 6] }
    }
}

pub struct BakedModelStore {
    /// Indexed by state id. Never empty entries — fallback cube is substituted
    /// at bake time for states whose assets failed to resolve.
    models: Vec<Arc<BakedModel>>,
    air: Arc<BakedModel>,
    /// Atlas UV rects of block/water_still and block/lava_still, corner order
    /// (0,0),(1,0),(1,1),(0,1) in face-local (s,t) — consumed by the mesher's
    /// fluid special-case, which has no other path to the atlas.
    water_still_uv: [[f32; 2]; 4],
    lava_still_uv: [[f32; 2]; 4],
}

/// What to bake for one state (decided in pass 1, executed in pass 3).
enum Plan {
    /// Air and fluids: nothing baked (the mesher special-cases fluids).
    Empty,
    /// Broken assets: checker-textured full cube.
    Fallback,
    /// Regular model parts (may be empty for multipart with no matching part).
    Parts(Vec<ModelRef>),
    /// Block-entity chest: the vanilla box+lid+latch model textured from the
    /// chest entity PNG (the block model itself is particle-only → invisible).
    Chest { tex: &'static str, y_steps: usize },
    /// Block-entity bed half: the vanilla mattress + legs textured from the
    /// per-colour bed entity PNG (also particle-only → invisible otherwise).
    Bed { tex: String, head: bool, y_steps: usize },
}

/// The bed entity texture for a `<colour>_bed` block short name, or `None`.
fn bed_tex(short: &str) -> Option<String> {
    let color = short.strip_suffix("_bed")?;
    const COLORS: &[&str] = &[
        "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray",
        "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black",
    ];
    COLORS.contains(&color).then(|| format!("entity/bed/{color}"))
}

/// The chest-family entity texture for a block short name, or `None`.
fn chest_tex(short: &str) -> Option<&'static str> {
    match short {
        "chest" => Some("entity/chest/normal"),
        "trapped_chest" => Some("entity/chest/trapped"),
        "ender_chest" => Some("entity/chest/ender"),
        "copper_chest" => Some("entity/chest/copper"),
        _ => None,
    }
}

/// Quarter-turns about +Y to bring the model's authored front (+Z / south) onto
/// the block's `facing` direction. `rot_pos_y90` is vanilla Ry(-90°): S→W→N→E.
fn chest_y_steps(facing: &str) -> usize {
    match facing {
        "south" => 0,
        "west" => 1,
        "north" => 2,
        "east" => 3,
        _ => 2,
    }
}

impl BakedModelStore {
    /// Drives the whole pipeline:
    /// 1. For every state id in `table`: read blockstate json (cache per block),
    ///    select variant / multipart parts, resolve model parent chains
    ///    (cache per model ref), collect referenced texture names.
    /// 2. Load all textures into an `AtlasBuilder`, build the `Atlas`.
    /// 3. Bake quads: apply element rotation, then variant x/y rotation
    ///    (rotate cullfaces along!), map UVs into atlas sprites, classify
    ///    layer per quad: sprite.translucent → Translucent (also forced for
    ///    water/ice/slime/honey/stained_glass/tinted_glass by name),
    ///    else sprite.has_cutout → Cutout, else Opaque.
    ///    tintindex → TintKind by block name (grass_block/short_grass/tall_grass/
    ///    fern/sugar_cane → Grass; *_leaves/vine → Foliage; water → Water).
    /// 4. occludes[]: full-cube geometry (elements cover 0..16 on that face)
    ///    with all-opaque sprites; leaves and translucent NEVER occlude.
    /// 5. Fluids (water/lava) get NO model here — the mesher special-cases them;
    ///    give them `BakedModel::empty()`.
    ///
    /// Air states get `BakedModel::empty()`. States with missing/broken assets
    /// get a full checker-textured cube (and a WARN log with the block name).
    pub fn bake_all(pack: &mut AssetPack, table: &BlockTable) -> Result<(BakedModelStore, Atlas)> {
        let t0 = std::time::Instant::now();
        let n = table.len();

        // ---- pass 1: blockstate selection, model resolution, texture set ----
        let mut bs_cache: HashMap<String, Option<serde_json::Value>> = HashMap::new();
        let mut resolved: HashMap<String, Option<Arc<ResolvedModel>>> = HashMap::new();
        let mut textures: BTreeSet<String> = BTreeSet::new();
        let mut warned: HashSet<String> = HashSet::new();
        let mut plans: Vec<Plan> = Vec::with_capacity(n);

        for id in 0..n as StateId {
            let Some(entry) = table.entry(id) else {
                plans.push(Plan::Empty);
                continue;
            };
            let short = entry.short_name.clone();
            if table.is_air(id) || matches!(short.as_str(), "water" | "lava" | "bubble_column") {
                plans.push(Plan::Empty);
                continue;
            }
            // Chests are block entities: their block model is particle-only, so
            // bake the vanilla box+lid+latch from the chest entity texture.
            if let Some(tex) = chest_tex(&short) {
                textures.insert(tex.to_owned());
                let y_steps = chest_y_steps(entry.prop("facing").unwrap_or("north"));
                plans.push(Plan::Chest { tex, y_steps });
                continue;
            }
            // Beds are block entities too (mattress + legs from the bed PNG).
            if let Some(tex) = bed_tex(&short) {
                textures.insert(tex.clone());
                let head = entry.prop("part") == Some("head");
                let y_steps = chest_y_steps(entry.prop("facing").unwrap_or("north"));
                plans.push(Plan::Bed { tex, head, y_steps });
                continue;
            }
            let bs = bs_cache.entry(short.clone()).or_insert_with(|| {
                match pack.blockstate_json(&short) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        warn!("models: {short}: blockstate missing/broken, using fallback cube: {e:#}");
                        None
                    }
                }
            });
            let Some(bs) = bs.as_ref() else {
                plans.push(Plan::Fallback);
                continue;
            };
            let Some(sel) = models::select_model_refs(bs, &entry.props) else {
                if warned.insert(short.clone()) {
                    warn!("models: {short}: no variant matched its state props, using fallback cube");
                }
                plans.push(Plan::Fallback);
                continue;
            };
            let requested = sel.len();
            let mut parts = Vec::with_capacity(requested);
            for mr in sel {
                if !resolved.contains_key(&mr.model) {
                    let rm = match models::resolve_model(&mut |r| pack.model_json(r), &mr.model) {
                        Ok(rm) => Some(Arc::new(rm)),
                        Err(e) => {
                            warn!("models: {short}: model {} failed to resolve: {e:#}", mr.model);
                            None
                        }
                    };
                    if let Some(rm) = &rm {
                        for el in &rm.elements {
                            for (_, f) in &el.faces {
                                if let Some(t) = models::lookup_texture(&rm.textures, &f.texture) {
                                    textures.insert(t);
                                } else if warned.insert(format!("tex:{}:{}", mr.model, f.texture)) {
                                    warn!(
                                        "models: {}: texture ref {:?} unresolvable, using checker",
                                        mr.model, f.texture
                                    );
                                }
                            }
                        }
                    }
                    resolved.insert(mr.model.clone(), rm);
                }
                if resolved.get(&mr.model).is_some_and(Option::is_some) {
                    parts.push(mr);
                }
            }
            if parts.is_empty() && requested > 0 {
                plans.push(Plan::Fallback); // every referenced model was broken
            } else {
                plans.push(Plan::Parts(parts));
            }
        }

        // ---- pass 2: atlas -------------------------------------------------
        // The mesher always needs the fluid stills, even though water/lava
        // bake to empty models.
        textures.insert("block/water_still".to_owned());
        textures.insert("block/lava_still".to_owned());
        let mut builder = AtlasBuilder::new();
        for name in &textures {
            match pack.texture_png(name) {
                Ok(img) => builder.add(name, img),
                Err(e) => warn!("models: texture {name} failed to load, using checker: {e:#}"),
            }
        }
        let atlas = builder.build();

        // ---- pass 3: bake --------------------------------------------------
        let empty = Arc::new(BakedModel::empty());
        let fallback = Arc::new(fallback_cube(&atlas));
        let mut neutral: HashMap<(String, i32, i32), Arc<NeutralModel>> = HashMap::new();
        let mut finals: HashMap<(String, Vec<(String, i32, i32)>), Arc<BakedModel>> = HashMap::new();
        let mut chests: HashMap<(&'static str, usize), Arc<BakedModel>> = HashMap::new();
        let mut beds: HashMap<(String, bool, usize), Arc<BakedModel>> = HashMap::new();
        let mut store = Vec::with_capacity(n);

        for (id, plan) in plans.iter().enumerate() {
            let model = match plan {
                Plan::Empty => empty.clone(),
                Plan::Fallback => fallback.clone(),
                Plan::Chest { tex, y_steps } => chests
                    .entry((tex, *y_steps))
                    .or_insert_with(|| Arc::new(bake_chest(tex, *y_steps, &atlas)))
                    .clone(),
                Plan::Bed { tex, head, y_steps } => beds
                    .entry((tex.clone(), *head, *y_steps))
                    .or_insert_with(|| Arc::new(bake_bed(tex, *head, *y_steps, &atlas)))
                    .clone(),
                Plan::Parts(parts) if parts.is_empty() => empty.clone(),
                Plan::Parts(parts) => {
                    let short = table
                        .entry(id as StateId)
                        .map(|e| e.short_name.as_str())
                        .unwrap_or("");
                    let key = (
                        short.to_owned(),
                        parts.iter().map(|p| (p.model.clone(), p.x, p.y)).collect::<Vec<_>>(),
                    );
                    if let Some(m) = finals.get(&key) {
                        m.clone()
                    } else {
                        let mut baked_parts = Vec::with_capacity(parts.len());
                        for p in parts {
                            let nk = (p.model.clone(), p.x, p.y);
                            let nm = match neutral.get(&nk) {
                                Some(nm) => nm.clone(),
                                None => {
                                    let nm = match resolved.get(&p.model).and_then(Option::as_ref) {
                                        Some(rm) => Arc::new(bake_neutral(rm, p.x, p.y, &atlas)),
                                        None => {
                                            // Unreachable by construction (pass 1 only
                                            // keeps resolvable parts); stay graceful.
                                            warn!("models: {} vanished from resolve cache", p.model);
                                            Arc::new(NeutralModel::default())
                                        }
                                    };
                                    neutral.insert(nk, nm.clone());
                                    nm
                                }
                            };
                            baked_parts.push(nm);
                        }
                        let m = Arc::new(assemble(short, &baked_parts));
                        finals.insert(key, m.clone());
                        m
                    }
                }
            };
            store.push(model);
        }

        info!(
            "models: baked {} states → {} unique models ({} neutral bakes), {} textures, atlas {}px, in {:.2?}",
            n,
            finals.len(),
            neutral.len(),
            textures.len(),
            atlas.image.width(),
            t0.elapsed()
        );
        let sprite_rect = |name: &str| {
            let s = atlas.sprite(name);
            [[s.u0, s.v0], [s.u1, s.v0], [s.u1, s.v1], [s.u0, s.v1]]
        };
        let water_still_uv = sprite_rect("block/water_still");
        let lava_still_uv = sprite_rect("block/lava_still");
        Ok((BakedModelStore { models: store, air: empty, water_still_uv, lava_still_uv }, atlas))
    }

    /// Atlas UV rect of the water still sprite, corners (0,0),(1,0),(1,1),(0,1)
    /// in face-local (s,t).
    #[inline]
    pub fn water_still_uv(&self) -> [[f32; 2]; 4] {
        self.water_still_uv
    }

    /// Atlas UV rect of the lava still sprite (same corner order).
    #[inline]
    pub fn lava_still_uv(&self) -> [[f32; 2]; 4] {
        self.lava_still_uv
    }

    #[inline]
    pub fn get(&self, id: StateId) -> &Arc<BakedModel> {
        self.models.get(id as usize).unwrap_or(&self.air)
    }

    /// Neighbor occlusion test used by the mesher.
    #[inline]
    pub fn occludes(&self, id: StateId, face: Face) -> bool {
        self.get(id).occludes[face as usize]
    }
}

// ---------------------------------------------------------------------------
// Neutral baking: (resolved model, variant x/y) → quads with final atlas UVs
// but no block-name-dependent decisions (layer forcing, tint kind, leaves).
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct NeutralQuad {
    verts: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    cull: Option<Face>,
    face: Face,
    tinted: bool,
    /// Sprite alpha classification (from the atlas).
    translucent: bool,
    has_cutout: bool,
}

#[derive(Clone, Debug, Default)]
struct NeutralModel {
    quads: Vec<NeutralQuad>,
    /// Geometric full-face coverage with opaque sprites, already rotated by
    /// the variant rotation. Leaves/translucent veto happens in `assemble`.
    occludes: [bool; 6],
}

fn bake_neutral(rm: &ResolvedModel, x_rot: i32, y_rot: i32, atlas: &Atlas) -> NeutralModel {
    let kx = (x_rot / 90).rem_euclid(4) as usize;
    let ky = (y_rot / 90).rem_euclid(4) as usize;
    let mut quads = Vec::new();
    let mut occ = [false; 6];

    for el in &rm.elements {
        let lo = [el.from[0] / 16.0, el.from[1] / 16.0, el.from[2] / 16.0];
        let hi = [el.to[0] / 16.0, el.to[1] / 16.0, el.to[2] / 16.0];
        let elem_rot = el.rot.as_ref().map(elem_rot_params);

        for (face, ef) in &el.faces {
            let sprite_name = models::lookup_texture(&rm.textures, &ef.texture)
                .unwrap_or_else(|| "missing".to_owned());
            let sprite = atlas.sprite(&sprite_name);
            let forced_translucent = rm.translucent_sprites.contains(&sprite_name);

            // Positions: face corners → element rotation → variant rotation.
            let mut verts = face_corners(*face, lo, hi);
            if let Some(params) = &elem_rot {
                for v in &mut verts {
                    *v = apply_elem_rot(*v, params);
                }
            }
            for v in &mut verts {
                *v = rot_pos(*v, kx, ky);
            }

            // UVs: explicit or face-projected default, rotated corner
            // assignment (vanilla index shift), lerped into the sprite rect.
            let rect = ef.uv.unwrap_or_else(|| models::default_uv(*face, el.from, el.to));
            let shift = (ef.rotation / 90) as usize % 4;
            let mut uvs = [[0f32; 2]; 4];
            for (i, uv) in uvs.iter_mut().enumerate() {
                let (u, v) = uv_corner(rect, (i + shift) % 4);
                *uv = [
                    sprite.u0 + (sprite.u1 - sprite.u0) * (u / 16.0),
                    sprite.v0 + (sprite.v1 - sprite.v0) * (v / 16.0),
                ];
            }

            let cull = ef.cullface.map(|c| rot_face(c, kx, ky));
            let face_out = dominant_face(&verts).unwrap_or_else(|| rot_face(*face, kx, ky));
            quads.push(NeutralQuad {
                verts,
                uvs,
                cull,
                face: face_out,
                tinted: ef.tintindex.is_some(),
                translucent: sprite.translucent || forced_translucent,
                has_cutout: sprite.has_cutout,
            });
        }

        // Occlusion coverage: only unrotated elements that span the full face.
        if el.rot.is_none() {
            for (face, ef) in &el.faces {
                if covers_face(*face, el.from, el.to) {
                    let sprite_name = models::lookup_texture(&rm.textures, &ef.texture)
                        .unwrap_or_else(|| "missing".to_owned());
                    if atlas.sprite(&sprite_name).opaque {
                        occ[rot_face(*face, kx, ky) as usize] = true;
                    }
                }
            }
        }
    }

    NeutralModel { quads, occludes: occ }
}

/// Final per-state assembly: block-name-dependent layer/tint/occlusion.
fn assemble(short: &str, parts: &[Arc<NeutralModel>]) -> BakedModel {
    let forced_translucent = matches!(short, "ice" | "slime_block" | "honey_block" | "tinted_glass")
        || short.contains("stained_glass");
    let leaves = short.ends_with("_leaves");
    let tint_kind = tint_kind_for(short);

    let mut quads = Vec::new();
    let mut occludes = [false; 6];
    let mut any_translucent = false;
    for p in parts {
        for q in &p.quads {
            let layer = if forced_translucent || q.translucent {
                RenderLayer::Translucent
            } else if q.has_cutout {
                RenderLayer::Cutout
            } else {
                RenderLayer::Opaque
            };
            any_translucent |= layer == RenderLayer::Translucent;
            quads.push(BakedQuad {
                verts: q.verts,
                uvs: q.uvs,
                cull: q.cull,
                face: q.face,
                tint: if q.tinted { tint_kind } else { None },
                layer,
            });
        }
        for (o, po) in occludes.iter_mut().zip(&p.occludes) {
            *o |= po;
        }
    }
    if leaves || any_translucent {
        occludes = [false; 6];
    }
    BakedModel { quads, occludes }
}

fn tint_kind_for(short: &str) -> Option<TintKind> {
    match short {
        "grass_block" | "short_grass" | "tall_grass" | "fern" | "large_fern" | "sugar_cane" => {
            Some(TintKind::Grass)
        }
        "vine" => Some(TintKind::Foliage),
        "water" => Some(TintKind::Water),
        s if s.ends_with("_leaves") => Some(TintKind::Foliage),
        _ => None,
    }
}

/// Full checker-textured cube for states with missing/broken assets.
fn fallback_cube(atlas: &Atlas) -> BakedModel {
    let sprite = atlas.sprite("missing");
    let mut quads = Vec::with_capacity(6);
    for face in Face::ALL {
        let verts = face_corners(face, [0.0; 3], [1.0; 3]);
        let mut uvs = [[0f32; 2]; 4];
        for (i, uv) in uvs.iter_mut().enumerate() {
            let (u, v) = uv_corner([0.0, 0.0, 16.0, 16.0], i);
            *uv = [
                sprite.u0 + (sprite.u1 - sprite.u0) * (u / 16.0),
                sprite.v0 + (sprite.v1 - sprite.v0) * (v / 16.0),
            ];
        }
        quads.push(BakedQuad {
            verts,
            uvs,
            cull: Some(face),
            face,
            tint: None,
            layer: RenderLayer::Opaque,
        });
    }
    BakedModel { quads, occludes: [true; 6] }
}

/// Bake the vanilla single-chest model (box + lid + latch) from the 64×64 chest
/// entity texture, rotated by `y_steps` quarter-turns to face the block's
/// `facing`. The block model is particle-only, so without this chests are
/// invisible in the world.
fn bake_chest(tex: &str, y_steps: usize, atlas: &Atlas) -> BakedModel {
    let sprite = *atlas.sprite(tex);
    let (tw, th) = (64.0f32, 64.0f32);
    let mut quads = Vec::with_capacity(18);
    let u = |px: f32| px / 16.0;
    // Bottom base: 1,0,1 → 15,10,15 (14×10×14), texOffs(0,19).
    push_chest_box(
        &mut quads, [u(1.0), u(0.0), u(1.0)], [u(15.0), u(10.0), u(15.0)],
        [0.0, 19.0], [14.0, 10.0, 14.0], &sprite, tw, th, y_steps,
    );
    // Lid: 1,9,1 → 15,14,15 (14×5×14), texOffs(0,0). Sits closed on the base.
    push_chest_box(
        &mut quads, [u(1.0), u(9.0), u(1.0)], [u(15.0), u(14.0), u(15.0)],
        [0.0, 0.0], [14.0, 5.0, 14.0], &sprite, tw, th, y_steps,
    );
    // Latch/keyhole: 7,7,15 → 9,11,16 (2×4×1) on the front face, texOffs(0,0)
    // (the unused top-left corner of the sheet holds the keyhole art).
    push_chest_box(
        &mut quads, [u(7.0), u(7.0), u(15.0)], [u(9.0), u(11.0), u(16.0)],
        [0.0, 0.0], [2.0, 4.0, 1.0], &sprite, tw, th, y_steps,
    );
    BakedModel { quads, occludes: [false; 6] }
}

/// Append the six faces of a box (unit-space `lo`..`hi`, front = +Z) with the
/// vanilla box UV unwrap starting at `off` (texture px) for a box of pixel
/// `dims` (w,h,d), sampled from `sprite`'s sub-rect of a `tw`×`th` texture, then
/// rotated `y_steps` quarter-turns about the block centre.
#[allow(clippy::too_many_arguments)]
fn push_chest_box(
    quads: &mut Vec<BakedQuad>,
    lo: [f32; 3],
    hi: [f32; 3],
    off: [f32; 2],
    dims: [f32; 3],
    sprite: &crate::assets::atlas::AtlasSprite,
    tw: f32,
    th: f32,
    y_steps: usize,
) {
    let [w, h, d] = dims;
    let [ox, oy] = off;
    // (face, texture-px rect [u0,v0,u1,v1]) — standard MC box unwrap, front=+Z.
    let faces: [(Face, [f32; 4]); 6] = [
        (Face::South, [ox + d, oy + d, ox + d + w, oy + d + h]),
        (Face::North, [ox + 2.0 * d + w, oy + d, ox + 2.0 * d + 2.0 * w, oy + d + h]),
        (Face::West, [ox, oy + d, ox + d, oy + d + h]),
        (Face::East, [ox + d + w, oy + d, ox + 2.0 * d + w, oy + d + h]),
        (Face::Up, [ox + d + w, oy, ox + d + 2.0 * w, oy + d]),
        (Face::Down, [ox + d, oy, ox + d + w, oy + d]),
    ];
    for (face, rect) in faces {
        let mut verts = face_corners(face, lo, hi);
        let mut f = face;
        for _ in 0..y_steps {
            for v in verts.iter_mut() {
                *v = rot_pos_y90(*v);
            }
            f = rot_face_y90(f);
        }
        let mut uvs = [[0f32; 2]; 4];
        for (i, uv) in uvs.iter_mut().enumerate() {
            let (upx, vpx) = uv_corner(rect, i);
            *uv = [
                sprite.u0 + (sprite.u1 - sprite.u0) * (upx / tw),
                sprite.v0 + (sprite.v1 - sprite.v0) * (vpx / th),
            ];
        }
        quads.push(BakedQuad { verts, uvs, cull: None, face: f, tint: None, layer: RenderLayer::Opaque });
    }
}

/// Append a box (unit-space `lo`..`hi`) with an explicit texture-px rect per
/// face — order [Up, Down, North, South, West, East] — then rotate `y_steps`
/// quarter-turns about the block centre. Used where the texture unwrap does not
/// follow the standard box layout (e.g. the bed, whose mattress top is the
/// authored box's *front* face after vanilla's 90° lay-flat rotation).
#[allow(clippy::too_many_arguments)]
fn push_box_faces(
    quads: &mut Vec<BakedQuad>,
    lo: [f32; 3],
    hi: [f32; 3],
    rects: [[f32; 4]; 6],
    sprite: &crate::assets::atlas::AtlasSprite,
    tw: f32,
    th: f32,
    y_steps: usize,
) {
    let faces = [Face::Up, Face::Down, Face::North, Face::South, Face::West, Face::East];
    for (fi, face) in faces.into_iter().enumerate() {
        // A negative u0 marks a face to skip (hidden/untextured, e.g. bed seam).
        if rects[fi][0] < 0.0 {
            continue;
        }
        let mut verts = face_corners(face, lo, hi);
        let mut f = face;
        for _ in 0..y_steps {
            for v in verts.iter_mut() {
                *v = rot_pos_y90(*v);
            }
            f = rot_face_y90(f);
        }
        let rect = rects[fi];
        let mut uvs = [[0f32; 2]; 4];
        for (i, uv) in uvs.iter_mut().enumerate() {
            let (upx, vpx) = uv_corner(rect, i);
            *uv = [
                sprite.u0 + (sprite.u1 - sprite.u0) * (upx / tw),
                sprite.v0 + (sprite.v1 - sprite.v0) * (vpx / th),
            ];
        }
        quads.push(BakedQuad { verts, uvs, cull: None, face: f, tint: None, layer: RenderLayer::Opaque });
    }
}

/// Bake one bed half (mattress + two outer legs) from the 64×64 bed entity
/// texture. `head` picks the head vs foot texture band and orients the piece so
/// its outer (pillow/foot) end points the right way for the block's `facing`.
fn bake_bed(tex: &str, head: bool, y_steps: usize, atlas: &Atlas) -> BakedModel {
    let sprite = *atlas.sprite(tex);
    let (tw, th) = (64.0f32, 64.0f32);
    // The head piece's outer end points toward `facing`; the foot piece's points
    // the opposite way (both authored with the outer end at +Z / south). The
    // seam face (toward the other half) is hidden and untextured → skipped.
    // Head and foot occupy different, non-mirrored bands of the sheet, so their
    // per-face rects are given explicitly (measured from the bed PNG).
    let steps = if head { y_steps } else { (y_steps + 2) % 4 };
    let u = |px: f32| px / 16.0;
    let mut quads = Vec::with_capacity(18);
    const SKIP: [f32; 4] = [-1.0, 0.0, 0.0, 0.0];
    // Mattress: 0,3,0 → 16,9,16. Order [Up, Down, North(seam), South(end), W, E].
    let mattress = if head {
        [
            [6.0, 6.0, 22.0, 22.0],   // Up   = pillow + blanket surface
            [28.0, 6.0, 44.0, 22.0],  // Down = underside
            SKIP,                     // North= seam (hidden)
            [6.0, 0.0, 22.0, 6.0],    // South= outer end panel (pillow edge)
            [0.0, 6.0, 6.0, 22.0],    // West = long side
            [22.0, 6.0, 28.0, 22.0],  // East = long side
        ]
    } else {
        [
            [6.0, 28.0, 22.0, 44.0],  // Up   = blanket surface
            [28.0, 28.0, 44.0, 44.0], // Down = underside
            SKIP,                     // North= seam (hidden)
            [22.0, 22.0, 38.0, 28.0], // South= outer end panel
            [0.0, 28.0, 6.0, 44.0],   // West = long side
            [22.0, 28.0, 28.0, 44.0], // East = long side
        ]
    };
    push_box_faces(
        &mut quads,
        [u(0.0), u(3.0), u(0.0)],
        [u(16.0), u(9.0), u(16.0)],
        mattress,
        &sprite,
        tw,
        th,
        steps,
    );
    // Two legs at the outer (+Z) corners, hanging below the mattress.
    let legv = if head { 6.0 } else { 0.0 };
    let leg = [50.0, legv, 53.0, legv + 3.0];
    for (x0, x1) in [(0.0, 3.0), (13.0, 16.0)] {
        push_box_faces(
            &mut quads,
            [u(x0), u(0.0), u(13.0)],
            [u(x1), u(3.0), u(16.0)],
            [leg; 6],
            &sprite,
            tw,
            th,
            steps,
        );
    }
    BakedModel { quads, occludes: [false; 6] }
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

/// The 4 corners of an axis-aligned element face, CCW seen from outside.
/// Vertex i pairs with UV corner i (before rotation): 0=(u0,v0) 1=(u0,v1)
/// 2=(u1,v1) 3=(u1,v0) — i.e. texture-top corners are 0 and 3.
fn face_corners(face: Face, lo: [f32; 3], hi: [f32; 3]) -> [[f32; 3]; 4] {
    let [x0, y0, z0] = lo;
    let [x1, y1, z1] = hi;
    match face {
        Face::Down => [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
        Face::Up => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        Face::North => [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]],
        Face::South => [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
        Face::West => [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
        Face::East => [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]],
    }
}

/// UV rect corner c: 0=(u0,v0) 1=(u0,v1) 2=(u1,v1) 3=(u1,v0).
fn uv_corner(rect: [f32; 4], c: usize) -> (f32, f32) {
    match c {
        0 => (rect[0], rect[1]),
        1 => (rect[0], rect[3]),
        2 => (rect[2], rect[3]),
        _ => (rect[2], rect[1]),
    }
}

/// Precomputed element rotation: matrix, origin (unit space), rescale factors.
struct ElemRotParams {
    mat: [[f32; 3]; 3],
    origin: [f32; 3],
    scale: [f32; 3],
}

fn elem_rot_params(r: &ElemRot) -> ElemRotParams {
    let rad = r.angle.to_radians();
    let (s, c) = rad.sin_cos();
    // Right-handed rotation about +axis (vanilla JOML rotationAxis).
    let mat = match r.axis {
        0 => [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]],
        1 => [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]],
        _ => [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
    };
    let scale = if r.rescale && c.abs() > 1e-4 {
        let f = 1.0 / c.abs();
        match r.axis {
            0 => [1.0, f, f],
            1 => [f, 1.0, f],
            _ => [f, f, 1.0],
        }
    } else {
        [1.0; 3]
    };
    let origin = [r.origin[0] / 16.0, r.origin[1] / 16.0, r.origin[2] / 16.0];
    ElemRotParams { mat, origin, scale }
}

fn apply_elem_rot(p: [f32; 3], params: &ElemRotParams) -> [f32; 3] {
    let ElemRotParams { mat, origin, scale } = params;
    let d = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
    let mut out = [0f32; 3];
    for i in 0..3 {
        let r = mat[i][0] * d[0] + mat[i][1] * d[1] + mat[i][2] * d[2];
        out[i] = origin[i] + r * scale[i];
    }
    out
}

/// One 90° variant X step on a unit-cube position (rotation about the cube
/// center, matching vanilla's Rx(-x°): north face → down for x=90).
#[inline]
fn rot_pos_x90(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], 1.0 - p[1]]
}

/// One 90° variant Y step (vanilla Ry(-y°): north face → east for y=90).
#[inline]
fn rot_pos_y90(p: [f32; 3]) -> [f32; 3] {
    [1.0 - p[2], p[1], p[0]]
}

/// Variant rotation of a position: x applied first, then y (vanilla order).
fn rot_pos(mut p: [f32; 3], kx: usize, ky: usize) -> [f32; 3] {
    for _ in 0..kx {
        p = rot_pos_x90(p);
    }
    for _ in 0..ky {
        p = rot_pos_y90(p);
    }
    p
}

fn rot_face_x90(f: Face) -> Face {
    match f {
        Face::Down => Face::South,
        Face::South => Face::Up,
        Face::Up => Face::North,
        Face::North => Face::Down,
        other => other,
    }
}

fn rot_face_y90(f: Face) -> Face {
    match f {
        Face::North => Face::East,
        Face::East => Face::South,
        Face::South => Face::West,
        Face::West => Face::North,
        other => other,
    }
}

/// Variant rotation of a direction: x steps, then y steps.
fn rot_face(mut f: Face, kx: usize, ky: usize) -> Face {
    for _ in 0..kx {
        f = rot_face_x90(f);
    }
    for _ in 0..ky {
        f = rot_face_y90(f);
    }
    f
}

/// Dominant facing from the quad's Newell normal (CCW → outward).
/// None for degenerate (zero-area) quads.
fn dominant_face(v: &[[f32; 3]; 4]) -> Option<Face> {
    let mut n = [0f32; 3];
    for i in 0..4 {
        let a = v[i];
        let b = v[(i + 1) % 4];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let (ax, ay, az) = (n[0].abs(), n[1].abs(), n[2].abs());
    let m = ax.max(ay).max(az);
    if m < 1e-7 {
        return None;
    }
    Some(if m == ay {
        if n[1] > 0.0 { Face::Up } else { Face::Down }
    } else if m == az {
        if n[2] > 0.0 { Face::South } else { Face::North }
    } else if n[0] > 0.0 {
        Face::East
    } else {
        Face::West
    })
}

/// Does an (unrotated) element span the full 16×16 cross-section flush against
/// the block boundary on `face`? (Model coords 0..16, small epsilon.)
fn covers_face(face: Face, from: [f32; 3], to: [f32; 3]) -> bool {
    const EPS: f32 = 1e-3;
    let full = |axis: usize| from[axis] < EPS && to[axis] > 16.0 - EPS;
    match face {
        Face::Down => from[1] < EPS && full(0) && full(2),
        Face::Up => to[1] > 16.0 - EPS && full(0) && full(2),
        Face::North => from[2] < EPS && full(0) && full(1),
        Face::South => to[2] > 16.0 - EPS && full(0) && full(1),
        Face::West => from[0] < EPS && full(1) && full(2),
        Face::East => to[0] > 16.0 - EPS && full(1) && full(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ElemFace;
    use crate::types::Face;
    use image::RgbaImage;
    use std::collections::HashMap;

    fn solid(rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_fn(16, 16, |_, _| image::Rgba(rgba))
    }

    /// Tiny in-memory atlas: one opaque, one cutout, one translucent sprite.
    fn test_atlas() -> Atlas {
        let mut b = AtlasBuilder::new();
        b.add("block/opaque", solid([200, 200, 200, 255]));
        b.add("block/cutout", {
            let mut img = solid([1, 2, 3, 255]);
            img.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
            img
        });
        b.add("block/glassy", solid([0, 0, 255, 100]));
        b.build()
    }

    fn full_cube(tex: &str) -> ResolvedModel {
        let mut textures = HashMap::new();
        textures.insert("all".to_string(), tex.to_string());
        let faces = Face::ALL
            .iter()
            .map(|&f| {
                (
                    f,
                    ElemFace {
                        texture: "#all".into(),
                        uv: None,
                        cullface: Some(f),
                        rotation: 0,
                        tintindex: Some(0),
                    },
                )
            })
            .collect();
        ResolvedModel {
            textures,
            elements: vec![crate::models::Element {
                from: [0.0; 3],
                to: [16.0; 3],
                rot: None,
                faces,
            }],
            translucent_sprites: Default::default(),
        }
    }

    fn assert_ccw_outward(q: &NeutralQuad) {
        let f = dominant_face(&q.verts).expect("non-degenerate");
        assert_eq!(f, q.face, "newell normal matches recorded face");
    }

    #[test]
    fn full_cube_bakes_six_ccw_quads_and_occludes() {
        let atlas = test_atlas();
        let nm = bake_neutral(&full_cube("block/opaque"), 0, 0, &atlas);
        assert_eq!(nm.quads.len(), 6);
        assert_eq!(nm.occludes, [true; 6]);
        for q in &nm.quads {
            assert_ccw_outward(q);
            assert_eq!(q.cull, Some(q.face));
            assert!(!q.translucent && !q.has_cutout);
            // All verts on the unit cube boundary.
            for v in &q.verts {
                for c in v {
                    assert!((*c - 0.0).abs() < 1e-6 || (*c - 1.0).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn cutout_and_translucent_sprites_do_not_occlude() {
        let atlas = test_atlas();
        let nm = bake_neutral(&full_cube("block/cutout"), 0, 0, &atlas);
        assert_eq!(nm.occludes, [false; 6]);
        assert!(nm.quads.iter().all(|q| q.has_cutout));
        let nm = bake_neutral(&full_cube("block/glassy"), 0, 0, &atlas);
        assert_eq!(nm.occludes, [false; 6]);
        assert!(nm.quads.iter().all(|q| q.translucent));
    }

    #[test]
    fn variant_rotation_rotates_positions_and_cullfaces() {
        let atlas = test_atlas();
        // x=90 maps north → down (observer facing=down semantics).
        let nm = bake_neutral(&full_cube("block/opaque"), 90, 0, &atlas);
        assert_eq!(nm.occludes, [true; 6]);
        let north_count = nm.quads.iter().filter(|q| q.face == Face::North).count();
        assert_eq!(north_count, 1, "still exactly one quad per direction");
        for q in &nm.quads {
            assert_ccw_outward(q);
            assert_eq!(q.cull, Some(q.face), "cull rotated along with geometry");
        }

        // Slab occlusion moves with rotation: bottom slab x=180 → top slab.
        let mut slab = full_cube("block/opaque");
        slab.elements[0].to = [16.0, 8.0, 16.0];
        let nm0 = bake_neutral(&slab, 0, 0, &atlas);
        assert!(nm0.occludes[Face::Down as usize]);
        assert!(!nm0.occludes[Face::Up as usize]);
        let nm180 = bake_neutral(&slab, 180, 0, &atlas);
        assert!(nm180.occludes[Face::Up as usize]);
        assert!(!nm180.occludes[Face::Down as usize]);
    }

    #[test]
    fn face_rotation_math() {
        assert_eq!(rot_face(Face::North, 1, 0), Face::Down);
        assert_eq!(rot_face(Face::North, 0, 1), Face::East);
        assert_eq!(rot_face(Face::North, 0, 2), Face::South);
        assert_eq!(rot_face(Face::Up, 0, 3), Face::Up);
        assert_eq!(rot_face(Face::Down, 1, 0), Face::South);
        // x then y order: north --x90--> down --y90--> down.
        assert_eq!(rot_face(Face::North, 1, 1), Face::Down);
        // Position: block-center invariant, corner cycles.
        assert_eq!(rot_pos([0.5, 0.5, 0.5], 3, 2), [0.5, 0.5, 0.5]);
        assert_eq!(rot_pos([0.0, 0.0, 0.0], 1, 0), [0.0, 0.0, 1.0]);
        // Four applications = identity.
        let p = [0.25, 0.5, 0.75];
        assert_eq!(rot_pos(p, 4 % 4, 0), p);
        let mut q = p;
        for _ in 0..4 {
            q = rot_pos_y90(q);
        }
        for (a, b) in q.iter().zip(&p) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn uv_rotation_cycles_corners() {
        let rect = [0.0, 0.0, 16.0, 16.0];
        // rotation 0: vertex 0 gets (u0,v0).
        assert_eq!(uv_corner(rect, 0), (0.0, 0.0));
        // rotation 90 (shift 1): vertex 0 gets corner 1 = (u0,v1).
        assert_eq!(uv_corner(rect, 1), (0.0, 16.0));
        assert_eq!(uv_corner(rect, 2), (16.0, 16.0));
        assert_eq!(uv_corner(rect, 3), (16.0, 0.0));
    }

    #[test]
    fn element_rotation_45deg_rescale() {
        // The classic "cross" element: y-axis 45° with rescale stretches the
        // plane to the full block diagonal.
        let r = ElemRot { origin: [8.0, 8.0, 8.0], axis: 1, angle: 45.0, rescale: true };
        let params = elem_rot_params(&r);
        let p = apply_elem_rot([0.0, 0.0, 0.5], &params);
        // (−0.5,0,0) about y by 45°: (−cos45·0.5, 0, sin45·0.5) → rescaled by
        // 1/cos45 → (−0.5, 0, 0.5) + origin = (0, 0, 1).
        assert!((p[0] - 0.0).abs() < 1e-5, "{p:?}");
        assert!((p[1] - 0.0).abs() < 1e-5, "{p:?}");
        assert!((p[2] - 1.0).abs() < 1e-5, "{p:?}");

        // Without rescale, the point stays at radius 0.5 from the origin axis.
        let r = ElemRot { origin: [8.0, 8.0, 8.0], axis: 1, angle: 45.0, rescale: false };
        let params = elem_rot_params(&r);
        let p = apply_elem_rot([0.0, 0.5, 0.5], &params);
        let dx = p[0] - 0.5;
        let dz = p[2] - 0.5;
        assert!((dx * dx + dz * dz - 0.25).abs() < 1e-5);
        assert!((p[1] - 0.5).abs() < 1e-6, "axis component unchanged");
    }

    #[test]
    fn assemble_layers_tints_and_occlusion_vetos() {
        let atlas = test_atlas();
        let opaque = Arc::new(bake_neutral(&full_cube("block/opaque"), 0, 0, &atlas));
        let glassy = Arc::new(bake_neutral(&full_cube("block/glassy"), 0, 0, &atlas));
        let cutout = Arc::new(bake_neutral(&full_cube("block/cutout"), 0, 0, &atlas));

        // Plain block: opaque layer, occludes, tintindex present but block
        // name has no tint kind → no tint.
        let m = assemble("stone", std::slice::from_ref(&opaque));
        assert_eq!(m.occludes, [true; 6]);
        assert!(m.quads.iter().all(|q| q.layer == RenderLayer::Opaque && q.tint.is_none()));

        // Grass tint via block name + tintindex.
        let m = assemble("grass_block", std::slice::from_ref(&opaque));
        assert!(m.quads.iter().all(|q| q.tint == Some(TintKind::Grass)));

        // Leaves: cutout layer, foliage tint, never occlude.
        let m = assemble("oak_leaves", std::slice::from_ref(&cutout));
        assert_eq!(m.occludes, [false; 6]);
        assert!(m.quads.iter().all(|q| q.layer == RenderLayer::Cutout));
        assert!(m.quads.iter().all(|q| q.tint == Some(TintKind::Foliage)));

        // Translucent sprite → translucent layer, no occlusion.
        let m = assemble("some_block", std::slice::from_ref(&glassy));
        assert_eq!(m.occludes, [false; 6]);
        assert!(m.quads.iter().all(|q| q.layer == RenderLayer::Translucent));

        // Forced translucent by name even with an opaque sprite.
        for name in ["ice", "slime_block", "honey_block", "tinted_glass", "red_stained_glass_pane"] {
            let m = assemble(name, std::slice::from_ref(&opaque));
            assert!(
                m.quads.iter().all(|q| q.layer == RenderLayer::Translucent),
                "{name} forced translucent"
            );
            assert_eq!(m.occludes, [false; 6], "{name} never occludes");
        }
        // NOT forced: packed/blue ice stay opaque.
        for name in ["packed_ice", "blue_ice"] {
            let m = assemble(name, std::slice::from_ref(&opaque));
            assert!(m.quads.iter().all(|q| q.layer == RenderLayer::Opaque), "{name} opaque");
            assert_eq!(m.occludes, [true; 6]);
        }

        // Multipart union: two parts merge quads and occlusion.
        let m = assemble("stone", &[opaque.clone(), cutout.clone()]);
        assert_eq!(m.quads.len(), 12);
        assert_eq!(m.occludes, [true; 6], "opaque part still occludes");
    }

    #[test]
    fn covers_face_checks() {
        let full = ([0.0f32; 3], [16.0f32; 3]);
        for f in Face::ALL {
            assert!(covers_face(f, full.0, full.1));
        }
        let bottom_slab = ([0.0, 0.0, 0.0], [16.0, 8.0, 16.0]);
        assert!(covers_face(Face::Down, bottom_slab.0, bottom_slab.1));
        assert!(!covers_face(Face::Up, bottom_slab.0, bottom_slab.1));
        assert!(!covers_face(Face::North, bottom_slab.0, bottom_slab.1));
        let carpet = ([0.0, 0.0, 0.0], [16.0, 1.0, 16.0]);
        assert!(covers_face(Face::Down, carpet.0, carpet.1));
        assert!(!covers_face(Face::East, carpet.0, carpet.1));
    }

    #[test]
    fn fallback_cube_is_opaque_checker() {
        let atlas = test_atlas();
        let m = fallback_cube(&atlas);
        assert_eq!(m.quads.len(), 6);
        assert_eq!(m.occludes, [true; 6]);
        let missing = atlas.sprite("missing");
        for q in &m.quads {
            assert_eq!(q.layer, RenderLayer::Opaque);
            assert_eq!(q.cull, Some(q.face));
            for uv in &q.uvs {
                assert!(uv[0] >= missing.u0 - 1e-6 && uv[0] <= missing.u1 + 1e-6);
                assert!(uv[1] >= missing.v0 - 1e-6 && uv[1] <= missing.v1 + 1e-6);
            }
        }
    }

    #[test]
    fn default_uv_maps_into_sprite_rect() {
        let atlas = test_atlas();
        let nm = bake_neutral(&full_cube("block/opaque"), 0, 0, &atlas);
        let s = atlas.sprite("block/opaque");
        for q in &nm.quads {
            // Full-face default UVs must hit all four sprite corners.
            let mut have = [false; 4];
            for uv in &q.uvs {
                let at_u0 = (uv[0] - s.u0).abs() < 1e-6;
                let at_u1 = (uv[0] - s.u1).abs() < 1e-6;
                let at_v0 = (uv[1] - s.v0).abs() < 1e-6;
                let at_v1 = (uv[1] - s.v1).abs() < 1e-6;
                assert!((at_u0 || at_u1) && (at_v0 || at_v1));
                let idx = (at_u1 as usize) * 2 + (at_v1 as usize);
                have[idx] = true;
            }
            assert_eq!(have, [true; 4], "all four sprite corners used");
        }
    }

    /// Full pipeline against the real 26.1 jar + report (skipped when absent).
    #[test]
    fn real_bake_smoke() {
        const JAR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../.mc-cache/client-26.1.jar");
        const REPORT: &str =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../.mc-cache/server/generated/reports/blocks.json");
        if !std::path::Path::new(JAR).exists() || !std::path::Path::new(REPORT).exists() {
            eprintln!("skipping real_bake_smoke: .mc-cache data not present");
            return;
        }
        let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
        let mut pack = AssetPack::open(std::path::Path::new(JAR)).expect("open client jar");
        let file = std::fs::File::open(REPORT).expect("open blocks.json");
        let table = BlockTable::load(std::io::BufReader::new(file)).expect("parse report");

        let t0 = std::time::Instant::now();
        let (store, atlas) = BakedModelStore::bake_all(&mut pack, &table).expect("bake_all");
        let dt = t0.elapsed();
        assert!(dt.as_secs() < 30, "bake_all too slow: {dt:?}");

        let find = |name: &str| {
            (0..table.len() as StateId)
                .find(|&i| table.entry(i).unwrap().short_name == name)
                .unwrap_or_else(|| panic!("{name} not in report"))
        };

        // stone: full cube, 6 quads, occludes on every side.
        let stone = store.get(find("stone"));
        assert_eq!(stone.quads.len(), 6, "stone is a full cube");
        assert_eq!(stone.occludes, [true; 6]);

        // water: empty model but a real (non-degenerate) still-sprite UV rect.
        assert!(store.get(find("water")).quads.is_empty());
        assert!(atlas.has("block/water_still"));
        let wuv = store.water_still_uv();
        assert!(wuv[0] != wuv[2], "water still UV rect is degenerate");

        // grass_block (snowy=false): 10 quads (cube + 4 side overlays), the
        // up face and overlays grass-tinted via tintindex.
        let grass = (0..table.len() as StateId)
            .find(|&i| {
                let e = table.entry(i).unwrap();
                e.short_name == "grass_block" && e.prop("snowy") == Some("false")
            })
            .expect("grass_block[snowy=false] in report");
        let gb = store.get(grass);
        assert_eq!(gb.quads.len(), 10, "grass_block = cube + 4 overlay faces");
        assert!(
            gb.quads.iter().any(|q| q.tint == Some(TintKind::Grass)),
            "grass_block has grass-tinted quads"
        );

        assert!(atlas.image.width() >= 256, "atlas suspiciously small");
    }
}
