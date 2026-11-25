using System;
using System.Collections.Generic;
using System.Linq;

namespace Aoko.Core;

/// <summary>A contiguous slice of a display name that matched the search query.</summary>
public sealed record ModuleSearchRange(int Start, int Length);

/// <summary>One searchable row in the command search overlay: a GUI module, or a non-module tab destination.</summary>
public sealed class ModuleSearchEntry
{
    public required string Id { get; init; }
    public required string DisplayName { get; init; }

    /// <summary>TabItem header the row jumps to.</summary>
    public required string Tab { get; init; }

    /// <summary>
    /// GUI category the row belongs to: the module's tab, or "Tabs" for destinations. The overlay renders
    /// one ranked list and never groups by it; ModuleSearchIndexTests reads it to pin tab correctness.
    /// </summary>
    public required string Group { get; init; }

    /// <summary>x:Name of the module card Border in MainWindow.xaml (empty for destinations).</summary>
    public string CardName { get; init; } = string.Empty;

    /// <summary>Clicker property the module card's checkbox binds to (empty when the row has no switch).</summary>
    public string ToggleProperty { get; init; } = string.Empty;

    /// <summary>
    /// Module card availability TextBlock that gates the card, or empty when the card is never gated. The row
    /// itself is gated on the bridge's module list instead; ModuleSearchIndexTests uses this to pin the label.
    /// </summary>
    public string AvailabilityLabel { get; init; } = string.Empty;

    public IReadOnlyList<string> Synonyms { get; init; } = Array.Empty<string>();

    /// <summary>True when ModuleCatalog requires a keybind button for this entry.</summary>
    public bool HasKeybind { get; init; }

    public bool DevOnly { get; init; }

    public bool IsDestination { get; init; }

    public int Order { get; init; }
}

/// <summary>
/// One ranked search result. <paramref name="MatchedAlias"/> is the alias or compact name that
/// matched when the display name itself did not, so the overlay can explain why the row is here.
/// </summary>
public sealed record ModuleSearchHit(
    ModuleSearchEntry Entry,
    int Rank,
    IReadOnlyList<ModuleSearchRange> NameRanges,
    string? MatchedAlias = null);

/// <summary>
/// WPF-free search index for the command search overlay. Entries mirror <see cref="ModuleCatalog"/> plus the
/// GUI placement of each module (tab, card element, toggle property). Kept in sync with MainWindow.xaml
/// by ModuleSearchIndexTests, so a new catalog module cannot ship without a searchable row.
/// </summary>
public sealed class ModuleSearchIndex
{
    // Rank tiers: lower is better. Ordering is what matters; gaps leave room for refinements.
    public const int RankExactPrefix = 0;
    public const int RankWordPrefix = 10;
    public const int RankSubstring = 20;
    public const int RankCompactSubstring = 25;
    public const int RankAliasPrefix = 30;
    public const int RankAliasSubstring = 40;
    public const int RankSubsequence = 50;
    public const int RankAliasSubsequence = 60;

    /// <summary>
    /// GUI category order: the same order as the tab rail, destinations last. Test-only since the overlay
    /// stopped grouping; ModuleSearchIndexTests asserts every entry's <see cref="ModuleSearchEntry.Group"/> is one of these.
    /// </summary>
    public static IReadOnlyList<string> GroupOrder { get; } =
        ["Combat", "Render", "Utility", "Risky", "Beta", "Tabs"];

    private sealed record GuiRow(
        string Id,
        string Tab,
        string CardName,
        string ToggleProperty,
        string AvailabilityLabel,
        string[] Synonyms);

    private sealed record DestinationRow(string Id, string DisplayName, string Tab, string[] Synonyms);

