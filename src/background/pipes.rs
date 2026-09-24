//! Pipes (§8, #7): pipes.sh's cursors laying box-drawing pipe across the screen.
//!
//! A couple of pipe heads wander the screen a cell at a time, now and then
//! turning a corner, wrapping at the edges, and leaving their pipe behind. Once
//! the screen has taken a fair amount of pipe it is wiped and they start again,
//! which is exactly what pipes.sh does.
//!
//! The pipe is drawn in the same line style as the board's border, and coloured
//! from the current theme's piece palette, so it belongs to the rest of the
//! screen rather than fighting it.

use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use ratatui::layout::Size;
use ratatui::style::{Modifier, Style};

use super::{Background, Canvas, PerformanceSignal};
use crate::engine::piece::PieceKind;
use crate::ui::style::{BorderStyle, Visuals};

/// Pipe heads at once.
const HEADS: usize = 2;
/// Cells each head lays per second: slow enough to watch, calm enough to
/// ignore.
const STEPS_PER_SECOND: f32 = 24.0;
/// Chance per step of turning. pipes.sh's default "steadiness" is about this.
const TURN_CHANCE: f64 = 0.13;
/// Pipe laid, as a fraction of the screen's cells, before it is wiped.
const FILL_LIMIT: f32 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Up,
    Right,
    Down,
    Left,
}

impl Dir {
    const ALL: [Dir; 4] = [Dir::Up, Dir::Right, Dir::Down, Dir::Left];

    /// A quarter turn either way; pipes never double back on themselves.
    fn turned(self, clockwise: bool) -> Dir {
        let index = Dir::ALL.iter().position(|&d| d == self).unwrap_or(0);
        Dir::ALL[(index + if clockwise { 1 } else { 3 }) % 4]
    }
}

/// Which piece of pipe a cell holds, by the two sides it connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Segment {
    Horizontal,
    Vertical,
    /// Connects right and down: `┌`.
    RightDown,
    /// Connects left and down: `┐`.
    LeftDown,
    /// Connects up and right: `└`.
    UpRight,
    /// Connects up and left: `┘`.
    UpLeft,
}

impl Segment {
    /// The piece a head lays when it arrives travelling `from` and leaves
    /// travelling `to`.
    fn joining(from: Dir, to: Dir) -> Segment {
        use Dir::*;
        match (from, to) {
            (Left | Right, Left | Right) => Segment::Horizontal,
            (Up | Down, Up | Down) => Segment::Vertical,
            // Arrived moving right (entered from the left), leaving down.
            (Right, Down) | (Up, Left) => Segment::LeftDown,
            (Left, Down) | (Up, Right) => Segment::RightDown,
            (Right, Up) | (Down, Left) => Segment::UpLeft,
            (Left, Up) | (Down, Right) => Segment::UpRight,
        }
    }

