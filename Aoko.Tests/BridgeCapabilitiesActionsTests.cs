using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

/// <summary>
/// The "actions" capability list gates the Accounts/Servers commands. Actions are
/// deliberately their own list: never modules, settings, or state fields.
/// </summary>
public sealed class BridgeCapabilitiesActionsTests
{
    [Fact]
    public void FromPayload_ParsesActionsCaseInsensitively()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("26.2");
        var payload = new JsonObject
        {
            ["modules"] = new JsonArray("AUTOCLICKER"),
            ["actions"] = new JsonArray(
                "account.getSession",
                "Account.SetSession",
                "ACCOUNT.RESTORESESSION",
                "server.join")
        };

        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(payload, fallback);

        Assert.Equal(4, parsed.ActionCount);
        Assert.True(parsed.SupportsAction("account.getSession"));
        Assert.True(parsed.SupportsAction("ACCOUNT.SETSESSION"));
        Assert.True(parsed.SupportsAction(" account.restoreSession "));
        Assert.True(parsed.SupportsAction("server.join"));
        Assert.False(parsed.SupportsAction("cosmetics.patch"));
        Assert.False(parsed.SupportsAction(""));
        Assert.Equal(4, parsed.Actions.Count);
    }

    [Fact]
    public void FromPayload_ParsesOnlyTheActionsTheBridgeAdvertises()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("1.8.9");
        var payload = new JsonObject
        {
            ["state"] = new JsonArray("inworld"),
            ["actions"] = new JsonArray("server.join")
        };

        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(payload, fallback);

        Assert.True(parsed.SupportsAction("server.join"));
        Assert.False(parsed.SupportsAction("account.setSession"));
    }

    [Fact]
    public void FromPayload_WithoutActionsInheritsTheFallbackActions()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("26.2");
        var payload = new JsonObject
        {
            ["modules"] = new JsonArray("triggerbot"),
            ["settings"] = new JsonArray("mincps"),
            ["state"] = new JsonArray("actionbar")
        };

        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(payload, fallback);

        // Both shipped bridges advertise the four actions, so a frame without the list comes
        // from a bridge that predates it: reporting them unsupported would tell the user to
        // rebuild the DLLs for a command that works.
        Assert.Equal(fallback.ActionCount, parsed.ActionCount);
        Assert.True(parsed.SupportsAction(BridgeActions.AccountGetSession));
        Assert.True(parsed.SupportsAction(BridgeActions.AccountSetSession));
        Assert.True(parsed.SupportsAction(BridgeActions.AccountRestoreSession));
        Assert.True(parsed.SupportsAction(BridgeActions.ServerJoin));
        // Existing surfaces are untouched by the new list.
        Assert.True(parsed.SupportsModule("triggerbot"));
        Assert.True(parsed.SupportsSetting("mincps"));
        Assert.True(parsed.SupportsStateField("actionbar"));
    }

    [Fact]
    public void FromPayload_EmptyDocumentStillReturnsTheFallback()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("1.8.9");
        var payload = new JsonObject { ["type"] = "capabilities" };

        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(payload, fallback);

        Assert.Same(fallback, parsed);
        Assert.Equal(fallback.ActionCount, parsed.ActionCount);
        Assert.True(parsed.SupportsAction(BridgeActions.ServerJoin));
    }

    [Fact]
    public void FromPayload_EmptyDocumentKeepsTheFallbackWhenActionsPresent()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("1.8.9");
        var payload = new JsonObject
        {
            ["type"] = "capabilities",
            ["actions"] = new JsonArray("account.getSession")
        };

        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(payload, fallback);

        Assert.True(parsed.SupportsAction("account.getSession"));
        Assert.True(parsed.SupportsModule("autoclicker"));
        Assert.True(parsed.SupportsSetting("mincps"));
        Assert.True(parsed.SupportsStateField("holdingblock"));
    }

    [Theory]
    [InlineData("1.8.9")]
    [InlineData("1.21.4")]
    [InlineData("26.1")]
    [InlineData("26.2")]
    public void VersionFallback_AdvertisesTheShippedActions(string version)
    {
        // Both bridge_capabilities.h sets list these four, and GameStateClient publishes the
        // fallback between connect and the capabilities frame: an empty list there made
        // ServersView report "rebuild the bridge DLLs" for a working join action.
        BridgeCapabilities caps = BridgeCapabilities.ForVersionFallback(version);

        Assert.Equal(4, caps.ActionCount);
        Assert.True(caps.SupportsAction(BridgeActions.AccountGetSession));
        Assert.True(caps.SupportsAction(BridgeActions.AccountSetSession));
        Assert.True(caps.SupportsAction(BridgeActions.AccountRestoreSession));
        Assert.True(caps.SupportsAction(BridgeActions.ServerJoin));
        Assert.False(caps.SupportsAction("cosmetics.patch"));
    }

    [Fact]
    public void Actions_AreNotModulesSettingsOrStateFields()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("26.2");
        var payload = new JsonObject
        {
            ["modules"] = new JsonArray("autoclicker"),
            ["settings"] = new JsonArray("mincps"),
            ["state"] = new JsonArray("inworld"),
            ["actions"] = new JsonArray(
                "account.getSession",
                "account.setSession",
                "account.restoreSession",
                "server.join")
        };

        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(payload, fallback);

        Assert.True(parsed.SupportsAction("account.setSession"));
        Assert.False(parsed.SupportsModule("account.setSession"));
        Assert.False(parsed.SupportsSetting("account.setSession"));
        Assert.False(parsed.SupportsStateField("account.setSession"));
        Assert.False(parsed.SupportsModule("server.join"));
        Assert.Equal(1, parsed.ModuleCount);
        Assert.Equal(1, parsed.SettingCount);
        Assert.Equal(1, parsed.StateFieldCount);
        Assert.Equal(4, parsed.ActionCount);
    }

    [Fact]
    public void FromPayload_EmptyActionsArrayAlsoInheritsTheFallback()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("26.2");
        var payload = new JsonObject
        {
            ["modules"] = new JsonArray("triggerbot"),
            ["actions"] = new JsonArray()
        };

        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(payload, fallback);

        // An empty array parses the same as an absent one, so it inherits too. A bridge that
        // genuinely supports nothing advertises nothing anywhere, and both shipped bridges
        // list the four actions.
        Assert.Equal(fallback.ActionCount, parsed.ActionCount);
        Assert.True(parsed.SupportsAction(BridgeActions.AccountGetSession));
        Assert.True(parsed.SupportsModule("triggerbot"));
        Assert.True(parsed.SupportsSetting("mincps"));
    }

    [Fact]
    public void FromPayload_AbsentActionsInheritTheVersionFallback()
    {
        BridgeCapabilities modern = BridgeCapabilities.ForVersionFallback("26.2");
        BridgeCapabilities legacy = BridgeCapabilities.ForVersionFallback("1.8.9");

        BridgeCapabilities fromModern = BridgeCapabilities.FromPayload(
            JsonNode.Parse("""{"type":"capabilities","modules":["TRIGGERBOT"]}"""),
            modern);
        BridgeCapabilities fromLegacy = BridgeCapabilities.FromPayload(
            JsonNode.Parse("""{"type":"capabilities","state":["ACTIONBAR"]}"""),
            legacy);

        Assert.Equal(modern.ActionCount, fromModern.ActionCount);
        Assert.Equal(legacy.ActionCount, fromLegacy.ActionCount);
        Assert.True(fromModern.SupportsAction(BridgeActions.ServerJoin));
        Assert.True(fromLegacy.SupportsAction(BridgeActions.AccountGetSession));
    }

    [Fact]
    public void FromPayload_ReadsTheNativeCompactPayload()
    {
        // Exactly the single-line shape the native capabilities writer emits:
        // "state" array then "actions", no whitespace.
        const string nativePayload =
            "{\"type\":\"capabilities\",\"modules\":[\"AUTOCLICKER\"]," +
            "\"settings\":[\"mincps\"],\"state\":[\"inworld\"]," +
            "\"actions\":[\"account.getSession\",\"account.setSession\"," +
            "\"account.restoreSession\",\"server.join\"]}";

        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("26.2");
        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(
            JsonNode.Parse(nativePayload),
            fallback);

        Assert.Equal(4, parsed.ActionCount);
        Assert.True(parsed.SupportsAction("account.getSession"));
        Assert.True(parsed.SupportsAction("account.setSession"));
        Assert.True(parsed.SupportsAction("account.restoreSession"));
        Assert.True(parsed.SupportsAction("server.join"));
        Assert.True(parsed.SupportsModule("autoclicker"));
        Assert.False(parsed.SupportsModule("account.setSession"));
    }

    [Fact]
    public void NativeCapabilitiesHeader_AdvertisesExactlyTheActionsWeParse()
    {
        string header = TestRepo.Read("McInjector", "src", "main", "cpp", "bridge_capabilities.h");

        var fragments = new List<string>();
        int offset = 0;
        while (true)
        {
            int start = header.IndexOf("\\\"actions\\\":[", offset, StringComparison.Ordinal);
            if (start < 0)
                break;

            int end = header.IndexOf("]", start, StringComparison.Ordinal);
            fragments.Add(header[start..(end + 1)]);
            offset = end;
        }

        Assert.Equal(2, fragments.Count);
        foreach (string fragment in fragments)
        {
            // Compact literal style: no padding after ':' or ','.
            Assert.DoesNotContain(": ", fragment, StringComparison.Ordinal);
            Assert.DoesNotContain(", ", fragment, StringComparison.Ordinal);

            string json = "{" + fragment.Replace("\\\"", "\"", StringComparison.Ordinal) + "}";
            BridgeCapabilities parsed = BridgeCapabilities.FromPayload(
                JsonNode.Parse(json),
                BridgeCapabilities.ForVersionFallback("1.8.9"));

            Assert.True(parsed.SupportsAction(BridgeActions.AccountGetSession), json);
            Assert.True(parsed.SupportsAction(BridgeActions.AccountSetSession), json);
            Assert.True(parsed.SupportsAction(BridgeActions.AccountRestoreSession), json);
            Assert.True(parsed.SupportsAction(BridgeActions.ServerJoin), json);
            // Extending the native list means extending BridgeActions too.
            Assert.Equal(4, parsed.ActionCount);
        }
    }

    [Fact]
    public void Actions_SurviveATypeOnlyPayloadAlongsideFallbackSurfaces()
    {
        BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback("26.2");
        BridgeCapabilities parsed = BridgeCapabilities.FromPayload(
            JsonNode.Parse("""
                {
                  "type": "capabilities",
                  "actions": ["ACCOUNT.GETSESSION", "future.command"]
                }
                """),
            fallback);

        Assert.True(parsed.SupportsAction("account.getsession"));
        Assert.True(parsed.SupportsAction("future.command"));
        Assert.True(parsed.SupportsModule("triggerbot"));
    }
}