    // GUI placement per catalog id. Card names and toggle properties are anchored to MainWindow.xaml
    // by ModuleSearchIndexTests; adding a catalog entry without a row here fails that test.
    private static readonly GuiRow[] GuiRows =
    [
        new("autoclicker", "Combat", "AutoClickerCard", "IsArmed", "", ["clicker", "cps", "left click"]),
        new("rightclick", "Combat", "RightClickCard", "RightClickEnabled", "", ["right", "right click"]),
        new("aimassist", "Combat", "AimAssistCard", "AimAssistEnabled", "AimAssistAvailabilityText", ["aim", "assist"]),
        new("triggerbot", "Combat", "TriggerbotCard", "TriggerbotEnabled", "TriggerbotAvailabilityText", ["trigger"]),
        new("autorod", "Combat", "AutoRodCard", "AutoRodEnabled", "AutoRodAvailabilityText", ["rod", "fishing rod"]),
        new("throwpot", "Combat", "ThrowpotCard", "ThrowpotEnabled", "ThrowpotAvailabilityText", ["pot", "splash"]),
        new("autoheal", "Combat", "AutoHealCard", "AutoHealEnabled", "AutoHealAvailabilityText", ["heal", "potion", "gapple"]),
        new("hitdelayfix", "Combat", "HitDelayFixCard", "HitDelayFixEnabled", "HitDelayFixAvailabilityText", ["hit delay", "cooldown"]),
        new("nametags", "Render", "NametagsCard", "NametagsEnabled", "", ["tags", "names"]),
        new("nickhider", "Render", "NickHiderCard", "NickHiderEnabled", "", ["nick", "name"]),
        new("fightstatus", "Render", "FightStatusCard", "FightStatusEnabled", "", ["fight", "status", "trade"]),
        new("chestesp", "Render", "ChestEspCard", "ChestEspEnabled", "", ["esp", "chests", "storage"]),
        new("blockesp", "Render", "BlockEspCard", "BlockEspEnabled", "", ["esp", "xray", "blocks"]),
        new("bedplates", "Render", "BedPlatesCard", "BedPlatesEnabled", "", ["beds", "bed defence"]),
        new("fireballwarning", "Render", "FireballWarningCard", "FireballWarningEnabled", "", ["fireball", "warning"]),
        new("closestplayer", "Render", "ClosestPlayerCard", "ClosestPlayerInfoEnabled", "", ["closest", "nearest", "player"]),
        new("antidebuff", "Render", "AntiDebuffCard", "AntiDebuffEnabled", "AntiDebuffAvailabilityText", ["debuff", "blindness"]),
        new("hudeditor", "Render", "HudEditorCard", "HudEditorActive", "", ["hud", "layout", "elements"]),
        new("speedbridge", "Utility", "SpeedBridgeCard", "SpeedBridgeEnabled", "SpeedBridgeAvailabilityText", ["bridge", "scaffold"]),
        new("gtbhelper", "Utility", "GtbHelperCard", "GtbHelperEnabled", "GtbHelperAvailabilityText", ["gtb", "guess the build"]),
        new("pixelpartyassist", "Utility", "PixelPartyAssistCard", "PixelPartyAssistEnabled", "PixelPartyAssistAvailabilityText", ["pixel party", "terracotta"]),
        new("cheststealer", "Utility", "ChestStealerCard", "ChestStealerEnabled", "ChestStealerAvailabilityText", ["stealer", "loot", "chest"]),
        new("refill", "Utility", "RefillCard", "RefillEnabled", "RefillAvailabilityText", ["refill", "blocks"]),
        new("autotool", "Utility", "AutoToolCard", "AutoToolEnabled", "AutoToolAvailabilityText", ["tool", "pickaxe"]),
        new("panic", "Utility", "PanicCard", "", "", ["panic", "emergency", "stop"]),
        new("reach", "Risky", "ReachCard", "ReachEnabled", "ReachAvailabilityText", ["reach", "range"]),
        new("velocity", "Risky", "VelocityCard", "VelocityEnabled", "VelocityAvailabilityText", ["velocity", "knockback", "antikb"]),
        new("autototem", "Risky", "AutoTotemCard", "AutoTotemEnabled", "AutoTotemAvailabilityText", ["totem", "pop"]),
        new("killaura", "Beta", "KillAuraCard", "KillAuraEnabled", "KillAuraAvailabilityText", ["aura", "ka", "aura kill"]),
        new("jumpreset", "Beta", "JumpResetCard", "JumpResetEnabled", "JumpResetAvailabilityText", ["jump", "reset"]),
        new("hitselect", "Beta", "HitSelectCard", "HitSelectEnabled", "HitSelectAvailabilityText", ["hit", "select"]),
        new("wtap", "Beta", "WTapCard", "WTapEnabled", "WTapAvailabilityText", ["w tap", "tap"]),
        new("trajectories", "Beta", "TrajectoriesCard", "TrajectoriesEnabled", "TrajectoriesAvailabilityText", ["trajectory", "arrow"]),
        new("murdermystery", "Beta", "MurderMysteryCard", "MurderMysteryEnabled", "MurderMysteryAvailabilityText", ["mm", "murder"]),
        new("breakprogress", "Beta", "BreakProgressCard", "BreakProgressEnabled", "BreakProgressAvailabilityText", ["break", "progress"]),
        new("nohurtcam", "Beta", "NoHurtCamCard", "NoHurtCamEnabled", "NoHurtCamAvailabilityText", ["hurt", "camera", "shake"]),
        new("tnttimer", "Beta", "TntTimerCard", "TntTimerEnabled", "TntTimerAvailabilityText", ["tnt", "fuse"]),
        new("fastplace", "Beta", "FastPlaceCard", "FastPlaceEnabled", "FastPlaceAvailabilityText", ["place", "block"]),
        new("sprint", "Beta", "SprintCard", "SprintEnabled", "SprintAvailabilityText", ["sprint", "speed"]),
    ];

