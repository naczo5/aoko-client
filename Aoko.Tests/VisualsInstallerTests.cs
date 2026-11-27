using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Aoko.Core.Visuals;

namespace Aoko.Tests;

public sealed class VisualsInstallerTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "aoko-install-" + Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch (IOException) { }
    }

    private sealed class FakeReader : IVisualsResourceReader
    {
        private readonly Dictionary<string, byte[]> _resources = new();

        public FakeReader Add(string relative, string content)
        {
            _resources[VisualsInstaller.ResourcePrefix + relative] = Encoding.UTF8.GetBytes(content);
            return this;
        }

        public void Set(string relative, string content) => Add(relative, content);

        public IReadOnlyList<string> GetResourceNames() => _resources.Keys.ToList();

        public byte[]? Read(string resourceName) => _resources.GetValueOrDefault(resourceName);
    }

    private static FakeReader Reader() => new FakeReader()
        .Add("capes/moon.png", "moon-v1")
        .Add("skies/night/sky.json", "{\"name\":\"Night\"}");

    private string PathOf(string relative) => Path.Combine(_root, relative.Replace('/', Path.DirectorySeparatorChar));

    private Dictionary<string, string> Manifest()
        => JsonSerializer.Deserialize<Dictionary<string, string>>(File.ReadAllText(PathOf(".builtin.json")))!;

    private static string Sha(string content)
        => Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(content))).ToLowerInvariant();

    [Fact]
    public void FreshInstall_ExtractsEverythingAndRecordsHashes()
    {
        VisualsInstallResult result = new VisualsInstaller(Reader()).Install(_root);

        Assert.True(result.Success);
        Assert.Equal(2, result.Installed);
        Assert.Equal("moon-v1", File.ReadAllText(PathOf("capes/moon.png")));
        Assert.Equal("{\"name\":\"Night\"}", File.ReadAllText(PathOf("skies/night/sky.json")));
        Assert.Equal(Sha("moon-v1"), Manifest()["capes/moon.png"]);
        Assert.True(Directory.Exists(PathOf("cosmetics")));
        Assert.Empty(Directory.GetFiles(_root, "*.tmp", SearchOption.AllDirectories));
    }

    [Fact]
    public void SecondRun_ChangesNothing()
    {
        var installer = new VisualsInstaller(Reader());
        installer.Install(_root);
        string manifestBefore = File.ReadAllText(PathOf(".builtin.json"));
        DateTime writtenBefore = File.GetLastWriteTimeUtc(PathOf("capes/moon.png"));

        VisualsInstallResult result = installer.Install(_root);

        Assert.True(result.Success);
        Assert.Equal(0, result.Installed + result.Updated + result.LeftAlone);
        Assert.Equal(2, result.UpToDate);
        Assert.Equal(manifestBefore, File.ReadAllText(PathOf(".builtin.json")));
        Assert.Equal(writtenBefore, File.GetLastWriteTimeUtc(PathOf("capes/moon.png")));
    }

    [Fact]
    public void UneditedBuiltIn_IsUpdatedWhenTheEmbeddedCopyChanges()
    {
        new VisualsInstaller(Reader()).Install(_root);

        FakeReader newer = Reader();
        newer.Set("capes/moon.png", "moon-v2");
        VisualsInstallResult result = new VisualsInstaller(newer).Install(_root);

        Assert.True(result.Success);
        Assert.Equal(1, result.Updated);
        Assert.Equal("moon-v2", File.ReadAllText(PathOf("capes/moon.png")));
        Assert.Equal(Sha("moon-v2"), Manifest()["capes/moon.png"]);
    }

    [Fact]
    public void UserEditedBuiltIn_IsLeftAlone()
    {
        new VisualsInstaller(Reader()).Install(_root);
        File.WriteAllText(PathOf("capes/moon.png"), "my custom moon");

        FakeReader newer = Reader();
        newer.Set("capes/moon.png", "moon-v2");
        VisualsInstallResult result = new VisualsInstaller(newer).Install(_root);

        Assert.True(result.Success);
        Assert.Equal(1, result.LeftAlone);
        Assert.Equal("my custom moon", File.ReadAllText(PathOf("capes/moon.png")));
        Assert.Equal(Sha("moon-v1"), Manifest()["capes/moon.png"]);
    }

    [Fact]
    public void UserFileWithABuiltInName_IsLeftAloneWhenNothingWasRecorded()
    {
        Directory.CreateDirectory(PathOf("capes"));
        File.WriteAllText(PathOf("capes/moon.png"), "mine first");

        VisualsInstallResult result = new VisualsInstaller(Reader()).Install(_root);

        Assert.True(result.Success);
        Assert.Equal(1, result.LeftAlone);
        Assert.Equal("mine first", File.ReadAllText(PathOf("capes/moon.png")));
        Assert.False(Manifest().ContainsKey("capes/moon.png"));
    }

    [Fact]
    public void UserAddedFiles_AreNeverTouched()
    {
        Directory.CreateDirectory(PathOf("capes"));
        Directory.CreateDirectory(PathOf("skies/mine"));
        File.WriteAllText(PathOf("capes/mine.png"), "my cape");
        File.WriteAllText(PathOf("skies/mine/sky.json"), "{\"name\":\"Mine\"}");
        File.WriteAllText(PathOf("notes.txt"), "keep");

        var installer = new VisualsInstaller(Reader());
        installer.Install(_root);
        installer.Install(_root);

        Assert.Equal("my cape", File.ReadAllText(PathOf("capes/mine.png")));
        Assert.Equal("{\"name\":\"Mine\"}", File.ReadAllText(PathOf("skies/mine/sky.json")));
        Assert.Equal("keep", File.ReadAllText(PathOf("notes.txt")));
        Assert.DoesNotContain("capes/mine.png", Manifest().Keys);
    }

    [Fact]
    public void DeletedBuiltIn_IsExtractedAgain()
    {
        var installer = new VisualsInstaller(Reader());
        installer.Install(_root);
        File.Delete(PathOf("capes/moon.png"));

        VisualsInstallResult result = installer.Install(_root);

        Assert.Equal(1, result.Installed);
        Assert.Equal("moon-v1", File.ReadAllText(PathOf("capes/moon.png")));
    }

    [Fact]
    public void CorruptManifest_IsTreatedAsEmptyAndNeverOverwritesUserFiles()
    {
        new VisualsInstaller(Reader()).Install(_root);
        File.WriteAllText(PathOf("capes/moon.png"), "edited");
        File.WriteAllText(PathOf(".builtin.json"), "{ not json");

        VisualsInstallResult result = new VisualsInstaller(Reader()).Install(_root);

        Assert.True(result.Success);
        Assert.Equal("edited", File.ReadAllText(PathOf("capes/moon.png")));
        Assert.Equal(1, result.LeftAlone);
        Assert.Equal(Sha("{\"name\":\"Night\"}"), Manifest()["skies/night/sky.json"]);
    }

    [Fact]
    public void UnwritableDirectory_ReturnsFailureInsteadOfThrowing()
    {
        // A file where the directory should be cannot be created or written on any platform, unlike permission bits.
        Directory.CreateDirectory(_root);
        string blocker = Path.Combine(_root, "visuals");
        File.WriteAllText(blocker, "not a directory");

        VisualsInstallResult result = new VisualsInstaller(Reader()).Install(blocker);

        Assert.False(result.Success);
        Assert.NotEmpty(result.Errors);
        Assert.Equal(0, result.Installed);
    }

    [Fact]
    public void NoResourcesOrBlankPath_DoNotThrow()
    {
        Assert.True(new VisualsInstaller(new FakeReader()).Install(_root).Success);
        Assert.False(new VisualsInstaller(Reader()).Install("").Success);
    }

    [Theory]
    [InlineData("../escape.png")]
    [InlineData("capes/../../escape.png")]
    [InlineData("capes\\evil.png")]
    [InlineData("capes//double.png")]
    public void UnsafeResourceNames_AreIgnored(string relative)
    {
        VisualsInstallResult result = new VisualsInstaller(new FakeReader().Add(relative, "x")).Install(_root);

        Assert.True(result.Success);
        Assert.Equal(0, result.Installed);
        Assert.False(File.Exists(Path.Combine(Path.GetDirectoryName(_root)!, "escape.png")));
    }

    [Fact]
    public void ReaderThatFails_IsReportedPerFile()
    {
        var reader = new ThrowingReader();

        VisualsInstallResult result = new VisualsInstaller(reader).Install(_root);

        Assert.False(result.Success);
        Assert.Equal(1, result.Failed);
    }

    private sealed class ThrowingReader : IVisualsResourceReader
    {
        public IReadOnlyList<string> GetResourceNames() => [VisualsInstaller.ResourcePrefix + "capes/x.png"];

        public byte[]? Read(string resourceName) => throw new IOException("boom");
    }

    [Fact]
    public void EmbeddedResources_UseTheVisualsPrefixWithForwardSlashes()
    {
        string[] names = new AssemblyVisualsResourceReader(typeof(VisualsInstaller).Assembly)
            .GetResourceNames()
            .Where(n => n.StartsWith("Aoko.Visuals", StringComparison.Ordinal))
            .ToArray();

        Assert.All(names, name =>
        {
            Assert.StartsWith(VisualsInstaller.ResourcePrefix, name, StringComparison.Ordinal);
            Assert.DoesNotContain('\\', name);
        });

        // Whatever is embedded must also install cleanly into an empty folder.
        VisualsInstallResult result = VisualsInstaller.InstallEmbedded(_root);
        Assert.True(result.Success, string.Join("; ", result.Errors));
        Assert.Equal(names.Length, result.Installed);
    }
}
