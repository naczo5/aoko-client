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
//! stops them. It is off until the shipped path turns it on.
//!
//! What those bodies may touch is [`crate::runtime::select_loop_surfaces`], one pure function
//! over the capability report. A session that can synthesise input *and* sample the pointer
//! gets `aoko_platform`'s own `InputBackend`; a session that can inspect windows also gets
//! `aoko_platform`'s `WindowInspector`, which is what answers "is the game focused" and "is
//! the cursor up". Each refused capability is named once, and a session that can do neither
//! builds no platform service at all, which is what keeps the suite and the capture harness
//! off the OS.
//!
//! Injection is gated a second time, per action: `crate::runtime::game_is_foreground` must
//! confirm that the foreground window is a Java launcher whose title names the game before a
//! click, a key press or a movement reaches the OS. Supervision therefore starts without a
//! user action — parity with the managed host — and a window that took focus still cannot
//! receive a synthetic click.
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
use aoko_platform::{
    CapabilityReport, InputBackend, InputEvent, Key, MouseButton, MouseState, PlatformError,
    WindowInspector,
};
use aoko_protocol::module_catalog::{self, EnabledFlag};

use crate::ingest::{Ingest, IngestStatus};

/// Seed the five loop bodies start from.
///
/// One RNG per body, as `ClickerLoops::start` expects: the bodies draw click intervals from
/// it, so two bodies sharing a seed would pace identically. Deterministic on purpose — a
/// loop test that failed once can be replayed.
const LOOP_SEED: u64 = 0x00A0_C0DE;

/// Why a synthetic action is refused when this session has no platform input backend.
///
/// The classification, not the reason: `aoko_platform::PlatformError::unsupported` is what the
/// loop bodies treat as "this action cannot happen here", and
/// [`crate::runtime::select_loop_surfaces`] logs the capability that caused it.
const NO_INJECTION: &str = "this session has no platform injection backend";

/// Injection refused: the session cannot synthesise clicks, keys or movement.
const REFUSED_SYNTHESIS: &str = "automation loops run without platform injection: \
     CapabilityReport::input_synthesis is false, so this session cannot synthesise a click, \
     a key press or relative movement";

/// Injection refused: the session cannot sample the current pointer and key state.
///
/// Separate from [`REFUSED_SYNTHESIS`] on purpose: a session can move the pointer and not see
/// whether a button is down (`input_observation` without `pointer_state_sampling` on Wayland),
/// and a hold guard reading "not held" from a backend that cannot see the button would fire
/// clicks nobody asked for.
const REFUSED_SAMPLING: &str = "automation loops run without platform injection: \
     CapabilityReport::pointer_state_sampling is false, so a hold guard cannot tell whether \
     a trigger is down";

/// Window sense refused: the session cannot see windows at all.
const REFUSED_WINDOWS: &str = "automation loops read no window state: \
     CapabilityReport::window_inspection is false, so no foreground window can be asked \
     which process is focused";

/// Injection refused because the guard that makes it safe has nothing to read.
///
/// The gate is [`game_is_foreground`], and it reads the foreground window. Without that
/// surface a synthetic click could land in whatever window happens to be in front, so the
/// session keeps the refusing backend rather than injecting blind.
const REFUSED_INJECTION_WITHOUT_WINDOWS: &str = "automation loops run without platform \
     injection: CapabilityReport::window_inspection is false, so the game cannot be confirmed \
     foreground before a synthetic action";

