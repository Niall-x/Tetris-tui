//! T-spin and mini T-spin detection.
//!
//! The rule, in order:
//!
//! 1. The placement must have ended with a rotation. A piece merely slid or
//!    dropped into the same position does not count.
//! 2. At least three of the T's four diagonal corners must be occupied, where the
//!    walls and floor count as occupied.
//! 3. The two corners on the side the T points toward are its *front* corners.
//!    Both front corners occupied is a full T-spin; only one, with both back
//!    corners occupied, is a mini.
//! 4. Except: a rotation that only fitted on the fifth kick test is always a full
//!    T-spin. That is what makes T-spin triples score as triples.
//!
//! Source: tetris.wiki/T-Spin, harddrop.com/wiki/T-Spin_Guide.

use crate::engine::board::Board;
use crate::engine::piece::{ActivePiece, PieceKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TSpin {
    None,
    Mini,
    Full,
}

/// The kick test index that always promotes to a full T-spin.
const PROMOTING_KICK_INDEX: usize = 4;

/// Corners of the T's 3x3 box, as (dx, dy) from the box origin.
const TOP_LEFT: (i32, i32) = (0, 0);
const TOP_RIGHT: (i32, i32) = (2, 0);
const BOTTOM_LEFT: (i32, i32) = (0, 2);
const BOTTOM_RIGHT: (i32, i32) = (2, 2);

/// The two front corners for each rotation state, front meaning the side the T's
/// nub points toward.
fn front_corners(state: usize) -> [(i32, i32); 2] {
    match state % 4 {
        0 => [TOP_LEFT, TOP_RIGHT],       // nub up
        1 => [TOP_RIGHT, BOTTOM_RIGHT],   // nub right
        2 => [BOTTOM_LEFT, BOTTOM_RIGHT], // nub down
        _ => [TOP_LEFT, BOTTOM_LEFT],     // nub left
    }
}

fn back_corners(state: usize) -> [(i32, i32); 2] {
    match state % 4 {
        0 => [BOTTOM_LEFT, BOTTOM_RIGHT],
        1 => [TOP_LEFT, BOTTOM_LEFT],
        2 => [TOP_LEFT, TOP_RIGHT],
        _ => [TOP_RIGHT, BOTTOM_RIGHT],
    }
}

fn occupied(board: &Board, piece: ActivePiece, (dx, dy): (i32, i32)) -> bool {
    board.is_blocked_or_wall(piece.x + dx, piece.y + dy)
}

/// Classify the placement that just happened.
///
/// `rotated_last` is whether the final action before locking was a rotation, and
/// `kick_index` which of the five kick tests that rotation used.
pub fn detect(board: &Board, piece: ActivePiece, rotated_last: bool, kick_index: usize) -> TSpin {
    if piece.kind != PieceKind::T || !rotated_last {
        return TSpin::None;
    }

    let front = front_corners(piece.state);
    let back = back_corners(piece.state);

    let front_filled = front.iter().filter(|&&c| occupied(board, piece, c)).count();
    let back_filled = back.iter().filter(|&&c| occupied(board, piece, c)).count();

    if front_filled + back_filled < 3 {
        return TSpin::None;
    }

    if kick_index == PROMOTING_KICK_INDEX {
        return TSpin::Full;
    }

    if front_filled == 2 {
        TSpin::Full
    } else {
        TSpin::Mini
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A T pointing up with its box origin at (x, y) on an otherwise empty board.
    fn setup(corners: &[(i32, i32)], state: usize) -> (Board, ActivePiece) {
        let mut board = Board::new(20);
        let piece = ActivePiece::new(PieceKind::T, state, 3, 25);
        for &(dx, dy) in corners {
            board.set(piece.x + dx, piece.y + dy, Some(PieceKind::I));
        }
        (board, piece)
    }

    #[test]
    fn only_the_t_piece_can_spin() {
        let mut board = Board::new(20);
        for &(dx, dy) in &[TOP_LEFT, TOP_RIGHT, BOTTOM_LEFT, BOTTOM_RIGHT] {
            board.set(3 + dx, 25 + dy, Some(PieceKind::I));
        }
        for kind in [PieceKind::J, PieceKind::L, PieceKind::S, PieceKind::I] {
            let piece = ActivePiece::new(kind, 0, 3, 25);
            assert_eq!(detect(&board, piece, true, 0), TSpin::None, "{kind:?}");
        }
    }

    #[test]
    fn a_placement_that_did_not_end_in_a_rotation_is_never_a_spin() {
        let (board, piece) = setup(&[TOP_LEFT, TOP_RIGHT, BOTTOM_LEFT], 0);
        assert_eq!(detect(&board, piece, false, 0), TSpin::None);
    }

    #[test]
    fn fewer_than_three_corners_is_not_a_spin() {
        let (board, piece) = setup(&[TOP_LEFT, TOP_RIGHT], 0);
        assert_eq!(detect(&board, piece, true, 0), TSpin::None);
    }

    /// Both front corners plus a back corner: the standard full T-spin.
    #[test]
    fn both_front_corners_make_a_full_spin() {
        let (board, piece) = setup(&[TOP_LEFT, TOP_RIGHT, BOTTOM_LEFT], 0);
        assert_eq!(detect(&board, piece, true, 0), TSpin::Full);
    }

    /// One front corner and both back corners: a mini.
    #[test]
    fn one_front_corner_with_both_backs_makes_a_mini() {
        let (board, piece) = setup(&[TOP_LEFT, BOTTOM_LEFT, BOTTOM_RIGHT], 0);
        assert_eq!(detect(&board, piece, true, 0), TSpin::Mini);
    }

    /// The exception that makes T-spin triples work: the fifth kick always counts
    /// as a full spin, even with mini-shaped corners.
    #[test]
    fn the_fifth_kick_test_promotes_a_mini_to_a_full_spin() {
        let (board, piece) = setup(&[TOP_LEFT, BOTTOM_LEFT, BOTTOM_RIGHT], 0);
        assert_eq!(detect(&board, piece, true, 0), TSpin::Mini);
        assert_eq!(detect(&board, piece, true, 4), TSpin::Full);
    }

    /// Promotion still requires three corners: the kick index alone is not enough.
    #[test]
    fn the_fifth_kick_does_not_invent_a_spin_from_nothing() {
        let (board, piece) = setup(&[TOP_LEFT], 0);
        assert_eq!(detect(&board, piece, true, 4), TSpin::None);
    }

    #[test]
    fn front_corners_follow_the_rotation_state() {
        // Nub right: the front corners are the two on the right.
        let (board, piece) = setup(&[TOP_RIGHT, BOTTOM_RIGHT, TOP_LEFT], 1);
        assert_eq!(detect(&board, piece, true, 0), TSpin::Full);

        // Same corners, but now the nub points left, so those are the back pair.
        let (board, piece) = setup(&[TOP_RIGHT, BOTTOM_RIGHT, TOP_LEFT], 3);
        assert_eq!(detect(&board, piece, true, 0), TSpin::Mini);
    }

    /// Walls and the floor count as filled corners, which is how spins against the
    /// edge of the board work at all.
    #[test]
    fn walls_and_floor_count_as_occupied_corners() {
        let mut board = Board::new(20);
        // Hard against the left wall: both left corners are off-board.
        let piece = ActivePiece::new(PieceKind::T, 1, -1, 25);
        // Nub right, so the front corners are the on-board right pair.
        board.set(piece.x + 2, piece.y, Some(PieceKind::I));
        board.set(piece.x + 2, piece.y + 2, Some(PieceKind::I));
        assert_eq!(detect(&board, piece, true, 0), TSpin::Full);

        // Nub down on the floor: the two floor corners are the front pair, but on
        // their own that is only two corners, so it is not yet a spin.
        let mut board = Board::new(20);
        let floor_y = board.height() as i32 - 2;
        let piece = ActivePiece::new(PieceKind::T, 2, 3, floor_y);
        assert_eq!(
            detect(&board, piece, true, 0),
            TSpin::None,
            "a flat floor supplies only two corners"
        );

        // One block above turns it into a genuine spin.
        board.set(piece.x, piece.y, Some(PieceKind::I));
        assert_eq!(detect(&board, piece, true, 0), TSpin::Full);
    }

    #[test]
    fn all_four_corners_filled_is_a_full_spin_in_every_state() {
        for state in 0..4 {
            let (board, piece) = setup(&[TOP_LEFT, TOP_RIGHT, BOTTOM_LEFT, BOTTOM_RIGHT], state);
            assert_eq!(detect(&board, piece, true, 0), TSpin::Full, "state {state}");
        }
    }

    #[test]
    fn front_and_back_corners_never_overlap() {
        for state in 0..4 {
            for front in front_corners(state) {
                assert!(
                    !back_corners(state).contains(&front),
                    "state {state} lists {front:?} as both front and back"
                );
            }
        }
    }
}
