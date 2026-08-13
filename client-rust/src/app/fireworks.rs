//! Firework stars: what a rocket actually paints on the sky when it goes off.
//!
//! A rocket carries a list of explosions in its `fireworks` item component —
//! each one a shape, up to eight dyed colours, optional fade colours, a trail
//! and a twinkle. Vanilla turns each explosion into a burst of particles whose
//! *directions* are fixed by the shape: a hollow ball for the two ball shapes,
//! and, for a star or a creeper face, an outline traced through a list of
//! coordinates that has been in the game since fireworks were added.
//!
//! This module is just that geometry and the colour table — no rendering, no
//! ECS — so it can be checked on its own. The app turns each direction into a
//! particle with the explosion's colours.

/// The five shapes a firework star can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    SmallBall,
    LargeBall,
    Star,
    Creeper,
    Burst,
}

/// One explosion off a rocket, decoded from the item component.
#[derive(Clone, Debug, PartialEq)]
pub struct Star {
    pub shape: Shape,
    /// Dye colours the sparks start as (packed RGB from the component).
    pub colors: Vec<[f32; 3]>,
    /// Colours they fade to, if the star was made with a fade dye.
    pub fade: Vec<[f32; 3]>,
    pub trail: bool,
    pub twinkle: bool,
}

impl Default for Star {
    fn default() -> Self {
        Self {
            shape: Shape::SmallBall,
            colors: vec![[1.0, 1.0, 1.0]],
            fade: Vec::new(),
            trail: false,
            twinkle: false,
        }
    }
}

/// Unpack a `0xRRGGBB` firework colour into linear 0..1 components.
pub fn rgb(packed: i32) -> [f32; 3] {
    let c = packed as u32;
    [
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
    ]
}

/// Vanilla's star outline (`STAR_PARTICLE_COORDS`) — a five-pointed star.
const STAR_COORDS: [[f32; 2]; 10] = [
    [0.0, 1.0],
    [0.3455, 0.309],
    [0.9511, 0.309],
    [0.3795, -0.1181],
    [0.5878, -0.809],
    [0.0, -0.3236],
    [-0.5878, -0.809],
    [-0.3795, -0.1181],
    [-0.9511, 0.309],
    [-0.3455, 0.309],
];

/// Vanilla's creeper face (`CREEPER_PARTICLE_COORDS`).
const CREEPER_COORDS: [[f32; 2]; 12] = [
    [0.0, 0.2],
    [0.2, 0.2],
    [0.2, 0.6],
    [0.6, 0.6],
    [0.6, 0.2],
    [0.2, 0.2],
    [0.2, 0.0],
    [0.4, 0.0],
    [0.4, -0.6],
    [0.2, -0.6],
    [0.2, -0.4],
    [0.0, -0.4],
];

/// How fast a shape's sparks leave the middle (vanilla's `speed` argument).
fn speed_of(shape: Shape) -> f32 {
    match shape {
        Shape::SmallBall => 0.25,
        Shape::LargeBall => 0.5,
        Shape::Star | Shape::Creeper => 0.5,
        Shape::Burst => 0.25,
    }
}

/// The hollow ball vanilla builds for the two ball shapes: every point on the
/// shell of a (2·size+1)³ lattice, normalised outwards. The inside is skipped,
/// which is what makes the burst read as a sphere rather than a blob.
fn ball(size: i32, speed: f32, jitter: &mut impl FnMut() -> f32, out: &mut Vec<[f32; 3]>) {
    for i in -size..=size {
        for j in -size..=size {
            for k in -size..=size {
                if i.abs() != size && j.abs() != size && k.abs() != size {
                    continue; // interior point — vanilla only uses the shell
                }
                let dx = j as f32 + (jitter() - jitter()) * 0.5;
                let dy = i as f32 + (jitter() - jitter()) * 0.5;
                let dz = k as f32 + (jitter() - jitter()) * 0.5;
                let len = (dx * dx + dy * dy + dz * dz).sqrt() / speed + jitter() * 0.05;
                if len <= 0.0001 {
                    continue;
                }
                out.push([dx / len, dy / len, dz / len]);
            }
        }
    }
}

