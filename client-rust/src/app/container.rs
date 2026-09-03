//! Container screens (inventory, chests, furnaces, villagers, …) drawn with
//! the real vanilla GUI textures and slot layouts. Clicks are translated to
//! `HudAction::SlotClick` which the app forwards to the server.

use egui::{Color32, Id, LayerId, Order, Rect, TextureId, pos2, vec2};
use std::sync::Arc;

use crate::app::hud::HudAction;
use crate::app::mcui::{McUi, tile_background};
use crate::assets::Lang;
use crate::assets::items::ItemIcons;
use crate::app::recipebook::{BookTab, RecipeBook, Station, craftable, grid_slots};
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
    /// How far down the loom's patterns or the stonecutter's recipes have been
    /// scrolled, 0..1 of the way through the rows that don't fit.
    pub scroll: f32,
    /// The beacon's two chosen effects, before they are confirmed.
    pub beacon_primary: Option<String>,
    pub beacon_secondary: Option<String>,
    /// Whether the beacon's choice has been touched, so the screen stops
    /// following the server's idea of what is active.
    pub beacon_touched: bool,
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
            scroll: 0.0,
            beacon_primary: None,
            beacon_secondary: None,
            beacon_touched: false,
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
    /// Every stonecutter recipe the server knows, in its numbering.
    pub stonecutter: &'a [crate::bridge::events::StonecutterRecipe],
    /// A picture of what each loom pattern would make of the banner currently
    /// in the loom, in the order the loom numbers them.
    pub loom_previews: &'a [TextureId],
    /// Effect icons for the beacon, by effect name.
    pub effect_icons: &'a std::collections::HashMap<String, TextureId>,
    /// The item-icon atlas, for the screens that draw items that are not in a
    /// slot (a stonecutter's choices).
    pub icons: &'a Option<(TextureId, Arc<ItemIcons>)>,
    /// The animal whose inventory is open, when one is (`llama` gets a carpet
    /// in its armour slot instead of barding).
    pub mount_kind: Option<&'a str>,
    /// Textures the renderer draws the panel entities into: `[player, mount]`.
    /// `None` = no 3D preview available, and the flat paper-doll stands in.
    pub previews: [Option<TextureId>; 2],
}

/// Where vanilla puts a screen's entity panel: the rect in menu pixels, how
/// many GUI pixels one block covers, and how far above the entity's midpoint
/// the panel is centred.
pub struct PreviewPanel {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub scale: f32,
    pub y_offset: f32,
}

impl PreviewPanel {
    /// The inventory's own recessed panel (vanilla `InventoryScreen`).
    pub const PLAYER: PreviewPanel =
        PreviewPanel { x1: 26.0, y1: 8.0, x2: 75.0, y2: 78.0, scale: 30.0, y_offset: 0.0625 };
    /// The mount screen's panel (vanilla `HorseInventoryScreen`).
    pub const MOUNT: PreviewPanel =
        PreviewPanel { x1: 26.0, y1: 18.0, x2: 78.0, y2: 70.0, scale: 17.0, y_offset: 0.25 };

    pub fn width(&self) -> f32 {
        self.x2 - self.x1
    }

    pub fn height(&self) -> f32 {
        self.y2 - self.y1
    }

