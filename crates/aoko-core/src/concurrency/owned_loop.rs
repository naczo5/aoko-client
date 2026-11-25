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
    unexpected_completions: AtomicU32,
    restarts: AtomicU32,
    generation: AtomicU64,
}

struct State {
    requested: bool,
    disposed: bool,
    body: Option<LoopBody>,
    cancel: Option<CancelToken>,
    running_generation: Option<u64>,
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
                    body: None,
                    cancel: None,
                    running_generation: None,
                    worker: None,
                }),
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
    /// cancellation through its [`CancelToken`].
    pub fn stop(&self) {
        let mut state = self.inner.state.lock();
        state.requested = false;
        if let Some(cancel) = state.cancel.as_ref() {
            cancel.cancel();
        }
    }

    /// Cancels and waits for the worker thread to finish.
    ///
    /// Safe to call from any thread except the loop's own body.
    pub fn stop_and_join(&self) {
        self.stop();
        loop {
            let handle = self.inner.state.lock().worker.take();
            let Some(handle) = handle else {
                return;
            };
            if handle.join().is_err() {
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

impl Inner {
    fn launch(self: &Arc<Self>, state: &mut State, restart: bool) {
        if !state.requested || state.disposed || state.body.is_none() {
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
        // Detaches this thread; its own handle cannot be joined from inside it.
        state.worker = None;

        if state.requested && !state.disposed && state.body.is_some() {
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

    #[test]
    fn an_empty_name_falls_back_to_the_generic_label() {
        assert_eq!(OwnedLoop::new("  ").name(), "owned loop");
    }
}
