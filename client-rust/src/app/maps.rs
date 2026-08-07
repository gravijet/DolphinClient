//! Filled maps: the server's colour indices turned into something you can look
//! at.
//!
//! A map is 128×128 *colour indices*, not pixels. The high six bits pick one of
//! vanilla's `MapColor` entries (grass green, water blue, oak brown, …) and the
//! low two bits pick a brightness — the same colour four times over, which is
//! how a map shades slopes. The client owns that table; the server only ever
//! sends indices.
//!
//! What comes out the other end is one composited image per map: the wooden
//! background, the 128×128 contents inset by vanilla's 7-pixel border, and the
//! markers (your white arrow, item frames, banners, monuments) drawn on top and
//! rotated. That single image is then used everywhere a map shows up — held in
//! your hands, inside an item frame, previewed in a cartography table.

use std::collections::HashMap;

use image::RgbaImage;

use crate::bridge::events::{MapDecoration, MapUpdate};

/// Side length of a map's colour data, in map pixels.
pub const MAP_SIZE: usize = 128;
/// Vanilla insets the contents by 7 map pixels of border on every side.
pub const BORDER: u32 = 7;
/// Side length of the composited image (border + contents + border).
pub const CANVAS: u32 = MAP_SIZE as u32 + BORDER * 2;

/// Vanilla's `MapColor` base palette, in registry order. A colour index's high
/// bits (`index >> 2`) select one of these; index 0 is "nothing here" and stays
/// transparent, which is what makes an unexplored map show the wooden back.
const BASE_COLORS: [[u8; 3]; 62] = [
    [0x00, 0x00, 0x00], // NONE
    [0x7F, 0xB2, 0x38], // GRASS
    [0xF7, 0xE9, 0xA3], // SAND
    [0xC7, 0xC7, 0xC7], // WOOL
    [0xFF, 0x00, 0x00], // FIRE
    [0xA0, 0xA0, 0xFF], // ICE
    [0xA7, 0xA7, 0xA7], // METAL
    [0x00, 0x7C, 0x00], // PLANT
    [0xFF, 0xFF, 0xFF], // SNOW
    [0xA4, 0xA8, 0xB8], // CLAY
    [0x97, 0x6D, 0x4D], // DIRT
    [0x70, 0x70, 0x70], // STONE
    [0x40, 0x40, 0xFF], // WATER
    [0x8F, 0x77, 0x48], // WOOD
    [0xFF, 0xFC, 0xF5], // QUARTZ
    [0xD8, 0x7F, 0x33], // COLOR_ORANGE
    [0xB2, 0x4C, 0xD8], // COLOR_MAGENTA
    [0x66, 0x99, 0xD8], // COLOR_LIGHT_BLUE
    [0xE5, 0xE5, 0x33], // COLOR_YELLOW
    [0x7F, 0xCC, 0x19], // COLOR_LIGHT_GREEN
    [0xF2, 0x7F, 0xA5], // COLOR_PINK
    [0x4C, 0x4C, 0x4C], // COLOR_GRAY
    [0x99, 0x99, 0x99], // COLOR_LIGHT_GRAY
    [0x4C, 0x7F, 0x99], // COLOR_CYAN
    [0x7F, 0x3F, 0xB2], // COLOR_PURPLE
    [0x33, 0x4C, 0xB2], // COLOR_BLUE
    [0x66, 0x4C, 0x33], // COLOR_BROWN
    [0x66, 0x7F, 0x33], // COLOR_GREEN
    [0x99, 0x33, 0x33], // COLOR_RED
    [0x19, 0x19, 0x19], // COLOR_BLACK
    [0xFA, 0xEE, 0x4D], // GOLD
    [0x5C, 0xDB, 0xD5], // DIAMOND
    [0x4A, 0x80, 0xFF], // LAPIS
    [0x00, 0xD9, 0x3A], // EMERALD
    [0x81, 0x56, 0x31], // PODZOL
    [0x70, 0x02, 0x00], // NETHER
    [0xD1, 0xB1, 0xA1], // TERRACOTTA_WHITE
    [0x9F, 0x52, 0x24], // TERRACOTTA_ORANGE
    [0x95, 0x57, 0x6C], // TERRACOTTA_MAGENTA
    [0x70, 0x6C, 0x8A], // TERRACOTTA_LIGHT_BLUE
    [0xBA, 0x85, 0x24], // TERRACOTTA_YELLOW
    [0x67, 0x75, 0x35], // TERRACOTTA_LIGHT_GREEN
    [0xA0, 0x4D, 0x4E], // TERRACOTTA_PINK
    [0x39, 0x29, 0x23], // TERRACOTTA_GRAY
    [0x87, 0x6B, 0x62], // TERRACOTTA_LIGHT_GRAY
    [0x57, 0x5C, 0x5C], // TERRACOTTA_CYAN
    [0x7A, 0x49, 0x58], // TERRACOTTA_PURPLE
    [0x4C, 0x3E, 0x5C], // TERRACOTTA_BLUE
    [0x4C, 0x32, 0x23], // TERRACOTTA_BROWN
    [0x4C, 0x52, 0x2A], // TERRACOTTA_GREEN
    [0x8E, 0x3C, 0x2E], // TERRACOTTA_RED
    [0x25, 0x16, 0x10], // TERRACOTTA_BLACK
    [0xBD, 0x30, 0x31], // CRIMSON_NYLIUM
    [0x94, 0x3F, 0x61], // CRIMSON_STEM
    [0x5C, 0x19, 0x1D], // CRIMSON_HYPHAE
    [0x16, 0x7E, 0x86], // WARPED_NYLIUM
    [0x3A, 0x8E, 0x8C], // WARPED_STEM
    [0x56, 0x2C, 0x3E], // WARPED_HYPHAE
    [0x14, 0xB4, 0x85], // WARPED_WART_BLOCK
    [0x64, 0x64, 0x64], // DEEPSLATE
    [0xD8, 0xAF, 0x93], // RAW_IRON
    [0x7F, 0xA7, 0x96], // GLOW_LICHEN
];

