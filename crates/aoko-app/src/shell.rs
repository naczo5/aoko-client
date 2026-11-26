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
//!   a module switch goes through [`crate::runtime::Runtime`] into the
//!   [`AppController`] this shell owns, and a non-module switch straight into the
//!   `Profile`. [`Shell::new`] is the startup path: it opens
//!   [`crate::views::profiles::Profiles`], migrates legacy profiles, reads the
//!   auto-save slot and applies it, and the two close paths write it back — the same
//!   cadence as `MainWindow.xaml.cs:407-412` (load once) and `:1861-1871` (save on
//!   exit), with `:946-970` for a named save. Decision 3's animation setting is
//!   *not* in the profile: it lives in [`crate::views::prefs`], because `Profile` is
//!   a bridge wire type.
//! * **The bridge link.** [`crate::shell::Shell::from_system`] is the only place that
//!   starts the reader (`crate::ingest::Ingest`), and it starts it on a worker thread,
//!   so a missing bridge is a warning line rather than a stalled first frame. A shell
//!   built by a test or by the capture harness owns a headless runtime and never dials;
//!   `Shell::runtime` hands the status to whoever draws it.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    div, px, AnyElement, Context, Entity, KeyDownEvent, MouseButton, Render, SharedString, Window,
};

use aoko_core::app::AppController;
use aoko_core::search;
use aoko_protocol::block_esp::{self, BlockEspTargetData};

use crate::components;
use crate::components::text_input::TextInput;
use crate::components::ButtonVariant;
use crate::runtime::Runtime;
use crate::theme::metrics;
use crate::theme::motion::{self, Motion};
use crate::theme::{Color, Theme};
use crate::views::accounts_state::{AccountPanel, AccountsState, OfflineSubmit, PasteSubmit};
use crate::views::command_search::{
    CommandSearchState, KeyboardGate, RowFactTable, RowFacts, SearchKey, SearchKeyOutcome,
};
use crate::views::copy::{self, Card, Control, TabBody, MODULE_LIST_PREVIEW_SAMPLE};
use crate::views::forms::FormFields;
use crate::views::palette_store::{self, PaletteDraft, PaletteStore};
use crate::views::prefs::ClientPrefs;
use crate::views::profiles::{CreateOutcome, Profiles};
use crate::views::servers_state::{AddOutcome, JoinOutcome, PingRequest, ServersState};
use crate::views::stores::{NoStore, StoreBridge};

/// The root view.
pub struct Shell {
    theme: Theme,
    tab: usize,
    /// The one owner of module state: every switch reads and writes the controller
    /// inside it. See [`crate::runtime`] for why there is no second copy.
    runtime: Runtime,
    /// The session override for every `bind` path: the capture harness seeds it and a
    /// press writes it. A value here wins over the profile snapshot below.
    draft: BTreeMap<&'static str, f64>,
    /// The persisted settings. Sliders, checks and combos read and write this through
    /// [`crate::views::bindings`]; see [`Shell::profile_json`] for saving it.
    profile: aoko_protocol::profile::Profile,
    /// Every profile-backed control value, by bind path, resolved once.
    values: BTreeMap<String, crate::views::bindings::ControlValue>,
    /// The capture button behind every keybind and action bind, keyed by what it writes.
    ///
    /// An entity per button rather than a fresh one per frame: a capture holds focus and a
    /// listening state across paints, so the button has to outlive the frame that drew it.
    captures: HashMap<KeyTarget, HeldCapture>,
    /// The line the keybind area shows after a refused module capture.
    ///
    /// The module rule is managed (`InputHooks.cs:124-134`), but where managed reports it is
    /// not: it writes it into the Auto Rod row's status line (`MainWindow.xaml.cs:2071`), so a
    /// module's conflict appears under a control it is not about. Here the line sits with the
    /// keybind buttons, names both bindings, and is replaced by the next module capture's
    /// outcome — a deliberate, user-visible deviation recorded in the trade-offs.
    keybind_conflict: Option<SharedString>,
    /// What each action row says about its last capture, indexed by [`status_slot`].
    ///
    /// `None` until that row has an outcome to report, and the explanatory sentence stays in
    /// the card's `Note` either way. Managed replaces that sentence in place
    /// (`MainWindow.xaml.cs:2049-2059`), which is the wart this avoids: nothing here is
    /// overwritten, so the copy comes back by the line being dropped.
    action_status: [Option<SharedString>; 2],
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
    /// The Block ESP add box (`MainWindow.xaml:720`).
    ///
    /// A text field is an entity, so it exists from the first render on rather than from
    /// [`Shell::new`]. It sits here rather than in [`FormFields`] because it is the one box the
    /// shell submits itself — on Enter and on the button beside it — rather than a form the
    /// account and server panels own.
    block_add: Option<Entity<TextInput>>,
    /// The palette file, the applied name, and the create-palette card's four hex drafts.
    ///
    /// The one owner of those four strings, for the same reason [`Shell::profile`] is the one
    /// owner of its fields: the boxes, the swatches and the Add press all read and write the
    /// draft, and nothing else holds a copy of it.
    ///
    /// It is plain owned UI state — no `Arc`, no `RwLock` — because nothing off this thread has
    /// any use for a palette. That is what makes the repaint borrow free: [`Shell::ensure_color_fields`]
    /// reads `self.palette.draft()` to seed each box once, which is a borrow of one field of the
    /// view it is already borrowing, and a borrow cannot block a writer the way a guard can. The
    /// store's own file reads (`palettes`, `resolve`, `save_draft`) happen on the same thread and
    /// never inside a paint.
    palette: PaletteStore,
    /// The four palette-editor hex boxes, by managed box name.
    ///
    /// The box is a mirror of the draft field, written through [`Shell::set_color_text`] on every
    /// edit and seeded from the draft once, here. A field typed into needs an entity, and the
    /// entity is the only text this view keeps: the draft below is where the value lives.
    color_fields: HashMap<&'static str, Entity<TextInput>>,
    /// The colour each swatch last parsed out of its box, by managed box name.
    ///
    /// A memo of the last successful parse, not a second copy of the text: the draft keeps an
    /// invalid entry visible so the user can fix it (`PaletteDraft`'s own rule), and the swatch
    /// keeps the colour it had rather than flashing empty. Seeded from the draft's own defaults,
    /// so the first frame paints what the managed card paints.
    swatches: HashMap<&'static str, Color>,
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
    ///
    /// This is also where the runtime is built, because it is the one constructor that
    /// runs with a window in front of it: [`Runtime::probed`] is the only caller of
    /// `aoko_platform::probe_capabilities`, and the report feeds both the controller and
    /// the "Open Configs Folder" row's `open_path`. `Shell::start` deliberately does
    /// not probe — three tests call it with a temporary store — so a test-built shell
    /// owns a [`Runtime::headless`] controller and the suite cannot depend on the
    /// machine. [`Shell::new`] needs no `Context` for either step: the controller is not
    /// an entity, so nothing here waits for `cx.new`.
    ///
    /// The bridge reader is started here too, and only here: it dials on its own thread,
    /// so a bridge that is not running costs a `warn!` instead of a stalled first frame.
    /// Supervision is switched on in the same place, which is what makes a module switch
    /// start and stop the automation loops — a shell built for a test never gets that far,
    /// so no test and no capture run owns an automation thread.
    #[must_use]
    pub fn from_system(theme: Theme) -> Self {
        let capabilities = aoko_platform::probe_capabilities();
        let mut shell = Self::start(
            theme,
            Profiles::from_system(),
            crate::views::prefs::PrefsFile::from_system(),
        );

        // `CapabilityReport::open_path` is the flag the "Open Configs Folder" row is
        // keyed on: Linux needs `xdg-open` on `PATH`, and a session without it must
        // not look like it can reveal the folder.
        shell.open_configs_folder = capabilities.open_path;
        shell.runtime = Runtime::new(Arc::new(AppController::new(capabilities)));
        // Before the link dials: the `config` frame is built from the controller's profile,
        // and the profile `start` loaded above went into the controller this line replaces.
        // Pushing first means the first frame the bridge sees already carries the saved
        // settings, rather than the defaults and then a correction.
        shell.push_profile();
        // Nothing else connects the store to a bridge; see `crate::ingest`.
        shell.runtime.connect_bridge();
        // From here on a module switch starts and stops loop threads; see
        // `Runtime::start_supervision`.
        shell.runtime.start_supervision();
        // The playtime clock starts with the window, as managed starts its own timer here
        // (`MainWindow.xaml.cs:421`). A shell built for a test or a capture never gets this
        // far, so its playtime stays at zero.
        shell.runtime.start_session();
        shell
    }

    /// The startup body, with both I/O sources handed in.
    ///
    /// `from_system` is the only caller in the shipped binary; tests call this with a
    /// temporary store and a temporary preferences file, which is what pins the
    /// startup *read*. Before this seam, `cfg(test)` skipped the whole block, so
    /// deleting the gate write from startup left a green suite.
    ///
    /// It probes nothing: the capability report is [`Shell::from_system`]'s to fetch, so
    /// a test that calls this cannot observe the machine it runs on.
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
            // The capture harness and every unit test land here, so the controller is
            // built without a probe; `from_system` replaces it with the probed one.
            runtime: Runtime::headless(),
            draft: BTreeMap::new(),
            profile: aoko_protocol::profile::Profile::default(),
            values: BTreeMap::new(),
            captures: HashMap::new(),
            keybind_conflict: None,
            action_status: [None, None],
            search: CommandSearchState::new(),
            accounts: AccountsState::new(),
            servers: ServersState::new(),
            stores: Box::new(NoStore),
            profiles,
            fields: FormFields::default(),
            block_add: None,
            // Seeded without reading the file: `from_system` only resolves the path, and `at`
            // touches nothing. The reads are `palettes()`/`resolve()`, the write is
            // `save_draft()`, and no constructor calls any of them — which is what keeps a unit
            // test and a screenshot off the machine's palette directory.
            palette: PaletteStore::from_system()
                .unwrap_or_else(|_| PaletteStore::at(std::path::PathBuf::new())),
            color_fields: HashMap::new(),
            swatches: HashMap::new(),
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
    ///
    /// This is one of the two places a profile is *loaded*, so it is where the load-time half
    /// of the action-bind rule runs
    /// ([`aoko_protocol::profile::Profile::normalize_action_binds`]): the module binds
    /// are applied first and win, and a dedicated Auto Rod or Throwpot key that duplicates one
    /// is cleared, as `Profile.cs:889-899` clears it on apply. A profile whose action binds are
    /// distinct changes nothing — the function returns false and the values are untouched — so
    /// the return is not needed here. The snapshot is taken after normalisation, because a
    /// cleared key has to paint as `Unbound` rather than as the key the file survives with.
    fn with_loaded_profile(mut self, mut profile: aoko_protocol::profile::Profile) -> Self {
        profile.normalize_action_binds();
        self.values = crate::views::bindings::snapshot(&profile);
        self.profile = profile;
        // The frame is built from the controller, not from this shell: a shell that starts
        // from a saved profile has to hand it over before anything reads it.
        self.push_profile();
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
        // The store stamps the profile's name before it writes (`save_auto`), so the copy it
        // hands back is not always the one that went in. A write site that keeps the stamped
        // copy without telling the controller leaves the two out of step from the first save.
        self.push_profile();
        result
    }

    /// The Settings card's Save: writes the selected config name.
    pub fn save_config(&mut self, name: &str) -> Result<(), String> {
        let mut profile = self.profile.clone();
        let result = self.profiles.save_named(name, &mut profile);
        self.profile = profile;
        // Same as [`Shell::save_profile`]: `save_named` sets `profile.name` on the way through.
        self.push_profile();
        result
    }

