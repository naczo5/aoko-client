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
        Assert.Null(merged.RefillState);
    }

    [Fact]
    public void PartialPatchPreservesRefillState()
    {
        var previous = new GameState
        {
            InWorld = true,
            RefillState = new RefillState
            {
                Ready = true,
                WindowId = 1,
                Slots = new() { new ChestStealerSlot { Index = 9, SlotNumber = 9, X = 10, Y = 10 } }
            }
        };

        var patch = JsonNode.Parse("{\"type\":\"statePatch\",\"inWorld\":true,\"attackCooldown\":1.0}")!.AsObject();
        GameState merged = GameStatePatchMerger.Apply(previous, patch);

        Assert.NotNull(merged.RefillState);
        Assert.True(merged.RefillState.Ready);
        Assert.Single(merged.RefillState.Slots);
        Assert.Equal(9, merged.RefillState.Slots[0].SlotNumber);
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
