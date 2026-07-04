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
}

pub struct Atlas {
    pub image: RgbaImage,
    sprites: HashMap<String, AtlasSprite>,
    missing: AtlasSprite,
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

#[derive(Default)]
pub struct AtlasBuilder {
    entries: Vec<(String, RgbaImage)>,
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
        let dimf = dim as f32;
        for ((name, tile), pos) in self.entries.iter().zip(&positions) {
            let Some((x, y)) = *pos else {
                warn!("atlas: sprite {name:?} did not fit at {dim}px, mapping to missing");
                continue;
            };
            image::imageops::replace(&mut image, tile, x as i64, y as i64);
            let (opaque, has_cutout, translucent) = classify(tile);
            let (w, h) = (tile.width() as f32, tile.height() as f32);
            sprites.insert(
                name.clone(),
                AtlasSprite {
                    u0: (x as f32 + 0.5) / dimf,
                    v0: (y as f32 + 0.5) / dimf,
                    u1: (x as f32 + w - 0.5) / dimf,
                    v1: (y as f32 + h - 0.5) / dimf,
                    opaque,
                    has_cutout,
                    translucent,
                },
            );
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
            }
        });

        Atlas { image, sprites, missing }
    }
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
