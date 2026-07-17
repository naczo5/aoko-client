using Aoko.Core;

namespace Aoko.Tests;

public class GameStateClientSeamTests
{
    [Theory]
    [InlineData("26.1", "26.1")]
    [InlineData("26.2", "26.2")]
    [InlineData("26.1.2", "26.1")]
    [InlineData("26", "26.1")]
    [InlineData("minecraft 26", "26.1")]
    [InlineData("1.21.4", "1.21")]
    [InlineData("1.8.9", "1.8.9")]
    [InlineData("1.8.8", "1.8.9")]
    [InlineData("2.26.0", null)]
    [InlineData("unknown", null)]
    [InlineData("", null)]
    public void NormalizeDetectedVersion_ReturnsExpectedValue(string input, string? expected)
    {
        string? normalized = GameStateClient.NormalizeDetectedVersion(input);
        Assert.Equal(expected, normalized);
    }

    [Theory]
    [InlineData("26.2", "bridge_261.dll")]
    [InlineData("26.1", "bridge_261.dll")]
    [InlineData("1.21", "bridge_261.dll")]
    [InlineData("1.8.9", "bridge.dll")]
    [InlineData("auto", "bridge.dll")]
    public void ResolveBridgeDllName_MapsModernVersionsTo261(string version, string expectedDll)
    {
        Assert.Equal(expectedDll, GameStateClient.ResolveBridgeDllName(version));
    }

    [Theory]
    [InlineData(
        @"C:\Prism\javaw.exe -cp C:/libs/net/fabricmc/intermediary/26.2/intermediary-26.2.jar;C:/libs/com/mojang/minecraft/26.2/minecraft-26.2-client.jar org.prismlauncher.EntryPoint",
        "26.2")]
    [InlineData(
        @"C:\libs\log4j-core-2.26.0.jar;C:\libs\minecraft-1.8.9-client.jar net.minecraft.client.main.Main",
        "1.8.9")]
    [InlineData(
        @"javaw.exe com.moonsworth.lunar.genesis.Genesis --version 26.2 --launcherVersion 3.0",
        "26.2")]
    [InlineData(
        @"javaw.exe -cp C:/versions/1.21.4/client.jar net.minecraft.client.main.Main --version 1.21.4",
        "1.21")]
    [InlineData(
        @"javaw.exe -cp C:/libs/log4j-api-2.26.0.jar;C:/libs/log4j-core-2.26.0.jar org.example.Main",
        null)]
    public void TryParseVersion_ReadsLauncherCommandLines(string commandLine, string? expected)
    {
        Assert.Equal(expected, ProcessCommandLine.TryParseVersion(commandLine));
    }

    [Theory]
    [InlineData("Default", 0)]
    [InlineData("Minimal", 1)]
    [InlineData("Outlined", 2)]
    [InlineData("Glass", 3)]
    [InlineData("Bold", 4)]
    [InlineData("unknown-style", 0)]
    [InlineData("", 0)]
    public void ModuleListStyleToIndex_MapsStylesSafely(string style, int expected)
    {
        int index = GameStateClient.ModuleListStyleToIndex(style);
        Assert.Equal(expected, index);
    }
}
