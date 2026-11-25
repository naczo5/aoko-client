using System;

namespace Aoko.Core.Servers;

/// <summary>
/// One saved server row. The JSON shape matches lunar-cookies <c>Core/ServerStore.cs</c> so an
/// existing <c>servers.json</c> keeps loading.
/// </summary>
public sealed class SavedServer
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public string Name { get; set; } = "";
    public string Address { get; set; } = "";
    public DateTimeOffset AddedAt { get; set; } = DateTimeOffset.UtcNow;
    public DateTimeOffset? LastJoinedAt { get; set; }
}
