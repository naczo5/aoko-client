using System;

namespace Aoko.Core.Accounts;

/// <summary>
/// An authenticated Minecraft profile. <see cref="Source"/> records which
/// credential produced it so the store can keep a refresh-backed entry from
/// being demoted by a looser import.
/// </summary>
internal sealed class MinecraftProfile
{
    public MinecraftProfile(
        string name,
        string uuid,
        string accessToken,
        string? refreshToken = null,
        string? source = null)
    {
        Name = name;
        Uuid = uuid;
        AccessToken = accessToken;
        RefreshToken = refreshToken ?? "";
        Source = string.IsNullOrWhiteSpace(source)
            ? (HasRefresh ? AccountSources.Localts : AccountSources.Token)
            : source;
    }

    public string Name { get; }
    public string Uuid { get; }
    public string AccessToken { get; }
    public string RefreshToken { get; }
    public string Source { get; }

    public bool HasRefresh => !string.IsNullOrWhiteSpace(RefreshToken);
}
