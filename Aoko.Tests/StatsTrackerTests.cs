using System.Windows;
using Aoko.Core;

namespace Aoko.Tests;

public class StatsTrackerTests
{
    [Fact]
    public void RecordClick_UpdatesCpsBucketsAverageAndPeak()
    {
        var stats = new StatsTracker();

        stats.RecordClick(8.4f, leftButton: true);
        stats.RecordClick(12.6f, leftButton: true);
        stats.RecordClick(12.1f, leftButton: true);

        Assert.Equal(3, stats.TotalClicks);
        Assert.Equal(3, stats.LeftClicks);
        Assert.Equal(0, stats.RightClicks);
        Assert.Equal("11.0", stats.AverageCpsText);
        Assert.Equal("12.6", stats.PeakCpsText);
        Assert.Equal(1, stats.CpsBuckets[14].Count);
        Assert.Equal(1, stats.CpsBuckets[22].Count);
        Assert.Equal(1, stats.CpsBuckets[23].Count);
        Assert.True(stats.HasCpsSamples);
    }

    [Fact]
    public void RecordClick_ShowsTightDetailedCpsWindow()
    {
        var stats = new StatsTracker();

        stats.RecordClick(12.1f, leftButton: true);
        stats.RecordClick(12.6f, leftButton: true);

        var visible = stats.CpsBuckets
            .Where(bucket => bucket.Visibility == Visibility.Visible)
            .Select(bucket => bucket.Label)
            .ToArray();

        Assert.Equal(new[] { "11.0", "11.5", "12.0", "12.5", "13.0", "13.5" }, visible);
    }

    [Fact]
    public void RecordClick_SeparatesLeftAndRightCounts()
    {
        var stats = new StatsTracker();

        stats.RecordClick(10.0f, leftButton: true);
        stats.RecordClick(11.0f, leftButton: false);
        stats.RecordClick(12.0f, leftButton: false);

        Assert.Equal(3, stats.TotalClicks);
        Assert.Equal(1, stats.LeftClicks);
        Assert.Equal(2, stats.RightClicks);
    }

    [Fact]
    public void RecordPlaytimeSample_OnlyAdvancesForActivePlay()
    {
        var stats = new StatsTracker();
        var inGame = new GameState { GuiOpen = false };
        var visibleMenu = new GameState { GuiOpen = true, ScreenName = "GuiMainMenu" };
        var ghostGui = new GameState { GuiOpen = true, ScreenName = "GuiIngameMenu" };

        stats.RecordPlaytimeSample(minecraftActive: true, bridgeConnected: true, inGame, cursorVisible: false);
        stats.RecordPlaytimeSample(minecraftActive: false, bridgeConnected: true, inGame, cursorVisible: false);
        stats.RecordPlaytimeSample(minecraftActive: true, bridgeConnected: true, visibleMenu, cursorVisible: true);
        stats.RecordPlaytimeSample(minecraftActive: true, bridgeConnected: true, ghostGui, cursorVisible: false);

        Assert.Equal(TimeSpan.FromSeconds(2), stats.TotalPlaytime);
        Assert.Equal("00:02", stats.TotalPlaytimeText);
    }

    [Fact]
    public void Reset_ClearsAllStatsAndBuckets()
    {
        var stats = new StatsTracker();
        stats.RecordClick(9.0f, leftButton: true);
        stats.RecordClick(13.0f, leftButton: false);
        stats.RecordPlaytimeSample(minecraftActive: true, bridgeConnected: false, new GameState(), cursorVisible: false);

        stats.Reset();

        Assert.Equal(0, stats.TotalClicks);
        Assert.Equal(0, stats.LeftClicks);
        Assert.Equal(0, stats.RightClicks);
        Assert.Equal(TimeSpan.Zero, stats.TotalPlaytime);
        Assert.False(stats.HasCpsSamples);
        Assert.Equal("n/a", stats.AverageCpsText);
        Assert.All(stats.CpsBuckets, bucket => Assert.Equal(0, bucket.Count));
        Assert.All(stats.CpsBuckets, bucket => Assert.Equal(Visibility.Collapsed, bucket.Visibility));
    }
}
