//! Vanilla-Minecraft UI toolkit for egui.
//!
//! Everything visual comes straight out of the 26.1 client jar so the menus
//! look exactly like the real game:
//! - the bitmap font (`font/ascii.png` + friends, parsed from
//!   `font/include/default.json` like vanilla does), with the vanilla
//!   **unifont** (unihex) fallback for characters the bitmap pages don't
//!   cover — loaded from the asset store when available,
//! - full style support (color, bold, italic, underline, strikethrough,
//!   obfuscated) for chat spans,
//! - nine-sliced widget textures (button / slider),
//! - tiled menu backgrounds (`menu_background`, `inworld_menu_background`),
//! - HUD sprites (hotbar, hearts, food, XP bar, crosshair, ping bars),
//! - container textures (inventory, chest, villager, …).
//!
//! All widget sizes are in *GUI pixels* (vanilla's 320×240 coordinate space)
//! multiplied by a GUI scale `s`, exactly like the real client.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result};
use egui::epaint::{Mesh, Vertex};
use egui::{
    Align2, Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, pos2, vec2,
};

use crate::assets::AssetPack;
use crate::bridge::events::ChatSpan;
use crate::settings::GameSettings;

/// Vanilla button metrics (GUI px).
pub const BTN_W: f32 = 200.0;
pub const BTN_H: f32 = 20.0;
/// Vertical gap between stacked menu buttons (GUI px), like vanilla lists.
pub const BTN_GAP: f32 = 4.0;
/// Width of one column in vanilla's two-column option rows.
pub const COL_W: f32 = 150.0;
/// Total width of a two-column option row (150 + 10 gap + 150).
pub const ROW_W: f32 = 310.0;

/// The DolphinClient logo (pixel-art dolphin), embedded so the title screen
/// never depends on external files.
const LOGO_PNG: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/brand/dolphin-256.png"));

/// One text line is 9 GUI px tall (8 px glyph box + 1 px leading).
pub const LINE_H: f32 = 9.0;

// ---------------------------------------------------------------------------
// Text style
// ---------------------------------------------------------------------------

/// Resolved drawing style for one run of text.
#[derive(Clone, Copy)]
pub struct TextStyle {
    pub color: Color32,
    pub bold: bool,
    pub italic: bool,
    pub underlined: bool,
    pub strikethrough: bool,
    pub obfuscated: bool,
}

impl TextStyle {
    pub fn plain(color: Color32) -> Self {
        Self {
            color,
            bold: false,
            italic: false,
            underlined: false,
            strikethrough: false,
            obfuscated: false,
        }
    }

    /// Style of a chat span (with `default` as the base color), dimmed by `alpha`.
    pub fn of_span(span: &ChatSpan, default: Color32, alpha: f32) -> Self {
        let color = span
            .color
            .map(|c| Color32::from_rgb(c[0], c[1], c[2]))
            .unwrap_or(default);
        Self {
            color: color.gamma_multiply(alpha),
            bold: span.bold,
            italic: span.italic,
            underlined: span.underlined,
            strikethrough: span.strikethrough,
            obfuscated: span.obfuscated,
        }
    }
}

// ---------------------------------------------------------------------------
// Bitmap font + unifont fallback
// ---------------------------------------------------------------------------

struct Glyph {
    /// Index into `McFont::pages`.
    page: u16,
    /// UV rect (0..1) of the glyph cell in its page texture.
    uv: Rect,
    /// Advance in GUI px (content width + 1), already provider-scaled.
    advance: f32,
    /// Rendered size in GUI px.
    w: f32,
    h: f32,
    /// Offset of the glyph-cell top from the text-line top (GUI px). Vanilla
    /// draws every provider so its `ascent` lands on the same baseline; the
    /// base line sits 7 GUI px below the line top.
    top: f32,
}

/// One unifont (unihex) glyph: 16 rows of up to 4 bytes, plus the trimmed
/// content column range (like vanilla's automatic bearing trim).
struct UniGlyph {
    bytes_per_row: usize,
    left: u16,
    right: u16,
    bits: [u8; 64],
}

/// The vanilla unihex fallback font. Pages (256 codepoints each) are rendered
/// into textures on demand.
struct Unifont {
    glyphs: HashMap<u32, UniGlyph>,
    pages: RefCell<HashMap<u32, TextureHandle>>,
}

