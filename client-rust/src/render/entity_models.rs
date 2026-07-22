//! Data-only definitions of non-humanoid entity models (creeper, pig, cow,
//! sheep, chicken, …). Each model is a list of textured cuboids authored in
//! Minecraft entity-model pixels, Y-up with the feet at y=0 and +Z facing
//! forward (vanilla "south"). The renderer turns these into vertex buffers and
//! animates the parts; the texture comes from the mob's real `entity/…` PNG.
//!
//! Box texture offsets and dimensions follow the vanilla model, so the standard
//! box UV-unwrap (right/front/left/back strip + top/bottom) maps 1:1 onto the
//! real texture. Horizontal bodies keep the vanilla trick of authoring a
//! vertical box and rotating it 90° about X (`x_rot`) so the unwrap still lines
//! up.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// How a part reacts to the per-frame animation inputs.
#[derive(Clone, Copy)]
pub enum PartAnim {
    /// Never moves (bodies, wings).
    Static,
    /// Rotates about X by the entity's head pitch (heads, snouts ride along).
    Head,
    /// Rotates about X by `swing * sign` — walking limbs. Diagonally opposite
    /// legs share a sign so a quadruped strides naturally.
    Leg(f32),
}

/// One textured cuboid. `center`/`size` are in texture pixels, relative to the
/// owning part's pivot; `uv` is the box's top-left on the texture; `inflate`
/// grows the geometry without changing UVs (overlay layers).
#[derive(Clone, Copy)]
pub struct Cube {
    pub center: [f32; 3],
    pub size: [f32; 3],
    pub uv: [f32; 2],
    pub inflate: f32,
}

impl Cube {
    const fn new(center: [f32; 3], size: [f32; 3], uv: [f32; 2]) -> Self {
        Cube { center, size, uv, inflate: 0.0 }
    }
}

/// A rigid group of cuboids that rotates together around `pivot` (feet-relative
/// pixels). `x_rot` is a fixed pre-rotation baked into the geometry (used to lay
/// bodies flat); `anim` selects the per-frame motion.
pub struct Part {
    pub anim: PartAnim,
    pub pivot: [f32; 3],
    pub x_rot: f32,
    /// Fixed pre-rotation about Y, baked after `x_rot` (boat walls keep the
    /// vanilla UV unwrap by authoring the box straight and turning it).
    pub y_rot: f32,
    /// Fixed pre-rotation about Z (roll), baked last — splays spider legs out to
    /// the side and angles limbs while keeping the standard box UV unwrap.
    pub z_rot: f32,
    pub cubes: Vec<Cube>,
}

impl Part {
    /// A part with no baked rotations — the common case.
    fn plain(anim: PartAnim, pivot: [f32; 3], cubes: Vec<Cube>) -> Self {
        Part { anim, pivot, x_rot: 0.0, y_rot: 0.0, z_rot: 0.0, cubes }
    }
}

/// A whole mob model: its texture dimensions, model scale (blocks per pixel) and
/// its parts.
pub struct ModelDef {
    pub tex_w: f32,
    pub tex_h: f32,
    pub scale: f32,
    pub parts: Vec<Part>,
}

/// The non-humanoid mob models we render with real textures. Order defines the
/// stable index used by the renderer's mesh table and by `index()`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MobModel {
    Creeper,
    Pig,
    Sheep,
    Chicken,
    Cow,
    Boat,
    Slime,
    Spider,
    Wolf,
    Fox,
    Villager,
    Enderman,
    IronGolem,
    Squid,
    Bat,
    Rabbit,
    Horse,
    Cat,
    SnowGolem,
    Turtle,
    Goat,
    Panda,
    PolarBear,
    Llama,
}

impl MobModel {
    pub fn all() -> [MobModel; 24] {
        use MobModel::*;
        [
            Creeper, Pig, Sheep, Chicken, Cow, Boat, Slime, Spider, Wolf, Fox, Villager,
            Enderman, IronGolem, Squid, Bat, Rabbit, Horse, Cat, SnowGolem, Turtle, Goat,
            Panda, PolarBear, Llama,
        ]
    }

