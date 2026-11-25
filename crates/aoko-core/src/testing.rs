//! Deterministic doubles for the seams in [`crate::clock`] and
//! [`crate::concurrency`].
//!
//! The rule the whole crate is built around: a test never reads the wall clock and
//! never sleeps. [`VirtualClock`] moves only when something sleeps on it (or when a
//! test advances it by hand), and [`ManualScheduler`] queues work that the test
//! drains explicitly.
//!
//! Available under `cfg(test)` and under the `testing` feature, so a dependent
//! crate's tests can wire the same doubles without duplicating them.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::clock::{Clock, Sleeper};
use crate::concurrency::{ScheduledTask, Scheduler};

/// A manually driven monotonic clock.
///
/// Time only moves when [`Sleeper::sleep_until`] is called, so a controller that
/// schedules a 25 ms callback observes exactly 25 ms of virtual time and no real
/// time at all.
#[derive(Debug)]
pub struct VirtualClock {
    state: Mutex<VirtualState>,
}

#[derive(Debug)]
struct VirtualState {
    now: Instant,
    gaps: Vec<Duration>,
    sleep_calls: u32,
}

impl VirtualClock {
    /// Starts at the current instant, which is only used as an origin; every
    /// assertion in this crate is relative.
    #[must_use]
    pub fn new() -> Self {
        Self::from_instant(Instant::now())
    }

    /// Starts at an explicit origin, so a test can build deadlines without
    /// depending on when the test ran.
    #[must_use]
    pub fn from_instant(origin: Instant) -> Self {
        Self {
            state: Mutex::new(VirtualState {
                now: origin,
                gaps: Vec::new(),
                sleep_calls: 0,
            }),
        }
    }

    /// Moves time forward without sleeping. Used to describe "10 ms later" from a
    /// test instead of from a sleeper.
    pub fn advance(&self, by: Duration) {
        let mut state = self.state.lock();
        state.now = state.now.checked_add(by).unwrap_or(state.now);
    }

    /// The gaps the sleeper was asked to wait out, in order.
    #[must_use]
    pub fn gaps(&self) -> Vec<Duration> {
        self.state.lock().gaps.clone()
    }

    /// Sum of every gap.
    #[must_use]
    pub fn total_slept(&self) -> Duration {
        self.state
            .lock()
            .gaps
            .iter()
            .copied()
            .fold(Duration::ZERO, |total, gap| total + gap)
    }

    /// How many times a sleeper was asked to wait, including waits for a deadline
    /// that had already passed.
    #[must_use]
    pub fn sleep_calls(&self) -> u32 {
        self.state.lock().sleep_calls
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for VirtualClock {
    fn now(&self) -> Instant {
        self.state.lock().now
    }
}

impl Sleeper for VirtualClock {
    fn sleep_until(&self, deadline: Instant) {
        let mut state = self.state.lock();
        state.sleep_calls += 1;
        // A deadline in the past is a no-op, like every real sleeper.
        if let Some(gap) = deadline.checked_duration_since(state.now) {
            state.now = deadline;
            state.gaps.push(gap);
        }
    }
}

/// A scheduler that only queues, so a test decides when work runs.
pub struct ManualScheduler {
    clock: Arc<dyn Clock>,
    sequence: AtomicU64,
    queued: Mutex<Vec<ManualEntry>>,
}

struct ManualEntry {
    deadline: Instant,
    sequence: u64,
    task: ScheduledTask,
}

impl ManualScheduler {
    /// Wires the scheduler to a clock. Time comes from the clock, so pairing this
    /// with [`VirtualClock`] keeps due times deterministic.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            sequence: AtomicU64::new(0),
            queued: Mutex::new(Vec::new()),
        }
    }

