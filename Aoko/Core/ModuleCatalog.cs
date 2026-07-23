using System;
using System.Collections.Generic;
using System.Linq;

namespace Aoko.Core;

/// <summary>
/// Canonical registry of product modules and which surfaces each must appear on.
/// When adding a module, register it here first, then satisfy the surfaces the entry requires.
/// ModuleRegistrationTests enforces that contract.
/// </summary>
public static class ModuleCatalog
{
    [Flags]
    public enum Surfaces
    {
        None = 0,
        /// <summary>Keybind button in MainWindow.xaml (Tag=id) plus ModuleTitles / UpdateKeybindButtons.</summary>
        KeybindGui = 1 << 0,
        /// <summary>InputHooks.ModuleKeys, Profile.ModuleKeys defaults, and ToggleModule switch.</summary>
        KeybindMaps = 1 << 1,
        /// <summary>Counted by Discord Rich Presence when enabled.</summary>
        DiscordRpc = 1 << 2,
        /// <summary>Shown in the in-game top-right module list (bridge pushMod).</summary>
        OverlayList = 1 << 3,
        /// <summary>Listed in BridgeCapabilities modern fallback / bridge_capabilities.h.</summary>
        Capability = 1 << 4,
    }

    public const Surfaces StandardSurfaces =
        Surfaces.KeybindGui | Surfaces.KeybindMaps | Surfaces.DiscordRpc | Surfaces.OverlayList | Surfaces.Capability;

    public sealed record Entry(
        string Id,
        string DisplayName,
        /// <summary>Unique probe string that must appear in bridge_261.cpp module-list pushMod block.</summary>
        string OverlayProbe,
        Func<Clicker, bool> IsEnabled,
        Surfaces RequiredSurfaces = StandardSurfaces,
        /// <summary>Hidden in the external GUI unless Clicker.DevMode is on.</summary>
        bool DevOnly = false);

    public static IReadOnlyList<Entry> All { get; } =
    [
        new("autoclicker", "AutoClicker", "cfg.armed", c => c.LeftClickEnabled),
        new("rightclick", "Right Click", "cfg.rightClick", c => c.RightClickEnabled),
        new("jitter", "Jitter", "cfg.jitter", c => c.JitterEnabled),
        new("clickinchests", "Click in Chests", "cfg.clickInChests", c => c.ClickInChests),
        new("breakblocks", "Break Blocks", "cfg.breakBlocks", c => c.BreakBlocksEnabled),
        new("aimassist", "Aim Assist", "cfg.aimAssist", c => c.AimAssistEnabled),
        new("triggerbot", "Triggerbot", "cfg.triggerbot", c => c.TriggerbotEnabled),
        new("killaura", "Kill Aura", "cfg.killAura", c => c.KillAuraEnabled, DevOnly: true),
        new("speedbridge", "SpeedBridge", "cfg.speedBridge", c => c.SpeedBridgeEnabled),
        new("gtbhelper", "GTB Helper", "cfg.gtbHelper", c => c.GtbHelperEnabled),
        new("pixelpartyassist", "Pixel Party Assist", "cfg.pixelPartyAssist", c => c.PixelPartyAssistEnabled),
        new("nametags", "Nametags", "cfg.nametags", c => c.NametagsEnabled),
        new("nickhider", "Nick Hider", "cfg.nickHiderEnabled", c => c.NickHiderEnabled),
        new("closestplayer", "Closest Player", "cfg.closestPlayer", c => c.ClosestPlayerInfoEnabled),
        new("fightstatus", "Fight Status", "cfg.fightStatus", c => c.FightStatusEnabled),
        new("chestesp", "Chest ESP", "cfg.chestEsp", c => c.ChestEspEnabled),
        new("cheststealer", "Chest Stealer", "cfg.chestStealer", c => c.ChestStealerEnabled),
        new("blockesp", "Block ESP", "cfg.blockEsp", c => c.BlockEspEnabled),
        new("reach", "Reach", "cfg.reachEnabled", c => c.ReachEnabled),
        new("velocity", "Velocity", "cfg.velocityEnabled", c => c.VelocityEnabled),
        new("autototem", "AutoTotem", "cfg.autoTotemEnabled", c => c.AutoTotemEnabled),
        new("antidebuff", "AntiDebuff", "cfg.antiDebuffEnabled", c => c.AntiDebuffEnabled),
        new("hitdelayfix", "Hit Delay Fix", "cfg.hitDelayFixEnabled", c => c.HitDelayFixEnabled),

        // Keybind-only emergency action — not a toggleable feature module.
        new("panic", "Panic", "", _ => false,
            Surfaces.KeybindGui | Surfaces.KeybindMaps),

        // In-game HUD layout editor — capability + keybind maps, not RPC/overlay/GUI keybind grid.
        new("hudeditor", "HUD Editor", "", c => c.HudEditorActive,
            Surfaces.KeybindMaps | Surfaces.Capability),
    ];

    public static IEnumerable<Entry> Requiring(Surfaces surface)
        => All.Where(e => (e.RequiredSurfaces & surface) == surface);

    public static bool IsDevOnly(string moduleId)
        => All.Any(e => e.DevOnly && string.Equals(e.Id, moduleId, StringComparison.OrdinalIgnoreCase));

    public static bool IsGuiVisible(Entry entry, Clicker clicker)
        => !entry.DevOnly || clicker.DevMode;

    public static int CountEnabledForDiscord(Clicker clicker)
        => Requiring(Surfaces.DiscordRpc)
            .Count(e => IsGuiVisible(e, clicker) && e.IsEnabled(clicker));
}