    // Non-module destinations.
    private static readonly DestinationRow[] DestinationRows =
    [
        new("accounts", "Accounts", "Accounts", ["account", "switch", "login", "cookies", "localts", "cracked", "session"]),
        new("servers", "Servers", "Servers", ["server", "join", "connect", "direct connect", "ping"]),
        new("settings", "Settings", "Settings", ["preferences", "theme", "palette", "configs"]),
        new("stats", "Stats", "Stats", ["statistics", "session", "kills"]),
    ];

    private static readonly Lazy<ModuleSearchIndex> LazyDefault = new(CreateDefault);

    public static ModuleSearchIndex Default => LazyDefault.Value;

    public IReadOnlyList<ModuleSearchEntry> Entries { get; }

    public ModuleSearchIndex(IReadOnlyList<ModuleSearchEntry> entries)
    {
        Entries = entries;
    }

    public static ModuleSearchIndex CreateDefault()
    {
        var rows = GuiRows.ToDictionary(row => row.Id, StringComparer.OrdinalIgnoreCase);
        var entries = new List<ModuleSearchEntry>(rows.Count + DestinationRows.Length);
        int order = 0;

        foreach (ModuleCatalog.Entry module in ModuleCatalog.All)
        {
            if (!rows.TryGetValue(module.Id, out GuiRow? row))
                continue;

            entries.Add(new ModuleSearchEntry
            {
                Id = module.Id,
                DisplayName = module.DisplayName,
                Tab = row.Tab,
                Group = row.Tab,
                CardName = row.CardName,
                ToggleProperty = row.ToggleProperty,
                AvailabilityLabel = row.AvailabilityLabel,
                Synonyms = row.Synonyms,
                HasKeybind = (module.RequiredSurfaces & ModuleCatalog.Surfaces.KeybindGui) != 0,
                DevOnly = module.DevOnly,
                Order = order++,
            });
        }

        foreach (DestinationRow destination in DestinationRows)
        {
            entries.Add(new ModuleSearchEntry
            {
                Id = destination.Id,
                DisplayName = destination.DisplayName,
                Tab = destination.Tab,
                Group = "Tabs",
                Synonyms = destination.Synonyms,
                IsDestination = true,
                Order = order++,
            });
        }

        return new ModuleSearchIndex(entries);
    }

