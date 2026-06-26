using System;
using System.Collections.Generic;
using System.Text.Json.Nodes;

namespace Aoko.Core;

/// <summary>
/// Normalized placement for a single HUD element.
/// X and Y are screen-fraction anchors in [0, 1]; Scale is a size multiplier in [0.5, 2.0].
/// </summary>
public sealed class HudElementLayout
{
    public double X { get; set; }
    public double Y { get; set; }
    public double Scale { get; set; }
}

/// <summary>
/// Stable lowercase element id constants shared with the C++ bridge.
/// </summary>
public static class HudElementId
{
    public const string ModuleList   = "modulelist";
    public const string ClosestPlayer = "closestplayer";
    public const string PixelParty   = "pixelparty";
    public const string ChestEspList = "chestesplist";
    public const string GtbHint      = "gtbhint";
    public const string Nametags      = "nametags";
}

/// <summary>
/// Whole-HUD layout keyed by stable element id.
/// All mutations clamp values to valid ranges and substitute the element's canonical
/// default when a value is NaN or Infinity.
/// </summary>
public sealed class HudLayout
{
    // ── Clamping constants ──────────────────────────────────────────────────────

    public const double MinScale = 0.5;
    public const double MaxScale = 2.0;

    // ── Canonical defaults ──────────────────────────────────────────────────────

    /// <summary>
    /// Factory that returns a fresh copy of the canonical default layout.
    /// </summary>
    public static Dictionary<string, HudElementLayout> BuildDefaults() => new()
    {
        [HudElementId.ModuleList]    = new HudElementLayout { X = 0.985, Y = 0.020, Scale = 1.0 },
        [HudElementId.ClosestPlayer] = new HudElementLayout { X = 0.500, Y = 0.780, Scale = 1.0 },
        [HudElementId.PixelParty]    = new HudElementLayout { X = 0.500, Y = 0.660, Scale = 1.0 },
        [HudElementId.ChestEspList]  = new HudElementLayout { X = 0.015, Y = 0.300, Scale = 1.0 },
        [HudElementId.GtbHint]       = new HudElementLayout { X = 0.500, Y = 0.620, Scale = 1.0 },
        [HudElementId.Nametags]      = new HudElementLayout { X = 0.000, Y = 0.000, Scale = 1.0 },
    };

    // ── Internal state ──────────────────────────────────────────────────────────

    // Ordered list of all known element ids — used by ToJson to guarantee all six are emitted.
    private static readonly IReadOnlyList<string> _knownIds = new[]
    {
        HudElementId.ModuleList,
        HudElementId.ClosestPlayer,
        HudElementId.PixelParty,
        HudElementId.ChestEspList,
        HudElementId.GtbHint,
        HudElementId.Nametags,
    };

    private readonly Dictionary<string, HudElementLayout> _elements;

    // ── Construction ────────────────────────────────────────────────────────────

    /// <summary>Initializes the layout with canonical defaults for all six elements.</summary>
    public HudLayout()
    {
        _elements = BuildDefaults();
    }

    /// <summary>Internal constructor used by <see cref="FromJson"/> to supply a pre-built dictionary.</summary>
    private HudLayout(Dictionary<string, HudElementLayout> elements)
    {
        _elements = elements;
    }

    // ── Public read access ──────────────────────────────────────────────────────

    /// <summary>Returns the layout for the given element, or its canonical default if not stored.</summary>
    public HudElementLayout Get(string elementId)
    {
        if (_elements.TryGetValue(elementId, out HudElementLayout? layout))
            return layout;

        // Fall back to canonical default (defensive; shouldn't happen for known ids).
        BuildDefaults().TryGetValue(elementId, out HudElementLayout? def);
        return def ?? new HudElementLayout { X = 0, Y = 0, Scale = 1.0 };
    }

    // ── Mutation ────────────────────────────────────────────────────────────────

    /// <summary>
    /// Stores a clamped copy of <paramref name="value"/> for <paramref name="elementId"/>.
    /// NaN/Infinity values are replaced with the element's canonical default.
    /// </summary>
    public void Set(string elementId, HudElementLayout value)
    {
        // Fetch canonical defaults for this element (used when a value is NaN/∞).
        BuildDefaults().TryGetValue(elementId, out HudElementLayout? def);
        def ??= new HudElementLayout { X = 0, Y = 0, Scale = 1.0 };

        double x     = ClampCoord(value.X, def.X);
        double y     = ClampCoord(value.Y, def.Y);
        double scale = ClampScale(value.Scale, def.Scale);

        _elements[elementId] = new HudElementLayout { X = x, Y = y, Scale = scale };
    }

