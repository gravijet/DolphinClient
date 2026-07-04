//! Pure conversion helpers for the bridge: azalea world/packet data → plain
//! `SectionData` pieces. No I/O, no locks — unit-testable.
//!
//! Layout facts (verified against azalea 0.16 + vanilla protocol):
//! - Block palette storage order is `y*256 + z*16 + x` — identical to our YZX
//!   `SectionData.blocks` layout, so entries copy through index-for-index.
//! - Biome palette is 4×4×4, order `y*16 + z*4 + x` — same YZX property.
//! - Light nibble arrays are the raw vanilla wire format: 2048 bytes,
//!   nibble i = block index i (YZX), low nibble first. We pass them through.
//! - Light masks: bit `i` covers light-section `i`, which is world section
//!   `i - 1` (bit 0 is the padding section *below* the world, the last bit is
//!   above it). The k-th nibble array pairs with the k-th set mask bit.

use azalea::block::BlockState;
use azalea::core::bitset::BitSet;
use azalea::protocol::packets::game::c_light_update::ClientboundLightUpdatePacketData;
use azalea::registry::DataRegistry as _;
use azalea::world::Section;
use azalea::world::palette::Palette;
use tracing::warn;

use crate::types::{SECTION_VOLUME, StateId};

/// Light for one section, as captured from packets. `None` = never received.
#[derive(Default, Clone)]
pub struct SectionLight {
    pub sky: Option<Box<[u8; 2048]>>,
    pub block: Option<Box<[u8; 2048]>>,
}

/// Light for one chunk column: one entry per world section, bottom-up.
#[derive(Default, Clone)]
pub struct ChunkLight {
    pub sections: Vec<SectionLight>,
}

/// Merge a `LevelChunkWithLight`/`LightUpdate` payload into `light`.
///
/// `section_count` is the world's section count (`height / 16`, 24 for the
/// overworld). Returns the (sorted, deduped) world-section indices whose light
/// changed. Sections whose mask bits are all clear keep their previous data.
pub fn apply_light_data(
    light: &mut ChunkLight,
    section_count: usize,
    data: &ClientboundLightUpdatePacketData,
) -> Vec<usize> {
    light.sections.resize_with(section_count, SectionLight::default);
    let mut changed = Vec::new();
    merge_channel(
        &mut light.sections,
        section_count,
        &data.sky_y_mask,
        &data.empty_sky_y_mask,
        &data.sky_updates,
        |s| &mut s.sky,
        &mut changed,
    );
    merge_channel(
        &mut light.sections,
        section_count,
        &data.block_y_mask,
        &data.empty_block_y_mask,
        &data.block_updates,
        |s| &mut s.block,
        &mut changed,
    );
    changed.sort_unstable();
    changed.dedup();
    changed
}

/// Merge one light channel (sky or block) per the vanilla mask semantics.
fn merge_channel(
    sections: &mut [SectionLight],
    section_count: usize,
    mask: &BitSet,
    empty_mask: &BitSet,
    updates: &[Box<[u8]>],
    pick: impl Fn(&mut SectionLight) -> &mut Option<Box<[u8; 2048]>>,
    changed: &mut Vec<usize>,
) {
    let mut k = 0usize; // index into `updates`, advances per set mask bit
    for li in 0..section_count + 2 {
        let new: Box<[u8; 2048]> = if mask.get(li).unwrap_or(false) {
            let arr = updates.get(k);
            k += 1;
            match arr {
                Some(a) if a.len() == 2048 => {
                    let mut out = Box::new([0u8; 2048]);
                    out.copy_from_slice(a);
                    out
                }
                Some(a) => {
                    warn!(len = a.len(), li, "bridge: light nibble array has wrong length; skipping");
                    continue;
                }
                None => {
                    warn!(li, "bridge: light mask has more set bits than arrays; skipping rest");
                    continue;
                }
            }
        } else if empty_mask.get(li).unwrap_or(false) {
            Box::new([0u8; 2048]) // explicitly all-zero section
        } else {
            continue; // unchanged
        };
        // Light-section li covers world section li - 1; drop the padding
        // sections below (li == 0) and above (li == section_count + 1).
        let Some(i) = li.checked_sub(1) else { continue };
        if i >= section_count {
            continue;
        }
        if let Some(sec) = sections.get_mut(i) {
            *pick(sec) = Some(new);
            changed.push(i);
        }
    }
}

