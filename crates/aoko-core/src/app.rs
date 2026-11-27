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

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aoko_platform::{BridgeSupport, CapabilityReport, SessionKind, WindowInfo};
use aoko_protocol::config_payload::ConfigPayload;
use aoko_protocol::hud::HudLayout;
use aoko_protocol::module_catalog::{self, EnabledFlag};
use aoko_protocol::profile::Profile;
use aoko_protocol::state::GameState;
use arc_swap::ArcSwapOption;
use parking_lot::{MappedRwLockReadGuard, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::clicker::toggles;
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
    /// Held behind an `Arc` so [`AppController::store_handle`] can hand the ingest side
    /// and the loops the same store. Every store method takes `&self` and its state is
    /// atomics plus an `ArcSwapOption`, so sharing it changes no publish semantics: there
    /// is still exactly one store and one writer at a time.
    store: Arc<StateStore>,
    profile: RwLock<Profile>,
    modules: RwLock<Vec<EnabledFlag>>,
    capabilities: CapabilityReport,
    /// The `config` frame the current profile produces, dropped by every mutator:
    /// [`AppController::update_profile`] and [`AppController::replace_profile`] for the profile,
    /// [`AppController::set_hud_editor`] for the mode.
    ///
    /// Always locked before `profile` and before `hud_editor`. A rebuild holds this and then
    /// reads both; the mutators take this and then write theirs. One order, so no cycle.
    frame_cache: RwLock<FrameCache>,
    /// Whether the HUD editor mode is open. Runtime state, not a profile setting: the managed
    /// frame reported `clicker.HudEditorActive`, which the window toggled.
    hud_editor: AtomicBool,
    /// Bumped by [`AppController::request_mapping_reload`]; the frame carries it as
    /// `reloadMappingsNonce` so the bridge knows to re-read its mappings.
    mapping_reload_nonce: AtomicI32,
}

/// The cached `config` frame and whether it still describes the controller.
///
/// A freshness flag rather than an `Option<ConfigFrame>`: `config_frame` hands back a mapped
/// guard over the frame, and mapping an `Option` needs an unwrap on the read path, which this
/// crate does not allow outside `main`. Holding a frame that starts out valid costs one build
/// in the constructor and removes the panic.
#[derive(Debug)]
struct FrameCache {
    frame: ConfigFrame,
    fresh: bool,
}

impl AppController {
    /// A controller with a default profile and the given capability report (use
    /// `aoko_platform::probe_capabilities()` in production).
    ///
    /// The switch set is seeded from the profile, which is the apply half of the managed
    /// `Profile.ApplyToClicker`: a saved module that is on has to be running, and the frame has
    /// to say so, before any button is pressed. The profile's defaults have every module off,
    /// so this is empty for a fresh controller — the seeding matters for an adopted profile.
    #[must_use]
    pub fn new(capabilities: CapabilityReport) -> Self {
        let profile = Profile::default();
        let modules = crate::config_map::profile_switches(&profile);
        let frame = ConfigFrame {
            payload: crate::config_map::payload(&profile, &modules, false, 0),
            hud_layout: profile.to_hud_layout(),
        };
        Self {
            store: Arc::new(StateStore::new()),
            profile: RwLock::new(profile),
            modules: RwLock::new(modules),
            capabilities,
            frame_cache: RwLock::new(FrameCache { frame, fresh: true }),
            hud_editor: AtomicBool::new(false),
            mapping_reload_nonce: AtomicI32::new(0),
        }
    }

    /// Whether the HUD editor mode is open.
    ///
    /// This is runtime state, not a profile setting: the managed host read
    /// `clicker.HudEditorActive` (`GameStateClient.cs:1540`), which the window toggled
    /// (`MainWindow.xaml.cs:1406`), and no profile field ever carried it. The app owns the
    /// mode; the controller holds it so the frame can report it.
    #[must_use]
    pub fn hud_editor(&self) -> bool {
        self.hud_editor.load(Ordering::Relaxed)
    }

    /// Opens or closes the HUD editor mode, returning whether it changed.
    ///
    /// Takes the frame lock before the flag, the same order a rebuild uses, so the two cannot
    /// be read apart, and drops the cached frame because the mode is a keyed field.
    pub fn set_hud_editor(&self, value: bool) -> bool {
        let mut cache = self.frame_cache.write();
        if self.hud_editor.swap(value, Ordering::Relaxed) == value {
            return false;
        }
        cache.fresh = false;
        true
    }

    /// The nonce the frame currently carries as `reloadMappingsNonce`.
    #[must_use]
    pub fn mapping_reload_nonce(&self) -> i32 {
        self.mapping_reload_nonce.load(Ordering::Relaxed)
    }

    /// Asks the bridge to re-read its mappings, returning the new nonce.
    ///
    /// The protocol is the managed one (`MainWindow.xaml.cs:1397-1404`): when the bridge
    /// advertises `reloadMappingsNonce`, the host bumps a counter and the next `config` frame
    /// carries the new value, which is the bridge's signal to reload. Nothing else in the frame
    /// changes, so a difference in this key alone is the request.
    ///
    /// Takes the frame lock before the counter, the same order a rebuild uses, and drops the
    /// cached frame so the next frame carries the new nonce.
    pub fn request_mapping_reload(&self) -> i32 {
        let mut cache = self.frame_cache.write();
        let nonce = self
            .mapping_reload_nonce
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        cache.fresh = false;
        nonce
    }

