//! Cowsay mood (§8.1): the one background that reacts to how the run is going.
//!
//! A cow stands at the foot of the screen and comments. What it says, and its
//! face, follow the performance signal — a celebration for a Tetris or a T-spin
//! clear, encouragement through a combo, smugness on back-to-back, alarm when
//! the stack nears the top, a last word at game over, and idle remarks
//! otherwise.
//!
//! Signals can flip every frame, and a cow that changed its mind as often would
//! just flicker, so a mood has to last a minimum time before another replaces
//! it. The exceptions are a celebration and game over, which are events and
//! land at once. The cow is authored here, not borrowed from `cowsay`.

use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};

use super::{Background, Canvas, ClearKind, PerformanceSignal};
use crate::engine::piece::PieceKind;
use crate::ui::style::Visuals;

/// Seconds a mood holds before an ordinary change of signal can replace it.
const DWELL: f32 = 1.5;
/// Seconds a celebration lasts.
const CELEBRATE_FOR: f32 = 3.0;
/// Seconds between idle remarks.
const IDLE_ROTATE: f32 = 8.0;
/// Stack height, as a fraction of the field, at which the cow gets nervous.
const PANIC_AT: f32 = 0.8;
/// Consecutive clearing placements before a combo is worth remarking on.
const STREAK_AT: u32 = 3;
/// Widest the speech bubble's text runs before wrapping.
const BUBBLE_TEXT: usize = 28;

/// The cow, with `{e}` for its eyes and `{t}` for its tongue.
const COW: [&str; 5] = [
    "        \\   ^__^",
    "         \\  ({e})\\_______",
    "            (__)\\       )\\/\\",
    "             {t}  ||----w |",
    "                ||     ||",
];
const COW_WIDTH: usize = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mood {
    Idle,
    Tetris,
    TSpin,
    Streak,
    Smug,
    Panic,
    GameOver,
}

impl Mood {
    /// Celebrations and game over are events rather than states: they take
    /// over at once instead of waiting out the current mood.
    fn is_urgent(self) -> bool {
        matches!(self, Mood::Tetris | Mood::TSpin | Mood::GameOver)
    }

    /// cowsay's own faces: the plain cow, `-w` wired, `-p` paranoid, `-b` borg
    /// shades, `-d` dead.
    fn eyes(self) -> &'static str {
        match self {
            Mood::Idle => "oo",
            Mood::Tetris | Mood::TSpin => "^^",
            Mood::Streak => "OO",
            Mood::Smug => "==",
            Mood::Panic => "@@",
            Mood::GameOver => "xx",
        }
    }

    fn tongue(self) -> &'static str {
        match self {
            Mood::Tetris | Mood::TSpin | Mood::GameOver => "U",
            _ => " ",
        }
    }

    fn colour(self, visuals: &Visuals) -> Color {
        match self {
            Mood::Idle => Color::White,
            // The pieces that earned it.
            Mood::Tetris => visuals.theme.color(PieceKind::I),
            Mood::TSpin => visuals.theme.color(PieceKind::T),
            Mood::Streak => Color::Yellow,
            Mood::Smug => Color::LightMagenta,
            Mood::Panic | Mood::GameOver => Color::LightRed,
        }
    }

    /// What it might say. `{n}` is the current combo.
    fn remarks(self) -> &'static [&'static str] {
        match self {
            Mood::Idle => &[
                "moo.",
                "nice day for stacking.",
                "i could watch this all day.",
                "take your time. i'm a cow.",
                "grass is fine. blocks are better.",
                "chew, chew, stack, stack.",
                "that I piece will come. probably.",
            ],
            Mood::Tetris => &[
                "TETRIS! moooo!",
                "four lines! udderly brilliant.",
                "now THAT is a clear.",
                "the long one! finally!",
            ],
            Mood::TSpin => &[
                "a T-spin! show-off.",
                "spun it right in. nice.",
                "twist and shout!",
            ],
            Mood::Streak => &[
                "{n} in a row! keep going!",
                "combo {n}! don't stop now.",
                "{n} straight. i'm impressed.",
            ],
            Mood::Smug => &[
                "back-to-back. naturally.",
                "we make this look easy.",
                "b2b. the cow approves.",
            ],
            Mood::Panic => &[
                "it's getting tall in here...",
                "the top! mind the top!",
                "i can see my barn from up here.",
                "breathe. find the gap.",
            ],
            Mood::GameOver => &[
                "game over. moo.",
                "well. that happened.",
                "the stack wins this round.",
                "again? i'll wait.",
            ],
        }
    }
}

pub struct Cow {
    rng: SmallRng,
    mood: Mood,
    remark: String,
    /// Seconds in the current mood.
    held: f32,
    /// A celebration still running, and for how much longer.
    celebrating: Option<(Mood, f32)>,
    /// The placement count last seen, so a new placement can be told from the
    /// same one still being reported.
    seen: u64,
}

