//! The root view: client-drawn chrome, the tab rail, and the nine tab bodies.
//!
//! `MIGRATION_ARCHITECTURE.md` section 12 decision 5 makes the title bar ours:
//! themed background, a drag region, our own minimise/maximise/close hit-testing
//! and resize edges. The caption, border and text colouring that DWM did
//! (`MainWindow.ApplyNativeTitleBarTheme`, `:494-517`) has no equivalent, which is
//! why drawing the whole strip is the parity-preserving choice rather than the
//! convenient one.
//!
//! The three window actions are the verified GPUI surface at the pin:
//! `Window::minimize_window` (`crates/gpui/src/window.rs:7285`),
//! `Window::zoom_window` (`:3397`), `Window::start_window_move` (`:3410`) and
//! `App::quit` (`crates/gpui/src/app.rs:1374`).
//!
//! Two things are deliberately minimal and say so:
//!
//! * **Resize edges.** Decision 5 asks for them and the pin cannot deliver them.
//!   `Window::resize(Size)` exists (`Window::resize`, `window.rs`), and
//!   `UI_INVENTORY.md:476-484` (§5.12) records it, but there is **no per-edge
//!   hit-testing API**: nothing maps a pointer at the window's edge to a resize
//!   direction, and `window_min_size` is creation-time only, so the managed
//!   `ResizeMode`/`MinWidth` swap at runtime (`MainWindow.xaml.cs:474-483`) has no
//!   equivalent either. The strip below therefore draws and drags but does not
//!   grow a resize handle, and the four edges are left to the compositor. This is
//!   the documented gap rather than a fake resize; a real one needs a GPUI
//!   addition, not a WPF parity fix.
//! * **Persistence.** The switch rows are wired through
//!   [`aoko_core::clicker::toggles::ModuleToggles`] and the search index's
//!   `toggle_property`, which is the same reverse map the overlay uses. Sliders,
//!   checkboxes and combos resolve their `bind` path against the `Profile` through
//!   [`crate::views::bindings`] and write it back, so [`Shell::profile_json`] is
//!   the persisted shape and [`Shell::with_profile`] restores it. What is *not*
//!   done: writing that JSON to disk. There is no app-data path in the migrated
//!   crates and `main.rs` is not this builder's file, so the save call is the
//!   owner's next step, named here rather than silently skipped.

use std::collections::BTreeMap;

use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context, Entity, MouseButton, Render, SharedString, Window};

use aoko_core::clicker::toggles::{self, ModuleToggles};
use aoko_core::search;

use crate::components;
use crate::components::ButtonVariant;
use crate::theme::metrics;
use crate::theme::motion::{self, Motion};
use crate::theme::Theme;
use crate::views::accounts_state::{AccountPanel, AccountsState, OfflineSubmit, PasteSubmit};
use crate::views::command_search::{CommandSearchState, KeyboardGate, RowFactTable, SearchKey};
use crate::views::copy::{self, Card, Control, TabBody, MODULE_LIST_PREVIEW_SAMPLE};
use crate::views::servers_state::{AddOutcome, JoinOutcome, PingRequest, ServersState};
use crate::views::stores::{NoStore, StoreBridge};

/// The root view.
pub struct Shell {
    theme: Theme,
    tab: usize,
    toggles: ModuleToggles,
    /// The session override for every `bind` path: the capture harness seeds it and a
    /// press writes it. A value here wins over the profile snapshot below.
    draft: BTreeMap<&'static str, f64>,
    /// The persisted settings. Sliders, checks and combos read and write this through
    /// [`crate::views::bindings`]; see [`Shell::profile_json`] for saving it.
    profile: aoko_protocol::profile::Profile,
    /// Every profile-backed control value, by bind path, resolved once.
    values: BTreeMap<String, crate::views::bindings::ControlValue>,
    search: CommandSearchState,
    accounts: AccountsState,
    servers: ServersState,
    /// Persistence behind the two data tabs. [`NoStore`] until `aoko-core` grows
    /// an account and server store; see [`crate::views::stores`].
    stores: Box<dyn StoreBridge>,
    /// A join or account switch the UI asked for and nothing has consumed yet.
    ///
    /// The bridge command surface is not in this crate, so the request is parked
    /// here rather than dropped, which keeps [`Shell::join_direct`] honest: it
    /// either stores the request or reports why it cannot.
    pending_join: Option<String>,
    pending_account_switch: Option<String>,
}

