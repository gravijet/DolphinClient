//! Inventory item icons for the HUD hotbar: one packed atlas, built once at
//! startup. Two kinds of icon, matching vanilla:
//! - Flat items (`item/*` model, e.g. sword, apple): composited from their
//!   `layerN` textures.
//! - Block items (`block/*` model, e.g. stone, stairs): software-rendered as a
//!   vanilla-style isometric cube from the already-baked block model + the
//!   block atlas (the same geometry + UVs the terrain mesher uses).
//!
//! The item model JSON (26.1 `assets/minecraft/items/<name>.json`) can wrap the
//! appearance in `select`/`condition`/`range_dispatch`/`composite`/`special`
//! nodes; we recursively pick the first `{"type":"model","model":…}` (or a
//! `base` ref) as the representative look — good enough for a hotbar icon.

use crate::assets::AssetPack;
use crate::assets::atlas::Atlas;
use crate::assets::blockmap::BlockTable;
use crate::models::{self, BakedModelStore, TintKind};
use crate::types::{StateId, tint};
use image::{Rgba, RgbaImage};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use tracing::info;

/// Output icon edge, in pixels.
const ICON: u32 = 32;
/// Supersample factor for block isometric renders (downsampled to `ICON`).
const SS: u32 = 4;
/// Internal render edge for block icons.
const RES: u32 = ICON * SS;

/// Packed grid of `ICON`×`ICON` item icons + a name→cell lookup.
pub struct ItemIcons {
    pub image: RgbaImage,
    /// item registry name (no namespace) → top-left cell coordinate in `image`.
    cells: HashMap<String, (u32, u32)>,
}

impl ItemIcons {
    pub fn len(&self) -> usize {
        self.cells.len()
    }
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Normalized atlas UV rect `[u0, v0, u1, v1]` for an item, if present.
    pub fn uv(&self, name: &str) -> Option<[f32; 4]> {
        let &(x, y) = self.cells.get(name)?;
        let (w, h) = (self.image.width() as f32, self.image.height() as f32);
        Some([x as f32 / w, y as f32 / h, (x + ICON) as f32 / w, (y + ICON) as f32 / h])
    }

    /// Debug: lay out the given items in a grid, each icon scaled `scale`× and
    /// composited over an opaque dark background (missing items → red cell).
    pub fn preview_montage(&self, names: &[&str], scale: u32) -> RgbaImage {
        let cell = ICON * scale;
        let pad = scale * 2;
        let step = cell + pad;
        let cols = 8u32;
        let rows = (names.len() as u32).div_ceil(cols);
        let mut img = RgbaImage::from_pixel(
            cols * step + pad,
            rows * step + pad,
            Rgba([32, 34, 40, 255]),
        );
        for (i, name) in names.iter().enumerate() {
            let (gx, gy) = (i as u32 % cols, i as u32 / cols);
            let (ox, oy) = (pad + gx * step, pad + gy * step);
            match self.cells.get(*name) {
                Some(&(sx, sy)) => {
                    for y in 0..cell {
                        for x in 0..cell {
                            let src = *self.image.get_pixel(sx + x / scale, sy + y / scale);
                            let dst = img.get_pixel(ox + x, oy + y).0;
                            *img.get_pixel_mut(ox + x, oy + y) = over(src.0, dst);
                        }
                    }
                }
                None => {
                    for y in 0..cell {
                        for x in 0..cell {
                            img.put_pixel(ox + x, oy + y, Rgba([120, 30, 30, 255]));
                        }
                    }
                }
            }
        }
        img
    }

