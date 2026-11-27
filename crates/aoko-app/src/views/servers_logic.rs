//! Pure decisions the Servers tab makes: ordering, selection retention, status
//! wording and tone, and the two independent gates.
//!
//! Ported from `Aoko/Views/ServersViewLogic.cs`.
//!
//! One structural difference from the managed source, and it is deliberate: the
//! saved-server record and the ping result are not ported yet (`Servers/*Store/Join*`
//! belongs to a later `aoko-core` wave), so this module works against
//! [`ServerRowSource`] and [`PingOutcome`] — the fields the tab actually paints.
//! The store then maps its own record into them and nothing here has to change.
//!
//! Pings never take the exclusive lock, so they only wait on other pings; joining
//! and editing the store are the only exclusive operations. That asymmetry is the
//! reason there are two gates rather than one.

use crate::views::relative_time;
use crate::views::tab_copy;

/// Colour family for a server card's latency chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerStatusTone {
    Neutral,
    Accent,
    Warn,
    Critical,
}

/// Whether this view can hand a join to the bridge right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ServerJoinState {
    Ready,
    /// The bridge is offline, so nothing can be joined.
    #[default]
    Disconnected,
    Unsupported,
}

/// Longest MOTD kept in the model; the card also trims to one line.
pub const MAXIMUM_MOTD_LENGTH: usize = 160;

/// Asks for a selection before a saved-server action arms.
pub const EMPTY_SELECTION_MESSAGE: &str = "Select a saved server first.";

/// The chip for text that cannot be parsed as an address.
pub const INVALID_ADDRESS_CHIP_LABEL: &str = "Invalid address";

/// `SavedServerRow.NotPingedLabel`.
pub const NOT_PINGED_LABEL: &str = "Not pinged";

/// `SavedServerRow.PingingLabel`; single ellipsis character, no three dots.
pub const PINGING_LABEL: &str = "Pinging\u{2026}";

/// The fields ordering and labels need from a saved server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRowSource {
    /// Local key; never shown.
    pub id: String,
    /// Saved name, or the address when the name is blank.
    pub name: String,
    /// Address as the user typed it.
    pub address: String,
    /// `addedAt`, ISO-8601 text.
    pub added_at: Option<String>,
    /// `lastJoinedAt`, ISO-8601 text.
    pub last_joined_at: Option<String>,
}

/// What a ping told the card.
///
/// The subset of the managed `ServerPingResult` the row paints: the online flag,
/// the latency the chip shows, the players label, the MOTD and the free-text
/// error the chip reduces to three labels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PingOutcome {
    pub online: bool,
    pub latency_ms: i32,
    /// Already formatted (`12/100`), empty when the ping had no player counts.
    pub players_label: String,
    /// Raw MOTD text from the ping, trimmed by [`motd_text`] before display.
    pub description: Option<String>,
    /// Raw failure text; reduced by [`error_label`] before display.
    pub error: String,
}

/// Most recently joined first, then newest addition, then a stable tiebreak.
///
/// The managed version compares `DateTimeOffset`s; here the stamps are the stored
/// ISO text, so they are parsed once and ordered by instant. An unparsable stamp
/// sorts last rather than first, which is what keeps a hand-edited file from
/// jumping to the top of the list.
#[must_use]
pub fn sort_for_display(rows: Vec<ServerRowSource>) -> Vec<ServerRowSource> {
    let mut rows = rows;
    rows.sort_by(|left, right| {
        let left_joined = joined_sort_key(left.last_joined_at.as_deref());
        let right_joined = joined_sort_key(right.last_joined_at.as_deref());

        right_joined
            .cmp(&left_joined)
            .then_with(|| {
                added_sort_key(right.added_at.as_deref())
                    .cmp(&added_sort_key(left.added_at.as_deref()))
            })
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });

    rows
}

/// Never-joined sorts below every real timestamp.
fn joined_sort_key(stamp: Option<&str>) -> Option<i64> {
    stamp.and_then(relative_time::parse_iso8601_utc_millis)
}

/// `AddedAt` is always present in practice; a missing one sorts below a real one.
fn added_sort_key(stamp: Option<&str>) -> i64 {
    stamp
        .and_then(relative_time::parse_iso8601_utc_millis)
        .unwrap_or(i64::MIN)
}

