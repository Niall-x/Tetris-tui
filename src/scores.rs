//! Per-mode high-score tables, persisted as hand-editable TOML.
//!
//! The two modes' scores are not comparable — NES tops out around a quarter of a
//! million where modern scoring runs an order of magnitude higher, and they reward
//! entirely different play — so they are kept as two separate tables rather than
//! one merged list with a mode column.
//!
//! Like `Config`, a missing or corrupt file falls back rather than blocking
//! launch — here entry by entry, so one mangled row costs only itself — and a
//! failed write is never worth interrupting play over.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::game::Mode;

const APP_DIR: &str = "tetris-tui";
const SCORES_FILE: &str = "scores.toml";

/// Length of each mode's table.
pub const MAX_ENTRIES: usize = 10;

/// Longest name we will store, which is also what the entry field accepts.
pub const MAX_NAME: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub name: String,
    pub score: u64,
    pub lines: u32,
    pub level: u32,
    /// `YYYY-MM-DD`, UTC. Stored as a string so the file stays readable and so
    /// nothing downstream has to parse it back.
    pub date: String,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            name: "player".into(),
            score: 0,
            lines: 0,
            level: 0,
            date: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Scores {
    nes: Vec<Entry>,
    modern: Vec<Entry>,
}

pub fn scores_path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(APP_DIR).join(SCORES_FILE))
}

impl Scores {
    /// Load from disk. A file that was not read in full is kept as
    /// `scores.toml.bak`, because the next score recorded rewrites the file
    /// from whatever was salvaged.
    pub fn load() -> Self {
        let Some(path) = scores_path() else {
            return Self::default();
        };
        crate::storage::load(&path, Self::from_table)
    }

    pub fn from_toml(text: &str) -> Self {
        text.parse()
            .map(|table| Self::from_table(table).0)
            .unwrap_or_default()
    }

    /// Entry by entry: one mangled row costs that row, not the table. Returns
    /// whether anything was dropped.
    fn from_table(table: toml::Table) -> (Self, bool) {
        let mut scores = Self::default();
        let mut lossy = false;
        for mode in [Mode::Nes, Mode::Modern] {
            let key = match mode {
                Mode::Nes => "nes",
                Mode::Modern => "modern",
            };
            let rows = match table.get(key) {
                None => continue,
                Some(toml::Value::Array(rows)) => rows,
                Some(_) => {
                    lossy = true;
                    continue;
                }
            };
            for row in rows {
                match row.clone().try_into::<Entry>() {
                    Ok(entry) => scores.table_mut(mode).push(entry),
                    Err(_) => lossy = true,
                }
            }

            // A hand-edited file can be out of order or over-long; normalise
            // rather than trusting it, so the ranking on screen is the real one.
            let table = scores.table_mut(mode);
            table.sort_by_key(|entry| std::cmp::Reverse(entry.score));
            table.truncate(MAX_ENTRIES);
        }
        (scores, lossy)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = scores_path() else {
            return Ok(());
        };
        crate::storage::write_atomic(&path, &self.to_toml())
    }

    pub fn table(&self, mode: Mode) -> &[Entry] {
        match mode {
            Mode::Nes => &self.nes,
            Mode::Modern => &self.modern,
        }
    }

    fn table_mut(&mut self, mode: Mode) -> &mut Vec<Entry> {
        match mode {
            Mode::Nes => &mut self.nes,
            Mode::Modern => &mut self.modern,
        }
    }

    /// Whether `score` would make this mode's table. A scoreless game never
    /// qualifies, however empty the table is.
    pub fn qualifies(&self, mode: Mode, score: u64) -> bool {
        if score == 0 {
            return false;
        }
        let table = self.table(mode);
        table.len() < MAX_ENTRIES || table.last().is_some_and(|last| score > last.score)
    }

    /// Insert and return the 0-based rank, or `None` if it did not make the table.
    /// A score equal to an existing one ranks below it: the earlier run keeps its
    /// place.
    pub fn insert(&mut self, mode: Mode, entry: Entry) -> Option<usize> {
        if !self.qualifies(mode, entry.score) {
            return None;
        }
        let table = self.table_mut(mode);
        let rank = table
            .iter()
            .position(|existing| entry.score > existing.score)
            .unwrap_or(table.len());
        table.insert(rank, entry);
        table.truncate(MAX_ENTRIES);
        Some(rank)
    }
}

