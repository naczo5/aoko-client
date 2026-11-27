//! The Accounts tab's state machine: rows, selection, the two inline forms, the
//! delete confirmation and every button's enabled state.
//!
//! `AccountsViewLogic` holds the managed *decisions*; this holds the managed
//! *state*. In WPF that state lived in `AccountsView.xaml.cs` fields plus
//! `Visibility` and `IsEnabled` properties set by `UpdateActionStates`
//! (`:1063-1100`), which is why it was never testable. Here it is one struct, no
//! widget in sight, so a test can drive a whole add/delete/switch session.
//!
//! Deliberate additions over the managed code, both forced by the migration:
//!
//! * **Delete confirmation is state, not a `MessageBox`.** GPUI has no native
//!   message box and the shell draws its own chrome (decision 5), so
//!   [`AccountsState::request_delete`] arms a confirmation the view renders and
//!   [`AccountsState::confirm_delete`] consumes. The question text is the managed
//!   one, verbatim.
//! * **Field text lives here.** There is no text-input widget at the pin
//!   (`UI_INVENTORY.md` row 9), so the paste and offline forms own their strings
//!   and take them from whatever input seam lands later.

use aoko_protocol::accounts::offline;
use aoko_protocol::accounts::StoredAccount;

use crate::views::accounts_logic::{self, AccountSourceTone, SessionChipTone};
use crate::views::tab_copy;

/// Which inline panel is open.
///
/// `AccountsView.xaml.cs:697-736` toggles one panel and collapses the other, so
/// this is one value rather than two booleans that can disagree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AccountPanel {
    /// Neither panel: the list is the whole body.
    #[default]
    None,
    /// `PastePanel` (`AccountsView.xaml:219`).
    Paste,
    /// `OfflinePanel` (`:271`).
    Offline,
}

impl AccountPanel {
    /// Whether the paste panel is the open one.
    #[must_use]
    pub const fn is_paste(self) -> bool {
        matches!(self, Self::Paste)
    }

    /// Whether the offline panel is the open one.
    #[must_use]
    pub const fn is_offline(self) -> bool {
        matches!(self, Self::Offline)
    }
}

/// The status line's tone, which selects a brush in `TabCopy.BrushKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Neutral,
    Success,
    Critical,
}

/// One status line: the shortened text plus the full text the tooltip shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    pub tone: StatusTone,
    pub detail: Option<String>,
}

impl StatusLine {
    /// A status line with no separate tooltip.text.
    #[must_use]
    pub fn new(text: impl Into<String>, tone: StatusTone) -> Self {
        Self {
            text: text.into(),
            tone,
            detail: None,
        }
    }

    fn with_detail(text: impl Into<String>, tone: StatusTone, detail: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone,
            detail: Some(detail.into()),
        }
    }

    /// The text as the line paints it: `TabCopy.Shorten` at 110 characters.
    #[must_use]
    pub fn short_text(&self) -> String {
        tab_copy::shorten(Some(&self.text))
    }

    /// The tooltip: the detail when there is one, else the full text.
    #[must_use]
    pub fn tooltip(&self) -> &str {
        self.detail.as_deref().unwrap_or(&self.text)
    }
}

/// One card, as the list paints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRowState {
    pub id: String,
    pub name: String,
    /// The avatar letter: `InitialOf` (`AccountsView.xaml.cs:351`).
    pub initial: String,
    pub source_label: &'static str,
    pub source_tone: AccountSourceTone,
    /// The stored `lastUsedAt` stamp, kept beside the sentence it produced so a repaint can
    /// re-age it. The managed card held the `DateTimeOffset` (`AccountsView.xaml.cs:307-323`) and
    /// re-ran `LastUsedLabel` on every load; a formatted string alone cannot do that, and
    /// [`AccountsState::refresh_session_marks`] is exactly the repaint that needs it.
    pub last_used_at: Option<String>,
    pub last_used_label: String,
    /// `UUID: {dashed}`.
    pub uuid_label: String,
    /// This account is the one the game is signed in with.
    pub is_current: bool,
    pub automation_name: String,
}

impl AccountRowState {
    /// `new Card(...)` (`AccountsView.xaml.cs:307-323`), one row at a time.
    #[must_use]
    pub fn new(account: &StoredAccount, session_uuid: Option<&str>, now_ms: i64) -> Self {
        let is_current = accounts_logic::is_current_session(Some(&account.uuid), session_uuid);
        let source_label =
            accounts_logic::source_chip_label(Some(&account.source), account.has_refresh());

        Self {
            id: account.id.clone(),
            name: account.name.clone(),
            initial: initial_of(Some(&account.name)),
            source_label,
            source_tone: accounts_logic::source_tone(Some(&account.source), account.has_refresh()),
            last_used_at: account.last_used_at.clone(),
            last_used_label: accounts_logic::last_used_label(
                account.last_used_at.as_deref(),
                now_ms,
            ),
            uuid_label: format!("UUID: {}", accounts_logic::format_uuid(Some(&account.uuid))),
            is_current,
            automation_name: accounts_logic::card_automation_name(
                &account.name,
                source_label,
                is_current,
            ),
        }
    }
}

/// `InitialOf`: the first character upper-cased, `?` when blank.
#[must_use]
pub fn initial_of(name: Option<&str>) -> String {
    let trimmed = name.unwrap_or("").trim();
    trimmed
        .chars()
        .next()
        .map(|first| first.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_owned())
}

/// What the game reports about its session, and what this build can read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionStrip {
    pub connected: bool,
    pub in_world: bool,
    pub supports_set_session: bool,
    pub supports_session_read: bool,
    /// `null` until a refresh succeeds (`_sessionKnown`).
    pub identity: Option<SessionIdentity>,
    pub refresh_in_flight: bool,
}

/// A successfully read session: a name and a UUID, each optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdentity {
    pub name: Option<String>,
    pub uuid: Option<String>,
}

