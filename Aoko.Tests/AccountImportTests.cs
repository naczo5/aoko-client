using System.Net;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using Aoko.Core.Accounts;

namespace Aoko.Tests;

public class AccountImportTests
{
    // ----- credential parsing -------------------------------------------------

    [Fact]
    public void FromText_ReadsLabeledLocaltsToken()
    {
        ParsedCookies parsed = CookieParser.FromText("0123456789abcdef0123456789ab\nexample:M.Csynthetic-token");

        Assert.Equal("M.Csynthetic-token", parsed.RefreshToken);
        Assert.Empty(parsed.Cookies);
        Assert.Equal("", parsed.AccessToken);
    }

    [Fact]
    public void FromText_ReadsBareLocaltsToken()
    {
        Assert.Equal("M.Csynthetic-token", CookieParser.FromText("M.Csynthetic-token").RefreshToken);
    }

    [Fact]
    public void FromText_ReadsSevenFieldNetscapeExport()
    {
        ParsedCookies parsed = CookieParser.FromText(
            ".login.live.com\tTRUE\t/\tTRUE\t1999999999\tJSH\tvalue\n" +
            "login.live.com\tFALSE\t/\tTRUE\t1999999999\t__Host-MSAAUTHP\tauth-value\n");

        Assert.Equal(2, parsed.Cookies.Count);
        Assert.Contains("__Host-MSAAUTHP=auth-value", parsed.ToSisuCookieHeader());

        CookieEntry jsh = Assert.Single(parsed.Cookies.Values, c => c.Name == "JSH");
        Assert.Equal("value", jsh.Value);
        Assert.True(jsh.Secure);
        Assert.Equal("/", jsh.Path);
    }

    [Fact]
    public void FromText_StripsHttpOnlyPrefixFromNetscapeLines()
    {
        ParsedCookies parsed = CookieParser.FromText(
            "# Netscape HTTP Cookie File\n" +
            "#HttpOnly_login.live.com\tFALSE\t/\tTRUE\t1999999999\t__Host-MSAAUTHP\tauth-value\n");

        CookieEntry entry = Assert.Single(parsed.Cookies.Values);
        Assert.Equal("login.live.com", entry.Domain);
        Assert.Equal("__Host-MSAAUTHP", entry.Name);
    }

    [Fact]
    public void FromText_ReadsSpaceSeparatedNetscapeExport()
    {
        ParsedCookies parsed = CookieParser.FromText(
            "login.live.com FALSE / TRUE 1999999999 __Host-MSAAUTH auth-value");

        CookieEntry entry = Assert.Single(parsed.Cookies.Values);
        Assert.Equal("__Host-MSAAUTH", entry.Name);
        Assert.Equal("auth-value", entry.Value);
    }

    [Fact]
    public void FromText_ReadsSemicolonCookieHeader()
    {
        ParsedCookies parsed = CookieParser.FromText("__Host-MSAAUTH=one; __Host-MSAAUTHP=two; other=three");

        Assert.Equal(3, parsed.Cookies.Count);
        Assert.All(parsed.Cookies.Values, c =>
        {
            Assert.Equal("login.live.com", c.Domain);
            Assert.Equal("/", c.Path);
            Assert.True(c.Secure);
        });
    }

    [Fact]
    public void FromText_RejectsCookieHeaderWithoutAuthenticationCookie()
    {
        CookieAuthException error = Assert.Throws<CookieAuthException>(
            () => CookieParser.FromText("ordinary=value; second=value"));

        Assert.Equal("Cookie file is missing __Host-MSAAUTHP or __Host-MSAAUTH.", error.Message);
        Assert.False(error.IsTransient);
        Assert.Null(error.HttpStatusCode);
        Assert.Null(error.RetryAfter);
    }

    [Fact]
    public void FromText_RejectsUnrecognizedInput()
    {
        CookieAuthException error = Assert.Throws<CookieAuthException>(
            () => CookieParser.FromText("not a supported account format"));

        Assert.StartsWith("Unrecognized cookie format.", error.Message);
    }

