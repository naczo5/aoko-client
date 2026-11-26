//! Runtime coordination: the shared game state, the active profile, the module
//! switches and the platform capabilities, assembled into UI view-model snapshots.
//!
//! The managed host spreads this across `AppController` (the WPF coordinator),
//! `GameStateClient` (the ingest thread) and `Clicker` (the property surface the
//! UI binds to). `aoko-core` is headless, so what survives the port is the *shape*
//! of the coordination:
//!
//! * [`StateStore`] is the single writer / many readers hand-off. The ingest side
//!   publishes a whole [`GameState`] with the [`Instant`] it arrived at, and every
//!   consumer reads one atomic snapshot. `arc-swap` keeps the read path
//!   lock-free, which matters because the click loop reads the state every tick
//!   and must never block on a UI thread holding the write side.
//! * [`AppController`] owns that store plus the profile and the enabled-module set,
//!   and answers the questions the view-model actually asks ("can I show the
//!   bridge as connected?", "how many modules are on?", "what does Discord
//!   publish?"). It never performs I/O itself.
//!
//! The timing rules are the managed ones: a state older than
//! [`crate::chest_stealer::FRESH_STATE_MS`] is reported as stale, and freshness is
//! measured from the snapshot's own arrival instant, so a clock that jumps cannot
//! make a stale state look fresh.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aoko_platform::{BridgeSupport, CapabilityReport, SessionKind, WindowInfo};
use aoko_protocol::module_catalog::{self, EnabledFlag};
use aoko_protocol::profile::Profile;
use aoko_protocol::state::GameState;
use arc_swap::ArcSwapOption;
use parking_lot::{Mutex, RwLock};

use crate::discord::{self, PresenceInputs, PresenceSnapshot};
use crate::loops::LoopSense;

/// A published state plus the moment it arrived.
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    /// The state as decoded from the bridge.
    pub state: Arc<GameState>,
    /// When the ingest side published it.
    pub at: Instant,
}

/// Lock-free hand-off from the bridge reader to every consumer.
#[derive(Debug, Default)]
pub struct StateStore {
    current: ArcSwapOption<StateSnapshot>,
    connected: AtomicBool,
    injected: AtomicBool,
    version: RwLock<Option<String>>,
}

impl StateStore {
    /// No state published yet, nothing connected.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Publishes a decoded state. The previous snapshot is dropped as soon as the
    /// last reader finishes with it.
    ///
    /// A caller that already has the state shared — the ingest side, holding the `Arc`
    /// a codec frame carries — should use [`Self::publish_shared`], which skips the
    /// re-wrap this performs.
    pub fn publish(&self, state: GameState, at: Instant) {
        self.publish_shared(Arc::new(state), at);
    }

    /// Publishes a state that is already shared.
    ///
    /// This is the zero-copy path: the decoder owns the snapshot and hands it out, so
    /// publishing costs the one `Arc<StateSnapshot>` the store holds and no copy of the
    /// state. Unwrapping and re-wrapping it here would put the copy back on every frame.
    pub fn publish_shared(&self, state: Arc<GameState>, at: Instant) {
        self.current
            .store(Some(Arc::new(StateSnapshot { state, at })));
    }

    /// Current snapshot, `None` before the first update.
    #[must_use]
    pub fn snapshot(&self) -> Option<Arc<StateSnapshot>> {
        self.current.load_full()
    }

    /// How long ago the current snapshot arrived.
    #[must_use]
    pub fn age(&self, now: Instant) -> Option<Duration> {
        self.current
            .load_full()
            .map(|snapshot| now.saturating_duration_since(snapshot.at))
    }

    /// Whether the bridge link is up.
    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Release);
    }

    /// Whether the native bridge reports the process as injected.
    pub fn set_injected(&self, injected: bool) {
        self.injected.store(injected, Ordering::Release);
    }

    /// Records the injected client version, as the bridge reports it.
    pub fn set_version(&self, version: Option<String>) {
        *self.version.write() = version;
    }

    /// Bridge link state.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    /// Injection state.
    #[must_use]
    pub fn is_injected(&self) -> bool {
        self.injected.load(Ordering::Acquire)
    }

    /// Injected client version.
    #[must_use]
    pub fn version(&self) -> Option<String> {
        self.version.read().clone()
    }
}

