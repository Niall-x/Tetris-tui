//! Tetromino identity and the in-play piece, shared by both rulesets.
//!
//! Cell offsets live in the per-mode rotation modules (`nes::rotation` and
//! `modern::srs`) because the two systems disagree on both how many rotation
//! states a piece has and where its pivot sits. `state` is just an index into whichever
//! table the active mode owns.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PieceKind {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

impl PieceKind {
    pub const ALL: [PieceKind; 7] = [
        PieceKind::I,
        PieceKind::O,
        PieceKind::T,
        PieceKind::S,
        PieceKind::Z,
        PieceKind::J,
        PieceKind::L,
    ];

    /// A `&str` rather than a `char`, since the skins draw cells from string
    /// glyphs and would otherwise allocate one per cell per frame.
    pub fn letter(self) -> &'static str {
        match self {
            PieceKind::I => "I",
            PieceKind::O => "O",
            PieceKind::T => "T",
            PieceKind::S => "S",
            PieceKind::Z => "Z",
            PieceKind::J => "J",
            PieceKind::L => "L",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationDir {
    Cw,
    Ccw,
}

/// A piece in play. `x`/`y` locate its pivot on the board, y growing downward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivePiece {
    pub kind: PieceKind,
    pub state: usize,
    pub x: i32,
    pub y: i32,
}

impl ActivePiece {
    pub fn new(kind: PieceKind, state: usize, x: i32, y: i32) -> Self {
        Self { kind, state, x, y }
    }

    pub fn moved(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }

    pub fn with_state(self, state: usize) -> Self {
        Self { state, ..self }
    }
}
