//! Colour theme, tetromino skin and board border — the three visual axes.
//!
//! They are deliberately independent (§7 of the brief): any theme works with any
//! skin and any border, so nothing here branches on another axis. A cell's colour
//! comes from the theme, its glyphs from the skin, and the chrome around the board
//! from the border style.
//!
//! Every skin renders a cell as exactly two terminal columns, because the board is
//! drawn two columns per cell — a skin that returned one or three would shear the
//! whole playfield, so the pair is the unit the API deals in.

use std::borrow::Cow;

use ratatui::style::Color;
use ratatui::symbols::border;
use ratatui::widgets::{Block, Borders};
use serde::{Deserialize, Serialize};

use crate::engine::piece::PieceKind;

/// What a cell is: which of these it is changes the glyphs, never the colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellRole {
    /// Part of the settled stack, or of the piece under the player's control.
    Filled,
    /// The landing preview. Terminals have no alpha, so this has to be a
    /// different glyph rather than a faded one.
    Ghost,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    /// The Guideline's own piece colours, as exact RGB.
    #[default]
    Guideline,
    /// The terminal's 16 ANSI slots, so pieces follow whatever palette the
    /// player has configured.
    SystemAnsi,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::Guideline, Theme::SystemAnsi];

    pub fn label(self) -> &'static str {
        match self {
            Theme::Guideline => "Guideline",
            Theme::SystemAnsi => "System ANSI",
        }
    }

    /// The colour for a piece.
    ///
    /// `Guideline` is the published palette as 24-bit colour, which needs a
    /// truecolor terminal; `SystemAnsi` uses symbolic codes, which every terminal
    /// renders from its own configured palette. The seven ANSI slots are chosen to
    /// stay distinguishable under an arbitrary user palette — no two pieces share a
    /// hue family, and L takes bright red rather than a second red so it reads
    /// apart from Z.
    pub fn color(self, kind: PieceKind) -> Color {
        match self {
            Theme::Guideline => match kind {
                PieceKind::I => Color::Rgb(0, 240, 240),
                PieceKind::O => Color::Rgb(240, 240, 0),
                PieceKind::T => Color::Rgb(160, 0, 240),
                PieceKind::S => Color::Rgb(0, 240, 0),
                PieceKind::Z => Color::Rgb(240, 0, 0),
                PieceKind::J => Color::Rgb(0, 0, 240),
                PieceKind::L => Color::Rgb(240, 160, 0),
            },
            Theme::SystemAnsi => match kind {
                PieceKind::I => Color::Cyan,
                PieceKind::O => Color::Yellow,
                PieceKind::T => Color::Magenta,
                PieceKind::S => Color::Green,
                PieceKind::Z => Color::Red,
                PieceKind::J => Color::Blue,
                PieceKind::L => Color::LightRed,
            },
        }
    }

    /// The empty-cell grid dots. Left symbolic in both themes so they sit against
    /// whatever background the terminal has.
    pub fn grid(self) -> Color {
        Color::DarkGray
    }

    /// The solid bar a completed row flashes as.
    pub fn flash(self) -> Color {
        Color::White
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Skin {
    #[default]
    SolidBlock,
    Shaded,
    /// Plain ASCII, for terminals with poor Unicode support.
    AsciiBracket,
    /// The piece's own letter, which identifies pieces without relying on colour
    /// at all.
    Letter,
}

impl Skin {
    pub const ALL: [Skin; 4] = [
        Skin::SolidBlock,
        Skin::Shaded,
        Skin::AsciiBracket,
        Skin::Letter,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Skin::SolidBlock => "Solid",
            Skin::Shaded => "Shaded",
            Skin::AsciiBracket => "ASCII",
            Skin::Letter => "Letters",
        }
    }

    /// The two glyphs making up one cell, left column first.
    pub fn cell(self, kind: PieceKind, role: CellRole) -> [&'static str; 2] {
        match (self, role) {
            (Skin::SolidBlock, CellRole::Filled) => ["█", "█"],
            (Skin::SolidBlock, CellRole::Ghost) => ["▒", "▒"],

            (Skin::Shaded, CellRole::Filled) => ["▓", "▓"],
            (Skin::Shaded, CellRole::Ghost) => ["░", "░"],

            (Skin::AsciiBracket, CellRole::Filled) => ["[", "]"],
            (Skin::AsciiBracket, CellRole::Ghost) => ["(", ")"],

            (Skin::Letter, CellRole::Filled) => [kind.letter(), " "],
            (Skin::Letter, CellRole::Ghost) => [lowercase_letter(kind), " "],
        }
    }

    /// The glyphs a completed row flashes as: a solid bar, or `#` for the two
    /// skins meant for terminals that cannot be trusted with block characters.
    pub fn flash(self) -> [&'static str; 2] {
        match self {
            Skin::AsciiBracket | Skin::Letter => ["#", "#"],
            _ => ["█", "█"],
        }
    }

    /// The two glyphs for an empty cell. Never painted with a background colour,
    /// so terminal transparency survives.
    pub fn empty(self) -> [&'static str; 2] {
        match self {
            Skin::AsciiBracket | Skin::Letter => [".", " "],
            _ => ["·", " "],
        }
    }
}

