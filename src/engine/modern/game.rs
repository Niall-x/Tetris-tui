//! The modern (Guideline) game state machine.
//!
//! Where NES counts frames from ROM tables, this counts seconds: gravity follows
//! the Guideline curve, lock delay is 500ms, and DAS/ARR are player preferences
//! rather than fixed constants. Hold, ghost piece and hard drop all exist here and
//! all are absent from NES.

use super::bag::{SevenBag, MAX_PREVIEW};
use super::lock_delay::LockDelay;
use super::scoring::{Placement, ScoreState};
use super::srs;
use super::tspin::{self, TSpin};
use crate::engine::board::Board;
use crate::engine::das::{Das, DasProfile, Direction};
use crate::engine::piece::{ActivePiece, PieceKind, RotationDir};

/// Hidden rows above the visible field, where pieces spawn.
const BUFFER_ROWS: usize = 20;

/// Soft drop descends this many times faster than the current gravity.
const SOFT_DROP_FACTOR: f32 = 20.0;

/// Guideline defaults. Unlike NES's fixed timing these are meant to be tunable,
/// so they live in `Settings` rather than as constants in the engine.
pub const DEFAULT_DAS_FRAMES: u32 = 8;
pub const DEFAULT_ARR_FRAMES: u32 = 2;
/// Five, as in Puyo Puyo Tetris and TETR.IO's default.
pub const DEFAULT_PREVIEWS: usize = 5;

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub das_frames: u32,
    pub arr_frames: u32,
    pub ghost: bool,
    /// Frames the cleared rows stay on screen, animating, before the rows above
    /// drop and the next piece spawns. Zero clears instantly. Guideline games
    /// differ on this, so it is the player's choice.
    pub line_clear_frames: u32,
    /// How many upcoming pieces the queue shows, 1 to `MAX_PREVIEW`. Guideline
    /// games range from one to six, and several let the player choose.
    pub previews: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            das_frames: DEFAULT_DAS_FRAMES,
            arr_frames: DEFAULT_ARR_FRAMES,
            ghost: true,
            line_clear_frames: 0,
            previews: DEFAULT_PREVIEWS,
        }
    }
}

