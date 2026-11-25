using System;
using System.Globalization;
using System.IO;
using System.Security;
using System.Text.Json;

namespace Aoko.Core.Accounts;

/// <summary>
/// App-level accounts preferences at <c>%AppData%\Aoko\accounts-settings.json</c>.
/// Writes go through a temp file so a crash mid-save cannot truncate the file,
/// an unreadable payload is moved aside instead of deleted, and loading never
/// throws: a bad settings file falls back to defaults and the GUI stays usable.
/// </summary>
internal sealed class AccountSettings
{
    private const string FileName = "accounts-settings.json";

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true
    };

    private readonly object _sync = new();
    private readonly string _path;

    public AccountSettings() : this(DefaultPath)
    {
    }

    public AccountSettings(string path)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);

        _path = path;
        string? directory = Path.GetDirectoryName(_path);
        if (!string.IsNullOrEmpty(directory))
            Directory.CreateDirectory(directory);
        Load();
    }

    public static string DefaultDirectory => AccountStore.DefaultDirectory;

    public static string DefaultPath => Path.Combine(DefaultDirectory, FileName);

    /// <summary>
    /// Mirrors saved accounts into Lunar Client's <c>accounts.json</c> after an
    /// import or switch. Off by default: Lunar owns that file and only reads it
    /// at startup.
    /// </summary>
    public bool PopulateLunarAccountManager { get; set; }

    /// <summary>
    /// Reloads from disk. Never throws; a corrupt file is quarantined, and a transient read
    /// failure keeps the values already loaded so the next save cannot reset them to defaults.
    /// </summary>
    public void Load()
    {
        lock (_sync)
        {
            if (!TryReadFile(out Payload? loaded))
                return;

            PopulateLunarAccountManager = loaded?.PopulateLunarAccountManager ?? false;
        }
    }

    public void Save()
    {
        lock (_sync)
        {
            string json = JsonSerializer.Serialize(
                new Payload { PopulateLunarAccountManager = PopulateLunarAccountManager },
                JsonOptions);
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

    /// <summary>
    /// Reads the file. Returns false only for a failure that says nothing about the stored
    /// payload (locked file, denied access), so the caller keeps the current values. A corrupt
    /// payload is quarantined and reported as no payload.
    /// </summary>
    private bool TryReadFile(out Payload? loaded)
    {
        loaded = null;
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

        try
        {
            loaded = JsonSerializer.Deserialize<Payload>(json, JsonOptions);
        }
        catch (Exception)
        {
            // Keep the unreadable file for the user instead of deleting it.
            QuarantineCorruptFile();
            loaded = null;
        }

        return true;
    }

    private void QuarantineCorruptFile()
    {
        try
        {
            string stamp = DateTimeOffset.UtcNow.ToString("yyyyMMdd-HHmmss", CultureInfo.InvariantCulture);
            File.Move(_path, $"{_path}.corrupt-{stamp}", overwrite: true);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                      or NotSupportedException or ArgumentException)
        {
            // Keep the unreadable settings on disk rather than deleting them.
        }
    }

    /// <summary>On-disk shape. Kept separate so reading never runs the constructor.</summary>
    private sealed class Payload
    {
        public bool PopulateLunarAccountManager { get; set; }
    }
}
