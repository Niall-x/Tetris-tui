//! Locomotive (§8, #9): sl's steam train, chugging across now and then.
//!
//! Like sl it runs right to left along the bottom of the screen, wheels turning
//! and smoke rising from the chimney, and then the screen is quiet again until
//! the next one. It is also the game-over easter egg the brief asks for:
//! topping out sends a train through straight away.
//!
//! The train is our own art rather than sl's D51, as with every other
//! background here.

use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};

use super::{Background, Canvas, PerformanceSignal};
use crate::ui::style::Visuals;

/// Columns per second; sl's own pace is about this.
const SPEED: f32 = 24.0;
/// Seconds to the first train, short so picking this background shows one.
const FIRST_TRAIN: f32 = 2.0;
/// Seconds between trains after that.
const BETWEEN: (f32, f32) = (20.0, 45.0);
/// Seconds per quarter turn of the wheels.
const WHEEL_TURN: f32 = 0.08;
/// Seconds between puffs of smoke.
const PUFF_EVERY: f32 = 0.2;
/// Rows per second smoke rises, and columns per second it drifts back.
const SMOKE_RISE: f32 = 3.0;
const SMOKE_DRIFT: f32 = 4.0;
/// A puff's growth, stage by stage, and how long each stage lasts.
const PUFF: [&str; 5] = [".", "o", "O", "( )", "(   )"];
const PUFF_STAGE: f32 = 0.35;

/// Rows of each vehicle, top to bottom; `{w}` is a wheel's spoke. The engine
/// leads, chimney at the front.
const ENGINE: [&str; 7] = [
    "   ___                      ",
    "   | |           _________  ",
    " __|_|__________|  _   _  | ",
    "|  ___________  | |_| |_| | ",
    "| |___________| |_________| ",
    "|__________________________|",
    "  ({w})=({w})=({w})        ({w})    ",
];
const TENDER: [&str; 7] = [
    "              ",
    "              ",
    " /\\/\\/\\/\\/\\/\\ ",
    "|____________|",
    "|            |",
    "|____________|",
    "  ({w})    ({w})  ",
];
const CARRIAGE: [&str; 7] = [
    "                    ",
    " __________________ ",
    "|  __  __  __  __  |",
    "| |__||__||__||__| |",
    "|                  |",
    "|__________________|",
    "   ({w})        ({w})   ",
];
/// The row the couplings sit on.
const COUPLING_ROW: usize = 5;
/// Where the chimney is, from the train's left edge.
const CHIMNEY: f32 = 4.0;
/// Turning right to left, the wheels go anticlockwise.
const SPOKES: [&str; 4] = ["|", "\\", "-", "/"];

