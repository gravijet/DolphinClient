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
    Ghast,
    Blaze,
    Dolphin,
    Guardian,
    Cod,
    Salmon,
    Bee,
    Silverfish,
    Parrot,
    Phantom,
    // 0.38.0 — bestiary expansion: the last big batch of overworld/nether/
    // deep-dark mobs that used to render as plain tinted boxes.
    Axolotl,
    Frog,
    Tadpole,
    Camel,
    Sniffer,
    Armadillo,
    Allay,
    Vex,
    Endermite,
    Pufferfish,
    Illager,
    Witch,
    Strider,
    Hoglin,
    Ravager,
    Warden,
    Creaking,
    Breeze,
    // 0.39.0 — the last entities: the two bosses and the two special mobs that
    // still fell back to a box.
    EnderDragon,
    Wither,
    Shulker,
    ArmorStand,
    EndCrystal,
    // 0.47.0 — minecarts (the boat model already covers boats/rafts).
    Minecart,
    // 0.50.0 — tropical fish come in two body shapes (A "kob"/small, B large/
    // flat); the app picks the shape from the packed variant and draws a tinted
    // base body + a tinted pattern overlay on the same model.
    TropicalFishA,
    TropicalFishB,
    // 0.52.0 — block entities. These are not mobs, but they are exactly what
    // this table is for: a rigid stack of textured cuboids drawn from an
    // `entity/…` PNG. The app places them on the block and never animates the
    // limbs, except for the bell's swing and the banner's sway.
    /// Standing banner: cloth, pole and crossbar (vanilla `BannerModel`).
    Banner,
    /// Wall banner: the same cloth, hung from a short bar, no pole.
    BannerWall,
    /// A mob head: one 8³ cube filling the lower half of its block.
    Skull,
    /// A player head: the same cube plus the skin's second head layer.
    PlayerHead,
    /// Piglin head: wider skull with a snout and the two floppy ears.
    SkullPiglin,
    /// Dragon head: the ender dragon's head, jaw and horns at head scale.
    SkullDragon,
    /// Conduit: the 6³ shell (open or closed is a texture swap).
    Conduit,
    /// Bell: the hanging gold body plus its top plate.
    Bell,
    /// Decorated pot: neck, body and foot; the sherds ride on the body's sides.
    DecoratedPot,
}

impl MobModel {
    pub fn all() -> [MobModel; 69] {
        use MobModel::*;
        [
            Creeper, Pig, Sheep, Chicken, Cow, Boat, Slime, Spider, Wolf, Fox, Villager,
            Enderman, IronGolem, Squid, Bat, Rabbit, Horse, Cat, SnowGolem, Turtle, Goat,
            Panda, PolarBear, Llama, Ghast, Blaze, Dolphin, Guardian, Cod, Salmon, Bee,
            Silverfish, Parrot, Phantom,
            Axolotl, Frog, Tadpole, Camel, Sniffer, Armadillo, Allay, Vex, Endermite,
            Pufferfish, Illager, Witch, Strider, Hoglin, Ravager, Warden, Creaking, Breeze,
            EnderDragon, Wither, Shulker, ArmorStand, EndCrystal, Minecart,
            TropicalFishA, TropicalFishB,
            Banner, BannerWall, Skull, PlayerHead, SkullPiglin, SkullDragon, Conduit, Bell,
            DecoratedPot,
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
        MobModel::Minecart => minecart(),
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
        MobModel::Ghast => ghast(),
        MobModel::Blaze => blaze(),
        MobModel::Dolphin => dolphin(),
        MobModel::Guardian => guardian(),
        MobModel::Cod => cod(),
        MobModel::Salmon => salmon(),
        MobModel::Bee => bee(),
        MobModel::Silverfish => silverfish(),
        MobModel::Parrot => parrot(),
        MobModel::Phantom => phantom(),
        MobModel::Axolotl => axolotl(),
        MobModel::Frog => frog(),
        MobModel::Tadpole => tadpole(),
        MobModel::Camel => camel(),
        MobModel::Sniffer => sniffer(),
        MobModel::Armadillo => armadillo(),
        MobModel::Allay => allay(),
        MobModel::Vex => vex(),
        MobModel::Endermite => endermite(),
        MobModel::Pufferfish => pufferfish(),
        MobModel::Illager => illager(),
        MobModel::Witch => witch(),
        MobModel::Strider => strider(),
        MobModel::Hoglin => hoglin(),
        MobModel::Ravager => ravager(),
        MobModel::Warden => warden(),
        MobModel::Creaking => creaking(),
        MobModel::Breeze => breeze(),
        MobModel::EnderDragon => ender_dragon(),
        MobModel::Wither => wither(),
        MobModel::Shulker => shulker(),
        MobModel::ArmorStand => armor_stand(),
        MobModel::EndCrystal => end_crystal(),
        MobModel::TropicalFishA => tropical_fish_a(),
        MobModel::TropicalFishB => tropical_fish_b(),
        MobModel::Banner => banner(true),
        MobModel::BannerWall => banner(false),
        MobModel::Skull => skull(false),
        MobModel::PlayerHead => skull(true),
        MobModel::SkullPiglin => skull_piglin(),
        MobModel::SkullDragon => skull_dragon(),
        MobModel::Conduit => conduit(),
        MobModel::Bell => bell(),
        MobModel::DecoratedPot => decorated_pot(),
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

/// Minecart: an open box (20×16 base + four 8-tall walls), matching the vanilla
/// `MinecartModel` on the 64×32 texture. All four walls reuse the same wall
/// region (texOffs 0,0), the base uses texOffs 0,10 — like vanilla.
fn minecart() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Base plate: a 20×16×2 vertical box laid flat so 16 runs along Z.
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 2.0, 0.0],
                x_rot: FRAC_PI_2,
                y_rot: 0.0,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [20.0, 16.0, 2.0], [0.0, 10.0])],
            },
            // Front wall (+Z), spanning the 20 px length.
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 2.0, 8.0],
                x_rot: 0.0,
                y_rot: 0.0,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 4.0, 0.0], [20.0, 8.0, 2.0], [0.0, 0.0])],
            },
            // Back wall (−Z).
            Part {
                anim: PartAnim::Static,
                pivot: [0.0, 2.0, -8.0],
                x_rot: 0.0,
                y_rot: PI,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 4.0, 0.0], [20.0, 8.0, 2.0], [0.0, 0.0])],
            },
            // Left wall (+X), turned so its 16 px length runs along Z.
            Part {
                anim: PartAnim::Static,
                pivot: [10.0, 2.0, 0.0],
                x_rot: 0.0,
                y_rot: FRAC_PI_2,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 4.0, 0.0], [16.0, 8.0, 2.0], [0.0, 0.0])],
            },
            // Right wall (−X).
            Part {
                anim: PartAnim::Static,
                pivot: [-10.0, 2.0, 0.0],
                x_rot: 0.0,
                y_rot: -FRAC_PI_2,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 4.0, 0.0], [16.0, 8.0, 2.0], [0.0, 0.0])],
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

/// Ghast (128×64): a big floating cube body with nine hanging tentacles. Authored
/// large (its `scale` makes it ~4 blocks).
fn ghast() -> ModelDef {
    let mut parts = vec![Part::plain(
        PartAnim::Static,
        [0.0, 0.0, 0.0],
        vec![Cube::new([0.0, 14.0, 0.0], [16.0, 16.0, 16.0], [0.0, 0.0])],
    )];
    // Nine tentacles in a 3×3 grid under the body, hanging down.
    for i in 0..9 {
        let (gx, gz) = ((i % 3) as f32 - 1.0, (i / 3) as f32 - 1.0);
        let len = 6.0 + (i % 3) as f32 * 3.0;
        parts.push(Part::plain(
            PartAnim::Static,
            [gx * 5.0, 6.0, gz * 5.0],
            vec![Cube::new([0.0, -len / 2.0, 0.0], [2.0, len, 2.0], [0.0, 0.0])],
        ));
    }
    ModelDef { tex_w: 128.0, tex_h: 64.0, scale: PX * 2.6, parts }
}

