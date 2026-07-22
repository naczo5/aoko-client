using System;
using System.Collections.Generic;
using System.Text.Json.Serialization;

namespace Aoko.Core;

/// <summary>
/// Represents the game state received from the injected Java agent.
/// Deserialized from JSON sent over TCP from the in-game agent.
/// </summary>
public class GameState
{
    [JsonPropertyName("mapped")]
    public bool Mapped { get; set; }

    [JsonPropertyName("guiOpen")]
    public bool GuiOpen { get; set; }

    [JsonPropertyName("screenName")]
    public string ScreenName { get; set; } = "unknown";

    [JsonPropertyName("actionBar")]
    public string ActionBar { get; set; } = "";

    [JsonPropertyName("health")]
    public float Health { get; set; } = -1;

    [JsonPropertyName("fov")]
    public float Fov { get; set; } = 70.0f;

    [JsonPropertyName("viewportWidth")]
    public int ViewportWidth { get; set; }

    [JsonPropertyName("viewportHeight")]
    public int ViewportHeight { get; set; }

    [JsonPropertyName("holdingBlock")]
    public bool HoldingBlock { get; set; }

    [JsonPropertyName("lookingAtBlock")]
    public bool LookingAtBlock { get; set; }

    [JsonPropertyName("lookingAtEntity")]
    public bool LookingAtEntity { get; set; }

    [JsonPropertyName("lookingAtEntityLatched")]
    public bool LookingAtEntityLatched { get; set; }

    [JsonPropertyName("breakingBlock")]
    public bool BreakingBlock { get; set; }

    [JsonPropertyName("attackCooldown")]
    public float AttackCooldown { get; set; } = 1.0f;

    [JsonPropertyName("attackCooldownPerTick")]
    public float AttackCooldownPerTick { get; set; } = 0.08f;

    [JsonPropertyName("killAuraUnavailableReason")]
    public string KillAuraUnavailableReason { get; set; } = "";

    [JsonPropertyName("killAuraHasTarget")]
    public bool KillAuraHasTarget { get; set; }

    [JsonPropertyName("killAuraBlocking")]
    public bool KillAuraBlocking { get; set; }

    [JsonPropertyName("stateMs")]
    public ulong StateMs { get; set; }

    [JsonPropertyName("posX")]
    public double PosX { get; set; }

    [JsonPropertyName("posY")]
    public double PosY { get; set; }

    [JsonPropertyName("posZ")]
    public double PosZ { get; set; }

    [JsonPropertyName("pitch")]
    public float Pitch { get; set; }

    [JsonPropertyName("entities")]
    public List<EntityInfo> Entities { get; set; } = new();

    [JsonPropertyName("chestStealerState")]
    public ChestStealerState? ChestStealerState { get; set; }

    [JsonPropertyName("pixelPartyTargetFound")]
    public bool PixelPartyTargetFound { get; set; }

    [JsonPropertyName("pixelPartyTargetYaw")]
    public float PixelPartyTargetYaw { get; set; }

    [JsonPropertyName("pixelPartyTargetDist")]
    public float PixelPartyTargetDist { get; set; }

    [JsonPropertyName("pixelPartyYawDelta")]
    public float PixelPartyYawDelta { get; set; }

    /// <summary>
    /// Whether the agent is connected and sending data.
    /// </summary>
    [JsonIgnore]
    public bool IsConnected { get; set; }

    /// <summary>
    /// Timestamp of the last received update.
    /// </summary>
    [JsonIgnore]
    public DateTime LastUpdate { get; set; } = DateTime.MinValue;
}

public class ChestStealerState
{
    [JsonPropertyName("ready")]
    public bool Ready { get; set; }

    [JsonPropertyName("physical")]
    public bool Physical { get; set; }

    [JsonPropertyName("windowId")]
    public int WindowId { get; set; } = -1;

    [JsonPropertyName("screenWidth")]
    public int ScreenWidth { get; set; }

    [JsonPropertyName("screenHeight")]
    public int ScreenHeight { get; set; }

    [JsonPropertyName("slots")]
    public List<ChestStealerSlot> Slots { get; set; } = new();
}

public class ChestStealerSlot
{
    [JsonPropertyName("index")]
    public int Index { get; set; }

    [JsonPropertyName("slotNumber")]
    public int SlotNumber { get; set; }

    [JsonPropertyName("x")]
    public int X { get; set; }

    [JsonPropertyName("y")]
    public int Y { get; set; }
}

public class EntityInfo
{
    [JsonPropertyName("sx")]
    public float Sx { get; set; }

    [JsonPropertyName("sy")]
    public float Sy { get; set; }

    [JsonPropertyName("dist")]
    public double Dist { get; set; }

    [JsonPropertyName("name")]
    public string Name { get; set; } = ""; // Was missing in original? No, it wasn't there?

    [JsonPropertyName("hp")]
    public float Hp { get; set; }

    // Legacy / Debug fields (optional)
    [JsonPropertyName("x")]
    public double X { get; set; }
    [JsonPropertyName("y")]
    public double Y { get; set; }
    [JsonPropertyName("z")]
    public double Z { get; set; }

    [JsonIgnore]
    public double Distance => Dist; // Helper alias
}
