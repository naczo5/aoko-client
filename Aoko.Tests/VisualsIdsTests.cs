using Aoko.Core.Visuals;

namespace Aoko.Tests;

public class VisualsIdsTests
{
    [Theory]
    [InlineData("halo", "halo")]
    [InlineData("  Halo  ", "halo")]
    [InlineData("TSUKI-hime_2", "tsuki-hime_2")]
    [InlineData("a", "a")]
    [InlineData("0day", "0day")]
    public void Normalize_AcceptsValidIds(string input, string expected)
        => Assert.Equal(expected, VisualsIds.Normalize(input));

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("-leading")]
    [InlineData("_leading")]
    [InlineData("has space")]
    [InlineData("dot.png")]
    [InlineData("a/b")]
    [InlineData("..\\evil")]
    [InlineData("caf\u00e9")]
    [InlineData("line\nbreak")]
    public void Normalize_RejectsInvalidIds(string? input)
        => Assert.Equal(string.Empty, VisualsIds.Normalize(input));

    [Fact]
    public void Normalize_EnforcesThirtyTwoCharacterLimit()
    {
        Assert.Equal(new string('a', 32), VisualsIds.Normalize(new string('A', 32)));
        Assert.Equal(string.Empty, VisualsIds.Normalize(new string('a', 33)));
    }

    [Fact]
    public void IsValid_RequiresCanonicalForm()
    {
        Assert.True(VisualsIds.IsValid("halo-2"));
        Assert.False(VisualsIds.IsValid("Halo"));
        Assert.False(VisualsIds.IsValid(" halo"));
        Assert.False(VisualsIds.IsValid(""));
        Assert.False(VisualsIds.IsValid(null));
    }

    [Theory]
    [InlineData("aoko-blue", "Aoko Blue")]
    [InlineData("moon_glass", "Moon Glass")]
    [InlineData("plain", "Plain")]
    public void DisplayNameFromId_CapitalisesWords(string id, string expected)
        => Assert.Equal(expected, VisualsIds.DisplayNameFromId(id));
}
