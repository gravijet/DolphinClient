//! Decode the block-entity NBT the server sends into the small, renderable
//! [`BlockEntityData`] payloads the app draws.
//!
//! Only the kinds that actually change what a block looks like are decoded —
//! sign text, banner patterns, head profiles, pot sherds and campfire items.
//! Everything else (chests' contents, spawner data, …) is dropped on the floor:
//! the block model already covers it.
//!
//! Text fields are the awkward part. A sign line is a chat component, and the
//! server may send it either as a JSON string (the pre-component wire form many
//! plugins still write) or as real NBT. Both are handled: strings go through
//! serde, NBT is re-encoded and read back through azalea's borrow-mode
//! `FormattedText`.

use std::io::Cursor;

use azalea::registry::builtin::BlockEntityKind;
use azalea_chat::FormattedText;
use simdnbt::owned::{Nbt, NbtCompound, NbtTag};

use super::events::{BlockEntityData, ChatSpan, SignFace};
use super::text::spans_of;

/// Vanilla dye colour names in id order (white = 0 … black = 15).
pub const DYE_NAMES: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray",
    "cyan", "purple", "blue", "brown", "green", "red", "black",
];

/// Dye colour name → vanilla id, or `None` for an unknown name.
pub fn dye_id(name: &str) -> Option<u8> {
    DYE_NAMES.iter().position(|n| *n == name).map(|i| i as u8)
}

/// Strip a `minecraft:` namespace (block entities spell registry ids out).
fn short(id: &str) -> String {
    id.strip_prefix("minecraft:").unwrap_or(id).to_owned()
}

