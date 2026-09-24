//! The background layer: what is drawn behind the playfield and on the title
//! screen's attract mode.
//!
//! Every background implements one trait and is handed the same
//! [`PerformanceSignal`] each frame. The purely cosmetic ones ignore it; the
//! reactive one (§8.1) reads it. That keeps a single trait object in the app —
//! no downcasting, no second trait for the one background that cares.
//!
//! Backgrounds never draw inside the playfield. The board paints a dot grid over
//! its own empty cells, so anything drawn under it would be hidden anyway, and a
//! background bleeding into the field would make the stack harder to read — the
//! one thing a background must not do. [`Canvas`] enforces that rather than
//! trusting each background to remember.

pub mod aquarium;
pub mod bonsai;
pub mod cow;
pub mod locomotive;
pub mod logo;
pub mod matrix;
pub mod nyancat;
pub mod pipes;
pub mod scenes;

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::style::{Color, Style};
use serde::{Deserialize, Serialize};

use crate::engine::piece::PieceKind;
use crate::game::Game;
use crate::ui::style::Visuals;

/// Rainbow bands, top to bottom, as the pieces whose colours they borrow: the
/// piece colours happen to run red to purple in exactly rainbow order, so a
/// rainbow follows the theme.
pub const RAINBOW: [PieceKind; 6] = [
    PieceKind::Z,
    PieceKind::L,
    PieceKind::O,
    PieceKind::S,
    PieceKind::J,
    PieceKind::T,
];

/// The colour of `row` in a rainbow running down through a piece of art,
/// `frames` into the animation, each band moving down a row every
/// `frames_per_row` frames. Each row takes the band above it a step later, so
/// the rainbow runs downward.
pub fn running_rainbow(row: usize, frames: u32, frames_per_row: u32, visuals: &Visuals) -> Color {
    let bands = RAINBOW.len();
    let step = (frames / frames_per_row.max(1)) as usize % bands;
    visuals.theme.color(RAINBOW[(row + bands - step) % bands])
}

/// Turn a piece of ASCII art round to face the other way: each line padded to
/// the art's width so the rows stay aligned, reversed, and the characters that
/// have a facing swapped for their partners. Trailing blanks are dropped, since
/// drawing skips spaces anyway.
pub fn mirror<S: AsRef<str>>(art: &[S]) -> Vec<String> {
    let width = art
        .iter()
        .map(|line| line.as_ref().chars().count())
        .max()
        .unwrap_or(0);
    art.iter()
        .map(|line| {
            let reversed: String = format!("{:<width$}", line.as_ref())
                .chars()
                .rev()
                .map(|ch| match ch {
                    '/' => '\\',
                    '\\' => '/',
                    '(' => ')',
                    ')' => '(',
                    '[' => ']',
                    ']' => '[',
                    '{' => '}',
                    '}' => '{',
                    '<' => '>',
                    '>' => '<',
                    other => other,
                })
                .collect();
            reversed.trim_end().to_string()
        })
        .collect()
}

/// What the game is currently doing, for backgrounds that react to it.
///
/// This is a snapshot, rebuilt each frame: nothing here is owned by a background,
/// so several could read it without coordinating.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PerformanceSignal {
    pub combo: u32,
    pub back_to_back: bool,
    /// Lines cleared by the most recent placement.
    pub last_clear: ClearKind,
    /// Whether the most recent placement was a T-spin, full or mini.
    pub last_tspin: bool,
    /// Pieces placed so far this run. The two fields above describe only the
    /// latest placement, so this is what tells a reactive background that a new
    /// one happened — two Tetrises in a row look identical otherwise.
    pub placements: u64,
    /// 0.0 empty, 1.0 stacked to the top.
    pub stack_height: f32,
    pub game_over: bool,
    /// Four rows are on the board mid-clear: a Tetris, while its line-clear
    /// delay runs. Always true for a moment in NES, which has a delay built
    /// in; in modern only when the player has set one.
    pub tetris_clearing: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClearKind {
    #[default]
    None,
    Single,
    Double,
    Triple,
    Tetris,
}

impl ClearKind {
    pub fn from_lines(lines: u32) -> Self {
        match lines {
            0 => ClearKind::None,
            1 => ClearKind::Single,
            2 => ClearKind::Double,
            3 => ClearKind::Triple,
            _ => ClearKind::Tetris,
        }
    }
}

/// What the run's placements have done so far, kept by the app as they happen:
/// the engines report a placement once, on the tick it locks, but a background
/// reads the signal on every tick after.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlacementHistory {
    pub last_clear: ClearKind,
    pub last_tspin: bool,
    pub placements: u64,
}