/// Why a synthetic action is refused by the foreground gate.
const GAME_NOT_FOREGROUND: &str = "the game is not the confirmed foreground window";

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
    ///
    /// The backend and the window surface are not chosen here:
    /// [`crate::runtime::select_loop_surfaces`] decides from the controller's capability
    /// report, and `platform_surfaces` builds exactly what that decision allows.
    fn start_loops(&mut self) {
        let surfaces = select_loop_surfaces(self.controller.capabilities());
        let (input, windows) = platform_surfaces(&surfaces);
        let sense: Arc<dyn LoopSense> =
            Arc::new(ControllerSense::new(Arc::clone(&self.controller), windows));
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

/// One platform surface, either reachable or refused with the reasons.
///
/// The variant is the choice: `Platform` means the loops may reach the platform service, and
/// `Refused` carries one line per missing capability rather than a bare `false`, because the
/// reason is what the log, the user-facing report and the tests all need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Surface {
    /// The session supports this surface; the service is built when the loops start.
    Platform,
    /// The session does not support it, for these capabilities.
    Refused(Vec<&'static str>),
}

impl Surface {
    /// Whether the loops may reach the platform for this surface.
    #[must_use]
    pub fn is_platform(&self) -> bool {
        matches!(self, Self::Platform)
    }

    /// The refusals, empty when the surface is reachable.
    #[must_use]
    pub fn refusals(&self) -> &[&'static str] {
        match self {
            Self::Platform => &[],
            Self::Refused(reasons) => reasons,
        }
    }
}

/// What the platform allows the automation loops to touch.
///
/// Two surfaces, decided separately: injection is synthetic input, window sense is reading focus
/// and the cursor. Window sense stands alone — a session can inspect windows and inject nothing
/// (X11 without `uinput`) — but injection does not: it is gated on the game being confirmed
/// foreground, so a session without window inspection keeps the refusing backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopSurfaces {
    /// Whether synthetic input may go through the platform's `InputBackend`.
    pub injection: Surface,
    /// Whether focus and cursor visibility may come from the platform's `WindowInspector`.
    pub windows: Surface,
}

/// Chooses the platform surfaces the automation loops get, from one capability report.
///
/// This is the whole decision, over nothing but a [`CapabilityReport`]: no machine is read, no
/// service is built and no clock or window is touched, so a test answers it directly and
/// [`crate::runtime::Runtime`] has nothing left to choose. `start_loops` is the only caller,
/// through `platform_surfaces`; no caller anywhere picks a backend by hand.
///
/// Injection needs three capabilities. `input_synthesis` is what moves and clicks,
/// `pointer_state_sampling` is what a hold guard reads before it treats a trigger as down, and
/// `window_inspection` is what [`game_is_foreground`] reads before every acting call. A backend
/// that can click and cannot see the button is worse than one that refuses, because the loop
/// cannot tell "held" from "not held"; and a backend that can click without confirming the
/// foreground is worse than both, because the click can land in an editor.
#[must_use]
pub fn select_loop_surfaces(capabilities: &CapabilityReport) -> LoopSurfaces {
    let injection_refusals: Vec<&'static str> = [
        (!capabilities.input_synthesis).then_some(REFUSED_SYNTHESIS),
        (!capabilities.pointer_state_sampling).then_some(REFUSED_SAMPLING),
        (!capabilities.window_inspection).then_some(REFUSED_INJECTION_WITHOUT_WINDOWS),
    ]
    .into_iter()
    .flatten()
    .collect();

    let injection = if injection_refusals.is_empty() {
        Surface::Platform
    } else {
        Surface::Refused(injection_refusals)
    };

    let windows = if capabilities.window_inspection {
        Surface::Platform
    } else {
        Surface::Refused(vec![REFUSED_WINDOWS])
    };

    LoopSurfaces { injection, windows }
}

/// The managed host's game-title rule, `WindowDetection.WindowTarget.IsGameTitle`.
///
/// `Aoko/Core/WindowDetection.cs:57` holds the titles (`Minecraft`, `Lunar Client`, `Badlion`) and
/// `:181` compares them as ordinal, case-insensitive substrings. The three literals are folded to
/// lowercase here and nothing else is added: a bare `lunar` is deliberately not enough, because
/// the managed literal is `Lunar Client`.
///
/// **This is the one copy in the crate, and it exists to be deleted.** The platform owner is
/// exposing the same rule as `aoko_platform::is_game_window`, which is title-only by design;
/// [`game_is_foreground`] adds the launcher test on top, so when that function commits this body
/// becomes `aoko_platform::is_game_window(&window)` and the composition stays. Until then it must
/// not drift: `a_game_title_rule_matches_the_managed_titles` pins the three literals.
#[must_use]
pub fn is_game_title(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("minecraft") || lower.contains("lunar client") || lower.contains("badlion")
}

