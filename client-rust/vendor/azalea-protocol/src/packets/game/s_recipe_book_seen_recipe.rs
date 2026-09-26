use azalea_buf::AzBuf;
use azalea_protocol_macros::ServerboundGamePacket;

/// Real 26.1 wire format, decompiled from `ServerboundRecipeBookSeenRecipePacket`
/// (a record of `(RecipeDisplayId recipe)`, itself a single VarInt index) —
/// same fix as `ServerboundPlaceRecipe`, see that file's comment.
#[derive(AzBuf, Clone, Debug, PartialEq, ServerboundGamePacket)]
pub struct ServerboundRecipeBookSeenRecipe {
    #[var]
    pub recipe: u32,
}
