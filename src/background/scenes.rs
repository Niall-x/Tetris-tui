//! Still scenes (§8.2): bundled ASCII art, drawn once and never animated.
//!
//! `tick` is a genuine no-op — these exist to give the terminal something calm
//! behind the field, not to move. Each scene is anchored to the bottom of the
//! screen and centred, so the art sits under the board rather than floating.

use std::time::Duration;

use ratatui::layout::Size;

use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

use super::{Background, Canvas, PerformanceSignal};
use crate::ui::style::Visuals;

/// Which scene to show. `Random` picks once when the background is created, not
/// per frame, so a session keeps the same scene (§8.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SceneChoice {
    #[default]
    Random,
    Mountains,
    Skyline,
    Forest,
    Ocean,
    NightSky,
}

impl SceneChoice {
    pub const ALL: [SceneChoice; 6] = [
        SceneChoice::Random,
        SceneChoice::Mountains,
        SceneChoice::Skyline,
        SceneChoice::Forest,
        SceneChoice::Ocean,
        SceneChoice::NightSky,
    ];

    /// The named scenes, without `Random`.
    pub const FIXED: [SceneChoice; 5] = [
        SceneChoice::Mountains,
        SceneChoice::Skyline,
        SceneChoice::Forest,
        SceneChoice::Ocean,
        SceneChoice::NightSky,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SceneChoice::Random => "Random",
            SceneChoice::Mountains => "Mountains",
            SceneChoice::Skyline => "Skyline",
            SceneChoice::Forest => "Forest",
            SceneChoice::Ocean => "Ocean",
            SceneChoice::NightSky => "Night sky",
        }
    }

    /// Resolve `Random` to an actual scene. Called once, when the background is
    /// built.
    fn resolve(self) -> Scene {
        match self {
            SceneChoice::Random => {
                use rand::seq::SliceRandom;
                let mut rng = rand::thread_rng();
                let pick = *SceneChoice::FIXED
                    .choose(&mut rng)
                    .unwrap_or(&SceneChoice::Mountains);
                pick.resolve()
            }
            SceneChoice::Mountains => Scene {
                art: MOUNTAINS,
                color: Color::Blue,
            },
            SceneChoice::Skyline => Scene {
                art: SKYLINE,
                color: Color::Magenta,
            },
            SceneChoice::Forest => Scene {
                art: FOREST,
                color: Color::Green,
            },
            SceneChoice::Ocean => Scene {
                art: OCEAN,
                color: Color::Cyan,
            },
            SceneChoice::NightSky => Scene {
                art: NIGHT_SKY,
                color: Color::White,
            },
        }
    }
}

struct Scene {
    art: &'static [&'static str],
    color: Color,
}

pub struct SceneBackground {
    scene: Scene,
}

impl SceneBackground {
    pub fn new(choice: SceneChoice) -> Self {
        Self {
            scene: choice.resolve(),
        }
    }
}

impl Background for SceneBackground {
    /// Nothing moves in a still scene.
    fn tick(&mut self, _dt: Duration, _size: Size, _signal: &PerformanceSignal) {}

    fn render(&self, canvas: &mut Canvas, _visuals: &Visuals, _signal: &PerformanceSignal) {
        let art_height = self.scene.art.len() as u16;
        let art_width = self
            .scene
            .art
            .iter()
            .map(|line| line.chars().count() as u16)
            .max()
            .unwrap_or(0);

        // Bottom-anchored and centred: the horizon belongs at the bottom of the
        // screen, and a scene wider than the terminal is simply clipped.
        let x = canvas.width().saturating_sub(art_width) / 2;
        let y = canvas.height().saturating_sub(art_height);

        // Dimmed on purpose. A background that competes with the stack for
        // attention is a worse background, however nice the art is.
        let style = Style::default()
            .fg(self.scene.color)
            .add_modifier(Modifier::DIM);
        canvas.block(x, y, self.scene.art, style);
    }

    fn name(&self) -> &'static str {
        "Scene"
    }
}

const MOUNTAINS: &[&str] = &[
    "                /\\                    /\\                ",
    "               /  \\        /\\        /  \\               ",
    "      /\\      /    \\      /  \\      /    \\      /\\      ",
    "     /  \\    /      \\    /    \\    /      \\    /  \\     ",
    "    /    \\  /   /\\   \\  /      \\  /   /\\   \\  /    \\    ",
    "   /      \\/   /  \\   \\/        \\/   /  \\   \\/      \\   ",
    "  /            \\  /            \\            \\  /     \\  ",
    " /              \\/              \\            \\/       \\ ",
    "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
];

const SKYLINE: &[&str] = &[
    "            ___                                  ___    ",
    "     __    |   |         ___            __      |   |   ",
    "    |  |   |[ ]|   __   |   |   ___    |  |     |[ ]|   ",
    " ___|[ ]|__|   |__|  |__|[ ]|__|   |___|[ ]|____|   |___",
    "|   |   |  |[ ]|  |[ ]| |   |  |[ ]|   |   |    |[ ]|   ",
    "|[ ]|[ ]|  |   |  |   | |[ ]|  |   |[ ]|[ ]|    |   |[ ]",
    "|   |   |[ ]|[ ]|[ ]|[ ]|   |[ ]|[ ]|   |   |[ ]|[ ]|   ",
    "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^",
];

