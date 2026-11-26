//! The runtime owner: the one `AppController` the shell reads and writes, plus the
//! optional bridge reader that feeds it and the loop supervisor its switches drive.
//!
//! Before this module, `aoko_core::app::AppController` had no reference outside
//! `aoko-core`. The shell kept its own `ModuleToggles`, so every module switch wrote a
//! set that nothing else in the process could see: 43 switches, no runtime on the other
//! end, and `TcpBridgeTransport::connect_bridge()` with no caller.
//!
//! [`crate::runtime::Runtime`] owns module state. It holds the `Arc<AppController>` and
//! resolves the switches' catalog ids onto the controller's own flag set
//! (`AppController::is_module_enabled` / `set_module_enabled`), so there is **one**
//! writable copy of module state in the app. `aoko_core::clicker::toggles::ModuleToggles`
//! is still the clicker loop's type inside `aoko-core`, but `aoko-app` constructs none: a
//! grep for `ModuleToggles` in this crate finds no instance.
//!
//! The bridge link is the second half: `crate::runtime::Runtime::connect_bridge` hands the
//! controller to a `crate::ingest::Ingest` worker, which decodes wire lines and publishes
//! `GameState` into the store, and `crate::runtime::Runtime::ingest_status` reports what
//! that store holds. Nothing dials by itself — the shipped path asks for it once, a
//! test-built runtime never does — so a switch flip is still observable with no bridge
//! present.
//!
//! Supervision is the third: `crate::runtime::Runtime::start_supervision` makes a module
//! switch the trigger for `aoko_core::loops::ClickerLoops`, which owns the five automation
//! threads, so enabling a loop-driving module starts them and turning the last one off
//! stops them. It is off until the shipped path turns it on, and the bodies run against a
//! backend that refuses every synthetic action: supervision is this slice, injection is the
//! next one.
//!
//! Two constructors, deliberately:
//!
//! * [`crate::runtime::Runtime::probed`] asks the platform what this session can do
//!   (`aoko_platform::probe_capabilities`) and is the shipped path, called from
//!   [`crate::shell::Shell::from_system`] where a window exists.
//! * [`crate::runtime::Runtime::headless`] takes the default report, so a unit test and the capture
//!   harness never depend on the machine they run on.
//!
//! ```text
//! let mut runtime = Runtime::headless();
//! runtime.set_module_enabled("autototem", true);
//! assert!(runtime.controller().is_module_enabled(EnabledFlag::AutoTotem));
//! ```

use std::sync::Arc;
use std::time::Instant;

use aoko_core::app::{AppController, StateSnapshot};
use aoko_core::clicker::toggles;
use aoko_core::loops::{
    ClickSink, ClickerLoopConfigs, ClickerLoops, LoopRuntime, LoopSense, NoClickSink,
};
use aoko_core::{Clock, Sleeper, SplitMix64, SystemClock, ThreadSleeper};
use aoko_platform::{InputBackend, InputEvent, Key, MouseButton, MouseState, PlatformError};
use aoko_protocol::module_catalog::{self, EnabledFlag};

use crate::ingest::{Ingest, IngestStatus};

/// Seed the five loop bodies start from.
///
/// One RNG per body, as `ClickerLoops::start` expects: the bodies draw click intervals from
/// it, so two bodies sharing a seed would pace identically. Deterministic on purpose — a
/// loop test that failed once can be replayed.
const LOOP_SEED: u64 = 0x00A0_C0DE;

/// Why a synthetic action is refused in this slice.
const NO_INJECTION: &str = "injection is not wired yet (supervision slice)";

/// The application's runtime state: the controller, the id mapping the switches need to
/// reach it, the bridge reader, and — on the shipped path only — the loop supervisor.
pub struct Runtime {
    controller: Arc<AppController>,
    /// The bridge reader, `None` until `connect_bridge` is called and on every
    /// runtime a test builds.
    ingest: Option<Ingest>,
    /// Whether a module switch starts and stops loop threads.
    ///
    /// `Shell::from_system` turns this on. Every test-built and capture-built runtime
    /// leaves it off, so a headless Runtime cannot start a thread however its switches move.
    supervise: bool,
    /// The running loop bundle, `None` while nothing is enabled.
    loops: Option<ClickerLoops>,
    /// The time source the loop bodies park on; a test swaps in its own before enabling.
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
}

