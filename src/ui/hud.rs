//! The four panels around the board: hold, next, stats and score.
//!
//! Both modes use the same four, in the same places (see `layout`), and fill them
//! with what their ruleset has. NES previews one piece and has no hold slot;
//! modern previews as many as the player asked for and adds combo and
//! back-to-back to the stats. A panel whose feature the ruleset lacks is not
//! drawn at all, rather than shown empty.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph};
use ratatui::Frame;

use super::style::{CellRole, Visuals};
use crate::engine::modern::srs;
use crate::engine::nes::rotation as nrs;
use crate::engine::piece::PieceKind;
use crate::game::{Game, Mode};

/// Minimum width a side panel needs to stay readable.
pub const PANEL_WIDTH: u16 = 20;

/// The score on one line, inside a border.
pub const SCORE_HEIGHT: u16 = 1 + 2;

/// Level, lines, the combo line and the seven piece counts, inside a border.
pub const STATS_HEIGHT: u16 = 3 + 7 + 2;

/// Width of a stats row, so labels and values line up down the panel: the
/// panel's interior less a column of padding either side.
const STATS_ROW: usize = PANEL_WIDTH as usize - 2 - 2;

/// The order NES's statistics panel lists pieces in. Modern uses it too, so
/// the panel reads the same in both modes.
const STATS_ORDER: [PieceKind; 7] = [
    PieceKind::T,
    PieceKind::J,
    PieceKind::Z,
    PieceKind::O,
    PieceKind::S,
    PieceKind::L,
    PieceKind::I,
];

/// A stats row: `label` on the left in `label_style`, `value` right-aligned.
fn stat_row(label: String, label_style: Style, value: String) -> Line<'static> {
    let pad = STATS_ROW.saturating_sub(label.chars().count());
    Line::from(vec![
        Span::styled(label, label_style),
        Span::styled(format!("{value:>pad$}"), Style::default().fg(Color::White)),
    ])
}

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn render_score(frame: &mut Frame, area: Rect, game: &Game, visuals: &Visuals) {
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            game.score().to_string(),
            Style::default().fg(Color::White),
        )))
        .alignment(Alignment::Center)
        .block(visuals.border.apply(Block::default().title(" SCORE "))),
        area,
    );
}

/// Level, lines, what the scoring state is doing, and how many of each piece
/// the run has dealt. `timing_label` says whether key releases are real or
/// inferred, which is worth seeing but not worth a row of its own.
pub fn render_stats(
    frame: &mut Frame,
    area: Rect,
    game: &Game,
    timing_label: &str,
    visuals: &Visuals,
) {
    let mut lines = vec![
        stat_row("LEVEL".into(), dim(), game.level().to_string()),
        stat_row("LINES".into(), dim(), game.lines().to_string()),
    ];

    // Always a row, blank when there is nothing to say, so the piece counts
    // below never jump.
    lines.push(if game.back_to_back() {
        Line::from(Span::styled(
            "BACK-TO-BACK",
            Style::default().fg(Color::Yellow),
        ))
    } else if let Some(combo) = game.combo().filter(|&c| c > 1) {
        Line::from(Span::styled(
            format!("COMBO x{}", combo - 1),
            Style::default().fg(Color::Cyan),
        ))
    } else {
        Line::from("")
    });

    lines.extend(STATS_ORDER.iter().map(|&kind| {
        stat_row(
            kind.letter().into(),
            Style::default().fg(visuals.theme.color(kind)),
            game.piece_count(kind).to_string(),
        )
    }));

    let block = visuals.border.apply(
        Block::default()
            .title(" STATS ")
            .padding(Padding::horizontal(1))
            .title_bottom(Line::from(Span::styled(format!(" {timing_label} "), dim()))),
    );
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Draw a piece using the same two-columns-per-cell mapping as the board, so
/// previews do not look distorted beside it.
fn preview_lines(kind: PieceKind, mode: Mode, visuals: &Visuals) -> Vec<Line<'static>> {
    let cells = match mode {
        Mode::Nes => nrs::cells(kind, 0).to_vec(),
        Mode::Modern => srs::cells(kind, 0).to_vec(),
    };

    let min_x = cells.iter().map(|c| c.0).min().unwrap_or(0);
    let max_x = cells.iter().map(|c| c.0).max().unwrap_or(0);
    let min_y = cells.iter().map(|c| c.1).min().unwrap_or(0);
    let max_y = cells.iter().map(|c| c.1).max().unwrap_or(0);

    let color = visuals.theme.color(kind);
    let glyphs = visuals.skin.cell(kind, CellRole::Filled);
    let cell = format!("{}{}", glyphs[0], glyphs[1]);
    (min_y..=max_y)
        .map(|y| {
            let spans = (min_x..=max_x)
                .map(|x| {
                    if cells.contains(&(x, y)) {
                        Span::styled(cell.clone(), Style::default().fg(color))
                    } else {
                        Span::raw("  ")
                    }
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

/// The queue, one piece per three-row slot. Each slot has a fixed height
/// whatever the piece in it, so the pieces step up by exactly one slot as the
/// queue advances instead of shuffling about with the I's single row.
fn queue_lines(queue: &[PieceKind], mode: Mode, visuals: &Visuals) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, &kind) in queue.iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }
        let piece = preview_lines(kind, mode, visuals);
        let pad = 2usize.saturating_sub(piece.len());
        lines.extend(piece);
        lines.extend(std::iter::repeat_n(Line::from(""), pad));
    }
    lines
}

pub fn render_next(frame: &mut Frame, area: Rect, game: &Game, visuals: &Visuals) {
    let lines = queue_lines(&game.preview(), game.mode(), visuals);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(visuals.border.apply(Block::default().title(" NEXT "))),
        area,
    );
}

