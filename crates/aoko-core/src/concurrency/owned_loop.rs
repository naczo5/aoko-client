//! One cancellable background loop, owned by its subsystem.
//!
//! Port of `Aoko/Core/OwnedAsyncLoop.cs`, renamed `OwnedLoop` because the Rust
//! port runs the body on a dedicated OS thread rather than as an `async` task;
//! there is no executor in this crate and `MIGRATION_ARCHITECTURE.md` section 4
//! assigns the blocking loops to an "automation" thread.
//!
//! Rules preserved from the C# original:
//!
//! - `Start`/`Stop` are serialised by one lock (`OwnedAsyncLoop.cs:62-107`).
//! - `Start` while a prior generation is still draining does **not** run a body
//!   immediately; the draining generation's completion starts the newest body.
//!   That is why `Stop`-then-`Start` runs only the latest generation.
//! - A body that returns while the loop is still requested is an *unexpected
//!   completion*: it is counted, reported, and restarted after a backoff
//!   (`1 << min(restartCount, 10))` ms, capped at 1000 ms,
//!   `OwnedAsyncLoop.cs:139-141`).
//! - A stale completion never touches the current generation
//!   (`OwnedAsyncLoop.cs:181-182`).
//! - A throwing failure callback does not prevent cleanup
//!   (`OwnedAsyncLoop.cs:204-214`).
//!
//! Difference forced by the port: the C# body is `async` and the restart delay is
//! `Task.Delay(ms, token)`, which is cancellable. The Rust body blocks, so the
//! backoff is a cancellation-aware wait rather than a raw sleep: `stop` still
//! takes effect immediately instead of waiting out the backoff.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::cancel::CancelToken;

/// Failure reported by a loop body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct LoopError {
    message: String,
}

