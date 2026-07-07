//! Minecraft-look UI toolkit for the launcher (egui 0.29).
//!
//! When the vanilla 26.1 client jar is already cached under `.minecraft`
//! (after the first play), the real game assets are used: the bitmap font
//! (`font/ascii.png` + `font/accented.png`), the nine-sliced button/slider
//! textures and the dirt background — the launcher then looks exactly like
//! the in-game menus. On a fresh install (no jar yet) everything falls back
//! to hand-drawn Minecraft-style widgets and a default font; after the first
//! game start the real assets appear automatically.

use std::collections::HashMap;
use std::io::Read;

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, TextureHandle,
    TextureOptions, pos2, vec2,
};

use crate::config;

/// Vanilla button metrics (GUI px); the launcher renders at a fixed 2× scale.
pub const BTN_H: f32 = 20.0;
pub const S: f32 = 2.0;

/// The DolphinClient logo, embedded (PNG, transparent circle).
pub const LOGO_PNG: &[u8] = include_bytes!("../../assets/brand/dolphin-256.png");

struct Glyph {
    page: usize,
    uv: Rect,
    advance: f32,
    w: f32,
    h: f32,
    top: f32,
}

/// Bitmap font from the vanilla jar (None until the jar is cached).
struct McFont {
    pages: Vec<TextureHandle>,
    glyphs: HashMap<char, Glyph>,
}

pub struct McUi {
    font: Option<McFont>,
    button: Option<TextureHandle>,
    button_highlighted: Option<TextureHandle>,
    button_disabled: Option<TextureHandle>,
    slider: Option<TextureHandle>,
    slider_handle: Option<TextureHandle>,
    slider_handle_highlighted: Option<TextureHandle>,
    dirt: Option<TextureHandle>,
    pub logo: TextureHandle,
}

fn load_png_tex(
    ctx: &egui::Context,
    name: &str,
    bytes: &[u8],
    opts: TextureOptions,
) -> Option<(TextureHandle, image::RgbaImage)> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some((ctx.load_texture(name, color, opts), img))
}

impl McUi {
    pub fn load(ctx: &egui::Context) -> Self {
        let logo = load_png_tex(ctx, "dolphin-logo", LOGO_PNG, TextureOptions::LINEAR)
            .map(|(t, _)| t)
            .expect("embedded logo PNG is valid");

        let mut ui = Self {
            font: None,
            button: None,
            button_highlighted: None,
            button_disabled: None,
            slider: None,
            slider_handle: None,
            slider_handle_highlighted: None,
            dirt: None,
            logo,
        };
        ui.try_load_jar_assets(ctx);
        ui
    }

    /// Best-effort: pull font + widget textures out of the cached vanilla jar.
    fn try_load_jar_assets(&mut self, ctx: &egui::Context) {
        let jar_path = config::minecraft_dir()
            .join("versions")
            .join(config::TARGET_VERSION)
            .join(format!("{}.jar", config::TARGET_VERSION));
        let Ok(file) = std::fs::File::open(&jar_path) else { return };
        let Ok(mut zip) = zip::ZipArchive::new(std::io::BufReader::new(file)) else { return };

        let mut read = |path: &str| -> Option<Vec<u8>> {
            let mut f = zip.by_name(path).ok()?;
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf).ok()?;
            Some(buf)
        };

        let tex = |ctx: &egui::Context, name: &str, bytes: Option<Vec<u8>>| {
            bytes.and_then(|b| load_png_tex(ctx, name, &b, TextureOptions::NEAREST).map(|(t, _)| t))
        };
        const W: &str = "assets/minecraft/textures/gui/sprites/widget";
        self.button = tex(ctx, "mc-button", read(&format!("{W}/button.png")));
        self.button_highlighted =
            tex(ctx, "mc-button-hi", read(&format!("{W}/button_highlighted.png")));
        self.button_disabled =
            tex(ctx, "mc-button-dis", read(&format!("{W}/button_disabled.png")));
        self.slider = tex(ctx, "mc-slider", read(&format!("{W}/slider.png")));
        self.slider_handle =
            tex(ctx, "mc-slider-h", read(&format!("{W}/slider_handle.png")));
        self.slider_handle_highlighted = tex(
            ctx,
            "mc-slider-hh",
            read(&format!("{W}/slider_handle_highlighted.png")),
        );
        self.dirt = tex(
            ctx,
            "mc-dirt",
            read("assets/minecraft/textures/block/dirt.png"),
        );

