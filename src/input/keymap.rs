//! Key bindings. Rebinding is a matter of editing this map, which is why nothing
//! downstream matches on key codes directly.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

use super::action::Action;

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: HashMap<(KeyCode, KeyModifiers), Action>,
}

impl Keymap {
    pub fn empty() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    pub fn bind(&mut self, code: KeyCode, modifiers: KeyModifiers, action: Action) {
        self.bindings.insert((code, modifiers), action);
    }

    pub fn action_for(&self, event: &KeyEvent) -> Option<Action> {
        self.bindings
            .get(&(event.code, event.modifiers))
            .copied()
            // Shift is reported alongside uppercase letters on some terminals;
            // fall back to an unmodified lookup rather than losing the binding.
            .or_else(|| {
                self.bindings
                    .get(&(event.code, KeyModifiers::NONE))
                    .copied()
            })
    }

    /// Every key currently bound to `action`, for display in a rebinding UI.
    pub fn keys_for(&self, action: Action) -> Vec<KeyCode> {
        self.bindings
            .iter()
            .filter(|(_, &a)| a == action)
            .map(|(&(code, _), _)| code)
            .collect()
    }
}

impl Default for Keymap {
    fn default() -> Self {
        let mut map = Self::empty();
        let none = KeyModifiers::NONE;

        map.bind(KeyCode::Left, none, Action::MoveLeft);
        map.bind(KeyCode::Char('h'), none, Action::MoveLeft);
        map.bind(KeyCode::Right, none, Action::MoveRight);
        map.bind(KeyCode::Char('l'), none, Action::MoveRight);
        map.bind(KeyCode::Down, none, Action::SoftDrop);
        map.bind(KeyCode::Char('j'), none, Action::SoftDrop);

        // NES pads A clockwise, B counter-clockwise; X/Z mirrors that on a keyboard.
        map.bind(KeyCode::Char('x'), none, Action::RotateCw);
        map.bind(KeyCode::Up, none, Action::RotateCw);
        map.bind(KeyCode::Char('z'), none, Action::RotateCcw);

        map.bind(KeyCode::Char(' '), none, Action::HardDrop);
        map.bind(KeyCode::Char('c'), none, Action::Hold);
        map.bind(KeyCode::Tab, none, Action::Hold);

        map.bind(KeyCode::Char('p'), none, Action::Pause);
        map.bind(KeyCode::Esc, none, Action::Pause);
        map.bind(KeyCode::Char('q'), none, Action::Quit);

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
    fn resolves_arrows_and_vim_keys_to_the_same_actions() {
        let map = Keymap::default();
        assert_eq!(map.action_for(&key(KeyCode::Left)), Some(Action::MoveLeft));
        assert_eq!(
            map.action_for(&key(KeyCode::Char('h'))),
            Some(Action::MoveLeft)
        );
        assert_eq!(
            map.action_for(&key(KeyCode::Char('x'))),
            Some(Action::RotateCw)
        );
        assert_eq!(
            map.action_for(&key(KeyCode::Char('z'))),
            Some(Action::RotateCcw)
        );
    }

    #[test]
    fn unbound_keys_resolve_to_nothing() {
        let map = Keymap::default();
        assert_eq!(map.action_for(&key(KeyCode::Char('!'))), None);
    }

    #[test]
    fn rebinding_replaces_the_previous_action_for_that_key() {
        let mut map = Keymap::default();
        map.bind(KeyCode::Char('h'), KeyModifiers::NONE, Action::RotateCcw);
        assert_eq!(
            map.action_for(&key(KeyCode::Char('h'))),
            Some(Action::RotateCcw)
        );
        // The other binding for MoveLeft survives.
        assert_eq!(map.action_for(&key(KeyCode::Left)), Some(Action::MoveLeft));
    }
}
