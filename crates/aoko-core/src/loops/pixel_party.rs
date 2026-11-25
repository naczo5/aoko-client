//! The Pixel Party assist executor.
//!
//! Port of `Clicker.PixelPartyAssistInputLoop(CancellationToken)`
//! (`Aoko/Core/Clicker.cs:2717-2811`), spawned by `Task.Run` in
//! `StartPixelPartyAssistInputLoop` (`:2711-2716`).
//!
//! Delays, in order:
//!
//! | Condition | Delay |
//! | --- | --- |
//! | neither auto-look nor auto-walk enabled | 16 ms |
//! | GUI open *and* OS cursor visible | 16 ms |
//! | state older than [`PIXEL_PARTY_STATE_FRESH_MS`] | 8 ms |
//! | target found (steer or walk active) | 4 ms |
//! | no target, still enabled | 8 ms |
//!
//! Key presses are edge-tracked by [`WalkKeys`], which is what makes the loop
//! allocation-free and keeps the release on *every* exit path — including
//! cancellation, which the managed `catch (TaskCanceledException)` did.

use std::time::Duration;

use aoko_platform::{InputBackend, Key};

use super::{LoopRuntime, LoopSense};

/// `PixelPartyAssistInputStateFreshMs` (`Clicker.cs:631`).
pub const PIXEL_PARTY_STATE_FRESH_MS: u64 = 120;
/// `PixelPartyTargetGraceMs` (`:89`).
pub const TARGET_GRACE_MS: i64 = 160;
/// `PixelPartyJumpPulseHalfMs` (`:81`).
pub const JUMP_PULSE_HALF_MS: i64 = 55;
/// `PixelPartyWalkAlignMaxDeg` (`:82`).
pub const WALK_ALIGN_MAX_DEG: f32 = 18.0;
/// `PixelPartyPrecisionAlignMaxDeg` (`:83`).
pub const PRECISION_ALIGN_MAX_DEG: f32 = 55.0;
/// `PixelPartyPrecisionDist` (`:84`).
pub const PRECISION_DIST: f32 = 4.0;
/// `PixelPartyJumpMinDist` (`:85`).
pub const JUMP_MIN_DIST: f32 = 5.0;
/// `PixelPartyMinWalkDist` (`:86`).
pub const MIN_WALK_DIST: f32 = 0.45;
/// Disabled/idle poll (`:2737`, `:2746`).
pub const IDLE_POLL: Duration = Duration::from_millis(16);
/// Stale-state poll (`:2766`).
pub const STALE_POLL: Duration = Duration::from_millis(8);
/// Steering poll while a target is tracked (`:2807`).
pub const TRACK_POLL: Duration = Duration::from_millis(4);
/// Poll while enabled but no target (`:2807`).
pub const NO_TARGET_POLL: Duration = Duration::from_millis(8);

/// Settings the loop reads each tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelPartyConfig {
    /// `PixelPartyAssistEnabled`.
    pub enabled: bool,
    /// `PixelPartyAutoLookEnabled`.
    pub auto_look: bool,
    /// `PixelPartyAutoWalkEnabled`.
    pub auto_walk: bool,
}

/// Edge-tracked walk keys, so a key is only sent when it changes state.
#[derive(Debug, Default)]
pub struct WalkKeys {
    w: bool,
    a: bool,
    d: bool,
    space: bool,
}

impl WalkKeys {
    /// Sets one key, sending only on an edge.
    fn set(&mut self, input: &dyn InputBackend, key: Key, down: bool, tracked: &mut bool) {
        if down == *tracked {
            return;
        }
        let result = if down {
            input.key_down(key)
        } else {
            input.key_up(key)
        };
        if let Err(error) = result {
            tracing::debug!(%error, ?key, down, "pixel party key failed");
        }
        // The tracked flag follows the *attempt*, not the result: the managed code
        // assigns after `SendInput` regardless of its return value (`:675-680`).
        *tracked = down;
    }

    /// Releases every key this loop owns. Idempotent.
    pub fn release_all(&mut self, input: &dyn InputBackend) {
        let (mut w, mut a, mut d, mut space) = (self.w, self.a, self.d, self.space);
        self.set(input, Key::Char('w'), false, &mut w);
        self.set(input, Key::Char('a'), false, &mut a);
        self.set(input, Key::Char('d'), false, &mut d);
        self.set(input, Key::Space, false, &mut space);
        self.w = w;
        self.a = a;
        self.d = d;
        self.space = space;
    }

    /// `SetPixelPartyWalkKey(VK_A, false, ...)`, then D.
    fn release_strafe(&mut self, input: &dyn InputBackend) {
        let (mut a, mut d) = (self.a, self.d);
        self.set(input, Key::Char('a'), false, &mut a);
        self.set(input, Key::Char('d'), false, &mut d);
        self.a = a;
        self.d = d;
    }

    fn set_forward(&mut self, input: &dyn InputBackend, down: bool) {
        let mut w = self.w;
        self.set(input, Key::Char('w'), down, &mut w);
        self.w = w;
    }

    fn set_jump(&mut self, input: &dyn InputBackend, down: bool) {
        let mut space = self.space;
        self.set(input, Key::Space, down, &mut space);
        self.space = space;
    }