impl PlacementHistory {
    pub fn record(&mut self, lines: u32, tspin: bool) {
        self.last_clear = ClearKind::from_lines(lines);
        self.last_tspin = tspin;
        self.placements += 1;
    }
}

impl PerformanceSignal {
    /// The signal for a run in progress.
    pub fn of(game: &Game, history: &PlacementHistory) -> Self {
        Self {
            combo: game.combo().unwrap_or(0),
            back_to_back: game.back_to_back(),
            last_clear: history.last_clear,
            last_tspin: history.last_tspin,
            placements: history.placements,
            stack_height: game.board().stack_height_fraction(),
            game_over: game.is_over(),
            tetris_clearing: game.clearing_rows().len() == 4,
        }
    }
}

pub trait Background {
    /// Advance any animation. Called once per 60Hz tick, on every screen, so the
    /// title screen's attract mode animates at the same rate as gameplay.
    ///
    /// `size` is the terminal's current size. Animations that need bounds — rain
    /// columns, pipes that wrap at the edge — size themselves from it here rather
    /// than assuming one (§5), and must cope with it changing between any two
    /// ticks.
    ///
    /// `signal` is here as well as in `render` because a reactive background
    /// has to notice changes as they happen and hold its reaction for a while,
    /// which is state, and `render` cannot change state.
    fn tick(&mut self, dt: Duration, size: Size, signal: &PerformanceSignal);

    /// Draw. `canvas` already refuses writes inside the playfield.
    fn render(&self, canvas: &mut Canvas, visuals: &Visuals, signal: &PerformanceSignal);

    fn name(&self) -> &'static str;
}

/// A clipped drawing surface: writes outside the area, or inside the playfield,
/// are dropped rather than being the caller's problem.
pub struct Canvas<'a> {
    buf: &'a mut Buffer,
    area: Rect,
    /// Regions the background must keep out of: the playfield and the HUD panels
    /// around it. Empty when no board is on screen, as on the title screen, and
    /// the whole area is then writable.
    ///
    /// The panels are in here as well as the board because ratatui widgets paint
    /// only the cells they actually write, so a background drawn underneath one
    /// would show through its blank space and make the HUD unreadable.
    reserved: &'a [Rect],
}

impl<'a> Canvas<'a> {
    pub fn new(buf: &'a mut Buffer, area: Rect, reserved: &'a [Rect]) -> Self {
        Self {
            buf,
            area,
            reserved,
        }
    }

    pub fn width(&self) -> u16 {
        self.area.width
    }

    pub fn height(&self) -> u16 {
        self.area.height
    }

    /// Whether a point is somewhere this canvas will actually draw.
    pub fn accepts(&self, x: u16, y: u16) -> bool {
        if x >= self.area.width || y >= self.area.height {
            return false;
        }
        let (ax, ay) = (self.area.x + x, self.area.y + y);
        !self.reserved.iter().any(|rect| {
            rect.width > 0
                && rect.height > 0
                && ax >= rect.x
                && ax < rect.right()
                && ay >= rect.y
                && ay < rect.bottom()
        })
    }

    /// Draw one glyph at a canvas-relative position. Positions may be negative:
    /// art sliding in from an edge is drawn from wherever it starts, and the
    /// part off the canvas is dropped.
    pub fn set(&mut self, x: impl Into<i32>, y: impl Into<i32>, symbol: &str, style: Style) {
        let (Ok(x), Ok(y)) = (u16::try_from(x.into()), u16::try_from(y.into())) else {
            return;
        };
        if !self.accepts(x, y) {
            return;
        }
        self.buf[(self.area.x + x, self.area.y + y)]
            .set_symbol(symbol)
            .set_style(style);
    }

    /// Draw one character, clipped like [`Canvas::set`].
    pub fn put(&mut self, x: impl Into<i32>, y: impl Into<i32>, ch: char, style: Style) {
        let mut buffer = [0u8; 4];
        self.set(x, y, ch.encode_utf8(&mut buffer), style);
    }

    /// Draw a line of text, one glyph per column, clipped like [`Canvas::set`].
    /// Spaces are skipped rather than painted, so art stays transparent where
    /// it is blank and the terminal's own background shows through (§5).
    pub fn text(&mut self, x: impl Into<i32>, y: impl Into<i32>, text: &str, style: Style) {
        let y = y.into();
        for (column, ch) in (x.into()..).zip(text.chars()) {
            if ch != ' ' {
                self.put(column, y, ch, style);
            }
        }
    }

