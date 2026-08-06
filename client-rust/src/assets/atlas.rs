//! Texture atlas: packs all needed block textures into one RGBA image.
//! Simple shelf packing is fine (a few hundred 16×16 tiles, some 32/64px).

use image::RgbaImage;
use std::collections::{HashMap, HashSet};
use tracing::warn;

/// Name the missing-texture checker is registered under.
const MISSING_NAME: &str = "missing";
/// wgpu's guaranteed-supported default limit for 2D textures.
const MAX_DIM: u32 = 8192;

#[derive(Clone, Copy, Debug)]
pub struct AtlasSprite {
    /// Normalized atlas coordinates of the sprite rectangle.
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    /// Every texel alpha == 255.
    pub opaque: bool,
    /// Any texel alpha == 0 (needs cutout pass).
    pub has_cutout: bool,
    /// Any texel 0 < alpha < 255 (needs translucent pass).
    pub translucent: bool,
    /// Pixel rectangle in the packed atlas image (x, y, w, h) — what the
    /// animation ticker overwrites in the GPU texture.
    pub px: (u32, u32, u32, u32),
}

pub struct Atlas {
    pub image: RgbaImage,
    sprites: HashMap<String, AtlasSprite>,
    missing: AtlasSprite,
    /// Every animated sprite that landed in the atlas, ready for the ticker.
    pub animations: Vec<AnimatedSprite>,
}

impl Atlas {
    /// Sprite by normalized name ("block/stone"). Unknown names return the
    /// magenta/black checker `missing` sprite (always present).
    pub fn sprite(&self, name: &str) -> &AtlasSprite {
        self.sprites.get(name).unwrap_or(&self.missing)
    }
    pub fn has(&self, name: &str) -> bool {
        self.sprites.contains_key(name)
    }
}

// ---------------------------------------------------------------------------
// Animated sprites (`<texture>.png.mcmeta` → "animation")
// ---------------------------------------------------------------------------

/// Vanilla texture-animation metadata, as parsed from a `.png.mcmeta` sidecar.
///
/// ```json
/// { "animation": { "frametime": 2, "interpolate": true,
///                  "width": 16, "height": 32,
///                  "frames": [0, 1, {"index": 2, "time": 8}] } }
/// ```
///
/// Frames tile the sheet left-to-right then top-to-bottom in `frame_w`×`frame_h`
/// cells (vanilla sheets are a single column, but the grid form is legal).
/// Without an explicit `width`/`height` a frame is a square of `min(w, h)`,
/// which is what makes a 16×512 sheet 32 frames of 16×16.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationMeta {
    pub frame_w: u32,
    pub frame_h: u32,
    /// Playback order: (frame index into the sheet, how many ticks it shows).
    pub sequence: Vec<(u32, u32)>,
    /// Cross-fade into the next frame instead of cutting to it.
    pub interpolate: bool,
}