/// Vanilla's four `Brightness` levels, in the order the low two bits select
/// them. This is the shading that makes a hillside readable on a map.
const SHADES: [u32; 4] = [180, 220, 255, 135];

/// The RGBA a single map colour index resolves to. Index `0` (and anything
/// past the palette) is transparent, so the wooden back shows through.
pub fn color_of(index: u8) -> [u8; 4] {
    let base = (index >> 2) as usize;
    if base == 0 || base >= BASE_COLORS.len() {
        return [0, 0, 0, 0];
    }
    let shade = SHADES[(index & 3) as usize];
    let c = BASE_COLORS[base];
    [
        ((c[0] as u32 * shade) / 255) as u8,
        ((c[1] as u32 * shade) / 255) as u8,
        ((c[2] as u32 * shade) / 255) as u8,
        255,
    ]
}

/// One map the client knows about.
pub struct MapData {
    /// Zoom level 0..=4; one map pixel covers `1 << scale` blocks.
    pub scale: u8,
    pub locked: bool,
    /// 128×128 colour indices, row-major.
    pub colors: Box<[u8; MAP_SIZE * MAP_SIZE]>,
    pub decorations: Vec<MapDecoration>,
    /// The composited image needs rebuilding (contents or markers changed).
    pub dirty: bool,
}

impl Default for MapData {
    fn default() -> Self {
        Self {
            scale: 0,
            locked: false,
            colors: Box::new([0; MAP_SIZE * MAP_SIZE]),
            decorations: Vec::new(),
            dirty: true,
        }
    }
}

/// Every map the session has seen, keyed by map id.
#[derive(Default)]
pub struct MapStore {
    maps: HashMap<u32, MapData>,
    /// Wooden background, straight out of the jar (64×64).
    background: Option<RgbaImage>,
    /// Marker sprites by name, from `textures/map/decorations/`.
    decorations: HashMap<String, RgbaImage>,
}

impl MapStore {
    /// Hand over the textures the composite needs. Without them a map still
    /// renders — just contents on a transparent sheet, no frame or markers.
    pub fn set_textures(
        &mut self,
        background: Option<RgbaImage>,
        decorations: HashMap<String, RgbaImage>,
    ) {
        self.background = background;
        self.decorations = decorations;
        for map in self.maps.values_mut() {
            map.dirty = true;
        }
    }

    pub fn get(&self, id: u32) -> Option<&MapData> {
        self.maps.get(&id)
    }

    pub fn len(&self) -> usize {
        self.maps.len()
    }

    pub fn clear(&mut self) {
        self.maps.clear();
    }

