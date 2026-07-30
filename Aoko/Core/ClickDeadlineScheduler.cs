using System;

namespace Aoko.Core;

internal readonly record struct ClickSchedule(
    long DeadlineTimestamp,
    int DelayMilliseconds);

internal static class ClickDeadlineScheduler
{
    public static ClickSchedule ScheduleNext(
        long previousDeadlineTimestamp,
        long nowTimestamp,
        double intervalMilliseconds,
        long timestampFrequency)
    {
        if (timestampFrequency <= 0)
            throw new ArgumentOutOfRangeException(nameof(timestampFrequency));

        double boundedIntervalMs = Math.Max(1.0, intervalMilliseconds);
        long intervalTicks = Math.Max(
            1,
            (long)Math.Round(
                boundedIntervalMs * timestampFrequency / 1000.0,
                MidpointRounding.AwayFromZero));

        long deadline = previousDeadlineTimestamp > 0
            ? previousDeadlineTimestamp + intervalTicks
            : nowTimestamp + intervalTicks;

        // Do not emit catch-up bursts after a stall, menu pause, or delayed
        // scheduler wake. Resume one complete interval from the current time.
        if (deadline <= nowTimestamp)
            deadline = nowTimestamp + intervalTicks;

        long remainingTicks = Math.Max(1, deadline - nowTimestamp);
        int delayMilliseconds = Math.Max(
            1,
            (int)Math.Ceiling(remainingTicks * 1000.0 / timestampFrequency));
        return new ClickSchedule(deadline, delayMilliseconds);
    }
}
