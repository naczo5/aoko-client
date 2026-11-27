//! Deterministic allocation accounting for the automation loops.
//!
//! `MIGRATION_ARCHITECTURE.md` section 6 binds every hot loop to one claim that can
//! be checked without a clock: **allocation-free per tick**. The click engine is
//! the loop that runs at the highest rate, so it is the one measured here, on both
//! of its per-tick outcomes — the tick that clicks and the tick that only waits.
//!
//! Allocation counts are deterministic, so they are asserted here. Timings are
//! noisy, so they live in `benches/core.rs` and are recorded, not asserted.
//!
//! # Why a global allocator
//!
//! A `#[global_allocator]` is the only way to see allocations made by code that
//! never names an allocator — a `String` grown by `push`, `Vec` internal growth,
//! `format!`, a `Box` handed to a trait object. It is installed in test builds
//! only: this module is behind `#[cfg(test)]` in `lib.rs`, so the shipped library
//! and every dependent crate link `System` unchanged.
//!
//! The counter is a `Cell` in a `const`-initialised thread-local. That matters: a
//! lazily initialised thread-local would allocate on first touch, from inside the
//! allocator, and an allocator that allocates while counting would not terminate.
//! `const` initialisation registers no lazy slot and [`Allocations`] has no
//! destructor, so `try_with` is pure arithmetic.
//!
//! # What a number means
//!
//! [`measure`] reports the allocations the **calling thread** made between two
//! points, in both count and requested bytes, so a test is unaffected by the other
//! tests `cargo test` runs on its own threads. `alloc`, `alloc_zeroed` and
//! `realloc` each count once: a reallocation moves memory, so it is an allocation.
//! Frees are not counted.
//!
//! `aoko-protocol` carries its own copy of this module. A global allocator is
//! per-binary, so the two crates cannot share one.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::fmt;

/// One measurement: how many allocations, and how many bytes they asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Allocations {
    /// Allocation calls (`alloc`, `alloc_zeroed`, `realloc`).
    pub count: usize,
    /// Sum of the sizes those calls requested.
    pub bytes: usize,
}

impl Allocations {
    /// The count a path that must not allocate reports.
    pub const NONE: Self = Self { count: 0, bytes: 0 };

    fn since(self, earlier: Self) -> Self {
        Self {
            count: self.count.saturating_sub(earlier.count),
            bytes: self.bytes.saturating_sub(earlier.bytes),
        }
    }
}

impl fmt::Display for Allocations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} allocation(s), {} byte(s)", self.count, self.bytes)
    }
}

thread_local! {
    /// Allocations made on this thread since it started.
    static ALLOCATED: Cell<Allocations> = const { Cell::new(Allocations::NONE) };
}

/// Adds one allocation of `bytes` to this thread's total.
fn record(bytes: usize) {
    // `try_with` rather than `with`: during thread teardown the slot can already be
    // gone, and an allocator that panics then would abort the whole test binary.
    let _ = ALLOCATED.try_with(|total| {
        let before = total.get();
        total.set(Allocations {
            count: before.count + 1,
            bytes: before.bytes + bytes,
        });
    });
}

fn allocated() -> Allocations {
    ALLOCATED.try_with(Cell::get).unwrap_or_default()
}

/// Runs `f` and returns its value with the allocations made on this thread while
/// it ran.
///
/// `f` must not start a thread; anything it does on this thread is counted.
pub fn measure<T>(f: impl FnOnce() -> T) -> (T, Allocations) {
    let before = allocated();
    let value = f();
    (value, allocated().since(before))
}

/// Counts every allocation, then hands it to `System`.
struct CountingAllocator;

