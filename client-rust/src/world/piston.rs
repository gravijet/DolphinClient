//! Vanilla's `PistonStructureResolver`, client-side.
//!
//! The server never tells us which blocks a piston is about to move. It sends
//! one `ClientboundBlockEvent` ("piston at P fired, facing D") and then the
//! *result* two ticks later; the blocks in between are moved by both sides
//! independently, from the same rules. This module is our copy of those rules:
//! given the world as we know it, which blocks travel, in which order, and
//! which ones simply break.
//!
//! It is a pure function of (position, facing, direction of travel, a block
//! lookup), so it is unit-tested against hand-built worlds — no server, no
//! renderer.

use crate::assets::blockmap::BlockTable;
use crate::types::{BlockPos, Face, StateId};

/// Vanilla's `PushReaction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    /// Travels with the piston.
    Normal,
    /// Breaks instead of moving (torches, plants, redstone wiring, fluids).
    Destroy,
    /// Refuses to move at all, which stalls the whole piston.
    Block,
    /// Can be pushed but never pulled (the glazed terracottas).
    PushOnly,
}

/// The most a piston will ever move, vanilla's limit.
pub const PUSH_LIMIT: usize = 12;

/// What one piston firing does to the world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PistonMove {
    /// Blocks that travel one cell along the push direction, in the order
    /// vanilla moves them (farthest from the piston first).
    pub push: Vec<BlockPos>,
    /// Blocks that break rather than travel.
    pub destroy: Vec<BlockPos>,
}

impl PistonMove {
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.push.is_empty() && self.destroy.is_empty()
    }
}

/// The two blocks that make a structure hold together.
fn is_sticky(name: &str) -> bool {
    name == "slime_block" || name == "honey_block"
}

/// Vanilla's `canStickToEachOther`: slime and honey are the only sticky blocks
/// and — famously — they refuse to stick to *each other*.
fn stick_to_each_other(a: &str, b: &str) -> bool {
    if (a == "honey_block" && b == "slime_block") || (a == "slime_block" && b == "honey_block") {
        return false;
    }
    is_sticky(a) || is_sticky(b)
}

/// Blocks that carry a block entity. Vanilla refuses to push any of them
/// (`isPushable` ends in `!state.hasBlockEntity()`), which is why a chest or a
/// furnace stops a piston dead.
pub fn has_block_entity(n: &str) -> bool {
    matches!(
        n,
        "chest"
            | "trapped_chest"
            | "ender_chest"
            | "barrel"
            | "furnace"
            | "blast_furnace"
            | "smoker"
            | "dispenser"
            | "dropper"
            | "hopper"
            | "brewing_stand"
            | "enchanting_table"
            | "beacon"
            | "spawner"
            | "trial_spawner"
            | "vault"
            | "jukebox"
            | "lectern"
            | "bell"
            | "campfire"
            | "soul_campfire"
            | "conduit"
            | "daylight_detector"
            | "comparator"
            | "sculk_sensor"
            | "calibrated_sculk_sensor"
            | "sculk_catalyst"
            | "sculk_shrieker"
            | "decorated_pot"
            | "chiseled_bookshelf"
            | "crafter"
            | "command_block"
            | "chain_command_block"
            | "repeating_command_block"
            | "structure_block"
            | "jigsaw"
            | "end_gateway"
            | "end_portal"
            | "moving_piston"
            | "suspicious_sand"
            | "suspicious_gravel"
            | "creaking_heart"
    ) || n.ends_with("_sign")
        || n.ends_with("_hanging_sign")
        || n.ends_with("_bed")
        || n.ends_with("shulker_box")
        || n.ends_with("_head")
        || n.ends_with("_skull")
        || n.ends_with("_banner")
        || n.starts_with("copper_chest")
        || (n.starts_with("waxed_") && n.ends_with("_chest"))
        || (n.ends_with("_copper_chest"))
}