/// Blaze (64×32): a floating head ringed by twelve rotating rods.
fn blaze() -> ModelDef {
    let mut parts = vec![Part::plain(
        PartAnim::Head,
        [0.0, 14.0, 0.0],
        vec![Cube::new([0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0])],
    )];
    for i in 0..12 {
        let ang = i as f32 / 12.0 * (2.0 * PI);
        let (sx, sz) = (ang.sin() * 5.0, ang.cos() * 5.0);
        let y = 6.0 + (i % 3) as f32 * 3.0;
        parts.push(Part::plain(
            PartAnim::Static,
            [sx, y, sz],
            vec![Cube::new([0.0, 0.0, 0.0], [2.0, 8.0, 2.0], [0.0, 16.0])],
        ));
    }
    ModelDef { tex_w: 64.0, tex_h: 32.0, scale: PX, parts }
}

/// Dolphin (64×64): a streamlined body with a head, tail fin and dorsal fin.
fn dolphin() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Body along +Z.
            Part::plain(PartAnim::Static, [0.0, 5.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [8.0, 7.0, 13.0], [22.0, 0.0])]),
            // Head at the front.
            Part::plain(PartAnim::Head, [0.0, 5.0, 6.0], vec![
                Cube::new([0.0, 0.0, 2.0], [8.0, 7.0, 6.0], [0.0, 0.0]),
                Cube::new([0.0, -1.0, 6.0], [4.0, 3.0, 4.0], [0.0, 13.0]),
            ]),
            // Tail + vertical tail fin.
            Part { anim: PartAnim::Static, pivot: [0.0, 5.0, -6.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, -3.0], [7.0, 3.0, 6.0], [0.0, 20.0])] },
            Part { anim: PartAnim::Static, pivot: [0.0, 5.0, -10.0], x_rot: FRAC_PI_2, y_rot: FRAC_PI_2, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [9.0, 1.0, 4.0], [29.0, 0.0])] },
            // Dorsal fin on top.
            Part { anim: PartAnim::Static, pivot: [0.0, 9.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 2.0, 0.0], [1.0, 4.0, 4.0], [51.0, 0.0])] },
        ],
    }
}

/// A small fish body + tail fin (cod / salmon share the shape, differ in size).
fn fish(len: f32, uv_body: [f32; 2], uv_tail: [f32; 2], tex: (f32, f32)) -> ModelDef {
    ModelDef {
        tex_w: tex.0,
        tex_h: tex.1,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Head, [0.0, 3.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [2.0, 4.0, len], uv_body)]),
            // Vertical tail fin.
            Part { anim: PartAnim::Static, pivot: [0.0, 3.0, -len / 2.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, -2.0], [1.0, 4.0, 4.0], uv_tail)] },
        ],
    }
}

fn cod() -> ModelDef {
    fish(7.0, [0.0, 0.0], [22.0, 3.0], (32.0, 32.0))
}

fn salmon() -> ModelDef {
    fish(12.0, [0.0, 0.0], [20.0, 0.0], (32.0, 32.0))
}

/// Tropical fish, small "kob" body (shape A, tex 32×32). The body box UV
/// (texOffs 0,0, size 2×3×6) matches `tropical_a.png`; the app draws it twice —
/// once with the base body texture tinted by the body colour, once with a
/// pattern texture tinted by the pattern colour — so any of the 65k variants
/// renders correctly. Head +Z, tail −Z (this codebase's convention).
fn tropical_fish_a() -> ModelDef {
    ModelDef {
        tex_w: 32.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Flattened body.
            Part::plain(PartAnim::Head, [0.0, 3.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [2.0, 3.0, 6.0], [0.0, 0.0])]),
            // Vertical tail fin.
            Part::plain(PartAnim::Static, [0.0, 3.0, -3.0], vec![Cube::new([0.0, 0.0, -2.0], [1.0, 3.0, 4.0], [22.0, 3.0])]),
            // Dorsal fin along the back.
            Part::plain(PartAnim::Static, [0.0, 4.5, 0.0], vec![Cube::new([0.0, 1.0, 0.0], [0.0, 2.0, 4.0], [10.0, 16.0])]),
        ],
    }
}

/// Tropical fish, large flat body (shape B, tex 32×32). Body box UV
/// (texOffs 0,20, size 2×6×6) matches `tropical_b.png`. Same two-pass tint
/// draw as shape A, plus a bottom (anal) fin.
fn tropical_fish_b() -> ModelDef {
    ModelDef {
        tex_w: 32.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Tall flat body.
            Part::plain(PartAnim::Head, [0.0, 4.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [2.0, 6.0, 6.0], [0.0, 20.0])]),
            // Vertical tail fin.
            Part::plain(PartAnim::Static, [0.0, 4.0, -3.0], vec![Cube::new([0.0, 0.0, -2.0], [1.0, 6.0, 4.0], [21.0, 16.0])]),
            // Dorsal fin (top), tucked against the back.
            Part::plain(PartAnim::Static, [0.0, 7.0, 0.0], vec![Cube::new([0.0, 1.0, 0.0], [0.0, 3.0, 6.0], [20.0, 10.0])]),
        ],
    }
}

/// Bee (64×64): a striped body with a head, two wings and a stinger.
fn bee() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Static, [0.0, 5.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [7.0, 7.0, 10.0], [0.0, 0.0])]),
            // Head at the front + antennae.
            Part::plain(PartAnim::Head, [0.0, 5.0, 5.0], vec![Cube::new([0.0, 0.0, 2.0], [7.0, 7.0, 6.0], [0.0, 17.0])]),
            // Stinger at the back.
            Part::plain(PartAnim::Static, [0.0, 4.0, -5.0], vec![Cube::new([0.0, 0.0, -1.0], [1.0, 1.0, 2.0], [26.0, 5.0])]),
            // Wings (translucent in vanilla; drawn opaque here).
            Part { anim: PartAnim::Static, pivot: [1.5, 9.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: -0.2,
                cubes: vec![Cube::new([4.0, 0.0, -1.0], [9.0, 0.0, 6.0], [0.0, 0.0])] },
            Part { anim: PartAnim::Static, pivot: [-1.5, 9.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.2,
                cubes: vec![Cube::new([-4.0, 0.0, -1.0], [9.0, 0.0, 6.0], [0.0, 0.0])] },
        ],
    }
}

/// Silverfish (64×32): a low body of overlapping segments that taper to the tail.
fn silverfish() -> ModelDef {
    let seg = |z: f32, w: f32, h: f32, uv: [f32; 2]| {
        Part::plain(PartAnim::Static, [0.0, h / 2.0, z], vec![Cube::new([0.0, 0.0, 0.0], [w, h, w], uv)])
    };
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            seg(5.0, 4.0, 3.0, [0.0, 0.0]),
            seg(2.0, 6.0, 4.0, [0.0, 4.0]),
            seg(-2.0, 5.0, 3.0, [20.0, 0.0]),
            seg(-5.0, 3.0, 2.0, [20.0, 4.0]),
            seg(-7.0, 2.0, 1.0, [20.0, 7.0]),
        ],
    }
}

