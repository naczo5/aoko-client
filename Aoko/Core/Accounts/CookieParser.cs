using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Aoko.Core.Accounts;

internal sealed class CookieEntry
{
    public CookieEntry(string domain, string path, bool secure, string name, string value)
    {
        Domain = domain;
        Path = path;
        Secure = secure;
        Name = name;
        Value = value;
    }

    public string Domain { get; }
    public string Path { get; }
    public bool Secure { get; }
    public string Name { get; }
    public string Value { get; }

    public string Pair() => $"{Name}={Value}";
}

internal sealed class ParsedCookies
{
    public ParsedCookies(
        IReadOnlyDictionary<string, CookieEntry> cookies,
        string? refreshToken = null,
        string? accessToken = null)
    {
        Cookies = cookies;
        RefreshToken = refreshToken ?? "";
        AccessToken = accessToken ?? "";
    }

    public IReadOnlyDictionary<string, CookieEntry> Cookies { get; }
    public string RefreshToken { get; }
    public string AccessToken { get; }

    /// <summary>
    /// Joins the <c>login.live.com</c> cookies into a request header. The SISU
    /// path imports into a <c>CookieContainer</c> instead; this is the compact
    /// form used by diagnostics and tests.
    /// </summary>
    public string ToSisuCookieHeader()
    {
        var byName = new Dictionary<string, CookieEntry>(StringComparer.Ordinal);
        foreach (CookieEntry entry in Cookies.Values)
        {
            if (!entry.Domain.EndsWith("login.live.com", StringComparison.OrdinalIgnoreCase))
                continue;
            if (!byName.ContainsKey(entry.Name))
                byName[entry.Name] = entry;
        }

        if (byName.Count == 0)
            throw new CookieAuthException("Cookie file has no login.live.com cookies for SISU auth.");

        return string.Join("; ", byName.Values.Select(e => e.Pair()));
    }
}

internal sealed class CookieAuthException : Exception
{
    public CookieAuthException(
        string message,
        HttpStatusCode? httpStatusCode = null,
        bool isTransient = false,
        TimeSpan? retryAfter = null) : base(message)
    {
        HttpStatusCode = httpStatusCode;
        IsTransient = isTransient;
        RetryAfter = retryAfter;
    }

    public HttpStatusCode? HttpStatusCode { get; }
    public bool IsTransient { get; }
    public TimeSpan? RetryAfter { get; }
}

/// <summary>
/// Parses the credential files the account UI accepts: Netscape cookie exports,
/// semicolon-separated cookie headers, Localts <c>M.C...</c> token files, and
/// bare Minecraft access-token JWTs.
/// </summary>
internal static class CookieParser
{
    private const string MsaTokenPrefix = "M.C";
    private const string HttpOnlyPrefix = "#HttpOnly_";

    private static readonly Regex SpaceNetscapeLine = new(
        @"^(\S+)\s+(TRUE|FALSE)\s+(\S+)\s+(TRUE|FALSE)\s+(\d+)\s+(\S+)(?:\s+(.*))?$",
        RegexOptions.IgnoreCase | RegexOptions.Compiled | RegexOptions.CultureInvariant);