    /// <summary>
    /// Ranks every entry against <paramref name="query"/>. An empty query returns every entry in
    /// catalog order. Ties keep catalog order, so the result is stable and deterministic.
    /// </summary>
    public IReadOnlyList<ModuleSearchHit> Rank(string? query)
    {
        string normalized = Normalize(query);

        if (normalized.Length == 0)
        {
            return Entries
                .OrderBy(entry => entry.Order)
                .Select(entry => new ModuleSearchHit(entry, RankExactPrefix, Array.Empty<ModuleSearchRange>()))
                .ToList();
        }

        var hits = new List<ModuleSearchHit>();
        foreach (ModuleSearchEntry entry in Entries)
        {
            if (TryMatch(entry, normalized, out int rank, out IReadOnlyList<ModuleSearchRange> ranges, out string? matchedAlias))
                hits.Add(new ModuleSearchHit(entry, rank, ranges, matchedAlias));
        }

        return hits
            .OrderBy(hit => hit.Rank)
            .ThenBy(hit => hit.Entry.Order)
            .ToList();
    }

    /// <summary>
    /// Rows the command search overlay displays: rank order, unchanged, minus the entries the GUI
    /// hides for the current mode. Nothing is regrouped or re-sorted, so the first row is the best
    /// match and therefore the row the overlay highlights and the row Enter continues on.
    /// </summary>
    public IReadOnlyList<ModuleSearchHit> Rows(string? query, Func<ModuleSearchEntry, bool> isVisible)
    {
        List<ModuleSearchHit> ranked = new(Rank(query));
        var rows = new List<ModuleSearchHit>(ranked.Count);
        foreach (ModuleSearchHit hit in ranked)
        {
            if (isVisible(hit.Entry))
                rows.Add(hit);
        }

        return rows;
    }

    private static string Normalize(string? query)
        => string.IsNullOrWhiteSpace(query) ? string.Empty : query.Trim().ToLowerInvariant();

    private static bool TryMatch(
        ModuleSearchEntry entry,
        string query,
        out int rank,
        out IReadOnlyList<ModuleSearchRange> ranges,
        out string? matchedAlias)
    {
        string name = entry.DisplayName.ToLowerInvariant();
        matchedAlias = null;

        if (name.StartsWith(query, StringComparison.Ordinal))
        {
            rank = RankExactPrefix;
            ranges = [new ModuleSearchRange(0, query.Length)];
            return true;
        }

        int wordStart = FindWordStart(name, query);
        if (wordStart > 0)
        {
            rank = RankWordPrefix;
            ranges = [new ModuleSearchRange(wordStart, query.Length)];
            return true;
        }

        int substring = name.IndexOf(query, StringComparison.Ordinal);
        if (substring >= 0)
        {
            rank = RankSubstring;
            ranges = [new ModuleSearchRange(substring, query.Length)];
            return true;
        }

        string compactName = name.Replace(" ", string.Empty);
        string compactQuery = query.Replace(" ", string.Empty);
        if (compactName.Length > 0 && compactName.Contains(compactQuery, StringComparison.Ordinal))
        {
            rank = RankCompactSubstring;
            ranges = Array.Empty<ModuleSearchRange>();
            matchedAlias = compactName;
            return true;
        }

        foreach (string alias in Aliases(entry))
        {
            if (alias.StartsWith(query, StringComparison.Ordinal))
            {
                rank = RankAliasPrefix;
                ranges = Array.Empty<ModuleSearchRange>();
                matchedAlias = alias;
                return true;
            }

            if (alias.Contains(query, StringComparison.Ordinal))
            {
                rank = RankAliasSubstring;
                ranges = Array.Empty<ModuleSearchRange>();
                matchedAlias = alias;
                return true;
            }
        }

        if (TrySubsequence(name, query, out IReadOnlyList<ModuleSearchRange> subsequenceRanges))
        {
            rank = RankSubsequence;
            ranges = subsequenceRanges;
            return true;
        }

        foreach (string alias in Aliases(entry))
        {
            if (TrySubsequence(alias, query, out _))
            {
                rank = RankAliasSubsequence;
                ranges = Array.Empty<ModuleSearchRange>();
                matchedAlias = alias;
                return true;
            }
        }

        rank = 0;
        ranges = Array.Empty<ModuleSearchRange>();
        return false;
    }

