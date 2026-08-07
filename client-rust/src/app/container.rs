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
    /// What has been typed into the anvil's name field.
    pub rename: String,
    /// The last name sent to the server, so we only send on change.
    pub rename_sent: String,
}

impl ContainerView {
    pub fn own_inventory(slots: Vec<Option<ItemSnapshot>>, carried: Option<ItemSnapshot>) -> Self {
        Self {
            id: 0,
            kind: "player".into(),
            title: vec![ChatSpan::plain("Inventory")],
            slots,
            carried,
            offers: Vec::new(),
            trade_scroll: 0,
            rename: String::new(),
            rename_sent: String::new(),
        }
    }
}

/// The live, server-driven part of a container screen: the properties the
/// server streams (`ClientboundContainerSetData`), the composited filled maps
/// and the enchantment registry the offers index into.
pub struct LiveData<'a> {
    pub props: &'a std::collections::HashMap<u16, u16>,
    pub maps: &'a std::collections::HashMap<u32, TextureId>,
    pub enchantments: &'a [String],
    pub trim_patterns: &'a [String],
    pub trim_materials: &'a [String],
}

impl LiveData<'_> {
    fn prop(&self, id: u16) -> u32 {
        self.props.get(&id).copied().unwrap_or(0) as u32
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
    registries: &Registries<'_>,
    time: f64,
) {
    let lines = tooltip_lines(lang, item, registries);

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
    live: &LiveData<'_>,
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

    // --- live, server-driven parts ----------------------------------------------
    // Drawn between the window texture and the slots, exactly where vanilla
    // draws them: the furnace flame burns behind the fuel slot, the arrow runs
    // under the item, the enchantment rows sit behind their level sprites.
    draw_live(ctx, mc, s, view, live, win, lang, actions);

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
            let reg = Registries {
                enchantments: live.enchantments,
                trim_patterns: live.trim_patterns,
                trim_materials: live.trim_materials,
            };
            tooltip(&painter, mc, s, lang, screen, p, item, &reg, ctx.input(|i| i.time));
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


    fn lang() -> Lang {
        Lang::empty()
    }

    fn stack(item: &str) -> ItemSnapshot {
        ItemSnapshot { item: item.into(), count: 1, ..Default::default() }
    }

    fn text_of(lines: &[Vec<ChatSpan>]) -> Vec<String> {
        lines.iter().map(|l| crate::bridge::events::spans_to_plain(l)).collect()
    }

    #[test]
    fn a_plain_item_tooltip_is_just_its_name() {
        let lines = tooltip_lines(&lang(), &stack("stone"), &Registries::EMPTY);
        assert_eq!(text_of(&lines), vec!["Stone".to_string()]);
    }

    #[test]
    fn enchantments_are_named_and_numbered() {
        let reg = Registries {
            enchantments: &["sharpness".to_string(), "mending".to_string()],
            trim_patterns: &[],
            trim_materials: &[],
        };
        let mut item = stack("diamond_sword");
        item.enchantments = vec![(0, 4), (1, 1)];
        let lines = text_of(&tooltip_lines(&lang(), &item, &reg));
        assert!(lines.iter().any(|l| l.contains("Sharpness") && l.ends_with("IV")), "{lines:?}");
        // Mending only ever comes at level I, so vanilla leaves the numeral off.
        assert!(lines.iter().any(|l| l.trim() == "Mending"), "{lines:?}");
    }

    #[test]
    fn a_potion_lists_its_effects_with_a_clock() {
        let mut item = stack("potion");
        item.effects = vec![("strength".into(), 1, 20 * 185)];
        let lines = text_of(&tooltip_lines(&lang(), &item, &Registries::EMPTY));
        // Amplifier 1 is level II, and 185 s reads as 3:05.
        assert!(lines.iter().any(|l| l.contains("II") && l.contains("(3:05)")), "{lines:?}");
    }

    #[test]
    fn attribute_modifiers_show_their_sign() {
        let mut item = stack("netherite_sword");
        item.modifiers = vec![
            ("attack_damage".into(), 7.0, 0),
            ("attack_speed".into(), -2.4, 0),
        ];
        let lines = text_of(&tooltip_lines(&lang(), &item, &Registries::EMPTY));
        assert!(lines.iter().any(|l| l.starts_with("+7 ")), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("-2.4 ")), "{lines:?}");
    }

    #[test]
    fn durability_only_shows_once_something_is_worn() {
        let mut item = stack("iron_pickaxe");
        item.max_damage = 250;
        let fresh = text_of(&tooltip_lines(&lang(), &item, &Registries::EMPTY));
        assert_eq!(fresh.len(), 1, "a mint tool shows no durability line");
        item.damage = 50;
        let worn = text_of(&tooltip_lines(&lang(), &item, &Registries::EMPTY));
        assert!(worn.iter().any(|l| l.contains("200") && l.contains("250")), "{worn:?}");
    }

    #[test]
    fn modifier_numbers_lose_their_trailing_zeros() {
        assert_eq!(format_amount(7.0, ""), "7");
        assert_eq!(format_amount(2.4, ""), "2.4");
        assert_eq!(format_amount(10.0, "%"), "10%");
    }

    #[test]
    fn every_offer_row_gets_its_own_rune_sentence() {
        let a = enchant_clue(4242, 0);
        let b = enchant_clue(4242, 1);
        let c = enchant_clue(4242, 2);
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn the_rune_sentence_is_stable_for_one_seed() {
        assert_eq!(enchant_clue(7, 1), enchant_clue(7, 1));
        assert_ne!(enchant_clue(7, 1), enchant_clue(8, 1));
    }

    #[test]
    fn enchantment_levels_read_as_roman_numerals() {
        assert_eq!(roman(1), "I");
        assert_eq!(roman(4), "IV");
        assert_eq!(roman(5), "V");
        assert_eq!(roman(9), "IX");
        assert_eq!(roman(10), "X");
        assert_eq!(roman(0), "");
    }

    #[test]
    fn the_anvil_layout_puts_three_slots_before_the_player_rows() {
        let layout = layout_for("anvil", 39);
        assert_eq!(layout.slots.len(), 39);
        // The output slot is the third one, on the right of the window.
        assert!(layout.slots[2].0 > layout.slots[0].0);
    }

}


