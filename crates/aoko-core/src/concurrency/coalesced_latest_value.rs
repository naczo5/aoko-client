//! Keeps the newest published value and one scheduled flush.
//!
//! Port of `Aoko/Core/CoalescedLatestValue.cs`. The rules being preserved:
//!
//! - `pending` allows at most one scheduled drain; every later publish inside
//!   that window only overwrites the retained value, so a burst collapses to one
//!   callback carrying the *newest* value (drop-oldest, keep-latest).
//! - A value published while the consumer is running schedules exactly one
//!   trailing drain, never a queue (`CoalescedLatestValue.cs:104-124`). This is
//!   the flush guarantee: a value published during the final drain is still
//!   delivered.
//! - `allow_schedule: false` retains the value for a later publish that is
//!   allowed to schedule, so a suppressed publish is deferred, never dropped
//!   (`CoalescedLatestValue.cs:42`).
//! - After `dispose` nothing is delivered, matching C#: the guarantee is "never
//!   lose the latest value while running", not "deliver during teardown". The
//!   retained value is released eagerly instead of being pinned until the owner
//!   drops, which is unobservable except in a heap profile.
//!
//! The consumer runs outside the lock, because the C# consumer is allowed to
//! publish again from inside the callback, and `CoalescedLatestValueTests`
//! does exactly that.

use std::sync::Arc;

use parking_lot::Mutex;

use super::scheduler::ScheduledTask;

/// Schedules the drain. Returns `false` when the sink refuses work, which is the
/// Rust spelling of the `ObjectDisposedException` the C# catch block handles.
pub type ScheduleFn = Box<dyn Fn(ScheduledTask) -> bool + Send + Sync>;

/// Consumes one drained value.
pub type ConsumeFn<T> = Box<dyn Fn(&T) + Send + Sync>;

/// Latest-value-wins coalescer.
pub struct CoalescedLatestValue<T> {
    core: Arc<Core<T>>,
}

struct Core<T> {
    schedule: ScheduleFn,
    consume: ConsumeFn<T>,
    state: Mutex<State<T>>,
}

struct State<T> {
    latest: Option<Arc<T>>,
    generation: u64,
    pending: bool,
    disposed: bool,
}

impl<T> CoalescedLatestValue<T>
where
    T: Send + Sync + 'static,
{
    /// Builds a coalescer over an explicit schedule/consume pair.
    ///
    /// `schedule` is `Action<Action>` in C#: it receives the drain action and
    /// reports whether it accepted it.
    pub fn new(
        schedule: impl Fn(ScheduledTask) -> bool + Send + Sync + 'static,
        consume: impl Fn(&T) + Send + Sync + 'static,
    ) -> Self {
        Self {
            core: Arc::new(Core {
                schedule: Box::new(schedule),
                consume: Box::new(consume),
                state: Mutex::new(State {
                    latest: None,
                    generation: 0,
                    pending: false,
                    disposed: false,
                }),
            }),
        }
    }

    /// Publishes a value. Returns whether this call scheduled a drain.
    pub fn publish(&self, value: T, allow_schedule: bool) -> bool {
        {
            let mut state = self.core.state.lock();
            if state.disposed {
                return false;
            }

            state.latest = Some(Arc::new(value));
            state.generation = state.generation.wrapping_add(1);
            if state.pending || !allow_schedule {
                return false;
            }
            state.pending = true;
        }

        let core = Arc::clone(&self.core);
        let accepted = (self.core.schedule)(Box::new(move || Core::drain(&core)));
        if !accepted {
            self.core.state.lock().pending = false;
        }
        accepted
    }

    /// Drops the retained value and refuses further publishes.
    pub fn dispose(&self) {
        let mut state = self.core.state.lock();
        state.disposed = true;
        state.pending = false;
        state.latest = None;
    }

    /// Whether a drain is scheduled or running.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.core.state.lock().pending
    }
}

impl<T> Drop for CoalescedLatestValue<T> {
    fn drop(&mut self) {
        let mut state = self.core.state.lock();
        state.disposed = true;
        state.pending = false;
        state.latest = None;
    }
}

impl<T> std::fmt::Debug for CoalescedLatestValue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoalescedLatestValue")
            .finish_non_exhaustive()
    }
}

