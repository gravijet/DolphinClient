//! Block → vanilla sound-group mapping ("block.<group>.break/place/hit").
//!
//! azalea carries no per-block SoundType data, so this is a curated table
//! plus name heuristics. Unknown blocks fall back to "stone", which always
//! exists in sounds.json — a slightly wrong sound beats silence.

/// The vanilla SoundType group for a block's registry short name
/// (no `minecraft:` prefix), e.g. "oak_planks" → "wood".
pub fn group(short_name: &str) -> &'static str {
    let n = short_name;

    // --- exact matches first (blocks whose group isn't guessable) ---------
    match n {
        "grass_block" | "dirt_path" => return "grass",
        "dirt" | "coarse_dirt" | "rooted_dirt" | "farmland" | "mud" | "clay" => return "gravel",
        "sand" | "red_sand" => return "sand",
        "gravel" => return "gravel",
        "glass" | "glass_pane" | "tinted_glass" | "sea_lantern" | "glowstone" | "redstone_lamp" => {
            return "glass";
        }
        "ice" | "packed_ice" | "blue_ice" | "frosted_ice" => return "glass",
        "snow" | "snow_block" | "powder_snow" => return "snow",
        "soul_sand" => return "soul_sand",
        "soul_soil" => return "soul_soil",
        "netherrack" | "nether_quartz_ore" | "nether_gold_ore" => return "netherrack",
        "cobweb" => return "cobweb",
        "slime_block" => return "slime_block",
        "honey_block" => return "honey_block",
        "bamboo" | "bamboo_sapling" => return "bamboo",
        "scaffolding" => return "scaffolding",
        "anvil" | "chipped_anvil" | "damaged_anvil" => return "anvil",
        "chain" => return "chain",
        "bone_block" => return "bone_block",
        "basalt" | "smooth_basalt" | "polished_basalt" => return "basalt",
        "blackstone" => return "stone",
        "ancient_debris" => return "ancient_debris",
        "nether_bricks" | "cracked_nether_bricks" | "chiseled_nether_bricks" => {
            return "nether_bricks";
        }
        "crimson_nylium" | "warped_nylium" => return "nylium",
        "shroomlight" => return "shroomlight",
        "nether_wart_block" | "warped_wart_block" => return "wart_block",
        "netherite_block" => return "netherite_block",
        "lodestone" => return "lodestone",
        "moss_block" | "moss_carpet" | "pale_moss_block" | "pale_moss_carpet" => return "moss",
        "calcite" => return "calcite",
        "tuff" => return "tuff",
        "dripstone_block" => return "dripstone_block",
        "pointed_dripstone" => return "pointed_dripstone",
        "sculk" => return "sculk",
        "sculk_sensor" | "calibrated_sculk_sensor" => return "sculk_sensor",
        "sculk_catalyst" => return "sculk_catalyst",
        "sculk_shrieker" => return "sculk_shrieker",
        "sculk_vein" => return "sculk_vein",
        "mud_bricks" => return "mud_bricks",
        "packed_mud" => return "packed_mud",
        "muddy_mangrove_roots" => return "muddy_mangrove_roots",
        "mangrove_roots" => return "mangrove_roots",
        "hanging_roots" => return "hanging_roots",
        "big_dripleaf" => return "big_dripleaf",
        "small_dripleaf" => return "small_dripleaf",
        "spore_blossom" => return "spore_blossom",
        "azalea" | "flowering_azalea" => return "azalea",
        "azalea_leaves" | "flowering_azalea_leaves" => return "azalea_leaves",
        "cake" => return "wool",
        "lily_pad" => return "lily_pad",
        "podzol" | "mycelium" => return "gravel",
        "obsidian" | "crying_obsidian" | "bedrock" | "end_stone" | "end_stone_bricks" => {
            return "stone";
        }
        "hay_block" | "target" => return "grass",
        "pumpkin" | "carved_pumpkin" | "jack_o_lantern" | "melon" => return "wood",
        "sponge" | "wet_sponge" => return "grass",
        "cactus" => return "wool",
        "chorus_plant" | "chorus_flower" => return "wood",
        "ladder" => return "ladder",
        "vine" | "glow_lichen" | "twisting_vines" | "weeping_vines" | "cave_vines" => return "vine",
        "sea_pickle" => return "slime_block",
        "dried_kelp_block" => return "grass",
        "redstone_wire" | "repeater" | "comparator" | "lever" => return "stone",
        "tnt" => return "grass",
        "bookshelf" | "chiseled_bookshelf" | "lectern" | "composter" | "barrel" | "beehive"
        | "bee_nest" => return "wood",
        "smithing_table" | "fletching_table" | "cartography_table" | "loom" | "crafting_table"
        | "chest" | "trapped_chest" | "jukebox" | "note_block" => return "wood",
        "furnace" | "blast_furnace" | "smoker" | "dispenser" | "dropper" | "hopper"
        | "observer" | "piston" | "sticky_piston" | "piston_head" | "moving_piston" => {
            return "stone";
        }
        "enchanting_table" | "ender_chest" | "spawner" | "trial_spawner" | "vault"
        | "grindstone" | "stonecutter" | "bell" | "lantern" | "soul_lantern" | "cauldron"
        | "water_cauldron" | "lava_cauldron" | "powder_snow_cauldron" | "brewing_stand" => {
            return "metal";
        }
        "amethyst_block" | "budding_amethyst" => return "amethyst_block",
        "amethyst_cluster" | "large_amethyst_bud" | "medium_amethyst_bud"
        | "small_amethyst_bud" => return "amethyst_cluster",
        _ => {}
    }

    // --- suffix / substring heuristics -------------------------------------
    if n.ends_with("_wool") || n.ends_with("_carpet") {
        return "wool";
    }
    if n.contains("copper") {
        return "copper";
    }
    if n.starts_with("deepslate") || n.contains("_deepslate") {
        return "deepslate";
    }
    if n.contains("deepslate_bricks") || n.contains("deepslate_tiles") {
        return "deepslate_bricks";
    }
    if n.contains("cherry") {
        return "cherry_wood";
    }
    if n.contains("bamboo_") {
        return "bamboo_wood";
    }
    if n.contains("crimson") || n.contains("warped") {
        return "nether_wood";
    }
    if n.ends_with("_leaves") {
        return "grass";
    }
    if n.ends_with("_planks")
        || n.ends_with("_log")
        || n.ends_with("_wood")
        || n.ends_with("_slab") && (n.contains("oak") || n.contains("spruce") || n.contains("birch") || n.contains("jungle") || n.contains("acacia") || n.contains("mangrove"))
        || n.ends_with("_fence")
        || n.ends_with("_fence_gate")
        || n.ends_with("_door") && !n.contains("iron")
        || n.ends_with("_trapdoor") && !n.contains("iron")
        || n.ends_with("_sign")
        || n.ends_with("_button") && !n.contains("stone")
        || n.ends_with("_pressure_plate") && (n.contains("oak") || n.contains("spruce") || n.contains("birch") || n.contains("jungle") || n.contains("acacia") || n.contains("dark") || n.contains("mangrove"))
        || n.ends_with("_stem")
        || n.ends_with("_hyphae")
        || n.contains("stripped_")
    {
        return "wood";
    }
    if n.contains("iron_") || n.contains("gold_block") || n.contains("_gold_block") || n == "bell"
    {
        return "metal";
    }
    if n.ends_with("_ore") {
        return "stone";
    }
    if n.contains("coral") {
        return "coral_block";
    }
    if n.ends_with("_sapling")
        || n.ends_with("_flower")
        || n.contains("tulip")
        || n == "dandelion"
        || n == "poppy"
        || n == "short_grass"
        || n == "tall_grass"
        || n == "fern"
        || n == "large_fern"
        || n == "seagrass"
        || n == "tall_seagrass"
        || n == "sugar_cane"
        || n == "sweet_berry_bush"
        || n == "nether_wart"
        || n.ends_with("_mushroom")
        || n.contains("_crop")
        || n == "wheat"
        || n == "carrots"
        || n == "potatoes"
        || n == "beetroots"
        || n == "kelp"
        || n == "kelp_plant"
    {
        return "grass";
    }
    if n.ends_with("_candle") || n == "candle" {
        return "candle";
    }
    if n.contains("froglight") {
        return "froglight";
    }
    if n.ends_with("_terracotta") && n.starts_with("glazed") {
        return "stone";
    }
    if n.contains("shulker_box") {
        return "shulker_box";
    }
    if n.ends_with("_head") || n.ends_with("_skull") {
        return "bone_block";
    }
    if n.ends_with("_bed") {
        return "wood";
    }
    if n.contains("concrete_powder") {
        return "sand";
    }

    // Stone-ish default: bricks, concrete, terracotta, sandstone, prismarine…
    "stone"
}

#[cfg(test)]
mod tests {
    use super::group;

    #[test]
    fn common_groups() {
        assert_eq!(group("oak_planks"), "wood");
        assert_eq!(group("stone"), "stone");
        assert_eq!(group("dirt"), "gravel");
        assert_eq!(group("grass_block"), "grass");
        assert_eq!(group("white_wool"), "wool");
        assert_eq!(group("sand"), "sand");
        assert_eq!(group("glass"), "glass");
        assert_eq!(group("soul_sand"), "soul_sand");
        assert_eq!(group("deepslate"), "deepslate");
        assert_eq!(group("crimson_planks"), "nether_wood");
        assert_eq!(group("cherry_log"), "cherry_wood");
        assert_eq!(group("iron_block"), "metal");
        assert_eq!(group("diamond_ore"), "stone");
        assert_eq!(group("unknown_modded_block"), "stone");
    }
}
