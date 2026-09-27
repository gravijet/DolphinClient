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

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_6, PI};

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
    /// Moves on its own, whether or not the entity is going anywhere: beating
    /// wings, swaying tentacles, spinning blaze rods. Driven by a free-running
    /// clock rather than by movement, which is what makes an idle mob look
    /// alive instead of frozen.
    Idle(IdleMotion),
    /// A chest lid: swings up about X by the open angle (in radians), which
    /// arrives on the same channel the walk swing does.
    Lid,
    /// A shulker box lid: rises half a block and turns 270° over the course of
    /// opening, driven by the same channel carrying 0 (shut) to 1 (open).
    ShulkerLid,
    /// Squash and stretch: the swing channel carries how much the body is
    /// stretched (positive) or flattened (negative), and the part is scaled
    /// about its pivot so a slime keeps its footing on the ground while its
    /// top rises and falls. Vanilla does the same with its `squish`.
    Squash,
    /// One half of a biting jaw: rotates about Z to a fixed 180° baseline
    /// offset by `sign * 0.35π * swing`, where the swing channel carries the
    /// bite-shut amount (1 = just spawned and wide open, 0 = clamped shut).
    /// Vanilla overwrites the whole angle every frame rather than adding to a
    /// rest pose, so this replaces rather than offsets the part's rotation.
    Jaw(f32),
}

/// How a self-animating part moves.
#[derive(Clone, Copy)]
pub enum IdleMotion {
    /// A wing beating about Z. `sign` mirrors the two sides, `rest` is the
    /// angle it hangs at and `amp` how far it beats from there.
    Wing { rest: f32, amp: f32, hz: f32, sign: f32 },
    /// A tentacle or tail swinging about X. `phase` staggers a ring of them so
    /// they don't move as one slab.
    Sway { amp: f32, hz: f32, phase: f32 },
    /// A wing mounted on the back, beating about Y instead of Z — the way an
    /// allay's or a vex's wings clap behind them.
    Flutter { rest: f32, amp: f32, hz: f32, sign: f32 },
    /// Turning steadily about Y — the ring of rods around a blaze.
    Spin { hz: f32 },
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

/// What a part *is*, for the mobs that strike poses. Vanilla poses a sitting
/// dog by moving each of its parts by hand; to do the same we have to know
/// which part is a hind leg and which is the tail.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PartRole {
    /// Not part of any pose — moves only the way its `PartAnim` says. (Named
    /// `Plain` rather than `None` so the poses below can still say `None` and
    /// mean "no override".)
    #[default]
    Plain,
    Head,
    /// The torso (already laid flat by its baked `x_rot`).
    Body,
    /// The wolf's shoulders/mane, which vanilla poses separately from the body.
    Mane,
    FrontLeg,
    BackLeg,
    Tail,
    PaddleLeft,
    PaddleRight,
    /// A raider's arm, thrown up in celebration.
    LeftArm,
    RightArm,
    /// A creaking's own leg — unlike every quadruped's `FrontLeg`/`BackLeg`
    /// pair, it only has one pair, so (like `LeftArm`/`RightArm`) each side
    /// gets its own direct label rather than a `mirror`-disambiguated shared
    /// role.
    LeftLeg,
    RightLeg,
}

/// Which part is which, for the models that can be posed. The order matches the
/// order the model function builds its parts in; `part_roles_match_models`
/// keeps the two from drifting apart.
pub fn part_roles(model: MobModel) -> &'static [PartRole] {
    use PartRole::*;
    match model {
        MobModel::Wolf => &[Head, Body, Mane, FrontLeg, FrontLeg, BackLeg, BackLeg, Tail],
        MobModel::Cat => &[Head, Body, FrontLeg, FrontLeg, BackLeg, BackLeg, Tail],
        MobModel::Fox => &[Head, Body, FrontLeg, FrontLeg, BackLeg, BackLeg, Tail],
        MobModel::Panda => &[Head, Body, FrontLeg, FrontLeg, BackLeg, BackLeg],
        MobModel::PolarBear => &[Head, Body, FrontLeg, FrontLeg, BackLeg, BackLeg],
        MobModel::Horse => &[Body, Head, FrontLeg, FrontLeg, BackLeg, BackLeg, Tail],
        MobModel::Boat => &[Plain, Plain, Plain, Plain, Plain, PaddleLeft, PaddleRight],
        // head, body, robe overlay, right arm, left arm, right leg, left leg —
        // see `fn illager()`. Legs keep walking normally; only the arms pose.
        MobModel::Illager => &[Head, Body, Plain, RightArm, LeftArm, Plain, Plain],
        // head, body, right_arm, left_arm, right_wing, left_wing — see
        // `fn allay()`. Only the head takes a pose override (the dance sway/
        // spin is a whole-body `pose_root` rotation, not per-part); the
        // wings keep their usual idle flutter even while dancing.
        MobModel::Allay => &[Head, Plain, Plain, Plain, Plain, Plain],
        // head, body, 4 legs, tail — see `fn axolotl()`. Only `PlayingDead`
        // poses this model, and only its legs + body (the tail's real delta
        // is 0 anyway — playDead never touches it).
        MobModel::Axolotl => &[Head, Body, FrontLeg, FrontLeg, BackLeg, BackLeg, Tail],
        // body, hump, head, 4 legs, tail — see `fn camel()`. The hump has no
        // real counterpart bone (vanilla's hump cubes just live inside the
        // real "body" bone's cube list); tagging it `Body` too so it rotates
        // along with the body rather than staying put is this engine's own
        // pragmatic fix for its flat (non-nested) `Part` list — a real
        // rigidly-nested bone would move exactly with its parent, this
        // approximates that by applying the same rotation about the hump's
        // own (nearby) pivot instead of body's.
        MobModel::Camel => &[Body, Body, Head, FrontLeg, FrontLeg, BackLeg, BackLeg, Tail],
        // body, head, 4 legs — see `fn sniffer()`. Real vanilla's rig also
        // has ears/nose/lower-beak and a separate mid pair of legs, none of
        // which this engine's simplified 6-part model has geometry for; see
        // the `SnifferHappy`/`SnifferSniffing`/`SnifferDigging`/
        // `SnifferRising` pose arms below for exactly what's kept vs.
        // dropped.
        MobModel::Sniffer => &[Body, Head, FrontLeg, FrontLeg, BackLeg, BackLeg],
        // head, torso, right_arm, left_arm, right_leg, left_leg — see
        // `fn creaking`. Real vanilla nests head/right_arm/left_arm under an
        // "upper_body" bone this engine has no equivalent for; the torso
        // part stands in for "upper_body" directly (see the `mod
        // creaking_*` doc comment for how each clip's "upper_body" track
        // gets summed into head/right_arm/left_arm's own).
        MobModel::Creaking => &[Head, Body, LeftArm, RightArm, LeftLeg, RightLeg],
        _ => &[],
    }
}

/// How a mob is holding itself, over and above walking: a dog told to sit, a
/// cat curled up by a bed, a horse up on its hind legs, a boat being rowed.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum MobPose {
    /// Standing normally — every part animates the way it always does.
    #[default]
    None,
    /// Sitting up on its haunches (a tamed dog or cat, a fox, a panda).
    Sitting,
    /// Curled up on the ground (a cat lying down, a sleeping fox).
    Lying,
    /// Up on the hind legs: a rearing horse, a polar bear standing.
    Rearing,
    /// Slinking low, the way a fox stalks.
    Crouching,
    /// Being rowed: which oars are in the water right now.
    Rowing { left: bool, right: bool },
    /// A raider throwing its arms up and cheering — a raid just won.
    Celebrating,
    /// An allay swaying to music, and — mid-cycle — spinning in place.
    Dancing { is_spinning: bool, spin_progress: f32 },
    /// A panda mid-tumble: legs kicking, head lolling back further than
    /// [`MobPose::OnBack`].
    Rolling { amount: f32 },
    /// A panda flopped on its back, legs kicking gently.
    OnBack { amount: f32 },
    /// A fox scrambling to its feet after a failed pounce.
    Faceplanted,
    /// An axolotl playing dead: real `BinaryAnimator(10, IN_OUT_SINE)`-eased
    /// factor, 0..1 (`AdultAxolotlModel.setupPlayDeadAnimation`).
    PlayingDead { factor: f32 },
    /// A camel mid-dash: seconds elapsed into the looping 0.5s `CAMEL_DASH`
    /// keyframe clip (`CamelAnimation.CAMEL_DASH`, applied via real
    /// `KeyframeAnimation`/`AnimationState` machinery — see [`camel_dash`]).
    Dashing { elapsed_secs: f32 },
    /// A sniffer nuzzling happily: seconds into the looping 2s
    /// `SNIFFER_HAPPY` clip (see [`sniffer_happy`]).
    SnifferHappy { elapsed_secs: f32 },
    /// A sniffer taking one long sniff: seconds into the non-looping 1s
    /// `SNIFFER_LONGSNIFF` clip (see [`sniffer_longsniff`]).
    SnifferSniffing { elapsed_secs: f32 },
    /// A sniffer digging: seconds into the non-looping 8s `SNIFFER_DIG`
    /// clip (see [`sniffer_dig`]).
    SnifferDigging { elapsed_secs: f32 },
    /// A sniffer rising back up: seconds into the non-looping 3s
    /// `SNIFFER_STAND_UP` clip (see [`sniffer_stand_up`]).
    SnifferRising { elapsed_secs: f32 },
    /// A creaking walking: seconds into the looping 1.125s `CREAKING_WALK`
    /// clip (see [`creaking_walk`]), reusing this engine's existing
    /// leg-swing phase as a stand-in for vanilla's own walk-distance
    /// accumulator (same substitution 0.118.0's cape wobble already made).
    CreakingWalking { elapsed_secs: f32 },
    /// A creaking mid-swing: seconds since its current attack started (the
    /// looping 0.7083s `CREAKING_ATTACK` clip, see [`creaking_attack`]).
    CreakingAttacking { elapsed_secs: f32 },
    /// A creaking flashing after being hit: seconds since the non-looping
    /// 0.2917s `CREAKING_INVULNERABLE` clip started (see
    /// [`creaking_invulnerable`]).
    CreakingFlashing { elapsed_secs: f32 },
    /// A creaking tearing down (dying): seconds into the non-looping 2.25s
    /// `CREAKING_DEATH` clip (see [`creaking_death`]).
    CreakingTearingDown { elapsed_secs: f32 },
    /// A pillager (no crossbow out) or vindicator swinging at something:
    /// `attack_time` is real `AnimationUtils.swingWeaponDown`'s raw 0..1
    /// `attackTime` (this engine's existing one-shot swing envelope's raw
    /// progress, not its already-transformed `attack_swing` shape). Main arm
    /// is assumed right — this engine doesn't yet read the rare
    /// `LeftHanded` flag, consistent with that flag being otherwise fully
    /// unread.
    Attacking { attack_time: f32 },
    /// An evoker or illusioner casting a spell: both arms thrown out to the
    /// sides, wobbling on the same per-entity clock `Celebrating`'s wobble
    /// already rides (`IllagerModel.setupAnim`'s `SPELLCASTING` branch).
    Spellcasting,
    /// An illusioner drawing its bow — a pure function of its own current
    /// head yaw/pitch, no clock needed (`BOW_AND_ARROW`, see
    /// [`illager_head_pose_part`]).
    BowAndArrow,
    /// A pillager holding its crossbow ready, not yet drawing — also
    /// head-relative, no clock (`CROSSBOW_HOLD`).
    CrossbowHold,
    /// A pillager drawing its crossbow back: `frac` is real
    /// `ticksUsingItem / getChargeDuration`, clamped 0..1
    /// (`CROSSBOW_CHARGE`, see [`illager_head_pose_part`]).
    CrossbowCharge { frac: f32 },
}

/// What a pose does to one part: shift where it hangs from (blocks) and turn it
/// about X, Y and Z instead of its usual animation.
pub struct PosePart {
    pub shift: [f32; 3],
    pub x_rot: f32,
    pub y_rot: f32,
    pub z_rot: f32,
}

