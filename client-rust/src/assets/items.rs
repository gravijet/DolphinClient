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
        }
        let mut jobs: Vec<(String, Job)> = Vec::new();
        for name in &names {
            let model_ref = read_item_model_ref(pack, name);
            let job = match model_ref.as_deref() {
                Some(r) if strip_ns(r).starts_with("item/") => {
                    load_flat_layers(pack, r).map(Job::Flat)
                }
                Some(r) if strip_ns(r).starts_with("block/") => {
                    first_state.get(name.as_str()).copied().map(Job::Block)
                }
                _ => None,
            };
            // Fallbacks: a bare item/<name> sprite, then the block model.
            let job = job
                .or_else(|| {
                    pack.texture_png(&format!("item/{name}")).ok().map(|i| Job::Flat(vec![i]))
                })
                .or_else(|| first_state.get(name.as_str()).copied().map(Job::Block));
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