/// The whole train for one wheel position: engine, tender and two carriages
/// coupled together.
fn train(spoke: &str) -> Vec<String> {
    let vehicles: [&[&str; 7]; 4] = [&ENGINE, &TENDER, &CARRIAGE, &CARRIAGE];
    (0..ENGINE.len())
        .map(|row| {
            let coupling = if row == COUPLING_ROW { "=" } else { " " };
            vehicles
                .iter()
                .map(|vehicle| vehicle[row].replace("{w}", spoke))
                .collect::<Vec<_>>()
                .join(coupling)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Run {
    Waiting {
        remaining: f32,
    },
    /// `x` is the engine's front.
    Crossing {
        x: f32,
    },
}

#[derive(Debug, Clone, Copy)]
struct Puff {
    x: f32,
    y: f32,
    age: f32,
}

pub struct Locomotive {
    rng: SmallRng,
    size: Size,
    run: Run,
    /// The train at each wheel position, built once.
    frames: Vec<Vec<String>>,
    width: f32,
    smoke: Vec<Puff>,
    wheel_clock: f32,
    puff_clock: f32,
    /// Whether the last signal was a game over, so topping out sends one
    /// train rather than one per tick.
    was_over: bool,
}

impl Locomotive {
    pub fn new() -> Self {
        Self::with_rng(SmallRng::from_entropy())
    }

    #[cfg(test)]
    fn seeded(seed: u64) -> Self {
        Self::with_rng(SmallRng::seed_from_u64(seed))
    }

    fn with_rng(rng: SmallRng) -> Self {
        let frames: Vec<Vec<String>> = SPOKES.iter().map(|spoke| train(spoke)).collect();
        let width = frames[0]
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0) as f32;
        Self {
            rng,
            size: Size::new(0, 0),
            run: Run::Waiting {
                remaining: FIRST_TRAIN,
            },
            frames,
            width,
            smoke: Vec::new(),
            wheel_clock: 0.0,
            puff_clock: 0.0,
            was_over: false,
        }
    }

    fn depart(&mut self) {
        self.run = Run::Crossing {
            x: f32::from(self.size.width),
        };
    }

    /// The train's top row: it runs along the bottom of the screen.
    fn top(&self) -> f32 {
        f32::from(self.size.height) - ENGINE.len() as f32
    }

    fn frame(&self) -> &[String] {
        let turn = (self.wheel_clock / WHEEL_TURN) as usize;
        &self.frames[turn % self.frames.len()]
    }
}

impl Default for Locomotive {
    fn default() -> Self {
        Self::new()
    }
}

impl Background for Locomotive {
    fn tick(&mut self, dt: Duration, size: Size, signal: &PerformanceSignal) {
        self.size = size;
        let dt = dt.as_secs_f32();

        // The easter egg: a run ending sends a train, unless one is already on
        // its way.
        if signal.game_over && !self.was_over && matches!(self.run, Run::Waiting { .. }) {
            self.depart();
        }
        self.was_over = signal.game_over;

        match self.run {
            Run::Waiting { remaining } if remaining <= dt => self.depart(),
            Run::Waiting { remaining } => {
                self.run = Run::Waiting {
                    remaining: remaining - dt,
                }
            }
            Run::Crossing { x } => {
                let x = x - SPEED * dt;
                self.wheel_clock += dt;
                self.puff_clock += dt;
                if self.puff_clock >= PUFF_EVERY {
                    self.puff_clock -= PUFF_EVERY;
                    self.smoke.push(Puff {
                        x: x + CHIMNEY,
                        y: self.top() - 1.0,
                        age: 0.0,
                    });
                }
                self.run = if x + self.width < 0.0 {
                    Run::Waiting {
                        remaining: self.rng.gen_range(BETWEEN.0..BETWEEN.1),
                    }
                } else {
                    Run::Crossing { x }
                };
            }
        }

        // Smoke rises, drifts back along the line and thins out.
        for puff in &mut self.smoke {
            puff.age += dt;
            puff.y -= SMOKE_RISE * dt;
            puff.x += SMOKE_DRIFT * dt;
        }
        let lifetime = PUFF.len() as f32 * PUFF_STAGE;
        self.smoke
            .retain(|puff| puff.age < lifetime && puff.y >= 0.0);
    }

    fn render(&self, canvas: &mut Canvas, _visuals: &Visuals, _signal: &PerformanceSignal) {
        let smoke = Style::default().fg(Color::Gray).add_modifier(Modifier::DIM);
        for puff in &self.smoke {
            let stage = ((puff.age / PUFF_STAGE) as usize).min(PUFF.len() - 1);
            let glyphs = PUFF[stage];
            // Centred on the puff, so it spreads as it grows.
            let left = puff.x.floor() as i32 - glyphs.chars().count() as i32 / 2;
            text(canvas, left, puff.y.floor() as i32, glyphs, smoke);
        }

        let Run::Crossing { x } = self.run else {
            return;
        };
        let body = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::DIM);
        let top = self.top().floor() as i32;
        let left = x.floor() as i32;
        for (row, line) in self.frame().iter().enumerate() {
            text(canvas, left, top + row as i32, line, body);
        }
    }

    fn name(&self) -> &'static str {
        "Locomotive"
    }
}

