//! The click-loop executor.
//!
//! Port of `Clicker.ClickLoop(CancellationToken, bool leftButton)`
//! (`Aoko/Core/Clicker.cs:3117-3300`), spawned per button by `Task.Run` at `:482`.
//!
//! The guard chain is [`crate::clicker::tick::decide`]; this module is the executor
//! the C# method also contains:
//!
//! * CPS selection per `RandomizationMode` (`:3229-3275`): uniform for `Basic`,
//!   Box-Muller gaussian clamped to the configured band for `Medium`, and
//!   [`AdvancedClickTimingState`] for `Advanced`.
//! * The click itself and the recorded sample (`:3278-3281`).
//! * Deadline scheduling through [`ClickDeadlineScheduler`] (`:3283-3286`), with
//!   the wait retargeted at the absolute deadline instead of `Task.Delay` from the
//!   moment the click returned. That is the one behavioural difference: it removes
//!   cumulative sleep drift. The interval is unchanged.
//!
//! Every guard's poll interval is the managed one, because it comes back from
//! `decide` as a `Duration` rather than being restated here.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use aoko_platform::{InputBackend, MouseButton};
use aoko_protocol::profile::ClickRandomizationMode;
use aoko_protocol::state::GameState;

use super::{ClickSink, LoopRuntime, LoopSense};
use crate::clicker::tick::{decide, ClickConfig, TickContext, TickDecision};
use crate::clicker::AdvancedClickTimingState;
use crate::clicker::ClickDeadlineScheduler;

/// Upper bound on one click interval: `1000 / 0.1` CPS, the managed
/// `Math.Max(0.1f, cps)` floor (`Clicker.cs:3243`).
pub const MAX_INTERVAL_MS: f64 = 10_000.0;

/// `Clicker.MinCPS` field initialiser (`Clicker.cs:118`).
pub const DEFAULT_MIN_CPS: f32 = 8.0;
/// `Clicker.MaxCPS` field initialiser (`Clicker.cs:119`).
pub const DEFAULT_MAX_CPS: f32 = 12.0;
/// `Clicker.RightMinCPS` field initialiser (`Clicker.cs:128`).
pub const DEFAULT_RIGHT_MIN_CPS: f32 = 10.0;
/// `Clicker.RightMaxCPS` field initialiser (`Clicker.cs:129`).
pub const DEFAULT_RIGHT_MAX_CPS: f32 = 14.0;

/// One clicker engine's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClickLoopConfig {
    /// Which engine this loop drives. Two loops run, one per button (`:482`).
    pub left_button: bool,
    /// Guard-chain settings (`ClickInChests`, `BreakBlocksEnabled`, ...).
    pub guards: ClickConfig,
    /// `RandomizationMode`.
    pub mode: ClickRandomizationMode,
    /// `MinCPS` or `RightMinCPS`.
    pub min_cps: f32,
    /// `MaxCPS` or `RightMaxCPS`.
    pub max_cps: f32,
}

impl ClickLoopConfig {
    /// The left engine's defaults with `ClickConfig::default()` guards.
    #[must_use]
    pub fn left() -> Self {
        Self {
            left_button: true,
            guards: ClickConfig::default(),
            mode: ClickRandomizationMode::Medium,
            min_cps: DEFAULT_MIN_CPS,
            max_cps: DEFAULT_MAX_CPS,
        }
    }

    /// The right engine's defaults.
    ///
    /// The CPS band is the right engine's own (`Clicker.cs:128-129`, `10`/`14`); the managed
    /// property pair was never shared with the left one, so `..Self::left()` would have started
    /// the right engine 2 CPS slow.
    #[must_use]
    pub fn right() -> Self {
        Self {
            left_button: false,
            min_cps: DEFAULT_RIGHT_MIN_CPS,
            max_cps: DEFAULT_RIGHT_MAX_CPS,
            ..Self::left()
        }
    }

    fn button(&self) -> MouseButton {
        if self.left_button {
            MouseButton::Left
        } else {
            MouseButton::Right
        }
    }
}

/// CPS and interval for one click (`Clicker.cs:3229-3275`).
///
/// Returns `(cps, interval_ms)`. `interval_ms` is always finite and positive, so
/// the caller can build a `Duration` from it without a panic path.
fn next_click_interval(
    runtime: &LoopRuntime,
    config: &ClickLoopConfig,
    timing: &mut AdvancedClickTimingState,
    now: Instant,
) -> (f32, f64) {
    // `if (minCps > maxCps) minCps = maxCps;` (`:3231-3232`).
    let min_cps = if config.min_cps > config.max_cps {
        config.max_cps
    } else {
        config.min_cps
    };
    let max_cps = config.max_cps;

    match config.mode {
        ClickRandomizationMode::Basic => {
            let draw = runtime.uniform() as f32;
            let cps = min_cps + draw * (max_cps - min_cps);
            let interval = 1000.0 / f64::from(cps.max(0.1));
            (cps, sanitize_interval(interval))
        }
        ClickRandomizationMode::Medium => {
            let mid = (min_cps + max_cps) / 2.0;
            let range = (max_cps - min_cps) / 4.0;
            let gaussian = runtime.gaussian(mid, range);
            // `Math.Clamp(cps, minCps, maxCps)`; `f32::clamp` panics when the
            // bounds are NaN, so the band is sanitised first.
            let low = if min_cps.is_finite() { min_cps } else { 1.0 };
            let high = if max_cps.is_finite() { max_cps } else { 1.0 };
            let (high, low) = if low > high { (low, high) } else { (high, low) };
            let cps = if gaussian.is_finite() {
                gaussian.clamp(low, high)
            } else {
                low
            };
            let interval = 1000.0 / f64::from(cps.max(0.1));
            (cps, sanitize_interval(interval))
        }
        ClickRandomizationMode::Advanced => {
            let interval = timing.calculate_next_delay_at(min_cps, max_cps, now);
            let cps = (1000.0 / interval.max(1.0)) as f32;
            (cps, sanitize_interval(interval))
        }
    }
}

