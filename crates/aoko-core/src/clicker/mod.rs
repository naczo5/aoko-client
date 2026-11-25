//! Click scheduling: interval maths, randomization mode, human-like timing.
//!
//! Ports of `Aoko/Core/ClickDeadlineScheduler.cs`,
//! `Aoko/Core/ClickRandomizationMode.cs` and
//! `Aoko/Core/AdvancedClickTimingState.cs`, plus the `RandomizationMode` /
//! `JitterEnabled` property pair that lives on `Clicker` in C#.
//!
//! `Clicker.cs`'s loop itself lives in [`crate::clicker`]'s siblings: the guard
//! chain is [`tick`], the injected-press latch is [`latch`] and the screen /
//! hotbar predicates are [`guards`]. This module keeps the timing maths.

mod deadline_scheduler;
pub mod guards;
pub mod latch;
pub mod randomization;
pub mod tick;
mod timing_state;
pub mod toggles;

pub use deadline_scheduler::{ClickDeadlineScheduler, ClickSchedule};
pub use randomization::ClickRandomizationMode;
pub use timing_state::{delay_to_interval, AdvancedClickTimingState};
pub use toggles::{panic_reset, toggle, ModuleToggles, ToggleOutcome};
