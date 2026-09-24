//! The distro-logo background (§8.3): the running system's logo, scattered.
//!
//! The logo is identified by reading `/etc/os-release` ourselves and matching its
//! `ID`, falling back to `ID_LIKE` for derivatives and then to a generic penguin.
//! Shelling out to `fastfetch` would give perfect fidelity to whatever the player
//! has configured, including custom logos, but it is the one background that would
//! need an external *binary* plus stable CLI flags — so, like the other novelty
//! backgrounds here, the art is our own.
//!
//! The copies are scattered like polka dots rather than tiled: a few, at random
//! spots that never touch, chosen once per terminal size so they hold still.

use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use ratatui::layout::Size;
use ratatui::style::{Color, Modifier, Style};

use super::{Background, Canvas, PerformanceSignal};
use crate::ui::style::Visuals;

/// Screen area per logo, in multiples of one logo's own area. Six keeps them
/// sparse enough to read as dots rather than wallpaper.
const AREA_PER_LOGO: u32 = 6;

/// Clear space kept around every logo, so no two ever touch. Columns are about
/// half as wide as rows are tall, hence twice as many.
const PAD_X: u16 = 8;
const PAD_Y: u16 = 3;

/// Random spots tried before settling for fewer logos than the area allows.
const ATTEMPTS: usize = 400;

pub struct LogoBackground {
    logo: Logo,
    rng: SmallRng,
    /// Top-left corners of the scattered copies, for `laid_out_for`.
    spots: Vec<(u16, u16)>,
    laid_out_for: Option<Size>,
}

impl LogoBackground {
    /// Identify the running distribution, or fall back to the generic logo.
    pub fn detect() -> Self {
        Self::new(
            from_os_release(&read_os_release()),
            SmallRng::from_entropy(),
        )
    }

    fn new(logo: Logo, rng: SmallRng) -> Self {
        Self {
            logo,
            rng,
            spots: Vec::new(),
            laid_out_for: None,
        }
    }

    #[cfg(test)]
    fn named(id: &str) -> Self {
        Self::new(for_id(id), SmallRng::seed_from_u64(7))
    }

    /// Room one copy needs: the larger of the art and its ASCII version, so the
    /// spots stay valid whichever is drawn.
    fn logo_size(&self) -> (u16, u16) {
        let arts = [Some(self.logo.art), self.logo.ascii];
        let width = arts
            .iter()
            .flatten()
            .flat_map(|art| art.iter())
            .map(|line| line.chars().count() as u16)
            .max()
            .unwrap_or(0);
        let height = arts
            .iter()
            .flatten()
            .map(|art| art.len())
            .max()
            .unwrap_or(0);
        (width, height as u16)
    }

    /// Pick spots at random, keeping each one that clears the others by the
    /// padding, until the area's share is placed or the attempts run out.
    fn scatter(&mut self, size: Size) {
        self.spots.clear();
        self.laid_out_for = Some(size);

        let (width, height) = self.logo_size();
        if width == 0 || height == 0 || width > size.width || height > size.height {
            return;
        }
        let wanted = (size.width as u32 * size.height as u32)
            / (width as u32 * height as u32 * AREA_PER_LOGO);
        let wanted = wanted.max(1) as usize;

        for _ in 0..ATTEMPTS {
            if self.spots.len() >= wanted {
                break;
            }
            let x = self.rng.gen_range(0..=size.width - width);
            let y = self.rng.gen_range(0..=size.height - height);
            let clear = self.spots.iter().all(|&(sx, sy)| {
                x >= sx + width + PAD_X
                    || sx >= x + width + PAD_X
                    || y >= sy + height + PAD_Y
                    || sy >= y + height + PAD_Y
            });
            if clear {
                self.spots.push((x, y));
            }
        }
    }
}

impl Background for LogoBackground {
    /// Still, like the scenes: the spots only move when the terminal is resized.
    fn tick(&mut self, _dt: Duration, size: Size, _signal: &PerformanceSignal) {
        if self.laid_out_for != Some(size) {
            self.scatter(size);
        }
    }