impl SessionStrip {
    /// Which chip is visible (`ApplySessionChip`, `:161`).
    #[must_use]
    pub fn chip(&self) -> SessionChipTone {
        accounts_logic::session_chip_tone(self.connected, self.in_world)
    }

    /// `SessionNameText` (`ApplySessionText`, `:172`).
    #[must_use]
    pub fn name_text(&self) -> String {
        match &self.identity {
            None => "Session unknown".to_owned(),
            Some(identity) => match &identity.name {
                Some(name) => name.clone(),
                None => "No account signed in".to_owned(),
            },
        }
    }

    /// `SessionUuidText`.
    #[must_use]
    pub fn detail_text(&self) -> String {
        match &self.identity {
            None => accounts_logic::session_caption(self.connected, self.supports_session_read)
                .to_owned(),
            Some(identity) => match &identity.name {
                None => "The game reports no active session.".to_owned(),
                Some(_) => match &identity.uuid {
                    Some(uuid) if !uuid.trim().is_empty() => {
                        accounts_logic::format_uuid(Some(uuid))
                    }
                    _ => "UUID unavailable".to_owned(),
                },
            },
        }
    }

    /// `RefreshSessionButton.IsEnabled` (`:1085`).
    #[must_use]
    pub const fn can_refresh(&self, busy: bool) -> bool {
        !busy && !self.refresh_in_flight && self.supports_session_read
    }

    /// The UUID the cards compare against for the "in use" dot.
    #[must_use]
    pub fn uuid(&self) -> Option<&str> {
        self.identity.as_ref()?.uuid.as_deref()
    }
}

/// Every button the tab draws, resolved in one place.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccountActions {
    pub use_account: bool,
    pub restore_session: bool,
    pub delete: bool,
    pub import_file: bool,
    pub paste_import: bool,
    pub add_offline: bool,
    pub paste_authenticate: bool,
    pub offline_add: bool,
    pub lunar_accounts: bool,
    pub refresh_session: bool,
}

/// What a delete resolved to, handed to the store and then back to
/// [`AccountsState::apply_removed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteOutcome {
    pub id: String,
    pub name: String,
    /// The card to select afterwards: the next one, else the previous one, else
    /// nothing (`AccountsView.xaml.cs:906-910`).
    pub neighbour_id: Option<String>,
}

/// The result of pressing Add in the offline panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineSubmit {
    /// A mutation is in flight; the press is dropped (`:645`).
    Busy,
    /// The name failed `OfflineAccount.TryCreate`; the panel shows the message.
    Invalid(&'static str),
    /// A valid offline profile: trimmed name and compact UUID.
    Ready { name: String, uuid: String },
}

/// The result of pressing Import in the paste panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteSubmit {
    Busy,
    /// The box was blank: `Paste a token or cookie export first.`
    Empty,
    /// Hand the text to the import service.
    Ready(String),
}

/// The Accounts tab's whole state.
#[derive(Debug, Clone, Default)]
pub struct AccountsState {
    rows: Vec<AccountRowState>,
    selected: Option<usize>,
    panel: AccountPanel,
    paste_text: String,
    offline_name: String,
    paste_validation: String,
    offline_validation: String,
    busy: bool,
    /// Whether the paste box has focus, so the view can draw the caret.
    paste_focused: bool,
    offline_focused: bool,
    session: SessionStrip,
    status: Option<StatusLine>,
    lunar_accounts: bool,
    recovery_notice: Option<String>,
    drop_hint: bool,
    pending_delete: Option<usize>,
}

impl AccountsState {
    /// An empty tab: no accounts, nothing capped, nothing selected.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The cards, in store order.
    #[must_use]
    pub fn rows(&self) -> &[AccountRowState] {
        &self.rows
    }

    /// Whether the empty state replaces the list (`ShowEmptyState`, `:345`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The selected card's index.
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// The selected card.
    #[must_use]
    pub fn selected_row(&self) -> Option<&AccountRowState> {
        self.rows.get(self.selected?)
    }

