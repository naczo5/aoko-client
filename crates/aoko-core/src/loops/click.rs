//! The click-loop executor.
//!
//! Port of `Clicker.ClickLoop(CancellationToken, bool leftButton)`
//! (`Aoko/Core/Clicker.cs:3117-3300`), spawned per button by `Task.Run` at `:482`.
//!
//! **The stream is a held button, not a switch.** `StartClicking(button)` — the only caller of
//! that `Task.Run` — was invoked by the low-level mouse hook on `WM_LBUTTONDOWN` /
//! `WM_RBUTTONDOWN`, and `StopClicking()` on the matching `WM_LBUTTONUP` / `WM_RBUTTONUP`
//! (`Aoko/Core/InputHooks.cs:565-585`, `:606-625`). Arming the AutoClicker module therefore
//! started nothing: it only allowed the *next* press to start a stream
//! (`Clicker.cs:462-470`). This body reproduces that: it clicks while its own button is held,
//! parks the moment it is released, and drops its deadline so the next press begins a fresh
//! schedule — `ClickLoop` restarted from `nextClickDeadline = 0` (`:3119`) for the same reason.
//! The one thing it cannot do itself is decide which button owns the mouse when both are down;
//! that is [`ClickStream`], the port of `Clicker`'s `_activity` (`:459-509`).
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
use crate::clicker::stream::ClickStream;
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

/// How often the body looks for a press while it has none.
///
/// The managed client had no poll at all — the mouse hook *called* `StartClicking`, so the
/// press started the stream with no latency — and this body's replacement for that hook is a
/// sample, so the wait is the press-detection latency. Eight milliseconds is a fifth of one
/// Minecraft tick: `ClickLoop`'s own guards poll at 10 to 100 ms, so this is the cheapest wait
/// in the body and still lands the first click inside the frame the user pressed in.
pub const HOLD_POLL: Duration = Duration::from_millis(8);

/// Releases this engine's claim on the mouse however the body leaves its loop.
///
/// `StopClicking()` ran on every managed path out of a stream: the release edge, `Disarm`,
/// `Stop()`, the panic press (`Clicker.cs:488-509,516-524`). A body that returned without
/// releasing would leave the cell owned by an engine that is no longer clicking, and the other
/// button would never start a stream again.
struct StreamHold<'a> {
    stream: Option<&'a ClickStream>,
    left: bool,
}

impl Drop for StreamHold<'_> {
    fn drop(&mut self) {
        if let Some(stream) = self.stream {
            stream.release(self.left);
        }
    }
}

/// Runs one click engine until the runtime is cancelled, with no shared stream.
///
/// The six-argument shape the tests drive: the claim always succeeds, so a test of one engine
/// sees the hold gate and the scheduling but not the "which button owns the mouse" rule. The
/// bundle starts both engines through [`run_with_stream`].
pub fn run(
    runtime: &LoopRuntime,
    sense: &dyn LoopSense,
    input: &dyn InputBackend,
    sink: &dyn ClickSink,
    config: &ClickLoopConfig,
    timing: &mut AdvancedClickTimingState,
) {
    run_with_stream(runtime, sense, input, sink, config, timing, None);
}

