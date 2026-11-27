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
    div, px, AnyElement, App, Context, Entity, FocusHandle, KeyDownEvent, MouseButton, Render,
    SharedString, Window,
};

use aoko_core::app::AppController;
use aoko_core::search;
use aoko_platform::{WindowInfo, WindowInspector};
use aoko_protocol::block_esp::{self, BlockEspTargetData};

use crate::components;
use crate::components::block_esp::{BlockEspRow, RowAction};
use crate::components::text_input::TextInput;
use crate::components::ButtonVariant;
use crate::components::FieldVariant;
use crate::dialogs::color_picker::{PickResult, Rgb};
use crate::dialogs::color_picker_view::ColorPickerView;
use crate::ingest::LinkFailure;
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
use crate::views::palette_store::{self, PaletteStore};
use crate::views::prefs::ClientPrefs;
use crate::views::profiles::{CreateOutcome, Profiles};
use crate::views::servers_state::{AddOutcome, JoinOutcome, PingRequest, ServersState};
use crate::views::stores::{NoStore, StoreBridge};

/// Which row the colour picker was opened for.
///
/// Two kinds of row host a picker: a palette editor's four hex rows, keyed by the managed box name
/// they are written back through, and a Block ESP tracked row, keyed by the `registry_id` it is
/// found by in the profile. The key is the string type each owner already uses — `&'static str`
/// for a box whose name is a constant, `String` for an id the profile holds — so the write-back
/// lands on the row that was pressed even if the tab or the list moved on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PickerTarget {
    /// One of the four palette editor rows, by the box name `PaletteDraft` keys on.
    Palette(&'static str),
    /// One tracked block, by `BlockEspTargetData::registry_id` as stored.
    Block(String),
}

