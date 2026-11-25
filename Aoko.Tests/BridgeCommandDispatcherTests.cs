using System.Diagnostics;
using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

/// <summary>
/// Correlation, timeout, and cancellation behaviour of the requestId registry.
/// The dispatcher takes the send/connected seams so no socket is required.
/// </summary>
public sealed class BridgeCommandDispatcherTests
{
    private const string SetSessionAction = BridgeActions.SetSession;

    [Fact]
    public async Task SendAsync_CompletesWithTheMatchingReply()
    {
        int requestId = 0;
        var dispatcher = CreateDispatcher();
        Task<BridgeCommandResult> pending = dispatcher.SendAsync(
            SetSessionAction,
            id =>
            {
                requestId = id;
                return "line\n";
            },
            CancellationToken.None);

        await WaitForPendingAsync(dispatcher);
        Assert.True(dispatcher.TryComplete(Reply(requestId, action: SetSessionAction, ok: true, username: "Notch")));

        BridgeCommandResult result = await pending;

        Assert.True(result.Ok);
        Assert.Equal("Notch", result.Username);
        Assert.Equal("Switched to Notch.", result.Message);
        Assert.Equal(0, dispatcher.PendingCount);
    }

    [Fact]
    public async Task SendAsync_ReplyWithoutActionKeepsTheRequestedAction()
    {
        int requestId = 0;
        var dispatcher = CreateDispatcher();
        Task<BridgeCommandResult> pending = dispatcher.SendAsync(
            BridgeActions.Join,
            id =>
            {
                requestId = id;
                return "line\n";
            },
            CancellationToken.None);

        await WaitForPendingAsync(dispatcher);
        Assert.True(dispatcher.TryComplete(Reply(requestId, action: null, ok: true)));

        BridgeCommandResult result = await pending;

        Assert.Equal(BridgeActions.Join, result.Action);
        Assert.Equal("Join started. The client is connecting to the server.", result.Message);
    }

    [Fact]
    public async Task SendAsync_NotConnectedFailsWithoutSending()
    {
        int sends = 0;
        var dispatcher = CreateDispatcher(
            send: (_, _) =>
            {
                sends++;
                return Task.FromResult(true);
            },
            connected: false);

        BridgeCommandResult result = await dispatcher.SendAsync(
            SetSessionAction,
            _ => "line\n",
            CancellationToken.None);

        Assert.False(result.Ok);
        Assert.Equal(BridgeCommandResult.NotConnectedError, result.Error);
        Assert.Equal(0, sends);
        Assert.Equal(0, dispatcher.PendingCount);
    }

    [Fact]
    public async Task SendAsync_RefusedSendIsReported()
    {
        var dispatcher = CreateDispatcher(send: (_, _) => Task.FromResult(false));

        BridgeCommandResult result = await dispatcher.SendAsync(
            SetSessionAction,
            _ => "line\n",
            CancellationToken.None);

        Assert.False(result.Ok);
        Assert.Equal(BridgeCommandResult.SendFailedError, result.Error);
        Assert.Equal(0, dispatcher.PendingCount);
    }

    [Fact]
    public async Task SendAsync_ClosedSocketIsReportedAsConnectionLost()
    {
        var dispatcher = CreateDispatcher(
            send: (_, _) => Task.FromException<bool>(new IOException("socket closed")));

        BridgeCommandResult result = await dispatcher.SendAsync(
            SetSessionAction,
            _ => "line\n",
            CancellationToken.None);

        Assert.False(result.Ok);
        Assert.Equal(BridgeCommandResult.ConnectionLostError, result.Error);
        Assert.Equal(0, dispatcher.PendingCount);
    }

    [Fact]
    public async Task SendAsync_NoReplyTimesOutWithAMenuSentence()
    {
        var dispatcher = CreateDispatcher(timeoutMs: 50);

        BridgeCommandResult result = await dispatcher.SendAsync(
            SetSessionAction,
            _ => "line\n",
            CancellationToken.None);

        Assert.False(result.Ok);
        Assert.Equal(BridgeCommandResult.TimeoutError, result.Error);
        Assert.Equal(
            "The game did not answer in time. Stay on the main menu and retry.",
            result.Message);
        Assert.Equal(0, dispatcher.PendingCount);
    }

