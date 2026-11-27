//! Refill: shift-clicking healing items from the backpack into the hotbar.
//!
//! Port of `Aoko/Core/RefillController.cs`. The C# class both decides and injects
//! (`SendInput`, `SetCursorPos`, `GetAsyncKeyState`); here the decisions are split
//! out so they can be tested with a virtual clock, and the injection is left to
//! `aoko_platform::InputBackend`. What is preserved exactly:
//!
//! * the gate (`RefillController.cs:150-166`) and its 45 ms idle poll (`:174`);
//! * the 30 ms poll when every pot is on cooldown (`:222`);
//! * the 150 ms slot cooldown, which is longer than the chest stealer's 85 ms
//!   because a shift-click moves items and the bridge needs a re-scan
//!   (`:83`, `:229`);
//! * the click shape: cursor jitter ±2 px, shift settle 4..9 ms, press held
//!   8..17 ms, then the remaining slice of `RefillDelayMs` (`:243-291`);
//! * the synthetic-Shift ownership rule: the flag is only cleared after the
//!   release is *accepted*, so a failed release is retried (`:318-326`).
//!
//! # Nothing drives this module yet
//!
//! As with [`crate::chest_stealer`], the loop that would use these pieces does not exist:
//! `loops/` has no refill body and `aoko-app::runtime::loop_for` has no
//! `EnabledFlag::Refill` arm, so the bridge's `refillState` (published only while the
//! module is on, `McInjector/src/main/cpp/bridge.cpp:10057`) is read by nobody and no
//! shift-click is ever sent. The two missing platform inputs are the same ones: an
//! absolute pointer placement for [`plan_click`]'s coordinates, and the foreground
//! window's client rect, which [`crate::loops::LoopSense`] does not carry.

use aoko_protocol::state::{ChestStealerSlot, GameState, RefillState};

use crate::chest_stealer::{map_scaled_point, NotReady, RetryBlocks, ScreenRect, FRESH_STATE_MS};
use crate::clock::{next_below, Rng};

/// Poll interval while the container is not usable (`RefillController.cs:174`).
pub const IDLE_POLL_MS: u64 = 45;
/// Poll interval when every reported slot is cooling down (`:222`).
pub const NO_SLOT_POLL_MS: u64 = 30;
/// Back-off after an unexpected error (`:262`).
pub const ERROR_BACKOFF_MS: u64 = 150;
/// Per-slot cooldown after a shift-click (`:83`).
pub const SLOT_RETRY_COOLDOWN_MS: u64 = 150;
/// Default `RefillDelayMs` (`Clicker.cs:1189`).
pub const DEFAULT_DELAY_MS: i32 = 120;
/// `RefillDelayMs` clamp (`Clicker.cs:1195`).
pub const DELAY_RANGE_MS: (i32, i32) = (50, 500);
/// Cursor jitter before the click (`RefillController.cs:247-248`).
pub const CURSOR_JITTER_PX: i32 = 2;
/// Shift-settle wait, `Random.Next(4, 10)` (`:253`).
pub const SHIFT_SETTLE_MS: (u32, u32) = (4, 10);
/// Press hold, `Random.Next(8, 18)` (`:261`).
pub const PRESS_HOLD_MS: (u32, u32) = (8, 18);

/// `Clicker.RefillDelayMs`'s setter: `Math.Clamp(value, 50, 500)`.
#[must_use]
pub fn normalize_delay_ms(value: i32) -> i32 {
    value.clamp(DELAY_RANGE_MS.0, DELAY_RANGE_MS.1)
}

/// `Task.Delay(Math.Max(1, RefillDelayMs - clickElapsedMs))` (`:231`).
///
/// The elapsed click time is subtracted from the cadence rather than added to it,
/// so a slow click does not make the next one late as well.
#[must_use]
pub fn next_wait_ms(delay_ms: i32, click_elapsed_ms: i32) -> u64 {
    (normalize_delay_ms(delay_ms) - click_elapsed_ms).max(1) as u64
}

/// One prepared shift-click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickPlan {
    /// Screen x after jitter.
    pub x: i32,
    /// Screen y after jitter.
    pub y: i32,
    /// How long the shift press should settle before the mouse button goes down.
    pub settle_ms: u64,
    /// How long the left button stays down.
    pub hold_ms: u64,
}

