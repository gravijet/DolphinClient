//! The recipe book.
//!
//! Everything the player has unlocked, kept the way the server sends it: one
//! entry per recipe display, with the item it makes and the items each of its
//! slots accepts. The screen groups them into vanilla's tabs, filters them by
//! what has been typed into the search box, and shows the chosen one as a ghost
//! in the crafting grid.
//!
//! Vanilla decides a recipe is *craftable* by checking the player's inventory
//! against the ingredients; so does this, which is why the book can grey out
//! what you cannot make yet without asking the server anything.

use std::collections::HashMap;

use crate::bridge::events::{BookRecipe, RecipeKind};

/// A tab down the side of the book. Vanilla's categories collapse into these:
/// the four crafting groups, and one per cooking station.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookTab {
    /// Everything, which is what the search box searches.
    Search,
    Building,
    Redstone,
    Equipment,
    Misc,
}

impl BookTab {
    pub const ALL: [BookTab; 5] =
        [BookTab::Search, BookTab::Building, BookTab::Redstone, BookTab::Equipment, BookTab::Misc];

    /// The item vanilla draws on the tab.
    pub fn icon(self) -> &'static str {
        match self {
            BookTab::Search => "compass",
            BookTab::Building => "bricks",
            BookTab::Redstone => "redstone",
            BookTab::Equipment => "iron_axe",
            BookTab::Misc => "lava_bucket",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BookTab::Search => "Search",
            BookTab::Building => "Building Blocks",
            BookTab::Redstone => "Redstone",
            BookTab::Equipment => "Equipment",
            BookTab::Misc => "Miscellaneous",
        }
    }

    /// Which tab a server recipe-book category belongs under. The category is
    /// the raw registry id of `RecipeBookCategory`, whose order is fixed:
    /// the four crafting groups first, then the cooking ones.
    fn of_category(category: u32, kind: RecipeKind) -> BookTab {
        match category {
            0 => BookTab::Building,
            1 => BookTab::Redstone,
            2 => BookTab::Equipment,
            3 => BookTab::Misc,
            // Everything cooked, cut or smithed lands in Miscellaneous rather
            // than in a crafting group it cannot be made in.
            _ => match kind {
                RecipeKind::Crafting => BookTab::Misc,
                _ => BookTab::Misc,
            },
        }
    }
}

/// Every recipe the player knows.
#[derive(Default)]
pub struct RecipeBook {
    entries: Vec<BookRecipe>,
    /// Where each recipe id lives in `entries`, so removals are cheap.
    index: HashMap<u32, usize>,
}

impl RecipeBook {
    /// Take what the server sent. `replace` means this is the whole book.
    pub fn add(&mut self, entries: Vec<BookRecipe>, replace: bool) {
        if replace {
            self.entries.clear();
            self.index.clear();
        }
        for entry in entries {
            match self.index.get(&entry.id) {
                Some(&at) => self.entries[at] = entry,
                None => {
                    self.index.insert(entry.id, self.entries.len());
                    self.entries.push(entry);
                }
            }
        }
    }

    /// Forget recipes by id.
    pub fn remove(&mut self, ids: &[u32]) {
        if ids.is_empty() {
            return;
        }
        let drop: std::collections::HashSet<u32> = ids.iter().copied().collect();
        self.entries.retain(|e| !drop.contains(&e.id));
        self.reindex();
    }

    fn reindex(&mut self) {
        self.index = self.entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    /// The recipes to show on a tab, optionally narrowed by a search string.
    /// Vanilla searches the *name* of what a recipe makes, so searching "boat"
    /// finds every boat rather than everything made of planks.
    pub fn page(&self, tab: BookTab, search: &str, station: Station) -> Vec<&BookRecipe> {
        let needle = search.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|r| station.accepts(r.kind))
            .filter(|r| tab == BookTab::Search || BookTab::of_category(r.category, r.kind) == tab)
            .filter(|r| needle.is_empty() || r.result.replace('_', " ").contains(&needle))
            .collect()
    }
}

/// Which station the book is open in front of — a furnace only ever shows
/// smelting recipes, a crafting table only crafting ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Station {
    Crafting,
    Furnace,
}

impl Station {
    /// The station a container kind puts the book in, if it has one at all.
    pub fn of_kind(kind: &str) -> Option<Station> {
        match kind {
            "crafting" | "player" => Some(Station::Crafting),
            "furnace" | "smoker" | "blast_furnace" => Some(Station::Furnace),
            _ => None,
        }
    }

    fn accepts(self, kind: RecipeKind) -> bool {
        match self {
            Station::Crafting => kind == RecipeKind::Crafting,
            Station::Furnace => kind == RecipeKind::Furnace,
        }
    }
}

/// Can this recipe be made from these items? `have` is how many of each item
/// name the player is carrying. Vanilla greys out what you cannot make; a slot
/// that accepts several items is satisfied by any one of them.
pub fn craftable(recipe: &BookRecipe, have: &HashMap<String, u32>) -> bool {
    let mut used: HashMap<&str, u32> = HashMap::new();
    for slot in &recipe.ingredients {
        if slot.is_empty() {
            continue;
        }
        // Take the first option there is still stock of.
        let Some(pick) = slot.iter().find(|item| {
            let taken = used.get(item.as_str()).copied().unwrap_or(0);
            have.get(item.as_str()).copied().unwrap_or(0) > taken
        }) else {
            return false;
        };
        *used.entry(pick.as_str()).or_insert(0) += 1;
    }
    true
}