/// Decode all 4096 block states of a section into a flat YZX StateId array.
pub fn copy_section_blocks(section: &Section) -> Box<[StateId; SECTION_VOLUME]> {
    let mut out: Box<[StateId; SECTION_VOLUME]> = vec![0 as StateId; SECTION_VOLUME]
        .into_boxed_slice()
        .try_into()
        .expect("SECTION_VOLUME-length vec");
    match &section.states.palette {
        Palette::SingleValue(v) => out.fill(v.id() as StateId),
        Palette::Linear(vals) | Palette::Hashmap(vals) => {
            for (i, id) in section.states.storage.iter().enumerate() {
                if i >= SECTION_VOLUME {
                    warn!("bridge: block storage longer than 4096; truncating");
                    break;
                }
                out[i] = vals
                    .get(id as usize)
                    .map(|s| s.id() as StateId)
                    .unwrap_or_else(|| {
                        warn!(id, "bridge: block palette id out of range; using air");
                        0
                    });
            }
        }
        Palette::Global => {
            for (i, id) in section.states.storage.iter().enumerate() {
                if i >= SECTION_VOLUME {
                    break;
                }
                // Validate against the registry range; invalid → air.
                out[i] = BlockState::try_from(id as u32)
                    .map(|s| s.id() as StateId)
                    .unwrap_or(0);
            }
        }
    }
    out
}

/// Decode the 4×4×4 biome grid of a section into flat YZX protocol ids.
pub fn copy_section_biomes(section: &Section) -> Box<[u32; 64]> {
    let mut out: Box<[u32; 64]> = Box::new([0u32; 64]);
    match &section.biomes.palette {
        Palette::SingleValue(b) => out.fill(b.protocol_id()),
        Palette::Linear(vals) | Palette::Hashmap(vals) => {
            for (i, id) in section.biomes.storage.iter().enumerate() {
                if i >= 64 {
                    warn!("bridge: biome storage longer than 64; truncating");
                    break;
                }
                out[i] = vals.get(id as usize).map(|b| b.protocol_id()).unwrap_or(0);
            }
        }
        Palette::Global => {
            for (i, id) in section.biomes.storage.iter().enumerate() {
                if i >= 64 {
                    break;
                }
                out[i] = id as u32;
            }
        }
    }
    out
}

/// Flatten formatted text to plain text: drop legacy `§x` codes (and a
/// dangling trailing `§`).
pub fn strip_legacy_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next(); // swallow the code char (may be None at end)
        } else {
            out.push(c);
        }
    }
    out
}

