//! User settings, persisted as hand-editable TOML.
//!
//! A missing or corrupt file must never stop the game starting, and a bad value
//! costs only itself: each setting that cannot be read falls back to its own
//! default (see `crate::storage`). A config written by an older version, which
//! simply lacks the newer fields, works the same way.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyModifiers};
use serde::{Deserialize, Serialize};

use crate::background::scenes::SceneChoice;
use crate::background::BackgroundKind;
use crate::engine::modern::bag::MAX_PREVIEW;
use crate::engine::modern::game::{
    Settings as ModernSettings, DEFAULT_ARR_FRAMES, DEFAULT_DAS_FRAMES, DEFAULT_PREVIEWS,
};
use crate::game::Mode;
use crate::input::action::{Action, MenuInput};
use crate::input::keymap::{Keymap, MenuKeymap};
use crate::input::keyname::{key_name, parse_key};
use crate::storage;
use crate::ui::style::{BorderStyle, Skin, Theme, Visuals};

const APP_DIR: &str = "tetris-tui";
const CONFIG_FILE: &str = "config.toml";

/// A second: longer stops being a pause and starts being a wait.
pub const MAX_LINE_CLEAR_FRAMES: u32 = 60;

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
    /// Modern only: frames cleared rows animate before the rows above drop. Zero
    /// is instant. NES's line-clear delay is fixed by the ruleset.
    pub line_clear_frames: u32,
    /// Modern only: how many upcoming pieces the next queue shows, 1 to 6. NES
    /// previews exactly one.
    pub previews: usize,
    /// The three visual axes (§7), independent of each other and of the ruleset.
    pub theme: Theme,
    pub skin: Skin,
    pub border: BorderStyle,
    pub background: BackgroundKind,
    /// Which still scene the `Scene` background shows; `Random` picks one per
    /// session.
    pub scene: SceneChoice,
    /// Draw the background at reduced brightness, so it stays behind the board.
    pub dim_background: bool,
    /// Name offered first in the high-score entry field, so a player who always
    /// uses the same one only types it once.
    pub player_name: String,
    /// Action name to the keys bound to it.
    pub bindings: BTreeMap<String, Vec<String>>,
    /// Menu input name to the rebindable keys for it. The arrows, Enter and Esc
    /// work on every menu regardless, so they are not stored here.
    pub menu_bindings: BTreeMap<String, Vec<String>>,
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
            line_clear_frames: 0,
            previews: DEFAULT_PREVIEWS,
            theme: Theme::default(),
            skin: Skin::default(),
            border: BorderStyle::default(),
            background: BackgroundKind::default(),
            scene: SceneChoice::default(),
            dim_background: true,
            player_name: "player".into(),
            bindings: default_bindings(),
            menu_bindings: default_menu_bindings(),
        }
    }
}

fn key_names(keys: Vec<KeyCode>) -> Vec<String> {
    let mut names: Vec<String> = keys.into_iter().map(key_name).collect();
    names.sort();
    names
}

fn default_bindings() -> BTreeMap<String, Vec<String>> {
    let keymap = Keymap::default();
    Action::ALL
        .into_iter()
        .map(|action| {
            (
                action.name().to_string(),
                key_names(keymap.keys_for(action)),
            )
        })
        .collect()
}

fn default_menu_bindings() -> BTreeMap<String, Vec<String>> {
    let keymap = MenuKeymap::default();
    MenuInput::ALL
        .into_iter()
        .map(|input| (input.name().to_string(), key_names(keymap.keys_for(input))))
        .collect()
}

/// The keys configured under `name`, or `None` if there are no usable ones.
fn configured_keys(table: &BTreeMap<String, Vec<String>>, name: &str) -> Option<Vec<KeyCode>> {
    let keys: Vec<KeyCode> = table
        .get(name)?
        .iter()
        .filter_map(|k| parse_key(k))
        .collect();
    (!keys.is_empty()).then_some(keys)
}