    /// Dense 0-based index into the renderer's mesh table.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Default model scale: 16 texture pixels = 1 block.
const PX: f32 = 1.0 / 16.0;

/// Parameters for the shared quadruped model (pig / cow / sheep …). All sizes in
/// texture pixels. The body is authored as a vertical box and rotated flat.
struct Quad {
    tex: (f32, f32),
    /// Head cube: size + UV, centered at `head_pivot`.
    head_size: [f32; 3],
    head_uv: [f32; 2],
    head_pivot: [f32; 3],
    /// Extra cubes attached to the head (snout, horns), relative to head_pivot.
    head_extra: Vec<Cube>,
    /// Body authored vertical dims [w, length, depth] + UV + center.
    body_dims: [f32; 3],
    body_uv: [f32; 2],
    body_center: [f32; 3],
    /// Leg cube size + UV, and placement.
    leg_size: [f32; 3],
    leg_uv: [f32; 2],
    leg_x: f32,
    leg_zf: f32,
    leg_zb: f32,
    leg_top: f32,
}

fn quadruped(q: Quad) -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| Part {
        anim: PartAnim::Leg(sign),
        pivot: [x, q.leg_top, z],
        x_rot: 0.0,
        y_rot: 0.0,
        z_rot: 0.0,
        cubes: vec![Cube::new([0.0, -q.leg_size[1] / 2.0, 0.0], q.leg_size, q.leg_uv)],
    };
    let mut head_cubes = vec![Cube::new([0.0, 0.0, 0.0], q.head_size, q.head_uv)];
    head_cubes.extend(q.head_extra);
    ModelDef {
        tex_w: q.tex.0,
        tex_h: q.tex.1,
        scale: PX,
        parts: vec![
            Part { anim: PartAnim::Head, pivot: q.head_pivot, x_rot: 0.0, y_rot: 0.0, z_rot: 0.0, cubes: head_cubes },
            // Body: vertical box laid flat (long axis → +Z) so the UV unwrap
            // matches vanilla.
            Part {
                anim: PartAnim::Static,
                pivot: q.body_center,
                x_rot: FRAC_PI_2,
                y_rot: 0.0,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], q.body_dims, q.body_uv)],
            },
            leg(q.leg_x, q.leg_zf, 1.0),
            leg(-q.leg_x, q.leg_zf, -1.0),
            leg(q.leg_x, q.leg_zb, -1.0),
            leg(-q.leg_x, q.leg_zb, 1.0),
        ],
    }
}

/// The geometry for one mob model.
pub fn model_def(m: MobModel) -> ModelDef {
    match m {
        MobModel::Creeper => creeper(),
        MobModel::Pig => pig(),
        MobModel::Sheep => sheep(),
        MobModel::Chicken => chicken(),
        MobModel::Cow => cow(),
        MobModel::Boat => boat(),
        MobModel::Slime => slime(),
        MobModel::Spider => spider(),
        MobModel::Wolf => wolf(),
        MobModel::Fox => fox(),
        MobModel::Villager => villager(),
        MobModel::Enderman => enderman(),
        MobModel::IronGolem => iron_golem(),
        MobModel::Squid => squid(),
        MobModel::Bat => bat(),
        MobModel::Rabbit => rabbit(),
        MobModel::Horse => horse(),
        MobModel::Cat => cat(),
        MobModel::SnowGolem => snow_golem(),
        MobModel::Turtle => turtle(),
        MobModel::Goat => goat(),
        MobModel::Panda => panda(),
        MobModel::PolarBear => polar_bear(),
        MobModel::Llama => llama(),
    }
}

/// Slime (and magma cube): the vanilla outer body cube (8³) with the eyes and
/// mouth on the front face. Authored at the size-1 slime scale (8 px = 0.5
/// block); the app scales the whole model by the entity's size. Texture 64×32.
fn slime() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![Part {
            anim: PartAnim::Static,
            pivot: [0.0, 0.0, 0.0],
            x_rot: 0.0,
            y_rot: 0.0,
            z_rot: 0.0,
            cubes: vec![
                // Body cube (the outer shell texture reads as a slime); the
                // eyes/mouth live in a separate texture patch that doesn't tile
                // cleanly onto the cube, so we keep the iconic plain green cube.
                Cube::new([0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [0.0, 16.0]),
            ],
        }],
    }
}

// --- individual models -------------------------------------------------------

fn creeper() -> ModelDef {
    // tex 64×32. Head 8³ on a 8×12×4 body over four 4×6×4 legs.
    let leg = |x: f32, z: f32, sign: f32| Part {
        anim: PartAnim::Leg(sign),
        pivot: [x, 6.0, z],
        x_rot: 0.0,
        y_rot: 0.0,
        z_rot: 0.0,
        cubes: vec![Cube::new([0.0, -3.0, 0.0], [4.0, 6.0, 4.0], [0.0, 16.0])],
    };
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            Part {
                anim: PartAnim::Head,
                pivot: [0.0, 18.0, 0.0],
                x_rot: 0.0,
                y_rot: 0.0,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0])],
            },
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 0.0, 0.0],
                x_rot: 0.0,
                y_rot: 0.0,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 12.0, 0.0], [8.0, 12.0, 4.0], [16.0, 16.0])],
            },
            leg(2.0, 2.0, 1.0),
            leg(-2.0, 2.0, -1.0),
            leg(2.0, -2.0, -1.0),
            leg(-2.0, -2.0, 1.0),
        ],
    }
}

fn pig() -> ModelDef {
    // 26.1's pig texture is padded to 64×64 but keeps the classic top-32 layout.
    quadruped(Quad {
        tex: (64.0, 64.0),
        head_size: [8.0, 8.0, 8.0],
        head_uv: [0.0, 0.0],
        head_pivot: [0.0, 12.0, 8.0],
        // Snout at the front-bottom of the head.
        head_extra: vec![Cube::new([0.0, -1.0, 4.0], [4.0, 3.0, 1.0], [16.0, 16.0])],
        body_dims: [10.0, 16.0, 8.0],
        body_uv: [28.0, 8.0],
        body_center: [0.0, 10.0, 0.0],
        leg_size: [4.0, 6.0, 4.0],
        leg_uv: [0.0, 16.0],
        leg_x: 3.0,
        leg_zf: 5.0,
        leg_zb: -5.0,
        leg_top: 6.0,
    })
}