    /// The panic reset: every switch the controller owns goes to its panic state, the
    /// profile's overlay, Discord and fight-status flags clear, and the cached frame is dropped.
    ///
    /// `Clicker.TriggerPanicAsync` (`Clicker.cs:530-609`) did this to the `Clicker`, then
    /// disconnected and shut the app down. The disconnect and the close are the shell's — they
    /// should end in the same path the window's own close action takes, so a panic exit and a
    /// normal exit do not diverge — and this stops at the state.
    ///
    /// The switch half is [`toggles::PANIC_ON`] / [`toggles::PANIC_OFF`] applied to the
    /// [`EnabledFlag`]s the controller holds, and the profile half is
    /// [`toggles::panic_profile_reset`]. Returns whether anything changed, so a caller can tell
    /// a panic that moved state from one that found it already down.
    pub fn panic_reset(&self) -> bool {
        let mut cache = self.frame_cache.write();
        let mut modules = self.modules.write();
        let mut profile = self.profile.write();

        let mut changed = false;
        for module_id in toggles::PANIC_ON {
            if let Some(flag) = module_catalog::flag_for_id(module_id) {
                changed |= write_flag(&mut modules, flag, true);
            }
        }
        for module_id in toggles::PANIC_OFF {
            if let Some(flag) = module_catalog::flag_for_id(module_id) {
                changed |= write_flag(&mut modules, flag, false);
            }
        }
        changed |= toggles::panic_profile_reset(&mut profile);

        if changed {
            cache.fresh = false;
        }
        changed
    }

    /// The store, for the ingest side and the automation loops.
    #[must_use]
    pub fn store(&self) -> &StateStore {
        &self.store
    }

