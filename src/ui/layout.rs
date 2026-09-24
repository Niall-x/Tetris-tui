//! Responsive layout.
//!
//! Both modes share one arrangement, so switching rulesets never moves anything:
//!
//! ```text
//! ┌ HOLD ┐┌──────────┐┌ NEXT ┐
//! └──────┘│          ││      │
//!         │  board   │└──────┘
//! ┌ STATS┐│          │
//! │      ││          │┌ SCORE┐
//! └──────┘└──────────┘└──────┘
//! ```
//!
//! Hold and next sit level with the top of the board, stats and score level
//! with its bottom. Hold top-left and the queue top-right is where Guideline
//! games put them, and NES keeps its piece statistics on the left, so each mode
//! finds its own panels roughly where it expects them. NES has no hold, so its
//! top-left corner is left to the background.
//!
//! The playfield is a fixed 10x20 logical grid, but the space around it is not, so
//! panels are shed as the terminal shrinks rather than letting the board break —
//! the same progression `btop` uses. Below the smallest usable size we show a
//! resize prompt instead of a mangled board.

use ratatui::layout::Rect;

use super::board_view::CELL_WIDTH;
use super::hud::{PANEL_WIDTH, SCORE_HEIGHT, STATS_HEIGHT};
use crate::engine::modern::bag::MAX_PREVIEW;

/// Board interior plus its border.
const BOARD_W: u16 = 10 * CELL_WIDTH + 2;
const BOARD_H: u16 = 20 + 2;

/// Below this there is nowhere to put a 10x20 field at all.
pub const MIN_WIDTH: u16 = BOARD_W;
pub const MIN_HEIGHT: u16 = BOARD_H;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Board flanked by both columns.
    Full,
    /// Board plus the right-hand column: next and score. Hold and stats go.
    Medium,
    /// The board alone, with the essentials along its bottom edge.
    Compact,
    /// Not enough room to play.
    TooSmall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub tier: Tier,
    pub board: Rect,
    pub hold: Option<Rect>,
    pub next: Option<Rect>,
    pub stats: Option<Rect>,
    pub score: Option<Rect>,
}

impl Layout {
    /// Every panel placed, for keeping the background out of them.
    pub fn panels(&self) -> impl Iterator<Item = Rect> {
        [self.hold, self.next, self.stats, self.score]
            .into_iter()
            .flatten()
    }
}

/// Rows a preview piece takes, counting the gap beneath it: no spawn orientation
/// is more than two rows tall.
const PREVIEW_ROWS: u16 = 3;

/// The hold slot holds one piece, at most two rows tall.
const HOLD_HEIGHT: u16 = 2 + 2;

/// Height of a queue of `previews` pieces: each piece and the gap under it, less
/// the last gap, plus the border. The box is sized for the tallest pieces rather
/// than the ones in it, so it does not twitch as the queue moves.
pub fn next_height(previews: usize) -> u16 {
    previews.clamp(1, MAX_PREVIEW) as u16 * PREVIEW_ROWS - 1 + 2
}

