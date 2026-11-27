using System;
using System.Linq;

namespace Aoko.Core.Visuals;

/// <summary>Turns the bridge's <c>visualsStatus</c> string into the one-line text shown on the module cards.</summary>
public static class VisualsStatusText
{
    private const int MaxDetailLength = 120;

    public const string Off = "Not active.";
    public const string Ok = "Active.";
    public const string Unsupported = "Needs Minecraft 1.8.9 or 26.1 and newer.";

    public static string Describe(string? status)
    {
        string value = status?.Trim() ?? string.Empty;
        if (value.Length == 0 || value.Equals("off", StringComparison.OrdinalIgnoreCase))
            return Off;
        if (value.Equals("ok", StringComparison.OrdinalIgnoreCase))
            return Ok;

        int colon = value.IndexOf(':');
        string kind = (colon < 0 ? value : value[..colon]).Trim().ToLowerInvariant();
        string detail = colon < 0 ? string.Empty : CleanDetail(value[(colon + 1)..]);

        return kind switch
        {
            "partial" => detail.Length == 0 ? "Partly applied." : $"Partly applied: {detail}.",
            "unsupported" => Unsupported,
            "error" => detail.Length == 0 ? "Something went wrong." : $"Something went wrong: {detail}.",
            _ => Off,
        };
    }

    private static string CleanDetail(string detail)
    {
        string cleaned = new string(detail.Where(c => !char.IsControl(c)).ToArray()).Trim().TrimEnd('.');
        return cleaned.Length > MaxDetailLength ? cleaned[..MaxDetailLength].TrimEnd() + "..." : cleaned;
    }
}
