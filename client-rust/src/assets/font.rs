//! The vanilla bitmap font, loaded from the game's own font definition.
//!
//! `font/include/default.json` lists the providers that make up the default
//! font: a `space` provider giving fixed advances for whitespace, and a stack of
//! `bitmap` pages (`ascii.png`, `accented.png`, `nonlatin_european.png`), each a
//! grid of glyph cells with a character map. This module loads exactly those and
//! reproduces vanilla's glyph metrics — the advance of a bitmap glyph is its
//! rightmost non-empty column plus one, scaled by the provider's height — so
//! text laid out here lines up with the real game pixel for pixel.
//!
//! Used to draw sign text onto a texture; the HUD keeps using egui's own text.

use std::collections::HashMap;

use image::RgbaImage;

use super::AssetPack;

/// One glyph: where it lives on its page, how big its cell is and how far the
/// pen moves after drawing it.
#[derive(Clone, Copy)]
pub struct Glyph {
    /// Index of the page image in `Font::pages`.
    page: usize,
    /// Top-left of the glyph cell on the page, in pixels.
    x: u32,
    y: u32,
    /// Cell width on the page, in pixels (the height is `cell_w`'s row).
    cell_w: u32,
    /// Scale from page pixels to font pixels (`height / cell_h`).
    scale: f32,
    /// Distance from the line's top to the glyph's baseline, in font pixels.
    ascent: i32,
    /// Rendered height in font pixels (the provider's `height`, normally 8).
    height: u32,
    /// Pen advance in font pixels.
    pub advance: f32,
}

/// The default font: its glyph pages plus the character → glyph map.
pub struct Font {
    pages: Vec<RgbaImage>,
    glyphs: HashMap<char, Glyph>,
    /// Fixed advances from the `space` provider (space, thin space, …).
    spaces: HashMap<char, f32>,
}

/// Vanilla's line height — 8 pixels of glyph plus one of leading.
pub const LINE_HEIGHT: u32 = 9;

