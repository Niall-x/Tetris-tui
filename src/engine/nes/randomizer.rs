//! The NES Tetris piece randomiser.
//!
//! This is deliberately not a 7-bag and deliberately not `rand`: the long droughts
//! and slight per-piece bias it produces are gameplay-defining, so the ROM's actual
//! algorithm is reproduced step for step.
//!
//! PRNG: 16-bit Fibonacci LFSR seeded with 0x8988. Each step XORs bits 1 and 9
//! (counting from the right, zero-based), places the result in bit 15, and shifts
//! right by one. The ROM steps it at least once per frame, so how long the player
//! takes to place a piece changes what comes next — `advance_frame` exists so the
//! game loop can preserve that.
//!
//! Selection:
//!   1. Take the high byte of the current PRNG value.
//!   2. Add the session's total spawn count (a wrapping byte, not reset per game).
//!   3. Reduce modulo 8.
//!   4. Accept it unless it is 7 (not a valid piece ID) or repeats the previous
//!      piece. Otherwise reroll: step the PRNG once, take the high byte, keep its
//!      low 3 bits, add the *spawn orientation ID* of the previous piece, and
//!      reduce modulo 7. That result is accepted unconditionally, which is why NES
//!      Tetris can still repeat a piece back to back.
//!
//! Sources: fractal161, "NES Tetris RNG" (§3.1–3.2), which derives this from the
//! ROM; cross-checked against meatfighter.com/nintendotetrisai ($9907, seed
//! 0x8988, LFSR formula, and the same reroll structure).

use super::rotation;
use crate::engine::piece::PieceKind;

pub const INITIAL_SEED: u16 = 0x8988;

/// Piece IDs follow the order of the in-game statistics bar, which is also the
/// order of the ROM's orientation table: T, J, Z, O, S, L, I.
const PIECE_BY_ID: [PieceKind; 7] = [
    PieceKind::T,
    PieceKind::J,
    PieceKind::Z,
    PieceKind::O,
    PieceKind::S,
    PieceKind::L,
    PieceKind::I,
];

fn piece_id(kind: PieceKind) -> u8 {
    PIECE_BY_ID.iter().position(|&k| k == kind).unwrap() as u8
}

/// One step of the ROM's 16-bit Fibonacci LFSR.
pub fn step_lfsr(value: u16) -> u16 {
    let feedback = ((value >> 1) ^ (value >> 9)) & 1;
    (feedback << 15) | (value >> 1)
}

#[derive(Debug, Clone)]
pub struct NesRandomizer {
    lfsr: u16,
    spawn_count: u8,
    previous: PieceKind,
}

impl NesRandomizer {
    pub fn new() -> Self {
        Self::with_seed(INITIAL_SEED)
    }

    pub fn with_seed(seed: u16) -> Self {
        Self {
            lfsr: seed,
            spawn_count: 0,
            // The ROM starts with a T in the "previous piece" slot.
            previous: PieceKind::T,
        }
    }

    pub fn lfsr(&self) -> u16 {
        self.lfsr
    }

    /// Step the PRNG once, as the ROM does every frame. Feeding real frame timing
    /// through this is what makes piece order depend on how the player plays.
    pub fn advance_frame(&mut self) {
        self.lfsr = step_lfsr(self.lfsr);
    }

    pub fn next_piece(&mut self) -> PieceKind {
        self.spawn_count = self.spawn_count.wrapping_add(1);

        let high = (self.lfsr >> 8) as u8;
        let mut index = high.wrapping_add(self.spawn_count) % 8;

        if index == 7 || index == piece_id(self.previous) {
            self.lfsr = step_lfsr(self.lfsr);
            let high = (self.lfsr >> 8) as u8;
            let previous_orientation = rotation::piece_data(self.previous).spawn_orientation_id;
            index = (high & 0b111).wrapping_add(previous_orientation) % 7;
        }

        let piece = PIECE_BY_ID[index as usize];
        self.previous = piece;
        piece
    }
}

