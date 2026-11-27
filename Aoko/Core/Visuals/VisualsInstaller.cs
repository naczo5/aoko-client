using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json;

namespace Aoko.Core.Visuals;

/// <summary>Source of the built-in assets. Production reads the embedded manifest resources.</summary>
public interface IVisualsResourceReader
{
    /// <summary>Resource names, each <c>Aoko.Visuals/&lt;relative path with forward slashes&gt;</c>.</summary>
    IReadOnlyList<string> GetResourceNames();

    /// <summary>Returns the content of one resource, or null when it cannot be read.</summary>
    byte[]? Read(string resourceName);
}

public sealed class AssemblyVisualsResourceReader : IVisualsResourceReader
{
    private readonly Assembly _assembly;

    public AssemblyVisualsResourceReader(Assembly assembly)
    {
        _assembly = assembly;
    }

    public IReadOnlyList<string> GetResourceNames() => _assembly.GetManifestResourceNames();

    public byte[]? Read(string resourceName)
    {
        using Stream? stream = _assembly.GetManifestResourceStream(resourceName);
        if (stream == null)
            return null;

        using var buffer = new MemoryStream();
        stream.CopyTo(buffer);
        return buffer.ToArray();
    }
}

public sealed class VisualsInstallResult
{
    private const int MaxErrors = 20;
    private readonly List<string> _errors = new();

    /// <summary>True when nothing failed. Files left alone because the user edited them are not failures.</summary>
    public bool Success => _errors.Count == 0;

    public int Installed { get; internal set; }
    public int Updated { get; internal set; }
    public int UpToDate { get; internal set; }
    public int LeftAlone { get; internal set; }
    public int Failed { get; internal set; }
    public IReadOnlyList<string> Errors => _errors;

    internal void AddError(string message)
    {
        Failed++;
        if (_errors.Count < MaxErrors)
            _errors.Add(message);
    }
}

/// <summary>
/// Copies the embedded built-in visuals into the visuals directory. A built-in file is rewritten
/// only when its hash differs from the embedded copy and the file on disk still matches the hash
/// recorded in <c>.builtin.json</c> (so the user did not edit it). Files the user adds are never
/// touched. Safe to call on every start; it reports failures in the result instead of throwing.
/// </summary>
public sealed class VisualsInstaller
{
    public const string ResourcePrefix = "Aoko.Visuals/";
    public const string ManifestFileName = ".builtin.json";

    private static readonly string[] AssetFolders =
    [
        VisualsPaths.CapesFolder,
        VisualsPaths.SkiesFolder,
        VisualsPaths.CosmeticsFolder,
    ];

    private readonly IVisualsResourceReader _reader;

    public VisualsInstaller(IVisualsResourceReader reader)
    {
        _reader = reader;
    }

    /// <summary>Installs the built-ins embedded in <c>Aoko.exe</c> into <see cref="VisualsPaths.Root"/> unless a directory is given.</summary>
    public static VisualsInstallResult InstallEmbedded(string? visualsDirectory = null)
        => new VisualsInstaller(new AssemblyVisualsResourceReader(typeof(VisualsInstaller).Assembly))
            .Install(visualsDirectory ?? VisualsPaths.Root);

    public VisualsInstallResult Install(string visualsDirectory)
    {
        var result = new VisualsInstallResult();
        try
        {
            InstallCore(visualsDirectory, result);
        }
        catch (Exception ex)
        {
            result.AddError($"Visuals install failed: {ex.Message}");
        }

        return result;
    }

