//! Locomotive (§8, #9): sl's D51 steam locomotive, back and forth.
//!
//! The train is sl's own: its D51 and coal car, and its six-frame wheel and
//! rod animation, stepping a frame for every column moved at sl's pace. It
//! crosses the screen, the line is quiet for a moment, and it comes back the
//! other way, at a new random height each crossing. sl only ever runs right to
//! left, so for the way back the art is mirrored and the train faces the way
//! it is going.
//!
//! Topping out is the game-over easter egg the brief asks for: it cuts a pause
//! short and sends the next train through at once.
//!
//! The D51 art is from sl (<https://github.com/mtoyoda/sl>), under its licence:
//!
//! > Copyright 1993,1998,2014 Toyoda Masashi (mtoyoda@acm.org)
//! >
//! > Everyone is permitted to do anything on this program including copying,
//! > modifying, and improving, unless you try to pretend that you wrote it.
//! > i.e., the above copyright notice has to appear in all copies.
//! > THE AUTHOR DISCLAIMS ANY RESPONSIBILITY WITH REGARD TO THIS SOFTWARE.

use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};

use super::{mirror, Background, Canvas, PerformanceSignal};
use crate::ui::style::Visuals;

/// Columns per second: sl moves one column every 40ms.
const SPEED: f32 = 25.0;
/// Seconds to the first train, short so picking this background shows one.
const FIRST_TRAIN: f32 = 2.0;
/// Seconds the line is quiet between a crossing and the next one back.
const PAUSE: (f32, f32) = (1.0, 2.5);
/// Seconds between puffs of smoke.
const PUFF_EVERY: f32 = 0.2;
/// Rows per second smoke rises, and columns per second it drifts back.
const SMOKE_RISE: f32 = 3.0;
const SMOKE_DRIFT: f32 = 4.0;
/// A puff's growth, stage by stage, and how long each stage lasts.
const PUFF: [&str; 5] = [".", "o", "O", "( )", "(   )"];
const PUFF_STAGE: f32 = 0.35;

/// sl's `D51STR1` to `D51STR7`: the engine above its wheels, the same in
/// every frame.
const D51_BODY: [&str; 7] = [
    "      ====        ________                ___________ ",
    "  _D _|  |_______/        \\__I_I_____===__|_________| ",
    "   |(_)---  |   H\\________/ |   |        =|___ ___|   ",
    "   /     |  |   H  |  |     |   |         ||_| |_||   ",
    "  |      |  |   H  |__--------------------| [___] |   ",
    "  | ________|___H__/__|_____/[][]~\\_______|       |   ",
    "  |/ |   |-----------I_____I [][] []  D   |=======|__ ",
];
/// sl's `D51WHL11` to `D51WHL63`: the wheels and coupling rods in each of
/// their six positions.
const D51_WHEELS: [[&str; 3]; 6] = [
    [
        "__/ =| o |=-~~\\  /~~\\  /~~\\  /~~\\ ____Y___________|__ ",
        " |/-=|___|=    ||    ||    ||    |_____/~\\___/        ",
        "  \\_/      \\O=====O=====O=====O_/      \\_/            ",
    ],
    [
        "__/ =| o |=-~~\\  /~~\\  /~~\\  /~~\\ ____Y___________|__ ",
        " |/-=|___|=O=====O=====O=====O   |_____/~\\___/        ",
        "  \\_/      \\__/  \\__/  \\__/  \\__/      \\_/            ",
    ],
    [
        "__/ =| o |=-O=====O=====O=====O \\ ____Y___________|__ ",
        " |/-=|___|=    ||    ||    ||    |_____/~\\___/        ",
        "  \\_/      \\__/  \\__/  \\__/  \\__/      \\_/            ",
    ],
    [
        "__/ =| o |=-~O=====O=====O=====O\\ ____Y___________|__ ",
        " |/-=|___|=    ||    ||    ||    |_____/~\\___/        ",
        "  \\_/      \\__/  \\__/  \\__/  \\__/      \\_/            ",
    ],
    [
        "__/ =| o |=-~~\\  /~~\\  /~~\\  /~~\\ ____Y___________|__ ",
        " |/-=|___|=   O=====O=====O=====O|_____/~\\___/        ",
        "  \\_/      \\__/  \\__/  \\__/  \\__/      \\_/            ",
    ],
    [
        "__/ =| o |=-~~\\  /~~\\  /~~\\  /~~\\ ____Y___________|__ ",
        " |/-=|___|=    ||    ||    ||    |_____/~\\___/        ",
        "  \\_/      \\_O=====O=====O=====O/      \\_/            ",
    ],
];
/// sl's `COAL01` to `COAL10`: the coal car, coupled on at `COAL_AT`.
const COAL: [&str; 10] = [
    "                              ",
    "                              ",
    "    _________________         ",
    "   _|                \\_____A  ",
    " =|                        |  ",
    " -|                        |  ",
    "__|________________________|_ ",
    "|__________________________|_ ",
    "   |_D__D__D_|  |_D__D__D_|   ",
    "    \\_/   \\_/    \\_/   \\_/    ",
];
/// sl draws the coal car this far along from the engine's front.
const COAL_AT: usize = 53;
/// sl's `D51LENGTH` and `D51HEIGHT`: the whole train, engine and coal car.
const LENGTH: usize = 83;
const HEIGHT: usize = 10;
/// sl's `D51FUNNEL`: the funnel's column, from the engine's front.
const FUNNEL: usize = 7;