/// Trace a 2D outline, in a plane turned by `yaw`, with enough points between
/// the corners that it reads as a continuous shape rather than a dozen dots.
///
/// `mirror` doubles every point across the vertical axis. Vanilla's creeper
/// face is stored as one half only and mirrored on the way out — without that
/// it is a lopsided squiggle rather than a face.
fn outline(coords: &[[f32; 2]], speed: f32, yaw: f32, mirror: bool, out: &mut Vec<[f32; 3]>) {
    const STEPS: usize = 8; // points along each edge
    let (sy, cy) = yaw.sin_cos();
    for n in 0..coords.len() {
        let a = coords[n];
        let b = coords[(n + 1) % coords.len()];
        for s in 0..STEPS {
            let t = s as f32 / STEPS as f32;
            let x = (a[0] + (b[0] - a[0]) * t) * speed;
            let y = (a[1] + (b[1] - a[1]) * t) * speed;
            // The shape stands upright, spun around the vertical axis so two
            // rockets never show the same star from the same angle.
            out.push([x * cy, y, x * sy]);
            if mirror {
                out.push([-x * cy, y, -x * sy]);
            }
        }
    }
}

/// Every spark direction one explosion throws out. `rand01` supplies the
/// jitter (and the shape's random spin) so the caller keeps its own RNG.
pub fn directions(star: &Star, rand01: &mut impl FnMut() -> f32) -> Vec<[f32; 3]> {
    let speed = speed_of(star.shape);
    let mut out = Vec::new();
    match star.shape {
        Shape::SmallBall => ball(1, speed, rand01, &mut out),
        Shape::LargeBall => ball(2, speed, rand01, &mut out),
        Shape::Star => {
            outline(&STAR_COORDS, speed, rand01() * std::f32::consts::TAU, false, &mut out)
        }
        Shape::Creeper => {
            outline(&CREEPER_COORDS, speed, rand01() * std::f32::consts::TAU, true, &mut out)
        }
        Shape::Burst => {
            // Vanilla's burst is a spray with no shape to it at all.
            for _ in 0..70 {
                let dx = rand01() * 2.0 - 1.0;
                let dy = rand01() * 2.0 - 1.0;
                let dz = rand01() * 2.0 - 1.0;
                let len = (dx * dx + dy * dy + dz * dz).sqrt();
                if len > 0.0001 {
                    let s = speed * (0.4 + rand01() * 0.6) / len;
                    out.push([dx * s, dy * s, dz * s]);
                }
            }
        }
    }
    out
}

/// The colour a spark shows at age `t` (0..1): its dye colour, crossing into
/// the fade colour over the second half of its life, exactly like vanilla's
/// `fadeToColor`.
pub fn spark_color(star: &Star, index: usize, t: f32) -> [f32; 3] {
    let base = if star.colors.is_empty() {
        [1.0, 1.0, 1.0]
    } else {
        star.colors[index % star.colors.len()]
    };
    if star.fade.is_empty() || t < 0.5 {
        return base;
    }
    let to = star.fade[index % star.fade.len()];
    let k = ((t - 0.5) * 2.0).clamp(0.0, 1.0);
    [
        base[0] + (to[0] - base[0]) * k,
        base[1] + (to[1] - base[1]) * k,
        base[2] + (to[2] - base[2]) * k,
    ]
}