impl Font {
    /// Load the default font out of the asset pack. Missing providers are
    /// skipped, so a stripped resource pack still yields whatever it does have.
    pub fn load(pack: &mut AssetPack) -> Font {
        let mut font = Font { pages: Vec::new(), glyphs: HashMap::new(), spaces: HashMap::new() };
        // `default.json` normally just includes `font/include/default.json`;
        // read the include directly and fall back to the outer file.
        let json = pack
            .read_bytes("assets/minecraft/font/include/default.json")
            .or_else(|_| pack.read_bytes("assets/minecraft/font/default.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let Some(json) = json else { return font };
        let Some(providers) = json.get("providers").and_then(|p| p.as_array()) else {
            return font;
        };
        // Later providers lose to earlier ones for a codepoint, matching
        // vanilla's "first provider wins" lookup order.
        for p in providers {
            match p.get("type").and_then(|t| t.as_str()) {
                Some("space") => font.load_space(p),
                Some("bitmap") => font.load_bitmap(pack, p),
                _ => {}
            }
        }
        font
    }

    fn load_space(&mut self, p: &serde_json::Value) {
        let Some(advances) = p.get("advances").and_then(|a| a.as_object()) else { return };
        for (k, v) in advances {
            let (Some(c), Some(adv)) = (k.chars().next(), v.as_f64()) else { continue };
            self.spaces.entry(c).or_insert(adv as f32);
        }
    }

    fn load_bitmap(&mut self, pack: &mut AssetPack, p: &serde_json::Value) {
        let Some(file) = p.get("file").and_then(|f| f.as_str()) else { return };
        // "minecraft:font/ascii.png" → the texture ref our pack loader wants.
        let tex = file
            .strip_prefix("minecraft:")
            .unwrap_or(file)
            .strip_suffix(".png")
            .unwrap_or(file);
        let Ok(img) = pack.texture_png_raw(tex) else { return };
        let rows: Vec<Vec<char>> = match p.get("chars").and_then(|c| c.as_array()) {
            Some(rows) => rows.iter().filter_map(|r| r.as_str()).map(|r| r.chars().collect()).collect(),
            None => return,
        };
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0) as u32;
        if rows.is_empty() || cols == 0 {
            return;
        }
        let height = p.get("height").and_then(|h| h.as_u64()).unwrap_or(8) as u32;
        let ascent = p.get("ascent").and_then(|a| a.as_i64()).unwrap_or(7) as i32;
        let (cell_w, cell_h) = (img.width() / cols, img.height() / rows.len() as u32);
        if cell_w == 0 || cell_h == 0 {
            return;
        }
        let scale = height as f32 / cell_h as f32;
        let page = self.pages.len();
        for (row, chars) in rows.iter().enumerate() {
            for (col, &c) in chars.iter().enumerate() {
                // U+0000 is vanilla's "no glyph here" filler.
                if c == '\0' || self.glyphs.contains_key(&c) {
                    continue;
                }
                let (x, y) = (col as u32 * cell_w, row as u32 * cell_h);
                let advance = glyph_advance(&img, x, y, cell_w, cell_h, scale);
                self.glyphs.insert(c, Glyph { page, x, y, cell_w, scale, ascent, height, advance });
            }
        }
        self.pages.push(img);
    }

    /// True once at least one glyph page loaded.
    pub fn is_loaded(&self) -> bool {
        !self.glyphs.is_empty()
    }

    /// Pen advance for one character, in font pixels (bold adds one, like
    /// vanilla's double-draw). Unknown characters fall back to the missing-glyph
    /// width of 6.
    pub fn advance(&self, c: char, bold: bool) -> f32 {
        let base = match self.glyphs.get(&c) {
            Some(g) => g.advance,
            None => *self.spaces.get(&c).unwrap_or(&6.0),
        };
        base + if bold { 1.0 } else { 0.0 }
    }

    /// Total width of a string in font pixels.
    pub fn width(&self, text: &str, bold: bool) -> f32 {
        text.chars().map(|c| self.advance(c, bold)).sum()
    }

    /// Draw `text` into `dst` with its left edge at `x` and the line's top at
    /// `y` (both in destination pixels, 1:1 with font pixels), in `color`.
    /// Alpha-composites, so outlines can be drawn under the glyph body.
    pub fn draw(&self, dst: &mut RgbaImage, x: f32, y: i32, text: &str, color: [u8; 3], bold: bool) {
        let mut pen = x;
        for c in text.chars() {
            if let Some(g) = self.glyphs.get(&c) {
                self.blit(dst, g, pen, y, color);
                if bold {
                    // Vanilla's bold is the same glyph drawn one pixel right.
                    self.blit(dst, g, pen + 1.0, y, color);
                }
            }
            pen += self.advance(c, bold);
        }
    }

    /// One glyph, nearest-sampled from its page onto the destination.
    fn blit(&self, dst: &mut RgbaImage, g: &Glyph, x: f32, y: i32, color: [u8; 3]) {
        let page = &self.pages[g.page];
        // The glyph's top in font pixels: vanilla lines the ascents up, so a
        // 12-px accented glyph with ascent 10 hangs 3 px above an 8-px ascii
        // glyph with ascent 7.
        let top = y - (g.ascent - 7);
        let (dw, dh) = (dst.width() as i32, dst.height() as i32);
        for py in 0..g.height {
            let dy = top + py as i32;
            if dy < 0 || dy >= dh {
                continue;
            }
            let sy = g.y + (py as f32 / g.scale) as u32;
            for px in 0..(g.cell_w as f32 * g.scale).ceil() as u32 {
                let dx = (x + px as f32).round() as i32;
                if dx < 0 || dx >= dw {
                    continue;
                }
                let sx = g.x + (px as f32 / g.scale) as u32;
                if sx >= page.width() || sy >= page.height() {
                    continue;
                }
                let src = page.get_pixel(sx, sy).0;
                if src[3] == 0 {
                    continue;
                }
                // The page is a white mask; the ink colour comes from `color`.
                let a = src[3] as u32;
                let out = dst.get_pixel_mut(dx as u32, dy as u32);
                for i in 0..3 {
                    let over = color[i] as u32 * src[i] as u32 / 255;
                    out.0[i] = ((over * a + out.0[i] as u32 * (255 - a)) / 255) as u8;
                }
                out.0[3] = (a + out.0[3] as u32 * (255 - a) / 255).min(255) as u8;
            }
        }
    }
}

/// Vanilla's bitmap-glyph advance: scan for the rightmost non-transparent
/// column, scale it to font pixels and add the one-pixel gap.
fn glyph_advance(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32, scale: f32) -> f32 {
    let mut last = -1i32;
    for col in (0..w).rev() {
        let filled = (0..h).any(|row| {
            let (sx, sy) = (x + col, y + row);
            sx < img.width() && sy < img.height() && img.get_pixel(sx, sy).0[3] != 0
        });
        if filled {
            last = col as i32;
            break;
        }
    }
    (0.5 + (last + 1) as f32 * scale).floor() + 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×1 grid page: 'A' fills its 8×8 cell, 'B' only its first 3 columns.
    fn test_font() -> Font {
        let mut img = RgbaImage::new(16, 8);
        for y in 0..8 {
            for x in 0..8 {
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
            for x in 8..11 {
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
        let mut font = Font { pages: vec![img.clone()], glyphs: HashMap::new(), spaces: HashMap::new() };
        let mk = |x: u32, adv: f32| Glyph {
            page: 0, x, y: 0, cell_w: 8, scale: 1.0, ascent: 7, height: 8, advance: adv,
        };
        font.glyphs.insert('A', mk(0, glyph_advance(&img, 0, 0, 8, 8, 1.0)));
        font.glyphs.insert('B', mk(8, glyph_advance(&img, 8, 0, 8, 8, 1.0)));
        font.spaces.insert(' ', 4.0);
        font
    }

    #[test]
    fn advance_follows_the_rightmost_filled_column() {
        let f = test_font();
        assert_eq!(f.advance('A', false), 9.0); // 8 filled columns + 1
        assert_eq!(f.advance('B', false), 4.0); // 3 filled columns + 1
        assert_eq!(f.advance(' ', false), 4.0); // from the space provider
        assert_eq!(f.advance('A', true), 10.0); // bold is one wider
    }

    #[test]
    fn width_sums_advances() {
        let f = test_font();
        assert_eq!(f.width("AB A", false), 9.0 + 4.0 + 4.0 + 9.0);
    }

    #[test]
    fn draw_puts_ink_where_the_glyph_is() {
        let f = test_font();
        let mut dst = RgbaImage::new(20, 10);
        f.draw(&mut dst, 0.0, 1, "B", [255, 0, 0], false);
        assert_eq!(dst.get_pixel(0, 1).0, [255, 0, 0, 255]);
        assert_eq!(dst.get_pixel(2, 5).0, [255, 0, 0, 255]);
        // 'B' is only 3 columns wide, and nothing is drawn above the line.
        assert_eq!(dst.get_pixel(4, 5).0[3], 0);
        assert_eq!(dst.get_pixel(0, 0).0[3], 0);
    }
}
