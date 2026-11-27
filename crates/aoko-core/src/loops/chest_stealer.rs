//! The chest-stealer executor.
//!
//! Port of `ChestStealerController.RunAsync` (`Aoko/Core/ChestStealerController.cs:143-207`).
//! The managed `Clicker` owned that controller and started or cancelled it from the module's
//! arm path (`Clicker.cs:1076-1090`), which is why no gate was needed inside the loop there;
//! this body outlives the switch, so it asks [`LoopSense::module_enabled`] every tick.
//!
//! ## Guards, in the managed order
//!
//! `ChestStealerController.cs:157-176` evaluates one conjunction and parks for 45 ms when any
//! term fails. [`crate::chest_stealer::ChestGate`] is that conjunction as data, and this body
//! evaluates it in the same order:
//!
//! | Term | Managed |
//! | --- | --- |
//! | `ChestStealerEnabled` | the body's own switch, checked before anything else |
//! | `GameStateClient.IsConnected` | `:159` |
//! | `WindowDetection.IsMinecraftForeground()` | `:160` |
//! | `state.GuiOpen \|\| IsCursorVisible()` | `:156`, `:161` |
//! | state never updated, or older than `FreshStateMs` | `:162-163` |
//! | `ChestStealerState` absent / not `Ready` | `:164-165` |
//! | `ChestStealerMenuCheck && !Physical` | `:166` |
//! | `Slots.Count == 0` | `:167` |
//!
//! ## After the gate
//!
//! The `-1` window sentinel is [`crate::chest_stealer::WindowTracker`]: a window-id change
//! releases the synthetic Shift, clears every retry block and resets the click counter
//! (`:178-185`), and every gate failure resets it to `-1` (`:169-176`). Then the slot retry
//! blocks are pruned against the live slots (`:190-191`), the nearest unmapped-or-cooling slot
//! is chosen (`:192-199`), and a slot whose click cannot be mapped or whose cursor move is
//! refused parks instead of being blocked (`:200-205`, `:304-306`: the managed `-1`
//! elapsed-time sentinel means "not clicked", so no cooldown is recorded).
//!
//! ## The click
//!
//! `ClickSlotAsync` (`:296-338`) is five steps, and every one of them is here: an absolute
//! cursor placement with `SetCursorPos` (`:305`), the synthetic-Shift ownership dance
//! (`EnsureShiftHeld`, `:300-316`), a 4..=10 ms settle, a left press, an 8..=18 ms hold, and a
//! release in a `finally` so a cancelled body cannot leave the button down. The clock starts
//! before the move (`:297`) and the elapsed time is subtracted from the next interval
//! (`:203-205`), because `NextIntervalMs` is a click cadence, not an idle delay.
//!
//! The jitter (±2 px), the settle and hold draws and the cadence maths all come from
//! [`crate::refill::plan_click`] and [`crate::chest_stealer::next_interval_ms`], so this body
//! owns sequencing and nothing else.

use std::time::Duration;

use aoko_platform::{InputBackend, Key, MouseButton};

use super::{LoopRuntime, LoopSense, CHEST_STEALER_MODULE, MODULE_OFF_POLL};
use crate::chest_stealer::{
    nearest_live_slot, next_interval_ms, ChestGate, RetryBlocks, WindowTracker, ERROR_BACKOFF_MS,
    IDLE_POLL_MS, NO_SLOT_POLL_MS,
};
use crate::refill::{plan_click, ShiftAction, SyntheticShift};

/// Settings the loop reads each tick.
///
/// **`ChestStealerEnabled` is not one of them** (`Clicker.cs:1076-1090`): the managed setter
/// started and cancelled the controller, so the loop's existence was the switch. See
/// [`LoopSense::module_enabled`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChestStealerLoopConfig {
    /// `ChestStealerDelayMs`, already clamped to `50..=500` (`Clicker.cs:1093-1106`).
    pub delay_ms: i32,
    /// `ChestStealerMenuCheck`: refuse a container the bridge did not confirm as a physical
    /// chest (`ChestStealerController.cs:166`).
    pub menu_check: bool,
}

