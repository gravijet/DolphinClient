//! Container screens (inventory, chests, furnaces, villagers, …) drawn with
//! the real vanilla GUI textures and slot layouts. Clicks are translated to
//! `HudAction::SlotClick` which the app forwards to the server.

use egui::{Align2, Area, Color32, Id, LayerId, Order, Rect, TextureId, pos2, vec2};
use std::sync::Arc;

use crate::app::hud::HudAction;
use crate::app::mcui::{self, LINE_H, McUi, tile_background};
use crate::assets::Lang;
use crate::assets::items::ItemIcons;
use crate::app::recipebook::{BookTab, RecipeBook, Station, craftable, grid_slots};
use crate::bridge::events::{ChatSpan, InstrumentDesc, ItemSnapshot, SlotClickKind, TradeOffer};

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
    /// The bundle slot currently being hovered, and which of its packed items
    /// the mouse wheel has selected — `BundleMouseActions`/`ScrollWheelHandler`
    /// live only on the client, so this never comes from the server. Cleared
    /// (and told to the server as -1) the moment the mouse leaves that slot.
    pub bundle_selected: Option<(u16, i32)>,
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
            bundle_selected: None,
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
    /// The instrument registry, so a goat horn's tooltip can name it.
    pub instruments: &'a [String],
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

/// Whether a crafter's slot (0..9) is disabled — decompiled `CrafterMenu`:
/// `containerData` indices 0-8 are one per slot, 1 = disabled.
fn crafter_slot_disabled(props: &std::collections::HashMap<u16, u16>, slot: u16) -> bool {
    slot < 9 && props.get(&slot).copied().unwrap_or(0) == 1
}

/// Whether the crafter is currently redstone-powered — decompiled
/// `CrafterMenu.isPowered`: `containerData` index 9, 1 = powered.
fn crafter_powered(props: &std::collections::HashMap<u16, u16>) -> bool {
    props.get(&9).copied().unwrap_or(0) == 1
}

/// The lectern's current page, straight off `ClientboundContainerSetData`
/// property 0 (decompiled `LecternMenu`: `DATA_COUNT = 1`, index 0 is the
/// only property) — clamped so a book that shrank (or an as-yet-unsynced 0
/// default before the server's first update arrives) never indexes past the
/// real page count.
fn lectern_page(props: &std::collections::HashMap<u16, u16>, num_pages: usize) -> usize {
    (props.get(&0).copied().unwrap_or(0) as usize).min(num_pages.saturating_sub(1))
}