/// Whether the game is the foreground window, confirmed strictly enough to gate injection.
///
/// Both halves of the managed test are required, not one score against one threshold:
///
/// * the owning process must be a Java launcher — `java`, `javaw`, `java.exe`, `javaw.exe`, which
///   is `WindowInfo::is_jvm` (`WindowDetection.WindowTarget.IsJvm`);
/// * the title must name the game — [`is_game_title`].
///
/// `aoko_platform::describe_target(..).is_likely_minecraft()` is deliberately not used: its
/// markers include project words (`fabric`, `forge`, `quilt`, `feather`), so a focused editor on
/// a mod workspace scores as the game, and a click gated on a confidence score would type into
/// code. Requiring the title half as well closes that case: `fabric-mod-template - IntelliJ IDEA`
/// is a Java process, so it passes the launcher test and fails the title test.
///
/// One residual case is worth stating rather than hiding: this is still a title test, so a Java
/// IDE whose window title contains a game word — a project called `minecraft-plugin`, say —
/// satisfies both halves. Closing it needs the managed host's stronger first test (the foreground
/// equals a known game window, i.e. `select_best_window` over a full enumeration) or a rule that
/// excludes IDE windows, and neither exists in `aoko_platform` today. Inventing an IDE blocklist
/// here would be the drift [`is_game_title`] exists to avoid, so it is reported instead.
///
/// A missing window surface, a foreground the backend cannot read, and a window with no process
/// name all answer `false`: this is the guard, so it fails closed.
#[must_use]
pub fn game_is_foreground(windows: &dyn WindowInspector) -> bool {
    let Ok(Some(foreground)) = windows.foreground() else {
        return false;
    };
    foreground.is_jvm() && is_game_title(&foreground.title)
}

/// The platform input backend, gated on the game being confirmed foreground.
///
/// Every acting call asks [`game_is_foreground`] first and is refused with a classified
/// `unsupported` when it answers no, so a window that took focus between two ticks cannot receive
/// a synthetic click. Observation calls pass through untouched — they cannot act, and a hold
/// guard must still see whether a button is down — and so do the two release calls
/// (`button_up`, `key_up`), because a focus change must never leave a button or a key held.
///
/// The check costs one `WindowInspector::foreground` per acting call — one X11 round trip plus a
/// `/proc` read on Linux — which is once per click or key press, never once per tick.
struct ForegroundGated {
    platform: Arc<dyn InputBackend>,
    windows: Arc<dyn WindowInspector>,
}

impl ForegroundGated {
    /// The confirmation, evaluated fresh for every acting call rather than cached.
    fn confirmed(&self) -> bool {
        game_is_foreground(self.windows.as_ref())
    }

    /// `Ok` when the game is confirmed foreground, otherwise the classified refusal.
    fn require_game(&self) -> Result<(), PlatformError> {
        if self.confirmed() {
            Ok(())
        } else {
            Err(PlatformError::unsupported(GAME_NOT_FOREGROUND))
        }
    }
}

impl InputBackend for ForegroundGated {
    fn probe_mouse_state(&self) -> Option<MouseState> {
        self.platform.probe_mouse_state()
    }

    fn probe_key_state(&self, key: Key) -> Option<bool> {
        self.platform.probe_key_state(key)
    }

    fn mouse_state(&self) -> MouseState {
        self.platform.mouse_state()
    }

    fn key_state(&self, key: Key) -> bool {
        self.platform.key_state(key)
    }

    fn mouse_position(&self) -> Option<(i32, i32)> {
        self.platform.mouse_position()
    }

    fn poll_events(&self, out: &mut Vec<InputEvent>) -> Result<(), PlatformError> {
        self.platform.poll_events(out)
    }

    fn click(&self, button: MouseButton) -> Result<(), PlatformError> {
        self.require_game()?;
        self.platform.click(button)
    }

    fn move_relative(&self, dx: i32, dy: i32) -> Result<(), PlatformError> {
        self.require_game()?;
        self.platform.move_relative(dx, dy)
    }

