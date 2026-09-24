//! Player intent, decoupled from the keys that produce it.
//!
//! Game logic only ever asks about `Action`s, never about key codes, which is what
//! makes rebinding possible without touching the engine.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    MoveLeft,
    MoveRight,
    SoftDrop,
    /// Modern mode only; NES has no hard drop.
    HardDrop,
    RotateCw,
    RotateCcw,
    /// Modern mode only; NES has no hold slot.
    Hold,
    Pause,
    Quit,
}

impl Action {
    pub const ALL: [Action; 9] = [
        Action::MoveLeft,
        Action::MoveRight,
        Action::SoftDrop,
        Action::HardDrop,
        Action::RotateCw,
        Action::RotateCcw,
        Action::Hold,
        Action::Pause,
        Action::Quit,
    ];

    /// Stable name used in the config file. Changing one of these orphans a
    /// user's existing binding, so they are deliberately not derived.
    pub fn name(self) -> &'static str {
        match self {
            Action::MoveLeft => "move_left",
            Action::MoveRight => "move_right",
            Action::SoftDrop => "soft_drop",
            Action::HardDrop => "hard_drop",
            Action::RotateCw => "rotate_cw",
            Action::RotateCcw => "rotate_ccw",
            Action::Hold => "hold",
            Action::Pause => "pause",
            Action::Quit => "quit",
        }
    }

    /// How this reads in a controls list.
    pub fn label(self) -> &'static str {
        match self {
            Action::MoveLeft => "Move left",
            Action::MoveRight => "Move right",
            Action::SoftDrop => "Soft drop",
            Action::HardDrop => "Hard drop",
            Action::RotateCw => "Rotate CW",
            Action::RotateCcw => "Rotate CCW",
            Action::Hold => "Hold",
            Action::Pause => "Pause",
            Action::Quit => "Quit",
        }
    }

    /// Actions whose timing depends on how long the key is held, as opposed to
    /// firing once per press.
    pub fn is_held(self) -> bool {
        matches!(
            self,
            Action::MoveLeft | Action::MoveRight | Action::SoftDrop
        )
    }
}

/// Menu navigation, the menus' counterpart to `Action`.
///
/// Menus have their own bindings rather than borrowing the gameplay ones, so what
/// a key does on a menu never changes behind the player's back when they rebind a
/// gameplay action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuInput {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
}

impl MenuInput {
    pub const ALL: [MenuInput; 6] = [
        MenuInput::Up,
        MenuInput::Down,
        MenuInput::Left,
        MenuInput::Right,
        MenuInput::Confirm,
        MenuInput::Back,
    ];

    /// Stable name used in the config file, like `Action::name`.
    pub fn name(self) -> &'static str {
        match self {
            MenuInput::Up => "up",
            MenuInput::Down => "down",
            MenuInput::Left => "left",
            MenuInput::Right => "right",
            MenuInput::Confirm => "confirm",
            MenuInput::Back => "back",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MenuInput::Up => "Menu up",
            MenuInput::Down => "Menu down",
            MenuInput::Left => "Menu left",
            MenuInput::Right => "Menu right",
            MenuInput::Confirm => "Menu confirm",
            MenuInput::Back => "Menu back",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_input_names_are_unique() {
        let mut names: Vec<&str> = MenuInput::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), MenuInput::ALL.len());
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two actions share a config name");
    }
}
