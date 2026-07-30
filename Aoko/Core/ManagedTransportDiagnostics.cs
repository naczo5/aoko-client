using System;
using System.Diagnostics;
using System.Globalization;
using System.Threading;

namespace Aoko.Core;

internal readonly record struct ManagedTransportSnapshot(
    long InboundMessages,
    long InboundCharacters,
    long InboundParseTicks,
    long ConfigSerializations,
    long ConfigCharacters,
    long ConfigSerializationTicks,
    long ConfigSends,
    long ConfigSendCharacters,
    double WindowSeconds)
{
    public string ToLogMessage()
    {
        double seconds = Math.Max(WindowSeconds, 0.001);
        double timestampToMilliseconds = 1000.0 / Stopwatch.Frequency;
        return string.Create(
            CultureInfo.InvariantCulture,
            $"transport window={seconds:0.0}s " +
            $"inbound={InboundMessages / seconds:0.0}/s chars={InboundCharacters / seconds:0}/s " +
            $"parse={InboundParseTicks * timestampToMilliseconds / seconds:0.00}ms/s " +
            $"configSerialize={ConfigSerializations / seconds:0.00}/s " +
            $"configSerializeCpu={ConfigSerializationTicks * timestampToMilliseconds / seconds:0.00}ms/s " +
            $"configSend={ConfigSends / seconds:0.00}/s chars={ConfigSendCharacters / seconds:0}/s");
    }
}

internal sealed class ManagedTransportDiagnostics
{
    private readonly bool _enabled;
    private readonly long _summaryIntervalTicks;
    private long _windowStarted;
    private long _inboundMessages;
    private long _inboundCharacters;
    private long _inboundParseTicks;
    private long _configSerializations;
    private long _configCharacters;
    private long _configSerializationTicks;
    private long _configSends;
    private long _configSendCharacters;

    public ManagedTransportDiagnostics(bool enabled, TimeSpan? summaryInterval = null)
    {
        _enabled = enabled;
        TimeSpan interval = summaryInterval ?? TimeSpan.FromSeconds(5);
        _summaryIntervalTicks = Math.Max(1, (long)(interval.TotalSeconds * Stopwatch.Frequency));
        _windowStarted = Stopwatch.GetTimestamp();
    }

    public static ManagedTransportDiagnostics FromEnvironment()
    {
        string? value = Environment.GetEnvironmentVariable("AOKO_PERF_DIAGNOSTICS");
        bool enabled = string.Equals(value, "1", StringComparison.OrdinalIgnoreCase)
            || string.Equals(value, "true", StringComparison.OrdinalIgnoreCase);
        return new ManagedTransportDiagnostics(enabled);
    }

    public void RecordInbound(int characters, long parseTicks)
    {
        if (!_enabled) return;
        Interlocked.Increment(ref _inboundMessages);
        Interlocked.Add(ref _inboundCharacters, Math.Max(0, characters));
        Interlocked.Add(ref _inboundParseTicks, Math.Max(0, parseTicks));
    }

    public void RecordConfigSerialization(int characters, long serializationTicks)
    {
        if (!_enabled) return;
        Interlocked.Increment(ref _configSerializations);
        Interlocked.Add(ref _configCharacters, Math.Max(0, characters));
        Interlocked.Add(ref _configSerializationTicks, Math.Max(0, serializationTicks));
    }

    public void RecordConfigSend(int characters)
    {
        if (!_enabled) return;
        Interlocked.Increment(ref _configSends);
        Interlocked.Add(ref _configSendCharacters, Math.Max(0, characters));
    }

    public bool TryTakeSnapshot(long nowTimestamp, out ManagedTransportSnapshot snapshot)
    {
        snapshot = default;
        if (!_enabled) return false;

        long started = Interlocked.Read(ref _windowStarted);
        long elapsed = nowTimestamp - started;
        if (elapsed < _summaryIntervalTicks) return false;
        if (Interlocked.CompareExchange(ref _windowStarted, nowTimestamp, started) != started)
            return false;

        snapshot = new ManagedTransportSnapshot(
            Interlocked.Exchange(ref _inboundMessages, 0),
            Interlocked.Exchange(ref _inboundCharacters, 0),
            Interlocked.Exchange(ref _inboundParseTicks, 0),
            Interlocked.Exchange(ref _configSerializations, 0),
            Interlocked.Exchange(ref _configCharacters, 0),
            Interlocked.Exchange(ref _configSerializationTicks, 0),
            Interlocked.Exchange(ref _configSends, 0),
            Interlocked.Exchange(ref _configSendCharacters, 0),
            elapsed / (double)Stopwatch.Frequency);
        return true;
    }
}
