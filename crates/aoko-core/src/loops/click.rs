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

use std::time::{Duration, Instant};

use aoko_platform::{InputBackend, MouseButton};
use aoko_protocol::profile::ClickRandomizationMode;

use super::{ClickSink, LoopRuntime, LoopSense};
use crate::clicker::tick::{decide, ClickConfig, TickContext, TickDecision};
use crate::clicker::AdvancedClickTimingState;
use crate::clicker::ClickDeadlineScheduler;

/// Upper bound on one click interval: `1000 / 0.1` CPS, the managed
/// `Math.Max(0.1f, cps)` floor (`Clicker.cs:3243`).
pub const MAX_INTERVAL_MS: f64 = 10_000.0;

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
            min_cps: 8.0,
            max_cps: 12.0,
        }
    }

    /// The right engine's defaults.
    #[must_use]
    pub fn right() -> Self {
        Self {
            left_button: false,
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
    let mut next_deadline: Option<Instant> = None;
    // The not-connected state, built once: no snapshot is not "do nothing", it is
    // the managed not-connected branch (`Clicker.cs:3175-3183`), which only delays
    // when the cursor is *visible* and falls straight through into the click logic
    // when it is hidden. That is what keeps standalone (un-injected) clicking
    // alive, and `decide` already models it from `GameState::is_connected`, so the
    // default state is fed in rather than short-circuiting here.
    let disconnected = aoko_protocol::state::GameState::default();

    while !runtime.is_cancelled() {
        let now = runtime.now();

        let snapshot = sense.snapshot();
        let state = snapshot
            .as_ref()
            .map_or(&disconnected, |snapshot| &*snapshot.state);

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

        let (cps, interval_ms) = next_click_interval(runtime, config, timing, now);

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
