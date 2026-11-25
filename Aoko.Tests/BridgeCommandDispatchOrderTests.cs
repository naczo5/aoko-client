using System.Text.Json;
using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

/// <summary>
/// A "commandResult" document must never reach the GameState branch of the read loop: any
/// JSON object deserializes into an empty GameState, which blanks CurrentState while the
/// pending command still waits out its whole budget. The routing decision is therefore
/// asserted on the parsed document, not on the serialized text the bridge happens to use.
/// </summary>
public sealed class BridgeCommandDispatchOrderTests
{
    [Theory]
    [InlineData("{\"type\":\"commandResult\",\"requestId\":1,\"ok\":true}")]
    [InlineData("{\"type\" : \"commandResult\" , \"requestId\" : 1 , \"ok\" : true }")]
    [InlineData("{\"requestId\":1,\"ok\":true,\"type\":\"commandResult\"}")]
    [InlineData("{\"requestId\":1,\"type\":\"CommandResult\",\"ok\":true}")]
    [InlineData("{\"type\":\"commandResult\",\"requestId\":124,\"ok\":true,\"action\":\"setSession\",\"username\":\"Notch\"}")]
    [InlineData("{\n  \"type\": \"commandResult\",\n  \"requestId\": 7,\n  \"ok\": false,\n  \"error\": \"in_world\"\n}")]
    public void AReply_RoutesToTheReplyHandlerWhateverTheSerialization(string line)
    {
        JsonNode? reply = JsonNode.Parse(line);

        Assert.Equal(BridgeInboundKind.CommandResult, BridgeInboundClassifier.Classify(reply));
    }

    [Theory]
    [InlineData("{\"type\":\"capabilities\",\"modules\":[]}")]
    [InlineData("{\"type\" : \"capabilities\" }")]
    [InlineData("{\"modules\":[],\"type\":\"CAPABILITIES\"}")]
    public void ACapabilitiesFrame_RoutesToTheCapabilitiesHandler(string line)
    {
        JsonNode? frame = JsonNode.Parse(line);

        Assert.Equal(BridgeInboundKind.Capabilities, BridgeInboundClassifier.Classify(frame));
    }

    [Theory]
    [InlineData("{\"type\":\"state\",\"inWorld\":true}")]
    [InlineData("{\"inWorld\":true}")]
    [InlineData("{\"type\":42,\"inWorld\":true}")]
    [InlineData("{\"type\":{\"nested\":true}}")]
    [InlineData("[]")]
    [InlineData("null")]
    public void EverythingElse_RoutesToTheStateBranch(string line)
    {
        JsonNode? node = JsonNode.Parse(line);

        Assert.Equal(BridgeInboundKind.State, BridgeInboundClassifier.Classify(node));
    }

    [Fact]
    public async Task ASpacedReply_CompletesTheRequestAndNeverTouchesTheStateBranch()
    {
        int requestId = 0;
        var dispatcher = new BridgeCommandDispatcher(
            (line, _) =>
            {
                Assert.EndsWith(BridgeActions.LineTerminator, line, StringComparison.Ordinal);
                return Task.FromResult(true);
            },
            () => true);

        Task<BridgeCommandResult> pending = dispatcher.SendAsync(
            BridgeActions.SetSession,
            id =>
            {
                requestId = id;
                return "command\n";
            },
            CancellationToken.None);

        await WaitForPendingAsync(dispatcher);

        JsonNode? reply = JsonNode.Parse(
            $"{{ \"type\" : \"commandResult\" , \"requestId\" : {requestId} , \"ok\" : true , " +
            "\"action\" : \"setSession\" , \"username\" : \"Notch\" }");

        Assert.Equal(BridgeInboundKind.CommandResult, BridgeInboundClassifier.Classify(reply));
        Assert.True(RouteLikeTheReadLoop(reply, dispatcher, out bool statePublished));
        Assert.False(statePublished, "a reply must never reach the GameState branch");

        BridgeCommandResult result = await pending;

        Assert.True(result.Ok);
        Assert.Equal("Notch", result.Username);
        Assert.Equal("Switched to Notch.", result.Message);
        Assert.Equal(0, dispatcher.PendingCount);
    }

    [Fact]
    public void CommandResultDocument_WouldParseAsAnEmptyGameState()
    {
        const string reply = """
            {"type":"commandResult","requestId":124,"ok":true,"action":"setSession","username":"Notch","uuid":"u","inWorld":false,"ready":true}
            """;

        // This is exactly why the read loop must route replies away from the state branch.
        GameState? state = JsonSerializer.Deserialize<GameState>(reply);

        Assert.NotNull(state);
        Assert.False(state!.InWorld);
        Assert.False(state.GuiOpen);
        Assert.Equal("unknown", state.ScreenName);
        Assert.Equal(-1f, state.Health);
        Assert.Equal(0, state.LastActionId);
        Assert.Empty(state.Entities);
        Assert.Empty(state.ModuleStatus);
    }

    /// <summary>
    /// Mirrors the switch in <c>GameStateClient.ReadLoop</c>, which cannot run in this harness
    /// (it needs WPF). Returns true when the line was consumed as a reply or capabilities frame;
    /// <paramref name="statePublished"/> reports that the loop would have published a state.
    /// </summary>
    private static bool RouteLikeTheReadLoop(
        JsonNode? node,
        BridgeCommandDispatcher dispatcher,
        out bool statePublished)
    {
        statePublished = false;

        switch (BridgeInboundClassifier.Classify(node))
        {
            case BridgeInboundKind.CommandResult:
                dispatcher.TryComplete(node);
                return true;

            case BridgeInboundKind.Capabilities:
                return true;

            default:
                statePublished = true;
                return false;
        }
    }

    private static async Task WaitForPendingAsync(BridgeCommandDispatcher dispatcher)
    {
        for (int attempt = 0; attempt < 200; attempt++)
        {
            if (dispatcher.PendingCount > 0)
                return;

            await Task.Delay(5);
        }

        throw new TimeoutException("The command was never registered as pending.");
    }
}
