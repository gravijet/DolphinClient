//! Container screens (inventory, chests, furnaces, villagers, …) drawn with
//! the real vanilla GUI textures and slot layouts. Clicks are translated to
//! `HudAction::SlotClick` which the app forwards to the server.

use egui::{Color32, Id, LayerId, Order, Rect, TextureId, pos2, vec2};
use std::sync::Arc;

use crate::app::hud::HudAction;
use crate::app::mcui::{McUi, tile_background};
use crate::assets::Lang;
use crate::assets::items::ItemIcons;
use crate::bridge::events::{ChatSpan, ItemSnapshot, SlotClickKind, TradeOffer};

/// The GUI state of the currently open container screen.
pub struct ContainerView {
    /// Server window id (0 = the player's own inventory).
    pub id: i32,
    pub kind: String,
    pub title: Vec<ChatSpan>,
    pub slots: Vec<Option<ItemSnapshot>>,
    pub carried: Option<ItemSnapshot>,
    /// Villager trades (merchant menus only).
    pub offers: Vec<TradeOffer>,
    pub trade_scroll: usize,
}

impl ContainerView {
    pub fn own_inventory(slots: Vec<Option<ItemSnapshot>>, carried: Option<ItemSnapshot>) -> Self {
        Self {
            id: 0,
            kind: "player".into(),
            title: vec![ChatSpan::plain("Inventar")],
            slots,
            carried,
            offers: Vec::new(),
            trade_scroll: 0,
        }
    }
}

/// Resolved screen layout: which texture, window size, one position per slot
/// (menu order, GUI px of the slot's 16×16 interior).
pub struct Layout {
    pub tex_kind: &'static str,
    pub w: f32,
    pub h: f32,
    pub slots: Vec<(f32, f32)>,
    /// For generic chests: row count (drives the two-piece texture draw).
    pub generic_rows: Option<usize>,
}

/// 27 main-inventory + 9 hotbar positions starting at `(x, y_main)`.
fn player_block(x: f32, y_main: f32, y_hotbar: f32, out: &mut Vec<(f32, f32)>) {
    for row in 0..3 {
        for col in 0..9 {
            out.push((x + col as f32 * 18.0, y_main + row as f32 * 18.0));
        }
    }
    for col in 0..9 {
        out.push((x + col as f32 * 18.0, y_hotbar));
    }
}

fn grid(x: f32, y: f32, cols: usize, count: usize, out: &mut Vec<(f32, f32)>) {
    for i in 0..count {
        out.push((
            x + (i % cols) as f32 * 18.0,
            y + (i / cols) as f32 * 18.0,
        ));
    }
}