/// Builds a click for a slot, mirroring `ClickSlotAsync`'s prologue (`:243-261`).
///
/// Returns `None` when the slot cannot be mapped, which the C# method reports as
/// `-1` and which must not update the retry cooldown.
pub fn plan_click(
    screen_width: i32,
    screen_height: i32,
    slot: &ChestStealerSlot,
    rect: ScreenRect,
    rng: &mut dyn Rng,
) -> Option<ClickPlan> {
    let (base_x, base_y) = map_scaled_point(screen_width, screen_height, slot.x, slot.y, rect)?;

    // `_random.Next(-2, 3)`: inclusive -2..=2.
    let jitter_x = next_below(rng, 5) as i32 - CURSOR_JITTER_PX;
    let jitter_y = next_below(rng, 5) as i32 - CURSOR_JITTER_PX;

    let settle_ms =
        u64::from(next_below(rng, SHIFT_SETTLE_MS.1 - SHIFT_SETTLE_MS.0) + SHIFT_SETTLE_MS.0);
    let hold_ms = u64::from(next_below(rng, PRESS_HOLD_MS.1 - PRESS_HOLD_MS.0) + PRESS_HOLD_MS.0);

    Some(ClickPlan {
        x: base_x + jitter_x,
        y: base_y + jitter_y,
        settle_ms,
        hold_ms,
    })
}

/// What the loop should do about the synthetic Shift key (`:300-326`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftAction {
    /// Shift is already down, either genuinely or synthetically: send nothing new
    /// unless the synthetic press needs re-sending.
    None,
    /// Send a shift-down.
    Press,
}

/// Ownership state for the synthetic Shift press.
///
/// Two invariants from the C# code: a real Shift press means we never claim
/// ownership, and a failed release keeps ownership so the next loop iteration
/// retries it instead of leaving Shift stuck down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyntheticShift {
    held: bool,
}

impl SyntheticShift {
    /// No synthetic press outstanding.
    #[must_use]
    pub fn new() -> Self {
        Self { held: false }
    }

    /// Whether the synthetic press is currently owned.
    #[must_use]
    pub fn is_held(self) -> bool {
        self.held
    }

    /// `EnsureShiftHeld` (`:300-316`) as a decision.
    ///
    /// A genuine Shift press short-circuits: the user is holding it, so the loop
    /// must not take ownership. If we already own the press, the C# code re-sends
    /// shift-down every time (some clients drop a synthetic press when the window
    /// focus changes without telling us), so the caller sends it again.
    #[must_use]
    pub fn action(self, real_shift_down: bool) -> ShiftAction {
        if real_shift_down && !self.held {
            ShiftAction::None
        } else {
            ShiftAction::Press
        }
    }

    /// Records the result of sending shift-down.
    pub fn note_press(&mut self, accepted: bool) {
        if accepted {
            self.held = true;
        }
    }

    /// Records the result of sending shift-up. Returns whether the press is still
    /// owned, i.e. whether the caller has to retry.
    pub fn note_release(&mut self, accepted: bool) -> bool {
        if accepted {
            self.held = false;
        }
        self.held
    }

    /// Forgets ownership without touching the keyboard. Only for a client that
    /// already released the key out of band; the normal path is [`Self::note_release`].
    pub fn forget(&mut self) {
        self.held = false;
    }
}

/// The refill gate (`RefillController.cs:150-166`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefillGate {
    /// `RefillEnabled`.
    pub enabled: bool,
    /// Transport connected.
    pub connected: bool,
    /// Game window foreground.
    pub foreground: bool,
    /// `GuiOpen || cursor visible`.
    pub gui_active: bool,
    /// Age of the snapshot, `None` when it was never updated.
    pub state_age_ms: Option<u64>,
}

impl RefillGate {
    /// Evaluates the gate against a snapshot.
    pub fn check<'a>(&self, state: &'a GameState) -> Result<&'a RefillState, NotReady> {
        if !self.enabled {
            return Err(NotReady::Disabled);
        }
        if !self.connected {
            return Err(NotReady::Disconnected);
        }
        if !self.foreground {
            return Err(NotReady::Background);
        }
        if !self.gui_active {
            return Err(NotReady::NoGui);
        }
        let Some(age) = self.state_age_ms else {
            return Err(NotReady::NeverUpdated);
        };
        if age > FRESH_STATE_MS {
            return Err(NotReady::Stale);
        }

        let Some(refill) = state.refill_state.as_ref() else {
            return Err(NotReady::NoContainer);
        };
        if !refill.ready {
            return Err(NotReady::NotReady);
        }
        if refill.slots.is_empty() {
            return Err(NotReady::NoSlots);
        }

        Ok(refill)
    }
}

