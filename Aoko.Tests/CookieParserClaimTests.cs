using System.Text;
using Aoko.Core.Accounts;

namespace Aoko.Tests;

/// <summary>
/// A pasted token is untrusted input. Every claim read must survive a non-string value, and no
/// failure message may quote a cookie value: parser messages reach the status bar and the batch
/// summary, and end up in screenshots and bug reports.
/// </summary>
public sealed class CookieParserClaimTests
{
    private const string Secret = "SUPER-SECRET-AUTH-VALUE";

    [Theory]
    [InlineData("""{"profiles":{"mc":"069a79f444e94726a5befca90e38aaf5"},"xuid":2533274909086470}""")]
    [InlineData("""{"profiles":{"mc":"069a79f444e94726a5befca90e38aaf5"},"xuid":["1","2"]}""")]
    [InlineData("""{"profiles":{"mc":"069a79f444e94726a5befca90e38aaf5"},"xid":{"id":"1"}}""")]
    [InlineData("""{"pfd":[{"type":1,"id":"069a79f4","name":"Notch"}],"xuid":"1"}""")]
    [InlineData("""{"pfd":[{"type":"mc","id":69,"name":"Notch"}],"xuid":"1"}""")]
    [InlineData("""{"profiles":"069a79f444e94726a5befca90e38aaf5","xuid":"1"}""")]
    [InlineData("""{"pfd":"mc","xuid":"1"}""")]
    public void APastedTokenWithNonStringClaims_FailsAsADomainError(string payloadJson)
    {
        // JsonElement.GetString() throws InvalidOperationException for these shapes, which sits
        // outside every caller's catch list and surfaced as a framework sentence in the GUI.
        CookieAuthException error = Assert.Throws<CookieAuthException>(
            () => CookieParser.FromText(Jwt(payloadJson)));

        Assert.Contains("Unrecognized cookie format", error.Message, StringComparison.Ordinal);
    }

    [Fact]
    public void AMalformedNetscapeLine_DoesNotQuoteTheCookieValue()
    {
        string text = "# Netscape HTTP Cookie File\n" + $".login.live.com\tTRUE\t/\t{Secret}\n";

        CookieAuthException error = Assert.Throws<CookieAuthException>(() => CookieParser.FromText(text));

        Assert.DoesNotContain(Secret, error.Message, StringComparison.Ordinal);
        Assert.Contains("expected at least 6 tab-separated fields", error.Message, StringComparison.Ordinal);
    }

    [Theory]
    [InlineData("nonsense")]
    [InlineData("a=b")]
    [InlineData(".example.com\tTRUE\t/")]
    [InlineData(".login.live.com\tTRUE\t/\tFALSE\t0\tother\t" + Secret)]
    [InlineData(".login.live.com\tTRUE\t/\tFALSE\t0\tother\t" + Secret + "\t9")]
    public void NoParseFailure_EchoesWhatTheUserPasted(string input)
    {
        try
        {
            CookieParser.FromText(input);
        }
        catch (CookieAuthException error)
        {
            Assert.DoesNotContain(Secret, error.Message, StringComparison.Ordinal);
        }
    }

    [Fact]
    public void AValidTokenWithStringClaims_StillParses()
    {
        string token = Jwt(
            """{"profiles":{"mc":"069a79f444e94726a5befca90e38aaf5"},"xuid":"2533274909086470"}""");

        ParsedCookies parsed = CookieParser.FromText(token);

        Assert.Equal(token, parsed.AccessToken);
        Assert.Empty(parsed.RefreshToken);
    }

    private static string Jwt(string payloadJson) =>
        $"{Base64Url("""{"alg":"RS256","typ":"JWT"}""")}.{Base64Url(payloadJson)}.sig";

    private static string Base64Url(string value)
    {
        string encoded = Convert.ToBase64String(Encoding.UTF8.GetBytes(value));
        return encoded.TrimEnd('=').Replace('+', '-').Replace('/', '_');
    }
}