    /// Build the whole icon atlas from the vanilla assets + baked block models.
    pub fn bake(
        pack: &mut AssetPack,
        table: &BlockTable,
        store: &BakedModelStore,
        atlas: &Atlas,
    ) -> ItemIcons {
        let t0 = std::time::Instant::now();

        // item registry names = basenames of assets/minecraft/items/*.json.
        let mut names: Vec<String> = pack
            .list_prefix("assets/minecraft/items/")
            .into_iter()
            .filter_map(|p| {
                p.strip_prefix("assets/minecraft/items/")
                    .and_then(|s| s.strip_suffix(".json"))
                    .map(str::to_owned)
            })
            .collect();
        names.sort();

        // First state id per block short-name (representative for block items).
        let mut first_state: HashMap<&str, StateId> = HashMap::new();
        for id in 0..table.len() as StateId {
            if let Some(e) = table.entry(id) {
                first_state.entry(e.short_name.as_str()).or_insert(id);
            }
        }

        // Resolve each item's model ref + read flat-item source textures on the
        // single-threaded AssetPack; the CPU-heavy iso raster is parallel below.
        enum Job {
            /// Flat item: pre-loaded layer textures, composited on a worker.
            Flat(Vec<RgbaImage>),
            /// Block item: render this baked state's model isometrically.
            Block(StateId),
            /// Block-entity item already rendered on the main thread.
            Ready(RgbaImage),
        }
        // A block state renders to an icon only if its baked model has geometry
        // (block-entity blocks like chests bake to nothing → handled specially).
        let solid_block = |name: &str| -> Option<StateId> {
            let id = *first_state.get(name)?;
            (!store.get(id).quads.is_empty()).then_some(id)
        };

        let mut jobs: Vec<(String, Job)> = Vec::new();
        for name in &names {
            if name == "air" {
                continue;
            }
            let model_ref = read_item_model_ref(pack, name);
            let job = match model_ref.as_deref() {
                Some(r) if strip_ns(r).starts_with("item/") => {
                    load_flat_layers(pack, r).map(Job::Flat)
                }
                Some(r) if strip_ns(r).starts_with("block/") => solid_block(name).map(Job::Block),
                _ => None,
            };
            // Fallbacks: a bare item/<name> sprite, then the block model, then a
            // dedicated block-entity render (chests, shulkers, heads, banners…)
            // so every item ends up with a real texture (no red placeholders).
            let job = job
                .or_else(|| {
                    pack.texture_png(&format!("item/{name}")).ok().map(|i| Job::Flat(vec![i]))
                })
                .or_else(|| solid_block(name).map(Job::Block))
                .or_else(|| special_icon(pack, name).map(Job::Ready));
            if let Some(job) = job {
                jobs.push((name.clone(), job));
            }
        }

        // Rasterize (parallel): each job → a 32×32 RGBA icon.
        let icons: Vec<(String, RgbaImage)> = jobs
            .into_par_iter()
            .filter_map(|(name, job)| {
                let img = match job {
                    Job::Flat(layers) => composite_flat(&layers),
                    Job::Block(id) => render_block_icon(store.get(id), &atlas.image),
                    Job::Ready(img) => Some(img),
                }?;
                Some((name, img))
            })
            .collect();

        // Pack into a square-ish grid.
        let n = icons.len().max(1) as u32;
        let cols = (n as f64).sqrt().ceil() as u32;
        let rows = n.div_ceil(cols);
        let mut image = RgbaImage::new(cols * ICON, rows * ICON);
        let mut cells = HashMap::with_capacity(icons.len());
        for (i, (name, icon)) in icons.into_iter().enumerate() {
            let (cx, cy) = (i as u32 % cols, i as u32 / cols);
            let (px, py) = (cx * ICON, cy * ICON);
            image::imageops::replace(&mut image, &icon, px as i64, py as i64);
            cells.insert(name, (px, py));
        }

        info!(
            "item icons: baked {} of {} items into {}×{} atlas in {:.2?}",
            cells.len(),
            names.len(),
            image.width(),
            image.height(),
            t0.elapsed()
        );
        if std::env::var("DOLPHIN_DUMP_MISSING").is_ok() {
            let missing: Vec<&str> =
                names.iter().map(String::as_str).filter(|n| !cells.contains_key(*n)).collect();
            eprintln!("MISSING {} items: {}", missing.len(), missing.join(", "));
        }
        ItemIcons { image, cells }
    }
}

// ---------------------------------------------------------------------------
// Item model JSON → representative model ref
// ---------------------------------------------------------------------------

fn strip_ns(r: &str) -> &str {
    r.split_once(':').map_or(r, |(_, s)| s)
}

/// Read `items/<name>.json` and pick a representative model ref: the first
/// nested `{"type":"model","model":S}`, else the first `"base":S`.
fn read_item_model_ref(pack: &mut AssetPack, name: &str) -> Option<String> {
    let bytes = pack.read_bytes(&format!("assets/minecraft/items/{name}.json")).ok()?;
    let json: Value = serde_json::from_slice(&bytes).ok()?;
    find_typed_model(&json).or_else(|| find_base(&json))
}

/// DFS for the first object of `"type": "minecraft:model"` (ns optional),
/// returning its `"model"` string.
fn find_typed_model(v: &Value) -> Option<String> {
    match v {
        Value::Object(o) => {
            if let (Some(Value::String(t)), Some(Value::String(m))) = (o.get("type"), o.get("model"))
                && strip_ns(t) == "model"
            {
                return Some(m.clone());
            }
            o.values().find_map(find_typed_model)
        }
        Value::Array(a) => a.iter().find_map(find_typed_model),
        _ => None,
    }
}

