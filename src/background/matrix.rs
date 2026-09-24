//! Matrix rain (§8, #2): falling glyph columns, cmatrix-style.
//!
//! Every other terminal column carries a drop — a bright head trailing a fading
//! green tail — which falls, runs off the bottom, and after a rest starts again
//! from the top at a new speed and length. Glyphs under a tail change now and
//! then, which is most of what makes it read as rain rather than falling text.

use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};

use super::{Background, Canvas, PerformanceSignal};
use crate::ui::style::Visuals;

/// Columns from one drop to the next. cmatrix fills every column; half that is
/// plenty behind a board and keeps glyphs from butting up against each other.
const SPACING: u16 = 2;
/// Rows per second, slowest to fastest.
const SPEED: (f32, f32) = (6.0, 20.0);
/// Seconds a column rests between drops.
const REST: (f32, f32) = (0.3, 4.0);
/// Changes per second to a column's glyphs.
const FLICKER_PER_SECOND: f32 = 6.0;

/// Half-width katakana (single-column, like the film's), digits and a few
/// symbols.
const GLYPHS: &[char] = &[
    'ｱ', 'ｲ', 'ｳ', 'ｴ', 'ｵ', 'ｶ', 'ｷ', 'ｸ', 'ｹ', 'ｺ', 'ｻ', 'ｼ', 'ｽ', 'ｾ', 'ｿ', 'ﾀ', 'ﾁ', 'ﾂ', 'ﾃ',
    'ﾄ', 'ﾅ', 'ﾆ', 'ﾇ', 'ﾈ', 'ﾉ', 'ﾊ', 'ﾋ', 'ﾌ', 'ﾍ', 'ﾎ', 'ﾏ', 'ﾐ', 'ﾑ', 'ﾒ', 'ﾓ', 'ﾔ', 'ﾕ', 'ﾖ',
    'ﾗ', 'ﾘ', 'ﾙ', 'ﾚ', 'ﾛ', 'ﾜ', 'ﾝ', '0', '1', '2', '3', '4', '5', '7', '8', '9', ':', '=', '*',
    '+', '<', '>', '|',
];

/// cmatrix's own default set, for terminals without the katakana.
const ASCII_GLYPHS: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't',
    'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '$', '%', '&',
    '#', '@', '*', '+', '=', '<', '>', '?',
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Drop {
    Resting {
        remaining: f32,
    },
    Falling {
        /// Row of the head, fractional so slow drops move smoothly on average.
        head: f32,
        /// Rows per second.
        speed: f32,
        length: u16,
    },
}

#[derive(Debug, Clone)]
struct Column {
    drop: Drop,
    /// One random value per row, mapped onto a glyph set when drawn — which
    /// set is a rendering choice, so it can follow the player's settings.
    glyphs: Vec<u16>,
}

pub struct MatrixRain {
    rng: SmallRng,
    size: Size,
    columns: Vec<Column>,
}

impl MatrixRain {
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
            columns: Vec::new(),
        }
    }

    /// Fit the columns to a new size, keeping any drop already on screen so a
    /// resize does not restart the whole display.
    fn resize(&mut self, size: Size) {
        self.size = size;
        let count = size.width.div_ceil(SPACING) as usize;
        let height = size.height as usize;

        self.columns.truncate(count);
        for column in &mut self.columns {
            let rng = &mut self.rng;
            column.glyphs.resize_with(height, || rng.gen());
        }
        while self.columns.len() < count {
            // A fresh column starts resting, staggered, so a first frame or a
            // widened terminal fills in gradually rather than all at once.
            let remaining = self.rng.gen_range(0.0..REST.1);
            let glyphs = (0..height).map(|_| self.rng.gen()).collect();
            self.columns.push(Column {
                drop: Drop::Resting { remaining },
                glyphs,
            });
        }
    }

    fn new_drop(&mut self) -> Drop {
        let height = self.size.height.max(1);
        let longest = (height * 2 / 3).max(5);
        Drop::Falling {
            head: 0.0,
            speed: self.rng.gen_range(SPEED.0..SPEED.1),
            length: self.rng.gen_range(4..=longest),
        }
    }
}

impl Default for MatrixRain {
    fn default() -> Self {
        Self::new()
    }
}

impl Background for MatrixRain {
    fn tick(&mut self, dt: Duration, size: Size, _signal: &PerformanceSignal) {
        if size != self.size {
            self.resize(size);
        }
        let dt = dt.as_secs_f32();
        let height = f32::from(size.height);

        for index in 0..self.columns.len() {
            let next = match self.columns[index].drop {
                Drop::Resting { remaining } if remaining <= dt => self.new_drop(),
                Drop::Resting { remaining } => Drop::Resting {
                    remaining: remaining - dt,
                },
                Drop::Falling {
                    head,
                    speed,
                    length,
                } => {
                    let head = head + speed * dt;
                    if head - f32::from(length) > height {
                        Drop::Resting {
                            remaining: self.rng.gen_range(REST.0..REST.1),
                        }
                    } else {
                        Drop::Falling {
                            head,
                            speed,
                            length,
                        }
                    }
                }
            };

            let flicker = self
                .rng
                .gen_bool(f64::from((FLICKER_PER_SECOND * dt).min(1.0)));
            let column = &mut self.columns[index];
            column.drop = next;
            if flicker && !column.glyphs.is_empty() {
                let row = self.rng.gen_range(0..column.glyphs.len());
                column.glyphs[row] = self.rng.gen();
            }
        }
    }

