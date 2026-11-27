//! Collapses a burst of signals into one callback per interval.
//!
//! Port of `Aoko/Core/CoalescedCallback.cs`. The rules being preserved verbatim:
//!
//! - At most one callback is outstanding: a signal that arrives while one is
//!   pending is *dropped*, not queued (`CoalescedCallback.cs:34`).
//! - The callback rate is capped by `minimumIntervalMs`; the first signal of a
//!   burst fires immediately, the second waits out the remainder of the interval
//!   (`CoalescedCallback.cs:56-70`).
//! - The latest signal is never lost: a signal that arrives during an interval
//!   leaves `pending` set, so the flush that closes the interval is guaranteed.
//! - After `dispose` nothing runs, including a callback already queued
//!   (`CoalescedCallback.cs:74`).
//!
//! Differences forced by the port, both mechanical:
//!
//! - `System.Threading.Timer` (`CoalescedCallback.cs:27`) becomes a [`Scheduler`]
//!   seam so tests are not wall-clock dependent.
//! - `Environment.TickCount64` becomes a monotonic [`Instant`] from the
//!   scheduler's clock, so a wall-clock adjustment cannot fake an interval.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::scheduler::{ScheduledTask, Scheduler};

/// One callback, rate limited to `minimum_interval`.
pub struct CoalescedCallback {
    core: Arc<Core>,
}

struct Core {
    callback: Box<dyn Fn() + Send + Sync>,
    minimum_interval: Duration,
    scheduler: Arc<dyn Scheduler>,
    state: Mutex<State>,
    disposed: AtomicBool,
}

#[derive(Default)]
struct State {
    last_callback_at: Option<Instant>,
    pending: bool,
}

impl CoalescedCallback {
    /// Wires the callback to a scheduler and a minimum callback interval.
    ///
    /// The C# equivalent is `new CoalescedCallback(callback, minimumIntervalMs)`.
    #[must_use]
    pub fn new(
        callback: impl Fn() + Send + Sync + 'static,
        minimum_interval: Duration,
        scheduler: Arc<dyn Scheduler>,
    ) -> Self {
        Self {
            core: Arc::new(Core {
                callback: Box::new(callback),
                minimum_interval,
                scheduler,
                state: Mutex::new(State::default()),
                disposed: AtomicBool::new(false),
            }),
        }
    }

    /// Requests one callback. Cheap and safe to call per event; a call inside an
    /// active interval is folded into the trailing callback.
    pub fn signal(&self) {
        if self.core.disposed.load(Ordering::Acquire) {
            return;
        }

        let due = {
            let mut state = self.core.state.lock();
            if state.pending {
                // Drop the signal, keep the pending callback: exactly the
                // `CompareExchange(ref _pending, 1, 0)` early return.
                return;
            }
            let now = self.core.scheduler.now();
            let due =
                Self::calculate_due_time(state.last_callback_at, now, self.core.minimum_interval);
            state.pending = true;
            due
        };

        let core = Arc::clone(&self.core);
        let accepted = self
            .core
            .scheduler
            .schedule_in(due, Box::new(move || core.invoke()) as ScheduledTask);
        if !accepted {
            // `Timer.Change` threw on a disposed timer; clear `pending` so the
            // next signal is not swallowed.
            self.core.state.lock().pending = false;
        }
    }

    /// Stops all future callbacks. Idempotent.
    pub fn dispose(&self) {
        if !self.core.disposed.swap(true, Ordering::AcqRel) {
            self.core.state.lock().pending = false;
        }
    }

    /// Whether a callback is scheduled or currently running.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.core.state.lock().pending
    }

    /// Delay before the next callback is allowed to run.
    ///
    /// Verbatim port of `CoalescedCallback.CalculateDueTime`
    /// (`Aoko/Core/CoalescedCallback.cs:56-70`). The `last == 0` sentinel of the
    /// C# code becomes `None`, and the `elapsed < 0` branch (only reachable with
    /// a tick counter that can run backwards) becomes "the previous callback is
    /// stamped in the future", for which the C# code waits a full interval.
    #[must_use]
    pub fn calculate_due_time(
        last_callback_at: Option<Instant>,
        now: Instant,
        minimum_interval: Duration,
    ) -> Duration {
        let Some(last) = last_callback_at else {
            return Duration::ZERO;
        };
        if minimum_interval.is_zero() {
            return Duration::ZERO;
        }

        match now.checked_duration_since(last) {
            None => minimum_interval,
            Some(elapsed) if elapsed >= minimum_interval => Duration::ZERO,
            Some(elapsed) => minimum_interval.saturating_sub(elapsed),
        }
    }
}

