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
//! Two things about this file are worth knowing before reading it:
//!
//! * **Resize edges.** Decision 5 asks for them, and they are drawn here rather
//!   than deferred: `resize_strips` places four 4 px edges and four 10 px corners
//!   over the client area, and each one calls `Window::start_window_resize(edge)`
//!   (`crates/gpui/src/window.rs:2873`; `ResizeEdge` has all eight variants at
//!   `platform.rs:850`, and both Linux backends implement it). What GPUI does not
//!   give us is *automatic* edge hit-testing — the old note here claimed resize was
//!   impossible at the pin, which was false — so the strips exist to be the hit
//!   targets. `window_min_size` remains creation-time only, which is why the managed
//!   `ResizeMode`/`MinWidth` swap at runtime (`MainWindow.xaml.cs:474-483`) is not
//!   ported.
//! * **Persistence.** Sliders, checkboxes and combos resolve their `bind` path
//!   against the `Profile` through [`crate::views::bindings`] and write it back;
//!   switches go through [`crate::views::bindings`]' reverse map into
//!   [`ModuleToggles`] on the press. [`Shell::new`] is the startup path: it opens
//!   [`crate::views::profiles::Profiles`], migrates legacy profiles, reads the
//!   auto-save slot and applies it, and the two close paths write it back — the same
//!   cadence as `MainWindow.xaml.cs:407-412` (load once) and `:1861-1871` (save on
//!   exit), with `:946-970` for a named save. Decision 3's animation setting is
//!   *not* in the profile: it lives in [`crate::views::prefs`], because `Profile` is
//!   a bridge wire type.

use std::collections::BTreeMap;

use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context, Entity, MouseButton, Render, SharedString, Window};

use aoko_core::clicker::toggles::{self, ModuleToggles};
use aoko_core::search;

use crate::components;
use crate::components::text_input::TextInput;
use crate::components::ButtonVariant;
use crate::theme::metrics;
use crate::theme::motion::{self, Motion};
use crate::theme::Theme;
use crate::views::accounts_state::{AccountPanel, AccountsState, OfflineSubmit, PasteSubmit};
use crate::views::command_search::{CommandSearchState, KeyboardGate, RowFactTable, SearchKey};
use crate::views::copy::{self, Card, Control, TabBody, MODULE_LIST_PREVIEW_SAMPLE};
use crate::views::forms::FormFields;
use crate::views::prefs::ClientPrefs;
use crate::views::profiles::{CreateOutcome, Profiles};
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
    /// The profile store, the config list and the Settings card's status line.
    profiles: Profiles,
    /// The six text fields. Created on the first render, because an entity needs a
    /// context and [`Shell::new`] has none (`main.rs` calls `cx.new`, which is the
    /// components owner's file).
    fields: FormFields,
    /// The new-config name box's text (`NewConfigNameBox`, `MainWindow.xaml:1552`).
    config_name: String,
    /// Whether the session can hand a folder to the file manager
    /// (`CapabilityReport::open_path`). False until [`Shell::from_system`] probes it,
    /// so the button cannot claim a capability this build has not checked.
    open_configs_folder: bool,
    /// Decision 3's client setting: whether animated sites run. Kept in
    /// `client-prefs.json` rather than `Profile`, which is a bridge wire type.
    animations: bool,
    /// The preferences file, `None` under `cfg(test)` and `capture` so neither
    /// touches the machine's app-data directory.
    prefs: Option<crate::views::prefs::PrefsFile>,
    /// Why a preference could not be read or written, shown in the Settings card.
    prefs_status: String,
    /// The last resize edge a strip asked for. The platform owns the actual resize
    /// (`Window::start_window_resize`), so this is the only part a test can observe,
    /// and it is what proves the strip is hit-testable.
    resize_requested: Option<gpui::ResizeEdge>,
    /// Whether the close hook has been registered on this window, so a repaint does
    /// not box a new callback every frame.
    close_hook_installed: bool,
    /// A join or account switch the UI asked for and nothing has consumed yet.
    ///
    /// The bridge command surface is not in this crate, so the request is parked
    /// here rather than dropped, which keeps [`Shell::join_direct`] honest: it
    /// either stores the request or reports why it cannot.
    pending_join: Option<String>,
    pending_account_switch: Option<String>,
    /// The title bar's strings, with the values they were built from.
    chrome: ChromeCache,
}

/// The two strings the chrome shows, cached against what they say.
///
/// `Render::render` asks for the chip text twice — the title bar and the rail footer —
/// and for the title once. Both change only when a toggle or the tab does, so keeping
/// the last value with its inputs turns three `format!`s per frame into two
/// `SharedString` clones, which allocate nothing.
#[derive(Default)]
struct ChromeCache {
    /// `armed · 7 on`, keyed by (armed, enabled count).
    chip: Option<((bool, usize), SharedString)>,
    /// `Combat · armed · 7 on`, keyed by (tab index, chip text).
    title: Option<((usize, SharedString), SharedString)>,
}

impl Shell {
    /// A shell on the given palette, showing Combat.
    ///
    /// In the shipped binary this is the startup path and it **loads the auto-save
    /// profile**, exactly as `MainWindow.xaml.cs:407-412` does: the managed app read
    /// the `config` slot once at construction and never reloaded per tab. Under
    /// `cfg(test)` and under the `capture` feature it deliberately does no I/O, so a
    /// unit test and a screenshot never depend on the machine's profile directory.
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        #[cfg(all(not(test), not(feature = "capture")))]
        {
            Self::from_system(theme)
        }