    /// Number of queued tasks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queued.lock().len()
    }

    /// Whether nothing is queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queued.lock().is_empty()
    }

    /// Deadline of the earliest queued task.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.queued.lock().iter().map(|entry| entry.deadline).min()
    }

    /// Runs the earliest queued task regardless of its deadline. Returns whether
    /// one ran.
    pub fn run_next(&self) -> bool {
        let task = {
            let mut queued = self.queued.lock();
            let index = queued
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| (entry.deadline, entry.sequence))
                .map(|(index, _)| index);
            index.map(|index| queued.remove(index).task)
        };

        match task {
            Some(task) => {
                task();
                true
            }
            None => false,
        }
    }

    /// Runs the earliest task only if its deadline has been reached. Returns
    /// whether one ran.
    pub fn run_due(&self) -> bool {
        let now = self.clock.now();
        let is_due = self.queued.lock().iter().any(|entry| entry.deadline <= now);
        if !is_due {
            return false;
        }
        self.run_next()
    }

    /// Drains every queued task, including ones queued while draining. Returns how
    /// many ran.
    ///
    /// A task that re-queues itself with a zero delay would loop forever; tests
    /// that want that behaviour drive [`Self::run_next`] instead.
    pub fn run_all(&self) -> usize {
        let mut ran = 0;
        while self.run_next() {
            ran += 1;
        }
        ran
    }
}

impl std::fmt::Debug for ManualScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManualScheduler")
            .field("queued", &self.len())
            .finish_non_exhaustive()
    }
}

impl Scheduler for ManualScheduler {
    fn now(&self) -> Instant {
        self.clock.now()
    }

    fn schedule_at(&self, deadline: Instant, task: ScheduledTask) -> bool {
        let entry = ManualEntry {
            deadline,
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            task,
        };
        self.queued.lock().push(entry);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_virtual_clock_only_moves_when_it_is_told_to() {
        let clock = VirtualClock::new();
        let start = clock.now();
        assert_eq!(clock.now(), start);

        clock.advance(Duration::from_millis(40));
        assert_eq!(clock.now(), start + Duration::from_millis(40));

        clock.sleep_until(start + Duration::from_millis(100));
        assert_eq!(clock.now(), start + Duration::from_millis(100));
        assert_eq!(clock.gaps(), vec![Duration::from_millis(60)]);
        assert_eq!(clock.total_slept(), Duration::from_millis(60));

        // A past deadline advances nothing but is still counted.
        clock.sleep_until(start);
        assert_eq!(clock.now(), start + Duration::from_millis(100));
        assert_eq!(clock.sleep_calls(), 2);
    }

    #[test]
    fn the_manual_scheduler_runs_the_earliest_task_first() {
        let clock = Arc::new(VirtualClock::new());
        let scheduler = ManualScheduler::new(Arc::clone(&clock) as Arc<dyn Clock>);
        let order = Arc::new(Mutex::new(Vec::new()));
        let start = scheduler.now();

        for (delay_ms, label) in [(20_u64, 2_u8), (10, 1), (30, 3)] {
            let order = Arc::clone(&order);
            assert!(scheduler.schedule_at(
                start + Duration::from_millis(delay_ms),
                Box::new(move || order.lock().push(label)),
            ));
        }

        assert_eq!(scheduler.len(), 3);
        assert_eq!(
            scheduler.next_deadline(),
            Some(start + Duration::from_millis(10))
        );

        // Nothing is due at t=0.
        assert!(!scheduler.run_due());
        assert_eq!(scheduler.run_all(), 3);
        assert_eq!(*order.lock(), vec![1, 2, 3]);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn a_task_queued_while_draining_is_run_by_run_all() {
        let clock = Arc::new(VirtualClock::new());
        let scheduler = Arc::new(ManualScheduler::new(Arc::clone(&clock) as Arc<dyn Clock>));
        let ran = Arc::new(AtomicU64::new(0));

        let nested = Arc::clone(&scheduler);
        let nested_ran = Arc::clone(&ran);
        assert!(scheduler.schedule_at(
            scheduler.now(),
            Box::new(move || {
                nested_ran.fetch_add(1, Ordering::AcqRel);
                assert!(nested.schedule_at(nested.now(), Box::new(|| {})));
            }),
        ));

        assert_eq!(scheduler.run_all(), 2);
        assert_eq!(ran.load(Ordering::Acquire), 1);
    }
}
