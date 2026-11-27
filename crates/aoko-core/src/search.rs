//! Command-search ranking, ported from `Aoko/Core/ModuleSearchIndex.cs`.
//!
//! WPF-free by design: the C# file already separated the ranking contract from the
//! overlay, and its own tests only touch XAML for the *placement* assertions. The
//! ranking (and the row-state policy at the bottom) is what this module owns.
//!
//! Rank tiers are lower-is-better and deliberately spaced, so a refinement can be
//! inserted between two tiers without renumbering:
//!
//! | Tier | Value | Meaning |
//! | --- | --- | --- |
//! | exact prefix | 0 | display name starts with the query |
//! | word prefix | 10 | a word inside the display name starts with it |
//! | substring | 20 | display name contains it |
//! | compact substring | 25 | display name with spaces removed contains it |
//! | alias prefix | 30 | module id, tab or a synonym starts with it |
//! | alias substring | 40 | one of those contains it |
//! | subsequence | 50 | scattered characters of the display name, with ranges |
//! | alias subsequence | 60 | same over an alias |
//!
//! Ties break on catalogue order, so the ranking is total and deterministic, and
//! the first row of the overlay, the highlighted row and the Enter target can never
//! disagree.

use std::borrow::Cow;

use aoko_protocol::module_catalog::{Surfaces, ALL as CATALOG};

/// One searchable row: a GUI module, or a non-module tab destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchEntry {
    /// Catalog id, or the destination id.
    pub id: &'static str,
    /// Display name shown in the row.
    pub display_name: &'static str,
    /// `TabItem` header the row jumps to.
    pub tab: &'static str,
    /// GUI group: the module's tab, or `"Tabs"` for a destination.
    pub group: &'static str,
    /// `x:Name` of the module card in `MainWindow.xaml` (empty for destinations).
    pub card_name: &'static str,
    /// `Clicker` property the card's switch binds to (empty when there is none).
    pub toggle_property: &'static str,
    /// Availability `TextBlock` that gates the card, when the card is gated.
    pub availability_label: &'static str,
    /// Curated search synonyms.
    pub synonyms: &'static [&'static str],
    /// Whether the catalog entry requires a keybind button.
    pub has_keybind: bool,
    /// Whether the catalog entry is developer-only.
    pub dev_only: bool,
    /// Whether this row is a tab destination rather than a module.
    pub is_destination: bool,
    /// Catalogue position, the tie-breaker.
    pub order: usize,
}

/// A contiguous slice of a display name that matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchRange {
    /// Byte offset into the lower-cased display name.
    pub start: usize,
    /// Length in bytes.
    pub length: usize,
}

impl SearchRange {
    const fn new(start: usize, length: usize) -> Self {
        Self { start, length }
    }
}

/// One ranked result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// The row.
    pub entry: &'static SearchEntry,
    /// Rank tier; lower is better.
    pub rank: i32,
    /// Highlighted slices when the display name itself matched.
    pub name_ranges: Vec<SearchRange>,
    /// The alias or compact name that matched, so the UI can explain the row.
    pub matched_alias: Option<Cow<'static, str>>,
}

/// Exact display-name prefix.
pub const RANK_EXACT_PREFIX: i32 = 0;
/// Word prefix inside the display name.
pub const RANK_WORD_PREFIX: i32 = 10;
/// Display-name substring.
pub const RANK_SUBSTRING: i32 = 20;
/// Space-stripped display-name substring.
pub const RANK_COMPACT_SUBSTRING: i32 = 25;
/// Alias prefix.
pub const RANK_ALIAS_PREFIX: i32 = 30;
/// Alias substring.
pub const RANK_ALIAS_SUBSTRING: i32 = 40;
/// Display-name subsequence.
pub const RANK_SUBSEQUENCE: i32 = 50;
/// Alias subsequence.
pub const RANK_ALIAS_SUBSEQUENCE: i32 = 60;

/// GUI category order, matching the tab rail; destinations last.
pub const GROUP_ORDER: [&str; 6] = ["Combat", "Render", "Utility", "Risky", "Beta", "Tabs"];

/// GUI placement for one catalog id.
struct GuiRow {
    id: &'static str,
    tab: &'static str,
    card_name: &'static str,
    toggle_property: &'static str,
    availability_label: &'static str,
    synonyms: &'static [&'static str],
}

