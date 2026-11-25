using Aoko.Core;

namespace Aoko.Views;

/// <summary>Tone of a transient status line. Both tabs map it to the same theme brush.</summary>
public enum StatusTone
{
    Neutral,
    Success,
    Critical
}

/// <summary>
/// The sentences both tabs share. The capability refusals live here so the two
/// views cannot drift apart, and the wording the bridge already owns is taken
/// from <see cref="BridgeCommandResult.Message"/> instead of being copied.
/// </summary>
internal static class TabCopy
{
    public const string SelectAccountHint = "Select an account to switch.";

    public const string SelectServerHint = "Select a server to join.";

    public const string NoServersHint = "Add a server, then join it from this tab.";

    public const string UnsupportedAccountSwitch =
        "This aoko build cannot switch accounts. Update aoko, then reconnect.";

    public const string UnsupportedServerJoin =
        "This aoko build cannot join servers. Update aoko, then reconnect.";

    /// <summary>One sentence fits the footer; the whole text stays in the tooltip.</summary>
    internal const int StatusLimit = 110;

    /// <summary>"Inject first, then wait for the main menu", as the bridge words it.</summary>
    public static readonly string NotConnected = CoreMessage(BridgeCommandResult.NotConnectedError);

    /// <summary>The one "return to the main menu" sentence in the app.</summary>
    public static readonly string ReturnToMainMenu = CoreMessage("in_world");

    public static string BrushKey(StatusTone tone) => tone switch
    {
        StatusTone.Success => "AccentBrush",
        StatusTone.Critical => "CriticalBrush",
        _ => "DimTextBrush"
    };

    /// <summary>Cuts a long status sentence at a word boundary; the tooltip keeps the rest.</summary>
    public static string Shorten(string? text)
    {
        string value = (text ?? "").Trim();
        if (value.Length <= StatusLimit)
            return value;

        int cut = value.LastIndexOf(' ', StatusLimit);
        if (cut < StatusLimit / 2)
            cut = StatusLimit;

        return value[..cut].TrimEnd() + "…";
    }

    private static string CoreMessage(string error)
        => BridgeCommandResult.Failure(BridgeActions.SetSession, error).Message;
}