    /// Apply one `ClientboundMapItemData`: blit the patch, replace the markers.
    pub fn apply(&mut self, update: &MapUpdate) {
        let map = self.maps.entry(update.id).or_default();
        map.scale = update.scale;
        map.locked = update.locked;
        if let Some(decorations) = &update.decorations {
            map.decorations = decorations.clone();
            map.dirty = true;
        }
        if let Some(patch) = &update.patch {
            let (w, h) = (patch.width as usize, patch.height as usize);
            for row in 0..h {
                let y = patch.start_y as usize + row;
                if y >= MAP_SIZE {
                    break;
                }
                for col in 0..w {
                    let x = patch.start_x as usize + col;
                    let Some(&value) = patch.colors.get(row * w + col) else { continue };
                    if x < MAP_SIZE {
                        map.colors[y * MAP_SIZE + x] = value;
                    }
                }
            }
            map.dirty = true;
        }
    }

    /// Ids whose composite is stale, so the app can re-upload just those.
    pub fn dirty_ids(&self) -> Vec<u32> {
        self.maps.iter().filter(|(_, m)| m.dirty).map(|(id, _)| *id).collect()
    }

    pub fn mark_clean(&mut self, id: u32) {
        if let Some(map) = self.maps.get_mut(&id) {
            map.dirty = false;
        }
    }

    /// The finished image for one map: background, contents, markers.
    pub fn compose(&self, id: u32) -> Option<RgbaImage> {
        let map = self.maps.get(&id)?;
        let mut out = RgbaImage::new(CANVAS, CANVAS);
        if let Some(bg) = &self.background {
            // Vanilla stretches the 64×64 background over the whole sheet,
            // border included — nearest so the frame keeps its hard edges.
            let bg = image::imageops::resize(
                bg,
                CANVAS,
                CANVAS,
                image::imageops::FilterType::Nearest,
            );
            image::imageops::overlay(&mut out, &bg, 0, 0);
        }
        for y in 0..MAP_SIZE {
            for x in 0..MAP_SIZE {
                let rgba = color_of(map.colors[y * MAP_SIZE + x]);
                if rgba[3] == 0 {
                    continue;
                }
                out.put_pixel(x as u32 + BORDER, y as u32 + BORDER, image::Rgba(rgba));
            }
        }
        for decor in &map.decorations {
            let Some(sprite) = self.decorations.get(decor.sprite) else { continue };
            // Vanilla puts a marker at `x/2 + 64` in map pixels and draws it
            // 8×8, spun by `rot` sixteenths of a full turn.
            let cx = decor.x as f32 / 2.0 + MAP_SIZE as f32 / 2.0 + BORDER as f32;
            let cy = decor.y as f32 / 2.0 + MAP_SIZE as f32 / 2.0 + BORDER as f32;
            let angle = (decor.rot & 15) as f32 * std::f32::consts::TAU / 16.0;
            blit_rotated(&mut out, sprite, cx, cy, angle);
        }
        Some(out)
    }
}

