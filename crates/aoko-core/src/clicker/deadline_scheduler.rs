//! Absolute click deadlines.
//!
//! Port of `Aoko/Core/ClickDeadlineScheduler.cs`. The rules being preserved:
//!
//! - The next deadline is derived from the **previous deadline**, not from "now"
//!   (`ClickDeadlineScheduler.cs:27-29`), so the error of one `sleep` never adds
//!   to the next one. Only a sleep that overshoots the deadline moves the phase,
//!   and it moves it by the overshoot alone.
//! - The sleep is rounded **up** to whole milliseconds
//!   (`ClickDeadlineScheduler.cs:36-39`), because the underlying sleep primitive is
//!   millisecond granular (`Task.Delay` in C#, `Sleeper` here). Rounding up means a
//!   click can land late but never early.
//! - A deadline that has already passed is re-based one full interval from now
//!   instead of being emitted immediately (`ClickDeadlineScheduler.cs:31-34`).
//!   A stall, a menu pause or a delayed scheduler wake must not produce a
//!   catch-up burst of clicks.
//!
//! Difference forced by the port: the C# code works in raw `Stopwatch` ticks and
//! rounds `intervalMilliseconds * Frequency / 1000` to whole ticks
//! (`ClickDeadlineScheduler.cs:21-25`). A Rust [`Duration`] is already an integral
//! number of nanoseconds, so that rounding step has nothing left to do; the
//! deadline keeps full nanosecond precision and only the returned sleep is
//! quantised to milliseconds.
//!
//! The four cases in `ClickDeadlineSchedulerTests` map one-to-one onto the tests
//! at the bottom of this file.

use std::time::{Duration, Instant};

/// Shortest interval the scheduler will honour, matching the C# clamp
/// `Math.Max(1.0, intervalMilliseconds)` (`ClickDeadlineScheduler.cs:20`).
const MIN_INTERVAL: Duration = Duration::from_millis(1);

/// One scheduled click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickSchedule {
    /// Absolute deadline for the next click. Absolute, so it survives a slow or
    /// interrupted sleep unchanged.
    pub deadline: Instant,
    /// How long to sleep before the click. Whole milliseconds, rounded up,
    /// never zero.
    pub delay: Duration,
}

/// Stateless interval maths, named after the C# static class it ports.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClickDeadlineScheduler;