/// The pose's whole-body transform: how far it tips back about the feet
/// (x, radians), how far it turns/sways about the feet (y/z, radians — an
/// allay's spin and dance sway; vanilla's `root.yRot`/`root.zRot`, applied at
/// the same "whole model" level this engine has no explicit `root` part for),
/// and how far it is lifted or lowered (blocks). `anim` is the same
/// continuously-running per-entity phase `pose_part`/`pose_swing` already
/// take — real vanilla's `ageInTicks * K°/tick` becomes `anim * (K * 20 *
/// PI/180)` here, since `anim` runs in seconds where vanilla's own constants
/// are per-tick (confirmed against this file's existing `Celebrating` wobble,
/// which already does this same tick→second rescale).
pub fn pose_root(pose: MobPose, anim: f32) -> (f32, f32, f32, f32) {
    match pose {
        // A fox stalking sinks toward the ground; everything else is posed part
        // by part, because tipping the whole animal would drive half of it
        // through the floor.
        MobPose::Crouching => (0.0, 0.0, 0.0, -0.12),
        // Allay: real `AllayModel.setupAnim`'s dancing branch. `danceSpeed`
        // omits vanilla's small `+ walkAnimationSpeed` phase offset (a minor,
        // deliberately-skipped refinement — the dominant sway/spin motion
        // below is an exact port of the rest of the formula).
        MobPose::Dancing { is_spinning, spin_progress } => {
            let dance_speed = anim * 2.7925268; // ageInTicks * 8° in rad/tick * 20
            let sway = dance_speed.cos() * 16f32.to_radians() * (1.0 - spin_progress);
            let spin = if is_spinning {
                std::f32::consts::PI * 4.0 * spin_progress
            } else {
                0.0
            };
            (0.0, spin, sway, 0.0)
        }
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}

/// A pose that swings most of the animal as one piece about a point: a rearing
/// horse turns about its hind hooves, and its hind legs stay planted.
pub struct PoseSwing {
    /// The point it turns about, relative to the feet (blocks).
    pub about: [f32; 3],
    pub x_rot: f32,
}

/// The whole-body swing this pose applies, if any.
pub fn pose_swing(pose: MobPose, hip: f32) -> Option<PoseSwing> {
    match pose {
        MobPose::Rearing => {
            Some(PoseSwing { about: [0.0, hip, -0.9 * hip], x_rot: -50f32.to_radians() })
        }
        _ => None,
    }
}

/// Parts a swing leaves where they are — the legs it is standing on.
pub fn pose_swing_skips(role: PartRole) -> bool {
    role == PartRole::BackLeg
}

// --- Real vanilla's `KeyframeAnimation` (`net.minecraft.client.animation`) --
//
// Camel's `CAMEL_DASH` is this engine's first pose driven by an actual
// keyframe *clip* (several named bone tracks, each independently interpolated
// over time) rather than a single scalar factor plugged into a closed-form
// formula — every pose above this point only ever needed the latter. This is
// a byte-exact port of vanilla's own sampling algorithm
// (`KeyframeAnimation.Entry.apply`/`AnimationChannel.Interpolations`,
// decompiled fresh from the 26.1 jar this release), not an approximation.

/// One track's interpolation mode (`AnimationChannel.Interpolations`).
#[derive(Clone, Copy)]
enum Interp {
    Linear,
    CatmullRom,
}

/// One keyframe (`net.minecraft.client.animation.Keyframe`): vanilla's own
/// pre/post-target split collapses to one value here, since every keyframe
/// this engine actually plays back uses the simple 3-arg constructor (pre ==
/// post) — real vanilla only splits them for a hard cut, which none of
/// Camel's tracks use.
#[derive(Clone, Copy)]
struct Kf {
    t: f32,
    v: [f32; 3],
    interp: Interp,
}
const fn kf(t: f32, v: [f32; 3], interp: Interp) -> Kf {
    Kf { t, v, interp }
}

/// Real `Mth.catmullrom`.
fn catmullrom(a: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    0.5 * (2.0 * p1
        + (p2 - p0) * a
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * a * a
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * a * a * a)
}

/// Real `KeyframeAnimation.Entry.apply`: find the keyframe pair either side of
/// `t` (real `Mth.binarySearch`, restated as a linear scan — these tracks are
/// only ever a handful of keyframes long), then interpolate between them.
/// `CatmullRom` reaches one keyframe past each side of the pair (clamped at
/// the track's own ends) for a smooth spline; `Linear` just lerps the pair.
fn sample_track(kfs: &[Kf], t: f32) -> [f32; 3] {
    let n = kfs.len();
    if n == 0 {
        return [0.0; 3];
    }
    if n == 1 {
        return kfs[0].v;
    }
    let idx = kfs.iter().position(|k| t <= k.t).unwrap_or(n);
    let prev = idx.saturating_sub(1);
    let next = (prev + 1).min(n - 1);
    let alpha = if next != prev {
        ((t - kfs[prev].t) / (kfs[next].t - kfs[prev].t)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    match kfs[next].interp {
        Interp::Linear => {
            let a = kfs[prev].v;
            let b = kfs[next].v;
            [
                a[0] + (b[0] - a[0]) * alpha,
                a[1] + (b[1] - a[1]) * alpha,
                a[2] + (b[2] - a[2]) * alpha,
            ]
        }
        Interp::CatmullRom => {
            let p0 = kfs[prev.saturating_sub(1)].v;
            let p1 = kfs[prev].v;
            let p2 = kfs[next].v;
            let p3 = kfs[(next + 1).min(n - 1)].v;
            [
                catmullrom(alpha, p0[0], p1[0], p2[0], p3[0]),
                catmullrom(alpha, p0[1], p1[1], p2[1], p3[1]),
                catmullrom(alpha, p0[2], p1[2], p2[2], p3[2]),
            ]
        }
    }
}

/// `CamelAnimation.CAMEL_DASH`, decompiled fresh from the real 26.1 jar:
/// 0.5s, looping, degrees converted to radians (matching real
/// `KeyframeAnimations.degreeVec`). Real vanilla also carries `left_ear`/
/// `right_ear` tracks, but both of their keyframes hold one identical value
/// throughout the whole clip (they never actually animate) — and this
/// engine's camel model has no separate ear geometry to move anyway, so
/// they're correctly omitted rather than approximated.
mod camel_dash {
    use super::{Interp::*, Kf, kf};
    const D: f32 = std::f32::consts::PI / 180.0;
    pub const BODY: &[Kf] = &[kf(0.0, [5.0 * D, 0.0, 0.0], Linear), kf(0.5, [5.0 * D, 0.0, 0.0], Linear)];
    pub const TAIL: &[Kf] = &[
        kf(0.0, [67.5 * D, 0.0, 0.0], CatmullRom),
        kf(0.125, [112.5 * D, 0.0, 0.0], CatmullRom),
        kf(0.25, [67.5 * D, 0.0, 0.0], CatmullRom),
        kf(0.375, [112.5 * D, 0.0, 0.0], CatmullRom),
        kf(0.5, [67.5 * D, 0.0, 0.0], CatmullRom),
    ];
    pub const HEAD: &[Kf] = &[
        kf(0.0, [10.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.125, [0.0, 0.0, 0.0], CatmullRom),
        kf(0.25, [10.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.375, [0.0, 0.0, 0.0], CatmullRom),
        kf(0.5, [10.0 * D, 0.0, 0.0], CatmullRom),
    ];
    pub const RIGHT_FRONT_LEG: &[Kf] = &[
        kf(0.0, [44.97272 * D, 1.76749 * D, -1.76833 * D], CatmullRom),
        kf(0.125, [-90.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.25, [44.97272 * D, 1.76749 * D, -1.76833 * D], CatmullRom),
        kf(0.375, [-90.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.5, [44.97272 * D, 1.76749 * D, -1.76833 * D], CatmullRom),
    ];
    pub const LEFT_FRONT_LEG: &[Kf] = &[
        kf(0.0, [-90.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.125, [44.97272 * D, -1.76749 * D, 1.76833 * D], CatmullRom),
        kf(0.25, [-90.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.375, [44.97272 * D, -1.76749 * D, 1.76833 * D], CatmullRom),
        kf(0.5, [-90.0 * D, 0.0, 0.0], CatmullRom),
    ];
    pub const LEFT_HIND_LEG: &[Kf] = &[
        kf(0.0, [90.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.125, [-45.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.25, [90.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.375, [-45.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.5, [90.0 * D, 0.0, 0.0], CatmullRom),
    ];
    pub const RIGHT_HIND_LEG: &[Kf] = &[
        kf(0.0, [-45.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.125, [90.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.25, [-45.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.375, [90.0 * D, 0.0, 0.0], CatmullRom),
        kf(0.5, [-45.0 * D, 0.0, 0.0], CatmullRom),
    ];
}

/// Samples one `CAMEL_DASH` bone track at `elapsed_secs` into the clip (real
/// `KeyframeAnimation.getElapsedSeconds`: looped `% 0.5` since the clip
/// loops). Front/hind leg tracks aren't simple mirrors of each other — real
/// vanilla's left/right legs are a quarter-cycle out of phase as well as
/// y/z-negated (a diagonal gait), so each side needs its own full track
/// rather than one shared formula multiplied by `mirror`.
fn camel_dash_track(kfs: &'static [Kf], elapsed_secs: f32) -> [f32; 3] {
    sample_track(kfs, elapsed_secs.rem_euclid(0.5))
}

// --- Sniffer's real `SnifferAnimation` clips (0.123.0 fast-follow, reusing
// the keyframe player built for Camel above) ---
//
// This engine's `fn sniffer()` model is a simplified 6-part rig (body, head,
// front legs, hind legs) — real vanilla's is 13 parts (adds separate ears,
// nose, lower beak, and a THIRD "mid" pair of legs). Every clip below is
// transcribed byte-exact from the real decompiled `SnifferAnimation`, but
// only the tracks this engine actually has geometry for are kept:
// ear/nose/lower-beak tracks are dropped (no cubes to move, same "missing
// geometry" precedent as Camel's ears), the real "mid" leg track is dropped
// (front/hind keep their own real timing, unlike Camel's legs the three
// leg-groups' rotation keyframes are only offset in TIME, not shape — using
// front's and hind's own real tracks rather than inventing a blended
// middle), and any real SCALE-target track is dropped (this engine's
// `PosePart` has no scale channel at all — a systemic engine limitation,
// distinct from a per-mob missing-geometry omission). This is also why
// `SNIFFER_SNIFFSNIFF` (real vanilla's "Scenting" state) isn't ported at
// all: its ONLY track scales the nose, so there is nothing left to show
// once that's dropped.
//
// Real vanilla's `SnifferModel.setupAnim` also feeds a live head-look
// rotation into `head.xRot`/`head.yRot` BEFORE adding each clip's own head
// delta on top (additive). This engine's pose mechanism *replaces* a part's
// rotation rather than adding to it (see the `PlayingDead`/`Dashing` doc
// comments above), so overriding `Head` here — like `Rolling`/`OnBack`/
// `Dancing` already do — trades away generic head-tracking for the
// duration of the clip. Consistent with that established precedent, not a
// new gap.
mod sniffer_longsniff {
    use super::{Interp::*, Kf, kf};
    const D: f32 = std::f32::consts::PI / 180.0;
    pub const HEAD: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.125, [-5.0 * D, 0.0, 0.0], Linear),
        kf(0.875, [-20.0 * D, 0.0, 0.0], Linear),
        kf(1.0, [0.0, 0.0, 0.0], Linear),
    ];
}

mod sniffer_happy {
    use super::{Interp::*, Kf, kf};
    const D: f32 = std::f32::consts::PI / 180.0;
    pub const HEAD: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.5, [-32.00206 * D, 19.3546 * D, -11.70092 * D], CatmullRom),
        kf(1.0, [0.0, 0.0, 0.0], CatmullRom),
        kf(1.5, [-32.00206 * D, -19.3546 * D, 11.70092 * D], CatmullRom),
        kf(2.0, [0.0, 0.0, 0.0], CatmullRom),
    ];
}

mod sniffer_stand_up {
    use super::{Interp::*, Kf, kf};
    const D: f32 = std::f32::consts::PI / 180.0;
    const P: f32 = 1.0 / 16.0;
    pub const BODY_ROT: &[Kf] = &[
        kf(0.25, [0.0, 0.0, 0.0], Linear),
        kf(0.75, [2.5 * D, 0.0, 0.0], Linear),
        kf(1.5, [-2.5 * D, 0.0, 0.0], Linear),
        kf(1.7083, [0.0, 0.0, 0.0], Linear),
    ];
    pub const BODY_POS: &[Kf] = &[
        kf(0.25, [0.0, -7.0 * P, 0.0], Linear),
        kf(0.75, [0.0, -7.0 * P, 0.0], Linear),
        kf(1.5, [0.0, 0.0, 0.0], Linear),
        kf(1.7083, [0.0, 0.0, 0.0], Linear),
    ];
    pub const HEAD_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.3333, [-5.0 * D, 0.0, 0.0], Linear),
        kf(0.7083, [0.0, 0.0, 0.0], Linear),
        kf(1.0, [10.0 * D, 0.0, 0.0], Linear),
        kf(1.375, [0.0, 0.0, 0.0], Linear),
    ];
    pub const HEAD_POS: &[Kf] = &[
        kf(0.0, [0.0, 1.0 * P, 0.0], Linear),
        kf(1.375, [0.0, 1.0 * P, 0.0], Linear),
    ];
    pub const LEFT_FRONT_LEG_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, -90.0 * D], CatmullRom),
        kf(0.4583, [0.0, 0.0, 0.0], CatmullRom),
    ];
    pub const LEFT_FRONT_LEG_POS: &[Kf] = &[
        kf(0.0, [4.0 * P, -5.5 * P, 0.0], CatmullRom),
        kf(0.2083, [-6.0 * P, -5.5 * P, 0.0], CatmullRom),
        kf(0.4583, [0.0, 0.0, 0.0], CatmullRom),
    ];
    pub const RIGHT_FRONT_LEG_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 90.0 * D], CatmullRom),
        kf(0.4583, [0.0, 0.0, 0.0], CatmullRom),
    ];
    pub const RIGHT_FRONT_LEG_POS: &[Kf] = &[
        kf(0.0, [-4.0 * P, -5.5 * P, 0.0], CatmullRom),
        kf(0.2083, [6.0 * P, -5.5 * P, 0.0], CatmullRom),
        kf(0.4583, [0.0, 0.0, 0.0], CatmullRom),
    ];
    pub const LEFT_HIND_LEG_ROT: &[Kf] = &[
        kf(0.1667, [0.0, 0.0, -90.0 * D], CatmullRom),
        kf(0.6667, [0.0, 0.0, 0.0], CatmullRom),
    ];
    pub const LEFT_HIND_LEG_POS: &[Kf] = &[
        kf(0.1667, [4.0 * P, -5.5 * P, 0.0], CatmullRom),
        kf(0.4167, [-6.0 * P, -5.5 * P, 0.0], CatmullRom),
        kf(0.6667, [0.0, 0.0, 0.0], CatmullRom),
    ];
    pub const RIGHT_HIND_LEG_ROT: &[Kf] = &[
        kf(0.1667, [0.0, 0.0, 90.0 * D], CatmullRom),
        kf(0.6667, [0.0, 0.0, 0.0], CatmullRom),
    ];
    pub const RIGHT_HIND_LEG_POS: &[Kf] = &[
        kf(0.1667, [-4.0 * P, -5.5 * P, 0.0], CatmullRom),
        kf(0.4167, [6.0 * P, -5.5 * P, 0.0], CatmullRom),
        kf(0.6667, [0.0, 0.0, 0.0], CatmullRom),
    ];
}

mod sniffer_dig {
    use super::{Interp::*, Kf, kf};
    const D: f32 = std::f32::consts::PI / 180.0;
    const P: f32 = 1.0 / 16.0;
    pub const BODY_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.5, [1.5 * D, 0.0, 0.0], Linear),
        kf(1.3333, [-5.0 * D, 0.0, 0.0], Linear),
        kf(1.5, [0.0, 0.0, 0.0], Linear),
        kf(2.0, [0.0, 0.0, 0.0], Linear),
        kf(2.5, [2.5 * D, 0.0, 0.0], Linear),
        kf(3.0, [0.0, 0.0, 0.0], Linear),
        kf(3.5, [2.5 * D, 0.0, 0.0], Linear),
        kf(4.0, [0.0, 0.0, 0.0], Linear),
        kf(4.5, [2.5 * D, 0.0, 0.0], Linear),
        kf(5.6667, [5.0 * D, 0.0, 0.0], Linear),
        kf(5.8333, [-2.5 * D, 0.0, 0.0], Linear),
        kf(6.0, [0.0, 0.0, 0.0], Linear),
    ];
    pub const BODY_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(1.3333, [0.0, 1.0 * P, 0.0], Linear),
        kf(1.5, [0.0, -7.0 * P, 0.0], Linear),
    ];
    pub const HEAD_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], CatmullRom),
        kf(1.1667, [10.0 * D, 0.0, 0.0], CatmullRom),
        kf(1.4167, [-10.0 * D, 0.0, 0.0], CatmullRom),
        kf(1.5, [0.0, 0.0, 0.0], CatmullRom),
        kf(1.5833, [0.0, 0.0, 0.0], CatmullRom),
        kf(1.875, [0.0, 0.0, 0.0], CatmullRom),
        kf(2.0833, [0.0, 0.0, 0.0], CatmullRom),
        kf(2.5, [47.5 * D, 0.0, 0.0], CatmullRom),
        kf(2.6667, [38.44 * D, 0.0, 0.0], CatmullRom),
        kf(2.875, [10.95951 * D, 13.57454 * D, -14.93501 * D], CatmullRom),
        kf(3.2083, [47.5 * D, 0.0, 0.0], CatmullRom),
        kf(3.5833, [55.0 * D, 0.0, 0.0], CatmullRom),
        kf(3.7917, [4.2932 * D, -16.187 * D, 10.90042 * D], CatmullRom),
        kf(4.125, [47.5 * D, 0.0, 0.0], CatmullRom),
        kf(4.4167, [54.71135 * D, 7.98009 * D, -5.56662 * D], CatmullRom),
        kf(4.5, [55.72895 * D, -6.77684 * D, 4.46197 * D], CatmullRom),
        kf(4.5833, [54.71135 * D, 7.98009 * D, -5.56662 * D], CatmullRom),
        kf(4.6667, [55.72895 * D, -6.77684 * D, 4.46197 * D], CatmullRom),
        kf(4.75, [54.71135 * D, 7.98009 * D, -5.56662 * D], CatmullRom),
        kf(4.8333, [55.72895 * D, -6.77684 * D, 4.46197 * D], CatmullRom),
        kf(5.0, [65.0 * D, 0.0, 0.0], CatmullRom),
        kf(5.75, [65.0 * D, 0.0, 0.0], CatmullRom),
        kf(5.9167, [-32.5 * D, 0.0, 0.0], CatmullRom),
        kf(6.25, [0.0, 0.0, 0.0], Linear),
    ];
    pub const HEAD_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.625, [0.0, 0.0, 0.0], Linear),
        kf(1.375, [0.0, 1.0 * P, 0.0], Linear),
        kf(1.5, [0.0, 1.0 * P, 0.0], Linear),
        kf(1.5833, [0.0, 1.0 * P, 0.0], Linear),
        kf(1.875, [0.0, 1.0 * P, 0.0], Linear),
        kf(2.0833, [0.0, 3.0 * P, 0.0], Linear),
        kf(2.2917, [0.0, 6.0 * P, 0.0], Linear),
        kf(2.6667, [0.0, 0.0, 0.0], Linear),
        kf(3.2083, [0.0, 4.0 * P, 0.0], Linear),
        kf(3.5833, [0.0, 0.0, 0.0], Linear),
        kf(4.125, [0.0, 4.0 * P, 0.0], Linear),
        kf(5.0, [0.0, 0.0, 0.0], Linear),
        kf(5.75, [0.0, 1.0 * P, 0.0], Linear),
        kf(6.0, [0.0, 1.5 * P, 0.0], Linear),
        kf(6.25, [0.0, 1.0 * P, 0.0], Linear),
    ];
    pub const LEFT_FRONT_LEG_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(1.2083, [0.0, 0.0, 0.0], Linear),
        kf(1.375, [0.0, 0.0, -90.0 * D], Linear),
    ];
    pub const LEFT_FRONT_LEG_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(1.2083, [0.0, 0.0, 0.0], Linear),
        kf(1.2917, [2.0 * P, -0.75 * P, 0.0], Linear),
        kf(1.375, [4.0 * P, -5.5 * P, 0.0], Linear),
    ];
    pub const RIGHT_FRONT_LEG_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(1.2083, [0.0, 0.0, 0.0], Linear),
        kf(1.375, [0.0, 0.0, 90.0 * D], Linear),
    ];
    pub const RIGHT_FRONT_LEG_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(1.2083, [0.0, 0.0, 0.0], Linear),
        kf(1.2917, [-2.0 * P, -0.75 * P, 0.0], Linear),
        kf(1.375, [-4.0 * P, -5.5 * P, 0.0], Linear),
    ];
    pub const LEFT_HIND_LEG_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(1.3333, [0.0, 0.0, 0.0], Linear),
        kf(1.5, [0.0, 0.0, -90.0 * D], Linear),
    ];
    pub const LEFT_HIND_LEG_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(1.3333, [0.0, 0.0, 0.0], Linear),
        kf(1.4167, [2.0 * P, -0.75 * P, 0.0], Linear),
        kf(1.5, [4.0 * P, -5.5 * P, 0.0], Linear),
    ];
    pub const RIGHT_HIND_LEG_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(1.3333, [0.0, 0.0, 0.0], Linear),
        kf(1.5, [0.0, 0.0, 90.0 * D], Linear),
    ];
    pub const RIGHT_HIND_LEG_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(1.3333, [0.0, 0.0, 0.0], Linear),
        kf(1.4167, [-2.0 * P, -0.75 * P, 0.0], Linear),
        kf(1.5, [-4.0 * P, -5.5 * P, 0.0], Linear),
    ];
}

