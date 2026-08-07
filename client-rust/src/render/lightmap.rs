//! Vanilla's light texture: the 16×16 lookup that turns a `(block, sky)` light
//! pair into the colour the world is actually lit with.
//!
//! Minecraft does not shade the world with the raw 0..15 light level. It builds
//! a small texture every frame — `LightTexture` — and every vertex samples it
//! with its two light levels. That texture is where the game's look comes from:
//!
//! - the **brightness curve** `f / (4 - 3f)`, which is why light falls off fast
//!   near a torch and slowly further out,
//! - **warm block light**: a torch is orange, not grey, because the green and
//!   blue channels run through two extra polynomials,
//! - **cool sky light**: at night the sky ramp keeps its blue while red and
//!   green drop away,
//! - the dimension's **ambient light** floor (0 in the Overworld, 0.1 in the
//!   Nether, so nothing there is ever truly black),
//! - the **Brightness slider**, which is a gamma curve on the finished colour,
//!   not a multiplier.
//!
//! Building the same texture — rather than approximating it in the shader —
//! means terrain, entities and block entities are all lit by one table, exactly
//! like the real game.

/// Side length of the light texture: 16 block-light levels × 16 sky-light.
pub const SIZE: usize = 16;

/// One frame's lighting inputs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightmapParams {
    /// Sky-light strength 0..1 (vanilla's `skyDarken`): 1 at noon, ~0.2 at
    /// midnight, dimmed further by rain.
    pub daylight: f32,
    /// The dimension's `ambient_light`: 0 in the Overworld, 0.1 in the Nether.
    /// It is the floor the brightness curve is lifted toward, so a Nether cave
    /// is gloomy but never pitch black.
    pub ambient: f32,
    /// Block-light flicker 0..1, re-rolled every tick — the reason torchlight
    /// never sits perfectly still.
    pub flicker: f32,
    /// Night-vision strength 0..1; raises the floor of the whole ramp.
    pub night_vision: f32,
    /// The Brightness ("gamma") slider, 0 = Moody .. 1 = Bright.
    pub gamma: f32,
    /// The End lights everything with a pale green-grey instead of a sky ramp.
    pub end: bool,
    /// Lightning flash 0..1 — the sky briefly counts as full daylight.
    pub flash: f32,
    /// Darkness (warden) 0..1: pulls the finished ramp back down.
    pub darkness: f32,
}

impl Default for LightmapParams {
    fn default() -> Self {
        Self {
            daylight: 1.0,
            ambient: 0.0,
            flicker: 0.0,
            night_vision: 0.0,
            gamma: 0.5,
            end: false,
            flash: 0.0,
            darkness: 0.0,
        }
    }
}

/// Vanilla's `getBrightness`: the 0..15 level scaled to 0..1, bent by
/// `f / (4 - 3f)` and then lifted toward 1 by the dimension's ambient light.
fn brightness(level: f32, ambient: f32) -> f32 {
    let f = level.clamp(0.0, 1.0);
    let g = f / (4.0 - 3.0 * f);
    g + ambient * (1.0 - g)
}

