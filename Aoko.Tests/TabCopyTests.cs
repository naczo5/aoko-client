using Aoko.Core;
using Aoko.Views;

namespace Aoko.Tests;

/// <summary>
/// The two tabs must explain a missing capability with the same words, and those
/// words must come from the bridge layer where the bridge already owns them.
/// </summary>
public class TabCopyTests
{
    [Fact]
    public void NotConnected_FollowsTheBridgeMessage()
        => Assert.Equal(
            BridgeCommandResult.Failure(BridgeActions.SetSession, BridgeCommandResult.NotConnectedError).Message,
            TabCopy.NotConnected);

    [Fact]
    public void ReturnToMainMenu_FollowsTheBridgeMessage()
        => Assert.Equal(
            BridgeCommandResult.Failure(BridgeActions.SetSession, "in_world").Message,
            TabCopy.ReturnToMainMenu);

    [Fact]
    public void CapacityRefusals_AskForAnUpdateNotForACompiler()
    {
        foreach (string message in new[] { TabCopy.UnsupportedAccountSwitch, TabCopy.UnsupportedServerJoin })
        {
            Assert.Contains("Update aoko, then reconnect.", message);
            Assert.DoesNotContain("Rebuild", message, StringComparison.OrdinalIgnoreCase);
            Assert.DoesNotContain("DLL", message, StringComparison.OrdinalIgnoreCase);
            Assert.DoesNotContain("client", message, StringComparison.OrdinalIgnoreCase);
        }
    }

    [Fact]
    public void CapacityRefusals_DifferOnlyByTheActionTheyRefuse()
    {
        Assert.Equal(
            TabCopy.UnsupportedServerJoin,
            TabCopy.UnsupportedAccountSwitch.Replace("switch accounts", "join servers", StringComparison.Ordinal));
    }

    [Theory]
    [InlineData("Neutral", "DimTextBrush")]
    [InlineData("Success", "AccentBrush")]
    [InlineData("Critical", "CriticalBrush")]
    // The enum is internal to the view assembly, so it cannot sit in a public test signature.
    public void StatusTone_MapsToTheThemeBrush(string tone, string expected)
        => Assert.Equal(expected, TabCopy.BrushKey(Enum.Parse<StatusTone>(tone)));

    [Fact]
    public void Shorten_KeepsOneSentenceAndCutsAtAWord()
    {
        Assert.Equal("", TabCopy.Shorten(null));
        Assert.Equal("Short sentence.", TabCopy.Shorten("Short sentence."));

        string longText = string.Join(' ', Enumerable.Repeat("word", 60));
        string shortened = TabCopy.Shorten(longText);

        Assert.True(shortened.Length <= TabCopy.StatusLimit + 1);
        Assert.EndsWith("…", shortened);
        Assert.DoesNotContain("  ", shortened);
    }
}
