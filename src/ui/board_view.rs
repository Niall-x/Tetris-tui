//! Drawing the playfield.
//!
//! Each board cell becomes two terminal columns, because a terminal cell is about
//! twice as tall as it is wide and one column per cell would render visibly squashed
//! blocks. The two columns can differ — the letter and bracket skins use that — so
//! a cell is painted from a glyph *pair* rather than one repeated glyph.
//!
//! Empty cells are left on `Color::Reset` rather than being painted a solid colour,
//! so a terminal configured with transparency or a compositor blur shows through.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::engine::board::Board;
use crate::engine::piece::ActivePiece;
use crate::game::Game;
use crate::ui::style::{CellRole, Visuals};

/// Terminal columns used per board cell.
pub const CELL_WIDTH: u16 = 2;

/// Terminal size needed to draw `board` including its border.
pub fn required_size(board: &Board) -> (u16, u16) {
    let width = board.width() as u16 * CELL_WIDTH + 2;
    let height = (board.height() - board.buffer_rows()) as u16 + 2;
    (width, height)
}

fn paint_cell(buf: &mut Buffer, area: Rect, col: u16, row: u16, glyphs: [&str; 2], style: Style) {
    for (i, glyph) in glyphs.iter().enumerate() {
        let x = area.x + col * CELL_WIDTH + i as u16;
        let y = area.y + row;
        if x < area.right() && y < area.bottom() {
            buf[(x, y)].set_symbol(glyph).set_style(style);
        }
    }
}

/// Render the locked stack, the ghost, the active piece, and any rows mid-clear.
///
/// `area` is the playfield interior, excluding the border.
pub fn render(buf: &mut Buffer, area: Rect, game: &Game, visuals: &Visuals) {
    let board = game.board();
    let clearing_rows = game.clearing_rows();
    let top = board.visible_top();

    for y in top..board.height() {
        let row = (y - top) as u16;

        if clearing_rows.contains(&y) {
            // Flash a cleared row as a solid bar, the NES line-clear tell.
            for x in 0..board.width() as u16 {
                let style = Style::default().fg(visuals.theme.flash());
                paint_cell(buf, area, x, row, visuals.skin.flash(), style);
            }
            continue;
        }

        for x in 0..board.width() {
            let (glyphs, style) = match board.get(x as i32, y as i32) {
                Some(kind) => (
                    visuals.skin.cell(kind, CellRole::Filled),
                    Style::default().fg(visuals.theme.color(kind)),
                ),
                // Untouched background: keeps terminal transparency intact.
                None => (
                    visuals.skin.empty(),
                    Style::default().fg(visuals.theme.grid()),
                ),
            };
            paint_cell(buf, area, x as u16, row, glyphs, style);
        }
    }

    // The ghost goes down first so the real piece paints over it where they meet.
    if let Some(ghost) = game.ghost() {
        paint_piece(buf, area, game, ghost, top, CellRole::Ghost, visuals);
    }

    if let Some(piece) = game.current() {
        paint_piece(buf, area, game, piece, top, CellRole::Filled, visuals);
    }
}

