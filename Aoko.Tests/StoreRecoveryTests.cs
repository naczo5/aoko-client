using System;
using System.IO;
using Aoko.Views;

namespace Aoko.Tests;

/// <summary>
/// The recovered-from-corruption notice: driven by an optional store property when
/// it exists, and by this session's quarantine file until then. An old copy must
/// never raise the notice on a later launch.
/// </summary>
public class StoreRecoveryTests
{
    [Fact]
    public void Message_NamesTheStoreAndTheCopy()
    {
        string path = Path.Combine("C:", "Users", "steve", "AppData", "Aoko", "accounts.json.corrupt-20240102");

        Assert.Equal(
            "accounts.json could not be read. A copy was kept as accounts.json.corrupt-20240102.",
            StoreRecovery.Message(path));
    }

    [Fact]
    public void FromStore_ReadsTheOptionalHook()
    {
        Assert.Equal(
            "servers.json could not be read. A copy was kept as servers.json.corrupt-7.",
            StoreRecovery.FromStore(new FakeStore { RecoveredFrom = @"C:\Aoko\servers.json.corrupt-7" }));

        Assert.Null(StoreRecovery.FromStore(new FakeStore()));
        Assert.Null(StoreRecovery.FromStore(new object()));
        Assert.Null(StoreRecovery.FromStore(null));
    }

    [Fact]
    public void FindQuarantined_OnlyCountsFilesFromThisSession()
    {
        using var temp = new TempDirectory();
        string file = Path.Combine(temp.Path, "accounts.json.corrupt-9");
        File.WriteAllText(file, "{}");

        DateTime now = DateTime.UtcNow;
        Assert.Equal(
            "accounts.json could not be read. A copy was kept as accounts.json.corrupt-9.",
            StoreRecovery.FindQuarantined(temp.Path, StoreRecovery.AccountsFileName, now.AddMinutes(-1)));

        // A copy written before this process started belongs to an older run.
        Assert.Null(StoreRecovery.FindQuarantined(temp.Path, StoreRecovery.AccountsFileName, now.AddMinutes(1)));
    }

    [Fact]
    public void FindQuarantined_IgnoresOtherStoresAndMissingFolders()
    {
        using var temp = new TempDirectory();
        File.WriteAllText(Path.Combine(temp.Path, "servers.json.corrupt-9"), "{}");

        Assert.Null(StoreRecovery.FindQuarantined(temp.Path, StoreRecovery.AccountsFileName, DateTime.UtcNow.AddMinutes(-1)));
        Assert.Null(StoreRecovery.FindQuarantined(
            Path.Combine(temp.Path, "missing"), StoreRecovery.ServersFileName, DateTime.UtcNow.AddMinutes(-1)));
    }

    [Fact]
    public void Notice_PrefersTheStoreHookOverTheFileSystem()
    {
        using var temp = new TempDirectory();
        File.WriteAllText(Path.Combine(temp.Path, "servers.json.corrupt-9"), "{}");

        Assert.Equal(
            "servers.json could not be read. A copy was kept as servers.json.corrupt-hook.",
            StoreRecovery.Notice(
                temp.Path,
                StoreRecovery.ServersFileName,
                new FakeStore { RecoveredFrom = "servers.json.corrupt-hook" },
                DateTime.UtcNow.AddMinutes(-1)));
    }

    private sealed class FakeStore
    {
        public string? RecoveredFrom { get; init; }
    }

    private sealed class TempDirectory : IDisposable
    {
        public TempDirectory()
        {
            Path = System.IO.Path.Combine(
                System.IO.Path.GetTempPath(),
                "aoko-store-recovery-" + Guid.NewGuid().ToString("N"));
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
                // Test cleanup only.
            }
            catch (UnauthorizedAccessException)
            {
                // Test cleanup only.
            }
        }
    }
}
