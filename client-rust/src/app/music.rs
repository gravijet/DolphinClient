//! Music and the quiet noises a world makes.
//!
//! Two things the client owns entirely, and which a Minecraft that has neither
//! feels wrong without:
//!
//! * **Music.** Vanilla plays one track, then waits a long, random silence
//!   before the next — ten to twenty minutes in a world, seconds on the title
//!   screen. Which track depends on where you are.
//! * **Cave ambience.** Vanilla keeps a "mood" that creeps up whenever the
//!   blocks around you are pitch dark and drops back when they are not. When it
//!   fills, you get one of those cave noises from somewhere nearby. It is the
//!   reason caves are frightening.
//!
//! Both are pure state machines here so they can be tested without a speaker.

/// Where the player is, as far as the music is concerned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MusicScene {
    /// Not in a world: the title screen.
    Menu,
    Overworld,
    Nether,
    End,
    /// Under water, which vanilla scores separately.
    Underwater,
}

impl MusicScene {
    /// The sound events to try, best first. The caller plays the first one the
    /// asset store actually has, so a pack without the newer biome tracks still
    /// gets music.
    pub fn candidates(self) -> &'static [&'static str] {
        match self {
            MusicScene::Menu => &["music.menu"],
            MusicScene::Overworld => &["music.overworld.forest", "music.game"],
            MusicScene::Nether => &["music.nether.nether_wastes", "music.nether", "music.game"],
            MusicScene::End => &["music.end", "music.game"],
            MusicScene::Underwater => &["music.under_water", "music.game"],
        }
    }

    /// Vanilla's silence between tracks, in seconds `(min, max)`. The menu
    /// loops almost immediately; a world waits a very long time.
    pub fn gap_secs(self) -> (f32, f32) {
        match self {
            // 20 ticks .. 600 ticks.
            MusicScene::Menu => (1.0, 30.0),
            // 12000 .. 24000 ticks — ten to twenty minutes, like vanilla.
            _ => (600.0, 1200.0),
        }
    }
}

/// Decides when the next track starts and which one it is.
pub struct MusicDirector {
    /// Seconds left before the next track may start.
    wait: f32,
    /// What was playing when the wait was set, so a change of scene can cut in.
    scene: Option<MusicScene>,
    /// Seconds the current track has been playing (nothing else may start).
    playing: f32,
    rng: u64,
}

impl Default for MusicDirector {
    fn default() -> Self {
        Self { wait: 0.0, scene: None, playing: 0.0, rng: 0x9E37_79B9_7F4A_7C15 }
    }
}