impl Cow {
    pub fn new() -> Self {
        Self::with_rng(SmallRng::from_entropy())
    }

    #[cfg(test)]
    fn seeded(seed: u64) -> Self {
        Self::with_rng(SmallRng::seed_from_u64(seed))
    }

    fn with_rng(rng: SmallRng) -> Self {
        let mut cow = Self {
            rng,
            mood: Mood::Idle,
            remark: String::new(),
            held: 0.0,
            celebrating: None,
            seen: 0,
        };
        cow.switch(Mood::Idle, 0);
        cow
    }

    /// Change mood and pick something to say, never the same remark twice in a
    /// row when there is another to choose.
    fn switch(&mut self, mood: Mood, combo: u32) {
        let remarks = mood.remarks();
        let mut remark = remarks[self.rng.gen_range(0..remarks.len())];
        if remarks.len() > 1 {
            while remark.replace("{n}", &combo.to_string()) == self.remark {
                remark = remarks[self.rng.gen_range(0..remarks.len())];
            }
        }
        self.remark = remark.replace("{n}", &combo.to_string());
        self.mood = mood;
        self.held = 0.0;
    }

    /// The mood the signal calls for, in order of precedence.
    fn wanted(&self, signal: &PerformanceSignal) -> Mood {
        if signal.game_over {
            Mood::GameOver
        } else if let Some((mood, _)) = self.celebrating {
            mood
        } else if signal.stack_height > PANIC_AT {
            Mood::Panic
        } else if signal.combo >= STREAK_AT {
            Mood::Streak
        } else if signal.back_to_back {
            Mood::Smug
        } else {
            Mood::Idle
        }
    }
}

impl Default for Cow {
    fn default() -> Self {
        Self::new()
    }
}

impl Background for Cow {
    fn tick(&mut self, dt: Duration, _size: Size, signal: &PerformanceSignal) {
        let dt = dt.as_secs_f32();
        self.held += dt;

        if let Some((mood, remaining)) = self.celebrating {
            self.celebrating = (remaining > dt).then_some((mood, remaining - dt));
        }

        // A new placement, and one worth celebrating?
        let mut fresh = false;
        if signal.placements != self.seen {
            // Fewer placements than last time is a new run starting, not a
            // placement; anything its predecessor did is not news.
            let is_new = signal.placements > self.seen;
            self.seen = signal.placements;
            let celebration = if signal.last_tspin && signal.last_clear != ClearKind::None {
                Some(Mood::TSpin)
            } else if signal.last_clear == ClearKind::Tetris {
                Some(Mood::Tetris)
            } else {
                None
            };
            if let (true, Some(mood)) = (is_new, celebration) {
                self.celebrating = Some((mood, CELEBRATE_FOR));
                fresh = true;
            }
        }

        let wanted = self.wanted(signal);
        let settled = self.held >= DWELL;
        if wanted != self.mood {
            if wanted.is_urgent() || settled {
                self.switch(wanted, signal.combo);
            }
        } else if fresh {
            // Another celebration on top of the last deserves its own remark.
            self.switch(wanted, signal.combo);
        } else if wanted == Mood::Idle && self.held >= IDLE_ROTATE {
            self.switch(Mood::Idle, 0);
        }
    }

    fn render(&self, canvas: &mut Canvas, visuals: &Visuals, _signal: &PerformanceSignal) {
        // Find somewhere near the bottom the board is not.
        let guess = wrap(&self.remark, BUBBLE_TEXT).len() as u16 + 2 + COW.len() as u16;
        let (start, span) = canvas.widest_free_span(canvas.height().saturating_sub(guess));
        if span == 0 {
            return;
        }

        // Wrap to what the space allows, so a narrow margin still gets the
        // whole remark rather than a clipped one.
        let text_width = (span as usize).saturating_sub(4).clamp(8, BUBBLE_TEXT);
        let bubble = bubble(&wrap(&self.remark, text_width));
        let cow: Vec<String> = COW
            .iter()
            .map(|line| {
                line.replace("{e}", self.mood.eyes())
                    .replace("{t}", self.mood.tongue())
            })
            .collect();

        let height = (bubble.len() + cow.len()) as u16;
        let Some(top) = canvas.height().checked_sub(height) else {
            return;
        };
        let width = bubble.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let width = width.max(COW_WIDTH) as u16;
        // Centred in a margin, but kept to the left of an open screen, where
        // the title menu takes the middle.
        let x = start + (span.saturating_sub(width) / 2).min(2);

        let text = Style::default().fg(self.mood.colour(visuals));
        let body = Style::default().fg(Color::Gray).add_modifier(Modifier::DIM);
        for (row, line) in bubble.iter().enumerate() {
            canvas.text(x, top + row as u16, line, text);
        }
        for (row, line) in cow.iter().enumerate() {
            let y = top + (bubble.len() + row) as u16;
            canvas.text(x, y, line, body);
        }
    }