    /// Whether any key is currently held.
    #[must_use]
    pub fn any_held(&self) -> bool {
        self.w || self.a || self.d || self.space
    }
}

/// `ApplyPixelPartySteer` (`Clicker.cs:690-717`): yaw snap in several passes.
pub fn apply_steer(input: &dyn InputBackend, yaw_delta_deg: f32, aggressive: bool) {
    let mut remaining = yaw_delta_deg;
    if remaining.abs() < 0.35 {
        return;
    }

    let gain = if aggressive { 6.5_f32 } else { 5.0_f32 };
    let max_step = if aggressive { 120_i32 } else { 80_i32 };
    let max_passes = if aggressive { 10 } else { 7 };

    for _ in 0..max_passes {
        if remaining.abs() <= 0.4 {
            break;
        }

        let mut move_x = (f64::from(remaining * gain)).round() as i32;
        if move_x == 0 {
            move_x = if remaining.is_sign_negative() { -1 } else { 1 };
        }
        move_x = move_x.clamp(-max_step, max_step);

        if let Err(error) = input.move_relative(move_x, 0) {
            tracing::debug!(%error, "pixel party steer failed");
            return;
        }

        remaining -= move_x as f32 / gain;
    }
}

/// Per-loop target memory, so a brief loss can coast (`:2772-2792`).
#[derive(Debug, Default)]
struct TargetMemory {
    last_ms: i64,
    last_yaw_delta: f32,
    last_dist: f32,
}

/// Runs the Pixel Party input loop until cancelled.
pub fn run(
    runtime: &LoopRuntime,
    sense: &dyn LoopSense,
    input: &dyn InputBackend,
    config: &PixelPartyConfig,
    keys: &mut WalkKeys,
) {
    let mut memory = TargetMemory::default();

    while !runtime.is_cancelled() {
        let module_ok = config.enabled
            && sense.supports_module("pixelpartyassist")
            && sense.supports_state_field("pixelpartyyawdelta")
            && sense.is_connected()
            && sense.minecraft_active();

        let walk_enabled = module_ok && config.auto_walk;
        let steer_enabled = module_ok && (config.auto_look || walk_enabled);

        if !steer_enabled && !walk_enabled {
            keys.release_all(input);
            memory.last_ms = 0;
            if !runtime.park(IDLE_POLL) {
                break;
            }
            continue;
        }

        // GUI before freshness (`:2741-2748` then `:2750-2768`), so a stale GUI
        // still takes the 16 ms path.
        if sense
            .snapshot()
            .is_some_and(|snapshot| snapshot.state.gui_open)
            && sense.cursor_visible()
        {
            keys.release_all(input);
            memory.last_ms = 0;
            if !runtime.park(IDLE_POLL) {
                break;
            }
            continue;
        }

        let now = runtime.now();
        let now_ms = runtime.now_ms();
        let Some(snapshot) = sense.fresh_state(now, PIXEL_PARTY_STATE_FRESH_MS) else {
            keys.release_all(input);
            memory.last_ms = 0;
            if !runtime.park(STALE_POLL) {
                break;
            }
            continue;
        };
        let state = &snapshot.state;

        let mut target_found = state.pixel_party_target_found;
        let mut yaw_delta = state.pixel_party_yaw_delta;
        let mut dist = state.pixel_party_target_dist;

        if target_found {
            memory.last_ms = now_ms;
            memory.last_yaw_delta = yaw_delta;
            memory.last_dist = dist;
        } else if memory.last_ms != 0 && (now_ms - memory.last_ms) <= TARGET_GRACE_MS {
            // Coast on the last known values: a target lost mid-jump must not make
            // the movement stutter (`:2774-2791`).
            target_found = true;
            yaw_delta = memory.last_yaw_delta;
            dist = memory.last_dist;
        }

        let precision_range = target_found && dist < PRECISION_DIST;
        let can_walk = target_found && dist > MIN_WALK_DIST;

        if steer_enabled && target_found {
            apply_steer(input, yaw_delta, walk_enabled);
        }

        if walk_enabled {
            if !can_walk {
                keys.release_all(input);
            } else {
                keys.release_strafe(input);

                let yaw_abs = yaw_delta.abs();
                let align_max = if precision_range {
                    PRECISION_ALIGN_MAX_DEG
                } else {
                    WALK_ALIGN_MAX_DEG
                };
                let aligned_forward = yaw_abs < align_max;
                // Close range: step onto the block even without perfect alignment.
                let walk_forward =
                    aligned_forward || (precision_range && dist < 2.2 && yaw_abs < 75.0);
                keys.set_forward(input, walk_forward);

                let jump_down = walk_forward
                    && !precision_range
                    && dist >= JUMP_MIN_DIST
                    && (now_ms / JUMP_PULSE_HALF_MS).rem_euclid(2) == 0;
                keys.set_jump(input, jump_down);
            }
        } else {
            keys.release_all(input);
        }

        let delay = if (steer_enabled || walk_enabled) && target_found {
            TRACK_POLL
        } else {
            NO_TARGET_POLL
        };
        if !runtime.park(delay) {
            break;
        }
    }

    // The managed `catch (TaskCanceledException)` releases the keys; so does this
    // loop, on every exit.
    keys.release_all(input);
}
