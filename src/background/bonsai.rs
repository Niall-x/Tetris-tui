//! Bonsai (§8, #6): a tree grown branch by branch, cbonsai-style.
//!
//! The growth rules are cbonsai's own — its trunk, shoot, dying and dead branch
//! types, their movement dice and their glyphs — so the trees have that shape
//! rather than a lookalike's. cbonsai grows a tree by recursion, drawing as it
//! goes; here the recursion is an explicit stack, so the tree can grow a step
//! per tick instead of all at once.
//!
//! A tall piece of art centred behind the board would be hidden by it, so the
//! tree stands in the widest stretch of screen the board leaves free. Once grown
//! it stays a while, then a new one is planted.

use std::collections::HashMap;
use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};

use super::{Background, Canvas, PerformanceSignal};
use crate::ui::style::Visuals;

/// cbonsai's default starting life and branching multiplier.
const LIFE: i32 = 32;
const MULTIPLIER: i32 = 5;
/// Growth steps per second; cbonsai's live mode is about this pace.
const STEPS_PER_SECOND: f32 = 30.0;
/// Seconds a finished tree stands before the next is planted.
const GROWN_FOR: f32 = 45.0;
/// A backstop on the growth of any one tree. cbonsai's rules always finish well
/// short of this, but nothing in them strictly guarantees it.
const MAX_STEPS: u32 = 10_000;

/// cbonsai's second base. The tree grows from the middle of its rim.
const POT: [&str; 3] = ["(---./~~~\\.---)", " (           ) ", "  (_________)  "];
const POT_CENTRE: u16 = 7;
/// Rows above the pot that have to be clear for a spot to count as free.
const STANDING_ROOM: u16 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Trunk,
    ShootLeft,
    ShootRight,
    Dying,
    Dead,
}

/// One branch still growing: a frame of cbonsai's recursion.
#[derive(Debug, Clone, Copy)]
struct Branch {
    /// Relative to the root; `y` grows downward, so the tree is at `y <= 0`.
    x: i32,
    y: i32,
    kind: Kind,
    life: i32,
    shoot_cooldown: i32,
}

