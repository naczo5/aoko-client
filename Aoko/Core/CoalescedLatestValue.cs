using System;
using System.Diagnostics;

namespace Aoko.Core;

/// <summary>
/// Keeps one scheduled callback while retaining the latest value published by
/// producers. A value published while the callback is running schedules one
/// trailing callback, never an unbounded queue of stale work.
/// </summary>
internal sealed class CoalescedLatestValue<T>
{
    private readonly object _gate = new();
    private readonly Action<Action> _schedule;
    private readonly Action<T> _consume;
    private T _latest = default!;
    private long _generation;
    private bool _pending;
    private bool _disposed;

    public CoalescedLatestValue(Action<Action> schedule, Action<T> consume)
    {
        ArgumentNullException.ThrowIfNull(schedule);
        ArgumentNullException.ThrowIfNull(consume);
        _schedule = schedule;
        _consume = consume;
    }

    /// <summary>
    /// Publishes a value. When <paramref name="allowSchedule"/> is false the
    /// value is retained for a later publish that is allowed to schedule work.
    /// </summary>
    public bool Publish(T value, bool allowSchedule)
    {
        lock (_gate)
        {
            if (_disposed)
                return false;

            _latest = value;
            _generation++;
            if (_pending || !allowSchedule)
                return false;

            _pending = true;
        }

        try
        {
            _schedule(Drain);
            return true;
        }
        catch
        {
            lock (_gate)
                _pending = false;
            throw;
        }
    }

    public void Dispose()
    {
        lock (_gate)
        {
            _disposed = true;
            _pending = false;
        }
    }

    internal bool IsPending
    {
        get
        {
            lock (_gate)
                return _pending;
        }
    }

    private void Drain()
    {
        T value;
        long generation;
        lock (_gate)
        {
            if (_disposed)
            {
                _pending = false;
                return;
            }

            value = _latest;
            generation = _generation;
        }

        try
        {
            _consume(value);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[CoalescedLatestValue] Consumer failed: {ex.Message}");
        }

        bool scheduleTrailing;
        lock (_gate)
        {
            if (_disposed)
            {
                _pending = false;
                return;
            }

            scheduleTrailing = _generation != generation;
            if (!scheduleTrailing)
                _pending = false;
        }

        if (!scheduleTrailing)
            return;

        try
        {
            _schedule(Drain);
        }
        catch
        {
            lock (_gate)
                _pending = false;
        }
    }
}
