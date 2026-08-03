using System.Text.Json;
using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

/// <summary>
/// Representative V1 messages captured from the two bridge families.  These
/// fixtures intentionally include optional fields and an unknown future field
/// so a loader/bridge version skew cannot silently become a protocol break.
/// </summary>
public sealed class ProtocolCompatibilityTests
{
    private const string LegacyStateFixture = """
        {
          "type": "state",
          "mapped": true,
          "inWorld": true,
          "guiOpen": true,
          "screenName": "GuiChest",
          "actionBar": "\u00a7aReady",
          "health": 19.5,
          "posX": 12.25,
          "posY": 64.0,
          "posZ": -3.75,
          "pitch": 8.0,
          "holdingBlock": true,
          "lookingAtBlock": true,
          "lookingAtEntity": false,
          "lookingAtEntityLatched": false,
          "breakingBlock": false,
          "attackCooldown": 0.8,
          "attackCooldownPerTick": 0.08,
          "killAuraUnavailableReason": "",
          "killAuraHasTarget": false,
          "killAuraBlocking": false,
          "stateMs": 123456,
          "chestStealerState": {
            "ready": true,
            "physical": true,
            "windowId": 4,
            "screenWidth": 854,
            "screenHeight": 480,
            "slots": [
              { "index": 4, "slotNumber": 4, "x": 381, "y": 128 }
            ]
          },
          "pixelPartyTargetFound": false,
          "pixelPartyTargetYaw": 0.0,
          "pixelPartyTargetDist": -1.0,
          "pixelPartyYawDelta": 0.0,
          "entities": [
            { "sx": 401.5, "sy": 219.0, "dist": 3.25, "name": "Steve", "hp": 20.0,
              "x": 14.0, "y": 64.0, "z": -5.0 }
          ]
        }
        """;

    private const string ModernStateFixture = """
        {
          "type": "state",
          "inWorld": true,
          "guiOpen": false,
          "screenName": "",
          "actionBar": "",
          "health": 20.0,
          "posX": 0.0,
          "posY": 0.0,
          "posZ": 0.0,
          "fov": 70.0,
          "viewportWidth": 1920,
          "viewportHeight": 1080,
          "holdingBlock": false,
          "lookingAtBlock": false,
          "lookingAtEntity": true,
          "lookingAtEntityLatched": true,
          "breakingBlock": false,
          "stateMs": 987654,
          "attackCooldown": 1.0,
          "attackCooldownPerTick": 0.08,
          "killAuraUnavailableReason": "",
          "killAuraHasTarget": true,
          "killAuraBlocking": false,
          "chestStealerState": null,
          "pixelPartyTargetFound": true,
          "pixelPartyTargetYaw": 181.25,
          "pixelPartyTargetDist": 6.5,
          "pixelPartyYawDelta": -12.5,
          "entities": [
            { "sx": 960.0, "sy": 540.0, "dist": 3.0, "name": "Alex", "hp": 18.0 }
          ],
          "futureStateField": { "schema": 2 }
        }
        """;

    [Fact]
    public void LegacyBridgeFixture_DeserializesCompleteV1State()
    {
        GameState? state = JsonSerializer.Deserialize<GameState>(LegacyStateFixture);

        Assert.NotNull(state);
        Assert.True(state!.Mapped);
        Assert.True(state.InWorld);
        Assert.True(state.GuiOpen);
        Assert.Equal("GuiChest", state.ScreenName);
        Assert.Equal(19.5f, state.Health);
        Assert.Equal(12.25, state.PosX);
        Assert.True(state.HoldingBlock);
        Assert.Equal((ulong)123456, state.StateMs);
        Assert.NotNull(state.ChestStealerState);
        Assert.Single(state.ChestStealerState!.Slots);
        Assert.Single(state.Entities);
        Assert.Equal("Steve", state.Entities[0].Name);
        Assert.Equal(-5.0, state.Entities[0].Z);
    }

    [Fact]
    public void ModernBridgeFixture_DeserializesAndIgnoresUnknownFutureFields()
    {
        GameState? state = JsonSerializer.Deserialize<GameState>(ModernStateFixture);

        Assert.NotNull(state);
        Assert.True(state!.InWorld);
        Assert.False(state.GuiOpen);
        Assert.Equal(1920, state.ViewportWidth);
        Assert.Equal(1080, state.ViewportHeight);
        Assert.True(state.LookingAtEntityLatched);
        Assert.True(state.KillAuraHasTarget);
        Assert.True(state.PixelPartyTargetFound);
        Assert.Equal(-12.5f, state.PixelPartyYawDelta);
        Assert.Null(state.ChestStealerState);
        Assert.Single(state.Entities);
        Assert.Equal("Alex", state.Entities[0].Name);
    }

    [Fact]
    public void LoaderNewerBridgeOlder_AbsentOptionalFieldsUseSafeDefaults()
    {
        const string oldBridgeMessage = """
            { "type": "state", "mapped": true, "inWorld": false, "guiOpen": false,
              "screenName": "unknown", "actionBar": "", "health": -1, "entities": [] }
            """;

        GameState? state = JsonSerializer.Deserialize<GameState>(oldBridgeMessage);

        Assert.NotNull(state);
        Assert.False(state!.InWorld);
        Assert.Equal(-1f, state.Health);
        Assert.Equal(70.0f, state.Fov);
        Assert.Equal(1.0f, state.AttackCooldown);
        Assert.Equal(0.08f, state.AttackCooldownPerTick);
        Assert.False(state.LookingAtEntityLatched);
        Assert.Empty(state.Entities);
    }

    [Fact]
    public void BridgeNewerLoaderOlder_UnknownCapabilityEntriesRemainTolerated()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("26.2");
        var payload = JsonNode.Parse("""
            {
              "type": "capabilities",
              "modules": ["TRIGGERBOT", "future_module"],
              "settings": ["AimAssistFov", "future_setting"],
              "state": ["ActionBar", "future_state_field"],
              "futureCapabilityBlock": { "version": 2 }
            }
            """);

        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(payload, fallback);

        Assert.True(parsed.SupportsModule("triggerbot"));
        Assert.True(parsed.SupportsSetting("aimassistfov"));
        Assert.True(parsed.SupportsStateField("actionbar"));
        Assert.True(parsed.SupportsModule("future_module"));
        Assert.True(parsed.SupportsSetting("future_setting"));
        Assert.True(parsed.SupportsStateField("future_state_field"));
        // An omitted array retains the known fallback surface.
        var sparsePayload = JsonNode.Parse("""
            { "type": "capabilities", "modules": ["triggerbot"] }
            """);
        BridgeCapabilities sparse = BridgeCapabilities.FromPayload(sparsePayload, fallback);
        Assert.True(sparse.SupportsSetting("mincps"));
        Assert.True(sparse.SupportsStateField("inworld"));
    }

    [Fact]
    public void CapabilityFallbacksExposePublishedStateAndHeldItemSettings()
    {
        BridgeCapabilities modern = BridgeCapabilities.ForVersionFallback("26.2");
        BridgeCapabilities legacy = BridgeCapabilities.ForVersionFallback("1.8.9");

        Assert.True(modern.SupportsStateField("holdingblock"));
        Assert.True(modern.SupportsStateField("lookingatblock"));
        Assert.True(modern.SupportsStateField("fov"));
        Assert.True(modern.SupportsStateField("viewportwidth"));
        Assert.True(modern.SupportsStateField("viewportheight"));
        Assert.True(modern.SupportsSetting("autototembehaviormode"));
        Assert.True(legacy.SupportsSetting("nametagshowhelditem"));
    }
}
