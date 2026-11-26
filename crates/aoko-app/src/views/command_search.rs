//! Command search: open/close, the query, ranking, selection, row actions and the
//! pulse-on-jump.
//!
//! **Mounted** (section 12 decision 7): `a8479a7` wired `open_search`/`close_search` into
//! the shell's key handling and mounted [`crate::overlay`], so Ctrl+K, type-to-open and the
//! row actions reach this state machine from the running app.
//!
//! Two things here are shared with the view rather than private to it: [`Row::hint_text`],
//! the row caption, and [`pulse_card`], which joins the pulse's catalogue id to the card
//! element the ring is painted on.
//!
//! The overlay *view* is the component builder's; this is the state behind it,
//! ported from `Aoko/Views/CommandSearchOverlay.xaml.cs` (open/close, the query,
//! ranking, selection movement, row actions) and `Aoko/MainWindow.xaml.cs`
//! (`Window_PreviewTextInput`, `Window_PreviewKeyDown`, `CanOpenCommandSearch`,
//! `JumpRequested`, `ToggleRequested`).
//!
//! Ranking itself is not duplicated: [`aoko_core::search::rank`] owns it and
//! [`aoko_core::search::rows`] applies the visibility filter. What lives here is
//! everything the managed overlay did around that call.
//!
//! Three managed behaviours are deliberately kept, because each one fixed a bug:
//!
//! * **Selection follows the ranking.** The first row of every rebuild is the row
//!   Enter acts on, so typing narrows and Enter takes the best match.
//! * **Only a changed row set scrolls.** A per-keystroke `BringIntoView` queued
//!   layout work the viewport already satisfied.
//! * **The switch is refused, not hidden, on an unavailable row.** A hidden switch
//!   cannot say why the module is off; [`RowState`] carries the reason.

use std::collections::BTreeMap;

use aoko_core::search::{self, RowState, SearchEntry, SearchHit, SearchRange};

use crate::theme::motion;

/// The command-search card never narrows below this.
pub const CARD_MIN_WIDTH: f32 = 340.0;
/// The card never widens past this.
pub const CARD_MAX_WIDTH: f32 = 620.0;
/// Top anchor as a ratio of the window height.
pub const CARD_TOP_RATIO: f32 = 0.30;
/// The card sits this far inside each edge: 3 sigma of the blur radius 28 halo
/// plus the 8 px shadow depth, so the halo's edge stays off the window.
pub const CARD_EDGE_GUTTER: f32 = 72.0;
/// The list is capped here and scrolls instead of pushing the query field up.
pub const RESULTS_MAX_HEIGHT: f32 = 300.0;
/// Margin above and below the results list.
pub const RESULTS_VERTICAL_MARGIN: f32 = 12.0;
/// `ResultsArea MinHeight="100"`.
pub const RESULTS_AREA_MIN_HEIGHT: f32 = 100.0;
/// One result row.
pub const ROW_HEIGHT: f32 = 48.0;
/// Scrim fade-in.
pub const OPEN_SCRIM_MS: u32 = 180;
/// Card fade-in, and the slide from 8 px down.
pub const OPEN_CARD_MS: u32 = 140;
/// Close fade, and the 6 px slide down.
pub const CLOSE_MS: u32 = 100;
/// The list is placed, not animated, on the first layout.
pub const PLACE_EASE_MS: u32 = 120;
/// What the window knows about one module, which the row cannot read itself.
///
/// This replaces the managed `StateProvider` callback: the shell fills it from the
/// profile and the capability report, and the overlay stays a pure function of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowFacts {
    /// Whether the current bridge advertises the module.
    pub supported: bool,
    /// Whether the module's own flag reads on.
    pub is_on: bool,
    /// Formatted keybind, empty when the module is not bindable.
    pub keybind_text: String,
}

/// Per-module facts, keyed by catalogue id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowFactTable {
    facts: BTreeMap<&'static str, RowFacts>,
}

impl RowFactTable {
    /// Records one module's facts.
    pub fn set(&mut self, module_id: &'static str, facts: RowFacts) {
        self.facts.insert(module_id, facts);
    }

    /// Facts for one module; an unknown module is unsupported and off, which is
    /// what the managed fallback resolved to when the provider was absent.
    #[must_use]
    pub fn get(&self, module_id: &str) -> RowFacts {
        self.facts.get(module_id).cloned().unwrap_or_default()
    }

    /// How many modules the table knows about.
    #[must_use]
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}

/// One painted row: the ranked entry plus the settled state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The catalogue entry.
    pub entry: &'static SearchEntry,
    /// Rank tier from [`aoko_core::search`].
    pub rank: i32,
    /// Highlighted slices of the display name.
    pub name_ranges: Vec<SearchRange>,
    /// The alias that matched, when the name itself did not.
    pub matched_alias: Option<String>,
    /// What the row may show.
    pub state: RowState,
}