        #[cfg(any(test, feature = "capture"))]
        {
            Self::with_profiles(theme, Profiles::unavailable())
        }
    }

    /// The startup shell: opens the profile store, migrates legacy profiles, loads
    /// the auto-save slot and reports what happened.
    ///
    /// A store that cannot be opened does not stop the app and is not replaced by a
    /// silent default: [`Profiles::store_error`] carries the classified reason and
    /// the Settings card shows it.
    #[must_use]
    pub fn from_system(theme: Theme) -> Self {
        Self::start(
            theme,
            Profiles::from_system(),
            crate::views::prefs::PrefsFile::from_system(),
        )
    }

    /// The startup body, with both I/O sources handed in.
    ///
    /// `from_system` is the only caller in the shipped binary; tests call this with a
    /// temporary store and a temporary preferences file, which is what pins the
    /// startup *read*. Before this seam, `cfg(test)` skipped the whole block, so
    /// deleting the gate write from startup left a green suite.
    fn start(
        theme: Theme,
        (profiles, profile): (Profiles, aoko_protocol::profile::Profile),
        prefs: Result<crate::views::prefs::PrefsFile, aoko_platform::PlatformError>,
    ) -> Self {
        if profiles.migrated() > 0 {
            tracing::info!(
                "copied {} legacy profile(s) into the profile directory",
                profiles.migrated()
            );
        }

        let mut shell = Self::with_profiles(theme, profiles).with_loaded_profile(profile);
        // `CapabilityReport::open_path` is the flag the "Open Configs Folder" row is
        // keyed on: Linux needs `xdg-open` on `PATH`, and a session without it must
        // not look like it can reveal the folder.
        shell.open_configs_folder = aoko_platform::probe_capabilities().open_path;

        // Section 12 decision 3: the client setting that gates every animation. It is
        // applied to `theme::motion` here, before the first frame, so a user who
        // turned animations off does not see one animated frame on startup.
        match prefs {
            Ok(file) => {
                match file.load() {
                    Ok(prefs) => {
                        shell.animations = prefs.animations;
                        motion::set_animations_enabled(prefs.animations);
                    }
                    Err(error) => {
                        shell.prefs_status = format!("Could not read client preferences: {error}");
                    }
                }
                shell.prefs = Some(file);
            }
            Err(error) => {
                shell.prefs_status = format!(
                    "Could not locate client preferences: {}",
                    error.user_facing()
                );
            }
        }

        shell
    }

    /// Sets whether this session can reveal a folder in the file manager.
    ///
    /// The startup path fills this from `aoko_platform::probe_capabilities()`; tests
    /// say it outright so their result does not depend on whether the machine has
    /// `xdg-open`.
    pub fn set_open_folder_supported(&mut self, supported: bool) {
        self.open_configs_folder = supported;
    }

    /// Attaches a preferences file, as the startup path does. Tests use it to prove
    /// a flipped animation setting reaches the disk.
    pub fn set_prefs(&mut self, prefs: crate::views::prefs::PrefsFile) {
        self.prefs = Some(prefs);
    }

    /// A shell over a caller-supplied store: tests, and an app-data root chosen
    /// elsewhere.
    #[must_use]
    pub fn with_store(theme: Theme, store: aoko_platform::ProfileStore) -> Self {
        let (profiles, profile) = Profiles::with_store(store);
        Self::with_profiles(theme, profiles).with_loaded_profile(profile)
    }

    /// A shell with no persistence, which is what the capture harness and the unit
    /// tests run.
    #[must_use]
    pub fn with_profiles(theme: Theme, profiles: Profiles) -> Self {
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
            profiles,
            fields: FormFields::default(),
            config_name: String::new(),
            open_configs_folder: false,
            // Under `cfg(test)` and `capture` nothing has changed the process gate, so
            // the row shows what the leaf widgets are already doing.
            animations: motion::animations_enabled(),
            prefs: None,
            prefs_status: String::new(),
            resize_requested: None,
            close_hook_installed: false,
            pending_join: None,
            pending_account_switch: None,
            chrome: ChromeCache::default(),
        };
        shell.reload_accounts();
        shell.reload_servers();
        shell.values = crate::views::bindings::snapshot(&shell.profile);
        shell
    }

    /// Replaces the loaded profile, which is what applying a saved config does.
    fn with_loaded_profile(mut self, profile: aoko_protocol::profile::Profile) -> Self {
        self.values = crate::views::bindings::snapshot(&profile);
        self.profile = profile;
        self
    }

    /// A shell that starts from a saved profile, with no store attached.
    #[must_use]
    pub fn with_profile(theme: Theme, profile: aoko_protocol::profile::Profile) -> Self {
        Self::with_profiles(theme, Profiles::unavailable()).with_loaded_profile(profile)
    }

    /// The profile store's state: the config list, the status line, the migration
    /// count and any store error.
    #[must_use]
    pub fn profiles(&self) -> &Profiles {
        &self.profiles
    }

    /// Writes the auto-save slot. The managed app does this on exit
    /// (`MainWindow.xaml.cs:1866-1869`), which is why the close paths call it.
    pub fn save_profile(&mut self) -> Result<(), String> {
        let mut profile = self.profile.clone();
        let result = self.profiles.save_auto(&mut profile);
        self.profile = profile;
        result
    }

    /// The Settings card's Save: writes the selected config name.
    pub fn save_config(&mut self, name: &str) -> Result<(), String> {
        let mut profile = self.profile.clone();
        let result = self.profiles.save_named(name, &mut profile);
        self.profile = profile;
        result
    }

    /// The Settings card's Load: replaces the live settings with a saved config.
    pub fn load_config(&mut self, name: &str) -> Result<(), String> {
        match self.profiles.load_named(name) {
            Ok(profile) => {
                self.values = crate::views::bindings::snapshot(&profile);
                self.profile = profile;
                // The palette travels in the profile, so applying a config has to
                // repaint with the theme it names (`ApplyLoadedProfile`,
                // `MainWindow.xaml.cs:835-848`).
                self.theme = Theme::by_name(&self.profile.gui_theme);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// Registers the close hook once, so a platform close (Alt+F4) saves the way
    /// our own close button does.
    fn ensure_close_hook(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_hook_installed {
            return;
        }
        self.close_hook_installed = true;

        let handle = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_window, cx| {
            if let Some(shell) = handle.upgrade() {
                shell.update(cx, |shell, _cx| {
                    let _ = shell.save_profile();
                });
            }
            // Never block the close: the save failing is reported in the Settings
            // card, not by trapping the user in a window they asked to close.
            true
        });
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
        // The fields carry their own copy of the palette, so they are recreated on
        // the next render. Their values live in the state machines, which is why
        // dropping the entities loses nothing.
        self.fields.clear();
        cx.notify();
    }

    // === Text fields ===

    /// Creates whichever text fields do not exist yet, wired to the state they view.
    ///
    /// Called at the top of [`Render::render`], which is the first point with both a
    /// `Context<Shell>` and the theme. Each field is created once; [`Self::sync`]
    /// pushes state changes back into it.
    fn ensure_form_fields(&mut self, cx: &mut Context<Self>) {
        let theme = self.theme.clone();

        if self.fields.token.is_none() {
            let value = self.accounts.paste_text().to_owned();
            let handle = cx.entity();
            let input = cx.new(|cx| {
                TextInput::new(
                    cx,
                    theme.clone(),
                    crate::views::accounts_view::copy::PASTE_PLACEHOLDER,
                )
                .masked(true)
                .on_change(move |text, _window, cx| {
                    let text = text.to_string();
                    handle.update(cx, |shell, cx| shell.set_token_text(text, cx));
                })
            });
            if !value.is_empty() {
                input.update(cx, |input, cx| input.set_text(value, cx));
            }
            self.fields.token = Some(input);
        }

        if self.fields.offline_name.is_none() {
            let value = self.accounts.offline_name().to_owned();
            let handle = cx.entity();
            let input = cx.new(|cx| {
                TextInput::new(cx, theme.clone(), "").on_change(move |text, _window, cx| {
                    let text = text.to_string();
                    handle.update(cx, |shell, cx| shell.set_offline_name_text(text, cx));
                })
            });
            if !value.is_empty() {
                input.update(cx, |input, cx| input.set_text(value, cx));
            }
            self.fields.offline_name = Some(input);
        }

        if self.fields.server_name.is_none() {
            let value = self.servers.add_name().to_owned();
            let handle = cx.entity();
            let input = cx.new(|cx| {
                TextInput::new(
                    cx,
                    theme.clone(),
                    crate::views::servers_view::copy::NAME_LABEL,
                )
                .on_change(move |text, _window, cx| {
                    let text = text.to_string();
                    handle.update(cx, |shell, cx| shell.set_server_name_text(text, cx));
                })
            });
            if !value.is_empty() {
                input.update(cx, |input, cx| input.set_text(value, cx));
            }
            self.fields.server_name = Some(input);
        }

        if self.fields.server_address.is_none() {
            let value = self.servers.add_address().to_owned();
            let handle = cx.entity();
            let input = cx.new(|cx| {
                TextInput::new(
                    cx,
                    theme.clone(),
                    crate::views::servers_view::copy::ADDRESS_LABEL,
                )
                .on_change(move |text, _window, cx| {
                    let text = text.to_string();
                    handle.update(cx, |shell, cx| shell.set_server_address_text(text, cx));
                })
            });
            if !value.is_empty() {
                input.update(cx, |input, cx| input.set_text(value, cx));
            }
            self.fields.server_address = Some(input);
        }

        if self.fields.direct_address.is_none() {
            let value = self.servers.direct_address().to_owned();
            let handle = cx.entity();
            let input = cx.new(|cx| {
                TextInput::new(cx, theme.clone(), "host or host:port").on_change(
                    move |text, _window, cx| {
                        let text = text.to_string();
                        handle.update(cx, |shell, cx| shell.set_direct_address_text(text, cx));
                    },
                )
            });
            if !value.is_empty() {
                input.update(cx, |input, cx| input.set_text(value, cx));
            }
            self.fields.direct_address = Some(input);
        }

        if self.fields.config_name.is_none() {
            let value = self.config_name.clone();
            let handle = cx.entity();
            let input = cx.new(|cx| {
                TextInput::new(cx, theme.clone(), "").on_change(move |text, _window, cx| {
                    let text = text.to_string();
                    handle.update(cx, |shell, cx| shell.set_config_name_text(text, cx));
                })
            });
            if !value.is_empty() {
                input.update(cx, |input, cx| input.set_text(value, cx));
            }
            self.fields.config_name = Some(input);
        }
    }

    /// Pushes the state's text into every field the user is not typing in.
    fn sync_form_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::views::forms::sync(&self.fields.token, self.accounts.paste_text(), window, cx);
        crate::views::forms::sync(
            &self.fields.offline_name,
            self.accounts.offline_name(),
            window,
            cx,
        );
        crate::views::forms::sync(
            &self.fields.server_name,
            self.servers.add_name(),
            window,
            cx,
        );
        crate::views::forms::sync(
            &self.fields.server_address,
            self.servers.add_address(),
            window,
            cx,
        );
        crate::views::forms::sync(
            &self.fields.direct_address,
            self.servers.direct_address(),
            window,
            cx,
        );
        crate::views::forms::sync(&self.fields.config_name, &self.config_name, window, cx);
    }

    /// The token box wrote a character: mirror it into the accounts state, which is
    /// what `submit_paste` reads.
    pub fn set_token_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.accounts.set_paste_text(text);
        cx.notify();
    }

    /// The offline username box.
    pub fn set_offline_name_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.accounts.set_offline_name(text);
        cx.notify();
    }

    /// The add form's name box.
    pub fn set_server_name_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.servers.set_add_name(text);
        cx.notify();
    }

    /// The add form's address box.
    pub fn set_server_address_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.servers.set_add_address(text);
        cx.notify();
    }

    /// The direct-join box.
    pub fn set_direct_address_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.servers.set_direct_address(text);
        cx.notify();
    }

    /// The new-config name box.
    pub fn set_config_name_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.config_name = text;
        cx.notify();
    }

    /// The new-config name, for tests and for the Create handler.
    #[must_use]
    pub fn config_name(&self) -> &str {
        &self.config_name
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

    /// The saved virtual key for one module, or `0` when the profile has none.
    ///
    /// The render path asks for this rather than for the formatted label: the label is a
    /// pure function of the key, so [`crate::reuse::by_number`] can memoise it.
    fn module_key(&self, module: &'static str) -> i32 {
        self.profile.module_keys.get(module).copied().unwrap_or(0)
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
    ///
    /// Section 6's "`cx.notify()` scoped to entities whose displayed fields changed":
    /// a slider drag coalesces to 25 ms but still delivers repeats of the same value at
    /// the ends of its travel, and each one used to repaint the whole shell. The paint
    /// is now conditional on the displayed number actually moving.
    pub fn set_value(&mut self, bind: &'static str, value: f64, cx: &mut Context<Self>) {
        let changed = self.draft.get(bind).copied() != Some(value);
        self.draft.insert(bind, value);
        self.apply_control(bind, crate::views::bindings::ControlValue::Number(value));
        if changed {
            cx.notify();
        }
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
    ///
    /// Like [`Shell::set_value`], the paint is conditional: a press that sets the value
    /// the checkbox already shows changes nothing visible.
    pub fn set_checked(&mut self, bind: &'static str, on: bool, cx: &mut Context<Self>) {
        let stored = if on { 1.0 } else { 0.0 };
        let changed = self.draft.get(bind).copied() != Some(stored);
        self.draft.insert(bind, stored);
        self.apply_control(bind, crate::views::bindings::ControlValue::Toggle(on));
        if changed {
            cx.notify();
        }
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
            return;
        }

        let before = self.value(bind, f64::NAN).to_bits();
        if crate::views::bindings::write(
            &mut self.profile,
            bind,
            crate::views::bindings::ControlValue::Number(index as f64),
        ) {
            self.values = crate::views::bindings::snapshot(&self.profile);
        } else {
            self.draft.insert(bind, index as f64);
        }
        if self.value(bind, f64::NAN).to_bits() != before {
            cx.notify();
        }
    }

    /// Records a combo selection by option text, which is how a mode field stores it.
    ///
    /// A press that lands on the option already shown does not repaint the card.
    pub fn set_selected_text(&mut self, bind: &'static str, text: &str, cx: &mut Context<Self>) {
        let unchanged = matches!(
            self.values.get(bind),
            Some(crate::views::bindings::ControlValue::Text(current)) if current == text
        ) && !self.draft.contains_key(bind);
        self.apply_control(
            bind,
            crate::views::bindings::ControlValue::Text(text.to_owned()),
        );
        if !unchanged {
            cx.notify();
        }
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

    /// The command-search state.
    ///
    /// **Not mounted in Phase 2** (section 12 decision 7): nothing calls
    /// [`Self::open_search`] and no key listener exists, so there is no overlay on
    /// screen. This accessor, the state machine and the view are built and tested,
    /// and the wiring is owed.
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

    // === Settings: configs ===

    /// Moves the config selection by one, which is what pressing the trigger does
    /// until a dropdown list exists.
    ///
    /// With fewer than two configs there is nowhere to move, so the press is a no-op
    /// that no longer repaints the settings card.
    pub fn select_next_config(&mut self, cx: &mut Context<Self>) {
        let before = self.profiles.selection();
        self.profiles.select_next();
        if self.profiles.selection() != before {
            cx.notify();
        }
    }

    /// `LoadConfigButton_Click` (`MainWindow.xaml.cs:813-831`).
    pub fn configs_load(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.profiles.selected().map(str::to_owned) else {
            self.profiles.set_status("Select a config to load.");
            cx.notify();
            return;
        };

        let _ = self.load_config(&name);
        cx.notify();
    }

    /// `SaveConfigButton_Click` (`MainWindow.xaml.cs:847-862`).
    pub fn configs_save(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.profiles.selected().map(str::to_owned) else {
            self.profiles
                .set_status("Select a config to overwrite, or create a new one.");
            cx.notify();
            return;
        };

        let _ = self.save_config(&name);
        cx.notify();
    }

    /// `CreateConfigButton_Click` (`MainWindow.xaml.cs:894-933`). The name comes from
    /// the card's field and the profile is the same live snapshot Save writes.
    pub fn configs_create(&mut self, cx: &mut Context<Self>) {
        let raw = self.config_name.clone();
        let mut profile = self.profile.clone();
        let outcome = self.profiles.create(&raw, &mut profile, false);
        self.profile = profile;
        if matches!(outcome, CreateOutcome::Created(_)) {
            self.config_name.clear();
        }
        cx.notify();
    }

    /// The overwrite prompt's confirm: the managed `MessageBox` answer, in place.
    pub fn configs_overwrite_confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.profiles.create_pending_name().map(str::to_owned) else {
            return;
        };
        let mut profile = self.profile.clone();
        let _ = self.profiles.create(&name, &mut profile, true);
        self.profile = profile;
        self.config_name.clear();
        cx.notify();
    }

    /// The overwrite prompt's Cancel.
    pub fn configs_overwrite_cancelled(&mut self, cx: &mut Context<Self>) {
        self.profiles.cancel_create();
        cx.notify();
    }

    /// "Delete": arms the in-card confirmation the managed app asked for in a
    /// `MessageBox` (`DeleteConfigButton_Click`, `MainWindow.xaml.cs:864-893`).
    pub fn configs_request_delete(&mut self, cx: &mut Context<Self>) {
        match self.profiles.selected().map(str::to_owned) {
            Some(name) => self.profiles.request_delete(&name),
            None => self.profiles.set_status("Select a config to delete."),
        }
        cx.notify();
    }

    /// The confirmation's own Delete.
    pub fn configs_confirm_delete(&mut self, cx: &mut Context<Self>) {
        let _ = self.profiles.confirm_delete();
        cx.notify();
    }

    /// The confirmation's Cancel: the file stays.
    pub fn configs_cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.profiles.cancel_delete();
        cx.notify();
    }

    /// `OpenConfigsFolderButton_Click` (`MainWindow.xaml.cs:969-985`), keyed on
    /// `CapabilityReport::open_path` instead of assuming a launcher exists.
    pub fn configs_open_folder(&mut self, cx: &mut Context<Self>) {
        if !self.open_configs_folder {
            self.profiles.set_status(
                "Could not open configs folder: no file manager launcher in this session.",
            );
            cx.notify();
            return;
        }

        match self.profiles.directory() {
            Some(dir) => {
                if let Err(error) = aoko_platform::open_path(std::path::Path::new(&dir)) {
                    self.profiles.set_status(format!(
                        "Could not open configs folder: {}",
                        error.user_facing()
                    ));
                }
            }
            None => self
                .profiles
                .set_status("Could not open configs folder: no profile store is open."),
        }
        cx.notify();
    }

    /// Whether animated sites run, from decision 3's client setting.
    #[must_use]
    pub fn animations(&self) -> bool {
        self.animations
    }

    /// Why a client preference could not be read or written, empty when nothing
    /// failed.
    #[must_use]
    pub fn prefs_status(&self) -> &str {
        &self.prefs_status
    }

    /// The Settings tab's Animations row: flips the gate and remembers the choice.
    pub fn toggle_animations(&mut self, cx: &mut Context<Self>) {
        self.set_animations(!self.animations, cx);
    }

    /// Applies an animation setting to `theme::motion` and persists it.
    ///
    /// The gate is process-wide, so this is the one write the shipped binary makes
    /// to it; a failure to save is reported in the card rather than dropped.
    pub fn set_animations(&mut self, enabled: bool, cx: &mut Context<Self>) {
        // Both the row's boolean and its status line are what a repaint would show, so
        // the paint waits for one of them to move. The gate is applied and the file
        // written either way: `theme::motion` is process-wide state a test can reset
        // independently of this field.
        let changed = self.animations != enabled;
        let status_before = self.prefs_status.clone();
        self.animations = enabled;
        motion::set_animations_enabled(enabled);

        if let Some(prefs) = self.prefs.as_ref() {
            match prefs.save(ClientPrefs {
                animations: enabled,
            }) {
                Ok(()) => self.prefs_status.clear(),
                Err(error) => {
                    self.prefs_status = format!("Could not save client preferences: {error}");
                }
            }
        }

        if changed || self.prefs_status != status_before {
            cx.notify();
        }
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

    /// Opens the search state, optionally seeded with the typed character.
    ///
    /// **Nothing calls this in Phase 2** (decision 7): the shell installs no key
    /// listener, so Ctrl+K and type-to-open do not reach it. When the wiring lands,
    /// both gestures land here and differ only in the seed.
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

    /// `armed · 7 on`, rebuilt only when a toggle changes.
    ///
    /// `Render::render` wants it twice a frame — the title bar and the rail footer — and
    /// the text is a pure function of (armed, enabled count), so it is cached against
    /// those two values rather than formatted per call. `perf::tests` counts the
    /// difference.
    fn chip_text(&mut self) -> SharedString {
        let enabled = self.toggles.enabled_count();
        let key = (self.toggles.armed(), enabled);

        if let Some((cached, text)) = &self.chrome.chip {
            if *cached == key {
                return SharedString::clone(text);
            }
        }

        let armed = if key.0 { "armed" } else { "disarmed" };
        let text = SharedString::from(format!("{armed} \u{00b7} {enabled} on"));
        self.chrome.chip = Some((key, text.clone()));
        text
    }

    /// `Combat · armed · 7 on`, rebuilt only when the tab or the chip changes.
    fn title_text(&mut self, chip: &SharedString) -> SharedString {
        let key = (self.tab, SharedString::clone(chip));

        if let Some((cached, text)) = &self.chrome.title {
            if *cached == key {
                return SharedString::clone(text);
            }
        }

        let text = SharedString::from(format!("{} \u{00b7} {}", self.selected_tab_name(), chip));
        self.chrome.title = Some((key, text.clone()));
        text
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The managed app saves on exit (`MainWindow.OnClosed`, `MainWindow.xaml.cs:1861`).
        // The hook covers a platform close; the title bar's own close button calls
        // [`Shell::close_window`], which saves before quitting.
        self.ensure_close_hook(window, cx);

        // The text fields are entities, so they exist only from the first render on;
        // `Shell::new` has no context to create them with. Syncing here is what makes
        // a state change (an add form prefill, a cleared form) reach the widget
        // without every mutator remembering to push it.
        self.ensure_form_fields(cx);
        self.sync_form_fields(window, cx);

        let motion = motion::motion();
        // The two chrome strings are built before the theme is borrowed: each needs
        // `&mut self` to consult its cache, and both are read only once the tree is
        // being assembled. `&self.theme` rather than a clone — a `Theme` carries a
        // `String` name, so cloning it was one heap allocation per frame.
        let chip = self.chip_text();
        let title = self.title_text(&chip);
        let theme = &self.theme;
        let entity = cx.entity();

        let body = match copy::TABS.get(self.tab).map(|tab| tab.body) {
            Some(TabBody::Cards(cards)) => self.render_cards(cards, theme, motion, &entity),
            Some(TabBody::Accounts) => crate::views::accounts_view::render(
                &self.accounts,
                &self.fields,
                theme,
                &entity,
                self.tab,
            ),
            Some(TabBody::Servers) => crate::views::servers_view::render(
                &self.servers,
                &self.fields,
                theme,
                &entity,
                self.tab,
            ),
            None => div().flex_1().into_any_element(),
        };

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            // Decision 6: one family for the whole tree, set once at the root so it
            // inherits. Installing the fonts without asking for the family would leave
            // every measurement to whatever fontconfig happens to resolve.
            .font_family(crate::assets::UI_FAMILY)
            .child(self.render_title_bar(theme, &entity, &title))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .gap(px(metrics::rail::GAP))
                    .child(self.render_rail(theme, &entity, &chip))
                    .child(div().flex().flex_1().min_w(px(0.0)).child(body)),
            );

        // Decision 5's resize edges. They are last so they paint above the title bar
        // and the tab body, and each consumes its own mouse-down before anything
        // else can see it.
        for handle in resize_strips() {
            root = root.child(self.render_resize_handle(&handle, &entity));
        }

        root.into_any_element()
    }
}