const GUI_ROWS: &[GuiRow] = &[
    GuiRow {
        id: "autoclicker",
        tab: "Combat",
        card_name: "AutoClickerCard",
        toggle_property: "IsArmed",
        availability_label: "",
        synonyms: &["clicker", "cps", "left click"],
    },
    GuiRow {
        id: "rightclick",
        tab: "Combat",
        card_name: "RightClickCard",
        toggle_property: "RightClickEnabled",
        availability_label: "",
        synonyms: &["right", "right click"],
    },
    GuiRow {
        id: "aimassist",
        tab: "Combat",
        card_name: "AimAssistCard",
        toggle_property: "AimAssistEnabled",
        availability_label: "AimAssistAvailabilityText",
        synonyms: &["aim", "assist"],
    },
    GuiRow {
        id: "triggerbot",
        tab: "Combat",
        card_name: "TriggerbotCard",
        toggle_property: "TriggerbotEnabled",
        availability_label: "TriggerbotAvailabilityText",
        synonyms: &["trigger"],
    },
    GuiRow {
        id: "autorod",
        tab: "Combat",
        card_name: "AutoRodCard",
        toggle_property: "AutoRodEnabled",
        availability_label: "AutoRodAvailabilityText",
        synonyms: &["rod", "fishing rod"],
    },
    GuiRow {
        id: "throwpot",
        tab: "Combat",
        card_name: "ThrowpotCard",
        toggle_property: "ThrowpotEnabled",
        availability_label: "ThrowpotAvailabilityText",
        synonyms: &["pot", "splash"],
    },
    GuiRow {
        id: "autoheal",
        tab: "Combat",
        card_name: "AutoHealCard",
        toggle_property: "AutoHealEnabled",
        availability_label: "AutoHealAvailabilityText",
        synonyms: &["heal", "potion", "gapple"],
    },
    GuiRow {
        id: "hitdelayfix",
        tab: "Combat",
        card_name: "HitDelayFixCard",
        toggle_property: "HitDelayFixEnabled",
        availability_label: "HitDelayFixAvailabilityText",
        synonyms: &["hit delay", "cooldown"],
    },
    GuiRow {
        id: "nametags",
        tab: "Render",
        card_name: "NametagsCard",
        toggle_property: "NametagsEnabled",
        availability_label: "",
        synonyms: &["tags", "names"],
    },
    GuiRow {
        id: "nickhider",
        tab: "Render",
        card_name: "NickHiderCard",
        toggle_property: "NickHiderEnabled",
        availability_label: "",
        synonyms: &["nick", "name"],
    },
    GuiRow {
        id: "fightstatus",
        tab: "Render",
        card_name: "FightStatusCard",
        toggle_property: "FightStatusEnabled",
        availability_label: "",
        synonyms: &["fight", "status", "trade"],
    },
    GuiRow {
        id: "chestesp",
        tab: "Render",
        card_name: "ChestEspCard",
        toggle_property: "ChestEspEnabled",
        availability_label: "",
        synonyms: &["esp", "chests", "storage"],
    },
    GuiRow {
        id: "blockesp",
        tab: "Render",
        card_name: "BlockEspCard",
        toggle_property: "BlockEspEnabled",
        availability_label: "",
        synonyms: &["esp", "xray", "blocks"],
    },
    GuiRow {
        id: "bedplates",
        tab: "Render",
        card_name: "BedPlatesCard",
        toggle_property: "BedPlatesEnabled",
        availability_label: "",
        synonyms: &["beds", "bed defence"],
    },
    GuiRow {
        id: "fireballwarning",
        tab: "Render",
        card_name: "FireballWarningCard",
        toggle_property: "FireballWarningEnabled",
        availability_label: "",
        synonyms: &["fireball", "warning"],
    },
    GuiRow {
        id: "closestplayer",
        tab: "Render",
        card_name: "ClosestPlayerCard",
        toggle_property: "ClosestPlayerInfoEnabled",
        availability_label: "",
        synonyms: &["closest", "nearest", "player"],
    },
    GuiRow {
        id: "antidebuff",
        tab: "Render",
        card_name: "AntiDebuffCard",
        toggle_property: "AntiDebuffEnabled",
        availability_label: "AntiDebuffAvailabilityText",
        synonyms: &["debuff", "blindness"],
    },
    GuiRow {
        id: "hudeditor",
        tab: "Render",
        card_name: "HudEditorCard",
        toggle_property: "HudEditorActive",
        availability_label: "",
        synonyms: &["hud", "layout", "elements"],
    },
    GuiRow {
        id: "speedbridge",
        tab: "Utility",
        card_name: "SpeedBridgeCard",
        toggle_property: "SpeedBridgeEnabled",
        availability_label: "SpeedBridgeAvailabilityText",
        synonyms: &["bridge", "scaffold"],
    },
    GuiRow {
        id: "gtbhelper",
        tab: "Utility",
        card_name: "GtbHelperCard",
        toggle_property: "GtbHelperEnabled",
        availability_label: "GtbHelperAvailabilityText",
        synonyms: &["gtb", "guess the build"],
    },
    GuiRow {
        id: "pixelpartyassist",
        tab: "Utility",
        card_name: "PixelPartyAssistCard",
        toggle_property: "PixelPartyAssistEnabled",
        availability_label: "PixelPartyAssistAvailabilityText",
        synonyms: &["pixel party", "terracotta"],
    },
    GuiRow {
        id: "cheststealer",
        tab: "Utility",
        card_name: "ChestStealerCard",
        toggle_property: "ChestStealerEnabled",
        availability_label: "ChestStealerAvailabilityText",
        synonyms: &["stealer", "loot", "chest"],
    },
    GuiRow {
        id: "refill",
        tab: "Utility",
        card_name: "RefillCard",
        toggle_property: "RefillEnabled",
        availability_label: "RefillAvailabilityText",
        synonyms: &["refill", "blocks"],
    },
    GuiRow {
        id: "autotool",
        tab: "Utility",
        card_name: "AutoToolCard",
        toggle_property: "AutoToolEnabled",
        availability_label: "AutoToolAvailabilityText",
        synonyms: &["tool", "pickaxe"],
    },
    GuiRow {
        id: "panic",
        tab: "Utility",
        card_name: "PanicCard",
        toggle_property: "",
        availability_label: "",
        synonyms: &["panic", "emergency", "stop"],
    },
    GuiRow {
        id: "reach",
        tab: "Risky",
        card_name: "ReachCard",
        toggle_property: "ReachEnabled",
        availability_label: "ReachAvailabilityText",
        synonyms: &["reach", "range"],
    },
    GuiRow {
        id: "velocity",
        tab: "Risky",
        card_name: "VelocityCard",
        toggle_property: "VelocityEnabled",
        availability_label: "VelocityAvailabilityText",
        synonyms: &["velocity", "knockback", "antikb"],
    },
    GuiRow {
        id: "autototem",
        tab: "Risky",
        card_name: "AutoTotemCard",
        toggle_property: "AutoTotemEnabled",
        availability_label: "AutoTotemAvailabilityText",
        synonyms: &["totem", "pop"],
    },
    GuiRow {
        id: "killaura",
        tab: "Beta",
        card_name: "KillAuraCard",
        toggle_property: "KillAuraEnabled",
        availability_label: "KillAuraAvailabilityText",
        synonyms: &["aura", "ka", "aura kill"],
    },
    GuiRow {
        id: "jumpreset",
        tab: "Beta",
        card_name: "JumpResetCard",
        toggle_property: "JumpResetEnabled",
        availability_label: "JumpResetAvailabilityText",
        synonyms: &["jump", "reset"],
    },
    GuiRow {
        id: "hitselect",
        tab: "Beta",
        card_name: "HitSelectCard",
        toggle_property: "HitSelectEnabled",
        availability_label: "HitSelectAvailabilityText",
        synonyms: &["hit", "select"],
    },
    GuiRow {
        id: "wtap",
        tab: "Beta",
        card_name: "WTapCard",
        toggle_property: "WTapEnabled",
        availability_label: "WTapAvailabilityText",
        synonyms: &["w tap", "tap"],
    },
    GuiRow {
        id: "trajectories",
        tab: "Beta",
        card_name: "TrajectoriesCard",
        toggle_property: "TrajectoriesEnabled",
        availability_label: "TrajectoriesAvailabilityText",
        synonyms: &["trajectory", "arrow"],
    },
    GuiRow {
        id: "murdermystery",
        tab: "Beta",
        card_name: "MurderMysteryCard",
        toggle_property: "MurderMysteryEnabled",
        availability_label: "MurderMysteryAvailabilityText",
        synonyms: &["mm", "murder"],
    },
    GuiRow {
        id: "breakprogress",
        tab: "Beta",
        card_name: "BreakProgressCard",
        toggle_property: "BreakProgressEnabled",
        availability_label: "BreakProgressAvailabilityText",
        synonyms: &["break", "progress"],
    },
    GuiRow {
        id: "nohurtcam",
        tab: "Beta",
        card_name: "NoHurtCamCard",
        toggle_property: "NoHurtCamEnabled",
        availability_label: "NoHurtCamAvailabilityText",
        synonyms: &["hurt", "camera", "shake"],
    },
    GuiRow {
        id: "tnttimer",
        tab: "Beta",
        card_name: "TntTimerCard",
        toggle_property: "TntTimerEnabled",
        availability_label: "TntTimerAvailabilityText",
        synonyms: &["tnt", "fuse"],
    },
    GuiRow {
        id: "fastplace",
        tab: "Beta",
        card_name: "FastPlaceCard",
        toggle_property: "FastPlaceEnabled",
        availability_label: "FastPlaceAvailabilityText",
        synonyms: &["place", "block"],
    },
    GuiRow {
        id: "sprint",
        tab: "Beta",
        card_name: "SprintCard",
        toggle_property: "SprintEnabled",
        availability_label: "SprintAvailabilityText",
        synonyms: &["sprint", "speed"],
    },
];

