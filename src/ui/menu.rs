//! Drawing for the non-gameplay screens: title, options, high scores, and the
//! pause / game-over overlays.
//!
//! All of these are driven by state in `crate::menu`; nothing here decides
//! anything, so the screens can be redrawn at any size without the menu logic
//! caring. Everything is sized defensively: the same 22x22 terminal that can just
//! about hold a playfield also has to hold these.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::config::Config;
use crate::game::Mode;
use crate::input::keymap::Keymap;
use crate::input::keyname::display_name;
use crate::menu::{
    GameOverItem, GameOverMenu, OptionRow, OptionsMenu, PauseItem, PauseMenu, ScoresView,
    TitleItem, TitleMenu,
};
use crate::scores::Scores;
use crate::ui::style::BorderStyle;

/// Figlet "ANSI Shadow", which is 45 columns wide — below that the title screen
/// falls back to plain text rather than wrapping into rubble.
const LOGO: [&str; 6] = [
    "████████╗███████╗████████╗██████╗ ██╗███████╗",
    "╚══██╔══╝██╔════╝╚══██╔══╝██╔══██╗██║██╔════╝",
    "   ██║   █████╗     ██║   ██████╔╝██║███████╗",
    "   ██║   ██╔══╝     ██║   ██╔══██╗██║╚════██║",
    "   ██║   ███████╗   ██║   ██║  ██║██║███████║",
    "   ╚═╝   ╚══════╝   ╚═╝   ╚═╝  ╚═╝╚═╝╚══════╝",
];

const LOGO_WIDTH: u16 = 45;

/// The tetromino palette, applied a row at a time down the logo.
const LOGO_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Yellow,
    Color::Magenta,
    Color::Green,
    Color::Red,
    Color::Blue,
];

const SELECTED: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::White)
    .add_modifier(Modifier::BOLD);

fn dim(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        text.into(),
        Style::default().fg(Color::DarkGray),
    ))
}

/// A centred rect, clamped to what the terminal actually has.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// The slice of a long list to show, keeping the cursor inside the window.
fn window(len: usize, selected: usize, height: usize) -> (usize, usize) {
    if height == 0 || len == 0 {
        return (0, 0);
    }
    if len <= height {
        return (0, len);
    }
    // Keep the cursor centred where possible, pinned at the ends otherwise.
    let half = height / 2;
    let start = selected.saturating_sub(half).min(len - height);
    (start, start + height)
}

fn menu_line(label: &str, selected: bool) -> Line<'static> {
    let marker = if selected { "> " } else { "  " };
    let style = if selected {
        SELECTED
    } else {
        Style::default().fg(Color::White)
    };
    Line::from(Span::styled(format!("{marker}{label}"), style))
}

/// A column of menu entries, each padded to the widest, so a centred list does
/// not shuffle sideways as the cursor moves down it.
fn menu_column(labels: &[String], selected: usize) -> Vec<Line<'static>> {
    let widest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    labels
        .iter()
        .enumerate()
        .map(|(index, label)| menu_line(&format!("{label:<widest$}"), index == selected))
        .collect()
}

pub fn render_title(frame: &mut Frame, area: Rect, menu: &TitleMenu, mode: Mode) {
    let mut lines: Vec<Line> = Vec::new();

    if area.width >= LOGO_WIDTH && area.height >= 16 {
        for (row, colour) in LOGO.iter().zip(LOGO_COLORS) {
            lines.push(Line::from(Span::styled(*row, Style::default().fg(colour))));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "T E T R I S",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(""));
    let labels: Vec<String> = menu
        .items()
        .into_iter()
        .map(|item| match item {
            // The title screen says which ruleset Play will start, so the mode is
            // never a surprise once the first piece is already falling.
            TitleItem::Play => format!("Play — {}", mode.label()),
            other => other.label().to_string(),
        })
        .collect();
    lines.extend(menu_column(&labels, menu.selected));
    lines.push(Line::from(""));
    lines.push(dim("↑↓ choose · enter select · q quit"));

    let height = lines.len() as u16;
    let width = lines
        .iter()
        .map(|line| line.width() as u16)
        .max()
        .unwrap_or(0);
    let rect = centered(area, width, height);
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), rect);
}