impl Shell {
    /// A shell on the given palette, showing Combat.
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        let mut shell = Self {
            theme,
            tab: 0,
            toggles: ModuleToggles::new(),
            draft: BTreeMap::new(),
            profile: aoko_protocol::profile::Profile::default(),
            values: BTreeMap::new(),
            search: CommandSearchState::new(),
            accounts: AccountsState::new(),
            servers: ServersState::new(),
            stores: Box::new(NoStore),
            pending_join: None,
            pending_account_switch: None,
        };
        shell.reload_accounts();
        shell.reload_servers();
        shell.values = crate::views::bindings::snapshot(&shell.profile);
        shell
    }

    /// A shell that starts from a saved profile.
    #[must_use]
    pub fn with_profile(theme: Theme, profile: aoko_protocol::profile::Profile) -> Self {
        let mut shell = Self::new(theme);
        shell.values = crate::views::bindings::snapshot(&profile);
        shell.profile = profile;
        shell
    }

    /// The settings this shell is showing.
    #[must_use]
    pub fn profile(&self) -> &aoko_protocol::profile::Profile {
        &self.profile
    }

    /// The profile as the JSON `aoko-protocol` writes, ready to save.
    #[must_use]
    pub fn profile_json(&self) -> String {
        aoko_protocol::profile::to_profile_json(&self.profile)
    }

    /// The active palette.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Replaces the palette, e.g. from the Settings tab.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Index of the visible tab.
    #[must_use]
    pub fn selected_tab(&self) -> usize {
        self.tab
    }

    /// The visible tab's rail entry.
    #[must_use]
    pub fn selected_tab_name(&self) -> &'static str {
        copy::TABS.get(self.tab).map_or("", |tab| tab.name)
    }

    /// Shows one tab by index; out of range is ignored.
    pub fn select_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < copy::TABS.len() && index != self.tab {
            self.tab = index;
            cx.notify();
        }
    }

    /// Shows one tab by name, case-insensitively, as the overlay's jump does.
    pub fn select_tab_named(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(index) = copy::TABS
            .iter()
            .position(|tab| tab.name.eq_ignore_ascii_case(name))
        {
            self.select_tab(index, cx);
        }
    }

    /// Turns one module on or off. Refuses nothing: the catalogue decides what a
    /// switch may show, and an unavailable row is drawn read-only by its card.
    pub fn toggle_module(&mut self, module_id: &str, cx: &mut Context<Self>) {
        let Some(canonical) = toggles::canonical_id(module_id) else {
            return;
        };
        let next = !self.toggles.is_enabled(canonical);
        self.toggles.set_enabled(canonical, next);
        cx.notify();
    }

    /// The switch state for a card, resolved through the search index's
    /// `toggle_property` map so a card and its overlay row cannot disagree.
    #[must_use]
    pub fn is_module_on(&self, module_id: &str) -> bool {
        self.toggles.is_enabled(module_id)
    }

    /// A control value: the session override, else the profile, else `default`.
    #[must_use]
    pub fn value(&self, bind: &'static str, default: f64) -> f64 {
        if let Some(value) = self.draft.get(bind) {
            return *value;
        }

        self.values
            .get(bind)
            .map(crate::views::bindings::ControlValue::as_f64)
            .unwrap_or(default)
    }

    /// Records a control value and writes it into the profile.
    ///
    /// The profile is the persisted shape: a restart that loads it shows the value.
    /// A path with no field behind it stays in the session override, which is what the
    /// two `NON_PROFILE_BINDS` in [`crate::views::bindings`] are.
    pub fn set_value(&mut self, bind: &'static str, value: f64, cx: &mut Context<Self>) {
        self.draft.insert(bind, value);
        self.apply_control(bind, crate::views::bindings::ControlValue::Number(value));
        cx.notify();
    }

    /// Writes one value into the profile and refreshes the snapshot.
    ///
    /// Returns whether a field accepted it; a path with no field behind it keeps the
    /// session override only, which is the honest outcome for the two
    /// `NON_PROFILE_BINDS`.
    fn apply_control(
        &mut self,
        bind: &'static str,
        value: crate::views::bindings::ControlValue,
    ) -> bool {
        let written = crate::views::bindings::write(&mut self.profile, bind, value);
        if written {
            self.values = crate::views::bindings::snapshot(&self.profile);
        }
        written
    }

    /// Records a checkbox, which the profile stores as a boolean.
    pub fn set_checked(&mut self, bind: &'static str, on: bool, cx: &mut Context<Self>) {
        self.draft.insert(bind, if on { 1.0 } else { 0.0 });
        self.apply_control(bind, crate::views::bindings::ControlValue::Toggle(on));
        cx.notify();
    }

    /// Records a combo selection by index, or by text when the field stores the option.
    pub fn set_selected_index(
        &mut self,
        bind: &'static str,
        index: usize,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let stores_text = matches!(
            self.values.get(bind),
            Some(crate::views::bindings::ControlValue::Text(_))
        );

        if stores_text {
            self.set_selected_text(bind, text, cx);
        } else if crate::views::bindings::write(
            &mut self.profile,
            bind,
            crate::views::bindings::ControlValue::Number(index as f64),
        ) {
            self.values = crate::views::bindings::snapshot(&self.profile);
            cx.notify();
        } else {
            self.draft.insert(bind, index as f64);
            cx.notify();
        }
    }

    /// Records a combo selection by option text, which is how a mode field stores it.
    pub fn set_selected_text(&mut self, bind: &'static str, text: &str, cx: &mut Context<Self>) {
        self.apply_control(
            bind,
            crate::views::bindings::ControlValue::Text(text.to_owned()),
        );
        cx.notify();
    }

    /// Seeds a control value without notifying, for the capture harness.
    pub fn seed_value(&mut self, bind: &'static str, value: f64) {
        self.draft.insert(bind, value);
    }

    /// Seeds one module's switch without notifying, for the capture harness.
    pub fn seed_module(&mut self, module_id: &str, on: bool) {
        if let Some(canonical) = toggles::canonical_id(module_id) {
            self.toggles.set_enabled(canonical, on);
        }
    }

    /// The command-search state, for the overlay view to read.
    ///
    /// The overlay owns the pixels; this owns open/close, the query, ranking and
    /// the pulse — see [`crate::views::command_search`].
    #[must_use]
    pub fn search(&self) -> &CommandSearchState {
        &self.search
    }

    /// The Accounts tab's state machine, for the capture harness to seed.
    #[must_use]
    pub fn accounts(&self) -> &AccountsState {
        &self.accounts
    }

    /// The Accounts tab's state machine, mutably: seeding and test setup.
    pub fn accounts_mut(&mut self) -> &mut AccountsState {
        &mut self.accounts
    }

    /// The Servers tab's state machine.
    #[must_use]
    pub fn servers(&self) -> &ServersState {
        &self.servers
    }

    /// The Servers tab's state machine, mutably.
    pub fn servers_mut(&mut self) -> &mut ServersState {
        &mut self.servers
    }

    /// Replaces the persistence seam.
    pub fn set_stores(&mut self, stores: Box<dyn StoreBridge>, cx: &mut Context<Self>) {
        self.stores = stores;
        self.reload_accounts();
        self.reload_servers();
        cx.notify();
    }

    /// The parked join request, if any.
    #[must_use]
    pub fn pending_join(&self) -> Option<&str> {
        self.pending_join.as_deref()
    }

    /// The parked account-switch request, if any.
    #[must_use]
    pub fn pending_account_switch(&self) -> Option<&str> {
        self.pending_account_switch.as_deref()
    }

    fn reload_accounts(&mut self) {
        let accounts = self.stores.accounts();
        let now = now_millis();
        let keep = self.accounts.selected_id().map(str::to_owned);
        self.accounts.load(&accounts, keep.as_deref(), now);
    }

    fn reload_servers(&mut self) {
        let servers = self.stores.servers();
        let now = now_millis();
        let keep = self.servers.selected_id().map(str::to_owned);
        let fallback = self.servers.selected_index();
        self.servers.load(&servers, keep.as_deref(), fallback, now);
    }

    // === Accounts tab ===

    /// Selects one account card.
    pub fn select_account(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.accounts.select_index(index) {
            cx.notify();
        }
    }

    /// Opens or closes one inline panel.
    pub fn toggle_account_panel(&mut self, panel: AccountPanel, cx: &mut Context<Self>) {
        self.accounts.toggle_panel(panel);
        cx.notify();
    }

    /// Closes whichever panel is open.
    pub fn close_account_panel(&mut self, cx: &mut Context<Self>) {
        self.accounts.close_panel();
        cx.notify();
    }

    /// Focuses one panel's field, for the caret.
    pub fn focus_account_field(
        &mut self,
        panel: AccountPanel,
        focused: bool,
        cx: &mut Context<Self>,
    ) {
        self.accounts.set_focus(panel, focused);
        cx.notify();
    }

    /// "Import file": there is no file dialog at the pin, so the managed
    /// dialog-failure line is what a press reports.
    pub fn accounts_import_file(&mut self, cx: &mut Context<Self>) {
        self.accounts.import_dialog_failed();
        cx.notify();
    }

    /// "Import" in the paste panel: validates, then hands the text to an import
    /// service that does not exist in this crate yet.
    pub fn accounts_paste_import(&mut self, cx: &mut Context<Self>) {
        match self.accounts.submit_paste() {
            PasteSubmit::Ready(_text) => self.accounts.import_unfinished(),
            PasteSubmit::Busy | PasteSubmit::Empty => {}
        }
        cx.notify();
    }

    /// "Add" in the offline panel: validates, then writes through the store seam.
    pub fn accounts_add_offline(&mut self, cx: &mut Context<Self>) {
        match self.accounts.submit_offline() {
            OfflineSubmit::Ready { name, uuid } => {
                let now = now_millis();
                match self.stores.add_offline_account(&name, &uuid) {
                    Some(account) => {
                        let accounts = self.stores.accounts();
                        self.accounts.apply_offline_added(&accounts, &account, now);
                    }
                    None => self.accounts.offline_add_failed(),
                }
            }
            OfflineSubmit::Busy | OfflineSubmit::Invalid(_) => {}
        }
        cx.notify();
    }

    /// "Refresh" on the session strip: gates, then reports that the read could
    /// not be made because no bridge command surface is wired here.
    pub fn accounts_refresh_session(&mut self, cx: &mut Context<Self>) {
        if self.accounts.session_read_started() {
            self.accounts.session_read_failed();
        }
        cx.notify();
    }

    /// "Delete": arms the confirmation the tab renders.
    pub fn accounts_request_delete(&mut self, cx: &mut Context<Self>) {
        self.accounts.request_delete();
        cx.notify();
    }

    /// The confirmation's own Delete: removes through the store seam.
    pub fn accounts_confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(outcome) = self.accounts.confirm_delete() else {
            return;
        };

        let now = now_millis();
        if self.stores.remove_account(&outcome.id) {
            let accounts = self.stores.accounts();
            self.accounts.apply_removed(&accounts, &outcome, now);
        } else {
            let accounts = self.stores.accounts();
            self.accounts.apply_remove_missing(&accounts, now);
        }
        cx.notify();
    }

    /// Cancels the delete confirmation.
    pub fn accounts_cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.accounts.cancel_delete();
        cx.notify();
    }

    /// "Use account": gates, then parks the switch for the bridge.
    pub fn accounts_use_selected(&mut self, cx: &mut Context<Self>) {
        if self.accounts.actions().use_account {
            self.pending_account_switch = self.accounts.selected_id().map(str::to_owned);
        } else if let Some(refusal) = self.accounts.switch_refusal() {
            self.accounts
                .set_status(refusal, crate::views::accounts_state::StatusTone::Critical);
        } else {
            self.accounts.set_status(
                crate::views::tab_copy::SELECT_ACCOUNT_HINT,
                crate::views::accounts_state::StatusTone::Neutral,
            );
        }
        cx.notify();
    }

    /// "Restore launch account": gates the same way, then parks the request.
    pub fn accounts_restore_session(&mut self, cx: &mut Context<Self>) {
        if self.accounts.actions().restore_session {
            self.pending_account_switch = Some(String::new());
        } else if let Some(refusal) = self.accounts.switch_refusal() {
            self.accounts
                .set_status(refusal, crate::views::accounts_state::StatusTone::Critical);
        }
        cx.notify();
    }

    /// The Lunar mirror checkbox.
    pub fn toggle_lunar_accounts(&mut self, cx: &mut Context<Self>) {
        let next = !self.accounts.lunar_accounts();
        self.accounts.set_lunar_accounts(next);
        cx.notify();
    }

    /// "Open folder": no `explorer.exe` equivalent is wired here.
    pub fn accounts_open_folder(&mut self, cx: &mut Context<Self>) {
        self.accounts.set_status(
            "The accounts folder could not be opened. Check permission for %AppData%\\Aoko.",
            crate::views::accounts_state::StatusTone::Critical,
        );
        cx.notify();
    }

    // === Servers tab ===

    /// Selects one server card.
    pub fn select_server(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.servers.select_index(index) {
            cx.notify();
        }
    }

    /// Focuses or blurs the direct-address box.
    pub fn focus_direct_address(&mut self, focused: bool, cx: &mut Context<Self>) {
        self.servers.set_direct_focused(focused);
        cx.notify();
    }

    /// Focuses or blurs the add form's address box.
    pub fn focus_add_address(&mut self, focused: bool, cx: &mut Context<Self>) {
        self.servers.set_add_address_focused(focused);
        cx.notify();
    }

    /// "Join" beside the direct-address box.
    pub fn join_direct(&mut self, cx: &mut Context<Self>) {
        if let JoinOutcome::Ready(address) = self.servers.submit_direct_join() {
            self.pending_join = Some(address.display());
        }
        cx.notify();
    }

    /// "Join selected".
    pub fn join_selected(&mut self, cx: &mut Context<Self>) {
        if let JoinOutcome::Ready(address) = self.servers.submit_selected_join() {
            self.pending_join = Some(address.display());
        }
        cx.notify();
    }

    /// "Add server" and the empty state's "Add a server".
    pub fn open_add_form(&mut self, cx: &mut Context<Self>) {
        self.servers.open_add_form();
        cx.notify();
    }

    /// Closes the add form.
    pub fn close_add_form(&mut self, cx: &mut Context<Self>) {
        self.servers.close_add_form();
        cx.notify();
    }

    /// The add form's "Add": validates, then writes through the store seam.
    pub fn add_server(&mut self, cx: &mut Context<Self>) {
        match self.servers.submit_add() {
            AddOutcome::Ready { name, display } => {
                let now = now_millis();
                match self.stores.add_server(name.as_deref(), &display) {
                    Some(saved) => {
                        let servers = self.stores.servers();
                        self.servers
                            .apply_added(&servers, &saved.id, &saved.name, now);
                    }
                    None => self.servers.add_failed(),
                }
            }
            AddOutcome::Busy | AddOutcome::Invalid(_) => {}
        }
        cx.notify();
    }

    /// "Ping all": there is no ping service in this crate yet, so the pass reports
    /// the managed failure line and puts the rows back to their rest state.
    pub fn ping_all(&mut self, cx: &mut Context<Self>) {
        if let PingRequest::Ids(_ids) = self.servers.start_ping_all() {
            self.servers.ping_failed();
        }
        cx.notify();
    }

    /// One row's chip.
    pub fn ping_server(&mut self, id: String, cx: &mut Context<Self>) {
        if let PingRequest::Ids(_ids) = self.servers.start_ping_one(&id) {
            self.servers.ping_failed();
        }
        cx.notify();
    }

    /// "Delete": arms the confirmation.
    pub fn servers_request_delete(&mut self, cx: &mut Context<Self>) {
        self.servers.request_delete();
        cx.notify();
    }

    /// The confirmation's own Delete.
    pub fn servers_confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(outcome) = self.servers.confirm_delete() else {
            return;
        };

        let now = now_millis();
        if self.stores.remove_server(&outcome.id) {
            let servers = self.stores.servers();
            self.servers.apply_deleted(&servers, &outcome, now);
        } else {
            self.servers.delete_failed();
        }
        cx.notify();
    }

    /// Cancels the delete confirmation.
    pub fn servers_cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.servers.cancel_delete();
        cx.notify();
    }

    /// Opens the overlay, optionally seeded with the typed character.
    ///
    /// Ctrl+K and type-to-open both land here; they differ only in the seed.
    pub fn open_search(&mut self, seed: Option<&str>, cx: &mut Context<Self>) {
        let gate = KeyboardGate::ready();
        let facts = RowFactTable::default();
        let dev_mode = false;
        let _ = self
            .search
            .handle_window_key(SearchKey::CtrlK, &gate, dev_mode, &facts);
        if self.search.is_open() {
            if let Some(seed) = seed {
                let _ = self.search.set_query(seed, dev_mode, &facts);
            }
            cx.notify();
        }
    }

    /// Closes the overlay, as Escape and a scrim click do.
    pub fn close_search(&mut self, cx: &mut Context<Self>) {
        if self.search.close() {
            cx.notify();
        }
    }

    fn chip_text(&self) -> SharedString {
        let enabled = self.toggles.enabled_count();
        let armed = if self.toggles.armed() {
            "armed"
        } else {
            "disarmed"
        };
        SharedString::from(format!("{armed} \u{00b7} {enabled} on"))
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let motion = motion::motion();
        let theme = self.theme.clone();
        let entity = cx.entity();

        let body = match copy::TABS.get(self.tab).map(|tab| tab.body) {
            Some(TabBody::Cards(cards)) => self.render_cards(cards, &theme, motion, &entity),
            Some(TabBody::Accounts) => {
                crate::views::accounts_view::render(&self.accounts, &theme, &entity, self.tab)
            }
            Some(TabBody::Servers) => {
                crate::views::servers_view::render(&self.servers, &theme, &entity, self.tab)
            }
            None => div().flex_1().into_any_element(),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            // Decision 6: one family for the whole tree, set once at the root so it
            // inherits. Installing the fonts without asking for the family would leave
            // every measurement to whatever fontconfig happens to resolve.
            .font_family(crate::assets::UI_FAMILY)
            .child(self.render_title_bar(&theme))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .gap(px(metrics::rail::GAP))
                    .child(self.render_rail(&theme, &entity))
                    .child(div().flex().flex_1().min_w(px(0.0)).child(body)),
            )
            .into_any_element()
    }
}