    fn render(&self, canvas: &mut Canvas, visuals: &Visuals, _signal: &PerformanceSignal) {
        let (width, height) = self.logo_size();
        // With an ASCII option picked, a logo with an ASCII version uses it, in
        // its main colour only.
        let (art, colored) = match (visuals.ascii_only(), self.logo.ascii) {
            (true, Some(ascii)) => (ascii, false),
            _ => (self.logo.art, true),
        };

        for &(x, y) in &self.spots {
            // A dot is drawn whole or not at all: a logo sliced by the board's
            // edge reads as a mistake rather than a pattern.
            let whole = (y..y.saturating_add(height))
                .all(|cy| (x..x.saturating_add(width)).all(|cx| canvas.accepts(cx, cy)));
            if !whole {
                continue;
            }
            for (row, line) in art.iter().enumerate() {
                for (col, ch) in line.chars().enumerate() {
                    if ch == ' ' {
                        continue;
                    }
                    let color = if colored {
                        self.logo.color_at(row, col)
                    } else {
                        self.logo.colors[0]
                    };
                    let style = Style::default().fg(color).add_modifier(Modifier::DIM);
                    canvas.put(x + col as u16, y + row as u16, ch, style);
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "Distro logo"
    }
}

struct Logo {
    art: &'static [&'static str],
    /// Which of `colors` each character is drawn in, `1` for the first, for a
    /// logo fastfetch colours in more than one. Without one it is all `colors[0]`.
    mask: Option<&'static [&'static str]>,
    colors: &'static [Color],
    /// A plain-ASCII version, for a logo whose art is not ASCII itself.
    ascii: Option<&'static [&'static str]>,
}

impl Logo {
    fn plain(art: &'static [&'static str], color: &'static [Color]) -> Self {
        Self {
            art,
            mask: None,
            colors: color,
            ascii: None,
        }
    }

    /// The colour of the character at `row`, `col`.
    fn color_at(&self, row: usize, col: usize) -> Color {
        let index = self
            .mask
            .and_then(|mask| mask.get(row)?.chars().nth(col)?.to_digit(10))
            .unwrap_or(1) as usize;
        self.colors
            .get(index.saturating_sub(1))
            .copied()
            .unwrap_or(Color::White)
    }
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

/// The distributions with a logo of their own. The empty ID is the generic
/// penguin, which is what anything else gets.
const KNOWN_IDS: [&str; 8] = [
    "arch", "debian", "ubuntu", "fedora", "nixos", "opensuse", "manjaro", "",
];

fn is_known(id: &str) -> bool {
    !id.is_empty() && KNOWN_IDS.contains(&id)
}

/// fastfetch's colours for each logo. It draws the penguin's body in black,
/// which vanishes on a dark terminal once dimmed, so that one is dark grey.
fn for_id(id: &str) -> Logo {
    match id {
        "arch" => Logo::plain(ARCH, &[Color::Cyan]),
        "debian" => Logo::plain(DEBIAN, &[Color::Red]),
        "ubuntu" => Logo::plain(UBUNTU, &[Color::Red]),
        "fedora" => Logo::plain(FEDORA, &[Color::Blue]),
        "nixos" => Logo {
            art: NIXOS,
            mask: Some(NIXOS_MASK),
            colors: &[Color::Blue, Color::Cyan],
            ascii: Some(NIXOS_ASCII),
        },
        "opensuse" => Logo::plain(OPENSUSE, &[Color::Green]),
        "manjaro" => Logo::plain(MANJARO, &[Color::Green]),
        // Anything unrecognised, and anything without an os-release file at all.
        _ => Logo {
            art: TUX,
            mask: Some(TUX_MASK),
            colors: &[Color::DarkGray, Color::White, Color::Yellow],
            ascii: None,
        },
    }
}

// Every logo below is fastfetch 2.68's small logo for that distribution,
// generated from its coloured output, padded to a rectangle. A `_MASK` gives each
// character's colour for the logos fastfetch draws in more than one.

const ARCH: &[&str] = &[
    "      /\\      ",
    "     /  \\     ",
    "    /    \\    ",
    "   /      \\   ",
    "  /   ,,   \\  ",
    " /   |  |   \\ ",
    "/_-''    ''-_\\",
];

const DEBIAN: &[&str] = &[
    "  _____  ",
    " /  __ \\ ",
    "|  /    |",
    "|  \\___- ",
    "-_       ",
    "  --_    ",
];

const UBUNTU: &[&str] = &[
    "       ..;,; .,;,.    ",
    "    .,lool: .ooooo,   ",
    "   ;oo;:    .coool.   ",
    " ....         ''' ,l; ",
    ":oooo,            'oo.",
    "looooc            :oo'",
    " '::'             ,oo:",
    "   ,.,       .... co, ",
    "    lo:;.   :oooo; .  ",
    "     ':ooo; cooooc    ",
    "        '''  ''''     ",
];

const FEDORA: &[&str] = &[
    "        ,'''''. ",
    "       |   ,.  |",
    "       |  |  '_'",
    "  ,....|  |..   ",
    ".'  ,_;|   ..'  ",
    "|  |   |  |     ",
    "|  ',_,'  |     ",
    " '.     ,'      ",
    "   '''''        ",
];