    /// Every run of columns that no reserved region touches anywhere, left to
    /// right, as `(first column, width)`: where art can stand beside the board
    /// without disappearing behind it. With a board on screen the first and
    /// last are the margins either side of it; with nothing reserved there is
    /// one, the whole width.
    pub fn free_spans(&self) -> Vec<(u16, u16)> {
        let covered = |x: u16| {
            let ax = self.area.x + x;
            self.reserved.iter().any(|rect| {
                rect.width > 0
                    && rect.height > 0
                    && ax >= rect.x
                    && ax < rect.right()
                    && rect.y < self.area.bottom()
                    && rect.bottom() > self.area.y
            })
        };

        let (mut spans, mut run_start) = (Vec::new(), None);
        for x in 0..=self.area.width {
            let free = x < self.area.width && !covered(x);
            match (free, run_start) {
                (true, None) => run_start = Some(x),
                (false, Some(start)) => {
                    spans.push((start, x - start));
                    run_start = None;
                }
                _ => {}
            }
        }
        spans
    }

    /// Draw a block of lines with its top-left at `(x, y)`, clipped like
    /// [`Canvas::set`].
    pub fn block<S: AsRef<str>>(
        &mut self,
        x: impl Into<i32>,
        y: impl Into<i32>,
        lines: &[S],
        style: Style,
    ) {
        let x = x.into();
        for (row, line) in (y.into()..).zip(lines) {
            self.text(x, row, line.as_ref(), style);
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundKind {
    /// The terminal, untouched — which is also what keeps transparency intact.
    #[default]
    Blank,
    Scene,
    DistroLogo,
    MatrixRain,
    Pipes,
    Nyancat,
    Bonsai,
    Aquarium,
    /// The reactive one (§8.1).
    Cowsay,
    Locomotive,
}

impl BackgroundKind {
    /// Still ones first, then the animations.
    pub const ALL: [BackgroundKind; 10] = [
        BackgroundKind::Blank,
        BackgroundKind::Scene,
        BackgroundKind::DistroLogo,
        BackgroundKind::MatrixRain,
        BackgroundKind::Pipes,
        BackgroundKind::Nyancat,
        BackgroundKind::Bonsai,
        BackgroundKind::Aquarium,
        BackgroundKind::Cowsay,
        BackgroundKind::Locomotive,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BackgroundKind::Blank => "Blank",
            BackgroundKind::Scene => "Scene",
            BackgroundKind::DistroLogo => "Distro logo",
            BackgroundKind::MatrixRain => "Matrix rain",
            BackgroundKind::Pipes => "Pipes",
            BackgroundKind::Nyancat => "Nyancat",
            BackgroundKind::Bonsai => "Bonsai",
            BackgroundKind::Aquarium => "Aquarium",
            BackgroundKind::Cowsay => "Cowsay",
            BackgroundKind::Locomotive => "Locomotive",
        }
    }

    /// The one-line explanation shown beside the options row (§9).
    pub fn description(self) -> &'static str {
        match self {
            BackgroundKind::Blank => "nothing drawn; keeps terminal transparency",
            BackgroundKind::Scene => "a still scene behind the field",
            BackgroundKind::DistroLogo => "your distribution's logo, scattered",
            BackgroundKind::MatrixRain => "falling glyph columns, cmatrix-style",
            BackgroundKind::Pipes => "pipes laid across the screen, pipes.sh-style",
            BackgroundKind::Nyancat => "a poptart cat, now and then",
            BackgroundKind::Bonsai => "bonsai trees growing beside the field",
            BackgroundKind::Aquarium => "fish, bubbles and seaweed, asciiquarium-style",
            BackgroundKind::Cowsay => "two cows with opinions on your play",
            BackgroundKind::Locomotive => "sl's steam train, back and forth",
        }
    }

    pub fn create(self, scene: scenes::SceneChoice) -> Box<dyn Background> {
        match self {
            BackgroundKind::Blank => Box::new(Blank),
            BackgroundKind::Scene => Box::new(scenes::SceneBackground::new(scene)),
            BackgroundKind::DistroLogo => Box::new(logo::LogoBackground::detect()),
            BackgroundKind::MatrixRain => Box::new(matrix::MatrixRain::new()),
            BackgroundKind::Pipes => Box::new(pipes::Pipes::new()),
            BackgroundKind::Nyancat => Box::new(nyancat::Nyancat::new()),
            BackgroundKind::Bonsai => Box::new(bonsai::Bonsai::new()),
            BackgroundKind::Aquarium => Box::new(aquarium::Aquarium::new()),
            BackgroundKind::Cowsay => Box::new(cow::Cow::new()),
            BackgroundKind::Locomotive => Box::new(locomotive::Locomotive::new()),
        }
    }
}

/// Draws nothing at all, deliberately: §5's transparency rule means the default
/// background has to leave the terminal's own alone.
#[derive(Debug, Default)]
pub struct Blank;

impl Background for Blank {
    fn tick(&mut self, _dt: Duration, _size: Size, _signal: &PerformanceSignal) {}