/// Everything the view-model needs in one value, so a render pass reads one
/// snapshot instead of chasing fields across threads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSnapshot {
    /// Bridge link state.
    pub connected: bool,
    /// Injection state.
    pub injected: bool,
    /// Injected client version.
    pub version: Option<String>,
    /// Age of the current state, `None` before the first update.
    pub state_age_ms: Option<u64>,
    /// Whether a GUI screen is open.
    pub gui_open: bool,
    /// Raw screen name, when a GUI is open.
    pub screen_name: Option<String>,
    /// Modules Discord counts as on.
    pub modules_on: usize,
    /// Whether the platform can drive clicks at all.
    pub can_click: bool,
    /// Whether the platform can sample current pointer and key state, i.e.
    /// [`CapabilityReport::pointer_state_sampling`].
    ///
    /// Deliberately *not* [`CapabilityReport::input_observation`]: events can be
    /// delivered while sampling is unavailable (Wayland with readable evdev), and
    /// the hold guards park on the sampling answer, so this is the flag the UI must
    /// show next to `can_click` to explain an inactive session. `input_observation`
    /// stays reachable through [`AppController::capabilities`] for the "events are
    /// being delivered" line, which is a different claim.
    pub pointer_state_sampling: bool,
    /// Whether the bridge transport is usable.
    pub bridge_usable: bool,
    /// Short reasons a control is disabled, for the UI's tooltip.
    pub notes: Vec<String>,
}

impl UiSnapshot {
    /// Freshness against [`crate::chest_stealer::FRESH_STATE_MS`].
    #[must_use]
    pub fn is_state_fresh(&self) -> bool {
        self.state_age_ms
            .is_some_and(|age| age <= crate::chest_stealer::FRESH_STATE_MS)
    }
}

/// The coordinator.
#[derive(Debug)]
pub struct AppController {
    store: StateStore,
    profile: RwLock<Profile>,
    modules: RwLock<Vec<EnabledFlag>>,
    capabilities: CapabilityReport,
}

impl AppController {
    /// A controller with a default profile, no modules on and the given
    /// capability report (use `aoko_platform::probe_capabilities()` in production).
    #[must_use]
    pub fn new(capabilities: CapabilityReport) -> Self {
        Self {
            store: StateStore::new(),
            profile: RwLock::new(Profile::default()),
            modules: RwLock::new(Vec::new()),
            capabilities,
        }
    }

    /// The store, for the ingest side and the automation loops.
    #[must_use]
    pub fn store(&self) -> &StateStore {
        &self.store
    }

    /// Capability report this controller was built with.
    #[must_use]
    pub fn capabilities(&self) -> &CapabilityReport {
        &self.capabilities
    }

    /// Replaces the capability report, e.g. after a display server change.
    pub fn set_capabilities(&mut self, capabilities: CapabilityReport) {
        self.capabilities = capabilities;
    }

    /// Clones the active profile. The click loop holds the clone for the duration
    /// of a tick so a settings change never tears a read.
    #[must_use]
    pub fn profile(&self) -> Profile {
        self.profile.read().clone()
    }

    /// Mutates the active profile in place, returning whether the mutation
    /// reported a change.
    pub fn update_profile(&self, update: impl FnOnce(&mut Profile) -> bool) -> bool {
        update(&mut self.profile.write())
    }

    /// Replaces the whole profile, as loading a saved config does.
    ///
    /// The apply path normalises dedicated action binds first
    /// ([`Profile::normalize_action_binds`]): `Profile.cs:889-899` clears both action
    /// keys, applies every module bind, then only keeps an action key that did not
    /// collide — general module binds win. The stored profile is the normalised one,
    /// because the managed `ApplyToClicker` also zeroes the field on the profile it
    /// was handed. Returns whether anything was changed by normalisation.
    pub fn replace_profile(&self, mut profile: Profile) -> bool {
        let normalised = profile.normalize_action_binds();
        *self.profile.write() = profile;
        normalised
    }

    /// Sets one module switch.
    pub fn set_module_enabled(&self, flag: EnabledFlag, enabled: bool) {
        let mut modules = self.modules.write();
        let present = modules.contains(&flag);
        if enabled && !present {
            modules.push(flag);
        } else if !enabled && present {
            modules.retain(|existing| *existing != flag);
        }
    }

    /// Whether one module switch is on.
    #[must_use]
    pub fn is_module_enabled(&self, flag: EnabledFlag) -> bool {
        self.modules.read().contains(&flag)
    }

    /// `ModuleCatalog.CountEnabledForDiscord`: modules that need the Discord
    /// surface, are visible at the current dev-mode setting, and are on.
    #[must_use]
    pub fn count_enabled_modules(&self) -> usize {
        let dev_mode = self.profile.read().dev_mode;
        let modules = self.modules.read();
        module_catalog::count_enabled_for_discord(dev_mode, |flag| modules.contains(&flag))
    }

