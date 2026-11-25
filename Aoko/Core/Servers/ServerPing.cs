using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;

namespace Aoko.Core.Servers;

/// <summary>One server list ping outcome, already shaped for the Servers tab labels.</summary>
public sealed class ServerPingResult
{
    public bool Online { get; init; }
    public int LatencyMs { get; init; }
    public string Description { get; init; } = "";
    public int OnlinePlayers { get; init; }
    public int MaxPlayers { get; init; }
    public string Version { get; init; } = "";
    public string Error { get; init; } = "";

    public string StatusLabel => Online
        ? $"{LatencyMs} ms"
        : string.IsNullOrEmpty(Error) ? "Offline" : Error;

    public string PlayersLabel => Online ? $"{OnlinePlayers}/{MaxPlayers}" : "—";
}

/// <summary>
/// Minecraft Server List Ping (protocol 767), ported from lunar-cookies <c>Core/ServerPing.cs</c>.
/// </summary>
public static class ServerPing
{
    public const int ProtocolVersion = 767;

    private const int MaximumPacketLength = 32767;
    private static readonly TimeSpan OperationTimeout = TimeSpan.FromSeconds(4);
    private static readonly TimeSpan PingBudget = TimeSpan.FromSeconds(10);
    private static readonly Regex ColorCodes = new("§[0-9A-FK-ORa-fk-or]", RegexOptions.Compiled);

    /// <summary>Resolves the text (SRV when no port was given), then pings. Never throws.</summary>
    public static async Task<ServerPingResult> QueryAsync(string addressText, CancellationToken ct = default)
    {
        ServerAddress address;
        try
        {
            address = await ServerAddress.ResolveForPingAsync(addressText, ct).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            // The outer catch below would report the framework's "The operation was canceled."
            return new ServerPingResult { Error = "Cancelled" };
        }
        catch (ArgumentException)
        {
            // Deliberately not ex.Message: it carries .NET's "(Parameter 'text')" suffix.
            return new ServerPingResult { Error = ServerAddress.InvalidAddressMessage };
        }
        catch (SocketException)
        {
            return new ServerPingResult { Error = "Offline" };
        }
        catch (Exception)
        {
            return new ServerPingResult { Error = "Ping failed." };
        }

        return await QueryAsync(address, ct).ConfigureAwait(false);
    }

    /// <summary>Pings an already resolved address. Never throws; failures come back in <see cref="ServerPingResult.Error"/>.</summary>
    public static Task<ServerPingResult> QueryAsync(ServerAddress address, CancellationToken ct = default)
        => QueryAsync(address, ct, PingBudget);

    /// <summary>
    /// Pings with one deadline for the whole exchange. The per-operation timeouts bound each
    /// step; this bounds the sum, so a slow server cannot hold the row on "Pinging".
    /// </summary>
    internal static async Task<ServerPingResult> QueryAsync(
        ServerAddress address,
        CancellationToken ct,
        TimeSpan budget)
    {
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(ct);
        deadline.CancelAfter(budget);
        CancellationToken token = deadline.Token;

        try
        {
            using var client = new TcpClient { NoDelay = true, ReceiveTimeout = 4000, SendTimeout = 4000 };
            var stopwatch = Stopwatch.StartNew();
            await RunWithTimeoutAsync(
                step => client.ConnectAsync(address.Host, address.Port, step).AsTask(), token).ConfigureAwait(false);

            using NetworkStream stream = client.GetStream();

            await RunWithTimeoutAsync(
                step => WritePacketAsync(stream, BuildHandshake(address.Host, address.Port), step), token)
                .ConfigureAwait(false);
            await RunWithTimeoutAsync(
                step => WritePacketAsync(stream, new byte[] { 0x00 }, step), token).ConfigureAwait(false);

            byte[] payload = await RunWithTimeoutAsync(
                step => ReadPacketAsync(stream, step), token).ConfigureAwait(false);
            stopwatch.Stop();

            if (payload.Length < 1 || payload[0] != 0x00)
                return new ServerPingResult { Error = "Unexpected ping response." };

            int offset = 1;
            string json = ReadString(payload, ref offset);
            return ParseStatus(json, (int)stopwatch.ElapsedMilliseconds);
        }
        catch (OperationCanceledException)
        {
            return new ServerPingResult { Error = ct.IsCancellationRequested ? "Cancelled" : "Timed out" };
        }
        catch (SocketException)
        {
            return new ServerPingResult { Error = "Offline" };
        }
        catch (Exception)
        {
            // Never surface a .NET type name to the GUI; the caller renders this as a chip.
            return new ServerPingResult { Error = "Ping failed." };
        }
    }