impl Row {
    /// The row's readable title.
    #[must_use]
    pub fn display_name(&self) -> &'static str {
        self.entry.display_name
    }

    /// Whether the row offers a switch.
    #[must_use]
    pub fn has_toggle(&self) -> bool {
        self.state.has_toggle
    }

    /// Whether the switch reads on.
    #[must_use]
    pub fn is_on(&self) -> bool {
        self.state.is_on
    }

    /// Whether activating the row can do anything.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.state.is_available
    }

    /// The row's dim caption: `CommandSearchRowViewModel.BuildHint`
    /// (`CommandSearchOverlay.xaml.cs:642-652`).
    ///
    /// An unavailable row states its reason instead of a destination — the row is where
    /// a user finds out why a module cannot run. An available one shows the tab it jumps
    /// to, as `Open the <tab> tab` for a destination and the bare tab for a module, and
    /// appends the alias or compact text that matched when the name itself did not.
    ///
    /// This is what the caption paints. `SearchEntry::card_name` is **not** copy: it is
    /// the `x:Name` of the card, which is the pulse and scroll-into-view target
    /// ([`pulse_card`]).
    #[must_use]
    pub fn hint_text(&self) -> String {
        if !self.state.is_available {
            return self.state.unavailable_reason.to_owned();
        }

        let lead = if self.entry.is_destination {
            format!("Open the {} tab", self.entry.tab)
        } else {
            self.entry.tab.to_owned()
        };
        match &self.matched_alias {
            Some(alias) => format!("{lead} \u{00b7} matches \"{alias}\""),
            None => lead,
        }
    }
}

/// The flag set that decides whether a key may open the search at all.
///
/// Port of `MainWindow.CanOpenCommandSearch`: an active keybind capture owns the
/// keyboard, so a printable character belongs to it and not to the search.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyboardGate {
    /// The window is in control mode (not the loader).
    pub control_mode: bool,
    /// `InputHooks.IsCapturingKey`: a low-level hook is swallowing keys.
    pub capturing_keybind: bool,
    /// A module keybind capture is waiting for a key.
    pub pending_module_keybind: bool,
    /// The Auto Rod action bind is waiting for a key.
    pub pending_auto_rod_action_bind: bool,
    /// The Throwpot action bind is waiting for a key.
    pub pending_throwpot_action_bind: bool,
}

impl KeyboardGate {
    /// A finished loader with nothing armed: the common case.
    #[must_use]
    pub const fn ready() -> Self {
        Self {
            control_mode: true,
            capturing_keybind: false,
            pending_module_keybind: false,
            pending_auto_rod_action_bind: false,
            pending_throwpot_action_bind: false,
        }
    }

    /// `CanOpenCommandSearch`.
    #[must_use]
    pub const fn can_open_search(&self) -> bool {
        self.control_mode
            && !self.capturing_keybind
            && !self.pending_module_keybind
            && !self.pending_auto_rod_action_bind
            && !self.pending_throwpot_action_bind
    }
}

/// A window-level key the search cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKey {
    /// Ctrl+K.
    CtrlK,
    /// Escape.
    Escape,
    /// A printable character typed outside a text field.
    Printable(char),
}

/// What a window-level key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKeyOutcome {
    /// The overlay opened; the caller must focus the query field.
    Opened,
    /// The overlay closed; the caller must restore focus.
    Closed,
    /// Not for the overlay.
    Ignored,
}

/// A key pressed while the query field has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryKey {
    Up,
    Down,
    Enter,
    /// Ctrl+Space.
    ToggleChord,
    /// F2: the same action without a chord the CJK IMEs claim.
    ToggleF2,
    Tab,
}

/// What a query-field key did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryKeyOutcome {
    /// The selection moved; scroll it into view.
    Moved(Option<usize>),
    /// Jump to this entry, then the overlay is already closed.
    Jumped(&'static SearchEntry),
    /// Ask the shell to toggle this module, then call [`CommandSearchState::refresh_rows`].
    Toggle(&'static SearchEntry),
    /// Consumed but did nothing.
    Ignored,
}

/// A jump the shell still has to perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpRequest {
    /// The entry to jump to.
    pub entry: &'static SearchEntry,
    /// Card to pulse; `None` for a tab destination or a cardless module.
    pub pulse_card: Option<&'static str>,
}

/// The accent ring drawn around a card after a jump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardPulse {
    /// Catalogue id, so a stale pulse cannot override a newer one.
    pub module_id: &'static str,
    /// When the jump happened, in milliseconds on the caller's clock.
    pub started_ms: i64,
}

impl CardPulse {
    /// Ring opacity at `now_ms`.
    ///
    /// The three keyframes are [`motion::PULSE_RISE`], [`motion::PULSE_HOLD`] and
    /// [`motion::PULSE_TOTAL`], and the curve is [`motion::card_pulse`]: this
    /// converts the overlay's millisecond clock to normalised progress and nothing
    /// else, so the ring and every other animated site share one implementation.
    #[must_use]
    pub fn opacity_at(&self, now_ms: i64) -> f32 {
        let elapsed = now_ms.saturating_sub(self.started_ms);
        let total = pulse_total_ms();
        if elapsed >= total {
            return 0.0;
        }

        motion::card_pulse(elapsed as f32 / total as f32).clamp(0.0, 1.0)
    }

    /// Whether the ring has finished and the caller may drop it.
    #[must_use]
    pub fn is_finished(&self, now_ms: i64) -> bool {
        now_ms.saturating_sub(self.started_ms) >= pulse_total_ms()
    }
}

/// The pulse's three keyframes, taken from [`motion`] rather than restated.
///
/// The curve has one implementation — [`motion::card_pulse`] — and this only
/// converts its `Duration` keyframes to the millisecond clock the overlay runs on.
/// A second copy here is what the Gate 2 review found drifting.
fn pulse_total_ms() -> i64 {
    millis(motion::PULSE_TOTAL)
}

