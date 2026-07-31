using System.Collections.Concurrent;

namespace Aoko.Tests;

public class OwnedAsyncLoopTests
{
    [Fact]
    public async Task StopThenStart_RunsOnlyTheLatestRequestedGeneration()
    {
        var loop = new Aoko.Core.OwnedAsyncLoop("test");
        var started = new ConcurrentQueue<int>();
        var secondStarted = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        int runNumber = 0;

        loop.Start(async token =>
        {
            int current = Interlocked.Increment(ref runNumber);
            started.Enqueue(current);
            if (current == 2)
                secondStarted.SetResult();

            await Task.Delay(Timeout.InfiniteTimeSpan, token);
        });

        await WaitAsync(() => Volatile.Read(ref runNumber) == 1);
        loop.Stop();
        loop.Start(async token =>
        {
            int current = Interlocked.Increment(ref runNumber);
            started.Enqueue(current);
            secondStarted.SetResult();
            await Task.Delay(Timeout.InfiniteTimeSpan, token);
        });

        await secondStarted.Task.WaitAsync(TimeSpan.FromSeconds(2));
        Assert.True(loop.IsRequested);
        Assert.True(loop.IsRunning);
        Assert.Equal([1, 2], started.ToArray());

        await loop.StopAsync();
        Assert.False(loop.IsRequested);
        Assert.False(loop.IsRunning);
        loop.Dispose();
    }

    [Fact]
    public async Task UnexpectedCompletion_IsRestartedAndReported()
    {
        var loop = new Aoko.Core.OwnedAsyncLoop("test", _ => { });
        var restarted = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        int runNumber = 0;

        loop.Start(async token =>
        {
            int current = Interlocked.Increment(ref runNumber);
            if (current == 1)
                return;

            restarted.SetResult();
            await Task.Delay(Timeout.InfiniteTimeSpan, token);
        });

        await restarted.Task.WaitAsync(TimeSpan.FromSeconds(2));
        Assert.Equal(1, loop.UnexpectedCompletionCount);
        Assert.Equal(1, loop.RestartCount);

        await loop.StopAsync();
        loop.Dispose();
    }

    [Fact]
    public async Task FailureCallback_DoesNotPreventCleanup()
    {
        Exception? failure = null;
        var loop = new Aoko.Core.OwnedAsyncLoop("test", ex => failure = ex);
        var restarted = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        int runNumber = 0;

        loop.Start(async token =>
        {
            if (Interlocked.Increment(ref runNumber) == 1)
                throw new InvalidOperationException("expected test failure");

            restarted.SetResult();
            await Task.Delay(Timeout.InfiniteTimeSpan, token);
        });

        await restarted.Task.WaitAsync(TimeSpan.FromSeconds(2));
        Assert.IsType<InvalidOperationException>(failure);
        Assert.Equal(1, loop.RestartCount);

        await loop.StopAsync();
        loop.Dispose();
    }

    private static async Task WaitAsync(Func<bool> condition)
    {
        DateTime deadline = DateTime.UtcNow + TimeSpan.FromSeconds(2);
        while (!condition() && DateTime.UtcNow < deadline)
            await Task.Delay(1);

        Assert.True(condition(), "Condition was not met before the timeout.");
    }
}