    /// <summary>Restores the named element to its canonical default anchor and scale.</summary>
    public void ResetElement(string elementId)
    {
        if (BuildDefaults().TryGetValue(elementId, out HudElementLayout? def))
            _elements[elementId] = def;
    }

    /// <summary>Restores every element to its canonical defaults.</summary>
    public void ResetAll()
    {
        foreach (var kv in BuildDefaults())
            _elements[kv.Key] = kv.Value;
    }

    // ── Serialization ────────────────────────────────────────────────────────────

    /// <summary>
    /// Serializes the full layout to a <see cref="JsonObject"/> emitting all six element ids.
    /// </summary>
    public JsonObject ToJson()
    {
        var root = new JsonObject();

        // Emit all known ids (including any that only have defaults) so the bridge
        // always receives a complete layout.
        foreach (string id in _knownIds)
        {
            HudElementLayout l = Get(id);
            root[id] = new JsonObject
            {
                ["x"]     = l.X,
                ["y"]     = l.Y,
                ["scale"] = l.Scale,
            };
        }

        return root;
    }

    /// <summary>
    /// Deserializes a layout from a <see cref="JsonNode"/>.
    /// Missing elements get their canonical default; unknown element ids are ignored
    /// (forward-compatibility).
    /// </summary>
    public static HudLayout FromJson(JsonNode? node)
    {
        // Start with canonical defaults so any missing element gets its default value.
        var elements = BuildDefaults();

        if (node is JsonObject obj)
        {
            foreach (var kv in obj)
            {
                string id = kv.Key;

                // Skip unknown element ids (forward-compat).
                if (!elements.ContainsKey(id))
                    continue;

                if (kv.Value is not JsonObject elementObj)
                    continue;

                // Fetch canonical default for this id to use as NaN/∞ fallback.
                HudElementLayout def = elements[id];

                double x     = ReadDouble(elementObj, "x",     def.X);
                double y     = ReadDouble(elementObj, "y",     def.Y);
                double scale = ReadDouble(elementObj, "scale", def.Scale);

                elements[id] = new HudElementLayout
                {
                    X     = ClampCoord(x, def.X),
                    Y     = ClampCoord(y, def.Y),
                    Scale = ClampScale(scale, def.Scale),
                };
            }
        }

        return new HudLayout(elements);
    }

    // ── Equality (echo guard) ────────────────────────────────────────────────────

    private const double Tolerance = 1e-9;

    /// <summary>
    /// Returns <see langword="true"/> iff all corresponding element anchors and scales are
    /// equal within <see cref="Tolerance"/>.  Used as the echo guard in
    /// <c>GameStateClient</c> to avoid re-sending the config when the bridge already has the
    /// same layout.
    /// </summary>
    public bool EqualsLayout(HudLayout other)
    {
        foreach (string id in _knownIds)
        {
            HudElementLayout a = Get(id);
            HudElementLayout b = other.Get(id);

            if (Math.Abs(a.X - b.X)         > Tolerance) return false;
            if (Math.Abs(a.Y - b.Y)         > Tolerance) return false;
            if (Math.Abs(a.Scale - b.Scale) > Tolerance) return false;
        }
        return true;
    }

    // ── Private helpers ──────────────────────────────────────────────────────────

    /// <summary>
    /// Clamps a coordinate to [0, 1].
    /// NaN or Infinity is replaced with <paramref name="fallback"/>.
    /// </summary>
    private static double ClampCoord(double value, double fallback)
    {
        if (double.IsNaN(value) || double.IsInfinity(value))
            return fallback;
        return Math.Clamp(value, 0.0, 1.0);
    }

    /// <summary>
    /// Clamps a scale to [MinScale, MaxScale].
    /// NaN, Infinity, or 0 is replaced with <paramref name="fallback"/> (defaults to 1.0).
    /// </summary>
    private static double ClampScale(double value, double fallback)
    {
        if (double.IsNaN(value) || double.IsInfinity(value) || value == 0.0)
            return fallback;
        return Math.Clamp(value, MinScale, MaxScale);
    }

    /// <summary>Reads a double from a JSON object, returning <paramref name="fallback"/> on any parse failure.</summary>
    private static double ReadDouble(JsonObject obj, string key, double fallback)
    {
        if (!obj.TryGetPropertyValue(key, out JsonNode? node) || node is null)
            return fallback;

        try
        {
            return node.GetValue<double>();
        }
        catch
        {
            return fallback;
        }
    }
}