    /// The selected card's id.
    #[must_use]
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_row().map(|row| row.id.as_str())
    }

    /// Whether anything is selected.
    #[must_use]
    pub const fn has_selection(&self) -> bool {
        self.selected.is_some()
    }

    /// The open panel.
    #[must_use]
    pub const fn panel(&self) -> AccountPanel {
        self.panel
    }

    /// The paste box's text.
    #[must_use]
    pub fn paste_text(&self) -> &str {
        &self.paste_text
    }

    /// The offline name box's text.
    #[must_use]
    pub fn offline_name(&self) -> &str {
        &self.offline_name
    }

    /// The paste validation line, empty when clean.
    #[must_use]
    pub fn paste_validation(&self) -> &str {
        &self.paste_validation
    }

    /// The offline validation line, empty when clean.
    #[must_use]
    pub fn offline_validation(&self) -> &str {
        &self.offline_validation
    }

    /// Whether a mutation is in flight.
    #[must_use]
    pub const fn is_busy(&self) -> bool {
        self.busy
    }

    /// Whether the paste box draws a caret.
    #[must_use]
    pub const fn paste_focused(&self) -> bool {
        self.paste_focused
    }

    /// Whether the offline name box draws a caret.
    #[must_use]
    pub const fn offline_focused(&self) -> bool {
        self.offline_focused
    }

    /// The session strip.
    #[must_use]
    pub const fn session(&self) -> &SessionStrip {
        &self.session
    }

    /// The status line, `None` when hidden.
    #[must_use]
    pub const fn status(&self) -> Option<&StatusLine> {
        self.status.as_ref()
    }

    /// The Lunar checkbox (`LunarAccountsCheck`).
    #[must_use]
    pub const fn lunar_accounts(&self) -> bool {
        self.lunar_accounts
    }

    /// The store-recovery notice, `None` when there is nothing to say.
    #[must_use]
    pub fn recovery_notice(&self) -> Option<&str> {
        self.recovery_notice.as_deref()
    }

    /// Whether the drop hint is up.
    #[must_use]
    pub const fn drop_hint(&self) -> bool {
        self.drop_hint
    }

    /// The delete confirmation, while one is armed: `(name, question)`.
    #[must_use]
    pub fn delete_confirmation(&self) -> Option<(&str, String)> {
        let index = self.pending_delete?;
        let row = self.rows.get(index)?;
        Some((row.name.as_str(), delete_question(&row.name)))
    }

    /// The panel's height hint: whether the paste panel is open.
    #[must_use]
    pub const fn paste_open(&self) -> bool {
        self.panel.is_paste()
    }

    /// `UpdateActionStates` (`:1063`): every enabled flag in one value.
    #[must_use]
    pub fn actions(&self) -> AccountActions {
        let idle = !self.busy;
        let can_switch = idle
            && accounts_logic::can_switch_sessions(
                self.session.connected,
                self.session.supports_set_session,
            );
        let has_selection = self.has_selection();

        AccountActions {
            use_account: accounts_logic::can_use_selected_account(
                self.session.connected,
                self.session.supports_set_session,
                self.busy,
                has_selection,
            ),
            restore_session: can_switch,
            delete: idle && has_selection,
            import_file: idle,
            paste_import: idle,
            add_offline: idle,
            paste_authenticate: idle,
            offline_add: idle,
            lunar_accounts: idle,
            refresh_session: self.session.can_refresh(self.busy),
        }
    }

    /// `PrimaryHint`, empty when the line is hidden.
    #[must_use]
    pub fn primary_hint(&self) -> String {
        accounts_logic::primary_hint(
            self.session.connected,
            self.session.supports_set_session,
            self.has_selection(),
        )
    }

    /// The refusal a switch would hit right now, or `None`.
    #[must_use]
    pub fn switch_refusal(&self) -> Option<String> {
        accounts_logic::session_switch_refusal(
            self.session.connected,
            self.session.supports_set_session,
            false,
        )
    }

    /// `UseAccountButton.ToolTip` (`:1091`).
    #[must_use]
    pub fn use_account_tooltip(&self) -> String {
        if let Some(refusal) = self.switch_refusal() {
            return refusal;
        }

        if self.has_selection() {
            "Switch the running game to this account".to_owned()
        } else {
            tab_copy::SELECT_ACCOUNT_HINT.to_owned()
        }
    }

    /// `DeleteButton.ToolTip`.
    #[must_use]
    pub fn delete_tooltip(&self) -> String {
        if self.has_selection() {
            "Remove the saved credentials for the selected account".to_owned()
        } else {
            tab_copy::SELECT_ACCOUNT_HINT.to_owned()
        }
    }

    /// Replaces the rows the way `LoadAccounts` does: a selection survives by id
    /// only when the caller names one.
    ///
    /// Returns the index now selected. Nothing is selected unless `keep_id` is
    /// still in the list, which is the rule that stops opening the tab from arming
    /// Use account or Delete (`:294-300`).
    pub fn load(
        &mut self,
        accounts: &[StoredAccount],
        keep_id: Option<&str>,
        now_ms: i64,
    ) -> Option<usize> {
        let session_uuid = self.session.uuid().map(str::to_owned);
        let ids: Vec<String> = accounts.iter().map(|account| account.id.clone()).collect();

        self.rows = accounts
            .iter()
            .map(|account| AccountRowState::new(account, session_uuid.as_deref(), now_ms))
            .collect();

        self.selected = accounts_logic::selection_index_after_load(&ids, keep_id);
        self.pending_delete = None;
        self.selected
    }

    /// Repaints the "in use" dots after a session read, without touching the
    /// selection: `ApplySessionResult` reloads with the current card kept.
    ///
    /// `now_ms` also re-ages every card's `Last used …` line from the stamp the row kept. This
    /// used to be discarded (`let _ = now_ms;`) and the caller passed `0`, which made every card
    /// read `Last used just now` after a Refresh: a timestamp in the future is the managed
    /// `elapsed < TimeSpan.Zero` branch, so a zero clock silently rewrote the one line on the card
    /// that records the past. The sentence is the same [`accounts_logic::last_used_label`] the
    /// load uses, so the two paths cannot drift.
    pub fn refresh_session_marks(&mut self, now_ms: i64) {
        let session_uuid = self.session.uuid().map(str::to_owned);
        let keep = self.selected_id().map(str::to_owned);

        self.rows = self
            .rows
            .iter()
            .map(|row| {
                let is_current =
                    accounts_logic::is_current_session(row_uuid(row), session_uuid.as_deref());
                let mut next = row.clone();
                next.is_current = is_current;
                next.last_used_label =
                    accounts_logic::last_used_label(next.last_used_at.as_deref(), now_ms);
                next.automation_name =
                    accounts_logic::card_automation_name(&row.name, row.source_label, is_current);
                next
            })
            .collect();

        self.selected = keep
            .as_deref()
            .and_then(|id| self.rows.iter().position(|row| row.id == id));
    }

    /// Selects a card by index: what a click does (`CardsControl_SelectionChanged`).
    pub fn select_index(&mut self, index: usize) -> bool {
        if index >= self.rows.len() {
            return false;
        }
        self.selected = Some(index);
        true
    }

    /// Selects a card by id.
    pub fn select_id(&mut self, id: &str) -> bool {
        match self.rows.iter().position(|row| row.id == id) {
            Some(index) => {
                self.selected = Some(index);
                true
            }
            None => false,
        }
    }

    /// Clears the selection, as an empty click does.
    pub fn clear_selection(&mut self) {
        self.selected = None;
    }

    /// Arms or disarms a panel with `TogglePanel`'s rule (`:697`): pressing the
    /// open panel's own button closes it.
    pub fn toggle_panel(&mut self, panel: AccountPanel) -> AccountPanel {
        self.panel = if self.panel == panel {
            AccountPanel::None
        } else {
            panel
        };

        if self.panel != AccountPanel::Paste {
            self.paste_focused = false;
        }
        if self.panel != AccountPanel::Offline {
            self.offline_focused = false;
        }

        self.panel
    }

    /// Closes a panel (`HidePanel`).
    pub fn close_panel(&mut self) {
        self.panel = AccountPanel::None;
        self.paste_focused = false;
        self.offline_focused = false;
    }

    /// Sets the paste box's text and clears any stale validation, as typing does.
    pub fn set_paste_text(&mut self, text: impl Into<String>) {
        self.paste_text = text.into();
        self.paste_validation.clear();
    }

    /// Sets the offline name box's text.
    pub fn set_offline_name(&mut self, text: impl Into<String>) {
        self.offline_name = text.into();
        self.offline_validation.clear();
    }

    /// Focuses a field, for the caret.
    pub fn set_focus(&mut self, panel: AccountPanel, focused: bool) {
        match panel {
            AccountPanel::Paste => self.paste_focused = focused,
            AccountPanel::Offline => self.offline_focused = focused,
            AccountPanel::None => {
                self.paste_focused = false;
                self.offline_focused = false;
            }
        }
    }

    /// `AddOfflineAccount`'s gate and validation (`:649-684`).
    pub fn submit_offline(&mut self) -> OfflineSubmit {
        if self.busy {
            return OfflineSubmit::Busy;
        }

        match offline::try_create(Some(&self.offline_name)) {
            Ok((name, uuid)) => OfflineSubmit::Ready { name, uuid },
            Err(message) => {
                self.offline_validation = accounts_logic::INVALID_OFFLINE_NAME.to_owned();
                OfflineSubmit::Invalid(message)
            }
        }
    }

    /// Applies a saved offline account: clears the box, closes the panel, selects
    /// the new card and announces it.
    pub fn apply_offline_added(
        &mut self,
        accounts: &[StoredAccount],
        account: &StoredAccount,
        now_ms: i64,
    ) {
        self.offline_name.clear();
        self.offline_validation.clear();
        self.panel = AccountPanel::None;
        self.offline_focused = false;
        let keep = account.id.clone();
        self.load(accounts, Some(&keep), now_ms);
        self.set_status(
            format!("{} was added as an offline account.", account.name),
            StatusTone::Success,
        );
    }

    /// Reports a failed offline save: `The offline account could not be saved.`
    pub fn offline_add_failed(&mut self) {
        self.set_status(
            "The offline account could not be saved. Try again.",
            StatusTone::Critical,
        );
    }

    /// `PasteAuthenticate_Click`'s gate (`:585-600`).
    pub fn submit_paste(&mut self) -> PasteSubmit {
        if self.busy {
            return PasteSubmit::Busy;
        }

        if self.paste_text.trim().is_empty() {
            self.paste_validation = "Paste a token or cookie export first.".to_owned();
            return PasteSubmit::Empty;
        }

        self.paste_validation.clear();
        PasteSubmit::Ready(self.paste_text.clone())
    }

    /// Applies an import: clears the box, closes the panel, selects the account
    /// and shows the service's detail line.
    pub fn apply_paste_imported(
        &mut self,
        accounts: &[StoredAccount],
        imported_id: Option<&str>,
        detail: &str,
        now_ms: i64,
    ) {
        self.paste_text.clear();
        self.paste_validation.clear();
        self.panel = AccountPanel::None;
        self.paste_focused = false;
        self.load(accounts, imported_id, now_ms);
        self.set_status(detail, StatusTone::Success);
    }

    /// Applies a failed import: the service's detail line, critical.
    pub fn paste_import_failed(&mut self, detail: impl Into<String>) {
        self.set_status(detail, StatusTone::Critical);
    }

    /// `ImportFolder` failure line (`TryEnsureStores`).
    pub fn storage_unavailable(&mut self) {
        self.set_status(
            "The accounts folder could not be opened. Check permission for %AppData%\\Aoko.",
            StatusTone::Critical,
        );
    }

    /// The import dialog never opened.
    pub fn import_dialog_failed(&mut self) {
        self.set_status("The import did not start. Try again.", StatusTone::Critical);
    }

    /// The import finished without a result.
    pub fn import_unfinished(&mut self) {
        self.set_status(
            "The import did not finish. Try again.",
            StatusTone::Critical,
        );
    }

    /// Sets or clears the busy flag, which every action's enabled state reads.
    pub fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    /// Runs the delete gate (`:873-900`).
    ///
    /// Returns the armed card's id, or `None` when a mutation is in flight or
    /// nothing is selected. A missing selection reports `Select an account to
    /// switch.` the way the managed click does.
    pub fn request_delete(&mut self) -> Option<String> {
        if self.busy {
            return None;
        }

        let Some(index) = self.selected else {
            self.set_status(tab_copy::SELECT_ACCOUNT_HINT, StatusTone::Neutral);
            return None;
        };

        if index >= self.rows.len() {
            return None;
        }

        self.pending_delete = Some(index);
        Some(self.rows[index].id.clone())
    }

    /// Cancels the confirmation, as `Cancel` in the message box does.
    pub fn cancel_delete(&mut self) {
        self.pending_delete = None;
    }

    /// Consumes the confirmation: the store removes `id`, then
    /// [`Self::apply_removed`] repaints.
    pub fn confirm_delete(&mut self) -> Option<DeleteOutcome> {
        let index = self.pending_delete.take()?;
        let row = self.rows.get(index)?.clone();

        let neighbour_id = if self.rows.len() > 1 {
            if index + 1 < self.rows.len() {
                Some(self.rows[index + 1].id.clone())
            } else {
                Some(self.rows[index - 1].id.clone())
            }
        } else {
            None
        };

        Some(DeleteOutcome {
            id: row.id,
            name: row.name,
            neighbour_id,
        })
    }

    /// Repaints after a successful removal: selects the neighbour and says
    /// `{name} was deleted from this PC.`
    pub fn apply_removed(
        &mut self,
        accounts: &[StoredAccount],
        outcome: &DeleteOutcome,
        now_ms: i64,
    ) {
        self.load(accounts, outcome.neighbour_id.as_deref(), now_ms);
        self.set_status(
            format!("{} was deleted from this PC.", outcome.name),
            StatusTone::Success,
        );
    }

    /// The store said the row was already gone (`:917-923`).
    pub fn apply_remove_missing(&mut self, accounts: &[StoredAccount], now_ms: i64) {
        self.load(accounts, None, now_ms);
        self.set_status("That account was already removed.", StatusTone::Neutral);
    }

    /// A delete that failed outright.
    pub fn delete_failed(&mut self) {
        self.set_status(
            "The account could not be deleted. Try again.",
            StatusTone::Critical,
        );
    }

    /// The session read was refused or failed to start: the strip's caption in the
    /// status line, critical.
    pub fn session_read_refused(&mut self) {
        let caption = accounts_logic::session_caption(
            self.session.connected,
            self.session.supports_session_read,
        );
        self.set_status(caption, StatusTone::Critical);
    }

    /// A session read started.
    pub fn session_read_started(&mut self) -> bool {
        if self.session.refresh_in_flight {
            return false;
        }

        if !self.session.connected || !self.session.supports_session_read {
            self.session_read_refused();
            return false;
        }

        self.session.refresh_in_flight = true;
        self.set_status(
            "Reading the session from the game\u{2026}",
            StatusTone::Neutral,
        );
        true
    }

    /// A session read answered.
    ///
    /// `now_ms` reaches [`Self::refresh_session_marks`], which re-ages each card's `Last used …`
    /// line. It is a parameter for the same reason [`Self::load`]'s is: this state owns no clock,
    /// and the zero this call used to pass itself rewrote every card's stamp as `just now`.
    /// Nothing in `shell.rs` calls this yet — the bridge reply handler that will is still missing —
    /// so the extra argument is a wiring note rather than a break.
    pub fn apply_session_read(
        &mut self,
        identity: Option<SessionIdentity>,
        message: Option<&str>,
        ok: bool,
        now_ms: i64,
    ) {
        self.session.refresh_in_flight = false;

        if ok {
            self.session.identity = identity;
            self.refresh_session_marks(now_ms);
        }

        if let Some(message) = message {
            self.set_status(
                message.to_owned(),
                if ok {
                    StatusTone::Success
                } else {
                    StatusTone::Critical
                },
            );
        }
    }

    /// A session read that never finished.
    pub fn session_read_failed(&mut self) {
        self.session.refresh_in_flight = false;
        self.set_status(
            "The session could not be read. Try again from the main menu.",
            StatusTone::Critical,
        );
    }

    /// The session read was cancelled.
    pub fn session_read_cancelled(&mut self) {
        self.session.refresh_in_flight = false;
        self.set_status("The session read was cancelled.", StatusTone::Neutral);
    }

    /// Repoints the session strip, as a bridge state change does.
    pub fn set_session(&mut self, session: SessionStrip) -> bool {
        let changed = self.session != session;
        self.session = session;
        changed
    }

    /// The Lunar mirror checkbox.
    pub fn set_lunar_accounts(&mut self, on: bool) {
        self.lunar_accounts = on;
    }

    /// The Lunar mirror's result line.
    pub fn set_lunar_status(&mut self, text: impl Into<String>, tone: StatusTone) {
        self.set_status(text, tone);
    }

    /// The store-recovery notice, `None` to hide it.
    pub fn set_recovery_notice(&mut self, notice: Option<String>) {
        self.recovery_notice = notice;
    }

    /// Whether the storage path line is shown: always, from the store's directory.
    #[must_use]
    pub fn storage_path_label(&self) -> String {
        "Saved credentials are stored unencrypted on this PC.".to_owned()
    }

    /// Raises or lowers the drop hint (`SetDropHint`, `:852`).
    pub fn set_drop_hint(&mut self, visible: bool) {
        self.drop_hint = visible;
    }

    /// Sets the status line, or clears it when the text is blank
    /// (`SetStatus`, `:1101`).
    pub fn set_status(&mut self, text: impl Into<String>, tone: StatusTone) {
        let text = text.into();
        if text.trim().is_empty() {
            self.status = None;
        } else {
            self.status = Some(StatusLine::new(text, tone));
        }
    }

    /// Sets a status line whose tooltip differs from the shortened text.
    pub fn set_status_detailed(
        &mut self,
        text: impl Into<String>,
        tone: StatusTone,
        detail: impl Into<String>,
    ) {
        self.status = Some(StatusLine::with_detail(text, tone, detail));
    }

    /// Dismisses the status line.
    pub fn clear_status(&mut self) {
        self.status = None;
    }
}

