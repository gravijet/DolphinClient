//! State-id → block-name + properties table from the official data-generator
//! report (`blocks.json`, produced by the 26.1 server jar with `--reports`).
//! Vanilla global state ids == azalea BlockState ids == what the mesher sees.

use crate::types::StateId;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// The 26.1 blocks report, gzipped and baked into the binary so the client is
/// self-sufficient: given only the vanilla client jar (models + textures), it
/// can map every block state without an external `blocks.json`. Produced once
/// from the 26.1 server jar `--reports` output; state ids are version-fixed.
pub const EMBEDDED_BLOCKS_26_1: &[u8] = include_bytes!("../../assets/blocks-26.1.json.gz");

#[derive(Debug)]
pub struct BlockEntry {
    /// Full name, e.g. "minecraft:oak_stairs".
    pub name: String,
    /// Short name without namespace, e.g. "oak_stairs".
    pub short_name: String,
    /// Property key/value pairs, sorted by key (e.g. [("facing","north"),("half","top")]).
    pub props: Vec<(String, String)>,
}

impl BlockEntry {
    /// Value of a property, if present.
    pub fn prop(&self, key: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    pub fn is_waterlogged(&self) -> bool {
        self.prop("waterlogged") == Some("true")
    }
}

#[derive(Debug)]
pub struct BlockTable {
    /// Indexed by state id; contiguous from 0.
    entries: Vec<BlockEntry>,
    air: Vec<bool>,
}

impl BlockTable {
    /// Parse the blocks.json report (optionally gzipped — sniff magic bytes).
    /// Report shape: { "minecraft:stone": { "properties": {name:[values]}?,
    /// "states": [ { "id": N, "default": true?, "properties": {k:v}? } ] } }
    /// Load from `path` when given (plain or gzipped), otherwise from the
    /// embedded 26.1 report. This is what callers should use: the launcher
    /// downloads only the vanilla jar, so the report normally comes from here.
    pub fn load_or_embedded(path: Option<&Path>) -> Result<Self> {
        match path {
            Some(p) => {
                let file = std::fs::File::open(p)
                    .with_context(|| format!("opening blocks report {}", p.display()))?;
                Self::load(std::io::BufReader::new(file))
            }
            None => Self::load(EMBEDDED_BLOCKS_26_1).context("loading embedded 26.1 blocks report"),
        }
    }

    pub fn load(mut reader: impl Read) -> Result<Self> {
        let mut raw = Vec::new();
        reader
            .read_to_end(&mut raw)
            .context("reading blocks.json report")?;
        // Gzip magic sniff (0x1f 0x8b): the report may be stored compressed.
        let json = if raw.len() >= 2 && raw[0] == 0x1f && raw[1] == 0x8b {
            let mut out = Vec::with_capacity(raw.len().saturating_mul(8));
            flate2::read::GzDecoder::new(raw.as_slice())
                .read_to_end(&mut out)
                .context("decompressing gzipped blocks.json report")?;
            out
        } else {
            raw
        };

        let report: HashMap<String, ReportBlock> =
            serde_json::from_slice(&json).context("parsing blocks.json report")?;

        let total: usize = report.values().map(|b| b.states.len()).sum();
        if total == 0 {
            bail!("blocks.json report contains no block states");
        }
        let mut slots: Vec<Option<BlockEntry>> = Vec::new();
        slots.resize_with(total, || None);

        for (name, block) in &report {
            let short_name = name
                .split_once(':')
                .map_or(name.as_str(), |(_, s)| s)
                .to_owned();
            for state in &block.states {
                let id = state.id as usize;
                if id >= slots.len() {
                    // Out-of-range id implies a hole somewhere; keep going and
                    // report the hole below with a precise message.
                    slots.resize_with(id + 1, || None);
                }
                if slots[id].is_some() {
                    bail!("blocks.json: duplicate state id {id} (block {name})");
                }
                let mut props: Vec<(String, String)> = state
                    .properties
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                props.sort_by(|a, b| a.0.cmp(&b.0));
                slots[id] = Some(BlockEntry {
                    name: name.clone(),
                    short_name: short_name.clone(),
                    props,
                });
            }
        }

        let mut entries = Vec::with_capacity(slots.len());
        for (id, slot) in slots.into_iter().enumerate() {
            match slot {
                Some(e) => entries.push(e),
                None => bail!("blocks.json: state ids are not contiguous (hole at id {id})"),
            }
        }
        let air = entries
            .iter()
            .map(|e| matches!(e.short_name.as_str(), "air" | "cave_air" | "void_air"))
            .collect();
        Ok(Self { entries, air })
    }