/// The sound a star makes, and how long the bang takes to arrive.
///
/// Vanilla plays the blast at the rocket and lets the client's own sound engine
/// handle distance, but it does pick a *different, louder* sample for a big
/// star, and adds the twinkle on top. The delay is real physics: sound covers
/// about 340 m/s, so a rocket 200 blocks away is heard a good half second after
/// it is seen — which is exactly why fireworks look wrong without it.
pub fn boom(star: &Star, distance: f64) -> (&'static str, Option<&'static str>, f32) {
    let big = matches!(star.shape, Shape::LargeBall | Shape::Star | Shape::Creeper);
    let blast = if big {
        "entity.firework_rocket.large_blast"
    } else {
        "entity.firework_rocket.blast"
    };
    let twinkle = star.twinkle.then_some("entity.firework_rocket.twinkle");
    (blast, twinkle, (distance / 340.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic stand-in for the app's RNG.
    fn seq() -> impl FnMut() -> f32 {
        let mut n = 0u32;
        move || {
            n = n.wrapping_mul(1664525).wrapping_add(1013904223);
            (n >> 8) as f32 / (1 << 24) as f32
        }
    }

    #[test]
    fn colours_unpack_like_the_component() {
        assert_eq!(rgb(0xFF0000), [1.0, 0.0, 0.0]);
        assert_eq!(rgb(0x000000), [0.0, 0.0, 0.0]);
        assert_eq!(rgb(0xFFFFFF), [1.0, 1.0, 1.0]);
        let grey = rgb(0x808080);
        assert!((grey[0] - 0.502).abs() < 0.01 && grey[0] == grey[1] && grey[1] == grey[2]);
    }

    #[test]
    fn a_ball_is_hollow() {
        // 3³ shell = 27 lattice points minus the 1 interior one.
        let mut r = seq();
        let small = directions(&Star { shape: Shape::SmallBall, ..Default::default() }, &mut r);
        assert_eq!(small.len(), 26);
        // 5³ shell = 125 − 27 interior.
        let large = directions(&Star { shape: Shape::LargeBall, ..Default::default() }, &mut r);
        assert_eq!(large.len(), 98);
    }

    #[test]
    fn a_large_ball_throws_its_sparks_further() {
        fn reach(shape: Shape) -> f32 {
            let mut r = seq();
            directions(&Star { shape, ..Default::default() }, &mut r)
                .iter()
                .map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())
                .fold(0.0f32, f32::max)
        }
        let small = reach(Shape::SmallBall);
        let large = reach(Shape::LargeBall);
        assert!(large > small, "large {large} should out-reach small {small}");
    }

    #[test]
    fn the_shaped_stars_trace_their_outline() {
        let mut r = seq();
        let star = directions(&Star { shape: Shape::Star, ..Default::default() }, &mut r);
        assert_eq!(star.len(), STAR_COORDS.len() * 8);
        // The creeper's coordinates are half a face; vanilla mirrors them, so
        // every point comes out twice.
        let creeper = directions(&Star { shape: Shape::Creeper, ..Default::default() }, &mut r);
        assert_eq!(creeper.len(), CREEPER_COORDS.len() * 8 * 2);
        let (left, right) = creeper.iter().fold((0, 0), |(l, r), d| {
            if d[0] < -0.001 { (l + 1, r) } else if d[0] > 0.001 { (l, r + 1) } else { (l, r) }
        });
        assert!(left > 0 && right > 0, "a face has two sides ({left}/{right})");
        // A star stands upright: it has real height, and no depth of its own
        // beyond the spin it was given.
        let tall = star.iter().map(|v| v[1].abs()).fold(0.0f32, f32::max);
        assert!(tall > 0.3, "the star should stand up, not lie flat ({tall})");
    }

    #[test]
    fn sparks_fade_into_their_second_colour() {
        let s = Star {
            colors: vec![[1.0, 0.0, 0.0]],
            fade: vec![[0.0, 0.0, 1.0]],
            ..Default::default()
        };
        assert_eq!(spark_color(&s, 0, 0.0), [1.0, 0.0, 0.0], "starts on the dye colour");
        assert_eq!(spark_color(&s, 0, 0.49), [1.0, 0.0, 0.0], "no fade in the first half");
        let mid = spark_color(&s, 0, 0.75);
        assert!(mid[0] > 0.4 && mid[0] < 0.6 && mid[2] > 0.4 && mid[2] < 0.6, "{mid:?}");
        assert_eq!(spark_color(&s, 0, 1.0), [0.0, 0.0, 1.0], "ends on the fade colour");
    }

    #[test]
    fn with_no_fade_a_spark_keeps_its_colour() {
        let s = Star { colors: vec![[0.2, 0.4, 0.6]], ..Default::default() };
        assert_eq!(spark_color(&s, 0, 1.0), [0.2, 0.4, 0.6]);
    }

    #[test]
    fn colours_cycle_through_the_dyes() {
        let s = Star {
            colors: vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            ..Default::default()
        };
        assert_eq!(spark_color(&s, 0, 0.0), [1.0, 0.0, 0.0]);
        assert_eq!(spark_color(&s, 1, 0.0), [0.0, 1.0, 0.0]);
        assert_eq!(spark_color(&s, 2, 0.0), [1.0, 0.0, 0.0], "wraps round");
    }

    #[test]
    fn a_big_star_bangs_louder_and_the_bang_takes_time_to_arrive() {
        let small = Star { shape: Shape::SmallBall, ..Default::default() };
        let big = Star { shape: Shape::LargeBall, twinkle: true, ..Default::default() };
        assert_eq!(boom(&small, 0.0).0, "entity.firework_rocket.blast");
        assert_eq!(boom(&big, 0.0).0, "entity.firework_rocket.large_blast");
        assert_eq!(boom(&small, 0.0).1, None, "no twinkle unless the star has one");
        assert_eq!(boom(&big, 0.0).1, Some("entity.firework_rocket.twinkle"));
        // Overhead: heard at once. Far across the map: noticeably late.
        assert!(boom(&big, 5.0).2 < 0.05);
        assert!(boom(&big, 200.0).2 > 0.4 && boom(&big, 200.0).2 < 0.8);
    }
}
