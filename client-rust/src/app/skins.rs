//! Player skins: async download + disk cache + egui head sprites + full skin
//! images for the 3D player renderer.
//!
//! Sources, in order: in-memory → our disk cache (`<config>/skins/<key>.png`)
//! → the launcher asset store's skin cache (`assets/skins/<2ch>/<key>`, the
//! vanilla layout) → HTTP download (textures.minecraft.net), cached on disk.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{TextureHandle, TextureOptions};
use image::RgbaImage;
use tracing::{info, warn};

use crate::settings::GameSettings;

struct Entry {
    img: Arc<RgbaImage>,
    head: Option<TextureHandle>,
    /// Front-facing paper-doll sprite (16×32), for the inventory preview.
    body: Option<TextureHandle>,
}

pub struct SkinManager {
    cache_dir: PathBuf,
    seed_dir: Option<PathBuf>,
    req_tx: Sender<(String, String)>,
    done_rx: Receiver<(String, RgbaImage)>,
    entries: HashMap<String, Entry>,
    requested: HashSet<String>,
}

/// Stable cache key of a skin URL: the texture hash path segment.
pub fn key_of_url(url: &str) -> String {
    let seg = url.rsplit('/').next().unwrap_or(url);
    let clean: String = seg.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if clean.is_empty() {
        format!("{:x}", fnv64(url.as_bytes()))
    } else {
        clean
    }
}

/// FNV-1a — stable renderer key for a skin cache key.
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h.max(1) // 0 is reserved for the default (Steve) skin
}

impl SkinManager {
    pub fn new(assets_dir: Option<&std::path::Path>) -> Self {
        let cache_dir = GameSettings::config_dir().join("skins");
        let _ = std::fs::create_dir_all(&cache_dir);
        let seed_dir = assets_dir.map(|d| d.join("skins")).filter(|d| d.is_dir());
        let (req_tx, req_rx) = channel::<(String, String)>();
        let (done_tx, done_rx) = channel::<(String, RgbaImage)>();
        spawn_downloader(req_rx, done_tx, cache_dir.clone());
        Self {
            cache_dir,
            seed_dir,
            req_tx,
            done_rx,
            entries: HashMap::new(),
            requested: HashSet::new(),
        }
    }

    /// Load a private skin selected in the launcher. The synthetic URL lets all
    /// existing tab-list, paper-doll and renderer paths treat it like a normal
    /// texture while `request()` never sends it to the downloader.
    pub fn load_local_skin(&mut self, path: &std::path::Path) -> anyhow::Result<String> {
        let img = image::open(path)?.to_rgba8();
        let (w, h) = img.dimensions();
        anyhow::ensure!(
            w >= 64 && w <= 1024 && (h == w || h * 2 == w),
            "local skin must use the Vanilla 64x64 or 64x32 layout"
        );
        let img = normalize_skin(img);
        let url = "local://dolphin-private-skin".to_string();
        let key = key_of_url(&url);
        self.requested.insert(key.clone());
        self.entries.insert(
            key,
            Entry {
                img: Arc::new(img),
                head: None,
                body: None,
            },
        );
        Ok(url)
    }

    /// Load a private 2:1 cape and pad it to the renderer's 64×64 texture
    /// convention. It remains local to this process.
    pub fn load_local_cape(&mut self, path: &std::path::Path) -> anyhow::Result<String> {
        let img = image::open(path)?.to_rgba8();
        let (w, h) = img.dimensions();
        anyhow::ensure!(
            w >= 64 && w <= 1024 && h * 2 == w,
            "local cape must use the Vanilla 64x32 layout"
        );
        let resized = image::imageops::resize(&img, 64, 32, image::imageops::FilterType::Nearest);
        let mut padded = RgbaImage::new(64, 64);
        image::imageops::overlay(&mut padded, &resized, 0, 0);
        let url = "local://dolphin-private-cape".to_string();
        let key = key_of_url(&url);
        self.requested.insert(key.clone());
        self.entries.insert(
            key,
            Entry {
                img: Arc::new(padded),
                head: None,
                body: None,
            },
        );
        Ok(url)
    }

    /// Drain finished downloads (call once per frame).
    pub fn poll(&mut self) {
        while let Ok((key, img)) = self.done_rx.try_recv() {
            let img = normalize_skin(img);
            self.entries.insert(
                key,
                Entry {
                    img: Arc::new(img),
                    head: None,
                    body: None,
                },
            );
        }
    }

