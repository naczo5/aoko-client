using System;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Threading;

namespace Aoko.Core;

/// <summary>
/// Wire action names for the bridge session/server commands plus the
/// capability strings that advertise them in the bridge "actions" array.
/// </summary>
public static class BridgeActions
{
    public const string GetSession = "getSession";
    public const string SetSession = "setSession";
    public const string RestoreSession = "restoreSession";
    public const string Join = "join";

    // Advertised actions (BridgeCapabilities.SupportsAction).
    public const string AccountGetSession = "account.getSession";
    public const string AccountSetSession = "account.setSession";
    public const string AccountRestoreSession = "account.restoreSession";
    public const string ServerJoin = "server.join";

    // Outbound message types, one JSON object per line on port 25590.
    public const string AccountCommandType = "accountCommand";
    public const string ServerCommandType = "serverCommand";

    // Inbound reply type carried alongside "state" and "capabilities".
    public const string CommandResultType = "commandResult";

    /// <summary>Line framing used by every existing bridge message.</summary>
    public const string LineTerminator = "\n";
}

/// <summary>
/// An ACCOUNTS command for the bridge. Only <see cref="SetSession"/> carries
/// credentials, and <see cref="ToString"/> never prints them: this type is safe
/// to log.
/// </summary>
public sealed record AccountCommand
{
    private AccountCommand(
        string action,
        string? name = null,
        string? uuid = null,
        string? msaToken = null,
        string? refreshToken = null)
    {
        Action = action;
        Name = name;
        Uuid = uuid;
        MsaToken = msaToken;
        RefreshToken = refreshToken;
    }

    public string Action { get; }

    public string? Name { get; }

    public string? Uuid { get; }

    /// <summary>Minecraft services access token. Never written to logs.</summary>
    public string? MsaToken { get; }

    public string? RefreshToken { get; }

    public bool CarriesCredentials
        => string.Equals(Action, BridgeActions.SetSession, StringComparison.OrdinalIgnoreCase);

    public static AccountCommand GetSession() => new(BridgeActions.GetSession);

    public static AccountCommand SetSession(
        string name,
        string uuid,
        string msaToken,
        string? refreshToken = null)
        => new(BridgeActions.SetSession, name, uuid, msaToken, refreshToken);

    public static AccountCommand RestoreSession() => new(BridgeActions.RestoreSession);

    /// <summary>Keeps tokens out of logs and diagnostics strings.</summary>
    public override string ToString() => $"AccountCommand {{ Action = {Action} }}";
}

/// <summary>
/// Monotonic, thread-safe request-id source for bridge commands. Command ids are
/// deliberately separate from the module-action ids echoed as "lastActionId".
/// </summary>
public sealed class BridgeRequestIdSource
{
    private int _last;

    public int Next() => Interlocked.Increment(ref _last);
}

/// <summary>
/// Builds the newline-terminated JSON lines for the session/server commands.
/// Serialisation matches the rest of <see cref="GameStateClient"/>: default
/// <see cref="JsonSerializer"/> options plus "\n" framing.
/// </summary>
public static class BridgeCommandWire
{
    public static string BuildAccountCommandLine(int requestId, AccountCommand command)
    {
        ArgumentNullException.ThrowIfNull(command);

        var payload = new JsonObject
        {
            ["type"] = BridgeActions.AccountCommandType,
            ["action"] = command.Action,
            ["requestId"] = requestId
        };

        if (command.CarriesCredentials)
        {
            payload["name"] = command.Name ?? string.Empty;
            payload["uuid"] = command.Uuid ?? string.Empty;
            payload["token"] = command.MsaToken ?? string.Empty;
            if (!string.IsNullOrEmpty(command.RefreshToken))
                payload["refreshToken"] = command.RefreshToken;
        }

        return JsonSerializer.Serialize(payload) + BridgeActions.LineTerminator;
    }

    public static string BuildServerCommandLine(int requestId, string host, int port)
    {
        var payload = new JsonObject
        {
            ["type"] = BridgeActions.ServerCommandType,
            ["action"] = BridgeActions.Join,
            ["requestId"] = requestId,
            ["host"] = host ?? string.Empty,
            ["port"] = port
        };

        return JsonSerializer.Serialize(payload) + BridgeActions.LineTerminator;
    }
}

/// <summary>The inbound handler one bridge line must run.</summary>
internal enum BridgeInboundKind
{
    /// <summary>Anything that is not a reply or a capabilities frame: game state or a patch.</summary>
    State = 0,

    /// <summary>A correlated account/server reply.</summary>
    CommandResult = 1,

    /// <summary>The capabilities frame the bridge sends once per connection.</summary>
    Capabilities = 2
}

/// <summary>
/// Decides the handler for an inbound line from the <em>parsed</em> <c>type</c> property.
/// Matching on serialized text meant a reply written with a space
/// (<c>"type" : "commandResult"</c>) or with the property reordered fell through to the state
/// branch, where it deserialized into an empty <see cref="GameState"/> and blanked the HUD
/// while the pending command still waited out its full budget.
/// </summary>
internal static class BridgeInboundClassifier
{
    internal static BridgeInboundKind Classify(JsonNode? node)
    {
        string type = ReadType(node);
        if (string.Equals(type, BridgeActions.CommandResultType, StringComparison.OrdinalIgnoreCase))
            return BridgeInboundKind.CommandResult;

        if (string.Equals(type, "capabilities", StringComparison.OrdinalIgnoreCase))
            return BridgeInboundKind.Capabilities;

        return BridgeInboundKind.State;
    }

    /// <summary>The message type, or an empty string when the line is not an object with a string type.</summary>
    internal static string ReadType(JsonNode? node)
    {
        if (node is not JsonObject payload
            || !payload.TryGetPropertyValue("type", out JsonNode? value)
            || value is not JsonValue jsonValue
            || !jsonValue.TryGetValue(out string? type))
        {
            return string.Empty;
        }

        return type ?? string.Empty;
    }
}