fn sheep() -> ModelDef {
    quadruped(Quad {
        tex: (64.0, 32.0),
        head_size: [6.0, 6.0, 8.0],
        head_uv: [0.0, 0.0],
        head_pivot: [0.0, 15.0, 8.0],
        head_extra: vec![],
        body_dims: [8.0, 16.0, 6.0],
        body_uv: [28.0, 8.0],
        body_center: [0.0, 15.0, 0.0],
        leg_size: [4.0, 12.0, 4.0],
        leg_uv: [0.0, 16.0],
        leg_x: 3.0,
        leg_zf: 5.0,
        leg_zb: -7.0,
        leg_top: 12.0,
    })
}

fn cow() -> ModelDef {
    // 26.1's cow texture is padded to 64×64 but keeps the classic top-32 layout.
    // Bigger quadruped: 12×18×10 body on four tall 4×12×4 legs, an 8×8×6 head
    // out front. UV offsets follow the vanilla cow texture unwrap.
    quadruped(Quad {
        tex: (64.0, 64.0),
        head_size: [8.0, 8.0, 6.0],
        head_uv: [0.0, 0.0],
        head_pivot: [0.0, 17.0, 8.0],
        // Small horns at the top-front of the head.
        head_extra: vec![
            Cube::new([-4.0, 4.0, 1.0], [1.0, 3.0, 1.0], [22.0, 0.0]),
            Cube::new([4.0, 4.0, 1.0], [1.0, 3.0, 1.0], [22.0, 0.0]),
        ],
        body_dims: [12.0, 18.0, 10.0],
        body_uv: [18.0, 4.0],
        body_center: [0.0, 13.0, 0.0],
        leg_size: [4.0, 12.0, 4.0],
        leg_uv: [0.0, 16.0],
        leg_x: 4.0,
        leg_zf: 6.0,
        leg_zb: -6.0,
        leg_top: 12.0,
    })
}

fn chicken() -> ModelDef {
    // tex 64×32. Small biped with two wings.
    let leg = |x: f32, sign: f32| Part {
        anim: PartAnim::Leg(sign),
        pivot: [x, 5.0, 0.0],
        x_rot: 0.0,
        y_rot: 0.0,
        z_rot: 0.0,
        cubes: vec![Cube::new([0.0, -2.5, 0.0], [3.0, 5.0, 3.0], [26.0, 0.0])],
    };
    let wing = |x: f32| Part {
        anim: PartAnim::Static,
        pivot: [x, 9.0, 0.0],
        x_rot: 0.0,
        y_rot: 0.0,
        z_rot: 0.0,
        cubes: vec![Cube::new([0.0, 0.0, 0.0], [1.0, 4.0, 6.0], [24.0, 13.0])],
    };
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            Part {
                anim: PartAnim::Head,
                pivot: [0.0, 9.0, 4.0],
                x_rot: 0.0,
                y_rot: 0.0,
                z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, 3.0, 0.0], [4.0, 6.0, 3.0], [0.0, 0.0]),
                    // Beak + wattle at the front of the head.
                    Cube::new([0.0, 3.0, 2.5], [4.0, 2.0, 2.0], [14.0, 0.0]),
                    Cube::new([0.0, 1.0, 2.5], [2.0, 2.0, 2.0], [14.0, 4.0]),
                ],
            },
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 8.0, 0.0],
                x_rot: FRAC_PI_2,
                y_rot: 0.0,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [6.0, 8.0, 6.0], [0.0, 9.0])],
            },
            leg(-2.0, -1.0),
            leg(2.0, 1.0),
            wing(-4.0),
            wing(4.0),
        ],
    }
}

/// Vanilla boat hull (128×64 texture): a flat bottom plus four walls. Authored
/// +Z = bow, feet at the waterline. Every box is authored straight (so the
/// standard UV unwrap matches the texture) and turned into place with the
/// baked rotations, exactly like vanilla's BoatModel does with its yaw offsets.
/// Paddles are omitted (static hull reads correctly in motion).
fn boat() -> ModelDef {
    use std::f32::consts::PI;
    ModelDef {
        tex_w: 128.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Bottom: a 28×16×3 vertical box laid flat (x_rot) and turned so
            // the 28 px length runs along Z.
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 3.0, 0.0],
                x_rot: FRAC_PI_2,
                y_rot: FRAC_PI_2,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [28.0, 16.0, 3.0], [0.0, 0.0])],
            },
            // Left wall (+X side), 28 px long, turned to run along Z.
            Part {
                anim: PartAnim::Static,
                pivot: [9.0, 4.0, 0.0],
                x_rot: 0.0,
                y_rot: -FRAC_PI_2,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 3.0, 0.0], [28.0, 6.0, 2.0], [0.0, 43.0])],
            },
            // Right wall (−X side).
            Part {
                anim: PartAnim::Static,
                pivot: [-9.0, 4.0, 0.0],
                x_rot: 0.0,
                y_rot: FRAC_PI_2,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 3.0, 0.0], [28.0, 6.0, 2.0], [0.0, 35.0])],
            },
            // Stern (back, −Z).
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 4.0, -13.0],
                x_rot: 0.0,
                y_rot: PI,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 3.0, 0.0], [18.0, 6.0, 2.0], [0.0, 19.0])],
            },
            // Bow (front, +Z).
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 4.0, 13.0],
                x_rot: 0.0,
                y_rot: 0.0,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 3.0, 0.0], [16.0, 6.0, 2.0], [0.0, 27.0])],
            },
        ],
    }
}