/// Runs the chest stealer until cancelled.
pub fn run(
    runtime: &LoopRuntime,
    sense: &dyn LoopSense,
    input: &dyn InputBackend,
    config: &ChestStealerLoopConfig,
) {
    // The fallback settings, for a sense with no profile behind it (tests, captures).
    let started = *config;
    let mut tracker = WindowTracker::new();
    let mut blocked = RetryBlocks::new();
    let mut shift = SyntheticShift::new();

    while !runtime.is_cancelled() {
        // `ChestStealerEnabled`, read fresh: the managed setter cancelled the controller
        // (`Clicker.cs:1076-1090`) instead of leaving it parked.
        if sense.module_enabled(CHEST_STEALER_MODULE) == Some(false) {
            release_shift(input, &mut shift);
            tracker.reset(&mut blocked);
            if !runtime.park(MODULE_OFF_POLL) {
                break;
            }
            continue;
        }

        // `ChestStealerDelayMs` and `ChestStealerMenuCheck` were read inside the managed loop
        // (`ChestStealerController.cs:156-166`, `:203`), so they come from the live profile.
        let config = sense
            .loop_configs()
            .map_or(started, |configs| configs.chest_stealer);

        // The slot geometry is mapped through the window's client rect, so a session that
        // cannot report one parks instead of clicking at a guessed position
        // (`ChestStealerCoordinateMapper.cs:20-23`).
        let Some(rect) = sense.client_rect() else {
            release_shift(input, &mut shift);
            tracker.reset(&mut blocked);
            if !runtime.park(Duration::from_millis(IDLE_POLL_MS)) {
                break;
            }
            continue;
        };

        let now = runtime.now();
        let Some(snapshot) = sense.snapshot() else {
            release_shift(input, &mut shift);
            tracker.reset(&mut blocked);
            if !runtime.park(Duration::from_millis(IDLE_POLL_MS)) {
                break;
            }
            continue;
        };
        let state = &snapshot.state;

        let gate = ChestGate {
            // The switch was checked above, and it is the one term the gate cannot read from
            // the snapshot.
            enabled: true,
            connected: sense.is_connected(),
            foreground: sense.minecraft_active(),
            gui_active: state.gui_open || sense.cursor_visible(),
            state_age_ms: sense.state_age_ms(now),
            menu_check: config.menu_check,
        };

        let chest = match gate.check(state) {
            Ok(chest) => chest,
            Err(reason) => {
                tracing::trace!(?reason, "chest stealer idle");
                release_shift(input, &mut shift);
                tracker.reset(&mut blocked);
                if !runtime.park(Duration::from_millis(IDLE_POLL_MS)) {
                    break;
                }
                continue;
            }
        };

        // A new container id drops the previous window's cooldowns (`:178-185`).
        tracker.observe(chest.window_id, &mut blocked);
        // Slots the bridge stopped reporting cannot be cooling down (`:190-191`).
        blocked.prune(&chest.slots);

        let cursor = input.mouse_position().unwrap_or_else(|| rect.centre());
        let now_ms = millis(runtime);
        let choice = nearest_live_slot(
            chest.screen_width,
            chest.screen_height,
            &chest.slots,
            &blocked,
            cursor,
            rect,
            now_ms,
        );
        let Some(choice) = choice else {
            if !runtime.park(Duration::from_millis(NO_SLOT_POLL_MS)) {
                break;
            }
            continue;
        };
        // `ChestStealerSlot` is `Copy`, so the slot outlives the borrow of the snapshot.
        let slot = chest.slots[choice.index];

        let plan = runtime
            .with_rng(|rng| plan_click(chest.screen_width, chest.screen_height, &slot, rect, rng));
        let Some(plan) = plan else {
            // `ClickSlotAsync` returns `-1` when the slot cannot be mapped (`:301-303`), which
            // records no cooldown; a poll rather than a click is the port's answer to a slot
            // that is listed but unmappable.
            if !runtime.park(Duration::from_millis(NO_SLOT_POLL_MS)) {
                break;
            }
            continue;
        };

        let click_started = runtime.now();

        // `SetCursorPos(x, y)` (`:305`): absolute, because a relative move cannot land on a
        // slot from an unknown cursor position.
        if let Err(error) = super::place_cursor(input, plan.x, plan.y) {
            tracing::debug!(%error, "chest stealer cursor move failed");
            if !runtime.park(Duration::from_millis(ERROR_BACKOFF_MS)) {
                break;
            }
            continue;
        }

        ensure_shift(input, &mut shift);
        if !runtime.park(Duration::from_millis(plan.settle_ms)) {
            release_shift(input, &mut shift);
            break;
        }

        let mut pressed = false;
        if let Err(error) = input.button_down(MouseButton::Left) {
            tracing::debug!(%error, "chest stealer press failed");
        } else {
            pressed = true;
            // The hold is a wait like any other: a stop during it must still release the
            // button, which is what the managed `finally` guaranteed (`:328-336`).
            let _ = runtime.park(Duration::from_millis(plan.hold_ms));
        }
        if pressed {
            if let Err(error) = input.button_up(MouseButton::Left) {
                tracing::debug!(%error, "chest stealer release failed");
            }
        }

        let elapsed_ms = millis32(runtime.now().saturating_duration_since(click_started));

        if runtime.is_cancelled() {
            release_shift(input, &mut shift);
            break;
        }

        if pressed {
            // `:326-330` recorded the cooldown unconditionally because it never read
            // `SendInput`'s result. The port records it only for an accepted press, so a
            // refused one does not hide the slot for a cooldown it never spent.
            tracker.note_click();
            blocked.block(slot.slot_number, millis(runtime));
        }

        // `NextIntervalMs(clicker.ChestStealerDelayMs, nextDistance)` minus the click time
        // (`:201-205`). The distance is measured from the cursor where it now is, which is the
        // slot just clicked, so the next wait is short for a neighbour and long across the GUI.
        let next_distance = nearest_live_slot(
            chest.screen_width,
            chest.screen_height,
            &chest.slots,
            &blocked,
            (plan.x, plan.y),
            rect,
            millis(runtime),
        )
        .map_or(0.0, |next| next.distance);
        let interval =
            runtime.with_rng(|rng| next_interval_ms(config.delay_ms, next_distance, rng));
        let wait_ms = u64::try_from((interval - elapsed_ms).max(1)).unwrap_or(1);

        if !runtime.park(Duration::from_millis(wait_ms)) {
            break;
        }
    }

    // `finally { ReleaseSyntheticShift(); }` (`:203-206`).
    release_shift(input, &mut shift);
}

