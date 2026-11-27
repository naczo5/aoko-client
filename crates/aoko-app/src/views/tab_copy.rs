//! The sentences both data tabs share.
//!
//! Ported from `Aoko/Views/TabCopy.cs`. Two rules carry over unchanged:
//!
//! * The capability refusals live here so the Accounts and Servers tabs cannot
//!   drift apart, and so the wording of a refusal is asserted in one place.
//! * The text the bridge already owns is taken from
//!   [`aoko_protocol::command_result`] instead of being copied, which is what
//!   `TabCopyTests.NotConnected_FollowsTheBridgeMessage` pins.

use aoko_protocol::command_result::{error, failure_message};

/// Tone of a transient status line.
///
/// The managed `TabCopy.BrushKey` maps these onto WPF resource keys
/// (`AccentBrush`, `CriticalBrush`, `DimTextBrush`). GPUI has no resource
/// dictionary, so the mapping is a semantic role and `theme::` resolves the
/// colour: `Critical` is the fixed `#F43F5E` from `Aoko/Views/TabStyles.xaml:10`,
/// which is deliberately outside the ten palette roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Neutral,
    Success,
    Critical,
}

/// Which theme role a status line paints with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusRole {
    /// `DimTextBrush`.
    Dim,
    /// `AccentBrush`.
    Accent,
    /// `CriticalBrush`.
    Critical,
}

/// The sentence a capability refusal shows in both tabs.
pub const SELECT_ACCOUNT_HINT: &str = "Select an account to switch.";

/// Asks for a selection before the join action arms.
pub const SELECT_SERVER_HINT: &str = "Select a server to join.";

/// Shown when the saved-server list is empty.
pub const NO_SERVERS_HINT: &str = "Add a server, then join it from this tab.";

/// The account-switch refusal: an update, not a rebuild.
pub const UNSUPPORTED_ACCOUNT_SWITCH: &str =
    "This aoko build cannot switch accounts. Update aoko, then reconnect.";

/// The server-join refusal, identical but for the action it names.
pub const UNSUPPORTED_SERVER_JOIN: &str =
    "This aoko build cannot join servers. Update aoko, then reconnect.";

/// One sentence fits the footer; the whole text stays in the tooltip.
pub const STATUS_LIMIT: usize = 110;

/// `TabCopy.NotConnected`, built from the bridge's own wording.
#[must_use]
pub fn not_connected() -> String {
    failure_message(error::NOT_CONNECTED)
}

/// `TabCopy.ReturnToMainMenu`, the one "return to the main menu" sentence.
#[must_use]
pub fn return_to_main_menu() -> String {
    failure_message("in_world")
}

/// `TabCopy.BrushKey` as a role.
#[must_use]
pub const fn status_role(tone: StatusTone) -> StatusRole {
    match tone {
        StatusTone::Success => StatusRole::Accent,
        StatusTone::Critical => StatusRole::Critical,
        StatusTone::Neutral => StatusRole::Dim,
    }
}

/// Cuts a long status sentence at a word boundary; the tooltip keeps the rest.
///
/// `STATUS_LIMIT` counts characters, not bytes: the managed limit is a UTF-16
/// code-unit budget and every character the app writes into a status line is in
/// the basic plane, so a character count is the same number without any chance of
/// slicing a multi-byte character in half.
#[must_use]
pub fn shorten(text: Option<&str>) -> String {
    let value = text.unwrap_or("").trim();
    let characters: Vec<char> = value.chars().collect();
    if characters.len() <= STATUS_LIMIT {
        return value.to_owned();
    }

    // `LastIndexOf(' ', StatusLimit)`, with the managed guard that refuses a cut
    // in the first half of the budget.
    let mut cut = STATUS_LIMIT;
    for (index, character) in characters.iter().enumerate().take(STATUS_LIMIT) {
        if *character == ' ' && index >= STATUS_LIMIT / 2 {
            cut = index;
        }
    }

    let mut shortened: String = characters[..cut].iter().collect();
    let trimmed = shortened.trim_end().len();
    shortened.truncate(trimmed);
    shortened.push('\u{2026}');
    shortened
}

#[cfg(test)]
mod tests {
    use super::*;
    use aoko_protocol::capabilities::actions;
    use aoko_protocol::command_result::BridgeCommandResult;

    #[test]
    fn not_connected_follows_the_bridge_message() {
        assert_eq!(
            not_connected(),
            BridgeCommandResult::failure(Some(actions::SET_SESSION), Some(error::NOT_CONNECTED))
                .message()
        );
    }

    #[test]
    fn return_to_main_menu_follows_the_bridge_message() {
        assert_eq!(
            return_to_main_menu(),
            BridgeCommandResult::failure(Some(actions::SET_SESSION), Some("in_world")).message()
        );
    }

    #[test]
    fn capacity_refusals_ask_for_an_update_not_for_a_compiler() {
        for message in [UNSUPPORTED_ACCOUNT_SWITCH, UNSUPPORTED_SERVER_JOIN] {
            assert!(message.contains("Update aoko, then reconnect."));
            let lowered = message.to_lowercase();
            assert!(!lowered.contains("rebuild"));
            assert!(!lowered.contains("dll"));
            assert!(!lowered.contains("client"));
        }
    }

    #[test]
    fn capacity_refusals_differ_only_by_the_action_they_refuse() {
        assert_eq!(
            UNSUPPORTED_SERVER_JOIN,
            UNSUPPORTED_ACCOUNT_SWITCH.replace("switch accounts", "join servers")
        );
    }

    #[test]
    fn status_tone_maps_to_the_theme_role() {
        assert_eq!(status_role(StatusTone::Neutral), StatusRole::Dim);
        assert_eq!(status_role(StatusTone::Success), StatusRole::Accent);
        assert_eq!(status_role(StatusTone::Critical), StatusRole::Critical);
    }

    #[test]
    fn shorten_keeps_one_sentence_and_cuts_at_a_word() {
        assert_eq!(shorten(None), "");
        assert_eq!(shorten(Some("  Short sentence.  ")), "Short sentence.");

        let long_text = std::iter::repeat_n("word", 60)
            .collect::<Vec<_>>()
            .join(" ");
        let shortened = shorten(Some(&long_text));

        assert!(shortened.chars().count() <= STATUS_LIMIT + 1);
        assert!(shortened.ends_with('\u{2026}'));
        assert!(!shortened.contains("  "));
    }

    #[test]
    fn shorten_never_splits_a_codepoint() {
        // A server name made of two-byte characters, cut in the middle by byte
        // count, must still land on a boundary.
        let text = "\u{00e9}".repeat(200);
        let shortened = shorten(Some(&text));
        assert!(shortened.ends_with('\u{2026}'));
        assert_eq!(shortened.chars().count(), STATUS_LIMIT + 1);
    }
}
