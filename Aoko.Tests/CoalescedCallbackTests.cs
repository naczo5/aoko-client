using Aoko.Core;

namespace Aoko.Tests;

public sealed class CoalescedCallbackTests
{
    [Theory]
    [InlineData(0, 100, 25, 0)]
    [InlineData(100, 125, 25, 0)]
    [InlineData(100, 110, 25, 15)]
    [InlineData(100, 99, 25, 25)]
    [InlineData(100, 110, 0, 0)]
    public void CalculateDueTime_EnforcesMinimumInterval(
        long last,
        long now,
        int interval,
        int expected)
    {
        Assert.Equal(
            expected,
            CoalescedCallback.CalculateDueTime(last, now, interval));
    }

    [Fact]
    public async Task Signal_CollapsesBurstAndDeliversTrailingCallback()
    {
        int callbackCount = 0;
        using var callback = new CoalescedCallback(
            () => Interlocked.Increment(ref callbackCount),
            minimumIntervalMs: 30);

        callback.Signal();
        Assert.True(
            SpinWait.SpinUntil(() => Volatile.Read(ref callbackCount) == 1, 1000),
            "Initial callback did not arrive.");

        for (int i = 0; i < 50; i++)
            callback.Signal();

        await Task.Delay(100);

        Assert.Equal(2, Volatile.Read(ref callbackCount));
    }
}
