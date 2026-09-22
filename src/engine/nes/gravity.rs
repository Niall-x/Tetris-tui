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
    let band = rows_from_bottom.saturating_sub(1) / 4;
    (10 + band * 2).min(18)
}

/// Frames spent on the line-clear flash before entry delay begins. Reported as
/// 17–20 depending on the frame a piece locked on; we use a flat 18 pending
/// confirmation (brief.md §14).
pub const LINE_CLEAR_DELAY_FRAMES: u32 = 18;

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

    #[test]
    fn entry_delay_grows_with_lock_height() {
        // Bottom two rows of a 20-row field.
        assert_eq!(entry_delay_frames(19, 20), 10);
        assert_eq!(entry_delay_frames(18, 20), 10);
        // Then two frames per four-row band, capped at 18.
        assert_eq!(entry_delay_frames(14, 20), 12);
        assert_eq!(entry_delay_frames(10, 20), 14);
        assert_eq!(entry_delay_frames(0, 20), 18);
    }

    #[test]
    fn entry_delay_never_leaves_the_documented_range() {
        for row in 0..20 {
            let frames = entry_delay_frames(row, 20);
            assert!((10..=18).contains(&frames), "row {row} gave {frames}");
        }
    }
}
