//! Blockstate + block model JSON pipeline:
//! blockstate variants/multipart → model refs (+x/y rotation) → resolved model
//! (parent chain, texture variables) → baked quads with final atlas UVs.
//!
//! Vanilla format reference:
//! - blockstates/<block>.json: {"variants": {"": M | [M,...], "k=v,k2=v2": M}}
//!   or {"multipart": [{"when": {...} | {"OR": [...]}, "apply": M | [M,...]}]}
//!   where M = {"model": "minecraft:block/x", "x": 90?, "y": 180?, "uvlock"?}
//! - models/*.json: {"parent": ref?, "textures": {var: "#other" | "block/x"},
//!   "elements": [{"from": [x,y,z 0-16], "to": [...], "rotation": {origin, axis,
//!   angle, rescale}?, "faces": {down/up/north/south/west/east: {"texture": "#var",
//!   "uv": [u0,v0,u1,v1 0-16]?, "cullface": dir?, "rotation": 0/90/180/270?,
//!   "tintindex": n?}}}]}
//!
//! Variant selection: for a state's props, "k=v,..." keys match iff every listed
//! pair equals the state's value. Arrays of M = random variants → pick first.
//! Multipart "when": {"prop": "v1|v2"} matches any of; OR combines conditions;
//! missing "when" always applies.

pub mod bake;

pub use bake::{BakedModelStore, BakedQuad, TintKind};

use crate::types::Face;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Blockstate selection
// ---------------------------------------------------------------------------

/// One model to draw for a state: a model ref plus the variant rotation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ModelRef {
    pub model: String,
    /// Variant rotation in degrees, normalized to 0/90/180/270.
    pub x: i32,
    pub y: i32,
}

/// Normalize a rotation angle to {0, 90, 180, 270}.
fn norm_deg(v: f64) -> i32 {
    (((v.round() as i32).div_euclid(90)).rem_euclid(4)) * 90
}

/// Parse a variant value `M` or `[M, ...]` (pick the first — "random" variants
/// are position-seeded in vanilla; v1 always uses the first).
fn parse_model_ref(v: &Value) -> Option<ModelRef> {
    let v = if let Some(arr) = v.as_array() { arr.first()? } else { v };
    let obj = v.as_object()?;
    let model = obj.get("model")?.as_str()?.to_owned();
    let x = obj.get("x").and_then(Value::as_f64).map_or(0, norm_deg);
    let y = obj.get("y").and_then(Value::as_f64).map_or(0, norm_deg);
    Some(ModelRef { model, x, y })
}

/// Does variant key `"k=v,k2=v2"` (or `""`) match the state's props?
/// Returns the number of matched pairs (specificity) or None.
fn variant_key_match(key: &str, props: &[(String, String)]) -> Option<usize> {
    let key = key.trim();
    if key.is_empty() {
        return Some(0);
    }
    let mut n = 0usize;
    for pair in key.split(',') {
        let (k, v) = pair.split_once('=')?;
        let (k, v) = (k.trim(), v.trim());
        let (_, have) = props.iter().find(|(pk, _)| pk == k)?;
        if have != v {
            return None;
        }
        n += 1;
    }
    Some(n)
}

/// Multipart "when" condition. Object of prop → "v1|v2" (AND semantics),
/// or {"OR": [cond,...]} / {"AND": [cond,...]}. JSON bools/numbers stringify.
pub(crate) fn when_matches(cond: &Value, props: &[(String, String)]) -> bool {
    let Some(obj) = cond.as_object() else {
        return false;
    };
    if let Some(alts) = obj.get("OR").and_then(Value::as_array) {
        return alts.iter().any(|c| when_matches(c, props));
    }
    if let Some(all) = obj.get("AND").and_then(Value::as_array) {
        return all.iter().all(|c| when_matches(c, props));
    }
    obj.iter().all(|(k, v)| {
        let want = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(), // true → "true", 3 → "3"
        };
        match props.iter().find(|(pk, _)| pk == k) {
            Some((_, have)) => want.split('|').any(|alt| alt.trim() == have),
            None => false,
        }
    })
}

