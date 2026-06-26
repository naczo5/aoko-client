using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Text;

namespace Aoko.Core;

/// <summary>
/// A single tracked block type for the Block ESP / X-ray module.
/// <see cref="RegistryId"/> is a Minecraft block identifier (e.g. <c>minecraft:diamond_ore</c>);
/// <see cref="ColorHex"/> is a 6-digit RRGGBB hex string (no leading '#').
/// Implements <see cref="INotifyPropertyChanged"/> so per-row edits in the GUI propagate live.
/// </summary>
public sealed class BlockEspTarget : INotifyPropertyChanged
{
    private string _registryId = "";
    private string _displayName = "";
    private string _colorHex = BlockEspConfig.DefaultColorHex;
    private bool _enabled = true;

    public BlockEspTarget() { }

    public BlockEspTarget(string registryId, string displayName, string colorHex, bool enabled)
    {
        _registryId = registryId ?? "";
        _displayName = displayName ?? "";
        _colorHex = BlockEspConfig.NormalizeColor(colorHex);
        _enabled = enabled;
    }

    public string RegistryId
    {
        get => _registryId;
        set { _registryId = value ?? ""; OnPropertyChanged(nameof(RegistryId)); }
    }

    /// <summary>Friendly label shown in the GUI. Falls back to the registry id when empty.</summary>
    public string DisplayName
    {
        get => string.IsNullOrWhiteSpace(_displayName) ? _registryId : _displayName;
        set { _displayName = value ?? ""; OnPropertyChanged(nameof(DisplayName)); }
    }

    public string ColorHex
    {
        get => _colorHex;
        set
        {
            string normalized = BlockEspConfig.NormalizeColor(value);
            if (_colorHex != normalized)
            {
                _colorHex = normalized;
                OnPropertyChanged(nameof(ColorHex));
            }
        }
    }

    public bool Enabled
    {
        get => _enabled;
        set
        {
            if (_enabled != value)
            {
                _enabled = value;
                OnPropertyChanged(nameof(Enabled));
            }
        }
    }

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged(string name) => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}

/// <summary>
/// Plain serializable DTO for persisting a Block ESP target inside a <c>Profile</c>.
/// Kept separate from <see cref="BlockEspTarget"/> (which is bindable/observable) so JSON
/// round-trips cleanly without change-notification plumbing.
/// </summary>
public sealed class BlockEspTargetData
{
    public string RegistryId { get; set; } = "";
    public string DisplayName { get; set; } = "";
    public string ColorHex { get; set; } = BlockEspConfig.DefaultColorHex;
    public bool Enabled { get; set; } = true;

    public static List<BlockEspTargetData> BuildDefaults()
    {
        var list = new List<BlockEspTargetData>();
        foreach (BlockEspTarget t in BlockEspPresets.BuildDefaultTargets())
            list.Add(FromTarget(t));
        return list;
    }

    public static BlockEspTargetData FromTarget(BlockEspTarget t) => new()
    {
        RegistryId = t.RegistryId,
        DisplayName = t.DisplayName,
        ColorHex = t.ColorHex,
        Enabled = t.Enabled,
    };

    public BlockEspTarget ToTarget() => new(RegistryId, DisplayName, ColorHex, Enabled);
}

/// <summary>
/// Curated catalog of common blocks/ores offered as presets in the Block ESP GUI.
/// </summary>
public static class BlockEspPresets
{
    public readonly record struct Preset(string DisplayName, string RegistryId, string ColorHex);

    /// <summary>All presets, in display order.</summary>
    public static readonly IReadOnlyList<Preset> All = new[]
    {
        new Preset("Diamond Ore",            "minecraft:diamond_ore",            "00E5FF"),
        new Preset("Deepslate Diamond Ore",  "minecraft:deepslate_diamond_ore",  "00E5FF"),
        new Preset("Emerald Ore",            "minecraft:emerald_ore",            "2BE07A"),
        new Preset("Deepslate Emerald Ore",  "minecraft:deepslate_emerald_ore",  "2BE07A"),
        new Preset("Gold Ore",               "minecraft:gold_ore",               "FFD700"),
        new Preset("Deepslate Gold Ore",     "minecraft:deepslate_gold_ore",     "FFD700"),
        new Preset("Iron Ore",               "minecraft:iron_ore",               "D8AF93"),
        new Preset("Deepslate Iron Ore",     "minecraft:deepslate_iron_ore",     "D8AF93"),
        new Preset("Redstone Ore",           "minecraft:redstone_ore",           "FF4040"),
        new Preset("Deepslate Redstone Ore", "minecraft:deepslate_redstone_ore", "FF4040"),
        new Preset("Lapis Ore",              "minecraft:lapis_ore",              "2D5BFF"),
        new Preset("Deepslate Lapis Ore",    "minecraft:deepslate_lapis_ore",    "2D5BFF"),
        new Preset("Coal Ore",               "minecraft:coal_ore",               "404040"),
        new Preset("Deepslate Coal Ore",     "minecraft:deepslate_coal_ore",     "404040"),
        new Preset("Copper Ore",             "minecraft:copper_ore",             "E08552"),
        new Preset("Deepslate Copper Ore",   "minecraft:deepslate_copper_ore",   "E08552"),
        new Preset("Ancient Debris",         "minecraft:ancient_debris",         "9B6A4A"),
        new Preset("Nether Gold Ore",        "minecraft:nether_gold_ore",        "FFD700"),
        new Preset("Nether Quartz Ore",      "minecraft:nether_quartz_ore",      "EDE5DC"),
        new Preset("Spawner",                "minecraft:spawner",                "C03030"),
    };

