using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace Aoko.Core.Servers;

/// <summary>
/// A Minecraft server target parsed from user text or a saved entry. Parsing rules are ported
/// from lunar-cookies <c>Core/MinecraftServerAddress.cs</c> so saved text keeps the same meaning.
/// </summary>
public readonly record struct ServerAddress(string Host, int Port, bool PortSpecified)
{
    public const int DefaultPort = 25565;
    public const int MaximumTextLength = 256;
    public const string InvalidAddressMessage = "Enter a server as host or host:port.";

    private const int DnsPort = 53;
    private const ushort SrvTransactionId = 0x1C0D;
    private const int MaximumSrvServers = 2;
    private static readonly TimeSpan SrvTimeout = TimeSpan.FromSeconds(2);
    private static readonly TimeSpan SrvFailureTtl = TimeSpan.FromMinutes(10);
    private static readonly TimeSpan SrvSuccessTtl = TimeSpan.FromHours(1);

    /// <summary>
    /// Resolvers used when no adapter reports a DNS server (VPN mid-connect, container, a
    /// virtual adapter). The reference sends the query to 8.8.8.8 unconditionally; keeping the
    /// system list first and these as the fallback preserves both behaviours.
    /// </summary>
    private static readonly IPAddress[] FallbackDnsServers =
    {
        IPAddress.Parse("8.8.8.8"),
        IPAddress.Parse("1.1.1.1")
    };

    /// <summary>
    /// Lookup results with a TTL: a success stays cached, a failure expires so a miss before
    /// DHCP/DNS is up cannot disable SRV for that host for the rest of the session.
    /// </summary>
    private static readonly SrvLookupCache SrvResults = new(SrvFailureTtl, SrvSuccessTtl);

    public string Display => PortSpecified || Port != DefaultPort ? $"{FormatHost(Host)}:{Port}" : Host;

    public static bool TryParse(string? text, out ServerAddress address)
    {
        address = default;
        if (string.IsNullOrWhiteSpace(text))
            return false;

        string value = text.Trim();
        if (value.Length > MaximumTextLength)
            return false;

        string host;
        int port = DefaultPort;
        bool portSpecified = false;

        if (value.StartsWith('['))
        {
            int close = value.IndexOf(']');
            if (close <= 1)
                return false;
            host = value[1..close];
            string rest = value[(close + 1)..];
            if (rest.Length > 0)
            {
                if (!rest.StartsWith(':') || !TryParsePort(rest[1..], out port))
                    return false;
                portSpecified = true;
            }
        }
        else
        {
            int colon = value.LastIndexOf(':');
            if (colon > 0 && value.IndexOf(':') == colon && colon < value.Length - 1 && !value.Contains('/'))
            {
                host = value[..colon];
                if (!TryParsePort(value[(colon + 1)..], out port))
                    return false;
                portSpecified = true;
            }
            else
            {
                // No bracketed form and more than one colon: treat the whole value as an IPv6 host.
                host = value;
            }
        }

        host = host.Trim();
        if (string.IsNullOrWhiteSpace(host) || host.Contains('/') || host.Contains(' '))
            return false;

        address = new ServerAddress(host, port, portSpecified);
        return true;
    }

    public static async Task<ServerAddress> ResolveForPingAsync(string text, CancellationToken ct = default)
    {
        if (!TryParse(text, out ServerAddress parsed))
            throw new ArgumentException(InvalidAddressMessage);
        if (parsed.PortSpecified)
            return parsed;

        ServerAddress? srv = await TrySrvAsync(parsed.Host, ct).ConfigureAwait(false);
        return srv ?? parsed;
    }

    private static bool TryParsePort(string text, out int port) =>
        int.TryParse(text, NumberStyles.None, CultureInfo.InvariantCulture, out port)
        && port is >= 1 and <= 65535;

    private static string FormatHost(string host) => host.Contains(':') ? $"[{host}]" : host;

    private static async Task<ServerAddress?> TrySrvAsync(string host, CancellationToken ct)
    {
        string key = host.TrimEnd('.');
        if (key.Length == 0)
            return null;

        if (SrvResults.TryGet(key, out ServerAddress? cached))
            return cached;

        ServerAddress? result = await LookupSrvAsync(key, ct).ConfigureAwait(false);
        // A cancelled lookup says nothing about the host: caching it would send the next
        // join to port 25565.
        if (!ct.IsCancellationRequested)
            SrvResults.Store(key, result);

        return result;
    }

    private static async Task<ServerAddress?> LookupSrvAsync(string host, CancellationToken ct)
    {
        // .NET has no SRV API, so the hand-built wire query is kept and sent to the resolvers
        // the system is configured with, falling back to the reference's public resolvers.
        foreach (IPAddress server in GetDnsServers())
        {
            try
            {
                using var client = new UdpClient(server.AddressFamily);
                client.Client.ReceiveTimeout = (int)SrvTimeout.TotalMilliseconds;
                client.Client.SendTimeout = (int)SrvTimeout.TotalMilliseconds;

                string queryName = $"_minecraft._tcp.{host.TrimEnd('.')}";
                byte[] query = BuildSrvQuery(queryName);
                var endpoint = new IPEndPoint(server, DnsPort);
                await client.SendAsync(query, query.Length, endpoint)
                    .WaitAsync(SrvTimeout, ct).ConfigureAwait(false);

                UdpReceiveResult response = await client.ReceiveAsync()
                    .WaitAsync(SrvTimeout, ct).ConfigureAwait(false);

                // Only the resolver we asked may answer, and only with our reply.
                if (!response.RemoteEndPoint.Equals(endpoint))
                    continue;

                if (TryReadSrv(response.Buffer, queryName, out string target, out int port)
                    && !string.IsNullOrWhiteSpace(target) && target != ".")
                {
                    // The target stays a host name so TcpClient resolves it through the system
                    // resolver and the handshake carries the name the server expects.
                    return new ServerAddress(target.TrimEnd('.'), port, true);
                }
            }
            catch
            {
                // SRV is optional; ping and join still use the explicit host.
            }
        }

        return null;
    }

    private static IReadOnlyList<IPAddress> GetDnsServers()
    {
        List<IPAddress> servers = CollectSystemDnsServers();
        return servers.Count == 0 ? FallbackDnsServers : servers;
    }

    private static List<IPAddress> CollectSystemDnsServers()
    {
        var servers = new List<IPAddress>(MaximumSrvServers);
        try
        {
            foreach (NetworkInterface networkInterface in NetworkInterface.GetAllNetworkInterfaces())
            {
                if (networkInterface.OperationalStatus != OperationalStatus.Up)
                    continue;

                foreach (IPAddress address in networkInterface.GetIPProperties().DnsAddresses)
                {
                    if (address.Equals(IPAddress.Any) || address.Equals(IPAddress.IPv6Any))
                        continue;
                    if (servers.Contains(address))
                        continue;

                    servers.Add(address);
                    if (servers.Count == MaximumSrvServers)
                        return servers;
                }
            }
        }
        catch (NetworkInformationException)
        {
        }
        catch (PlatformNotSupportedException)
        {
        }
        catch (SocketException)
        {
        }

        return servers;
    }

    internal static byte[] BuildSrvQuery(string name)
    {
        using var stream = new MemoryStream();
        using var writer = new BinaryWriter(stream);
        writer.Write((ushort)IPAddress.HostToNetworkOrder((short)SrvTransactionId));
        writer.Write((ushort)IPAddress.HostToNetworkOrder((short)0x0100));
        writer.Write((ushort)IPAddress.HostToNetworkOrder((short)1));
        writer.Write((ushort)0);
        writer.Write((ushort)0);
        writer.Write((ushort)0);
        foreach (string label in name.Split('.', StringSplitOptions.RemoveEmptyEntries))
        {
            byte[] bytes = Encoding.ASCII.GetBytes(label);
            writer.Write((byte)bytes.Length);
            writer.Write(bytes);
        }

        writer.Write((byte)0);
        writer.Write((ushort)IPAddress.HostToNetworkOrder((short)33));
        writer.Write((ushort)IPAddress.HostToNetworkOrder((short)1));
        return stream.ToArray();
    }

    internal static bool TryReadSrv(byte[] buffer, out string target, out int port)
        => TryReadSrv(buffer, null, out target, out port);

    /// <summary>
    /// Reads a SRV answer. When <paramref name="expectedQueryName"/> is given the datagram must be
    /// our own reply - matching transaction id, a response bit, rcode 0, and the question we asked -
    /// so a spoofed or stale packet cannot be cached as this server's target.
    /// </summary>
    internal static bool TryReadSrv(byte[] buffer, string? expectedQueryName, out string target, out int port)
    {
        target = "";
        port = DefaultPort;
        if (buffer.Length < 12)
            return false;

        if (((buffer[0] << 8) | buffer[1]) != SrvTransactionId)
            return false;
        if ((buffer[2] & 0x80) == 0 || (buffer[3] & 0x0F) != 0)
            return false;

        int questionCount = (buffer[4] << 8) | buffer[5];
        int answerCount = (buffer[6] << 8) | buffer[7];
        if (expectedQueryName != null && questionCount < 1)
            return false;

        int offset = 12;
        for (int i = 0; i < questionCount; i++)
        {
            int nameOffset = offset;
            if (!SkipName(buffer, ref offset) || offset + 4 > buffer.Length)
                return false;

            if (i == 0 && expectedQueryName != null)
            {
                if (!TryReadName(buffer, ref nameOffset, out string question))
                    return false;
                if (!string.Equals(question.TrimEnd('.'), expectedQueryName.TrimEnd('.'),
                        StringComparison.OrdinalIgnoreCase))
                    return false;
                if (((buffer[offset] << 8) | buffer[offset + 1]) != 33)
                    return false;
            }

            offset += 4;
        }

        for (int i = 0; i < answerCount; i++)
        {
            if (!SkipName(buffer, ref offset) || offset + 10 > buffer.Length)
                return false;
            int type = (buffer[offset] << 8) | buffer[offset + 1];
            int length = (buffer[offset + 8] << 8) | buffer[offset + 9];
            offset += 10;
            if (offset + length > buffer.Length)
                return false;
            if (type == 33 && length >= 6)
            {
                // SRV rdata: priority, weight, port, then the target name (may be compressed).
                port = (buffer[offset + 4] << 8) | buffer[offset + 5];
                int nameOffset = offset + 6;
                if (TryReadName(buffer, ref nameOffset, out target) && port is >= 1 and <= 65535)
                    return true;
            }

            offset += length;
        }

        return false;
    }

    private static bool SkipName(byte[] buffer, ref int offset)
    {
        int hops = 0;
        while (offset < buffer.Length && hops++ < 32)
        {
            byte len = buffer[offset];
            if (len == 0)
            {
                offset++;
                return true;
            }

            if ((len & 0xC0) == 0xC0)
            {
                offset += 2;
                return true;
            }

            offset += 1 + len;
        }

        return false;
    }

    private static bool TryReadName(byte[] buffer, ref int offset, out string name)
    {
        var labels = new List<string>();
        int hops = 0;
        int end = offset;
        bool jumped = false;
        while (offset < buffer.Length && hops++ < 32)
        {
            byte len = buffer[offset];
            if (len == 0)
            {
                if (!jumped)
                    end = offset + 1;
                break;
            }

            if ((len & 0xC0) == 0xC0)
            {
                if (offset + 1 >= buffer.Length)
                    break;
                if (!jumped)
                    end = offset + 2;
                offset = ((len & 0x3F) << 8) | buffer[offset + 1];
                jumped = true;
                continue;
            }

            offset++;
            if (offset + len > buffer.Length)
                break;
            labels.Add(Encoding.ASCII.GetString(buffer, offset, len));
            offset += len;
        }

        offset = end;
        name = string.Join('.', labels);
        return labels.Count > 0;
    }
}

