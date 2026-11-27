using Aoko.Core.Visuals;

namespace Aoko.Tests;

public class VisualsStatusTextTests
{
    [Theory]
    [InlineData(null, "Not active.")]
    [InlineData("", "Not active.")]
    [InlineData("off", "Not active.")]
    [InlineData("ok", "Active.")]
    [InlineData("partial:skybox ignored", "Partly applied: skybox ignored.")]
    [InlineData("partial:", "Partly applied.")]
    [InlineData("unsupported:1.21.4", "Needs Minecraft 1.8.9 or 26.1 and newer.")]
    [InlineData("error:cape texture missing.", "Something went wrong: cape texture missing.")]
    [InlineData("error:", "Something went wrong.")]
    [InlineData("whatever", "Not active.")]
    public void StatusText_MapsEveryStatusKind(string? status, string expected)
        => Assert.Equal(expected, VisualsStatusText.Describe(status));

    [Fact]
    public void StatusText_CapsLongDetails()
    {
        string text = VisualsStatusText.Describe("error:" + new string('x', 500));

        Assert.True(text.Length < 200);
        Assert.EndsWith("....", text);
    }
}
