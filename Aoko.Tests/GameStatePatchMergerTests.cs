using System.Text.Json.Nodes;
using Aoko.Core;
using Xunit;

namespace Aoko.Tests;

public class GameStatePatchMergerTests
{
    [Fact]
    public void UnmarkedStateIsNotTreatedAsPartial()
    {
        Assert.False(GameStatePatchMerger.IsPartial(JsonNode.Parse("{\"type\":\"state\"}")));
    }

    [Fact]
    public void ExplicitPatchPreservesFieldsThatAreNotPresent()
    {
        var previous = new GameState
        {
            InWorld = true,
            ScreenName = "GuiChat",
            ActionBar = "keep me",
            AttackCooldown = 0.35f,
            Entities = new() { new EntityInfo { Name = "Target", Dist = 3.0 } }
        };

        var patch = JsonNode.Parse("{\"type\":\"statePatch\",\"inWorld\":true,\"attackCooldown\":0.9}")!.AsObject();
        GameState merged = GameStatePatchMerger.Apply(previous, patch);

        Assert.True(merged.InWorld);
        Assert.Equal("GuiChat", merged.ScreenName);
        Assert.Equal("keep me", merged.ActionBar);
        Assert.Equal(0.9f, merged.AttackCooldown);
        Assert.Single(merged.Entities);
        Assert.Equal("Target", merged.Entities[0].Name);
    }

    [Fact]
    public void WorldExitPatchClearsTransientTargets()
    {
        var previous = new GameState
        {
            InWorld = true,
            Entities = new() { new EntityInfo { Name = "Target" } },
            ChestStealerState = new ChestStealerState { Ready = true }
        };

        var patch = JsonNode.Parse("{\"partial\":true,\"inWorld\":false}")!.AsObject();
        GameState merged = GameStatePatchMerger.Apply(previous, patch);

        Assert.Empty(merged.Entities);
        Assert.Null(merged.ChestStealerState);
    }

    [Fact]
    public void ExplicitNullEntitiesClearsThePreviousSnapshot()
    {
        var previous = new GameState
        {
            Entities = new() { new EntityInfo { Name = "Target" } }
        };

        var patch = JsonNode.Parse("{\"partial\":true,\"entities\":null}")!.AsObject();
        GameState merged = GameStatePatchMerger.Apply(previous, patch);

        Assert.Empty(merged.Entities);
    }

    [Fact]
    public void BooleanPartialMarkerIsRecognized()
    {
        Assert.True(GameStatePatchMerger.IsPartial(JsonNode.Parse("{\"partial\":true}")));
        Assert.False(GameStatePatchMerger.IsPartial(JsonNode.Parse("{\"partial\":false}")));
    }
}
