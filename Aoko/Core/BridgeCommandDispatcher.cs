using System;
using System.Collections.Concurrent;
using System.IO;
using System.Net.Sockets;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;

namespace Aoko.Core;

/// <summary>
/// Matches account/server commands with their correlated "commandResult" reply.
/// The registry, the 20 s budget, and cancellation cleanup live here so they stay
/// testable without a live bridge socket; <see cref="GameStateClient"/> owns the
/// transport and calls in from its read loop and teardown paths.
/// </summary>
internal sealed class BridgeCommandDispatcher
{
    internal const int DefaultTimeoutMs = 20000;

    private readonly ConcurrentDictionary<int, PendingCommand> _pending = new();
    private readonly BridgeRequestIdSource _requestIds = new();
    private readonly Func<string, CancellationToken, Task<bool>> _send;
    private readonly Func<bool> _isConnected;
    private readonly int _timeoutMs;

    internal BridgeCommandDispatcher(
        Func<string, CancellationToken, Task<bool>> send,
        Func<bool> isConnected,
        int timeoutMs = DefaultTimeoutMs)
    {
        _send = send ?? throw new ArgumentNullException(nameof(send));
        _isConnected = isConnected ?? throw new ArgumentNullException(nameof(isConnected));
        _timeoutMs = Math.Max(1, timeoutMs);
    }

    internal int PendingCount => _pending.Count;

    /// <summary>
    /// Sends one command line and waits for its reply. Failures the caller can act
    /// on come back as a result; cancelling <paramref name="token"/> throws
    /// <see cref="OperationCanceledException"/> and leaves no pending entry.
    /// </summary>
    internal async Task<BridgeCommandResult> SendAsync(
        string action,
        Func<int, string> buildLine,
        CancellationToken token)
    {
        if (!_isConnected())
            return BridgeCommandResult.Failure(action, BridgeCommandResult.NotConnectedError);

        token.ThrowIfCancellationRequested();

        int requestId = _requestIds.Next();
        var completion = new TaskCompletionSource<BridgeCommandResult>(
            TaskCreationOptions.RunContinuationsAsynchronously);
        _pending[requestId] = new PendingCommand(action, completion);

        try
        {
            bool sent = await _send(buildLine(requestId), token).ConfigureAwait(false);
            if (!sent)
                return BridgeCommandResult.Failure(action, BridgeCommandResult.SendFailedError);

            return await completion.Task
                .WaitAsync(TimeSpan.FromMilliseconds(_timeoutMs), token)
                .ConfigureAwait(false);
        }
        catch (TimeoutException)
        {
            return BridgeCommandResult.Failure(action, BridgeCommandResult.TimeoutError);
        }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException or SocketException)
        {
            return BridgeCommandResult.Failure(action, BridgeCommandResult.ConnectionLostError);
        }
        finally
        {
            _pending.TryRemove(requestId, out _);
        }
    }

    /// <summary>
    /// Completes the pending request a "commandResult" document belongs to.
    /// Returns false when the line is malformed or the id is unknown, so the read
    /// loop can ignore it without touching state parsing.
    /// </summary>
    internal bool TryComplete(JsonNode? node)
    {
        if (!BridgeCommandResult.TryParse(node, out int requestId, out BridgeCommandResult result))
            return false;

        if (!_pending.TryRemove(requestId, out PendingCommand pending))
            return false;

        pending.Completion.TrySetResult(result.WithAction(pending.Action));
        return true;
    }

    /// <summary>Fails every pending request, e.g. when the connection drops.</summary>
    internal int FailAll(string error)
    {
        int failed = 0;
        foreach (int requestId in _pending.Keys)
        {
            if (_pending.TryRemove(requestId, out PendingCommand pending))
            {
                pending.Completion.TrySetResult(
                    BridgeCommandResult.Failure(pending.Action, error));
                failed++;
            }
        }

        return failed;
    }

    private readonly record struct PendingCommand(
        string Action,
        TaskCompletionSource<BridgeCommandResult> Completion);
}
