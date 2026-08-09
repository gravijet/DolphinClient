//! The creative menu: every item in the game, searchable, and the rules for
//! how many of one you get.
//!
//! Vanilla keeps its creative tab contents in code rather than in the assets,
//! so there is nothing to read out of the jar. What there *is* is the item
//! registry, in the game's own order — the same order vanilla's "Search Items"
//! tab walks — and that is what this lists.

use std::sync::Arc;

/// Window size of the creative screen (vanilla `CreativeModeInventoryScreen`).
pub const W: f32 = 195.0;
pub const H: f32 = 136.0;
/// The 9×5 item grid.
pub const COLS: usize = 9;
pub const ROWS: usize = 5;
pub const PAGE: usize = COLS * ROWS;
pub const GRID_X: f32 = 9.0;
pub const GRID_Y: f32 = 18.0;
/// The player's hotbar along the bottom of an item tab.
pub const HOTBAR_Y: f32 = 112.0;
/// The scrollbar: a 12×15 knob running down a 95px track.
pub const SCROLL_X: f32 = 175.0;
pub const SCROLL_Y: f32 = 18.0;
pub const SCROLL_TRACK: f32 = 95.0;
/// The search field's text area.
pub const SEARCH_BOX: (f32, f32, f32, f32) = (82.0, 6.0, 80.0, 9.0);
/// Tabs: 26×32 sprites, seven to a row.
pub const TAB_W: f32 = 26.0;
pub const TAB_H: f32 = 32.0;
/// Which of the seven columns each of our two tabs sits in (vanilla puts both
/// at the right-hand end of their row).
pub const TAB_COL: f32 = 6.0;

/// Which tab of the creative menu is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// Every item, filtered by the search box.
    Search,
    /// The player's own inventory — the ordinary screen, with real slots.
    Inventory,
}

/// The creative menu's own state. The server has no idea this screen exists:
/// picking an item up is a local decision, and only putting one down turns
/// into a packet.
pub struct Creative {
    /// Every item in the game, in registry order.
    all: Vec<Arc<str>>,
    /// Indices into `all` that match the search.
    matches: Vec<u32>,
    pub tab: Tab,
    pub search: String,
    /// 0..1 of the way down the list.
    pub scroll: f32,
}

impl Creative {
    pub fn new(all: Vec<Arc<str>>) -> Self {
        let matches = (0..all.len() as u32).collect();
        Self { all, matches, tab: Tab::Search, search: String::new(), scroll: 0.0 }
    }

    /// Re-run the search. Vanilla matches on the item's display name; we match
    /// on the registry name, which is the same words with underscores.
    pub fn refilter(&mut self) {
        let needle = self.search.trim().to_lowercase().replace(' ', "_");
        self.matches = if needle.is_empty() {
            (0..self.all.len() as u32).collect()
        } else {
            self.all
                .iter()
                .enumerate()
                .filter(|(_, n)| n.contains(&needle))
                .map(|(i, _)| i as u32)
                .collect()
        };
        self.scroll = 0.0;
    }

    pub fn total(&self) -> usize {
        self.matches.len()
    }

    /// How many rows of items are hidden below the window.
    pub fn hidden_rows(&self) -> usize {
        self.total().div_ceil(COLS).saturating_sub(ROWS)
    }

    pub fn can_scroll(&self) -> bool {
        self.hidden_rows() > 0
    }

    /// Scroll by whole rows, the way the wheel does in vanilla.
    pub fn scroll_by(&mut self, rows: f32) {
        let hidden = self.hidden_rows();
        if hidden == 0 {
            self.scroll = 0.0;
            return;
        }
        self.scroll = (self.scroll - rows / hidden as f32).clamp(0.0, 1.0);
    }

    /// The 45 items on screen, as (slot index, name).
    pub fn page(&self) -> Vec<(usize, &str)> {
        let first_row = (self.scroll * self.hidden_rows() as f32).round() as usize;
        let start = first_row * COLS;
        self.matches
            .iter()
            .skip(start)
            .take(PAGE)
            .enumerate()
            .map(|(i, &idx)| (i, &*self.all[idx as usize]))
            .collect()
    }
}