    fn key_press(&self, key: Key) -> Result<(), PlatformError> {
        self.require_game()?;
        self.platform.key_press(key)
    }

    fn button_down(&self, button: MouseButton) -> Result<(), PlatformError> {
        self.require_game()?;
        self.platform.button_down(button)
    }

    fn key_down(&self, key: Key) -> Result<(), PlatformError> {
        self.require_game()?;
        self.platform.key_down(key)
    }

    /// Not gated: a release must always be attempted, or a focus change mid-hold would strand a
    /// button down in whatever application has it.
    fn button_up(&self, button: MouseButton) -> Result<(), PlatformError> {
        self.platform.button_up(button)
    }

    /// Not gated, for the same reason as [`ForegroundGated::button_up`]'s note: a held key must
    /// never be stranded.
    fn key_up(&self, key: Key) -> Result<(), PlatformError> {
        self.platform.key_up(key)
    }
}

/// Builds the two platform surfaces that [`select_loop_surfaces`] allows.
///
/// `aoko_platform::default_services` is called only when at least one surface is reachable, so
/// a headless runtime and every test reach no platform service at all: the controller's default
/// report refuses both. Each refusal is logged once — no injection at `warn`, because the loops
/// run and cannot act, and no window sense at `info`, because the loops still act on the
/// fail-closed sense.
///
/// The reachable surfaces are the platform's own implementations, not wrappers: nothing in this
/// crate re-implements an input call or a window query. The one addition is the gate —
/// `ForegroundGated` refuses an acting call the platform backend would have performed, which is
/// a safety condition rather than a second implementation of the call.
fn platform_surfaces(
    surfaces: &LoopSurfaces,
) -> (Arc<dyn InputBackend>, Option<Arc<dyn WindowInspector>>) {
    let mut input: Arc<dyn InputBackend> = Arc::new(NoInjection);
    let mut windows: Option<Arc<dyn WindowInspector>> = None;

    for reason in surfaces.injection.refusals() {
        tracing::warn!("{reason}");
    }
    for reason in surfaces.windows.refusals() {
        tracing::info!("{reason}");
    }

    if !surfaces.injection.is_platform() && !surfaces.windows.is_platform() {
        return (input, windows);
    }

    let services = aoko_platform::default_services();
    if surfaces.windows.is_platform() {
        windows = Some(Arc::from(services.windows));
    }
    if surfaces.injection.is_platform() {
        match windows.as_ref() {
            // The decision refuses injection without window inspection, so a reachable
            // injection surface always has the guard it needs.
            Some(window_surface) => {
                input = Arc::new(ForegroundGated {
                    platform: Arc::from(services.input),
                    windows: Arc::clone(window_surface),
                });
            }
            None => tracing::warn!(
                "platform injection selected without a window surface; keeping the refusing backend"
            ),
        }
    }
    (input, windows)
}

/// The read side of a loop body, over the controller's store and, when the capability report
/// allows it, the platform's window surface.
///
/// `aoko_core::app::StoreSense` is the production implementation, but it is built over an
/// `Arc<StateStore>` and [`AppController::store`] hands out `&StateStore`, so a
/// `StoreSense` cannot be built here without a second store or a change in `aoko-core`.
/// This is that read side over the controller. The store half keeps the defaults
/// `StoreSense::new` starts with: module and state-field support stay `false` until the
/// bridge's `capabilities` frame is stored (the ingest decodes it and drops it today).
///
/// The window half is [`crate::runtime::select_loop_surfaces`]'s decision: `windows` is `None`
/// exactly when that function refused the surface, and then both window questions answer
/// fail-closed rather than guessing.
struct ControllerSense {
    controller: Arc<AppController>,
    windows: Option<Arc<dyn WindowInspector>>,
}

