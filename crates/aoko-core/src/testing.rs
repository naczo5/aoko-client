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

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// One call a loop made into the input backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputCall {
    /// `click(button)`.
    Click(aoko_platform::MouseButton),
    /// `move_relative(dx, dy)`.
    Move(i32, i32),
    /// `key_down(key)`.
    KeyDown(aoko_platform::Key),
    /// `key_up(key)`.
    KeyUp(aoko_platform::Key),
}

/// An [`aoko_platform::InputBackend`] that records instead of injecting.
///
/// `cancel_after_clicks` makes a loop stop itself deterministically after a fixed
/// number of clicks, which is how the thread-lifecycle tests avoid any wall-clock
/// coordination with the loop thread.
#[derive(Debug, Default)]
pub struct FakeInput {
    calls: Mutex<Vec<InputCall>>,
    left_down: AtomicBool,
    unobservable: AtomicBool,
    cancel_after_clicks: AtomicU64,
    cancel: Mutex<Option<crate::concurrency::CancelToken>>,
}

impl FakeInput {
    /// Nothing recorded, left button up.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels `token` once this many clicks have been recorded.
    pub fn cancel_after_clicks(&self, clicks: u64, token: crate::concurrency::CancelToken) {
        self.cancel_after_clicks
            .store(clicks.max(1), Ordering::Release);
        *self.cancel.lock() = Some(token);
    }

    /// Makes `mouse_state().left_down` report this.
    pub fn set_left_down(&self, down: bool) {
        self.left_down.store(down, Ordering::Release);
    }

    /// Makes the backend report that it cannot see the mouse at all, the way a
    /// Wayland session without `/dev/input` access does: `probe_mouse_state`
    /// returns `None` while `mouse_state` stays lossy.
    pub fn set_mouse_unobservable(&self, unobservable: bool) {
        self.unobservable.store(unobservable, Ordering::Release);
    }

    /// Recorded calls, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<InputCall> {
        self.calls.lock().clone()
    }

    /// Number of recorded calls.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.lock().len()
    }

    /// Number of recorded clicks.
    #[must_use]
    pub fn click_count(&self) -> usize {
        self.calls
            .lock()
            .iter()
            .filter(|call| matches!(call, InputCall::Click(_)))
            .count()
    }

    /// Number of recorded relative moves.
    #[must_use]
    pub fn move_count(&self) -> usize {
        self.calls
            .lock()
            .iter()
            .filter(|call| matches!(call, InputCall::Move(..)))
            .count()
    }

    /// Forgets everything recorded.
    pub fn clear(&self) {
        self.calls.lock().clear();
    }

    fn record(&self, call: InputCall) {
        let mut calls = self.calls.lock();
        calls.push(call);
        let clicks = calls
            .iter()
            .filter(|call| matches!(call, InputCall::Click(_)))
            .count() as u64;
        drop(calls);

        let threshold = self.cancel_after_clicks.load(Ordering::Acquire);
        if threshold > 0 && clicks >= threshold {
            if let Some(token) = self.cancel.lock().as_ref() {
                token.cancel();
            }
        }
    }
}

impl aoko_platform::InputBackend for FakeInput {
    fn probe_mouse_state(&self) -> Option<aoko_platform::MouseState> {
        if self.unobservable.load(Ordering::Acquire) {
            return None;
        }
        // Explicit, not inherited: the trait default is fail-closed (`None`), and
        // this fake models a session that *can* observe, so a test relying on
        // observable input must not silently switch to "unobservable" behaviour
        // behind a green suite.
        Some(self.mouse_state())
    }

    fn probe_key_state(&self, key: aoko_platform::Key) -> Option<bool> {
        // Same reasoning: the fake observes keys and always answers `up`.
        Some(self.key_state(key))
    }

    fn mouse_state(&self) -> aoko_platform::MouseState {
        aoko_platform::MouseState {
            left_down: self.left_down.load(Ordering::Acquire),
            right_down: false,
        }
    }

    fn key_state(&self, _key: aoko_platform::Key) -> bool {
        false
    }

