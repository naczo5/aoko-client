using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Security;
using System.Text.Json;

namespace Aoko.Core.Accounts;

/// <summary>
/// Persists saved logins at <c>%AppData%\Aoko\accounts.json</c>. Writes go
/// through a temp file so a crash mid-save cannot truncate the credentials, and
/// an unreadable file is renamed aside instead of being discarded.
/// </summary>
internal sealed class AccountStore
{
    private const string FileName = "accounts.json";

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase
    };

    private readonly object _sync = new();
    private readonly string _path;
    private readonly List<StoredAccount> _accounts = new();
    private bool _persistenceBlocked;
    private bool _loadedOnce;

    /// <summary>
    /// Path of the file this store moved aside because it could not be read, or null.
    /// A recovered store otherwise looks exactly like a fresh install, so the GUI shows
    /// this to explain where the saved credentials went.
    /// </summary>
    public string? RecoveredFrom { get; private set; }

    /// <summary>Where this store reads and writes; the GUI links the folder from here.</summary>
    public string StorePath => _path;

    /// <summary>
    /// True when the saved file could not be read at all (locked, denied, or a directory on the
    /// path). Saves are skipped while this holds: writing the in-memory list over a file the user
    /// still has would destroy credentials we could not see.
    /// </summary>
    public bool PersistenceBlocked
    {
        get
        {
            lock (_sync)
                return _persistenceBlocked;
        }
    }

    public AccountStore() : this(DefaultPath)
    {
    }

    public AccountStore(string path)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);

        _path = path;
        string? directory = Path.GetDirectoryName(_path);
        if (!string.IsNullOrEmpty(directory))
            Directory.CreateDirectory(directory);
        Load();
    }

    public static string DefaultDirectory => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
        "Aoko");

    public static string DefaultPath => Path.Combine(DefaultDirectory, FileName);

    /// <summary>
    /// Snapshot of the saved accounts. The array is a copy, the entries are live:
    /// write them through this store (<see cref="Upsert"/>, <see cref="UpdateTokens"/>,
    /// <see cref="PersistRefreshToken"/>) so the change stays under the store lock.
    /// </summary>
    public IReadOnlyList<StoredAccount> Accounts
    {
        get
        {
            lock (_sync)
                return _accounts.ToArray();
        }
    }

    /// <summary>
    /// Reloads from disk. A corrupt file is quarantined, never silently dropped, and
    /// a transient read failure leaves the credentials already in memory untouched:
    /// the list is only swapped once the read succeeded, so the next save cannot
    /// rewrite <c>accounts.json</c> from an empty list.
    /// </summary>
    public void Load()
    {
        lock (_sync)
        {
            if (!TryReadFile(out List<StoredAccount> loaded))
            {
                // A failure after a good read leaves a trustworthy list in memory. A failure on the
                // very first read does not: memory is empty only because the file was unreadable,
                // so saving from here would destroy the credentials we could not see.
                if (!_loadedOnce)
                    _persistenceBlocked = true;
                return;
            }

            _loadedOnce = true;
            _persistenceBlocked = false;
            _accounts.Clear();
            _accounts.AddRange(loaded);
        }
    }

    public void Save()
    {
        lock (_sync)
            SaveCore();
    }

    public StoredAccount Upsert(MinecraftProfile profile, string source)
    {
        ArgumentNullException.ThrowIfNull(profile);

        lock (_sync)
        {
            // Offline entries match by UUID only, so a cracked username can never
            // overwrite a Microsoft/cookie account with the same name. The reverse
            // direction is accepted risk (inherited from the reference): importing a
            // premium account whose name matches an existing cracked row rewrites that
            // row instead of adding a second entry.
            bool offline = string.Equals(source, AccountSources.Offline, StringComparison.OrdinalIgnoreCase);
            StoredAccount? existing = _accounts.FirstOrDefault(a =>
                SameUuid(a.Uuid, profile.Uuid)
                || (!offline && string.Equals(a.Name, profile.Name, StringComparison.OrdinalIgnoreCase)));

            if (existing == null)
            {
                existing = new StoredAccount();
                _accounts.Add(existing);
            }

            existing.Name = profile.Name;
            existing.Uuid = profile.Uuid;
            existing.AccessToken = profile.AccessToken;
            if (!string.IsNullOrWhiteSpace(profile.RefreshToken))
                existing.RefreshToken = profile.RefreshToken;

            // Never demote a refresh-backed (Localts) entry to a refresh-less
            // token/cookie source when the same account is re-imported via MCA.
            bool incomingHasRefresh = !string.IsNullOrWhiteSpace(profile.RefreshToken);
            if (incomingHasRefresh || !existing.HasRefresh)
                existing.Source = source;

            SaveCore();
            return existing;
        }
    }

    public StoredAccount Upsert(MinecraftProfile profile) => Upsert(profile, profile.Source);

    /// <summary>
    /// Rewrites the tokens of a saved account after a successful switch. Callers that
    /// refreshed the credentials must use <see cref="PersistRefreshToken"/> instead
    /// until the switch succeeded: the MSA exchange already invalidated the old token.
    /// </summary>
    public void UpdateTokens(StoredAccount account, MinecraftProfile profile)
    {
        ArgumentNullException.ThrowIfNull(account);
        ArgumentNullException.ThrowIfNull(profile);

        lock (_sync)
        {
            if (!_accounts.Contains(account))
                _accounts.Add(account);

            account.Name = profile.Name;
            account.Uuid = profile.Uuid;
            account.AccessToken = profile.AccessToken;
            if (!string.IsNullOrWhiteSpace(profile.RefreshToken))
                account.RefreshToken = profile.RefreshToken;
            account.LastUsedAt = DateTimeOffset.UtcNow;
            SaveCore();
        }
    }

    /// <summary>
    /// Stores the rotated MSA refresh token the moment authentication returned it,
    /// before the switch is attempted. The token exchange invalidates the previous
    /// refresh token, so a failed or cancelled switch must not keep the stale one on
    /// disk: the saved account would no longer be refreshable.
    /// </summary>
    public void PersistRefreshToken(StoredAccount account, string? refreshToken)
    {
        ArgumentNullException.ThrowIfNull(account);
        if (string.IsNullOrWhiteSpace(refreshToken))
            return;

        lock (_sync)
        {
            if (!_accounts.Contains(account))
                _accounts.Add(account);

            if (string.Equals(account.RefreshToken, refreshToken, StringComparison.Ordinal))
                return;

            account.RefreshToken = refreshToken;
            SaveCore();
        }
    }

    /// <summary>Stamps the account as the one last switched to.</summary>
    public void MarkUsed(StoredAccount account)
    {
        ArgumentNullException.ThrowIfNull(account);

        lock (_sync)
        {
            if (!_accounts.Contains(account))
                _accounts.Add(account);

            account.LastUsedAt = DateTimeOffset.UtcNow;
            SaveCore();
        }
    }

    public bool Remove(string id)
    {
        lock (_sync)
        {
            int removed = _accounts.RemoveAll(a => string.Equals(a.Id, id, StringComparison.Ordinal));
            if (removed == 0)
                return false;

            SaveCore();
            return true;
        }
    }

    internal static bool SameUuid(string? left, string? right) =>
        !string.IsNullOrWhiteSpace(left)
        && !string.IsNullOrWhiteSpace(right)
        && left.Replace("-", "", StringComparison.Ordinal).Equals(
            right.Replace("-", "", StringComparison.Ordinal),
            StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Reads and parses the file. Returns false only for a failure that says nothing
    /// about the stored payload (locked file, denied access), so the caller keeps the
    /// accounts it already has. A corrupt payload is quarantined and reported as an
    /// empty list.
    /// </summary>
    private bool TryReadFile(out List<StoredAccount> accounts)
    {
        accounts = new List<StoredAccount>();
        if (Directory.Exists(_path))
            return false;

        if (!File.Exists(_path))
            return true;

        string json;
        try
        {
            json = File.ReadAllText(_path);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                      or NotSupportedException or ArgumentException or SecurityException)
        {
            return false;
        }

        List<StoredAccount>? loaded;
        try
        {
            loaded = JsonSerializer.Deserialize<List<StoredAccount>>(json, JsonOptions);
        }
        catch (Exception)
        {
            // Unreadable payload: keep the file for the user instead of
            // silently discarding saved credentials.
            QuarantineCorruptFile();
            return true;
        }

        if (loaded == null)
            return true;

        foreach (StoredAccount account in loaded)
        {
            if (account != null)
                accounts.Add(account);
        }

        return true;
    }

    private void SaveCore()
    {
        if (_persistenceBlocked)
            return;

        string json = JsonSerializer.Serialize(_accounts, JsonOptions);
        string temp = _path + ".tmp";
        try
        {
            File.WriteAllText(temp, json);

            if (!File.Exists(_path))
            {
                File.Move(temp, _path);
                return;
            }

            try
            {
                File.Replace(temp, _path, destinationBackupFileName: null);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or PlatformNotSupportedException)
            {
                // Some file systems refuse File.Replace; a plain overwrite keeps the
                // file complete and drops only the atomic swap.
                File.Move(temp, _path, overwrite: true);
            }
        }
        catch
        {
            // A failed save must not leave the temp file behind for the next one to trip over.
            TryDelete(temp);
            throw;
        }
    }

    private static void TryDelete(string path)
    {
        try
        {
            if (File.Exists(path))
                File.Delete(path);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // Nothing else to do: the caller sees the original failure.
        }
    }

    private void QuarantineCorruptFile()
    {
        try
        {
            string stamp = DateTimeOffset.UtcNow.ToString("yyyyMMdd-HHmmss", CultureInfo.InvariantCulture);
            string quarantine = $"{_path}.corrupt-{stamp}";
            File.Move(_path, quarantine, overwrite: true);
            RecoveredFrom = quarantine;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                      or NotSupportedException or ArgumentException)
        {
            // Keep the unreadable credentials on disk rather than deleting them.
        }
    }
}