/// Today's date as `YYYY-MM-DD`, UTC.
///
/// A whole date library would be a dependency bought for one string, so the civil
/// date is computed directly from the epoch day count.
pub fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 / 86_400)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's days-from-civil inverse: exact for the whole proleptic
/// Gregorian calendar, no lookup tables and no leap-year special cases.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01 so leap days land at the end of the cycle.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // day of era, [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year, March-based
    let mp = (5 * doy + 2) / 153; // March-based month, [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, score: u64) -> Entry {
        Entry {
            name: name.into(),
            score,
            lines: 10,
            level: 3,
            date: "2026-09-22".into(),
        }
    }

    #[test]
    fn entries_are_kept_in_descending_score_order() {
        let mut scores = Scores::default();
        for (name, score) in [("a", 100), ("b", 300), ("c", 200)] {
            scores.insert(Mode::Nes, entry(name, score));
        }
        let names: Vec<&str> = scores
            .table(Mode::Nes)
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["b", "c", "a"]);
    }

    #[test]
    fn insert_reports_the_rank_it_landed_at() {
        let mut scores = Scores::default();
        assert_eq!(scores.insert(Mode::Nes, entry("first", 500)), Some(0));
        assert_eq!(scores.insert(Mode::Nes, entry("lower", 100)), Some(1));
        assert_eq!(scores.insert(Mode::Nes, entry("top", 900)), Some(0));
    }

    /// A tie should not demote the run that got there first.
    #[test]
    fn an_equal_score_ranks_below_the_existing_one() {
        let mut scores = Scores::default();
        scores.insert(Mode::Nes, entry("early", 500));
        assert_eq!(scores.insert(Mode::Nes, entry("late", 500)), Some(1));
    }

    #[test]
    fn the_table_is_capped_and_the_weakest_entry_falls_off() {
        let mut scores = Scores::default();
        for i in 0..MAX_ENTRIES as u64 {
            scores.insert(Mode::Nes, entry("filler", (i + 1) * 100));
        }
        assert_eq!(scores.table(Mode::Nes).len(), MAX_ENTRIES);

        assert!(scores.qualifies(Mode::Nes, 150));
        assert_eq!(scores.insert(Mode::Nes, entry("new", 150)), Some(9));
        assert_eq!(scores.table(Mode::Nes).len(), MAX_ENTRIES);
        assert_eq!(scores.table(Mode::Nes).last().unwrap().name, "new");

        assert!(!scores.qualifies(Mode::Nes, 50), "below the whole table");
        assert_eq!(scores.insert(Mode::Nes, entry("nope", 50)), None);
    }

    #[test]
    fn a_scoreless_game_never_qualifies() {
        let scores = Scores::default();
        assert!(!scores.qualifies(Mode::Nes, 0));
        assert!(scores.qualifies(Mode::Nes, 1));
    }

    /// The two rulesets' scores are not comparable, so their tables must not mix.
    #[test]
    fn the_two_modes_keep_separate_tables() {
        let mut scores = Scores::default();
        scores.insert(Mode::Nes, entry("nes", 100));
        assert!(scores.table(Mode::Modern).is_empty());
        assert_eq!(scores.table(Mode::Nes).len(), 1);
    }

    #[test]
    fn scores_round_trip_through_toml() {
        let mut scores = Scores::default();
        scores.insert(Mode::Nes, entry("nes", 100));
        scores.insert(Mode::Modern, entry("modern", 9000));

        let restored = Scores::from_toml(&scores.to_toml());
        assert_eq!(restored.table(Mode::Nes), scores.table(Mode::Nes));
        assert_eq!(restored.table(Mode::Modern), scores.table(Mode::Modern));
    }

    #[test]
    fn a_corrupt_file_falls_back_to_an_empty_table() {
        let scores = Scores::from_toml("}}} not toml");
        assert!(scores.table(Mode::Nes).is_empty());
        assert!(scores.table(Mode::Modern).is_empty());
    }

    /// One mangled row used to cost both tables — and the next high score then
    /// saved the empty result over every score the player had.
    #[test]
    fn a_mangled_row_costs_only_itself() {
        let text = "\
[[nes]]
name = \"good\"
score = 500

[[nes]]
name = \"bad\"
score = \"lots\"

[[modern]]
name = \"fine\"
score = 9000
";
        let table = text.parse::<toml::Table>().unwrap();
        let (scores, lossy) = Scores::from_table(table);
        assert!(lossy, "the dropped row is reported");
        let nes: Vec<&str> = scores
            .table(Mode::Nes)
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(nes, ["good"]);
        assert_eq!(scores.table(Mode::Modern)[0].score, 9000);
    }

    #[test]
    fn a_clean_file_reports_nothing_dropped() {
        let mut scores = Scores::default();
        scores.insert(Mode::Nes, entry("a", 100));
        let table = scores.to_toml().parse::<toml::Table>().unwrap();
        let (restored, lossy) = Scores::from_table(table);
        assert!(!lossy);
        assert_eq!(restored.table(Mode::Nes), scores.table(Mode::Nes));
    }

    /// The file is meant to be hand-editable, so it cannot be trusted to be sorted
    /// or within length.
    #[test]
    fn a_hand_edited_file_is_normalised_on_load() {
        let text = r#"
[[nes]]
name = "low"
score = 10
lines = 1
level = 0
date = "2026-01-01"

[[nes]]
name = "high"
score = 999
lines = 9
level = 2
date = "2026-01-02"
"#;
        let scores = Scores::from_toml(text);
        assert_eq!(scores.table(Mode::Nes)[0].name, "high");
    }

    #[test]
    fn missing_entry_fields_fall_back_rather_than_dropping_the_file() {
        let scores = Scores::from_toml("[[modern]]\nscore = 42\n");
        let entry = &scores.table(Mode::Modern)[0];
        assert_eq!(entry.score, 42);
        assert_eq!(entry.lines, 0);
        assert!(!entry.name.is_empty());
    }

    #[test]
    fn the_epoch_and_a_leap_day_convert_correctly() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(365), (1971, 1, 1));
        assert_eq!(civil_from_days(789), (1972, 2, 29), "1972 was a leap year");
        assert_eq!(civil_from_days(790), (1972, 3, 1));
        // 2000 was a leap year despite being a century.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn today_is_a_zero_padded_iso_date() {
        let date = today();
        assert_eq!(date.len(), 10, "{date}");
        let parts: Vec<&str> = date.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[1].len(), 2);
        assert_eq!(parts[2].len(), 2);
        assert!(parts[0].parse::<u32>().unwrap() >= 2024);
    }
}