// ============================================================================
// Extended mob roster. Each model uses the mob's real `entity/…` texture with
// vanilla `texOffs` values copied straight into `uv` so the standard box unwrap
// maps 1:1; geometry is authored feet-up, +Z forward, in texture pixels.
// ============================================================================

/// A biped/quadruped limb (arm or leg) that swings fore/aft. `x`/`z` place the
/// pivot, `top` is its height, `size` the cuboid hanging straight down from it.
fn limb_at(sign: f32, x: f32, z: f32, top: f32, size: [f32; 3], uv: [f32; 2]) -> Part {
    Part::plain(
        PartAnim::Leg(sign),
        [x, top, z],
        vec![Cube::new([0.0, -size[1] / 2.0, 0.0], size, uv)],
    )
}

/// A biped limb at the body centre-line (`z = 0`).
fn limb(sign: f32, x: f32, top: f32, size: [f32; 3], uv: [f32; 2]) -> Part {
    limb_at(sign, x, 0.0, top, size, uv)
}

/// Spider / cave spider (64×32): a small cephalothorax + head out front, a large
/// abdomen behind, and eight legs splayed out to the sides.
fn spider() -> ModelDef {
    // Right-side legs point +X and droop down; left mirror. Four per side,
    // spread fore/aft by y_rot. Vanilla leg box 16×2×2 at texOffs(18,0).
    let leg = |sign: f32, x: f32, z: f32, yaw: f32| Part {
        anim: PartAnim::Leg(if x > 0.0 { 1.0 } else { -1.0 } * sign),
        pivot: [x, 9.0, z],
        x_rot: 0.0,
        y_rot: yaw,
        // Droop the outward leg down so the tip reaches the ground.
        z_rot: if x > 0.0 { -0.62 } else { 0.62 },
        cubes: vec![Cube::new([x.signum() * 8.0, 0.0, 0.0], [16.0, 2.0, 2.0], [18.0, 0.0])],
    };
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Head out front (+Z).
            Part::plain(PartAnim::Head, [0.0, 9.0, 4.0], vec![Cube::new([0.0, 0.0, 4.0], [8.0, 8.0, 8.0], [32.0, 4.0])]),
            // Cephalothorax (front body).
            Part::plain(PartAnim::Static, [0.0, 9.0, 2.0], vec![Cube::new([0.0, 0.0, 0.0], [6.0, 6.0, 6.0], [0.0, 0.0])]),
            // Abdomen (rear body).
            Part::plain(PartAnim::Static, [0.0, 9.0, -7.0], vec![Cube::new([0.0, 0.0, 0.0], [10.0, 8.0, 12.0], [0.0, 12.0])]),
            leg(1.0, 3.0, 2.0, FRAC_PI_4 * 1.4),
            leg(-1.0, 3.0, 1.0, FRAC_PI_4 * 0.5),
            leg(1.0, 3.0, 0.0, -FRAC_PI_4 * 0.5),
            leg(-1.0, 3.0, -1.0, -FRAC_PI_4 * 1.4),
            leg(1.0, -3.0, 2.0, -FRAC_PI_4 * 1.4),
            leg(-1.0, -3.0, 1.0, -FRAC_PI_4 * 0.5),
            leg(1.0, -3.0, 0.0, FRAC_PI_4 * 0.5),
            leg(-1.0, -3.0, -1.0, FRAC_PI_4 * 1.4),
        ],
    }
}