/// Like [`Canvas::text`], but for a line that may start off the left or top
/// edge, as the train does for most of its run.
fn text(canvas: &mut Canvas, x: i32, y: i32, line: &str, style: Style) {
    let Ok(y) = u16::try_from(y) else { return };
    for (offset, ch) in line.chars().enumerate() {
        if ch == ' ' {
            continue;
        }
        if let Ok(x) = u16::try_from(x + offset as i32) {
            canvas.put(x, y, ch, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const TICK: Duration = Duration::from_nanos(16_666_667);
    const SIZE: Size = Size::new(100, 24);

    fn run(loco: &mut Locomotive, seconds: f32, signal: &PerformanceSignal) {
        for _ in 0..(seconds * 60.0) as usize {
            loco.tick(TICK, SIZE, signal);
        }
    }

    fn as_string(loco: &Locomotive) -> String {
        let area = Rect::new(0, 0, SIZE.width, SIZE.height);
        let mut buf = Buffer::empty(area);
        let mut canvas = Canvas::new(&mut buf, area, &[]);
        loco.render(
            &mut canvas,
            &Visuals::default(),
            &PerformanceSignal::default(),
        );
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    /// Every row of every vehicle is the vehicle's width, or the couplings and
    /// the next vehicle along would come out crooked.
    #[test]
    fn every_vehicle_is_a_clean_rectangle() {
        for vehicle in [&ENGINE, &TENDER, &CARRIAGE] {
            let widths: Vec<usize> = vehicle
                .iter()
                .map(|row| row.replace("{w}", "|").chars().count())
                .collect();
            assert!(
                widths.windows(2).all(|pair| pair[0] == pair[1]),
                "{widths:?}"
            );
        }
        let rows = train("|");
        assert!(rows.windows(2).all(|pair| pair[0].len() == pair[1].len()));
    }

    #[test]
    fn the_first_train_comes_soon_and_runs_right_to_left() {
        let mut loco = Locomotive::seeded(1);
        let calm = PerformanceSignal::default();
        run(&mut loco, FIRST_TRAIN + 0.1, &calm);
        let Run::Crossing { x: start } = loco.run else {
            panic!("no train after {FIRST_TRAIN}s");
        };
        run(&mut loco, 1.0, &calm);
        let Run::Crossing { x } = loco.run else {
            panic!("the train vanished mid-crossing");
        };
        assert!(x < start, "it went the wrong way");

        let rendered = as_string(&loco);
        println!("{rendered}");
        let rows: Vec<&str> = rendered.lines().collect();
        assert!(
            rows[SIZE.height as usize - 1].contains('('),
            "wheels on the bottom row"
        );
    }

    #[test]
    fn a_crossing_ends_and_the_line_goes_quiet_until_the_next() {
        let mut loco = Locomotive::seeded(2);
        let calm = PerformanceSignal::default();
        // Onto the screen, then all the way across: width plus train length.
        let crossing = (f32::from(SIZE.width) + loco.width) / SPEED;
        run(&mut loco, FIRST_TRAIN + crossing + 0.5, &calm);
        assert!(matches!(loco.run, Run::Waiting { .. }));
        let Run::Waiting { remaining } = loco.run else {
            unreachable!()
        };
        assert!(remaining >= BETWEEN.0 - 1.0);
    }

    /// The easter egg: a run ending sends a train at once, and only one.
    #[test]
    fn topping_out_sends_a_train_through() {
        let mut loco = Locomotive::seeded(3);
        let calm = PerformanceSignal::default();
        let crossing = (f32::from(SIZE.width) + loco.width) / SPEED;
        run(&mut loco, FIRST_TRAIN + crossing + 0.5, &calm);
        assert!(matches!(loco.run, Run::Waiting { .. }));

        let over = PerformanceSignal {
            game_over: true,
            ..Default::default()
        };
        loco.tick(TICK, SIZE, &over);
        assert!(
            matches!(loco.run, Run::Crossing { .. }),
            "no train at game over"
        );

        // Staying on the game-over screen must not send another after it.
        run(&mut loco, crossing + 0.5, &over);
        assert!(matches!(loco.run, Run::Waiting { .. }));
        run(&mut loco, 5.0, &over);
        assert!(matches!(loco.run, Run::Waiting { .. }));
    }

    #[test]
    fn smoke_rises_from_the_chimney_and_clears() {
        let mut loco = Locomotive::seeded(4);
        let calm = PerformanceSignal::default();
        run(&mut loco, FIRST_TRAIN + 1.5, &calm);
        assert!(!loco.smoke.is_empty(), "a moving train smokes");
        for puff in &loco.smoke {
            assert!(puff.y < loco.top(), "smoke below the chimney");
        }

        let crossing = (f32::from(SIZE.width) + loco.width) / SPEED;
        run(&mut loco, crossing + PUFF.len() as f32 * PUFF_STAGE, &calm);
        assert!(loco.smoke.is_empty(), "the smoke never cleared");
    }

    #[test]
    fn the_wheels_turn_as_it_goes() {
        let mut loco = Locomotive::seeded(5);
        let calm = PerformanceSignal::default();
        run(&mut loco, FIRST_TRAIN + 0.5, &calm);
        let first = loco.frame().last().unwrap().clone();
        run(&mut loco, WHEEL_TURN * 1.5, &calm);
        assert_ne!(loco.frame().last().unwrap(), &first);
    }
}
