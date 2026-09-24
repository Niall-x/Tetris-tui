//! Reading and writing the two files the player may edit by hand: settings and
//! high scores.
//!
//! A hand edit can go wrong, and a mistake in one must cost as little as
//! possible: the one bad value rather than the whole file, and never the
//! player's data. So a file is loaded leniently, keeping everything that still
//! parses; one that could not be read in full is copied aside before the game
//! can write over it; and writes go to a temporary file renamed into place, so
//! a crash mid-save cannot leave half a file behind.

use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// The folder both files live in, under the platform's config directory for
/// settings and its data directory for scores.
pub const APP_DIR: &str = "tetris-tui";

/// Load `path` with `parse`, which returns the value and whether it had to drop
/// anything to get it. A missing file is simply the default.
pub fn load<T: Default>(path: &Path, parse: impl FnOnce(toml::Table) -> (T, bool)) -> T {
    let Ok(text) = std::fs::read_to_string(path) else {
        return T::default();
    };
    match text.parse::<toml::Table>() {
        Ok(table) => {
            let (value, lossy) = parse(table);
            if lossy {
                set_aside(path);
            }
            value
        }
        Err(_) => {
            if !text.trim().is_empty() {
                set_aside(path);
            }
            T::default()
        }
    }
}

/// Where a file that could not be read in full is copied to.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// Keep a copy of a file the game is about to stop trusting. Copied rather than
/// moved: the scores file in particular may not be written again for a while,
/// and until it is, the original is still the best there is. Best effort — a
/// failure here must not stop the game.
fn set_aside(path: &Path) {
    let _ = std::fs::copy(path, backup_path(path));
}

/// Write `text` to `path` via a temporary file in the same directory, so the
/// file is only ever the old version or the new one.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let temporary = path.with_file_name(name);
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path)
}

/// Deserialize a table field by field over `T`'s defaults: each of the
/// player's values is kept if the result still parses, and dropped — leaving
/// the default — if it does not. Returns whether anything was dropped.
///
/// Trying each value against the whole struct, rather than listing fields,
/// means a field added later is covered without anyone remembering to.
pub fn lenient<T>(user: toml::Table) -> (T, bool)
where
    T: Serialize + DeserializeOwned + Default,
{
    let Ok(toml::Value::Table(mut merged)) = toml::Value::try_from(T::default()) else {
        return (T::default(), !user.is_empty());
    };

    let mut lossy = false;
    for (key, value) in user {
        let previous = merged.insert(key.clone(), value);
        if toml::Value::Table(merged.clone()).try_into::<T>().is_err() {
            lossy = true;
            match previous {
                Some(previous) => merged.insert(key, previous),
                None => merged.remove(&key),
            };
        }
    }

    match toml::Value::Table(merged).try_into() {
        Ok(value) => (value, lossy),
        Err(_) => (T::default(), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    /// A directory of its own under the system temp dir, so tests never touch
    /// the player's real files or each other's.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tetris-tui-storage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Settings {
        level: u32,
        name: String,
        on: bool,
    }

    fn parse(table: toml::Table) -> (Settings, bool) {
        lenient(table)
    }

    #[test]
    fn one_bad_value_costs_only_itself() {
        let table = "level = \"high\"\nname = \"ada\"\non = true"
            .parse::<toml::Table>()
            .unwrap();
        let (settings, lossy) = lenient::<Settings>(table);
        assert!(lossy);
        assert_eq!(
            settings,
            Settings {
                level: 0,
                name: "ada".into(),
                on: true
            }
        );
    }

    #[test]
    fn a_clean_table_loses_nothing_and_unknown_keys_are_ignored() {
        let table = "level = 3\nretired_setting = 1"
            .parse::<toml::Table>()
            .unwrap();
        let (settings, lossy) = lenient::<Settings>(table);
        assert!(!lossy);
        assert_eq!(settings.level, 3);
    }

    #[test]
    fn an_unreadable_file_is_copied_aside_before_it_can_be_overwritten() {
        let dir = scratch("unreadable");
        let path = dir.join("config.toml");
        std::fs::write(&path, "level = {{{").unwrap();

        let settings: Settings = load(&path, parse);
        assert_eq!(settings, Settings::default());
        assert_eq!(
            std::fs::read_to_string(backup_path(&path)).unwrap(),
            "level = {{{"
        );
        assert!(path.exists(), "copied, not moved");
    }

    #[test]
    fn a_partly_bad_file_is_copied_aside_too() {
        let dir = scratch("partly");
        let path = dir.join("config.toml");
        std::fs::write(&path, "level = \"high\"\nname = \"ada\"").unwrap();

        let settings: Settings = load(&path, parse);
        assert_eq!(settings.name, "ada");
        assert!(backup_path(&path).exists());
    }

    #[test]
    fn a_good_or_missing_file_leaves_no_backup() {
        let dir = scratch("good");
        let path = dir.join("config.toml");
        let _: Settings = load(&path, parse);
        std::fs::write(&path, "level = 2").unwrap();
        let settings: Settings = load(&path, parse);
        assert_eq!(settings.level, 2);
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn an_atomic_write_lands_whole_and_leaves_nothing_behind() {
        let dir = scratch("atomic");
        let path = dir.join("nested").join("scores.toml");
        write_atomic(&path, "first").unwrap();
        write_atomic(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["scores.toml"]);
    }
}
