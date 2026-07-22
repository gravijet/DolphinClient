//! Turning the server's biome registry into per-biome grass/foliage/water tint
//! colors, the vanilla way: an explicit `effects` override when the biome sets
//! one, otherwise the grass/foliage colormap sampled at the biome's climate
//! (temperature + downfall), then the grass-color modifier (dark forest / swamp).

use crate::bridge::events::BiomeInfo;
use crate::types::{BiomeTints, tint};
use image::RgbaImage;

/// Sample a 256×256 climate colormap (grass.png / foliage.png). `x` runs with
/// falling temperature, `y` with falling adjusted downfall — the vanilla
/// `getDefaultColor` mapping.
fn colormap_sample(img: &RgbaImage, temperature: f32, downfall: f32) -> [u8; 3] {
    let t = temperature.clamp(0.0, 1.0);
    let d = downfall.clamp(0.0, 1.0) * t;
    let x = ((1.0 - t) * 255.0) as u32;
    let y = ((1.0 - d) * 255.0) as u32;
    let x = x.min(img.width().saturating_sub(1));
    let y = y.min(img.height().saturating_sub(1));
    let p = img.get_pixel(x, y).0;
    [p[0], p[1], p[2]]
}

/// Apply the biome's `grass_color_modifier`: dark forest averages toward a deep
/// green, swamp forces the fixed swamp green (vanilla also perlin-picks a second
/// swamp shade — we use the dominant one).
fn apply_grass_mod(c: [u8; 3], modifier: u8) -> [u8; 3] {
    match modifier {
        1 => [
            ((c[0] as u32 + 0x28) / 2) as u8,
            ((c[1] as u32 + 0x34) / 2) as u8,
            ((c[2] as u32 + 0x0A) / 2) as u8,
        ],
        2 => [0x6A, 0x70, 0x39],
        _ => c,
    }
}

/// Build the per-biome tint table (indexed by protocol biome id) from the biome
/// registry and the two climate colormaps. Missing colormaps fall back to the
/// plains constants so tinting degrades gracefully.
pub fn build_biome_tints(
    biomes: &[BiomeInfo],
    grass_map: Option<&RgbaImage>,
    foliage_map: Option<&RgbaImage>,
) -> BiomeTints {
    let rows = biomes
        .iter()
        .map(|b| {
            let grass = b
                .grass_override
                .or_else(|| grass_map.map(|m| colormap_sample(m, b.temperature, b.downfall)))
                .unwrap_or(tint::GRASS);
            let grass = apply_grass_mod(grass, b.grass_modifier);
            let foliage = b
                .foliage_override
                .or_else(|| foliage_map.map(|m| colormap_sample(m, b.temperature, b.downfall)))
                .unwrap_or(tint::FOLIAGE);
            [grass, foliage, b.water]
        })
        .collect();
    BiomeTints::from_rows(rows)
}
