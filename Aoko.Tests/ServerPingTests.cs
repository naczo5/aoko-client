using System.Diagnostics;
using System.Net;
using System.Net.Sockets;
using System.Text.Json;
using Aoko.Core.Servers;

namespace Aoko.Tests;

public class ServerPingTests
{
    [Fact]
    public void ParseStatus_ReadsPlayersAndVersion()
    {
        ServerPingResult result = ServerPing.ParseStatus(
            """{"version":{"name":"1.21.4"},"players":{"max":100,"online":12},"description":{"text":"Test"}}""",
            42);

        Assert.True(result.Online);
        Assert.Equal(42, result.LatencyMs);
        Assert.Equal(12, result.OnlinePlayers);
        Assert.Equal(100, result.MaxPlayers);
        Assert.Equal("Test", result.Description);
        Assert.Equal("1.21.4", result.Version);
        Assert.Equal("42 ms", result.StatusLabel);
        Assert.Equal("12/100", result.PlayersLabel);
    }

    [Fact]
    public void ParseStatus_SurvivesAMinimalResponse()
    {
        ServerPingResult result = ServerPing.ParseStatus("{}", 7);

        Assert.True(result.Online);
        Assert.Equal("", result.Description);
        Assert.Equal(0, result.OnlinePlayers);
        Assert.Equal(0, result.MaxPlayers);
        Assert.Equal("", result.Version);
        Assert.Equal("0/0", result.PlayersLabel);
    }

    [Fact]
    public void Labels_FallBackToTheError()
    {
        Assert.Equal("Offline", new ServerPingResult().StatusLabel);
        Assert.Equal("Timed out", new ServerPingResult { Error = "Timed out" }.StatusLabel);
        Assert.Equal("—", new ServerPingResult().PlayersLabel);
        Assert.Equal("0/0", new ServerPingResult { Online = true }.PlayersLabel);
    }

    [Theory]
    [InlineData("\"Hello World\"", "Hello World")]
    [InlineData("""{"text":"Hello World"}""", "Hello World")]
    [InlineData("""{"text":"Hello ","extra":[{"text":"§aWorld"}]}""", "Hello World")]
    [InlineData("""{"text":"§c§lShop §r§7| §aOpen"}""", "Shop | Open")]
    [InlineData("""[{"text":"A"},"B",{"text":"C"}]""", "ABC")]
    [InlineData("""{"text":"§zX"}""", "§zX")]
    [InlineData("\"  padded  \"", "padded")]
    public void FlattenMotd_JoinsComponentsAndStripsColours(string json, string expected)
    {
        using JsonDocument document = JsonDocument.Parse(json);

        Assert.Equal(expected, ServerPing.FlattenMotd(document.RootElement));
    }

    [Fact]
    public void BuildHandshake_UsesProtocol767AndTheBigEndianPort()
    {
        byte[] payload = ServerPing.BuildHandshake("mc.example.com", 25565);

        Assert.Equal(0x00, payload[0]);
        Assert.Equal(0xFF, payload[1]);
        Assert.Equal(0x05, payload[2]);
        Assert.Equal(1 + 2 + 1 + "mc.example.com".Length + 2 + 1, payload.Length);

        int portOffset = payload.Length - 3;
        Assert.Equal(0x63, payload[portOffset]);
        Assert.Equal(0xDD, payload[portOffset + 1]);
        Assert.Equal(0x01, payload[portOffset + 2]);
    }

    [Theory]
    [InlineData(0)]
    [InlineData(1)]
    [InlineData(127)]
    [InlineData(128)]
    [InlineData(255)]
    [InlineData(767)]
    [InlineData(25565)]
    [InlineData(32767)]
    public void VarInt_RoundTripsThroughTheBufferReader(int value)
    {
        using var stream = new MemoryStream();
        ServerPing.WriteVarInt(stream, value);

        byte[] bytes = stream.ToArray();
        int offset = 0;
        Assert.Equal(value, ServerPing.ReadVarInt(bytes, ref offset));
        Assert.Equal(bytes.Length, offset);
    }

    [Fact]
    public async Task ReadPacketAsync_ReadsAFramedPacket()
    {
        using var stream = new MemoryStream([0x01, 0x42]);

        byte[] payload = await ServerPing.ReadPacketAsync(stream, CancellationToken.None);

        Assert.Single(payload);
        Assert.Equal(0x42, payload[0]);
    }

    [Fact]
    public async Task ReadPacketAsync_ReassemblesPacketsDeliveredInPieces()
    {
        using var stream = new ChunkedStream(new MemoryStream([0x05, 0x00, 0x01, 0x02, 0x03, 0x04]), 1);

        byte[] payload = await ServerPing.ReadPacketAsync(stream, CancellationToken.None);

        Assert.Equal(5, payload.Length);
        Assert.Equal(0x04, payload[4]);
    }

