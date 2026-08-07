//! The ambience blocks make on their own.
//!
//! Almost none of what you see idling in a Minecraft world comes from the
//! server. Every tick the client picks a few hundred random blocks near the
//! player and asks each one whether it wants to do something — vanilla's
//! `Block.animateTick`. That is where torch smoke, campfire columns, lava pops,
//! portal swirl, cherry petals and drips all come from. Without it a world is
//! technically correct and completely dead.
//!
//! Everything here is a pure function of a block's name, its state properties
//! and its neighbours, so it can be tested without a world.

use crate::bridge::events::ParticleTex;
use crate::types::BlockPos;

/// One particle burst a block asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emission {
    pub pos: [f64; 3],
    pub tex: ParticleTex,
    pub color: [f32; 3],
    pub size: f32,
    pub count: u32,
    pub spread: [f32; 3],
    /// Initial speed in blocks per tick, the same unit the server uses.
    pub speed: f32,
    /// Downward acceleration in blocks/s²; negative floats upward.
    pub gravity: f32,
}

impl Emission {
    fn at(pos: [f64; 3], tex: ParticleTex) -> Self {
        Self {
            pos,
            tex,
            color: [1.0, 1.0, 1.0],
            size: 0.14,
            count: 1,
            spread: [0.0; 3],
            speed: 0.0,
            gravity: 0.0,
        }
    }

    fn color(mut self, color: [f32; 3]) -> Self {
        self.color = color;
        self
    }

    fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    fn spread(mut self, x: f32, y: f32, z: f32) -> Self {
        self.spread = [x, y, z];
        self
    }

    fn drift(mut self, speed: f32, gravity: f32) -> Self {
        self.speed = speed;
        self.gravity = gravity;
        self
    }
}

/// A tiny deterministic xorshift, so a scene renders the same twice.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }

    /// True with probability `1/n` — vanilla writes most of these as
    /// `random.nextInt(n) == 0`.
    pub fn one_in(&mut self, n: u32) -> bool {
        (self.next_f32() * n as f32) as u32 == 0
    }
}

/// What the block above matters for: a few rules only fire when something
/// specific sits on top (water over magma, air under a dripstone tip).
pub struct Neighbours<'a> {
    pub above: &'a str,
    pub below: &'a str,
}

/// Look up a state property (they arrive sorted by key from the block table).
fn prop<'a>(props: &'a [(String, String)], key: &str) -> Option<&'a str> {
    props.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn is(props: &[(String, String)], key: &str, value: &str) -> bool {
    prop(props, key) == Some(value)
}

/// The offset of a wall-mounted torch's flame from the block's own corner.
fn wall_torch_flame(props: &[(String, String)]) -> [f64; 3] {
    // Vanilla hangs the flame off the opposite side from the wall it is on.
    let (dx, dz) = match prop(props, "facing") {
        Some("north") => (0.0, 0.27),
        Some("south") => (0.0, -0.27),
        Some("west") => (0.27, 0.0),
        Some("east") => (-0.27, 0.0),
        _ => (0.0, 0.0),
    };
    [0.5 + dx, 0.7, 0.5 + dz]
}

/// The face a lit furnace/smoker breathes out of.
fn facing_offset(props: &[(String, String)]) -> (f64, f64) {
    match prop(props, "facing") {
        Some("north") => (0.0, -0.52),
        Some("south") => (0.0, 0.52),
        Some("west") => (-0.52, 0.0),
        Some("east") => (0.52, 0.0),
        _ => (0.0, 0.0),
    }
}