/// Select the models to draw for a state.
/// `None` = broken blockstate (caller substitutes the fallback cube);
/// `Some(vec)` may be empty (multipart where no part matched → draw nothing).
pub(crate) fn select_model_refs(bs: &Value, props: &[(String, String)]) -> Option<Vec<ModelRef>> {
    let obj = bs.as_object()?;
    if let Some(variants) = obj.get("variants").and_then(Value::as_object) {
        // Most specific matching key wins (vanilla keys are mutually
        // exclusive per block; specificity is belt-and-braces).
        let mut best: Option<(usize, &Value)> = None;
        for (key, val) in variants {
            if let Some(n) = variant_key_match(key, props)
                && best.is_none_or(|(bn, _)| n > bn) {
                    best = Some((n, val));
                }
        }
        return best.and_then(|(_, v)| parse_model_ref(v)).map(|m| vec![m]);
    }
    if let Some(parts) = obj.get("multipart").and_then(Value::as_array) {
        let mut out = Vec::new();
        for part in parts {
            let Some(pobj) = part.as_object() else {
                continue;
            };
            let applies = match pobj.get("when") {
                Some(cond) => when_matches(cond, props),
                None => true,
            };
            if applies
                && let Some(m) = pobj.get("apply").and_then(parse_model_ref_opt)
            {
                out.push(m);
            }
        }
        return Some(out);
    }
    None
}

fn parse_model_ref_opt(v: &Value) -> Option<ModelRef> {
    parse_model_ref(v)
}

// ---------------------------------------------------------------------------
// Model resolution (parent chain + texture variables)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub(crate) struct ResolvedModel {
    /// Texture variables, fully merged child-over-parent (values may still be
    /// "#var" references — resolve with [`lookup_texture`]).
    pub textures: HashMap<String, String>,
    pub elements: Vec<Element>,
    /// Normalized sprite names flagged `force_translucent` by the 26.x object
    /// texture form `{"sprite": ..., "force_translucent": true}` (glass,
    /// redstone overlays, ...) — quads using them go to the Translucent layer.
    pub translucent_sprites: HashSet<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Element {
    /// Model coords, 0..16.
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub rot: Option<ElemRot>,
    pub faces: Vec<(Face, ElemFace)>,
}