    /// <summary>
    /// Builds the default working list: every preset present, but only Diamond Ore enabled.
    /// </summary>
    public static List<BlockEspTarget> BuildDefaultTargets()
    {
        var list = new List<BlockEspTarget>(All.Count);
        foreach (Preset p in All)
        {
            bool enabled = string.Equals(p.RegistryId, "minecraft:diamond_ore", StringComparison.OrdinalIgnoreCase);
            list.Add(new BlockEspTarget(p.RegistryId, p.DisplayName, p.ColorHex, enabled));
        }
        return list;
    }
}

/// <summary>
/// Serialization + normalization helpers shared by the loader and (conceptually) the bridge.
/// The bridge config transport uses a naive string parser that cannot handle JSON arrays, so the
/// enabled block list is encoded as a single delimited string: <c>id=RRGGBB;id=RRGGBB</c>.
/// Entries are separated by ';' and the id/color by '='; block ids never contain '=' or ';'.
/// </summary>
public static class BlockEspConfig
{
    public const string DefaultColorHex = "00E5FF";

    /// <summary>Hard cap on the encoded string length to bound the TCP payload.</summary>
    public const int MaxSerializedLength = 4000;

    /// <summary>
    /// Reduces any block identifier form to a lowercase path token suitable for matching:
    /// <list type="bullet">
    /// <item><c>minecraft:diamond_ore</c> -&gt; <c>diamond_ore</c></item>
    /// <item><c>diamond_ore</c> -&gt; <c>diamond_ore</c></item>
    /// <item><c>block.minecraft.diamond_ore</c> -&gt; <c>diamond_ore</c></item>
    /// </list>
    /// Returns an empty string when nothing valid remains.
    /// </summary>
    public static string NormalizeId(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
            return "";

        string value = raw.Trim().ToLowerInvariant();

        // Translation-key form: "block.<namespace>.<path>" -> drop the "block." prefix.
        if (value.StartsWith("block.", StringComparison.Ordinal))
            value = value.Substring("block.".Length);

        // Namespaced identifier "namespace:path" -> keep path.
        int colon = value.IndexOf(':');
        if (colon >= 0)
            value = value.Substring(colon + 1);
        else
        {
            // After stripping "block.", a translation key looks like "minecraft.diamond_ore".
            // Drop the leading namespace component before the first dot.
            int dot = value.IndexOf('.');
            if (dot >= 0)
                value = value.Substring(dot + 1);
        }

        // Keep only [a-z0-9_]; '.' (sub-paths) collapse to '_'.
        var sb = new StringBuilder(value.Length);
        foreach (char c in value)
        {
            if ((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_')
                sb.Append(c);
            else if (c == '.' || c == '/')
                sb.Append('_');
        }

        return sb.ToString().Trim('_');
    }

    /// <summary>
    /// Normalizes a color to a 6-digit uppercase RRGGBB hex string (no '#').
    /// Accepts a leading '#'. Returns <see cref="DefaultColorHex"/> when invalid.
    /// </summary>
    public static string NormalizeColor(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
            return DefaultColorHex;

        string value = raw.Trim();
        if (value.StartsWith("#", StringComparison.Ordinal))
            value = value.Substring(1);

        if (value.Length != 6)
            return DefaultColorHex;

        foreach (char c in value)
        {
            bool hex = (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
            if (!hex)
                return DefaultColorHex;
        }

        return value.ToUpperInvariant();
    }

    /// <summary>
    /// Serializes the enabled targets to the delimited wire string <c>id=RRGGBB;id=RRGGBB</c>.
    /// Disabled entries are skipped; ids are normalized and de-duplicated; output is length-capped.
    /// </summary>
    public static string Serialize(IEnumerable<BlockEspTarget> targets)
    {
        if (targets == null)
            return "";

        var seen = new HashSet<string>(StringComparer.Ordinal);
        var sb = new StringBuilder();

        foreach (BlockEspTarget t in targets)
        {
            if (t == null || !t.Enabled)
                continue;

            string id = NormalizeId(t.RegistryId);
            if (id.Length == 0 || !seen.Add(id))
                continue;

            string color = NormalizeColor(t.ColorHex);
            string entry = id + "=" + color;

            // Stop before exceeding the cap (account for the ';' separator).
            int projected = sb.Length + (sb.Length > 0 ? 1 : 0) + entry.Length;
            if (projected > MaxSerializedLength)
                break;

            if (sb.Length > 0)
                sb.Append(';');
            sb.Append(entry);
        }

        return sb.ToString();
    }

    /// <summary>
    /// Parses the delimited wire string back into (normalized id, color) pairs.
    /// Malformed entries are skipped. Primarily used for tests and round-trip validation.
    /// </summary>
    public static List<(string Id, string Color)> Parse(string? serialized)
    {
        var result = new List<(string, string)>();
        if (string.IsNullOrWhiteSpace(serialized))
            return result;

        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (string rawEntry in serialized.Split(';', StringSplitOptions.RemoveEmptyEntries))
        {
            int eq = rawEntry.IndexOf('=');
            if (eq <= 0)
                continue;

            string id = NormalizeId(rawEntry.Substring(0, eq));
            if (id.Length == 0 || !seen.Add(id))
                continue;

            string color = NormalizeColor(rawEntry.Substring(eq + 1));
            result.Add((id, color));
        }

        return result;
    }
}
