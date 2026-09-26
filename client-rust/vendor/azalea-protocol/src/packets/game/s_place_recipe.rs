use azalea_buf::AzBuf;
use azalea_protocol_macros::ServerboundGamePacket;

/// Real 26.1 wire format, decompiled from `ServerboundPlaceRecipePacket`
/// (a record of `(int containerId, RecipeDisplayId recipe, boolean
/// useMaxItems)`, where `RecipeDisplayId` is itself just a single VarInt
/// index) — azalea's published struct still keyed `recipe` by the old
/// `Identifier` shape from before the recipe-book display-id rework
/// (`ClientboundRecipeBookAdd` already correctly uses a `#[var] u32` id for
/// the clientbound direction, this brings the serverbound side in line).
#[derive(AzBuf, Clone, Debug, PartialEq, ServerboundGamePacket)]
pub struct ServerboundPlaceRecipe {
    #[var]
    pub container_id: i32,
    #[var]
    pub recipe: u32,
    pub use_max_items: bool,
}
