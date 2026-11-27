//! The refill executor.
//!
//! Port of `RefillController.RunAsync` (`Aoko/Core/RefillController.cs:143-296`). The managed
//! `Clicker` owned the controller and started or cancelled it from the module's arm path
//! (`Clicker.cs:1174-1188`), so the loop's existence was its switch; this body outlives the
//! switch and asks [`LoopSense::module_enabled`] every tick.
//!
//! ## Guards, in the managed order
//!
//! `RefillController.cs:150-166` is one conjunction, [`crate::refill::RefillGate`] as data:
//!
//! | Term | Managed |
//! | --- | --- |
//! | `RefillEnabled` | the body's own switch, checked before anything else |
//! | `GameStateClient.IsConnected` | `:150` |
//! | `WindowDetection.IsMinecraftForeground()` | `:151` |
//! | `state.GuiOpen \|\| IsCursorVisible()` | `:152` |
//! | state never updated, or older than `FreshStateMs` | `:153-154` |
//! | `RefillState` absent / not `Ready` | `:155` |
//! | `Slots.Count == 0` | `:156` |
//!
//! Refill has no menu-check term: it only ever clicks slots the bridge classified as a healing
//! item inside the survival inventory, so there is no server-menu shape to reject.
//!
//! ## Differences from the chest stealer, all managed
//!
//! * A window-id change clears the retry blocks but does **not** release the synthetic Shift
//!   (`:159-163`), where the chest stealer does both (`ChestStealerController.cs:178-185`).
//! * The wait is `Math.Max(1, RefillDelayMs - clickElapsedMs)` (`:230`) — a fixed cadence off
//!   the configured delay, not the chest stealer's distance-adaptive interval.
//! * No client rect means a 20 ms poll (`:167-172`); no live slot means a 30 ms poll
//!   (`:222-226`).
//!
//! The Shift ownership rule, the click shape and the per-slot cooldown come from
//! [`crate::refill`], which is where the managed constants for them live.

use std::time::Duration;

use aoko_platform::{InputBackend, Key, MouseButton};

use super::{LoopRuntime, LoopSense, MODULE_OFF_POLL, REFILL_MODULE};
use crate::chest_stealer::{nearest_live_slot, RetryBlocks, ERROR_BACKOFF_MS};
use crate::refill::{
    next_wait_ms, note_click, plan_click, RefillGate, RefillWindow, ShiftAction, SyntheticShift,
    IDLE_POLL_MS, NO_SLOT_POLL_MS,
};

/// Settings the loop reads each tick.
///
/// **`RefillEnabled` is not one of them** (`Clicker.cs:1174-1188`): the managed setter started
/// and cancelled the controller. See [`LoopSense::module_enabled`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RefillLoopConfig {
    /// `RefillDelayMs`, already clamped to `50..=500` (`Clicker.cs:1191-1204`).
    pub delay_ms: i32,
}

/// Poll while the window's client rect is unavailable (`RefillController.cs:167-172`).
///
/// It is shorter than [`IDLE_POLL_MS`] because a window that is still being created is a
/// transient condition, not "the container is unusable"; the managed body made the same
/// distinction with the same two numbers.
pub const NO_RECT_POLL_MS: u64 = 20;

