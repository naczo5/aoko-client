using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

public class AutoToolManagedTests
{
    private static readonly string RepoRoot = FindRepoRoot();

    [Fact]
    public void ModelAndProfile_DefaultsAndClampValues()
    {
        var defaults = new Profile();
        var profile = new Profile
        {
            AutoToolEnabled = true,
            AutoToolSwapWeapon = false,
            AutoToolInstantSwap = false,
            AutoToolSwapToDelay = 150,
            AutoToolSwapBack = true,
            AutoToolSwapBackDelay = 500,
            AutoToolRequireMouseDown = false,
            AutoToolOnlySneaking = true
        };

        Assert.False(defaults.AutoToolEnabled);
        Assert.True(defaults.AutoToolSwapWeapon);
        Assert.True(defaults.AutoToolInstantSwap);
        Assert.Equal(50, defaults.AutoToolSwapToDelay);
        Assert.False(defaults.AutoToolSwapBack);
        Assert.Equal(350, defaults.AutoToolSwapBackDelay);
        Assert.True(defaults.AutoToolRequireMouseDown);
        Assert.False(defaults.AutoToolOnlySneaking);
        Assert.Equal(0, defaults.ModuleKeys["autotool"]);

        Assert.True(profile.AutoToolEnabled);
        Assert.False(profile.AutoToolSwapWeapon);
        Assert.False(profile.AutoToolInstantSwap);
        Assert.Equal(150, profile.AutoToolSwapToDelay);
        Assert.True(profile.AutoToolSwapBack);
        Assert.Equal(500, profile.AutoToolSwapBackDelay);
        Assert.False(profile.AutoToolRequireMouseDown);
        Assert.True(profile.AutoToolOnlySneaking);

        Clicker clicker = Clicker.Instance;
        bool savedEnabled = clicker.AutoToolEnabled;
        bool savedSwapWeapon = clicker.AutoToolSwapWeapon;
        bool savedInstant = clicker.AutoToolInstantSwap;
        int savedSwapTo = clicker.AutoToolSwapToDelay;
        bool savedSwapBack = clicker.AutoToolSwapBack;
        int savedSwapBackDelay = clicker.AutoToolSwapBackDelay;
        bool savedMouseDown = clicker.AutoToolRequireMouseDown;
        bool savedSneaking = clicker.AutoToolOnlySneaking;

        try
        {
            clicker.AutoToolEnabled = true;
            clicker.AutoToolSwapWeapon = false;
            clicker.AutoToolInstantSwap = false;
            clicker.AutoToolSwapToDelay = -50;
            clicker.AutoToolSwapBack = true;
            clicker.AutoToolSwapBackDelay = 5000;
            clicker.AutoToolRequireMouseDown = false;
            clicker.AutoToolOnlySneaking = true;

            Assert.True(clicker.AutoToolEnabled);
            Assert.False(clicker.AutoToolSwapWeapon);
            Assert.False(clicker.AutoToolInstantSwap);
            Assert.Equal(0, clicker.AutoToolSwapToDelay);
            Assert.True(clicker.AutoToolSwapBack);
            Assert.Equal(1000, clicker.AutoToolSwapBackDelay);
            Assert.False(clicker.AutoToolRequireMouseDown);
            Assert.True(clicker.AutoToolOnlySneaking);
        }
        finally
        {
            clicker.AutoToolEnabled = savedEnabled;
            clicker.AutoToolSwapWeapon = savedSwapWeapon;
            clicker.AutoToolInstantSwap = savedInstant;
            clicker.AutoToolSwapToDelay = savedSwapTo;
            clicker.AutoToolSwapBack = savedSwapBack;
            clicker.AutoToolSwapBackDelay = savedSwapBackDelay;
            clicker.AutoToolRequireMouseDown = savedMouseDown;
            clicker.AutoToolOnlySneaking = savedSneaking;
        }
    }

