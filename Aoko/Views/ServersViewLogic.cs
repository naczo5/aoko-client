using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Linq;
using Aoko.Core.Servers;

namespace Aoko.Views;

/// <summary>Colour family for a server card's latency chip.</summary>
public enum ServerStatusTone
{
    Neutral,
    Accent,
    Warn,
    Critical
}

/// <summary>Whether this view can hand a join to the bridge right now.</summary>
public enum ServerJoinState
{
    Ready,
    Disconnected,
    Unsupported
}

/// <summary>
/// One row of the saved-server list. Holds the last ping outcome so re-sorting or a
/// store reload does not blank the latency chips on screen.
/// </summary>
public sealed class SavedServerRow : INotifyPropertyChanged
{
    public const string NotPingedLabel = "Not pinged";

    /// <summary>Single ellipsis character, matching the rest of the app's typography.</summary>
    public const string PingingLabel = "Pinging…";

    private string _motd = "";
    private string _statusText = NotPingedLabel;
    private ServerStatusTone _statusTone = ServerStatusTone.Neutral;
    private string _playersLabel = "";
    private bool _isSelected;

    public SavedServerRow(SavedServer server)
    {
        ArgumentNullException.ThrowIfNull(server);

        Id = server.Id ?? "";
        Address = server.Address ?? "";
        Name = string.IsNullOrWhiteSpace(server.Name) ? Address : server.Name;
        LastJoinedLabel = ServersViewLogic.LastJoinedLabel(server.LastJoinedAt);
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    public string Id { get; }

    public string Name { get; }

    public string Address { get; }

    public string LastJoinedLabel { get; }

    public string AutomationName => HasPlayers
        ? $"{Name}, {Address}, {StatusText}, {PlayersLabel} online"
        : $"{Name}, {Address}, {StatusText}";

    public string Motd
    {
        get => _motd;
        private set
        {
            if (string.Equals(_motd, value, StringComparison.Ordinal))
                return;

            _motd = value;
            OnPropertyChanged(nameof(Motd));
            OnPropertyChanged(nameof(HasMotd));
        }
    }

    public bool HasMotd => _motd.Length > 0;

    public string StatusText
    {
        get => _statusText;
        private set
        {
            if (string.Equals(_statusText, value, StringComparison.Ordinal))
                return;

            _statusText = value;
            OnPropertyChanged(nameof(StatusText));
            OnPropertyChanged(nameof(AutomationName));
        }
    }

    public ServerStatusTone StatusTone
    {
        get => _statusTone;
        private set
        {
            if (_statusTone == value)
                return;

            _statusTone = value;
            OnPropertyChanged(nameof(StatusTone));
        }
    }

    public string PlayersLabel
    {
        get => _playersLabel;
        private set
        {
            if (string.Equals(_playersLabel, value, StringComparison.Ordinal))
                return;

            _playersLabel = value;
            OnPropertyChanged(nameof(PlayersLabel));
            OnPropertyChanged(nameof(HasPlayers));
            OnPropertyChanged(nameof(AutomationName));
        }
    }

    public bool HasPlayers => _playersLabel.Length > 0;

    public bool IsSelected
    {
        get => _isSelected;
        set
        {
            if (_isSelected == value)
                return;

            _isSelected = value;
            OnPropertyChanged(nameof(IsSelected));
            OnPropertyChanged(nameof(SelectionStatus));
        }
    }

    /// <summary>Announced by a screen reader; a Button cannot expose SelectionItemPattern.</summary>
    public string SelectionStatus => _isSelected ? "selected" : "";

    public void MarkPinging()
    {
        StatusText = PingingLabel;
        StatusTone = ServerStatusTone.Neutral;
        PlayersLabel = "";
    }

    public void MarkNotPinged()
    {
        StatusText = NotPingedLabel;
        StatusTone = ServerStatusTone.Neutral;
        PlayersLabel = "";
        Motd = "";
    }

    public void ApplyPing(ServerPingResult result)
    {
        ArgumentNullException.ThrowIfNull(result);

        Motd = ServersViewLogic.MotdText(result.Description);
        StatusText = ServersViewLogic.PingLabel(result);
        StatusTone = ServersViewLogic.PingTone(result.Online, result.LatencyMs);
        PlayersLabel = result.Online ? result.PlayersLabel : "";
    }

    public void ApplyPingFailure(string message)
    {
        StatusText = ServersViewLogic.ErrorLabel(message);
        StatusTone = ServerStatusTone.Critical;
        PlayersLabel = "";
    }

    /// <summary>An address that cannot even be parsed: no ping is attempted.</summary>
    public void ApplyInvalidAddress()
    {
        StatusText = ServersViewLogic.InvalidAddressChipLabel;
        StatusTone = ServerStatusTone.Critical;
        PlayersLabel = "";
        Motd = "";
    }

    public void CopyPingStateFrom(SavedServerRow other)
    {
        ArgumentNullException.ThrowIfNull(other);

        Motd = other.Motd;
        StatusText = other.StatusText;
        StatusTone = other.StatusTone;
        PlayersLabel = other.PlayersLabel;
    }

    private void OnPropertyChanged(string propertyName)
        => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
}

/// <summary>
/// Pure decisions the Servers tab makes: ordering, selection retention, status
/// wording and tone, and the two independent gates (exclusive mutations versus
/// pings, which are allowed to run alongside them).
/// </summary>
internal static class ServersViewLogic
{
    /// <summary>Longest MOTD kept in the model; the card also trims to one line.</summary>
    internal const int MaximumMotdLength = 160;