/// The attach call behind one accepted target, as a value.
///
/// `aoko_platform::inject_bridge` is resolve-then-load: the library named for the target's version
/// beside the host's own executable, loaded into the target's pid. It is a field rather than a
/// direct call because `host_executable_dir()` answers with the *test harness's* directory under
/// `TestAppContext`, where no bridge library exists — so a press test hands in the same call over a
/// directory it owns, and the resolver, the name and the refusal all stay the platform's.
/// [`Shell::from_system`] builds the production one; [`Shell::set_inject_bridge`] is the seam.
///
/// `version` is the target's detected version, which is the argument
/// `aoko_platform::bridge_library_path` maps onto `bridge_*.dll`; `None` is the managed fallback to
/// the legacy bridge. It is `&'static str` rather than a borrow because the only version this
/// crate can name is [`aoko_platform::normalize_detected_version`]'s answer, which is one of five
/// canonical labels — so the seam cannot hold a borrow of the window title it was read from.
pub type InjectBridge = dyn Fn(u32, Option<&'static str>) -> InjectOutcome;

/// What one attach answers: the platform's report, or the refusal that stopped it.
///
/// Named so [`InjectBridge`] stays one line — the full paths twice in the return position is what
/// rustfmt breaks across six, and the seam is one sentence.
pub type InjectOutcome = Result<aoko_platform::InjectionReport, InjectRefusal>;

/// Why an attach did not load the bridge, in the two shapes the panel can tell apart.
///
/// A [`aoko_platform::PlatformError`] cannot travel as far as the panel unchanged: `user_facing`
/// prefixes its own kind to the sentence (`error.rs:64-71`), so painting one puts the marker of an
/// enum — a type the user has no business knowing about — at the front of a status line, and the
/// resolver's `NotFound` detail adds the version-selection reasoning behind it. The arms are
/// therefore split where the classification is still possible: where the executable's directory is
/// known, which is the production seam.
pub enum InjectRefusal {
    /// The resolver found no library at `path`.
    ///
    /// The sentence is [`copy::LOADER_REASON_BRIDGE_LIBRARY_MISSING`]'s, filled at the paint: `path`
    /// is the absolute path the resolver looked at, which is the one fact the user acts on.
    MissingLibrary { path: std::path::PathBuf },
    /// A platform refusal with no better sentence than its own.
    ///
    /// A permission error, an `Os` error, a process that vanished mid-attach: each names nothing
    /// this crate can say more usefully, so `user_facing`'s sentence is what the panel shows.
    Platform(String),
}

impl InjectRefusal {
    /// The sentence after [`copy::LOADER_STATUS_PREFIX`].
    fn sentence(&self) -> String {
        match self {
            Self::MissingLibrary { path } => copy::LOADER_REASON_BRIDGE_LIBRARY_MISSING
                .replace("{path}", &path.display().to_string()),
            Self::Platform(sentence) => sentence.clone(),
        }
    }
}

/// Classifies a platform attach failure into a refusal the panel can paint.
///
/// `NotFound` is two different answers at this seam and only the file on disk tells them apart:
/// `aoko_platform::inject_bridge` resolves the library first, while the injector reports a pid it
/// cannot open (`windows/injector.rs:47`), and both arrive as the same variant. The path is the
/// platform's own pure mapping for this version and directory — never a name joined here — so the
/// check asks the resolver's own question rather than parsing a sentence for the path inside it.
fn classify_inject(
    error: aoko_platform::PlatformError,
    version: Option<&str>,
    directory: &std::path::Path,
) -> InjectRefusal {
    let library = aoko_platform::bridge_library_path(version, directory);
    if matches!(&error, aoko_platform::PlatformError::NotFound { .. }) && !library.exists() {
        InjectRefusal::MissingLibrary { path: library }
    } else {
        InjectRefusal::Platform(error.user_facing())
    }
}

/// Opens an absolute URL in the browser: the platform's [`aoko_platform::open_url`], injected.
///
/// A field for the same reason [`InjectBridge`] is one, one degree stronger: `open_url` spawns the
/// desktop's launcher, so a test that called it would open a browser on whatever machine runs the
/// suite, and the assertion would be about the session rather than about the press. `None` is a
/// shell that opens nothing — every test starts there, and [`Shell::from_system`] is the only
/// caller that fills it.
///
/// The URL is the argument rather than baked in, so the single call site stays visible in the test
/// that counts it: [`aoko_core::services::update::RELEASES_URL`], which is the releases page the
/// managed check ended at (`MainWindow.xaml.cs:1885`).
pub type OpenUrl = dyn Fn(&str) -> Result<(), aoko_platform::PlatformError>;

/// The root view.
pub struct Shell {
    theme: Theme,
    /// Whether `--theme` pinned the palette, so the saved profile's `GuiTheme` cannot
    /// replace it at startup.
    ///
    /// The managed app has no command line, so there is no rule to port: this is the
    /// precedence a port with two palette sources needs, decided once and set by
    /// `main`. An explicit flag wins; otherwise the saved profile wins, because a user
    /// who picked a palette in Settings expects it to still be there after a restart.
    ///
    /// It governs startup only. A later, deliberate Load on the Settings card still
    /// applies the config's palette on top of a pinned one — the pin answers "which
    /// source wins before anything is clicked", not "may the palette ever change".
    theme_pinned: bool,
    tab: usize,
    /// The one owner of module state: every switch reads and writes the controller
    /// inside it. See [`crate::runtime`] for why there is no second copy.
    runtime: Runtime,
    /// The session override for every `bind` path: the capture harness seeds it and a
    /// press writes it. A value here wins over the profile snapshot below.
    ///
    /// It is an override *on top of* one profile, so `Shell::adopt_profile` clears it whenever the
    /// profile underneath is replaced wholesale — a config load. Without that, the panel would keep
    /// painting the value the user last touched while the profile, the controller and the wire all
    /// said something else, and the next touch would write the stale value back over the load.
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
    /// The overlay's query box, the seventh text field.
    ///
    /// Created with the six in [`FormFields`] for the same reason as [`Shell::block_add`]: an
    /// entity needs a context, so it exists from the first render rather than from
    /// [`Shell::new`]. It is not in that struct because nothing in it is a form: the search
    /// state is the owner of the query, and this box is its keyboard.
    ///
    /// It is in [`Shell::any_text_field_focused`] for the same reason the Block ESP box is. A
    /// character typed into it must not also be read as type-to-open, and a Ctrl+K while it has
    /// focus must not open what is already open. That last half is why the close path blurs it:
    /// a field that holds focus after its overlay is gone would close the gate for good.
    search_query: Option<Entity<TextInput>>,
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
    /// The saved palettes as their cards paint them, read from the store once.
    ///
    /// [`PaletteStore::rows`] parses the palette file, and the card paints every frame, so a paint
    /// that called it would read and parse `palettes.json` sixty times a second — the rule the
    /// store's own field doc states, that its file reads happen on this thread and never inside a
    /// paint. `None` is "not read yet": [`Shell::ensure_palette_rows`] fills it, and every press that
    /// changes the file clears it again.
    palette_rows: Option<Vec<palette_store::PaletteRow>>,
    /// The live-palette line above those rows, `PALETTE_CURRENT_PREFIX` plus the applied name.
    ///
    /// Cached on the same invalidation as [`Shell::palette_rows`], for the same reason: the name
    /// changes only when the store does, and the card paints every frame, so composing it in the
    /// render would be one `format!` per frame in the section-6 budget. `None` is "not read yet".
    palette_current: Option<SharedString>,
    /// The line the card shows for the last press that had something to say.
    ///
    /// The four stores' own `status()` methods compose it — a refused name, a refused colour, a
    /// deleted row, a file that would not write — so the card paints the store's sentence rather
    /// than a second copy of it. `None` is "nothing to say", which paints no row at all rather than
    /// an empty one.
    palette_status: Option<SharedString>,
    /// The palette card's name box (`NewPaletteNameBox`, `MainWindow.xaml:1953`).
    ///
    /// The typed name is not store state — [`PaletteStore::save_draft`] takes it as an argument and
    /// the store's four drafts are the hex boxes — so the entity is the shell's, the way
    /// [`Shell::block_add`] is, and its callback writes the draft's `name`, which is the field the
    /// Add press reads.
    palette_name: Option<Entity<TextInput>>,
    /// The colour picker, while it is open: the row it edits and the dialog entity.
    ///
    /// `None` is closed, and the row is named in the terms its owner already uses (see
    /// [`PickerTarget`]), so the write lands on the row the picker was opened for even if the tab,
    /// the draft or the list moved on.
    ///
    /// The dialog reports and does not dismiss itself, so this is the host's half: it is cleared
    /// on any result, and the row is written only for an acceptance.
    picker: Option<(PickerTarget, Entity<ColorPickerView>)>,
    /// The shell's own focus handle: what holds the keyboard when no control does.
    ///
    /// This is what makes the window keys work at all. GPUI dispatches a key event from
    /// `Window::focus`'s node up to the dispatch tree's *synthetic* root, and that root carries no
    /// listeners (`window.rs:7113-7121`, `key_dispatch.rs:611-614`). With nothing focused the walk
    /// therefore starts and ends at a node that has no `on_key_down` on it, so the root listener
    /// this shell installs is never called: Ctrl+K on a freshly opened window did nothing, and
    /// Escape could not reach the colour picker's host. Tracking this handle on the root element
    /// and holding it whenever nothing else does puts the shell's own tree back on the path.
    ///
    /// Anything that takes focus — a text field, a keybind capture — takes it away from here for
    /// as long as it holds it, which is the same rule [`Shell::search_gate`] reads. `None` until
    /// the first frame, because the handle needs a context the constructors do not have.
    window_focus: Option<FocusHandle>,
    /// The loader's state machine: `InjectButton_Click` and the status line it paints
    /// (`MainWindow.xaml.cs:1431-1503`).
    ///
    /// The *transient* half only — idle, detecting, connecting, a refusal, and an attach the
    /// platform would not perform. Whether injection has actually landed is the controller's answer,
    /// read on every frame by [`InjectionView::resolve`] through [`Shell::injection_view`] and never
    /// stored here: a second `connected` flag is the defect this branch keeps deleting.
    injection: InjectionStatus,
    /// The windows the last detection found, in the platform's own order.
    ///
    /// Every row here satisfies at least one half of the injection gate and names its owning
    /// process, so the panel's list is the enumeration filtered by the same rules the press applies
    /// — never a title list of its own. Empty until the first press.
    injection_targets: Vec<WindowInfo>,
    /// Which of [`Shell::injection_targets`] the press acts on.
    injection_selected: Option<usize>,
    /// Whether the loader is still what the window paints.
    ///
    /// The managed two-state swap (`MainWindow.xaml:306`/`:344`, `MainWindow.xaml.cs:469-492`),
    /// latched the way `MainWindow.xaml.cs:524-528` latches it: leaving the loader for the control
    /// panel happens once, on the first connection, and a later disconnect does not bring the
    /// loader back.
    ///
    /// A session with no window surface starts *in* control mode, because the loader's whole job
    /// is choosing a window: with nothing to enumerate there is no state it could reach. That is
    /// also what every unit test and the capture harness get, which is what keeps the nine
    /// screenshots painting the tabs.
    loader_visible: bool,
    /// The platform's window surface, `None` when this session cannot inspect windows.
    ///
    /// Built by [`Shell::from_system`] from `aoko_platform::default_services`, and `None` under
    /// `cfg(test)` and `capture` — the same rule the rest of the startup path keeps, so a test
    /// never opens a display connection. [`Shell::set_windows`] is the seam a test drives the
    /// loader through.
    windows: Option<Arc<dyn WindowInspector>>,
    /// The attach call, [`aoko_platform::inject_bridge`] with the host's directory resolved, or
    /// `None` in a build with no injector.
    ///
    /// Built by [`Shell::from_system`] from the platform's own injector and left `None` under
    /// `cfg(test)` and `capture`, the same rule [`Shell::windows`] keeps and for a related reason: a
    /// test harness's executable directory holds no bridge library, and a press in a test must not
    /// load one into anything. A `None` seam is a press that dials without attaching, which is the
    /// shape every test built before this seam existed relies on; [`Shell::set_inject_bridge`] is how
    /// a test supplies its own.
    inject_bridge: Option<Arc<InjectBridge>>,
    /// The new-config name box's text (`NewConfigNameBox`, `MainWindow.xaml:1552`).
    config_name: String,
    /// Whether the session can hand a folder to the file manager
    /// (`CapabilityReport::open_path`). False until [`Shell::from_system`] probes it,
    /// so the button cannot claim a capability this build has not checked.
    open_configs_folder: bool,
    /// The URL opener behind the rail's version line (`MainWindow.xaml.cs:1885`).
    ///
    /// `None` until [`Shell::from_system`] installs the platform's, which is what keeps the suite
    /// from spawning a launcher; [`Shell::set_url_opener`] is the seam the press test uses.
    url_opener: Option<Arc<OpenUrl>>,
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
    /// A shell on the given palette, showing Combat, with no palette pinned.
    ///
    /// In the shipped binary this **loads the auto-save profile**, exactly as
    /// `MainWindow.xaml.cs:407-412` does: the managed app read the `config` slot once at
    /// construction and never reloaded per tab. Under `cfg(test)` and under the
    /// `capture` feature it deliberately does no I/O, so a unit test and a screenshot
    /// never depend on the machine's profile directory.
    ///
    /// The binary itself calls [`Shell::startup`], which is this shell with the
    /// `--theme` pin answered; `new` is the same startup with nothing pinned, because a
    /// test and a capture have no command line to read one from.
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        Self::startup(theme, false)
    }

    /// The startup shell, with `--theme`'s precedence answered by the caller.
    ///
    /// `theme_pinned` is true when the command line named a palette. Then that palette
    /// stays, and the saved profile's `GuiTheme` does not replace it; when it is false
    /// the profile's own palette is what the window paints, which is what makes a
    /// palette chosen in Settings survive a restart. The flag is a parameter rather than
    /// a comparison against the default, so the precedence is stated at the call site
    /// instead of inferred here.
    ///
    /// Under `cfg(test)` and the `capture` feature nothing is read, so there is no
    /// profile to adopt and the pin has nothing to decide.
    #[must_use]
    pub fn startup(theme: Theme, theme_pinned: bool) -> Self {
        #[cfg(all(not(test), not(feature = "capture")))]
        {
            Self::from_system(theme, theme_pinned)
        }

        #[cfg(any(test, feature = "capture"))]
        {
            let _ = theme_pinned;
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
    /// machine. [`Shell::startup`] needs no `Context` for either step: the controller is not
    /// an entity, so nothing here waits for `cx.new`.
    ///
    /// The bridge reader is started here too, and only here: it dials on its own thread,
    /// so a bridge that is not running costs a `warn!` instead of a stalled first frame.
    /// Supervision is switched on in the same place, which is what makes a module switch
    /// start and stop the automation loops — a shell built for a test never gets that far,
    /// so no test and no capture run owns an automation thread.
    #[must_use]
    pub fn from_system(theme: Theme, theme_pinned: bool) -> Self {
        let capabilities = aoko_platform::probe_capabilities();
        let mut shell = Self::start(
            theme,
            Profiles::from_system(),
            crate::views::prefs::PrefsFile::from_system(),
        );

        // The profile has just landed, so it is now that the palette question can be
        // answered: `--theme` wins when it was given, the profile's `GuiTheme` otherwise.
        // Doing it in `with_loaded_profile` instead would make every `with_store` test
        // read this machine's palette file, which is the property `Shell::new` keeps.
        shell.set_theme_pinned(theme_pinned);
        shell.adopt_profile_theme();

        // `CapabilityReport::open_path` is the flag the "Open Configs Folder" row is
        // keyed on: Linux needs `xdg-open` on `PATH`, and a session without it must
        // not look like it can reveal the folder.
        shell.open_configs_folder = capabilities.open_path;
        shell.runtime = Runtime::new(Arc::new(AppController::new(capabilities)));

        // The loader's window list comes from the platform, and this is the only session that may
        // touch it: `default_services()` builds the real inspector, so a test or a capture run
        // keeps `None` and opens no display connection (see `Shell::windows`).
        //
        // The loader is what a shipped window paints until the bridge answers
        // (`MainWindow.xaml.cs:469-492`). A session with no window surface starts *in* the control
        // panel instead, because the loader's whole job is choosing a window it cannot enumerate —
        // and that is exactly what keeps the nine captured screens painting tabs.
        let services = aoko_platform::default_services();
        let windows: Arc<dyn WindowInspector> = services.windows.into();
        shell.set_windows(Some(windows));
        shell.loader_visible = true;

        // The attach behind the Inject press, as the platform spells it: resolve the bridge library
        // named for the target's version beside *this* executable, then load it into the target's
        // pid. Composing it here rather than in the press is what keeps the library name out of this
        // crate — `aoko_platform::inject_bridge` owns the version-to-name mapping and
        // `host_executable_dir` owns where the host ships the file, so a wrong name is fixed there.
        //
        // The two calls are the whole seam, in the order that lets a missing file fail before the
        // injector is touched: `host_executable_dir` first, because it is the OS call that can fail
        // on its own, then `inject_bridge`, whose `NotFound` names the path it looked for.
        let injector = services.injector;
        shell.set_inject_bridge(Some(Arc::new(move |pid, version| {
            // A directory that cannot be found is its own refusal: there is no path to name, so the
            // platform's sentence is the only one available.
            let directory = aoko_platform::host_executable_dir()
                .map_err(|error| InjectRefusal::Platform(error.user_facing()))?;
            aoko_platform::inject_bridge(injector.as_ref(), pid, version, &directory)
                .map_err(|error| classify_inject(error, version, &directory))
        })));

        // The rail's version line opens the releases page — the outcome managed's check existed
        // for: it fetched a manifest to learn whether a newer tag existed and then handed the
        // `html_url` to `Process.Start` (`MainWindow.xaml.cs:1885`). With no fetcher in this tree
        // there is no update status to paint, but the page it led to is one press away. Installed
        // here so the shipped binary is the only build that can launch anything.
        shell.set_url_opener(Some(Arc::new(aoko_platform::open_url)));

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

    /// Installs the URL opener, as the startup path installs the platform's.
    ///
    /// Tests hand in a recorder instead, for the reason [`OpenUrl`] documents: the real one
    /// launches the desktop's browser, which no assertion can read and no suite should open.
    pub fn set_url_opener(&mut self, opener: Option<Arc<OpenUrl>>) {
        self.url_opener = opener;
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
            // Nothing is pinned until a caller says so: `startup` passes `--theme`'s answer,
            // and a test or a capture starts with the palette it was handed.
            theme_pinned: false,
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
            search_query: None,
            // No window surface and no loader: a test or a capture starts in control mode; see
            // the field docs. `from_system` fills both in.
            injection: InjectionStatus::NotInjected,
            injection_targets: Vec::new(),
            injection_selected: None,
            loader_visible: false,
            windows: None,
            // No injector either: `from_system` is the only session that may load a bridge library,
            // and a press in a test or a capture therefore dials without attaching. See the field's
            // own docs.
            inject_bridge: None,
            // Seeded without reading the file: `from_system` only resolves the path, and `at`
            // touches nothing. The reads are `palettes()`/`resolve()`, the write is
            // `save_draft()`, and no constructor calls any of them — which is what keeps a unit
            // test and a screenshot off the machine's palette directory.
            palette: PaletteStore::from_system()
                .unwrap_or_else(|_| PaletteStore::at(std::path::PathBuf::new())),
            color_fields: HashMap::new(),
            swatches: HashMap::new(),
            // Both read on the first frame: the rows are a file read and the box needs a context.
            palette_rows: None,
            palette_current: None,
            palette_status: None,
            palette_name: None,
            // Closed: a picker exists only between a swatch press and the report that ends it.
            picker: None,
            // Filled on the first frame, where a context is available; see the field docs.
            window_focus: None,
            config_name: String::new(),
            open_configs_folder: false,
            url_opener: None,
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
    fn with_loaded_profile(mut self, profile: aoko_protocol::profile::Profile) -> Self {
        // The same replacement a load performs, so the rule lives in one place; a fresh shell's
        // override map is empty, and this keeps it that way if one is ever planted before this
        // runs.
        self.adopt_profile(profile);
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
    ///
    /// The profile it writes is the controller's
    /// ([`AppController::profile_for_save`]), not this view's `self.profile` copy: a switch press
    /// writes the controller's flag set and never touches that copy, so saving it wrote the state
    /// from before the press — which is how a module switched on came back off after a restart.
    /// Managed rebuilt the profile from the clicker at every save
    /// (`ProfileManager.CreateFromClicker()`, `:956-958`, `:1867-1869`), which is the same rule.
    pub fn save_profile(&mut self) -> Result<(), String> {
        let mut profile = self.controller().profile_for_save();
        let result = self.profiles.save_auto(&mut profile);
        self.profile = profile;
        // The store stamps the profile's name before it writes (`save_auto`), so the copy it
        // hands back is not always the one that went in. A write site that keeps the stamped
        // copy without telling the controller leaves the two out of step from the first save.
        self.push_profile();
        result
    }

    /// The Settings card's Save: writes the selected config name.
    ///
    /// Starts from the controller's profile for the same reason [`Shell::save_profile`] does: the
    /// live switches are the controller's, and a copy of `self.profile` never carries them.
    pub fn save_config(&mut self, name: &str) -> Result<(), String> {
        let mut profile = self.controller().profile_for_save();
        let result = self.profiles.save_named(name, &mut profile);
        self.profile = profile;
        // Same as [`Shell::save_profile`]: `save_named` sets `profile.name` on the way through.
        self.push_profile();
        result
    }

    /// Replaces the whole profile, which invalidates the session overrides with it.
    ///
    /// [`Shell::draft`] is an override *on top of* the profile snapshot, so it is only truthful
    /// while the profile underneath it is the one the user was editing. A wholesale replacement —
    /// a config load — makes every override a value nobody chose for this profile, and the next
    /// touch would write it back over the load. Managed has no such layer: `ApplyLoadedProfile`
    /// rewrites every `Clicker` property and the TwoWay bindings repaint from it
    /// (`MainWindow.xaml.cs:835-848`). So the replacement is the one place that clears it, and every
    /// path that swaps the profile goes through here rather than assigning the field.
    fn adopt_profile(&mut self, mut profile: aoko_protocol::profile::Profile) {
        profile.normalize_action_binds();
        self.values = crate::views::bindings::snapshot(&profile);
        self.profile = profile;
        self.draft.clear();
    }

    /// The Settings card's Load: replaces the live settings with a saved config.
    pub fn load_config(&mut self, name: &str) -> Result<(), String> {
        match self.profiles.load_named(name) {
            Ok(profile) => {
                // The load-time half of the action-bind rule, and the override reset, are the
                // replacement's own business; see `Shell::adopt_profile`.
                self.adopt_profile(profile);
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
    /// fresh store, so the draft returns to [`palette_store::PaletteDraft::default`] — which is
    /// why it belongs before the first frame and not in the middle of an edit.
    pub fn set_palette_path(&mut self, path: impl Into<std::path::PathBuf>) {
        self.palette = PaletteStore::at(path);
        // A new file is a new list: the cards paint the cache, not the store.
        self.palette_rows = None;
        // And a new list has a new applied entry, so the line above it is stale too.
        self.palette_current = None;
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

    /// Pins the palette against the saved profile, or unpins it; see `theme_pinned`.
    ///
    /// The binary decides through [`Shell::startup`]'s parameter. Tests call this to drive
    /// the startup branch itself, because a test shell has no command line to read a pin
    /// from and no profile store to load one out of.
    pub fn set_theme_pinned(&mut self, pinned: bool) {
        self.theme_pinned = pinned;
    }

    /// Adopts the palette the loaded profile names, unless `--theme` pinned one.
    ///
    /// This is the startup half of `ApplyLoadedProfile` (`MainWindow.xaml.cs:835-848`) and
    /// the same rule [`Shell::load_config`] applies when a config is loaded by hand: the
    /// palette travels in the profile, so a shell that has just read one has to paint with
    /// the name that profile carries. The managed app got this for free because it resolved
    /// its theme from the profile it had just read; here the theme is resolved before that
    /// read, so the read has to correct it.
    ///
    /// Resolved through [`Shell::palette`] — the store this shell writes palettes into —
    /// rather than through [`palette_store::theme_for`], which builds its own store from the
    /// machine's app-data path. That choice is what makes the rule observable: a test points
    /// the store at a scratch file and sees a custom palette adopted without this machine's
    /// palette directory being read at all. `load_config` still calls `theme_for`, which reads
    /// the same file in the shipped binary; the two should become one call once the
    /// saved-palette card makes this store the writer as well as the reader.
    fn adopt_profile_theme(&mut self) {
        if self.theme_pinned {
            return;
        }
        self.theme = self.palette.resolve(&self.profile.gui_theme);
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

        // The overlay's query box. The placeholder is the copy owner's constant, which is also
        // what `crate::overlay`'s static branch paints: one sentence, one owner, whichever of the
        // two branches a caller draws.
        if self.search_query.is_none() {
            let handle = cx.entity();
            let input = cx.new(|cx| {
                TextInput::new(cx, theme.clone(), copy::SEARCH_QUERY_NAME)
                    .variant(FieldVariant::Query)
                    .on_change(move |text, _window, cx| {
                        let text = text.to_string();
                        handle.update(cx, |shell, cx| shell.set_search_query(text, cx));
                    })
            });
            self.search_query = Some(input);
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

            let seed = self.palette.get(bind).unwrap_or_default().to_owned();
            if let Some(color) = self.palette.get(bind).and_then(palette_store::parse_entry) {
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

    /// Reads the saved palettes once, for the cards the `CustomPalettesItems` template paints.
    ///
    /// Called at the top of [`Render::render`] beside the other `ensure_*` calls, because
    /// [`PaletteStore::rows`] is a file read: once, not once per frame. The field doc has the rule.
    fn ensure_palette_rows(&mut self) {
        if self.palette_rows.is_none() {
            self.palette_rows = Some(self.palette.rows());
        }
        // The live-palette line comes from the same read, because it is the same fact: which palette
        // this file says is applied. Composing it here keeps the `format!` out of the paint.
        if self.palette_current.is_none() {
            self.palette_current = Some(SharedString::from(format!(
                "{}{}",
                copy::PALETTE_CURRENT_PREFIX,
                self.palette.current_name()
            )));
        }
    }

    /// Creates the palette card's name box, once.
    ///
    /// The same reason as [`Shell::ensure_block_add`] — an entity needs a context — and the same
    /// owner rule: the box has no placeholder (`views/copy.rs` carries none for `NewPaletteNameBox`)
    /// and its text lands in the draft's `name`, which is what [`PaletteStore::save_draft`] reads, so
    /// there is no second copy of the typed name.
    fn ensure_palette_name(&mut self, cx: &mut Context<Self>) {
        if self.palette_name.is_some() {
            return;
        }
        let theme = self.theme.clone();
        let handle = cx.entity();
        self.palette_name = Some(cx.new(|cx| {
            TextInput::new(cx, theme, "").on_change(move |text, _window, cx| {
                let name = text.to_string();
                handle.update(cx, |shell, cx| shell.set_palette_name(name, cx));
            })
        }));
    }

    /// The name box's write, which lands in the draft the Add press reads.
    fn set_palette_name(&mut self, name: String, cx: &mut Context<Self>) {
        if self.palette.draft().name == name {
            return;
        }
        self.palette.draft_mut().name = name;
        cx.notify();
    }

    /// Applies one palette: the row press, and the preview press the card's copy calls instant
    /// (`Click a palette preview to apply it instantly.`).
    ///
    /// `GuiPaletteButton_Click` (`MainWindow.xaml.cs:930-960`) in three steps: the store's name, the
    /// theme that name paints, and the profile the name lives in. The last is not decoration — a
    /// store that moved without the profile leaves the two disagreeing at the next load, which is
    /// what [`palette_store::PaletteStore::apply`]'s own note warns about. `set_theme` recreates the
    /// field entities because each carries a copy of the palette.
    pub fn apply_palette(&mut self, name: &str, cx: &mut Context<Self>) {
        self.palette.apply(name);
        let theme = self.palette.theme();
        self.profile.gui_theme = theme.name.clone();
        self.set_theme(theme, cx);
        if let Err(error) = self.save_profile() {
            self.palette_status = Some(SharedString::from(error));
        }
        // The line above the rows reads the store's current name, which just moved.
        self.palette_current = None;
        cx.notify();
    }

    /// A cap press: arms the confirmation and removes nothing.
    ///
    /// Managed asked in a `MessageBox` (`MainWindow.xaml.cs:1178-1181`); the card asks in place, so
    /// the armed name is state and the removal is a second press.
    pub fn palette_request_delete(&mut self, name: &str, cx: &mut Context<Self>) {
        self.palette.request_delete(name);
        cx.notify();
    }

    /// The confirmation's Delete.
    pub fn palette_confirm_delete(&mut self, cx: &mut Context<Self>) {
        match self.palette.confirm_delete() {
            Some(Ok(outcome)) => {
                self.palette_status = Some(SharedString::from(outcome.status));
                self.palette_rows = None;
                // Deleting the applied palette falls back to `Slate`; the theme, the live line and
                // the profile all follow the fallback the outcome names.
                if let Some(fallback) = outcome.fallback {
                    self.profile.gui_theme = fallback.to_owned();
                    let theme = self.palette.theme();
                    self.set_theme(theme, cx);
                    let _ = self.save_profile();
                }
                self.palette_current = None;
            }
            Some(Err(error)) => self.palette_status = Some(SharedString::from(error.status())),
            // Nothing armed: the confirmation is only painted while a row is, so this is unreachable
            // rather than a state the card can be in.
            None => {}
        }
        cx.notify();
    }

    /// The confirmation's Cancel: the file keeps the palette.
    pub fn palette_cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.palette.cancel_delete();
        cx.notify();
    }

    /// `AddCustomPaletteButton_Click` (`MainWindow.xaml.cs:1095-1171`), first press: save, or ask.
    ///
    /// This is where the case-insensitive built-in refusal and the `#AARRGGBB` refusal become
    /// reachable by a user — `save_draft` returns them as the sentence the card paints.
    pub fn palette_add(&mut self, cx: &mut Context<Self>) {
        match self.palette.save_draft(false) {
            Ok(palette_store::DraftOutcome::Saved { status, .. }) => {
                self.adopt_saved_palette(status, cx)
            }
            // The store armed the question; the card paints it from `overwrite_pending`.
            Ok(palette_store::DraftOutcome::ConfirmOverwrite { .. }) => {}
            Err(error) => self.palette_status = Some(SharedString::from(error.status())),
        }
        cx.notify();
    }

    /// The overwrite prompt's confirm.
    pub fn palette_confirm_overwrite(&mut self, cx: &mut Context<Self>) {
        match self.palette.confirm_overwrite() {
            Ok(palette_store::DraftOutcome::Saved { status, .. }) => {
                self.adopt_saved_palette(status, cx)
            }
            // `overwrite = true` cannot ask again, so this arm keeps the prompt rather than claiming
            // a save that did not happen.
            Ok(palette_store::DraftOutcome::ConfirmOverwrite { .. }) => {}
            Err(error) => self.palette_status = Some(SharedString::from(error.status())),
        }
        cx.notify();
    }

    /// The overwrite prompt's Cancel: the file keeps its row and nothing is written.
    pub fn palette_cancel_overwrite(&mut self, cx: &mut Context<Self>) {
        self.palette.cancel_overwrite();
        cx.notify();
    }

    /// Reads a finished save back into the card, the theme and the profile.
    ///
    /// `save_draft` applies the name it wrote, so these are the same steps a press on that row runs
    /// — which is the point: a save and an apply must not paint differently.
    fn adopt_saved_palette(&mut self, status: String, cx: &mut Context<Self>) {
        self.palette_status = Some(SharedString::from(status));
        self.palette_rows = None;
        self.palette_current = None;
        let theme = self.palette.theme();
        self.profile.gui_theme = theme.name.clone();
        self.set_theme(theme, cx);
        if let Err(error) = self.save_profile() {
            self.palette_status = Some(SharedString::from(error));
        }
    }

    /// The one write path for a palette row's text: the box's `on_change`, the Enter commit and
    /// the colour picker all land here.
    ///
    /// The draft is the owner, and [`PaletteStore::set`] is the only way in: the text is stored
    /// even when it does not parse, because `PaletteDraft` deliberately keeps an invalid entry
    /// visible so it can be fixed, and the Add press is where it is refused. The swatch's memo
    /// only moves on a successful parse, so a half-typed `#0A` leaves the colour it had rather
    /// than flashing empty.
    ///
    /// The parse reads the draft back rather than the argument, so the swatch can only show what
    /// the store holds — an unknown bind writes nothing, and a row for it gets no colour either.
    /// [`palette_store::parse_entry`] is the reader the Add press itself uses, so a form the
    /// swatch accepts is a form the press accepts and a role the picker shows as valid cannot be
    /// the reason the save refuses.
    fn set_color_text(&mut self, bind: &'static str, text: &str, cx: &mut Context<Self>) {
        self.palette.set(bind, text);

        if let Some(color) = self.palette.get(bind).and_then(palette_store::parse_entry) {
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

        self.write_color_text(bind, &color.to_hex(), cx);
    }

    /// The commit half shared by Enter and the picker: the store, the memo, then the box.
    ///
    /// One function so the two gestures cannot end in different places. The store is written by
    /// [`Shell::set_color_text`], which is also what moves the memo; the box is written last
    /// because it is the display, and `set_text` does not fire `on_change`, so this cannot feed
    /// itself back through the write path it just used.
    fn write_color_text(&mut self, bind: &'static str, canonical: &str, cx: &mut Context<Self>) {
        self.set_color_text(bind, canonical, cx);
        if let Some(field) = self.color_fields.get(bind).cloned() {
            if field.read(cx).text() != canonical {
                field.update(cx, |field, cx| field.set_text(canonical.to_owned(), cx));
            }
        }
    }

    /// Opens the colour picker on the colour one palette row is painting.
    ///
    /// The initial value is the swatch's memo — the colour on screen — with the draft's own text
    /// as the fallback for a row whose memo has not been seeded. Not `Rgb::default()`: a picker
    /// that opened on black would change the row simply by being accepted.
    ///
    /// The dialog is built here and given the one callback it reports through; it neither writes
    /// the row nor dismisses itself. `Accepted` is what the callback carries, and a refusal has
    /// to leave every one of the three writes alone.
    pub fn open_color_picker(&mut self, bind: &'static str, cx: &mut Context<Self>) {
        let initial = self
            .swatches
            .get(bind)
            .copied()
            .or_else(|| self.palette.get(bind).and_then(palette_store::parse_entry))
            .map(Rgb::from)
            .unwrap_or_default();

        self.open_picker_on(PickerTarget::Palette(bind), initial, cx);
    }

    /// Opens the colour picker on one tracked block, which is the row's `Color` control.
    ///
    /// The initial value is the colour the row is painting, through the component's own
    /// `BlockEspRow::color` — the same `normalize_color` rule the swatch draws with, so a picker
    /// that opened on anything else would change the entry merely by being accepted.
    pub fn open_block_color_picker(&mut self, registry_id: &str, cx: &mut Context<Self>) {
        let Some(target) = self
            .profile
            .block_esp_targets
            .iter()
            .find(|target| target.registry_id == registry_id)
        else {
            return;
        };

        // The row is cloned out before the dialog is built, so the borrow of `self.profile` ends
        // here rather than overlapping the `&mut self` that opens the picker.
        let row = BlockEspRow::new(
            target.registry_id.clone(),
            target.display_name.clone(),
            target.color_hex.clone(),
            target.enabled,
        );
        let key = row.registry_id.to_string();

        self.open_picker_on(PickerTarget::Block(key), Rgb::from(row.color()), cx);
    }

    /// Builds the dialog for one row and records it as the open picker.
    ///
    /// The two hosts differ only in what they ask and where the answer is written, so the dialog,
    /// its callback and the "reports, never dismisses itself" contract live here once.
    fn open_picker_on(&mut self, target: PickerTarget, initial: Rgb, cx: &mut Context<Self>) {
        let theme = self.theme.clone();
        let handle = cx.entity().downgrade();
        let key = target.clone();
        let view = cx.new(move |cx| {
            ColorPickerView::new(cx, theme, initial).on_result(move |result, _window, cx| {
                if let Some(shell) = handle.upgrade() {
                    shell.update(cx, |shell, cx| {
                        shell.finish_color_picker(key.clone(), result, cx)
                    });
                }
            })
        });

        self.picker = Some((target, view));
        cx.notify();
    }

    /// The picker reported: close it, and write the row only when the report was an acceptance.
    ///
    /// A refusal changes no colour, no draft text, no memo and no status line. It says nothing
    /// because the dialog has already said whatever there was to say about the entry it refused,
    /// and because Cancel is not an error. Closing happens either way, which is the host's half
    /// of the contract: the view reports an outcome and never dismisses itself.
    fn finish_color_picker(
        &mut self,
        target: PickerTarget,
        result: PickResult,
        cx: &mut Context<Self>,
    ) {
        self.picker = None;
        if result.accepted {
            match target {
                PickerTarget::Palette(bind) => self.apply_color(bind, result.color, cx),
                PickerTarget::Block(registry_id) => {
                    self.apply_block_color(&registry_id, result.color, cx);
                }
            }
        }
        cx.notify();
    }

    /// Writes a picked colour into one tracked block.
    ///
    /// `BlockEspTargetData::color_hex` is six uppercase digits with no `#` — the form
    /// `block_esp::normalize_color` accepts and the form the presets are written in — so the
    /// dialog's canonical `#RRGGBB` is reduced to that rather than stored as it arrives.
    fn apply_block_color(&mut self, registry_id: &str, color: Rgb, cx: &mut Context<Self>) {
        // Exactly one leading `#`, which is what `palette_store::parse_entry` strips on the way
        // in, so the value written here is the value that would be read back.
        let canonical = color.to_hex();
        let stored = canonical.strip_prefix('#').unwrap_or(&canonical).to_owned();
        let Some(index) = self
            .profile
            .block_esp_targets
            .iter()
            .position(|target| target.registry_id == registry_id)
        else {
            return;
        };
        if self.profile.block_esp_targets[index].color_hex == stored {
            return;
        }

        self.profile.block_esp_targets[index].color_hex = stored;
        self.push_profile();
        cx.notify();
    }

    /// One tracked-block row's press, written through to the profile.
    ///
    /// The row carries the value to write, not a request to invert what it paints: `Toggle`'s
    /// `enabled` is the checkbox's new state, so a frame that arrives late cannot flip the entry
    /// twice. `Color` opens the picker on that entry, and `Remove` drops exactly one id rather than
    /// re-creating the list from the visible rows.
    ///
    /// Every write ends in `push_profile`, because the profile is what the config frame is built
    /// from; a row that changed the paint and not the controller would be the second-copy defect
    /// this branch keeps deleting.
    pub fn apply_row_action(&mut self, action: RowAction, cx: &mut Context<Self>) {
        match action {
            RowAction::Toggle {
                registry_id,
                enabled,
            } => {
                let Some(index) = self
                    .profile
                    .block_esp_targets
                    .iter()
                    .position(|target| target.registry_id == registry_id)
                else {
                    return;
                };
                if self.profile.block_esp_targets[index].enabled == enabled {
                    return;
                }
                self.profile.block_esp_targets[index].enabled = enabled;
            }
            RowAction::Remove { registry_id } => {
                let before = self.profile.block_esp_targets.len();
                self.profile
                    .block_esp_targets
                    .retain(|target| target.registry_id != registry_id);
                if self.profile.block_esp_targets.len() == before {
                    return;
                }
            }
            RowAction::Color { registry_id } => {
                self.open_block_color_picker(&registry_id, cx);
                return;
            }
        }

        self.push_profile();
        cx.notify();
    }

    /// Writes a picked colour into one row.
    ///
    /// The same three writes as Enter, minus the parse: the picker hands back an `Rgb`, which is
    /// a colour by construction, so there is nothing to refuse here. A picked role and a typed
    /// one therefore cannot end up in different places.
    pub fn apply_color(&mut self, bind: &'static str, color: Rgb, cx: &mut Context<Self>) {
        let canonical = color.to_hex();
        self.write_color_text(bind, &canonical, cx);
    }

    // === The loader (`LoaderPanel`, `MainWindow.xaml:306-340`) ===

    /// Replaces the window surface, which is the seam a test drives the loader through.
    ///
    /// `None` is what `cfg(test)` and `capture` builds get, and it is also what a platform without
    /// window inspection gets; see `Shell::windows`.
    pub fn set_windows(&mut self, windows: Option<Arc<dyn WindowInspector>>) {
        self.windows = windows;
    }

    /// Replaces the attach call, which is the seam a test drives the Inject press's injection
    /// through.
    ///
    /// `None` is what a `cfg(test)` and a `capture` build get: the production call resolves the
    /// bridge library beside `current_exe`, which under `TestAppContext` is the test harness, so a
    /// test hands in [`aoko_platform::inject_bridge`] over a directory it owns instead of reaching
    /// for the filesystem the shipped build reads. Same shape as [`Shell::set_windows`]: a
    /// caller-supplied dependency the production constructor fills in.
    pub fn set_inject_bridge(&mut self, inject: Option<Arc<InjectBridge>>) {
        self.inject_bridge = inject;
    }

    /// Replaces the runtime, which is how a test hands the loader a clock it can advance.
    ///
    /// `Shell::new` builds a headless runtime on the system clock, so nothing else in a unit test
    /// can cross [`crate::runtime::CONNECT_DEADLINE_MS`] without sleeping for it. Same seam as
    /// [`Shell::set_windows`]: a caller-supplied dependency the production constructors fill in.
    #[cfg(test)]
    pub fn set_runtime(&mut self, runtime: Runtime) {
        self.runtime = runtime;
    }

    /// Whether the window is still painting the loader rather than the control panel.
    #[must_use]
    pub fn loader_visible(&self) -> bool {
        self.loader_visible
    }

    /// The targets the last detection found, in the platform's order.
    #[must_use]
    pub fn injection_targets(&self) -> &[WindowInfo] {
        &self.injection_targets
    }

    /// Which target the press acts on.
    #[must_use]
    pub fn injection_selection(&self) -> Option<usize> {
        self.injection_selected
    }

    /// What the panel paints, resolved from the machine, the controller's link and the reader's
    /// report.
    #[must_use]
    pub fn injection_view(&self) -> InjectionView {
        InjectionView::resolve(
            &self.injection,
            self.runtime.ingest_status().connected,
            self.runtime.link_failure(),
        )
    }

    /// The Inject press (`InjectButton_Click`, `MainWindow.xaml.cs:1431-1467`).
    ///
    /// The managed order: the button goes busy and says "Detecting", the targets are enumerated,
    /// a single one is taken without asking (`:1449-1452`), and more than one waits for a choice.
    /// Then `Shell::attach` runs.
    ///
    /// The press is gated on the *painted* rule rather than on a second one of its own: the button
    /// is pressable exactly when [`Shell::injection_view`] says `enabled`, so a state the panel
    /// shows as pressable cannot refuse the press and a disabled button cannot be bypassed.
    /// `components::button` has no disabled state at the pin, so the button's copy ("Detecting...",
    /// "...", "Connected") is the whole affordance and this early return is what makes it truthful.
    ///
    /// That is also what makes a failed dial retryable: [`InjectionStatus::Connecting`] stays on the
    /// machine, but once the runtime reports why the link is down the painted rule is "pressable",
    /// so the press after a failure is a real retry rather than a press the machine swallows.
    pub fn inject(&mut self, cx: &mut Context<Self>) {
        if !self.injection_view().enabled {
            return;
        }

        self.injection = InjectionStatus::Detecting;
        cx.notify();

        let Some(windows) = self.windows.clone() else {
            self.injection = InjectionStatus::Refused(Refusal::NoWindowSense);
            cx.notify();
            return;
        };

        // An enumeration that fails is "no target". There is no message channel from the
        // platform to this panel, so it is not distinguishable from an empty list here.
        let mut targets: Vec<WindowInfo> = windows
            .enumerate()
            .unwrap_or_default()
            .into_iter()
            .filter(target_is_candidate)
            .collect();
        aoko_platform::sort_windows(&mut targets);
        self.injection_targets = targets;

        if self.injection_targets.is_empty() {
            self.injection_selected = None;
            self.injection = InjectionStatus::Refused(Refusal::NoWindow);
            cx.notify();
            return;
        }

        // One target needs no choice; more than one stays in `Detecting` with the list on screen,
        // which is where the managed panel stands behind its open picker.
        if self.injection_targets.len() == 1 {
            self.injection_selected = Some(0);
        }

        match self
            .injection_selected
            .filter(|index| *index < self.injection_targets.len())
        {
            Some(index) => self.attach(index, cx),
            None => cx.notify(),
        }
    }

    /// A press on one window row: it becomes the target and is gated at once.
    ///
    /// The gate is the press's gate, so a row that cannot be injected says which rule refused it
    /// now rather than failing later.
    pub fn select_injection_target(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.injection_targets.len() {
            return;
        }
        self.injection_selected = Some(index);
        self.attach(index, cx);
    }

    /// Gates one target, attaches the bridge, and only then starts the reader.
    ///
    /// The order is managed's and it is what makes a failure arrive before anything touches the
    /// game: the gate first, so a window the rules refuse never reaches the injector; then
    /// [`aoko_platform::inject_bridge`], which resolves the bridge library for the target's version
    /// beside the host's executable and loads it into its pid; then the dial, because the bridge the
    /// injector just started is what the reader connects to. A refusal or a failed attach therefore
    /// leaves the reader untouched — which is the assertion the press's tests make, because a dial
    /// is the one observable effect [`Shell::dial`] counts.
    ///
    /// The version is read off the window's title with the platform's own parser: a `WindowInfo`
    /// carries a window, not a launcher command line, so the title is the whole of the evidence this
    /// half has — `aoko_platform::describe_target` reads the same three inputs and normalises the
    /// title for exactly this case.
    fn attach(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(target) = self.injection_targets.get(index) else {
            return;
        };

        if let Err(refusal) = injection_target_gate(target) {
            self.injection = InjectionStatus::Refused(refusal);
            cx.notify();
            return;
        }

        // The list holds only windows that name their owner (`target_is_candidate`), so this case is
        // unreachable rather than a silent press: a window with no pid has nothing to load into.
        let Some(pid) = target.pid else {
            return;
        };
        let version = aoko_platform::normalize_detected_version(&target.title);
        let inject = self.inject_bridge.clone();

        if let Some(inject) = inject {
            if let Err(refusal) = inject(pid, version) {
                // A state, not a panic: the platform refused, and the sentence is the one the panel
                // owes — the copy line naming the resolved path for a missing library, the
                // platform's own only where there is nothing more useful to say. The press stays
                // pressable, so a user who drops the library beside the executable retries without
                // restarting the host.
                self.injection = InjectionStatus::Failed(refusal.sentence());
                cx.notify();
                return;
            }
        }

        self.dial();
        self.injection = InjectionStatus::Connecting;
        cx.notify();
    }

    /// Starts the bridge reader for an accepted target — the `Connecting` transition's only real
    /// effect.
    ///
    /// [`Runtime::connect_bridge`] is called in both builds, because the attempt it records is what
    /// the deadline is measured from: a test that skipped it would have nothing to time out. What
    /// differs is the dial itself, and it is not cosmetic. The shipped build spawns the reader on a
    /// worker and dials the loopback port. A test must never dial: `crate::ingest`'s header pins
    /// that every test attaches a memory duplex instead, and a bridge genuinely listening on the
    /// machine running the suite would flip the controller's `connected` under an assertion — so
    /// `Ingest::spawn` returns a reader with no worker under `cfg!(test)`, and this call counts the
    /// press, which is what lets a press prove it got past the gate without a socket.
    ///
    /// The dial is not target-specific: the endpoint is the constant loopback port whoever was
    /// chosen, which is why one reader per attempt is enough for a list of windows.
    fn dial(&mut self) {
        #[cfg(test)]
        DIALS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.runtime.connect_bridge();
    }

    /// The loader panel, `LoaderPanel` (`MainWindow.xaml:306-340`), in its own document order.
    ///
    /// Geometry is the markup's: heading 16 bold with 10 below, versions 11 dim with 10, the
    /// `DarkButton` at height 35 with 8, the autodetect sentence at 10 with 10, the status line at
    /// 12 (accent once connected, `MainWindow.xaml.cs:1528-1529`). The progress bar (`:334-338`)
    /// is deliberately absent: it tracks `GameStateClient.InjectionProgress`, and this port's
    /// attach has no progress to report — it is one dial on the worker thread.
    ///
    /// The window list is the port's replacement for `WindowPickerDialog`
    /// (`MainWindow.xaml.cs:1470-1486`): managed opened a modal when more than one client was
    /// running, and the port lists the same targets inside the panel. Recorded as a deviation;
    /// the rows come from the enumeration and a press on one feeds the same gate as the button.
    fn render_loader(&self, theme: &Theme, entity: &Entity<Self>) -> AnyElement {
        let view = self.injection_view();
        let handle = entity.clone();
        let inject = components::button(
            "loader-inject",
            theme,
            ButtonVariant::Dark,
            view.button,
            move |_, _window, cx| {
                handle.update(cx, |shell, cx| shell.inject(cx));
            },
        );

        let mut targets = div().flex().flex_col().items_center().gap(px(2.0));
        for (index, target) in self.injection_targets.iter().enumerate() {
            let selected = self.injection_selected == Some(index);
            let handle = entity.clone();
            targets = targets.child(
                div()
                    .id(crate::reuse::indexed("loader-target", index))
                    .debug_selector(move || format!("loader-target-{index}"))
                    .px(px(4.0))
                    .py(px(2.0))
                    .text_size(px(11.0))
                    .text_color(if selected {
                        theme.accent
                    } else {
                        theme.dim_text
                    })
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                        handle.update(cx, |shell, cx| shell.select_injection_target(index, cx));
                    })
                    .child(SharedString::from(target.display_label())),
            );
        }

        // `WindowPickerDialog.xaml:28`: the managed sentence above a multi-target list. Only the
        // modal's frame is not ported — the sentence that explains the choice is, and it shows in
        // exactly the state the modal opened in.
        let mut list = div().flex().flex_col().items_center().gap(px(4.0));
        if self.injection_targets.len() > 1 {
            list = list.child(
                div()
                    .max_w(px(520.0))
                    .text_size(px(11.0))
                    .text_color(theme.dim_text)
                    .text_center()
                    .debug_selector(|| "loader-targets-prompt".to_owned())
                    .child(copy::LOADER_TARGETS_PROMPT),
            );
        }
        list = list.child(targets);

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px(16.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.text)
                    .mb(px(10.0))
                    .child(copy::LOADER_HEADING),
            )
            .child(
                div()
                    .max_w(px(520.0))
                    .text_size(px(11.0))
                    .text_color(theme.dim_text)
                    .text_center()
                    .mb(px(10.0))
                    .child(copy::LOADER_SUPPORTED_VERSIONS),
            )
            // Named so a test can press it: `components::button` carries the id but no debug
            // selector, and the press is the panel's only control worth addressing.
            .child(
                div()
                    .mb(px(8.0))
                    .debug_selector(|| "loader-inject".to_owned())
                    .child(inject),
            )
            .child(
                div()
                    .max_w(px(520.0))
                    .text_size(px(10.0))
                    .text_color(theme.dim_text)
                    .text_center()
                    .mb(px(10.0))
                    .child(copy::LOADER_AUTODETECT),
            )
            .child(
                div()
                    .debug_selector(|| "loader-status".to_owned())
                    .text_size(px(12.0))
                    .text_color(if view.accent {
                        theme.accent
                    } else {
                        theme.dim_text
                    })
                    .text_center()
                    .child(SharedString::from(view.status)),
            )
            .child(div().mt(px(8.0)).child(list))
            .into_any_element()
    }

    /// The managed two-state swap's latch (`MainWindow.xaml.cs:469-492`), run once per frame.
    ///
    /// Managed left the loader on the tick the bridge answered (`:524-528`) — the same tick it
    /// wrote `Status: Connected & Injected` into the panel it was about to hide. The port runs the
    /// latch at the *end* of the frame for that reason: the frame that first observes the
    /// connection is the loader's last, painting the connected state managed also set, and the
    /// next frame is the control panel. Latching at the start instead would make the terminal
    /// state unreachable, which is a state the managed panel really draws (`:1528`).
    ///
    /// The latch is one-way: a later disconnect does not bring the loader back, which is what
    /// `:524-528` pins. `connected` is read from the controller and never stored; the field this
    /// flips is the latch, not the fact.
    fn latch_control_mode(&mut self) {
        if self.loader_visible && self.runtime.ingest_status().connected {
            self.loader_visible = false;
        }
    }

    /// One palette-editor row: the caption, the swatch, and the hex box beside it.
    ///
    /// `MainWindow.xaml:1962` is the swatch's geometry — 22×22, radius 3, a 1 px `#40FFFFFF`
    /// edge — and the edge is [`crate::theme::fixed::CONTRAST_LABEL`] at alpha `0x40` rather than
    /// a new colour literal. The hand cursor and the managed "Click to pick a color" tooltip are
    /// the affordance the press now answers: a swatch press opens
    /// [`ColorPickerView`] on this row's colour ([`Shell::open_color_picker`]).
    fn render_color_field(
        &self,
        label: &'static str,
        bind: &'static str,
        theme: &Theme,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let swatch = self
            .swatches
            .get(bind)
            .copied()
            .unwrap_or(Color::TRANSPARENT);
        let row_selector = format!("palette-row-{bind}");
        let swatch_selector = format!("palette-swatch-{bind}");
        let handle = entity.clone();

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
                            .debug_selector(move || swatch_selector)
                            .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                                handle.update(cx, |shell, cx| shell.open_color_picker(bind, cx));
                            }),
                    )
                    .child(div().flex_1().min_w(px(0.0)).child(field)),
            )
            .into_any_element()
    }

    /// The saved palettes, one card each (`MainWindow.xaml:1910-1937`).
    ///
    /// The template's numbers are [`metrics::palette_card`]'s: a 180 px card at radius 6 with 8×6 of
    /// padding, the name at 12 px, and the four 34×12 swatches 4 px apart. The data is the store's —
    /// the name and the four roles [`palette_store::PaletteRow`] carries — so nothing here derives a
    /// shade.
    fn render_palette_rows(
        &self,
        theme: &Theme,
        entity: &Entity<Self>,
        delete_tooltip: &'static str,
        apply_tooltip: &'static str,
    ) -> AnyElement {
        let mut cards = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(metrics::palette_card::GAP));

        for (index, row) in self.palette_rows.iter().flatten().enumerate() {
            let mut swatches = div()
                .flex()
                .flex_row()
                .mt(px(metrics::palette_card::PREVIEW_GAP));
            for color in [row.background, row.panel, row.slider, row.accent] {
                swatches = swatches.child(
                    div()
                        .w(px(metrics::palette_card::SWATCH_WIDTH))
                        .h(px(metrics::palette_card::SWATCH_HEIGHT))
                        .mr(px(metrics::palette_card::SWATCH_GAP))
                        .rounded(px(metrics::palette_card::SWATCH_RADIUS))
                        .bg(color),
                );
            }

            // The cap is its own control, so one press cannot both ask to remove the row and apply
            // it: `stop_propagation` is what keeps the two apart.
            let remove = {
                let handle = entity.clone();
                let name = row.name.clone();
                div()
                    .id(crate::reuse::indexed("saved-palette-delete", index))
                    .debug_selector(move || format!("saved-palette-delete-{index}"))
                    .w(px(metrics::palette_card::DELETE))
                    .h(px(metrics::palette_card::DELETE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(metrics::palette_card::SWATCH_RADIUS))
                    .cursor_pointer()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .tooltip(components::tooltip(theme, delete_tooltip))
                    .on_click(move |_, _, cx| {
                        let name = name.clone();
                        handle.update(cx, |shell, cx| shell.palette_request_delete(&name, cx));
                        cx.stop_propagation();
                    })
                    .child(copy::REMOVE_GLYPH)
            };

            // The card is the apply button (`MainWindow.xaml:1926` — the card itself is the click
            // target, which is what the row's tooltip says), so the press is on the card rather than
            // on a button inside it.
            let handle = entity.clone();
            let name = row.name.clone();
            cards = cards.child(
                div()
                    .id(crate::reuse::indexed("saved-palette", index))
                    .debug_selector(move || format!("saved-palette-{index}"))
                    .w(px(metrics::palette_card::WIDTH))
                    .rounded(px(metrics::palette_card::RADIUS))
                    .border_1()
                    .border_color(theme.card_border())
                    .bg(theme.panel)
                    .px(px(metrics::palette_card::PAD_X))
                    .py(px(metrics::palette_card::PAD_Y))
                    .cursor_pointer()
                    .tooltip(components::tooltip(theme, apply_tooltip))
                    .on_click(move |_, _, cx| {
                        let name = name.clone();
                        handle.update(cx, |shell, cx| shell.apply_palette(&name, cx));
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(metrics::palette_card::NAME_TEXT))
                                    .text_color(theme.text)
                                    .child(SharedString::from(row.name.clone())),
                            )
                            .child(div().ml_auto().child(remove)),
                    )
                    .child(swatches),
            );
        }

        // The live-palette line, `MainWindow.xaml:1904`: a `Run` inside the sentence with the name
        // after it, which is why the cached text is the prefix and the name together rather than two
        // children. Above the cards because that is where the markup puts it, and the empty string
        // before the first read paints no line rather than a lone colon.
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(metrics::palette_card::PREVIEW_GAP))
            .child(
                div()
                    .debug_selector(|| String::from("palette-current"))
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(match &self.palette_current {
                        Some(line) => SharedString::clone(line),
                        None => SharedString::from(""),
                    }),
            );

        // The two questions a press can leave open, both asked in place and both answered by a
        // second press: the remove confirmation, and the overwrite one `save_draft` arms. Managed
        // asked in a `MessageBox`; the configs card already proves the in-card shape in this tree.
        if let Some(name) = self.palette.delete_armed() {
            let prompt = palette_store::PaletteStore::delete_confirmation(name).0;
            let handle = entity.clone();
            let confirm = components::button(
                "palette-delete-confirm",
                theme,
                ButtonVariant::Danger,
                "Delete",
                move |_, _, cx| handle.update(cx, |shell, cx| shell.palette_confirm_delete(cx)),
            );
            let handle = entity.clone();
            let cancel = components::button(
                "palette-delete-cancel",
                theme,
                ButtonVariant::Secondary,
                "Cancel",
                move |_, _, cx| handle.update(cx, |shell, cx| shell.palette_cancel_delete(cx)),
            );
            column = column.child(
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
                            .child(SharedString::from(prompt)),
                    )
                    .child(confirm)
                    .child(cancel),
            );
        }

        if let Some(name) = self.palette.overwrite_pending() {
            let prompt = copy::PALETTE_OVERWRITE_PROMPT.replace("{name}", name);
            let handle = entity.clone();
            let confirm = components::button(
                "palette-overwrite-confirm",
                theme,
                ButtonVariant::Danger,
                "Overwrite",
                move |_, _, cx| handle.update(cx, |shell, cx| shell.palette_confirm_overwrite(cx)),
            );
            let handle = entity.clone();
            let cancel = components::button(
                "palette-overwrite-cancel",
                theme,
                ButtonVariant::Secondary,
                "Cancel",
                move |_, _, cx| handle.update(cx, |shell, cx| shell.palette_cancel_overwrite(cx)),
            );
            column = column.child(
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
                            .child(SharedString::from(prompt)),
                    )
                    .child(confirm)
                    .child(cancel),
            );
        }

        // The store's own sentence for the last press that had one. No row when there is nothing to
        // say: a card that always paints a line would have to paint an empty one.
        if let Some(status) = &self.palette_status {
            column = column.child(
                div()
                    .debug_selector(|| String::from("palette-status"))
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(status.clone()),
            );
        }

        column.child(cards).into_any_element()
    }

    /// The create box's name row (`MainWindow.xaml:1949-1956`): the caption, then the box.
    ///
    /// The box is the shell's entity ([`Shell::ensure_palette_name`]), created before the tree for
    /// the same reason every other field is; a frame that reaches here without one paints the
    /// caption alone rather than a box that cannot take focus.
    fn render_palette_name_box(&self, label: &'static str, theme: &Theme) -> AnyElement {
        let field: AnyElement = match &self.palette_name {
            Some(field) => div()
                .debug_selector(|| String::from("palette-name-box"))
                .child(field.clone())
                .into_any_element(),
            None => div().into_any_element(),
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(label),
            )
            .child(field)
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
    /// the line away is how the row reads the explanatory copy again.
    ///
    /// The label is the other half, and it is repaired by the *next frame* rather than here:
    /// [`Shell::ensure_capture`] rebuilds a button that has stopped listening and is still
    /// painting its pending line. This callback runs inside the capture entity's own update, so
    /// dropping its entity from here would be the one place the file forbids — the rebuild has to
    /// happen after the update unwinds, which is exactly what the frame boundary gives.
    fn clear_capture_status(&mut self, target: KeyTarget, cx: &mut Context<Self>) {
        match target {
            KeyTarget::Module(_) => self.keybind_conflict = None,
            KeyTarget::Action(which) => self.action_status[status_slot(which)] = None,
        }
        cx.notify();
    }

    /// Ends a capture whose button no longer holds the keyboard.
    ///
    /// The component listens for its own blur and reports `Abandoned` through
    /// [`Shell::clear_capture_status`], and that is the ordinary path. This is the invariant
    /// beside it, for the case that path cannot see: `Window::blur` only clears `Window::focus`
    /// (`window.rs:2785-2800`), the blur is delivered by the next frame's listener dispatch, and
    /// that dispatch compares the previous and current focus *paths* — so a focus move made during
    /// the same frame (the root reclaiming itself, a modal closing) can leave the listener without
    /// the event it needs. A capture still listening with no focus is orphaned either way: it
    /// cannot receive a key, so nothing it does can ever be reported.
    ///
    /// The button is replaced rather than told to cancel. `cancel` reports through this shell's
    /// own callback, and that callback would re-enter this update; replacing the entity from the
    /// frame is safe because the frame is not inside its update. Nothing is stored either way,
    /// which is the property that matters: focus loss is not a key.
    ///
    /// Allocation-free on an idle frame: the scan clones nothing until a capture is stale.
    fn abandon_unfocused_captures(&mut self, window: &Window, cx: &mut Context<Self>) {
        let stale: Vec<KeyTarget> = self
            .captures
            .iter()
            .filter(|(_, held)| {
                let capture = held.capture.read(cx);
                capture.is_capturing() && !capture.focus_handle().is_focused(window)
            })
            .map(|(target, _)| *target)
            .collect();

        for target in stale {
            match target {
                KeyTarget::Module(_) => self.keybind_conflict = None,
                KeyTarget::Action(which) => self.action_status[status_slot(which)] = None,
            }
            self.captures.remove(&target);
            self.ensure_capture(target, cx);
        }
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
    ///
    /// This is the state half only; [`Shell::show_tab`] is the half that also has a window and can
    /// drop the keyboard a hidden field was holding. Every user-visible tab change goes through
    /// that one — the rail is the only control that switches tabs — while this stays for callers
    /// with no window (a jump, a test setting a scene).
    pub fn select_tab_named(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(index) = copy::TABS
            .iter()
            .position(|tab| tab.name.eq_ignore_ascii_case(name))
        {
            self.select_tab(index, cx);
        }
    }

    /// Shows one tab from a window, and drops the keyboard from a field the new body hides.
    ///
    /// The rail is the only control that changes tabs, so this is the choke point for a user-visible
    /// switch. A text field that lived in the old body has no node in the new frame, so it receives
    /// nothing — every character falls back to the dispatch tree's synthetic root — while
    /// `Window::focused` keeps reporting it, which also keeps `Shell::window_focus` from taking
    /// the keyboard back. The symptom is a window whose Ctrl+K is dead until the user clicks a
    /// field.
    ///
    /// The fix belongs here rather than in a broader "reclaim focus when the focused node is gone"
    /// rule, because that rule cannot tell a hidden field from a field that is about to be painted:
    /// `overlay::focus_query` focuses the query box one frame before its node exists, and a rule
    /// that swept stale handles would take the keyboard straight back out of it.
    pub fn show_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab(index, cx);

        if self.tab_text_field_focused(window, cx) {
            window.blur(cx);
            cx.notify();
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
    /// The loader's status line reads it — `ingest_status().connected` for whether the link is up
    /// and `link_failure()` for why it is not — and nothing else in this view draws it yet.
    #[must_use]
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// The runtime, mutably, which is how a test attaches a reader without a socket.
    ///
    /// Same seam as [`Shell::set_runtime`], narrower: a test that has already built its shell keeps
    /// the runtime it was given and brings a link up on it, rather than replacing the owner the
    /// shell's writes already go to.
    #[cfg(test)]
    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    /// A control value: the session override, else the profile, else `default`.
    ///
    /// The override only exists for a value the session set on *this* profile; a wholesale
    /// replacement drops it, so a load cannot leave the panel painting a value the profile no
    /// longer holds. See `Shell::adopt_profile`.
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
    ///
    /// No `Window`: the one caller is `views::stats_view`'s reset button, whose callback receives a
    /// window and lets it go. There is no second entry point — the Stats tab is `TabBody::Stats`, so
    /// its `StatsActionsCard` table entry never paints and the action table carries no arm for it.
    pub fn reset_stats(&mut self, cx: &mut Context<Self>) {
        if self.runtime.reset_stats() {
            cx.notify();
        }
    }

    /// The command-search state.
    ///
    /// Mounted now: `Ctrl+K`, a typed character and the rail button open it, the root listener
    /// handles Enter and Escape, and `Shell::render_search` paints
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
        // The controller's profile, as in [`Shell::save_profile`]: `create` writes a file, and a
        // second write site that saved the view's copy would forget every switch the user closed.
        let mut profile = self.controller().profile_for_save();
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
        // The controller's profile again: the overwrite path writes the same file, so it is the same
        // rule — see [`Shell::save_profile`].
        let mut profile = self.controller().profile_for_save();
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

    /// The HUD editor's Reset layout (`ResetHudLayoutButton_Click`,
    /// `MainWindow.xaml.cs:1411-1415`).
    ///
    /// Managed reset the live `HudLayout` and then saved the profile it built from the clicker, so
    /// the press writes the file rather than leaving a pending edit. The reset itself is
    /// [`aoko_protocol::hud::HudLayout::reset_all`]'s, not a table of anchors here: the canonical
    /// layout is the protocol's, and a copy would drift from the one the bridge parses.
    ///
    /// What is stored is the *stored* form of the reset layout — `Profile::hud_layout_map` of the
    /// canonical layout — which is what `Profile::to_hud_layout` reads back, so a save and a
    /// reload agree with what the frame carries.
    pub fn reset_hud_layout(&mut self, cx: &mut Context<Self>) {
        let mut layout = self.profile.to_hud_layout();
        layout.reset_all();
        self.profile.hud_layout = Some(aoko_protocol::profile::Profile::hud_layout_map(&layout));
        // The press is also the save (`:1414`); a failure leaves the reset in place and the
        // profile store reports it, which is the same shape the config saves use.
        let _ = self.save_profile();
        // The frame carries the layout half, so the bridge has to see the reset now, not on the
        // next edit.
        self.push_profile();
        cx.notify();
    }

    /// The Reload Mappings press (`ReloadMappingsButton_Click`, `MainWindow.xaml.cs:1397-1404`).
    ///
    /// Managed's whole handler is two lines: return unless the bridge advertises
    /// `reloadMappingsNonce`, then `RequestBridgeMappingReload`. The ask is the nonce — it is a field
    /// of the `config` frame, and a value the bridge has not seen is its signal to re-read — so
    /// nothing here names a message, and the gate is the capability the managed guard read.
    ///
    /// An unsupported press changes nothing, which is also managed's behaviour: the early return is
    /// the answer rather than a swallowed error, and the button has no disabled state at the pin to
    /// show it with. The frame is rebuilt and sent by `ingest`'s `sync_config`, which compares the
    /// controller's frame against the last one on the wire, so the press needs no send of its own.
    pub fn reload_mappings(&mut self, cx: &mut Context<Self>) {
        if !self.runtime.setting_supported("reloadMappingsNonce") {
            return;
        }
        let _nonce = self.runtime.controller().request_mapping_reload();
        cx.notify();
    }

    /// The Panic press (`PanicButton_Click`, `MainWindow.xaml.cs:1393-1396`).
    ///
    /// `Clicker.TriggerPanicAsync` (`Clicker.cs:536-609`) is the work: every module but the left
    /// clicker off, the overlay, logo and Discord flags cleared, then the link dropped and the app
    /// shut down. The reset is [`AppController::panic_reset`]'s — the switches are the controller's
    /// state, and a second table here would be the copy this branch keeps deleting.
    ///
    /// The exit is [`Shell::close`], the same two steps the title bar's close runs, so a panic and a
    /// normal exit cannot diverge. The profile is taken back from the controller first, because
    /// `close` saves *this* shell's copy and the reset changed the controller's: saving the
    /// pre-panic flags would persist exactly what the press exists to clear. Managed's shutdown ran
    /// the same `OnClosed` that saved a profile built from the panicked `Clicker`
    /// (`MainWindow.xaml.cs:1861-1876`), which is the same ordering.
    ///
    /// Two managed steps are recorded rather than faked. `EnterPanicStealthMode` hid the window and
    /// dropped it from the taskbar before the shutdown, which a quit makes invisible here; and the
    /// `await Task.Delay(250)` before `Disconnect()` gave the bridge a beat to take the config,
    /// where this port's quit closes the socket from under the reader — a sleep on the UI thread
    /// would freeze the frame the button was pressed in.
    pub fn panic(&mut self, cx: &mut Context<Self>) {
        self.runtime.controller().panic_reset();
        let reset = self.runtime.controller().profile();
        self.adopt_profile(reset);
        self.close(cx);
    }

    /// The window's close: save the profile, then quit.
    ///
    /// `CloseButton_Click` (`MainWindow.xaml.cs:1384-1392`) is `SaveProfile` then
    /// `Application.Current.Shutdown()`, and `OnClosed` (`:1861-1876`) saves again on the way out.
    /// One function for both exits — the title bar's button and [`Shell::panic`] — so the save and
    /// the quit cannot drift apart.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        let _ = self.save_profile();
        cx.quit();
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

    /// The Esc chip's click: the same close Escape runs, focus included.
    ///
    /// `CloseChip_Click` (`CommandSearchOverlay.xaml.cs:245-250`) called the overlay's own `Close`,
    /// which is why this is not a second rule — it is the `WindowKeyOutcome::Closed` half of
    /// [`Shell::handle_window_key`] with a pointer instead of a key. The blur is not optional: a box
    /// that keeps focus after its overlay is gone makes [`Shell::any_text_field_focused`] report a
    /// focused field for a field nobody can see, and Ctrl+K and type-to-open are refused for the
    /// rest of the session.
    fn close_search_at_pointer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_search(cx);
        self.blur_search_query(window, cx);
    }

    /// Mirrors the query box into the search state, which is the owner of the query.
    ///
    /// The field is the keyboard, not a second copy: every keystroke lands here and the state
    /// re-ranks from the same string, so the rows and the box cannot disagree. A text that does
    /// not change the query (an arrow key, the caret moving) returns before it rebuilds.
    fn set_search_query(&mut self, text: String, cx: &mut Context<Self>) {
        let facts = self.row_facts();
        let dev_mode = self.profile.dev_mode;
        if self.search.set_query(&text, dev_mode, &facts) {
            cx.notify();
        }
    }

    /// Seeds the query box from the state and focuses it: the overlay's open path.
    ///
    /// The seed is the state's query rather than the keystroke that opened it, so the type-to-open
    /// character and Ctrl+K's empty string take one route and the box cannot disagree with the
    /// rows the state already ranked. [`crate::overlay::focus_query`] is the managed
    /// `CommandSearchOverlay.Open` (`CommandSearchOverlay.xaml.cs:69-88`): seed, caret at the end,
    /// focus, in that order.
    fn focus_search_query(&self, window: &mut Window, cx: &mut App) {
        let Some(field) = &self.search_query else {
            return;
        };
        let seed = self.search.query().to_owned();
        crate::overlay::focus_query(field, &seed, window, cx);
    }

    /// Blurs the query box, which is the restore-focus half of [`SearchKeyOutcome::Closed`].
    ///
    /// Without it the box keeps the window's focus after the overlay is gone, and
    /// [`Shell::any_text_field_focused`] then reports a focused field for a field nobody can see:
    /// Ctrl+K and type-to-open would both be refused for the rest of the session.
    fn blur_search_query(&self, window: &mut Window, cx: &mut App) {
        let Some(field) = &self.search_query else {
            return;
        };
        if field.read(cx).focus_handle().is_focused(window) {
            window.blur(cx);
        }
    }

    /// One window keystroke, and the focus that goes with its outcome.
    ///
    /// The focus is applied here rather than at each return site, because every `Closed` is one —
    /// Escape, a scrim click's equivalent and the Enter that jumps all end the overlay — and a
    /// path that forgot to blur would leave the gate refusing Ctrl+K for good. A repaint follows
    /// only an outcome that changed something.
    fn handle_window_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> WindowKeyOutcome {
        let outcome = self.window_key_outcome(event, window, cx);
        match outcome {
            // `SearchKeyOutcome::Opened`'s contract: the caller focuses the query field.
            WindowKeyOutcome::Opened => self.focus_search_query(window, cx),
            // And `Closed`'s: it restores focus, which here means handing it back to the window.
            WindowKeyOutcome::Closed => self.blur_search_query(window, cx),
            WindowKeyOutcome::Consumed => cx.notify(),
            WindowKeyOutcome::Ignored => {}
        }
        outcome
    }

    /// The two controls that take a window key, in the order that decides which of them sees it.
    ///
    /// Every outcome but `Ignored` stops the key at the root listener, and the reason is easy to
    /// lose: GPUI's simulated input fills a `key_char` for `enter` (`"\n"`), `tab` (`"\t"`) and
    /// `space` (`" "`) before the event is dispatched (`gpui/src/window.rs:6178-6201`,
    /// `platform/keystroke.rs:268-289`), and hands that character to the platform input handler
    /// whenever the event still propagates. A branch that acted on Enter and answered `Ignored`
    /// therefore acted *and* typed the newline: the palette box committed `#0A0B0F` and then read
    /// `#0A0B0F\n`, and the add box typed `stone`, which normalises to nothing, so the append
    /// looked like it never happened. `Consumed` is the answer for a key the shell acted on.
    fn window_key_outcome(
        &mut self,
        event: &KeyDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> WindowKeyOutcome {
        // Escape cancels the colour picker. That is the managed `IsCancel="True"` on the dialog's
        // Cancel button (`ColorPickerDialog.xaml:109`): WPF closed the window with
        // `DialogResult = false`, so nothing was written and nothing was said. The dialog itself
        // has no key handler, so the window's Escape lands here, and closing is the whole of the
        // refusal — the row, its text and its memo are not touched.
        if self.picker.is_some() && event.keystroke.key.eq_ignore_ascii_case("escape") {
            self.picker = None;
            cx.notify();
            // Consumed: the picker answered the key, so nothing behind it may also see it.
            return WindowKeyOutcome::Consumed;
        }

        // Enter in the Block ESP add box submits. `components::TextInput` deliberately leaves
        // Tab, Enter and Escape to its owner (`text_input.rs:421`), so a focused box hands the
        // key up the tree and this listener is where it lands. It is checked before the overlay
        // because a box with focus is a box the user is typing in.
        if event.keystroke.key.eq_ignore_ascii_case("enter")
            && !self.search.is_open()
            && self.block_add_focused(window, cx)
        {
            self.add_block_target(cx);
            // Consumed, not `Ignored`: the submit happened, and `Ignored` would let
            // `dispatch_keystroke` insert Enter's simulated `"\n"` into the same box afterwards,
            // which makes `stone\n` normalise to nothing and the add look dropped.
            return WindowKeyOutcome::Consumed;
        }

        // Enter in a palette box commits its hex in the canonical `#RRGGBB` form. Same route,
        // same reason: the widget leaves Enter to its owner.
        if event.keystroke.key.eq_ignore_ascii_case("enter") && !self.search.is_open() {
            if let Some(bind) = self.color_field_focused(window, cx) {
                self.commit_color_text(bind, cx);
                // Consumed for the same reason as the add box: the commit happened, and Enter's
                // simulated `"\n"` must not be appended to the hex it just canonicalised.
                return WindowKeyOutcome::Consumed;
            }
        }

        // Enter jumps the selected row, and only while the overlay is open. It is not a
        // `SearchKey` because the managed `Jump` is a query-field key, not a window gesture.
        // The query row is painted rather than focusable (`overlay::query_row`), so the root
        // listener is what receives it.
        if self.search.is_open() && event.keystroke.key.eq_ignore_ascii_case("enter") {
            self.perform_jump(cx);
            return WindowKeyOutcome::Closed;
        }

        let Some(key) = search_key_of(event) else {
            return WindowKeyOutcome::Ignored;
        };

        let facts = self.row_facts();
        let dev_mode = self.profile.dev_mode;
        let gate = self.search_gate(Some(window), cx);
        self.search
            .handle_window_key(key, &gate, dev_mode, &facts)
            .into()
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

    /// Holds the shell's own focus when nothing else does, so the window keys have somewhere to
    /// land.
    ///
    /// Run at the top of every frame, and cheap: `Window::focus` returns immediately when this
    /// handle already has the keyboard (`window.rs:2770-2773`).
    ///
    /// The "when nothing else does" half is not politeness. A text field or a keybind capture that
    /// holds focus owns the keyboard, and pulling it back here would make the box the user is
    /// typing in stop receiving characters — the gate in [`Shell::search_gate`] would then say a
    /// text field is focused while nothing it types arrives. So this only fills the empty case.
    fn hold_window_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) -> FocusHandle {
        let handle = match &self.window_focus {
            Some(handle) => handle.clone(),
            None => {
                let handle = cx.focus_handle();
                self.window_focus = Some(handle.clone());
                handle
            }
        };

        if window.focused(cx).is_none() {
            window.focus(&handle, cx);
        }

        handle
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
            // A modal owns the keyboard while it is up. The picker's hex box is the dialog's own
            // entity, so it is not in [`Shell::any_text_field_focused`] — without this a
            // character typed into it would also be read as type-to-open, and the overlay would
            // open behind the dialog that is taking the keys.
            control_mode: self.picker.is_none()
                && !window.is_some_and(|window| self.any_text_field_focused(window, cx)),
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
    /// and the three that live on the shell instead (the Block ESP box, the palette boxes and the
    /// overlay's query box) are here for the same reason: a character typed into one must not
    /// open the search overlay. The query box matters twice over, because it is the one field
    /// that exists while the overlay is open, so the close path blurs it.
    fn any_text_field_focused(&self, window: &Window, cx: &App) -> bool {
        self.tab_text_field_focused(window, cx)
            || self
                .search_query
                .as_ref()
                .is_some_and(|field| field.read(cx).focus_handle().is_focused(window))
    }

    /// Whether a text field painted *inside a tab body* owns the keyboard.
    ///
    /// The same set as [`Shell::any_text_field_focused`] minus the overlay's query box, and the two
    /// differ for one reason each. The gate needs the query box, because a character typed into an
    /// open overlay must not be read as a second type-to-open. Tab changes need it *excluded*,
    /// because the overlay is not a tab: nothing a tab change does can hide that box, and blurring
    /// it here would drop the keyboard from a surface that is still up.
    fn tab_text_field_focused(&self, window: &Window, cx: &App) -> bool {
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

    /// The colour picker's host layer: the dialog's card, centred, over everything else.
    ///
    /// The dialog paints its own card and no backdrop, because managed opened it as a second
    /// window and modality came from WPF. Here it is a child of this window, so modality is this
    /// layer's job: it covers the window, centres the card, and takes the press — a modal that
    /// let a press through to a switch behind it would be modal only in name.
    ///
    /// It paints no backdrop. The scrim the search overlay uses is that overlay's; inventing a
    /// second one for a dialog that was never a scrim in managed would be a visual decision
    /// rather than a port, so the layer is transparent and reports that it is owed a look.
    fn render_picker(&self, view: Entity<ColorPickerView>) -> AnyElement {
        div()
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            // A press outside the card lands here and goes no further. Deeper listeners — the
            // card's buttons, the hex box — have already run by the time this bubbles up.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(view)
            .into_any_element()
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
        // A second handle because the two callbacks are separate `'static` closures: the row press
        // jumps, the chip closes, and neither can borrow the other's.
        let close = entity.clone();
        // The live field when it exists, the static query line otherwise. The two shapes are
        // `OverlayQuery`'s, so which one paints is this expression and nothing inside the overlay.
        let query: crate::overlay::OverlayQuery<'_> = match &self.search_query {
            Some(field) => (field, window.viewport_size()).into(),
            None => window.viewport_size().into(),
        };
        Some(crate::overlay::overlay(
            theme,
            &self.search,
            motion,
            query,
            move |index, _event, _window, cx| {
                handle.update(cx, |shell, cx| shell.jump_to_row(index, cx));
            },
            move |_event, window, cx| {
                close.update(cx, |shell, cx| shell.close_search_at_pointer(window, cx));
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
        // [`Shell::close`], which saves before quitting.
        self.ensure_close_hook(window, cx);

        // The window keys live on the root element, and a key event only reaches it when the
        // shell's own handle is on the focus path. This is what holds it whenever nothing else
        // does; see [`Shell::hold_window_focus`].
        let window_focus = self.hold_window_focus(window, cx);

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
        // A capture that is listening while its button holds no focus can never finish: it
        // receives no keys, and the blur that would end it is only delivered if GPUI's focus-path
        // comparison sees the path shrink (`window.rs:3944-3995`). The frame is where the
        // condition is checkable without depending on that. See
        // [`Shell::abandon_unfocused_captures`].
        self.abandon_unfocused_captures(window, cx);
        // The add-block box is an entity too, and for the same reason it is created here
        // rather than in `Shell::new`.
        self.ensure_block_add(cx);
        // And the four palette boxes, for the same reason.
        self.ensure_color_fields(cx);
        // The saved palettes are a file read, so it happens here once rather than in the paint,
        // and the create box's name field is an entity for the same reason as the boxes above.
        self.ensure_palette_rows();
        self.ensure_palette_name(cx);

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

        // The managed swap (`MainWindow.xaml:306`/`:344`, `MainWindow.xaml.cs:469-492`) is latched
        // at the *end* of the frame, not here — see [`Shell::latch_control_mode`]: the frame that
        // first sees the connection is still the loader, painting its terminal state.
        //
        // The tab body is built only in control mode. The loader paints neither the rail nor a
        // tab, so assembling them first would be work thrown away on every loader frame.
        let content = if self.loader_visible {
            self.render_loader(theme, &entity)
        } else {
            let body = match copy::TABS.get(self.tab).map(|tab| tab.body) {
                // The Stats tab draws `views::stats_view`, which reads the runtime's tracker.
                Some(TabBody::Stats) => {
                    let handle = cx.entity();
                    crate::views::stats_view::render(
                        &self.runtime.stats(),
                        theme,
                        self.tab,
                        move |_, _window, cx| {
                            handle.update(cx, |shell, cx| shell.reset_stats(cx));
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

            div()
                .flex()
                .flex_row()
                .flex_1()
                .min_h(px(0.0))
                .gap(px(metrics::rail::GAP))
                .child(self.render_rail(theme, &entity, &chip))
                .child(div().flex().flex_1().min_w(px(0.0)).child(body))
                .into_any_element()
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
            // The shell tracks its own focus handle so the listener below is on the key path even
            // when no control holds focus. GPUI starts a key event at `Window::focus` and walks up
            // to the dispatch tree's *synthetic* root, which carries no listeners
            // (`window.rs:7113-7121`, `key_dispatch.rs:611-614`): with nothing focused the walk
            // never passed through this element, so the window keys did nothing at all.
            // [`Shell::hold_window_focus`] is what keeps this handle focused.
            .id("shell-root")
            .track_focus(&window_focus)
            // The window-level keys, all of them: Ctrl+K, Escape, type-to-open and Enter. This is
            // the one place a keystroke is guaranteed to arrive: the root is a focusable element
            // and it holds the handle whenever no field does.
            .on_key_down({
                let handle = entity.clone();
                move |event, window, cx| {
                    let outcome =
                        handle.update(cx, |shell, cx| shell.handle_window_key(event, window, cx));
                    // Every outcome but `Ignored` stops the key here: the element below must not
                    // also act on it, and `Window::dispatch_keystroke` must not hand its simulated
                    // character to the input handler afterwards — Enter's is `"\n"`. See
                    // [`WindowKeyOutcome`].
                    if outcome.is_consumed() {
                        cx.stop_propagation();
                    }
                }
            })
            .child(self.render_title_bar(theme, &entity, &title))
            .child(content);

        // The overlay sits above the rail and the body and below the resize strips: a strip
        // the scrim swallowed would be a resize a user cannot perform, which is why this is
        // one child rather than a wrapper around the tree.
        if let Some(search) = self.render_search(theme, motion, &entity, window) {
            root = root.child(search);
        }

        // The colour picker, when a swatch press opened it. Same layer as the overlay and above
        // the strips for the same reason: a modal must not cost the user a resize edge.
        if let Some((_, view)) = &self.picker {
            let view = view.clone();
            root = root.child(self.render_picker(view));
        }

        // Decision 5's resize edges. They are last so they paint above the title bar
        // and the tab body, and each consumes its own mouse-down before anything
        // else can see it.
        for handle in resize_strips() {
            root = root.child(self.render_resize_handle(&handle, &entity));
        }

        // Last, so this frame is the loader's and the next is the control panel.
        self.latch_control_mode();

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
                            // (`MainWindow.xaml.cs:1384-1392`): save, then quit. The panic press
                            // ends in the same function, so the two exits cannot diverge.
                            handle.update(cx, |shell, cx| shell.close(cx));
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
                move |_, window, cx| {
                    handle.update(cx, |shell, cx| {
                        shell.open_search(None, cx);
                        // The click opens the same overlay Ctrl+K does, so it puts the caret in
                        // the same box: a press that left the keyboard nowhere would paint a
                        // query field that cannot be typed into.
                        shell.focus_search_query(window, cx);
                    });
                },
            )
            .into_any_element()
        });

        for (index, tab) in copy::TABS.iter().enumerate() {
            rail = rail.child(self.render_rail_item(index, tab.name, theme, entity));
        }

        // `CurrentVersionText` (`MainWindow.xaml:278`), above the footer, and the rail's one update
        // control. Managed's status half (`UpdateStatusText`, `:272`) belonged to a check this tree
        // cannot run, so what is left is the press that check existed to offer: the releases page.
        //
        // The line is the control rather than a button beside it, because there is no copy for a
        // button label that would not restate the version, and managed's own sentence for the state
        // whose press opened the page is the `click to view` tail — that tail is the affordance, so
        // it belongs on the thing that is pressed.
        rail = rail.child({
            let handle = entity.clone();
            div()
                .id("rail-update")
                .debug_selector(|| String::from("rail-version"))
                .px(px(12.0))
                .py(px(2.0))
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .cursor_pointer()
                .tooltip(components::tooltip(theme, copy::UPDATE_TOOLTIP))
                .on_click(move |_, _window, cx| {
                    handle.update(cx, |shell, cx| {
                        shell.open_release_page();
                        cx.notify();
                    });
                })
                .child(SharedString::from(Self::version_line()))
        });

        rail.child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(SharedString::clone(chip)),
        )
        .into_any_element()
    }

    /// The rail's version line, `CurrentVersionText` (`MainWindow.xaml:278`).
    ///
    /// The version is the running build's, from [`aoko_core::services::update::current_version`] —
    /// the module's port of `Assembly.GetName().Version?.ToString(3)` — and never a literal here, so
    /// a release bump changes this line and nothing else. The `{version}` placeholder is filled with
    /// `str::replace`, the convention this crate keeps for a template in copy.
    ///
    /// The sentence is [`copy::UPDATE_CURRENT_VIEW`]'s: managed drew the plain `Current vX.Y` in the
    /// two states whose press did nothing (checking, already current) and this tail only where the
    /// press opened the release page (`MainWindow.xaml.cs:1907`, `:1912`, `:1915`). With no fetcher
    /// the port is permanently in that last state, so the tail is the honest one — it is the only
    /// affordance saying a press does something.
    ///
    /// Computed once: the value cannot change while the process lives, and the rail paints every
    /// frame — a `format!` here would be one allocation per frame in the section-6 budget, which is
    /// the thing `perf.rs`'s steady-state pin counts.
    fn version_line() -> &'static str {
        static LINE: OnceLock<String> = OnceLock::new();
        LINE.get_or_init(|| {
            copy::UPDATE_CURRENT_VIEW
                .replace("{version}", &aoko_core::services::update::current_version())
        })
    }

    /// The version line's press: the releases page, `Process.Start` (`MainWindow.xaml.cs:1885`).
    ///
    /// A launcher that refuses is logged rather than painted: the rail has no status area, no copy
    /// owns a sentence for it, and the one refusal that actually happens — no `xdg-open` on `PATH` —
    /// is a session condition `probe_capabilities` reports for the folder button already. Inventing
    /// a line here would be the same class of mistake as an inert control, from the other side.
    fn open_release_page(&mut self) {
        let Some(open) = self.url_opener.clone() else {
            return;
        };
        if let Err(error) = open(aoko_core::services::update::RELEASES_URL) {
            tracing::warn!("could not open the releases page: {}", error.user_facing());
        }
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
            .on_click(move |_, window, cx| {
                // `show_tab`, not `select_tab`: the window is here, and a tab change is the one
                // moment a focused field can be hidden under the keyboard.
                handle.update(cx, |shell, cx| shell.show_tab(index, window, cx));
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
            // The card is wrapped in a `relative()` box so the pulse ring can sit on top of it
            // without taking part in its layout, and the wrapper carries the card's `x:Name` as
            // its selector: the ring's box is the card's box, so a test can compare the two and
            // say which card rang.
            //
            // `flex_col` is not decoration. `div()` is a row by default and the card sets no
            // width of its own: it has always stretched because it was a direct child of the
            // column the cards live in. A row wrapper would stop that stretch and paint every
            // card as narrow as its widest control.
            body = body.child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .debug_selector(move || card.id.to_owned())
                    .child(self.render_card(card, theme, motion, entity))
                    .children(crate::overlay::card_pulse_ring(
                        theme,
                        &self.search,
                        motion,
                        card.id,
                    )),
            );
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
                // Configs Folder (`:969-985`). The three the review named as inert are wired here
                // too, each to the one call managed's handler made, so a rendered card button is
                // never a decoration.
                let action: fn(&mut Shell, &mut Context<Shell>) = match (card.id, *label) {
                    ("ConfigsCard", "Load") => Shell::configs_load,
                    ("ConfigsCard", "Save") => Shell::configs_save,
                    ("ConfigsCard", "Delete") => Shell::configs_request_delete,
                    ("ConfigsCard", "Create") => Shell::configs_create,
                    ("ConfigsCard", "Open Configs Folder") => Shell::configs_open_folder,
                    // `ResetHudLayoutButton_Click` (`:1411-1415`): the one card button whose action
                    // already existed under another name, so this is the wire-up the review asked
                    // for rather than a behaviour added here.
                    ("HudEditorCard", "Reset layout") => Shell::reset_hud_layout,
                    // `ReloadMappingsButton_Click` (`:1397-1404`): the bridge's own setting is the
                    // gate, and the nonce in the frame is the request.
                    ("ReloadMappingsCard", "Reload Mappings") => Shell::reload_mappings,
                    // `PanicButton_Click` (`:1393-1396`): the reset, then the exit the title bar's
                    // close runs. Not a reset-only button — managed's shutdown is half of it.
                    ("PanicCard", "Trigger Panic Now") => Shell::panic,
                    // `AddCustomPaletteButton_Click` (`MainWindow.xaml.cs:1095-1171`). The card's
                    // other presses are not in this table because they are not `Control::Button`s:
                    // a saved row is its own click target, and its cap is walked by
                    // `render_palette_rows`, which is also where the confirmation is painted.
                    ("GuiCustomizationCard", copy::PALETTE_ADD) => Shell::palette_add,
                    // The stats card is not wired here, on purpose: the Stats tab is
                    // `TabBody::Stats`, so `STATS` is never painted as cards and this table entry is
                    // unreachable. The reset the user presses is `views::stats_view`'s own button,
                    // whose callback calls [`Shell::reset_stats`] directly — one path to one
                    // function.
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

            Control::BlockEspRows => self.render_block_esp_rows(theme, entity),

            Control::BlockEspAdd { bind } => self.render_block_add(bind, theme, entity),

            Control::ColorField { label, bind } => {
                self.render_color_field(label, bind, theme, entity)
            }

            // The saved palettes (`MainWindow.xaml:1910-1937`) and the name box (`:1949-1956`), with
            // every press the card advertises: apply on the card, remove on its cap through the
            // store's confirmation, and the line the store's own errors compose. The two tooltips
            // are the variant's because a `DataTemplate` row has no card entry to hang copy on.
            Control::PaletteRows {
                delete_tooltip,
                apply_tooltip,
            } => self.render_palette_rows(theme, entity, *delete_tooltip, *apply_tooltip),

            Control::PaletteNameBox { label } => self.render_palette_name_box(label, theme),

            Control::PalettePreview { name } => {
                let preview_accent = Theme::accent_by_name(name);
                // The card's own copy says what this press does — `Click a palette preview to apply
                // it instantly.` — so it is the same call the saved row makes, not a second path.
                let handle = entity.clone();
                let applied = *name;
                div()
                    .id(crate::reuse::stitched("palette-", "", name))
                    .debug_selector(move || format!("palette-preview-{applied}"))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.apply_palette(applied, cx));
                    })
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

    /// The tracked-block list: one [`BlockEspRow`] per entry in the profile.
    ///
    /// `MainWindow.xaml:692-715` is an `ItemsControl` over `BlockEspTargets` inside a
    /// `MaxHeight="170"` `ScrollViewer`, so the cap is the markup's and the order is the
    /// profile's: the managed collection is only ever appended to and removed from, never
    /// sorted, and a reorder here would repaint the rows in an order no saved profile records.
    ///
    /// The rows own nothing. Each control yields a [`RowAction`] naming its `registry_id` and
    /// [`Shell::apply_row_action`] is the one writer, because the profile is what the config
    /// frame is built from and a row that wrote itself would be a second owner of that list.
    ///
    /// The container carries the debug selector the rows do not: `components::block_esp` gives
    /// its controls `ElementId`s and no selectors, so the list's own bounds are the only proof
    /// from a `VisualTestContext` that the rows mounted at all — the failure shape that let a
    /// placeholder arm pass its tests while painting nothing.
    fn render_block_esp_rows(&self, theme: &Theme, entity: &Entity<Self>) -> AnyElement {
        let handle = entity.clone();
        let on_action = move |action: RowAction, _window: &mut Window, cx: &mut App| {
            handle.update(cx, |shell, cx| shell.apply_row_action(action, cx));
        };

        let mut list = components::scroll(crate::reuse::indexed("blockesp-rows", 0))
            .max_h(px(BLOCK_ESP_LIST_MAX_HEIGHT))
            .debug_selector(|| "blockesp-rows".to_owned());

        for target in &self.profile.block_esp_targets {
            list = list.child(
                BlockEspRow::new(
                    target.registry_id.clone(),
                    target.display_name.clone(),
                    target.color_hex.clone(),
                    target.enabled,
                )
                .element(theme, on_action.clone()),
            );
        }

        list.into_any_element()
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

/// The tracked-block list's cap: `MaxHeight="170"` on the `ScrollViewer` at
/// `MainWindow.xaml:692`.
///
/// Belongs in `theme::metrics::block_esp` beside the row pad and the swatch size, which is the
/// module that holds this panel's geometry; it sits here because that file is another builder's
/// and a listener is not worth a cross-file edit. The report names the move.
const BLOCK_ESP_LIST_MAX_HEIGHT: f32 = 170.0;

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

/// How many times a press reached the dial — how a test proves the press got past the gate without
/// opening a socket. See [`Shell::dial`].
///
/// Test-only, and a module static rather than a `Shell` field so the shipped struct carries nothing
/// only a test could read. Tests take a delta rather than an absolute, because the suite runs
/// tests in parallel.
#[cfg(test)]
static DIALS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The loader panel's state machine.
///
/// The managed states are the ones `MainWindow.xaml.cs` can actually draw: the panel starts at
/// `Status: Not Injected` (`:1464` restores it after a cancelled picker), goes to
/// `Status: Detecting Java clients...` (`:1438`), then `Status: Connecting...` (`:1490`), and
/// either reaches `Status: Connected & Injected` (`:1528`) or reports a failure (`:1539`).
///
/// `Connected` is deliberately **not** a variant here: whether injection landed is the
/// controller's answer, read every frame by [`InjectionView::resolve`], and a variant for it
/// would be the second copy this branch keeps deleting. What is left is the transient half the
/// controller has no opinion about — the press's own progress, an attach the platform would not
/// perform, and the port's refusals.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InjectionStatus {
    /// `Status: Not Injected` — at rest, before any press and after a refused one.
    #[default]
    NotInjected,
    /// `Status: Detecting Java clients...` — enumerating, and while a target is being chosen.
    Detecting,
    /// `Status: Connecting...` — the gate passed and the bridge reader is dialling.
    ///
    /// It has two exits and only one of them is a variant: the link coming up is read from the
    /// controller every frame, and a dial that fails is read from the runtime's
    /// [`crate::runtime::Runtime::link_failure`] — a reported failure, or an attempt that outlived
    /// [`crate::runtime::CONNECT_DEADLINE_MS`]. So `Connecting` is never a state the panel is
    /// stuck in, which is what [`InjectionView::resolve`] paints.
    Connecting,
    /// The platform would not attach the bridge, so the reader was never started.
    ///
    /// Holds the sentence the panel paints, which is one of two: for a library the resolver could
    /// not find, [`copy::LOADER_REASON_BRIDGE_LIBRARY_MISSING`] filled with the path it looked at —
    /// because that path is the thing a user acts on, and `PlatformError::user_facing` would put
    /// `Not found:` and the version-selection reasoning in front of it; for every other refusal, the
    /// platform's own sentence, because a permission or `Os` error names nothing this crate can say
    /// better. [`InjectRefusal`] is where that split happens, one layer before the paint.
    /// `String` rather than a `&'static str` rule label is that path, which is why this enum is not
    /// `Copy`: a refusal is a fixed rule, an attach failure is a place on disk.
    ///
    /// It is not the exit `Connecting` has: that one is a dialled attempt nobody answered, this one
    /// is an attach that never got as far as dialling. Both leave the button pressable, and that is
    /// the only thing a user can do about either.
    Failed(String),
    /// A press the platform's rules refused before anything was attempted.
    Refused(Refusal),
}

/// Which rule refused an Inject press.
///
/// The two gate rules are port-only: managed's `InjectButton_Click` trusted the discovery list
/// and had no gate to fail (`MainWindow.xaml.cs:1431-1467`), so there is no XAML line to cite for
/// their wording. The four sentences live in `views/copy.rs` with their managed analogues cited —
/// this enum names the rule, and [`Refusal::message`] is the one place the two meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The session cannot inspect windows, so a target cannot be confirmed at all.
    NoWindowSense,
    /// The enumeration found no window that could be injected into.
    NoWindow,
    /// [`aoko_platform::is_game_window`] said no.
    NotGameTitle,
    /// [`WindowInfo::is_jvm`] said no.
    NotJavaProcess,
}

impl Refusal {
    /// The sentence after [`copy::LOADER_STATUS_PREFIX`], naming the rule that refused.
    ///
    /// Read from `views/copy.rs` rather than written here: the copy owner's sentence is the one
    /// the panel paints, and a literal beside it would be a second copy of it.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::NoWindowSense => copy::LOADER_REFUSAL_NO_WINDOW_SENSE,
            Self::NoWindow => copy::LOADER_REFUSAL_NO_WINDOW,
            Self::NotGameTitle => copy::LOADER_REFUSAL_NOT_GAME_TITLE,
            Self::NotJavaProcess => copy::LOADER_REFUSAL_NOT_JAVA_PROCESS,
        }
    }
}

/// What the loader panel paints for one state, resolved from the machine and the link.
///
/// One function for the whole mapping, so the widget holds no condition of its own and a test can
/// assert every row of it without a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectionView {
    /// The status line, `Status: …`.
    pub status: String,
    /// The Inject button's label.
    pub button: &'static str,
    /// Whether the button takes a press.
    pub enabled: bool,
    /// Whether the status line is the accent one (`MainWindow.xaml.cs:1528-1529`) or the dim one.
    pub accent: bool,
}

impl InjectionView {
    /// `UpdateGameStateUI` (`MainWindow.xaml.cs:1510-1547`) as a value.
    ///
    /// The link state that wins over the machine: a connected controller paints the accent
    /// "Connected & Injected" and "Connected" on the button whatever the last press left behind,
    /// which is the managed order and the one-owner rule in one line.
    ///
    /// The two failures are read only while the machine is [`InjectionStatus::Connecting`], and
    /// that is the whole of their reach: a dial that never answered, and a link that went down
    /// after answering, are both "the press is over and here is why". A refused press stays the
    /// refusal even if an older attempt left a reason behind, because the rule that just refused is
    /// what a user has to act on.
    ///
    /// One managed behaviour is deliberately not reproduced. `:1543-1547` re-enables the button on
    /// any game-state tick where the bridge is disconnected and the client is not injected, so a
    /// button disabled for a press in flight flips back under the user's cursor as soon as a tick
    /// lands. The port keeps `Detecting`/`Connecting` disabled until the press resolves — where a
    /// *failure* is a resolution and does re-enable — and the test
    /// `a_press_in_flight_is_not_re_enabled_by_a_tick` pins that.
    #[must_use]
    pub fn resolve(state: &InjectionStatus, connected: bool, failure: Option<LinkFailure>) -> Self {
        if connected {
            return Self {
                status: copy::LOADER_STATUS_CONNECTED.to_owned(),
                button: copy::LOADER_INJECT_CONNECTED,
                enabled: false,
                accent: true,
            };
        }

        let (status, button, enabled) = match state {
            InjectionStatus::NotInjected => (
                copy::LOADER_STATUS_NOT_INJECTED.to_owned(),
                copy::LOADER_INJECT,
                true,
            ),
            InjectionStatus::Detecting => (
                copy::LOADER_STATUS_DETECTING.to_owned(),
                copy::LOADER_INJECT_DETECTING,
                false,
            ),
            // The one state with an exit the managed panel had and this one did not: `Connecting`
            // with no answer is not a state the machine can stay in, because either the reader
            // reports why the link is down or the dial's deadline does.
            InjectionStatus::Connecting => match failure {
                Some(failure) => (link_failure_line(failure), copy::LOADER_INJECT, true),
                None => (
                    copy::LOADER_STATUS_CONNECTING.to_owned(),
                    copy::LOADER_INJECT_CONNECTING,
                    false,
                ),
            },
            // The attach's own failure, in the same shape as the dial's: the panel's prefix plus
            // one sentence, and a pressable button. The sentence is the platform's — it names the
            // library path — which is why nothing is composed here but the prefix.
            InjectionStatus::Failed(sentence) => (
                format!("{}{}", copy::LOADER_STATUS_PREFIX, sentence),
                copy::LOADER_INJECT,
                true,
            ),
            InjectionStatus::Refused(refusal) => (
                format!("{}{}", copy::LOADER_STATUS_PREFIX, refusal.message()),
                copy::LOADER_INJECT,
                true,
            ),
        };

        Self {
            status,
            button,
            enabled,
            accent: false,
        }
    }
}

/// The status line for a dial's outcome: [`copy::LOADER_STATUS_PREFIX`] plus the reader's reason.
///
/// `{port}` is filled with `str::replace`, which is this crate's convention for a template in copy
/// (`copy::MODULE_BIND_CONFLICT_NAMED` does the same): the sentence is data owned by the copy
/// module, and `format!` would need a literal format string here, which is the second copy in
/// disguise. Neither sentence carries a `Status:` of its own — the prefix supplies it once — which
/// is why both are composed here rather than in the widget.
fn link_failure_line(failure: LinkFailure) -> String {
    let sentence = match failure {
        LinkFailure::ConnectFailed { port } => {
            copy::LOADER_REASON_CONNECT_FAILED.replace("{port}", &port.to_string())
        }
        LinkFailure::Disconnected => copy::LOADER_REASON_DISCONNECTED.to_owned(),
    };
    format!("{}{}", copy::LOADER_STATUS_PREFIX, sentence)
}

/// Whether one enumerated window belongs in the loader's list.
///
/// Managed's discovery lists Java processes (`InjectionTargetDiscovery.ListTargets`); this lists a
/// window that satisfies *either* half of the injection gate, so every row is a plausible target
/// and the press can name the half that is missing. A window that satisfies neither is not a
/// target at all and is not offered.
///
/// Neither is a window that names no owning process: the bridge is loaded into a pid, so a row
/// without one would be a press with nothing to attach to. Dropping it here rather than at the
/// press is what lets [`Shell::attach`] read the pid as a fact about every row it can see.
fn target_is_candidate(info: &WindowInfo) -> bool {
    info.pid.is_some() && (info.is_jvm() || aoko_platform::is_game_window(info))
}

/// The injection gate over one window: the pair [`crate::runtime::game_is_foreground`] composes
/// for the focused window, applied to the window the user chose.
///
/// Title first, then the launcher — the order `game_is_foreground` reads them in — so the two
/// shapes refuse for the same reason when both halves fail. The rules themselves are the
/// platform's (`is_game_window`, `is_jvm`); this composes, it does not re-implement.
fn injection_target_gate(info: &WindowInfo) -> Result<(), Refusal> {
    if !aoko_platform::is_game_window(info) {
        return Err(Refusal::NotGameTitle);
    }
    if !info.is_jvm() {
        return Err(Refusal::NotJavaProcess);
    }
    Ok(())
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

/// What the window's one key listener did with a keystroke.
///
/// [`SearchKeyOutcome`] answers for the overlay. The window has three other owners — the block
/// add box's Enter, a palette box's Enter and the colour picker's Escape — and the listener has to
/// say whether a key it acted on may travel on down the tree. That is not cosmetic:
/// `Window::dispatch_keystroke` (`gpui/src/window.rs:6178-6201`) gives `enter` a `key_char` of
/// `"\n"` through `keystroke.with_simulated_ime()` (`platform/keystroke.rs:268-289`) and hands it
/// to the platform input handler if the event still propagates, so a shell that consumed Enter
/// without stopping propagation also typed a newline into the very box it had just submitted.
/// `Consumed` is the answer that avoids it, and it is the same rule Tab already needs: a field
/// that does not stop Tab types a literal tab instead of moving focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowKeyOutcome {
    /// Nothing took it. The key travels on to the element that would have had it.
    Ignored,
    /// The shell acted on it. Nothing behind may see it, and its simulated character must not be
    /// inserted anywhere.
    Consumed,
    /// The overlay opened; the caller focuses the query field. The key stops here.
    Opened,
    /// The overlay closed; the caller restores focus. The key stops here.
    Closed,
}

impl WindowKeyOutcome {
    /// Whether the key stops at the window listener.
    fn is_consumed(self) -> bool {
        !matches!(self, Self::Ignored)
    }
}

impl From<SearchKeyOutcome> for WindowKeyOutcome {
    fn from(outcome: SearchKeyOutcome) -> Self {
        match outcome {
            SearchKeyOutcome::Opened => Self::Opened,
            SearchKeyOutcome::Closed => Self::Closed,
            SearchKeyOutcome::Ignored => Self::Ignored,
        }
    }
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

    /// A load replaces the profile, so the session override that described the old one goes.
    ///
    /// The round-trip test above writes through `apply_control`, which never touches
    /// [`Shell::draft`] — so it passed while the real drag path was broken: `set_value` writes the
    /// override, `value()` reads it first, and a load that only replaced `values` and `profile`
    /// left the panel painting the value the user last dragged and the next touch writing it back
    /// over the load. This drives the path a slider actually uses.
    #[gpui::test]
    fn a_load_drops_the_session_override_it_replaces(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let handle = cx.update(|cx| {
            cx.new(|_| Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root)))
        });

        // A saved config whose Min CPS is 9, saved the way a user saves one.
        cx.update(|cx| {
            handle.update(cx, |shell, cx| {
                shell.set_value("MinCPS", 9.0, cx);
                shell.save_config("PvP").expect("named save");
            })
        });

        // The session moves on: the slider is dragged to 20, which is an override on top of the
        // same profile and repaints.
        cx.update(|cx| handle.update(cx, |shell, cx| shell.set_value("MinCPS", 20.0, cx)));
        assert_eq!(
            cx.read(|cx| handle.read(cx).value("MinCPS", 0.0)),
            20.0,
            "the drag is what the panel paints before the load"
        );

        cx.update(|cx| {
            handle.update(cx, |shell, _cx| {
                shell.load_config("PvP").expect("named load");
            })
        });

        let (painted, stored, controller) = cx.read(|cx| {
            let shell = handle.read(cx);
            (
                shell.value("MinCPS", 0.0),
                shell.profile().min_cps,
                shell.controller().profile().min_cps,
            )
        });
        assert_eq!(
            painted, 9.0,
            "the slider paints the loaded value, not the override the load replaced"
        );
        assert_eq!(stored, 9.0, "and the profile is the loaded one");
        assert_eq!(
            controller, 9.0,
            "and the controller, which the config frame is built from"
        );
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

    /// Reset layout writes the canonical HUD layout into the profile, the file and the frame.
    ///
    /// `ResetHudLayoutButton_Click` (`MainWindow.xaml.cs:1411-1415`) reset the live layout and then
    /// saved the profile; the port has no live editor to reset, so the assertion is that the stored
    /// form, the saved profile and the layout half of the config frame all become canonical. The
    /// button's own press is [`Shell::reset_hud_layout`] — `components::button` gives it an
    /// `ElementId` and no debug selector, so a `VisualTestContext` has no bounds to click — which is
    /// the same shape the configs-card presses use.
    #[gpui::test]
    fn reset_layout_stores_the_canonical_hud_layout_and_sends_it(cx: &mut gpui::TestAppContext) {
        use aoko_protocol::hud::{element_id, HudElementLayout};

        let root = scratch_root();
        let handle = cx.update(|cx| {
            cx.new(|_| Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root)))
        });

        // A layout a user moved: one element well away from its canonical anchor.
        let moved = HudElementLayout::new(0.42, 0.13, 1.25);
        cx.update(|cx| {
            handle.update(cx, |shell, _| {
                let mut layout = shell.profile.to_hud_layout();
                layout.set(element_id::MODULE_LIST, moved);
                shell.profile.hud_layout =
                    Some(aoko_protocol::profile::Profile::hud_layout_map(&layout));
            })
        });
        assert_eq!(
            cx.read(|cx| handle
                .read(cx)
                .profile()
                .to_hud_layout()
                .get(element_id::MODULE_LIST)),
            moved,
            "the profile really holds a moved element before the press"
        );

        cx.update(|cx| handle.update(cx, |shell, cx| shell.reset_hud_layout(cx)));

        let (stored, frame) = cx.read(|cx| {
            let shell = handle.read(cx);
            (
                shell.profile().to_hud_layout(),
                shell.controller().config_frame().hud_layout.clone(),
            )
        });
        assert_eq!(
            stored,
            aoko_protocol::hud::HudLayout::new(),
            "the press reset every element, not just the one that moved"
        );
        assert_eq!(
            frame,
            aoko_protocol::hud::HudLayout::new(),
            "and the frame carries the layout the bridge parses, so the wire does not lag the file"
        );

        // The press is also the save (`:1414`): a fresh read of the same store is canonical.
        let (_, reloaded) =
            crate::views::profiles::Profiles::with_store(aoko_platform::ProfileStore::new(&root));
        assert_eq!(
            reloaded.to_hud_layout(),
            aoko_protocol::hud::HudLayout::new(),
            "the saved profile holds the reset layout"
        );
    }

    /// The panic press resets the state, saves it, and ends in the close path.
    ///
    /// `PanicButton_Click` (`MainWindow.xaml.cs:1393-1396`) is one call, and `TriggerPanicAsync`
    /// (`Clicker.cs:536-609`) is the work: every module but the left clicker off, the profile flags
    /// cleared, then the app shut down. `quit` is GPUI's, and the test platform's is a no-op, so what
    /// this pins is the half the port owns — the state the exit saves, and the fact that the press
    /// ends in [`Shell::close`], which is also what the title bar's button calls.
    #[gpui::test]
    fn the_panic_press_resets_the_state_and_saves_what_the_exit_would(
        cx: &mut gpui::TestAppContext,
    ) {
        use aoko_protocol::module_catalog::EnabledFlag;

        let root = scratch_root();
        let handle = cx.update(|cx| {
            cx.new(|_| Shell::with_store(Theme::slate(), aoko_platform::ProfileStore::new(&root)))
        });

        // A live state for the panic to clear, written through the shell so the controller, the
        // frame and the file all hold it before the press.
        cx.update(|cx| {
            handle.update(cx, |shell, _| {
                shell.profile.discord_rpc_enabled = true;
                shell.profile.show_module_list = true;
                shell.push_profile();
                let _ = shell.save_profile();
                shell
                    .runtime()
                    .controller()
                    .set_module_enabled(EnabledFlag::RightClick, true);
                shell
                    .runtime()
                    .controller()
                    .set_module_enabled(EnabledFlag::KillAura, true);
            })
        });

        cx.update(|cx| handle.update(cx, |shell, cx| shell.panic(cx)));

        let (modules_reset, left_clicker_on) = cx.read(|cx| {
            let controller = handle.read(cx).runtime().controller();
            (
                !controller.is_module_enabled(EnabledFlag::RightClick)
                    && !controller.is_module_enabled(EnabledFlag::KillAura),
                controller.is_module_enabled(EnabledFlag::LeftClick),
            )
        });
        assert!(modules_reset, "every module but the left clicker is off");
        assert!(
            left_clicker_on,
            "and the one switch a panic turns on is on (`Clicker.cs:543`)"
        );

        let adopted = cx.read(|cx| {
            let shell = handle.read(cx);
            (
                shell.profile.discord_rpc_enabled,
                shell.profile.show_module_list,
            )
        });
        assert_eq!(
            adopted,
            (false, false),
            "the shell's copy is the controller's, so the exit saves the reset rather than the flags"
        );

        // The save is `Shell::close`'s first step — the same one the title bar's close runs.
        let (_, reloaded) =
            crate::views::profiles::Profiles::with_store(aoko_platform::ProfileStore::new(&root));
        assert!(
            !reloaded.discord_rpc_enabled,
            "the exit's save wrote the cleared flag"
        );
        assert!(!reloaded.show_module_list, "and the other one");
    }

    /// The Stats card's reset clears the one tracker the runtime owns.
    ///
    /// `ResetStatsButton_Click` (`MainWindow.xaml.cs:1976-1979`) is `StatsTracker.Instance.Reset()`,
    /// so the press has to move the numbers the Stats tab draws and nothing else. The click is
    /// recorded through the same sink the automation loops hold, which is what makes this the real
    /// tracker rather than a second set of counters.
    #[gpui::test]
    fn the_stats_reset_press_clears_the_trackers_numbers(cx: &mut gpui::TestAppContext) {
        use crate::views::stats_view::StatsTexts;

        let handle = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        cx.update(|cx| {
            handle.update(cx, |shell, _| {
                let sink = shell.runtime().click_sink();
                sink.record_click(8.5, true);
                sink.record_click(11.0, false);
            })
        });
        assert_eq!(
            cx.read(|cx| handle.read(cx).runtime().stats().total_clicks()),
            2,
            "the clicks reached the tracker the tab reads"
        );

        cx.update(|cx| handle.update(cx, |shell, cx| shell.reset_stats(cx)));

        let (total, split) = cx.read(|cx| {
            let stats = handle.read(cx).runtime().stats();
            (stats.total_clicks(), StatsTexts::of(&stats).clicks_split)
        });
        assert_eq!(total, 0, "the press cleared the tracker");
        assert_eq!(split, "L 0  R 0", "and the split the tab paints with it");
    }

    /// Every button the shell wires by `(card id, label)` exists in the copy table under that pair.
    ///
    /// `Control::Button`'s action match keys on two strings that `views/copy.rs` owns, so a rename
    /// there would silently unwire a button instead of failing to compile — the failure mode the
    /// review found three of. This is that seam's own check; it is deliberately not a render test.
    ///
    /// The list is exactly the arms in `render_control`, which is why it caught `StatsActionsCard`:
    /// that entry is in the `STATS` table, but its tab is `TabBody::Stats`, so nothing paints it and
    /// the arm was a second path to `reset_stats` for a card that never renders. The reset the user
    /// presses is `views::stats_view`'s own button.
    #[test]
    fn every_wired_card_button_is_a_label_the_table_carries() {
        let wired = [
            ("ConfigsCard", "Load"),
            ("ConfigsCard", "Save"),
            ("ConfigsCard", "Delete"),
            ("ConfigsCard", "Create"),
            ("ConfigsCard", "Open Configs Folder"),
            ("HudEditorCard", "Reset layout"),
            ("ReloadMappingsCard", "Reload Mappings"),
            ("PanicCard", "Trigger Panic Now"),
        ];

        let cards: Vec<&Card> = copy::TABS
            .iter()
            .flat_map(|tab| match tab.body {
                TabBody::Cards(cards) => cards,
                _ => &[],
            })
            .collect();

        for (id, label) in wired {
            let card = cards
                .iter()
                .find(|card| card.id == id)
                .unwrap_or_else(|| panic!("{id} is a card in the table"));
            assert!(
                card.controls.iter().copied().any(|control| matches!(
                    control,
                    Control::Button { label: carried, .. } if carried == label
                )),
                "{id} carries a {label:?} button, so the wire is not stale"
            );
        }
    }

    /// Reload Mappings is the managed two lines: the bridge's setting, then the nonce.
    ///
    /// `ReloadMappingsButton_Click` (`MainWindow.xaml.cs:1397-1404`) returned unless the bridge
    /// advertised `reloadMappingsNonce`, then asked it to re-read. The ask is the `config` frame's
    /// nonce, so the observable is the frame the controller would send, and the capabilities frame
    /// is decoded by the real reader rather than poked into the cell — the gate is then the one a
    /// bridge trips, not a stub's.
    #[gpui::test]
    fn the_reload_mappings_press_needs_the_bridges_setting_and_bumps_the_nonce(
        cx: &mut gpui::TestAppContext,
    ) {
        let handle = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));

        let nonce = |cx: &mut gpui::TestAppContext| {
            cx.read(|cx| {
                handle
                    .read(cx)
                    .runtime()
                    .controller()
                    .mapping_reload_nonce()
            })
        };
        let advertised = |cx: &mut gpui::TestAppContext| {
            cx.read(|cx| {
                handle
                    .read(cx)
                    .runtime()
                    .setting_supported("reloadMappingsNonce")
            })
        };

        // The codec's fallback table does not name the nonce — neither did the managed fallback —
        // so this press is the early return, not a broken wire.
        assert!(!advertised(cx), "nothing has advertised the nonce yet");
        let before = nonce(cx);
        cx.update(|cx| handle.update(cx, |shell, cx| shell.reload_mappings(cx)));
        assert_eq!(
            nonce(cx),
            before,
            "an unsupported press does nothing, which is managed's `return`"
        );

        // A bridge that names it is what the managed guard was asking about. The frame is queued
        // first: the memory transport reports end of stream on an empty queue, so a reader attached
        // before the bytes would close the link instead of decoding them.
        let (transport, peer) = aoko_protocol::transport::memory::duplex();
        peer.feed(b"{\"type\":\"capabilities\",\"settings\":[\"reloadMappingsNonce\"]}\n");
        cx.update(|cx| {
            handle.update(cx, |shell, _| shell.runtime_mut().attach_ingest(transport));
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !advertised(cx) {
            assert!(
                std::time::Instant::now() < deadline,
                "the capabilities frame never reached the advertised cell"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        cx.update(|cx| handle.update(cx, |shell, cx| shell.reload_mappings(cx)));
        assert_eq!(
            nonce(cx),
            before + 1,
            "the press bumped the nonce, which is the whole request"
        );
        let line = cx.read(|cx| {
            handle
                .read(cx)
                .runtime()
                .controller()
                .config_frame()
                .to_line()
        });
        let wanted = format!("\"reloadMappingsNonce\":{}", before + 1);
        assert!(
            line.contains(wanted.as_str()),
            "and the frame the bridge reads carries the new value: {line}"
        );
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
    /// A module switch reaches the file, both ways round.
    ///
    /// The press writes the controller's flag set and never touches `Shell::profile`, so a save that
    /// wrote the view's copy wrote the state from before the press — a module switched on came back
    /// off after a restart. Both assertions read the file the save wrote rather than the shell's
    /// copy, because a test that read `self.profile` would pass against the defect.
    #[gpui::test]
    fn a_module_switch_reaches_the_saved_file(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (shell, _handle, _visual) = shell_in_window(cx, &root);

        // The call the switch's own handler runs — `clicking_the_module_switch_arms_the_module`
        // pins that a click reaches it — and then the real save the close path runs.
        cx.update(|cx| shell.update(cx, |shell, cx| shell.toggle_module("killaura", cx)));
        cx.update(|cx| shell.update(cx, |shell, _| drop(shell.save_profile())));

        let saved = read_auto_save(&root);
        assert_eq!(
            saved["killAuraEnabled"],
            serde_json::json!(true),
            "the switch the user closed is on in the file: {saved}"
        );

        // And the inverse, which a save that only ever wrote `true` would fail.
        cx.update(|cx| shell.update(cx, |shell, cx| shell.toggle_module("killaura", cx)));
        cx.update(|cx| shell.update(cx, |shell, _| drop(shell.save_profile())));

        let saved = read_auto_save(&root);
        assert_eq!(
            saved["killAuraEnabled"],
            serde_json::json!(false),
            "and a switch turned off is off in it: {saved}"
        );
    }

    /// The auto-save slot as the last save left it, parsed.
    fn read_auto_save(root: &std::path::Path) -> serde_json::Value {
        let json = aoko_platform::ProfileStore::new(root)
            .read_profile(crate::views::profiles::AUTO_SAVE_NAME)
            .expect("the save wrote the auto-save slot");
        serde_json::from_str(&json).expect("the profile file is JSON")
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

    /// Ctrl+K puts the caret in the query box, typing lands there, and Escape hands the keyboard
    /// back.
    ///
    /// The three halves are one test because they are one object: the box is seeded from the
    /// state on open, writes back to it on every character, and must not keep focus after the
    /// overlay closes — a box that kept it would report a focused text field forever and the gate
    /// would refuse Ctrl+K from then on.
    ///
    /// `text()` and the state's query are both asserted: the field must not become a second copy
    /// of the query, which is the defect this wiring exists to avoid.
    #[gpui::test]
    fn typing_reaches_the_query_box_and_the_state(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("ctrl-k").expect("keystroke"));
        // The overlay is painted on the next frame, and the box is only in the tree once it is.
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let focused = visual.update(|window, cx| {
            let shell = shell.read(cx);
            let field = shell
                .search_query
                .as_ref()
                .expect("the query box exists after the first frame");
            field.read(cx).focus_handle().is_focused(window)
        });
        assert!(
            focused,
            "the open path seeds the box and focuses it, which is `CommandSearchOverlay.Open`"
        );

        for character in ["a", "u", "t", "o"] {
            visual
                .cx
                .dispatch_keystroke(handle, Keystroke::parse(character).expect("keystroke"));
        }

        let (text, caret, query) = visual.update(|_, cx| {
            let shell = shell.read(cx);
            let field = shell.search_query.as_ref().expect("the box still exists");
            (
                field.read(cx).text().to_owned(),
                field.read(cx).edit().caret(),
                shell.search().query().to_owned(),
            )
        });
        assert_eq!(
            text, "auto",
            "the characters land in the field, from the caret the open left at its end"
        );
        assert_eq!(caret, 4, "and the caret stays after them");
        assert_eq!(
            query, "auto",
            "and the state ranks from the same string, so the rows and the box cannot disagree"
        );

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("escape").expect("keystroke"));

        let (open, still_focused) = visual.update(|window, cx| {
            let shell = shell.read(cx);
            let field = shell.search_query.as_ref().expect("the box still exists");
            (
                shell.search().is_open(),
                field.read(cx).focus_handle().is_focused(window),
            )
        });
        assert!(!open, "Escape closed the overlay");
        assert!(
            !still_focused,
            "and the box gave the keyboard back, so the gate can open the overlay again"
        );

        // The gate really did reopen: a typed character opens the overlay again, seeded with
        // itself and with the caret after it rather than before it. The seed here is the state's
        // query, so a caret at 0 would show the wrong half of `Open`'s three steps.
        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("e").expect("keystroke"));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let (open, text, caret, query) = visual.update(|_, cx| {
            let shell = shell.read(cx);
            let field = shell.search_query.as_ref().expect("the box still exists");
            (
                shell.search().is_open(),
                field.read(cx).text().to_owned(),
                field.read(cx).edit().caret(),
                shell.search().query().to_owned(),
            )
        });
        assert!(open, "type-to-open still works after the close");
        assert_eq!(text, "e", "and the box is seeded with the character");
        assert_eq!(caret, 1, "with the caret at the end of the seed");
        assert_eq!(
            query, "e",
            "and the state agrees, so the first frame ranks from it"
        );
    }

    /// The ring names one card, and it needs motion — with the pulse live in both halves.
    ///
    /// Two properties at the one function that decides them, with no paint and no global flag:
    /// the copy beside the landed card is not ringed, and `Motion::STILL` paints no ring even
    /// though the pulse is still running. The second half is what proves the gate rather than an
    /// empty state, which is the failure a `None` from a dead pulse would look like.
    ///
    /// The card is read from the catalogue rather than named here, so a rename in `copy.rs` is a
    /// test that fails on the missing card rather than a selector that silently stops matching.
    #[test]
    fn the_ring_names_one_card_and_needs_motion() {
        use crate::views::command_search::{pulse_card, JumpRequest};

        let theme = Theme::slate();
        let (entry, cards) = a_card_bearing_entry();
        let other = cards
            .iter()
            .find(|card| !card.id.eq_ignore_ascii_case(entry.card_name))
            .expect("the tab has a second card to not ring")
            .id;

        let mut state = CommandSearchState::new();
        let request = JumpRequest {
            entry,
            pulse_card: Some(entry.card_name),
        };
        assert!(
            state.begin_pulse(&request, 1_000, true).is_some(),
            "the pulse starts, so the motion half is not passing on an empty state"
        );
        assert_eq!(pulse_card(&state), Some(entry.card_name));

        assert!(
            crate::overlay::card_pulse_ring(&theme, &state, Motion::ANIMATED, entry.card_name)
                .is_some(),
            "the card the jump landed on gets the ring"
        );
        assert!(
            crate::overlay::card_pulse_ring(&theme, &state, Motion::ANIMATED, other).is_none(),
            "the card beside it does not"
        );
        assert!(state.pulse().is_some(), "the pulse is still live");
        assert!(
            crate::overlay::card_pulse_ring(&theme, &state, Motion::STILL, entry.card_name)
                .is_none(),
            "`Motion::STILL` paints no ring while the pulse runs: the gate, not an empty state"
        );
    }

    /// A live pulse on the landed card paints a ring on that card's box, and the mount hands the
    /// ring the card's own `x:Name`.
    ///
    /// The pulse is installed here rather than taken from `perform_jump`, because that path's
    /// gate reads the process-wide animation flag and another test owns it. What this proves is
    /// the wiring: the wrapper's selector and the ring's selector are the same box, so the ring
    /// is on the card the id names and the tab body passes that id through.
    #[gpui::test]
    fn a_live_pulse_paints_the_ring_on_the_landed_cards_box(cx: &mut gpui::TestAppContext) {
        use crate::views::command_search::JumpRequest;

        let root = scratch_root();
        let (shell, _, mut visual) = shell_in_window(cx, &root);
        let (entry, _) = a_card_bearing_entry();

        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.select_tab_named(entry.tab, cx);
                let now = shell.runtime.now_ms();
                let request = JumpRequest {
                    entry,
                    pulse_card: Some(entry.card_name),
                };
                assert!(
                    shell.search.begin_pulse(&request, now, true).is_some(),
                    "the pulse is live before the frame that paints it"
                );
            });
        });

        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        scroll_into_view(&mut visual, entry.card_name);

        let card = visual
            .debug_bounds(entry.card_name)
            .expect("the card wrapper carries the card's own name as its selector");
        let ring = visual
            .debug_bounds("card-pulse-ring")
            .expect("the ring paints while the pulse is live");
        assert_eq!(ring.origin, card.origin, "the ring sits on the card's box");
        assert_eq!(ring.size, card.size, "and covers it exactly");
    }

    /// The first catalogue entry whose card is on a card-bearing tab, with that tab's cards.
    ///
    /// Read from the catalogue and the copy table rather than named, so this cannot drift into a
    /// selector for a card that no longer exists.
    fn a_card_bearing_entry() -> (&'static search::SearchEntry, &'static [Card]) {
        search::entries()
            .iter()
            .find_map(|entry| {
                if entry.card_name.is_empty() {
                    return None;
                }
                let cards = cards_on(entry.tab)?;
                cards
                    .iter()
                    .any(|card| card.id.eq_ignore_ascii_case(entry.card_name))
                    .then_some((entry, cards))
            })
            .expect("the catalogue has a module card on a card-bearing tab")
    }

    /// The cards a tab paints, or `None` for a view tab.
    fn cards_on(tab_name: &str) -> Option<&'static [Card]> {
        copy::TABS
            .iter()
            .find(|tab| tab.name.eq_ignore_ascii_case(tab_name))
            .and_then(|tab| match tab.body {
                TabBody::Cards(cards) => Some(cards),
                _ => None,
            })
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
        // The token box belongs to the Accounts tab's *Paste* panel, and that panel is closed
        // until it is asked for (`accounts_view.rs:236`); the shell also opens on Combat. A
        // keystroke reaches only an element the frame painted — a focus handle with no dispatch
        // node falls back to the tree's synthetic root, which carries no listeners — so the tab is
        // selected, the panel opened and the frame drawn before the field is focused. Without this
        // the box is "focused" in the window's state and still receives nothing, which is exactly
        // the failure this test caught.
        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.select_tab_named("accounts", cx);
                shell.toggle_account_panel(AccountPanel::Paste, cx);
            })
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
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

    /// The rail paints the running build's version, and the module's reader is what says which.
    ///
    /// `CurrentVersionText` (`MainWindow.xaml:278`); the value is
    /// [`aoko_core::services::update::current_version`]'s — the port of
    /// `Assembly.GetName().Version?.ToString(3)` — so a release bump moves this line and no version
    /// literal exists in this file. The sentence is the `click to view` one, because the line is
    /// also the update control and that tail is what says so.
    #[gpui::test]
    fn the_rail_paints_the_running_builds_version(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (_shell, _handle, mut visual) = shell_in_window(cx, &root);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(
            visual.debug_bounds("rail-version").is_some(),
            "the version line is in the rail, not only in the model"
        );

        let current = aoko_core::services::update::current_version();
        let expected = copy::UPDATE_CURRENT_VIEW.replace("{version}", &current);
        assert_eq!(
            Shell::version_line(),
            expected.as_str(),
            "the copy owner's sentence with the build's own version in its slot"
        );
    }

    /// The version line is the rail's update control: one press, one releases page.
    ///
    /// The opener is injected, so what this asserts is the call site rather than a browser: the
    /// shipped `aoko_platform::open_url` launches the desktop's launcher and no assertion can read
    /// it. The URL is the releases page the managed check ended at (`MainWindow.xaml.cs:1885`) — the
    /// check itself has no fetcher in this tree, which is why the press is the whole of the surface.
    #[gpui::test]
    fn the_version_lines_press_opens_the_releases_page(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;

        let root = scratch_root();
        let (shell, _handle, mut visual) = shell_in_window(cx, &root);

        let opened: Arc<std::sync::Mutex<Vec<String>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = opened.clone();
        cx.update(|cx| {
            shell.update(cx, |shell, _| {
                shell.set_url_opener(Some(Arc::new(move |url: &str| {
                    recorder
                        .lock()
                        .expect("the press recorder")
                        .push(url.to_owned());
                    Ok::<(), aoko_platform::PlatformError>(())
                })));
            })
        });

        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let bounds = visual
            .debug_bounds("rail-version")
            .expect("the version line paints");
        visual.simulate_click(bounds.center(), Modifiers::default());

        assert_eq!(
            opened.lock().expect("the press recorder").clone(),
            vec![aoko_core::services::update::RELEASES_URL.to_owned()],
            "one press, one URL: the page the managed check existed to offer"
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

    /// The Esc chip is a button, not a painted box: a click closes the card and hands focus back.
    ///
    /// Managed's chip was `CloseChip_Click` (`CommandSearchOverlay.xaml.cs:245-250`), and it called
    /// the same `Close` the key did. The blur half is the one worth pinning: a close that left the
    /// query box focused makes [`Shell::any_text_field_focused`] report a field nobody can see, and
    /// Ctrl+K and type-to-open are refused for the rest of the session.
    #[gpui::test]
    fn a_click_on_the_esc_chip_closes_the_card_and_hands_focus_back(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("ctrl-k").expect("keystroke"));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let chip = visual
            .debug_bounds("command-search-close-chip")
            .expect("the chip paints while the card is open");

        // The open path focuses the box, which is what makes the release observable below.
        let focused = visual.update(|window, cx| {
            shell
                .read(cx)
                .search_query
                .as_ref()
                .is_some_and(|field| field.read(cx).focus_handle().is_focused(window))
        });
        assert!(
            focused,
            "the open path focuses the box the close has to release"
        );

        visual.simulate_click(chip.center(), gpui::Modifiers::default());

        assert!(
            !cx.read(|cx| shell.read(cx).search().is_open()),
            "the click closed the overlay"
        );
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(
            visual.debug_bounds("command-search-card").is_none(),
            "and the card is off the frame, not only out of the state"
        );
        let still_focused = visual.update(|window, cx| {
            shell
                .read(cx)
                .search_query
                .as_ref()
                .is_some_and(|field| field.read(cx).focus_handle().is_focused(window))
        });
        assert!(
            !still_focused,
            "and the box no longer owns the keyboard, or the gate stays shut"
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
        // The box is on the Render tab's Block ESP card. A keystroke reaches only an element that
        // is in the painted tree — a focus handle with no dispatch node falls back to the tree's
        // synthetic root, which carries no listeners — so the tab is selected and drawn before
        // the field is focused.
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named("render", cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

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

    /// Every id form names one block; a new block appends, a tracked one is enabled.
    ///
    /// The two halves need different blocks, which is the thing the first version got wrong: the
    /// default profile is seeded from `PRESETS`, so an id in that catalog exercises *enable* and
    /// an id outside it exercises *append*.
    #[gpui::test]
    fn adding_a_known_block_enables_it_and_stores_no_second_entry(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);

        let before = cx.read(|cx| shell.read(cx).profile().block_esp_targets.len());
        // A block the presets do not cover, because the append half cannot be tested with one
        // they do: `build_default_targets` seeds all twenty `PRESETS`
        // (`aoko-protocol/src/block_esp.rs:266-281`), so the first version's
        // `minecraft:diamond_ore` was already tracked and took the *enable* branch — the
        // assertion `before + 1` was unreachable against a default profile rather than failing
        // on a broken add.
        submit_block_add(
            cx,
            &shell,
            &mut visual,
            handle,
            "minecraft:Amethyst_Cluster",
        );

        let after = cx.read(|cx| shell.read(cx).profile().block_esp_targets.clone());
        assert_eq!(after.len(), before + 1, "one target was appended");
        let index = after
            .iter()
            .position(|target| target.registry_id == "minecraft:amethyst_cluster")
            .expect("the namespaced, mixed-case form normalises to the registry id");
        assert_eq!(after[index].display_name, "amethyst cluster");

        // The enable half is a preset, which the default profile already tracks and seeds
        // disabled: the rule under test is that the second add turns it on instead of appending.
        let preset = after
            .iter()
            .position(|target| target.registry_id == "minecraft:emerald_ore")
            .expect("the presets seed emerald ore");
        assert!(
            !after[preset].enabled,
            "the default profile seeds emerald ore disabled"
        );

        // The bare form of the same block: normalisation is what makes it the same entry.
        submit_block_add(cx, &shell, &mut visual, handle, "emerald_ore");

        let after = cx.read(|cx| shell.read(cx).profile().block_esp_targets.clone());
        assert_eq!(
            after.len(),
            before + 1,
            "no second entry for a block that was already tracked"
        );
        assert!(
            after[preset].enabled,
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

        // `BlockEspCard` is on the Render tab, and the card sits below the fold at the test
        // viewport. The tab is selected first: the row is painted only in its own tab's body, and
        // this test is about what the paint carries, not about which tab the shell opens on.
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named("render", cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
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

    /// Selects the Render tab and draws, which is what every row test has to do first.
    ///
    /// `BlockEspCard` is on Render and `render_cards` paints only the selected tab's body, so a
    /// row's callback cannot be reached from a window that never painted its card — the class of
    /// failure that had a placeholder arm pass its own tests.
    fn show_block_esp_rows(
        cx: &mut gpui::TestAppContext,
        shell: &Entity<Shell>,
        visual: &mut gpui::VisualTestContext,
    ) {
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named("render", cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(
            visual.debug_bounds("blockesp-rows").is_some(),
            "the tracked-block list paints on the Block ESP card"
        );
        scroll_into_view(visual, "blockesp-rows");
    }

    /// A toggle writes the flag the row carried, not its negation, and reaches the controller.
    ///
    /// Driven through [`Shell::apply_row_action`] because that is the function the checkbox's
    /// closure calls: `components::block_esp` gives the checkbox a `format!`ed `ElementId` and no
    /// debug selector, so a `VisualTestContext` has no bounds to click. What the paint can prove
    /// is that the list is on screen, which is the half the placeholder arm used to fail.
    ///
    /// The frame half is asserted on the controller's profile rather than
    /// `config_payload().block_esp_blocks`: `AppController::payload`
    /// (`aoko-core/src/app.rs:426-437`) fills `fight_status` and the keybinds and leaves that
    /// string at its default, so a frame assertion here would pass against a broken write-through.
    /// That gap is `aoko-core`'s, and the report names it.
    #[gpui::test]
    fn a_toggle_writes_the_flag_the_row_reported(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (shell, _, mut visual) = shell_in_window(cx, &root);
        show_block_esp_rows(cx, &shell, &mut visual);

        // A preset the default profile seeds disabled: `build_default_targets` seeds every
        // `PRESETS` entry (`aoko-protocol/src/block_esp.rs:164`), so this is the state a real
        // first frame shows rather than one this test arranged.
        let id = "minecraft:emerald_ore";
        let seeded = cx.read(|cx| {
            shell
                .read(cx)
                .profile()
                .block_esp_targets
                .iter()
                .find(|target| target.registry_id == id)
                .map(|target| target.enabled)
        });
        assert_eq!(seeded, Some(false), "the preset starts disabled");

        // The value the row reported is the value written: a handler that negated would turn
        // this into `true` and prove itself here.
        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_row_action(
                    RowAction::Toggle {
                        registry_id: id.to_owned(),
                        enabled: false,
                    },
                    cx,
                );
            });
        });
        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .profile()
                .block_esp_targets
                .iter()
                .find(|target| target.registry_id == id)
                .map(|target| target.enabled)),
            Some(false),
            "an action carrying `false` leaves the entry disabled"
        );

        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_row_action(
                    RowAction::Toggle {
                        registry_id: id.to_owned(),
                        enabled: true,
                    },
                    cx,
                );
            });
        });
        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .controller()
                .profile()
                .block_esp_targets
                .iter()
                .find(|target| target.registry_id == id)
                .map(|target| target.enabled)),
            Some(true),
            "and the enable reached the controller, which is what the frame is built from"
        );

        // The paint agrees with the write, so the checkbox is not reading a stale row.
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(
            cx.read(|cx| shell
                .read(cx)
                .profile()
                .block_esp_targets
                .iter()
                .find(|target| target.registry_id == id)
                .map(|target| target.enabled)),
            Some(true),
            "the row repaints from the profile it just wrote"
        );
    }

    /// A remove drops the entry it names and leaves every other one in place.
    ///
    /// `retain` by id rather than by index: the row carries the `registry_id` it was painted
    /// with, so a list that reordered between the press and the write still removes the row the
    /// user pressed instead of whatever moved into its place.
    #[gpui::test]
    fn a_remove_drops_exactly_that_entry(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (shell, _, mut visual) = shell_in_window(cx, &root);
        show_block_esp_rows(cx, &shell, &mut visual);

        let before = cx.read(|cx| shell.read(cx).profile().block_esp_targets.clone());
        assert!(
            before.len() > 1,
            "the remove has to be distinguishable from emptying the list"
        );
        let removed = "minecraft:diamond_ore";
        let kept = "minecraft:emerald_ore";
        assert!(before.iter().any(|target| target.registry_id == removed));
        assert!(before.iter().any(|target| target.registry_id == kept));

        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_row_action(
                    RowAction::Remove {
                        registry_id: removed.to_owned(),
                    },
                    cx,
                );
            });
        });

        let after = cx.read(|cx| {
            shell
                .read(cx)
                .controller()
                .profile()
                .block_esp_targets
                .clone()
        });
        assert_eq!(
            after.len(),
            before.len() - 1,
            "exactly one entry left the list"
        );
        assert!(
            !after.iter().any(|target| target.registry_id == removed),
            "and it is the one the row named"
        );
        assert!(
            after.iter().any(|target| target.registry_id == kept),
            "the entry beside it is untouched"
        );

        // A second remove for the same id is the no-op a stale row would produce, not a wipe.
        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_row_action(
                    RowAction::Remove {
                        registry_id: removed.to_owned(),
                    },
                    cx,
                );
            });
        });
        assert_eq!(
            cx.read(|cx| shell.read(cx).profile().block_esp_targets.len()),
            after.len(),
            "an id that is already gone removes nothing"
        );
    }

    /// The row's colour control opens the picker on that entry, and an acceptance writes it back.
    ///
    /// Two halves in one cause: the press has to open on the colour the row paints — a picker
    /// that opened on black would change the entry merely by being accepted — and the accepted
    /// value has to land in the profile's own form, six uppercase digits with no `#`, which is
    /// what `block_esp::normalize_color` accepts and what the presets are written in.
    #[gpui::test]
    fn a_row_colour_press_opens_the_picker_on_that_rows_colour(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (shell, _, mut visual) = shell_in_window(cx, &root);
        show_block_esp_rows(cx, &shell, &mut visual);

        let id = "minecraft:diamond_ore";
        let painted = cx.read(|cx| {
            let shell = shell.read(cx);
            let target = shell
                .profile()
                .block_esp_targets
                .iter()
                .find(|target| target.registry_id == id)
                .expect("the default profile tracks diamond ore");
            Rgb::from(
                BlockEspRow::new(
                    target.registry_id.clone(),
                    target.display_name.clone(),
                    target.color_hex.clone(),
                    target.enabled,
                )
                .color(),
            )
        });

        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_row_action(
                    RowAction::Color {
                        registry_id: id.to_owned(),
                    },
                    cx,
                );
            });
        });

        let opened = cx.read(|cx| {
            shell
                .read(cx)
                .picker
                .as_ref()
                .map(|(target, view)| (target.clone(), view.read(cx).selected()))
        });
        assert_eq!(
            opened,
            Some((PickerTarget::Block(id.to_owned()), painted)),
            "the press opened this row's picker, on the colour the row is showing"
        );

        // OK, as the dialog reports it: the accepted colour is canonical `#RRGGBB` and the
        // profile stores it without the hash.
        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.finish_color_picker(
                    PickerTarget::Block(id.to_owned()),
                    PickResult {
                        color: Rgb::new(0x11, 0x22, 0x33),
                        accepted: true,
                    },
                    cx,
                );
            });
        });

        let (stored, open) = cx.read(|cx| {
            let shell = shell.read(cx);
            (
                shell
                    .controller()
                    .profile()
                    .block_esp_targets
                    .iter()
                    .find(|target| target.registry_id == id)
                    .map(|target| target.color_hex.clone()),
                shell.picker.is_some(),
            )
        });
        assert_eq!(
            stored.as_deref(),
            Some("112233"),
            "the accepted colour reached the profile in the form the presets use"
        );
        assert!(!open, "and the dialog closed");
    }

    // ---- the startup palette precedence ----------------------------------------

    /// `--theme` wins over the saved profile's palette; without one, the profile wins.
    ///
    /// This is the decision [`Shell::adopt_profile_theme`] makes, driven twice from the same
    /// scratch palette file. The store is injected with [`Shell::set_palette_path`], so the
    /// custom palette is a file this test wrote and no assertion can depend on — or disturb —
    /// this machine's palette directory.
    ///
    /// It does not go through [`Shell::startup`], because that reads the profile store and
    /// probes capabilities; what it pins is the one branch, and the call site in `from_system`
    /// is a single line that names it.
    #[test]
    fn an_explicit_theme_pins_the_palette_against_the_saved_profile() {
        let root = scratch_root();
        let path = root.join("palettes.json");

        // Written by the production writer, so the shape of the file is the file's own and
        // not a fixture this test invented beside it.
        let mut store = PaletteStore::at(path.clone());
        store.draft_mut().name = "Handmade".to_owned();
        store
            .save_draft(false)
            .expect("writes into the scratch root");
        assert!(
            store
                .palettes()
                .iter()
                .any(|stored| stored.name == "Handmade"),
            "the file really carries the palette before any shell reads it"
        );

        let mut profile = aoko_protocol::profile::Profile::default();
        profile.gui_theme = "Handmade".to_owned();

        // Unpinned, as in no `--theme` on the command line: the profile's palette is what the
        // window paints, which is how a palette chosen in Settings survives a restart.
        let mut unpinned = Shell::with_profile(Theme::slate(), profile.clone());
        unpinned.set_palette_path(path.clone());
        unpinned.adopt_profile_theme();
        assert_eq!(
            unpinned.theme().name,
            "Handmade",
            "the saved profile's custom palette is adopted when nothing pinned one"
        );

        // Pinned, as in `--theme Slate` on the command line: the profile names a custom
        // palette and the theme does not move, which is the whole point of the flag.
        let mut pinned = Shell::with_profile(Theme::slate(), profile);
        pinned.set_palette_path(path);
        pinned.set_theme_pinned(true);
        pinned.adopt_profile_theme();
        assert_eq!(
            pinned.theme().name,
            "Slate",
            "an explicit theme leaves the saved profile unable to replace it"
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
        // The four boxes are on the Settings tab's palette card, and a keystroke reaches only an
        // element that is in the painted tree: a focus handle with no dispatch node falls back to
        // the tree's synthetic root, which has no listeners, so the character would go nowhere.
        // Selecting the tab and drawing once is what puts the box on the path — the same two
        // steps `typing_in_the_config_name_box_reaches_the_shell` takes for the config box.
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named("settings", cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

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

    /// The saved palettes paint one card each, and the name box is a field whose text is the draft's.
    ///
    /// The two controls the card had no mount for: `CustomPalettesItems` (`MainWindow.xaml:1910-1937`),
    /// one card per store entry with the name and the four swatches the template binds, and
    /// `NewPaletteNameBox` (`:1949-1956`). The presses that make a card a button are not part of this
    /// — `render_palette_rows` says why — but the data on screen is the store's, which is what the
    /// mount had to prove, and the box's text is the draft's `name`, which is what the Add press
    /// reads.
    /// The palette card on the Settings tab, with one saved palette and one drawn frame.
    ///
    /// The file is the test's and the frame is what gives the card's rows bounds: every test below
    /// presses a painted control, and a control with no bounds cannot be pressed.
    fn palette_card_in_settings(
        cx: &mut gpui::TestAppContext,
    ) -> (std::path::PathBuf, Entity<Shell>, gpui::VisualTestContext) {
        let root = scratch_root();
        let path = root.join("palettes.json");
        let mut store = palette_store::PaletteStore::at(path.clone());
        store.draft_mut().name = "Ember".to_owned();
        store.save_draft(false).expect("the palette was written");

        let (shell, _handle, mut visual) = shell_in_window(cx, &root);
        cx.update(|cx| shell.update(cx, |shell, _| shell.set_palette_path(path.clone())));
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named("settings", cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        (path, shell, visual)
    }

    /// A cap press asks, and only the confirmation removes.
    ///
    /// Managed asked in a `MessageBox` (`MainWindow.xaml.cs:1178-1181`); the card asks in place, so
    /// the first press has to leave the file exactly as it was — the assertion that would fail if a
    /// cap ever deleted on its own.
    #[gpui::test]
    fn a_cap_press_asks_and_the_confirmation_removes(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;

        let (path, shell, mut visual) = palette_card_in_settings(cx);

        scroll_into_view(&mut visual, "saved-palette-delete-0");
        let cap = visual
            .debug_bounds("saved-palette-delete-0")
            .expect("the cap paints");
        visual.simulate_click(cap.center(), Modifiers::default());

        assert_eq!(
            cx.read(|cx| shell.read(cx).palette.delete_armed().map(str::to_owned)),
            Some("Ember".to_owned()),
            "the cap asks about the row it belongs to"
        );
        assert_eq!(
            palette_store::PaletteStore::at(path.clone())
                .palettes()
                .len(),
            1,
            "and nothing is removed until the confirmation"
        );
        assert_eq!(
            cx.read(|cx| shell.read(cx).palette.current_name().to_owned()),
            "Slate",
            "nor did the press fall through to the card's apply: the cap stops propagation"
        );

        cx.update(|cx| shell.update(cx, |shell, cx| shell.palette_confirm_delete(cx)));

        assert_eq!(
            palette_store::PaletteStore::at(path).palettes().len(),
            0,
            "the confirmation removed it"
        );
        assert_eq!(
            cx.read(|cx| shell.read(cx).palette_status.clone()),
            Some(SharedString::from(
                copy::PALETTE_STATUS_DELETED.replace("{name}", "Ember")
            )),
            "and the card paints the store's own sentence for it"
        );
    }

    /// A row press applies it: the theme, the profile and the live-palette line.
    ///
    /// The three are one behaviour in managed (`GuiPaletteButton_Click`, `:930-960`) — a press that
    /// moved only the theme would leave the profile pointing at the old name, and the next load
    /// would undo the press.
    #[gpui::test]
    fn a_saved_rows_press_applies_it_to_the_theme_the_profile_and_the_line(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::Modifiers;

        let (_path, shell, mut visual) = palette_card_in_settings(cx);

        scroll_into_view(&mut visual, "saved-palette-0");
        let row = visual
            .debug_bounds("saved-palette-0")
            .expect("the row paints");
        visual.simulate_click(row.center(), Modifiers::default());

        assert_eq!(
            cx.read(|cx| shell.read(cx).theme().name.clone()),
            "Ember",
            "the press paints the palette it names"
        );
        assert_eq!(
            cx.read(|cx| shell.read(cx).profile.gui_theme.clone()),
            "Ember",
            "and the name reaches the profile the config frame is built from"
        );

        // The line above the rows is read once a frame, so the frame after the press is what shows it.
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(
            cx.read(|cx| shell.read(cx).palette_current.clone()),
            Some(SharedString::from(format!(
                "{}{}",
                copy::PALETTE_CURRENT_PREFIX,
                "Ember"
            ))),
            "the live-palette line follows the store"
        );
    }

    /// The Add press writes the row, and the card paints it.
    ///
    /// The press itself is invoked through the method the button's callback runs: `components::button`
    /// installs an `ElementId` but no `debug_selector`, so a card button is not addressable from a
    /// `VisualTestContext` — the limitation `the_rail_button_opens_the_state_ctrl_k_opens` records
    /// for the rail's own button. What is asserted here is the write, the sentence and the new row.
    #[gpui::test]
    fn the_add_press_writes_the_row_the_store_keeps(cx: &mut gpui::TestAppContext) {
        let (path, shell, mut visual) = palette_card_in_settings(cx);

        cx.update(|cx| {
            shell.update(cx, |shell, cx| {
                shell.palette.draft_mut().name = "Dusk".to_owned();
                shell.palette_add(cx);
            })
        });

        assert!(
            palette_store::PaletteStore::at(path)
                .palettes()
                .iter()
                .any(|stored| stored.name == "Dusk"),
            "the press wrote the row"
        );
        assert_eq!(
            cx.read(|cx| shell.read(cx).palette_status.clone()),
            Some(SharedString::from(
                copy::PALETTE_STATUS_SAVED.replace("{name}", "Dusk")
            )),
            "and the card says so in the store's words"
        );

        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        scroll_into_view(&mut visual, "saved-palette-1");
        assert!(
            visual.debug_bounds("saved-palette-1").is_some(),
            "the card paints a second row"
        );
    }

    /// A preview press applies that palette instantly, which is what its own copy promises.
    #[gpui::test]
    fn a_preview_press_applies_that_palette(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;

        let (_path, shell, mut visual) = palette_card_in_settings(cx);

        scroll_into_view(&mut visual, "palette-preview-Ink");
        let preview = visual
            .debug_bounds("palette-preview-Ink")
            .expect("the preview paints");
        visual.simulate_click(preview.center(), Modifiers::default());

        assert_eq!(
            cx.read(|cx| shell.read(cx).theme().name.clone()),
            "Ink",
            "the preview press is the apply, not a second path"
        );
    }

    /// The live-palette line above the cards names the palette the store says is applied.
    ///
    /// `MainWindow.xaml:1904` is a `Run` inside one sentence with the name after it, which is why the
    /// cached text is the prefix and the name together rather than two children. GPUI exposes no text
    /// readback, so the assertion is the two halves the paint is made of — the line is in the frame,
    /// and its source is `PALETTE_CURRENT_PREFIX` plus the store's current name, not the prefix alone.
    #[gpui::test]
    fn the_palette_card_names_the_applied_palette(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let palettes = root.join("palettes.json");

        let (shell, _handle, mut visual) = shell_in_window(cx, &root);
        cx.update(|cx| shell.update(cx, |shell, _| shell.set_palette_path(palettes)));
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named("settings", cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        scroll_into_view(&mut visual, "palette-current");
        assert!(
            visual.debug_bounds("palette-current").is_some(),
            "the line above the cards is in the frame"
        );

        let expected = cx.read(|cx| {
            format!(
                "{}{}",
                copy::PALETTE_CURRENT_PREFIX,
                shell.read(cx).palette.current_name()
            )
        });
        assert_eq!(
            cx.read(|cx| shell.read(cx).palette_current.clone()),
            Some(SharedString::from(expected)),
            "the cached text is the prefix and the applied name together"
        );
    }

    #[gpui::test]
    fn the_palette_card_paints_the_saved_rows_and_the_name_box(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let palettes = root.join("palettes.json");
        // One saved palette, written by the store rather than by hand.
        let mut store = palette_store::PaletteStore::at(palettes.clone());
        store.draft_mut().name = "Ember".to_owned();
        store.save_draft(false).expect("the palette was written");

        let (shell, handle, mut visual) = shell_in_window(cx, &root);
        cx.update(|cx| shell.update(cx, |shell, _| shell.set_palette_path(palettes)));
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named("settings", cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        // The rows are the store's, one card per entry.
        scroll_into_view(&mut visual, "saved-palette-0");
        let card = visual
            .debug_bounds("saved-palette-0")
            .expect("the saved palette paints a card");
        assert_eq!(
            card.size.width,
            px(metrics::palette_card::WIDTH),
            "180 wide, as the markup says"
        );
        assert!(
            visual.debug_bounds("saved-palette-1").is_none(),
            "one entry is one card"
        );

        // The name box is a field, not a painted line: a keystroke reaches the draft.
        scroll_into_view(&mut visual, "palette-name-box");
        assert!(
            visual.debug_bounds("palette-name-box").is_some(),
            "the name box paints"
        );
        let focus = cx.read(|cx| {
            shell
                .read(cx)
                .palette_name
                .as_ref()
                .expect("the box exists after the first frame")
                .read(cx)
                .focus_handle()
                .clone()
        });
        visual.update(|window, cx| window.focus(&focus, cx));
        for key in ["e", "m"] {
            visual
                .cx
                .dispatch_keystroke(handle, Keystroke::parse(key).expect("keystroke"));
        }
        assert_eq!(
            cx.read(|cx| shell.read(cx).palette().draft().name.clone()),
            "em",
            "the box's callback writes the draft's name, which is the field the Add press reads"
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

    // ---- the colour picker's host ----------------------------------------------

    /// Puts one palette row in the state a user leaves it in before pressing anything: text in
    /// the box, no Enter, so the store holds the typed spelling rather than the canonical form.
    ///
    /// That difference is what makes the accept below observable. A row that was already
    /// canonical would be written back unchanged, and the test would pass without the picker
    /// having written anything.
    fn seed_uncanonical_row(
        cx: &mut gpui::TestAppContext,
        shell: &Entity<Shell>,
        bind: &'static str,
        text: &'static str,
    ) {
        cx.update(|cx| {
            shell.update(cx, |shell, cx| shell.write_color_text(bind, text, cx));
        });
    }

    /// Selects Settings, draws, and scrolls one palette swatch into view, returning its box.
    fn show_swatch(
        cx: &mut gpui::TestAppContext,
        shell: &Entity<Shell>,
        visual: &mut gpui::VisualTestContext,
        selector: &'static str,
    ) -> gpui::Bounds<gpui::Pixels> {
        cx.update(|cx| shell.update(cx, |shell, cx| shell.select_tab_named("settings", cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        scroll_into_view(visual, selector);
        visual.debug_bounds(selector).expect("the swatch paints")
    }

    /// A swatch press opens the picker on the colour that row is painting.
    ///
    /// The initial value is the swatch memo, not `Rgb::default()`: a picker that opened on black
    /// would change the row merely by being accepted, which is the bug this pins.
    #[gpui::test]
    fn a_swatch_press_opens_the_picker_on_the_rows_colour(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;

        let root = scratch_root();
        let (shell, _, mut visual) = shell_in_window(cx, &root);
        let bounds = show_swatch(cx, &shell, &mut visual, "palette-swatch-PaletteBgBox");

        visual.simulate_click(bounds.center(), Modifiers::default());

        let (target, selected, painted) = cx.read(|cx| {
            let shell = shell.read(cx);
            let opened = shell
                .picker
                .as_ref()
                .map(|(target, view)| (target.clone(), view.read(cx).selected()));
            let (target, selected) = match opened {
                Some((target, color)) => (Some(target), Some(color)),
                None => (None, None),
            };
            (
                target,
                selected,
                shell.swatches.get("PaletteBgBox").copied().map(Rgb::from),
            )
        });
        assert_eq!(
            target,
            Some(PickerTarget::Palette("PaletteBgBox")),
            "the press opened the bg row's picker"
        );
        assert_eq!(
            selected, painted,
            "and it opened on the colour the swatch is showing"
        );
    }

    /// OK writes the store, the memo and the box; a row whose text was not canonical comes back
    /// canonical, which is the only way the write is visible.
    #[gpui::test]
    fn accepting_the_picker_writes_the_store_and_the_box(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;

        let root = scratch_root();
        let (shell, _, mut visual) = shell_in_window(cx, &root);
        seed_uncanonical_row(cx, &shell, "PaletteBgBox", "123456");
        let swatch = show_swatch(cx, &shell, &mut visual, "palette-swatch-PaletteBgBox");

        visual.simulate_click(swatch.center(), Modifiers::default());
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let opened_on = cx.read(|cx| {
            shell
                .read(cx)
                .picker
                .as_ref()
                .map(|(_, view)| view.read(cx).selected())
        });
        assert_eq!(
            opened_on,
            Some(Rgb::new(0x12, 0x34, 0x56)),
            "the dialog opened on the colour the row is showing"
        );

        let ok = visual
            .debug_bounds("color-picker-ok")
            .expect("the dialog's OK paints in the host layer");
        visual.simulate_click(ok.center(), Modifiers::default());

        let (store, box_text, open) = cx.read(|cx| {
            let shell = shell.read(cx);
            (
                shell.palette.get("PaletteBgBox").map(str::to_owned),
                shell
                    .color_fields
                    .get("PaletteBgBox")
                    .map(|field| field.read(cx).text().to_owned()),
                shell.picker.is_some(),
            )
        });
        assert_eq!(
            store.as_deref(),
            Some("#123456"),
            "the accepted colour reached the store in canonical form"
        );
        assert_eq!(
            box_text.as_deref(),
            Some("#123456"),
            "and the box shows the same string, so the row cannot disagree with the store"
        );
        assert!(!open, "and the dialog closed");
    }

    /// Cancel closes the dialog and writes nothing at all — not the colour, not the text.
    #[gpui::test]
    fn cancelling_the_picker_writes_nothing(cx: &mut gpui::TestAppContext) {
        use gpui::Modifiers;

        let root = scratch_root();
        let (shell, _, mut visual) = shell_in_window(cx, &root);
        seed_uncanonical_row(cx, &shell, "PaletteBgBox", "123456");
        let swatch = show_swatch(cx, &shell, &mut visual, "palette-swatch-PaletteBgBox");

        visual.simulate_click(swatch.center(), Modifiers::default());
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let cancel = visual
            .debug_bounds("color-picker-cancel")
            .expect("the dialog's Cancel paints in the host layer");
        visual.simulate_click(cancel.center(), Modifiers::default());

        let (store, box_text, swatch, open) = cx.read(|cx| {
            let shell = shell.read(cx);
            (
                shell.palette.get("PaletteBgBox").map(str::to_owned),
                shell
                    .color_fields
                    .get("PaletteBgBox")
                    .map(|field| field.read(cx).text().to_owned()),
                shell.swatches.get("PaletteBgBox").copied(),
                shell.picker.is_some(),
            )
        });
        assert_eq!(
            store.as_deref(),
            Some("123456"),
            "a refusal leaves the store's text exactly as it was"
        );
        assert_eq!(
            box_text.as_deref(),
            Some("123456"),
            "and the box, which is the display of that same text"
        );
        assert_eq!(
            swatch,
            palette_store::parse_entry("123456"),
            "and the swatch keeps the colour it had"
        );
        assert!(
            !open,
            "the dialog still closes: Cancel is an answer, not an error"
        );
    }

    /// Escape cancels the picker, which is the managed `IsCancel="True"` on its Cancel button.
    ///
    /// WPF closed the dialog with `DialogResult = false` and wrote nothing; this asserts the same
    /// two halves, because the dialog has no key handler of its own and the host is where the
    /// window's Escape lands.
    #[gpui::test]
    fn escape_cancels_the_picker_like_the_managed_is_cancel(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);
        seed_uncanonical_row(cx, &shell, "PaletteBgBox", "123456");
        let swatch = show_swatch(cx, &shell, &mut visual, "palette-swatch-PaletteBgBox");

        visual.simulate_click(swatch.center(), gpui::Modifiers::default());
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("escape").expect("keystroke"));

        let (open, store, box_text) = cx.read(|cx| {
            let shell = shell.read(cx);
            (
                shell.picker.is_some(),
                shell.palette.get("PaletteBgBox").map(str::to_owned),
                shell
                    .color_fields
                    .get("PaletteBgBox")
                    .map(|field| field.read(cx).text().to_owned()),
            )
        });
        assert!(!open, "Escape closed the dialog, as `IsCancel` does");
        assert_eq!(
            store.as_deref(),
            Some("123456"),
            "and wrote nothing: the refusal is the whole of it"
        );
        assert_eq!(box_text.as_deref(), Some("123456"), "the box included");
    }

    // === the loader (`LoaderPanel`, `MainWindow.xaml:306-340`) =========================

    /// A window surface that answers from a fixed list and never touches the OS.
    ///
    /// The same stand-in `runtime`'s suite uses, for the same reason: a loader test that enumerated
    /// this machine would assert the machine. `cursor_visible` keeps the trait's default, because
    /// it is not part of the Inject gate — see [`Refusal`].
    struct FakeWindows {
        windows: Vec<WindowInfo>,
    }

    impl WindowInspector for FakeWindows {
        fn enumerate(&self) -> Result<Vec<WindowInfo>, aoko_platform::PlatformError> {
            Ok(self.windows.clone())
        }

        fn foreground(&self) -> Result<Option<WindowInfo>, aoko_platform::PlatformError> {
            Ok(self.windows.first().cloned())
        }

        fn client_bounds(
            &self,
            _handle: aoko_platform::WindowHandle,
        ) -> Result<aoko_platform::Rect, aoko_platform::PlatformError> {
            Ok(aoko_platform::Rect::default())
        }
    }

    fn window_surface(windows: Vec<WindowInfo>) -> Arc<dyn WindowInspector> {
        Arc::new(FakeWindows { windows })
    }

    /// One top-level window over `WindowInfo`'s public fields.
    fn window(title: &str, process: &str) -> WindowInfo {
        WindowInfo {
            handle: aoko_platform::WindowHandle(0x51),
            title: title.to_string(),
            class_name: Some("LWJGL".to_string()),
            pid: Some(4242),
            process_name: Some(process.to_string()),
            bounds: aoko_platform::Rect::default(),
        }
    }

    /// The client: a game title in a Java process, so both halves of the gate pass.
    fn game_window() -> WindowInfo {
        window("Minecraft 1.8.9", "javaw.exe")
    }

    /// A Java process whose title names the game while an editor owns it.
    ///
    /// This is the blind spot [`aoko_platform::is_game_window`]'s own doc records, not a refusal:
    /// managed's rule is title-only (`GameTitles`, `Contains(…, OrdinalIgnoreCase)`, ported at
    /// `aoko-platform/src/window.rs:69-85`), so a title containing `Minecraft` is the game whatever
    /// owns it — and this window is also a JVM, so **both** halves of the gate accept it. It is in
    /// the list test as a second JVM entry. The platform suite pins the same window and says why it
    /// passes (`the_jvm_composition_rejects_an_editor_whose_title_names_minecraft`,
    /// `window.rs:322-334`); a refusal fixture has to be [`java_ide_window`].
    fn editor_window() -> WindowInfo {
        window("Minecraft mod workspace", "javaw.exe")
    }

    /// A Java process whose title names no game: a Java IDE, which is the window the *title* rule
    /// refuses.
    ///
    /// It is offered by the discovery filter because it is a JVM (`target_is_candidate` accepts
    /// either half), and it cannot pass the gate for the reason the fixture beside it cannot fail
    /// it: the title carries none of the three markers. This is the `Refusal::NotGameTitle`
    /// fixture.
    fn java_ide_window() -> WindowInfo {
        window("IntelliJ IDEA", "javaw.exe")
    }

    /// A game title in a process that is not a launcher.
    fn plain_process_window() -> WindowInfo {
        window("Minecraft", "code.exe")
    }

    fn dials() -> usize {
        DIALS.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The status line a refusal paints, composed the way [`InjectionView::resolve`] composes it.
    fn refusal_line(refusal: Refusal) -> String {
        format!("{}{}", copy::LOADER_STATUS_PREFIX, refusal.message())
    }

    /// The status line a dial's outcome should paint, composed from the copy here.
    ///
    /// Composed from `copy` rather than from [`link_failure_line`] on purpose: the panel's rule is
    /// the thing under test, and an expectation that called it would agree with any change to it.
    fn failure_line(failure: LinkFailure) -> String {
        let sentence = match failure {
            LinkFailure::ConnectFailed { port } => {
                copy::LOADER_REASON_CONNECT_FAILED.replace("{port}", &port.to_string())
            }
            LinkFailure::Disconnected => copy::LOADER_REASON_DISCONNECTED.to_owned(),
        };
        format!("{}{}", copy::LOADER_STATUS_PREFIX, sentence)
    }

    /// An injector that records what it was asked to load and loads nothing.
    ///
    /// The platform's `inject_bridge` is resolve-then-load, so a press whose library is on disk
    /// reaches `inject`. Recording the pid together with the resolved path is what proves both
    /// halves arrived: the target's owner, and the name the version selected.
    #[derive(Default)]
    struct RecordingInjector {
        seen: std::sync::Mutex<Vec<(u32, std::path::PathBuf)>>,
    }

    impl RecordingInjector {
        fn calls(&self) -> Vec<(u32, std::path::PathBuf)> {
            self.seen.lock().expect("lock").clone()
        }
    }

    impl aoko_platform::NativeBridgeInjector for RecordingInjector {
        fn inject(
            &self,
            pid: u32,
            library: &std::path::Path,
        ) -> Result<aoko_platform::InjectionReport, aoko_platform::PlatformError> {
            self.seen
                .lock()
                .expect("lock")
                .push((pid, library.to_path_buf()));
            Ok(aoko_platform::InjectionReport {
                pid,
                library: library.display().to_string(),
                remote_base: None,
            })
        }

        fn support(&self) -> aoko_platform::BridgeSupport {
            aoko_platform::BridgeSupport::Supported
        }
    }

    /// The production attach over a directory the test owns, and an injector it can read.
    ///
    /// `Shell::from_system` composes `host_executable_dir()` with `inject_bridge`, and the first of
    /// those answers with the *test harness's* directory — no place a bridge library is ever going
    /// to be. The directory is therefore the seam, and `inject_bridge` itself is the call the press
    /// makes: the resolution, the name and the refusal stay the platform's.
    fn attach_over(
        directory: std::path::PathBuf,
        injector: Arc<RecordingInjector>,
    ) -> Arc<InjectBridge> {
        // The production seam, over a directory this test owns: the same call and the same
        // classification, so a test cannot assert a sentence `from_system` would not produce.
        Arc::new(move |pid, version| {
            aoko_platform::inject_bridge(injector.as_ref(), pid, version, &directory)
                .map_err(|error| classify_inject(error, version, &directory))
        })
    }

    /// A shell with no window surface still names a status; it just cannot reach a target.
    ///
    /// This is the shipped shape of a test or capture build (`cfg(test)`, `cfg(feature =
    /// "capture")`), and it is why the nine screenshots keep painting tabs: the loader's whole job
    /// is choosing a window, so a session with nothing to enumerate starts in the control panel.
    #[test]
    fn a_session_with_no_window_surface_starts_in_the_control_panel() {
        let shell = Shell::new(Theme::slate());

        assert!(
            !shell.loader_visible(),
            "no window surface means control mode"
        );
        assert!(shell.injection_targets().is_empty());
        assert_eq!(shell.injection_selection(), None);

        let view = shell.injection_view();
        assert_eq!(view.status, copy::LOADER_STATUS_NOT_INJECTED);
        assert_eq!(view.button, copy::LOADER_INJECT);
        assert!(view.enabled, "nothing has been attempted yet");
        assert!(!view.accent, "the accent line is the connected one");
    }

    /// The first of the machine's refusals: nothing can inspect windows, so nothing is attempted.
    #[gpui::test]
    fn a_press_with_no_window_surface_names_that_rule(cx: &mut gpui::TestAppContext) {
        let handle = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));

        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));

        let (view, targets) = cx.read(|cx| {
            let shell = handle.read(cx);
            (shell.injection_view(), shell.injection_targets().len())
        });
        // The exact line, so the sentence the panel paints is the copy owner's and the rule it
        // names is this one; a `contains` beside it would only restate the equality.
        assert_eq!(view.status, refusal_line(Refusal::NoWindowSense));
        assert!(
            view.enabled,
            "the press is repeatable; nothing is in flight"
        );
        assert_eq!(targets, 0, "no enumeration happened, so there is no list");
    }

    /// An enumeration with no candidate is a different refusal from a session that cannot look.
    #[gpui::test]
    fn an_enumeration_with_no_client_names_the_empty_list(cx: &mut gpui::TestAppContext) {
        let handle = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        cx.update(|cx| {
            handle.update(cx, |shell, _| {
                shell.set_windows(Some(window_surface(vec![window("Firefox", "firefox")])));
            })
        });

        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));

        let view = cx.read(|cx| handle.read(cx).injection_view());
        assert_eq!(view.status, refusal_line(Refusal::NoWindow));
        assert!(view.enabled, "an empty list is a state to retry from");
    }

    /// One game window is taken without asking, which is `:1449-1452`.
    ///
    /// The dial is the transition's only effect, and the test build counts it instead of opening
    /// the loopback socket; see [`Shell::dial`].
    #[gpui::test]
    fn one_game_window_is_taken_without_a_choice_and_dials(cx: &mut gpui::TestAppContext) {
        let handle = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        cx.update(|cx| {
            handle.update(cx, |shell, _| {
                shell.set_windows(Some(window_surface(vec![game_window()])));
            })
        });

        let before = dials();
        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));

        let (view, selection, targets) = cx.read(|cx| {
            let shell = handle.read(cx);
            (
                shell.injection_view(),
                shell.injection_selection(),
                shell.injection_targets().len(),
            )
        });
        assert_eq!(selection, Some(0), "the only target is the chosen one");
        assert_eq!(targets, 1);
        assert_eq!(view.status, copy::LOADER_STATUS_CONNECTING);
        assert_eq!(view.button, copy::LOADER_INJECT_CONNECTING);
        assert!(!view.enabled, "a press in flight is not pressable again");
        assert_eq!(
            dials(),
            before + 1,
            "the accepted press dialled exactly once"
        );
    }

    /// More than one client waits for a choice, which is `:1470-1486` without the modal.
    ///
    /// The list is the platform's enumeration filtered by the gate, in `sort_windows` order (JVM
    /// first, then by title), so a candidate that fails the gate is still *offered* — that is what
    /// lets the row press name the missing half instead of hiding the window.
    #[gpui::test]
    fn several_clients_are_listed_in_the_platforms_order_and_wait(cx: &mut gpui::TestAppContext) {
        let root = scratch_root();
        let (handle, _rail, mut visual) = shell_in_window(cx, &root);
        cx.update(|cx| {
            handle.update(cx, |shell, _| {
                shell.loader_visible = true;
                shell.set_windows(Some(window_surface(vec![
                    plain_process_window(),
                    editor_window(),
                    game_window(),
                    window("Firefox", "firefox"),
                ])));
            })
        });

        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let (view, selection, titles) = cx.read(|cx| {
            let shell = handle.read(cx);
            (
                shell.injection_view(),
                shell.injection_selection(),
                shell
                    .injection_targets()
                    .iter()
                    .map(|target| target.title.clone())
                    .collect::<Vec<_>>(),
            )
        });

        assert_eq!(
            titles,
            ["Minecraft 1.8.9", "Minecraft mod workspace", "Minecraft"],
            "the two JVM entries first, by title, then the rest — `sort_windows`'s order, and no \
             title list of the panel's own"
        );
        assert_eq!(selection, None, "the user has to choose");
        assert_eq!(view.status, copy::LOADER_STATUS_DETECTING);
        assert!(!view.enabled, "the list is what the press waits on now");

        // The paint carries the managed sentence and one row per target, which is the half the
        // state assertions cannot see. `WindowPickerDialog.xaml:28` is the sentence; the modal's
        // frame is the part the port does not have.
        assert!(
            visual.debug_bounds("loader-targets-prompt").is_some(),
            "the multi-target prompt paints in exactly the state the modal opened in"
        );
        for selector in ["loader-target-0", "loader-target-1", "loader-target-2"] {
            assert!(
                visual.debug_bounds(selector).is_some(),
                "{selector} is a control, not just a string in the state"
            );
        }
        assert!(
            visual.debug_bounds("loader-target-3").is_none(),
            "and the window that failed both halves is not offered at all"
        );
    }

    /// A row press and the button press run the same gate, so the two cannot disagree.
    ///
    /// The pair this asserts is the point: the *same* window, through both entries, yields the same
    /// refusal and dials nothing. Row order is the platform's, so the indices are found by title.
    ///
    /// The window is a JVM whose title names no game, and that is the whole of the rule the gate
    /// can refuse on the title half: managed's test is `GameTitles` matched as substrings, ordinal
    /// and case-insensitive (`aoko-platform/src/window.rs:69-85`), so a JVM titled
    /// `Minecraft mod workspace` is *accepted* — the blind spot that rule's own doc records. A
    /// `NotGameTitle` fixture therefore cannot be the workspace editor; it has to be a window whose
    /// title is not nameable as the game at all, which is the Java IDE below.
    #[gpui::test]
    fn a_row_press_and_the_button_press_refuse_for_the_same_reason(cx: &mut gpui::TestAppContext) {
        // Through the row: two candidates, so the machine waits for a choice.
        let list = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        cx.update(|cx| {
            list.update(cx, |shell, _| {
                shell.set_windows(Some(window_surface(vec![java_ide_window(), game_window()])));
            })
        });
        cx.update(|cx| list.update(cx, |shell, cx| shell.inject(cx)));

        let before = dials();
        let ide = cx.read(|cx| {
            list.read(cx)
                .injection_targets()
                .iter()
                .position(|target| target.title == "IntelliJ IDEA")
                .expect("the IDE is offered")
        });
        cx.update(|cx| list.update(cx, |shell, cx| shell.select_injection_target(ide, cx)));
        let by_row = cx.read(|cx| list.read(cx).injection_view()).status;

        // Through the button: the same window alone, so the press takes it without asking.
        let single = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        cx.update(|cx| {
            single.update(cx, |shell, _| {
                shell.set_windows(Some(window_surface(vec![java_ide_window()])));
            })
        });
        cx.update(|cx| single.update(cx, |shell, cx| shell.inject(cx)));
        let by_press = cx.read(|cx| single.read(cx).injection_view()).status;

        assert_eq!(by_row, refusal_line(Refusal::NotGameTitle));
        assert_eq!(
            by_press, by_row,
            "one gate: the row press and the button press refuse in the same words"
        );
        assert_eq!(dials(), before, "a refused target never dials");
    }

    /// The gate's other half: a game title in a process that is not a launcher.
    #[gpui::test]
    fn a_game_title_outside_a_java_process_names_the_launcher(cx: &mut gpui::TestAppContext) {
        let handle = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        cx.update(|cx| {
            handle.update(cx, |shell, _| {
                shell.set_windows(Some(window_surface(vec![plain_process_window()])));
            })
        });

        let before = dials();
        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));

        let view = cx.read(|cx| handle.read(cx).injection_view());
        assert_eq!(view.status, refusal_line(Refusal::NotJavaProcess));
        // The fixture is what makes that the *process* rule and not the title: the title half
        // passes for this window, which is why the other half had to be the one to refuse.
        assert!(
            aoko_platform::is_game_window(&plain_process_window()),
            "the title rule passes for this window"
        );
        assert_eq!(dials(), before);
    }

    /// The attach's failure is a state the panel can act on, and its sentence is the copy's.
    ///
    /// `inject_bridge` refuses before the injector is touched, and the only fact the line owes is
    /// the resolved path — not the platform's `Not found:` marker, and not the version-selection
    /// reasoning behind it. Both of those are our types talking, and the rule is that neither
    /// reaches a status line. The dial is the proof that the attach failed *first*: a press that
    /// failed has not started the reader, and the button is pressable so the retry is real.
    #[gpui::test]
    fn a_press_with_no_bridge_library_names_the_path_it_looked_for(cx: &mut gpui::TestAppContext) {
        // A directory this test owns and never fills, so the resolution is the platform's own call
        // over a directory with nothing in it.
        let empty = scratch_root();
        let expected = aoko_platform::bridge_library_path(Some("1.8.9"), &empty);
        assert!(!expected.exists(), "the test's directory holds no library");

        let injector = Arc::new(RecordingInjector::default());
        let seam = attach_over(empty, injector.clone());
        let handle = cx.update(|cx| {
            cx.new(move |_| {
                let mut shell = Shell::new(Theme::slate());
                shell.set_windows(Some(window_surface(vec![game_window()])));
                shell.set_inject_bridge(Some(seam));
                shell
            })
        });

        let before = dials();
        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));

        let view = cx.read(|cx| handle.read(cx).injection_view());
        assert_eq!(
            view.status,
            format!(
                "{}{}",
                copy::LOADER_STATUS_PREFIX,
                copy::LOADER_REASON_BRIDGE_LIBRARY_MISSING
                    .replace("{path}", &expected.display().to_string())
            ),
            "the panel paints the copy line, filled with the path the resolver looked at"
        );
        assert!(
            !view.status.contains("Not found") && !view.status.contains("selects"),
            "neither the enum's marker nor the selection reasoning reaches the panel: {}",
            view.status
        );
        assert_eq!(view.button, copy::LOADER_INJECT, "the press is the retry");
        assert!(
            view.enabled,
            "a missing library is a state to fix, not a dead end"
        );
        assert!(!view.accent, "the accent line is the connected one");
        assert_eq!(
            dials(),
            before,
            "the attach failed before the reader was started"
        );
        assert!(
            injector.calls().is_empty(),
            "and the injector was never touched"
        );
    }

    /// A library that is on disk reaches the injector, and only then does the reader dial.
    #[gpui::test]
    fn a_press_with_the_library_on_disk_reaches_the_injector(cx: &mut gpui::TestAppContext) {
        let directory = scratch_root();
        let library = aoko_platform::bridge_library_path(Some("1.8.9"), &directory);
        std::fs::write(&library, b"not a real library").expect("write the bridge");

        let injector = Arc::new(RecordingInjector::default());
        let seam = attach_over(directory, injector.clone());
        let handle = cx.update(|cx| {
            cx.new(move |_| {
                let mut shell = Shell::new(Theme::slate());
                shell.set_windows(Some(window_surface(vec![game_window()])));
                shell.set_inject_bridge(Some(seam));
                shell
            })
        });

        let before = dials();
        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));

        assert_eq!(
            injector.calls(),
            vec![(4242, library)],
            "the injector was asked for the resolved path and the target's own pid"
        );
        let view = cx.read(|cx| handle.read(cx).injection_view());
        assert_eq!(
            view.status,
            copy::LOADER_STATUS_CONNECTING,
            "the attach succeeded, so the reader owns the panel"
        );
        assert!(!view.enabled, "and the press waits on the link");
        assert_eq!(
            dials(),
            before + 1,
            "the dial follows the attach; it does not replace it"
        );
    }

    /// The library name follows the target, and the mapping stays the platform's.
    ///
    /// Two titles, two names: `1.8.9` is the legacy `bridge.dll` and `26.3` is `bridge_261.dll`. A
    /// shell that joined a file name of its own would pass with one version and fail here, which is
    /// why the expectation is written out as the name and then resolved by the platform's own
    /// function rather than spelled twice.
    #[gpui::test]
    fn the_library_name_follows_the_targets_detected_version(cx: &mut gpui::TestAppContext) {
        let directory = scratch_root();
        let cases = [
            ("Minecraft 1.8.9", "1.8.9", "bridge.dll"),
            ("Minecraft 26.3", "26.3", "bridge_261.dll"),
        ];
        for (_, version, name) in cases {
            std::fs::write(directory.join(name), b"not a real library").expect("write the bridge");
            assert_eq!(
                aoko_platform::bridge_library_path(Some(version), &directory)
                    .file_name()
                    .and_then(|name| name.to_str()),
                Some(name),
                "{version} is the name the platform maps it to"
            );
        }

        for (title, version, _) in cases {
            let injector = Arc::new(RecordingInjector::default());
            let seam = attach_over(directory.clone(), injector.clone());
            let handle = cx.update(|cx| {
                cx.new(move |_| {
                    let mut shell = Shell::new(Theme::slate());
                    shell.set_windows(Some(window_surface(vec![window(title, "javaw.exe")])));
                    shell.set_inject_bridge(Some(seam));
                    shell
                })
            });
            cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));

            assert_eq!(
                injector.calls(),
                vec![(
                    4242,
                    aoko_platform::bridge_library_path(Some(version), &directory)
                )],
                "{title} selects the library its version names"
            );
        }
    }

    /// A window with no owning process is not offered, because there is nothing to load into.
    ///
    /// [`Shell::attach`] reads the pid as a fact about every row it can see, and this is the filter
    /// that makes it one: an enumeration whose only client names no process is an empty list, and an
    /// empty list is a refusal the panel already has words for.
    #[gpui::test]
    fn a_window_without_an_owning_process_is_not_offered(cx: &mut gpui::TestAppContext) {
        let mut ownerless = game_window();
        ownerless.pid = None;
        let handle = cx.update(|cx| {
            cx.new(|_| {
                let mut shell = Shell::new(Theme::slate());
                shell.set_windows(Some(window_surface(vec![ownerless])));
                shell
            })
        });

        let before = dials();
        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));

        let (view, targets) = cx.read(|cx| {
            let shell = handle.read(cx);
            (shell.injection_view(), shell.injection_targets().len())
        });
        assert_eq!(
            targets, 0,
            "a window the bridge cannot be loaded into is not a target"
        );
        assert_eq!(view.status, refusal_line(Refusal::NoWindow));
        assert!(view.enabled, "an empty list is a state to retry from");
        assert_eq!(dials(), before, "and nothing dialled");
    }

    /// `UpdateGameStateUI`'s re-enable wart is not ported: a press in flight stays disabled.
    ///
    /// Managed re-enabled the button on any tick where the bridge was disconnected and the client
    /// not injected (`MainWindow.xaml.cs:1543-1547`), so a button disabled for a press flipped back
    /// under the user's cursor. The port keeps `Detecting` and `Connecting` disabled until the
    /// press resolves — where a reported failure *is* a resolution and does re-enable, see
    /// `an_unanswered_dial_fails_and_the_press_is_pressable_again` — and a disconnected tick
    /// changes nothing.
    #[test]
    fn a_press_in_flight_is_not_re_enabled_by_a_tick() {
        for state in [InjectionStatus::Detecting, InjectionStatus::Connecting] {
            assert!(
                !InjectionView::resolve(&state, false, None).enabled,
                "{state:?} is in flight and must not be pressable"
            );
        }

        assert!(
            InjectionView::resolve(&InjectionStatus::NotInjected, false, None).enabled,
            "and an idle panel is pressable, or the wart would have been ported too far"
        );
        assert!(
            !InjectionView::resolve(&InjectionStatus::Detecting, true, None).enabled,
            "a connected link wins over the machine, whichever state it left behind"
        );
    }

    /// A dial nobody answers stops saying `Connecting...` and says why, and the press comes back.
    ///
    /// This is the exit `Connecting` did not have: `self.injection` is written only by `inject` and
    /// `attach`, and nothing in `IngestStatus` fails, so a dial that was never answered left the
    /// panel spinning with a dead button for as long as the process lived. The deadline is the
    /// runtime's ([`crate::runtime::CONNECT_DEADLINE_MS`]) and it is measured on the injected
    /// clock, so this crosses it with a step clock instead of sleeping three seconds.
    #[gpui::test]
    fn an_unanswered_dial_fails_and_the_press_is_pressable_again(cx: &mut gpui::TestAppContext) {
        let (runtime, clock) = Runtime::stepped();
        let handle = cx.update(|cx| {
            cx.new(|_| {
                let mut shell = Shell::new(Theme::slate());
                shell.set_runtime(runtime);
                shell.set_windows(Some(window_surface(vec![game_window()])));
                shell
            })
        });

        let before = dials();
        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));

        let opening = cx.read(|cx| handle.read(cx).injection_view());
        assert_eq!(
            opening.status,
            copy::LOADER_STATUS_CONNECTING,
            "the gate passed, so the dial owns the panel"
        );
        assert!(!opening.enabled, "and it owns the press until it answers");
        assert_eq!(dials(), before + 1, "one press, one dial");

        // One millisecond short: still a dial in flight, and still not pressable.
        clock.advance(std::time::Duration::from_millis(
            crate::runtime::CONNECT_DEADLINE_MS as u64 - 1,
        ));
        let waiting = cx.read(|cx| handle.read(cx).injection_view());
        assert_eq!(waiting.status, copy::LOADER_STATUS_CONNECTING);
        assert!(!waiting.enabled);

        clock.advance(std::time::Duration::from_millis(1));
        let failed = cx.read(|cx| handle.read(cx).injection_view());
        assert_eq!(
            failed.status,
            failure_line(LinkFailure::ConnectFailed {
                port: aoko_protocol::BRIDGE_PORT
            }),
            "at the deadline the panel names the endpoint nobody answered on"
        );
        assert_eq!(
            failed.button,
            copy::LOADER_INJECT,
            "the button is the only thing a user can do about a failed dial"
        );
        assert!(
            failed.enabled,
            "so a failure is pressable again, which is what makes it a state rather than a spinner"
        );
        assert!(!failed.accent, "the accent line is the connected one");

        // And the retry really dials: a press after a failure is not swallowed by the old attempt.
        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));
        assert_eq!(dials(), before + 2, "the second press dialled again");
        let retried = cx.read(|cx| handle.read(cx).injection_view());
        assert_eq!(
            retried.status,
            copy::LOADER_STATUS_CONNECTING,
            "and the panel is waiting on the new attempt, not on the dead one"
        );
    }

    /// A link that goes down after answering lands on the disconnected line.
    ///
    /// The reader reports it ([`crate::ingest::LinkFailure::Disconnected`]) and the store's
    /// `connected` goes false, but the machine is still `Connecting` — so without the reason the
    /// panel would fall back to a dial that has already finished. The reader here is the real one
    /// over a memory duplex, which is why the drop this asserts is the drop a socket produces
    /// rather than one a stub wrote.
    #[gpui::test]
    fn a_link_that_drops_paints_the_disconnected_line(cx: &mut gpui::TestAppContext) {
        let handle = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        cx.update(|cx| {
            handle.update(cx, |shell, _| {
                shell.set_windows(Some(window_surface(vec![game_window()])));
            })
        });

        // The press first, so the machine reaches `Connecting` on the button path; the reader is
        // attached after it, because `connect_bridge` clears the last answer when a new attempt
        // starts and that would wipe the drop this test is waiting for.
        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));
        assert_eq!(
            cx.read(|cx| handle.read(cx).injection_view()).status,
            copy::LOADER_STATUS_CONNECTING
        );

        cx.update(|cx| {
            handle.update(cx, |shell, _| {
                let (transport, _peer) = aoko_protocol::transport::memory::duplex();
                shell.runtime_mut().attach_ingest(transport);
            })
        });

        // The peer never feeds a line, so the reader leaves at end of stream and reports the drop.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let reported = cx.read(|cx| handle.read(cx).runtime().link_failure());
            if reported == Some(LinkFailure::Disconnected) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the reader never reported the drop: {reported:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        let (view, connected) = cx.read(|cx| {
            let shell = handle.read(cx);
            (
                shell.injection_view(),
                shell.runtime().ingest_status().connected,
            )
        });
        assert!(!connected, "the link really is down");
        assert_eq!(
            view.status,
            failure_line(LinkFailure::Disconnected),
            "and the panel says so instead of going back to a dial that already ran"
        );
        assert!(
            view.enabled,
            "a dropped link is retryable, or the panel has a second dead end"
        );
    }

    /// Every state the machine can enter paints its own line, and both reasons do too.
    ///
    /// `resolve` is the whole mapping, so a state that fell through to another arm — or a failure
    /// no state ever read — would be invisible here rather than at a call site.
    #[test]
    fn every_state_and_both_reasons_paint_their_own_line() {
        use std::collections::BTreeSet;

        let lines = [
            InjectionView::resolve(&InjectionStatus::NotInjected, false, None).status,
            InjectionView::resolve(&InjectionStatus::Detecting, false, None).status,
            InjectionView::resolve(&InjectionStatus::Connecting, false, None).status,
            InjectionView::resolve(&InjectionStatus::Refused(Refusal::NoWindow), false, None)
                .status,
            InjectionView::resolve(
                &InjectionStatus::Failed(
                    copy::LOADER_REASON_BRIDGE_LIBRARY_MISSING.replace("{path}", "/nowhere"),
                ),
                false,
                None,
            )
            .status,
            InjectionView::resolve(
                &InjectionStatus::Connecting,
                false,
                Some(LinkFailure::ConnectFailed {
                    port: aoko_protocol::BRIDGE_PORT,
                }),
            )
            .status,
            InjectionView::resolve(
                &InjectionStatus::Connecting,
                false,
                Some(LinkFailure::Disconnected),
            )
            .status,
        ];

        let unique: BTreeSet<&str> = lines.iter().map(String::as_str).collect();
        assert_eq!(
            unique.len(),
            lines.len(),
            "no two states share a line: {lines:?}"
        );
        assert_eq!(
            lines[4],
            format!(
                "{}{}",
                copy::LOADER_STATUS_PREFIX,
                copy::LOADER_REASON_BRIDGE_LIBRARY_MISSING.replace("{path}", "/nowhere")
            ),
            "an attach failure is the panel's prefix plus the sentence the press stored"
        );
        assert_eq!(
            lines[5],
            failure_line(LinkFailure::ConnectFailed {
                port: aoko_protocol::BRIDGE_PORT
            })
        );
        assert!(
            !lines[5].contains("{port}"),
            "the template's placeholder is filled, not painted: {}",
            lines[5]
        );
        assert_eq!(lines[6], failure_line(LinkFailure::Disconnected));
    }

    /// The connected line is the controller's answer, not a state the panel stores.
    ///
    /// `InjectionStatus` has no `Connected` variant on purpose: the link is read every frame, so a
    /// connected panel is a *mapping* of `Connecting` plus a live store, and nothing has to
    /// remember to change it back when the bridge drops.
    #[gpui::test]
    fn a_connected_link_is_the_terminal_state_of_every_press(cx: &mut gpui::TestAppContext) {
        let handle = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        cx.update(|cx| {
            handle.update(cx, |shell, _| {
                shell.set_windows(Some(window_surface(vec![game_window()])));
            })
        });
        cx.update(|cx| handle.update(cx, |shell, cx| shell.inject(cx)));

        // The store is not `Clone`: `StateStore` has interior mutability and the controller hands
        // out a reference, so the write goes inside the closure rather than into a binding.
        cx.read(|cx| {
            handle
                .read(cx)
                .runtime
                .controller()
                .store()
                .set_connected(true)
        });

        let view = cx.read(|cx| handle.read(cx).injection_view());
        assert_eq!(view.status, copy::LOADER_STATUS_CONNECTED);
        assert_eq!(view.button, copy::LOADER_INJECT_CONNECTED);
        assert!(view.accent, "the accent line is the connected one");
        assert!(!view.enabled, "there is nothing left to press");
    }

    /// The managed swap is a one-way latch: first connection only, and never back.
    ///
    /// `latch_control_mode` runs at the end of a frame so the frame that first sees the connection
    /// is still the loader's; this drives the latch itself, which needs no window.
    #[test]
    fn the_loader_latches_to_the_control_panel_once_and_never_returns() {
        let mut shell = Shell::new(Theme::slate());
        shell.loader_visible = true;

        shell.latch_control_mode();
        assert!(
            shell.loader_visible(),
            "no connection yet, so the loader stays"
        );

        // Re-borrowed per statement, not bound once: a bound `store` would borrow `shell` and the
        // `&mut` that `latch_control_mode` needs could not be taken while it lived.
        shell.runtime.controller().store().set_connected(true);
        shell.latch_control_mode();
        assert!(
            !shell.loader_visible(),
            "the first connection swaps the panel"
        );

        shell.runtime.controller().store().set_connected(false);
        shell.latch_control_mode();
        assert!(
            !shell.loader_visible(),
            "a later disconnect does not bring the loader back"
        );
    }

    /// The panel is mounted, and it is what the window paints in loader mode.
    ///
    /// Glyphs are not readable at the pin, so this asserts what the frame carries: the panel's own
    /// selectors are present and the control panel's cards are not. The terminal frame is also
    /// pinned — the swap happens at the *end* of the frame that first sees the connection, because
    /// latching at the start would make `Status: Connected & Injected`, a state managed really
    /// draws, unreachable.
    #[gpui::test]
    fn the_loader_paints_instead_of_the_tabs_and_swaps_after_the_connected_frame(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::Modifiers;

        let root = scratch_root();
        let (shell, _handle, mut visual) = shell_in_window(cx, &root);
        cx.update(|cx| {
            shell.update(cx, |shell, _| {
                shell.loader_visible = true;
                shell.set_windows(Some(window_surface(vec![game_window()])));
            })
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        assert!(
            visual.debug_bounds("loader-status").is_some(),
            "the panel's status line is in the frame"
        );
        assert!(
            visual.debug_bounds("loader-inject").is_some(),
            "and so is the Inject button"
        );
        assert!(
            visual.debug_bounds("keybind-AutoClickerCard").is_none(),
            "the control panel's cards are not: the loader replaces the tabs"
        );

        let before = dials();
        let press = visual
            .debug_bounds("loader-inject")
            .expect("the button paints");
        visual.simulate_click(press.center(), Modifiers::default());
        assert_eq!(dials(), before + 1, "the press reached the dial");
        assert_eq!(
            cx.read(|cx| shell.read(cx).injection_view().status),
            copy::LOADER_STATUS_CONNECTING,
            "and the panel says so"
        );

        // The bridge answers: this frame is still the loader, painting its terminal state. The write
        // goes inside the closure because the store is a reference, not a value.
        cx.read(|cx| {
            shell
                .read(cx)
                .runtime
                .controller()
                .store()
                .set_connected(true)
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(
            visual.debug_bounds("loader-status").is_some(),
            "the frame that sees the connection is the loader's last"
        );
        assert!(
            !cx.read(|cx| shell.read(cx).loader_visible()),
            "and the latch closes at the end of that frame"
        );

        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(
            visual.debug_bounds("loader-status").is_none(),
            "the next frame is the control panel"
        );
        assert!(
            visual.debug_bounds("keybind-AutoClickerCard").is_some(),
            "with the cards back"
        );
    }

    /// Switching tabs drops the keyboard from a field the new body does not paint.
    ///
    /// Without it the box keeps `Window::focused` while losing its node: characters fall back to
    /// the dispatch tree's synthetic root, and the shell's own handle never gets the keyboard back,
    /// so the window's Ctrl+K stays dead until the user clicks a field. The assertion is that
    /// symptom rather than the focus value — the press works again afterwards.
    #[gpui::test]
    fn switching_tabs_returns_the_keyboard_to_the_window(cx: &mut gpui::TestAppContext) {
        use gpui::Keystroke;

        let root = scratch_root();
        let (shell, handle, mut visual) = shell_in_window(cx, &root);
        // Focuses the Block ESP box, which lives on Render.
        focus_block_add(cx, &shell, &mut visual);

        visual.update(|window, cx| {
            assert!(
                !shell
                    .read(cx)
                    .search_gate(Some(window), cx)
                    .can_open_search(),
                "a focused field closes the gate while its tab is showing"
            );
        });

        // The rail's own path: it is the only control that changes tabs, and the only one with a
        // window to blur.
        visual.update(|window, cx| shell.update(cx, |shell, cx| shell.show_tab(0, window, cx)));
        visual.update(|window, cx| {
            assert!(
                shell
                    .read(cx)
                    .search_gate(Some(window), cx)
                    .can_open_search(),
                "the hidden box no longer holds the keyboard"
            );
            let _ = window.draw(cx);
        });

        visual
            .cx
            .dispatch_keystroke(handle, Keystroke::parse("ctrl-k").expect("keystroke"));
        assert!(
            cx.read(|cx| shell.read(cx).search().is_open()),
            "and Ctrl+K reaches the window again, which is what the user notices"
        );
    }
}