impl LoopError {
    /// Builds a failure from a message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The failure message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Result a loop body returns. `Ok(())` means "finished", which the loop treats as
/// unexpected while it is still requested; `Err` is reported to the failure hook.
pub type LoopResult = Result<(), LoopError>;

/// The loop body. Re-runnable: a restart invokes the same closure again, exactly
/// as the C# `Func<CancellationToken, Task>` is re-invoked.
pub type LoopBody = Arc<dyn Fn(&CancelToken) -> LoopResult + Send + Sync + 'static>;

/// Called for a body failure, off the owning thread.
pub type FailureHook = Arc<dyn Fn(&LoopError) + Send + Sync + 'static>;

/// Highest restart backoff in milliseconds (`OwnedAsyncLoop.cs:140`).
const MAX_RESTART_BACKOFF_MS: u64 = 1_000;
/// Backoff exponent cap (`OwnedAsyncLoop.cs:140`).
const MAX_RESTART_SHIFT: u32 = 10;

/// A background loop whose lifetime is tied to one subsystem.
pub struct OwnedLoop {
    inner: Arc<Inner>,
}

struct Inner {
    name: String,
    on_failure: Mutex<Option<FailureHook>>,
    state: Mutex<State>,
    /// Signalled when a joiner releases the worker handle, so the joiners queued
    /// behind it can finish their own `stop_and_join` instead of returning early.
    join_finished: parking_lot::Condvar,
    unexpected_completions: AtomicU32,
    restarts: AtomicU32,
    generation: AtomicU64,
}

struct State {
    requested: bool,
    disposed: bool,
    /// How many callers are between "cancel" and "the thread is gone" inside
    /// [`OwnedLoop::stop_and_join`].
    ///
    /// A count, not a flag. While it is non-zero no generation may be launched, by
    /// `start`, by an unexpected-completion restart, or by the relaunch tail of
    /// `stop_and_join`. Without that seal a `start` landing after `stop` cancelled
    /// generation 1 but before generation 1's completion runs makes that completion
    /// launch generation 2 with a *fresh, uncancelled* token, and the in-progress
    /// join waits on a generation nobody will ever cancel. A flag is not enough to
    /// express that: with two concurrent joiners whichever exits first would clear
    /// the flag while the other is still inside `join`, reopening the window.
    joiners: usize,
    /// Whether one of those callers currently holds the detached worker handle.
    ///
    /// Only one caller may own the [`std::thread::JoinHandle`] at a time, so the
    /// others wait on [`Inner::join_finished`] instead of observing `None` and
    /// returning as if there were nothing to join.
    worker_checked_out: bool,
    body: Option<LoopBody>,
    cancel: Option<CancelToken>,
    running_generation: Option<u64>,
    /// The current (or last) worker handle.
    ///
    /// It is *not* cleared when the body completes: it is the only way another
    /// thread can wait for this thread, and the thread cannot join itself.
    worker: Option<std::thread::JoinHandle<()>>,
}

impl OwnedLoop {
    /// A loop that is neither requested nor running.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let name = if name.trim().is_empty() {
            "owned loop".to_owned()
        } else {
            name
        };
        Self {
            inner: Arc::new(Inner {
                name,
                on_failure: Mutex::new(None),
                state: Mutex::new(State {
                    requested: false,
                    disposed: false,
                    joiners: 0,
                    worker_checked_out: false,
                    body: None,
                    cancel: None,
                    running_generation: None,
                    worker: None,
                }),
                join_finished: parking_lot::Condvar::new(),
                unexpected_completions: AtomicU32::new(0),
                restarts: AtomicU32::new(0),
                generation: AtomicU64::new(0),
            }),
        }
    }

    /// Installs the failure callback. Replaces any previous one.
    pub fn set_failure_hook(&self, hook: FailureHook) {
        *self.inner.on_failure.lock() = Some(hook);
    }

    /// Diagnostic name, also used as the worker thread name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Requests the loop to run `body`. Returns `false` once the loop is disposed.
    pub fn start(&self, body: LoopBody) -> bool {
        let mut state = self.inner.state.lock();
        if state.disposed {
            tracing::warn!(loop_name = %self.inner.name, "start on a disposed loop ignored");
            return false;
        }

        state.body = Some(body);
        state.requested = true;
        if state.running_generation.is_none() {
            Inner::launch(&self.inner, &mut state, false);
        }
        true
    }

    /// Cancels the current generation and returns immediately. The body observes
    /// cancellation through its [`CancelToken`]. [`Self::stop_and_join`] does not
    /// delegate here: it takes the same lock once and also seals the loop.
    pub fn stop(&self) {
        let mut state = self.inner.state.lock();
        state.requested = false;
        if let Some(cancel) = state.cancel.as_ref() {
            cancel.cancel();
        }
    }

    /// Cancels and waits for the worker thread to finish.
    ///
    /// The loop is *sealed* for as long as any caller is inside this method, as a
    /// count: `start` and an unexpected-completion restart both decline to launch,
    /// so a join can never end up waiting on a generation nobody cancels. Every
    /// caller waits for the worker thread; when several arrive at once, one owns
    /// the handle and the rest wait for it rather than reporting "nothing to join".
    /// The last caller out honours a `start` that raced the join, which is the
    /// "most recent request wins" rule `start`/`stop` already follow.
    ///
    /// Safe to call from any thread except the loop's own body, and safe to call
    /// concurrently with itself.
    pub fn stop_and_join(&self) {
        let _seal = JoinerGuard::enter(Arc::clone(&self.inner));

        loop {
            let handle = {
                let mut state = self.inner.state.lock();
                // Another joiner owns the handle. Its thread is the one being
                // waited for, so waiting for it here is the same guarantee, not a
                // weaker one.
                while state.worker_checked_out {
                    self.inner.join_finished.wait(&mut state);
                }
                match state.worker.take() {
                    Some(handle) => {
                        state.worker_checked_out = true;
                        handle
                    }
                    // Nothing left to wait for: every launched generation has been
                    // joined by this caller or by a joiner it waited for.
                    None => break,
                }
            };

            // Joined with no lock held: the worker's completion needs the same
            // lock, and a body may be waiting on whoever calls this.
            let joined = handle.join();

            {
                let mut state = self.inner.state.lock();
                state.worker_checked_out = false;
                self.inner.join_finished.notify_all();
            }

            if joined.is_err() {
                // The body panicked and `panic = "abort"` was off; the failure was
                // already recorded inside the worker.
                tracing::warn!(loop_name = %self.inner.name, "loop worker panicked");
            }
        }
    }

    /// Stops the loop for good. Idempotent.
    pub fn dispose(&self) {
        let mut state = self.inner.state.lock();
        state.disposed = true;
        state.requested = false;
        if let Some(cancel) = state.cancel.as_ref() {
            cancel.cancel();
        }
    }

    /// Whether the caller most recently requested that the loop run.
    #[must_use]
    pub fn is_requested(&self) -> bool {
        self.inner.state.lock().requested
    }

    /// Whether a generation is currently active.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.inner.state.lock().running_generation.is_some()
    }

    /// Whether a [`Self::stop_and_join`] is between its cancel and its join.
    ///
    /// While sealed, no generation is launched: `start` only records the request and
    /// the last joiner's tail honours it afresh. Diagnostics and tests use this to
    /// observe the sealed window instead of racing it.
    #[must_use]
    pub fn is_sealed(&self) -> bool {
        self.inner.state.lock().joiners > 0
    }

    /// How many callers are currently inside [`Self::stop_and_join`].
    ///
    /// The seal is this count, so a test can wait for the second stopper to arrive
    /// instead of assuming it did.
    #[must_use]
    pub fn joiners(&self) -> usize {
        self.inner.state.lock().joiners
    }

    /// Whether a joiner currently holds the worker handle and is inside `join`.
    ///
    /// Diagnostics and tests only: it makes the window in which a second stopper
    /// must *wait* rather than observe `None` observable.
    #[must_use]
    pub fn is_joining_worker(&self) -> bool {
        self.inner.state.lock().worker_checked_out
    }

    /// Unexpected exits observed while the loop was requested.
    #[must_use]
    pub fn unexpected_completion_count(&self) -> u32 {
        self.inner.unexpected_completions.load(Ordering::Acquire)
    }

    /// Automatic restarts after an unexpected exit.
    #[must_use]
    pub fn restart_count(&self) -> u32 {
        self.inner.restarts.load(Ordering::Acquire)
    }
}