#[derive(Clone, Debug)]
pub(crate) struct ElemRot {
    /// Model coords, 0..16.
    pub origin: [f32; 3],
    /// 0 = x, 1 = y, 2 = z.
    pub axis: usize,
    /// Degrees, vanilla allows -45..45 in 22.5 steps.
    pub angle: f32,
    pub rescale: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ElemFace {
    /// Raw texture ref, usually "#var".
    pub texture: String,
    /// [u0, v0, u1, v1] in 0..16 texture units; None = face-projected default.
    pub uv: Option<[f32; 4]>,
    pub cullface: Option<Face>,
    /// UV rotation in degrees, normalized to 0/90/180/270.
    pub rotation: u32,
    pub tintindex: Option<i32>,
}

/// Strip the default "minecraft:" namespace so names match atlas keys
/// ("block/stone"); non-default namespaces are kept verbatim.
pub(crate) fn normalize_tex(name: &str) -> String {
    name.strip_prefix("minecraft:").unwrap_or(name).to_owned()
}

/// Ref without namespace, for builtin/ checks.
fn strip_ns(model_ref: &str) -> &str {
    match model_ref.split_once(':') {
        Some((_, rest)) => rest,
        None => model_ref,
    }
}

/// Follow "#var" references through the texture map (caps at 16 hops).
/// Returns the normalized concrete texture name, or None if unresolvable.
pub(crate) fn lookup_texture(textures: &HashMap<String, String>, tex_ref: &str) -> Option<String> {
    let mut cur = tex_ref;
    for _ in 0..16 {
        match cur.strip_prefix('#') {
            Some(var) => cur = textures.get(var)?.as_str(),
            // Vanilla tolerates a missing '#' (e.g. heavy_core.json faces use
            // "all"): a bare name that is a texture variable resolves as a
            // reference, otherwise it is a concrete texture path.
            None => match textures.get(cur) {
                Some(next) if next != cur => cur = next.as_str(),
                _ => return Some(normalize_tex(cur)),
            },
        }
    }
    None // reference cycle
}

/// Resolve a model ref through its parent chain: textures merge child-over-
/// parent, elements come from the deepest model that defines them.
/// `load` fetches raw model JSON by ref (see `AssetPack::model_json`);
/// injected so pure-logic tests can use synthetic JSON.
pub(crate) fn resolve_model(
    load: &mut dyn FnMut(&str) -> Result<Value>,
    model_ref: &str,
) -> Result<ResolvedModel> {
    let mut textures: HashMap<String, String> = HashMap::new();
    let mut translucent_sprites: HashSet<String> = HashSet::new();
    let mut elements_json: Option<Value> = None;
    let mut cur = model_ref.to_owned();
    let mut hops = 0;
    loop {
        if strip_ns(&cur).starts_with("builtin/") {
            // builtin/generated etc. — flat item models, nothing to bake in v1.
            break;
        }
        let json = load(&cur).with_context(|| format!("loading model {cur}"))?;
        let obj = json
            .as_object()
            .ok_or_else(|| anyhow!("model {cur} is not a JSON object"))?;
        if let Some(tex) = obj.get("textures").and_then(Value::as_object) {
            for (k, v) in tex {
                // Plain "block/x" string, or the 26.x object form
                // {"sprite": "block/x", "force_translucent": bool}.
                let (sprite, forced) = match v {
                    Value::String(s) => (Some(s.as_str()), false),
                    Value::Object(o) => (
                        o.get("sprite").and_then(Value::as_str),
                        o.get("force_translucent").and_then(Value::as_bool).unwrap_or(false),
                    ),
                    _ => (None, false),
                };
                let Some(s) = sprite else { continue };
                // Walking child → parent: first writer (deepest child) wins.
                if !textures.contains_key(k.as_str()) {
                    textures.insert(k.clone(), s.to_owned());
                    if forced && !s.starts_with('#') {
                        translucent_sprites.insert(normalize_tex(s));
                    }
                }
            }
        }
        if elements_json.is_none()
            && let Some(e) = obj.get("elements")
        {
            elements_json = Some(e.clone());
        }
        match obj.get("parent").and_then(Value::as_str) {
            Some(p) => cur = p.to_owned(),
            None => break,
        }
        hops += 1;
        if hops >= 32 {
            tracing::warn!("models: parent chain of {model_ref} exceeds 32 hops (cycle?), truncating");
            break;
        }
    }
    let elements = match &elements_json {
        Some(v) => parse_elements(v).with_context(|| format!("elements of model {model_ref}"))?,
        None => Vec::new(),
    };
    Ok(ResolvedModel { textures, elements, translucent_sprites })
}

fn vec3(v: Option<&Value>, what: &str) -> Result<[f32; 3]> {
    let arr = v
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("{what}: expected [x, y, z]"))?;
    if arr.len() != 3 {
        return Err(anyhow!("{what}: expected 3 components, got {}", arr.len()));
    }
    let mut out = [0f32; 3];
    for (i, c) in arr.iter().enumerate() {
        out[i] = c.as_f64().ok_or_else(|| anyhow!("{what}: non-numeric component"))? as f32;
    }
    Ok(out)
}

fn parse_elements(v: &Value) -> Result<Vec<Element>> {
    let arr = v.as_array().ok_or_else(|| anyhow!("elements is not an array"))?;
    arr.iter().map(parse_element).collect()
}