impl ControllerSense {
    fn new(controller: Arc<AppController>, windows: Option<Arc<dyn WindowInspector>>) -> Self {
        Self {
            controller,
            windows,
        }
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

    /// `WindowDetection.IsMinecraftActive()`.
    ///
    /// The same strict confirmation the injection gate uses — [`game_is_foreground`], which is
    /// the foreground window with a Java launcher name *and* a game title — so the click bodies'
    /// own gate and the backend's gate cannot disagree about whether the game is in front.
    ///
    /// `false` when the session refused the window surface: a loop must not believe the game is
    /// focused because nothing can check.
    fn minecraft_active(&self) -> bool {
        self.windows.as_deref().is_some_and(game_is_foreground)
    }

    /// `WindowDetection.IsCursorVisible()`.
    ///
    /// **Linux cannot answer this.** The platform returns the cursor's visibility where it can
    /// (Windows uses `GetCursorInfo`), and `WindowInspector::cursor_visible` defaults to `None`
    /// with no override in `linux/window.rs`, so on X11 there is nothing to read. `None` keeps
    /// the fail-closed `false` this sense has always answered: `triggerbot` acts on
    /// `!cursor_visible`, so guessing "hidden" would make it click while a menu is open, and
    /// `aim`/`pixel-party` require the cursor, so guessing "visible" would make those act in the
    /// game view.
    ///
    /// What makes the fail-closed half safe is not this method: it is [`game_is_foreground`] in
    /// [`ForegroundGated`], which refuses every acting call unless the game is the confirmed
    /// foreground window. A wrong cursor answer can therefore only stop a loop, never aim one at
    /// another application.
    fn cursor_visible(&self) -> bool {
        self.windows
            .as_deref()
            .and_then(|windows| windows.cursor_visible())
            .unwrap_or(false)
    }

    fn supports_module(&self, _module: &str) -> bool {
        false
    }

    fn supports_state_field(&self, _field: &str) -> bool {
        false
    }

    /// `None`: window geometry needs `WindowInspector::client_bounds` plus a known game handle,
    /// and the state's own viewport is what the bodies fall back to until that handle exists.
    fn framebuffer_size(&self) -> Option<(i32, i32)> {
        None
    }
}

/// The input backend the loops get when the platform's is refused, and the one every test runs
/// against.
///
/// Nothing here may reach the OS, so every synthetic action is refused with a classified
/// `Unsupported`, which the loop bodies log at `debug` and carry on from (`click.rs:214-216`),
/// and the refusal is off by default (`main.rs`'s filter is `info` for our crates). Observation
/// answers the fail-closed defaults, so a hold guard cannot read "nothing is held" from a
/// backend that cannot see a button.
///
/// [`crate::runtime::select_loop_surfaces`] picks between this and `aoko_platform`'s own
/// backend; no caller chooses one by hand, and a session that refuses injection keeps this one.
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
        let runtime = Runtime::headless();
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

        let runtime = Runtime::headless();
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
        let clock: Arc<dyn Clock> = source.clone();
        let sleeper: Arc<dyn Sleeper> = source.clone();

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
        let clock: Arc<dyn Clock> = source.clone();
        let sleeper: Arc<dyn Sleeper> = source.clone();

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

    // ---- platform surfaces: the decision, and the sense behind it -------------------

    /// A window surface that answers from fixed fields and never touches the OS.
    ///
    /// Every window-sense assertion below runs against this, so the suite cannot depend on
    /// what happens to be focused on the machine running it.
    struct FakeWindows {
        foreground: Option<aoko_platform::WindowInfo>,
        cursor: Option<bool>,
    }

    impl WindowInspector for FakeWindows {
        fn enumerate(&self) -> Result<Vec<aoko_platform::WindowInfo>, PlatformError> {
            Ok(self.foreground.iter().cloned().collect())
        }

        fn foreground(&self) -> Result<Option<aoko_platform::WindowInfo>, PlatformError> {
            Ok(self.foreground.clone())
        }

        fn client_bounds(
            &self,
            _handle: aoko_platform::WindowHandle,
        ) -> Result<aoko_platform::Rect, PlatformError> {
            Ok(aoko_platform::Rect::default())
        }

        fn cursor_visible(&self) -> Option<bool> {
            self.cursor
        }
    }

