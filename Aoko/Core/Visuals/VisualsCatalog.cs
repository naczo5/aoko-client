using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.IO;
using System.Linq;
using System.Text.Json;

namespace Aoko.Core.Visuals;

public sealed record CapeInfo(string Id, string Name);

/// <param name="Time">Default tick of day from <c>sky.json</c>, or null to follow the server.</param>
public sealed record SkyInfo(string Id, string Name, bool HasSkybox, int? Time);

/// <param name="Slot">Either <c>hat</c> or <c>back</c>.</param>
public sealed record CosmeticInfo(string Id, string Name, string Slot);

/// <summary>Immutable result of one scan of the visuals directory.</summary>
public sealed class VisualsSnapshot
{
    public static VisualsSnapshot Empty { get; } = new(
        ImmutableArray<CapeInfo>.Empty,
        ImmutableArray<SkyInfo>.Empty,
        ImmutableArray<CosmeticInfo>.Empty);

    public VisualsSnapshot(
        ImmutableArray<CapeInfo> capes,
        ImmutableArray<SkyInfo> skies,
        ImmutableArray<CosmeticInfo> cosmetics)
    {
        Capes = capes;
        Skies = skies;
        Cosmetics = cosmetics;
    }

    public ImmutableArray<CapeInfo> Capes { get; }
    public ImmutableArray<SkyInfo> Skies { get; }
    public ImmutableArray<CosmeticInfo> Cosmetics { get; }

    public IEnumerable<CosmeticInfo> CosmeticsForSlot(string slot)
        => Cosmetics.Where(c => string.Equals(c.Slot, slot, StringComparison.Ordinal));

    /// <summary>Value comparison, so callers can skip rebuilding UI lists when nothing changed.</summary>
    public bool SameContentAs(VisualsSnapshot? other)
        => other != null
           && Capes.SequenceEqual(other.Capes)
           && Skies.SequenceEqual(other.Skies)
           && Cosmetics.SequenceEqual(other.Cosmetics);
}

/// <summary>
/// Scans a visuals directory for capes, skies and cosmetics. Reading is lenient: an entry
/// that is missing, oversized or malformed is skipped, and nothing here throws.
/// </summary>
public sealed class VisualsCatalog
{
    public const string HatSlot = "hat";
    public const string BackSlot = "back";

    /// <summary>Largest <c>sky.json</c> / <c>cosmetic.json</c> that will be read.</summary>
    internal const long MaxJsonBytes = 256 * 1024;

    private static readonly JsonDocumentOptions JsonOptions = new()
    {
        AllowTrailingCommas = true,
        CommentHandling = JsonCommentHandling.Skip,
        MaxDepth = 16,
    };

    private readonly object _gate = new();
    private VisualsSnapshot _current = VisualsSnapshot.Empty;

    public VisualsCatalog(string root)
    {
        Root = root;
    }

    public string Root { get; }

    /// <summary>Raised from <see cref="Refresh"/> only when the scan result differs from the previous one.</summary>
    public event Action<VisualsSnapshot>? Changed;

    public VisualsSnapshot Current
    {
        get
        {
            lock (_gate)
                return _current;
        }
    }

    public VisualsSnapshot Refresh()
    {
        VisualsSnapshot scanned = Scan(Root);
        bool changed;
        lock (_gate)
        {
            changed = !_current.SameContentAs(scanned);
            if (changed)
                _current = scanned;
        }

        if (changed)
            Changed?.Invoke(scanned);
        return scanned;
    }

    public static VisualsSnapshot Scan(string directory)
    {
        if (string.IsNullOrWhiteSpace(directory))
            return VisualsSnapshot.Empty;

        try
        {
            return new VisualsSnapshot(
                ScanCapes(Path.Combine(directory, VisualsPaths.CapesFolder)),
                ScanSkies(Path.Combine(directory, VisualsPaths.SkiesFolder)),
                ScanCosmetics(Path.Combine(directory, VisualsPaths.CosmeticsFolder)));
        }
        catch (Exception)
        {
            return VisualsSnapshot.Empty;
        }
    }

    private static ImmutableArray<CapeInfo> ScanCapes(string folder)
    {
        var capes = ImmutableArray.CreateBuilder<CapeInfo>();
        foreach (string file in EnumerateFiles(folder, "*.png"))
        {
            string id = Path.GetFileNameWithoutExtension(file);
            if (!VisualsIds.IsValid(id) || !file.EndsWith(".png", StringComparison.OrdinalIgnoreCase))
                continue;
            capes.Add(new CapeInfo(id, VisualsIds.DisplayNameFromId(id)));
        }

        capes.Sort((a, b) => string.CompareOrdinal(a.Id, b.Id));
        return capes.ToImmutable();
    }

