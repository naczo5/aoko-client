//! Click scheduling: interval maths, randomization mode, human-like timing.
//!
//! Ports of `Aoko/Core/ClickDeadlineScheduler.cs`,
//! `Aoko/Core/ClickRandomizationMode.cs` and
//! `Aoko/Core/AdvancedClickTimingState.cs`, plus the `RandomizationMode` /
//! `JitterEnabled` property pair that lives on `Clicker` in C#.
//!
//! `Clicker.cs` itself is **not** ported here: it needs `GameState` and belongs to
//! wave 2. The pieces below are the ones it calls.

mod deadline_scheduler;
mod randomization;
mod timing_state;

pub use deadline_scheduler::{ClickDeadlineScheduler, ClickSchedule};
pub use randomization::ClickRandomizationMode;
pub use timing_state::{delay_to_interval, AdvancedClickTimingState};
