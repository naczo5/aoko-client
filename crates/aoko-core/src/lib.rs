//! Headless automation core. Time, sleep and randomness are injected so every
//! controller is deterministically testable.
//!
//! Layout follows `MIGRATION_ARCHITECTURE.md` section 2. What exists after wave 1:
//!
//! | Module | Ported from |
//! | --- | --- |
//! | [`clock`] | the determinism seams from section 3 (no C# counterpart) |
//! | [`concurrency`] | `CoalescedCallback.cs`, `CoalescedLatestValue.cs`, `OwnedAsyncLoop.cs` |
//! | [`clicker`] | `ClickDeadlineScheduler.cs`, `ClickRandomizationMode.cs`, `AdvancedClickTimingState.cs` |
//! | [`aim_assist`] | `AimAssistMotionController.cs` |
//! | [`services::update`] | `UpdateService.cs` |
//!
//! Deliberately absent until wave 2, because they need `aoko-protocol::GameState`:
//! `Clicker.cs`, `ChestStealerController.cs`, `ChestStealerCoordinateMapper.cs`,
//! `RefillController.cs`, the AutoRod / AutoTool state machines,
//! `DiscordRichPresenceService.cs` (state half) and `ModuleSearchIndex.cs`.
//!
//! Two rules hold across the crate:
//!
//! - Timing code takes an absolute `Instant` deadline and a [`clock::Sleeper`],
//!   never `Instant::now()` inline and never a cumulative `sleep(duration)`.
//! - Nothing in a library path panics on a runtime value: no `unwrap`, no
//!   `expect`, no slice indexing by a caller-supplied value, and no
//!   `Duration::from_secs_f64` on an unvalidated float.

pub mod aim_assist;
pub mod clicker;
pub mod clock;
pub mod concurrency;
pub mod services;

/// Deterministic test doubles (virtual clock, manual scheduler).
///
/// Compiled under `cfg(test)` and under the `testing` feature so dependent crates
/// can wire the same doubles.
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use clock::{Clock, EntropyRng, Rng, Sleeper, SplitMix64, SystemClock, ThreadSleeper, Timing};