/// Guardian (64×64): a bulky body with a single eye, spikes and a tail.
fn guardian() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Main body.
            Part::plain(PartAnim::Head, [0.0, 8.0, 0.0], vec![
                Cube::new([0.0, 0.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0]),
                // The single eye on the front.
                Cube::new([0.0, 0.0, 4.0], [2.0, 2.0, 1.0], [8.0, 0.0]),
            ]),
            // Tail.
            Part { anim: PartAnim::Static, pivot: [0.0, 8.0, -4.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, 0.0, -2.0], [3.0, 3.0, 5.0], [40.0, 0.0]),
                    Cube::new([0.0, 0.0, -6.0], [1.0, 4.0, 4.0], [0.0, 54.0]),
                ] },
            // Four spikes around the body.
            Part { anim: PartAnim::Static, pivot: [5.0, 8.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: -FRAC_PI_2,
                cubes: vec![Cube::new([3.0, 0.0, 0.0], [2.0, 8.0, 2.0], [0.0, 20.0])] },
            Part { anim: PartAnim::Static, pivot: [-5.0, 8.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: FRAC_PI_2,
                cubes: vec![Cube::new([-3.0, 0.0, 0.0], [2.0, 8.0, 2.0], [0.0, 20.0])] },
            Part { anim: PartAnim::Static, pivot: [0.0, 13.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 3.0, 0.0], [2.0, 8.0, 2.0], [0.0, 20.0])] },
        ],
    }
}

/// Parrot (32×32): a small perched bird — body, head, tail and two wings.
fn parrot() -> ModelDef {
    ModelDef {
        tex_w: 32.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Static, [0.0, 4.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [3.0, 6.0, 3.0], [2.0, 8.0])]),
            // Head + beak.
            Part::plain(PartAnim::Head, [0.0, 9.0, 0.0], vec![
                Cube::new([0.0, 1.0, 0.0], [2.0, 3.0, 2.0], [10.0, 0.0]),
                Cube::new([0.0, 0.0, 1.0], [1.0, 2.0, 1.0], [11.0, 7.0]),
            ]),
            // Tail.
            Part { anim: PartAnim::Static, pivot: [0.0, 2.0, -1.0], x_rot: FRAC_PI_4 * 0.5, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -3.0, 0.0], [3.0, 4.0, 1.0], [22.0, 1.0])] },
            // Wings.
            Part::plain(PartAnim::Static, [2.0, 5.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [1.0, 5.0, 3.0], [19.0, 8.0])]),
            Part::plain(PartAnim::Static, [-2.0, 5.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [1.0, 5.0, 3.0], [19.0, 8.0])]),
        ],
    }
}

/// Phantom (64×64): a flat flying body with wide membrane wings and a tail.
fn phantom() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Head, [0.0, 4.0, 0.0], vec![
                Cube::new([0.0, 0.0, 0.0], [5.0, 3.0, 9.0], [0.0, 8.0]),
                // Head at the front.
                Cube::new([0.0, 0.0, 6.0], [3.0, 2.0, 3.0], [0.0, 0.0]),
            ]),
            // Wings, sloping out and down.
            Part { anim: PartAnim::Leg(1.0), pivot: [2.0, 4.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: -0.3,
                cubes: vec![
                    Cube::new([6.0, 0.0, -1.0], [13.0, 1.0, 7.0], [23.0, 12.0]),
                    Cube::new([16.0, 0.0, 1.0], [8.0, 1.0, 4.0], [16.0, 24.0]),
                ] },
            Part { anim: PartAnim::Leg(-1.0), pivot: [-2.0, 4.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.3,
                cubes: vec![
                    Cube::new([-6.0, 0.0, -1.0], [13.0, 1.0, 7.0], [23.0, 12.0]),
                    Cube::new([-16.0, 0.0, 1.0], [8.0, 1.0, 4.0], [16.0, 24.0]),
                ] },
            // Tail.
            Part { anim: PartAnim::Static, pivot: [0.0, 4.0, -4.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, -3.0], [2.0, 2.0, 6.0], [3.0, 20.0])] },
        ],
    }
}

// --- 0.38.0 bestiary expansion ----------------------------------------------
// Overworld, nether and deep-dark mobs that used to fall back to a plain box.
// Geometry follows the vanilla proportions; the standard box UV unwrap lands on
// the mob's real texture (dims confirmed from the 26.1 jar).

/// Axolotl (64×64): a small salamander — flat body, head with three top gill
/// fronds, four stubby legs and a tall tail fin.
fn axolotl() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 3.0, [2.0, 3.0, 1.0], [2.0, 16.0]);
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Head + three flat gill fronds fanning off the top and sides.
            Part::plain(PartAnim::Head, [0.0, 4.0, 4.0], vec![
                Cube::new([0.0, 0.0, 1.0], [5.0, 4.0, 5.0], [0.0, 0.0]),
                Cube::new([0.0, 3.0, -1.0], [0.0, 3.0, 4.0], [11.0, 4.0]),
                Cube::new([-3.0, 1.0, -1.0], [0.0, 2.0, 3.0], [11.0, 0.0]),
                Cube::new([3.0, 1.0, -1.0], [0.0, 2.0, 3.0], [11.0, 0.0]),
            ]),
            // Body laid flat.
            Part { anim: PartAnim::Static, pivot: [0.0, 4.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [5.0, 10.0, 4.0], [11.0, 15.0])] },
            leg(2.0, 3.0, 1.0),
            leg(-2.0, 3.0, -1.0),
            leg(2.0, -3.0, -1.0),
            leg(-2.0, -3.0, 1.0),
            // Tall flat tail fin at the back.
            Part { anim: PartAnim::Static, pivot: [0.0, 4.0, -5.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, -3.0], [0.0, 7.0, 7.0], [2.0, 13.0])] },
        ],
    }
}

/// Frog (48×48): a low flat body, a wide head with two bulging eyes on top, two
/// small front legs and two large folded hind legs.
fn frog() -> ModelDef {
    ModelDef {
        tex_w: 48.0,
        tex_h: 48.0,
        scale: PX,
        parts: vec![
            // Body laid flat.
            Part { anim: PartAnim::Static, pivot: [0.0, 3.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [7.0, 9.0, 3.0], [3.0, 1.0])] },
            // Head with two eye bumps.
            Part::plain(PartAnim::Head, [0.0, 3.0, 4.0], vec![
                Cube::new([0.0, -1.0, 0.0], [7.0, 2.0, 5.0], [23.0, 25.0]),
                Cube::new([-2.0, 1.0, -1.0], [2.0, 2.0, 2.0], [0.0, 16.0]),
                Cube::new([2.0, 1.0, -1.0], [2.0, 2.0, 2.0], [0.0, 16.0]),
            ]),
            // Small front legs.
            limb(1.0, 3.0, 3.0, [1.0, 3.0, 1.0], [14.0, 25.0]),
            limb(-1.0, -3.0, 3.0, [1.0, 3.0, 1.0], [14.0, 25.0]),
            // Big folded hind legs.
            Part::plain(PartAnim::Leg(-1.0), [3.0, 3.0, -3.0], vec![Cube::new([0.0, -1.0, 0.0], [2.0, 2.0, 5.0], [24.0, 17.0])]),
            Part::plain(PartAnim::Leg(1.0), [-3.0, 3.0, -3.0], vec![Cube::new([0.0, -1.0, 0.0], [2.0, 2.0, 5.0], [24.0, 17.0])]),
        ],
    }
}

