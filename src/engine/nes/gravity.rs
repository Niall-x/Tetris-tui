//! NES gravity, entry delay and DAS constants, all measured in NTSC frames.
//!
//! The game runs at ~60.0988 Hz; we tick at 60 Hz and treat one tick as one frame,
//! which keeps every table below usable as-is.

/// Frames per row of gravity, indexed by level, from the ROM table at $898E.
/// Deliberately a table rather than a formula — the curve is irregular (note the
/// jump from 8 to 6 at level 9).
///
/// Source: meatfighter.com/nintendotetrisai; cross-checked against
/// harddrop.com / tetris.wiki "Tetris (NES, Nintendo)".
const FRAMES_PER_ROW: [u32; 29] = [
    48, 43, 38, 33, 28, 23, 18, 13, 8, 6, // 0-9
    5, 5, 5, // 10-12
    4, 4, 4, // 13-15
    3, 3, 3, // 16-18
    2, 2, 2, 2, 2, 2, 2, 2, 2, 2, // 19-28
];

/// Level 29 onward is locked at one row per frame — the kill screen.
pub const KILL_SCREEN_LEVEL: u32 = 29;

pub fn frames_per_row(level: u32) -> u32 {
    if level >= KILL_SCREEN_LEVEL {
        1
    } else {
        FRAMES_PER_ROW[level as usize]
    }
}

/// DAS: the first shift happens on press, then the charge must reach 16 frames
/// before auto-shift starts. On each auto-shift the counter is reset to 10 rather
/// than 0, so repeats land every 6 frames thereafter.
///
/// Source: harddrop.com/wiki/DAS.
pub const DAS_CHARGE_FRAMES: u32 = 16;
pub const DAS_RECHARGE_FRAMES: u32 = 10;
pub const DAS_REPEAT_FRAMES: u32 = DAS_CHARGE_FRAMES - DAS_RECHARGE_FRAMES;

/// Entry delay after a piece locks, 10–18 frames depending on how high it locked:
/// the bottom two rows give 10 frames, and every four rows above that adds two.
///
/// The banding pattern is consistently documented, but the exact row cutoffs have
/// not been confirmed against a disassembly — see brief.md §14. Kept as one
/// function so a correction is a single edit.
pub fn entry_delay_frames(lock_row: u32, visible_height: u32) -> u32 {
    let rows_from_bottom = visible_height.saturating_sub(lock_row + 1);
    // Two rows short of a full band at the bottom, so bands start at rows 2, 6,
    // 10 and 14 from the floor.
    let band = (rows_from_bottom + 2) / 4;
    (10 + band * 2).min(18)
}

/// The line-clear animation erases each full row from the centre outward, one
/// column either side per step: columns 4 and 5, then 3 and 6, out to 0 and 9.
///
/// A step only happens on a frame where the global frame counter is a multiple of
/// four, so the first one lands 1–4 frames after the lock and the whole clear
/// takes 17–20 frames, depending on where in the cycle the piece locked.
///
/// Source: `updateLineClearingAnimation` and its `leftColumns`/`rightColumns`
/// tables in the Tetris (NES) disassembly (github.com/CelestialAmber/
/// TetrisNESDisasm); the 17–20 total matches tetris.wiki "Tetris (NES)".
pub const LINE_CLEAR_STEPS: u32 = 5;
pub const LINE_CLEAR_STEP_FRAMES: u32 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravity_table_matches_the_rom_at_every_boundary() {
        let expected = [
            (0, 48),
            (1, 43),
            (2, 38),
            (3, 33),
            (4, 28),
            (5, 23),
            (6, 18),
            (7, 13),
            (8, 8),
            (9, 6),
            (10, 5),
            (12, 5),
            (13, 4),
            (15, 4),
            (16, 3),
            (18, 3),
            (19, 2),
            (28, 2),
            (29, 1),
            (35, 1),
            (255, 1),
        ];
        for (level, frames) in expected {
            assert_eq!(frames_per_row(level), frames, "level {level}");
        }
    }

    #[test]
    fn das_repeats_every_six_frames_after_a_sixteen_frame_charge() {
        assert_eq!(DAS_CHARGE_FRAMES, 16);
        assert_eq!(DAS_REPEAT_FRAMES, 6);
    }

    /// Every band edge, so an off-by-some-rows band cannot hide between the
    /// rows a spot check happens to pick.
    #[test]
    fn entry_delay_grows_two_frames_every_four_rows_above_the_bottom_two() {
        // (rows up from the floor, frames) at each band's first and last row.
        let bands = [
            (0, 10),
            (1, 10),
            (2, 12),
            (5, 12),
            (6, 14),
            (9, 14),
            (10, 16),
            (13, 16),
            (14, 18),
            (19, 18),
        ];
        for (up, frames) in bands {
            assert_eq!(entry_delay_frames(19 - up, 20), frames, "{up} rows up");
        }
    }

    #[test]
    fn entry_delay_never_leaves_the_documented_range() {
        for row in 0..20 {
            let frames = entry_delay_frames(row, 20);
            assert!((10..=18).contains(&frames), "row {row} gave {frames}");
        }
    }
}