    pub fn entry(&self, id: StateId) -> Option<&BlockEntry> {
        self.entries.get(id as usize)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// air / cave_air / void_air (precomputed).
    #[inline]
    pub fn is_air(&self, id: StateId) -> bool {
        self.air.get(id as usize).copied().unwrap_or(true)
    }

    /// water or lava source/flowing? Returns the fluid short name.
    pub fn fluid_kind(&self, id: StateId) -> Option<&'static str> {
        let e = self.entry(id)?;
        match e.short_name.as_str() {
            "water" | "bubble_column" => Some("water"),
            "lava" => Some("lava"),
            _ => None,
        }
    }
}

/// One block in the data-generator report. Unknown keys ("definition",
/// block-level "properties" enumerating possible values) are ignored.
#[derive(Deserialize)]
struct ReportBlock {
    #[serde(default)]
    states: Vec<ReportState>,
}

/// One concrete state. "default" and any unknown keys are ignored.
#[derive(Deserialize)]
struct ReportState {
    id: StateId,
    #[serde(default)]
    properties: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNIPPET: &str = r#"{
        "minecraft:air": {
            "definition": {"type": "minecraft:air"},
            "states": [{"id": 0, "default": true}]
        },
        "minecraft:grass_block": {
            "definition": {"type": "minecraft:grass"},
            "properties": {"snowy": ["true", "false"]},
            "states": [
                {"id": 1, "properties": {"snowy": "true"}},
                {"id": 2, "default": true, "properties": {"snowy": "false"}},
                {"future_unknown_key": 7, "id": 3, "properties": {"snowy": "maybe"}}
            ]
        },
        "minecraft:oak_stairs": {
            "states": [
                {"id": 4, "properties": {"waterlogged": "true", "half": "top", "facing": "north"}}
            ]
        }
    }"#;

    #[test]
    fn snippet_parse() {
        let t = BlockTable::load(SNIPPET.as_bytes()).expect("parse snippet");
        assert_eq!(t.len(), 5);
        assert!(!t.is_empty());

        let air = t.entry(0).expect("id 0");
        assert_eq!(air.name, "minecraft:air");
        assert_eq!(air.short_name, "air");
        assert!(air.props.is_empty());
        assert!(t.is_air(0));
        assert!(!t.is_air(1));
        // Out-of-range ids count as air (safe to skip in the mesher).
        assert!(t.is_air(999));
        assert!(t.entry(999).is_none());

        let grass = t.entry(2).expect("id 2");
        assert_eq!(grass.short_name, "grass_block");
        assert_eq!(grass.prop("snowy"), Some("false"));
        assert_eq!(grass.prop("nope"), None);

        // Props sorted by key regardless of JSON order.
        let stairs = t.entry(4).expect("id 4");
        let keys: Vec<&str> = stairs.props.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["facing", "half", "waterlogged"]);
        assert!(stairs.is_waterlogged());
        assert!(!grass.is_waterlogged());
        assert_eq!(t.fluid_kind(4), None);
    }

    #[test]
    fn gzip_sniffed() {
        use std::io::Write;
        let mut enc =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(SNIPPET.as_bytes()).unwrap();
        let gz = enc.finish().unwrap();
        assert_eq!(&gz[..2], &[0x1f, 0x8b]);
        let t = BlockTable::load(gz.as_slice()).expect("parse gzipped snippet");
        assert_eq!(t.len(), 5);
        assert_eq!(t.entry(4).unwrap().short_name, "oak_stairs");
    }

    #[test]
    fn hole_and_duplicate_rejected() {
        let hole = r#"{"minecraft:a": {"states": [{"id": 0}, {"id": 2}]}}"#;
        let err = BlockTable::load(hole.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("hole at id 1"), "{err}");

        let dup = r#"{"minecraft:a": {"states": [{"id": 0}, {"id": 0}]}}"#;
        let err = BlockTable::load(dup.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("duplicate state id 0"), "{err}");

        let empty = r#"{}"#;
        assert!(BlockTable::load(empty.as_bytes()).is_err());
    }

    const REPORT: &str =
        "/home/benj/DolphinClient/.mc-cache/server/generated/reports/blocks.json";

    /// The report baked into the binary must be complete and valid on its own —
    /// this is what ships, so the client is self-sufficient given just the jar.
    #[test]
    fn embedded_report_smoke() {
        let t = BlockTable::load(EMBEDDED_BLOCKS_26_1).expect("parse embedded report");
        assert_eq!(t.len(), 29873, "embedded 26.1 report has 29873 states");
        assert!(t.is_air(0));
        assert_eq!(t.entry(0).unwrap().name, "minecraft:air");
        let stone = (0..t.len() as StateId)
            .find(|&i| t.entry(i).unwrap().short_name == "stone")
            .expect("stone present in embedded report");
        assert_eq!(t.entry(stone).unwrap().name, "minecraft:stone");
    }

    #[test]
    fn real_report_smoke() {
        let path = std::path::Path::new(REPORT);
        if !path.exists() {
            eprintln!("skipping real_report_smoke: {REPORT} not present");
            return;
        }
        let file = std::fs::File::open(path).expect("open blocks.json");
        let t = BlockTable::load(std::io::BufReader::new(file)).expect("parse real report");
        assert_eq!(t.len(), 29873, "26.1 report has 29873 states");

        // id 0 is air in every vanilla version.
        assert!(t.is_air(0));
        assert_eq!(t.entry(0).unwrap().name, "minecraft:air");

        // Find stone and water by name and sanity-check them.
        let stone_id = (0..t.len() as StateId)
            .find(|&i| t.entry(i).unwrap().short_name == "stone")
            .expect("stone present");
        assert!(!t.is_air(stone_id));
        assert_eq!(t.entry(stone_id).unwrap().name, "minecraft:stone");

        let water_id = (0..t.len() as StateId)
            .find(|&i| t.entry(i).unwrap().short_name == "water")
            .expect("water present");
        assert_eq!(t.fluid_kind(water_id), Some("water"));
        assert_eq!(t.entry(water_id).unwrap().prop("level"), Some("0"));

        // Every entry's props are sorted by key.
        for i in 0..t.len() as StateId {
            let e = t.entry(i).unwrap();
            assert!(
                e.props.windows(2).all(|w| w[0].0 <= w[1].0),
                "props unsorted for id {i} ({})",
                e.name
            );
        }
    }
}
