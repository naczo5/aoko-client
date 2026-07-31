using System;
using System.Collections.Generic;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Aoko.Core;

/// <summary>
/// Applies an explicitly marked partial state update to the latest complete state.
/// V1 messages remain unchanged; this seam is only used for messages whose type is
/// <c>statePatch</c> or that carry <c>partial=true</c>.
/// </summary>
internal static class GameStatePatchMerger
{
    private static readonly JsonSerializerOptions SerializerOptions = new(JsonSerializerDefaults.Web);

    public static bool IsPartial(JsonNode? node)
    {
        if (node is not JsonObject obj)
            return false;

        try
        {
            if (obj["type"]?.GetValue<string>() == "statePatch")
                return true;

            return obj["partial"]?.GetValue<bool>() == true;
        }
        catch (InvalidOperationException)
        {
            return false;
        }
        catch (FormatException)
        {
            return false;
        }
    }

    public static GameState Apply(GameState previous, JsonObject patch)
    {
        ArgumentNullException.ThrowIfNull(previous);
        ArgumentNullException.ThrowIfNull(patch);

        var merged = new GameState
        {
            Mapped = previous.Mapped,
            GuiOpen = previous.GuiOpen,
            InWorld = previous.InWorld,
            ScreenName = previous.ScreenName,
            ActionBar = previous.ActionBar,
            Health = previous.Health,
            Fov = previous.Fov,
            ViewportWidth = previous.ViewportWidth,
            ViewportHeight = previous.ViewportHeight,
            HoldingBlock = previous.HoldingBlock,
            LookingAtBlock = previous.LookingAtBlock,
            LookingAtEntity = previous.LookingAtEntity,
            LookingAtEntityLatched = previous.LookingAtEntityLatched,
            BreakingBlock = previous.BreakingBlock,
            AttackCooldown = previous.AttackCooldown,
            AttackCooldownPerTick = previous.AttackCooldownPerTick,
            KillAuraUnavailableReason = previous.KillAuraUnavailableReason,
            KillAuraHasTarget = previous.KillAuraHasTarget,
            KillAuraBlocking = previous.KillAuraBlocking,
            StateMs = previous.StateMs,
            PosX = previous.PosX,
            PosY = previous.PosY,
            PosZ = previous.PosZ,
            Pitch = previous.Pitch,
            Entities = previous.Entities,
            ChestStealerState = previous.ChestStealerState,
            PixelPartyTargetFound = previous.PixelPartyTargetFound,
            PixelPartyTargetYaw = previous.PixelPartyTargetYaw,
            PixelPartyTargetDist = previous.PixelPartyTargetDist,
            PixelPartyYawDelta = previous.PixelPartyYawDelta,
            IsConnected = previous.IsConnected,
            LastUpdate = previous.LastUpdate
        };

        foreach ((string name, JsonNode? value) in patch)
        {
            if (name is "type" or "partial")
                continue;

            switch (name)
            {
                case "mapped": TryRead<bool>(value, v => merged.Mapped = v); break;
                case "guiOpen": TryRead<bool>(value, v => merged.GuiOpen = v); break;
                case "inWorld": TryRead<bool>(value, v => merged.InWorld = v); break;
                case "screenName": TryRead<string>(value, v => merged.ScreenName = v); break;
                case "actionBar": TryRead<string>(value, v => merged.ActionBar = v); break;
                case "health": TryRead<float>(value, v => merged.Health = v); break;
                case "fov": TryRead<float>(value, v => merged.Fov = v); break;
                case "viewportWidth": TryRead<int>(value, v => merged.ViewportWidth = v); break;
                case "viewportHeight": TryRead<int>(value, v => merged.ViewportHeight = v); break;
                case "holdingBlock": TryRead<bool>(value, v => merged.HoldingBlock = v); break;
                case "lookingAtBlock": TryRead<bool>(value, v => merged.LookingAtBlock = v); break;
                case "lookingAtEntity": TryRead<bool>(value, v => merged.LookingAtEntity = v); break;
                case "lookingAtEntityLatched": TryRead<bool>(value, v => merged.LookingAtEntityLatched = v); break;
                case "breakingBlock": TryRead<bool>(value, v => merged.BreakingBlock = v); break;
                case "attackCooldown": TryRead<float>(value, v => merged.AttackCooldown = v); break;
                case "attackCooldownPerTick": TryRead<float>(value, v => merged.AttackCooldownPerTick = v); break;
                case "killAuraUnavailableReason": TryRead<string>(value, v => merged.KillAuraUnavailableReason = v); break;
                case "killAuraHasTarget": TryRead<bool>(value, v => merged.KillAuraHasTarget = v); break;
                case "killAuraBlocking": TryRead<bool>(value, v => merged.KillAuraBlocking = v); break;
                case "stateMs": TryRead<ulong>(value, v => merged.StateMs = v); break;
                case "posX": TryRead<double>(value, v => merged.PosX = v); break;
                case "posY": TryRead<double>(value, v => merged.PosY = v); break;
                case "posZ": TryRead<double>(value, v => merged.PosZ = v); break;
                case "pitch": TryRead<float>(value, v => merged.Pitch = v); break;
                case "entities":
                    if (value is null) merged.Entities = new List<EntityInfo>();
                    else TryRead<List<EntityInfo>>(value, v => merged.Entities = v ?? new());
                    break;
                case "chestStealerState":
                    if (value is null) merged.ChestStealerState = null;
                    else TryRead<ChestStealerState>(value, v => merged.ChestStealerState = v);
                    break;
                case "pixelPartyTargetFound": TryRead<bool>(value, v => merged.PixelPartyTargetFound = v); break;
                case "pixelPartyTargetYaw": TryRead<float>(value, v => merged.PixelPartyTargetYaw = v); break;
                case "pixelPartyTargetDist": TryRead<float>(value, v => merged.PixelPartyTargetDist = v); break;
                case "pixelPartyYawDelta": TryRead<float>(value, v => merged.PixelPartyYawDelta = v); break;
            }
        }

        // A transition packet is allowed to omit expensive producer payloads,
        // but it must not leave combat/stealer targets from the previous world
        // visible to input modules for the next state notification.
        if (patch["inWorld"] is JsonValue inWorldValue &&
            inWorldValue.TryGetValue<bool>(out bool inWorld) && !inWorld)
        {
            merged.Entities = new List<EntityInfo>();
            merged.ChestStealerState = null;
        }

        return merged;
    }

    private static void TryRead<T>(JsonNode? value, Action<T> assign)
    {
        if (value is null)
            return;

        try
        {
            T? parsed = value.Deserialize<T>(SerializerOptions);
            if (parsed is not null)
                assign(parsed);
        }
        catch (JsonException)
        {
        }
        catch (InvalidOperationException)
        {
        }
        catch (FormatException)
        {
        }
        catch (OverflowException)
        {
        }
    }
}
