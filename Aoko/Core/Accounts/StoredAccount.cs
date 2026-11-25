using System;
using System.Text.Json.Serialization;

namespace Aoko.Core.Accounts;

/// <summary>Values stored in <see cref="StoredAccount.Source"/>.</summary>
internal static class AccountSources
{
    public const string Unknown = "unknown";
    public const string Localts = "localts";
    public const string Cookie = "cookie";
    public const string Token = "token";
    public const string Offline = "offline";

    /// <summary>Player-facing wording for a source, so import results and chips agree.</summary>
    public static string Display(string? source) => source?.ToLowerInvariant() switch
    {
        Localts => "Localts token",
        Cookie => "Microsoft cookies",
        Token => "access token",
        Offline => "cracked name",
        _ => string.IsNullOrWhiteSpace(source) ? "unknown source" : source,
    };
}

/// <summary>
/// A saved login. <see cref="Id"/> is a local key that never leaves the app;
/// the access token is the Minecraft JWT and the refresh token is the MSA
/// <c>M.C...</c> token when the source supplied one.
/// </summary>
internal sealed class StoredAccount
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public string Name { get; set; } = "";
    public string Uuid { get; set; } = "";
    public string AccessToken { get; set; } = "";
    public string RefreshToken { get; set; } = "";
    public string Source { get; set; } = AccountSources.Unknown;
    public DateTimeOffset AddedAt { get; set; } = DateTimeOffset.UtcNow;
    public DateTimeOffset? LastUsedAt { get; set; }

    [JsonIgnore]
    public bool HasRefresh => !string.IsNullOrWhiteSpace(RefreshToken);

    [JsonIgnore]
    public bool IsOffline => string.Equals(Source, AccountSources.Offline, StringComparison.OrdinalIgnoreCase);

    [JsonIgnore]
    public string DisplayLabel => IsOffline
        ? $"{Name}  (cracked)"
        : HasRefresh
            ? $"{Name}  (Localts)"
            : string.Equals(Source, AccountSources.Token, StringComparison.OrdinalIgnoreCase)
                ? $"{Name}  (token)"
                : string.Equals(Source, AccountSources.Cookie, StringComparison.OrdinalIgnoreCase)
                    ? $"{Name}  (cookie)"
                    // A hand-edited or migrated entry: show the source word rather than
                    // claiming a cookie import that never happened.
                    : $"{Name}  ({Source})";
}