impl AnimationMeta {
    /// Parse the `animation` section of an mcmeta document against a sheet of
    /// `sheet_w`×`sheet_h`. Returns `None` when there is no animation section,
    /// when the sheet holds a single frame, or when the metadata is unusable.
    pub fn parse(json: &serde_json::Value, sheet_w: u32, sheet_h: u32) -> Option<Self> {
        let anim = json.get("animation")?.as_object()?;
        let num = |k: &str| anim.get(k).and_then(serde_json::Value::as_u64).map(|v| v as u32);
        // Vanilla's FrameSize default: a square of the smaller sheet dimension.
        let square = sheet_w.min(sheet_h);
        let frame_w = num("width").unwrap_or(square).max(1);
        let frame_h = num("height").unwrap_or(square).max(1);
        if frame_w > sheet_w || frame_h > sheet_h {
            warn!("atlas: animation frame {frame_w}x{frame_h} larger than its {sheet_w}x{sheet_h} sheet");
            return None;
        }
        let cols = sheet_w / frame_w;
        let rows = sheet_h / frame_h;
        let count = cols * rows;
        if count <= 1 {
            return None; // one frame: nothing to animate
        }
        let default_time = num("frametime").unwrap_or(1).max(1);

        let sequence: Vec<(u32, u32)> = match anim.get("frames").and_then(serde_json::Value::as_array) {
            Some(list) => list
                .iter()
                .filter_map(|f| match f {
                    // Bare number: the frame index, shown for `frametime` ticks.
                    serde_json::Value::Number(n) => Some((n.as_u64()? as u32, default_time)),
                    // Object form: an explicit per-frame duration.
                    serde_json::Value::Object(o) => {
                        let idx = o.get("index")?.as_u64()? as u32;
                        let t = o
                            .get("time")
                            .and_then(serde_json::Value::as_u64)
                            .map_or(default_time, |v| (v as u32).max(1));
                        Some((idx, t))
                    }
                    _ => None,
                })
                .filter(|&(idx, _)| idx < count)
                .collect(),
            // No explicit order: play every frame in sheet order.
            None => (0..count).map(|i| (i, default_time)).collect(),
        };
        if sequence.len() < 2 {
            return None;
        }
        let interpolate =
            anim.get("interpolate").and_then(serde_json::Value::as_bool).unwrap_or(false);
        Some(Self { frame_w, frame_h, sequence, interpolate })
    }

    /// Total ticks for one full cycle.
    pub fn cycle_ticks(&self) -> u32 {
        self.sequence.iter().map(|&(_, t)| t).sum::<u32>().max(1)
    }
}

/// An animated sprite as it sits in the packed atlas: where it lives in the
/// atlas image, and every frame's pixels ready to be uploaded there.
pub struct AnimatedSprite {
    pub name: String,
    /// Top-left of the sprite in the atlas image.
    pub x: u32,
    pub y: u32,
    pub meta: AnimationMeta,
    /// One tightly packed RGBA buffer per sheet frame, `frame_w`×`frame_h`.
    pub frames: Vec<Vec<u8>>,
}

/// Drives every animated sprite in an atlas and hands back the sub-rectangles
/// that changed, exactly like vanilla's per-tick sprite tickers: a frame is held
/// for its `time` ticks, then the next one is uploaded; interpolating sprites
/// re-blend and re-upload on every tick.
pub struct AtlasAnimator {
    sprites: Vec<AnimatedSprite>,
    /// Last uploaded (sequence position, sub-frame tick) per sprite; `None`
    /// until the first tick so every sprite uploads once at startup.
    last: Vec<Option<(usize, u32)>>,
    /// Scratch blend buffer, reused across sprites and ticks.
    scratch: Vec<u8>,
    tick: u64,
}

/// One texture region to re-upload: (x, y, w, h, rgba rows).
pub struct AtlasUpdate<'a> {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub rgba: &'a [u8],
}

impl AtlasAnimator {
    pub fn new(sprites: Vec<AnimatedSprite>) -> Self {
        let last = vec![None; sprites.len()];
        Self { sprites, last, scratch: Vec::new(), tick: u64::MAX }
    }

    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty()
    }

    pub fn len(&self) -> usize {
        self.sprites.len()
    }

    /// Advance every sprite to absolute game `tick` and invoke `upload` for each
    /// region whose pixels changed. Calling this twice with the same tick is a
    /// no-op, so it is safe to call once per rendered frame.
    pub fn tick(&mut self, tick: u64, mut upload: impl FnMut(AtlasUpdate<'_>)) {
        if tick == self.tick {
            return;
        }
        self.tick = tick;
        for (i, s) in self.sprites.iter().enumerate() {
            let (pos, sub) = sequence_position(&s.meta, tick);
            // A non-interpolating sprite only changes when its frame changes;
            // an interpolating one changes on every tick of the hold.
            let unchanged = match self.last[i] {
                Some((last_pos, last_sub)) => {
                    last_pos == pos && (!s.meta.interpolate || last_sub == sub)
                }
                None => false,
            };
            if unchanged {
                continue;
            }
            self.last[i] = Some((pos, sub));

            let (frame_idx, hold) = s.meta.sequence[pos];
            let Some(cur) = s.frames.get(frame_idx as usize) else { continue };
            let (w, h) = (s.meta.frame_w, s.meta.frame_h);
            let rgba: &[u8] = if s.meta.interpolate && hold > 1 {
                let next_pos = (pos + 1) % s.meta.sequence.len();
                let next_idx = s.meta.sequence[next_pos].0;
                match s.frames.get(next_idx as usize) {
                    Some(next) => {
                        blend_frames(cur, next, sub as f32 / hold as f32, &mut self.scratch);
                        &self.scratch
                    }
                    None => cur,
                }
            } else {
                cur
            };
            upload(AtlasUpdate { x: s.x, y: s.y, w, h, rgba });
        }
    }
}

