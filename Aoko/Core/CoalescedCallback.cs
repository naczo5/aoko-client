using System;
using System.Diagnostics;
using System.Threading;

namespace Aoko.Core;

/// <summary>
/// Collapses bursts into one callback per interval while guaranteeing that the
/// latest signal receives a callback after an active interval.
/// </summary>
internal sealed class CoalescedCallback : IDisposable
{
    private readonly Action _callback;
    private readonly int _minimumIntervalMs;
    private readonly Timer _timer;
    private long _lastCallbackAt;
    private int _pending;
    private int _disposed;

    public CoalescedCallback(Action callback, int minimumIntervalMs)
    {
        ArgumentNullException.ThrowIfNull(callback);
        ArgumentOutOfRangeException.ThrowIfNegative(minimumIntervalMs);

        _callback = callback;
        _minimumIntervalMs = minimumIntervalMs;
        _timer = new Timer(InvokeCallback, null, Timeout.Infinite, Timeout.Infinite);
    }

    public void Signal()
    {
        if (Volatile.Read(ref _disposed) != 0)
            return;
        if (Interlocked.CompareExchange(ref _pending, 1, 0) != 0)
            return;

        long now = Environment.TickCount64;
        long last = Interlocked.Read(ref _lastCallbackAt);
        int dueTime = CalculateDueTime(last, now, _minimumIntervalMs);
        try
        {
            _timer.Change(dueTime, Timeout.Infinite);
        }
        catch (ObjectDisposedException)
        {
            Interlocked.Exchange(ref _pending, 0);
        }
    }

    public void Dispose()
    {
        if (Interlocked.Exchange(ref _disposed, 1) == 0)
            _timer.Dispose();
    }

    internal static int CalculateDueTime(
        long lastCallbackAt,
        long now,
        int minimumIntervalMs)
    {
        if (lastCallbackAt == 0 || minimumIntervalMs <= 0)
            return 0;

        long elapsed = now - lastCallbackAt;
        if (elapsed >= minimumIntervalMs)
            return 0;
        if (elapsed < 0)
            return minimumIntervalMs;
        return (int)Math.Min(minimumIntervalMs - elapsed, int.MaxValue);
    }

    private void InvokeCallback(object? state)
    {
        if (Volatile.Read(ref _disposed) != 0)
            return;

        Interlocked.Exchange(ref _lastCallbackAt, Environment.TickCount64);
        Interlocked.Exchange(ref _pending, 0);
        try
        {
            _callback();
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[CoalescedCallback] Subscriber failed: {ex.Message}");
        }
    }
}