/// Wolf (64×32): head with snout, a flat body under a raised mane, four legs and
/// an upright tail.
fn wolf() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 8.0, [2.0, 8.0, 2.0], [0.0, 18.0]);
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Head (uv 0,0 6×6×4) + snout (uv 0,10 3×3×4) at the front.
            Part::plain(PartAnim::Head, [0.0, 12.0, 6.0], vec![
                Cube::new([0.0, 1.0, 0.0], [6.0, 6.0, 4.0], [0.0, 0.0]),
                Cube::new([0.0, -1.0, 3.0], [3.0, 3.0, 4.0], [16.0, 14.0]),
                // Ears.
                Cube::new([-2.0, 5.0, 0.0], [2.0, 2.0, 1.0], [16.0, 14.0]),
                Cube::new([2.0, 5.0, 0.0], [2.0, 2.0, 1.0], [16.0, 14.0]),
            ]),
            // Body (laid flat) + mane on top.
            Part { anim: PartAnim::Static, pivot: [0.0, 11.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [6.0, 9.0, 6.0], [18.0, 14.0])] },
            Part { anim: PartAnim::Static, pivot: [0.0, 12.0, 1.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [8.0, 7.0, 7.0], [21.0, 0.0])] },
            leg(2.0, 5.0, 1.0),
            leg(-2.0, 5.0, -1.0),
            leg(2.0, -4.0, -1.0),
            leg(-2.0, -4.0, 1.0),
            // Tail hanging down at the back.
            Part { anim: PartAnim::Static, pivot: [0.0, 12.0, -6.0], x_rot: -FRAC_PI_4, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -4.0, 0.0], [2.0, 8.0, 2.0], [9.0, 18.0])] },
        ],
    }
}

/// Fox (48×32): low body, pointed head with big ears, bushy tail.
fn fox() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 7.0, [2.0, 7.0, 2.0], [13.0, 24.0]);
    ModelDef {
        tex_w: 48.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Head (uv 1,5 8×6×6) with ears + snout.
            Part::plain(PartAnim::Head, [0.0, 9.0, 6.0], vec![
                Cube::new([0.0, 0.0, 0.0], [8.0, 6.0, 6.0], [1.0, 5.0]),
                Cube::new([-3.0, 4.0, 0.0], [2.0, 2.0, 1.0], [8.0, 1.0]),
                Cube::new([3.0, 4.0, 0.0], [2.0, 2.0, 1.0], [15.0, 1.0]),
                Cube::new([0.0, -1.0, 3.0], [4.0, 2.0, 3.0], [6.0, 18.0]),
            ]),
            // Body laid flat.
            Part { anim: PartAnim::Static, pivot: [0.0, 8.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [6.0, 11.0, 6.0], [24.0, 15.0])] },
            leg(2.0, 4.0, 1.0),
            leg(-2.0, 4.0, -1.0),
            leg(2.0, -5.0, -1.0),
            leg(-2.0, -5.0, 1.0),
            // Bushy tail.
            Part { anim: PartAnim::Static, pivot: [0.0, 8.0, -6.0], x_rot: -FRAC_PI_4 * 0.6, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -5.0, 0.0], [4.0, 11.0, 4.0], [30.0, 0.0])] },
        ],
    }
}

/// Villager (64×64): big head with brow + nose, robed body, crossed arms, legs.
fn villager() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Head + brow ridge + nose.
            Part::plain(PartAnim::Head, [0.0, 24.0, 0.0], vec![
                Cube::new([0.0, 5.0, 0.0], [8.0, 10.0, 8.0], [0.0, 0.0]),
                Cube::new([0.0, 4.0, 4.0], [8.0, 2.0, 0.0], [24.0, 0.0]),
                Cube::new([0.0, 3.0, 4.0], [2.0, 4.0, 2.0], [24.0, 0.0]),
            ]),
            // Body + robe (overlay hanging lower).
            Part::plain(PartAnim::Static, [0.0, 12.0, 0.0], vec![Cube::new([0.0, 6.0, 0.0], [8.0, 12.0, 6.0], [16.0, 20.0])]),
            Part::plain(PartAnim::Static, [0.0, 12.0, 0.0], vec![Cube { center: [0.0, 3.0, 0.0], size: [8.0, 18.0, 6.0], uv: [0.0, 38.0], inflate: 0.5 }]),
            // Crossed arms across the belly.
            Part { anim: PartAnim::Static, pivot: [0.0, 20.0, 0.0], x_rot: -0.75, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 2.0, 0.0], [8.0, 4.0, 4.0], [40.0, 38.0])] },
            // Legs.
            limb(1.0, 2.0, 12.0, [4.0, 12.0, 4.0], [0.0, 22.0]),
            limb(-1.0, -2.0, 12.0, [4.0, 12.0, 4.0], [0.0, 22.0]),
        ],
    }
}

/// Enderman (64×32): tall, slim, long thin arms and legs.
fn enderman() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Head on top (uv 0,0, 8×8×8).
            Part::plain(PartAnim::Head, [0.0, 37.0, 0.0], vec![Cube::new([0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0])]),
            // Slim torso (uv 32,16, 8×12×4).
            Part::plain(PartAnim::Static, [0.0, 25.0, 0.0], vec![Cube::new([0.0, 6.0, 0.0], [8.0, 12.0, 4.0], [32.0, 16.0])]),
            // Long thin arms hanging just outside the torso.
            limb(1.0, 5.0, 37.0, [2.0, 25.0, 2.0], [56.0, 0.0]),
            limb(-1.0, -5.0, 37.0, [2.0, 25.0, 2.0], [56.0, 0.0]),
            // Long thin legs.
            limb(1.0, 2.0, 25.0, [2.0, 25.0, 2.0], [56.0, 0.0]),
            limb(-1.0, -2.0, 25.0, [2.0, 25.0, 2.0], [56.0, 0.0]),
        ],
    }
}

