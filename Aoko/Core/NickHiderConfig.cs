using System;
using System.Text;

namespace Aoko.Core;

public static class NickHiderConfig
{
    public const int MaxAliasLength = 32;

    /// <summary>Returns a safe, plain-text alias or an empty string when it is invalid.</summary>
    public static string NormalizeAlias(string? value)
    {
        string alias = (value ?? string.Empty).Trim();
        if (alias.Length == 0 || alias.Length > MaxAliasLength)
            return string.Empty;

        foreach (char character in alias)
        {
            if (char.IsControl(character) || character == '\u00A7')
                return string.Empty;
        }

        return alias;
    }

    public static string NormalizeInput(string? value)
    {
        string alias = value ?? string.Empty;
        if (alias.Length > MaxAliasLength)
            return string.Empty;

        foreach (char character in alias)
        {
            if (char.IsControl(character) || character == '\u00A7')
                return string.Empty;
        }

        return alias;
    }
}