/// Tadpole (16×16): a tiny head-blob with a flat swishing tail.
fn tadpole() -> ModelDef {
    ModelDef {
        tex_w: 16.0,
        tex_h: 16.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Head, [0.0, 2.0, 1.0], vec![Cube::new([0.0, 0.0, 0.0], [1.0, 3.0, 4.0], [0.0, 0.0])]),
            Part { anim: PartAnim::Static, pivot: [0.0, 2.0, -1.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, -2.0], [0.0, 3.0, 5.0], [3.0, 1.0])] },
        ],
    }
}

/// Camel (128×128): a tall body with a back hump, a long neck rising to a head
/// with ears, four tall legs and a tail.
fn camel() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 21.0, [4.0, 21.0, 4.0], [58.0, 17.0]);
    ModelDef {
        tex_w: 128.0,
        tex_h: 128.0,
        scale: PX,
        parts: vec![
            // Body laid flat.
            Part { anim: PartAnim::Static, pivot: [0.0, 21.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [14.0, 24.0, 12.0], [0.0, 25.0])] },
            // Hump on the back.
            Part::plain(PartAnim::Static, [0.0, 27.0, 0.0], vec![Cube::new([0.0, 0.0, -2.0], [7.0, 6.0, 7.0], [74.0, 0.0])]),
            // Neck angled up-forward + head + ears.
            Part { anim: PartAnim::Head, pivot: [0.0, 24.0, 6.0], x_rot: -FRAC_PI_4 * 1.1, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, 7.0, 0.0], [7.0, 16.0, 7.0], [45.0, 58.0]),
                    Cube::new([0.0, 15.0, 3.0], [7.0, 8.0, 12.0], [60.0, 24.0]),
                    Cube::new([-3.0, 21.0, 0.0], [2.0, 3.0, 1.0], [45.0, 81.0]),
                    Cube::new([3.0, 21.0, 0.0], [2.0, 3.0, 1.0], [45.0, 81.0]),
                ] },
            leg(4.0, 7.0, 1.0),
            leg(-4.0, 7.0, -1.0),
            leg(4.0, -7.0, -1.0),
            leg(-4.0, -7.0, 1.0),
            // Tail.
            Part { anim: PartAnim::Static, pivot: [0.0, 20.0, -6.0], x_rot: -0.2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -6.0, 0.0], [2.0, 10.0, 1.0], [122.0, 12.0])] },
        ],
    }
}

/// Sniffer (192×192): a large low-slung body (dark-red flanks, mossy back), a
/// big snouted head and four short legs. UVs picked from the real texture: the
/// face sits top-left, the body sides in the dark-red block mid-right, the legs
/// bottom-left.
fn sniffer() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 12.0, [7.0, 12.0, 7.0], [0.0, 96.0]);
    ModelDef {
        tex_w: 192.0,
        tex_h: 192.0,
        scale: PX,
        parts: vec![
            // Body laid flat (uv in the dark-red flank block).
            Part { anim: PartAnim::Static, pivot: [0.0, 16.0, -2.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [22.0, 40.0, 24.0], [64.0, 64.0])] },
            // Head + broad snout (uv on the face region, top-left).
            Part::plain(PartAnim::Head, [0.0, 18.0, 20.0], vec![
                Cube::new([0.0, 2.0, 0.0], [20.0, 16.0, 18.0], [0.0, 0.0]),
                Cube::new([0.0, -3.0, 8.0], [14.0, 6.0, 6.0], [0.0, 42.0]),
            ]),
            leg(7.0, 13.0, 1.0),
            leg(-7.0, 13.0, -1.0),
            leg(7.0, -13.0, -1.0),
            leg(-7.0, -13.0, 1.0),
        ],
    }
}

/// Armadillo (64×64): a domed shell body, a small head with ears, four short
/// legs and a stubby tail.
fn armadillo() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 4.0, [3.0, 3.0, 3.0], [0.0, 32.0]);
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Shell body laid flat.
            Part { anim: PartAnim::Static, pivot: [0.0, 4.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 1.0, -1.0], [9.0, 11.0, 8.0], [0.0, 20.0])] },
            // Head + ears.
            Part::plain(PartAnim::Head, [0.0, 3.0, 5.0], vec![
                Cube::new([0.0, 0.0, 1.0], [4.0, 3.0, 3.0], [43.0, 10.0]),
                Cube::new([-2.0, 3.0, 0.0], [1.0, 2.0, 1.0], [43.0, 0.0]),
                Cube::new([2.0, 3.0, 0.0], [1.0, 2.0, 1.0], [43.0, 0.0]),
            ]),
            leg(3.0, 3.0, 1.0),
            leg(-3.0, 3.0, -1.0),
            leg(3.0, -3.0, -1.0),
            leg(-3.0, -3.0, 1.0),
            // Tail.
            Part { anim: PartAnim::Static, pivot: [0.0, 3.0, -6.0], x_rot: -0.4, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -3.0, 0.0], [1.0, 5.0, 1.0], [38.0, 50.0])] },
        ],
    }
}

/// Allay (32×32): a small floating sprite — round head, tiny body, thin arms and
/// two long membrane wings that flap.
fn allay() -> ModelDef {
    ModelDef {
        tex_w: 32.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Head, [0.0, 7.0, 0.0], vec![Cube::new([0.0, 1.0, 0.0], [3.0, 3.0, 3.0], [0.0, 0.0])]),
            Part::plain(PartAnim::Static, [0.0, 3.0, 0.0], vec![Cube::new([0.0, 1.0, 0.0], [3.0, 4.0, 2.0], [0.0, 6.0])]),
            // Thin arms.
            Part { anim: PartAnim::Static, pivot: [2.0, 6.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: -0.1,
                cubes: vec![Cube::new([0.0, -2.0, 0.0], [1.0, 4.0, 1.0], [23.0, 0.0])] },
            Part { anim: PartAnim::Static, pivot: [-2.0, 6.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.1,
                cubes: vec![Cube::new([0.0, -2.0, 0.0], [1.0, 4.0, 1.0], [23.0, 0.0])] },
            // Wings behind (flap via the Leg animation channel).
            Part { anim: PartAnim::Leg(1.0), pivot: [0.5, 8.0, 1.5], x_rot: 0.0, y_rot: -0.35, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -4.0, 0.0], [0.0, 7.0, 4.0], [16.0, 14.0])] },
            Part { anim: PartAnim::Leg(-1.0), pivot: [-0.5, 8.0, 1.5], x_rot: 0.0, y_rot: 0.35, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -4.0, 0.0], [0.0, 7.0, 4.0], [16.0, 14.0])] },
        ],
    }
}

/// Vex (32×32): a tiny angry flying humanoid with membrane wings. Authored at
/// player-ish pixels and scaled down to its ~0.8-block height.
fn vex() -> ModelDef {
    ModelDef {
        tex_w: 32.0,
        tex_h: 32.0,
        scale: PX * 0.55,
        parts: vec![
            Part::plain(PartAnim::Head, [0.0, 20.0, 0.0], vec![Cube::new([0.0, 4.0, 0.0], [6.0, 6.0, 6.0], [0.0, 0.0])]),
            Part::plain(PartAnim::Static, [0.0, 12.0, 0.0], vec![Cube::new([0.0, 4.0, 0.0], [6.0, 10.0, 3.0], [16.0, 20.0])]),
            // Arms.
            limb(1.0, 4.0, 20.0, [2.0, 8.0, 2.0], [24.0, 0.0]),
            limb(-1.0, -4.0, 20.0, [2.0, 8.0, 2.0], [24.0, 0.0]),
            // Joined legs.
            limb(1.0, 1.5, 12.0, [2.0, 10.0, 2.0], [16.0, 0.0]),
            limb(-1.0, -1.5, 12.0, [2.0, 10.0, 2.0], [16.0, 0.0]),
            // Wings.
            Part { anim: PartAnim::Leg(1.0), pivot: [0.0, 18.0, 2.0], x_rot: 0.0, y_rot: -0.4, z_rot: 0.0,
                cubes: vec![Cube::new([2.0, -3.0, 0.0], [8.0, 10.0, 0.0], [16.0, 14.0])] },
            Part { anim: PartAnim::Leg(-1.0), pivot: [0.0, 18.0, 2.0], x_rot: 0.0, y_rot: 0.4, z_rot: 0.0,
                cubes: vec![Cube::new([-2.0, -3.0, 0.0], [8.0, 10.0, 0.0], [16.0, 14.0])] },
        ],
    }
}