/// Decode one block entity, or `None` if this kind draws nothing extra.
pub fn decode(kind: BlockEntityKind, nbt: &Nbt) -> Option<BlockEntityData> {
    let Nbt::Some(base) = nbt else { return None };
    let c: &NbtCompound = base;
    match kind {
        BlockEntityKind::Sign | BlockEntityKind::HangingSign => Some(BlockEntityData::Sign {
            front: sign_face(c.compound("front_text")),
            back: sign_face(c.compound("back_text")),
        }),
        BlockEntityKind::Banner => Some(BlockEntityData::Banner { layers: banner_layers(c) }),
        BlockEntityKind::Skull => {
            let (texture_url, owner) = skull_profile(c);
            Some(BlockEntityData::Skull { texture_url, owner })
        }
        BlockEntityKind::DecoratedPot => {
            Some(BlockEntityData::DecoratedPot { sherds: pot_sherds(c) })
        }
        BlockEntityKind::Campfire => Some(BlockEntityData::Campfire { items: campfire(c) }),
        // Data-free markers: the chunk packet is simply how we learn where
        // these are, since neither has a visible block model.
        BlockEntityKind::Bell => Some(BlockEntityData::Bell),
        BlockEntityKind::Conduit => Some(BlockEntityData::Conduit),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Signs
// ---------------------------------------------------------------------------

/// One `front_text`/`back_text` compound: four message lines, the dye colour
/// and the glowing-ink flag. A missing compound is a blank black side.
fn sign_face(c: Option<&NbtCompound>) -> SignFace {
    let mut face = SignFace { color: "black".to_owned(), ..SignFace::default() };
    let Some(c) = c else { return face };
    if let Some(col) = c.string("color") {
        face.color = short(&col.to_string());
    }
    face.glowing = c.byte("has_glowing_text").is_some_and(|b| b != 0);
    if let Some(list) = c.list("messages") {
        for (i, tag) in list.as_nbt_tags().into_iter().take(4).enumerate() {
            face.lines[i] = text_spans(&tag);
        }
    }
    face
}

/// A chat component stored in NBT → styled spans. Accepts both wire forms and
/// falls back to the raw text so a line is never silently lost.
fn text_spans(tag: &NbtTag) -> Vec<ChatSpan> {
    if let Some(s) = tag.string() {
        let raw = s.to_string();
        // Cheap pre-check: only text that *looks* like a component is worth
        // handing to serde. Plugins also write bare strings.
        let looks_json = raw.starts_with('{') || raw.starts_with('[') || raw.starts_with('"');
        if looks_json && let Ok(ft) = serde_json::from_str::<FormattedText>(&raw) {
            return spans_of(&ft);
        }
        if raw.is_empty() {
            return Vec::new();
        }
        return vec![ChatSpan::plain(raw)];
    }
    // Real NBT: round-trip it through a one-key compound so azalea's
    // borrow-mode reader (the only one it implements) can see it.
    let wrapper = NbtCompound::from_values(vec![("t".into(), tag.clone())]);
    let mut buf = Vec::new();
    wrapper.write(&mut buf);
    let Ok(base) = simdnbt::borrow::read_compound(&mut Cursor::new(&buf[..])) else {
        return Vec::new();
    };
    let borrowed = simdnbt::borrow::NbtCompound::from(&base);
    let Some(t) = borrowed.get("t") else { return Vec::new() };
    match <FormattedText as simdnbt::FromNbtTag>::from_nbt_tag(t) {
        Some(ft) => spans_of(&ft),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Banners
// ---------------------------------------------------------------------------

/// `patterns: [{pattern: "minecraft:stripe_bottom", color: "red"}, …]`, in
/// paint order. A pattern may also be an inline definition with its own
/// `asset_id`, which is what data-pack banners use.
fn banner_layers(c: &NbtCompound) -> Vec<(String, u8)> {
    let Some(list) = c.list("patterns") else { return Vec::new() };
    let Some(entries) = list.compounds() else { return Vec::new() };
    let mut out = Vec::new();
    // Vanilla caps a banner at 6 layers; a hostile server could send more.
    for e in entries.iter().take(6) {
        let Some(color) = e.string("color").and_then(|s| dye_id(&short(&s.to_string()))) else {
            continue;
        };
        let asset = match e.get("pattern") {
            Some(NbtTag::String(s)) => short(&s.to_string()),
            // Inline pattern definition: {asset_id: "...", translation_key: ...}
            Some(NbtTag::Compound(inner)) => match inner.string("asset_id") {
                Some(s) => short(&s.to_string()),
                None => continue,
            },
            _ => continue,
        };
        out.push((asset, color));
    }
    out
}

// ---------------------------------------------------------------------------
// Heads, pots, campfires
// ---------------------------------------------------------------------------

/// A player head's `profile`: the skin URL out of its `textures` property and
/// the owner's name. Modern servers send a compound; very old data sends just
/// the name as a string.
fn skull_profile(c: &NbtCompound) -> (Option<String>, Option<String>) {
    let Some(tag) = c.get("profile") else { return (None, None) };
    let profile = match tag {
        NbtTag::String(s) => return (None, Some(s.to_string())),
        NbtTag::Compound(p) => p,
        _ => return (None, None),
    };
    let owner = profile.string("name").map(|s| s.to_string());
    let url = profile
        .list("properties")
        .and_then(|l| l.compounds().map(|c| c.to_vec()))
        .into_iter()
        .flatten()
        .find(|p| p.string("name").is_some_and(|n| n.to_string() == "textures"))
        .and_then(|p| p.string("value").map(|v| v.to_string()))
        .and_then(|b64| skin_url(&b64));
    (url, owner)
}

/// A profile's base64 `textures` property → the SKIN url inside it.
fn skin_url(b64: &str) -> Option<String> {
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    Some(json.get("textures")?.get("SKIN")?.get("url")?.as_str()?.to_owned())
}

/// `sherds: ["minecraft:brick", "minecraft:angler_pottery_sherd", …]` — four
/// entries in vanilla's back/left/right/front order. Plain `brick` sides carry
/// no decoration and become `None`.
fn pot_sherds(c: &NbtCompound) -> [Option<String>; 4] {
    let mut out: [Option<String>; 4] = Default::default();
    let Some(list) = c.list("sherds") else { return out };
    let Some(names) = list.strings() else { return out };
    for (i, s) in names.iter().take(4).enumerate() {
        let name = short(&s.to_string());
        if name == "brick" {
            continue;
        }
        // The item is `<x>_pottery_sherd`; the texture is `<x>_pottery_pattern`.
        let base = name.strip_suffix("_pottery_sherd").unwrap_or(&name);
        out[i] = Some(format!("{base}_pottery_pattern"));
    }
    out
}

/// `Items: [{Slot: 0b, id: "minecraft:porkchop", …}]` — up to four items
/// cooking on a campfire, placed by their slot index.
fn campfire(c: &NbtCompound) -> [Option<String>; 4] {
    let mut out: [Option<String>; 4] = Default::default();
    let Some(list) = c.list("Items") else { return out };
    let Some(items) = list.compounds() else { return out };
    for it in items {
        let slot = it.byte("Slot").unwrap_or(0).clamp(0, 3) as usize;
        if let Some(id) = it.string("id") {
            out[slot] = Some(short(&id.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compound(values: Vec<(&str, NbtTag)>) -> NbtCompound {
        NbtCompound::from_values(values.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    #[test]
    fn sign_reads_plain_and_json_lines() {
        let messages = simdnbt::owned::NbtList::String(vec![
            "\"hello\"".into(),
            "{\"text\":\"world\",\"color\":\"red\"}".into(),
            "".into(),
            "plain".into(),
        ]);
        let front = compound(vec![
            ("messages", NbtTag::List(messages)),
            ("color", NbtTag::String("lime".into())),
            ("has_glowing_text", NbtTag::Byte(1)),
        ]);
        let face = sign_face(Some(&front));
        assert_eq!(face.color, "lime");
        assert!(face.glowing);
        assert_eq!(face.lines[0][0].text, "hello");
        assert_eq!(face.lines[1][0].text, "world");
        assert_eq!(face.lines[1][0].color, Some([0xFF, 0x55, 0x55]));
        assert!(face.lines[2].is_empty());
        assert_eq!(face.lines[3][0].text, "plain");
    }

    #[test]
    fn banner_layers_read_pattern_and_colour() {
        let layer = compound(vec![
            ("pattern", NbtTag::String("minecraft:stripe_bottom".into())),
            ("color", NbtTag::String("red".into())),
        ]);
        let inline = compound(vec![
            ("pattern", NbtTag::Compound(compound(vec![("asset_id", NbtTag::String("globe".into()))]))),
            ("color", NbtTag::String("white".into())),
        ]);
        let c = compound(vec![(
            "patterns",
            NbtTag::List(simdnbt::owned::NbtList::Compound(vec![layer, inline])),
        )]);
        assert_eq!(
            banner_layers(&c),
            vec![("stripe_bottom".to_owned(), 14), ("globe".to_owned(), 0)]
        );
    }

    #[test]
    fn pot_sherds_skip_plain_bricks() {
        let c = compound(vec![(
            "sherds",
            NbtTag::List(simdnbt::owned::NbtList::String(vec![
                "minecraft:brick".into(),
                "minecraft:angler_pottery_sherd".into(),
                "minecraft:brick".into(),
                "minecraft:heart_pottery_sherd".into(),
            ])),
        )]);
        assert_eq!(
            pot_sherds(&c),
            [
                None,
                Some("angler_pottery_pattern".to_owned()),
                None,
                Some("heart_pottery_pattern".to_owned()),
            ]
        );
    }

    #[test]
    fn data_free_kinds_still_produce_a_marker() {
        // Bells and conduits carry no NBT worth reading, but the chunk packet
        // is the only place we learn where they are.
        let empty = Nbt::Some(simdnbt::owned::BaseNbt::new("", NbtCompound::new()));
        assert_eq!(decode(BlockEntityKind::Bell, &empty), Some(BlockEntityData::Bell));
        assert_eq!(decode(BlockEntityKind::Conduit, &empty), Some(BlockEntityData::Conduit));
        // Kinds the block model already draws are dropped on the floor.
        assert_eq!(decode(BlockEntityKind::Chest, &empty), None);
        assert_eq!(decode(BlockEntityKind::Furnace, &empty), None);
        // No NBT at all is never a block entity.
        assert_eq!(decode(BlockEntityKind::Bell, &Nbt::None), None);
    }

    #[test]
    fn campfire_items_land_in_their_slots() {
        let item = |slot: i8, id: &str| {
            compound(vec![("Slot", NbtTag::Byte(slot)), ("id", NbtTag::String(id.into()))])
        };
        let c = compound(vec![(
            "Items",
            NbtTag::List(simdnbt::owned::NbtList::Compound(vec![
                item(2, "minecraft:porkchop"),
                item(0, "minecraft:potato"),
            ])),
        )]);
        assert_eq!(
            campfire(&c),
            [Some("potato".to_owned()), None, Some("porkchop".to_owned()), None]
        );
    }

    #[test]
    fn dye_ids_round_trip() {
        assert_eq!(dye_id("white"), Some(0));
        assert_eq!(dye_id("black"), Some(15));
        assert_eq!(dye_id("mauve"), None);
    }
}