struct DestinationRow {
    id: &'static str,
    display_name: &'static str,
    tab: &'static str,
    synonyms: &'static [&'static str],
}

const DESTINATION_ROWS: &[DestinationRow] = &[
    DestinationRow {
        id: "accounts",
        display_name: "Accounts",
        tab: "Accounts",
        synonyms: &[
            "account", "switch", "login", "cookies", "localts", "cracked", "session",
        ],
    },
    DestinationRow {
        id: "servers",
        display_name: "Servers",
        tab: "Servers",
        synonyms: &["server", "join", "connect", "direct connect", "ping"],
    },
    DestinationRow {
        id: "settings",
        display_name: "Settings",
        tab: "Settings",
        synonyms: &["preferences", "theme", "palette", "configs"],
    },
    DestinationRow {
        id: "stats",
        display_name: "Stats",
        tab: "Stats",
        synonyms: &["statistics", "session", "kills"],
    },
];

/// Builds the default entry table.
///
/// Catalog order first, GUI rows only, then the tab destinations. Built once and
/// leaked into a `&'static` slice: the table is immutable program data, and every
/// hit carries a `&'static` entry so the overlay can hold rows without cloning.
fn build_entries() -> &'static [SearchEntry] {
    let mut entries = Vec::with_capacity(GUI_ROWS.len() + DESTINATION_ROWS.len());
    let mut order = 0;

    for row in GUI_ROWS {
        let Some(entry) = CATALOG.iter().find(|entry| entry.id == row.id) else {
            // A GUI row whose catalog entry disappeared is a build-time mistake;
            // skipping keeps the index usable instead of panicking at startup.
            tracing::warn!(module_id = row.id, "search row without a catalog entry");
            continue;
        };

        entries.push(SearchEntry {
            id: entry.id,
            display_name: entry.display_name,
            tab: row.tab,
            group: row.tab,
            card_name: row.card_name,
            toggle_property: row.toggle_property,
            availability_label: row.availability_label,
            synonyms: row.synonyms,
            has_keybind: entry.requires(Surfaces::KEYBIND_GUI),
            dev_only: entry.dev_only,
            is_destination: false,
            order,
        });
        order += 1;
    }

    for row in DESTINATION_ROWS {
        entries.push(SearchEntry {
            id: row.id,
            display_name: row.display_name,
            tab: row.tab,
            group: "Tabs",
            card_name: "",
            toggle_property: "",
            availability_label: "",
            synonyms: row.synonyms,
            has_keybind: false,
            dev_only: false,
            is_destination: true,
            order,
        });
        order += 1;
    }

    Box::leak(entries.into_boxed_slice())
}