    /// One top-level window, over `WindowInfo`'s public fields.
    fn window(title: &str, process: &str) -> aoko_platform::WindowInfo {
        aoko_platform::WindowInfo {
            handle: aoko_platform::WindowHandle(0x51),
            title: title.to_string(),
            class_name: Some("LWJGL".to_string()),
            pid: Some(4242),
            process_name: Some(process.to_string()),
            bounds: aoko_platform::Rect::default(),
        }
    }

    /// A report with every OS service off names one refusal per capability, and the text says
    /// which flag refused it — that string is what the log and the report carry.
    ///
    /// Three, not two: injection is gated on the game being confirmed foreground, and that
    /// confirmation reads the foreground window, so `window_inspection` refuses injection as
    /// well as window sense.
    #[test]
    fn every_missing_capability_names_its_own_refusal() {
        let surfaces = select_loop_surfaces(&CapabilityReport::transport_only(
            aoko_platform::SessionKind::None,
        ));

        assert!(!surfaces.injection.is_platform());
        assert!(!surfaces.windows.is_platform());
        assert_eq!(
            surfaces.injection.refusals().len(),
            3,
            "{:?}",
            surfaces.injection.refusals()
        );
        assert!(surfaces
            .injection
            .refusals()
            .iter()
            .any(|reason| reason.contains("input_synthesis")));
        assert!(surfaces
            .injection
            .refusals()
            .iter()
            .any(|reason| reason.contains("pointer_state_sampling")));
        assert!(surfaces
            .injection
            .refusals()
            .iter()
            .any(|reason| reason.contains("window_inspection")));
        assert_eq!(surfaces.windows.refusals().len(), 1);
        assert!(surfaces.windows.refusals()[0].contains("window_inspection"));
    }

    /// A session that can inject and inspect gets both platform surfaces.
    #[test]
    fn a_capable_session_gets_both_platform_surfaces() {
        let mut report = CapabilityReport::transport_only(aoko_platform::SessionKind::X11);
        report.input_synthesis = true;
        report.pointer_state_sampling = true;
        report.window_inspection = true;

        let surfaces = select_loop_surfaces(&report);

        assert_eq!(surfaces.injection, Surface::Platform);
        assert_eq!(surfaces.windows, Surface::Platform);
        assert!(surfaces.injection.refusals().is_empty());
        assert!(surfaces.windows.refusals().is_empty());
    }

    /// Window sense stands alone; injection does not.
    ///
    /// The first half is the Wayland shape the platform reports: readable input devices, no
    /// pointer sampling, so injection is refused for exactly that one capability. The second is
    /// a session that can synthesise and sample but cannot inspect windows: injection is refused
    /// because the foreground gate would have nothing to read, which is the dependency the strict
    /// check adds. The third is the direction that still holds freely — windows without injection.
    #[test]
    fn window_sense_stands_alone_and_injection_depends_on_it() {
        let mut report = CapabilityReport::transport_only(aoko_platform::SessionKind::Wayland);
        report.input_synthesis = true;
        report.window_inspection = true;

        let surfaces = select_loop_surfaces(&report);

        assert!(
            !surfaces.injection.is_platform(),
            "sampling is still missing"
        );
        assert_eq!(
            surfaces.injection.refusals().len(),
            1,
            "only the sampling capability refuses injection here"
        );
        assert!(surfaces.injection.refusals()[0].contains("pointer_state_sampling"));
        assert!(surfaces.windows.is_platform(), "window inspection is on");

        let mut report = CapabilityReport::transport_only(aoko_platform::SessionKind::X11);
        report.input_synthesis = true;
        report.pointer_state_sampling = true;

        let surfaces = select_loop_surfaces(&report);

        assert!(
            !surfaces.injection.is_platform(),
            "the foreground gate needs the window surface"
        );
        assert_eq!(
            surfaces.injection.refusals().len(),
            1,
            "the missing capability is the window one"
        );
        assert!(surfaces.injection.refusals()[0].contains("window_inspection"));
        assert!(!surfaces.windows.is_platform());

        let mut report = CapabilityReport::transport_only(aoko_platform::SessionKind::X11);
        report.window_inspection = true;

        let surfaces = select_loop_surfaces(&report);

        assert!(
            surfaces.windows.is_platform(),
            "window sense needs no injection capability"
        );
        assert!(!surfaces.injection.is_platform());
        assert_eq!(
            surfaces.injection.refusals().len(),
            2,
            "synthesis and sampling are both off"
        );
    }

