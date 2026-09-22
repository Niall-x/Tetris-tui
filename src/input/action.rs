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

    pub fn from_name(name: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.name() == name)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_round_trips_through_its_config_name() {
        for action in Action::ALL {
            assert_eq!(Action::from_name(action.name()), Some(action));
        }
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two actions share a config name");
    }

    #[test]
    fn unknown_names_are_rejected() {
        assert_eq!(Action::from_name("teleport"), None);
    }
}