/// A block's push reaction, by registry short name. Curated from vanilla's
/// block properties, with suffix rules for the families that share one
/// (planted things break, glazed terracotta only ever pushes).
pub fn push_reaction(n: &str) -> Reaction {
    // --- never moves ------------------------------------------------------
    if matches!(
        n,
        "obsidian"
            | "crying_obsidian"
            | "respawn_anchor"
            | "reinforced_deepslate"
            | "bedrock"
            | "barrier"
            | "light"
            | "structure_void"
            | "end_portal_frame"
            | "end_portal"
            | "end_gateway"
            | "nether_portal"
            | "budding_amethyst"
            | "spawner"
            | "trial_spawner"
            | "vault"
            | "command_block"
            | "chain_command_block"
            | "repeating_command_block"
            | "structure_block"
            | "jigsaw"
            | "moving_piston"
            | "piston_head"
            | "sculk_shrieker"
    ) {
        return Reaction::Block;
    }
    // --- pushed, never pulled --------------------------------------------
    if n.ends_with("_glazed_terracotta") {
        return Reaction::PushOnly;
    }
    // --- breaks -----------------------------------------------------------
    if matches!(
        n,
        "water"
            | "lava"
            | "bubble_column"
            | "fire"
            | "soul_fire"
            | "cobweb"
            | "snow"
            | "redstone_wire"
            | "repeater"
            | "comparator"
            | "lever"
            | "tripwire"
            | "tripwire_hook"
            | "rail"
            | "powered_rail"
            | "detector_rail"
            | "activator_rail"
            | "ladder"
            | "vine"
            | "glow_lichen"
            | "resin_clump"
            | "sculk_vein"
            | "lily_pad"
            | "sea_pickle"
            | "seagrass"
            | "tall_seagrass"
            | "kelp"
            | "kelp_plant"
            | "sugar_cane"
            | "bamboo"
            | "bamboo_sapling"
            | "cactus_flower"
            | "chorus_plant"
            | "chorus_flower"
            | "cocoa"
            | "nether_wart"
            | "wheat"
            | "carrots"
            | "potatoes"
            | "beetroots"
            | "torchflower_crop"
            | "pitcher_crop"
            | "melon_stem"
            | "pumpkin_stem"
            | "attached_melon_stem"
            | "attached_pumpkin_stem"
            | "sweet_berry_bush"
            | "cave_vines"
            | "cave_vines_plant"
            | "twisting_vines"
            | "twisting_vines_plant"
            | "weeping_vines"
            | "weeping_vines_plant"
            | "hanging_roots"
            | "spore_blossom"
            | "big_dripleaf"
            | "big_dripleaf_stem"
            | "small_dripleaf"
            | "pointed_dripstone"
            | "turtle_egg"
            | "frogspawn"
            | "pink_petals"
            | "wildflowers"
            | "leaf_litter"
            | "flower_pot"
            | "cake"
            | "brown_mushroom"
            | "red_mushroom"
            | "crimson_fungus"
            | "warped_fungus"
            | "crimson_roots"
            | "warped_roots"
            | "nether_sprouts"
            | "short_grass"
            | "tall_grass"
            | "fern"
            | "large_fern"
            | "bush"
            | "firefly_bush"
            | "dead_bush"
            | "seagrass_block"
            | "sunflower"
            | "lilac"
            | "rose_bush"
            | "peony"
            | "pitcher_plant"
            | "glow_berries"
            | "dragon_egg"
            | "scaffolding"
    ) {
        return Reaction::Destroy;
    }
    if n.ends_with("_torch")
        || n.ends_with("_button")
        || n.ends_with("_pressure_plate")
        || n.ends_with("_door")
        || n.ends_with("_sapling")
        || n.ends_with("_propagule")
        || n.ends_with("_amethyst_bud")
        || n == "amethyst_cluster"
        || n == "torch"
        || n == "amethyst_shard"
        || n.ends_with("_bed")
        || n.ends_with("_coral")
        || n.ends_with("_coral_fan")
        || n.ends_with("_coral_wall_fan")
        || n.ends_with("_wall_torch")
        || n.ends_with("_tulip")
        || n.ends_with("_orchid")
        || n.ends_with("_daisy")
        || matches!(
            n,
            "dandelion"
                | "poppy"
                | "cornflower"
                | "allium"
                | "azure_bluet"
                | "lily_of_the_valley"
                | "wither_rose"
                | "torchflower"
                | "closed_eyeblossom"
                | "open_eyeblossom"
                | "spore_blossom"
        )
    {
        return Reaction::Destroy;
    }
    Reaction::Normal
}

