//! Responsive layout.
//!
//! The playfield is a fixed 10x20 logical grid, but the space around it is not, so
//! panels are shed as the terminal shrinks rather than letting the board break —
//! the same progression `btop` uses. Below the smallest usable size we show a
//! resize prompt instead of a mangled board.

use ratatui::layout::Rect;

use super::board_view::CELL_WIDTH;
use super::hud::PANEL_WIDTH;

/// Board interior plus its border.
const BOARD_W: u16 = 10 * CELL_WIDTH + 2;
const BOARD_H: u16 = 20 + 2;

/// Below this there is nowhere to put a 10x20 field at all.
pub const MIN_WIDTH: u16 = BOARD_W;
pub const MIN_HEIGHT: u16 = BOARD_H;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Board plus stats, next and piece counts.
    Full,
    /// Board plus stats and next; piece counts dropped.
    Medium,
    /// Board and a compact stats line only.
    Compact,
    /// Not enough room to play.
    TooSmall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub tier: Tier,
    pub board: Rect,
    pub stats: Option<Rect>,
    pub next: Option<Rect>,
    pub piece_counts: Option<Rect>,
}

/// Rows one preview piece occupies, including the gap beneath it.
const PREVIEW_ROWS: u16 = 3;

/// `preview_slots` is how many upcoming pieces the active mode shows — one for
/// NES, a queue for modern — so the panel is sized to its contents rather than
/// leaving a tall empty box.
pub fn compute(area: Rect, preview_slots: usize) -> Layout {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return Layout {
            tier: Tier::TooSmall,
            board: area,
            stats: None,
            next: None,
            piece_counts: None,
        };
    }

    let one_panel = BOARD_W + PANEL_WIDTH;
    let two_panels = BOARD_W + PANEL_WIDTH * 2;

    let tier = if area.width >= two_panels && area.height >= BOARD_H {
        Tier::Full
    } else if area.width >= one_panel {
        Tier::Medium
    } else {
        Tier::Compact
    };

    // Centre the whole cluster horizontally.
    let cluster_width = match tier {
        Tier::Full => two_panels,
        Tier::Medium => one_panel,
        _ => BOARD_W,
    };
    let x = area.x + (area.width.saturating_sub(cluster_width)) / 2;
    let y = area.y + (area.height.saturating_sub(BOARD_H)) / 2;

    let board = Rect::new(x, y, BOARD_W, BOARD_H);

    match tier {
        Tier::Compact => Layout {
            tier,
            board,
            stats: None,
            next: None,
            piece_counts: None,
        },
        Tier::Medium | Tier::Full => {
            let panel_x = board.right();
            // Score needs 5 content rows plus its border; the preview queue takes
            // whatever is left, so modern mode can show several upcoming pieces.
            let stats_h = 7.min(board.height);
            let wanted = preview_slots.max(1) as u16 * PREVIEW_ROWS + 2;
            let next_h = wanted.min(board.height.saturating_sub(stats_h));

            Layout {
                tier,
                board,
                stats: Some(Rect::new(panel_x, y, PANEL_WIDTH, stats_h)),
                next: Some(Rect::new(panel_x, y + stats_h, PANEL_WIDTH, next_h)),
                piece_counts: if tier == Tier::Full {
                    Some(Rect::new(panel_x + PANEL_WIDTH, y, PANEL_WIDTH, 11))
                } else {
                    None
                },
            }
        }
        Tier::TooSmall => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tiny_terminal_is_reported_as_too_small() {
        assert_eq!(compute(Rect::new(0, 0, 20, 20), 5).tier, Tier::TooSmall);
        assert_eq!(compute(Rect::new(0, 0, 80, 10), 5).tier, Tier::TooSmall);
        assert_eq!(compute(Rect::new(0, 0, 0, 0), 5).tier, Tier::TooSmall);
    }

    #[test]
    fn the_board_alone_fits_at_exactly_the_minimum() {
        let layout = compute(Rect::new(0, 0, MIN_WIDTH, MIN_HEIGHT), 5);
        assert_eq!(layout.tier, Tier::Compact);
        assert_eq!(layout.board.width, 22);
        assert_eq!(layout.board.height, 22);
        assert!(layout.stats.is_none());
    }

    #[test]
    fn panels_appear_as_the_terminal_grows() {
        assert_eq!(compute(Rect::new(0, 0, 42, 24), 5).tier, Tier::Medium);
        assert_eq!(compute(Rect::new(0, 0, 62, 24), 5).tier, Tier::Full);
        assert_eq!(compute(Rect::new(0, 0, 120, 40), 5).tier, Tier::Full);
    }

    #[test]
    fn medium_keeps_stats_and_next_but_drops_piece_counts() {
        let layout = compute(Rect::new(0, 0, 45, 30), 5);
        assert_eq!(layout.tier, Tier::Medium);
        assert!(layout.stats.is_some());
        assert!(layout.next.is_some());
        assert!(layout.piece_counts.is_none());
    }

    #[test]
    fn nothing_is_placed_outside_the_terminal() {
        for width in [22u16, 30, 45, 62, 100, 200] {
            for height in [22u16, 24, 30, 50] {
                let area = Rect::new(0, 0, width, height);
                let layout = compute(area, 5);
                if layout.tier == Tier::TooSmall {
                    continue;
                }
                for rect in [
                    Some(layout.board),
                    layout.stats,
                    layout.next,
                    layout.piece_counts,
                ]
                .into_iter()
                .flatten()
                {
                    assert!(
                        rect.right() <= area.right() && rect.bottom() <= area.bottom(),
                        "{rect:?} escapes {area:?} at tier {:?}",
                        layout.tier
                    );
                }
            }
        }
    }

    /// The preview panel is sized to the queue, so NES's single next piece does not
    /// leave a tall empty box.
    #[test]
    fn the_preview_panel_follows_the_queue_length() {
        let area = Rect::new(0, 0, 120, 40);
        let one = compute(area, 1).next.unwrap();
        let five = compute(area, 5).next.unwrap();
        assert!(five.height > one.height);
        assert_eq!(one.height, PREVIEW_ROWS + 2);
    }

    #[test]
    fn the_board_is_centred_in_a_large_terminal() {
        let area = Rect::new(0, 0, 200, 60);
        let layout = compute(area, 5);
        assert!(layout.board.x > 50, "board should be nudged toward centre");
        assert!(layout.board.y > 10);
    }
}