/// The whole train in wheel position `frame`, facing left as sl draws it.
fn train(frame: usize) -> Vec<String> {
    D51_BODY
        .iter()
        .chain(&D51_WHEELS[frame])
        .zip(COAL)
        .map(|(engine, coal)| {
            let mut row: String = engine.chars().take(COAL_AT).collect();
            row.push_str(coal);
            row
        })
        .collect()
}

/// Which way a train is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Heading {
    /// sl's way, right to left.
    Left,
    Right,
}

impl Heading {
    fn reversed(self) -> Self {
        match self {
            Heading::Left => Heading::Right,
            Heading::Right => Heading::Left,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Run {
    Waiting {
        remaining: f32,
    },
    /// `x` is the train's left edge and `top` its top row.
    Crossing {
        x: f32,
        top: u16,
        heading: Heading,
    },
}

#[derive(Debug, Clone, Copy)]
struct Puff {
    x: f32,
    y: f32,
    age: f32,
    /// Columns per second, back along the line the train came from.
    drift: f32,
}

pub struct Locomotive {
    rng: SmallRng,
    size: Size,
    run: Run,
    /// Which way the next train goes.
    next: Heading,
    /// The row the last train ran at, so the next runs somewhere else.
    last_top: Option<u16>,
    /// Every wheel frame, facing left and facing right, built once.
    facing_left: Vec<Vec<String>>,
    facing_right: Vec<Vec<String>>,
    smoke: Vec<Puff>,
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
        let facing_left: Vec<Vec<String>> = (0..D51_WHEELS.len()).map(train).collect();
        let facing_right = facing_left.iter().map(|frame| mirror(frame)).collect();
        Self {
            rng,
            size: Size::new(0, 0),
            run: Run::Waiting {
                remaining: FIRST_TRAIN,
            },
            next: Heading::Left,
            last_top: None,
            facing_left,
            facing_right,
            smoke: Vec::new(),
            puff_clock: 0.0,
            was_over: false,
        }
    }

    fn depart(&mut self) {
        let heading = self.next;
        self.next = heading.reversed();
        let top = self.pick_top();
        let x = match heading {
            Heading::Left => f32::from(self.size.width),
            Heading::Right => -(LENGTH as f32),
        };
        self.run = Run::Crossing { x, top, heading };
    }

    /// A random row for the next train, never the last one's while the screen
    /// has room for another.
    fn pick_top(&mut self) -> u16 {
        let lowest = self.size.height.saturating_sub(HEIGHT as u16);
        let top = loop {
            let top = self.rng.gen_range(0..=lowest);
            if lowest == 0 || Some(top) != self.last_top {
                break top;
            }
        };
        self.last_top = Some(top);
        top
    }

    /// The train as it looks at left edge `left`. sl picks the wheel frame by
    /// column, one step back for every column the train moves; mirrored, the
    /// same steps run as it moves right.
    fn frame(&self, left: i32, heading: Heading) -> &[String] {
        let frames = D51_WHEELS.len() as i32;
        let (all, index) = match heading {
            Heading::Left => (&self.facing_left, (LENGTH as i32 + left).rem_euclid(frames)),
            Heading::Right => (
                &self.facing_right,
                (LENGTH as i32 - left).rem_euclid(frames),
            ),
        };
        &all[index as usize]
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

        // The easter egg: a run ending sends the next train now, unless one is
        // already on its way.
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
            Run::Crossing { x, top, heading } => {
                let (x, funnel, drift) = match heading {
                    Heading::Left => (x - SPEED * dt, FUNNEL, SMOKE_DRIFT),
                    Heading::Right => (x + SPEED * dt, LENGTH - 1 - FUNNEL, -SMOKE_DRIFT),
                };
                self.puff_clock += dt;
                if self.puff_clock >= PUFF_EVERY {
                    self.puff_clock -= PUFF_EVERY;
                    self.smoke.push(Puff {
                        x: x + funnel as f32,
                        y: f32::from(top) - 1.0,
                        age: 0.0,
                        drift,
                    });
                }
                let gone = match heading {
                    Heading::Left => x + (LENGTH as f32) < 0.0,
                    Heading::Right => x > f32::from(size.width),
                };
                self.run = if gone {
                    Run::Waiting {
                        remaining: self.rng.gen_range(PAUSE.0..PAUSE.1),
                    }
                } else {
                    Run::Crossing { x, top, heading }
                };
            }
        }

        // Smoke rises, drifts back along the line and thins out.
        for puff in &mut self.smoke {
            puff.age += dt;
            puff.y -= SMOKE_RISE * dt;
            puff.x += puff.drift * dt;
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
            canvas.text(left, puff.y.floor() as i32, glyphs, smoke);
        }

        let Run::Crossing { x, top, heading } = self.run else {
            return;
        };
        let body = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::DIM);
        let left = x.floor() as i32;
        canvas.block(left, top, self.frame(left, heading), body);
    }

