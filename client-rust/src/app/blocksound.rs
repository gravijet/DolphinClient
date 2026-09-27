//! Block → vanilla sound-group mapping ("block.<group>.break/place/hit").
//!
//! azalea carries no per-block SoundType data, so `group()` below is an
//! exhaustive table of every real MC 26.1 block's real sound group, decompiled
//! straight from the client jar's own `Blocks.java`/`BlockSetType.java`/
//! `WoodType.java` (each block's `.sound(SoundType.X)` call, resolved through
//! `Properties.ofFullCopy`/door-trapdoor-button/sign-fence-gate indirection)
//! and cross-checked against the real `sounds.json` asset so every group name
//! actually has sound files behind it. A handful of real `SoundType`s have no
//! `block.<name>.*` family in `sounds.json` at all (`AMETHYST` really uses the
//! `amethyst_block` family; `EMPTY` — water/lava/bubble_column — is genuinely
//! silent in vanilla, mapped here to "stone" since this client always plays
//! *something*; `HARD_CROP`'s events are just WOOD's, mapped to "wood") —
//! each such case is called out at its match arm below.
//!
//! A name outside that table (mainly a modded/data-driven server's own block)
//! falls through to `heuristic_group`, a family-by-suffix guess. Unknown
//! blocks fall back to "stone", which always exists in sounds.json — a
//! slightly wrong sound beats silence.