impl Runtime {
    /// A runtime over a controller the caller built.
    ///
    /// This is the seam a test uses to hold the same controller the UI writes through.
    /// It builds no bridge reader and starts no loop: `connect_bridge` and
    /// `start_supervision` are the only two things with side effects.
    #[must_use]
    pub fn new(controller: Arc<AppController>) -> Self {
        Self::with_time(controller, Arc::new(SystemClock), Arc::new(ThreadSleeper))
    }

    /// A runtime with an explicit time source for its loop bodies.
    ///
    /// Production passes `SystemClock`/`ThreadSleeper` throughout [`Runtime::new`]; a test
    /// passes a virtual clock so a body's parks cost no real time.
    fn with_time(
        controller: Arc<AppController>,
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
    ) -> Self {
        Self {
            controller,
            ingest: None,
            supervise: false,
            loops: None,
            clock,
            sleeper,
        }
    }

    /// A runtime that probes nothing.
    ///
    /// `AppController::default` is built over `CapabilityReport::transport_only` with
    /// `SessionKind::None` (`app.rs:315-319`), so the report says outright that no session
    /// was discovered rather than being filled in from whatever machine the suite happens
    /// to run on.
    #[must_use]
    pub fn headless() -> Self {
        Self::new(Arc::new(AppController::default()))
    }

    /// A runtime over the real capability report.
    ///
    /// Only [`crate::shell::Shell::from_system`] calls this: it is the one path where a
    /// window exists and the probe's answer is meaningful.
    #[must_use]
    pub fn probed() -> Self {
        Self::new(Arc::new(AppController::new(
            aoko_platform::probe_capabilities(),
        )))
    }

    /// The controller this runtime owns.
    ///
    /// Cloning the `Arc` gives a transport owner or a loop the same state the switches
    /// write, without going through the view.
    #[must_use]
    pub fn controller(&self) -> &Arc<AppController> {
        &self.controller
    }

    /// Starts the bridge reader on its own thread, once.
    ///
    /// Idempotent so a startup path that runs twice (a reload, a second window) cannot
    /// attach two readers to one store and interleave their `connected` writes. The dial
    /// itself happens on the worker (`crate::ingest::Ingest::spawn`), so this call returns
    /// without waiting for a bridge that is not there.
    pub fn connect_bridge(&mut self) {
        if self.ingest.is_some() {
            return;
        }
        self.ingest = Some(Ingest::spawn(Arc::clone(&self.controller)));
    }

    /// What the bridge link currently reports.
    ///
    /// Read out of the controller's store, so it is the same truth the rest of the app
    /// sees; nothing in this crate keeps a second "connected" flag. `fresh` is
    /// `aoko_core::app::UiSnapshot::is_state_fresh`, i.e. the age is inside
    /// `aoko_core::chest_stealer::FRESH_STATE_MS`.
    #[must_use]
    pub fn ingest_status(&self) -> IngestStatus {
        let now = Instant::now();
        let store = self.controller.store();
        IngestStatus {
            connected: store.is_connected(),
            state_age: store.age(now),
            fresh: self.controller.ui_snapshot(now).is_state_fresh(),
        }
    }

    /// Turns module switches into loop lifecycle and starts what is already enabled.
    ///
    /// `Shell::from_system` is the only caller, so the shipped path supervises and every
    /// test-built or capture-built runtime does not: a headless `Runtime` starts zero
    /// threads however its switches move.
    pub fn start_supervision(&mut self) {
        self.supervise = true;
        self.reconcile_loops();
    }

    /// The names the supervisor has registered, empty when nothing is running.
    ///
    /// `ClickerLoops` starts its five bodies as one bundle, so enabling any loop-driving
    /// module registers all five names, and the last one to be disabled stops all five.
    #[must_use]
    pub fn loop_names(&self) -> Vec<&str> {
        self.loops
            .as_ref()
            .map(ClickerLoops::names)
            .unwrap_or_default()
    }