/// Draw one sub-rectangle of a standalone sprite: vanilla's `blitSprite` with
/// an explicit source rect, which is how every progress bar is animated.
#[allow(clippy::too_many_arguments)]
fn blit_part(
    painter: &egui::Painter,
    tex: &egui::TextureHandle,
    win: Rect,
    s: f32,
    // Sprite-space source rect.
    (u, v, sw, sh): (f32, f32, f32, f32),
    // Window-relative destination corner.
    (x, y): (f32, f32),
) {
    if sw <= 0.0 || sh <= 0.0 {
        return;
    }
    let size = tex.size_vec2();
    let uv = Rect::from_min_max(
        pos2(u / size.x, v / size.y),
        pos2((u + sw) / size.x, (v + sh) / size.y),
    );
    painter.image(
        tex.id(),
        Rect::from_min_size(win.min + vec2(x * s, y * s), vec2(sw * s, sh * s)),
        uv,
        Color32::WHITE,
    );
}

/// Vanilla's `EnchantmentNames` word pool: the enchanting table writes a random
/// sentence from it in galactic runes. It is decoration — the real enchantment
/// only ever shows in the tooltip.
const ENCHANT_WORDS: &[&str] = &[
    "the", "elder", "scrolls", "klaatu", "berata", "niktu", "xyzzy", "bless", "curse", "light",
    "darkness", "fire", "air", "earth", "water", "hot", "dry", "cold", "wet", "ignite", "snuff",
    "embiggen", "twist", "shorten", "stretch", "fiddle", "destroy", "imbue", "galvanize",
    "enchant", "free", "limited", "unlimited", "within", "without", "gauntlet", "break",
    "curved", "sharpened", "hexed", "banished", "hallowed", "spectral", "sinister",
];