impl<T> Core<T>
where
    T: Send + Sync + 'static,
{
    fn drain(self: &Arc<Self>) {
        let drained = {
            let mut state = self.state.lock();
            match (&state.latest, state.disposed) {
                (_, true) | (None, _) => {
                    state.pending = false;
                    None
                }
                (Some(value), false) => Some((Arc::clone(value), state.generation)),
            }
        };

        let Some((value, generation)) = drained else {
            return;
        };

        // Outside the lock: the consumer may publish again, and the C# code
        // explicitly allows that.
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.consume)(&value)))
            .is_err()
        {
            tracing::error!("coalesced value consumer panicked");
        }

        let schedule_trailing = {
            let mut state = self.state.lock();
            if state.disposed {
                state.pending = false;
                return;
            }
            if state.generation == generation {
                state.pending = false;
                false
            } else {
                // A publish landed while the consumer ran: keep `pending` set and
                // deliver exactly one trailing drain.
                true
            }
        };

        if schedule_trailing {
            let core = Arc::clone(self);
            let accepted = (self.schedule)(Box::new(move || Core::drain(&core)));
            if !accepted {
                self.state.lock().pending = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Records scheduled drains so a test can run them by hand, the way
    /// `CoalescedLatestValueTests` uses a local `Queue<Action>`.
    #[derive(Default)]
    struct QueueScheduler {
        queued: Mutex<VecDeque<ScheduledTask>>,
    }

    impl QueueScheduler {
        fn len(&self) -> usize {
            self.queued.lock().len()
        }

        fn run_next(&self) -> bool {
            let task = self.queued.lock().pop_front();
            match task {
                Some(task) => {
                    task();
                    true
                }
                None => false,
            }
        }
    }

    fn enqueue(scheduler: &Arc<QueueScheduler>) -> impl Fn(ScheduledTask) -> bool + Send + Sync {
        let sink = Arc::clone(scheduler);
        move |task| {
            sink.queued.lock().push_back(task);
            true
        }
    }

    #[test]
    fn pending_publishes_are_collapsed_to_the_latest_value() {
        let scheduler = Arc::new(QueueScheduler::default());
        let consumed = Arc::new(Mutex::new(Vec::<String>::new()));

        let recorder = Arc::clone(&consumed);
        let coalesced = CoalescedLatestValue::new(enqueue(&scheduler), move |value: &String| {
            recorder.lock().push(value.clone());
        });

        assert!(coalesced.publish("first".to_owned(), true));
        assert!(!coalesced.publish("second".to_owned(), true));
        assert!(!coalesced.publish("third".to_owned(), true));
        assert_eq!(scheduler.len(), 1);

        assert!(scheduler.run_next());

        assert_eq!(*consumed.lock(), vec!["third".to_owned()]);
        assert!(!coalesced.is_pending());
        assert_eq!(scheduler.len(), 0);
    }

    #[test]
    fn value_published_while_consuming_gets_one_trailing_callback() {
        let scheduler = Arc::new(QueueScheduler::default());
        let consumed = Arc::new(Mutex::new(Vec::<String>::new()));
        // Filled after construction so the consumer can publish back into the
        // coalescer, as the C# test does through a captured local.
        let handle: Arc<Mutex<Option<Arc<CoalescedLatestValue<String>>>>> =
            Arc::new(Mutex::new(None));

        let recorder = Arc::clone(&consumed);
        let publisher = Arc::clone(&handle);
        let coalesced = Arc::new(CoalescedLatestValue::new(
            enqueue(&scheduler),
            move |value: &String| {
                recorder.lock().push(value.clone());
                if value == "first" {
                    if let Some(coalesced) = publisher.lock().as_ref() {
                        coalesced.publish("latest".to_owned(), true);
                    }
                }
            },
        ));
        *handle.lock() = Some(Arc::clone(&coalesced));

        coalesced.publish("first".to_owned(), true);
        assert!(scheduler.run_next());

        assert_eq!(*consumed.lock(), vec!["first".to_owned()]);
        assert_eq!(scheduler.len(), 1, "one trailing drain, not a queue");

        assert!(scheduler.run_next());
        assert_eq!(
            *consumed.lock(),
            vec!["first".to_owned(), "latest".to_owned()]
        );
        assert!(!coalesced.is_pending());

        // Break the handle -> coalescer cycle so the test does not leak.
        *handle.lock() = None;
    }

    #[test]
    fn disallowed_schedule_still_retains_the_latest_value() {
        let scheduler = Arc::new(QueueScheduler::default());
        let consumed = Arc::new(Mutex::new(Vec::<String>::new()));

        let recorder = Arc::clone(&consumed);
        let coalesced = CoalescedLatestValue::new(enqueue(&scheduler), move |value: &String| {
            recorder.lock().push(value.clone());
        });

        assert!(!coalesced.publish("latest".to_owned(), false));
        assert_eq!(scheduler.len(), 0);
        assert!(coalesced.publish("trigger".to_owned(), true));
        assert!(scheduler.run_next());

        assert_eq!(*consumed.lock(), vec!["trigger".to_owned()]);
    }

    #[test]
    fn dispose_drops_the_retained_value_and_refuses_work() {
        let scheduler = Arc::new(QueueScheduler::default());
        let consumed = Arc::new(Mutex::new(Vec::<String>::new()));

        let recorder = Arc::clone(&consumed);
        let coalesced = CoalescedLatestValue::new(enqueue(&scheduler), move |value: &String| {
            recorder.lock().push(value.clone());
        });

        assert!(coalesced.publish("pending".to_owned(), true));
        coalesced.dispose();
        assert!(!coalesced.publish("ignored".to_owned(), true));

        assert!(scheduler.run_next());
        assert!(consumed.lock().is_empty());
        assert!(!coalesced.is_pending());
    }
}