    private void InstallCore(string root, VisualsInstallResult result)
    {
        if (string.IsNullOrWhiteSpace(root))
        {
            result.AddError("Visuals directory is not set.");
            return;
        }

        root = Path.GetFullPath(root);
        Directory.CreateDirectory(root);
        foreach (string folder in AssetFolders)
            Directory.CreateDirectory(Path.Combine(root, folder));

        var builtIns = ListBuiltIns();
        if (builtIns.Count == 0)
            return;

        string manifestPath = Path.Combine(root, ManifestFileName);
        Dictionary<string, string> recorded = ReadManifest(manifestPath);
        bool manifestChanged = false;

        foreach ((string relative, string resourceName) in builtIns)
        {
            try
            {
                byte[]? content = _reader.Read(resourceName);
                if (content == null)
                {
                    result.AddError($"Could not read built-in '{relative}'.");
                    continue;
                }

                string destination = Path.GetFullPath(Path.Combine(root, relative.Replace('/', Path.DirectorySeparatorChar)));
                if (!IsInside(root, destination))
                {
                    result.AddError($"Built-in '{relative}' points outside the visuals directory.");
                    continue;
                }

                string embeddedHash = Hash(content);
                recorded.TryGetValue(relative, out string? recordedHash);

                if (!File.Exists(destination))
                {
                    WriteAtomic(destination, content);
                    result.Installed++;
                }
                else
                {
                    string diskHash = Hash(File.ReadAllBytes(destination));
                    if (diskHash == embeddedHash)
                    {
                        result.UpToDate++;
                    }
                    else if (recordedHash != null && diskHash == recordedHash)
                    {
                        WriteAtomic(destination, content);
                        result.Updated++;
                    }
                    else
                    {
                        // The user edited this file, or owns a file that shares a built-in's name.
                        result.LeftAlone++;
                        continue;
                    }
                }

                if (recordedHash != embeddedHash)
                {
                    recorded[relative] = embeddedHash;
                    manifestChanged = true;
                }
            }
            catch (Exception ex)
            {
                result.AddError($"Could not install '{relative}': {ex.Message}");
            }
        }

        if (manifestChanged)
        {
            try
            {
                WriteAtomic(manifestPath, JsonSerializer.SerializeToUtf8Bytes(
                    new SortedDictionary<string, string>(recorded, StringComparer.Ordinal),
                    new JsonSerializerOptions { WriteIndented = true }));
            }
            catch (Exception ex)
            {
                result.AddError($"Could not write {ManifestFileName}: {ex.Message}");
            }
        }
    }

    private List<(string Relative, string ResourceName)> ListBuiltIns()
    {
        var list = new List<(string, string)>();
        foreach (string name in _reader.GetResourceNames())
        {
            if (!name.StartsWith(ResourcePrefix, StringComparison.Ordinal))
                continue;

            string relative = name[ResourcePrefix.Length..];
            if (relative.Length == 0 || relative.EndsWith('/') || relative.Contains('\\'))
                continue;
            if (relative.Split('/').Any(part => part.Length == 0 || part == "." || part == ".."))
                continue;

            list.Add((relative, name));
        }

        list.Sort((a, b) => string.CompareOrdinal(a.Item1, b.Item1));
        return list;
    }

    private static Dictionary<string, string> ReadManifest(string path)
    {
        try
        {
            if (!File.Exists(path))
                return new Dictionary<string, string>(StringComparer.Ordinal);

            var parsed = JsonSerializer.Deserialize<Dictionary<string, string>>(File.ReadAllBytes(path));
            return parsed == null
                ? new Dictionary<string, string>(StringComparer.Ordinal)
                : new Dictionary<string, string>(
                    parsed.Where(p => p.Value != null),
                    StringComparer.Ordinal);
        }
        catch (Exception)
        {
            // An unreadable manifest means "nothing recorded": existing files that differ stay untouched.
            return new Dictionary<string, string>(StringComparer.Ordinal);
        }
    }

    private static void WriteAtomic(string destination, byte[] content)
    {
        string? folder = Path.GetDirectoryName(destination);
        if (!string.IsNullOrEmpty(folder))
            Directory.CreateDirectory(folder);

        string temp = destination + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            File.WriteAllBytes(temp, content);
            File.Move(temp, destination, overwrite: true);
        }
        catch
        {
            try { File.Delete(temp); } catch (Exception) { /* best effort */ }
            throw;
        }
    }

    private static string Hash(byte[] content) => Convert.ToHexString(SHA256.HashData(content)).ToLowerInvariant();

    private static bool IsInside(string root, string path)
    {
        string prefix = root.EndsWith(Path.DirectorySeparatorChar) ? root : root + Path.DirectorySeparatorChar;
        return path.StartsWith(prefix, StringComparison.OrdinalIgnoreCase);
    }
}
