//! Press latch for a bound *action* key (`InputHooks.PressLatch`,
//! `Aoko/Core/InputHooks.cs:202-226`).
//!
//! AutoRod has two binds: the module key that toggles it, and an action key that
//! presses-and-holds the rod use. The action key must trigger exactly once per
//! physical press even though the hook sees key-repeat frames, and the release
//! must be reported even when the press was not consumed. That is this latch,
//! ported as-is.

/// Per-key latch state.
///
/// One instance per action bind. Not `Clone`: the C# latch is a mutable singleton
/// per key, and copying it would silently double-trigger a press.
#[derive(Debug, Default)]
pub struct PressLatch {
    is_down: bool,
    consume: bool,
}

/// Outcome of [`PressLatch::begin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatchBegin {
    /// Whether the module owns this press.
    pub consume: bool,
    /// Whether this is the first frame of the press and the module owns it.
    pub trigger: bool,
}

impl PressLatch {
    /// A released latch.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `PressLatch.Begin`: called on every frame the key is seen down.
    ///
    /// `can_consume` is the runtime gate (see
    /// [`super::guards::should_consume_auto_rod_action`]). A press that starts
    /// while the gate is closed is *passed through* to the game and never becomes a
    /// trigger, even if the gate opens while the key is still held.
    pub fn begin(&mut self, can_consume: bool) -> LatchBegin {
        if self.is_down {
            return LatchBegin {
                consume: self.consume,
                trigger: false,
            };
        }

        self.is_down = true;
        self.consume = can_consume;
        LatchBegin {
            consume: self.consume,
            trigger: can_consume,
        }
    }

    /// `PressLatch.End`: called when the key is seen up.
    ///
    /// Returns whether the module owned the press, so the caller knows it still has
    /// to emit the release.
    pub fn end(&mut self) -> bool {
        let consume = self.consume;
        self.is_down = false;
        self.consume = false;
        consume
    }

    /// Whether a press is currently latched.
    #[must_use]
    pub fn is_down(&self) -> bool {
        self.is_down
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `AutoRodManagedTests.ActionLatch_TriggersOnceUntilReleaseAndKeepsConsumptionPaired`.
    #[test]
    fn the_latch_triggers_once_until_release_and_keeps_consumption_paired() {
        let mut latch = PressLatch::new();

        let first = latch.begin(true);
        assert!(first.consume);
        assert!(first.trigger);

        let repeat = latch.begin(true);
        assert!(repeat.consume, "a held key stays owned");
        assert!(!repeat.trigger, "key repeat must not retrigger");

        assert!(latch.end(), "the release belongs to the module");
        assert!(!latch.is_down());

        let second = latch.begin(true);
        assert!(second.trigger, "a new press triggers again");
    }

    #[test]
    fn a_press_that_starts_gated_stays_passed_through() {
        let mut latch = PressLatch::new();

        let blocked = latch.begin(false);
        assert!(!blocked.consume);
        assert!(!blocked.trigger);

        // The gate opening mid-press must not steal the key from the game.
        let still_held = latch.begin(true);
        assert!(!still_held.consume);
        assert!(!still_held.trigger);

        assert!(!latch.end(), "nothing to release");
        assert!(!latch.is_down());
    }

    #[test]
    fn end_is_idempotent() {
        let mut latch = PressLatch::new();
        assert!(latch.begin(true).trigger);
        assert!(latch.end());
        assert!(!latch.end());
    }
}