    fn name(&self) -> &'static str {
        "Locomotive"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const TICK: Duration = Duration::from_nanos(16_666_667);
    const SIZE: Size = Size::new(100, 30);

    fn run(loco: &mut Locomotive, seconds: f32, signal: &PerformanceSignal) {
        for _ in 0..(seconds * 60.0) as usize {
            loco.tick(TICK, SIZE, signal);
        }
    }

    /// Tick until the current state changes kind: from a crossing to a pause
    /// or back. Returns false if it never did.
    fn run_until_change(loco: &mut Locomotive) -> bool {
        let calm = PerformanceSignal::default();
        let crossing = matches!(loco.run, Run::Crossing { .. });
        for _ in 0..60 * 30 {
            loco.tick(TICK, SIZE, &calm);
            if matches!(loco.run, Run::Crossing { .. }) != crossing {
                return true;
            }
        }
        false
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

    /// sl's own dimensions: `D51LENGTH` by `D51HEIGHT`, every row and every
    /// wheel frame, or the coal car and the rods would come out crooked.
    #[test]
    fn the_train_is_sls_d51_and_coal_car() {
        assert_eq!(D51_WHEELS.len(), 6, "sl's D51PATTERNS");
        for frame in 0..D51_WHEELS.len() {
            let rows = train(frame);
            assert_eq!(rows.len(), HEIGHT);
            for row in &rows {
                assert_eq!(row.chars().count(), LENGTH, "{row:?}");
            }
        }
        // The funnel sl puffs smoke from.
        assert_eq!(&train(0)[0][FUNNEL - 1..FUNNEL + 3], "====");
    }

    #[test]
    fn the_mirrored_train_faces_right() {
        let facing_right = mirror(&train(0));
        // The funnel, at the engine's front, is now on the right.
        assert!(facing_right[0].ends_with("===="));
        // Mirrored rows keep their columns: every one still ends where the
        // left-facing row began, so the train stays in one piece.
        for (left, right) in train(0).iter().zip(&facing_right) {
            let lead = left.len() - left.trim_start().len();
            assert_eq!(right.len(), LENGTH - lead, "{right:?}");
        }
    }

    #[test]
    fn the_first_train_comes_soon_and_runs_sls_way() {
        let mut loco = Locomotive::seeded(1);
        let calm = PerformanceSignal::default();
        run(&mut loco, FIRST_TRAIN + 0.1, &calm);
        let Run::Crossing {
            x: start, heading, ..
        } = loco.run
        else {
            panic!("no train after {FIRST_TRAIN}s");
        };
        assert_eq!(heading, Heading::Left);
        run(&mut loco, 1.0, &calm);
        let Run::Crossing { x, .. } = loco.run else {
            panic!("the train vanished mid-crossing");
        };
        assert!(x < start, "it went the wrong way");

        let rendered = as_string(&loco);
        println!("{rendered}");
        assert!(rendered.contains("===="), "the funnel is on screen");
    }

    /// Across, a short pause, back the other way, and never at the height it
    /// last ran.
    #[test]
    fn trains_go_back_and_forth_each_at_a_new_height() {
        let mut loco = Locomotive::seeded(2);
        let mut last: Option<(Heading, u16)> = None;
        for _ in 0..12 {
            // Into the next crossing.
            assert!(run_until_change(&mut loco), "no train came");
            let Run::Crossing { heading, top, .. } = loco.run else {
                unreachable!()
            };
            assert!(usize::from(top) + HEIGHT <= usize::from(SIZE.height));
            if let Some((previous, previous_top)) = last {
                assert_eq!(heading, previous.reversed(), "it did not turn back");
                assert_ne!(top, previous_top, "the same height twice");
            }
            last = Some((heading, top));

            // Off the far side, and a short pause.
            assert!(run_until_change(&mut loco), "the train never left");
            let Run::Waiting { remaining } = loco.run else {
                unreachable!()
            };
            assert!((PAUSE.0..PAUSE.1).contains(&remaining), "{remaining}");
        }
    }

    #[test]
    fn a_right_bound_train_runs_the_mirrored_art_left_to_right() {
        let mut loco = Locomotive::seeded(3);
        assert!(run_until_change(&mut loco));
        assert!(run_until_change(&mut loco));
        assert!(run_until_change(&mut loco));
        let Run::Crossing {
            x: start, heading, ..
        } = loco.run
        else {
            panic!("no second train");
        };
        assert_eq!(heading, Heading::Right);
        run(&mut loco, 4.0, &PerformanceSignal::default());
        let Run::Crossing { x, .. } = loco.run else {
            panic!("the train vanished mid-crossing");
        };
        assert!(x > start, "it went the wrong way");

        let rendered = as_string(&loco);
        println!("{rendered}");
        // The coal car's front, mirrored, leads on the left.
        assert!(rendered.contains("A_____/"), "the mirrored coal car");
    }

    /// The easter egg: topping out cuts the pause short, and only once.
    #[test]
    fn topping_out_sends_the_next_train_at_once() {
        let mut loco = Locomotive::seeded(4);
        assert!(run_until_change(&mut loco));
        assert!(run_until_change(&mut loco));
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

        // Staying on the game-over screen must not cut the next pause short.
        let crossing = (f32::from(SIZE.width) + LENGTH as f32) / SPEED;
        run(&mut loco, crossing + 0.1, &over);
        assert!(matches!(loco.run, Run::Waiting { .. }));
        loco.tick(TICK, SIZE, &over);
        assert!(matches!(loco.run, Run::Waiting { .. }));
    }

    #[test]
    fn smoke_rises_from_the_funnel_and_drifts_back() {
        let mut loco = Locomotive::seeded(5);
        let calm = PerformanceSignal::default();
        run(&mut loco, FIRST_TRAIN + 1.5, &calm);
        let Run::Crossing { top, .. } = loco.run else {
            panic!("no train");
        };
        assert!(!loco.smoke.is_empty(), "a moving train smokes");
        for puff in &loco.smoke {
            assert!(puff.y < f32::from(top), "smoke below the funnel");
            assert!(puff.drift > 0.0, "a left-bound train's smoke drifts right");
        }

        // Off the far side; by the time its last puff would have thinned out,
        // none of its smoke is left, whatever the next train is doing.
        assert!(run_until_change(&mut loco));
        run(&mut loco, PUFF.len() as f32 * PUFF_STAGE, &calm);
        assert!(
            loco.smoke.iter().all(|puff| puff.drift < 0.0),
            "the smoke never cleared"
        );
    }

    /// sl steps the wheels a frame for every column the train moves.
    #[test]
    fn the_wheels_turn_a_frame_a_column() {
        let loco = Locomotive::seeded(6);
        for heading in [Heading::Left, Heading::Right] {
            let frames: Vec<&[String]> = (0..6).map(|left| loco.frame(left, heading)).collect();
            for pair in frames.windows(2) {
                assert_ne!(pair[0], pair[1], "{heading:?}");
            }
            assert_eq!(loco.frame(0, heading), loco.frame(6, heading));
        }
    }
}