    /// The panel a screen kind shows, if it shows one.
    pub fn of_kind(kind: &str) -> Option<(usize, PreviewPanel)> {
        match kind {
            "player" => Some((0, PreviewPanel::PLAYER)),
            "horse" => Some((1, PreviewPanel::MOUNT)),
            _ => None,
        }
    }
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

/// Chest columns of a mount inventory, worked back out of the slot count: two
/// equipment slots and the 36 player slots are always there, the rest is the
/// chest, three rows deep.
pub fn horse_columns(total: usize) -> usize {
    total.saturating_sub(38) / 3
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
        "horse" => {
            // Saddle and body armour always exist (the server just greys the
            // ones this animal cannot use); the chest columns only appear on a
            // donkey, mule or llama that is carrying one.
            slots.push((8.0, 18.0));
            slots.push((8.0, 36.0));
            for j in 0..3 {
                for k in 0..horse_columns(total) {
                    slots.push((80.0 + k as f32 * 18.0, 18.0 + j as f32 * 18.0));
                }
            }
            player_block(8.0, 84.0, 142.0, &mut slots);
            Layout { tex_kind: "horse", w: 176.0, h: 166.0, slots, generic_rows: None }
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
        "beacon" => {
            // One payment slot, low and to the right of the effect panels.
            slots.push((136.0, 110.0));
            player_block(36.0, 137.0, 195.0, &mut slots);
            Layout { tex_kind: "beacon", w: 230.0, h: 219.0, slots, generic_rows: None }
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

/// A bundle's or a shulker box's (or other block-entity container item's)
/// packed contents draw as a small icon grid under the tooltip text,
/// vanilla-style — capped so a full one doesn't produce a screen-filling
/// tooltip. An item is never both, so whichever list is non-empty wins.
const BUNDLE_TOOLTIP_COLS: usize = 6;
const BUNDLE_TOOLTIP_MAX: usize = 18;

fn packed_contents(item: &ItemSnapshot) -> &[ItemSnapshot] {
    if !item.bundle_contents.is_empty() {
        &item.bundle_contents
    } else {
        &item.container_contents
    }
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
    icons: &Option<(TextureId, Arc<ItemIcons>)>,
    registries: &Registries<'_>,
    time: f64,
) {
    let lines = tooltip_lines(lang, item, registries);

    let line_h = 10.0 * s;
    let pad = 4.0 * s;
    let cell = 16.0 * s;
    let text_w = lines
        .iter()
        .map(|l| mc.font.spans_width(l, s))
        .fold(0.0_f32, f32::max);
    let packed = packed_contents(item);
    let shown = packed.len().min(BUNDLE_TOOLTIP_MAX);
    let grid_rows = shown.div_ceil(BUNDLE_TOOLTIP_COLS.max(1));
    let grid_w = cell * BUNDLE_TOOLTIP_COLS.min(packed.len().max(1)) as f32;
    let w = text_w.max(grid_w) + pad * 2.0;
    let grid_gap = if packed.is_empty() { 0.0 } else { 3.0 * s };
    let h = pad * 2.0 + line_h * lines.len() as f32 + grid_gap + cell * grid_rows as f32;
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
    let grid_top = tp.y + pad + line_h * lines.len() as f32 + grid_gap;
    for (i, packed) in packed.iter().take(shown).enumerate() {
        let (col, row) = (i % BUNDLE_TOOLTIP_COLS, i / BUNDLE_TOOLTIP_COLS);
        let rect = Rect::from_min_size(
            pos2(tp.x + pad + col as f32 * cell, grid_top + row as f32 * cell),
            vec2(cell, cell),
        );
        draw_item(painter, mc, icons, rect, packed, s);
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
    // Our own-skin paper-doll (16×32), the stand-in when there is no renderer
    // to draw the real model into the preview panel.
    player_body: Option<TextureId>,
    // Filled in with the mouse offset from each panel's centre, in menu pixels,
    // for the panels this screen actually showed.
    preview_mouse: &mut [Option<[f32; 2]>; 2],
    live: &LiveData<'_>,
    // The recipe book beside the screen, and everything it can show.
    book: &mut crate::app::hud::BookState,
    recipes: &RecipeBook,
    actions: &mut Vec<HudAction>,
) {
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("container")));
    let screen = ctx.content_rect();
    // Vanilla-style darkened world behind the window.
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha(176));

    let layout = layout_for(&view.kind, view.slots.len());
    // A screen with a recipe book slides right to make room for it, exactly as
    // vanilla does — the book is a panel beside the window, not over it.
    let station = Station::of_kind(&view.kind);
    let shift = if book.open && station.is_some() { BOOK_SHIFT * s } else { 0.0 };
    let win = Rect::from_center_size(screen.center(), vec2(layout.w * s, layout.h * s))
        .translate(vec2(shift, 0.0));

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

    // --- the mount screen's extra pieces ---------------------------------------
    // horse.png is only the frame: the chest grid is a sprite stretched over
    // however many columns the animal carries, and the two equipment slots
    // show a ghost of what belongs in them.
    if view.kind == "horse" {
        let columns = horse_columns(view.slots.len());
        if columns > 0
            && let Some(tex) = mc.tex.container_sprites.get("horse/chest_slots")
        {
            // The sprite holds the widest grid there is (five columns, 90 px);
            // a narrower animal gets the left part of it.
            let w = columns as f32 * 18.0;
            painter.image(
                tex.id(),
                Rect::from_min_size(
                    win.min + vec2(79.0 * s, 17.0 * s),
                    vec2(w * s, 54.0 * s),
                ),
                Rect::from_min_max(pos2(0.0, 0.0), pos2((w / 90.0).min(1.0), 1.0)),
                Color32::WHITE,
            );
        }
        let ghost = |name: &str, slot: usize, y: f32| {
            if view.slots.get(slot).is_some_and(|s| s.is_some()) {
                return; // the real item covers it
            }
            if let Some(tex) = mc.tex.container_sprites.get(name) {
                painter.image(
                    tex.id(),
                    Rect::from_min_size(win.min + vec2(8.0 * s, y * s), vec2(16.0 * s, 16.0 * s)),
                    FULL_UV,
                    Color32::WHITE,
                );
            }
        };
        ghost("slot/saddle", 0, 18.0);
        let llama = live.mount_kind.is_some_and(|k| k.ends_with("llama"));
        ghost(if llama { "slot/llama_armor" } else { "slot/horse_armor" }, 1, 36.0);
    }

    // --- the entity in its panel ---------------------------------------------
    // Vanilla renders you (and your mount) live in a recessed panel, turning to
    // follow the mouse. The renderer draws the model into a little texture of
    // its own; here it is only blitted, and the mouse offset is handed back so
    // the next frame can pose it.
    if let Some((slot, panel)) = PreviewPanel::of_kind(&view.kind)
        && let Some(tex) = live.previews[slot]
    {
        let rect = Rect::from_min_size(
            win.min + vec2(panel.x1 * s, panel.y1 * s),
            vec2(panel.width() * s, panel.height() * s),
        );
        painter.image(
            tex,
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        // Vanilla's follow-mouse angles, in menu pixels from the panel centre.
        let mouse = ctx.pointer_latest_pos().unwrap_or(rect.center());
        preview_mouse[slot] =
            Some([(rect.center().x - mouse.x) / s, (rect.center().y - mouse.y) / s]);
    } else if view.kind == "player"
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

    // --- the recipe book ---------------------------------------------------
    if let Some(station) = station {
        // Vanilla's knowledge-book button, where vanilla puts it on each screen.
        let (bx, by) = match view.kind.as_str() {
            "crafting" => (5.0, 17.0),
            "player" => (104.0, 61.0),
            _ => (20.0, 33.0),
        };
        let rect = Rect::from_min_size(win.min + vec2(bx * s, by * s), vec2(20.0 * s, 18.0 * s));
        let hovered = ctx.pointer_latest_pos().is_some_and(|p| rect.contains(p));
        if let Some(tex) =
            mc.tex.book_sprites.get(if hovered { "button_highlighted" } else { "button" })
        {
            painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
        }
        if hovered && ctx.input(|i| i.pointer.primary_clicked()) {
            book.open = !book.open;
        }
        if book.open {
            draw_recipe_book(
                ctx, mc, s, view, book, recipes, station, live, win, lang, &painter,
            );
        }
        // The ghost the book (or the server) put in the grid.
        if let Some((id, recipe)) = book.ghost.as_ref().filter(|(id, _)| *id == view.id) {
            let _ = id;
            draw_ghost(&painter, view, live, &layout, win, s, recipe, ctx.input(|i| i.time));
        }
    }

    // --- title -----------------------------------------------------------------
    // The beacon has no room for one: its window is nearly all panel, and
    // vanilla draws the two power headings there instead.
    let title_x = if view.kind == "merchant" { 110.0 } else { 8.0 };
    if view.kind != "beacon" {
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
    }

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
            tooltip(&painter, mc, s, lang, screen, p, item, icons, &reg, ctx.input(|i| i.time));
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

    /// The two screens that show you a live model, and the sizes vanilla
    /// gives their panels.
    #[test]
    fn only_two_screens_have_an_entity_panel() {
        let (slot, panel) = PreviewPanel::of_kind("player").expect("the inventory has one");
        assert_eq!(slot, 0);
        assert_eq!((panel.width(), panel.height()), (49.0, 70.0));
        assert_eq!(panel.scale, 30.0);
        let (slot, panel) = PreviewPanel::of_kind("horse").expect("the mount screen has one");
        assert_eq!(slot, 1);
        assert_eq!((panel.width(), panel.height()), (52.0, 52.0));
        assert_eq!(panel.scale, 17.0);
        for kind in ["crafting", "furnace", "generic_9x3", "merchant", "beacon"] {
            assert!(PreviewPanel::of_kind(kind).is_none(), "{kind} should have no panel");
        }
    }

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
    fn a_plain_horse_has_only_its_two_equipment_slots() {
        // Saddle, body armour and the 36 player slots.
        let l = layout_for("horse", 38);
        assert_eq!(horse_columns(38), 0);
        assert_eq!(l.slots.len(), 38);
        assert_eq!(l.slots[0], (8.0, 18.0)); // saddle
        assert_eq!(l.slots[1], (8.0, 36.0)); // barding
        assert_eq!(l.slots[2], (8.0, 84.0)); // straight into the backpack
    }

    #[test]
    fn a_loaded_llama_lays_its_chest_out_in_columns() {
        // Five columns of three, on top of the two equipment slots.
        let total = 2 + 15 + 36;
        let l = layout_for("horse", total);
        assert_eq!(horse_columns(total), 5);
        assert_eq!(l.slots.len(), total);
        // Vanilla numbers the chest row by row, starting at x=80.
        assert_eq!(l.slots[2], (80.0, 18.0));
        assert_eq!(l.slots[6], (152.0, 18.0));
        assert_eq!(l.slots[7], (80.0, 36.0));
        // And the player block still starts right after it.
        assert_eq!(l.slots[17], (8.0, 84.0));
    }

    #[test]
    fn a_donkey_carries_three_columns() {
        assert_eq!(horse_columns(2 + 9 + 36), 3);
        // A truncated slot list can never give a negative column count.
        assert_eq!(horse_columns(0), 0);
        assert_eq!(horse_columns(37), 0);
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
        // A menu kind we have no layout for still has to be usable: the
        // container part is laid out as plain nine-wide rows.
        let l = layout_for("some_plugin_menu", 9 + 36);
        assert_eq!(l.slots.len(), 45);
        assert!(l.generic_rows.is_some());
    }

    #[test]
    fn the_beacon_has_its_own_wider_window() {
        let l = layout_for("beacon", 1 + 36);
        assert_eq!(l.slots.len(), 37);
        assert!(l.generic_rows.is_none(), "the beacon is not a chest");
        assert_eq!((l.w, l.h), (230.0, 219.0));
        assert_eq!(l.slots[0], (136.0, 110.0), "the payment slot");
        assert_eq!(l.slots[1], (36.0, 137.0), "the inventory sits low and indented");
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
        // The beacon: pick a power from the tiers your pyramid pays for, then
        // pay the iron/gold/emerald/diamond and confirm.
        "beacon" => {
            draw_beacon(ctx, mc, s, view, live, win, lang, &painter, actions);
        }
        // The loom: every pattern you could weave, drawn as the banner it would
        // actually make, in a grid you scroll through.
        "loom" => {
            draw_loom(ctx, mc, s, view, live, win, &painter, actions);
        }
        // The stonecutter: everything the stone in the input slot can be cut
        // into, one button each.
        "stonecutter" => {
            draw_stonecutter(ctx, mc, s, view, live, win, lang, &painter, actions);
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

/// How far vanilla slides a screen to the right to make room for the open
/// recipe book beside it.
const BOOK_SHIFT: f32 = 77.0;
/// The book panel's own size, in GUI pixels.
const BOOK_W: f32 = 147.0;
const BOOK_H: f32 = 166.0;

/// The recipe book: the panel of everything you have unlocked, next to a
/// crafting or smelting screen. Vanilla's layout — a search box across the top,
/// twenty recipes to a page in a five-by-four grid, page arrows underneath, and
/// the category tabs sticking out of the left edge.
#[allow(clippy::too_many_arguments)]
fn draw_recipe_book(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    view: &ContainerView,
    book: &mut crate::app::hud::BookState,
    recipes: &RecipeBook,
    station: Station,
    live: &LiveData<'_>,
    win: Rect,
    lang: &Lang,
    painter: &egui::Painter,
) {
    const COLS: usize = 5;
    const ROWS: usize = 4;
    const CELL: f32 = 25.0;
    const PER_PAGE: usize = COLS * ROWS;

    let origin = win.min - vec2((BOOK_W + 4.0) * s, 0.0);
    let at = |x: f32, y: f32, w: f32, h: f32| {
        Rect::from_min_size(origin + vec2(x * s, y * s), vec2(w * s, h * s))
    };
    // The panel itself: the top-left 147×166 of the book sheet.
    if let Some(tex) = mc.tex.recipe_book.as_ref() {
        let ts = tex.size_vec2();
        painter.image(
            tex.id(),
            at(0.0, 0.0, BOOK_W, BOOK_H),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(BOOK_W / ts.x, BOOK_H / ts.y)),
            Color32::WHITE,
        );
    }
    let sprite = |name: &str| mc.tex.book_sprites.get(name);
    let pointer = ctx.pointer_latest_pos();
    let clicked = ctx.input(|i| i.pointer.primary_clicked());

    // --- the tabs, down the outside of the left edge -----------------------
    for (i, tab) in BookTab::ALL.iter().enumerate() {
        let rect = at(-25.0, 3.0 + i as f32 * 27.0, 35.0, 27.0);
        let selected = book.tab == i;
        if let Some(tex) = sprite(if selected { "tab_selected" } else { "tab" }) {
            painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
        }
        let icon = ItemSnapshot { item: tab.icon().to_owned(), count: 1, ..Default::default() };
        draw_item(
            painter,
            mc,
            live.icons,
            Rect::from_min_size(rect.min + vec2(6.0 * s, 5.0 * s), vec2(16.0 * s, 16.0 * s)),
            &icon,
            s,
        );
        if pointer.is_some_and(|p| rect.contains(p)) && clicked {
            book.tab = i;
            book.page = 0;
        }
    }

    // --- the search box ----------------------------------------------------
    let search_rect = at(23.0, 12.0, 81.0, 12.0);
    if let Some(tex) = mc.tex.text_field.as_ref() {
        painter.image(tex.id(), search_rect, FULL_UV, Color32::WHITE);
    }
    // Everything typed while the book is open goes into the search box, which
    // is what vanilla does too — the box has focus as soon as the book opens.
    ctx.input(|i| {
        for event in &i.events {
            match event {
                egui::Event::Text(text) => {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        if book.search.chars().count() < 24 {
                            book.search.push(c);
                            book.page = 0;
                        }
                    }
                }
                egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                    book.search.pop();
                    book.page = 0;
                }
                _ => {}
            }
        }
    });
    let caret = if ctx.input(|i| i.time) % 1.0 < 0.5 { "_" } else { "" };
    mc.font.draw(
        painter,
        search_rect.min + vec2(4.0 * s, 3.0 * s),
        &format!("{}{caret}", book.search),
        s,
        Color32::WHITE,
        false,
    );

    // --- the recipes -------------------------------------------------------
    let tab = BookTab::ALL[book.tab.min(BookTab::ALL.len() - 1)];
    let page_of = recipes.page(tab, &book.search, station);
    let pages = page_of.len().div_ceil(PER_PAGE).max(1);
    book.page = book.page.min(pages - 1);
    // What the player is carrying decides which recipes are drawn lit up.
    let mut have: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for item in view.slots.iter().flatten() {
        *have.entry(item.item.clone()).or_insert(0) += item.count.max(1) as u32;
    }
    let mut tooltip: Option<(egui::Pos2, Vec<ChatSpan>)> = None;
    let mut picked: Option<crate::bridge::events::BookRecipe> = None;
    for cell in 0..PER_PAGE {
        let Some(recipe) = page_of.get(book.page * PER_PAGE + cell) else { break };
        let rect = at(
            11.0 + (cell % COLS) as f32 * CELL,
            31.0 + (cell / COLS) as f32 * CELL,
            CELL,
            CELL,
        );
        let can = craftable(recipe, &have);
        let hovered = pointer.is_some_and(|p| rect.contains(p));
        if let Some(tex) = sprite(if can { "slot_craftable" } else { "slot_uncraftable" }) {
            painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
        }
        let item = ItemSnapshot {
            item: recipe.result.clone(),
            count: recipe.result_count.max(1),
            ..Default::default()
        };
        draw_item(
            painter,
            mc,
            live.icons,
            Rect::from_min_size(rect.min + vec2(4.0 * s, 4.0 * s), vec2(16.0 * s, 16.0 * s)),
            &item,
            s,
        );
        if hovered {
            if let Some(p) = pointer {
                let mut lines = vec![ChatSpan::plain(lang.item_name(&recipe.result))];
                if !can {
                    lines.push(ChatSpan {
                        text: "You are missing ingredients".into(),
                        color: Some([0xAA, 0xAA, 0xAA]),
                        ..Default::default()
                    });
                }
                tooltip = Some((p, lines));
            }
            if clicked {
                picked = Some((*recipe).clone());
            }
        }
    }
    if let Some(recipe) = picked {
        book.ghost = Some((view.id, recipe));
    }

    // --- the page arrows and counter ---------------------------------------
    if pages > 1 {
        for (x, forward) in [(15.0, false), (93.0, true)] {
            let rect = at(x, 137.0, 12.0, 17.0);
            let hovered = pointer.is_some_and(|p| rect.contains(p));
            let name = match (forward, hovered) {
                (true, true) => "page_forward_highlighted",
                (true, false) => "page_forward",
                (false, true) => "page_backward_highlighted",
                (false, false) => "page_backward",
            };
            if let Some(tex) = sprite(name) {
                painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
            }
            if hovered && clicked {
                book.page = if forward {
                    (book.page + 1).min(pages - 1)
                } else {
                    book.page.saturating_sub(1)
                };
            }
        }
        let text = format!("{}/{}", book.page + 1, pages);
        // The panel behind it is nearly black, so the counter is drawn light.
        mc.font.draw_anchored(
            painter,
            origin + vec2(73.0 * s, 141.0 * s),
            egui::Align2::CENTER_TOP,
            &text,
            s,
            Color32::from_gray(0xE0),
            false,
        );
    }
    if let Some((at, lines)) = tooltip {
        simple_tooltip(painter, mc, s, at, &lines);
    }
}

/// The ghost of the picked recipe, laid into the crafting grid: the items it
/// wants, drawn faintly wherever the slot is still empty. A slot that accepts
/// several things cycles through them a second at a time, like vanilla's does.
#[allow(clippy::too_many_arguments)]
fn draw_ghost(
    painter: &egui::Painter,
    view: &ContainerView,
    live: &LiveData<'_>,
    layout: &Layout,
    win: Rect,
    s: f32,
    recipe: &crate::bridge::events::BookRecipe,
    time: f64,
) {
    let Some((atlas, icons)) = live.icons.as_ref() else { return };
    // Which layout slot each 3×3 grid cell is: a crafting table's grid starts
    // after the result slot, the inventory's own 2×2 only has room for four.
    let (width, first) = match view.kind.as_str() {
        "crafting" => (3usize, 1usize),
        "player" => (2, 1),
        _ => return,
    };
    // The result, ghosted into the output slot the way vanilla shows it.
    if view.slots.first().is_some_and(|s| s.is_none())
        && let Some(uv) = icons.uv(&recipe.result)
        && let Some(&(x, y)) = layout.slots.first()
    {
        painter.image(
            *atlas,
            Rect::from_min_size(win.min + vec2(x * s, y * s), vec2(16.0 * s, 16.0 * s)),
            Rect::from_min_max(pos2(uv[0], uv[1]), pos2(uv[2], uv[3])),
            Color32::from_white_alpha(120),
        );
    }
    for (cell, options) in grid_slots(recipe) {
        let (col, row) = (cell % 3, cell / 3);
        if col >= width || row >= width || options.is_empty() {
            continue;
        }
        let slot = first + row * width + col;
        // Never draw over something the player has already put there.
        if view.slots.get(slot).is_some_and(|s| s.is_some()) {
            continue;
        }
        let Some(&(x, y)) = layout.slots.get(slot) else { continue };
        let name = &options[(time as usize) % options.len()];
        let Some(uv) = icons.uv(name) else { continue };
        painter.image(
            *atlas,
            Rect::from_min_size(win.min + vec2(x * s, y * s), vec2(16.0 * s, 16.0 * s)),
            Rect::from_min_max(pos2(uv[0], uv[1]), pos2(uv[2], uv[3])),
            Color32::from_white_alpha(120),
        );
    }
}

/// The powers a beacon offers, by how tall its pyramid has to be. Vanilla's
/// list, in vanilla's order: the left column is what the beacon does, the right
/// one is the second power a full pyramid buys.
const BEACON_TIERS: [&[&str]; 4] = [
    &["speed", "haste"],
    &["resistance", "jump_boost"],
    &["strength"],
    &["regeneration"],
];

/// The beacon screen: three rows of powers on the left, the second power on the
/// right, and the confirm/cancel pair under them. A power is greyed out until
/// the pyramid under the beacon is tall enough to pay for it.
#[allow(clippy::too_many_arguments)]
fn draw_beacon(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    view: &mut ContainerView,
    live: &LiveData<'_>,
    win: Rect,
    lang: &Lang,
    painter: &egui::Painter,
    actions: &mut Vec<HudAction>,
) {
    let levels = live.prop(0) as i32;
    let pointer = ctx.pointer_latest_pos();
    let clicked = ctx.input(|i| i.pointer.primary_clicked());
    // Until the player touches anything the screen shows what the beacon is
    // already doing, which is how vanilla opens.
    if !view.beacon_touched {
        view.beacon_primary = effect_name(live.prop(1));
        view.beacon_secondary = effect_name(live.prop(2));
    }
    let mut tooltip: Option<(egui::Pos2, Vec<ChatSpan>)> = None;
    let sprite = |name: &str| mc.tex.container_sprites.get(name);

    // One power button: 22×22, its effect icon in the middle.
    let mut button = |painter: &egui::Painter,
                      x: f32,
                      y: f32,
                      icon: Option<&str>,
                      enabled: bool,
                      selected: bool,
                      tip: Vec<ChatSpan>|
     -> bool {
        let rect = Rect::from_min_size(win.min + vec2(x * s, y * s), vec2(22.0 * s, 22.0 * s));
        let hovered = enabled && pointer.is_some_and(|p| rect.contains(p));
        let name = if !enabled {
            "beacon/button_disabled"
        } else if selected {
            "beacon/button_selected"
        } else if hovered {
            "beacon/button_highlighted"
        } else {
            "beacon/button"
        };
        if let Some(tex) = sprite(name) {
            painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
        }
        if let Some(icon) = icon
            && let Some(tex) = live.effect_icons.get(icon)
        {
            painter.image(
                *tex,
                Rect::from_min_size(rect.min + vec2(2.0 * s, 2.0 * s), vec2(18.0 * s, 18.0 * s)),
                FULL_UV,
                if enabled { Color32::WHITE } else { Color32::from_gray(90) },
            );
        }
        if hovered && let Some(p) = pointer {
            tooltip = Some((p, tip));
        }
        hovered && clicked
    };

    // The three tiers of primary powers, centred over the left panel.
    for (tier, effects) in BEACON_TIERS.iter().enumerate().take(3) {
        let span = effects.len() as f32 * 22.0 + (effects.len() as f32 - 1.0) * 2.0;
        for (i, effect) in effects.iter().enumerate() {
            let x = 76.0 + i as f32 * 24.0 - span / 2.0;
            let y = 22.0 + tier as f32 * 25.0;
            let enabled = levels > tier as i32;
            let selected = view.beacon_primary.as_deref() == Some(*effect);
            if button(painter, x, y, Some(effect), enabled, selected, vec![effect_span(lang, effect, 1)])
            {
                view.beacon_touched = true;
                view.beacon_primary = Some((*effect).to_owned());
                // Vanilla drops the second power when the first one changes.
                view.beacon_secondary = None;
            }
        }
    }
    // The right panel: at a full pyramid you may add regeneration, or spend the
    // second slot doubling the power you already picked.
    let full = levels >= 4;
    let primary = view.beacon_primary.clone();
    let span = 2.0 * 22.0 + 2.0;
    for i in 0..2 {
        let x = 167.0 + i as f32 * 24.0 - span / 2.0;
        let (icon, tip, pick) = if i == 0 {
            let e = BEACON_TIERS[3][0];
            (Some(e), effect_span(lang, e, 1), Some(e.to_owned()))
        } else {
            match &primary {
                Some(e) => (Some(e.as_str()), effect_span(lang, e, 2), Some(e.clone())),
                None => (None, ChatSpan::plain(""), None),
            }
        };
        let enabled = full && primary.is_some();
        let selected = pick.is_some() && view.beacon_secondary == pick;
        if button(painter, x, 22.0, icon, enabled, selected, vec![tip]) {
            view.beacon_touched = true;
            view.beacon_secondary = pick;
        }
    }

    // Confirm only once there is a payment in the slot and a power chosen —
    // vanilla greys the tick out otherwise.
    let paid = view.slots.first().is_some_and(|s| s.is_some());
    let ready = paid && view.beacon_primary.is_some();
    let icon_button = |painter: &egui::Painter, x: f32, sprite_name: &str, enabled: bool| -> bool {
        let rect = Rect::from_min_size(win.min + vec2(x * s, 107.0 * s), vec2(22.0 * s, 22.0 * s));
        let hovered = enabled && pointer.is_some_and(|p| rect.contains(p));
        let bg = if !enabled {
            "beacon/button_disabled"
        } else if hovered {
            "beacon/button_highlighted"
        } else {
            "beacon/button"
        };
        if let Some(tex) = sprite(bg) {
            painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
        }
        if let Some(tex) = sprite(sprite_name) {
            painter.image(
                tex.id(),
                Rect::from_min_size(rect.min + vec2(2.0 * s, 2.0 * s), vec2(18.0 * s, 18.0 * s)),
                FULL_UV,
                if enabled { Color32::WHITE } else { Color32::from_gray(90) },
            );
        }
        hovered && clicked
    };
    if icon_button(painter, 164.0, "beacon/confirm", ready) {
        actions.push(HudAction::SetBeacon {
            primary: view.beacon_primary.clone(),
            secondary: view.beacon_secondary.clone(),
        });
        actions.push(HudAction::CloseContainer { id: view.id });
    }
    if icon_button(painter, 190.0, "beacon/cancel", true) {
        actions.push(HudAction::CloseContainer { id: view.id });
    }

    // Vanilla labels the two panels.
    let title = |painter: &egui::Painter, x: f32, key: &str, fallback: &str| {
        let text = lang.get(key).unwrap_or(fallback);
        mc.font.draw_anchored(
            painter,
            win.min + vec2(x * s, 12.0 * s),
            egui::Align2::CENTER_TOP,
            text,
            s,
            Color32::from_rgb(0xE0, 0xE0, 0xE0),
            true,
        );
    };
    title(painter, 62.0, "block.minecraft.beacon.primary", "Primary Power");
    title(painter, 169.0, "block.minecraft.beacon.secondary", "Secondary Power");
    if let Some((at, lines)) = tooltip {
        simple_tooltip(painter, mc, s, at, &lines);
    }
}

/// A beacon data slot's effect: the server sends the mob-effect registry id, or
/// -1 (which arrives as an all-ones short) for "no power".
fn effect_name(id: u32) -> Option<String> {
    if id == 0xFFFF {
        return None;
    }
    BEACON_TIERS
        .iter()
        .flat_map(|t| t.iter())
        .find(|name| effect_id(name) == Some(id))
        .map(|name| (*name).to_string())
}

/// The mob-effect registry ids of the powers a beacon can give. These are the
/// ids the server sends in the beacon's data slots and the ones we send back.
fn effect_id(name: &str) -> Option<u32> {
    use azalea::registry::Registry as _;
    use std::str::FromStr as _;
    azalea::registry::builtin::MobEffect::from_str(name).ok().map(|e| e.to_u32())
}

/// "Speed II" — an effect's name at a level, the way a beacon button labels it.
fn effect_span(lang: &Lang, effect: &str, level: u32) -> ChatSpan {
    let name = lang
        .get(&format!("effect.minecraft.{effect}"))
        .map(str::to_owned)
        .unwrap_or_else(|| crate::assets::prettify(effect));
    ChatSpan::plain(if level > 1 { format!("{name} {}", roman(level)) } else { name })
}

/// The loom: a scrolling grid of every pattern you could weave onto the banner
/// in the left slot, each drawn as the banner it would produce.
#[allow(clippy::too_many_arguments)]
fn draw_loom(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    view: &mut ContainerView,
    live: &LiveData<'_>,
    win: Rect,
    painter: &egui::Painter,
    actions: &mut Vec<HudAction>,
) {
    const COLS: usize = 4;
    const ROWS: usize = 4;
    const CELL: f32 = 14.0;
    let sprite = |name: &str| mc.tex.container_sprites.get(name);
    // A loom only offers patterns once it has a banner and a dye to work with.
    let has_banner = view.slots.first().is_some_and(|s| s.is_some());
    let has_dye = view.slots.get(1).is_some_and(|s| s.is_some());
    let count = if has_banner && has_dye { live.loom_previews.len() } else { 0 };
    if count == 0 {
        return;
    }
    let rows = count.div_ceil(COLS);
    let hidden = rows.saturating_sub(ROWS);
    let selected = live.prop(0) as usize;

    let pointer = ctx.pointer_latest_pos();
    let clicked = ctx.input(|i| i.pointer.primary_clicked());
    let area = Rect::from_min_size(
        win.min + vec2(60.0 * s, 13.0 * s),
        vec2(COLS as f32 * CELL * s, ROWS as f32 * CELL * s),
    );
    // The wheel scrolls the grid, a row at a time, like vanilla.
    if hidden > 0 && pointer.is_some_and(|p| area.contains(p)) {
        let dy = ctx.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            let step = 1.0 / hidden as f32;
            view.scroll = (view.scroll - dy.signum() * step).clamp(0.0, 1.0);
        }
    }
    let first_row = (view.scroll * hidden as f32).round() as usize;

    for cell in 0..COLS * ROWS {
        let index = (first_row + cell / COLS) * COLS + cell % COLS;
        let Some(preview) = live.loom_previews.get(index) else { break };
        let x = 60.0 + (cell % COLS) as f32 * CELL;
        let y = 13.0 + (cell / COLS) as f32 * CELL;
        let rect = Rect::from_min_size(win.min + vec2(x * s, y * s), vec2(CELL * s, CELL * s));
        let hovered = pointer.is_some_and(|p| rect.contains(p));
        let name = if index + 1 == selected {
            "loom/pattern_selected"
        } else if hovered {
            "loom/pattern_highlighted"
        } else {
            "loom/pattern"
        };
        if let Some(tex) = sprite(name) {
            painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
        }
        // The banner picture, inset and in the cloth's own 1:2 proportions.
        painter.image(
            *preview,
            Rect::from_min_size(rect.min + vec2(4.0 * s, 2.0 * s), vec2(5.0 * s, 10.0 * s)),
            FULL_UV,
            Color32::WHITE,
        );
        if hovered && clicked {
            // Vanilla numbers the loom's buttons from one.
            actions.push(HudAction::ContainerButton {
                window_id: view.id,
                button: (index + 1).min(255) as u8,
            });
        }
    }
    // The scroll bar down the right-hand side of the grid.
    let bar = if hidden > 0 { "loom/scroller" } else { "loom/scroller_disabled" };
    if let Some(tex) = sprite(bar) {
        let y = 13.0 + 41.0 * view.scroll;
        painter.image(
            tex.id(),
            Rect::from_min_size(win.min + vec2(119.0 * s, y * s), vec2(12.0 * s, 15.0 * s)),
            FULL_UV,
            Color32::WHITE,
        );
    }
}

/// The stonecutter: every recipe the item in the input slot can be cut into,
/// filtered out of the list the server sent on join.
#[allow(clippy::too_many_arguments)]
fn draw_stonecutter(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    view: &mut ContainerView,
    live: &LiveData<'_>,
    win: Rect,
    lang: &Lang,
    painter: &egui::Painter,
    actions: &mut Vec<HudAction>,
) {
    const COLS: usize = 4;
    const ROWS: usize = 3;
    const CELL_W: f32 = 16.0;
    const CELL_H: f32 = 18.0;
    let sprite = |name: &str| mc.tex.container_sprites.get(name);
    let Some(input) = view.slots.first().and_then(|s| s.as_ref()) else { return };
    // Vanilla's stonecutter menu numbers the recipes that accept this input,
    // in the order the server listed them.
    let choices: Vec<&crate::bridge::events::StonecutterRecipe> =
        live.stonecutter.iter().filter(|r| r.inputs.iter().any(|i| *i == input.item)).collect();
    if choices.is_empty() {
        return;
    }
    let rows = choices.len().div_ceil(COLS);
    let hidden = rows.saturating_sub(ROWS);
    let selected = live.prop(0) as usize;
    let pointer = ctx.pointer_latest_pos();
    let clicked = ctx.input(|i| i.pointer.primary_clicked());
    let area = Rect::from_min_size(
        win.min + vec2(52.0 * s, 14.0 * s),
        vec2(COLS as f32 * CELL_W * s, ROWS as f32 * CELL_H * s),
    );
    if hidden > 0 && pointer.is_some_and(|p| area.contains(p)) {
        let dy = ctx.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            let step = 1.0 / hidden as f32;
            view.scroll = (view.scroll - dy.signum() * step).clamp(0.0, 1.0);
        }
    }
    let first_row = (view.scroll * hidden as f32).round() as usize;
    let mut tooltip: Option<(egui::Pos2, Vec<ChatSpan>)> = None;

    for cell in 0..COLS * ROWS {
        let index = (first_row + cell / COLS) * COLS + cell % COLS;
        let Some(recipe) = choices.get(index) else { break };
        let x = 52.0 + (cell % COLS) as f32 * CELL_W;
        let y = 14.0 + (cell / COLS) as f32 * CELL_H;
        let rect = Rect::from_min_size(win.min + vec2(x * s, y * s), vec2(CELL_W * s, CELL_H * s));
        let hovered = pointer.is_some_and(|p| rect.contains(p));
        let name = if index == selected {
            "stonecutter/recipe_selected"
        } else if hovered {
            "stonecutter/recipe_highlighted"
        } else {
            "stonecutter/recipe"
        };
        if let Some(tex) = sprite(name) {
            painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
        }
        let item = ItemSnapshot { item: recipe.result.clone(), count: 1, ..Default::default() };
        draw_item(
            painter,
            mc,
            live.icons,
            Rect::from_min_size(rect.min + vec2(0.0, s), vec2(16.0 * s, 16.0 * s)),
            &item,
            s,
        );
        if hovered {
            if let Some(p) = pointer {
                tooltip = Some((p, vec![ChatSpan::plain(lang.item_name(&recipe.result))]));
            }
            if clicked {
                actions.push(HudAction::ContainerButton {
                    window_id: view.id,
                    button: index.min(255) as u8,
                });
            }
        }
    }
    let bar = if hidden > 0 { "stonecutter/scroller" } else { "stonecutter/scroller_disabled" };
    if let Some(tex) = sprite(bar) {
        let y = 15.0 + 41.0 * view.scroll;
        painter.image(
            tex.id(),
            Rect::from_min_size(win.min + vec2(119.0 * s, y * s), vec2(12.0 * s, 15.0 * s)),
            FULL_UV,
            Color32::WHITE,
        );
    }
    if let Some((at, lines)) = tooltip {
        simple_tooltip(painter, mc, s, at, &lines);
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

    // A firework rocket's flight duration (short/medium/long gunpowder count).
    if let Some(fd) = item.flight_duration {
        let word = match fd {
            1 => "Short",
            2 => "Medium",
            3 => "Long",
            _ => "",
        };
        let text = if word.is_empty() {
            format!("Flight Duration: {fd}")
        } else {
            lang.get("item.minecraft.firework_rocket.flight")
                .map(|t| format!("{t} {word}"))
                .unwrap_or_else(|| format!("Flight Duration: {word}"))
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

    // A signed book's generation — copies get a little more worn each time.
    if let Some(book) = &item.book {
        let (key, fallback) = match book.generation {
            1 => ("book.generation.copy", "Copy of original"),
            2 => ("book.generation.copy_of_copy", "Copy of a copy"),
            3 => ("book.generation.tattered", "Tattered"),
            _ => ("", ""),
        };
        if !key.is_empty() {
            lines.push(vec![span(lang.get(key).unwrap_or(fallback), GREY)]);
        }
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

// ---------------------------------------------------------------------------
// The creative menu
// ---------------------------------------------------------------------------

/// Draw the creative "Search Items" tab: every item in the game in a scrolling
/// 9×5 grid, a search box, the player's hotbar along the bottom and the slot
/// that throws things away. Clicking is entirely local — the only thing the
/// server ever hears is "put this in that slot".
#[allow(clippy::too_many_arguments)]
pub fn draw_creative(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    menu: &mut crate::app::creative::Creative,
    carried: &mut Option<ItemSnapshot>,
    hotbar: &[Option<ItemSnapshot>],
    icons: &Option<(TextureId, Arc<ItemIcons>)>,
    lang: &Lang,
    registries: &Registries<'_>,
    actions: &mut Vec<HudAction>,
) {
    use crate::app::creative as cr;

    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("creative")));
    let screen = ctx.content_rect();
    if menu.tab == cr::Tab::Search {
        // The inventory tab draws its own darkened world behind the screen.
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha(176));
    }
    // The Inventory tab is the ordinary inventory screen, which is a different
    // size; the tabs have to sit against whichever window is showing.
    let on_search = menu.tab == cr::Tab::Search;
    let (win_w, win_h) = if on_search { (cr::W, cr::H) } else { (176.0, 166.0) };
    let win = Rect::from_center_size(screen.center(), vec2(win_w * s, win_h * s));
    let at = |x: f32, y: f32| win.min + vec2(x * s, y * s);
    let cell = |x: f32, y: f32| Rect::from_min_size(at(x, y), vec2(16.0 * s, 16.0 * s));
    let pointer = ctx.pointer_latest_pos();
    let clicked = ctx.input(|i| i.pointer.primary_clicked());
    let right_clicked = ctx.input(|i| i.pointer.secondary_clicked());
    let sprite = |name: &str| mc.tex.container_sprites.get(name);

    // --- the two tabs, above and below the window ---------------------------
    // Vanilla puts "Search Items" last in the top row and "Inventory" last in
    // the bottom row, and lets the selected one stick out a little.
    let tab_x = (1.0 + cr::TAB_COL * 27.0).min(win_w - cr::TAB_W - 6.0);
    let search_tab =
        Rect::from_min_size(at(tab_x, -cr::TAB_H + 4.0), vec2(cr::TAB_W * s, cr::TAB_H * s));
    let inv_tab =
        Rect::from_min_size(at(tab_x, win_h - 4.0), vec2(cr::TAB_W * s, cr::TAB_H * s));
    for (rect, name, icon) in [
        (
            search_tab,
            if on_search { "tab_top_selected_7" } else { "tab_top_unselected_7" },
            "compass",
        ),
        (
            inv_tab,
            if on_search { "tab_bottom_unselected_7" } else { "tab_bottom_selected_7" },
            "chest",
        ),
    ] {
        if let Some(tex) = sprite(&format!("creative_inventory/{name}")) {
            painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
        }
        if let Some((atlas, icons)) = icons
            && let Some(uv) = icons.uv(icon)
        {
            let icon_rect = Rect::from_min_size(
                rect.min + vec2(5.0 * s, if name.starts_with("tab_top") { 9.0 } else { 7.0 } * s),
                vec2(16.0 * s, 16.0 * s),
            );
            painter.image(
                *atlas,
                icon_rect,
                Rect::from_min_max(pos2(uv[0], uv[1]), pos2(uv[2], uv[3])),
                Color32::WHITE,
            );
        }
    }
    if clicked && let Some(p) = pointer {
        if search_tab.contains(p) {
            menu.tab = cr::Tab::Search;
        } else if inv_tab.contains(p) {
            menu.tab = cr::Tab::Inventory;
        }
    }
    if !on_search {
        // The Inventory tab is the ordinary inventory screen, drawn by the
        // normal container path — nothing more to do here.
        return;
    }

    // --- the window ---------------------------------------------------------
    if let Some(tex) = mc.tex.containers.get("creative_search") {
        let ts = tex.size_vec2();
        painter.image(
            tex.id(),
            win,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(cr::W / ts.x, cr::H / ts.y)),
            Color32::WHITE,
        );
    }
    mc.font.draw(
        &painter,
        at(8.0, 6.0),
        lang.get("itemGroup.search").unwrap_or("Search Items"),
        s,
        Color32::from_rgb(0x40, 0x40, 0x40),
        false,
    );

    // --- the search box -----------------------------------------------------
    let mut changed = false;
    ctx.input(|i| {
        for event in &i.events {
            match event {
                egui::Event::Text(text) => {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        if menu.search.chars().count() < 50 {
                            menu.search.push(c);
                            changed = true;
                        }
                    }
                }
                egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                    menu.search.pop();
                    changed = true;
                }
                _ => {}
            }
        }
    });
    if changed {
        menu.refilter();
    }
    let caret = if ctx.input(|i| i.time) % 1.0 < 0.5 { "_" } else { "" };
    mc.font.draw(
        &painter,
        at(cr::SEARCH_BOX.0 + 1.0, cr::SEARCH_BOX.1 + 1.0),
        &format!("{}{caret}", menu.search),
        s,
        Color32::from_gray(0xE0),
        false,
    );

