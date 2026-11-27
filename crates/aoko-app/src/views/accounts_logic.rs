//! The Accounts tab's decisions, kept away from any widget.
//!
//! Ported from `Aoko/Views/AccountsViewLogic.cs`. The managed class is `internal
//! static` for exactly this reason: the capability gate, the session strip, the
//! source chips and the "no action is armed until the user picks a card" rule are
//! all testable without WPF, and they are the parts that break.

use aoko_protocol::accounts::sources;

use crate::views::relative_time;
use crate::views::tab_copy;

/// Which session chip the strip shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionChipTone {
    Offline,
    MainMenu,
    InWorld,
}

/// Colour family for an account's source chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountSourceTone {
    Neutral,
    Localts,
    Offline,
}

/// Short, actionable variant of the offline-name rule.
///
/// The en dash is deliberate: the managed constant uses U+2013 and the register
/// form asserts the wording.
pub const INVALID_OFFLINE_NAME: &str = "Use 3\u{2013}16 letters, numbers or underscores.";

/// `AccountsViewLogic.SessionChipToneFor`.
#[must_use]
pub const fn session_chip_tone(connected: bool, in_world: bool) -> SessionChipTone {
    if !connected {
        return SessionChipTone::Offline;
    }

    if in_world {
        SessionChipTone::InWorld
    } else {
        SessionChipTone::MainMenu
    }
}

/// `AccountsViewLogic.CanSwitchSessions`.
#[must_use]
pub const fn can_switch_sessions(connected: bool, supports_set_session: bool) -> bool {
    connected && supports_set_session
}

/// Only an explicit selection plus an idle, capable tab arms the primary action.
#[must_use]
pub const fn can_use_selected_account(
    connected: bool,
    supports_set_session: bool,
    busy: bool,
    has_selection: bool,
) -> bool {
    !busy && has_selection && can_switch_sessions(connected, supports_set_session)
}

/// Index to select after a reload, or `None`.
///
/// No fallback: opening the tab must not arm Use account / Delete for a card the
/// user never chose.
#[must_use]
pub fn selection_index_after_load(ids: &[String], keep_id: Option<&str>) -> Option<usize> {
    let keep_id = keep_id?;
    ids.iter().position(|id| id == keep_id)
}

/// Why a switch or restore cannot start right now, or `None` when it can.
///
/// The in-world case stays clickable so the refusal shows where the user clicked;
/// that is why this returns a message rather than a boolean.
#[must_use]
pub fn session_switch_refusal(
    connected: bool,
    supports_set_session: bool,
    in_world: bool,
) -> Option<String> {
    if !connected {
        return Some(tab_copy::not_connected());
    }

    if !supports_set_session {
        return Some(tab_copy::UNSUPPORTED_ACCOUNT_SWITCH.to_owned());
    }

    if in_world {
        return Some(tab_copy::return_to_main_menu());
    }

    None
}

/// Persistent line beside the primary action while it is disabled.
#[must_use]
pub fn primary_hint(connected: bool, supports_set_session: bool, has_selection: bool) -> String {
    if !connected {
        return tab_copy::not_connected();
    }

    if !supports_set_session {
        return tab_copy::UNSUPPORTED_ACCOUNT_SWITCH.to_owned();
    }

    if has_selection {
        String::new()
    } else {
        tab_copy::SELECT_ACCOUNT_HINT.to_owned()
    }
}

/// The strip's second line: never asks for a button that is disabled.
#[must_use]
pub fn session_caption(connected: bool, supports_session_read: bool) -> &'static str {
    if !connected {
        return "Inject the client to read its session.";
    }

    if supports_session_read {
        "Refresh to read it."
    } else {
        "This aoko build cannot read the session."
    }
}

/// `AccountsViewLogic.SourceChipLabel`: five labels, case-insensitive matching.
#[must_use]
pub fn source_chip_label(source: Option<&str>, has_refresh: bool) -> &'static str {
    if is_source(source, sources::OFFLINE) {
        return "Cracked";
    }

    if has_refresh || is_source(source, sources::LOCALTS) {
        return "Localts";
    }

    if is_source(source, sources::TOKEN) {
        return "Token";
    }

    if is_source(source, sources::COOKIE) {
        return "Cookie";
    }

    "Saved"
}

