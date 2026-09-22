//! Super Rotation System: piece geometry and wall kicks.
//!
//! Unlike NRS, pieces here are anchored by the top-left corner of their bounding
//! box — 3x3 for JLSTZ, 4x4 for I, 2x2 for O — and every piece has four rotation
//! states. Rotation is a true rotation within that box, then a walk down a table
//! of five offsets, taking the first that does not collide.
//!
//! Sign convention: the published kick tables use a y-axis that grows *upward*,
//! while the board's rows grow downward. The tables below are stored exactly as
//! published and the sign is flipped in one place, `offset_to_board`, so the data
//! can be checked against the source without mental arithmetic.
//!
//! Source: harddrop.com/wiki/SRS.

use crate::engine::board::Board;
use crate::engine::piece::{ActivePiece, PieceKind, RotationDir};

/// Cells are (column, row) within the piece's bounding box.
pub type Cells = [(i32, i32); 4];

/// Rotation states in SRS order: spawn, right, two (180°), left.
pub const STATE_COUNT: usize = 4;

const T: [Cells; 4] = [
    [(1, 0), (0, 1), (1, 1), (2, 1)],
    [(1, 0), (1, 1), (2, 1), (1, 2)],
    [(0, 1), (1, 1), (2, 1), (1, 2)],
    [(1, 0), (0, 1), (1, 1), (1, 2)],
];

const J: [Cells; 4] = [
    [(0, 0), (0, 1), (1, 1), (2, 1)],
    [(1, 0), (2, 0), (1, 1), (1, 2)],
    [(0, 1), (1, 1), (2, 1), (2, 2)],
    [(1, 0), (1, 1), (0, 2), (1, 2)],
];

const L: [Cells; 4] = [
    [(2, 0), (0, 1), (1, 1), (2, 1)],
    [(1, 0), (1, 1), (1, 2), (2, 2)],
    [(0, 1), (1, 1), (2, 1), (0, 2)],
    [(0, 0), (1, 0), (1, 1), (1, 2)],
];

const S: [Cells; 4] = [
    [(1, 0), (2, 0), (0, 1), (1, 1)],
    [(1, 0), (1, 1), (2, 1), (2, 2)],
    [(1, 1), (2, 1), (0, 2), (1, 2)],
    [(0, 0), (0, 1), (1, 1), (1, 2)],
];

const Z: [Cells; 4] = [
    [(0, 0), (1, 0), (1, 1), (2, 1)],
    [(2, 0), (1, 1), (2, 1), (1, 2)],
    [(0, 1), (1, 1), (1, 2), (2, 2)],
    [(1, 0), (0, 1), (1, 1), (0, 2)],
];

const I: [Cells; 4] = [
    [(0, 1), (1, 1), (2, 1), (3, 1)],
    [(2, 0), (2, 1), (2, 2), (2, 3)],
    [(0, 2), (1, 2), (2, 2), (3, 2)],
    [(1, 0), (1, 1), (1, 2), (1, 3)],
];

/// O never rotates, so all four states are identical and its kicks are all zero.
const O: [Cells; 4] = [
    [(0, 0), (1, 0), (0, 1), (1, 1)],
    [(0, 0), (1, 0), (0, 1), (1, 1)],
    [(0, 0), (1, 0), (0, 1), (1, 1)],
    [(0, 0), (1, 0), (0, 1), (1, 1)],
];

/// Side length of the piece's bounding box.
pub fn box_size(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::I => 4,
        PieceKind::O => 2,
        _ => 3,
    }
}

pub fn cells(kind: PieceKind, state: usize) -> Cells {
    let table = match kind {
        PieceKind::T => &T,
        PieceKind::J => &J,
        PieceKind::L => &L,
        PieceKind::S => &S,
        PieceKind::Z => &Z,
        PieceKind::I => &I,
        PieceKind::O => &O,
    };
    table[state % STATE_COUNT]
}

/// Board coordinates of `kind` in `state` with its bounding box at (x, y).
pub fn cells_at(kind: PieceKind, state: usize, x: i32, y: i32) -> Cells {
    let mut out = cells(kind, state);
    for cell in out.iter_mut() {
        cell.0 += x;
        cell.1 += y;
    }
    out
}

/// Five kick offsets per transition, as published, with y growing upward.
/// Indexed by [from_state][direction], direction 0 = clockwise, 1 = anticlockwise.
const JLSTZ_KICKS: [[[(i32, i32); 5]; 2]; 4] = [
    // From spawn (0)
    [
        [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)], // 0 -> R
        [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],    // 0 -> L
    ],
    // From right (R)
    [
        [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)], // R -> 2
        [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)], // R -> 0
    ],
    // From two (2)
    [
        [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],    // 2 -> L
        [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)], // 2 -> R
    ],
    // From left (L)
    [
        [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)], // L -> 0
        [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)], // L -> 2
    ],
];