impl Shell {
    /// Decision 5: the themed, draggable, hit-tested title bar.
    fn render_title_bar(&self, theme: &Theme) -> AnyElement {
        let close = crate::theme::fixed::CRITICAL;
        div()
            .id("title-bar")
            .flex()
            .flex_row()
            .items_center()
            .h(px(metrics::window::TITLE_BAR_HEIGHT))
            .w_full()
            .bg(theme.panel)
            .border_b(px(metrics::HAIRLINE))
            .border_color(theme.card_border())
            .px(px(12.0))
            .gap(px(8.0))
            .on_mouse_down(MouseButton::Left, |_, window, _| {
                // The drag region is the whole strip; the buttons sit above it and
                // consume their own clicks first.
                window.start_window_move();
            })
            .child(
                div()
                    .text_size(px(metrics::text::SECTION_TITLE))
                    .text_color(theme.text)
                    .child("Aoko"),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(format!(
                        "{} \u{00b7} {}",
                        self.selected_tab_name(),
                        self.chip_text()
                    )),
            )
            .child(
                div()
                    .id("title-bar-minimise")
                    .px(px(8.0))
                    .cursor_pointer()
                    .text_color(theme.dim_text)
                    .aria_label(copy::TITLE_BAR_MINIMISE)
                    .on_click(|_, window, _| window.minimize_window())
                    .child("\u{2013}"),
            )
            .child(
                div()
                    .id("title-bar-maximise")
                    .px(px(8.0))
                    .cursor_pointer()
                    .text_color(theme.dim_text)
                    .aria_label(copy::TITLE_BAR_MAXIMISE)
                    .on_click(|_, window, _| window.zoom_window())
                    .child("\u{25a1}"),
            )
            .child(
                div()
                    .id("title-bar-close")
                    .px(px(8.0))
                    .cursor_pointer()
                    .text_color(close)
                    .aria_label(copy::TITLE_BAR_CLOSE)
                    .on_click(|_, _, cx| cx.quit())
                    .child("\u{00d7}"),
            )
            .into_any_element()
    }

