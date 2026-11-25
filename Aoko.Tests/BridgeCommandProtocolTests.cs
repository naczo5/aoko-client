using System.Collections.Concurrent;
using System.Text.Json.Nodes;
using Aoko.Core;

namespace Aoko.Tests;

/// <summary>
/// Message shapes for the session/server commands. The native side is written
/// against these exact lines, so the assertions are literal strings.
/// </summary>
public sealed class BridgeCommandProtocolTests
{
    [Fact]
    public void GetSession_BuildsContractLine()
    {
        string line = BridgeCommandWire.BuildAccountCommandLine(
            123,
            AccountCommand.GetSession());

        Assert.Equal(
            "{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":123}\n",
            line);
    }

    [Fact]
    public void SetSession_BuildsContractLineWithRefreshToken()
    {
        string line = BridgeCommandWire.BuildAccountCommandLine(
            124,
            AccountCommand.SetSession(
                "Notch",
                "069a79f4-44e9-4726-a5be-fca90e38aaf5",
                "msa-token",
                "refresh-token"));

        Assert.Equal(
            "{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":124," +
            "\"name\":\"Notch\",\"uuid\":\"069a79f4-44e9-4726-a5be-fca90e38aaf5\"," +
            "\"token\":\"msa-token\",\"refreshToken\":\"refresh-token\"}\n",
            line);
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    public void SetSession_OmitsBlankRefreshToken(string? refreshToken)
    {
        string line = BridgeCommandWire.BuildAccountCommandLine(
            124,
            AccountCommand.SetSession("Notch", "uuid-1", "msa-token", refreshToken));

        Assert.DoesNotContain("refreshToken", line);
        Assert.Equal(
            "{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":124," +
            "\"name\":\"Notch\",\"uuid\":\"uuid-1\",\"token\":\"msa-token\"}\n",
            line);
    }

    [Fact]
    public void RestoreSession_BuildsContractLine()
    {
        string line = BridgeCommandWire.BuildAccountCommandLine(
            125,
            AccountCommand.RestoreSession());

        Assert.Equal(
            "{\"type\":\"accountCommand\",\"action\":\"restoreSession\",\"requestId\":125}\n",
            line);
    }

    [Fact]
    public void JoinServer_BuildsContractLine()
    {
        string line = BridgeCommandWire.BuildServerCommandLine(126, "mc.example.com", 25565);

        Assert.Equal(
            "{\"type\":\"serverCommand\",\"action\":\"join\",\"requestId\":126," +
            "\"host\":\"mc.example.com\",\"port\":25565}\n",
            line);
    }

    [Fact]
    public void CommandLines_AreSingleJsonObjectPerLine()
    {
        string[] lines =
        {
            BridgeCommandWire.BuildAccountCommandLine(1, AccountCommand.GetSession()),
            BridgeCommandWire.BuildAccountCommandLine(
                2,
                AccountCommand.SetSession("Notch", "uuid", "token")),
            BridgeCommandWire.BuildAccountCommandLine(3, AccountCommand.RestoreSession()),
            BridgeCommandWire.BuildServerCommandLine(4, "host", 25565)
        };

        foreach (string line in lines)
        {
            Assert.EndsWith("\n", line);
            Assert.Equal(1, line.Count(character => character == '\n'));
            Assert.DoesNotContain("\r", line);
            Assert.IsType<JsonObject>(JsonNode.Parse(line));
        }
    }

    [Fact]
    public void SetSession_EscapesUserNamesWithoutBreakingTheLine()
    {
        string line = BridgeCommandWire.BuildAccountCommandLine(
            7,
            AccountCommand.SetSession("weird\"name", "uuid", "token"));

        Assert.Equal(1, line.Count(character => character == '\n'));
        Assert.Equal(
            "weird\"name",
            JsonNode.Parse(line)!["name"]!.GetValue<string>());
    }

    [Fact]
    public void AccountCommand_ToStringNeverPrintsCredentials()
    {
        AccountCommand command = AccountCommand.SetSession(
            "Notch",
            "uuid-1",
            "super-secret-token",
            "super-secret-refresh");

        string text = command.ToString();

        Assert.Equal("AccountCommand { Action = setSession }", text);
        Assert.DoesNotContain("secret", text);
    }

    [Fact]
    public void Credentials_AreOnlyAttachedToSetSession()
    {
        Assert.False(AccountCommand.GetSession().CarriesCredentials);
        Assert.False(AccountCommand.RestoreSession().CarriesCredentials);
        Assert.True(AccountCommand.SetSession("a", "b", "c").CarriesCredentials);
    }

    [Fact]
    public void RequestIdSource_IsMonotonic()
    {
        var source = new BridgeRequestIdSource();

        int first = source.Next();
        int second = source.Next();
        int third = source.Next();

        Assert.Equal(1, first);
        Assert.True(second > first);
        Assert.True(third > second);
    }

    [Fact]
    public void RequestIdSource_IsUniqueUnderConcurrency()
    {
        var source = new BridgeRequestIdSource();
        var ids = new ConcurrentBag<int>();

        Parallel.For(0, 256, _ => ids.Add(source.Next()));

        Assert.Equal(256, ids.Distinct().Count());
        Assert.Equal(256, ids.Max());
    }

    /// <summary>
    /// A real MSA/JWT access token is 1-2 KB, so a setSession line routinely
    /// exceeds the native 4096-byte recv buffer. The native reader must allow
    /// 64 KiB lines; this pins the size the wire actually produces.
    /// </summary>
    [Fact]
    public void SetSession_WithA2kTokenRoundTripsUnescapedAndFitsTheLegacyBuffer()
    {
        string token = BuildJwtLikeToken(2000);
        string line = BridgeCommandWire.BuildAccountCommandLine(
            200,
            AccountCommand.SetSession(
                "Notch",
                "069a79f4-44e9-4726-a5be-fca90e38aaf5",
                token));

        JsonNode? parsed = JsonNode.Parse(line);

        Assert.Equal(token, parsed!["token"]!.GetValue<string>());
        Assert.Contains(token, line, StringComparison.Ordinal);
        Assert.Equal(1, line.Count(character => character == '\n'));
        Assert.True(line.Length > 2000, $"unexpected line length {line.Length}");
        Assert.True(
            line.Length < 4096,
            $"a 2 KB token produced {line.Length} characters; the legacy 4096-byte buffer cannot hold it");
    }

    [Fact]
    public void SetSession_WithAccessAndRefreshTokensExceedsTheLegacyBuffer()
    {
        string token = BuildJwtLikeToken(2000);
        string refreshToken = BuildJwtLikeToken(2000);
        string line = BridgeCommandWire.BuildAccountCommandLine(
            201,
            AccountCommand.SetSession(
                "Notch",
                "069a79f4-44e9-4726-a5be-fca90e38aaf5",
                token,
                refreshToken));

        JsonNode? parsed = JsonNode.Parse(line);

        Assert.Equal(token, parsed!["token"]!.GetValue<string>());
        Assert.Equal(refreshToken, parsed["refreshToken"]!.GetValue<string>());
        Assert.True(
            line.Length > 4096,
            $"access+refresh token line was {line.Length} characters; native lines need 64 KiB");
    }

    private static string BuildJwtLikeToken(int length)
    {
        const string segment =
            "eyJhbGciOiJSUzI1NiIsImtpZCI6IjEyMyJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.";
        var builder = new System.Text.StringBuilder(length);
        while (builder.Length < length)
            builder.Append(segment);

        return builder.ToString(0, length);
    }
}