/// The lectern's `BookViewScreen`: the book texture with page text and
/// Prev/Next/Take-Book buttons, no slot grid at all. Decompiled
/// `LecternScreen`: page-turn and take-book both go through
/// `ServerboundContainerButtonClick` (button ids 1/2/3 — same packet this
/// codebase's `HudAction::ContainerButton` already sends for other screens),
/// and the current page is server-authoritative (`ClientboundContainerSetData`
/// property 0), read here straight off `live.props` like every other
/// container-data-driven screen already does — no new event plumbing needed.
#[allow(clippy::too_many_arguments)]
fn draw_lectern(
    ctx: &egui::Context,
    mc: &McUi,
    s: f32,
    view: &ContainerView,
    lang: &Lang,
    live: &LiveData<'_>,
    painter: &egui::Painter,
    screen: Rect,
    actions: &mut Vec<HudAction>,
) {
    let page_rect = Rect::from_center_size(screen.center(), vec2(192.0 * s, 192.0 * s));
    if let Some(tex) = &mc.tex.book {
        painter.image(
            tex.id(),
            page_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(192.0 / 256.0, 192.0 / 256.0)),
            Color32::WHITE,
        );
    } else {
        painter.rect_filled(page_rect, 0.0, Color32::from_rgb(0xDD, 0xCE, 0xA8));
    }
    let ink = Color32::from_rgb(0x30, 0x30, 0x30);
    let text_x = page_rect.left() + 36.0 * s;
    let text_w = 114.0 * s;

    // Real vanilla never shows a lectern with no book while the screen is
    // open (opening it requires a book already in the slot); an empty single
    // page is the same defensive fallback `open_book`'s local reader uses.
    let content = view.slots.first().and_then(|s| s.as_ref()).and_then(|item| item.book.clone());
    let pages = content.map(|c| c.pages).filter(|p| !p.is_empty()).unwrap_or_else(|| vec![Vec::new()]);
    let page = lectern_page(live.props, pages.len());

    let index = lang
        .get("book.pageIndicator")
        .unwrap_or("Page %1$s of %2$s")
        .replace("%1$s", &(page + 1).to_string())
        .replace("%2$s", &pages.len().to_string());
    mc.font.draw_anchored(
        painter,
        pos2(text_x + text_w, page_rect.top() + 16.0 * s),
        Align2::RIGHT_TOP,
        &index,
        s,
        ink,
        false,
    );
    let mut y = page_rect.top() + 32.0 * s;
    if let Some(page_spans) = pages.get(page) {
        for wrapped in crate::app::chat::wrap_spans(mc, page_spans, s, text_w) {
            mc.font.draw_spans(painter, pos2(text_x, y), &wrapped, s, ink, 1.0, false, 0.0);
            y += LINE_H * s;
        }
    }

    let (mut prev, mut next, mut done, mut take) = (false, false, false, false);
    Area::new(Id::new("lectern-buttons"))
        .order(Order::Tooltip)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 106.0 * s))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if mcui::button(ui, mc, 26.0, s, "<", page > 0) {
                    prev = true;
                }
                if mcui::button(ui, mc, 98.0, s, lang.get("gui.done").unwrap_or("Done"), true) {
                    done = true;
                }
                if mcui::button(ui, mc, 26.0, s, ">", page + 1 < pages.len()) {
                    next = true;
                }
            });
            ui.horizontal(|ui| {
                if mcui::button(
                    ui,
                    mc,
                    98.0,
                    s,
                    lang.get("lectern.take_book").unwrap_or("Take Book"),
                    true,
                ) {
                    take = true;
                }
            });
        });
    if prev {
        actions.push(HudAction::ContainerButton { window_id: view.id, button: 1 });
    }
    if next {
        actions.push(HudAction::ContainerButton { window_id: view.id, button: 2 });
    }
    if take {
        actions.push(HudAction::ContainerButton { window_id: view.id, button: 3 });
    }
    // Escape is handled generically for any open container at the app level
    // (`container_open()` -> `close_container()`); only the Done button needs
    // handling here.
    if done {
        actions.push(HudAction::CloseContainer { id: view.id });
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

/// How many of a bundle's items are individually shown (and so selectable)
/// before the rest collapse into a single "+N" overflow cell — vanilla
/// `BundleContents.getNumberOfItemsToShow()`, ported 1:1: a full last row
/// stays fully shown, a partial one gives up its remainder to the overflow
/// cell.
fn bundle_items_to_show(count: usize) -> usize {
    let available: usize = if count > 12 { 11 } else { 12 };
    let on_last_row = count % 4;
    let empty_on_last_row = if on_last_row == 0 { 0 } else { 4 - on_last_row };
    count.min(available.saturating_sub(empty_on_last_row))
}

/// Vanilla `ScrollWheelHandler.getNextScrollWheelSelection`: one notch moves
/// the highlighted item by one, wrapping in both directions through
/// `0..limit` (an out-of-range `current`, including the unselected `-1`,
/// is treated as if it were `-1` first).
fn next_bundle_selection(step: i32, current: i32, limit: i32) -> i32 {
    let mut cur = (current - step).max(-1);
    while cur < 0 {
        cur += limit;
    }
    while cur >= limit {
        cur -= limit;
    }
    cur
}

/// A real bundle item (any of the 17 dyed variants, or the plain `bundle`) —
/// the only items whose tooltip carries `ClientBundleTooltip`'s fullness bar.
/// A shulker box's `container_contents` never gets one, even though it shares
/// `packed_contents()`'s icon grid.
fn is_bundle_item(item: &str) -> bool {
    item == "bundle" || item.ends_with("_bundle")
}

/// Vanilla `BundleContents.getWeight`: how much of a bundle's capacity one
/// packed stack occupies. A nested bundle (data-model-legal even though
/// survival crafting can't currently produce one) costs its own fullness
/// plus 1/16 for the bundle-in-bundle overhead; a plain item costs
/// `1 / max_stack_size`. Suspicious-stew-style edge cases (a beehive/bee nest
/// stack's bees making it always cost a full slot) aren't tracked by
/// `ItemSnapshot` and are deliberately left as the plain-item formula rather
/// than guessed at.
fn bundle_item_weight(item: &ItemSnapshot) -> f64 {
    if !item.bundle_contents.is_empty() {
        return bundle_weight(&item.bundle_contents) + 1.0 / 16.0;
    }
    use std::str::FromStr as _;
    let max_stack = azalea::registry::builtin::ItemKind::from_str(&item.item)
        .map(|k| azalea_inventory::item::MaxStackSizeExt::max_stack_size(&k))
        .unwrap_or(64)
        .max(1);
    1.0 / max_stack as f64
}

/// Vanilla `BundleContents.computeContentWeight`: the sum of every packed
/// stack's weight, each counted `count` times.
fn bundle_weight(packed: &[ItemSnapshot]) -> f64 {
    packed
        .iter()
        .map(|it| bundle_item_weight(it) * it.count as f64)
        .sum()
}

/// Vanilla `ChargedProjectiles.addToTooltip`: consecutive identical loaded
/// projectiles collapse into one `(name, count)` group (a run-length
/// encoding, not a full histogram — two separated runs of the same item
/// stay two groups, exactly like vanilla's single linear pass). A projectile
/// with a server custom name shows that instead of the registry name, same
/// priority as the top-level tooltip name.
fn charged_projectile_groups(projectiles: &[ItemSnapshot], lang: &Lang) -> Vec<(String, u32)> {
    let mut groups: Vec<(String, u32)> = Vec::new();
    for p in projectiles {
        let name = p
            .name
            .as_ref()
            .and_then(|spans| spans.iter().find(|sp| !sp.text.is_empty()))
            .map(|sp| sp.text.clone())
            .unwrap_or_else(|| lang.item_name(&p.item));
        match groups.last_mut() {
            Some((last_name, count)) if *last_name == name => *count += 1,
            _ => groups.push((name, 1)),
        }
    }
    groups
}

/// Vanilla `Mth.mulAndTruncate(weight, 94)`: the fill sprite's pixel width
/// out of the bar's 94-pixel interior, truncated (not rounded) and clamped —
/// a weight over 1.0 can't happen (packing stops the bundle at full), but the
/// clamp mirrors vanilla's defensive bound anyway.
fn bundle_progressbar_fill_px(weight: f64) -> i32 {
    (weight * 94.0).floor().clamp(0.0, 94.0) as i32
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
    // The packed item a bundle's mouse-wheel selection currently highlights
    // (an index into `item.bundle_contents`), if this tooltip's item is the
    // hovered bundle. Never set for shulker-box-style `container_contents`.
    bundle_selected: Option<i32>,
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
    let is_bundle = is_bundle_item(&item.item);
    let bar_h = if is_bundle { 8.0 * s } else { 0.0 };
    let bar_gap = if is_bundle { 3.0 * s } else { 0.0 };
    let w = text_w.max(grid_w).max(if is_bundle { cell * 6.0 } else { 0.0 }) + pad * 2.0;
    let grid_gap = if packed.is_empty() { 0.0 } else { 3.0 * s };
    let h = pad * 2.0
        + line_h * lines.len() as f32
        + grid_gap
        + cell * grid_rows as f32
        + bar_gap
        + bar_h;
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
        if bundle_selected == Some(i as i32) {
            if let Some(tex) = mc.tex.container_sprites.get("bundle/slot_highlight_back") {
                painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
            }
        }
        draw_item(painter, mc, icons, rect, packed, s);
        if bundle_selected == Some(i as i32) {
            if let Some(tex) = mc.tex.container_sprites.get("bundle/slot_highlight_front") {
                painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
            }
        }
    }
    if is_bundle {
        let weight = bundle_weight(&item.bundle_contents);
        let bar_top = grid_top + cell * grid_rows as f32 + bar_gap;
        let bar_w = w - pad * 2.0;
        let bar_left = tp.x + pad;
        // Vanilla's bar is 96px wide with a 1px border on each side around a
        // 94px fill — scale that same 1:94:1 split to this bar's own width.
        let border_px = bar_w / 96.0;
        let fill_max_w = bar_w - border_px * 2.0;
        let fill_frac = bundle_progressbar_fill_px(weight) as f32 / 94.0;
        let fill_sprite = if weight >= 1.0 { "bundle/bundle_progressbar_full" } else { "bundle/bundle_progressbar_fill" };
        if let Some(tex) = mc.tex.container_sprites.get(fill_sprite) {
            let fill_rect = Rect::from_min_size(
                pos2(bar_left + border_px, bar_top),
                vec2(fill_max_w * fill_frac, bar_h),
            );
            painter.image(tex.id(), fill_rect, FULL_UV, Color32::WHITE);
        }
        if let Some(tex) = mc.tex.container_sprites.get("bundle/bundle_progressbar_border") {
            let border_rect = Rect::from_min_size(pos2(bar_left, bar_top), vec2(bar_w, bar_h));
            painter.image(tex.id(), border_rect, FULL_UV, Color32::WHITE);
        }
        let label = if weight <= 0.0 {
            lang.get("item.minecraft.bundle.empty")
        } else if weight >= 1.0 {
            lang.get("item.minecraft.bundle.full")
        } else {
            None
        };
        if let Some(label) = label {
            let tw = mc.font.width(label, s);
            mc.font.draw(
                painter,
                pos2(bar_left + (bar_w - tw) / 2.0, bar_top + 1.0 * s),
                label,
                s,
                Color32::WHITE,
                true,
            );
        }
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

    // The lectern is a `BookViewScreen`, not a slot-grid screen at all — real
    // vanilla's `LecternMenu` has a single (invisible) internal slot and
    // drives everything through page-turn/take-book button clicks, decompiled
    // `LecternMenu`/`LecternScreen`. Handle it entirely separately rather than
    // forcing it through `layout_for`'s slot-grid machinery.
    if view.kind == "lectern" {
        draw_lectern(ctx, mc, s, view, lang, live, &painter, screen, actions);
        return;
    }

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
        let book_kind = crate::bridge::events::RecipeBookKind::of_container_kind(&view.kind);
        if hovered && ctx.input(|i| i.pointer.primary_clicked()) {
            book.open = !book.open;
            // Decompiled `RecipeBookComponent.setVisible`: toggling the
            // panel updates *and immediately sends* that station's settings.
            if let Some(kind) = book_kind {
                book.settings.set(kind, crate::app::hud::RecipeBookStationSettings {
                    open: book.open,
                    filtering: book.filtering,
                });
                actions.push(HudAction::RecipeBookChangeSettings {
                    kind,
                    open: book.open,
                    filtering: book.filtering,
                });
            }
        }
        if book.open {
            draw_recipe_book(
                ctx, mc, s, view, book, recipes, station, live, win, lang, &painter, book_kind,
                actions,
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
    let wheel_step = ctx.input(|i| {
        let d = i.smooth_scroll_delta.y;
        if d > 0.5 {
            1
        } else if d < -0.5 {
            -1
        } else {
            0
        }
    });
    let mut hover_item: Option<ItemSnapshot> = None;
    let mut hover_slot: Option<u16> = None;
    for (i, pos) in layout.slots.iter().enumerate() {
        if i >= view.slots.len() {
            break;
        }
        let rect = Rect::from_min_size(
            win.min + vec2(pos.0 * s, pos.1 * s),
            vec2(16.0 * s, 16.0 * s),
        );
        let crafter_slot = view.kind == "crafter_3x3" && i < 9;
        if let Some(item) = &view.slots[i] {
            draw_item(&painter, mc, icons, rect, item, s);
        } else if crafter_slot && crafter_slot_disabled(live.props, i as u16) {
            // Decompiled `CrafterScreen.extractDisabledSlot`: an 18×18 overlay
            // 1px outside the slot on every side, drawn instead of an item.
            let overlay = Rect::from_min_size(
                win.min + vec2((pos.0 - 1.0) * s, (pos.1 - 1.0) * s),
                vec2(18.0 * s, 18.0 * s),
            );
            if let Some(tex) = mc.tex.container_sprites.get("crafter/disabled_slot") {
                painter.image(tex.id(), overlay, FULL_UV, Color32::WHITE);
            }
        }
        if pointer.is_some_and(|p| rect.contains(p)) {
            painter.rect_filled(rect, 0.0, Color32::from_white_alpha(110));
            hover_item = view.slots[i].clone();
            hover_slot = Some(i as u16);
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
                // Decompiled `CrafterScreen.slotClicked`: a plain left click
                // on an EMPTY crafter slot toggles it disabled/enabled — a
                // disabled slot always re-enables; an enabled one only
                // disables while the cursor isn't carrying an item (so a
                // normal "place item here" click still works). Real vanilla
                // still runs the ordinary click afterward either way (it is
                // a no-op here since the slot is empty), so `SlotClick` below
                // is pushed unconditionally too.
                if crafter_slot && kind == SlotClickKind::Left && view.slots[i].is_none() {
                    if crafter_slot_disabled(live.props, i as u16) {
                        actions.push(HudAction::ContainerSlotStateChanged {
                            window_id: view.id,
                            slot: i as u16,
                            enabled: true,
                        });
                    } else if view.carried.is_none() {
                        actions.push(HudAction::ContainerSlotStateChanged {
                            window_id: view.id,
                            slot: i as u16,
                            enabled: false,
                        });
                    }
                }
                actions.push(HudAction::SlotClick {
                    window_id: view.id,
                    slot: i as u16,
                    kind,
                });
            }
            // Mouse wheel over an open bundle cycles which packed item a
            // following click extracts (`BundleMouseActions.onMouseScrolled`).
            if wheel_step != 0
                && let Some(item) = &view.slots[i]
                && !item.bundle_contents.is_empty()
            {
                let shown = bundle_items_to_show(item.bundle_contents.len()) as i32;
                if shown > 0 {
                    let current =
                        view.bundle_selected.filter(|(s, _)| *s == i as u16).map_or(-1, |(_, sel)| sel);
                    let next = next_bundle_selection(wheel_step, current, shown);
                    if next != current {
                        view.bundle_selected = Some((i as u16, next));
                        actions.push(HudAction::BundleSelectItem {
                            window_id: view.id,
                            slot: i as u16,
                            selected: next,
                        });
                    }
                }
            }
        } else if view.bundle_selected.is_some_and(|(slot, _)| slot == i as u16) {
            // The mouse left this bundle: vanilla's `onStopHovering` clears
            // the selection rather than leaving a stale extraction target.
            view.bundle_selected = None;
            actions.push(HudAction::BundleSelectItem { window_id: view.id, slot: i as u16, selected: -1 });
        }
    }

    // --- crafter redstone-power indicator -----------------------------------------
    // Decompiled `CrafterScreen.extractRedstone`: drawn screen-centre-relative
    // (not window-relative, though the window itself sits centred on screen
    // here too), independent of the recipe-book's own shift.
    if view.kind == "crafter_3x3" {
        let name =
            if crafter_powered(live.props) { "crafter/powered_redstone" } else { "crafter/unpowered_redstone" };
        if let Some(tex) = mc.tex.container_sprites.get(name) {
            let rect = Rect::from_min_size(
                screen.center() + vec2(9.0 * s, -48.0 * s),
                vec2(16.0 * s, 16.0 * s),
            );
            painter.image(tex.id(), rect, FULL_UV, Color32::WHITE);
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
                instruments: live.instruments,
            };
            let bundle_selected = view
                .bundle_selected
                .filter(|(slot, _)| Some(*slot) == hover_slot)
                .map(|(_, sel)| sel);
            tooltip(
                &painter, mc, s, lang, screen, p, item, icons, &reg, bundle_selected,
                ctx.input(|i| i.time),
            );
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
    fn crafter_slot_state_reads_per_slot_and_powered_properties() {
        let mut props = std::collections::HashMap::new();
        for i in 0..9u16 {
            assert!(!crafter_slot_disabled(&props, i), "nothing synced yet -> enabled");
        }
        assert!(!crafter_powered(&props));
        props.insert(3u16, 1u16);
        assert!(crafter_slot_disabled(&props, 3));
        assert!(!crafter_slot_disabled(&props, 4), "only slot 3 was disabled");
        props.insert(9u16, 1u16);
        assert!(crafter_powered(&props));
        // Index 9 is the powered flag, not a 10th slot.
        assert!(!crafter_slot_disabled(&props, 9));
    }

    #[test]
    fn lectern_page_reads_property_zero_and_clamps_to_the_real_page_count() {
        let mut props = std::collections::HashMap::new();
        assert_eq!(lectern_page(&props, 5), 0, "no data yet -> page 0, like a fresh open");
        props.insert(0u16, 3u16);
        assert_eq!(lectern_page(&props, 5), 3);
        // A book with fewer pages than the last-synced property (e.g. right
        // after the slot's book changed) must never index out of bounds.
        assert_eq!(lectern_page(&props, 2), 1);
        // An unrelated property (another screen's data slot) must not leak in.
        let mut other = std::collections::HashMap::new();
        other.insert(1u16, 7u16);
        assert_eq!(lectern_page(&other, 5), 0);
    }

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

    /// `BundleContents.getNumberOfItemsToShow()`: a full last row of four
    /// stays fully shown; a partial one gives up its remainder to the "+N"
    /// overflow cell so the grid never shows a ragged row.
    #[test]
    fn bundle_items_to_show_matches_vanilla() {
        assert_eq!(bundle_items_to_show(0), 0);
        assert_eq!(bundle_items_to_show(1), 1);
        assert_eq!(bundle_items_to_show(4), 4);
        assert_eq!(bundle_items_to_show(5), 5); // under the 12 cap: a partial row still shows fully
        assert_eq!(bundle_items_to_show(12), 12); // exactly at the cap: still all shown
        assert_eq!(bundle_items_to_show(13), 8); // over the cap: caps at 11, minus the partial row's 3
        assert_eq!(bundle_items_to_show(16), 11); // over the cap, but a full row: no extra hiding
        assert_eq!(bundle_items_to_show(64), 11); // 64 % 4 == 0, same as 16: capped at 11
    }

    /// `ScrollWheelHandler.getNextScrollWheelSelection`: circular stepping
    /// through `0..limit`, with the unselected `-1` resolving like one step
    /// before index 0.
    #[test]
    fn next_bundle_selection_matches_vanilla() {
        assert_eq!(next_bundle_selection(1, -1, 4), 3);
        assert_eq!(next_bundle_selection(-1, -1, 4), 0);
        assert_eq!(next_bundle_selection(1, 0, 4), 3);
        assert_eq!(next_bundle_selection(1, 3, 4), 2);
        assert_eq!(next_bundle_selection(-1, 3, 4), 0);
        assert_eq!(next_bundle_selection(-1, 0, 4), 1);
        assert_eq!(next_bundle_selection(1, 0, 1), 0);
    }

    /// `BundleContents.getWeight`/`computeContentWeight`: a plain stack costs
    /// `count / max_stack_size` of the bundle, a nested bundle costs its own
    /// weight plus the fixed 1/16 bundle-in-bundle overhead.
    #[test]
    fn bundle_weight_matches_vanilla() {
        // 16 iron ingots (max stack 64): 16/64 = 0.25.
        let ingots = ItemSnapshot { item: "iron_ingot".into(), count: 16, ..Default::default() };
        assert!((bundle_weight(std::slice::from_ref(&ingots)) - 0.25).abs() < 1e-9);

        // 64 of a max-1 item (an unstackable tool, e.g. a shield): full.
        let shield = ItemSnapshot { item: "shield".into(), count: 1, ..Default::default() };
        assert!((bundle_weight(std::slice::from_ref(&shield)) - 1.0).abs() < 1e-9);

        // An empty bundle costs nothing.
        assert_eq!(bundle_weight(&[]), 0.0);

        // A bundle nested one level deep, itself half full: 0.5 + 1/16.
        let inner = ItemSnapshot { item: "iron_ingot".into(), count: 32, ..Default::default() };
        let nested_bundle = ItemSnapshot {
            item: "bundle".into(),
            count: 1,
            bundle_contents: vec![inner],
            ..Default::default()
        };
        let w = bundle_weight(std::slice::from_ref(&nested_bundle));
        assert!((w - (0.5 + 1.0 / 16.0)).abs() < 1e-9);
    }

    /// `Mth.mulAndTruncate(weight, 94)`: truncating (not rounding) fill width.
    #[test]
    fn bundle_progressbar_fill_px_matches_vanilla() {
        assert_eq!(bundle_progressbar_fill_px(0.0), 0);
        assert_eq!(bundle_progressbar_fill_px(1.0), 94);
        assert_eq!(bundle_progressbar_fill_px(0.5), 47);
        // 0.25 * 94 = 23.5 → truncates down to 23, not rounds to 24.
        assert_eq!(bundle_progressbar_fill_px(0.25), 23);
    }

    /// `ChargedProjectiles.addToTooltip`'s run-length grouping: consecutive
    /// identical stacks collapse into one `(name, count)` entry; a run
    /// broken by a different item starts a fresh group even if the same
    /// item reappears later (matches vanilla's single linear pass, not a
    /// histogram).
    #[test]
    fn charged_projectile_groups_matches_vanilla() {
        let lang = Lang::empty();
        let arrow = || ItemSnapshot { item: "arrow".into(), count: 1, ..Default::default() };
        let firework = || ItemSnapshot { item: "firework_rocket".into(), count: 1, ..Default::default() };

        assert_eq!(charged_projectile_groups(&[], &lang), vec![]);
        assert_eq!(charged_projectile_groups(&[arrow()], &lang), vec![("Arrow".to_string(), 1)]);
        assert_eq!(
            charged_projectile_groups(&[firework(), firework(), firework()], &lang),
            vec![("Firework Rocket".to_string(), 3)]
        );
        // Non-consecutive same item: two separate groups, not merged.
        assert_eq!(
            charged_projectile_groups(&[arrow(), firework(), arrow()], &lang),
            vec![("Arrow".to_string(), 1), ("Firework Rocket".to_string(), 1), ("Arrow".to_string(), 1)]
        );
    }

    #[test]
    fn is_bundle_item_matches_vanilla() {
        assert!(is_bundle_item("bundle"));
        assert!(is_bundle_item("white_bundle"));
        assert!(is_bundle_item("black_bundle"));
        assert!(!is_bundle_item("shulker_box"));
        assert!(!is_bundle_item("chest"));
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
            instruments: &[],
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
    fn a_potion_takes_its_name_from_the_base_potion() {
        let lang = Lang::with(&[("item.minecraft.potion.effect.swiftness", "Potion of Swiftness")]);
        let mut item = stack("potion");
        item.potion = Some("swiftness".into());
        let lines = text_of(&tooltip_lines(&lang, &item, &Registries::EMPTY));
        assert_eq!(lines[0], "Potion of Swiftness");
    }

    #[test]
    fn an_extended_or_upgraded_potion_still_reads_as_the_base_name() {
        // "long_" and "strong_" only change the effect line, never the name.
        let lang = Lang::with(&[("item.minecraft.potion.effect.strength", "Potion of Strength")]);
        let mut item = stack("potion");
        item.potion = Some("strong_strength".into());
        let lines = text_of(&tooltip_lines(&lang, &item, &Registries::EMPTY));
        assert_eq!(lines[0], "Potion of Strength");
    }

    #[test]
    fn a_written_book_shows_its_title_and_author() {
        let lang = Lang::with(&[("book.byAuthor", "by %1$s")]);
        let mut item = stack("written_book");
        item.book = Some(crate::bridge::events::BookContent {
            title: "My Adventures".into(),
            author: "Steve".into(),
            generation: 0,
            pages: Vec::new(),
        });
        let lines = text_of(&tooltip_lines(&lang, &item, &Registries::EMPTY));
        assert_eq!(lines[0], "My Adventures");
        assert!(lines.iter().any(|l| l == "by Steve"), "{lines:?}");
    }

    #[test]
    fn a_music_disc_shows_its_composer_and_track_under_the_name() {
        let lang = Lang::with(&[
            ("item.minecraft.music_disc_13", "Music Disc"),
            ("item.minecraft.music_disc_13.desc", "C418 - 13"),
        ]);
        let lines = text_of(&tooltip_lines(&lang, &stack("music_disc_13"), &Registries::EMPTY));
        assert_eq!(lines[0], "Music Disc");
        assert_eq!(lines[1], "C418 - 13");
    }

    #[test]
    fn a_goat_horns_instrument_is_named_under_its_registry_reference() {
        let lang = Lang::with(&[("instrument.minecraft.ponder_goat_horn", "Ponder")]);
        let reg = Registries {
            enchantments: &[],
            trim_patterns: &[],
            trim_materials: &[],
            instruments: &["admire_goat_horn".to_string(), "ponder_goat_horn".to_string()],
        };
        let mut item = stack("goat_horn");
        item.instrument = Some(InstrumentDesc::Id(1));
        let lines = text_of(&tooltip_lines(&lang, &item, &reg));
        assert!(lines.iter().any(|l| l == "Ponder"), "{lines:?}");
    }

    #[test]
    fn a_datapack_instruments_inline_description_is_shown_verbatim() {
        let mut item = stack("goat_horn");
        item.instrument = Some(InstrumentDesc::Text("A Custom Tune".to_string()));
        let lines = text_of(&tooltip_lines(&lang(), &item, &Registries::EMPTY));
        assert!(lines.iter().any(|l| l == "A Custom Tune"), "{lines:?}");
    }

    #[test]
    fn an_enchanted_books_stored_enchantments_are_named_and_numbered() {
        let reg = Registries {
            enchantments: &["sharpness".to_string()],
            trim_patterns: &[],
            trim_materials: &[],
            instruments: &[],
        };
        let mut item = stack("enchanted_book");
        item.stored_enchantments = vec![(0, 3)];
        let lines = text_of(&tooltip_lines(&lang(), &item, &reg));
        assert!(lines.iter().any(|l| l.contains("Sharpness") && l.ends_with("III")), "{lines:?}");
    }

    #[test]
    fn an_ominous_bottles_bad_omen_runs_an_hour_and_forty_minutes() {
        let mut item = stack("ominous_bottle");
        item.effects = vec![("bad_omen".into(), 2, 120_000)];
        let lines = text_of(&tooltip_lines(&lang(), &item, &Registries::EMPTY));
        // Amplifier 2 is Bad Omen III, and past the hour mark the clock
        // grows an hours digit, unlike every other timed effect.
        assert!(lines.iter().any(|l| l.contains("III") && l.contains("(1:40:00)")), "{lines:?}");
    }

    #[test]
    fn duration_formatting_grows_an_hours_digit_past_sixty_minutes() {
        assert_eq!(format_duration(185), "(3:05)");
        assert_eq!(format_duration(6000), "(1:40:00)");
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
    book_kind: Option<crate::bridge::events::RecipeBookKind>,
    actions: &mut Vec<HudAction>,
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

    // --- "craftable only" filter toggle ------------------------------------
    // Real position decompiled from `RecipeBookComponent`'s filter
    // `CycleButton`: `(xo + 110, yo + 12, 26, 16)` relative to the panel's
    // own origin (this function's `origin`/`at`).
    let filter_rect = at(110.0, 12.0, 26.0, 16.0);
    if let Some(tex) = sprite(if book.filtering { "filter_enabled" } else { "filter_disabled" }) {
        painter.image(tex.id(), filter_rect, FULL_UV, Color32::WHITE);
    }
    if pointer.is_some_and(|p| filter_rect.contains(p)) && clicked {
        book.filtering = !book.filtering;
        book.page = 0;
        if let Some(kind) = book_kind {
            book.settings.set(kind, crate::app::hud::RecipeBookStationSettings {
                open: book.open,
                filtering: book.filtering,
            });
            actions.push(HudAction::RecipeBookChangeSettings {
                kind,
                open: book.open,
                filtering: book.filtering,
            });
        }
    }

    // --- the recipes -------------------------------------------------------
    // What the player is carrying decides which recipes are drawn lit up
    // (and, with the filter on, which are shown at all).
    let mut have: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for item in view.slots.iter().flatten() {
        *have.entry(item.item.clone()).or_insert(0) += item.count.max(1) as u32;
    }
    let tab = BookTab::ALL[book.tab.min(BookTab::ALL.len() - 1)];
    let mut page_of = recipes.page(tab, &book.search, station);
    if book.filtering {
        page_of.retain(|r| craftable(r, &have));
    }
    let pages = page_of.len().div_ceil(PER_PAGE).max(1);
    book.page = book.page.min(pages - 1);
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
    pub instruments: &'a [String],
}

impl Registries<'_> {
    /// An empty set, for the screens that have nothing to resolve.
    pub const EMPTY: Registries<'static> =
        Registries { enchantments: &[], trim_patterns: &[], trim_materials: &[], instruments: &[] };
}

/// Vanilla's grey.
const GREY: [u8; 3] = [0xAA, 0xAA, 0xAA];
/// Vanilla's blue for enchantments and trim lines.
const BLUE: [u8; 3] = [0x55, 0x55, 0xFF];

fn span(text: impl Into<String>, color: [u8; 3]) -> ChatSpan {
    ChatSpan { text: text.into(), color: Some(color), ..Default::default() }
}

/// Vanilla's per-rarity name colour: 0 Common (white), 1 Uncommon (yellow),
/// 2 Rare (aqua), 3 Epic (light purple).
fn rarity_color(rarity: u8) -> [u8; 3] {
    match rarity {
        1 => [0xFF, 0xFF, 0x55],
        2 => [0x55, 0xFF, 0xFF],
        3 => [0xFF, 0x55, 0xFF],
        _ => [0xFF, 0xFF, 0xFF],
    }
}

/// A potion, splash potion, lingering potion or tipped arrow's real name,
/// e.g. "Potion of Swiftness", "Water Bottle", "Arrow of Harming" — vanilla
/// looks this up as `item.minecraft.<item>.effect.<potion>` and, tellingly,
/// the lang file only ever defines the *base* potion id: "Potion of
/// Strength" is shown whether it's the normal, extended ("long_") or
/// upgraded ("strong_") brew, since only the effect line below the name
/// (amplifier, duration) actually changes. `None` for anything that isn't
/// one of these four items, or that carries no base potion at all.
fn potion_display_name(lang: &Lang, item_id: &str, potion: Option<&str>) -> Option<String> {
    if !matches!(item_id, "potion" | "splash_potion" | "lingering_potion" | "tipped_arrow") {
        return None;
    }
    let potion = potion?;
    let base = potion.strip_prefix("long_").or_else(|| potion.strip_prefix("strong_")).unwrap_or(potion);
    lang.get(&format!("item.minecraft.{item_id}.effect.{base}")).map(str::to_string)
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
        // No server custom name. A written book overrides its own name to
        // the title the player gave it, and a potion/tipped arrow's name
        // comes from its base potion rather than the generic registry name
        // — both exactly like vanilla's own `Item.getName()` overrides.
        // Everything else falls back to the translated registry name.
        _ => {
            let title = item.book.as_ref().filter(|b| !b.title.is_empty()).map(|b| b.title.clone());
            let name = title
                .or_else(|| potion_display_name(lang, &item.item, item.potion.as_deref()))
                .unwrap_or_else(|| lang.item_name(&item.item));
            lines.push(vec![span(name, rarity_color(item.rarity))]);
        }
    }
    // `tooltip_display.hide_tooltip`: the server wants nothing but the name
    // shown — no lore, enchantments, effects, attributes, durability, …
    if item.hide_tooltip {
        return lines;
    }

    // Music discs, disc fragments and the four-Trial-Chambers banner pattern
    // items carry no lore of their own — vanilla instead prints a second,
    // italic grey line straight from `item.minecraft.<id>.desc` in the lang
    // file (e.g. "C418 - 13" under "Music Disc").
    if let Some(desc) = lang.get(&format!("item.minecraft.{}.desc", item.item)) {
        lines.push(vec![ChatSpan { text: desc.to_string(), color: Some(GREY), italic: true, ..Default::default() }]);
    }

    // Enchantments, one per line, named and numbered like vanilla. An
    // enchanted book's stored enchantments render exactly the same way — a
    // book never carries both lists at once, so chaining them is safe.
    for (id, level) in item.enchantments.iter().chain(item.stored_enchantments.iter()) {
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

    // A charged crossbow's loaded projectiles: `ChargedProjectiles.
    // addToTooltip` groups consecutive identical stacks into one line each —
    // "Projectile: <Name>" for a single arrow, "Projectile: N x <Name>" for
    // Multishot's up to three fireworks (usually identical, since a crossbow
    // loads the same projectile into every slot it draws).
    for (name, count) in charged_projectile_groups(&item.charged_projectiles, lang) {
        let text = if count == 1 {
            lang.get("item.minecraft.crossbow.projectile.single")
                .map(|t| t.replace("%s", &name))
                .unwrap_or_else(|| format!("Projectile: {name}"))
        } else {
            lang.get("item.minecraft.crossbow.projectile.multiple")
                .map(|t| t.replacen("%s", &count.to_string(), 1).replacen("%s", &name, 1))
                .unwrap_or_else(|| format!("Projectile: {count} x {name}"))
        };
        lines.push(vec![span(text, GREY)]);
    }

    // A goat horn's instrument: `InstrumentComponent.addToTooltip` prints one
    // grey line straight from the instrument's description text.
    if let Some(desc) = &item.instrument {
        let text = match desc {
            InstrumentDesc::Id(id) => reg.instruments.get(*id as usize).map(|name| {
                lang.get(&format!("instrument.minecraft.{name}"))
                    .map(str::to_string)
                    .unwrap_or_else(|| crate::assets::prettify(name))
            }),
            InstrumentDesc::Text(t) => Some(t.clone()),
        };
        if let Some(text) = text {
            lines.push(vec![span(text, GREY)]);
        }
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
            text.push(' ');
            text.push_str(&format_duration((*duration / 20).max(0) as u32));
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

    // A signed book's author line, then its generation — copies get a
    // little more worn each time. Vanilla prints "by <author>" right under
    // the title, above the generation line.
    if let Some(book) = &item.book {
        if !book.author.is_empty() {
            let text = lang
                .get("book.byAuthor")
                .map(|t| t.replacen("%1$s", &book.author, 1))
                .unwrap_or_else(|| format!("by {}", book.author));
            lines.push(vec![span(text, GREY)]);
        }
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

    // Adventure-mode restrictions — always shown on the tooltip, not just in
    // adventure mode itself, exactly like vanilla.
    if !item.can_break.is_empty() {
        lines.push(vec![span(
            lang.get("item.canBreak").unwrap_or("Can break:"),
            GREY,
        )]);
        for block in &item.can_break {
            lines.push(vec![span(format!(" {}", lang.item_name(block)), BLUE)]);
        }
    }
    if !item.can_place_on.is_empty() {
        lines.push(vec![span(
            lang.get("item.canPlace").unwrap_or("Can place on:"),
            GREY,
        )]);
        for block in &item.can_place_on {
            lines.push(vec![span(format!(" {}", lang.item_name(block)), BLUE)]);
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

/// An effect's remaining time in parentheses, e.g. "(4:00)" or, once an hour
/// is on the clock — an Ominous Bottle's Bad Omen runs 100 minutes — "(1:40:00)".
fn format_duration(secs: u32) -> String {
    if secs >= 3600 {
        format!("({}:{:02}:{:02})", secs / 3600, (secs / 60) % 60, secs % 60)
    } else {
        format!("({}:{:02})", secs / 60, secs % 60)
    }
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
            tooltip(
                &painter, mc, s, lang, screen, p, item, icons, registries, None,
                ctx.input(|i| i.time),
            );
        }
    }
}