        // Bitmap font, data-driven from the jar's own font definition.
        let Some(def) = read("assets/minecraft/font/include/default.json") else { return };
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&def) else { return };
        let Some(providers) = json.get("providers").and_then(|p| p.as_array()) else { return };

        let mut pages = Vec::new();
        let mut glyphs = HashMap::new();
        for prov in providers {
            if prov.get("type").and_then(|t| t.as_str()) != Some("bitmap") {
                continue;
            }
            let Some(file) = prov.get("file").and_then(|f| f.as_str()) else { continue };
            let rel = file.strip_prefix("minecraft:").unwrap_or(file);
            let Some(bytes) = read(&format!("assets/minecraft/textures/{rel}")) else { continue };
            let Ok(img) = image::load_from_memory(&bytes) else { continue };
            let img = img.to_rgba8();
            let ascent = prov.get("ascent").and_then(|a| a.as_f64()).unwrap_or(7.0) as f32;
            let height = prov.get("height").and_then(|h| h.as_f64()).unwrap_or(8.0) as f32;
            let rows: Vec<String> = prov
                .get("chars")
                .and_then(|c| c.as_array())
                .map(|a| a.iter().filter_map(|r| r.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            if rows.is_empty() {
                continue;
            }
            let cols = rows[0].chars().count().max(1);
            let (iw, ih) = (img.width() as f32, img.height() as f32);
            let cell_w = iw / cols as f32;
            let cell_h = ih / rows.len() as f32;
            let scale = height / cell_h;
            let page_idx = pages.len();
            for (row_i, row) in rows.iter().enumerate() {
                for (col_i, ch) in row.chars().enumerate() {
                    if ch == '\u{0}' || glyphs.contains_key(&ch) {
                        continue;
                    }
                    let x0 = col_i as f32 * cell_w;
                    let y0 = row_i as f32 * cell_h;
                    let mut content_w = 0u32;
                    for px in 0..cell_w as u32 {
                        for py in 0..cell_h as u32 {
                            if img.get_pixel(x0 as u32 + px, y0 as u32 + py).0[3] > 96 {
                                content_w = px + 1;
                                break;
                            }
                        }
                    }
                    let advance = if content_w == 0 {
                        if ch == ' ' { 4.0 } else { continue }
                    } else {
                        content_w as f32 * scale + 1.0
                    };
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
            pages.push(ctx.load_texture(format!("mcfont-{rel}"), color, TextureOptions::NEAREST));
        }
        if !glyphs.is_empty() {
            self.font = Some(McFont { pages, glyphs });
        }
    }

    /// True once the real game assets are loaded (jar cached).
    pub fn has_game_assets(&self) -> bool {
        self.font.is_some() && self.button.is_some()
    }

    // -- text ---------------------------------------------------------------

    /// Width of `text` at GUI scale `s`.
    pub fn text_width(&self, text: &str, s: f32) -> f32 {
        match &self.font {
            Some(f) => text
                .chars()
                .map(|c| f.glyphs.get(&c).map_or(4.0, |g| g.advance))
                .sum::<f32>()
                * s,
            None => text.chars().count() as f32 * 4.9 * s, // monospace estimate
        }
    }

    /// Draw one line of Minecraft text, top-left at `pos` (8·s px tall).
    pub fn text(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        text: &str,
        s: f32,
        color: Color32,
        shadow: bool,
    ) {
        match &self.font {
            Some(font) => {
                if shadow {
                    let sc = Color32::from_rgba_unmultiplied(
                        (color.r() as u32 * 63 / 255) as u8,
                        (color.g() as u32 * 63 / 255) as u8,
                        (color.b() as u32 * 63 / 255) as u8,
                        color.a(),
                    );
                    self.text_run(font, painter, pos + vec2(s, s), text, s, sc);
                }
                self.text_run(font, painter, pos, text, s, color);
            }
            None => {
                let font_id = FontId::monospace(8.0 * s);
                if shadow {
                    painter.text(
                        pos + vec2(s, s),
                        Align2::LEFT_TOP,
                        text,
                        font_id.clone(),
                        Color32::from_black_alpha(160),
                    );
                }
                painter.text(pos, Align2::LEFT_TOP, text, font_id, color);
            }
        }
    }

    fn text_run(
        &self,
        font: &McFont,
        painter: &egui::Painter,
        pos: Pos2,
        text: &str,
        s: f32,
        color: Color32,
    ) {
        let mut x = pos.x;
        for c in text.chars() {
            let Some(g) = font.glyphs.get(&c) else {
                x += 4.0 * s;
                continue;
            };
            if c != ' ' {
                let rect =
                    Rect::from_min_size(pos2(x, pos.y + g.top * s), vec2(g.w * s, g.h * s));
                painter.image(font.pages[g.page].id(), rect, g.uv, color);
            }
            x += g.advance * s;
        }
    }

    /// Anchored text (8·s px line box).
    pub fn text_anchored(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        anchor: Align2,
        text: &str,
        s: f32,
        color: Color32,
        shadow: bool,
    ) {
        let w = self.text_width(text, s);
        let h = 8.0 * s;
        let rect = anchor.anchor_size(pos, vec2(w, h));
        self.text(painter, rect.min, text, s, color, shadow);
    }

    // -- widgets --------------------------------------------------------------

    /// Tiled dark-dirt background over `rect` (16 GUI px per tile). Falls back
    /// to a flat dark fill until the jar is cached.
    pub fn dirt_background(&self, painter: &egui::Painter, rect: Rect, s: f32) {
        match &self.dirt {
            Some(tex) => {
                let step = 16.0 * s;
                let mut y = rect.top();
                while y < rect.bottom() {
                    let mut x = rect.left();
                    while x < rect.right() {
                        let cell =
                            Rect::from_min_size(pos2(x, y), vec2(step, step)).intersect(rect);
                        let cuv = Rect::from_min_max(
                            pos2(0.0, 0.0),
                            pos2(cell.width() / step, cell.height() / step),
                        );
                        painter.image(tex.id(), cell, cuv, Color32::from_gray(64));
                        x += step;
                    }
                    y += step;
                }
            }
            None => {
                painter.rect_filled(rect, 0.0, Color32::from_rgb(0x2a, 0x21, 0x1a));
            }
        }
    }

    fn nine_slice(&self, painter: &egui::Painter, tex: &TextureHandle, rect: Rect, border: f32, s: f32) {
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

    /// Fallback stone-grey bevel button body (until the jar is cached).
    fn bevel_button(&self, painter: &egui::Painter, rect: Rect, hovered: bool, enabled: bool) {
        let (top, bottom) = if !enabled {
            (Color32::from_rgb(0x35, 0x35, 0x35), Color32::from_rgb(0x2a, 0x2a, 0x2a))
        } else if hovered {
            (Color32::from_rgb(0x8a, 0x8f, 0xbf), Color32::from_rgb(0x70, 0x74, 0x9e))
        } else {
            (Color32::from_rgb(0x8b, 0x8b, 0x8b), Color32::from_rgb(0x6f, 0x6f, 0x6f))
        };
        let mid = rect.center().y;
        painter.rect_filled(Rect::from_min_max(rect.min, pos2(rect.max.x, mid)), 0.0, top);
        painter.rect_filled(Rect::from_min_max(pos2(rect.min.x, mid), rect.max), 0.0, bottom);
        painter.line_segment(
            [rect.left_top() + vec2(1.0, 1.0), rect.right_top() + vec2(-1.0, 1.0)],
            Stroke::new(2.0, Color32::from_white_alpha(60)),
        );
        painter.line_segment(
            [rect.left_bottom() + vec2(1.0, -1.0), rect.right_bottom() + vec2(-1.0, -1.0)],
            Stroke::new(2.0, Color32::from_black_alpha(110)),
        );
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::BLACK));
    }

    /// A Minecraft button; `w`/`h` in GUI px at scale `s`. Returns the response
    /// (use `.clicked()`).
    pub fn button_sized(
        &self,
        ui: &mut egui::Ui,
        w: f32,
        h: f32,
        s: f32,
        label: &str,
        enabled: bool,
    ) -> Response {
        let sense = if enabled { Sense::click() } else { Sense::hover() };
        let (rect, resp) = ui.allocate_exact_size(vec2(w * s, h * s), sense);
        let hovered = enabled && resp.hovered();
        match (&self.button, &self.button_highlighted, &self.button_disabled) {
            (Some(b), Some(bh), Some(bd)) => {
                let tex = if !enabled { bd } else if hovered { bh } else { b };
                self.nine_slice(ui.painter(), tex, rect, 3.0, s);
            }
            _ => self.bevel_button(ui.painter(), rect, hovered, enabled),
        }
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
        self.text_anchored(
            ui.painter(),
            rect.center() - vec2(0.0, 0.5 * s),
            Align2::CENTER_CENTER,
            label,
            s,
            color,
            true,
        );
        resp
    }

    pub fn button(&self, ui: &mut egui::Ui, w: f32, s: f32, label: &str, enabled: bool) -> bool {
        enabled && self.button_sized(ui, w, BTN_H, s, label, enabled).clicked()
    }

    /// A Minecraft slider (label centered); `t` is normalized 0..1.
    pub fn slider(&self, ui: &mut egui::Ui, w: f32, s: f32, label: &str, t: &mut f32) -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(w * s, BTN_H * s), Sense::click_and_drag());
        let mut changed = false;
        if resp.dragged() || resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let nt =
                    ((p.x - rect.left() - 4.0 * s) / (rect.width() - 8.0 * s)).clamp(0.0, 1.0);
                if (nt - *t).abs() > f32::EPSILON {
                    *t = nt;
                    changed = true;
                }
            }
        }
        match (&self.slider, &self.slider_handle, &self.slider_handle_highlighted) {
            (Some(track), Some(handle), Some(handle_hi)) => {
                self.nine_slice(ui.painter(), track, rect, 3.0, s);
                let hx = rect.left() + (rect.width() - 8.0 * s) * t.clamp(0.0, 1.0);
                let hrect = Rect::from_min_size(pos2(hx, rect.top()), vec2(8.0 * s, BTN_H * s));
                let htex = if resp.hovered() || resp.dragged() { handle_hi } else { handle };
                self.nine_slice(ui.painter(), htex, hrect, 2.0, s);
            }
            _ => {
                ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(0x1e, 0x1e, 0x1e));
                ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::BLACK));
                let hx = rect.left() + (rect.width() - 8.0 * s) * t.clamp(0.0, 1.0);
                let hrect = Rect::from_min_size(pos2(hx, rect.top()), vec2(8.0 * s, BTN_H * s));
                self.bevel_button(ui.painter(), hrect, resp.hovered(), true);
            }
        }
        self.text_anchored(
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

    /// Vanilla text box (black, grey border, white when focused).
    pub fn text_field(
        &self,
        ui: &mut egui::Ui,
        w: f32,
        s: f32,
        buf: &mut String,
        hint: &str,
    ) -> Response {
        let (rect, _) = ui.allocate_exact_size(vec2(w * s, BTN_H * s), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
        let inner = rect.shrink2(vec2(4.0 * s, 0.0));
        let resp = ui.put(
            inner,
            egui::TextEdit::singleline(buf)
                .frame(false)
                .font(FontId::monospace(7.5 * s))
                .text_color(Color32::from_rgb(0xE0, 0xE0, 0xE0))
                .hint_text(egui::RichText::new(hint).color(Color32::from_gray(110)))
                .vertical_align(egui::Align::Center),
        );
        let border = if resp.has_focus() {
            Color32::WHITE
        } else {
            Color32::from_rgb(0xA0, 0xA0, 0xA0)
        };
        ui.painter()
            .rect_stroke(rect, 0.0, Stroke::new(s.max(1.0), border));
        resp
    }

    /// One line of Minecraft text as a laid-out widget.
    pub fn label(&self, ui: &mut egui::Ui, s: f32, text: &str, color: Color32) {
        let w = self.text_width(text, s);
        let (rect, _) = ui.allocate_exact_size(vec2(w, 9.0 * s), Sense::hover());
        self.text(ui.painter(), rect.min, text, s, color, true);
    }
}
