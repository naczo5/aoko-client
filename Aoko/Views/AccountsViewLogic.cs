using System;
using System.Collections.Generic;
using Aoko.Core.Accounts;

namespace Aoko.Views;

/// <summary>Which session chip the strip shows.</summary>
public enum SessionChipTone
{
    Offline,
    MainMenu,
    InWorld
}

/// <summary>Colour family for an account's source chip.</summary>
public enum AccountSourceTone
{
    Neutral,
    Localts,
    Offline
}

/// <summary>
/// The Accounts tab's decisions, kept out of the control so they can be exercised
/// without WPF: the capability gate, the session strip, the source chips, and the
/// rule that no action is armed until the user picks a card.
/// </summary>
internal static class AccountsViewLogic
{
    /// <summary>Short, actionable variant of the offline-name rule.</summary>
    public const string InvalidOfflineName = "Use 3–16 letters, numbers or underscores.";

    internal static SessionChipTone SessionChipToneFor(bool connected, bool inWorld)
    {
        if (!connected)
            return SessionChipTone.Offline;

        return inWorld ? SessionChipTone.InWorld : SessionChipTone.MainMenu;
    }

    internal static bool CanSwitchSessions(bool connected, bool supportsSetSession)
        => connected && supportsSetSession;

    /// <summary>Only an explicit selection plus an idle, capable tab arms the primary action.</summary>
    internal static bool CanUseSelectedAccount(bool connected, bool supportsSetSession, bool busy, bool hasSelection)
        => !busy && hasSelection && CanSwitchSessions(connected, supportsSetSession);

    /// <summary>
    /// Index to select after a reload, or -1. No fallback: opening the tab must not
    /// arm Use account / Delete for a card the user never chose.
    /// </summary>
    internal static int SelectionIndexAfterLoad(IReadOnlyList<string> ids, string? keepId)
    {
        ArgumentNullException.ThrowIfNull(ids);

        if (keepId is null)
            return -1;

        for (int i = 0; i < ids.Count; i++)
        {
            if (string.Equals(ids[i], keepId, StringComparison.Ordinal))
                return i;
        }

        return -1;
    }

    /// <summary>
    /// Why a switch or restore cannot start right now, or null when it can. The
    /// in-world case stays clickable so the refusal shows where the user clicked.
    /// </summary>
    internal static string? SessionSwitchRefusal(bool connected, bool supportsSetSession, bool inWorld)
    {
        if (!connected)
            return TabCopy.NotConnected;
        if (!supportsSetSession)
            return TabCopy.UnsupportedAccountSwitch;
        return inWorld ? TabCopy.ReturnToMainMenu : null;
    }

    /// <summary>Persistent line beside the primary action while it is disabled.</summary>
    internal static string PrimaryHint(bool connected, bool supportsSetSession, bool hasSelection)
    {
        if (!connected)
            return TabCopy.NotConnected;
        if (!supportsSetSession)
            return TabCopy.UnsupportedAccountSwitch;
        return hasSelection ? "" : TabCopy.SelectAccountHint;
    }

    /// <summary>The strip's second line: never asks for a button that is disabled.</summary>
    internal static string SessionCaption(bool connected, bool supportsSessionRead)
    {
        if (!connected)
            return "Inject the client to read its session.";
        return supportsSessionRead ? "Refresh to read it." : "This aoko build cannot read the session.";
    }

    internal static string SourceChipLabel(string? source, bool hasRefresh)
    {
        if (IsSource(source, AccountSources.Offline))
            return "Cracked";
        if (hasRefresh || IsSource(source, AccountSources.Localts))
            return "Localts";
        if (IsSource(source, AccountSources.Token))
            return "Token";
        if (IsSource(source, AccountSources.Cookie))
            return "Cookie";
        return "Saved";
    }

    internal static AccountSourceTone SourceToneFor(string? source, bool hasRefresh)
    {
        if (IsSource(source, AccountSources.Offline))
            return AccountSourceTone.Offline;
        return hasRefresh || IsSource(source, AccountSources.Localts)
            ? AccountSourceTone.Localts
            : AccountSourceTone.Neutral;
    }

    internal static bool IsCurrentSession(string? accountUuid, string? sessionUuid)
        => AccountStore.SameUuid(accountUuid, sessionUuid);

    internal static string FormatUuid(string? uuid)
    {
        string compact = (uuid ?? "").Replace("-", "", StringComparison.Ordinal);
        if (compact.Length != 32)
            return uuid ?? "";

        return $"{compact[..8]}-{compact[8..12]}-{compact[12..16]}-{compact[16..20]}-{compact[20..]}";
    }

    internal static string LastUsedLabel(DateTimeOffset? lastUsed)
        => lastUsed is null ? "Never used" : $"Last used {RelativeTime.Format(lastUsed.Value)}";

    /// <summary>What a screen reader says for one card.</summary>
    internal static string CardAutomationName(string? name, string? sourceLabel, bool isCurrent)
    {
        string value = $"{name}, {sourceLabel}";
        return isCurrent ? $"{value}, in use" : value;
    }

    private static bool IsSource(string? source, string expected)
        => string.Equals(source ?? "", expected, StringComparison.OrdinalIgnoreCase);
}
