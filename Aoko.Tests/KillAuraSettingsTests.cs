using System.Text.Json;
using Aoko.Core;

namespace Aoko.Tests;

public class KillAuraSettingsTests
{
    [Fact]
    public void Defaults_MatchOpenMyauPlusFc7c95d()
    {
        var settings = new KillAuraSettings();

        Assert.Equal("normal", settings.CpsMode);
        Assert.Equal("switch", settings.Mode);
        Assert.Equal("health", settings.Sort);
        Assert.Equal("hypixel", settings.AutoBlock);
        Assert.Equal(0, settings.AttackTick);
        Assert.Equal(8.0f, settings.AutoBlockCps);
        Assert.Equal(6.0f, settings.AutoBlockRange);
        Assert.Equal(3.5f, settings.SwingRange);
        Assert.Equal(3.0f, settings.AttackRange);
        Assert.Equal(360, settings.Fov);
        Assert.Equal(14, settings.MinCps);
        Assert.Equal(14, settings.MaxCps);
        Assert.Equal(150, settings.SwitchDelay);
        Assert.Equal("silent", settings.Rotations);
        Assert.Equal("silent", settings.MoveFix);
        Assert.True(settings.Players);
        Assert.True(settings.Teams);
        Assert.False(settings.Mobs);
    }

    [Fact]
    public void Profile_RoundTrip_PreservesNestedSettings()
    {
        var profile = new Profile();
        profile.KillAuraSettings.AutoBlock = "morden";
        profile.KillAuraSettings.Rotations = "liquidbounce";
        profile.KillAuraSettings.AttackRange = 4.2f;
        profile.KillAuraSettings.Bosses = true;

        string json = JsonSerializer.Serialize(profile);
        Profile? restored = JsonSerializer.Deserialize<Profile>(json);

        Assert.NotNull(restored);
        Assert.Equal("morden", restored!.KillAuraSettings.AutoBlock);
        Assert.Equal("liquidbounce", restored.KillAuraSettings.Rotations);
        Assert.Equal(4.2f, restored.KillAuraSettings.AttackRange);
        Assert.True(restored.KillAuraSettings.Bosses);
    }

    [Fact]
    public void Clone_IsIndependentAndRetainsConditionalState()
    {
        var original = new KillAuraSettings { Rotations = "hypixel", RavenPredictTicks = 4 };
        KillAuraSettings clone = original.Clone();
        clone.RavenPredictTicks = 1;

        Assert.True(original.IsHypixelRotation);
        Assert.Equal(4, original.RavenPredictTicks);
        Assert.Equal(1, clone.RavenPredictTicks);
    }

    [Fact]
    public void GrokRotation_IsAcceptedAndExposesSkew()
    {
        var settings = new KillAuraSettings { Rotations = "grok", GrokMaxSkew = 15f };

        Assert.Equal("grok", settings.Rotations);
        Assert.True(settings.IsGrokRotation);
        Assert.False(settings.IsHypixelRotation);
        Assert.Equal(15f, settings.GrokMaxSkew);

        settings.GrokMaxSkew = 100f;
        Assert.Equal(25f, settings.GrokMaxSkew);
        settings.GrokMaxSkew = 1f;
        Assert.Equal(6f, settings.GrokMaxSkew);

        settings.Rotations = "not-a-mode";
        Assert.Equal("none", settings.Rotations);
    }

    [Fact]
    public void GameState_DeserializesDynamicUnavailableReason()
    {
        GameState? state = JsonSerializer.Deserialize<GameState>(
            "{\"killAuraUnavailableReason\":\"Held sword does not support native blocking.\"}");

        Assert.Equal("Held sword does not support native blocking.", state!.KillAuraUnavailableReason);
    }
}