/// DFS for the first `"base": S` string (special models: chest, shield, …).
fn find_base(v: &Value) -> Option<String> {
    match v {
        Value::Object(o) => {
            if let Some(Value::String(b)) = o.get("base") {
                return Some(b.clone());
            }
            o.values().find_map(find_base)
        }
        Value::Array(a) => a.iter().find_map(find_base),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Flat items: composite layerN textures
// ---------------------------------------------------------------------------

/// Load the `layerN` textures of a flat item model, in ascending layer order.
fn load_flat_layers(pack: &mut AssetPack, model_ref: &str) -> Option<Vec<RgbaImage>> {
    let rm = models::resolve_model(&mut |r| pack.model_json(r), model_ref).ok()?;
    let mut layers: Vec<u32> = rm
        .textures
        .keys()
        .filter_map(|k| k.strip_prefix("layer").and_then(|n| n.parse::<u32>().ok()))
        .collect();
    layers.sort_unstable();
    if layers.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for n in layers {
        if let Some(tex) = models::lookup_texture(&rm.textures, &format!("#layer{n}"))
            && let Ok(img) = pack.texture_png(&tex)
        {
            out.push(img);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Alpha-composite layers (layer0 bottom) and scale to `ICON` (nearest).
fn composite_flat(layers: &[RgbaImage]) -> Option<RgbaImage> {
    let base = layers.first()?;
    let (w, h) = (base.width(), base.height());
    if w == 0 || h == 0 {
        return None;
    }
    let mut acc = RgbaImage::new(w, h);
    for layer in layers {
        if layer.width() != w || layer.height() != h {
            continue; // mismatched extra layer (rare) — skip rather than distort
        }
        for (x, y, px) in layer.enumerate_pixels() {
            *acc.get_pixel_mut(x, y) = over(px.0, acc.get_pixel(x, y).0);
        }
    }
    Some(image::imageops::resize(&acc, ICON, ICON, image::imageops::FilterType::Nearest))
}

/// Straight-alpha `src` over `dst`.
fn over(src: [u8; 4], dst: [u8; 4]) -> Rgba<u8> {
    let sa = src[3] as f32 / 255.0;
    if sa >= 1.0 {
        return Rgba(src);
    }
    if sa <= 0.0 {
        return Rgba(dst);
    }
    let da = dst[3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let mut out = [0u8; 4];
    for i in 0..3 {
        let s = src[i] as f32 * sa;
        let d = dst[i] as f32 * da * (1.0 - sa);
        out[i] = ((s + d) / oa).round().clamp(0.0, 255.0) as u8;
    }
    out[3] = (oa * 255.0).round() as u8;
    Rgba(out)
}

// ---------------------------------------------------------------------------
// Block items: isometric software render of the baked model
// ---------------------------------------------------------------------------

/// Vanilla GUI block transform: rotate 225° about Y then 30° about X. Both the
/// projection scale and screen centering are derived from the unit cube so all
/// block icons share one scale (slabs look half-height, like vanilla).
fn iso_rotate(v: [f32; 3]) -> [f32; 3] {
    // Model space is the unit cube [0,1]³; center it.
    let p = [v[0] - 0.5, v[1] - 0.5, v[2] - 0.5];
    let (sy, cy) = 225.0_f32.to_radians().sin_cos();
    let p = [cy * p[0] + sy * p[2], p[1], -sy * p[0] + cy * p[2]];
    let (sx, cx) = 30.0_f32.to_radians().sin_cos();
    [p[0], cx * p[1] - sx * p[2], sx * p[1] + cx * p[2]]
}

/// Fixed screen fit for the rotated unit cube (max |x|,|y| over its 8 corners),
/// leaving a small margin.
fn iso_fit() -> f32 {
    let mut m = 0.0f32;
    for cx in [0.0, 1.0] {
        for cy in [0.0, 1.0] {
            for cz in [0.0, 1.0] {
                let r = iso_rotate([cx, cy, cz]);
                m = m.max(r[0].abs()).max(r[1].abs());
            }
        }
    }
    (RES as f32 * 0.5 * 0.94) / m
}

fn tint_rgb(t: Option<TintKind>) -> [f32; 3] {
    let c = match t {
        None => tint::NONE,
        Some(TintKind::Grass) => tint::GRASS,
        Some(TintKind::Foliage) => tint::FOLIAGE,
        Some(TintKind::Water) => tint::WATER,
    };
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

/// Render one baked block model to a 32×32 icon. `None` if it has no geometry.
fn render_block_icon(model: &crate::models::bake::BakedModel, atlas: &RgbaImage) -> Option<RgbaImage> {
    if model.quads.is_empty() {
        return None;
    }
    let fit = iso_fit();
    let center = RES as f32 * 0.5;
    let (aw, ah) = (atlas.width() as i32, atlas.height() as i32);

    let px = (RES * RES) as usize;
    let mut color = vec![[0f32; 4]; px]; // straight-alpha nearest fragment
    let mut depth = vec![f32::NEG_INFINITY; px]; // viewer at +z: keep max z

    // Project a unit-cube vertex to screen space [sx, sy, depth].
    let project = |v: [f32; 3]| {
        let r = iso_rotate(v);
        [center + r[0] * fit, center - r[1] * fit, r[2]]
    };

    for q in &model.quads {
        let shade = q.face.shade();
        let tc = tint_rgb(q.tint);
        let p: [[f32; 3]; 4] = [
            project(q.verts[0]),
            project(q.verts[1]),
            project(q.verts[2]),
            project(q.verts[3]),
        ];
        // Two triangles: 0-1-2, 0-2-3 (matches push_quad winding).
        for tri in [[0usize, 1, 2], [0, 2, 3]] {
            raster_tri(
                [p[tri[0]], p[tri[1]], p[tri[2]]],
                [q.uvs[tri[0]], q.uvs[tri[1]], q.uvs[tri[2]]],
                shade,
                tc,
                atlas,
                (aw, ah),
                &mut color,
                &mut depth,
            );
        }
    }

    // Downsample RES→ICON with alpha-weighted averaging (no dark fringes).
    let mut out = RgbaImage::new(ICON, ICON);
    for oy in 0..ICON {
        for ox in 0..ICON {
            let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let idx = ((oy * SS + sy) * RES + (ox * SS + sx)) as usize;
                    let c = color[idx];
                    r += c[0] * c[3];
                    g += c[1] * c[3];
                    b += c[2] * c[3];
                    a += c[3];
                }
            }
            let px = if a > 0.0 {
                [
                    (r / a).round() as u8,
                    (g / a).round() as u8,
                    (b / a).round() as u8,
                    (a / (SS * SS) as f32 * 255.0).round() as u8,
                ]
            } else {
                [0, 0, 0, 0]
            };
            out.put_pixel(ox, oy, Rgba(px));
        }
    }
    // Guard against fully-empty renders (e.g. a model entirely back-facing).
    out.pixels().any(|p| p.0[3] > 0).then_some(out)
}

/// Rasterize one textured triangle into the (color, depth) buffers.
#[allow(clippy::too_many_arguments)]
fn raster_tri(
    v: [[f32; 3]; 3],
    uv: [[f32; 2]; 3],
    shade: f32,
    tint: [f32; 3],
    atlas: &RgbaImage,
    (aw, ah): (i32, i32),
    color: &mut [[f32; 4]],
    depth: &mut [f32],
) {
    let minx = v[0][0].min(v[1][0]).min(v[2][0]).floor().max(0.0) as i32;
    let maxx = v[0][0].max(v[1][0]).max(v[2][0]).ceil().min(RES as f32) as i32;
    let miny = v[0][1].min(v[1][1]).min(v[2][1]).floor().max(0.0) as i32;
    let maxy = v[0][1].max(v[1][1]).max(v[2][1]).ceil().min(RES as f32) as i32;

    // Edge-function area; skip degenerate/back-facing (either winding) tris by
    // taking the absolute area and orienting barycentrics accordingly.
    let area = edge(v[0], v[1], v[2]);
    if area.abs() < 1e-6 {
        return;
    }
    let inv = 1.0 / area;

    for y in miny..maxy {
        for x in minx..maxx {
            let p = [x as f32 + 0.5, y as f32 + 0.5, 0.0];
            let w0 = edge(v[1], v[2], p) * inv;
            let w1 = edge(v[2], v[0], p) * inv;
            let w2 = edge(v[0], v[1], p) * inv;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let z = w0 * v[0][2] + w1 * v[1][2] + w2 * v[2][2];
            let idx = (y as u32 * RES + x as u32) as usize;
            if z <= depth[idx] {
                continue;
            }
            let u = w0 * uv[0][0] + w1 * uv[1][0] + w2 * uv[2][0];
            let vv = w0 * uv[0][1] + w1 * uv[1][1] + w2 * uv[2][1];
            let tx = ((u * aw as f32) as i32).clamp(0, aw - 1);
            let ty = ((vv * ah as f32) as i32).clamp(0, ah - 1);
            let texel = atlas.get_pixel(tx as u32, ty as u32).0;
            if texel[3] == 0 {
                continue; // cutout hole
            }
            depth[idx] = z;
            color[idx] = [
                texel[0] as f32 * shade * tint[0],
                texel[1] as f32 * shade * tint[1],
                texel[2] as f32 * shade * tint[2],
                texel[3] as f32 / 255.0,
            ];
        }
    }
}

/// Twice the signed area of triangle (a, b, c) in screen space.
#[inline]
fn edge(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

// ---------------------------------------------------------------------------
// Block-entity items: chests, shulker boxes, beds, banners, skulls, conduit,
// decorated pots, copper golem statues. These use `builtin/entity` block
// models (nothing bakes), so vanilla renders them with dedicated 3D renderers
// from entity textures. We reproduce a recognizable icon from those textures:
// an isometric box (the three visible faces) or a flat billboard.
// ---------------------------------------------------------------------------

/// Crop a normalized sub-rect `[x, y, w, h]` (0..1 of the source) into a small
/// image. Empty/degenerate rects yield a 1×1 transparent pixel.
fn crop_norm(src: &RgbaImage, r: [f32; 4]) -> RgbaImage {
    let (sw, sh) = (src.width() as f32, src.height() as f32);
    let x = (r[0] * sw).round() as u32;
    let y = (r[1] * sh).round() as u32;
    let w = ((r[2] * sw).round() as u32).clamp(1, src.width().saturating_sub(x).max(1));
    let h = ((r[3] * sh).round() as u32).clamp(1, src.height().saturating_sub(y).max(1));
    image::imageops::crop_imm(src, x.min(src.width() - 1), y.min(src.height() - 1), w, h)
        .to_image()
}

/// Render an isometric box (top + the two viewer-facing sides), sharing the
/// block-icon projection so it lines up with real block icons. `size` is the
/// box extent in unit-cube space (x/z centered, y from the floor); `left` is
/// the east face (screen-left), `right` the north face (screen-right).
fn iso_box(
    size: [f32; 3],
    top: &RgbaImage,
    left: &RgbaImage,
    right: &RgbaImage,
) -> Option<RgbaImage> {
    let fit = iso_fit();
    let center = RES as f32 * 0.5;
    let n = (RES * RES) as usize;
    let mut color = vec![[0f32; 4]; n];
    let mut depth = vec![f32::NEG_INFINITY; n];
    let project = |v: [f32; 3]| {
        let r = iso_rotate(v);
        [center + r[0] * fit, center - r[1] * fit, r[2]]
    };

    // Box centered in x/z on the unit cube, resting on the floor (y ∈ 0..sy).
    let (sx, sy, sz) = (size[0], size[1], size[2]);
    let (x0, x1) = (0.5 - sx / 2.0, 0.5 + sx / 2.0);
    let (z0, z1) = (0.5 - sz / 2.0, 0.5 + sz / 2.0);
    let (y0, y1) = (0.5 - sy / 2.0, 0.5 + sy / 2.0);

    // (four corners, uv corners, shade, image) for each visible face.
    let faces: [([[f32; 3]; 4], f32, &RgbaImage); 3] = [
        // top (+y): brightest.
        ([[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]], 1.0, top),
        // east (+x): screen-left.
        ([[x1, y1, z0], [x1, y1, z1], [x1, y0, z1], [x1, y0, z0]], 0.62, left),
        // north (-z): screen-right.
        ([[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]], 0.80, right),
    ];
    let uv: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    for (verts, shade, img) in faces {
        let (iw, ih) = (img.width() as i32, img.height() as i32);
        let p = [
            project(verts[0]),
            project(verts[1]),
            project(verts[2]),
            project(verts[3]),
        ];
        for tri in [[0usize, 1, 2], [0, 2, 3]] {
            raster_tri(
                [p[tri[0]], p[tri[1]], p[tri[2]]],
                [uv[tri[0]], uv[tri[1]], uv[tri[2]]],
                shade,
                [1.0, 1.0, 1.0],
                img,
                (iw, ih),
                &mut color,
                &mut depth,
            );
        }
    }
    downsample(&color)
}

/// Downsample the RES² supersampled buffer to an `ICON` image (alpha-weighted).
fn downsample(color: &[[f32; 4]]) -> Option<RgbaImage> {
    let mut out = RgbaImage::new(ICON, ICON);
    for oy in 0..ICON {
        for ox in 0..ICON {
            let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let idx = ((oy * SS + sy) * RES + (ox * SS + sx)) as usize;
                    let c = color[idx];
                    r += c[0] * c[3];
                    g += c[1] * c[3];
                    b += c[2] * c[3];
                    a += c[3];
                }
            }
            let px = if a > 0.0 {
                [
                    (r / a).round() as u8,
                    (g / a).round() as u8,
                    (b / a).round() as u8,
                    (a / (SS * SS) as f32 * 255.0).round() as u8,
                ]
            } else {
                [0, 0, 0, 0]
            };
            out.put_pixel(ox, oy, Rgba(px));
        }
    }
    out.pixels().any(|p| p.0[3] > 0).then_some(out)
}

/// DYE colors for the 16 vanilla colors (banner/bed tint fallback).
fn dye_rgb(color: &str) -> [u8; 3] {
    match color {
        "white" => [233, 236, 236],
        "orange" => [240, 118, 19],
        "magenta" => [189, 68, 179],
        "light_blue" => [58, 175, 217],
        "yellow" => [248, 198, 39],
        "lime" => [112, 185, 25],
        "pink" => [237, 141, 172],
        "gray" => [62, 68, 71],
        "light_gray" => [142, 142, 134],
        "cyan" => [21, 137, 145],
        "purple" => [121, 42, 172],
        "blue" => [53, 57, 157],
        "brown" => [114, 71, 40],
        "green" => [84, 109, 27],
        "red" => [160, 39, 34],
        "black" => [29, 29, 33],
        _ => [160, 160, 160],
    }
}

/// Scale `src` into a centered billboard filling `fw`×`fh` fractions of the
/// icon (nearest, transparent margins). Used for flat items (shield).
fn billboard(src: &RgbaImage, fw: f32, fh: f32) -> RgbaImage {
    let (sw, sh) = (src.width().max(1), src.height().max(1));
    let dw = (ICON as f32 * fw) as u32;
    let dh = (ICON as f32 * fh) as u32;
    let ox = (ICON - dw) / 2;
    let oy = (ICON - dh) / 2;
    let mut out = RgbaImage::new(ICON, ICON);
    for y in 0..dh {
        for x in 0..dw {
            let p = *src.get_pixel((x * sw / dw).min(sw - 1), (y * sh / dh).min(sh - 1));
            if p.0[3] >= 24 {
                out.put_pixel(ox + x, oy + y, p);
            }
        }
    }
    out
}

/// A flat banner icon: the white `base.png` shape tinted by the dye color.
fn banner_icon(pack: &mut AssetPack, color: &str) -> Option<RgbaImage> {
    let base = pack
        .texture_png_raw("entity/banner/base")
        .ok()
        .or_else(|| pack.texture_png_raw("entity/banner/banner_base").ok())?;
    let [tr, tg, tb] = dye_rgb(color);
    // The banner texture's flag region is the top-left ~20×40 of a 64×64 sheet;
    // tint white pixels by the dye and drop the pole/rest.
    let flag = crop_norm(&base, [0.015, 0.015, 0.31, 0.625]);
    let mut out = RgbaImage::new(ICON, ICON);
    let (fw, fh) = (flag.width().max(1), flag.height().max(1));
    // Center the flag with a small margin.
    let dw = (ICON as f32 * 0.66) as u32;
    let dh = (ICON as f32 * 0.92) as u32;
    let ox = (ICON - dw) / 2;
    let oy = (ICON - dh) / 2;
    for y in 0..dh {
        for x in 0..dw {
            let sx = (x * fw / dw).min(fw - 1);
            let sy = (y * fh / dh).min(fh - 1);
            let p = flag.get_pixel(sx, sy).0;
            if p[3] < 24 {
                continue;
            }
            let l = p[0] as f32 / 255.0; // white texture → luminance shading
            out.put_pixel(
                ox + x,
                oy + y,
                Rgba([
                    (tr as f32 * l).round() as u8,
                    (tg as f32 * l).round() as u8,
                    (tb as f32 * l).round() as u8,
                    p[3],
                ]),
            );
        }
    }
    out.pixels().any(|p| p.0[3] > 0).then_some(out)
}

/// Icon for a block-entity item that has no bakeable block model. Returns
/// `None` for items we don't special-case (e.g. `air`).
fn special_icon(pack: &mut AssetPack, name: &str) -> Option<RgbaImage> {
    // Skulls / heads: standard mob-head cube UV (top 8,0; face 8,8; side 0,8)
    // on the entity texture. Show the face on the wide (right) face.
    let head: Option<(&str, [f32; 4], [f32; 4], [f32; 4])> = match name {
        "skeleton_skull" => Some(("entity/skeleton/skeleton", head_uv(8), head_uv(0), head_uv(24))),
        "wither_skeleton_skull" => {
            Some(("entity/skeleton/wither_skeleton", head_uv(8), head_uv(0), head_uv(24)))
        }
        "zombie_head" => Some(("entity/zombie/zombie", head_uv(8), head_uv(0), head_uv(24))),
        "creeper_head" => Some(("entity/creeper/creeper", head_uv(8), head_uv(0), head_uv(24))),
        "piglin_head" => Some(("entity/piglin/piglin", head_uv(8), head_uv(0), head_uv(24))),
        "player_head" => Some(("entity/player/wide/steve", head_uv(8), head_uv(0), head_uv(24))),
        "dragon_head" => Some(("entity/enderdragon/dragon", [0.0, 0.0, 0.5, 0.25], [0.0, 0.0, 0.25, 0.25], [0.5, 0.0, 0.5, 0.25])),
        _ => None,
    };
    if let Some((tex, top_uv, side_uv, face_uv)) = head {
        let img = pack.texture_png_raw(tex).ok()?;
        let top = crop_norm(&img, top_uv);
        let left = crop_norm(&img, side_uv);
        let right = crop_norm(&img, face_uv);
        return iso_box([0.8, 0.8, 0.8], &top, &left, &right);
    }

    // Banners: tinted flag billboard.
    if let Some(color) = name.strip_suffix("_banner") {
        return banner_icon(pack, color);
    }

    // Shield: the front plate of the shield base texture as a billboard.
    if name == "shield" {
        let img = pack
            .texture_png_raw("entity/shield/shield_base_nopattern")
            .or_else(|_| pack.texture_png_raw("entity/shield/base"))
            .ok()?;
        // Front plate ≈ (2,2)-(22,44) on the 64×64 sheet.
        let front = crop_norm(&img, [0.031, 0.031, 0.313, 0.656]);
        return Some(billboard(&front, 0.62, 0.95));
    }

    // Beds: a low box using the mattress-top region of entity/bed/<color>.png.
    if let Some(color) = name.strip_suffix("_bed") {
        if let Ok(img) = pack.texture_png_raw(&format!("entity/bed/{color}")) {
            // Head-piece: top at (6,6)-(22,22) region; side strips nearby.
            let top = crop_norm(&img, [0.09, 0.09, 0.25, 0.25]);
            let side = crop_norm(&img, [0.0, 0.34, 0.09, 0.09]);
            let end = crop_norm(&img, [0.34, 0.09, 0.09, 0.25]);
            return iso_box([0.86, 0.42, 0.86], &top, &side, &end);
        }
        let c = dye_rgb(color);
        return Some(flat_color_icon(c));
    }

    // Shulker boxes: an iso box from the shulker entity texture.
    if name == "shulker_box" || name.strip_suffix("_shulker_box").is_some() {
        let tex = match name.strip_suffix("_shulker_box") {
            Some(color) => format!("entity/shulker/shulker_{color}"),
            None => "entity/shulker/shulker".to_string(),
        };
        if let Ok(img) = pack.texture_png_raw(&tex) {
            // Shulker sheet (64×64): lid top ~(16,0)-(32,16); base sides lower.
            let top = crop_norm(&img, [0.25, 0.0, 0.25, 0.22]);
            let left = crop_norm(&img, [0.0, 0.45, 0.25, 0.28]);
            let right = crop_norm(&img, [0.25, 0.45, 0.25, 0.28]);
            return iso_box([0.82, 0.82, 0.82], &top, &left, &right);
        }
    }

    // Chests (normal, trapped, ender + copper oxidation variants): iso box
    // from the single-chest entity texture (64×64).
    if let Some(tex) = chest_texture(name) {
        if let Ok(img) = pack.texture_png_raw(&tex) {
            let top = crop_norm(&img, [0.22, 0.0, 0.22, 0.22]);
            let front = crop_norm(&img, [0.0, 0.52, 0.22, 0.30]);
            let side = crop_norm(&img, [0.68, 0.52, 0.22, 0.30]);
            return iso_box([0.86, 0.86, 0.86], &top, &side, &front);
        }
    }

    // Decorated pot: the base pottery texture wrapped on a box.
    if name == "decorated_pot" {
        if let Ok(img) = pack.texture_png_raw("entity/decorated_pot/decorated_pot_base") {
            let top = crop_norm(&img, [0.0, 0.0, 0.4, 0.4]);
            let side = crop_norm(&img, [0.0, 0.5, 0.5, 0.5]);
            return iso_box([0.7, 0.9, 0.7], &top, &side, &side);
        }
    }

    // Conduit: small pale box from the conduit base texture.
    if name == "conduit" {
        if let Ok(img) = pack.texture_png_raw("entity/conduit/base") {
            let f = crop_norm(&img, [0.0, 0.0, 0.375, 0.75]);
            return iso_box([0.55, 0.55, 0.55], &f, &f, &f);
        }
    }

    // Copper golem statue (+ oxidation/waxed variants): tint a plain box by the
    // copper oxidation stage (no per-pose texture is worth reproducing here).
    if name.contains("copper_golem_statue") {
        let c = if name.contains("oxidized") {
            [82, 138, 116]
        } else if name.contains("weathered") {
            [109, 154, 122]
        } else if name.contains("exposed") {
            [161, 125, 99]
        } else {
            [193, 107, 76]
        };
        return Some(flat_color_icon(c));
    }

    None
}

/// UV rect (normalized, 64×32 head sheet) of an 8×8 head face at texture
/// column `col` (row 8 for side faces, row 0 for the top).
fn head_uv(col: u32) -> [f32; 4] {
    // Sheet is 64 wide, 32 tall; 8px cells.
    let row = if col == 0 || col == 16 { 0.0 } else { 8.0 };
    [col as f32 / 64.0, row / 32.0, 8.0 / 64.0, 8.0 / 32.0]
}

/// Entity texture ref for a chest-family item, or `None`.
fn chest_texture(name: &str) -> Option<String> {
    let base = match name {
        "chest" => "normal",
        "trapped_chest" => "trapped",
        "ender_chest" => "ender",
        "copper_chest" => "copper",
        "exposed_copper_chest" => "copper_exposed",
        "weathered_copper_chest" => "copper_weathered",
        "oxidized_copper_chest" => "copper_oxidized",
        "waxed_copper_chest" => "copper",
        "waxed_exposed_copper_chest" => "copper_exposed",
        "waxed_weathered_copper_chest" => "copper_weathered",
        "waxed_oxidized_copper_chest" => "copper_oxidized",
        _ => return None,
    };
    Some(format!("entity/chest/{base}"))
}

/// A plain shaded iso cube in a single flat color (last-resort recognizable
/// icon for items with no usable texture).
fn flat_color_icon(rgb: [u8; 3]) -> RgbaImage {
    let shade = |m: f32| RgbaImage::from_pixel(
        1,
        1,
        Rgba([
            (rgb[0] as f32 * m) as u8,
            (rgb[1] as f32 * m) as u8,
            (rgb[2] as f32 * m) as u8,
            255,
        ]),
    );
    iso_box([0.82, 0.82, 0.82], &shade(1.0), &shade(0.62), &shade(0.8))
        .unwrap_or_else(|| RgbaImage::from_pixel(ICON, ICON, Rgba([rgb[0], rgb[1], rgb[2], 255])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_plain_model_ref() {
        let v = json!({"model": {"type": "minecraft:model", "model": "minecraft:item/apple"}});
        assert_eq!(find_typed_model(&v).as_deref(), Some("minecraft:item/apple"));
    }

    #[test]
    fn extracts_nested_and_base_refs() {
        // condition wrapping a model (bow-like).
        let v = json!({"model": {"type": "minecraft:condition",
            "on_false": {"type": "minecraft:model", "model": "minecraft:item/bow"}}});
        assert_eq!(find_typed_model(&v).as_deref(), Some("minecraft:item/bow"));
        // special model with only a base (chest-like).
        let v = json!({"model": {"type": "minecraft:special", "base": "minecraft:item/chest"}});
        assert_eq!(find_typed_model(&v), None);
        assert_eq!(find_base(&v).as_deref(), Some("minecraft:item/chest"));
    }

    #[test]
    fn over_composites_alpha() {
        // Opaque src replaces dst.
        assert_eq!(over([10, 20, 30, 255], [0, 0, 0, 255]).0, [10, 20, 30, 255]);
        // Transparent src keeps dst.
        assert_eq!(over([9, 9, 9, 0], [1, 2, 3, 255]).0, [1, 2, 3, 255]);
        // Half over opaque black → half brightness, full alpha.
        let r = over([200, 200, 200, 128], [0, 0, 0, 255]).0;
        assert!(r[0] >= 99 && r[0] <= 101 && r[3] == 255, "{r:?}");
    }

    #[test]
    fn iso_fit_positive_and_stable() {
        let f = iso_fit();
        assert!(f > 0.0 && f.is_finite());
        // A centered vertex projects to the icon center.
        let c = iso_rotate([0.5, 0.5, 0.5]);
        assert!(c[0].abs() < 1e-6 && c[1].abs() < 1e-6);
    }
}
