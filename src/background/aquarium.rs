//! Aquarium (§8, #3): asciiquarium's fish tank, reimplemented natively.
//!
//! A rippling surface along the top, seaweed swaying along the bottom, and fish
//! of a few sizes swimming across in both directions at their own speeds, now
//! and then breathing out a bubble that rises and pops at the surface. When a
//! fish swims off one side another is soon along.
//!
//! Every sprite is authored facing right; the left-facing one is its mirror
//! image, so each fish only has to be drawn once.

use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};

use super::{mirror, Background, Canvas, PerformanceSignal};
use crate::engine::piece::PieceKind;
use crate::ui::style::Visuals;

/// Rows of water surface at the top of the tank.
const SURFACE: [&str; 4] = [
    "~~~~~~~~~~~~~~~~",
    "^^^^ ^^^  ^^^   ",
    "^^^^      ^^^^  ",
    "^^    ^^^^^     ",
];
/// Columns per second the ripples under the top line drift, alternately left
/// and right by row.
const RIPPLE_SPEED: f32 = 1.5;

/// Fish, all facing right. asciiquarium's own, trimmed to the smaller ones:
/// behind a board there is no room for its whales.
const FISH: [&[&str]; 5] = [
    &["><>"],
    &["><(((('>"],
    &[" __", "\\/ o\\", "/\\__/"],
    &["   \\", "\\ /--\\", " >= o >", "/ \\__/", "   /"],
    &["  ,", "\\}\\", " \\  .\\", " /  '/", "/ }/", "  '"],
];
/// Columns per second, slowest to fastest.
const FISH_SPEED: (f32, f32) = (2.5, 9.0);
/// Screen cells per fish in the tank.
const CELLS_PER_FISH: usize = 300;
/// Seconds between one fish leaving and the next being let in.
const RESTOCK: (f32, f32) = (0.3, 2.5);
/// Seconds between one fish's bubbles.
const BREATH: (f32, f32) = (1.5, 6.0);

/// Rows per second a bubble rises.
const BUBBLE_SPEED: f32 = 3.0;
/// A bubble grows as it rises: this many rows per stage.
const BUBBLE_GROWTH: f32 = 2.0;
const BUBBLE: [char; 3] = ['.', 'o', 'O'];

/// Columns of floor per stalk of seaweed.
const COLUMNS_PER_WEED: u16 = 12;
/// Seconds per sway of the seaweed.
const SWAY: f32 = 0.7;

