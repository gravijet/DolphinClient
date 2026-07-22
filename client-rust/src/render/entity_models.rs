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

use std::f32::consts::FRAC_PI_2;

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
    pub cubes: Vec<Cube>,
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
}

impl MobModel {
    pub fn all() -> [MobModel; 7] {
        use MobModel::*;
        [Creeper, Pig, Sheep, Chicken, Cow, Boat, Slime]
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
        cubes: vec![Cube::new([0.0, -q.leg_size[1] / 2.0, 0.0], q.leg_size, q.leg_uv)],
    };
    let mut head_cubes = vec![Cube::new([0.0, 0.0, 0.0], q.head_size, q.head_uv)];
    head_cubes.extend(q.head_extra);
    ModelDef {
        tex_w: q.tex.0,
        tex_h: q.tex.1,
        scale: PX,
        parts: vec![
            Part { anim: PartAnim::Head, pivot: q.head_pivot, x_rot: 0.0, y_rot: 0.0, cubes: head_cubes },
            // Body: vertical box laid flat (long axis → +Z) so the UV unwrap
            // matches vanilla.
            Part {
                anim: PartAnim::Static,
                pivot: q.body_center,
                x_rot: FRAC_PI_2,
                y_rot: 0.0,
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
                cubes: vec![Cube::new([0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0])],
            },
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 0.0, 0.0],
                x_rot: 0.0,
                y_rot: 0.0,
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
        cubes: vec![Cube::new([0.0, -2.5, 0.0], [3.0, 5.0, 3.0], [26.0, 0.0])],
    };
    let wing = |x: f32| Part {
        anim: PartAnim::Static,
        pivot: [x, 9.0, 0.0],
        x_rot: 0.0,
        y_rot: 0.0,
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
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [28.0, 16.0, 3.0], [0.0, 0.0])],
            },
            // Left wall (+X side), 28 px long, turned to run along Z.
            Part {
                anim: PartAnim::Static,
                pivot: [9.0, 4.0, 0.0],
                x_rot: 0.0,
                y_rot: -FRAC_PI_2,
                cubes: vec![Cube::new([0.0, 3.0, 0.0], [28.0, 6.0, 2.0], [0.0, 43.0])],
            },
            // Right wall (−X side).
            Part {
                anim: PartAnim::Static,
                pivot: [-9.0, 4.0, 0.0],
                x_rot: 0.0,
                y_rot: FRAC_PI_2,
                cubes: vec![Cube::new([0.0, 3.0, 0.0], [28.0, 6.0, 2.0], [0.0, 35.0])],
            },
            // Stern (back, −Z).
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 4.0, -13.0],
                x_rot: 0.0,
                y_rot: PI,
                cubes: vec![Cube::new([0.0, 3.0, 0.0], [18.0, 6.0, 2.0], [0.0, 19.0])],
            },
            // Bow (front, +Z).
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 4.0, 13.0],
                x_rot: 0.0,
                y_rot: 0.0,
                cubes: vec![Cube::new([0.0, 3.0, 0.0], [16.0, 6.0, 2.0], [0.0, 27.0])],
            },
        ],
    }
}