const FOREST: &[&str] = &[
    "        /\\              /\\          /\\                  ",
    "       /  \\      /\\    /  \\        /  \\      /\\         ",
    "      /    \\    /  \\  /    \\      /    \\    /  \\        ",
    "     /  /\\  \\  /    \\/  /\\  \\    /  /\\  \\  /    \\       ",
    "    /  /  \\  \\/  /\\    /  \\  \\  /  /  \\  \\/  /\\  \\      ",
    "   /__/    \\____/  \\__/    \\__\\/__/    \\____/  \\__\\     ",
    "        ||            ||          ||          ||        ",
    "       _||_          _||_        _||_        _||_       ",
    "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
];

const OCEAN: &[&str] = &[
    "                          |                             ",
    "                         /|\\                            ",
    "                        / | \\                           ",
    "                       /__|__\\                          ",
    "                          |                             ",
    "                     \\____|____/                        ",
    "                      \\_______/                         ",
    "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
    "  ~~~~~~~~~   ~~~~~~~~~~~   ~~~~~~~~~   ~~~~~~~~~~~~~   ",
    "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
];

const NIGHT_SKY: &[&str] = &[
    "    .        *           .          *        .     *    ",
    "         *        .            *        .          .    ",
    "   *          .        *            .        *          ",
    "        .           *       .            *        .     ",
    "  *        .              *        .           *        ",
    "      .         *     .        *        .          *    ",
    "          .        .       .        .         .         ",
    "________________________________________________________",
];

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn draw(choice: SceneChoice, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let background = SceneBackground::new(choice);
        let mut canvas = Canvas::new(&mut buf, area, &[]);
        background.render(
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

    #[test]
    fn every_named_scene_draws_something() {
        for choice in SceneChoice::FIXED {
            let buf = draw(choice, 70, 24);
            let rendered = as_string(&buf);
            println!("=== {} ===\n{rendered}", choice.label());
            assert!(
                rendered.chars().any(|c| !c.is_whitespace()),
                "{choice:?} drew nothing"
            );
        }
    }

    /// Scene art is authored as a rectangle; a ragged one would draw crooked.
    #[test]
    fn every_scene_has_lines_of_equal_length() {
        for choice in SceneChoice::FIXED {
            let art = choice.resolve().art;
            let widths: Vec<usize> = art.iter().map(|line| line.chars().count()).collect();
            assert!(
                widths.windows(2).all(|pair| pair[0] == pair[1]),
                "{choice:?} has ragged lines: {widths:?}"
            );
        }
    }

    #[test]
    fn scenes_are_anchored_to_the_bottom() {
        let buf = draw(SceneChoice::Mountains, 70, 24);
        let rows: Vec<String> = as_string(&buf).lines().map(|l| l.to_string()).collect();
        assert!(
            rows[0].trim().is_empty(),
            "the top of a tall terminal stays clear"
        );
        assert!(
            !rows[rows.len() - 1].trim().is_empty(),
            "the horizon sits on the bottom row"
        );
    }

    /// A terminal smaller than the art must clip, not panic or wrap.
    #[test]
    fn a_small_terminal_clips_the_art() {
        for choice in SceneChoice::FIXED {
            draw(choice, 10, 4);
            draw(choice, 1, 1);
        }
    }

    #[test]
    fn the_playfield_stays_clear_of_the_scene() {
        let area = Rect::new(0, 0, 70, 24);
        let board = Rect::new(20, 2, 22, 22);
        let mut buf = Buffer::empty(area);
        let background = SceneBackground::new(SceneChoice::Mountains);
        let reserved = [board];
        let mut canvas = Canvas::new(&mut buf, area, &reserved);
        background.render(
            &mut canvas,
            &Visuals::default(),
            &PerformanceSignal::default(),
        );

        for y in board.y..board.bottom() {
            for x in board.x..board.right() {
                assert_eq!(buf[(x, y)].symbol(), " ", "scene bled into the board");
            }
        }
    }

    #[test]
    fn random_resolves_to_one_of_the_named_scenes() {
        for _ in 0..20 {
            let scene = SceneChoice::Random.resolve();
            assert!(
                SceneChoice::FIXED
                    .iter()
                    .any(|choice| choice.resolve().art == scene.art),
                "random produced art that is not in the set"
            );
        }
    }

    /// The scene is picked when the background is built, so it cannot change
    /// under the player mid-session.
    #[test]
    fn a_random_scene_is_stable_for_the_life_of_the_background() {
        let background = SceneBackground::new(SceneChoice::Random);
        let first = as_string(&{
            let area = Rect::new(0, 0, 70, 24);
            let mut buf = Buffer::empty(area);
            let mut canvas = Canvas::new(&mut buf, area, &[]);
            background.render(
                &mut canvas,
                &Visuals::default(),
                &PerformanceSignal::default(),
            );
            buf
        });
        let second = as_string(&{
            let area = Rect::new(0, 0, 70, 24);
            let mut buf = Buffer::empty(area);
            let mut canvas = Canvas::new(&mut buf, area, &[]);
            background.render(
                &mut canvas,
                &Visuals::default(),
                &PerformanceSignal::default(),
            );
            buf
        });
        assert_eq!(first, second);
    }

    #[test]
    fn every_choice_including_random_has_a_label() {
        for choice in SceneChoice::ALL {
            assert!(!choice.label().is_empty(), "{choice:?}");
        }
    }
}