/// Where each ingredient of a recipe goes in a 3×3 grid, as slot indexes 0..8.
/// A shaped recipe keeps its shape (and sits in the top-left, like vanilla's
/// ghost does); a shapeless one is simply filled in reading order.
pub fn grid_slots(recipe: &BookRecipe) -> Vec<(usize, &Vec<String>)> {
    let mut out = Vec::new();
    match recipe.shape {
        Some((w, h)) => {
            let w = w.max(1) as usize;
            for (i, slot) in recipe.ingredients.iter().enumerate() {
                let (col, row) = (i % w, i / w);
                if row >= h.max(1) as usize || col >= 3 || row >= 3 {
                    continue;
                }
                out.push((row * 3 + col, slot));
            }
        }
        None => {
            for (i, slot) in recipe.ingredients.iter().enumerate().take(9) {
                out.push((i, slot));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(id: u32, result: &str, ingredients: &[&[&str]]) -> BookRecipe {
        BookRecipe {
            id,
            result: result.to_owned(),
            result_count: 1,
            shape: None,
            ingredients: ingredients
                .iter()
                .map(|s| s.iter().map(|i| (*i).to_string()).collect())
                .collect(),
            category: 0,
            kind: RecipeKind::Crafting,
        }
    }

    #[test]
    fn the_book_replaces_and_forgets() {
        let mut book = RecipeBook::default();
        book.add(vec![recipe(1, "stick", &[&["oak_planks"]])], false);
        book.add(vec![recipe(2, "torch", &[&["coal"], &["stick"]])], false);
        assert_eq!(book.len(), 2);
        // The same id again updates in place rather than doubling up.
        book.add(vec![recipe(1, "stick", &[&["birch_planks"]])], false);
        assert_eq!(book.len(), 2);
        book.remove(&[1]);
        assert_eq!(book.len(), 1);
        // A replacing add throws the old book away.
        book.add(vec![recipe(9, "bread", &[&["wheat"]])], true);
        assert_eq!(book.len(), 1);
        assert_eq!(book.page(BookTab::Search, "", Station::Crafting)[0].result, "bread");
    }

    #[test]
    fn searching_matches_what_the_recipe_makes() {
        let mut book = RecipeBook::default();
        book.add(
            vec![
                recipe(1, "oak_boat", &[&["oak_planks"]]),
                recipe(2, "birch_boat", &[&["birch_planks"]]),
                recipe(3, "stone_pickaxe", &[&["cobblestone"]]),
            ],
            true,
        );
        assert_eq!(book.page(BookTab::Search, "boat", Station::Crafting).len(), 2);
        assert_eq!(book.page(BookTab::Search, "pick", Station::Crafting).len(), 1);
        // Vanilla searches on the readable name, so a space matches too.
        assert_eq!(book.page(BookTab::Search, "oak boat", Station::Crafting).len(), 1);
    }

    #[test]
    fn a_furnace_only_shows_what_it_can_cook() {
        let mut book = RecipeBook::default();
        let mut smelt = recipe(1, "iron_ingot", &[&["raw_iron"]]);
        smelt.kind = RecipeKind::Furnace;
        book.add(vec![smelt, recipe(2, "stick", &[&["oak_planks"]])], true);
        assert_eq!(book.page(BookTab::Search, "", Station::Furnace).len(), 1);
        assert_eq!(book.page(BookTab::Search, "", Station::Crafting).len(), 1);
    }

    #[test]
    fn craftable_counts_every_slot_separately() {
        let torch = recipe(1, "torch", &[&["coal", "charcoal"], &["stick"]]);
        let mut have = HashMap::new();
        have.insert("stick".to_string(), 1);
        assert!(!craftable(&torch, &have), "no coal yet");
        have.insert("charcoal".to_string(), 1);
        assert!(craftable(&torch, &have), "charcoal also satisfies the slot");

        // Two slots of the same item need two of it.
        let planks = recipe(2, "crafting_table", &[&["oak_planks"], &["oak_planks"]]);
        let mut one = HashMap::new();
        one.insert("oak_planks".to_string(), 1);
        assert!(!craftable(&planks, &one));
        one.insert("oak_planks".to_string(), 2);
        assert!(craftable(&planks, &one));
    }

    #[test]
    fn a_shaped_recipe_keeps_its_shape_in_the_grid() {
        let mut pickaxe = recipe(1, "wooden_pickaxe", &[
            &["oak_planks"], &["oak_planks"], &["oak_planks"],
            &[], &["stick"], &[],
            &[], &["stick"], &[],
        ]);
        pickaxe.shape = Some((3, 3));
        let slots: Vec<usize> = grid_slots(&pickaxe).iter().map(|(i, _)| *i).collect();
        assert_eq!(slots, (0..9).collect::<Vec<_>>());

        // A 2×2 recipe sits in the top-left corner of the 3×3 grid.
        let mut torch = recipe(2, "torch", &[&["coal"], &["stick"]]);
        torch.shape = Some((1, 2));
        let slots: Vec<usize> = grid_slots(&torch).iter().map(|(i, _)| *i).collect();
        assert_eq!(slots, vec![0, 3]);
    }
}
