//! Delayed Auto Shift, driven one frame at a time.
//!
//! The charge/repeat shape is common to both rulesets; only the constants differ,
//! so they are passed in rather than baked here. NES's charge deliberately
//! survives between pieces (see `NesGame`), so this holds no per-piece state.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
}

impl Direction {
    pub fn dx(self) -> i32 {
        match self {
            Direction::Left => -1,
            Direction::Right => 1,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DasProfile {
    /// Frames a direction must be held before auto-shift starts.
    pub charge_frames: u32,
    /// Value the counter drops back to after each auto-shift, so repeats land
    /// every `charge_frames - recharge_frames` frames.
    pub recharge_frames: u32,
}

/// What the game should do with the active piece this frame.
///
/// Taps and auto-shift repeats are distinct because only a repeat feeds the
/// counter back to `recharge_frames`. A tap starts the charge from zero, so the
/// first repeat is a full `charge_frames` away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DasAction {
    None,
    Tap(Direction),
    AutoShift(Direction),
}

impl DasAction {
    pub fn direction(self) -> Option<Direction> {
        match self {
            DasAction::None => None,
            DasAction::Tap(d) | DasAction::AutoShift(d) => Some(d),
        }
    }

    pub fn is_auto_shift(self) -> bool {
        matches!(self, DasAction::AutoShift(_))
    }
}

#[derive(Debug, Clone)]
pub struct Das {
    profile: DasProfile,
    direction: Option<Direction>,
    charge: u32,
}

impl Das {
    pub fn new(profile: DasProfile) -> Self {
        Self {
            profile,
            direction: None,
            charge: 0,
        }
    }

    pub fn charge(&self) -> u32 {
        self.charge
    }

    pub fn direction(&self) -> Option<Direction> {
        self.direction
    }

    /// Advance one frame. `held` is the direction currently held, if any: when both
    /// keys are down the caller decides which wins.
    pub fn update(&mut self, held: Option<Direction>) -> DasAction {
        match held {
            None => {
                self.direction = None;
                self.charge = 0;
                DasAction::None
            }
            Some(dir) => {
                if self.direction != Some(dir) {
                    // Fresh press: shift immediately and start charging from zero.
                    self.direction = Some(dir);
                    self.charge = 0;
                    return DasAction::Tap(dir);
                }

                self.charge += 1;
                if self.charge >= self.profile.charge_frames {
                    DasAction::AutoShift(dir)
                } else {
                    DasAction::None
                }
            }
        }
    }

    /// Called after an auto-shift actually moved the piece.
    pub fn on_shift_succeeded(&mut self) {
        self.charge = self.profile.recharge_frames;
    }

    /// Called when an auto-shift was blocked by a wall or the stack. The charge
    /// stays topped up, which is what lets a player hold into a wall and then move
    /// instantly on release — NES's "wall charge".
    pub fn on_shift_blocked(&mut self) {
        self.charge = self.profile.charge_frames;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NES: DasProfile = DasProfile {
        charge_frames: 16,
        recharge_frames: 10,
    };

    #[test]
    fn first_press_shifts_immediately() {
        let mut das = Das::new(NES);
        assert_eq!(
            das.update(Some(Direction::Left)),
            DasAction::Tap(Direction::Left)
        );
    }

    /// The documented cadence: an initial tap, 16 frames of charge, then a repeat
    /// every 6 frames.
    #[test]
    fn nes_charges_for_sixteen_frames_then_repeats_every_six() {
        let mut das = Das::new(NES);
        assert_eq!(
            das.update(Some(Direction::Right)),
            DasAction::Tap(Direction::Right)
        );

        for frame in 1..16 {
            assert_eq!(
                das.update(Some(Direction::Right)),
                DasAction::None,
                "frame {frame} should still be charging"
            );
        }

        assert_eq!(
            das.update(Some(Direction::Right)),
            DasAction::AutoShift(Direction::Right)
        );
        das.on_shift_succeeded();

        for _ in 0..5 {
            assert_eq!(das.update(Some(Direction::Right)), DasAction::None);
        }
        assert_eq!(
            das.update(Some(Direction::Right)),
            DasAction::AutoShift(Direction::Right)
        );
    }

    /// A tap must not borrow the repeat cadence: after the initial shift the
    /// counter starts at zero, so the first repeat is 16 frames away, not 6.
    #[test]
    fn a_tap_does_not_shorten_the_first_repeat() {
        let mut das = Das::new(NES);
        assert_eq!(
            das.update(Some(Direction::Left)),
            DasAction::Tap(Direction::Left)
        );
        assert_eq!(das.charge(), 0);

        for _ in 0..15 {
            assert_eq!(das.update(Some(Direction::Left)), DasAction::None);
        }
        assert_eq!(
            das.update(Some(Direction::Left)),
            DasAction::AutoShift(Direction::Left)
        );
    }

    #[test]
    fn releasing_clears_the_charge() {
        let mut das = Das::new(NES);
        das.update(Some(Direction::Left));
        for _ in 0..10 {
            das.update(Some(Direction::Left));
        }
        assert!(das.charge() > 0);
        das.update(None);
        assert_eq!(das.charge(), 0);
    }

    #[test]
    fn changing_direction_restarts_the_charge_with_an_immediate_shift() {
        let mut das = Das::new(NES);
        das.update(Some(Direction::Left));
        for _ in 0..12 {
            das.update(Some(Direction::Left));
        }
        assert_eq!(
            das.update(Some(Direction::Right)),
            DasAction::Tap(Direction::Right)
        );
        assert_eq!(das.charge(), 0);
    }

    /// Holding into a wall keeps the charge at maximum, so the next legal shift is
    /// instant rather than needing a fresh 16-frame charge.
    #[test]
    fn blocked_shifts_stay_charged_against_a_wall() {
        let mut das = Das::new(NES);
        das.update(Some(Direction::Left));
        for _ in 0..15 {
            das.update(Some(Direction::Left));
        }
        assert_eq!(
            das.update(Some(Direction::Left)),
            DasAction::AutoShift(Direction::Left)
        );
        das.on_shift_blocked();
        // Still charged, so the very next frame tries again.
        assert_eq!(
            das.update(Some(Direction::Left)),
            DasAction::AutoShift(Direction::Left)
        );
    }
}