fn clamp_level(mode: Mode, level: u32) -> u32 {
    let range = mode.start_levels();
    level.clamp(*range.start(), *range.end())
}

pub fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_DIR).join(CONFIG_FILE))
}

impl Config {
    /// Load from disk, falling back to the default for each setting that is
    /// missing or unreadable. A file that was not read in full is kept as
    /// `config.toml.bak`, since it is rewritten on exit.
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        storage::load(&path, storage::lenient)
    }

    pub fn from_toml(text: &str) -> Self {
        text.parse()
            .map(|table| storage::lenient(table).0)
            .unwrap_or_default()
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = config_path() else {
            return Ok(());
        };
        storage::write_atomic(&path, &self.to_toml())
    }

    /// Clamped on the way out as well as in, since a hand-edited file can hold
    /// anything.
    pub fn start_level(&self, mode: Mode) -> u32 {
        let level = match mode {
            Mode::Nes => self.nes_start_level,
            Mode::Modern => self.modern_start_level,
        };
        clamp_level(mode, level)
    }

    /// Out-of-range levels — from the command line, say — are pulled into the
    /// mode's range rather than rejected.
    pub fn set_start_level(&mut self, mode: Mode, level: u32) {
        let level = clamp_level(mode, level);
        match mode {
            Mode::Nes => self.nes_start_level = level,
            Mode::Modern => self.modern_start_level = level,
        }
    }

    pub fn visuals(&self) -> Visuals {
        Visuals {
            theme: self.theme,
            skin: self.skin,
            border: self.border,
        }
    }

    pub fn modern_settings(&self) -> ModernSettings {
        ModernSettings {
            das_frames: self.das_frames.max(1),
            arr_frames: self.arr_frames.max(1),
            ghost: self.ghost,
            line_clear_frames: self.line_clear_frames.min(MAX_LINE_CLEAR_FRAMES),
            previews: self.previews.clamp(1, MAX_PREVIEW),
        }
    }

    /// Build a keymap from the stored bindings. Unparseable entries are skipped,
    /// and an action left with no usable key falls back to its default binding so
    /// the player cannot lock themselves out.
    pub fn keymap(&self) -> Keymap {
        let defaults = Keymap::default();
        let mut map = Keymap::empty();

        for action in Action::ALL {
            let keys = configured_keys(&self.bindings, action.name())
                .unwrap_or_else(|| defaults.keys_for(action));
            for code in keys {
                map.bind(code, KeyModifiers::NONE, action);
            }
        }
        map
    }

    /// The menu keymap, with the same fallback as `keymap`. A fixed menu key in
    /// the file is skipped: it already does its fixed job and cannot do another.
    pub fn menu_keymap(&self) -> MenuKeymap {
        let defaults = MenuKeymap::default();
        let mut map = MenuKeymap::empty();

        for input in MenuInput::ALL {
            let keys = configured_keys(&self.menu_bindings, input.name())
                .unwrap_or_else(|| defaults.keys_for(input));
            for code in keys {
                if !MenuKeymap::is_fixed(code) {
                    map.bind(code, input);
                }
            }
        }
        map
    }

    pub fn set_binding(&mut self, action: Action, keys: &[KeyCode]) {
        self.bindings
            .insert(action.name().to_string(), key_names(keys.to_vec()));
    }

    pub fn set_menu_binding(&mut self, input: MenuInput, keys: &[KeyCode]) {
        self.menu_bindings
            .insert(input.name().to_string(), key_names(keys.to_vec()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};

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
    fn the_visual_settings_round_trip_through_toml() {
        let config = Config {
            theme: Theme::SystemAnsi,
            skin: Skin::Letter,
            border: BorderStyle::Ascii,
            ..Default::default()
        };
        let restored = Config::from_toml(&config.to_toml());
        assert_eq!(restored.theme, Theme::SystemAnsi);
        assert_eq!(restored.skin, Skin::Letter);
        assert_eq!(restored.border, BorderStyle::Ascii);
        assert_eq!(restored.visuals(), config.visuals());
    }

    /// A config from before theming existed has none of these fields.
    #[test]
    fn a_config_without_visual_settings_takes_their_defaults() {
        let config = Config::from_toml("mode = \"Nes\"\n");
        assert_eq!(config.visuals(), Visuals::default());
    }

    #[test]
    fn a_corrupt_file_falls_back_to_defaults_instead_of_failing() {
        let config = Config::from_toml("this is not toml {{{");
        assert_eq!(config.mode, Mode::Nes);
        assert_eq!(config.das_frames, DEFAULT_DAS_FRAMES);
        assert!(!config.bindings.is_empty());
    }

    /// One bad value used to reset every setting, and the config is written on
    /// exit, so the player's hand edits were lost with it.
    #[test]
    fn one_bad_value_does_not_reset_the_rest() {
        let config = Config::from_toml(
            "mode = \"Tetris\"\nghost = false\ntheme = \"SystemAnsi\"\nplayer_name = \"ada\"",
        );
        assert_eq!(config.mode, Mode::Nes, "the bad value takes its default");
        assert!(!config.ghost);
        assert_eq!(config.theme, Theme::SystemAnsi);
        assert_eq!(config.player_name, "ada");
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
        config.set_binding(Action::RotateCw, &[KeyCode::Char('n')]);

        let map = config.keymap();
        assert_eq!(
            map.action_for(&key(KeyCode::Char('n'))),
            Some(Action::RotateCw)
        );
        assert_eq!(
            map.action_for(&key(KeyCode::Char('k'))),
            None,
            "the replaced default should be gone"
        );
    }

    #[test]
    fn custom_menu_bindings_are_honoured_and_round_trip() {
        let mut config = Config::default();
        config.set_menu_binding(MenuInput::Confirm, &[KeyCode::Char('l')]);

        let restored = Config::from_toml(&config.to_toml());
        let menu = restored.menu_keymap();
        assert_eq!(
            menu.input_for(&key(KeyCode::Char('l'))),
            Some(MenuInput::Confirm)
        );
        assert_eq!(menu.input_for(&key(KeyCode::Char('j'))), None);
        // Enter is fixed, so it confirms whatever the file says.
        assert_eq!(
            menu.input_for(&key(KeyCode::Enter)),
            Some(MenuInput::Confirm)
        );
    }

    /// A config written before menu bindings existed gets the default ones.
    #[test]
    fn a_config_without_menu_bindings_takes_the_defaults() {
        let config = Config::from_toml("[bindings]\nhold = [\"c\"]\n");
        assert_eq!(
            config.menu_keymap().input_for(&key(KeyCode::Char('w'))),
            Some(MenuInput::Up)
        );
    }

    /// A hand-edited file binding a fixed key to another menu input must not
    /// change what the fixed key does.
    #[test]
    fn a_fixed_menu_key_in_the_file_keeps_its_fixed_job() {
        let config = Config::from_toml("[menu_bindings]\nback = [\"enter\", \"x\"]\n");
        let menu = config.menu_keymap();
        assert_eq!(
            menu.input_for(&key(KeyCode::Enter)),
            Some(MenuInput::Confirm)
        );
        assert_eq!(
            menu.input_for(&key(KeyCode::Char('x'))),
            Some(MenuInput::Back)
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

    /// A level from the command line or a hand-edited file can be anything; the
    /// run must still start on one the mode actually has.
    #[test]
    fn start_levels_are_clamped_to_the_modes_range() {
        let mut config = Config::default();
        config.set_start_level(Mode::Nes, 99);
        assert_eq!(config.start_level(Mode::Nes), 29);
        config.set_start_level(Mode::Modern, 0);
        assert_eq!(config.start_level(Mode::Modern), 1);

        let edited = Config::from_toml("modern_start_level = 500");
        assert_eq!(edited.start_level(Mode::Modern), 20);
    }
}
