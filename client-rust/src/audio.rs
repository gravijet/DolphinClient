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
struct SoundFile {
    /// Resource namespace (almost always `minecraft`).
    namespace: String,
    /// Path under `<namespace>/sounds/`, e.g. `mob/zombie/say1`.
    path: String,
    volume: f32,
    pitch: f32,
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
        let f = &files[(seed as usize) % files.len()];
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

/// Parse `sounds.json` into event → variants. Skips `type: "event"` aliases
/// (they reference another event and would need recursion — rare in practice).
fn parse_defs(v: &Value) -> HashMap<String, Vec<SoundFile>> {
    let mut defs = HashMap::new();
    let obj = match v.as_object() {
        Some(o) => o,
        None => return defs,
    };
    for (event, entry) in obj {
        let sounds = match entry.get("sounds").and_then(|s| s.as_array()) {
            Some(a) => a,
            None => continue,
        };
        let mut files = Vec::new();
        for s in sounds {
            let (name, volume, pitch, is_event) = match s {
                Value::String(name) => (name.clone(), 1.0f32, 1.0f32, false),
                Value::Object(o) => (
                    o.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                    o.get("volume").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                    o.get("pitch").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                    o.get("type").and_then(|t| t.as_str()) == Some("event"),
                ),
                _ => continue,
            };
            if name.is_empty() || is_event {
                continue;
            }
            let (namespace, path) = match name.split_once(':') {
                Some((ns, p)) => (ns.to_string(), p.to_string()),
                None => ("minecraft".to_string(), name),
            };
            files.push(SoundFile { namespace, path, volume, pitch });
        }
        if !files.is_empty() {
            defs.insert(event.clone(), files);
        }
    }
    defs
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