/// One `Duration` keyframe in milliseconds, saturating rather than wrapping.
fn millis(duration: std::time::Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

/// The overlay's state.
#[derive(Debug, Default)]
pub struct CommandSearchState {
    open: bool,
    query: String,
    rows: Vec<Row>,
    selected: Option<usize>,
    scroll_request: Option<usize>,
    pulse: Option<CardPulse>,
}

impl CommandSearchState {
    /// A closed overlay with no query.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the overlay is up.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The current query.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The ranked rows.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Index of the highlighted row.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// The highlighted row.
    #[must_use]
    pub fn selected_row(&self) -> Option<&Row> {
        self.selected.and_then(|index| self.rows.get(index))
    }

    /// A row the view should scroll into view, cleared by
    /// [`CommandSearchState::take_scroll_request`].
    #[must_use]
    pub fn scroll_request(&self) -> Option<usize> {
        self.scroll_request
    }

    /// Takes the pending scroll request.
    pub fn take_scroll_request(&mut self) -> Option<usize> {
        self.scroll_request.take()
    }

    /// The live pulse, if any.
    #[must_use]
    pub fn pulse(&self) -> Option<&CardPulse> {
        self.pulse.as_ref()
    }

    /// Drops a finished pulse; returns whether one is still running.
    pub fn prune_pulse(&mut self, now_ms: i64) -> bool {
        if let Some(pulse) = &self.pulse {
            if !pulse.is_finished(now_ms) {
                return true;
            }
        }

        self.pulse = None;
        false
    }

    /// Whether the placeholder shows: the query is empty.
    #[must_use]
    pub fn placeholder_visible(&self) -> bool {
        self.query.is_empty()
    }

    /// `Open`: refuses while already open, adopts the seed, ranks, selects the
    /// first row and clears any previous pulse.
    pub fn open(&mut self, seed: Option<&str>, dev_mode: bool, facts: &RowFactTable) -> bool {
        if self.open {
            return false;
        }

        self.open = true;
        self.pulse = None;
        self.query = seed.unwrap_or("").to_owned();
        self.rebuild(dev_mode, facts);
        true
    }

    /// `Close`: refuses while already closed.
    pub fn close(&mut self) -> bool {
        if !self.open {
            return false;
        }

        self.open = false;
        self.scroll_request = None;
        true
    }

    /// `QueryBox_TextChanged`: re-ranks and re-selects.
    ///
    /// Returns whether the row set changed, which is what gates the scroll.
    pub fn set_query(&mut self, query: &str, dev_mode: bool, facts: &RowFactTable) -> bool {
        if self.query == query {
            return false;
        }

        self.query = query.to_owned();
        self.rebuild(dev_mode, facts)
    }

    /// `RefreshRows`: re-reads live state for the visible rows without re-ranking,
    /// so a keybind or capability change while the overlay is open reaches the
    /// switches. Returns how many rows changed.
    pub fn refresh_rows(&mut self, dev_mode: bool, facts: &RowFactTable) -> usize {
        if !self.open {
            return 0;
        }

        let mut changed = 0;
        for row in &mut self.rows {
            let state = resolve(row.entry, dev_mode, facts);
            if state != row.state {
                row.state = state;
                changed += 1;
            }
        }

        changed
    }

    /// Window-level keys: Ctrl+K, Escape, and type-to-open.
    pub fn handle_window_key(
        &mut self,
        key: SearchKey,
        gate: &KeyboardGate,
        dev_mode: bool,
        facts: &RowFactTable,
    ) -> SearchKeyOutcome {
        match key {
            SearchKey::CtrlK => {
                if !self.open && gate.can_open_search() && self.open(None, dev_mode, facts) {
                    SearchKeyOutcome::Opened
                } else {
                    SearchKeyOutcome::Ignored
                }
            }
            SearchKey::Escape => {
                if self.close() {
                    SearchKeyOutcome::Closed
                } else {
                    SearchKeyOutcome::Ignored
                }
            }
            SearchKey::Printable(character) => {
                // `text.Length == 1 && !char.IsControl && !char.IsWhiteSpace`, and
                // never while the overlay owns the keyboard or a field has focus.
                if self.open || character.is_control() || character.is_whitespace() {
                    return SearchKeyOutcome::Ignored;
                }

                if !gate.can_open_search() {
                    return SearchKeyOutcome::Ignored;
                }

                if self.open(Some(&character.to_string()), dev_mode, facts) {
                    SearchKeyOutcome::Opened
                } else {
                    SearchKeyOutcome::Ignored
                }
            }
        }
    }

    /// Query-field keys: the arrows, Enter, the toggle chord, F2 and Tab.
    pub fn handle_query_key(&mut self, key: QueryKey) -> QueryKeyOutcome {
        match key {
            QueryKey::Down => {
                self.move_selection(1);
                QueryKeyOutcome::Moved(self.selected)
            }
            QueryKey::Up => {
                self.move_selection(-1);
                QueryKeyOutcome::Moved(self.selected)
            }
            QueryKey::Enter => match self.selected_row().map(|row| row.entry) {
                Some(entry) => {
                    self.close();
                    QueryKeyOutcome::Jumped(entry)
                }
                None => QueryKeyOutcome::Ignored,
            },
            QueryKey::ToggleChord | QueryKey::ToggleF2 => match self.toggle_selected() {
                Some(entry) => QueryKeyOutcome::Toggle(entry),
                None => QueryKeyOutcome::Ignored,
            },
            // Focus is not moved out of the field: the rows are not tab stops.
            QueryKey::Tab => QueryKeyOutcome::Ignored,
        }
    }

    /// `MoveSelection`: wraps, and enters the list from either end.
    pub fn move_selection(&mut self, delta: i32) {
        if self.rows.is_empty() {
            return;
        }

        let count = self.rows.len();
        let next = match self.selected {
            None => {
                if delta > 0 {
                    0
                } else {
                    count - 1
                }
            }
            Some(index) => {
                let count_i = count as i64;
                let moved = (index as i64 + i64::from(delta) + count_i) % count_i;
                usize::try_from(moved).unwrap_or(0)
            }
        };

        self.selected = Some(next);
        self.scroll_request = Some(next);
    }

    /// `SetSelection` for a click on a row: no scroll, because the row is visible.
    pub fn select(&mut self, index: Option<usize>) {
        if let Some(index) = index {
            if index >= self.rows.len() {
                return;
            }
        }

        self.selected = index;
    }

    /// `Jump`: the request the shell performs, and the pulse it should draw.
    ///
    /// The overlay closes immediately; the window selects the tab, brings the card
    /// into view and pulses it when the entry owns a card.
    pub fn jump_selected(&mut self) -> Option<JumpRequest> {
        let row = self.selected_row()?;
        let entry = row.entry;
        self.close();

        Some(JumpRequest {
            entry,
            pulse_card: pulse_card_of(entry),
        })
    }

    /// `Toggle`: refuses the rows the toggle path would refuse.
    pub fn toggle_selected(&mut self) -> Option<&'static SearchEntry> {
        let row = self.selected_row()?;
        if !row.state.has_toggle || !row.state.is_available {
            return None;
        }

        Some(row.entry)
    }

    /// Starts the accent ring on the card a jump landed on.
    ///
    /// Returns `None` when there is nothing to pulse: a tab destination, a module
    /// with no card, or the motion setting is off. The card still scrolls into view
    /// in that case, unmarked.
    pub fn begin_pulse(
        &mut self,
        request: &JumpRequest,
        now_ms: i64,
        motion_enabled: bool,
    ) -> Option<&CardPulse> {
        request.pulse_card?;

        if !motion_enabled {
            return None;
        }

        self.pulse = Some(CardPulse {
            module_id: request.entry.id,
            started_ms: now_ms,
        });
        self.pulse.as_ref()
    }

    /// `BuildResultsSummary`: the live-region text.
    #[must_use]
    pub fn results_summary(&self) -> String {
        match self.rows.len() {
            0 => "No results".to_owned(),
            1 => "1 result".to_owned(),
            count => format!("{count} results"),
        }
    }

    /// `BuildEmptyMessage`: names the query, capped, and suggests three things.
    #[must_use]
    pub fn empty_message(&self) -> String {
        let trimmed = self.query.trim();
        let capped = if trimmed.chars().count() > 24 {
            let mut cut = 24;
            while cut > 0 && !trimmed.is_char_boundary(cut) {
                cut -= 1;
            }
            format!("{}...", &trimmed[..cut])
        } else {
            trimmed.to_owned()
        };

        format!("No module matches \"{capped}\". Try \"esp\", \"aim\", or a tab name.")
    }

    /// Re-ranks and settles the selection.
    ///
    /// Returns whether the row set changed, which is what the managed
    /// `SameRows` check fed into `scrollIntoView`.
    fn rebuild(&mut self, dev_mode: bool, facts: &RowFactTable) -> bool {
        let visible: Vec<Row> = search::rows(&self.query, |entry| {
            resolve(entry, dev_mode, facts).is_visible
        })
        .into_iter()
        .map(|hit| row_of(&hit, dev_mode, facts))
        .collect();

        let changed = visible.len() != self.rows.len()
            || visible
                .iter()
                .zip(&self.rows)
                .any(|(left, right)| left.entry.id != right.entry.id);

        // Same rows in the same order still get replaced, so hover and the
        // per-keystroke path stay free of layout work: only the scroll is gated on
        // `changed`, never the data.
        self.rows = visible;

        self.selected = if self.rows.is_empty() { None } else { Some(0) };
        self.scroll_request = if changed { self.selected } else { None };

        changed
    }
}