    [Fact]
    public void FromText_ReadsBareMinecraftAccessToken()
    {
        string jwt = FakeMcaJwt("2533274909086470", "c812a564-a9da-437f-a33c-a32760e3156f", "Sv2Void7vh");

        ParsedCookies parsed = CookieParser.FromText(jwt);

        Assert.Equal(jwt, parsed.AccessToken);
        Assert.Equal("", parsed.RefreshToken);
        Assert.Empty(parsed.Cookies);
    }

    [Fact]
    public void FromText_ReadsMinecraftAccessTokenWithBearerPrefixAndJsonWrapper()
    {
        string jwt = FakeMcaJwt("2533274909086470", "c812a564-a9da-437f-a33c-a32760e3156f", "Sv2Void7vh");

        Assert.Equal(jwt, CookieParser.FromText("Bearer " + jwt).AccessToken);
        Assert.Equal(jwt, CookieParser.FromText("{\"mcToken\":\"" + jwt + "\"}").AccessToken);
    }

    [Fact]
    public void FromText_RejectsJwtWithoutMinecraftProfile()
    {
        string jwt = FakeJwt(("xuid", "2535443995591896"), ("exp", "1786969129"));

        Assert.Throws<CookieAuthException>(() => CookieParser.FromText(jwt));
    }

    // ----- offline accounts ---------------------------------------------------

    [Theory]
    [InlineData("Notch", "b50ad385-829d-3141-a216-7e7d7539ba7f")]
    [InlineData("Player_1", "914e9094-dd43-376a-a052-b63bf94389aa")]
    public void ComputeOfflineUuid_MatchesNameUuidFromBytes(string name, string expected)
    {
        Assert.Equal(expected, OfflineAccount.ComputeOfflineUuid(name));
    }

    [Fact]
    public void TryCreate_TrimsNameAndReturnsUndashedUuid()
    {
        Assert.True(OfflineAccount.TryCreate(" Notch ", out string name, out string uuid, out string error));

        Assert.Equal("", error);
        Assert.Equal("Notch", name);
        Assert.Equal("b50ad385829d3141a2167e7d7539ba7f", uuid);
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("  ")]
    [InlineData("ab")]
    [InlineData("thisusernameiswaytoolong")]
    [InlineData("bad name")]
    [InlineData("bad-name")]
    [InlineData("pl@yer")]
    public void TryCreate_RejectsInvalidNames(string? candidate)
    {
        Assert.False(OfflineAccount.TryCreate(candidate, out _, out _, out string error));
        Assert.Equal("Usernames must be 3-16 characters using only letters, numbers, and underscores.", error);
    }

    // ----- persistence --------------------------------------------------------

    [Fact]
    public void AccountStore_RoundTripsCamelCaseJson()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        var store = new AccountStore(path);
        store.Upsert(new MinecraftProfile("Notch", "b50ad385829d3141a2167e7d7539ba7f", "0", null, AccountSources.Offline));
        store.Upsert(new MinecraftProfile("Sv2Void7vh", "c812a564-a9da-437f-a33c-a32760e3156f",
            FakeMcaJwt("1", "c812a564-a9da-437f-a33c-a32760e3156f", "Sv2Void7vh"), "M.Crefresh", AccountSources.Localts));

        string json = File.ReadAllText(path);
        Assert.Contains("\"accessToken\"", json);
        Assert.StartsWith("[", json, StringComparison.Ordinal);
        // Indented, one object per line: the file stays readable when a user opens it by hand.
        Assert.Contains("\n  {", json, StringComparison.Ordinal);
        Assert.Contains("\n    \"id\":", json, StringComparison.Ordinal);
        Assert.DoesNotContain("displayLabel", json, StringComparison.OrdinalIgnoreCase);
        Assert.False(File.Exists(path + ".tmp"));

