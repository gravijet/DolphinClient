//! Audio engine: plays Minecraft's own OGG/Vorbis sounds via rodio.
//!
//! Sound *definitions* (`sounds.json`) and the OGG files both live in Mojang's
//! asset store, not in the client jar. Downloading every sound up front is
//! ~350 MB, so instead we:
//!   1. read the asset index + `sounds.json` (the launcher fetches these two
//!      tiny files and passes `--assets-dir` / `--asset-index`), and
//!   2. stream each OGG on demand the first time it is heard, caching it in the
//!      shared `assets/objects` store. The first play of a brand-new sound is
//!      silent while it downloads in the background; every play after is instant.
//!
//! Everything degrades gracefully: no audio device, missing assets or a failed
//! download just means (that) sound is skipped — the game keeps running.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use serde_json::Value;
use tracing::{info, warn};

/// Mojang's content-addressed asset host (same one the launcher uses).
const RESOURCES: &str = "https://resources.download.minecraft.net";

/// One playable variant of a sound event, resolved from `sounds.json`.
#[derive(Clone)]
struct SoundFile {
    /// Resource namespace (almost always `minecraft`).
    namespace: String,
    /// Path under `<namespace>/sounds/`, e.g. `mob/zombie/say1`.
    path: String,
    volume: f32,
    pitch: f32,
    /// Relative pick weight (`sounds.json`'s `weight` field, default 1) — a
    /// variant listed with `weight: 20` is 20x as likely to be chosen as one
    /// left at the default. Real vanilla (`WeighedSoundEvents.getSound`)
    /// picks proportionally to this, not uniformly.
    weight: u32,
}

pub struct AudioEngine {
    // Keep the stream alive for as long as we play — dropping it stops audio.
    _stream: OutputStream,
    handle: OutputStreamHandle,
    /// Event name (namespace stripped, e.g. `entity.zombie.ambient`) → variants.
    defs: HashMap<String, Vec<SoundFile>>,
    /// Asset key (`minecraft/sounds/…​.ogg`) → content hash.
    index: HashMap<String, String>,
    objects_dir: PathBuf,
    dl_tx: Sender<(String, String)>,
    inflight: Arc<Mutex<HashSet<String>>>,
    counter: Cell<u64>,
    /// Sounds still playing, so `ClientboundStopSound` can silence them.
    live: Mutex<Vec<Playing>>,
    /// Category of the sound about to be played (see `set_category`).
    category: Cell<Option<crate::settings::SoundCategory>>,
}

/// One sound currently coming out of the speakers.
struct Playing {
    /// Event name without its namespace, e.g. `music_disc.cat`.
    name: String,
    category: Option<crate::settings::SoundCategory>,
    sink: Sink,
}

impl AudioEngine {
    /// Build the engine from an on-disk asset store. Fails if there is no audio
    /// device or the index / `sounds.json` are missing.
    pub fn new(assets_dir: &Path, index_id: &str) -> Result<Self> {
        let (stream, handle) =
            OutputStream::try_default().context("no default audio output device")?;

        let index_path = assets_dir.join("indexes").join(format!("{index_id}.json"));
        let index_json: Value = serde_json::from_slice(
            &std::fs::read(&index_path)
                .with_context(|| format!("reading asset index {}", index_path.display()))?,
        )?;
        let objects = index_json
            .get("objects")
            .and_then(|o| o.as_object())
            .context("asset index has no `objects`")?;
        let mut index = HashMap::with_capacity(objects.len());
        for (k, v) in objects {
            if let Some(h) = v.get("hash").and_then(|h| h.as_str()) {
                index.insert(k.clone(), h.to_string());
            }
        }

        let objects_dir = assets_dir.join("objects");
        let sj_hash = index
            .get("minecraft/sounds.json")
            .context("sounds.json missing from the asset index")?;
        let sj_path = object_path(&objects_dir, sj_hash);
        let sounds_json: Value = serde_json::from_slice(
            &std::fs::read(&sj_path)
                .with_context(|| format!("reading {}", sj_path.display()))?,
        )?;
        let defs = parse_defs(&sounds_json);
        info!(events = defs.len(), objects = index.len(), "audio: sound library loaded");

        let (dl_tx, dl_rx) = channel::<(String, String)>();
        let inflight = Arc::new(Mutex::new(HashSet::new()));
        spawn_downloader(dl_rx, objects_dir.clone(), inflight.clone());

        Ok(Self {
            _stream: stream,
            handle,
            defs,
            index,
            objects_dir,
            dl_tx,
            inflight,
            counter: Cell::new(0),
            live: Mutex::new(Vec::new()),
            category: Cell::new(None),
        })
    }

