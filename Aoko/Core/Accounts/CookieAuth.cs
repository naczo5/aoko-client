using System;
using System.Collections.Generic;
using System.Globalization;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace Aoko.Core.Accounts;

/// <summary>
/// Microsoft/Xbox/Minecraft authentication for the three credential shapes the
/// importer accepts. Localts uses the Minecraft launcher client id and an
/// <c>t=</c> ticket prefix; cookies walk SISU redirects from
/// <c>login.live.com</c>. Messages carry service names and HTTP status codes
/// only — no token, cookie value, or response body ever reaches them.
/// </summary>
internal static class CookieAuth
{
    private const string SisuAuthUrl =
        "https://sisu.xboxlive.com/connect/XboxLive/?state=login&cobrandId=8058f65d-ce06-4c30-9559-473c9275a65d&tid=896928775&ru=https%3A%2F%2Fwww.minecraft.net%2Fen-us%2Flogin&aid=1142970254";
    private const string CookieUserAgent =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:146.0) Gecko/20100101 Firefox/146.0";
    private const string MinecraftOauthClientId = "00000000402b5328";
    private const string MinecraftOauthScope = "service::user.auth.xboxlive.com::MBI_SSL";

    private const int MaxRedirectHops = 10;

    private static readonly TimeSpan AuthTimeout = TimeSpan.FromSeconds(45);
    private static readonly TimeSpan ProfileTimeout = TimeSpan.FromSeconds(30);

    private static readonly HttpClient Http = CreateClient(AuthTimeout, handler: null);

    public static Task<MinecraftProfile> AuthenticateAsync(ParsedCookies cookies, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(cookies);

        if (!string.IsNullOrWhiteSpace(cookies.RefreshToken))
            return ProfileFromRefreshTokenAsync(cookies.RefreshToken.Trim(), cancellationToken);

        if (!string.IsNullOrWhiteSpace(cookies.AccessToken))
            return ProfileFromAccessTokenAsync(cookies.AccessToken.Trim(), cancellationToken);

        return CookiesToProfileAsync(cookies, cancellationToken);
    }

    public static async Task<MinecraftProfile> ProfileFromRefreshTokenAsync(string refresh, CancellationToken cancellationToken = default)
    {
        (string access, string rotated) = await ExchangeRefreshTokenAsync(refresh, cancellationToken).ConfigureAwait(false);
        MinecraftProfile profile = await ProfileFromMsaAsync(access, "t=", cancellationToken).ConfigureAwait(false);
        return new MinecraftProfile(profile.Name, profile.Uuid, profile.AccessToken, rotated, AccountSources.Localts);
    }

    public static Task<MinecraftProfile> ProfileFromAccessTokenAsync(string token, CancellationToken cancellationToken = default)
    {
        ThrowIfMcaExpired(token.Trim());
        return ProfileFromMcaAsync(token.Trim(), cancellationToken);
    }

    /// <summary>
    /// Classification shared by every HTTP failure path: 408, 429 and 5xx are
    /// worth retrying, and a server <c>Retry-After</c> is passed through.
    /// </summary>
    internal static CookieAuthException FailureFromResponse(
        string service,
        HttpResponseMessage response,
        string? message = null)
    {
        ArgumentNullException.ThrowIfNull(response);

        int status = (int)response.StatusCode;
        bool transient = status == 408 || status == 429 || status >= 500;

        TimeSpan? retryAfter = response.Headers.RetryAfter?.Delta;
        if (retryAfter == null && response.Headers.RetryAfter?.Date is { } retryDate)
            retryAfter = retryDate - DateTimeOffset.UtcNow;
        if (retryAfter < TimeSpan.Zero)
            retryAfter = TimeSpan.Zero;

        string safeMessage = message
            ?? (status == 429
                ? $"{service} throttled the request (HTTP 429)."
                : $"{service} failed (HTTP {status} {response.StatusCode}).");
        return new CookieAuthException(safeMessage, response.StatusCode, transient, retryAfter);
    }

    private static StringContent JsonBody<T>(T payload) =>
        new(JsonSerializer.Serialize(payload), Encoding.UTF8, "application/json");

    private static HttpClient CreateClient(TimeSpan timeout, HttpClientHandler? handler)
    {
        handler ??= new HttpClientHandler
        {
            AllowAutoRedirect = false,
            AutomaticDecompression = DecompressionMethods.GZip | DecompressionMethods.Deflate
        };
        var client = new HttpClient(handler, disposeHandler: false) { Timeout = timeout };
        client.DefaultRequestHeaders.TryAddWithoutValidation("User-Agent", CookieUserAgent);
        return client;
    }

    private static async Task<MinecraftProfile> CookiesToProfileAsync(ParsedCookies cookies, CancellationToken cancellationToken)
    {
        string mca = await CookiesToMcaViaSisuAsync(cookies, cancellationToken).ConfigureAwait(false);
        return await ProfileFromMcaAsync(mca, cancellationToken).ConfigureAwait(false);
    }

    private static async Task<(string Access, string Refresh)> ExchangeRefreshTokenAsync(
        string refresh,
        CancellationToken cancellationToken)
    {
        var form = new Dictionary<string, string>
        {
            ["client_id"] = MinecraftOauthClientId,
            ["refresh_token"] = refresh,
            ["grant_type"] = "refresh_token",
            ["redirect_uri"] = "https://login.live.com/oauth20_desktop.srf",
            ["scope"] = MinecraftOauthScope
        };

        using var content = new FormUrlEncodedContent(form);
        using var request = new HttpRequestMessage(HttpMethod.Post, "https://login.live.com/oauth20_token.srf")
        {
            Content = content
        };
        request.Headers.TryAddWithoutValidation("Accept", "application/json");

        using HttpResponseMessage response = await Http.SendAsync(request, cancellationToken).ConfigureAwait(false);
        string body = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
            throw FailureFromResponse("Microsoft token exchange", response);

        using JsonDocument document = ParseBody(body, "Microsoft token exchange");
        if (!document.RootElement.TryGetProperty("access_token", out JsonElement accessElement)
            || string.IsNullOrWhiteSpace(accessElement.GetString()))
        {
            throw new CookieAuthException("Localts refresh response missing access_token.");
        }

        string access = accessElement.GetString()!;
        string rotated = document.RootElement.TryGetProperty("refresh_token", out JsonElement refreshElement)
            && !string.IsNullOrWhiteSpace(refreshElement.GetString())
                ? refreshElement.GetString()!
                : refresh;
        return (access, rotated);
    }

    private static async Task<string> CookiesToMcaViaSisuAsync(ParsedCookies cookies, CancellationToken cancellationToken)
    {
        using var handler = new HttpClientHandler
        {
            AllowAutoRedirect = false,
            AutomaticDecompression = DecompressionMethods.GZip | DecompressionMethods.Deflate,
            CookieContainer = new CookieContainer(),
            UseCookies = true
        };
        ImportCookies(handler.CookieContainer, cookies);

        using HttpClient client = CreateClient(AuthTimeout, handler);

        var current = new Uri(SisuAuthUrl);
        string? encoded = null;
        for (int hop = 0; hop < MaxRedirectHops; hop++)
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, current);
            request.Headers.TryAddWithoutValidation(
                "Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8");
            request.Headers.TryAddWithoutValidation("Accept-Language", "en-US,en;q=0.8");

            using HttpResponseMessage response = await client.SendAsync(request, cancellationToken).ConfigureAwait(false);
            if (!IsRedirect(response.StatusCode))
                throw FailureFromResponse("Microsoft/Xbox SISU authentication", response);
            if (response.Headers.Location == null)
                throw new CookieAuthException("SISU redirect missing Location header.");

            current = response.Headers.Location.IsAbsoluteUri
                ? response.Headers.Location
                : new Uri(current, response.Headers.Location);
            encoded = ExtractSisuAccessToken(current.AbsoluteUri);
            if (!string.IsNullOrWhiteSpace(encoded))
                break;
        }

        if (string.IsNullOrWhiteSpace(encoded))
            throw new CookieAuthException("No Xbox access token was returned. The Microsoft cookies may be expired.");

        string decoded;
        try
        {
            decoded = Encoding.UTF8.GetString(Convert.FromBase64String(PadBase64(encoded)));
        }
        catch (FormatException)
        {
            throw new CookieAuthException("The SISU access token response was malformed.");
        }

        int relyingParty = decoded.IndexOf("\"rp://api.minecraftservices.com/\",", StringComparison.Ordinal);
        if (relyingParty < 0)
            throw new CookieAuthException("Unexpected SISU token payload.");

        string slice = decoded[relyingParty..];
        const string hashNeedle = "{\"DisplayClaims\":{\"xui\":[{\"uhs\":\"";
        const string tokenNeedle = "\"Token\":\"";
        int hashMarker = slice.IndexOf(hashNeedle, StringComparison.Ordinal);
        int tokenMarker = slice.IndexOf(tokenNeedle, StringComparison.Ordinal);
        if (hashMarker < 0 || tokenMarker < 0)
            throw new CookieAuthException("Unable to parse SISU XSTS payload.");

        string hash = slice[(hashMarker + hashNeedle.Length)..];
        hash = hash[..hash.IndexOf('"')];
        string xsts = slice[(tokenMarker + tokenNeedle.Length)..];
        xsts = xsts[..xsts.IndexOf('"')];
        return await XstsToMcaAsync(xsts, hash, cancellationToken).ConfigureAwait(false);
    }

    private static void ImportCookies(CookieContainer container, ParsedCookies parsed)
    {
        int imported = 0;
        foreach (CookieEntry entry in parsed.Cookies.Values)
        {
            string domain = entry.Domain.Trim().TrimStart('.');
            if (string.IsNullOrWhiteSpace(domain)
                || (!domain.EndsWith("live.com", StringComparison.OrdinalIgnoreCase)
                    && !domain.EndsWith("xboxlive.com", StringComparison.OrdinalIgnoreCase)))
            {
                continue;
            }

            try
            {
                var cookie = new Cookie(
                    entry.Name,
                    entry.Value,
                    string.IsNullOrWhiteSpace(entry.Path) ? "/" : entry.Path,
                    domain)
                {
                    Secure = entry.Secure,
                    HttpOnly = entry.Name.StartsWith("__Host-", StringComparison.Ordinal)
                };
                container.Add(cookie);
                imported++;
            }
            catch (CookieException)
            {
                // Unrelated malformed browser cookies are skipped; the required
                // auth cookies are checked below.
            }
        }

        var login = new Uri("https://login.live.com/");
        bool hasAuth = false;
        foreach (Cookie cookie in container.GetCookies(login))
        {
            if (cookie.Name is "__Host-MSAAUTHP" or "__Host-MSAAUTH")
            {
                hasAuth = true;
                break;
            }
        }

        if (imported == 0 || !hasAuth)
            throw new CookieAuthException("No usable Microsoft authentication cookies were found.");
    }

    private static bool IsRedirect(HttpStatusCode status) =>
        status is HttpStatusCode.Moved
            or HttpStatusCode.Redirect
            or HttpStatusCode.RedirectMethod
            or HttpStatusCode.TemporaryRedirect
            or HttpStatusCode.PermanentRedirect;

    private static string PadBase64(string value) =>
        value.PadRight(value.Length + ((4 - (value.Length % 4)) % 4), '=');

    private static JsonDocument ParseBody(string body, string service)
    {
        try
        {
            return JsonDocument.Parse(body);
        }
        catch (JsonException)
        {
            throw new CookieAuthException($"{service} returned an unreadable response.");
        }
    }

    /// <summary>Direct MCA tokens live ~24h and cannot be refreshed, so an expired one is rejected early.</summary>
    internal static void ThrowIfMcaExpired(string token, DateTimeOffset? now = null)
    {
        try
        {
            string[] parts = token.Split('.');
            if (parts.Length != 3)
                return;

            byte[] bytes = Convert.FromBase64String(PadBase64(parts[1].Replace('-', '+').Replace('_', '/')));
            using JsonDocument document = JsonDocument.Parse(bytes);
            if (!document.RootElement.TryGetProperty("exp", out JsonElement exp))
                return;

            long seconds = exp.ValueKind == JsonValueKind.Number
                ? exp.GetInt64()
                : long.Parse(exp.GetString() ?? "");
            var expiry = DateTimeOffset.FromUnixTimeSeconds(seconds);
            if ((now ?? DateTimeOffset.UtcNow) > expiry.AddMinutes(1))
            {
                // Invariant: a Thai or Japanese-era locale would otherwise print a wrong year.
                string stamp = expiry.ToString("yyyy-MM-dd HH:mm", CultureInfo.InvariantCulture);
                throw new CookieAuthException(
                    $"Minecraft access token expired on {stamp} UTC. Re-export a fresh token file — access tokens last ~24h and cannot be refreshed.");
            }
        }
        catch (CookieAuthException)
        {
            throw;
        }
        catch
        {
            // Unreadable expiry is left to the profile lookup.
        }
    }

    private static string? ExtractSisuAccessToken(string url)
    {
        int index = url.IndexOf("accessToken=", StringComparison.Ordinal);
        if (index < 0)
            return null;

        string raw = url[(index + "accessToken=".Length)..];
        int ampersand = raw.IndexOf('&');
        if (ampersand >= 0)
            raw = raw[..ampersand];
        int hash = raw.IndexOf('#');
        if (hash >= 0)
            raw = raw[..hash];

        try
        {
            return Uri.UnescapeDataString(raw.Replace('+', ' '));
        }
        catch
        {
            return raw;
        }
    }

    private static async Task<string> XstsToMcaAsync(string xsts, string hash, CancellationToken cancellationToken)
    {
        using StringContent content = JsonBody(new { identityToken = $"XBL3.0 x={hash};{xsts}" });
        using var request = new HttpRequestMessage(HttpMethod.Post, "https://api.minecraftservices.com/authentication/login_with_xbox")
        {
            Content = content
        };
        request.Headers.TryAddWithoutValidation("Accept", "application/json");

        using HttpResponseMessage response = await Http.SendAsync(request, cancellationToken).ConfigureAwait(false);
        string body = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
            throw FailureFromResponse("Minecraft token exchange", response);

        using JsonDocument document = ParseBody(body, "Minecraft token exchange");
        if (!document.RootElement.TryGetProperty("access_token", out JsonElement accessElement)
            || string.IsNullOrWhiteSpace(accessElement.GetString()))
        {
            throw new CookieAuthException("Minecraft login_with_xbox missing access_token.");
        }

        return accessElement.GetString()!;
    }

    private static async Task<MinecraftProfile> ProfileFromMcaAsync(string mca, CancellationToken cancellationToken)
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(ProfileTimeout);

        using var request = new HttpRequestMessage(HttpMethod.Get, "https://api.minecraftservices.com/minecraft/profile");
        request.Headers.TryAddWithoutValidation("Authorization", "Bearer " + mca);

        using HttpResponseMessage response = await Http.SendAsync(request, timeout.Token).ConfigureAwait(false);
        string body = await response.Content.ReadAsStringAsync(timeout.Token).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            string message = response.StatusCode is HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden
                ? "Minecraft profile lookup rejected the token; the credentials may be expired or revoked."
                : "Minecraft profile lookup failed.";
            throw FailureFromResponse("Minecraft profile lookup", response, message);
        }

        using JsonDocument document = ParseBody(body, "Minecraft profile lookup");
        string? name = document.RootElement.TryGetProperty("name", out JsonElement nameElement)
            ? nameElement.GetString()
            : null;
        string? uuid = document.RootElement.TryGetProperty("id", out JsonElement idElement)
            ? idElement.GetString()
            : null;
        if (string.IsNullOrWhiteSpace(name))
            throw new CookieAuthException("Minecraft profile response missing name.");
        if (string.IsNullOrWhiteSpace(uuid))
            throw new CookieAuthException("Minecraft profile response missing id.");

        return new MinecraftProfile(name, uuid, mca);
    }

    private static async Task<MinecraftProfile> ProfileFromMsaAsync(
        string msa,
        string ticketPrefix,
        CancellationToken cancellationToken)
    {
        string mca = await MsaToMcaAsync(msa, ticketPrefix, cancellationToken).ConfigureAwait(false);
        return await ProfileFromMcaAsync(mca, cancellationToken).ConfigureAwait(false);
    }

    private static async Task<string> MsaToMcaAsync(
        string msa,
        string ticketPrefix,
        CancellationToken cancellationToken)
    {
        var xblPayload = new
        {
            Properties = new
            {
                AuthMethod = "RPS",
                SiteName = "user.auth.xboxlive.com",
                RpsTicket = ticketPrefix + msa
            },
            RelyingParty = "http://auth.xboxlive.com",
            TokenType = "JWT"
        };

        string xbl;
        using (StringContent content = JsonBody(xblPayload))
        using (var request = new HttpRequestMessage(HttpMethod.Post, "https://user.auth.xboxlive.com/user/authenticate")
               {
                   Content = content
               })
        {
            request.Headers.TryAddWithoutValidation("Accept", "application/json");
            using HttpResponseMessage response = await Http.SendAsync(request, cancellationToken).ConfigureAwait(false);
            string body = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
                throw FailureFromResponse("Xbox Live authentication", response);

            using JsonDocument document = ParseBody(body, "Xbox Live authentication");
            if (!document.RootElement.TryGetProperty("Token", out JsonElement tokenElement)
                || string.IsNullOrWhiteSpace(tokenElement.GetString()))
            {
                throw new CookieAuthException("Xbox Live authentication response missing Token.");
            }

            xbl = tokenElement.GetString()!;
        }

        var xstsPayload = new
        {
            Properties = new
            {
                SandboxId = "RETAIL",
                UserTokens = new[] { xbl }
            },
            RelyingParty = "rp://api.minecraftservices.com/",
            TokenType = "JWT"
        };

        using StringContent xstsContent = JsonBody(xstsPayload);
        using var xstsRequest = new HttpRequestMessage(
            HttpMethod.Post,
            "https://xsts.auth.xboxlive.com/xsts/authorize")
        {
            Content = xstsContent
        };
        xstsRequest.Headers.TryAddWithoutValidation("Accept", "application/json");

        using HttpResponseMessage xstsResponse = await Http.SendAsync(xstsRequest, cancellationToken).ConfigureAwait(false);
        string xstsBody = await xstsResponse.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
        if (xstsResponse.StatusCode == HttpStatusCode.Unauthorized)
        {
            throw new CookieAuthException(
                "Xbox rejected this account (HTTP 401). It may not have an Xbox/Minecraft profile linked.",
                HttpStatusCode.Unauthorized);
        }

        if (!xstsResponse.IsSuccessStatusCode)
            throw FailureFromResponse("Xbox XSTS authorization", xstsResponse);

        using JsonDocument xstsDocument = ParseBody(xstsBody, "Xbox XSTS authorization");
        string? hash = xstsDocument.RootElement.TryGetProperty("DisplayClaims", out JsonElement claims)
            && claims.TryGetProperty("xui", out JsonElement xui)
            && xui.ValueKind == JsonValueKind.Array
            && xui.GetArrayLength() > 0
            && xui[0].TryGetProperty("uhs", out JsonElement uhs)
                ? uhs.GetString()
                : null;
        string? xsts = xstsDocument.RootElement.TryGetProperty("Token", out JsonElement xstsToken)
            ? xstsToken.GetString()
            : null;
        if (string.IsNullOrWhiteSpace(hash))
            throw new CookieAuthException("XSTS response missing uhs.");
        if (string.IsNullOrWhiteSpace(xsts))
            throw new CookieAuthException("XSTS response missing Token.");

        return await XstsToMcaAsync(xsts, hash, cancellationToken).ConfigureAwait(false);
    }
}
