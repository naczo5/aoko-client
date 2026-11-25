using Aoko.Core.Servers;

namespace Aoko.Tests;

public class ServerStoreTests
{
    [Fact]
    public void GetStorePath_UsesTheAokoAppDataFolder()
    {
        Assert.Equal(
            Path.Combine(Path.GetTempPath(), "appdata", "Aoko", "servers.json"),
            ServerStore.GetStorePath(Path.Combine(Path.GetTempPath(), "appdata")));
    }

    [Fact]
    public void Add_DedupesByParsedHostAndPort()
    {
        using var store = new TempStore();

        SavedServer first = store.Store.Add("", "play.hypixel.net");
        Assert.Equal("play.hypixel.net", first.Name);
        Assert.Equal("play.hypixel.net", first.Address);
        Assert.Equal(32, first.Id.Length);

        SavedServer named = store.Store.Add("Fancy", "play.hypixel.net");
        Assert.Equal(first.Id, named.Id);
        Assert.Equal("Fancy", named.Name);
        Assert.Single(store.Store.Servers);

        SavedServer samePort = store.Store.Add("", "play.hypixel.net:25565");
        Assert.Equal(first.Id, samePort.Id);
        // The bookmark keeps the label and the address the user saved: joining from the
        // address box must not rename "Fancy" to the text typed that time.
        Assert.Equal("Fancy", samePort.Name);
        Assert.Equal("play.hypixel.net", samePort.Address);
        Assert.Single(store.Store.Servers);

        SavedServer renamed = store.Store.Add("Renamed", "play.hypixel.net:25565");
        Assert.Equal(first.Id, renamed.Id);
        Assert.Equal("Renamed", renamed.Name);
        Assert.Equal("play.hypixel.net", renamed.Address);

        SavedServer second = store.Store.Add("Other", "mc.example.com:25566");
        Assert.NotEqual(first.Id, second.Id);
        Assert.Equal(2, store.Store.Servers.Count);
    }

    [Fact]
    public void Save_WritesCamelCaseIndentedJsonAtomically()
    {
        using var store = new TempStore();

        store.Store.Add("Alpha", "alpha.example.com:25566");
        store.Store.Add("", "beta.example.com");

        Assert.True(File.Exists(store.FilePath));
        Assert.False(File.Exists(store.FilePath + ".tmp"));

        string json = File.ReadAllText(store.FilePath);
        Assert.Contains("\"id\"", json, StringComparison.Ordinal);
        Assert.Contains("\"name\"", json, StringComparison.Ordinal);
        Assert.Contains("\"address\"", json, StringComparison.Ordinal);
        Assert.Contains("\"addedAt\"", json, StringComparison.Ordinal);
        Assert.Contains("\n  ", json, StringComparison.Ordinal);
        Assert.False(json.StartsWith('\uFEFF'));
    }

    [Fact]
    public void Load_RestoresEntriesAndJoinedStamps()
    {
        using var store = new TempStore();

        SavedServer added = store.Store.Add("Alpha", "alpha.example.com:25566");
        Assert.Null(added.LastJoinedAt);

        store.Store.MarkJoined(added);
        Assert.NotNull(added.LastJoinedAt);

        var reloaded = new ServerStore(store.FilePath);
        Assert.Single(reloaded.Servers);
        Assert.Equal(added.Id, reloaded.Servers[0].Id);
        Assert.Equal(added.LastJoinedAt, reloaded.Servers[0].LastJoinedAt);
    }

    [Fact]
    public void Remove_DeletesByIdAndPersists()
    {
        using var store = new TempStore();
        SavedServer added = store.Store.Add("Alpha", "alpha.example.com:25566");

        Assert.True(store.Store.Remove(added.Id));
        Assert.False(store.Store.Remove(added.Id));
        Assert.Empty(store.Store.Servers);
        Assert.Empty(new ServerStore(store.FilePath).Servers);
    }