    // --- the scrollbar ------------------------------------------------------
    let knob = Rect::from_min_size(
        at(cr::SCROLL_X, cr::SCROLL_Y + (cr::SCROLL_TRACK - 15.0) * menu.scroll),
        vec2(12.0 * s, 15.0 * s),
    );
    let knob_name =
        if menu.can_scroll() { "creative_inventory/scroller" } else { "creative_inventory/scroller_disabled" };
    if let Some(tex) = sprite(knob_name) {
        painter.image(tex.id(), knob, FULL_UV, Color32::WHITE);
    }
    // The wheel scrolls the grid, and dragging the knob does too.
    let wheel = ctx.input(|i| i.smooth_scroll_delta.y);
    if wheel != 0.0 {
        menu.scroll_by(wheel.signum());
    }
    let track = Rect::from_min_size(
        at(cr::SCROLL_X, cr::SCROLL_Y),
        vec2(12.0 * s, cr::SCROLL_TRACK * s),
    );
    if menu.can_scroll()
        && ctx.input(|i| i.pointer.primary_down())
        && let Some(p) = pointer
        && track.expand2(vec2(0.0, 8.0 * s)).contains(p)
    {
        menu.scroll =
            ((p.y - track.top()) / (track.height() - 15.0 * s).max(1.0)).clamp(0.0, 1.0);
    }