// --- Creaking's real `CreakingAnimation` clips (0.124.0, reusing the
// keyframe player built for Camel/Sniffer above) ---
//
// Real vanilla's `CreakingModel` nests head/body/right_arm/left_arm under one
// "upper_body" parent bone, so a track on "upper_body" rotates all four
// together (composed at render time by the mesh hierarchy) before each
// child's own local track adds further on top. This engine's `Part` list has
// no such nesting (see the Y-axis-pitfall precedent for static geometry —
// the same flattening problem, here for a per-frame animated rotation
// instead of a one-time bake): the accepted fix, per that same precedent
// (0.97.0's nested-`PartPose` flattening, which already sums ancestor
// rotations directly rather than composing rotation matrices), is to SUM
// "upper_body"'s track with each affected child's own track, per axis. This
// engine's simplified 6-part model (`fn creaking`) has no separate "body"
// cube distinguishing it from "upper_body" — none of these four clips
// carries its own "body" channel either, so the torso ("Body" role here)
// just takes "upper_body"'s track directly. Real "left_leg"/"right_leg" are
// children of "root", NOT "upper_body" — they never need the summing step.
// All of Creaking's keyframes use plain LINEAR interpolation (no CatmullRom
// anywhere in the real clip data).
mod creaking_walk {
    use super::{Interp::*, Kf, kf};
    const D: f32 = std::f32::consts::PI / 180.0;
    const P: f32 = 1.0 / 16.0;
    pub const UPPER_BODY_ROT: &[Kf] = &[
        kf(0.0, [26.8802 * D, -23.399 * D, -9.0616 * D], Linear),
        kf(0.125, [-2.2093 * D, 5.9119 * D, 0.0675 * D], Linear),
        kf(0.5417, [23.0778 * D, 14.2906 * D, 4.6066 * D], Linear),
        kf(0.7083, [-10.0 * D, 0.0, 0.0], Linear),
        kf(0.875, [7.5 * D, 0.0, 0.0], Linear),
        kf(1.125, [26.8802 * D, -23.399 * D, -9.0616 * D], Linear),
    ];
    pub const HEAD_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.0417, [-17.5 * D, -62.5 * D, 0.0], Linear),
        kf(0.0833, [0.0, 0.0, 0.0], Linear),
        kf(0.4167, [0.0, 0.0, 0.0], Linear),
        kf(0.4583, [0.0, 15.0 * D, 0.0], Linear),
        kf(0.5, [0.0, 0.0, 0.0], Linear),
        kf(1.0417, [0.0, 0.0, 0.0], Linear),
        kf(1.0833, [-37.1532 * D, 81.1131 * D, -28.3621 * D], Linear),
        kf(1.125, [0.0, 0.0, 0.0], Linear),
    ];
    pub const RIGHT_ARM_ROT: &[Kf] = &[
        kf(0.0, [12.5 * D, 0.0, 0.0], Linear),
        kf(0.25, [-32.0 * D, 0.0, 0.0], Linear),
        kf(0.875, [12.0 * D, 0.0, 0.0], Linear),
        kf(1.125, [-15.0 * D, 0.0, 0.0], Linear),
    ];
    pub const LEFT_ARM_ROT: &[Kf] = &[
        kf(0.0, [-15.0 * D, 0.0, 0.0], Linear),
        kf(0.125, [10.0 * D, 0.0, 0.0], Linear),
        kf(0.5417, [-25.0 * D, 0.0, 0.0], Linear),
        kf(0.75, [-9.0923 * D, 0.0, 0.0], Linear),
        kf(0.7917, [-15.137 * D, -66.7758 * D, 13.9603 * D], Linear),
        kf(0.8333, [-9.0923 * D, 0.0, 0.0], Linear),
        kf(1.0, [10.0 * D, 0.0, 0.0], Linear),
        kf(1.125, [-15.0 * D, 0.0, 0.0], Linear),
    ];
    pub const LEFT_LEG_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.25, [30.0 * D, 0.0, 0.0], Linear),
        kf(0.375, [49.8924 * D, -3.8282 * D, 3.2187 * D], Linear),
        kf(0.5, [17.5 * D, 0.0, 0.0], Linear),
        kf(0.625, [-56.5613 * D, -12.2403 * D, -8.7374 * D], Linear),
        kf(0.9167, [0.0, 0.0, 0.0], Linear),
        kf(1.125, [0.0, 0.0, 0.0], Linear),
    ];
    pub const LEFT_LEG_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 2.0 * P], Linear),
        kf(0.25, [0.0, 0.1846 * P, 0.5979 * P], Linear),
        kf(0.375, [0.0, -0.0665 * P, -2.2177 * P], Linear),
        kf(0.5, [0.0, 1.3563 * P, -4.3474 * P], Linear),
        kf(0.625, [0.0, 0.1047 * P, -1.6556 * P], Linear),
        kf(0.9167, [0.0, 0.0, -1.0 * P], Linear),
        kf(1.125, [0.0, 0.0, 2.0 * P], Linear),
    ];
    pub const RIGHT_LEG_ROT: &[Kf] = &[
        kf(0.0, [25.5305 * D, 11.3125 * D, 5.3525 * D], Linear),
        kf(0.125, [-49.5628 * D, 7.3556 * D, 6.7933 * D], Linear),
        kf(0.25, [0.0, 0.0, 0.0], Linear),
        kf(0.4583, [0.0, 0.0, 0.0], Linear),
        kf(0.9167, [30.0 * D, 0.0, 0.0], Linear),
        kf(1.0417, [55.0 * D, 0.0, 0.0], Linear),
        kf(1.125, [25.5305 * D, 11.3125 * D, 5.3525 * D], Linear),
    ];
    pub const RIGHT_LEG_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.9674 * P, -3.6578 * P], Linear),
        kf(0.125, [0.0, -0.2979 * P, -0.9411 * P], Linear),
        kf(0.25, [0.0, -0.3 * P, -0.94 * P], Linear),
        kf(0.4583, [0.0, -0.3 * P, 1.06 * P], Linear),
        kf(1.125, [0.0, 0.9674 * P, -3.6578 * P], Linear),
    ];
}

mod creaking_attack {
    use super::{Interp::*, Kf, kf};
    const D: f32 = std::f32::consts::PI / 180.0;
    const P: f32 = 1.0 / 16.0;
    pub const UPPER_BODY_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.0833, [0.0, 45.0 * D, 0.0], Linear),
        kf(0.1667, [-115.0 * D, 67.5 * D, -90.0 * D], Linear),
        kf(0.375, [67.5 * D, 0.0, 0.0], Linear),
        kf(0.5417, [0.0, 45.0 * D, 0.0], Linear),
        kf(0.7083, [0.0, 0.0, 0.0], Linear),
    ];
    pub const UPPER_BODY_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.0833, [0.0, 0.0, 0.0], Linear),
        kf(0.2917, [0.0, -2.7716 * P, -1.1481 * P], Linear),
        kf(0.375, [0.0, 0.0, 0.0], Linear),
        kf(0.5417, [0.0, 0.0, 0.0], Linear),
        kf(0.7083, [0.0, 0.0, 0.0], Linear),
    ];
    pub const HEAD_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.1667, [0.0, -45.0 * D, 0.0], Linear),
        kf(0.25, [-11.25 * D, -45.0 * D, 0.0], Linear),
        kf(0.2917, [-117.3939 * D, 76.6331 * D, -130.1483 * D], Linear),
        kf(0.4167, [-45.0 * D, -45.0 * D, 0.0], Linear),
        kf(0.5, [60.0 * D, -45.0 * D, 0.0], Linear),
        kf(0.5833, [60.0 * D, -45.0 * D, 0.0], Linear),
        kf(0.625, [0.0, -45.0 * D, 0.0], Linear),
        kf(0.7083, [0.0, 0.0, 0.0], Linear),
    ];
    pub const HEAD_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.1667, [0.0, 0.0, 0.0], Linear),
        kf(0.4167, [0.0, 0.0, 0.0], Linear),
        kf(0.5, [0.3827 * P, 0.5133 * P, -0.7682 * P], Linear),
        kf(0.5833, [0.3827 * P, 0.5133 * P, -0.7682 * P], Linear),
        kf(0.625, [0.0, 0.0, 0.0], Linear),
        kf(0.7083, [0.0, 0.0, 0.0], Linear),
    ];
    pub const RIGHT_ARM_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.1667, [0.0, 0.0, 0.0], Linear),
        kf(0.25, [7.5 * D, 0.0, 0.0], Linear),
        kf(0.4583, [55.0 * D, 0.0, 0.0], Linear),
        kf(0.625, [0.0, 0.0, 0.0], Linear),
        kf(0.7083, [0.0, 0.0, 0.0], Linear),
    ];
    pub const LEFT_ARM_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.1667, [0.0, 0.0, 0.0], Linear),
        kf(0.25, [10.3453 * D, 14.7669 * D, 2.664 * D], Linear),
        kf(0.4583, [57.5 * D, 0.0, 0.0], Linear),
        kf(0.625, [0.0, 0.0, 0.0], Linear),
        kf(0.7083, [0.0, 0.0, 0.0], Linear),
    ];
    pub const LEFT_LEG_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.1667, [0.0, 0.0, -2.0 * P], Linear),
        kf(0.625, [0.0, 0.0, -2.0 * P], Linear),
        kf(0.7083, [0.0, 0.0, 0.0], Linear),
    ];
    pub const RIGHT_LEG_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.1667, [0.0, 45.0 * D, 0.0], Linear),
        kf(0.625, [0.0, 45.0 * D, 0.0], Linear),
        kf(0.7083, [0.0, 0.0, 0.0], Linear),
    ];
    pub const RIGHT_LEG_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.1667, [0.7071 * P, 0.0, 0.0], Linear),
        kf(0.625, [0.7071 * P, 0.0, 0.0], Linear),
        kf(0.7083, [0.0, 0.0, 0.0], Linear),
    ];
}

mod creaking_invulnerable {
    use super::{Interp::*, Kf, kf};
    const D: f32 = std::f32::consts::PI / 180.0;
    pub const UPPER_BODY_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.0833, [-5.0 * D, 0.0, 0.0], Linear),
        kf(0.1667, [5.0 * D, 0.0, 0.0], Linear),
        kf(0.25, [0.0, 0.0, 0.0], Linear),
    ];
    pub const RIGHT_ARM_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.0833, [17.5 * D, 0.0, 0.0], Linear),
        kf(0.1667, [-15.0 * D, 0.0, 0.0], Linear),
        kf(0.25, [0.0, 0.0, 0.0], Linear),
    ];
    pub const LEFT_ARM_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.0833, [20.0 * D, 0.0, 0.0], Linear),
        kf(0.1667, [-15.0 * D, 0.0, 0.0], Linear),
        kf(0.25, [0.0, 0.0, 0.0], Linear),
    ];
}

mod creaking_death {
    use super::{Interp::*, Kf, kf};
    const D: f32 = std::f32::consts::PI / 180.0;
    const P: f32 = 1.0 / 16.0;
    pub const UPPER_BODY_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.0833, [-40.0 * D, 0.0, 0.0], Linear),
        kf(0.1667, [-5.0 * D, 0.0, 0.0], Linear),
        kf(0.2917, [7.5 * D, 0.0, 0.0], Linear),
        kf(0.5833, [16.25 * D, 0.0, 0.0], Linear),
        kf(0.6667, [29.0814 * D, 62.5516 * D, 26.5771 * D], Linear),
        kf(0.75, [12.2115 * D, 0.0, 0.0], Linear),
        kf(1.0, [10.25 * D, 0.0, 0.0], Linear),
        kf(1.0417, [-47.64 * D, 0.0, 0.0], Linear),
        kf(1.125, [21.96 * D, 0.0, 0.0], Linear),
        kf(1.25, [12.5 * D, 0.0, 0.0], Linear),
        kf(2.25, [17.3266 * D, 7.9022 * D, -0.1381 * D], Linear),
    ];
    pub const UPPER_BODY_POS: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.0833, [0.0, 0.557 * P, 1.2659 * P], Linear),
        kf(0.1667, [0.0, -2.0889 * P, -0.3493 * P], Linear),
        kf(0.2917, [0.0, 0.0, 0.0], Linear),
    ];
    pub const RIGHT_ARM_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.2917, [-10.0 * D, 0.0, 0.0], Linear),
        kf(0.5, [0.0, 0.0, 0.0], Linear),
        kf(1.25, [-10.0 * D, 0.0, 0.0], Linear),
        kf(1.5417, [-10.0 * D, 0.0, 0.0], Linear),
        kf(1.5833, [-12.1479 * D, -34.3927 * D, 6.9326 * D], Linear),
        kf(1.6667, [-10.0 * D, 0.0, 0.0], Linear),
    ];
    pub const LEFT_ARM_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.2917, [-10.0 * D, 0.0, 0.0], Linear),
        kf(0.5, [0.0, 0.0, 0.0], Linear),
        kf(0.8333, [-4.4444 * D, 0.0, 0.0], Linear),
        kf(0.875, [-26.7402 * D, -78.831 * D, 26.3025 * D], Linear),
        kf(0.9583, [-5.5556 * D, 0.0, 0.0], Linear),
        kf(1.25, [-10.0 * D, 0.0, 0.0], Linear),
    ];
    pub const HEAD_ROT: &[Kf] = &[
        kf(0.0, [0.0, 0.0, 0.0], Linear),
        kf(0.0833, [-5.0 * D, 0.0, 0.0], Linear),
        kf(0.2917, [10.0 * D, 0.0, 0.0], Linear),
        kf(0.5, [2.5 * D, 0.0, 0.0], Linear),
        kf(0.5417, [5.5 * D, 0.0, 0.0], Linear),
        kf(0.5833, [-67.4168 * D, -12.9552 * D, -8.0231 * D], Linear),
        kf(0.6667, [8.5 * D, 0.0, 0.0], Linear),
        kf(1.0, [10.773 * D, -29.5608 * D, -5.3627 * D], Linear),
        kf(1.25, [10.0 * D, 0.0, 0.0], Linear),
        kf(1.7917, [10.0 * D, 0.0, 0.0], Linear),
        kf(1.8333, [12.9625 * D, 39.2735 * D, 8.2901 * D], Linear),
        kf(1.9167, [10.0 * D, 0.0, 0.0], Linear),
    ];
}

/// Samples a Creaking track at `elapsed_secs`, looping `% 1.125` for the walk
/// cycle (real `CREAKING_WALK.looping()`) or clamping-at-end for the other
/// three (real vanilla's non-looping `AnimationState`s hold their last
/// keyframe, which `sample_track` already does naturally past the end).
fn creaking_walk_track(kfs: &'static [Kf], elapsed_secs: f32) -> [f32; 3] {
    sample_track(kfs, elapsed_secs.rem_euclid(1.125))
}
fn creaking_track(kfs: &'static [Kf], elapsed_secs: f32) -> [f32; 3] {
    sample_track(kfs, elapsed_secs)
}

/// Sums two rotation deltas per axis — real vanilla's "upper_body" bone
/// parents head/right_arm/left_arm in the mesh hierarchy, so its own track's
/// rotation composes with each child's local track at render time. This
/// engine's flat `Part` list has no parent-child nesting, so the two are
/// summed directly instead (the same technique 0.97.0 already validated for
/// flattening a nested *static* `PartPose` — extended here to a per-frame
/// *animated* one, reasonable since Creaking's rotations are all modest in
/// magnitude rather than needing exact matrix composition).
fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Samples one non-looping Sniffer track at `elapsed_secs` — unlike Camel's
/// dash, these clips play once and hold their final keyframe's value
/// (`sample_track` already does this naturally: past the last keyframe,
/// `next == prev` and it returns that keyframe's value verbatim), matching
/// real vanilla's own non-looping `AnimationState` behaviour.
fn sniffer_track(kfs: &'static [Kf], elapsed_secs: f32) -> [f32; 3] {
    sample_track(kfs, elapsed_secs)
}

/// Samples one keyframe of the looping 2s `SNIFFER_HAPPY` clip.
fn sniffer_happy_track(kfs: &'static [Kf], elapsed_secs: f32) -> [f32; 3] {
    sample_track(kfs, elapsed_secs.rem_euclid(2.0))
}