/// Everything one block wants to spawn this tick. Empty for the vast majority
/// of blocks, which is what keeps this cheap enough to run on hundreds of them.
pub fn emissions(
    name: &str,
    props: &[(String, String)],
    pos: BlockPos,
    n: &Neighbours<'_>,
    rng: &mut Rng,
    out: &mut Vec<Emission>,
) {
    let (bx, by, bz) = (pos.x as f64, pos.y as f64, pos.z as f64);
    let at = |dx: f64, dy: f64, dz: f64| [bx + dx, by + dy, bz + dz];

    match name {
        // Torches: one wisp of smoke and one flame, right at the tip.
        "torch" | "soul_torch" | "redstone_torch" => {
            if name == "redstone_torch" {
                if is(props, "lit", "false") {
                    return;
                }
                out.push(
                    Emission::at(at(0.5, 0.7, 0.5), ParticleTex::Dust)
                        .color([0.9, 0.15, 0.1])
                        .size(0.08)
                        .spread(0.1, 0.1, 0.1),
                );
                return;
            }
            let p = at(0.5, 0.7, 0.5);
            out.push(Emission::at(p, ParticleTex::Smoke).size(0.10).drift(0.0, -0.4));
            out.push(Emission::at(
                p,
                if name == "soul_torch" { ParticleTex::SoulFlame } else { ParticleTex::Flame },
            ));
        }
        "wall_torch" | "soul_wall_torch" | "redstone_wall_torch" => {
            let o = wall_torch_flame(props);
            let p = at(o[0], o[1], o[2]);
            if name == "redstone_wall_torch" {
                if is(props, "lit", "false") {
                    return;
                }
                out.push(
                    Emission::at(p, ParticleTex::Dust)
                        .color([0.9, 0.15, 0.1])
                        .size(0.08)
                        .spread(0.1, 0.1, 0.1),
                );
                return;
            }
            out.push(Emission::at(p, ParticleTex::Smoke).size(0.10).drift(0.0, -0.4));
            out.push(Emission::at(
                p,
                if name == "soul_wall_torch" { ParticleTex::SoulFlame } else { ParticleTex::Flame },
            ));
        }
        // A lit campfire pushes a slow column of smoke well above itself.
        "campfire" | "soul_campfire" => {
            if is(props, "lit", "false") {
                return;
            }
            out.push(
                Emission::at(at(0.5, 1.0, 0.5), ParticleTex::Smoke)
                    .size(0.5)
                    .spread(0.25, 0.1, 0.25)
                    .drift(0.02, -0.55),
            );
            if rng.one_in(5) {
                out.push(
                    Emission::at(at(0.5, 0.6, 0.5), ParticleTex::Lava)
                        .size(0.14)
                        .spread(0.2, 0.0, 0.2)
                        .drift(0.05, 3.0),
                );
            }
        }
        // Lit smelters breathe smoke and a flicker out of their front face.
        "furnace" | "blast_furnace" | "smoker" => {
            if is(props, "lit", "false") {
                return;
            }
            let (dx, dz) = facing_offset(props);
            let p = at(0.5 + dx, 0.5, 0.5 + dz);
            out.push(Emission::at(p, ParticleTex::Smoke).size(0.10).spread(0.05, 0.05, 0.05));
            if rng.one_in(2) {
                out.push(Emission::at(p, ParticleTex::Flame).size(0.10));
            }
        }
        // Lava pops and steams. Vanilla is sparing with this — it is loud.
        "lava" => {
            if !is(props, "level", "0") {
                return;
            }
            if rng.one_in(100) {
                out.push(
                    Emission::at(at(0.5, 1.0, 0.5), ParticleTex::Lava)
                        .size(0.22)
                        .spread(0.4, 0.0, 0.4)
                        .drift(0.12, 6.0),
                );
            }
            if rng.one_in(200) {
                out.push(
                    Emission::at(at(0.5, 1.1, 0.5), ParticleTex::Smoke)
                        .size(0.3)
                        .spread(0.3, 0.0, 0.3)
                        .drift(0.0, -0.3),
                );
            }
        }
        "fire" | "soul_fire" => {
            out.push(
                Emission::at(at(0.5, 0.6, 0.5), ParticleTex::Smoke)
                    .size(0.2)
                    .spread(0.3, 0.2, 0.3)
                    .drift(0.0, -0.4),
            );
        }
        // The portal's purple drift, in both directions.
        "nether_portal" => {
            // Vanilla throws several out per tick, and they drift clear of the
            // frame rather than sitting inside it.
            let mut e = Emission::at(at(0.5, 0.5, 0.5), ParticleTex::Portal)
                .color([0.55, 0.20, 0.85])
                .size(0.18)
                .spread(0.5, 0.6, 0.5)
                .drift(0.14, -0.05);
            e.count = 3;
            out.push(e);
        }
        "end_portal" | "end_gateway" => {
            out.push(
                Emission::at(at(0.5, 0.8, 0.5), ParticleTex::Portal)
                    .color([0.35, 0.15, 0.55])
                    .size(0.16)
                    .spread(0.45, 0.2, 0.45)
                    .drift(0.05, 0.0),
            );
        }
        "dragon_egg" => {
            out.push(
                Emission::at(at(0.5, 0.8, 0.5), ParticleTex::Portal)
                    .color([0.35, 0.15, 0.55])
                    .size(0.16)
                    .spread(0.3, 0.3, 0.3)
                    .drift(0.04, 0.0),
            );
        }
        // End rods throw a fine spark out of whichever way they point.
        "end_rod" => {
            let (dx, dy, dz) = match prop(props, "facing") {
                Some("up") => (0.0, 1.0, 0.0),
                Some("down") => (0.0, 0.0, 0.0),
                Some("north") => (0.5, 0.5, 0.0),
                Some("south") => (0.5, 0.5, 1.0),
                Some("west") => (0.0, 0.5, 0.5),
                Some("east") => (1.0, 0.5, 0.5),
                _ => (0.5, 1.0, 0.5),
            };
            let (dx, dz) = if matches!(prop(props, "facing"), Some("up") | Some("down") | None) {
                (0.5, 0.5)
            } else {
                (dx, dz)
            };
            out.push(
                Emission::at(at(dx, dy, dz), ParticleTex::Spark)
                    .size(0.08)
                    .spread(0.05, 0.05, 0.05)
                    .drift(0.02, -0.05),
            );
        }
        // Magma only bubbles when it is under water.
        "magma_block" => {
            if n.above == "water" && rng.one_in(3) {
                out.push(
                    Emission::at(at(0.5, 1.05, 0.5), ParticleTex::Bubble)
                        .size(0.16)
                        .spread(0.4, 0.0, 0.4)
                        .drift(0.03, -1.2),
                );
            }
        }
        "bubble_column" => {
            out.push(
                Emission::at(at(0.5, 0.5, 0.5), ParticleTex::Bubble)
                    .size(0.16)
                    .spread(0.4, 0.4, 0.4)
                    .drift(0.05, -2.0),
            );
        }
        // Spore blossoms rain green motes onto whatever is below them.
        "spore_blossom" => {
            out.push(
                Emission::at(at(0.5, -0.1, 0.5), ParticleTex::Happy)
                    .color([0.55, 0.75, 0.35])
                    .size(0.10)
                    .spread(0.45, 0.0, 0.45)
                    .drift(0.0, 0.4),
            );
        }
        "cherry_leaves" => {
            if rng.one_in(6) {
                out.push(
                    Emission::at(at(0.5, -0.05, 0.5), ParticleTex::Cherry)
                        .size(0.24)
                        .spread(0.5, 0.1, 0.5)
                        .drift(0.0, 0.6),
                );
            }
        }
        "pale_oak_leaves" => {
            if rng.one_in(8) {
                out.push(
                    Emission::at(at(0.5, -0.05, 0.5), ParticleTex::PaleOak)
                        .size(0.24)
                        .spread(0.5, 0.1, 0.5)
                        .drift(0.0, 0.6),
                );
            }
        }
        // Every other leaf sheds the occasional leaf, but only into open air.
        _ if name.ends_with("_leaves") => {
            if n.below == "air" && rng.one_in(30) {
                out.push(
                    Emission::at(at(0.5, -0.05, 0.5), ParticleTex::Leaf)
                        .color([0.42, 0.65, 0.28])
                        .size(0.20)
                        .spread(0.5, 0.1, 0.5)
                        .drift(0.0, 0.6),
                );
            }
        }
        // Crying obsidian weeps; a dripstone tip drips into the air below it.
        "crying_obsidian" => {
            if rng.one_in(5) {
                out.push(
                    Emission::at(at(0.5, -0.05, 0.5), ParticleTex::Drip)
                        .color([0.55, 0.15, 0.9])
                        .size(0.10)
                        .spread(0.4, 0.0, 0.4)
                        .drift(0.0, 2.0),
                );
            }
        }
        "pointed_dripstone" => {
            if !is(props, "vertical_direction", "down") || n.below != "air" {
                return;
            }
            if rng.one_in(10) {
                out.push(
                    Emission::at(at(0.5, -0.05, 0.5), ParticleTex::Drip)
                        .color([0.25, 0.45, 0.9])
                        .size(0.09)
                        .drift(0.0, 3.0),
                );
            }
        }
        "brewing_stand" => {
            out.push(
                Emission::at(
                    at(0.4 + rng.next_f32() as f64 * 0.2, 0.7, 0.4 + rng.next_f32() as f64 * 0.2),
                    ParticleTex::Smoke,
                )
                    .size(0.08)
                    .drift(0.0, -0.3),
            );
        }
        "conduit" => {
            out.push(
                Emission::at(at(0.5, 0.5, 0.5), ParticleTex::Nautilus)
                    .size(0.20)
                    .spread(0.6, 0.6, 0.6)
                    .drift(0.04, 0.0),
            );
        }
        "sculk_catalyst" | "sculk_shrieker" | "sculk_sensor" | "calibrated_sculk_sensor" => {
            if rng.one_in(4) {
                out.push(
                    Emission::at(at(0.5, 1.05, 0.5), ParticleTex::SculkSoul)
                        .size(0.16)
                        .spread(0.35, 0.0, 0.35)
                        .drift(0.02, -0.5),
                );
            }
        }
        "soul_sand" | "soul_soil" => {
            if rng.one_in(20) {
                out.push(
                    Emission::at(at(0.5, 1.02, 0.5), ParticleTex::Soul)
                        .size(0.18)
                        .spread(0.35, 0.0, 0.35)
                        .drift(0.01, -0.3),
                );
            }
        }
        "firefly_bush" => {
            if rng.one_in(3) {
                out.push(
                    Emission::at(at(0.5, 0.6, 0.5), ParticleTex::Firefly)
                        .size(0.10)
                        .spread(0.6, 0.5, 0.6)
                        .drift(0.02, 0.0),
                );
            }
        }
        "respawn_anchor" => {
            if is(props, "charges", "0") {
                return;
            }
            out.push(
                Emission::at(at(0.5, 1.0, 0.5), ParticleTex::Portal)
                    .color([0.9, 0.25, 0.95])
                    .size(0.14)
                    .spread(0.3, 0.1, 0.3)
                    .drift(0.03, -0.2),
            );
        }
        // Mycelium's little spore cloud, vanilla's "town aura".
        "mycelium" => {
            if rng.one_in(10) {
                out.push(
                    Emission::at(at(0.5, 1.05, 0.5), ParticleTex::Happy)
                        .color([0.62, 0.55, 0.66])
                        .size(0.08)
                        .spread(0.4, 0.0, 0.4)
                        .drift(0.0, -0.05),
                );
            }
        }
        // Powered redstone glows red where the signal runs.
        "redstone_wire" => {
            let power: u32 = prop(props, "power").and_then(|p| p.parse().ok()).unwrap_or(0);
            if power > 0 && rng.one_in(6) {
                let k = 0.3 + 0.7 * power as f32 / 15.0;
                out.push(
                    Emission::at(at(0.5, 0.08, 0.5), ParticleTex::Dust)
                        .color([k, k * 0.15, k * 0.1])
                        .size(0.07)
                        .spread(0.4, 0.0, 0.4),
                );
            }
        }
        // Enchanting tables pull glyphs out of the shelves around them.
        "enchanting_table" => {
            if rng.one_in(3) {
                out.push(
                    Emission::at(at(0.5, 1.1, 0.5), ParticleTex::Effect)
                        .color([0.75, 0.7, 0.95])
                        .size(0.14)
                        .spread(1.4, 0.4, 1.4)
                        .drift(0.03, 0.6),
                );
            }
        }
        // A full beehive drips honey.
        "beehive" | "bee_nest" if is(props, "honey_level", "5") && rng.one_in(8) => {
            out.push(
                Emission::at(at(0.5, -0.05, 0.5), ParticleTex::Drip)
                    .color([0.95, 0.7, 0.15])
                    .size(0.10)
                    .spread(0.35, 0.0, 0.35)
                    .drift(0.0, 2.0),
            );
        }
        _ => {}
    }
}

