//! Lock delay with the modern "Extended Placement" reset cap.
//!
//! A grounded piece gets 500ms (30 ticks at 60 Hz) before it locks. Any successful
//! move or rotation refreshes that timer, but only 15 times — without the cap a
//! player could hold a piece in play forever, which is the old "Infinity"
//! behaviour that current Guideline games removed.
//!
//! The one escape from the cap is falling: reaching a new lowest row hands back a
//! full budget, so a piece descending through a wide board is never starved of
//! placement time.
//!
//! Source: tetris.wiki/Lock_delay, tetris.wiki/Infinity.

pub const LOCK_DELAY_FRAMES: u32 = 30;
pub const MAX_RESETS: u32 = 15;

#[derive(Debug, Clone)]
pub struct LockDelay {
    delay_frames: u32,
    max_resets: u32,
    /// Frames left before the piece locks; only counts down while grounded.
    timer: u32,
    resets_used: u32,
    lowest_row: i32,
    grounded: bool,
}

impl LockDelay {
    pub fn new(delay_frames: u32, max_resets: u32) -> Self {
        Self {
            delay_frames,
            max_resets,
            timer: delay_frames,
            resets_used: 0,
            lowest_row: i32::MIN,
            grounded: false,
        }
    }

    pub fn guideline() -> Self {
        Self::new(LOCK_DELAY_FRAMES, MAX_RESETS)
    }

    pub fn resets_used(&self) -> u32 {
        self.resets_used
    }

    pub fn is_grounded(&self) -> bool {
        self.grounded
    }

    pub fn frames_remaining(&self) -> u32 {
        self.timer
    }

    /// Start tracking a freshly spawned piece.
    pub fn on_spawn(&mut self, row: i32) {
        self.timer = self.delay_frames;
        self.resets_used = 0;
        self.lowest_row = row;
        self.grounded = false;
    }

    /// Advance one frame. Returns true when the piece should lock now.
    pub fn tick(&mut self, grounded: bool) -> bool {
        if !grounded {
            // Airborne pieces hold a full timer, ready for when they land.
            self.grounded = false;
            self.timer = self.delay_frames;
            return false;
        }

        self.grounded = true;
        self.timer = self.timer.saturating_sub(1);
        self.timer == 0
    }

    /// Called after any successful move or rotation, with the piece's lowest row.
    ///
    /// Falling to a new low refreshes the whole budget; otherwise a reset is spent,
    /// and once they run out the timer keeps draining.
    pub fn on_piece_moved(&mut self, lowest_row: i32) {
        if lowest_row > self.lowest_row {
            self.lowest_row = lowest_row;
            self.resets_used = 0;
            self.timer = self.delay_frames;
            return;
        }

        if self.resets_used < self.max_resets {
            self.resets_used += 1;
            self.timer = self.delay_frames;
        }
    }
}

impl Default for LockDelay {
    fn default() -> Self {
        Self::guideline()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_airborne_piece_never_locks() {
        let mut lock = LockDelay::guideline();
        lock.on_spawn(0);
        for _ in 0..1_000 {
            assert!(!lock.tick(false));
        }
    }

    #[test]
    fn a_grounded_piece_locks_after_the_full_delay() {
        let mut lock = LockDelay::guideline();
        lock.on_spawn(0);

        for frame in 1..LOCK_DELAY_FRAMES {
            assert!(!lock.tick(true), "locked early on frame {frame}");
        }
        assert!(lock.tick(true), "should lock on frame {LOCK_DELAY_FRAMES}");
    }

    #[test]
    fn landing_again_restores_the_full_delay() {
        let mut lock = LockDelay::guideline();
        lock.on_spawn(0);
        for _ in 0..20 {
            lock.tick(true);
        }
        // Lifted off the stack again (a kick, say).
        lock.tick(false);
        assert_eq!(lock.frames_remaining(), LOCK_DELAY_FRAMES);
    }

    #[test]
    fn moving_refreshes_the_timer() {
        let mut lock = LockDelay::guideline();
        lock.on_spawn(5);
        for _ in 0..25 {
            lock.tick(true);
        }
        assert!(lock.frames_remaining() < LOCK_DELAY_FRAMES);

        lock.on_piece_moved(5);
        assert_eq!(lock.frames_remaining(), LOCK_DELAY_FRAMES);
        assert_eq!(lock.resets_used(), 1);
    }

    /// The cap is what stops a piece being held in play indefinitely.
    #[test]
    fn the_sixteenth_reset_is_refused_and_the_piece_locks() {
        let mut lock = LockDelay::guideline();
        lock.on_spawn(5);

        for i in 0..MAX_RESETS {
            for _ in 0..29 {
                assert!(!lock.tick(true));
            }
            lock.on_piece_moved(5);
            assert_eq!(lock.resets_used(), i + 1);
        }

        // Budget spent: further moves no longer refresh the timer.
        for _ in 0..29 {
            lock.on_piece_moved(5);
            assert!(!lock.tick(true));
        }
        lock.on_piece_moved(5);
        assert!(lock.tick(true), "must lock once the reset budget is gone");
        assert_eq!(lock.resets_used(), MAX_RESETS);
    }

    /// Falling to a new lowest row is the one thing that escapes the cap.
    #[test]
    fn reaching_a_new_lowest_row_restores_the_budget() {
        let mut lock = LockDelay::guideline();
        lock.on_spawn(5);

        for _ in 0..MAX_RESETS {
            lock.on_piece_moved(5);
        }
        assert_eq!(lock.resets_used(), MAX_RESETS);

        lock.on_piece_moved(6);
        assert_eq!(lock.resets_used(), 0, "descending refreshes the budget");
        assert_eq!(lock.frames_remaining(), LOCK_DELAY_FRAMES);
    }

    #[test]
    fn sideways_movement_at_the_same_depth_does_not_refresh_the_budget() {
        let mut lock = LockDelay::guideline();
        lock.on_spawn(5);
        lock.on_piece_moved(4); // moved up, e.g. a kick
        assert_eq!(lock.resets_used(), 1);
    }

    #[test]
    fn a_new_piece_starts_with_a_clean_slate() {
        let mut lock = LockDelay::guideline();
        lock.on_spawn(5);
        for _ in 0..MAX_RESETS {
            lock.on_piece_moved(5);
        }
        lock.tick(true);

        lock.on_spawn(0);
        assert_eq!(lock.resets_used(), 0);
        assert_eq!(lock.frames_remaining(), LOCK_DELAY_FRAMES);
        assert!(!lock.is_grounded());
    }
}
