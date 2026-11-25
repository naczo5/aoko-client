using System;
using System.Diagnostics;
using System.Text.Json.Nodes;

namespace Aoko.Core;

/// <summary>
/// A correlated bridge reply to an account/server command, decoded from an
/// inbound <c>{"type":"commandResult"}</c> line. <see cref="Message"/> turns the
/// bridge error vocabulary into a sentence the GUI can show as-is.
/// </summary>
public readonly record struct BridgeCommandResult
{
    // Error codes produced by the client itself. The bridge may also answer with
    // its own codes: invalid_session_data, in_world,
    // unsupported_session_constructor, session_write_not_observed,
    // game_thread_timeout, no_launch_session, invalid_address,
    // connect_path_not_found, or free-text reasons.
    public const string NotConnectedError = "not_connected";
    public const string SendFailedError = "send_failed";
    public const string TimeoutError = "timeout";
    public const string ConnectionLostError = "connection_lost";
    public const string InvalidAddressError = "invalid_address";
    public const string InvalidSessionDataError = "invalid_session_data";

    private const string InWorldMessage = "Return to the main menu before switching accounts.";
    private const string NotReadyMessage = "Wait until the client reaches the main menu, then retry.";
    private const string TimeoutMessage = "The game did not answer in time. Stay on the main menu and retry.";
    private const string ConnectPathMessage =
        "This client's connect screen could not be resolved, so the join cannot be started.";
    private const string UnmappedCodeMessage =
        "The game refused the request. Wait until the client reaches the main menu, then retry.";

    public BridgeCommandResult(
        bool ok,
        string? action = null,
        string? username = null,
        string? uuid = null,
        bool inWorld = false,
        bool ready = false,
        string? error = null)
    {
        Ok = ok;
        Action = action ?? string.Empty;
        Username = username ?? string.Empty;
        Uuid = uuid ?? string.Empty;
        InWorld = inWorld;
        Ready = ready;
        Error = error ?? string.Empty;
    }

    public bool Ok { get; }

    public string Action { get; }

    public string Username { get; }

    public string Uuid { get; }

    public bool InWorld { get; }

    public bool Ready { get; }

    public string Error { get; }

    /// <summary>User-facing sentence for this reply. Never includes a token.</summary>
    public string Message => Ok ? BuildSuccessMessage() : BuildFailureMessage(Error);

    /// <summary>A command that never reached the bridge.</summary>
    public static BridgeCommandResult Failure(string? action, string? error)
        => new(false, action, error: error);

    /// <summary>Keeps the requested action when the reply omits it.</summary>
    public BridgeCommandResult WithAction(string? action)
        => Action.Length > 0 || string.IsNullOrEmpty(action)
            ? this
            : new BridgeCommandResult(Ok, action, Username, Uuid, InWorld, Ready, Error);

    /// <summary>
    /// Decodes a "commandResult" payload. Returns false for any other message
    /// type, a non-object document, or a missing/non-positive requestId, so the
    /// read loop can leave such lines alone.
    /// </summary>
    public static bool TryParse(JsonNode? node, out int requestId, out BridgeCommandResult result)
    {
        requestId = 0;
        result = default;

        if (node is not JsonObject payload)
            return false;

        if (!string.Equals(
                ReadString(payload, "type"),
                BridgeActions.CommandResultType,
                StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        if (!TryReadInt(payload, "requestId", out int parsedRequestId) || parsedRequestId <= 0)
            return false;

        result = new BridgeCommandResult(
            ReadBool(payload, "ok"),
            ReadString(payload, "action"),
            ReadString(payload, "username"),
            ReadString(payload, "uuid"),
            ReadBool(payload, "inWorld"),
            ReadBool(payload, "ready"),
            ReadString(payload, "error"));
        requestId = parsedRequestId;
        return true;
    }

    private string BuildSuccessMessage()
    {
        bool hasUsername = Username.Length > 0;

        if (string.Equals(Action, BridgeActions.GetSession, StringComparison.OrdinalIgnoreCase))
            return hasUsername ? $"Signed in as {Username}." : "No account is signed in right now.";

        if (string.Equals(Action, BridgeActions.SetSession, StringComparison.OrdinalIgnoreCase))
            return hasUsername ? $"Switched to {Username}." : "Account switched.";

        if (string.Equals(Action, BridgeActions.RestoreSession, StringComparison.OrdinalIgnoreCase))
            return hasUsername ? $"Restored {Username}." : "Session restored.";

        if (string.Equals(Action, BridgeActions.Join, StringComparison.OrdinalIgnoreCase))
            return "Join started. The client is connecting to the server.";

        return "The game completed the command.";
    }

    private string BuildFailureMessage(string? error)
    {
        string code = (error ?? string.Empty).Trim();
        if (code.Length == 0)
            return "The game refused the request without a reason. Wait until the client reaches the main menu, then retry.";

        string normalized = code.ToLowerInvariant();
        switch (normalized)
        {
            case "in_world":
                return InWorldMessage;
            case "game_thread_timeout":
            case TimeoutError:
                return TimeoutMessage;
            case "connect_path_not_found":
                return ConnectPathMessage;
            case "not_ready":
            case "notready":
                return NotReadyMessage;
            case InvalidSessionDataError:
                return "The account details were rejected. Re-import the account and retry.";
            case "unsupported_session_constructor":
                return "This Minecraft version's session constructor is not supported, so the account cannot be switched.";
            case "session_write_not_observed":
                return "The game did not apply the new session. Stay on the main menu and retry.";
            case "no_launch_session":
                return "The client has no launch session to restore.";
            case InvalidAddressError:
                return "That server address is not valid.";
            case NotConnectedError:
                return "The GUI is not connected to the game yet. Inject first, then wait for the main menu.";
            case SendFailedError:
                return "The command could not be sent to the game.";
            case ConnectionLostError:
                return "The connection to the game dropped before the command finished.";
        }

        // Free-text reasons such as "Session helper not ready (wait on main menu,
        // then retry)" or "Disconnect from the world before switching" still map.
        if (normalized.Contains("not ready", StringComparison.Ordinal))
            return NotReadyMessage;
        if (normalized.Contains("disconnect from the world", StringComparison.Ordinal))
            return InWorldMessage;

        // Anything else is bridge text we did not vet: keep it out of the UI (a future
        // describeThrowable string can carry Java exception text) and record it locally.
        Debug.WriteLine($"[BridgeCommandResult] unmapped bridge error code: {code}");
        return UnmappedCodeMessage;
    }

    private static string ReadString(JsonObject payload, string name)
        => payload.TryGetPropertyValue(name, out JsonNode? value)
            && value is JsonValue jsonValue
            && jsonValue.TryGetValue(out string? text)
                ? text ?? string.Empty
                : string.Empty;

    private static bool ReadBool(JsonObject payload, string name)
        => payload.TryGetPropertyValue(name, out JsonNode? value)
            && value is JsonValue jsonValue
            && jsonValue.TryGetValue(out bool flag)
            && flag;

    private static bool TryReadInt(JsonObject payload, string name, out int parsed)
    {
        parsed = 0;
        return payload.TryGetPropertyValue(name, out JsonNode? value)
            && value is JsonValue jsonValue
            && jsonValue.TryGetValue(out parsed);
    }
}