    /// Play a positional server sound. `category_gain` is `master × category`
    /// from the settings; `packet_volume` follows vanilla semantics (values > 1
    /// extend the audible range rather than getting louder).
    pub fn play_positional(
        &self,
        name: &str,
        category_gain: f32,
        packet_volume: f32,
        pitch: f32,
        distance: f32,
        seed: u64,
    ) {
        let range = 16.0 * packet_volume.max(1.0);
        let atten = (1.0 - distance / range).clamp(0.0, 1.0);
        let gain = category_gain * packet_volume.min(1.0) * atten;
        self.play_named(name, gain, pitch, seed);
    }

    /// Play a non-positional UI sound (menu clicks) at `gain` (master volume).
    pub fn play_ui(&self, name: &str, gain: f32) {
        let seed = self.next_seed();
        self.play_named(name, gain, 1.0, seed);
    }

    /// `ClientboundStopSound`: silence what is playing. Either filter may be
    /// absent — no name means every sound, no category means every category.
    /// A long sound (a record, an ambient loop) is what this is for; short ones
    /// have usually finished by the time the packet lands, which is fine.
    pub fn stop(&self, name: Option<&str>, category: Option<crate::settings::SoundCategory>) {
        let key = name.map(strip_ns);
        let mut live = match self.live.lock() {
            Ok(l) => l,
            Err(e) => e.into_inner(),
        };
        live.retain(|s| {
            let hit = key.is_none_or(|k| s.name == k)
                && category.is_none_or(|c| s.category == Some(c));
            if hit {
                s.sink.stop();
            }
            !hit && !s.sink.empty()
        });
    }

    /// The category the *next* played sound belongs to. The play methods are
    /// called from many places with the gain already folded in, so rather than
    /// threading a category through all of them, the caller sets it here right
    /// before playing (single-threaded: the app plays sounds from one place).
    pub fn set_category(&self, category: Option<crate::settings::SoundCategory>) {
        self.category.set(category);
    }

    /// Whether a sound event of this name exists in sounds.json (namespace
    /// tolerated) — lets callers fall back before requesting a bogus key.
    pub fn has(&self, name: &str) -> bool {
        self.defs.contains_key(strip_ns(name))
    }

    /// A fresh pseudo-random seed for locally synthesized sounds, so variant
    /// selection rotates like server-sent sounds.
    pub fn local_seed(&self) -> u64 {
        self.next_seed()
    }

    fn play_named(&self, name: &str, gain: f32, pitch: f32, seed: u64) {
        let key = strip_ns(name);
        let files = match self.defs.get(key) {
            Some(f) if !f.is_empty() => f,
            _ => return,
        };
        let f = pick_weighted(files, seed);
        let asset_key = format!("{}/sounds/{}.ogg", f.namespace, f.path);
        let hash = match self.index.get(&asset_key) {
            Some(h) => h.clone(),
            None => return,
        };
        let obj = object_path(&self.objects_dir, &hash);
        if !obj.exists() {
            // Not cached yet — fetch it for next time and stay silent now.
            self.enqueue(asset_key, hash);
            return;
        }

        let final_gain = (gain * f.volume).clamp(0.0, 1.0);
        if final_gain <= 0.001 {
            return;
        }
        let final_pitch = (pitch * f.pitch).clamp(0.05, 5.0);

        let data = match std::fs::read(&obj) {
            Ok(d) => d,
            Err(_) => return,
        };
        match rodio::Decoder::new(Cursor::new(data)) {
            Ok(dec) => {
                let src = dec.amplify(final_gain).speed(final_pitch);
                if let Ok(sink) = Sink::try_new(&self.handle) {
                    sink.append(src);
                    // Keep a handle so StopSound can silence it; finished ones
                    // are dropped here, which is also what detaching did.
                    if let Ok(mut live) = self.live.lock() {
                        live.retain(|p| !p.sink.empty());
                        // Never let a stuck sink pile up unbounded.
                        if live.len() < 64 {
                            live.push(Playing {
                                name: key.to_string(),
                                category: self.category.get(),
                                sink,
                            });
                            return;
                        }
                    }
                    sink.detach();
                }
            }
            Err(e) => warn!(sound = name, error = %e, "audio: decode failed"),
        }
    }

    fn enqueue(&self, key: String, hash: String) {
        if let Ok(mut set) = self.inflight.lock() {
            if set.contains(&key) {
                return;
            }
            set.insert(key.clone());
            let _ = self.dl_tx.send((key, hash));
        }
    }

    fn next_seed(&self) -> u64 {
        let n = self.counter.get().wrapping_add(1);
        self.counter.set(n);
        n
    }
}

/// `objects/<h0..2>/<hash>`.
fn object_path(objects_dir: &Path, hash: &str) -> PathBuf {
    objects_dir.join(&hash[0..2]).join(hash)
}