/// Vanilla's `PistonBaseBlock.isPushable`, minus the world-height checks the
/// caller already does.
///
/// * `travel` — the direction the structure is travelling.
/// * `allow_destroy` — whether a breakable block counts as movable here (only
///   true for the block a growing structure runs into head-on).
/// * `face` — the direction the block is being approached from, which is what
///   separates "pushed" from "pulled" for the push-only blocks.
fn is_pushable(name: &str, travel: Face, allow_destroy: bool, face: Face) -> bool {
    if name == "air" {
        return false;
    }
    if name == "piston" || name == "sticky_piston" {
        // Handled by the caller: an extended piston is immovable, a retracted
        // one travels like any other block.
        return true;
    }
    match push_reaction(name) {
        Reaction::Block => false,
        Reaction::Destroy => allow_destroy,
        Reaction::PushOnly => travel == face,
        Reaction::Normal => !has_block_entity(name),
    }
}

/// The resolver. Holds the world lookup and the growing lists, exactly like
/// vanilla's object of the same name.
pub struct Resolver<'a, F: Fn(BlockPos) -> StateId> {
    get: F,
    table: &'a BlockTable,
    piston: BlockPos,
    /// Which way the piston faces.
    facing: Face,
    /// Which way the structure travels (the facing when extending, the
    /// opposite when retracting).
    travel: Face,
    start: BlockPos,
    extending: bool,
    push: Vec<BlockPos>,
    destroy: Vec<BlockPos>,
}

fn offset(p: BlockPos, f: Face, n: i32) -> BlockPos {
    let d = f.normal();
    BlockPos { x: p.x + d[0] * n, y: p.y + d[1] * n, z: p.z + d[2] * n }
}

impl<'a, F: Fn(BlockPos) -> StateId> Resolver<'a, F> {
    pub fn new(
        table: &'a BlockTable,
        piston: BlockPos,
        facing: Face,
        extending: bool,
        get: F,
    ) -> Self {
        let (travel, start) = if extending {
            (facing, offset(piston, facing, 1))
        } else {
            (facing.opposite(), offset(piston, facing, 2))
        };
        Self {
            get,
            table,
            piston,
            facing,
            travel,
            start,
            extending,
            push: Vec::new(),
            destroy: Vec::new(),
        }
    }

    fn name(&self, p: BlockPos) -> &str {
        let n = self.table.entry((self.get)(p)).map(|e| e.short_name.as_str()).unwrap_or("air");
        // Vanilla clears the head out of the way before resolving a pull
        // (`moveBlocks` sets it to air), otherwise the piston would jam on its
        // own arm.
        if n == "piston_head" && !self.extending && p == offset(self.piston, self.facing, 1) {
            return "air";
        }
        n
    }

    /// True when this block is an *extended* piston, which nothing can move.
    fn is_extended_piston(&self, p: BlockPos) -> bool {
        let id = (self.get)(p);
        let Some(e) = self.table.entry(id) else { return false };
        (e.short_name == "piston" || e.short_name == "sticky_piston")
            && e.prop("extended") == Some("true")
    }

    fn movable(&self, p: BlockPos, allow_destroy: bool, face: Face) -> bool {
        if self.is_extended_piston(p) {
            return false;
        }
        is_pushable(self.name(p), self.travel, allow_destroy, face)
    }

