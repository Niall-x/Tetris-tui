//! NES scoring and level progression.
//!
//! Source: the NES instruction manual's scoring table, cross-checked against
//! harddrop.com / tetris.wiki "Scoring".

/// Base line-clear values, multiplied by (level + 1).
const BASE: [u32; 5] = [0, 40, 100, 300, 1200];

pub fn line_clear_score(lines: u32, level: u32) -> u32 {
    BASE[lines.min(4) as usize] * (level + 1)
}

/// Soft drop pays one point per cell and does not scale with level.
pub fn soft_drop_score(cells: u32) -> u32 {
    cells
}

/// Lines needed before the first level-up. After that every level takes a flat 10.
pub fn lines_to_first_level_up(start_level: u32) -> u32 {
    let a = start_level * 10 + 10;
    let b = (start_level * 10).saturating_sub(50).max(100);
    a.min(b)
}

/// The level after clearing `total_lines` from `start_level`.
pub fn level_for_lines(start_level: u32, total_lines: u32) -> u32 {
    let first = lines_to_first_level_up(start_level);
    if total_lines < first {
        start_level
    } else {
        start_level + 1 + (total_lines - first) / 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_clear_values_match_the_manual_at_level_zero() {
        assert_eq!(line_clear_score(1, 0), 40);
        assert_eq!(line_clear_score(2, 0), 100);
        assert_eq!(line_clear_score(3, 0), 300);
        assert_eq!(line_clear_score(4, 0), 1200);
    }

    #[test]
    fn line_clear_values_scale_with_level_plus_one() {
        assert_eq!(line_clear_score(4, 9), 12_000);
        assert_eq!(line_clear_score(1, 18), 760);
        assert_eq!(line_clear_score(3, 29), 9_000);
    }

    #[test]
    fn clearing_nothing_scores_nothing() {
        assert_eq!(line_clear_score(0, 15), 0);
    }

    #[test]
    fn soft_drop_is_flat_one_point_per_cell() {
        assert_eq!(soft_drop_score(17), 17);
    }

    /// The widely cited figures: start 0 levels up at 10 lines, start 9 at 100,
    /// start 19 at 140.
    #[test]
    fn first_level_up_matches_known_thresholds() {
        assert_eq!(lines_to_first_level_up(0), 10);
        assert_eq!(lines_to_first_level_up(9), 100);
        assert_eq!(lines_to_first_level_up(19), 140);
    }

    #[test]
    fn levels_advance_every_ten_lines_after_the_first() {
        assert_eq!(level_for_lines(0, 0), 0);
        assert_eq!(level_for_lines(0, 9), 0);
        assert_eq!(level_for_lines(0, 10), 1);
        assert_eq!(level_for_lines(0, 19), 1);
        assert_eq!(level_for_lines(0, 20), 2);
        assert_eq!(level_for_lines(0, 130), 13);

        assert_eq!(level_for_lines(9, 99), 9);
        assert_eq!(level_for_lines(9, 100), 10);
        assert_eq!(level_for_lines(9, 110), 11);

        assert_eq!(level_for_lines(19, 139), 19);
        assert_eq!(level_for_lines(19, 140), 20);
    }
}