    /// The 176 px rail: search button, the nine tabs, the update row.
    fn render_rail(&self, theme: &Theme, entity: &Entity<Self>) -> AnyElement {
        let mut rail = div()
            .flex()
            .flex_col()
            .w(px(metrics::rail::WIDTH))
            .h_full()
            .py(px(10.0))
            .px(px(10.0))
            .gap(px(2.0))
            .bg(theme.background);

        rail = rail.child(
            components::button(
                "rail-search",
                theme,
                ButtonVariant::Secondary,
                format!(
                    "{}   {}",
                    copy::SEARCH_BUTTON_TOOLTIP
                        .split(" (")
                        .next()
                        .unwrap_or(copy::SEARCH_BUTTON_TOOLTIP),
                    copy::SEARCH_BUTTON_CHORD
                ),
                |_, _, _| {},
            )
            .into_any_element(),
        );

        for (index, tab) in copy::TABS.iter().enumerate() {
            rail = rail.child(self.render_rail_item(index, tab.name, theme, entity));
        }

        rail.child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(self.chip_text()),
        )
        .into_any_element()
    }

    fn render_rail_item(
        &self,
        index: usize,
        name: &'static str,
        theme: &Theme,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let selected = index == self.tab;
        let fill = if selected {
            theme.tab_selected
        } else {
            theme.background
        };
        let label = if selected { theme.text } else { theme.dim_text };
        let handle = entity.clone();

        div()
            .id(SharedString::from(format!("rail-tab-{index}")))
            .flex()
            .items_center()
            .w(px(metrics::rail::ITEM_WIDTH))
            .h(px(metrics::rail::ITEM_MIN_HEIGHT))
            .px(px(12.0))
            .rounded(px(metrics::radius::TAB_ITEM))
            .bg(fill)
            .cursor_pointer()
            .text_size(px(metrics::text::SECTION_TITLE))
            .text_color(label)
            .aria_selected(selected)
            .on_click(move |_, _, cx| {
                handle.update(cx, |shell, cx| shell.select_tab(index, cx));
            })
            .child(name)
            .into_any_element()
    }

    /// The card list for a module tab, inside its own scroll container.
    ///
    /// The scroll id is the tab index, so the same tab keeps its offset when the
    /// rail is clicked twice and two tabs never share one.
    fn render_cards(
        &self,
        cards: &'static [Card],
        theme: &Theme,
        motion: Motion,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let mut body = components::scroll(SharedString::from(format!("tab-scroll-{}", self.tab)))
            .flex_col()
            .p(px(12.0))
            .gap(px(12.0));

        for card in cards {
            body = body.child(self.render_card(card, theme, motion, entity));
        }

        body.into_any_element()
    }

    fn render_card(
        &self,
        card: &Card,
        theme: &Theme,
        motion: Motion,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let mut body = components::card(theme).child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w_full()
                .text_size(px(metrics::text::BUTTON))
                .text_color(theme.text)
                .child(SharedString::from(card_title(card))),
        );

        for control in card.controls {
            body = body.child(self.render_control(card, control, theme, motion, entity));
        }

        body.into_any_element()
    }

    fn render_control(
        &self,
        card: &Card,
        control: &Control,
        theme: &Theme,
        motion: Motion,
        entity: &Entity<Self>,
    ) -> AnyElement {
        match control {
            Control::Note { text } => div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(*text)
                .into_any_element(),

            Control::Switch { label, bind } => {
                let module = module_for_bind(bind).or_else(|| card_module(card));
                let is_on = module.is_some_and(|module| self.is_module_on(module));
                let row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .child(
                        div()
                            .text_size(px(metrics::text::BUTTON))
                            .text_color(theme.text)
                            .child(*label),
                    )
                    .child(components::switch(
                        SharedString::from(format!("{}-{}", card.id, bind)),
                        theme,
                        is_on,
                        motion,
                    ));

                row.into_any_element()
            }

            Control::Check {
                label,
                bind,
                tooltip,
            } => {
                // The closure runs after this frame, so it needs the `'static` copy
                // the table already holds rather than the borrow of `control`.
                let bind: &'static str = bind;
                let value = self.value(bind, 0.0) > 0.5;
                let id = SharedString::from(format!("{}-{}", card.id, bind));
                let handle = entity.clone();
                let boxed = components::checkbox(id, theme, value, *label, move |_, _, cx| {
                    handle.update(cx, |shell, cx| shell.set_checked(bind, !value, cx));
                });

                let mut row = div().flex().flex_col().gap(px(2.0)).child(boxed);
                if !tooltip.is_empty() {
                    row = row.child(
                        div()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(theme.dim_text)
                            .child(*tooltip),
                    );
                }
                row.into_any_element()
            }

            Control::Slider {
                label,
                bind,
                min,
                max,
                snap,
            } => {
                // The closure runs after this frame, so it needs the `'static copy
                // the table already holds rather than the borrow of `control`.
                let bind: &'static str = bind;
                let value = self.value(bind, f64::from(*min));
                let geometry = components::SliderGeometry::new(
                    f64::from(*min),
                    f64::from(*max),
                    if *snap { 1.0 } else { 0.0 },
                );
                let handle = entity.clone();
                let name = SharedString::from(format!("{}-{}", card.id, bind));

                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .w_full()
                            .child(
                                div()
                                    .text_size(px(metrics::text::CAPTION))
                                    .text_color(theme.text)
                                    .child(*label),
                            )
                            .child(
                                div()
                                    .text_size(px(metrics::text::CAPTION))
                                    .text_color(theme.dim_text)
                                    .child(SharedString::from(format_value(value))),
                            ),
                    )
                    .child(components::slider(
                        name,
                        theme,
                        geometry,
                        value,
                        move |next, _window, cx| {
                            handle.update(cx, |shell, cx| shell.set_value(bind, next, cx));
                        },
                    ))
                    .into_any_element()
            }

            Control::Combo {
                label,
                bind,
                options,
            } => {
                // The closure runs after this frame, so it needs the `'static copies the
                // table already holds rather than this borrow.
                let bind: &'static str = bind;
                let options: &'static [&'static str] = options;

                // The setting is either an index or the option text; the profile decides
                // which, so the press converts back through the same table.
                let stored = self
                    .values
                    .get(bind)
                    .cloned()
                    .unwrap_or(crate::views::bindings::ControlValue::Number(0.0));
                let selected = match &stored {
                    crate::views::bindings::ControlValue::Text(text) => options
                        .iter()
                        .position(|option| option.eq_ignore_ascii_case(text))
                        .unwrap_or(0),
                    other => other.as_f64() as usize,
                };
                let clamped = selected.min(options.len().saturating_sub(1));
                let current = options.get(clamped).copied().unwrap_or("");
                let next = if options.is_empty() {
                    0
                } else {
                    (clamped + 1) % options.len()
                };
                let next_text = options.get(next).copied().unwrap_or("");
                let options_empty = options.is_empty();
                let handle = entity.clone();
                let trigger = components::combo(
                    SharedString::from(format!("{}-combo-{bind}", card.id)),
                    theme,
                    SharedString::from(current),
                    move |_, _, cx| {
                        if options_empty {
                            return;
                        }
                        handle.update(cx, |shell, cx| {
                            shell.set_selected_index(bind, next, next_text, cx);
                        });
                    },
                );

                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .w_full()
                    .child(
                        div()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(theme.text)
                            .child(*label),
                    )
                    .child(trigger)
                    .into_any_element()
            }

            Control::Keybind { module } => components::button(
                SharedString::from(format!("{}-keybind", card.id)),
                theme,
                ButtonVariant::Secondary,
                SharedString::from(keybind_label(module)),
                |_, _, _| {},
            )
            .into_any_element(),

            Control::Button { label, tooltip } => components::button(
                SharedString::from(format!("{}-{label}", card.id)),
                theme,
                if tooltip.is_empty() {
                    ButtonVariant::Dark
                } else {
                    ButtonVariant::Danger
                },
                SharedString::from(*label),
                |_, _, _| {},
            )
            .into_any_element(),

            Control::ActionBind { which } => components::button(
                SharedString::from(format!("{}-{which:?}", card.id)),
                theme,
                ButtonVariant::Secondary,
                SharedString::from("Action: Unbound"),
                |_, _, _| {},
            )
            .into_any_element(),

            Control::Availability { text, bind } => div()
                .id(SharedString::from(*bind))
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .aria_label(SharedString::from(*bind))
                .child(*text)
                .into_any_element(),

            Control::Expand { header } => div()
                .mt(px(6.0))
                .text_size(px(metrics::text::BUTTON))
                .text_color(theme.text)
                .child(SharedString::from(format!("\u{25b8} {header}")))
                .into_any_element(),

            Control::BlockEspRows => div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child("Tracked blocks: add a block to start.")
                .into_any_element(),

            Control::PalettePreview { name } => {
                let preview = Theme::by_name(name);
                div()
                    .id(SharedString::from(format!("palette-{name}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .child(
                        div()
                            .w(px(22.0))
                            .h(px(22.0))
                            .rounded(px(3.0))
                            .bg(preview.accent),
                    )
                    .child(
                        div()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(theme.text)
                            .child(SharedString::from(*name)),
                    )
                    .into_any_element()
            }
        }
    }
}