impl ClickDeadlineScheduler {
    /// Computes the next click deadline and the sleep that leads to it.
    ///
    /// `previous_deadline` is `None` for the first click of a run (the C# code
    /// uses the `0` tick sentinel for the same thing).
    #[must_use]
    pub fn schedule_next(
        previous_deadline: Option<Instant>,
        now: Instant,
        interval: Duration,
    ) -> ClickSchedule {
        let interval = interval.max(MIN_INTERVAL);

        let candidate = match previous_deadline {
            Some(previous) => previous.checked_add(interval).unwrap_or(now),
            None => now,
        };

        // No catch-up burst: resume one complete interval from now.
        let deadline = if candidate <= now {
            now.checked_add(interval).unwrap_or(now)
        } else {
            candidate
        };

        let remaining_nanos = u64::try_from(
            deadline
                .checked_duration_since(now)
                .map_or(1, |d| d.as_nanos()),
        )
        .unwrap_or(u64::MAX)
        .max(1);

        // `(int)Math.Ceiling(remainingTicks * 1000.0 / frequency)`
        // (`ClickDeadlineScheduler.cs:36-39`), floored at one millisecond.
        // Saturating instead of wrapping: an hour-long interval must not turn into
        // a three-second sleep.
        let whole_ms = remaining_nanos / 1_000_000;
        let has_remainder = remaining_nanos % 1_000_000 != 0;
        let delay_ms = whole_ms
            .saturating_add(u64::from(has_remainder))
            .clamp(1, u64::from(u32::MAX) / 2);

        ClickSchedule {
            deadline,
            delay: Duration::from_millis(delay_ms),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: Duration = Duration::from_millis(80);

    /// `ClickDeadlineSchedulerTests.FirstDeadline_IsOneIntervalFromNow`.
    #[test]
    fn first_deadline_is_one_interval_from_now() {
        let now = Instant::now();
        let schedule = ClickDeadlineScheduler::schedule_next(None, now, INTERVAL);

        assert_eq!(schedule.deadline, now + INTERVAL);
        assert_eq!(schedule.delay, INTERVAL);
    }

    /// `ClickDeadlineSchedulerTests.MinorOvershoot_IsCompensatedByTheNextDeadline`:
    /// 5 ms of overshoot shortens the next sleep to 75 ms rather than stretching
    /// the phase.
    #[test]
    fn minor_overshoot_is_compensated_by_the_next_deadline() {
        let base = Instant::now();
        let schedule = ClickDeadlineScheduler::schedule_next(
            Some(base + Duration::from_millis(1080)),
            base + Duration::from_millis(1085),
            Duration::from_millis(80),
        );

        assert_eq!(schedule.deadline, base + Duration::from_millis(1160));
        assert_eq!(schedule.delay, Duration::from_millis(75));
    }

    /// `ClickDeadlineSchedulerTests.MissedInterval_ResetsWithoutCatchUpBurst`:
    /// 120 ms late must not emit two clicks to catch up.
    #[test]
    fn missed_interval_resets_without_a_catch_up_burst() {
        let base = Instant::now();
        let schedule = ClickDeadlineScheduler::schedule_next(
            Some(base + Duration::from_millis(1080)),
            base + Duration::from_millis(1200),
            Duration::from_millis(80),
        );

        assert_eq!(schedule.deadline, base + Duration::from_millis(1280));
        assert_eq!(schedule.delay, Duration::from_millis(80));
    }

    /// `ClickDeadlineSchedulerTests.FractionalInterval_RoundsDelayUp`: the C# test
    /// needs `Frequency = 10_000` to make 83.25 ms fractional in ticks; nanos
    /// represent it exactly, so the property under test is the *sleep* rounding
    /// while the deadline keeps its precision.
    #[test]
    fn fractional_interval_rounds_the_delay_up() {
        let now = Instant::now();
        let schedule =
            ClickDeadlineScheduler::schedule_next(None, now, Duration::from_nanos(83_250_000));

        assert_eq!(schedule.deadline, now + Duration::from_nanos(83_250_000));
        assert_eq!(schedule.delay, Duration::from_millis(84));
    }

    #[test]
    fn a_sub_millisecond_interval_is_clamped_to_one_millisecond() {
        let now = Instant::now();
        let schedule = ClickDeadlineScheduler::schedule_next(None, now, Duration::from_micros(200));

        assert_eq!(schedule.deadline, now + MIN_INTERVAL);
        assert_eq!(schedule.delay, MIN_INTERVAL);
    }

    #[test]
    fn a_zero_interval_is_clamped_instead_of_spinning() {
        let now = Instant::now();
        let schedule = ClickDeadlineScheduler::schedule_next(None, now, Duration::ZERO);

        assert_eq!(schedule.delay, MIN_INTERVAL);
        assert!(schedule.deadline > now);
    }

    /// The drift property: 500 clicks whose sleeps are all exactly honoured keep
    /// landing on `base + n * interval`, with no accumulating error.
    #[test]
    fn absolute_deadlines_do_not_accumulate_sleep_drift() {
        let base = Instant::now();
        let mut previous = None;
        let mut now = base;

        for click in 1..=500_u32 {
            let schedule = ClickDeadlineScheduler::schedule_next(previous, now, INTERVAL);
            assert_eq!(schedule.deadline, base + INTERVAL * click);
            assert_eq!(schedule.delay, INTERVAL);
            previous = Some(schedule.deadline);
            now = schedule.deadline;
        }

        assert_eq!(previous, Some(base + INTERVAL * 500));
    }

    /// The same 500 clicks with 400 us of wake-up overshoot on every sleep: the
    /// phase offset stays at one overshoot instead of compounding, and every
    /// deadline stays on the interval grid.
    #[test]
    fn wake_up_overshoot_stays_a_constant_offset() {
        let base = Instant::now();
        let overshoot = Duration::from_micros(400);
        let mut previous = None;
        let mut now = base;

        for click in 1..=500_u32 {
            let schedule = ClickDeadlineScheduler::schedule_next(previous, now, INTERVAL);
            assert_eq!(schedule.deadline, base + INTERVAL * click);
            // 79.6 ms remaining rounds up to a whole 80 ms sleep.
            assert_eq!(schedule.delay, INTERVAL);
            previous = Some(schedule.deadline);
            now = schedule.deadline + overshoot;
        }
    }

    /// A sleeper that is late by more than one interval re-syncs instead of
    /// firing a burst, so the click rate can never exceed the configured one.
    #[test]
    fn a_late_sleeper_never_exceeds_one_click_per_interval() {
        let base = Instant::now();
        let mut previous = None;
        let mut now = base;
        let horizon = base + Duration::from_secs(8);
        let mut clicks = 0_u32;

        while now < horizon {
            let schedule = ClickDeadlineScheduler::schedule_next(previous, now, INTERVAL);
            assert!(schedule.deadline > now);
            assert!(schedule.delay >= MIN_INTERVAL);
            assert_eq!(schedule.delay.as_micros() % 1_000, 0);
            previous = Some(schedule.deadline);
            // A pathological sleeper: 3.5 intervals late every time.
            now = schedule.deadline + INTERVAL * 3 + Duration::from_millis(40);
            clicks += 1;
        }

        // 8 s at 80 ms is 100 clicks; a burst would push this far higher.
        assert!(
            clicks <= 100,
            "{clicks} clicks in 8 s exceeds the configured rate"
        );
    }
}
