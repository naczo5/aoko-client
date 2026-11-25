using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace Aoko.Core.Accounts;

internal sealed record AccountImportResult(bool Ok, StoredAccount? Account, string Source, string Detail)
{
    public static AccountImportResult Failure(string detail, string source = "") =>
        new(false, null, source, detail);

    public static AccountImportResult Success(StoredAccount account, string source, string detail) =>
        new(true, account, source, detail);
}

internal sealed record AccountImportFailure(string FileName, string Detail);

internal sealed record AccountImportProgress(int Index, int Total, string FileName);

internal sealed record AccountBatchImportResult(int Imported, int Total, IReadOnlyList<AccountImportFailure> Failures)
{
    public bool AllSucceeded => Failures.Count == 0;

    /// <summary>One-line outcome plus one line per failed file, for a status bar or log.</summary>
    public string Summary
    {
        get
        {
            var builder = new StringBuilder();
            builder.Append($"Imported {Imported} of {Total} account files.");
            foreach (AccountImportFailure failure in Failures)
                builder.Append(Environment.NewLine).Append(failure.FileName).Append(": ").Append(failure.Detail);
            return builder.ToString();
        }
    }
}

/// <summary>
/// Turns imported text or files into saved accounts: parse, authenticate with
/// retries, persist, and optionally mirror into Lunar's account manager. Expected
/// failures come back as results; tokens never reach logs or messages.
/// </summary>
internal sealed class AccountImportService
{
    private const int MaxAttempts = 3;
    private static readonly TimeSpan DefaultBatchSpacing = TimeSpan.FromSeconds(2);

    private readonly AccountStore _store;
    private readonly LunarAccountManager? _lunar;
    private readonly TimeSpan _batchSpacing;
    private readonly Func<ParsedCookies, CancellationToken, Task<MinecraftProfile>> _authenticate;

    public AccountImportService(
        AccountStore store,
        LunarAccountManager? lunar = null,
        TimeSpan? batchSpacing = null,
        Func<ParsedCookies, CancellationToken, Task<MinecraftProfile>>? authenticate = null)
    {
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _lunar = lunar;
        _batchSpacing = batchSpacing ?? DefaultBatchSpacing;
        _authenticate = authenticate ?? CookieAuth.AuthenticateAsync;
    }

    internal Action<string> Log { get; set; } = static message => Debug.WriteLine(message);

    /// <summary>Delay hook so tests can assert the backoff without waiting.</summary>
    internal Func<TimeSpan, CancellationToken, Task> DelayAsync { get; set; } =
        static (delay, cancellationToken) => Task.Delay(delay, cancellationToken);

    public Task<AccountImportResult> ImportTextAsync(string? text, CancellationToken cancellationToken = default) =>
        ImportTextAsync(text, "Account import", cancellationToken);