/// The process-wide index.
///
/// `OnceLock` rather than a lazy global so a test can build a second, smaller index
/// without touching the default one.
#[must_use]
pub fn entries() -> &'static [SearchEntry] {
    static ENTRIES: std::sync::OnceLock<&'static [SearchEntry]> = std::sync::OnceLock::new();
    ENTRIES.get_or_init(build_entries)
}

fn normalize(query: &str) -> String {
    query.trim().to_lowercase()
}

/// Ranks every entry against `query`, best first.
///
/// An empty or whitespace-only query returns every entry in catalogue order at
/// [`RANK_EXACT_PREFIX`], which is what makes the overlay's initial list stable.
#[must_use]
pub fn rank(query: &str) -> Vec<SearchHit> {
    let normalized = normalize(query);

    if normalized.is_empty() {
        return entries()
            .iter()
            .map(|entry| SearchHit {
                entry,
                rank: RANK_EXACT_PREFIX,
                name_ranges: Vec::new(),
                matched_alias: None,
            })
            .collect();
    }

    let mut hits: Vec<SearchHit> = entries()
        .iter()
        .filter_map(|entry| try_match(entry, &normalized))
        .collect();

    hits.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then_with(|| left.entry.order.cmp(&right.entry.order))
    });
    hits
}

/// Rows the overlay displays: rank order, minus what the GUI hides for this mode.
///
/// Nothing is regrouped or re-sorted, so the first row is the best match and
/// therefore the row the overlay highlights and the row Enter continues on.
#[must_use]
pub fn rows(query: &str, is_visible: impl Fn(&SearchEntry) -> bool) -> Vec<SearchHit> {
    rank(query)
        .into_iter()
        .filter(|hit| is_visible(hit.entry))
        .collect()
}