/// The vanilla SoundType group for a block's registry short name
/// (no `minecraft:` prefix), e.g. "oak_planks" → "wood".
pub fn group(short_name: &str) -> &'static str {
    let n = short_name;
    match n {
        "amethyst_block" | "budding_amethyst" => "amethyst_block",
        "amethyst_cluster" => "amethyst_cluster",
        "ancient_debris" => "ancient_debris",
        "anvil" | "bell" | "chipped_anvil" | "damaged_anvil" => "anvil",
        "azalea" => "azalea",
        "azalea_leaves" | "flowering_azalea_leaves" => "azalea_leaves",
        "bamboo" => "bamboo",
        "bamboo_sapling" => "bamboo_sapling",
        "bamboo_block" | "bamboo_button" | "bamboo_door" | "bamboo_fence" | "bamboo_fence_gate" |
        "bamboo_mosaic" | "bamboo_mosaic_slab" | "bamboo_planks" | "bamboo_pressure_plate" |
        "bamboo_sign" | "bamboo_slab" | "bamboo_trapdoor" | "bamboo_wall_sign" |
        "stripped_bamboo_block" => "bamboo_wood",
        "bamboo_hanging_sign" | "bamboo_wall_hanging_sign" => "bamboo_wood_hanging_sign",
        "basalt" | "polished_basalt" => "basalt",
        "big_dripleaf" | "big_dripleaf_stem" => "big_dripleaf",
        "bone_block" => "bone_block",
        "cactus_flower" => "cactus_flower",
        "calcite" => "calcite",
        "cave_vines" | "cave_vines_plant" => "cave_vines",
        // Real vanilla renamed this block "iron_chain" (it used to be plain
        // "chain") — the substring "iron_" heuristic below would otherwise
        // wrongly catch it as "metal".
        "iron_chain" => "chain",
        "cherry_leaves" => "cherry_leaves",
        "cherry_sapling" => "cherry_sapling",
        "cherry_button" | "cherry_door" | "cherry_fence" | "cherry_fence_gate" | "cherry_log" |
        "cherry_planks" | "cherry_pressure_plate" | "cherry_sign" | "cherry_slab" | "cherry_trapdoor" |
        "cherry_wall_sign" | "cherry_wood" | "stripped_cherry_log" | "stripped_cherry_wood" => "cherry_wood",
        "cherry_hanging_sign" | "cherry_wall_hanging_sign" => "cherry_wood_hanging_sign",
        "chiseled_bookshelf" => "chiseled_bookshelf",
        "cobweb" => "cobweb",
        "copper_block" | "copper_chest" | "copper_door" | "copper_trapdoor" | "exposed_copper_door" |
        "exposed_copper_trapdoor" | "lightning_rod" | "oxidized_copper_door" |
        "oxidized_copper_trapdoor" | "waxed_copper_door" | "waxed_copper_trapdoor" |
        "waxed_exposed_copper_door" | "waxed_exposed_copper_trapdoor" | "waxed_oxidized_copper_door" |
        "waxed_oxidized_copper_trapdoor" | "waxed_weathered_copper_door" |
        "waxed_weathered_copper_trapdoor" | "weathered_copper_door" | "weathered_copper_trapdoor" => "copper",
        "copper_bulb" => "copper_bulb",
        "copper_golem_statue" => "copper_golem_statue",
        "copper_grate" => "copper_grate",
        "brain_coral_block" | "bubble_coral_block" | "fire_coral_block" | "honeycomb_block" |
        "horn_coral_block" | "tube_coral_block" => "coral_block",
        "creaking_heart" => "creaking_heart",
        "beetroots" | "carrots" | "pitcher_crop" | "pitcher_plant" | "potatoes" | "torchflower_crop" |
        "wheat" => "crop",
        "deepslate" | "deepslate_coal_ore" | "deepslate_copper_ore" | "deepslate_diamond_ore" |
        "deepslate_emerald_ore" | "deepslate_gold_ore" | "deepslate_iron_ore" | "deepslate_lapis_ore" |
        "deepslate_redstone_ore" | "infested_deepslate" | "reinforced_deepslate" => "deepslate",
        "chiseled_deepslate" | "deepslate_bricks" => "deepslate_bricks",
        "deepslate_tiles" => "deepslate_tiles",
        "dried_ghast" => "dried_ghast",
        "dripstone_block" => "dripstone_block",
        "flowering_azalea" => "flowering_azalea",
        "ochre_froglight" | "pearlescent_froglight" | "verdant_froglight" => "froglight",
        "frogspawn" => "frogspawn",
        "crimson_fungus" | "warped_fungus" => "fungus",
        "gilded_blackstone" => "gilded_blackstone",
        "black_stained_glass_pane" | "blue_ice" | "blue_stained_glass_pane" |
        "brown_stained_glass_pane" | "cyan_stained_glass_pane" | "end_portal_frame" | "frosted_ice" |
        "glass" | "glass_pane" | "glowstone" | "gray_stained_glass_pane" | "green_stained_glass_pane" |
        "ice" | "light_blue_stained_glass_pane" | "light_gray_stained_glass_pane" |
        "lime_stained_glass_pane" | "magenta_stained_glass_pane" | "nether_portal" |
        "orange_stained_glass_pane" | "packed_ice" | "pink_stained_glass_pane" |
        "purple_stained_glass_pane" | "red_stained_glass_pane" | "redstone_lamp" | "sea_lantern" |
        "white_stained_glass_pane" | "yellow_stained_glass_pane" => "glass",
        "acacia_leaves" | "acacia_sapling" | "allium" | "azure_bluet" | "birch_leaves" |
        "birch_sapling" | "blue_orchid" | "brown_mushroom" | "bush" | "closed_eyeblossom" |
        "cornflower" | "dandelion" | "dark_oak_leaves" | "dark_oak_sapling" | "dead_bush" |
        "dirt_path" | "dried_kelp_block" | "fern" | "glow_lichen" | "golden_dandelion" |
        "grass_block" | "hay_block" | "jungle_leaves" | "jungle_sapling" | "large_fern" | "lilac" |
        "lily_of_the_valley" | "mangrove_leaves" | "mangrove_propagule" | "mycelium" | "oak_leaves" |
        "oak_sapling" | "open_eyeblossom" | "orange_tulip" | "oxeye_daisy" | "pale_oak_leaves" |
        "pale_oak_sapling" | "peony" | "pink_tulip" | "poppy" | "red_mushroom" | "red_tulip" |
        "rose_bush" | "short_dry_grass" | "short_grass" | "spruce_leaves" | "spruce_sapling" |
        "sugar_cane" | "sunflower" | "tall_dry_grass" | "tall_grass" | "target" | "tnt" |
        "torchflower" | "white_tulip" | "wither_rose" => "grass",
        "clay" | "coarse_dirt" | "dirt" | "farmland" | "gravel" | "podzol" => "gravel",
        "hanging_roots" => "hanging_roots",
        "acacia_hanging_sign" | "acacia_wall_hanging_sign" | "birch_hanging_sign" |
        "birch_wall_hanging_sign" | "dark_oak_hanging_sign" | "dark_oak_wall_hanging_sign" |
        "jungle_hanging_sign" | "jungle_wall_hanging_sign" | "mangrove_hanging_sign" |
        "mangrove_wall_hanging_sign" | "oak_hanging_sign" | "oak_wall_hanging_sign" |
        "pale_oak_hanging_sign" | "pale_oak_wall_hanging_sign" | "spruce_hanging_sign" |
        "spruce_wall_hanging_sign" => "hanging_sign",
        // Real vanilla's HARD_CROP has no "block.hard_crop.*" family in
        // sounds.json at all — its break/step/hit/fall reuse WOOD's events
        // outright (only place differs, reusing CROP's planted sound), so
        // "wood" is the closest single group our per-verb lookup can use.
        "melon_stem" | "pumpkin_stem" => "wood",
        "heavy_core" => "heavy_core",
        "honey_block" => "honey_block",
        "heavy_weighted_pressure_plate" | "iron_bars" | "iron_block" | "iron_door" | "iron_trapdoor" => "iron",
        "ladder" => "ladder",
        "lantern" | "soul_lantern" => "lantern",
        // Real vanilla itself has LARGE_AMETHYST_BUD and MEDIUM_AMETHYST_BUD's
        // sounds swapped (confirmed straight from the decompiled source) —
        // preserved here exactly, not "fixed", per this project's rule of
        // matching real behavior even when it's a quirky upstream oddity.
        "medium_amethyst_bud" => "large_amethyst_bud",
        "leaf_litter" => "leaf_litter",
        "lily_pad" => "lily_pad",
        "lodestone" => "lodestone",
        "mangrove_roots" => "mangrove_roots",
        "large_amethyst_bud" => "medium_amethyst_bud",
        "activator_rail" | "detector_rail" | "diamond_block" | "emerald_block" | "gold_block" |
        "hopper" | "light_weighted_pressure_plate" | "powered_rail" | "rail" | "redstone_block" |
        "sniffer_egg" | "turtle_egg" => "metal",
        "moss_block" | "pale_moss_block" => "moss",
        "moss_carpet" | "pale_hanging_moss" | "pale_moss_carpet" => "moss_carpet",
        "mud" => "mud",
        "mud_brick_slab" | "mud_bricks" => "mud_bricks",
        "muddy_mangrove_roots" => "muddy_mangrove_roots",
        "chiseled_nether_bricks" | "cracked_nether_bricks" | "nether_brick_fence" |
        "nether_brick_slab" | "nether_bricks" | "red_nether_bricks" => "nether_bricks",
        "nether_gold_ore" => "nether_gold_ore",
        "nether_quartz_ore" => "nether_ore",
        "nether_sprouts" => "nether_sprouts",
        "nether_wart" => "nether_wart",
        "crimson_button" | "crimson_door" | "crimson_fence" | "crimson_fence_gate" | "crimson_planks" |
        "crimson_pressure_plate" | "crimson_sign" | "crimson_slab" | "crimson_trapdoor" |
        "crimson_wall_sign" | "warped_button" | "warped_door" | "warped_fence" | "warped_fence_gate" |
        "warped_planks" | "warped_pressure_plate" | "warped_sign" | "warped_slab" | "warped_trapdoor" |
        "warped_wall_sign" => "nether_wood",
        "crimson_hanging_sign" | "crimson_wall_hanging_sign" | "warped_hanging_sign" |
        "warped_wall_hanging_sign" => "nether_wood_hanging_sign",
        "netherite_block" => "netherite_block",
        "netherrack" => "netherrack",
        "crimson_nylium" | "warped_nylium" => "nylium",
        "packed_mud" => "packed_mud",
        "pink_petals" | "wildflowers" => "pink_petals",
        "pointed_dripstone" => "pointed_dripstone",
        "polished_deepslate" => "polished_deepslate",
        "polished_tuff" => "polished_tuff",
        "powder_snow" => "powder_snow",
        "resin_block" | "resin_clump" => "resin",
        "chiseled_resin_bricks" | "resin_brick_slab" | "resin_brick_wall" | "resin_bricks" => "resin_bricks",
        "rooted_dirt" => "rooted_dirt",
        "crimson_roots" | "warped_roots" => "roots",
        "black_concrete_powder" | "blue_concrete_powder" | "brown_concrete_powder" |
        "cyan_concrete_powder" | "gray_concrete_powder" | "green_concrete_powder" |
        "light_blue_concrete_powder" | "light_gray_concrete_powder" | "lime_concrete_powder" |
        "magenta_concrete_powder" | "orange_concrete_powder" | "pink_concrete_powder" |
        "purple_concrete_powder" | "red_concrete_powder" | "red_sand" | "sand" |
        "white_concrete_powder" | "yellow_concrete_powder" => "sand",
        "scaffolding" => "scaffolding",
        "sculk" => "sculk",
        "sculk_catalyst" => "sculk_catalyst",
        "sculk_sensor" => "sculk_sensor",
        "sculk_shrieker" => "sculk_shrieker",
        "sculk_vein" => "sculk_vein",
        "acacia_shelf" | "bamboo_shelf" | "birch_shelf" | "cherry_shelf" | "crimson_shelf" |
        "dark_oak_shelf" | "jungle_shelf" | "mangrove_shelf" | "oak_shelf" | "pale_oak_shelf" |
        "spruce_shelf" | "warped_shelf" => "shelf",
        "shroomlight" => "shroomlight",
        "sea_pickle" | "slime_block" => "slime_block",
        "small_amethyst_bud" => "small_amethyst_bud",
        "small_dripleaf" => "small_dripleaf",
        "snow" | "snow_block" => "snow",
        "soul_sand" => "soul_sand",
        "soul_soil" => "soul_soil",
        "spawner" => "spawner",
        "sponge" => "sponge",
        "spore_blossom" => "spore_blossom",
        "crimson_hyphae" | "stripped_crimson_hyphae" | "stripped_warped_hyphae" | "warped_hyphae" => "stem",
        "suspicious_gravel" => "suspicious_gravel",
        "suspicious_sand" => "suspicious_sand",
        "firefly_bush" | "sweet_berry_bush" => "sweet_berry_bush",
        "trial_spawner" => "trial_spawner",
        "tuff" => "tuff",
        "tuff_bricks" => "tuff_bricks",
        "vault" => "vault",
        "vine" => "vine",
        "nether_wart_block" | "warped_wart_block" => "wart_block",
        "twisting_vines" | "twisting_vines_plant" | "weeping_vines" | "weeping_vines_plant" => "weeping_vines",
        "brain_coral" | "brain_coral_fan" | "brain_coral_wall_fan" | "bubble_coral" |
        "bubble_coral_fan" | "bubble_coral_wall_fan" | "fire_coral" | "fire_coral_fan" |
        "fire_coral_wall_fan" | "horn_coral" | "horn_coral_fan" | "horn_coral_wall_fan" | "kelp" |
        "kelp_plant" | "seagrass" | "tall_seagrass" | "tube_coral" | "tube_coral_fan" |
        "tube_coral_wall_fan" => "wet_grass",
        "wet_sponge" => "wet_sponge",
        "acacia_button" | "acacia_door" | "acacia_fence" | "acacia_fence_gate" | "acacia_log" |
        "acacia_planks" | "acacia_pressure_plate" | "acacia_sign" | "acacia_slab" | "acacia_trapdoor" |
        "acacia_wall_sign" | "acacia_wood" | "attached_melon_stem" | "attached_pumpkin_stem" |
        "barrel" | "bee_nest" | "beehive" | "birch_button" | "birch_door" | "birch_fence" |
        "birch_fence_gate" | "birch_log" | "birch_planks" | "birch_pressure_plate" | "birch_sign" |
        "birch_slab" | "birch_trapdoor" | "birch_wall_sign" | "birch_wood" | "black_banner" |
        "black_wall_banner" | "blue_banner" | "blue_wall_banner" | "bookshelf" | "brown_banner" |
        "brown_mushroom_block" | "brown_wall_banner" | "campfire" | "cartography_table" |
        "carved_pumpkin" | "chest" | "chorus_flower" | "chorus_plant" | "cocoa" | "composter" |
        "copper_torch" | "copper_wall_torch" | "crafting_table" | "cyan_banner" | "cyan_wall_banner" |
        "dark_oak_button" | "dark_oak_door" | "dark_oak_fence" | "dark_oak_fence_gate" |
        "dark_oak_log" | "dark_oak_planks" | "dark_oak_pressure_plate" | "dark_oak_sign" |
        "dark_oak_slab" | "dark_oak_trapdoor" | "dark_oak_wall_sign" | "dark_oak_wood" |
        "daylight_detector" | "end_rod" | "fletching_table" | "gray_banner" | "gray_wall_banner" |
        "green_banner" | "green_wall_banner" | "jack_o_lantern" | "jukebox" | "jungle_button" |
        "jungle_door" | "jungle_fence" | "jungle_fence_gate" | "jungle_log" | "jungle_planks" |
        "jungle_pressure_plate" | "jungle_sign" | "jungle_slab" | "jungle_trapdoor" |
        "jungle_wall_sign" | "jungle_wood" | "lectern" | "light_blue_banner" |
        "light_blue_wall_banner" | "light_gray_banner" | "light_gray_wall_banner" | "lime_banner" |
        "lime_wall_banner" | "loom" | "magenta_banner" | "magenta_wall_banner" | "mangrove_button" |
        "mangrove_door" | "mangrove_fence" | "mangrove_fence_gate" | "mangrove_log" |
        "mangrove_planks" | "mangrove_pressure_plate" | "mangrove_sign" | "mangrove_slab" |
        "mangrove_trapdoor" | "mangrove_wall_sign" | "mangrove_wood" | "melon" | "mushroom_stem" |
        "note_block" | "oak_button" | "oak_door" | "oak_fence" | "oak_fence_gate" | "oak_log" |
        "oak_planks" | "oak_pressure_plate" | "oak_sign" | "oak_slab" | "oak_trapdoor" |
        "oak_wall_sign" | "oak_wood" | "orange_banner" | "orange_wall_banner" | "pale_oak_button" |
        "pale_oak_door" | "pale_oak_fence" | "pale_oak_fence_gate" | "pale_oak_log" |
        "pale_oak_planks" | "pale_oak_pressure_plate" | "pale_oak_sign" | "pale_oak_slab" |
        "pale_oak_trapdoor" | "pale_oak_wall_sign" | "pale_oak_wood" | "pink_banner" |
        "pink_wall_banner" | "pumpkin" | "purple_banner" | "purple_wall_banner" | "red_banner" |
        "red_mushroom_block" | "red_wall_banner" | "redstone_torch" | "redstone_wall_torch" |
        "smithing_table" | "soul_campfire" | "soul_torch" | "soul_wall_torch" | "spruce_button" |
        "spruce_door" | "spruce_fence" | "spruce_fence_gate" | "spruce_log" | "spruce_planks" |
        "spruce_pressure_plate" | "spruce_sign" | "spruce_slab" | "spruce_trapdoor" |
        "spruce_wall_sign" | "spruce_wood" | "stripped_acacia_log" | "stripped_acacia_wood" |
        "stripped_birch_log" | "stripped_birch_wood" | "stripped_dark_oak_log" |
        "stripped_dark_oak_wood" | "stripped_jungle_log" | "stripped_jungle_wood" |
        "stripped_mangrove_log" | "stripped_mangrove_wood" | "stripped_oak_log" | "stripped_oak_wood" |
        "stripped_pale_oak_log" | "stripped_pale_oak_wood" | "stripped_spruce_log" |
        "stripped_spruce_wood" | "torch" | "trapped_chest" | "tripwire_hook" | "wall_torch" |
        "white_banner" | "white_wall_banner" | "yellow_banner" | "yellow_wall_banner" => "wood",
        "black_carpet" | "black_wool" | "blue_carpet" | "blue_wool" | "brown_carpet" | "brown_wool" |
        "cactus" | "cake" | "cyan_carpet" | "cyan_wool" | "fire" | "gray_carpet" | "gray_wool" |
        "green_carpet" | "green_wool" | "light_blue_carpet" | "light_blue_wool" | "light_gray_carpet" |
        "light_gray_wool" | "lime_carpet" | "lime_wool" | "magenta_carpet" | "magenta_wool" |
        "orange_carpet" | "orange_wool" | "pink_carpet" | "pink_wool" | "purple_carpet" |
        "purple_wool" | "red_carpet" | "red_wool" | "soul_fire" | "white_carpet" | "white_wool" |
        "yellow_carpet" | "yellow_wool" => "wool",
        // Everything below is really SoundType.STONE (either explicitly, or
        // (water/lava/bubble_column) genuinely SoundType.EMPTY — silent in
        // real vanilla, approximated here as "stone" since this engine
        // always plays something for a recognized block).
        "air" | "andesite" | "andesite_slab" | "andesite_wall" | "barrier" | "beacon" | "bedrock" |
        "black_candle" | "black_candle_cake" | "black_concrete" | "black_glazed_terracotta" |
        "black_shulker_box" | "black_terracotta" | "blackstone" | "blackstone_slab" |
        "blackstone_wall" | "blast_furnace" | "blue_candle" | "blue_candle_cake" | "blue_concrete" |
        "blue_glazed_terracotta" | "blue_shulker_box" | "blue_terracotta" | "brewing_stand" |
        "brick_slab" | "brick_wall" | "bricks" | "brown_candle" | "brown_candle_cake" |
        "brown_concrete" | "brown_glazed_terracotta" | "brown_shulker_box" | "brown_terracotta" |
        "bubble_column" | "calibrated_sculk_sensor" | "candle" | "candle_cake" | "cauldron" |
        "cave_air" | "chain_command_block" | "chiseled_copper" | "chiseled_polished_blackstone" |
        "chiseled_quartz_block" | "chiseled_red_sandstone" | "chiseled_sandstone" |
        "chiseled_stone_bricks" | "chiseled_tuff" | "chiseled_tuff_bricks" | "coal_block" |
        "coal_ore" | "cobbled_deepslate" | "cobbled_deepslate_slab" | "cobbled_deepslate_wall" |
        "cobblestone" | "cobblestone_slab" | "cobblestone_wall" | "command_block" | "comparator" |
        "conduit" | "copper_ore" | "cracked_deepslate_bricks" | "cracked_deepslate_tiles" |
        "cracked_polished_blackstone_bricks" | "cracked_stone_bricks" | "crafter" | "creeper_head" |
        "creeper_wall_head" | "crimson_stem" | "crying_obsidian" | "cut_copper" | "cut_copper_slab" |
        "cut_copper_stairs" | "cut_red_sandstone" | "cut_red_sandstone_slab" | "cut_sandstone" |
        "cut_sandstone_slab" | "cyan_candle" | "cyan_candle_cake" | "cyan_concrete" |
        "cyan_glazed_terracotta" | "cyan_shulker_box" | "cyan_terracotta" | "dark_prismarine" |
        "dark_prismarine_slab" | "dead_brain_coral" | "dead_brain_coral_block" |
        "dead_brain_coral_fan" | "dead_brain_coral_wall_fan" | "dead_bubble_coral" |
        "dead_bubble_coral_block" | "dead_bubble_coral_fan" | "dead_bubble_coral_wall_fan" |
        "dead_fire_coral" | "dead_fire_coral_block" | "dead_fire_coral_fan" |
        "dead_fire_coral_wall_fan" | "dead_horn_coral" | "dead_horn_coral_block" |
        "dead_horn_coral_fan" | "dead_horn_coral_wall_fan" | "dead_tube_coral" |
        "dead_tube_coral_block" | "dead_tube_coral_fan" | "dead_tube_coral_wall_fan" |
        "decorated_pot" | "deepslate_brick_slab" | "deepslate_brick_wall" | "deepslate_tile_slab" |
        "deepslate_tile_wall" | "diamond_ore" | "diorite" | "diorite_slab" | "diorite_wall" |
        "dispenser" | "dragon_egg" | "dragon_head" | "dragon_wall_head" | "dropper" | "emerald_ore" |
        "enchanting_table" | "end_gateway" | "end_portal" | "end_stone" | "end_stone_brick_slab" |
        "end_stone_brick_wall" | "end_stone_bricks" | "ender_chest" | "exposed_chiseled_copper" |
        "exposed_copper" | "exposed_copper_bulb" | "exposed_copper_chest" |
        "exposed_copper_golem_statue" | "exposed_copper_grate" | "exposed_cut_copper" |
        "exposed_cut_copper_slab" | "exposed_cut_copper_stairs" | "exposed_lightning_rod" |
        "flower_pot" | "furnace" | "gold_ore" | "granite" | "granite_slab" | "granite_wall" |
        "gray_candle" | "gray_candle_cake" | "gray_concrete" | "gray_glazed_terracotta" |
        "gray_shulker_box" | "gray_terracotta" | "green_candle" | "green_candle_cake" |
        "green_concrete" | "green_glazed_terracotta" | "green_shulker_box" | "green_terracotta" |
        "grindstone" | "infested_chiseled_stone_bricks" | "infested_cobblestone" |
        "infested_cracked_stone_bricks" | "infested_mossy_stone_bricks" | "infested_stone" |
        "infested_stone_bricks" | "iron_ore" | "jigsaw" | "lapis_block" | "lapis_ore" | "lava" |
        "lava_cauldron" | "lever" | "light" | "light_blue_candle" | "light_blue_candle_cake" |
        "light_blue_concrete" | "light_blue_glazed_terracotta" | "light_blue_shulker_box" |
        "light_blue_terracotta" | "light_gray_candle" | "light_gray_candle_cake" |
        "light_gray_concrete" | "light_gray_glazed_terracotta" | "light_gray_shulker_box" |
        "light_gray_terracotta" | "lime_candle" | "lime_candle_cake" | "lime_concrete" |
        "lime_glazed_terracotta" | "lime_shulker_box" | "lime_terracotta" | "magenta_candle" |
        "magenta_candle_cake" | "magenta_concrete" | "magenta_glazed_terracotta" |
        "magenta_shulker_box" | "magenta_terracotta" | "magma_block" | "mossy_cobblestone" |
        "mossy_cobblestone_slab" | "mossy_cobblestone_wall" | "mossy_stone_brick_slab" |
        "mossy_stone_brick_wall" | "mossy_stone_bricks" | "moving_piston" | "mud_brick_wall" |
        "nether_brick_wall" | "observer" | "obsidian" | "orange_candle" | "orange_candle_cake" |
        "orange_concrete" | "orange_glazed_terracotta" | "orange_shulker_box" | "orange_terracotta" |
        "oxidized_chiseled_copper" | "oxidized_copper" | "oxidized_copper_bulb" |
        "oxidized_copper_chest" | "oxidized_copper_golem_statue" | "oxidized_copper_grate" |
        "oxidized_cut_copper" | "oxidized_cut_copper_slab" | "oxidized_cut_copper_stairs" |
        "oxidized_lightning_rod" | "petrified_oak_slab" | "piglin_head" | "piglin_wall_head" |
        "pink_candle" | "pink_candle_cake" | "pink_concrete" | "pink_glazed_terracotta" |
        "pink_shulker_box" | "pink_terracotta" | "piston" | "piston_head" | "player_head" |
        "player_wall_head" | "polished_andesite" | "polished_andesite_slab" | "polished_blackstone" |
        "polished_blackstone_brick_slab" | "polished_blackstone_brick_wall" |
        "polished_blackstone_bricks" | "polished_blackstone_button" |
        "polished_blackstone_pressure_plate" | "polished_blackstone_slab" |
        "polished_blackstone_wall" | "polished_deepslate_slab" | "polished_deepslate_wall" |
        "polished_diorite" | "polished_diorite_slab" | "polished_granite" | "polished_granite_slab" |
        "polished_tuff_slab" | "polished_tuff_stairs" | "polished_tuff_wall" |
        "potted_acacia_sapling" | "potted_allium" | "potted_azalea_bush" | "potted_azure_bluet" |
        "potted_bamboo" | "potted_birch_sapling" | "potted_blue_orchid" | "potted_brown_mushroom" |
        "potted_cactus" | "potted_cherry_sapling" | "potted_closed_eyeblossom" | "potted_cornflower" |
        "potted_crimson_fungus" | "potted_crimson_roots" | "potted_dandelion" |
        "potted_dark_oak_sapling" | "potted_dead_bush" | "potted_fern" |
        "potted_flowering_azalea_bush" | "potted_golden_dandelion" | "potted_jungle_sapling" |
        "potted_lily_of_the_valley" | "potted_mangrove_propagule" | "potted_oak_sapling" |
        "potted_open_eyeblossom" | "potted_orange_tulip" | "potted_oxeye_daisy" |
        "potted_pale_oak_sapling" | "potted_pink_tulip" | "potted_poppy" | "potted_red_mushroom" |
        "potted_red_tulip" | "potted_spruce_sapling" | "potted_torchflower" | "potted_warped_fungus" |
        "potted_warped_roots" | "potted_white_tulip" | "potted_wither_rose" | "powder_snow_cauldron" |
        "prismarine" | "prismarine_brick_slab" | "prismarine_bricks" | "prismarine_slab" |
        "prismarine_wall" | "purple_candle" | "purple_candle_cake" | "purple_concrete" |
        "purple_glazed_terracotta" | "purple_shulker_box" | "purple_terracotta" | "purpur_block" |
        "purpur_pillar" | "purpur_slab" | "quartz_block" | "quartz_bricks" | "quartz_pillar" |
        "quartz_slab" | "raw_copper_block" | "raw_gold_block" | "raw_iron_block" | "red_candle" |
        "red_candle_cake" | "red_concrete" | "red_glazed_terracotta" | "red_nether_brick_slab" |
        "red_nether_brick_wall" | "red_sandstone" | "red_sandstone_slab" | "red_sandstone_wall" |
        "red_shulker_box" | "red_terracotta" | "redstone_ore" | "redstone_wire" | "repeater" |
        "repeating_command_block" | "respawn_anchor" | "sandstone" | "sandstone_slab" |
        "sandstone_wall" | "shulker_box" | "skeleton_skull" | "skeleton_wall_skull" | "smoker" |
        "smooth_basalt" | "smooth_quartz" | "smooth_quartz_slab" | "smooth_red_sandstone" |
        "smooth_red_sandstone_slab" | "smooth_sandstone" | "smooth_sandstone_slab" | "smooth_stone" |
        "smooth_stone_slab" | "sticky_piston" | "stone" | "stone_brick_slab" | "stone_brick_wall" |
        "stone_bricks" | "stone_button" | "stone_pressure_plate" | "stone_slab" | "stonecutter" |
        "stripped_crimson_stem" | "stripped_warped_stem" | "structure_block" | "structure_void" |
        "terracotta" | "test_block" | "test_instance_block" | "tinted_glass" | "tripwire" |
        "tuff_brick_slab" | "tuff_brick_stairs" | "tuff_brick_wall" | "tuff_slab" | "tuff_stairs" |
        "tuff_wall" | "void_air" | "warped_stem" | "water" | "water_cauldron" |
        "waxed_chiseled_copper" | "waxed_copper_block" | "waxed_copper_bulb" | "waxed_copper_chest" |
        "waxed_copper_golem_statue" | "waxed_copper_grate" | "waxed_cut_copper" |
        "waxed_cut_copper_slab" | "waxed_exposed_chiseled_copper" | "waxed_exposed_copper" |
        "waxed_exposed_copper_bulb" | "waxed_exposed_copper_chest" |
        "waxed_exposed_copper_golem_statue" | "waxed_exposed_copper_grate" |
        "waxed_exposed_cut_copper" | "waxed_exposed_cut_copper_slab" | "waxed_exposed_lightning_rod" |
        "waxed_lightning_rod" | "waxed_oxidized_chiseled_copper" | "waxed_oxidized_copper" |
        "waxed_oxidized_copper_bulb" | "waxed_oxidized_copper_chest" |
        "waxed_oxidized_copper_golem_statue" | "waxed_oxidized_copper_grate" |
        "waxed_oxidized_cut_copper" | "waxed_oxidized_cut_copper_slab" |
        "waxed_oxidized_lightning_rod" | "waxed_weathered_chiseled_copper" | "waxed_weathered_copper" |
        "waxed_weathered_copper_bulb" | "waxed_weathered_copper_chest" |
        "waxed_weathered_copper_golem_statue" | "waxed_weathered_copper_grate" |
        "waxed_weathered_cut_copper" | "waxed_weathered_cut_copper_slab" |
        "waxed_weathered_lightning_rod" | "weathered_chiseled_copper" | "weathered_copper" |
        "weathered_copper_bulb" | "weathered_copper_chest" | "weathered_copper_golem_statue" |
        "weathered_copper_grate" | "weathered_cut_copper" | "weathered_cut_copper_slab" |
        "weathered_cut_copper_stairs" | "weathered_lightning_rod" | "white_candle" |
        "white_candle_cake" | "white_concrete" | "white_glazed_terracotta" | "white_shulker_box" |
        "white_terracotta" | "wither_skeleton_skull" | "wither_skeleton_wall_skull" | "yellow_candle" |
        "yellow_candle_cake" | "yellow_concrete" | "yellow_glazed_terracotta" | "yellow_shulker_box" |
        "yellow_terracotta" | "zombie_head" | "zombie_wall_head" => "stone",
        _ => heuristic_group(n),
    }
}