/// Where absolute `tick` lands in an animation: (sequence position, ticks into
/// that frame's hold).
fn sequence_position(meta: &AnimationMeta, tick: u64) -> (usize, u32) {
    let mut t = (tick % meta.cycle_ticks() as u64) as u32;
    for (i, &(_, hold)) in meta.sequence.iter().enumerate() {
        if t < hold {
            return (i, t);
        }
        t -= hold;
    }
    (0, 0)
}

/// Linear cross-fade between two RGBA frames, written into `out`.
fn blend_frames(a: &[u8], b: &[u8], k: f32, out: &mut Vec<u8>) {
    let k = k.clamp(0.0, 1.0);
    out.clear();
    out.reserve(a.len());
    for i in 0..a.len() {
        let av = a[i] as f32;
        let bv = *b.get(i).unwrap_or(&a[i]) as f32;
        out.push((av + (bv - av) * k).round().clamp(0.0, 255.0) as u8);
    }
}

#[derive(Default)]
pub struct AtlasBuilder {
    entries: Vec<(String, RgbaImage)>,
    /// Animation sheets by sprite name; the packed tile is frame 0.
    anims: HashMap<String, (AnimationMeta, Vec<Vec<u8>>)>,
    seen: HashSet<String>,
}

impl AtlasBuilder {
    pub fn new() -> Self {
        let mut b = Self::default();
        b.add(MISSING_NAME, checker_tile());
        b
    }

    /// Register a texture under a normalized name ("block/stone").
    /// Duplicate adds are no-ops.
    pub fn add(&mut self, name: &str, img: RgbaImage) {
        if img.width() == 0 || img.height() == 0 {
            warn!("atlas: ignoring zero-sized texture {name:?}");
            return;
        }
        if !self.seen.insert(name.to_owned()) {
            return; // duplicate
        }
        self.entries.push((name.to_owned(), img));
    }

    /// Register a texture that may be an animation sheet. Without metadata this
    /// is just `add`; with it, the *first played* frame is packed into the atlas
    /// and every frame is kept so the ticker can swap them in at runtime.
    pub fn add_maybe_animated(&mut self, name: &str, sheet: RgbaImage, meta: Option<AnimationMeta>) {
        let Some(meta) = meta else {
            // Still texture. A sheet taller than wide with no usable animation
            // metadata is a stray strip: keep the top square, as before.
            let (w, h) = (sheet.width(), sheet.height());
            let img = if h > w && w > 0 {
                image::imageops::crop_imm(&sheet, 0, 0, w, w).to_image()
            } else {
                sheet
            };
            self.add(name, img);
            return;
        };
        let frames = split_frames(&sheet, &meta);
        let first = meta.sequence[0].0 as usize;
        let tile = image::imageops::crop_imm(
            &sheet,
            (first as u32 % (sheet.width() / meta.frame_w)) * meta.frame_w,
            (first as u32 / (sheet.width() / meta.frame_w)) * meta.frame_h,
            meta.frame_w,
            meta.frame_h,
        )
        .to_image();
        let fresh = !self.seen.contains(name);
        self.add(name, tile);
        if fresh {
            self.anims.insert(name.to_owned(), (meta, frames));
        }
    }

