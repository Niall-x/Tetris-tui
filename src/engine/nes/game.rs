//! The NES game state machine: spawn, fall, lock, clear, entry delay, repeat.
//!
//! Everything timed here is counted in frames at 60 Hz, matching the tables in
//! `gravity.rs`. There is no lock delay, no hold, no ghost and no hard drop —
//! those are modern-mode features and their absence is the ruleset, not an
//! omission.

use super::{gravity, randomizer::NesRandomizer, rotation, scoring};
use crate::engine::board::{Board, VISIBLE_HEIGHT};
use crate::engine::das::{Das, DasProfile, Direction};
use crate::engine::piece::{ActivePiece, PieceKind, RotationDir};

/// Column the pivot of every spawning piece sits on.
const SPAWN_X: i32 = 5;

/// Hidden rows above the visible field.
///
/// The ROM's playfield is exactly 10x20 and pieces spawn flat on the top row, but
/// the vertical orientations of T/J/L reach one row above their pivot and I reaches
/// two. With no headroom a player could not rotate a piece the moment it appeared,
/// which is not how the real game plays. Two hidden rows reconcile both observed
/// behaviours: pieces still *appear* flat on the first visible row, and rotation
/// works immediately. Flagged in brief.md §14 for playtest confirmation.
const SPAWN_BUFFER_ROWS: usize = 2;
const SPAWN_Y: i32 = SPAWN_BUFFER_ROWS as i32;

/// Rows the piece descends per frame of held Down. Soft drop in NES does not lock
/// the piece on contact and pays one point per row.
///
/// The two-frame rate is the commonly cited figure but has not been confirmed
/// against a disassembly — see brief.md §14.
const SOFT_DROP_FRAMES_PER_ROW: u32 = 2;

const NES_DAS: DasProfile = DasProfile {
    charge_frames: gravity::DAS_CHARGE_FRAMES,
    recharge_frames: gravity::DAS_RECHARGE_FRAMES,
};

/// What the player is holding or pressed this frame. Presses are edge-triggered by
/// the input layer; holds are level-triggered.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameInput {
    pub left: bool,
    pub right: bool,
    pub soft_drop: bool,
    pub rotate_cw: bool,
    pub rotate_ccw: bool,
}

impl FrameInput {
    fn held_direction(&self) -> Option<Direction> {
        match (self.left, self.right) {
            (true, false) => Some(Direction::Left),
            (false, true) => Some(Direction::Right),
            // The ROM resolves a both-held frame in favour of one side; either is
            // unreachable on a real controller's d-pad.
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Counting out entry delay before the next piece appears.
    EntryDelay,
    Falling,
    /// Line-clear animation, before entry delay starts.
    LineClear,
    GameOver,
}

/// Things that happened during a frame, for the UI and sound to react to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameEvents {
    pub piece_locked: bool,
    pub lines_cleared: u32,
    pub level_up: bool,
    pub topped_out: bool,
}

pub struct NesGame {
    board: Board,
    rng: NesRandomizer,
    das: Das,

    current: Option<ActivePiece>,
    next: PieceKind,

    start_level: u32,
    level: u32,
    lines: u32,
    score: u32,
    piece_counts: [u32; 7],