    fn name(&self) -> &'static str {
        "Cowsay"
    }
}

/// Greedy word wrap. A word longer than the width is split rather than
/// allowed to overflow.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > width {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            lines.push(word.drain(..width).collect());
        }
        let word: String = word.into_iter().collect();
        if line.is_empty() {
            line = word;
        } else if line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(&word);
        } else {
            lines.push(std::mem::replace(&mut line, word));
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// cowsay's speech bubble: `< >` round a single line, `/ | \` down the sides
/// of several.
fn bubble(lines: &[String]) -> Vec<String> {
    let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let mut out = vec![format!(" {}", "_".repeat(width + 2))];
    for (index, line) in lines.iter().enumerate() {
        let (open, close) = match (lines.len(), index) {
            (1, _) => ('<', '>'),
            (_, 0) => ('/', '\\'),
            (n, i) if i == n - 1 => ('\\', '/'),
            _ => ('|', '|'),
        };
        out.push(format!("{open} {line:<width$} {close}"));
    }
    out.push(format!(" {}", "-".repeat(width + 2)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    const TICK: Duration = Duration::from_nanos(16_666_667);
    const SIZE: Size = Size::new(80, 24);

    fn run(cow: &mut Cow, seconds: f32, signal: &PerformanceSignal) {
        for _ in 0..(seconds * 60.0) as usize {
            cow.tick(TICK, SIZE, signal);
        }
    }

    fn placed(placements: u64, clear: ClearKind, tspin: bool) -> PerformanceSignal {
        PerformanceSignal {
            placements,
            last_clear: clear,
            last_tspin: tspin,
            ..Default::default()
        }
    }

    fn render(cow: &Cow, width: u16, height: u16, reserved: &[Rect]) -> String {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let mut canvas = Canvas::new(&mut buf, area, reserved);
        cow.render(
            &mut canvas,
            &Visuals::default(),
            &PerformanceSignal::default(),
        );
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn each_signal_brings_its_own_mood() {
        let cases = [
            (PerformanceSignal::default(), Mood::Idle),
            (
                PerformanceSignal {
                    stack_height: 0.9,
                    ..Default::default()
                },
                Mood::Panic,
            ),
            (
                PerformanceSignal {
                    combo: 4,
                    ..Default::default()
                },
                Mood::Streak,
            ),
            (
                PerformanceSignal {
                    back_to_back: true,
                    ..Default::default()
                },
                Mood::Smug,
            ),
            (
                PerformanceSignal {
                    game_over: true,
                    ..Default::default()
                },
                Mood::GameOver,
            ),
        ];
        for (signal, mood) in cases {
            let mut cow = Cow::seeded(1);
            run(&mut cow, 2.0, &signal);
            assert_eq!(cow.mood, mood, "{signal:?}");
        }
    }

    /// Both legs stand in cowsay's column 16 whatever the face, tongue or not.
    #[test]
    fn the_cows_legs_line_up() {
        for mood in [Mood::Idle, Mood::GameOver] {
            let legs: Vec<usize> = COW[3..]
                .iter()
                .map(|line| line.replace("{t}", mood.tongue()).find("||").unwrap())
                .collect();
            assert_eq!(legs, [16, 16], "{mood:?}");
        }
    }

    /// Danger outranks encouragement: a combo on a nearly full board is still a
    /// nearly full board.
    #[test]
    fn panic_outranks_a_streak_and_a_streak_outranks_smugness() {
        let mut cow = Cow::seeded(2);
        let signal = PerformanceSignal {
            stack_height: 0.95,
            combo: 5,
            back_to_back: true,
            ..Default::default()
        };
        run(&mut cow, 2.0, &signal);
        assert_eq!(cow.mood, Mood::Panic);

        let signal = PerformanceSignal {
            combo: 5,
            back_to_back: true,
            ..Default::default()
        };
        run(&mut cow, 2.0, &signal);
        assert_eq!(cow.mood, Mood::Streak);
    }

    #[test]
    fn a_streak_remark_names_the_combo() {
        let mut cow = Cow::seeded(3);
        let signal = PerformanceSignal {
            combo: 7,
            ..Default::default()
        };
        run(&mut cow, 2.0, &signal);
        assert!(cow.remark.contains('7'), "{:?}", cow.remark);
    }

    /// A signal flickering every frame must not make the cow flicker with it.
    #[test]
    fn ordinary_moods_hold_for_the_dwell_time() {
        let mut cow = Cow::seeded(4);
        let calm = PerformanceSignal::default();
        let smug = PerformanceSignal {
            back_to_back: true,
            ..Default::default()
        };
        let mut changes = 0;
        let mut last = cow.mood;
        for frame in 0..60 * 6 {
            let signal = if frame % 2 == 0 { &calm } else { &smug };
            cow.tick(TICK, SIZE, signal);
            if cow.mood != last {
                changes += 1;
                last = cow.mood;
            }
        }
        assert!(changes <= 4, "{changes} changes in six seconds");
    }

    #[test]
    fn a_tetris_is_celebrated_at_once_and_then_passes() {
        let mut cow = Cow::seeded(5);
        run(&mut cow, 0.1, &PerformanceSignal::default());

        let tetris = placed(1, ClearKind::Tetris, false);
        cow.tick(TICK, SIZE, &tetris);
        assert_eq!(cow.mood, Mood::Tetris, "no dwell for a celebration");
        assert_eq!(cow.mood.eyes(), "^^");

        run(&mut cow, CELEBRATE_FOR + DWELL + 0.5, &tetris);
        assert_eq!(cow.mood, Mood::Idle, "the same placement is not news twice");
    }

    #[test]
    fn a_tspin_clear_is_celebrated_but_a_bare_tspin_is_not() {
        let mut cow = Cow::seeded(6);
        cow.tick(TICK, SIZE, &placed(1, ClearKind::None, true));
        assert_eq!(cow.mood, Mood::Idle);
        cow.tick(TICK, SIZE, &placed(2, ClearKind::Double, true));
        assert_eq!(cow.mood, Mood::TSpin);
    }

    /// Two Tetrises in a row report the same last clear; only the placement
    /// count shows the second one happened.
    #[test]
    fn back_to_back_tetrises_each_get_a_fresh_remark_and_timer() {
        let mut cow = Cow::seeded(7);
        cow.tick(TICK, SIZE, &placed(1, ClearKind::Tetris, false));
        run(
            &mut cow,
            CELEBRATE_FOR - 0.5,
            &placed(1, ClearKind::Tetris, false),
        );
        let first = cow.remark.clone();

        cow.tick(TICK, SIZE, &placed(2, ClearKind::Tetris, false));
        assert_ne!(cow.remark, first);
        run(&mut cow, 1.0, &placed(2, ClearKind::Tetris, false));
        assert_eq!(cow.mood, Mood::Tetris, "the timer restarted");
    }

    #[test]
    fn a_new_run_does_not_rejoice_in_the_last_one() {
        let mut cow = Cow::seeded(8);
        run(&mut cow, 2.0, &placed(40, ClearKind::Single, false));
        cow.tick(TICK, SIZE, &placed(0, ClearKind::Tetris, false));
        assert_eq!(cow.mood, Mood::Idle);
    }

    #[test]
    fn idle_remarks_come_round_every_so_often() {
        let mut cow = Cow::seeded(9);
        let first = cow.remark.clone();
        run(&mut cow, IDLE_ROTATE + 0.5, &PerformanceSignal::default());
        assert_ne!(cow.remark, first);
    }

    #[test]
    fn the_bubble_is_shaped_like_cowsays() {
        assert_eq!(
            bubble(&["moo.".to_string()]),
            [" ______", "< moo. >", " ------"]
        );
        let tall = bubble(&["one".into(), "two".into(), "three".into()]);
        assert_eq!(tall[1], "/ one   \\");
        assert_eq!(tall[2], "| two   |");
        assert_eq!(tall[3], "\\ three /");
    }

    #[test]
    fn wrapping_keeps_words_whole_and_splits_only_what_cannot_fit() {
        assert_eq!(wrap("i can see my barn", 10), ["i can see", "my barn"]);
        assert_eq!(wrap("udderly", 4), ["udde", "rly"]);
        assert_eq!(wrap("", 10), [""]);
    }

    #[test]
    fn the_cow_stands_in_the_free_margin_saying_its_piece() {
        let mut cow = Cow::seeded(10);
        cow.switch(Mood::Panic, 0);
        // A board over the middle; the wider margin is the right-hand one.
        let board = Rect::new(20, 0, 30, 24);
        let rendered = render(&cow, 90, 24, &[board]);
        println!("{rendered}");
        assert!(rendered.contains("(@@)"), "the panicked face");

        let first_word = cow.remark.split_whitespace().next().unwrap();
        let row = rendered.lines().find(|l| l.contains(first_word)).unwrap();
        assert!(
            row.find(first_word).unwrap() >= 50,
            "the remark is in the margin"
        );
    }

    #[test]
    fn a_narrow_margin_wraps_the_remark_rather_than_clipping_it() {
        let mut cow = Cow::seeded(11);
        cow.remark = "i can see my barn from up here.".into();
        let board = Rect::new(18, 0, 60, 24);
        let rendered = render(&cow, 78, 24, &[board]);
        println!("{rendered}");
        for word in ["barn", "here."] {
            assert!(rendered.contains(word), "{word} was cut off");
        }
    }
}