        var reloaded = new AccountStore(path);
        Assert.Equal(2, reloaded.Accounts.Count);
        StoredAccount localts = Assert.Single(reloaded.Accounts, a => a.Source == AccountSources.Localts);
        Assert.True(localts.HasRefresh);
        Assert.Equal("M.Crefresh", localts.RefreshToken);
        Assert.Equal($"{localts.Name}  (Localts)", localts.DisplayLabel);
        Assert.True(Assert.Single(reloaded.Accounts, a => a.Source == AccountSources.Offline).IsOffline);
    }

    [Fact]
    public void AccountStore_RoundTripsValuesThatContainNewlines()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        var store = new AccountStore(path);
        store.Upsert(new MinecraftProfile(
            "Multi\nLine",
            "c812a564-a9da-437f-a33c-a32760e3156f",
            "token\nwith\nnewlines",
            null,
            AccountSources.Token));

        // A raw newline in a token must be escaped, not written into the file as a line break.
        string json = File.ReadAllText(path);
        Assert.DoesNotContain("token\nwith", json, StringComparison.Ordinal);

        var reloaded = new AccountStore(path);
        StoredAccount account = Assert.Single(reloaded.Accounts);
        Assert.Equal("Multi\nLine", account.Name);
        Assert.Equal("token\nwith\nnewlines", account.AccessToken);
        Assert.Equal("Multi\nLine  (token)", account.DisplayLabel);
    }

    [Fact]
    public void StoredAccount_UnknownSourceShowsTheSourceWord()
    {
        var account = new StoredAccount { Name = "Mystery", Source = AccountSources.Unknown };

        // The default source must not claim a cookie import that never happened.
        Assert.Equal("Mystery  (unknown)", account.DisplayLabel);
    }

    [Fact]
    public void AccountStore_QuarantinesCorruptFileInsteadOfDiscardingIt()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        File.WriteAllText(path, "{ this is not json");

        var store = new AccountStore(path);

        Assert.Empty(store.Accounts);
        Assert.False(File.Exists(path));
        string quarantined = Assert.Single(Directory.GetFiles(temp.Path, "accounts.json.corrupt-*"));
        Assert.Equal("{ this is not json", File.ReadAllText(quarantined));

        store.Save();
        Assert.True(File.Exists(path));
    }

    [Fact]
    public void Upsert_MatchesDashedAndUndashedUuids()
    {
        using var temp = new TempDirectory();
        var store = new AccountStore(Path.Combine(temp.Path, "accounts.json"));

        StoredAccount first = store.Upsert(new MinecraftProfile("Player", "b50ad385-829d-3141-a216-7e7d7539ba7f",
            "token-1", "M.Ckeep", AccountSources.Localts));
        StoredAccount second = store.Upsert(new MinecraftProfile("Player", "b50ad385829d3141a2167e7d7539ba7f",
            "token-2", null, AccountSources.Token));

        Assert.Same(first, second);
        Assert.Single(store.Accounts);
        Assert.Equal("token-2", second.AccessToken);
    }

    [Fact]
    public void Upsert_KeepsCrackedAccountBesidePremiumAccountOfSameName()
    {
        using var temp = new TempDirectory();
        var store = new AccountStore(Path.Combine(temp.Path, "accounts.json"));
        store.Upsert(new MinecraftProfile("Notch", "069a79f444e94726a5befca90e38aaf5", "token", "M.Crefresh",
            AccountSources.Localts));
        store.Upsert(new MinecraftProfile("Notch", OfflineAccount.ComputeOfflineUuid("Notch").Replace("-", ""), "0",
            null, AccountSources.Offline));

        Assert.Equal(2, store.Accounts.Count);
        StoredAccount cracked = Assert.Single(store.Accounts, a => a.IsOffline);
        Assert.Equal(OfflineAccount.OfflineToken, cracked.AccessToken);
    }

    [Fact]
    public void Upsert_DoesNotDowngradeRefreshBackedSource()
    {
        using var temp = new TempDirectory();
        var store = new AccountStore(Path.Combine(temp.Path, "accounts.json"));
        store.Upsert(new MinecraftProfile("Player", "c812a564a9da437fa33ca32760e3156f", "token-1", "M.Crefresh",
            AccountSources.Localts));

        StoredAccount downgraded = store.Upsert(new MinecraftProfile("Player", "c812a564a9da437fa33ca32760e3156f",
            "token-2", null, AccountSources.Token));

        Assert.Equal(AccountSources.Localts, downgraded.Source);
        Assert.Equal("M.Crefresh", downgraded.RefreshToken);
    }

    [Fact]
    public void UpdateTokens_StampsLastUsedAt()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "accounts.json");
        var store = new AccountStore(path);
        StoredAccount account = store.Upsert(new MinecraftProfile("Player", "uuid-1", "token-1", "M.C",
            AccountSources.Localts));
        Assert.Null(account.LastUsedAt);

        store.UpdateTokens(account, new MinecraftProfile("Player2", "uuid-2", "token-2", "M.C2"));

        Assert.Equal("Player2", account.Name);
        Assert.Equal("uuid-2", account.Uuid);
        Assert.Equal("token-2", account.AccessToken);
        Assert.Equal("M.C2", account.RefreshToken);
        Assert.NotNull(account.LastUsedAt);
        Assert.NotNull(new AccountStore(path).Accounts.Single().LastUsedAt);
    }

    [Fact]
    public void Remove_ReportsWhetherItRemoved()
    {
        using var temp = new TempDirectory();
        var store = new AccountStore(Path.Combine(temp.Path, "accounts.json"));
        StoredAccount account = store.Upsert(new MinecraftProfile("Player", "uuid", "token", null, AccountSources.Token));

        Assert.True(store.Remove(account.Id));
        Assert.False(store.Remove(account.Id));
        Assert.Empty(store.Accounts);
    }

    // ----- Lunar interop ------------------------------------------------------

    [Fact]
    public void LunarUpsert_PreservesUnknownKeysAndSetsLunarFields()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "lunar-accounts.json");
        File.WriteAllText(path, "{\"accounts\":{\"keep-me\":{\"username\":\"Other\"}},\"mojangClientToken\":\"keep\"}");

        var manager = new LunarAccountManager(path);
        string token = FakeJwt(("xuid", "2535443995591896"), ("exp", "1786969129"));
        LunarAccountWriteResult result = manager.Upsert(
            "Player", "12345678123456781234567812345678", token, "M.Crefresh", setActive: true);

        Assert.True(result.Success, result.Detail);
        Assert.Contains("Fully close and relaunch Lunar", result.Detail);

        using JsonDocument document = JsonDocument.Parse(File.ReadAllText(path));
        Assert.Equal("keep", document.RootElement.GetProperty("mojangClientToken").GetString());
        Assert.True(document.RootElement.GetProperty("accounts").TryGetProperty("keep-me", out _));

        JsonElement written = document.RootElement.GetProperty("accounts")
            .GetProperty("12345678123456781234567812345678");
        Assert.Equal(token, written.GetProperty("accessToken").GetString());
        Assert.Equal("M.Crefresh", written.GetProperty("refreshToken").GetString());
        Assert.Equal("2535443995591896", written.GetProperty("remoteId").GetString());
        Assert.Equal("12345678123456781234567812345678", written.GetProperty("localId").GetString());
        Assert.Equal("Player", written.GetProperty("minecraftProfile").GetProperty("name").GetString());
        Assert.Equal("12345678123456781234567812345678",
            document.RootElement.GetProperty("activeAccountLocalId").GetString());
        Assert.False(File.Exists(path + ".tmp"));
    }

    [Fact]
    public void LunarUpsert_ReportsTheFailureTypeWithoutTheFilesystemPath()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "lunar-accounts.json");
        Directory.CreateDirectory(path);

        var manager = new LunarAccountManager(path);
        LunarAccountWriteResult result = manager.Upsert(
            "Player",
            "12345678123456781234567812345678",
            FakeJwt(("xuid", "2535443995591896")),
            "M.Crefresh",
            setActive: true);

        Assert.False(result.Success);
        Assert.Contains("Could not update Lunar accounts.json (", result.Detail, StringComparison.Ordinal);
        // The exception message embeds the absolute settings path; that must not reach the UI.
        Assert.DoesNotContain(temp.Path, result.Detail, StringComparison.Ordinal);
        Assert.DoesNotContain(path, result.Detail, StringComparison.Ordinal);
    }

    [Fact]
    public void LunarUpsertAll_WritesTheBatchWithOnePendingActiveAccount()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "lunar-accounts.json");
        var manager = new LunarAccountManager(path);
        string activeUuid = "069a79f444e94726a5befca90e38aaf5";
        string otherUuid = "12345678123456781234567812345678";

        LunarAccountWriteResult result = manager.UpsertAll(
            new[]
            {
                ("Other", otherUuid, FakeJwt(("xuid", "1")), "M.Cother"),
                ("Active", activeUuid, FakeJwt(("xuid", "2")), "M.Cactive")
            },
            activeUuid);

        Assert.True(result.Success, result.Detail);
        using JsonDocument document = JsonDocument.Parse(File.ReadAllText(path));
        Assert.Equal(2, document.RootElement.GetProperty("accounts").EnumerateObject().Count());
        Assert.Equal(
            activeUuid,
            document.RootElement.GetProperty("activeAccountLocalId").GetString());
        Assert.False(File.Exists(path + ".tmp"));
    }

    [Fact]
    public void LunarUpsert_RefusesOfflineAccounts()
    {
        using var temp = new TempDirectory();
        string path = Path.Combine(temp.Path, "lunar-accounts.json");
        var manager = new LunarAccountManager(path);

        LunarAccountWriteResult result = manager.Upsert(
            new MinecraftProfile("Notch", "b50ad385829d3141a2167e7d7539ba7f", "0", null, AccountSources.Offline),
            setActive: true);

        Assert.False(result.Success);
        Assert.False(File.Exists(path));
    }

    [Fact]
    public void LunarJwtHelpers_ReadClaimsAndCompactUuids()
    {
        string token = FakeJwt(("xuid", "2535443995591896"), ("exp", "1786969129"));

        Assert.Equal("2535443995591896", LunarAccountManager.TryReadJwtString(token, "xuid"));
        Assert.Equal(DateTimeOffset.FromUnixTimeSeconds(1786969129), LunarAccountManager.TryReadJwtExpiry(token));
        Assert.Null(LunarAccountManager.TryReadJwtPayload("not-a-jwt"));
        Assert.Equal("b50ad385829d3141a2167e7d7539ba7f",
            LunarAccountManager.CompactUuid("B50AD385-829D-3141-A216-7E7D7539BA7F"));
    }

    // ----- import orchestration ----------------------------------------------

    [Fact]
    public async Task ImportTextAsync_SavesLocaltsAccountWithoutLoggingTokens()
    {
        using var temp = new TempDirectory();
        var store = new AccountStore(Path.Combine(temp.Path, "accounts.json"));
        var logs = new List<string>();
        var service = new AccountImportService(store, authenticate: FakeAuthenticate("Sv2Void7vh"))
        {
            Log = logs.Add
        };

        AccountImportResult result = await service.ImportTextAsync("user:M.Csynthetic-token");

        Assert.True(result.Ok, result.Detail);
        Assert.Equal(AccountSources.Localts, result.Source);
        Assert.Equal("Sv2Void7vh", result.Account?.Name);
        Assert.Equal("Imported Sv2Void7vh (Localts token).", result.Detail);
        Assert.True(store.Accounts.Single().HasRefresh);
        Assert.Contains(logs, line => line.Contains("saved account Sv2Void7vh (localts).", StringComparison.Ordinal));
        Assert.DoesNotContain(logs, line => line.Contains("M.Csynthetic-token", StringComparison.Ordinal));
    }

    [Fact]
    public async Task ImportTextAsync_RejectsEmptyInput()
    {
        using var temp = new TempDirectory();
        var service = new AccountImportService(new AccountStore(Path.Combine(temp.Path, "accounts.json")));

        AccountImportResult result = await service.ImportTextAsync("   ");

        Assert.False(result.Ok);
        Assert.Equal("The file or pasted text is empty.", result.Detail);
    }

    [Fact]
    public async Task ImportTextAsync_RetriesTransientFailuresAndHonoursRetryAfter()
    {
        using var temp = new TempDirectory();
        var store = new AccountStore(Path.Combine(temp.Path, "accounts.json"));
        var delays = new List<TimeSpan>();
        int calls = 0;

        var service = new AccountImportService(store, authenticate: (_, _) =>
        {
            calls++;
            return calls switch
            {
                1 => Task.FromException<MinecraftProfile>(new CookieAuthException(
                    "Xbox Live authentication failed (HTTP 503 ServiceUnavailable).",
                    HttpStatusCode.ServiceUnavailable,
                    isTransient: true)),
                2 => Task.FromException<MinecraftProfile>(new CookieAuthException(
                    "Microsoft token exchange throttled the request (HTTP 429).",
                    HttpStatusCode.TooManyRequests,
                    isTransient: true,
                    retryAfter: TimeSpan.FromSeconds(7))),
                _ => Task.FromResult(new MinecraftProfile("Player", "uuid", "token", "M.C", AccountSources.Localts))
            };
        })
        {
            Log = _ => { },
            DelayAsync = (delay, _) =>
            {
                delays.Add(delay);
                return Task.CompletedTask;
            }
        };

        AccountImportResult result = await service.ImportTextAsync("M.Csynthetic-token");

        Assert.True(result.Ok, result.Detail);
        Assert.Equal(3, calls);
        Assert.Equal(new[] { TimeSpan.FromSeconds(5), TimeSpan.FromSeconds(7) }, delays);
    }

    [Fact]
    public async Task ImportTextAsync_GivesUpAfterThreeAttempts()
    {
        using var temp = new TempDirectory();
        var delays = new List<TimeSpan>();
        int calls = 0;
        var service = new AccountImportService(
            new AccountStore(Path.Combine(temp.Path, "accounts.json")),
            authenticate: (_, _) =>
            {
                calls++;
                return Task.FromException<MinecraftProfile>(new CookieAuthException(
                    "Xbox Live authentication failed (HTTP 500 InternalServerError).",
                    HttpStatusCode.InternalServerError,
                    isTransient: true));
            })
        {
            Log = _ => { },
            DelayAsync = (delay, _) =>
            {
                delays.Add(delay);
                return Task.CompletedTask;
            }
        };

        AccountImportResult result = await service.ImportTextAsync("M.Ctoken");

        Assert.False(result.Ok);
        Assert.Equal(3, calls);
        Assert.Equal(new[] { TimeSpan.FromSeconds(5), TimeSpan.FromSeconds(10) }, delays);
    }

    [Fact]
    public async Task ImportTextAsync_LeavesTheLunarActiveAccountAlone()
    {
        using var temp = new TempDirectory();
        var store = new AccountStore(Path.Combine(temp.Path, "accounts.json"));
        string lunarPath = Path.Combine(temp.Path, "lunar-accounts.json");
        const string playingUuid = "069a79f444e94726a5befca90e38aaf5";
        File.WriteAllText(
            lunarPath,
            "{\"accounts\":{\"" + playingUuid + "\":{\"username\":\"Playing\"}}," +
            "\"activeAccountLocalId\":\"" + playingUuid + "\"}");

        var service = new AccountImportService(
            store,
            new LunarAccountManager(lunarPath),
            authenticate: FakeAuthenticate("Sv2Void7vh"))
        {
            Log = _ => { }
        };

        AccountImportResult result = await service.ImportTextAsync("user:M.Csynthetic-token");

        Assert.True(result.Ok, result.Detail);
        using JsonDocument document = JsonDocument.Parse(File.ReadAllText(lunarPath));
        // Importing must not decide which account Lunar starts with: only a switch does.
        Assert.Equal(playingUuid, document.RootElement.GetProperty("activeAccountLocalId").GetString());
        Assert.Equal(2, document.RootElement.GetProperty("accounts").EnumerateObject().Count());
    }

    [Fact]
    public async Task ImportFilesAsync_AggregatesFailuresByFileName()
    {
        using var temp = new TempDirectory();
        var store = new AccountStore(Path.Combine(temp.Path, "accounts.json"));
        string good = Path.Combine(temp.Path, "good.txt");
        string bad = Path.Combine(temp.Path, "bad.txt");
        string missing = Path.Combine(temp.Path, "missing.txt");
        File.WriteAllText(good, "user:M.Cfile-token");
        File.WriteAllText(bad, "not a supported account format");

        var service = new AccountImportService(store, authenticate: FakeAuthenticate("Player"))
        {
            Log = _ => { }
        };

        AccountBatchImportResult result = await service.ImportFilesAsync(
            new[] { good, bad, missing },
            spacingDelay: TimeSpan.Zero);

        Assert.Equal(3, result.Total);
        Assert.Equal(1, result.Imported);
        Assert.False(result.AllSucceeded);
        Assert.Contains(result.Failures, f => f.FileName == "bad.txt");
        Assert.Contains(result.Failures, f => f.FileName == "missing.txt");
        Assert.StartsWith("Imported 1 of 3 account files.", result.Summary);
        Assert.Contains("bad.txt: ", result.Summary);
    }

    [Theory]
    [InlineData(1, null, 5)]
    [InlineData(2, null, 10)]
    [InlineData(10, null, 120)]
    [InlineData(1, 0, 1)]
    [InlineData(1, 7, 7)]
    public void RetryDelay_UsesBackoffClampedToTwoMinutes(int failedAttempt, int? serverSeconds, double expected)
    {
        TimeSpan? serverDelay = serverSeconds is { } seconds ? TimeSpan.FromSeconds(seconds) : null;

        Assert.Equal(TimeSpan.FromSeconds(expected), AccountImportService.RetryDelay(serverDelay, failedAttempt));
    }

    [Fact]
    public void FormatDelay_RendersSecondsUnderAMinute()
    {
        Assert.Equal("5s", AccountImportService.FormatDelay(TimeSpan.FromSeconds(5)));
        Assert.Equal("2m", AccountImportService.FormatDelay(TimeSpan.FromSeconds(75)));
    }

    [Fact]
    public void FailureFromResponse_ClassifiesTransientStatusesAndRetryAfter()
    {
        using var throttled = new HttpResponseMessage(HttpStatusCode.TooManyRequests);
        throttled.Headers.RetryAfter = new System.Net.Http.Headers.RetryConditionHeaderValue(TimeSpan.FromSeconds(9));

        CookieAuthException transient = CookieAuth.FailureFromResponse("Microsoft token exchange", throttled);

        Assert.True(transient.IsTransient);
        Assert.Equal(HttpStatusCode.TooManyRequests, transient.HttpStatusCode);
        Assert.Equal(TimeSpan.FromSeconds(9), transient.RetryAfter);
        Assert.Equal("Microsoft token exchange throttled the request (HTTP 429).", transient.Message);

        using var badRequest = new HttpResponseMessage(HttpStatusCode.BadRequest);
        CookieAuthException permanent = CookieAuth.FailureFromResponse("Xbox Live authentication", badRequest);

        Assert.False(permanent.IsTransient);
        Assert.Equal("Xbox Live authentication failed (HTTP 400 BadRequest).", permanent.Message);

        using var unavailable = new HttpResponseMessage(HttpStatusCode.ServiceUnavailable);
        Assert.True(CookieAuth.FailureFromResponse("Minecraft profile lookup", unavailable).IsTransient);
    }

    [Fact]
    public void ThrowIfMcaExpired_RejectsStaleTokensWithOnlyTheExpiryDate()
    {
        string expired = FakeJwt(("xuid", "2535443995591896"), ("exp", "1000000000"));

        CookieAuthException error = Assert.Throws<CookieAuthException>(
            () => CookieAuth.ThrowIfMcaExpired(expired));

        Assert.StartsWith("Minecraft access token expired on ", error.Message);
        Assert.DoesNotContain(expired, error.Message);
    }

    // ----- helpers ------------------------------------------------------------

    private static Func<ParsedCookies, CancellationToken, Task<MinecraftProfile>> FakeAuthenticate(string name) =>
        (parsed, _) => Task.FromResult(new MinecraftProfile(
            name,
            "c812a564-a9da-437f-a33c-a32760e3156f",
            FakeMcaJwt("1", "c812a564-a9da-437f-a33c-a32760e3156f", name),
            string.IsNullOrWhiteSpace(parsed.RefreshToken) ? null : parsed.RefreshToken,
            string.IsNullOrWhiteSpace(parsed.RefreshToken) ? AccountSources.Token : AccountSources.Localts));

    private static string FakeJwt(params (string Name, string Value)[] claims)
    {
        var payload = new StringBuilder("{");
        for (int index = 0; index < claims.Length; index++)
        {
            if (index > 0)
                payload.Append(',');
            payload.Append('"').Append(claims[index].Name).Append("\":");
            if (claims[index].Name == "exp" && long.TryParse(claims[index].Value, out _))
                payload.Append(claims[index].Value);
            else
                payload.Append('"').Append(claims[index].Value).Append('"');
        }

        payload.Append('}');
        return "eyJhbGciOiJub25lIn0." + Base64Url(payload.ToString()) + ".sig";
    }

    private static string FakeMcaJwt(string xuid, string uuid, string name)
    {
        string header = Base64Url("{\"kid\":\"049181\",\"alg\":\"RS256\"}");
        string payload = Base64Url(
            "{\"xuid\":\"" + xuid + "\",\"agg\":\"Adult\",\"sub\":\"da307ebc-556f-4b3a-b1ca-864ffdede4b6\"," +
            "\"auth\":\"XBOX\",\"ns\":\"default\",\"roles\":[],\"iss\":\"authentication\"," +
            "\"flags\":[\"multiplayer\"],\"profiles\":{\"mc\":\"" + uuid + "\"}," +
            "\"platform\":\"PC_LAUNCHER\",\"tid\":\"E99B0\"," +
            "\"pfd\":[{\"type\":\"mc\",\"id\":\"" + uuid + "\",\"name\":\"" + name + "\"}]," +
            "\"xid\":\"" + xuid + "\",\"nbf\":1790831105,\"exp\":1790917505,\"iat\":1790831105," +
            "\"aid\":\"00000000-0000-0000-0000-0000402b5328\"}");
        return "eyJ" + header.Substring(3) + "." + payload + ".sig";
    }

    private static string Base64Url(string value)
    {
        string encoded = Convert.ToBase64String(Encoding.UTF8.GetBytes(value));
        return encoded.TrimEnd('=').Replace('+', '-').Replace('/', '_');
    }

    private sealed class TempDirectory : IDisposable
    {
        public TempDirectory()
        {
            Path = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "aoko-accounts-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(Path);
        }

        public string Path { get; }

        public void Dispose()
        {
            try
            {
                Directory.Delete(Path, recursive: true);
            }
            catch (IOException)
            {
                // Test cleanup only.
            }
            catch (UnauthorizedAccessException)
            {
                // Test cleanup only.
            }
        }
    }
}