    fn render(&self, _canvas: &mut Canvas, _visuals: &Visuals, _signal: &PerformanceSignal) {}

    fn name(&self) -> &'static str {
        "Blank"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn canvas_buffer() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 20, 10))
    }

    fn rendered(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_blank_background_writes_nothing() {
        let mut buf = canvas_buffer();
        let before = rendered(&buf);
        let mut canvas = Canvas::new(&mut buf, Rect::new(0, 0, 20, 10), &[]);
        Blank.render(
            &mut canvas,
            &Visuals::default(),
            &PerformanceSignal::default(),
        );
        assert_eq!(rendered(&buf), before);
    }

    /// The playfield is the one thing a background must never draw over.
    #[test]
    fn writes_inside_the_playfield_are_dropped() {
        let mut buf = canvas_buffer();
        let board = Rect::new(5, 2, 6, 4);
        let reserved = [board];
        let mut canvas = Canvas::new(&mut buf, Rect::new(0, 0, 20, 10), &reserved);

        for y in 0..10 {
            for x in 0..20 {
                canvas.set(x, y, "#", Style::default());
            }
        }

        for y in 0..10u16 {
            for x in 0..20u16 {
                let inside = (5..11).contains(&x) && (2..6).contains(&y);
                let symbol = buf[(x, y)].symbol();
                if inside {
                    assert_ne!(symbol, "#", "({x}, {y}) is inside the board");
                } else {
                    assert_eq!(symbol, "#", "({x}, {y}) should have been drawn");
                }
            }
        }
    }

    #[test]
    fn an_empty_board_rect_leaves_the_whole_canvas_writable() {
        let mut buf = canvas_buffer();
        let mut canvas = Canvas::new(&mut buf, Rect::new(0, 0, 20, 10), &[]);
        assert!(canvas.accepts(0, 0));
        canvas.set(0, 0, "#", Style::default());
        assert_eq!(buf[(0, 0)].symbol(), "#");
    }

    #[test]
    fn drawing_past_the_edge_is_clipped_rather_than_panicking() {
        let mut buf = canvas_buffer();
        let area = Rect::new(0, 0, 20, 10);
        let mut canvas = Canvas::new(&mut buf, area, &[]);

        canvas.set(100, 100, "#", Style::default());
        canvas.text(18, 0, "overlong text", Style::default());
        canvas.block(0, 8, &["one", "two", "three", "four"], Style::default());

        assert_eq!(buf[(18, 0)].symbol(), "o");
        assert_eq!(buf[(19, 0)].symbol(), "v");
    }

    /// Blank space in art has to stay blank, or every scene would punch a hole in
    /// a transparent terminal.
    #[test]
    fn spaces_in_art_are_not_painted() {
        let mut buf = canvas_buffer();
        let mut canvas = Canvas::new(&mut buf, Rect::new(0, 0, 20, 10), &[]);
        canvas.set(1, 0, "X", Style::default());
        canvas.text(0, 0, "a b", Style::default().fg(Color::Red));

        assert_eq!(buf[(0, 0)].symbol(), "a");
        assert_eq!(
            buf[(1, 0)].symbol(),
            "X",
            "the space did not overwrite this"
        );
        assert_eq!(buf[(2, 0)].symbol(), "b");
    }

    #[test]
    fn a_canvas_relative_origin_is_offset_by_the_area() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 10));
        let area = Rect::new(4, 3, 10, 5);
        let mut canvas = Canvas::new(&mut buf, area, &[]);
        canvas.set(0, 0, "#", Style::default());
        assert_eq!(buf[(4, 3)].symbol(), "#");
    }

    #[test]
    fn free_spans_are_listed_left_to_right() {
        let area = Rect::new(0, 0, 20, 10);
        let mut buf = canvas_buffer();
        let reserved = [Rect::new(4, 0, 6, 10), Rect::new(12, 0, 2, 10)];
        let canvas = Canvas::new(&mut buf, area, &reserved);
        assert_eq!(canvas.free_spans(), [(0, 4), (10, 2), (14, 6)]);

        // A region covering only some rows still takes its columns.
        let short = [Rect::new(4, 2, 6, 3)];
        let canvas = Canvas::new(&mut buf, area, &short);
        assert_eq!(canvas.free_spans(), [(0, 4), (10, 10)]);

        let canvas = Canvas::new(&mut buf, area, &[]);
        assert_eq!(canvas.free_spans(), [(0, 20)], "all of it, unreserved");

        let everything = [area];
        let canvas = Canvas::new(&mut buf, area, &everything);
        assert!(canvas.free_spans().is_empty());
    }

    /// Every background, animated or not, across the sizes a terminal might be
    /// and resizes between them: no panics, nothing inside the reserved regions,
    /// and never a painted cell background, which would punch through terminal
    /// transparency (§5).
    #[test]
    fn every_background_is_transparent_and_keeps_to_its_canvas() {
        const TICK: Duration = Duration::from_nanos(16_666_667);

        for kind in BackgroundKind::ALL {
            let mut background = kind.create(scenes::SceneChoice::default());
            for (width, height) in [(120, 40), (80, 24), (30, 10), (1, 1), (0, 0), (90, 30)] {
                let size = Size::new(width, height);
                for _ in 0..300 {
                    background.tick(TICK, size, &PerformanceSignal::default());
                }

                let area = Rect::new(0, 0, width, height);
                let board = Rect::new(width / 3, 0, width / 3, height);
                let reserved = [board];
                let mut buf = Buffer::empty(area);
                let mut canvas = Canvas::new(&mut buf, area, &reserved);
                background.render(
                    &mut canvas,
                    &Visuals::default(),
                    &PerformanceSignal::default(),
                );

                for y in 0..height {
                    for x in 0..width {
                        let cell = &buf[(x, y)];
                        assert_eq!(cell.bg, Color::Reset, "{kind:?} painted ({x}, {y})");
                        if board.contains((x, y).into()) {
                            assert_eq!(cell.symbol(), " ", "{kind:?} drew on the board");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_kind_has_a_label_a_description_and_builds() {
        for kind in BackgroundKind::ALL {
            assert!(!kind.label().is_empty());
            assert!(!kind.description().is_empty());
            let background = kind.create(scenes::SceneChoice::default());
            assert!(!background.name().is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn clear_kinds_follow_the_line_count() {
        assert_eq!(ClearKind::from_lines(0), ClearKind::None);
        assert_eq!(ClearKind::from_lines(1), ClearKind::Single);
        assert_eq!(ClearKind::from_lines(4), ClearKind::Tetris);
        assert_eq!(ClearKind::from_lines(9), ClearKind::Tetris, "clamped");
    }

    #[test]
    fn mirrored_art_faces_the_other_way() {
        assert_eq!(mirror(&["><>"]), ["<><"]);
        assert_eq!(mirror(&["><(((('>"]), ["<'))))><"]);
        // Padded before reversing, so short rows stay in their column.
        assert_eq!(mirror(&[" __", "\\/ o\\"]), ["  __", "/o \\/"]);
    }

    /// Art sliding in from an edge is drawn from wherever it starts, with the
    /// part off the canvas simply dropped.
    #[test]
    fn text_off_the_top_or_left_edge_is_clipped_not_lost() {
        let area = Rect::new(0, 0, 20, 10);
        let mut buf = canvas_buffer();
        let mut canvas = Canvas::new(&mut buf, area, &[]);
        canvas.text(-2, 0, "abcd", Style::default());
        canvas.text(0, -1, "hidden", Style::default());
        assert_eq!(
            rendered(&buf).lines().next().unwrap(),
            format!("cd{}", " ".repeat(18))
        );
    }

    #[test]
    fn the_signal_reflects_the_game_it_came_from() {
        use crate::game::{Game, Mode};

        let game = Game::new(Mode::Modern, 3);
        let mut history = PlacementHistory::default();
        history.record(4, false);
        history.record(2, true);
        let signal = PerformanceSignal::of(&game, &history);
        assert_eq!(
            signal.last_clear,
            ClearKind::Double,
            "only the latest counts"
        );
        assert!(signal.last_tspin);
        assert_eq!(signal.placements, 2);
        assert!(!signal.game_over);
        assert_eq!(signal.stack_height, 0.0, "a fresh board is empty");
    }
}
