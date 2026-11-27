//! The Servers tab's state machine: rows, selection, the add form, the delete
//! confirmation, ping bookkeeping and every button's enabled state.
//!
//! The managed tab kept this in `ServersView.xaml.cs` fields plus XAML
//! `Visibility`/`IsEnabled` writes (`UpdateActionStates`, `:142-170`), so the only
//! thing under test was `ServersViewLogic`. Here the whole flow is one struct: a
//! test can open the add form, type an address, save it, ping the new row and
//! delete it again.
//!
//! Two deliberate additions, both forced by the migration:
//!
//! * **The add form has no text input.** The pin has no text-field widget
//!   (`UI_INVENTORY.md` row 9), so the form owns its strings and takes them from
//!   whatever input seam lands later; [`ServersState::open_add_form`] still does
//!   the managed prefill from the direct-address box.
//! * **Delete confirmation is state**, not a `MessageBox` (`:447-470`).

use aoko_protocol::servers::{validate_target, ServerAddress};

use crate::views::accounts_state::StatusTone;
use crate::views::servers_logic::{
    self, PingOutcome, ServerJoinState, ServerRowSource, ServerRowState, ServerStatusTone,
};
use crate::views::tab_copy;

/// The Servers tab's status line: same shape as the Accounts tab's.
pub use crate::views::accounts_state::StatusLine;

/// What a join press resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinOutcome {
    /// A mutation is in flight.
    Busy,
    /// No saved row is selected: `Select a saved server first.`
    NoSelection,
    /// The text did not parse: the validation line gets this.
    Invalid(&'static str),
    /// The bridge cannot take a join right now: the status line gets this.
    Blocked(String),
    /// Hand this address to the bridge.
    Ready(ServerAddress),
}

/// What a save in the add form resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    Busy,
    /// The address did not parse: the form's validation line gets this.
    Invalid(&'static str),
    /// Save this name (already trimmed, `None` when blank) and this display text.
    Ready {
        name: Option<String>,
        display: String,
    },
}

/// What a ping pass asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PingRequest {
    /// Nothing to ping, or a pass is already running.
    Refused,
    /// Ping these ids, in order.
    Ids(Vec<String>),
}

/// A delete that the store should now perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteOutcome {
    pub id: String,
    pub name: String,
    /// The index the selection falls back to after the removal.
    pub fallback_index: usize,
}

/// Tracks a ping pass: which rows were asked, and how many answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PingPass {
    total: usize,
    online: usize,
}

/// The Servers tab's whole state.
#[derive(Debug, Clone, Default)]
pub struct ServersState {
    rows: Vec<ServerRowState>,
    selected: Option<usize>,
    busy: bool,
    ping_in_flight: bool,
    auto_pinged: bool,
    join_state: ServerJoinState,
    direct_address: String,
    direct_validation: String,
    direct_focused: bool,
    add_open: bool,
    add_name: String,
    add_address: String,
    add_validation: String,
    add_address_focused: bool,
    status: Option<StatusLine>,
    recovery_notice: Option<String>,
    pending_delete: Option<usize>,
    pass: PingPass,
}

impl ServersState {
    /// An empty tab: no rows, nothing selected, joins blocked until the bridge
    /// says otherwise.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The rows, in display order.
    #[must_use]
    pub fn rows(&self) -> &[ServerRowState] {
        &self.rows
    }

    /// Whether the empty state replaces the list.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The selected row's index.
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// The selected row.
    #[must_use]
    pub fn selected_row(&self) -> Option<&ServerRowState> {
        self.rows.get(self.selected?)
    }