    fn render(&self, canvas: &mut Canvas, visuals: &Visuals, _signal: &PerformanceSignal) {
        let glyphs = if visuals.ascii_only() {
            ASCII_GLYPHS
        } else {
            GLYPHS
        };

        let head_style = Style::default()
            .fg(Color::LightGreen)
            .add_modifier(Modifier::BOLD);
        let body_style = Style::default().fg(Color::Green);
        let tail_style = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::DIM);

        for (index, column) in self.columns.iter().enumerate() {
            let Drop::Falling { head, length, .. } = column.drop else {
                continue;
            };
            let x = index as u16 * SPACING;
            let head = head.floor() as i32;

            for offset in 0..i32::from(length) {
                let row = head - offset;
                if row < 0 {
                    break;
                }
                let Some(&value) = column.glyphs.get(row as usize) else {
                    continue; // still below the bottom edge
                };
                let style = if offset == 0 {
                    head_style
                } else if offset < i32::from(length) * 2 / 3 {
                    body_style
                } else {
                    tail_style
                };
                let glyph = glyphs[value as usize % glyphs.len()];
                canvas.put(x, row as u16, glyph, style);
            }
        }
    }

    fn name(&self) -> &'static str {
        "Matrix rain"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const TICK: Duration = Duration::from_nanos(16_666_667);

    fn run(rain: &mut MatrixRain, ticks: usize, size: Size) {
        for _ in 0..ticks {
            rain.tick(TICK, size, &PerformanceSignal::default());
        }
    }

    fn draw(rain: &MatrixRain, size: Size, visuals: &Visuals) -> Buffer {
        let area = Rect::new(0, 0, size.width, size.height);
        let mut buf = Buffer::empty(area);
        let mut canvas = Canvas::new(&mut buf, area, &[]);
        rain.render(&mut canvas, visuals, &PerformanceSignal::default());
        buf
    }

    fn heads(rain: &MatrixRain) -> Vec<(usize, f32)> {
        rain.columns
            .iter()
            .enumerate()
            .filter_map(|(index, column)| match column.drop {
                Drop::Falling { head, .. } => Some((index, head)),
                Drop::Resting { .. } => None,
            })
            .collect()
    }

    #[test]
    fn there_is_one_column_every_other_terminal_column() {
        let mut rain = MatrixRain::seeded(1);
        rain.tick(TICK, Size::new(80, 24), &PerformanceSignal::default());
        assert_eq!(rain.columns.len(), 40);
        rain.tick(TICK, Size::new(81, 24), &PerformanceSignal::default());
        assert_eq!(rain.columns.len(), 41, "a final odd column still gets one");
    }

    #[test]
    fn drops_fall_downward() {
        let size = Size::new(80, 24);
        let mut rain = MatrixRain::seeded(2);
        run(&mut rain, 120, size);
        let before = heads(&rain);
        assert!(!before.is_empty(), "two seconds in, something is falling");

        rain.tick(TICK, size, &PerformanceSignal::default());
        let after = heads(&rain);
        for (index, head) in before {
            if let Some(&(_, next)) = after.iter().find(|(i, _)| *i == index) {
                assert!(next > head || next == 0.0, "column {index} rose");
            }
        }
    }

    #[test]
    fn a_drop_that_leaves_the_bottom_rests_and_then_comes_back() {
        let size = Size::new(2, 10);
        let mut rain = MatrixRain::seeded(3);
        let mut rested_after_falling = false;
        let mut fell_again = false;
        let mut was_falling = false;

        for _ in 0..60 * 20 {
            rain.tick(TICK, size, &PerformanceSignal::default());
            let falling = matches!(rain.columns[0].drop, Drop::Falling { .. });
            if was_falling && !falling {
                rested_after_falling = true;
            }
            if rested_after_falling && falling {
                fell_again = true;
                break;
            }
            was_falling = falling;
        }
        assert!(rested_after_falling && fell_again);
    }

    #[test]
    fn the_rain_is_drawn_in_green_and_only_on_its_own_columns() {
        let size = Size::new(40, 20);
        let mut rain = MatrixRain::seeded(4);
        run(&mut rain, 240, size);
        let buf = draw(&rain, size, &Visuals::default());

        let mut drawn = 0;
        for y in 0..size.height {
            for x in 0..size.width {
                let cell = &buf[(x, y)];
                if cell.symbol() == " " {
                    continue;
                }
                drawn += 1;
                assert_eq!(x % SPACING, 0, "({x}, {y}) is between columns");
                assert!(
                    matches!(cell.fg, Color::Green | Color::LightGreen),
                    "({x}, {y}) is {:?}",
                    cell.fg
                );
            }
        }
        assert!(drawn > 0);
    }

    #[test]
    fn the_ascii_settings_keep_the_glyphs_ascii() {
        use crate::ui::style::Skin;

        let size = Size::new(40, 20);
        let mut rain = MatrixRain::seeded(5);
        run(&mut rain, 240, size);
        let visuals = Visuals {
            skin: Skin::AsciiBracket,
            ..Default::default()
        };
        let buf = draw(&rain, size, &visuals);
        assert!(buf.content().iter().all(|cell| cell.symbol().is_ascii()));
    }

    #[test]
    fn a_shrinking_terminal_keeps_the_columns_in_bounds() {
        let mut rain = MatrixRain::seeded(6);
        run(&mut rain, 200, Size::new(120, 40));
        let small = Size::new(10, 5);
        run(&mut rain, 1, small);
        assert_eq!(rain.columns.len(), 5);
        assert!(rain.columns.iter().all(|column| column.glyphs.len() == 5));
        draw(&rain, small, &Visuals::default());
    }
}
