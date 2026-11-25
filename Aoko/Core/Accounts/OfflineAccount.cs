using System;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;

namespace Aoko.Core.Accounts;

/// <summary>
/// Creates offline-mode ("cracked") profiles from a username alone. The UUID is
/// the standard offline v3 (MD5) UUID over the bytes of <c>"OfflinePlayer:" + name</c>,
/// the same value Java's <c>UUID.nameUUIDFromBytes</c> produces, so offline
/// servers and plugins recognise the account. The access token is the
/// conventional <c>"0"</c> placeholder; offline-mode servers never validate it.
/// </summary>
internal static class OfflineAccount
{
    public const string SourceName = AccountSources.Offline;
    public const string OfflineToken = "0";

    private static readonly Regex NamePattern = new(
        "^[A-Za-z0-9_]{3,16}$",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    public static bool IsValidName(string? name) =>
        !string.IsNullOrWhiteSpace(name) && NamePattern.IsMatch(name.Trim());

    public static bool TryCreate(string? rawName, out string name, out string uuid, out string error)
    {
        name = "";
        uuid = "";
        string trimmed = rawName?.Trim() ?? "";

        if (!IsValidName(trimmed))
        {
            error = "Usernames must be 3-16 characters using only letters, numbers, and underscores.";
            return false;
        }

        name = trimmed;
        uuid = ComputeOfflineUuid(trimmed).Replace("-", "", StringComparison.Ordinal);
        error = "";
        return true;
    }

    public static string ComputeOfflineUuid(string name)
    {
        byte[] hash = MD5.HashData(Encoding.UTF8.GetBytes("OfflinePlayer:" + name));
        hash[6] = (byte)((hash[6] & 0x0F) | 0x30); // version 3
        hash[8] = (byte)((hash[8] & 0x3F) | 0x80); // RFC 4122 variant
        return FormatDashed(hash);
    }

    private static string FormatDashed(ReadOnlySpan<byte> bytes) =>
        bytes[0].ToString("x2") + bytes[1].ToString("x2") + bytes[2].ToString("x2") + bytes[3].ToString("x2") + "-" +
        bytes[4].ToString("x2") + bytes[5].ToString("x2") + "-" +
        bytes[6].ToString("x2") + bytes[7].ToString("x2") + "-" +
        bytes[8].ToString("x2") + bytes[9].ToString("x2") + "-" +
        bytes[10].ToString("x2") + bytes[11].ToString("x2") + bytes[12].ToString("x2") + bytes[13].ToString("x2") +
        bytes[14].ToString("x2") + bytes[15].ToString("x2");
}
