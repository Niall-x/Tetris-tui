//! The 7-bag randomiser.
//!
//! Each bag is a shuffled permutation of all seven pieces, dealt in order and
//! refilled when empty. The Guideline places no constraint on the first piece —
//! that is a Tetris The Grand Master Ace behaviour, not a Guideline one — so this
//! is a plain shuffle.
//!
//! Source: tetris.wiki/Random_Generator.

use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::collections::VecDeque;

use crate::engine::piece::PieceKind;

/// How many upcoming pieces the preview queue holds.
pub const PREVIEW_LEN: usize = 5;

#[derive(Debug)]
pub struct SevenBag {
    rng: SmallRng,
    queue: VecDeque<PieceKind>,
}

impl SevenBag {
    pub fn new() -> Self {
        Self::from_rng(SmallRng::from_entropy())
    }

    pub fn from_seed(seed: u64) -> Self {
        Self::from_rng(SmallRng::seed_from_u64(seed))
    }

    fn from_rng(rng: SmallRng) -> Self {
        let mut bag = Self {
            rng,
            queue: VecDeque::new(),
        };
        bag.refill_to_preview();
        bag
    }

    fn refill_to_preview(&mut self) {
        while self.queue.len() <= PREVIEW_LEN {
            let mut next = PieceKind::ALL;
            next.shuffle(&mut self.rng);
            self.queue.extend(next);
        }
    }

    pub fn next_piece(&mut self) -> PieceKind {
        let piece = self
            .queue
            .pop_front()
            .expect("queue is refilled to at least the preview length");
        self.refill_to_preview();
        piece
    }

    /// The upcoming pieces, spanning bag boundaries transparently.
    pub fn preview(&self) -> Vec<PieceKind> {
        self.queue.iter().copied().take(PREVIEW_LEN).collect()
    }
}

impl Default for SevenBag {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn every_seven_draws_from_a_bag_boundary_contain_each_piece_once() {
        let mut bag = SevenBag::from_seed(20260922);
        for _ in 0..500 {
            let mut seen: Vec<PieceKind> = (0..7).map(|_| bag.next_piece()).collect();
            seen.sort_by_key(|k| k.letter());
            let mut expected = PieceKind::ALL.to_vec();
            expected.sort_by_key(|k| k.letter());
            assert_eq!(seen, expected);
        }
    }

    /// The documented guarantee: no more than 12 pieces between one I and the next.
    #[test]
    fn the_gap_between_repeats_never_exceeds_twelve() {
        let mut bag = SevenBag::from_seed(7);
        let mut last_seen: HashMap<PieceKind, usize> = HashMap::new();
        let mut worst = 0;

        for index in 0..100_000 {
            let piece = bag.next_piece();
            if let Some(previous) = last_seen.insert(piece, index) {
                worst = worst.max(index - previous - 1);
            }
        }
        assert!(worst <= 12, "saw a drought of {worst}");
    }

    /// Runs of S and Z are limited to four, which is part of why the bag feels
    /// fairer than the NES randomiser.
    #[test]
    fn s_and_z_runs_are_bounded() {
        let mut bag = SevenBag::from_seed(99);
        let mut run = 0;
        let mut worst = 0;
        for _ in 0..100_000 {
            match bag.next_piece() {
                PieceKind::S | PieceKind::Z => {
                    run += 1;
                    worst = worst.max(run);
                }
                _ => run = 0,
            }
        }
        assert!(worst <= 4, "saw an S/Z run of {worst}");
    }

    #[test]
    fn the_distribution_is_even() {
        let mut bag = SevenBag::from_seed(4242);
        let mut counts: HashMap<PieceKind, u32> = HashMap::new();
        let total = 70_000;
        for _ in 0..total {
            *counts.entry(bag.next_piece()).or_insert(0) += 1;
        }
        for kind in PieceKind::ALL {
            let share = counts[&kind] as f64 / total as f64;
            assert!(
                (0.135..0.15).contains(&share),
                "{kind:?} share was {share:.4}, expected about 1/7"
            );
        }
    }

    #[test]
    fn the_preview_shows_the_pieces_that_actually_arrive() {
        let mut bag = SevenBag::from_seed(11);
        let preview = bag.preview();
        assert_eq!(preview.len(), PREVIEW_LEN);
        for expected in preview {
            assert_eq!(bag.next_piece(), expected);
        }
    }

    #[test]
    fn the_preview_stays_full_across_bag_boundaries() {
        let mut bag = SevenBag::from_seed(3);
        for _ in 0..50 {
            assert_eq!(bag.preview().len(), PREVIEW_LEN);
            bag.next_piece();
        }
    }

    #[test]
    fn a_seed_reproduces_the_same_sequence() {
        let mut a = SevenBag::from_seed(1234);
        let mut b = SevenBag::from_seed(1234);
        let left: Vec<PieceKind> = (0..50).map(|_| a.next_piece()).collect();
        let right: Vec<PieceKind> = (0..50).map(|_| b.next_piece()).collect();
        assert_eq!(left, right);
    }
}