/// <summary>
/// TTL cache for SRV lookups. Successes are reused, failures expire: an SRV miss during
/// startup or from one unresponsive resolver must not send later joins to port 25565
/// for the rest of the session.
/// </summary>
internal sealed class SrvLookupCache
{
    private readonly ConcurrentDictionary<string, Entry> _entries =
        new(StringComparer.OrdinalIgnoreCase);
    private readonly TimeSpan _failureTtl;
    private readonly TimeSpan _successTtl;
    private readonly Func<DateTimeOffset> _utcNow;

    internal SrvLookupCache(
        TimeSpan failureTtl,
        TimeSpan successTtl,
        Func<DateTimeOffset>? utcNow = null)
    {
        _failureTtl = failureTtl;
        _successTtl = successTtl;
        _utcNow = utcNow ?? (static () => DateTimeOffset.UtcNow);
    }

    /// <summary>True when a live entry exists; <paramref name="address"/> is null for a cached miss.</summary>
    internal bool TryGet(string host, out ServerAddress? address)
    {
        address = null;
        if (!_entries.TryGetValue(host, out Entry entry))
            return false;

        if (entry.ExpiresAt > _utcNow())
        {
            address = entry.Address;
            return true;
        }

        _entries.TryRemove(host, out _);
        return false;
    }

    internal void Store(string host, ServerAddress? address)
    {
        TimeSpan ttl = address is null ? _failureTtl : _successTtl;
        _entries[host] = new Entry(address, _utcNow() + ttl);
    }

    private readonly record struct Entry(ServerAddress? Address, DateTimeOffset ExpiresAt);
}