const I_KICKS: [[[(i32, i32); 5]; 2]; 4] = [
    // From spawn (0)
    [
        [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)], // 0 -> R
        [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)], // 0 -> L
    ],
    // From right (R)
    [
        [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)], // R -> 2
        [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)], // R -> 0
    ],
    // From two (2)
    [
        [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)], // 2 -> L
        [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)], // 2 -> R
    ],
    // From left (L)
    [
        [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)], // L -> 0
        [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)], // L -> 2
    ],
];

pub fn kick_table(kind: PieceKind, from: usize, dir: RotationDir) -> [(i32, i32); 5] {
    if kind == PieceKind::O {
        return [(0, 0); 5];
    }
    let table = if kind == PieceKind::I {
        &I_KICKS
    } else {
        &JLSTZ_KICKS
    };
    let d = match dir {
        RotationDir::Cw => 0,
        RotationDir::Ccw => 1,
    };
    table[from % STATE_COUNT][d]
}

/// Published offsets grow y upward; board rows grow downward.
fn offset_to_board((dx, dy): (i32, i32)) -> (i32, i32) {
    (dx, -dy)
}

pub fn rotated_state(from: usize, dir: RotationDir) -> usize {
    match dir {
        RotationDir::Cw => (from + 1) % STATE_COUNT,
        RotationDir::Ccw => (from + STATE_COUNT - 1) % STATE_COUNT,
    }
}

/// The result of a rotation attempt.
///
/// `kick_index` is which of the five tests succeeded. It is not bookkeeping: a
/// rotation that lands on the fifth test always counts as a full T-spin, so T-spin
/// detection needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rotated {
    pub piece: ActivePiece,
    pub kick_index: usize,
}

