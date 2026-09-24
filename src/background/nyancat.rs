//! Nyancat (§8, #8): the poptart cat, flying across now and then.
//!
//! The terminal nyancat fills the screen with one enormous cat; behind a board
//! that would be all cat and no game. Here it is a visitor instead: every few
//! seconds a small cat crosses at a random height, trailing a waving rainbow the
//! width of the screen, while a sparse starfield drifts the other way.
//!
//! The rainbow is drawn in the current theme's piece colours, which happen to
//! run red to purple in exactly rainbow order.

use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};

use super::{Background, Canvas, PerformanceSignal, RAINBOW};
use crate::ui::style::Visuals;

/// Columns per second the cat flies at.
const CAT_SPEED: f32 = 18.0;
/// Columns per second the stars drift at, the other way.
const STAR_SPEED: f32 = 6.0;
/// Seconds between one cat leaving and the next arriving.
const BETWEEN: (f32, f32) = (3.0, 9.0);
/// Seconds per animation frame: legs, tail and the rainbow's wave.
const FRAME: f32 = 0.2;
/// Columns in one crest or trough of the rainbow's wave.
const WAVE: i32 = 4;
/// Screen cells per star.
const CELLS_PER_STAR: usize = 160;

/// The cat is drawn in two layers so each can take its own colour. Rows are
/// relative to the cat's top, which sits one row below the rainbow's top.
const POPTART: [&str; 3] = [",------,", "|", "|__"];
const CAT: [&str; 2] = ["    /\\_/\\", "   ( ^ .^)"];
const LEGS: [&str; 2] = [" \"\"  \"\"", "\"\"  \"\""];
const TAIL: [char; 2] = ['~', '-'];
const CAT_WIDTH: i32 = 10;

const TWINKLE: [char; 4] = ['.', '+', '*', '+'];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Flight {
    Waiting {
        remaining: f32,
    },
    /// `x` is the cat's left edge, which starts off screen; `y` is the rainbow's
    /// top row.
    Flying {
        x: f32,
        y: u16,
    },
}

#[derive(Debug, Clone, Copy)]
struct Star {
    x: f32,
    y: u16,
    twinkle: usize,
}

pub struct Nyancat {
    rng: SmallRng,
    size: Size,
    flight: Flight,
    stars: Vec<Star>,
    /// Time into the current animation frame.
    clock: f32,
    frame: usize,
}

impl Nyancat {
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
            // The first cat comes quickly, so the title screen's attract mode
            // shows one before anybody gets bored.
            flight: Flight::Waiting { remaining: 1.0 },
            stars: Vec::new(),
            clock: 0.0,
            frame: 0,
        }
    }

    fn resize(&mut self, size: Size) {
        self.size = size;
        let count = size.width as usize * size.height as usize / CELLS_PER_STAR;
        self.stars.truncate(count);
        for star in &mut self.stars {
            if star.x >= f32::from(size.width) || star.y >= size.height {
                star.x = self.rng.gen_range(0.0..f32::from(size.width));
                star.y = self.rng.gen_range(0..size.height);
            }
        }
        while self.stars.len() < count {
            let star = Star {
                x: self.rng.gen_range(0.0..f32::from(size.width)),
                y: self.rng.gen_range(0..size.height),
                twinkle: self.rng.gen_range(0..TWINKLE.len()),
            };
            self.stars.push(star);
        }
        if let Flight::Flying { x, y } = self.flight {
            self.flight = Flight::Flying {
                x,
                y: y.min(self.lowest_top()),
            };
        }
    }

    /// The lowest rainbow top that still fits the whole rainbow on screen, with a
    /// row spare for the wave's dip.
    fn lowest_top(&self) -> u16 {
        self.size.height.saturating_sub(RAINBOW.len() as u16 + 1)
    }

    /// How far behind the cat the rainbow reaches: all the way across, so it
    /// streams in from the left edge and then follows the cat off the right.
    fn trail(&self) -> f32 {
        f32::from(self.size.width)
    }
}

