//! Timing pass over one tick of the click loop and the deadline maths it uses.
//!
//! `MIGRATION_ARCHITECTURE.md` section 6 binds the loops to "allocation-free per
//! tick" and to absolute deadlines. The allocations are asserted in `src/perf.rs`;
//! this is the companion timing figure. There is no bench framework here on
//! purpose: two cases do not justify a dependency, and the workspace carries none.
//!
//! One line per case, machine-readable and stable in key order:
//!
//! ```text
//! bench case=click.tick iterations=… total_ns=… ns_per_op=…
//! ```
//!
//! A click tick is one iteration of `click::run`'s body. The body only returns when
//! the loop is cancelled, so a case runs [`TICKS_PER_RUN`] ticks per measured call
//! and divides: the loop's own start-up (one `LoopRuntime`, one RNG box) is then
//! amortised to well under a percent instead of dominating a single-tick figure.
//! That start-up is not part of what this number claims to measure.
//!
//! **These numbers prove nothing about correctness**, and this machine is a
//! 1.1 GHz dual-core: read them as one machine's baseline, not as a budget.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use aoko_core::clicker::ClickDeadlineScheduler;
use aoko_core::concurrency::CancelToken;
use aoko_core::loops::click::{self, ClickLoopConfig};
use aoko_core::loops::{LoopRuntime, NoClickSink};
use aoko_core::{Clock, Sleeper, SplitMix64};
use aoko_platform::{InputBackend, InputEvent, Key, MouseButton, MouseState, PlatformError};
use aoko_protocol::state::GameState;

/// Ticks per measured call. The loop is stopped from the input side once this many
/// clicks have been injected, so one call is exactly this many ticks.
const TICKS_PER_RUN: u32 = 32;

/// A clock and sleeper over fake time: sleeping advances the clock to the deadline
/// and returns immediately, so a tick costs what its code costs and not 25 ms.
struct FakeTime {
    origin: Instant,
    elapsed_nanos: AtomicU64,
}

impl FakeTime {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            origin: Instant::now(),
            elapsed_nanos: AtomicU64::new(0),
        })
    }
}

impl Clock for FakeTime {
    fn now(&self) -> Instant {
        let elapsed = Duration::from_nanos(self.elapsed_nanos.load(Ordering::Acquire));
        self.origin.checked_add(elapsed).unwrap_or(self.origin)
    }
}

impl Sleeper for FakeTime {
    fn sleep_until(&self, deadline: Instant) {
        let gap = deadline
            .checked_duration_since(self.now())
            .map_or(0, |gap| u64::try_from(gap.as_nanos()).unwrap_or(u64::MAX));
        self.elapsed_nanos.fetch_add(gap, Ordering::AcqRel);
    }
}

/// A sense over one pre-built snapshot and a foreground window.
struct Foreground {
    snapshot: Arc<aoko_core::app::StateSnapshot>,
}

impl aoko_core::loops::LoopSense for Foreground {
    fn snapshot(&self) -> Option<Arc<aoko_core::app::StateSnapshot>> {
        Some(Arc::clone(&self.snapshot))
    }
    fn is_connected(&self) -> bool {
        true
    }
    fn is_injected(&self) -> bool {
        true
    }
    fn minecraft_active(&self) -> bool {
        true
    }
    fn cursor_visible(&self) -> bool {
        false
    }
    fn supports_module(&self, _module: &str) -> bool {
        false
    }
    fn supports_state_field(&self, _field: &str) -> bool {
        false
    }
    fn framebuffer_size(&self) -> Option<(i32, i32)> {
        None
    }
}

/// An input backend that injects clicks and cancels the loop after `stop_at` of
/// them, which is how one measured call is bounded to [`TICKS_PER_RUN`] ticks.
#[derive(Default)]
struct StopAfter {
    clicks: AtomicU32,
    stop_at: AtomicU32,
    token: Mutex<Option<CancelToken>>,
}

impl StopAfter {
    fn clicks(&self) -> u32 {
        self.clicks.load(Ordering::Acquire)
    }
}

impl InputBackend for StopAfter {
    fn mouse_state(&self) -> MouseState {
        MouseState::default()
    }
    fn key_state(&self, _key: Key) -> bool {
        false
    }
    fn click(&self, _button: MouseButton) -> Result<(), PlatformError> {
        let clicks = self.clicks.fetch_add(1, Ordering::AcqRel) + 1;
        if clicks >= self.stop_at.load(Ordering::Acquire) {
            if let Some(token) = self.token.lock().as_ref() {
                token.cancel();
            }
        }
        Ok(())
    }
    fn move_relative(&self, _dx: i32, _dy: i32) -> Result<(), PlatformError> {
        Ok(())
    }
    fn key_press(&self, _key: Key) -> Result<(), PlatformError> {
        Ok(())
    }
    fn poll_events(&self, _out: &mut Vec<InputEvent>) -> Result<(), PlatformError> {
        Ok(())
    }
}

/// Times `body` once untimed and then `iterations` times, reporting the total it
/// produced as `ticks` per iteration so the case can name what it divides by.
fn time(name: &str, iterations: u64, ticks: u64, mut body: impl FnMut() -> u32) {
    body();
    let started = Instant::now();
    for _ in 0..iterations {
        assert!(
            body() == u32::try_from(ticks).unwrap_or(u32::MAX),
            "each measured call must run exactly {ticks} ticks"
        );
    }
    let total_nanos = started.elapsed().as_nanos();
    let per_op = total_nanos / u128::from(iterations.max(1) * ticks.max(1));
    let operations = iterations * ticks;
    println!("bench case={name} iterations={operations} total_ns={total_nanos} ns_per_op={per_op}");
}

fn main() {
    let config = ClickLoopConfig::left();
    let state = GameState {
        is_connected: true,
        health: 20.0,
        ..GameState::default()
    };
    let sense = Foreground {
        snapshot: Arc::new(aoko_core::app::StateSnapshot {
            state: Arc::new(state),
            at: Instant::now(),
        }),
    };

    // 1. One click tick: guard chain, CPS draw, `InputBackend::click`, the absolute
    //    deadline and the park.
    {
        let input = StopAfter::default();
        input.stop_at.store(TICKS_PER_RUN, Ordering::Release);
        let mut timing = aoko_core::clicker::AdvancedClickTimingState::new(
            FakeTime::new() as Arc<dyn Clock>,
            Box::new(SplitMix64::new(1)),
        );

        time("click.tick", 3_000, u64::from(TICKS_PER_RUN), || {
            let clock = FakeTime::new();
            let runtime = LoopRuntime::new(
                Arc::clone(&clock) as Arc<dyn Clock>,
                clock as Arc<dyn Sleeper>,
                Box::new(SplitMix64::new(0x5EED)),
            );
            *input.token.lock() = Some(runtime.token().clone());
            input.clicks.store(0, Ordering::Release);
            click::run(&runtime, &sense, &input, &NoClickSink, &config, &mut timing);
            input.clicks()
        });
    }

    // 2. The deadline maths on its own: the absolute-deadline rule that keeps sleep
    //    error from accumulating.
    {
        let mut previous = None;
        let mut now = Instant::now();
        let interval = Duration::from_millis(97);
        time("click.deadline", 2_000_000, 1, || {
            let schedule = ClickDeadlineScheduler::schedule_next(previous, now, interval);
            previous = Some(schedule.deadline);
            now = schedule.deadline;
            1
        });
    }
}