impl Unifont {
    /// Parse a `.hex` file (lines of `XXXX:HEXDATA`, 16 rows per glyph).
    fn parse(text: &str) -> Self {
        let mut glyphs = HashMap::new();
        for line in text.lines() {
            let Some((cp, data)) = line.split_once(':') else { continue };
            let Ok(cp) = u32::from_str_radix(cp.trim(), 16) else { continue };
            let data = data.trim();
            let bytes_per_row = data.len() / 32; // 16 rows × 2 hex chars per byte
            if !(1..=4).contains(&bytes_per_row) || data.len() != bytes_per_row * 32 {
                continue;
            }
            let mut bits = [0u8; 64];
            let mut ok = true;
            for (i, chunk) in data.as_bytes().chunks(2).enumerate() {
                match u8::from_str_radix(std::str::from_utf8(chunk).unwrap_or("zz"), 16) {
                    Ok(b) => bits[i] = b,
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue;
            }
            // Trim empty columns to get the content range (vanilla behavior).
            let width = (bytes_per_row * 8) as u16;
            let (mut left, mut right) = (width, 0u16);
            for row in 0..16usize {
                for col in 0..width {
                    let byte = bits[row * bytes_per_row + (col / 8) as usize];
                    if byte & (0x80 >> (col % 8)) != 0 {
                        left = left.min(col);
                        right = right.max(col);
                    }
                }
            }
            if left > right {
                // Blank glyph (e.g. space-like): centered narrow advance.
                left = 0;
                right = width / 4;
            }
            glyphs.insert(cp, UniGlyph { bytes_per_row, left, right, bits });
        }
        Self { glyphs, pages: RefCell::new(HashMap::new()) }
    }

    fn contains(&self, cp: u32) -> bool {
        self.glyphs.contains_key(&cp)
    }

    /// Advance in GUI px: content width at half scale + 1 spacing.
    fn advance(&self, cp: u32) -> Option<f32> {
        let g = self.glyphs.get(&cp)?;
        Some((g.right - g.left + 1) as f32 / 2.0 + 1.0)
    }

    /// Texture page for the 256-codepoint block of `cp`, rendering it on
    /// first use. Pages are 512×256: 16 columns × 16 rows of 32×16 cells.
    fn page(&self, ctx: &egui::Context, cp: u32) -> Option<TextureHandle> {
        let block = cp >> 8;
        if let Some(t) = self.pages.borrow().get(&block) {
            return Some(t.clone());
        }
        // Render the block: cell (col, row) = codepoint (block<<8 | row<<4 | col).
        const CELL_W: usize = 32;
        let (w, h) = (16 * CELL_W, 16 * 16);
        let mut img = vec![0u8; w * h * 4];
        let mut any = false;
        for idx in 0..256u32 {
            let cp = (block << 8) | idx;
            let Some(g) = self.glyphs.get(&cp) else { continue };
            any = true;
            let cx = (idx & 0xF) as usize * CELL_W;
            let cy = (idx >> 4) as usize * 16;
            let width = g.bytes_per_row * 8;
            for row in 0..16usize {
                for col in 0..width.min(CELL_W) {
                    let byte = g.bits[row * g.bytes_per_row + col / 8];
                    if byte & (0x80 >> (col % 8)) != 0 {
                        let p = ((cy + row) * w + cx + col) * 4;
                        img[p..p + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
        }
        if !any {
            return None;
        }
        let color = egui::ColorImage::from_rgba_unmultiplied([w, h], &img);
        let tex = ctx.load_texture(format!("unifont-{block:x}"), color, TextureOptions::NEAREST);
        self.pages.borrow_mut().insert(block, tex.clone());
        Some(tex)
    }

    /// UV rect of the trimmed glyph content inside its page.
    fn uv(&self, cp: u32) -> Option<Rect> {
        let g = self.glyphs.get(&cp)?;
        const CELL_W: f32 = 32.0;
        let (pw, ph) = (16.0 * CELL_W, 256.0);
        let cx = (cp & 0xF) as f32 * CELL_W;
        let cy = ((cp >> 4) & 0xF) as f32 * 16.0;
        Some(Rect::from_min_max(
            pos2((cx + g.left as f32) / pw, cy / ph),
            pos2((cx + g.right as f32 + 1.0) / pw, (cy + 16.0) / ph),
        ))
    }
}

/// The Standard Galactic Alphabet sheet (`font/ascii_sga.png`): the runes an
/// enchanting table writes its offers in. Same 16×16 grid as the ASCII sheet,
/// so a glyph's cell is just its codepoint.
pub struct SgaFont {
    page: TextureHandle,
    /// Trimmed advance per ASCII codepoint, in GUI pixels.
    advance: [f32; 128],
}

impl SgaFont {
    fn load(pack: &mut AssetPack, ctx: &egui::Context) -> Option<Self> {
        let img = pack.texture_png_raw("font/ascii_sga").ok()?;
        let (cw, ch) = (img.width() / 16, img.height() / 16);
        if cw == 0 || ch == 0 {
            return None;
        }
        // Vanilla measures each bitmap glyph by its rightmost opaque column.
        let mut advance = [6.0f32; 128];
        for (code, slot) in advance.iter_mut().enumerate() {
            let (col, row) = ((code % 16) as u32, (code / 16) as u32);
            let mut right = 0u32;
            for y in 0..ch {
                for x in 0..cw {
                    if img.get_pixel(col * cw + x, row * ch + y)[3] > 0 {
                        right = right.max(x + 1);
                    }
                }
            }
            // Scale the trimmed width back to the 8-px design grid, +1 spacing.
            *slot = if right == 0 {
                4.0
            } else {
                (right as f32 * 8.0 / cw as f32).round() + 1.0
            };
        }
        let size = [img.width() as usize, img.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
        Some(Self {
            page: ctx.load_texture("font-sga", color, TextureOptions::NEAREST),
            advance,
        })
    }

    pub fn width(&self, text: &str, s: f32) -> f32 {
        text.chars()
            .map(|c| self.advance.get(c as usize).copied().unwrap_or(6.0))
            .sum::<f32>()
            * s
    }

    /// Draw `text` in runes, top-left at `pos`.
    pub fn draw(&self, painter: &egui::Painter, pos: Pos2, text: &str, s: f32, color: Color32) {
        let mut x = pos.x;
        for c in text.chars() {
            let code = c as usize;
            if code >= 128 {
                continue;
            }
            let (col, row) = ((code % 16) as f32, (code / 16) as f32);
            let uv = Rect::from_min_max(
                pos2(col / 16.0, row / 16.0),
                pos2((col + 1.0) / 16.0, (row + 1.0) / 16.0),
            );
            painter.image(
                self.page.id(),
                Rect::from_min_size(pos2(x, pos.y), vec2(8.0 * s, 8.0 * s)),
                uv,
                color,
            );
            x += self.advance[code] * s;
        }
    }
}

/// Vanilla's font, loaded from the client jar. Draws through the egui painter
/// as textured quads — pixel-perfect at any integer GUI scale. Unknown
/// characters fall back to the vanilla unifont, then to a box glyph.
pub struct McFont {
    ctx: egui::Context,
    pages: Vec<TextureHandle>,
    glyphs: HashMap<char, Glyph>,
    uni: Option<Unifont>,
    /// ASCII chars grouped by rounded advance — obfuscated (§k) picks from these.
    obf_pool: HashMap<u32, Vec<char>>,
}

impl McFont {
    fn load(pack: &mut AssetPack, ctx: &egui::Context, uni: Option<Unifont>) -> Result<Self> {
        let font_def: serde_json::Value = serde_json::from_slice(
            &pack.read_bytes("assets/minecraft/font/include/default.json")?,
        )?;
        let providers = font_def
            .get("providers")
            .and_then(|p| p.as_array())
            .context("font default.json without providers")?;

        let mut pages = Vec::new();
        let mut glyphs = HashMap::new();
        for prov in providers {
            if prov.get("type").and_then(|t| t.as_str()) != Some("bitmap") {
                continue; // space/unihex providers handled separately
            }
            let Some(file) = prov.get("file").and_then(|f| f.as_str()) else { continue };
            // "minecraft:font/ascii.png" → texture ref "font/ascii.png". Use
            // the raw loader: font atlases are taller than wide but must NOT be
            // treated as animation strips (that would drop most glyph rows).
            let tex_ref = file.strip_prefix("minecraft:").unwrap_or(file);
            let img = match pack.texture_png_raw(tex_ref) {
                Ok(i) => i,
                Err(_) => continue,
            };
            let ascent = prov.get("ascent").and_then(|a| a.as_f64()).unwrap_or(7.0) as f32;
            let height = prov.get("height").and_then(|h| h.as_f64()).unwrap_or(8.0) as f32;
            let rows: Vec<String> = prov
                .get("chars")
                .and_then(|c| c.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|r| r.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if rows.is_empty() {
                continue;
            }
            let cols = rows[0].chars().count().max(1);
            let (iw, ih) = (img.width() as f32, img.height() as f32);
            let cell_w = iw / cols as f32;
            let cell_h = ih / rows.len() as f32;
            // Vanilla renders bitmap glyphs `height` GUI px tall; width scales
            // with the same factor.
            let scale = height / cell_h;

            let page_idx = pages.len() as u16;
            for (row_i, row) in rows.iter().enumerate() {
                for (col_i, ch) in row.chars().enumerate() {
                    if ch == '\u{0}' || glyphs.contains_key(&ch) {
                        continue;
                    }
                    let x0 = col_i as f32 * cell_w;
                    let y0 = row_i as f32 * cell_h;
                    // Content width: rightmost column with an opaque pixel.
                    let mut content_w = 0u32;
                    for px in 0..cell_w as u32 {
                        let mut any = false;
                        for py in 0..cell_h as u32 {
                            let p = img.get_pixel(x0 as u32 + px, y0 as u32 + py);
                            if p.0[3] > 96 {
                                any = true;
                                break;
                            }
                        }
                        if any {
                            content_w = px + 1;
                        }
                    }
                    let advance = if content_w == 0 {
                        if ch == ' ' { 4.0 } else { continue }
                    } else {
                        content_w as f32 * scale + 1.0
                    };
                    // Inset the UV rect slightly so scaled sampling never
                    // bleeds into the neighboring glyph cell (e.g. '_' above
                    // 'o' in the ascii atlas).
                    let (ex, ey) = (0.26 / iw, 0.26 / ih);
                    glyphs.insert(
                        ch,
                        Glyph {
                            page: page_idx,
                            uv: Rect::from_min_max(
                                pos2(x0 / iw + ex, y0 / ih + ey),
                                pos2((x0 + cell_w) / iw - ex, (y0 + cell_h) / ih - ey),
                            ),
                            advance,
                            w: cell_w * scale,
                            h: height,
                            top: 7.0 - ascent,
                        },
                    );
                }
            }
            let size = [img.width() as usize, img.height() as usize];
            let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            pages.push(ctx.load_texture(format!("mcfont-{tex_ref}"), color, TextureOptions::NEAREST));
        }
        anyhow::ensure!(!glyphs.is_empty(), "no bitmap font glyphs found in jar");

        // Pool of printable ASCII glyphs per advance, for §k obfuscation.
        let mut obf_pool: HashMap<u32, Vec<char>> = HashMap::new();
        for c in '!'..='~' {
            if let Some(g) = glyphs.get(&c) {
                obf_pool.entry((g.advance * 10.0) as u32).or_default().push(c);
            }
        }

        Ok(Self { ctx: ctx.clone(), pages, glyphs, uni, obf_pool })
    }

    /// Advance of one char in GUI px (`bold` adds 1, like vanilla).
    pub fn char_advance(&self, c: char, bold: bool) -> f32 {
        let base = if let Some(g) = self.glyphs.get(&c) {
            g.advance
        } else if let Some(a) = self.uni.as_ref().and_then(|u| u.advance(c as u32)) {
            a
        } else {
            6.0 // missing-glyph box
        };
        base + if bold { 1.0 } else { 0.0 }
    }

    /// Width of `text` in points at GUI scale `s`.
    pub fn width(&self, text: &str, s: f32) -> f32 {
        self.width_styled(text, s, false)
    }

    pub fn width_styled(&self, text: &str, s: f32, bold: bool) -> f32 {
        text.chars().map(|c| self.char_advance(c, bold)).sum::<f32>() * s
    }

    /// Total width of styled spans at GUI scale `s`.
    pub fn spans_width(&self, spans: &[ChatSpan], s: f32) -> f32 {
        spans
            .iter()
            .map(|sp| self.width_styled(&sp.text, s, sp.bold))
            .sum()
    }

    /// Draw one plain line, top-left at `pos`. Returns the advance (points).
    pub fn draw(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        text: &str,
        s: f32,
        color: Color32,
        shadow: bool,
    ) -> f32 {
        self.draw_styled(painter, pos, text, s, TextStyle::plain(color), shadow, 0.0)
    }

    /// Draw one styled run. Returns the advance (points).
    pub fn draw_styled(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        text: &str,
        s: f32,
        style: TextStyle,
        shadow: bool,
        time: f64,
    ) -> f32 {
        if shadow {
            let sc = Color32::from_rgba_unmultiplied(
                (style.color.r() as u32 * 63 / 255) as u8,
                (style.color.g() as u32 * 63 / 255) as u8,
                (style.color.b() as u32 * 63 / 255) as u8,
                style.color.a(),
            );
            let shadow_style = TextStyle { color: sc, ..style };
            self.draw_run(painter, pos + vec2(s, s), text, s, shadow_style, time);
        }
        self.draw_run(painter, pos, text, s, style, time)
    }

    /// Draw styled chat spans in one line. Returns total advance (points).
    pub fn draw_spans(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        spans: &[ChatSpan],
        s: f32,
        default: Color32,
        alpha: f32,
        shadow: bool,
        time: f64,
    ) -> f32 {
        let mut x = pos.x;
        for span in spans {
            let style = TextStyle::of_span(span, default, alpha);
            x += self.draw_styled(painter, pos2(x, pos.y), &span.text, s, style, shadow, time);
        }
        x - pos.x
    }

    /// Substitute an obfuscated (§k) char with a random same-width glyph.
    fn obfuscate(&self, c: char, x: f32, time: f64) -> char {
        let adv = (self.char_advance(c, false) * 10.0) as u32;
        let Some(pool) = self.obf_pool.get(&adv) else { return c };
        if pool.is_empty() {
            return c;
        }
        // Cheap hash of (position, 20 Hz time step) → index.
        let t = (time * 20.0) as u64;
        let mut h = (x.to_bits() as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ t.wrapping_mul(0xD1B54A32D192ED03);
        h ^= h >> 31;
        pool[(h as usize) % pool.len()]
    }

    fn draw_run(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        text: &str,
        s: f32,
        style: TextStyle,
        time: f64,
    ) -> f32 {
        let mut x = pos.x;
        let color = style.color;
        for mut c in text.chars() {
            if style.obfuscated && c != ' ' {
                c = self.obfuscate(c, x, time);
            }
            let adv = self.char_advance(c, style.bold) * s;
            if c != ' ' {
                if let Some(g) = self.glyphs.get(&c) {
                    let rect = Rect::from_min_size(
                        pos2(x, pos.y + g.top * s),
                        vec2(g.w * s, g.h * s),
                    );
                    let tex = self.pages[g.page as usize].id();
                    self.glyph_quad(painter, tex, rect, g.uv, color, style.italic, s);
                    if style.bold {
                        self.glyph_quad(
                            painter,
                            tex,
                            rect.translate(vec2(s, 0.0)),
                            g.uv,
                            color,
                            style.italic,
                            s,
                        );
                    }
                } else if let Some(uni) = &self.uni
                    && uni.contains(c as u32)
                    && let Some(page) = uni.page(&self.ctx, c as u32)
                    && let Some(uv) = uni.uv(c as u32)
                {
                    let w = (adv / s - 1.0 - if style.bold { 1.0 } else { 0.0 }).max(1.0);
                    let rect = Rect::from_min_size(pos2(x, pos.y), vec2(w * s, 8.0 * s));
                    self.glyph_quad(painter, page.id(), rect, uv, color, style.italic, s);
                    if style.bold {
                        self.glyph_quad(
                            painter,
                            page.id(),
                            rect.translate(vec2(s * 0.5, 0.0)),
                            uv,
                            color,
                            style.italic,
                            s,
                        );
                    }
                } else {
                    // Missing everywhere: hollow box like vanilla's missing glyph.
                    let rect = Rect::from_min_size(pos2(x + s, pos.y + s), vec2(4.0 * s, 6.0 * s));
                    painter.rect_stroke(rect, 0.0, Stroke::new(s.max(1.0), color), StrokeKind::Inside);
                }
            }
            x += adv;
        }
        let w = x - pos.x;
        // Underline sits just below the glyph box, strike through the middle.
        if style.underlined {
            let y = pos.y + 8.0 * s;
            painter.rect_filled(
                Rect::from_min_size(pos2(pos.x - s, y), vec2(w + s, s)),
                0.0,
                color,
            );
        }
        if style.strikethrough {
            let y = pos.y + 3.0 * s;
            painter.rect_filled(
                Rect::from_min_size(pos2(pos.x - s, y), vec2(w + s, s)),
                0.0,
                color,
            );
        }
        w
    }

    /// One glyph quad; italic shears the top edge 1 GUI px right (vanilla).
    fn glyph_quad(
        &self,
        painter: &egui::Painter,
        tex: egui::TextureId,
        rect: Rect,
        uv: Rect,
        color: Color32,
        italic: bool,
        s: f32,
    ) {
        if !italic {
            painter.image(tex, rect, uv, color);
            return;
        }
        let shear = s; // 1 GUI px
        let mut mesh = Mesh::with_texture(tex);
        let idx = mesh.vertices.len() as u32;
        mesh.vertices.push(Vertex {
            pos: pos2(rect.left() + shear, rect.top()),
            uv: uv.left_top(),
            color,
        });
        mesh.vertices.push(Vertex {
            pos: pos2(rect.right() + shear, rect.top()),
            uv: uv.right_top(),
            color,
        });
        mesh.vertices.push(Vertex {
            pos: pos2(rect.right() - shear, rect.bottom()),
            uv: uv.right_bottom(),
            color,
        });
        mesh.vertices.push(Vertex {
            pos: pos2(rect.left() - shear, rect.bottom()),
            uv: uv.left_bottom(),
            color,
        });
        mesh.indices
            .extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
        painter.add(mesh);
    }

    /// Draw anchored text (e.g. centered on a button). The text box is
    /// `width × 8` GUI px, baseline-aligned like vanilla.
    pub fn draw_anchored(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        anchor: Align2,
        text: &str,
        s: f32,
        color: Color32,
        shadow: bool,
    ) {
        let w = self.width(text, s);
        let h = 8.0 * s;
        let rect = anchor.anchor_size(pos, vec2(w, h));
        self.draw(painter, rect.min, text, s, color, shadow);
    }

    /// Draw anchored styled spans.
    pub fn draw_spans_anchored(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        anchor: Align2,
        spans: &[ChatSpan],
        s: f32,
        default: Color32,
        shadow: bool,
        time: f64,
    ) {
        let w = self.spans_width(spans, s);
        let rect = anchor.anchor_size(pos, vec2(w, 8.0 * s));
        self.draw_spans(painter, rect.min, spans, s, default, 1.0, shadow, time);
    }
}

/// Try to load the vanilla unifont from the launcher asset store
/// (`assets/indexes/<id>.json` → `minecraft/font/unifont.zip` object).
fn load_unifont(assets_dir: Option<&Path>, index_id: Option<&str>) -> Option<Unifont> {
    let dir = assets_dir?;
    let index_id = index_id?;
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("indexes").join(format!("{index_id}.json"))).ok()?)
            .ok()?;
    let hash = index
        .get("objects")?
        .get("minecraft/font/unifont.zip")?
        .get("hash")?
        .as_str()?;
    let path = dir.join("objects").join(&hash[0..2]).join(hash);
    let file = std::fs::File::open(&path).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    let hex_name = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .find(|n| n.ends_with(".hex"))?;
    let mut text = String::new();
    zip.by_name(&hex_name).ok()?.read_to_string(&mut text).ok()?;
    let uni = Unifont::parse(&text);
    tracing::info!(glyphs = uni.glyphs.len(), "font: unifont fallback loaded");
    Some(uni)
}

// ---------------------------------------------------------------------------
// Textures
// ---------------------------------------------------------------------------

pub struct McTextures {
    pub button: TextureHandle,
    pub button_highlighted: TextureHandle,
    pub button_disabled: TextureHandle,
    pub slider: TextureHandle,
    pub slider_handle: TextureHandle,
    pub slider_handle_highlighted: TextureHandle,
    /// 16×16 dark tile behind out-of-game menus (25 % black — meant to sit
    /// over a panorama; we layer it over dirt instead).
    pub menu_bg: TextureHandle,
    /// 16×16 translucent tile behind in-game menus (pause/options).
    pub inworld_bg: TextureHandle,
    /// `block/dirt` — the classic Minecraft menu background, drawn dark.
    pub dirt: TextureHandle,
    pub logo: TextureHandle,
    // HUD sprites:
    pub crosshair: TextureHandle,
    pub hotbar: TextureHandle,
    pub hotbar_selection: TextureHandle,
    /// Off-hand slot frame; optional (older/partial packs may lack it).
    pub hotbar_offhand: Option<TextureHandle>,
    /// Attack-cooldown indicator (below the crosshair); optional.
    pub attack_bg: Option<TextureHandle>,
    pub attack_progress: Option<TextureHandle>,
    /// Full-charge indicator (crossed swords), shown when an attackable
    /// entity is in reach and the cooldown has recharged.
    pub attack_full: Option<TextureHandle>,
    /// Air bubbles (full / popping / empty), drawn above the hunger row while
    /// diving.
    pub air: Option<TextureHandle>,
    pub air_bursting: Option<TextureHandle>,
    pub air_empty: Option<TextureHandle>,
    /// The animated fire strip (block/fire_1, 16×N frames) for the burning
    /// screen overlay — loaded raw so all frames survive.
    pub fire: Option<TextureHandle>,
    pub heart_container: TextureHandle,
    pub heart_full: TextureHandle,
    pub heart_half: TextureHandle,
    /// Effect-tinted heart variants (poison = green, wither = black), if the
    /// vanilla sprites are present; `None` falls back to the normal red heart.
    pub heart_poison_full: Option<TextureHandle>,
    pub heart_poison_half: Option<TextureHandle>,
    pub heart_wither_full: Option<TextureHandle>,
    pub heart_wither_half: Option<TextureHandle>,
    /// Absorption ("shield") hearts, drawn gold above the health row.
    pub heart_absorb_full: Option<TextureHandle>,
    pub heart_absorb_half: Option<TextureHandle>,
    /// Frozen (powder snow) cyan hearts, shown when fully frozen.
    pub heart_frozen_full: Option<TextureHandle>,
    pub heart_frozen_half: Option<TextureHandle>,
    /// The mount's own hearts, which take the hunger bar's place while riding.
    pub heart_vehicle_full: Option<TextureHandle>,
    pub heart_vehicle_half: Option<TextureHandle>,
    pub heart_vehicle_container: Option<TextureHandle>,
    /// Powder-snow frost border overlay (`misc/powder_snow_outline`).
    pub freeze_overlay: Option<TextureHandle>,
    /// Carved-pumpkin helmet blur overlay (`misc/pumpkinblur`).
    pub pumpkin_blur: Option<TextureHandle>,
    /// Spyglass round scope overlay (`misc/spyglass_scope`).
    pub spyglass_scope: Option<TextureHandle>,
    /// Standing in a nether portal: the swirl drawn over the whole screen.
    pub portal_overlay: Option<TextureHandle>,
    /// Nausea: since 1.21 vanilla draws this sheet over the view instead of
    /// running the old confusion shader.
    pub nausea: Option<TextureHandle>,
    pub food_empty: TextureHandle,
    pub food_full: TextureHandle,
    pub food_half: TextureHandle,
    pub xp_bg: TextureHandle,
    pub xp_progress: TextureHandle,
    /// The mount's jump bar, drawn where the XP bar normally is.
    pub jump_bg: Option<TextureHandle>,
    pub jump_progress: Option<TextureHandle>,
    /// The locator bar's background (182×5) — also drawn where the XP bar
    /// normally is, whenever the player has a tracked waypoint.
    pub locator_bar_bg: Option<TextureHandle>,
    pub locator_bar_arrow_up: Option<TextureHandle>,
    pub locator_bar_arrow_down: Option<TextureHandle>,
    /// Locator dot sprites by basename (`default_0`..`default_3`, `bowtie`).
    pub locator_bar_dot: HashMap<&'static str, TextureHandle>,
    /// Ping bars: [1..5 bars], last = unknown.
    pub ping: [TextureHandle; 6],
    /// Fallback server icon for the server list.
    pub unknown_server: TextureHandle,
    /// Container GUI textures by menu kind ("player", "generic_9x3", …).
    pub containers: HashMap<&'static str, TextureHandle>,
    /// Boss-bar sprites by name (`red_background`, `notched_12_progress`, …).
    /// The 5×2 grid of colours × fill states plus the four notch overlays.
    pub boss_bar: HashMap<&'static str, TextureHandle>,
    /// Toast backgrounds and their tutorial icons, by sprite name
    /// (`advancement`, `recipe`, `system`, `tutorial`, `now_playing`, `tree`, …).
    pub toast: HashMap<&'static str, TextureHandle>,
    /// The advancements window frame (256×256) and the five tab backgrounds.
    pub advancement_window: Option<TextureHandle>,
    pub advancement_bg: HashMap<&'static str, TextureHandle>,
    /// Advancement frames, tabs and boxes from `gui/sprites/advancements/`.
    pub advancement: HashMap<&'static str, TextureHandle>,
    /// The written-book page background (`gui/book`).
    pub book: Option<TextureHandle>,
    /// Container sprites that only appear when something is happening: the
    /// furnace flame, the progress arrows, brewing bubbles, enchantment levels.
    /// Keyed by `<container>/<sprite>`, e.g. `furnace/burn_progress`.
    pub container_sprites: HashMap<&'static str, TextureHandle>,
    /// The recipe book panel and its own sprites (tabs, recipe slots, arrows).
    pub recipe_book: Option<TextureHandle>,
    pub book_sprites: HashMap<&'static str, TextureHandle>,
    /// The vanilla text-field background, used by the book's search box.
    pub text_field: Option<TextureHandle>,
}

fn load_tex(pack: &mut AssetPack, ctx: &egui::Context, tex_ref: &str) -> Result<TextureHandle> {
    let img = pack
        .texture_png(tex_ref)
        .with_context(|| format!("loading gui texture {tex_ref}"))?;
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Ok(ctx.load_texture(tex_ref.to_string(), color, TextureOptions::NEAREST))
}

// ---------------------------------------------------------------------------
// McUi
// ---------------------------------------------------------------------------

/// Loaded once at startup; shared by all menu/HUD drawing.
pub struct McUi {
    pub font: McFont,
    /// The Standard Galactic Alphabet, for the enchanting table's offers.
    pub sga: Option<SgaFont>,
    pub tex: McTextures,
    /// UI clicks this frame (buttons/sliders); the app plays the click sound
    /// and resets it. Atomic so `McUi` stays shareable in an `Arc`.
    pub clicks: AtomicU32,
    /// `misc/enchanted_glint_item.png` as raw pixels — the scrolling sprite the
    /// enchantment glint is composited from (see `glint_texture`).
    glint: Option<image::RgbaImage>,
    /// One egui texture per enchanted item currently on screen, refreshed as the
    /// glint scrolls. Behind a mutex so `McUi` stays `Sync` inside its `Arc`.
    glint_tex: Mutex<HashMap<String, TextureHandle>>,
}

impl McUi {
    pub fn load(
        pack: &mut AssetPack,
        ctx: &egui::Context,
        assets_dir: Option<&Path>,
        asset_index: Option<&str>,
    ) -> Result<Self> {
        let uni = load_unifont(assets_dir, asset_index);
        if uni.is_none() {
            tracing::info!("font: no unifont in asset store — box fallback for unknown chars");
        }
        let font = McFont::load(pack, ctx, uni)?;
        let t = |p: &mut AssetPack, r: &str| load_tex(p, ctx, r);

        let mut containers = HashMap::new();
        for (kind, path) in [
            ("player", "gui/container/inventory"),
            ("generic_9x1", "gui/container/generic_54"),
            ("generic_9x2", "gui/container/generic_54"),
            ("generic_9x3", "gui/container/generic_54"),
            ("generic_9x4", "gui/container/generic_54"),
            ("generic_9x5", "gui/container/generic_54"),
            ("generic_9x6", "gui/container/generic_54"),
            ("shulker_box", "gui/container/shulker_box"),
            ("generic_3x3", "gui/container/dispenser"),
            ("crafter_3x3", "gui/container/crafter"),
            ("crafting", "gui/container/crafting_table"),
            ("furnace", "gui/container/furnace"),
            ("smoker", "gui/container/smoker"),
            ("blast_furnace", "gui/container/blast_furnace"),
            ("hopper", "gui/container/hopper"),
            ("merchant", "gui/container/villager"),
            ("anvil", "gui/container/anvil"),
            ("beacon", "gui/container/beacon"),
            ("brewing_stand", "gui/container/brewing_stand"),
            ("enchantment", "gui/container/enchanting_table"),
            ("grindstone", "gui/container/grindstone"),
            ("loom", "gui/container/loom"),
            ("cartography_table", "gui/container/cartography_table"),
            ("smithing", "gui/container/smithing"),
            ("stonecutter", "gui/container/stonecutter"),
            // The creative menu: one 256×256 sheet per tab, the window in its
            // top-left corner.
            ("horse", "gui/container/horse"),
            ("creative_search", "gui/container/creative_inventory/tab_item_search"),
            ("creative_items", "gui/container/creative_inventory/tab_items"),
        ] {
            if let Ok(tex) = t(pack, path) {
                containers.insert(kind, tex);
            }
        }

        // Boss bars: one background/progress pair per colour, plus the four
        // notch overlays that get drawn on top of both.
        let mut boss_bar = HashMap::new();
        for name in [
            "pink_background", "pink_progress",
            "blue_background", "blue_progress",
            "red_background", "red_progress",
            "green_background", "green_progress",
            "yellow_background", "yellow_progress",
            "purple_background", "purple_progress",
            "white_background", "white_progress",
            "notched_6_background", "notched_6_progress",
            "notched_10_background", "notched_10_progress",
            "notched_12_background", "notched_12_progress",
            "notched_20_background", "notched_20_progress",
        ] {
            if let Ok(tex) = t(pack, &format!("gui/sprites/boss_bar/{name}")) {
                boss_bar.insert(name, tex);
            }
        }

        // Toasts: the five backgrounds plus the icons the tutorial ones show.
        let mut toast = HashMap::new();
        for name in [
            "advancement", "recipe", "system", "tutorial", "now_playing",
            "mouse", "movement_keys", "recipe_book", "right_click",
            "social_interactions", "tree", "wooden_planks",
        ] {
            if let Ok(tex) = t(pack, &format!("gui/sprites/toast/{name}")) {
                toast.insert(name, tex);
            }
        }

        // The advancements screen: the window frame, one tiled background per
        // root tab, and the frames/tabs/boxes drawn inside it.
        let advancement_window = t(pack, "gui/advancements/window").ok();
        let mut advancement_bg = HashMap::new();
        for name in ["adventure", "end", "husbandry", "nether", "stone"] {
            if let Ok(tex) = t(pack, &format!("gui/advancements/backgrounds/{name}")) {
                advancement_bg.insert(name, tex);
            }
        }
        let mut advancement = HashMap::new();
        for name in [
            "task_frame_obtained", "task_frame_unobtained",
            "goal_frame_obtained", "goal_frame_unobtained",
            "challenge_frame_obtained", "challenge_frame_unobtained",
            "box_obtained", "box_unobtained", "title_box",
            "tab_above_left", "tab_above_left_selected",
            "tab_above_middle", "tab_above_middle_selected",
            "tab_above_right", "tab_above_right_selected",
        ] {
            if let Ok(tex) = t(pack, &format!("gui/sprites/advancements/{name}")) {
                advancement.insert(name, tex);
            }
        }

        // Container sprites that only show while something is happening.
        let mut container_sprites = HashMap::new();
        for name in [
            "furnace/burn_progress", "furnace/lit_progress",
            "smoker/burn_progress", "smoker/lit_progress",
            "blast_furnace/burn_progress", "blast_furnace/lit_progress",
            "brewing_stand/brew_progress", "brewing_stand/bubbles",
            "brewing_stand/fuel_length",
            "enchanting_table/enchantment_slot",
            "enchanting_table/enchantment_slot_disabled",
            "enchanting_table/enchantment_slot_highlighted",
            "enchanting_table/level_1", "enchanting_table/level_1_disabled",
            "enchanting_table/level_2", "enchanting_table/level_2_disabled",
            "enchanting_table/level_3", "enchanting_table/level_3_disabled",
            "anvil/text_field", "anvil/error",
            "stonecutter/recipe", "stonecutter/recipe_selected",
            "stonecutter/recipe_highlighted", "stonecutter/scroller",
            "stonecutter/scroller_disabled",
            "loom/pattern", "loom/pattern_selected", "loom/pattern_highlighted",
            "loom/scroller", "loom/scroller_disabled", "loom/error",
            "beacon/button", "beacon/button_selected", "beacon/button_highlighted",
            "beacon/button_disabled", "beacon/confirm", "beacon/cancel",
            "cartography_table/map", "cartography_table/scaled_map",
            "cartography_table/duplicated_map", "cartography_table/locked",
            // The creative menu's scrollbar and the two tabs we show.
            "creative_inventory/scroller", "creative_inventory/scroller_disabled",
            "creative_inventory/tab_top_selected_7", "creative_inventory/tab_top_unselected_7",
            "creative_inventory/tab_bottom_selected_7",
            "creative_inventory/tab_bottom_unselected_7",
            // The mount screen: the chest grid it draws only when the animal
            // carries one, and the ghost items in its two equipment slots.
            "horse/chest_slots", "slot/saddle", "slot/horse_armor",
            "slot/llama_armor",
            // The open-bundle tooltip: which packed item is selected (mouse
            // wheel while hovering), so a normal click then extracts it.
            "bundle/slot_background", "bundle/slot_highlight_back",
            "bundle/slot_highlight_front",
            // The bundle tooltip's fullness/weight bar (0.107.0): border,
            // the normal fill, and a distinct "at capacity" fill.
            "bundle/bundle_progressbar_border", "bundle/bundle_progressbar_fill",
            "bundle/bundle_progressbar_full",
            // The crafter's per-slot disable toggle and its redstone-power
            // indicator (decompiled `CrafterScreen`).
            "crafter/disabled_slot", "crafter/powered_redstone",
            "crafter/unpowered_redstone",
            // The villager screen: the demand-price strikethrough between a
            // trade's base and current cost, and the trader's level/XP bar
            // (decompiled `MerchantScreen`).
            "villager/discount_strikethrough",
            "villager/experience_bar_background", "villager/experience_bar_current",
        ] {
            if let Ok(tex) = t(pack, &format!("gui/sprites/container/{name}")) {
                container_sprites.insert(name, tex);
            }
        }

        // The recipe book's own sprites.
        let mut book_sprites = HashMap::new();
        for name in [
            "button", "button_highlighted", "tab", "tab_selected",
            "slot_craftable", "slot_uncraftable",
            "page_forward", "page_forward_highlighted",
            "page_backward", "page_backward_highlighted",
            "filter_enabled", "filter_disabled", "overlay_recipe",
        ] {
            if let Ok(tex) = t(pack, &format!("gui/sprites/recipe_book/{name}")) {
                book_sprites.insert(name, tex);
            }
        }

        // Locator bar dot sprites: real vanilla ships exactly these five,
        // shared by its two waypoint styles (`default_0..3`, plus `bowtie`
        // for the style that leads with a distinct close-up icon).
        let mut locator_bar_dot = HashMap::new();
        for name in ["default_0", "default_1", "default_2", "default_3", "bowtie"] {
            if let Ok(tex) = t(pack, &format!("gui/sprites/hud/locator_bar_dot/{name}")) {
                locator_bar_dot.insert(name, tex);
            }
        }

        let tex = McTextures {
            button: t(pack, "gui/sprites/widget/button")?,
            button_highlighted: t(pack, "gui/sprites/widget/button_highlighted")?,
            button_disabled: t(pack, "gui/sprites/widget/button_disabled")?,
            slider: t(pack, "gui/sprites/widget/slider")?,
            slider_handle: t(pack, "gui/sprites/widget/slider_handle")?,
            slider_handle_highlighted: t(pack, "gui/sprites/widget/slider_handle_highlighted")?,
            menu_bg: t(pack, "gui/menu_background")?,
            inworld_bg: t(pack, "gui/inworld_menu_background")?,
            dirt: t(pack, "block/dirt")?,
            logo: {
                let img = image::load_from_memory(LOGO_PNG)
                    .context("embedded logo PNG")?
                    .to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                ctx.load_texture("dolphin-logo", color, TextureOptions::LINEAR)
            },
            crosshair: t(pack, "gui/sprites/hud/crosshair")?,
            hotbar: t(pack, "gui/sprites/hud/hotbar")?,
            hotbar_selection: t(pack, "gui/sprites/hud/hotbar_selection")?,
            hotbar_offhand: t(pack, "gui/sprites/hud/hotbar_offhand_left").ok(),
            attack_bg: t(pack, "gui/sprites/hud/crosshair_attack_indicator_background").ok(),
            attack_progress: t(pack, "gui/sprites/hud/crosshair_attack_indicator_progress").ok(),
            attack_full: t(pack, "gui/sprites/hud/crosshair_attack_indicator_full").ok(),
            air: t(pack, "gui/sprites/hud/air").ok(),
            air_bursting: t(pack, "gui/sprites/hud/air_bursting").ok(),
            air_empty: t(pack, "gui/sprites/hud/air_empty").ok(),
            fire: pack
                .texture_png_raw("block/fire_1")
                .ok()
                .map(|img| {
                    let size = [img.width() as usize, img.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    ctx.load_texture("fire-overlay", color, TextureOptions::NEAREST)
                }),
            heart_container: t(pack, "gui/sprites/hud/heart/container")?,
            heart_full: t(pack, "gui/sprites/hud/heart/full")?,
            heart_half: t(pack, "gui/sprites/hud/heart/half")?,
            heart_poison_full: t(pack, "gui/sprites/hud/heart/poisoned_full").ok(),
            heart_poison_half: t(pack, "gui/sprites/hud/heart/poisoned_half").ok(),
            heart_wither_full: t(pack, "gui/sprites/hud/heart/withered_full").ok(),
            heart_wither_half: t(pack, "gui/sprites/hud/heart/withered_half").ok(),
            heart_absorb_full: t(pack, "gui/sprites/hud/heart/absorbing_full").ok(),
            heart_absorb_half: t(pack, "gui/sprites/hud/heart/absorbing_half").ok(),
            heart_frozen_full: t(pack, "gui/sprites/hud/heart/frozen_full").ok(),
            heart_frozen_half: t(pack, "gui/sprites/hud/heart/frozen_half").ok(),
            heart_vehicle_full: t(pack, "gui/sprites/hud/heart/vehicle_full").ok(),
            heart_vehicle_half: t(pack, "gui/sprites/hud/heart/vehicle_half").ok(),
            heart_vehicle_container: t(pack, "gui/sprites/hud/heart/vehicle_container").ok(),
            freeze_overlay: t(pack, "misc/powder_snow_outline").ok(),
            pumpkin_blur: t(pack, "misc/pumpkinblur").ok(),
            spyglass_scope: t(pack, "misc/spyglass_scope").ok(),
            portal_overlay: t(pack, "block/nether_portal").ok(),
            nausea: t(pack, "misc/nausea").ok(),
            food_empty: t(pack, "gui/sprites/hud/food_empty")?,
            food_full: t(pack, "gui/sprites/hud/food_full")?,
            food_half: t(pack, "gui/sprites/hud/food_half")?,
            xp_bg: t(pack, "gui/sprites/hud/experience_bar_background")?,
            xp_progress: t(pack, "gui/sprites/hud/experience_bar_progress")?,
            jump_bg: t(pack, "gui/sprites/hud/jump_bar_background").ok(),
            jump_progress: t(pack, "gui/sprites/hud/jump_bar_progress").ok(),
            locator_bar_bg: t(pack, "gui/sprites/hud/locator_bar_background").ok(),
            locator_bar_arrow_up: t(pack, "gui/sprites/hud/locator_bar_arrow_up").ok(),
            locator_bar_arrow_down: t(pack, "gui/sprites/hud/locator_bar_arrow_down").ok(),
            locator_bar_dot,
            ping: [
                t(pack, "gui/sprites/icon/ping_1")?,
                t(pack, "gui/sprites/icon/ping_2")?,
                t(pack, "gui/sprites/icon/ping_3")?,
                t(pack, "gui/sprites/icon/ping_4")?,
                t(pack, "gui/sprites/icon/ping_5")?,
                t(pack, "gui/sprites/icon/ping_unknown")?,
            ],
            unknown_server: t(pack, "misc/unknown_server")
                .or_else(|_| t(pack, "gui/unknown_server"))
                .unwrap_or_else(|_| {
                    let color = egui::ColorImage::filled([64, 64], Color32::from_gray(40));
                    ctx.load_texture("unknown-server", color, TextureOptions::NEAREST)
                }),
            containers,
            boss_bar,
            toast,
            advancement_window,
            advancement_bg,
            advancement,
            book: t(pack, "gui/book").ok(),
            container_sprites,
            recipe_book: t(pack, "gui/recipe_book").ok(),
            book_sprites,
            text_field: t(pack, "gui/sprites/widget/text_field").ok(),
        };
        let glint = pack
            .texture_png("misc/enchanted_glint_item")
            .map_err(|e| tracing::info!("gui: no enchantment glint sprite: {e:#}"))
            .ok();
        Ok(Self {
            font,
            sga: SgaFont::load(pack, ctx),
            tex,
            clicks: AtomicU32::new(0),
            glint,
            glint_tex: Mutex::new(HashMap::new()),
        })
    }

    /// The item icon with vanilla's enchantment glint composited over it, as an
    /// egui texture. Vanilla draws the glint sprite additively through the
    /// item's own alpha mask, tiled 8×, rotated 10° and scrolling with time; we
    /// do the same on the CPU — an icon is a thousand pixels, and only the
    /// enchanted stacks actually on screen are ever composited.
    ///
    /// `phase` is seconds; `None` when the sprite or the icon is unavailable, in
    /// which case the caller just draws the plain icon.
    pub fn glint_texture(
        &self,
        ctx: &egui::Context,
        icons: &crate::assets::items::ItemIcons,
        item: &str,
        phase: f32,
    ) -> Option<egui::TextureId> {
        let glint = self.glint.as_ref()?;
        let icon = icons.icon_image(item)?;
        let (gw, gh) = (glint.width() as f32, glint.height() as f32);
        // Vanilla's glint texture matrix: scale 8, rotate 10°, translate by two
        // offsets running at different speeds (periods 13.75 s and 3.75 s).
        let (sin, cos) = 10.0_f32.to_radians().sin_cos();
        let off_u = (phase / 13.75).fract();
        let off_v = (phase / 3.75).fract();
        let (w, h) = (icon.width(), icon.height());
        let mut out = icon.clone();
        for y in 0..h {
            for x in 0..w {
                let a = out.get_pixel(x, y).0[3];
                if a == 0 {
                    continue;
                }
                let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                let gu = (u * 8.0 * cos - v * 8.0 * sin - off_u).rem_euclid(1.0);
                let gv = (u * 8.0 * sin + v * 8.0 * cos + off_v).rem_euclid(1.0);
                let gp = glint
                    .get_pixel(
                        ((gu * gw) as u32).min(glint.width() - 1),
                        ((gv * gh) as u32).min(glint.height() - 1),
                    )
                    .0;
                // Vanilla's glint blend is SRC_COLOR × SRC_COLOR + DST: the
                // sprite is *squared* before it is added, which crushes its dark
                // purple base and leaves only the bright streaks. Modulated by
                // the icon's alpha so the glint stays inside the item's shape.
                let px = out.get_pixel_mut(x, y);
                let k = a as u32;
                for c in 0..3 {
                    let g = gp[c] as u32;
                    px.0[c] = (px.0[c] as u32 + g * g / 255 * k / 255).min(255) as u8;
                }
            }
        }
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            out.as_raw(),
        );
        let mut cache = self.glint_tex.lock().ok()?;
        match cache.get_mut(item) {
            Some(handle) => {
                handle.set(color, TextureOptions::NEAREST);
                Some(handle.id())
            }
            None => {
                let handle =
                    ctx.load_texture(format!("glint-{item}"), color, TextureOptions::NEAREST);
                let id = handle.id();
                cache.insert(item.to_owned(), handle);
                Some(id)
            }
        }
    }

    /// Record a widget click (button press / slider release) for the app to
    /// play the vanilla click sound.
    pub fn click(&self) {
        self.clicks.fetch_add(1, Ordering::Relaxed);
    }

    /// Drain the click counter (app side, once per frame).
    pub fn take_clicks(&self) -> u32 {
        self.clicks.swap(0, Ordering::Relaxed)
    }

    /// GUI scale like vanilla's "Auto": the largest integer scale that keeps
    /// the 320×240 GUI inside the window; `setting` 1..=4 caps it.
    pub fn gui_scale(&self, ctx: &egui::Context, settings: &GameSettings) -> f32 {
        let r = ctx.content_rect();
        let auto = (r.width() / 320.0).min(r.height() / 240.0).floor().max(1.0);
        if settings.gui_scale == 0 {
            auto
        } else {
            auto.min(settings.gui_scale as f32)
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

/// Draw `tex` into `rect` as a nine-slice with `border` source px on every
/// side (vanilla widgets use 3), scaled by `s`.
pub fn nine_slice(painter: &egui::Painter, tex: &TextureHandle, rect: Rect, border: f32, s: f32) {
    let ts = tex.size_vec2();
    let b = border;
    let bd = border * s;
    let (u0, u1, u2, u3) = (0.0, b / ts.x, (ts.x - b) / ts.x, 1.0);
    let (v0, v1, v2, v3) = (0.0, b / ts.y, (ts.y - b) / ts.y, 1.0);
    let (x0, x1, x2, x3) = (rect.left(), rect.left() + bd, rect.right() - bd, rect.right());
    let (y0, y1, y2, y3) = (rect.top(), rect.top() + bd, rect.bottom() - bd, rect.bottom());
    let cells = [
        ((x0, x1, y0, y1), (u0, u1, v0, v1)),
        ((x1, x2, y0, y1), (u1, u2, v0, v1)),
        ((x2, x3, y0, y1), (u2, u3, v0, v1)),
        ((x0, x1, y1, y2), (u0, u1, v1, v2)),
        ((x1, x2, y1, y2), (u1, u2, v1, v2)),
        ((x2, x3, y1, y2), (u2, u3, v1, v2)),
        ((x0, x1, y2, y3), (u0, u1, v2, v3)),
        ((x1, x2, y2, y3), (u1, u2, v2, v3)),
        ((x2, x3, y2, y3), (u2, u3, v2, v3)),
    ];
    for ((ax, bx, ay, by), (au, bu, av, bv)) in cells {
        if bx > ax && by > ay {
            painter.image(
                tex.id(),
                Rect::from_min_max(pos2(ax, ay), pos2(bx, by)),
                Rect::from_min_max(pos2(au, av), pos2(bu, bv)),
                Color32::WHITE,
            );
        }
    }
}

/// Tile a 16×16 background texture over `rect` at 16 GUI px per tile.
/// `tint` multiplies the texture (vanilla darkens dirt to 25 % grey).
pub fn tile_background(
    painter: &egui::Painter,
    tex: &TextureHandle,
    rect: Rect,
    s: f32,
    tint: Color32,
) {
    let step = 16.0 * s;
    let mut y = rect.top();
    while y < rect.bottom() {
        let mut x = rect.left();
        while x < rect.right() {
            let cell = Rect::from_min_size(pos2(x, y), vec2(step, step))
                .intersect(rect);
            // Clip the UV so partial edge tiles don't stretch.
            let cuv = Rect::from_min_max(
                pos2(0.0, 0.0),
                pos2(cell.width() / step, cell.height() / step),
            );
            painter.image(tex.id(), cell, cuv, tint);
            x += step;
        }
        y += step;
    }
}

/// A vanilla button. `w` in GUI px. Returns true on click.
pub fn button(ui: &mut egui::Ui, mc: &McUi, w: f32, s: f32, label: &str, enabled: bool) -> bool {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(w * s, BTN_H * s), sense);
    let hovered = enabled && resp.hovered();
    let tex = if !enabled {
        &mc.tex.button_disabled
    } else if hovered {
        &mc.tex.button_highlighted
    } else {
        &mc.tex.button
    };
    nine_slice(ui.painter(), tex, rect, 3.0, s);
    let color = if !enabled {
        Color32::from_rgb(0xA0, 0xA0, 0xA0)
    } else if hovered {
        Color32::from_rgb(0xFF, 0xFF, 0xA0)
    } else {
        Color32::WHITE
    };
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    mc.font.draw_anchored(
        ui.painter(),
        rect.center() - vec2(0.0, 0.5 * s),
        Align2::CENTER_CENTER,
        label,
        s,
        color,
        true,
    );
    let clicked = enabled && resp.clicked();
    if clicked {
        mc.click();
    }
    clicked
}

/// A vanilla slider: textured track + draggable handle, label centered.
/// `t` is the normalized 0..1 position. Returns true while the user changes it.
pub fn slider(ui: &mut egui::Ui, mc: &McUi, w: f32, s: f32, label: &str, t: &mut f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(w * s, BTN_H * s), Sense::click_and_drag());
    let mut changed = false;
    if resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let nt = ((p.x - rect.left() - 4.0 * s) / (rect.width() - 8.0 * s)).clamp(0.0, 1.0);
            if (nt - *t).abs() > f32::EPSILON {
                *t = nt;
                changed = true;
            }
        }
    }
    if resp.drag_stopped() || resp.clicked() {
        mc.click();
    }
    nine_slice(ui.painter(), &mc.tex.slider, rect, 3.0, s);
    let hx = rect.left() + (rect.width() - 8.0 * s) * t.clamp(0.0, 1.0);
    let handle = Rect::from_min_size(pos2(hx, rect.top()), vec2(8.0 * s, BTN_H * s));
    let handle_tex = if resp.hovered() || resp.dragged() {
        &mc.tex.slider_handle_highlighted
    } else {
        &mc.tex.slider_handle
    };
    nine_slice(ui.painter(), handle_tex, handle, 2.0, s);
    mc.font.draw_anchored(
        ui.painter(),
        rect.center() - vec2(0.0, 0.5 * s),
        Align2::CENTER_CENTER,
        label,
        s,
        Color32::WHITE,
        true,
    );
    changed
}

// ---------------------------------------------------------------------------
// Minecraft-font text editing
// ---------------------------------------------------------------------------

/// Apply egui keyboard events to a `(text, cursor)` pair. Returns true when
/// the text changed. `cursor` is a byte offset on a char boundary.
pub fn edit_events(ctx: &egui::Context, text: &mut String, cursor: &mut usize) -> bool {
    let mut changed = false;
    *cursor = (*cursor).min(text.len());
    let events = ctx.input(|i| i.events.clone());
    for ev in events {
        match ev {
            egui::Event::Text(t) => {
                for c in t.chars().filter(|c| !c.is_control() && *c != '§') {
                    text.insert(*cursor, c);
                    *cursor += c.len_utf8();
                    changed = true;
                }
            }
            egui::Event::Paste(t) => {
                let clean: String = t.chars().filter(|c| !c.is_control()).collect();
                text.insert_str(*cursor, &clean);
                *cursor += clean.len();
                changed = true;
            }
            egui::Event::Copy => {
                ctx.copy_text(text.clone());
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => match key {
                egui::Key::Backspace => {
                    if *cursor > 0 {
                        let prev = prev_char_boundary(text, *cursor);
                        text.replace_range(prev..*cursor, "");
                        *cursor = prev;
                        changed = true;
                    }
                }
                egui::Key::Delete => {
                    if *cursor < text.len() {
                        let next = next_char_boundary(text, *cursor);
                        text.replace_range(*cursor..next, "");
                        changed = true;
                    }
                }
                egui::Key::ArrowLeft => {
                    if *cursor > 0 {
                        *cursor = prev_char_boundary(text, *cursor);
                    }
                }
                egui::Key::ArrowRight => {
                    if *cursor < text.len() {
                        *cursor = next_char_boundary(text, *cursor);
                    }
                }
                egui::Key::Home => *cursor = 0,
                egui::Key::End => *cursor = text.len(),
                egui::Key::V if modifiers.command => {} // Paste event handles it
                _ => {}
            },
            _ => {}
        }
    }
    changed
}

pub fn prev_char_boundary(s: &str, i: usize) -> usize {
    let mut j = i.saturating_sub(1);
    while j > 0 && !s.is_char_boundary(j) {
        j -= 1;
    }
    j
}

pub fn next_char_boundary(s: &str, i: usize) -> usize {
    let mut j = (i + 1).min(s.len());
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

/// A vanilla text box drawn entirely with the Minecraft font. Focus follows
/// clicks; when `force_focus` the field grabs focus immediately.
pub fn text_field(
    ui: &mut egui::Ui,
    mc: &McUi,
    w: f32,
    s: f32,
    buf: &mut String,
    hint: &str,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w * s, BTN_H * s), Sense::click());
    let id = resp.id;
    let ctx = ui.ctx().clone();
    if resp.clicked() {
        ctx.memory_mut(|m| m.request_focus(id));
    }
    let focused = ctx.memory(|m| m.has_focus(id));

    // Per-field cursor lives in egui temp memory.
    let cur_id = id.with("cursor");
    let mut cursor: usize = ctx.data(|d| d.get_temp(cur_id)).unwrap_or(buf.len());
    if focused {
        edit_events(&ctx, buf, &mut cursor);
    }
    cursor = cursor.min(buf.len());
    ctx.data_mut(|d| d.insert_temp(cur_id, cursor));

    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
    let pad = 4.0 * s;
    let text_pos = pos2(rect.left() + pad, rect.center().y - 4.0 * s);
    if buf.is_empty() && !hint.is_empty() {
        mc.font.draw(ui.painter(), text_pos, hint, s, Color32::from_gray(110), false);
    } else {
        mc.font.draw(ui.painter(), text_pos, buf, s, Color32::from_rgb(0xE0, 0xE0, 0xE0), true);
    }
    if focused {
        // Blinking cursor bar after the cursor's prefix width.
        let t = ui.input(|i| i.time);
        if (t * 3.0) as u64 % 2 == 0 {
            let cx = text_pos.x + mc.font.width(&buf[..cursor], s);
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(cx, text_pos.y - s), vec2(s.max(1.0), 10.0 * s)),
                0.0,
                Color32::from_rgb(0xE0, 0xE0, 0xE0),
            );
        }
        ui.ctx().request_repaint();
    }
    let border = if focused {
        Color32::WHITE
    } else {
        Color32::from_rgb(0xA0, 0xA0, 0xA0)
    };
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(s.max(1.0), border), StrokeKind::Inside);
    resp
}

/// Left-aligned label in the Minecraft font (one line, GUI px sizing).
pub fn label(ui: &mut egui::Ui, mc: &McUi, s: f32, text: &str, color: Color32) {
    let w = mc.font.width(text, s);
    let (rect, _) = ui.allocate_exact_size(vec2(w, LINE_H * s), Sense::hover());
    mc.font.draw(ui.painter(), rect.min, text, s, color, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unifont_hex_parses_and_trims() {
        // 16 rows × 1 byte: rows 4..=13 hold 0x42 = 0b01000010 → columns 1 and 6.
        let good = "0041:00000000424242424242424242420000";
        let uni = Unifont::parse(good);
        assert!(uni.contains(0x41));
        let g = uni.glyphs.get(&0x41).unwrap();
        assert_eq!(g.bytes_per_row, 1);
        assert_eq!((g.left, g.right), (1, 6));
        // Content 6 px wide → advance 6/2 + 1 = 4 GUI px.
        assert_eq!(uni.advance(0x41), Some(4.0));
        // Malformed lines are skipped, not fatal.
        let uni = Unifont::parse("zzzz:123\nnot a line\n0042:00000000424242424242424242420000");
        assert!(uni.contains(0x42) && !uni.contains(0x43));
    }

    #[test]
    fn char_boundaries() {
        let s = "aöb";
        assert_eq!(next_char_boundary(s, 0), 1);
        assert_eq!(next_char_boundary(s, 1), 3); // ö is 2 bytes
        assert_eq!(prev_char_boundary(s, 3), 1);
        assert_eq!(prev_char_boundary(s, 1), 0);
    }
}