/// Draw `sprite` centred on `(cx, cy)`, rotated by `angle` radians, nearest
/// neighbour. Written as an inverse map over the destination rectangle so no
/// pixel of the marker can be skipped by rounding.
fn blit_rotated(dst: &mut RgbaImage, sprite: &RgbaImage, cx: f32, cy: f32, angle: f32) {
    let (sw, sh) = (sprite.width() as f32, sprite.height() as f32);
    // A rotated square needs a √2 bigger box to fit its corners.
    let reach = (sw.max(sh) * std::f32::consts::SQRT_2 / 2.0).ceil() as i32 + 1;
    let (sin, cos) = angle.sin_cos();
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            let (px, py) = (cx + dx as f32, cy + dy as f32);
            if px < 0.0 || py < 0.0 || px >= dst.width() as f32 || py >= dst.height() as f32 {
                continue;
            }
            // Rotate the destination offset back into the sprite's own frame.
            let (ox, oy) = (dx as f32 + 0.5, dy as f32 + 0.5);
            let sx = ox * cos + oy * sin + sw / 2.0;
            let sy = -ox * sin + oy * cos + sh / 2.0;
            if sx < 0.0 || sy < 0.0 || sx >= sw || sy >= sh {
                continue;
            }
            let texel = *sprite.get_pixel(sx as u32, sy as u32);
            if texel[3] == 0 {
                continue;
            }
            let base = *dst.get_pixel(px as u32, py as u32);
            let a = texel[3] as u32;
            let mix = |t: u8, b: u8| ((t as u32 * a + b as u32 * (255 - a)) / 255) as u8;
            dst.put_pixel(
                px as u32,
                py as u32,
                image::Rgba([
                    mix(texel[0], base[0]),
                    mix(texel[1], base[1]),
                    mix(texel[2], base[2]),
                    base[3].max(texel[3]),
                ]),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::events::MapPatch;

    #[test]
    fn index_zero_is_transparent() {
        for shade in 0..4u8 {
            assert_eq!(color_of(shade)[3], 0, "colour 0 shade {shade} should be empty");
        }
    }

    #[test]
    fn shades_of_one_colour_only_differ_in_brightness() {
        // Grass is base colour 1, so indices 4..=7 are its four shades.
        let lowest = color_of(7); // shade id 3 = 135/255
        let high = color_of(6); // shade id 2 = 255/255
        assert!(high[1] > lowest[1], "the bright shade should be brighter");
        // The hue is untouched: every channel scales by the same factor.
        assert_eq!(high[0] as u32 * 135 / 255, lowest[0] as u32);
        assert_eq!(high[2] as u32 * 135 / 255, lowest[2] as u32);
    }

    #[test]
    fn water_is_blue_and_grass_is_green() {
        let water = color_of(12 * 4 + 2);
        assert!(water[2] > water[0] && water[2] > water[1]);
        let grass = color_of(1 * 4 + 2);
        assert!(grass[1] > grass[0] && grass[1] > grass[2]);
    }

    #[test]
    fn a_patch_lands_where_the_server_put_it() {
        let mut store = MapStore::default();
        store.apply(&MapUpdate {
            id: 3,
            scale: 1,
            locked: false,
            decorations: None,
            patch: Some(MapPatch {
                start_x: 10,
                start_y: 20,
                width: 2,
                height: 2,
                colors: vec![4, 5, 6, 7],
            }),
        });
        let map = store.get(3).expect("map 3 exists");
        assert_eq!(map.scale, 1);
        assert_eq!(map.colors[20 * MAP_SIZE + 10], 4);
        assert_eq!(map.colors[20 * MAP_SIZE + 11], 5);
        assert_eq!(map.colors[21 * MAP_SIZE + 10], 6);
        assert_eq!(map.colors[21 * MAP_SIZE + 11], 7);
        // Everything outside the patch is untouched.
        assert_eq!(map.colors[0], 0);
    }

    #[test]
    fn a_later_patch_does_not_wipe_the_earlier_one() {
        let mut store = MapStore::default();
        let patch = |start_x, value| MapUpdate {
            id: 1,
            scale: 0,
            locked: false,
            decorations: None,
            patch: Some(MapPatch {
                start_x,
                start_y: 0,
                width: 1,
                height: 1,
                colors: vec![value],
            }),
        };
        store.apply(&patch(0, 8));
        store.apply(&patch(1, 12));
        let map = store.get(1).unwrap();
        assert_eq!(map.colors[0], 8);
        assert_eq!(map.colors[1], 12);
    }

    #[test]
    fn composing_insets_the_contents_by_the_border() {
        let mut store = MapStore::default();
        store.apply(&MapUpdate {
            id: 7,
            scale: 0,
            locked: false,
            decorations: None,
            patch: Some(MapPatch {
                start_x: 0,
                start_y: 0,
                width: 1,
                height: 1,
                // Snow (base 8) at full brightness.
                colors: vec![8 * 4 + 2],
            }),
        });
        let img = store.compose(7).expect("composed");
        assert_eq!(img.width(), CANVAS);
        assert_eq!(img.height(), CANVAS);
        assert_eq!(img.get_pixel(BORDER, BORDER).0, [255, 255, 255, 255]);
        // Just outside the contents there is no map data (no background loaded).
        assert_eq!(img.get_pixel(BORDER - 1, BORDER).0[3], 0);
    }

    #[test]
    fn markers_are_drawn_where_the_server_put_them() {
        let mut store = MapStore::default();
        let mut sprite = RgbaImage::new(8, 8);
        for p in sprite.pixels_mut() {
            *p = image::Rgba([255, 0, 0, 255]);
        }
        store.set_textures(None, HashMap::from([("player".to_string(), sprite)]));
        store.apply(&MapUpdate {
            id: 1,
            scale: 0,
            locked: false,
            decorations: Some(vec![MapDecoration {
                sprite: "player",
                x: 0,
                y: 0,
                rot: 0,
                name: None,
            }]),
            patch: None,
        });
        let img = store.compose(1).unwrap();
        // x = 0 puts the marker in the middle of the map.
        let mid = MAP_SIZE as u32 / 2 + BORDER;
        assert_eq!(img.get_pixel(mid, mid).0, [255, 0, 0, 255]);
        // …and nowhere near the corner.
        assert_eq!(img.get_pixel(BORDER, BORDER).0[3], 0);
    }
}