/// Map a 26.1 SetTime clock state to the app convention:
/// ticks 0..24000; negated when the daylight cycle is frozen (rate == 0).
pub fn time_of_day(total_ticks: u64, rate: f32) -> i64 {
    let t = (total_ticks % 24000) as i64;
    if rate == 0.0 { -t } else { t }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use azalea::registry::data::Biome;
    use azalea::world::BitStorage;
    use azalea::world::palette::PalettedContainer;

    use super::*;

    #[test]
    fn strip_codes() {
        assert_eq!(strip_legacy_codes("§aHello §lWorld§r!"), "Hello World!");
        assert_eq!(strip_legacy_codes("no codes"), "no codes");
        assert_eq!(strip_legacy_codes("dangling§"), "dangling");
        assert_eq!(strip_legacy_codes(""), "");
    }

    #[test]
    fn time_of_day_mapping() {
        assert_eq!(time_of_day(0, 1.0), 0);
        assert_eq!(time_of_day(24000 + 137, 1.0), 137);
        assert_eq!(time_of_day(6000, 0.0), -6000);
    }

    fn boxed(fill: u8) -> Box<[u8]> {
        vec![fill; 2048].into_boxed_slice()
    }

    #[test]
    fn light_masks_map_to_world_sections() {
        // 4 world sections → 6 light sections (one below, one above).
        let mut sky_mask = BitSet::new(6);
        sky_mask.set(0); // below the world: consumed but dropped
        sky_mask.set(2); // world section 1
        let mut empty_sky = BitSet::new(6);
        empty_sky.set(3); // world section 2: explicit zeros
        let mut block_mask = BitSet::new(6);
        block_mask.set(5); // above the world: consumed but dropped
        let data = ClientboundLightUpdatePacketData {
            sky_y_mask: sky_mask,
            block_y_mask: block_mask,
            empty_sky_y_mask: empty_sky,
            empty_block_y_mask: BitSet::new(6),
            sky_updates: Arc::new(vec![boxed(0x11), boxed(0x22)].into_boxed_slice()),
            block_updates: Arc::new(vec![boxed(0x33)].into_boxed_slice()),
        };

        let mut light = ChunkLight::default();
        let changed = apply_light_data(&mut light, 4, &data);
        assert_eq!(changed, vec![1, 2]);
        assert_eq!(light.sections.len(), 4);
        assert!(light.sections[0].sky.is_none()); // untouched
        let s1 = light.sections[1].sky.as_ref().expect("section 1 sky");
        assert!(s1.iter().all(|&b| b == 0x22)); // 2nd array → 2nd set bit
        let s2 = light.sections[2].sky.as_ref().expect("section 2 sky");
        assert!(s2.iter().all(|&b| b == 0));
        assert!(light.sections.iter().all(|s| s.block.is_none())); // out-of-world block array dropped
    }

    #[test]
    fn light_update_merges_over_existing() {
        let mut light = ChunkLight::default();
        light.sections.resize_with(2, SectionLight::default);
        light.sections[0].sky = Some(Box::new([0xAA; 2048]));
        light.sections[1].sky = Some(Box::new([0xBB; 2048]));

        // Update only world section 1 (light-section 2).
        let mut sky_mask = BitSet::new(4);
        sky_mask.set(2);
        let data = ClientboundLightUpdatePacketData {
            sky_y_mask: sky_mask,
            block_y_mask: BitSet::new(4),
            empty_sky_y_mask: BitSet::new(4),
            empty_block_y_mask: BitSet::new(4),
            sky_updates: Arc::new(vec![boxed(0xCC)].into_boxed_slice()),
            block_updates: Arc::new(Box::new([])),
        };
        let changed = apply_light_data(&mut light, 2, &data);
        assert_eq!(changed, vec![1]);
        assert!(light.sections[0].sky.as_ref().unwrap().iter().all(|&b| b == 0xAA));
        assert!(light.sections[1].sky.as_ref().unwrap().iter().all(|&b| b == 0xCC));
    }

    #[test]
    fn malformed_light_is_skipped_not_panicked() {
        let mut mask = BitSet::new(4);
        mask.set(1);
        mask.set(2);
        let data = ClientboundLightUpdatePacketData {
            sky_y_mask: mask,
            block_y_mask: BitSet::new(4),
            empty_sky_y_mask: BitSet::new(4),
            empty_block_y_mask: BitSet::new(4),
            // wrong length for the first array, missing the second entirely
            sky_updates: Arc::new(vec![vec![1u8; 100].into_boxed_slice()].into_boxed_slice()),
            block_updates: Arc::new(Box::new([])),
        };
        let mut light = ChunkLight::default();
        let changed = apply_light_data(&mut light, 2, &data);
        assert!(changed.is_empty());
        assert!(light.sections.iter().all(|s| s.sky.is_none()));
    }

    /// Build a section: Linear block palette with one non-air block placed at
    /// (x=1, y=2, z=3), single-value biome.
    fn test_section() -> Section {
        let stone = BlockState::try_from(1u32).expect("state id 1 exists");
        let mut storage = BitStorage::new(4, SECTION_VOLUME, None).expect("4-bit storage");
        let idx = 2 * 256 + 3 * 16 + 1; // y*256 + z*16 + x
        storage.set(idx, 1); // palette index 1 = stone
        let states = PalettedContainer::<BlockState> {
            bits_per_entry: 4,
            palette: Palette::Linear(vec![BlockState::AIR, stone]),
            storage,
        };
        let biomes = PalettedContainer::<Biome> {
            bits_per_entry: 0,
            palette: Palette::SingleValue(Biome::new_raw(7)),
            storage: BitStorage::new(0, 64, Some(Box::new([]))).expect("0-bit storage"),
        };
        Section { block_count: 1, fluid_count: 0, states, biomes }
    }

    #[test]
    fn block_copy_is_yzx() {
        let section = test_section();
        let blocks = copy_section_blocks(&section);
        let expected_idx = crate::types::BlockPos { x: 1, y: 2, z: 3 }.section_index();
        let stone_id = BlockState::try_from(1u32).unwrap().id() as StateId;
        assert_eq!(blocks[expected_idx], stone_id);
        assert_eq!(blocks.iter().filter(|&&b| b != 0).count(), 1);
    }

    #[test]
    fn single_value_palettes() {
        let mut section = test_section();
        let stone = BlockState::try_from(1u32).unwrap();
        section.states = PalettedContainer::<BlockState> {
            bits_per_entry: 0,
            palette: Palette::SingleValue(stone),
            storage: BitStorage::new(0, SECTION_VOLUME, Some(Box::new([]))).unwrap(),
        };
        let blocks = copy_section_blocks(&section);
        assert!(blocks.iter().all(|&b| b == stone.id() as StateId));
        let biomes = copy_section_biomes(&section);
        assert!(biomes.iter().all(|&b| b == 7));
    }

    #[test]
    fn linear_biome_copy_is_yzx() {
        let mut section = test_section();
        let mut storage = BitStorage::new(1, 64, None).expect("1-bit storage");
        let idx = 2 * 16 + 3 * 4 + 1; // y*16 + z*4 + x for (1, 2, 3)
        storage.set(idx, 1);
        section.biomes = PalettedContainer::<Biome> {
            bits_per_entry: 1,
            palette: Palette::Linear(vec![Biome::new_raw(0), Biome::new_raw(42)]),
            storage,
        };
        let biomes = copy_section_biomes(&section);
        assert_eq!(biomes[idx], 42);
        assert_eq!(biomes.iter().filter(|&&b| b != 0).count(), 1);
    }
}