/// Records a click against the retry set, using the refill cooldown.
pub fn note_click(blocked: &mut RetryBlocks, slot_number: i32, now_ms: u64) {
    blocked.block_for(slot_number, now_ms, SLOT_RETRY_COOLDOWN_MS);
}

/// Shared tracker for the container window id, so a new inventory screen clears
/// stale cooldowns (`RefillController.cs:176-180`).
#[derive(Debug)]
pub struct RefillWindow {
    active_window_id: i32,
}

/// The managed loop starts at `-1`, the "no container" id
/// (`RefillController.cs:150`), which is also what [`RefillWindow::reset`] restores and
/// [`RefillWindow::active_window_id`] promises. A derived default would be `0`, a real
/// window id the loop has never seen.
impl Default for RefillWindow {
    fn default() -> Self {
        Self {
            active_window_id: -1,
        }
    }
}

impl RefillWindow {
    /// No window seen yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Notices a container change. Returns whether the window changed.
    pub fn observe(&mut self, window_id: i32, blocked: &mut RetryBlocks) -> bool {
        if window_id == self.active_window_id {
            return false;
        }
        self.active_window_id = window_id;
        blocked.clear();
        true
    }

    /// Drops the active window and its cooldowns, as the closed-gate path does.
    pub fn reset(&mut self, blocked: &mut RetryBlocks) {
        self.active_window_id = -1;
        blocked.clear();
    }

