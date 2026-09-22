//! Nintendo Rotation System (right-handed variant, as in NES Tetris).
//!
//! The ROM stores 19 orientations at $8A9C as (Y, tile, X) triples relative to a
//! centre square, and the game indexes them by orientation ID. Those IDs matter
//! beyond drawing: the piece randomiser adds the previous piece's *spawn*
//! orientation ID during its reroll (see `randomizer.rs`), so they are reproduced
//! here exactly rather than renumbered.
//!
//! IDs, in ROM order:
//!   Tu 0x00, Tr 0x01, Td 0x02, Tl 0x03,
//!   Jl 0x04, Ju 0x05, Jr 0x06, Jd 0x07,
//!   Zh 0x08, Zv 0x09, O 0x0A, Sh 0x0B, Sv 0x0C,
//!   Lr 0x0D, Ld 0x0E, Ll 0x0F, Lu 0x10, Iv 0x11, Ih 0x12
//!
//! Incrementing the ID within a piece's group is a clockwise rotation, which is
//! how the tables below are ordered. There are no wall kicks: a rotation that
//! collides simply fails.
//!
//! Sources: meatfighter.com/nintendotetrisai (orientation table $8A9C, spawn
//! orientations), harddrop.com/wiki/Nintendo_Rotation_System (wallkickless,
//! right-handed, no lock delay).

use crate::engine::piece::PieceKind;

/// Offsets are (x, y) from the piece's pivot, y growing downward.
pub type Cells = [(i32, i32); 4];

/// Orientations for one piece, in clockwise order starting from its spawn state.
pub struct NrsPiece {
    pub spawn_orientation_id: u8,
    pub states: &'static [Cells],
}

// T: 4 states. Spawn is Td (0x02) — flat bar on top, nub hanging below.
const T_STATES: &[Cells] = &[
    [(-1, 0), (0, 0), (1, 0), (0, 1)],  // Td 0x02 (spawn)
    [(0, -1), (0, 0), (0, 1), (-1, 0)], // Tl 0x03
    [(-1, 0), (0, 0), (1, 0), (0, -1)], // Tu 0x00
    [(0, -1), (0, 0), (0, 1), (1, 0)],  // Tr 0x01
];

// J: 4 states. Spawn is Jd (0x07) — bar on top, nub below its right end.
const J_STATES: &[Cells] = &[
    [(-1, 0), (0, 0), (1, 0), (1, 1)],   // Jd 0x07 (spawn)
    [(0, -1), (0, 0), (0, 1), (-1, 1)],  // Jl 0x04
    [(-1, -1), (-1, 0), (0, 0), (1, 0)], // Ju 0x05
    [(0, -1), (0, 0), (0, 1), (1, -1)],  // Jr 0x06
];

// L: 4 states. Spawn is Ld (0x0E) — bar on top, nub below its left end.
const L_STATES: &[Cells] = &[
    [(-1, 0), (0, 0), (1, 0), (-1, 1)],  // Ld 0x0E (spawn)
    [(-1, -1), (0, -1), (0, 0), (0, 1)], // Ll 0x0F
    [(1, -1), (-1, 0), (0, 0), (1, 0)],  // Lu 0x10
    [(0, -1), (0, 0), (0, 1), (1, 1)],   // Lr 0x0D
];

// Z: 2 states. Spawn is Zh (0x08).
const Z_STATES: &[Cells] = &[
    [(-1, 0), (0, 0), (0, 1), (1, 1)],   // Zh 0x08 (spawn)
    [(0, -1), (0, 0), (-1, 0), (-1, 1)], // Zv 0x09
];

// S: 2 states. Spawn is Sh (0x0B).
const S_STATES: &[Cells] = &[
    [(0, 0), (1, 0), (-1, 1), (0, 1)],   // Sh 0x0B (spawn)
    [(-1, -1), (-1, 0), (0, 0), (0, 1)], // Sv 0x0C
];

// I: 2 states. Spawn is Ih (0x12), spanning columns 3..6 from a pivot at column 5.
const I_STATES: &[Cells] = &[
    [(-2, 0), (-1, 0), (0, 0), (1, 0)], // Ih 0x12 (spawn)
    [(0, -2), (0, -1), (0, 0), (0, 1)], // Iv 0x11
];

// O: 1 state, no rotation.
const O_STATES: &[Cells] = &[[(-1, 0), (0, 0), (-1, 1), (0, 1)]]; // O 0x0A (spawn)

pub fn piece_data(kind: PieceKind) -> NrsPiece {
    match kind {
        PieceKind::T => NrsPiece {
            spawn_orientation_id: 0x02,
            states: T_STATES,
        },
        PieceKind::J => NrsPiece {
            spawn_orientation_id: 0x07,
            states: J_STATES,
        },
        PieceKind::Z => NrsPiece {
            spawn_orientation_id: 0x08,
            states: Z_STATES,
        },
        PieceKind::O => NrsPiece {
            spawn_orientation_id: 0x0A,
            states: O_STATES,
        },
        PieceKind::S => NrsPiece {
            spawn_orientation_id: 0x0B,
            states: S_STATES,
        },
        PieceKind::L => NrsPiece {
            spawn_orientation_id: 0x0E,
            states: L_STATES,
        },
        PieceKind::I => NrsPiece {
            spawn_orientation_id: 0x12,
            states: I_STATES,
        },
    }
}

