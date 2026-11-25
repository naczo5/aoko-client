using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.Json;

namespace Aoko.Core.Servers;

/// <summary>
/// JSON-backed saved-server list at <c>%AppData%\Aoko\servers.json</c>. Writes go through a temp
/// file so a crash mid-save cannot truncate the list, and an unreadable file is moved aside
/// instead of being overwritten by the next save.
/// </summary>
public sealed class ServerStore
{
    private const string StoreFolder = "Aoko";
    private const string StoreFileName = "servers.json";
    private const string TempSuffix = ".tmp";
    private const string CorruptPrefix = ".corrupt-";

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase
    };

    private readonly string _path;
    private readonly object _gate = new();
    private readonly object _writeGate = new();
    private readonly List<SavedServer> _servers = new();
    private bool _persistenceBlocked;
    private bool _loadedOnce;

    public ServerStore()
        : this(GetStorePath(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData)))
    {
    }

    internal ServerStore(string path)
    {
        _path = path;
        Load();
    }

    /// <summary>Snapshot of the saved servers; mutated only through this store.</summary>
    public IReadOnlyList<SavedServer> Servers
    {
        get
        {
            lock (_gate)
                return _servers.ToArray();
        }
    }

    /// <summary>
    /// Path of the file this store moved aside because it could not be read, or null.
    /// A recovered store otherwise looks exactly like a fresh install.
    /// </summary>
    public string? RecoveredFrom { get; private set; }

    /// <summary>Where this store reads and writes.</summary>
    public string StorePath => _path;

    /// <summary>
    /// True when the saved file could not be read at all (locked, denied, or a directory on the
    /// path). Saves are skipped while this holds, so an unreadable list is never overwritten.
    /// </summary>
    public bool PersistenceBlocked
    {
        get
        {
            lock (_gate)
                return _persistenceBlocked;
        }
    }

    internal static string GetStoreDirectory(string appData) => Path.Combine(appData, StoreFolder);

    internal static string GetStorePath(string appData) =>
        Path.Combine(GetStoreDirectory(appData), StoreFileName);

    /// <summary>
    /// Reloads from disk. A transient read failure (locked or inaccessible file) keeps the
    /// servers already in memory: the list is only swapped once the read succeeded, so the next
    /// save cannot rewrite <c>servers.json</c> from an empty list.
    /// </summary>
    public void Load()
    {
        lock (_gate)
        {
            if (!TryReadFile(out List<SavedServer> loaded))
            {
                // Only a failed first read means memory cannot be trusted; a later failure
                // leaves the list we already loaded intact.
                if (!_loadedOnce)
                    _persistenceBlocked = true;
                return;
            }

            _loadedOnce = true;
            _persistenceBlocked = false;

            _servers.Clear();
            _servers.AddRange(loaded);
        }
    }

    public void Save()
    {
        List<SavedServer> snapshot;
        lock (_gate)
        {
            if (_persistenceBlocked)
                return;

            snapshot = new List<SavedServer>(_servers);
        }

        WriteAtomic(JsonSerializer.Serialize(snapshot, JsonOptions));
    }

    /// <summary>
    /// Stores a server and returns the entry that represents it. Re-adding a server that already
    /// resolves to the same host and port reuses the existing row, so <c>host</c> and
    /// <c>host:25565</c> stay one entry.
    /// </summary>
    public SavedServer Add(string? name, string address)
    {
        ArgumentNullException.ThrowIfNull(address);

        string trimmed = address.Trim();
        bool parsed = ServerAddress.TryParse(trimmed, out ServerAddress target);
        string stored = parsed ? target.Display : trimmed;
        string resolvedName = string.IsNullOrWhiteSpace(name) ? stored : name.Trim();

        SavedServer entry;
        lock (_gate)
        {
            SavedServer? existing = FindMatch(target, parsed, stored);
            if (existing == null)
            {
                entry = new SavedServer
                {
                    AddedAt = DateTimeOffset.UtcNow,
                    Name = resolvedName,
                    Address = stored
                };
                _servers.Add(entry);
            }
            else
            {
                // A match keeps the bookmark the user saved: a blank name means "join this
                // address", not "rename my bookmark", and the stored address is left alone so
                // re-adding through the address box cannot rewrite it.
                if (!string.IsNullOrWhiteSpace(name) || string.IsNullOrWhiteSpace(existing.Name))
                    existing.Name = resolvedName;

                entry = existing;
            }
        }

        Save();
        return entry;
    }

    public void MarkJoined(SavedServer server)
    {
        ArgumentNullException.ThrowIfNull(server);
        server.LastJoinedAt = DateTimeOffset.UtcNow;
        Save();
    }

    public bool Remove(string id)
    {
        if (string.IsNullOrEmpty(id))
            return false;

        int removed;
        lock (_gate)
            removed = _servers.RemoveAll(s => string.Equals(s.Id, id, StringComparison.Ordinal));

        if (removed == 0)
            return false;

        Save();
        return true;
    }

    private SavedServer? FindMatch(ServerAddress target, bool parsed, string stored)
    {
        foreach (SavedServer existing in _servers)
        {
            if (string.Equals(existing.Address, stored, StringComparison.OrdinalIgnoreCase))
                return existing;

            if (!parsed || !ServerAddress.TryParse(existing.Address, out ServerAddress other))
                continue;

            if (other.Port == target.Port
                && string.Equals(other.Host, target.Host, StringComparison.OrdinalIgnoreCase))
            {
                return existing;
            }
        }

        return null;
    }

    private bool TryReadFile(out List<SavedServer> servers)
    {
        servers = new List<SavedServer>();
        try
        {
            if (Directory.Exists(_path))
                return false;

            if (!File.Exists(_path))
                return true;

            string json = File.ReadAllText(_path);
            List<SavedServer>? parsed = JsonSerializer.Deserialize<List<SavedServer>>(json, JsonOptions);
            if (parsed == null)
                return true;

            foreach (SavedServer server in parsed)
            {
                if (server == null)
                    continue;

                server.Id = string.IsNullOrWhiteSpace(server.Id) ? Guid.NewGuid().ToString("N") : server.Id;
                server.Name ??= "";
                server.Address ??= "";
                servers.Add(server);
            }

            return true;
        }
        catch (Exception ex) when (ex is JsonException or NotSupportedException or ArgumentException)
        {
            // Unreadable payload: move it aside instead of letting the next save overwrite it.
            Quarantine();
            servers = new List<SavedServer>();
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // Unreadable for an environmental reason; keep the file and the loaded list.
            servers = new List<SavedServer>();
            return false;
        }
    }

    private void WriteAtomic(string json)
    {
        string tempPath = _path + TempSuffix;
        lock (_writeGate)
        {
            try
            {
                string? directory = Path.GetDirectoryName(_path);
                if (!string.IsNullOrEmpty(directory))
                    Directory.CreateDirectory(directory);

                using (var stream = new FileStream(tempPath, FileMode.Create, FileAccess.Write, FileShare.None))
                using (var writer = new StreamWriter(stream, new UTF8Encoding(encoderShouldEmitUTF8Identifier: false)))
                {
                    writer.Write(json);
                    writer.Flush();
                    stream.Flush(flushToDisk: true);
                }

                File.Move(tempPath, _path, overwrite: true);
            }
            catch
            {
                TryDelete(tempPath);
                throw;
            }
        }
    }

    private void Quarantine()
    {
        try
        {
            string stamp = DateTime.UtcNow.ToString("yyyyMMddTHHmmssfffZ", CultureInfo.InvariantCulture);
            string quarantine = _path + CorruptPrefix + stamp;
            File.Move(_path, quarantine, overwrite: true);
            RecoveredFrom = quarantine;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // Nothing else to do; the list starts empty either way and the file stays for inspection.
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
        }
    }
}
