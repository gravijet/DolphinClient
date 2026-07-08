//! Reading vanilla assets straight out of the 26.1 client jar (a zip).
//! Single-threaded, used only during startup baking.

pub mod atlas;
pub mod blockmap;
pub mod items;

use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Wraps the vanilla client jar. All lookups accept short refs
/// ("block/stone", "minecraft:block/stone") and resolve to
/// `assets/minecraft/...` paths inside the jar.
pub struct AssetPack {
    zip: zip::ZipArchive<BufReader<File>>,
}

/// Split `"ns:rest"` into `(ns, rest)`; a ref without a namespace defaults
/// to `"minecraft"`.
fn split_namespace(r: &str) -> (&str, &str) {
    match r.split_once(':') {
        Some((ns, rest)) if !ns.is_empty() => (ns, rest),
        Some((_, rest)) => ("minecraft", rest),
        None => ("minecraft", r),
    }
}

/// "block/stone" | "minecraft:block/stone" -> "assets/minecraft/textures/block/stone.png".
fn texture_path(tex_ref: &str) -> String {
    let (ns, rest) = split_namespace(tex_ref);
    if rest.ends_with(".png") {
        format!("assets/{ns}/textures/{rest}")
    } else {
        format!("assets/{ns}/textures/{rest}.png")
    }
}

/// "block/cube_all" -> Some("assets/minecraft/models/block/cube_all.json");
/// "builtin/..." refs have no file in the jar -> None.
fn model_path(model_ref: &str) -> Option<String> {
    let (ns, rest) = split_namespace(model_ref);
    if rest.starts_with("builtin/") {
        return None;
    }
    if rest.ends_with(".json") {
        Some(format!("assets/{ns}/models/{rest}"))
    } else {
        Some(format!("assets/{ns}/models/{rest}.json"))
    }
}

/// "stone" | "minecraft:stone" -> "assets/minecraft/blockstates/stone.json".
fn blockstate_path(block: &str) -> String {
    let (ns, short) = split_namespace(block);
    format!("assets/{ns}/blockstates/{short}.json")
}

impl AssetPack {
    pub fn open(jar: &Path) -> Result<Self> {
        let file = File::open(jar)
            .with_context(|| format!("opening client jar {}", jar.display()))?;
        let zip = zip::ZipArchive::new(BufReader::new(file))
            .with_context(|| format!("reading zip directory of {}", jar.display()))?;
        Ok(Self { zip })
    }

