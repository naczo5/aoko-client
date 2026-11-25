using Aoko.Core.Servers;

namespace Aoko.Tests;

/// <summary>
/// SRV results decide the port a join actually uses. A failure cached for the whole process sent
/// every later ping and join to 25565, so failures must expire while successes stay reusable.
/// </summary>
public sealed class SrvLookupCacheTests
{
    private static readonly TimeSpan FailureTtl = TimeSpan.FromMinutes(10);
    private static readonly TimeSpan SuccessTtl = TimeSpan.FromHours(1);

    [Fact]
    public void AFailureExpiresSoTheNextLookupAsksAgain()
    {
        DateTimeOffset now = DateTimeOffset.UnixEpoch;
        var cache = new SrvLookupCache(FailureTtl, SuccessTtl, () => now);

        cache.Store("play.example.com", null);

        Assert.True(cache.TryGet("play.example.com", out ServerAddress? cached));
        Assert.Null(cached);

        now += FailureTtl - TimeSpan.FromSeconds(1);
        Assert.True(cache.TryGet("play.example.com", out _));

        now += TimeSpan.FromSeconds(2);
        Assert.False(cache.TryGet("play.example.com", out _));
    }

    [Fact]
    public void ASuccessIsReusedForTheSession()
    {
        DateTimeOffset now = DateTimeOffset.UnixEpoch;
        var cache = new SrvLookupCache(FailureTtl, SuccessTtl, () => now);
        var target = new ServerAddress("proxy.example.com", 25566, true);

        cache.Store("play.example.com", target);

        now += TimeSpan.FromMinutes(30);
        Assert.True(cache.TryGet("play.example.com", out ServerAddress? cached));
        Assert.Equal(target, cached);

        now += SuccessTtl;
        Assert.False(cache.TryGet("play.example.com", out _));
    }

    [Fact]
    public void AFailureDoesNotHideASuccessThatReplacesIt()
    {
        DateTimeOffset now = DateTimeOffset.UnixEpoch;
        var cache = new SrvLookupCache(FailureTtl, SuccessTtl, () => now);

        cache.Store("play.example.com", null);
        cache.Store("play.example.com", new ServerAddress("proxy.example.com", 25566, true));

        // A retry that worked must take effect immediately, not after the failure TTL.
        Assert.True(cache.TryGet("play.example.com", out ServerAddress? cached));
        Assert.Equal("proxy.example.com", cached!.Value.Host);
        Assert.Equal(25566, cached.Value.Port);
    }

    [Fact]
    public void HostsAreMatchedCaseInsensitively()
    {
        DateTimeOffset now = DateTimeOffset.UnixEpoch;
        var cache = new SrvLookupCache(FailureTtl, SuccessTtl, () => now);
        cache.Store("play.example.com", new ServerAddress("proxy.example.com", 25566, true));

        Assert.True(cache.TryGet("PLAY.Example.COM", out ServerAddress? cached));
        Assert.Equal(25566, cached!.Value.Port);
    }

    [Fact]
    public void UnknownHostsAreMisses()
    {
        var cache = new SrvLookupCache(FailureTtl, SuccessTtl, () => DateTimeOffset.UnixEpoch);

        Assert.False(cache.TryGet("never-looked-up.example.com", out ServerAddress? cached));
        Assert.Null(cached);
    }
}