fn try_match(entry: &'static SearchEntry, query: &str) -> Option<SearchHit> {
    let name = entry.display_name.to_lowercase();

    if name.starts_with(query) {
        return Some(hit(
            entry,
            RANK_EXACT_PREFIX,
            vec![SearchRange::new(0, query.len())],
        ));
    }

    if let Some(word_start) = find_word_start(&name, query) {
        return Some(hit(
            entry,
            RANK_WORD_PREFIX,
            vec![SearchRange::new(word_start, query.len())],
        ));
    }

    if let Some(substring) = name.find(query) {
        return Some(hit(
            entry,
            RANK_SUBSTRING,
            vec![SearchRange::new(substring, query.len())],
        ));
    }

    let compact_name: String = name.replace(' ', "");
    let compact_query: String = query.replace(' ', "");
    if !compact_name.is_empty() && compact_name.contains(&compact_query) {
        return Some(SearchHit {
            entry,
            rank: RANK_COMPACT_SUBSTRING,
            name_ranges: Vec::new(),
            matched_alias: Some(Cow::Owned(compact_name)),
        });
    }

    for alias in aliases(entry) {
        if starts_with_ignore_case(alias, query) {
            return Some(alias_hit(entry, RANK_ALIAS_PREFIX, alias));
        }
    }
    for alias in aliases(entry) {
        if contains_ignore_case(alias, query) {
            return Some(alias_hit(entry, RANK_ALIAS_SUBSTRING, alias));
        }
    }

    if let Some(ranges) = subsequence_ranges(&name, query) {
        return Some(hit(entry, RANK_SUBSEQUENCE, ranges));
    }

    for alias in aliases(entry) {
        if subsequence_ranges(alias, query).is_some() {
            return Some(alias_hit(entry, RANK_ALIAS_SUBSEQUENCE, alias));
        }
    }

    None
}

fn hit(entry: &'static SearchEntry, rank: i32, name_ranges: Vec<SearchRange>) -> SearchHit {
    SearchHit {
        entry,
        rank,
        name_ranges,
        matched_alias: None,
    }
}

fn alias_hit(entry: &'static SearchEntry, rank: i32, matched: &'static str) -> SearchHit {
    SearchHit {
        entry,
        rank,
        name_ranges: Vec::new(),
        matched_alias: Some(Cow::Borrowed(matched)),
    }
}

/// ASCII case-insensitive `starts_with`, so aliases can stay `&'static str`.
fn starts_with_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.len() >= needle.len()
        && haystack.as_bytes()[..needle.len()].eq_ignore_ascii_case(needle.as_bytes())
}