    /// Pack everything. Half-pixel-inset UVs to avoid bleeding
    /// (plus 1px padding between sprites; no mipmaps in v1).
    pub fn build(mut self) -> Atlas {
        // Guarantee the checker even if the builder was made via `default()`.
        if !self.seen.contains(MISSING_NAME) {
            self.add(MISSING_NAME, checker_tile());
        }

        // Shelf packing wants tallest-first; tie-break on name for determinism.
        self.entries.sort_by(|(na, ia), (nb, ib)| {
            ib.height().cmp(&ia.height()).then_with(|| na.cmp(nb))
        });

        let sizes: Vec<(u32, u32)> = self
            .entries
            .iter()
            .map(|(_, img)| (img.width(), img.height()))
            .collect();
        let (positions, dim) = pack_shelves(&sizes);

        let mut image = RgbaImage::new(dim, dim);
        let mut sprites = HashMap::with_capacity(self.entries.len());
        let mut anims = std::mem::take(&mut self.anims);
        let mut animations = Vec::with_capacity(anims.len());
        let dimf = dim as f32;
        for ((name, tile), pos) in self.entries.iter().zip(&positions) {
            let Some((x, y)) = *pos else {
                warn!("atlas: sprite {name:?} did not fit at {dim}px, mapping to missing");
                continue;
            };
            image::imageops::replace(&mut image, tile, x as i64, y as i64);
            // An animated sprite's alpha class must cover EVERY frame, not just
            // the one that happens to be packed — a sprite that turns cutout
            // three frames in would otherwise be meshed into the opaque pass.
            let (opaque, has_cutout, translucent) = match anims.get(name) {
                Some((_, frames)) => classify_frames(frames),
                None => classify(tile),
            };
            let (w, h) = (tile.width(), tile.height());
            sprites.insert(
                name.clone(),
                AtlasSprite {
                    u0: (x as f32 + 0.5) / dimf,
                    v0: (y as f32 + 0.5) / dimf,
                    u1: (x as f32 + w as f32 - 0.5) / dimf,
                    v1: (y as f32 + h as f32 - 0.5) / dimf,
                    opaque,
                    has_cutout,
                    translucent,
                    px: (x, y, w, h),
                },
            );
            if let Some((meta, frames)) = anims.remove(name) {
                animations.push(AnimatedSprite { name: name.clone(), x, y, meta, frames });
            }
        }

        let missing = sprites.get(MISSING_NAME).copied().unwrap_or_else(|| {
            // Only reachable if the atlas overflowed so hard the 16px checker
            // itself was dropped; degrade to a corner texel rather than panic.
            warn!("atlas: missing-texture checker was dropped in overflow");
            AtlasSprite {
                u0: 0.0,
                v0: 0.0,
                u1: 0.0,
                v1: 0.0,
                opaque: true,
                has_cutout: false,
                translucent: false,
                px: (0, 0, 1, 1),
            }
        });

        Atlas { image, sprites, missing, animations }
    }
}

/// Cut an animation sheet into per-frame RGBA buffers, in sheet order
/// (left-to-right, then top-to-bottom).
fn split_frames(sheet: &RgbaImage, meta: &AnimationMeta) -> Vec<Vec<u8>> {
    let cols = (sheet.width() / meta.frame_w).max(1);
    let rows = (sheet.height() / meta.frame_h).max(1);
    let mut out = Vec::with_capacity((cols * rows) as usize);
    for r in 0..rows {
        for c in 0..cols {
            let tile =
                image::imageops::crop_imm(sheet, c * meta.frame_w, r * meta.frame_h, meta.frame_w, meta.frame_h)
                    .to_image();
            out.push(tile.into_raw());
        }
    }
    out
}