    /// How many loop threads the supervisor still owns.
    #[must_use]
    pub fn loops_owned(&self) -> usize {
        self.loops.as_ref().map_or(0, ClickerLoops::owned)
    }

    /// Starts or stops the loop bundle so it runs exactly while a loop-driving module is on.
    fn reconcile_loops(&mut self) {
        if !self.supervise {
            return;
        }

        match (self.wants_loops(), self.loops.is_some()) {
            (true, false) => self.start_loops(),
            (false, true) => {
                if let Some(mut loops) = self.loops.take() {
                    loops.stop();
                    tracing::info!("automation loops stopped: no loop-driving module is on");
                }
            }
            _ => {}
        }
    }

    /// Whether any catalog module that drives a loop is enabled.
    ///
    /// The walk is over `module_catalog::ALL`, so the set of drivers is the catalog's, not
    /// a list kept here. [`loop_for`] only decides which of those modules own a loop.
    fn wants_loops(&self) -> bool {
        module_catalog::ALL.iter().any(|entry| {
            loop_for(entry.enabled_flag).is_some()
                && self.controller.is_module_enabled(entry.enabled_flag)
        })
    }

    /// Starts the five bodies on their own threads.
    fn start_loops(&mut self) {
        let sense: Arc<dyn LoopSense> =
            Arc::new(ControllerSense::new(Arc::clone(&self.controller)));
        // Not the platform backend: this slice supervises, it does not inject. See
        // `NoInjection` for why the loops may still run against it safely.
        let input: Arc<dyn InputBackend> = Arc::new(NoInjection);
        let sink: Arc<dyn ClickSink> = Arc::new(NoClickSink);

        let clock = Arc::clone(&self.clock);
        let sleeper = Arc::clone(&self.sleeper);
        let make_runtime = move |seed: u64| {
            LoopRuntime::new(
                Arc::clone(&clock),
                Arc::clone(&sleeper),
                Box::new(SplitMix64::new(LOOP_SEED.wrapping_add(seed))),
            )
        };

        match ClickerLoops::start(
            sense,
            input,
            sink,
            ClickerLoopConfigs::defaults(),
            make_runtime,
        ) {
            Ok(loops) => {
                tracing::info!(names = ?loops.names(), "automation loops started");
                self.loops = Some(loops);
            }
            Err(error) => {
                tracing::warn!("cannot start the automation loops: {error}");
            }
        }
    }

    /// Whether one module's switch is on, by catalog id (case-insensitive).
    ///
    /// A module id the catalog does not have, or one with no boolean behind it, reads
    /// as off.
    #[must_use]
    pub fn module_enabled(&self, module_id: &str) -> bool {
        flag_for(module_id).is_some_and(|flag| self.controller.is_module_enabled(flag))
    }

    /// Sets one module's switch, by catalog id (case-insensitive).
    ///
    /// An unknown id is ignored, and so is `panic`: the catalog maps it to
    /// [`EnabledFlag::Never`] because it is a keybind-only action with no stored
    /// boolean, which is the same reason `count_enabled_modules` skips it.
    ///
    /// A module that drives a loop starts or stops the supervisor, which is the only
    /// place work begins in this crate.
    pub fn set_module_enabled(&mut self, module_id: &str, enabled: bool) {
        if let Some(flag) = flag_for(module_id) {
            self.controller.set_module_enabled(flag, enabled);
            self.reconcile_loops();
        }
    }

    /// Flips one module's switch, returning whether the id named a module with a
    /// boolean behind it.
    ///
    /// The return value is what makes `cx.notify()` honest: a click on a row that has no
    /// state to change does not repaint the window.
    pub fn flip_module(&mut self, module_id: &str) -> bool {
        let Some(flag) = flag_for(module_id) else {
            return false;
        };
        let enabled = !self.controller.is_module_enabled(flag);
        self.controller.set_module_enabled(flag, enabled);
        self.reconcile_loops();
        true
    }