    // --- the items ----------------------------------------------------------
    let mut hovered: Option<(Rect, ItemSnapshot)> = None;
    let page = menu.page().into_iter().map(|(i, n)| (i, n.to_string())).collect::<Vec<_>>();
    for (i, name) in &page {
        let (col, row) = (i % cr::COLS, i / cr::COLS);
        let r = cell(cr::GRID_X + col as f32 * 18.0, cr::GRID_Y + row as f32 * 18.0);
        let item = ItemSnapshot::plain(name, cr::stack_size(name));
        draw_item(&painter, mc, icons, r, &item, s);
        if pointer.is_some_and(|p| r.contains(p)) {
            painter.rect_filled(r, 0.0, Color32::from_white_alpha(100));
            hovered = Some((r, item.clone()));
            if clicked {
                // Vanilla hands you a whole stack; a right-click takes one.
                let mut taken = item.clone();
                if right_clicked {
                    taken.count = 1;
                }
                *carried = Some(taken);
            }
        }
    }

    // --- the player's hotbar, and the slot that eats things -----------------
    for i in 0..9usize {
        let r = cell(cr::GRID_X + i as f32 * 18.0, cr::HOTBAR_Y);
        if let Some(Some(item)) = hotbar.get(i) {
            draw_item(&painter, mc, icons, r, item, s);
            if pointer.is_some_and(|p| r.contains(p)) && hovered.is_none() {
                hovered = Some((r, item.clone()));
            }
        }
        if pointer.is_some_and(|p| r.contains(p)) {
            painter.rect_filled(r, 0.0, Color32::from_white_alpha(100));
            if clicked {
                // Slot 36 is the first hotbar slot of the player's own menu.
                match carried.take() {
                    Some(item) => actions.push(HudAction::CreativeSet {
                        slot: 36 + i as u16,
                        item: item.item.clone(),
                        count: item.count.max(1),
                    }),
                    None => {
                        // Empty hand on a full slot picks the stack up.
                        if let Some(Some(item)) = hotbar.get(i) {
                            *carried = Some(item.clone());
                            actions.push(HudAction::CreativeSet {
                                slot: 36 + i as u16,
                                item: String::new(),
                                count: 0,
                            });
                        }
                    }
                }
            }
        }
    }
    // --- the carried stack, and the tooltip ---------------------------------
    if let Some(p) = pointer {
        if let Some(item) = carried {
            draw_item(
                &painter,
                mc,
                icons,
                Rect::from_center_size(p, vec2(16.0 * s, 16.0 * s)),
                item,
                s,
            );
            // Clicking anywhere off the window throws it into the world, which
            // is vanilla's slot -1.
            if clicked && !win.contains(p) && !search_tab.contains(p) && !inv_tab.contains(p) {
                actions.push(HudAction::CreativeSet {
                    slot: u16::MAX,
                    item: item.item.clone(),
                    count: item.count.max(1),
                });
                *carried = None;
            }
        } else if let Some((_, item)) = &hovered {
            tooltip(&painter, mc, s, lang, screen, p, item, icons, registries, ctx.input(|i| i.time));
        }
    }
}