/// Renders a value the way the markup does: whole numbers with no decimals,
/// fractional ones with one or two.
fn format_value(value: f64) -> String {
    if (value - value.round()).abs() < 0.0005 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.2}")
    }
}

/// The card's title: the first `Note` that reads like a heading, else the module
/// name its keybind names, else the id.
fn card_title(card: &Card) -> String {
    if let Some(module) = card_module(card) {
        if let Some(entry) = search::entries().iter().find(|entry| entry.id == module) {
            return entry.display_name.to_owned();
        }
    }

    card.id.trim_end_matches("Card").to_owned()
}

/// The catalogue module a card's keybind button names.
fn card_module(card: &Card) -> Option<&'static str> {
    card.controls.iter().find_map(|control| match control {
        Control::Keybind { module } => Some(*module),
        _ => None,
    })
}

/// The module a switch's managed binding path belongs to.
///
/// The search index already carries the pairing (`SearchEntry::toggle_property`),
/// so this is the same reverse map the overlay's row state uses; duplicating it
/// would let a card and its row disagree.
fn module_for_bind(bind: &'static str) -> Option<&'static str> {
    search::entries()
        .iter()
        .find(|entry| entry.toggle_property.eq_ignore_ascii_case(bind))
        .map(|entry| entry.id)
}

/// The keybind button's rest label: the markup shows the formatted key, and an
/// unbound module reads `Unbound` here (the capture never seeds keybinds).
fn keybind_label(_module: &'static str) -> &'static str {
    "Unbound"
}

/// So `MODULE_LIST_PREVIEW_SAMPLE` stays referenced: the Settings tab draws the
/// live module-list sample row from it once the preview rows are wired.
#[must_use]
pub fn module_list_sample() -> &'static str {
    MODULE_LIST_PREVIEW_SAMPLE
}

/// Milliseconds since the Unix epoch, for the `Last used` / `Last joined` lines.
fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_opens_on_combat_and_selects_by_index_and_name() {
        let shell = Shell::new(Theme::slate());
        assert!(matches!(
            copy::TABS[shell.selected_tab()].body,
            TabBody::Cards(_)
        ));
        assert_eq!(shell.selected_tab_name(), "Combat");
    }

    #[test]
    fn every_tab_name_selects_itself() {
        for (index, tab) in copy::TABS.iter().enumerate() {
            let position = copy::TABS
                .iter()
                .position(|candidate| candidate.name.eq_ignore_ascii_case(tab.name))
                .expect("every tab has a name");
            assert_eq!(position, index);
        }
    }

    #[test]
    fn cards_are_titled_by_their_catalogue_display_name() {
        let combat = match copy::TABS[0].body {
            TabBody::Cards(cards) => cards,
            _ => unreachable!(),
        };

        assert_eq!(card_title(&combat[0]), "AutoClicker");
        assert_eq!(card_title(&combat[1]), "Right Click");
    }

    #[test]
    fn a_switch_binding_resolves_to_its_catalogue_module() {
        assert_eq!(module_for_bind("IsArmed"), Some("autoclicker"));
        assert_eq!(module_for_bind("NametagsEnabled"), Some("nametags"));
        assert_eq!(module_for_bind("NoSuchFlag"), None);
    }

    #[test]
    fn a_control_reads_the_profile_then_the_session_override() {
        let mut shell = Shell::new(Theme::slate());
        // `Profile::default` sets MinCPS to 8: the profile wins over a caller default.
        assert_eq!(shell.value("MinCPS", 12.0), 8.0);

        shell.draft.insert("MinCPS", 14.0);
        assert_eq!(shell.value("MinCPS", 12.0), 14.0, "the override wins");

        // A path the profile does not carry still falls back to the default.
        let mut shell = Shell::new(Theme::slate());
        shell.draft.clear();
        assert_eq!(shell.value("ConfigSelection", 3.0), 3.0);
    }

    #[test]
    fn a_slider_change_persists_into_the_profile_across_a_reload() {
        let mut shell = Shell::new(Theme::slate());

        assert!(shell.apply_control("MinCPS", crate::views::bindings::ControlValue::Number(13.0)));
        assert!((shell.profile().min_cps - 13.0).abs() < f32::EPSILON);

        // The saved JSON is what a restart reads back.
        let saved = shell.profile_json();
        let profile = aoko_protocol::profile::load_profile_from_json(&saved).expect("profile");
        let restored = Shell::with_profile(Theme::slate(), profile);
        assert_eq!(restored.value("MinCPS", 0.0), 13.0);
    }

    #[test]
    fn a_checkbox_and_a_combo_write_their_own_shapes() {
        let mut shell = Shell::new(Theme::slate());

        assert!(shell.apply_control(
            "AimAssistEnabled",
            crate::views::bindings::ControlValue::Toggle(true)
        ));
        assert!(shell.profile().aim_assist_enabled);

        assert!(shell.apply_control(
            "KillAuraSettings.CpsMode",
            crate::views::bindings::ControlValue::Text("Record".to_owned())
        ));
        assert_eq!(shell.profile().kill_aura_settings.cps_mode(), "record");
    }

    #[test]
    fn formatted_values_drop_useless_decimals() {
        assert_eq!(format_value(12.0), "12");
        assert_eq!(format_value(4.5), "4.50");
        assert_eq!(format_value(0.0), "0");
    }
}