    [Fact]
    public void Profile_CreateAndApplyRoundTrip()
    {
        Clicker clicker = Clicker.Instance;
        bool savedEnabled = clicker.AutoToolEnabled;
        bool savedSwapWeapon = clicker.AutoToolSwapWeapon;
        bool savedInstant = clicker.AutoToolInstantSwap;
        int savedSwapTo = clicker.AutoToolSwapToDelay;
        bool savedSwapBack = clicker.AutoToolSwapBack;
        int savedSwapBackDelay = clicker.AutoToolSwapBackDelay;
        bool savedMouseDown = clicker.AutoToolRequireMouseDown;
        bool savedSneaking = clicker.AutoToolOnlySneaking;

        try
        {
            clicker.AutoToolEnabled = true;
            clicker.AutoToolSwapWeapon = true;
            clicker.AutoToolInstantSwap = false;
            clicker.AutoToolSwapToDelay = 120;
            clicker.AutoToolSwapBack = true;
            clicker.AutoToolSwapBackDelay = 400;
            clicker.AutoToolRequireMouseDown = true;
            clicker.AutoToolOnlySneaking = false;

            Profile profile = ProfileManager.CreateFromClicker();
            Assert.True(profile.AutoToolEnabled);
            Assert.True(profile.AutoToolSwapWeapon);
            Assert.False(profile.AutoToolInstantSwap);
            Assert.Equal(120, profile.AutoToolSwapToDelay);
            Assert.True(profile.AutoToolSwapBack);
            Assert.Equal(400, profile.AutoToolSwapBackDelay);
            Assert.True(profile.AutoToolRequireMouseDown);
            Assert.False(profile.AutoToolOnlySneaking);

            // Change clicker values
            clicker.AutoToolEnabled = false;
            clicker.AutoToolSwapToDelay = 0;

            // Apply back
            ProfileManager.ApplyToClicker(profile);
            Assert.True(clicker.AutoToolEnabled);
            Assert.Equal(120, clicker.AutoToolSwapToDelay);
        }
        finally
        {
            clicker.AutoToolEnabled = savedEnabled;
            clicker.AutoToolSwapWeapon = savedSwapWeapon;
            clicker.AutoToolInstantSwap = savedInstant;
            clicker.AutoToolSwapToDelay = savedSwapTo;
            clicker.AutoToolSwapBack = savedSwapBack;
            clicker.AutoToolSwapBackDelay = savedSwapBackDelay;
            clicker.AutoToolRequireMouseDown = savedMouseDown;
            clicker.AutoToolOnlySneaking = savedSneaking;
        }
    }

    [Theory]
    [InlineData("1.8.9")]
    [InlineData("26.1")]
    [InlineData("26.2")]
    public void Capabilities_IncludesAutoToolOnAllSupportedVersions(string version)
    {
        var caps = BridgeCapabilities.ForVersionFallback(version);
        Assert.True(caps.SupportsModule("autotool"));
        Assert.True(caps.SupportsSetting("autotoolenabled"));
        Assert.True(caps.SupportsSetting("autotoolswapweapon"));
        Assert.True(caps.SupportsSetting("autotoolinstantswap"));
        Assert.True(caps.SupportsSetting("autotoolswaptodelay"));
        Assert.True(caps.SupportsSetting("autotoolswapback"));
        Assert.True(caps.SupportsSetting("autotoolswapbackdelay"));
        Assert.True(caps.SupportsSetting("autotoolrequiremousedown"));
        Assert.True(caps.SupportsSetting("autotoolonlysneaking"));
    }

    [Fact]
    public void NativeWiring_PresentInBothBridges()
    {
        string legacy = File.ReadAllText(Path.Combine(RepoRoot, "McInjector", "src", "main", "cpp", "bridge.cpp"));
        string modern = File.ReadAllText(Path.Combine(RepoRoot, "McInjector", "src", "main", "cpp", "bridge_261.cpp"));

        Assert.Contains("autoToolEnabled", legacy, StringComparison.Ordinal);
        Assert.Contains("autoToolSwapWeapon", legacy, StringComparison.Ordinal);
        Assert.Contains("UpdateAutoToolLegacy", legacy, StringComparison.Ordinal);
        Assert.Contains("ResetAutoToolLegacyJniCaches", legacy, StringComparison.Ordinal);
        Assert.Contains("ApplyAutoToolSlotLegacy", legacy, StringComparison.Ordinal);
        Assert.Contains("HasPendingAutoRodLegacyTransaction()", legacy, StringComparison.Ordinal);

        Assert.Contains("autoToolEnabled", modern, StringComparison.Ordinal);
        Assert.Contains("autoToolSwapWeapon", modern, StringComparison.Ordinal);
        Assert.Contains("UpdateAutoToolModern", modern, StringComparison.Ordinal);
        Assert.Contains("ResetAutoToolModernCaches", modern, StringComparison.Ordinal);
        Assert.Contains("ApplyAutoToolSlotModern", modern, StringComparison.Ordinal);
        Assert.Contains("IsAutoRodTransactionActive121()", modern, StringComparison.Ordinal);
    }

    [Fact]
    public void XamlCard_ConfiguredProperly()
    {
        string xaml = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "MainWindow.xaml"));

        Assert.Contains("x:Name=\"AutoToolCard\"", xaml, StringComparison.Ordinal);
        Assert.Contains("x:Name=\"AutoToolAvailabilityText\"", xaml, StringComparison.Ordinal);
        Assert.Contains("AutoToolEnabled", xaml, StringComparison.Ordinal);
        Assert.Contains("AutoToolSwapWeapon", xaml, StringComparison.Ordinal);
        Assert.Contains("AutoToolInstantSwap", xaml, StringComparison.Ordinal);
        Assert.Contains("AutoToolSwapToDelay", xaml, StringComparison.Ordinal);
        Assert.Contains("AutoToolSwapBack", xaml, StringComparison.Ordinal);
        Assert.Contains("AutoToolSwapBackDelay", xaml, StringComparison.Ordinal);
        Assert.Contains("AutoToolRequireMouseDown", xaml, StringComparison.Ordinal);
        Assert.Contains("AutoToolOnlySneaking", xaml, StringComparison.Ordinal);
        Assert.Contains("x:Name=\"KeybindAutoToolButton\" Tag=\"autotool\"", xaml, StringComparison.Ordinal);
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

        return Directory.GetCurrentDirectory();
    }
}