    /// <summary>
    /// Pings every address with at most <paramref name="maxParallel"/> sockets open. Results keep the
    /// input order; a cancelled ping comes back as an error result rather than throwing.
    /// </summary>
    public static async Task<IReadOnlyList<ServerPingResult>> PingManyAsync(
        IEnumerable<ServerAddress> addresses,
        int maxParallel,
        CancellationToken ct = default)
    {
        ArgumentNullException.ThrowIfNull(addresses);

        ServerAddress[] targets = addresses as ServerAddress[] ?? new List<ServerAddress>(addresses).ToArray();
        var results = new ServerPingResult[targets.Length];
        if (targets.Length == 0)
            return results;

        int limit = Math.Clamp(maxParallel, 1, targets.Length);
        using var gate = new SemaphoreSlim(limit, limit);
        var tasks = new Task[targets.Length];
        for (int i = 0; i < targets.Length; i++)
        {
            int index = i;
            tasks[i] = Task.Run(async () =>
            {
                try
                {
                    await gate.WaitAsync(ct).ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    results[index] = new ServerPingResult { Error = "Cancelled" };
                    return;
                }

                try
                {
                    results[index] = await QueryAsync(targets[index], ct).ConfigureAwait(false);
                }
                finally
                {
                    gate.Release();
                }
            });
        }

        await Task.WhenAll(tasks).ConfigureAwait(false);
        return results;
    }

    public static ServerPingResult ParseStatus(string json, int latencyMs)
    {
        using JsonDocument document = JsonDocument.Parse(json);
        JsonElement root = document.RootElement;

        string description = root.TryGetProperty("description", out JsonElement desc) ? FlattenMotd(desc) : "";
        int online = 0;
        int max = 0;
        if (root.TryGetProperty("players", out JsonElement players) && players.ValueKind == JsonValueKind.Object)
        {
            if (players.TryGetProperty("online", out JsonElement onlineElement)
                && onlineElement.TryGetInt32(out int onlineValue))
            {
                online = onlineValue;
            }

            if (players.TryGetProperty("max", out JsonElement maxElement)
                && maxElement.TryGetInt32(out int maxValue))
            {
                max = maxValue;
            }
        }

        string version = "";
        if (root.TryGetProperty("version", out JsonElement versionElement)
            && versionElement.ValueKind == JsonValueKind.Object
            && versionElement.TryGetProperty("name", out JsonElement nameElement)
            && nameElement.ValueKind == JsonValueKind.String)
        {
            version = nameElement.GetString() ?? "";
        }

        return new ServerPingResult
        {
            Online = true,
            LatencyMs = latencyMs,
            Description = description,
            OnlinePlayers = online,
            MaxPlayers = max,
            Version = version
        };
    }

    public static string FlattenMotd(JsonElement element)
    {
        var text = new StringBuilder();
        AppendMotd(element, text);
        return ColorCodes.Replace(text.ToString(), "").Trim();
    }

    private static void AppendMotd(JsonElement element, StringBuilder text)
    {
        switch (element.ValueKind)
        {
            case JsonValueKind.String:
                text.Append(element.GetString());
                break;
            case JsonValueKind.Object:
                if (element.TryGetProperty("text", out JsonElement inner) && inner.ValueKind == JsonValueKind.String)
                    text.Append(inner.GetString());
                if (element.TryGetProperty("extra", out JsonElement extra) && extra.ValueKind == JsonValueKind.Array)
                {
                    foreach (JsonElement child in extra.EnumerateArray())
                        AppendMotd(child, text);
                }

                break;
            case JsonValueKind.Array:
                foreach (JsonElement child in element.EnumerateArray())
                    AppendMotd(child, text);
                break;
        }
    }