/// How many of an item one creative click hands over — vanilla gives a full
/// stack, and the server refuses the packet outright if we ask for more than
/// the item can hold.
pub fn stack_size(n: &str) -> u32 {
    // Tools, weapons, armour and everything else that comes one at a time.
    if n.ends_with("_sword")
        || n.ends_with("_pickaxe")
        || n.ends_with("_axe")
        || n.ends_with("_shovel")
        || n.ends_with("_hoe")
        || n.ends_with("_helmet")
        || n.ends_with("_chestplate")
        || n.ends_with("_leggings")
        || n.ends_with("_boots")
        || n.ends_with("_horse_armor")
        || n.ends_with("_boat")
        || n.ends_with("_raft")
        || n.ends_with("_bed")
        || n.ends_with("_minecart")
        || n.ends_with("_potion")
        || n.ends_with("_bundle")
        || n.starts_with("music_disc_")
        || (n.ends_with("_bucket") && n != "bucket")
    {
        return 1;
    }
    if matches!(
        n,
        "bow"
            | "crossbow"
            | "trident"
            | "shield"
            | "elytra"
            | "mace"
            | "brush"
            | "shears"
            | "flint_and_steel"
            | "fishing_rod"
            | "carrot_on_a_stick"
            | "warped_fungus_on_a_stick"
            | "spyglass"
            | "saddle"
            | "wolf_armor"
            | "potion"
            | "bundle"
            | "cake"
            | "totem_of_undying"
            | "goat_horn"
            | "debug_stick"
            | "enchanted_book"
            | "knowledge_book"
            | "written_book"
            | "writable_book"
            | "suspicious_stew"
            | "mushroom_stew"
            | "rabbit_stew"
            | "beetroot_soup"
            | "ominous_bottle"
            | "milk_bucket"
    ) {
        return 1;
    }
    // Sixteens.
    if n.ends_with("_sign") || n.ends_with("_banner") || n == "bucket" {
        return 16;
    }
    if matches!(n, "snowball" | "egg" | "ender_pearl" | "armor_stand" | "honey_bottle") {
        return 16;
    }
    64
}

/// The item a block hands over when you pick it — vanilla's
/// `getCloneItemStack`. Most blocks give the item of the same name; the ones
/// that don't are the plants, the wiring and the things that stand on a wall.
///
/// `known` answers whether an item of that name exists, so a block with no
/// item at all (a fluid, a fire, the moving piston) picks nothing.
pub fn pick_item(block: &str, known: impl Fn(&str) -> bool) -> Option<String> {
    let mapped = match block {
        "redstone_wire" => "redstone",
        "tripwire" => "string",
        "wheat" => "wheat_seeds",
        "carrots" => "carrot",
        "potatoes" => "potato",
        "beetroots" => "beetroot_seeds",
        "melon_stem" | "attached_melon_stem" => "melon_seeds",
        "pumpkin_stem" | "attached_pumpkin_stem" => "pumpkin_seeds",
        "torchflower_crop" => "torchflower_seeds",
        "pitcher_crop" => "pitcher_pod",
        "cocoa" => "cocoa_beans",
        "kelp_plant" => "kelp",
        "bamboo_sapling" => "bamboo",
        "cave_vines" | "cave_vines_plant" => "glow_berries",
        "twisting_vines_plant" => "twisting_vines",
        "weeping_vines_plant" => "weeping_vines",
        "big_dripleaf_stem" => "big_dripleaf",
        "sweet_berry_bush" => "sweet_berries",
        "tall_seagrass" => "seagrass",
        "farmland" | "dirt_path" => "dirt",
        "lava" => "lava_bucket",
        "water" | "bubble_column" => "water_bucket",
        "powder_snow" => "powder_snow_bucket",
        "nether_portal" | "end_portal" | "end_gateway" | "fire" | "soul_fire" | "moving_piston"
        | "piston_head" | "air" | "cave_air" | "void_air" => return None,
        other => {
            // Anything on a wall is the same item as its standing form.
            let base = other
                .strip_prefix("wall_")
                .map(|s| s.to_string())
                .or_else(|| other.strip_suffix("_wall_torch").map(|s| format!("{s}_torch")))
                .or_else(|| other.strip_suffix("_wall_sign").map(|s| format!("{s}_sign")))
                .or_else(|| {
                    other
                        .strip_suffix("_wall_hanging_sign")
                        .map(|s| format!("{s}_hanging_sign"))
                })
                .or_else(|| other.strip_suffix("_wall_banner").map(|s| format!("{s}_banner")))
                .or_else(|| other.strip_suffix("_wall_head").map(|s| format!("{s}_head")))
                .or_else(|| other.strip_suffix("_wall_skull").map(|s| format!("{s}_skull")))
                .or_else(|| other.strip_suffix("_wall_fan").map(|s| format!("{s}_fan")))
                .unwrap_or_else(|| other.to_string());
            return known(&base).then_some(base);
        }
    };
    known(mapped).then(|| mapped.to_string())
}