    /// A headless runtime would take no platform surface, so it can never reach
    /// `aoko_platform::default_services` and never touches the OS.
    ///
    /// This is the suite's guarantee rather than a preference: `AppController::default` is
    /// `transport_only(SessionKind::None)`, so every test-built runtime and the capture path
    /// refuse both surfaces before a service is built.
    #[test]
    fn a_headless_runtime_selects_no_platform_surface() {
        let runtime = Runtime::headless();
        let surfaces = select_loop_surfaces(runtime.controller().capabilities());

        assert!(!surfaces.injection.is_platform());
        assert!(!surfaces.windows.is_platform());
        assert!(!surfaces.injection.refusals().is_empty());
        assert!(!surfaces.windows.refusals().is_empty());
    }

    /// The window sense asks the platform surface, and fails closed when it has none.
    ///
    /// Nothing here clicks, moves the cursor or opens a window: the answer comes from
    /// `FakeWindows`, so the assertions hold on a headless CI runner as well as a desktop.
    #[test]
    fn window_sense_reads_the_platform_surface_and_never_the_os() {
        let controller = Arc::new(AppController::default());

        // The game is focused: a Java launcher whose title names the game.
        let game: Arc<dyn WindowInspector> = Arc::new(FakeWindows {
            foreground: Some(window("Minecraft 1.8.9", "java")),
            cursor: Some(false),
        });
        let sense = ControllerSense::new(Arc::clone(&controller), Some(game));

        assert!(sense.minecraft_active(), "a focused game window is active");
        assert!(!sense.cursor_visible(), "the platform answered: no cursor");

        // A Java process with a window, and not the game: the title half of the strict check
        // fails, which is what keeps a focused IDE from reading as the game.
        let editor: Arc<dyn WindowInspector> = Arc::new(FakeWindows {
            foreground: Some(window("MainActivity.java - IntelliJ IDEA", "java")),
            cursor: None,
        });
        let sense = ControllerSense::new(Arc::clone(&controller), Some(editor));

        assert!(
            !sense.minecraft_active(),
            "a focused Java IDE is not the game"
        );
        assert!(
            !sense.cursor_visible(),
            "an unanswered cursor keeps the fail-closed false"
        );

        // A session with no window surface: both questions answer fail-closed.
        let sense = ControllerSense::new(Arc::clone(&controller), None);

        assert!(!sense.minecraft_active());
        assert!(!sense.cursor_visible());

        // And a platform that saw the pointer drawn is believed.
        let drawn: Arc<dyn WindowInspector> = Arc::new(FakeWindows {
            foreground: None,
            cursor: Some(true),
        });
        let sense = ControllerSense::new(controller, Some(drawn));

        assert!(!sense.minecraft_active(), "no foreground window, no game");
        assert!(
            sense.cursor_visible(),
            "the platform answered: cursor drawn"
        );
    }

    /// A platform backend that accepts everything, so a refusal can only come from the gate.
    struct AcceptingInput;

    impl InputBackend for AcceptingInput {
        fn probe_mouse_state(&self) -> Option<MouseState> {
            Some(MouseState::default())
        }

        fn mouse_state(&self) -> MouseState {
            MouseState::default()
        }

        fn key_state(&self, _key: Key) -> bool {
            false
        }

        fn click(&self, _button: MouseButton) -> Result<(), PlatformError> {
            Ok(())
        }

        fn move_relative(&self, _dx: i32, _dy: i32) -> Result<(), PlatformError> {
            Ok(())
        }

        fn key_press(&self, _key: Key) -> Result<(), PlatformError> {
            Ok(())
        }

        fn poll_events(&self, _out: &mut Vec<InputEvent>) -> Result<(), PlatformError> {
            Ok(())
        }

        fn button_up(&self, _button: MouseButton) -> Result<(), PlatformError> {
            Ok(())
        }