    [Fact]
    public async Task SendAsync_CancellationThrowsAndLeavesNothingPending()
    {
        var dispatcher = CreateDispatcher();
        using var cts = new CancellationTokenSource();

        Task<BridgeCommandResult> pending = dispatcher.SendAsync(
            SetSessionAction,
            _ => "line\n",
            cts.Token);
        cts.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => pending);
        Assert.Equal(0, dispatcher.PendingCount);
    }

    [Fact]
    public async Task SendAsync_AlreadyCancelledTokenNeverSends()
    {
        int sends = 0;
        var dispatcher = CreateDispatcher(
            send: (_, _) =>
            {
                sends++;
                return Task.FromResult(true);
            });
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => dispatcher.SendAsync(
            SetSessionAction,
            _ => "line\n",
            cts.Token));

        Assert.Equal(0, sends);
        Assert.Equal(0, dispatcher.PendingCount);
    }

    [Fact]
    public async Task FailAll_CompletesEveryPendingRequest()
    {
        var dispatcher = CreateDispatcher();
        Task<BridgeCommandResult> first = dispatcher.SendAsync(
            BridgeActions.GetSession,
            _ => "line\n",
            CancellationToken.None);
        Task<BridgeCommandResult> second = dispatcher.SendAsync(
            BridgeActions.Join,
            _ => "line\n",
            CancellationToken.None);

        await WaitForPendingCountAsync(dispatcher, 2);
        Assert.Equal(2, dispatcher.FailAll(BridgeCommandResult.ConnectionLostError));

        BridgeCommandResult firstResult = await first;
        BridgeCommandResult secondResult = await second;

        Assert.False(firstResult.Ok);
        Assert.Equal(BridgeActions.GetSession, firstResult.Action);
        Assert.Equal(BridgeCommandResult.ConnectionLostError, firstResult.Error);
        Assert.Equal(BridgeActions.Join, secondResult.Action);
        Assert.Equal(0, dispatcher.PendingCount);
        Assert.Equal(0, dispatcher.FailAll(BridgeCommandResult.ConnectionLostError));
    }

    [Fact]
    public async Task TryComplete_ALateReplyAfterTheTimeoutIsDiscarded()
    {
        var dispatcher = CreateDispatcher(timeoutMs: 50);
        int requestId = 0;
        BridgeCommandResult timedOut = await dispatcher.SendAsync(
            SetSessionAction,
            id =>
            {
                requestId = id;
                return "line\n";
            },
            CancellationToken.None);

        Assert.Equal(BridgeCommandResult.TimeoutError, timedOut.Error);
        Assert.Equal(0, dispatcher.PendingCount);

        // The bridge answers after the budget expired: nothing to complete, no double completion,
        // no exception from the orphaned completion source.
        Assert.False(dispatcher.TryComplete(Reply(requestId, action: SetSessionAction, ok: true)));
        Assert.Equal(0, dispatcher.PendingCount);
    }

    [Fact]
    public async Task TryComplete_AReplyAfterFailAllIsDiscarded()
    {
        var dispatcher = CreateDispatcher();
        int requestId = 0;
        Task<BridgeCommandResult> pending = dispatcher.SendAsync(
            SetSessionAction,
            id =>
            {
                requestId = id;
                return "line\n";
            },
            CancellationToken.None);

        await WaitForPendingAsync(dispatcher);
        Assert.Equal(1, dispatcher.FailAll(BridgeCommandResult.ConnectionLostError));

        BridgeCommandResult failed = await pending;
        Assert.Equal(BridgeCommandResult.ConnectionLostError, failed.Error);
        Assert.False(dispatcher.TryComplete(Reply(requestId, action: SetSessionAction, ok: true)));
        Assert.False(failed.Ok);
    }

    [Fact]
    public async Task TryComplete_UnknownRequestIdIsIgnored()
    {
        var dispatcher = CreateDispatcher();
        Task<BridgeCommandResult> pending = dispatcher.SendAsync(
            SetSessionAction,
            _ => "line\n",
            CancellationToken.None);
        await WaitForPendingAsync(dispatcher);

        Assert.False(dispatcher.TryComplete(Reply(9999, action: SetSessionAction, ok: true)));
        Assert.False(dispatcher.TryComplete(JsonNode.Parse("""{"type":"state"}""")));
        Assert.Equal(1, dispatcher.PendingCount);

        Assert.Equal(1, dispatcher.FailAll(BridgeCommandResult.ConnectionLostError));
        Assert.False((await pending).Ok);
    }

    [Fact]
    public async Task TryComplete_CompletesOnlyTheAddressedRequest()
    {
        var dispatcher = CreateDispatcher();
        int firstId = 0;
        int secondId = 0;
        Task<BridgeCommandResult> first = dispatcher.SendAsync(
            BridgeActions.GetSession,
            id =>
            {
                firstId = id;
                return "line\n";
            },
            CancellationToken.None);
        Task<BridgeCommandResult> second = dispatcher.SendAsync(
            BridgeActions.Join,
            id =>
            {
                secondId = id;
                return "line\n";
            },
            CancellationToken.None);

        await WaitForPendingCountAsync(dispatcher, 2);
        Assert.NotEqual(firstId, secondId);
        Assert.True(dispatcher.TryComplete(Reply(secondId, action: BridgeActions.Join, ok: true)));

        Assert.Equal(1, dispatcher.PendingCount);
        Assert.False(first.IsCompleted);

        Assert.True(dispatcher.TryComplete(Reply(firstId, action: BridgeActions.GetSession, ok: false, error: "in_world")));
        Assert.False((await first).Ok);
        Assert.True((await second).Ok);
    }

    private static BridgeCommandDispatcher CreateDispatcher(
        Func<string, CancellationToken, Task<bool>>? send = null,
        bool connected = true,
        int timeoutMs = BridgeCommandDispatcher.DefaultTimeoutMs)
        => new(send ?? ((_, _) => Task.FromResult(true)), () => connected, timeoutMs);

    private static JsonNode Reply(int requestId, string? action, bool ok, string? username = null, string? error = null)
    {
        var payload = new JsonObject
        {
            ["type"] = "commandResult",
            ["requestId"] = requestId,
            ["ok"] = ok
        };
        if (action != null)
            payload["action"] = action;
        if (username != null)
            payload["username"] = username;
        if (error != null)
            payload["error"] = error;

        return payload;
    }

    private static async Task WaitForPendingAsync(BridgeCommandDispatcher dispatcher)
        => await WaitForPendingCountAsync(dispatcher, 1);

    private static async Task WaitForPendingCountAsync(BridgeCommandDispatcher dispatcher, int expected)
    {
        var stopwatch = Stopwatch.StartNew();
        while (dispatcher.PendingCount < expected)
        {
            if (stopwatch.ElapsedMilliseconds > 2000)
                throw new TimeoutException($"Only {dispatcher.PendingCount} of {expected} requests were registered.");

            await Task.Delay(5);
        }
    }
}
