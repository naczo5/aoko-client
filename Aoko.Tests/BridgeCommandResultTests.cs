using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

/// <summary>
/// Reply decoding and the user-facing sentence mapping for "commandResult".
/// </summary>
public sealed class BridgeCommandResultTests
{
    private const string SetSessionReply = """
        {
          "type": "commandResult",
          "requestId": 124,
          "ok": true,
          "action": "setSession",
          "username": "Notch",
          "uuid": "069a79f4-44e9-4726-a5be-fca90e38aaf5",
          "inWorld": false,
          "ready": true
        }
        """;

    [Fact]
    public void TryParse_ReadsTheContractReply()
    {
        bool parsed = BridgeCommandResult.TryParse(
            JsonNode.Parse(SetSessionReply),
            out int requestId,
            out BridgeCommandResult result);

        Assert.True(parsed);
        Assert.Equal(124, requestId);
        Assert.True(result.Ok);
        Assert.Equal("setSession", result.Action);
        Assert.Equal("Notch", result.Username);
        Assert.Equal("069a79f4-44e9-4726-a5be-fca90e38aaf5", result.Uuid);
        Assert.False(result.InWorld);
        Assert.True(result.Ready);
        Assert.Equal("", result.Error);
        Assert.Equal("Switched to Notch.", result.Message);
    }

    [Fact]
    public void TryParse_ReadsTheJoinFailureReply()
    {
        const string reply = """
            {"type":"commandResult","requestId":126,"ok":false,"action":"join","error":"connect_path_not_found"}
            """;

        bool parsed = BridgeCommandResult.TryParse(
            JsonNode.Parse(reply),
            out int requestId,
            out BridgeCommandResult result);

        Assert.True(parsed);
        Assert.Equal(126, requestId);
        Assert.False(result.Ok);
        Assert.Equal("join", result.Action);
        Assert.Equal("connect_path_not_found", result.Error);
        Assert.Equal(
            "This client's connect screen could not be resolved, so the join cannot be started.",
            result.Message);
    }

    [Fact]
    public void TryParse_MissingOptionalFieldsUseSafeDefaults()
    {
        const string reply = """{"type":"commandResult","requestId":9,"ok":true}""";

        bool parsed = BridgeCommandResult.TryParse(
            JsonNode.Parse(reply),
            out int requestId,
            out BridgeCommandResult result);

        Assert.True(parsed);
        Assert.Equal(9, requestId);
        Assert.True(result.Ok);
        Assert.Equal("", result.Action);
        Assert.Equal("", result.Username);
        Assert.Equal("", result.Uuid);
        Assert.Equal("", result.Error);
        Assert.False(result.InWorld);
        Assert.False(result.Ready);
        Assert.Equal("The game completed the command.", result.Message);
    }

    [Fact]
    public void TryParse_UnknownActionStillDecodes()
    {
        const string reply = """
            {"type":"commandResult","requestId":3,"ok":true,"action":"cosmetics.patch","ready":true}
            """;

        bool parsed = BridgeCommandResult.TryParse(
            JsonNode.Parse(reply),
            out _,
            out BridgeCommandResult result);

        Assert.True(parsed);
        Assert.Equal("cosmetics.patch", result.Action);
        Assert.Equal("The game completed the command.", result.Message);
    }

    [Theory]
    [InlineData("""{"type":"state","inWorld":true}""")]
    [InlineData("""{"requiredAction":"commandResult","requestId":1,"ok":true}""")]
    [InlineData("""{"type":"commandResult","ok":true}""")]
    [InlineData("""{"type":"commandResult","requestId":0,"ok":true}""")]
    [InlineData("""{"type":"commandResult","requestId":-4,"ok":true}""")]
    [InlineData("[]")]
    public void TryParse_RejectsNonCommandDocuments(string json)
    {
        bool parsed = BridgeCommandResult.TryParse(
            JsonNode.Parse(json),
            out int requestId,
            out BridgeCommandResult result);

        Assert.False(parsed);
        Assert.Equal(0, requestId);
        Assert.False(result.Ok);
    }