/// Layout of a menu kind. `total` = slot count reported by the server
/// (container part + 36 player slots).
pub fn layout_for(kind: &str, total: usize) -> Layout {
    let mut slots = Vec::with_capacity(total);
    match kind {
        "player" => {
            // craft_result, craft ×4, armor ×4, inv 27+9, offhand.
            slots.push((154.0, 28.0));
            grid(98.0, 18.0, 2, 4, &mut slots);
            for i in 0..4 {
                slots.push((8.0, 8.0 + i as f32 * 18.0));
            }
            player_block(8.0, 84.0, 142.0, &mut slots);
            slots.push((77.0, 62.0));
            Layout { tex_kind: "player", w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        k if k.starts_with("generic_9x") || k == "shulker_box" => {
            let rows = if k == "shulker_box" {
                3
            } else {
                k.trim_start_matches("generic_9x").parse::<usize>().unwrap_or(3).clamp(1, 6)
            };
            grid(8.0, 18.0, 9, rows * 9, &mut slots);
            let y = rows as f32 * 18.0;
            player_block(8.0, y + 31.0, y + 89.0, &mut slots);
            if k == "shulker_box" {
                Layout { tex_kind: "shulker_box", w: 176.0, h: 166.0, slots, generic_rows: None }
            } else {
                let tex_kind: &'static str = match rows {
                    1 => "generic_9x1",
                    2 => "generic_9x2",
                    3 => "generic_9x3",
                    4 => "generic_9x4",
                    5 => "generic_9x5",
                    _ => "generic_9x6",
                };
                Layout {
                    tex_kind,
                    w: 176.0,
                    h: 114.0 + rows as f32 * 18.0,
                    slots,
                    generic_rows: Some(rows),
                }
            }
        }
        "generic_3x3" | "crafter_3x3" => {
            let x = if kind == "generic_3x3" { 62.0 } else { 26.0 };
            grid(x, 17.0, 3, 9, &mut slots);
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: kind_static(kind), w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        "crafting" => {
            slots.push((124.0, 35.0));
            grid(30.0, 17.0, 3, 9, &mut slots);
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "crafting", w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        "furnace" | "smoker" | "blast_furnace" => {
            slots.push((56.0, 17.0));
            slots.push((56.0, 53.0));
            slots.push((116.0, 35.0));
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: kind_static(kind), w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        "hopper" => {
            grid(44.0, 20.0, 5, 5, &mut slots);
            player_block(8.0, 51.0, 109.0, &mut slots);
            Layout { tex_kind: "hopper", w: 176.0, h: 133.0, slots, generic_rows: None }
        }
        "merchant" => {
            slots.push((136.0, 37.0));
            slots.push((162.0, 37.0));
            slots.push((220.0, 38.0));
            player_block(108.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "merchant", w: 276.0, h: 166.0, slots, generic_rows: None }
        }
        "brewing_stand" => {
            slots.push((56.0, 51.0));
            slots.push((79.0, 58.0));
            slots.push((102.0, 51.0));
            slots.push((79.0, 17.0));
            slots.push((17.0, 17.0));
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "brewing_stand", w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        "enchantment" => {
            slots.push((15.0, 47.0));
            slots.push((35.0, 47.0));
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "enchantment", w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        "anvil" => {
            slots.push((27.0, 47.0));
            slots.push((76.0, 47.0));
            slots.push((134.0, 47.0));
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "anvil", w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        "grindstone" => {
            slots.push((49.0, 19.0));
            slots.push((49.0, 40.0));
            slots.push((129.0, 34.0));
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "grindstone", w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        "loom" => {
            slots.push((13.0, 26.0));
            slots.push((33.0, 26.0));
            slots.push((23.0, 45.0));
            slots.push((143.0, 58.0));
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "loom", w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        "cartography_table" => {
            slots.push((15.0, 15.0));
            slots.push((15.0, 52.0));
            slots.push((145.0, 39.0));
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "cartography_table", w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        "stonecutter" => {
            slots.push((20.0, 33.0));
            slots.push((143.0, 33.0));
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "stonecutter", w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        "smithing" => {
            slots.push((8.0, 48.0));
            slots.push((26.0, 48.0));
            slots.push((44.0, 48.0));
            slots.push((98.0, 48.0));
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "smithing", w: 176.0, h: 166.0, slots, generic_rows: None }
        }
        _ => {
            // Unknown kind: draw the container part as generic 9-wide rows.
            let container = total.saturating_sub(36);
            let rows = container.div_ceil(9).clamp(1, 6);
            grid(8.0, 18.0, 9, container, &mut slots);
            let y = rows as f32 * 18.0;
            player_block(8.0, y + 31.0, y + 89.0, &mut slots);
            Layout {
                tex_kind: "generic_9x3",
                w: 176.0,
                h: 114.0 + rows as f32 * 18.0,
                slots,
                generic_rows: Some(rows),
            }
        }
    }
}

fn kind_static(kind: &str) -> &'static str {
    match kind {
        "generic_3x3" => "generic_3x3",
        "crafter_3x3" => "crafter_3x3",
        "furnace" => "furnace",
        "smoker" => "smoker",
        "blast_furnace" => "blast_furnace",
        _ => "generic_9x3",
    }
}

/// Draw an item icon + stack count into `rect` (16×16 GUI px cell).
pub fn draw_item(
    painter: &egui::Painter,
    mc: &McUi,
    icons: &Option<(TextureId, Arc<ItemIcons>)>,
    rect: Rect,
    item: &ItemSnapshot,
    s: f32,
) {
    let drawn = icons.as_ref().and_then(|(tex, icons)| {
        // Enchanted stacks get the scrolling glint composited into their own
        // little texture; everything else samples the shared icon atlas.
        if item.enchanted {
            let phase = painter.ctx().input(|i| i.time) as f32;
            if let Some(id) = mc.glint_texture(painter.ctx(), icons, &item.item, phase) {
                painter.image(id, rect, FULL_UV, Color32::WHITE);
                return Some(());
            }
        }
        let uv = icons.uv(&item.item)?;
        let uv_rect = Rect::from_min_max(pos2(uv[0], uv[1]), pos2(uv[2], uv[3]));
        painter.image(*tex, rect, uv_rect, Color32::WHITE);
        Some(())
    });
    if drawn.is_none() {
        let name: String = item.item.chars().take(3).collect();
        mc.font.draw_anchored(
            painter,
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &name,
            s * 0.75,
            Color32::WHITE,
            true,
        );
    }
    // Vanilla durability bar: a 13×2 GUI-pixel bar two pixels above the slot's
    // bottom edge — a black track with a fill that runs green → red as the item
    // wears out (hue 1/3 → 0 of the remaining fraction).
    if item.damage > 0 && item.max_damage > 0 {
        let left = item.max_damage.saturating_sub(item.damage) as f32 / item.max_damage as f32;
        let px = rect.width() / 16.0;
        let x0 = rect.left() + 2.0 * px;
        let y0 = rect.bottom() - 3.0 * px;
        let track = Rect::from_min_size(pos2(x0, y0), vec2(13.0 * px, 2.0 * px));
        painter.rect_filled(track, 0.0, Color32::BLACK);
        let fill = Rect::from_min_size(
            pos2(x0, y0),
            vec2((13.0 * left).round().max(0.0) * px, 1.0 * px),
        );
        let (r, g, b) = hsv_rgb(left / 3.0, 1.0, 1.0);
        painter.rect_filled(fill, 0.0, Color32::from_rgb(r, g, b));
    }
    if item.count > 1 {
        mc.font.draw_anchored(
            painter,
            rect.right_bottom() + vec2(1.0 * s, 1.0 * s),
            egui::Align2::RIGHT_BOTTOM,
            &item.count.to_string(),
            s,
            Color32::WHITE,
            true,
        );
    }
}

/// The whole 0..1 texture, for images that have a texture to themselves.
const FULL_UV: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(1.0, 1.0) };

/// Vanilla's `Mth.hsvToRgb` for the durability bar (saturation/value 1).
fn hsv_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match (i as i32).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    (
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    )
}

/// Vanilla item tooltip: the display name (server custom name if present, else
/// the translated registry name) on the first line, then any lore lines below.
#[allow(clippy::too_many_arguments)]
pub fn tooltip(
    painter: &egui::Painter,
    mc: &McUi,
    s: f32,
    lang: &Lang,
    screen: Rect,
    p: egui::Pos2,
    item: &ItemSnapshot,
    time: f64,
) {
    // Line 0 = name; the rest = lore. Each line is a list of styled spans.
    let mut lines: Vec<Vec<ChatSpan>> = Vec::with_capacity(1 + item.lore.len());
    match &item.name {
        Some(spans) if spans.iter().any(|sp| !sp.text.is_empty()) => lines.push(spans.clone()),
        _ => lines.push(vec![ChatSpan::plain(lang.item_name(&item.item))]),
    }
    lines.extend(item.lore.iter().cloned());

    let line_h = 10.0 * s;
    let pad = 4.0 * s;
    let w = lines
        .iter()
        .map(|l| mc.font.spans_width(l, s))
        .fold(0.0_f32, f32::max)
        + pad * 2.0;
    let h = pad * 2.0 + line_h * lines.len() as f32;
    let tp = pos2(
        (p.x + 12.0 * s).min(screen.right() - w).max(screen.left()),
        (p.y - 12.0 * s).clamp(screen.top(), screen.bottom() - h),
    );
    painter.rect_filled(
        Rect::from_min_size(tp, vec2(w, h)),
        1.0 * s,
        Color32::from_rgba_unmultiplied(16, 0, 16, 240),
    );
    for (i, line) in lines.iter().enumerate() {
        // Name defaults to white, lore to vanilla gray (spans keep their own color).
        let default = if i == 0 {
            Color32::WHITE
        } else {
            Color32::from_rgb(0xAA, 0xAA, 0xAA)
        };
        mc.font.draw_spans(
            painter,
            tp + vec2(pad, pad + i as f32 * line_h),
            line,
            s,
            default,
            1.0,
            true,
            time,
        );
    }
}

/// Draw the whole container screen; emits SlotClick / SelectTrade actions.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    view: &mut ContainerView,
    icons: &Option<(TextureId, Arc<ItemIcons>)>,
    lang: &Lang,
    // Our own-skin paper-doll (16×32) for the inventory preview panel.
    player_body: Option<TextureId>,
    actions: &mut Vec<HudAction>,
) {
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("container")));
    let screen = ctx.content_rect();
    // Vanilla-style darkened world behind the window.
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha(176));

    let layout = layout_for(&view.kind, view.slots.len());
    let win = Rect::from_center_size(
        screen.center(),
        vec2(layout.w * s, layout.h * s),
    );

    // --- window texture ------------------------------------------------------
    if let Some(tex) = mc.tex.containers.get(layout.tex_kind) {
        let ts = tex.size_vec2();
        match layout.generic_rows {
            Some(rows) => {
                // generic_54.png: top piece (0,0)-(176, rows*18+17), then the
                // 96-px player section from y=126.
                let top_h = rows as f32 * 18.0 + 17.0;
                painter.image(
                    tex.id(),
                    Rect::from_min_size(win.min, vec2(176.0 * s, top_h * s)),
                    Rect::from_min_max(
                        pos2(0.0, 0.0),
                        pos2(176.0 / ts.x, top_h / ts.y),
                    ),
                    Color32::WHITE,
                );
                painter.image(
                    tex.id(),
                    Rect::from_min_size(
                        win.min + vec2(0.0, top_h * s),
                        vec2(176.0 * s, 96.0 * s),
                    ),
                    Rect::from_min_max(
                        pos2(0.0, 126.0 / ts.y),
                        pos2(176.0 / ts.x, 222.0 / ts.y),
                    ),
                    Color32::WHITE,
                );
            }
            None => {
                painter.image(
                    tex.id(),
                    win,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(layout.w / ts.x, layout.h / ts.y)),
                    Color32::WHITE,
                );
            }
        }
    } else {
        // No texture (very old jar?): plain box.
        tile_background(&painter, &mc.tex.menu_bg, win, s, Color32::WHITE);
    }

    // --- player paper-doll (own inventory preview panel) ----------------------
    if view.kind == "player"
        && let Some(body) = player_body
    {
        // Recessed panel in the vanilla inventory sits at ~x 26..73, y 8..70.
        // The doll is 16×32 px; draw it centered there, NEAREST-scaled.
        let doll = Rect::from_center_size(
            win.min + vec2(49.0 * s, 40.0 * s),
            vec2(26.0 * s, 52.0 * s),
        );
        painter.image(
            body,
            doll,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    // --- title -----------------------------------------------------------------
    let title_x = if view.kind == "merchant" { 110.0 } else { 8.0 };
    mc.font.draw_spans(
        &painter,
        win.min + vec2(title_x * s, 6.0 * s),
        &view.title,
        s,
        Color32::from_rgb(0x40, 0x40, 0x40),
        1.0,
        false,
        ctx.input(|i| i.time),
    );

    // --- slots -----------------------------------------------------------------
    let pointer = ctx.pointer_latest_pos();
    let (lclick, rclick, shift, throw) = ctx.input(|i| {
        (
            i.pointer.primary_clicked(),
            i.pointer.secondary_clicked(),
            i.modifiers.shift,
            i.key_pressed(egui::Key::Q),
        )
    });
    let mut hover_item: Option<ItemSnapshot> = None;
    for (i, pos) in layout.slots.iter().enumerate() {
        if i >= view.slots.len() {
            break;
        }
        let rect = Rect::from_min_size(
            win.min + vec2(pos.0 * s, pos.1 * s),
            vec2(16.0 * s, 16.0 * s),
        );
        if let Some(item) = &view.slots[i] {
            draw_item(&painter, mc, icons, rect, item, s);
        }
        if pointer.is_some_and(|p| rect.contains(p)) {
            painter.rect_filled(rect, 0.0, Color32::from_white_alpha(110));
            hover_item = view.slots[i].clone();
            if lclick || rclick || throw {
                let kind = if throw {
                    SlotClickKind::Throw
                } else if shift {
                    SlotClickKind::QuickMove
                } else if rclick {
                    SlotClickKind::Right
                } else {
                    SlotClickKind::Left
                };
                actions.push(HudAction::SlotClick {
                    window_id: view.id,
                    slot: i as u16,
                    kind,
                });
            }
        }
    }

    // --- villager trades ---------------------------------------------------------
    if view.kind == "merchant" && !view.offers.is_empty() {
        draw_trades(ctx, mc, s, view, icons, &painter, win, actions);
    }

    // --- carried item / tooltip ---------------------------------------------------
    if let Some(p) = pointer {
        if let Some(item) = &view.carried {
            let rect = Rect::from_center_size(p, vec2(16.0 * s, 16.0 * s));
            draw_item(&painter, mc, icons, rect, item, s);
        } else if let Some(item) = &hover_item {
            tooltip(&painter, mc, s, lang, screen, p, item, ctx.input(|i| i.time));
        }
    }
}

/// The merchant trade list (left panel of the villager GUI).
#[allow(clippy::too_many_arguments)]
fn draw_trades(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    view: &mut ContainerView,
    icons: &Option<(TextureId, Arc<ItemIcons>)>,
    painter: &egui::Painter,
    win: Rect,
    actions: &mut Vec<HudAction>,
) {
    const VISIBLE: usize = 7;
    let list = Rect::from_min_size(
        win.min + vec2(5.0 * s, 17.0 * s),
        vec2(97.0 * s, (VISIBLE as f32) * 20.0 * s),
    );
    let pointer = ctx.pointer_latest_pos();
    // Wheel scrolling over the list.
    if pointer.is_some_and(|p| list.contains(p)) {
        let d = ctx.input(|i| i.smooth_scroll_delta.y);
        if d < -0.5 {
            view.trade_scroll =
                (view.trade_scroll + 1).min(view.offers.len().saturating_sub(VISIBLE));
        } else if d > 0.5 {
            view.trade_scroll = view.trade_scroll.saturating_sub(1);
        }
    }
    let clicked = ctx.input(|i| i.pointer.primary_clicked());
    for (row, idx) in (view.trade_scroll..view.offers.len()).take(VISIBLE).enumerate() {
        let offer = &view.offers[idx];
        let r = Rect::from_min_size(
            pos2(list.left(), list.top() + row as f32 * 20.0 * s),
            vec2(list.width(), 20.0 * s),
        );
        let hovered = pointer.is_some_and(|p| r.contains(p));
        if hovered {
            painter.rect_filled(r, 0.0, Color32::from_white_alpha(60));
            if clicked && !offer.disabled {
                actions.push(HudAction::SelectTrade { index: idx as u32 });
            }
        }
        let tint = if offer.disabled { 0.4 } else { 1.0 };
        let cell = |x: f32| {
            Rect::from_min_size(
                pos2(r.left() + x * s, r.top() + 2.0 * s),
                vec2(16.0 * s, 16.0 * s),
            )
        };
        let _ = tint;
        draw_item(painter, mc, icons, cell(2.0), &offer.input_a, s);
        if let Some(b) = &offer.input_b {
            draw_item(painter, mc, icons, cell(22.0), b, s);
        }
        // Arrow.
        mc.font.draw(
            painter,
            pos2(r.left() + 44.0 * s, r.top() + 6.0 * s),
            if offer.disabled { "x" } else { ">" },
            s,
            if offer.disabled {
                Color32::from_rgb(0xFF, 0x55, 0x55)
            } else {
                Color32::from_rgb(0x40, 0x40, 0x40)
            },
            false,
        );
        draw_item(painter, mc, icons, cell(60.0), &offer.output, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_layouts_have_all_slots() {
        for rows in 1..=6usize {
            let kind = format!("generic_9x{rows}");
            let total = rows * 9 + 36;
            let l = layout_for(&kind, total);
            assert_eq!(l.slots.len(), total, "{kind}");
            assert_eq!(l.generic_rows, Some(rows));
        }
    }

    #[test]
    fn player_layout_matches_menu_order() {
        // 46 slots: result, 4 craft, 4 armor, 27 inv, 9 hotbar, offhand.
        let l = layout_for("player", 46);
        assert_eq!(l.slots.len(), 46);
        assert_eq!(l.slots[0], (154.0, 28.0)); // craft result
        assert_eq!(l.slots[5], (8.0, 8.0)); // helmet
        assert_eq!(l.slots[9], (8.0, 84.0)); // first main slot
        assert_eq!(l.slots[36], (8.0, 142.0)); // first hotbar slot
        assert_eq!(l.slots[45], (77.0, 62.0)); // offhand
    }

    #[test]
    fn unknown_kind_falls_back_to_generic() {
        let l = layout_for("beacon", 1 + 36);
        assert_eq!(l.slots.len(), 37);
        assert!(l.generic_rows.is_some());
    }
}