/// `classify` across every frame of an animation: the strictest classification
/// wins, so the sprite is meshed into a pass that is correct all cycle long.
fn classify_frames(frames: &[Vec<u8>]) -> (bool, bool, bool) {
    let (mut opaque, mut has_cutout, mut translucent) = (true, false, false);
    for f in frames {
        for a in f.iter().skip(3).step_by(4) {
            match a {
                255 => {}
                0 => {
                    opaque = false;
                    has_cutout = true;
                }
                _ => {
                    opaque = false;
                    translucent = true;
                }
            }
        }
    }
    (opaque, has_cutout, translucent)
}

/// 16×16 magenta/black checker (8px quadrants), the vanilla-style fallback.
fn checker_tile() -> RgbaImage {
    RgbaImage::from_fn(16, 16, |x, y| {
        if (x / 8 + y / 8) % 2 == 0 {
            image::Rgba([255, 0, 255, 255])
        } else {
            image::Rgba([0, 0, 0, 255])
        }
    })
}

/// (opaque, has_cutout, translucent) per the alpha classification rules.
fn classify(img: &RgbaImage) -> (bool, bool, bool) {
    let mut opaque = true;
    let mut has_cutout = false;
    let mut translucent = false;
    for p in img.pixels() {
        match p.0[3] {
            255 => {}
            0 => {
                opaque = false;
                has_cutout = true;
            }
            _ => {
                opaque = false;
                translucent = true;
            }
        }
    }
    (opaque, has_cutout, translucent)
}

/// Shelf-pack `sizes` (must be sorted by height desc) into the smallest
/// power-of-two square starting at 256, doubling until everything fits
/// (capped at MAX_DIM — sprites that still don't fit come back as `None`).
/// 1px padding between sprites both horizontally and between shelves.
fn pack_shelves(sizes: &[(u32, u32)]) -> (Vec<Option<(u32, u32)>>, u32) {
    let mut dim: u32 = 256;
    loop {
        let (positions, all_fit) = pack_at(sizes, dim);
        if all_fit {
            return (positions, dim);
        }
        if dim >= MAX_DIM {
            warn!("atlas: overflow at {dim}px, dropping sprites that do not fit");
            return (positions, dim);
        }
        dim *= 2;
    }
}