impl Branch {
    fn new(x: i32, y: i32, kind: Kind, life: i32) -> Self {
        Self {
            x,
            y,
            kind,
            life,
            shoot_cooldown: MULTIPLIER,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Wood,
    Leaf,
}

#[derive(Debug, Clone, Copy)]
struct Cell {
    glyph: char,
    part: Part,
    bright: bool,
}

pub struct Bonsai {
    rng: SmallRng,
    /// Branches still growing, innermost last — the recursion's call stack.
    growing: Vec<Branch>,
    cells: HashMap<(i32, i32), Cell>,
    /// cbonsai alternates new shoots left and right from a random start.
    shoots: u32,
    steps: u32,
    /// Steps owed but not yet taken; carries the fraction between ticks.
    pending: f32,
    /// Seconds since the tree finished growing.
    grown: f32,
}

impl Bonsai {
    pub fn new() -> Self {
        Self::with_rng(SmallRng::from_entropy())
    }

    #[cfg(test)]
    fn seeded(seed: u64) -> Self {
        Self::with_rng(SmallRng::seed_from_u64(seed))
    }

    fn with_rng(rng: SmallRng) -> Self {
        let mut bonsai = Self {
            rng,
            growing: Vec::new(),
            cells: HashMap::new(),
            shoots: 0,
            steps: 0,
            pending: 0.0,
            grown: 0.0,
        };
        bonsai.plant();
        bonsai
    }

    fn plant(&mut self) {
        self.cells.clear();
        self.growing = vec![Branch::new(0, 0, Kind::Trunk, LIFE)];
        self.shoots = self.rng.gen();
        self.steps = 0;
        self.grown = 0.0;
    }

    fn is_growing(&self) -> bool {
        !self.growing.is_empty()
    }

    /// One pass of cbonsai's `branch()` loop for the innermost growing branch:
    /// maybe sprout a child, move, draw. Returns false once the tree is done.
    fn step(&mut self) -> bool {
        while self.growing.last().is_some_and(|branch| branch.life <= 0) {
            self.growing.pop();
        }
        if self.steps >= MAX_STEPS {
            self.growing.clear();
        }
        let Some(&branch) = self.growing.last() else {
            return false;
        };
        self.steps += 1;

        let mut branch = branch;
        branch.life -= 1;
        let age = LIFE - branch.life;
        let (dx, mut dy) = self.deltas(branch.kind, branch.life, age);
        // Never down through the ground.
        if dy > 0 && branch.y >= 0 {
            dy -= 1;
        }

        let (x, y, life) = (branch.x, branch.y, branch.life);
        let child = if life < 3 {
            // A near-dead branch bursts into leaves.
            Some(Branch::new(x, y, Kind::Dead, life))
        } else if branch.kind != Kind::Dying && branch.kind != Kind::Dead && life < MULTIPLIER + 2 {
            // So does a trunk or shoot coming to its end.
            Some(Branch::new(x, y, Kind::Dying, life))
        } else if branch.kind == Kind::Trunk
            && (self.rng.gen_range(0..3) == 0 || life % MULTIPLIER == 0)
        {
            if self.rng.gen_range(0..8) == 0 && life > 7 {
                // Now and then the trunk forks.
                branch.shoot_cooldown = MULTIPLIER * 2;
                let fork_life = life + self.rng.gen_range(-2..=2);
                Some(Branch::new(x, y, Kind::Trunk, fork_life))
            } else if branch.shoot_cooldown <= 0 {
                branch.shoot_cooldown = MULTIPLIER * 2;
                self.shoots = self.shoots.wrapping_add(1);
                let kind = if self.shoots % 2 == 1 {
                    Kind::ShootLeft
                } else {
                    Kind::ShootRight
                };
                Some(Branch::new(x, y, kind, life + MULTIPLIER))
            } else {
                None
            }
        } else {
            None
        };
        branch.shoot_cooldown -= 1;

        branch.x += dx;
        branch.y += dy;
        self.draw(branch, dx, dy);

        if let Some(top) = self.growing.last_mut() {
            *top = branch;
        }
        // Pushed last, so it grows to the end before its parent resumes — the
        // order cbonsai's recursion gives.
        if let Some(child) = child {
            self.growing.push(child);
        }
        true
    }

    /// cbonsai's `setDeltas`: how far a branch of this kind moves this step.
    fn deltas(&mut self, kind: Kind, life: i32, age: i32) -> (i32, i32) {
        let rng = &mut self.rng;
        let mut dice = |sides: i32| rng.gen_range(0..sides);
        match kind {
            Kind::Trunk if age <= 2 || life < 4 => (dice(3) - 1, 0),
            // A young trunk spreads wide, rising only every other step.
            Kind::Trunk if age < MULTIPLIER * 3 => {
                let dy = if age % (MULTIPLIER / 2) == 0 { -1 } else { 0 };
                let dx = match dice(10) {
                    0 => -2,
                    1..=3 => -1,
                    4..=5 => 0,
                    6..=8 => 1,
                    _ => 2,
                };
                (dx, dy)
            }
            Kind::Trunk => {
                let dy = if dice(10) > 2 { -1 } else { 0 };
                (dice(3) - 1, dy)
            }
            Kind::ShootLeft | Kind::ShootRight => {
                let dy = match dice(10) {
                    0..=1 => -1,
                    2..=7 => 0,
                    _ => 1,
                };
                let dx = match dice(10) {
                    0..=1 => -2,
                    2..=5 => -1,
                    6..=8 => 0,
                    _ => 1,
                };
                let dx = if kind == Kind::ShootRight { -dx } else { dx };
                (dx, dy)
            }
            Kind::Dying => {
                let dy = match dice(10) {
                    0..=1 => -1,
                    2..=8 => 0,
                    _ => 1,
                };
                let dx = match dice(15) {
                    0 => -3,
                    1..=2 => -2,
                    3..=5 => -1,
                    6..=8 => 0,
                    9..=11 => 1,
                    12..=13 => 2,
                    _ => 3,
                };
                (dx, dy)
            }
            Kind::Dead => {
                let dy = match dice(10) {
                    0..=2 => -1,
                    3..=6 => 0,
                    _ => 1,
                };
                (dice(3) - 1, dy)
            }
        }
    }

    /// cbonsai's `chooseString` and `chooseColor`. A branch in its last few
    /// steps draws leaves whatever it is, but keeps its own colour, so trunk
    /// tips come out as brown leaves — cbonsai does the same.
    fn draw(&mut self, branch: Branch, dx: i32, dy: i32) {
        let glyphs = if branch.life < 4 {
            "&"
        } else {
            match branch.kind {
                Kind::Trunk if dy == 0 => "/~",
                Kind::Trunk if dx < 0 => "\\|",
                Kind::Trunk if dx == 0 => "/|\\",
                Kind::Trunk => "|/",
                Kind::ShootLeft if dy > 0 => "\\",
                Kind::ShootLeft if dy == 0 => "\\_",
                Kind::ShootRight if dy > 0 => "/",
                Kind::ShootRight if dy == 0 => "_/",
                Kind::ShootLeft | Kind::ShootRight if dx < 0 => "\\|",
                Kind::ShootLeft | Kind::ShootRight if dx == 0 => "/|",
                Kind::ShootLeft | Kind::ShootRight => "/",
                Kind::Dying | Kind::Dead => "&",
            }
        };

        let (part, bright) = match branch.kind {
            Kind::Trunk | Kind::ShootLeft | Kind::ShootRight => {
                (Part::Wood, self.rng.gen_range(0..2) == 0)
            }
            Kind::Dying => (Part::Leaf, self.rng.gen_range(0..10) == 0),
            Kind::Dead => (Part::Leaf, self.rng.gen_range(0..3) == 0),
        };

        for (offset, glyph) in glyphs.chars().enumerate() {
            let cell = Cell {
                glyph,
                part,
                bright,
            };
            self.cells
                .insert((branch.x + offset as i32, branch.y), cell);
        }
    }
}

impl Default for Bonsai {
    fn default() -> Self {
        Self::new()
    }
}

impl Background for Bonsai {
    /// The tree is grown in its own coordinates, so the terminal's size only
    /// matters when it is placed on screen.
    fn tick(&mut self, dt: Duration, _size: Size, _signal: &PerformanceSignal) {
        let dt = dt.as_secs_f32();
        if self.is_growing() {
            self.pending += dt * STEPS_PER_SECOND;
            while self.pending >= 1.0 {
                self.pending -= 1.0;
                if !self.step() {
                    self.pending = 0.0;
                    break;
                }
            }
        } else {
            self.grown += dt;
            if self.grown >= GROWN_FOR {
                self.plant();
            }
        }
    }

    fn render(&self, canvas: &mut Canvas, _visuals: &Visuals, _signal: &PerformanceSignal) {
        let pot_height = POT.len() as u16;
        if canvas.height() <= pot_height {
            return;
        }
        // Placed by the space free at the pot and the tree's lower half; the
        // crown may reach up behind a panel, but the tree still reads.
        let pot_top = canvas.height() - pot_height;
        let (start, width) = canvas.widest_free_span(pot_top.saturating_sub(STANDING_ROOM));
        if width == 0 {
            return;
        }

        let centre = start + width / 2;
        let root = (i32::from(centre), i32::from(pot_top) - 1);

        let pot_style = Style::default().fg(Color::Gray).add_modifier(Modifier::DIM);
        canvas.block(centre.saturating_sub(POT_CENTRE), pot_top, &POT, pot_style);

        for (&(x, y), cell) in &self.cells {
            let (x, y) = (root.0 + x, root.1 + y);
            let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let colour = match (cell.part, cell.bright) {
                (Part::Wood, false) => Color::Yellow,
                (Part::Wood, true) => Color::LightYellow,
                (Part::Leaf, false) => Color::Green,
                (Part::Leaf, true) => Color::LightGreen,
            };
            let style = Style::default().fg(colour).add_modifier(Modifier::DIM);
            canvas.put(x, y, cell.glyph, style);
        }
    }

    fn name(&self) -> &'static str {
        "Bonsai"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const TICK: Duration = Duration::from_nanos(16_666_667);
    const SIZE: Size = Size::new(80, 30);

    fn grow_fully(bonsai: &mut Bonsai) {
        while bonsai.step() {}
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

    fn draw(bonsai: &Bonsai, width: u16, height: u16, reserved: &[Rect]) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let mut canvas = Canvas::new(&mut buf, area, reserved);
        bonsai.render(
            &mut canvas,
            &Visuals::default(),
            &PerformanceSignal::default(),
        );
        buf
    }

    #[test]
    fn a_tree_grows_a_step_at_a_time() {
        let mut bonsai = Bonsai::seeded(1);
        let mut sizes = Vec::new();
        for _ in 0..4 {
            for _ in 0..30 {
                bonsai.tick(TICK, SIZE, &PerformanceSignal::default());
            }
            sizes.push(bonsai.cells.len());
        }
        assert!(
            sizes.windows(2).all(|pair| pair[0] < pair[1]),
            "the tree should keep growing: {sizes:?}"
        );
    }

    /// Every seed has to finish, with both wood and leaves, and never grow down
    /// into the pot.
    #[test]
    fn every_tree_finishes_with_wood_and_leaves_above_the_ground() {
        for seed in 0..40 {
            let mut bonsai = Bonsai::seeded(seed);
            grow_fully(&mut bonsai);
            assert!(!bonsai.is_growing());
            assert!(bonsai.steps < MAX_STEPS, "seed {seed} hit the backstop");

            let parts: Vec<Part> = bonsai.cells.values().map(|cell| cell.part).collect();
            assert!(parts.contains(&Part::Wood), "seed {seed}: no wood");
            assert!(parts.contains(&Part::Leaf), "seed {seed}: no leaves");
            assert!(
                bonsai.cells.keys().all(|&(_, y)| y <= 0),
                "seed {seed} grew below the root"
            );
        }
    }

    #[test]
    fn a_grown_tree_stands_a_while_and_then_a_new_one_is_planted() {
        let mut bonsai = Bonsai::seeded(2);
        grow_fully(&mut bonsai);
        let grown = bonsai.cells.len();

        for _ in 0..60 {
            bonsai.tick(TICK, SIZE, &PerformanceSignal::default());
        }
        assert_eq!(bonsai.cells.len(), grown, "still standing a second later");

        for _ in 0..(GROWN_FOR as usize * 60) {
            bonsai.tick(TICK, SIZE, &PerformanceSignal::default());
        }
        assert!(bonsai.cells.len() < grown, "replanted and growing again");
    }

    #[test]
    fn a_grown_tree_is_drawn_in_its_pot() {
        let mut bonsai = Bonsai::seeded(3);
        grow_fully(&mut bonsai);
        let rendered = as_string(&draw(&bonsai, 80, 30, &[]));
        println!("{rendered}");
        assert!(rendered.contains("(_________)"), "the pot");
        assert!(rendered.contains('&'), "leaves");
    }

    /// With the board in the middle, the tree belongs in whichever margin is
    /// wider — centred behind the board it would never be seen.
    #[test]
    fn the_tree_stands_in_the_widest_free_margin() {
        let mut bonsai = Bonsai::seeded(4);
        grow_fully(&mut bonsai);
        // Board on the left: 0..10 free, 40..100 free.
        let board = Rect::new(10, 0, 30, 30);
        let buf = draw(&bonsai, 100, 30, &[board]);
        println!("{}", as_string(&buf));

        let pot_row = 29;
        let pot: Vec<u16> = (0..100)
            .filter(|&x| buf[(x, pot_row)].symbol() != " ")
            .collect();
        assert!(!pot.is_empty());
        assert!(
            pot.iter().all(|&x| x >= 40),
            "pot drawn at {pot:?}, not in the right margin"
        );
    }

    #[test]
    fn short_or_fully_covered_terminals_draw_nothing_and_survive() {
        let mut bonsai = Bonsai::seeded(5);
        grow_fully(&mut bonsai);
        draw(&bonsai, 80, 2, &[]);
        draw(&bonsai, 0, 0, &[]);
        let everything = Rect::new(0, 0, 20, 20);
        let buf = draw(&bonsai, 20, 20, &[everything]);
        assert!(buf.content().iter().all(|cell| cell.symbol() == " "));
    }
}
