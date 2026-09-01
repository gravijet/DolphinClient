//! Local, per-profile skin and cape imports.
//!
//! Imported PNGs are decoded, validated and normalized before they enter the
//! DolphinClient config directory. The original path is never needed again and
//! no cosmetic is uploaded to a server — the native client reads these copies
//! only for the active local player.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::{imageops::FilterType, RgbaImage};

use crate::config;

fn profile_dir(uuid: &str) -> PathBuf {
    let safe: String = uuid.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    config::config_dir().join("cosmetics").join(safe)
}

fn read_png(path: &Path) -> Result<RgbaImage> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Could not read {}", path.display()))?;
    image::load_from_memory(&bytes)
        .context("The selected file is not a readable PNG image")
        .map(|img| img.to_rgba8())
}

/// Normalize a Vanilla skin to 64×64. HD skins are accepted when their size is
/// an integer multiple of the 64×64 or legacy 64×32 layout and downscaled with
/// nearest-neighbour filtering so pixel art remains crisp.
pub fn normalize_skin(img: RgbaImage) -> Result<RgbaImage> {
    let (w, h) = img.dimensions();
    if w < 64 || w > 1024 || (h != w && h * 2 != w) || w % 64 != 0 {
        bail!(
            "Skins must use the Vanilla 64×64 or legacy 64×32 layout (HD multiples are supported)."
        );
    }
    let legacy = h * 2 == w;
    let resized =
        image::imageops::resize(&img, 64, if legacy { 32 } else { 64 }, FilterType::Nearest);
    if !legacy {
        return Ok(resized);
    }

    let mut out = RgbaImage::new(64, 64);
    image::imageops::overlay(&mut out, &resized, 0, 0);
    let leg = image::imageops::flip_horizontal(
        &image::imageops::crop_imm(&resized, 0, 16, 16, 16).to_image(),
    );
    image::imageops::overlay(&mut out, &leg, 16, 48);
    let arm = image::imageops::flip_horizontal(
        &image::imageops::crop_imm(&resized, 40, 16, 16, 16).to_image(),
    );
    image::imageops::overlay(&mut out, &arm, 32, 48);
    Ok(out)
}

/// Normalize a 2:1 Vanilla cape sheet to the renderer's padded 64×64 sheet.
pub fn normalize_cape(img: RgbaImage) -> Result<RgbaImage> {
    let (w, h) = img.dimensions();
    if w < 64 || w > 1024 || h * 2 != w || w % 64 != 0 {
        bail!("Capes must use the Vanilla 64×32 layout (HD multiples are supported).");
    }
    let resized = image::imageops::resize(&img, 64, 32, FilterType::Nearest);
    let mut out = RgbaImage::new(64, 64);
    image::imageops::overlay(&mut out, &resized, 0, 0);
    Ok(out)
}

fn save_import(uuid: &str, file: &str, img: &RgbaImage) -> Result<PathBuf> {
    let dir = profile_dir(uuid);
    std::fs::create_dir_all(&dir).with_context(|| format!("Could not create {}", dir.display()))?;
    let path = dir.join(file);
    img.save(&path)
        .with_context(|| format!("Could not save {}", path.display()))?;
    Ok(path)
}

pub fn import_skin(uuid: &str, source: &Path) -> Result<PathBuf> {
    save_import(uuid, "skin.png", &normalize_skin(read_png(source)?)?)
}

pub fn import_cape(uuid: &str, source: &Path) -> Result<PathBuf> {
    save_import(uuid, "cape.png", &normalize_cape(read_png(source)?)?)
}

/// Face + hat overlay, enlarged for the launcher's account cards.
pub fn head_sprite(skin: &RgbaImage) -> RgbaImage {
    let mut face = RgbaImage::new(8, 8);
    for y in 0..8 {
        for x in 0..8 {
            let mut px = *skin.get_pixel(8 + x, 8 + y);
            let hat = *skin.get_pixel(40 + x, 8 + y);
            if hat.0[3] > 8 {
                px = hat;
            }
            face.put_pixel(x, y, px);
        }
    }
    image::imageops::resize(&face, 64, 64, FilterType::Nearest)
}

/// Front-facing skin paper doll used by the Cosmetics and Home previews.
pub fn body_sprite(skin: &RgbaImage, slim: bool) -> RgbaImage {
    let arm_width: u32 = if slim { 3 } else { 4 };
    let mut out = RgbaImage::new(16, 32);
    let mut part = |sx: u32, sy: u32, w: u32, h: u32, ox: u32, oy: u32, dx: i64, dy: i64| {
        for y in 0..h {
            for x in 0..w {
                let mut px = *skin.get_pixel(sx + x, sy + y);
                let overlay = *skin.get_pixel(ox + x, oy + y);
                if overlay.0[3] > 8 {
                    px = overlay;
                }
                let (tx, ty) = (dx + x as i64, dy + y as i64);
                if px.0[3] > 0 && (0..16).contains(&tx) && (0..32).contains(&ty) {
                    out.put_pixel(tx as u32, ty as u32, px);
                }
            }
        }
    };
    part(8, 8, 8, 8, 40, 8, 4, 0);
    part(20, 20, 8, 12, 20, 36, 4, 8);
    part(44, 20, arm_width, 12, 44, 36, 4 - arm_width as i64, 8);
    part(36, 52, arm_width, 12, 52, 52, 12, 8);
    part(4, 20, 4, 12, 4, 36, 4, 20);
    part(20, 52, 4, 12, 4, 52, 8, 20);
    image::imageops::resize(&out, 64, 128, FilterType::Nearest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_normalizes_vanilla_layouts() {
        assert_eq!(
            normalize_skin(RgbaImage::new(64, 64)).unwrap().dimensions(),
            (64, 64)
        );
        assert_eq!(
            normalize_skin(RgbaImage::new(128, 64))
                .unwrap()
                .dimensions(),
            (64, 64)
        );
        assert!(normalize_skin(RgbaImage::new(63, 63)).is_err());
        assert_eq!(
            normalize_cape(RgbaImage::new(128, 64))
                .unwrap()
                .dimensions(),
            (64, 64)
        );
        assert!(normalize_cape(RgbaImage::new(64, 64)).is_err());
    }
}
