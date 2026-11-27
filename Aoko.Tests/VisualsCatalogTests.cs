using System.Text;
using Aoko.Core.Visuals;

namespace Aoko.Tests;

public sealed class VisualsCatalogTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "aoko-visuals-" + Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch (IOException) { }
    }

    private string Write(string relative, string content)
    {
        string path = Path.Combine(_root, relative.Replace('/', Path.DirectorySeparatorChar));
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content, new UTF8Encoding(false));
        return path;
    }

    [Fact]
    public void Scan_MissingDirectory_IsEmpty()
    {
        VisualsSnapshot snapshot = VisualsCatalog.Scan(Path.Combine(_root, "nope"));

        Assert.Empty(snapshot.Capes);
        Assert.Empty(snapshot.Skies);
        Assert.Empty(snapshot.Cosmetics);
    }

    [Fact]
    public void Scan_ReadsCapesSkiesAndCosmetics()
    {
        Write("capes/moon-glass.png", "png");
        Write("capes/Upper.png", "png");
        Write("capes/readme.txt", "x");
        Write("skies/tsukihime/sky.json", """{"name":"Tsukihime","time":18000,"skybox":{"texture":"sky.jpg"}}""");
        Write("skies/tsukihime/sky.jpg", "jpg");
        Write("skies/plain/sky.json", """{"name":"Plain"}""");
        Write("cosmetics/halo/cosmetic.json", """{"name":"Halo","slot":"hat"}""");
        Write("cosmetics/wings/cosmetic.json", """{"name":"Wings","slot":"back"}""");

        VisualsSnapshot snapshot = VisualsCatalog.Scan(_root);

        Assert.Equal(new[] { new CapeInfo("moon-glass", "Moon Glass") }, snapshot.Capes);
        Assert.Equal(
            new[] { new SkyInfo("plain", "Plain", false, null), new SkyInfo("tsukihime", "Tsukihime", true, 18000) },
            snapshot.Skies);
        Assert.Equal(
            new[] { new CosmeticInfo("halo", "Halo", "hat"), new CosmeticInfo("wings", "Wings", "back") },
            snapshot.Cosmetics);
        Assert.Equal("halo", Assert.Single(snapshot.CosmeticsForSlot("hat")).Id);
        Assert.Equal("wings", Assert.Single(snapshot.CosmeticsForSlot("back")).Id);
    }

    [Fact]
    public void Scan_SkipsMalformedEntriesWithoutThrowing()
    {
        Write("skies/broken/sky.json", "{ not json");
        Write("skies/array/sky.json", "[1,2,3]");
        Write("skies/noname/sky.json", """{"time":6000}""");
        Write("skies/numname/sky.json", """{"name":42}""");
        Write("skies/empty/sky.json", "");
        Write("skies/missingfile/readme.md", "no sky.json here");
        Write("skies/good/sky.json", """{"name":"Good",}""");
        Write("cosmetics/badslot/cosmetic.json", """{"name":"Bad","slot":"feet"}""");
        Write("cosmetics/noslot/cosmetic.json", """{"name":"Bad"}""");
        Write("cosmetics/broken/cosmetic.json", "\0\0\0");

        VisualsSnapshot snapshot = VisualsCatalog.Scan(_root);

        Assert.Equal("good", Assert.Single(snapshot.Skies).Id);
        Assert.Empty(snapshot.Cosmetics);
    }

    [Fact]
    public void Scan_SkipsBadIds()
    {
        Write("capes/Has Space.png", "png");
        Write("capes/-dash.png", "png");
        Write("capes/" + new string('a', 33) + ".png", "png");
        Write("capes/" + new string('a', 32) + ".png", "png");
        Write("skies/Mixed/sky.json", """{"name":"Mixed"}""");
        Write("skies/with space/sky.json", """{"name":"Spaced"}""");
        Write("cosmetics/_under/cosmetic.json", """{"name":"Under","slot":"hat"}""");

        VisualsSnapshot snapshot = VisualsCatalog.Scan(_root);

        Assert.Equal(new string('a', 32), Assert.Single(snapshot.Capes).Id);
        Assert.Empty(snapshot.Skies);
        Assert.Empty(snapshot.Cosmetics);
    }

    [Fact]
    public void Scan_SkipsOversizeNamesAndFiles()
    {
        Write("skies/longname/sky.json", "{\"name\":\"" + new string('n', 65) + "\"}");
        Write("skies/maxname/sky.json", "{\"name\":\"" + new string('n', 64) + "\"}");
        Write("skies/huge/sky.json", "{\"name\":\"Huge\",\"pad\":\"" + new string('x', (int)VisualsCatalog.MaxJsonBytes) + "\"}");

        VisualsSnapshot snapshot = VisualsCatalog.Scan(_root);

        Assert.Equal("maxname", Assert.Single(snapshot.Skies).Id);
    }

    [Fact]
    public void Scan_IgnoresOutOfRangeTimeAndMissingSkyboxTexture()
    {
        Write("skies/late/sky.json", """{"name":"Late","time":24000,"skybox":{"texture":"sky.jpg"}}""");
        Write("skies/neg/sky.json", """{"name":"Neg","time":-1}""");
        Write("skies/escape/sky.json", """{"name":"Escape","skybox":{"texture":"../outside.jpg"}}""");
        Write("outside.jpg", "jpg");

        VisualsSnapshot snapshot = VisualsCatalog.Scan(_root);

        Assert.All(snapshot.Skies, sky =>
        {
            Assert.Null(sky.Time);
            Assert.False(sky.HasSkybox);
        });
        Assert.Equal(3, snapshot.Skies.Length);
    }

    [Fact]
    public void Refresh_RaisesChangedOnlyWhenContentChanges()
    {
        var catalog = new VisualsCatalog(_root);
        int raised = 0;
        catalog.Changed += _ => raised++;

        catalog.Refresh();
        Assert.Equal(0, raised);

        Write("capes/one.png", "png");
        VisualsSnapshot first = catalog.Refresh();
        Assert.Equal(1, raised);
        Assert.Same(first, catalog.Current);

        catalog.Refresh();
        Assert.Equal(1, raised);

        File.Delete(Path.Combine(_root, "capes", "one.png"));
        catalog.Refresh();
        Assert.Equal(2, raised);
        Assert.Empty(catalog.Current.Capes);
        Assert.Single(first.Capes);
    }
}