    [Fact]
    public void Failure_CarriesActionAndError()
    {
        BridgeCommandResult result = BridgeCommandResult.Failure(
            BridgeActions.Join,
            BridgeCommandResult.NotConnectedError);

        Assert.False(result.Ok);
        Assert.Equal("join", result.Action);
        Assert.Equal("not_connected", result.Error);
        Assert.Equal(
            "The GUI is not connected to the game yet. Inject first, then wait for the main menu.",
            result.Message);
    }

    [Fact]
    public void WithAction_FillsOnlyAMissingAction()
    {
        var missing = new BridgeCommandResult(ok: false, error: "in_world");
        var present = new BridgeCommandResult(ok: true, action: "join");

        Assert.Equal("setSession", missing.WithAction("setSession").Action);
        Assert.Equal("join", present.WithAction("setSession").Action);
    }

    [Theory]
    [InlineData("in_world", "Return to the main menu before switching accounts.")]
    [InlineData("game_thread_timeout", "The game did not answer in time. Stay on the main menu and retry.")]
    [InlineData("timeout", "The game did not answer in time. Stay on the main menu and retry.")]
    [InlineData("not_ready", "Wait until the client reaches the main menu, then retry.")]
    [InlineData("notready", "Wait until the client reaches the main menu, then retry.")]
    [InlineData("no_launch_session", "The client has no launch session to restore.")]
    [InlineData("invalid_address", "That server address is not valid.")]
    [InlineData("invalid_session_data", "The account details were rejected. Re-import the account and retry.")]
    [InlineData("session_write_not_observed", "The game did not apply the new session. Stay on the main menu and retry.")]
    [InlineData("connection_lost", "The connection to the game dropped before the command finished.")]
    [InlineData("send_failed", "The command could not be sent to the game.")]
    public void Message_MapsKnownErrorCodes(string error, string expected)
    {
        BridgeCommandResult result = BridgeCommandResult.Failure("join", error);

        Assert.Equal(expected, result.Message);
    }

    [Theory]
    [InlineData("Session helper not ready (wait on main menu, then retry)")]
    [InlineData("session helper NOT READY")]
    public void Message_MapsFreeTextNotReadyReasons(string error)
    {
        BridgeCommandResult result = BridgeCommandResult.Failure("setSession", error);

        Assert.Equal("Wait until the client reaches the main menu, then retry.", result.Message);
    }

    [Fact]
    public void Message_MapsFreeTextInWorldReasons()
    {
        BridgeCommandResult result = BridgeCommandResult.Failure(
            "setSession",
            "Disconnect from the world before switching");

        Assert.Equal("Return to the main menu before switching accounts.", result.Message);
    }

    [Fact]
    public void Message_UnknownCodeDoesNotEchoBridgeText()
    {
        // A future describeThrowable string can carry Java exception text; only vetted
        // sentences may reach the status bar. The raw code stays on Error for a local log.
        BridgeCommandResult result = BridgeCommandResult.Failure("join", "joinServer exception");

        Assert.Equal("joinServer exception", result.Error);
        Assert.DoesNotContain("joinServer exception", result.Message, StringComparison.Ordinal);
        Assert.Equal(
            "The game refused the request. Wait until the client reaches the main menu, then retry.",
            result.Message);
    }

    [Fact]
    public void Message_EmptyErrorStillExplainsTheMenuRequirement()
    {
        BridgeCommandResult result = new(ok: false);

        Assert.Contains("main menu", result.Message);
    }

    [Theory]
    [InlineData("getSession", "Notch", "Signed in as Notch.")]
    [InlineData("getSession", "", "No account is signed in right now.")]
    [InlineData("setSession", "Notch", "Switched to Notch.")]
    [InlineData("restoreSession", "Notch", "Restored Notch.")]
    [InlineData("join", "Notch", "Join started. The client is connecting to the server.")]
    public void Message_SuccessDependsOnAction(string action, string username, string expected)
    {
        var result = new BridgeCommandResult(ok: true, action: action, username: username);

        Assert.Equal(expected, result.Message);
    }
}