/// Colour family for the source chip.
#[must_use]
pub fn source_tone(source: Option<&str>, has_refresh: bool) -> AccountSourceTone {
    if is_source(source, sources::OFFLINE) {
        return AccountSourceTone::Offline;
    }

    if has_refresh || is_source(source, sources::LOCALTS) {
        AccountSourceTone::Localts
    } else {
        AccountSourceTone::Neutral
    }
}

/// Whether a saved account is the signed-in one.
///
/// Port of `AccountStore.SameUuid`: both sides must be non-blank, and the
/// comparison ignores dashes and case. `aoko-protocol` does not publish this
/// helper yet, so it lives here rather than being reinvented per call site.
#[must_use]
pub fn is_current_session(account_uuid: Option<&str>, session_uuid: Option<&str>) -> bool {
    let (Some(left), Some(right)) = (account_uuid, session_uuid) else {
        return false;
    };

    if left.trim().is_empty() || right.trim().is_empty() {
        return false;
    }

    let left = left.replace('-', "");
    let right = right.replace('-', "");
    left.eq_ignore_ascii_case(&right)
}

/// Adds the dashes back to a compact UUID; leaves anything else alone.
#[must_use]
pub fn format_uuid(uuid: Option<&str>) -> String {
    let value = uuid.unwrap_or("");
    let compact = value.replace('-', "");
    if compact.chars().count() != 32 || !compact.is_ascii() {
        return value.to_owned();
    }

    format!(
        "{}-{}-{}-{}-{}",
        &compact[0..8],
        &compact[8..12],
        &compact[12..16],
        &compact[16..20],
        &compact[20..32]
    )
}

/// `Last used {relative}`, or `Never used`.
#[must_use]
pub fn last_used_label(last_used_iso: Option<&str>, now_ms: i64) -> String {
    match relative_time::format_stored(last_used_iso, now_ms) {
        Some(relative) => format!("Last used {relative}"),
        None => "Never used".to_owned(),
    }
}

/// What a screen reader says for one card.
#[must_use]
pub fn card_automation_name(name: &str, source_label: &str, is_current: bool) -> String {
    let value = format!("{name}, {source_label}");
    if is_current {
        format!("{value}, in use")
    } else {
        value
    }
}