/// Iron golem (128×128): bulky torso, big head with nose, long arms, thick legs.
fn iron_golem() -> ModelDef {
    ModelDef {
        tex_w: 128.0,
        tex_h: 128.0,
        scale: PX,
        parts: vec![
            // Head + nose, sitting on top of the torso.
            Part::plain(PartAnim::Head, [0.0, 28.0, 0.0], vec![
                Cube::new([0.0, 5.0, 1.0], [8.0, 10.0, 8.0], [0.0, 0.0]),
                Cube::new([0.0, 1.0, 5.0], [2.0, 4.0, 2.0], [24.0, 0.0]),
            ]),
            // Torso (broad chest) + belt band linking it to the legs.
            Part::plain(PartAnim::Static, [0.0, 14.0, 0.0], vec![
                Cube::new([0.0, 8.0, 0.0], [18.0, 12.0, 11.0], [0.0, 40.0]),
                Cube::new([0.0, 1.0, 0.0], [9.0, 5.0, 12.0], [0.0, 70.0]),
            ]),
            // Long heavy arms hanging at the sides down to the knees.
            limb(1.0, 11.0, 25.0, [4.0, 24.0, 6.0], [60.0, 41.0]),
            limb(-1.0, -11.0, 25.0, [4.0, 24.0, 6.0], [60.0, 41.0]),
            // Thick legs.
            limb(1.0, 4.0, 14.0, [6.0, 14.0, 5.0], [37.0, 0.0]),
            limb(-1.0, -4.0, 14.0, [6.0, 14.0, 5.0], [60.0, 0.0]),
        ],
    }
}

/// Squid (64×32): a rounded mantle with eight tentacles hanging below.
fn squid() -> ModelDef {
    let mut parts = vec![Part::plain(
        PartAnim::Static,
        [0.0, 8.0, 0.0],
        vec![Cube::new([0.0, 4.0, 0.0], [12.0, 12.0, 12.0], [0.0, 0.0])],
    )];
    // Eight tentacles around the lower rim.
    for i in 0..8 {
        let ang = i as f32 / 8.0 * (2.0 * PI);
        let (sx, sz) = (ang.sin(), ang.cos());
        parts.push(Part {
            anim: PartAnim::Static,
            pivot: [sx * 4.5, 6.0, sz * 4.5],
            x_rot: 0.0,
            y_rot: ang,
            z_rot: 0.0,
            cubes: vec![Cube::new([0.0, -6.0, 0.0], [2.0, 14.0, 2.0], [48.0, 0.0])],
        });
    }
    ModelDef { tex_w: 64.0, tex_h: 32.0, scale: PX, parts }
}

/// Bat (32×32): body hanging with a small head and two wings.
fn bat() -> ModelDef {
    ModelDef {
        tex_w: 32.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Head, [0.0, 10.0, 0.0], vec![
                Cube::new([0.0, 1.0, 0.0], [6.0, 6.0, 6.0], [0.0, 0.0]),
                Cube::new([-2.0, 5.0, 0.0], [3.0, 4.0, 1.0], [24.0, 0.0]),
                Cube::new([2.0, 5.0, 0.0], [3.0, 4.0, 1.0], [24.0, 0.0]),
            ]),
            Part::plain(PartAnim::Static, [0.0, 4.0, 0.0], vec![Cube::new([0.0, 3.0, 0.0], [6.0, 12.0, 6.0], [0.0, 16.0])]),
            // Wings (roll out to the side).
            Part { anim: PartAnim::Leg(1.0), pivot: [3.0, 12.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: -0.3,
                cubes: vec![Cube::new([5.0, -2.0, 0.0], [10.0, 12.0, 1.0], [14.0, 0.0])] },
            Part { anim: PartAnim::Leg(-1.0), pivot: [-3.0, 12.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.3,
                cubes: vec![Cube::new([-5.0, -2.0, 0.0], [10.0, 12.0, 1.0], [14.0, 0.0])] },
        ],
    }
}

/// Rabbit (64×64): small crouched body, tall ears, big hind feet.
fn rabbit() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Head + ears + nose.
            Part::plain(PartAnim::Head, [0.0, 6.0, 3.0], vec![
                Cube::new([0.0, 2.0, 0.0], [5.0, 4.0, 5.0], [32.0, 0.0]),
                Cube::new([-1.5, 6.0, 0.0], [2.0, 5.0, 1.0], [52.0, 0.0]),
                Cube::new([1.5, 6.0, 0.0], [2.0, 5.0, 1.0], [58.0, 0.0]),
            ]),
            // Body laid flat.
            Part { anim: PartAnim::Static, pivot: [0.0, 5.0, -1.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [6.0, 8.0, 5.0], [38.0, 15.0])] },
            // Front legs.
            limb(1.0, 1.5, 4.0, [2.0, 4.0, 2.0], [8.0, 24.0]),
            limb(-1.0, -1.5, 4.0, [2.0, 4.0, 2.0], [0.0, 24.0]),
            // Big hind feet.
            Part::plain(PartAnim::Leg(-1.0), [2.0, 4.0, -3.0], vec![Cube::new([0.0, -1.0, 1.0], [2.0, 2.0, 6.0], [26.0, 24.0])]),
            Part::plain(PartAnim::Leg(1.0), [-2.0, 4.0, -3.0], vec![Cube::new([0.0, -1.0, 1.0], [2.0, 2.0, 6.0], [16.0, 24.0])]),
        ],
    }
}