/// How a pose moves one part. `None` leaves the part to its usual animation;
/// `hip` is the height the model's legs hang from, so the same pose fits a cat
/// and a panda. `mirror` is the part's own pivot-X sign (+1/-1, or 0 for a
/// centred part) — the only way this function can tell a model's two
/// `FrontLeg`/`BackLeg` instances apart, needed for a pose whose left and
/// right sides move oppositely (Rolling/OnBack/Faceplanted all do; every
/// pose before them was left-right symmetric and ignored it).
///
/// The angles are vanilla's own (a sitting dog's body sits 45° off its walking
/// angle, its hind legs fold a full 90° forward, its forelegs brace 27°); the
/// distances are vanilla's too, expressed in hip heights rather than in the
/// wolf's pixels so every sitter uses them.
pub fn pose_part(pose: MobPose, role: PartRole, hip: f32, anim: f32, mirror: f32) -> Option<PosePart> {
    use PartRole::*;
    let p = |shift: [f32; 3], x_rot: f32| PosePart { shift, x_rot, y_rot: 0.0, z_rot: 0.0 };
    Some(match (pose, role) {
        // Allay: real `AllayModel.setupAnim`'s dancing branch head tilt.
        // `spin_progress` fades the tilt out over the spin (matching real
        // vanilla, which also zeroes it while spinning).
        (MobPose::Dancing { spin_progress, .. }, Head) => {
            let dance_speed = anim * 2.7925268;
            let fade = 1.0 - spin_progress;
            PosePart {
                shift: [0.0; 3],
                x_rot: 0.0,
                y_rot: dance_speed.cos() * 30f32.to_radians() * fade,
                z_rot: dance_speed.cos() * 14f32.to_radians() * fade,
            }
        }
        // Panda: real `PandaModel.setupAnim`'s `rollAmount > 0` branch. The
        // legs swing at full amplitude the instant `amount` leaves 0 — real
        // vanilla's own leg formula ignores `rollAmount` entirely once past
        // that threshold, only the head lerps smoothly by it.
        (MobPose::Rolling { amount }, Head) if amount > 0.0 => {
            p([0.0, 0.0, 0.0], amount * 2.0561945)
        }
        (MobPose::Rolling { amount }, FrontLeg) if amount > 0.0 => {
            p([0.0, 0.0, 0.0], mirror * (anim * 10.0).sin() * 0.5)
        }
        (MobPose::Rolling { amount }, BackLeg) if amount > 0.0 => {
            p([0.0, 0.0, 0.0], -mirror * (anim * 10.0).sin() * 0.5)
        }
        // Panda: real `PandaModel.setupAnim`'s `lieOnBackAmount > 0` branch.
        (MobPose::OnBack { amount }, Head) if amount > 0.0 => {
            p([0.0, 0.0, 0.0], amount * FRAC_PI_2)
        }
        (MobPose::OnBack { amount }, BackLeg) if amount > 0.0 => {
            p([0.0, 0.0, 0.0], -mirror * (anim * 3.0).sin() * 0.6)
        }
        (MobPose::OnBack { amount }, FrontLeg) if amount > 0.0 => {
            p([0.0, 0.0, 0.0], mirror * (anim * 5.0).sin() * 0.3)
        }
        // Fox: real `FoxModel.setupAnim`'s `isFaceplanted` branch — `anim`
        // carries `EntityTrack::leg_motion_pos` here, not the generic
        // per-entity clock (see that field's doc comment for why).
        (MobPose::Faceplanted, FrontLeg) => p([0.0, 0.0, 0.0], mirror * (anim * 0.4662).cos() * 0.1),
        (MobPose::Faceplanted, BackLeg) => p([0.0, 0.0, 0.0], -mirror * (anim * 0.4662).cos() * 0.1),
        // Axolotl: real `AdultAxolotlModel.setupPlayDeadAnimation` — the
        // dominant splayed-legs shape is an exact port (`mirror` here matches
        // this engine's own pivot-X convention against real vanilla's
        // `left_hind_leg`/`left_front_leg`, which likewise sit at positive
        // local X). Deliberately NOT ported: vanilla's separate
        // `mirroredLegsFactor` blend (a function of on-ground/moving state
        // this client doesn't track at all for axolotls, since it never
        // implemented the swimming/hovering/ground-crawling animation stack
        // those factors belong to) — the right-side legs are mirrored
        // straight off `factor` instead of that blended value, which is
        // exact whenever nothing else is animating the legs at the same
        // time (the common case: a playing-dead axolotl is inert).
        (MobPose::PlayingDead { factor }, Body) if factor > 0.0 => PosePart {
            shift: [0.0; 3],
            x_rot: FRAC_PI_2 - 0.15 * factor, // FRAC_PI_2: this model's own lay-flat bake.
            y_rot: 0.0,
            z_rot: 0.35 * factor,
        },
        (MobPose::PlayingDead { factor }, FrontLeg) if factor > 0.0 => PosePart {
            shift: [0.0; 3],
            x_rot: 0.7853982 * factor,
            y_rot: mirror * 2.042035 * factor,
            z_rot: 0.0,
        },
        (MobPose::PlayingDead { factor }, BackLeg) if factor > 0.0 => PosePart {
            shift: [0.0; 3],
            x_rot: 1.4137167 * factor,
            y_rot: mirror * 1.0995574 * factor,
            z_rot: mirror * 0.7853982 * factor,
        },
        // Camel: real `CamelAnimation.CAMEL_DASH`, applied via the keyframe
        // sampler above. Each part's own baked rest rotation (this engine's
        // equivalent of vanilla's constructor-time `ModelPart` rotation,
        // which a posed part otherwise replaces rather than adds to — see
        // this function's doc comment) is folded into the returned angle so
        // the part keeps its correct rest shape plus the real animated
        // delta on top, exactly as vanilla's own additive `offsetRotation`
        // would produce.
        (MobPose::Dashing { elapsed_secs }, Body) => {
            let r = camel_dash_track(camel_dash::BODY, elapsed_secs);
            PosePart { shift: [0.0; 3], x_rot: FRAC_PI_2 + r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::Dashing { elapsed_secs }, Head) => {
            let r = camel_dash_track(camel_dash::HEAD, elapsed_secs);
            PosePart { shift: [0.0; 3], x_rot: -FRAC_PI_4 * 1.1 + r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::Dashing { elapsed_secs }, Tail) => {
            let r = camel_dash_track(camel_dash::TAIL, elapsed_secs);
            PosePart { shift: [0.0; 3], x_rot: -0.2 + r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::Dashing { elapsed_secs }, FrontLeg) => {
            let track = if mirror >= 0.0 { camel_dash::LEFT_FRONT_LEG } else { camel_dash::RIGHT_FRONT_LEG };
            let r = camel_dash_track(track, elapsed_secs);
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::Dashing { elapsed_secs }, BackLeg) => {
            let track = if mirror >= 0.0 { camel_dash::LEFT_HIND_LEG } else { camel_dash::RIGHT_HIND_LEG };
            let r = camel_dash_track(track, elapsed_secs);
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        // Sniffer: real `SnifferAnimation.SNIFFER_HAPPY`/`SNIFFER_LONGSNIFF`
        // — only the head has geometry to move (see the `mod sniffer_*`
        // doc comment above for what's dropped and why).
        (MobPose::SnifferHappy { elapsed_secs }, Head) => {
            let r = sniffer_happy_track(sniffer_happy::HEAD, elapsed_secs);
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::SnifferSniffing { elapsed_secs }, Head) => {
            let r = sniffer_track(sniffer_longsniff::HEAD, elapsed_secs);
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        // Sniffer digging/rising: body keeps its own baked lay-flat `x_rot`
        // (this engine's equivalent of vanilla's constructor-time rotation,
        // which a posed part otherwise replaces — same fold-in Camel's
        // `Body` arm above uses) plus the real animated delta on top.
        (MobPose::SnifferDigging { elapsed_secs }, Body) => {
            let rot = sniffer_track(sniffer_dig::BODY_ROT, elapsed_secs);
            let shift = sniffer_track(sniffer_dig::BODY_POS, elapsed_secs);
            PosePart { shift, x_rot: FRAC_PI_2 + rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        (MobPose::SnifferDigging { elapsed_secs }, Head) => {
            let rot = sniffer_track(sniffer_dig::HEAD_ROT, elapsed_secs);
            let shift = sniffer_track(sniffer_dig::HEAD_POS, elapsed_secs);
            PosePart { shift, x_rot: rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        (MobPose::SnifferDigging { elapsed_secs }, FrontLeg) => {
            let (rot_t, pos_t) = if mirror >= 0.0 {
                (sniffer_dig::LEFT_FRONT_LEG_ROT, sniffer_dig::LEFT_FRONT_LEG_POS)
            } else {
                (sniffer_dig::RIGHT_FRONT_LEG_ROT, sniffer_dig::RIGHT_FRONT_LEG_POS)
            };
            let rot = sniffer_track(rot_t, elapsed_secs);
            let shift = sniffer_track(pos_t, elapsed_secs);
            PosePart { shift, x_rot: rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        (MobPose::SnifferDigging { elapsed_secs }, BackLeg) => {
            let (rot_t, pos_t) = if mirror >= 0.0 {
                (sniffer_dig::LEFT_HIND_LEG_ROT, sniffer_dig::LEFT_HIND_LEG_POS)
            } else {
                (sniffer_dig::RIGHT_HIND_LEG_ROT, sniffer_dig::RIGHT_HIND_LEG_POS)
            };
            let rot = sniffer_track(rot_t, elapsed_secs);
            let shift = sniffer_track(pos_t, elapsed_secs);
            PosePart { shift, x_rot: rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        (MobPose::SnifferRising { elapsed_secs }, Body) => {
            let rot = sniffer_track(sniffer_stand_up::BODY_ROT, elapsed_secs);
            let shift = sniffer_track(sniffer_stand_up::BODY_POS, elapsed_secs);
            PosePart { shift, x_rot: FRAC_PI_2 + rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        (MobPose::SnifferRising { elapsed_secs }, Head) => {
            let rot = sniffer_track(sniffer_stand_up::HEAD_ROT, elapsed_secs);
            let shift = sniffer_track(sniffer_stand_up::HEAD_POS, elapsed_secs);
            PosePart { shift, x_rot: rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        (MobPose::SnifferRising { elapsed_secs }, FrontLeg) => {
            let (rot_t, pos_t) = if mirror >= 0.0 {
                (sniffer_stand_up::LEFT_FRONT_LEG_ROT, sniffer_stand_up::LEFT_FRONT_LEG_POS)
            } else {
                (sniffer_stand_up::RIGHT_FRONT_LEG_ROT, sniffer_stand_up::RIGHT_FRONT_LEG_POS)
            };
            let rot = sniffer_track(rot_t, elapsed_secs);
            let shift = sniffer_track(pos_t, elapsed_secs);
            PosePart { shift, x_rot: rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        (MobPose::SnifferRising { elapsed_secs }, BackLeg) => {
            let (rot_t, pos_t) = if mirror >= 0.0 {
                (sniffer_stand_up::LEFT_HIND_LEG_ROT, sniffer_stand_up::LEFT_HIND_LEG_POS)
            } else {
                (sniffer_stand_up::RIGHT_HIND_LEG_ROT, sniffer_stand_up::RIGHT_HIND_LEG_POS)
            };
            let rot = sniffer_track(rot_t, elapsed_secs);
            let shift = sniffer_track(pos_t, elapsed_secs);
            PosePart { shift, x_rot: rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        // Creaking: real `CreakingAnimation`'s four clips, applied via the
        // same keyframe sampler above. `Body` stands in for vanilla's
        // "upper_body" bone directly (no clip has its own "body" channel);
        // `Head`/`LeftArm`/`RightArm` sum "upper_body"'s track with their
        // own local one (`add3`, see that fn's doc comment); `LeftLeg`/
        // `RightLeg` only ever carry their own track, since real legs are
        // never children of "upper_body" and no clip but Walk/Attack
        // touches them at all.
        (MobPose::CreakingWalking { elapsed_secs }, Body) => {
            let r = creaking_walk_track(creaking_walk::UPPER_BODY_ROT, elapsed_secs);
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingWalking { elapsed_secs }, Head) => {
            let ub = creaking_walk_track(creaking_walk::UPPER_BODY_ROT, elapsed_secs);
            let r = add3(ub, creaking_walk_track(creaking_walk::HEAD_ROT, elapsed_secs));
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingWalking { elapsed_secs }, LeftArm) => {
            let ub = creaking_walk_track(creaking_walk::UPPER_BODY_ROT, elapsed_secs);
            let r = add3(ub, creaking_walk_track(creaking_walk::LEFT_ARM_ROT, elapsed_secs));
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingWalking { elapsed_secs }, RightArm) => {
            let ub = creaking_walk_track(creaking_walk::UPPER_BODY_ROT, elapsed_secs);
            let r = add3(ub, creaking_walk_track(creaking_walk::RIGHT_ARM_ROT, elapsed_secs));
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingWalking { elapsed_secs }, LeftLeg) => {
            let rot = creaking_walk_track(creaking_walk::LEFT_LEG_ROT, elapsed_secs);
            let shift = creaking_walk_track(creaking_walk::LEFT_LEG_POS, elapsed_secs);
            PosePart { shift, x_rot: rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        (MobPose::CreakingWalking { elapsed_secs }, RightLeg) => {
            let rot = creaking_walk_track(creaking_walk::RIGHT_LEG_ROT, elapsed_secs);
            let shift = creaking_walk_track(creaking_walk::RIGHT_LEG_POS, elapsed_secs);
            PosePart { shift, x_rot: rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        (MobPose::CreakingAttacking { elapsed_secs }, Body) => {
            let r = creaking_track(creaking_attack::UPPER_BODY_ROT, elapsed_secs);
            let shift = creaking_track(creaking_attack::UPPER_BODY_POS, elapsed_secs);
            PosePart { shift, x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingAttacking { elapsed_secs }, Head) => {
            let ub = creaking_track(creaking_attack::UPPER_BODY_ROT, elapsed_secs);
            let ub_pos = creaking_track(creaking_attack::UPPER_BODY_POS, elapsed_secs);
            let r = add3(ub, creaking_track(creaking_attack::HEAD_ROT, elapsed_secs));
            let shift = add3(ub_pos, creaking_track(creaking_attack::HEAD_POS, elapsed_secs));
            PosePart { shift, x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingAttacking { elapsed_secs }, LeftArm) => {
            let ub = creaking_track(creaking_attack::UPPER_BODY_ROT, elapsed_secs);
            let r = add3(ub, creaking_track(creaking_attack::LEFT_ARM_ROT, elapsed_secs));
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingAttacking { elapsed_secs }, RightArm) => {
            let ub = creaking_track(creaking_attack::UPPER_BODY_ROT, elapsed_secs);
            let r = add3(ub, creaking_track(creaking_attack::RIGHT_ARM_ROT, elapsed_secs));
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingAttacking { elapsed_secs }, LeftLeg) => {
            let shift = creaking_track(creaking_attack::LEFT_LEG_POS, elapsed_secs);
            PosePart { shift, x_rot: 0.0, y_rot: 0.0, z_rot: 0.0 }
        }
        (MobPose::CreakingAttacking { elapsed_secs }, RightLeg) => {
            let rot = creaking_track(creaking_attack::RIGHT_LEG_ROT, elapsed_secs);
            let shift = creaking_track(creaking_attack::RIGHT_LEG_POS, elapsed_secs);
            PosePart { shift, x_rot: rot[0], y_rot: rot[1], z_rot: rot[2] }
        }
        (MobPose::CreakingFlashing { elapsed_secs }, Body) => {
            let r = creaking_track(creaking_invulnerable::UPPER_BODY_ROT, elapsed_secs);
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingFlashing { elapsed_secs }, Head) => {
            let r = creaking_track(creaking_invulnerable::UPPER_BODY_ROT, elapsed_secs);
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingFlashing { elapsed_secs }, LeftArm) => {
            let ub = creaking_track(creaking_invulnerable::UPPER_BODY_ROT, elapsed_secs);
            let r = add3(ub, creaking_track(creaking_invulnerable::LEFT_ARM_ROT, elapsed_secs));
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingFlashing { elapsed_secs }, RightArm) => {
            let ub = creaking_track(creaking_invulnerable::UPPER_BODY_ROT, elapsed_secs);
            let r = add3(ub, creaking_track(creaking_invulnerable::RIGHT_ARM_ROT, elapsed_secs));
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingTearingDown { elapsed_secs }, Body) => {
            let r = creaking_track(creaking_death::UPPER_BODY_ROT, elapsed_secs);
            let shift = creaking_track(creaking_death::UPPER_BODY_POS, elapsed_secs);
            PosePart { shift, x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingTearingDown { elapsed_secs }, Head) => {
            let ub = creaking_track(creaking_death::UPPER_BODY_ROT, elapsed_secs);
            let r = add3(ub, creaking_track(creaking_death::HEAD_ROT, elapsed_secs));
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingTearingDown { elapsed_secs }, LeftArm) => {
            let ub = creaking_track(creaking_death::UPPER_BODY_ROT, elapsed_secs);
            let r = add3(ub, creaking_track(creaking_death::LEFT_ARM_ROT, elapsed_secs));
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::CreakingTearingDown { elapsed_secs }, RightArm) => {
            let ub = creaking_track(creaking_death::UPPER_BODY_ROT, elapsed_secs);
            let r = add3(ub, creaking_track(creaking_death::RIGHT_ARM_ROT, elapsed_secs));
            PosePart { shift: [0.0; 3], x_rot: r[0], y_rot: r[1], z_rot: r[2] }
        }
        (MobPose::Sitting, Body) => p([0.0, -0.50 * hip, 0.25 * hip], -FRAC_PI_4),
        (MobPose::Sitting, Mane) => p([0.0, -0.25 * hip, 0.0], -18f32.to_radians()),
        (MobPose::Sitting, BackLeg) => p([0.0, -0.75 * hip, 0.35 * hip], -FRAC_PI_2),
        (MobPose::Sitting, FrontLeg) => p([0.0, -0.13 * hip, 0.0], -27f32.to_radians()),
        (MobPose::Sitting, Tail) => p([0.0, -1.10 * hip, 0.25 * hip], 0.0),
        (MobPose::Lying, Head | Body | Mane | Tail) => p([0.0, -0.62 * hip, 0.0], 0.0),
        (MobPose::Lying, FrontLeg | BackLeg) => p([0.0, -0.80 * hip, 0.30 * hip], -FRAC_PI_2),
        // Rearing swings the animal as one piece (see `pose_swing`); on top of
        // that the forelegs tuck up, the way a rearing horse folds them.
        (MobPose::Rearing, FrontLeg) => p([0.0, 0.0, 0.0], -40f32.to_radians()),
        (MobPose::Rowing { left, right }, PaddleLeft | PaddleRight) => {
            let is_left = role == PaddleLeft;
            let rowing = if is_left { left } else { right };
            // Vanilla runs a per-side rowing clock and reads two lerps off it;
            // an idle oar sits at the clock's zero, angled out of the water.
            let t = if rowing { anim * 5.0 } else { 0.0 };
            let x_rot = lerp(-1.0, -0.2617994, ((-t).sin() + 1.0) * 0.5);
            let y_rot = lerp(-FRAC_PI_4, FRAC_PI_4, ((-t + 1.0).sin() + 1.0) * 0.5);
            PosePart {
                shift: [0.0; 3],
                x_rot,
                y_rot: if is_left { y_rot } else { -y_rot },
                z_rot: 0.0,
            }
        }
        // Both arms thrown up and out: vanilla `IllagerModel.setupAnim`'s
        // CELEBRATING branch (`AbstractIllager.IllagerArmPose.CELEBRATING`) —
        // a near-fixed roll out to the side (153° right, 135° left — vanilla
        // isn't quite symmetric) with only a small cheering wobble riding the
        // same per-entity clock idle motions use.
        (MobPose::Celebrating, RightArm) => PosePart {
            shift: [0.0; 3],
            x_rot: (anim * 13.324).cos() * 0.05,
            y_rot: 0.0,
            z_rot: 2.670354,
        },
        (MobPose::Celebrating, LeftArm) => PosePart {
            shift: [0.0; 3],
            x_rot: (anim * 13.324).cos() * 0.05,
            y_rot: 0.0,
            z_rot: -2.3561945,
        },
        // Both arms thrown out to the sides mid-cast: vanilla
        // `IllagerModel.setupAnim`'s SPELLCASTING branch — same reset-to-
        // default-pivot/wobble idiom as `Celebrating` above (this engine's
        // `part.pivot` already encodes the "reset" vanilla's own
        // `arm.x=∓5,z=0` performs, so it's a no-op `shift`), a bigger wobble
        // (0.25 vs 0.05) and a symmetric ±135° roll (vanilla's own
        // `Celebrating` roll isn't quite symmetric; `Spellcasting` is).
        (MobPose::Spellcasting, RightArm) => PosePart {
            shift: [0.0; 3],
            x_rot: (anim * 13.324).cos() * 0.25,
            y_rot: 0.0,
            z_rot: 2.3561945,
        },
        (MobPose::Spellcasting, LeftArm) => PosePart {
            shift: [0.0; 3],
            x_rot: (anim * 13.324).cos() * 0.25,
            y_rot: 0.0,
            z_rot: -2.3561945,
        },
        // A pillager (unarmed or melee) or vindicator swinging: vanilla
        // `AnimationUtils.swingWeaponDown` (main arm assumed right) plus its
        // own `bobArms`/`bobModelPart` idle sway folded in (both always run
        // together in real vanilla — never observed separately), all
        // algebraically combined into one closed form per arm. `anim` here
        // is this engine's per-entity clock in seconds, standing in for
        // vanilla's `ageInTicks` the same way `Celebrating`'s wobble already
        // does (`* 20` ticks/sec folded into each constant below).
        (MobPose::Attacking { attack_time }, RightArm) => {
            let attack2 = (attack_time * PI).sin();
            let attack = ((1.0 - (1.0 - attack_time) * (1.0 - attack_time)) * PI).sin();
            PosePart {
                shift: [0.0; 3],
                x_rot: -1.8849558 + (anim * 1.8).cos() * 0.15 + attack2 * 2.2 - attack * 0.4
                    + (anim * 1.34).sin() * 0.05,
                y_rot: 0.15707964,
                z_rot: (anim * 1.8).cos() * 0.05 + 0.05,
            }
        }
        (MobPose::Attacking { attack_time }, LeftArm) => {
            let attack2 = (attack_time * PI).sin();
            let attack = ((1.0 - (1.0 - attack_time) * (1.0 - attack_time)) * PI).sin();
            PosePart {
                shift: [0.0; 3],
                x_rot: (anim * 3.8).cos() * 0.5 + attack2 * 1.2 - attack * 0.4
                    - (anim * 1.34).sin() * 0.05,
                y_rot: -0.15707964,
                z_rot: -((anim * 1.8).cos() * 0.05 + 0.05),
            }
        }
        _ => return None,
    })
}

/// Illager arm poses keyed off the mob's own current head yaw/pitch
/// (`BOW_AND_ARROW`/`CROSSBOW_HOLD`, real `IllagerModel.setupAnim`) or off
/// how far into its crossbow draw it is (`CROSSBOW_CHARGE`) rather than the
/// shared per-entity clock every [`pose_part`] branch above uses. Kept
/// separate from that function — its `anim` parameter is always the shared
/// clock, never live head angles, and growing its signature for one family
/// of poses would touch the ~40 existing call sites that have no use for it.
pub fn illager_head_pose_part(
    pose: MobPose,
    role: PartRole,
    head_yaw: f32,
    head_pitch: f32,
) -> Option<PosePart> {
    use PartRole::*;
    Some(match (pose, role) {
        (MobPose::BowAndArrow, RightArm) => PosePart {
            shift: [0.0; 3],
            x_rot: -1.5707964 + head_pitch,
            y_rot: -0.1 + head_yaw,
            z_rot: 0.0,
        },
        (MobPose::BowAndArrow, LeftArm) => PosePart {
            shift: [0.0; 3],
            x_rot: -0.9424779 + head_pitch,
            y_rot: head_yaw - 0.4,
            z_rot: 1.5707964,
        },
        // `animateCrossbowHold(rightArm, leftArm, head, holdingInRightArm:
        // true)` — the holding (right) arm braces the stock, the shooting
        // (left) arm rests along the barrel.
        (MobPose::CrossbowHold, RightArm) => PosePart {
            shift: [0.0; 3],
            x_rot: -1.5707964 + head_pitch + 0.1,
            y_rot: -0.3 + head_yaw,
            z_rot: 0.0,
        },
        (MobPose::CrossbowHold, LeftArm) => PosePart {
            shift: [0.0; 3],
            x_rot: -1.5 + head_pitch,
            y_rot: 0.6 + head_yaw,
            z_rot: 0.0,
        },
        // `animateCrossbowCharge(rightArm, leftArm, maxDuration, ticksUsingItem,
        // holdingInRightArm: true)` — NOT head-relative at all: the holding
        // (right) arm braces at a fixed angle while the pulling (left) arm
        // lerps back as the draw progresses.
        (MobPose::CrossbowCharge { .. }, RightArm) => {
            PosePart { shift: [0.0; 3], x_rot: -0.97079635, y_rot: -0.8, z_rot: 0.0 }
        }
        (MobPose::CrossbowCharge { frac }, LeftArm) => PosePart {
            shift: [0.0; 3],
            x_rot: lerp(-0.97079635, -1.5707964, frac),
            y_rot: lerp(0.4, 0.85, frac),
            z_rot: 0.0,
        },
        _ => return None,
    })
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
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
    // 0.56.0 — the containers that open. These are drawn per frame (never
    // baked into the terrain) precisely so their lids can move.
    /// A single chest: box, and the lid the latch hangs off.
    Chest,
    /// The left half of a double chest — 15 wide, meeting its partner at the
    /// block edge, with half the latch.
    ChestLeft,
    /// The right half of a double chest.
    ChestRight,
    /// A shulker box: base shell and the lid that rises and turns as it opens.
    ShulkerBox,
    /// An open book: two covers, two blocks of pages and the spine between
    /// them — the one that floats over an enchanting table, and the one on a
    /// lectern.
    Book,
    // 0.93.0 — Mounts of Mayhem: the nautilus. Its shell never moves; only its
    // texture tells a tamed nautilus from a hostile zombie nautilus, exactly
    // like the vanilla model this is transcribed from.
    /// A nautilus: a shell plus a separate body-and-mouth assembly riding
    /// inside its front opening. Shared by the regular nautilus and (with the
    /// zombie texture) the ordinary zombie nautilus.
    Nautilus,
    /// The warm-ocean zombie nautilus's extra coral growths on its shell
    /// (`ZombieNautilusCoralModel`) — drawn as an extra layer over `Nautilus`
    /// for the warm variant, hidden the moment it's wearing body armour,
    /// exactly like vanilla.
    NautilusCorals,
    /// A copper golem: body, a head topped with a little periscope antenna,
    /// and swinging arms and legs. Its oxidation stage (unweathered through
    /// oxidized) is a texture swap, same as the block it is built from.
    CopperGolem,
    /// An evoker's fangs: a burst of teeth from the ground that snaps shut —
    /// a base cube and two jaw halves that bite together.
    EvokerFangs,
    /// A shulker's homing bullet: three crossed flat plates tumbling in
    /// flight, drawn from vanilla's own "spark" texture.
    ShulkerBullet,
    /// A llama's spit ball: a small cluster of cubes flung at a target.
    LlamaSpit,
    /// A goat's left horn — hidden the moment `HasLeftHorn` goes false (it
    /// rammed something and knocked the horn clean off, dropping a real Goat
    /// Horn item), drawn as an extra layer over `Goat` exactly like vanilla's
    /// `GoatModel.setupAnim` toggles the part's own `visible` flag.
    GoatLeftHorn,
    /// A goat's right horn — see `GoatLeftHorn`; independent of it (a goat
    /// can lose either horn, or both, separately).
    GoatRightHorn,
}

impl MobModel {
    pub fn all() -> [MobModel; 82] {
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
            Chest, ChestLeft, ChestRight, ShulkerBox, Book,
            Nautilus, NautilusCorals, CopperGolem, EvokerFangs, ShulkerBullet, LlamaSpit,
            GoatLeftHorn, GoatRightHorn,
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
        MobModel::Chest => chest(14.0, 0.0, 2.0, 0.0),
        MobModel::ChestLeft => chest(15.0, 0.5, 1.0, 7.5),
        MobModel::ChestRight => chest(15.0, -0.5, 1.0, -7.5),
        MobModel::ShulkerBox => shulker_box(),
        MobModel::Book => book(),
        MobModel::Nautilus => nautilus(),
        MobModel::NautilusCorals => nautilus_corals(),
        MobModel::CopperGolem => copper_golem(),
        MobModel::EvokerFangs => evoker_fangs(),
        MobModel::ShulkerBullet => shulker_bullet(),
        MobModel::LlamaSpit => llama_spit(),
        MobModel::GoatLeftHorn => goat_horn(-3.0),
        MobModel::GoatRightHorn => goat_horn(3.0),
    }
}

/// A chest, built the way vanilla builds it: a 10-tall box with the lid sitting
/// on top of it, hinged along its back edge, and the latch riding on the lid so
/// it swings with it. `w`/`cx` size and centre the boxes (a double chest's half
/// is a pixel wider and pushed to the block edge it shares with its partner);
/// `lw`/`lx` do the same for the latch, which is halved on a double.
fn chest(w: f32, cx: f32, lw: f32, lx: f32) -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // The box, texOffs(0,19) in the vanilla sheet.
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
                Cube::new([cx, 5.0, 0.0], [w, 10.0, 14.0], [0.0, 19.0]),
            ]),
            // Lid + latch, hinged at the back-bottom edge of the lid.
            Part::plain(PartAnim::Lid, [0.0, 9.0, -7.0], vec![
                Cube::new([cx, 2.5, 7.0], [w, 5.0, 14.0], [0.0, 0.0]),
                Cube::new([lx, 0.0, 14.5], [lw, 4.0, 1.0], [0.0, 0.0]),
            ]),
        ],
    }
}

/// An open book, as vanilla builds it: two thin covers hinged at the spine,
/// with a block of pages resting on each. The covers are angled slightly open
/// and the pages follow them, which is the shape you see hanging over every
/// enchanting table. Texture 64×32 (`entity/enchanting_table_book`).
fn book() -> ModelDef {
    // Vanilla's covers sit a little past a right angle from each other.
    const OPEN: f32 = 1.25;
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            // Left cover and the pages on it, hinged at the spine.
            Part { anim: PartAnim::Static, pivot: [0.0, 0.0, 0.0], x_rot: 0.0, y_rot: PI + OPEN, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, 0.0, -3.0], [6.0, 10.0, 0.0], [0.0, 0.0]),
                ] },
            Part { anim: PartAnim::Static, pivot: [0.0, 0.0, 0.0], x_rot: 0.0, y_rot: OPEN, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, 0.0, -3.0], [6.0, 10.0, 0.0], [16.0, 0.0]),
                ] },
            // The page blocks, a hair inside the covers.
            Part { anim: PartAnim::Static, pivot: [0.0, 0.0, 0.0], x_rot: 0.0, y_rot: PI + OPEN * 0.86, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -0.5, -2.6], [5.0, 8.0, 1.0], [0.0, 10.0])] },
            Part { anim: PartAnim::Static, pivot: [0.0, 0.0, 0.0], x_rot: 0.0, y_rot: OPEN * 0.86, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -0.5, -2.6], [5.0, 8.0, 1.0], [12.0, 10.0])] },
            // The spine holding the two halves together.
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
                Cube::new([0.0, 0.0, 0.0], [2.0, 10.0, 0.0], [12.0, 0.0]),
            ]),
        ],
    }
}

/// A shulker box: the 8-tall base and the 12-tall lid shell that overlaps it.
/// Opening lifts the lid half a block and turns it three quarters of a turn,
/// which is what makes a shulker box unmistakable from across a room.
///
/// Unlike every other model here this one is authored around the **block
/// centre** rather than standing on y=0, because a shulker box can be stuck to
/// any of the six faces: the app tips it onto that face, and tipping has to
/// happen about the middle of the block or the box would swing into its
/// neighbour.
fn shulker_box() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
                Cube::new([0.0, -4.0, 0.0], [16.0, 8.0, 16.0], [0.0, 28.0]),
            ]),
            Part::plain(PartAnim::ShulkerLid, [0.0, 0.0, 0.0], vec![
                Cube::new([0.0, 2.0, 0.0], [16.0, 12.0, 16.0], [0.0, 0.0]),
            ]),
        ],
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
            // A slime is never still: it flattens as it lands and stretches as
            // it hops, which is most of what makes one recognisable.
            anim: PartAnim::Squash,
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
            // The two oars, hung off the gunwales just behind the middle
            // (vanilla `BoatModel::addPaddle`, 2×2×18 at uv 62,0). They rest
            // along the hull and swing when the boat is being rowed.
            paddle(1.0),
            paddle(-1.0),
        ],
    }
}

/// One oar: a shaft reaching back and out from the gunwale, `side` +1 left.
fn paddle(side: f32) -> Part {
    Part {
        anim: PartAnim::Static,
        pivot: [side * 9.0, 9.0, 3.0],
        x_rot: 0.0,
        y_rot: 0.0,
        z_rot: 0.0,
        cubes: vec![Cube::new([0.0, 0.0, -7.0], [2.0, 2.0, 18.0], [62.0, 0.0])],
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
            // Tail hanging down at the back, wagging gently.
            Part { anim: PartAnim::Idle(IdleMotion::Flutter { rest: 0.0, amp: 0.30, hz: 0.8, sign: 1.0 }),
                pivot: [0.0, 12.0, -6.0], x_rot: -FRAC_PI_4, y_rot: 0.0, z_rot: 0.0,
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
            // Bushy tail, swaying behind it.
            Part { anim: PartAnim::Idle(IdleMotion::Flutter { rest: 0.0, amp: 0.22, hz: 0.55, sign: 1.0 }),
                pivot: [0.0, 8.0, -6.0], x_rot: -FRAC_PI_4 * 0.6, y_rot: 0.0, z_rot: 0.0,
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
        // The ring ripples rather than moving as one slab: each tentacle is a
        // little further through the same slow swing.
        parts.push(Part {
            anim: PartAnim::Idle(IdleMotion::Sway { amp: 0.30, hz: 0.35, phase: ang }),
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
            Part { anim: PartAnim::Idle(IdleMotion::Wing { rest: 0.30, amp: 0.45, hz: 5.0, sign: -1.0 }),
                pivot: [3.0, 12.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([5.0, -2.0, 0.0], [10.0, 12.0, 1.0], [14.0, 0.0])] },
            Part { anim: PartAnim::Idle(IdleMotion::Wing { rest: 0.30, amp: 0.45, hz: 5.0, sign: 1.0 }),
                pivot: [-3.0, 12.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
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
            // Head + beard. The horns are drawn separately (`goat_horn`) so
            // each can be hidden on its own when rammed off.
            Part::plain(PartAnim::Head, [0.0, 15.0, 7.0], vec![
                Cube::new([0.0, 0.0, 1.0], [5.0, 6.0, 7.0], [34.0, 0.0]),
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

/// One of a goat's horns, drawn as its own tiny model so it can be hidden
/// independently of the other when `HasLeftHorn`/`HasRightHorn` goes false
/// (see `MobModel::GoatLeftHorn`/`GoatRightHorn`). Same pivot and cube as
/// the horn this replaced inside `goat()`'s head part — `x` is `-3.0` for
/// the left horn, `3.0` for the right.
fn goat_horn(x: f32) -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![Part::plain(PartAnim::Head, [0.0, 15.0, 7.0], vec![
            Cube::new([x, 5.0, -2.0], [1.0, 4.0, 1.0], [50.0, 0.0]),
        ])],
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
            PartAnim::Idle(IdleMotion::Sway { amp: 0.16, hz: 0.22, phase: i as f32 * 0.7 }),
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
        // Pivot at the centre with the rod offset outward, so spinning about Y
        // carries the whole ring around the blaze like vanilla's does.
        parts.push(Part::plain(
            PartAnim::Idle(IdleMotion::Spin { hz: 0.15 }),
            [0.0, y, 0.0],
            vec![Cube::new([sx, 0.0, sz], [2.0, 8.0, 2.0], [0.0, 16.0])],
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
            // Tail + vertical tail fin. A dolphin's tail beats up and down,
            // not side to side like a fish's.
            Part { anim: PartAnim::Idle(IdleMotion::Sway { amp: 0.25, hz: 1.1, phase: 0.0 }),
                pivot: [0.0, 5.0, -6.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
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
            // Vertical tail fin, beating side to side — a fish that does not
            // move its tail reads as a dead fish.
            Part { anim: PartAnim::Idle(IdleMotion::Flutter { rest: 0.0, amp: 0.45, hz: 1.6, sign: 1.0 }),
                pivot: [0.0, 3.0, -len / 2.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
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
            Part::plain(PartAnim::Idle(IdleMotion::Flutter { rest: 0.0, amp: 0.5, hz: 2.2, sign: 1.0 }),
                [0.0, 3.0, -3.0], vec![Cube::new([0.0, 0.0, -2.0], [1.0, 3.0, 4.0], [22.0, 3.0])]),
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
            // A bee's wings are a blur; vanilla beats them far faster than any
            // other flier.
            Part { anim: PartAnim::Idle(IdleMotion::Wing { rest: 0.2, amp: 0.45, hz: 11.0, sign: -1.0 }),
                pivot: [1.5, 9.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([4.0, 0.0, -1.0], [9.0, 0.0, 6.0], [0.0, 0.0])] },
            Part { anim: PartAnim::Idle(IdleMotion::Wing { rest: 0.2, amp: 0.45, hz: 11.0, sign: 1.0 }),
                pivot: [-1.5, 9.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
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

/// Nautilus (128×128, Mounts of Mayhem): a rounded shell with a separate
/// body-and-three-part-mouth assembly nested in its front opening. Vanilla
/// tilts that assembly a few degrees to follow the swim direction; left out
/// here so the mouth can never visibly drift off the body it is bolted to,
/// since every part below is placed independently rather than parented.
fn nautilus() -> ModelDef {
    ModelDef {
        tex_w: 128.0,
        tex_h: 128.0,
        scale: PX,
        parts: vec![
            // The shell: a rounded top half, a taller lower half, and a flat
            // cap closing its front opening where the body sits.
            Part { anim: PartAnim::Static, pivot: [0.0, 16.0, -1.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, -5.0, 1.0], [14.0, 10.0, 16.0], [0.0, 0.0]),
                    Cube::new([0.0, 4.0, 3.0], [14.0, 8.0, 20.0], [0.0, 26.0]),
                    Cube::new([0.0, 4.0, 6.0], [14.0, 8.0, 0.0], [48.0, 26.0]),
                ] },
            // Body, riding inside the shell's opening.
            Part { anim: PartAnim::Static, pivot: [0.0, 20.5, 6.3], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([0.0, -0.51, 4.0], [10.0, 8.0, 14.0], [0.0, 54.0]),
                    Cube::new([0.0, -0.51, 7.0], [10.0, 8.0, 0.0], [0.0, 76.0]),
                ] },
            // The three-piece mouth on the front of the body.
            Part { anim: PartAnim::Static, pivot: [0.0, 17.99, 13.3], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 2.0], [10.0, 4.0, 4.0], [54.0, 54.0])] },
            Part { anim: PartAnim::Static, pivot: [0.0, 19.99, 13.8], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.0, 1.5], [6.0, 4.0, 4.0], [54.0, 70.0])] },
            Part { anim: PartAnim::Static, pivot: [0.0, 21.99, 13.3], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, 0.02, 2.0], [10.0, 4.0, 4.0], [54.0, 62.0])] },
        ],
    }
}

/// The warm-ocean zombie nautilus's coral growths (`ZombieNautilusCoralModel`,
/// same 128×128 canvas as the base nautilus above), drawn as an extra layer
/// over `Nautilus` for the warm variant. Vanilla hangs this whole "corals"
/// group off the *shell* part (not the mesh root), three levels deep in
/// places (corals → a coral clump → its two angled fronds); this engine has
/// no part nesting, so every leaf clump is flattened to one absolute pivot —
/// the shell's own pivot `[0, 16, -1]` (see `nautilus()`) plus the sum of
/// every ancestor's local offset — carrying each ancestor's baked rotation
/// along with it. The nautilus family has no `.transformed(0, N, 0)` wrapper
/// and no renderer Y-flip, so (unlike the copper golem) every offset, cube
/// corner and baked rotation here copies straight from the decompiled
/// `PartPose` values with no sign changes at all.
fn nautilus_corals() -> ModelDef {
    // Shell pivot [0, 16, -1] + this group's own offset(8, 4.5, -8).
    const CORALS: [f32; 3] = [8.0, 20.5, -9.0];
    ModelDef {
        tex_w: 128.0,
        tex_h: 128.0,
        scale: PX,
        parts: vec![
            // Yellow coral: two fronds splayed ±45° off a shared root.
            Part { anim: PartAnim::Static,
                pivot: [CORALS[0], CORALS[1] - 11.0, CORALS[2] + 11.0],
                x_rot: 0.0, y_rot: 0.7854, z_rot: 0.0,
                cubes: vec![Cube::new([-1.5, 0.5, 0.0], [6.0, 8.0, 0.0], [0.0, 85.0])] },
            Part { anim: PartAnim::Static,
                pivot: [CORALS[0], CORALS[1] - 11.0, CORALS[2] + 13.0],
                x_rot: 0.0, y_rot: -0.7854, z_rot: 0.0,
                cubes: vec![Cube::new([-1.5, 0.5, 0.0], [6.0, 8.0, 0.0], [0.0, 85.0])] },
            // Pink coral: a flat frond plus a second one turned 90° off it.
            Part { anim: PartAnim::Static,
                pivot: [CORALS[0] - 12.5, CORALS[1] - 18.0, CORALS[2] + 11.0],
                x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([-1.5, 4.5, 4.0], [6.0, 0.0, 8.0], [-8.0, 94.0])] },
            Part { anim: PartAnim::Static,
                pivot: [CORALS[0] - 14.0, CORALS[1] - 13.5, CORALS[2] + 15.0],
                x_rot: 0.0, y_rot: 0.0, z_rot: 1.5708,
                cubes: vec![Cube::new([0.0, 0.0, 0.0], [6.0, 0.0, 8.0], [-8.0, 94.0])] },
            // Blue coral: two fronds splayed ±45°.
            Part { anim: PartAnim::Static,
                pivot: [CORALS[0] - 14.0, CORALS[1], CORALS[2] + 5.5],
                x_rot: 0.0, y_rot: -0.7854, z_rot: 0.0,
                cubes: vec![Cube::new([-1.0, -0.5, 0.0], [5.0, 10.0, 0.0], [0.0, 102.0])] },
            Part { anim: PartAnim::Static,
                pivot: [CORALS[0] - 14.0, CORALS[1], CORALS[2] + 3.5],
                x_rot: 0.0, y_rot: 0.7854, z_rot: 0.0,
                cubes: vec![Cube::new([-1.0, -0.5, 0.0], [5.0, 10.0, 0.0], [0.0, 102.0])] },
            // Red coral: two fronds, one splayed 45°, one at a shallower angle.
            Part { anim: PartAnim::Static,
                pivot: CORALS,
                x_rot: 0.0, y_rot: 0.7854, z_rot: 0.0,
                cubes: vec![Cube::new([-1.5, -0.5, 0.0], [6.0, 10.0, 0.0], [0.0, 112.0])] },
            Part { anim: PartAnim::Static,
                pivot: [CORALS[0] - 0.5, CORALS[1] - 1.0, CORALS[2] + 1.5],
                x_rot: 0.0, y_rot: -0.829, z_rot: 0.0,
                cubes: vec![Cube::new([-0.5, -0.5, 0.0], [4.0, 10.0, 0.0], [0.0, 112.0])] },
        ],
    }
}

/// Copper Golem (64×64, Mounts of Mayhem): a small wind-up automaton — a
/// boxy body, a head topped with a little periscope antenna, and swinging
/// arms and legs. Vanilla's own model authors this one in the older
/// top-down-from-24 convention (unlike the nautilus above): its mesh is
/// wrapped in a `translated(0, 24, 0)` before any part offset is read, and
/// every offset and box corner is then negated on Y relative to those raw
/// numbers. The two cancel out — dropping vanilla's +24 here (rather than
/// carrying it into the pivot) is what actually lands the feet on y=0, which
/// is the confirmation this reading is right: legs sit below body below
/// head, touching cleanly at each seam, with the lowest cube exactly on the
/// ground this engine expects a standing mob's feet to be on.
fn copper_golem() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 64.0,
        scale: PX,
        parts: vec![
            // Body.
            Part::plain(PartAnim::Static, [0.0, 5.0, 0.0], vec![
                Cube::new([0.0, 3.0, 0.0], [8.0, 6.0, 6.0], [0.0, 15.0]),
            ]),
            // Head: the main block, a chin nub at the front, and the neck
            // post + antenna cap stacked straight up out of the top.
            Part::plain(PartAnim::Head, [0.0, 11.0, 0.0], vec![
                Cube::new([0.0, 2.5, 0.0], [8.0, 5.0, 10.0], [0.0, 0.0]),
                Cube::new([0.0, 0.5, -5.0], [2.0, 3.0, 2.0], [56.0, 0.0]),
                Cube::new([0.0, 7.0, 0.0], [2.0, 4.0, 2.0], [37.0, 8.0]),
                Cube::new([0.0, 11.0, 0.0], [4.0, 4.0, 4.0], [37.0, 0.0]),
            ]),
            // Arms, swinging opposite to the same-side leg for a natural gait.
            Part { anim: PartAnim::Leg(-1.0), pivot: [-4.0, 11.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([-1.5, -4.0, 0.0], [3.0, 10.0, 4.0], [36.0, 16.0])] },
            Part { anim: PartAnim::Leg(1.0), pivot: [4.0, 11.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([1.5, -4.0, 0.0], [3.0, 10.0, 4.0], [50.0, 16.0])] },
            // Legs.
            Part { anim: PartAnim::Leg(1.0), pivot: [0.0, 5.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([-2.0, -2.5, 0.0], [4.0, 5.0, 4.0], [0.0, 27.0])] },
            Part { anim: PartAnim::Leg(-1.0), pivot: [0.0, 5.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([2.0, -2.5, 0.0], [4.0, 5.0, 4.0], [16.0, 27.0])] },
        ],
    }
}

/// Evoker fangs (64×32): a base cube plus two jaw halves that snap shut.
/// Vanilla's own model is authored the same "top-down-from-24" way as the
/// copper golem above (its renderer applies its own Y-flip too), so the same
/// drop-the-24/negate-Y-of-the-rest reading applies. Unlike every other model
/// here, the vertical "bursting out of the ground" motion isn't a part
/// rotation at all — it is vanilla's `root`/`base` *position* changing every
/// frame, which this engine's baked-once meshes can't express, so the caller
/// folds that into the entity's world position instead and leaves this mesh's
/// own pivots at their structural (non-animated) relationship to each other.
fn evoker_fangs() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![
            Part::plain(PartAnim::Static, [-5.0, 0.0, -5.0], vec![
                Cube::new([5.0, -6.0, 5.0], [10.0, 12.0, 10.0], [0.0, 0.0]),
            ]),
            // Upper jaw: sign -1 so it opens the other way from the lower.
            Part::plain(PartAnim::Jaw(-1.0), [1.5, 0.0, -4.0], vec![
                Cube::new([2.0, -7.0, 4.0], [4.0, 14.0, 8.0], [40.0, 0.0]),
            ]),
            Part::plain(PartAnim::Jaw(1.0), [-1.5, 0.0, 4.0], vec![
                Cube::new([2.0, -7.0, 4.0], [4.0, 14.0, 8.0], [40.0, 0.0]),
            ]),
        ],
    }
}

/// A shulker bullet (64×32): three perpendicular flat plates crossing through
/// the origin, drawn from vanilla's "spark" texture. Fully symmetric about its
/// own center, so unlike the fangs above, the sign of vanilla's Y-flip doesn't
/// actually change any of these numbers — negating zero is still zero.
fn shulker_bullet() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
            Cube::new([0.0, 0.0, 0.0], [8.0, 8.0, 2.0], [0.0, 0.0]),
            Cube::new([0.0, 0.0, 0.0], [2.0, 8.0, 8.0], [0.0, 10.0]),
            Cube::new([0.0, 0.0, 0.0], [8.0, 2.0, 8.0], [20.0, 0.0]),
        ])],
    }
}