    phase: Phase,
    /// Frames of entry delay left. Set when a piece locks, and counted down
    /// once any line clear it caused has finished.
    entry_delay: u32,
    gravity_counter: u32,
    soft_drop_counter: u32,
    /// Rows the current piece has been soft-dropped, paid out when it locks.
    soft_drop_rows: u32,
    pending_clear: Vec<usize>,
    /// How many line-clear animation steps have run on `pending_clear`.
    clear_step: u32,
    /// The ROM's global frame counter, which the line-clear animation is timed
    /// against.
    frame: u32,
}

impl NesGame {
    pub fn new(start_level: u32) -> Self {
        let mut rng = NesRandomizer::new();
        let first = rng.next_piece();
        let next = rng.next_piece();

        let mut game = Self {
            board: Board::new(SPAWN_BUFFER_ROWS),
            rng,
            das: Das::new(NES_DAS),
            current: None,
            next,
            start_level,
            level: start_level,
            lines: 0,
            score: 0,
            piece_counts: [0; 7],
            phase: Phase::Falling,
            entry_delay: 0,
            gravity_counter: 0,
            soft_drop_counter: 0,
            soft_drop_rows: 0,
            pending_clear: Vec::new(),
            clear_step: 0,
            frame: 0,
        };
        game.spawn(first);
        game
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    #[cfg(test)]
    pub fn board_mut(&mut self) -> &mut Board {
        &mut self.board
    }

    pub fn current(&self) -> Option<ActivePiece> {
        self.current
    }

    pub fn next_piece(&self) -> PieceKind {
        self.next
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn lines(&self) -> u32 {
        self.lines
    }

    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_over(&self) -> bool {
        self.phase == Phase::GameOver
    }

    pub fn piece_count(&self, kind: PieceKind) -> u32 {
        self.piece_counts[Self::stat_index(kind)]
    }

    /// Rows mid-clear, still on the board until the animation finishes.
    pub fn clearing_rows(&self) -> &[usize] {
        &self.pending_clear
    }

    /// How many columns either side of the centre have been erased from the
    /// clearing rows so far, 0 to 4. (The fifth step empties the row, which is
    /// the moment the rows above drop, so it never shows as a step of its own.)
    pub fn clear_step(&self) -> u32 {
        self.clear_step
    }

    /// Whether this frame is one of a Tetris's screen flashes: the ROM turns the
    /// background white on every animation step of a four-line clear.
    pub fn tetris_flash(&self) -> bool {
        self.phase == Phase::LineClear
            && self.pending_clear.len() == 4
            && self.frame.is_multiple_of(gravity::LINE_CLEAR_STEP_FRAMES)
    }

    fn stat_index(kind: PieceKind) -> usize {
        match kind {
            PieceKind::T => 0,
            PieceKind::J => 1,
            PieceKind::Z => 2,
            PieceKind::O => 3,
            PieceKind::S => 4,
            PieceKind::L => 5,
            PieceKind::I => 6,
        }
    }

    fn cells_of(piece: ActivePiece) -> [(i32, i32); 4] {
        rotation::cells_at(piece.kind, piece.state, piece.x, piece.y)
    }

    fn fits(&self, piece: ActivePiece) -> bool {
        !self.board.collides(&Self::cells_of(piece))
    }

    fn spawn(&mut self, kind: PieceKind) {
        let piece = ActivePiece::new(kind, 0, SPAWN_X, SPAWN_Y);
        self.piece_counts[Self::stat_index(kind)] += 1;
        self.gravity_counter = 0;
        self.soft_drop_counter = 0;
        self.soft_drop_rows = 0;

        self.current = Some(piece);
        self.phase = if self.fits(piece) {
            Phase::Falling
        } else {
            // Top out: the stack has reached the spawn rows.
            Phase::GameOver
        };
    }

    /// Advance exactly one frame.
    pub fn tick(&mut self, input: FrameInput) -> FrameEvents {
        let mut events = FrameEvents::default();

        // The ROM steps its PRNG every frame, which is what makes piece order
        // depend on how long the player takes. Keep that true here.
        self.rng.advance_frame();
        self.frame = self.frame.wrapping_add(1);

        match self.phase {
            Phase::GameOver => return events,
            Phase::LineClear => {
                // Steps land on the global frame counter's multiples of four, not
                // at fixed offsets from the lock — hence 17–20 frames in all.
                if self.frame.is_multiple_of(gravity::LINE_CLEAR_STEP_FRAMES) {
                    self.clear_step += 1;
                    if self.clear_step == gravity::LINE_CLEAR_STEPS {
                        self.complete_line_clear(&mut events);
                    }
                }
                return events;
            }
            Phase::EntryDelay => {
                // DAS keeps charging through entry delay, so a held direction
                // carries into the next piece.
                self.update_das_charge_only(input);
                self.entry_delay = self.entry_delay.saturating_sub(1);
                if self.entry_delay == 0 {
                    let kind = self.next;
                    self.next = self.rng.next_piece();
                    self.spawn(kind);
                    if self.phase == Phase::GameOver {
                        events.topped_out = true;
                    }
                }
                return events;
            }
            Phase::Falling => {}
        }

        self.apply_shift(input);
        self.apply_rotation(input);
        self.apply_gravity(input, &mut events);

        events
    }

    fn update_das_charge_only(&mut self, input: FrameInput) {
        // Charge accumulates but there is no piece to move yet.
        let _ = self.das.update(input.held_direction());
    }

    fn apply_shift(&mut self, input: FrameInput) {
        let action = self.das.update(input.held_direction());
        let Some(dir) = action.direction() else {
            return;
        };
        let Some(piece) = self.current else { return };

        let candidate = piece.moved(dir.dx(), 0);
        let moved = self.fits(candidate);
        if moved {
            self.current = Some(candidate);
        }

        // Only repeats feed the counter; a tap leaves it charging from zero.
        if action.is_auto_shift() {
            if moved {
                self.das.on_shift_succeeded();
            } else {
                self.das.on_shift_blocked();
            }
        }
    }

    fn apply_rotation(&mut self, input: FrameInput) {
        let Some(piece) = self.current else { return };
        let dir = match (input.rotate_cw, input.rotate_ccw) {
            (true, false) => Some(RotationDir::Cw),
            (false, true) => Some(RotationDir::Ccw),
            _ => None,
        };
        let Some(dir) = dir else { return };

        let count = rotation::state_count(piece.kind);
        if count <= 1 {
            return;
        }
        let next_state = match dir {
            RotationDir::Cw => (piece.state + 1) % count,
            RotationDir::Ccw => (piece.state + count - 1) % count,
        };

        // No wall kicks: a rotation that does not fit simply does not happen.
        let candidate = piece.with_state(next_state);
        if self.fits(candidate) {
            self.current = Some(candidate);
        }
    }

    fn apply_gravity(&mut self, input: FrameInput, events: &mut FrameEvents) {
        let Some(piece) = self.current else { return };

        let should_step = if input.soft_drop {
            self.soft_drop_counter += 1;
            if self.soft_drop_counter >= SOFT_DROP_FRAMES_PER_ROW {
                self.soft_drop_counter = 0;
                true
            } else {
                false
            }
        } else {
            self.soft_drop_counter = 0;
            self.gravity_counter += 1;
            if self.gravity_counter >= gravity::frames_per_row(self.level) {
                self.gravity_counter = 0;
                true
            } else {
                false
            }
        };

        if !should_step {
            return;
        }

        let dropped = piece.moved(0, 1);
        if self.fits(dropped) {
            self.current = Some(dropped);
            if input.soft_drop {
                self.soft_drop_rows += 1;
            }
        } else {
            self.lock_piece(events);
        }
    }

    fn lock_piece(&mut self, events: &mut FrameEvents) {
        let Some(piece) = self.current.take() else {
            return;
        };
        let cells = Self::cells_of(piece);
        self.board.lock_cells(&cells, piece.kind);
        self.score += scoring::soft_drop_score(self.soft_drop_rows);
        events.piece_locked = true;

        // Entry delay is banded by how high the piece locked within the *visible*
        // field, so drop the hidden rows before measuring. It is decided here,
        // at the lock, whether or not the piece goes on to clear lines: the
        // clear animation runs first, and the delay waits out after it.
        let lowest_row = cells
            .iter()
            .map(|&(_, y)| y)
            .max()
            .unwrap_or(0)
            .max(0)
            .saturating_sub(SPAWN_BUFFER_ROWS as i32) as u32;
        self.entry_delay = gravity::entry_delay_frames(lowest_row, VISIBLE_HEIGHT as u32);

        let full: Vec<usize> = (0..self.board.height())
            .filter(|&y| self.board.is_row_full(y))
            .collect();

        if full.is_empty() {
            self.phase = Phase::EntryDelay;
        } else {
            self.pending_clear = full;
            self.phase = Phase::LineClear;
            self.clear_step = 0;
        }
    }

    fn complete_line_clear(&mut self, events: &mut FrameEvents) {
        let count = self.board.clear_full_lines().len() as u32;
        self.pending_clear.clear();
        self.clear_step = 0;

        self.score += scoring::line_clear_score(count, self.level);
        self.lines += count;

        let new_level = scoring::level_for_lines(self.start_level, self.lines);
        if new_level != self.level {
            self.level = new_level;
            events.level_up = true;
        }

        events.lines_cleared = count;
        // The entry delay was set when the piece locked.
        self.phase = Phase::EntryDelay;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> FrameInput {
        FrameInput::default()
    }

    fn run(game: &mut NesGame, frames: u32, input: FrameInput) -> Vec<FrameEvents> {
        (0..frames).map(|_| game.tick(input)).collect()
    }

    #[test]
    fn starts_with_a_piece_on_the_spawn_row() {
        let game = NesGame::new(0);
        let piece = game.current().expect("a piece should be in play");
        assert_eq!(piece.x, 5);
        assert_eq!(piece.y, SPAWN_Y);
        assert_eq!(game.level(), 0);
        assert_eq!(game.score(), 0);
    }

    /// A piece must be rotatable the instant it appears, which is why the field
    /// carries hidden rows above the visible top.
    #[test]
    fn pieces_can_rotate_immediately_on_spawn() {
        for kind in PieceKind::ALL {
            let mut game = NesGame::new(0);
            game.spawn(kind);
            let before = game.current().unwrap();

            let mut rotate = idle();
            rotate.rotate_cw = true;
            game.tick(rotate);
            let after = game.current().unwrap();

            if rotation::state_count(kind) > 1 {
                assert_ne!(
                    after.state, before.state,
                    "{kind:?} could not rotate at the spawn row"
                );
            }
        }
    }

    #[test]
    fn gravity_steps_at_the_level_rate() {
        let mut game = NesGame::new(0);
        let start_y = game.current().unwrap().y;

        run(&mut game, 47, idle());
        assert_eq!(game.current().unwrap().y, start_y, "must not fall early");

        game.tick(idle());
        assert_eq!(game.current().unwrap().y, start_y + 1);
    }

    #[test]
    fn higher_levels_fall_faster() {
        let mut game = NesGame::new(19);
        let start_y = game.current().unwrap().y;
        run(&mut game, 2, idle());
        assert_eq!(game.current().unwrap().y, start_y + 1);
    }

    #[test]
    fn tapping_a_direction_shifts_once_then_waits_for_das() {
        let mut game = NesGame::new(0);
        let start_x = game.current().unwrap().x;

        let mut left = idle();
        left.left = true;

        game.tick(left);
        assert_eq!(
            game.current().unwrap().x,
            start_x - 1,
            "tap shifts immediately"
        );

        // 15 further frames only charge DAS; nothing moves yet.
        run(&mut game, 15, left);
        assert_eq!(game.current().unwrap().x, start_x - 1, "still charging");

        // The 16th charging frame is the first auto-shift.
        game.tick(left);
        assert_eq!(game.current().unwrap().x, start_x - 2);

        // Repeats then land every 6 frames.
        run(&mut game, 5, left);
        assert_eq!(game.current().unwrap().x, start_x - 2);
        game.tick(left);
        assert_eq!(game.current().unwrap().x, start_x - 3);
    }

    #[test]
    fn soft_drop_descends_faster_than_gravity_and_pays_a_point_per_row() {
        let mut game = NesGame::new(0);
        let start_y = game.current().unwrap().y;

        let mut down = idle();
        down.soft_drop = true;

        run(&mut game, 4, down);
        assert_eq!(game.current().unwrap().y, start_y + 2);

        // Drop it the rest of the way and check the soft-drop points land on lock.
        for _ in 0..200 {
            if game.phase() != Phase::Falling {
                break;
            }
            game.tick(down);
        }
        assert!(game.score() > 0, "soft drop should have paid points");
    }

    #[test]
    fn rotation_is_rejected_rather_than_kicked_at_a_wall() {
        let mut game = NesGame::new(0);
        // Walk the piece to the left wall.
        let mut left = idle();
        left.left = true;
        run(&mut game, 200, left);

        let before = game.current().unwrap();
        assert_eq!(before.x, 1, "I/T pivots stop one column in from the wall");

        let mut rotate = idle();
        rotate.rotate_cw = true;
        game.tick(rotate);
        let after = game.current().unwrap();

        // Either it fit and rotated, or it was refused outright — never nudged.
        assert_eq!(after.x, before.x, "NRS must not wall kick");
    }

    #[test]
    fn locking_a_piece_runs_entry_delay_before_the_next_one() {
        let mut game = NesGame::new(0);
        let mut down = idle();
        down.soft_drop = true;

        let mut locked = false;
        for _ in 0..500 {
            let events = game.tick(down);
            if events.piece_locked {
                locked = true;
                break;
            }
        }
        assert!(locked, "piece should have locked");
        assert_eq!(game.phase(), Phase::EntryDelay);
        assert!(game.current().is_none(), "no piece during entry delay");

        // It comes back once the delay expires.
        run(&mut game, 20, idle());
        assert_eq!(game.phase(), Phase::Falling);
        assert!(game.current().is_some());
    }

    #[test]
    fn das_charge_survives_entry_delay() {
        let mut game = NesGame::new(0);
        let mut down_left = idle();
        down_left.soft_drop = true;
        down_left.left = true;

        for _ in 0..500 {
            if game.tick(down_left).piece_locked {
                break;
            }
        }
        assert_eq!(game.phase(), Phase::EntryDelay);

        // Keep holding left through the delay; the charge must not reset.
        let mut left = idle();
        left.left = true;
        for _ in 0..30 {
            game.tick(left);
            if game.phase() == Phase::Falling {
                break;
            }
        }

        let piece = game.current().expect("next piece spawned");
        let x_at_spawn = piece.x;
        game.tick(left);
        assert!(
            game.current().unwrap().x < x_at_spawn,
            "a charged DAS should shift the new piece immediately"
        );
    }

    #[test]
    fn filling_a_row_clears_it_and_scores() {
        let mut game = NesGame::new(0);
        // Fill row 19 except one column, then let the test drive a lock into it.
        for x in 0..9 {
            game.board.set(x, 19, Some(PieceKind::I));
        }
        assert!(!game.board.is_row_full(19));

        game.board.set(9, 19, Some(PieceKind::I));
        let mut events = FrameEvents::default();
        game.complete_line_clear(&mut events);

        assert_eq!(events.lines_cleared, 1);
        assert_eq!(game.lines(), 1);
        assert_eq!(game.score(), 40, "single at level 0");
    }

    /// Fill the bottom `rows` rows and lock a piece up out of the way, with the
    /// global frame counter at `frame`. Returns the game mid line clear.
    fn lock_into_full_rows(rows: i32, frame: u32) -> NesGame {
        let mut game = NesGame::new(0);
        let bottom = game.board.height() as i32 - 1;
        for y in bottom - rows + 1..=bottom {
            for x in 0..10 {
                game.board.set(x, y, Some(PieceKind::I));
            }
        }
        game.frame = frame;
        let mut events = FrameEvents::default();
        game.lock_piece(&mut events);
        assert_eq!(game.phase(), Phase::LineClear);
        game
    }

    fn frames_until_cleared(game: &mut NesGame) -> u32 {
        let mut frames = 0;
        while game.phase() == Phase::LineClear {
            game.tick(idle());
            frames += 1;
        }
        frames
    }

    /// Steps land on multiples of four of the global frame counter, so where in
    /// that cycle the piece locked decides whether the clear takes 17, 18, 19 or
    /// 20 frames.
    #[test]
    fn a_line_clear_takes_17_to_20_frames_depending_on_the_lock_frame() {
        let durations: Vec<u32> = (0..4)
            .map(|frame| frames_until_cleared(&mut lock_into_full_rows(1, frame)))
            .collect();
        assert_eq!(durations, vec![20, 19, 18, 17]);
    }

    /// The rows stay on the board, erasing from the centre outward, and only drop
    /// once the animation is over.
    #[test]
    fn cleared_rows_erase_outward_and_only_then_drop() {
        let mut game = lock_into_full_rows(1, 0);
        let bottom = game.board.height() - 1;
        let mut steps_seen = vec![game.clear_step()];

        while game.phase() == Phase::LineClear {
            assert!(game.board.is_row_full(bottom), "not dropped mid-animation");
            assert_eq!(game.clearing_rows(), &[bottom]);
            game.tick(idle());
            if steps_seen.last() != Some(&game.clear_step()) {
                steps_seen.push(game.clear_step());
            }
        }

        assert_eq!(steps_seen, vec![0, 1, 2, 3, 4, 0]);
        assert!(game.board.is_row_empty(bottom), "dropped once it finished");
        assert_eq!(game.lines(), 1);
        assert_eq!(game.phase(), Phase::EntryDelay);
    }

    /// The entry delay after a clear follows where the piece locked, not where
    /// the cleared rows were: this piece locks at the very top while the bottom
    /// row clears, so it waits the top band's 18 frames rather than the
    /// bottom's 10.
    #[test]
    fn entry_delay_after_a_clear_is_banded_by_the_lock_not_the_cleared_rows() {
        let mut game = lock_into_full_rows(1, 0);
        frames_until_cleared(&mut game);
        assert_eq!(game.phase(), Phase::EntryDelay);

        let mut frames = 0;
        while game.phase() == Phase::EntryDelay {
            game.tick(idle());
            frames += 1;
        }
        assert_eq!(frames, 18);
    }

    #[test]
    fn only_a_tetris_flashes_and_only_on_animation_steps() {
        let mut single = lock_into_full_rows(1, 0);
        let mut tetris = lock_into_full_rows(4, 0);
        let mut flashes = 0;
        while tetris.phase() == Phase::LineClear {
            single.tick(idle());
            tetris.tick(idle());
            assert!(!single.tetris_flash());
            if tetris.tetris_flash() {
                flashes += 1;
                assert_eq!(tetris.frame % 4, 0);
            }
        }
        assert_eq!(flashes, 4, "one per visible step");
    }

    #[test]
    fn stacking_to_the_spawn_row_ends_the_game() {
        let mut game = NesGame::new(0);
        for y in 0..20 {
            for x in 0..10 {
                game.board.set(x, y, Some(PieceKind::I));
            }
        }
        // Force the next spawn.
        game.spawn(PieceKind::T);
        assert_eq!(game.phase(), Phase::GameOver);
        assert!(game.is_over());
        assert_eq!(game.tick(idle()), FrameEvents::default(), "no further play");
    }
}