    private static IEnumerable<string> Aliases(ModuleSearchEntry entry)
    {
        yield return entry.Id.ToLowerInvariant();
        yield return entry.Tab.ToLowerInvariant();
        foreach (string synonym in entry.Synonyms)
            yield return synonym.ToLowerInvariant();
    }

    private static int FindWordStart(string name, string query)
    {
        for (int i = 1; i + query.Length <= name.Length; i++)
        {
            if (name[i - 1] != ' ')
                continue;
            if (string.CompareOrdinal(name, i, query, 0, query.Length) == 0)
                return i;
        }

        return -1;
    }

    private static bool TrySubsequence(string haystack, string needle, out IReadOnlyList<ModuleSearchRange> ranges)
    {
        var found = new List<ModuleSearchRange>(needle.Length);
        int cursor = 0;

        foreach (char character in needle)
        {
            int index = cursor < haystack.Length ? haystack.IndexOf(character, cursor) : -1;
            if (index < 0)
            {
                ranges = Array.Empty<ModuleSearchRange>();
                return false;
            }

            if (found.Count > 0)
            {
                ModuleSearchRange last = found[^1];
                if (last.Start + last.Length == index)
                {
                    found[^1] = last with { Length = last.Length + 1 };
                    cursor = index + 1;
                    continue;
                }
            }

            found.Add(new ModuleSearchRange(index, 1));
            cursor = index + 1;
        }

        ranges = found;
        return true;
    }
}

/// <summary>
/// What the window knows about the module behind one command search row: visibility, whether the bridge
/// can run it, its current switch value, and its keybind text.
/// </summary>
public sealed record CommandSearchRowState(
    bool IsVisible,
    bool IsAvailable,
    string UnavailableReason,
    bool IsOn,
    bool HasToggle,
    string KeybindText);

/// <summary>
/// Row-state rules, WPF-free so the switch invariant is testable without a window. The window supplies the
/// live values; this decides what the row may show. A row only offers a switch the toggle path would honour,
/// and it never reports a state the module is not in.
/// </summary>
public static class CommandSearchRowPolicy
{
    /// <summary>
    /// Resolves one row. <paramref name="supported"/> is the bridge's own answer for the module, the same
    /// predicate the keybind toggle checks before it touches the module, so what the row offers and what the
    /// toggle does cannot disagree.
    /// </summary>
    public static CommandSearchRowState Resolve(
        ModuleSearchEntry entry,
        bool devMode,
        bool supported,
        bool isOn,
        string keybindText)
    {
        bool visible = !entry.DevOnly || devMode;
        // A destination is a tab, not a module the bridge advertises, so the module list does not gate it.
        bool available = visible && (entry.IsDestination || supported);
        bool hasToggle = !entry.IsDestination && entry.ToggleProperty.Length > 0;

        return new CommandSearchRowState(
            IsVisible: visible,
            IsAvailable: available,
            UnavailableReason: available ? string.Empty : UnavailableReason(entry.Id),
            // An unsupported module is not running, so its switch reads off even when the stored flag is set.
            IsOn: hasToggle && available && isOn,
            HasToggle: hasToggle,
            KeybindText: keybindText);
    }

    /// <summary>Why a row cannot be toggled on the current bridge.</summary>
    public static string UnavailableReason(string moduleId)
        => string.Equals(moduleId, "triggerbot", StringComparison.OrdinalIgnoreCase)
            ? "Unavailable on 1.8.9 (cooldown-era PvP only)"
            : "Unavailable on current bridge";
}