    /// The Settings card's Load: replaces the live settings with a saved config.
    pub fn load_config(&mut self, name: &str) -> Result<(), String> {
        match self.profiles.load_named(name) {
            Ok(mut profile) => {
                // The load-time half of the action-bind rule; see `with_loaded_profile`.
                profile.normalize_action_binds();
                self.values = crate::views::bindings::snapshot(&profile);
                self.profile = profile;
                // The palette travels in the profile, so applying a config has to
                // repaint with the theme it names (`ApplyLoadedProfile`,
                // `MainWindow.xaml.cs:835-848`).
                self.theme = crate::views::palette_store::theme_for(&self.profile.gui_theme);
                // And the loaded settings are what the bridge should be running: the config
                // frame reads the controller's copy.
                self.push_profile();
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
                    // Managed stops its playtime timer in `OnClosed`
                    // (`MainWindow.xaml.cs:1865`), not on disconnect, and keeps the total; see
                    // `Runtime::stop_session`.
                    shell.runtime.stop_session();
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

    /// The palette store: the file, the applied palette and the editor's draft.
    #[must_use]
    pub fn palette(&self) -> &PaletteStore {
        &self.palette
    }

    /// Points the palette store at an explicit file, for a test or a capture.
    ///
    /// The seam [`Profiles::with_store`] gives the profile side: a test that saves a draft
    /// should write into its scratch root rather than this machine's palette file. It builds a
    /// fresh store, so the draft returns to [`PaletteDraft::default`] — which is why it belongs
    /// before the first frame and not in the middle of an edit.
    pub fn set_palette_path(&mut self, path: impl Into<std::path::PathBuf>) {
        self.palette = PaletteStore::at(path);
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

    /// Creates the Block ESP add box, once.
    ///
    /// Called at the top of [`Render::render`] for the same reason as
    /// [`Shell::ensure_form_fields`]: an entity needs a context and [`Shell::new`] has none. The
    /// box is not in [`FormFields`] because nothing else owns its text — [`Shell::add_block_target`]
    /// reads it straight off the entity, so there is no second copy to fall out of step.
    fn ensure_block_add(&mut self, cx: &mut Context<Self>) {
        if self.block_add.is_some() {
            return;
        }
        let theme = self.theme.clone();
        // The placeholder is empty rather than a literal: the managed box draws the
        // `Block id, e.g. …` sentence as its tooltip, and inventing wording here is exactly
        // what `views::copy` exists to prevent.
        self.block_add = Some(cx.new(|cx| TextInput::new(cx, theme.clone(), "")));
    }

    /// Creates the four palette-editor boxes, once each.
    ///
    /// The seed is the store's draft — the one owner of those four strings — and the first paint
    /// parses it into the swatch memo, so the card shows the draft's colours before anything is
    /// typed.
    ///
    /// The placeholder is empty: the managed `HexBox` has no placeholder in `views/copy.rs`, and
    /// inventing one is what that file exists to prevent. A recorded gap, not an omission.
    fn ensure_color_fields(&mut self, cx: &mut Context<Self>) {
        let theme = self.theme.clone();
        for bind in COLOR_FIELDS {
            if self.color_fields.contains_key(bind) {
                continue;
            }

            let seed = draft_field(self.palette.draft(), bind)
                .cloned()
                .unwrap_or_default();
            if let Some(color) = palette_store::parse_entry(&seed) {
                self.swatches.insert(bind, color);
            }

            let handle = cx.entity().downgrade();
            let input = cx.new(|cx| {
                TextInput::new(cx, theme.clone(), "").on_change(move |text, _window, cx| {
                    if let Some(shell) = handle.upgrade() {
                        shell.update(cx, |shell, cx| {
                            shell.set_color_text(bind, text.as_ref(), cx);
                        });
                    }
                })
            });
            if !seed.is_empty() {
                input.update(cx, |input, cx| input.set_text(seed, cx));
            }
            self.color_fields.insert(bind, input);
        }
    }

    /// The one write path for a palette row's text: the box's `on_change`, the Enter commit and
    /// the colour picker all land here.
    ///
    /// The draft is the owner, so the text is stored even when it does not parse: `PaletteDraft`
    /// deliberately keeps an invalid entry visible so it can be fixed, and the Add press is where
    /// it is refused. The swatch's memo only moves on a successful parse, so a half-typed `#0A`
    /// leaves the colour it had rather than flashing empty.
    ///
    /// [`palette_store::parse_entry`] is the tolerant reader the Add press itself uses, so a form
    /// the swatch accepts is a form the press accepts — `#123`, `123` and `#123456` all land.
    fn set_color_text(&mut self, bind: &'static str, text: &str, cx: &mut Context<Self>) {
        if let Some(stored) = draft_field_mut(self.palette.draft_mut(), bind) {
            if stored.as_str() != text {
                stored.clear();
                stored.push_str(text);
            }
        }

        if let Some(color) = palette_store::parse_entry(text) {
            if self.swatches.get(bind) != Some(&color) {
                self.swatches.insert(bind, color);
            }
        }
        cx.notify();
    }

    /// Writes one palette box back in the canonical `#RRGGBB` form, when it parses.
    ///
    /// Enter is the commit gesture. Normalising on every keystroke would rewrite the text under
    /// the caret, so a user typing `#0a0b0f` could never finish the string they started. A box
    /// that does not parse keeps exactly what was typed, which is what lets it be fixed.
    fn commit_color_text(&mut self, bind: &'static str, cx: &mut Context<Self>) {
        let Some(field) = self.color_fields.get(bind).cloned() else {
            return;
        };
        let text = field.read(cx).text().to_owned();
        let Some(color) = palette_store::parse_entry(&text) else {
            return;
        };

        let canonical = color.to_hex();
        self.set_color_text(bind, &canonical, cx);
        if text != canonical {
            field.update(cx, |field, cx| field.set_text(canonical, cx));
        }
    }

    /// One palette-editor row: the caption, the swatch, and the hex box beside it.
    ///
    /// `MainWindow.xaml:1962` is the swatch's geometry — 22×22, radius 3, a 1 px `#40FFFFFF`
    /// edge — and the edge is [`crate::theme::fixed::CONTRAST_LABEL`] at alpha `0x40` rather than
    /// a new colour literal.
    ///
    /// The swatch is painted and is not yet a picker host: the press has to open
    /// `dialogs::color_picker_view::ColorPickerView`, which no route in this app mounts yet. The
    /// hand cursor and the managed "Click to pick a color" tooltip stay, so the affordance is the
    /// one the managed card has; what it needs is reported rather than stubbed with a handler that
    /// does nothing.
    fn render_color_field(
        &self,
        label: &'static str,
        bind: &'static str,
        theme: &Theme,
    ) -> AnyElement {
        let swatch = self
            .swatches
            .get(bind)
            .copied()
            .unwrap_or(Color::TRANSPARENT);
        let row_selector = format!("palette-row-{bind}");
        let swatch_selector = format!("palette-swatch-{bind}");

        let field = match self.color_fields.get(bind) {
            Some(field) => field.clone().into_any_element(),
            // Created in `render`, before the tree, so this is the defensive answer.
            None => div().into_any_element(),
        };

        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .debug_selector(move || row_selector)
            .child(
                div()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(label),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .id(crate::reuse::stitched("palette-swatch-", "", bind))
                            .w(px(22.0))
                            .h(px(22.0))
                            .rounded(px(3.0))
                            .border_1()
                            .border_color(crate::theme::fixed::CONTRAST_LABEL.with_alpha(0x40))
                            .bg(swatch)
                            .cursor_pointer()
                            .tooltip(components::tooltip(theme, copy::PALETTE_SWATCH_TOOLTIP))
                            .debug_selector(move || swatch_selector),
                    )
                    .child(div().flex_1().min_w(px(0.0)).child(field)),
            )
            .into_any_element()
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
    /// The render path asks for this rather than for the formatted label: a capture button is
    /// built once per key, so the label is a pure function of what this returns.
    fn module_key(&self, module: &'static str) -> i32 {
        self.profile.module_keys.get(module).copied().unwrap_or(0)
    }

    // ---- keybind capture -------------------------------------------------------------

    /// Creates the capture button for every binding, and rebuilds any whose label no longer
    /// matches the profile.
    ///
    /// Called from `render` before the tree is assembled, which is where
    /// [`Shell::ensure_form_fields`] creates its entities too: an entity needs a context and
    /// [`Shell::new`] has none. One pass over 40 targets whose only work is a key comparison,
    /// so a frame that changed nothing builds nothing.
    ///
    /// The rebuild is deliberately not done from inside a capture's own callback — that would
    /// drop the entity from within its update. The callback writes the profile and notifies;
    /// the next paint sees the mismatch and replaces the button, so the label on screen is
    /// always the label of the key the profile holds.
    fn sync_keybind_captures(&mut self, cx: &mut Context<Self>) {
        for &target in key_targets() {
            self.ensure_capture(target, cx);
        }
    }

    /// One capture button, built or rebuilt from the profile's current key.
    fn ensure_capture(&mut self, target: KeyTarget, cx: &mut Context<Self>) {
        let vk = self.capture_key(target);
        if self.captures.get(&target).is_some_and(|held| held.vk == vk) {
            return;
        }

        let theme = self.theme.clone();
        let handle = cx.entity().downgrade();
        let (name, width, rest, pending) = match target {
            KeyTarget::Module(module) => (
                SharedString::from(format!("capture-keybind-{module}")),
                KEYBIND_WIDTH,
                SharedString::from(keybind_label(&self.profile, module)),
                SharedString::from(copy::KEYBIND_PENDING),
            ),
            KeyTarget::Action(which) => (
                SharedString::from(format!("capture-action-{}", which.id())),
                ACTION_BIND_WIDTH,
                SharedString::from(action_bind_label(&self.profile, which)),
                SharedString::from(copy::ACTION_BIND_PENDING),
            ),
        };

        let capture = cx.new(move |cx| {
            components::KeybindCapture::new(cx, theme, name, width, rest, pending).on_capture(
                move |result, _window, cx| {
                    // `Abandoned` is the one outcome with nothing to store: a capture that
                    // lost focus leaves the profile exactly as it was, and it also takes the
                    // last outcome's line away, because that line no longer describes what
                    // just happened. `Key(0)` is Escape, which is a value — the managed
                    // unbind — and does get written.
                    let Some(vk) = result.virtual_key() else {
                        if let Some(shell) = handle.upgrade() {
                            shell.update(cx, |shell, cx| shell.clear_capture_status(target, cx));
                        }
                        return;
                    };
                    if let Some(shell) = handle.upgrade() {
                        shell.update(cx, |shell, cx| shell.apply_captured_key(target, vk, cx));
                    }
                },
            )
        });

        self.captures.insert(target, HeldCapture { vk, capture });
    }

    /// The profile's key for one binding, `0` when it has none.
    ///
    /// The same read [`crate::views::keybind_text`] formats: an absent `module_keys` entry is
    /// `0`, which is the managed `GetModuleKey` and reads as `Unbound`.
    fn capture_key(&self, target: KeyTarget) -> i32 {
        match target {
            KeyTarget::Module(module) => self.module_key(module),
            KeyTarget::Action(copy::ActionBind::AutoRod) => self.profile.auto_rod_action_key,
            KeyTarget::Action(copy::ActionBind::Throwpot) => self.profile.throwpot_action_key,
        }
    }

    /// Writes one captured key into the profile, or refuses it and says why.
    ///
    /// The profile is the single owner: this is the same copy [`crate::views::bindings`]'
    /// controls write and [`Shell::save_profile`] persists, and there is no pending-key field
    /// beside it. A module key lands in `module_keys` (a map, so the entry is created here)
    /// and an action bind in its own field.
    ///
    /// Escape arrives as `0`, which is the managed unbind, and a stored `0` reads `Unbound` —
    /// so an escape is persisted rather than treated as a cancel. A key another binding
    /// already holds is refused before the profile is touched, which is
    /// [`InputHooks.SetModuleKey`] and `SetAutoRodActionKey`/`SetThrowpotActionKey`
    /// (`InputHooks.cs:124-156`); [`keybind_holder`] is the whole rule, and
    /// `Profile::normalize_action_binds` is the load-time half of it.
    fn apply_captured_key(&mut self, target: KeyTarget, vk: i32, cx: &mut Context<Self>) {
        // A module capture's outcome supersedes the line the previous one wrote, whatever it
        // said, so a refusal cannot outlive the attempt that caused it.
        if matches!(target, KeyTarget::Module(_)) {
            self.keybind_conflict = None;
        }

        match keybind_holder(&self.profile, target, vk) {
            Some(holder) => self.refuse_captured_key(target, holder, vk),
            None => {
                match target {
                    KeyTarget::Module(module) => {
                        self.profile.module_keys.insert(module.to_owned(), vk);
                    }
                    KeyTarget::Action(copy::ActionBind::AutoRod) => {
                        self.profile.auto_rod_action_key = vk;
                    }
                    KeyTarget::Action(copy::ActionBind::Throwpot) => {
                        self.profile.throwpot_action_key = vk;
                    }
                }
                if let KeyTarget::Action(which) = target {
                    self.action_status[status_slot(which)] =
                        Some(SharedString::from(copy::ACTION_BIND_UPDATED));
                }
                // The keybinds are the case a user notices: the `config` frame carries every
                // one of them, and the controller is the copy it is built from.
                self.push_profile();
            }
        }

        // What rebuilds the label: the next paint sees a key that no longer matches the one
        // the button was built from. See `sync_keybind_captures`.
        cx.notify();
    }

    /// Records a refusal on the status line that belongs to the binding that asked.
    ///
    /// A module's conflict reads in the keybind area and names both bindings; an action row's
    /// reads under its own button, in the managed words. The profile is untouched either way,
    /// which is what keeps a refused key out of the `config` frame.
    fn refuse_captured_key(&mut self, target: KeyTarget, holder: KeybindHolder, vk: i32) {
        match (target, holder) {
            (KeyTarget::Module(module), KeybindHolder::Action(action)) => {
                // Filled with `replace` rather than `format!`: the copy lives in `views/copy.rs`
                // as data, and `format!` takes a literal format string.
                let line = copy::MODULE_BIND_CONFLICT_NAMED
                    .replace("{key}", &crate::views::keybind_text::format_virtual_key(vk))
                    .replace("{holder}", action_bind_name(action))
                    .replace("{requested}", module_label(module));
                self.keybind_conflict = Some(SharedString::from(line));
            }
            (KeyTarget::Action(which), KeybindHolder::Module) => {
                self.action_status[status_slot(which)] =
                    Some(SharedString::from(copy::ACTION_BIND_CONFLICT));
            }
            // The two kinds are disjoint by target: `keybind_holder` answers an action for a
            // module and a module for an action, so this pair is unreachable.
            (KeyTarget::Module(_), KeybindHolder::Module)
            | (KeyTarget::Action(_), KeybindHolder::Action(_)) => {}
        }
    }

    /// Drops the status line the binding's last capture wrote.
    ///
    /// A capture that loses focus stores nothing, so the outcome on the row is stale: taking
    /// the line away is how the row reads the explanatory copy again. The repaint is
    /// unconditional, and that is the half that matters on screen: the button also has to go
    /// back to its resting label, and the label lives on the capture entity, so the frame is
    /// what paints it. A conditional notify here left the button reading
    /// `Bind: [Press key or mouse...]` after focus moved away.
    fn clear_capture_status(&mut self, target: KeyTarget, cx: &mut Context<Self>) {
        match target {
            KeyTarget::Module(_) => self.keybind_conflict = None,
            KeyTarget::Action(which) => self.action_status[status_slot(which)] = None,
        }
        cx.notify();
    }

    /// The capture button for one binding, as the tree paints it.
    ///
    /// [`Shell::sync_keybind_captures`] built it earlier in the same `render`, so the lookup
    /// cannot miss; an empty element is the defensive answer rather than a panic in a paint.
    fn render_capture(&self, target: KeyTarget) -> AnyElement {
        match self.captures.get(&target) {
            Some(held) => held.capture.clone().into_any_element(),
            None => div().into_any_element(),
        }
    }

    /// The capture button a binding renders, for the tests that read the label it paints.
    ///
    /// GPUI has no text readback, so the label assertion goes through the entity the button
    /// draws rather than through painted glyphs.
    #[cfg(test)]
    fn capture_entity(&self, target: KeyTarget) -> Option<Entity<components::KeybindCapture>> {
        self.captures.get(&target).map(|held| held.capture.clone())
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
    ///
    /// The flip lands in the controller ([`crate::runtime::Runtime::flip_module`]), not
    /// in a field of this view, which is what makes it observable to anything else
    /// holding an `Arc<AppController>`. A click on a row that names no module boolean
    /// changes nothing and does not repaint.
    pub fn toggle_module(&mut self, module_id: &str, cx: &mut Context<Self>) {
        if self.runtime.flip_module(module_id) {
            cx.notify();
        }
    }

    /// The switch state for a card, resolved through the search index's
    /// `toggle_property` map so a card and its overlay row cannot disagree.
    #[must_use]
    pub fn is_module_on(&self, module_id: &str) -> bool {
        self.runtime.module_enabled(module_id)
    }

    /// The `AppController` this shell writes through.
    ///
    /// The same `Arc` a transport owner or a click loop would hold, so a caller can
    /// watch a switch flip without asking this view what it drew.
    #[must_use]
    pub fn controller(&self) -> &Arc<AppController> {
        self.runtime.controller()
    }

    /// The runtime owner: the controller plus the bridge link behind it.
    ///
    /// A status line reads `runtime().ingest_status()` for connected, state age and
    /// freshness; nothing in this view draws it yet.
    #[must_use]
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
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
            // The controller's copy is what the `config` frame is built from, so it has to
            // see every accepted write; see `push_profile`.
            self.push_profile();
        }
        written
    }

    /// Hands the live profile to the controller, which is the copy the `config` frame reads.
    ///
    /// The shell owns the profile for the UI — [`crate::views::bindings`] writes it and
    /// [`Shell::save_profile`] persists it — while `AppController::config_payload` builds the
    /// frame from its own copy. Nothing else pushes between them, so every write that changes
    /// the profile calls this, or the bridge keeps the old value for the life of the link.
    ///
    /// Why two copies rather than one: the controller lends no profile, only clones it.
    /// `AppController::profile` returns `self.profile.read().clone()`, and it and
    /// `update_profile` are the whole read/write surface — a mapped `RwLockReadGuard`
    /// would need an accessor in `aoko-core`, which this slice may not add. Reading through the
    /// clone in the paint would allocate a whole profile once per capture label, and there is
    /// one of those per key target in `key_targets`, rebuilt every frame by
    /// `sync_keybind_captures`, so the borrow costs the allocation budget rather than this
    /// function.
    ///
    /// Every write site calls this. They are [`Shell::apply_control`] (every slider, checkbox,
    /// combo and module switch), [`Shell::apply_captured_key`], [`Shell::with_loaded_profile`],
    /// [`Shell::load_config`], [`Shell::save_profile`], [`Shell::save_config`],
    /// [`Shell::configs_create`] and [`Shell::configs_overwrite_confirmed`]; the last four
    /// because the store stamps `profile.name` on the copy it hands back. A grep for
    /// `self.profile = ` and `&mut self.profile` in this file lists the current assignments,
    /// and `every_catalog_bind_leaves_the_controllers_copy_equal` fails on a bind that goes
    /// through `apply_control` and forgets.
    ///
    /// `update_profile` rather than `replace_profile`: the latter normalises the action binds
    /// on the copy it is handed (`Profile::normalize_action_binds`), which is the rule for
    /// *applying a saved config*, not for a live edit. Normalisation runs on the two load
    /// paths instead, so the copy pushed here is already the normalised one. Assigning keeps
    /// the two copies equal, so what the UI shows is what the wire carries.
    fn push_profile(&mut self) {
        let profile = self.profile.clone();
        self.runtime.controller().update_profile(move |active| {
            *active = profile;
            true
        });
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
        // Through `apply_control`, like every other control: that is the one place that
        // refreshes the snapshot and hands the profile to the controller, and a second copy of
        // that sequence here was the shape that let a write reach the card and not the wire.
        if !self.apply_control(
            bind,
            crate::views::bindings::ControlValue::Number(index as f64),
        ) {
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
    ///
    /// It writes the same controller a click writes, so a seeded screenshot and a
    /// clicked switch cannot drift apart.
    pub fn seed_module(&mut self, module_id: &str, on: bool) {
        self.runtime.set_module_enabled(module_id, on);
    }

    /// `ResetStatsButton_Click` (`MainWindow.xaml.cs:1978`): clears the runtime's tracker.
    ///
    /// The shell holds no tracker of its own — the runtime owns the numbers and the view reads
    /// them through it — so the button is one call to the owner and then a repaint. That is
    /// what makes the reset observable: the next frame composes from the cleared tracker
    /// rather than from a copy the view kept.
    pub fn reset_stats(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.runtime.reset_stats() {
            cx.notify();
        }
    }

    /// The command-search state.
    ///
    /// Mounted now: `Ctrl+K`, a typed character and the rail button open it, the root listener
    /// handles Enter and Escape, and [`Shell::render_search`] paints
    /// [`crate::overlay::overlay`] over the rail and body while it is open.
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
        // `create` stamps `profile.name`; the controller's copy has to follow, as in
        // [`Shell::save_profile`].
        self.push_profile();
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
        // The overwrite path stamps the name too; see [`Shell::configs_create`].
        self.push_profile();
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
    /// Two callers, and they differ only in the seed: the rail button passes `None` and the
    /// type-to-open path passes the character that opened it.
    ///
    /// A click does not consult the text-field half of the gate: a press on the rail button is
    /// deliberate, and the managed button opens the overlay from any field. What it does
    /// consult is a live keybind capture, which owns the keyboard outright — an overlay opened
    /// under a capture would take keys the capture is waiting for.
    pub fn open_search(&mut self, seed: Option<&str>, cx: &mut Context<Self>) {
        let facts = self.row_facts();
        let dev_mode = self.profile.dev_mode;
        let gate = self.search_gate(None, cx);
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

    /// One window keystroke: the two controls that take a window key, in the order that decides
    /// which of them sees it.
    ///
    /// `Opened` and `Closed` are the outcomes worth stopping propagation for: the overlay has
    /// taken the keyboard, and letting the key travel on would also hit whatever is behind it.
    /// Everything else is `Ignored`, and the key goes on to the element that would have had it.
    fn handle_window_key(
        &mut self,
        event: &KeyDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> SearchKeyOutcome {
        // Enter in the Block ESP add box submits. `components::TextInput` deliberately leaves
        // Tab, Enter and Escape to its owner (`text_input.rs:421`), so a focused box hands the
        // key up the tree and this listener is where it lands. It is checked before the overlay
        // because a box with focus is a box the user is typing in.
        if event.keystroke.key.eq_ignore_ascii_case("enter")
            && !self.search.is_open()
            && self.block_add_focused(window, cx)
        {
            self.add_block_target(cx);
            return SearchKeyOutcome::Ignored;
        }

        // Enter in a palette box commits its hex in the canonical `#RRGGBB` form. Same route,
        // same reason: the widget leaves Enter to its owner.
        if event.keystroke.key.eq_ignore_ascii_case("enter") && !self.search.is_open() {
            if let Some(bind) = self.color_field_focused(window, cx) {
                self.commit_color_text(bind, cx);
                return SearchKeyOutcome::Ignored;
            }
        }

        // Enter jumps the selected row, and only while the overlay is open. It is not a
        // `SearchKey` because the managed `Jump` is a query-field key, not a window gesture.
        // The query row is painted rather than focusable (`overlay::query_row`), so the root
        // listener is what receives it.
        if self.search.is_open() && event.keystroke.key.eq_ignore_ascii_case("enter") {
            self.perform_jump(cx);
            return SearchKeyOutcome::Closed;
        }

        let Some(key) = search_key_of(event) else {
            return SearchKeyOutcome::Ignored;
        };

        let facts = self.row_facts();
        let dev_mode = self.profile.dev_mode;
        let gate = self.search_gate(Some(window), cx);
        let outcome = self.search.handle_window_key(key, &gate, dev_mode, &facts);
        if outcome != SearchKeyOutcome::Ignored {
            cx.notify();
        }
        outcome
    }

    /// A click on one result row: select it, then perform the jump.
    fn jump_to_row(&mut self, index: usize, cx: &mut Context<Self>) {
        self.search.select(Some(index));
        self.perform_jump(cx);
    }

    /// Performs the jump the overlay asked for: the entry's tab, and the pulse on its card.
    ///
    /// A row click and Enter both come through here, and `jump_selected` is what closes the
    /// overlay and hands back the request, so there is one landing place rather than two.
    ///
    /// The pulse is started and the state pruned, but nothing paints a ring yet: the card
    /// border a view would draw is `views/`'s, and no frame loop advances a running pulse. The
    /// report names that as owed rather than pretending the jump is animated.
    fn perform_jump(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.search.jump_selected() else {
            return;
        };

        self.select_tab_named(request.entry.tab, cx);

        let now = self.runtime.now_ms();
        let motion_enabled = motion::motion().enabled();
        let _ = self.search.begin_pulse(&request, now, motion_enabled);

        // `entry.card_name` is what a scroll-into-view would target; the shell has no
        // scroll-to-card yet, so the tab change is the whole navigation.
        cx.notify();
    }

    /// The per-module facts the overlay's rows read.
    ///
    /// `supported` is the bridge's advertised list through [`Runtime::module_supported`], so
    /// the row and the capability report cannot disagree; `is_on` is the same flag the card's
    /// switch shows. `keybind_text` is empty for a module with no bind, which is the managed
    /// "not bindable" rather than an `Unbound` chip.
    fn row_facts(&self) -> RowFactTable {
        let mut facts = RowFactTable::default();
        for entry in search::entries() {
            let vk = self.profile.module_keys.get(entry.id).copied().unwrap_or(0);
            facts.set(
                entry.id,
                RowFacts {
                    supported: self.runtime.module_supported(entry.id),
                    is_on: self.runtime.module_enabled(entry.id),
                    keybind_text: if vk > 0 {
                        crate::views::keybind_text::format_virtual_key(vk)
                    } else {
                        String::new()
                    },
                },
            );
        }
        facts
    }

    /// `CanOpenCommandSearch`, as far as this port can answer it.
    ///
    /// `capturing_keybind` covers the managed hook flag *and* both pending action binds,
    /// because a keybind or action bind here is one `KeybindCapture` entity per binding.
    ///
    /// `control_mode` also carries the one condition the managed struct never needed: a text
    /// field with focus. WPF sent window keys to the window, so a field could not swallow a
    /// chord; GPUI sends a typed character to the focused field and bubbles it to this
    /// listener, so without the check a typed `a` in the token box would open the overlay as
    /// well as type. `window` is the focus authority and only a keystroke has one — see
    /// [`Shell::open_search`] for why a click passes `None`.
    fn search_gate(&self, window: Option<&Window>, cx: &App) -> KeyboardGate {
        KeyboardGate {
            control_mode: !window.is_some_and(|window| self.any_text_field_focused(window, cx)),
            capturing_keybind: self
                .captures
                .values()
                .any(|held| held.capture.read(cx).is_capturing()),
            ..KeyboardGate::default()
        }
    }

    /// Whether any of the shell's text fields owns the keyboard.
    ///
    /// Read off the entities rather than a tracked flag: six of them live in one place
    /// ([`FormFields`]) and a flag beside each would be a second copy of the focus GPUI already
    /// keeps. A new field added to `FormFields` is in this array, so the gate cannot forget it —
    /// and the Block ESP add box, which is the one text field that lives on the shell instead, is
    /// here for the same reason: a character typed into it must not open the search overlay.
    fn any_text_field_focused(&self, window: &Window, cx: &App) -> bool {
        [
            &self.fields.token,
            &self.fields.offline_name,
            &self.fields.server_name,
            &self.fields.server_address,
            &self.fields.direct_address,
            &self.fields.config_name,
            &self.block_add,
        ]
        .into_iter()
        .flatten()
        .chain(self.color_fields.values())
        .any(|field| field.read(cx).focus_handle().is_focused(window))
    }

    /// Whether the Block ESP add box owns the keyboard, which is what makes Enter a submit.
    fn block_add_focused(&self, window: &Window, cx: &App) -> bool {
        self.block_add
            .as_ref()
            .is_some_and(|field| field.read(cx).focus_handle().is_focused(window))
    }

    /// Which palette box owns the keyboard, for the Enter that commits its hex.
    fn color_field_focused(&self, window: &Window, cx: &App) -> Option<&'static str> {
        self.color_fields.iter().find_map(|(bind, field)| {
            field
                .read(cx)
                .focus_handle()
                .is_focused(window)
                .then_some(*bind)
        })
    }

    /// The command-search overlay, or `None` while it is closed.
    ///
    /// Mounted after the rail and the body row and before the resize strips, so the strips stay
    /// above the scrim: a resize edge the overlay swallowed would be a bug a user finds by
    /// trying. The pulse is pruned first, so a ring that has run out does not outlive its
    /// motion in the state.
    fn render_search(
        &self,
        theme: &Theme,
        motion: Motion,
        entity: &Entity<Self>,
        window: &Window,
    ) -> Option<AnyElement> {
        if !self.search.is_open() {
            return None;
        }

        let handle = entity.clone();
        Some(crate::overlay::overlay(
            theme,
            &self.search,
            motion,
            window.viewport_size(),
            move |index, _event, _window, cx| {
                handle.update(cx, |shell, cx| shell.jump_to_row(index, cx));
            },
        ))
    }

    /// `armed · 7 on`, rebuilt only when a toggle changes.
    ///
    /// `Render::render` wants it twice a frame — the title bar and the rail footer — and
    /// the text is a pure function of (armed, enabled count), so it is cached against
    /// those two values rather than formatted per call. `perf::tests` counts the
    /// difference.
    ///
    /// Both inputs are read from the controller, and reading them allocates nothing: the
    /// count is one `is_module_enabled` per catalog entry and the armed bit is one more.
    /// That is the same per-frame work the old `ModuleToggles` arithmetic did (one set
    /// lookup per entry), now against the one owner of module state.
    fn chip_text(&mut self) -> SharedString {
        let enabled = self.runtime.enabled_count();
        let key = (self.runtime.armed(), enabled);

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
        // One capture button per keybind and action bind, built before the tree is assembled:
        // a capture holds focus across frames, so its entity outlives the paint that drew it,
        // and this is also where a profile that changed rebuilds one.
        self.sync_keybind_captures(cx);
        // The add-block box is an entity too, and for the same reason it is created here
        // rather than in `Shell::new`.
        self.ensure_block_add(cx);
        // And the four palette boxes, for the same reason.
        self.ensure_color_fields(cx);

        // The GTB helper is polled here because this is the only per-frame hook the shell
        // has: the runtime decides whether there is work (module on, link live, frame fresh,
        // no menu, line changed), so an idle frame costs one atomic load, one `Arc` clone and
        // a `&str` comparison — no allocation, which is what keeps the section-6 render
        // budget intact.
        self.runtime.poll_gtb(Instant::now());
        // The playtime timer rides the same hook. It is the same shape of decision: with no
        // session running, or with no whole second since the last tick, it returns before it
        // touches the clock's value, so an idle frame costs one branch.
        self.runtime.sample_playtime();

        let motion = motion::motion();
        // The two chrome strings are built before the theme is borrowed: each needs
        // `&mut self` to consult its cache, and both are read only once the tree is
        // being assembled. `&self.theme` rather than a clone — a `Theme` carries a
        // `String` name, so cloning it was one heap allocation per frame.
        let chip = self.chip_text();
        let title = self.title_text(&chip);
        let theme = &self.theme;
        let entity = cx.entity();

        // A pulse that has run out is dropped before the tree is assembled, so a jump's ring
        // cannot outlive its motion in the state. Nothing paints it yet; see
        // [`Shell::perform_jump`].
        let now = self.runtime.now_ms();
        self.search.prune_pulse(now);

        let body = match copy::TABS.get(self.tab).map(|tab| tab.body) {
            // The Stats tab draws `views::stats_view`, which reads the runtime's tracker.
            Some(TabBody::Stats) => {
                let handle = cx.entity();
                crate::views::stats_view::render(
                    &self.runtime.stats(),
                    theme,
                    self.tab,
                    move |_, window, cx| {
                        handle.update(cx, |shell, cx| shell.reset_stats(window, cx));
                    },
                )
            }
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
            // The window-level keys, all of them: Ctrl+K, Escape, type-to-open and Enter.
            // GPUI dispatches a key from the focused node up the tree, and with nothing
            // focused the path starts at the root, so this listener is the one place a
            // keystroke is guaranteed to arrive.
            .on_key_down({
                let handle = entity.clone();
                move |event, window, cx| {
                    let outcome =
                        handle.update(cx, |shell, cx| shell.handle_window_key(event, window, cx));
                    // Only the two outcomes that take the keyboard stop it here.
                    if matches!(outcome, SearchKeyOutcome::Opened | SearchKeyOutcome::Closed) {
                        cx.stop_propagation();
                    }
                }
            })
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

        // The overlay sits above the rail and the body and below the resize strips: a strip
        // the scrim swallowed would be a resize a user cannot perform, which is why this is
        // one child rather than a wrapper around the tree.
        if let Some(search) = self.render_search(theme, motion, &entity, window) {
            root = root.child(search);
        }

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

        // The rail button opens the overlay, the same state Ctrl+K opens. It was inert in
        // Phase 2 while nothing mounted the view or listened for keys; both are wired now.
        rail = rail.child({
            let handle = entity.clone();
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
                move |_, _window, cx| {
                    handle.update(cx, |shell, cx| shell.open_search(None, cx));
                },
            )
            .into_any_element()
        });

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

        // The keybind area's one status line: a refused module capture names the key, the
        // binding that holds it and the module that asked. Above the cards rather than in one
        // of them, because the conflict is between a module bind and an action bind and
        // managed's choice of the Auto Rod row puts it under a control it is not about.
        if let Some(conflict) = &self.keybind_conflict {
            body = body.child(
                div()
                    .id(SharedString::from("keybind-conflict"))
                    .debug_selector(|| "keybind-conflict".to_owned())
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(conflict.clone()),
            );
        }

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
            // `Margin="8,0,0,0"`. The capture's own box sets 140, as WPF's `Width` does, so
            // the fill is 140 px rather than the label's width.
            let mut slot = div().ml_auto();
            if let Some(selector) = keybind_debug_selector(card.id) {
                slot = slot.debug_selector(move || selector.to_owned());
            }
            // The slot stays the shell's so the 140 px geometry test keeps a stable
            // selector; the button inside it is the capture, which is what makes a press
            // start listening (`KeybindButton_Click`, `MainWindow.xaml.cs:1943-1952`).
            row = row.child(slot.child(self.render_capture(KeyTarget::Module(module))));
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
        // internally, so this is where the click is routed: a module flag goes to the
        // controller through [`Runtime::flip_module`], a profile boolean writes its own
        // bind through `bindings`. Every switch has one of the two targets — the old
        // fallback to the card's module silently misrouted five of them.
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

            // The header arm paints this control on every card that has one
            // (`control_is_header`), so this is the same capture, reached by the same
            // target rather than by a second button.
            Control::Keybind { module } => self.render_capture(KeyTarget::Module(module)),

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

            // `AutoRodActionBindButton_Click` / `ThrowpotActionBindButton_Click`
            // (`MainWindow.xaml.cs:1954-1963`) start the same one-key capture the header
            // buttons do, and so does this arm: the target is the action field, not a card.
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
                let button = self.render_capture(KeyTarget::Action(*which));
                // The row's own outcome, under its own button. A line of its own rather than a
                // rewrite of the explanatory `Note` the card paints next, so the explanation is
                // never lost: the line appears with an outcome and is dropped by the next.
                slot = match &self.action_status[status_slot(*which)] {
                    None => slot.child(button),
                    Some(status) => {
                        let id = format!("action-status-{}", which.id());
                        let selector = id.clone();
                        slot.flex().flex_col().gap(px(6.0)).child(button).child(
                            div()
                                .id(SharedString::from(id))
                                .debug_selector(move || selector.clone())
                                .text_size(px(metrics::text::CAPTION))
                                .text_color(theme.dim_text)
                                .child(status.clone()),
                        )
                    }
                };
                slot.into_any_element()
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
                // Empty until the row controls land: the managed list has no empty state to
                // copy, because `BlockEspTargets` arrives populated by default. A placeholder
                // sentence here would be a port invention, not a gap to fill with wording.
                .into_any_element(),

            Control::BlockEspAdd { bind } => self.render_block_add(bind, theme, entity),

            Control::ColorField { label, bind } => self.render_color_field(label, bind, theme),

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

    /// The Block ESP add row: the box, the button beside it, and the id hint under it.
    ///
    /// The hint is the tooltip painted as a dim line, which is the shape
    /// [`Control::Check`]'s tooltip already takes in this tree; `components::TextInput` has no
    /// tooltip of its own, so the sentence sits where a user reads it rather than on hover.
    ///
    /// The row carries a debug selector because it is the only handle a test has on the mount:
    /// `components::button` gives the button an `ElementId` and no selector, the same shape that
    /// left the rail's search button unpaintable from a test.
    fn render_block_add(
        &self,
        bind: &'static str,
        theme: &Theme,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let selector = block_add_selector(bind);

        let field = match &self.block_add {
            Some(field) => field.clone().into_any_element(),
            // The entity is created in `render`, before the tree, so this is the defensive
            // answer rather than a state a laid-out frame can be in.
            None => div().into_any_element(),
        };

        let handle = entity.clone();
        let add = components::button(
            crate::reuse::stitched(bind, "-", "button"),
            theme,
            ButtonVariant::Dark,
            copy::BLOCK_ESP_ADD_LABEL,
            move |_, _window, cx| {
                handle.update(cx, |shell, cx| shell.add_block_target(cx));
            },
        );

        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .debug_selector(move || selector)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    // The box takes what the button leaves. The managed `Width` is not
                    // recorded anywhere in this tree, so this asks for no number rather
                    // than inventing one.
                    .child(div().flex_1().min_w(px(0.0)).child(field))
                    .child(add),
            )
            .child(
                div()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(copy::BLOCK_ESP_ID_TOOLTIP),
            )
            .into_any_element()
    }

    /// Adds the block the box names, or enables it when the list already has it.
    ///
    /// [`block_esp::normalize_id`] is the whole normalisation: `minecraft:diamond_ore`,
    /// `diamond_ore` and `block.minecraft.diamond_ore` all become `diamond_ore`, so a user can
    /// type any of the three. An empty result is the managed no-op — whitespace and punctuation
    /// alone store nothing and leave the box as it was.
    ///
    /// An id already tracked is *enabled* rather than appended: a second entry with the same
    /// registry id would paint twice and send twice. A new one is appended as
    /// `minecraft:{id}`, with the friendly name the managed code derives from the id, the
    /// default colour and `enabled: true`.
    ///
    /// The box is then cleared so the next add starts fresh, which is managed behaviour rather
    /// than an assumption: `MainWindow.xaml.cs:2013-2014` empties `BlockEspAddBox` at the end of
    /// `AddBlockEspFromBox`, on both the enable and the append branch, and the empty no-op at
    /// `:1998-1999` returns before it — so whitespace alone leaves what you typed, and a real id
    /// does not.
    fn add_block_target(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.block_add.clone() else {
            return;
        };
        let id = block_esp::normalize_id(Some(field.read(cx).text()));
        if id.is_empty() {
            return;
        }

        // The index rather than the target: the `None` arm appends, which needs the borrow to
        // be over.
        let existing =
            self.profile.block_esp_targets.iter().position(|target| {
                block_esp::normalize_id(Some(target.registry_id.as_str())) == id
            });
        match existing {
            Some(index) => self.profile.block_esp_targets[index].enabled = true,
            None => self.profile.block_esp_targets.push(BlockEspTargetData {
                registry_id: format!("minecraft:{id}"),
                display_name: id.replace('_', " "),
                color_hex: block_esp::DEFAULT_COLOR_HEX.to_owned(),
                enabled: true,
            }),
        }

        field.update(cx, |field, cx| field.set_text("", cx));
        // The profile is the owner the config frame is built from, so the add has to reach it
        // before it reaches the wire.
        self.push_profile();
        cx.notify();
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
    /// A catalogue module flag: the switch flips it in the controller through
    /// [`Runtime::flip_module`].
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

/// The Block ESP add row's debug selector.
///
/// The bind the row carries is a port-only identity, `blockesp-add`, and not the name of the
/// profile list the add appends to, so it needs no role prefix to read as the row it is: the
/// selector is exactly that name and the button inside it is `blockesp-add-button`. A test needs
/// the bounds to prove the row is mounted and to scroll it into view; the button has an
/// `ElementId` and no selector, so this is the only handle there is.
fn block_add_selector(bind: &'static str) -> String {
    bind.to_owned()
}

/// The four create-palette colour rows, in the markup's order.
///
/// `MainWindow.xaml:1963`, `:1974`, `:1985` and `:1996` are the boxes; the order is the one the
/// derived shades are read in, and it is the order the card paints them.
const COLOR_FIELDS: [&str; 4] = [
    "PaletteBgBox",
    "PalettePanelBox",
    "PaletteAccentBox",
    "PaletteTextBox",
];

/// The draft field one palette box edits, by managed name.
///
/// The four names and the four draft fields are exactly the pairing
/// [`crate::views::palette_store`] owns, so this is the seam that wants to be
/// `PaletteDraft::get(bind)` there (with the writable mirror beside it). Until then the pairing
/// lives here once, rather than a second set of initial hexes beside it: the box, the swatch and
/// the eventual picker all read the one string the draft holds.
fn draft_field<'a>(draft: &'a PaletteDraft, bind: &str) -> Option<&'a String> {
    match bind {
        "PaletteBgBox" => Some(&draft.background),
        "PalettePanelBox" => Some(&draft.panel),
        "PaletteAccentBox" => Some(&draft.accent),
        "PaletteTextBox" => Some(&draft.text),
        _ => None,
    }
}

/// The writable half of [`draft_field`].
fn draft_field_mut<'a>(draft: &'a mut PaletteDraft, bind: &str) -> Option<&'a mut String> {
    match bind {
        "PaletteBgBox" => Some(&mut draft.background),
        "PalettePanelBox" => Some(&mut draft.panel),
        "PaletteAccentBox" => Some(&mut draft.accent),
        "PaletteTextBox" => Some(&mut draft.text),
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

/// The header keybind button's `Width="140"` (`MainWindow.xaml:359`).
const KEYBIND_WIDTH: f32 = 140.0;

/// The action-bind buttons' `Width="210"` (`MainWindow.xaml:525`, `:542`).
const ACTION_BIND_WIDTH: f32 = 210.0;

/// One capturable binding: a card header's module key, or one of the two action binds.
///
/// A target names a field; it never holds the key. [`Shell::capture_key`] reads the profile,
/// so there is no second copy of the value to fall out of step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyTarget {
    /// `Profile::module_keys` under this catalogue id, from `Control::Keybind`.
    Module(&'static str),
    /// `Profile::auto_rod_action_key` or `Profile::throwpot_action_key`.
    Action(copy::ActionBind),
}

impl Hash for KeyTarget {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // `ActionBind` derives no `Hash` and `views/copy.rs` is not this slice's to edit;
        // its `id()` is injective, so the variant plus that name is the whole key.
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Module(module) => module.hash(state),
            Self::Action(which) => which.id().hash(state),
        }
    }
}

/// A capture button together with the key its label was built from.
struct HeldCapture {
    /// The virtual key the label was built from.
    ///
    /// A cache, not a copy of the value: [`Shell::profile`] owns the number, and a mismatch
    /// here is what rebuilds the button so its label follows the profile.
    vk: i32,
    capture: Entity<components::KeybindCapture>,
}

/// Every capturable binding in the table: one per keybind control, then the two actions.
///
/// Built once. The table is `'static`, so the list is too, and the per-frame work is one key
/// comparison per button rather than a walk of every card's controls.
fn key_targets() -> &'static [KeyTarget] {
    static TARGETS: OnceLock<Vec<KeyTarget>> = OnceLock::new();
    TARGETS.get_or_init(|| {
        let mut targets: Vec<KeyTarget> = copy::all_cards()
            .filter_map(card_module)
            .map(KeyTarget::Module)
            .collect();
        targets.push(KeyTarget::Action(copy::ActionBind::AutoRod));
        targets.push(KeyTarget::Action(copy::ActionBind::Throwpot));
        targets
    })
}

/// The catalogue module a card's keybind button names.
fn card_module(card: &Card) -> Option<&'static str> {
    card.controls.iter().find_map(|control| match control {
        Control::Keybind { module } => Some(*module),
        _ => None,
    })
}

/// The slot one action bind's status occupies in [`Shell::action_status`].
///
/// A two-element array rather than a map: `ActionBind` derives no `Hash` and `views/copy.rs`
/// is not this slice's to edit, and a slot turns the read into an index.
const fn status_slot(which: copy::ActionBind) -> usize {
    match which {
        copy::ActionBind::AutoRod => 0,
        copy::ActionBind::Throwpot => 1,
    }
}

/// What a refused capture collided with.
enum KeybindHolder {
    /// A module's general bind holds the key.
    ///
    /// Carries no id because nothing reads one: the managed action refusal is verbatim and
    /// names the binding kind only (`copy::ACTION_BIND_CONFLICT`), and the module-conflict
    /// line names the module that *asked*, which comes from the target through
    /// [`module_label`]. An id here would be a `String` clone per refused capture with no
    /// reader at the other end.
    Module,
    /// One of the two action binds.
    Action(copy::ActionBind),
}

/// Which binding already holds the key a capture asked for, or `None` when nothing does.
///
/// `InputHooks.cs:124-156`, verbatim: `SetModuleKey` refuses a key either action bind holds,
/// `SetAutoRodActionKey` and `SetThrowpotActionKey` refuse a key any general module bind
/// holds, and a key of `0` — Escape, the managed unbind — never conflicts, so unbinding always
/// lands. The two directions are disjoint, which is why the holder is exactly one of the two
/// kinds and never both.
fn keybind_holder(
    profile: &aoko_protocol::profile::Profile,
    target: KeyTarget,
    vk: i32,
) -> Option<KeybindHolder> {
    if vk <= 0 {
        return None;
    }

    match target {
        KeyTarget::Module(_) => [
            (copy::ActionBind::AutoRod, profile.auto_rod_action_key),
            (copy::ActionBind::Throwpot, profile.throwpot_action_key),
        ]
        .into_iter()
        .find(|(_, key)| *key == vk)
        .map(|(which, _)| KeybindHolder::Action(which)),
        KeyTarget::Action(_) => profile
            .module_keys
            .iter()
            .find(|(_, key)| **key == vk)
            .map(|_| KeybindHolder::Module),
    }
}

/// The name a card's header paints, which is the label a conflict line has to read.
fn header_label(card: &Card) -> Option<&'static str> {
    match copy::card_header(card) {
        Some(copy::CardHeader::Switch { label, .. } | copy::CardHeader::Text(label)) => Some(label),
        None => None,
    }
}

/// The name a module's card shows, so a conflict line and the header cannot disagree.
fn module_label(module: &'static str) -> &'static str {
    copy::all_cards()
        .find(|card| card_module(card) == Some(module))
        .and_then(header_label)
        .unwrap_or(module)
}

/// The name an action bind's row shows (`Auto Rod`, `Throwpot`), read from its card.
fn action_bind_name(which: copy::ActionBind) -> &'static str {
    copy::all_cards()
        .find(|card| {
            card.controls.iter().any(
                |control| matches!(control, Control::ActionBind { which: held } if *held == which),
            )
        })
        .and_then(header_label)
        .unwrap_or(which.id())
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
/// pending-capture branch is [`copy::ACTION_BIND_PENDING`], which the capture reads while it
/// listens.
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

/// A module keybind button's rest label for a whole profile.
///
/// The XAML button has no `Content` (`MainWindow.xaml:359`): every label comes from
/// `SetKeybindButtonContent` (`MainWindow.xaml.cs:2143-2156`), which composes
/// ``Bind: {FormatVirtualKey(InputHooks.GetModuleKey(module))}`` from the module's
/// virtual key, or `Unbound` when there is none — so an untouched profile reads
/// `Bind: Unbound`. See [`crate::views::keybind_text`] for the format and the one
/// branch (`Key` enum names) that is not ported.
///
/// [`Shell::ensure_capture`] builds a capture's label with this and the tests pin the same
/// function, so what a button paints and what a test expects cannot drift apart.
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

/// The search key one keystroke is, or `None` when the search does not care about it.
///
/// Ctrl+K and Escape are the managed chord and the managed dismissal. A printable character is
/// `key_char`, which is what the platform delivers for typing and is `None` for a modifier or a
/// named key; a chord with a character behind it (`Ctrl+A`) is a text-field gesture, not a
/// type-to-open, so any of ctrl/alt/platform disqualifies it. Shift does not, because `key_char`
/// is already the shifted character.
fn search_key_of(event: &KeyDownEvent) -> Option<SearchKey> {
    let modifiers = event.keystroke.modifiers;
    let chorded = modifiers.control || modifiers.alt || modifiers.platform;

    if modifiers.control
        && !modifiers.alt
        && !modifiers.platform
        && event.keystroke.key.eq_ignore_ascii_case("k")
    {
        return Some(SearchKey::CtrlK);
    }

    if event.keystroke.key.eq_ignore_ascii_case("escape") {
        return Some(SearchKey::Escape);
    }

    if chorded {
        return None;
    }

    // `key_char` is the typed character; `key` is the fallback, because a platform reports some
    // keys only there and `Keystroke::parse` (which the tests use) puts the character in `key`
    // and leaves `key_char` empty. A named key ("tab", "f1") is longer than one character and
    // falls out below.
    let character = event
        .keystroke
        .key_char
        .as_deref()
        .unwrap_or(event.keystroke.key.as_str());
    let mut characters = character.chars();
    let first = characters.next()?;
    if characters.next().is_some() {
        return None;
    }

    Some(SearchKey::Printable(first))
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

    /// The Stats row is the one `render` mounts on `views::stats_view`, and the numbers that
    /// view composes are the runtime's — before a reset and after it.
    ///
    /// Painted glyphs are not readable at the pin: GPUI exposes no text readback, which is why
    /// `stats_view`'s own suite pins `StatsTexts::of` against the tracker and geometry through
    /// a mounted window. This test does the same two halves one level up — the shell's row
    /// selects the view, and `StatsTexts` fed by the shell's runtime carries the clicked count
    /// — so a wrong number fails without a window and a wrong row fails without a render.
    ///
    /// The clicks are recorded through `Runtime::click_sink`, the object the loop bodies hold,
    /// so this is the production path and not a test-only setter.
    #[test]
    fn the_stats_tab_is_the_row_this_mounts() {
        use crate::views::stats_view::StatsTexts;

        let stats_tab = copy::TABS
            .iter()
            .position(|tab| tab.name == "Stats")
            .expect("a Stats tab");
        // The variant itself, because that is what the render match dispatches on: keying the
        // old mount on the `STATS` slice's address made the arm a guard, and a guard is what
        // let the tab stay a static copy table.
        assert_eq!(copy::TABS[stats_tab].body, TabBody::Stats);
        assert!(
            matches!(copy::TABS[0].body, TabBody::Cards(_)),
            "Combat keeps its cards"
        );
        assert_eq!(
            copy::TABS
                .iter()
                .filter(|tab| tab.body == TabBody::Stats)
                .count(),
            1,
            "exactly one row is the Stats view"
        );

        let shell = Shell::new(Theme::slate());
        let sink = shell.runtime.click_sink();
        sink.record_click(8.5, true);
        sink.record_click(11.0, false);

        let texts = StatsTexts::of(&shell.runtime.stats());
        assert_eq!(texts.clicks, "2", "the tab draws the runtime's count");
        assert_eq!(texts.clicks_split, "L 1  R 1");

        assert!(
            shell.runtime.reset_stats(),
            "two clicks were there to clear"
        );

        let texts = StatsTexts::of(&shell.runtime.stats());
        assert_eq!(texts.clicks, "0", "the tab draws the cleared count");
        assert_eq!(texts.samples, "0 samples");
    }

    #[test]
    fn card_headers_are_the_managed_shapes_and_never_an_identifier() {
        let cards = match copy::TABS[0].body {
            TabBody::Cards(cards) => cards,
            TabBody::Stats | TabBody::Accounts | TabBody::Servers => &[],
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

    /// A profile write reaches the controller, which is the copy the `config` frame is built
    /// from.
    ///
    /// Two writers with no push between them is the defect this migration keeps finding: the
    /// UI would show the new value, the saved file would carry it, and the bridge would keep
    /// running the old one for the life of the link. `FightStatusEnabled` is one of the two
    /// fields the config slice names, and it stands in for every bind path that shares
    /// `apply_control`.
    #[test]
    fn a_profile_write_reaches_the_controller_the_config_frame_reads() {
        use crate::views::bindings::ControlValue;

        let mut shell = Shell::new(Theme::slate());
        assert!(!shell.profile().fight_status_enabled);
        assert!(
            !shell.controller().config_payload().fight_status,
            "the frame the link would send starts on the profile's default"
        );

        assert!(
            shell.apply_control("FightStatusEnabled", ControlValue::Toggle(true)),
            "the switch's bind path has a field behind it"
        );

        assert!(shell.profile().fight_status_enabled, "the UI copy took it");
        assert!(
            shell.controller().profile().fight_status_enabled,
            "and the controller's copy, which `config_payload` fills `fightStatus` from"
        );
        assert!(
            shell.controller().config_payload().fight_status,
            "so the frame the reader sends carries it"
        );
        assert!(
            !shell.controller().store().is_connected(),
            "with no reconnect: the payload moved on the controller the live link already reads"
        );
    }

    /// Every bind the catalogue declares writes the controller's copy too.
    ///
    /// Push-on-change is the design, so its failure mode is a write site that forgets the push:
    /// the value reaches the card and the saved file and not the wire, silently. This is the
    /// net for that. It drives every bind in [`crate::views::bindings::all_binds`] through the
    /// same `apply_control` a click goes through and asserts the copies agree after each one,
    /// so a bind added to the catalogue later is covered by existing rather than by memory.
    #[test]
    fn every_catalog_bind_leaves_the_controllers_copy_equal() {
        use crate::views::bindings::{self, ControlValue};

        let mut shell = Shell::new(Theme::slate());
        let binds = bindings::all_binds();
        assert!(
            binds.len() > 80,
            "the catalogue is the net, so it has to be wide: {} binds",
            binds.len()
        );

        let mut written = 0;
        for bind in binds {
            let Some(current) = bindings::snapshot(shell.profile()).get(bind).cloned() else {
                continue;
            };
            // A different value of the same kind, so a field that clamps or refuses still
            // exercises its path; whether it accepted is not what is asserted here.
            let changed = match current {
                ControlValue::Toggle(on) => ControlValue::Toggle(!on),
                ControlValue::Number(number) => ControlValue::Number(number + 1.0),
                ControlValue::Index(index) => ControlValue::Index(index + 1),
                ControlValue::Text(text) => ControlValue::Text(format!("{text}x")),
            };
            let before = shell.profile().clone();
            shell.apply_control(bind, changed);
            assert_eq!(
                *shell.profile(),
                shell.controller().profile(),
                "`{bind}` wrote the shell's copy and not the controller's"
            );
            if *shell.profile() != before {
                written += 1;
            }
        }
        assert!(
            written > 20,
            "the sweep has to write, not skip: {written} binds changed"
        );
    }

    /// A captured key survives the Settings card's save and load, on both copies.
    ///
    /// The capture test below proves the key reaches the profile, the button and the saved
    /// slot. This is the other direction: a load has to put it back, and the controller is the
    /// copy the `config` frame reads, so a load that only fixed the UI would leave the bridge
    /// running the previous key until the next restart.
    #[gpui::test]
    fn a_captured_key_survives_a_save_and_a_load(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);
        let target = KeyTarget::Module("autoclicker");

        press_autoclicker_keybind(&mut visual);
        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("f1").expect("keystroke"));
        cx.update(|cx| {
            shell.update(cx, |shell, _cx| {
                shell.save_config("RoundTrip").expect("named save")
            });
        });

        // A second key, so the load has something to undo rather than nothing to do. The click
        // path is proven above; what this test is about is the load, so the write can go
        // straight to the profile's writer.
        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_captured_key(KeyTarget::Module("autoclicker"), 0x71, cx)
            });
        });
        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .profile()
                .module_keys
                .get("autoclicker")
                .copied()),
            Some(0x71),
            "the second key took"
        );

        cx.update(|cx| {
            shell.update(cx, |shell, _cx| {
                shell.load_config("RoundTrip").expect("named load")
            });
        });

        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .profile()
                .module_keys
                .get("autoclicker")
                .copied()),
            Some(0x70),
            "the load put the saved key back"
        );
        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .controller()
                .profile()
                .module_keys
                .get("autoclicker")
                .copied()),
            Some(0x70),
            "and the controller's copy, which the config frame reads"
        );
        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .controller()
                .config_payload()
                .keybind_autoclicker),
            0x70,
            "so the frame carries the saved key after a load, with no reconnect"
        );

        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(
            capture_label(&shell, cx, target),
            "Bind: F1",
            "the button follows the profile the load replaced"
        );
    }

    /// Loading runs the load-time half of the action-bind rule: a module bind wins over a
    /// dedicated action key that duplicates it, and a distinct key is left alone.
    ///
    /// `Profile::normalize_action_binds` had no caller in the app, so the rule only existed on
    /// the managed side and in its own tests. Both load paths call it now, and the assertion
    /// that matters is the controller's copy: a cleared key that stayed in the controller would
    /// keep the duplicated bind on the wire.
    #[test]
    fn loading_clears_a_duplicated_action_bind_and_keeps_a_distinct_one() {
        // The startup path: the saved slot is read into `with_loaded_profile`.
        let mut colliding = aoko_protocol::profile::Profile::default();
        colliding.module_keys.insert("autoclicker".to_owned(), 0x70);
        colliding.auto_rod_action_key = 0x70;
        let shell = Shell::with_profile(Theme::slate(), colliding);
        assert_eq!(
            shell.profile().auto_rod_action_key,
            0,
            "the module bind wins, as `Profile.cs:889-899` makes it win on apply"
        );
        assert_eq!(
            shell.profile().module_keys.get("autoclicker").copied(),
            Some(0x70),
            "and the general bind is kept"
        );
        assert_eq!(
            *shell.profile(),
            shell.controller().profile(),
            "the controller got the normalised copy, not the file's"
        );

        // The Settings card path: a distinct action bind is not a collision, so it survives.
        let root = scratch_root();
        let store = aoko_platform::ProfileStore::new(&root);
        let mut distinct = aoko_protocol::profile::Profile::default();
        distinct.module_keys.insert("autoclicker".to_owned(), 0x70);
        distinct.auto_rod_action_key = 0x71;
        store
            .write_profile(
                "Distinct",
                &aoko_protocol::profile::to_profile_json(&distinct),
            )
            .expect("seed");

        let mut shell = Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root));
        shell.load_config("Distinct").expect("named load");
        assert_eq!(
            shell.profile().auto_rod_action_key,
            0x71,
            "a distinct action bind survives a load unchanged"
        );
        assert_eq!(*shell.profile(), shell.controller().profile());
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
        // A real palette name: an unknown one resolves to Slate
        // (`palette_store::resolve_in`), which is why this one has to be a built-in.
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

    /// A switch click has to be visible on the controller, not only on a field the view
    /// happens to own.
    ///
    /// The shell used to keep its own `ModuleToggles`, so a test could only ask the view
    /// what it had drawn. Now the state lives in the `Arc<AppController>` inside
    /// [`Runtime`], and this holds a second handle to it — the same handle a transport
    /// owner or a click loop would take — and reads the flip without going through
    /// `Shell` at all.
    #[gpui::test]
    fn a_switch_click_is_observable_through_the_controller(cx: &mut gpui::TestAppContext) {
        use aoko_protocol::module_catalog::EnabledFlag;
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

        let controller = cx.read(|cx| Arc::clone(shell.read(cx).controller()));
        assert!(!controller.is_module_enabled(EnabledFlag::LeftClick));

        let bounds = visual
            .debug_bounds("switch-IsArmed")
            .expect("the switch renders with its debug selector");
        visual.simulate_click(bounds.center(), Modifiers::default());

        assert!(
            controller.is_module_enabled(EnabledFlag::LeftClick),
            "the click reached AppController::set_module_enabled"
        );
        assert!(
            !controller.is_module_enabled(EnabledFlag::AutoTotem),
            "the AutoClicker switch flips its own flag and no other"
        );

        let bounds = visual
            .debug_bounds("switch-IsArmed")
            .expect("the switch is still there");
        visual.simulate_click(bounds.center(), Modifiers::default());
        assert!(
            !controller.is_module_enabled(EnabledFlag::LeftClick),
            "a second click flips it back through the same owner"
        );
    }

    /// The suite must not depend on the machine it runs on.
    ///
    /// A shell built for a test owns a controller that probed nothing, and its capability
    /// report says so: `AppController::default` is built over
    /// `CapabilityReport::transport_only(SessionKind::None)`, whereas a probe on this
    /// machine would name X11 or Wayland. If the probe ever moves back into
    /// [`Shell::start`], this test fails on a developer box and not only in CI.
    #[gpui::test]
    fn a_test_shell_owns_a_controller_that_probed_nothing(cx: &mut gpui::TestAppContext) {
        use aoko_platform::SessionKind;

        let shell = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        let session = cx.read(|cx| shell.read(cx).controller().capabilities().session);
        let open_path = cx.read(|cx| shell.read(cx).controller().capabilities().open_path);

        assert_eq!(session, SessionKind::None);
        assert!(!open_path);
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

    // ---- keybind capture -------------------------------------------------------------

    /// The label a capture button paints, read from the entity the button draws.
    ///
    /// GPUI exposes no text readback, so this is the string the button would paint — the same
    /// choice `StatsTexts` makes for the Stats view. A missing entity is a failure, not a
    /// skip: it means the render built no button for that binding.
    fn capture_label(
        shell: &Entity<Shell>,
        cx: &mut gpui::TestAppContext,
        target: KeyTarget,
    ) -> String {
        cx.read(|cx| {
            shell
                .read(cx)
                .capture_entity(target)
                .expect("the render built a capture for this binding")
                .read(cx)
                .label()
                .to_string()
        })
    }

    /// A shell with a store under `root`, on a window, painted once.
    ///
    /// The three capture tests need the same four things: a store so a save can be read back,
    /// an entity so the app can be read from a test, a window so a click and a keystroke have
    /// somewhere to land, and one paint so the buttons exist.
    fn shell_in_window(
        cx: &mut gpui::TestAppContext,
        root: &std::path::Path,
    ) -> (
        Entity<Shell>,
        gpui::AnyWindowHandle,
        gpui::VisualTestContext,
    ) {
        use gpui::WindowOptions;

        let shell = cx.update(|cx| {
            let mut shell =
                Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(root));
            // The palette file lives in the scratch root too, so a test that ever saves a draft
            // cannot write into this machine's palette directory.
            shell.set_palette_path(root.join("palettes.json"));
            cx.new(|_| shell)
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| shell.clone())
                .expect("test window")
        });
        let handle: gpui::AnyWindowHandle = window.into();
        let mut visual = gpui::VisualTestContext::from_window(handle, cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        (shell, handle, visual)
    }

    /// Presses the AutoClicker card's keybind button, which is what starts a capture.
    fn press_autoclicker_keybind(visual: &mut gpui::VisualTestContext) {
        use gpui::Modifiers;

        let bounds = visual
            .debug_bounds("keybind-AutoClickerCard")
            .expect("the header's keybind button is painted");
        visual.simulate_click(bounds.center(), Modifiers::default());
    }

    /// A key captured on a keybind button lands in the profile, on the button, and on disk.
    ///
    /// The press and the keystroke are the real ones — a click on the button the tree painted,
    /// then `dispatch_keystroke` to the window the capture focused — so nothing here calls
    /// `apply_captured_key` directly. The label is read after the repaint the write schedules,
    /// because that repaint is what rebuilds the button.
    #[gpui::test]
    fn a_captured_key_reaches_the_profile_the_button_and_the_saved_file(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);
        let target = KeyTarget::Module("autoclicker");

        assert_eq!(
            capture_label(&shell, cx, target),
            "Bind: Unbound",
            "an untouched profile binds nothing"
        );

        press_autoclicker_keybind(&mut visual);
        let keystroke = Keystroke::parse("f1").expect("keystroke");
        visual.cx.dispatch_keystroke(handle, keystroke);

        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .profile()
                .module_keys
                .get("autoclicker")
                .copied()),
            Some(0x70),
            "the capture wrote the module key through the profile"
        );
        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .controller()
                .profile()
                .module_keys
                .get("autoclicker")
                .copied()),
            Some(0x70),
            "and the controller's copy, which the config frame fills `keybindAutoclicker` from"
        );
        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .controller()
                .config_payload()
                .keybind_autoclicker),
            0x70,
            "and the frame itself, so a captured key reaches the wire with no reconnect"
        );

        // The paint the write scheduled is what rebuilds the button.
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(
            capture_label(&shell, cx, target),
            "Bind: F1",
            "the button follows the profile it just wrote"
        );

        cx.update(|cx| {
            shell.update(cx, |shell, _cx| shell.save_profile().expect("save on exit"));
        });

        // A second shell over the same directory is the next launch.
        let reloaded = Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root));
        assert_eq!(
            reloaded.profile().module_keys.get("autoclicker").copied(),
            Some(0x70),
            "a reload of the saved slot carries the captured key"
        );
    }

    /// Escape is the managed unbind, not a cancel: it writes `0` and the button reads Unbound.
    ///
    /// The profile starts bound at F7, so the assertion can tell "wrote 0" from "never wrote":
    /// an absent entry and a stored zero both read Unbound, and only the stored zero proves the
    /// escape was persisted.
    #[gpui::test]
    fn escape_writes_the_managed_unbind(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let mut profile = aoko_protocol::profile::Profile::default();
        profile
            .module_keys
            .insert(String::from("autoclicker"), 0x76);
        let shell = cx.update(|cx| cx.new(|_| Shell::with_profile(Theme::slate(), profile)));
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, _| shell.clone())
                .expect("test window")
        });
        let handle: gpui::AnyWindowHandle = window.into();
        let mut visual = gpui::VisualTestContext::from_window(handle, cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let target = KeyTarget::Module("autoclicker");
        assert_eq!(capture_label(&shell, cx, target), "Bind: F7");

        press_autoclicker_keybind(&mut visual);
        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("escape").expect("keystroke"));

        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .profile()
                .module_keys
                .get("autoclicker")
                .copied()),
            Some(0),
            "escape stores the managed `finalVk = 0` rather than leaving the key alone"
        );

        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(
            capture_label(&shell, cx, target),
            "Bind: Unbound",
            "a stored zero is the unbound form"
        );
    }

    /// A capture that loses focus writes nothing at all.
    ///
    /// `Captured::Abandoned` carries no virtual key, so the handler returns before it touches
    /// the profile — the bound key and the label are exactly what they were.
    #[gpui::test]
    fn losing_focus_leaves_the_profile_alone(cx: &mut gpui::TestAppContext) {
        let mut profile = aoko_protocol::profile::Profile::default();
        profile
            .module_keys
            .insert(String::from("autoclicker"), 0x76);
        let shell = cx.update(|cx| cx.new(|_| Shell::with_profile(Theme::slate(), profile)));

        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, _| shell.clone())
                .expect("test window")
        });
        let handle: gpui::AnyWindowHandle = window.into();
        let mut visual = gpui::VisualTestContext::from_window(handle, cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let target = KeyTarget::Module("autoclicker");
        // A stale refusal line is showing before the capture starts; abandoning it takes the
        // line away, so the area reads its own state rather than an outcome that no longer
        // applies.
        cx.update(|cx| {
            shell.update(cx, |shell, _cx| {
                shell.keybind_conflict = Some(SharedString::from("stale"));
            })
        });
        press_autoclicker_keybind(&mut visual);
        // The press moves focus, and GPUI dispatches focus listeners inside `Window::draw`
        // rather than in `focus`/`blur` themselves (`window.rs:3938-3993`), which is the same
        // requirement the component's own blur test records. So the focus move needs a frame
        // before the blur can end the capture, or the abandonment is never delivered and the
        // button stays on its pending label.
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let capturing = cx.read(|cx| {
            shell
                .read(cx)
                .capture_entity(target)
                .expect("a capture for the header")
                .read(cx)
                .is_capturing()
        });
        assert!(capturing, "the press started a capture");

        visual.update(|window, cx| window.blur(cx));

        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .profile()
                .module_keys
                .get("autoclicker")
                .copied()),
            Some(0x76),
            "focus loss is not a key: the bound key is untouched"
        );

        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(
            capture_label(&shell, cx, target),
            "Bind: F7",
            "and the button still shows the key the profile holds"
        );
        assert!(
            cx.read(|cx| shell.read(cx).keybind_conflict.is_none()),
            "focus loss also drops the line the last refusal wrote"
        );
    }

    /// A module bind refuses a key the action binds hold, and the line names both bindings.
    ///
    /// `InputHooks.SetModuleKey` (`InputHooks.cs:124-134`) refuses it. Where managed reports it
    /// is the wart: it writes the reason into the Auto Rod row's status line
    /// (`MainWindow.xaml.cs:2071`), which is a control the conflict is not about. Here the line
    /// belongs to the keybind area and says which two bindings collided.
    #[gpui::test]
    fn a_module_bind_refuses_a_key_an_action_bind_holds(cx: &mut gpui::TestAppContext) {
        let mut profile = aoko_protocol::profile::Profile::default();
        profile.auto_rod_action_key = 0x70; // F1
        let shell = cx.update(|cx| cx.new(|_| Shell::with_profile(Theme::slate(), profile)));

        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_captured_key(KeyTarget::Module("autoclicker"), 0x70, cx)
            });
        });

        let (module_key, line, row) = cx.read(|cx| {
            let shell = shell.read(cx);
            (
                shell.module_key("autoclicker"),
                shell.keybind_conflict.clone(),
                shell.action_status[status_slot(copy::ActionBind::AutoRod)].clone(),
            )
        });
        assert_eq!(module_key, 0, "the refused key is not stored");
        assert!(
            row.is_none(),
            "and the Auto Rod row says nothing: the conflict is not its own"
        );
        let line = line.expect("the refusal left a line").to_string();
        assert!(line.contains("F1"), "the line names the key: {line}");
        assert!(
            line.contains("Auto Rod"),
            "and the action bind that holds it: {line}"
        );
        assert!(
            line.contains("AutoClicker"),
            "and the module that asked for it: {line}"
        );
    }

    /// An action bind refuses a key a module holds, in the managed words, on its own row.
    #[gpui::test]
    fn an_action_bind_refuses_a_key_a_module_holds(cx: &mut gpui::TestAppContext) {
        let mut profile = aoko_protocol::profile::Profile::default();
        profile
            .module_keys
            .insert(String::from("autoclicker"), 0x70);
        let shell = cx.update(|cx| cx.new(|_| Shell::with_profile(Theme::slate(), profile)));

        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_captured_key(KeyTarget::Action(copy::ActionBind::Throwpot), 0x70, cx)
            });
        });

        let (throwpot, status, conflict) = cx.read(|cx| {
            let shell = shell.read(cx);
            (
                shell.profile().throwpot_action_key,
                shell.action_status[status_slot(copy::ActionBind::Throwpot)].clone(),
                shell.keybind_conflict.clone(),
            )
        });
        assert_eq!(throwpot, 0, "the refused key is not stored");
        assert_eq!(
            status.expect("the row said why").to_string(),
            copy::ACTION_BIND_CONFLICT,
            "the managed line, under the button it is about"
        );
        assert!(
            conflict.is_none(),
            "an action row's refusal is not the keybind area's line"
        );
    }

    /// A free key lands on an action row, which says so; Escape still unbinds.
    ///
    /// The managed accepted line is the row's, and the unbind is the case the refusal must
    /// never catch: `0` is Escape, which `InputHooks` lets through for both bind kinds.
    #[gpui::test]
    fn an_accepted_action_bind_says_so_and_escape_still_unbinds(cx: &mut gpui::TestAppContext) {
        let shell = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        let target = KeyTarget::Action(copy::ActionBind::AutoRod);

        cx.update(|cx| {
            shell.update(cx, |shell, cx| shell.apply_captured_key(target, 0x71, cx));
        });
        let (key, status, controller_key) = cx.read(|cx| {
            let shell = shell.read(cx);
            (
                shell.profile().auto_rod_action_key,
                shell.action_status[status_slot(copy::ActionBind::AutoRod)].clone(),
                shell.controller().profile().auto_rod_action_key,
            )
        });
        assert_eq!(key, 0x71, "the free key landed");
        assert_eq!(
            status.expect("the row said so").to_string(),
            copy::ACTION_BIND_UPDATED
        );
        assert_eq!(
            controller_key, 0x71,
            "and reached the copy the config frame is built from"
        );
        assert_eq!(
            cx.read(|cx| action_bind_label(shell.read(cx).profile(), copy::ActionBind::AutoRod)),
            "Action: F2",
            "the label follows the profile, which is the path it already read"
        );

        cx.update(|cx| {
            shell.update(cx, |shell, cx| shell.apply_captured_key(target, 0, cx));
        });
        assert_eq!(
            cx.read(|cx| shell.read(cx).profile().auto_rod_action_key),
            0,
            "Escape is a value the refusal does not catch"
        );
        assert_eq!(
            cx.read(|cx| action_bind_label(shell.read(cx).profile(), copy::ActionBind::AutoRod)),
            "Action: Unbound",
            "and the label reads the unbound form"
        );
    }

    /// Both status lines are painted, on the bindings they are about.
    #[gpui::test]
    fn the_refusal_lines_paint_on_the_bindings_they_are_about(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (shell, _handle, mut visual) = shell_in_window(cx, &root);

        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.profile.auto_rod_action_key = 0x70;
                shell.apply_captured_key(KeyTarget::Module("autoclicker"), 0x70, cx);
            });
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let area = visual
            .debug_bounds("keybind-conflict")
            .expect("the keybind area paints the refusal line");
        assert!(
            f32::from(area.size.width) > 0.0,
            "the line has width to paint"
        );

        // The Auto Rod row is on its own tab, and its line paints under its own button.
        let tab = test_tab_of("AutoRodCard");
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named(tab, cx)));
        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .profile
                    .module_keys
                    .insert(String::from("autoclicker"), 0x71);
                shell.apply_captured_key(KeyTarget::Action(copy::ActionBind::AutoRod), 0x71, cx);
            });
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        scroll_into_view(&mut visual, "action-bind-AutoRod");
        let row = visual
            .debug_bounds("action-status-AutoRod")
            .expect("the row paints its own outcome");
        assert!(
            f32::from(row.size.height) > 0.0,
            "the line has height to paint"
        );
    }

    /// Every keybind and action-bind button is a capture, and the action binds write their
    /// own fields.
    ///
    /// The count is the managed one: 38 `Control::Keybind` rows and the two action binds. The
    /// write goes through the same entry point the widget's callback uses, so a target that
    /// named the wrong field would fail here rather than in a screenshot.
    #[gpui::test]
    fn every_binding_gets_a_capture_and_the_actions_write_their_own_fields(
        cx: &mut gpui::TestAppContext,
    ) {
        let modules = key_targets()
            .iter()
            .filter(|target| matches!(target, KeyTarget::Module(_)))
            .count();
        assert_eq!(modules, 38, "one per `Control::Keybind` row in the table");
        assert_eq!(key_targets().len(), 40, "and the two action binds");

        let root = scratch_root();
        let (shell, _handle, mut visual) = shell_in_window(cx, &root);
        for &target in key_targets() {
            assert!(
                cx.read(|cx| shell.read(cx).capture_entity(target).is_some()),
                "the first paint built no capture for {target:?}"
            );
        }

        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_captured_key(KeyTarget::Action(copy::ActionBind::Throwpot), 0x70, cx);
            });
        });

        assert_eq!(
            cx.read(|cx| shell.read(cx).profile().throwpot_action_key),
            0x70,
            "the throwpot bind writes its own field"
        );
        assert_eq!(
            cx.read(|cx| shell.read(cx).profile().auto_rod_action_key),
            0,
            "and leaves the other action alone"
        );
        // The profile carries every module id at `0` by default (`default_module_keys`), so
        // the assertion is that no key holds the action's key, not that the map has no
        // entry: `autoclicker` is present and still unbound.
        assert!(
            cx.read(|cx| shell
                .read(cx)
                .profile()
                .module_keys
                .values()
                .all(|key| *key != 0x70)),
            "an action key never lands in `module_keys`"
        );

        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(
            capture_label(&shell, cx, KeyTarget::Action(copy::ActionBind::Throwpot)),
            "Action: F1",
            "the action button follows the field it wrote"
        );
    }

    /// The tab whose card list holds `card_id`, for the switch tests.
    fn test_tab_of(card_id: &str) -> &'static str {
        copy::TABS
            .iter()
            .find(|tab| match tab.body {
                TabBody::Cards(cards) => cards.iter().any(|card| card.id == card_id),
                TabBody::Stats | TabBody::Accounts | TabBody::Servers => false,
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

    // ---- the command-search overlay -------------------------------------------

    /// The state the overlay paints, as a tuple a test can compare.
    ///
    /// The four fields decide what is on screen: whether it is open, the query, the selection
    /// and the row count. Two paths that open the same overlay have to agree on all of them.
    fn search_state(shell: &Shell) -> (bool, String, Option<usize>, usize) {
        let search = shell.search();
        (
            search.is_open(),
            search.query().to_owned(),
            search.selected_index(),
            search.rows().len(),
        )
    }

    /// A row click dismisses the card and lands on the tab the row names.
    ///
    /// The click goes through [`Shell::jump_to_row`], which is the function
    /// `crate::overlay::overlay`'s row callback calls. The overlay's rows carry no debug
    /// selector, so a painted click is not addressable at the pin and the model is what is
    /// asserted. The row is chosen because its tab is **not** the current one: a jump that
    /// changed nothing would otherwise pass.
    #[gpui::test]
    fn a_row_click_dismisses_the_card_and_lands_on_its_tab(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("ctrl-k").expect("keystroke"));

        let (open, current, index, tab_of_row) = cx.read(|cx| {
            let shell = shell.read(cx);
            let current = shell.selected_tab_name();
            let index = shell
                .search()
                .rows()
                .iter()
                .position(|row| !row.entry.tab.eq_ignore_ascii_case(current))
                .expect("an empty query ranks destinations outside the tab the shell opens on");
            (
                shell.search().is_open(),
                current,
                index,
                shell.search().rows()[index].entry.tab,
            )
        });
        assert!(open, "Ctrl+K opened the overlay");

        cx.update(|cx| shell.update(cx, |shell, cx| shell.jump_to_row(index, cx)));

        let (open, tab) = cx.read(|cx| {
            let shell = shell.read(cx);
            (shell.search().is_open(), shell.selected_tab_name())
        });
        assert!(!open, "the jump dismissed the card");
        assert_eq!(tab, tab_of_row, "and landed on the tab the row names");
        assert_ne!(tab, current, "which is not the tab the shell started on");
    }

    /// Enter dismisses the card and lands where the selected row points.
    ///
    /// A typed character is the managed type-to-open gesture, so this also covers the seed.
    /// The selection is moved onto a row whose tab is not the current one — the model call an
    /// arrow key lands on — so the landing tab proves the jump rather than coinciding with
    /// where the shell already was.
    #[gpui::test]
    fn enter_dismisses_the_card_and_lands_on_the_selected_rows_tab(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("s").expect("keystroke"));

        let (open, query, current, index, tab_of_row) = cx.read(|cx| {
            let shell = shell.read(cx);
            let current = shell.selected_tab_name();
            let index = shell
                .search()
                .rows()
                .iter()
                .position(|row| !row.entry.tab.eq_ignore_ascii_case(current))
                .expect("`s` ranks a destination outside the tab the shell opens on");
            (
                shell.search().is_open(),
                shell.search().query().to_owned(),
                current,
                index,
                shell.search().rows()[index].entry.tab,
            )
        });
        assert!(open, "a typed character opens the overlay");
        assert_eq!(query, "s", "seeded with the character that opened it");

        cx.update(|cx| shell.update(cx, |shell, _| shell.search.select(Some(index))));

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("enter").expect("keystroke"));

        let (open, tab) = cx.read(|cx| {
            let shell = shell.read(cx);
            (shell.search().is_open(), shell.selected_tab_name())
        });
        assert!(!open, "Enter dismissed the card");
        assert_eq!(tab, tab_of_row, "and landed on the selected row's tab");
        assert_ne!(tab, current, "which is not the tab the shell started on");
    }

    /// A focused text field owns the keyboard: Ctrl+K and a typed character both belong to it.
    ///
    /// This condition is the one the managed `CanOpenCommandSearch` never needed. WPF sent
    /// window keys to the window, so a field could not swallow a chord; GPUI bubbles a typed
    /// character from the focused field to the root listener, so without the field check a key
    /// typed into the token box would open the overlay as well as type.
    ///
    /// The gate is asserted directly as well as through the keystrokes, because a widget that
    /// stopped propagation would make the keystroke half pass without the gate ever being
    /// consulted.
    #[gpui::test]
    fn a_focused_text_field_keeps_ctrl_k_and_the_character(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);

        visual.update(|window, cx| {
            assert!(
                shell
                    .read(cx)
                    .search_gate(Some(window), cx)
                    .can_open_search(),
                "nothing has focus yet, so the gate is open"
            );
        });

        let field = cx
            .read(|cx| shell.read(cx).fields.token.clone())
            .expect("the token box exists after the first frame");
        let focus = cx.read(|cx| field.read(cx).focus_handle().clone());
        visual.update(|window, cx| window.focus(&focus, cx));

        visual.update(|window, cx| {
            assert!(
                !shell
                    .read(cx)
                    .search_gate(Some(window), cx)
                    .can_open_search(),
                "a focused text field closes the gate"
            );
        });

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("ctrl-k").expect("keystroke"));
        assert!(
            !cx.read(|cx| shell.read(cx).search().is_open()),
            "Ctrl+K belongs to the field"
        );

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("a").expect("keystroke"));
        assert!(
            !cx.read(|cx| shell.read(cx).search().is_open()),
            "and a typed character belongs to the field"
        );
        assert_eq!(
            cx.read(|cx| field.read(cx).text().to_owned()),
            "a",
            "the field is what received it"
        );
    }

    /// The rail button opens exactly the state Ctrl+K opens.
    ///
    /// `components::button` gives the rail button an `ElementId` but installs no
    /// `debug_selector`, so a painted click is not addressable from a `VisualTestContext`;
    /// this asserts the state, and the wiring is the single `open_search(None, cx)` call in
    /// `render_rail`.
    #[gpui::test]
    fn the_rail_button_opens_the_state_ctrl_k_opens(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("ctrl-k").expect("keystroke"));
        let by_key = cx.read(|cx| search_state(shell.read(cx)));
        assert!(by_key.0, "Ctrl+K opened the overlay");

        cx.update(|cx| shell.update(cx, |shell, cx| shell.close_search(cx)));
        assert!(
            !cx.read(|cx| shell.read(cx).search().is_open()),
            "and the dismissal closed it, so the second open starts from closed"
        );

        cx.update(|cx| shell.update(cx, |shell, cx| shell.open_search(None, cx)));
        let by_button = cx.read(|cx| search_state(shell.read(cx)));

        assert!(by_button.0, "the rail button's call opened the overlay");
        assert_eq!(
            by_button, by_key,
            "the same open, query, selection and row count"
        );
    }

    /// The overlay is mounted: its card paints while it is open and not while it is closed.
    ///
    /// `command-search-card` is the one stable selector the overlay carries, and the mount
    /// sits above the rail and the body in the tree, so this is the assertion that the view
    /// is in the frame at all rather than only in the state.
    #[gpui::test]
    fn the_overlay_paints_only_while_it_is_open(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);

        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(
            visual.debug_bounds("command-search-card").is_none(),
            "a closed overlay paints nothing"
        );

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("ctrl-k").expect("keystroke"));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(
            visual.debug_bounds("command-search-card").is_some(),
            "an open overlay paints its card"
        );

        cx.update(|cx| shell.update(cx, |shell, cx| shell.close_search(cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(
            visual.debug_bounds("command-search-card").is_none(),
            "and closing it takes the card off screen"
        );
    }

    // ---- the Block ESP add row -------------------------------------------------

    /// Focuses the Block ESP add box, so a keystroke lands in it.
    ///
    /// The box lives on the shell rather than in `FormFields`, so a test reads it the same way
    /// [`Shell::add_block_target`] does: off the entity.
    fn focus_block_add(
        cx: &mut gpui::TestAppContext,
        shell: &Entity<Shell>,
        visual: &mut gpui::VisualTestContext,
    ) -> Entity<TextInput> {
        let field = cx
            .read(|cx| shell.read(cx).block_add.clone())
            .expect("the add box exists after the first frame");
        let focus = cx.read(|cx| field.read(cx).focus_handle().clone());
        visual.update(|window, cx| window.focus(&focus, cx));
        field
    }

    /// Puts `text` in the box the owner's way and presses Enter, which is the submit
    /// `components::TextInput` deliberately refuses to own (`text_input.rs:421`).
    fn submit_block_add(
        cx: &mut gpui::TestAppContext,
        shell: &Entity<Shell>,
        visual: &mut gpui::VisualTestContext,
        window: gpui::AnyWindowHandle,
        text: &str,
    ) {
        use gpui::Keystroke;

        let field = focus_block_add(cx, shell, visual);
        cx.update(|cx| field.update(cx, |field, cx| field.set_text(text, cx)));
        visual
            .cx
            .dispatch_keystroke(window, Keystroke::parse("enter").expect("keystroke"));
    }

    /// Enter in the add box appends one target, and typing in that box is not the search's.
    ///
    /// The two behaviours are one keystroke apart, so one walk covers both: the characters go to
    /// the field — the overlay has to stay shut while a text field holds focus, which is what
    /// adding this box to `any_text_field_focused` buys — and Enter goes to the shell because
    /// the widget leaves it to its owner.
    #[gpui::test]
    fn typing_into_the_block_add_box_then_enter_adds_the_block(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);
        let field = focus_block_add(cx, &shell, &mut visual);

        for character in ["s", "t", "o", "n", "e"] {
            visual
                .cx
                .dispatch_keystroke(handle, Keystroke::parse(character).expect("keystroke"));
        }

        assert_eq!(
            cx.read(|cx| field.read(cx).text().to_owned()),
            "stone",
            "the box received the characters"
        );
        assert!(
            !cx.read(|cx| shell.read(cx).search().is_open()),
            "and the search overlay stayed shut while a text field had focus"
        );

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("enter").expect("keystroke"));

        let targets = cx.read(|cx| shell.read(cx).profile().block_esp_targets.clone());
        let added = targets
            .iter()
            .find(|target| target.registry_id == "minecraft:stone")
            .expect("Enter added the block the box named");
        assert_eq!(added.display_name, "stone", "named from the id");
        assert_eq!(added.color_hex, aoko_protocol::block_esp::DEFAULT_COLOR_HEX);
        assert!(added.enabled, "a new target starts enabled");
        assert_eq!(
            cx.read(|cx| field.read(cx).text().to_owned()),
            "",
            "and the box is cleared for the next add"
        );
        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .controller()
                .profile()
                .block_esp_targets
                .len()),
            targets.len(),
            "the controller is the owner the config frame is built from, so the add reached it"
        );
    }

    /// Every id form names one block, and a second add enables rather than duplicates.
    #[gpui::test]
    fn adding_a_known_block_enables_it_and_stores_no_second_entry(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);

        let before = cx.read(|cx| shell.read(cx).profile().block_esp_targets.len());
        submit_block_add(cx, &shell, &mut visual, handle, "minecraft:Diamond_Ore");

        let after = cx.read(|cx| shell.read(cx).profile().block_esp_targets.clone());
        assert_eq!(after.len(), before + 1, "one target was appended");
        let index = after
            .iter()
            .position(|target| target.registry_id == "minecraft:diamond_ore")
            .expect("the namespaced, mixed-case form normalises to the registry id");
        assert_eq!(after[index].display_name, "diamond ore");

        // Turn it off behind the box's back, then add the bare form: the same block.
        cx.update(|cx| {
            shell.update(cx, |shell, _| {
                shell.profile.block_esp_targets[index].enabled = false;
            });
        });
        submit_block_add(cx, &shell, &mut visual, handle, "diamond_ore");

        let after = cx.read(|cx| shell.read(cx).profile().block_esp_targets.clone());
        assert_eq!(
            after.len(),
            before + 1,
            "no second entry for the same block"
        );
        assert!(
            after[index].enabled,
            "the existing target was enabled rather than duplicated"
        );
    }

    /// An empty or punctuation-only box stores nothing, which is the managed no-op.
    #[gpui::test]
    fn an_empty_block_add_box_stores_nothing(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);

        let before = cx.read(|cx| shell.read(cx).profile().block_esp_targets.len());
        let field = focus_block_add(cx, &shell, &mut visual);

        for text in ["", "   ", ".:/"] {
            cx.update(|cx| field.update(cx, |field, cx| field.set_text(text, cx)));
            visual
                .cx
                .dispatch_keystroke(handle, Keystroke::parse("enter").expect("keystroke"));
            assert_eq!(
                cx.read(|cx| shell.read(cx).profile().block_esp_targets.len()),
                before,
                "{text:?} normalises to nothing, so it stores nothing"
            );
        }
    }

    /// The row is mounted on the Block ESP card, with the box and the button the spec names.
    ///
    /// The button inside it has an `ElementId` and no debug selector (`components::button`), so
    /// the press is asserted through [`Shell::add_block_target`] — the function the button's
    /// closure calls — and what the paint can prove is that the row exists at a width that holds
    /// both, on the card's tab.
    #[gpui::test]
    fn the_block_add_row_is_mounted_on_the_block_esp_card(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (shell, _, mut visual) = shell_in_window(cx, &root);

        // `BlockEspCard` is on Combat, and the card sits below the fold at the test viewport.
        scroll_into_view(&mut visual, "blockesp-add");
        let bounds = visual
            .debug_bounds("blockesp-add")
            .expect("the add row paints");
        assert!(
            bounds.size.width > px(80.0),
            "the row holds the box and the button side by side, so it is wider than a caption"
        );

        // The button's closure is this call, and an empty box is still a no-op through it.
        let before = cx.read(|cx| shell.read(cx).profile().block_esp_targets.len());
        cx.update(|cx| shell.update(cx, |shell, cx| shell.add_block_target(cx)));
        assert_eq!(
            cx.read(|cx| shell.read(cx).profile().block_esp_targets.len()),
            before,
            "an empty box is still a no-op through the press path"
        );
    }

    // ---- the palette editor's colour rows --------------------------------------

    /// Focuses one palette hex box, so a keystroke lands in it.
    fn focus_color_field(
        cx: &mut gpui::TestAppContext,
        shell: &Entity<Shell>,
        visual: &mut gpui::VisualTestContext,
        bind: &'static str,
    ) -> Entity<TextInput> {
        let field = cx
            .read(|cx| shell.read(cx).color_fields.get(bind).cloned())
            .expect("the box exists after the first frame");
        let focus = cx.read(|cx| field.read(cx).focus_handle().clone());
        visual.update(|window, cx| window.focus(&focus, cx));
        field
    }

    /// The swatch a row paints, as the shell remembers it.
    fn swatch_of(
        cx: &mut gpui::TestAppContext,
        shell: &Entity<Shell>,
        bind: &'static str,
    ) -> Color {
        cx.read(|cx| {
            shell
                .read(cx)
                .swatches
                .get(bind)
                .copied()
                .expect("the seed parsed on the first frame")
        })
    }

    /// Each colour row paints its caption, a 22x22 swatch and a box, seeded from the draft.
    ///
    /// The swatch's box is the only part a test can measure, so 22x22 is the geometry assertion
    /// and the seed is the model assertion: the first frame has to match what the managed card
    /// shows before anything is typed, and it gets there by parsing the shell's
    /// [`PaletteStore`] draft rather than by holding a second set of hexes.
    #[gpui::test]
    fn a_palette_colour_row_paints_a_swatch_and_a_box(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (shell, _, mut visual) = shell_in_window(cx, &root);

        // The create-palette card is on Settings, below the palettes it previews.
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named("settings", cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        scroll_into_view(&mut visual, "palette-swatch-PaletteBgBox");

        let bounds = visual
            .debug_bounds("palette-swatch-PaletteBgBox")
            .expect("the swatch paints");
        assert_eq!(bounds.size.width, px(22.0), "22 wide, as the markup says");
        assert_eq!(bounds.size.height, px(22.0), "and 22 tall");
        assert!(
            visual.debug_bounds("palette-row-PaletteBgBox").is_some(),
            "the caption and the box are in the same row"
        );

        // The seed is the store's draft, so the assertion reads it from the shell rather than
        // from a `PaletteDraft::default()` of the test's own: a second copy here would pass even
        // if the card seeded from somewhere else.
        let seed = cx.read(|cx| shell.read(cx).palette().draft().background.clone());
        assert_eq!(
            swatch_of(cx, &shell, "PaletteBgBox"),
            palette_store::parse_entry(&seed).expect("the draft's own default"),
            "the first frame paints the draft's background, not a literal of our own"
        );
    }

    /// A typed hex reaches the swatch, and the box that owns the text is a text field like any
    /// other: the search overlay must stay shut while it has focus.
    #[gpui::test]
    fn typing_a_hex_repaints_the_swatch(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);
        let field = focus_color_field(cx, &shell, &mut visual, "PaletteBgBox");

        // An owner write, so the six characters below are the whole value: `set_text` does not
        // fire `on_change`, which is the behaviour `TextInput` documents.
        cx.update(|cx| field.update(cx, |field, cx| field.set_text("", cx)));
        for character in ["1", "2", "3", "4", "5", "6"] {
            visual
                .cx
                .dispatch_keystroke(handle, Keystroke::parse(character).expect("keystroke"));
        }

        assert_eq!(
            cx.read(|cx| field.read(cx).text().to_owned()),
            "123456",
            "the box received the characters"
        );
        assert!(
            !cx.read(|cx| shell.read(cx).search().is_open()),
            "and the search overlay stayed shut while a text field had focus"
        );
        assert_eq!(
            swatch_of(cx, &shell, "PaletteBgBox"),
            palette_store::parse_entry("123456").expect("six digits parse"),
            "every keystroke re-parses, so the swatch follows the box"
        );
        // The draft is the owner and stores the text as typed: no `#` was added, because
        // canonicalising per keystroke would rewrite the string under the caret.
        assert_eq!(
            cx.read(|cx| shell.read(cx).palette().draft().background.clone()),
            "123456",
            "the box's text is the store's, exactly as typed"
        );

        // The repaint reaches the paint: the swatch is still the swatch.
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let bounds = visual
            .debug_bounds("palette-swatch-PaletteBgBox")
            .expect("the swatch still paints");
        assert_eq!(bounds.size.width, px(22.0), "22 wide after the change");
        assert_eq!(bounds.size.height, px(22.0), "and 22 tall");
    }

    /// A form the palette card's Add press accepts is a form the swatch accepts, because there
    /// is one tolerant reader and not two.
    ///
    /// [`palette_store::parse_entry`] is the function [`PaletteStore::save_draft`] validates
    /// with: it expands a three-digit hex and takes it without the `#`. The shell imports it
    /// rather than keeping a strict six-digit parse of its own, so a user who types `123` gets a
    /// coloured swatch instead of the refusal a second copy would give — the defect where the
    /// swatch paints a colour the press then rejects. If this fails, the two readers have
    /// diverged.
    #[gpui::test]
    fn the_swatch_accepts_what_the_add_press_accepts(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);
        let field = focus_color_field(cx, &shell, &mut visual, "PaletteAccentBox");

        cx.update(|cx| field.update(cx, |field, cx| field.set_text("", cx)));
        for character in ["1", "2", "3"] {
            visual
                .cx
                .dispatch_keystroke(handle, Keystroke::parse(character).expect("keystroke"));
        }

        assert_eq!(
            swatch_of(cx, &shell, "PaletteAccentBox"),
            palette_store::parse_entry("123").expect("the shared reader expands it"),
            "three digits parse, so the swatch took the value the Add press would"
        );
        assert_eq!(
            cx.read(|cx| shell.read(cx).palette().draft().accent.clone()),
            "123",
            "and the draft kept the short form the user typed rather than the expansion"
        );
    }

    /// Enter writes the box back as `#RRGGBB`, which is the form the palette file stores.
    #[gpui::test]
    fn enter_writes_the_hex_back_in_canonical_form(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);
        let field = focus_color_field(cx, &shell, &mut visual, "PaletteTextBox");

        for text in ["0a0b0f", "#0a0b0f"] {
            cx.update(|cx| field.update(cx, |field, cx| field.set_text(text, cx)));
            visual
                .cx
                .dispatch_keystroke(handle, Keystroke::parse("enter").expect("keystroke"));
            assert_eq!(
                cx.read(|cx| field.read(cx).text().to_owned()),
                "#0A0B0F",
                "{text:?} commits as the canonical form"
            );
            assert_eq!(
                swatch_of(cx, &shell, "PaletteTextBox"),
                palette_store::parse_entry("#0A0B0F").expect("parses"),
                "and the swatch ends on that colour"
            );
            assert_eq!(
                cx.read(|cx| shell.read(cx).palette().draft().text.clone()),
                "#0A0B0F",
                "{text:?} reaches the store in the canonical form"
            );
        }
    }

    /// A box that does not parse leaves the swatch where it was, which is what stops a
    /// half-typed `#0A` from flashing the card grey.
    #[gpui::test]
    fn an_unparseable_hex_leaves_the_swatch_alone(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);
        let field = focus_color_field(cx, &shell, &mut visual, "PaletteAccentBox");
        let before = swatch_of(cx, &shell, "PaletteAccentBox");

        for text in ["", "#", "#12345", "zzz", "#1234567"] {
            cx.update(|cx| field.update(cx, |field, cx| field.set_text(text, cx)));
            visual
                .cx
                .dispatch_keystroke(handle, Keystroke::parse("enter").expect("keystroke"));
            assert_eq!(
                swatch_of(cx, &shell, "PaletteAccentBox"),
                before,
                "{text:?} does not parse, so the swatch keeps its colour"
            );
            assert_eq!(
                cx.read(|cx| field.read(cx).text().to_owned()),
                text,
                "and the box is left exactly as typed, so it can be fixed"
            );
        }
    }
}
