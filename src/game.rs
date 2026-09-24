//! One handle over both rulesets.
//!
//! The two modes deliberately do not share rule logic, but the UI does need a
//! single thing to draw and feed input to. This is that seam, and nothing more:
//! it dispatches, it does not decide. Note `cells_of` in particular — the two
//! modes use different rotation systems, so even "which cells does this piece
//! occupy" has to come from the mode.

use crate::engine::board::Board;
use crate::engine::modern::game::{
    FrameInput as ModernInput, ModernGame, Phase as ModernPhase, Settings as ModernSettings,
};
use crate::engine::modern::srs;
use crate::engine::nes::game::{FrameInput as NesInput, NesGame, Phase as NesPhase};
use crate::engine::nes::rotation as nrs;
use crate::engine::piece::{ActivePiece, PieceKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    Nes,
    Modern,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Nes => "NES",
            Mode::Modern => "MODERN",
        }
    }

    /// NES has no hard drop, no hold and no ghost piece.
    pub fn has_hard_drop(self) -> bool {
        self == Mode::Modern
    }

    pub fn has_hold(self) -> bool {
        self == Mode::Modern
    }

    /// The levels a run can start on. NES offers 0-29: the level-select screen's
    /// 0-19 plus the 20-29 the A+select trick reaches. Modern is 1-based, and past
    /// 20 the Guideline curve is already beyond 1G, so a higher start stops meaning
    /// anything.
    pub fn start_levels(self) -> std::ops::RangeInclusive<u32> {
        match self {
            Mode::Nes => 0..=29,
            Mode::Modern => 1..=20,
        }
    }
}

/// The superset of both modes' inputs. NES ignores the fields it has no concept of.
#[derive(Debug, Clone, Copy, Default)]
pub struct Input {
    pub left: bool,
    pub right: bool,
    pub soft_drop: bool,
    pub hard_drop: bool,
    pub rotate_cw: bool,
    pub rotate_ccw: bool,
    pub hold: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Events {
    pub piece_locked: bool,
    pub lines_cleared: u32,
    /// The placement was a T-spin, full or mini. Only modern mode has them.
    pub tspin: bool,
    pub level_up: bool,
    pub topped_out: bool,
}

pub enum Game {
    Nes(Box<NesGame>),
    Modern(Box<ModernGame>),
}

impl Game {
    pub fn new(mode: Mode, start_level: u32) -> Self {
        Self::with_settings(mode, start_level, ModernSettings::default())
    }

    /// `settings` only affects modern mode: NES timing is fixed by its ruleset.
    pub fn with_settings(mode: Mode, start_level: u32, settings: ModernSettings) -> Self {
        match mode {
            Mode::Nes => Game::Nes(Box::new(NesGame::new(start_level))),
            Mode::Modern => {
                Game::Modern(Box::new(ModernGame::with_settings(start_level, settings)))
            }
        }
    }

    pub fn mode(&self) -> Mode {
        match self {
            Game::Nes(_) => Mode::Nes,
            Game::Modern(_) => Mode::Modern,
        }
    }

    pub fn tick(&mut self, input: Input) -> Events {
        match self {
            Game::Nes(game) => {
                let events = game.tick(NesInput {
                    left: input.left,
                    right: input.right,
                    soft_drop: input.soft_drop,
                    rotate_cw: input.rotate_cw,
                    rotate_ccw: input.rotate_ccw,
                });
                Events {
                    piece_locked: events.piece_locked,
                    lines_cleared: events.lines_cleared,
                    tspin: false,
                    level_up: events.level_up,
                    topped_out: events.topped_out,
                }
            }
            Game::Modern(game) => {
                let events = game.tick(ModernInput {
                    left: input.left,
                    right: input.right,
                    soft_drop: input.soft_drop,
                    hard_drop: input.hard_drop,
                    rotate_cw: input.rotate_cw,
                    rotate_ccw: input.rotate_ccw,
                    hold: input.hold,
                });
                Events {
                    piece_locked: events.piece_locked,
                    lines_cleared: events.lines_cleared,
                    tspin: events.tspin.is_some(),
                    level_up: events.level_up,
                    topped_out: events.topped_out,
                }
            }
        }
    }

    pub fn board(&self) -> &Board {
        match self {
            Game::Nes(game) => game.board(),
            Game::Modern(game) => game.board(),
        }
    }

    /// Lets tests arrange a stack without playing a whole game into it.
    #[cfg(test)]
    pub fn board_mut(&mut self) -> &mut Board {
        match self {
            Game::Nes(game) => game.board_mut(),
            Game::Modern(game) => game.board_mut(),
        }
    }

    pub fn current(&self) -> Option<ActivePiece> {
        match self {
            Game::Nes(game) => game.current(),
            Game::Modern(game) => game.current(),
        }
    }

    pub fn ghost(&self) -> Option<ActivePiece> {
        match self {
            Game::Nes(_) => None,
            Game::Modern(game) => game.ghost(),
        }
    }