    /// The same store, shared.
    ///
    /// For the consumers that outlive a borrow of the controller: the ingest worker
    /// thread and the automation loops hold their own handle, so they read the store
    /// without keeping the controller alive and without a second store that would
    /// publish to nobody. It is the same allocation [`Self::store`] points at, not a
    /// clone of the state.
    #[must_use]
    pub fn store_handle(&self) -> Arc<StateStore> {
        Arc::clone(&self.store)
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

    /// Clones the active profile, for a caller that has to own it.
    ///
    /// The automation loops do **not** use this: they map over the live profile once per tick
    /// through [`Self::with_profile`], because a clone per tick would allocate on the
    /// automation thread and a clone held across a tick is exactly the snapshot that let a
    /// settings change go unnoticed.
    #[must_use]
    pub fn profile(&self) -> Profile {
        self.profile.read().clone()
    }

    /// Runs `f` over the active profile without cloning it.
    ///
    /// The automation loops read their settings every tick
    /// (`aoko_core::loops::LoopSense::loop_configs`), and a full `Profile` clone per tick would
    /// allocate a dozen strings and three vectors on the automation thread for values that
    /// change once per edit. A read guard keeps the profile borrowed for exactly the length of
    /// the mapping, which is also what stops a settings change from tearing a tick's read.
    ///
    /// Nothing below `f` may take another lock: `replace_profile` holds `modules` and then
    /// `profile`, so a read that nested `modules` under `profile` the other way round could
    /// deadlock. Read the switches first, then call this.
    pub fn with_profile<T>(&self, f: impl FnOnce(&Profile) -> T) -> T {
        f(&self.profile.read())
    }

    /// `working` with every module switch copied into the field that stores it, ready to save.
    ///
    /// `working` is the caller's copy, not this controller's. The app owns the profile it is
    /// editing — settings pending a push, a just-loaded palette — and a save has to write those,
    /// while the switches are this controller's and a press never touches the profile. Taking
    /// the caller's copy is what keeps one writer of the profile (the shell) and one writer of
    /// the switches (here), instead of two views of the same state.
    ///
    /// A save is the only place the switch set flows back into a profile, and it is what the
    /// managed save sites did: each rebuilt the profile from the live clicker
    /// (`ProfileManager.CreateFromClicker()` then `SaveProfile(...)`,
    /// `MainWindow.xaml.cs:956-958`, `:1386-1388`, `:1414`, `:1867-1869`) instead of writing a
    /// copy the buttons never touched. Without it a module switched on and then quit came back
    /// off, because a press writes only the set.
    ///
    /// One table drives the copy (`crate::config_map::SWITCH_PROFILE_KEYS`), so a switch added
    /// there needs no second edit here. `isArmed` is in that table, so the left clicker follows
    /// the same rule as every other module.
    #[must_use]
    pub fn profile_for_save(&self, working: &Profile) -> Profile {
        let modules = self.modules.read();
        let mut profile = working.clone();
        for (flag, _) in crate::config_map::SWITCH_PROFILE_KEYS {
            crate::config_map::set_switch(&mut profile, flag, modules.contains(&flag));
        }
        // The HUD layout is the one field two writers share, and the controller is the copy
        // that is never stale: a drag in the game's HUD editor arrives as an inbound
        // `hudLayout` and is applied here ([`AppController::update_profile`] through the
        // reader), while a GUI edit reaches this controller too — the shell pushes its whole
        // working copy on every change. Taking it from the controller therefore keeps an
        // in-game drag that the shell never saw, which saving the working copy alone would
        // revert to the pre-drag layout (`ApplyInboundHudLayout`,
        // `GameStateClient.cs:1624-1648`).
        profile.hud_layout = self.profile.read().hud_layout.clone();
        profile
    }

    /// Mutates the active profile in place, returning whether the mutation
    /// reported a change.
    ///
    /// Always drops the cached `config` frame, whatever the closure reports: the closure can
    /// write any field, and a caller that returns `false` may still have moved the HUD.
    ///
    /// A closure that turns `dev_mode` off also forces the `DevOnly` modules off, which is the
    /// `Clicker.DevMode` setter (`Clicker.cs:180-193`): the cards for those modules are hidden
    /// from then on, so a switch left on is one no button can reach. Only the transition acts,
    /// as the managed setter returned early when the value did not change, and it runs after the
    /// closure's own lock is released, so a closure is free to touch the switches. The return
    /// value folds in whether that drop moved one.
    pub fn update_profile(&self, update: impl FnOnce(&mut Profile) -> bool) -> bool {
        let mut cache = self.frame_cache.write();
        let changed;
        let turned_off_dev_mode;
        {
            let mut profile = self.profile.write();
            let dev_mode_before = profile.dev_mode;
            changed = update(&mut profile);
            turned_off_dev_mode = dev_mode_before && !profile.dev_mode;
        }
        let mut dropped = false;
        if turned_off_dev_mode {
            // Dev Mode reads off here, which is the state the sweep acts on.
            let mut modules = self.modules.write();
            dropped = crate::config_map::disable_dev_only_modules(&mut modules, false);
        }
        cache.fresh = false;
        changed || dropped
    }

    /// Replaces the whole profile, as loading a saved config does.
    ///
    /// The apply path normalises dedicated action binds first
    /// ([`Profile::normalize_action_binds`]): `Profile.cs:889-899` clears both action
    /// keys, applies every module bind, then only keeps an action key that did not
    /// collide — general module binds win. The stored profile is the normalised one,
    /// because the managed `ApplyToClicker` also zeroes the field on the profile it
    /// was handed. Returns whether anything was changed by normalisation.
    ///
    /// It also **seeds the switches** from the profile (`crate::config_map::profile_switches`),
    /// which is the other half of `ApplyToClicker`: the managed host copied the saved module
    /// state into `Clicker` properties, and the frame read those. Adopting a profile is the
    /// apply step here, so the set is replaced rather than merged, and a module the loaded
    /// file turns off stops running. An in-place [`update_profile`](Self::update_profile) is a
    /// settings edit, not an apply, and leaves the switches alone.
    ///
    /// The last step of the apply is [`disable_dev_only_modules`](Self::disable_dev_only_modules)
    /// (`Profile.cs:903`), so a config that still stores `killAuraEnabled` cannot start Kill Aura
    /// while Dev Mode is off, when its card is hidden and its keybind is refused.
    pub fn replace_profile(&self, mut profile: Profile) -> bool {
        let normalised = profile.normalize_action_binds();
        let mut switches = crate::config_map::profile_switches(&profile);
        crate::config_map::disable_dev_only_modules(&mut switches, profile.dev_mode);
        let mut cache = self.frame_cache.write();
        let mut modules = self.modules.write();
        *self.profile.write() = profile;
        *modules = switches;
        cache.fresh = false;
        normalised
    }

    /// `Clicker.DisableDevOnlyModules` (`Clicker.cs:194-200`): forces every `DevOnly` module
    /// switch off while Dev Mode is off, returning whether one was running.
    ///
    /// The two ways a profile reaches this controller already run it —
    /// [`replace_profile`](Self::replace_profile) on the apply path, and
    /// [`update_profile`](Self::update_profile) when an edit turns Dev Mode off — so this is the
    /// same rule for a caller that has just moved the mode itself or wants the switch set
    /// settled without touching the profile. With Dev Mode on it is a no-op, which is why it is
    /// safe on any path.
    ///
    /// Only the switch set changes. The profile's saved copy of it is written on the next save
    /// ([`profile_for_save`](Self::profile_for_save)), the way the managed save sites rebuilt the
    /// profile from the `Clicker`.
    pub fn disable_dev_only_modules(&self) -> bool {
        let mut cache = self.frame_cache.write();
        let mut modules = self.modules.write();
        let dev_mode = self.profile.read().dev_mode;
        let changed = crate::config_map::disable_dev_only_modules(&mut modules, dev_mode);
        if changed {
            cache.fresh = false;
        }
        changed
    }

    /// Sets one module switch, returning whether it moved.
    ///
    /// The frame carries every module boolean from this set, so a press invalidates the cache
    /// the way [`set_hud_editor`](Self::set_hud_editor) does, in the same order: the frame
    /// lock first, then the data. Taking the data lock first would let a rebuild read the old
    /// set and mark the cache fresh, and the wire would then lag the button by one frame.
    pub fn set_module_enabled(&self, flag: EnabledFlag, enabled: bool) -> bool {
        let mut cache = self.frame_cache.write();
        let changed = write_flag(&mut self.modules.write(), flag, enabled);
        if changed {
            cache.fresh = false;
        }
        changed
    }

    /// The switch behind a checkbox bind, or `None` when that key names no switch.
    ///
    /// The shell writes two kinds of control through the profile: a module card, which is a
    /// switch ([`set_module_enabled`](Self::set_module_enabled)), and a `Control::Check`,
    /// which is a profile field. A few of those fields are the saved copy of a switch —
    /// `breakBlocksEnabled` is one, `"Pause when breaking"` in the Block ESP card is where it
    /// is painted — and the frame reads the switch, so the check has to move the switch too
    /// or the module neither starts nor reaches the bridge until a save and a reload.
    ///
    /// The key is the profile's own property name, matched case-insensitively because the bind
    /// is PascalCase (`"BreakBlocksEnabled"`) while the profile's serde names are camelCase.
    /// That is the rule [`module_catalog::flag_for_id`] already uses for module ids.
    ///
    /// `None` for every key that names no switch, which is why this is a lookup and not "any
    /// bind ending in `Enabled`": `PixelPartyAutoLookEnabled` and `PixelPartyAutoWalkEnabled`
    /// are sub-settings the profile owns and the frame reads from the profile, and they must
    /// stay that way.
    ///
    /// Returns what [`set_module_enabled`](Self::set_module_enabled) answers — whether the
    /// switch moved — so a caller can fold it into its own change detection.
    pub fn set_switch_for_profile_key(&self, key: &str, on: bool) -> Option<bool> {
        let flag =
            crate::config_map::SWITCH_PROFILE_KEYS
                .iter()
                .find_map(|(flag, profile_key)| {
                    profile_key.eq_ignore_ascii_case(key).then_some(*flag)
                })?;
        Some(self.set_module_enabled(flag, on))
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

    /// The `config` frame this host would send right now: the payload and the HUD layout it
    /// carries, served from a cache that the mutators drop.
    ///
    /// The frame is more than [`config_payload`](Self::config_payload) because the encoding
    /// takes a `HudLayout`, and the layout lives on the profile as the managed JSON blob. This
    /// is where `Profile::to_hud_layout` is applied, so a user who moved the HUD sends the
    /// layout they moved instead of the canonical default. A profile with no stored
    /// `hudLayout` — the default profile, and any profile older than the field — parses to
    /// `HudLayout::new`, which is the managed `BuildDefaults`.
    ///
    /// One profile read covers both halves at build time, so a settings change landing
    /// mid-build cannot pair a new payload with an old layout.
    ///
    /// # Cost
    ///
    /// A warm call allocates nothing: it takes the cache lock, finds the frame and hands back
    /// a guard, which is why the return type is [`ConfigFrameRef`] and not a clone. A rebuild
    /// is the only expensive path, and it is `2` allocations for the payload — `gtb_hint` and
    /// `gtb_preview` both own a `"-"`, every other default string is empty — plus
    /// `Profile::to_hud_layout`'s `1 + 5n` for `n` stored elements: 36 for a complete
    /// seven-element blob, none when the profile stores no `hudLayout`. So one invalidation
    /// costs nothing by itself and makes the next call a rebuild: 38 allocations with a stored
    /// layout, 2 without. Predicted from the code, not measured; CI reports the real numbers.
    ///
    /// Because [`ConfigFrame`] compares both halves, a sender that keeps the frame it sent
    /// sees a HUD-only change; a sender comparing [`config_payload`](Self::config_payload)
    /// cannot see one.
    #[must_use]
    pub fn config_frame(&self) -> ConfigFrameRef<'_> {
        let mut cache = self.frame_cache.write();
        if !cache.fresh {
            let modules = self.modules.read();
            let profile = self.profile.read();
            cache.frame = ConfigFrame {
                payload: Self::payload(
                    &profile,
                    modules.as_slice(),
                    self.hud_editor(),
                    self.mapping_reload_nonce(),
                ),
                hud_layout: profile.to_hud_layout(),
            };
            cache.fresh = true;
        }
        ConfigFrameRef(RwLockReadGuard::map(
            RwLockWriteGuard::downgrade(cache),
            |cache| &cache.frame,
        ))
    }

    /// The payload half of the `config` frame: everything but the HUD layout.
    ///
    /// Every setting comes from the profile, through `crate::config_map`, which names the
    /// `Clicker` property each key serialised and therefore the profile field behind it. Two
    /// runtime keys do not come from a field: `hudEditor` from [`hud_editor`](Self::hud_editor),
    /// and `reloadMappingsNonce` from
    /// [`mapping_reload_nonce`](Self::mapping_reload_nonce). The five keys nothing owns yet are
    /// listed in that module.
    ///
    /// All of it is one direction: the profile is the owner of the truth, this copies it out,
    /// and nothing writes a payload field back. A module the profile has no bind for sends
    /// `0`, which is the managed "unbound".
    ///
    /// A sender needs [`config_frame`](Self::config_frame), not this alone: the payload does
    /// not carry the layout, and encoding it against a default layout is the defect that method
    /// exists to close.
    #[must_use]
    pub fn config_payload(&self) -> ConfigPayload {
        let modules = self.modules.read();
        Self::payload(
            &self.profile.read(),
            modules.as_slice(),
            self.hud_editor(),
            self.mapping_reload_nonce(),
        )
    }

    /// The payload the profile and the switch set produce.
    ///
    /// Shared by [`config_payload`](Self::config_payload) and
    /// [`config_frame`](Self::config_frame) so the frame's halves cannot drift apart, and
    /// delegated to `crate::config_map`, which owns the one mapping into the payload.
    fn payload(
        profile: &Profile,
        modules: &[EnabledFlag],
        hud_editor: bool,
        reload_nonce: i32,
    ) -> ConfigPayload {
        crate::config_map::payload(profile, modules, hud_editor, reload_nonce)
    }
}

/// Writes one switch, reporting whether it moved.
///
/// The controller's switches are a `Vec<EnabledFlag>` used as a set; the panic reset and
/// [`AppController::set_module_enabled`] both go through here so one rule decides what "on"
/// means.
fn write_flag(modules: &mut Vec<EnabledFlag>, flag: EnabledFlag, enabled: bool) -> bool {
    let present = modules.contains(&flag);
    if enabled == present {
        return false;
    }
    if enabled {
        modules.push(flag);
    } else {
        modules.retain(|existing| *existing != flag);
    }
    true
}

/// One `config` frame: the payload the profile produces and the HUD layout it carries.
///
/// [`ConfigPayload`] holds no layout — [`ConfigPayload::to_line`] takes it as an argument — so
/// a sender that keeps only the payload encodes the canonical HUD for a user who moved theirs.
/// Keeping the halves together makes the frame the thing that is compared and sent, and
/// `PartialEq` spans both, so moving a HUD element counts as a change.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigFrame {
    /// The scalar half, exactly as [`AppController::config_payload`] returns it.
    pub payload: ConfigPayload,
    /// The layout parsed from the profile, as the frame carries it.
    pub hud_layout: HudLayout,
}