/// Keeps an interval usable as a `Duration`: positive, finite, and inside the
/// managed `Math.Max(0.1f, cps)` band.
fn sanitize_interval(interval_ms: f64) -> f64 {
    if !interval_ms.is_finite() || interval_ms <= 0.0 {
        return MAX_INTERVAL_MS;
    }
    interval_ms.clamp(1.0, MAX_INTERVAL_MS)
}

/// The state a loop reads before the bridge has published anything.
///
/// A `GameState` default carries one heap string (`screenName`), so rebuilding it
/// per loop start would be an allocation on a path that never changes its value.
fn disconnected_state() -> &'static GameState {
    static DISCONNECTED: OnceLock<GameState> = OnceLock::new();
    DISCONNECTED.get_or_init(GameState::default)
}

/// Runs one click engine until the runtime is cancelled.
///
/// Called on a dedicated thread through [`super::LoopHandle`]; also callable
/// directly from a test that wants to drive a single tick sequence.
pub fn run(
    runtime: &LoopRuntime,
    sense: &dyn LoopSense,
    input: &dyn InputBackend,
    sink: &dyn ClickSink,
    config: &ClickLoopConfig,
    timing: &mut AdvancedClickTimingState,
) {
    // The settings this body was started with. They are the fallback for a sense with no
    // profile behind it (tests, captures) and the source of `left_button`, which a live
    // config has to agree with because it decides which module and which button this body
    // is.
    let started = *config;
    let module = if started.left_button {
        super::AUTOCLICKER_MODULE
    } else {
        super::RIGHT_CLICK_MODULE
    };
    let mut next_deadline: Option<Instant> = None;
    // The not-connected state, built once per process: no snapshot is not "do nothing",
    // it is the managed not-connected branch (`Clicker.cs:3175-3183`), which only
    // delays when the cursor is *visible* and falls straight through into the click
    // logic when it is hidden. That is what keeps standalone (un-injected) clicking
    // alive, and `decide` already models it from `GameState::is_connected`, so the
    // default state is fed in rather than short-circuiting here. It is built once
    // because a `String` clone per loop start is an allocation on a hot path for a
    // value that never changes; `perf` pins the per-tick cost at zero.
    let disconnected = disconnected_state();

    while !runtime.is_cancelled() {
        // The engine's own switch, read on every tick (`LoopSense::module_enabled`). The
        // managed host never ran this loop while its module was off: `StartClicking`
        // refused to start a stream whose module was disabled (`Clicker.cs:470-476`) and
        // `StopClicking`/`Disarm` cancelled the running one (`:459`, `:488-509`). Without
        // this gate the body kept clicking for as long as *any* loop-driving module held
        // the bundle up, so switching the AutoClicker off did nothing while aim assist was
        // on, and switching aim assist on started a click stream nobody asked for.
        if sense.module_enabled(module) == Some(false) {
            // Drop the phase. A managed restart began from `nextClickDeadline = 0`
            // (`:3119`), so re-arming must not resume a deadline from before the pause.
            next_deadline = None;
            if !runtime.park(super::MODULE_OFF_POLL) {
                break;
            }
            continue;
        }

        // The managed loop read `Clicker`'s properties inside its `while`, so a CPS or guard
        // change took effect on the next click. This does the same instead of trusting the
        // snapshot the bundle was started with.
        let config = sense.loop_configs().map_or(started, |configs| {
            if started.left_button {
                configs.left
            } else {
                configs.right
            }
        });

        let now = runtime.now();

        let snapshot = sense.snapshot();
        let state = snapshot
            .as_ref()
            .map_or(disconnected, |snapshot| &*snapshot.state);

        let mut guards = config.guards;
        guards.injected = sense.is_injected();
        let context = TickContext {
            minecraft_active: sense.minecraft_active(),
            cursor_visible: sense.cursor_visible(),
        };

        match decide(state, &guards, context, config.left_button) {
            TickDecision::Wait(delay) => {
                if !runtime.park(delay) {
                    break;
                }
                continue;
            }
            TickDecision::Click => {}
        }

        let (cps, interval_ms) = next_click_interval(runtime, &config, timing, now);

        // `if (token.IsCancellationRequested) break;` right before the click
        // (`:3277-3278`): a stop never injects one more click.
        if runtime.is_cancelled() {
            break;
        }

        sink.record_click(cps, config.left_button);
        if let Err(error) = input.click(config.button()) {
            tracing::debug!(%error, left = config.left_button, "click injection failed");
        }

        let interval = Duration::from_secs_f64(interval_ms / 1000.0);
        let schedule =
            ClickDeadlineScheduler::schedule_next(next_deadline, runtime.now(), interval);
        next_deadline = Some(schedule.deadline);

        if !runtime.park_until(schedule.deadline) {
            break;
        }
    }
}
