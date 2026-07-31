using System;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;

namespace Aoko.Core;

/// <summary>
/// Owns one cancellable background loop without introducing a global module
/// scheduler. Start/Stop requests are serialized for this loop and a loop
/// that exits unexpectedly is restarted while it remains enabled.
/// </summary>
internal sealed class OwnedAsyncLoop : IDisposable
{
    private readonly object _gate = new();
    private readonly string _name;
    private readonly Action<Exception>? _onFailure;
    private CancellationTokenSource? _cts;
    private Task? _task;
    private Func<CancellationToken, Task>? _body;
    private bool _requested;
    private bool _disposed;
    private int _unexpectedCompletions;
    private int _restartCount;

    public OwnedAsyncLoop(string name, Action<Exception>? onFailure = null)
    {
        _name = string.IsNullOrWhiteSpace(name) ? "owned loop" : name;
        _onFailure = onFailure;
    }

    /// <summary>Whether the caller most recently requested that this loop run.</summary>
    public bool IsRequested
    {
        get
        {
            lock (_gate)
                return _requested;
        }
    }

    /// <summary>Whether a loop task is currently active.</summary>
    public bool IsRunning
    {
        get
        {
            lock (_gate)
                return _task != null;
        }
    }

    /// <summary>Unexpected loop exits observed while the loop was requested.</summary>
    public int UnexpectedCompletionCount => Volatile.Read(ref _unexpectedCompletions);

    /// <summary>Number of automatic restarts after an unexpected loop exit.</summary>
    public int RestartCount => Volatile.Read(ref _restartCount);

    /// <summary>
    /// Requests the loop to run. The request is synchronous; if a prior
    /// cancellation is still draining, its completion will start this body.
    /// </summary>
    public void Start(Func<CancellationToken, Task> body)
    {
        ArgumentNullException.ThrowIfNull(body);

        lock (_gate)
        {
            ThrowIfDisposed();
            _body = body;
            _requested = true;
            if (_task == null)
                LaunchLocked();
        }
    }

    /// <summary>
    /// Cancels the current iteration and returns immediately. Use StopAsync
    /// when the caller is able to await owned-loop cleanup.
    /// </summary>
    public void Stop()
    {
        lock (_gate)
        {
            _requested = false;
            _cts?.Cancel();
        }
    }

    /// <summary>
    /// Requests cancellation and awaits the loop that was current when the
    /// request acquired the lock. A concurrently issued later <see cref="Start"/>
    /// request wins by design; callers that require a quiescent loop should
    /// stop it before publishing a new start request.
    /// </summary>
    public async Task StopAsync()
    {
        Task? task;
        lock (_gate)
        {
            _requested = false;
            _cts?.Cancel();
            task = _task;
        }

        if (task == null)
            return;

        try
        {
            await task.ConfigureAwait(false);
        }
        catch
        {
            // ExecuteAsync observes failures and keeps cleanup fail-open.
        }
    }

    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed)
                return;

            _disposed = true;
            _requested = false;
            _cts?.Cancel();
        }
    }

    private void LaunchLocked(bool restart = false)
    {
        if (!_requested || _disposed || _body == null)
            return;

        var cts = new CancellationTokenSource();
        _cts = cts;
        Func<CancellationToken, Task> body = _body;
        var restartDelayMs = restart
            ? Math.Min(1000, 1 << Math.Min(_restartCount, 10))
            : 0;
        _task = Task.Run(() => ExecuteAsync(body, cts, restartDelayMs));
    }

    private async Task ExecuteAsync(
        Func<CancellationToken, Task> body,
        CancellationTokenSource cts,
        int restartDelayMs)
    {
        bool canceled = false;
        try
        {
            // Avoid a tight fault/restart spin if a loop fails before reaching
            // its normal delay. The cancellation path remains immediate.
            if (restartDelayMs > 0)
                await Task.Delay(restartDelayMs, cts.Token).ConfigureAwait(false);

            await body(cts.Token).ConfigureAwait(false);
            canceled = cts.IsCancellationRequested;
        }
        catch (OperationCanceledException) when (cts.IsCancellationRequested)
        {
            canceled = true;
        }
        catch (Exception ex)
        {
            RecordFailure(ex);
        }
        finally
        {
            Complete(cts, unexpected: !canceled);
        }
    }

    private void Complete(CancellationTokenSource cts, bool unexpected)
    {
        lock (_gate)
        {
            // A future implementation may allow an overlap-free handoff; this
            // guard keeps a stale completion from touching the current owner.
            if (!ReferenceEquals(_cts, cts))
                return;

            _cts = null;
            _task = null;
            cts.Dispose();

            if (_requested && !_disposed && _body != null)
            {
                if (unexpected)
                {
                    Interlocked.Increment(ref _unexpectedCompletions);
                    Interlocked.Increment(ref _restartCount);
                }
                LaunchLocked(restart: unexpected);
            }
            else if (unexpected)
            {
                Interlocked.Increment(ref _unexpectedCompletions);
            }
        }
    }

    private void RecordFailure(Exception exception)
    {
        try
        {
            _onFailure?.Invoke(exception);
        }
        catch (Exception callbackFailure)
        {
            Debug.WriteLine($"[{_name}] failure callback threw: {callbackFailure}");
        }
    }

    private void ThrowIfDisposed()
    {
        if (_disposed)
            throw new ObjectDisposedException(nameof(OwnedAsyncLoop));
    }
}