/// Horse (64×64): big body, angled neck to a long head, four tall legs, tail.
fn horse() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 12.0, [4.0, 12.0, 4.0], [48.0, 21.0]);
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Body laid flat.
            Part { anim: PartAnim::Static, pivot: [0.0, 14.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [10.0, 22.0, 10.0], [0.0, 34.0])] },
            // Neck (angled up-forward) + head + snout.
            Part { anim: PartAnim::Head, pivot: [0.0, 18.0, 6.0], x_rot: -PI / 3.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, 5.0, 0.0], [4.0, 14.0, 4.0], [0.0, 12.0]),
                    Cube::new([0.0, 12.0, 1.0], [5.0, 5.0, 7.0], [0.0, 25.0]),
                    Cube::new([0.0, 11.0, 5.0], [6.0, 5.0, 5.0], [24.0, 18.0]),
                ] },
            leg(3.0, 7.0, 1.0),
            leg(-3.0, 7.0, -1.0),
            leg(3.0, -8.0, -1.0),
            leg(-3.0, -8.0, 1.0),
            // Tail.
            Part { anim: PartAnim::Static, pivot: [0.0, 16.0, -10.0], x_rot: -FRAC_PI_4 * 0.5, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -6.0, 0.0], [3.0, 14.0, 3.0], [42.0, 36.0])] },
        ],
    }
}

/// Cat / ocelot (64×32): slim body, small head, thin tail curling back.
fn cat() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 6.0, [2.0, 6.0, 2.0], [8.0, 13.0]);
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Head + ears.
            Part::plain(PartAnim::Head, [0.0, 8.0, 6.0], vec![
                Cube::new([0.0, 0.0, 0.0], [5.0, 4.0, 5.0], [0.0, 0.0]),
                Cube::new([-1.5, 3.0, 0.0], [2.0, 2.0, 1.0], [0.0, 10.0]),
                Cube::new([1.5, 3.0, 0.0], [2.0, 2.0, 1.0], [6.0, 10.0]),
            ]),
            // Body laid flat.
            Part { anim: PartAnim::Static, pivot: [0.0, 7.0, -1.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [4.0, 10.0, 4.0], [20.0, 0.0])] },
            leg(1.5, 4.0, 1.0),
            leg(-1.5, 4.0, -1.0),
            leg(1.5, -4.0, -1.0),
            leg(-1.5, -4.0, 1.0),
            // Tail curling up-back.
            Part { anim: PartAnim::Static, pivot: [0.0, 7.0, -5.0], x_rot: -FRAC_PI_4 * 1.3, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -4.0, 0.0], [1.0, 9.0, 1.0], [0.0, 15.0])] },
        ],
    }
}

/// Snow golem (64×64): two stacked snow spheres, a pumpkin head, two stick arms.
fn snow_golem() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Pumpkin head.
            Part::plain(PartAnim::Head, [0.0, 18.0, 0.0], vec![Cube::new([0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0])]),
            // Upper snowball.
            Part::plain(PartAnim::Static, [0.0, 10.0, 0.0], vec![Cube::new([0.0, 4.0, 0.0], [10.0, 10.0, 10.0], [0.0, 16.0])]),
            // Lower snowball.
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![Cube::new([0.0, 6.0, 0.0], [12.0, 12.0, 12.0], [0.0, 36.0])]),
            // Stick arms out to the sides.
            Part { anim: PartAnim::Static, pivot: [5.0, 14.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: -0.6,
                cubes: vec![Cube::new([6.0, 0.0, 0.0], [12.0, 2.0, 2.0], [32.0, 0.0])] },
            Part { anim: PartAnim::Static, pivot: [-5.0, 14.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.6,
                cubes: vec![Cube::new([-6.0, 0.0, 0.0], [12.0, 2.0, 2.0], [32.0, 0.0])] },
        ],
    }
}

/// Turtle (128×64): a big flat shell, small head, four flippers.
fn turtle() -> ModelDef {
    ModelDef {
        tex_w: 128.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Head, [0.0, 4.0, 10.0], vec![Cube::new([0.0, 0.0, 2.0], [6.0, 5.0, 6.0], [3.0, 0.0])]),
            // Shell (laid flat, wide).
            Part { anim: PartAnim::Static, pivot: [0.0, 3.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [20.0, 24.0, 8.0], [7.0, 37.0])] },
            // Flippers.
            limb(1.0, 9.0, 2.0, [8.0, 2.0, 8.0], [70.0, 33.0]),
            limb(-1.0, -9.0, 2.0, [8.0, 2.0, 8.0], [70.0, 33.0]),
            limb(-1.0, 6.0, 2.0, [6.0, 2.0, 6.0], [74.0, 23.0]),
            limb(1.0, -6.0, 2.0, [6.0, 2.0, 6.0], [74.0, 23.0]),
        ],
    }
}