impl Default for Nyancat {
    fn default() -> Self {
        Self::new()
    }
}

impl Background for Nyancat {
    fn tick(&mut self, dt: Duration, size: Size, _signal: &PerformanceSignal) {
        if size != self.size {
            self.resize(size);
        }
        let dt = dt.as_secs_f32();

        self.clock += dt;
        if self.clock >= FRAME {
            self.clock -= FRAME;
            self.frame = (self.frame + 1) % 2;
            for star in &mut self.stars {
                if self.rng.gen_bool(0.3) {
                    star.twinkle = (star.twinkle + 1) % TWINKLE.len();
                }
            }
        }

        for index in 0..self.stars.len() {
            let star = &mut self.stars[index];
            star.x -= STAR_SPEED * dt;
            if star.x < 0.0 {
                star.x += f32::from(size.width.max(1));
                star.y = self.rng.gen_range(0..size.height.max(1));
            }
        }

        self.flight = match self.flight {
            Flight::Waiting { remaining } if remaining <= dt => Flight::Flying {
                x: -(CAT_WIDTH as f32),
                y: self.rng.gen_range(0..=self.lowest_top()),
            },
            Flight::Waiting { remaining } => Flight::Waiting {
                remaining: remaining - dt,
            },
            Flight::Flying { x, y } => {
                let x = x + CAT_SPEED * dt;
                // Gone once the end of the rainbow has followed it off screen.
                if x - self.trail() > f32::from(size.width) {
                    Flight::Waiting {
                        remaining: self.rng.gen_range(BETWEEN.0..BETWEEN.1),
                    }
                } else {
                    Flight::Flying { x, y }
                }
            }
        };
    }

    fn render(&self, canvas: &mut Canvas, visuals: &Visuals, _signal: &PerformanceSignal) {
        let star_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::DIM);
        for star in &self.stars {
            canvas.put(star.x as u16, star.y, TWINKLE[star.twinkle], star_style);
        }

        let Flight::Flying { x, y } = self.flight else {
            return;
        };
        let cat_x = x.floor() as i32;

        // The rainbow, back from the cat's tail, rising and falling a row in
        // alternate stretches — and the stretches swap every frame, so it waves.
        let glyph = if visuals.ascii_only() { '=' } else { '█' };
        let tail_end = (x - self.trail()).floor() as i32;
        for column in tail_end.max(0)..cat_x.min(i32::from(canvas.width())) {
            let stretch = (cat_x - 1 - column) / WAVE;
            let dip = (stretch + self.frame as i32) % 2;
            for (band, kind) in RAINBOW.iter().enumerate() {
                let row = i32::from(y) + band as i32 + dip;
                let style = Style::default()
                    .fg(visuals.theme.color(*kind))
                    .add_modifier(Modifier::DIM);
                canvas.put(column, row, glyph, style);
            }
        }