fn paint_piece(
    buf: &mut Buffer,
    area: Rect,
    game: &Game,
    piece: ActivePiece,
    top: usize,
    role: CellRole,
    visuals: &Visuals,
) {
    let board = game.board();
    let glyphs = visuals.skin.cell(piece.kind, role);
    let style = Style::default().fg(visuals.theme.color(piece.kind));

    for (cx, cy) in game.cells_of(piece) {
        // Cells in the hidden spawn rows are simply not drawn.
        if cy < top as i32 || cy >= board.height() as i32 || cx < 0 {
            continue;
        }
        paint_cell(
            buf,
            area,
            cx as u16,
            (cy - top as i32) as u16,
            glyphs,
            style,
        );
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

    use crate::engine::piece::PieceKind;
    use crate::game::{Game, Input, Mode};
    use crate::ui::style::{Skin, Theme};
    use ratatui::style::Color;

    fn draw(game: &Game) -> Buffer {
        draw_with(game, &Visuals::default())
    }

    fn draw_with(game: &Game, visuals: &Visuals) -> Buffer {
        let area = Rect::new(0, 0, 20, 20);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, game, visuals);
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
        assert_eq!(buf[(0, row)].fg, Theme::default().color(PieceKind::I));
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
        assert_eq!(buf[(0, row)].fg, Theme::default().flash());
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

    /// The two columns of a cell are not always the same glyph — the bracket and
    /// letter skins depend on being able to differ.
    #[test]
    fn a_skin_can_paint_the_two_columns_of_a_cell_differently() {
        let mut game = Game::new(Mode::Nes, 0);
        let bottom = game.board().height() as i32 - 1;
        game.board_mut().set(0, bottom, Some(PieceKind::T));
        let row = (bottom - game.board().visible_top() as i32) as u16;

        let visuals = Visuals {
            skin: Skin::AsciiBracket,
            ..Default::default()
        };
        let buf = draw_with(&game, &visuals);
        assert_eq!(buf[(0, row)].symbol(), "[");
        assert_eq!(buf[(1, row)].symbol(), "]");

        let visuals = Visuals {
            skin: Skin::Letter,
            ..Default::default()
        };
        let buf = draw_with(&game, &visuals);
        assert_eq!(buf[(0, row)].symbol(), "T", "letters name the piece");
        assert_eq!(buf[(1, row)].symbol(), " ");
    }

    /// Whatever the skin, an empty cell must stay unpainted so transparency holds.
    #[test]
    fn no_skin_paints_a_background_over_an_empty_cell() {
        let game = Game::new(Mode::Nes, 0);
        for skin in Skin::ALL {
            let visuals = Visuals {
                skin,
                ..Default::default()
            };
            let buf = draw_with(&game, &visuals);
            assert_eq!(buf[(0, 10)].bg, Color::Reset, "{skin:?}");
        }
    }

    /// Renders a real board in every skin and dumps it, so the glyph choices can
    /// be looked at rather than imagined.
    #[test]
    fn every_skin_draws_a_board() {
        let mut game = Game::new(Mode::Modern, 1);
        let bottom = game.board().height() as i32 - 1;
        for x in 0..7 {
            game.board_mut()
                .set(x, bottom, Some(PieceKind::ALL[x as usize]));
        }
        game.board_mut().set(0, bottom - 1, Some(PieceKind::S));

        for skin in Skin::ALL {
            let visuals = Visuals {
                skin,
                ..Default::default()
            };
            let buf = draw_with(&game, &visuals);
            let mut rendered = String::new();
            for y in 0..buf.area.height {
                for x in 0..buf.area.width {
                    rendered.push_str(buf[(x, y)].symbol());
                }
                rendered.push('\n');
            }
            println!("=== {} ===\n{rendered}", skin.label());

            // Whatever the glyphs, the row is still ten double-width cells.
            let row = (bottom - game.board().visible_top() as i32) as u16;
            let width: usize = (0..20)
                .map(|x| buf[(x, row)].symbol().chars().count())
                .sum();
            assert_eq!(width, 20, "{skin:?} changed the board's width");
        }
    }

    #[test]
    fn the_theme_decides_the_piece_colour() {
        let mut game = Game::new(Mode::Nes, 0);
        let bottom = game.board().height() as i32 - 1;
        game.board_mut().set(0, bottom, Some(PieceKind::Z));
        let row = (bottom - game.board().visible_top() as i32) as u16;

        for theme in Theme::ALL {
            let visuals = Visuals {
                theme,
                ..Default::default()
            };
            let buf = draw_with(&game, &visuals);
            assert_eq!(buf[(0, row)].fg, theme.color(PieceKind::Z), "{theme:?}");
        }
    }
}