/// Vanilla's `notGamma`: a strong ease-out that lifts the dark end of the ramp
/// without blowing out the bright end. The Brightness slider blends toward it.
fn not_gamma(x: f32) -> f32 {
    let inv = 1.0 - x;
    1.0 - inv * inv * inv * inv
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// The colour for one `(block, sky)` pair, linear 0..1.
pub fn sample(p: &LightmapParams, block_level: u32, sky_level: u32) -> [f32; 3] {
    // The sky never goes fully dark while a bolt is on screen, and even at
    // midnight vanilla keeps 5 % so the world stays readable.
    let sky_scale = (p.daylight * 0.95 + 0.05).max(p.flash);
    let sky = brightness(sky_level as f32 / 15.0, p.ambient) * sky_scale;
    // The flicker is a small wobble around 1, not a boost: the channel curves
    // below only give warm light while the value stays inside 0..1.
    let blk = brightness(block_level as f32 / 15.0, p.ambient) * (0.97 + p.flicker * 0.06);

    // Warm block light: red keeps the full value, green and blue are pulled
    // down by their own curves — this is the orange of torchlight.
    let mut c = [blk, blk * ((blk * 0.6 + 0.4) * 0.6 + 0.4), blk * (blk * blk * 0.6 + 0.4)];

    if p.end {
        // No sky in the End: vanilla tints the block ramp toward a pale
        // green-white and leaves it there.
        c = lerp3(c, [0.99, 1.12, 1.0], 0.25);
    } else {
        // Sky light holds its blue as it dims, which is what makes night blue
        // instead of grey.
        let tinted = lerp3([sky, sky, 1.0], [1.0, 1.0, 1.0], 0.35);
        for i in 0..3 {
            c[i] += tinted[i] * sky;
        }
        // A touch of flat grey so nothing reads as a pure primary.
        c = lerp3(c, [0.75, 0.75, 0.75], 0.04);
    }

    if p.darkness > 0.0 {
        let k = 1.0 - p.darkness.clamp(0.0, 1.0) * 0.9;
        for v in &mut c {
            *v *= k;
        }
    }
    for v in &mut c {
        *v = v.clamp(0.0, 1.0);
    }

    // The Brightness slider blends the finished colour toward its gamma curve.
    let g = [not_gamma(c[0]), not_gamma(c[1]), not_gamma(c[2])];
    c = lerp3(c, g, p.gamma.clamp(0.0, 1.0));
    c = lerp3(c, [0.75, 0.75, 0.75], 0.04);

    // Night vision lifts the dark end of the ramp without touching the bright
    // end — everything looks washed out but nothing is black.
    let floor = p.night_vision.clamp(0.0, 1.0) * 0.85;
    for v in &mut c {
        *v = v.max(floor).clamp(0.0, 1.0);
    }
    c
}

/// The whole 16×16 texture as RGBA8 rows, block light across, sky light down.
pub fn build(p: &LightmapParams) -> [u8; SIZE * SIZE * 4] {
    let mut out = [0u8; SIZE * SIZE * 4];
    for sky in 0..SIZE {
        for blk in 0..SIZE {
            let c = sample(p, blk as u32, sky as u32);
            let i = (sky * SIZE + blk) * 4;
            out[i] = (c[0] * 255.0 + 0.5) as u8;
            out[i + 1] = (c[1] * 255.0 + 0.5) as u8;
            out[i + 2] = (c[2] * 255.0 + 0.5) as u8;
            out[i + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: [f32; 3]) -> f32 {
        (c[0] + c[1] + c[2]) / 3.0
    }

    #[test]
    fn brightness_curve_matches_vanilla_endpoints() {
        assert!((brightness(0.0, 0.0)).abs() < 1e-6);
        assert!((brightness(1.0, 0.0) - 1.0).abs() < 1e-6);
        // Half light is much darker than half brightness: 0.5 / 2.5 = 0.2.
        assert!((brightness(0.5, 0.0) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn ambient_light_lifts_the_floor() {
        // The Nether's 0.1 ambient means level 0 is not black.
        assert!(brightness(0.0, 0.1) > 0.09);
        // ...but full light is still full light.
        assert!((brightness(1.0, 0.1) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn block_light_is_warm() {
        let p = LightmapParams { daylight: 0.0, gamma: 0.0, ..Default::default() };
        let c = sample(&p, 14, 0);
        assert!(c[0] > c[1], "red should lead green: {c:?}");
        assert!(c[1] > c[2], "green should lead blue: {c:?}");
    }

    #[test]
    fn sky_light_is_cool_at_night() {
        let night = LightmapParams { daylight: 0.2, gamma: 0.0, ..Default::default() };
        let c = sample(&night, 0, 15);
        assert!(c[2] > c[0], "night sky light should keep its blue: {c:?}");
    }

    #[test]
    fn more_light_is_never_darker() {
        let p = LightmapParams::default();
        for sky in 0..16 {
            for blk in 1..16 {
                let lo = lum(sample(&p, blk - 1, sky));
                let hi = lum(sample(&p, blk, sky));
                assert!(hi >= lo - 1e-6, "block {blk} at sky {sky}: {hi} < {lo}");
            }
        }
        for blk in 0..16 {
            for sky in 1..16 {
                let lo = lum(sample(&p, blk, sky - 1));
                let hi = lum(sample(&p, blk, sky));
                assert!(hi >= lo - 1e-6, "sky {sky} at block {blk}: {hi} < {lo}");
            }
        }
    }

    #[test]
    fn daylight_drives_the_sky_column() {
        let noon = LightmapParams { daylight: 1.0, ..Default::default() };
        let midnight = LightmapParams { daylight: 0.2, ..Default::default() };
        assert!(lum(sample(&noon, 0, 15)) > lum(sample(&midnight, 0, 15)));
        // Block light does not care what time it is.
        let a = sample(&noon, 15, 0);
        let b = sample(&midnight, 15, 0);
        assert!((lum(a) - lum(b)).abs() < 1e-6);
    }

    #[test]
    fn night_vision_lifts_the_dark_end() {
        let dark = LightmapParams { daylight: 0.0, ..Default::default() };
        let nv = LightmapParams { night_vision: 1.0, ..dark };
        assert!(lum(sample(&dark, 0, 0)) < 0.3);
        assert!(lum(sample(&nv, 0, 0)) > 0.8);
    }

    #[test]
    fn brightness_slider_only_brightens() {
        let moody = LightmapParams { daylight: 0.3, gamma: 0.0, ..Default::default() };
        let bright = LightmapParams { gamma: 1.0, ..moody };
        for sky in 0..16 {
            for blk in 0..16 {
                let a = lum(sample(&moody, blk, sky));
                let b = lum(sample(&bright, blk, sky));
                assert!(b >= a - 1e-6, "gamma darkened ({blk},{sky}): {b} < {a}");
            }
        }
    }

    #[test]
    fn texture_is_rgba_and_opaque() {
        let px = build(&LightmapParams::default());
        assert_eq!(px.len(), 16 * 16 * 4);
        assert!(px.chunks_exact(4).all(|p| p[3] == 255));
        // Fully lit corner is (near) white, unlit corner is dark.
        let bright = px[(15 * 16 + 15) * 4];
        let dark = px[0];
        assert!(bright > 200, "lit corner too dark: {bright}");
        assert!(dark < 90, "unlit corner too bright: {dark}");
    }
}