/// Case-insensitive source comparison against a stored value.
fn is_source(source: Option<&str>, expected: &str) -> bool {
    source.unwrap_or("").eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_700_000_000_000;

    #[test]
    fn session_chip_tracks_the_bridge_state() {
        assert_eq!(
            session_chip_tone(false, true),
            SessionChipTone::Offline,
            "no connection outranks the world state"
        );
        assert_eq!(session_chip_tone(true, false), SessionChipTone::MainMenu);
        assert_eq!(session_chip_tone(true, true), SessionChipTone::InWorld);
    }

    #[test]
    fn the_primary_action_needs_every_gate() {
        assert!(can_use_selected_account(true, true, false, true));
        assert!(!can_use_selected_account(true, true, true, true), "busy");
        assert!(
            !can_use_selected_account(true, true, false, false),
            "no selection"
        );
        assert!(
            !can_use_selected_account(false, true, false, true),
            "disconnected"
        );
        assert!(
            !can_use_selected_account(true, false, false, true),
            "unsupported"
        );
    }

    #[test]
    fn selection_after_load_never_falls_back_to_the_first_card() {
        let ids = vec!["one".to_owned(), "two".to_owned(), "three".to_owned()];

        assert_eq!(selection_index_after_load(&ids, None), None);
        assert_eq!(selection_index_after_load(&ids, Some("gone")), None);
        assert_eq!(selection_index_after_load(&ids, Some("three")), Some(2));
        assert_eq!(selection_index_after_load(&[], Some("one")), None);
    }

    #[test]
    fn switch_refusal_names_the_first_blocker() {
        assert_eq!(
            session_switch_refusal(false, false, true).as_deref(),
            Some(tab_copy::not_connected().as_str())
        );
        assert_eq!(
            session_switch_refusal(true, false, true).as_deref(),
            Some(tab_copy::UNSUPPORTED_ACCOUNT_SWITCH)
        );
        assert_eq!(
            session_switch_refusal(true, true, true).as_deref(),
            Some(tab_copy::return_to_main_menu().as_str())
        );
        assert_eq!(session_switch_refusal(true, true, false), None);
    }

    #[test]
    fn primary_hint_explains_a_disabled_primary_action() {
        assert!(primary_hint(false, true, true).contains("not connected"));
        assert_eq!(
            primary_hint(true, false, true),
            tab_copy::UNSUPPORTED_ACCOUNT_SWITCH
        );
        assert_eq!(
            primary_hint(true, true, false),
            tab_copy::SELECT_ACCOUNT_HINT
        );
        assert_eq!(primary_hint(true, true, true), "");
    }

    #[test]
    fn session_caption_never_asks_for_a_disabled_refresh() {
        assert!(session_caption(false, true).contains("Inject the client"));
        assert!(session_caption(true, false).contains("cannot read"));
        assert_eq!(session_caption(true, true), "Refresh to read it.");
    }

    #[test]
    fn source_chip_labels_match_the_managed_table() {
        assert_eq!(source_chip_label(Some(sources::OFFLINE), false), "Cracked");
        assert_eq!(source_chip_label(Some(sources::LOCALTS), true), "Localts");
        assert_eq!(source_chip_label(Some(sources::LOCALTS), false), "Localts");
        assert_eq!(source_chip_label(Some(sources::TOKEN), true), "Localts");
        assert_eq!(source_chip_label(Some(sources::TOKEN), false), "Token");
        assert_eq!(source_chip_label(Some(sources::COOKIE), false), "Cookie");
        assert_eq!(source_chip_label(Some(sources::UNKNOWN), false), "Saved");
        assert_eq!(source_chip_label(None, false), "Saved");
    }

    #[test]
    fn source_chip_labels_ignore_case() {
        assert_eq!(source_chip_label(Some("LOCALTS"), false), "Localts");
        assert_eq!(source_chip_label(Some("Offline"), true), "Cracked");
        assert_eq!(source_chip_label(Some("Cookie"), false), "Cookie");
    }

    #[test]
    fn source_tone_follows_the_chip() {
        assert_eq!(
            source_tone(Some(sources::OFFLINE), true),
            AccountSourceTone::Offline
        );
        assert_eq!(
            source_tone(Some(sources::TOKEN), true),
            AccountSourceTone::Localts
        );
        assert_eq!(
            source_tone(Some(sources::TOKEN), false),
            AccountSourceTone::Neutral
        );
    }

    #[test]
    fn uuid_formatting_adds_dashes_only_for_compact_ids() {
        assert_eq!(
            format_uuid(Some("b50ad385829d3141a2167e7d7539ba7f")),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert_eq!(
            format_uuid(Some("b50ad385-829d-3141-a216-7e7d7539ba7f")),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert_eq!(format_uuid(None), "");
        assert_eq!(format_uuid(Some("offline")), "offline");
    }

    #[test]
    fn current_session_ignores_dashes_and_case() {
        assert!(is_current_session(
            Some("B50AD385829D3141A2167E7D7539BA7F"),
            Some("b50ad385-829d-3141-a216-7e7d7539ba7f")
        ));
        assert!(!is_current_session(
            Some(""),
            Some("b50ad385829d3141a2167e7d7539ba7f")
        ));
        assert!(!is_current_session(
            None,
            Some("b50ad385829d3141a2167e7d7539ba7f")
        ));
        assert!(!is_current_session(Some("aaaa"), Some("bbbb")));
    }

    #[test]
    fn last_used_labels_are_sentence_case() {
        assert_eq!(last_used_label(None, NOW_MS), "Never used");
        assert_eq!(
            last_used_label(Some("2023-11-11T22:13:20Z"), NOW_MS),
            "Last used 3d ago"
        );
        assert_eq!(last_used_label(Some("garbage"), NOW_MS), "Never used");
    }

    #[test]
    fn card_automation_name_adds_in_use_and_keeps_the_source() {
        assert_eq!(
            card_automation_name("Steve", "Localts", true),
            "Steve, Localts, in use"
        );
        assert_eq!(
            card_automation_name("Steve", "Cookie", false),
            "Steve, Cookie"
        );
    }

    #[test]
    fn the_offline_name_rule_keeps_its_en_dash() {
        assert_eq!(
            INVALID_OFFLINE_NAME,
            "Use 3\u{2013}16 letters, numbers or underscores."
        );
    }
}
