using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

public class BridgeCapabilitiesTests
{
    [Fact]
    public void ModernFallback_ExposesModernFeatures()
    {
        BridgeCapabilities caps = BridgeCapabilities.ForVersionFallback("26.1");

        Assert.True(caps.SupportsModule("triggerbot"));
        Assert.True(caps.SupportsModule("pixelpartyassist"));
        Assert.True(caps.SupportsModule("cheststealer"));
        Assert.True(caps.SupportsSetting("pixelpartyassist"));
        Assert.True(caps.SupportsStateField("pixelpartyyawdelta"));
        Assert.True(caps.SupportsSetting("cheststealerenabled"));
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