pub fn render_hold(frame: &mut Frame, area: Rect, game: &Game, visuals: &Visuals) {
    let lines = match game.hold_piece() {
        Some(kind) => preview_lines(kind, game.mode(), visuals),
        None => vec![Line::from(Span::styled("empty", dim()))],
    };

    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(visuals.border.apply(Block::default().title(" HOLD "))),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::layout::next_height;

    #[test]
    fn previews_use_double_width_cells() {
        let lines = preview_lines(PieceKind::O, Mode::Nes, &Visuals::default());
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
                let lines = preview_lines(kind, mode, &Visuals::default());
                assert!(!lines.is_empty(), "{kind:?} in {mode:?} produced nothing");
                assert!(lines.len() <= 2, "{kind:?} in {mode:?} is too tall a slot");
            }
        }
    }

    #[test]
    fn i_preview_is_a_single_row_of_four() {
        for mode in [Mode::Nes, Mode::Modern] {
            let lines = preview_lines(PieceKind::I, mode, &Visuals::default());
            assert_eq!(lines.len(), 1, "{mode:?}");
            let filled = lines[0]
                .spans
                .iter()
                .filter(|s| s.content.trim() == "██")
                .count();
            assert_eq!(filled, 4, "{mode:?}");
        }
    }

    /// Every slot is the same height, so an I does not pull the rest of the
    /// queue up a row, and the full queue fills exactly the box made for it.
    #[test]
    fn queue_slots_are_a_fixed_height_and_fill_the_box() {
        let visuals = Visuals::default();
        let with_i = queue_lines(&[PieceKind::I, PieceKind::T], Mode::Modern, &visuals);
        let without = queue_lines(&[PieceKind::O, PieceKind::T], Mode::Modern, &visuals);
        assert_eq!(with_i.len(), without.len());

        for count in 1..=6 {
            let queue = vec![PieceKind::T; count];
            let lines = queue_lines(&queue, Mode::Modern, &visuals);
            assert_eq!(lines.len() as u16 + 2, next_height(count), "{count} pieces");
        }
    }

    /// The two rotation systems spawn the T differently, and the preview should
    /// show each mode's own shape.
    #[test]
    fn previews_reflect_the_modes_own_spawn_orientation() {
        let nes = preview_lines(PieceKind::T, Mode::Nes, &Visuals::default());
        let modern = preview_lines(PieceKind::T, Mode::Modern, &Visuals::default());
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
