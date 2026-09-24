//! Menu state and navigation, with no rendering in it.
//!
//! Keeping the cursor movement, the option adjustments and the rebinding rules
//! here — rather than inside the draw code — is what makes them unit-testable:
//! every screen below can be driven by feeding it `MenuInput`s and asserting on
//! the config it produced.
//!
//! Menu keys have bindings of their own, separate from the gameplay ones, so a
//! gameplay rebind never changes what a key does on a menu. The arrows, Enter and
//! Esc work on every menu whatever is bound (`FIXED_MENU_KEYS`), which is what
//! keeps a menu rebind from locking the player out of the screen that undoes it.

use crossterm::event::{KeyCode, KeyEvent};

use crate::background::scenes::SceneChoice;
use crate::background::BackgroundKind;
use crate::config::{Config, MAX_LINE_CLEAR_FRAMES};
use crate::game::Mode;
use crate::input::action::Action;
pub use crate::input::action::MenuInput;
use crate::input::keymap::MenuKeymap;
use crate::input::keyname::display_name;
use crate::scores::MAX_NAME;
use crate::ui::style::{BorderStyle, Skin, Theme};

/// Move a wrapping cursor. Wrapping matters more than it sounds on the options
/// screen, where the keybind list runs well past the bottom of a short terminal.
fn step(selected: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if forward {
        (selected + 1) % len
    } else {
        (selected + len - 1) % len
    }
}

// ---------------------------------------------------------------------------
// Title
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleItem {
    Play,
    Options,
    HighScores,
    Quit,
}

