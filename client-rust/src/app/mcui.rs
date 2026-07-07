//! Vanilla-Minecraft UI toolkit for egui.
//!
//! Everything visual comes straight out of the 26.1 client jar so the menus
//! look exactly like the real game:
//! - the bitmap font (`font/ascii.png` + friends, parsed from
//!   `font/include/default.json` like vanilla does),
//! - nine-sliced widget textures (button / slider),
//! - tiled menu backgrounds (`menu_background`, `inworld_menu_background`),
//! - HUD sprites (hotbar, hearts, food, XP bar, crosshair).
//!
//! All widget sizes are in *GUI pixels* (vanilla's 320×240 coordinate space)
//! multiplied by a GUI scale `s`, exactly like the real client.

use std::collections::HashMap;

use anyhow::{Context, Result};
use egui::{
    Align2, Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, TextureHandle,
    TextureOptions, pos2, vec2,
};

use crate::assets::AssetPack;
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

// ---------------------------------------------------------------------------
// Bitmap font
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

/// Vanilla's bitmap font, loaded from the client jar. Draws through the egui
/// painter as textured quads — pixel-perfect at any integer GUI scale.
pub struct McFont {
    pages: Vec<TextureHandle>,
    glyphs: HashMap<char, Glyph>,
}

/// One text line is 9 GUI px tall (8 px glyph box + 1 px leading).
pub const LINE_H: f32 = 9.0;

impl McFont {
    fn load(pack: &mut AssetPack, ctx: &egui::Context) -> Result<Self> {
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
                continue; // skip space/unihex providers — menus don't need them
            }
            let Some(file) = prov.get("file").and_then(|f| f.as_str()) else { continue };
            // "minecraft:font/ascii.png" → texture ref "font/ascii.png"
            let tex_ref = file.strip_prefix("minecraft:").unwrap_or(file);
            let img = match pack.texture_png(tex_ref) {
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
        Ok(Self { pages, glyphs })
    }

    /// Width of `text` in points at GUI scale `s`.
    pub fn width(&self, text: &str, s: f32) -> f32 {
        text.chars()
            .map(|c| self.glyphs.get(&c).map_or(4.0, |g| g.advance))
            .sum::<f32>()
            * s
    }

    /// Draw one line, top-left at `pos`. Returns the advance (points).
    pub fn draw(
        &self,
        painter: &egui::Painter,
        pos: Pos2,
        text: &str,
        s: f32,
        color: Color32,
        shadow: bool,
    ) -> f32 {
        if shadow {
            let sc = Color32::from_rgba_unmultiplied(
                (color.r() as u32 * 63 / 255) as u8,
                (color.g() as u32 * 63 / 255) as u8,
                (color.b() as u32 * 63 / 255) as u8,
                color.a(),
            );
            self.draw_run(painter, pos + vec2(s, s), text, s, sc);
        }
        self.draw_run(painter, pos, text, s, color)
    }

    fn draw_run(&self, painter: &egui::Painter, pos: Pos2, text: &str, s: f32, color: Color32) -> f32 {
        let mut x = pos.x;
        for c in text.chars() {
            let Some(g) = self.glyphs.get(&c) else {
                x += 4.0 * s;
                continue;
            };
            if g.w > 0.0 && c != ' ' {
                let rect = Rect::from_min_size(
                    pos2(x, pos.y + g.top * s),
                    vec2(g.w * s, g.h * s),
                );
                painter.image(self.pages[g.page as usize].id(), rect, g.uv, color);
            }
            x += g.advance * s;
        }
        x - pos.x
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
    pub heart_container: TextureHandle,
    pub heart_full: TextureHandle,
    pub heart_half: TextureHandle,
    pub food_empty: TextureHandle,
    pub food_full: TextureHandle,
    pub food_half: TextureHandle,
    pub xp_bg: TextureHandle,
    pub xp_progress: TextureHandle,
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
    pub tex: McTextures,
}

impl McUi {
    pub fn load(pack: &mut AssetPack, ctx: &egui::Context) -> Result<Self> {
        let font = McFont::load(pack, ctx)?;
        let t = |p: &mut AssetPack, r: &str| load_tex(p, ctx, r);
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
            heart_container: t(pack, "gui/sprites/hud/heart/container")?,
            heart_full: t(pack, "gui/sprites/hud/heart/full")?,
            heart_half: t(pack, "gui/sprites/hud/heart/half")?,
            food_empty: t(pack, "gui/sprites/hud/food_empty")?,
            food_full: t(pack, "gui/sprites/hud/food_full")?,
            food_half: t(pack, "gui/sprites/hud/food_half")?,
            xp_bg: t(pack, "gui/sprites/hud/experience_bar_background")?,
            xp_progress: t(pack, "gui/sprites/hud/experience_bar_progress")?,
        };
        Ok(Self { font, tex })
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
    enabled && resp.clicked()
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

/// A vanilla text box: black fill, grey border (white when focused). The
/// content is an egui `TextEdit` (for cursor/selection/IME support).
pub fn text_field(
    ui: &mut egui::Ui,
    mc: &McUi,
    w: f32,
    s: f32,
    buf: &mut String,
    hint: &str,
) -> Response {
    let _ = mc;
    let (rect, _) = ui.allocate_exact_size(vec2(w * s, BTN_H * s), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
    let inner = rect.shrink2(vec2(4.0 * s, 0.0));
    let resp = ui.put(
        inner,
        egui::TextEdit::singleline(buf)
            .frame(egui::Frame::NONE)
            .font(egui::FontId::monospace(7.5 * s))
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
        .rect_stroke(rect, 0.0, Stroke::new(s.max(1.0), border), StrokeKind::Inside);
    resp
}

/// Left-aligned label in the Minecraft font (one line, GUI px sizing).
pub fn label(ui: &mut egui::Ui, mc: &McUi, s: f32, text: &str, color: Color32) {
    let w = mc.font.width(text, s);
    let (rect, _) = ui.allocate_exact_size(vec2(w, LINE_H * s), Sense::hover());
    mc.font.draw(ui.painter(), rect.min, text, s, color, true);
}