    public async Task<AccountImportResult> ImportFileAsync(string? path, CancellationToken cancellationToken = default)
    {
        string fileName = SafeFileName(path);
        if (string.IsNullOrWhiteSpace(path))
            return AccountImportResult.Failure("The file could not be read.");

        string text;
        try
        {
            text = await File.ReadAllTextAsync(path, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                      or NotSupportedException or ArgumentException)
        {
            const string detail = "The file could not be read.";
            Log($"{fileName}: {detail} ({ex.GetType().Name})");
            return AccountImportResult.Failure(detail);
        }

        return await ImportTextAsync(text, fileName, cancellationToken).ConfigureAwait(false);
    }

    public async Task<AccountBatchImportResult> ImportFilesAsync(
        IReadOnlyList<string>? paths,
        TimeSpan? spacingDelay = null,
        IProgress<AccountImportProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        if (paths == null || paths.Count == 0)
            return new AccountBatchImportResult(0, 0, Array.Empty<AccountImportFailure>());

        TimeSpan spacing = spacingDelay ?? _batchSpacing;
        var failures = new List<AccountImportFailure>();
        int imported = 0;

        for (int index = 0; index < paths.Count; index++)
        {
            string fileName = SafeFileName(paths[index]);
            progress?.Report(new AccountImportProgress(index + 1, paths.Count, fileName));

            AccountImportResult result = await ImportFileAsync(paths[index], cancellationToken).ConfigureAwait(false);
            if (result.Ok)
                imported++;
            else
                failures.Add(new AccountImportFailure(fileName, result.Detail));

            // Space the complete Microsoft/Xbox/Minecraft chains rather than
            // firing a burst at the account services.
            if (index < paths.Count - 1 && spacing > TimeSpan.Zero)
                await DelayAsync(spacing, cancellationToken).ConfigureAwait(false);
        }

        return new AccountBatchImportResult(imported, paths.Count, failures);
    }

    private async Task<AccountImportResult> ImportTextAsync(
        string? text,
        string label,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(text))
            return AccountImportResult.Failure("The file or pasted text is empty.");

        ParsedCookies parsed;
        try
        {
            parsed = CookieParser.FromText(text);
        }
        catch (CookieAuthException ex)
        {
            // Parser messages carry field counts, never a cookie value or token.
            Log($"{label}: parsing failed ({ex.Message}).");
            return AccountImportResult.Failure(ex.Message);
        }

        string source = !string.IsNullOrWhiteSpace(parsed.RefreshToken) ? AccountSources.Localts
            : !string.IsNullOrWhiteSpace(parsed.AccessToken) ? AccountSources.Token
            : AccountSources.Cookie;
        Log($"{label}: parsed {source} data; authenticating.");

        MinecraftProfile profile;
        try
        {
            profile = await AuthenticateWithRetryAsync(parsed, label, cancellationToken).ConfigureAwait(false);
        }
        catch (CookieAuthException ex)
        {
            Log($"{label}: authentication failed - {ex.Message}");
            return AccountImportResult.Failure(ex.Message, source);
        }
        catch (HttpRequestException ex)
        {
            string status = ex.StatusCode is { } code ? $" (HTTP {(int)code})" : "";
            string detail = $"Network authentication failed{status}.";
            Log($"{label}: {detail}");
            return AccountImportResult.Failure(detail, source);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (OperationCanceledException)
        {
            const string detail = "Authentication timed out after retries.";
            Log($"{label}: {detail}");
            return AccountImportResult.Failure(detail, source);
        }

        StoredAccount stored;
        try
        {
            stored = _store.Upsert(profile, source);
        }
        catch (Exception ex)
        {
            // The account authenticated but the payload could not be written: serializer
            // failures (NotSupportedException/JsonException) belong to this class too, not
            // to the caller as an unexpected exception.
            const string detail = "The account authenticated but could not be saved.";
            Log($"{label}: {profile.Name} ({source}) could not be saved ({ex.GetType().Name}).");
            return AccountImportResult.Failure(detail, source);
        }

        WriteLunarAccount(profile);
        string success = $"Imported {profile.Name} ({AccountSources.Display(source)}).";
        Log($"{label}: saved account {profile.Name} ({source}).");
        return AccountImportResult.Success(stored, source, success);
    }

    private async Task<MinecraftProfile> AuthenticateWithRetryAsync(
        ParsedCookies parsed,
        string label,
        CancellationToken cancellationToken)
    {
        for (int attempt = 1; ; attempt++)
        {
            try
            {
                return await _authenticate(parsed, cancellationToken).ConfigureAwait(false);
            }
            catch (CookieAuthException ex) when (ex.IsTransient && attempt < MaxAttempts)
            {
                TimeSpan delay = RetryDelay(ex.RetryAfter, attempt);
                Log($"{label}: {ex.Message} Retrying in {FormatDelay(delay)} ({attempt + 1}/{MaxAttempts}).");
                await DelayAsync(delay, cancellationToken).ConfigureAwait(false);
            }
            catch (HttpRequestException ex) when (attempt < MaxAttempts)
            {
                TimeSpan delay = RetryDelay(null, attempt);
                string status = ex.StatusCode is { } code ? $"HTTP {(int)code}" : "network error";
                Log($"{label}: transient {status}. Retrying in {FormatDelay(delay)} ({attempt + 1}/{MaxAttempts}).");
                await DelayAsync(delay, cancellationToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested && attempt < MaxAttempts)
            {
                TimeSpan delay = RetryDelay(null, attempt);
                Log($"{label}: authentication timed out. Retrying in {FormatDelay(delay)} ({attempt + 1}/{MaxAttempts}).");
                await DelayAsync(delay, cancellationToken).ConfigureAwait(false);
            }
        }
    }

    /// <summary>Server <c>Retry-After</c> when present, else 5·2ⁿ seconds, clamped to 1..120.</summary>
    internal static TimeSpan RetryDelay(TimeSpan? serverDelay, int failedAttempt)
    {
        TimeSpan delay = serverDelay ?? TimeSpan.FromSeconds(5 * Math.Pow(2, failedAttempt - 1));
        return TimeSpan.FromSeconds(Math.Clamp(delay.TotalSeconds, 1, 120));
    }

    internal static string FormatDelay(TimeSpan delay) =>
        delay.TotalSeconds < 60
            ? $"{Math.Ceiling(delay.TotalSeconds)}s"
            : $"{Math.Ceiling(delay.TotalMinutes)}m";

    private void WriteLunarAccount(MinecraftProfile profile)
    {
        if (_lunar == null)
            return;

        // Importing must not decide which account Lunar starts with: the switch path owns
        // activeAccountLocalId, so a batch import leaves the user's current account alone.
        LunarAccountWriteResult result = _lunar.Upsert(profile, setActive: false);
        Log($"{profile.Name} ({profile.Source}): {result.Detail}");
    }

    private static string SafeFileName(string? path)
    {
        string name;
        try
        {
            name = Path.GetFileName(path ?? "");
        }
        catch (ArgumentException)
        {
            name = path ?? "";
        }

        name = name.Replace('\r', ' ').Replace('\n', ' ').Trim();
        return string.IsNullOrEmpty(name) ? "(unnamed file)" : name;
    }
}