/// Every item in the game, in the registry's own order.
pub fn registry_items() -> Vec<Arc<str>> {
    use azalea::registry::Registry as _;
    let mut out: Vec<Arc<str>> = Vec::new();
    for id in 0u32.. {
        let Some(item) = azalea::registry::builtin::ItemKind::from_u32(id) else { break };
        let name = item.to_string();
        let short = name.strip_prefix("minecraft:").unwrap_or(&name);
        if short == "air" {
            continue;
        }
        out.push(Arc::from(short));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(names: &[&str]) -> Creative {
        Creative::new(names.iter().map(|n| Arc::from(*n)).collect())
    }

    #[test]
    fn the_registry_has_every_item_in_it() {
        let items = registry_items();
        assert!(items.len() > 1000, "only {} items", items.len());
        assert!(items.iter().any(|n| &**n == "stone"));
        assert!(items.iter().any(|n| &**n == "diamond_sword"));
        assert!(!items.iter().any(|n| &**n == "air"), "air is not an item you can hold");
    }

    #[test]
    fn searching_narrows_the_list() {
        let mut c = menu(&["stone", "stone_bricks", "diamond", "diamond_sword"]);
        assert_eq!(c.total(), 4);
        c.search = "diamond".into();
        c.refilter();
        assert_eq!(c.total(), 2);
        // A space reads as the underscore the registry name uses.
        c.search = "stone bricks".into();
        c.refilter();
        assert_eq!(c.total(), 1);
    }

    #[test]
    fn a_short_list_does_not_scroll() {
        let mut c = menu(&["stone", "dirt"]);
        assert!(!c.can_scroll());
        c.scroll_by(-3.0);
        assert_eq!(c.scroll, 0.0);
        assert_eq!(c.page().len(), 2);
    }

    #[test]
    fn scrolling_walks_the_list_a_row_at_a_time() {
        let names: Vec<String> = (0..90).map(|i| format!("item_{i}")).collect();
        let mut c = Creative::new(names.iter().map(|n| Arc::from(n.as_str())).collect());
        assert_eq!(c.hidden_rows(), 5);
        assert_eq!(c.page()[0].1, "item_0");
        c.scroll_by(-1.0);
        assert_eq!(c.page()[0].1, "item_9", "one notch is one row of nine");
        c.scroll_by(-99.0);
        assert_eq!(c.scroll, 1.0);
        assert_eq!(c.page()[0].1, "item_45");
        assert_eq!(c.page().len(), PAGE);
    }

    #[test]
    fn stacks_are_the_size_vanilla_allows() {
        assert_eq!(stack_size("stone"), 64);
        assert_eq!(stack_size("diamond_sword"), 1);
        assert_eq!(stack_size("netherite_boots"), 1);
        assert_eq!(stack_size("oak_boat"), 1);
        assert_eq!(stack_size("water_bucket"), 1);
        assert_eq!(stack_size("bucket"), 16);
        assert_eq!(stack_size("oak_sign"), 16);
        assert_eq!(stack_size("ender_pearl"), 16);
        assert_eq!(stack_size("music_disc_cat"), 1);
    }

    #[test]
    fn picking_a_block_gives_the_right_item() {
        let known = |n: &str| n != "wall_torch" && n != "moving_piston";
        assert_eq!(pick_item("stone", known).as_deref(), Some("stone"));
        assert_eq!(pick_item("redstone_wire", known).as_deref(), Some("redstone"));
        assert_eq!(pick_item("oak_wall_sign", known).as_deref(), Some("oak_sign"));
        assert_eq!(pick_item("wheat", known).as_deref(), Some("wheat_seeds"));
        assert_eq!(pick_item("lava", known).as_deref(), Some("lava_bucket"));
        assert_eq!(pick_item("air", known), None);
        assert_eq!(pick_item("moving_piston", known), None);
    }
}