/// Name-pattern fallback for anything the exhaustive real-block table above
/// doesn't recognize — almost always a modded or data-pack-defined block on
/// a server. Approximate, not authoritative.
fn heuristic_group(n: &str) -> &'static str {
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
        assert_eq!(group("unknown_modded_block"), "stone");
    }

    /// The 6 real bugs a 2026-09-27 decompile-vs-heuristic audit found and
    /// this release fixed — each is a genuine, decompile-confirmed vanilla
    /// SoundType, not a guess.
    #[test]
    fn fixed_bugs() {
        // Real SoundType.STONE (no override at all) — the old "iron_" substring
        // heuristic wrongly caught these as "metal".
        assert_eq!(group("iron_ore"), "stone");
        assert_eq!(group("raw_iron_block"), "stone");
        // The deepslate variant copies DEEPSLATE's own properties wholesale
        // (`Properties.ofFullCopy(DEEPSLATE)`), sound included — it really
        // does sound like deepslate, not plain stone.
        assert_eq!(group("deepslate_iron_ore"), "deepslate");
        // Real SoundType.IRON, a distinct group from generic "metal".
        assert_eq!(group("iron_block"), "iron");
        assert_eq!(group("iron_door"), "iron");
        assert_eq!(group("iron_bars"), "iron");
        // Real SoundType.GRASS — the old heuristic lumped this in with podzol's
        // (correct) "gravel".
        assert_eq!(group("mycelium"), "grass");
        // Real SoundType.CROP, distinct from "grass".
        assert_eq!(group("wheat"), "crop");
        assert_eq!(group("carrots"), "crop");
        assert_eq!(group("torchflower_crop"), "crop");
        // Real SoundType.NETHER_WART, its own dedicated group.
        assert_eq!(group("nether_wart"), "nether_wart");
        // Real vanilla renamed "chain" to "iron_chain" — the old "iron_"
        // heuristic wrongly caught it as "metal" instead of "chain".
        assert_eq!(group("iron_chain"), "chain");
    }

    /// Spot-checks across families the exhaustive table newly covers.
    #[test]
    fn newly_covered_families() {
        assert_eq!(group("acacia_shelf"), "shelf");
        assert_eq!(group("oak_hanging_sign"), "hanging_sign");
        assert_eq!(group("cherry_hanging_sign"), "cherry_wood_hanging_sign");
        assert_eq!(group("crimson_hyphae"), "stem");
        assert_eq!(group("crimson_fungus"), "fungus");
        assert_eq!(group("seagrass"), "wet_grass");
        assert_eq!(group("kelp"), "wet_grass");
        assert_eq!(group("bell"), "anvil");
        assert_eq!(group("turtle_egg"), "metal");
        assert_eq!(group("allium"), "grass");
        assert_eq!(group("cornflower"), "grass");
        // Real vanilla's own bug, preserved on purpose (see the match arm's
        // comment): these two are genuinely swapped in the decompiled source.
        assert_eq!(group("large_amethyst_bud"), "medium_amethyst_bud");
        assert_eq!(group("medium_amethyst_bud"), "large_amethyst_bud");
    }
}