    /// The selected row's id.
    #[must_use]
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_row().map(|row| row.id.as_str())
    }

    /// Whether a row is selected.
    #[must_use]
    pub const fn has_selection(&self) -> bool {
        self.selected.is_some()
    }

    /// Whether a mutation is in flight.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        self.busy
    }

    /// Whether a ping pass is running.
    #[must_use]
    pub const fn ping_in_flight(&self) -> bool {
        self.ping_in_flight
    }

    /// The bridge's join capability.
    #[must_use]
    pub const fn join_state(&self) -> ServerJoinState {
        self.join_state
    }

    /// The direct-address box's text.
    #[must_use]
    pub fn direct_address(&self) -> &str {
        &self.direct_address
    }

    /// The direct address's validation line.
    #[must_use]
    pub fn direct_validation(&self) -> &str {
        &self.direct_validation
    }

    /// Whether the direct-address box draws a caret.
    #[must_use]
    pub const fn direct_focused(&self) -> bool {
        self.direct_focused
    }

    /// Whether the add form is open.
    #[must_use]
    pub const fn add_open(&self) -> bool {
        self.add_open
    }

    /// The add form's name box.
    #[must_use]
    pub fn add_name(&self) -> &str {
        &self.add_name
    }

    /// The add form's address box.
    #[must_use]
    pub fn add_address(&self) -> &str {
        &self.add_address
    }

    /// The add form's validation line.
    #[must_use]
    pub fn add_validation(&self) -> &str {
        &self.add_validation
    }

    /// Whether the add form's address box draws a caret.
    #[must_use]
    pub const fn add_address_focused(&self) -> bool {
        self.add_address_focused
    }

    /// The status line.
    #[must_use]
    pub const fn status(&self) -> Option<&StatusLine> {
        self.status.as_ref()
    }

    /// The store-recovery notice.
    #[must_use]
    pub fn recovery_notice(&self) -> Option<&str> {
        self.recovery_notice.as_deref()
    }

    /// The delete confirmation, while one is armed.
    #[must_use]
    pub fn delete_confirmation(&self) -> Option<(&str, String)> {
        let index = self.pending_delete?;
        let row = self.rows.get(index)?;
        Some((row.name.as_str(), delete_question(&row.name)))
    }

    /// `JoinBlockedMessage`, empty when a join would go through.
    #[must_use]
    pub fn join_blocked_message(&self) -> String {
        servers_logic::join_blocked_message(self.join_state)
    }

    /// `PrimaryHint`, empty when the line is hidden.
    #[must_use]
    pub fn primary_hint(&self) -> String {
        servers_logic::primary_hint(self.join_state, !self.rows.is_empty(), self.has_selection())
    }

    /// `DirectJoinButton.ToolTip`.
    #[must_use]
    pub fn direct_join_tooltip(&self) -> String {
        let blocked = self.join_blocked_message();
        if blocked.is_empty() {
            "Join this address. Press Enter in the box to do the same.".to_owned()
        } else {
            blocked
        }
    }

    /// `PingAllButton.ToolTip` (`:152-155`).
    #[must_use]
    pub fn ping_all_tooltip(&self) -> String {
        if self.rows.is_empty() {
            tab_copy::NO_SERVERS_HINT.to_owned()
        } else if self.ping_in_flight {
            "A ping pass is already running".to_owned()
        } else {
            "Ping every saved server".to_owned()
        }
    }

    /// `DeleteButton.ToolTip`.
    #[must_use]
    pub fn delete_tooltip(&self) -> String {
        if self.has_selection() {
            "Remove this server from the saved list".to_owned()
        } else {
            tab_copy::SELECT_SERVER_HINT.to_owned()
        }
    }

    /// Whether the direct Join button is enabled.
    #[must_use]
    pub const fn can_join_direct(&self) -> bool {
        servers_logic::can_join_direct(self.busy, self.join_state)
    }

    /// Whether Join selected is enabled.
    #[must_use]
    pub const fn can_join_selected(&self) -> bool {
        servers_logic::can_join_selected(self.busy, self.join_state, self.selected.is_some())
    }

    /// Whether Add server and the form's Add are enabled.
    #[must_use]
    pub const fn can_start_mutation(&self) -> bool {
        servers_logic::can_start_mutation(self.busy)
    }

    /// Whether Ping all is enabled.
    #[must_use]
    pub const fn can_start_ping(&self) -> bool {
        servers_logic::can_start_ping(self.ping_in_flight, self.rows.len())
    }

    /// Whether Delete is enabled.
    #[must_use]
    pub const fn can_delete(&self) -> bool {
        self.can_start_mutation() && self.selected.is_some()
    }

    /// Reloads the list the way `ReloadServers` does (`:81-116`).
    ///
    /// Ping state survives per id, the list is sorted for display, and the
    /// selection resolves to `selected_id`, else `fallback_index`, clamped.
    /// Returns the index now selected.
    pub fn load(
        &mut self,
        sources: &[ServerRowSource],
        selected_id: Option<&str>,
        fallback_index: Option<usize>,
        now_ms: i64,
    ) -> Option<usize> {
        let keep: Option<String> = selected_id
            .map(str::to_owned)
            .or_else(|| self.selected_id().map(str::to_owned));
        let previous: Vec<ServerRowState> = self.rows.clone();
        let mut sorted = servers_logic::sort_for_display(sources.to_vec());

        // `SortForDisplay` returns newly built sources; the ping state is keyed by
        // id, which is what survives a reorder.
        let mut rows = Vec::with_capacity(sorted.len());
        for source in sorted.drain(..) {
            let mut row = ServerRowState::new(&source, now_ms);
            if let Some(old) = previous.iter().find(|row| row.id == source.id) {
                copy_ping_state(&mut row, old);
            }
            rows.push(row);
        }

        self.rows = rows;
        let ids: Vec<String> = self.rows.iter().map(|row| row.id.clone()).collect();
        let resolved =
            servers_logic::resolve_selection_index(&ids, keep.as_deref(), fallback_index);
        self.selected = resolved;
        self.apply_selection();
        self.pending_delete = None;
        resolved
    }

    /// Repaints `IsSelected` across the rows (`ApplySelection`, `:117`).
    fn apply_selection(&mut self) {
        let selected = self.selected;
        for (index, row) in self.rows.iter_mut().enumerate() {
            row.is_selected = selected == Some(index);
        }
    }

    /// Selects a row by index (`SelectRow`, `:396`).
    pub fn select_index(&mut self, index: usize) -> bool {
        if index >= self.rows.len() {
            return false;
        }
        self.selected = Some(index);
        self.apply_selection();
        true
    }

    /// Selects a row by id.
    pub fn select_id(&mut self, id: &str) -> bool {
        match self.rows.iter().position(|row| row.id == id) {
            Some(index) => self.select_index(index),
            None => false,
        }
    }

    /// Reports the bridge's join capability.
    pub fn set_join_state(&mut self, state: ServerJoinState) -> bool {
        let changed = self.join_state != state;
        self.join_state = state;
        changed
    }

    /// `TryBeginBusy` (`:172`): false when a mutation is already running.
    pub fn begin_busy(&mut self) -> bool {
        if !servers_logic::can_start_mutation(self.busy) {
            return false;
        }
        self.busy = true;
        true
    }

    /// `EndBusy`.
    pub fn end_busy(&mut self) {
        self.busy = false;
    }

    /// Sets the direct-address text, clearing stale validation.
    pub fn set_direct_address(&mut self, text: impl Into<String>) {
        self.direct_address = text.into();
        self.direct_validation.clear();
    }

    /// Focuses or blurs the direct-address box.
    pub fn set_direct_focused(&mut self, focused: bool) {
        self.direct_focused = focused;
    }

    /// `JoinDirect` (`:244-263`).
    pub fn submit_direct_join(&mut self) -> JoinOutcome {
        if !servers_logic::can_start_mutation(self.busy) {
            return JoinOutcome::Busy;
        }

        let target = validate_target(Some(&self.direct_address));
        let Some(address) = target.address else {
            self.direct_validation = servers_logic::validation_message(&target);
            return JoinOutcome::Invalid(target.error);
        };

        self.direct_validation.clear();
        self.ready_to_join(address)
    }

    /// `JoinSelectedAsync` (`:271-300`).
    pub fn submit_selected_join(&mut self) -> JoinOutcome {
        if !servers_logic::can_start_mutation(self.busy) {
            return JoinOutcome::Busy;
        }

        let Some(row) = self.selected_row().cloned() else {
            self.set_status(servers_logic::EMPTY_SELECTION_MESSAGE, StatusTone::Neutral);
            return JoinOutcome::NoSelection;
        };

        let target = validate_target(Some(&row.address));
        let Some(address) = target.address else {
            self.set_status(
                servers_logic::validation_message(&target),
                StatusTone::Critical,
            );
            return JoinOutcome::Invalid(target.error);
        };

        self.ready_to_join(address)
    }

    /// `JoinTargetAsync`'s gate (`:301-320`).
    fn ready_to_join(&mut self, address: ServerAddress) -> JoinOutcome {
        if self.join_state != ServerJoinState::Ready {
            let blocked = self.join_blocked_message();
            self.set_status(blocked.clone(), StatusTone::Critical);
            return JoinOutcome::Blocked(blocked);
        }

        JoinOutcome::Ready(address)
    }

    /// The join was handed to the bridge: `Joining {display}\u{2026}`.
    pub fn join_started(&mut self, display: &str) {
        self.set_status(
            servers_logic::join_started_text(display),
            StatusTone::Neutral,
        );
    }

    /// A join that could not be started.
    pub fn join_failed(&mut self) {
        self.set_status(
            "The join could not be started. Try again.",
            StatusTone::Critical,
        );
    }

    /// `AddServerButton_Click` (`:477-491`): opens the form and prefills the
    /// address from the direct box when the form's own box is blank.
    pub fn open_add_form(&mut self) -> bool {
        if !servers_logic::can_start_mutation(self.busy) {
            return false;
        }

        self.add_open = true;
        self.add_validation.clear();

        if self.add_address.trim().is_empty() {
            self.add_address = self.direct_address.trim().to_owned();
        }

        self.add_address_focused = true;
        true
    }

    /// `HideAddForm` (`:534-546`).
    pub fn close_add_form(&mut self) {
        self.add_open = false;
        self.add_validation.clear();
        self.add_name.clear();
        self.add_address.clear();
        self.add_address_focused = false;
    }

    /// Sets the add form's name box.
    pub fn set_add_name(&mut self, text: impl Into<String>) {
        self.add_name = text.into();
    }

    /// Sets the add form's address box, clearing stale validation.
    pub fn set_add_address(&mut self, text: impl Into<String>) {
        self.add_address = text.into();
        self.add_validation.clear();
    }

    /// Focuses or blurs the add form's address box.
    pub fn set_add_address_focused(&mut self, focused: bool) {
        self.add_address_focused = focused;
    }

    /// `AddFromForm` (`:505-533`).
    pub fn submit_add(&mut self) -> AddOutcome {
        if !servers_logic::can_start_mutation(self.busy) {
            return AddOutcome::Busy;
        }

        let target = validate_target(Some(&self.add_address));
        let Some(address) = target.address else {
            self.add_validation = servers_logic::validation_message(&target);
            return AddOutcome::Invalid(target.error);
        };

        let name = self.add_name.trim();
        AddOutcome::Ready {
            name: if name.is_empty() {
                None
            } else {
                Some(name.to_owned())
            },
            display: address.display(),
        }
    }

    /// Applies a saved server: closes the form, selects the new row and announces
    /// it, then pings it without announcing (`:524-528`).
    pub fn apply_added(
        &mut self,
        sources: &[ServerRowSource],
        saved_id: &str,
        saved_name: &str,
        now_ms: i64,
    ) -> String {
        let fallback = self.selected;
        self.close_add_form();
        self.load(sources, Some(saved_id), fallback, now_ms);
        self.set_status(servers_logic::added_text(saved_name), StatusTone::Success);
        servers_logic::added_text(saved_name)
    }

    /// The save failed outright (`:530`).
    pub fn add_failed(&mut self) {
        self.set_status(
            "The server could not be saved. Try again.",
            StatusTone::Critical,
        );
    }

    /// `DeleteButton_Click` (`:439-470`): arms the confirmation.
    pub fn request_delete(&mut self) -> Option<String> {
        if !servers_logic::can_start_mutation(self.busy) {
            return None;
        }

        let Some(index) = self.selected else {
            self.set_status(servers_logic::EMPTY_SELECTION_MESSAGE, StatusTone::Neutral);
            return None;
        };

        if index >= self.rows.len() {
            return None;
        }

        self.pending_delete = Some(index);
        Some(self.rows[index].id.clone())
    }

    /// Cancels the confirmation.
    pub fn cancel_delete(&mut self) {
        self.pending_delete = None;
    }

    /// Consumes the confirmation.
    pub fn confirm_delete(&mut self) -> Option<DeleteOutcome> {
        let index = self.pending_delete.take()?;
        let row = self.rows.get(index)?.clone();
        Some(DeleteOutcome {
            id: row.id,
            name: row.name,
            fallback_index: index,
        })
    }

    /// Applies a removal: reloads with the old index as the fallback and says
    /// `Deleted {name}.`
    pub fn apply_deleted(
        &mut self,
        sources: &[ServerRowSource],
        outcome: &DeleteOutcome,
        now_ms: i64,
    ) {
        self.load(sources, None, Some(outcome.fallback_index), now_ms);
        self.set_status(
            servers_logic::deleted_text(&outcome.name),
            StatusTone::Success,
        );
    }

    /// The removal failed.
    pub fn delete_failed(&mut self) {
        self.set_status(
            "The server could not be deleted. Try again.",
            StatusTone::Critical,
        );
    }

    /// `PingAllButton_Click` (`:423-437`).
    pub fn start_ping_all(&mut self) -> PingRequest {
        if !servers_logic::can_start_ping(self.ping_in_flight, self.rows.len()) {
            return PingRequest::Refused;
        }

        let ids: Vec<String> = self.rows.iter().map(|row| row.id.clone()).collect();
        self.begin_pass(ids.len(), &ids);
        PingRequest::Ids(ids)
    }

    /// `LatencyChip_Click` (`:404-421`): one row, only while a pass is not
    /// running.
    pub fn start_ping_one(&mut self, id: &str) -> PingRequest {
        if !servers_logic::can_start_ping(self.ping_in_flight, self.rows.len()) {
            return PingRequest::Refused;
        }

        if self.rows.iter().all(|row| row.id != id) {
            return PingRequest::Refused;
        }

        self.begin_pass(1, &[id.to_owned()]);
        PingRequest::Ids(vec![id.to_owned()])
    }

    fn begin_pass(&mut self, total: usize, ids: &[String]) {
        self.ping_in_flight = true;
        self.pass = PingPass { total, online: 0 };

        for row in &mut self.rows {
            if ids.contains(&row.id) {
                row.mark_pinging();
            }
        }
    }

    /// Applies one ping answer.
    pub fn apply_ping(&mut self, id: &str, outcome: &PingOutcome) -> bool {
        let Some(row) = self.rows.iter_mut().find(|row| row.id == id) else {
            return false;
        };

        row.apply_ping(outcome);
        if outcome.online {
            self.pass.online += 1;
        }
        true
    }

    /// Applies one ping failure: three labels, never the raw exception text.
    pub fn apply_ping_failure(&mut self, id: &str, message: &str) -> bool {
        let Some(row) = self.rows.iter_mut().find(|row| row.id == id) else {
            return false;
        };

        row.apply_ping_failure(message);
        true
    }

    /// Ends the pass and returns the footer summary, empty when the pass was
    /// silent (`announce: false`).
    ///
    /// The tone is the managed one: `ServersView.xaml.cs:611-617` paints the summary
    /// `StatusTone.Success` when every pinged server answered and `StatusTone.Critical` otherwise,
    /// so a pass with one dead bookmark reads as a problem rather than as neutral information. A
    /// silent pass (`announce: false`, the auto-ping after an add) sets no status at all, so the
    /// tone is unused there.
    pub fn finish_pass(&mut self, announce: bool) -> String {
        self.ping_in_flight = false;
        self.auto_pinged = true;

        let summary = servers_logic::ping_summary_text(self.pass.total, self.pass.online);
        let tone = if self.pass.online == self.pass.total {
            StatusTone::Success
        } else {
            StatusTone::Critical
        };
        self.pass = PingPass::default();

        if announce {
            self.set_status(summary.clone(), tone);
        }

        summary
    }

    /// Whether the first load has already pinged once (`_autoPinged`).
    #[must_use]
    pub const fn auto_pinged(&self) -> bool {
        self.auto_pinged
    }

    /// Marks the first auto-ping as done without pinging: used by the capture
    /// harness so a screenshot shows the rest state rather than a spinner.
    pub fn mark_auto_pinged(&mut self) {
        self.auto_pinged = true;
    }

    /// A ping that never finished (`:411-437`): the rows are left alone and the
    /// pass stops.
    pub fn ping_failed(&mut self) {
        self.ping_in_flight = false;
        self.pass = PingPass::default();
        for row in &mut self.rows {
            if row.status_text == servers_logic::PINGING_LABEL {
                row.mark_not_pinged();
            }
        }
        self.set_status(
            "The ping could not finish. Try again.",
            StatusTone::Critical,
        );
    }

    /// The store-recovery notice, `None` to hide it.
    pub fn set_recovery_notice(&mut self, notice: Option<String>) {
        self.recovery_notice = notice;
    }

    /// Sets the status line, clearing it when the text is blank.
    pub fn set_status(&mut self, text: impl Into<String>, tone: StatusTone) {
        let text = text.into();
        if text.trim().is_empty() {
            self.status = None;
        } else {
            self.status = Some(StatusLine::new(text, tone));
        }
    }

    /// Dismisses the status line.
    pub fn clear_status(&mut self) {
        self.status = None;
    }

    /// The chip tone for a row, resolved once so the view cannot pick another.
    #[must_use]
    pub fn row_tone(row: &ServerRowState) -> ServerStatusTone {
        row.status_tone
    }
}