    [Theory]
    [InlineData(new byte[] { 0x00 })]
    [InlineData(new byte[] { 0x80, 0x80, 0x02, 0x00 })]
    public async Task ReadPacketAsync_RejectsInvalidLengths(byte[] framed)
    {
        using var stream = new MemoryStream(framed);

        InvalidOperationException ex = await Assert.ThrowsAsync<InvalidOperationException>(
            () => ServerPing.ReadPacketAsync(stream, CancellationToken.None));

        Assert.Equal("Invalid ping packet length.", ex.Message);
    }

    [Fact]
    public async Task ReadPacketAsync_ThrowsOnATruncatedPacket()
    {
        using var stream = new MemoryStream([0x05, 0x00]);

        await Assert.ThrowsAsync<EndOfStreamException>(
            () => ServerPing.ReadPacketAsync(stream, CancellationToken.None));
    }

    [Fact]
    public async Task QueryAsync_ReportsParseFailuresInsteadOfThrowing()
    {
        ServerPingResult result = await ServerPing.QueryAsync("bad host");

        Assert.False(result.Online);
        // Exact match: neither the parameter suffix nor a framework sentence may leak here.
        Assert.Equal(ServerAddress.InvalidAddressMessage, result.Error);
        Assert.Equal(result.Error, result.StatusLabel);
    }

    [Fact]
    public async Task PingManyAsync_HandlesAnEmptyList()
    {
        IReadOnlyList<ServerPingResult> results = await ServerPing.PingManyAsync(Array.Empty<ServerAddress>(), 4);

        Assert.Empty(results);
    }

    [Fact]
    public async Task QueryAsync_GivesUpAtTheOverallBudgetWhenTheServerNeverAnswers()
    {
        // A socket that accepts and then says nothing is the shape that used to hold the row on
        // "Pinging" for the sum of the per-operation timeouts.
        using var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        int port = ((IPEndPoint)listener.LocalEndpoint).Port;
        using var stop = new CancellationTokenSource();
        Task accept = Task.Run(async () =>
        {
            using TcpClient accepted = await listener.AcceptTcpClientAsync(stop.Token);
            await Task.Delay(TimeSpan.FromSeconds(10), stop.Token);
        });

        var stopwatch = Stopwatch.StartNew();
        ServerPingResult result = await ServerPing.QueryAsync(
            new ServerAddress("127.0.0.1", port, true),
            CancellationToken.None,
            TimeSpan.FromMilliseconds(250));
        stopwatch.Stop();

        stop.Cancel();
        await Task.WhenAny(accept, Task.Delay(TimeSpan.FromSeconds(1)));

        Assert.False(result.Online);
        Assert.Equal("Timed out", result.Error);
        Assert.True(
            stopwatch.Elapsed < TimeSpan.FromSeconds(5),
            $"the ping ignored its overall budget and took {stopwatch.Elapsed}");
    }

    [Fact]
    public async Task QueryAsync_ReportsACancelledResolutionAsCancelled()
    {
        using var stop = new CancellationTokenSource();
        stop.Cancel();

        ServerPingResult result = await ServerPing.QueryAsync("play.example.com", stop.Token);

        Assert.False(result.Online);
        Assert.Equal("Cancelled", result.Error);
    }

    [Fact]
    public async Task PingManyAsync_ReportsCancellationPerEntry()
    {
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        IReadOnlyList<ServerPingResult> results = await ServerPing.PingManyAsync(
            [new ServerAddress("127.0.0.1", 1, true)], 1, cts.Token);

        Assert.Single(results);
        Assert.Equal("Cancelled", results[0].Error);
    }

    /// <summary>Serves at most one byte per read so the reference partial-read loop is exercised.</summary>
    private sealed class ChunkedStream(Stream inner, int chunkSize) : Stream
    {
        public override bool CanRead => true;
        public override bool CanSeek => false;
        public override bool CanWrite => false;
        public override long Length => throw new NotSupportedException();

        public override long Position
        {
            get => throw new NotSupportedException();
            set => throw new NotSupportedException();
        }

        public override int Read(byte[] buffer, int offset, int count) =>
            inner.Read(buffer, offset, Math.Min(count, chunkSize));

        public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default) =>
            inner.ReadAsync(buffer[..Math.Min(buffer.Length, chunkSize)], cancellationToken);

        public override void Flush()
        {
        }

        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();

        public override void SetLength(long value) => throw new NotSupportedException();

        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }
}
