using System.Text.Json;
using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

public class GameStateAndProfileTests
{
    [Fact]
    public void GameState_DeserializesExpectedContract()
    {
        const string json = """
            {
              "mapped": true,
              "guiOpen": false,
              "screenName": "HUD",
              "actionBar": "The theme is C_T",
              "health": 18.5,
              "viewportWidth": 1920,
              "viewportHeight": 1001,
              "entities": [
                { "sx": 120.5, "sy": 44.2, "dist": 3.25, "name": "Steve", "hp": 20.0 }
              ]
            }
            """;

        GameState? state = JsonSerializer.Deserialize<GameState>(json);

        Assert.NotNull(state);
        Assert.True(state.Mapped);
        Assert.False(state.GuiOpen);
        Assert.Equal("HUD", state.ScreenName);
        Assert.Equal("The theme is C_T", state.ActionBar);
        Assert.Equal(18.5f, state.Health);
        Assert.Equal(1920, state.ViewportWidth);
        Assert.Equal(1001, state.ViewportHeight);
        Assert.Single(state.Entities);
        Assert.Equal("Steve", state.Entities[0].Name);
        Assert.Equal(3.25, state.Entities[0].Distance, 3);
    }

    [Fact]
    public void GameState_DeserializesChestStealerState()
    {
        const string json = """
            {
              "mapped": true,
              "guiOpen": true,
              "screenName": "GuiChest",
              "chestStealerState": {
                "ready": true,
                "physical": true,
                "windowId": 2,
                "screenWidth": 854,
                "screenHeight": 480,
                "slots": [
                  { "index": 4, "slotNumber": 4, "x": 381, "y": 128 }
                ]
              },
              "entities": []
            }
            """;

        GameState? state = JsonSerializer.Deserialize<GameState>(json);

        Assert.NotNull(state);
        Assert.NotNull(state!.ChestStealerState);
        Assert.True(state.ChestStealerState!.Ready);
        Assert.True(state.ChestStealerState.Physical);
        Assert.Equal(2, state.ChestStealerState.WindowId);
        Assert.Single(state.ChestStealerState.Slots);
        Assert.Equal(381, state.ChestStealerState.Slots[0].X);
    }

    [Fact]
    public void ChestStealerCoordinateMapper_ScalesToClientRect()
    {
        var state = new ChestStealerState { ScreenWidth = 854, ScreenHeight = 480 };
        var slot = new ChestStealerSlot { X = 427, Y = 240 };
        var clientRect = new WindowDetection.RECT { Left = 100, Top = 50, Right = 1808, Bottom = 1010 };

        bool mapped = ChestStealerCoordinateMapper.TryMapScaledPoint(state, slot, clientRect, out int x, out int y);

        Assert.True(mapped);
        Assert.Equal(954, x);
        Assert.Equal(530, y);
    }

    [Fact]
    public void Profile_DefaultValuesRemainStable()
    {
        var profile = new Profile();

        Assert.Equal("Default", profile.Name);
        Assert.Equal(8.0f, profile.MinCPS);
        Assert.Equal(12.0f, profile.MaxCPS);
        Assert.True(profile.LeftClickEnabled);
        Assert.False(profile.TriggerbotEnabled);
        Assert.False(profile.KillAuraEnabled);
        Assert.Equal(2.9f, profile.KillAuraRange);
        Assert.Equal(3.5f, profile.KillAuraAimRange);
        Assert.Equal(10.0f, profile.KillAuraMinTurnSpeed);
        Assert.Equal(20.0f, profile.KillAuraMaxTurnSpeed);
        Assert.Equal("distance", profile.KillAuraTargetMode);
        Assert.Equal(350, profile.KillAuraSwitchDelayMs);
        Assert.Equal(18, profile.KillAuraRandomization);
        Assert.Equal("cooldown", profile.KillAuraClickMode);
        Assert.Equal(10.0f, profile.KillAuraMinCps);
        Assert.Equal(14.0f, profile.KillAuraMaxCps);
        Assert.Equal(120, profile.KillAuraFov);
        Assert.Equal("legit", profile.KillAuraRotMode);
        Assert.False(profile.KillAuraRequirePress);
        Assert.True(profile.KillAuraWeaponsOnly);
        Assert.False(profile.ChestStealerEnabled);
        Assert.Equal(120, profile.ChestStealerDelayMs);
        Assert.Equal(100, profile.ReachChance);
        Assert.True(profile.ModuleKeys.ContainsKey("autoclicker"));
        Assert.True(profile.ModuleKeys.ContainsKey("cheststealer"));
    }

