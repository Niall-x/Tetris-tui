//! Tracking which actions are *currently held*, which terminals make harder than
//! it sounds.
//!
//! DAS timing needs to know that a direction is still down on a given frame. A
//! plain terminal never says so: it sends a press, then — after the OS's autorepeat
//! delay — a stream of repeats, and no release at all.
//!
//! Two modes:
//!
//! * `Precise` — the terminal supports the Kitty keyboard protocol, so we asked for
//!   event types and get real Press/Release pairs. Held state is then exact and DAS
//!   is frame-accurate.
//! * `Inferred` — no protocol support (plain xterm, many tmux/screen setups,
//!   VTE-based terminals). An action counts as held while press/repeat events keep
//!   arriving, and is treated as released after `RELEASE_GRACE` of silence. The
//!   grace has to exceed the OS autorepeat *delay*, so releases register late and
//!   DAS feel is approximate. This is a terminal limitation, not something more
//!   code fixes; the UI tells the player which mode they are in.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::action::Action;

/// Must comfortably exceed the OS autorepeat delay (commonly ~250ms) or a genuinely
/// held key looks released in the gap before repeats begin.
const RELEASE_GRACE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingMode {
    /// Real press/release events: frame-accurate.
    Precise,
    /// Timeout-inferred releases: approximate.
    Inferred,
}

impl TimingMode {
    pub fn label(self) -> &'static str {
        match self {
            TimingMode::Precise => "precise input",
            TimingMode::Inferred => "approx. input",
        }
    }
}

#[derive(Debug)]
pub struct HeldKeys {
    mode: TimingMode,
    /// In `Precise` mode the timestamp is unused; presence means held.
    held: HashMap<Action, Instant>,
}

impl HeldKeys {
    pub fn new(mode: TimingMode) -> Self {
        Self {
            mode,
            held: HashMap::new(),
        }
    }

    pub fn mode(&self) -> TimingMode {
        self.mode
    }

    pub fn press(&mut self, action: Action, now: Instant) {
        self.held.insert(action, now);
    }

    pub fn release(&mut self, action: Action) {
        self.held.remove(&action);
    }

    pub fn is_held(&self, action: Action, now: Instant) -> bool {
        match self.held.get(&action) {
            None => false,
            Some(&last_seen) => match self.mode {
                TimingMode::Precise => true,
                TimingMode::Inferred => now.duration_since(last_seen) < RELEASE_GRACE,
            },
        }
    }

    /// Forget every held action. Used when a run starts or ends: a key still down
    /// at the time must not carry into the next one.
    pub fn clear(&mut self) {
        self.held.clear();
    }

    /// Drop entries that have gone quiet, so the map does not grow unbounded in
    /// inferred mode.
    pub fn expire(&mut self, now: Instant) {
        if self.mode == TimingMode::Inferred {
            self.held
                .retain(|_, &mut last_seen| now.duration_since(last_seen) < RELEASE_GRACE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precise_mode_holds_until_an_explicit_release() {
        let mut keys = HeldKeys::new(TimingMode::Precise);
        let t0 = Instant::now();
        keys.press(Action::MoveLeft, t0);

        // Even long after the press, with no release, it is still held.
        let later = t0 + Duration::from_secs(5);
        assert!(keys.is_held(Action::MoveLeft, later));

        keys.release(Action::MoveLeft);
        assert!(!keys.is_held(Action::MoveLeft, later));
    }

    #[test]
    fn inferred_mode_releases_after_the_grace_window() {
        let mut keys = HeldKeys::new(TimingMode::Inferred);
        let t0 = Instant::now();
        keys.press(Action::MoveRight, t0);

        assert!(keys.is_held(Action::MoveRight, t0 + Duration::from_millis(100)));
        assert!(!keys.is_held(Action::MoveRight, t0 + Duration::from_millis(400)));
    }

    /// Autorepeat refreshes the timestamp, so a key held down stays held across the
    /// repeat stream.
    #[test]
    fn inferred_mode_stays_held_while_repeats_arrive() {
        let mut keys = HeldKeys::new(TimingMode::Inferred);
        let mut now = Instant::now();
        keys.press(Action::MoveRight, now);

        for _ in 0..20 {
            now += Duration::from_millis(33);
            keys.press(Action::MoveRight, now);
            assert!(keys.is_held(Action::MoveRight, now));
        }

        assert!(!keys.is_held(Action::MoveRight, now + Duration::from_millis(400)));
    }

    #[test]
    fn expiry_clears_stale_inferred_holds() {
        let mut keys = HeldKeys::new(TimingMode::Inferred);
        let t0 = Instant::now();
        keys.press(Action::SoftDrop, t0);
        keys.expire(t0 + Duration::from_millis(500));
        assert!(!keys.is_held(Action::SoftDrop, t0));
    }

    #[test]
    fn clearing_forgets_everything_held() {
        let mut keys = HeldKeys::new(TimingMode::Precise);
        let now = Instant::now();
        keys.press(Action::MoveLeft, now);
        keys.clear();
        assert!(!keys.is_held(Action::MoveLeft, now));
    }

    #[test]
    fn unpressed_actions_are_never_held() {
        let keys = HeldKeys::new(TimingMode::Precise);
        assert!(!keys.is_held(Action::SoftDrop, Instant::now()));
    }
}
