using System.Reflection;
using Aoko.Core;

namespace Aoko.Tests;

public class FightStatusWiringTests
{
    private static readonly string RepoRoot = FindRepoRoot();

    [Fact]
    public void ProfileManager_MapsFightStatusBothDirections()
    {
        string source = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "Core", "Profile.cs"));

        Assert.Contains("FightStatusEnabled = clicker.FightStatusEnabled", source, StringComparison.Ordinal);
        Assert.Contains("clicker.FightStatusEnabled = profile.FightStatusEnabled", source, StringComparison.Ordinal);
        Assert.Contains("[\"fightstatus\"]      = 0", source, StringComparison.Ordinal);
    }

    [Fact]
    public void ConfigPayload_ContainsFightStatusAndItsKeybind()
    {
        string source = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "Core", "GameStateClient.cs"));

        Assert.Contains("fightStatus = clicker.FightStatusEnabled", source, StringComparison.Ordinal);
        Assert.Contains("keybindFightStatus   = InputHooks.GetModuleKey(\"fightstatus\")", source, StringComparison.Ordinal);
    }

    [Fact]
    public void NativeCapabilityPayloads_AdvertiseFightStatusSettingAndKeybind()
    {
        string header = File.ReadAllText(Path.Combine(RepoRoot, "McInjector", "src", "main", "cpp", "bridge_capabilities.h"));
        int modern = header.IndexOf("ModernCapabilitiesJson", StringComparison.Ordinal);
        int legacy = header.IndexOf("LegacyCapabilitiesJson", StringComparison.Ordinal);

        Assert.True(legacy >= 0);
        Assert.True(modern > legacy);
        string legacyPayload = header[legacy..modern];
        string modernPayload = header[modern..];

        Assert.Contains("\\\"fightstatus\\\"", legacyPayload, StringComparison.Ordinal);
        Assert.Contains("\\\"keybindfightstatus\\\"", legacyPayload, StringComparison.Ordinal);
        Assert.Contains("\\\"fightstatus\\\"", modernPayload, StringComparison.Ordinal);
        Assert.Contains("\\\"keybindfightstatus\\\"", modernPayload, StringComparison.Ordinal);
    }

    [Fact]
    public void LocalAndBridgeToggles_UpdateFightStatus()
    {
        Clicker clicker = Clicker.Instance;
        bool original = clicker.FightStatusEnabled;
        MethodInfo? localToggle = typeof(InputHooks).GetMethod("ToggleModule", BindingFlags.NonPublic | BindingFlags.Static);
        MethodInfo? bridgeCommand = typeof(GameStateClient).GetMethod("HandleBridgeCommand", BindingFlags.NonPublic | BindingFlags.Instance);

        Assert.NotNull(localToggle);
        Assert.NotNull(bridgeCommand);

        try
        {
            clicker.FightStatusEnabled = false;
            localToggle!.Invoke(null, new object[] { "fightstatus" });
            Assert.True(clicker.FightStatusEnabled);

            bridgeCommand!.Invoke(GameStateClient.Instance, new object[] { "{\"action\":\"toggleFightStatus\"}" });
            Assert.False(clicker.FightStatusEnabled);
        }
        finally
        {
            clicker.FightStatusEnabled = original;
        }
    }

    [Fact]
    public void Panic_ResetDisablesFightStatus()
    {
        string source = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "Core", "Clicker.cs"));
        int start = source.IndexOf("public async Task TriggerPanicAsync()", StringComparison.Ordinal);
        int end = source.IndexOf("private void StartAimAssistLoop()", start, StringComparison.Ordinal);

        Assert.True(start >= 0);
        Assert.True(end > start);
        Assert.Contains("FightStatusEnabled = false;", source[start..end], StringComparison.Ordinal);
    }

    private static string FindRepoRoot()
    {
        string? dir = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
        while (!string.IsNullOrEmpty(dir))
        {
            if (File.Exists(Path.Combine(dir, "Aoko", "MainWindow.xaml")))
                return dir;
            dir = Directory.GetParent(dir)?.FullName;
        }

        string cwd = Directory.GetCurrentDirectory();
        if (File.Exists(Path.Combine(cwd, "Aoko", "MainWindow.xaml")))
            return cwd;

        string? parent = Directory.GetParent(cwd)?.FullName;
        if (parent != null && File.Exists(Path.Combine(parent, "Aoko", "MainWindow.xaml")))
            return parent;

        throw new InvalidOperationException("Could not locate repository root.");
    }
}