/// Builds a row from a ranked hit.
fn row_of(hit: &SearchHit, dev_mode: bool, facts: &RowFactTable) -> Row {
    let facts = facts.get(hit.entry.id);

    Row {
        entry: hit.entry,
        rank: hit.rank,
        name_ranges: hit.name_ranges.clone(),
        matched_alias: hit.matched_alias.as_ref().map(|alias| alias.to_string()),
        state: search::resolve_row(
            hit.entry,
            dev_mode,
            facts.supported,
            facts.is_on,
            facts.keybind_text,
        ),
    }
}

/// Resolves one entry's state, used by the visibility filter and by refresh.
fn resolve(entry: &SearchEntry, dev_mode: bool, facts: &RowFactTable) -> RowState {
    let facts = facts.get(entry.id);
    search::resolve_row(
        entry,
        dev_mode,
        facts.supported,
        facts.is_on,
        facts.keybind_text,
    )
}

/// The card a live pulse is ringing, if any — the XAML `x:Name` of the card, which is
/// what a card painter compares against.
///
/// [`CardPulse`] carries the *catalogue* id (`autoclicker`), and the card a jump lands
/// on is the entry's `card_name` (`AutoClickerCard`). This is the one place the two are
/// joined, so a view never has to know both vocabularies.
#[must_use]
pub fn pulse_card(state: &CommandSearchState) -> Option<&'static str> {
    let pulse = state.pulse()?;
    let entry = search::entries()
        .iter()
        .find(|entry| entry.id == pulse.module_id)?;
    pulse_card_of(entry)
}