impl Drop for OwnedLoop {
    fn drop(&mut self) {
        // Cancels without joining: a body may be waiting on the owner that is
        // being dropped right now. Callers that need quiescence call
        // `stop_and_join` first. This matches C# `Dispose`, which cancels only.
        self.dispose();
    }
}

impl std::fmt::Debug for OwnedLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedLoop")
            .field("name", &self.inner.name)
            .field("requested", &self.is_requested())
            .field("running", &self.is_running())
            .finish()
    }
}

/// Holds the seal for one [`OwnedLoop::stop_and_join`] caller.
///
/// Entry increments [`State::joiners`], cancels the current generation and clears
/// `requested` under one lock; drop decrements it and, on the last caller out,
/// relaunches only if a `start` raced the join. Decrementing on every path is why
/// this is a guard: an early return (a panicking `join`, a body that aborts) must
/// not leave the loop sealed forever.
struct JoinerGuard {
    inner: Arc<Inner>,
}

impl JoinerGuard {
    fn enter(inner: Arc<Inner>) -> Self {
        {
            let mut state = inner.state.lock();
            state.joiners += 1;
            state.requested = false;
            if let Some(cancel) = state.cancel.as_ref() {
                cancel.cancel();
            }
        }
        Self { inner }
    }
}

impl Drop for JoinerGuard {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock();
        state.joiners = state.joiners.saturating_sub(1);
        if state.joiners == 0 && state.requested && !state.disposed && state.body.is_some() {
            let inner = Arc::clone(&self.inner);
            Inner::launch(&inner, &mut state, false);
        }
    }
}

