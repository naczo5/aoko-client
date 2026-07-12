using Aoko.Core;

namespace Aoko.Tests;

public class UpdateServiceTests
{
    [Theory]
    [InlineData("0.9.1", "v0.9.2", true)]
    [InlineData("0.9.1", "0.10.0", true)]
    [InlineData("0.9.1", "v0.10", true)]
    [InlineData("0.9.1", "v0.9.1", false)]
    [InlineData("0.9.2", "v0.9.1", false)]
    [InlineData("1.0.0", "v1.0.1-beta", true)]
    public void IsNewerReleaseComparesSemanticVersions(string current, string latest, bool expected)
    {
        Assert.Equal(expected, UpdateService.IsNewerRelease(current, latest));
    }

    [Fact]
    public void ParseReleaseVersionAcceptsGitHubTagPrefixAndMetadata()
    {
        Assert.Equal(new Version(0, 9, 1, 0), UpdateService.ParseReleaseVersion("v0.9.1+build.4"));
    }
}
