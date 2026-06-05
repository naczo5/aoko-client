using Aoko.Core;

namespace Aoko.Tests;

public class WindowDetectionTests
{
    [Fact]
    public void SortWindowTargets_PlacesJvmEntriesFirst_ThenAlphabeticalByTitle()
    {
        var targets = new[]
        {
            new WindowTarget(IntPtr.Zero, 100, "Zulu App", "chrome", false),
            new WindowTarget(IntPtr.Zero, 200, "Alpha Game", "javaw", true),
            new WindowTarget(IntPtr.Zero, 300, "Beta Tool", "notepad", false),
            new WindowTarget(IntPtr.Zero, 400, "Minecraft", "java", true),
        };

        var sorted = WindowDetection.SortWindowTargets(targets);

        Assert.True(sorted[0].IsJvm);
        Assert.True(sorted[1].IsJvm);
        Assert.False(sorted[2].IsJvm);
        Assert.False(sorted[3].IsJvm);
        Assert.Equal("Alpha Game", sorted[0].Title);
        Assert.Equal("Minecraft", sorted[1].Title);
        Assert.Equal("Beta Tool", sorted[2].Title);
        Assert.Equal("Zulu App", sorted[3].Title);
    }

    [Fact]
    public void WindowTarget_DisplayLabel_MarksJvmTargets()
    {
        var jvm = new WindowTarget(IntPtr.Zero, 42, "Lunar Client", "javaw", true);
        var other = new WindowTarget(IntPtr.Zero, 99, "Notepad", "notepad", false);

        Assert.StartsWith("[JVM]", jvm.DisplayLabel);
        Assert.DoesNotContain("[JVM]", other.DisplayLabel);
        Assert.Contains("PID 42", jvm.DisplayLabel);
    }
}