/// Runs one click engine, sharing the mouse with its sibling through `stream`.
///
/// Called on a dedicated thread through [`super::LoopHandle`].
///
/// `stream` is the cell the two engines share; `None` for a test that drives one engine on its
/// own, in which case the claim always succeeds and only the hold gate is exercised.
pub fn run_with_stream(
    runtime: &LoopRuntime,
    sense: &dyn LoopSense,
    input: &dyn InputBackend,
    sink: &dyn ClickSink,
    config: &ClickLoopConfig,
    timing: &mut AdvancedClickTimingState,
    stream: Option<&ClickStream>,
) {
    // The settings this body was started with. They are the fallback for a sense with no
    // profile behind it (tests, captures) and the source of `left_button`, which a live
    // config has to agree with because it decides which module and which button this body
    // is.
    let started = *config;
    let left = started.left_button;
    let module = if left {
        super::AUTOCLICKER_MODULE
    } else {
        super::RIGHT_CLICK_MODULE
    };
    let _release_on_exit = StreamHold { stream, left };
    let mut next_deadline: Option<Instant> = None;
    // `StartClicking` ran on the press edge and `StopClicking` on the release edge, so the body
    // needs both edges, not the level: a button that was already held when the module came on
    // must not start a stream (the managed hook saw no press either), and a button whose press
    // was refused because the other engine owned the mouse must wait for its own next press.
    let mut pressed = false;
    // Whether this engine owns the mouse. `true` while a sense does not model the hold at all,
    // so such a sense keeps the switch-only behaviour.
    let mut owns = stream.is_none();
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
            // `Disarm` called `StopClicking` (`:459`): a module switched off mid-press gives the
            // mouse back, so the other button's stream is not blocked by an engine that is not
            // clicking. The press state stays set, because the managed hook also needed a new
            // press after a re-arm.
            if owns {
                if let Some(stream) = stream {
                    stream.release(left);
                }
                owns = stream.is_none();
            }
            if !runtime.park(super::MODULE_OFF_POLL) {
                break;
            }
            continue;
        }

        // The hold gate: `StartClicking` on the press, `StopClicking` on the release. A button
        // that is not held does not click, however the module switch reads.
        if let Some(mouse) = sense.physical_mouse() {
            let down = if left {
                mouse.left_down
            } else {
                mouse.right_down
            };
            if !down {
                if owns {
                    if let Some(stream) = stream {
                        stream.release(left);
                    }
                    owns = stream.is_none();
                }
                pressed = false;
                // The release cancels the stream; the next press starts a new one from a fresh
                // deadline, which is what `StopClicking` + `StartClicking` did to
                // `nextClickDeadline` (`:3119`, `:488-509`).
                next_deadline = None;
                if !runtime.park(HOLD_POLL) {
                    break;
                }
                continue;
            }

            if !pressed {
                pressed = true;
                // `if (Activity != ClickerActivity.None) return;` (`:470`): the second button's
                // press is refused while the first stream runs, and it stays refused until that
                // stream stops, because no new press edge arrives while it is still held.
                if let Some(stream) = stream {
                    owns = stream.claim(left);
                }
            }

            if !owns {
                if !runtime.park(HOLD_POLL) {
                    break;
                }
                continue;
            }
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
        // `HitSelectEnabled` is a module switch, so the switch set is the only truth for it
        // between a save and the next load (`config_map::SWITCH_PROFILE_KEYS`): the Hit Select
        // card's switch writes `EnabledFlag::HitSelect` and never the profile field this config
        // was built from. Read it live, the way the managed loop read the property inside its
        // `while`; `None` keeps the started value, for a sense that models no switches.
        if let Some(on) = sense.module_enabled(super::HIT_SELECT_MODULE) {
            guards.hit_select_enabled = on;
        }
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

        match wait_for_next_click(runtime, sense, left, schedule.deadline) {
            ClickWait::Due => {}
            // The release cancelled the sleep, exactly as `StopClicking` cancelled the managed
            // `Task.Delay` (`Clicker.cs:501`): the stream emits nothing more, and the top of the
            // loop drops the deadline and the claim.
            ClickWait::Released => continue,
            ClickWait::Stopped => break,
        }
    }
}

/// Why a wait between clicks ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClickWait {
    /// The deadline arrived: the next click is due.
    Due,
    /// The button came up first.
    Released,
    /// The runtime was cancelled.
    Stopped,
}

/// Waits for the next click, ending early when the button comes up or the runtime stops.
///
/// `await Task.Delay(schedule.DelayMilliseconds, token)` was cancelled by `StopClicking` the
/// moment the button was released, so a managed stream never emitted a click it had already
/// scheduled. Slicing the wait at [`HOLD_POLL`] is what reproduces that here; a plain
/// `park_until` would keep clicking until the sleep ran out, which at one click per second is a
/// whole extra click after the user let go.
fn wait_for_next_click(
    runtime: &LoopRuntime,
    sense: &dyn LoopSense,
    left: bool,
    deadline: Instant,
) -> ClickWait {
    loop {
        if runtime.is_cancelled() {
            return ClickWait::Stopped;
        }

        let now = runtime.now();
        let Some(remaining) = deadline.checked_duration_since(now) else {
            return ClickWait::Due;
        };
        if remaining.is_zero() {
            return ClickWait::Due;
        }

        if let Some(mouse) = sense.physical_mouse() {
            let down = if left {
                mouse.left_down
            } else {
                mouse.right_down
            };
            if !down {
                return ClickWait::Released;
            }
        }

        if !runtime.park(remaining.min(HOLD_POLL)) {
            return ClickWait::Stopped;
        }
    }
}