const NIXOS: &[&str] = &[
    "  ▗▄   ▗▄ ▄▖  ",
    " ▄▄🬸█▄▄▄🬸█▛ ▃ ",
    "   ▟▛    ▜▃▟🬕 ",
    "🬋🬋🬫█      █🬛🬋🬋",
    " 🬷▛🮃▙    ▟▛   ",
    " 🮃 ▟█🬴▀▀▀█🬴▀▀ ",
    "  ▝▀ ▀▘   ▀▘  ",
];

const NIXOS_MASK: &[&str] = &[
    "  11   22 22  ",
    " 1111111222 1 ",
    "   22    2111 ",
    "2222      1111",
    " 2221    11   ",
    " 2 1112222222 ",
    "  11 11   22  ",
];

const OPENSUSE: &[&str] = &[
    "  _______  ",
    "__|   __ \\ ",
    "     / .\\ \\",
    "     \\__/ |",
    "   _______|",
    "   \\_______",
    "__________/",
];

const MANJARO: &[&str] = &[
    "||||||||| ||||",
    "||||||||| ||||",
    "||||      ||||",
    "|||| |||| ||||",
    "|||| |||| ||||",
    "|||| |||| ||||",
    "|||| |||| ||||",
];

const TUX: &[&str] = &[
    "    ___   ",
    "   (.. \\  ",
    "   (<> |  ",
    "  //  \\ \\ ",
    " ( |  | /|",
    "_/\\ __)/_)",
    "\\/-____\\/ ",
];

const TUX_MASK: &[&str] = &[
    "    111   ",
    "   122 1  ",
    "   133 1  ",
    "  12  2 1 ",
    " 1 2  2 11",
    "311 222131",
    "331111133 ",
];

