using Aoko.Core.Accounts;

namespace Aoko.Tests;

/// <summary>
/// A store may never lose what it already has because one read or one write failed. An
/// antivirus scan holding <c>accounts.json</c> open is enough to hit the read path, and the next
/// mutation would otherwise rewrite the file from an empty list.
/// </summary>
public sealed class AccountStoreFailureTests
{
    private const string NotchUuid = "b50ad385-829d-3141-a216-7e7d7539ba7f";
    private const string SecondUuid = "069a79f444e94726a5befca90e38aaf5";

    [Fact]
    public void Load_KeepsTheCredentialsWhenTheFileCannotBeRead()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        var store = new AccountStore(path);
        StoredAccount saved = store.Upsert(
            new MinecraftProfile("Notch", NotchUuid, "access-token", "M.Crefresh", AccountSources.Localts));

        // A locked file (indexer, another launcher, a backup agent) is the common failure.
        using (new FileStream(path, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
            store.Load();

        StoredAccount kept = Assert.Single(store.Accounts);
        Assert.Equal(saved.Id, kept.Id);
        Assert.Equal("M.Crefresh", kept.RefreshToken);

        // The follow-on risk: a later mutation saving from the empty list. Both accounts must
        // survive on disk.
        store.Upsert(new MinecraftProfile("Second", SecondUuid, "token-2", null, AccountSources.Token));

        Assert.Equal(2, new AccountStore(path).Accounts.Count);
    }

    [Fact]
    public void ColdStartReadFailureBlocksSavingSoTheFileIsNotOverwritten()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        new AccountStore(path).Upsert(
            new MinecraftProfile("Notch", NotchUuid, "access-token", "M.Crefresh", AccountSources.Localts));
        string before = File.ReadAllText(path);

        AccountStore store;
        using (new FileStream(path, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
            store = new AccountStore(path);

        Assert.True(store.PersistenceBlocked);

        // An import while the file is unreadable must not rewrite it from a list we never saw.
        store.Upsert(new MinecraftProfile("Second", SecondUuid, "token-2", null, AccountSources.Token));
        Assert.Equal(before, File.ReadAllText(path));
    }

    [Fact]
    public void Load_KeepsTheCredentialsWhenThePathIsADirectory()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        var store = new AccountStore(path);
        store.Upsert(new MinecraftProfile("Notch", NotchUuid, "access-token", "M.Crefresh", AccountSources.Localts));

        File.Delete(path);
        Directory.CreateDirectory(path);

        store.Load();

        Assert.Single(store.Accounts);
    }

    [Fact]
    public void AccountSettings_KeepsTheLoadedValueWhenTheFileCannotBeRead()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts-settings.json");
        var settings = new AccountSettings(path) { PopulateLunarAccountManager = true };
        settings.Save();

        using (new FileStream(path, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
            settings.Load();

        // Resetting to the default here would silently disable the Lunar mirror on the next save.
        Assert.True(settings.PopulateLunarAccountManager);
    }

    [Fact]
    public void Save_KeepsThePreviousFileWhenTheWriteFails()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        var store = new AccountStore(path);
        store.Upsert(new MinecraftProfile("Notch", NotchUuid, "access-token", "M.Crefresh", AccountSources.Localts));
        string before = File.ReadAllText(path);

        // A directory where the temp file belongs makes the write fail before the replace.
        Directory.CreateDirectory(path + ".tmp");

        Assert.ThrowsAny<Exception>(() =>
            store.Upsert(new MinecraftProfile("Second", SecondUuid, "token-2", null, AccountSources.Token)));

        Assert.Equal(before, File.ReadAllText(path));
        Assert.Equal("M.Crefresh", Assert.Single(new AccountStore(path).Accounts).RefreshToken);
    }

    [Fact]
    public void Save_LeavesNoTempFileBehindWhenTheFinalMoveFails()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        var store = new AccountStore(path);
        store.Upsert(new MinecraftProfile("Notch", NotchUuid, "access-token", "M.Crefresh", AccountSources.Localts));

        // A directory at the store path makes the temp-to-final move fail after the payload was
        // written, so a leftover .tmp is the only trace a failed save can leave.
        File.Delete(path);
        Directory.CreateDirectory(path);

        Assert.ThrowsAny<Exception>(() =>
            store.Upsert(new MinecraftProfile("Second", SecondUuid, "token-2", null, AccountSources.Token)));

        Assert.False(File.Exists(path + ".tmp"));
    }

    [Fact]
    public void PersistRefreshToken_StoresTheRotationWithoutMarkingTheAccountUsed()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        var store = new AccountStore(path);
        StoredAccount account = store.Upsert(
            new MinecraftProfile("Notch", NotchUuid, "access-token", "M.Cstale", AccountSources.Localts));
        Assert.Null(account.LastUsedAt);

        // What the view does the moment the MSA exchange returns, before the bridge is asked to
        // switch: the exchange already invalidated the old token.
        store.PersistRefreshToken(account, "M.Crotated");

        Assert.Equal("M.Crotated", account.RefreshToken);
        Assert.Equal("access-token", account.AccessToken);
        Assert.Null(account.LastUsedAt);

        StoredAccount reloaded = Assert.Single(new AccountStore(path).Accounts);
        Assert.Equal("M.Crotated", reloaded.RefreshToken);
        Assert.Null(reloaded.LastUsedAt);
    }

    [Fact]
    public void PersistRefreshToken_IgnoresABlankRotation()
    {
        using var temp = new TempDirectory();
        var store = new AccountStore(Path.Combine(temp.Path, "accounts.json"));
        StoredAccount account = store.Upsert(
            new MinecraftProfile("Notch", NotchUuid, "access-token", "M.Cstale", AccountSources.Localts));

        store.PersistRefreshToken(account, null);
        store.PersistRefreshToken(account, "   ");

        Assert.Equal("M.Cstale", account.RefreshToken);
    }

    [Fact]
    public void MarkUsed_StampsTheAccountWithoutTouchingItsTokens()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        var store = new AccountStore(path);
        StoredAccount account = store.Upsert(
            new MinecraftProfile("Notch", NotchUuid, "access-token", "M.Crefresh", AccountSources.Localts));

        store.MarkUsed(account);

        Assert.NotNull(account.LastUsedAt);
        Assert.Equal("M.Crefresh", account.RefreshToken);
        Assert.NotNull(Assert.Single(new AccountStore(path).Accounts).LastUsedAt);
    }

    private sealed class TempDirectory : IDisposable
    {
        public TempDirectory()
        {
            Path = System.IO.Path.Combine(
                System.IO.Path.GetTempPath(),
                "aoko-store-failure-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(Path);
        }

        public string Path { get; }

        public void Dispose()
        {
            try
            {
                Directory.Delete(Path, recursive: true);
            }
            catch (IOException)
            {
            }
        }
    }
}
