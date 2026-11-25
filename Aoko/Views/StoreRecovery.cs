using System;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Reflection;
using Aoko.Core.Accounts;

namespace Aoko.Views;

/// <summary>
/// A recovered store looks exactly like a fresh install, so both tabs ask this
/// helper whether a corrupt file was moved aside during this session. The hook is
/// optional: when a store grows a <c>RecoveredFrom</c> property this reads it
/// reflectively, and until then the quarantine file on disk answers the question.
/// Nothing here is a compile-time dependency on a member that does not exist yet.
/// </summary>
internal static class StoreRecovery
{
    internal const string AccountsFileName = "accounts.json";
    internal const string ServersFileName = "servers.json";
    internal const string QuarantineMarker = ".corrupt-";

    /// <summary>Where both stores live; the tabs never show a quarantine from an older run.</summary>
    internal static string DefaultDirectory => AccountStore.DefaultDirectory;

    /// <summary>Anything written before this process started belongs to an older run.</summary>
    internal static DateTime SessionStartUtc { get; } = ReadProcessStart();

    private static DateTime ReadProcessStart()
    {
        try
        {
            return Process.GetCurrentProcess().StartTime.ToUniversalTime();
        }
        catch (Exception ex) when (ex is InvalidOperationException or NotSupportedException
                                      or System.ComponentModel.Win32Exception)
        {
            return DateTime.UtcNow;
        }
    }

    /// <summary>"accounts.json could not be read. A copy was kept as accounts.json.corrupt-…"</summary>
    internal static string Message(string quarantinedPath)
    {
        string file = FileNameOf(quarantinedPath);
        int marker = file.IndexOf(QuarantineMarker, StringComparison.Ordinal);
        string original = marker > 0 ? file[..marker] : "The saved file";
        return $"{original} could not be read. A copy was kept as {file}.";
    }

    /// <summary>Both separators, so a Windows path formats correctly on any test host.</summary>
    private static string FileNameOf(string path)
    {
        int separator = path.LastIndexOfAny(new[] { '/', '\\' });
        return separator < 0 ? path : path[(separator + 1)..];
    }

    /// <summary>The optional store hook: a public <c>RecoveredFrom</c> string property.</summary>
    internal static string? FromStore(object? store)
    {
        if (store is null)
            return null;

        try
        {
            PropertyInfo? property = store.GetType().GetProperty(
                "RecoveredFrom",
                BindingFlags.Public | BindingFlags.Instance);

            if (property?.PropertyType != typeof(string))
                return null;

            string? recovered = property.GetValue(store) as string;
            return string.IsNullOrWhiteSpace(recovered) ? null : Message(recovered);
        }
        catch (Exception ex) when (ex is TargetInvocationException or MemberAccessException
                                      or InvalidOperationException or ArgumentException)
        {
            return null;
        }
    }

    /// <summary>
    /// Newest quarantine file written after <paramref name="recoveredAfterUtc"/>, so an
    /// old corrupt-* copy from a previous run does not raise the notice forever.
    /// </summary>
    internal static string? FindQuarantined(string directory, string storeFileName, DateTime recoveredAfterUtc)
    {
        try
        {
            if (!Directory.Exists(directory))
                return null;

            string prefix = storeFileName + QuarantineMarker;
            string? newest = Directory.GetFiles(directory)
                .Where(path => Path.GetFileName(path).StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                .Where(path => File.GetLastWriteTimeUtc(path) >= recoveredAfterUtc)
                .OrderByDescending(File.GetLastWriteTimeUtc)
                .FirstOrDefault();

            return newest is null ? null : Message(newest);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException
                                      or ArgumentException or NotSupportedException)
        {
            return null;
        }
    }

    internal static string? Notice(string directory, string storeFileName, object? store, DateTime recoveredAfterUtc)
        => BlockedFromStore(store)
           ?? FromStore(store)
           ?? FindQuarantined(directory, storeFileName, recoveredAfterUtc);

    /// <summary>
    /// A store whose file could not be read keeps the saved rows out of sight, so it says so and
    /// pauses saving rather than overwriting them.
    /// </summary>
    internal const string ReadBlockedMessage =
        "The saved file is locked or unreadable, so saving is paused to avoid overwriting it.";

    private static string? BlockedFromStore(object? store)
    {
        if (store is null)
            return null;

        try
        {
            PropertyInfo? property = store.GetType().GetProperty(
                "PersistenceBlocked",
                BindingFlags.Public | BindingFlags.Instance);

            if (property?.PropertyType != typeof(bool))
                return null;

            return (bool?)property.GetValue(store) == true ? ReadBlockedMessage : null;
        }
        catch (Exception ex) when (ex is TargetInvocationException or MemberAccessException
                                      or InvalidOperationException or ArgumentException)
        {
            return null;
        }
    }
}