/// fastfetch's previous `NixOS_small`, for when an ASCII option is picked: none
/// of its current NixOS logos are ASCII, and sextants are recent enough that
/// plenty of fonts still lack them.
const NIXOS_ASCII: &[&str] = &[
    "  \\\\  \\\\ //  ",
    " ==\\\\__\\\\/ //",
    "   //   \\\\// ",
    "==//     //==",
    " //\\\\___//   ",
    "// /\\\\  \\\\== ",
    "  // \\\\  \\\\  ",
];

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn draw(background: &mut LogoBackground, width: u16, height: u16, board: Rect) -> Buffer {
        let signal = PerformanceSignal::default();
        background.tick(Duration::ZERO, Size::new(width, height), &signal);
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

    /// A mask has to line up with its art character for character, mark only
    /// cells that have something in them, and name only colours the logo has.
    #[test]
    fn every_colour_mask_matches_its_art() {
        for id in KNOWN_IDS {
            let logo = for_id(id);
            let Some(mask) = logo.mask else { continue };
            assert_eq!(logo.art.len(), mask.len(), "{id}");
            for (line, mask_line) in logo.art.iter().zip(mask) {
                assert_eq!(
                    line.chars().count(),
                    mask_line.chars().count(),
                    "{id}: {line:?}"
                );
                for (ch, m) in line.chars().zip(mask_line.chars()) {
                    match m.to_digit(10) {
                        None => assert_eq!(ch, ' ', "{id}: {line:?} / {mask_line:?}"),
                        Some(n) => assert!(
                            ch != ' ' && (1..=logo.colors.len() as u32).contains(&n),
                            "{id}: {line:?} / {mask_line:?}"
                        ),
                    }
                }
            }
        }
    }

    #[test]
    fn the_generic_penguin_keeps_fastfetchs_three_colours() {
        let mut background = LogoBackground::named("");
        let buf = draw(&mut background, 120, 40, Rect::ZERO);
        let colours: std::collections::HashSet<_> = buf
            .content
            .iter()
            .filter(|cell| cell.symbol() != " ")
            .map(|cell| cell.fg)
            .collect();
        assert_eq!(
            colours,
            [Color::DarkGray, Color::White, Color::Yellow]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn nixos_draws_in_two_colours_as_fastfetch_does() {
        let mut background = LogoBackground::named("nixos");
        let buf = draw(&mut background, 120, 40, Rect::ZERO);
        let colours: std::collections::HashSet<_> = buf
            .content
            .iter()
            .filter(|cell| cell.symbol() != " ")
            .map(|cell| cell.fg)
            .collect();
        assert_eq!(
            colours,
            [Color::Blue, Color::Cyan].into_iter().collect(),
            "blue body, cyan accents"
        );
    }

    /// Sextants are missing from plenty of fonts, so an ASCII option swaps in
    /// the line-art logo.
    #[test]
    fn an_ascii_option_gets_the_ascii_nixos_logo() {
        let mut background = LogoBackground::named("nixos");
        let signal = PerformanceSignal::default();
        background.tick(Duration::ZERO, Size::new(120, 40), &signal);
        let area = Rect::new(0, 0, 120, 40);
        let mut buf = Buffer::empty(area);
        let mut canvas = Canvas::new(&mut buf, area, &[]);
        let ascii = Visuals {
            border: crate::ui::style::BorderStyle::Ascii,
            ..Default::default()
        };
        background.render(&mut canvas, &ascii, &signal);
        assert!(buf.content.iter().all(|cell| cell.symbol().is_ascii()));
        assert!(buf.content.iter().any(|cell| cell.symbol() == "\\"));
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
        for id in KNOWN_IDS {
            let logo = for_id(id);
            let widths: Vec<usize> = logo.art.iter().map(|l| l.chars().count()).collect();
            assert!(
                widths.windows(2).all(|pair| pair[0] == pair[1]),
                "{id} has ragged lines: {widths:?}"
            );

            let mut background = LogoBackground::named(id);
            let rendered = as_string(&draw(&mut background, 60, 20, Rect::ZERO));
            println!("=== {id} ===\n{rendered}");
            assert!(rendered.chars().any(|c| !c.is_whitespace()), "{id}");
        }
    }

    fn scattered(id: &str, width: u16, height: u16) -> LogoBackground {
        let mut background = LogoBackground::named(id);
        background.scatter(Size::new(width, height));
        background
    }

    /// Polka dots, not a grid: a handful of copies, none of them touching.
    #[test]
    fn a_few_copies_are_scattered_without_touching() {
        let background = scattered("manjaro", 200, 60);
        let (width, height) = background.logo_size();
        let spots = &background.spots;
        assert!(spots.len() >= 3, "a big terminal holds several: {spots:?}");

        let area_share = 200 * 60 / (width as usize * height as usize * AREA_PER_LOGO as usize);
        assert!(spots.len() <= area_share, "no more than the area allows");

        for (i, &(ax, ay)) in spots.iter().enumerate() {
            assert!(ax + width <= 200 && ay + height <= 60, "on screen");
            for &(bx, by) in &spots[i + 1..] {
                let apart = ax >= bx + width + PAD_X
                    || bx >= ax + width + PAD_X
                    || ay >= by + height + PAD_Y
                    || by >= ay + height + PAD_Y;
                assert!(apart, "{:?} and {:?} touch", (ax, ay), (bx, by));
            }
        }
    }

    /// Not on a lattice: the copies do not all share rows and columns.
    #[test]
    fn the_copies_are_not_lined_up() {
        let background = scattered("manjaro", 200, 60);
        let mut xs: Vec<u16> = background.spots.iter().map(|s| s.0).collect();
        xs.sort_unstable();
        xs.dedup();
        assert!(xs.len() > 1);
        let gaps: Vec<u16> = xs.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            gaps.windows(2).any(|g| g[0] != g[1]) || gaps.len() < 2,
            "evenly spaced columns look like tiling: {xs:?}"
        );
    }

    /// The dots hold still frame to frame, and move only when the terminal does.
    #[test]
    fn the_spots_only_change_on_resize() {
        let mut background = LogoBackground::named("arch");
        let signal = PerformanceSignal::default();
        background.tick(Duration::ZERO, Size::new(120, 40), &signal);
        let first = background.spots.clone();
        for _ in 0..10 {
            background.tick(Duration::ZERO, Size::new(120, 40), &signal);
        }
        assert_eq!(background.spots, first);

        background.tick(Duration::ZERO, Size::new(160, 50), &signal);
        assert_eq!(background.laid_out_for, Some(Size::new(160, 50)));
    }

    /// A copy that would be cut by the board is left out rather than sliced.
    #[test]
    fn a_copy_under_the_board_is_skipped_whole() {
        let mut background = scattered("arch", 120, 40);
        let (width, height) = background.logo_size();
        let (x, y) = background.spots[0];
        // A "board" covering just one cell of the first copy.
        let board = Rect::new(x + width / 2, y + height / 2, 1, 1);
        let buf = draw(&mut background, 120, 40, board);
        for cy in y..y + height {
            for cx in x..x + width {
                assert_eq!(buf[(cx, cy)].symbol(), " ", "a partial copy was drawn");
            }
        }
    }

    #[test]
    fn tiles_never_reach_into_the_playfield() {
        let board = Rect::new(20, 1, 22, 22);
        let buf = draw(&mut LogoBackground::named("arch"), 80, 24, board);
        for y in board.y..board.bottom() {
            for x in board.x..board.right() {
                assert_eq!(buf[(x, y)].symbol(), " ", "a tile bled into the board");
            }
        }
    }

    #[test]
    fn a_terminal_smaller_than_one_tile_still_renders() {
        draw(&mut LogoBackground::named("arch"), 4, 2, Rect::ZERO);
        draw(&mut LogoBackground::named("arch"), 1, 1, Rect::ZERO);
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
