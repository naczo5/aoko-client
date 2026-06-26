using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Aoko.Core;

/// <summary>
/// A user-defined GUI color palette. All colors are stored as <c>#RRGGBB</c> hex
/// strings so a palette is fully self-contained on disk and forward-compatible with
/// future editors that may expose more of the individual roles.
/// </summary>
public sealed class CustomPalette
{
    public string Name { get; set; } = "Custom";
    public string Background { get; set; } = "#0A0B0F";
    public string Panel { get; set; } = "#12141A";
    public string SliderBackground { get; set; } = "#181B22";
    public string SliderForeground { get; set; } = "#2A2F38";
    public string Accent { get; set; } = "#C7625A";
    public string Text { get; set; } = "#E8EAEE";
    public string DimText { get; set; } = "#7A8290";
    public string TabSelected { get; set; } = "#1F2229";
    public string TabHover { get; set; } = "#181B22";
}

/// <summary>
/// Loads and persists user-defined palettes to
/// <c>%AppData%\Aoko\palettes.json</c>. All file access is best-effort; failures
/// degrade to an empty palette set rather than throwing into the UI.
/// </summary>
public static class PaletteStore
{
    private static readonly string PalettesFile;
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = JsonIgnoreCondition.Never
    };

    static PaletteStore()
    {
        string appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        string dir = Path.Combine(appData, "Aoko");
        try { Directory.CreateDirectory(dir); }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
        PalettesFile = Path.Combine(dir, "palettes.json");
    }

    /// <summary>Absolute path of the palettes file (also used by the "open folder" affordance).</summary>
    public static string FilePath => PalettesFile;

    /// <summary>Reads all saved palettes. Returns an empty list when none exist or on error.</summary>
    public static List<CustomPalette> LoadAll()
    {
        try
        {
            if (!File.Exists(PalettesFile))
                return new List<CustomPalette>();

            string json = File.ReadAllText(PalettesFile);
            if (string.IsNullOrWhiteSpace(json))
                return new List<CustomPalette>();

            var list = JsonSerializer.Deserialize<List<CustomPalette>>(json, JsonOptions);
            return list ?? new List<CustomPalette>();
        }
        catch
        {
            return new List<CustomPalette>();
        }
    }

    /// <summary>Writes the full palette set to disk.</summary>
    public static void SaveAll(IEnumerable<CustomPalette> palettes)
    {
        var list = new List<CustomPalette>(palettes);
        string json = JsonSerializer.Serialize(list, JsonOptions);
        File.WriteAllText(PalettesFile, json);
    }

    /// <summary>
    /// Inserts or replaces a palette by name (case-insensitive) and persists the set.
    /// </summary>
    public static List<CustomPalette> Upsert(CustomPalette palette)
    {
        var list = LoadAll();
        int idx = list.FindIndex(p => string.Equals(p.Name, palette.Name, StringComparison.OrdinalIgnoreCase));
        if (idx >= 0)
            list[idx] = palette;
        else
            list.Add(palette);

        SaveAll(list);
        return list;
    }

    /// <summary>Removes a palette by name (case-insensitive) and persists the set.</summary>
    public static List<CustomPalette> Delete(string name)
    {
        var list = LoadAll();
        list.RemoveAll(p => string.Equals(p.Name, name, StringComparison.OrdinalIgnoreCase));
        SaveAll(list);
        return list;
    }
}