    internal const string EmptySelectionMessage = "Select a saved server first.";

    internal const string InvalidAddressChipLabel = "Invalid address";

    /// <summary>Most recently joined first, then newest addition, then a stable tiebreak.</summary>
    internal static List<SavedServer> SortForDisplay(IEnumerable<SavedServer> servers)
    {
        ArgumentNullException.ThrowIfNull(servers);

        return servers
            .Where(server => server != null)
            .OrderByDescending(server => server.LastJoinedAt ?? DateTimeOffset.MinValue)
            .ThenByDescending(server => server.AddedAt)
            .ThenBy(server => server.Name ?? "", StringComparer.OrdinalIgnoreCase)
            .ThenBy(server => server.Id ?? "", StringComparer.Ordinal)
            .ToList();
    }

    /// <summary>
    /// Index to keep selected after a reload. The previous id wins; otherwise the caller's
    /// index survives so deleting a row lands on its neighbour.
    /// </summary>
    internal static int ResolveSelectionIndex(IReadOnlyList<string> ids, string? selectedId, int fallbackIndex)
    {
        ArgumentNullException.ThrowIfNull(ids);

        if (ids.Count == 0)
            return -1;

        if (!string.IsNullOrEmpty(selectedId))
        {
            for (int i = 0; i < ids.Count; i++)
            {
                if (string.Equals(ids[i], selectedId, StringComparison.Ordinal))
                    return i;
            }
        }

        return Math.Clamp(fallbackIndex, 0, ids.Count - 1);
    }

    internal static ServerJoinState EvaluateJoinState(bool isConnected, bool supportsJoin)
    {
        if (!isConnected)
            return ServerJoinState.Disconnected;

        return supportsJoin ? ServerJoinState.Ready : ServerJoinState.Unsupported;
    }

    internal static string JoinBlockedMessage(ServerJoinState state) => state switch
    {
        ServerJoinState.Unsupported => TabCopy.UnsupportedServerJoin,
        ServerJoinState.Disconnected => TabCopy.NotConnected,
        _ => ""
    };

    /// <summary>Persistent line beside the primary action while it is disabled.</summary>
    internal static string PrimaryHint(ServerJoinState state, bool hasRows, bool hasSelection)
    {
        string blocked = JoinBlockedMessage(state);
        if (blocked.Length > 0)
            return blocked;

        if (!hasRows)
            return TabCopy.NoServersHint;

        return hasSelection ? "" : TabCopy.SelectServerHint;
    }