        fn key_up(&self, _key: Key) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    /// The gate over `AcceptingInput`, with the foreground the caller names.
    fn gated(foreground: Option<aoko_platform::WindowInfo>) -> ForegroundGated {
        let windows: Arc<dyn WindowInspector> = Arc::new(FakeWindows {
            foreground,
            cursor: None,
        });
        ForegroundGated {
            platform: Arc::new(AcceptingInput),
            windows,
        }
    }

    /// The title rule is the platform's private one, copied once, so it is pinned here: a drift
    /// before `aoko_platform` exposes it would quietly widen or narrow the injection guard.
    #[test]
    fn a_game_title_rule_matches_the_managed_titles() {
        for title in [
            "Minecraft",
            "Minecraft 1.8.9",
            "Lunar Client v3.2.1",
            "Badlion Client",
        ] {
            assert!(is_game_title(title), "{title} names the game");
        }

        for title in [
            "",
            "Visual Studio Code",
            "MainActivity.java - IntelliJ IDEA",
            "fabric-mod-template - IntelliJ IDEA",
            "Lunaris Notes",
            "lunar",
        ] {
            assert!(!is_game_title(title), "{title} does not name the game");
        }
    }

    /// The confirmation needs both halves: neither a Java process nor a game title is enough.
    #[test]
    fn the_foreground_needs_a_java_launcher_and_a_game_title() {
        fn confirmed(title: &str, process: &str) -> bool {
            let windows: Arc<dyn WindowInspector> = Arc::new(FakeWindows {
                foreground: Some(window(title, process)),
                cursor: None,
            });
            game_is_foreground(windows.as_ref())
        }

        assert!(confirmed("Minecraft 1.8.9", "java"));
        assert!(confirmed("Lunar Client v3.2.1", "javaw.exe"));

        assert!(
            !confirmed("Minecraft 1.8.9", "explorer.exe"),
            "a game title in a non-Java process is not the game"
        );
        assert!(
            !confirmed("fabric-mod-template - IntelliJ IDEA", "java"),
            "a Java process that is not titled as the game is not the game"
        );
        assert!(!confirmed("MainActivity.java - IntelliJ IDEA", "java"));

        let no_foreground = FakeWindows {
            foreground: None,
            cursor: None,
        };
        assert!(
            !game_is_foreground(&no_foreground),
            "no foreground window, no confirmation"
        );
    }

    /// A focused IDE on a mod workspace must not enable injection.
    ///
    /// This is the case the score test missed: `aoko_platform::describe_target`'s markers include
    /// `fabric`, `forge` and `quilt`, so a mod workspace scores as the game. The gate does not
    /// score — it requires a Java launcher name *and* a game title — and this window passes
    /// neither, so every acting call is refused before the platform backend is reached.
    #[test]
    fn a_focused_ide_on_a_mod_workspace_cannot_inject() {
        let editor = gated(Some(window("fabric-mod-template - IntelliJ IDEA", "java")));

        assert!(!editor.confirmed(), "the title names a mod, not the game");
        assert!(
            matches!(
                editor.click(MouseButton::Left),
                Err(PlatformError::Unsupported { .. })
            ),
            "a click must be refused, not aimed at the editor"
        );
        assert!(
            matches!(
                editor.key_press(Key::Char('a')),
                Err(PlatformError::Unsupported { .. })
            ),
            "a key press must be refused for the same reason"
        );
        assert!(
            editor.button_up(MouseButton::Left).is_ok(),
            "a release is never gated: a focus change must not strand a button"
        );
        assert_eq!(
            editor.probe_mouse_state(),
            Some(MouseState::default()),
            "observation is not gated: the hold guards still see the button"
        );

        // The same backend with the game in front: the gate opens, and it is the gate that
        // decides, since `AcceptingInput` would accept on either side.
        let game = gated(Some(window("Minecraft 1.8.9", "java")));

        assert!(game.confirmed());
        assert!(game.click(MouseButton::Left).is_ok());
        assert!(game.move_relative(4, 0).is_ok());
        assert!(game.key_press(Key::Char('a')).is_ok());
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