fn option_value(row: OptionRow, config: &Config, keymap: &Keymap) -> String {
    match row {
        OptionRow::Mode => config.mode.label().to_string(),
        OptionRow::StartLevel => config.start_level(config.mode).to_string(),
        OptionRow::Das => format!("{} frames", config.das_frames),
        OptionRow::Arr => format!("{} frames", config.arr_frames),
        OptionRow::Ghost => if config.ghost { "on" } else { "off" }.to_string(),
        OptionRow::Theme => config.theme.label().to_string(),
        OptionRow::Skin => config.skin.label().to_string(),
        OptionRow::Border => config.border.label().to_string(),
        OptionRow::Bind(action) => {
            let mut keys: Vec<String> = keymap
                .keys_for(action)
                .into_iter()
                .map(display_name)
                .collect();
            keys.sort();
            if keys.is_empty() {
                "unbound".to_string()
            } else {
                keys.join(" / ")
            }
        }
    }
}

fn option_label(row: OptionRow) -> String {
    match row {
        OptionRow::Mode => "Game mode".into(),
        OptionRow::StartLevel => "Starting level".into(),
        OptionRow::Das => "DAS".into(),
        OptionRow::Arr => "ARR".into(),
        OptionRow::Ghost => "Ghost piece".into(),
        OptionRow::Theme => "Colour theme".into(),
        OptionRow::Skin => "Tetromino skin".into(),
        OptionRow::Border => "Board border".into(),
        OptionRow::Bind(action) => action.label().to_string(),
    }
}

pub fn render_options(frame: &mut Frame, area: Rect, menu: &OptionsMenu, config: &Config) {
    let rows = OptionsMenu::rows(config);
    let keymap = config.keymap();

    let block = config.border.apply(
        Block::default()
            .title(" OPTIONS ")
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    let width = 44.min(area.width);
    let height = (rows.len() as u16 + 5).min(area.height);
    let rect = centered(area, width, height);
    let interior = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);

    if interior.height == 0 {
        return;
    }

    // Two lines at the bottom are reserved for the hint and any notice.
    let list_height = interior.height.saturating_sub(2).max(1) as usize;
    let (start, end) = window(rows.len(), menu.selected, list_height);

    let mut lines: Vec<Line> = Vec::new();
    for (index, row) in rows[start..end].iter().enumerate() {
        let index = start + index;
        let selected = index == menu.selected;
        let label = option_label(*row);
        let awaiting_key = menu.rebinding.is_some() && menu.rebinding == action_of(*row);
        let value = if awaiting_key {
            "press a key…".to_string()
        } else {
            option_value(*row, config, &keymap)
        };

        // Right-align the value against the panel edge so the column reads.
        let pad = (interior.width as usize)
            .saturating_sub(2 + label.chars().count() + value.chars().count());
        let text = format!("{label}{}{value}", " ".repeat(pad));
        lines.push(menu_line(&text, selected));
    }

    while lines.len() < list_height {
        lines.push(Line::from(""));
    }

    if let Some(notice) = &menu.notice {
        lines.push(Line::from(Span::styled(
            notice.clone(),
            Style::default().fg(Color::Yellow),
        )));
    } else if menu.rebinding.is_some() {
        lines.push(dim("press a key to bind, esc to cancel"));
    } else {
        lines.push(dim("←→ change · enter rebind · esc back"));
    }
    lines.push(dim("changes are saved as you make them"));

    // Deliberately unwrapped: the rows are padded to the panel width, and wrapping
    // would trim that padding away and break the value column.
    frame.render_widget(Paragraph::new(lines), interior);
}

/// The action a row rebinds, if it is a rebind row.
fn action_of(row: OptionRow) -> Option<crate::input::action::Action> {
    match row {
        OptionRow::Bind(action) => Some(action),
        _ => None,
    }
}

