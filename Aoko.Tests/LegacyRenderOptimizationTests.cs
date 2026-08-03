namespace Aoko.Tests;

public sealed class LegacyRenderOptimizationTests
{
    [Fact]
    public void MenuClose_DoesNotDestroyValidRendererResources()
    {
        string source = LegacyBridgeSource();

        Assert.DoesNotContain(
            "ResetImGuiBackendsForReinit(\"minecraft screen closed\")",
            source,
            StringComparison.Ordinal);
        Assert.Contains(
            "keep the expensive ImGui context, font atlas, and GL backend",
            source,
            StringComparison.Ordinal);
    }

    [Fact]
    public void ClosestPlayerRenderer_ConsumesPreparedSnapshotWithoutJni()
    {
        string source = LegacyBridgeSource();
        string renderer = Slice(
            source,
            "void RenderClosestPlayerInfo(",
            "void RenderChestESP(");

        Assert.Contains("ClosestPlayerDrawSnapshot18& snapshot", renderer);
        Assert.DoesNotContain("ScopedJNIEnv", renderer);
        Assert.DoesNotContain("CallObjectMethod", renderer);
    }

    [Fact]
    public void RenderEntry_CapturesConfigAndHudLayoutTogether()
    {
        string source = LegacyBridgeSource();
        string renderEntry = Slice(
            source,
            "BOOL WINAPI HookedSwapBuffers(",
            "// IAT hook:");

        Assert.Contains("renderConfig = g_config;", renderEntry);
        Assert.Contains("renderHudLayout = g_hudLayout;", renderEntry);
        Assert.Contains(
            "RenderBlockESP(w, h, renderConfig, renderHudLayout);",
            renderEntry);
    }

    [Fact]
    public void HeldItemNametagSetting_IsParsedAndApplied()
    {
        string source = LegacyBridgeSource();

        Assert.Contains("nametagShowHeldItem", source, StringComparison.Ordinal);
        Assert.Contains("showHeldItem", source, StringComparison.Ordinal);
        Assert.Contains("GetEntityHeldItemInfo", source, StringComparison.Ordinal);
        Assert.Contains(
            "if (!heldText.empty())",
            source,
            StringComparison.Ordinal);
    }

    private static string LegacyBridgeSource()
        => File.ReadAllText(
            Path.Combine(
                FindRepoRoot(),
                "McInjector",
                "src",
                "main",
                "cpp",
                "bridge.cpp"));

    private static string Slice(string source, string startMarker, string endMarker)
    {
        int start = source.IndexOf(startMarker, StringComparison.Ordinal);
        int end = source.IndexOf(endMarker, start, StringComparison.Ordinal);
        Assert.True(start >= 0, $"Missing start marker: {startMarker}");
        Assert.True(end > start, $"Missing end marker: {endMarker}");
        return source[start..end];
    }

    private static string FindRepoRoot()
    {
        string? directory = AppContext.BaseDirectory;
        while (!string.IsNullOrEmpty(directory))
        {
            if (File.Exists(Path.Combine(directory, "Aoko", "Aoko.csproj")))
                return directory;
            directory = Directory.GetParent(directory)?.FullName;
        }

        throw new InvalidOperationException("Could not locate repository root.");
    }
}