/// Runs the refill loop until cancelled.
pub fn run(
    runtime: &LoopRuntime,
    sense: &dyn LoopSense,
    input: &dyn InputBackend,
    config: &RefillLoopConfig,
) {
    // The fallback settings, for a sense with no profile behind it (tests, captures).
    let started = *config;
    let mut window = RefillWindow::new();
    let mut blocked = RetryBlocks::new();
    let mut shift = SyntheticShift::new();

    while !runtime.is_cancelled() {
        // `RefillEnabled`, read fresh: the managed setter cancelled the controller
        // (`Clicker.cs:1174-1188`) instead of leaving it parked.
        if sense.module_enabled(REFILL_MODULE) == Some(false) {
            release_shift(input, &mut shift);
            window.reset(&mut blocked);
            if !runtime.park(MODULE_OFF_POLL) {
                break;
            }
            continue;
        }

        // `RefillDelayMs` was read on every iteration (`RefillController.cs:230`), so it comes
        // from the live profile rather than from the value the bundle started with.
        let config = sense.loop_configs().map_or(started, |configs| configs.refill);

        // The slot geometry is mapped through the window's client rect
        // (`RefillController.cs:167-172`, `:241-247`).
        let Some(rect) = sense.client_rect() else {
            release_shift(input, &mut shift);
            window.reset(&mut blocked);
            if !runtime.park(Duration::from_millis(NO_RECT_POLL_MS)) {
                break;
            }
            continue;
        };

        let now = runtime.now();
        let Some(snapshot) = sense.snapshot() else {
            release_shift(input, &mut shift);
            window.reset(&mut blocked);
            if !runtime.park(Duration::from_millis(IDLE_POLL_MS)) {
                break;
            }
            continue;
        };
        let state = &snapshot.state;

        let gate = RefillGate {
            // The switch was checked above; it is the one term the gate cannot read from the
            // snapshot.
            enabled: true,
            connected: sense.is_connected(),
            foreground: sense.minecraft_active(),
            gui_active: state.gui_open || sense.cursor_visible(),
            state_age_ms: sense.state_age_ms(now),
        };

        let refill = match gate.check(state) {
            Ok(refill) => refill,
            Err(reason) => {
                tracing::trace!(?reason, "refill idle");
                release_shift(input, &mut shift);
                window.reset(&mut blocked);
                if !runtime.park(Duration::from_millis(IDLE_POLL_MS)) {
                    break;
                }
                continue;
            }
        };

        // A new container id drops the previous window's cooldowns and nothing else
        // (`:159-163`).
        window.observe(refill.window_id, &mut blocked);
        // Slots the bridge stopped reporting cannot be cooling down (`:165`).
        blocked.prune(&refill.slots);

        let cursor = input.mouse_position().unwrap_or_else(|| rect.centre());
        let now_ms = millis(runtime);
        let choice = nearest_live_slot(
            refill.screen_width,
            refill.screen_height,
            &refill.slots,
            &blocked,
            cursor,
            rect,
            now_ms,
        );
        let Some(choice) = choice else {
            // Every reported pot was recently clicked; wait for the bridge to re-scan the
            // inventory (`:222-226`).
            if !runtime.park(Duration::from_millis(NO_SLOT_POLL_MS)) {
                break;
            }
            continue;
        };
        // `ChestStealerSlot` is `Copy`, so the slot outlives the borrow of the snapshot.
        let slot = refill.slots[choice.index];

        let plan = runtime.with_rng(|rng| {
            plan_click(
                refill.screen_width,
                refill.screen_height,
                &slot,
                rect,
                rng,
            )
        });
        let Some(plan) = plan else {
            // `ClickSlotAsync` returns `-1` for an unmappable slot (`:243-245`), which records
            // no cooldown; the port polls rather than clicking a position it could not compute.
            if !runtime.park(Duration::from_millis(NO_SLOT_POLL_MS)) {
                break;
            }
            continue;
        };

        let click_started = runtime.now();

        // `SetCursorPos(x, y)` (`:247`): absolute, because a relative move cannot land on a
        // slot from an unknown cursor position.
        if let Err(error) = super::place_cursor(input, plan.x, plan.y) {
            tracing::debug!(%error, "refill cursor move failed");
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
            tracing::debug!(%error, "refill press failed");
        } else {
            pressed = true;
            // A stop during the hold must still release the button, which is what the managed
            // `finally` guaranteed (`:262-270`).
            let _ = runtime.park(Duration::from_millis(plan.hold_ms));
        }
        if pressed {
            if let Err(error) = input.button_up(MouseButton::Left) {
                tracing::debug!(%error, "refill release failed");
            }
        }

        let elapsed_ms = millis32(runtime.now().saturating_duration_since(click_started));

        if runtime.is_cancelled() {
            release_shift(input, &mut shift);
            break;
        }

        if pressed {
            // `:227-228` recorded the cooldown unconditionally because it never read
            // `SendInput`'s result. The port records it only for an accepted press, so a refused
            // one does not hide the slot for a cooldown it never spent.
            note_click(&mut blocked, slot.slot_number, millis(runtime));
        }

        // `Task.Delay(Math.Max(1, RefillDelayMs - clickElapsedMs))` (`:230`).
        let wait_ms = next_wait_ms(config.delay_ms, elapsed_ms);
        if !runtime.park(Duration::from_millis(wait_ms)) {
            break;
        }
    }

    // `finally { ReleaseSyntheticShift(); }` (`:292-295`).
    release_shift(input, &mut shift);
}

/// `LoopRuntime::now_ms` as the `u64` the retry blocks are keyed by.
fn millis(runtime: &LoopRuntime) -> u64 {
    u64::try_from(runtime.now_ms()).unwrap_or(0)
}

/// A click's elapsed time, clamped into `i32` for [`next_wait_ms`].
fn millis32(elapsed: Duration) -> i32 {
    i32::try_from(elapsed.as_millis()).unwrap_or(i32::MAX)
}

/// `EnsureShiftHeld` (`RefillController.cs:300-316`), the same rule as the chest stealer's:
/// re-send an owned press, never claim a genuine one.
fn ensure_shift(input: &dyn InputBackend, shift: &mut SyntheticShift) {
    let real_shift = input.probe_key_state(Key::Shift) == Some(true);
    if shift.action(real_shift) == ShiftAction::Press {
        shift.note_press(input.key_down(Key::Shift).is_ok());
    }
}

/// `ReleaseSyntheticShift` (`:318-326`): a refused release keeps ownership so it is retried.
fn release_shift(input: &dyn InputBackend, shift: &mut SyntheticShift) {
    if !shift.is_held() {
        return;
    }
    let _ = shift.note_release(input.key_up(Key::Shift).is_ok());
}