impl ConfigFrame {
    /// The frame as the single line the bridges parse.
    #[must_use]
    pub fn to_line(&self) -> String {
        self.payload.to_line(&self.hud_layout)
    }
}

/// A read guard on the cached [`ConfigFrame`].
///
/// [`AppController::config_frame`] hands this back rather than a clone, so a sender compares
/// and encodes through it without building a second frame. It dereferences to the
/// `ConfigFrame`, and the lock stays out of the public signature.
///
/// It is a read lock on the cache, so hold it for the compare and the encode and let it go
/// before the socket write: a stalled bridge would otherwise block every profile write behind
/// this lock. `(*frame).clone()` is how a sender keeps what it sent.
pub struct ConfigFrameRef<'a>(MappedRwLockReadGuard<'a, ConfigFrame>);

impl std::ops::Deref for ConfigFrameRef<'_> {
    type Target = ConfigFrame;

    fn deref(&self) -> &ConfigFrame {
        &self.0
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
    use aoko_protocol::hud::{element_id, HudElementLayout};
    use aoko_protocol::profile::{load_profile_from_json, to_profile_json};
    use serde_json::Value;

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

    /// The last line of `Profile.ApplyToClicker` (`Profile.cs:903`): a profile that still
    /// stores a `DevOnly` module on does not start it while Dev Mode is off.
    ///
    /// The managed host needed it because the GUI hides a `DevOnly` card unless Dev Mode is on
    /// and the keybind gate refuses the toggle for the same reason, so a module started from
    /// the file was one the user could not stop from either surface.
    #[test]
    fn a_loaded_profile_cannot_start_a_dev_only_module_with_dev_mode_off() {
        let app = AppController::default();
        assert!(
            module_catalog::is_dev_only("killaura"),
            "the module this drives has to be the catalog's dev-only one"
        );

        let mut stored = Profile::default();
        stored.kill_aura_enabled = true;
        app.replace_profile(stored);

        assert!(
            !app.is_module_enabled(EnabledFlag::KillAura),
            "the apply path drops the dev-only switch"
        );
        assert!(
            !app.config_payload().kill_aura,
            "the bridge hears it is off"
        );
        assert!(
            app.profile().kill_aura_enabled,
            "the stored field is the saved copy: the file it came from still says on"
        );

        // A save is what writes that copy back, so the next load of this slot starts clean.
        assert!(!app.profile_for_save(&app.profile()).kill_aura_enabled);

        // Dev Mode on is the one case that starts it.
        let mut dev = Profile::default();
        dev.dev_mode = true;
        dev.kill_aura_enabled = true;
        app.replace_profile(dev);
        assert!(app.is_module_enabled(EnabledFlag::KillAura));
        assert!(app.config_payload().kill_aura);
    }

    /// `Clicker.DevMode`'s setter (`Clicker.cs:180-193`): turning Dev Mode off mid-session
    /// stops the modules it was hiding, and an edit that leaves the mode alone does not.
    #[test]
    fn turning_dev_mode_off_stops_the_dev_only_module() {
        let app = AppController::default();
        app.update_profile(|profile| {
            profile.dev_mode = true;
            true
        });
        assert!(app.set_module_enabled(EnabledFlag::KillAura, true));

        app.update_profile(|profile| {
            profile.min_cps = 10.0;
            true
        });
        assert!(
            app.is_module_enabled(EnabledFlag::KillAura),
            "a settings edit is not a mode change"
        );

        let changed = app.update_profile(|profile| {
            profile.dev_mode = false;
            true
        });
        assert!(changed, "dropping a running switch is a change");
        assert!(!app.is_module_enabled(EnabledFlag::KillAura));
        assert!(!app.config_payload().kill_aura);
    }

    /// The sweep's own contract: a no-op with Dev Mode on, and it touches `DevOnly` switches
    /// only.
    #[test]
    fn the_dev_only_sweep_is_a_no_op_with_dev_mode_on() {
        let app = AppController::default();
        assert!(app.set_module_enabled(EnabledFlag::LeftClick, true));
        assert!(app.set_module_enabled(EnabledFlag::KillAura, true));
        app.update_profile(|profile| {
            profile.dev_mode = true;
            true
        });

        assert!(
            !app.disable_dev_only_modules(),
            "there is nothing to drop with Dev Mode on"
        );
        assert!(app.is_module_enabled(EnabledFlag::KillAura));

        app.update_profile(|profile| {
            profile.dev_mode = false;
            true
        });
        assert!(
            !app.disable_dev_only_modules(),
            "the edit that turned Dev Mode off already dropped it"
        );
        assert!(
            app.is_module_enabled(EnabledFlag::LeftClick),
            "only dev-only switches go"
        );
    }

    /// The gap the retired `Profile.cs` source pin was hiding, in its final shape: the profile
    /// carries `fight_status_enabled`, the payload carries `fightStatus`, and what connects
    /// them is the module switch — the profile seeds it on apply, the button moves it at
    /// runtime, and the frame reads it. The managed `clicker.FightStatusEnabled` was exactly
    /// that pair: a `Clicker` property `ApplyToClicker` filled and the toggle changed.
    #[test]
    fn the_config_payload_carries_the_fight_status_switch() {
        let app = AppController::default();

        // The connect-time value, in both hosts for the same reason: the managed frame sent
        // `clicker.FightStatusEnabled`, whose backing field is `false`, and `ProfileManager`
        // loaded that from `Profile.FightStatusEnabled = false`. Here the default profile's
        // field seeds no switch, so the frame sends the same `false`.
        assert!(!app.profile().fight_status_enabled);
        let payload = app.config_payload();
        assert!(!payload.fight_status);
        // The frame is the profile, so a default profile produces the profile's own values
        // rather than an all-zero payload; these are the managed connect-time defaults.
        let profile = app.profile();
        assert_eq!(payload.min_cps, profile.min_cps);
        assert_eq!(payload.gui_theme, profile.gui_theme);
        assert_eq!(payload.left, profile.left_click_enabled);

        // The switch is what moves it, in either direction.
        assert!(app.set_module_enabled(EnabledFlag::FightStatus, true));
        let payload = app.config_payload();
        assert!(payload.fight_status);
        assert!(
            payload
                .to_line(&HudLayout::default())
                .contains("\"fightStatus\":true"),
            "the frame the bridge parses has to carry it, not just the struct"
        );

        assert!(app.set_module_enabled(EnabledFlag::FightStatus, false));
        assert!(!app.config_payload().fight_status);

        // The saved field on its own does not: it is the value the switch is seeded from, not
        // a second source inside the mapping. Replacing the profile seeds it, which is the
        // apply step, and that is how a saved fight status reaches the wire.
        assert!(app.update_profile(|profile| {
            profile.fight_status_enabled = true;
            true
        }));
        assert!(
            !app.config_payload().fight_status,
            "an in-place profile edit is not an apply: the switch is what the frame reads"
        );
        let mut loaded = Profile::default();
        loaded.fight_status_enabled = true;
        app.replace_profile(loaded);
        assert!(
            app.config_payload().fight_status,
            "adopting a profile seeds its switches, the way ApplyToClicker filled the Clicker"
        );

        // And a panic reset moves it, which is the case the managed host had to get right:
        // `TriggerPanicAsync` cleared `Clicker.FightStatusEnabled`, so the frame it sent said
        // false afterwards.
        assert!(
            app.panic_reset(),
            "the panic has to find something to turn off"
        );
        assert!(
            !app.config_payload().fight_status,
            "a panic reset turns fight status off in the frame the bridge receives"
        );
    }

    /// The same rule for the rest of the config frame: the managed host read every keybind
    /// through `InputHooks.GetModuleKey(module)` (`GameStateClient.cs:1509-1539`), so each of
    /// the 31 frame keybinds is the profile's bind, never the payload's own default.
    #[test]
    fn the_config_payload_carries_the_profile_keybinds() {
        // Every module the managed host sent a bind for, in the order it read them, paired
        // with the key that carries it. Distinct codes below mean a field left at its
        // default, a field wired to its neighbour's module, and a wrong wire key all fail.
        const BINDS: [(&str, &str); 31] = [
            ("autoclicker", "keybindAutoclicker"),
            ("rightclick", "keybindRightClick"),
            ("aimassist", "keybindAimAssist"),
            ("triggerbot", "keybindTriggerbot"),
            ("killaura", "keybindKillAura"),
            ("speedbridge", "keybindSpeedBridge"),
            ("gtbhelper", "keybindGtbHelper"),
            ("nametags", "keybindNametags"),
            ("closestplayer", "keybindClosestPlayer"),
            ("fightstatus", "keybindFightStatus"),
            ("chestesp", "keybindChestEsp"),
            ("cheststealer", "keybindChestStealer"),
            ("refill", "keybindRefill"),
            ("blockesp", "keybindBlockEsp"),
            ("bedplates", "keybindBedPlates"),
            ("pixelpartyassist", "keybindPixelPartyAssist"),
            ("autorod", "keybindAutoRod"),
            ("throwpot", "keybindThrowpot"),
            ("autoheal", "keybindAutoheal"),
            ("autotool", "keybindAutoTool"),
            ("fireballwarning", "keybindFireballWarning"),
            ("fastplace", "keybindFastPlace"),
            ("nohurtcam", "keybindNoHurtCam"),
            ("sprint", "keybindSprint"),
            ("tnttimer", "keybindTntTimer"),
            ("wtap", "keybindWTap"),
            ("jumpreset", "keybindJumpReset"),
            ("hitselect", "keybindHitSelect"),
            ("trajectories", "keybindTrajectories"),
            ("murdermystery", "keybindMurderMystery"),
            ("breakprogress", "keybindBreakProgress"),
        ];

        let app = AppController::default();

        // A default profile produces the managed default, which is key 0 for all 31.
        let line = app.config_payload().to_line(&HudLayout::default());
        for (module, key) in BINDS {
            assert!(
                line.contains(&format!("\"{key}\":0")),
                "{key} must default to the unbound key for {module}"
            );
        }

        assert!(app.update_profile(|profile| {
            for (index, (module, _)) in BINDS.iter().enumerate() {
                profile
                    .module_keys
                    .insert((*module).to_owned(), 100 + index as i32);
            }
            true
        }));

        let line = app.config_payload().to_line(&HudLayout::default());
        for (index, (module, key)) in BINDS.iter().enumerate() {
            let code = 100 + index as i32;
            assert!(
                line.contains(&format!("\"{key}\":{code}")),
                "{module} must reach {key} as {code}"
            );
        }
    }

    /// `hudEditor` is runtime state, not a profile setting: the managed frame reported
    /// `clicker.HudEditorActive`, which the window toggled. The controller owns the mode now,
    /// and the frame has to follow it — a mapping that left it `false` would tell the bridge
    /// the editor is closed for the whole session.
    #[test]
    fn the_config_frame_reports_the_hud_editor_mode() {
        let app = AppController::default();
        // Read the frame first so the cache is warm: the assertions below then also prove the
        // mode drops it.
        assert!(
            app.config_frame().to_line().contains("\"hudEditor\":false"),
            "the editor starts closed"
        );

        assert!(app.set_hud_editor(true), "opening the editor is a change");
        assert!(app.hud_editor());
        assert!(
            app.config_frame().to_line().contains("\"hudEditor\":true"),
            "the frame has to report the mode, and the cached frame has to be dropped"
        );
        assert!(
            !app.set_hud_editor(true),
            "setting the mode it already has reports no change"
        );

        assert!(app.set_hud_editor(false));
        assert!(
            app.config_frame().to_line().contains("\"hudEditor\":false"),
            "closing it has to reach the frame too"
        );
    }

    /// A switch press has to reach a frame that is already built. The sender compares the
    /// frame it last sent, so a press that left the cache warm would keep telling the bridge
    /// the module was off, which is the defect this wiring closes.
    #[test]
    fn a_switch_press_reaches_the_warm_frame() {
        let app = AppController::default();
        let before = app.config_frame().to_line();
        assert!(
            !before.contains("\"aimAssist\":true"),
            "the default profile has no module on: {before}"
        );
        let layout = app.config_frame().hud_layout.clone();

        assert!(
            app.set_module_enabled(EnabledFlag::AimAssist, true),
            "the press moves the switch"
        );
        let after = app.config_frame();
        assert!(
            after.to_line().contains("\"aimAssist\":true"),
            "a switch press has to reach a frame that was already built"
        );
        assert_eq!(
            &after.hud_layout, &layout,
            "a switch press must not disturb the HUD layout"
        );

        // The guard is a read lock on the cache; the next line writes it.
        drop(after);

        assert!(
            !app.set_module_enabled(EnabledFlag::AimAssist, true),
            "a press that changes nothing reports nothing"
        );
    }

    /// A save carries the live switches, which is the other half of the ownership: the set is
    /// what runs, and the file has to keep what runs. The managed save sites rebuilt the
    /// profile from the `Clicker` for this reason.
    ///
    /// The property, not the shape, and it has two halves: a profile that disagrees with the
    /// switches must come back agreeing with them, and the caller's copy must win over the
    /// controller's — the shell holds edits the controller has not seen, and a save that read
    /// the controller's copy would write the settings the user just changed away.
    #[test]
    fn saving_writes_the_live_switches_into_the_working_profile() {
        // The stored profile says the opposite of the switches below: armed and aim assist
        // off, fight status on.
        let mut stored = Profile::default();
        stored.is_armed = false;
        stored.aim_assist_enabled = false;
        stored.fight_status_enabled = true;
        let app = AppController::default();
        app.replace_profile(stored);
        assert_eq!(
            crate::config_map::profile_switches(&app.profile()),
            vec![EnabledFlag::FightStatus],
            "adopting the profile seeds its one live module"
        );

        assert!(app.set_module_enabled(EnabledFlag::LeftClick, true));
        assert!(app.set_module_enabled(EnabledFlag::AimAssist, true));
        assert!(app.set_module_enabled(EnabledFlag::FightStatus, false));

        // The caller's copy, carrying an edit the controller has not been told about. A save
        // that cloned the controller's profile instead would write the old theme back.
        let mut working = app.profile();
        working.gui_theme = "Ember".to_owned();

        let saved = app.profile_for_save(&working);
        assert!(saved.is_armed, "the left clicker was on when the save ran");
        assert!(saved.aim_assist_enabled, "and so was aim assist");
        assert!(
            !saved.fight_status_enabled,
            "fight status was switched off before the save"
        );
        assert_eq!(
            saved.gui_theme, "Ember",
            "the working copy is what a save writes, not the controller's copy"
        );

        let reloaded = load_profile_from_json(&to_profile_json(&saved))
            .expect("the saved profile loads again");
        assert!(reloaded.is_armed);
        assert!(reloaded.aim_assist_enabled);
        assert!(!reloaded.fight_status_enabled);
        assert_eq!(
            crate::config_map::profile_switches(&reloaded),
            vec![EnabledFlag::LeftClick, EnabledFlag::AimAssist],
            "every switch the table carries is the switch the file keeps"
        );
        // The rest of the profile is the user's settings, untouched by the overlay.
        assert_eq!(reloaded.min_cps, working.min_cps);
        assert_eq!(reloaded.gui_theme, "Ember");
    }

    /// A HUD drag in the game reaches the file, even though the shell never saw it.
    ///
    /// This is the one field the save path takes from the controller instead of the working
    /// copy: an inbound `hudLayout` is applied to the controller's profile by the reader, so a
    /// save that wrote the shell's pre-drag copy would silently undo the user's drag the next
    /// time the app closed. `ApplyInboundHudLayout` is the managed counterpart.
    #[test]
    fn a_save_takes_the_hud_layout_from_the_controller() {
        let app = AppController::default();
        let stale = app.profile();

        // The reader's write, the shape `ingest::adopt_inbound_hud_layout` makes.
        let mut layout = stale.to_hud_layout();
        layout.set(
            element_id::MODULE_LIST,
            HudElementLayout::new(640.0, 300.0, 1.0),
        );
        let adopted = Profile::hud_layout_map(&layout);
        assert!(app.update_profile(|profile| {
            profile.hud_layout = Some(adopted.clone());
            true
        }));

        let saved = app.profile_for_save(&stale);
        assert_eq!(
            saved.hud_layout,
            Some(adopted),
            "the controller's layout is the one the game last moved"
        );
    }

    /// The checkbox half of the switch wiring: a bind that names a switch has to move it, and a
    /// bind that names a sub-setting has to leave it alone.
    ///
    /// The frame assertion is the one that matters. It goes through the payload the bridge
    /// parses, so the helper and `module_catalog::MODULE_WIRE_KEYS` are proved to agree about
    /// the key the bridge reads — not merely to be built from the same constant.
    #[test]
    fn a_profile_key_can_move_the_switch_behind_it() {
        let app = AppController::default();
        // The nick hider's key needs an alias to be allowed on; that half is the profile's.
        assert!(app.update_profile(|profile| {
            profile.nick_hider_alias = "Player".to_owned();
            true
        }));

        for (flag, wire_key) in module_catalog::MODULE_WIRE_KEYS {
            let profile_key = crate::config_map::SWITCH_PROFILE_KEYS
                .iter()
                .find_map(|(mapped, key)| (*mapped == flag).then_some(*key))
                .expect("every switch that feeds a key has a profile field");
            // The binds are PascalCase, so the lookup has to fold case.
            assert_eq!(
                app.set_switch_for_profile_key(&profile_key.to_uppercase(), true),
                Some(true),
                "{profile_key} has to name {flag:?}"
            );
            let line = app.config_payload().to_line(&HudLayout::new());
            let frame: Value = serde_json::from_str(&line).expect("the frame is JSON");
            assert_eq!(
                frame.get(wire_key).and_then(Value::as_bool),
                Some(true),
                "{profile_key} has to move {wire_key} on the wire"
            );
        }

        // Sub-settings and unknown keys are not switches: the profile owns them and the frame
        // reads the profile, so a blanket "any `*Enabled` bind" would be wrong here.
        for key in [
            "PixelPartyAutoLookEnabled",
            "PixelPartyAutoWalkEnabled",
            "NotASwitch",
        ] {
            assert_eq!(
                app.set_switch_for_profile_key(key, true),
                None,
                "{key} names no switch"
            );
        }

        // The bind this was found through: `"Pause when breaking"` in the Block ESP card.
        assert_eq!(
            app.set_switch_for_profile_key("BreakBlocksEnabled", false),
            Some(true),
            "the checkbox that pauses breaking has to reach its switch"
        );
        assert!(
            !app.config_payload().break_blocks,
            "and the frame has to follow it back off"
        );
    }

    /// `reloadMappingsNonce` is how "Reload Mappings" reaches the bridge: the host bumps a
    /// counter (`MainWindow.xaml.cs:1397-1404`) and the next frame's new value is the request.
    #[test]
    fn a_mapping_reload_moves_the_nonce_and_nothing_else() {
        let app = AppController::default();
        let before_line = app.config_frame().to_line();
        let before: Value = serde_json::from_str(&before_line).expect("the frame is JSON");
        assert_eq!(app.mapping_reload_nonce(), 0);

        // The frame above warmed the cache, so this request also has to drop it.
        assert_eq!(
            app.request_mapping_reload(),
            1,
            "the request reports the nonce"
        );
        let after_line = app.config_frame().to_line();
        let after: Value = serde_json::from_str(&after_line).expect("the frame is JSON");

        assert_eq!(after["reloadMappingsNonce"], 1);
        let before = before.as_object().expect("the frame is an object");
        let after = after.as_object().expect("the frame is an object");
        for (key, value) in before {
            if key == "reloadMappingsNonce" {
                continue;
            }
            assert_eq!(
                after.get(key),
                Some(value),
                "{key} must not move for a mapping reload"
            );
        }

        // Every request is a new value, so the bridge sees each one.
        assert_eq!(app.request_mapping_reload(), 2);
        assert!(app
            .config_frame()
            .to_line()
            .contains("\"reloadMappingsNonce\":2"));
    }

    /// The panic path resets the state the controller owns. The disconnect and the window
    /// close are the shell's, so this stops at the switches and the profile.
    #[test]
    fn the_panic_reset_clears_the_switches_and_the_profile_flags() {
        let app = AppController::default();
        let off_after_panic = [
            EnabledFlag::RightClick,
            EnabledFlag::AimAssist,
            EnabledFlag::KillAura,
            EnabledFlag::WTap,
        ];
        for flag in off_after_panic {
            app.set_module_enabled(flag, true);
        }
        // Warm the frame, so the reset has to drop it as well as move the state.
        let _ = app.config_frame().to_line();

        assert!(app.panic_reset(), "a panic that moves state reports it");

        assert!(
            app.is_module_enabled(EnabledFlag::LeftClick),
            "the left clicker stays usable, which is the managed panic state"
        );
        for flag in off_after_panic {
            assert!(
                !app.is_module_enabled(flag),
                "{flag:?} must be off after a panic"
            );
        }

        let profile = app.profile();
        assert!(!profile.show_module_list);
        assert!(!profile.show_logo);
        assert!(!profile.discord_rpc_enabled);
        assert!(!profile.fight_status_enabled);
        assert!(!profile.pixel_party_auto_look_enabled);
        assert!(!profile.pixel_party_auto_walk_enabled);
        assert!(!profile.nametag_hide_vanilla);
        assert!(
            !app.config_frame()
                .to_line()
                .contains("\"fightStatus\":true"),
            "the frame must not keep claiming fight status after a panic"
        );

        assert!(
            !app.panic_reset(),
            "a second panic finds everything already down"
        );
    }

    /// The gap the app builder found: `to_line` takes a `HudLayout` and nothing converted
    /// `Profile::hud_layout`, so a customised HUD shipped the canonical default.
    #[test]
    fn the_config_frame_carries_the_profile_hud_layout() {
        let defaults = AppController::default();
        let default_line = defaults.config_frame().to_line();

        // The default's module list sits at the top right, and a default profile — no stored
        // blob — sends exactly that, which is the managed `BuildDefaults`.
        assert!(default_line.contains("\"modulelist\":{\"x\":0.985,\"y\":0.02,\"scale\":1}"));
        assert_eq!(
            defaults.config_frame().hud_layout,
            HudLayout::new(),
            "a default profile must send the managed default layout"
        );
        assert_eq!(
            default_line,
            defaults.config_payload().to_line(&HudLayout::new()),
            "and encode it exactly as the default layout encodes"
        );

        // A customised one sends what the user set. The cache is warm before the write, so a
        // frame cache that is not dropped on a profile write keeps the default and fails here.
        let app = AppController::default();
        assert_eq!(app.config_frame().to_line(), default_line);

        let mut layout = HudLayout::new();
        layout.set(
            element_id::MODULE_LIST,
            HudElementLayout::new(0.25, 0.4, 1.5),
        );
        assert!(app.update_profile(|profile| {
            profile.hud_layout = Some(Profile::hud_layout_map(&layout));
            true
        }));

        let frame = app.config_frame();
        assert_eq!(
            frame.hud_layout, layout,
            "the stored blob has to round-trip back to the layout the user set"
        );
        assert!(
            frame
                .to_line()
                .contains("\"modulelist\":{\"x\":0.25,\"y\":0.4,\"scale\":1.5}"),
            "the customised element has to reach the wire, not just the profile"
        );
        assert_ne!(
            frame.to_line(),
            default_line,
            "a moved HUD must not encode to the default layout"
        );

        // The sender compares frames, so a HUD-only change has to read as one: the payload is
        // identical here, and only the layout differs.
        assert_eq!(frame.payload, defaults.config_payload());
        assert_ne!(
            *frame,
            *defaults.config_frame(),
            "a moved HUD must reach the bridge even when the payload is unchanged"
        );

        // The guard is a read lock on the cache, and the next line writes it: let it go first.
        drop(frame);

        // `replace_profile` is the other writer of the profile, and it drops the cache too.
        let mut loaded = Profile::default();
        loaded.hud_layout = Some(Profile::hud_layout_map(&HudLayout::new()));
        let _ = app.replace_profile(loaded);
        assert_eq!(
            app.config_frame().hud_layout,
            HudLayout::new(),
            "loading a profile has to drop the cached frame as well"
        );
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