fn parse_element(v: &Value) -> Result<Element> {
    let obj = v.as_object().ok_or_else(|| anyhow!("element is not an object"))?;
    let from = vec3(obj.get("from"), "element.from")?;
    let to = vec3(obj.get("to"), "element.to")?;
    let rot = match obj.get("rotation") {
        Some(r) => Some(parse_rotation(r)?),
        None => None,
    };
    let mut faces = Vec::new();
    if let Some(fobj) = obj.get("faces").and_then(Value::as_object) {
        for (name, fv) in fobj {
            let Some(face) = Face::from_name(name) else {
                tracing::warn!("models: unknown face name {name:?}, skipping");
                continue;
            };
            faces.push((face, parse_face(fv)?));
        }
    }
    Ok(Element { from, to, rot, faces })
}

fn parse_rotation(v: &Value) -> Result<ElemRot> {
    let obj = v.as_object().ok_or_else(|| anyhow!("rotation is not an object"))?;
    let origin = match obj.get("origin") {
        Some(_) => vec3(obj.get("origin"), "rotation.origin")?,
        None => [8.0, 8.0, 8.0],
    };
    let axis = match obj.get("axis").and_then(Value::as_str) {
        Some("x") => 0,
        Some("y") => 1,
        Some("z") => 2,
        other => return Err(anyhow!("rotation.axis invalid: {other:?}")),
    };
    let angle = obj.get("angle").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let rescale = obj.get("rescale").and_then(Value::as_bool).unwrap_or(false);
    Ok(ElemRot { origin, axis, angle, rescale })
}

fn parse_face(v: &Value) -> Result<ElemFace> {
    let obj = v.as_object().ok_or_else(|| anyhow!("face is not an object"))?;
    let texture = obj
        .get("texture")
        .and_then(Value::as_str)
        .unwrap_or("#__undefined__")
        .to_owned();
    let uv = match obj.get("uv") {
        Some(u) => {
            let arr = u.as_array().ok_or_else(|| anyhow!("face.uv is not an array"))?;
            if arr.len() != 4 {
                return Err(anyhow!("face.uv: expected 4 components"));
            }
            let mut out = [0f32; 4];
            for (i, c) in arr.iter().enumerate() {
                out[i] = c.as_f64().ok_or_else(|| anyhow!("face.uv: non-numeric"))? as f32;
            }
            Some(out)
        }
        None => None,
    };
    let cullface = obj
        .get("cullface")
        .and_then(Value::as_str)
        .and_then(Face::from_name);
    let rotation = obj
        .get("rotation")
        .and_then(Value::as_i64)
        .map_or(0, |r| (r.rem_euclid(360) as u32) / 90 % 4 * 90);
    let tintindex = obj.get("tintindex").and_then(Value::as_i64).map(|t| t as i32);
    Ok(ElemFace { texture, uv, cullface, rotation, tintindex })
}