impl Settings {
    fn das_profile(&self) -> DasProfile {
        DasProfile {
            charge_frames: self.das_frames,
            recharge_frames: self.das_frames.saturating_sub(self.arr_frames.max(1)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameInput {
    pub left: bool,
    pub right: bool,
    pub soft_drop: bool,
    pub hard_drop: bool,
    pub rotate_cw: bool,
    pub rotate_ccw: bool,
    pub hold: bool,
}

impl FrameInput {
    fn held_direction(&self) -> Option<Direction> {
        match (self.left, self.right) {
            (true, false) => Some(Direction::Left),
            (false, true) => Some(Direction::Right),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Falling,
    /// Cleared rows animating out, when a line-clear delay is set.
    LineClear,
    GameOver,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameEvents {
    pub piece_locked: bool,
    pub lines_cleared: u32,
    pub tspin: Option<TSpin>,
    pub perfect_clear: bool,
    pub level_up: bool,
    pub topped_out: bool,
}

/// Seconds per row at `level`, per the Guideline curve
/// `(0.8 - (level - 1) * 0.007) ^ (level - 1)`.
pub fn seconds_per_row(level: u32) -> f32 {
    let n = level.max(1) as f32 - 1.0;
    (0.8 - n * 0.007).max(0.000_001).powf(n)
}

pub struct ModernGame {
    board: Board,
    bag: SevenBag,
    das: Das,
    lock: LockDelay,
    settings: Settings,

    current: Option<ActivePiece>,
    hold: Option<PieceKind>,
    /// Hold is once per piece, until the piece locks.
    hold_used: bool,
    /// Pieces dealt from the bag so far, indexed as `PieceKind::ALL`. A piece
    /// coming back out of hold is not dealt again.
    piece_counts: [u32; 7],

    score: ScoreState,
    start_level: u32,
    level: u32,
    phase: Phase,

    /// Fractional rows owed by gravity, so sub-frame speeds still work.
    gravity_debt: f32,
    /// Whether the last successful action was a rotation, and which kick it used —
    /// together these decide T-spins.
    rotated_last: bool,
    last_kick_index: usize,
    soft_drop_cells: u32,
    /// Rows mid-clear while a line-clear delay runs, and how far through it is.
    pending_clear: Vec<usize>,
    clear_elapsed: u32,
    /// How long this clear lasts, fixed when the piece locks.
    clear_frames: u32,
}

impl ModernGame {
    pub fn new(start_level: u32) -> Self {
        Self::with_settings(start_level, Settings::default())
    }

    pub fn with_settings(start_level: u32, settings: Settings) -> Self {
        let mut game = Self {
            board: Board::new(BUFFER_ROWS),
            bag: SevenBag::new(),
            das: Das::new(settings.das_profile()),
            lock: LockDelay::guideline(),
            settings,
            current: None,
            hold: None,
            hold_used: false,
            piece_counts: [0; 7],
            score: ScoreState::new(),
            start_level: start_level.max(1),
            level: start_level.max(1),
            phase: Phase::Falling,
            gravity_debt: 0.0,
            rotated_last: false,
            last_kick_index: 0,
            soft_drop_cells: 0,
            pending_clear: Vec::new(),
            clear_elapsed: 0,
            clear_frames: 0,
        };
        let first = game.deal();
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

    pub fn hold_piece(&self) -> Option<PieceKind> {
        self.hold
    }

    pub fn preview(&self) -> Vec<PieceKind> {
        let mut queue = self.bag.preview();
        queue.truncate(self.settings.previews.clamp(1, MAX_PREVIEW));
        queue
    }

    pub fn piece_count(&self, kind: PieceKind) -> u32 {
        self.piece_counts[Self::count_index(kind)]
    }

    fn count_index(kind: PieceKind) -> usize {
        PieceKind::ALL
            .iter()
            .position(|&k| k == kind)
            .expect("every kind is in ALL")
    }

    /// The next piece from the bag, counted as it is dealt.
    fn deal(&mut self) -> PieceKind {
        let kind = self.bag.next_piece();
        self.piece_counts[Self::count_index(kind)] += 1;
        kind
    }

    pub fn score(&self) -> u64 {
        self.score.score
    }

    pub fn lines(&self) -> u32 {
        self.score.lines
    }

    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn combo(&self) -> u32 {
        self.score.combo
    }

    pub fn back_to_back(&self) -> bool {
        self.score.back_to_back
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_over(&self) -> bool {
        self.phase == Phase::GameOver
    }

    fn cells_of(piece: ActivePiece) -> [(i32, i32); 4] {
        srs::cells_at(piece.kind, piece.state, piece.x, piece.y)
    }

    fn fits(&self, piece: ActivePiece) -> bool {
        !self.board.collides(&Self::cells_of(piece))
    }

    /// Spawn columns: everything is centred, which puts the 3-wide pieces at
    /// columns 3..5, I at 3..6 and O at 4..5.
    fn spawn_x(kind: PieceKind) -> i32 {
        match kind {
            PieceKind::O => 4,
            _ => 3,
        }
    }

    fn spawn_piece_at(kind: PieceKind) -> ActivePiece {
        // Place the piece so its topmost filled row sits on the first visible row.
        let top = srs::cells(kind, 0).iter().map(|c| c.1).min().unwrap_or(0);
        ActivePiece::new(kind, 0, Self::spawn_x(kind), BUFFER_ROWS as i32 - top)
    }

    fn spawn(&mut self, kind: PieceKind) {
        let piece = Self::spawn_piece_at(kind);
        self.hold_used = false;
        self.gravity_debt = 0.0;
        self.rotated_last = false;
        self.last_kick_index = 0;
        self.soft_drop_cells = 0;
        self.lock.on_spawn(Self::lowest_row(piece));

        self.current = Some(piece);
        if !self.fits(piece) {
            // Block out: the stack has reached the spawn rows.
            self.phase = Phase::GameOver;
        }
    }

    fn lowest_row(piece: ActivePiece) -> i32 {
        Self::cells_of(piece)
            .iter()
            .map(|&(_, y)| y)
            .max()
            .unwrap_or(0)
    }

    fn grounded(&self, piece: ActivePiece) -> bool {
        !self.fits(piece.moved(0, 1))
    }

    /// Where the active piece would land if dropped now.
    pub fn ghost(&self) -> Option<ActivePiece> {
        if !self.settings.ghost {
            return None;
        }
        let mut piece = self.current?;
        while self.fits(piece.moved(0, 1)) {
            piece = piece.moved(0, 1);
        }
        Some(piece)
    }

    pub fn tick(&mut self, input: FrameInput) -> FrameEvents {
        let mut events = FrameEvents::default();
        match self.phase {
            Phase::GameOver => return events,
            Phase::LineClear => {
                // No piece to move, but a held direction keeps charging so the
                // next piece can auto-shift straight away.
                let _ = self.das.update(input.held_direction());
                self.clear_elapsed += 1;
                if self.clear_elapsed >= self.clear_frames {
                    self.complete_line_clear(&mut events);
                }
                return events;
            }
            Phase::Falling => {}
        }

        if input.hold {
            self.apply_hold();
        }
        self.apply_shift(input);
        self.apply_rotation(input);

        if input.hard_drop {
            self.apply_hard_drop(&mut events);
            return events;
        }

        self.apply_gravity(input);

        // Lock delay only runs while the piece is resting on something.
        if let Some(piece) = self.current {
            if self.lock.tick(self.grounded(piece)) {
                self.lock_piece(&mut events);
            }
        }

        events
    }

    fn apply_hold(&mut self) {
        if self.hold_used {
            return;
        }
        let Some(piece) = self.current else { return };

        let incoming = match self.hold.replace(piece.kind) {
            Some(held) => held,
            None => self.deal(),
        };

        self.spawn(incoming);
        // `spawn` clears the flag, but a hold is spent for this piece.
        self.hold_used = true;
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
            self.rotated_last = false;
            self.lock.on_piece_moved(Self::lowest_row(candidate));
        }

        if action.is_auto_shift() {
            if moved {
                self.das.on_shift_succeeded();
            } else {
                self.das.on_shift_blocked();
            }
        }
    }

    fn apply_rotation(&mut self, input: FrameInput) {
        let dir = match (input.rotate_cw, input.rotate_ccw) {
            (true, false) => Some(RotationDir::Cw),
            (false, true) => Some(RotationDir::Ccw),
            _ => None,
        };
        let (Some(dir), Some(piece)) = (dir, self.current) else {
            return;
        };

        if let Some(result) = srs::rotate(&self.board, piece, dir) {
            self.current = Some(result.piece);
            self.rotated_last = true;
            self.last_kick_index = result.kick_index;
            self.lock.on_piece_moved(Self::lowest_row(result.piece));
        }
    }

    fn apply_hard_drop(&mut self, events: &mut FrameEvents) {
        let Some(piece) = self.current else { return };
        let mut dropped = piece;
        let mut cells = 0;
        while self.fits(dropped.moved(0, 1)) {
            dropped = dropped.moved(0, 1);
            cells += 1;
        }

        if cells > 0 {
            // A hard drop is a translation, so it cancels any pending T-spin.
            self.rotated_last = false;
        }
        self.current = Some(dropped);
        self.score.add_hard_drop(cells);
        self.lock_piece(events);
    }

    fn apply_gravity(&mut self, input: FrameInput) {
        let Some(piece) = self.current else { return };

        let mut rows_per_frame = 1.0 / (seconds_per_row(self.level) * 60.0);
        if input.soft_drop {
            rows_per_frame *= SOFT_DROP_FACTOR;
        }
        self.gravity_debt += rows_per_frame;

        let mut piece = piece;
        while self.gravity_debt >= 1.0 {
            self.gravity_debt -= 1.0;
            if self.fits(piece.moved(0, 1)) {
                piece = piece.moved(0, 1);
                self.rotated_last = false;
                self.lock.on_piece_moved(Self::lowest_row(piece));
                if input.soft_drop {
                    self.soft_drop_cells += 1;
                }
            } else {
                self.gravity_debt = 0.0;
                break;
            }
        }
        // Landing on the stack still has to wait out lock delay, so nothing locks
        // here; `tick` runs the lock timer next.
        self.current = Some(piece);
    }

    fn lock_piece(&mut self, events: &mut FrameEvents) {
        let Some(piece) = self.current.take() else {
            return;
        };

        let spin = tspin::detect(&self.board, piece, self.rotated_last, self.last_kick_index);
        let cells = Self::cells_of(piece);

        // Lock out: a piece that locks entirely above the visible field ends it.
        let locked_out = cells.iter().all(|&(_, y)| y < BUFFER_ROWS as i32);

        self.board.lock_cells(&cells, piece.kind);
        self.score.add_soft_drop(self.soft_drop_cells);
        events.piece_locked = true;

        let full: Vec<usize> = (0..self.board.height())
            .filter(|&y| self.board.is_row_full(y))
            .collect();

        // The placement is scored on the lock frame either way, so the perfect
        // clear is judged on a copy with the rows already gone.
        let perfect = !full.is_empty() && {
            let mut after = self.board.clone();
            after.clear_full_lines();
            after.stack_height() == 0
        };
        let delay = if full.is_empty() {
            0
        } else {
            self.settings.line_clear_frames
        };
        if delay == 0 {
            self.board.clear_full_lines();
        }

        let placement = Placement {
            lines: full.len() as u32,
            tspin: spin,
            perfect_clear: perfect,
        };
        self.score.apply(placement, self.level);

        events.lines_cleared = placement.lines;
        events.perfect_clear = perfect;
        if spin != TSpin::None {
            events.tspin = Some(spin);
        }

        let new_level = super::scoring::level_for_lines(self.start_level, self.score.lines);
        if new_level != self.level {
            self.level = new_level;
            events.level_up = true;
        }

        if locked_out {
            self.phase = Phase::GameOver;
            events.topped_out = true;
            return;
        }

        if delay > 0 {
            self.pending_clear = full;
            self.clear_elapsed = 0;
            self.clear_frames = delay;
            self.phase = Phase::LineClear;
            return;
        }

        self.spawn_next(events);
    }

    fn complete_line_clear(&mut self, events: &mut FrameEvents) {
        self.board.clear_full_lines();
        self.pending_clear.clear();
        self.clear_elapsed = 0;
        self.phase = Phase::Falling;
        self.spawn_next(events);
    }

    fn spawn_next(&mut self, events: &mut FrameEvents) {
        let next = self.deal();
        self.spawn(next);
        if self.phase == Phase::GameOver {
            events.topped_out = true;
        }
    }

    /// Rows mid-clear while a line-clear delay runs; empty otherwise.
    pub fn clearing_rows(&self) -> &[usize] {
        &self.pending_clear
    }

    /// How far the clear animation is through, as columns erased either side of
    /// the centre, 0 to 4 — the same outward erase NES uses, spread evenly over
    /// whatever delay the player chose.
    pub fn clear_step(&self) -> u32 {
        if self.pending_clear.is_empty() {
            return 0;
        }
        let total = self.clear_frames.max(1);
        (self.clear_elapsed * 5 / total).min(4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> FrameInput {
        FrameInput::default()
    }

    fn drop_input() -> FrameInput {
        FrameInput {
            hard_drop: true,
            ..Default::default()
        }
    }

    #[test]
    fn gravity_follows_the_guideline_curve() {
        // Level 1 is one second per row.
        assert!((seconds_per_row(1) - 1.0).abs() < 1e-6);
        // It accelerates monotonically.
        for level in 1..20 {
            assert!(
                seconds_per_row(level + 1) < seconds_per_row(level),
                "level {level} should be slower than {}",
                level + 1
            );
        }
        // By the high teens a piece is falling faster than one row per frame.
        assert!(seconds_per_row(20) < 1.0 / 60.0);
    }

    #[test]
    fn a_new_game_has_a_piece_a_preview_and_no_hold() {
        let game = ModernGame::new(1);
        assert!(game.current().is_some());
        assert_eq!(game.preview().len(), DEFAULT_PREVIEWS);
        assert!(game.hold_piece().is_none());
        assert_eq!(game.level(), 1);
    }

    #[test]
    fn pieces_spawn_on_the_first_visible_row() {
        for kind in PieceKind::ALL {
            let piece = ModernGame::spawn_piece_at(kind);
            let top = srs::cells_at(kind, 0, piece.x, piece.y)
                .iter()
                .map(|c| c.1)
                .min()
                .unwrap();
            assert_eq!(top, BUFFER_ROWS as i32, "{kind:?} spawned at the wrong row");
        }
    }

    #[test]
    fn the_ghost_sits_at_the_bottom_of_the_column() {
        let game = ModernGame::new(1);
        let ghost = game.ghost().expect("ghost enabled by default");
        let current = game.current().unwrap();
        assert_eq!(ghost.state, current.state);
        assert_eq!(ghost.x, current.x);
        assert!(ghost.y > current.y);
        // And it must be resting on the floor.
        assert!(!game.fits(ghost.moved(0, 1)));
    }

    #[test]
    fn the_ghost_can_be_turned_off() {
        let settings = Settings {
            ghost: false,
            ..Default::default()
        };
        let game = ModernGame::with_settings(1, settings);
        assert!(game.ghost().is_none());
    }

    #[test]
    fn hard_drop_locks_the_piece_and_pays_two_points_a_cell() {
        let mut game = ModernGame::new(1);
        let start = game.current().unwrap();
        let ghost = game.ghost().unwrap();
        let distance = (ghost.y - start.y) as u64;

        let events = game.tick(drop_input());
        assert!(events.piece_locked);
        assert_eq!(game.score(), distance * 2);
        // A new piece is already in play.
        assert!(game.current().is_some());
    }

    #[test]
    fn hold_swaps_the_piece_and_only_works_once_per_piece() {
        let mut game = ModernGame::new(1);
        let first = game.current().unwrap().kind;

        let mut hold = idle();
        hold.hold = true;
        game.tick(hold);

        assert_eq!(game.hold_piece(), Some(first));
        let second = game.current().unwrap().kind;
        assert_ne!(second, first, "a different piece should be in play");

        // A second hold before locking is refused.
        game.tick(hold);
        assert_eq!(game.hold_piece(), Some(first));
        assert_eq!(game.current().unwrap().kind, second);
    }

    #[test]
    fn the_preview_shows_as_many_pieces_as_asked_for() {
        for previews in 1..=MAX_PREVIEW {
            let settings = Settings {
                previews,
                ..Default::default()
            };
            let game = ModernGame::with_settings(1, settings);
            assert_eq!(game.preview().len(), previews);
        }
        // A hand-edited count outside the range is pulled back into it.
        for (asked, shown) in [(0, 1), (99, MAX_PREVIEW)] {
            let settings = Settings {
                previews: asked,
                ..Default::default()
            };
            assert_eq!(
                ModernGame::with_settings(1, settings).preview().len(),
                shown
            );
        }
    }

    /// The statistics count pieces as the bag deals them, so a piece swapped out
    /// of hold is not counted a second time.
    #[test]
    fn piece_counts_follow_the_bag_not_the_hold_slot() {
        let total = |game: &ModernGame| {
            PieceKind::ALL
                .iter()
                .map(|&k| game.piece_count(k))
                .sum::<u32>()
        };
        let mut game = ModernGame::new(1);
        let first = game.current().unwrap().kind;
        assert_eq!(game.piece_count(first), 1);
        assert_eq!(total(&game), 1);

        let mut hold = idle();
        hold.hold = true;
        // The first hold deals a fresh piece from the bag.
        game.tick(hold);
        assert_eq!(total(&game), 2);

        // Locking deals the next; holding again brings `first` back out of the
        // slot, which deals nothing.
        game.tick(drop_input());
        assert_eq!(total(&game), 3);
        game.tick(hold);
        assert_eq!(game.current().unwrap().kind, first);
        assert_eq!(total(&game), 3);
    }

    #[test]
    fn hold_becomes_available_again_after_a_lock() {
        let mut game = ModernGame::new(1);
        let mut hold = idle();
        hold.hold = true;

        game.tick(hold);
        let held = game.hold_piece().unwrap();

        game.tick(drop_input());
        game.tick(hold);
        assert_ne!(
            game.hold_piece(),
            Some(held),
            "hold should have swapped again"
        );
    }

    #[test]
    fn a_grounded_piece_waits_out_lock_delay_before_locking() {
        let mut game = ModernGame::new(1);
        // Drop it to the floor without locking.
        let mut piece = game.current().unwrap();
        while game.fits(piece.moved(0, 1)) {
            piece = piece.moved(0, 1);
        }
        game.current = Some(piece);

        for frame in 0..29 {
            let events = game.tick(idle());
            assert!(!events.piece_locked, "locked early on frame {frame}");
        }
        assert!(game.tick(idle()).piece_locked);
    }

    #[test]
    fn filling_a_row_clears_it_and_scores() {
        let mut game = ModernGame::new(1);
        let bottom = game.board.height() as i32 - 1;
        for x in 0..10 {
            game.board.set(x, bottom, Some(PieceKind::I));
        }
        let mut events = FrameEvents::default();
        game.lock_piece(&mut events);

        assert_eq!(events.lines_cleared, 1);
        assert_eq!(game.lines(), 1);
        assert!(game.score() >= 100);
    }

    /// Drop the active piece onto the stack, so locking it leaves room to spawn.
    fn drop_current(game: &mut ModernGame) {
        let mut piece = game.current.unwrap();
        while game.fits(piece.moved(0, 1)) {
            piece = piece.moved(0, 1);
        }
        game.current = Some(piece);
    }

    fn with_line_clear_delay(frames: u32) -> ModernGame {
        ModernGame::with_settings(
            1,
            Settings {
                line_clear_frames: frames,
                ..Default::default()
            },
        )
    }

    #[test]
    fn with_no_delay_the_rows_drop_and_the_next_piece_spawns_at_once() {
        let mut game = ModernGame::new(1);
        let bottom = game.board.height() as i32 - 1;
        for x in 0..10 {
            game.board.set(x, bottom, Some(PieceKind::I));
        }
        drop_current(&mut game);
        game.lock_piece(&mut FrameEvents::default());
        assert_eq!(game.phase(), Phase::Falling);
        assert!(game.clearing_rows().is_empty());
        assert!(game.current().is_some());
    }

    /// With a delay set, the placement scores on the lock frame but the rows stay
    /// up, animating, and the next piece waits until they have gone.
    #[test]
    fn a_line_clear_delay_holds_the_rows_and_the_next_piece() {
        let mut game = with_line_clear_delay(20);
        let bottom = game.board.height() - 1;
        for x in 0..10 {
            game.board.set(x, bottom as i32, Some(PieceKind::I));
        }
        let mut events = FrameEvents::default();
        drop_current(&mut game);
        game.lock_piece(&mut events);

        assert_eq!(events.lines_cleared, 1, "scored on the lock frame");
        assert_eq!(game.lines(), 1);
        assert_eq!(game.phase(), Phase::LineClear);
        assert!(game.current().is_none());

        let mut last_step = 0;
        // The field holds for the 20 frames set.
        for _ in 0..19 {
            game.tick(idle());
            assert!(game.board.is_row_full(bottom), "still showing");
            assert!(
                game.clear_step() >= last_step,
                "the erase only moves outward"
            );
            last_step = game.clear_step();
        }
        assert_eq!(last_step, 4);

        game.tick(idle());
        assert_eq!(game.phase(), Phase::Falling);
        assert!(!game.board.is_row_full(bottom), "the full row has gone");
        assert!(game.current().is_some(), "the next piece spawns");
    }

    /// Perfect-clear detection has to look past rows that are still on screen:
    /// with or without a delay, the same placement is a perfect clear.
    #[test]
    fn a_delayed_clear_still_counts_a_perfect_clear() {
        for delay in [0, 10] {
            let mut game = with_line_clear_delay(delay);
            let bottom = game.board.height() as i32 - 1;
            for x in 0..6 {
                game.board.set(x, bottom, Some(PieceKind::I));
            }
            // A flat I finishing the row, and nothing else on the board.
            let piece = (0..4)
                .map(|dy| ActivePiece::new(PieceKind::I, 0, 6, bottom - dy))
                .find(|&p| {
                    ModernGame::cells_of(p)
                        .iter()
                        .all(|&(x, y)| y == bottom && x >= 6)
                })
                .expect("some offset puts a flat I on the bottom row");
            game.current = Some(piece);

            let mut events = FrameEvents::default();
            game.lock_piece(&mut events);
            assert_eq!(events.lines_cleared, 1, "delay {delay}");
            assert!(events.perfect_clear, "delay {delay}");
        }
    }

    #[test]
    fn stacking_into_the_spawn_rows_ends_the_game() {
        let mut game = ModernGame::new(1);
        for y in 0..game.board.height() as i32 {
            for x in 0..10 {
                game.board.set(x, y, Some(PieceKind::I));
            }
        }
        game.spawn(PieceKind::T);
        assert!(game.is_over());
        assert_eq!(game.tick(idle()), FrameEvents::default());
    }

    #[test]
    fn levels_advance_every_ten_lines() {
        let mut game = ModernGame::new(1);
        game.score.lines = 9;
        let mut events = FrameEvents::default();
        game.lock_piece(&mut events);
        assert_eq!(game.level(), 1);

        game.score.lines = 10;
        let mut events = FrameEvents::default();
        game.lock_piece(&mut events);
        assert_eq!(game.level(), 2);
        assert!(events.level_up);
    }

    /// A hard drop is a translation, so it must cancel a pending T-spin.
    #[test]
    fn dropping_after_a_rotation_cancels_the_spin() {
        let mut game = ModernGame::new(1);
        game.rotated_last = true;
        game.tick(drop_input());
        assert!(!game.rotated_last);
    }

    #[test]
    fn soft_drop_descends_faster_than_gravity() {
        let mut fast = ModernGame::new(1);
        let mut slow = ModernGame::new(1);
        let start = fast.current().unwrap().y;

        let mut soft = idle();
        soft.soft_drop = true;
        for _ in 0..10 {
            fast.tick(soft);
            slow.tick(idle());
        }

        assert!(
            fast.current().unwrap().y > slow.current().unwrap().y,
            "soft drop should outpace gravity"
        );
        assert!(fast.current().unwrap().y > start);
    }
}