/// Endermite (64×32): a tiny four-segment purple bug that tapers to the tail.
fn endermite() -> ModelDef {
    let seg = |z: f32, w: f32, h: f32, uv: [f32; 2]| {
        Part::plain(PartAnim::Static, [0.0, h / 2.0, z], vec![Cube::new([0.0, 0.0, 0.0], [w, h, w], uv)])
    };
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            seg(2.0, 4.0, 3.0, [0.0, 0.0]),
            seg(-1.0, 5.0, 2.0, [0.0, 5.0]),
            seg(-4.0, 3.0, 2.0, [0.0, 9.0]),
            seg(-6.0, 2.0, 1.0, [0.0, 13.0]),
        ],
    }
}

/// Pufferfish (32×32), fully puffed: a spiky cube with fins and spikes poking
/// out of every face.
fn pufferfish() -> ModelDef {
    let spike = |c: [f32; 3], s: [f32; 3]| Part::plain(PartAnim::Static, [0.0, 4.0, 0.0], vec![Cube::new(c, s, [24.0, 0.0])]);
    ModelDef {
        tex_w: 32.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Head, [0.0, 4.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0])]),
            // Spikes on each face.
            spike([0.0, 6.0, 0.0], [2.0, 2.0, 2.0]),
            spike([0.0, -6.0, 0.0], [2.0, 2.0, 2.0]),
            spike([6.0, 0.0, 0.0], [2.0, 2.0, 2.0]),
            spike([-6.0, 0.0, 0.0], [2.0, 2.0, 2.0]),
            spike([0.0, 0.0, 6.0], [2.0, 2.0, 2.0]),
            spike([0.0, 0.0, -6.0], [2.0, 2.0, 2.0]),
            // Side fins.
            Part::plain(PartAnim::Static, [5.0, 4.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [3.0, 1.0, 3.0], [24.0, 3.0])]),
            Part::plain(PartAnim::Static, [-5.0, 4.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [3.0, 1.0, 3.0], [24.0, 3.0])]),
        ],
    }
}

/// Illager biped (64×64): pillager / vindicator / evoker / illusioner share the
/// villager-derived layout — big nosed head, robed body, arms at the sides that
/// swing, and two legs.
fn illager() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Head + brow + nose.
            Part::plain(PartAnim::Head, [0.0, 24.0, 0.0], vec![
                Cube::new([0.0, 5.0, 0.0], [8.0, 10.0, 8.0], [0.0, 0.0]),
                Cube::new([0.0, 4.0, 4.0], [8.0, 2.0, 0.0], [24.0, 0.0]),
                Cube::new([0.0, 2.0, 4.0], [2.0, 4.0, 2.0], [24.0, 0.0]),
            ]),
            // Body + robe overlay.
            Part::plain(PartAnim::Static, [0.0, 12.0, 0.0], vec![Cube::new([0.0, 6.0, 0.0], [8.0, 12.0, 6.0], [16.0, 20.0])]),
            Part::plain(PartAnim::Static, [0.0, 12.0, 0.0], vec![Cube { center: [0.0, 3.0, 0.0], size: [8.0, 18.0, 6.0], uv: [0.0, 38.0], inflate: 0.5 }]),
            // Arms at the sides.
            limb(1.0, 6.0, 22.0, [4.0, 12.0, 4.0], [40.0, 38.0]),
            limb(-1.0, -6.0, 22.0, [4.0, 12.0, 4.0], [40.0, 38.0]),
            // Legs.
            limb(1.0, 2.0, 12.0, [4.0, 12.0, 4.0], [0.0, 22.0]),
            limb(-1.0, -2.0, 12.0, [4.0, 12.0, 4.0], [0.0, 22.0]),
        ],
    }
}

/// Witch (64×128): the villager body layout (top half of the texture) plus a
/// pointed hat and a warty hooked nose.
fn witch() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 128.0,
        scale: PX,
        parts: vec![
            // Head + brow + long warty nose (extra tip cube + wart).
            Part::plain(PartAnim::Head, [0.0, 24.0, 0.0], vec![
                Cube::new([0.0, 5.0, 0.0], [8.0, 10.0, 8.0], [0.0, 0.0]),
                Cube::new([0.0, 4.0, 4.0], [8.0, 2.0, 0.0], [24.0, 0.0]),
                Cube::new([0.0, 2.0, 4.0], [2.0, 4.0, 2.0], [24.0, 0.0]),
                Cube::new([0.0, 1.0, 5.0], [1.0, 2.0, 2.0], [0.0, 0.0]),
                Cube { center: [0.0, 2.0, 6.0], size: [1.0, 1.0, 1.0], uv: [0.0, 0.0], inflate: 0.25 },
            ]),
            // Body + robe.
            Part::plain(PartAnim::Static, [0.0, 12.0, 0.0], vec![Cube::new([0.0, 6.0, 0.0], [8.0, 12.0, 6.0], [16.0, 20.0])]),
            Part::plain(PartAnim::Static, [0.0, 12.0, 0.0], vec![Cube { center: [0.0, 3.0, 0.0], size: [8.0, 18.0, 6.0], uv: [0.0, 38.0], inflate: 0.5 }]),
            // Crossed arms.
            Part { anim: PartAnim::Static, pivot: [0.0, 20.0, 0.0], x_rot: -0.75, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 2.0, 0.0], [8.0, 4.0, 4.0], [40.0, 38.0])] },
            // Legs.
            limb(1.0, 2.0, 12.0, [4.0, 12.0, 4.0], [0.0, 22.0]),
            limb(-1.0, -2.0, 12.0, [4.0, 12.0, 4.0], [0.0, 22.0]),
            // Pointed hat: a wide brim then a leaning stack of shrinking cubes.
            Part::plain(PartAnim::Head, [0.0, 30.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [10.0, 2.0, 10.0], [0.0, 64.0])]),
            Part { anim: PartAnim::Head, pivot: [0.0, 31.0, 0.0], x_rot: -0.05, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 1.5, -0.5], [7.0, 3.0, 7.0], [0.0, 76.0])] },
            Part { anim: PartAnim::Head, pivot: [0.0, 34.0, -1.0], x_rot: -0.12, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 1.5, -0.5], [4.0, 3.0, 4.0], [0.0, 86.0])] },
            Part { anim: PartAnim::Head, pivot: [0.0, 37.0, -2.0], x_rot: -0.22, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 1.5, -0.5], [2.0, 3.0, 2.0], [0.0, 96.0])] },
        ],
    }
}

