using System.Diagnostics;
using Aoko.Core;

namespace Aoko.Tests;

public sealed class ManagedTransportDiagnosticsTests
{
    [Theory]
    [InlineData(2, 1, 0, 2000, true)]
    [InlineData(1, 1, 1999, 2000, false)]
    [InlineData(1, 1, 2000, 2000, true)]
    [InlineData(1, 1, 5000, 2000, true)]
    public void ConfigSendPolicy_SendsOnRevisionOrHeartbeat(
        long revision,
        long lastSentRevision,
        long elapsedMs,
        int heartbeatMs,
        bool expected)
    {
        Assert.Equal(
            expected,
            GameStateClient.IsConfigSendDue(
                revision,
                lastSentRevision,
                elapsedMs,
                heartbeatMs));
    }

    [Fact]
    public void DisabledDiagnostics_DoNotProduceSnapshots()
    {
        var diagnostics = new ManagedTransportDiagnostics(
            enabled: false,
            summaryInterval: TimeSpan.Zero);

        diagnostics.RecordInbound(100, 20);
        diagnostics.RecordConfigSerialization(200, 30);
        diagnostics.RecordConfigSend(200);

        Assert.False(diagnostics.TryTakeSnapshot(long.MaxValue, out _));
    }

    [Fact]
    public void Snapshot_AtomicallyReportsAndResetsTheWindow()
    {
        var diagnostics = new ManagedTransportDiagnostics(
            enabled: true,
            summaryInterval: TimeSpan.Zero);
        long now = Stopwatch.GetTimestamp() + Stopwatch.Frequency;

        diagnostics.RecordInbound(120, 12);
        diagnostics.RecordInbound(80, 8);
        diagnostics.RecordConfigSerialization(300, 25);
        diagnostics.RecordConfigSend(300);

        Assert.True(diagnostics.TryTakeSnapshot(now, out ManagedTransportSnapshot snapshot));
        Assert.Equal(2, snapshot.InboundMessages);
        Assert.Equal(200, snapshot.InboundCharacters);
        Assert.Equal(20, snapshot.InboundParseTicks);
        Assert.Equal(1, snapshot.ConfigSerializations);
        Assert.Equal(300, snapshot.ConfigCharacters);
        Assert.Equal(25, snapshot.ConfigSerializationTicks);
        Assert.Equal(1, snapshot.ConfigSends);
        Assert.Equal(300, snapshot.ConfigSendCharacters);
        Assert.True(snapshot.WindowSeconds > 0);

        Assert.True(diagnostics.TryTakeSnapshot(now + 1, out ManagedTransportSnapshot reset));
        Assert.Equal(0, reset.InboundMessages);
        Assert.Equal(0, reset.ConfigSerializations);
        Assert.Equal(0, reset.ConfigSends);
    }

    [Fact]
    public void LogMessage_ContainsRatesWithoutPayloadContent()
    {
        var snapshot = new ManagedTransportSnapshot(
            InboundMessages: 10,
            InboundCharacters: 1000,
            InboundParseTicks: Stopwatch.Frequency / 100,
            ConfigSerializations: 2,
            ConfigCharacters: 500,
            ConfigSerializationTicks: Stopwatch.Frequency / 200,
            ConfigSends: 1,
            ConfigSendCharacters: 250,
            WindowSeconds: 5);

        string message = snapshot.ToLogMessage();

        Assert.Contains("inbound=2.0/s", message);
        Assert.Contains("configSerialize=0.40/s", message);
        Assert.Contains("configSend=0.20/s", message);
    }
}