fn strip_ns(name: &str) -> &str {
    name.split_once(':').map(|(_, p)| p).unwrap_or(name)
}

/// One `type: "event"` alias entry: play a *different* named sound event
/// instead of a file, with this entry's own volume/pitch/weight layered on.
struct EventRef {
    target: String,
    volume: f32,
    pitch: f32,
    weight: u32,
}

/// An event's sound list before alias resolution.
#[derive(Default)]
struct RawEvent {
    direct: Vec<SoundFile>,
    refs: Vec<EventRef>,
}

/// Parse `sounds.json` into event → variants, resolving `type: "event"`
/// aliases (a sound that plays another named event instead of a file —
/// e.g. every parrot/note-block mob imitation, baby cat variants, a camel's
/// saddle sound) against that event's own direct file variants.
fn parse_defs(v: &Value) -> HashMap<String, Vec<SoundFile>> {
    let obj = match v.as_object() {
        Some(o) => o,
        None => return HashMap::new(),
    };
    let mut raw: HashMap<String, RawEvent> = HashMap::with_capacity(obj.len());
    for (event, entry) in obj {
        let sounds = match entry.get("sounds").and_then(|s| s.as_array()) {
            Some(a) => a,
            None => continue,
        };
        let mut r = RawEvent::default();
        for s in sounds {
            let (name, volume, pitch, weight, is_event) = match s {
                Value::String(name) => (name.clone(), 1.0f32, 1.0f32, 1u32, false),
                Value::Object(o) => (
                    o.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                    o.get("volume").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                    o.get("pitch").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                    o.get("weight").and_then(|x| x.as_u64()).unwrap_or(1).max(1) as u32,
                    o.get("type").and_then(|t| t.as_str()) == Some("event"),
                ),
                _ => continue,
            };
            if name.is_empty() {
                continue;
            }
            if is_event {
                r.refs.push(EventRef { target: name, volume, pitch, weight });
                continue;
            }
            let (namespace, path) = match name.split_once(':') {
                Some((ns, p)) => (ns.to_string(), p.to_string()),
                None => ("minecraft".to_string(), name),
            };
            r.direct.push(SoundFile { namespace, path, volume, pitch, weight });
        }
        raw.insert(event.clone(), r);
    }

    // Resolve aliases. Verified against the decompiled 26.1 client
    // (`SoundManager.Preparations.handleRegistration`): choosing an event-type
    // alternative re-rolls the target event's own weighted pick, multiplying
    // that pick's volume and pitch by the alias entry's own. Flattening this
    // into one list — target variant weight × alias weight — reproduces the
    // same distribution (real `sounds.json` never nests an alias more than
    // one level deep, confirmed by scanning all 26.1 `type: "event"` entries).
    let mut defs = HashMap::with_capacity(raw.len());
    for (event, r) in &raw {
        let mut files = r.direct.clone();
        for er in &r.refs {
            if let Some(target) = raw.get(&er.target) {
                files.extend(target.direct.iter().map(|tf| SoundFile {
                    namespace: tf.namespace.clone(),
                    path: tf.path.clone(),
                    volume: tf.volume * er.volume,
                    pitch: tf.pitch * er.pitch,
                    weight: tf.weight.saturating_mul(er.weight),
                }));
            }
        }
        if !files.is_empty() {
            defs.insert(event.clone(), files);
        }
    }
    defs
}

/// Pick one variant with probability proportional to its `weight`, using
/// `seed` the same way the old uniform pick did (rotates through on repeat
/// plays; a real `RandomSource` in vanilla, but this only needs to feel
/// varied, not be cryptographically random).
fn pick_weighted(files: &[SoundFile], seed: u64) -> &SoundFile {
    let total: u64 = files.iter().map(|f| f.weight as u64).sum();
    if total == 0 {
        return &files[0];
    }
    let mut idx = seed % total;
    for f in files {
        let w = f.weight as u64;
        if idx < w {
            return f;
        }
        idx -= w;
    }
    &files[files.len() - 1]
}