/// Strider (64×128): a tall bulky body on two thick legs, with a shaggy top.
fn strider() -> ModelDef {
    let leg = |x: f32, sign: f32| limb(sign, x, 16.0, [4.0, 16.0, 4.0], [0.0, 32.0]);
    let hair = |x: f32, z: f32| Part::plain(PartAnim::Static, [x, 32.0, z], vec![Cube::new([0.0, 0.0, 0.0], [1.0, 4.0, 1.0], [8.0, 35.0])]);
    ModelDef {
        tex_w: 64.0,
        tex_h: 128.0,
        scale: PX,
        parts: vec![
            // Body (tall block).
            Part::plain(PartAnim::Static, [0.0, 16.0, 0.0], vec![Cube::new([0.0, 8.0, 0.0], [16.0, 16.0, 10.0], [0.0, 0.0])]),
            // Shaggy hair on top.
            hair(-5.0, -3.0), hair(-2.0, 2.0), hair(1.0, -2.0), hair(4.0, 3.0), hair(6.0, 0.0),
            leg(4.0, 1.0),
            leg(-4.0, -1.0),
        ],
    }
}

/// Hoglin (128×64): a bulky boar — flat body with a mane, a long tusked snout
/// with ears, and four legs.
fn hoglin() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32, uv: [f32; 2]| limb_at(sign, x, z, 14.0, [5.0, 14.0, 5.0], uv);
    ModelDef {
        tex_w: 128.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Body laid flat.
            Part { anim: PartAnim::Static, pivot: [0.0, 14.0, 0.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [14.0, 22.0, 14.0], [1.0, 1.0])] },
            // Head + snout + tusks + ears.
            Part { anim: PartAnim::Head, pivot: [0.0, 16.0, 8.0], x_rot: 0.35, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, 0.0, 3.0], [10.0, 8.0, 14.0], [61.0, 1.0]),
                    Cube::new([-4.0, -4.0, 10.0], [1.0, 4.0, 2.0], [10.0, 13.0]),
                    Cube::new([4.0, -4.0, 10.0], [1.0, 4.0, 2.0], [10.0, 13.0]),
                    Cube::new([-6.0, 5.0, 0.0], [3.0, 4.0, 1.0], [1.0, 45.0]),
                    Cube::new([6.0, 5.0, 0.0], [3.0, 4.0, 1.0], [1.0, 45.0]),
                ] },
            leg(5.0, 7.0, 1.0, [44.0, 22.0]),
            leg(-5.0, 7.0, -1.0, [44.0, 22.0]),
            leg(5.0, -7.0, -1.0, [76.0, 22.0]),
            leg(-5.0, -7.0, 1.0, [76.0, 22.0]),
        ],
    }
}

/// Ravager (128×128): a huge beast — big flat body, a lowered head with horns
/// and a jaw, and four thick legs.
fn ravager() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 21.0, [8.0, 21.0, 8.0], [0.0, 77.0]);
    ModelDef {
        tex_w: 128.0,
        tex_h: 128.0,
        scale: PX,
        parts: vec![
            // Body laid flat.
            Part { anim: PartAnim::Static, pivot: [0.0, 21.0, -3.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [16.0, 28.0, 20.0], [0.0, 0.0])] },
            // Neck + head + horns + jaw.
            Part { anim: PartAnim::Head, pivot: [0.0, 26.0, 11.0], x_rot: 0.4, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, -2.0, 2.0], [8.0, 16.0, 8.0], [68.0, 73.0]),
                    Cube::new([0.0, 2.0, 8.0], [16.0, 20.0, 16.0], [0.0, 0.0]),
                    Cube::new([0.0, -8.0, 12.0], [16.0, 5.0, 12.0], [0.0, 36.0]),
                    Cube::new([-8.0, 12.0, 8.0], [2.0, 5.0, 2.0], [74.0, 55.0]),
                    Cube::new([8.0, 12.0, 8.0], [2.0, 5.0, 2.0], [74.0, 55.0]),
                ] },
            leg(7.0, 8.0, 1.0),
            leg(-7.0, 8.0, -1.0),
            leg(7.0, -8.0, -1.0),
            leg(-7.0, -8.0, 1.0),
        ],
    }
}

/// Warden (128×128): a tall bulky biped — broad torso, a head with three sensory
/// tendrils, long heavy arms and thick legs.
fn warden() -> ModelDef {
    ModelDef {
        tex_w: 128.0,
        tex_h: 128.0,
        scale: PX * 0.9,
        parts: vec![
            // Torso.
            Part::plain(PartAnim::Static, [0.0, 16.0, 0.0], vec![Cube::new([0.0, 10.0, 0.0], [18.0, 21.0, 11.0], [0.0, 0.0])]),
            // Head + three tendrils.
            Part::plain(PartAnim::Head, [0.0, 37.0, 0.0], vec![
                Cube::new([0.0, 6.0, 0.0], [16.0, 16.0, 10.0], [0.0, 32.0]),
                Cube::new([-5.0, 15.0, 0.0], [2.0, 6.0, 2.0], [57.0, 61.0]),
                Cube::new([5.0, 15.0, 0.0], [2.0, 6.0, 2.0], [57.0, 61.0]),
                Cube::new([0.0, 15.0, -3.0], [2.0, 6.0, 2.0], [57.0, 61.0]),
            ]),
            // Long heavy arms.
            limb(1.0, 11.0, 35.0, [8.0, 28.0, 8.0], [44.0, 0.0]),
            limb(-1.0, -11.0, 35.0, [8.0, 28.0, 8.0], [0.0, 0.0]),
            // Thick legs.
            limb(1.0, 5.0, 16.0, [7.0, 16.0, 7.0], [0.0, 90.0]),
            limb(-1.0, -5.0, 16.0, [7.0, 16.0, 7.0], [28.0, 90.0]),
        ],
    }
}

/// Creaking (64×64): a tall, gaunt tree-creature — a small head with branch-like
/// horns, a thin bark torso and long branch limbs.
fn creaking() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Head + branch horns.
            Part::plain(PartAnim::Head, [0.0, 34.0, 0.0], vec![
                Cube::new([0.0, 3.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0]),
                Cube::new([-4.0, 8.0, 0.0], [3.0, 5.0, 1.0], [34.0, 0.0]),
                Cube::new([4.0, 8.0, 0.0], [3.0, 5.0, 1.0], [34.0, 0.0]),
            ]),
            // Thin bark torso.
            Part::plain(PartAnim::Static, [0.0, 20.0, 0.0], vec![Cube::new([0.0, 7.0, 0.0], [8.0, 14.0, 5.0], [24.0, 16.0])]),
            // Long branch arms.
            limb(1.0, 5.0, 32.0, [3.0, 18.0, 3.0], [42.0, 16.0]),
            limb(-1.0, -5.0, 32.0, [3.0, 18.0, 3.0], [42.0, 16.0]),
            // Thin legs.
            limb(1.0, 2.0, 20.0, [3.0, 20.0, 3.0], [0.0, 22.0]),
            limb(-1.0, -2.0, 20.0, [3.0, 20.0, 3.0], [0.0, 22.0]),
        ],
    }
}

/// Breeze (32×32): a floating wind elemental — a head with a dark face on top, a
/// tapering rod body, and a wide ring of swirling wind at the base.
fn breeze() -> ModelDef {
    ModelDef {
        tex_w: 32.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Head with face on top.
            Part::plain(PartAnim::Head, [0.0, 16.0, 0.0], vec![Cube::new([0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [4.0, 0.0])]),
            // Rod body.
            Part::plain(PartAnim::Static, [0.0, 8.0, 0.0], vec![Cube::new([0.0, 4.0, 0.0], [6.0, 8.0, 6.0], [0.0, 16.0])]),
            // Wide low wind ring (swirl approximated as a flat wide band).
            Part::plain(PartAnim::Static, [0.0, 2.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [12.0, 3.0, 12.0], [0.0, 24.0])]),
            // A second, smaller upper wind band.
            Part::plain(PartAnim::Static, [0.0, 6.0, 0.0], vec![Cube { center: [0.0, 0.0, 0.0], size: [9.0, 2.0, 9.0], uv: [0.0, 24.0], inflate: 0.0 }]),
        ],
    }
}