/// The managed delete question, verbatim (`:901-904`).
#[must_use]
pub fn delete_question(name: &str) -> String {
    format!(
        "Delete {name}? This removes the saved credentials only. \
         It does not change the account currently active in the game."
    )
}

/// A session row's UUID, if the row still carries one.
///
/// [`AccountRowState`] keeps the dashed UUID for display; this reverses it for
/// the comparison so a repaint does not need the store again.
fn row_uuid(row: &AccountRowState) -> Option<&str> {
    row.uuid_label.strip_prefix("UUID: ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_700_000_000_000;

    fn account(id: &str, name: &str, source: &str) -> StoredAccount {
        StoredAccount {
            id: id.to_owned(),
            name: name.to_owned(),
            uuid: "069a79f444e94726a5befca90e38aaf5".to_owned(),
            access_token: String::new(),
            refresh_token: String::new(),
            source: source.to_owned(),
            added_at: None,
            last_used_at: None,
        }
    }

    fn three() -> Vec<StoredAccount> {
        vec![
            account("a", "Alpha", "localts"),
            account("b", "Beta", "offline"),
            account("c", "Gamma", "cookie"),
        ]
    }

    fn connected() -> SessionStrip {
        SessionStrip {
            connected: true,
            supports_set_session: true,
            supports_session_read: true,
            ..SessionStrip::default()
        }
    }

    #[test]
    fn loading_does_not_select_anything_unless_asked() {
        let mut state = AccountsState::new();
        state.set_session(connected());

        assert_eq!(state.load(&three(), None, NOW_MS), None);
        assert!(!state.has_selection());
        assert!(!state.actions().use_account, "nothing armed on open");
        assert!(!state.actions().delete);
        assert_eq!(state.primary_hint(), tab_copy::SELECT_ACCOUNT_HINT);
    }

    #[test]
    fn a_selection_survives_a_reload_by_id() {
        let mut state = AccountsState::new();
        state.load(&three(), None, NOW_MS);
        assert!(state.select_id("c"));

        assert_eq!(state.load(&three(), Some("c"), NOW_MS), Some(2));
        assert_eq!(state.selected_id(), Some("c"));
        assert_eq!(state.load(&three(), Some("gone"), NOW_MS), None);
    }

    #[test]
    fn the_empty_state_replaces_the_list_only_when_there_are_no_rows() {
        let mut state = AccountsState::new();
        assert!(state.is_empty());

        state.load(&three(), None, NOW_MS);
        assert!(!state.is_empty());

        state.load(&[], None, NOW_MS);
        assert!(state.is_empty());
    }

    #[test]
    fn a_selected_card_arms_use_account_and_delete_only_when_idle_and_capable() {
        let mut state = AccountsState::new();
        state.load(&three(), None, NOW_MS);
        state.select_index(0);

        assert!(!state.actions().use_account, "bridge is offline");
        state.set_session(connected());
        assert!(state.actions().use_account);
        assert!(state.actions().delete);

        state.set_busy(true);
        assert!(!state.actions().use_account);
        assert!(!state.actions().delete);
        assert!(!state.actions().import_file);
        assert!(state.primary_hint().is_empty());
    }

    #[test]
    fn an_uncapable_build_disables_the_switch_buttons_and_says_why() {
        let mut state = AccountsState::new();
        state.load(&three(), None, NOW_MS);
        state.select_index(0);
        state.set_session(SessionStrip {
            connected: true,
            supports_set_session: false,
            ..SessionStrip::default()
        });

        assert!(!state.actions().use_account);
        assert!(!state.actions().restore_session);
        assert_eq!(state.primary_hint(), tab_copy::UNSUPPORTED_ACCOUNT_SWITCH);
        assert_eq!(
            state.use_account_tooltip(),
            tab_copy::UNSUPPORTED_ACCOUNT_SWITCH
        );
    }

    #[test]
    fn selection_is_what_enables_delete_and_the_tooltip_follows() {
        let mut state = AccountsState::new();
        state.load(&three(), None, NOW_MS);

        assert_eq!(state.delete_tooltip(), tab_copy::SELECT_ACCOUNT_HINT);
        state.select_index(1);
        assert_eq!(
            state.delete_tooltip(),
            "Remove the saved credentials for the selected account"
        );
    }

    #[test]
    fn a_panel_toggles_and_closes_its_sibling() {
        let mut state = AccountsState::new();
        assert_eq!(state.toggle_panel(AccountPanel::Paste), AccountPanel::Paste);
        assert!(state.paste_open());

        assert_eq!(
            state.toggle_panel(AccountPanel::Offline),
            AccountPanel::Offline,
            "opening the other panel closes the first"
        );
        assert!(!state.paste_open());

        assert_eq!(
            state.toggle_panel(AccountPanel::Offline),
            AccountPanel::None,
            "pressing the open panel's button closes it"
        );
    }

    #[test]
    fn deleting_asks_first_and_the_question_is_the_managed_one() {
        let mut state = AccountsState::new();
        state.load(&three(), None, NOW_MS);
        state.select_index(1);

        assert_eq!(state.request_delete().as_deref(), Some("b"));
        let (name, question) = state.delete_confirmation().expect("armed");
        assert_eq!(name, "Beta");
        assert_eq!(
            question,
            "Delete Beta? This removes the saved credentials only. \
             It does not change the account currently active in the game."
        );

        state.cancel_delete();
        assert!(state.delete_confirmation().is_none());
        assert!(state.confirm_delete().is_none(), "cancelled means gone");
    }

    #[test]
    fn delete_without_a_selection_reports_the_hint_and_arms_nothing() {
        let mut state = AccountsState::new();
        state.load(&three(), None, NOW_MS);

        assert!(state.request_delete().is_none());
        assert!(state.delete_confirmation().is_none());
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some(tab_copy::SELECT_ACCOUNT_HINT)
        );
        assert_eq!(
            state.status().map(|status| status.tone),
            Some(StatusTone::Neutral)
        );
    }

    #[test]
    fn a_delete_selects_the_next_card_then_the_previous_one() {
        let mut state = AccountsState::new();
        state.load(&three(), None, NOW_MS);

        // Middle: the next card takes the selection.
        state.select_index(1);
        state.request_delete();
        let middle = state.confirm_delete().expect("armed");
        assert_eq!(middle.neighbour_id.as_deref(), Some("c"));

        // Last: the previous card takes it.
        state.select_index(2);
        state.request_delete();
        let last = state.confirm_delete().expect("armed");
        assert_eq!(last.neighbour_id.as_deref(), Some("b"), "the previous card");

        // Only card: nothing to select.
        state.load(&three()[..1], Some("a"), NOW_MS);
        state.request_delete();
        let only = state.confirm_delete().expect("armed");
        assert_eq!(only.neighbour_id, None);
    }

    #[test]
    fn applying_a_removal_repaints_and_announces_it() {
        let mut state = AccountsState::new();
        state.load(&three(), None, NOW_MS);
        state.select_index(0);
        state.request_delete();
        let outcome = state.confirm_delete().expect("armed");

        let remaining = &three()[1..];
        state.apply_removed(remaining, &outcome, NOW_MS);

        assert_eq!(state.rows().len(), 2);
        assert_eq!(state.selected_id(), Some("b"), "the neighbour is selected");
        let status = state.status().expect("status");
        assert_eq!(status.text, "Alpha was deleted from this PC.");
        assert_eq!(status.tone, StatusTone::Success);
    }

    #[test]
    fn a_row_that_was_already_gone_clears_the_selection() {
        let mut state = AccountsState::new();
        state.load(&three(), None, NOW_MS);
        state.select_index(0);

        state.apply_remove_missing(&three()[1..], NOW_MS);
        assert_eq!(state.selected_index(), None);
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some("That account was already removed.")
        );
    }

    #[test]
    fn an_offline_name_is_validated_before_anything_is_saved() {
        let mut state = AccountsState::new();
        state.toggle_panel(AccountPanel::Offline);

        state.set_offline_name("ab");
        assert!(matches!(state.submit_offline(), OfflineSubmit::Invalid(_)));
        assert_eq!(
            state.offline_validation(),
            accounts_logic::INVALID_OFFLINE_NAME
        );

        state.set_offline_name("Notch");
        assert_eq!(
            state.offline_validation(),
            "",
            "typing clears the stale message"
        );

        match state.submit_offline() {
            OfflineSubmit::Ready { name, uuid } => {
                assert_eq!(name, "Notch");
                assert_eq!(uuid.len(), 32);
            }
            other => panic!("wanted Ready, got {other:?}"),
        }
    }

    #[test]
    fn a_busy_tab_drops_an_offline_add_and_a_paste() {
        let mut state = AccountsState::new();
        state.set_busy(true);
        state.set_offline_name("Notch");
        state.set_paste_text("token");

        assert_eq!(state.submit_offline(), OfflineSubmit::Busy);
        assert_eq!(state.submit_paste(), PasteSubmit::Busy);
    }

    #[test]
    fn an_empty_paste_is_refused_with_the_managed_line() {
        let mut state = AccountsState::new();
        assert_eq!(state.submit_paste(), PasteSubmit::Empty);
        assert_eq!(
            state.paste_validation(),
            "Paste a token or cookie export first."
        );

        state.set_paste_text("  ");
        assert_eq!(state.paste_validation(), "", "typing clears the message");
        assert_eq!(
            state.submit_paste(),
            PasteSubmit::Empty,
            "whitespace is empty"
        );
    }

    #[test]
    fn an_offline_add_closes_the_panel_and_announces_the_new_card() {
        let mut state = AccountsState::new();
        state.toggle_panel(AccountPanel::Offline);
        state.set_offline_name("Notch");
        let added = account("n", "Notch", "offline");
        let mut accounts = three();
        accounts.push(added.clone());

        state.apply_offline_added(&accounts, &added, NOW_MS);

        assert_eq!(state.panel(), AccountPanel::None);
        assert_eq!(state.offline_name(), "");
        assert_eq!(state.selected_id(), Some("n"));
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some("Notch was added as an offline account.")
        );
    }

    #[test]
    fn an_import_selects_the_new_account_and_closes_the_paste_panel() {
        let mut state = AccountsState::new();
        state.toggle_panel(AccountPanel::Paste);
        state.set_paste_text("cookie-export");
        let accounts = three();

        state.apply_paste_imported(&accounts, Some("b"), "Beta imported.", NOW_MS);

        assert_eq!(state.panel(), AccountPanel::None);
        assert_eq!(state.paste_text(), "");
        assert_eq!(state.selected_id(), Some("b"));
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some("Beta imported.")
        );
    }

    #[test]
    fn the_session_strip_falls_back_through_its_three_texts() {
        let mut strip = SessionStrip::default();
        assert_eq!(strip.name_text(), "Session unknown");
        assert_eq!(
            strip.detail_text(),
            "Inject the client to read its session."
        );

        strip.connected = true;
        strip.supports_session_read = true;
        assert_eq!(strip.detail_text(), "Refresh to read it.");

        strip.identity = Some(SessionIdentity {
            name: None,
            uuid: None,
        });
        assert_eq!(strip.name_text(), "No account signed in");
        assert_eq!(strip.detail_text(), "The game reports no active session.");

        strip.identity = Some(SessionIdentity {
            name: Some("Notch".to_owned()),
            uuid: None,
        });
        assert_eq!(strip.name_text(), "Notch");
        assert_eq!(strip.detail_text(), "UUID unavailable");

        strip.identity = Some(SessionIdentity {
            name: Some("Notch".to_owned()),
            uuid: Some("069a79f444e94726a5befca90e38aaf5".to_owned()),
        });
        assert_eq!(strip.detail_text(), "069a79f4-44e9-4726-a5be-fca90e38aaf5");
    }

    #[test]
    fn a_session_read_is_gated_by_capability_and_marks_the_current_card() {
        let mut state = AccountsState::new();
        state.load(&three(), None, NOW_MS);
        assert!(state.session().chip() == SessionChipTone::Offline);

        assert!(!state.session_read_started(), "offline: nothing to read");
        assert_eq!(
            state.status().map(|status| status.tone),
            Some(StatusTone::Critical)
        );

        state.set_session(connected());
        assert!(state.session_read_started());
        assert!(state.session().refresh_in_flight);
        assert!(!state.actions().refresh_session);

        state.apply_session_read(
            Some(SessionIdentity {
                name: Some("Notch".to_owned()),
                uuid: Some("069a79f444e94726a5befca90e38aaf5".to_owned()),
            }),
            Some("Session read."),
            true,
            NOW_MS,
        );

        assert!(!state.session().refresh_in_flight);
        assert_eq!(state.session().chip(), SessionChipTone::MainMenu);
        let first = &state.rows()[0];
        assert!(first.is_current, "the dashes and case are ignored");
        assert_eq!(first.automation_name, "Alpha, Localts, in use");
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some("Session read.")
        );
    }

    /// A session read repaints the "in use" dot; it must not touch the one line on the card that
    /// records the past.
    ///
    /// The bug this pins: `refresh_session_marks` discarded its timestamp and its caller passed
    /// `0`, so every card's `Last used 3d ago` became `Last used just now` the moment a Refresh
    /// answered — a future timestamp is the managed `elapsed < TimeSpan.Zero` branch.
    #[test]
    fn a_session_read_re_ages_the_last_used_line_instead_of_clearing_it() {
        let mut state = AccountsState::new();
        let mut accounts = three();
        // 2023-11-11T22:13:20Z against NOW_MS (2023-11-14T22:13:20Z) is exactly three days.
        accounts[0].last_used_at = Some("2023-11-11T22:13:20Z".to_owned());
        state.set_session(connected());

        state.load(&accounts, None, NOW_MS);
        assert_eq!(state.rows()[0].last_used_label, "Last used 3d ago");

        assert!(state.session_read_started());
        state.apply_session_read(
            Some(SessionIdentity {
                name: Some("Notch".to_owned()),
                uuid: Some("069a79f444e94726a5befca90e38aaf5".to_owned()),
            }),
            Some("Session read."),
            true,
            NOW_MS,
        );

        assert_eq!(
            state.rows()[0].last_used_label,
            "Last used 3d ago",
            "the stamp is re-aged against the caller's clock, not against zero"
        );
        assert_eq!(
            state.rows()[1].last_used_label,
            "Never used",
            "a card with no stamp keeps its own sentence"
        );
    }

    #[test]
    fn a_failed_or_cancelled_session_read_says_so() {
        let mut state = AccountsState::new();
        state.set_session(connected());
        state.session_read_started();

        state.session_read_failed();
        assert!(!state.session().refresh_in_flight);
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some("The session could not be read. Try again from the main menu.")
        );

        state.session_read_started();
        state.session_read_cancelled();
        assert_eq!(
            state.status().map(|status| status.text.as_str()),
            Some("The session read was cancelled.")
        );
    }

    #[test]
    fn the_source_chip_and_uuid_are_the_painted_ones() {
        let state = AccountsState::new();
        let accounts = three();
        let _ = state;

        let localts = AccountRowState::new(&accounts[0], None, NOW_MS);
        assert_eq!(localts.source_label, "Localts");
        assert_eq!(localts.source_tone, AccountSourceTone::Localts);
        assert_eq!(localts.initial, "A");
        assert_eq!(
            localts.uuid_label,
            "UUID: 069a79f4-44e9-4726-a5be-fca90e38aaf5"
        );
        assert_eq!(localts.last_used_label, "Never used");

        let offline = AccountRowState::new(&accounts[1], None, NOW_MS);
        assert_eq!(offline.source_label, "Cracked");
        assert_eq!(offline.source_tone, AccountSourceTone::Offline);
    }

    #[test]
    fn a_blank_name_still_draws_an_initial() {
        assert_eq!(initial_of(Some("  notch")), "N");
        assert_eq!(initial_of(Some("   ")), "?");
        assert_eq!(initial_of(None), "?");
    }

    #[test]
    fn the_status_line_shortens_and_keeps_the_full_text_for_the_tooltip() {
        let long = "x".repeat(200);
        let status = StatusLine::with_detail(
            tab_copy::shorten(Some(&long)),
            StatusTone::Neutral,
            long.clone(),
        );

        assert!(status.short_text().chars().count() <= tab_copy::STATUS_LIMIT + 1);
        assert_eq!(status.tooltip(), long);
    }

    #[test]
    fn a_blank_status_clears_the_line() {
        let mut state = AccountsState::new();
        state.set_status("something", StatusTone::Success);
        assert!(state.status().is_some());

        state.set_status("   ", StatusTone::Success);
        assert!(state.status().is_none());
    }

    #[test]
    fn the_recovery_notice_and_drop_hint_are_state_not_visibility() {
        let mut state = AccountsState::new();
        assert!(state.recovery_notice().is_none());
        assert!(!state.drop_hint());

        state.set_recovery_notice(Some("A backup was restored.".to_owned()));
        state.set_drop_hint(true);
        assert_eq!(state.recovery_notice(), Some("A backup was restored."));
        assert!(state.drop_hint());
    }

    #[test]
    fn the_lunar_checkbox_follows_the_setting_and_the_busy_flag() {
        let mut state = AccountsState::new();
        assert!(state.actions().lunar_accounts, "idle: the checkbox works");

        state.set_lunar_accounts(true);
        assert!(state.lunar_accounts());

        state.set_busy(true);
        assert!(!state.actions().lunar_accounts);
    }

    #[test]
    fn focusing_a_field_is_per_panel_and_cleared_when_it_closes() {
        let mut state = AccountsState::new();
        state.toggle_panel(AccountPanel::Paste);
        state.set_focus(AccountPanel::Paste, true);
        assert!(state.paste_focused());

        state.toggle_panel(AccountPanel::Offline);
        assert!(!state.paste_focused(), "the paste panel closed");

        state.set_focus(AccountPanel::Offline, true);
        assert!(state.offline_focused());
        state.close_panel();
        assert!(!state.offline_focused());
    }

    #[test]
    fn the_storage_footer_text_is_the_managed_one() {
        let state = AccountsState::new();
        assert_eq!(
            state.storage_path_label(),
            "Saved credentials are stored unencrypted on this PC."
        );
    }
}