        let top = i32::from(y) + 1;
        let poptart = Style::default().fg(Color::LightMagenta);
        let cat = Style::default().fg(Color::White);
        for (row, line) in POPTART.iter().enumerate() {
            canvas.text(cat_x, top + row as i32, line, poptart);
        }
        for (row, line) in CAT.iter().enumerate() {
            canvas.text(cat_x, top + 1 + row as i32, line, cat);
        }
        canvas.text(cat_x, top + 3, LEGS[self.frame], cat);
        canvas.put(cat_x - 1, top + 2, TAIL[self.frame], cat);
    }

    fn name(&self) -> &'static str {
        "Nyancat"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const TICK: Duration = Duration::from_nanos(16_666_667);

    fn draw(cat: &Nyancat) -> Buffer {
        let area = Rect::new(0, 0, cat.size.width, cat.size.height);
        let mut buf = Buffer::empty(area);
        let mut canvas = Canvas::new(&mut buf, area, &[]);
        cat.render(
            &mut canvas,
            &Visuals::default(),
            &PerformanceSignal::default(),
        );
        buf
    }

    fn as_string(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn fly_until_on_screen(cat: &mut Nyancat, size: Size, column: f32) {
        for _ in 0..60 * 30 {
            cat.tick(TICK, size, &PerformanceSignal::default());
            if matches!(cat.flight, Flight::Flying { x, .. } if x >= column) {
                return;
            }
        }
        panic!("the cat never got to column {column}");
    }

    #[test]
    fn a_cat_crosses_with_its_rainbow_behind_it() {
        let size = Size::new(60, 20);
        let mut cat = Nyancat::seeded(1);
        fly_until_on_screen(&mut cat, size, 30.0);

        let rendered = as_string(&draw(&cat));
        println!("{rendered}");
        assert!(rendered.contains("( ^ .^)"), "the cat's face");
        assert!(rendered.contains(",------,"), "the poptart");

        let Flight::Flying { x, y } = cat.flight else {
            unreachable!()
        };
        // Every band is somewhere left of the cat, in its own colour.
        let buf = draw(&cat);
        for (band, kind) in RAINBOW.iter().enumerate() {
            let colour = Visuals::default().theme.color(*kind);
            let found = (0..x as u16).any(|column| {
                [0, 1].iter().any(|dip| {
                    let row = y + band as u16 + dip;
                    row < size.height && buf[(column, row)].fg == colour
                })
            });
            assert!(found, "band {band} missing");
        }
    }

    #[test]
    fn the_rainbow_waves_between_frames() {
        let size = Size::new(60, 20);
        let mut cat = Nyancat::seeded(2);
        fly_until_on_screen(&mut cat, size, 40.0);

        let first = as_string(&draw(&cat));
        let frame = cat.frame;
        while cat.frame == frame {
            cat.tick(TICK, size, &PerformanceSignal::default());
        }
        assert_ne!(first, as_string(&draw(&cat)));
    }

    #[test]
    fn after_crossing_the_cat_leaves_and_another_comes() {
        let size = Size::new(40, 12);
        let mut cat = Nyancat::seeded(3);
        let mut crossings = 0;
        let mut was_flying = false;
        for _ in 0..60 * 60 {
            cat.tick(TICK, size, &PerformanceSignal::default());
            let flying = matches!(cat.flight, Flight::Flying { .. });
            if was_flying && !flying {
                crossings += 1;
            }
            was_flying = flying;
        }
        assert!(crossings >= 2, "only {crossings} crossings in a minute");
    }

    #[test]
    fn the_whole_rainbow_fits_whatever_row_it_picks() {
        let size = Size::new(30, 8);
        let mut cat = Nyancat::seeded(4);
        for _ in 0..60 * 60 {
            cat.tick(TICK, size, &PerformanceSignal::default());
            if let Flight::Flying { y, .. } = cat.flight {
                assert!(
                    y + (RAINBOW.len() as u16) < size.height,
                    "no room for the dip"
                );
            }
        }
    }

    #[test]
    fn stars_stay_on_screen_through_a_resize() {
        let mut cat = Nyancat::seeded(5);
        for _ in 0..120 {
            cat.tick(TICK, Size::new(100, 40), &PerformanceSignal::default());
        }
        let small = Size::new(20, 8);
        for _ in 0..120 {
            cat.tick(TICK, small, &PerformanceSignal::default());
        }
        assert_eq!(cat.stars.len(), 1);
        for star in &cat.stars {
            assert!(star.x < 20.0 && star.y < 8);
        }
        draw(&cat);
    }

    #[test]
    fn tiny_and_empty_terminals_are_survived() {
        let mut cat = Nyancat::seeded(6);
        for size in [Size::new(0, 0), Size::new(3, 2), Size::new(1, 40)] {
            for _ in 0..600 {
                cat.tick(TICK, size, &PerformanceSignal::default());
            }
            draw(&cat);
        }
    }
}
