//! Key bindings. Rebinding is a matter of editing these maps, which is why nothing
//! downstream matches on key codes directly.
//!
//! There are two: `Keymap` for play and `MenuKeymap` for the menus. They are
//! separate namespaces, so one key can rotate a piece and confirm a menu choice.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

use super::action::{Action, MenuInput};

/// Letters are bound in lower case, so Caps Lock or Shift must not unbind them.
fn normalize(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
        other => other,
    }
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: HashMap<KeyCode, Action>,
}

impl Keymap {
    pub fn empty() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    pub fn bind(&mut self, code: KeyCode, action: Action) {
        self.bindings.insert(normalize(code), action);
    }

    /// The action a key performs. Modifiers are ignored: no binding is a
    /// chord, and some terminals report Shift alongside every capital. Ctrl-C
    /// never gets this far; the app takes it as quit first.
    pub fn action_for(&self, event: &KeyEvent) -> Option<Action> {
        self.bindings.get(&normalize(event.code)).copied()
    }

    /// Every key currently bound to `action`, for display in a rebinding UI.
    pub fn keys_for(&self, action: Action) -> Vec<KeyCode> {
        self.bindings
            .iter()
            .filter(|(_, &a)| a == action)
            .map(|(&code, _)| code)
            .collect()
    }
}

impl Default for Keymap {
    fn default() -> Self {
        let mut map = Self::empty();

        // WASD, with the arrows mirroring it for the other hand.
        map.bind(KeyCode::Char('a'), Action::MoveLeft);
        map.bind(KeyCode::Left, Action::MoveLeft);
        map.bind(KeyCode::Char('d'), Action::MoveRight);
        map.bind(KeyCode::Right, Action::MoveRight);
        map.bind(KeyCode::Char('s'), Action::SoftDrop);
        map.bind(KeyCode::Down, Action::SoftDrop);
        map.bind(KeyCode::Char('w'), Action::HardDrop);
        map.bind(KeyCode::Up, Action::HardDrop);

        // The NES pad's B and A, in pad order under the right hand.
        map.bind(KeyCode::Char('j'), Action::RotateCcw);
        map.bind(KeyCode::Char('k'), Action::RotateCw);

        map.bind(KeyCode::Char(' '), Action::Hold);

        map.bind(KeyCode::Char('p'), Action::Pause);
        map.bind(KeyCode::Esc, Action::Pause);
        map.bind(KeyCode::Char('q'), Action::Quit);

        map
    }
}

/// Keys that drive every menu whatever the bindings say. They are the way back
/// out of a menu keymap that has been rebound into a corner, so they cannot be
/// rebound themselves.
pub const FIXED_MENU_KEYS: [(KeyCode, MenuInput); 6] = [
    (KeyCode::Up, MenuInput::Up),
    (KeyCode::Down, MenuInput::Down),
    (KeyCode::Left, MenuInput::Left),
    (KeyCode::Right, MenuInput::Right),
    (KeyCode::Enter, MenuInput::Confirm),
    (KeyCode::Esc, MenuInput::Back),
];

#[derive(Debug, Clone)]
pub struct MenuKeymap {
    bindings: HashMap<KeyCode, MenuInput>,
}

impl MenuKeymap {
    pub fn empty() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    pub fn bind(&mut self, code: KeyCode, input: MenuInput) {
        self.bindings.insert(normalize(code), input);
    }

    pub fn is_fixed(code: KeyCode) -> bool {
        FIXED_MENU_KEYS.iter().any(|&(fixed, _)| fixed == code)
    }

    /// Menu navigation from a key event, fixed keys first.
    ///
    /// Chords are deliberately not navigation: in raw mode a terminal delivers
    /// Ctrl-D as `Char('d')` with a modifier, and taking that for "right" would let
    /// stray control input walk the menu. Shift is allowed through because
    /// terminals report it alongside ordinary capitals.
    pub fn input_for(&self, event: &KeyEvent) -> Option<MenuInput> {
        if event
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return None;
        }
        let code = normalize(event.code);
        FIXED_MENU_KEYS
            .iter()
            .find(|&&(fixed, _)| fixed == code)
            .map(|&(_, input)| input)
            .or_else(|| self.bindings.get(&code).copied())
    }

    /// The rebindable keys for `input`; the fixed ones are not included.
    pub fn keys_for(&self, input: MenuInput) -> Vec<KeyCode> {
        self.bindings
            .iter()
            .filter(|(_, &i)| i == input)
            .map(|(&code, _)| code)
            .collect()
    }
}

