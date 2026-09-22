//! The playfield grid, shared by both rulesets.
//!
//! Geometry, collision and line clearing are genuinely identical between NES and
//! modern Tetris, so they live here. Anything the two rulesets disagree about
//! (rotation, spawning, top-out, timing, scoring) does not.
//!
//! Coordinates: `y` grows downward. Rows `0..buffer_rows` are the hidden buffer
//! above the visible field; rows `buffer_rows..height` are what the player sees.

use super::piece::PieceKind;

pub const BOARD_WIDTH: usize = 10;
pub const VISIBLE_HEIGHT: usize = 20;

#[derive(Debug, Clone)]
pub struct Board {
    width: usize,
    height: usize,
    buffer_rows: usize,
    cells: Vec<Option<PieceKind>>,
}

impl Board {
    /// `buffer_rows` hidden rows are added above the 20 visible rows. Modern mode
    /// needs them to spawn pieces above the field; NES mode uses 0.
    pub fn new(buffer_rows: usize) -> Self {
        let height = VISIBLE_HEIGHT + buffer_rows;
        Self {
            width: BOARD_WIDTH,
            height,
            buffer_rows,
            cells: vec![None; BOARD_WIDTH * height],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn buffer_rows(&self) -> usize {
        self.buffer_rows
    }

    /// First visible row index. Rows above this are the hidden spawn buffer.
    pub fn visible_top(&self) -> usize {
        self.buffer_rows
    }

    fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    pub fn get(&self, x: i32, y: i32) -> Option<PieceKind> {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return None;
        }
        self.cells[self.index(x as usize, y as usize)]
    }

    pub fn set(&mut self, x: i32, y: i32, kind: Option<PieceKind>) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let i = self.index(x as usize, y as usize);
        self.cells[i] = kind;
    }

    pub fn is_inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }

    pub fn is_occupied(&self, x: i32, y: i32) -> bool {
        self.get(x, y).is_some()
    }

    /// True if any of `cells` is out of bounds or lands on a filled cell.
    pub fn collides(&self, cells: &[(i32, i32)]) -> bool {
        cells
            .iter()
            .any(|&(x, y)| !self.is_inside(x, y) || self.is_occupied(x, y))
    }

    /// Treats out-of-bounds as filled. Used by T-spin corner checks, where walls
    /// and the floor count the same as stack.
    pub fn is_blocked_or_wall(&self, x: i32, y: i32) -> bool {
        !self.is_inside(x, y) || self.is_occupied(x, y)
    }

    pub fn lock_cells(&mut self, cells: &[(i32, i32)], kind: PieceKind) {
        for &(x, y) in cells {
            self.set(x, y, Some(kind));
        }
    }

    pub fn is_row_full(&self, y: usize) -> bool {
        (0..self.width).all(|x| self.cells[self.index(x, y)].is_some())
    }

    pub fn is_row_empty(&self, y: usize) -> bool {
        (0..self.width).all(|x| self.cells[self.index(x, y)].is_none())
    }

    /// Removes every full row, collapsing rows above downward. Returns the cleared
    /// row indices, top-most first.
    pub fn clear_full_lines(&mut self) -> Vec<usize> {
        let cleared: Vec<usize> = (0..self.height).filter(|&y| self.is_row_full(y)).collect();
        for &row in &cleared {
            for y in (1..=row).rev() {
                for x in 0..self.width {
                    let above = self.cells[self.index(x, y - 1)];
                    let i = self.index(x, y);
                    self.cells[i] = above;
                }
            }
            for x in 0..self.width {
                let i = self.index(x, 0);
                self.cells[i] = None;
            }
        }
        cleared
    }

    /// Height of the tallest column, measured in visible rows. 0 = empty field.
    pub fn stack_height(&self) -> usize {
        for y in 0..self.height {
            if !self.is_row_empty(y) {
                return self.height - y;
            }
        }
        0
    }

    /// 0.0 when empty, 1.0 when stacked to the top of the visible field. Feeds the
    /// reactive background's danger signal.
    pub fn stack_height_fraction(&self) -> f32 {
        (self.stack_height() as f32 / VISIBLE_HEIGHT as f32).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_board_is_empty_and_correctly_sized() {
        let board = Board::new(0);
        assert_eq!(board.width(), 10);
        assert_eq!(board.height(), 20);
        assert_eq!(board.stack_height(), 0);

        let buffered = Board::new(20);
        assert_eq!(buffered.height(), 40);
        assert_eq!(buffered.visible_top(), 20);
    }

    #[test]
    fn collision_detects_walls_floor_and_stack() {
        let mut board = Board::new(0);
        assert!(!board.collides(&[(0, 0), (9, 19)]));
        assert!(board.collides(&[(-1, 0)]));
        assert!(board.collides(&[(10, 0)]));
        assert!(board.collides(&[(0, 20)]));
        assert!(board.collides(&[(0, -1)]));

        board.set(5, 5, Some(PieceKind::T));
        assert!(board.collides(&[(5, 5)]));
        assert!(!board.collides(&[(5, 6)]));
    }

    #[test]
    fn out_of_bounds_counts_as_blocked_for_corner_checks() {
        let board = Board::new(0);
        assert!(board.is_blocked_or_wall(-1, 5));
        assert!(board.is_blocked_or_wall(0, 20));
        assert!(!board.is_blocked_or_wall(0, 0));
    }

    #[test]
    fn clearing_a_full_row_collapses_rows_above() {
        let mut board = Board::new(0);
        for x in 0..10 {
            board.set(x, 19, Some(PieceKind::I));
        }
        board.set(3, 18, Some(PieceKind::T));

        let cleared = board.clear_full_lines();
        assert_eq!(cleared, vec![19]);
        assert_eq!(board.get(3, 19), Some(PieceKind::T));
        assert_eq!(board.get(3, 18), None);
        assert_eq!(board.stack_height(), 1);
    }

    #[test]
    fn clears_four_rows_at_once() {
        let mut board = Board::new(0);
        for y in 16..20 {
            for x in 0..10 {
                board.set(x, y, Some(PieceKind::I));
            }
        }
        let cleared = board.clear_full_lines();
        assert_eq!(cleared, vec![16, 17, 18, 19]);
        assert_eq!(board.stack_height(), 0);
    }

    #[test]
    fn clears_non_adjacent_rows_and_keeps_the_gap_contents() {
        let mut board = Board::new(0);
        for x in 0..10 {
            board.set(x, 19, Some(PieceKind::I));
            board.set(x, 17, Some(PieceKind::I));
        }
        board.set(4, 18, Some(PieceKind::S));

        let cleared = board.clear_full_lines();
        assert_eq!(cleared, vec![17, 19]);
        assert_eq!(board.get(4, 19), Some(PieceKind::S));
        assert_eq!(board.stack_height(), 1);
    }

    #[test]
    fn stack_height_fraction_tracks_danger() {
        let mut board = Board::new(0);
        assert_eq!(board.stack_height_fraction(), 0.0);
        board.set(0, 0, Some(PieceKind::O));
        assert_eq!(board.stack_height_fraction(), 1.0);
    }
}
