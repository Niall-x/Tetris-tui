//! The distro-logo background (§8.3): the running system's logo, tiled.
//!
//! The logo is identified by reading `/etc/os-release` ourselves and matching its
//! `ID`, falling back to `ID_LIKE` for derivatives and then to a generic penguin.
//! Shelling out to `fastfetch` would give perfect fidelity to whatever the player
//! has configured, including custom logos, but it is the one background that would
//! need an external *binary* plus stable CLI flags — so, like the other novelty
//! backgrounds here, the art is our own.

use std::time::Duration;

use ratatui::layout::Size;

use ratatui::style::{Color, Modifier, Style};

use super::{Background, Canvas, PerformanceSignal};
use crate::ui::style::Visuals;

/// Clear space left between tiled copies: enough that the logos read as a
/// pattern rather than a wall of glyphs behind the board.
const GAP: u16 = 8;

pub struct LogoBackground {
    logo: Logo,
}

impl LogoBackground {
    /// Identify the running distribution, or fall back to the generic logo.
    pub fn detect() -> Self {
        Self {
            logo: from_os_release(&read_os_release()),
        }
    }

    #[cfg(test)]
    fn named(id: &str) -> Self {
        Self { logo: for_id(id) }
    }
}

impl Background for LogoBackground {
    /// §8.3: a repeated logo reads fine without motion, so this is static.
    fn tick(&mut self, _dt: Duration, _size: Size, _signal: &PerformanceSignal) {}

    fn render(&self, canvas: &mut Canvas, _visuals: &Visuals, _signal: &PerformanceSignal) {
        let art = self.logo.art;
        let height = art.len() as u16;
        let width = art
            .iter()
            .map(|line| line.chars().count() as u16)
            .max()
            .unwrap_or(0);
        if width == 0 || height == 0 {
            return;
        }

        let style = Style::default()
            .fg(self.logo.color)
            .add_modifier(Modifier::DIM);

        let step_x = width + GAP;
        let step_y = height + GAP / 2;

        let mut y = 0;
        while y < canvas.height() {
            let mut x = 0;
            while x < canvas.width() {
                canvas.block(x, y, art, style);
                x += step_x;
            }
            y += step_y;
        }
    }

    fn name(&self) -> &'static str {
        "Distro logo"
    }
}

struct Logo {
    art: &'static [&'static str],
    color: Color,
}

fn read_os_release() -> String {
    std::fs::read_to_string("/etc/os-release").unwrap_or_default()
}

/// Pull `ID` out of an os-release file, falling back to the first entry of
/// `ID_LIKE` so derivatives land on their parent's logo.
fn from_os_release(text: &str) -> Logo {
    let field = |key: &str| -> Option<String> {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .map(|value| value.trim().trim_matches('"').to_lowercase())
    };

    if let Some(id) = field("ID") {
        if is_known(&id) {
            return for_id(&id);
        }
    }
    if let Some(like) = field("ID_LIKE") {
        if let Some(parent) = like.split_whitespace().find(|word| is_known(word)) {
            return for_id(parent);
        }
    }
    for_id("")
}

fn is_known(id: &str) -> bool {
    matches!(
        id,
        "arch" | "debian" | "ubuntu" | "fedora" | "nixos" | "opensuse" | "manjaro"
    )
}

fn for_id(id: &str) -> Logo {
    match id {
        "arch" => Logo {
            art: ARCH,
            color: Color::Cyan,
        },
        "debian" => Logo {
            art: DEBIAN,
            color: Color::Red,
        },
        "ubuntu" => Logo {
            art: UBUNTU,
            color: Color::LightRed,
        },
        "fedora" => Logo {
            art: FEDORA,
            color: Color::Blue,
        },
        "nixos" => Logo {
            art: NIXOS,
            color: Color::Cyan,
        },
        "opensuse" => Logo {
            art: OPENSUSE,
            color: Color::Green,
        },
        "manjaro" => Logo {
            art: MANJARO,
            color: Color::Green,
        },
        // Anything unrecognised, and anything without an os-release file at all.
        _ => Logo {
            art: TUX,
            color: Color::White,
        },
    }
}

const ARCH: &[&str] = &[
    "      /\\      ",
    "     /  \\     ",
    "    /\\   \\    ",
    "   /      \\   ",
    "  /   ,,   \\  ",
    " /   |  |   \\ ",
    "/_-''    ''-_\\",
];

const DEBIAN: &[&str] = &[
    "   ,---._    ",
    "  /  ,-.  \\  ",
    " |  /   \\ |  ",
    " |  |   ' '  ",
    " |  \\        ",
    "  \\  `-._    ",
    "   `-.___.'  ",
];

const UBUNTU: &[&str] = &[
    "         _    ",
    "     ---(_)   ",
    " _/  ---  \\   ",
    "(_) |   |     ",
    "  \\  --- _/   ",
    "     ---(_)   ",
];

const FEDORA: &[&str] = &[
    "   ,'''''.   ",
    "  |   ,.  |  ",
    "  |  |  '_'  ",
    "  |  |__     ",
    "  |     |    ",
    "   '.___'    ",
];

