//! The Statistics screen, filled from the server.
//!
//! Vanilla asks the server for the player's statistics whenever the screen
//! opens and lays them out in three tabs: General (the custom counters, each
//! formatted its own way — distances in metres, times in hours, damage in
//! hearts), Items (a table of mined/crafted/used/broken/picked-up/dropped) and
//! Mobs (what you killed and what killed you).

use std::collections::BTreeMap;

use crate::bridge::events::StatEntry;

/// How one custom statistic's raw integer should read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatFormat {
    /// A plain count.
    Count,
    /// Centimetres travelled.
    Distance,
    /// Ticks elapsed.
    Time,
    /// Tenths of a half-heart.
    Damage,
}

/// Vanilla registers a formatter per custom statistic; these are its rules.
pub fn format_of(key: &str) -> StatFormat {
    if key.ends_with("_one_cm") {
        StatFormat::Distance
    } else if matches!(
        key,
        "play_time" | "sneak_time" | "time_since_death" | "time_since_rest" | "total_world_time"
    ) {
        StatFormat::Time
    } else if key.starts_with("damage_") {
        StatFormat::Damage
    } else {
        StatFormat::Count
    }
}

/// Render one statistic the way vanilla's `StatFormatter` would.
pub fn format_value(key: &str, value: i32) -> String {
    let v = value as f64;
    match format_of(key) {
        StatFormat::Count => group_digits(value),
        StatFormat::Damage => format!("{:.1}", v / 10.0),
        StatFormat::Distance => {
            let metres = v / 100.0;
            let km = metres / 1000.0;
            if km > 0.5 {
                format!("{km:.2} km")
            } else if metres > 0.5 {
                format!("{metres:.1} m")
            } else {
                format!("{value} cm")
            }
        }
        StatFormat::Time => {
            let seconds = v / 20.0;
            let minutes = seconds / 60.0;
            let hours = minutes / 60.0;
            let days = hours / 24.0;
            let years = days / 365.0;
            if years > 0.5 {
                format!("{years:.1} y")
            } else if days > 0.5 {
                format!("{days:.1} d")
            } else if hours > 0.5 {
                format!("{hours:.1} h")
            } else if minutes > 0.5 {
                format!("{minutes:.1} m")
            } else {
                format!("{seconds:.0} s")
            }
        }
    }
}

/// `1234567` → `1,234,567`, like vanilla's number format.
fn group_digits(value: i32) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if negative { format!("-{out}") } else { out }
}

/// One row of the Items table: an item and its six counters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemRow {
    pub item: String,
    pub mined: i32,
    pub crafted: i32,
    pub used: i32,
    pub broken: i32,
    pub picked_up: i32,
    pub dropped: i32,
}

impl ItemRow {
    fn total(&self) -> i64 {
        self.mined as i64
            + self.crafted as i64
            + self.used as i64
            + self.broken as i64
            + self.picked_up as i64
            + self.dropped as i64
    }
}

/// One row of the Mobs tab.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MobRow {
    pub entity: String,
    pub killed: i32,
    pub killed_by: i32,
}

/// The server's statistics, sorted into the three tabs vanilla shows.
#[derive(Default)]
pub struct Statistics {
    /// Custom counters in the order the General tab shows them (biggest first).
    pub general: Vec<(String, i32)>,
    pub items: Vec<ItemRow>,
    pub mobs: Vec<MobRow>,
    /// The server has answered at least once.
    pub received: bool,
}

impl Statistics {
    pub fn is_empty(&self) -> bool {
        self.general.is_empty() && self.items.is_empty() && self.mobs.is_empty()
    }