/// The ghost uses the lowercase form, so the two are distinguishable without
/// colour. Spelled out as `&'static str` for the same reason `PieceKind::letter`
/// is.
fn lowercase_letter(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::I => "i",
        PieceKind::O => "o",
        PieceKind::T => "t",
        PieceKind::S => "s",
        PieceKind::Z => "z",
        PieceKind::J => "j",
        PieceKind::L => "l",
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BorderStyle {
    None,
    Ascii,
    #[default]
    Single,
    Double,
    Rounded,
    Heavy,
}

/// Plain `+`/`-`/`|` corners, for the same terminals the ASCII skin is for.
const ASCII_BORDER: border::Set = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

impl BorderStyle {
    pub const ALL: [BorderStyle; 6] = [
        BorderStyle::None,
        BorderStyle::Ascii,
        BorderStyle::Single,
        BorderStyle::Double,
        BorderStyle::Rounded,
        BorderStyle::Heavy,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BorderStyle::None => "None",
            BorderStyle::Ascii => "ASCII",
            BorderStyle::Single => "Single",
            BorderStyle::Double => "Double",
            BorderStyle::Rounded => "Rounded",
            BorderStyle::Heavy => "Heavy",
        }
    }

    /// Apply this style to a block. Kept as one function so every panel borders
    /// consistently and nothing hand-rolls a `Borders::ALL`.
    pub fn apply(self, block: Block<'_>) -> Block<'_> {
        match self {
            BorderStyle::None => block.borders(Borders::NONE),
            BorderStyle::Ascii => block.borders(Borders::ALL).border_set(ASCII_BORDER),
            BorderStyle::Single => block.borders(Borders::ALL).border_set(border::PLAIN),
            BorderStyle::Double => block.borders(Borders::ALL).border_set(border::DOUBLE),
            BorderStyle::Rounded => block.borders(Borders::ALL).border_set(border::ROUNDED),
            BorderStyle::Heavy => block.borders(Borders::ALL).border_set(border::THICK),
        }
    }
}

/// The three axes together, which is what the drawing code is handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Visuals {
    pub theme: Theme,
    pub skin: Skin,
    pub border: BorderStyle,
}

impl Visuals {
    /// Whether the player picked either ASCII option, which is the only signal
    /// there is that the terminal cannot be trusted with Unicode. Backgrounds with
    /// Unicode glyphs fall back to ASCII ones when this is set.
    pub fn ascii_only(&self) -> bool {
        self.skin == Skin::AsciiBracket || self.border == BorderStyle::Ascii
    }

    /// Whether the menus and HUD keep to ASCII. That follows the border alone,
    /// since the border is the setting that already styles the menus; the skin
    /// is about the pieces, and changing it must not rewrite the interface.
    pub fn ascii_interface(&self) -> bool {
        self.border == BorderStyle::Ascii
    }

