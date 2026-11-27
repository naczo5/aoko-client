using System;
using System.Text;
using System.Text.RegularExpressions;

namespace Aoko.Core.Visuals;

/// <summary>
/// Id rules shared by the cosmetics and sky modules. The bridge validates ids again,
/// so anything this returns must also satisfy the bridge's check.
/// </summary>
public static class VisualsIds
{
    /// <summary>Longest string the wire contract allows for ids and display names.</summary>
    public const int MaxNameLength = 64;

    private static readonly Regex IdPattern = new(
        @"^[a-z0-9][a-z0-9_-]{0,31}$",
        RegexOptions.CultureInvariant | RegexOptions.Compiled);

    /// <summary>Returns the trimmed, lowercased id, or "" when it is not a valid id.</summary>
    public static string Normalize(string? id)
    {
        if (string.IsNullOrWhiteSpace(id))
            return string.Empty;

        string candidate = id.Trim().ToLowerInvariant();
        return IdPattern.IsMatch(candidate) ? candidate : string.Empty;
    }

    /// <summary>True when <paramref name="id"/> is already in canonical form.</summary>
    public static bool IsValid(string? id)
        => id != null && id.Length > 0 && IdPattern.IsMatch(id);

    /// <summary>Cape display name: separators become spaces and each word is capitalised.</summary>
    public static string DisplayNameFromId(string id)
    {
        var builder = new StringBuilder(id.Length);
        bool startOfWord = true;
        foreach (char c in id)
        {
            if (c == '-' || c == '_')
            {
                builder.Append(' ');
                startOfWord = true;
            }
            else
            {
                builder.Append(startOfWord ? char.ToUpperInvariant(c) : c);
                startOfWord = false;
            }
        }

        return builder.ToString().Trim();
    }
}
