using Aoko.Core;

namespace Aoko.Tests;

public sealed class ClickDeadlineSchedulerTests
{
    private const long Frequency = 1000;

    [Fact]
    public void FirstDeadline_IsOneIntervalFromNow()
    {
        ClickSchedule schedule = ClickDeadlineScheduler.ScheduleNext(
            previousDeadlineTimestamp: 0,
            nowTimestamp: 1000,
            intervalMilliseconds: 80,
            timestampFrequency: Frequency);

        Assert.Equal(1080, schedule.DeadlineTimestamp);
        Assert.Equal(80, schedule.DelayMilliseconds);
    }

    [Fact]
    public void MinorOvershoot_IsCompensatedByTheNextDeadline()
    {
        ClickSchedule schedule = ClickDeadlineScheduler.ScheduleNext(
            previousDeadlineTimestamp: 1080,
            nowTimestamp: 1085,
            intervalMilliseconds: 80,
            timestampFrequency: Frequency);

        Assert.Equal(1160, schedule.DeadlineTimestamp);
        Assert.Equal(75, schedule.DelayMilliseconds);
    }

    [Fact]
    public void MissedInterval_ResetsWithoutCatchUpBurst()
    {
        ClickSchedule schedule = ClickDeadlineScheduler.ScheduleNext(
            previousDeadlineTimestamp: 1080,
            nowTimestamp: 1200,
            intervalMilliseconds: 80,
            timestampFrequency: Frequency);

        Assert.Equal(1280, schedule.DeadlineTimestamp);
        Assert.Equal(80, schedule.DelayMilliseconds);
    }

    [Fact]
    public void FractionalInterval_RoundsDelayUp()
    {
        ClickSchedule schedule = ClickDeadlineScheduler.ScheduleNext(
            previousDeadlineTimestamp: 0,
            nowTimestamp: 1000,
            intervalMilliseconds: 83.25,
            timestampFrequency: 10_000);

        Assert.Equal(1833, schedule.DeadlineTimestamp);
        Assert.Equal(84, schedule.DelayMilliseconds);
    }
}