/// `LoopRuntime::now_ms` as the `u64` the retry blocks are keyed by.
///
/// Negative is impossible for a monotonic clock, and saturating at zero keeps it total.
fn millis(runtime: &LoopRuntime) -> u64 {
    u64::try_from(runtime.now_ms()).unwrap_or(0)
}

/// A click's elapsed time, clamped into `i32` for the interval subtraction.
fn millis32(elapsed: Duration) -> i32 {
    i32::try_from(elapsed.as_millis()).unwrap_or(i32::MAX)
}

/// `EnsureShiftHeld` (`ChestStealerController.cs:300-316`).
///
/// A genuine Shift press short-circuits **unless** this body already owns a synthetic press,
/// in which case the press is re-sent: a focus change can drop it without telling either side,
/// and re-sending is idempotent while a missing press turns the click into a plain pickup.
fn ensure_shift(input: &dyn InputBackend, shift: &mut SyntheticShift) {
    // `GetAsyncKeyState` answers for the physical key; an unobservable session is read as "not
    // held", which is the managed answer when the call fails.
    let real_shift = input.probe_key_state(Key::Shift) == Some(true);
    if shift.action(real_shift) == ShiftAction::Press {
        shift.note_press(input.key_down(Key::Shift).is_ok());
    }
}

/// `ReleaseSyntheticShift` (`:318-326`).
///
/// Ownership survives a refused release, so the next call retries instead of leaving a
/// synthetic Shift held with nothing tracking it.
fn release_shift(input: &dyn InputBackend, shift: &mut SyntheticShift) {
    if !shift.is_held() {
        return;
    }
    let _ = shift.note_release(input.key_up(Key::Shift).is_ok());
}