/// Vanilla default face UVs, projected from the element's from/to
/// (BlockElement.uvsByFace). Model coords 0..16, [u0, v0, u1, v1].
pub(crate) fn default_uv(face: Face, from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    let [x0, y0, z0] = from;
    let [x1, y1, z1] = to;
    match face {
        Face::Down => [x0, 16.0 - z1, x1, 16.0 - z0],
        Face::Up => [x0, z0, x1, z1],
        Face::North => [16.0 - x1, 16.0 - y1, 16.0 - x0, 16.0 - y0],
        Face::South => [x0, 16.0 - y1, x1, 16.0 - y0],
        Face::West => [z0, 16.0 - y1, z1, 16.0 - y0],
        Face::East => [16.0 - z1, 16.0 - y1, 16.0 - z0, 16.0 - y0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn props(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn variant_key_matching() {
        let p = props(&[("facing", "east"), ("half", "bottom"), ("waterlogged", "false")]);
        assert_eq!(variant_key_match("", &p), Some(0));
        assert_eq!(variant_key_match("facing=east", &p), Some(1));
        assert_eq!(variant_key_match("facing=east,half=bottom", &p), Some(2));
        assert_eq!(variant_key_match("facing=west", &p), None);
        assert_eq!(variant_key_match("facing=east,half=top", &p), None);
        assert_eq!(variant_key_match("nosuch=x", &p), None);
        assert_eq!(variant_key_match("garbage", &p), None); // malformed pair
    }

    #[test]
    fn variants_selection_most_specific() {
        let bs = json!({"variants": {
            "": {"model": "block/generic"},
            "snowy=true": {"model": "block/snowy", "y": 90.0},
            "snowy=false": [{"model": "block/plain"}, {"model": "block/plain", "y": 180}]
        }});
        let sel = select_model_refs(&bs, &props(&[("snowy", "false")])).unwrap();
        assert_eq!(sel, vec![ModelRef { model: "block/plain".into(), x: 0, y: 0 }]);
        let sel = select_model_refs(&bs, &props(&[("snowy", "true")])).unwrap();
        assert_eq!(sel, vec![ModelRef { model: "block/snowy".into(), x: 0, y: 90 }]);
        // No prop info at all → "" still matches.
        let sel = select_model_refs(&bs, &[]).unwrap();
        assert_eq!(sel[0].model, "block/generic");
    }

    #[test]
    fn variants_no_match_is_invalid() {
        let bs = json!({"variants": {"a=1": {"model": "m"}}});
        assert!(select_model_refs(&bs, &props(&[("a", "2")])).is_none());
        assert!(select_model_refs(&json!({"neither": {}}), &[]).is_none());
        assert!(select_model_refs(&json!(42), &[]).is_none());
    }

    #[test]
    fn rotation_normalization() {
        assert_eq!(norm_deg(0.0), 0);
        assert_eq!(norm_deg(90.0), 90);
        assert_eq!(norm_deg(270.0), 270);
        assert_eq!(norm_deg(-90.0), 270);
        assert_eq!(norm_deg(360.0), 0);
        assert_eq!(norm_deg(450.0), 90);
    }

    #[test]
    fn multipart_when() {
        let p = props(&[("north", "side"), ("south", "none"), ("power", "3")]);
        assert!(when_matches(&json!({"north": "side|up"}), &p));
        assert!(!when_matches(&json!({"north": "up"}), &p));
        assert!(when_matches(&json!({"north": "side", "power": 3}), &p)); // number
        assert!(!when_matches(&json!({"north": "side", "south": "side"}), &p));
        assert!(when_matches(&json!({"OR": [{"south": "side"}, {"north": "side"}]}), &p));
        assert!(!when_matches(&json!({"OR": [{"south": "side"}, {"north": "up"}]}), &p));
        assert!(when_matches(&json!({"AND": [{"north": "side"}, {"power": "3"}]}), &p));
        assert!(!when_matches(&json!({"nosuch": "true"}), &p));

        // Boolean condition values.
        let q = props(&[("up", "true")]);
        assert!(when_matches(&json!({"up": true}), &q));
        assert!(!when_matches(&json!({"up": false}), &q));
    }

    #[test]
    fn multipart_selection() {
        let bs = json!({"multipart": [
            {"apply": {"model": "base"}},
            {"when": {"up": "true"}, "apply": [{"model": "post", "x": 90}]},
            {"when": {"up": "false"}, "apply": {"model": "nope"}}
        ]});
        let sel = select_model_refs(&bs, &props(&[("up", "true")])).unwrap();
        assert_eq!(
            sel,
            vec![
                ModelRef { model: "base".into(), x: 0, y: 0 },
                ModelRef { model: "post".into(), x: 90, y: 0 },
            ]
        );
        // Nothing matches (besides nothing): valid empty selection.
        let bs = json!({"multipart": [{"when": {"up": "true"}, "apply": {"model": "m"}}]});
        let sel = select_model_refs(&bs, &props(&[("up", "false")])).unwrap();
        assert!(sel.is_empty());
    }

    #[test]
    fn parent_chain_merge_and_texture_vars() {
        let mut files: HashMap<&str, Value> = HashMap::new();
        files.insert(
            "block/leaf",
            json!({"parent": "block/leaves_base", "textures": {"all": "block/oak_leaves"}}),
        );
        files.insert(
            "block/leaves_base",
            json!({
                "parent": "block/cube_all_ish",
                "textures": {"all": "block/fallback", "extra": "#all"}
            }),
        );
        files.insert(
            "block/cube_all_ish",
            json!({"elements": [{
                "from": [0, 0, 0], "to": [16, 16, 16],
                "faces": {"up": {"texture": "#extra", "cullface": "up", "tintindex": 0}}
            }]}),
        );
        let mut load = |r: &str| {
            files
                .get(r)
                .cloned()
                .ok_or_else(|| anyhow!("no such model {r}"))
        };
        let rm = resolve_model(&mut load, "block/leaf").unwrap();
        // Child override beats the parent's own value.
        assert_eq!(rm.textures.get("all").map(String::as_str), Some("block/oak_leaves"));
        assert_eq!(rm.elements.len(), 1);
        let (face, ef) = &rm.elements[0].faces[0];
        assert_eq!(*face, Face::Up);
        assert_eq!(ef.cullface, Some(Face::Up));
        assert_eq!(ef.tintindex, Some(0));
        // #extra → #all → block/oak_leaves.
        assert_eq!(
            lookup_texture(&rm.textures, &ef.texture).as_deref(),
            Some("block/oak_leaves")
        );
        // Namespace is stripped.
        assert_eq!(
            lookup_texture(&rm.textures, "minecraft:block/stone").as_deref(),
            Some("block/stone")
        );
        // Unresolvable and cyclic refs → None.
        assert_eq!(lookup_texture(&rm.textures, "#nope"), None);
        let mut cyc = HashMap::new();
        cyc.insert("a".to_string(), "#b".to_string());
        cyc.insert("b".to_string(), "#a".to_string());
        assert_eq!(lookup_texture(&cyc, "#a"), None);
    }

    #[test]
    fn builtin_parent_resolves_empty() {
        let mut files: HashMap<&str, Value> = HashMap::new();
        files.insert(
            "item/thing",
            json!({"parent": "builtin/generated", "textures": {"layer0": "item/thing"}}),
        );
        let mut load = |r: &str| {
            files
                .get(r)
                .cloned()
                .ok_or_else(|| anyhow!("no such model {r}"))
        };
        let rm = resolve_model(&mut load, "item/thing").unwrap();
        assert!(rm.elements.is_empty());
        let rm = resolve_model(&mut load, "builtin/entity").unwrap();
        assert!(rm.elements.is_empty() && rm.textures.is_empty());
    }

    #[test]
    fn default_uv_projection() {
        let from = [2.0, 3.0, 4.0];
        let to = [10.0, 12.0, 15.0];
        assert_eq!(default_uv(Face::Up, from, to), [2.0, 4.0, 10.0, 15.0]);
        assert_eq!(default_uv(Face::Down, from, to), [2.0, 1.0, 10.0, 12.0]);
        assert_eq!(default_uv(Face::South, from, to), [2.0, 4.0, 10.0, 13.0]);
        assert_eq!(default_uv(Face::North, from, to), [6.0, 4.0, 14.0, 13.0]);
        assert_eq!(default_uv(Face::West, from, to), [4.0, 4.0, 15.0, 13.0]);
        assert_eq!(default_uv(Face::East, from, to), [1.0, 4.0, 12.0, 13.0]);
    }
}