    /// Active window id, `-1` when none.
    #[must_use]
    pub fn active_window_id(&self) -> i32 {
        self.active_window_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SplitMix64;

    fn rect() -> ScreenRect {
        ScreenRect {
            left: 100,
            top: 50,
            right: 1700,
            bottom: 950,
        }
    }

    fn slot(slot_number: i32, x: i32, y: i32) -> ChestStealerSlot {
        ChestStealerSlot {
            index: slot_number,
            slot_number,
            x,
            y,
        }
    }

    #[test]
    fn the_delay_is_clamped_like_the_property_setter() {
        assert_eq!(normalize_delay_ms(0), 50);
        assert_eq!(normalize_delay_ms(49), 50);
        assert_eq!(normalize_delay_ms(120), 120);
        assert_eq!(normalize_delay_ms(501), 500);
        assert_eq!(normalize_delay_ms(i32::MAX), 500);
        assert_eq!(DEFAULT_DELAY_MS, 120);
    }

    #[test]
    fn the_wait_subtracts_the_click_time_and_never_reaches_zero() {
        assert_eq!(next_wait_ms(120, 0), 120);
        assert_eq!(next_wait_ms(120, 30), 90);
        assert_eq!(next_wait_ms(120, 120), 1);
        // A click slower than the cadence still yields a positive wait, so the
        // loop cannot spin.
        assert_eq!(next_wait_ms(120, 500), 1);
        assert_eq!(next_wait_ms(10, 0), 50);
    }

    #[test]
    fn a_click_plan_jitters_within_two_pixels_and_times_the_press() {
        let mut rng = SplitMix64::new(11);
        let slot = slot(0, 800, 450);
        let exact = map_scaled_point(1600, 900, 800, 450, rect()).expect("in range");

        for _ in 0..200 {
            let plan = plan_click(1600, 900, &slot, rect(), &mut rng).expect("mappable");
            assert!((plan.x - exact.0).abs() <= CURSOR_JITTER_PX);
            assert!((plan.y - exact.1).abs() <= CURSOR_JITTER_PX);
            assert!((SHIFT_SETTLE_MS.0 as u64..SHIFT_SETTLE_MS.1 as u64).contains(&plan.settle_ms));
            assert!((PRESS_HOLD_MS.0 as u64..PRESS_HOLD_MS.1 as u64).contains(&plan.hold_ms));
        }
    }

    #[test]
    fn an_unmappable_slot_produces_no_plan_and_no_cooldown() {
        let mut rng = SplitMix64::new(3);
        // Outside the reported screen space.
        assert!(plan_click(1600, 900, &slot(0, 1600, 450), rect(), &mut rng).is_none());
        // Degenerate client rect.
        let zero = ScreenRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        assert!(plan_click(1600, 900, &slot(0, 10, 10), zero, &mut rng).is_none());

        let mut blocked = RetryBlocks::new();
        note_click(&mut blocked, 4, 1_000);
        assert!(blocked.is_blocked(4, 1_000));
        // The refill cooldown is the longer 150 ms one.
        assert!(blocked.is_blocked(4, 1_149));
        assert!(!blocked.is_blocked(4, 1_150));
    }

    #[test]
    fn the_synthetic_shift_is_never_claimed_when_the_user_holds_shift() {
        let mut shift = SyntheticShift::new();
        assert_eq!(shift.action(false), ShiftAction::Press);
        assert_eq!(shift.action(true), ShiftAction::None);

        shift.note_press(true);
        assert!(shift.is_held());
        // Owned press: re-send so a focus change cannot silently drop it.
        assert_eq!(shift.action(false), ShiftAction::Press);
        assert_eq!(shift.action(true), ShiftAction::Press);
    }

    #[test]
    fn a_failed_release_keeps_ownership_so_it_is_retried() {
        let mut shift = SyntheticShift::new();
        shift.note_press(true);
        assert!(shift.note_release(false), "release refused: still owned");
        assert!(shift.is_held());
        assert!(
            !shift.note_release(true),
            "release accepted: ownership dropped"
        );
        assert!(!shift.is_held());

        // A refused press never takes ownership.
        let mut refused = SyntheticShift::new();
        refused.note_press(false);
        assert!(!refused.is_held());
        assert_eq!(refused.action(false), ShiftAction::Press);

        shift.note_press(true);
        shift.forget();
        assert!(!shift.is_held());
    }

    #[test]
    fn the_gate_mirrors_the_chest_gate_without_a_physical_check() {
        let mut state = GameState {
            refill_state: Some(RefillState {
                ready: true,
                window_id: 9,
                screen_width: 1600,
                screen_height: 900,
                slots: vec![slot(0, 10, 10)],
            }),
            ..GameState::default()
        };

        let gate = RefillGate {
            enabled: true,
            connected: true,
            foreground: true,
            gui_active: true,
            state_age_ms: Some(0),
        };
        assert!(gate.check(&state).is_ok());
        assert_eq!(
            RefillGate {
                connected: false,
                ..gate
            }
            .check(&state),
            Err(NotReady::Disconnected)
        );
        assert_eq!(
            RefillGate {
                foreground: false,
                ..gate
            }
            .check(&state),
            Err(NotReady::Background)
        );
        assert_eq!(
            RefillGate {
                state_age_ms: None,
                ..gate
            }
            .check(&state),
            Err(NotReady::NeverUpdated)
        );
        assert_eq!(
            RefillGate {
                state_age_ms: Some(FRESH_STATE_MS + 1),
                ..gate
            }
            .check(&state),
            Err(NotReady::Stale)
        );

        state.refill_state = Some(RefillState {
            ready: false,
            window_id: 9,
            screen_width: 1600,
            screen_height: 900,
            slots: vec![slot(0, 10, 10)],
        });
        assert_eq!(gate.check(&state), Err(NotReady::NotReady));

        state.refill_state = Some(RefillState {
            ready: true,
            window_id: 9,
            screen_width: 1600,
            screen_height: 900,
            slots: Vec::new(),
        });
        assert_eq!(gate.check(&state), Err(NotReady::NoSlots));

        state.refill_state = None;
        assert_eq!(gate.check(&state), Err(NotReady::NoContainer));
    }

    #[test]
    fn the_window_tracker_clears_cooldowns_once() {
        let mut window = RefillWindow::new();
        assert_eq!(
            window.active_window_id(),
            -1,
            "a tracker that has not seen a container must not name one"
        );
        let mut blocked = RetryBlocks::new();
        blocked.block(1, 0);

        assert!(window.observe(9, &mut blocked));
        assert!(blocked.is_empty());
        blocked.block(2, 0);

        assert!(
            !window.observe(9, &mut blocked),
            "same window keeps cooldowns"
        );
        assert!(blocked.is_blocked(2, 0));

        window.reset(&mut blocked);
        assert_eq!(window.active_window_id(), -1);
        assert!(blocked.is_empty());
    }
}