    /// Interface text as it should be drawn: unchanged normally, and with its
    /// arrows and punctuation spelled out in ASCII when `ascii_interface` is set.
    pub fn text<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if !self.ascii_interface() || text.is_ascii() {
            return Cow::Borrowed(text);
        }
        // Pairs first, so a hint's "↑↓" reads as one phrase, not two words.
        const PLAIN: [(&str, &str); 9] = [
            ("↑↓", "up/down"),
            ("←→", "left/right"),
            ("←", "Left"),
            ("→", "Right"),
            ("↑", "Up"),
            ("↓", "Down"),
            ("—", "-"),
            ("·", "|"),
            ("…", "..."),
        ];
        let mut plain = text.to_string();
        for (fancy, ascii) in PLAIN {
            plain = plain.replace(fancy, ascii);
        }
        Cow::Owned(plain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cell is two columns wide: the board's whole geometry depends on it.
    #[test]
    fn every_skin_renders_exactly_two_columns_per_cell() {
        for skin in Skin::ALL {
            for kind in PieceKind::ALL {
                for role in [CellRole::Filled, CellRole::Ghost] {
                    let cell = skin.cell(kind, role);
                    let width: usize = cell.iter().map(|g| g.chars().count()).sum();
                    assert_eq!(width, 2, "{skin:?} {kind:?} {role:?} -> {cell:?}");
                }
            }
            let width: usize = skin.empty().iter().map(|g| g.chars().count()).sum();
            assert_eq!(width, 2, "{skin:?} empty");
        }
    }

    /// Terminals have no alpha, so the ghost has to differ by glyph.
    #[test]
    fn the_ghost_is_never_the_same_glyph_as_a_filled_cell() {
        for skin in Skin::ALL {
            for kind in PieceKind::ALL {
                assert_ne!(
                    skin.cell(kind, CellRole::Filled),
                    skin.cell(kind, CellRole::Ghost),
                    "{skin:?} {kind:?}"
                );
            }
        }
    }

    /// The ASCII skin exists for terminals that cannot render Unicode; a stray
    /// multi-byte glyph in it would defeat the point.
    /// The skin is about the pieces: picking the ASCII one must not rewrite the
    /// menus. Only the ASCII border does that.
    #[test]
    fn only_the_ascii_border_spells_out_menu_arrows() {
        let ascii_skin = Visuals {
            skin: Skin::AsciiBracket,
            ..Default::default()
        };
        assert_eq!(ascii_skin.text("↑ / ↓"), "↑ / ↓");

        let ascii_border = Visuals {
            border: BorderStyle::Ascii,
            ..Default::default()
        };
        assert_eq!(ascii_border.text("↑ / ↓"), "Up / Down");
    }

    #[test]
    fn the_ascii_skin_is_actually_ascii() {
        for kind in PieceKind::ALL {
            for role in [CellRole::Filled, CellRole::Ghost] {
                for glyph in Skin::AsciiBracket.cell(kind, role) {
                    assert!(glyph.is_ascii(), "{glyph:?}");
                }
            }
        }
        for glyph in Skin::AsciiBracket.empty() {
            assert!(glyph.is_ascii());
        }
        for glyph in [
            ASCII_BORDER.top_left,
            ASCII_BORDER.horizontal_top,
            ASCII_BORDER.vertical_left,
            ASCII_BORDER.bottom_right,
        ] {
            assert!(glyph.is_ascii(), "{glyph:?}");
        }
    }

    /// Colour is what tells pieces apart in every skin but Letters, so no two may
    /// collide within a theme.
    #[test]
    fn no_two_pieces_share_a_colour_in_either_theme() {
        for theme in Theme::ALL {
            let mut seen = Vec::new();
            for kind in PieceKind::ALL {
                let color = theme.color(kind);
                assert!(!seen.contains(&color), "{theme:?} reuses {color:?}");
                seen.push(color);
            }
        }
    }

    /// The Letters skin is the colourblind / no-colour option, so its glyphs have
    /// to be unique on their own.
    #[test]
    fn letters_identify_pieces_without_colour() {
        let mut seen = Vec::new();
        for kind in PieceKind::ALL {
            let glyph = Skin::Letter.cell(kind, CellRole::Filled)[0];
            assert!(!seen.contains(&glyph), "{glyph} is used twice");
            seen.push(glyph);
        }
    }

    /// The ghost's lowercase letters are spelled out separately from
    /// `PieceKind::letter`; they must not drift from it.
    #[test]
    fn the_ghost_letters_match_the_pieces_own_letter() {
        for kind in PieceKind::ALL {
            assert_eq!(lowercase_letter(kind), kind.letter().to_lowercase());
        }
    }

    #[test]
    fn the_guideline_theme_uses_its_published_colours() {
        assert_eq!(
            Theme::Guideline.color(PieceKind::I),
            Color::Rgb(0, 240, 240)
        );
        assert_eq!(
            Theme::Guideline.color(PieceKind::L),
            Color::Rgb(240, 160, 0)
        );
        // The ANSI theme stays symbolic, so the terminal's own palette applies.
        assert_eq!(Theme::SystemAnsi.color(PieceKind::I), Color::Cyan);
    }

    #[test]
    fn labels_are_distinct_on_every_axis() {
        let themes: Vec<_> = Theme::ALL.iter().map(|t| t.label()).collect();
        let skins: Vec<_> = Skin::ALL.iter().map(|s| s.label()).collect();
        let borders: Vec<_> = BorderStyle::ALL.iter().map(|b| b.label()).collect();
        for mut labels in [themes, skins, borders] {
            let count = labels.len();
            labels.sort_unstable();
            labels.dedup();
            assert_eq!(labels.len(), count, "duplicate label");
        }
    }

    #[test]
    fn the_defaults_are_the_shipped_look() {
        let visuals = Visuals::default();
        assert_eq!(visuals.theme, Theme::Guideline);
        assert_eq!(visuals.skin, Skin::SolidBlock);
        assert_eq!(visuals.border, BorderStyle::Single);
    }
}