// --- 0.39.0: the last entities (bosses + specials) --------------------------

/// Ender dragon (256×256): a big black body, an angled neck to a horned head, a
/// tapering tail and two wide membrane wings spread out to the sides. The
/// texture is almost entirely near-black, so approximate box UVs still read
/// correctly; the wing membrane is picked from the dark triangular sheet.
fn ender_dragon() -> ModelDef {
    let leg = |x: f32, z: f32, sign: f32| limb_at(sign, x, z, 14.0, [6.0, 16.0, 6.0], [112.0, 104.0]);
    ModelDef {
        tex_w: 256.0,
        tex_h: 256.0,
        scale: PX * 1.3,
        parts: vec![
            // Body (big, laid flat along +Z).
            Part { anim: PartAnim::Static, pivot: [0.0, 22.0, -6.0], x_rot: FRAC_PI_2, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [24.0, 48.0, 22.0], [0.0, 32.0])] },
            // Neck rising up-forward.
            Part { anim: PartAnim::Static, pivot: [0.0, 26.0, 16.0], x_rot: -0.45, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 4.0, 6.0], [10.0, 10.0, 20.0], [0.0, 0.0])] },
            // Head + jaw + horns.
            Part { anim: PartAnim::Head, pivot: [0.0, 33.0, 30.0], x_rot: -0.15, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, 2.0, 6.0], [16.0, 16.0, 16.0], [112.0, 30.0]),
                    Cube::new([-7.0, 8.0, 2.0], [2.0, 4.0, 4.0], [112.0, 0.0]),
                    Cube::new([7.0, 8.0, 2.0], [2.0, 4.0, 4.0], [112.0, 0.0]),
                    Cube::new([0.0, -4.0, 8.0], [14.0, 4.0, 16.0], [176.0, 44.0]),
                ] },
            // Tail, two tapering segments.
            Part::plain(PartAnim::Static, [0.0, 22.0, -18.0], vec![Cube::new([0.0, 0.0, -12.0], [10.0, 10.0, 24.0], [152.0, 88.0])]),
            Part::plain(PartAnim::Static, [0.0, 22.0, -40.0], vec![Cube::new([0.0, 0.0, -12.0], [6.0, 6.0, 24.0], [220.0, 88.0])]),
            // Wings spread out (flat membranes) with a bone bar along the front.
            Part { anim: PartAnim::Static, pivot: [10.0, 26.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: -0.12,
                cubes: vec![
                    Cube::new([28.0, 0.0, 0.0], [56.0, 2.0, 8.0], [112.0, 88.0]),
                    Cube::new([28.0, -1.0, -14.0], [56.0, 0.0, 24.0], [0.0, 152.0]),
                ] },
            Part { anim: PartAnim::Static, pivot: [-10.0, 26.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.12,
                cubes: vec![
                    Cube::new([-28.0, 0.0, 0.0], [56.0, 2.0, 8.0], [112.0, 88.0]),
                    Cube::new([-28.0, -1.0, -14.0], [56.0, 0.0, 24.0], [0.0, 152.0]),
                ] },
            leg(12.0, 14.0, 1.0),
            leg(-12.0, 14.0, -1.0),
            leg(10.0, -14.0, -1.0),
            leg(-10.0, -14.0, 1.0),
        ],
    }
}

/// Wither (64×64): three skull heads (one big centre, two smaller sides) over a
/// short spine that tapers to a tail. Floats — no legs.
fn wither() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Centre head.
            Part::plain(PartAnim::Head, [0.0, 24.0, 0.0], vec![Cube::new([0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0])]),
            // Two side heads.
            Part::plain(PartAnim::Head, [-6.0, 22.0, 0.0], vec![Cube::new([0.0, 3.0, 0.0], [6.0, 6.0, 6.0], [32.0, 0.0])]),
            Part::plain(PartAnim::Head, [6.0, 22.0, 0.0], vec![Cube::new([0.0, 3.0, 0.0], [6.0, 6.0, 6.0], [32.0, 0.0])]),
            // Ribcage top bar (shoulders).
            Part::plain(PartAnim::Static, [0.0, 20.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [20.0, 3.0, 3.0], [0.0, 16.0])]),
            // Vertical spine.
            Part::plain(PartAnim::Static, [0.0, 11.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [4.0, 12.0, 4.0], [0.0, 22.0])]),
            // Tail, tapering.
            Part { anim: PartAnim::Static, pivot: [0.0, 6.0, 0.0], x_rot: 0.3, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, -3.0, 0.0], [3.0, 6.0, 3.0], [0.0, 22.0]),
                    Cube::new([0.0, -8.0, 1.0], [2.0, 5.0, 2.0], [0.0, 22.0]),
                ] },
        ],
    }
}

/// Shulker (64×64): a closed purple shell — a bottom box, a lid box on top, and
/// the yellow inner head peeking out the front.
fn shulker() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Bottom shell.
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![Cube::new([0.0, 5.0, 0.0], [16.0, 8.0, 16.0], [0.0, 28.0])]),
            // Lid on top.
            Part::plain(PartAnim::Static, [0.0, 8.0, 0.0], vec![Cube::new([0.0, 4.0, 0.0], [16.0, 12.0, 16.0], [0.0, 0.0])]),
            // Inner head peeking out the front.
            Part::plain(PartAnim::Head, [0.0, 6.0, 4.0], vec![Cube::new([0.0, 0.0, 0.0], [6.0, 6.0, 6.0], [0.0, 52.0])]),
        ],
    }
}

/// Armor stand (64×64): a thin wooden frame — small head, shoulder bar, plank
/// body, thin arms and legs, on a flat stone base plate.
// Armour stand. Authored as SEVEN parts in a fixed order so the renderer's
// posed path (EntityDrawKind::ArmorStandPosed) can turn each by its own pose:
// 0 head, 1 body, 2 right arm, 3 left arm, 4 right leg, 5 left leg, 6 base.
// Each part pivots at its natural joint (neck / waist / shoulders / hips) so a
// rotation swings the limb from the right place.
fn armor_stand() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // 0: head (turns about the neck at y=23).
            Part::plain(PartAnim::Head, [0.0, 23.0, 0.0], vec![Cube::new([0.0, 3.0, 0.0], [2.0, 7.0, 2.0], [0.0, 0.0])]),
            // 1: body — shoulder bar + two plank slats + hips, as one rigid part
            //    pivoting at the waist (y=12) so a body pose tilts the torso.
            Part::plain(PartAnim::Static, [0.0, 12.0, 0.0], vec![
                Cube::new([0.0, 10.0, 0.0], [12.0, 3.0, 3.0], [0.0, 26.0]),  // shoulder bar
                Cube::new([-2.0, -1.0, 0.0], [2.0, 12.0, 1.0], [16.0, 0.0]), // left slat
                Cube::new([2.0, -1.0, 0.0], [2.0, 12.0, 1.0], [48.0, 16.0]), // right slat
                Cube::new([0.0, -2.0, 0.0], [8.0, 2.0, 2.0], [0.0, 48.0]),   // hips
            ]),
            // 2/3: thin arms, pivoting at the shoulders (y=22).
            limb(1.0, 5.0, 22.0, [2.0, 12.0, 2.0], [24.0, 0.0]),
            limb(-1.0, -5.0, 22.0, [2.0, 12.0, 2.0], [24.0, 0.0]),
            // 4/5: thin legs, pivoting at the hips (y=11), feet at y=0.
            limb(1.0, 2.0, 11.0, [2.0, 11.0, 2.0], [8.0, 0.0]),
            limb(-1.0, -2.0, 11.0, [2.0, 11.0, 2.0], [40.0, 16.0]),
            // 6: stone base plate.
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![Cube::new([0.0, 0.5, 0.0], [12.0, 1.0, 12.0], [0.0, 44.0])]),
        ],
    }
}