pub fn render_scores(
    frame: &mut Frame,
    area: Rect,
    view: &ScoresView,
    scores: &Scores,
    border: BorderStyle,
) {
    let table = scores.table(view.mode);

    let block = border.apply(
        Block::default()
            .title(format!(" HIGH SCORES — {} ", view.mode.label()))
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    let mut lines: Vec<Line> = Vec::new();
    if table.is_empty() {
        lines.push(dim("no scores yet"));
    } else {
        lines.push(dim(format!(
            "{:<3}{:<11}{:>9}{:>6}{:>4}  {}",
            "#", "NAME", "SCORE", "LINES", "LV", "DATE"
        )));
        for (index, entry) in table.iter().enumerate() {
            lines.push(Line::from(format!(
                "{:<3}{:<11}{:>9}{:>6}{:>4}  {}",
                index + 1,
                truncate(&entry.name, 10),
                entry.score,
                entry.lines,
                entry.level,
                entry.date
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(dim("←→ other mode · esc back"));

    // Wide enough for the full row: rank, a 10-character name, score, lines,
    // level and the date, which is the widest thing on the screen. The box is
    // sized to the table so a short list does not sit in a mostly empty frame.
    let rect = centered(area, 48.min(area.width), lines.len() as u16 + 2);
    let interior = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(lines), interior);
}

fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

pub fn render_pause(frame: &mut Frame, area: Rect, menu: &PauseMenu, border: BorderStyle) {
    let mut lines = vec![Line::from(Span::styled(
        "PAUSED",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ))];
    let labels: Vec<String> = PauseItem::ALL
        .into_iter()
        .map(|item| item.label().to_string())
        .collect();
    lines.extend(menu_column(&labels, menu.selected));

    overlay(frame, area, lines, 21, border);
}

pub fn render_game_over(
    frame: &mut Frame,
    area: Rect,
    menu: &GameOverMenu,
    score: u64,
    lines_cleared: u32,
    level: u32,
    border: BorderStyle,
) {
    let mut lines = vec![
        Line::from(Span::styled(
            "GAME OVER",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("score {score}")),
        Line::from(format!("lines {lines_cleared}   level {level}")),
        Line::from(""),
    ];

    if menu.entering {
        lines.push(Line::from(Span::styled(
            "NEW HIGH SCORE",
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(vec![
            Span::styled("name ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}_", menu.name),
                Style::default().fg(Color::White).bg(Color::Black),
            ),
        ]));
        lines.push(dim("enter to confirm"));
    } else {
        if let Some(rank) = menu.rank {
            lines.push(Line::from(Span::styled(
                format!("ranked #{}", rank + 1),
                Style::default().fg(Color::Yellow),
            )));
        }
        let labels: Vec<String> = GameOverItem::ALL
            .into_iter()
            .map(|item| item.label().to_string())
            .collect();
        lines.extend(menu_column(&labels, menu.selected));
    }

    overlay(frame, area, lines, 24, border);
}

/// A bordered box drawn over the board, sized to its contents.
fn overlay(
    frame: &mut Frame,
    area: Rect,
    lines: Vec<Line<'static>>,
    min_width: u16,
    border: BorderStyle,
) {
    let content_width = lines
        .iter()
        .map(|line| line.width() as u16)
        .max()
        .unwrap_or(0);
    let width = (content_width + 4).max(min_width);
    let height = lines.len() as u16 + 2;
    let rect = centered(area, width, height);

    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(border.apply(Block::default())),
        rect,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn draw(width: u16, height: u16, f: impl FnOnce(&mut Frame)) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(f).unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_window_keeps_the_cursor_visible_in_a_long_list() {
        assert_eq!(window(5, 0, 10), (0, 5), "short lists are not scrolled");
        assert_eq!(window(20, 0, 5), (0, 5), "pinned at the top");
        assert_eq!(window(20, 19, 5), (15, 20), "pinned at the bottom");

        let (start, end) = window(20, 10, 5);
        assert!(start <= 10 && 10 < end, "cursor inside {start}..{end}");
        assert_eq!(end - start, 5);
    }

    #[test]
    fn the_window_survives_degenerate_sizes() {
        assert_eq!(window(0, 0, 5), (0, 0));
        assert_eq!(window(5, 0, 0), (0, 0));
    }

    #[test]
    fn centering_clamps_to_a_terminal_smaller_than_the_content() {
        let area = Rect::new(0, 0, 10, 4);
        let rect = centered(area, 80, 40);
        assert_eq!((rect.width, rect.height), (10, 4));
        assert!(rect.right() <= area.right() && rect.bottom() <= area.bottom());
    }

    #[test]
    fn the_title_screen_shows_the_logo_and_the_mode_to_be_played() {
        let menu = TitleMenu::default();
        let rendered = draw(80, 24, |frame| {
            render_title(frame, frame.area(), &menu, Mode::Modern)
        });
        println!("{rendered}");
        assert!(rendered.contains("Play"));
        assert!(rendered.contains("MODERN"));
        assert!(rendered.contains("High scores"));
        assert!(rendered.contains('█'), "logo should be drawn at this size");
    }

    /// The logo is 45 columns; a narrow terminal must get the plain title instead
    /// of a wrapped mess.
    #[test]
    fn a_narrow_title_screen_falls_back_to_plain_text() {
        let menu = TitleMenu::default();
        let rendered = draw(24, 22, |frame| {
            render_title(frame, frame.area(), &menu, Mode::Nes)
        });
        println!("{rendered}");
        assert!(rendered.contains("T E T R I S"));
        assert!(!rendered.contains('█'));
        assert!(rendered.contains("Play"));
    }

    #[test]
    fn the_options_screen_lists_values_and_bindings() {
        let config = Config::default();
        let menu = OptionsMenu::default();
        let rendered = draw(80, 30, |frame| {
            render_options(frame, frame.area(), &menu, &config)
        });
        println!("{rendered}");
        assert!(rendered.contains("OPTIONS"));
        assert!(rendered.contains("Game mode"));
        assert!(rendered.contains("NES"));
        assert!(rendered.contains("Starting level"));
        assert!(rendered.contains("Rotate CW"));
    }

    #[test]
    fn a_rebind_in_progress_is_shown_on_the_row() {
        let config = Config::default();
        let rows = OptionsMenu::rows(&config);
        let menu = OptionsMenu {
            selected: rows
                .iter()
                .position(|r| matches!(r, OptionRow::Bind(_)))
                .unwrap(),
            rebinding: Some(crate::input::action::Action::MoveLeft),
            notice: None,
        };
        let rendered = draw(80, 30, |frame| {
            render_options(frame, frame.area(), &menu, &config)
        });
        println!("{rendered}");
        assert!(rendered.contains("press a key"));
    }

    #[test]
    fn the_options_screen_lists_the_visual_axes() {
        let config = Config::default();
        let menu = OptionsMenu::default();
        let rendered = draw(80, 34, |frame| {
            render_options(frame, frame.area(), &menu, &config)
        });
        println!("{rendered}");
        for label in ["Colour theme", "Tetromino skin", "Board border"] {
            assert!(rendered.contains(label), "{label} missing");
        }
        assert!(rendered.contains("Guideline"));
        assert!(rendered.contains("Solid"));
    }

    /// The ASCII border exists for terminals that cannot draw box characters, so
    /// picking it must change the chrome everywhere, menus included.
    #[test]
    fn the_border_style_reaches_the_menu_chrome() {
        let config = Config {
            border: BorderStyle::Ascii,
            ..Default::default()
        };
        let rendered = draw(80, 34, |frame| {
            render_options(frame, frame.area(), &OptionsMenu::default(), &config)
        });
        println!("{rendered}");
        assert!(rendered.contains('+'), "ASCII corners expected");
        assert!(!rendered.contains('┌'), "no box drawing at this setting");

        let plain = draw(40, 20, |frame| {
            render_pause(
                frame,
                frame.area(),
                &PauseMenu::default(),
                BorderStyle::None,
            )
        });
        assert!(!plain.contains('┌'), "the None border draws no frame");
        assert!(plain.contains("PAUSED"), "contents are still drawn");
    }

    /// The keybind list is longer than a short terminal, so the panel scrolls
    /// rather than leaving the cursor somewhere off screen.
    #[test]
    fn a_short_options_screen_scrolls_to_the_cursor() {
        let config = Config::default();
        let rows = OptionsMenu::rows(&config);
        let menu = OptionsMenu {
            selected: rows.len() - 1, // Quit, the last rebind row
            ..Default::default()
        };
        let rendered = draw(60, 12, |frame| {
            render_options(frame, frame.area(), &menu, &config)
        });
        println!("{rendered}");
        assert!(rendered.contains("Quit"), "the selected row is visible");
        assert!(
            !rendered.contains("Game mode"),
            "the top of the list has scrolled away"
        );
    }

    #[test]
    fn an_empty_score_table_says_so_rather_than_drawing_nothing() {
        let scores = Scores::default();
        let view = ScoresView::new(Mode::Nes);
        let rendered = draw(60, 20, |frame| {
            render_scores(frame, frame.area(), &view, &scores, BorderStyle::default())
        });
        println!("{rendered}");
        assert!(rendered.contains("HIGH SCORES"));
        assert!(rendered.contains("NES"));
        assert!(rendered.contains("no scores yet"));
    }

    #[test]
    fn score_rows_are_rendered_with_their_details() {
        let mut scores = Scores::default();
        scores.insert(
            Mode::Modern,
            crate::scores::Entry {
                name: "niall".into(),
                score: 12345,
                lines: 42,
                level: 5,
                date: "2026-09-22".into(),
            },
        );
        let view = ScoresView::new(Mode::Modern);
        let rendered = draw(60, 20, |frame| {
            render_scores(frame, frame.area(), &view, &scores, BorderStyle::default())
        });
        println!("{rendered}");
        assert!(rendered.contains("niall"));
        assert!(rendered.contains("12345"));
        assert!(rendered.contains("2026-09-22"));
    }

    #[test]
    fn the_game_over_overlay_asks_for_a_name_when_the_run_placed() {
        let menu = GameOverMenu::new(true, "player");
        let rendered = draw(40, 20, |frame| {
            render_game_over(
                frame,
                frame.area(),
                &menu,
                999,
                12,
                3,
                BorderStyle::default(),
            )
        });
        println!("{rendered}");
        assert!(rendered.contains("GAME OVER"));
        assert!(rendered.contains("NEW HIGH SCORE"));
        assert!(rendered.contains("player"));
    }

    #[test]
    fn the_game_over_overlay_offers_retry_once_the_name_is_in() {
        let mut menu = GameOverMenu::new(true, "player");
        menu.submit();
        menu.rank = Some(2);
        let rendered = draw(40, 20, |frame| {
            render_game_over(
                frame,
                frame.area(),
                &menu,
                999,
                12,
                3,
                BorderStyle::default(),
            )
        });
        println!("{rendered}");
        assert!(rendered.contains("ranked #3"));
        assert!(rendered.contains("Retry"));
        assert!(rendered.contains("Back to title"));
    }

    #[test]
    fn the_pause_overlay_lists_its_choices() {
        let menu = PauseMenu::default();
        let rendered = draw(40, 20, |frame| {
            render_pause(frame, frame.area(), &menu, BorderStyle::default())
        });
        println!("{rendered}");
        assert!(rendered.contains("PAUSED"));
        assert!(rendered.contains("Resume"));
        assert!(rendered.contains("Quit to title"));
    }

    /// Every screen has to survive the smallest terminal the game runs in.
    #[test]
    fn no_screen_panics_at_the_minimum_terminal_size() {
        let config = Config::default();
        let scores = Scores::default();
        for (width, height) in [(22u16, 22u16), (24, 10), (1, 1)] {
            draw(width, height, |frame| {
                render_title(frame, frame.area(), &TitleMenu::default(), Mode::Nes)
            });
            draw(width, height, |frame| {
                render_options(frame, frame.area(), &OptionsMenu::default(), &config)
            });
            draw(width, height, |frame| {
                render_scores(
                    frame,
                    frame.area(),
                    &ScoresView::new(Mode::Nes),
                    &scores,
                    BorderStyle::default(),
                )
            });
            draw(width, height, |frame| {
                render_pause(
                    frame,
                    frame.area(),
                    &PauseMenu::default(),
                    BorderStyle::default(),
                )
            });
            draw(width, height, |frame| {
                render_game_over(
                    frame,
                    frame.area(),
                    &GameOverMenu::new(true, "x"),
                    1,
                    1,
                    1,
                    BorderStyle::default(),
                )
            });
        }
    }
}