    pub fn hold_piece(&self) -> Option<PieceKind> {
        match self {
            Game::Nes(_) => None,
            Game::Modern(game) => game.hold_piece(),
        }
    }

    /// Upcoming pieces: NES shows exactly one, modern shows a queue.
    pub fn preview(&self) -> Vec<PieceKind> {
        match self {
            Game::Nes(game) => vec![game.next_piece()],
            Game::Modern(game) => game.preview(),
        }
    }

    /// Cells a piece occupies, resolved through the mode's own rotation system.
    pub fn cells_of(&self, piece: ActivePiece) -> [(i32, i32); 4] {
        match self {
            Game::Nes(_) => nrs::cells_at(piece.kind, piece.state, piece.x, piece.y),
            Game::Modern(_) => srs::cells_at(piece.kind, piece.state, piece.x, piece.y),
        }
    }

    pub fn score(&self) -> u64 {
        match self {
            Game::Nes(game) => game.score() as u64,
            Game::Modern(game) => game.score(),
        }
    }

    pub fn lines(&self) -> u32 {
        match self {
            Game::Nes(game) => game.lines(),
            Game::Modern(game) => game.lines(),
        }
    }

    pub fn level(&self) -> u32 {
        match self {
            Game::Nes(game) => game.level(),
            Game::Modern(game) => game.level(),
        }
    }

    pub fn is_over(&self) -> bool {
        match self {
            Game::Nes(game) => game.phase() == NesPhase::GameOver,
            Game::Modern(game) => game.phase() == ModernPhase::GameOver,
        }
    }

    /// Rows currently flashing from a line clear; modern mode clears instantly.
    pub fn clearing_rows(&self) -> &[usize] {
        match self {
            Game::Nes(game) => game.clearing_rows(),
            Game::Modern(_) => &[],
        }
    }

    /// NES tracks how many of each piece it has dealt; modern does not.
    pub fn piece_count(&self, kind: PieceKind) -> Option<u32> {
        match self {
            Game::Nes(game) => Some(game.piece_count(kind)),
            Game::Modern(_) => None,
        }
    }

    pub fn combo(&self) -> Option<u32> {
        match self {
            Game::Nes(_) => None,
            Game::Modern(game) => Some(game.combo()),
        }
    }

    pub fn back_to_back(&self) -> bool {
        match self {
            Game::Nes(_) => false,
            Game::Modern(game) => game.back_to_back(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_modes_start_with_a_piece_in_play() {
        for mode in [Mode::Nes, Mode::Modern] {
            let game = Game::new(mode, 1);
            assert!(game.current().is_some(), "{mode:?}");
            assert!(!game.preview().is_empty(), "{mode:?}");
            assert!(!game.is_over(), "{mode:?}");
        }
    }

    #[test]
    fn nes_exposes_no_modern_only_features() {
        let game = Game::new(Mode::Nes, 0);
        assert!(game.ghost().is_none());
        assert!(game.hold_piece().is_none());
        assert!(game.combo().is_none());
        assert_eq!(game.preview().len(), 1, "NES shows a single next piece");
        assert!(game.piece_count(PieceKind::T).is_some());
    }

    #[test]
    fn modern_exposes_hold_ghost_and_a_queue() {
        let game = Game::new(Mode::Modern, 1);
        assert!(game.ghost().is_some());
        assert_eq!(game.preview().len(), 5);
        assert_eq!(game.combo(), Some(0));
        assert!(game.piece_count(PieceKind::T).is_none());
    }

    /// Each mode must resolve piece cells through its own rotation system.
    #[test]
    fn piece_cells_come_from_the_modes_rotation_system() {
        let piece = ActivePiece::new(PieceKind::T, 0, 3, 20);
        let nes = Game::new(Mode::Nes, 0);
        let modern = Game::new(Mode::Modern, 1);
        assert_ne!(
            nes.cells_of(piece),
            modern.cells_of(piece),
            "NRS and SRS anchor pieces differently"
        );
    }

    #[test]
    fn hard_drop_only_reaches_the_mode_that_has_one() {
        let input = Input {
            hard_drop: true,
            ..Default::default()
        };

        let mut nes = Game::new(Mode::Nes, 0);
        let before = nes.current().unwrap();
        nes.tick(input);
        assert_eq!(
            nes.current().unwrap().y,
            before.y,
            "NES has no hard drop, so the piece must not move"
        );

        let mut modern = Game::new(Mode::Modern, 1);
        let events = modern.tick(input);
        assert!(events.piece_locked, "modern should hard drop and lock");
    }

    #[test]
    fn both_modes_advance_under_gravity() {
        for mode in [Mode::Nes, Mode::Modern] {
            let mut game = Game::new(mode, 1);
            let start = game.current().unwrap().y;
            for _ in 0..70 {
                game.tick(Input::default());
            }
            assert!(
                game.current().unwrap().y > start || game.lines() > 0,
                "{mode:?} piece never fell"
            );
        }
    }
}
