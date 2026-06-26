using Aoko.Core;
using System.Diagnostics;

namespace Aoko.Tests;

public class GameStateClientInjectionTests
{
    [Fact]
    public void ResolveInjectionTarget_ReturnsCurrentProcess_WhenPidMatches()
    {
        using Process current = Process.GetCurrentProcess();

        Process? resolved = GameStateClient.ResolveInjectionTarget(current.Id);

        Assert.NotNull(resolved);
        Assert.Equal(current.Id, resolved!.Id);
    }

    [Fact]
    public void ResolveInjectionTarget_ReturnsNull_ForInvalidPid()
    {
        Process? resolved = GameStateClient.ResolveInjectionTarget(-1);

        Assert.Null(resolved);
    }
}