    fn click(
        &self,
        button: aoko_platform::MouseButton,
    ) -> Result<(), aoko_platform::PlatformError> {
        self.record(InputCall::Click(button));
        Ok(())
    }

    fn move_relative(&self, dx: i32, dy: i32) -> Result<(), aoko_platform::PlatformError> {
        self.record(InputCall::Move(dx, dy));
        Ok(())
    }

    fn key_press(&self, key: aoko_platform::Key) -> Result<(), aoko_platform::PlatformError> {
        self.record(InputCall::KeyDown(key));
        self.record(InputCall::KeyUp(key));
        Ok(())
    }

    fn poll_events(
        &self,
        _out: &mut Vec<aoko_platform::InputEvent>,
    ) -> Result<(), aoko_platform::PlatformError> {
        Ok(())
    }

    fn key_down(&self, key: aoko_platform::Key) -> Result<(), aoko_platform::PlatformError> {
        self.record(InputCall::KeyDown(key));
        Ok(())
    }

    fn key_up(&self, key: aoko_platform::Key) -> Result<(), aoko_platform::PlatformError> {
        self.record(InputCall::KeyUp(key));
        Ok(())
    }
}

/// A sleeper that parks inside `sleep_until` until the test releases it.
///
/// This is the one place a test does coordinate with a loop thread, and it does so
/// through a condvar rather than a duration: `entered` proves the loop is inside a
/// sleep, so "cancel while sleeping" is an ordering guarantee, not a race.
#[derive(Debug, Default)]
pub struct GateSleeper {
    state: Mutex<GateState>,
    cvar: parking_lot::Condvar,
}

#[derive(Debug, Default)]
struct GateState {
    entered: bool,
    released: bool,
}

impl GateSleeper {
    /// Closed gate: the first `sleep_until` parks.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Blocks until a loop has parked in `sleep_until`.
    pub fn wait_until_entered(&self) {
        let mut state = self.state.lock();
        while !state.entered {
            self.cvar.wait(&mut state);
        }
    }

    /// Lets every parked wait return.
    pub fn release(&self) {
        let mut state = self.state.lock();
        state.released = true;
        self.cvar.notify_all();
    }

    /// Whether a loop parked at least once.
    #[must_use]
    pub fn entered(&self) -> bool {
        self.state.lock().entered
    }
}

impl Sleeper for GateSleeper {
    fn sleep_until(&self, _deadline: Instant) {
        let mut state = self.state.lock();
        state.entered = true;
        self.cvar.notify_all();
        while !state.released {
            self.cvar.wait(&mut state);
        }
    }
}

/// Advances a [`VirtualClock`] and cancels a token after a fixed number of sleeps.
///
/// Lets a test pin down a loop's delay schedule without running a thread: the loop
/// sleeps `n` times, then stops, and `clock.gaps()` is the exact sequence. The token
/// is attached after the runtime exists, because the runtime owns it.
#[derive(Debug)]
pub struct CancelAfterSleeper {
    clock: Arc<VirtualClock>,
    token: Mutex<Option<crate::concurrency::CancelToken>>,
    after: u32,
    sleeps: AtomicU64,
}

impl CancelAfterSleeper {
    /// Cancels the attached token on the `after`-th sleep.
    #[must_use]
    pub fn new(clock: Arc<VirtualClock>, after: u32) -> Self {
        Self {
            clock,
            token: Mutex::new(None),
            after: after.max(1),
            sleeps: AtomicU64::new(0),
        }
    }

    /// Attaches the runtime's token. Call once, after building the runtime.
    pub fn attach(&self, token: crate::concurrency::CancelToken) {
        *self.token.lock() = Some(token);
    }

    /// How many sleeps were observed.
    #[must_use]
    pub fn sleeps(&self) -> u64 {
        self.sleeps.load(Ordering::Acquire)
    }
}

impl Sleeper for CancelAfterSleeper {
    fn sleep_until(&self, deadline: Instant) {
        self.clock.sleep_until(deadline);
        let count = self.sleeps.fetch_add(1, Ordering::AcqRel) + 1;
        if count >= u64::from(self.after) {
            if let Some(token) = self.token.lock().as_ref() {
                token.cancel();
            }
        }
    }
}