    [Fact]
    public void Load_MovesACorruptFileAsideInsteadOfDiscardingIt()
    {
        using var store = new TempStore();
        store.Store.Add("Alpha", "alpha.example.com:25566");
        File.WriteAllText(store.FilePath, "{ this is not json");

        var recovered = new ServerStore(store.FilePath);

        Assert.Empty(recovered.Servers);
        Assert.False(File.Exists(store.FilePath));

        string[] quarantined = Directory.GetFiles(store.RootDirectory, "servers.json.corrupt-*");
        Assert.Single(quarantined);
        Assert.Contains("this is not json", File.ReadAllText(quarantined[0]), StringComparison.Ordinal);

        recovered.Add("Beta", "beta.example.com");
        Assert.Single(new ServerStore(store.FilePath).Servers);
    }

    [Fact]
    public void ColdStartReadFailureBlocksSavingSoTheFileIsNotOverwritten()
    {
        using var store = new TempStore();
        store.Store.Add("Alpha", "alpha.example.com:25566");
        string before = File.ReadAllText(store.FilePath);

        ServerStore blocked;
        using (new FileStream(store.FilePath, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
            blocked = new ServerStore(store.FilePath);

        Assert.True(blocked.PersistenceBlocked);

        blocked.Add("Beta", "beta.example.com");
        Assert.Equal(before, File.ReadAllText(store.FilePath));
    }

    [Fact]
    public void Load_KeepsTheLoadedServersWhenTheFileCannotBeRead()
    {        using var store = new TempStore();
        SavedServer alpha = store.Store.Add("Alpha", "alpha.example.com:25566");

        // A transient lock (indexer, antivirus, another launcher) must not empty the list:
        // the next save would rewrite servers.json without the user's servers.
        using (new FileStream(store.FilePath, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
            store.Store.Load();

        Assert.Equal(alpha.Id, Assert.Single(store.Store.Servers).Id);

        store.Store.Add("Beta", "beta.example.com");
        Assert.Equal(2, new ServerStore(store.FilePath).Servers.Count);
    }

    [Fact]
    public void Save_KeepsThePreviousFileWhenTheWriteFails()
    {
        using var store = new TempStore();
        store.Store.Add("Alpha", "alpha.example.com:25566");
        string before = File.ReadAllText(store.FilePath);

        // A directory where the temp file belongs makes the write fail before the replace:
        // the saved list must still be the one on disk, not a truncated rewrite.
        Directory.CreateDirectory(store.FilePath + ".tmp");

        Assert.ThrowsAny<Exception>(() => store.Store.Add("Beta", "beta.example.com"));

        Assert.Equal(before, File.ReadAllText(store.FilePath));
        Assert.Single(new ServerStore(store.FilePath).Servers);
    }

    [Fact]
    public void Save_LeavesNoTempFileBehindWhenTheFinalMoveFails()
    {
        using var store = new TempStore();
        store.Store.Add("Alpha", "alpha.example.com:25566");

        // A directory at the store path makes the temp-to-final move fail after the payload
        // was written, so the leftover .tmp is the only trace a failed save can leave.
        File.Delete(store.FilePath);
        Directory.CreateDirectory(store.FilePath);

        Assert.ThrowsAny<Exception>(() => store.Store.Add("Beta", "beta.example.com"));

        Assert.False(File.Exists(store.FilePath + ".tmp"));
    }

    [Fact]
    public void Load_TreatsAMissingOrNullFileAsEmpty()
    {
        using var store = new TempStore();
        Assert.Empty(store.Store.Servers);

        File.WriteAllText(store.FilePath, "null");
        var nullStore = new ServerStore(store.FilePath);

        Assert.Empty(nullStore.Servers);
        Assert.True(File.Exists(store.FilePath));
    }

    private sealed class TempStore : IDisposable
    {
        public TempStore()
        {
            RootDirectory = Path.Combine(
                Path.GetTempPath(), "aoko-server-store-tests", Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(RootDirectory);
            FilePath = Path.Combine(RootDirectory, "servers.json");
            Store = new ServerStore(FilePath);
        }

        public string RootDirectory { get; }

        public string FilePath { get; }

        public ServerStore Store { get; }

        public void Dispose()
        {
            try
            {
                Directory.Delete(RootDirectory, recursive: true);
            }
            catch (IOException)
            {
            }
        }
    }
}