    /// Make sure a skin is loaded or being fetched.
    pub fn request(&mut self, url: &str) {
        let key = key_of_url(url);
        if self.entries.contains_key(&key) || !self.requested.insert(key.clone()) {
            return;
        }
        // Synchronous fast paths: our cache, then the asset-store skin cache.
        let cached = self.cache_dir.join(format!("{key}.png"));
        if let Ok(img) = image::open(&cached) {
            self.entries.insert(
                key,
                Entry {
                    img: Arc::new(normalize_skin(img.to_rgba8())),
                    head: None,
                    body: None,
                },
            );
            return;
        }
        if let Some(seed) = &self.seed_dir
            && key.len() >= 2
        {
            // Asset-store skins are hash-named with no extension; decode from
            // the bytes so the PNG format is sniffed by content.
            let p = seed.join(&key[0..2]).join(&key);
            if let Ok(bytes) = std::fs::read(&p)
                && let Ok(img) = image::load_from_memory(&bytes)
            {
                info!(key, "skins: found in launcher asset cache");
                let img = normalize_skin(img.to_rgba8());
                let _ = img.save(&cached);
                self.entries.insert(
                    key,
                    Entry {
                        img: Arc::new(img),
                        head: None,
                        body: None,
                    },
                );
                return;
            }
        }
        let _ = self.req_tx.send((key, url.to_string()));
    }

    /// Full 64×64 skin image (renderer upload), if loaded.
    pub fn skin(&self, url: &str) -> Option<Arc<RgbaImage>> {
        self.entries.get(&key_of_url(url)).map(|e| e.img.clone())
    }

    /// 8×-scaled head sprite (face + hat overlay) for tab list / GUIs.
    pub fn head(&mut self, ctx: &egui::Context, url: &str) -> Option<TextureHandle> {
        let key = key_of_url(url);
        let entry = self.entries.get_mut(&key)?;
        if entry.head.is_none() {
            let img = head_sprite(&entry.img);
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            );
            entry.head =
                Some(ctx.load_texture(format!("head-{key}"), color, TextureOptions::NEAREST));
        }
        entry.head.clone()
    }

    /// Front-facing paper-doll sprite (16×32 px) for the inventory preview.
    /// Cached per skin; drawn NEAREST-scaled by the container screen.
    pub fn body(&mut self, ctx: &egui::Context, url: &str, slim: bool) -> Option<TextureHandle> {
        let key = key_of_url(url);
        let entry = self.entries.get_mut(&key)?;
        if entry.body.is_none() {
            let img = body_sprite(&entry.img, slim);
            let color = egui::ColorImage::from_rgba_unmultiplied([16, 32], img.as_raw());
            entry.body =
                Some(ctx.load_texture(format!("body-{key}"), color, TextureOptions::NEAREST));
        }
        entry.body.clone()
    }
}

/// Compose a front-facing player paper-doll (head, body, arms, legs) from a
/// 64×64 skin, applying each part's overlay layer. Output is a 16×32 sprite:
/// arm(4) + body(8) + arm(4) wide, head(8) + torso(12) + legs(12) tall.
fn body_sprite(skin: &RgbaImage, slim: bool) -> RgbaImage {
    let aw: u32 = if slim { 3 } else { 4 };
    let mut out = RgbaImage::new(16, 32);
    // Blit a front-face region (`w`×`h` at `sx,sy`) to `dx,dy`, letting the
    // overlay layer at `ox,oy` win where it's opaque (hat / jacket / sleeves).
    let mut part = |sx: u32, sy: u32, w: u32, h: u32, ox: u32, oy: u32, dx: i64, dy: i64| {
        for y in 0..h {
            for x in 0..w {
                let mut px = *skin.get_pixel(sx + x, sy + y);
                let ov = *skin.get_pixel(ox + x, oy + y);
                if ov.0[3] > 8 {
                    px = ov;
                }
                let (tx, ty) = (dx + x as i64, dy + y as i64);
                if px.0[3] > 0 && (0..16).contains(&tx) && (0..32).contains(&ty) {
                    out.put_pixel(tx as u32, ty as u32, px);
                }
            }
        }
    };
    // Head (base 8,8 / hat overlay 40,8) centered over the 16-wide canvas.
    part(8, 8, 8, 8, 40, 8, 4, 0);
    // Torso (base 20,20 / jacket overlay 20,36).
    part(20, 20, 8, 12, 20, 36, 4, 8);
    // Right arm (base 44,20 / sleeve 44,36), sitting left of the torso.
    part(44, 20, aw, 12, 44, 36, 4 - aw as i64, 8);
    // Left arm (base 36,52 / sleeve 52,52), right of the torso.
    part(36, 52, aw, 12, 52, 52, 12, 8);
    // Right leg (base 4,20 / overlay 4,36).
    part(4, 20, 4, 12, 4, 36, 4, 20);
    // Left leg (base 20,52 / overlay 4,52).
    part(20, 52, 4, 12, 4, 52, 8, 20);
    out
}