    /// Raw bytes of an exact path inside the jar, e.g.
    /// "assets/minecraft/textures/block/stone.png".
    pub fn read_bytes(&mut self, path: &str) -> Result<Vec<u8>> {
        let mut entry = self
            .zip
            .by_name(path)
            .with_context(|| format!("jar entry not found: {path}"))?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut buf)
            .with_context(|| format!("reading jar entry {path}"))?;
        Ok(buf)
    }

    /// Parsed `assets/minecraft/blockstates/<block>.json` (block short name
    /// without namespace, e.g. "oak_stairs").
    pub fn blockstate_json(&mut self, block: &str) -> Result<serde_json::Value> {
        let path = blockstate_path(block);
        let bytes = self.read_bytes(&path)?;
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {path}"))
    }

    /// Parsed model json. Accepts "block/cube_all", "minecraft:block/cube_all",
    /// or "builtin/generated" (returns an empty object for builtins).
    pub fn model_json(&mut self, model_ref: &str) -> Result<serde_json::Value> {
        let Some(path) = model_path(model_ref) else {
            // builtin models (item generators, chests, ...) have no JSON file.
            return Ok(serde_json::Value::Object(serde_json::Map::new()));
        };
        let bytes = self.read_bytes(&path)?;
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {path}"))
    }

    /// Decoded RGBA texture. Accepts "block/stone" or "minecraft:block/stone"
    /// (".png" appended). Animated strips (height > width) are cropped to the
    /// first width×width frame.
    pub fn texture_png(&mut self, tex_ref: &str) -> Result<image::RgbaImage> {
        let img = self.texture_png_raw(tex_ref)?;
        let (w, h) = (img.width(), img.height());
        let img = if h > w && w > 0 {
            // Vertical animation strip: keep the first frame only (v1: no animation).
            image::imageops::crop_imm(&img, 0, 0, w, w).to_image()
        } else {
            img
        };
        Ok(img)
    }

    /// Decoded RGBA texture, verbatim — no animation-strip cropping. Font
    /// atlases (e.g. `accented.png`, 144×900) are taller than wide but are NOT
    /// animations; cropping them would throw away most of the glyphs.
    pub fn texture_png_raw(&mut self, tex_ref: &str) -> Result<image::RgbaImage> {
        let path = texture_path(tex_ref);
        let bytes = self.read_bytes(&path)?;
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .with_context(|| format!("decoding PNG {path}"))?;
        Ok(img.into_rgba8())
    }

    /// All file paths in the jar starting with `prefix`.
    pub fn list_prefix(&mut self, prefix: &str) -> Vec<String> {
        self.zip
            .file_names()
            .filter(|n| n.starts_with(prefix) && !n.ends_with('/'))
            .map(str::to_owned)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Language / translations
// ---------------------------------------------------------------------------

/// Item/block display names. Loads the requested language from the launcher
/// asset store when available, always backed by the jar's `en_us.json`.
pub struct Lang {
    map: std::collections::HashMap<String, String>,
}

impl Lang {
    pub fn load(pack: &mut AssetPack, assets_dir: Option<&Path>, index_id: Option<&str>, code: &str) -> Lang {
        let mut map: std::collections::HashMap<String, String> = pack
            .read_bytes("assets/minecraft/lang/en_us.json")
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        // Overlay the requested language from the asset store (e.g. de_de).
        if code != "en_us"
            && let (Some(dir), Some(id)) = (assets_dir, index_id)
            && let Some(over) = load_lang_from_store(dir, id, code)
        {
            map.extend(over);
        }
        Lang { map }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(String::as_str)
    }

    /// Display name of an item registry name ("diamond_sword" → "Diamantschwert").
    pub fn item_name(&self, registry: &str) -> String {
        self.get(&format!("item.minecraft.{registry}"))
            .or_else(|| self.get(&format!("block.minecraft.{registry}")))
            .map(str::to_string)
            .unwrap_or_else(|| prettify(registry))
    }
}

/// `oak_stairs` → `Oak Stairs` — fallback when a translation is missing.
fn prettify(registry: &str) -> String {
    registry
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn load_lang_from_store(
    dir: &Path,
    index_id: &str,
    code: &str,
) -> Option<std::collections::HashMap<String, String>> {
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("indexes").join(format!("{index_id}.json"))).ok()?)
            .ok()?;
    let hash = index
        .get("objects")?
        .get(format!("minecraft/lang/{code}.json").as_str())?
        .get("hash")?
        .as_str()?;
    let path = dir.join("objects").join(&hash[0..2]).join(hash);
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prettify_fallback_names() {
        assert_eq!(prettify("diamond_sword"), "Diamond Sword");
        assert_eq!(prettify("tnt"), "Tnt");
    }

    #[test]
    fn ref_normalization() {
        assert_eq!(
            texture_path("block/stone"),
            "assets/minecraft/textures/block/stone.png"
        );
        assert_eq!(
            texture_path("minecraft:block/stone"),
            "assets/minecraft/textures/block/stone.png"
        );
        assert_eq!(
            texture_path("block/stone.png"),
            "assets/minecraft/textures/block/stone.png"
        );
        assert_eq!(
            model_path("block/cube_all").as_deref(),
            Some("assets/minecraft/models/block/cube_all.json")
        );
        assert_eq!(
            model_path("minecraft:block/cube_all").as_deref(),
            Some("assets/minecraft/models/block/cube_all.json")
        );
        assert_eq!(model_path("builtin/generated"), None);
        assert_eq!(model_path("minecraft:builtin/generated"), None);
        assert_eq!(
            blockstate_path("oak_stairs"),
            "assets/minecraft/blockstates/oak_stairs.json"
        );
        assert_eq!(
            blockstate_path("minecraft:oak_stairs"),
            "assets/minecraft/blockstates/oak_stairs.json"
        );
    }

    const JAR: &str = "/home/benj/DolphinClient/.mc-cache/client-26.1.jar";

    #[test]
    fn real_jar_smoke() {
        let jar = Path::new(JAR);
        if !jar.exists() {
            eprintln!("skipping real_jar_smoke: {JAR} not present");
            return;
        }
        let mut pack = AssetPack::open(jar).expect("open client jar");

        // Plain 16x16 texture, both ref forms.
        let stone = pack.texture_png("block/stone").expect("stone texture");
        assert_eq!((stone.width(), stone.height()), (16, 16));
        let stone2 = pack.texture_png("minecraft:block/stone").expect("stone texture (ns)");
        assert_eq!(stone.as_raw(), stone2.as_raw());

        // Animated strip crops to square.
        let water = pack.texture_png("block/water_still").expect("water_still texture");
        assert_eq!(water.width(), water.height());

        // Blockstate + model JSON.
        let bs = pack.blockstate_json("stone").expect("stone blockstate");
        assert!(bs.get("variants").is_some(), "stone.json has variants");
        let model = pack.model_json("block/cube_all").expect("cube_all model");
        assert!(model.is_object() && !model.as_object().unwrap().is_empty());
        let builtin = pack.model_json("builtin/generated").expect("builtin model");
        assert_eq!(builtin, serde_json::Value::Object(serde_json::Map::new()));

        // Listing.
        let states = pack.list_prefix("assets/minecraft/blockstates/");
        assert!(states.len() > 500, "expected many blockstates, got {}", states.len());
        assert!(states.iter().all(|p| p.ends_with(".json")));
    }
}