impl TitleItem {
    pub const ALL: [TitleItem; 4] = [
        TitleItem::Play,
        TitleItem::Options,
        TitleItem::HighScores,
        TitleItem::Quit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TitleItem::Play => "Play",
            TitleItem::Options => "Options",
            TitleItem::HighScores => "High scores",
            TitleItem::Quit => "Quit",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TitleMenu {
    pub selected: usize,
}

impl TitleMenu {
    pub fn items(&self) -> [TitleItem; 4] {
        TitleItem::ALL
    }

    pub fn navigate(&mut self, input: MenuInput) -> Option<TitleItem> {
        match input {
            MenuInput::Up | MenuInput::Left => {
                self.selected = step(self.selected, TitleItem::ALL.len(), false);
                None
            }
            MenuInput::Down | MenuInput::Right => {
                self.selected = step(self.selected, TitleItem::ALL.len(), true);
                None
            }
            MenuInput::Confirm => Some(TitleItem::ALL[self.selected]),
            // Backing out of the title screen points at Quit rather than quitting:
            // Back is a rotate key by default, and a stray press should not end
            // the session. A second Back confirms.
            MenuInput::Back => {
                let quit = TitleItem::ALL.len() - 1;
                if self.selected == quit {
                    Some(TitleItem::Quit)
                } else {
                    self.selected = quit;
                    None
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pause
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseItem {
    Resume,
    Options,
    QuitToTitle,
}

impl PauseItem {
    pub const ALL: [PauseItem; 3] = [
        PauseItem::Resume,
        PauseItem::Options,
        PauseItem::QuitToTitle,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PauseItem::Resume => "Resume",
            PauseItem::Options => "Options",
            PauseItem::QuitToTitle => "Quit to title",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PauseMenu {
    pub selected: usize,
}

impl PauseMenu {
    pub fn navigate(&mut self, input: MenuInput) -> Option<PauseItem> {
        match input {
            MenuInput::Up | MenuInput::Left => {
                self.selected = step(self.selected, PauseItem::ALL.len(), false);
                None
            }
            MenuInput::Down | MenuInput::Right => {
                self.selected = step(self.selected, PauseItem::ALL.len(), true);
                None
            }
            MenuInput::Confirm => Some(PauseItem::ALL[self.selected]),
            MenuInput::Back => Some(PauseItem::Resume),
        }
    }
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

const MAX_DELAY_FRAMES: u32 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionRow {
    Mode,
    StartLevel,
    /// Modern only: NES's DAS is fixed by its ruleset and deliberately not exposed.
    Das,
    Arr,
    Ghost,
    /// Modern only: NES's line-clear delay is part of its ruleset.
    LineClear,
    Theme,
    Skin,
    Border,
    Background,
    /// Only shown while the scene background is the selected one.
    Scene,
    DimBackground,
    Bind(Action),
    MenuBind(MenuInput),
}

/// A binding being captured: a gameplay action or a menu input. The two are
/// separate namespaces, so a key only conflicts with others of its own kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rebind {
    Game(Action),
    Menu(MenuInput),
}

impl Rebind {
    pub fn label(self) -> &'static str {
        match self {
            Rebind::Game(action) => action.label(),
            Rebind::Menu(input) => input.label(),
        }
    }
}

impl OptionRow {
    /// The binding this row rebinds, if it is a rebind row.
    pub fn rebind(self) -> Option<Rebind> {
        match self {
            OptionRow::Bind(action) => Some(Rebind::Game(action)),
            OptionRow::MenuBind(input) => Some(Rebind::Menu(input)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OptionsMenu {
    pub selected: usize,
    /// Set while waiting for the next key press to bind.
    pub rebinding: Option<Rebind>,
    /// Feedback for the last action — a rejected conflicting key, mostly.
    pub notice: Option<String>,
}

/// What the caller should do after handing the menu an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsOutcome {
    Stay,
    /// The config changed and should be written out.
    Changed,
    Back,
}

impl OptionsMenu {
    /// The rows on offer, which follow the selected mode: modern's tuning knobs
    /// are absent in NES mode because NES's equivalents are fixed by the ruleset.
    pub fn rows(config: &Config) -> Vec<OptionRow> {
        let mut rows = vec![OptionRow::Mode, OptionRow::StartLevel];
        if config.mode == Mode::Modern {
            rows.extend([
                OptionRow::Das,
                OptionRow::Arr,
                OptionRow::Ghost,
                OptionRow::LineClear,
            ]);
        }
        // The visual axes are independent of the ruleset, so they are offered in
        // both modes.
        rows.extend([OptionRow::Theme, OptionRow::Skin, OptionRow::Border]);
        rows.push(OptionRow::Background);
        // The scene picker is meaningless unless the scene background is showing.
        if config.background == BackgroundKind::Scene {
            rows.push(OptionRow::Scene);
        }
        // Blank draws nothing, so there is nothing to dim.
        if config.background != BackgroundKind::Blank {
            rows.push(OptionRow::DimBackground);
        }
        rows.extend(Action::ALL.map(OptionRow::Bind));
        rows.extend(MenuInput::ALL.map(OptionRow::MenuBind));
        rows
    }

    pub fn row(&self, config: &Config) -> OptionRow {
        let rows = Self::rows(config);
        rows[self.selected.min(rows.len() - 1)]
    }

    pub fn navigate(&mut self, input: MenuInput, config: &mut Config) -> OptionsOutcome {
        let rows = Self::rows(config);
        let row = rows[self.selected.min(rows.len() - 1)];

        match input {
            MenuInput::Up => {
                self.notice = None;
                self.selected = step(self.selected, rows.len(), false);
                OptionsOutcome::Stay
            }
            MenuInput::Down => {
                self.notice = None;
                self.selected = step(self.selected, rows.len(), true);
                OptionsOutcome::Stay
            }
            MenuInput::Left => self.adjust(row, config, false),
            MenuInput::Right => self.adjust(row, config, true),
            MenuInput::Confirm => match row.rebind() {
                Some(rebind) => {
                    self.rebinding = Some(rebind);
                    self.notice = None;
                    OptionsOutcome::Stay
                }
                // Confirm on a value row is the same as nudging it forward, so
                // Enter is never a dead key.
                None => self.adjust(row, config, true),
            },
            MenuInput::Back => OptionsOutcome::Back,
        }
    }

    fn adjust(&mut self, row: OptionRow, config: &mut Config, forward: bool) -> OptionsOutcome {
        self.notice = None;
        match row {
            OptionRow::Mode => {
                config.mode = match config.mode {
                    Mode::Nes => Mode::Modern,
                    Mode::Modern => Mode::Nes,
                };
                // The row list differs per mode, so an offset past the end of the
                // shorter list has to be pulled back in.
                let len = Self::rows(config).len();
                self.selected = self.selected.min(len - 1);
            }
            OptionRow::StartLevel => {
                // `set_start_level` clamps to the mode's range, so this only has
                // to step.
                let mode = config.mode;
                let level = config.start_level(mode);
                let next = if forward {
                    level + 1
                } else {
                    level.saturating_sub(1)
                };
                config.set_start_level(mode, next);
            }
            OptionRow::Das => {
                config.das_frames = nudge(config.das_frames, forward);
            }
            OptionRow::Arr => {
                config.arr_frames = nudge(config.arr_frames, forward);
            }
            OptionRow::Ghost => config.ghost = !config.ghost,
            OptionRow::LineClear => {
                config.line_clear_frames = step_line_clear(config.line_clear_frames, forward);
            }
            OptionRow::Theme => config.theme = cycle(config.theme, &Theme::ALL, forward),
            OptionRow::Skin => config.skin = cycle(config.skin, &Skin::ALL, forward),
            OptionRow::Border => config.border = cycle(config.border, &BorderStyle::ALL, forward),
            OptionRow::DimBackground => config.dim_background = !config.dim_background,
            OptionRow::Background => {
                config.background = cycle(config.background, &BackgroundKind::ALL, forward);
                // Leaving the scene background removes a row below this one.
                let len = Self::rows(config).len();
                self.selected = self.selected.min(len - 1);
            }
            OptionRow::Scene => config.scene = cycle(config.scene, &SceneChoice::ALL, forward),
            // Rebinding is driven by `capture`, not by the direction keys.
            OptionRow::Bind(_) | OptionRow::MenuBind(_) => return OptionsOutcome::Stay,
        }
        OptionsOutcome::Changed
    }

    /// Take the next key press as the new binding for whatever is being rebound.
    ///
    /// Esc cancels. A key already bound elsewhere is rejected with a notice rather
    /// than silently stealing it, which would leave the other binding unreachable.
    /// Conflicts are only checked within the same keymap, so a key can both
    /// rotate and confirm.
    pub fn capture(&mut self, key: &KeyEvent, config: &mut Config) -> OptionsOutcome {
        let Some(rebind) = self.rebinding else {
            return OptionsOutcome::Stay;
        };

        // A terminal reporting every key sends Shift and friends on their own
        // too; a modifier is part of the key being pressed, not the key itself.
        if matches!(key.code, KeyCode::Modifier(_)) {
            return OptionsOutcome::Stay;
        }

        if key.code == KeyCode::Esc {
            self.rebinding = None;
            self.notice = Some("rebind cancelled".into());
            return OptionsOutcome::Stay;
        }

        let name = display_name(key.code);
        let conflict = match rebind {
            Rebind::Game(action) => config
                .keymap()
                .action_for(key)
                .filter(|&existing| existing != action)
                .map(|existing| existing.label()),
            Rebind::Menu(_) if MenuKeymap::is_fixed(key.code) => {
                self.notice = Some(format!("{name} already works on every menu"));
                return OptionsOutcome::Stay;
            }
            Rebind::Menu(input) => config
                .menu_keymap()
                .input_for(key)
                .filter(|&existing| existing != input)
                .map(|existing| existing.label()),
        };

        if let Some(existing) = conflict {
            self.notice = Some(format!("{name} is already bound to {existing}"));
            return OptionsOutcome::Stay;
        }

        match rebind {
            Rebind::Game(action) => config.set_binding(action, &[key.code]),
            Rebind::Menu(input) => config.set_menu_binding(input, &[key.code]),
        }
        self.rebinding = None;
        self.notice = Some(format!("{} bound to {name}", rebind.label()));
        OptionsOutcome::Changed
    }
}

/// Step through a fixed list of choices, wrapping at both ends. An unrecognised
/// current value — a hand-edited config naming something that no longer exists —
/// lands on the first choice rather than failing.
fn cycle<T: Copy + PartialEq>(current: T, all: &[T], forward: bool) -> T {
    let index = all.iter().position(|item| *item == current).unwrap_or(0);
    all[step(index, all.len(), forward)]
}

fn nudge(frames: u32, forward: bool) -> u32 {
    if forward {
        (frames + 1).min(MAX_DELAY_FRAMES)
    } else {
        frames.saturating_sub(1).max(1)
    }
}

/// Line-clear delay moves in steps of five frames, from instant up to a second.
/// Unlike DAS and ARR, zero is a real choice here: it is the instant clear.
fn step_line_clear(frames: u32, forward: bool) -> u32 {
    const STEP: u32 = 5;
    let frames = frames.min(MAX_LINE_CLEAR_FRAMES) / STEP * STEP;
    if forward {
        (frames + STEP).min(MAX_LINE_CLEAR_FRAMES)
    } else {
        frames.saturating_sub(STEP)
    }
}

// ---------------------------------------------------------------------------
// High scores (view-only)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ScoresView {
    pub mode: Mode,
}

impl ScoresView {
    pub fn new(mode: Mode) -> Self {
        Self { mode }
    }

    /// Returns true once the player wants out.
    pub fn navigate(&mut self, input: MenuInput) -> bool {
        match input {
            MenuInput::Left | MenuInput::Right | MenuInput::Up | MenuInput::Down => {
                self.mode = match self.mode {
                    Mode::Nes => Mode::Modern,
                    Mode::Modern => Mode::Nes,
                };
                false
            }
            MenuInput::Back | MenuInput::Confirm => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Game over / high-score entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOverItem {
    Retry,
    Title,
}

impl GameOverItem {
    pub const ALL: [GameOverItem; 2] = [GameOverItem::Retry, GameOverItem::Title];

    pub fn label(self) -> &'static str {
        match self {
            GameOverItem::Retry => "Retry",
            GameOverItem::Title => "Back to title",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GameOverMenu {
    /// Name being typed, while `entering` is set.
    pub name: String,
    pub entering: bool,
    /// Where the run placed, once the name has been submitted.
    pub rank: Option<usize>,
    pub selected: usize,
}

impl GameOverMenu {
    /// `qualifies` comes from the score table: a run that did not place skips
    /// straight to the Retry/Title choice rather than asking for a name.
    pub fn new(qualifies: bool, default_name: &str) -> Self {
        Self {
            name: default_name.chars().take(MAX_NAME).collect(),
            entering: qualifies,
            rank: None,
            selected: 0,
        }
    }

    /// A typed character during name entry. Returns whether it was consumed.
    pub fn type_char(&mut self, c: char) -> bool {
        if !self.entering || c.is_control() || self.name.chars().count() >= MAX_NAME {
            return false;
        }
        self.name.push(c);
        true
    }

    pub fn backspace(&mut self) -> bool {
        if !self.entering {
            return false;
        }
        self.name.pop().is_some()
    }

    /// Finish name entry. Returns the name to record, or `None` if entry is not
    /// in progress. An all-blank name falls back to "player" rather than leaving
    /// an anonymous row in the table.
    pub fn submit(&mut self) -> Option<String> {
        if !self.entering {
            return None;
        }
        self.entering = false;
        let name = self.name.trim();
        Some(if name.is_empty() {
            "player".to_string()
        } else {
            name.to_string()
        })
    }

    pub fn navigate(&mut self, input: MenuInput) -> Option<GameOverItem> {
        match input {
            MenuInput::Up | MenuInput::Left => {
                self.selected = step(self.selected, GameOverItem::ALL.len(), false);
                None
            }
            MenuInput::Down | MenuInput::Right => {
                self.selected = step(self.selected, GameOverItem::ALL.len(), true);
                None
            }
            MenuInput::Confirm => Some(GameOverItem::ALL[self.selected]),
            MenuInput::Back => Some(GameOverItem::Title),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    /// A config in a given mode, since almost every options test needs one.
    fn config(mode: Mode) -> Config {
        Config {
            mode,
            ..Default::default()
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn the_title_cursor_wraps_in_both_directions() {
        let mut menu = TitleMenu::default();
        menu.navigate(MenuInput::Up);
        assert_eq!(menu.selected, TitleItem::ALL.len() - 1);
        menu.navigate(MenuInput::Down);
        assert_eq!(menu.selected, 0);
    }

    #[test]
    fn confirming_on_the_title_returns_the_selected_item() {
        let mut menu = TitleMenu::default();
        assert_eq!(menu.navigate(MenuInput::Confirm), Some(TitleItem::Play));
        menu.navigate(MenuInput::Down);
        assert_eq!(menu.navigate(MenuInput::Confirm), Some(TitleItem::Options));
    }

    /// Back is a rotate key by default, so one stray press on the title screen
    /// only points at Quit; a second one leaves.
    #[test]
    fn backing_out_of_the_title_points_at_quit_then_quits() {
        let mut menu = TitleMenu::default();
        assert_eq!(menu.navigate(MenuInput::Back), None);
        assert_eq!(TitleItem::ALL[menu.selected], TitleItem::Quit);
        assert_eq!(menu.navigate(MenuInput::Back), Some(TitleItem::Quit));
    }

    #[test]
    fn backing_out_of_the_pause_menu_resumes() {
        let mut menu = PauseMenu::default();
        assert_eq!(menu.navigate(MenuInput::Back), Some(PauseItem::Resume));
    }

    /// NES's DAS and ghost piece are ruleset facts, not preferences, so those rows
    /// are absent in NES mode rather than shown greyed out.
    #[test]
    fn modern_only_rows_appear_only_in_modern_mode() {
        let mut config = config(Mode::Nes);
        let nes = OptionsMenu::rows(&config);
        assert!(!nes.contains(&OptionRow::Das));
        assert!(!nes.contains(&OptionRow::Ghost));
        assert!(!nes.contains(&OptionRow::LineClear));

        config.mode = Mode::Modern;
        let modern = OptionsMenu::rows(&config);
        assert!(modern.contains(&OptionRow::Das));
        assert!(modern.contains(&OptionRow::Arr));
        assert!(modern.contains(&OptionRow::Ghost));
        assert!(modern.contains(&OptionRow::LineClear));
    }

    #[test]
    fn every_action_gets_a_rebind_row() {
        let rows = OptionsMenu::rows(&Config::default());
        for action in Action::ALL {
            assert!(rows.contains(&OptionRow::Bind(action)), "{action:?}");
        }
    }

    #[test]
    fn toggling_mode_changes_the_config_and_reports_a_change() {
        let mut config = Config::default();
        let mut menu = OptionsMenu::default();
        assert_eq!(config.mode, Mode::Nes);
        assert_eq!(
            menu.navigate(MenuInput::Right, &mut config),
            OptionsOutcome::Changed
        );
        assert_eq!(config.mode, Mode::Modern);
    }

    /// NES has three fewer rows than modern, so a cursor left at the end of the
    /// modern list must not index past the NES one.
    #[test]
    fn the_cursor_survives_the_row_list_shrinking() {
        let mut modern = config(Mode::Modern);
        let mut menu = OptionsMenu {
            selected: OptionsMenu::rows(&modern).len() - 1,
            ..Default::default()
        };

        // The same cursor, now against the shorter list.
        modern.mode = Mode::Nes;
        assert!(matches!(menu.row(&modern), OptionRow::MenuBind(_)));
        menu.navigate(MenuInput::Down, &mut modern);
        assert!(menu.selected < OptionsMenu::rows(&modern).len());
    }

    #[test]
    fn the_nes_start_level_is_clamped_to_its_range() {
        let mut config = config(Mode::Nes);
        let mut menu = OptionsMenu {
            selected: 1, // start level
            ..Default::default()
        };

        for _ in 0..40 {
            menu.navigate(MenuInput::Right, &mut config);
        }
        assert_eq!(
            config.start_level(Mode::Nes),
            *Mode::Nes.start_levels().end()
        );

        for _ in 0..40 {
            menu.navigate(MenuInput::Left, &mut config);
        }
        assert_eq!(config.start_level(Mode::Nes), 0);
    }

    /// Modern levels are 1-based, so the floor is 1 rather than 0.
    #[test]
    fn the_modern_start_level_never_drops_below_one() {
        let mut config = config(Mode::Modern);
        let mut menu = OptionsMenu {
            selected: 1,
            ..Default::default()
        };

        for _ in 0..40 {
            menu.navigate(MenuInput::Left, &mut config);
        }
        assert_eq!(config.start_level(Mode::Modern), 1);
    }

    #[test]
    fn das_and_arr_stay_within_a_usable_range() {
        let mut config = config(Mode::Modern);
        let rows = OptionsMenu::rows(&config);
        let mut menu = OptionsMenu {
            selected: rows.iter().position(|r| *r == OptionRow::Arr).unwrap(),
            ..Default::default()
        };

        for _ in 0..200 {
            menu.navigate(MenuInput::Left, &mut config);
        }
        assert_eq!(config.arr_frames, 1, "a zero-frame ARR would be instant");

        for _ in 0..200 {
            menu.navigate(MenuInput::Right, &mut config);
        }
        assert_eq!(config.arr_frames, MAX_DELAY_FRAMES);
    }

    /// Zero is a real setting here — the instant clear — unlike DAS and ARR.
    #[test]
    fn the_line_clear_delay_steps_from_instant_to_a_second() {
        let mut config = config(Mode::Modern);
        let rows = OptionsMenu::rows(&config);
        let mut menu = OptionsMenu {
            selected: rows
                .iter()
                .position(|r| *r == OptionRow::LineClear)
                .unwrap(),
            ..Default::default()
        };
        assert_eq!(config.line_clear_frames, 0, "instant by default");

        menu.navigate(MenuInput::Right, &mut config);
        assert_eq!(config.line_clear_frames, 5);
        for _ in 0..40 {
            menu.navigate(MenuInput::Right, &mut config);
        }
        assert_eq!(config.line_clear_frames, MAX_LINE_CLEAR_FRAMES);
        for _ in 0..40 {
            menu.navigate(MenuInput::Left, &mut config);
        }
        assert_eq!(config.line_clear_frames, 0);
    }

    /// The visual axes are ruleset-independent, so both modes offer all three.
    #[test]
    fn the_visual_rows_are_offered_in_both_modes() {
        for mode in [Mode::Nes, Mode::Modern] {
            let rows = OptionsMenu::rows(&config(mode));
            for row in [OptionRow::Theme, OptionRow::Skin, OptionRow::Border] {
                assert!(rows.contains(&row), "{row:?} missing in {mode:?}");
            }
        }
    }

    #[test]
    fn the_visual_axes_cycle_and_wrap() {
        let mut config = config(Mode::Nes);
        let rows = OptionsMenu::rows(&config);
        let mut menu = OptionsMenu {
            selected: rows.iter().position(|r| *r == OptionRow::Skin).unwrap(),
            ..Default::default()
        };

        assert_eq!(config.skin, Skin::SolidBlock);
        assert_eq!(
            menu.navigate(MenuInput::Right, &mut config),
            OptionsOutcome::Changed
        );
        assert_eq!(config.skin, Skin::Shaded);

        // All the way round lands back where it started.
        for _ in 1..Skin::ALL.len() {
            menu.navigate(MenuInput::Right, &mut config);
        }
        assert_eq!(config.skin, Skin::SolidBlock);

        // And backwards wraps to the far end.
        menu.navigate(MenuInput::Left, &mut config);
        assert_eq!(config.skin, *Skin::ALL.last().unwrap());
    }

    #[test]
    fn theme_and_border_cycle_independently_of_each_other() {
        let mut config = config(Mode::Nes);
        let rows = OptionsMenu::rows(&config);
        let theme_row = rows.iter().position(|r| *r == OptionRow::Theme).unwrap();
        let mut menu = OptionsMenu {
            selected: theme_row,
            ..Default::default()
        };

        let skin_before = config.skin;
        let border_before = config.border;
        menu.navigate(MenuInput::Right, &mut config);
        assert_eq!(config.theme, Theme::SystemAnsi);
        assert_eq!(config.skin, skin_before, "skin is a separate axis");
        assert_eq!(config.border, border_before, "border is a separate axis");

        menu.selected = rows.iter().position(|r| *r == OptionRow::Border).unwrap();
        menu.navigate(MenuInput::Right, &mut config);
        assert_ne!(config.border, border_before);
        assert_eq!(config.theme, Theme::SystemAnsi, "theme is untouched");
    }

    /// A hand-edited config naming a choice that no longer exists must still be
    /// adjustable rather than sticking.
    #[test]
    fn cycling_from_an_unknown_value_lands_on_the_first_choice() {
        assert_eq!(cycle(9, &[1, 2, 3], true), 2);
        assert_eq!(cycle(9, &[1, 2, 3], false), 3);
    }

    #[test]
    fn confirming_a_bind_row_starts_a_rebind() {
        let mut config = Config::default();
        let rows = OptionsMenu::rows(&config);
        let mut menu = OptionsMenu {
            selected: rows
                .iter()
                .position(|r| *r == OptionRow::Bind(Action::RotateCw))
                .unwrap(),
            ..Default::default()
        };

        menu.navigate(MenuInput::Confirm, &mut config);
        assert_eq!(menu.rebinding, Some(Rebind::Game(Action::RotateCw)));

        let outcome = menu.capture(&key(KeyCode::Char('n')), &mut config);
        assert_eq!(outcome, OptionsOutcome::Changed);
        assert_eq!(menu.rebinding, None);
        assert_eq!(
            config.keymap().action_for(&key(KeyCode::Char('n'))),
            Some(Action::RotateCw)
        );
    }

    /// Stealing a key would leave the other action unreachable, so a conflict is
    /// refused and explained instead.
    #[test]
    fn a_conflicting_key_is_refused_with_a_notice() {
        let mut config = Config::default();
        let mut menu = OptionsMenu {
            rebinding: Some(Rebind::Game(Action::RotateCw)),
            ..Default::default()
        };

        let outcome = menu.capture(&key(KeyCode::Char('q')), &mut config);
        assert_eq!(outcome, OptionsOutcome::Stay);
        assert_eq!(
            menu.rebinding,
            Some(Rebind::Game(Action::RotateCw)),
            "still rebinding"
        );
        assert!(menu.notice.unwrap().contains("Quit"));
        assert_eq!(
            config.keymap().action_for(&key(KeyCode::Char('q'))),
            Some(Action::Quit),
            "the existing binding is untouched"
        );
    }

    /// Rebinding an action to a key it already owns is a no-op, not a conflict.
    #[test]
    fn rebinding_to_its_own_existing_key_is_allowed() {
        let mut config = Config::default();
        let mut menu = OptionsMenu {
            rebinding: Some(Rebind::Game(Action::RotateCw)),
            ..Default::default()
        };
        let outcome = menu.capture(&key(KeyCode::Char('k')), &mut config);
        assert_eq!(outcome, OptionsOutcome::Changed);
        assert_eq!(
            config.keymap().action_for(&key(KeyCode::Char('k'))),
            Some(Action::RotateCw)
        );
    }

    #[test]
    fn escape_cancels_a_rebind_without_changing_anything() {
        let mut config = Config::default();
        let before = config.bindings.clone();
        let mut menu = OptionsMenu {
            rebinding: Some(Rebind::Game(Action::Hold)),
            ..Default::default()
        };

        assert_eq!(
            menu.capture(&key(KeyCode::Esc), &mut config),
            OptionsOutcome::Stay
        );
        assert_eq!(menu.rebinding, None);
        assert_eq!(config.bindings, before);
    }

    #[test]
    fn every_menu_input_gets_a_rebind_row_after_the_gameplay_ones() {
        let rows = OptionsMenu::rows(&Config::default());
        let last_game = rows
            .iter()
            .rposition(|r| matches!(r, OptionRow::Bind(_)))
            .unwrap();
        for input in MenuInput::ALL {
            let at = rows.iter().position(|r| *r == OptionRow::MenuBind(input));
            assert!(at.is_some_and(|at| at > last_game), "{input:?}");
        }
    }

    #[test]
    fn a_menu_input_can_be_rebound() {
        let mut config = Config::default();
        let rows = OptionsMenu::rows(&config);
        let mut menu = OptionsMenu {
            selected: rows
                .iter()
                .position(|r| *r == OptionRow::MenuBind(MenuInput::Confirm))
                .unwrap(),
            ..Default::default()
        };

        menu.navigate(MenuInput::Confirm, &mut config);
        assert_eq!(menu.rebinding, Some(Rebind::Menu(MenuInput::Confirm)));
        let outcome = menu.capture(&key(KeyCode::Char('l')), &mut config);
        assert_eq!(outcome, OptionsOutcome::Changed);
        assert_eq!(
            config.menu_keymap().input_for(&key(KeyCode::Char('l'))),
            Some(MenuInput::Confirm)
        );
    }

    /// The two keymaps are separate namespaces: `k` rotating a piece does not stop
    /// it confirming a menu choice, and binding a gameplay key never touches the
    /// menu ones.
    #[test]
    fn menu_and_gameplay_bindings_only_conflict_within_their_own_kind() {
        let mut config = Config::default();
        let mut menu = OptionsMenu {
            rebinding: Some(Rebind::Menu(MenuInput::Back)),
            ..Default::default()
        };
        // `p` is gameplay's pause, but free on the menus.
        assert_eq!(
            menu.capture(&key(KeyCode::Char('p')), &mut config),
            OptionsOutcome::Changed
        );

        // `w` is already menu up, so menu confirm cannot take it.
        menu.rebinding = Some(Rebind::Menu(MenuInput::Confirm));
        assert_eq!(
            menu.capture(&key(KeyCode::Char('w')), &mut config),
            OptionsOutcome::Stay
        );
        assert!(menu.notice.as_deref().unwrap().contains("Menu up"));
        assert_eq!(
            config.keymap().action_for(&key(KeyCode::Char('p'))),
            Some(Action::Pause),
            "the gameplay binding is untouched"
        );
    }

    /// The arrows, Enter and Esc work on every menu regardless; binding one would
    /// either do nothing or take away the way out.
    #[test]
    fn a_fixed_menu_key_cannot_be_bound() {
        let mut config = Config::default();
        let before = config.menu_bindings.clone();
        let mut menu = OptionsMenu {
            rebinding: Some(Rebind::Menu(MenuInput::Back)),
            ..Default::default()
        };
        assert_eq!(
            menu.capture(&key(KeyCode::Enter), &mut config),
            OptionsOutcome::Stay
        );
        assert_eq!(menu.rebinding, Some(Rebind::Menu(MenuInput::Back)));
        assert_eq!(config.menu_bindings, before);
    }

    /// With every key reported, Shift arrives on its own before the capital it
    /// is part of; it must not become the binding.
    #[test]
    fn a_bare_modifier_is_not_captured() {
        use crossterm::event::ModifierKeyCode;
        let mut config = Config::default();
        let mut menu = OptionsMenu {
            rebinding: Some(Rebind::Game(Action::Hold)),
            ..Default::default()
        };
        let shift = key(KeyCode::Modifier(ModifierKeyCode::LeftShift));
        assert_eq!(menu.capture(&shift, &mut config), OptionsOutcome::Stay);
        assert_eq!(menu.rebinding, Some(Rebind::Game(Action::Hold)));
    }

    #[test]
    fn the_high_score_view_toggles_between_the_two_tables() {
        let mut view = ScoresView::new(Mode::Nes);
        assert!(!view.navigate(MenuInput::Right));
        assert_eq!(view.mode, Mode::Modern);
        assert!(view.navigate(MenuInput::Back), "back leaves the view");
    }

    #[test]
    fn a_qualifying_run_asks_for_a_name() {
        let mut menu = GameOverMenu::new(true, "player");
        assert!(menu.entering);
        menu.name.clear();
        assert!(menu.type_char('A'));
        assert!(menu.type_char('B'));
        assert!(!menu.type_char('\n'), "control characters are ignored");
        assert_eq!(menu.submit().as_deref(), Some("AB"));
        assert!(!menu.entering);
    }

    #[test]
    fn a_run_that_did_not_place_skips_name_entry() {
        let mut menu = GameOverMenu::new(false, "player");
        assert!(!menu.entering);
        assert!(!menu.type_char('A'));
        assert_eq!(menu.submit(), None);
        assert_eq!(menu.navigate(MenuInput::Confirm), Some(GameOverItem::Retry));
    }

    #[test]
    fn the_name_field_is_bounded_and_blanks_fall_back() {
        let mut menu = GameOverMenu::new(true, "");
        for _ in 0..40 {
            menu.type_char('x');
        }
        assert_eq!(menu.name.chars().count(), MAX_NAME);

        let mut blank = GameOverMenu::new(true, "   ");
        assert_eq!(blank.submit().as_deref(), Some("player"));
    }

    #[test]
    fn backspace_only_applies_during_name_entry() {
        let mut menu = GameOverMenu::new(true, "ab");
        assert!(menu.backspace());
        assert_eq!(menu.name, "a");
        menu.submit();
        assert!(!menu.backspace());
    }
}