    /// Sort a flat `AwardStats` reply into the three tabs.
    pub fn apply(&mut self, entries: &[StatEntry]) {
        self.received = true;
        self.general.clear();
        let mut items: BTreeMap<String, ItemRow> = BTreeMap::new();
        let mut mobs: BTreeMap<String, MobRow> = BTreeMap::new();
        for e in entries {
            match e.category {
                "custom" => self.general.push((e.key.clone(), e.value)),
                "killed" | "killed_by" => {
                    let row = mobs.entry(e.key.clone()).or_insert_with(|| MobRow {
                        entity: e.key.clone(),
                        ..Default::default()
                    });
                    if e.category == "killed" {
                        row.killed = e.value;
                    } else {
                        row.killed_by = e.value;
                    }
                }
                other => {
                    let row = items.entry(e.key.clone()).or_insert_with(|| ItemRow {
                        item: e.key.clone(),
                        ..Default::default()
                    });
                    match other {
                        "mined" => row.mined = e.value,
                        "crafted" => row.crafted = e.value,
                        "used" => row.used = e.value,
                        "broken" => row.broken = e.value,
                        "picked_up" => row.picked_up = e.value,
                        "dropped" => row.dropped = e.value,
                        _ => {}
                    }
                }
            }
        }
        // Vanilla's General tab is alphabetical by translated name; the closest
        // stable thing without translating first is the registry key.
        self.general.sort_by(|a, b| a.0.cmp(&b.0));
        self.items = items.into_values().collect();
        self.items.sort_by(|a, b| b.total().cmp(&a.total()).then(a.item.cmp(&b.item)));
        self.mobs = mobs.into_values().collect();
        self.mobs.sort_by(|a, b| {
            (b.killed + b.killed_by).cmp(&(a.killed + a.killed_by)).then(a.entity.cmp(&b.entity))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(category: &'static str, key: &str, value: i32) -> StatEntry {
        StatEntry { category, key: key.to_string(), value }
    }

    #[test]
    fn distances_read_in_metres_and_kilometres() {
        assert_eq!(format_value("walk_one_cm", 30), "30 cm");
        assert_eq!(format_value("walk_one_cm", 12_345), "123.5 m");
        assert_eq!(format_value("walk_one_cm", 250_000), "2.50 km");
    }

    #[test]
    fn times_read_in_the_biggest_useful_unit() {
        // 20 ticks = 1 second.
        assert_eq!(format_value("play_time", 200), "10 s");
        assert_eq!(format_value("play_time", 20 * 60 * 5), "5.0 m");
        assert_eq!(format_value("play_time", 20 * 60 * 60 * 3), "3.0 h");
        assert_eq!(format_value("play_time", 20 * 60 * 60 * 24 * 4), "4.0 d");
    }

    #[test]
    fn damage_is_tenths_of_a_half_heart() {
        assert_eq!(format_value("damage_dealt", 155), "15.5");
    }

    #[test]
    fn plain_counts_get_thousands_separators() {
        assert_eq!(format_value("jump", 7), "7");
        assert_eq!(format_value("jump", 1234), "1,234");
        assert_eq!(format_value("jump", 1234567), "1,234,567");
    }

    #[test]
    fn the_three_tabs_get_the_right_rows() {
        let mut stats = Statistics::default();
        stats.apply(&[
            entry("custom", "jump", 12),
            entry("mined", "stone", 40),
            entry("crafted", "stone", 2),
            entry("killed", "zombie", 3),
            entry("killed_by", "zombie", 1),
            entry("picked_up", "dirt", 9),
        ]);
        assert_eq!(stats.general, vec![("jump".to_string(), 12)]);
        assert_eq!(stats.items.len(), 2);
        let stone = stats.items.iter().find(|r| r.item == "stone").unwrap();
        assert_eq!((stone.mined, stone.crafted), (40, 2));
        assert_eq!(stats.mobs, vec![MobRow {
            entity: "zombie".into(),
            killed: 3,
            killed_by: 1
        }]);
    }

    #[test]
    fn the_busiest_item_comes_first() {
        let mut stats = Statistics::default();
        stats.apply(&[entry("mined", "dirt", 5), entry("mined", "stone", 500)]);
        assert_eq!(stats.items[0].item, "stone");
    }

    #[test]
    fn a_second_reply_replaces_the_first() {
        let mut stats = Statistics::default();
        stats.apply(&[entry("custom", "jump", 1)]);
        stats.apply(&[entry("custom", "jump", 2)]);
        assert_eq!(stats.general, vec![("jump".to_string(), 2)]);
    }
}