    /// Builds the presence lines from the current state, exactly as
    /// `DiscordRichPresenceService` does.
    #[must_use]
    pub fn presence(
        &self,
        is_clicking: bool,
        is_armed: bool,
        gtb_helper_enabled: bool,
    ) -> PresenceSnapshot {
        let snapshot = self.store.snapshot();
        let fallback = GameState::default();
        let state = snapshot
            .as_ref()
            .map_or(&fallback, |snapshot| &*snapshot.state);
        let modules_on = self.count_enabled_modules();
        let version = self.store.version();

        discord::build_presence(&PresenceInputs {
            connected: self.store.is_connected(),
            injected: self.store.is_injected(),
            version: version.as_deref(),
            status_text: discord::mode_label(is_clicking, is_armed),
            enabled_modules: modules_on,
            is_clicking,
            is_armed,
            gtb_helper_enabled,
            state,
        })
    }

    /// Builds the UI snapshot. `now` comes from the injected clock so tests are
    /// deterministic.
    #[must_use]
    pub fn ui_snapshot(&self, now: Instant) -> UiSnapshot {
        let snapshot = self.store.snapshot();
        let state = snapshot
            .as_ref()
            .map(|snapshot| Arc::clone(&snapshot.state));
        let state_age_ms = snapshot
            .as_ref()
            .map(|snapshot| now.saturating_duration_since(snapshot.at).as_millis() as u64);

        UiSnapshot {
            connected: self.store.is_connected(),
            injected: self.store.is_injected(),
            version: self.store.version(),
            state_age_ms,
            gui_open: state.as_ref().is_some_and(|state| state.gui_open),
            screen_name: state.as_ref().map(|state| state.screen_name.clone()),
            modules_on: self.count_enabled_modules(),
            can_click: self.capabilities.input_synthesis,
            pointer_state_sampling: self.capabilities.pointer_state_sampling,
            bridge_usable: self.capabilities.bridge.is_usable(),
            notes: self.capabilities.notes.clone(),
        }
    }
}

impl Default for AppController {
    fn default() -> Self {
        Self::new(CapabilityReport::transport_only(SessionKind::None))
    }
}

/// Picks the game window out of an enumerated list, using the platform's own
/// preference order (`select_best_window`: title match first, then a live handle).
#[must_use]
pub fn best_game_window(windows: &[WindowInfo]) -> Option<&WindowInfo> {
    aoko_platform::select_best_window(windows)
}