/// `previews` is how many upcoming pieces the run shows — one for NES, the
/// player's choice for modern — and sizes the next box. `hold` is whether the
/// mode has a hold slot to place.
pub fn compute(area: Rect, previews: usize, hold: bool) -> Layout {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return Layout {
            tier: Tier::TooSmall,
            board: area,
            hold: None,
            next: None,
            stats: None,
            score: None,
        };
    }

    let one_column = BOARD_W + PANEL_WIDTH;
    let two_columns = BOARD_W + PANEL_WIDTH * 2;

    let tier = if area.width >= two_columns {
        Tier::Full
    } else if area.width >= one_column {
        Tier::Medium
    } else {
        Tier::Compact
    };

    // Centre the whole cluster.
    let cluster_width = match tier {
        Tier::Full => two_columns,
        Tier::Medium => one_column,
        _ => BOARD_W,
    };
    let x = area.x + (area.width.saturating_sub(cluster_width)) / 2;
    let y = area.y + (area.height.saturating_sub(BOARD_H)) / 2;
    let bottom = y + BOARD_H;

    let board_x = if tier == Tier::Full {
        x + PANEL_WIDTH
    } else {
        x
    };
    let board = Rect::new(board_x, y, BOARD_W, BOARD_H);

    let mut layout = Layout {
        tier,
        board,
        hold: None,
        next: None,
        stats: None,
        score: None,
    };
    if tier == Tier::Compact {
        return layout;
    }

    // Six previews and the score box exactly fill the board's height, so the
    // two can never collide.
    let right = board.right();
    layout.next = Some(Rect::new(right, y, PANEL_WIDTH, next_height(previews)));
    layout.score = Some(Rect::new(
        right,
        bottom - SCORE_HEIGHT,
        PANEL_WIDTH,
        SCORE_HEIGHT,
    ));

    if tier == Tier::Full {
        if hold {
            layout.hold = Some(Rect::new(x, y, PANEL_WIDTH, HOLD_HEIGHT));
        }
        layout.stats = Some(Rect::new(
            x,
            bottom - STATS_HEIGHT,
            PANEL_WIDTH,
            STATS_HEIGHT,
        ));
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.intersection(b).area() > 0
    }

    #[test]
    fn a_tiny_terminal_is_reported_as_too_small() {
        assert_eq!(
            compute(Rect::new(0, 0, 20, 20), 5, true).tier,
            Tier::TooSmall
        );
        assert_eq!(
            compute(Rect::new(0, 0, 80, 10), 5, true).tier,
            Tier::TooSmall
        );
        assert_eq!(compute(Rect::new(0, 0, 0, 0), 5, true).tier, Tier::TooSmall);
    }

    #[test]
    fn the_board_alone_fits_at_exactly_the_minimum() {
        let layout = compute(Rect::new(0, 0, MIN_WIDTH, MIN_HEIGHT), 5, true);
        assert_eq!(layout.tier, Tier::Compact);
        assert_eq!(layout.board.width, 22);
        assert_eq!(layout.board.height, 22);
        assert_eq!(layout.panels().count(), 0);
    }

    #[test]
    fn panels_appear_as_the_terminal_grows() {
        assert_eq!(compute(Rect::new(0, 0, 42, 24), 5, true).tier, Tier::Medium);
        assert_eq!(compute(Rect::new(0, 0, 62, 24), 5, true).tier, Tier::Full);
        assert_eq!(compute(Rect::new(0, 0, 120, 40), 5, true).tier, Tier::Full);
    }

    /// The right-hand column is what a run cannot do without: what is coming and
    /// the score. It is the one kept when only one fits.
    #[test]
    fn medium_keeps_next_and_score_and_drops_the_left_column() {
        let layout = compute(Rect::new(0, 0, 45, 30), 5, true);
        assert_eq!(layout.tier, Tier::Medium);
        assert!(layout.next.is_some());
        assert!(layout.score.is_some());
        assert!(layout.hold.is_none());
        assert!(layout.stats.is_none());
    }

    #[test]
    fn hold_and_stats_go_left_of_the_board_next_and_score_right() {
        let layout = compute(Rect::new(0, 0, 120, 40), 5, true);
        let board = layout.board;
        for left in [layout.hold.unwrap(), layout.stats.unwrap()] {
            assert_eq!(left.right(), board.left());
        }
        for right in [layout.next.unwrap(), layout.score.unwrap()] {
            assert_eq!(right.left(), board.right());
        }
    }

    #[test]
    fn top_panels_align_with_the_board_top_and_bottom_panels_with_its_bottom() {
        let layout = compute(Rect::new(0, 0, 120, 40), 5, true);
        let board = layout.board;
        assert_eq!(layout.hold.unwrap().top(), board.top());
        assert_eq!(layout.next.unwrap().top(), board.top());
        assert_eq!(layout.stats.unwrap().bottom(), board.bottom());
        assert_eq!(layout.score.unwrap().bottom(), board.bottom());
    }

    /// NES has no hold, and the stats panel keeps its place rather than sliding
    /// up into the gap, so the two modes look alike.
    #[test]
    fn nes_has_no_hold_slot_but_the_same_stats_panel() {
        let area = Rect::new(0, 0, 120, 40);
        let nes = compute(area, 1, false);
        let modern = compute(area, 5, true);
        assert!(nes.hold.is_none());
        assert_eq!(nes.stats, modern.stats);
        assert_eq!(nes.score, modern.score);
        assert_eq!(nes.board, modern.board);
    }

    #[test]
    fn the_next_box_grows_with_the_queue_and_never_meets_the_score() {
        let area = Rect::new(0, 0, 120, 40);
        let mut last = 0;
        for previews in 1..=MAX_PREVIEW {
            let layout = compute(area, previews, true);
            let next = layout.next.unwrap();
            assert!(next.height > last, "{previews} previews");
            assert!(
                !overlaps(next, layout.score.unwrap()),
                "{previews} previews run into the score"
            );
            last = next.height;
        }
        // A single piece needs its two rows and a border, nothing more.
        assert_eq!(next_height(1), 4);
    }

    #[test]
    fn no_panels_overlap_each_other_or_the_board() {
        for (width, height) in [(42u16, 22u16), (62, 22), (100, 30), (200, 60)] {
            for previews in 1..=MAX_PREVIEW {
                let layout = compute(Rect::new(0, 0, width, height), previews, true);
                let rects: Vec<Rect> = std::iter::once(layout.board)
                    .chain(layout.panels())
                    .collect();
                for (i, a) in rects.iter().enumerate() {
                    for b in &rects[i + 1..] {
                        assert!(!overlaps(*a, *b), "{a:?} overlaps {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn nothing_is_placed_outside_the_terminal() {
        for width in [22u16, 30, 45, 62, 100, 200] {
            for height in [22u16, 24, 30, 50] {
                let area = Rect::new(0, 0, width, height);
                let layout = compute(area, MAX_PREVIEW, true);
                if layout.tier == Tier::TooSmall {
                    continue;
                }
                for rect in std::iter::once(layout.board).chain(layout.panels()) {
                    assert!(
                        rect.left() >= area.left()
                            && rect.right() <= area.right()
                            && rect.bottom() <= area.bottom(),
                        "{rect:?} escapes {area:?} at tier {:?}",
                        layout.tier
                    );
                }
            }
        }
    }

    #[test]
    fn the_board_is_centred_in_a_large_terminal() {
        let area = Rect::new(0, 0, 200, 60);
        let layout = compute(area, 5, true);
        let left_gap = layout.stats.unwrap().left();
        let right_gap = area.right() - layout.next.unwrap().right();
        assert!(left_gap.abs_diff(right_gap) <= 1);
        assert!(layout.board.y > 10);
    }
}
