//! Which click stream owns the mouse.
//!
//! Port of `Clicker`'s `_activity` field (`Aoko/Core/Clicker.cs:459-509`).
//!
//! In the managed client the click loop was not spawned by the module switch: the mouse
//! hook spawned it on the button press and cancelled it on release
//! (`InputHooks.cs:565-585`, `:606-625`). `StartClicking(button)` then refused to start a
//! second stream while one was running (`Clicker.cs:470`), so holding both buttons produced
//! exactly one stream — the one whose press came first — and the other button did nothing
//! until the first was released. The two engines live on their own threads here, so that
//! rule needs a cell both of them can see.

use std::sync::atomic::{AtomicU8, Ordering};

/// `ClickerActivity`: at most one stream runs at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickActivity {
    /// No stream: the modules are armed but no button is held.
    None,
    /// The left engine is clicking.
    Left,
    /// The right engine is clicking.
    Right,
}

impl ClickActivity {
    const fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Left => 1,
            Self::Right => 2,
        }
    }

    const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Left,
            2 => Self::Right,
            _ => Self::None,
        }
    }
}

/// The shared activity cell, written only by the two click engines.
///
/// Default is [`ClickActivity::None`], which is the state `Clicker` starts in.
#[derive(Debug, Default)]
pub struct ClickStream {
    activity: AtomicU8,
}

impl ClickStream {
    /// A cell with no stream running.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Which engine owns the mouse right now.
    #[must_use]
    pub fn activity(&self) -> ClickActivity {
        ClickActivity::from_code(self.activity.load(Ordering::Acquire))
    }

    /// `StartClicking`: claims the stream for one engine.
    ///
    /// Returns whether this engine now owns it. `false` means another stream is running, and
    /// the managed client's answer to that was to leave the press unserved rather than to
    /// switch streams (`Clicker.cs:470`): the button that was pressed first keeps clicking
    /// until it is released.
    #[must_use]
    pub fn claim(&self, left: bool) -> bool {
        let wanted = if left {
            ClickActivity::Left
        } else {
            ClickActivity::Right
        };
        self.activity
            .compare_exchange(
                ClickActivity::None.code(),
                wanted.code(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    /// `StopClicking`: gives the stream back, if this engine is the one holding it.
    ///
    /// Idempotent, and a no-op for the engine that does not own the cell, so a body can call
    /// it on every path out of its loop.
    pub fn release(&self, left: bool) {
        let mine = if left {
            ClickActivity::Left
        } else {
            ClickActivity::Right
        };
        let _ = self.activity.compare_exchange(
            mine.code(),
            ClickActivity::None.code(),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_stream_at_a_time_and_the_first_press_wins() {
        let stream = ClickStream::new();
        assert_eq!(stream.activity(), ClickActivity::None);

        assert!(stream.claim(true), "the left press starts the left stream");
        assert_eq!(stream.activity(), ClickActivity::Left);
        assert!(
            !stream.claim(false),
            "a right press while the left stream runs is refused, as StartClicking refused it"
        );
        assert_eq!(stream.activity(), ClickActivity::Left);

        stream.release(true);
        assert_eq!(stream.activity(), ClickActivity::None);
        assert!(
            stream.claim(false),
            "once the left button is released the right press can start its own"
        );
        assert_eq!(stream.activity(), ClickActivity::Right);
    }

    #[test]
    fn a_release_from_the_engine_that_does_not_own_it_changes_nothing() {
        let stream = ClickStream::new();
        assert!(stream.claim(true));

        stream.release(false);

        assert_eq!(
            stream.activity(),
            ClickActivity::Left,
            "StopClicking only stops the stream that is running"
        );
    }

    #[test]
    fn release_is_idempotent() {
        let stream = ClickStream::new();
        assert!(stream.claim(true));
        stream.release(true);
        stream.release(true);
        assert_eq!(stream.activity(), ClickActivity::None);
    }
}