// SAFETY: every method forwards to `System` with the arguments it was given, so the
// global allocator's contract is `System`'s contract. The added work is a
// thread-local increment that neither allocates nor unwinds.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: `layout` is the caller's, forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: `layout` is the caller's, forwarded unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(new_size);
        // SAFETY: `ptr`, `layout` and `new_size` are the caller's, forwarded
        // unchanged, so the caller's invariants still hold.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` are the caller's, forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use parking_lot::Mutex;

    use aoko_platform::{InputBackend, InputEvent, Key, MouseButton, MouseState, PlatformError};
    use aoko_protocol::state::GameState;

    use super::*;
    use crate::app::{StateSnapshot, StateStore};
    use crate::clicker::AdvancedClickTimingState;
    use crate::clock::{Clock, Sleeper, SplitMix64};
    use crate::concurrency::CancelToken;
    use crate::loops::click::{self, ClickLoopConfig};
    use crate::loops::{LoopRuntime, LoopSense, NoClickSink};

    /// A clock and sleeper that never touches real time, and that stops the body on
    /// its first sleep so `click::run` executes exactly one tick and returns.
    ///
    /// Interiors are atomics and a mutex rather than `Cell`s because the loop seams
    /// require `Send + Sync`. None of it allocates per call.
    struct OneTick {
        origin: Instant,
        elapsed_nanos: AtomicU64,
        sleeps: AtomicU32,
        token: Mutex<Option<CancelToken>>,
    }

    impl OneTick {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                origin: Instant::now(),
                elapsed_nanos: AtomicU64::new(0),
                sleeps: AtomicU32::new(0),
                token: Mutex::new(None),
            })
        }

        fn attach(&self, token: CancelToken) {
            *self.token.lock() = Some(token);
        }

        fn sleeps(&self) -> u32 {
            self.sleeps.load(Ordering::Acquire)
        }
    }

    impl Clock for OneTick {
        fn now(&self) -> Instant {
            let elapsed = Duration::from_nanos(self.elapsed_nanos.load(Ordering::Acquire));
            self.origin.checked_add(elapsed).unwrap_or(self.origin)
        }
    }

    impl Sleeper for OneTick {
        fn sleep_until(&self, deadline: Instant) {
            let gap = deadline
                .checked_duration_since(self.now())
                .map_or(0, |gap| u64::try_from(gap.as_nanos()).unwrap_or(u64::MAX));
            self.elapsed_nanos.fetch_add(gap, Ordering::AcqRel);
            self.sleeps.fetch_add(1, Ordering::AcqRel);
            if let Some(token) = self.token.lock().as_ref() {
                token.cancel();
            }
        }
    }

    /// A runtime wired to `clock`, with the clock holding a handle to stop it.
    fn runtime(clock: &Arc<OneTick>) -> LoopRuntime {
        let as_clock = Arc::clone(clock) as Arc<dyn Clock>;
        let as_sleeper = Arc::clone(clock) as Arc<dyn Sleeper>;
        let runtime = LoopRuntime::new(as_clock, as_sleeper, Box::new(SplitMix64::new(0x5EED)));
        clock.attach(runtime.token().clone());
        runtime
    }

    /// A sense over one pre-built snapshot, so reading it is an `Arc` clone.
    struct FixedSense {
        snapshot: Arc<StateSnapshot>,
        active: bool,
    }

    impl LoopSense for FixedSense {
        fn snapshot(&self) -> Option<Arc<StateSnapshot>> {
            Some(Arc::clone(&self.snapshot))
        }
        fn is_connected(&self) -> bool {
            true
        }
        fn is_injected(&self) -> bool {
            true
        }
        fn minecraft_active(&self) -> bool {
            self.active
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

    /// An input backend that counts clicks and allocates nothing.
    #[derive(Default)]
    struct CountingInput {
        clicks: AtomicU32,
    }

    impl CountingInput {
        fn clicks(&self) -> u32 {
            self.clicks.load(Ordering::Acquire)
        }
    }

    impl InputBackend for CountingInput {
        fn mouse_state(&self) -> MouseState {
            MouseState::default()
        }
        fn key_state(&self, _key: Key) -> bool {
            false
        }
        fn click(&self, _button: MouseButton) -> Result<(), PlatformError> {
            self.clicks.fetch_add(1, Ordering::AcqRel);
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

    fn sense(active: bool) -> FixedSense {
        let state = GameState {
            is_connected: true,
            health: 20.0,
            ..GameState::default()
        };
        FixedSense {
            snapshot: Arc::new(StateSnapshot {
                state: Arc::new(state),
                at: Instant::now(),
            }),
            active,
        }
    }

    fn timing() -> AdvancedClickTimingState {
        AdvancedClickTimingState::new(
            OneTick::new() as Arc<dyn Clock>,
            Box::new(SplitMix64::new(1)),
        )
    }

    #[test]
    fn one_click_tick_allocates_nothing() {
        let sense = sense(true);
        let input = CountingInput::default();
        let config = ClickLoopConfig::left();
        let mut timing = timing();

        // Warm-up: the first tick, on its own runtime, pays whatever the loop builds
        // once — the not-connected default state, the RNG's first draw. That is not
        // per tick, so it must not be inside the measurement.
        let warm_clock = OneTick::new();
        let warm_runtime = runtime(&warm_clock);
        click::run(
            &warm_runtime,
            &sense,
            &input,
            &NoClickSink,
            &config,
            &mut timing,
        );
        assert_eq!(input.clicks(), 1, "the warm-up must be exactly one click");
        assert_eq!(warm_clock.sleeps(), 1, "one tick, one sleep");

        let clock = OneTick::new();
        let measured_runtime = runtime(&clock);
        let ((), allocations) = measure(|| {
            click::run(
                &measured_runtime,
                &sense,
                &input,
                &NoClickSink,
                &config,
                &mut timing,
            );
        });

        assert_eq!(
            input.clicks(),
            2,
            "the measured run must be exactly one tick"
        );
        assert_eq!(clock.sleeps(), 1, "one tick, one sleep");
        assert_eq!(
            allocations,
            Allocations::NONE,
            "a click tick must not touch the allocator"
        );
    }

    #[test]
    fn one_waiting_tick_allocates_nothing() {
        // The other per-tick outcome: a guard says "poll again later". It takes the
        // `Wait` branch, sleeps once and returns, and must be just as free.
        let sense = sense(false);
        let input = CountingInput::default();
        let config = ClickLoopConfig::left();
        let mut timing = timing();

        let warm_clock = OneTick::new();
        let warm_runtime = runtime(&warm_clock);
        click::run(
            &warm_runtime,
            &sense,
            &input,
            &NoClickSink,
            &config,
            &mut timing,
        );
        assert_eq!(input.clicks(), 0, "an inactive window never clicks");
        assert_eq!(warm_clock.sleeps(), 1, "one tick, one sleep");

        let clock = OneTick::new();
        let measured_runtime = runtime(&clock);
        let ((), allocations) = measure(|| {
            click::run(
                &measured_runtime,
                &sense,
                &input,
                &NoClickSink,
                &config,
                &mut timing,
            );
        });

        assert_eq!(input.clicks(), 0);
        assert_eq!(clock.sleeps(), 1, "one tick, one sleep");
        assert_eq!(
            allocations,
            Allocations::NONE,
            "a guard tick must not touch the allocator"
        );
    }

    #[test]
    fn publishing_a_shared_state_costs_the_snapshot_box_and_no_copy() {
        // The ingest side hands the codec's `Arc` straight to the store, so publishing a
        // frame costs the snapshot the store owns and nothing else. `publish` wraps an
        // owned state in a fresh `Arc` first, which is the copy this entry point exists to
        // avoid; a change that made this path unwrap and re-wrap would add one allocation
        // per frame here.
        let state = Arc::new(GameState {
            is_connected: true,
            health: 20.0,
            ..GameState::default()
        });
        let store = StateStore::new();
        let at = Instant::now();

        // Warm-up so the store's own machinery is not being measured on its first use.
        store.publish_shared(Arc::clone(&state), at);
        let ((), allocations) = measure(|| store.publish_shared(Arc::clone(&state), at));

        assert_eq!(
            allocations.count, 1,
            "one snapshot box, no state copy: {allocations}"
        );
        // Identity, not a refcount: this is true exactly when the store kept the
        // snapshot it was handed instead of copying the state into a new one.
        let stored = store.snapshot().expect("a state was published");
        assert!(
            Arc::ptr_eq(&stored.state, &state),
            "the store must share the snapshot it was handed"
        );
    }
}