    private static readonly Regex McaTokenPattern = new(
        @"eyJ[A-Za-z0-9_-]*\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    public static ParsedCookies FromPath(string path)
    {
        string normalized = NormalizePath(path);
        string text;
        try
        {
            text = File.ReadAllText(normalized);
        }
        catch (IOException)
        {
            throw new CookieAuthException($"Unable to read cookie file: {normalized}");
        }

        return FromText(text);
    }

    public static ParsedCookies FromText(string text)
    {
        string trimmed = NormalizeInput(text).Trim();
        if (trimmed.Contains('\t') && LooksLikeNetscape(trimmed))
            return FromNetscape(trimmed);
        if (LooksLikeCookieHeader(trimmed))
            return FromCookieHeader(trimmed);
        if (LooksLikeLocalts(trimmed))
            return FromLocalts(trimmed);
        if (TryExtractMcaToken(trimmed, out string? mca) && !string.IsNullOrWhiteSpace(mca))
            return FromMca(mca);
        if (trimmed.Contains('\t'))
            return FromNetscape(trimmed);

        throw new CookieAuthException(
            "Unrecognized cookie format. Use a Netscape cookie file, semicolon-separated cookie header, Localts token file, or Minecraft access-token (eyJ...) file.");
    }

    private static string NormalizePath(string path)
    {
        path = path.Trim();
        if (path.Length >= 2 && path[0] == '"' && path[^1] == '"')
            return path[1..^1].Trim();
        return path;
    }

    private static string NormalizeInput(string text)
    {
        text = text.Replace("\uFEFF", "").Replace("\r\n", "\n").Replace('\r', '\n');
        if (text.Contains('\t') || !LooksLikeSpaceSeparatedNetscape(text))
            return text;
        return ConvertSpaceSeparatedNetscape(text);
    }

    private static bool LooksLikeSpaceSeparatedNetscape(string text)
    {
        foreach (string line in text.Split('\n'))
        {
            string candidate = line.Trim();
            if (string.IsNullOrEmpty(candidate) || IsCommentLine(candidate))
                continue;
            candidate = StripHttpOnlyPrefix(candidate);
            return SpaceNetscapeLine.IsMatch(candidate);
        }

        return false;
    }

    private static string ConvertSpaceSeparatedNetscape(string text)
    {
        var builder = new StringBuilder(text.Length);
        foreach (string line in text.Split('\n'))
        {
            string stripped = line.Trim();
            if (string.IsNullOrEmpty(stripped) || IsCommentLine(stripped))
            {
                builder.Append(line).Append('\n');
                continue;
            }

            stripped = StripHttpOnlyPrefix(stripped);
            Match match = SpaceNetscapeLine.Match(stripped);
            if (!match.Success)
            {
                builder.Append(line).Append('\n');
                continue;
            }

            builder.Append(match.Groups[1].Value).Append('\t')
                .Append(match.Groups[2].Value).Append('\t')
                .Append(match.Groups[3].Value).Append('\t')
                .Append(match.Groups[4].Value).Append('\t')
                .Append(match.Groups[5].Value).Append('\t')
                .Append(match.Groups[6].Value);
            if (match.Groups[7].Success && !string.IsNullOrEmpty(match.Groups[7].Value))
                builder.Append('\t').Append(match.Groups[7].Value);
            builder.Append('\n');
        }

        return builder.ToString();
    }

    private static bool LooksLikeNetscape(string text)
    {
        foreach (string line in text.Split('\n'))
        {
            string candidate = line.Trim();
            if (string.IsNullOrEmpty(candidate) || IsCommentLine(candidate))
                continue;
            candidate = StripHttpOnlyPrefix(candidate);
            return candidate.Split('\t').Length >= 6;
        }

        return false;
    }

    private static bool LooksLikeCookieHeader(string text)
    {
        if (text.Contains('\t') || !text.Contains('='))
            return false;
        if (text.Contains(';'))
            return true;

        foreach (string line in text.Split('\n'))
        {
            string candidate = line.Trim();
            if (!string.IsNullOrEmpty(candidate) && !candidate.StartsWith('#') && candidate.Contains('='))
                return true;
        }

        return false;
    }

    private static ParsedCookies FromCookieHeader(string text)
    {
        var cookies = new Dictionary<string, CookieEntry>(StringComparer.Ordinal);
        const string domain = "login.live.com";
        foreach (string segment in text.Split(';'))
        {
            string entry = segment.Trim();
            if (string.IsNullOrEmpty(entry))
                continue;

            int equals = entry.IndexOf('=');
            if (equals <= 0)
                continue;

            string name = entry[..equals].Trim();
            string value = entry[(equals + 1)..].Trim();
            if (string.IsNullOrEmpty(name))
                continue;

            cookies[domain + '\0' + name] = new CookieEntry(domain, "/", true, name, value);
        }

        return ValidateAuthCookies(cookies);
    }

    private static ParsedCookies ValidateAuthCookies(Dictionary<string, CookieEntry> cookies)
    {
        if (cookies.Count == 0)
            throw new CookieAuthException("Cookie file is empty.");

        bool hasAuth = cookies.Values.Any(e => e.Name is "__Host-MSAAUTHP" or "__Host-MSAAUTH");
        if (!hasAuth)
            throw new CookieAuthException("Cookie file is missing __Host-MSAAUTHP or __Host-MSAAUTH.");

        return new ParsedCookies(cookies);
    }

    private static bool LooksLikeLocalts(string text)
    {
        foreach (string line in text.Split('\n'))
        {
            string candidate = line.Trim();
            if (string.IsNullOrEmpty(candidate) || candidate.StartsWith('#'))
                continue;
            if (candidate.Contains("Localts", StringComparison.OrdinalIgnoreCase))
                return true;
            if (LooksLikeLocaltsRefreshToken(candidate))
                return true;
            if (candidate.Contains('\t') || candidate.Contains('='))
                continue;

            int separator = candidate.IndexOf(':');
            if (separator > 0 && LooksLikeLocaltsRefreshToken(candidate[(separator + 1)..].Trim()))
                return true;
        }

        return false;
    }

    private static ParsedCookies FromLocalts(string text)
    {
        string? token = ExtractLocaltsToken(text);
        if (string.IsNullOrWhiteSpace(token))
            throw new CookieAuthException("Localts input is missing a valid MSA session token (M.C... or username:M.C...).");
        return new ParsedCookies(new Dictionary<string, CookieEntry>(), token);
    }

    private static string? ExtractLocaltsToken(string text)
    {
        foreach (string line in text.Split('\n'))
        {
            string candidate = line.Trim();
            if (string.IsNullOrEmpty(candidate)
                || candidate.StartsWith('#')
                || candidate.Contains("Localts", StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            int separator = candidate.IndexOf(':');
            if (separator > 0)
            {
                string value = candidate[(separator + 1)..].Trim();
                if (LooksLikeLocaltsRefreshToken(value))
                    return value;
            }

            int equals = candidate.IndexOf('=');
            if (equals > 0)
            {
                string value = candidate[(equals + 1)..].Trim();
                if (LooksLikeLocaltsRefreshToken(value))
                    return value;
            }
        }

        int marker = text.IndexOf(MsaTokenPrefix, StringComparison.Ordinal);
        if (marker < 0)
            return null;

        int end = marker;
        while (end < text.Length)
        {
            char current = text[end];
            if (current is '\n' or '\r' or ' ' or '\t')
                break;
            end++;
        }

        return text[marker..end];
    }

    private static bool LooksLikeLocaltsRefreshToken(string value) =>
        value.StartsWith(MsaTokenPrefix, StringComparison.Ordinal);

    private static ParsedCookies FromMca(string token) =>
        new(new Dictionary<string, CookieEntry>(), null, token.Trim().Trim('"', '\''));

    private static bool TryExtractMcaToken(string text, out string? token)
    {
        token = null;
        if (string.IsNullOrWhiteSpace(text) || text.Contains('\t'))
            return false;

        // Also handles {"mcToken":"eyJ..."} wrappers and a "Bearer eyJ..." prefix.
        foreach (Match match in McaTokenPattern.Matches(text))
        {
            string candidate = match.Value.Trim().Trim('"', '\'');
            if (!IsMinecraftAccessToken(candidate))
                continue;
            token = candidate;
            return true;
        }

        return false;
    }

    private static bool IsMinecraftAccessToken(string jwt)
    {
        string[] parts = jwt.Split('.');
        if (parts.Length != 3)
            return false;

        JsonElement header;
        JsonElement payload;
        try
        {
            using var headerDocument = JsonDocument.Parse(DecodeBase64Url(parts[0]));
            header = headerDocument.RootElement.Clone();
            using var payloadDocument = JsonDocument.Parse(DecodeBase64Url(parts[1]));
            payload = payloadDocument.RootElement.Clone();
        }
        catch
        {
            return false;
        }

        if (!header.TryGetProperty("alg", out _))
            return false;

        // login_with_xbox tokens carry the Minecraft profile plus Xbox ids:
        // profiles.mc uuid or pfd[type=mc].name/id, with xuid/xid.
        bool hasMinecraftProfile = false;
        if (payload.TryGetProperty("profiles", out JsonElement profiles)
            && profiles.ValueKind == JsonValueKind.Object
            && !string.IsNullOrWhiteSpace(ReadString(profiles, "mc")))
        {
            hasMinecraftProfile = true;
        }
        else if (payload.TryGetProperty("pfd", out JsonElement pfd)
            && pfd.ValueKind == JsonValueKind.Array)
        {
            foreach (JsonElement entry in pfd.EnumerateArray())
            {
                if (entry.ValueKind != JsonValueKind.Object)
                    continue;

                bool isMinecraft = string.Equals(ReadString(entry, "type"), "mc", StringComparison.OrdinalIgnoreCase);
                bool hasId = !string.IsNullOrWhiteSpace(ReadString(entry, "id"));
                bool hasName = !string.IsNullOrWhiteSpace(ReadString(entry, "name"));
                if (!isMinecraft || !hasId || !hasName)
                    continue;

                hasMinecraftProfile = true;
                break;
            }
        }

        if (!hasMinecraftProfile)
            return false;

        bool hasXuid = !string.IsNullOrWhiteSpace(ReadString(payload, "xuid"))
            || !string.IsNullOrWhiteSpace(ReadString(payload, "xid"));
        return hasXuid;
    }

    /// <summary>
    /// A string claim, or null when the property is absent or of another kind. A pasted
    /// token is untrusted input: <see cref="JsonElement.GetString"/> would throw
    /// <see cref="InvalidOperationException"/> for a numeric xuid or an object claim, which
    /// escapes the domain exception every caller catches.
    /// </summary>
    private static string? ReadString(JsonElement parent, string name)
        => parent.TryGetProperty(name, out JsonElement value) && value.ValueKind == JsonValueKind.String
            ? value.GetString()
            : null;

    private static string DecodeBase64Url(string value)
    {
        string padded = value.Replace('-', '+').Replace('_', '/');
        switch (padded.Length % 4)
        {
            case 2: padded += "=="; break;
            case 3: padded += "="; break;
        }

        return Encoding.UTF8.GetString(Convert.FromBase64String(padded));
    }

    private static ParsedCookies FromNetscape(string text)
    {
        var cookies = new Dictionary<string, CookieEntry>(StringComparer.Ordinal);
        foreach (string line in text.Split('\n'))
        {
            string candidate = line.Trim();
            if (string.IsNullOrEmpty(candidate) || IsCommentLine(candidate))
                continue;
            candidate = StripHttpOnlyPrefix(candidate);

            string[] parts = candidate.Split('\t');
            if (parts.Length < 6)
            {
                // Never quote the line: field 7 is a live cookie value that would end up
                // in the status bar and in any screenshot or bug report the user shares.
                throw new CookieAuthException(
                    $"Invalid cookie line (expected at least 6 tab-separated fields; got {parts.Length}).");
            }

            string domain = parts[0];
            string cookiePath = parts[2];
            bool secure = parts[3].Equals("TRUE", StringComparison.OrdinalIgnoreCase);
            string name = parts[5];
            string value = parts.Length == 6 ? "" : string.Join("\t", parts.Skip(6));
            cookies[domain + '\0' + name] = new CookieEntry(domain, cookiePath, secure, name, value);
        }

        return ValidateAuthCookies(cookies);
    }

    private static bool IsCommentLine(string line) =>
        line.StartsWith('#') && !line.StartsWith(HttpOnlyPrefix, StringComparison.OrdinalIgnoreCase);

    private static string StripHttpOnlyPrefix(string line) =>
        line.StartsWith(HttpOnlyPrefix, StringComparison.OrdinalIgnoreCase)
            ? line[HttpOnlyPrefix.Length..]
            : line;
}