impl Shell {
    /// Decision 5: the themed, draggable, hit-tested title bar.
    fn render_title_bar(
        &self,
        theme: &Theme,
        self_handle: &Entity<Self>,
        title: &SharedString,
    ) -> AnyElement {
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
                    .child(SharedString::clone(title)),
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
                    .on_click({
                        let handle = self_handle.clone();
                        move |_, _, cx| {
                            // Our own close is `CloseButton_Click`
                            // (`MainWindow.xaml.cs:1384-1392`): save, then quit.
                            handle.update(cx, |shell, _cx| {
                                let _ = shell.save_profile();
                            });
                            cx.quit();
                        }
                    })
                    .child("\u{00d7}"),
            )
            .into_any_element()
    }

    /// The 176 px rail: search button, the nine tabs, the update row.
    fn render_rail(&self, theme: &Theme, entity: &Entity<Self>, chip: &SharedString) -> AnyElement {
        let mut rail = div()
            .flex()
            .flex_col()
            .w(px(metrics::rail::WIDTH))
            .h_full()
            .py(px(10.0))
            .px(px(10.0))
            .gap(px(2.0))
            .bg(theme.background);

        // **Inert in Phase 2** (section 12 decision 7 debt): the managed button opens the
        // command-search overlay (`MainWindow.xaml.cs:1623-1824`). The overlay view and
        // its state exist, but nothing mounts the view and the shell installs no key
        // handling, so this is a click handler that does nothing. It keeps the managed
        // label rather than switching to a disabled style the C# does not have.
        rail = rail.child(
            components::button(
                "rail-search",
                theme,
                ButtonVariant::Secondary,
                // Fixed copy: the label is the tooltip without its chord, then the
                // chord. `reuse::stitched` builds it on the first frame.
                crate::reuse::stitched(
                    copy::SEARCH_BUTTON_TOOLTIP
                        .split(" (")
                        .next()
                        .unwrap_or(copy::SEARCH_BUTTON_TOOLTIP),
                    "   ",
                    copy::SEARCH_BUTTON_CHORD,
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
                .child(SharedString::clone(chip)),
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
            .id(crate::reuse::indexed("rail-tab", index))
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
        let mut body = components::scroll(crate::reuse::indexed("tab-scroll", self.tab))
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
        let header = copy::card_header(card);
        let mut body = components::card(theme);

        if let Some(header) = header {
            body = body.child(self.render_card_header(card, header, theme, motion, entity));
        }

        for control in card.controls {
            if control_is_header(control, header) {
                continue;
            }
            body = body.child(self.render_control(card, control, theme, motion, entity));
        }

        // `ConfigStatusText` (`MainWindow.xaml:1536`): the configs card owns one status
        // line, set by `SetConfigStatus` in every config flow.
        if card.id == "ConfigsCard" && self.profiles.status().is_visible() {
            body = body.child(
                div()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(crate::reuse::text(
                        "configs-status",
                        &self.profiles.status().message,
                    )),
            );
        }

        body.into_any_element()
    }

    /// One resize strip: absolutely placed, and the only element that asks the
    /// platform to start a resize for its edge.
    fn render_resize_handle(&self, strip: &ResizeStrip, entity: &Entity<Self>) -> AnyElement {
        let mut element = div().id(strip.id).absolute().cursor(strip.cursor);
        if let Some(left) = strip.left {
            element = element.left(px(left));
        }
        if let Some(right) = strip.right {
            element = element.right(px(right));
        }
        if let Some(top) = strip.top {
            element = element.top(px(top));
        }
        if let Some(bottom) = strip.bottom {
            element = element.bottom(px(bottom));
        }
        if let Some(width) = strip.width {
            element = element.w(px(width));
        }
        if let Some(height) = strip.height {
            element = element.h(px(height));
        }

        let edge = strip.edge;
        let handle = entity.clone();
        element
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                // Recorded first: the test platform's `start_window_resize` is a
                // no-op, so this field is what a test can see, and on a real
                // backend the call below is the actual resize.
                handle.update(cx, |shell, _cx| shell.resize_requested = Some(edge));
                window.start_window_resize(edge);
                cx.stop_propagation();
            })
            .into_any_element()
    }

    /// The managed header row: the switch or the bold label on the left, the
    /// 140-wide keybind button docked right (`MainWindow.xaml:359-379`).
    fn render_card_header(
        &self,
        card: &Card,
        header: copy::CardHeader,
        theme: &Theme,
        motion: Motion,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let mut row = div().flex().flex_row().items_center().w_full().gap(px(8.0));

        match header {
            copy::CardHeader::Text(text) => {
                row = row.child(div().text_size(px(16.0)).text_color(theme.text).child(text));
            }
            copy::CardHeader::Switch { label, bind } => {
                row = row.child(self.render_switch(card, label, bind, theme, motion, entity));
            }
        }

        if let Some(module) = card_module(card) {
            // `DockPanel` docks the button right and gives it `Width="140"`
            // (`MainWindow.xaml:359`); the row's 8 px gap is that button's
            // `Margin="8,0,0,0"`. `button_sized` sets the width on the button itself, as
            // WPF's `Width` does, so the fill is 140 px rather than the label's width.
            let mut slot = div().ml_auto();
            if let Some(selector) = keybind_debug_selector(card.id) {
                slot = slot.debug_selector(move || selector.to_owned());
            }
            row = row.child(slot.child(keybind_button(
                card.id,
                self.module_key(module),
                theme,
                entity,
            )));
        }

        row.into_any_element()
    }

    /// The switch row's widget, used both in a header and in a body row.
    fn render_switch(
        &self,
        card: &Card,
        label: &'static str,
        bind: &'static str,
        theme: &Theme,
        motion: Motion,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let target = switch_target(bind);
        let is_on = match target {
            SwitchTarget::Module(module) => self.is_module_on(module),
            // The profile snapshot, so a card paints the saved boolean rather than a
            // hardcoded `false`. The five non-module switches are `ShowLogo`,
            // `DevMode`, `ShowModuleList`, `DiscordRpcEnabled` and `AutoTotemElytra`.
            SwitchTarget::Profile(bind) => self.value(bind, 0.0) > 0.5,
        };
        // `switch` is an interactive element and attaches `.id()`/`.on_click()`
        // internally, so this is where the click is routed: a module flag flips
        // `ModuleToggles`, a profile boolean writes its own bind through
        // `bindings`. Every switch has one of the two targets — the old fallback to
        // the card's module silently misrouted five of them.
        let handle = entity.clone();
        let switch = components::switch(
            crate::reuse::stitched(card.id, "-", bind),
            theme,
            is_on,
            motion,
            move |_, _, cx| {
                handle.update(cx, |shell, cx| match target {
                    SwitchTarget::Module(module) => shell.toggle_module(module, cx),
                    SwitchTarget::Profile(bind) => shell.set_checked(bind, !is_on, cx),
                });
            },
        );

        let mut switch = switch;
        if let Some(selector) = switch_debug_selector(bind) {
            // A test needs to know where the switch is to click it, and
            // `debug_selector` takes a `&'static str`; only the switches the click
            // tests target carry one.
            switch = div()
                .debug_selector(move || selector.to_owned())
                .child(switch)
                .into_any_element();
        }

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .child(
                div()
                    .text_size(px(metrics::text::BUTTON))
                    .text_color(theme.text)
                    .child(label),
            )
            .child(switch)
            .into_any_element()
    }

    /// The Delete button plus, when armed, the in-card confirmation.
    ///
    /// The managed surface was a `YesNo` `MessageBox` (`DeleteConfigButton_Click`,
    /// `MainWindow.xaml.cs:864-893`). A card cannot host a modal, so the message is
    /// the managed string verbatim and the buttons name the action instead of saying
    /// Yes/No, which only makes sense next to a dialog's title.
    fn configs_delete_row(
        &self,
        button: AnyElement,
        theme: &Theme,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let Some(name) = self.profiles.delete_armed() else {
            return button;
        };

        let handle = entity.clone();
        let confirm = components::button(
            "configs-delete-confirm",
            theme,
            ButtonVariant::Danger,
            "Delete",
            move |_, _, cx| handle.update(cx, |shell, cx| shell.configs_confirm_delete(cx)),
        );
        let handle = entity.clone();
        let cancel = components::button(
            "configs-delete-cancel",
            theme,
            ButtonVariant::Secondary,
            "Cancel",
            move |_, _, cx| handle.update(cx, |shell, cx| shell.configs_cancel_delete(cx)),
        );

        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .w_full()
            .child(button)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(theme.text)
                            .child(SharedString::from(format!(
                                "Delete config '{name}'? This cannot be undone."
                            ))),
                    )
                    .child(confirm)
                    .child(cancel),
            )
            .into_any_element()
    }

    /// The Create row: the name field and the button, plus the overwrite
    /// confirmation when the name already exists (`CreateConfigButton_Click`,
    /// `MainWindow.xaml.cs:894-933`).
    fn configs_create_row(
        &self,
        button: AnyElement,
        theme: &Theme,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let mut body = div().flex().flex_col().gap(px(4.0)).w_full().child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .w_full()
                .child(
                    div()
                        .flex_1()
                        .child(FormFields::element(&self.fields.config_name)),
                )
                .child(button),
        );

        if let Some(name) = self.profiles.create_pending_name() {
            let handle = entity.clone();
            let overwrite = components::button(
                "configs-overwrite-confirm",
                theme,
                ButtonVariant::Danger,
                "Overwrite",
                move |_, _, cx| {
                    handle.update(cx, |shell, cx| shell.configs_overwrite_confirmed(cx))
                },
            );
            let handle = entity.clone();
            let cancel = components::button(
                "configs-overwrite-cancel",
                theme,
                ButtonVariant::Secondary,
                "Cancel",
                move |_, _, cx| {
                    handle.update(cx, |shell, cx| shell.configs_overwrite_cancelled(cx))
                },
            );

            body = body.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(theme.text)
                            .child(SharedString::from(format!(
                                "Config '{name}' already exists. Overwrite it?"
                            ))),
                    )
                    .child(overwrite)
                    .child(cancel),
            );
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
                self.render_switch(card, label, bind, theme, motion, entity)
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
                let id = crate::reuse::stitched(card.id, "-", bind);
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
                let name = crate::reuse::stitched(card.id, "-", bind);

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
                                    .child(crate::reuse::by_number(
                                        "value",
                                        value.to_bits(),
                                        || format_value(value),
                                    )),
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

                // The Settings config combo is the one combo whose list is not in the
                // copy table: `RefreshConfigList` (`MainWindow.xaml.cs:781-805`) filled
                // it from the profile directory, so it reads the store's list here.
                if bind == "ConfigSelection" {
                    // Borrowed, not cloned: the store's list is rebuilt only when a
                    // config is created or deleted, so a per-frame `to_vec` was a `Vec`
                    // and a `String` per name for nothing.
                    let names = self.profiles.configs();
                    let current = self.profiles.selected().unwrap_or("");
                    let handle = entity.clone();
                    let has_names = !names.is_empty();
                    let mut row = div().flex().flex_col().gap(px(2.0)).w_full().child(
                        div()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(theme.text)
                            .child(*label),
                    );

                    if has_names {
                        row = row.child(components::combo(
                            crate::reuse::stitched(card.id, "-combo-", bind),
                            theme,
                            // `selected` already names a config; the label is a view of
                            // it, so it is memoised rather than copied twice a frame.
                            crate::reuse::text("config-selection", current),
                            move |_, _, cx| {
                                handle.update(cx, |shell, cx| shell.select_next_config(cx));
                            },
                        ));
                    } else {
                        // `ConfigComboBox` with no items: an empty trigger, exactly what a
                        // WPF `ComboBox` with an empty `ItemsSource` paints.
                        row = row.child(components::combo(
                            crate::reuse::stitched(card.id, "-combo-", bind),
                            theme,
                            SharedString::from(""),
                            |_, _, _| {},
                        ));
                    }

                    return row.into_any_element();
                }

                // The setting is either an index or the option text; the profile decides
                // which, so the press converts back through the same table. Borrowed, not
                // cloned: a `Text` variant would copy its `String` every frame.
                let stored = self.values.get(bind);
                let selected = match stored {
                    Some(crate::views::bindings::ControlValue::Text(text)) => options
                        .iter()
                        .position(|option| option.eq_ignore_ascii_case(text))
                        .unwrap_or(0),
                    Some(other) => other.as_f64() as usize,
                    None => 0,
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
                    crate::reuse::stitched(card.id, "-combo-", bind),
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

            Control::Preference { label, hint } => {
                let on = self.animations;
                let handle = entity.clone();
                let boxed = components::checkbox(
                    crate::reuse::stitched(card.id, "-", label),
                    theme,
                    on,
                    *label,
                    move |_, _, cx| handle.update(cx, |shell, cx| shell.toggle_animations(cx)),
                );

                let mut column = div().flex().flex_col().gap(px(2.0)).child(boxed).child(
                    div()
                        .text_size(px(metrics::text::CAPTION))
                        .text_color(theme.dim_text)
                        .child(*hint),
                );

                if !self.prefs_status.is_empty() {
                    column = column.child(
                        div()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(crate::theme::fixed::CRITICAL)
                            .child(crate::reuse::text("prefs-status", &self.prefs_status)),
                    );
                }

                column.into_any_element()
            }

            Control::Keybind { module } => {
                keybind_button(card.id, self.module_key(module), theme, entity)
            }

            Control::Button { label, tooltip } => {
                // The configs card's buttons are real actions: Load and Save
                // (`LoadConfigButton_Click` `:813-831`, `SaveConfigButton_Click`
                // `:847-862`), Delete and Create with their confirmations, and Open
                // Configs Folder (`:969-985`). Every other card button still needs the
                // command surface it would call, so it stays inert rather than
                // pretending.
                let action: fn(&mut Shell, &mut Context<Shell>) = match (card.id, *label) {
                    ("ConfigsCard", "Load") => Shell::configs_load,
                    ("ConfigsCard", "Save") => Shell::configs_save,
                    ("ConfigsCard", "Delete") => Shell::configs_request_delete,
                    ("ConfigsCard", "Create") => Shell::configs_create,
                    ("ConfigsCard", "Open Configs Folder") => Shell::configs_open_folder,
                    _ => |_, _| {},
                };
                let handle = entity.clone();

                let button = components::button(
                    crate::reuse::stitched(card.id, "-", label),
                    theme,
                    if tooltip.is_empty() {
                        ButtonVariant::Dark
                    } else {
                        ButtonVariant::Danger
                    },
                    SharedString::from(*label),
                    move |_, _, cx| handle.update(cx, action),
                )
                .into_any_element();

                if card.id != "ConfigsCard" {
                    return button;
                }
                match *label {
                    "Delete" => self.configs_delete_row(button, theme, entity),
                    "Create" => self.configs_create_row(button, theme, entity),
                    _ => button,
                }
            }

            // **Inert in Phase 2** (decision 7 debt), same reason as the keybind buttons:
            // `AutoRodActionBindButton_Click` (`MainWindow.xaml.cs:1954-1963`) starts the
            // key capture, and there is no capture surface to start.
            //
            // Both buttons are `Style="{StaticResource DarkButton}"
            // HorizontalAlignment="Left" Width="210" Margin="0,10,0,0"`
            // (`MainWindow.xaml:525`, `:542`), so neither the keybind button's width (140)
            // nor a content-sized box is right. These two rows are `Control::ActionBind`
            // rather than `Control::Button` precisely so the variant, width and label can
            // differ; before that they were generic rows and this arm had no callers.
            Control::ActionBind { which } => {
                // The card stacks its children with a 6 px gap, so 4 more is the managed
                // 10 px top margin.
                // `HorizontalAlignment="Left"`: shrink to the button and sit at the card's
                // left edge instead of stretching across the column.
                let mut slot = div().mt(px(4.0)).self_start();
                if let Some(selector) = action_bind_debug_selector(card.id, *which) {
                    slot = slot.debug_selector(move || selector.to_owned());
                }
                slot.child(components::button_sized(
                    crate::reuse::stitched(card.id, "-", which.id()),
                    theme,
                    ButtonVariant::Dark,
                    // The label is a function of the saved key, nothing else, so it is
                    // memoised by that key instead of formatted per frame.
                    crate::reuse::by_number(
                        "action-label",
                        action_vk(&self.profile, *which),
                        || action_bind_label(&self.profile, *which),
                    ),
                    210.0,
                    |_, _, _| {},
                ))
                .into_any_element()
            }

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
                .child(crate::reuse::joined("\u{25b8} ", header))
                .into_any_element(),

            Control::BlockEspRows => div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child("Tracked blocks: add a block to start.")
                .into_any_element(),

            Control::PalettePreview { name } => {
                let preview_accent = Theme::accent_by_name(name);
                div()
                    .id(crate::reuse::stitched("palette-", "", name))
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
                            .bg(preview_accent),
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

/// One client-drawn resize strip: an edge or a corner, placed absolutely.
struct ResizeStrip {
    id: &'static str,
    edge: gpui::ResizeEdge,
    cursor: gpui::CursorStyle,
    left: Option<f32>,
    right: Option<f32>,
    top: Option<f32>,
    bottom: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
}

/// The eight strips decision 5 asks for: four edges and four corners.
///
/// `Window::start_window_resize(edge)` is public at the pin
/// (`crates/gpui/src/window.rs:2873`), `ResizeEdge` has all eight variants
/// (`platform.rs:850`), and both Linux backends implement it, but GPUI does no edge
/// hit-testing for us: the pointer has to land on an element that asks. These are
/// that element, drawn in client area like the title bar.
#[must_use]
fn resize_strips() -> [ResizeStrip; 8] {
    use gpui::{CursorStyle, ResizeEdge};

    /// How thick an edge strip is, in px.
    const EDGE: f32 = 4.0;
    /// How thick a corner strip is, in px.
    const CORNER: f32 = 10.0;

    [
        ResizeStrip {
            id: "resize-left",
            edge: ResizeEdge::Left,
            cursor: CursorStyle::ResizeLeftRight,
            left: Some(0.0),
            right: None,
            top: Some(0.0),
            bottom: Some(0.0),
            width: Some(EDGE),
            height: None,
        },
        ResizeStrip {
            id: "resize-right",
            edge: ResizeEdge::Right,
            cursor: CursorStyle::ResizeLeftRight,
            left: None,
            right: Some(0.0),
            top: Some(0.0),
            bottom: Some(0.0),
            width: Some(EDGE),
            height: None,
        },
        ResizeStrip {
            id: "resize-top",
            edge: ResizeEdge::Top,
            cursor: CursorStyle::ResizeUpDown,
            left: Some(0.0),
            right: Some(0.0),
            top: Some(0.0),
            bottom: None,
            width: None,
            height: Some(EDGE),
        },
        ResizeStrip {
            id: "resize-bottom",
            edge: ResizeEdge::Bottom,
            cursor: CursorStyle::ResizeUpDown,
            left: Some(0.0),
            right: Some(0.0),
            top: None,
            bottom: Some(0.0),
            width: None,
            height: Some(EDGE),
        },
        ResizeStrip {
            id: "resize-top-left",
            edge: ResizeEdge::TopLeft,
            cursor: CursorStyle::ResizeUpRightDownLeft,
            left: Some(0.0),
            right: None,
            top: Some(0.0),
            bottom: None,
            width: Some(CORNER),
            height: Some(CORNER),
        },
        ResizeStrip {
            id: "resize-top-right",
            edge: ResizeEdge::TopRight,
            cursor: CursorStyle::ResizeUpLeftDownRight,
            left: None,
            right: Some(0.0),
            top: Some(0.0),
            bottom: None,
            width: Some(CORNER),
            height: Some(CORNER),
        },
        ResizeStrip {
            id: "resize-bottom-left",
            edge: ResizeEdge::BottomLeft,
            cursor: CursorStyle::ResizeUpLeftDownRight,
            left: Some(0.0),
            right: None,
            top: None,
            bottom: Some(0.0),
            width: Some(CORNER),
            height: Some(CORNER),
        },
        ResizeStrip {
            id: "resize-bottom-right",
            edge: ResizeEdge::BottomRight,
            cursor: CursorStyle::ResizeUpRightDownLeft,
            left: None,
            right: Some(0.0),
            top: None,
            bottom: Some(0.0),
            width: Some(CORNER),
            height: Some(CORNER),
        },
    ]
}

/// What a `Control::Switch` row flips.
///
/// The managed markup styles two different things as `ModuleSwitch`: a module's own
/// flag, whose bind resolves through the catalogue's `toggle_property`, and a
/// profile boolean that belongs to no module (`ShowLogo`, `DevMode`,
/// `ShowModuleList`, `DiscordRpcEnabled`, `AutoTotemElytra`). The old code fell back
/// to the card's module when the bind resolved to nothing, which flipped the whole
/// AutoTotem module from the Elytra row and left the other four dead — and three of
/// those painted OFF over a default profile that says ON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwitchTarget {
    /// A catalogue module flag: the switch flips [`ModuleToggles`].
    Module(&'static str),
    /// A profile boolean with no module: the switch writes this bind through
    /// [`crate::views::bindings`].
    Profile(&'static str),
}

/// Which of the two a switch's bind names. Never falls back to the card.
fn switch_target(bind: &'static str) -> SwitchTarget {
    match module_for_bind(bind) {
        Some(module) => SwitchTarget::Module(module),
        None => SwitchTarget::Profile(bind),
    }
}

/// The debug selector for the action-bind button the geometry test measures.
///
/// Same rule as [`switch_debug_selector`]: name only what a test targets. Both buttons
/// are 210 wide in the markup, and this measures one of them.
fn action_bind_debug_selector(
    card_id: &'static str,
    which: crate::views::copy::ActionBind,
) -> Option<&'static str> {
    match (card_id, which) {
        ("AutoRodCard", crate::views::copy::ActionBind::AutoRod) => Some("action-bind-AutoRod"),
        _ => None,
    }
}

/// The debug selector for the keybind button the geometry test measures.
///
/// Same rule as [`switch_debug_selector`]: name only what a test targets, because the
/// selector is a `String` and every card would otherwise record one. A no-op outside a
/// debug build with `test-support`.
fn keybind_debug_selector(card_id: &'static str) -> Option<&'static str> {
    match card_id {
        "AutoClickerCard" => Some("keybind-AutoClickerCard"),
        _ => None,
    }
}

/// The debug selector for the switches the click tests target.
///
/// `debug_selector` takes a `&'static str`, and an id built from a card and a bind
/// is a `SharedString`, so this names the six switches a test needs rather than all
/// 43. It records nothing in a release build (`debug_assertions` off).
fn switch_debug_selector(bind: &'static str) -> Option<&'static str> {
    match bind {
        "IsArmed" => Some("switch-IsArmed"),
        "ShowModuleList" => Some("switch-ShowModuleList"),
        "DiscordRpcEnabled" => Some("switch-DiscordRpcEnabled"),
        "AutoTotemElytra" => Some("switch-AutoTotemElytra"),
        "ShowLogo" => Some("switch-ShowLogo"),
        "DevMode" => Some("switch-DevMode"),
        _ => None,
    }
}

/// Whether a control was already painted as part of the card's header.
///
/// The managed header row *is* the switch on a module card and the first `Note` on
/// a text-headed one (`MainWindow.xaml:359-379`), so painting them again in the body
/// would duplicate them — which is what the old title did on all 38 module cards.
fn control_is_header(control: &Control, header: Option<copy::CardHeader>) -> bool {
    match (control, header) {
        (Control::Keybind { .. }, Some(_)) => true,
        (
            Control::Switch { bind, .. },
            Some(copy::CardHeader::Switch {
                bind: header_bind, ..
            }),
        ) => *bind == header_bind,
        (Control::Note { text }, Some(copy::CardHeader::Text(heading_text))) => {
            *text == heading_text
        }
        _ => false,
    }
}

/// The 140-wide keybind button the managed header docks right.
///
/// **Inert in Phase 2** (section 12 decision 7 debt). `KeybindButton_Click`
/// (`MainWindow.xaml.cs:1943-1952`) starts the global key capture; this crate has no
/// capture surface yet, so the handler is a documented no-op rather than a second capture
/// path, and the button keeps the managed copy so a reader can see it is the same control.
fn keybind_button(
    card_id: &'static str,
    vk: i32,
    theme: &Theme,
    _entity: &Entity<Shell>,
) -> AnyElement {
    components::button::button_sized(
        crate::reuse::stitched(card_id, "-keybind", ""),
        theme,
        ButtonVariant::Secondary,
        // `Bind: {key}` depends on the saved key and nothing else, so it is memoised by
        // the key. Formatting it here allocated a `String` per keybind button per frame.
        crate::reuse::by_number("keybind-label", action_vk_of(vk), || {
            crate::views::keybind_text::button_label(vk)
        }),
        140.0,
        |_, _, _| {},
    )
    .into_any_element()
}

/// A virtual key as the unsigned key [`crate::reuse::by_number`] memoises on.
fn action_vk_of(vk: i32) -> u64 {
    u64::from(vk as u32)
}

/// The saved action key for one action-bind button.
fn action_vk(
    profile: &aoko_protocol::profile::Profile,
    which: crate::views::copy::ActionBind,
) -> u64 {
    let vk = match which {
        crate::views::copy::ActionBind::AutoRod => profile.auto_rod_action_key,
        crate::views::copy::ActionBind::Throwpot => profile.throwpot_action_key,
    };
    action_vk_of(vk)
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

/// An action-bind button's rest label.
///
/// `UpdateKeybindButtons` (`MainWindow.xaml.cs:2135-2140`) composes
/// ``Action: {FormatVirtualKey(InputHooks.AutoRodActionKey)}`` from
/// `Profile::auto_rod_action_key` / `Profile::throwpot_action_key`, so the button follows
/// the saved key the same way the keybind button follows `Profile::module_keys`. The
/// pending-capture branch (`Action: [Press key or mouse...]`) needs the capture surface
/// this phase does not have.
fn action_bind_label(
    profile: &aoko_protocol::profile::Profile,
    which: crate::views::copy::ActionBind,
) -> String {
    let vk = match which {
        crate::views::copy::ActionBind::AutoRod => profile.auto_rod_action_key,
        crate::views::copy::ActionBind::Throwpot => profile.throwpot_action_key,
    };
    crate::views::keybind_text::action_label(vk)
}

/// The keybind button's rest label for a whole profile.
///
/// The XAML button has no `Content` (`MainWindow.xaml:359`): every label comes from
/// `SetKeybindButtonContent` (`MainWindow.xaml.cs:2143-2156`), which composes
/// ``Bind: {FormatVirtualKey(InputHooks.GetModuleKey(module))}`` from the module's
/// virtual key, or `Unbound` when there is none — so an untouched profile reads
/// `Bind: Unbound`. See [`crate::views::keybind_text`] for the format and the one
/// branch (`Key` enum names) that is not ported.
///
/// Test-only: `render` goes through [`Shell::module_key`] and
/// [`crate::reuse::by_number`] so the label is built once per key rather than once per
/// frame, and this is the same composition written out for the tests that pin it.
#[cfg(test)]
fn keybind_label(profile: &aoko_protocol::profile::Profile, module: &'static str) -> String {
    let vk = profile.module_keys.get(module).copied().unwrap_or(0);
    crate::views::keybind_text::button_label(vk)
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
    fn card_headers_are_the_managed_shapes_and_never_an_identifier() {
        let cards = match copy::TABS[0].body {
            TabBody::Cards(cards) => cards,
            TabBody::Accounts | TabBody::Servers => &[],
        };

        // `MainWindow.xaml:359-362`: the header of a module card is its switch.
        assert_eq!(
            copy::card_header(&cards[0]),
            Some(copy::CardHeader::Switch {
                label: "AutoClicker",
                bind: "IsArmed"
            })
        );
        // `:376-379`: Right Click is a bold label, and its switch is a body row.
        assert_eq!(
            copy::card_header(&cards[1]),
            Some(copy::CardHeader::Text("Right Click"))
        );

        // No card may paint an identifier: every header is either its own switch's
        // label or a string the table already carries (a `Note`, or one of the four
        // managed headings).
        for tab in copy::TABS {
            let TabBody::Cards(cards) = tab.body else {
                continue;
            };
            for card in cards {
                match copy::card_header(card) {
                    Some(copy::CardHeader::Switch { label, bind }) => {
                        assert!(
                            card.controls.iter().any(|control| matches!(
                                control,
                                Control::Switch { label: own, bind: own_bind }
                                    if *own == label && *own_bind == bind
                            )),
                            "{} header switch is not the card's own",
                            card.id
                        );
                    }
                    Some(copy::CardHeader::Text(text)) => {
                        assert!(
                            copy::heading(card.id) == Some(text)
                                || card.controls.iter().any(|control| matches!(
                                    control,
                                    Control::Note { text: own } if *own == text
                                )),
                            "{} header {text} is not in the table",
                            card.id
                        );
                    }
                    None => {}
                }
            }
        }
    }

    #[test]
    fn the_four_managed_headings_are_verbatim() {
        assert_eq!(copy::heading("ConfigsCard"), Some("Configs"));
        assert_eq!(copy::heading("ReloadMappingsCard"), Some("Reload Mappings"));
        assert_eq!(
            copy::heading("GuiCustomizationCard"),
            Some("GUI Customization")
        );
        assert_eq!(copy::heading("CpsHistogramCard"), Some("CPS Distribution"));
        // Cards the C# gives no heading must not acquire one.
        assert_eq!(copy::heading("PlaytimeSummaryCard"), None);
        assert_eq!(copy::heading("DiscordRpcCard"), None);
        assert_eq!(copy::heading("BetaBannerCard"), None);
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

    fn scratch_root() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        let index = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!("aoko-shell-{}-{index}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        path
    }

    #[test]
    fn a_shell_over_a_store_starts_from_the_auto_save_slot() {
        let root = scratch_root();
        let store = aoko_platform::ProfileStore::new(&root);

        let mut profile = aoko_protocol::profile::Profile::default();
        profile.min_cps = 9.0;
        store
            .write_profile(
                crate::views::profiles::AUTO_SAVE_NAME,
                &aoko_protocol::profile::to_profile_json(&profile),
            )
            .expect("seed");

        let shell = Shell::with_store(Theme::slate(), store);
        assert_eq!(shell.value("MinCPS", 0.0), 9.0, "loaded at startup");
        assert!(shell.profiles().has_store());
        assert_eq!(shell.profiles().store_error(), None);
    }

    #[test]
    fn saving_on_exit_is_what_the_next_launch_reads() {
        let root = scratch_root();
        let mut shell = Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root));
        assert!(shell.apply_control("MinCPS", crate::views::bindings::ControlValue::Number(11.0)));

        shell.save_profile().expect("save on exit");

        // A second shell over the same directory is the next launch.
        let reloaded = Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root));
        assert_eq!(reloaded.value("MinCPS", 0.0), 11.0);
    }

    #[test]
    fn the_configs_save_and_load_round_trip_through_the_shell() {
        let root = scratch_root();
        let mut shell = Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root));

        assert!(shell.apply_control("MinCPS", crate::views::bindings::ControlValue::Number(12.0)));
        shell.save_config("PvP").expect("named save");
        assert_eq!(shell.profiles().configs(), ["PvP".to_owned()]);

        assert!(shell.apply_control("MinCPS", crate::views::bindings::ControlValue::Number(3.0)));
        shell.load_config("PvP").expect("named load");
        assert_eq!(shell.value("MinCPS", 0.0), 12.0, "the config wins");
    }

    #[test]
    fn loading_a_config_applies_its_palette() {
        let root = scratch_root();
        let mut shell = Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root));
        // A real palette name: an unknown one normalises to Slate (`Theme::by_name`).
        shell.profile.gui_theme = "Ink".to_owned();
        shell.save_config("Dark").expect("save");
        assert_eq!(shell.theme().name, "Slate", "saving does not repaint");

        shell.load_config("Dark").expect("load");
        assert_eq!(shell.theme().name, "Ink");
    }

    #[gpui::test]
    fn the_config_create_and_delete_actions_run_through_the_shell(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let handle = cx.update(|cx| {
            cx.new(|_| Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root)))
        });

        // Create: the name is sanitised, the config appears, the field clears.
        cx.update(|cx| {
            handle.update(cx, |shell, cx| {
                shell.set_config_name_text("  Arena  ".to_owned(), cx);
                shell.configs_create(cx);
            })
        });
        let created = cx.read(|cx| {
            let shell = handle.read(cx);
            (
                shell.profiles().configs().to_vec(),
                shell.config_name().to_owned(),
                shell.profiles().status().message.clone(),
            )
        });
        assert_eq!(created.0, ["Arena".to_owned()]);
        assert_eq!(created.1, "", "the name box clears after a create");
        assert_eq!(created.2, "Created config 'Arena'.");

        // Delete asks before it removes anything.
        cx.update(|cx| handle.update(cx, |shell, cx| shell.configs_request_delete(cx)));
        assert_eq!(
            cx.read(|cx| handle.read(cx).profiles().delete_armed().map(str::to_owned)),
            Some("Arena".to_owned())
        );
        assert!(aoko_platform::ProfileStore::new(&root).profile_exists("Arena"));

        cx.update(|cx| handle.update(cx, |shell, cx| shell.configs_cancel_delete(cx)));
        assert!(aoko_platform::ProfileStore::new(&root).profile_exists("Arena"));

        cx.update(|cx| handle.update(cx, |shell, cx| shell.configs_request_delete(cx)));
        cx.update(|cx| handle.update(cx, |shell, cx| shell.configs_confirm_delete(cx)));
        assert!(!aoko_platform::ProfileStore::new(&root).profile_exists("Arena"));
        assert_eq!(
            cx.read(|cx| handle.read(cx).profiles().status().message.clone()),
            "Deleted config 'Arena'."
        );

        // With the last config gone nothing is selected, and Delete says so.
        cx.update(|cx| handle.update(cx, |shell, cx| shell.configs_request_delete(cx)));
        assert_eq!(
            cx.read(|cx| handle.read(cx).profiles().status().message.clone()),
            "Select a config to delete."
        );
    }

    #[gpui::test]
    fn creating_over_an_existing_name_asks_then_overwrites(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let handle = cx.update(|cx| {
            cx.new(|_| Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root)))
        });

        cx.update(|cx| {
            handle.update(cx, |shell, cx| {
                shell.set_config_name_text("Arena".to_owned(), cx);
                shell.configs_create(cx);
            })
        });

        // A blank name reports the managed refusal rather than doing nothing.
        cx.update(|cx| {
            handle.update(cx, |shell, cx| {
                shell.set_config_name_text("   ".to_owned(), cx);
                shell.configs_create(cx);
            })
        });
        assert_eq!(
            cx.read(|cx| handle.read(cx).profiles().status().message.clone()),
            "Enter a valid config name (letters, numbers, spaces, - or _)."
        );

        // Same name again: the prompt, not a write.
        cx.update(|cx| {
            handle.update(cx, |shell, cx| {
                shell.set_config_name_text("Arena".to_owned(), cx);
                shell.profile.gui_theme = "Ink".to_owned();
                shell.configs_create(cx);
            })
        });
        assert_eq!(
            cx.read(|cx| handle
                .read(cx)
                .profiles()
                .create_pending_name()
                .map(str::to_owned)),
            Some("Arena".to_owned())
        );
        let on_disk = aoko_platform::ProfileStore::new(&root)
            .read_profile("Arena")
            .expect("still there");
        assert_eq!(
            aoko_protocol::profile::load_profile_from_json(&on_disk)
                .expect("parses")
                .gui_theme,
            "Slate",
            "unconfirmed means untouched"
        );

        cx.update(|cx| handle.update(cx, |shell, cx| shell.configs_overwrite_confirmed(cx)));
        assert_eq!(
            cx.read(|cx| handle
                .read(cx)
                .profiles()
                .create_pending_name()
                .map(str::to_owned)),
            None
        );
        let on_disk = aoko_platform::ProfileStore::new(&root)
            .read_profile("Arena")
            .expect("overwritten");
        assert_eq!(
            aoko_protocol::profile::load_profile_from_json(&on_disk)
                .expect("parses")
                .gui_theme,
            "Ink"
        );
        assert_eq!(
            cx.read(|cx| handle.read(cx).profiles().status().message.clone()),
            "Created config 'Arena'."
        );
    }

    #[gpui::test]
    fn the_open_folder_button_reports_the_missing_capability(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let handle = cx.update(|cx| {
            cx.new(|_| Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root)))
        });

        // Tests do not probe the session (`Shell::from_system` does), so the button
        // must report rather than attempt a launch.
        cx.update(|cx| handle.update(cx, |shell, cx| shell.configs_open_folder(cx)));
        assert_eq!(
            cx.read(|cx| handle.read(cx).profiles().status().message.clone()),
            "Could not open configs folder: no file manager launcher in this session."
        );
    }

    #[gpui::test]
    fn the_text_fields_are_created_once_and_seeded_from_the_state(cx: &mut gpui::TestAppContext) {
        let handle = cx.update(|cx| {
            cx.new(|_| {
                let mut shell = Shell::new(Theme::slate());
                shell.accounts.set_paste_text("seed token");
                shell.accounts.set_offline_name("Steve");
                shell.servers.set_add_name("Arena");
                shell.servers.set_add_address("play.example.net");
                shell.servers.set_direct_address("host:25565");
                shell.config_name = "MyConfig".to_owned();
                shell
            })
        });

        cx.update(|cx| handle.update(cx, |shell, cx| shell.ensure_form_fields(cx)));
        let seeded = cx.read(|cx| {
            let shell = handle.read(cx);
            (
                shell
                    .fields
                    .token
                    .as_ref()
                    .expect("token")
                    .read(cx)
                    .text()
                    .to_owned(),
                shell
                    .fields
                    .offline_name
                    .as_ref()
                    .expect("offline")
                    .read(cx)
                    .text()
                    .to_owned(),
                shell
                    .fields
                    .server_name
                    .as_ref()
                    .expect("name")
                    .read(cx)
                    .text()
                    .to_owned(),
                shell
                    .fields
                    .server_address
                    .as_ref()
                    .expect("address")
                    .read(cx)
                    .text()
                    .to_owned(),
                shell
                    .fields
                    .direct_address
                    .as_ref()
                    .expect("direct")
                    .read(cx)
                    .text()
                    .to_owned(),
                shell
                    .fields
                    .config_name
                    .as_ref()
                    .expect("config")
                    .read(cx)
                    .text()
                    .to_owned(),
            )
        });
        assert_eq!(
            seeded,
            (
                "seed token".to_owned(),
                "Steve".to_owned(),
                "Arena".to_owned(),
                "play.example.net".to_owned(),
                "host:25565".to_owned(),
                "MyConfig".to_owned(),
            ),
            "a field starts from the state it views"
        );

        // Recreating them per frame would drop focus and the caret, so a second call
        // must hand back the same entities.
        let before = cx.read(|cx| handle.read(cx).fields.token.clone().expect("token"));
        cx.update(|cx| handle.update(cx, |shell, cx| shell.ensure_form_fields(cx)));
        let after = cx.read(|cx| handle.read(cx).fields.token.clone().expect("token"));
        assert_eq!(before, after);

        // The widget's change path writes back into the state it views.
        cx.update(|cx| handle.update(cx, |shell, cx| shell.set_token_text("typed".to_owned(), cx)));
        assert_eq!(
            cx.read(|cx| handle.read(cx).accounts().paste_text().to_owned()),
            "typed"
        );
    }

    #[gpui::test]
    fn typing_in_the_config_name_box_reaches_the_shell(cx: &mut gpui::TestAppContext) {
        use gpui::{Keystroke, WindowOptions};

        let root = scratch_root();
        let handle = cx.update(|cx| {
            cx.new(|cx| {
                let mut shell =
                    Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root));
                shell.select_tab_named("settings", cx);
                shell.ensure_form_fields(cx);
                shell
            })
        });

        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| handle.clone())
                .expect("test window")
        });

        let focus = cx.read(|cx| {
            handle
                .read(cx)
                .fields
                .config_name
                .as_ref()
                .expect("config field")
                .read(cx)
                .focus_handle()
                .clone()
        });
        window
            .update(cx, |_, window, cx| window.focus(&focus, cx))
            .expect("focus the config name box");

        let window_handle: gpui::AnyWindowHandle = window.into();
        for key in ["p", "v", "p"] {
            cx.dispatch_keystroke(window_handle, Keystroke::parse(key).expect("keystroke"));
        }
        assert_eq!(
            cx.read(|cx| handle.read(cx).config_name().to_owned()),
            "pvp",
            "the widget's on_change writes into the shell's config name"
        );
    }

    /// Serialises the tests that write the process-wide animation gate, and restores it
    /// even when an assertion panics: the re-review caught a failed mutation leaving the
    /// flag `false` and taking an unrelated test down with it.
    ///
    /// Full independence is not reachable from this module. The gate is one `AtomicBool`
    /// owned by [`crate::theme::motion`], whose own `the_client_setting_drives_the_gate`
    /// asserts the ambient default is `true` and cannot take this lock; `theme/` is the
    /// component builder's file, so the fix is a `motion`-exposed test lock rather than an
    /// edit here. What this module can do, and does: order the tests *we* write the gate
    /// in, restore it on unwind through [`GateReset`], and restore it immediately after
    /// each assertion, so the window a concurrently scheduled test could observe is the
    /// write plus the read instead of the whole test.
    static MOTION_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct GateReset;

    impl Drop for GateReset {
        fn drop(&mut self) {
            motion::set_animations_enabled(true);
        }
    }

    fn gate_lock() -> std::sync::MutexGuard<'static, ()> {
        let guard = MOTION_GATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Start from the shipped default rather than whatever a previous test left, so
        // "the gate did not move" is a claim about this test's work.
        motion::set_animations_enabled(true);
        guard
    }

    #[test]
    fn the_startup_read_applies_the_saved_animation_setting() {
        let _gate = gate_lock();
        let _reset = GateReset;

        let root = scratch_root();
        let file = crate::views::prefs::PrefsFile::at(root.join(crate::views::prefs::FILE_NAME));
        file.save(crate::views::prefs::ClientPrefs { animations: false })
            .expect("write the preference");

        // The same body `from_system` runs, with a temporary file instead of the
        // machine's app-data directory. Deleting the gate write from `Shell::start`
        // fails here, which is what pins the startup read.
        let shell = Shell::start(
            Theme::slate(),
            (
                Profiles::unavailable(),
                aoko_protocol::profile::Profile::default(),
            ),
            Ok(file),
        );

        assert!(!shell.animations(), "the saved value is applied at startup");
        assert!(
            !motion::animations_enabled(),
            "the gate is set before the first frame, not at the first repaint"
        );
        // Restore now, not at the end of the test: `GateReset` covers the panic path, this
        // keeps the contaminated window to the two lines above.
        motion::set_animations_enabled(true);
        assert_eq!(shell.prefs_status(), "");
        assert!(
            shell.prefs.is_some(),
            "and the file stays attached for later writes"
        );
    }

    #[test]
    fn an_unreadable_prefs_file_is_reported_and_the_gate_is_untouched() {
        let _gate = gate_lock();
        let _reset = GateReset;

        let root = scratch_root();
        let path = root.join(crate::views::prefs::FILE_NAME);
        std::fs::write(&path, "not json").expect("write a corrupt file");
        let before = motion::animations_enabled();

        let shell = Shell::start(
            Theme::slate(),
            (
                Profiles::unavailable(),
                aoko_protocol::profile::Profile::default(),
            ),
            Ok(crate::views::prefs::PrefsFile::at(&path)),
        );

        assert!(
            shell
                .prefs_status()
                .contains("Could not read client preferences"),
            "a corrupt file is card copy, not a silent default: {}",
            shell.prefs_status()
        );
        assert_eq!(
            motion::animations_enabled(),
            before,
            "the gate did not move"
        );
    }

    #[test]
    fn a_prefs_directory_that_cannot_be_found_is_reported() {
        let shell = Shell::start(
            Theme::slate(),
            (
                Profiles::unavailable(),
                aoko_protocol::profile::Profile::default(),
            ),
            Err(aoko_platform::PlatformError::unsupported(
                "no app-data directory",
            )),
        );

        assert!(
            shell
                .prefs_status()
                .contains("Could not locate client preferences"),
            "{}",
            shell.prefs_status()
        );
    }

    #[gpui::test]
    fn flipping_the_animations_row_persists_and_gates(cx: &mut gpui::TestAppContext) {
        let _gate = gate_lock();
        let _reset = GateReset;

        let root = scratch_root();
        let prefs = crate::views::prefs::PrefsFile::at(root.join(crate::views::prefs::FILE_NAME));
        let shell = cx.update(|cx| {
            cx.new(|_| {
                let mut shell = Shell::new(Theme::slate());
                shell.set_prefs(prefs.clone());
                shell
            })
        });

        assert!(cx.read(|cx| shell.read(cx).animations()));

        cx.update(|cx| shell.update(cx, |shell, cx| shell.toggle_animations(cx)));
        assert!(!cx.read(|cx| shell.read(cx).animations()));
        assert!(!motion::animations_enabled(), "the gate is process-wide");
        assert_eq!(
            prefs.load(),
            Ok(crate::views::prefs::ClientPrefs { animations: false }),
            "the flip reached the disk"
        );

        cx.update(|cx| shell.update(cx, |shell, cx| shell.toggle_animations(cx)));
        assert!(motion::animations_enabled());
        assert_eq!(
            prefs.load(),
            Ok(crate::views::prefs::ClientPrefs { animations: true })
        );
        // Already true here, but stated: this test must not end with the gate down.
        motion::set_animations_enabled(true);
    }

    #[gpui::test]
    fn the_bottom_right_strip_asks_for_a_window_resize(cx: &mut gpui::TestAppContext) {
        use gpui::{point, Modifiers, WindowOptions};

        let shell = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| shell.clone())
                .expect("test window")
        });

        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let (width, height) = visual.update(|window, cx| {
            let _ = window.draw(cx);
            let size = window.viewport_size();
            (size.width, size.height)
        });

        visual.simulate_mouse_down(
            point(width - px(2.0), height - px(2.0)),
            MouseButton::Left,
            Modifiers::default(),
        );

        assert_eq!(
            visual.cx.read(|cx| shell.read(cx).resize_requested),
            Some(gpui::ResizeEdge::BottomRight),
            "the corner strip is the hit target and asks for its own edge"
        );
    }

    /// A root view that renders exactly one switch arm, so the click lands at a known
    /// point instead of somewhere in a 1020x760 shell.
    struct SwitchProbe {
        shell: Entity<Shell>,
        theme: Theme,
    }

    impl Render for SwitchProbe {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let shell = self.shell.clone();
            let row = shell.read(cx).render_switch(
                &crate::views::copy::COMBAT[0],
                "Armed",
                "IsArmed",
                &self.theme,
                Motion::STILL,
                &shell,
            );
            div().size_full().child(div().w(px(200.0)).child(row))
        }
    }

    /// The blocker's second half: the arm's handler must reach `toggle_module`, so a click
    /// on a rendered switch flips the module it names.
    #[gpui::test]
    fn clicking_the_module_switch_arms_the_module(cx: &mut gpui::TestAppContext) {
        use gpui::{point, Modifiers, WindowOptions};

        let probe = cx.update(|cx| {
            let shell = cx.new(|_| Shell::new(Theme::slate()));
            cx.new(|_| SwitchProbe {
                shell,
                theme: Theme::slate(),
            })
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| probe.clone())
                .expect("test window")
        });

        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        let _ = visual.update(|window, cx| window.draw(cx));

        let before = visual
            .cx
            .read(|cx| probe.read(cx).shell.read(cx).is_module_on("autoclicker"));
        // The row is 200 wide with the switch at its right edge, so the switch occupies
        // x in [154, 200]; (177, 12) is inside the track.
        visual.simulate_click(point(px(177.0), px(12.0)), Modifiers::default());
        let after = visual
            .cx
            .read(|cx| probe.read(cx).shell.read(cx).is_module_on("autoclicker"));

        assert_ne!(
            before, after,
            "a click on the switch must flip the module it names"
        );
    }
    #[gpui::test]
    fn clicking_the_autoclicker_switch_flips_the_module(cx: &mut gpui::TestAppContext) {
        use gpui::{Modifiers, WindowOptions};

        let shell = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| shell.clone())
                .expect("test window")
        });
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        assert!(!cx.read(|cx| shell.read(cx).is_module_on("autoclicker")));

        let bounds = visual
            .debug_bounds("switch-IsArmed")
            .expect("the switch renders with its debug selector");
        visual.simulate_click(bounds.center(), Modifiers::default());
        assert!(
            cx.read(|cx| shell.read(cx).is_module_on("autoclicker")),
            "the click reached toggle_module"
        );

        let bounds = visual
            .debug_bounds("switch-IsArmed")
            .expect("the switch is still there");
        visual.simulate_click(bounds.center(), Modifiers::default());
        assert!(
            !cx.read(|cx| shell.read(cx).is_module_on("autoclicker")),
            "a second click flips it back"
        );
    }
    /// The header button is WPF's `Width="140"` on the button itself
    /// (`MainWindow.xaml:359`), not a 140 px slot with a content-sized button inside it.
    /// The re-review measured the slot version: a 71 px fill and a 54 px gap.
    #[gpui::test]
    fn the_keybind_button_is_140_wide_and_docked_right(cx: &mut gpui::TestAppContext) {
        use gpui::WindowOptions;

        let shell = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| shell.clone())
                .expect("test window")
        });
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let button = visual
            .debug_bounds("keybind-AutoClickerCard")
            .expect("the keybind button records its bounds in a debug build");
        assert_eq!(
            f32::from(button.size.width),
            140.0,
            "the button fills WPF's Width=140, not its label's width"
        );

        let switch = visual
            .debug_bounds("switch-IsArmed")
            .expect("the header's switch is beside it");
        assert!(
            button.origin.x >= switch.origin.x + switch.size.width + px(8.0),
            "DockPanel docks the button right with Margin=8,0,0,0: button at {}, switch ends at {}",
            f32::from(button.origin.x),
            f32::from(switch.origin.x + switch.size.width),
        );
    }

    /// The action-bind button is `DarkButton` at `Width="210"` (`MainWindow.xaml:525`),
    /// not a content-sized `Secondary` — that measured 96 px and carried a 1 px edge.
    #[gpui::test]
    fn the_action_bind_button_is_210_wide(cx: &mut gpui::TestAppContext) {
        use gpui::WindowOptions;

        let shell = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| shell.clone())
                .expect("test window")
        });
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);

        let tab = test_tab_of("AutoRodCard");
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named(tab, cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        scroll_into_view(&mut visual, "action-bind-AutoRod");

        let button = visual
            .debug_bounds("action-bind-AutoRod")
            .expect("the action-bind button records its bounds in a debug build");
        assert_eq!(
            f32::from(button.size.width),
            210.0,
            "the button fills Width=210 from the markup"
        );
    }

    /// The label follows `Profile::module_keys`, which is the one line that makes a
    /// bound module read its key. Every default key is zero, so a capture cannot tell
    /// this apart from a constant.
    #[test]
    fn the_keybind_label_reads_the_profile() {
        let mut profile = aoko_protocol::profile::Profile::default();
        assert_eq!(
            keybind_label(&profile, "aimassist"),
            "Bind: Unbound",
            "an absent key is zero, which the managed method formats as Unbound"
        );

        profile.module_keys.insert(String::from("aimassist"), 0x70);
        assert_eq!(
            keybind_label(&profile, "aimassist"),
            "Bind: F1",
            "the module's own key, formatted as `Bind: ` plus FormatVirtualKey(vk)"
        );
        assert_eq!(
            keybind_label(&profile, "triggerbot"),
            "Bind: Unbound",
            "and a module with no key in the same profile is untouched"
        );

        // The two action binds read their own profile fields, not `module_keys`.
        assert_eq!(
            action_bind_label(&profile, copy::ActionBind::AutoRod),
            "Action: Unbound"
        );
        profile.auto_rod_action_key = 0x02;
        assert_eq!(
            action_bind_label(&profile, copy::ActionBind::AutoRod),
            "Action: Mouse Right"
        );
        assert_eq!(
            action_bind_label(&profile, copy::ActionBind::Throwpot),
            "Action: Unbound",
            "and the other action keeps its own key"
        );
    }

    /// The tab whose card list holds `card_id`, for the switch tests.
    fn test_tab_of(card_id: &str) -> &'static str {
        copy::TABS
            .iter()
            .find(|tab| match tab.body {
                TabBody::Cards(cards) => cards.iter().any(|card| card.id == card_id),
                TabBody::Accounts | TabBody::Servers => false,
            })
            .map(|tab| tab.name)
            .expect("the card is in a tab")
    }

    #[test]
    fn every_switch_is_a_module_flag_or_its_own_profile_boolean() {
        let mut profile_bound = Vec::new();
        let mut module_bound = 0;
        for tab in copy::TABS {
            let TabBody::Cards(cards) = tab.body else {
                continue;
            };
            for card in cards {
                for control in card.controls {
                    let Control::Switch { bind, .. } = control else {
                        continue;
                    };
                    match switch_target(bind) {
                        SwitchTarget::Module(_) => module_bound += 1,
                        // A profile switch must have a field behind it, or its click
                        // writes nowhere. `snapshot` is the same table `value` reads.
                        SwitchTarget::Profile(bind) => {
                            assert!(
                                crate::views::bindings::snapshot(
                                    &aoko_protocol::profile::Profile::default()
                                )
                                .contains_key(bind),
                                "{bind} has no profile field"
                            );
                            profile_bound.push(bind);
                        }
                    }
                }
            }
        }

        profile_bound.sort_unstable();
        assert_eq!(
            profile_bound,
            [
                "AutoTotemElytra",
                "DevMode",
                "DiscordRpcEnabled",
                "ShowLogo",
                "ShowModuleList"
            ],
            "exactly the five the managed markup binds to a profile boolean"
        );
        assert_eq!(
            module_bound + profile_bound.len(),
            43,
            "and every switch is one of the two"
        );
    }

    /// Scrolls a control up into the viewport before clicking it.
    ///
    /// GPUI's hit test drops a point outside the viewport, and the tab body is a
    /// scroll container (`components::scroll`), so the cards below the fold at the
    /// test platform's 1536x1080 are scrolled up first. The scroll is a real
    /// `ScrollEvent` delivered to the same container a wheel reaches.
    fn scroll_into_view(visual: &mut gpui::VisualTestContext, selector: &'static str) {
        use gpui::{point, ScrollDelta, ScrollEvent, ScrollSource, TouchPhase};

        let viewport = visual.update(|window, _| window.viewport_size());
        let mut bounds = visual.debug_bounds(selector).expect("the control renders");
        let mut scrolls = 0;
        while bounds.center().y > viewport.height - px(24.0) && scrolls < 40 {
            visual.simulate_event(ScrollEvent {
                position: point(px(700.0), px(600.0)),
                delta: ScrollDelta::Pixels(point(px(0.0), px(-60.0))),
                modifiers: gpui::Modifiers::default(),
                touch_phase: TouchPhase::Moved,
                source: ScrollSource::Pointer,
            });
            visual.update(|window, cx| {
                let _ = window.draw(cx);
            });
            bounds = visual
                .debug_bounds(selector)
                .expect("the control still renders");
            scrolls += 1;
        }
        assert!(
            bounds.center().y < viewport.height,
            "{selector} is in the viewport after {scrolls} scrolls"
        );
    }

    #[gpui::test]
    fn the_five_non_module_switches_flip_their_own_boolean(cx: &mut gpui::TestAppContext) {
        use gpui::{Modifiers, WindowOptions};

        let shell = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| shell.clone())
                .expect("test window")
        });
        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);

        // (card, bind, selector, what the default profile says)
        let cases = [
            (
                "ModuleListCard",
                "ShowModuleList",
                "switch-ShowModuleList",
                true,
            ),
            (
                "DiscordRpcCard",
                "DiscordRpcEnabled",
                "switch-DiscordRpcEnabled",
                true,
            ),
            (
                "AutoTotemCard",
                "AutoTotemElytra",
                "switch-AutoTotemElytra",
                true,
            ),
            ("GuiCustomizationCard", "ShowLogo", "switch-ShowLogo", true),
            ("GuiCustomizationCard", "DevMode", "switch-DevMode", false),
        ];

        for (card, bind, selector, default_on) in cases {
            let tab = test_tab_of(card);
            cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named(tab, cx)));
            visual.update(|window, cx| {
                let _ = window.draw(cx);
            });

            assert_eq!(
                cx.read(|cx| shell.read(cx).value(bind, 0.0) > 0.5),
                default_on,
                "{card} paints the profile's value"
            );

            // Three of the five cards sit below the fold, so scroll before clicking.
            scroll_into_view(&mut visual, selector);
            let bounds = visual
                .debug_bounds(selector)
                .expect("the switch renders with its debug selector");
            visual.simulate_click(bounds.center(), Modifiers::default());
            assert_eq!(
                cx.read(|cx| shell.read(cx).value(bind, 0.0) > 0.5),
                !default_on,
                "{bind} flipped its own boolean"
            );

            if bind == "AutoTotemElytra" {
                assert!(
                    !cx.read(|cx| shell.read(cx).is_module_on("autototem")),
                    "the Elytra row must not flip the AutoTotem module"
                );
            }
        }
    }
}
