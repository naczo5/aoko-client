using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Aoko.Core.Accounts;

internal sealed class LunarAccountWriteResult
{
    public LunarAccountWriteResult(bool success, string detail)
    {
        Success = success;
        Detail = detail;
    }

    public bool Success { get; }
    public string Detail { get; }
}

/// <summary>
/// Writes already-authenticated Microsoft/Minecraft profiles into Lunar Client's
/// local <c>accounts.json</c>. The schema follows Lunar's own launcher file:
/// undashed local ids, Xbox XUID remote ids, and MSA refresh tokens when we have
/// them. Lunar reads this file at startup, so a running client must be
/// relaunched. Offline accounts are refused: Lunar validates Microsoft
/// credentials and rejects them anyway.
/// </summary>
internal sealed class LunarAccountManager
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true
    };

    private readonly string _path;

    public LunarAccountManager(string? path = null)
    {
        _path = path ?? DefaultPath;
    }

    public static string DefaultPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
        ".lunarclient",
        "settings",
        "game",
        "accounts.json");

    public LunarAccountWriteResult Upsert(MinecraftProfile profile, bool setActive)
    {
        ArgumentNullException.ThrowIfNull(profile);
        return Upsert(profile.Name, profile.Uuid, profile.AccessToken, profile.RefreshToken, setActive, profile.Source);
    }

    public LunarAccountWriteResult Upsert(
        string name,
        string uuid,
        string accessToken,
        string? refreshToken,
        bool setActive,
        string? source = null)
    {
        if (ValidateAccount(source, name, uuid, accessToken) is { } rejection)
            return rejection;

        try
        {
            JsonObject root = LoadRoot();
            ApplyEntry(root, name, uuid, accessToken, refreshToken, setActive, source);
            SaveRoot(root);
            string refreshNote = string.IsNullOrWhiteSpace(refreshToken)
                ? "No MSA refresh token was available; Lunar may still reject this account."
                : "MSA refresh token included.";
            return new LunarAccountWriteResult(
                true,
                $"Wrote {name} to Lunar's accounts.json. {refreshNote} Fully close and relaunch Lunar to load it.");
        }
        catch (Exception ex)
        {
            // Best-effort interop with another launcher: report the failure type, never the
            // absolute path or parser offset that sits in the exception message.
            return new LunarAccountWriteResult(
                false,
                $"Could not update Lunar accounts.json ({ex.GetType().Name}).");
        }
    }

    public LunarAccountWriteResult UpsertAll(
        IEnumerable<(string Name, string Uuid, string AccessToken, string RefreshToken)> accounts,
        string? activeUuid)
    {
        ArgumentNullException.ThrowIfNull(accounts);

        int written = 0;
        int withRefresh = 0;
        try
        {
            // One read and one atomic write for the whole batch: rewriting Lunar's file per
            // account stalls the caller and leaves a half-mirrored file if one entry fails.
            JsonObject root = LoadRoot();
            foreach ((string name, string uuid, string accessToken, string refreshToken) in accounts)
            {
                LunarAccountWriteResult single = ApplyEntry(
                    root,
                    name,
                    uuid,
                    accessToken,
                    refreshToken,
                    setActive: SameUuid(uuid, activeUuid));
                if (!single.Success)
                    return single;

                written++;
                if (!string.IsNullOrWhiteSpace(refreshToken))
                    withRefresh++;
            }

            if (written == 0)
                return new LunarAccountWriteResult(true, "No accounts to write.");

            SaveRoot(root);
        }
        catch (Exception ex)
        {
            return new LunarAccountWriteResult(
                false,
                $"Could not update Lunar accounts.json ({ex.GetType().Name}).");
        }

        string refreshNote = withRefresh == written
            ? "MSA refresh tokens included."
            : $"{withRefresh}/{written} account(s) had an MSA refresh token.";
        return new LunarAccountWriteResult(
            true,
            $"Wrote {written} account(s) to Lunar's accounts.json. {refreshNote} Fully close and relaunch Lunar to load them.");
    }

    /// <summary>
    /// Writes one account into an already-loaded payload. Returns the outcome without saving,
    /// so a batch can mutate the document once.
    /// </summary>
    private static LunarAccountWriteResult ApplyEntry(
        JsonObject root,
        string name,
        string uuid,
        string accessToken,
        string? refreshToken,
        bool setActive,
        string? source = null)
    {
        if (ValidateAccount(source, name, uuid, accessToken) is { } rejection)
            return rejection;

        JsonObject accounts = GetOrCreateAccounts(root);
        string compact = CompactUuid(uuid);
        string key = FindExistingKey(accounts, uuid) ?? compact;
        JsonObject entry = accounts[key] as JsonObject ?? new JsonObject();
        string? xuid = TryReadJwtString(accessToken, "xuid");
        DateTimeOffset expiresAt = TryReadJwtExpiry(accessToken) ?? DateTimeOffset.UtcNow.AddHours(24);

        entry["accessToken"] = accessToken;
        entry["accessTokenExpiresAt"] = expiresAt.UtcDateTime.ToString(
            "yyyy-MM-dd'T'HH:mm:ss.fff'Z'",
            CultureInfo.InvariantCulture);
        entry["eligibleForMigration"] = false;
        entry["hasMultipleProfiles"] = false;
        if (!entry.ContainsKey("legacy"))
            entry["legacy"] = false;
        entry["persistent"] = true;
        entry["localId"] = key;
        if (!string.IsNullOrWhiteSpace(refreshToken))
            entry["refreshToken"] = refreshToken;
        entry["minecraftProfile"] = new JsonObject
        {
            ["id"] = compact,
            ["name"] = name
        };
        if (!string.IsNullOrWhiteSpace(xuid))
            entry["remoteId"] = xuid;
        else if (entry["remoteId"] is null)
            entry["remoteId"] = compact;
        entry["type"] = "Xbox";
        entry["username"] = name;

        accounts[key] = entry;
        if (setActive || root["activeAccountLocalId"] is null)
            root["activeAccountLocalId"] = key;

        return new LunarAccountWriteResult(true, string.Empty);
    }

    /// <summary>
    /// Rejects what Lunar cannot use. Returns null when the account is writable, so both the
    /// single-account and the batch path share one rule set.
    /// </summary>
    private static LunarAccountWriteResult? ValidateAccount(
        string? source,
        string name,
        string uuid,
        string accessToken)
    {
        if (string.Equals(source, AccountSources.Offline, StringComparison.OrdinalIgnoreCase))
        {
            return new LunarAccountWriteResult(
                false,
                "Lunar only accepts Microsoft accounts; cracked accounts are not written to its account manager.");
        }

        if (string.IsNullOrWhiteSpace(name) || string.IsNullOrWhiteSpace(uuid)
            || string.IsNullOrWhiteSpace(accessToken))
        {
            return new LunarAccountWriteResult(false, "Account is missing a name, UUID, or access token.");
        }

        return null;
    }

    internal static string? TryReadJwtString(string token, string claim)
    {
        JsonElement? payload = TryReadJwtPayload(token);
        if (payload is not { } root)
            return null;

        return root.TryGetProperty(claim, out JsonElement value)
            ? value.ValueKind switch
            {
                JsonValueKind.String => value.GetString(),
                JsonValueKind.Number => value.GetRawText(),
                _ => null
            }
            : null;
    }

    internal static DateTimeOffset? TryReadJwtExpiry(string token)
    {
        JsonElement? payload = TryReadJwtPayload(token);
        if (payload is not { } root || !root.TryGetProperty("exp", out JsonElement exp))
            return null;

        try
        {
            long seconds = exp.ValueKind == JsonValueKind.Number
                ? exp.GetInt64()
                : long.Parse(exp.GetString() ?? "");
            return DateTimeOffset.FromUnixTimeSeconds(seconds);
        }
        catch
        {
            return null;
        }
    }

    internal static JsonElement? TryReadJwtPayload(string token)
    {
        if (string.IsNullOrWhiteSpace(token))
            return null;

        string[] parts = token.Split('.');
        if (parts.Length < 2)
            return null;

        try
        {
            byte[] bytes = DecodeBase64Url(parts[1]);
            using JsonDocument document = JsonDocument.Parse(bytes);
            return document.RootElement.Clone();
        }
        catch
        {
            return null;
        }
    }

    internal static string CompactUuid(string uuid) =>
        uuid.Replace("-", "", StringComparison.Ordinal).Trim().ToLowerInvariant();

    private static byte[] DecodeBase64Url(string value)
    {
        string padded = value.Replace('-', '+').Replace('_', '/');
        switch (padded.Length % 4)
        {
            case 2: padded += "=="; break;
            case 3: padded += "="; break;
        }

        return Convert.FromBase64String(padded);
    }

    private JsonObject LoadRoot()
    {
        if (!File.Exists(_path))
            return new JsonObject { ["accounts"] = new JsonObject() };

        string json = File.ReadAllText(_path);
        return JsonNode.Parse(json) as JsonObject ?? new JsonObject { ["accounts"] = new JsonObject() };
    }

    private void SaveRoot(JsonObject root)
    {
        string? directory = Path.GetDirectoryName(_path);
        if (!string.IsNullOrEmpty(directory))
            Directory.CreateDirectory(directory);

        string json = root.ToJsonString(JsonOptions);
        string temp = _path + ".tmp";
        File.WriteAllText(temp, json);
        if (File.Exists(_path))
            File.Replace(temp, _path, destinationBackupFileName: null);
        else
            File.Move(temp, _path);
    }

    private static JsonObject GetOrCreateAccounts(JsonObject root)
    {
        if (root["accounts"] is JsonObject existing)
            return existing;

        var created = new JsonObject();
        root["accounts"] = created;
        return created;
    }

    private static string? FindExistingKey(JsonObject accounts, string uuid)
    {
        foreach ((string key, JsonNode? node) in accounts)
        {
            if (SameUuid(key, uuid))
                return key;
            if (node is not JsonObject entry)
                continue;

            string? localId = AsString(entry["localId"]);
            string? profileId = entry["minecraftProfile"] is JsonObject profile ? AsString(profile["id"]) : null;
            if (SameUuid(localId, uuid) || SameUuid(profileId, uuid))
                return key;
        }

        return null;
    }

    private static string? AsString(JsonNode? node)
    {
        try
        {
            return node?.GetValue<string>();
        }
        catch (InvalidOperationException)
        {
            return null;
        }
    }

    private static bool SameUuid(string? left, string? right) =>
        !string.IsNullOrWhiteSpace(left)
        && !string.IsNullOrWhiteSpace(right)
        && string.Equals(CompactUuid(left), CompactUuid(right), StringComparison.OrdinalIgnoreCase);
}
