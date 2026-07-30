using System.Diagnostics;
using System.Reflection;
using Aoko.Core;

namespace Aoko.Tests;

public sealed class ManagedTransportDiagnosticsTests
{
    [Fact]
    public void Enabled_ReflectsConfiguredState()
    {
        Assert.True(new ManagedTransportDiagnostics(enabled: true).Enabled);
        Assert.False(new ManagedTransportDiagnostics(enabled: false).Enabled);
    }

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

    [Theory]
    [InlineData(1, 1, false, true)]
    [InlineData(2, 1, true, true)]
    [InlineData(1, 1, true, false)]
    public void ConfigSerializationPolicy_ReusesUnchangedPayload(
        long revision,
        long cachedRevision,
        bool hasCachedPayload,
        bool expected)
    {
        Assert.Equal(
            expected,
            GameStateClient.ShouldSerializeConfig(
                revision,
                cachedRevision,
                hasCachedPayload));
    }

    [Fact]
    public void ConfigChangeTracking_CoversPropertiesNestedSettingsAndKeybinds()
    {
        MethodInfo? method = typeof(GameStateClient).GetMethod(
            "EnsureConfigChangeTracking",
            BindingFlags.Instance | BindingFlags.NonPublic);
        Assert.NotNull(method);

        string source = File.ReadAllText(
            Path.Combine(
                FindRepoRoot(),
                "Aoko",
                "Core",
                "GameStateClient.cs"));
        Assert.Contains(
            "Clicker.Instance.PropertyChanged += OnClickerConfigPropertyChanged;",
            source,
            StringComparison.Ordinal);
        Assert.Contains(
            "Clicker.Instance.StateChanged += MarkBridgeConfigDirty;",
            source,
            StringComparison.Ordinal);
        Assert.Contains(
            "InputHooks.OnStateChanged += MarkBridgeConfigDirty;",
            source,
            StringComparison.Ordinal);
    }

    [Fact]
    public void DiagnosticsFlag_IsCarriedToBothNativeBridges()
    {
        string root = FindRepoRoot();
        string managedSource = File.ReadAllText(
            Path.Combine(root, "Aoko", "Core", "GameStateClient.cs"));
        string legacySource = File.ReadAllText(
            Path.Combine(root, "McInjector", "src", "main", "cpp", "bridge.cpp"));
        string modernSource = File.ReadAllText(
            Path.Combine(root, "McInjector", "src", "main", "cpp", "bridge_261.cpp"));

        Assert.Contains(
            "perfDiagnostics = _transportDiagnostics.Enabled",
            managedSource,
            StringComparison.Ordinal);
        Assert.Contains(
            "reader.GetString(\"perfDiagnostics\")",
            legacySource,
            StringComparison.Ordinal);
        Assert.Contains(
            "reader.GetString(\"perfDiagnostics\")",
            modernSource,
            StringComparison.Ordinal);
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
        diagnostics.RecordStateNotification();
        diagnostics.RecordConfigSerialization(300, 25);
        diagnostics.RecordConfigSend(300);

        Assert.True(diagnostics.TryTakeSnapshot(now, out ManagedTransportSnapshot snapshot));
        Assert.Equal(2, snapshot.InboundMessages);
        Assert.Equal(200, snapshot.InboundCharacters);
        Assert.Equal(20, snapshot.InboundParseTicks);
        Assert.Equal(1, snapshot.StateNotifications);
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
            StateNotifications: 25,
            ConfigSerializations: 2,
            ConfigCharacters: 500,
            ConfigSerializationTicks: Stopwatch.Frequency / 200,
            ConfigSends: 1,
            ConfigSendCharacters: 250,
            WindowSeconds: 5);

        string message = snapshot.ToLogMessage();

        Assert.Contains("inbound=2.0/s", message);
        Assert.Contains("stateNotify=5.0/s", message);
        Assert.Contains("configSerialize=0.40/s", message);
        Assert.Contains("configSend=0.20/s", message);
    }

    private static string FindRepoRoot()
    {
        string? directory = AppContext.BaseDirectory;
        while (!string.IsNullOrEmpty(directory))
        {
            if (File.Exists(Path.Combine(directory, "Aoko", "Aoko.csproj")))
                return directory;
            directory = Directory.GetParent(directory)?.FullName;
        }

        throw new InvalidOperationException("Could not locate repository root.");
    }
}