/// Whether a bridge report makes the native path usable.
#[must_use]
pub fn bridge_supported(bridge: BridgeSupport) -> bool {
    bridge.is_usable()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Clock;
    use crate::testing::VirtualClock;
    use aoko_platform::Rect;
    use aoko_platform::WindowHandle;

    fn transport_only() -> CapabilityReport {
        CapabilityReport::transport_only(SessionKind::None)
    }

    fn window(handle: u64, title: &str) -> WindowInfo {
        WindowInfo {
            handle: WindowHandle(handle),
            title: title.to_owned(),
            class_name: None,
            pid: None,
            process_name: None,
            bounds: Rect::default(),
        }
    }

    #[test]
    fn an_empty_store_reports_no_state() {
        let store = StateStore::new();
        assert!(store.snapshot().is_none());
        assert!(store.age(Instant::now()).is_none());
        assert!(!store.is_connected());
        assert!(!store.is_injected());
        assert_eq!(store.version(), None);
    }

    #[test]
    fn publishing_replaces_the_snapshot_and_measures_its_age() {
        let clock = VirtualClock::new();
        let start = clock.now();
        let store = StateStore::new();

        store.publish(GameState::default(), start);
        let first = store.snapshot().expect("published");
        assert_eq!(store.age(start), Some(Duration::ZERO));

        // A later publish wins and the age resets.
        let newer = GameState {
            gui_open: true,
            ..GameState::default()
        };
        store.publish(newer, start + Duration::from_millis(120));
        let second = store.snapshot().expect("published");
        assert!(second.state.gui_open);
        assert!(!first.state.gui_open, "the old snapshot is unchanged");
        assert_eq!(
            store.age(start + Duration::from_millis(200)),
            Some(Duration::from_millis(80))
        );

        // A timestamp in the future saturates instead of wrapping.
        assert_eq!(store.age(start), Some(Duration::ZERO));
    }

    #[test]
    fn the_controller_derives_freshness_from_the_snapshot_instant() {
        let clock = VirtualClock::new();
        let start = clock.now();
        let app = AppController::default();

        let empty = app.ui_snapshot(start);
        assert_eq!(empty.state_age_ms, None);
        assert!(!empty.is_state_fresh());
        assert!(!empty.can_click);
        assert!(!empty.bridge_usable);

        app.store().publish(GameState::default(), start);
        let fresh = app.ui_snapshot(start + Duration::from_millis(10));
        assert_eq!(fresh.state_age_ms, Some(10));
        assert!(fresh.is_state_fresh());

        // Exactly at the limit is still fresh, one millisecond later is not.
        let edge = app.ui_snapshot(start + Duration::from_millis(300));
        assert!(edge.is_state_fresh());
        let stale = app.ui_snapshot(start + Duration::from_millis(301));
        assert!(!stale.is_state_fresh());
    }

    #[test]
    fn module_switches_drive_the_discord_count() {
        let app = AppController::default();
        assert_eq!(app.count_enabled_modules(), 0);

        app.set_module_enabled(EnabledFlag::LeftClick, true);
        let with_clicker = app.count_enabled_modules();
        assert!(with_clicker >= 1);

        // A non-Discord module does not change the count.
        app.set_module_enabled(EnabledFlag::Never, true);
        assert_eq!(app.count_enabled_modules(), with_clicker);

        // Setting the same switch off restores the previous count.
        app.set_module_enabled(EnabledFlag::LeftClick, false);
        assert_eq!(app.count_enabled_modules(), 0);
        assert!(!app.is_module_enabled(EnabledFlag::LeftClick));
    }

    #[test]
    fn the_presence_reflects_the_store_and_the_switch_set() {
        let app = AppController::default();
        let idle = app.presence(false, false, false);
        assert_eq!(idle.state, "Idle • Waiting for Minecraft client");

        app.store().set_connected(true);
        app.store().set_version(Some("1.21.4".to_owned()));
        let healthy = GameState {
            health: 20.0,
            ..GameState::default()
        };
        app.store().publish(healthy, Instant::now());
        let armed = app.presence(false, true, false);
        assert_eq!(armed.details, "aoko client • 1.21 • 0 modules on");
        assert_eq!(armed.state, "Armed • In game • HP 20");
    }

    #[test]
    fn the_profile_can_be_read_mutated_and_replaced() {
        let app = AppController::default();
        assert_eq!(app.profile().name, Profile::default().name);

        let changed = app.update_profile(|profile| {
            profile.dev_mode = true;
            true
        });
        assert!(changed);
        assert!(app.profile().dev_mode);

        let mut replacement = Profile::default();
        replacement.name = String::from("Loaded");
        app.replace_profile(replacement);
        assert_eq!(app.profile().name, "Loaded");
        assert!(!app.profile().dev_mode);
    }

    /// `AutoRodManagedTests.ProfileConflict_PreservesGeneralBindAndClearsActionBind`
    /// (`Aoko.Tests/AutoRodManagedTests.cs:117`), on the apply path.
    #[test]
    fn applying_a_profile_clears_a_colliding_action_bind_and_keeps_the_module_bind() {
        let app = AppController::default();

        let mut profile = Profile::default();
        profile.auto_rod_action_key = 0x52;
        profile.module_keys.insert(String::from("autorod"), 0x52);
        assert!(profile.has_action_bind_collision());

        assert!(
            app.replace_profile(profile),
            "normalisation must report the change"
        );

        let applied = app.profile();
        assert_eq!(
            applied.module_keys.get("autorod").copied(),
            Some(0x52),
            "the general module bind wins and is preserved"
        );
        assert_eq!(applied.auto_rod_action_key, 0);
        assert!(!applied.has_action_bind_collision());
    }

    #[test]
    fn applying_a_profile_without_a_collision_leaves_the_action_bind_alone() {
        let app = AppController::default();

        let mut profile = Profile::default();
        profile.auto_rod_action_key = 0x53;
        profile.module_keys.insert(String::from("autorod"), 0x52);

        assert!(!app.replace_profile(profile));
        assert_eq!(app.profile().auto_rod_action_key, 0x53);
    }

    #[test]
    fn the_game_window_is_chosen_by_title_then_handle() {
        let windows = [
            window(0, "Terminal"),
            window(0, "Minecraft 1.21"),
            window(7, "Minecraft 1.8.9"),
        ];
        let best = best_game_window(&windows).expect("a window matches");
        // Title match beats a live handle on a non-game window; among titles, a
        // live handle wins.
        assert_eq!(best.title, "Minecraft 1.8.9");

        assert!(best_game_window(&[]).is_none());
    }

    #[test]
    fn capability_reporting_is_surfaced_verbatim() {
        let mut report = transport_only();
        report.input_synthesis = true;
        let app = AppController::new(report);
        let snapshot = app.ui_snapshot(Instant::now());
        assert!(snapshot.can_click);
        assert!(!snapshot.bridge_usable);
        assert!(!snapshot.notes.is_empty());
        // `transport_only` cannot sample input, so an unobservable session is
        // explainable from the snapshot instead of looking like a stuck clicker.
        assert!(!snapshot.pointer_state_sampling);

        // Events delivered without sampling: the Wayland-with-evdev case. The
        // snapshot must follow `pointer_state_sampling`, not `input_observation`,
        // because that is what the hold guards actually consult.
        let mut events_only = transport_only();
        events_only.input_observation = true;
        assert!(
            !AppController::new(events_only)
                .ui_snapshot(Instant::now())
                .pointer_state_sampling
        );

        let mut sampling = transport_only();
        sampling.input_observation = true;
        sampling.pointer_state_sampling = true;
        assert!(
            AppController::new(sampling)
                .ui_snapshot(Instant::now())
                .pointer_state_sampling
        );

        assert!(bridge_supported(BridgeSupport::Supported));
        assert!(!bridge_supported(BridgeSupport::UnsupportedOnLinuxYet));
    }
}