    /// Run the resolution. `None` means the piston is blocked and does not move
    /// at all; `Some(m)` is what travels and what breaks.
    pub fn resolve(mut self) -> Option<PistonMove> {
        let start = self.start;
        if !self.movable(start, false, self.facing) {
            // A breakable block right in front is simply swept away — but only
            // when extending. Pulling never breaks anything.
            if self.extending && push_reaction(self.name(start)) == Reaction::Destroy {
                self.destroy.push(start);
                return Some(PistonMove { push: self.push, destroy: self.destroy });
            }
            // Air in front of a retracting piston is not a failure, there is
            // just nothing to pull.
            if self.name(start) == "air" {
                return Some(PistonMove::default());
            }
            return None;
        }
        if !self.add_line(start, self.travel) {
            return None;
        }
        let mut i = 0;
        while i < self.push.len() {
            let p = self.push[i];
            if is_sticky(self.name(p)) && !self.add_branches(p) {
                return None;
            }
            i += 1;
        }
        Some(PistonMove { push: self.push, destroy: self.destroy })
    }

    /// Vanilla's `addBlockLine`: walk the sticky chain backwards, then grow
    /// forwards until the line ends in air, breaks something, or jams.
    fn add_line(&mut self, origin: BlockPos, face: Face) -> bool {
        if self.name(origin) == "air" {
            return true;
        }
        if !self.movable(origin, false, face) {
            return true;
        }
        if origin == self.piston {
            return true;
        }
        if self.push.contains(&origin) {
            return true;
        }
        // Back along the chain: every sticky neighbour behind us comes too.
        let back = self.travel.opposite();
        let mut count = 1usize;
        if count + self.push.len() > PUSH_LIMIT {
            return false;
        }
        let mut prev = self.name(origin).to_string();
        while is_sticky(&prev) {
            let p = offset(origin, back, count as i32);
            let here = self.name(p).to_string();
            if here == "air"
                || !stick_to_each_other(&prev, &here)
                || !self.movable(p, false, back)
                || p == self.piston
            {
                break;
            }
            count += 1;
            if count + self.push.len() > PUSH_LIMIT {
                return false;
            }
            prev = here;
        }
        // Farthest-back block first, matching vanilla's move order.
        let mut added = 0usize;
        for j in (0..count).rev() {
            self.push.push(offset(origin, back, j as i32));
            added += 1;
        }
        // Forwards from the origin until something ends the line.
        let mut step = 1i32;
        loop {
            let front = offset(origin, self.travel, step);
            if let Some(idx) = self.push.iter().position(|&p| p == front) {
                // Ran into a line we already collected: vanilla re-orders so
                // the blocks we just added end up ahead of it.
                self.reorder(added, idx);
                for k in 0..=(idx + added) {
                    let p = self.push[k];
                    if is_sticky(self.name(p)) && !self.add_branches(p) {
                        return false;
                    }
                }
                return true;
            }
            let n = self.name(front).to_string();
            if n == "air" {
                return true;
            }
            if !self.movable(front, true, self.travel) || front == self.piston {
                return false;
            }
            if push_reaction(&n) == Reaction::Destroy {
                self.destroy.push(front);
                return true;
            }
            if self.push.len() >= PUSH_LIMIT {
                return false;
            }
            self.push.push(front);
            added += 1;
            step += 1;
        }
    }

    /// Vanilla's `reorderListAtCollision`.
    fn reorder(&mut self, added: usize, index: usize) {
        let head: Vec<BlockPos> = self.push[..index].to_vec();
        let tail: Vec<BlockPos> = self.push[self.push.len() - added..].to_vec();
        let mid: Vec<BlockPos> = self.push[index..self.push.len() - added].to_vec();
        self.push.clear();
        self.push.extend(tail);
        self.push.extend(head);
        self.push.extend(mid);
    }

    /// Vanilla's `addBranchingBlocks`: everything stuck to the sides of a
    /// slime or honey block comes along too.
    fn add_branches(&mut self, pos: BlockPos) -> bool {
        let here = self.name(pos).to_string();
        for f in Face::ALL {
            // Sideways only — the line itself already covers the travel axis.
            if f.normal()[axis(self.travel)] != 0 {
                continue;
            }
            let side = offset(pos, f, 1);
            let there = self.name(side).to_string();
            if stick_to_each_other(&there, &here) && !self.add_line(side, f) {
                return false;
            }
        }
        true
    }
}