/// One shelf-packing attempt at a fixed square dimension.
/// Returns per-sprite positions (`None` = did not fit) and whether all fit.
fn pack_at(sizes: &[(u32, u32)], dim: u32) -> (Vec<Option<(u32, u32)>>, bool) {
    let mut out = Vec::with_capacity(sizes.len());
    let mut all_fit = true;
    let (mut cx, mut cy, mut shelf_h) = (0u32, 0u32, 0u32);
    for &(w, h) in sizes {
        if w > dim || h > dim {
            out.push(None);
            all_fit = false;
            continue;
        }
        if cx + w > dim {
            // Start a new shelf below (1px vertical padding).
            cy += shelf_h + 1;
            cx = 0;
            shelf_h = 0;
        }
        if cy + h > dim {
            out.push(None);
            all_fit = false;
            continue;
        }
        out.push(Some((cx, cy)));
        cx += w + 1; // 1px horizontal padding
        shelf_h = shelf_h.max(h);
    }
    (out, all_fit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_fn(w, h, |_, _| image::Rgba(rgba))
    }

    #[test]
    fn shelf_pack_no_overlap() {
        // Height-desc, as the packer requires.
        let mut sizes: Vec<(u32, u32)> = Vec::new();
        sizes.extend(std::iter::repeat_n((64, 64), 3));
        sizes.extend(std::iter::repeat_n((48, 32), 4));
        sizes.extend(std::iter::repeat_n((32, 32), 10));
        sizes.extend(std::iter::repeat_n((16, 16), 200));
        let (positions, dim) = pack_shelves(&sizes);
        assert!(dim >= 256 && dim.is_power_of_two());

        let rects: Vec<(u32, u32, u32, u32)> = positions
            .iter()
            .zip(&sizes)
            .map(|(p, &(w, h))| {
                let (x, y) = p.expect("everything fits");
                assert!(x + w <= dim && y + h <= dim, "in bounds");
                (x, y, w, h)
            })
            .collect();
        for (i, &(ax, ay, aw, ah)) in rects.iter().enumerate() {
            for &(bx, by, bw, bh) in &rects[i + 1..] {
                let disjoint = ax + aw <= bx || bx + bw <= ax || ay + ah <= by || by + bh <= ay;
                assert!(disjoint, "overlap: ({ax},{ay},{aw},{ah}) vs ({bx},{by},{bw},{bh})");
            }
        }
    }

    #[test]
    fn atlas_uvs_and_pixels() {
        let mut b = AtlasBuilder::new();
        b.add("red", solid(16, 16, [255, 0, 0, 255]));
        b.add("green", solid(32, 32, [0, 255, 0, 255]));
        b.add("holey", {
            let mut img = solid(16, 16, [10, 20, 30, 255]);
            img.put_pixel(3, 3, image::Rgba([0, 0, 0, 0]));
            img
        });
        b.add("glassy", solid(16, 16, [0, 0, 255, 128]));
        // Duplicate add is a no-op.
        b.add("red", solid(16, 16, [0, 0, 0, 255]));
        let atlas = b.build();

        let (w, h) = (atlas.image.width() as f32, atlas.image.height() as f32);
        for name in ["red", "green", "holey", "glassy", "missing"] {
            assert!(atlas.has(name), "{name} in atlas");
            let s = atlas.sprite(name);
            assert!(s.u0 > 0.0 && s.v0 > 0.0 && s.u1 < 1.0 && s.v1 < 1.0);
            assert!(s.u0 < s.u1 && s.v0 < s.v1, "{name} uv order");
        }

        // Half-pixel inset: a 16px sprite spans 15px of UV.
        let red = atlas.sprite("red");
        assert!(((red.u1 - red.u0) * w - 15.0).abs() < 1e-3);

        // Sample sprite centers back out of the packed image.
        for (name, rgba) in [
            ("red", [255u8, 0, 0, 255]),
            ("green", [0, 255, 0, 255]),
            ("glassy", [0, 0, 255, 128]),
        ] {
            let s = atlas.sprite(name);
            let px = ((s.u0 + s.u1) / 2.0 * w) as u32;
            let py = ((s.v0 + s.v1) / 2.0 * h) as u32;
            assert_eq!(atlas.image.get_pixel(px, py).0, rgba, "{name} center texel");
        }

        // Alpha classification.
        assert!(atlas.sprite("red").opaque);
        let holey = atlas.sprite("holey");
        assert!(!holey.opaque && holey.has_cutout && !holey.translucent);
        let glassy = atlas.sprite("glassy");
        assert!(!glassy.opaque && !glassy.has_cutout && glassy.translucent);

        // Unknown names fall back to the checker.
        let miss = atlas.sprite("no/such/texture");
        assert!(miss.opaque);
        let mx = (miss.u0 * w) as u32;
        let my = (miss.v0 * h) as u32;
        assert_eq!(atlas.image.get_pixel(mx, my).0, [255, 0, 255, 255]);
    }

    #[test]
    fn grows_past_256() {
        let mut b = AtlasBuilder::new();
        // 300 tiles of 32px can't fit in 256²: (256/33)² = 49 per shelf grid.
        for i in 0..300 {
            b.add(&format!("t{i}"), solid(32, 32, [i as u8, 0, 0, 255]));
        }
        let atlas = b.build();
        assert!(atlas.image.width() >= 512);
        assert!(atlas.has("t299"));
    }
}