/// A llama's spit ball (64×32): seven small cubes clustered around a center
/// cube, one poking out along each axis. Vanilla's own renderer never flips
/// this one (no scale call at all in its `submit`), so — unlike every other
/// model on this page — its numbers copy straight across with no Y negation.
fn llama_spit() -> ModelDef {
    ModelDef {
        tex_w: 64.0,
        tex_h: 32.0,
        scale: PX,
        parts: vec![Part::plain(PartAnim::Static, [0.0, 0.0, 0.0], vec![
            Cube::new([-3.0, 1.0, 1.0], [2.0, 2.0, 2.0], [0.0, 0.0]),
            Cube::new([1.0, -3.0, 1.0], [2.0, 2.0, 2.0], [0.0, 0.0]),
            Cube::new([1.0, 1.0, -3.0], [2.0, 2.0, 2.0], [0.0, 0.0]),
            Cube::new([1.0, 1.0, 1.0], [2.0, 2.0, 2.0], [0.0, 0.0]),
            Cube::new([3.0, 1.0, 1.0], [2.0, 2.0, 2.0], [0.0, 0.0]),
            Cube::new([1.0, 3.0, 1.0], [2.0, 2.0, 2.0], [0.0, 0.0]),
            Cube::new([1.0, 1.0, 3.0], [2.0, 2.0, 2.0], [0.0, 0.0]),
        ])],
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
            Part::plain(PartAnim::Idle(IdleMotion::Wing { rest: 0.0, amp: 0.30, hz: 3.5, sign: -1.0 }),
                [2.0, 5.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [1.0, 5.0, 3.0], [19.0, 8.0])]),
            Part::plain(PartAnim::Idle(IdleMotion::Wing { rest: 0.0, amp: 0.30, hz: 3.5, sign: 1.0 }),
                [-2.0, 5.0, 0.0], vec![Cube::new([0.0, 0.0, 0.0], [1.0, 5.0, 3.0], [19.0, 8.0])]),
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
            Part { anim: PartAnim::Idle(IdleMotion::Wing { rest: 0.30, amp: 0.28, hz: 1.4, sign: -1.0 }),
                pivot: [2.0, 4.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([6.0, 0.0, -1.0], [13.0, 1.0, 7.0], [23.0, 12.0]),
                    Cube::new([16.0, 0.0, 1.0], [8.0, 1.0, 4.0], [16.0, 24.0]),
                ] },
            Part { anim: PartAnim::Idle(IdleMotion::Wing { rest: 0.30, amp: 0.28, hz: 1.4, sign: 1.0 }),
                pivot: [-2.0, 4.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
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
            Part { anim: PartAnim::Idle(IdleMotion::Flutter { rest: -0.35, amp: 0.45, hz: 7.0, sign: 1.0 }),
                pivot: [0.5, 8.0, 1.5], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([0.0, -4.0, 0.0], [0.0, 7.0, 4.0], [16.0, 14.0])] },
            Part { anim: PartAnim::Idle(IdleMotion::Flutter { rest: 0.35, amp: 0.45, hz: 7.0, sign: -1.0 }),
                pivot: [-0.5, 8.0, 1.5], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
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
            Part { anim: PartAnim::Idle(IdleMotion::Flutter { rest: -0.4, amp: 0.5, hz: 8.0, sign: 1.0 }),
                pivot: [0.0, 18.0, 2.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![Cube::new([2.0, -3.0, 0.0], [8.0, 10.0, 0.0], [16.0, 14.0])] },
            Part { anim: PartAnim::Idle(IdleMotion::Flutter { rest: 0.4, amp: 0.5, hz: 8.0, sign: -1.0 }),
                pivot: [0.0, 18.0, 2.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
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
            Part { anim: PartAnim::Idle(IdleMotion::Wing { rest: -0.12, amp: 0.22, hz: 0.35, sign: 1.0 }),
                pivot: [10.0, 26.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
                cubes: vec![
                    Cube::new([28.0, 0.0, 0.0], [56.0, 2.0, 8.0], [112.0, 88.0]),
                    Cube::new([28.0, -1.0, -14.0], [56.0, 0.0, 24.0], [0.0, 152.0]),
                ] },
            Part { anim: PartAnim::Idle(IdleMotion::Wing { rest: -0.12, amp: 0.22, hz: 0.35, sign: -1.0 }),
                pivot: [-10.0, 26.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: 0.0,
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
            // Lid on top — rises and turns as the shulker peeks out, same
            // formula as a shulker box's lid (see `PartAnim::ShulkerLid`).
            Part::plain(PartAnim::ShulkerLid, [0.0, 8.0, 0.0], vec![Cube::new([0.0, 4.0, 0.0], [16.0, 12.0, 16.0], [0.0, 0.0])]),
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
            Part { anim: PartAnim::Static, pivot: [4.5, 6.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: -FRAC_PI_6,
                cubes: vec![Cube::new([0.5, -2.5, 0.0], [1.0, 5.0, 4.0], [51.0, 6.0])] },
            Part { anim: PartAnim::Static, pivot: [-4.5, 6.0, 0.0], x_rot: 0.0, y_rot: 0.0, z_rot: FRAC_PI_6,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Poses move parts *by index*, so a model that grows a part without its
    /// role list growing too would quietly pose the wrong limb.
    #[test]
    fn part_roles_match_their_models() {
        for m in MobModel::all() {
            let roles = part_roles(m);
            if roles.is_empty() {
                continue;
            }
            assert_eq!(roles.len(), model_def(m).parts.len(), "{m:?} role list is out of date");
        }
    }

    /// Every posed model must actually have legs to hang the pose off — except
    /// a boat (paddles, not legs), a biped whose pose only moves its arms, or
    /// an allay (whose dance pose only moves its head and whole-body root).
    #[test]
    fn posed_models_have_legs() {
        for m in MobModel::all() {
            let roles = part_roles(m);
            if roles.is_empty()
                || m == MobModel::Boat
                || m == MobModel::Illager
                || m == MobModel::Allay
                // A creaking is a biped with one leg pair, not a quadruped's
                // front/back pair — it has `LeftLeg`/`RightLeg` instead.
                || m == MobModel::Creaking
            {
                continue;
            }
            assert!(
                roles.iter().any(|r| *r == PartRole::FrontLeg),
                "{m:?} has a pose but no front leg"
            );
        }
    }

    #[test]
    fn celebrating_throws_both_raider_arms_up_and_out() {
        let r = pose_part(MobPose::Celebrating, PartRole::RightArm, 0.0, 0.0, 1.0).expect("right arm pose");
        let l = pose_part(MobPose::Celebrating, PartRole::LeftArm, 0.0, 0.0, 1.0).expect("left arm pose");
        assert!((r.z_rot - 2.670354).abs() < 1e-6);
        assert!((l.z_rot - (-2.3561945)).abs() < 1e-6);
        // At anim=0 the cheering wobble is at its peak (cos(0) == 1).
        assert!((r.x_rot - 0.05).abs() < 1e-6);
    }

    #[test]
    fn spellcasting_throws_both_arms_out_symmetrically() {
        let r = pose_part(MobPose::Spellcasting, PartRole::RightArm, 0.0, 0.0, 1.0).expect("right arm");
        let l = pose_part(MobPose::Spellcasting, PartRole::LeftArm, 0.0, 0.0, 1.0).expect("left arm");
        assert!((r.z_rot - 2.3561945).abs() < 1e-6);
        assert!((l.z_rot - (-2.3561945)).abs() < 1e-6, "unlike Celebrating, Spellcasting IS symmetric");
        // At anim=0 the wobble is at its peak (cos(0) == 1), bigger than Celebrating's.
        assert!((r.x_rot - 0.25).abs() < 1e-6);
    }

    #[test]
    fn attacking_swings_the_right_arm_down_and_forward_at_full_attack_time() {
        let idle = pose_part(MobPose::Attacking { attack_time: 0.0 }, PartRole::RightArm, 0.0, 0.0, 1.0)
            .expect("idle attack stance");
        let mid = pose_part(MobPose::Attacking { attack_time: 0.5 }, PartRole::RightArm, 0.0, 0.0, 1.0)
            .expect("mid-swing");
        // The swing sweeps the arm forward past its idle drop.
        assert!(mid.x_rot > idle.x_rot);
        let left = pose_part(MobPose::Attacking { attack_time: 0.5 }, PartRole::LeftArm, 0.0, 0.0, 1.0)
            .expect("off arm");
        assert!((left.y_rot - (-0.15707964)).abs() < 1e-6);
    }

    #[test]
    fn bow_and_arrow_is_a_pure_function_of_current_head_angles() {
        let r = illager_head_pose_part(MobPose::BowAndArrow, PartRole::RightArm, 0.2, -0.1)
            .expect("right arm");
        let l = illager_head_pose_part(MobPose::BowAndArrow, PartRole::LeftArm, 0.2, -0.1)
            .expect("left arm");
        assert!((r.y_rot - (-0.1 + 0.2)).abs() < 1e-6);
        assert!((r.x_rot - (-1.5707964 + -0.1)).abs() < 1e-6);
        assert!((l.z_rot - 1.5707964).abs() < 1e-6);
        assert!(illager_head_pose_part(MobPose::BowAndArrow, PartRole::Head, 0.2, -0.1).is_none());
    }

    #[test]
    fn crossbow_hold_braces_a_static_head_relative_pose() {
        let r = illager_head_pose_part(MobPose::CrossbowHold, PartRole::RightArm, 0.0, 0.0)
            .expect("holding arm");
        let l = illager_head_pose_part(MobPose::CrossbowHold, PartRole::LeftArm, 0.0, 0.0)
            .expect("shooting arm");
        assert!((r.y_rot - (-0.3)).abs() < 1e-6);
        assert!((l.y_rot - 0.6).abs() < 1e-6);
    }

    #[test]
    fn crossbow_charge_lerps_the_pulling_arm_and_leaves_the_holding_arm_fixed() {
        let hold_at_0 = illager_head_pose_part(MobPose::CrossbowCharge { frac: 0.0 }, PartRole::RightArm, 1.0, 1.0)
            .expect("holding arm");
        let hold_at_1 = illager_head_pose_part(MobPose::CrossbowCharge { frac: 1.0 }, PartRole::RightArm, -1.0, -1.0)
            .expect("holding arm, unaffected by head angle");
        assert_eq!(hold_at_0.x_rot, hold_at_1.x_rot, "the holding arm never head-tracks or lerps");
        let pull_at_0 = illager_head_pose_part(MobPose::CrossbowCharge { frac: 0.0 }, PartRole::LeftArm, 0.0, 0.0)
            .expect("pulling arm at 0%");
        let pull_at_1 = illager_head_pose_part(MobPose::CrossbowCharge { frac: 1.0 }, PartRole::LeftArm, 0.0, 0.0)
            .expect("pulling arm at 100%");
        assert!((pull_at_0.y_rot - 0.4).abs() < 1e-6);
        assert!((pull_at_1.y_rot - 0.85).abs() < 1e-6);
        assert!((pull_at_1.x_rot - (-1.5707964)).abs() < 1e-6);
    }

    #[test]
    fn sitting_folds_the_hind_legs_forward_and_drops_them() {
        let p = pose_part(MobPose::Sitting, PartRole::BackLeg, 0.5, 0.0, 1.0).expect("hind leg pose");
        assert!(p.x_rot < -1.5, "the hind legs should fold a right angle forward");
        assert!(p.shift[1] < 0.0 && p.shift[2] > 0.0, "and drop toward the ground, forward");
    }

    /// Nothing is posed unless the pose says so — a walking dog keeps walking.
    #[test]
    fn no_pose_means_no_override() {
        for role in [PartRole::Body, PartRole::Head, PartRole::FrontLeg, PartRole::Tail] {
            assert!(pose_part(MobPose::None, role, 0.5, 0.0, 1.0).is_none());
        }
    }

    #[test]
    fn oars_swing_only_while_they_are_pulled() {
        let idle = pose_part(MobPose::Rowing { left: false, right: false }, PartRole::PaddleLeft, 0.5, 3.0, 1.0)
            .expect("idle oar");
        let same = pose_part(MobPose::Rowing { left: false, right: false }, PartRole::PaddleLeft, 0.5, 9.0, 1.0)
            .expect("idle oar");
        assert_eq!(idle.x_rot, same.x_rot, "an oar out of the water does not move");
        let rowing = pose_part(MobPose::Rowing { left: true, right: false }, PartRole::PaddleLeft, 0.5, 0.3, 1.0)
            .expect("rowing oar");
        assert!(rowing.x_rot != idle.x_rot, "a pulled oar does");
        // The two oars mirror each other, so a boat rows evenly.
        let right = pose_part(MobPose::Rowing { left: true, right: true }, PartRole::PaddleRight, 0.5, 0.3, 1.0)
            .expect("rowing oar");
        assert!((right.y_rot + rowing.y_rot).abs() < 1e-6);
    }

    /// An allay's whole-body spin only turns while `is_spinning`, and the
    /// sway fades out to nothing as `spin_progress` reaches 1 — matching
    /// real `AllayModel.setupAnim`'s `(1.0 - spinningRotation)` factor.
    #[test]
    fn allay_dance_sways_and_spins() {
        let (x, y, z, lift) = pose_root(MobPose::Dancing { is_spinning: false, spin_progress: 0.0 }, 0.0);
        assert_eq!((x, y, lift), (0.0, 0.0, 0.0));
        assert!(z.abs() > 0.0, "should sway at anim=0 (cos(0)=1)");
        let (_, y_spin, z_full_fade, _) =
            pose_root(MobPose::Dancing { is_spinning: true, spin_progress: 1.0 }, 0.0);
        assert!((y_spin - std::f32::consts::PI * 4.0).abs() < 1e-5, "full spin is 4π");
        assert_eq!(z_full_fade, 0.0, "sway fully fades out at spin_progress=1");
        let (_, y_not_spinning, _, _) =
            pose_root(MobPose::Dancing { is_spinning: false, spin_progress: 0.5 }, 0.0);
        assert_eq!(y_not_spinning, 0.0, "no spin rotation outside the spinning phase");
    }

    /// A rolling panda's legs kick at full amplitude the instant `amount`
    /// leaves 0 (real vanilla's leg formula ignores the ease entirely, only
    /// the head lerps by it), and front/back + left/right all move opposite
    /// each other.
    #[test]
    fn panda_rolling_kicks_all_four_legs_oppositely() {
        // anim*10 == π/2, where sin peaks at 1 — the amplitude is easiest to
        // check there.
        let a = std::f32::consts::FRAC_PI_2 / 10.0;
        let front_r = pose_part(MobPose::Rolling { amount: 0.01 }, PartRole::FrontLeg, 0.5, a, 1.0)
            .expect("front leg");
        let front_l = pose_part(MobPose::Rolling { amount: 0.01 }, PartRole::FrontLeg, 0.5, a, -1.0)
            .expect("front leg");
        assert_eq!(front_r.x_rot, -front_l.x_rot);
        assert!(front_r.x_rot.abs() > 0.4, "full amplitude even at a tiny roll amount");
        let back_r = pose_part(MobPose::Rolling { amount: 0.01 }, PartRole::BackLeg, 0.5, a, 1.0)
            .expect("back leg");
        assert_eq!(front_r.x_rot, -back_r.x_rot, "front and back legs kick opposite phase");
        let head = pose_part(MobPose::Rolling { amount: 0.5 }, PartRole::Head, 0.5, 0.0, 1.0).expect("head");
        assert!((head.x_rot - 0.5 * 2.0561945).abs() < 1e-6, "head eases smoothly by amount");
        assert!(pose_part(MobPose::Rolling { amount: 0.0 }, PartRole::FrontLeg, 0.5, a, 1.0).is_none());
    }

    /// A faceplanted fox's legs scramble off `EntityTrack::leg_motion_pos`
    /// (passed in as `anim` here), matching real `FoxModel`'s four-way phase
    /// offset (right-hind and left-front share a phase, left-hind and
    /// right-front share the opposite one).
    #[test]
    fn fox_faceplant_scrambles_legs_out_of_phase() {
        let a = std::f32::consts::FRAC_PI_2 / 0.4662;
        let front_r = pose_part(MobPose::Faceplanted, PartRole::FrontLeg, 0.5, a, 1.0).expect("front leg");
        let hind_r = pose_part(MobPose::Faceplanted, PartRole::BackLeg, 0.5, a, 1.0).expect("hind leg");
        assert_eq!(front_r.x_rot, -hind_r.x_rot);
        let front_l = pose_part(MobPose::Faceplanted, PartRole::FrontLeg, 0.5, a, -1.0).expect("front leg");
        assert_eq!(front_r.x_rot, -front_l.x_rot);
    }

    /// A playing-dead axolotl's legs splay out the instant `factor` leaves 0,
    /// scaling linearly with it (real `setupPlayDeadAnimation`'s plain
    /// `+= K * factor` terms — no ease baked into the pose itself, unlike
    /// Rolling's head), and the right-side legs mirror the left's y/z but
    /// share its x (matching real vanilla's own `applyMirrorLegRotations`,
    /// which never touches x).
    #[test]
    fn axolotl_playing_dead_splays_legs_and_mirrors_the_right_side() {
        assert!(pose_part(MobPose::PlayingDead { factor: 0.0 }, PartRole::FrontLeg, 0.5, 0.0, 1.0).is_none());
        let front_l =
            pose_part(MobPose::PlayingDead { factor: 1.0 }, PartRole::FrontLeg, 0.5, 0.0, 1.0).expect("left front");
        let front_r =
            pose_part(MobPose::PlayingDead { factor: 1.0 }, PartRole::FrontLeg, 0.5, 0.0, -1.0).expect("right front");
        assert!((front_l.x_rot - 0.7853982).abs() < 1e-6);
        assert_eq!(front_l.x_rot, front_r.x_rot, "x is not mirrored");
        assert_eq!(front_l.y_rot, -front_r.y_rot, "y is mirrored");
        let half = pose_part(MobPose::PlayingDead { factor: 0.5 }, PartRole::FrontLeg, 0.5, 0.0, 1.0).expect("half");
        assert!((half.x_rot - front_l.x_rot * 0.5).abs() < 1e-6, "scales linearly with factor");
        let hind_l =
            pose_part(MobPose::PlayingDead { factor: 1.0 }, PartRole::BackLeg, 0.5, 0.0, 1.0).expect("left hind");
        assert!((hind_l.x_rot - 1.4137167).abs() < 1e-6);
        assert!((hind_l.y_rot - 1.0995574).abs() < 1e-6);
        assert!((hind_l.z_rot - 0.7853982).abs() < 1e-6);
        // Body keeps its own lay-flat bake plus the real delta on top.
        let body = pose_part(MobPose::PlayingDead { factor: 1.0 }, PartRole::Body, 0.5, 0.0, 1.0).expect("body");
        assert!((body.x_rot - (FRAC_PI_2 - 0.15)).abs() < 1e-6);
        assert!((body.z_rot - 0.35).abs() < 1e-6);
    }

    /// Real `KeyframeAnimation.Entry.apply`, both interpolation modes.
    #[test]
    fn keyframe_track_sampling_matches_real_vanilla() {
        use Interp::*;
        let linear = [kf(0.0, [0.0, 0.0, 0.0], Linear), kf(1.0, [10.0, 0.0, 0.0], Linear)];
        assert_eq!(sample_track(&linear, 0.0), [0.0, 0.0, 0.0]);
        assert_eq!(sample_track(&linear, 1.0), [10.0, 0.0, 0.0]);
        assert_eq!(sample_track(&linear, 0.5), [5.0, 0.0, 0.0]);
        // Before the first / after the last keyframe: clamp to the nearest end.
        assert_eq!(sample_track(&linear, -1.0), [0.0, 0.0, 0.0]);
        assert_eq!(sample_track(&linear, 5.0), [10.0, 0.0, 0.0]);
        // Catmull-Rom through four real `CAMEL_DASH`-shaped control points:
        // hand-computed via `Mth.catmullrom` at its own defined midpoint.
        let cr = [
            kf(0.0, [67.5, 0.0, 0.0], CatmullRom),
            kf(0.125, [112.5, 0.0, 0.0], CatmullRom),
            kf(0.25, [67.5, 0.0, 0.0], CatmullRom),
            kf(0.375, [112.5, 0.0, 0.0], CatmullRom),
        ];
        // At the second keyframe itself, alpha=0 into the (1,2) segment: the
        // spline must still pass exactly through the keyframe's own value.
        let at_kf = sample_track(&cr, 0.125);
        assert!((at_kf[0] - 112.5).abs() < 1e-3);
        let mid = catmullrom(0.5, 67.5, 112.5, 67.5, 112.5);
        assert_eq!(sample_track(&cr, 0.1875)[0], mid);
    }

    /// `CAMEL_DASH` loops every 0.5s (real `getElapsedSeconds`'s `% length`),
    /// and its front legs are a genuine quarter-cycle-out-of-phase mirror —
    /// not a simple sign flip of one shared track — so left and right must
    /// each come from their own real keyframe data.
    #[test]
    fn camel_dash_loops_and_mirrors_front_legs_out_of_phase() {
        let at_0 = camel_dash_track(camel_dash::RIGHT_FRONT_LEG, 0.0);
        let looped = camel_dash_track(camel_dash::RIGHT_FRONT_LEG, 0.5);
        assert_eq!(at_0, looped, "the clip loops every 0.5s");
        let right_at_0 = camel_dash_track(camel_dash::RIGHT_FRONT_LEG, 0.0);
        let left_at_0 = camel_dash_track(camel_dash::LEFT_FRONT_LEG, 0.0);
        assert!(
            (right_at_0[0] - left_at_0[0]).abs() > 1.0,
            "left/right front legs are a quarter-cycle apart, not identical at t=0"
        );
        let dashing_r =
            pose_part(MobPose::Dashing { elapsed_secs: 0.0 }, PartRole::FrontLeg, 0.5, 0.0, -1.0).expect("right");
        assert!((dashing_r.x_rot - right_at_0[0]).abs() < 1e-5);
    }

    /// `CREAKING_WALK` loops every 1.125s, and a child of real vanilla's
    /// "upper_body" bone (head/left_arm/right_arm) must show upper_body's
    /// rotation SUMMED with its own local track — not upper_body's alone,
    /// and not its own local track alone.
    #[test]
    fn creaking_walk_loops_and_sums_upper_body_into_its_children() {
        let at_0 = creaking_walk_track(creaking_walk::UPPER_BODY_ROT, 0.0);
        let looped = creaking_walk_track(creaking_walk::UPPER_BODY_ROT, 1.125);
        assert_eq!(at_0, looped, "the clip loops every 1.125s");
        // t=0.0417 sits exactly on a HEAD_ROT keyframe with a large,
        // distinctive y value, so upper_body-alone vs. the real summed
        // result are unambiguously different.
        let t = 0.0417;
        let head_pose =
            pose_part(MobPose::CreakingWalking { elapsed_secs: t }, PartRole::Head, 0.0, 0.0, 0.0).expect("head");
        let ub = creaking_walk_track(creaking_walk::UPPER_BODY_ROT, t);
        let head_only = creaking_walk_track(creaking_walk::HEAD_ROT, t);
        assert!((head_pose.x_rot - (ub[0] + head_only[0])).abs() < 1e-5, "head = upper_body + its own local track");
        assert!((head_pose.y_rot - (ub[1] + head_only[1])).abs() < 1e-5);
        assert!(
            (head_pose.y_rot - ub[1]).abs() > 0.5,
            "head must not be upper_body's rotation alone — its own local track contributes a big -62.5° here"
        );
    }

    /// `CREAKING_DEATH` doesn't loop — past its 2.25s length it must hold the
    /// final keyframe's value forever, matching real vanilla's non-looping
    /// `AnimationState` (never wrapping back to the start).
    #[test]
    fn creaking_death_holds_its_last_keyframe_past_the_clip_end() {
        let at_end = creaking_track(creaking_death::UPPER_BODY_ROT, 2.25);
        let past_end = creaking_track(creaking_death::UPPER_BODY_ROT, 9.0);
        assert_eq!(at_end, past_end, "a non-looping clip holds, it doesn't wrap");
    }

    /// A creaking's legs are never children of "upper_body" in real vanilla
    /// (added straight to "root"), so — unlike head/arms — `LeftLeg`/
    /// `RightLeg` must show ONLY their own track, with no upper_body summed
    /// in, and each side must come from its own real per-side data (a real
    /// diagonal gait, not a mirrored single track).
    #[test]
    fn creaking_walk_legs_are_not_children_of_upper_body_and_differ_left_to_right() {
        let t = 0.125;
        let left = creaking_walk_track(creaking_walk::LEFT_LEG_ROT, t);
        let right = creaking_walk_track(creaking_walk::RIGHT_LEG_ROT, t);
        assert!((left[0] - right[0]).abs() > 1.0, "left/right legs run their own real timing, not a mirror");
        let left_pose =
            pose_part(MobPose::CreakingWalking { elapsed_secs: t }, PartRole::LeftLeg, 0.0, 0.0, 0.0).expect("leg");
        assert!((left_pose.x_rot - left[0]).abs() < 1e-6, "leg pose is the raw track, no upper_body summed in");
    }

    /// `CREAKING_INVULNERABLE` has no head or leg channel at all — the head
    /// must still show upper_body's own rotation (a real hierarchy child
    /// inherits its parent's rotation even with no local delta of its own),
    /// while the legs (never upper_body's children) get no override at all.
    #[test]
    fn creaking_flashing_leaves_legs_alone_but_still_turns_the_head_with_the_body() {
        let ub = creaking_track(creaking_invulnerable::UPPER_BODY_ROT, 0.1);
        let head_pose =
            pose_part(MobPose::CreakingFlashing { elapsed_secs: 0.1 }, PartRole::Head, 0.0, 0.0, 0.0).expect("head");
        assert!((head_pose.x_rot - ub[0]).abs() < 1e-6);
        assert!(
            pose_part(MobPose::CreakingFlashing { elapsed_secs: 0.1 }, PartRole::LeftLeg, 0.0, 0.0, 0.0).is_none(),
            "invulnerable never touches legs, so they should fall back to the default idle animation"
        );
    }

    #[test]
    fn every_model_is_reachable_and_densely_indexed() {
        // `all()` must stay in exact sync with the enum's own declaration
        // order — `index()` (a bare `as usize`) depends on it, and the
        // renderer's mesh table is built by baking `all()` in order.
        let all = MobModel::all();
        for (i, m) in all.iter().enumerate() {
            assert_eq!(m.index(), i, "{m:?} is out of order in MobModel::all()");
        }
    }

    #[test]
    fn the_nautilus_has_a_shell_and_a_separate_mouth() {
        let m = model_def(MobModel::Nautilus);
        assert_eq!(m.parts.len(), 5, "shell, body, and three mouth pieces");
        // The shell is the widest part — nothing else should stick out past it.
        let shell = &m.parts[0];
        let widest = shell.cubes.iter().map(|c| c.size[0]).fold(0.0, f32::max);
        for part in &m.parts[1..] {
            for cube in &part.cubes {
                assert!(cube.size[0] <= widest, "a mouth piece wider than the shell");
            }
        }
    }

    /// The coral overlay is 8 flattened clumps (yellow×2, pink×2, blue×2,
    /// red×2) all sitting on the shell's own footprint — nothing should drift
    /// off past the shell the corals are supposed to be growing on.
    #[test]
    fn the_nautilus_corals_sit_on_the_shell() {
        let corals = model_def(MobModel::NautilusCorals);
        assert_eq!(corals.parts.len(), 8, "yellow/pink/blue/red, two clumps each");
        let shell = &model_def(MobModel::Nautilus).parts[0];
        let shell_half_x = shell.cubes.iter().map(|c| c.size[0] / 2.0).fold(0.0, f32::max);
        for part in &corals.parts {
            assert!(
                (part.pivot[0] - shell.pivot[0]).abs() <= shell_half_x + 8.0,
                "a coral clump pivot drifted past the shell it grows on"
            );
        }
    }

    /// A part's absolute vertical span: its pivot plus each cube's own local
    /// span, in this engine's Y-up-from-feet units.
    fn y_span(part: &Part) -> (f32, f32) {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for c in &part.cubes {
            lo = lo.min(part.pivot[1] + c.center[1] - c.size[1] / 2.0);
            hi = hi.max(part.pivot[1] + c.center[1] + c.size[1] / 2.0);
        }
        (lo, hi)
    }

    #[test]
    fn the_copper_golem_stands_with_legs_below_body_below_head() {
        let m = model_def(MobModel::CopperGolem);
        assert_eq!(m.parts.len(), 6, "body, head, two arms, two legs");
        let (legs_lo, legs_hi) = {
            let (l1, h1) = y_span(&m.parts[4]);
            let (l2, h2) = y_span(&m.parts[5]);
            (l1.min(l2), h1.max(h2))
        };
        let (body_lo, body_hi) = y_span(&m.parts[0]);
        // The head's main block, not its chin nub — vanilla hangs that nub a
        // little below the head block itself, over the top of the body, the
        // way a chin naturally would.
        let head_block = m.parts[1].cubes[0];
        let head_lo = head_block.center[1] - head_block.size[1] / 2.0 + m.parts[1].pivot[1];
        let (_, head_hi) = y_span(&m.parts[1]);
        assert!(legs_lo.abs() < 1e-6, "the feet should rest exactly on y=0, got {legs_lo}");
        assert!(legs_hi <= body_lo + 1e-6, "legs should end at or below where the body starts");
        assert!(body_hi <= head_lo + 1e-6, "the body should end at or below where the head block starts");
        assert!(head_hi > body_hi, "the head should reach higher than the body");
    }

    /// `SNIFFER_HAPPY` loops every real 2s; well past the clip's own length
    /// it should sample as if it had just wrapped, not hold the last frame
    /// (unlike Sniffer's other, non-looping clips).
    #[test]
    fn sniffer_happy_head_bob_loops_every_two_seconds() {
        let at_start = pose_part(MobPose::SnifferHappy { elapsed_secs: 0.0 }, PartRole::Head, 0.0, 0.0, 1.0)
            .expect("head pose");
        let one_loop_later =
            pose_part(MobPose::SnifferHappy { elapsed_secs: 2.0 }, PartRole::Head, 0.0, 0.0, 1.0)
                .expect("head pose");
        assert!((at_start.x_rot - one_loop_later.x_rot).abs() < 1e-5, "should be back where it started");
        let mid_loop = pose_part(MobPose::SnifferHappy { elapsed_secs: 0.5 }, PartRole::Head, 0.0, 0.0, 1.0)
            .expect("head pose");
        assert!(mid_loop.x_rot != at_start.x_rot, "should actually move partway through");
    }

    /// `SNIFFER_LONGSNIFF`/`SNIFFER_DIG`/`SNIFFER_STAND_UP` don't loop — real
    /// vanilla just holds the final keyframe once the state's own duration
    /// elapses (until the server transitions to a different real state).
    #[test]
    fn sniffer_non_looping_clips_hold_their_last_frame() {
        let at_end = pose_part(MobPose::SnifferSniffing { elapsed_secs: 1.0 }, PartRole::Head, 0.0, 0.0, 1.0)
            .expect("head pose");
        let long_after =
            pose_part(MobPose::SnifferSniffing { elapsed_secs: 50.0 }, PartRole::Head, 0.0, 0.0, 1.0)
                .expect("head pose");
        assert_eq!(at_end.x_rot, long_after.x_rot, "should hold, not wrap or extrapolate");
        assert_eq!(at_end.x_rot, 0.0, "SNIFFER_LONGSNIFF's own last keyframe returns the head to rest");
    }

    /// Digging/rising fold in the body's own baked lay-flat rotation
    /// (`FRAC_PI_2`) the same way Camel's dash does — losing it would stand
    /// the sniffer's body up on end mid-animation.
    #[test]
    fn sniffer_digging_and_rising_keep_the_bodys_lay_flat_baseline() {
        for pose in [
            MobPose::SnifferDigging { elapsed_secs: 0.0 },
            MobPose::SnifferRising { elapsed_secs: 0.0 },
        ] {
            let body = pose_part(pose, PartRole::Body, 0.0, 0.0, 1.0).expect("body pose");
            assert!((body.x_rot - FRAC_PI_2).abs() < 1e-5, "{pose:?} should keep the lay-flat baseline");
        }
    }

    /// Sniffer's front/hind legs are real vanilla data with their own real
    /// per-side timing — mirrored left/right (opposite rotation sign, same
    /// magnitude and timing), not a shared formula.
    #[test]
    fn sniffer_rising_legs_mirror_left_and_right() {
        let left = pose_part(MobPose::SnifferRising { elapsed_secs: 0.2 }, PartRole::FrontLeg, 0.0, 0.0, 1.0)
            .expect("front leg pose");
        let right =
            pose_part(MobPose::SnifferRising { elapsed_secs: 0.2 }, PartRole::FrontLeg, 0.0, 0.0, -1.0)
                .expect("front leg pose");
        assert!((left.z_rot + right.z_rot).abs() < 1e-6, "left/right should mirror in sign");
        assert!(left.z_rot.abs() > 0.0, "should actually be rotating partway through the rise");
    }

    /// Every leg starts the Rising clip already lifted (real vanilla's own
    /// track for each leg has no keyframe before its own onset, so it holds
    /// that first raised value) and lowers back to standing in a staggered
    /// wave — front legs settle first (their track starts and ends first),
    /// hind legs last. At a time both tracks are actively interpolating,
    /// front (which started lowering at t=0.0) should already be further
    /// toward neutral than hind (which only starts lowering at t=0.1667).
    #[test]
    fn sniffer_rising_front_and_hind_legs_are_staggered_not_shared() {
        let front = pose_part(MobPose::SnifferRising { elapsed_secs: 0.3 }, PartRole::FrontLeg, 0.0, 0.0, 1.0)
            .expect("front leg pose");
        let hind = pose_part(MobPose::SnifferRising { elapsed_secs: 0.3 }, PartRole::BackLeg, 0.0, 0.0, 1.0)
            .expect("hind leg pose");
        assert!(front.z_rot.abs() > 0.0 && hind.z_rot.abs() > 0.0, "both mid-lower at t=0.3s");
        assert!(
            front.z_rot.abs() < hind.z_rot.abs(),
            "front started lowering earlier, so should be further along (closer to neutral) by t=0.3s"
        );
    }
}