    /// <summary>Pings never take the exclusive lock, so they only wait on other pings.</summary>
    internal static bool CanStartPing(bool pingInFlight, int rows) => rows > 0 && !pingInFlight;

    /// <summary>Joining and editing the store are the only exclusive operations.</summary>
    internal static bool CanStartMutation(bool busy) => !busy;

    internal static bool CanJoinDirect(bool busy, ServerJoinState state)
        => !busy && state == ServerJoinState.Ready;

    internal static bool CanJoinSelected(bool busy, ServerJoinState state, bool hasSelection)
        => CanJoinDirect(busy, state) && hasSelection;

    internal static string ValidationMessage(ServerTarget target)
        => target.Ok
            ? ""
            : string.IsNullOrWhiteSpace(target.Error) ? ServerJoinService.InvalidAddressMessage : target.Error;

    internal static string LastJoinedLabel(DateTimeOffset? lastJoined)
        => lastJoined is null ? "Not joined yet" : $"Last joined {RelativeTime.Format(lastJoined.Value)}";

    internal static string JoinStartedText(ServerAddress address) => $"Joining {address.Display}.";

    internal static string PingSummaryText(int total, int online)
    {
        if (total <= 0)
            return "";

        if (total == 1)
            return online == 1 ? "The server answered the ping." : "The server did not answer the ping.";

        int offline = total - online;
        return $"Pinged {total} servers: {online} online, {offline} offline.";
    }

    internal static string MotdText(string? motd)
    {
        if (string.IsNullOrWhiteSpace(motd))
            return "";

        string singleLine = string.Join(' ', motd.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries));
        return singleLine.Length <= MaximumMotdLength
            ? singleLine
            : singleLine[..MaximumMotdLength].TrimEnd();
    }

    /// <summary>The chip never shows a raw exception type or a sentence fragment.</summary>
    internal static string PingLabel(ServerPingResult result)
    {
        ArgumentNullException.ThrowIfNull(result);

        return result.Online ? $"{result.LatencyMs} ms" : ErrorLabel(result.Error);
    }

    /// <summary>Latency that a player can act on: fast, acceptable, or a failure.</summary>
    internal static ServerStatusTone PingTone(bool online, int latencyMs)
    {
        if (!online)
            return ServerStatusTone.Critical;

        if (latencyMs < 100)
            return ServerStatusTone.Accent;

        return latencyMs <= 300 ? ServerStatusTone.Warn : ServerStatusTone.Critical;
    }

    /// <summary>
    /// Maps the ping layer's free text onto three human labels. Anything that looks
    /// like a .NET type name or an unexpected reply becomes "Ping failed"; the raw
    /// reason stays in the debug log.
    /// </summary>
    internal static string ErrorLabel(string? error)
    {
        string text = (error ?? "").Trim();
        if (text.Length == 0)
            return "Offline";

        string lowered = text.ToLowerInvariant();
        if (lowered.Contains("address", StringComparison.Ordinal)
            || lowered.Contains("host:port", StringComparison.Ordinal))
            return InvalidAddressChipLabel;
        if (lowered.Contains("timed out", StringComparison.Ordinal)
            || lowered.Contains("timeout", StringComparison.Ordinal))
            return "No response";
        if (lowered.Contains("cancel", StringComparison.Ordinal))
            return "Not pinged";
        if (lowered.Contains("refused", StringComparison.Ordinal)
            || lowered.Contains("offline", StringComparison.Ordinal)
            || lowered.Contains("unreachable", StringComparison.Ordinal))
            return "Offline";

        return "Ping failed";
    }

    internal static string AddedText(SavedServer server) => $"Added {server.Name}.";

    internal static string DeletedText(string name) => $"Deleted {name}.";
}