    fn glyph(self, set: &[&'static str; 6]) -> &'static str {
        set[match self {
            Segment::Horizontal => 0,
            Segment::Vertical => 1,
            Segment::RightDown => 2,
            Segment::LeftDown => 3,
            Segment::UpRight => 4,
            Segment::UpLeft => 5,
        }]
    }
}

/// Glyphs in [`Segment`] order, one set per board line style.
fn glyph_set(visuals: &Visuals) -> [&'static str; 6] {
    if visuals.ascii_only() {
        return ["-", "|", "+", "+", "+", "+"];
    }
    match visuals.border {
        BorderStyle::Double => ["═", "║", "╔", "╗", "╚", "╝"],
        BorderStyle::Heavy => ["━", "┃", "┏", "┓", "┗", "┛"],
        BorderStyle::Rounded => ["─", "│", "╭", "╮", "╰", "╯"],
        _ => ["─", "│", "┌", "┐", "└", "┘"],
    }
}

#[derive(Debug, Clone, Copy)]
struct Head {
    x: u16,
    y: u16,
    dir: Dir,
    /// Which piece's colour this pipe is laid in.
    colour: PieceKind,
}

#[derive(Debug, Clone, Copy)]
struct Cell {
    segment: Segment,
    colour: PieceKind,
}

pub struct Pipes {
    rng: SmallRng,
    size: Size,
    grid: Vec<Option<Cell>>,
    heads: Vec<Head>,
    /// Steps owed but not yet taken; carries the fraction between ticks.
    pending: f32,
    laid: usize,
}

impl Pipes {
    pub fn new() -> Self {
        Self::with_rng(SmallRng::from_entropy())
    }

    #[cfg(test)]
    fn seeded(seed: u64) -> Self {
        Self::with_rng(SmallRng::seed_from_u64(seed))
    }

    fn with_rng(rng: SmallRng) -> Self {
        Self {
            rng,
            size: Size::new(0, 0),
            grid: Vec::new(),
            heads: Vec::new(),
            pending: 0.0,
            laid: 0,
        }
    }

    /// Wipe the screen and put fresh heads down, as pipes.sh does when it has
    /// drawn its limit.
    fn restart(&mut self) {
        let (width, height) = (self.size.width, self.size.height);
        self.grid = vec![None; width as usize * height as usize];
        self.laid = 0;
        self.heads.clear();
        if width == 0 || height == 0 {
            return;
        }
        for _ in 0..HEADS {
            let head = Head {
                x: self.rng.gen_range(0..width),
                y: self.rng.gen_range(0..height),
                dir: Dir::ALL[self.rng.gen_range(0..4)],
                colour: self.random_colour(),
            };
            self.heads.push(head);
        }
    }

    fn random_colour(&mut self) -> PieceKind {
        PieceKind::ALL[self.rng.gen_range(0..PieceKind::ALL.len())]
    }

    fn step(&mut self) {
        let (width, height) = (self.size.width, self.size.height);
        for index in 0..self.heads.len() {
            let mut head = self.heads[index];
            let dir = if self.rng.gen_bool(TURN_CHANCE) {
                head.dir.turned(self.rng.gen())
            } else {
                head.dir
            };

            self.grid[head.y as usize * width as usize + head.x as usize] = Some(Cell {
                segment: Segment::joining(head.dir, dir),
                colour: head.colour,
            });

            // Off one edge and back in at the other, in a new colour — the
            // pipes.sh look.
            let (x, y, wrapped) = match dir {
                Dir::Up if head.y == 0 => (head.x, height - 1, true),
                Dir::Up => (head.x, head.y - 1, false),
                Dir::Down if head.y + 1 == height => (head.x, 0, true),
                Dir::Down => (head.x, head.y + 1, false),
                Dir::Left if head.x == 0 => (width - 1, head.y, true),
                Dir::Left => (head.x - 1, head.y, false),
                Dir::Right if head.x + 1 == width => (0, head.y, true),
                Dir::Right => (head.x + 1, head.y, false),
            };
            head.x = x;
            head.y = y;
            head.dir = dir;
            if wrapped {
                head.colour = self.random_colour();
            }
            self.heads[index] = head;
            self.laid += 1;
        }

        if self.laid as f32 > self.grid.len() as f32 * FILL_LIMIT {
            self.restart();
        }
    }
}

impl Default for Pipes {
    fn default() -> Self {
        Self::new()
    }
}

impl Background for Pipes {
    fn tick(&mut self, dt: Duration, size: Size, _signal: &PerformanceSignal) {
        // Laid pipe has nowhere sensible to go in a resized grid, so a resize
        // simply starts over.
        if size != self.size {
            self.size = size;
            self.restart();
        }
        if self.heads.is_empty() {
            return;
        }

        self.pending += dt.as_secs_f32() * STEPS_PER_SECOND;
        while self.pending >= 1.0 {
            self.pending -= 1.0;
            self.step();
        }
    }

    fn render(&self, canvas: &mut Canvas, visuals: &Visuals, _signal: &PerformanceSignal) {
        let set = glyph_set(visuals);
        let width = self.size.width as usize;
        if width == 0 {
            return;
        }
        for (index, cell) in self.grid.iter().enumerate() {
            let Some(cell) = cell else { continue };
            let (x, y) = ((index % width) as u16, (index / width) as u16);
            let style = Style::default()
                .fg(visuals.theme.color(cell.colour))
                .add_modifier(Modifier::DIM);
            canvas.set(x, y, cell.segment.glyph(&set), style);
        }
    }

    fn name(&self) -> &'static str {
        "Pipes"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const TICK: Duration = Duration::from_nanos(16_666_667);

    fn draw(pipes: &Pipes, visuals: &Visuals) -> Buffer {
        let area = Rect::new(0, 0, pipes.size.width, pipes.size.height);
        let mut buf = Buffer::empty(area);
        let mut canvas = Canvas::new(&mut buf, area, &[]);
        pipes.render(&mut canvas, visuals, &PerformanceSignal::default());
        buf
    }

    /// A corner has to open towards both the way the pipe came in and the way it
    /// goes out, or the pipe visibly breaks at every turn.
    #[test]
    fn corners_connect_the_side_entered_to_the_side_left() {
        // Travelling right means it entered through the left side.
        assert_eq!(Segment::joining(Dir::Right, Dir::Down), Segment::LeftDown);
        assert_eq!(Segment::joining(Dir::Right, Dir::Up), Segment::UpLeft);
        assert_eq!(Segment::joining(Dir::Left, Dir::Down), Segment::RightDown);
        assert_eq!(Segment::joining(Dir::Left, Dir::Up), Segment::UpRight);
        // Travelling down means it entered through the top.
        assert_eq!(Segment::joining(Dir::Down, Dir::Right), Segment::UpRight);
        assert_eq!(Segment::joining(Dir::Down, Dir::Left), Segment::UpLeft);
        assert_eq!(Segment::joining(Dir::Up, Dir::Right), Segment::RightDown);
        assert_eq!(Segment::joining(Dir::Up, Dir::Left), Segment::LeftDown);

        assert_eq!(Segment::joining(Dir::Up, Dir::Up), Segment::Vertical);
        assert_eq!(Segment::joining(Dir::Left, Dir::Left), Segment::Horizontal);
    }

    #[test]
    fn a_turn_is_never_a_reversal() {
        for dir in Dir::ALL {
            for clockwise in [true, false] {
                let turned = dir.turned(clockwise);
                let horizontal = |d: Dir| matches!(d, Dir::Left | Dir::Right);
                assert_ne!(horizontal(dir), horizontal(turned));
            }
        }
    }

    /// Every laid cell must join its neighbour along the path. Following a single
    /// head step by step checks the glyph against where it actually went next.
    #[test]
    fn a_laid_pipe_is_continuous() {
        let mut pipes = Pipes::seeded(7);
        pipes.tick(TICK, Size::new(60, 30), &PerformanceSignal::default());
        pipes.heads.truncate(1);

        for _ in 0..400 {
            let before = pipes.heads[0];
            pipes.step();
            if pipes.laid == 0 {
                break; // wiped; the path restarts
            }
            let after = pipes.heads[0];
            let cell = pipes.grid[before.y as usize * 60 + before.x as usize].unwrap();
            assert_eq!(cell.segment, Segment::joining(before.dir, after.dir));
        }
    }

    #[test]
    fn the_screen_is_wiped_once_it_has_taken_its_fill() {
        let size = Size::new(20, 10);
        let mut pipes = Pipes::seeded(8);
        let mut wiped = false;
        let mut peak = 0;
        for _ in 0..60 * 30 {
            pipes.tick(TICK, size, &PerformanceSignal::default());
            if pipes.laid < peak {
                wiped = true;
                break;
            }
            peak = pipes.laid;
        }
        assert!(wiped, "the screen never reset");
        assert!(peak as f32 <= 200.0 * FILL_LIMIT + HEADS as f32);
    }

    #[test]
    fn the_pipe_follows_the_border_style_and_the_theme() {
        let size = Size::new(40, 20);
        let mut pipes = Pipes::seeded(9);
        for _ in 0..120 {
            pipes.tick(TICK, size, &PerformanceSignal::default());
        }

        let visuals = Visuals {
            border: BorderStyle::Double,
            ..Default::default()
        };
        let buf = draw(&pipes, &visuals);
        let double = ["═", "║", "╔", "╗", "╚", "╝"];
        let palette: Vec<_> = PieceKind::ALL
            .iter()
            .map(|&kind| visuals.theme.color(kind))
            .collect();

        let mut drawn = 0;
        for cell in buf.content() {
            if cell.symbol() == " " {
                continue;
            }
            drawn += 1;
            assert!(double.contains(&cell.symbol()), "{:?}", cell.symbol());
            assert!(palette.contains(&cell.fg));
        }
        assert!(drawn > 0);

        let ascii = Visuals {
            border: BorderStyle::Ascii,
            ..Default::default()
        };
        assert!(draw(&pipes, &ascii)
            .content()
            .iter()
            .all(|cell| cell.symbol().is_ascii()));
    }

    #[test]
    fn a_resize_starts_over_inside_the_new_bounds() {
        let mut pipes = Pipes::seeded(10);
        for _ in 0..300 {
            pipes.tick(TICK, Size::new(100, 40), &PerformanceSignal::default());
        }
        let small = Size::new(5, 3);
        for _ in 0..300 {
            pipes.tick(TICK, small, &PerformanceSignal::default());
            for head in &pipes.heads {
                assert!(head.x < 5 && head.y < 3);
            }
        }
        assert_eq!(pipes.grid.len(), 15);
    }

    #[test]
    fn a_zero_sized_terminal_is_survived() {
        let mut pipes = Pipes::seeded(11);
        for size in [Size::new(0, 0), Size::new(0, 10), Size::new(1, 1)] {
            for _ in 0..60 {
                pipes.tick(TICK, size, &PerformanceSignal::default());
            }
            draw(&pipes, &Visuals::default());
        }
    }
}
