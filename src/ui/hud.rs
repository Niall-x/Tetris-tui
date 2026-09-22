//! Score, level, previews and the mode-specific side panel.
//!
//! The two modes show genuinely different things: NES has one preview piece and a
//! piece-statistics bar, modern has a hold slot, a five-piece queue and combo /
//! back-to-back state. The panels follow the mode rather than showing a blank slot
//! for whatever the current ruleset lacks.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::board_view::piece_color;
use crate::engine::modern::srs;
use crate::engine::nes::rotation as nrs;
use crate::engine::piece::PieceKind;
use crate::game::{Game, Mode};

/// Minimum width the side panel needs to stay readable.
pub const PANEL_WIDTH: u16 = 20;

fn labelled(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label} "), Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{value:>9}"), Style::default().fg(Color::White)),
    ])
}

pub fn render_stats(frame: &mut Frame, area: Rect, game: &Game, timing_label: &str) {
    let mut lines = vec![
        labelled("SCORE", game.score().to_string()),
        labelled("LEVEL", game.level().to_string()),
        labelled("LINES", game.lines().to_string()),
    ];

    if game.back_to_back() {
        lines.push(Line::from(Span::styled(
            "BACK-TO-BACK",
            Style::default().fg(Color::Yellow),
        )));
    } else if let Some(combo) = game.combo().filter(|&c| c > 1) {
        lines.push(Line::from(Span::styled(
            format!("COMBO x{}", combo - 1),
            Style::default().fg(Color::Cyan),
        )));
    } else {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        timing_label,
        Style::default().fg(Color::DarkGray),
    )));

    // The board's border already names the mode, so this panel does not repeat it.
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" SCORE ")),
        area,
    );
}

/// Draw a piece using the same two-columns-per-cell mapping as the board, so
/// previews do not look distorted beside it.
fn preview_lines(kind: PieceKind, mode: Mode) -> Vec<Line<'static>> {
    let cells = match mode {
        Mode::Nes => nrs::cells(kind, 0).to_vec(),
        Mode::Modern => srs::cells(kind, 0).to_vec(),
    };

    let min_x = cells.iter().map(|c| c.0).min().unwrap_or(0);
    let max_x = cells.iter().map(|c| c.0).max().unwrap_or(0);
    let min_y = cells.iter().map(|c| c.1).min().unwrap_or(0);
    let max_y = cells.iter().map(|c| c.1).max().unwrap_or(0);

    let color = piece_color(kind);
    (min_y..=max_y)
        .map(|y| {
            let spans = (min_x..=max_x)
                .map(|x| {
                    if cells.contains(&(x, y)) {
                        Span::styled("██", Style::default().fg(color))
                    } else {
                        Span::raw("  ")
                    }
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

pub fn render_next(frame: &mut Frame, area: Rect, game: &Game) {
    let mode = game.mode();
    let mut lines = Vec::new();

    // NES previews a single piece; modern shows as much of the queue as fits.
    let room = area.height.saturating_sub(2) as usize;
    for kind in game.preview() {
        let piece = preview_lines(kind, mode);
        if lines.len() + piece.len() + 1 > room {
            break;
        }
        lines.extend(piece);
        lines.push(Line::from(""));
    }

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" NEXT ")),
        area,
    );
}

/// The panel whose contents depend entirely on the ruleset: piece statistics for
/// NES, the hold slot for modern.
pub fn render_side_panel(frame: &mut Frame, area: Rect, game: &Game) {
    match game.mode() {
        Mode::Nes => render_piece_counts(frame, area, game),
        Mode::Modern => render_hold(frame, area, game),
    }
}

fn render_hold(frame: &mut Frame, area: Rect, game: &Game) {
    let lines = match game.hold_piece() {
        Some(kind) => preview_lines(kind, game.mode()),
        None => vec![Line::from(Span::styled(
            "  empty",
            Style::default().fg(Color::DarkGray),
        ))],
    };

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" HOLD ")),
        area,
    );
}

fn render_piece_counts(frame: &mut Frame, area: Rect, game: &Game) {
    // The in-game statistics bar order.
    let order = [
        PieceKind::T,
        PieceKind::J,
        PieceKind::Z,
        PieceKind::O,
        PieceKind::S,
        PieceKind::L,
        PieceKind::I,
    ];

    let lines: Vec<Line> = order
        .iter()
        .filter_map(|&kind| {
            game.piece_count(kind).map(|count| {
                Line::from(vec![
                    Span::styled(
                        format!("{} ", kind.letter()),
                        Style::default().fg(piece_color(kind)),
                    ),
                    Span::styled(format!("{count:>5}"), Style::default().fg(Color::White)),
                ])
            })
        })
        .collect();

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" STATS ")),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_use_double_width_cells() {
        let lines = preview_lines(PieceKind::O, Mode::Nes);
        assert_eq!(lines.len(), 2, "O is two rows tall");
        let width: usize = lines[0]
            .spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(width, 4, "two cells at two columns each");
    }

    #[test]
    fn preview_covers_every_piece_in_both_modes() {
        for mode in [Mode::Nes, Mode::Modern] {
            for kind in PieceKind::ALL {
                let lines = preview_lines(kind, mode);
                assert!(!lines.is_empty(), "{kind:?} in {mode:?} produced nothing");
            }
        }
    }

    #[test]
    fn i_preview_is_a_single_row_of_four() {
        for mode in [Mode::Nes, Mode::Modern] {
            let lines = preview_lines(PieceKind::I, mode);
            assert_eq!(lines.len(), 1, "{mode:?}");
            let filled = lines[0]
                .spans
                .iter()
                .filter(|s| s.content.trim() == "██")
                .count();
            assert_eq!(filled, 4, "{mode:?}");
        }
    }

    /// The two rotation systems spawn the T differently, and the preview should
    /// show each mode's own shape.
    #[test]
    fn previews_reflect_the_modes_own_spawn_orientation() {
        let nes = preview_lines(PieceKind::T, Mode::Nes);
        let modern = preview_lines(PieceKind::T, Mode::Modern);
        let render = |lines: &[Line]| {
            lines
                .iter()
                .map(|l| {
                    l.spans
                        .iter()
                        .map(|s| s.content.to_string())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        };
        assert_ne!(
            render(&nes),
            render(&modern),
            "NES spawns the T nub-down, SRS spawns it nub-up"
        );
    }
}
