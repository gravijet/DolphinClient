//! What an entity-status byte looks and sounds like.
//!
//! `ClientboundEntityEvent` is how the server tells the client about the small
//! moments it renders itself: a wolf shaking off water, a villager's angry
//! cloud, hearts over two animals in love, a totem of undying saving somebody.
//! The packet carries one entity and one number, and every client turns that
//! number into particles and a sound. This is that table.
//!
//! Statuses the client already handles elsewhere (2 hurt, 3 death) are not
//! listed here — the bridge sends those on as their own events.

use crate::bridge::events::ParticleTex;

/// The particles and sound one status is worth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatusFx {
    /// Sprite family, colour, how many, and how far they scatter.
    pub particles: Option<(ParticleTex, [f32; 3], u32, f32)>,
    /// Sound event name, without the namespace.
    pub sound: Option<&'static str>,
    /// Vanilla's full-screen totem flash (status 35 only).
    pub totem: bool,
    /// Particles rise from the whole body rather than sitting at its middle
    /// (hearts, angry clouds and happy sparkles float above the head).
    pub above: bool,
}

const fn fx(particles: Option<(ParticleTex, [f32; 3], u32, f32)>, sound: Option<&'static str>) -> StatusFx {
    StatusFx { particles, sound, totem: false, above: false }
}

/// Colourless helper for the plain white/grey clouds.
const SMOKE: ParticleTex = ParticleTex::Smoke;
const HEART: ParticleTex = ParticleTex::Heart;
const ANGRY: ParticleTex = ParticleTex::Angry;
const HAPPY: ParticleTex = ParticleTex::Happy;
const EFFECT: ParticleTex = ParticleTex::Effect;
const CRIT: ParticleTex = ParticleTex::Crit;
const SPLASH: ParticleTex = ParticleTex::Splash;

/// Vanilla's `handleEntityEvent`, as far as it is visible or audible.
pub fn status_fx(status: u8) -> Option<StatusFx> {
    let white = [1.0, 1.0, 1.0];
    Some(match status {
        // Taming: a puff of smoke for failure, hearts for success. Cats use
        // their own pair (40 / 41) and horses another (6 / 7 as well).
        6 | 40 => StatusFx { above: true, ..fx(Some((SMOKE, [0.4, 0.4, 0.4], 7, 0.6)), None) },
        7 | 41 => StatusFx { above: true, ..fx(Some((HEART, white, 7, 0.6)), None) },
        // A wolf shaking the water out of its coat.
        8 => fx(Some((SPLASH, white, 8, 0.5)), Some("entity.wolf.shake")),
        // Sheep pulling up a mouthful of grass.
        10 => fx(Some((SMOKE, [0.35, 0.55, 0.25], 5, 0.35)), Some("entity.sheep.eat")),
        // An iron golem holding out a poppy, and putting it away again.
        11 => fx(Some((HAPPY, [0.9, 0.3, 0.3], 4, 0.4)), None),
        34 => fx(None, None),
        // Villagers: in love, angry, pleased, sweating.
        12 | 18 => StatusFx { above: true, ..fx(Some((HEART, white, 7, 0.6)), None) },
        13 => StatusFx { above: true, ..fx(Some((ANGRY, white, 6, 0.4)), None) },
        14 => StatusFx { above: true, ..fx(Some((HAPPY, white, 8, 0.5)), None) },
        44 => StatusFx { above: true, ..fx(Some((SPLASH, [0.3, 0.5, 0.9], 5, 0.3)), None) },
        // A witch brewing up her purple cloud.
        15 => fx(Some((EFFECT, [0.5, 0.2, 0.6], 12, 0.5)), None),
        // A zombie villager finishing its cure.
        16 => fx(None, Some("entity.zombie_villager.converted")),
        // A firework going off, and a mob spawning in a cloud.
        17 => fx(Some((ParticleTex::Explosion, white, 8, 0.9)), Some("entity.firework_rocket.blast")),
        20 => fx(Some((ParticleTex::Explosion, white, 12, 0.8)), None),
        // A guardian's beam, a dolphin's delight, a ravager stunned.
        21 => fx(None, Some("entity.guardian.attack")),
        38 => StatusFx { above: true, ..fx(Some((HAPPY, white, 7, 0.5)), None) },
        39 => fx(None, Some("entity.ravager.stunned")),
        // Shields: one blocked hit, one shield broken.
        29 => fx(None, Some("item.shield.block")),
        30 => fx(Some((CRIT, [0.6, 0.5, 0.4], 8, 0.4)), Some("item.shield.break")),
        // Thorns, drowning, burning — vanilla's three "hurt by X" statuses.
        33 => fx(Some((CRIT, [0.8, 0.2, 0.2], 5, 0.3)), Some("enchant.thorns.hit")),
        36 => fx(Some((ParticleTex::Bubble, white, 8, 0.4)), Some("entity.generic.hurt")),
        37 => fx(Some((ParticleTex::Flame, white, 6, 0.4)), Some("entity.generic.burn")),
        // A totem of undying spending itself to keep somebody alive.
        35 => StatusFx {
            particles: Some((ParticleTex::Glow, [0.9, 0.8, 0.25], 24, 1.0)),
            sound: Some("item.totem.use"),
            totem: true,
            above: false,
        },
        // Something teleported away (enderman, chorus fruit).
        46 => fx(Some((ParticleTex::Portal, [0.6, 0.3, 0.8], 16, 0.8)), None),
        // A piece of equipment broke — vanilla's little burst of item pieces.
        47..=52 => fx(Some((ParticleTex::Generic, [0.7, 0.7, 0.7], 8, 0.35)), Some("entity.item.break")),
        // Honey: sliding down a block, and landing in it.
        53 => fx(Some((ParticleTex::Generic, [0.95, 0.7, 0.15], 5, 0.3)), None),
        54 => fx(Some((ParticleTex::Generic, [0.95, 0.7, 0.15], 8, 0.35)), Some("block.honey_block.slide")),
        // A sniffer's nose in the dirt.
        60 => fx(Some((ParticleTex::Generic, [0.45, 0.32, 0.22], 8, 0.4)), None),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_totem_flashes_the_screen() {
        for s in 0..=80u8 {
            let is_totem = status_fx(s).is_some_and(|f| f.totem);
            assert_eq!(is_totem, s == 35, "status {s}");
        }
    }

    #[test]
    fn taming_smoke_and_hearts_are_the_two_answers() {
        let fail = status_fx(6).unwrap();
        let win = status_fx(7).unwrap();
        assert_eq!(fail.particles.unwrap().0, ParticleTex::Smoke);
        assert_eq!(win.particles.unwrap().0, ParticleTex::Heart);
        assert!(win.above, "hearts float above the animal");
        // A cat is tamed with its own pair of statuses, same two answers.
        assert_eq!(status_fx(40).unwrap().particles.unwrap().0, ParticleTex::Smoke);
        assert_eq!(status_fx(41).unwrap().particles.unwrap().0, ParticleTex::Heart);
    }

    #[test]
    fn unknown_statuses_do_nothing_at_all() {
        for s in [0u8, 1, 2, 3, 5, 22, 24, 70, 200] {
            assert!(status_fx(s).is_none(), "status {s} should be ignored");
        }
    }

    #[test]
    fn every_effect_does_something() {
        for s in 0..=255u8 {
            if let Some(f) = status_fx(s) {
                assert!(
                    f.particles.is_some() || f.sound.is_some() || f.totem || s == 34,
                    "status {s} is listed but has no effect",
                );
            }
        }
    }
}