/// ASCII case-insensitive `contains`.
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// The alias strings a row matches on: id, tab, then every synonym.
///
/// Returned as `&'static str` (no allocation) and compared case-insensitively,
/// because catalog ids are lower-case but tab names are not.
fn aliases(entry: &'static SearchEntry) -> impl Iterator<Item = &'static str> {
    std::iter::once(entry.id)
        .chain(std::iter::once(entry.tab))
        .chain(entry.synonyms.iter().copied())
}

fn find_word_start(name: &str, query: &str) -> Option<usize> {
    let bytes = name.as_bytes();
    let query_len = query.len();
    let mut index = 1;
    while index + query_len <= name.len() {
        if bytes[index - 1] == b' ' && name[index..].starts_with(query) {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// Merged subsequence ranges, exactly as the C# `TrySubsequence` builds them: a
/// character adjacent to the previous run extends that run instead of starting a
/// new one.
fn subsequence_ranges(haystack: &str, needle: &str) -> Option<Vec<SearchRange>> {
    let mut found: Vec<SearchRange> = Vec::with_capacity(needle.len());
    let mut cursor = 0;

    for character in needle.chars() {
        let index = if cursor < haystack.len() {
            haystack[cursor..]
                .find(character)
                .map(|offset| cursor + offset)
        } else {
            None
        };
        let index = index?;

        if let Some(last) = found.last_mut() {
            if last.start + last.length == index {
                last.length += 1;
                cursor = index + character.len_utf8();
                continue;
            }
        }

        found.push(SearchRange::new(index, character.len_utf8()));
        cursor = index + character.len_utf8();
    }

    Some(found)
}

/// What the window knows about the module behind one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowState {
    /// Whether the row is shown in the current mode.
    pub is_visible: bool,
    /// Whether the bridge can run the module right now.
    pub is_available: bool,
    /// Why it cannot, when it cannot.
    pub unavailable_reason: &'static str,
    /// Whether the switch reads on.
    pub is_on: bool,
    /// Whether the row offers a switch at all.
    pub has_toggle: bool,
    /// Keybind text for the row.
    pub keybind_text: String,
}

/// `CommandSearchRowPolicy.Resolve`: what a row may show.
///
/// A row only offers a switch the toggle path would honour, and never reports a
/// state the module is not in: an unsupported module reads off even when the stored
/// flag is set.
#[must_use]
pub fn resolve_row(
    entry: &SearchEntry,
    dev_mode: bool,
    supported: bool,
    is_on: bool,
    keybind_text: impl Into<String>,
) -> RowState {
    let is_visible = !entry.dev_only || dev_mode;
    // A destination is a tab, not a module the bridge advertises.
    let is_available = is_visible && (entry.is_destination || supported);
    let has_toggle = !entry.is_destination && !entry.toggle_property.is_empty();

    RowState {
        is_visible,
        is_available,
        unavailable_reason: if is_available {
            ""
        } else {
            unavailable_reason(entry.id)
        },
        is_on: has_toggle && is_available && is_on,
        has_toggle,
        keybind_text: keybind_text.into(),
    }
}

/// `CommandSearchRowPolicy.UnavailableReason`.
#[must_use]
pub fn unavailable_reason(module_id: &str) -> &'static str {
    if module_id.eq_ignore_ascii_case("triggerbot") {
        "Unavailable on 1.8.9 (cooldown-era PvP only)"
    } else {
        "Unavailable on current bridge"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id_of(hit: &SearchHit) -> &'static str {
        hit.entry.id
    }

    #[test]
    fn an_empty_query_returns_every_entry_in_catalog_order() {
        let hits = rank("   ");
        assert_eq!(hits.len(), entries().len());

        let mut sorted: Vec<_> = entries().iter().collect();
        sorted.sort_by_key(|entry| entry.order);
        assert_eq!(
            hits.iter().map(id_of).collect::<Vec<_>>(),
            sorted.iter().map(|entry| entry.id).collect::<Vec<_>>()
        );
        assert!(hits.iter().all(|hit| hit.rank == RANK_EXACT_PREFIX));
    }

    #[test]
    fn a_query_with_no_match_returns_nothing() {
        assert!(rank("zzzzz").is_empty());
    }

    #[test]
    fn prefix_beats_word_prefix_beats_substring() {
        let hits = rank("esp");
        assert_eq!(id_of(&hits[0]), "chestesp");
        assert_eq!(id_of(&hits[1]), "blockesp");
        assert_eq!(hits[0].rank, RANK_WORD_PREFIX);
        assert_eq!(hits[1].rank, RANK_WORD_PREFIX);

        // "tnt" is a display-name prefix of TNT Timer.
        let tnt = rank("tnt");
        assert_eq!(id_of(&tnt[0]), "tnttimer");
        assert_eq!(tnt[0].rank, RANK_EXACT_PREFIX);
    }

    #[test]
    fn word_prefix_beats_subsequence() {
        let hits = rank("aura");
        assert_eq!(id_of(&hits[0]), "killaura");
        assert_eq!(hits[0].rank, RANK_WORD_PREFIX);
        assert!(hits.iter().all(|hit| hit.rank >= RANK_WORD_PREFIX));
    }

    #[test]
    fn a_module_id_alias_matches_without_name_ranges() {
        let hits = rank("ka");
        assert_eq!(id_of(&hits[0]), "killaura");
        assert_eq!(hits[0].rank, RANK_ALIAS_PREFIX);
        assert!(hits[0].name_ranges.is_empty());
        assert_eq!(hits[0].matched_alias.as_deref(), Some("ka"));
    }

    #[test]
    fn a_compact_name_match_is_explained_by_the_alias() {
        let hits = rank("hitselect");
        let hit = &hits[0];
        assert_eq!(id_of(hit), "hitselect");
        // "hitselect" is the compacted display name "Hit Select", which ranks
        // above an alias match (`ModuleSearchIndex.cs:286-292`).
        assert_eq!(hit.rank, RANK_COMPACT_SUBSTRING);
        assert_eq!(hit.matched_alias.as_deref(), Some("hitselect"));
    }

    #[test]
    fn a_synonym_match_carries_the_alias() {
        let rows = super::rows("cps", |_| true);
        let hit = &rows[0];
        assert_eq!(id_of(hit), "autoclicker");
        assert!(hit.entry.synonyms.contains(&"cps"));
        assert_eq!(hit.matched_alias.as_deref(), Some("cps"));
        assert!(hit.name_ranges.is_empty());

        // A display-name match explains itself through ranges instead.
        let esp = rank("esp");
        let chest = esp
            .iter()
            .find(|hit| hit.entry.id == "chestesp")
            .expect("chestesp is ranked");
        assert_eq!(chest.matched_alias, None);
    }

    #[test]
    fn rows_keep_rank_order_and_drop_hidden_entries() {
        let cases = [
            ("hit", false, "hitdelayfix"),
            ("ka", true, "killaura"),
            ("esp", false, "chestesp"),
            ("auto", false, "autoclicker"),
        ];

        for (query, dev_mode, expected) in cases {
            let hits = rank(query);
            let visible = super::rows(query, |entry| !entry.dev_only || dev_mode);
            assert!(!visible.is_empty(), "{query}");
            assert_eq!(id_of(&visible[0]), expected, "{query}");
            assert_eq!(
                visible[0].entry.id, hits[0].entry.id,
                "the first row is the top hit for {query}"
            );
            let expected_ids: Vec<_> = hits
                .iter()
                .filter(|hit| !hit.entry.dev_only || dev_mode)
                .map(id_of)
                .collect();
            assert_eq!(visible.iter().map(id_of).collect::<Vec<_>>(), expected_ids);
        }
    }

    #[test]
    fn hidden_rows_are_dropped_without_reordering() {
        let hits = rank("ka");
        assert_eq!(id_of(&hits[0]), "killaura");
        assert!(hits[0].entry.dev_only);

        let visible = super::rows("ka", |entry| !entry.dev_only);
        assert!(visible.iter().all(|hit| hit.entry.id != "killaura"));
        assert_eq!(
            visible.iter().map(id_of).collect::<Vec<_>>(),
            hits.iter().skip(1).map(id_of).collect::<Vec<_>>()
        );
    }

    #[test]
    fn rows_are_sorted_for_every_query() {
        for query in ["a", "s", "hit", "ka", "esp", "auto", "e", "r"] {
            let rows = super::rows(query, |_| true);
            for pair in rows.windows(2) {
                let (left, right) = (&pair[0], &pair[1]);
                assert!(
                    left.rank < right.rank
                        || (left.rank == right.rank && left.entry.order < right.entry.order),
                    "rows for {query:?} are out of order"
                );
            }
        }
    }

    #[test]
    fn curated_synonyms_match() {
        assert_eq!(id_of(&rank("cps")[0]), "autoclicker");
        assert_eq!(id_of(&rank("clicker")[0]), "autoclicker");
        let pickaxe = rank("pickaxe");
        assert_eq!(pickaxe[0].entry.synonyms.last().copied(), Some("pickaxe"));
    }

    #[test]
    fn tab_names_match() {
        let hits = rank("render");
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|hit| hit.entry.tab == "Render"));
    }

    #[test]
    fn a_subsequence_match_reports_merged_ranges() {
        let hits = rank("ntgs");
        let hit = hits
            .iter()
            .find(|hit| hit.entry.id == "nametags")
            .expect("nametags matches as a subsequence");
        assert_eq!(hit.rank, RANK_SUBSEQUENCE);
        assert_eq!(
            hit.name_ranges,
            vec![
                SearchRange::new(0, 1),
                SearchRange::new(4, 1),
                SearchRange::new(6, 2)
            ]
        );
    }

    #[test]
    fn a_display_name_match_reports_its_range() {
        let hits = rank("esp");
        let chest = hits
            .iter()
            .find(|hit| hit.entry.id == "chestesp")
            .expect("chestesp is ranked");
        assert_eq!(chest.name_ranges, vec![SearchRange::new(6, 3)]);
    }

    #[test]
    fn ranking_is_deterministic_and_sorted() {
        let first = rank("s");
        let second = rank("s");
        assert_eq!(
            first.iter().map(id_of).collect::<Vec<_>>(),
            second.iter().map(id_of).collect::<Vec<_>>()
        );

        for pair in first.windows(2) {
            let (left, right) = (&pair[0], &pair[1]);
            assert!(
                left.rank < right.rank
                    || (left.rank == right.rank && left.entry.order < right.entry.order)
            );
        }
    }

    #[test]
    fn every_entry_has_a_unique_id_and_a_known_group() {
        let mut ids: Vec<&str> = entries().iter().map(|entry| entry.id).collect();
        ids.sort_unstable();
        let unique = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), unique, "duplicate search ids");

        for entry in entries() {
            assert!(
                GROUP_ORDER.contains(&entry.group),
                "{} has an unknown group {}",
                entry.id,
                entry.group
            );
            if !entry.is_destination {
                assert_eq!(entry.group, entry.tab);
            }
        }
    }

    #[test]
    fn every_gui_module_is_searchable() {
        let gui_modules: Vec<_> = CATALOG
            .iter()
            .filter(|entry| entry.requires(Surfaces::KEYBIND_GUI) || entry.id == "hudeditor")
            .collect();

        for module in gui_modules {
            let row = entries()
                .iter()
                .find(|entry| entry.id == module.id)
                .unwrap_or_else(|| panic!("{} has no search row", module.id));
            assert_eq!(row.display_name, module.display_name);
            assert!(!row.tab.is_empty());
            assert!(!row.card_name.is_empty());
            if module.id == "panic" {
                assert!(row.toggle_property.is_empty());
                continue;
            }
            assert!(
                !row.toggle_property.is_empty(),
                "{} has no toggle property",
                module.id
            );
        }
    }

    #[test]
    fn destinations_point_at_tabs() {
        let destinations: Vec<_> = entries()
            .iter()
            .filter(|entry| entry.is_destination)
            .collect();
        assert!(!destinations.is_empty());
        for destination in destinations {
            assert_eq!(destination.group, "Tabs");
            assert!(destination.toggle_property.is_empty());
            assert_eq!(destination.tab, destination.display_name);
        }
    }

    #[test]
    fn row_state_matches_the_switch_invariant() {
        let kill_aura = entries()
            .iter()
            .find(|entry| entry.id == "killaura")
            .expect("killaura row");

        // Unsupported: off, unavailable, explained.
        let unsupported = resolve_row(kill_aura, true, false, true, "");
        assert!(unsupported.is_visible);
        assert!(!unsupported.is_available);
        assert!(!unsupported.is_on);
        assert!(unsupported.has_toggle);
        assert_eq!(
            unsupported.unavailable_reason,
            "Unavailable on current bridge"
        );

        // Hidden in non-dev mode, and hidden means unavailable.
        let hidden = resolve_row(kill_aura, false, true, false, "");
        assert!(!hidden.is_visible);
        assert!(!hidden.is_available);

        // Supported and on.
        let on = resolve_row(kill_aura, true, true, true, "R");
        assert!(on.is_available);
        assert!(on.is_on);
        assert_eq!(on.keybind_text, "R");

        // A destination never offers a switch.
        let accounts = entries()
            .iter()
            .find(|entry| entry.id == "accounts")
            .expect("accounts row");
        let destination = resolve_row(accounts, false, false, true, "");
        assert!(
            destination.is_available,
            "destinations ignore the module list"
        );
        assert!(!destination.has_toggle);
        assert!(!destination.is_on);
    }

    #[test]
    fn the_triggerbot_reason_is_specific() {
        assert_eq!(
            unavailable_reason("triggerbot"),
            "Unavailable on 1.8.9 (cooldown-era PvP only)"
        );
        assert_eq!(
            unavailable_reason("Autorod"),
            "Unavailable on current bridge"
        );
    }
}