/// Compose the 8×8 face + hat layer, upscaled 8× (nearest) to 64×64.
fn head_sprite(skin: &RgbaImage) -> RgbaImage {
    let mut face = RgbaImage::new(8, 8);
    for y in 0..8 {
        for x in 0..8 {
            let mut px = *skin.get_pixel(8 + x, 8 + y);
            let hat = *skin.get_pixel(40 + x, 8 + y);
            if hat.0[3] > 8 {
                px = hat;
            }
            if px.0[3] == 0 {
                px = image::Rgba([0, 0, 0, 255]); // never fully transparent faces
            }
            face.put_pixel(x, y, px);
        }
    }
    image::imageops::resize(&face, 64, 64, image::imageops::FilterType::Nearest)
}

/// Bring any skin to the modern 64×64 layout (mirror legacy 64×32 limbs).
pub fn normalize_skin(img: RgbaImage) -> RgbaImage {
    if img.width() == 64 && img.height() == 64 {
        return img;
    }
    if img.width() == 64 && img.height() == 32 {
        let mut out = RgbaImage::new(64, 64);
        image::imageops::overlay(&mut out, &img, 0, 0);
        // Left leg (16,48) ← mirrored right leg (0,16); left arm (32,48) ←
        // mirrored right arm (40,16). Whole-region mirror (close enough for
        // the classic symmetric skins that still use the legacy layout).
        let leg = image::imageops::flip_horizontal(
            &image::imageops::crop_imm(&img, 0, 16, 16, 16).to_image(),
        );
        image::imageops::overlay(&mut out, &leg, 16, 48);
        let arm = image::imageops::flip_horizontal(
            &image::imageops::crop_imm(&img, 40, 16, 16, 16).to_image(),
        );
        image::imageops::overlay(&mut out, &arm, 32, 48);
        return out;
    }
    // Unexpected size: scale to 64×64 as a last resort.
    image::imageops::resize(&img, 64, 64, image::imageops::FilterType::Nearest)
}

/// Background downloader: fetch skin PNGs and cache them on disk.
fn spawn_downloader(
    rx: Receiver<(String, String)>,
    tx: Sender<(String, RgbaImage)>,
    cache_dir: PathBuf,
) {
    let _ = std::thread::Builder::new()
        .name("skin-dl".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(_) => return,
            };
            let client = reqwest::Client::builder()
                .user_agent(concat!("DolphinClient/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            rt.block_on(async move {
                while let Ok((key, url)) = rx.recv() {
                    // Only fetch from Mojang's texture servers.
                    let allowed = url.starts_with("http://textures.minecraft.net/")
                        || url.starts_with("https://textures.minecraft.net/");
                    if !allowed {
                        warn!(url, "skins: refusing non-Mojang skin URL");
                        continue;
                    }
                    let url = url.replacen("http://", "https://", 1);
                    match client.get(&url).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(bytes) = resp.bytes().await
                                && let Ok(img) = image::load_from_memory(&bytes)
                            {
                                let img = img.to_rgba8();
                                let _ = img.save(cache_dir.join(format!("{key}.png")));
                                let _ = tx.send((key, img));
                            }
                        }
                        Ok(resp) => warn!(url, status = %resp.status(), "skins: download failed"),
                        Err(e) => warn!(url, error = %e, "skins: download failed"),
                    }
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_keys_are_stable_and_clean() {
        let url = "http://textures.minecraft.net/texture/abc123DEF";
        assert_eq!(key_of_url(url), "abc123DEF");
        assert_eq!(key_of_url("weird///"), key_of_url("weird///"));
        assert_ne!(fnv64(b"a"), fnv64(b"b"));
        assert!(fnv64(b"") >= 1);
    }

    #[test]
    fn legacy_skins_get_extended() {
        let legacy = RgbaImage::from_pixel(64, 32, image::Rgba([10, 20, 30, 255]));
        let out = normalize_skin(legacy);
        assert_eq!((out.width(), out.height()), (64, 64));
        // Mirrored left-leg region is filled.
        assert_eq!(out.get_pixel(20, 52).0[3], 255);
    }
}