impl Inner {
    fn launch(self: &Arc<Self>, state: &mut State, restart: bool) {
        if !state.requested || state.disposed || state.joiners > 0 || state.body.is_none() {
            return;
        }

        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let cancel = CancelToken::new();
        state.cancel = Some(cancel.clone());
        state.running_generation = Some(generation);

        let backoff = if restart {
            // The restart counter is incremented before the relaunch, so the first
            // restart waits 2 ms; `OwnedAsyncLoop.cs:193` then `:139-141`.
            let shift = self.restarts.load(Ordering::Acquire).min(MAX_RESTART_SHIFT);
            Duration::from_millis(u64::from(1_u32 << shift).min(MAX_RESTART_BACKOFF_MS))
        } else {
            Duration::ZERO
        };

        let body = state.body.clone();
        let worker = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name(self.name.clone())
            .spawn(move || Inner::run(&worker, generation, cancel, body, backoff));

        match spawned {
            Ok(handle) => state.worker = Some(handle),
            Err(error) => {
                state.running_generation = None;
                state.cancel = None;
                // No retry loop: a thread that cannot be spawned will not spawn on
                // the next attempt either, and spinning on it would be worse than
                // reporting it.
                self.record_failure(&LoopError::new(format!(
                    "failed to spawn loop thread: {error}"
                )));
            }
        }
    }