/// Pick a deterministic rune sentence for one offer row, seeded like vanilla
/// (the server's enchantment seed plus the row index).
fn enchant_clue(seed: u32, row: usize) -> String {
    // A small xorshift keeps this reproducible without pulling in a PRNG.
    let mut state = (seed ^ (row as u32).wrapping_mul(0x9E37_79B9)).wrapping_add(0x8544_2761) | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    // Warm the state up so neighbouring rows don't start out alike.
    for _ in 0..4 {
        next();
    }
    let count = 3 + (next() % 3) as usize;
    (0..count)
        .map(|_| ENCHANT_WORDS[(next() as usize) % ENCHANT_WORDS.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

/// Roman numeral for an enchantment level, like vanilla's tooltips.
fn roman(level: u32) -> String {
    const TABLE: [(u32, &str); 8] =
        [(10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (3, "III"), (2, "II"), (1, "I"), (0, "")];
    let mut out = String::new();
    let mut n = level;
    for (value, sym) in TABLE {
        while value > 0 && n >= value {
            out.push_str(sym);
            n -= value;
        }
    }
    out
}

/// The parts of a container screen the server drives: furnace flames, cook and
/// brew progress, enchantment offers, the anvil's cost and name field, and the
/// map a cartography table is working on.
#[allow(clippy::too_many_arguments)]
fn draw_live(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    view: &mut ContainerView,
    live: &LiveData<'_>,
    win: Rect,
    lang: &Lang,
    actions: &mut Vec<HudAction>,
) {
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("container")));
    let sprite = |name: &str| mc.tex.container_sprites.get(name);
    match view.kind.as_str() {
        // Smelting: the flame burns down as the fuel is used, the arrow fills
        // as the item cooks.
        "furnace" | "smoker" | "blast_furnace" => {
            let (lit, lit_total) = (live.prop(0), live.prop(1));
            let (cook, cook_total) = (live.prop(2), live.prop(3));
            if lit > 0 && lit_total > 0
                && let Some(tex) = sprite(&format!("{}/lit_progress", view.kind))
            {
                // Vanilla: 13 pixels of flame plus one, bottom-aligned.
                let k = ((lit * 13) / lit_total + 1).min(14) as f32;
                blit_part(&painter, tex, win, s, (0.0, 14.0 - k, 14.0, k), (56.0, 36.0 + 14.0 - k));
            }
            if cook > 0 && cook_total > 0
                && let Some(tex) = sprite(&format!("{}/burn_progress", view.kind))
            {
                let k = ((cook * 24) / cook_total).min(24) as f32;
                blit_part(&painter, tex, win, s, (0.0, 0.0, k, 16.0), (79.0, 34.0));
            }
        }
        // Brewing: the fuel bar on the left, the arrow filling downward and the
        // bubbles rising in their seven-step loop.
        "brewing_stand" => {
            let (brew, fuel) = (live.prop(0), live.prop(1));
            if fuel > 0
                && let Some(tex) = sprite("brewing_stand/fuel_length")
            {
                let k = ((18 * fuel).div_ceil(20).min(18)) as f32;
                blit_part(&painter, tex, win, s, (0.0, 0.0, k, 4.0), (60.0, 44.0));
            }
            if brew > 0 {
                if let Some(tex) = sprite("brewing_stand/brew_progress") {
                    let k = (28.0 * (1.0 - brew as f32 / 400.0)).floor().clamp(0.0, 28.0);
                    blit_part(&painter, tex, win, s, (0.0, 0.0, 9.0, k), (97.0, 16.0));
                }
                if let Some(tex) = sprite("brewing_stand/bubbles") {
                    const LENGTHS: [f32; 7] = [29.0, 24.0, 20.0, 16.0, 11.0, 6.0, 0.0];
                    let k = LENGTHS[(brew as usize / 2) % 7];
                    if k > 0.0 {
                        blit_part(
                            &painter,
                            tex,
                            win,
                            s,
                            (0.0, 29.0 - k, 12.0, k),
                            (63.0, 14.0 + 29.0 - k),
                        );
                    }
                }
            }
        }
        // The enchanting table: three offers, each a level cost, a numeral
        // sprite and an unreadable rune sentence.
        "enchantment" => {
            let pointer = ctx.pointer_latest_pos();
            let clicked = ctx.input(|i| i.pointer.primary_clicked());
            let seed = live.prop(3);
            let mut tooltip: Option<(egui::Pos2, Vec<ChatSpan>)> = None;
            for row in 0..3usize {
                let cost = live.prop(row as u16);
                let top = 14.0 + 19.0 * row as f32;
                let rect = Rect::from_min_size(
                    win.min + vec2(60.0 * s, top * s),
                    vec2(108.0 * s, 19.0 * s),
                );
                let hovered = pointer.is_some_and(|p| rect.contains(p));
                let enabled = cost > 0;
                let slot_name = if !enabled {
                    "enchanting_table/enchantment_slot_disabled"
                } else if hovered {
                    "enchanting_table/enchantment_slot_highlighted"
                } else {
                    "enchanting_table/enchantment_slot"
                };
                if let Some(tex) = sprite(slot_name) {
                    painter.image(
                        tex.id(),
                        rect,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                if !enabled {
                    continue;
                }
                let level_name = format!("enchanting_table/level_{}", row + 1);
                if let Some(tex) = sprite(&level_name) {
                    painter.image(
                        tex.id(),
                        Rect::from_min_size(
                            win.min + vec2(61.0 * s, (top + 1.0) * s),
                            vec2(16.0 * s, 16.0 * s),
                        ),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                // The rune sentence, clipped to the room vanilla gives it.
                if let Some(sga) = &mc.sga {
                    let clue = enchant_clue(seed, row);
                    let clip = Rect::from_min_size(
                        win.min + vec2(80.0 * s, (top + 2.0) * s),
                        vec2(66.0 * s, 15.0 * s),
                    );
                    sga.draw(
                        &painter.with_clip_rect(clip),
                        clip.min,
                        &clue,
                        s,
                        Color32::from_rgb(0x68, 0x5E, 0x4A),
                    );
                }
                let text = cost.to_string();
                let w = mc.font.width(&text, s);
                mc.font.draw(
                    &painter,
                    win.min + vec2(146.0 * s, (top + 8.0) * s) - vec2(w, 0.0),
                    &text,
                    s,
                    Color32::from_rgb(0x80, 0xFF, 0x20),
                    true,
                );
                if hovered {
                    // The tooltip is where the real enchantment shows up.
                    let id = live.prop(4 + row as u16) as usize;
                    let level = live.prop(7 + row as u16);
                    let name = live
                        .enchantments
                        .get(id)
                        .map(|n| {
                            lang.get(&format!("enchantment.minecraft.{n}"))
                                .unwrap_or(n)
                                .to_string()
                        })
                        .unwrap_or_default();
                    let mut lines = vec![ChatSpan {
                        text: if name.is_empty() {
                            lang.get("container.enchant.clue")
                                .unwrap_or("%s . . . ?")
                                .replace("%s", "?")
                        } else {
                            format!("{name} {}", roman(level))
                        },
                        color: Some([0xAA, 0xAA, 0xAA]),
                        ..Default::default()
                    }];
                    lines.push(ChatSpan {
                        text: lang
                            .get("container.enchant.level.requirement")
                            .unwrap_or("Level Requirement: %s")
                            .replace("%s", &cost.to_string()),
                        color: Some([0x55, 0xFF, 0x55]),
                        ..Default::default()
                    });
                    if let Some(p) = pointer {
                        tooltip = Some((p, lines));
                    }
                    if clicked {
                        actions.push(HudAction::ContainerButton {
                            window_id: view.id,
                            button: row as u8,
                        });
                    }
                }
            }
            if let Some((at, lines)) = tooltip {
                simple_tooltip(&painter, mc, s, at, &lines);
            }
        }
        // The anvil: a name field you can actually type in, plus the level cost.
        "anvil" => {
            if let Some(tex) = sprite("anvil/text_field") {
                painter.image(
                    tex.id(),
                    Rect::from_min_size(
                        win.min + vec2(59.0 * s, 20.0 * s),
                        vec2(110.0 * s, 16.0 * s),
                    ),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            // Typing goes into the name field while the anvil is open.
            let mut changed = false;
            ctx.input(|i| {
                for event in &i.events {
                    match event {
                        egui::Event::Text(text) => {
                            for c in text.chars().filter(|c| !c.is_control()) {
                                if view.rename.chars().count() < 50 {
                                    view.rename.push(c);
                                    changed = true;
                                }
                            }
                        }
                        egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                            view.rename.pop();
                            changed = true;
                        }
                        _ => {}
                    }
                }
            });
            if changed && view.rename != view.rename_sent {
                view.rename_sent = view.rename.clone();
                actions.push(HudAction::RenameItem { name: view.rename.clone() });
            }
            // The caret blinks at 2 Hz, like every vanilla text field.
            let caret = if ctx.input(|i| i.time) % 1.0 < 0.5 { "_" } else { "" };
            mc.font.draw(
                &painter,
                win.min + vec2(62.0 * s, 24.0 * s),
                &format!("{}{caret}", view.rename),
                s,
                Color32::from_gray(0xE0),
                false,
            );
            let cost = live.prop(0);
            if cost > 0 {
                let expensive = cost >= 40;
                let text = if expensive {
                    lang.get("container.repair.expensive").unwrap_or("Too Expensive!").to_string()
                } else {
                    lang.get("container.repair.cost")
                        .unwrap_or("Enchantment Cost: %1$s")
                        .replace("%1$s", &cost.to_string())
                };
                let w = mc.font.width(&text, s);
                let x = 168.0 * s - w - 2.0 * s;
                painter.rect_filled(
                    Rect::from_min_size(win.min + vec2(x - 2.0 * s, 67.0 * s), vec2(w + 4.0 * s, 12.0 * s)),
                    0.0,
                    Color32::from_black_alpha(80),
                );
                mc.font.draw(
                    &painter,
                    win.min + vec2(x, 69.0 * s),
                    &text,
                    s,
                    if expensive {
                        Color32::from_rgb(0xFF, 0x61, 0x60)
                    } else {
                        Color32::from_rgb(0x80, 0xFF, 0x20)
                    },
                    true,
                );
            }
        }
        // The cartography table previews the map you are about to copy or zoom.
        "cartography_table" => {
            let map = view
                .slots
                .first()
                .and_then(|s| s.as_ref())
                .and_then(|item| item.map_id)
                .and_then(|id| live.maps.get(&id));
            if let Some(tex) = map {
                // Vanilla's preview panel sits to the right of the two inputs.
                painter.image(
                    *tex,
                    Rect::from_min_size(
                        win.min + vec2(66.0 * s, 13.0 * s),
                        vec2(66.0 * s, 66.0 * s),
                    ),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
        _ => {}
    }
}

/// A compact dark tooltip for the container screens.
fn simple_tooltip(
    painter: &egui::Painter,
    mc: &McUi,
    s: f32,
    at: egui::Pos2,
    lines: &[ChatSpan],
) {
    let width = lines.iter().map(|l| mc.font.width(&l.text, s)).fold(0.0, f32::max);
    let rect = Rect::from_min_size(
        at + vec2(8.0 * s, -4.0 * s),
        vec2(width + 8.0 * s, lines.len() as f32 * 10.0 * s + 6.0 * s),
    );
    painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(16, 0, 16, 240));
    for (i, line) in lines.iter().enumerate() {
        mc.font.draw_spans(
            painter,
            rect.min + vec2(4.0 * s, (4.0 + i as f32 * 10.0) * s),
            std::slice::from_ref(line),
            s,
            Color32::WHITE,
            1.0,
            true,
            0.0,
        );
    }
}


/// The registries a tooltip needs to turn protocol ids into names.
pub struct Registries<'a> {
    pub enchantments: &'a [String],
    pub trim_patterns: &'a [String],
    pub trim_materials: &'a [String],
}

impl Registries<'_> {
    /// An empty set, for the screens that have nothing to resolve.
    pub const EMPTY: Registries<'static> =
        Registries { enchantments: &[], trim_patterns: &[], trim_materials: &[] };
}

/// Vanilla's grey.
const GREY: [u8; 3] = [0xAA, 0xAA, 0xAA];
/// Vanilla's blue for enchantments and trim lines.
const BLUE: [u8; 3] = [0x55, 0x55, 0xFF];

fn span(text: impl Into<String>, color: [u8; 3]) -> ChatSpan {
    ChatSpan { text: text.into(), color: Some(color), ..Default::default() }
}

/// Everything vanilla writes on an item's tooltip, in vanilla's order: the
/// name, then enchantments, then the potion's effects, then lore, then the
/// attribute modifiers, then durability.
pub fn tooltip_lines(
    lang: &Lang,
    item: &ItemSnapshot,
    reg: &Registries<'_>,
) -> Vec<Vec<ChatSpan>> {
    let mut lines: Vec<Vec<ChatSpan>> = Vec::new();
    match &item.name {
        Some(spans) if spans.iter().any(|sp| !sp.text.is_empty()) => lines.push(spans.clone()),
        _ => lines.push(vec![ChatSpan::plain(lang.item_name(&item.item))]),
    }

    // Enchantments, one per line, named and numbered like vanilla.
    for (id, level) in &item.enchantments {
        let key = reg.enchantments.get(*id as usize);
        let name = key
            .and_then(|k| lang.get(&format!("enchantment.minecraft.{k}")))
            .map(str::to_string)
            .or_else(|| key.map(|k| crate::assets::prettify(k)))
            .unwrap_or_else(|| "Enchantment".to_string());
        // Level I is left off single-level enchantments, exactly like vanilla.
        let text = if *level <= 1 && is_single_level(key.map(String::as_str)) {
            name
        } else {
            format!("{name} {}", roman(*level))
        };
        lines.push(vec![span(text, GREY)]);
    }

    // An armour trim reads as its own little block: a header, then the two
    // halves of what it is made of.
    if let Some((pattern, material)) = item.trim {
        let pat = reg.trim_patterns.get(pattern as usize);
        let mat = reg.trim_materials.get(material as usize);
        if let (Some(pat), Some(mat)) = (pat, mat) {
            lines.push(vec![span(
                lang.get("item.minecraft.smithing_template.upgrade").unwrap_or("Upgrade: "),
                GREY,
            )]);
            let pretty = crate::assets::prettify;
            let material_name = lang
                .get(&format!("trim_material.minecraft.{mat}"))
                .map(str::to_string)
                .unwrap_or_else(|| pretty(mat));
            let pattern_name = lang
                .get(&format!("trim_pattern.minecraft.{pat}"))
                .map(str::to_string)
                .unwrap_or_else(|| format!("{} Armor Trim", pretty(pat)));
            lines.push(vec![span(format!(" {pattern_name}"), BLUE)]);
            lines.push(vec![span(format!(" {material_name}"), BLUE)]);
        }
    }

    // Potion effects: name, level, and how long they last.
    for (effect, amplifier, duration) in &item.effects {
        let name = lang
            .get(&format!("effect.minecraft.{effect}"))
            .map(str::to_string)
            .unwrap_or_else(|| crate::assets::prettify(effect));
        let mut text = name;
        if *amplifier > 0 {
            text.push(' ');
            text.push_str(&roman(*amplifier + 1));
        }
        if *duration > 20 {
            let secs = *duration / 20;
            text.push_str(&format!(" ({}:{:02})", secs / 60, secs % 60));
        }
        // Vanilla colours harmful effects red and helpful ones blue.
        let color = if is_harmful(effect) { [0xFC, 0x54, 0x54] } else { [0x54, 0x54, 0xFC] };
        lines.push(vec![span(text, color)]);
    }

    lines.extend(item.lore.iter().cloned());

    // Attribute modifiers, the way vanilla prints them under the lore.
    for (attribute, amount, operation) in &item.modifiers {
        let name = lang
            .get(&format!("attribute.name.{attribute}"))
            .map(str::to_string)
            .unwrap_or_else(|| crate::assets::prettify(attribute));
        let (value, suffix) = match operation {
            0 => (*amount, ""),
            _ => (*amount * 100.0, "%"),
        };
        // Vanilla writes the sign either way: "+8 Armor", "-5% Speed".
        let text = if value < 0.0 {
            format!("-{} {name}", format_amount(-value, suffix))
        } else {
            format!("+{} {name}", format_amount(value, suffix))
        };
        let color = if value < 0.0 { [0xFC, 0x54, 0x54] } else { [0x54, 0x54, 0xFC] };
        lines.push(vec![span(text, color)]);
    }

    if item.unbreakable {
        lines.push(vec![span(lang.get("item.unbreakable").unwrap_or("Unbreakable"), BLUE)]);
    }
    if item.dyed.is_some() {
        lines.push(vec![span(lang.get("item.dyed").unwrap_or("Dyed"), GREY)]);
    }
    // Durability, but only once the item has actually been used.
    if item.max_damage > 0 && item.damage > 0 {
        let left = item.max_damage.saturating_sub(item.damage);
        lines.push(vec![span(
            lang.get("item.durability")
                .unwrap_or("Durability: %s / %s")
                .replacen("%s", &left.to_string(), 1)
                .replacen("%s", &item.max_damage.to_string(), 1),
            GREY,
        )]);
    }
    lines
}

/// Trim a modifier's number the way vanilla does: no trailing zeros.
fn format_amount(value: f64, suffix: &str) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if (rounded - rounded.round()).abs() < 1e-9 {
        format!("{}{suffix}", rounded.round() as i64)
    } else {
        format!("{rounded}{suffix}")
    }
}

/// Enchantments that only ever come at level I, so vanilla leaves the numeral
/// off entirely.
fn is_single_level(id: Option<&str>) -> bool {
    matches!(
        id,
        Some(
            "aqua_affinity"
                | "channeling"
                | "flame"
                | "infinity"
                | "mending"
                | "multishot"
                | "silk_touch"
                | "binding_curse"
                | "vanishing_curse"
        )
    )
}

/// The effects vanilla prints in red.
fn is_harmful(effect: &str) -> bool {
    matches!(
        effect,
        "slowness"
            | "mining_fatigue"
            | "instant_damage"
            | "nausea"
            | "blindness"
            | "hunger"
            | "weakness"
            | "poison"
            | "wither"
            | "levitation"
            | "unluck"
            | "darkness"
            | "infested"
            | "oozing"
            | "weaving"
            | "wind_charged"
            | "trial_omen"
            | "raid_omen"
            | "bad_omen"
    )
}