/// End crystal (128×64): a bedrock base slab with two nested magenta glass cubes
/// floating above (the inner core spins in vanilla; drawn static here).
fn end_crystal() -> ModelDef {
    ModelDef {
        tex_w: 128.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Bedrock base.
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![Cube::new([0.0, 2.0, 0.0], [12.0, 4.0, 12.0], [0.0, 16.0])]),
            // Inner core, floating.
            Part::plain(PartAnim::Static, [0.0, 15.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0])]),
            // Outer glass frame, inflated around the core.
            Part::plain(PartAnim::Static, [0.0, 15.0, 0.0], vec![Cube { center: [0.0, 0.0, 0.0], size: [8.0, 8.0, 8.0], uv: [32.0, 0.0], inflate: 2.5 }]),
        ],
    }
}

// ---------------------------------------------------------------------------
// Block entities (0.52.0)
// ---------------------------------------------------------------------------

/// Vanilla renders banners at two thirds of model scale, so the 42-pixel pole
/// stands 1.75 blocks tall.
const BANNER_PX: f32 = PX * 2.0 / 3.0;

/// Banner (64×64): the 20×40 cloth hanging from a crossbar, on a 42-pixel pole.
/// `standing` keeps the pole; a wall banner hangs from the bar alone. Part 0 is
/// the cloth, so the app can sway it with the `Leg` animation slot.
fn banner(standing: bool) -> ModelDef {
    let mut parts = vec![
        // Cloth: hangs from the crossbar down to the ground. Authored as a
        // `Leg` part so the app's swing input becomes vanilla's wind sway,
        // pivoting at the top edge.
        Part {
            anim: PartAnim::Leg(1.0),
            pivot: [0.0, 40.0, 0.0],
            x_rot: 0.0,
            y_rot: 0.0,
            z_rot: 0.0,
            // The cloth's patterned face is the box's +Z side, so it hangs in
            // front of the pole and faces the way the banner is turned.
            cubes: vec![Cube::new([0.0, -20.0, 1.5], [20.0, 40.0, 1.0], [0.0, 0.0])],
        },
        // Crossbar the cloth hangs from.
        Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
            Cube::new([0.0, 41.0, 0.0], [20.0, 2.0, 2.0], [0.0, 42.0]),
        ]),
    ];
    if standing {
        parts.push(Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
            Cube::new([0.0, 21.0, 0.0], [2.0, 42.0, 2.0], [44.0, 0.0]),
        ]));
    }
    ModelDef { tex_w: 64.0, tex_h: 64.0, scale: BANNER_PX, parts }
}

/// A head (64×64 — 64×32 mob skull sheets are padded by the app): one 8³ cube
/// filling the lower half of the block. `hat` adds the skin's second head
/// layer, which only player heads have — a mob sheet has body pixels there.
fn skull(hat: bool) -> ModelDef {
    let mut cubes = vec![Cube::new([0.0, 4.0, 0.0], [8.0, 8.0, 8.0], [0.0, 0.0])];
    if hat {
        cubes.push(Cube {
            center: [0.0, 4.0, 0.0],
            size: [8.0, 8.0, 8.0],
            uv: [32.0, 0.0],
            inflate: 0.25,
        });
    }
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], cubes)],
    }
}

/// Piglin head (64×64): a 10-wide skull with a snout, two tusks and the ears
/// splayed out to the sides.
fn skull_piglin() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
                Cube::new([0.0, 4.0, 0.0], [10.0, 8.0, 8.0], [0.0, 0.0]),
                // Snout on the +Z face, with a tusk either side of it.
                Cube::new([0.0, 2.0, 4.5], [4.0, 4.0, 1.0], [31.0, 1.0]),
                Cube::new([2.5, 1.0, 5.5], [1.0, 2.0, 1.0], [2.0, 4.0]),
                Cube::new([-2.5, 1.0, 5.5], [1.0, 2.0, 1.0], [2.0, 0.0]),
            ]),
            // Ears, tilted 30° away from the head.
            Part { anim: PartAnim::Static, pivot: [4.5, 6.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: -0.5236,
                cubes: vec![Cube::new([0.5, -2.5, 0.0], [1.0, 5.0, 4.0], [51.0, 6.0])] },
            Part { anim: PartAnim::Static, pivot: [-4.5, 6.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.5236,
                cubes: vec![Cube::new([-0.5, -2.5, 0.0], [1.0, 5.0, 4.0], [39.0, 6.0])] },
        ],
    }
}

/// Dragon head (256×256): the ender dragon's head, jaw and horns, shrunk to
/// wearable size (vanilla uses 0.75 of the dragon's own scale).
fn skull_dragon() -> ModelDef {
    ModelDef {
        tex_w: 256.0,
        tex_h: 256.0,
        scale: PX * 0.5,
        parts: vec![
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
                Cube::new([0.0, 10.0, 2.0], [16.0, 16.0, 16.0], [112.0, 30.0]),
                Cube::new([-7.0, 16.0, -2.0], [2.0, 4.0, 4.0], [112.0, 0.0]),
                Cube::new([7.0, 16.0, -2.0], [2.0, 4.0, 4.0], [112.0, 0.0]),
                Cube::new([0.0, 4.0, 4.0], [14.0, 4.0, 16.0], [176.0, 44.0]),
            ]),
        ],
    }
}

/// Conduit (32×16): the 6³ shell in the middle of its block. Vanilla swaps the
/// texture between the closed base and the open cage; the app picks which.
fn conduit() -> ModelDef {
    ModelDef {
        tex_w: 32.0,
        tex_h: 16.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
                Cube::new([0.0, 8.0, 0.0], [6.0, 6.0, 6.0], [0.0, 0.0]),
            ]),
        ],
    }
}

/// Bell (32×32): the gold body hanging under its top plate. Part 0 is the body,
/// so the app can swing it on the `Leg` slot when the bell is rung.
fn bell() -> ModelDef {
    ModelDef {
        tex_w: 32.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Body, pivoting at the plate above it.
            Part {
                anim: PartAnim::Leg(1.0),
                pivot: [0.0, 12.0, 0.0],
                x_rot: 0.0,
                y_rot: 0.0,
                z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -3.5, 0.0], [6.0, 7.0, 6.0], [0.0, 0.0])],
            },
            // Top plate, fixed to the support above.
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
                Cube::new([0.0, 13.0, 0.0], [8.0, 2.0, 8.0], [0.0, 13.0]),
            ]),
        ],
    }
}

/// Decorated pot: the 14×16×14 body whose four sides carry the sherds. The app
/// composites them, the top and the bottom into one 64×64 sheet laid out for
/// exactly this box's UV unwrap.
fn decorated_pot() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
                Cube::new([0.0, 8.0, 0.0], [14.0, 16.0, 14.0], [0.0, 0.0]),
            ]),
        ],
    }
}