    internal static byte[] BuildHandshake(string host, int port)
    {
        using var stream = new MemoryStream();
        WriteVarInt(stream, 0x00);
        WriteVarInt(stream, ProtocolVersion);
        WriteString(stream, host);
        Span<byte> portBytes = stackalloc byte[2];
        BinaryPrimitives.WriteUInt16BigEndian(portBytes, (ushort)port);
        stream.Write(portBytes);
        WriteVarInt(stream, 1);
        return stream.ToArray();
    }

    internal static async Task WritePacketAsync(Stream stream, byte[] packet, CancellationToken ct)
    {
        using var buffer = new MemoryStream();
        WriteVarInt(buffer, packet.Length);
        buffer.Write(packet);
        byte[] framed = buffer.ToArray();
        await stream.WriteAsync(framed, ct).ConfigureAwait(false);
        await stream.FlushAsync(ct).ConfigureAwait(false);
    }

    internal static async Task<byte[]> ReadPacketAsync(Stream stream, CancellationToken ct)
    {
        int length = await ReadVarIntAsync(stream, ct).ConfigureAwait(false);
        if (length is < 1 or > MaximumPacketLength)
            throw new InvalidOperationException("Invalid ping packet length.");

        byte[] payload = new byte[length];
        int read = 0;
        while (read < length)
        {
            int count = await stream.ReadAsync(payload.AsMemory(read, length - read), ct).ConfigureAwait(false);
            if (count <= 0)
                throw new EndOfStreamException();
            read += count;
        }

        return payload;
    }

    internal static async Task<int> ReadVarIntAsync(Stream stream, CancellationToken ct)
    {
        int value = 0;
        int position = 0;
        byte[] one = new byte[1];
        while (true)
        {
            int count = await stream.ReadAsync(one.AsMemory(0, 1), ct).ConfigureAwait(false);
            if (count <= 0)
                throw new EndOfStreamException();
            byte current = one[0];
            value |= (current & 0x7F) << position;
            if ((current & 0x80) == 0)
                return value;
            position += 7;
            if (position >= 32)
                throw new InvalidOperationException("VarInt too long.");
        }
    }

    internal static void WriteVarInt(Stream stream, int value)
    {
        uint remaining = (uint)value;
        while (true)
        {
            if ((remaining & ~0x7Fu) == 0)
            {
                stream.WriteByte((byte)remaining);
                return;
            }

            stream.WriteByte((byte)((remaining & 0x7F) | 0x80));
            remaining >>= 7;
        }
    }

    internal static void WriteString(Stream stream, string value)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(value);
        WriteVarInt(stream, bytes.Length);
        stream.Write(bytes);
    }

    internal static string ReadString(byte[] buffer, ref int offset)
    {
        int length = ReadVarInt(buffer, ref offset);
        if (length < 0 || offset + length > buffer.Length)
            throw new InvalidOperationException("Invalid ping string.");

        string value = Encoding.UTF8.GetString(buffer, offset, length);
        offset += length;
        return value;
    }

    internal static int ReadVarInt(byte[] buffer, ref int offset)
    {
        int value = 0;
        int position = 0;
        while (offset < buffer.Length)
        {
            byte current = buffer[offset++];
            value |= (current & 0x7F) << position;
            if ((current & 0x80) == 0)
                return value;
            position += 7;
            if (position >= 32)
                throw new InvalidOperationException("VarInt too long.");
        }

        throw new EndOfStreamException();
    }

    private static async Task RunWithTimeoutAsync(Func<CancellationToken, Task> operation, CancellationToken ct)
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(ct);
        timeout.CancelAfter(OperationTimeout);
        await operation(timeout.Token).ConfigureAwait(false);
    }

    private static async Task<T> RunWithTimeoutAsync<T>(Func<CancellationToken, Task<T>> operation, CancellationToken ct)
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(ct);
        timeout.CancelAfter(OperationTimeout);
        return await operation(timeout.Token).ConfigureAwait(false);
    }
}