impl Default for MenuKeymap {
    fn default() -> Self {
        let mut map = Self::empty();
        map.bind(KeyCode::Char('w'), MenuInput::Up);
        map.bind(KeyCode::Char('s'), MenuInput::Down);
        map.bind(KeyCode::Char('a'), MenuInput::Left);
        map.bind(KeyCode::Char('d'), MenuInput::Right);
        // The rotate keys double as confirm and back.
        map.bind(KeyCode::Char('j'), MenuInput::Confirm);
        map.bind(KeyCode::Char('k'), MenuInput::Back);
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    #[test]
    fn default_bindings_cover_every_nes_action() {
        let map = Keymap::default();
        for action in [
            Action::MoveLeft,
            Action::MoveRight,
            Action::SoftDrop,
            Action::HardDrop,
            Action::RotateCw,
            Action::RotateCcw,
            Action::Hold,
            Action::Pause,
            Action::Quit,
        ] {
            assert!(
                !map.keys_for(action).is_empty(),
                "{action:?} has no default binding"
            );
        }
    }

    #[test]
    fn the_defaults_are_wasd_with_the_arrows_mirroring_it() {
        let map = Keymap::default();
        for (letter, arrow, action) in [
            ('a', KeyCode::Left, Action::MoveLeft),
            ('d', KeyCode::Right, Action::MoveRight),
            ('s', KeyCode::Down, Action::SoftDrop),
            ('w', KeyCode::Up, Action::HardDrop),
        ] {
            assert_eq!(map.action_for(&key(KeyCode::Char(letter))), Some(action));
            assert_eq!(map.action_for(&key(arrow)), Some(action));
        }
        assert_eq!(
            map.action_for(&key(KeyCode::Char('j'))),
            Some(Action::RotateCcw)
        );
        assert_eq!(
            map.action_for(&key(KeyCode::Char('k'))),
            Some(Action::RotateCw)
        );
        assert_eq!(map.action_for(&key(KeyCode::Char(' '))), Some(Action::Hold));
    }

    /// Caps Lock, or a terminal reporting Shift with the capital, must not unbind
    /// a letter.
    #[test]
    fn capitals_resolve_to_the_lower_case_binding() {
        let map = Keymap::default();
        let shifted = KeyEvent {
            modifiers: KeyModifiers::SHIFT,
            ..key(KeyCode::Char('A'))
        };
        assert_eq!(map.action_for(&shifted), Some(Action::MoveLeft));
        assert_eq!(
            MenuKeymap::default().input_for(&key(KeyCode::Char('W'))),
            Some(MenuInput::Up)
        );
    }

    #[test]
    fn the_menu_defaults_are_wasd_with_the_rotate_keys_to_confirm_and_go_back() {
        let menu = MenuKeymap::default();
        for (code, input) in [
            (KeyCode::Char('w'), MenuInput::Up),
            (KeyCode::Char('a'), MenuInput::Left),
            (KeyCode::Char('s'), MenuInput::Down),
            (KeyCode::Char('d'), MenuInput::Right),
            (KeyCode::Char('j'), MenuInput::Confirm),
            (KeyCode::Char('k'), MenuInput::Back),
        ] {
            assert_eq!(menu.input_for(&key(code)), Some(input), "{code:?}");
        }
    }

    /// The fixed keys are the way out of a menu keymap bound into a corner, so
    /// they work even from an empty one.
    #[test]
    fn the_fixed_menu_keys_work_whatever_is_bound() {
        let menu = MenuKeymap::empty();
        for (code, input) in FIXED_MENU_KEYS {
            assert_eq!(menu.input_for(&key(code)), Some(input));
            assert!(MenuKeymap::is_fixed(code));
        }
        assert_eq!(menu.input_for(&key(KeyCode::Char('w'))), None);
    }

    /// Ctrl-D reaches a raw-mode terminal as `Char('d')`, which is also the "right"
    /// key: without the modifier check it would walk the menu on its own.
    #[test]
    fn control_chords_are_not_menu_navigation() {
        let menu = MenuKeymap::default();
        for code in [KeyCode::Char('d'), KeyCode::Char('j'), KeyCode::Down] {
            let chord = KeyEvent {
                modifiers: KeyModifiers::CONTROL,
                ..key(code)
            };
            assert_eq!(menu.input_for(&chord), None, "{code:?}");
        }
    }

    #[test]
    fn unbound_keys_resolve_to_nothing() {
        let map = Keymap::default();
        assert_eq!(map.action_for(&key(KeyCode::Char('!'))), None);
    }

    #[test]
    fn rebinding_replaces_the_previous_action_for_that_key() {
        let mut map = Keymap::default();
        map.bind(KeyCode::Char('a'), Action::RotateCcw);
        assert_eq!(
            map.action_for(&key(KeyCode::Char('a'))),
            Some(Action::RotateCcw)
        );
        // The other binding for MoveLeft survives.
        assert_eq!(map.action_for(&key(KeyCode::Left)), Some(Action::MoveLeft));
    }
}