    private static ImmutableArray<SkyInfo> ScanSkies(string folder)
    {
        var skies = ImmutableArray.CreateBuilder<SkyInfo>();
        foreach (string dir in EnumerateDirectories(folder))
        {
            string id = Path.GetFileName(dir);
            if (!VisualsIds.IsValid(id))
                continue;

            using JsonDocument? doc = ReadJson(Path.Combine(dir, "sky.json"));
            if (doc == null)
                continue;

            JsonElement root = doc.RootElement;
            string? name = ReadName(root);
            if (name == null)
                continue;

            skies.Add(new SkyInfo(id, name, HasSkybox(root, dir), ReadTime(root)));
        }

        skies.Sort((a, b) => string.CompareOrdinal(a.Id, b.Id));
        return skies.ToImmutable();
    }

    private static ImmutableArray<CosmeticInfo> ScanCosmetics(string folder)
    {
        var cosmetics = ImmutableArray.CreateBuilder<CosmeticInfo>();
        foreach (string dir in EnumerateDirectories(folder))
        {
            string id = Path.GetFileName(dir);
            if (!VisualsIds.IsValid(id))
                continue;

            using JsonDocument? doc = ReadJson(Path.Combine(dir, "cosmetic.json"));
            if (doc == null)
                continue;

            JsonElement root = doc.RootElement;
            string? name = ReadName(root);
            if (name == null)
                continue;

            string? slot = ReadString(root, "slot");
            if (slot != HatSlot && slot != BackSlot)
                continue;

            cosmetics.Add(new CosmeticInfo(id, name, slot));
        }

        cosmetics.Sort((a, b) => string.CompareOrdinal(a.Id, b.Id));
        return cosmetics.ToImmutable();
    }

    private static string? ReadName(JsonElement root)
    {
        string? name = ReadString(root, "name")?.Trim();
        if (string.IsNullOrEmpty(name) || name.Length > VisualsIds.MaxNameLength)
            return null;
        return name;
    }

    private static int? ReadTime(JsonElement root)
    {
        if (root.TryGetProperty("time", out JsonElement time)
            && time.ValueKind == JsonValueKind.Number
            && time.TryGetInt32(out int ticks)
            && ticks is >= 0 and <= 23999)
        {
            return ticks;
        }

        return null;
    }

    private static bool HasSkybox(JsonElement root, string skyDirectory)
    {
        if (!root.TryGetProperty("skybox", out JsonElement skybox) || skybox.ValueKind != JsonValueKind.Object)
            return false;

        string texture = ReadString(skybox, "texture")?.Trim() ?? "sky.jpg";
        if (texture.Length == 0
            || texture.Length > VisualsIds.MaxNameLength
            || texture != Path.GetFileName(texture))
        {
            return false;
        }

        try
        {
            return File.Exists(Path.Combine(skyDirectory, texture));
        }
        catch (Exception)
        {
            return false;
        }
    }

    private static string? ReadString(JsonElement element, string property)
    {
        if (element.ValueKind == JsonValueKind.Object
            && element.TryGetProperty(property, out JsonElement value)
            && value.ValueKind == JsonValueKind.String)
        {
            return value.GetString();
        }

        return null;
    }

    private static JsonDocument? ReadJson(string path)
    {
        try
        {
            var info = new FileInfo(path);
            if (!info.Exists || info.Length > MaxJsonBytes)
                return null;

            using FileStream stream = new(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
            JsonDocument doc = JsonDocument.Parse(stream, JsonOptions);
            if (doc.RootElement.ValueKind == JsonValueKind.Object)
                return doc;

            doc.Dispose();
            return null;
        }
        catch (Exception)
        {
            return null;
        }
    }

    private static string[] EnumerateFiles(string folder, string pattern)
    {
        try
        {
            return Directory.Exists(folder)
                ? Directory.GetFiles(folder, pattern)
                : [];
        }
        catch (Exception)
        {
            return [];
        }
    }

    private static string[] EnumerateDirectories(string folder)
    {
        try
        {
            return Directory.Exists(folder)
                ? Directory.GetDirectories(folder)
                : [];
        }
        catch (Exception)
        {
            return [];
        }
    }
}