/// `CopyPingStateFrom`: keeps the last ping when the list is rebuilt.
fn copy_ping_state(target: &mut ServerRowState, old: &ServerRowState) {
    target.motd = old.motd.clone();
    target.status_text = old.status_text.clone();
    target.status_tone = old.status_tone;
    target.players_label = old.players_label.clone();
}

/// The managed delete question, verbatim (`:455-457`).
#[must_use]
pub fn delete_question(name: &str) -> String {
    format!("Delete {name}? This only removes the bookmark. Your running game is not affected.")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_700_000_000_000;

    fn source(id: &str, name: &str, address: &str) -> ServerRowSource {
        ServerRowSource {
            id: id.to_owned(),
            name: name.to_owned(),
            address: address.to_owned(),
            added_at: Some("2024-01-01T00:00:00Z".to_owned()),
            last_joined_at: None,
        }
    }

    fn three() -> Vec<ServerRowSource> {
        vec![
            source("a", "Alpha", "alpha.example.net"),
            source("b", "Beta", "beta.example.net:25566"),
            source("c", "Gamma", "gamma.example.net"),
        ]
    }

    fn ready() -> ServersState {
        let mut state = ServersState::new();
        state.set_join_state(ServerJoinState::Ready);
        state
    }

    #[test]
    fn the_first_load_selects_the_first_row_so_a_join_can_start() {
        let mut state = ready();
        assert_eq!(state.load(&three(), None, None, NOW_MS), Some(0));
        assert!(state.has_selection());
        assert!(state.rows()[0].is_selected);
        assert!(!state.rows()[1].is_selected);
    }

    #[test]
    fn a_selection_survives_a_reorder_by_id() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);
        state.select_index(2);
        assert_eq!(state.selected_id(), Some("c"));

        // Gamma becomes the most recently joined, so it sorts first: the id wins.
        let mut reordered = three();
        reordered[2].last_joined_at = Some("2024-06-01T00:00:00Z".to_owned());
        assert_eq!(state.load(&reordered, Some("c"), None, NOW_MS), Some(0));
        assert_eq!(state.selected_id(), Some("c"));
    }

    #[test]
    fn a_fallback_index_keeps_the_selection_near_the_deleted_row() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);
        state.select_index(2);

        // Deleting the last row falls back to index 2, clamped to the new length.
        let remaining = &three()[..2];
        assert_eq!(state.load(remaining, None, Some(2), NOW_MS), Some(1));
        assert_eq!(state.selected_id(), Some("b"));
    }

    #[test]
    fn an_empty_list_has_no_selection_and_the_empty_state() {
        let mut state = ready();
        state.load(&[], None, None, NOW_MS);
        assert!(state.is_empty());
        assert_eq!(state.selected_index(), None);
        assert_eq!(state.primary_hint(), tab_copy::NO_SERVERS_HINT);
        assert!(!state.can_start_ping(), "nothing to ping");
    }

    #[test]
    fn a_join_without_a_selection_reports_the_hint() {
        let mut state = ready();
        assert_eq!(state.submit_selected_join(), JoinOutcome::NoSelection);
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some(servers_logic::EMPTY_SELECTION_MESSAGE)
        );
        assert_eq!(
            state.status().map(|status| status.tone),
            Some(StatusTone::Neutral)
        );
    }

    #[test]
    fn a_direct_join_validates_before_it_needs_the_bridge() {
        let mut state = ready();
        state.set_direct_address("not a host");
        assert!(matches!(
            state.submit_direct_join(),
            JoinOutcome::Invalid(_)
        ));
        assert_eq!(
            state.direct_validation(),
            servers_logic::validation_message(&validate_target(Some("not a host")))
        );

        state.set_direct_address("mc.example.net");
        assert_eq!(
            state.direct_validation(),
            "",
            "typing clears the stale message"
        );
        match state.submit_direct_join() {
            JoinOutcome::Ready(address) => assert_eq!(address.display(), "mc.example.net"),
            other => panic!("wanted Ready, got {other:?}"),
        }
    }

    #[test]
    fn a_join_while_disconnected_reports_why() {
        let mut state = ServersState::new();
        state.set_direct_address("mc.example.net");

        let JoinOutcome::Blocked(message) = state.submit_direct_join() else {
            panic!("the bridge is offline");
        };
        assert_eq!(message, tab_copy::not_connected());
        assert_eq!(
            state.status().map(|status| status.tone),
            Some(StatusTone::Critical)
        );
    }

    #[test]
    fn a_busy_tab_refuses_joins_and_edits() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);
        assert!(state.begin_busy());

        assert!(!state.begin_busy(), "one mutation at a time");
        assert_eq!(state.submit_direct_join(), JoinOutcome::Busy);
        assert_eq!(state.submit_selected_join(), JoinOutcome::Busy);
        assert_eq!(state.submit_add(), AddOutcome::Busy);
        assert!(!state.open_add_form());
        assert!(state.request_delete().is_none());
        assert!(!state.can_delete());

        state.end_busy();
        assert!(state.can_start_mutation());
    }

    #[test]
    fn the_add_form_prefills_from_the_direct_box_and_clears_on_close() {
        let mut state = ready();
        state.set_direct_address("mc.example.net");

        assert!(state.open_add_form());
        assert_eq!(state.add_address(), "mc.example.net");
        assert!(state.add_address_focused());

        state.set_add_name("Main");
        state.close_add_form();
        assert!(!state.add_open());
        assert_eq!(state.add_name(), "");
        assert_eq!(state.add_address(), "");
        assert!(!state.add_address_focused());
    }

    #[test]
    fn a_saved_server_takes_the_selection_and_a_blank_name_stays_blank() {
        let mut state = ready();
        state.load(&three()[..1], None, None, NOW_MS);
        state.open_add_form();
        state.set_add_address("new.example.net");

        let AddOutcome::Ready { name, display } = state.submit_add() else {
            panic!("a valid address");
        };
        assert_eq!(name, None, "a blank name is the address's job");
        assert_eq!(display, "new.example.net");

        state.set_add_name("  Spawn  ");
        let AddOutcome::Ready { name, .. } = state.submit_add() else {
            panic!("a valid address");
        };
        assert_eq!(name.as_deref(), Some("Spawn"), "trimmed");

        let mut with_new = three();
        with_new.push(source("n", "Spawn", "new.example.net"));
        let announce = state.apply_added(&with_new, "n", "Spawn", NOW_MS);
        assert_eq!(announce, "Added Spawn.");
        assert_eq!(state.selected_id(), Some("n"));
        assert!(!state.add_open());
    }

    #[test]
    fn an_invalid_add_address_shows_the_validation_line_and_keeps_the_form() {
        let mut state = ready();
        state.open_add_form();
        state.set_add_address("bad host:port:extra");

        assert!(matches!(state.submit_add(), AddOutcome::Invalid(_)));
        assert!(!state.add_validation().is_empty());
        assert!(state.add_open());
    }

    #[test]
    fn deleting_asks_first_with_the_managed_question() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);
        state.select_index(1);

        assert_eq!(state.request_delete().as_deref(), Some("b"));
        let (name, question) = state.delete_confirmation().expect("armed");
        assert_eq!(name, "Beta");
        assert_eq!(
            question,
            "Delete Beta? This only removes the bookmark. Your running game is not affected."
        );

        state.cancel_delete();
        assert!(state.confirm_delete().is_none());
    }

    #[test]
    fn a_confirmed_delete_reloads_on_the_old_index_and_announces() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);
        state.select_index(1);
        state.request_delete();
        let outcome = state.confirm_delete().expect("armed");
        assert_eq!(outcome.fallback_index, 1);

        let remaining = vec![three()[0].clone(), three()[2].clone()];
        state.apply_deleted(&remaining, &outcome, NOW_MS);

        assert_eq!(state.rows().len(), 2);
        assert_eq!(state.selected_id(), Some("c"), "the row that moved up");
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some("Deleted Beta.")
        );
    }

    #[test]
    fn a_ping_pass_marks_every_row_and_then_summarises() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);

        let PingRequest::Ids(ids) = state.start_ping_all() else {
            panic!("a full pass");
        };
        assert_eq!(ids.len(), 3);
        assert!(state.ping_in_flight());
        assert!(state
            .rows()
            .iter()
            .all(|row| row.status_text == servers_logic::PINGING_LABEL));
        assert_eq!(
            state.start_ping_all(),
            PingRequest::Refused,
            "one pass at a time"
        );
        assert!(!state.can_start_ping());

        assert!(state.apply_ping(
            "a",
            &PingOutcome {
                online: true,
                latency_ms: 42,
                players_label: "12/100".to_owned(),
                description: Some("A Minecraft Server".to_owned()),
                error: String::new(),
            }
        ));
        assert!(state.apply_ping_failure("b", "timed out"));
        assert!(state.apply_ping(
            "c",
            &PingOutcome {
                online: true,
                latency_ms: 90,
                ..PingOutcome::default()
            }
        ));

        let summary = state.finish_pass(true);
        assert_eq!(summary, "Pinged 3 servers: 2 online, 1 offline.");
        assert!(!state.ping_in_flight());
        assert!(state.auto_pinged());
        assert_eq!(state.rows()[0].status_text, "42 ms");
        assert_eq!(state.rows()[0].motd, "A Minecraft Server");
        assert_eq!(state.rows()[0].players_label, "12/100");
        assert_eq!(
            state.status().map(|status| status.tone),
            Some(StatusTone::Critical),
            "one offline server makes the summary a problem, not a neutral note"
        );
    }

    #[test]
    fn a_pass_where_every_server_answered_is_a_success() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);
        state.start_ping_all();

        for id in ["a", "b", "c"] {
            assert!(state.apply_ping(
                id,
                &PingOutcome {
                    online: true,
                    latency_ms: 20,
                    ..PingOutcome::default()
                }
            ));
        }

        assert_eq!(
            state.finish_pass(true),
            "Pinged 3 servers: 3 online, 0 offline."
        );
        assert_eq!(
            state.status().map(|status| status.tone),
            Some(StatusTone::Success),
            "`online == pinged.Count` is the managed Success branch"
        );
    }

    #[test]
    fn a_silent_pass_paints_nothing() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);
        state.start_ping_all();
        state.apply_ping(
            "a",
            &PingOutcome {
                online: true,
                ..PingOutcome::default()
            },
        );

        state.finish_pass(false);
        assert!(
            state.status().is_none(),
            "an auto-ping after an add announces nothing"
        );
    }

    #[test]
    fn pinging_one_row_is_refused_while_a_pass_runs() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);

        assert_eq!(
            state.start_ping_one("b"),
            PingRequest::Ids(vec!["b".to_owned()])
        );
        assert_eq!(state.start_ping_one("a"), PingRequest::Refused);
        assert!(state.rows()[1].status_text == servers_logic::PINGING_LABEL);
        assert!(state.rows()[0].status_text == servers_logic::NOT_PINGED_LABEL);

        state.finish_pass(false);
        assert_eq!(state.start_ping_one("missing"), PingRequest::Refused);
    }

    #[test]
    fn a_ping_survives_a_reload() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);
        state.start_ping_all();
        state.apply_ping(
            "a",
            &PingOutcome {
                online: true,
                latency_ms: 30,
                description: Some("Hello".to_owned()),
                ..PingOutcome::default()
            },
        );
        state.finish_pass(false);

        state.load(&three(), None, None, NOW_MS);
        assert_eq!(state.rows()[0].status_text, "30 ms");
        assert_eq!(state.rows()[0].motd, "Hello");
    }

    #[test]
    fn a_failed_ping_stops_the_pass_and_says_so() {
        let mut state = ready();
        state.load(&three(), None, None, NOW_MS);
        state.start_ping_all();
        state.ping_failed();

        assert!(!state.ping_in_flight());
        assert!(
            state
                .rows()
                .iter()
                .all(|row| row.status_text == servers_logic::NOT_PINGED_LABEL),
            "a failed pass clears the spinner"
        );
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some("The ping could not finish. Try again.")
        );
    }

    #[test]
    fn a_summary_of_nothing_is_empty() {
        let mut state = ready();
        assert_eq!(state.finish_pass(true), "");
        assert!(state.status().is_none(), "an empty summary hides the line");
    }

    #[test]
    fn the_tooltips_are_the_managed_ones() {
        let mut state = ServersState::new();
        assert_eq!(state.ping_all_tooltip(), tab_copy::NO_SERVERS_HINT);
        assert_eq!(state.delete_tooltip(), tab_copy::SELECT_SERVER_HINT);

        state.load(&three(), None, None, NOW_MS);
        assert_eq!(state.ping_all_tooltip(), "Ping every saved server");
        assert_eq!(
            state.delete_tooltip(),
            "Remove this server from the saved list"
        );
        assert_eq!(
            state.direct_join_tooltip(),
            tab_copy::not_connected(),
            "the refusal replaces the tooltip"
        );

        state.set_join_state(ServerJoinState::Ready);
        assert_eq!(
            state.direct_join_tooltip(),
            "Join this address. Press Enter in the box to do the same."
        );
    }

    #[test]
    fn the_primary_hint_walks_capability_rows_and_selection() {
        let mut state = ServersState::new();
        assert_eq!(state.primary_hint(), tab_copy::not_connected());

        state.set_join_state(ServerJoinState::Ready);
        assert_eq!(state.primary_hint(), tab_copy::NO_SERVERS_HINT);

        state.load(&three(), None, None, NOW_MS);
        assert!(state.primary_hint().is_empty(), "a row is selected");

        state.load(&[], None, None, NOW_MS);
        assert_eq!(state.primary_hint(), tab_copy::NO_SERVERS_HINT);
    }

    #[test]
    fn a_join_status_line_starts_as_the_managed_text() {
        let mut state = ready();
        state.join_started("mc.example.net");
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some("Joining mc.example.net.")
        );
        assert_eq!(
            state.status().map(|status| status.tone),
            Some(StatusTone::Neutral)
        );

        state.join_failed();
        assert_eq!(
            state.status().map(|status| status.tone),
            Some(StatusTone::Critical)
        );
    }
}
