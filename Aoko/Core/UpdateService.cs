using System;
using System.Net.Http;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;

namespace Aoko.Core;

internal sealed record UpdateCheckResult(
    string CurrentVersion,
    string LatestVersion,
    string ReleaseUrl,
    bool IsUpdateAvailable);

internal static class UpdateService
{
    private const string LatestReleaseApiUrl = "https://api.github.com/repos/naczo5/aoko-client/releases/latest";
    internal const string ReleasesUrl = "https://github.com/naczo5/aoko-client/releases";

    private static readonly HttpClient HttpClient = CreateHttpClient();

    internal static string CurrentVersion =>
        typeof(UpdateService).Assembly.GetName().Version?.ToString(3) ?? "0.0.0";

    internal static async Task<UpdateCheckResult> CheckAsync(CancellationToken cancellationToken = default)
    {
        using HttpResponseMessage response = await HttpClient.GetAsync(LatestReleaseApiUrl, cancellationToken);
        response.EnsureSuccessStatusCode();

        await using var stream = await response.Content.ReadAsStreamAsync(cancellationToken);
        using JsonDocument document = await JsonDocument.ParseAsync(stream, cancellationToken: cancellationToken);

        JsonElement root = document.RootElement;
        string tag = root.GetProperty("tag_name").GetString()
            ?? throw new InvalidOperationException("The latest release has no tag.");
        string releaseUrl = root.TryGetProperty("html_url", out JsonElement urlElement)
            ? urlElement.GetString() ?? ReleasesUrl
            : ReleasesUrl;

        Version current = ParseReleaseVersion(CurrentVersion);
        Version latest = ParseReleaseVersion(tag);
        return new UpdateCheckResult(CurrentVersion, FormatVersion(latest), releaseUrl, latest > current);
    }

    internal static Version ParseReleaseVersion(string value)
    {
        string normalized = value.Trim().TrimStart('v', 'V');
        int suffixIndex = normalized.IndexOfAny(new[] { '-', '+' });
        if (suffixIndex >= 0)
            normalized = normalized[..suffixIndex];

        string[] parts = normalized.Split('.', StringSplitOptions.RemoveEmptyEntries);
        if (parts.Length is < 1 or > 4)
            throw new FormatException($"Unsupported release version: {value}");

        int[] numbers = new int[4];
        for (int i = 0; i < parts.Length; i++)
        {
            if (!int.TryParse(parts[i], out numbers[i]) || numbers[i] < 0)
                throw new FormatException($"Unsupported release version: {value}");
        }

        return new Version(numbers[0], numbers[1], numbers[2], numbers[3]);
    }

    internal static bool IsNewerRelease(string currentVersion, string latestTag) =>
        ParseReleaseVersion(latestTag) > ParseReleaseVersion(currentVersion);

    private static string FormatVersion(Version version) =>
        $"{version.Major}.{version.Minor}.{version.Build}";

    private static HttpClient CreateHttpClient()
    {
        var client = new HttpClient { Timeout = TimeSpan.FromSeconds(8) };
        client.DefaultRequestHeaders.UserAgent.ParseAdd($"aoko-client-update-checker/{CurrentVersion}");
        client.DefaultRequestHeaders.Accept.ParseAdd("application/vnd.github+json");
        return client;
    }
}
