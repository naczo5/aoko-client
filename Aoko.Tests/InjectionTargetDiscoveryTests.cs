using System;
using Aoko.Core;

namespace Aoko.Tests;

public class InjectionTargetDiscoveryTests
{
    [Fact]
    public void Describe_RecognizesLunarAndCommandLineVersion()
    {
        InjectionTarget target = InjectionTargetDiscovery.Describe(
            123,
            IntPtr.Zero,
            "Lunar Client",
            "javaw",
            "javaw.exe --version 1.8.9 -Dfoo=lunarclient",
            @"C:\Users\Test\.lunarclient\jre\bin\javaw.exe");

        Assert.Equal("Lunar Client", target.ClientType);
        Assert.Equal("1.8.9", target.DetectedVersion);
        Assert.True(target.IsLikelyMinecraft);
        Assert.Contains("PID 123", target.DisplayLabel);
    }

    [Fact]
    public void Describe_UsesVersionAsEvidenceForUnidentifiedJavaProcess()
    {
        InjectionTarget target = InjectionTargetDiscovery.Describe(
            456,
            IntPtr.Zero,
            "Game Window",
            "java",
            "java.exe --version 26.2",
            null);

        Assert.Equal("Minecraft Java", target.ClientType);
        Assert.Equal("26.2", target.DetectedVersion);
        Assert.True(target.IsLikelyMinecraft);
    }

    [Fact]
    public void SortTargets_PrefersLikelyClientsAndVisibleWindows()
    {
        var hidden = new InjectionTarget(100, IntPtr.Zero, "", "java", "Java process", "", "No client marker found", 10);
        var visible = new InjectionTarget(200, new IntPtr(42), "Minecraft", "javaw", "Minecraft Java", "1.8.9", "Minecraft/client marker in process metadata", 85);
        var lunar = new InjectionTarget(300, IntPtr.Zero, "Lunar Client", "javaw", "Lunar Client", "1.8.9", "Lunar markers in process metadata", 100);

        var sorted = InjectionTargetDiscovery.SortTargets(new[] { hidden, visible, lunar });

        Assert.Equal(300, sorted[0].ProcessId);
        Assert.Equal(200, sorted[1].ProcessId);
        Assert.Equal(100, sorted[2].ProcessId);
    }
}
