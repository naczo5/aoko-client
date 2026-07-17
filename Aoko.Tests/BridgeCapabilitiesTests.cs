using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

public class BridgeCapabilitiesTests
{
    [Fact]
    public void ModernFallback_ExposesModernFeaturesFor262()
    {
        BridgeCapabilities caps = BridgeCapabilities.ForVersionFallback("26.2");

        Assert.True(caps.SupportsModule("triggerbot"));
        Assert.True(caps.SupportsModule("silentaura"));
        Assert.True(caps.SupportsModule("nametags"));
        Assert.True(caps.SupportsModule("nickhider"));
        Assert.True(caps.SupportsSetting("nickhiderenabled"));
    }

    [Fact]
    public void ModernFallback_ExposesModernFeatures()
    {
        BridgeCapabilities caps = BridgeCapabilities.ForVersionFallback("26.1");

        Assert.True(caps.SupportsModule("triggerbot"));
        Assert.True(caps.SupportsModule("pixelpartyassist"));
        Assert.True(caps.SupportsModule("nickhider"));
        Assert.True(caps.SupportsSetting("nickhideralias"));
        Assert.True(caps.SupportsModule("cheststealer"));
        Assert.True(caps.SupportsSetting("pixelpartyassist"));
        Assert.True(caps.SupportsStateField("pixelpartyyawdelta"));
        Assert.True(caps.SupportsSetting("cheststealerenabled"));
        Assert.True(caps.SupportsSetting("silentarange"));
        Assert.True(caps.SupportsSetting("silentaurarotspeed"));
        Assert.True(caps.SupportsSetting("silentauraaimrange"));
        Assert.True(caps.SupportsSetting("silentauratargetmode"));
        Assert.True(caps.SupportsSetting("silentauraswitchdelayms"));
        Assert.True(caps.SupportsSetting("silentauraaccuracy"));
        Assert.True(caps.SupportsSetting("silentauraspammode"));
        Assert.True(caps.SupportsSetting("silentauraspammincps"));
        Assert.True(caps.SupportsSetting("silentauraspammaxcps"));
        Assert.True(caps.SupportsStateField("cheststealerstate"));
        Assert.True(caps.SupportsSetting("nametagshowhelditem"));
        Assert.True(caps.SupportsStateField("attackcooldown"));
    }

    [Fact]
    public void LegacyFallback_ExposesPixelPartyAssist()
    {
        BridgeCapabilities caps = BridgeCapabilities.ForVersionFallback("1.8.9");

        Assert.True(caps.SupportsModule("pixelpartyassist"));
        Assert.True(caps.SupportsSetting("pixelpartyautowalk"));
        Assert.True(caps.SupportsStateField("pixelpartyyawdelta"));
    }

    [Fact]
    public void LegacyFallback_DoesNotExposeTriggerbot()
    {
        BridgeCapabilities caps = BridgeCapabilities.ForVersionFallback("1.8.9");

        Assert.False(caps.SupportsModule("triggerbot"));
        Assert.False(caps.SupportsSetting("triggerbot"));
        Assert.True(caps.SupportsStateField("holdingblock"));
    }

    [Fact]
    public void LegacyFallback_ExposesChestStealer()
    {
        BridgeCapabilities caps = BridgeCapabilities.ForVersionFallback("1.8.9");

        Assert.True(caps.SupportsModule("cheststealer"));
        Assert.True(caps.SupportsSetting("cheststealerenabled"));
        Assert.True(caps.SupportsSetting("cheststealerdelayms"));
        Assert.True(caps.SupportsSetting("keybindcheststealer"));
        Assert.True(caps.SupportsStateField("cheststealerstate"));
    }

    [Fact]
    public void AntiDebuff_AvailableOnAllVersions()
    {
        BridgeCapabilities modern = BridgeCapabilities.ForVersionFallback("26.1");
        BridgeCapabilities legacy = BridgeCapabilities.ForVersionFallback("1.8.9");

        Assert.True(modern.SupportsModule("antidebuff"));
        Assert.True(modern.SupportsSetting("antidebuffenabled"));
        Assert.True(legacy.SupportsModule("antidebuff"));
        Assert.True(legacy.SupportsSetting("antidebuffenabled"));
    }

    [Fact]
    public void FromPayload_UsesFallbackWhenArraysMissing()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("1.8.9");
        var payload = new JsonObject { ["type"] = "capabilities" };

        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(payload, fallback);

        Assert.Equal(fallback.ModuleCount, parsed.ModuleCount);
        Assert.True(parsed.SupportsModule("autoclicker"));
        Assert.False(parsed.SupportsModule("triggerbot"));
    }

    [Fact]
    public void FromPayload_MergesAndNormalizesPayloadData()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("1.8.9");
        var payload = new JsonObject
        {
            ["modules"] = new JsonArray("TRIGGERBOT"),
            ["state"] = new JsonArray("ActionBar")
        };

        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(payload, fallback);

        Assert.True(parsed.SupportsModule("triggerbot"));
        Assert.True(parsed.SupportsStateField("actionbar"));
        Assert.True(parsed.SupportsSetting("mincps"));
    }
}