/// Rotate `piece` on `board`, walking the kick table. Returns `None` if all five
/// tests collide, in which case the rotation simply does not happen.
pub fn rotate(board: &Board, piece: ActivePiece, dir: RotationDir) -> Option<Rotated> {
    let target = rotated_state(piece.state, dir);
    let kicks = kick_table(piece.kind, piece.state, dir);

    for (index, &offset) in kicks.iter().enumerate() {
        let (dx, dy) = offset_to_board(offset);
        let candidate = ActivePiece::new(piece.kind, target, piece.x + dx, piece.y + dy);
        if !board.collides(&cells_at(
            candidate.kind,
            candidate.state,
            candidate.x,
            candidate.y,
        )) {
            return Some(Rotated {
                piece: candidate,
                kick_index: index,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut c: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
        c.sort();
        c
    }

    /// Every state must be the spawn state rotated within the bounding box:
    /// clockwise is (col, row) -> (size - 1 - row, col).
    #[test]
    fn all_states_are_true_rotations_of_the_spawn_state() {
        for kind in PieceKind::ALL {
            let size = box_size(kind);
            for state in 0..STATE_COUNT {
                let expected: Vec<(i32, i32)> = cells(kind, state)
                    .iter()
                    .map(|&(c, r)| (size - 1 - r, c))
                    .collect();
                let next = cells(kind, (state + 1) % STATE_COUNT).to_vec();
                assert_eq!(
                    sorted(expected),
                    sorted(next),
                    "{kind:?} state {state} -> {} is not a rotation",
                    (state + 1) % STATE_COUNT
                );
            }
        }
    }

    #[test]
    fn every_state_has_four_cells_inside_its_box() {
        for kind in PieceKind::ALL {
            let size = box_size(kind);
            for state in 0..STATE_COUNT {
                let mut c = cells(kind, state).to_vec();
                c.sort();
                c.dedup();
                assert_eq!(c.len(), 4, "{kind:?} state {state}");
                for (col, row) in c {
                    assert!(
                        (0..size).contains(&col) && (0..size).contains(&row),
                        "{kind:?} state {state} leaves its {size}x{size} box"
                    );
                }
            }
        }
    }

    /// Transcription guard against harddrop.com/wiki/SRS.
    #[test]
    fn jlstz_kick_tables_match_the_published_values() {
        assert_eq!(
            kick_table(PieceKind::T, 0, RotationDir::Cw),
            [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)]
        );
        assert_eq!(
            kick_table(PieceKind::T, 1, RotationDir::Ccw),
            [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)]
        );
        assert_eq!(
            kick_table(PieceKind::T, 2, RotationDir::Cw),
            [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)]
        );
        assert_eq!(
            kick_table(PieceKind::T, 3, RotationDir::Cw),
            [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)]
        );
    }

    #[test]
    fn i_kick_tables_match_the_published_values() {
        assert_eq!(
            kick_table(PieceKind::I, 0, RotationDir::Cw),
            [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)]
        );
        assert_eq!(
            kick_table(PieceKind::I, 1, RotationDir::Cw),
            [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)]
        );
        assert_eq!(
            kick_table(PieceKind::I, 2, RotationDir::Cw),
            [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)]
        );
        assert_eq!(
            kick_table(PieceKind::I, 3, RotationDir::Cw),
            [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)]
        );
    }

    #[test]
    fn o_never_rotates_and_never_kicks() {
        let board = Board::new(20);
        let piece = ActivePiece::new(PieceKind::O, 0, 4, 20);
        let result = rotate(&board, piece, RotationDir::Cw).unwrap();
        assert_eq!(result.kick_index, 0);
        assert_eq!(
            sorted(
                cells_at(
                    PieceKind::O,
                    result.piece.state,
                    result.piece.x,
                    result.piece.y
                )
                .to_vec()
            ),
            sorted(cells_at(PieceKind::O, 0, 4, 20).to_vec()),
            "the O piece must not move when rotated"
        );
    }

    #[test]
    fn rotating_in_open_space_uses_the_first_test_and_does_not_move() {
        let board = Board::new(20);
        for kind in PieceKind::ALL {
            let piece = ActivePiece::new(kind, 0, 3, 20);
            let result = rotate(&board, piece, RotationDir::Cw).expect("open space rotation");
            assert_eq!(result.kick_index, 0, "{kind:?} should need no kick");
            assert_eq!((result.piece.x, result.piece.y), (3, 20));
        }
    }

    /// A vertical I flush against the left wall, rotated flat, has nowhere to go
    /// until the third test shifts it two columns right. Pins the x sign
    /// convention on a real board.
    #[test]
    fn a_wall_kick_pushes_away_from_the_left_wall() {
        let board = Board::new(20);
        // State 1 occupies box column 2, so x = -2 puts it in column 0.
        let piece = ActivePiece::new(PieceKind::I, 1, -2, 20);
        assert!(
            !board.collides(&cells_at(piece.kind, piece.state, piece.x, piece.y)),
            "test setup: the starting piece must be legal"
        );
        assert!(
            board.collides(&cells_at(PieceKind::I, 2, -2, 20)),
            "test setup: the un-kicked rotation should hit the wall"
        );

        let result = rotate(&board, piece, RotationDir::Cw).expect("should kick");
        assert_eq!(result.kick_index, 2, "the (+2, 0) test should be the one");
        assert_eq!(
            result.piece.x, 0,
            "kick moves the piece right, off the wall"
        );
        assert!(!board.collides(&cells_at(
            PieceKind::I,
            result.piece.state,
            result.piece.x,
            result.piece.y
        )));
    }

    /// A flat I resting on the floor, rotated upright, only fits once the fifth
    /// test lifts it two rows. Pins the y sign convention: a published +2 must
    /// move the piece toward row 0.
    #[test]
    fn a_floor_kick_lifts_the_piece_up_the_board() {
        let board = Board::new(20);
        let floor_y = board.height() as i32 - 2; // state 0 occupies box row 1
        let piece = ActivePiece::new(PieceKind::I, 0, 3, floor_y);
        assert!(
            !board.collides(&cells_at(piece.kind, piece.state, piece.x, piece.y)),
            "test setup: the starting piece must rest on the floor"
        );

        let result = rotate(&board, piece, RotationDir::Cw).expect("should kick");
        assert_eq!(result.kick_index, 4, "only the last test has the lift");
        assert_eq!(
            result.piece.y,
            floor_y - 2,
            "a published (1, 2) must lift the piece two rows"
        );
        assert_eq!(result.piece.x, 4);
        assert!(!board.collides(&cells_at(
            PieceKind::I,
            result.piece.state,
            result.piece.x,
            result.piece.y
        )));
    }

    /// Published offsets grow upward, board rows grow downward. A positive
    /// published dy must move the piece toward row 0.
    #[test]
    fn positive_published_offsets_move_the_piece_up_the_board() {
        assert_eq!(offset_to_board((0, 2)), (0, -2));
        assert_eq!(offset_to_board((1, -2)), (1, 2));
        assert_eq!(offset_to_board((-1, 0)), (-1, 0));
    }

    #[test]
    fn a_rotation_with_no_room_at_all_is_refused() {
        let mut board = Board::new(20);
        // Bury the piece so every one of the five tests collides.
        for y in 0..board.height() as i32 {
            for x in 0..board.width() as i32 {
                board.set(x, y, Some(PieceKind::I));
            }
        }
        for x in 0..3 {
            board.set(x + 3, 21, None);
        }
        let piece = ActivePiece::new(PieceKind::T, 0, 3, 20);
        assert!(rotate(&board, piece, RotationDir::Cw).is_none());
    }

    #[test]
    fn state_transitions_wrap_in_both_directions() {
        assert_eq!(rotated_state(0, RotationDir::Cw), 1);
        assert_eq!(rotated_state(3, RotationDir::Cw), 0);
        assert_eq!(rotated_state(0, RotationDir::Ccw), 3);
        assert_eq!(rotated_state(2, RotationDir::Ccw), 1);
    }
}