    [Fact]
    public void Profile_CamelCaseSerialization_UsesExpectedKeys()
    {
        var profile = new Profile
        {
            Name = "PvP",
            MinCPS = 9.5f,
            TriggerbotEnabled = true,
            KillAuraEnabled = true
        };

        var options = new JsonSerializerOptions
        {
            PropertyNamingPolicy = JsonNamingPolicy.CamelCase
        };

        string json = JsonSerializer.Serialize(profile, options);
        JsonNode? node = JsonNode.Parse(json);
        string? minCpsKey = node?.AsObject().Select(kvp => kvp.Key)
            .FirstOrDefault(k => string.Equals(k, "minCPS", StringComparison.OrdinalIgnoreCase));

        Assert.NotNull(node);
        Assert.NotNull(minCpsKey);
        Assert.Equal("PvP", node!["name"]?.GetValue<string>());
        Assert.Equal(9.5, node[minCpsKey!]!.GetValue<double>(), 3);
        Assert.True(node["triggerbotEnabled"]!.GetValue<bool>());
        Assert.True(node["killAuraEnabled"]!.GetValue<bool>());
    }

    [Fact]
    public void Profile_AntiDebuff_DefaultsFalse()
    {
        var profile = new Profile();

        Assert.False(profile.AntiDebuffEnabled);
    }

    [Fact]
    public void Profile_AntiDebuff_SerializesRoundTrip()
    {
        var profile = new Profile { AntiDebuffEnabled = true };
        var options = new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };

        string json = JsonSerializer.Serialize(profile, options);
        JsonNode? node = JsonNode.Parse(json);
        Profile? roundTripped = JsonSerializer.Deserialize<Profile>(json, options);

        Assert.True(node!["antiDebuffEnabled"]!.GetValue<bool>());
        Assert.NotNull(roundTripped);
        Assert.True(roundTripped!.AntiDebuffEnabled);
    }

    [Fact]
    public void ProfileManager_UsesAokoProfileDirectory()
    {
        string appData = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N"));

        string profilesDir = ProfileManager.GetProfilesDirectory(appData);

        Assert.Equal(Path.Combine(appData, "Aoko", "profiles"), profilesDir);
    }

    [Fact]
    public void ProfileManager_InitializesByCopyingLegacyProfilesWhenNewFolderIsEmpty()
    {
        string appData = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N"));

        try
        {
            string legacyDir = ProfileManager.GetLegacyProfilesDirectory(appData);
            Directory.CreateDirectory(legacyDir);
            File.WriteAllText(Path.Combine(legacyDir, "config.json"), """{"name":"config"}""");

            string profilesDir = ProfileManager.InitializeProfileDirectory(appData);

            Assert.True(File.Exists(Path.Combine(profilesDir, "config.json")));
            Assert.True(File.Exists(Path.Combine(legacyDir, "config.json")));
        }
        finally
        {
            if (Directory.Exists(appData))
                Directory.Delete(appData, recursive: true);
        }
    }

    [Fact]
    public void ProfileManager_DoesNotOverwriteExistingAokoProfiles()
    {
        string appData = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N"));

        try
        {
            string profilesDir = ProfileManager.GetProfilesDirectory(appData);
            string legacyDir = ProfileManager.GetLegacyProfilesDirectory(appData);
            Directory.CreateDirectory(profilesDir);
            Directory.CreateDirectory(legacyDir);
            File.WriteAllText(Path.Combine(profilesDir, "config.json"), """{"name":"new"}""");
            File.WriteAllText(Path.Combine(legacyDir, "config.json"), """{"name":"legacy"}""");

            ProfileManager.InitializeProfileDirectory(appData);

            string json = File.ReadAllText(Path.Combine(profilesDir, "config.json"));
            Assert.Contains("\"new\"", json);
        }
        finally
        {
            if (Directory.Exists(appData))
                Directory.Delete(appData, recursive: true);
        }
    }
}