impl Default for NesRandomizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The worked example from the RNG paper: 0b1000100110001000 steps to
    /// 0b0100010011000100.
    #[test]
    fn lfsr_matches_the_documented_worked_example() {
        assert_eq!(step_lfsr(0b1000_1001_1000_1000), 0b0100_0100_1100_0100);
    }

    #[test]
    fn lfsr_has_the_documented_period_of_32767() {
        let start = INITIAL_SEED;
        let mut value = start;
        let mut steps: u32 = 0;
        loop {
            value = step_lfsr(value);
            steps += 1;
            if value == start || steps > 70_000 {
                break;
            }
        }
        assert_eq!(steps, 32_767);
    }

    /// Two pieces never spawn on the same frame: there is entry delay plus however
    /// long the player takes to place the previous one. The ROM steps the PRNG on
    /// every one of those frames, and its output only looks random because of it,
    /// so any test about distribution has to sample it the same way.
    fn spawn_after_playing(rng: &mut NesRandomizer, nth: u32) -> PieceKind {
        let frames = 18 + (nth.wrapping_mul(13) ^ nth.wrapping_mul(nth)) % 47;
        for _ in 0..frames {
            rng.advance_frame();
        }
        rng.next_piece()
    }

    #[test]
    fn produces_only_valid_pieces() {
        let mut rng = NesRandomizer::new();
        for i in 0..5_000 {
            let piece = spawn_after_playing(&mut rng, i);
            assert!(PieceKind::ALL.contains(&piece));
        }
    }

    /// Without per-frame stepping the high byte barely moves and the spawn counter
    /// dominates, degenerating into a near-cycle. That is a property of the real
    /// algorithm, not a bug — but it means the game loop must step the PRNG every
    /// frame, so pin it here to keep that requirement visible.
    #[test]
    fn starves_without_per_frame_stepping() {
        let mut rng = NesRandomizer::new();
        let mut longest_drought = 0;
        let mut gap = 0;
        for _ in 0..10_000 {
            if rng.next_piece() == PieceKind::I {
                gap = 0;
            } else {
                gap += 1;
                longest_drought = longest_drought.max(gap);
            }
        }
        assert!(
            longest_drought < 12,
            "unstepped PRNG should degenerate, got drought of {longest_drought}"
        );
    }

    /// The reroll accepts its result unconditionally, so unlike 7-bag the same
    /// piece can still arrive twice in a row. Guards against someone quietly
    /// swapping a bag-shaped implementation in here.
    #[test]
    fn immediate_repeats_are_possible() {
        let mut rng = NesRandomizer::new();
        let mut previous = rng.next_piece();
        let mut repeats = 0;
        for i in 0..20_000 {
            let piece = spawn_after_playing(&mut rng, i);
            if piece == previous {
                repeats += 1;
            }
            previous = piece;
        }
        assert!(repeats > 0, "expected some back-to-back repeats");
    }

    /// Droughts are the defining feel of this randomiser: 7-bag can never go more
    /// than 12 pieces without a given type, and this must.
    #[test]
    fn long_droughts_occur() {
        let mut rng = NesRandomizer::new();
        let mut gap = 0;
        let mut longest = 0;
        for i in 0..50_000 {
            if spawn_after_playing(&mut rng, i) == PieceKind::I {
                gap = 0;
            } else {
                gap += 1;
                longest = longest.max(gap);
            }
        }
        assert!(
            longest > 12,
            "longest I drought was {longest}, expected well over a bag's worth"
        );
    }

    /// meatfighter's analysis of this algorithm gives T and S ~14.73% each and
    /// I and L ~13.84% each. Checked loosely: the point is the shape of the bias,
    /// not bit-exact convergence.
    #[test]
    fn distribution_is_biased_toward_t_and_s_away_from_i_and_l() {
        let mut rng = NesRandomizer::new();
        let mut counts: HashMap<PieceKind, u32> = HashMap::new();
        let total = 400_000;
        for i in 0..total {
            *counts.entry(spawn_after_playing(&mut rng, i)).or_insert(0) += 1;
        }

        let share = |kind: PieceKind| *counts.get(&kind).unwrap_or(&0) as f64 / total as f64;

        for kind in PieceKind::ALL {
            let s = share(kind);
            assert!(
                (0.13..0.156).contains(&s),
                "{kind:?} share {s:.4} is outside the expected band"
            );
        }

        assert!(share(PieceKind::T) > share(PieceKind::I));
        assert!(share(PieceKind::S) > share(PieceKind::I));
        assert!(share(PieceKind::T) > share(PieceKind::L));
        assert!(share(PieceKind::S) > share(PieceKind::L));
    }

    /// Frame-by-frame PRNG stepping must change the resulting sequence: this is the
    /// "how you play changes what you get" property.
    #[test]
    fn frame_advances_change_the_sequence() {
        let mut steady = NesRandomizer::new();
        let mut fidgety = NesRandomizer::new();

        let a: Vec<PieceKind> = (0..20).map(|_| steady.next_piece()).collect();
        let b: Vec<PieceKind> = (0..20)
            .map(|_| {
                for _ in 0..7 {
                    fidgety.advance_frame();
                }
                fidgety.next_piece()
            })
            .collect();

        assert_ne!(a, b);
    }
}