    /// `Clicker.IsArmed`.
    ///
    /// The catalog models the AutoClicker's armed state as
    /// [`EnabledFlag::LeftClick`]: its entry's overlay probe is `cfg.armed`
    /// (`module_catalog.rs:228-233`), which is the string the injected bridge reports
    /// for `IsArmed`. There is no separate armed bit on the controller, so this flag is
    /// the only honest source and a second copy would be exactly the split this slice
    /// removes.
    #[must_use]
    pub fn armed(&self) -> bool {
        self.controller.is_module_enabled(EnabledFlag::LeftClick)
    }

    /// How many catalog modules are on, which is what the title chip counts.
    ///
    /// Counted from the catalog rather than from `AppController::count_enabled_modules`:
    /// that method is `ModuleCatalog.CountEnabledForDiscord` and answers a different
    /// question (Discord-RPC surface, dev-mode visibility), and this number is shown to
    /// the user as "N on".
    #[must_use]
    pub fn enabled_count(&self) -> usize {
        module_catalog::ALL
            .iter()
            .filter(|entry| entry.enabled_flag != EnabledFlag::Never)
            .filter(|entry| self.controller.is_module_enabled(entry.enabled_flag))
            .count()
    }
}

/// The controller flag behind a module id.
///
/// `None` for an id the catalog does not have, and for the entries whose flag is
/// [`EnabledFlag::Never`] — `panic` is the only one — because those have no stored
/// boolean to read or write.
fn flag_for(module_id: &str) -> Option<EnabledFlag> {
    let canonical = toggles::canonical_id(module_id)?;
    module_catalog::ALL
        .iter()
        .find(|entry| entry.id == canonical)
        .map(|entry| entry.enabled_flag)
        .filter(|flag| *flag != EnabledFlag::Never)
}

/// The loop a catalog flag drives, `None` for a module that is only a setting.
///
/// The catalog has no loop column: `Entry` carries the id, the flag, the surfaces and the
/// settings, and nothing that names a thread. So the table lives here, keyed by
/// `EnabledFlag` — the catalog's own identity — rather than by a spelling of the module id,
/// which means a catalog rename cannot desynchronise it. `every_loop_driver_is_a_catalog_entry`
/// pins each arm to a real entry and pins the names to the strings `ClickerLoops` registers.
///
/// The five arms are the five bodies `ClickerLoops::start` spawns. Everything else
/// (`jitter`, `clickinchests`, `reach`, ...) is read by one of those bodies as a setting
/// or a guard, so enabling it starts no thread of its own.
fn loop_for(flag: EnabledFlag) -> Option<&'static str> {
    match flag {
        EnabledFlag::LeftClick => Some("click-left"),
        EnabledFlag::RightClick => Some("click-right"),
        EnabledFlag::AimAssist => Some("aim-assist"),
        EnabledFlag::PixelPartyAssist => Some("pixel-party"),
        EnabledFlag::Triggerbot => Some("triggerbot"),
        _ => None,
    }
}

/// The read side of a loop body, over the controller's store.
///
/// `aoko_core::app::StoreSense` is the production implementation, but it is built over an
/// `Arc<StateStore>` and [`AppController::store`] hands out `&StateStore`, so a
/// `StoreSense` cannot be built here without a second store or a change in `aoko-core`.
/// This is that read side over the controller, with the same defaults for the facts no
/// surface sets yet: window activity and cursor visibility are `false`, and module and
/// state-field support are `false` until the bridge's `capabilities` frame is stored (the
/// ingest decodes it and drops it today) — exactly what `StoreSense::new` starts with.
struct ControllerSense {
    controller: Arc<AppController>,
}

impl ControllerSense {
    fn new(controller: Arc<AppController>) -> Self {
        Self { controller }
    }
}

impl LoopSense for ControllerSense {
    fn snapshot(&self) -> Option<Arc<StateSnapshot>> {
        self.controller.store().snapshot()
    }

    fn is_connected(&self) -> bool {
        self.controller.store().is_connected()
    }

    fn is_injected(&self) -> bool {
        self.controller.store().is_injected()
    }