/// Production [`crate::loops::LoopSense`]: the shared [`StateStore`] plus the
/// window and capability facts the app samples elsewhere.
///
/// The managed loops each called `WindowDetection` and `GameStateClient` statics
/// directly; a headless core cannot, so the app pushes those observations in with
/// the setters below and the loops read them back without a lock on the hot path.
#[derive(Debug)]
pub struct StoreSense {
    store: Arc<StateStore>,
    minecraft_active: AtomicBool,
    cursor_visible: AtomicBool,
    framebuffer: Mutex<Option<(i32, i32)>>,
    support: RwLock<Support>,
}

#[derive(Debug, Default)]
struct Support {
    modules: Vec<String>,
    state_fields: Vec<String>,
}

impl StoreSense {
    /// Wraps a store. Nothing is active, visible or supported until the app says so.
    #[must_use]
    pub fn new(store: Arc<StateStore>) -> Self {
        Self {
            store,
            minecraft_active: AtomicBool::new(false),
            cursor_visible: AtomicBool::new(false),
            framebuffer: Mutex::new(None),
            support: RwLock::new(Support::default()),
        }
    }

    /// `WindowDetection.IsMinecraftActive()`.
    pub fn set_minecraft_active(&self, active: bool) {
        self.minecraft_active.store(active, Ordering::Release);
    }

    /// `WindowDetection.IsCursorVisible()`.
    pub fn set_cursor_visible(&self, visible: bool) {
        self.cursor_visible.store(visible, Ordering::Release);
    }

    /// Client size, used when the state carries no viewport.
    pub fn set_framebuffer_size(&self, size: Option<(i32, i32)>) {
        *self.framebuffer.lock() = size;
    }

    /// Replaces the advertised module set, as a capabilities handshake does.
    pub fn set_supported_modules(&self, modules: Vec<String>) {
        self.support.write().modules = modules;
    }

    /// Replaces the advertised state-field set.
    pub fn set_supported_state_fields(&self, fields: Vec<String>) {
        self.support.write().state_fields = fields;
    }

    /// The store this sense reads.
    #[must_use]
    pub fn store(&self) -> &Arc<StateStore> {
        &self.store
    }
}

impl LoopSense for StoreSense {
    fn snapshot(&self) -> Option<Arc<StateSnapshot>> {
        self.store.snapshot()
    }

    fn is_connected(&self) -> bool {
        self.store.is_connected()
    }

    fn is_injected(&self) -> bool {
        self.store.is_injected()
    }

    fn minecraft_active(&self) -> bool {
        self.minecraft_active.load(Ordering::Acquire)
    }

    fn cursor_visible(&self) -> bool {
        self.cursor_visible.load(Ordering::Acquire)
    }

    fn supports_module(&self, module: &str) -> bool {
        self.support
            .read()
            .modules
            .iter()
            .any(|known| known.eq_ignore_ascii_case(module))
    }

    fn supports_state_field(&self, field: &str) -> bool {
        self.support
            .read()
            .state_fields
            .iter()
            .any(|known| known.eq_ignore_ascii_case(field))
    }

    fn framebuffer_size(&self) -> Option<(i32, i32)> {
        *self.framebuffer.lock()
    }
}
