using Aoko.Core;

namespace Aoko.Tests;

public class ChestClickCheckTests
{
    [Fact]
    public void IsChestClickAllowed_ChecksOff_AllowsAnyContainer()
    {
        Assert.True(Clicker.IsChestClickAllowed(new GameState(), menuCheckEnabled: false));
    }

    [Fact]
    public void IsChestClickAllowed_ChecksOn_RejectsContainerWithoutBridgeState()
    {
        Assert.False(Clicker.IsChestClickAllowed(new GameState { ChestStealerState = null }, menuCheckEnabled: true));
    }

    [Fact]
    public void IsChestClickAllowed_ChecksOn_RejectsMenuContainer()
    {
        var state = new GameState { ChestStealerState = new ChestStealerState { Ready = false, Physical = false } };
        Assert.False(Clicker.IsChestClickAllowed(state, menuCheckEnabled: true));
    }

    [Fact]
    public void IsChestClickAllowed_ChecksOn_AllowsPhysicalChest()
    {
        var state = new GameState { ChestStealerState = new ChestStealerState { Ready = true, Physical = true } };
        Assert.True(Clicker.IsChestClickAllowed(state, menuCheckEnabled: true));
    }
}