    fn minecraft_active(&self) -> bool {
        // `WindowDetection` is a later slice. False is the fail-closed answer: a loop must
        // not believe the game is focused because nothing can check.
        false
    }

    fn cursor_visible(&self) -> bool {
        false
    }

    fn supports_module(&self, _module: &str) -> bool {
        false
    }

    fn supports_state_field(&self, _field: &str) -> bool {
        false
    }

    fn framebuffer_size(&self) -> Option<(i32, i32)> {
        None
    }
}

/// The input backend this slice hands the loops.
///
/// Injection is the next slice: nothing here may reach the OS, so every synthetic action is
/// refused with a classified `Unsupported`, which the loop bodies log at `debug` and carry
/// on from (`click.rs:214-216`), and the refusal is off by default
/// (`main.rs`'s filter is `info` for our crates). Observation answers the fail-closed
/// defaults, so a hold guard cannot read "nothing is held" from a backend that cannot see a
/// button. Swapping this for `aoko_platform`'s backend is the whole of the injection slice.
struct NoInjection;

impl InputBackend for NoInjection {
    fn mouse_state(&self) -> MouseState {
        MouseState::default()
    }

    fn key_state(&self, _key: Key) -> bool {
        false
    }

    fn click(&self, _button: MouseButton) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported(NO_INJECTION))
    }

    fn move_relative(&self, _dx: i32, _dy: i32) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported(NO_INJECTION))
    }

    fn key_press(&self, _key: Key) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported(NO_INJECTION))
    }

    fn poll_events(&self, _out: &mut Vec<InputEvent>) -> Result<(), PlatformError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The suite must not depend on the machine: a headless runtime reports that it
    /// probed nothing, and its module state still works.
    #[test]
    fn a_headless_runtime_probes_nothing_and_still_holds_state() {
        let mut runtime = Runtime::headless();

        // `AppController::default` is `transport_only(SessionKind::None)`. A probed
        // report on this machine would name X11 or Wayland instead.
        assert_eq!(
            runtime.controller().capabilities().session,
            aoko_platform::SessionKind::None
        );

        assert!(!runtime.module_enabled("autototem"));
        runtime.set_module_enabled("autototem", true);
        assert!(runtime.module_enabled("autototem"));
        assert!(runtime
            .controller()
            .is_module_enabled(EnabledFlag::AutoTotem));
        assert_eq!(runtime.enabled_count(), 1);

        runtime.set_module_enabled("AutoTotem", false);
        assert!(!runtime.module_enabled("autototem"));
        assert_eq!(runtime.enabled_count(), 0);
    }

    /// The AutoClicker row is the armed bit, not an ordinary module, and flipping it
    /// reaches the controller's `LeftClick` flag.
    #[test]
    fn the_autoclicker_switch_is_the_armed_flag() {
        let mut runtime = Runtime::headless();

        assert!(!runtime.armed());
        assert!(runtime.flip_module("autoclicker"));
        assert!(runtime.armed());
        assert!(runtime.module_enabled("autoclicker"));
        assert!(runtime
            .controller()
            .is_module_enabled(EnabledFlag::LeftClick));

        assert!(runtime.flip_module("AUTOCLICKER"));
        assert!(!runtime.armed());
        assert!(!runtime
            .controller()
            .is_module_enabled(EnabledFlag::LeftClick));
    }

    /// `panic` has no boolean (`EnabledFlag::Never`), and an unknown id names nothing:
    /// both are ignored, and neither of them reports a change that would repaint.
    #[test]
    fn ids_without_a_boolean_are_ignored() {
        let mut runtime = Runtime::headless();

        runtime.set_module_enabled("panic", true);
        assert!(!runtime.module_enabled("panic"));
        assert!(!runtime.flip_module("panic"));
        assert!(!runtime.flip_module("no-such-module"));
        assert_eq!(runtime.enabled_count(), 0);
    }

    /// Ids are matched case-insensitively, as `canonical_id` is, and a key that is not
    /// a catalog module is refused.
    ///
    /// The second half is a narrowing worth stating: `ModuleToggles::set_enabled`
    /// accepted non-catalog keys so the panic reset could clear the AutoClicker
    /// sub-settings, but both of the shell's write paths canonicalised their id first, so
    /// no switch ever wrote one. The controller stores catalog flags only, and this view
    /// refuses what it cannot represent rather than pretending the write landed.
    #[test]
    fn ids_are_case_insensitive_and_only_catalog_modules_are_written() {
        let mut runtime = Runtime::headless();

        runtime.set_module_enabled("ClickInChests", true);
        assert!(runtime.module_enabled("clickinchests"));

        runtime.set_module_enabled("pixelpartyautolook", true);
        assert!(!runtime.module_enabled("pixelpartyautolook"));
        assert_eq!(runtime.enabled_count(), 1);
    }

    /// A runtime a test builds must not touch the network, and must say so rather than
    /// waiting on a dial.
    ///
    /// The structural half is the decisive one: `ingest` is `None`, so no reader thread
    /// exists that could connect later, and `connect_bridge` is the only thing that
    /// starts one — the shipped path, never this constructor. The elapsed bound is the
    /// only way to observe "does not block" directly: on a machine with a bridge, a
    /// synchronous dial would succeed, and on one without it, a refused connect could
    /// still stall a loaded loopback stack. Two seconds is far above either.
    #[test]
    fn a_headless_runtime_opens_no_socket() {
        let started = Instant::now();
        let mut runtime = Runtime::headless();
        let status = runtime.ingest_status();

        assert!(runtime.ingest.is_none(), "headless builds no bridge reader");
        assert!(!status.connected, "nothing connected, nothing claimed");
        assert_eq!(status.state_age, None, "no frame has arrived");
        assert!(!status.fresh, "a state that does not exist is not fresh");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "constructing a runtime must not dial"
        );
    }

    /// The status is the store's own truth, and freshness is measured from the frame's
    /// arrival instant rather than from when the question is asked.
    #[test]
    fn status_reports_what_the_store_holds() {
        use aoko_protocol::state::GameState;

        let mut runtime = Runtime::headless();
        let store = runtime.controller().store();

        store.set_connected(true);
        // Stamped in the future, so the age saturates to zero: the fresh branch is then a
        // fact rather than a race against the 300 ms window.
        store.publish(
            GameState::default(),
            Instant::now() + Duration::from_secs(1),
        );

        let status = runtime.ingest_status();
        assert!(status.connected);
        assert_eq!(status.state_age, Some(Duration::ZERO));
        assert!(status.fresh);

        // A second in the past is outside the window whatever the scheduler does.
        store.publish(
            GameState::default(),
            Instant::now() - Duration::from_secs(1),
        );

        let status = runtime.ingest_status();
        assert!(
            status
                .state_age
                .is_some_and(|age| age >= Duration::from_secs(1)),
            "the age is measured from the publish instant"
        );
        assert!(!status.fresh, "an old frame is stale by definition");
    }

    /// A clock and sleeper that move only when a body parks.
    ///
    /// The bodies never read the wall clock (`LoopRuntime` hands them this), so a park costs
    /// no real time and the test asserts what a switch does, not how long it takes.
    struct StepClock {
        now: std::sync::Mutex<Instant>,
    }

    impl StepClock {
        fn new() -> Self {
            Self {
                now: std::sync::Mutex::new(Instant::now()),
            }
        }
    }

    impl Clock for StepClock {
        fn now(&self) -> Instant {
            *self.now.lock().expect("clock")
        }
    }

    impl Sleeper for StepClock {
        fn sleep_until(&self, deadline: Instant) {
            let mut now = self.now.lock().expect("clock");
            if deadline.checked_duration_since(*now).is_some() {
                *now = deadline;
            }
        }
    }

    /// The switch is the supervisor's only trigger: enabling a loop-driving module starts
    /// the bundle, and turning the last one off stops it.
    ///
    /// `autoclicker` is the catalog entry whose flag is `EnabledFlag::LeftClick`, and
    /// `LeftClick` is the flag [`loop_for`] names `click-left`, so this is the click loop
    /// asked about by name rather than by index.
    #[test]
    fn enabling_a_loop_module_starts_supervision_and_disabling_stops_it() {
        let controller = Arc::new(AppController::default());
        let source = Arc::new(StepClock::new());
        let clock: Arc<dyn Clock> = Arc::clone(&source);
        let sleeper: Arc<dyn Sleeper> = Arc::clone(&source);

        let mut runtime = Runtime::with_time(Arc::clone(&controller), clock, sleeper);
        runtime.start_supervision();

        assert!(runtime.loop_names().is_empty(), "nothing is on yet");
        assert_eq!(runtime.loops_owned(), 0);

        runtime.set_module_enabled("autoclicker", true);

        let names = runtime.loop_names();
        assert!(
            names.contains(&"click-left"),
            "the click loop must be registered by name: {names:?}"
        );
        assert_eq!(
            runtime.loops_owned(),
            5,
            "the bundle owns one thread per body: {names:?}"
        );

        runtime.set_module_enabled("autoclicker", false);

        assert!(
            runtime.loop_names().is_empty(),
            "the last loop-driving module off stops the bundle"
        );
        assert_eq!(runtime.loops_owned(), 0);
    }

    /// A module that is only a setting starts nothing, even with supervision on.
    #[test]
    fn a_setting_only_module_starts_no_loops() {
        let controller = Arc::new(AppController::default());
        let source = Arc::new(StepClock::new());
        let clock: Arc<dyn Clock> = Arc::clone(&source);
        let sleeper: Arc<dyn Sleeper> = Arc::clone(&source);

        let mut runtime = Runtime::with_time(controller, clock, sleeper);
        runtime.start_supervision();
        runtime.set_module_enabled("jitter", true);

        assert!(runtime.module_enabled("jitter"));
        assert!(runtime.loop_names().is_empty(), "jitter is a setting");
        assert_eq!(runtime.loops_owned(), 0);
    }

    /// A headless runtime starts nothing, whatever its switches do.
    ///
    /// This is what keeps the suite and the capture harness free of automation threads: the
    /// supervisor is off until `start_supervision`, and only the shipped path calls that.
    #[test]
    fn a_headless_runtime_starts_no_loops() {
        let mut runtime = Runtime::headless();

        assert!(runtime.loop_names().is_empty());
        assert_eq!(runtime.loops_owned(), 0);

        runtime.set_module_enabled("autoclicker", true);

        assert!(
            runtime.module_enabled("autoclicker"),
            "the switch still works"
        );
        assert!(
            runtime.loop_names().is_empty(),
            "no supervision, no threads"
        );
        assert_eq!(runtime.loops_owned(), 0);
    }

    /// The table is anchored to the catalog, not to a list of spellings.
    ///
    /// Every id that answers "yes" must be a real `module_catalog::ALL` entry, the set of
    /// them must be the five bodies `ClickerLoops` starts, and the names must be the strings
    /// those bodies register — a rename on either side fails here rather than leaving a
    /// switch that starts a loop it cannot name.
    #[test]
    fn every_loop_driver_is_a_catalog_entry() {
        let mut drivers: Vec<&'static str> = module_catalog::ALL
            .iter()
            .filter(|entry| loop_for(entry.enabled_flag).is_some())
            .map(|entry| entry.id)
            .collect();
        drivers.sort_unstable();

        assert_eq!(
            drivers,
            vec![
                "aimassist",
                "autoclicker",
                "pixelpartyassist",
                "rightclick",
                "triggerbot"
            ]
        );

        let mut names: Vec<&str> = module_catalog::ALL
            .iter()
            .filter_map(|entry| loop_for(entry.enabled_flag))
            .collect();
        names.sort_unstable();

        assert_eq!(
            names,
            vec![
                "aim-assist",
                "click-left",
                "click-right",
                "pixel-party",
                "triggerbot"
            ]
        );

        assert!(loop_for(EnabledFlag::Never).is_none(), "panic has no loop");
        assert!(
            loop_for(EnabledFlag::Jitter).is_none(),
            "a click setting, not a loop"
        );
        assert!(
            loop_for(EnabledFlag::HudEditorActive).is_none(),
            "the HUD editor is a modal, not a thread"
        );
    }
}