/// A rain splash on top of `pos`, the way vanilla stipples wet ground.
pub fn rain_splash(pos: BlockPos, rng: &mut Rng) -> Emission {
    Emission::at(
        [
            pos.x as f64 + rng.next_f32() as f64,
            pos.y as f64 + 1.02,
            pos.z as f64 + rng.next_f32() as f64,
        ],
        ParticleTex::Splash,
    )
    .size(0.10)
    .drift(0.01, 1.0)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn run(name: &str, p: &[(&str, &str)]) -> Vec<Emission> {
        let mut out = Vec::new();
        let mut rng = Rng::new(0x1234_5678);
        let n = Neighbours { above: "air", below: "stone" };
        // Several ticks, so the one-in-N rules get a fair chance to fire.
        for _ in 0..64 {
            emissions(name, &props(p), BlockPos { x: 0, y: 0, z: 0 }, &n, &mut rng, &mut out);
        }
        out
    }

    #[test]
    fn most_blocks_do_nothing_at_all() {
        assert!(run("stone", &[]).is_empty());
        assert!(run("oak_planks", &[]).is_empty());
        assert!(run("dirt", &[]).is_empty());
    }

    #[test]
    fn a_torch_makes_both_smoke_and_flame() {
        let e = run("torch", &[]);
        assert!(e.iter().any(|x| x.tex == ParticleTex::Smoke));
        assert!(e.iter().any(|x| x.tex == ParticleTex::Flame));
        // …and a soul torch burns blue instead.
        let soul = run("soul_torch", &[]);
        assert!(soul.iter().any(|x| x.tex == ParticleTex::SoulFlame));
        assert!(!soul.iter().any(|x| x.tex == ParticleTex::Flame));
    }

    #[test]
    fn a_wall_torch_puts_its_flame_on_the_open_side() {
        let mut out = Vec::new();
        let mut rng = Rng::new(1);
        let n = Neighbours { above: "air", below: "stone" };
        emissions(
            "wall_torch",
            &props(&[("facing", "north")]),
            BlockPos { x: 0, y: 0, z: 0 },
            &n,
            &mut rng,
            &mut out,
        );
        // Facing north means the torch hangs off the south side of the block.
        assert!(out[0].pos[2] > 0.5, "flame should sit south of centre: {:?}", out[0].pos);
    }

    #[test]
    fn an_unlit_block_stays_quiet() {
        assert!(run("campfire", &[("lit", "false")]).is_empty());
        assert!(run("furnace", &[("lit", "false")]).is_empty());
        assert!(run("redstone_torch", &[("lit", "false")]).is_empty());
        assert!(!run("campfire", &[("lit", "true")]).is_empty());
    }

    #[test]
    fn flowing_lava_does_not_pop_only_the_source_does() {
        assert!(run("lava", &[("level", "3")]).is_empty());
    }

    #[test]
    fn magma_only_bubbles_under_water() {
        let mut out = Vec::new();
        let mut rng = Rng::new(7);
        let dry = Neighbours { above: "air", below: "stone" };
        for _ in 0..64 {
            emissions("magma_block", &[], BlockPos { x: 0, y: 0, z: 0 }, &dry, &mut rng, &mut out);
        }
        assert!(out.is_empty());
        let wet = Neighbours { above: "water", below: "stone" };
        for _ in 0..64 {
            emissions("magma_block", &[], BlockPos { x: 0, y: 0, z: 0 }, &wet, &mut rng, &mut out);
        }
        assert!(out.iter().any(|e| e.tex == ParticleTex::Bubble));
    }

    #[test]
    fn a_dripstone_tip_only_drips_downward_into_air() {
        let mut out = Vec::new();
        let mut rng = Rng::new(3);
        let solid = Neighbours { above: "stone", below: "stone" };
        for _ in 0..64 {
            emissions(
                "pointed_dripstone",
                &props(&[("vertical_direction", "down")]),
                BlockPos { x: 0, y: 0, z: 0 },
                &solid,
                &mut rng,
                &mut out,
            );
        }
        assert!(out.is_empty(), "no room below, so nothing drips");
        let open = Neighbours { above: "stone", below: "air" };
        for _ in 0..64 {
            emissions(
                "pointed_dripstone",
                &props(&[("vertical_direction", "up")]),
                BlockPos { x: 0, y: 0, z: 0 },
                &open,
                &mut rng,
                &mut out,
            );
        }
        assert!(out.is_empty(), "a stalagmite points the wrong way to drip");
    }

    #[test]
    fn redstone_brightens_with_the_signal() {
        let weak = run("redstone_wire", &[("power", "1")]);
        let strong = run("redstone_wire", &[("power", "15")]);
        assert!(!weak.is_empty() && !strong.is_empty());
        assert!(strong[0].color[0] > weak[0].color[0]);
        assert!(run("redstone_wire", &[("power", "0")]).is_empty());
    }

    #[test]
    fn cherry_leaves_drop_petals_and_ordinary_leaves_drop_leaves() {
        assert!(run("cherry_leaves", &[]).iter().all(|e| e.tex == ParticleTex::Cherry));
        let oak = run("oak_leaves", &[]);
        assert!(oak.is_empty(), "with stone below there is nowhere for a leaf to fall");
        let mut out = Vec::new();
        let mut rng = Rng::new(11);
        let open = Neighbours { above: "air", below: "air" };
        for _ in 0..256 {
            emissions("oak_leaves", &[], BlockPos { x: 0, y: 0, z: 0 }, &open, &mut rng, &mut out);
        }
        assert!(out.iter().any(|e| e.tex == ParticleTex::Leaf));
    }

    #[test]
    fn the_same_seed_gives_the_same_ambience() {
        let a = run("campfire", &[("lit", "true")]);
        let b = run("campfire", &[("lit", "true")]);
        assert_eq!(a.len(), b.len());
    }
}
