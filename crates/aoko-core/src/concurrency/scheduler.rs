//! Deferred work delivery.
//!
//! The C# coalescers rely on a `System.Threading.Timer`
//! (`Aoko/Core/CoalescedCallback.cs:27`) or on the WPF dispatcher
//! (`Aoko/Core/CoalescedLatestValue.cs:11`). Neither is available headlessly and
//! both make tests wall-clock dependent, so both are replaced by this seam: the
//! coalescers ask a [`Scheduler`] to run a task at an absolute deadline, and the
//! test wiring supplies a queue the test drains by hand.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use crate::clock::Clock;

/// A task handed to a [`Scheduler`]. Runs at most once.
pub type ScheduledTask = Box<dyn FnOnce() + Send + 'static>;

/// Runs a task once, no earlier than an absolute deadline.
pub trait Scheduler: Send + Sync {
    /// Monotonic time the scheduler orders deadlines against. Exposed here so a
    /// consumer can measure "how long since the last callback" without being
    /// handed a second clock.
    fn now(&self) -> Instant;

    /// Queues `task` for `deadline`. Returns `false` when the scheduler is shut
    /// down and the task was dropped.
    ///
    /// The `bool` stands in for the `ObjectDisposedException` that
    /// `Timer.Change` throws in C# (`CoalescedCallback.cs:44`): the caller has to
    /// be able to clear its pending flag instead of losing the signal.
    fn schedule_at(&self, deadline: Instant, task: ScheduledTask) -> bool;

    /// Convenience wrapper for a relative delay.
    fn schedule_in(&self, delay: Duration, task: ScheduledTask) -> bool {
        let deadline = self.now().checked_add(delay).unwrap_or_else(|| self.now());
        self.schedule_at(deadline, task)
    }
}

/// A pending task ordered by deadline, then by insertion order.
///
/// Ordering is what `BinaryHeap` needs; the task itself is never compared.
struct TimerEntry {
    deadline: Instant,
    sequence: u64,
    task: ScheduledTask,
}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline && self.sequence == other.sequence
    }
}

impl Eq for TimerEntry {}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.deadline
            .cmp(&other.deadline)
            .then_with(|| self.sequence.cmp(&other.sequence))
    }
}

/// Production scheduler: one worker thread owning a deadline heap.
///
/// One thread per scheduler, not one per task, so a 25 ms config coalescer
/// (`Aoko-protocol::cadence::CONFIG_CHANGE_COALESCE_MS`) does not spawn a thread
/// per slider pixel. The worker blocks in `recv_timeout` on the next deadline;
/// there is no polling and no idle wake-up.
pub struct TimerScheduler {
    clock: Arc<dyn Clock>,
    sender: Option<Sender<TimerEntry>>,
    worker: Option<std::thread::JoinHandle<()>>,
    sequence: AtomicU64,
}

impl TimerScheduler {
    /// Starts the worker thread.
    ///
    /// # Errors
    /// Returns the OS error when the worker thread cannot be spawned.
    pub fn new(clock: Arc<dyn Clock>) -> io::Result<Self> {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let worker_clock = Arc::clone(&clock);
        let worker = std::thread::Builder::new()
            .name("aoko-timer".to_owned())
            .spawn(move || run_worker(&worker_clock, &receiver))?;
        Ok(Self {
            clock,
            sender: Some(sender),
            worker: Some(worker),
            sequence: AtomicU64::new(0),
        })
    }
}

impl std::fmt::Debug for TimerScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimerScheduler")
            .field("live", &self.sender.is_some())
            .finish_non_exhaustive()
    }
}

impl Scheduler for TimerScheduler {
    fn now(&self) -> Instant {
        self.clock.now()
    }

    fn schedule_at(&self, deadline: Instant, task: ScheduledTask) -> bool {
        let Some(sender) = self.sender.as_ref() else {
            tracing::warn!("schedule_at after shutdown; task dropped");
            return false;
        };
        let entry = TimerEntry {
            deadline,
            sequence: self.sequence.fetch_add(1, AtomicOrdering::Relaxed),
            task,
        };
        sender.send(entry).is_ok()
    }
}

impl Drop for TimerScheduler {
    fn drop(&mut self) {
        // Closing the channel ends the worker after the task it is running.
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_worker(clock: &Arc<dyn Clock>, receiver: &Receiver<TimerEntry>) {
    let mut heap: BinaryHeap<std::cmp::Reverse<TimerEntry>> = BinaryHeap::new();

    loop {
        // Run everything already due, including tasks queued by a task that just
        // ran with a zero delay.
        while let Some(next) = heap.peek().map(|entry| entry.0.deadline) {
            if next > clock.now() {
                break;
            }
            if let Some(entry) = heap.pop() {
                (entry.0.task)();
            }
        }

        match heap.peek().map(|entry| entry.0.deadline) {
            None => match receiver.recv() {
                Ok(entry) => heap.push(std::cmp::Reverse(entry)),
                Err(_) => return,
            },
            Some(next) => {
                let wait = next
                    .checked_duration_since(clock.now())
                    .unwrap_or(Duration::ZERO);
                match receiver.recv_timeout(wait) {
                    Ok(entry) => heap.push(std::cmp::Reverse(entry)),
                    // Re-check for due work.
                    Err(RecvTimeoutError::Timeout) => {}
                    // Shutdown: queued work is dropped rather than run late.
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SystemClock;
    use std::sync::mpsc;

    #[test]
    fn timer_scheduler_runs_a_task_once_at_its_deadline() {
        let scheduler = TimerScheduler::new(Arc::new(SystemClock)).expect("timer thread");
        let (tx, rx) = mpsc::channel();

        let accepted = scheduler.schedule_in(
            Duration::from_millis(2),
            Box::new(move || {
                let _ = tx.send(1_u8);
            }),
        );
        assert!(accepted);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(1));
        assert!(
            rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "a scheduled task must run exactly once"
        );
    }

    #[test]
    fn timer_scheduler_orders_past_due_work_before_later_work() {
        let scheduler = TimerScheduler::new(Arc::new(SystemClock)).expect("timer thread");
        let (tx, rx) = mpsc::channel();
        let now = scheduler.now();

        // Insertion order is deliberately the opposite of deadline order.
        assert!(scheduler.schedule_at(
            now + Duration::from_millis(20),
            Box::new({
                let tx = tx.clone();
                move || {
                    let _ = tx.send("later");
                }
            }),
        ));
        assert!(scheduler.schedule_at(
            now,
            Box::new(move || {
                let _ = tx.send("now");
            }),
        ));

        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok("now"));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok("later"));
    }

    #[test]
    fn dropping_the_scheduler_does_not_wait_out_pending_deadlines() {
        let scheduler = TimerScheduler::new(Arc::new(SystemClock)).expect("timer thread");
        assert!(scheduler.schedule_in(Duration::from_secs(3_600), Box::new(|| {})));

        let started = Instant::now();
        // Would block for an hour if the worker slept through the deadline
        // instead of observing the closed channel.
        drop(scheduler);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