/// The card a jump should pulse, if the entry has one.
fn pulse_card_of(entry: &SearchEntry) -> Option<&'static str> {
    if entry.is_destination || entry.card_name.is_empty() {
        return None;
    }

    Some(entry.card_name)
}

/// Card width for a window of `available_width`, from `LayoutCard`.
#[must_use]
pub fn card_width(available_width: f32) -> f32 {
    CARD_MAX_WIDTH.min(CARD_MIN_WIDTH.max(available_width - CARD_EDGE_GUTTER * 2.0))
}

/// Card top for a window of `available_height`, from `LayoutCard`.
#[must_use]
pub fn card_top(available_height: f32) -> f32 {
    CARD_EDGE_GUTTER.max(available_height * CARD_TOP_RATIO)
}

/// Results height cap for `room_for_card` with `chrome` taken off, from
/// `LimitResultsHeight`. A short window scrolls instead of sliding the card up.
#[must_use]
pub fn results_max_height(room_for_card: f32, chrome: f32) -> f32 {
    (room_for_card - chrome - RESULTS_VERTICAL_MARGIN)
        .clamp(RESULTS_AREA_MIN_HEIGHT, RESULTS_MAX_HEIGHT)
}

/// `panel` height the layout actually gets, given a window size in logical pixels.
/// Handy for the capture registry, which has to pick the same numbers as the window.
#[must_use]
pub fn results_viewport_height(available_height: f32, chrome: f32) -> f32 {
    let top = card_top(available_height);
    results_max_height(available_height - top - CARD_EDGE_GUTTER, chrome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(supported: bool, is_on: bool) -> RowFactTable {
        let mut facts = RowFactTable::default();
        for entry in search::entries() {
            facts.set(
                entry.id,
                RowFacts {
                    supported,
                    is_on,
                    keybind_text: if entry.has_keybind {
                        "V".to_owned()
                    } else {
                        String::new()
                    },
                },
            );
        }
        facts
    }

    fn open_with(seed: Option<&str>, dev_mode: bool, facts: &RowFactTable) -> CommandSearchState {
        let mut state = CommandSearchState::new();
        assert!(state.open(seed, dev_mode, facts));
        state
    }

    #[test]
    fn opening_ranks_every_visible_row_and_selects_the_first() {
        let facts = table(true, false);
        let state = open_with(None, false, &facts);

        assert!(state.is_open());
        assert_eq!(state.query(), "");
        assert!(state.placeholder_visible());
        assert!(!state.rows().is_empty());
        assert_eq!(state.selected_index(), Some(0));
        assert_eq!(
            state.selected_row().map(|row| row.entry.id),
            Some(state.rows()[0].entry.id)
        );
        assert!(
            state.rows().iter().all(|row| !row.entry.dev_only),
            "dev-only rows are hidden with DevMode off"
        );
        assert_eq!(state.scroll_request(), Some(0));
    }

    #[test]
    fn opening_twice_is_refused_so_a_keystroke_cannot_reset_the_query() {
        let facts = table(true, false);
        let mut state = open_with(Some("esp"), false, &facts);

        assert!(!state.open(Some("other"), false, &facts));
        assert_eq!(state.query(), "esp");
    }

    #[test]
    fn closing_twice_is_refused() {
        let facts = table(true, false);
        let mut state = open_with(None, false, &facts);

        assert!(state.close());
        assert!(!state.is_open());
        assert!(!state.close());
    }

    #[test]
    fn dev_mode_reveals_the_dev_only_rows() {
        let facts = table(true, true);
        let hidden = open_with(None, false, &facts);
        let shown = open_with(None, true, &facts);

        assert!(shown.rows().len() > hidden.rows().len());
        assert!(shown.rows().iter().any(|row| row.entry.id == "killaura"));
    }

    #[test]
    fn typing_reranks_and_moves_the_selection_to_the_best_match() {
        let facts = table(true, false);
        let mut state = open_with(None, false, &facts);

        assert!(state.set_query("esp", false, &facts));
        assert_eq!(state.rows()[0].entry.id, "chestesp");
        assert_eq!(state.selected_index(), Some(0));
        assert!(!state.placeholder_visible());

        // The same query twice is a no-op, so a caret move cannot re-rank.
        assert!(!state.set_query("esp", false, &facts));
    }

    #[test]
    fn a_query_with_no_match_says_so_and_selects_nothing() {
        let facts = table(true, false);
        let mut state = open_with(None, false, &facts);
        state.set_query("zzzzz", false, &facts);

        assert!(state.rows().is_empty());
        assert_eq!(state.selected_index(), None);
        assert_eq!(state.results_summary(), "No results");
        assert_eq!(
            state.empty_message(),
            "No module matches \"zzzzz\". Try \"esp\", \"aim\", or a tab name."
        );
    }

    #[test]
    fn the_empty_message_caps_a_long_query_at_24_characters() {
        let facts = table(true, false);
        let mut state = open_with(None, false, &facts);
        let long = "a".repeat(40);
        state.set_query(&long, false, &facts);

        assert_eq!(
            state.empty_message(),
            format!(
                "No module matches \"{}...\". Try \"esp\", \"aim\", or a tab name.",
                "a".repeat(24)
            )
        );
    }

    #[test]
    fn the_results_summary_pluralises_like_the_managed_switch() {
        let facts = table(true, false);
        let mut state = open_with(None, false, &facts);

        state.set_query("zzzzz", false, &facts);
        assert_eq!(state.results_summary(), "No results");

        // `blockesp` alone: one row, singular.
        state.set_query("blockesp", false, &facts);
        assert_eq!(state.rows().len(), 1);
        assert_eq!(state.results_summary(), "1 result");

        state.set_query("esp", false, &facts);
        assert!(state.rows().len() > 1);
        assert!(state.results_summary().ends_with(" results"));
    }

    #[test]
    fn the_arrows_wrap_and_enter_the_list_from_either_end() {
        let facts = table(true, false);
        let mut state = open_with(None, false, &facts);
        let count = state.rows().len();
        state.select(None);

        assert_eq!(
            state.handle_query_key(QueryKey::Down),
            QueryKeyOutcome::Moved(Some(0))
        );
        assert_eq!(
            state.handle_query_key(QueryKey::Up),
            QueryKeyOutcome::Moved(Some(count - 1)),
            "up from the first row wraps to the last"
        );
        assert_eq!(
            state.handle_query_key(QueryKey::Down),
            QueryKeyOutcome::Moved(Some(0)),
            "down from the last row wraps to the first"
        );
    }

    #[test]
    fn moving_the_selection_requests_a_scroll() {
        let facts = table(true, false);
        let mut state = open_with(None, false, &facts);
        let _ = state.take_scroll_request();

        state.handle_query_key(QueryKey::Down);
        assert_eq!(state.take_scroll_request(), Some(1));
        assert_eq!(
            state.take_scroll_request(),
            None,
            "the request is taken once"
        );
    }

    #[test]
    fn enter_jumps_to_the_highlighted_row_and_closes() {
        let facts = table(true, false);
        let mut state = open_with(Some("esp"), false, &facts);

        let outcome = state.handle_query_key(QueryKey::Enter);
        let QueryKeyOutcome::Jumped(entry) = outcome else {
            panic!("expected a jump");
        };

        assert_eq!(entry.id, "chestesp");
        assert!(!state.is_open(), "a jump closes the overlay");
    }

    #[test]
    fn enter_with_no_rows_does_nothing() {
        let facts = table(true, false);
        let mut state = open_with(Some("zzzzz"), false, &facts);

        assert_eq!(
            state.handle_query_key(QueryKey::Enter),
            QueryKeyOutcome::Ignored
        );
        assert!(state.is_open());
    }

    #[test]
    fn the_switch_is_refused_on_an_unavailable_row_but_the_row_stays() {
        let facts = table(false, false);
        let mut state = open_with(Some("nametags"), false, &facts);

        let row = state.selected_row().expect("a row");
        assert!(row.has_toggle(), "the control stays so it can say why");
        assert!(!row.is_available());
        assert!(!row.is_on());

        assert_eq!(state.toggle_selected(), None);
        assert_eq!(
            state.handle_query_key(QueryKey::ToggleChord),
            QueryKeyOutcome::Ignored
        );
    }

    #[test]
    fn an_available_row_toggles_and_f2_does_the_same() {
        let facts = table(true, false);
        let mut state = open_with(Some("nametags"), false, &facts);

        let entry = state.toggle_selected().expect("an available row");
        assert_eq!(entry.id, "nametags");

        let outcome = state.handle_query_key(QueryKey::ToggleF2);
        assert!(matches!(outcome, QueryKeyOutcome::Toggle(_)));
    }

    #[test]
    fn a_tab_destination_offers_no_switch_and_pulses_nothing() {
        let facts = table(true, false);
        let mut state = open_with(None, false, &facts);

        let destination = state
            .rows()
            .iter()
            .position(|row| row.entry.is_destination)
            .expect("destinations are searchable");
        state.select(Some(destination));

        let row = state.selected_row().expect("a row");
        assert!(
            row.is_available(),
            "a tab is not gated by the bridge module list"
        );
        assert!(!row.has_toggle());
        assert_eq!(state.toggle_selected(), None);

        let request = state.jump_selected().expect("a jump");
        assert_eq!(request.pulse_card, None);
        assert!(state.begin_pulse(&request, 0, true).is_none());
    }

    #[test]
    fn a_live_pulse_names_the_card_it_rings() {
        let facts = table(true, false);
        let mut state = open_with(Some("autoclicker"), false, &facts);
        assert_eq!(pulse_card(&state), None, "nothing has been jumped to yet");

        let request = state.jump_selected().expect("a jump");
        state.begin_pulse(&request, 1_000, true).expect("live");

        // The pulse carries the catalogue id; the card painter compares against the
        // XAML element name, so the join happens here.
        assert_eq!(pulse_card(&state), Some("AutoClickerCard"));
        assert_eq!(
            state.pulse().map(|pulse| pulse.module_id),
            Some("autoclicker")
        );

        // A tab destination never has a card, and the motion gate removes the pulse.
        let mut tab = open_with(Some("accounts"), false, &facts);
        let tab_request = tab.jump_selected().expect("a jump");
        assert!(tab.begin_pulse(&tab_request, 0, true).is_none());
        assert_eq!(pulse_card(&tab), None);

        let finished = state.pulse().expect("live").started_ms + millis(motion::PULSE_TOTAL);
        assert!(!state.prune_pulse(finished), "the ring has run out");
        assert_eq!(pulse_card(&state), None, "a pruned pulse targets nothing");
    }

    #[test]
    fn a_rows_hint_is_the_tab_the_row_jumps_to() {
        let facts = table(true, false);
        let state = open_with(Some("autoclicker"), false, &facts);
        let row = state.rows().first().expect("a row");

        assert_eq!(row.hint_text(), row.entry.tab);
        assert!(
            !row.hint_text().contains("Card"),
            "the caption is not the card's element name: {}",
            row.hint_text()
        );
    }

    #[test]
    fn a_destinations_hint_says_what_it_opens() {
        let facts = table(true, false);
        let state = open_with(Some("accounts"), false, &facts);
        let row = state.rows().first().expect("a row");

        assert!(row.entry.is_destination);
        assert_eq!(row.hint_text(), format!("Open the {} tab", row.entry.tab));
    }

    #[test]
    fn an_unavailable_rows_hint_is_its_reason() {
        let facts = table(false, false);
        let state = open_with(Some("autoclicker"), false, &facts);
        let row = state.rows().first().expect("a row");

        assert!(!row.is_available());
        assert_eq!(row.hint_text(), row.state.unavailable_reason);
        assert!(
            !row.hint_text().is_empty(),
            "a row that cannot run says why"
        );
    }

    #[test]
    fn an_alias_match_appends_what_matched() {
        // `matches "..."` only exists on the alias/compact route, so this drives the
        // entry through a synonym rather than its name. It asserts on the row that
        // matched by alias rather than on the first row, so a ranking change cannot make
        // the test assert the wrong branch.
        let facts = table(true, false);
        let state = open_with(Some("cps"), false, &facts);
        let row = state
            .rows()
            .iter()
            .find(|row| row.matched_alias.is_some())
            .expect("the synonym matched a row");

        assert_eq!(
            row.hint_text(),
            format!(
                "{} \u{00b7} matches \"{}\"",
                row.entry.tab,
                row.matched_alias.as_deref().expect("an alias matched")
            )
        );
    }

    #[test]
    fn a_jump_to_a_module_asks_for_its_card_pulse() {
        let facts = table(true, false);
        let mut state = open_with(Some("autoclicker"), false, &facts);

        let request = state.jump_selected().expect("a jump");
        assert_eq!(request.entry.id, "autoclicker");
        assert_eq!(request.pulse_card, Some("AutoClickerCard"));

        let pulse = state
            .begin_pulse(&request, 1_000, true)
            .expect("motion is on");
        assert_eq!(pulse.module_id, "autoclicker");
    }

    #[test]
    fn the_motion_setting_suppresses_the_pulse_but_not_the_jump() {
        let facts = table(true, false);
        let mut state = open_with(Some("autoclicker"), false, &facts);
        let request = state.jump_selected().expect("a jump");

        assert_eq!(request.pulse_card, Some("AutoClickerCard"));
        assert!(state.begin_pulse(&request, 1_000, false).is_none());
        assert!(state.pulse().is_none());
    }

    #[test]
    fn the_pulse_ring_follows_the_managed_keyframes() {
        let pulse = CardPulse {
            module_id: "autoclicker",
            started_ms: 0,
        };

        assert_eq!(pulse.opacity_at(0), 0.0);
        let rise = millis(motion::PULSE_RISE);
        let hold = millis(motion::PULSE_HOLD);
        let total = millis(motion::PULSE_TOTAL);
        assert!((pulse.opacity_at(rise) - 1.0).abs() < 1e-5);
        // At the hold keyframe the two implementations can differ in the last bit of
        // the f32 division, so the boundary is compared with a tolerance.
        assert!((pulse.opacity_at(hold) - 1.0).abs() < 1e-5);
        let mid_fade = pulse.opacity_at((hold + total) / 2);
        assert!(
            mid_fade > 0.0 && mid_fade < 1.0,
            "the fall is linear in between"
        );
        assert_eq!(pulse.opacity_at(total), 0.0);
        assert!(pulse.is_finished(total));
        assert!(!pulse.is_finished(total - 1));
    }

    #[test]
    fn a_finished_pulse_is_dropped() {
        let facts = table(true, false);
        let mut state = open_with(Some("autoclicker"), false, &facts);
        let request = state.jump_selected().expect("a jump");
        state.begin_pulse(&request, 0, true);

        let total = millis(motion::PULSE_TOTAL);
        assert!(state.prune_pulse(total - 1));
        assert!(!state.prune_pulse(total));
        assert!(state.pulse().is_none());
    }

    #[test]
    fn opening_clears_a_pulse_left_by_the_previous_jump() {
        let facts = table(true, false);
        let mut state = open_with(Some("autoclicker"), false, &facts);
        let request = state.jump_selected().expect("a jump");
        state.begin_pulse(&request, 0, true);
        assert!(state.pulse().is_some());

        assert!(state.open(None, false, &facts));
        assert!(state.pulse().is_none());
    }

    #[test]
    fn ctrl_k_opens_only_when_the_gate_allows_it() {
        let facts = table(true, false);
        let mut state = CommandSearchState::new();
        let gate = KeyboardGate::ready();

        assert_eq!(
            state.handle_window_key(SearchKey::CtrlK, &gate, false, &facts),
            SearchKeyOutcome::Opened
        );
        assert!(state.is_open());

        // A second Ctrl+K while open is not a toggle.
        assert_eq!(
            state.handle_window_key(SearchKey::CtrlK, &gate, false, &facts),
            SearchKeyOutcome::Ignored
        );
    }

    #[test]
    fn a_keybind_capture_owns_the_keyboard() {
        let facts = table(true, false);

        for gate in [
            KeyboardGate {
                control_mode: true,
                pending_module_keybind: true,
                ..KeyboardGate::ready()
            },
            KeyboardGate {
                capturing_keybind: true,
                ..KeyboardGate::ready()
            },
            KeyboardGate {
                pending_auto_rod_action_bind: true,
                ..KeyboardGate::ready()
            },
            KeyboardGate {
                pending_throwpot_action_bind: true,
                ..KeyboardGate::ready()
            },
            KeyboardGate {
                control_mode: false,
                ..KeyboardGate::ready()
            },
        ] {
            let mut state = CommandSearchState::new();
            assert_eq!(
                state.handle_window_key(SearchKey::CtrlK, &gate, false, &facts),
                SearchKeyOutcome::Ignored
            );
            assert_eq!(
                state.handle_window_key(SearchKey::Printable('a'), &gate, false, &facts),
                SearchKeyOutcome::Ignored
            );
            assert!(!state.is_open());
        }
    }

    #[test]
    fn a_printable_character_opens_with_itself_as_the_seed() {
        let facts = table(true, false);
        let mut state = CommandSearchState::new();

        assert_eq!(
            state.handle_window_key(
                SearchKey::Printable('e'),
                &KeyboardGate::ready(),
                false,
                &facts
            ),
            SearchKeyOutcome::Opened
        );
        assert_eq!(state.query(), "e");
        assert!(!state.rows().is_empty());
    }

    #[test]
    fn whitespace_and_control_characters_never_open_the_overlay() {
        let facts = table(true, false);

        for character in [' ', '\t', '\n', '\u{7}'] {
            let mut state = CommandSearchState::new();
            assert_eq!(
                state.handle_window_key(
                    SearchKey::Printable(character),
                    &KeyboardGate::ready(),
                    false,
                    &facts
                ),
                SearchKeyOutcome::Ignored,
                "accepted {character:?}"
            );
        }
    }

    #[test]
    fn escape_closes_and_is_ignored_when_already_closed() {
        let facts = table(true, false);
        let mut state = open_with(None, false, &facts);

        assert_eq!(
            state.handle_window_key(SearchKey::Escape, &KeyboardGate::ready(), false, &facts),
            SearchKeyOutcome::Closed
        );
        assert_eq!(
            state.handle_window_key(SearchKey::Escape, &KeyboardGate::ready(), false, &facts),
            SearchKeyOutcome::Ignored
        );
    }

    #[test]
    fn refresh_rows_reaches_a_module_that_changed_underneath() {
        let off = table(true, false);
        let mut state = open_with(Some("nametags"), false, &off);
        assert!(!state.selected_row().expect("a row").is_on());

        let on = table(true, true);
        assert_eq!(state.refresh_rows(false, &on), 1);
        assert!(state.selected_row().expect("a row").is_on());

        // Nothing changed: no notify.
        assert_eq!(state.refresh_rows(false, &on), 0);
    }

    #[test]
    fn refresh_rows_is_a_no_op_while_closed() {
        let facts = table(true, true);
        let mut state = CommandSearchState::new();

        assert_eq!(state.refresh_rows(false, &facts), 0);
    }

    #[test]
    fn the_card_geometry_matches_the_managed_layout() {
        // Clamped up at a narrow window, clamped down at a wide one, and exact in
        // between: 340 + 2 * 72 is the narrowest un-clipped card.
        assert_eq!(card_width(400.0), CARD_MIN_WIDTH);
        assert_eq!(card_width(484.0), CARD_MIN_WIDTH);
        assert_eq!(card_width(700.0), 556.0);
        assert_eq!(card_width(1200.0), CARD_MAX_WIDTH);

        assert!((card_top(760.0) - 228.0).abs() < 0.01);
        assert_eq!(card_top(100.0), CARD_EDGE_GUTTER);

        assert_eq!(results_max_height(100.0, 40.0), RESULTS_AREA_MIN_HEIGHT);
        assert_eq!(results_max_height(600.0, 40.0), RESULTS_MAX_HEIGHT);
        assert_eq!(results_max_height(200.0, 40.0), 148.0);
    }

    #[test]
    fn a_row_carries_the_ranked_highlights_through() {
        let facts = table(true, false);
        let state = open_with(Some("esp"), false, &facts);
        let first = &state.rows()[0];

        assert!(
            !first.name_ranges.is_empty(),
            "the matched slice is highlighted"
        );
        assert_eq!(first.display_name(), "Chest ESP");
    }
}
