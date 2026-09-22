//! Drawing the playfield.
//!
//! Each board cell becomes two terminal columns, because a terminal cell is about
//! twice as tall as it is wide and one column per cell would render visibly squashed
//! blocks.
//!
//! Empty cells are left on `Color::Reset` rather than being painted a solid colour,
//! so a terminal configured with transparency or a compositor blur shows through.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use crate::engine::board::Board;
use crate::engine::piece::{ActivePiece, PieceKind};
use crate::game::Game;

/// Terminal columns used per board cell.
pub const CELL_WIDTH: u16 = 2;

/// The Guideline colours. Phase 6 generalises this into a selectable theme; the
/// mapping lives here until there is a second palette to choose between.
pub fn piece_color(kind: PieceKind) -> Color {
    match kind {
        PieceKind::I => Color::Cyan,
        PieceKind::O => Color::Yellow,
        PieceKind::T => Color::Magenta,
        PieceKind::S => Color::Green,
        PieceKind::Z => Color::Red,
        PieceKind::J => Color::Blue,
        PieceKind::L => Color::LightRed,
    }
}

/// Terminal size needed to draw `board` including its border.
pub fn required_size(board: &Board) -> (u16, u16) {
    let width = board.width() as u16 * CELL_WIDTH + 2;
    let height = (board.height() - board.buffer_rows()) as u16 + 2;
    (width, height)
}

fn paint_cell(buf: &mut Buffer, area: Rect, col: u16, row: u16, glyph: &str, style: Style) {
    for i in 0..CELL_WIDTH {
        let x = area.x + col * CELL_WIDTH + i;
        let y = area.y + row;
        if x < area.right() && y < area.bottom() {
            buf[(x, y)].set_symbol(glyph).set_style(style);
        }
    }
}

/// Render the locked stack, the ghost, the active piece, and any rows mid-clear.
///
/// `area` is the playfield interior, excluding the border.
pub fn render(buf: &mut Buffer, area: Rect, game: &Game) {
    let board = game.board();
    let clearing_rows = game.clearing_rows();
    let top = board.visible_top();

    for y in top..board.height() {
        let row = (y - top) as u16;

        if clearing_rows.contains(&y) {
            // Flash a cleared row as a solid bar, the NES line-clear tell.
            for x in 0..board.width() as u16 {
                paint_cell(buf, area, x, row, "█", Style::default().fg(Color::White));
            }
            continue;
        }

        for x in 0..board.width() {
            let style = match board.get(x as i32, y as i32) {
                Some(kind) => Style::default().fg(piece_color(kind)),
                // Untouched background: keeps terminal transparency intact.
                None => Style::default().fg(Color::DarkGray),
            };
            let glyph = if board.get(x as i32, y as i32).is_some() {
                "█"
            } else {
                "·"
            };
            paint_cell(buf, area, x as u16, row, glyph, style);
        }
    }

    // The ghost goes down first so the real piece paints over it where they meet.
    if let Some(ghost) = game.ghost() {
        let style = Style::default().fg(piece_color(ghost.kind));
        paint_piece(buf, area, game, ghost, top, "▒", style);
    }

    if let Some(piece) = game.current() {
        let style = Style::default().fg(piece_color(piece.kind));
        paint_piece(buf, area, game, piece, top, "█", style);
    }
}

fn paint_piece(
    buf: &mut Buffer,
    area: Rect,
    game: &Game,
    piece: ActivePiece,
    top: usize,
    glyph: &str,
    style: Style,
) {
    let board = game.board();
    for (cx, cy) in game.cells_of(piece) {
        // Cells in the hidden spawn rows are simply not drawn.
        if cy < top as i32 || cy >= board.height() as i32 || cx < 0 {
            continue;
        }
        paint_cell(buf, area, cx as u16, (cy - top as i32) as u16, glyph, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_size_accounts_for_double_width_cells_and_border() {
        let board = Board::new(0);
        assert_eq!(required_size(&board), (22, 22));
    }

    #[test]
    fn buffer_rows_are_not_counted_in_visible_height() {
        let board = Board::new(20);
        let (_, height) = required_size(&board);
        assert_eq!(height, 22, "hidden spawn rows must not be drawn");
    }

    use crate::game::{Game, Input, Mode};

    fn draw(game: &Game) -> Buffer {
        let area = Rect::new(0, 0, 20, 20);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, game);
        buf
    }

    #[test]
    fn locked_cells_paint_two_columns_each() {
        let mut game = Game::new(Mode::Nes, 0);
        let bottom = game.board().height() as i32 - 1;
        game.board_mut().set(0, bottom, Some(PieceKind::I));

        let buf = draw(&game);
        let row = (bottom - game.board().visible_top() as i32) as u16;
        assert_eq!(buf[(0, row)].symbol(), "█");
        assert_eq!(buf[(1, row)].symbol(), "█");
        assert_eq!(buf[(0, row)].fg, Color::Cyan);
    }

    #[test]
    fn empty_cells_keep_the_default_background_for_transparency() {
        let game = Game::new(Mode::Nes, 0);
        let buf = draw(&game);
        assert_eq!(
            buf[(0, 10)].bg,
            Color::Reset,
            "painting a solid background would defeat terminal transparency"
        );
    }

    #[test]
    fn the_active_piece_is_drawn_on_the_field() {
        let game = Game::new(Mode::Nes, 0);
        let piece = game.current().unwrap();
        let top = game.board().visible_top() as i32;
        let buf = draw(&game);

        for (cx, cy) in game.cells_of(piece) {
            if cy < top {
                continue;
            }
            let row = (cy - top) as u16;
            assert_eq!(
                buf[(cx as u16 * CELL_WIDTH, row)].symbol(),
                "█",
                "piece cell ({cx}, {cy}) was not drawn"
            );
        }
    }

    #[test]
    fn clearing_rows_render_as_a_flash() {
        let mut game = Game::new(Mode::Nes, 0);
        let bottom = game.board().height() as i32 - 1;
        for x in 0..10 {
            game.board_mut().set(x, bottom, Some(PieceKind::I));
        }

        // Soft drop until a piece locks, which puts the already-full row into the
        // clear animation.
        let down = Input {
            soft_drop: true,
            ..Default::default()
        };
        for _ in 0..500 {
            if game.tick(down).piece_locked {
                break;
            }
        }
        assert!(!game.clearing_rows().is_empty(), "expected a row mid-clear");

        let buf = draw(&game);
        let row = (bottom - game.board().visible_top() as i32) as u16;
        assert_eq!(buf[(0, row)].fg, Color::White);
    }

    /// Modern mode draws a ghost; NES has none.
    #[test]
    fn the_ghost_is_drawn_beneath_the_piece_in_modern_mode() {
        let game = Game::new(Mode::Modern, 1);
        let ghost = game.ghost().expect("modern mode has a ghost");
        let top = game.board().visible_top() as i32;
        let buf = draw(&game);

        let (gx, gy) = game.cells_of(ghost)[0];
        assert_eq!(
            buf[(gx as u16 * CELL_WIDTH, (gy - top) as u16)].symbol(),
            "▒"
        );

        let nes = Game::new(Mode::Nes, 0);
        assert!(nes.ghost().is_none());
    }
}