/// Background thread: fetch requested OGG objects into the cache. Uses a tiny
/// current-thread tokio runtime with reqwest (already a dependency), so there's
/// no extra HTTP crate.
fn spawn_downloader(
    rx: Receiver<(String, String)>,
    objects_dir: PathBuf,
    inflight: Arc<Mutex<HashSet<String>>>,
) {
    let _ = std::thread::Builder::new()
        .name("sound-dl".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(r) => r,
                Err(_) => return,
            };
            let client = reqwest::Client::builder()
                .user_agent(concat!("DolphinClient/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            rt.block_on(async move {
                while let Ok((key, hash)) = rx.recv() {
                    let dest = object_path(&objects_dir, &hash);
                    if !dest.exists() {
                        let url = format!("{RESOURCES}/{}/{}", &hash[0..2], hash);
                        if let Ok(resp) = client.get(&url).send().await
                            && resp.status().is_success()
                            && let Ok(bytes) = resp.bytes().await
                        {
                            if let Some(parent) = dest.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let tmp = dest.with_extension("part");
                            if std::fs::write(&tmp, &bytes).is_ok() {
                                let _ = std::fs::rename(&tmp, &dest);
                            }
                        }
                    }
                    if let Ok(mut set) = inflight.lock() {
                        set.remove(&key);
                    }
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A `type: "event"` alias with no direct variants of its own (the real
    /// 26.1 `entity.parrot.imitate.creeper`, which used to resolve to nothing
    /// and drop the sound entirely) must play the target event's real files.
    #[test]
    fn event_alias_resolves_to_its_targets_real_files() {
        let v = json!({
            "entity.creeper.primed": {
                "sounds": ["entity/creeper/primed"]
            },
            "entity.parrot.imitate.creeper": {
                "sounds": [
                    { "name": "entity.creeper.primed", "type": "event", "pitch": 1.8, "volume": 0.6 }
                ]
            }
        });
        let defs = parse_defs(&v);
        let files = defs.get("entity.parrot.imitate.creeper").expect("alias must resolve, not vanish");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "entity/creeper/primed");
        // Composed, not overwritten: alias volume/pitch multiply the target's own.
        assert!((files[0].volume - 0.6).abs() < 1e-6);
        assert!((files[0].pitch - 1.8).abs() < 1e-6);
    }

    /// An event made of *only* aliases (no direct sound of its own — most of
    /// the 26.1 mob-imitation/variant events) must not disappear from `defs`.
    #[test]
    fn pure_alias_event_is_not_dropped() {
        let v = json!({
            "entity.cat.purr": { "sounds": ["entity/cat/purr1", "entity/cat/purr2"] },
            "entity.baby_cat.purr": {
                "sounds": [{ "name": "entity.cat.purr", "type": "event" }]
            }
        });
        let defs = parse_defs(&v);
        assert_eq!(defs.get("entity.baby_cat.purr").map(Vec::len), Some(2));
    }

    /// A mix of direct variants and one alias (the real `music.creative`,
    /// the only 26.1 event that combines both) keeps the direct ones too.
    #[test]
    fn mixed_direct_and_alias_keeps_both() {
        let v = json!({
            "music.game": { "sounds": ["music/game/a", "music/game/b"] },
            "music.creative": {
                "sounds": [
                    { "name": "music.game", "type": "event" },
                    "music/game/creative/aria_math"
                ]
            }
        });
        let defs = parse_defs(&v);
        let files = defs.get("music.creative").unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.iter().any(|f| f.path == "music/game/creative/aria_math"));
        assert!(files.iter().any(|f| f.path == "music/game/a"));
        assert!(files.iter().any(|f| f.path == "music/game/b"));
    }

    /// Weighted picking must respect real `sounds.json` weights (e.g. a rare
    /// variant listed at `weight: 2` next to a common one) rather than
    /// treating every variant as equally likely.
    #[test]
    fn pick_weighted_respects_relative_weight() {
        let files = vec![
            SoundFile { namespace: "minecraft".into(), path: "common".into(), volume: 1.0, pitch: 1.0, weight: 18 },
            SoundFile { namespace: "minecraft".into(), path: "rare".into(), volume: 1.0, pitch: 1.0, weight: 2 },
        ];
        let mut common = 0;
        let mut rare = 0;
        for seed in 0..20u64 {
            match pick_weighted(&files, seed).path.as_str() {
                "common" => common += 1,
                "rare" => rare += 1,
                _ => unreachable!(),
            }
        }
        // Exactly the 18:2 split a modulo-20 walk over these weights produces.
        assert_eq!((common, rare), (18, 2));
    }

    /// A plain, unweighted event (the overwhelming majority of `sounds.json`)
    /// still cycles through every variant, unweighted picking's old behavior.
    #[test]
    fn default_weight_is_uniform() {
        let v = json!({
            "block.stone.break": { "sounds": ["a", "b", "c", "d"] }
        });
        let defs = parse_defs(&v);
        let files = &defs["block.stone.break"];
        assert_eq!(files.iter().map(|f| f.weight).collect::<Vec<_>>(), vec![1, 1, 1, 1]);
        let picks: Vec<&str> =
            (0..4).map(|seed| pick_weighted(files, seed).path.as_str()).collect();
        assert_eq!(picks, vec!["a", "b", "c", "d"]);
    }
}