/// Index of a face's axis in a `[x, y, z]` triple.
fn axis(f: Face) -> usize {
    match f {
        Face::West | Face::East => 0,
        Face::Down | Face::Up => 1,
        Face::North | Face::South => 2,
    }
}

#[cfg(test)]
mod tests_support {
    use super::*;
    use crate::assets::blockmap::BlockTable;

    pub fn table() -> BlockTable {
        BlockTable::load_or_embedded(None).expect("embedded block table")
    }

    pub fn id(t: &BlockTable, name: &str) -> StateId {
        t.find_state(name, &[]).unwrap_or_else(|| panic!("no state for {name}"))
    }

    /// A tiny world: everything is air except what the test puts in.
    pub struct W {
        pub blocks: std::collections::HashMap<(i32, i32, i32), StateId>,
        pub air: StateId,
    }
    impl W {
        pub fn new(t: &BlockTable) -> Self {
            Self { blocks: Default::default(), air: id(t, "air") }
        }
        pub fn set(&mut self, t: &BlockTable, x: i32, y: i32, z: i32, name: &str) -> &mut Self {
            self.blocks.insert((x, y, z), id(t, name));
            self
        }
        pub fn get(&self) -> impl Fn(BlockPos) -> StateId + '_ {
            move |p: BlockPos| *self.blocks.get(&(p.x, p.y, p.z)).unwrap_or(&self.air)
        }
    }

    pub fn at(x: i32, y: i32, z: i32) -> BlockPos {
        BlockPos { x, y, z }
    }

    #[test]
    fn a_single_stone_is_pushed_one_cell() {
        let t = table();
        let mut w = W::new(&t);
        w.set(&t, 1, 0, 0, "stone");
        let m = Resolver::new(&t, at(0, 0, 0), Face::East, true, w.get()).resolve().unwrap();
        assert_eq!(m.push, vec![at(1, 0, 0)]);
        assert!(m.destroy.is_empty());
    }

    #[test]
    fn a_line_of_twelve_moves_and_thirteen_jams() {
        let t = table();
        let mut w = W::new(&t);
        for i in 1..=12 {
            w.set(&t, i, 0, 0, "stone");
        }
        let m = Resolver::new(&t, at(0, 0, 0), Face::East, true, w.get()).resolve().unwrap();
        assert_eq!(m.push.len(), 12);
        // The farthest block travels first.
        assert_eq!(m.push[0], at(1, 0, 0));
        w.set(&t, 13, 0, 0, "stone");
        assert!(Resolver::new(&t, at(0, 0, 0), Face::East, true, w.get()).resolve().is_none());
    }

    #[test]
    fn obsidian_stops_the_piston_and_a_torch_just_breaks() {
        let t = table();
        let mut w = W::new(&t);
        w.set(&t, 1, 0, 0, "stone").set(&t, 2, 0, 0, "obsidian");
        assert!(Resolver::new(&t, at(0, 0, 0), Face::East, true, w.get()).resolve().is_none());

        let mut w = W::new(&t);
        w.set(&t, 1, 0, 0, "stone").set(&t, 2, 0, 0, "torch");
        let m = Resolver::new(&t, at(0, 0, 0), Face::East, true, w.get()).resolve().unwrap();
        assert_eq!(m.push, vec![at(1, 0, 0)]);
        assert_eq!(m.destroy, vec![at(2, 0, 0)]);
    }

    #[test]
    fn a_chest_is_immovable_because_it_has_a_block_entity() {
        let t = table();
        let mut w = W::new(&t);
        w.set(&t, 1, 0, 0, "chest");
        assert!(Resolver::new(&t, at(0, 0, 0), Face::East, true, w.get()).resolve().is_none());
    }

    #[test]
    fn a_slime_block_drags_its_neighbours_sideways() {
        let t = table();
        let mut w = W::new(&t);
        w.set(&t, 1, 0, 0, "slime_block");
        w.set(&t, 1, 1, 0, "stone");
        w.set(&t, 1, 0, 1, "stone");
        let m = Resolver::new(&t, at(0, 0, 0), Face::East, true, w.get()).resolve().unwrap();
        assert!(m.push.contains(&at(1, 0, 0)));
        assert!(m.push.contains(&at(1, 1, 0)), "the block on top of the slime comes along");
        assert!(m.push.contains(&at(1, 0, 1)), "and so does the one beside it");
        assert_eq!(m.push.len(), 3);
    }

    #[test]
    fn honey_and_slime_refuse_to_stick_to_each_other() {
        let t = table();
        let mut w = W::new(&t);
        w.set(&t, 1, 0, 0, "slime_block");
        w.set(&t, 1, 1, 0, "honey_block");
        let m = Resolver::new(&t, at(0, 0, 0), Face::East, true, w.get()).resolve().unwrap();
        assert_eq!(m.push, vec![at(1, 0, 0)]);
    }

    #[test]
    fn retracting_pulls_the_block_two_cells_out() {
        let t = table();
        let mut w = W::new(&t);
        w.set(&t, 1, 0, 0, "piston_head").set(&t, 2, 0, 0, "stone");
        let m = Resolver::new(&t, at(0, 0, 0), Face::East, false, w.get()).resolve().unwrap();
        assert_eq!(m.push, vec![at(2, 0, 0)]);
        assert!(m.destroy.is_empty(), "a retracting piston never breaks anything");
    }

    #[test]
    fn retracting_into_nothing_is_not_a_failure() {
        let t = table();
        let w = W::new(&t);
        let m = Resolver::new(&t, at(0, 0, 0), Face::East, false, w.get()).resolve().unwrap();
        assert!(m.is_empty());
    }

    #[test]
    fn glazed_terracotta_pushes_but_will_not_be_pulled() {
        let t = table();
        let mut w = W::new(&t);
        w.set(&t, 1, 0, 0, "white_glazed_terracotta");
        let m = Resolver::new(&t, at(0, 0, 0), Face::East, true, w.get()).resolve().unwrap();
        assert_eq!(m.push, vec![at(1, 0, 0)]);

        let mut w = W::new(&t);
        w.set(&t, 1, 0, 0, "piston_head").set(&t, 2, 0, 0, "white_glazed_terracotta");
        assert!(
            Resolver::new(&t, at(0, 0, 0), Face::East, false, w.get()).resolve().is_none(),
            "pulling it is not allowed"
        );
    }

    #[test]
    fn an_extended_piston_cannot_be_pushed() {
        let t = table();
        let mut w = W::new(&t);
        let extended = t.find_state("piston", &[("extended", "true"), ("facing", "east")]).unwrap();
        w.blocks.insert((1, 0, 0), extended);
        assert!(Resolver::new(&t, at(0, 0, 0), Face::East, true, w.get()).resolve().is_none());
    }

    #[test]
    fn a_slime_chain_never_exceeds_the_limit() {
        let t = table();
        let mut w = W::new(&t);
        // A slab of slime 4×4 across the push direction: 16 blocks, over the
        // twelve-block limit, so the piston refuses.
        for y in 0..4 {
            for z in 0..4 {
                w.set(&t, 1, y, z, "slime_block");
            }
        }
        assert!(Resolver::new(&t, at(1, -1, 0), Face::Up, true, w.get()).resolve().is_none());
    }
}

#[cfg(test)]
mod retract_tests {
    use super::tests_support::*;
    use super::*;

    #[test]
    fn a_sticky_piston_pulls_a_slime_block_and_its_passenger() {
        let t = table();
        let mut w = W::new(&t);
        w.set(&t, 1, 0, 0, "piston_head");
        w.set(&t, 2, 0, 0, "slime_block");
        w.set(&t, 2, 1, 0, "stone");
        let m = Resolver::new(&t, at(0, 0, 0), Face::East, false, w.get())
            .resolve()
            .expect("the pull should resolve");
        assert!(m.push.contains(&at(2, 0, 0)));
        assert!(m.push.contains(&at(2, 1, 0)), "the block riding the slime comes too");
    }
}
