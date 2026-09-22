//! Guideline scoring: line clears, T-spins, back-to-back, combos, perfect clears.
//!
//! Note the difference from NES, which multiplies by (level + 1): here the
//! multiplier is the level itself, with level counted from 1.
//!
//! Source: tetris.wiki/Scoring, harddrop.com/wiki/T-Spin_Guide.

use super::tspin::TSpin;

/// A placement's outcome, everything scoring needs to know about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub lines: u32,
    pub tspin: TSpin,
    /// Board is completely empty after the clear.
    pub perfect_clear: bool,
}

/// Running state that scoring carries between placements.
#[derive(Debug, Clone, Default)]
pub struct ScoreState {
    pub score: u64,
    pub lines: u32,
    pub combo: u32,
    pub back_to_back: bool,
}

/// Base value of a placement before the level multiplier.
fn base_value(placement: Placement) -> u64 {
    match (placement.tspin, placement.lines) {
        (TSpin::None, 0) => 0,
        (TSpin::None, 1) => 100,
        (TSpin::None, 2) => 300,
        (TSpin::None, 3) => 500,
        (TSpin::None, _) => 800,

        (TSpin::Mini, 0) => 100,
        (TSpin::Mini, 1) => 200,
        (TSpin::Mini, _) => 400,

        (TSpin::Full, 0) => 400,
        (TSpin::Full, 1) => 800,
        (TSpin::Full, 2) => 1200,
        (TSpin::Full, _) => 1600,
    }
}

fn perfect_clear_bonus(lines: u32, back_to_back_tetris: bool) -> u64 {
    match lines {
        1 => 800,
        2 => 1200,
        3 => 1800,
        4 if back_to_back_tetris => 3200,
        4 => 2000,
        _ => 0,
    }
}

/// Clears that extend a back-to-back chain: a tetris, or any T-spin that cleared
/// lines. Everything else with lines breaks it.
fn is_difficult(placement: Placement) -> bool {
    placement.lines == 4 || (placement.tspin != TSpin::None && placement.lines > 0)
}

impl ScoreState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a placement and return the points it earned.
    pub fn apply(&mut self, placement: Placement, level: u32) -> u64 {
        let level = level.max(1) as u64;
        let mut points = base_value(placement) * level;

        if placement.lines > 0 {
            let difficult = is_difficult(placement);
            // Whether a chain was already running when this placement landed. Both
            // the multiplier and the perfect-clear bonus key off the *previous*
            // state, so read it before updating.
            let chain_was_live = self.back_to_back;

            if difficult && chain_was_live {
                // The chain multiplies the clear, not the drop points.
                points = points * 3 / 2;
            }

            // A T-spin that cleared nothing leaves the chain untouched; it neither
            // extends nor breaks it.
            self.back_to_back = difficult;

            points += 50 * self.combo as u64 * level;
            self.combo += 1;
            self.lines += placement.lines;

            if placement.perfect_clear {
                let b2b_tetris = placement.lines == 4 && chain_was_live;
                points += perfect_clear_bonus(placement.lines, b2b_tetris) * level;
            }
        } else {
            self.combo = 0;
        }

        self.score += points;
        points
    }

    /// Soft drop pays one point per cell, hard drop two. Neither is level-scaled
    /// and neither touches the back-to-back chain.
    pub fn add_soft_drop(&mut self, cells: u32) {
        self.score += cells as u64;
    }

    pub fn add_hard_drop(&mut self, cells: u32) {
        self.score += 2 * cells as u64;
    }
}

