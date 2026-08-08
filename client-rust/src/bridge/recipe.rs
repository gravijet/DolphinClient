//! Recipes, as the server describes them.
//!
//! Since 1.21.2 the server no longer sends recipes as data the client has to
//! know how to craft with — it sends a *display*: what to draw in each slot and
//! what comes out. That is all the recipe book and the stonecutter screen need,
//! and it means neither has to reimplement any crafting logic.
//!
//! A slot display is a little tree (an item, a stack, a tag, "any fuel", a
//! composite of several of those), so everything here boils one down to the
//! plain item names our screens can look icons up by.

use azalea::protocol::common::recipe::{
    Ingredient, RecipeDisplayData, SlotDisplayData,
};
use azalea::registry::Registry as _;
use azalea_inventory::ItemStack;

use super::events::{BookRecipe, RecipeKind};
use super::strip_minecraft_ns;

/// Every item a slot display can stand for, in the order it would cycle through
/// them in vanilla's book. Empty for "nothing here".
pub fn display_items(display: &SlotDisplayData) -> Vec<String> {
    let mut out = Vec::new();
    collect(display, &mut out);
    out
}

fn collect(display: &SlotDisplayData, out: &mut Vec<String>) {
    match display {
        SlotDisplayData::Empty | SlotDisplayData::AnyFuel => {}
        SlotDisplayData::Item(i) => out.push(strip_minecraft_ns(i.item.to_str())),
        SlotDisplayData::ItemStack(s) => {
            if let ItemStack::Present(d) = &s.stack {
                out.push(strip_minecraft_ns(d.kind.to_str()));
            }
        }
        SlotDisplayData::Tag(_) => {}
        SlotDisplayData::Composite(c) => {
            for part in &c.contents {
                collect(part, out);
            }
        }
        // Wrappers: what matters is the thing being wrapped.
        SlotDisplayData::WithAnyPotion(w) => collect(&w.contents, out),
        SlotDisplayData::OnlyWithComponent(w) => collect(&w.contents, out),
        SlotDisplayData::WithRemainder(w) => collect(&w.input, out),
        SlotDisplayData::Dyed(w) => collect(&w.target, out),
        SlotDisplayData::SmithingTrim(w) => collect(&w.base, out),
    }
}

/// The single item a slot display stands for, if it stands for exactly one
/// thing — what a result slot always is.
pub fn display_item(display: &SlotDisplayData) -> Option<String> {
    display_items(display).into_iter().next()
}

/// How many of it a result display makes (an `ItemStack` display carries a
/// count; everything else means one).
fn display_count(display: &SlotDisplayData) -> u32 {
    match display {
        SlotDisplayData::ItemStack(s) => match &s.stack {
            ItemStack::Present(d) => d.count.max(1) as u32,
            ItemStack::Empty => 1,
        },
        SlotDisplayData::WithRemainder(w) => display_count(&w.input),
        _ => 1,
    }
}

/// The items an ingredient accepts. A tag ingredient arrives as a named holder
/// set with no contents, so there is nothing to list — the display carries the
/// drawable items instead.
pub fn ingredient_items(ingredient: &Ingredient) -> Vec<String> {
    use azalea::registry::HolderSet;
    match &ingredient.allowed {
        HolderSet::Direct { contents } => {
            contents.iter().map(|i| strip_minecraft_ns(i.to_str())).collect()
        }
        HolderSet::Named { .. } => Vec::new(),
    }
}

/// One entry of the recipe book, from the server's display of it.
pub fn book_entry(
    entry: &azalea::protocol::packets::game::c_recipe_book_add::RecipeDisplayEntry,
) -> Option<BookRecipe> {
    let mut recipe = from_display(entry.id, &entry.display)?;
    recipe.category = entry.category.to_u32();
    Some(recipe)
}

/// Turn a recipe display into the flat shape our screens draw.
pub fn from_display(id: u32, display: &RecipeDisplayData) -> Option<BookRecipe> {
    let (result, shape, ingredients, kind) = match display {
        RecipeDisplayData::Shapeless(r) => (
            &r.result,
            None,
            r.ingredients.iter().map(display_items).collect(),
            RecipeKind::Crafting,
        ),
        RecipeDisplayData::Shaped(r) => (
            &r.result,
            Some((r.width.clamp(1, 3), r.height.clamp(1, 3))),
            r.ingredients.iter().map(display_items).collect(),
            RecipeKind::Crafting,
        ),
        RecipeDisplayData::Furnace(r) => (
            &r.result,
            None,
            vec![display_items(&r.ingredient)],
            RecipeKind::Furnace,
        ),
        RecipeDisplayData::Stonecutter(r) => (
            &r.result,
            None,
            vec![display_items(&r.input)],
            RecipeKind::Stonecutter,
        ),
        RecipeDisplayData::Smithing(r) => (
            &r.result,
            None,
            vec![
                display_items(&r.template),
                display_items(&r.base),
                display_items(&r.addition),
            ],
            RecipeKind::Smithing,
        ),
    };
    Some(BookRecipe {
        id,
        result: display_item(result)?,
        result_count: display_count(result),
        shape,
        ingredients,
        category: 0,
        kind,
    })
}