/// Index to keep selected after a reload.
///
/// The previous id wins; otherwise the caller's index survives, clamped, so
/// deleting a row lands on its neighbour. `None` when there are no rows.
#[must_use]
pub fn resolve_selection_index(
    ids: &[String],
    selected_id: Option<&str>,
    fallback_index: Option<usize>,
) -> Option<usize> {
    if ids.is_empty() {
        return None;
    }

    if let Some(selected_id) = selected_id.filter(|value| !value.is_empty()) {
        if let Some(index) = ids.iter().position(|id| id == selected_id) {
            return Some(index);
        }
    }

    Some(fallback_index.unwrap_or(0).min(ids.len() - 1))
}

/// `ServersViewLogic.EvaluateJoinState`.
#[must_use]
pub const fn evaluate_join_state(is_connected: bool, supports_join: bool) -> ServerJoinState {
    if !is_connected {
        return ServerJoinState::Disconnected;
    }

    if supports_join {
        ServerJoinState::Ready
    } else {
        ServerJoinState::Unsupported
    }
}

/// Why the join action is blocked, or an empty string when it is not.
#[must_use]
pub fn join_blocked_message(state: ServerJoinState) -> String {
    match state {
        ServerJoinState::Unsupported => tab_copy::UNSUPPORTED_SERVER_JOIN.to_owned(),
        ServerJoinState::Disconnected => tab_copy::not_connected(),
        ServerJoinState::Ready => String::new(),
    }
}

/// Persistent line beside the primary action while it is disabled.
#[must_use]
pub fn primary_hint(state: ServerJoinState, has_rows: bool, has_selection: bool) -> String {
    let blocked = join_blocked_message(state);
    if !blocked.is_empty() {
        return blocked;
    }

    if !has_rows {
        return tab_copy::NO_SERVERS_HINT.to_owned();
    }

    if has_selection {
        String::new()
    } else {
        tab_copy::SELECT_SERVER_HINT.to_owned()
    }
}

/// Pings never take the exclusive lock, so they only wait on other pings.
#[must_use]
pub const fn can_start_ping(ping_in_flight: bool, rows: usize) -> bool {
    rows > 0 && !ping_in_flight
}

/// Joining and editing the store are the only exclusive operations.
#[must_use]
pub const fn can_start_mutation(busy: bool) -> bool {
    !busy
}

/// The direct-join row can act only when the tab is idle and capable.
#[must_use]
pub const fn can_join_direct(busy: bool, state: ServerJoinState) -> bool {
    !busy && matches!(state, ServerJoinState::Ready)
}

/// The join-selected action adds the selection requirement.
#[must_use]
pub const fn can_join_selected(busy: bool, state: ServerJoinState, has_selection: bool) -> bool {
    can_join_direct(busy, state) && has_selection
}

/// Validation text for user-entered address text; empty when it parses.
#[must_use]
pub fn validation_message(target: &aoko_protocol::servers::ServerTarget) -> String {
    if target.ok {
        return String::new();
    }

    if target.error.trim().is_empty() {
        aoko_protocol::servers::INVALID_ADDRESS_MESSAGE.to_owned()
    } else {
        target.error.to_owned()
    }
}

/// `Not joined yet`, or `Last joined {relative}`.
#[must_use]
pub fn last_joined_label(last_joined_iso: Option<&str>, now_ms: i64) -> String {
    match relative_time::format_stored(last_joined_iso, now_ms) {
        Some(relative) => format!("Last joined {relative}"),
        None => "Not joined yet".to_owned(),
    }
}

/// `Joining {address}.`
#[must_use]
pub fn join_started_text(display: &str) -> String {
    format!("Joining {display}.")
}

/// The footer summary after a ping-all, or an empty string with no rows.
#[must_use]
pub fn ping_summary_text(total: usize, online: usize) -> String {
    if total == 0 {
        return String::new();
    }

    if total == 1 {
        return if online == 1 {
            "The server answered the ping.".to_owned()
        } else {
            "The server did not answer the ping.".to_owned()
        };
    }

    let offline = total.saturating_sub(online);
    format!("Pinged {total} servers: {online} online, {offline} offline.")
}

/// One line of MOTD, capped at [`MAXIMUM_MOTD_LENGTH`] characters.
///
/// The cap counts characters, matching the managed slice of a `string` whose
/// code units are all in the basic plane, so a non-ASCII MOTD cannot produce a
/// half character.
#[must_use]
pub fn motd_text(motd: Option<&str>) -> String {
    let Some(motd) = motd.filter(|value| !value.trim().is_empty()) else {
        return String::new();
    };

    let single_line = motd.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut characters: Vec<char> = single_line.chars().collect();
    if characters.len() <= MAXIMUM_MOTD_LENGTH {
        return single_line;
    }

    characters.truncate(MAXIMUM_MOTD_LENGTH);
    characters.iter().collect::<String>().trim_end().to_owned()
}