fn sprite_width<S: AsRef<str>>(sprite: &[S]) -> usize {
    sprite
        .iter()
        .map(|line| line.as_ref().chars().count())
        .max()
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
struct Fish {
    /// Left edge; fractional so slow fish move smoothly on average.
    x: f32,
    y: u16,
    /// Columns per second: positive swims right, negative left.
    speed: f32,
    sprite: usize,
    colour: PieceKind,
    /// Seconds to its next bubble.
    breath: f32,
}

impl Fish {
    fn facing_right(&self) -> bool {
        self.speed > 0.0
    }
}

#[derive(Debug, Clone, Copy)]
struct Bubble {
    x: u16,
    y: f32,
    /// Where it started, which decides how big it has grown.
    from: f32,
}

#[derive(Debug, Clone, Copy)]
struct Weed {
    x: u16,
    height: u16,
    /// Which way it leans at the start, so neighbouring stalks do not sway in
    /// lockstep.
    phase: bool,
}

pub struct Aquarium {
    rng: SmallRng,
    size: Size,
    /// Each sprite facing right, then facing left.
    sprites: Vec<[Vec<String>; 2]>,
    fish: Vec<Fish>,
    bubbles: Vec<Bubble>,
    weeds: Vec<Weed>,
    restock: f32,
    /// Seconds since the tank was set up, which drives the ripple and sway.
    clock: f32,
}

impl Aquarium {
    pub fn new() -> Self {
        Self::with_rng(SmallRng::from_entropy())
    }

    #[cfg(test)]
    fn seeded(seed: u64) -> Self {
        Self::with_rng(SmallRng::seed_from_u64(seed))
    }

    fn with_rng(rng: SmallRng) -> Self {
        let sprites = FISH
            .iter()
            .map(|sprite| {
                let right = sprite.iter().map(|line| line.to_string()).collect();
                [right, mirror(sprite)]
            })
            .collect();
        Self {
            rng,
            size: Size::new(0, 0),
            sprites,
            fish: Vec::new(),
            bubbles: Vec::new(),
            weeds: Vec::new(),
            restock: 0.0,
            clock: 0.0,
        }
    }

    /// Replant the floor and clear out anything the new size strands. Fish are
    /// kept where they still fit, so a resize does not empty the tank.
    fn resize(&mut self, size: Size) {
        self.size = size;

        self.weeds.clear();
        let tallest = (size.height / 3).clamp(1, 8);
        for _ in 0..size.width / COLUMNS_PER_WEED {
            let weed = Weed {
                x: self.rng.gen_range(0..size.width.saturating_sub(1).max(1)),
                height: self.rng.gen_range(1..=tallest),
                phase: self.rng.gen(),
            };
            self.weeds.push(weed);
        }

        let sprites = &self.sprites;
        self.fish.retain(|fish| {
            let height = sprites[fish.sprite][0].len() as u16;
            fish.y + height <= size.height
        });
        self.bubbles
            .retain(|bubble| bubble.x < size.width && bubble.y < f32::from(size.height));
    }

    fn capacity(&self) -> usize {
        self.size.width as usize * self.size.height as usize / CELLS_PER_FISH
    }

    /// Let a new fish in at one edge, at a depth it fits, or nothing if the tank
    /// is too shallow for any of them.
    fn spawn(&mut self) {
        let sprite = self.rng.gen_range(0..self.sprites.len());
        let [right, _] = &self.sprites[sprite];
        let (width, height) = (sprite_width(right) as f32, right.len() as u16);

        // Below the surface, and clear of the bottom row.
        let top = SURFACE.len() as u16;
        let Some(bottom) = self.size.height.checked_sub(height + 1) else {
            return;
        };
        if bottom < top {
            return;
        }

        let speed = self.rng.gen_range(FISH_SPEED.0..FISH_SPEED.1);
        let (x, speed) = if self.rng.gen() {
            (-width, speed)
        } else {
            (f32::from(self.size.width), -speed)
        };
        let fish = Fish {
            x,
            y: self.rng.gen_range(top..=bottom),
            speed,
            sprite,
            colour: PieceKind::ALL[self.rng.gen_range(0..PieceKind::ALL.len())],
            breath: self.rng.gen_range(BREATH.0..BREATH.1),
        };
        self.fish.push(fish);
    }

    fn sprite(&self, fish: &Fish) -> &[String] {
        let facing = if fish.facing_right() { 0 } else { 1 };
        &self.sprites[fish.sprite][facing]
    }
}

impl Default for Aquarium {
    fn default() -> Self {
        Self::new()
    }
}

impl Background for Aquarium {
    fn tick(&mut self, dt: Duration, size: Size, _signal: &PerformanceSignal) {
        if size != self.size {
            self.resize(size);
        }
        let dt = dt.as_secs_f32();
        self.clock += dt;

        let width = f32::from(size.width);
        let mut exhaled = Vec::new();
        for index in 0..self.fish.len() {
            let sprite_width = sprite_width(self.sprite(&self.fish[index])) as f32;
            let height = self.sprite(&self.fish[index]).len() as u16;
            let fish = &mut self.fish[index];
            fish.x += fish.speed * dt;
            fish.breath -= dt;
            if fish.breath <= 0.0 {
                fish.breath = self.rng.gen_range(BREATH.0..BREATH.1);
                // From the mouth, which is whichever end it is swimming towards.
                let mouth = if fish.facing_right() {
                    fish.x + sprite_width
                } else {
                    fish.x - 1.0
                };
                if (0.0..width).contains(&mouth) {
                    let y = f32::from(fish.y + height / 2);
                    exhaled.push(Bubble {
                        x: mouth as u16,
                        y,
                        from: y,
                    });
                }
            }
        }
        self.bubbles.extend(exhaled);

        // Out of the tank on the far side.
        let sprites = &self.sprites;
        let before = self.fish.len();
        self.fish.retain(|fish| {
            let sprite_width = sprite_width(&sprites[fish.sprite][0]) as f32;
            fish.x > -sprite_width - 1.0 && fish.x < width + 1.0
        });
        if self.fish.len() < before {
            self.restock = self.rng.gen_range(RESTOCK.0..RESTOCK.1);
        }

        self.restock -= dt;
        if self.restock <= 0.0 && self.fish.len() < self.capacity() {
            self.spawn();
            self.restock = self.rng.gen_range(RESTOCK.0..RESTOCK.1);
        }

        // Bubbles pop as they reach the water's surface.
        let surface = SURFACE.len() as f32;
        for bubble in &mut self.bubbles {
            bubble.y -= BUBBLE_SPEED * dt;
        }
        self.bubbles.retain(|bubble| bubble.y >= surface);
    }

    fn render(&self, canvas: &mut Canvas, visuals: &Visuals, _signal: &PerformanceSignal) {
        let width = canvas.width();

        // The surface: a still top line, and ripples beneath it drifting
        // alternately left and right.
        let water = Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM);
        let drift = (self.clock * RIPPLE_SPEED) as usize;
        for (row, pattern) in SURFACE.iter().enumerate() {
            let pattern: Vec<char> = pattern.chars().collect();
            let len = pattern.len();
            let shift = match row {
                0 => 0,
                r if r % 2 == 1 => drift % len,
                _ => len - drift % len,
            };
            for x in 0..width {
                let ch = pattern[(x as usize + shift) % len];
                if ch != ' ' {
                    canvas.put(x, row as u16, ch, water);
                }
            }
        }

        // Seaweed along the floor, each stalk a zigzag of parentheses whose
        // lean flips with the sway.
        let weed = Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::DIM);
        let sway = (self.clock / SWAY) as u64 % 2 == 1;
        for stalk in &self.weeds {
            for level in 0..stalk.height {
                let Some(y) = canvas.height().checked_sub(level + 1) else {
                    break;
                };
                let leans_left = (level % 2 == 0) ^ sway ^ stalk.phase;
                let (x, ch) = if leans_left {
                    (stalk.x, '(')
                } else {
                    (stalk.x + 1, ')')
                };
                canvas.put(x, y, ch, weed);
            }
        }

        let bubble_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM);
        for bubble in &self.bubbles {
            let stage = ((bubble.from - bubble.y) / BUBBLE_GROWTH) as usize;
            let ch = BUBBLE[stage.min(BUBBLE.len() - 1)];
            canvas.put(bubble.x, bubble.y as u16, ch, bubble_style);
        }

        for fish in &self.fish {
            let style = Style::default()
                .fg(visuals.theme.color(fish.colour))
                .add_modifier(Modifier::DIM);
            canvas.block(fish.x.floor() as i32, fish.y, self.sprite(fish), style);
        }
    }

    fn name(&self) -> &'static str {
        "Aquarium"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const TICK: Duration = Duration::from_nanos(16_666_667);
    const SIZE: Size = Size::new(80, 30);

    fn run(tank: &mut Aquarium, seconds: usize, size: Size) {
        for _ in 0..seconds * 60 {
            tank.tick(TICK, size, &PerformanceSignal::default());
        }
    }

    fn as_string(tank: &Aquarium) -> String {
        let area = Rect::new(0, 0, tank.size.width, tank.size.height);
        let mut buf = Buffer::empty(area);
        let mut canvas = Canvas::new(&mut buf, area, &[]);
        tank.render(
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

    #[test]
    fn every_sprite_keeps_its_shape_when_mirrored() {
        for sprite in FISH {
            let flipped = mirror(sprite);
            assert_eq!(flipped.len(), sprite.len());
            assert!(sprite_width(&flipped) <= sprite_width(sprite));
            let original: Vec<String> = sprite.iter().map(|l| l.trim_end().to_string()).collect();
            assert_eq!(
                mirror(&flipped),
                original,
                "mirroring twice is the identity"
            );
        }
    }

    #[test]
    fn the_tank_fills_with_fish_that_swim_their_own_way() {
        let mut tank = Aquarium::seeded(1);
        run(&mut tank, 20, SIZE);
        assert!(!tank.fish.is_empty());
        assert!(tank.fish.len() <= tank.capacity());

        let before: Vec<(f32, f32)> = tank.fish.iter().map(|f| (f.x, f.speed)).collect();
        tank.tick(TICK, SIZE, &PerformanceSignal::default());
        for ((x, speed), fish) in before.iter().zip(&tank.fish) {
            assert_eq!(fish.x > *x, *speed > 0.0, "a fish swam backwards");
        }
    }

    #[test]
    fn fish_leave_and_others_take_their_place() {
        let mut tank = Aquarium::seeded(2);
        run(&mut tank, 10, SIZE);
        let first: Vec<u16> = tank.fish.iter().map(|f| f.y).collect();
        // Long enough for even the slowest to cross an 80-column tank.
        run(&mut tank, 60, SIZE);
        assert!(!tank.fish.is_empty(), "the tank was never restocked");
        let now: Vec<u16> = tank.fish.iter().map(|f| f.y).collect();
        assert_ne!(first, now);
    }

    #[test]
    fn fish_stay_under_the_surface_and_off_the_floor() {
        let mut tank = Aquarium::seeded(3);
        for _ in 0..60 * 60 {
            tank.tick(TICK, SIZE, &PerformanceSignal::default());
            for fish in &tank.fish {
                let height = tank.sprite(fish).len() as u16;
                assert!(fish.y >= SURFACE.len() as u16);
                assert!(fish.y + height < SIZE.height);
            }
        }
    }

    #[test]
    fn bubbles_rise_and_pop_at_the_surface() {
        let mut tank = Aquarium::seeded(4);
        tank.tick(TICK, SIZE, &PerformanceSignal::default());
        tank.bubbles.push(Bubble {
            x: 10,
            y: 20.0,
            from: 20.0,
        });
        let mut last = 20.0;
        let mut ticks = 0;
        while let Some(bubble) = tank.bubbles.iter().find(|b| b.x == 10 && b.from == 20.0) {
            assert!(bubble.y <= last, "a bubble sank");
            assert!(bubble.y >= SURFACE.len() as f32, "above the surface");
            last = bubble.y;
            tank.tick(TICK, SIZE, &PerformanceSignal::default());
            ticks += 1;
            assert!(ticks < 60 * 20, "the bubble never popped");
        }
    }

    #[test]
    fn a_frame_has_surface_seaweed_and_fish() {
        let mut tank = Aquarium::seeded(5);
        run(&mut tank, 15, SIZE);
        let rendered = as_string(&tank);
        println!("{rendered}");
        let rows: Vec<&str> = rendered.lines().collect();
        assert!(rows[0].starts_with("~~~~"), "the surface");
        assert!(
            rows[SIZE.height as usize - 1].contains(['(', ')']),
            "seaweed"
        );
        assert!(tank.fish.iter().any(|f| (0.0..80.0).contains(&f.x)));
    }

    #[test]
    fn a_resize_keeps_everything_in_the_new_tank() {
        let mut tank = Aquarium::seeded(6);
        run(&mut tank, 20, Size::new(150, 50));
        let small = Size::new(40, 12);
        tank.tick(TICK, small, &PerformanceSignal::default());
        for fish in &tank.fish {
            assert!(fish.y + tank.sprite(fish).len() as u16 <= small.height);
        }
        assert!(tank.weeds.iter().all(|w| w.x < small.width));
        run(&mut tank, 5, small);
        as_string(&tank);
    }
}
