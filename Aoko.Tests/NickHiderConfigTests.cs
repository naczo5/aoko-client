using Aoko.Core;

namespace Aoko.Tests;

public class NickHiderConfigTests
{
    [Fact]
    public void NormalizeAlias_TrimsPlainText()
    {
        Assert.Equal("Stream Alias", NickHiderConfig.NormalizeAlias("  Stream Alias  "));
    }

    [Fact]
    public void NormalizeInput_PreservesTypingWhitespaceUntilSend()
    {
        Assert.Equal("Stream ", NickHiderConfig.NormalizeInput("Stream "));
        Assert.Equal("Stream", NickHiderConfig.NormalizeAlias("Stream "));
    }

    [Theory]
    [InlineData("")]
    [InlineData("bad\nname")]
    [InlineData("\u00A7aBlue")]
    public void NormalizeAlias_RejectsInvalidText(string alias)
    {
        Assert.Equal(string.Empty, NickHiderConfig.NormalizeAlias(alias));
    }

    [Fact]
    public void Profile_NickHider_RoundTrips()
    {
        var profile = new Profile { NickHiderEnabled = true, NickHiderAlias = "Alias" };
        var options = new System.Text.Json.JsonSerializerOptions { PropertyNamingPolicy = System.Text.Json.JsonNamingPolicy.CamelCase };

        string json = System.Text.Json.JsonSerializer.Serialize(profile, options);
        Profile? restored = System.Text.Json.JsonSerializer.Deserialize<Profile>(json, options);

        Assert.NotNull(restored);
        Assert.True(restored!.NickHiderEnabled);
        Assert.Equal("Alias", restored.NickHiderAlias);
    }
}