/// The chip text for a ping: latency when online, an error label otherwise.
#[must_use]
pub fn ping_label(result: &PingOutcome) -> String {
    if result.online {
        format!("{} ms", result.latency_ms)
    } else {
        error_label(Some(&result.error)).to_owned()
    }
}

/// Latency a player can act on: fast, acceptable, or a failure.
#[must_use]
pub const fn ping_tone(online: bool, latency_ms: i32) -> ServerStatusTone {
    if !online {
        return ServerStatusTone::Critical;
    }

    if latency_ms < 100 {
        return ServerStatusTone::Accent;
    }

    if latency_ms <= 300 {
        ServerStatusTone::Warn
    } else {
        ServerStatusTone::Critical
    }
}

/// Maps the ping layer's free text onto three human labels.
///
/// Anything that looks like a .NET type name or an unexpected reply becomes
/// `Ping failed`; the raw reason stays in the debug log. The substring order is
/// load-bearing: `address` is checked before `timed out` so a parse failure that
/// mentions both is reported as an invalid address.
#[must_use]
pub fn error_label(error: Option<&str>) -> &'static str {
    let text = error.unwrap_or("").trim();
    if text.is_empty() {
        return "Offline";
    }

    let lowered = text.to_lowercase();
    if lowered.contains("address") || lowered.contains("host:port") {
        return INVALID_ADDRESS_CHIP_LABEL;
    }

    if lowered.contains("timed out") || lowered.contains("timeout") {
        return "No response";
    }

    if lowered.contains("cancel") {
        return NOT_PINGED_LABEL;
    }

    if lowered.contains("refused") || lowered.contains("offline") || lowered.contains("unreachable")
    {
        return "Offline";
    }

    "Ping failed"
}

/// `Added {name}.`
#[must_use]
pub fn added_text(name: &str) -> String {
    format!("Added {name}.")
}

/// `Deleted {name}.`
#[must_use]
pub fn deleted_text(name: &str) -> String {
    format!("Deleted {name}.")
}

/// The chip state one server card paints.
///
/// Mirrors `SavedServerRow`, minus `INotifyPropertyChanged`: the tab compares the
/// previous value with the new one to decide whether to notify, which is the GPUI
/// equivalent and is cheaper than five change callbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRowState {
    pub id: String,
    pub name: String,
    pub address: String,
    pub last_joined_label: String,
    pub motd: String,
    pub status_text: String,
    pub status_tone: ServerStatusTone,
    pub players_label: String,
    pub is_selected: bool,
}

impl ServerRowState {
    /// A freshly loaded row: nothing has been pinged yet.
    #[must_use]
    pub fn new(source: &ServerRowSource, now_ms: i64) -> Self {
        let name = if source.name.trim().is_empty() {
            source.address.clone()
        } else {
            source.name.clone()
        };

        Self {
            id: source.id.clone(),
            name,
            address: source.address.clone(),
            last_joined_label: last_joined_label(source.last_joined_at.as_deref(), now_ms),
            motd: String::new(),
            status_text: NOT_PINGED_LABEL.to_owned(),
            status_tone: ServerStatusTone::Neutral,
            players_label: String::new(),
            is_selected: false,
        }
    }

    /// Whether the card shows a second MOTD line.
    #[must_use]
    pub fn has_motd(&self) -> bool {
        !self.motd.is_empty()
    }

    /// Whether the chip shows a player count.
    #[must_use]
    pub fn has_players(&self) -> bool {
        !self.players_label.is_empty()
    }

    /// What a screen reader announces for the row.
    #[must_use]
    pub fn automation_name(&self) -> String {
        if self.has_players() {
            format!(
                "{}, {}, {}, {} online",
                self.name, self.address, self.status_text, self.players_label
            )
        } else {
            format!("{}, {}, {}", self.name, self.address, self.status_text)
        }
    }