/// How many distinct orientations this piece has under NRS (1, 2 or 4).
pub fn state_count(kind: PieceKind) -> usize {
    piece_data(kind).states.len()
}

/// Cell offsets for `kind` in rotation `state`, where state 0 is the spawn
/// orientation and increasing state is a clockwise rotation.
pub fn cells(kind: PieceKind, state: usize) -> Cells {
    let data = piece_data(kind);
    data.states[state % data.states.len()]
}

/// Board coordinates of `kind` at `state` when its pivot sits at (x, y).
pub fn cells_at(kind: PieceKind, state: usize, x: i32, y: i32) -> Cells {
    let mut out = cells(kind, state);
    for cell in out.iter_mut() {
        cell.0 += x;
        cell.1 += y;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one orientation quoted verbatim by meatfighter's dump of $8A9C.
    #[test]
    fn t_spawn_matches_documented_rom_orientation() {
        let mut td = cells(PieceKind::T, 0);
        td.sort();
        let mut expected = [(-1, 0), (0, 0), (1, 0), (0, 1)];
        expected.sort();
        assert_eq!(td, expected);
    }

    #[test]
    fn state_counts_match_the_19_orientation_table() {
        assert_eq!(state_count(PieceKind::T), 4);
        assert_eq!(state_count(PieceKind::J), 4);
        assert_eq!(state_count(PieceKind::L), 4);
        assert_eq!(state_count(PieceKind::S), 2);
        assert_eq!(state_count(PieceKind::Z), 2);
        assert_eq!(state_count(PieceKind::I), 2);
        assert_eq!(state_count(PieceKind::O), 1);
        let total: usize = PieceKind::ALL.iter().map(|&k| state_count(k)).sum();
        assert_eq!(total, 19);
    }

    #[test]
    fn every_state_has_four_distinct_cells() {
        for kind in PieceKind::ALL {
            for state in 0..state_count(kind) {
                let mut c = cells(kind, state).to_vec();
                c.sort();
                c.dedup();
                assert_eq!(c.len(), 4, "{kind:?} state {state} has duplicate cells");
            }
        }
    }

    /// Each successive state must be the previous one rotated 90° clockwise:
    /// with y growing downward that is (x, y) -> (-y, x).
    ///
    /// Only 4-state pieces close the loop. S, Z and I have two states, so rotating
    /// out of the second one returns to the first rather than continuing around —
    /// that truncation is the NRS behaviour, not an error in the tables.
    #[test]
    fn successive_states_are_clockwise_rotations() {
        for kind in PieceKind::ALL {
            let count = state_count(kind);
            if count < 2 {
                continue;
            }
            let transitions = if count == 4 { count } else { count - 1 };
            for state in 0..transitions {
                let mut rotated: Vec<(i32, i32)> =
                    cells(kind, state).iter().map(|&(x, y)| (-y, x)).collect();
                let mut next: Vec<(i32, i32)> = cells(kind, (state + 1) % count).to_vec();
                rotated.sort();
                next.sort();
                assert_eq!(
                    rotated,
                    next,
                    "{kind:?} state {state} -> {} is not a clockwise rotation",
                    (state + 1) % count
                );
            }
        }
    }

    /// Spawn orientation IDs drive the randomiser's reroll, and the RNG paper
    /// tabulates them mod 7 as T=2, J=0, Z=1, O=3, S=4, L=0, I=4.
    #[test]
    fn spawn_orientation_ids_match_rom_and_rng_paper() {
        let expected: [(PieceKind, u8, u8); 7] = [
            (PieceKind::T, 0x02, 2),
            (PieceKind::J, 0x07, 0),
            (PieceKind::Z, 0x08, 1),
            (PieceKind::O, 0x0A, 3),
            (PieceKind::S, 0x0B, 4),
            (PieceKind::L, 0x0E, 0),
            (PieceKind::I, 0x12, 4),
        ];
        for (kind, id, id_mod_7) in expected {
            let actual = piece_data(kind).spawn_orientation_id;
            assert_eq!(actual, id, "{kind:?} spawn orientation id");
            assert_eq!(actual % 7, id_mod_7, "{kind:?} spawn orientation id mod 7");
        }
    }

    /// Every spawn state must sit in rows 0..=1 when placed at the spawn row, which
    /// is what makes pieces appear flat-topped at the top of the NES playfield.
    #[test]
    fn spawn_states_occupy_the_top_two_rows() {
        for kind in PieceKind::ALL {
            for (_, y) in cells(kind, 0) {
                assert!(
                    (0..=1).contains(&y),
                    "{kind:?} spawn state leaves rows 0..=1 (y={y})"
                );
            }
        }
    }

    /// The I piece spawns across columns 3..=6 from a pivot at column 5.
    #[test]
    fn i_spawn_spans_columns_three_to_six() {
        let mut xs: Vec<i32> = cells_at(PieceKind::I, 0, 5, 0)
            .iter()
            .map(|c| c.0)
            .collect();
        xs.sort();
        assert_eq!(xs, vec![3, 4, 5, 6]);
    }
}
