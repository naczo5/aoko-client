using System.Collections.Generic;
using Aoko.Core;

namespace Aoko.Tests;

public sealed class CoalescedLatestValueTests
{
    [Fact]
    public void PendingPublishesAreCollapsedToTheLatestValue()
    {
        var scheduled = new Queue<Action>();
        var consumed = new List<string>();
        var coalesced = new CoalescedLatestValue<string>(
            action => scheduled.Enqueue(action),
            consumed.Add);

        Assert.True(coalesced.Publish("first", allowSchedule: true));
        Assert.False(coalesced.Publish("second", allowSchedule: true));
        Assert.False(coalesced.Publish("third", allowSchedule: true));
        Assert.Single(scheduled);

        scheduled.Dequeue()();

        Assert.Equal(["third"], consumed);
        Assert.False(coalesced.IsPending);
        Assert.Empty(scheduled);
    }

    [Fact]
    public void ValuePublishedWhileConsumingGetsOneTrailingCallback()
    {
        var scheduled = new Queue<Action>();
        var consumed = new List<string>();
        CoalescedLatestValue<string>? coalesced = null;
        coalesced = new CoalescedLatestValue<string>(
            action => scheduled.Enqueue(action),
            value =>
            {
                consumed.Add(value);
                if (value == "first")
                    coalesced!.Publish("latest", allowSchedule: true);
            });

        coalesced.Publish("first", allowSchedule: true);
        scheduled.Dequeue()();

        Assert.Equal(["first"], consumed);
        Assert.Single(scheduled);
        scheduled.Dequeue()();
        Assert.Equal(["first", "latest"], consumed);
        Assert.False(coalesced.IsPending);
    }

    [Fact]
    public void DisallowedScheduleStillRetainsLatestValue()
    {
        var scheduled = new Queue<Action>();
        string? consumed = null;
        var coalesced = new CoalescedLatestValue<string>(
            action => scheduled.Enqueue(action),
            value => consumed = value);

        Assert.False(coalesced.Publish("latest", allowSchedule: false));
        Assert.Empty(scheduled);
        Assert.True(coalesced.Publish("trigger", allowSchedule: true));
        scheduled.Dequeue()();

        Assert.Equal("trigger", consumed);
    }
}
