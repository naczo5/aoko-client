//! Cross-thread primitives: coalescing, deferred delivery, cancellation.
//!
//! Ports of `Aoko/Core/CoalescedCallback.cs`,
//! `Aoko/Core/CoalescedLatestValue.cs` and `Aoko/Core/OwnedAsyncLoop.cs`.
//! Everything here is driven through injected seams ([`Scheduler`],
//! [`CancelToken`]) so none of it depends on a wall clock or an executor.

mod cancel;
mod coalesced_callback;
mod coalesced_latest_value;
mod owned_loop;
mod scheduler;

pub use cancel::CancelToken;
pub use coalesced_callback::CoalescedCallback;
pub use coalesced_latest_value::{CoalescedLatestValue, ConsumeFn, ScheduleFn};
pub use owned_loop::{FailureHook, LoopBody, LoopError, LoopResult, OwnedLoop};
pub use scheduler::{ScheduledTask, Scheduler, TimerScheduler};
