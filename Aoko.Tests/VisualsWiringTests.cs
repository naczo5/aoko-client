using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Aoko.Core;
using Aoko.Core.Visuals;

namespace Aoko.Tests;

public class VisualsWiringTests
{
    private static readonly string[] SettingKeys =
    [
        "cosmeticsEnabled", "cosmeticsCape", "cosmeticsHat", "cosmeticsBack",
        "skyEnabled", "skyPreset", "skyTime", "skyClearWeather", "visualsDir",
    ];

    private static readonly JsonSerializerOptions ProfileOptions = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };

    [Fact]
    public void Catalog_RegistersBothModulesAsBeta()
    {
        foreach (string id in new[] { "cosmetics", "sky" })
        {
            ModuleCatalog.Entry entry = Assert.Single(ModuleCatalog.All, e => e.Id == id);
            Assert.True(entry.Beta);
            Assert.False(entry.DevOnly);
            Assert.Equal(ModuleCatalog.StandardSurfaces, entry.RequiredSurfaces);
        }

        Assert.Equal("cfg.cosmeticsEnabled", ModuleCatalog.All.Single(e => e.Id == "cosmetics").OverlayProbe);
        Assert.Equal("cfg.skyEnabled", ModuleCatalog.All.Single(e => e.Id == "sky").OverlayProbe);
    }

    [Fact]
    public void Clicker_NormalizesIdsAndClampsSkyTime()
    {
        Clicker clicker = Clicker.Instance;
        string cape = clicker.CosmeticsCape, hat = clicker.CosmeticsHat, back = clicker.CosmeticsBack, sky = clicker.SkyPreset;
        int time = clicker.SkyTime;
        try
        {
            clicker.CosmeticsCape = "  Moon-Glass ";
            clicker.CosmeticsHat = "bad id!";
            clicker.CosmeticsBack = "wings_2";
            clicker.SkyPreset = "../x";
            Assert.Equal("moon-glass", clicker.CosmeticsCape);
            Assert.Equal("", clicker.CosmeticsHat);
            Assert.Equal("wings_2", clicker.CosmeticsBack);
            Assert.Equal("", clicker.SkyPreset);

            clicker.SkyTime = 99999;
            Assert.Equal(23999, clicker.SkyTime);
            clicker.SkyTime = -50;
            Assert.Equal(-2, clicker.SkyTime);
            clicker.SkyTime = -1;
            Assert.Equal(-1, clicker.SkyTime);
        }
        finally
        {
            clicker.CosmeticsCape = cape;
            clicker.CosmeticsHat = hat;
            clicker.CosmeticsBack = back;
            clicker.SkyPreset = sky;
            clicker.SkyTime = time;
        }
    }

    [Fact]
    public void Clicker_RaisesPropertyChangedForEachVisualsSetting()
    {
        Clicker clicker = Clicker.Instance;
        var raised = new List<string?>();
        void Handler(object? s, System.ComponentModel.PropertyChangedEventArgs e) => raised.Add(e.PropertyName);

        bool cosmetics = clicker.CosmeticsEnabled, skyOn = clicker.SkyEnabled, weather = clicker.SkyClearWeather;
        string cape = clicker.CosmeticsCape, hat = clicker.CosmeticsHat, back = clicker.CosmeticsBack, sky = clicker.SkyPreset;
        int time = clicker.SkyTime;
        clicker.PropertyChanged += Handler;
        try
        {
            clicker.CosmeticsEnabled = !cosmetics;
            clicker.CosmeticsCape = cape == "aa" ? "bb" : "aa";
            clicker.CosmeticsHat = hat == "aa" ? "bb" : "aa";
            clicker.CosmeticsBack = back == "aa" ? "bb" : "aa";
            clicker.SkyEnabled = !skyOn;
            clicker.SkyPreset = sky == "aa" ? "bb" : "aa";
            clicker.SkyTime = time == 6000 ? 12000 : 6000;
            clicker.SkyClearWeather = !weather;
        }
        finally
        {
            clicker.PropertyChanged -= Handler;
            clicker.CosmeticsEnabled = cosmetics;
            clicker.CosmeticsCape = cape;
            clicker.CosmeticsHat = hat;
            clicker.CosmeticsBack = back;
            clicker.SkyEnabled = skyOn;
            clicker.SkyPreset = sky;
            clicker.SkyTime = time;
            clicker.SkyClearWeather = weather;
        }

        Assert.Equal(
            new[]
            {
                nameof(Clicker.CosmeticsEnabled), nameof(Clicker.CosmeticsCape), nameof(Clicker.CosmeticsHat),
                nameof(Clicker.CosmeticsBack), nameof(Clicker.SkyEnabled), nameof(Clicker.SkyPreset),
                nameof(Clicker.SkyTime), nameof(Clicker.SkyClearWeather),
            },
            raised);
    }

    [Fact]
    public void Profile_Defaults()
    {
        var profile = new Profile();

        Assert.False(profile.CosmeticsEnabled);
        Assert.Equal("", profile.CosmeticsCape);
        Assert.Equal("", profile.CosmeticsHat);
        Assert.Equal("", profile.CosmeticsBack);
        Assert.False(profile.SkyEnabled);
        Assert.Equal("", profile.SkyPreset);
        Assert.Equal(-2, profile.SkyTime);
        Assert.False(profile.SkyClearWeather);
        Assert.Equal(0, profile.ModuleKeys["cosmetics"]);
        Assert.Equal(0, profile.ModuleKeys["sky"]);
    }

    [Fact]
    public void Profile_RoundTripsThroughClickerAndJson()
    {
        Clicker clicker = Clicker.Instance;
        Profile saved = ProfileManager.CreateFromClicker();
        try
        {
            clicker.CosmeticsEnabled = true;
            clicker.CosmeticsCape = "moon-glass";
            clicker.CosmeticsHat = "halo";
            clicker.CosmeticsBack = "wings";
            clicker.SkyEnabled = true;
            clicker.SkyPreset = "tsukihime";
            clicker.SkyTime = 18000;
            clicker.SkyClearWeather = true;

            string json = JsonSerializer.Serialize(ProfileManager.CreateFromClicker(), ProfileOptions);
            JsonNode node = JsonNode.Parse(json)!;
            Assert.True(node["cosmeticsEnabled"]!.GetValue<bool>());
            Assert.Equal("moon-glass", node["cosmeticsCape"]!.GetValue<string>());
            Assert.Equal(18000, node["skyTime"]!.GetValue<int>());

            clicker.CosmeticsEnabled = false;
            clicker.CosmeticsCape = "";
            clicker.CosmeticsHat = "";
            clicker.CosmeticsBack = "";
            clicker.SkyEnabled = false;
            clicker.SkyPreset = "";
            clicker.SkyTime = -2;
            clicker.SkyClearWeather = false;

            ProfileManager.ApplyToClicker(JsonSerializer.Deserialize<Profile>(json, ProfileOptions)!);

            Assert.True(clicker.CosmeticsEnabled);
            Assert.Equal("moon-glass", clicker.CosmeticsCape);
            Assert.Equal("halo", clicker.CosmeticsHat);
            Assert.Equal("wings", clicker.CosmeticsBack);
            Assert.True(clicker.SkyEnabled);
            Assert.Equal("tsukihime", clicker.SkyPreset);
            Assert.Equal(18000, clicker.SkyTime);
            Assert.True(clicker.SkyClearWeather);
        }
        finally
        {
            ProfileManager.ApplyToClicker(saved);
        }
    }

    [Fact]
    public void Profile_OldProfileWithoutVisualsKeysKeepsDefaults()
    {
        Profile? profile = JsonSerializer.Deserialize<Profile>("""{"name":"old","fastPlaceEnabled":true}""", ProfileOptions);

        Assert.NotNull(profile);
        Assert.True(profile!.FastPlaceEnabled);
        Assert.False(profile.CosmeticsEnabled);
        Assert.Equal("", profile.CosmeticsCape);
        Assert.Equal(-2, profile.SkyTime);
        Assert.False(profile.SkyClearWeather);
    }

    [Fact]
    public void Profile_NullIdsAndOutOfRangeTimeAreSanitisedOnApply()
    {
        Clicker clicker = Clicker.Instance;
        Profile saved = ProfileManager.CreateFromClicker();
        try
        {
            Profile? profile = JsonSerializer.Deserialize<Profile>(
                """{"cosmeticsCape":null,"cosmeticsHat":"Not Valid!","skyPreset":" NIGHT ","skyTime":1000000}""",
                ProfileOptions);

            ProfileManager.ApplyToClicker(profile!);

            Assert.Equal("", clicker.CosmeticsCape);
            Assert.Equal("", clicker.CosmeticsHat);
            Assert.Equal("night", clicker.SkyPreset);
            Assert.Equal(23999, clicker.SkyTime);
        }
        finally
        {
            ProfileManager.ApplyToClicker(saved);
        }
    }

    [Fact]
    public void ConfigPayload_SendsTheNineWireKeysFromTheMatchingClickerProperties()
    {
        string source = TestRepo.Read("Aoko", "Core", "GameStateClient.cs");
        int start = source.IndexOf("var config = new", StringComparison.Ordinal);
        int end = source.IndexOf("string json = JsonSerializer.Serialize(config)", start, StringComparison.Ordinal);
        string block = source[start..end];

        var expected = new Dictionary<string, string>
        {
            ["cosmeticsEnabled"] = "clicker.CosmeticsEnabled",
            ["cosmeticsCape"] = "clicker.CosmeticsCape",
            ["cosmeticsHat"] = "clicker.CosmeticsHat",
            ["cosmeticsBack"] = "clicker.CosmeticsBack",
            ["skyEnabled"] = "clicker.SkyEnabled",
            ["skyPreset"] = "clicker.SkyPreset",
            ["skyTime"] = "clicker.SkyTime",
            ["skyClearWeather"] = "clicker.SkyClearWeather",
            ["visualsDir"] = "VisualsPaths.Root",
        };

        Assert.Equal(SettingKeys.OrderBy(k => k), expected.Keys.OrderBy(k => k));
        foreach ((string key, string value) in expected)
            Assert.Matches($@"(?m)^\s*{Regex.Escape(key)}\s*=\s*{Regex.Escape(value)}\s*,", block);
    }

    [Fact]
    public void VisualsPaths_ResolvesUnderAppData()
    {
        Assert.Equal(
            Path.Combine("data", "Aoko", "visuals"),
            VisualsPaths.ResolveRoot("data"));
        Assert.True(Path.IsPathRooted(VisualsPaths.Root));
        Assert.EndsWith(Path.Combine("Aoko", "visuals"), VisualsPaths.Root);
    }

    [Theory]
    [InlineData("1.8.9")]
    [InlineData("26.1")]
    public void Capabilities_AdvertiseBothModulesTheNineSettingsAndTheStatusField(string version)
    {
        BridgeCapabilities caps = BridgeCapabilities.ForVersionFallback(version);

        Assert.True(caps.SupportsModule("cosmetics"));
        Assert.True(caps.SupportsModule("sky"));
        foreach (string key in SettingKeys)
            Assert.True(caps.SupportsSetting(key), key);
        Assert.True(caps.SupportsStateField("visualsStatus"));
    }

    [Fact]
    public void NativeCapabilityHeader_MatchesManagedListsForBothBridges()
    {
        string header = TestRepo.Read("McInjector", "src", "main", "cpp", "bridge_capabilities.h");

        foreach (string anchor in new[] { "LegacyCapabilitiesJson", "ModernCapabilitiesJson" })
        {
            string version = anchor.StartsWith("Legacy", StringComparison.Ordinal) ? "1.8.9" : "26.1";
            BridgeCapabilities managed = BridgeCapabilities.ForVersionFallback(version);
            int at = header.IndexOf(anchor, StringComparison.Ordinal);
            Assert.True(at >= 0, anchor);

            HashSet<string> modules = ListInside(header, at, "modules");
            HashSet<string> settings = ListInside(header, at, "settings");
            HashSet<string> state = ListInside(header, at, "state");

            Assert.Contains("cosmetics", modules);
            Assert.Contains("sky", modules);
            foreach (string key in SettingKeys)
                Assert.Contains(key.ToLowerInvariant(), settings);
            Assert.Contains("visualsstatus", state);

            // Native and managed copies must agree on every id this feature added.
            foreach (string key in SettingKeys.Select(k => k.ToLowerInvariant()))
                Assert.Equal(managed.SupportsSetting(key), settings.Contains(key));
            Assert.Equal(managed.SupportsStateField("visualsStatus"), state.Contains("visualsstatus"));
        }
    }

    [Theory]
    [InlineData("""{"visualsStatus":"error:no cape texture"}""", "error:no cape texture")]
    [InlineData("""{"visualsStatus":"ok"}""", "ok")]
    public void FullState_ParsesVisualsStatus(string json, string expected)
    {
        GameState? state = JsonSerializer.Deserialize<GameState>(json);

        Assert.Equal(expected, state!.VisualsStatus);
    }

    [Fact]
    public void GameState_VisualsStatusDefaultsToOff()
    {
        Assert.Equal("off", new GameState().VisualsStatus);
        Assert.Equal("off", JsonSerializer.Deserialize<GameState>("{}")!.VisualsStatus);
    }

    [Fact]
    public void Patch_UpdatesVisualsStatus_AndKeepsItWhenAbsent()
    {
        var previous = new GameState { VisualsStatus = "ok" };

        GameState kept = GameStatePatchMerger.Apply(
            previous, JsonNode.Parse("""{"type":"statePatch","inWorld":true}""")!.AsObject());
        GameState changed = GameStatePatchMerger.Apply(
            previous, JsonNode.Parse("""{"type":"statePatch","visualsStatus":"partial:skybox ignored"}""")!.AsObject());
        GameState ignoredBadType = GameStatePatchMerger.Apply(
            previous, JsonNode.Parse("""{"type":"statePatch","visualsStatus":12}""")!.AsObject());

        Assert.Equal("ok", kept.VisualsStatus);
        Assert.Equal("partial:skybox ignored", changed.VisualsStatus);
        Assert.Equal("ok", ignoredBadType.VisualsStatus);
    }

    [Fact]
    public void SearchIndex_ListsBothModulesInTheBetaTab()
    {
        foreach (string id in new[] { "cosmetics", "sky" })
        {
            ModuleSearchEntry entry = ModuleSearchIndex.Default.Entries.Single(e => e.Id == id);
            Assert.Equal("Beta", entry.Tab);
            Assert.True(entry.HasKeybind);
        }
    }

    private static HashSet<string> ListInside(string header, int from, string list)
    {
        string marker = "\\\"" + list + "\\\":[";
        int start = header.IndexOf(marker, from, StringComparison.Ordinal) + marker.Length;
        int end = header.IndexOf(']', start);
        return Regex.Matches(header[start..end], "\\\\\"(?<id>[^\\\\\"]+)\\\\\"")
            .Select(m => m.Groups["id"].Value)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
    }
}
