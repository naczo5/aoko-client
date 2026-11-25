using System.Text;
using Aoko.Core.Servers;

namespace Aoko.Tests;

public class ServerAddressTests
{
    [Theory]
    [InlineData("play.hypixel.net", "play.hypixel.net", 25565, false)]
    [InlineData("  mc.example.com:25566  ", "mc.example.com", 25566, true)]
    [InlineData("[::1]:25565", "::1", 25565, true)]
    [InlineData("[2001:db8::1]", "2001:db8::1", 25565, false)]
    [InlineData("::1", "::1", 25565, false)]
    [InlineData("fe80::1:2", "fe80::1:2", 25565, false)]
    [InlineData("example.com:1", "example.com", 1, true)]
    [InlineData("example.com:65535", "example.com", 65535, true)]
    public void TryParse_AcceptsSupportedForms(string text, string host, int port, bool portSpecified)
    {
        Assert.True(ServerAddress.TryParse(text, out ServerAddress parsed));
        Assert.Equal(host, parsed.Host);
        Assert.Equal(port, parsed.Port);
        Assert.Equal(portSpecified, parsed.PortSpecified);
    }

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData(null)]
    [InlineData("example.com:0")]
    [InlineData("example.com:65536")]
    [InlineData("example.com:70000")]
    [InlineData("example.com:-1")]
    [InlineData("example.com:abc")]
    [InlineData("example.com: 25565")]
    [InlineData("[::1")]
    [InlineData("[]")]
    [InlineData("[]:25565")]
    [InlineData("[::1]x")]
    [InlineData("[::1]:0")]
    [InlineData("a b")]
    [InlineData("host/path")]
    public void TryParse_RejectsInvalidForms(string? text) => Assert.False(ServerAddress.TryParse(text, out _));

    [Fact]
    public void TryParse_KeepsATrailingColonAsABareHost()
    {
        // A colon at the last position never splits into host:port (reference
        // MinecraftServerAddress.cs:45 requires a character after the colon).
        Assert.True(ServerAddress.TryParse("example.com:", out ServerAddress parsed));
        Assert.Equal("example.com:", parsed.Host);
        Assert.Equal(25565, parsed.Port);
        Assert.False(parsed.PortSpecified);
    }

    [Fact]
    public void TryParse_EnforcesThe256CharacterCap()
    {
        Assert.True(ServerAddress.TryParse(new string('a', 256), out _));
        Assert.False(ServerAddress.TryParse(new string('a', 257), out _));

        string atCap = new string('a', 250) + ":25565";
        Assert.Equal(256, atCap.Length);
        Assert.True(ServerAddress.TryParse(atCap, out ServerAddress parsed));
        Assert.Equal(250, parsed.Host.Length);
        Assert.False(ServerAddress.TryParse(new string('a', 251) + ":25565", out _));
    }

    [Theory]
    [InlineData("host", 25565, true, "host:25565")]
    [InlineData("host", 25565, false, "host")]
    [InlineData("host", 25566, false, "host:25566")]
    [InlineData("::1", 25565, false, "::1")]
    [InlineData("::1", 25566, false, "[::1]:25566")]
    public void Display_FormatsPortAndIpv6(string host, int port, bool portSpecified, string expected) =>
        Assert.Equal(expected, new ServerAddress(host, port, portSpecified).Display);

    [Fact]
    public async Task ResolveForPingAsync_KeepsAnExplicitPort()
    {
        ServerAddress resolved = await ServerAddress.ResolveForPingAsync("mc.example.com:25566");

        Assert.Equal("mc.example.com", resolved.Host);
        Assert.Equal(25566, resolved.Port);
        Assert.True(resolved.PortSpecified);
    }

    [Fact]
    public async Task ResolveForPingAsync_RejectsGarbage()
    {
        ArgumentException ex = await Assert.ThrowsAsync<ArgumentException>(
            () => ServerAddress.ResolveForPingAsync("bad host"));

        // Exact match: the framework's "(Parameter 'text')" suffix must not reach the status bar.
        Assert.Equal(ServerAddress.InvalidAddressMessage, ex.Message);
    }

    [Fact]
    public void BuildSrvQuery_EncodesTheMinecraftServiceName()
    {
        byte[] query = ServerAddress.BuildSrvQuery("_minecraft._tcp.play.hypixel.net");

        Assert.Equal(0x1C, query[0]);
        Assert.Equal(0x0D, query[1]);
        Assert.Equal(0x01, query[2]);
        Assert.Equal(0x00, query[3]);
        Assert.Equal(1, query[5]);
        Assert.Equal(0, query[7]);
        Assert.Equal(10, query[12]);
        Assert.Equal("_minecraft", Encoding.ASCII.GetString(query, 13, 10));
        Assert.Equal(4, query[23]);
        Assert.Equal("_tcp", Encoding.ASCII.GetString(query, 24, 4));

        int tail = query.Length - 5;
        Assert.Equal(0, query[tail]);
        Assert.Equal(0x00, query[tail + 1]);
        Assert.Equal(0x21, query[tail + 2]);
        Assert.Equal(0x00, query[tail + 3]);
        Assert.Equal(0x01, query[tail + 4]);
    }

    [Fact]
    public void TryReadSrv_ParsesALiteralTarget()
    {
        byte[] response = DnsResponse.Build("proxy.example.com", 25566);

        Assert.True(ServerAddress.TryReadSrv(response, out string target, out int port));
        Assert.Equal("proxy.example.com", target);
        Assert.Equal(25566, port);
    }

    [Fact]
    public void TryReadSrv_SkipsACnameAndFollowsACompressionPointer()
    {
        byte[] response = DnsResponse.Build("proxy.example.com", 25565, leadingCname: true);

        Assert.True(ServerAddress.TryReadSrv(response, out string target, out int port));
        Assert.Equal("proxy.example.com", target);
        Assert.Equal(25565, port);
    }

    [Fact]
    public void TryReadSrv_RejectsTruncatedResponses()
    {
        Assert.False(ServerAddress.TryReadSrv(Array.Empty<byte>(), out _, out _));
        Assert.False(ServerAddress.TryReadSrv(new byte[11], out _, out _));

        byte[] response = DnsResponse.Build("proxy.example.com", 25566);
        Assert.False(ServerAddress.TryReadSrv(response[..^4], out _, out _));
    }

    [Fact]
    public void TryReadSrv_AcceptsOnlyTheAnswerToOurOwnQuestion()
    {
        byte[] response = DnsResponse.Build("proxy.example.com", 25566);

        Assert.True(ServerAddress.TryReadSrv(
            response, "_minecraft._tcp.example.com", out string target, out int port));
        Assert.Equal("proxy.example.com", target);
        Assert.Equal(25566, port);

        // Somebody else's answer, or one for another name, must not be cached as this server.
        Assert.False(ServerAddress.TryReadSrv(
            response, "_minecraft._tcp.other.net", out _, out _));

        byte[] foreignTransaction = (byte[])response.Clone();
        foreignTransaction[0] ^= 0xFF;
        Assert.False(ServerAddress.TryReadSrv(
            foreignTransaction, "_minecraft._tcp.example.com", out _, out _));

        byte[] notAResponse = (byte[])response.Clone();
        notAResponse[2] &= 0x7F;
        Assert.False(ServerAddress.TryReadSrv(
            notAResponse, "_minecraft._tcp.example.com", out _, out _));
    }

    [Fact]
    public void ValidateTarget_ReturnsTheUserFacingMessage()
    {
        ServerTarget ok = ServerJoinService.ValidateTarget("play.hypixel.net");
        Assert.True(ok.Ok);
        Assert.Equal("", ok.Error);
        Assert.Equal("play.hypixel.net", ok.Address.Display);

        ServerTarget bad = ServerJoinService.ValidateTarget("bad host");
        Assert.False(bad.Ok);
        Assert.Equal("Enter a server as host or host:port.", bad.Error);

        ServerTarget empty = ServerJoinService.ValidateTarget(null);
        Assert.False(empty.Ok);
        Assert.Equal("Enter a server as host or host:port.", empty.Error);
    }

    [Fact]
    public void TryBuildJoinRequest_ReturnsTheBridgeHostAndPort()
    {
        Assert.True(ServerJoinService.TryBuildJoinRequest(
            new ServerAddress("mc.example.com", 25566, true), out string host, out int port));
        Assert.Equal("mc.example.com", host);
        Assert.Equal(25566, port);

        Assert.True(ServerJoinService.TryBuildJoinRequest(
            new ServerAddress("  padded.example.com ", 25565, false), out string trimmed, out _));
        Assert.Equal("padded.example.com", trimmed);
    }

    [Fact]
    public void TryBuildJoinRequest_RejectsOutOfRangeTargets()
    {
        Assert.False(ServerJoinService.TryBuildJoinRequest(default, out _, out _));
        Assert.False(ServerJoinService.TryBuildJoinRequest(new ServerAddress("host", 0, true), out _, out _));
        Assert.False(ServerJoinService.TryBuildJoinRequest(new ServerAddress("host", 65536, true), out _, out _));
        Assert.False(ServerJoinService.TryBuildJoinRequest(
            new ServerAddress(new string('a', 257), 25565, false), out _, out _));
        Assert.True(ServerJoinService.TryBuildJoinRequest(
            new ServerAddress(new string('a', 256), 25565, false), out _, out _));
    }

    /// <summary>Builds a DNS response with one question and an optional CNAME ahead of the SRV answer.</summary>
    private sealed class DnsResponse
    {
        private readonly List<byte> _bytes = new();

        public static byte[] Build(string target, int port, bool leadingCname = false)
        {
            var writer = new DnsResponse();
            writer.U16(0x1C0D);
            writer.U16(0x8180);
            writer.U16(1);
            writer.U16(leadingCname ? 2 : 1);
            writer.U16(0);
            writer.U16(0);

            int nameStart = writer.Position;
            writer.Name("_minecraft", "_tcp", "example.com");
            writer.U16(33);
            writer.U16(1);

            int cnameStart = 0;
            if (leadingCname)
            {
                writer.Pointer(nameStart);
                writer.U16(5);
                writer.U16(1);
                writer.U32(0);
                int cnameLength = writer.ReserveU16();
                cnameStart = writer.Position;
                writer.Name("proxy", "example.com");
                writer.PatchU16(cnameLength, writer.Position - cnameStart);
            }

            writer.Pointer(nameStart);
            writer.U16(33);
            writer.U16(1);
            writer.U32(0);
            int rdataLength = writer.ReserveU16();
            int rdataStart = writer.Position;
            writer.U16(0);
            writer.U16(0);
            writer.U16(port);
            if (leadingCname)
                writer.Pointer(cnameStart);
            else
                writer.Name(target.Split('.'));
            writer.PatchU16(rdataLength, writer.Position - rdataStart);
            return writer._bytes.ToArray();
        }

        private int Position => _bytes.Count;

        private void U16(int value)
        {
            _bytes.Add((byte)(value >> 8));
            _bytes.Add((byte)(value & 0xFF));
        }

        private void U32(uint value)
        {
            _bytes.Add((byte)(value >> 24));
            _bytes.Add((byte)(value >> 16));
            _bytes.Add((byte)(value >> 8));
            _bytes.Add((byte)value);
        }

        private void Name(params string[] labels)
        {
            foreach (string label in labels)
            {
                byte[] encoded = Encoding.ASCII.GetBytes(label);
                _bytes.Add((byte)encoded.Length);
                _bytes.AddRange(encoded);
            }

            _bytes.Add(0);
        }

        private void Pointer(int offset) => U16(0xC000 | (offset & 0x3FFF));

        private int ReserveU16()
        {
            int position = _bytes.Count;
            U16(0);
            return position;
        }

        private void PatchU16(int position, int value)
        {
            _bytes[position] = (byte)(value >> 8);
            _bytes[position + 1] = (byte)(value & 0xFF);
        }
    }
}