impl Drop for CoalescedCallback {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl std::fmt::Debug for CoalescedCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoalescedCallback")
            .field("minimum_interval", &self.core.minimum_interval)
            .field("disposed", &self.core.disposed.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl Core {
    fn invoke(self: &Arc<Self>) {
        if self.disposed.load(Ordering::Acquire) {
            return;
        }

        {
            let mut state = self.state.lock();
            state.last_callback_at = Some(self.scheduler.now());
            state.pending = false;
        }

        // A panicking subscriber must not take the scheduler thread down with it.
        // Under the release profile (`panic = "abort"`) this cannot catch, which
        // is why no library path in this crate panics.
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.callback)())).is_err() {
            tracing::error!("coalesced callback subscriber panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ManualScheduler, VirtualClock};
    use std::sync::atomic::AtomicU32;

    #[test]
    fn calculate_due_time_enforces_the_minimum_interval() {
        let base = Instant::now();
        let interval = Duration::from_millis(25);
        let at = |ms: u64| base + Duration::from_millis(ms);

        // CoalescedCallbackTests.CalculateDueTime_EnforcesMinimumInterval rows.
        assert_eq!(
            CoalescedCallback::calculate_due_time(None, at(100), interval),
            Duration::ZERO
        );
        assert_eq!(
            CoalescedCallback::calculate_due_time(Some(at(100)), at(125), interval),
            Duration::ZERO
        );
        assert_eq!(
            CoalescedCallback::calculate_due_time(Some(at(100)), at(110), interval),
            Duration::from_millis(15)
        );
        assert_eq!(
            CoalescedCallback::calculate_due_time(Some(at(100)), at(99), interval),
            interval
        );
        assert_eq!(
            CoalescedCallback::calculate_due_time(Some(at(100)), at(110), Duration::ZERO),
            Duration::ZERO
        );
    }

    #[test]
    fn signal_collapses_a_burst_and_delivers_one_trailing_callback() {
        let clock = Arc::new(VirtualClock::new());
        let scheduler = Arc::new(ManualScheduler::new(
            Arc::clone(&clock) as Arc<dyn crate::clock::Clock>
        ));
        let calls = Arc::new(AtomicU32::new(0));

        let counter = Arc::clone(&calls);
        let coalesced = CoalescedCallback::new(
            move || {
                counter.fetch_add(1, Ordering::AcqRel);
            },
            Duration::from_millis(30),
            Arc::clone(&scheduler) as Arc<dyn Scheduler>,
        );

        coalesced.signal();
        assert_eq!(scheduler.len(), 1, "the first signal schedules immediately");
        assert!(scheduler.run_next());
        assert_eq!(calls.load(Ordering::Acquire), 1);

        for _ in 0..50 {
            coalesced.signal();
        }
        assert_eq!(
            scheduler.len(),
            1,
            "50 signals inside one interval must collapse into one callback"
        );

        assert_eq!(scheduler.run_all(), 1);
        assert_eq!(calls.load(Ordering::Acquire), 2);
        assert!(scheduler.is_empty());
        assert!(!coalesced.is_pending());
    }

    #[test]
    fn signal_waits_out_the_remainder_of_an_active_interval() {
        let clock = Arc::new(VirtualClock::new());
        let scheduler = Arc::new(ManualScheduler::new(
            Arc::clone(&clock) as Arc<dyn crate::clock::Clock>
        ));
        let calls = Arc::new(AtomicU32::new(0));

        let counter = Arc::clone(&calls);
        let coalesced = CoalescedCallback::new(
            move || {
                counter.fetch_add(1, Ordering::AcqRel);
            },
            Duration::from_millis(30),
            Arc::clone(&scheduler) as Arc<dyn Scheduler>,
        );

        coalesced.signal();
        scheduler.run_next();

        // 10 ms into a 30 ms interval the next callback is due 20 ms out.
        clock.advance(Duration::from_millis(10));
        let before = scheduler.now();
        coalesced.signal();
        let deadline = scheduler.next_deadline().expect("a callback was scheduled");
        assert_eq!(deadline.duration_since(before), Duration::from_millis(20));

        clock.advance(Duration::from_millis(19));
        assert!(
            !scheduler.run_due(),
            "19 ms in is still inside the interval"
        );
        clock.advance(Duration::from_millis(1));
        assert!(scheduler.run_due());
        assert_eq!(calls.load(Ordering::Acquire), 2);
    }

    #[test]
    fn dispose_prevents_a_queued_callback_from_running() {
        let clock = Arc::new(VirtualClock::new());
        let scheduler = Arc::new(ManualScheduler::new(
            Arc::clone(&clock) as Arc<dyn crate::clock::Clock>
        ));
        let calls = Arc::new(AtomicU32::new(0));

        let counter = Arc::clone(&calls);
        let coalesced = CoalescedCallback::new(
            move || {
                counter.fetch_add(1, Ordering::AcqRel);
            },
            Duration::from_millis(30),
            Arc::clone(&scheduler) as Arc<dyn Scheduler>,
        );

        coalesced.signal();
        coalesced.dispose();
        scheduler.run_all();

        assert_eq!(calls.load(Ordering::Acquire), 0);
        assert!(!coalesced.is_pending());

        // Signals after disposal are ignored rather than scheduled.
        coalesced.signal();
        assert!(scheduler.is_empty());
    }
}
