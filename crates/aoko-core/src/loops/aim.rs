//! The aim-assist executor.
//!
//! Port of `Clicker.AimAssistLoop(CancellationToken)`
//! (`Aoko/Core/Clicker.cs:2657-2715`), spawned by `Task.Run` at `:2643`.
//!
//! Delays and their order, exactly as managed:
//!
//! | Condition | Delay |
//! | --- | --- |
//! | not enabled / bridge lacks the module / not connected / window inactive | 8 ms |
//! | GUI open *and* OS cursor visible | 16 ms |
//! | state older than [`AIM_STATE_FRESH_MS`] | 8 ms |
//! | normal tick | 8 ms |
//!
//! The motion controller is reset on every early return, which is what stops a
//! stale filter state from flinging the crosshair when the module comes back.

use crate::aim_assist::{AimAssistMotionController, AimAssistParams};

use super::{LoopRuntime, LoopSense};

/// `AimAssistStateFreshMs` (`Clicker.cs:15`).
pub const AIM_STATE_FRESH_MS: u64 = 220;
/// Poll interval while the module cannot run (`:2673`).
pub const IDLE_POLL: std::time::Duration = std::time::Duration::from_millis(8);
/// Poll interval while a GUI owns the cursor (`:2682`).
pub const GUI_POLL: std::time::Duration = std::time::Duration::from_millis(16);
/// Steady-state tick (`:2710`).
pub const TICK_POLL: std::time::Duration = IDLE_POLL;

/// Settings the loop reads each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimLoopConfig {
    /// `AimAssistEnabled`.
    pub enabled: bool,
    /// Configured FOV cone, range and strength. `width`/`height` are overwritten
    /// from the state each tick.
    pub params: AimAssistParams,
    /// `IsUsingLeftButton`: the click loop is holding the left button, so assist
    /// applies even without a physical press (`:2704-2705`).
    pub auto_left_clicking: bool,
}

/// Runs the aim-assist loop until cancelled.
pub fn run(
    runtime: &LoopRuntime,
    sense: &dyn LoopSense,
    input: &dyn aoko_platform::InputBackend,
    config: &AimLoopConfig,
    controller: &mut AimAssistMotionController,
) {
    while !runtime.is_cancelled() {
        let should_run = config.enabled
            && sense.supports_module("aimassist")
            && sense.is_connected()
            && sense.minecraft_active();

        if !should_run {
            controller.reset();
            if !runtime.park(IDLE_POLL) {
                break;
            }
            continue;
        }

        // GUI before freshness, exactly as managed: the loop reads `CurrentState`
        // and checks `GuiOpen && IsCursorVisible()` at `:2676-2684` and only then
        // looks at the timestamp at `:2686-2696`. A stale GUI therefore still takes
        // the 16 ms path.
        if sense
            .snapshot()
            .is_some_and(|snapshot| snapshot.state.gui_open)
            && sense.cursor_visible()
        {
            controller.reset();
            if !runtime.park(GUI_POLL) {
                break;
            }
            continue;
        }

        let now = runtime.now();
        let Some(snapshot) = sense.fresh_state(now, AIM_STATE_FRESH_MS) else {
            // No state yet or a stale one: the 8 ms reset path (`:2686-2696`).
            controller.reset();
            if !runtime.park(IDLE_POLL) {
                break;
            }
            continue;
        };
        let state = &snapshot.state;

        // The hold guard (`Clicker.cs:2704-2706`) is licensed by the physical left
        // button: `if (leftHeld || autoLeftClicking) TryApplyAimAssist(...)`.
        //
        // `probe_mouse_state` is `None` on a session that cannot see the mouse at
        // all - Wayland plus uinput, or no `/dev/input` access. Unknown is neither
        // "held" nor "up": the managed code only had the always-answering
        // `GetAsyncKeyState`, so there is no C# antecedent for this state. Acting is
        // licensed by proof of the hold, so an unobservable session does not move
        // this tick; it does not silently claim the button is up either, which
        // would be the same silent no-op wearing a `false`. Do not rewrite this as
        // `unwrap_or(false)`.
        let left_held = match input.probe_mouse_state() {
            Some(mouse) => mouse.left_down,
            None => {
                if !config.auto_left_clicking {
                    controller.reset();
                    if !runtime.park(IDLE_POLL) {
                        break;
                    }
                    continue;
                }
                // Auto-left-clicking supplies the hold the guard asks for, so the
                // physical sample is not part of this decision.
                false
            }
        };

        if left_held || config.auto_left_clicking {
            apply(sense, input, config, controller, state);
        } else {
            controller.reset();
        }

        if !runtime.park(TICK_POLL) {
            break;
        }
    }
}

/// `TryApplyAimAssist` (`Clicker.cs:3310-3345`).
fn apply(
    sense: &dyn LoopSense,
    input: &dyn aoko_platform::InputBackend,
    config: &AimLoopConfig,
    controller: &mut AimAssistMotionController,
    state: &aoko_protocol::state::GameState,
) {
    let (mut width, mut height) = (state.viewport_width, state.viewport_height);
    if width <= 0 || height <= 0 {
        match sense.framebuffer_size() {
            Some((w, h)) if w > 0 && h > 0 => {
                width = w;
                height = h;
            }
            _ => {
                controller.reset();
                return;
            }
        }
    }

    let params = AimAssistParams {
        width,
        height,
        game_fov: state.fov,
        ..config.params
    };

    let motion = controller.update_from_state(&state.entities, &params, state.looking_at_entity);
    if !motion.has_target || motion.suppressed || (motion.move_x == 0 && motion.move_y == 0) {
        return;
    }

    // `SendInput` with a relative move; the selector's smoothing already happened.
    if let Err(error) = input.move_relative(motion.move_x, motion.move_y) {
        tracing::debug!(%error, "aim-assist move failed");
    }
}