const NIXOS: &[&str] = &[
    "  \\\\  \\\\ //  ",
    " ==\\\\__\\\\/ //",
    "   //   \\\\// ",
    "==//     //==",
    " //\\\\___//   ",
    "// /\\\\  \\\\== ",
    "  // \\\\  \\\\  ",
];

const OPENSUSE: &[&str] = &[
    "  _______    ",
    "__|   __ \\   ",
    "     / .\\ \\  ",
    "     \\__/ |  ",
    "   _______|  ",
    "   \\_______  ",
    "__________/  ",
];

const MANJARO: &[&str] = &[
    "||||||||| ||||",
    "||||||||| ||||",
    "||||      ||||",
    "|||| |||| ||||",
    "|||| |||| ||||",
    "|||| |||| ||||",
];

const TUX: &[&str] = &[
    "    .--.     ",
    "   |o_o |    ",
    "   |:_/ |    ",
    "  //   \\ \\   ",
    " (|     | )  ",
    "/'\\_   _/`\\  ",
    "\\___)=(___/  ",
];

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn draw(background: &LogoBackground, width: u16, height: u16, board: Rect) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        let reserved = [board];
        let mut canvas = Canvas::new(&mut buf, area, &reserved);
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
    fn the_id_field_picks_the_logo() {
        let text = "NAME=\"Arch Linux\"\nID=arch\nPRETTY_NAME=\"Arch Linux\"\n";
        assert_eq!(from_os_release(text).art, ARCH);
    }

    /// Quoted values are normal in os-release files.
    #[test]
    fn quoted_and_mixed_case_ids_are_handled() {
        assert_eq!(from_os_release("ID=\"Fedora\"\n").art, FEDORA);
        assert_eq!(from_os_release("ID=NixOS\n").art, NIXOS);
    }

    /// A derivative names its parent in ID_LIKE; landing on the parent's logo
    /// beats falling through to the generic one.
    #[test]
    fn a_derivative_falls_back_to_its_parent() {
        let text = "ID=pop\nID_LIKE=\"ubuntu debian\"\n";
        assert_eq!(from_os_release(text).art, UBUNTU);
    }

    #[test]
    fn an_unknown_or_missing_os_release_gets_the_generic_logo() {
        assert_eq!(from_os_release("").art, TUX);
        assert_eq!(from_os_release("ID=plan9\n").art, TUX);
        assert_eq!(from_os_release("ID=plan9\nID_LIKE=inferno\n").art, TUX);
    }

    /// `ID` is a prefix of `ID_LIKE`, which a careless parser reads as the ID.
    #[test]
    fn id_like_is_not_mistaken_for_id() {
        let text = "ID_LIKE=debian\nID=ubuntu\n";
        assert_eq!(from_os_release(text).art, UBUNTU);
    }

    #[test]
    fn every_logo_is_a_rectangle_and_draws() {
        for id in [
            "arch", "debian", "ubuntu", "fedora", "nixos", "opensuse", "manjaro", "",
        ] {
            let logo = for_id(id);
            let widths: Vec<usize> = logo.art.iter().map(|l| l.chars().count()).collect();
            assert!(
                widths.windows(2).all(|pair| pair[0] == pair[1]),
                "{id} has ragged lines: {widths:?}"
            );

            let background = LogoBackground::named(id);
            let rendered = as_string(&draw(&background, 60, 20, Rect::ZERO));
            println!("=== {id} ===\n{rendered}");
            assert!(rendered.chars().any(|c| !c.is_whitespace()), "{id}");
        }
    }

    /// Tiling is the point: a wide terminal should hold several copies.
    #[test]
    fn the_logo_is_tiled_across_the_area() {
        let background = LogoBackground::named("manjaro");
        let rendered = as_string(&draw(&background, 80, 30, Rect::ZERO));
        let copies = rendered.matches("||||||||| ||||").count();
        assert!(copies >= 4, "expected several tiles, got {copies}");
    }

    #[test]
    fn tiles_never_reach_into_the_playfield() {
        let board = Rect::new(20, 1, 22, 22);
        let buf = draw(&LogoBackground::named("arch"), 80, 24, board);
        for y in board.y..board.bottom() {
            for x in board.x..board.right() {
                assert_eq!(buf[(x, y)].symbol(), " ", "a tile bled into the board");
            }
        }
    }

    #[test]
    fn a_terminal_smaller_than_one_tile_still_renders() {
        draw(&LogoBackground::named("arch"), 4, 2, Rect::ZERO);
        draw(&LogoBackground::named("arch"), 1, 1, Rect::ZERO);
    }

    /// Detection runs against the real filesystem; whatever it finds, it must
    /// produce a usable background rather than failing.
    #[test]
    fn detection_always_yields_a_logo() {
        let background = LogoBackground::detect();
        assert!(!background.logo.art.is_empty());
        assert_eq!(background.name(), "Distro logo");
    }
}
