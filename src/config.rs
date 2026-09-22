//! User settings, persisted as hand-editable TOML.
//!
//! A missing or corrupt file must never stop the game starting: anything that
//! fails to load falls back to defaults. The same goes for individual fields, so
//! a config written by an older version still works.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crossterm::event::KeyModifiers;
use serde::{Deserialize, Serialize};

use crate::engine::modern::game::{
    Settings as ModernSettings, DEFAULT_ARR_FRAMES, DEFAULT_DAS_FRAMES,
};
use crate::game::Mode;
use crate::input::action::Action;
use crate::input::keymap::Keymap;
use crate::input::keyname::{key_name, parse_key};

const APP_DIR: &str = "tetris-tui";
const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mode: Mode,
    pub nes_start_level: u32,
    pub modern_start_level: u32,
    /// Frames before auto-shift begins in modern mode. NES timing is fixed by the
    /// ruleset and deliberately not configurable.
    pub das_frames: u32,
    pub arr_frames: u32,
    pub ghost: bool,
    /// Action name to the keys bound to it.
    pub bindings: BTreeMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Nes,
            nes_start_level: 0,
            modern_start_level: 1,
            das_frames: DEFAULT_DAS_FRAMES,
            arr_frames: DEFAULT_ARR_FRAMES,
            ghost: true,
            bindings: default_bindings(),
        }
    }
}

fn default_bindings() -> BTreeMap<String, Vec<String>> {
    let keymap = Keymap::default();
    Action::ALL
        .into_iter()
        .map(|action| {
            let mut keys: Vec<String> = keymap.keys_for(action).into_iter().map(key_name).collect();
            keys.sort();
            (action.name().to_string(), keys)
        })
        .collect()
}

pub fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_DIR).join(CONFIG_FILE))
}

impl Config {
    /// Load from disk, falling back to defaults for anything unreadable.
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        Self::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> Self {
        toml::from_str(text).unwrap_or_default()
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = config_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_toml())
    }

    pub fn start_level(&self, mode: Mode) -> u32 {
        match mode {
            Mode::Nes => self.nes_start_level,
            Mode::Modern => self.modern_start_level,
        }
    }

    pub fn set_start_level(&mut self, mode: Mode, level: u32) {
        match mode {
            Mode::Nes => self.nes_start_level = level,
            Mode::Modern => self.modern_start_level = level.max(1),
        }
    }

    pub fn modern_settings(&self) -> ModernSettings {
        ModernSettings {
            das_frames: self.das_frames.max(1),
            arr_frames: self.arr_frames.max(1),
            ghost: self.ghost,
        }
    }

    /// Build a keymap from the stored bindings. Unparseable entries are skipped,
    /// and an action left with no usable key falls back to its default binding so
    /// the player cannot lock themselves out.
    pub fn keymap(&self) -> Keymap {
        let defaults = Keymap::default();
        let mut map = Keymap::empty();

        for action in Action::ALL {
            let configured: Vec<_> = self
                .bindings
                .get(action.name())
                .map(|keys| keys.iter().filter_map(|k| parse_key(k)).collect())
                .unwrap_or_default();

            let keys = if configured.is_empty() {
                defaults.keys_for(action)
            } else {
                configured
            };

            for code in keys {
                map.bind(code, KeyModifiers::NONE, action);
            }
        }
        map
    }

    pub fn set_binding(&mut self, action: Action, keys: &[crossterm::event::KeyCode]) {
        self.bindings.insert(
            action.name().to_string(),
            keys.iter().map(|&k| key_name(k)).collect(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn a_default_config_round_trips_through_toml() {
        let config = Config::default();
        let restored = Config::from_toml(&config.to_toml());
        assert_eq!(restored.mode, config.mode);
        assert_eq!(restored.das_frames, config.das_frames);
        assert_eq!(restored.bindings, config.bindings);
    }

    #[test]
    fn a_corrupt_file_falls_back_to_defaults_instead_of_failing() {
        let config = Config::from_toml("this is not toml {{{");
        assert_eq!(config.mode, Mode::Nes);
        assert_eq!(config.das_frames, DEFAULT_DAS_FRAMES);
        assert!(!config.bindings.is_empty());
    }

    /// A config written by an older version is missing fields; those should take
    /// their defaults rather than losing the whole file.
    #[test]
    fn missing_fields_fall_back_individually() {
        let config = Config::from_toml("mode = \"Modern\"\nghost = false\n");
        assert_eq!(config.mode, Mode::Modern);
        assert!(!config.ghost);
        assert_eq!(config.das_frames, DEFAULT_DAS_FRAMES, "default preserved");
        assert!(!config.bindings.is_empty(), "bindings default preserved");
    }

    #[test]
    fn the_default_keymap_matches_the_built_in_one() {
        let from_config = Config::default().keymap();
        for action in Action::ALL {
            let mut a = from_config.keys_for(action);
            let mut b = Keymap::default().keys_for(action);
            a.sort_by_key(|k| format!("{k:?}"));
            b.sort_by_key(|k| format!("{k:?}"));
            assert_eq!(a, b, "{action:?}");
        }
    }

    #[test]
    fn custom_bindings_are_honoured() {
        let mut config = Config::default();
        config.set_binding(Action::RotateCw, &[KeyCode::Char('k')]);

        let map = config.keymap();
        assert_eq!(
            map.action_for(&key(KeyCode::Char('k'))),
            Some(Action::RotateCw)
        );
        assert_eq!(
            map.action_for(&key(KeyCode::Char('x'))),
            None,
            "the replaced default should be gone"
        );
    }

    /// An unusable binding must not leave an action unreachable.
    #[test]
    fn an_action_bound_to_nothing_usable_keeps_its_default() {
        let mut config = Config::default();
        config
            .bindings
            .insert(Action::Quit.name().to_string(), vec!["wibble".into()]);

        let map = config.keymap();
        assert_eq!(map.action_for(&key(KeyCode::Char('q'))), Some(Action::Quit));
    }

    #[test]
    fn start_levels_are_tracked_per_mode() {
        let mut config = Config::default();
        config.set_start_level(Mode::Nes, 18);
        config.set_start_level(Mode::Modern, 7);
        assert_eq!(config.start_level(Mode::Nes), 18);
        assert_eq!(config.start_level(Mode::Modern), 7);
    }

    /// Modern levels are 1-based, so 0 would make the gravity curve nonsense.
    #[test]
    fn a_modern_start_level_of_zero_is_clamped() {
        let mut config = Config::default();
        config.set_start_level(Mode::Modern, 0);
        assert_eq!(config.start_level(Mode::Modern), 1);
    }

    #[test]
    fn modern_settings_never_produce_a_zero_length_delay() {
        let config = Config {
            das_frames: 0,
            arr_frames: 0,
            ..Default::default()
        };
        let settings = config.modern_settings();
        assert!(settings.das_frames >= 1);
        assert!(settings.arr_frames >= 1);
    }

    #[test]
    fn the_written_file_is_readable_toml() {
        let text = Config::default().to_toml();
        assert!(text.contains("mode"));
        assert!(text.contains("move_left"));
        assert!(toml::from_str::<toml::Value>(&text).is_ok());
    }
}
