//! Cancellation handle shared with an owned loop body.
//!
//! The C# original threads a `CancellationToken` through the body
//! (`Aoko/Core/OwnedAsyncLoop.cs:138`). A bare `AtomicBool` would force every
//! body to poll; this handle also exposes a blocking wait so a body can park
//! until it is cancelled instead of spinning.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

#[derive(Debug, Default)]
struct Gate {
    cancelled: AtomicBool,
    lock: Mutex<()>,
    cvar: Condvar,
}

/// Cloneable cancellation signal.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    gate: Arc<Gate>,
}

impl CancelToken {
    /// A fresh, un-cancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether [`Self::cancel`] has been called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.gate.cancelled.load(Ordering::Acquire)
    }

    /// Cancels and wakes every waiter. Idempotent.
    pub fn cancel(&self) {
        // The flag is set under the lock so a waiter that has not yet reached
        // `wait` cannot miss the notification.
        let _guard = self.gate.lock.lock();
        self.gate.cancelled.store(true, Ordering::Release);
        self.gate.cvar.notify_all();
    }

    /// Blocks until cancelled. Returns immediately when already cancelled.
    pub fn wait(&self) {
        if self.is_cancelled() {
            return;
        }
        let mut guard = self.gate.lock.lock();
        while !self.gate.cancelled.load(Ordering::Acquire) {
            self.gate.cvar.wait(&mut guard);
        }
    }

    /// Blocks until cancelled or `timeout` elapses. Returns whether it was
    /// cancelled.
    ///
    /// The timeout is a safety guard, not a schedule: loops that must measure
    /// time take it from [`crate::clock::Clock`]. A body that needs "wait up to
    /// one tick" uses this. (C# has the same split: `Task.Delay` for durations,
    /// `CancellationToken.WaitHandle` for the guard.)
    pub fn wait_for(&self, timeout: Duration) -> bool {
        if self.is_cancelled() {
            return true;
        }

        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);
        let mut guard = self.gate.lock.lock();
        loop {
            if self.gate.cancelled.load(Ordering::Acquire) {
                return true;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            if self.gate.cvar.wait_for(&mut guard, remaining).timed_out() && !self.is_cancelled() {
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn wait_wakes_the_waiter_on_cancel() {
        let token = CancelToken::new();
        let (tx, rx) = mpsc::channel();
        let waiter = token.clone();
        let handle = std::thread::spawn(move || {
            waiter.wait();
            // Reporting after the wait is what proves the notification landed.
            let _ = tx.send(waiter.is_cancelled());
        });

        assert!(!token.is_cancelled());
        token.cancel();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(true));
        handle.join().expect("waiter thread");
    }

    #[test]
    fn wait_for_reports_a_timeout_without_cancelling() {
        let token = CancelToken::new();
        assert!(!token.wait_for(Duration::from_millis(1)));
        assert!(!token.is_cancelled());

        token.cancel();
        assert!(token.wait_for(Duration::from_millis(1)));
        // Cancel is idempotent.
        token.cancel();
        assert!(token.is_cancelled());
    }
}