/// Goat (64×64): quadruped with a head, horns and a small beard.
fn goat() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 10.0, [3.0, 10.0, 3.0], [0.0, 44.0]);
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Head + horns + beard.
            Part::plain(PartAnim::Head, [0.0, 15.0, 7.0], vec![
                Cube::new([0.0, 0.0, 1.0], [5.0, 6.0, 7.0], [34.0, 0.0]),
                Cube::new([-3.0, 5.0, -2.0], [1.0, 4.0, 1.0], [50.0, 0.0]),
                Cube::new([3.0, 5.0, -2.0], [1.0, 4.0, 1.0], [50.0, 0.0]),
                Cube::new([0.0, -3.0, 2.0], [2.0, 3.0, 1.0], [58.0, 0.0]),
            ]),
            Part { anim: PartAnim::Static, pivot: [0.0, 13.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [7.0, 16.0, 8.0], [1.0, 13.0])] },
            leg(2.0, 5.0, 1.0),
            leg(-2.0, 5.0, -1.0),
            leg(2.0, -6.0, -1.0),
            leg(-2.0, -6.0, 1.0),
        ],
    }
}

/// Panda (64×64): a chunky bear body, big round head with ears.
fn panda() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 9.0, [5.0, 9.0, 5.0], [40.0, 16.0]);
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Head, [0.0, 13.0, 8.0], vec![
                Cube::new([0.0, 2.0, 1.0], [10.0, 10.0, 9.0], [0.0, 6.0]),
                Cube::new([-5.0, 8.0, 0.0], [3.0, 3.0, 1.0], [26.0, 0.0]),
                Cube::new([5.0, 8.0, 0.0], [3.0, 3.0, 1.0], [26.0, 0.0]),
            ]),
            Part { anim: PartAnim::Static, pivot: [0.0, 12.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [12.0, 18.0, 10.0], [0.0, 25.0])] },
            leg(4.0, 6.0, 1.0),
            leg(-4.0, 6.0, -1.0),
            leg(4.0, -7.0, -1.0),
            leg(-4.0, -7.0, 1.0),
        ],
    }
}

/// Polar bear (128×64): large bear, broad body, four heavy legs.
fn polar_bear() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32, uv: [f32; 2]| limb_at(sign, x, z, 10.0, [5.0, 10.0, 6.0], uv);
    ModelDef {
        tex_w: 128.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Head, [0.0, 14.0, 10.0], vec![
                Cube::new([0.0, 1.0, 1.0], [7.0, 7.0, 7.0], [0.0, 44.0]),
                Cube::new([0.0, -1.0, 5.0], [5.0, 3.0, 4.0], [0.0, 44.0]),
                Cube::new([-2.0, 6.0, 0.0], [2.0, 2.0, 1.0], [26.0, 0.0]),
                Cube::new([2.0, 6.0, 0.0], [2.0, 2.0, 1.0], [26.0, 0.0]),
            ]),
            Part { anim: PartAnim::Static, pivot: [0.0, 13.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [14.0, 22.0, 11.0], [0.0, 19.0])] },
            leg(5.0, 8.0, 1.0, [50.0, 22.0]),
            leg(-5.0, 8.0, -1.0, [50.0, 22.0]),
            leg(5.0, -8.0, -1.0, [50.0, 22.0]),
            leg(-5.0, -8.0, 1.0, [50.0, 22.0]),
        ],
    }
}

/// Llama (128×64): tall body, long neck up to a head with ears, four legs.
fn llama() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 14.0, [4.0, 14.0, 4.0], [29.0, 29.0]);
    ModelDef {
        tex_w: 128.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Neck (up) + head + ears.
            Part { anim: PartAnim::Head, pivot: [0.0, 18.0, 6.0], x_rot: -FRAC_PI_4 * 0.4, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, 7.0, 0.0], [4.0, 16.0, 4.0], [0.0, 0.0]),
                    Cube::new([0.0, 16.0, 3.0], [5.0, 5.0, 8.0], [0.0, 14.0]),
                    Cube::new([-2.0, 20.0, 0.0], [2.0, 3.0, 1.0], [17.0, 0.0]),
                    Cube::new([2.0, 20.0, 0.0], [2.0, 3.0, 1.0], [17.0, 0.0]),
                ] },
            Part { anim: PartAnim::Static, pivot: [0.0, 16.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [9.0, 18.0, 12.0], [29.0, 0.0])] },
            leg(3.5, 6.0, 1.0),
            leg(-3.5, 6.0, -1.0),
            leg(3.5, -6.0, -1.0),
            leg(-3.5, -6.0, 1.0),
        ],
    }
}