    /// `selected` or empty; a Button cannot expose a selection pattern.
    #[must_use]
    pub fn selection_status(&self) -> &'static str {
        if self.is_selected {
            "selected"
        } else {
            ""
        }
    }

    /// A ping was queued for this row.
    pub fn mark_pinging(&mut self) {
        self.status_text = PINGING_LABEL.to_owned();
        self.status_tone = ServerStatusTone::Neutral;
        self.players_label.clear();
    }

    /// The ping was cancelled or the row was reloaded: back to the rest state.
    pub fn mark_not_pinged(&mut self) {
        self.status_text = NOT_PINGED_LABEL.to_owned();
        self.status_tone = ServerStatusTone::Neutral;
        self.players_label.clear();
        self.motd.clear();
    }

    /// Applies a completed ping.
    pub fn apply_ping(&mut self, result: &PingOutcome) {
        self.motd = motd_text(result.description.as_deref());
        self.status_text = ping_label(result);
        self.status_tone = ping_tone(result.online, result.latency_ms);
        self.players_label = if result.online {
            result.players_label.clone()
        } else {
            String::new()
        };
    }

    /// Applies a failed ping: three labels, never the raw exception text.
    pub fn apply_ping_failure(&mut self, message: &str) {
        self.status_text = error_label(Some(message)).to_owned();
        self.status_tone = ServerStatusTone::Critical;
        self.players_label.clear();
    }

    /// An address that cannot even be parsed: no ping is attempted.
    pub fn apply_invalid_address(&mut self) {
        self.status_text = INVALID_ADDRESS_CHIP_LABEL.to_owned();
        self.status_tone = ServerStatusTone::Critical;
        self.players_label.clear();
        self.motd.clear();
    }

    /// Carries the last ping result onto a rebuilt row, so a reload or a re-sort
    /// does not blank the chip on screen.
    pub fn copy_ping_state_from(&mut self, other: &Self) {
        self.motd = other.motd.clone();
        self.status_text = other.status_text.clone();
        self.status_tone = other.status_tone;
        self.players_label = other.players_label.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_700_000_000_000;

    fn server(
        id: &str,
        name: &str,
        address: &str,
        joined: Option<&str>,
        added: &str,
    ) -> ServerRowSource {
        ServerRowSource {
            id: id.to_owned(),
            name: name.to_owned(),
            address: address.to_owned(),
            added_at: Some(added.to_owned()),
            last_joined_at: joined.map(str::to_owned),
        }
    }

    #[test]
    fn sort_puts_most_recently_joined_first_and_never_joined_last() {
        let older = server(
            "1",
            "Older",
            "older.example.com",
            Some("2023-11-09T22:13:20Z"),
            "2024-01-01T00:00:00Z",
        );
        let never = server(
            "2",
            "Never",
            "never.example.com",
            None,
            "2024-01-01T00:00:00Z",
        );
        let newer = server(
            "3",
            "Newer",
            "newer.example.com",
            Some("2023-11-14T20:13:20Z"),
            "2024-01-01T00:00:00Z",
        );

        let ordered = sort_for_display(vec![older, never, newer]);
        let ids: Vec<&str> = ordered.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, vec!["3", "1", "2"]);
    }

    #[test]
    fn sort_falls_back_to_added_at_then_name() {
        let oldest = server("a", "Zulu", "z.example.com", None, "2024-01-01T00:00:00Z");
        let newest = server("b", "Alpha", "a.example.com", None, "2024-02-01T00:00:00Z");
        let twin = server("c", "Beta", "b.example.com", None, "2024-02-01T00:00:00Z");

        let ordered = sort_for_display(vec![oldest, twin, newest]);
        let ids: Vec<&str> = ordered.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c", "a"]);
    }

    #[test]
    fn an_unparsable_join_stamp_sorts_below_a_real_one() {
        let broken = server(
            "1",
            "Broken",
            "broken.example.com",
            Some("not a date"),
            "2024-01-01T00:00:00Z",
        );
        let real = server(
            "2",
            "Real",
            "real.example.com",
            Some("2000-01-01T00:00:00Z"),
            "2024-01-01T00:00:00Z",
        );

        let ordered = sort_for_display(vec![broken, real]);
        assert_eq!(ordered[0].id, "2");
    }

    #[test]
    fn selection_keeps_the_selected_id_across_reloads() {
        let ids = vec!["one".to_owned(), "two".to_owned(), "three".to_owned()];

        assert_eq!(resolve_selection_index(&ids, Some("two"), Some(0)), Some(1));
        assert_eq!(resolve_selection_index(&[], Some("two"), Some(0)), None);
        assert_eq!(
            resolve_selection_index(&ids, Some("gone"), Some(2)),
            Some(2),
            "a deleted row lands on its neighbour"
        );
        assert_eq!(
            resolve_selection_index(&ids, None, Some(9)),
            Some(2),
            "the fallback is clamped to the last row"
        );
        assert_eq!(resolve_selection_index(&ids, None, None), Some(0));
    }

    #[test]
    fn join_state_names_the_first_blocker() {
        assert_eq!(
            evaluate_join_state(false, false),
            ServerJoinState::Disconnected
        );
        assert_eq!(
            evaluate_join_state(true, false),
            ServerJoinState::Unsupported
        );
        assert_eq!(evaluate_join_state(true, true), ServerJoinState::Ready);

        assert_eq!(join_blocked_message(ServerJoinState::Ready), "");
        assert_eq!(
            join_blocked_message(ServerJoinState::Disconnected),
            tab_copy::not_connected()
        );
        assert_eq!(
            join_blocked_message(ServerJoinState::Unsupported),
            tab_copy::UNSUPPORTED_SERVER_JOIN
        );
    }

    #[test]
    fn primary_hint_explains_a_disabled_join() {
        assert_eq!(
            primary_hint(ServerJoinState::Disconnected, true, true),
            tab_copy::not_connected()
        );
        assert_eq!(
            primary_hint(ServerJoinState::Ready, false, false),
            tab_copy::NO_SERVERS_HINT
        );
        assert_eq!(
            primary_hint(ServerJoinState::Ready, true, false),
            tab_copy::SELECT_SERVER_HINT
        );
        assert_eq!(primary_hint(ServerJoinState::Ready, true, true), "");
    }

    #[test]
    fn pings_are_allowed_alongside_mutations() {
        assert!(can_start_ping(false, 3));
        assert!(!can_start_ping(true, 3), "one ping-all at a time");
        assert!(!can_start_ping(false, 0), "nothing to ping");
        assert!(can_start_mutation(false));
        assert!(!can_start_mutation(true));
    }

    #[test]
    fn join_actions_need_ready_plus_selection() {
        let busy = false;
        assert!(can_join_direct(busy, ServerJoinState::Ready));
        assert!(!can_join_direct(busy, ServerJoinState::Disconnected));
        assert!(can_join_selected(busy, ServerJoinState::Ready, true));
        assert!(!can_join_selected(busy, ServerJoinState::Ready, false));
    }

    #[test]
    fn latency_tones_match_the_thresholds() {
        assert_eq!(ping_tone(false, 10), ServerStatusTone::Critical);
        assert_eq!(ping_tone(true, 99), ServerStatusTone::Accent);
        assert_eq!(ping_tone(true, 100), ServerStatusTone::Warn);
        assert_eq!(ping_tone(true, 300), ServerStatusTone::Warn);
        assert_eq!(ping_tone(true, 301), ServerStatusTone::Critical);
    }

    #[test]
    fn error_labels_reduce_free_text_to_three_words() {
        assert_eq!(error_label(None), "Offline");
        assert_eq!(error_label(Some("   ")), "Offline");
        assert_eq!(
            error_label(Some("System.Net.Sockets.SocketException: address")),
            INVALID_ADDRESS_CHIP_LABEL
        );
        assert_eq!(error_label(Some("Connection timed out")), "No response");
        assert_eq!(
            error_label(Some("Operation was canceled")),
            NOT_PINGED_LABEL
        );
        assert_eq!(error_label(Some("Connection refused")), "Offline");
        assert_eq!(error_label(Some("unreachable host")), "Offline");
        assert_eq!(error_label(Some("something odd")), "Ping failed");
    }

    #[test]
    fn motd_collapses_whitespace_and_caps_the_length() {
        assert_eq!(motd_text(None), "");
        assert_eq!(motd_text(Some("   ")), "");
        assert_eq!(motd_text(Some("  A   B \n C  ")), "A B C");

        let long = "x".repeat(MAXIMUM_MOTD_LENGTH + 40);
        assert_eq!(motd_text(Some(&long)).chars().count(), MAXIMUM_MOTD_LENGTH);
    }

    #[test]
    fn motd_cap_keeps_multibyte_text_valid() {
        let long = "\u{2014}".repeat(MAXIMUM_MOTD_LENGTH + 5);
        let text = motd_text(Some(&long));
        assert_eq!(text.chars().count(), MAXIMUM_MOTD_LENGTH);
    }

    #[test]
    fn ping_summary_counts_online_and_offline() {
        assert_eq!(ping_summary_text(0, 0), "");
        assert_eq!(ping_summary_text(1, 1), "The server answered the ping.");
        assert_eq!(
            ping_summary_text(1, 0),
            "The server did not answer the ping."
        );
        assert_eq!(
            ping_summary_text(3, 2),
            "Pinged 3 servers: 2 online, 1 offline."
        );
    }

    #[test]
    fn row_state_starts_unpinged_and_builds_its_automation_name() {
        let source = server(
            "1",
            "",
            "play.example.com",
            Some("2023-11-11T22:13:20Z"),
            "2024-01-01T00:00:00Z",
        );
        let mut row = ServerRowState::new(&source, NOW_MS);

        assert_eq!(
            row.name, "play.example.com",
            "a blank name falls back to the address"
        );
        assert_eq!(row.status_text, NOT_PINGED_LABEL);
        assert_eq!(row.last_joined_label, "Last joined 3d ago");
        assert_eq!(
            row.automation_name(),
            "play.example.com, play.example.com, Not pinged"
        );
        assert_eq!(row.selection_status(), "");

        row.is_selected = true;
        assert_eq!(row.selection_status(), "selected");
    }

    #[test]
    fn applying_a_ping_moves_the_chip_and_the_motd() {
        let source = server("1", "One", "one.example.com", None, "2024-01-01T00:00:00Z");
        let mut row = ServerRowState::new(&source, NOW_MS);

        row.mark_pinging();
        assert_eq!(row.status_text, PINGING_LABEL);

        row.apply_ping(&PingOutcome {
            online: true,
            latency_ms: 42,
            players_label: "12/100".to_owned(),
            description: Some("Welcome  to\n the server".to_owned()),
            error: String::new(),
        });

        assert_eq!(row.status_text, "42 ms");
        assert_eq!(row.status_tone, ServerStatusTone::Accent);
        assert_eq!(row.motd, "Welcome to the server");
        assert!(row.has_motd());
        assert_eq!(
            row.automation_name(),
            "One, one.example.com, 42 ms, 12/100 online"
        );
    }

    #[test]
    fn a_failed_ping_never_shows_raw_exception_text() {
        let source = server("1", "One", "one.example.com", None, "2024-01-01T00:00:00Z");
        let mut row = ServerRowState::new(&source, NOW_MS);

        row.apply_ping_failure("System.Net.Sockets.SocketException (10061): connection refused");
        assert_eq!(row.status_text, "Offline");
        assert_eq!(row.status_tone, ServerStatusTone::Critical);
        assert!(!row.has_players());
    }

    #[test]
    fn an_invalid_address_never_pings() {
        let source = server("1", "One", "not an address", None, "2024-01-01T00:00:00Z");
        let mut row = ServerRowState::new(&source, NOW_MS);

        row.apply_invalid_address();
        assert_eq!(row.status_text, INVALID_ADDRESS_CHIP_LABEL);
        assert!(!row.has_motd());
    }

    #[test]
    fn a_rebuilt_row_keeps_its_chip() {
        let source = server("1", "One", "one.example.com", None, "2024-01-01T00:00:00Z");
        let mut original = ServerRowState::new(&source, NOW_MS);
        original.apply_ping(&PingOutcome {
            online: true,
            latency_ms: 42,
            players_label: "1/20".to_owned(),
            description: Some("motd".to_owned()),
            error: String::new(),
        });

        let mut rebuilt = ServerRowState::new(&source, NOW_MS);
        rebuilt.copy_ping_state_from(&original);

        assert_eq!(rebuilt.status_text, original.status_text);
        assert_eq!(rebuilt.motd, "motd");
        assert_eq!(rebuilt.players_label, "1/20");
    }

    #[test]
    fn last_joined_and_small_sentences_keep_their_wording() {
        assert_eq!(last_joined_label(None, NOW_MS), "Not joined yet");
        assert_eq!(
            last_joined_label(Some("2023-11-11T22:13:20Z"), NOW_MS),
            "Last joined 3d ago"
        );
        assert_eq!(
            join_started_text("play.example.com"),
            "Joining play.example.com."
        );
        assert_eq!(added_text("Play"), "Added Play.");
        assert_eq!(deleted_text("Play"), "Deleted Play.");
    }

    #[test]
    fn validation_message_prefers_the_target_error() {
        let ok = aoko_protocol::servers::validate_target(Some("play.example.com"));
        assert_eq!(validation_message(&ok), "");

        let bad = aoko_protocol::servers::validate_target(Some(""));
        assert_eq!(
            validation_message(&bad),
            aoko_protocol::servers::INVALID_ADDRESS_MESSAGE
        );
    }
}