/// Guideline levels advance every ten lines.
pub fn level_for_lines(start_level: u32, total_lines: u32) -> u32 {
    start_level + total_lines / 10
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear(lines: u32) -> Placement {
        Placement {
            lines,
            tspin: TSpin::None,
            perfect_clear: false,
        }
    }

    fn spin(lines: u32, tspin: TSpin) -> Placement {
        Placement {
            lines,
            tspin,
            perfect_clear: false,
        }
    }

    #[test]
    fn line_clear_values_match_the_published_table() {
        let cases = [(1, 100), (2, 300), (3, 500), (4, 800)];
        for (lines, expected) in cases {
            let mut state = ScoreState::new();
            assert_eq!(state.apply(clear(lines), 1), expected, "{lines} lines");
        }
    }

    #[test]
    fn t_spin_values_match_the_published_table() {
        let cases = [
            (TSpin::Mini, 0, 100),
            (TSpin::Mini, 1, 200),
            (TSpin::Mini, 2, 400),
            (TSpin::Full, 0, 400),
            (TSpin::Full, 1, 800),
            (TSpin::Full, 2, 1200),
            (TSpin::Full, 3, 1600),
        ];
        for (kind, lines, expected) in cases {
            let mut state = ScoreState::new();
            assert_eq!(
                state.apply(spin(lines, kind), 1),
                expected,
                "{kind:?} {lines}"
            );
        }
    }

    #[test]
    fn scores_scale_with_the_level() {
        let mut state = ScoreState::new();
        assert_eq!(state.apply(clear(4), 5), 4000);
    }

    #[test]
    fn a_second_tetris_gets_the_back_to_back_bonus() {
        let mut state = ScoreState::new();
        assert_eq!(
            state.apply(clear(4), 1),
            800,
            "first tetris is unmultiplied"
        );
        // Isolate the chain bonus from the combo the second clear also earns.
        state.combo = 0;
        assert_eq!(state.apply(clear(4), 1), 1200, "second is 800 * 1.5");
    }

    /// Back-to-back tetrises are also consecutive clears, so both bonuses apply.
    #[test]
    fn chained_clears_earn_the_combo_as_well() {
        let mut state = ScoreState::new();
        state.apply(clear(4), 1);
        assert_eq!(state.apply(clear(4), 1), 1200 + 50);
    }

    #[test]
    fn a_plain_clear_breaks_the_chain() {
        let mut state = ScoreState::new();
        state.apply(clear(4), 1);
        assert!(state.back_to_back);

        state.combo = 0;
        state.apply(clear(2), 1);
        assert!(!state.back_to_back, "a double should break the chain");

        state.combo = 0;
        assert_eq!(state.apply(clear(4), 1), 800, "chain restarts unmultiplied");
    }

    /// A T-spin with no lines cleared neither extends nor breaks the chain.
    #[test]
    fn a_spin_without_lines_leaves_the_chain_alone() {
        let mut state = ScoreState::new();
        state.apply(clear(4), 1);
        assert!(state.back_to_back);

        state.apply(spin(0, TSpin::Full), 1);
        assert!(
            state.back_to_back,
            "a zero-line spin must not break the chain"
        );

        state.combo = 0;
        assert_eq!(state.apply(clear(4), 1), 1200, "chain should still be live");
    }

    #[test]
    fn t_spins_and_tetrises_chain_together() {
        let mut state = ScoreState::new();
        state.apply(clear(4), 1);
        state.combo = 0;
        // T-spin double continues the chain: 1200 * 1.5
        assert_eq!(state.apply(spin(2, TSpin::Full), 1), 1800);
    }

    #[test]
    fn combos_build_and_reset() {
        let mut state = ScoreState::new();
        // First clear of a chain has combo 0, so no combo points.
        assert_eq!(state.apply(clear(1), 1), 100);
        assert_eq!(state.combo, 1);
        // Second consecutive clear: 100 + 50 * 1
        assert_eq!(state.apply(clear(1), 1), 150);
        // Third: 100 + 50 * 2
        assert_eq!(state.apply(clear(1), 1), 200);

        state.apply(clear(0), 1);
        assert_eq!(state.combo, 0, "a placement with no clear resets the combo");
    }

    #[test]
    fn combo_points_scale_with_level() {
        let mut state = ScoreState::new();
        state.apply(clear(1), 3);
        assert_eq!(state.apply(clear(1), 3), 300 + 150);
    }

    #[test]
    fn drop_points_are_flat_and_do_not_disturb_the_chain() {
        let mut state = ScoreState::new();
        state.apply(clear(4), 9);
        let before = state.back_to_back;

        state.add_soft_drop(10);
        state.add_hard_drop(15);
        assert_eq!(state.score, 800 * 9 + 10 + 30);
        assert_eq!(state.back_to_back, before);
    }

    #[test]
    fn perfect_clears_pay_their_bonus_on_top() {
        let mut state = ScoreState::new();
        let placement = Placement {
            lines: 4,
            tspin: TSpin::None,
            perfect_clear: true,
        };
        // 800 for the tetris, 2000 for a non-back-to-back perfect clear.
        assert_eq!(state.apply(placement, 1), 2800);
    }

    #[test]
    fn a_back_to_back_tetris_perfect_clear_pays_the_larger_bonus() {
        let mut state = ScoreState::new();
        state.apply(clear(4), 1);
        state.combo = 0;

        let placement = Placement {
            lines: 4,
            tspin: TSpin::None,
            perfect_clear: true,
        };
        // 800 * 1.5 for the chained tetris, plus the 3200 back-to-back bonus.
        assert_eq!(state.apply(placement, 1), 1200 + 3200);
    }

    #[test]
    fn an_empty_placement_scores_nothing() {
        let mut state = ScoreState::new();
        assert_eq!(state.apply(clear(0), 5), 0);
        assert_eq!(state.score, 0);
    }

    #[test]
    fn levels_advance_every_ten_lines() {
        assert_eq!(level_for_lines(1, 0), 1);
        assert_eq!(level_for_lines(1, 9), 1);
        assert_eq!(level_for_lines(1, 10), 2);
        assert_eq!(level_for_lines(1, 105), 11);
    }

    #[test]
    fn total_lines_accumulate() {
        let mut state = ScoreState::new();
        state.apply(clear(4), 1);
        state.apply(clear(2), 1);
        assert_eq!(state.lines, 6);
    }
}