    fn run(
        self: &Arc<Self>,
        generation: u64,
        cancel: CancelToken,
        body: Option<LoopBody>,
        backoff: Duration,
    ) {
        if !backoff.is_zero() && cancel.wait_for(backoff) {
            // Cancelled during the backoff: behave like the C# `Task.Delay(token)`
            // throwing, which routes straight to `complete(canceled: true)`.
            self.complete(generation, false);
            return;
        }

        let mut failure = None;
        if !cancel.is_cancelled() {
            if let Some(body) = body {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&cancel))) {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => failure = Some(error),
                    Err(_) => failure = Some(LoopError::new("loop body panicked")),
                }
            }
        }

        if let Some(error) = failure {
            self.record_failure(&error);
        }
        self.complete(generation, !cancel.is_cancelled());
    }

    fn complete(self: &Arc<Self>, generation: u64, unexpected: bool) {
        let mut state = self.state.lock();

        // A stale completion must not touch the current owner.
        if state.running_generation != Some(generation) {
            return;
        }

        state.running_generation = None;
        state.cancel = None;
        // The worker handle is deliberately left in place. This thread cannot join
        // itself, and the handle is the only way a [`OwnedLoop::stop_and_join`]
        // caller can wait for it; clearing it here made a joiner take `None` and
        // return without having joined anything. A restart overwrites it below,
        // which is safe because this thread has reached its last statement.

        // A completion while a join is in progress is relaunched by that joiner's
        // tail, once the seal count reaches zero and no one is waiting any more.
        if state.joiners == 0 && state.requested && !state.disposed && state.body.is_some() {
            if unexpected {
                self.unexpected_completions.fetch_add(1, Ordering::AcqRel);
                self.restarts.fetch_add(1, Ordering::AcqRel);
            }
            Self::launch(self, &mut state, unexpected);
        } else if unexpected {
            self.unexpected_completions.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn record_failure(&self, error: &LoopError) {
        tracing::warn!(loop_name = %self.name, error = %error, "owned loop failed");
        let hook = self.on_failure.lock().clone();
        if let Some(hook) = hook {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook(error))).is_err() {
                tracing::error!(loop_name = %self.name, "failure callback panicked");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// Failure guard for thread synchronisation. The assertions are event driven;
    /// the timeout only stops a broken loop from hanging the suite.
    const SIGNAL_TIMEOUT: Duration = Duration::from_secs(5);

    /// Waits for a condition another thread is responsible for, without sleeping.
    ///
    /// A bounded spin is an assertion, not a schedule: exhausting the budget means
    /// the other thread never reached the state the test needs, which is a real
    /// failure rather than a slow machine.
    fn spin_until(mut condition: impl FnMut() -> bool, what: &str) {
        let mut spins = 0_u32;
        while !condition() {
            std::hint::spin_loop();
            spins += 1;
            assert!(spins < 50_000_000, "{what}");
        }
    }

    #[test]
    fn stop_then_start_runs_only_the_latest_requested_generation() {
        let loop_ = OwnedLoop::new("test");
        let (tx, rx) = mpsc::channel();
        let runs = Arc::new(Mutex::new(Vec::<u32>::new()));

        let first_tx = tx.clone();
        let first_runs = Arc::clone(&runs);
        assert!(loop_.start(Arc::new(move |cancel: &CancelToken| {
            first_runs.lock().push(1);
            let _ = first_tx.send(1_u32);
            cancel.wait();
            Ok(())
        })));

        assert_eq!(rx.recv_timeout(SIGNAL_TIMEOUT), Ok(1));
        loop_.stop();

        let second_runs = Arc::clone(&runs);
        assert!(loop_.start(Arc::new(move |cancel: &CancelToken| {
            second_runs.lock().push(2);
            let _ = tx.send(2_u32);
            cancel.wait();
            Ok(())
        })));

        assert_eq!(rx.recv_timeout(SIGNAL_TIMEOUT), Ok(2));
        assert!(loop_.is_requested());
        assert!(loop_.is_running());
        assert_eq!(*runs.lock(), vec![1, 2]);

        loop_.stop_and_join();
        assert!(!loop_.is_requested());
        assert!(!loop_.is_running());
        assert_eq!(loop_.restart_count(), 0);
    }

    #[test]
    fn unexpected_completion_is_restarted_and_reported() {
        let loop_ = OwnedLoop::new("test");
        let (tx, rx) = mpsc::channel();
        let run_number = Arc::new(AtomicU32::new(0));

        assert!(loop_.start(Arc::new(move |cancel: &CancelToken| {
            if run_number.fetch_add(1, Ordering::AcqRel) == 0 {
                // Returning while requested is the unexpected completion.
                return Ok(());
            }
            let _ = tx.send(());
            cancel.wait();
            Ok(())
        })));

        rx.recv_timeout(SIGNAL_TIMEOUT)
            .expect("the loop restarted after an unexpected completion");
        assert_eq!(loop_.unexpected_completion_count(), 1);
        assert_eq!(loop_.restart_count(), 1);

        loop_.stop_and_join();
        assert!(!loop_.is_running());
    }

    #[test]
    fn failure_callback_does_not_prevent_cleanup() {
        let loop_ = OwnedLoop::new("test");
        let (failure_tx, failure_rx) = mpsc::channel();
        loop_.set_failure_hook(Arc::new(move |error: &LoopError| {
            let _ = failure_tx.send(error.to_string());
        }));

        let (tx, rx) = mpsc::channel();
        let run_number = Arc::new(AtomicU32::new(0));
        assert!(loop_.start(Arc::new(move |cancel: &CancelToken| {
            if run_number.fetch_add(1, Ordering::AcqRel) == 0 {
                return Err(LoopError::new("expected test failure"));
            }
            let _ = tx.send(());
            cancel.wait();
            Ok(())
        })));

        rx.recv_timeout(SIGNAL_TIMEOUT)
            .expect("the loop restarted after a failure");
        assert_eq!(
            failure_rx.recv_timeout(SIGNAL_TIMEOUT).as_deref(),
            Ok("expected test failure")
        );
        assert_eq!(loop_.restart_count(), 1);
        assert_eq!(loop_.unexpected_completion_count(), 1);

        loop_.stop_and_join();
        assert!(!loop_.is_requested());
        assert!(!loop_.is_running());
    }

    #[test]
    fn a_disposed_loop_refuses_to_start_again() {
        let loop_ = OwnedLoop::new("test");
        let (tx, rx) = mpsc::channel();

        loop_.dispose();
        assert!(!loop_.start(Arc::new(move |cancel: &CancelToken| {
            let _ = tx.send(());
            cancel.wait();
            Ok(())
        })));

        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        assert!(!loop_.is_requested());
        assert!(!loop_.is_running());
        assert_eq!(loop_.name(), "test");
    }

    /// Regression for the `stop_and_join` / `start` race: a `start` that lands
    /// inside the sealed window must not make generation 1's completion launch a
    /// fresh generation that the join then waits on forever.
    #[test]
    fn a_start_during_stop_and_join_never_strands_the_join() {
        let loop_ = Arc::new(OwnedLoop::new("race"));
        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let gate = Arc::new((Mutex::new(false), parking_lot::Condvar::new()));

        let body: LoopBody = {
            let gate = Arc::clone(&gate);
            let entered_tx = entered_tx.clone();
            let first = Arc::new(std::sync::atomic::AtomicBool::new(true));
            let first_generation = Arc::clone(&first);
            Arc::new(move |cancel: &CancelToken| {
                if first_generation.swap(false, Ordering::AcqRel) {
                    let _ = entered_tx.send(());
                    // Park *after* cancellation but *before* returning, which is
                    // exactly the window in which `complete` used to relaunch.
                    let (lock, cvar) = &*gate;
                    let mut released = lock.lock();
                    while !*released {
                        cvar.wait(&mut released);
                    }
                    return Ok(());
                }
                cancel.wait();
                Ok(())
            })
        };

        assert!(loop_.start(Arc::clone(&body)));
        entered_rx
            .recv_timeout(SIGNAL_TIMEOUT)
            .expect("generation 1 entered its body");

        let stopper = {
            let loop_ = Arc::clone(&loop_);
            std::thread::spawn(move || loop_.stop_and_join())
        };

        // Wait for the seal rather than guessing: the request below must land
        // inside it.
        let mut spins = 0_u32;
        while !loop_.is_sealed() {
            std::hint::spin_loop();
            spins += 1;
            assert!(spins < 10_000_000, "stop_and_join never sealed the loop");
        }
        assert!(
            loop_.start(Arc::clone(&body)),
            "a start during a join is accepted"
        );

        {
            let (lock, cvar) = &*gate;
            *lock.lock() = true;
            cvar.notify_all();
        }

        // Before the fix this join waited on an uncancelled generation 2 forever.
        // A watchdog keeps a regression a failure instead of a hung suite.
        let (done_tx, done_rx) = mpsc::channel();
        let watchdog = std::thread::spawn(move || {
            stopper.join().expect("stopper thread");
            let _ = done_tx.send(());
        });
        done_rx
            .recv_timeout(SIGNAL_TIMEOUT)
            .expect("stop_and_join must not wait on a generation nobody cancels");
        watchdog.join().expect("watchdog thread");

        // The racing start was honoured after the join, so the loop is live again and
        // a second stop can reach it.
        let loop_2 = Arc::clone(&loop_);
        let (done_tx, done_rx) = mpsc::channel();
        let watchdog = std::thread::spawn(move || {
            loop_2.stop_and_join();
            let _ = done_tx.send(());
        });
        done_rx
            .recv_timeout(SIGNAL_TIMEOUT)
            .expect("the relaunched generation must be stoppable");
        watchdog.join().expect("watchdog thread");

        assert!(!loop_.is_running());
        assert!(!loop_.is_sealed());
    }

    /// Regression for the *second* `stop_and_join` race.
    ///
    /// The seal used to be a flag, so a stopper that found the worker handle
    /// already taken returned immediately and cleared the flag while the first
    /// stopper was still inside `join`. A `start` landing in that window then
    /// relaunched a generation with a fresh, uncancelled token which the first
    /// stopper joined forever. A one-stopper test cannot see that, which is why it
    /// reported the fix as complete.
    #[test]
    fn two_concurrent_stoppers_and_a_racing_start_never_strand_a_join() {
        let loop_ = Arc::new(OwnedLoop::new("race-two"));
        let (entered_tx, entered_rx) = mpsc::channel::<()>();
        let gate = Arc::new((Mutex::new(false), parking_lot::Condvar::new()));

        let body: LoopBody = {
            let gate = Arc::clone(&gate);
            let entered_tx = entered_tx.clone();
            let first = Arc::new(std::sync::atomic::AtomicBool::new(true));
            let first_generation = Arc::clone(&first);
            Arc::new(move |cancel: &CancelToken| {
                if first_generation.swap(false, Ordering::AcqRel) {
                    let _ = entered_tx.send(());
                    // Parks after cancellation but before returning, so the joiners
                    // are provably inside `join` when the race is staged.
                    let (lock, cvar) = &*gate;
                    let mut released = lock.lock();
                    while !*released {
                        cvar.wait(&mut released);
                    }
                    return Ok(());
                }
                cancel.wait();
                Ok(())
            })
        };

        assert!(loop_.start(Arc::clone(&body)));
        entered_rx
            .recv_timeout(SIGNAL_TIMEOUT)
            .expect("generation 1 entered its body");

        let spawn_stopper = |loop_: Arc<OwnedLoop>| {
            let (tx, rx) = mpsc::channel::<()>();
            let handle = std::thread::spawn(move || {
                loop_.stop_and_join();
                let _ = tx.send(());
            });
            (handle, rx)
        };

        let (first, first_done) = spawn_stopper(Arc::clone(&loop_));
        // The exact window the old code lost: the first stopper owns the handle and
        // is inside `join`, so a second one used to see `None`.
        spin_until(
            || loop_.is_joining_worker(),
            "the first stopper never began joining the worker",
        );

        let (second, second_done) = spawn_stopper(Arc::clone(&loop_));
        spin_until(
            || loop_.joiners() == 2,
            "the second stopper never joined the seal",
        );

        // The racing start: accepted, recorded, and unable to launch through the
        // seal.
        assert!(loop_.start(Arc::clone(&body)));
        assert!(loop_.is_requested());
        assert!(loop_.is_sealed(), "two joiners must keep the loop sealed");

        {
            let (lock, cvar) = &*gate;
            *lock.lock() = true;
            cvar.notify_all();
        }

        // Both must return. Before the fix the first one waited on a generation
        // nobody cancels and this times out.
        first_done
            .recv_timeout(SIGNAL_TIMEOUT)
            .expect("the first stop_and_join never returned");
        second_done
            .recv_timeout(SIGNAL_TIMEOUT)
            .expect("the second stop_and_join never returned");
        first.join().expect("first stopper thread");
        second.join().expect("second stopper thread");

        assert_eq!(loop_.joiners(), 0);
        assert!(!loop_.is_sealed());
        // Only the racing start relaunched anything, and it is not an
        // unexpected-completion restart.
        assert_eq!(loop_.restart_count(), 0);
        assert_eq!(loop_.unexpected_completion_count(), 0);
        assert!(
            loop_.is_running(),
            "the last joiner out honours the racing start"
        );

        // And that generation is reachable: nothing uncancelled survives the pair.
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let final_loop = Arc::clone(&loop_);
        let stopper = std::thread::spawn(move || {
            final_loop.stop_and_join();
            let _ = done_tx.send(());
        });
        done_rx
            .recv_timeout(SIGNAL_TIMEOUT)
            .expect("a generation nobody cancels survived the two-stopper race");
        stopper.join().expect("final stopper thread");

        assert!(!loop_.is_running());
        assert_eq!(loop_.joiners(), 0);
    }

    #[test]
    fn an_empty_name_falls_back_to_the_generic_label() {
        assert_eq!(OwnedLoop::new("  ").name(), "owned loop");
    }
}