impl MusicDirector {
    fn next_f32(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        ((self.rng >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Nothing should play for a while — used when the player leaves a world.
    pub fn reset(&mut self) {
        self.wait = 0.0;
        self.scene = None;
        self.playing = 0.0;
    }

    /// Advance by `dt` seconds. Returns the scene whose track should start now,
    /// or `None` to stay quiet.
    pub fn tick(&mut self, dt: f32, scene: MusicScene) -> Option<MusicScene> {
        // Changing world (walking into the Nether) restarts the wait rather
        // than cutting the current track off mid-bar.
        if self.scene != Some(scene) {
            self.scene = Some(scene);
            // Vanilla starts the first track of a new scene fairly soon.
            let (lo, _) = scene.gap_secs();
            self.wait = lo.min(20.0) + self.next_f32() * 10.0;
            self.playing = 0.0;
            return None;
        }
        if self.playing > 0.0 {
            self.playing = (self.playing - dt).max(0.0);
            return None;
        }
        self.wait -= dt;
        if self.wait > 0.0 {
            return None;
        }
        let (lo, hi) = scene.gap_secs();
        self.wait = lo + self.next_f32() * (hi - lo);
        // Vanilla will not start a second track over the first; a generous
        // estimate of track length keeps them from overlapping.
        self.playing = 180.0;
        Some(scene)
    }

}

/// Vanilla's cave-mood accumulator.
pub struct MoodMeter {
    /// 0..1; at 1 a cave sound plays and it resets.
    mood: f32,
}

impl Default for MoodMeter {
    fn default() -> Self {
        Self { mood: 0.0 }
    }
}

impl MoodMeter {
    #[cfg(test)]
    pub fn value(&self) -> f32 {
        self.mood
    }

    pub fn reset(&mut self) {
        self.mood = 0.0;
    }

    /// One tick of vanilla's mood algorithm: it samples a random block nearby
    /// and, if that block is in total darkness *and* the sky cannot see it, the
    /// mood creeps up. Anything else lets it fall back.
    ///
    /// `dark` is whether the sampled block was pitch black; `sky_lit` whether
    /// the sky reaches it. Returns true when a cave sound should play.
    pub fn tick(&mut self, dark: bool, sky_lit: bool) -> bool {
        if dark && !sky_lit {
            // Vanilla's rate: about a minute of standing in the dark.
            self.mood += 0.001;
        } else {
            self.mood -= 0.001;
        }
        self.mood = self.mood.clamp(0.0, 1.0);
        if self.mood >= 1.0 {
            self.mood = 0.0;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_loops_quickly_and_a_world_does_not() {
        assert!(MusicScene::Menu.gap_secs().1 < 60.0);
        assert!(MusicScene::Overworld.gap_secs().0 >= 600.0);
    }

    #[test]
    fn every_scene_has_a_fallback_track() {
        for scene in [
            MusicScene::Menu,
            MusicScene::Overworld,
            MusicScene::Nether,
            MusicScene::End,
            MusicScene::Underwater,
        ] {
            assert!(!scene.candidates().is_empty());
        }
        // Everything in a world falls back to the generic track.
        assert!(MusicScene::Nether.candidates().contains(&"music.game"));
    }

    #[test]
    fn nothing_plays_the_instant_you_join() {
        let mut d = MusicDirector::default();
        assert_eq!(d.tick(0.05, MusicScene::Overworld), None);
        // …but something does once the opening wait is over.
        let mut played = 0;
        for _ in 0..2000 {
            if d.tick(0.05, MusicScene::Overworld).is_some() {
                played += 1;
            }
        }
        assert_eq!(played, 1, "exactly one track in the first 100 seconds");
    }

    #[test]
    fn a_second_track_waits_out_the_first_and_the_long_silence() {
        let mut d = MusicDirector::default();
        // Run out the opening wait and the first track.
        let mut plays = 0;
        for _ in 0..(20 * 60 * 12) {
            if d.tick(0.05, MusicScene::Overworld).is_some() {
                plays += 1;
            }
        }
        // Twelve minutes covers the first track and, at most, one more.
        assert!((1..=2).contains(&plays), "got {plays} tracks in twelve minutes");
    }

    #[test]
    fn walking_into_the_nether_restarts_the_wait() {
        let mut d = MusicDirector::default();
        d.tick(0.05, MusicScene::Overworld);
        assert_eq!(d.tick(0.05, MusicScene::Nether), None);
        assert_eq!(d.scene, Some(MusicScene::Nether));
    }

    #[test]
    fn the_mood_only_climbs_in_the_pitch_dark() {
        let mut m = MoodMeter::default();
        for _ in 0..100 {
            assert!(!m.tick(true, false));
        }
        assert!(m.value() > 0.0);
        let peak = m.value();
        for _ in 0..50 {
            m.tick(false, false);
        }
        assert!(m.value() < peak, "daylight should drain the mood");
    }

    #[test]
    fn a_full_mood_makes_a_sound_and_starts_over() {
        let mut m = MoodMeter::default();
        // About a thousand dark ticks — vanilla's "a minute in the dark".
        let first = (0..2000).find(|_| m.tick(true, false)).expect("a cave sound eventually");
        assert!((950..=1050).contains(&first), "took {first} ticks");
        assert_eq!(m.value(), 0.0, "the meter starts over afterwards");
        // …and it keeps happening, rather than firing once and stopping.
        let second = (0..2000).find(|_| m.tick(true, false));
        assert!(second.is_some());
    }

    #[test]
    fn a_lit_sky_never_builds_a_mood() {
        let mut m = MoodMeter::default();
        for _ in 0..5000 {
            assert!(!m.tick(true, true), "sky-lit blocks are not cave");
        }
        assert_eq!(m.value(), 0.0);
    }
}
