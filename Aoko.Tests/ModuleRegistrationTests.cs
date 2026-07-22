using System.Reflection;
using System.Text.RegularExpressions;
using Aoko.Core;

namespace Aoko.Tests;

/// <summary>
/// Ensures every module registered in <see cref="ModuleCatalog"/> is wired into
/// keybind GUI/maps, Discord RPC counting, and the bridge overlay module list.
/// Add new modules to ModuleCatalog first — these tests will fail until the other surfaces match.
/// </summary>
public class ModuleRegistrationTests
{
    private static readonly string RepoRoot = FindRepoRoot();

    [Fact]
    public void Catalog_HasUniqueIds()
    {
        var duplicates = ModuleCatalog.All
            .GroupBy(e => e.Id, StringComparer.OrdinalIgnoreCase)
            .Where(g => g.Count() > 1)
            .Select(g => g.Key)
            .ToList();

        Assert.True(duplicates.Count == 0,
            "Duplicate ModuleCatalog ids: " + string.Join(", ", duplicates));
    }

    [Fact]
    public void Catalog_CapabilityModules_MatchModernBridgeCapabilities()
    {
        var catalogIds = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.Capability)
            .Select(e => e.Id)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        var capabilityIds = BridgeCapabilities.ForVersionFallback("26.1").Modules
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        AssertMissing("ModuleCatalog (Capability)", capabilityIds, catalogIds);
        AssertMissing("BridgeCapabilities modern modules", catalogIds, capabilityIds);
    }

    [Fact]
    public void Catalog_CapabilityModules_MatchNativeModernCapabilitiesJson()
    {
        string header = File.ReadAllText(Path.Combine(RepoRoot, "McInjector", "src", "main", "cpp", "bridge_capabilities.h"));
        var nativeIds = ParseQuotedStringsInside(header, "ModernCapabilitiesJson", "\\\"modules\\\":[", "]");

        var catalogIds = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.Capability)
            .Select(e => e.Id)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        AssertMissing("ModuleCatalog (Capability)", nativeIds, catalogIds);
        AssertMissing("bridge_capabilities.h ModernCapabilitiesJson modules", catalogIds, nativeIds);
    }

    [Fact]
    public void KeybindGui_MainWindowXaml_HasButtonForEveryCatalogModule()
    {
        string xaml = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "MainWindow.xaml"));
        var tags = Regex.Matches(
                xaml,
                @"x:Name=""Keybind\w+Button""\s+Tag=""(?<id>[^""]+)""",
                RegexOptions.CultureInvariant)
            .Select(m => m.Groups["id"].Value)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        var expected = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.KeybindGui)
            .Select(e => e.Id)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        AssertMissing("MainWindow.xaml Keybind*Button Tag", expected, tags);
    }

    [Fact]
    public void KeybindGui_UpdateKeybindButtons_CoversEveryCatalogModule()
    {
        string code = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "MainWindow.xaml.cs"));
        int start = code.IndexOf("void UpdateKeybindButtons()", StringComparison.Ordinal);
        Assert.True(start >= 0, "UpdateKeybindButtons not found");
        int end = code.IndexOf("\n    private void SetKeybindButtonContent", start, StringComparison.Ordinal);
        Assert.True(end > start, "SetKeybindButtonContent boundary not found");
        string method = code[start..end];

        var ids = Regex.Matches(method, @"SetKeybindButtonContent\(\w+,\s*""(?<id>[^""]+)""\)")
            .Select(m => m.Groups["id"].Value)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        var expected = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.KeybindGui)
            .Select(e => e.Id)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        AssertMissing("UpdateKeybindButtons", expected, ids);
    }

    [Fact]
    public void KeybindGui_ModuleTitles_CoversEveryCatalogModule()
    {
        string code = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "MainWindow.xaml.cs"));
        int start = code.IndexOf("Dictionary<string, string> ModuleTitles", StringComparison.Ordinal);
        Assert.True(start >= 0, "ModuleTitles not found");
        int end = code.IndexOf("Dictionary<string, GuiPalette> GuiPalettes", start, StringComparison.Ordinal);
        Assert.True(end > start, "GuiPalettes boundary not found");
        string block = code[start..end];

        var ids = Regex.Matches(block, @"\[""(?<id>[^""]+)""\]")
            .Select(m => m.Groups["id"].Value)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        var expected = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.KeybindGui)
            .Select(e => e.Id)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        AssertMissing("ModuleTitles", expected, ids);
    }

    [Fact]
    public void KeybindMaps_InputHooksModuleKeys_CoverEveryCatalogModule()
    {
        var expected = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.KeybindMaps)
            .Select(e => e.Id)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        var actual = InputHooks.ModuleKeys.Keys.ToHashSet(StringComparer.OrdinalIgnoreCase);
        AssertMissing("InputHooks.ModuleKeys", expected, actual);
    }

    [Fact]
    public void KeybindMaps_ProfileModuleKeys_CoverEveryCatalogModule()
    {
        var expected = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.KeybindMaps)
            .Select(e => e.Id)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        var actual = new Profile().ModuleKeys.Keys.ToHashSet(StringComparer.OrdinalIgnoreCase);
        AssertMissing("Profile.ModuleKeys defaults", expected, actual);
    }

    [Fact]
    public void KeybindMaps_ToggleModuleSwitch_CoversEveryToggleableCatalogModule()
    {
        string code = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "Core", "InputHooks.cs"));
        int start = code.IndexOf("private static void ToggleModule(string moduleId)", StringComparison.Ordinal);
        Assert.True(start >= 0, "ToggleModule not found");
        int end = code.IndexOf("private static bool ShouldBlockModuleKeybinds()", start, StringComparison.Ordinal);
        Assert.True(end > start, "ShouldBlockModuleKeybinds boundary not found");
        string method = code[start..end];

        var cases = Regex.Matches(method, @"case\s+""(?<id>[^""]+)"":")
            .Select(m => m.Groups["id"].Value)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        // panic is handled above the switch; every other KeybindMaps module must be in the switch.
        var expected = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.KeybindMaps)
            .Select(e => e.Id)
            .Where(id => !string.Equals(id, "panic", StringComparison.OrdinalIgnoreCase))
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        AssertMissing("ToggleModule switch cases", expected, cases);
    }

    [Fact]
    public void DiscordRpc_CountTracksEveryCatalogModule()
    {
        Clicker clicker = Clicker.Instance;
        var snapshot = SnapshotDiscordFlags(clicker);
        bool savedDevMode = clicker.DevMode;
        try
        {
            ForceAllDiscordModules(clicker, enabled: false);
            Assert.Equal(0, ModuleCatalog.CountEnabledForDiscord(clicker));

            foreach (ModuleCatalog.Entry entry in ModuleCatalog.Requiring(ModuleCatalog.Surfaces.DiscordRpc))
            {
                ForceAllDiscordModules(clicker, enabled: false);
                clicker.DevMode = entry.DevOnly; // Dev-only modules only count while Dev Mode is on.
                Assert.True(TrySetDiscordEnabled(clicker, entry.Id, true),
                    $"No enable setter wired in test harness for Discord module '{entry.Id}'. " +
                    "Add it to TrySetDiscordEnabled when registering the module in ModuleCatalog.");
                Assert.True(entry.IsEnabled(clicker),
                    $"ModuleCatalog.IsEnabled returned false after enabling '{entry.Id}'");
                Assert.Equal(1, ModuleCatalog.CountEnabledForDiscord(clicker));
            }
        }
        finally
        {
            clicker.DevMode = savedDevMode;
            RestoreDiscordFlags(clicker, snapshot);
        }
    }

    private static Dictionary<string, bool> SnapshotDiscordFlags(Clicker clicker)
    {
        var snapshot = new Dictionary<string, bool>(StringComparer.OrdinalIgnoreCase);
        foreach (ModuleCatalog.Entry entry in ModuleCatalog.Requiring(ModuleCatalog.Surfaces.DiscordRpc))
            snapshot[entry.Id] = entry.IsEnabled(clicker);
        return snapshot;
    }

    private static void RestoreDiscordFlags(Clicker clicker, Dictionary<string, bool> snapshot)
    {
        foreach (var kvp in snapshot)
            TrySetDiscordEnabled(clicker, kvp.Key, kvp.Value);
    }

    [Fact]
    public void DiscordRpc_ServiceUsesModuleCatalog()
    {
        string code = File.ReadAllText(Path.Combine(RepoRoot, "Aoko", "Core", "DiscordRichPresenceService.cs"));
        Assert.Contains("ModuleCatalog.CountEnabledForDiscord", code, StringComparison.Ordinal);
    }

    [Fact]
    public void OverlayList_Bridge261PushMod_CoversEveryCatalogModule()
    {
        string bridge = File.ReadAllText(Path.Combine(RepoRoot, "McInjector", "src", "main", "cpp", "bridge_261.cpp"));
        string block = ExtractModuleListPushModBlock(bridge);
        Assert.False(string.IsNullOrWhiteSpace(block), "Could not locate overlay pushMod module-list block in bridge_261.cpp");

        var missing = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.OverlayList)
            .Where(e => string.IsNullOrWhiteSpace(e.OverlayProbe) || !block.Contains(e.OverlayProbe, StringComparison.Ordinal))
            .Select(e => $"{e.Id} (probe: {e.OverlayProbe})")
            .ToList();

        Assert.True(missing.Count == 0,
            "bridge_261.cpp module list missing catalog OverlayList probes:\n- " + string.Join("\n- ", missing));
    }

    [Fact]
    public void OverlayList_LegacyBridgePushMod_CoversLegacyCapableOverlayModules()
    {
        string bridge = File.ReadAllText(Path.Combine(RepoRoot, "McInjector", "src", "main", "cpp", "bridge.cpp"));
        string block = ExtractLegacyModuleListPushModBlock(bridge);
        Assert.False(string.IsNullOrWhiteSpace(block), "Could not locate overlay pushMod module-list block in bridge.cpp");

        var legacyCaps = BridgeCapabilities.ForVersionFallback("1.8.9").Modules
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        var missing = ModuleCatalog.Requiring(ModuleCatalog.Surfaces.OverlayList)
            .Where(e => legacyCaps.Contains(e.Id))
            .Where(e => string.IsNullOrWhiteSpace(e.OverlayProbe) || !block.Contains(e.OverlayProbe, StringComparison.Ordinal))
            .Select(e => $"{e.Id} (probe: {e.OverlayProbe})")
            .ToList();

        Assert.True(missing.Count == 0,
            "bridge.cpp module list missing legacy-capable OverlayList probes:\n- " + string.Join("\n- ", missing));
    }

    [Fact]
    public void Catalog_CapabilityModules_MatchNativeLegacyCapabilitiesJson()
    {
        string header = File.ReadAllText(Path.Combine(RepoRoot, "McInjector", "src", "main", "cpp", "bridge_capabilities.h"));
        var nativeIds = ParseQuotedStringsInside(header, "LegacyCapabilitiesJson", "\\\"modules\\\":[", "]");

        var fallbackIds = BridgeCapabilities.ForVersionFallback("1.8.9").Modules
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        AssertMissing("BridgeCapabilities legacy modules", nativeIds, fallbackIds);
        AssertMissing("bridge_capabilities.h LegacyCapabilitiesJson modules", fallbackIds, nativeIds);
    }

    private static void ForceAllDiscordModules(Clicker clicker, bool enabled)
    {
        foreach (ModuleCatalog.Entry entry in ModuleCatalog.Requiring(ModuleCatalog.Surfaces.DiscordRpc))
            Assert.True(TrySetDiscordEnabled(clicker, entry.Id, enabled),
                $"No enable setter wired in test harness for Discord module '{entry.Id}'");
    }

    private static bool TrySetDiscordEnabled(Clicker clicker, string moduleId, bool enabled)
    {
        switch (moduleId)
        {
            case "autoclicker": clicker.LeftClickEnabled = enabled; return true;
            case "rightclick": clicker.RightClickEnabled = enabled; return true;
            case "jitter": clicker.JitterEnabled = enabled; return true;
            case "clickinchests": clicker.ClickInChests = enabled; return true;
            case "breakblocks": clicker.BreakBlocksEnabled = enabled; return true;
            case "aimassist": clicker.AimAssistEnabled = enabled; return true;
            case "triggerbot": clicker.TriggerbotEnabled = enabled; return true;
            case "killaura": clicker.KillAuraEnabled = enabled; return true;
            case "speedbridge": clicker.SpeedBridgeEnabled = enabled; return true;
            case "gtbhelper": clicker.GtbHelperEnabled = enabled; return true;
            case "pixelpartyassist": clicker.PixelPartyAssistEnabled = enabled; return true;
            case "nametags": clicker.NametagsEnabled = enabled; return true;
            case "nickhider": clicker.NickHiderEnabled = enabled; return true;
            case "closestplayer": clicker.ClosestPlayerInfoEnabled = enabled; return true;
            case "chestesp": clicker.ChestEspEnabled = enabled; return true;
            case "cheststealer": clicker.ChestStealerEnabled = enabled; return true;
            case "blockesp": clicker.BlockEspEnabled = enabled; return true;
            case "reach": clicker.ReachEnabled = enabled; return true;
            case "velocity": clicker.VelocityEnabled = enabled; return true;
            case "autototem": clicker.AutoTotemEnabled = enabled; return true;
            case "antidebuff": clicker.AntiDebuffEnabled = enabled; return true;
            case "hitdelayfix": clicker.HitDelayFixEnabled = enabled; return true;
            default: return false;
        }
    }

    private static string ExtractModuleListPushModBlock(string bridgeSource)
    {
        // Prefer the modern overlay list that enumerates cfg.* before pushMod.
        Match match = Regex.Match(
            bridgeSource,
            @"if\s*\(\s*cfg\.armed\s*\)[\s\S]*?if\s*\(\s*cfg\.hitDelayFixEnabled\s*\)\s*pushMod\([^;]+;",
            RegexOptions.CultureInvariant);
        return match.Success ? match.Value : string.Empty;
    }

    private static string ExtractLegacyModuleListPushModBlock(string bridgeSource)
    {
        Match match = Regex.Match(
            bridgeSource,
            @"if\s*\(\s*cfg\.armed\s*\)[\s\S]*?if\s*\(\s*cfg\.hitDelayFixEnabled\s*\)\s*pushMod\([^;]+;",
            RegexOptions.CultureInvariant);
        return match.Success ? match.Value : string.Empty;
    }

    private static HashSet<string> ParseQuotedStringsInside(string source, string anchor, string startMarker, string endMarker)
    {
        int anchorIdx = source.IndexOf(anchor, StringComparison.Ordinal);
        Assert.True(anchorIdx >= 0, $"Anchor '{anchor}' not found");
        int start = source.IndexOf(startMarker, anchorIdx, StringComparison.Ordinal);
        Assert.True(start >= 0, $"Start marker '{startMarker}' not found after '{anchor}'");
        start += startMarker.Length;
        int end = source.IndexOf(endMarker, start, StringComparison.Ordinal);
        Assert.True(end > start, $"End marker '{endMarker}' not found after '{startMarker}'");
        string slice = source[start..end];

        return Regex.Matches(slice, "\\\\\"(?<id>[^\\\\\"]+)\\\\\"")
            .Select(m => m.Groups["id"].Value)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
    }

    private static void AssertMissing(string surface, IEnumerable<string> expected, ISet<string> actual)
    {
        var missing = expected
            .Where(id => !actual.Contains(id))
            .OrderBy(id => id, StringComparer.OrdinalIgnoreCase)
            .ToList();

        Assert.True(missing.Count == 0,
            $"{surface} is missing module id(s): {string.Join(", ", missing)}");
    }

    private static string FindRepoRoot()
    {
        string? dir = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
        while (!string.IsNullOrEmpty(dir))
        {
            if (File.Exists(Path.Combine(dir, "Aoko", "MainWindow.xaml"))
                && File.Exists(Path.Combine(dir, "McInjector", "src", "main", "cpp", "bridge_261.cpp")))
            {
                return dir;
            }

            dir = Directory.GetParent(dir)?.FullName;
        }

        // Fallback for `dotnet test` from repo root with content not copied beside the DLL.
        string cwd = Directory.GetCurrentDirectory();
        if (File.Exists(Path.Combine(cwd, "Aoko", "MainWindow.xaml")))
            return cwd;

        string? parent = Directory.GetParent(cwd)?.FullName;
        if (parent != null && File.Exists(Path.Combine(parent, "Aoko", "MainWindow.xaml")))
            return parent;

        throw new InvalidOperationException(
            "Could not locate repository root containing Aoko/MainWindow.xaml from test assembly location.");
    }
}
