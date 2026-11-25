using System;

namespace Aoko.Core.Servers;

/// <summary>Outcome of validating user-entered server text.</summary>
public readonly record struct ServerTarget(bool Ok, ServerAddress Address, string Error)
{
    public static ServerTarget Valid(ServerAddress address) => new(true, address, "");

    public static ServerTarget Invalid(string error) => new(false, default, error);
}

/// <summary>
/// Validation the Servers tab runs before it touches the bridge. Nothing here opens a socket;
/// the join command is handed to <c>GameStateClient</c> by the caller.
/// </summary>
public static class ServerJoinService
{
    public const string InvalidAddressMessage = ServerAddress.InvalidAddressMessage;

    /// <summary>Same host cap the bridge-side Java validation applies.</summary>
    public const int MaximumHostLength = 256;

    public static ServerTarget ValidateTarget(string? text)
    {
        if (!ServerAddress.TryParse(text, out ServerAddress address))
            return ServerTarget.Invalid(InvalidAddressMessage);

        return ServerTarget.Valid(address);
    }

    /// <summary>Builds the host and port the bridge's join action expects.</summary>
    public static bool TryBuildJoinRequest(ServerAddress address, out string host, out int port)
    {
        host = "";
        port = ServerAddress.DefaultPort;

        string trimmed = address.Host?.Trim() ?? "";
        if (trimmed.Length == 0 || trimmed.Length > MaximumHostLength)
            return false;
        if (address.Port is < 1 or > 65535)
            return false;

        host = trimmed;
        port = address.Port;
        return true;
    }
}
