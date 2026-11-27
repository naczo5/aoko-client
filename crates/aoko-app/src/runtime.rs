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
//! that store holds. Nothing here dials on its own — the shipped path asks for it once, a
//! test-built runtime never does — so a switch flip is still observable with no bridge
//! present. Re-dialling belongs to the reader: a link that drops is dialled again on the
//! worker, and `crate::runtime::Runtime::link_failure` reports the round in flight as
//! `LinkFailure::Reconnecting`.
//!
//! Supervision is the third: `crate::runtime::Runtime::start_supervision` makes a module
//! switch the trigger for `aoko_core::loops::ClickerLoops`, which owns the seven automation
//! threads, so enabling a loop-driving module starts them and turning the last one off
//! stops them. It is off until the shipped path turns it on. The bundle is not the gate: each
//! body asks `ControllerSense` about its **own** module switch on every tick, so the left
//! clicker stops when the AutoClicker is switched off even while aim assist keeps the bundle
//! alive, and enabling aim assist no longer starts a click stream. The same sense carries the
//! live profile, so a settings edit reaches the engine on the next click instead of at the
//! next restart, and the bridge's advertised lists, so the three assist loops can answer
//! `SupportsModule` the way the managed client did.
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
//! The GTB helper is the third piece of state: `aoko_protocol::gtb::GtbWordSolver` had no
//! production caller before this, so the solver's "a repeated line touches no allocator"
//! claim was priced against a path the binary never ran.
//! `crate::runtime::Runtime::poll_gtb` is called once a frame from
//! `crate::shell::Shell::render`, solves only when the helper is on, the link is connected, the
//! frame is fresh, no menu is open and the action-bar line changed, and
//! `crate::runtime::Runtime::gtb_answer` hands a view the solved word. The wordlist is
//! embedded, so nothing reads a file per tick.
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
//!
//! The stats tracker is the fourth piece, and the one the Stats tab reads: `StatsTracker` is
//! owned here, fed by the loops' own `ClickSink` — which is where the managed host records a
//! click (`Clicker.cs:3270`) — and reset through `crate::runtime::Runtime::reset_stats`, so the
//! view has no copy of its own to go stale. No message carries a click or a CPS value, so the
//! ingest path feeds nothing and this file says so where the sink is defined.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, RwLock, RwLockReadGuard};
use std::time::{Duration, Instant};

use aoko_core::app::{AppController, StateSnapshot};
use aoko_core::chest_stealer::{ScreenRect, FRESH_STATE_MS};
use aoko_core::loops::{
    ClickSink, ClickerLoopConfigs, ClickerLoops, LoopRuntime, LoopSense, AUTOCLICKER_MODULE,
};
use aoko_core::{Clock, Sleeper, SplitMix64, SystemClock, ThreadSleeper};
use aoko_platform::{
    CapabilityReport, InputBackend, InputEvent, Key, MouseButton, MouseState, PlatformError,
    WindowInspector,
};
use aoko_protocol::capabilities::BridgeCapabilities;
use aoko_protocol::gtb::{parse_wordlist_js, GtbWordSolver};
use aoko_protocol::module_catalog::{self, EnabledFlag};
use aoko_protocol::stats::StatsTracker;

use crate::ingest::{Ingest, IngestStatus, LinkFailure, LinkReport};

/// Seed the seven loop bodies start from.
///
/// One RNG per body, as `ClickerLoops::start` expects: the bodies draw click intervals from
/// it, so two bodies sharing a seed would pace identically. Deterministic on purpose — a
/// loop test that failed once can be replayed.
const LOOP_SEED: u64 = 0x00A0_C0DE;

/// The most whole seconds one [`Runtime::sample_playtime`] call may hand the tracker.
///
/// A frame loop calls the sample roughly thirty times a second, so a running app offers one
/// second per call and never reaches the cap. It is reached only after a stall — a debugger
/// break, a suspended machine — where the clock jumped by minutes: replaying those seconds
/// would put time on the Stats tab that the app was not running to observe. The managed host's
/// `Timer` coalesces the ticks it misses for the same reason, so the cap follows it instead of
/// inventing a rule. Five seconds is far beyond any frame gap a running app has.
const PLAYTIME_MAX_CATCH_UP_SECS: u64 = 5;

/// How many keys and mouse buttons [`Runtime::poll_module_keys`] will believe are held at once.
///
/// One entry per press whose release has not been seen yet. A human holds a few keys; the cap
/// exists for the case where a release never arrives — the hook's queue is fixed-capacity
/// (`windows/input.rs::EVENT_CAPACITY`), so a stall long enough to overflow it drops events,
/// and a dropped `KeyUp` or `ButtonUp` would otherwise cost that control's bind for the rest of
/// the session. Past the cap the oldest press is forgotten, which is the recoverable direction:
/// the key dispatches again on its next press instead of never.
const MAX_HELD_KEYS: usize = 16;

/// How long a bridge dial may stay unanswered before the port calls it failed.
///
/// The managed host had no ceiling of its own: `GameStateClient`'s connect either returned or
/// threw, and the socket had whatever the OS offered (`aoko-platform/src/tcp.rs:59-62` records
/// that `TcpClient.ConnectAsync` ran without an explicit timeout). On this machine's loopback a
/// dial is answered in milliseconds — a listener accepts, a closed port refuses — so the only
/// case this deadline covers is a host that drops the SYN, where the worker thread can block for
/// the OS default and the panel would otherwise say `Connecting...` for as long as the process
/// lives.
///
/// Three seconds is a ceiling, not a target: it is far above any loopback handshake and short
/// enough that a user watching the loader gets an answer it can act on. The number is the port's,
/// there is no managed line to cite for it, and it is measured in [`Runtime::now_ms`] so a test
/// crosses it with a step clock instead of sleeping.
pub const CONNECT_DEADLINE_MS: i64 = 3_000;

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

/// The catalog flag the GTB helper is gated on.
///
/// `module_catalog::ALL` names it `gtbhelper`, `cfg.gtbHelper` — `Clicker.GtbHelperEnabled`,
/// which `Clicker.cs:2630` checks before it touches the solver at all.
const GTB_FLAG: EnabledFlag = EnabledFlag::GtbHelper;

/// How many candidate words one solve keeps.
///
/// `Clicker.cs:2637` asks for `maxResults: 200` and renders the joined list, so the app asks
/// for the same number rather than a shorter guess at what the card wants to show.
const GTB_MAX_RESULTS: usize = 200;

/// The shipped wordlist, embedded from this crate's own assets.
///
/// The text is a copy of `Aoko/Data/gtb_wordlist.js` — the file the managed host loaded from
/// disk and `Aoko/Core/GtbWordSolver.cs` read — and it lives at
/// `crates/aoko-app/assets/gtb_wordlist.js`, beside the fonts, for two reasons:
///
/// * `include_str!` may not reach outside the crate directory: `cargo package` rejects a path
///   that leaves the package, and the workspace is meant to be packageable;
/// * the C# tree is scheduled for deletion at sign-off (`MIGRATION_ARCHITECTURE.md` section 2),
///   and a path into it would become a compile error on the day that commit lands.
///
/// The asset's own header carries the provenance and the command that compares it against the
/// managed file, which stays the source of truth until that deletion. This is the one place the
/// two copies could drift, so the header names the comparison rather than leaving it implicit.
const WORDLIST_JS: &str = include_str!("../assets/gtb_wordlist.js");

/// The wordlist parsed once per process, however many runtimes a test builds.
static WORDLIST: OnceLock<Vec<String>> = OnceLock::new();

/// The shipped wordlist, normalised and deduplicated.
fn wordlist() -> &'static [String] {
    WORDLIST
        .get_or_init(|| parse_wordlist_js(WORDLIST_JS))
        .as_slice()
}

/// What the GTB helper last answered, for a view to read.
///
/// A copy: [`Runtime::gtb_answer`] clones, so a render path reads it when it wants the text
/// and calls [`Runtime::poll_gtb`] — which allocates nothing on an unchanged line — every frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GtbAnswer {
    /// The mask the action-bar line normalises to, e.g. `c_t`. Empty when nothing is shown.
    pub mask: String,
    /// Candidate words in wordlist order, at most `GTB_MAX_RESULTS` of them.
    pub matches: Vec<String>,
    /// The action-bar line this answer came from.
    pub line: String,
}

/// The GTB helper: one solver, the line it last answered, and that answer.
///
/// Built on the first line that passes the guards, because the solver indexes the whole
/// wordlist at construction and a runtime that never sees a GTB line should not pay for it.
struct GtbHelper {
    solver: GtbWordSolver,
    /// The line the last solve answered. Compared against the store's line before any work:
    /// the bridge repeats the same action bar on every frame, so this is the whole cost of a
    /// frame that changes nothing.
    last_line: String,
    mask: String,
    matches: Vec<String>,
    /// How many times the solver actually ran.
    ///
    /// This is the work evidence the tests assert: the count is deterministic, and "the same
    /// line twice does the work once" is a statement about this number, not about a clock.
    solves: usize,
}

impl GtbHelper {
    fn new() -> Self {
        Self {
            solver: GtbWordSolver::from_words(wordlist()),
            last_line: String::new(),
            mask: String::new(),
            matches: Vec::new(),
            solves: 0,
        }
    }
}

/// The click feed the loop supervisor writes and the Stats tab reads.
///
/// `aoko_core::loops::ClickSink::record_click` is the production seam, and the two click engines
/// already call it once per accepted click — the other five bodies are handed the same sink by
/// the bundle but never record. That is also where the managed host records a click —
/// `Clicker.cs:3270`, `StatsTracker.Instance.RecordClick(cps, leftButton)`, one line before
/// `PerformClick`.
///
/// **Why not the ingest path:** nothing on the wire carries a click count or a CPS value.
/// `aoko_protocol::codec::Inbound` has `State`, `Capabilities`, `CommandResult` and `Ignored`,
/// and `GameState` has no click field, so there is no decoded message a tracker could be fed
/// from. The `ClickSink` is not a second feed path — it is the managed one.
struct StatsFeed {
    tracker: Mutex<StatsTracker>,
}

impl StatsFeed {
    fn new() -> Self {
        Self {
            tracker: Mutex::new(StatsTracker::new()),
        }
    }

    /// The tracker, ignoring poisoning.
    ///
    /// A loop body that panicked while holding the lock must not take the Stats tab down with
    /// it: the numbers are statistics, not state anything else is derived from.
    fn tracker(&self) -> MutexGuard<'_, StatsTracker> {
        self.tracker.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ClickSink for StatsFeed {
    fn record_click(&self, cps: f32, left_button: bool) {
        self.tracker().record_click(cps, left_button);
    }
}

/// The platform's two surfaces, built once per runtime.
///
/// `aoko_platform::default_services()` is what builds them and it probes the machine, so it is
/// asked for at most once: the loop bundle takes a clone of `input`, and the playtime session
/// reads the same `windows`. A runtime that neither supervises nor starts a session — every
/// test and every capture — never asks, so nothing here reaches the platform at all.
struct PlatformSurfaces {
    input: Arc<dyn InputBackend>,
    windows: Option<Arc<dyn WindowInspector>>,
}

/// The session timer behind the Stats tab's playtime counter.
///
/// `StatsTracker.StartSessionTimer` (`Aoko/Core/StatsTracker.cs:123-131`) is a `Timer` that
/// fires once a second and hands `RecordPlaytimeSample` four facts: whether the game window is
/// foreground, whether the bridge is connected, the current state, and whether the cursor is
/// visible. This is that timer with the same four facts, read from the same [`LoopSense`] the
/// click bodies use, so the playtime counter and the loops cannot disagree about whether the
/// game is in front.
///
/// **It is not a thread.** [`Runtime::sample_playtime`] runs from the shell's one per-frame
/// hook and counts the whole seconds that have passed since the previous call, so a frame that
/// finds none costs one comparison and no lock. The clock is the injected seam:
/// `Runtime::with_time` hands a test a step clock, and the test advances seconds instead of
/// waiting for them.
struct SessionTimer {
    /// What the host can see, built when the session starts over the same window surface the
    /// loops get. No window inspection reads as "not foreground", which counts no playtime
    /// rather than counting it anyway.
    sense: Arc<dyn LoopSense>,
    /// The instant playtime is accounted up to; the next sample counts the whole seconds after
    /// it, so a sub-second remainder is carried rather than dropped.
    last: Instant,
}

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
    /// The platform surfaces, built the first time the loops or the session ask for them.
    platform: Option<PlatformSurfaces>,
    /// The playtime session, `Some` from `Shell::from_system` until the window closes.
    session: Option<SessionTimer>,
    /// The time source the loop bodies park on; a test swaps in its own before enabling.
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
    /// The GTB helper, `None` until a line passes the guards and the module is on.
    gtb: Option<GtbHelper>,
    /// The click feed: the sink the loops record into and the tracker the Stats tab reads.
    ///
    /// One object, so there is one copy of the numbers. `Arc` because the loops hold the sink
    /// on their own threads; `Mutex` because `StatsTracker` records through `&mut self`.
    stats: Arc<StatsFeed>,
    /// The module list the bridge advertises, kept current by the reader thread.
    ///
    /// `aoko_protocol::capabilities::BridgeCapabilities` is the one authority for "this bridge
    /// supports this module", and the reader is the only thing that receives it, so the cell is
    /// shared with `crate::ingest` rather than copied here. Before the first capability frame it
    /// holds the codec's own version fallback — the table the codec would decode against — so a
    /// question asked before the handshake gets the codec's answer rather than a list this
    /// crate invented.
    advertised: Arc<RwLock<BridgeCapabilities>>,
    /// The instant [`Runtime::now_ms`] counts from: the clock's reading at construction.
    epoch: Instant,
    /// Why the link went down, as the reader reported it.
    ///
    /// Shared with the reader thread, which is the only writer: it is the thread that dials and
    /// the thread the link closes on. [`Runtime::link_failure`] reads it, so the panel's reason is
    /// the reader's report rather than a second guess about the same socket.
    link_report: LinkReport,
    /// When the current dial started, on [`Runtime::now_ms`]'s clock, `None` before the first.
    ///
    /// This is the deadline's other half: a dial that is still unanswered this many milliseconds
    /// after it started is a failure to report, even though the worker is blocked in `connect`.
    connect_started_ms: Option<i64>,
    /// The hook's event queue, drained and reused by [`Runtime::poll_module_keys`].
    ///
    /// This runs in the paint, so a fresh `Vec` per frame would be an allocation per frame for a
    /// queue that is almost always empty: the buffer keeps its capacity instead, and the drain
    /// refills it. The events are `InputEvent`s and not key presses because that is what the
    /// backend reports, which is also what makes a mouse bind one arm of the same drain; only the
    /// pointer's own movement in that queue is skipped.
    key_events: Vec<InputEvent>,
    /// The keys and mouse buttons the hook currently reports as down, so one press is one
    /// dispatch.
    ///
    /// A virtual-key code rather than a [`Key`], because a mouse button is a managed pseudo-key
    /// with no [`Key`] spelling: `0x01`..`0x06` is `VK_LBUTTON`..`VK_XBUTTON2`
    /// (`mouse_virtual_key`), which is the code the capture stores and the code a mouse bind
    /// holds, so one latch covers both families — the keyboard's codes start at `0x08`
    /// (`VK_BACK`), so the two cannot collide.
    ///
    /// Both observers repeat a held control — `WM_KEYDOWN` repeats on Windows, and evdev sends
    /// `value == 2` which the observer reports as another `KeyDown`
    /// (`linux/evdev.rs:265-273`) — and a module is a toggle, so trusting the repeats is a
    /// module flickering dozens of times a second. A press enters the list, its release leaves
    /// it, and a repeat finds it already there.
    ///
    /// The list is capped: a `KeyUp` or `ButtonUp` dropped by a full event queue would otherwise
    /// leave a control held for the life of the session and cost that one bind, so past the cap
    /// the oldest entry is dropped instead of the newest press.
    keys_held: Vec<i32>,
}

impl Runtime {
    /// A runtime over a controller the caller built.
    ///
    /// This is the seam a test uses to hold the same controller the UI writes through.
    /// It builds no bridge reader, starts no loop, solves nothing and starts no session:
    /// `connect_bridge`, `start_supervision`, `start_session` and `poll_gtb` are what do work,
    /// and none of them runs on its own.
    #[must_use]
    pub fn new(controller: Arc<AppController>) -> Self {
        Self::with_time(controller, Arc::new(SystemClock), Arc::new(ThreadSleeper))
    }

    /// A runtime with an explicit time source for its loop bodies and its session timer.
    ///
    /// Production passes `SystemClock`/`ThreadSleeper` throughout [`Runtime::new`]; a test
    /// passes a step clock so a body's parks cost no real time and playtime is advanced by the
    /// test rather than by the wall.
    fn with_time(
        controller: Arc<AppController>,
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
    ) -> Self {
        // The clock's own reading is the origin of the millisecond clock, so a step clock in a
        // test moves `now_ms` by exactly the steps the test injects and never by wall time.
        let epoch = clock.now();
        Self {
            controller,
            ingest: None,
            supervise: false,
            loops: None,
            platform: None,
            session: None,
            clock,
            sleeper,
            gtb: None,
            stats: Arc::new(StatsFeed::new()),
            advertised: Arc::new(RwLock::new(BridgeCapabilities::for_version_fallback(None))),
            epoch,
            link_report: LinkReport::new(),
            connect_started_ms: None,
            key_events: Vec::new(),
            keys_held: Vec::new(),
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

    /// Milliseconds on this runtime's clock, counted from its construction.
    ///
    /// The overlay's pulse arithmetic is `i64` milliseconds (`views::command_search::CardPulse`),
    /// while [`Clock::now`] is an `Instant`, so this is the conversion: one subtraction from the
    /// epoch the constructor read off the same clock.
    /// Monotonic, and measured on the injected clock, so a test's step clock advances it by
    /// exactly the steps the test injects and nothing here reads the wall.
    #[must_use]
    pub fn now_ms(&self) -> i64 {
        let elapsed = self.clock.now().saturating_duration_since(self.epoch);
        i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
    }

    /// Whether the bridge advertises one module, by catalogue id.
    ///
    /// The authority is
    /// [`BridgeCapabilities::supports_module`](aoko_protocol::capabilities::BridgeCapabilities::supports_module)
    /// over the list the bridge reports at handshake, which the reader thread keeps in the
    /// shared cell; nothing in the app carries a second list. Case-insensitive, as the codec's
    /// normalisation is, and `false` for everything the bridge has not advertised — which is
    /// what makes an overlay row read unavailable rather than guessed at.
    #[must_use]
    pub fn module_supported(&self, module_id: &str) -> bool {
        advertised_read(&self.advertised).supports_module(module_id)
    }

    /// Whether the bridge advertises one setting, by the name the `config` frame uses.
    ///
    /// The other array of the same cell [`Runtime::module_supported`] reads: `settings` is what the
    /// bridge says it understands, so a press gated on one is asking the bridge rather than this
    /// crate. The reader replaces the set when a `capabilities` frame arrives and the codec's
    /// fallback stands before the handshake, which is the managed `SupportsSetting` answer exactly —
    /// including the `false` that makes a press a no-op against a bridge that never named it.
    #[must_use]
    pub fn setting_supported(&self, setting: &str) -> bool {
        advertised_read(&self.advertised).supports_setting(setting)
    }

    /// Whether the bridge advertises one session action, by its `actions` name.
    ///
    /// The fourth array of the same cell [`Runtime::module_supported`] reads, and the question the
    /// two data tabs ask before they offer a command: `BridgeCapabilities.SupportsAction`
    /// (`ServersView.xaml.cs:50-53`, `AccountsView.xaml.cs:78-82`) is the managed gate, and the
    /// names are [`aoko_protocol::capabilities::actions`]'s — `server.join`, `account.setSession`
    /// and their neighbours. The codec's fallback table carries all four before the handshake, so
    /// the answer is "no" only for a bridge that named a narrower list.
    #[must_use]
    pub fn action_supported(&self, action: &str) -> bool {
        advertised_read(&self.advertised).supports_action(action)
    }

    /// The shared advertised-module cell, for the reader that fills it.
    ///
    /// [`Runtime::connect_bridge`] is the only caller: the ingest thread receives the
    /// capability frames, so it is the one writer, and this gives it the same cell
    /// [`Runtime::module_supported`] reads.
    fn advertised_handle(&self) -> Arc<RwLock<BridgeCapabilities>> {
        Arc::clone(&self.advertised)
    }

    /// Starts the bridge reader on its own thread, once per attempt.
    ///
    /// The dial itself happens on the worker (`crate::ingest::Ingest::spawn`), so this call
    /// returns without waiting for a bridge that is not there. A live link is never dialled again,
    /// and neither is an attempt that has answered nothing and still has time on its deadline —
    /// that is the same dial the panel is already waiting for, and two readers interleaving their
    /// `connected` writes on one store is what the guard exists to prevent.
    ///
    /// An attempt that *has* answered (the reader published a failure) or that has outlived
    /// [`CONNECT_DEADLINE_MS`] is replaced instead, because the press that follows a failure is a
    /// retry and has to actually dial. The reader being replaced is *stopped* before the
    /// replacement dials, not detached: dropping its handle raises its stop flag and shuts the
    /// socket it holds down (`Ingest`'s `Drop`), so it cannot keep the bridge's single accept slot
    /// while the new reader dials for it — which is what left the panel saying `connected` about a
    /// link the old reader no longer read.
    pub fn connect_bridge(&mut self) {
        if self.controller.store().is_connected() || self.attempt_is_live() {
            return;
        }

        // Disposed of as its own statement, before the new reader exists. `Ingest::spawn`
        // evaluates its dial when it is called, so assigning over the old handle would keep the
        // previous reader - and its socket - alive for the whole of the new attempt's first dial,
        // and the two would race for the one connection the bridge accepts.
        self.ingest = None;

        // A new attempt supersedes the last answer, and its deadline starts now.
        self.link_report.clear();
        self.connect_started_ms = Some(self.now_ms());

        // The dial is the reader's, and a test build's reader does not dial: see
        // [`Ingest::spawn`] and `crate::ingest`'s header.
        self.ingest = Some(Ingest::spawn(
            Arc::clone(&self.controller),
            self.advertised_handle(),
            self.link_report.clone(),
        ));
    }

    /// Whether an unanswered dial still has time on its deadline.
    fn attempt_is_live(&self) -> bool {
        if self.link_report.read().is_some() {
            return false;
        }
        self.connect_started_ms
            .is_some_and(|started| self.now_ms() - started < CONNECT_DEADLINE_MS)
    }

    /// Why the link is down, or `None` while it is up or still being dialled.
    ///
    /// Three facts in one place, in this order: what the reader reported, whether the controller's
    /// store says the link is up, and whether the dial has outlived its deadline. The report wins
    /// because it is the socket's own answer; the store answers "is it up"; the deadline is the
    /// only thing anyone can say about a dial nobody answered.
    #[must_use]
    pub fn link_failure(&self) -> Option<LinkFailure> {
        if let Some(reported) = self.link_report.read() {
            return Some(reported);
        }

        let started = self.connect_started_ms?;
        if self.controller.store().is_connected() {
            return None;
        }
        (self.now_ms() - started >= CONNECT_DEADLINE_MS).then_some(LinkFailure::ConnectFailed {
            port: aoko_protocol::BRIDGE_PORT,
        })
    }

    /// Attaches a reader over a transport the caller already connected.
    ///
    /// The seam a test uses to bring the link up and take it down without a socket, the same shape
    /// `crate::ingest::Ingest::attach` gives `ingest`'s own suite: `memory::duplex` on one side,
    /// the reader on the other, so the disconnect the panel paints is the one the real reader
    /// reports rather than one a stub wrote.
    #[cfg(test)]
    pub fn attach_ingest<T: aoko_protocol::transport::Transport + 'static>(
        &mut self,
        transport: T,
    ) {
        // The previous reader first, for the reason [`Runtime::connect_bridge`] gives: it is
        // stopped here rather than after the replacement has already claimed the store.
        self.ingest = None;
        self.link_report.clear();
        self.connect_started_ms = Some(self.now_ms());
        self.ingest = Some(Ingest::attach(
            transport,
            Arc::clone(&self.controller),
            self.advertised_handle(),
            self.link_report.clone(),
        ));
    }

    /// A runtime on a caller-owned step clock, which is what crosses a deadline without sleeping.
    ///
    /// `pub(crate)` rather than `pub`: the clock type is a test item, and a public signature naming
    /// it would be a test seam in the crate's exported API.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn stepped() -> (Self, Arc<StepClock>) {
        let controller = Arc::new(AppController::default());
        let source = Arc::new(StepClock::new());
        let clock: Arc<dyn Clock> = source.clone();
        let sleeper: Arc<dyn Sleeper> = source.clone();
        (Self::with_time(controller, clock, sleeper), source)
    }

    /// What the bridge link currently reports.
    ///
    /// Read out of the controller's store, so it is the same truth the rest of the app
    /// sees; nothing in this crate keeps a second "connected" flag. `fresh` is
    /// `aoko_core::app::UiSnapshot::is_state_fresh`, i.e. the age is inside
    /// `aoko_core::chest_stealer::FRESH_STATE_MS` — answered from the one age this reads
    /// rather than through `AppController::ui_snapshot`, which clones the screen name, the
    /// version and the capability notes for a value nobody else here uses, and this runs
    /// once a frame.
    #[must_use]
    pub fn ingest_status(&self) -> IngestStatus {
        let now = Instant::now();
        let store = self.controller.store();
        let state_age = store.age(now);
        IngestStatus {
            connected: store.is_connected(),
            state_age,
            fresh: state_age.is_some_and(|age| age.as_millis() as u64 <= FRESH_STATE_MS),
        }
    }

    /// The current state when the bridge link is live and the frame is fresh, else `None`.
    ///
    /// Allocation-free on purpose: one atomic load, one `Arc` clone and a duration comparison.
    /// This is what a per-frame caller can afford, where `AppController::ui_snapshot` is not —
    /// that one clones `screen_name` and `version` into an owned `UiSnapshot`.
    fn fresh_state(&self, now: Instant) -> Option<Arc<StateSnapshot>> {
        let store = self.controller.store();
        if !store.is_connected() {
            return None;
        }
        if store.age(now)?.as_millis() as u64 > FRESH_STATE_MS {
            return None;
        }
        store.snapshot()
    }

    /// Solves the action bar into a word, when the helper is on and the round is live.
    ///
    /// `crate::shell::Shell::render` calls this once a frame, so the steady state has to be
    /// free: an unchanged line returns after one `&str` comparison, and the guards are an
    /// atomic load, one `Arc` clone and a duration compare. Nothing here allocates unless the
    /// line is new, and then only the answer, which is the cost the solver's own bench prices.
    ///
    /// Four guards, each for a different reason:
    ///
    /// * **the module is on** — `Clicker.cs:2630` returns before touching the solver, and a
    ///   disabled helper also drops its answer, so flipping the switch on cannot publish a
    ///   result for a line that arrived while it was off;
    /// * **the link is connected** — no frame means no round to answer, and a disconnected
    ///   store still holds the last line it saw;
    /// * **the frame is fresh, inside `FRESH_STATE_MS`** — GTB rounds end, and the action bar
    ///   keeps the completed theme after one does, so a stale line is the previous round's
    ///   answer. This is the guard that makes "enabling must not retroactively solve a stale
    ///   line" true;
    /// * **no GUI is open** — the game does not draw the action bar over a menu, so the line
    ///   while one is up belongs to whatever was there before it. This one is *not* managed
    ///   parity (`UpdateGtbFromActionBar` has no such check); it is here because every other
    ///   consumer in this crate treats `gui_open` as "not gameplay", and it is reported as an
    ///   addition rather than a port.
    pub fn poll_gtb(&mut self, now: Instant) {
        if !self.controller.is_module_enabled(GTB_FLAG) {
            self.gtb = None;
            return;
        }

        let Some(snapshot) = self.fresh_state(now) else {
            return;
        };
        if snapshot.state.gui_open || snapshot.state.action_bar.is_empty() {
            return;
        }

        let line = snapshot.state.action_bar.as_str();
        let helper = self.gtb.get_or_insert_with(GtbHelper::new);
        if helper.last_line == line {
            return;
        }

        // `Clicker.cs:2636` learns the finished word before it solves, which is what lets the
        // next round's one-letter mask resolve immediately. It clears the solver's memo, so it
        // must run before `solve`, not after.
        helper.solver.try_learn_solved_word(line);
        let (mask, matches) = {
            let (mask, matches) = helper.solver.solve(line, GTB_MAX_RESULTS);
            (mask.to_owned(), matches.to_vec())
        };

        helper.last_line.clear();
        helper.last_line.push_str(line);
        helper.mask = mask;
        helper.matches = matches;
        helper.solves += 1;
    }

    /// One frame's worth of module keybind edges, dispatched to the controller.
    ///
    /// This is the managed `KeyboardProc`'s module loop (`InputHooks.cs:406-420`) with the
    /// hook's thread replaced by the frame: drain the queue, and for each press find the
    /// module whose saved virtual key it is and toggle it. Nothing else in the app drains
    /// `InputBackend::poll_events`, so without this every key the user bound to a module
    /// (`Profile::module_keys`, the managed `InputHooks.ModuleKeys` table) does nothing.
    ///
    /// A press is a keystroke *or* a mouse button. The managed mouse hook reported
    /// `VK_LBUTTON`..`VK_XBUTTON2` and the capture stores those same five codes, so a mouse bind
    /// lives in `Profile::module_keys` beside every keystroke bind and takes the same two steps
    /// here — `mouse_virtual_key` gives the code, and the latch and the lookup do not care which
    /// family it came from. Dropping the button events left those binds storable and dead.
    ///
    /// Five properties are load-bearing, and each one is a way this can be wrong:
    ///
    /// * **One dispatch per physical press.** A held key repeats (`WM_KEYDOWN` auto-repeat on
    ///   Windows, evdev `value == 2` on Linux) and a module is a toggle, so repeats are dropped
    ///   through `keys_held` rather than flipping the switch dozens of times a
    ///   second. The managed host repeated, because it had no latch either; a module that
    ///   flickers is the bug a user reports, not the behaviour to match.
    /// * **The comparison is a virtual key**, through [`Key::virtual_key`] for a keystroke and
    ///   `mouse_virtual_key` for a button, because a [`Key`] is a spelling: the Windows hook
    ///   says `Key::Other(0x41)`, evdev says `Key::Char('a')` and a capture stores `0x41`.
    ///   Comparing the values made a letter bind fire on neither observer.
    /// * **The managed guard comes first.** `ShouldBlockModuleKeybinds` (`:340-352`) blocks
    ///   module keybinds unless the game is the foreground window, and when the bridge is
    ///   connected, unless no game screen is open. That is what stops a bound letter toggling a
    ///   module while the user types it into this window's own search box, and it is why a bind
    ///   does nothing while a chest is open — as in the host.
    /// * **Nothing is consumed.** The port reads the hook and leaves the keystroke alone:
    ///   `keyboard_proc` passes every event to `CallNextHookEx`, so a bind never eats the game's
    ///   own key. There is nothing here to undo, and this note is what keeps it that way.
    /// * **No allocation per frame.** The queue and the held set are reused buffers, and the
    ///   binds are read from the live profile by scanning it — a clone per frame would be a
    ///   `String` per module at the frame rate, for a table whose keys are `i32`.
    ///
    /// Returns how many switches were flipped, which is what tells the caller whether a
    /// repaint is needed. A refusal is silent, as in the managed host: an unbound code, a
    /// module the bridge does not advertise, and a dev-only module with dev mode off are all
    /// keys that do nothing.
    pub fn poll_module_keys(&mut self) -> usize {
        let Some(input) = self
            .platform
            .as_ref()
            .map(|platform| Arc::clone(&platform.input))
        else {
            return 0;
        };

        let mut events = std::mem::take(&mut self.key_events);
        events.clear();
        if let Err(error) = input.poll_events(&mut events) {
            tracing::debug!(%error, "global input hook could not be drained");
            self.key_events = events;
            return 0;
        }
        if events.is_empty() {
            self.key_events = events;
            return 0;
        }

        let mut flipped = 0;
        for event in &events {
            match event {
                InputEvent::KeyDown(key) => {
                    if key
                        .virtual_key()
                        .is_some_and(|code| self.press_virtual_key(code))
                    {
                        flipped += 1;
                    }
                }
                InputEvent::KeyUp(key) => {
                    if let Some(code) = key.virtual_key() {
                        self.release_virtual_key(code);
                    }
                }
                // A mouse bind is one of the managed pseudo-keys: the capture stores
                // `VK_LBUTTON`..`VK_XBUTTON2` for these buttons and the profile holds that code,
                // so a button is the same two steps a key takes — one latch, one bind lookup.
                // The arm is the whole enumeration rather than a wildcard, so a button the
                // platform starts reporting cannot be dropped here in silence.
                InputEvent::ButtonDown(button) => {
                    if self.press_virtual_key(mouse_virtual_key(*button)) {
                        flipped += 1;
                    }
                }
                InputEvent::ButtonUp(button) => {
                    self.release_virtual_key(mouse_virtual_key(*button));
                }
                // The pointer position rides the same queue and names no bind.
                InputEvent::MouseMove { .. } => {}
            }
        }

        self.key_events = events;
        flipped
    }

    /// One press of a virtual key or a mouse button: latched, then dispatched.
    ///
    /// Returns whether a module moved. The latch is `keys_held`, so a held control dispatches
    /// once however often the hook repeats it, and a press the platform gives no virtual key
    /// never reaches here at all.
    fn press_virtual_key(&mut self, code: i32) -> bool {
        if self.keys_held.contains(&code) {
            // The auto-repeat of a press already dispatched.
            return false;
        }
        if self.keys_held.len() >= MAX_HELD_KEYS {
            self.keys_held.remove(0);
        }
        self.keys_held.push(code);
        self.dispatch_module_key(code)
    }

    /// The release of a virtual key or a mouse button, which re-arms its next press.
    fn release_virtual_key(&mut self, code: i32) {
        if let Some(index) = self.keys_held.iter().position(|held| *held == code) {
            self.keys_held.swap_remove(index);
        }
    }

    /// Toggles the module the virtual key or mouse button `code` is bound to, and reports whether
    /// it moved.
    ///
    /// `InputHooks.ToggleModule`'s guards, in its order: an unsupported module is ignored so a
    /// keybind cannot desynchronise the two sides, and a dev-only module needs dev mode. Both
    /// answer before the switch is written, and neither allocates.
    fn dispatch_module_key(&mut self, code: i32) -> bool {
        // The live profile, scanned rather than cloned: a capture that just recorded a key has
        // to be in force on the next press, which is the reason this is not cached.
        let bound = self.controller.with_profile(|profile| {
            profile
                .module_keys
                .iter()
                .find(|(_, saved)| **saved == code)
                .map(|(module, _)| module.clone())
        });
        let Some(module) = bound else {
            return false;
        };

        // The managed guard is checked after the match, so a key no module claims costs one
        // profile scan and no window query at all.
        if !self.module_keybinds_allowed() {
            return false;
        }
        if !self.module_supported(&module) {
            return false;
        }
        if module_catalog::is_dev_only(&module) && !self.controller.with_profile(|p| p.dev_mode) {
            return false;
        }

        self.flip_module(&module)
    }

    /// `!InputHooks.ShouldBlockModuleKeybinds` (`InputHooks.cs:312-321`).
    ///
    /// Three facts, in the managed order. The game must be the confirmed foreground window,
    /// which is the same strict [`game_is_foreground`] the injection gate reads — so a key
    /// pressed while this window is focused cannot toggle anything, and typing a bound letter
    /// into a search box is just typing. Then, when the bridge is connected, the game must not
    /// be showing a screen: a menu, an inventory or a chest is not gameplay, and the managed
    /// host blocked there too. Without a connection the cursor is the only signal, and a
    /// backend that cannot report it reads as "not drawn" — the managed `GetCursorInfo`
    /// failure answer — rather than as a refusal that would leave every bind dead.
    fn module_keybinds_allowed(&self) -> bool {
        let Some(windows) = self
            .platform
            .as_ref()
            .and_then(|platform| platform.windows.as_deref())
        else {
            // No window surface, no confirmation: the same fail-closed answer the injection
            // gate gives a session that cannot see windows.
            return false;
        };
        if !game_is_foreground(windows) {
            return false;
        }

        let store = self.controller.store();
        if store.is_connected() {
            let snapshot = store.snapshot();
            let fallback = aoko_protocol::state::GameState::default();
            let state = snapshot
                .as_deref()
                .map_or(&fallback, |snapshot| snapshot.state.as_ref());
            return !any_game_screen_open(state);
        }

        // `WindowDetection.IsCursorVisible()` is a `false` on failure, so "cannot tell" here
        // must not block: only a backend that says the pointer *is* drawn does.
        windows.cursor_visible() != Some(true)
    }

    /// What the helper last answered, or `None` when there is nothing to show.
    ///
    /// A view reads this instead of the store: it is the solved word, already normalised, and
    /// reading it does no work. The copy is deliberate — a render path that wants the text asks
    /// for it when the answer changes, not every frame.
    ///
    /// `None` also covers an action bar with no mask in it — the line that reports a completed
    /// build carries no underscores, so the solver answers with an empty mask and the managed
    /// host shows `-` for it (`Clicker.SetGtbState("", 0, "")`).
    #[must_use]
    pub fn gtb_answer(&self) -> Option<GtbAnswer> {
        let helper = self.gtb.as_ref()?;
        if helper.mask.is_empty() {
            return None;
        }
        Some(GtbAnswer {
            mask: helper.mask.clone(),
            matches: helper.matches.clone(),
            line: helper.last_line.clone(),
        })
    }

    /// How many times the solver has run, `0` before the first line and while the module is
    /// off.
    ///
    /// The work evidence: two polls of one line leave this at one, and no clock is involved.
    #[must_use]
    pub fn gtb_solves(&self) -> usize {
        self.gtb.as_ref().map_or(0, |helper| helper.solves)
    }

    /// The Stats tab's tracker, borrowed.
    ///
    /// A guard rather than a copy: `StatsTracker` is not `Clone`, the view reads a dozen numbers
    /// from it, and a copy is exactly the stale second source this slice removes.
    ///
    /// **A guard for one read, not for a paint.** Everything that writes the tracker takes the same
    /// lock (`ClickSink::record_click` from a loop thread, [`Runtime::reset_stats`]), and
    /// `MutexGuard` is neither `Send` nor reentrant: a caller that keeps the guard alive across a
    /// callback which resets the stats — `views::stats_view::render`'s reset button — deadlocks the
    /// window against itself. Read out what is needed, or let the guard drop at the end of the
    /// statement.
    ///
    /// No `#[must_use]`: `MutexGuard` already carries one, and saying it twice is a clippy error
    /// rather than a stronger promise.
    pub fn stats(&self) -> MutexGuard<'_, StatsTracker> {
        self.stats.tracker()
    }

    /// The sink the loop bodies record into.
    ///
    /// The same object the supervisor was handed, so a test can feed a click through the
    /// production path without starting a thread.
    #[must_use]
    pub fn click_sink(&self) -> Arc<dyn ClickSink> {
        // `Arc::clone` does not unsize; the annotation on the binding is what coerces
        // `Arc<StatsFeed>` into `Arc<dyn ClickSink>`, so the clone has to land in one first.
        let sink: Arc<dyn ClickSink> = self.stats.clone();
        sink
    }

    /// `StatsTracker.Reset` (`MainWindow.xaml.cs:1978`), through the one owner of the tracker.
    ///
    /// Returns whether there was anything to clear, so a caller can repaint only on a real
    /// change.
    pub fn reset_stats(&self) -> bool {
        let mut tracker = self.stats.tracker();
        let had_samples = tracker.total_clicks() > 0;
        tracker.reset();
        had_samples
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

    /// Starts the playtime session: the Stats tab's counter begins at this instant.
    ///
    /// `Shell::from_system` is the only caller, next to `start_supervision`. That is where the
    /// managed host starts its own timer (`MainWindow.xaml.cs:421`), so the two agree about when
    /// a session begins. A headless runtime and every capture never call this, and
    /// `the_session_timer_counts_playtime_from_the_clock` pins that they count nothing however
    /// long they live.
    ///
    /// The sense is built over the same window surface the loops get, so playtime is only
    /// counted while [`game_is_foreground`] confirms the game — the same strict rule that gates
    /// injection. A session that cannot inspect windows has no sense that says "foreground",
    /// and therefore accrues no playtime.
    pub fn start_session(&mut self) {
        if self.session.is_some() {
            return;
        }
        let windows = self.platform().windows.clone();
        let sense: Arc<dyn LoopSense> = Arc::new(
            ControllerSense::new(Arc::clone(&self.controller), windows)
                .with_advertised(self.advertised_handle()),
        );
        self.start_session_with(sense);
    }

    /// The session start with the sense handed in.
    ///
    /// The seam a test uses to drive the timer over a fake window inspector: it cannot reach
    /// [`Runtime::platform`] without a capability report that claims window inspection, and a
    /// report that claims it would build the real inspector.
    fn start_session_with(&mut self, sense: Arc<dyn LoopSense>) {
        let last = self.clock.now();
        self.session = Some(SessionTimer { sense, last });
    }

    /// Stops the playtime session and keeps the total.
    ///
    /// The managed host stops its timer in `OnClosed` (`MainWindow.xaml.cs:1865`) — when the
    /// window closes, not when the bridge disconnects — and `StopSessionTimer`
    /// (`StatsTracker.cs:129`) only disposes the timer: `TotalPlaytime` keeps its value until
    /// `Reset`. This follows that, so a bridge that drops and reconnects does not lose the
    /// session's time, and the shell's close hook is the only caller.
    pub fn stop_session(&mut self) {
        self.session = None;
    }

    /// The session timer's tick: offers the tracker the whole seconds since the last call.
    ///
    /// Returns how many seconds were offered, so a test can assert the clock reading rather
    /// than the total. Whether a second *counts* is the tracker's own decision:
    /// `StatsTracker.record_playtime_sample` returns early when the game is not foreground, or
    /// when the bridge is connected with a blocking GUI open
    /// (`Aoko/Core/StatsTracker.cs:181-193`), so this feeds it the four facts and adds nothing of
    /// its own.
    ///
    /// At most five seconds are offered per call (see `PLAYTIME_MAX_CATCH_UP_SECS`), and a
    /// sub-second remainder is carried into the next one.
    pub fn sample_playtime(&mut self) -> u64 {
        // No session is the headless and capture case: one branch, and the clock is not
        // read at all.
        let Some(session) = self.session.as_mut() else {
            return 0;
        };

        let now = self.clock.now();
        let elapsed = now.saturating_duration_since(session.last);
        let whole = elapsed.as_secs();
        if whole == 0 {
            return 0;
        }

        let counted = whole.min(PLAYTIME_MAX_CATCH_UP_SECS);
        // Advance by the seconds that were counted rather than to `now`, so the sub-second
        // remainder is carried: at thirty frames a second the sample lands on the second
        // boundary every time instead of drifting a frame later on each tick. When the cap
        // clips a long stall there is no remainder worth carrying, so jump to `now` and drop
        // it — see the constant.
        session.last = if whole > PLAYTIME_MAX_CATCH_UP_SECS {
            now
        } else {
            session.last + Duration::from_secs(counted)
        };

        let sense = Arc::clone(&session.sense);
        let connected = sense.is_connected();
        let active = sense.minecraft_active();
        let cursor_visible = sense.cursor_visible();
        let snapshot = sense.snapshot();

        // No snapshot is not "no game": the managed host reads `GameStateClient.CurrentState`,
        // which is a default instance before the first frame, and still counts the second when
        // the game is foreground and no menu is open. One default per tick, not per second.
        let fallback = aoko_protocol::state::GameState::default();
        let state: &aoko_protocol::state::GameState = snapshot
            .as_deref()
            .map_or(&fallback, |snapshot| snapshot.state.as_ref());

        let mut tracker = self.stats.tracker();
        for _ in 0..counted {
            tracker.record_playtime_sample(active, connected, state, cursor_visible);
        }
        counted
    }

    /// The names the supervisor has registered, empty when nothing is running.
    ///
    /// `ClickerLoops` starts its seven bodies as one bundle, so enabling any loop-driving
    /// module registers all seven names, and the last one to be disabled stops all seven.
    /// Registered is not the same as acting: each body also checks its own switch on every
    /// tick, so the bundle can outlive the module that raised it without that module's body
    /// doing anything.
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

    /// The platform's two surfaces, built on first use and shared afterwards.
    ///
    /// [`select_loop_surfaces`] is still the only decision about *what* the session may reach;
    /// this decides only *when* the services behind that decision are built.
    /// `aoko_platform::default_services` probes the machine, so a session that both starts the
    /// loop bundle and counts playtime must not probe it twice. A runtime that does neither
    /// never calls this, which is why no test and no capture touches the platform.
    fn platform(&mut self) -> &PlatformSurfaces {
        let surfaces = select_loop_surfaces(self.controller.capabilities());
        self.platform.get_or_insert_with(|| {
            let (input, windows) = platform_surfaces(&surfaces);
            PlatformSurfaces { input, windows }
        })
    }

    /// Starts the seven bodies on their own threads.
    ///
    /// The backend and the window surface are not chosen here:
    /// [`crate::runtime::select_loop_surfaces`] decides from the controller's capability
    /// report, and `platform_surfaces` builds exactly what that decision allows. Both surfaces
    /// come from [`Runtime::platform`], so a session that is also counting playtime shares one
    /// construction of them.
    fn start_loops(&mut self) {
        let (input, windows) = {
            let platform = self.platform();
            // Already unsized, so a plain clone — `Arc::clone` on a concrete `Arc` would need
            // the annotation to coerce, and there is nothing to coerce here.
            (Arc::clone(&platform.input), platform.windows.clone())
        };
        let sense: Arc<dyn LoopSense> = Arc::new(
            ControllerSense::new(Arc::clone(&self.controller), windows)
                .with_advertised(self.advertised_handle())
                .with_input(Some(Arc::clone(&input))),
        );
        // The one click feed: the sink the bodies record into, which is `StatsFeed`.
        let sink: Arc<dyn ClickSink> = self.stats.clone();

        let clock = Arc::clone(&self.clock);
        let sleeper = Arc::clone(&self.sleeper);
        let make_runtime = move |seed: u64| {
            LoopRuntime::new(
                Arc::clone(&clock),
                Arc::clone(&sleeper),
                Box::new(SplitMix64::new(LOOP_SEED.wrapping_add(seed))),
            )
        };

        match ClickerLoops::start(sense, input, sink, make_runtime) {
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
        module_catalog::flag_for_id(module_id)
            .is_some_and(|flag| self.controller.is_module_enabled(flag))
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
        if let Some(flag) = module_catalog::flag_for_id(module_id) {
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
        let Some(flag) = module_catalog::flag_for_id(module_id) else {
            return false;
        };
        let enabled = !self.controller.is_module_enabled(flag);
        self.controller.set_module_enabled(flag, enabled);
        self.reconcile_loops();
        true
    }

    /// `Clicker.IsArmed` — as far as this port models it, which is one flag where managed had two.
    ///
    /// The catalog models the AutoClicker's armed state as [`EnabledFlag::LeftClick`]: its entry's
    /// overlay probe is `cfg.armed` (`module_catalog.rs:228-233`), which is the string the injected
    /// bridge reports for `IsArmed`. There is no separate armed bit on the controller, so this flag
    /// is the only honest source and a second copy would be exactly the split this slice removes.
    ///
    /// **This is a recorded deviation, not a bug to fix here.** Managed held two bits —
    /// `IsArmed` and `LeftClickEnabled` (`Clicker.cs`) — so its `Disarm()` left the clicker
    /// disarmed with left click still enabled, and its chip could say one thing while the other was
    /// true. The port holds one. Two consequences follow and both are accepted: the title chip's
    /// `armed` reports the left clicker rather than a separate arm state, and `PANIC_ON` turning this
    /// flag on is the panic's own definition — it disarms by ending the session, not by clearing a
    /// second bit that does not exist. Inventing that bit at this stage, to change what an existing
    /// label counts, would be the larger change; the frame's `armed` is being moved onto this same
    /// flag, so the chip and the wire will agree with each other even where both differ from managed.
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

/// The advertised-module cell's read guard, with a poisoned lock recovered rather than skipped.
///
/// The cell guards the bridge's advertised lists - a table a capability frame replaces wholesale -
/// so the value a panicking reader left behind is stale, not corrupt. Answering "nothing is
/// supported" instead would be worse than the stale table: every module gate in the app reads this
/// cell, so a panicked reader would switch off every capability-gated control and refuse every
/// module keybind for the rest of the session. `crate::ingest` recovers the write side the same way.
fn advertised_read(cell: &RwLock<BridgeCapabilities>) -> RwLockReadGuard<'_, BridgeCapabilities> {
    cell.read().unwrap_or_else(PoisonError::into_inner)
}

/// The loop a catalog flag drives, `None` for a module that is only a setting.
///
/// The catalog has no loop column: `Entry` carries the id, the flag, the surfaces and the
/// settings, and nothing that names a thread. So the table lives here, keyed by
/// `EnabledFlag` — the catalog's own identity — rather than by a spelling of the module id,
/// which means a catalog rename cannot desynchronise it. `every_loop_driver_is_a_catalog_entry`
/// pins each arm to a real entry and pins the names to the strings `ClickerLoops` registers.
///
/// The seven arms are the seven bodies `ClickerLoops::start` spawns. Everything else
/// (`jitter`, `clickinchests`, `reach`, ...) is read by one of those bodies as a setting
/// or a guard, so enabling it starts no thread of its own.
fn loop_for(flag: EnabledFlag) -> Option<&'static str> {
    match flag {
        EnabledFlag::LeftClick => Some("click-left"),
        EnabledFlag::RightClick => Some("click-right"),
        EnabledFlag::AimAssist => Some("aim-assist"),
        EnabledFlag::PixelPartyAssist => Some("pixel-party"),
        EnabledFlag::Triggerbot => Some("triggerbot"),
        // The two container controllers the managed `Clicker` owned beside the clicker
        // (`_chestStealerController` / `_refillController`, `Clicker.cs:521-522`), armed by
        // `ChestStealerEnabled` (`:1076-1090`) and `RefillEnabled` (`:1174-1188`). Without these
        // arms the switches started nothing, so neither module could ever press a button.
        EnabledFlag::ChestStealer => Some("chest-stealer"),
        EnabledFlag::Refill => Some("refill"),
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

/// Whether the game is the foreground window, by the managed rule.
///
/// `WindowDetection.IsMinecraftActive` (`WindowDetection.cs:184-204`) is **title-only**: the
/// foreground window is the game when its title contains one of `GameTitles` (`Minecraft`,
/// `Lunar Client`, `Badlion`, ordinal substring). [`aoko_platform::is_game_window`] is that
/// rule, and it is the whole test here.
///
/// **The process-name half was this port's addition and is deliberately gone.** The guard used
/// to require `foreground.is_jvm()` as well. The managed host never required it on the acting
/// path — the click loop, the four assist loops and both container controllers gate on
/// `IsMinecraftActive`/`IsMinecraftForeground`, both title-based — so a client whose process
/// name the platform cannot resolve (a permission-restricted `/proc`, a Wine/Proton wrapper, a
/// renamed launcher) passed the managed check and failed this one: every loop parked, every
/// synthetic click was refused, and the only symptom was a module that did nothing. Parity wins
/// over the extra strictness here because the strictness was never in the reference.
///
/// The case the pair used to catch is still caught by the title test, which is why it is kept:
/// a focused IDE on a mod workspace (`fabric-mod-template - IntelliJ IDEA`) names no game and is
/// refused. What a title test cannot catch — an editor whose *project* is called `minecraft-…`
/// — is a residual of the managed rule too, so this port inherits it rather than inventing an
/// IDE blocklist; closing it needs the managed cached-handle comparison against a full window
/// enumeration, which is `select_loop_surfaces`' window surface and not this function.
///
/// A missing window surface, a foreground the backend cannot read, and a window with no title
/// all answer `false`: this is still a guard, so it fails closed.
#[must_use]
pub fn game_is_foreground(windows: &dyn WindowInspector) -> bool {
    let Ok(Some(foreground)) = windows.foreground() else {
        return false;
    };
    if !aoko_platform::is_game_window(&foreground) {
        return false;
    }
    // The title alone is not the managed rule, and it is not safe on its own: a browser tab or an
    // editor titled `Minecraft 1.8.9` passes it, and the only thing between that window and a
    // synthetic click would be this function. `WindowDetection.IsMinecraftActive` pairs the title
    // with a live `FindMinecraftWindow` target; the port pairs it with the JVM name test.
    //
    // The *unknown* case is the one that used to be wrong. Requiring `is_jvm` outright meant a
    // client whose process name the backend could not resolve (`process_name: None` — a protected
    // process, a handle without query rights) was treated as "not the game", and every module
    // silently stopped acting in a real session. An unreadable name is not evidence against the
    // title, so it passes: exactly one unverifiable fact, and the title already matched.
    match foreground.process_name.as_deref() {
        Some(name) if !name.is_empty() => aoko_platform::is_java_process_name(name),
        _ => true,
    }
}

/// `InputHooks.IsAnyGameScreenOpen` (`InputHooks.cs:166-175`): whether the game is showing a
/// screen rather than gameplay.
///
/// Two facts, either of which is a screen: the `GuiOpen` flag, and a `screenName` that names
/// something. `none`, `unknown` and an empty name are the bridge's own words for "no screen" —
/// `unknown` is `GameState`'s default (`aoko-protocol/src/state.rs:26-28`), so a frame that
/// carries no screen name does not count as one.
#[must_use]
fn any_game_screen_open(state: &aoko_protocol::state::GameState) -> bool {
    if state.gui_open {
        return true;
    }
    let screen = state.screen_name.trim();
    !(screen.is_empty()
        || screen.eq_ignore_ascii_case("none")
        || screen.eq_ignore_ascii_case("unknown"))
}

/// The managed virtual key of one mouse button, `VK_LBUTTON`..`VK_XBUTTON2`.
///
/// `Aoko/Core/InputHooks.cs:40-44` and `:429-448` are the reference: the mouse hook reported the
/// button as one of these five codes, the capture stores the same number
/// (`components/keybind_capture.rs::mouse_virtual_key`), and `views::keybind_text` names them one
/// by one because they are not contiguous — `0x03` is `VK_CANCEL` and names no button. A mouse bind
/// is therefore written into `Profile::module_keys` in the same table, and as the same code, as a
/// keystroke.
///
/// The arm is the whole enumeration on purpose: a button the platform starts reporting has to be
/// given a code here rather than falling into a wildcard that no bind could ever match.
#[must_use]
fn mouse_virtual_key(button: MouseButton) -> i32 {
    match button {
        MouseButton::Left => 0x01,
        MouseButton::Right => 0x02,
        MouseButton::Middle => 0x04,
        MouseButton::X1 => 0x05,
        MouseButton::X2 => 0x06,
    }
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

    /// Gated like every other acting call, and it has to be spelled out here rather than
    /// inherited.
    ///
    /// `InputBackend::move_to`'s default refuses (a backend has to opt in to placing the
    /// cursor), so leaving this unoverridden would hand the chest stealer an `Unsupported`
    /// on a machine where the platform backend implements it: the module would park at the
    /// pre-move guard forever with no error a user could see. The gate is the same one
    /// `click` uses — an absolute move is a click's first half.
    fn move_to(&self, x: f32, y: f32) -> Result<(), PlatformError> {
        self.require_game()?;
        self.platform.move_to(x, y)
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
/// conservative sense.
///
/// The reachable surfaces are the platform's own implementations, not wrappers: nothing in this
/// crate re-implements an input call or a window query. The one addition is the gate —
/// `ForegroundGated` refuses an acting call the platform backend would have performed, which is
/// a safety condition rather than a second implementation of the call.
///
/// Building a reachable surface is also where the process-wide input hook is installed on
/// Windows, for the reason spelled out at the call: the backend observes nothing until someone
/// installs it, and this is the one place a session decides it can reach the platform.
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
    // The observation hook has no other installer: `WindowsInputBackend::new` only reads a
    // queue, so a session that never installs the hook is a session whose queue stays empty and
    // whose module keybinds are all dead. This is the thread that pumps the window's messages,
    // which is the thread a `WH_KEYBOARD_LL` hook is delivered on, and the install is
    // idempotent, so a second surface build costs one branch.
    #[cfg(windows)]
    {
        if let Err(error) = aoko_platform::install_hooks() {
            tracing::warn!(
                %error,
                "the global input hook could not be installed: module keybinds will not fire"
            );
        }
    }
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

/// The read side of a loop body, over the controller's store, its switches and profile, and,
/// when the capability report allows it, the platform's window surface.
///
/// `aoko_core::app::StoreSense` is the store-and-cursor implementation, but it is built over an
/// `Arc<StateStore>` and [`AppController::store`] hands out `&StateStore`, so a
/// `StoreSense` cannot be built here without a second store or a change in `aoko-core`. This
/// is that read side over the controller, plus the three things the loops lost by not going
/// through it: the module switch that owns each body
/// ([`LoopSense::module_enabled`] → [`AppController::is_module_enabled`]), the clicker settings
/// ([`LoopSense::loop_configs`] → [`AppController::with_profile`]), and the bridge's advertised
/// module and state-field lists ([`Runtime::advertised_handle`]).
///
/// The window half is [`crate::runtime::select_loop_surfaces`]'s decision: `windows` is `None`
/// exactly when that function refused the surface. Without it the game can never be confirmed
/// foreground, so `minecraft_active` answers `false`, and the cursor answers the conservative
/// "drawn" that parks every consumer rather than the permissive "hidden".
struct ControllerSense {
    controller: Arc<AppController>,
    windows: Option<Arc<dyn WindowInspector>>,
    /// The bridge's advertised lists, shared with the reader thread that fills them.
    ///
    /// `None` for a sense a test builds, which answers `false` to every support question — the
    /// same answer the shared cell gives before a handshake, and the reason `supports_module`
    /// used to answer `false` in production too: the loops therefore never ran aim assist,
    /// Pixel Party assist or the triggerbot, all three of which gate on it
    /// (`Clicker.cs:2665`, `:2727-2729`, `:2853`).
    advertised: Option<Arc<RwLock<BridgeCapabilities>>>,
    /// The platform input backend, shared with the loops.
    ///
    /// The click engines need it for one question: is the button held? In the managed client the
    /// low-level mouse hook answered that by *starting* the click stream (`InputHooks.cs:565-585`),
    /// so the port's loops have to ask the backend that owns the same observation. `None` for a
    /// sense a test builds, which then models no hold at all.
    input: Option<Arc<dyn InputBackend>>,
}

impl ControllerSense {
    fn new(controller: Arc<AppController>, windows: Option<Arc<dyn WindowInspector>>) -> Self {
        Self {
            controller,
            windows,
            advertised: None,
            input: None,
        }
    }

    /// Hands the sense the bridge's advertised lists, as [`Runtime::connect_bridge`] fills them.
    ///
    /// The cell is the one [`Runtime::module_supported`] reads, so the loops and the UI cannot
    /// disagree about what the attached bridge implements.
    fn with_advertised(mut self, advertised: Arc<RwLock<BridgeCapabilities>>) -> Self {
        self.advertised = Some(advertised);
        self
    }

    /// Hands the sense the input backend, so the click engines can see their button held.
    fn with_input(mut self, input: Option<Arc<dyn InputBackend>>) -> Self {
        self.input = input;
        self
    }

    /// `Clicker.IsUsingLeftButton`, as far as this port models it: the left click engine's
    /// switch (`EnabledFlag::LeftClick`, which the AutoClicker card toggles).
    ///
    /// Read here rather than inside the profile lock: `AppController::replace_profile` takes
    /// `modules` and then `profile`, so anything that read `modules` while holding `profile`
    /// could deadlock against a config load.
    fn left_clicking(&self) -> bool {
        module_catalog::flag_for_id(AUTOCLICKER_MODULE)
            .is_some_and(|flag| self.controller.is_module_enabled(flag))
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

    /// `InputHooks.IsPhysicalLeftButtonDown` and its right twin, straight from the backend.
    ///
    /// Probed per tick rather than cached: the click engines sample this between clicks and
    /// while they wait for one, and a cached flag would keep a stream alive after a release.
    /// The backend's own probe already filters *injected* presses on both platforms — the
    /// Windows hook skips `LLKHF_INJECTED`/`LLMHF_INJECTED` and the Linux observer skips the
    /// client's own uinput device — which is the property that makes this safe to gate on: the
    /// engine's own clicks never read back as a held button.
    ///
    /// A backend that cannot answer is treated as "nothing held". That is fail-closed on
    /// purpose: the managed client had no way to click without its hook, and a clicker that
    /// cannot see the mouse must not spray clicks into whatever window has focus.
    fn physical_mouse(&self) -> Option<MouseState> {
        let input = self.input.as_deref()?;
        match input.probe_mouse_state() {
            Some(state) => Some(state),
            None => {
                static WARNED: std::sync::Once = std::sync::Once::new();
                WARNED.call_once(|| {
                    tracing::warn!(
                        "the input backend cannot sample the mouse: the click engines will not \
                         click until it can, because a click stream needs a held button"
                    );
                });
                Some(MouseState::default())
            }
        }
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
    /// **Linux cannot answer this, and `None` is its normal answer.** `LinuxWindowInspector`
    /// overrides the default to return `None` explicitly: `XQueryPointer` never reports
    /// visibility, `XFixes` exposes no getter, the game hides the pointer with a client-side
    /// `XDefineCursor` whose only trace is cursor-image pixels, and Wayland exposes nothing.
    /// `CapabilityReport::notes` names the limit on every Linux session, so this is a reported
    /// platform limit rather than an emergency path.
    ///
    /// `None` therefore maps to `true`: assume the cursor is drawn. Every consumer reads `false`
    /// as permission — `loops/triggerbot.rs` fires on `!cursor_visible`;
    /// `clicker/tick.rs` reads a GUI flag with a hidden cursor as a ghosted GUI and skips the
    /// menu block; `loops/aim.rs` and `loops/pixel_party.rs` only take their GUI branch when the
    /// cursor is visible — so `true` parks the triggerbot, blocks menus and takes the GUI path.
    /// `false` would do the opposite on all four. The managed host returns `false` for a failing
    /// `GetCursorInfo` because that is a rare Windows failure; a backend that can *never* answer
    /// must not copy the permissive value.
    ///
    /// `an_unanswered_cursor_parks_the_consumers` pins those consequences, not the mapping.
    /// [`ForegroundGated`] still refuses every acting call unless the game is the confirmed
    /// foreground window, so neither direction of this guess can aim a loop at another
    /// application.
    fn cursor_visible(&self) -> bool {
        self.windows
            .as_deref()
            .and_then(|windows| windows.cursor_visible())
            // "Cannot tell" is `true` on purpose: see the doc above.
            .unwrap_or(true)
    }

    /// `GameStateClient.SupportsModule(id)`, from the bridge's own advertised list.
    ///
    /// The three assist loops gate on this exactly as managed did, and a hardcoded `false` is
    /// why aim assist, Pixel Party assist and the triggerbot did nothing in the packaged build:
    /// their bodies parked on the very first condition, forever.
    fn supports_module(&self, module: &str) -> bool {
        self.advertised
            .as_ref()
            .is_some_and(|cell| advertised_read(cell).supports_module(module))
    }

    fn supports_state_field(&self, field: &str) -> bool {
        self.advertised
            .as_ref()
            .is_some_and(|cell| advertised_read(cell).supports_state_field(field))
    }

    /// The catalog switch behind one loop-driving module, read live.
    ///
    /// This is the gate each body checks every tick. An id the catalog does not carry, or one
    /// with no boolean behind it, answers `Some(false)` — off — rather than `None`, because
    /// `None` means "this sense models no switches" and would let the body run ungoverned.
    fn module_enabled(&self, module: &str) -> Option<bool> {
        Some(
            module_catalog::flag_for_id(module)
                .is_some_and(|flag| self.controller.is_module_enabled(flag)),
        )
    }

    /// `Clicker`'s clicker settings, mapped from the controller's live profile on every read.
    ///
    /// The managed loops read those properties inside their `while`, so a slider or a guard
    /// took effect on the next click; a settings set captured when the bundle started ignored
    /// every edit until a restart.
    fn loop_configs(&self) -> Option<ClickerLoopConfigs> {
        let left_clicking = self.left_clicking();
        Some(
            self.controller
                .with_profile(|profile| ClickerLoopConfigs::from_profile(profile, left_clicking)),
        )
    }

    /// `WindowDetection.GetMinecraftWindowRect()` (`WindowDetection.cs:245-254`).
    ///
    /// Only the foreground window is considered. The managed helper searched every top-level
    /// window for the game; each caller here (aim assist's viewport fallback, the triggerbot's)
    /// only runs while the game *is* the foreground window, so the focused window is the same
    /// answer for a fraction of the cost — one `WindowInspector::foreground` per tick instead of
    /// an `enumerate`.
    ///
    /// `None` when the session refused the window surface, when nothing is focused, when the
    /// focused window is not the game, or when its bounds are empty. `None` is what makes the
    /// aim loop reset instead of moving with a guessed aspect ratio.
    fn framebuffer_size(&self) -> Option<(i32, i32)> {
        let windows = self.windows.as_deref()?;
        let Ok(Some(foreground)) = windows.foreground() else {
            return None;
        };
        if !aoko_platform::is_game_window(&foreground) {
            return None;
        }
        let bounds = foreground.bounds;
        if !bounds.is_non_empty() {
            return None;
        }
        Some((bounds.width, bounds.height))
    }

    /// `WindowDetection.GetMinecraftClientRectOnScreen()` (`WindowDetection.cs:256-276`).
    ///
    /// The chest-stealer and refill bodies scale a slot's GUI-scaled coordinate into this rect
    /// before they click (`ChestStealerCoordinateMapper.cs:20-25`), so it has to be the
    /// **client** area — `WindowInspector::client_bounds` is `GetClientRect` mapped through
    /// `ClientToScreen` — and not [`Self::framebuffer_size`]'s outer rect, which includes the
    /// decorations and would move every click up by the height of the title bar.
    ///
    /// Only the focused game window is considered, for the reason [`Self::framebuffer_size`]
    /// gives: both container loops gate on the game being foreground, so the focused window is
    /// the one whose slots are on screen. `None` when the surface is refused, nothing is
    /// focused, the focused window is not the game, or the client area is empty — the bodies
    /// park on `None` rather than clicking a position they could not compute.
    fn client_rect(&self) -> Option<ScreenRect> {
        let windows = self.windows.as_deref()?;
        let Ok(Some(foreground)) = windows.foreground() else {
            return None;
        };
        if !aoko_platform::is_game_window(&foreground) {
            return None;
        }
        let Ok(bounds) = windows.client_bounds(foreground.handle) else {
            return None;
        };
        if !bounds.is_non_empty() {
            return None;
        }
        Some(ScreenRect {
            left: bounds.x,
            top: bounds.y,
            right: bounds.x.saturating_add(bounds.width),
            bottom: bounds.y.saturating_add(bounds.height),
        })
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

/// A clock and sleeper that move only when a body parks, or when a test says so.
///
/// Lives at module level rather than inside this file's test module because two suites drive it:
/// the runtime's own (a loop body's parks, playtime's second boundaries) and the loader's, where a
/// connect deadline is crossed by advancing the clock instead of sleeping through it. The bodies
/// never read the wall clock (`LoopRuntime` hands them this), so a park costs no real time.
#[cfg(test)]
pub(crate) struct StepClock {
    now: std::sync::Mutex<Instant>,
}

#[cfg(test)]
impl StepClock {
    /// A clock at the current instant.
    pub(crate) fn new() -> Self {
        Self {
            now: std::sync::Mutex::new(Instant::now()),
        }
    }

    /// Moves the clock forward.
    ///
    /// A loop body only moves this clock by parking on the sleeper, and the playtime timer never
    /// parks, so this is how a test crosses a second boundary — or a connect deadline — at no real
    /// cost.
    pub(crate) fn advance(&self, by: Duration) {
        let mut now = self.now.lock().expect("clock");
        *now += by;
    }
}

#[cfg(test)]
impl Clock for StepClock {
    fn now(&self) -> Instant {
        *self.now.lock().expect("clock")
    }
}

#[cfg(test)]
impl Sleeper for StepClock {
    fn sleep_until(&self, deadline: Instant) {
        let mut now = self.now.lock().expect("clock");
        if deadline.checked_duration_since(*now).is_some() {
            *now = deadline;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The search overlay's clock: `i64` milliseconds on the injected clock.
    ///
    /// `CardPulse` counts in milliseconds and the clock hands back an `Instant`, so the accessor
    /// is the conversion. A step clock proves it is the injected clock and not the wall: the
    /// accessor moves by exactly the steps the test injects.
    #[test]
    fn now_ms_counts_the_injected_clock() {
        let controller = Arc::new(AppController::default());
        let source = Arc::new(StepClock::new());
        let clock: Arc<dyn Clock> = source.clone();
        let sleeper: Arc<dyn Sleeper> = source.clone();

        let runtime = Runtime::with_time(controller, clock, sleeper);
        let start = runtime.now_ms();
        assert_eq!(
            start, 0,
            "the epoch is the construction, so it starts at zero"
        );

        source.advance(Duration::from_millis(250));
        assert_eq!(
            runtime.now_ms(),
            250,
            "and it moves with the clock, not the wall"
        );

        source.advance(Duration::from_secs(2));
        assert_eq!(runtime.now_ms(), 2_250);
    }

    /// A dial nobody answers becomes a failure at the deadline, on the injected clock.
    ///
    /// This is the state `Connecting` had no exit from: the worker blocks in `connect`
    /// (`aoko-platform/src/tcp.rs:90-92` dials without a timeout), the store never says connected,
    /// and nothing else would ever change the panel. The deadline is [`Runtime::now_ms`], so the
    /// test advances a step clock instead of sleeping three seconds — and the retry half is
    /// asserted too, because a failure a user cannot act on is the same defect wearing a line.
    #[test]
    fn an_unanswered_dial_fails_at_its_deadline_and_can_be_retried() {
        let (mut runtime, clock) = Runtime::stepped();
        assert_eq!(runtime.link_failure(), None, "nothing has been dialled yet");

        runtime.connect_bridge();
        assert_eq!(
            runtime.link_failure(),
            None,
            "a fresh attempt is a dial in flight, not a failure"
        );

        clock.advance(Duration::from_millis(CONNECT_DEADLINE_MS as u64 - 1));
        assert_eq!(
            runtime.link_failure(),
            None,
            "one millisecond short of the deadline the dial is still whatever it is"
        );

        clock.advance(Duration::from_millis(1));
        assert_eq!(
            runtime.link_failure(),
            Some(LinkFailure::ConnectFailed {
                port: aoko_protocol::BRIDGE_PORT
            }),
            "at the deadline an unanswered dial is a failure with the endpoint in it"
        );

        runtime.connect_bridge();
        assert_eq!(
            runtime.link_failure(),
            None,
            "the press after a failure dials again and restarts the deadline"
        );
    }

    /// A link the store reports as up is never a failure, however old the attempt is.
    ///
    /// The deadline is what to say about a dial *nobody answered*; a link that answered is not
    /// that, so the store's `connected` wins over the clock. Without this ordering an attempt that
    /// took longer than the deadline to come up would flip to "could not connect" the moment it
    /// succeeded.
    #[test]
    fn a_connected_link_is_never_a_failure_however_old_the_attempt() {
        let (mut runtime, clock) = Runtime::stepped();
        runtime.connect_bridge();

        runtime.controller().store().set_connected(true);
        clock.advance(Duration::from_millis(CONNECT_DEADLINE_MS as u64 + 1));

        assert_eq!(
            runtime.link_failure(),
            None,
            "an attempt that answered is not a dial nobody answered"
        );
    }

    /// A link the reader is re-dialling is painted, and a press still replaces that reader.
    ///
    /// The report answers for the round in flight, so the panel says the reader is reconnecting
    /// rather than repeating the drop; the press after it dials again, because an attempt that is
    /// re-dialling has nothing left to say about the link the press wants back.
    #[test]
    fn a_redialling_link_reports_the_wait_and_stays_retryable() {
        let (mut runtime, _clock) = Runtime::stepped();
        runtime.connect_bridge();

        runtime.link_report.publish(LinkFailure::Reconnecting);

        assert_eq!(
            runtime.link_failure(),
            Some(LinkFailure::Reconnecting),
            "the reader's own answer is the one the panel paints"
        );

        runtime.connect_bridge();
        assert_eq!(
            runtime.link_failure(),
            None,
            "a press after a redial is a fresh attempt, not the reason it replaced"
        );
    }

    /// The row facts' `supported` source: the bridge's advertised list, not a list of ours.
    ///
    /// The cell starts on the codec's own fallback table, which is what a window opened before
    /// the handshake would read, and an empty advertised set supports nothing — there is no
    /// second list in this crate to fall back to.
    #[test]
    fn module_support_reads_the_advertised_cell() {
        let runtime = Runtime::headless();
        assert!(
            runtime.module_supported("autoclicker"),
            "the codec's fallback table advertises autoclicker"
        );
        assert!(
            runtime.module_supported("AutoClicker"),
            "the lookup is the codec's normalised one, so case does not matter"
        );

        *runtime.advertised.write().expect("cell") = BridgeCapabilities::empty();
        assert!(
            !runtime.module_supported("autoclicker"),
            "an empty advertised list supports nothing rather than everything"
        );
    }

    /// The action half of the same cell: a bridge that narrows `actions` narrows the tabs.
    ///
    /// The fallback table carries all four session actions (`actions::ALL`), which is what lets the
    /// Servers and Accounts tabs offer their commands before a handshake; an empty advertised set
    /// answers "no" for every one of them, which is the managed `SupportsAction` against a bridge
    /// that named none.
    #[test]
    fn action_support_reads_the_advertised_cell() {
        use aoko_protocol::capabilities::actions;

        let runtime = Runtime::headless();
        assert!(
            runtime.action_supported(actions::SERVER_JOIN),
            "the codec's fallback table advertises the join action"
        );
        assert!(
            runtime.action_supported(actions::ACCOUNT_SET_SESSION),
            "and the session write the Accounts tab sends"
        );

        *runtime.advertised.write().expect("cell") = BridgeCapabilities::empty();
        assert!(
            !runtime.action_supported(actions::SERVER_JOIN),
            "a bridge that advertises nothing supports no action"
        );
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
            7,
            "the bundle owns one thread per body: {names:?}"
        );

        runtime.set_module_enabled("autoclicker", false);

        assert!(
            runtime.loop_names().is_empty(),
            "the last loop-driving module off stops the bundle"
        );
        assert_eq!(runtime.loops_owned(), 0);
    }

    /// The two container modules drive bodies of their own, and the bundle outlives one of them.
    ///
    /// `cheststealer` and `refill` had no `loop_for` arm, so their cards started no thread and
    /// neither could press a button. The second half is the supervision rule the other five
    /// already followed: one driver off is not the bundle off — the remaining module's body is
    /// still allowed to act, and the body whose switch went off parks on it.
    #[test]
    fn the_container_modules_drive_their_own_bodies_and_one_driver_off_keeps_the_bundle() {
        let controller = Arc::new(AppController::default());
        let source = Arc::new(StepClock::new());
        let clock: Arc<dyn Clock> = source.clone();
        let sleeper: Arc<dyn Sleeper> = source.clone();

        let mut runtime = Runtime::with_time(controller, clock, sleeper);
        runtime.start_supervision();

        runtime.set_module_enabled("cheststealer", true);
        let names = runtime.loop_names();
        assert!(
            names.contains(&"chest-stealer"),
            "the chest stealer must be registered by name: {names:?}"
        );
        assert_eq!(runtime.loops_owned(), 7);

        runtime.set_module_enabled("refill", true);
        assert!(runtime.loop_names().contains(&"refill"));

        runtime.set_module_enabled("cheststealer", false);
        assert_eq!(
            runtime.loops_owned(),
            7,
            "refill is still a driver, so the bundle stays up"
        );

        runtime.set_module_enabled("refill", false);
        assert!(
            runtime.loop_names().is_empty(),
            "the last driver off stops every body"
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

    // ---- playtime ---------------------------------------------------------------------

    /// The session timer counts the seconds the clock moves, and only those.
    ///
    /// The clock is [`StepClock`], so nothing here waits: `advance` is what crosses a second.
    /// The game is reported foreground through the same `ControllerSense` the click bodies
    /// read, over a fake window inspector — no window is opened and nothing is injected.
    #[test]
    fn the_session_timer_counts_playtime_from_the_clock() {
        let controller = Arc::new(AppController::default());
        let source = Arc::new(StepClock::new());
        let clock: Arc<dyn Clock> = source.clone();
        let sleeper: Arc<dyn Sleeper> = source.clone();
        let mut runtime = Runtime::with_time(Arc::clone(&controller), clock, sleeper);

        let game: Arc<dyn WindowInspector> = Arc::new(FakeWindows {
            foreground: Some(window("Minecraft 1.8.9", "java")),
            cursor: None,
        });
        let sense: Arc<dyn LoopSense> =
            Arc::new(ControllerSense::new(Arc::clone(&controller), Some(game)));

        // Before the session starts nothing counts, however far the clock moves.
        source.advance(Duration::from_secs(3));
        assert_eq!(runtime.sample_playtime(), 0, "no session, no tick");
        assert_eq!(runtime.stats().total_playtime(), Duration::ZERO);

        runtime.start_session_with(Arc::clone(&sense));

        source.advance(Duration::from_secs(3));
        assert_eq!(runtime.sample_playtime(), 3, "three whole seconds");
        assert_eq!(
            runtime.stats().total_playtime(),
            Duration::from_secs(3),
            "the tracker adds one second per sample"
        );

        // Half a second is not a sample, and it is not lost either: the next half completes
        // the second, which is what keeps the count from drifting behind the clock.
        source.advance(Duration::from_millis(500));
        assert_eq!(runtime.sample_playtime(), 0, "half a second is no sample");
        source.advance(Duration::from_millis(500));
        assert_eq!(runtime.sample_playtime(), 1, "the remainder is carried");
        assert_eq!(runtime.stats().total_playtime(), Duration::from_secs(4));

        // A closed window stops the counting and keeps the total, as `OnClosed` does.
        runtime.stop_session();
        source.advance(Duration::from_secs(10));
        assert_eq!(
            runtime.sample_playtime(),
            0,
            "a closed session counts nothing"
        );
        assert_eq!(
            runtime.stats().total_playtime(),
            Duration::from_secs(4),
            "StopSessionTimer disposes the timer, not the total"
        );
    }

    /// The two ways playtime must not accrue: no session at all, and an open menu.
    ///
    /// A headless runtime never starts a session, so it samples nothing. The second half is
    /// the tracker's own gate: connected, game foreground, cursor drawn and `GuiOpen` set is a
    /// blocking GUI, and `StatsTracker.record_playtime_sample` returns before adding the second.
    #[test]
    fn playtime_counts_nothing_without_a_session_or_behind_an_open_menu() {
        use aoko_protocol::state::GameState;

        let mut headless = Runtime::headless();
        assert_eq!(headless.sample_playtime(), 0);
        assert_eq!(headless.stats().total_playtime(), Duration::ZERO);

        let controller = Arc::new(AppController::default());
        let source = Arc::new(StepClock::new());
        let clock: Arc<dyn Clock> = source.clone();
        let sleeper: Arc<dyn Sleeper> = source.clone();
        let mut runtime = Runtime::with_time(Arc::clone(&controller), clock, sleeper);

        let game: Arc<dyn WindowInspector> = Arc::new(FakeWindows {
            foreground: Some(window("Minecraft 1.8.9", "java")),
            cursor: Some(true),
        });
        runtime.start_session_with(Arc::new(ControllerSense::new(
            Arc::clone(&controller),
            Some(game),
        )));

        controller.store().set_connected(true);
        controller.store().publish(
            GameState {
                gui_open: true,
                ..GameState::default()
            },
            source.now(),
        );

        source.advance(Duration::from_secs(2));
        assert_eq!(runtime.sample_playtime(), 2, "the seconds were offered");
        assert_eq!(
            runtime.stats().total_playtime(),
            Duration::ZERO,
            "the tracker refuses them: connected, menu open, cursor drawn"
        );
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
            sense.cursor_visible(),
            "an unanswered cursor means assume drawn, not hidden"
        );

        // A session with no window surface: nothing can confirm the game, and the cursor
        // answers the conservative value.
        let sense = ControllerSense::new(Arc::clone(&controller), None);

        assert!(!sense.minecraft_active());
        assert!(
            sense.cursor_visible(),
            "no surface at all is still \"cannot tell\", so still drawn"
        );

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

    /// A window surface whose client area is a real rectangle.
    ///
    /// `FakeWindows` answers an empty `Rect`, which is enough for every test that only asks
    /// whether the rect exists; the chest-stealer and refill bodies *map through* it, so their
    /// input needs a non-empty one. A separate fixture keeps the shared one untouched.
    struct ClientArea {
        game: aoko_platform::WindowInfo,
        client: aoko_platform::Rect,
    }

    impl WindowInspector for ClientArea {
        fn enumerate(&self) -> Result<Vec<aoko_platform::WindowInfo>, PlatformError> {
            Ok(vec![self.game.clone()])
        }

        fn foreground(&self) -> Result<Option<aoko_platform::WindowInfo>, PlatformError> {
            Ok(Some(self.game.clone()))
        }

        fn client_bounds(
            &self,
            _handle: aoko_platform::WindowHandle,
        ) -> Result<aoko_platform::Rect, PlatformError> {
            Ok(self.client)
        }

        fn cursor_visible(&self) -> Option<bool> {
            None
        }
    }

    /// The two window numbers the container bodies need, from the two different rects.
    ///
    /// `framebuffer_size` is `WindowDetection.GetMinecraftWindowRect()` — the *outer* window,
    /// used as the aim/triggerbot viewport fallback. `client_rect` is
    /// `GetMinecraftClientRectOnScreen()` — the *client* area, which the slot mapper scales
    /// into. Answering the outer rect to the mapper would offset every click by the title bar,
    /// so the test hands the fixture two different rects and asserts both answers.
    #[test]
    fn the_client_rect_is_the_focused_game_windows_client_area_not_its_outer_rect() {
        let controller = Arc::new(AppController::default());

        let mut game = window("Minecraft 1.8.9", "java");
        game.bounds = aoko_platform::Rect {
            x: 100,
            y: 100,
            width: 1280,
            height: 800,
        };
        let client = aoko_platform::Rect {
            x: 108,
            y: 130,
            width: 1264,
            height: 735,
        };

        let sense = ControllerSense::new(
            Arc::clone(&controller),
            Some(Arc::new(ClientArea {
                game: game.clone(),
                client,
            })),
        );

        assert_eq!(
            sense.framebuffer_size(),
            Some((1280, 800)),
            "the viewport fallback is the outer window rect"
        );
        let rect = sense
            .client_rect()
            .expect("a focused game has a client area");
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (108, 130, 108 + 1264, 130 + 735),
            "the slot mapper scales into the client area, so a click does not drift by the \
             title bar's height"
        );

        // Nothing focused, or no window surface at all: no rect, and the bodies park.
        let unfocused = ControllerSense::new(
            Arc::clone(&controller),
            Some(Arc::new(ClientArea {
                game,
                client: aoko_platform::Rect::default(),
            })),
        );
        assert!(unfocused.client_rect().is_none(), "an empty client area");
        assert!(
            ControllerSense::new(Arc::clone(&controller), None)
                .client_rect()
                .is_none(),
            "no window surface, no mapping"
        );

        // A focused IDE is not the game, so it has no client area to map into.
        let mut editor = window("fabric-mod-template - IntelliJ IDEA", "java");
        editor.bounds = client;
        let sense = ControllerSense::new(
            controller,
            Some(Arc::new(ClientArea {
                game: editor,
                client,
            })),
        );
        assert!(sense.client_rect().is_none());
    }

    /// Every body sees the same switch the UI writes, and sees it change.
    ///
    /// This is the gate that the shipped build was missing: the bundle started on any driver and
    /// every body ran, so the left clicker kept clicking after its own card went off, and
    /// switching aim assist on started a click stream. A captured value cannot pass this test.
    #[test]
    fn the_loop_switch_is_read_live_from_the_controller() {
        let controller = Arc::new(AppController::default());
        let sense = ControllerSense::new(Arc::clone(&controller), None);

        assert_eq!(
            sense.module_enabled("autoclicker"),
            Some(false),
            "off by default"
        );

        controller.set_module_enabled(EnabledFlag::LeftClick, true);
        assert_eq!(
            sense.module_enabled("autoclicker"),
            Some(true),
            "the same call each body makes, on the same tick"
        );

        controller.set_module_enabled(EnabledFlag::LeftClick, false);
        assert_eq!(sense.module_enabled("autoclicker"), Some(false));

        assert_eq!(
            sense.module_enabled("nonsense"),
            Some(false),
            "an id the catalog does not carry is off, not ungoverned"
        );
        assert_eq!(
            sense.module_enabled("panic"),
            Some(false),
            "panic has no stored boolean, so it can gate nothing on"
        );

        // The two container modules the bundle used to have no arms for.
        assert_eq!(sense.module_enabled("cheststealer"), Some(false));
        controller.set_module_enabled(EnabledFlag::ChestStealer, true);
        assert_eq!(sense.module_enabled("cheststealer"), Some(true));
        assert_eq!(sense.module_enabled("refill"), Some(false));
    }

    /// The settings a body reads are the profile's, and they are re-read every tick.
    ///
    /// `Clicker`'s properties were read inside each managed `while`, so an edit reached the
    /// engine on the next click. A settings set captured when the bundle started is exactly the
    /// defect this pins: the second read must differ from the first after one `update_profile`.
    #[test]
    fn the_loop_settings_follow_the_live_profile() {
        let controller = Arc::new(AppController::default());
        controller.set_module_enabled(EnabledFlag::LeftClick, true);
        let sense = ControllerSense::new(Arc::clone(&controller), None);

        let first = sense
            .loop_configs()
            .expect("the controller holds a profile");
        assert_eq!(first.left.min_cps, 8.0);
        assert_eq!(first.left.max_cps, 12.0);
        assert!(!first.left.guards.click_in_chests);
        assert!(
            first.chest_stealer.menu_check,
            "the profile default has the menu checks on"
        );
        assert!(
            first.aim.auto_left_clicking,
            "`IsUsingLeftButton` is the left engine's switch"
        );

        let changed = controller.update_profile(|profile| {
            profile.click_in_chests = true;
            profile.min_cps = 14.0;
            profile.max_cps = 14.0;
            profile.aim_assist_fov = 45.0;
            profile.chest_stealer_delay_ms = 200;
            profile.refill_delay_ms = 5_000;
            true
        });
        assert!(changed);

        let second = sense.loop_configs().expect("still a live profile");
        assert!(
            second.left.guards.click_in_chests,
            "a settings edit reaches the engine without restarting the bundle"
        );
        assert_eq!(second.left.min_cps, 14.0);
        assert_eq!(second.left.max_cps, 14.0);
        assert_eq!(second.aim.params.aim_assist_fov, 45.0);
        assert_eq!(second.chest_stealer.delay_ms, 200);
        assert_eq!(
            second.refill.delay_ms, 500,
            "an out-of-range delay is clamped the way the managed setter clamped it"
        );

        // The earlier read is a value, not a view: nothing about it moved.
        assert!(!first.left.guards.click_in_chests);
        assert_eq!(first.left.min_cps, 8.0);
    }

    /// `SupportsModule` answers from the bridge's advertised lists, not from a constant.
    ///
    /// The three assist loops gate on it (`Clicker.cs:2665`, `:2727-2729`, `:2853`), which is
    /// why a hardcoded `false` made aim assist, Pixel Party assist and the triggerbot do
    /// nothing; the cell is the same one [`Runtime::module_supported`] reads.
    #[test]
    fn module_support_reads_the_advertised_capabilities_cell() {
        let controller = Arc::new(AppController::default());

        let bare = ControllerSense::new(Arc::clone(&controller), None);
        assert!(
            !bare.supports_module("aimassist"),
            "no cell, nothing advertised, nothing supported"
        );
        assert!(!bare.supports_state_field("pixelpartyyawdelta"));

        let cell = Arc::new(RwLock::new(BridgeCapabilities::for_version_fallback(None)));
        let sense =
            ControllerSense::new(Arc::clone(&controller), None).with_advertised(Arc::clone(&cell));

        assert!(
            sense.supports_module("aimassist"),
            "the codec's fallback table is what a pre-handshake question gets"
        );
        assert!(
            sense.supports_module("AIMASSIST"),
            "the lookup normalises case, like `BridgeCapabilities`"
        );
        assert!(
            sense.supports_state_field("pixelpartyyawdelta"),
            "Pixel Party assist gates on its state field too"
        );

        *cell.write().expect("cell") = BridgeCapabilities::empty();
        assert!(
            !sense.supports_module("aimassist"),
            "a bridge that advertises nothing supports nothing rather than everything"
        );
    }

    // ---- GTB helper -----------------------------------------------------------------

    /// A GTB action bar for a seven-letter theme with only its first letter revealed.
    ///
    /// `Airport` is the first seven-letter `A` theme in the shipped wordlist, and it is what
    /// the solved answer must contain: the mask `a______` is the game's own shape, not one the
    /// test invented, and `(1)` is the round timer the pipeline strips.
    const GTB_LINE: &str = "The theme is A______ (1)";

    /// One frame carrying that line, as the bridge would publish it.
    fn gtb_state() -> aoko_protocol::state::GameState {
        aoko_protocol::state::GameState {
            action_bar: GTB_LINE.to_string(),
            ..aoko_protocol::state::GameState::default()
        }
    }

    /// The helper solves the line once and exposes the word, and a repeated publish of the same
    /// line does no work.
    ///
    /// The evidence is `gtb_solves`, an integer: the bridge re-sends the same action bar every
    /// frame, and this number is what "the work happens once" means without a clock.
    #[test]
    fn the_gtb_helper_solves_a_new_line_once() {
        let mut runtime = Runtime::headless();
        runtime.set_module_enabled("gtbhelper", true);

        let now = Instant::now();
        runtime.controller().store().set_connected(true);
        runtime.controller().store().publish(gtb_state(), now);

        runtime.poll_gtb(now);

        let answer = runtime.gtb_answer().expect("a mask with candidates");
        assert_eq!(answer.mask, "a______", "the revealed letter survives");
        assert!(
            answer.matches.iter().any(|word| word == "airport"),
            "the shipped wordlist was searched: {:?}",
            answer.matches
        );
        assert_eq!(
            answer.line, GTB_LINE,
            "the answer names the line it came from"
        );
        assert_eq!(runtime.gtb_solves(), 1);

        // The same line, three more frames: no solve, same answer.
        for _ in 0..3 {
            runtime.poll_gtb(now);
        }

        assert_eq!(
            runtime.gtb_solves(),
            1,
            "an unchanged line must not run the solver again"
        );
        assert_eq!(
            runtime.gtb_answer().map(|answer| answer.mask),
            Some("a______".to_string())
        );

        // A different line is new work, so the counter is not simply stuck.
        runtime.controller().store().publish(
            aoko_protocol::state::GameState {
                action_bar: "The theme is A_____ (2)".to_string(),
                ..aoko_protocol::state::GameState::default()
            },
            now,
        );
        runtime.poll_gtb(now);

        assert_eq!(runtime.gtb_solves(), 2, "a new line is a new solve");
    }

    /// The four guards: a disabled module, a dead link, a stale frame and an open menu all
    /// report nothing and do no work.
    #[test]
    fn the_gtb_helper_does_no_work_when_the_round_is_not_live() {
        let mut runtime = Runtime::headless();
        let now = Instant::now();

        // Module off, line present: `Clicker.cs:2630` returns before the solver.
        runtime.controller().store().set_connected(true);
        runtime.controller().store().publish(gtb_state(), now);
        runtime.poll_gtb(now);

        assert!(
            runtime.gtb_answer().is_none(),
            "a disabled helper holds nothing"
        );
        assert_eq!(runtime.gtb_solves(), 0);

        // Module on, link down: the stored line is from a round nobody is playing.
        runtime.set_module_enabled("gtbhelper", true);
        runtime.controller().store().set_connected(false);
        runtime.poll_gtb(now);

        assert!(runtime.gtb_answer().is_none());
        assert_eq!(
            runtime.gtb_solves(),
            0,
            "an unconnected store solves nothing"
        );

        // Module on, link up, frame older than the fresh window: enabling the helper must not
        // retroactively solve the line that was already sitting there.
        runtime.controller().store().set_connected(true);
        let stale = now + Duration::from_millis(FRESH_STATE_MS + 1);
        runtime.poll_gtb(stale);

        assert!(
            runtime.gtb_answer().is_none(),
            "a stale line is the previous round"
        );
        assert_eq!(runtime.gtb_solves(), 0);

        // Module on, link up, fresh frame, menu open: the action bar is not the round.
        runtime.controller().store().publish(
            aoko_protocol::state::GameState {
                gui_open: true,
                ..gtb_state()
            },
            stale,
        );
        runtime.poll_gtb(stale);

        assert!(
            runtime.gtb_answer().is_none(),
            "a menu is not gameplay for any other consumer either"
        );
        assert_eq!(runtime.gtb_solves(), 0);
    }

    // ---- stats tracker --------------------------------------------------------------

    /// A click reaches the tracker through the sink the loop bodies use, the view's own text
    /// carries it, and a reset through the runtime clears it.
    ///
    /// The feed is `ClickSink::record_click`, which is what `click-left`/`click-right` call and
    /// what `Clicker.cs:3270` calls. The ingest path is asserted to *not* feed it, because no
    /// decoded message carries a click or a CPS value: a future attempt to wire one from there
    /// has to delete that assertion first.
    ///
    /// The view half is the model, not the pixels: GPUI at the pin exposes no readback of
    /// rasterised glyphs, which is why `stats_view`'s own suite pins text through
    /// `StatsTexts::of` and geometry through a mounted window. This asserts the same model the
    /// Stats tab draws from.
    #[test]
    fn a_click_reaches_the_stats_tracker_and_a_reset_clears_it() {
        use crate::views::stats_view::StatsTexts;

        let runtime = Runtime::headless();

        // A decoded state frame moves the store, not the tracker.
        let now = Instant::now();
        runtime.controller().store().set_connected(true);
        runtime.controller().store().publish(
            aoko_protocol::state::GameState {
                in_world: true,
                ..aoko_protocol::state::GameState::default()
            },
            now,
        );
        assert_eq!(
            runtime.stats().total_clicks(),
            0,
            "a state frame carries no click count for a tracker to read"
        );

        let sink = runtime.click_sink();
        sink.record_click(8.5, true);
        sink.record_click(9.5, true);
        sink.record_click(11.0, false);

        assert_eq!(runtime.stats().total_clicks(), 3);
        assert_eq!(runtime.stats().left_clicks(), 2);
        assert_eq!(runtime.stats().right_clicks(), 1);

        let texts = StatsTexts::of(&runtime.stats());
        assert_eq!(texts.clicks, "3");
        assert_eq!(texts.clicks_split, "L 2  R 1");
        assert_eq!(texts.samples, "3 samples");

        assert!(runtime.reset_stats(), "three clicks were there to clear");
        assert_eq!(runtime.stats().total_clicks(), 0);
        assert_eq!(runtime.stats().left_clicks(), 0);
        assert_eq!(runtime.stats().right_clicks(), 0);
        assert_eq!(
            StatsTexts::of(&runtime.stats()).clicks,
            "0",
            "the view reads the cleared tracker, not a cached number"
        );
        assert!(
            !runtime.reset_stats(),
            "a second reset has nothing to clear"
        );
    }

    /// An unanswered cursor must read as "drawn", and this pins the consumers' consequences.
    ///
    /// `None` is the normal Linux answer — `LinuxWindowInspector` overrides `cursor_visible` to
    /// return it and the capability notes say so — so the fallback is the common case there, not
    /// an emergency. Every consumer treats `false` as permission, which is why it cannot be the
    /// fallback: `clicker/tick.rs` skips the menu block for a GUI it thinks is ghosted, and
    /// `loops/triggerbot.rs` fires on `!cursor_visible`. The assertions below are the real
    /// decisions those bodies make, not the mapping.
    #[test]
    fn an_unanswered_cursor_parks_the_consumers() {
        use aoko_core::clicker::tick::{
            decide, ClickConfig, TickContext, TickDecision, MENU_BLOCK_POLL,
        };
        use aoko_protocol::state::GameState;

        let controller = Arc::new(AppController::default());
        let unanswered: Arc<dyn WindowInspector> = Arc::new(FakeWindows {
            foreground: Some(window("Minecraft 1.8.9", "java")),
            cursor: None,
        });
        let sense = ControllerSense::new(controller, Some(unanswered));

        assert!(sense.cursor_visible(), "cannot tell means assume drawn");

        // The clicker with a chest GUI open: a drawn cursor blocks the click, and the old `false`
        // would have read the GUI as ghosted and gone through it.
        let state = GameState {
            is_connected: true,
            gui_open: true,
            screen_name: "GuiChest".to_string(),
            ..GameState::default()
        };
        let config = ClickConfig::default();

        let decision = decide(
            &state,
            &config,
            TickContext {
                minecraft_active: sense.minecraft_active(),
                cursor_visible: sense.cursor_visible(),
            },
            true,
        );
        assert_eq!(
            decision,
            TickDecision::Wait(MENU_BLOCK_POLL),
            "a menu must block the click"
        );

        let inverted = decide(
            &state,
            &config,
            TickContext {
                minecraft_active: sense.minecraft_active(),
                cursor_visible: false,
            },
            true,
        );
        assert_ne!(
            inverted,
            TickDecision::Wait(MENU_BLOCK_POLL),
            "the old fallback skipped the menu block, which is why it was wrong"
        );

        // The triggerbot's own guard is `minecraft_active() && !cursor_visible()`.
        assert!(
            !(sense.minecraft_active() && !sense.cursor_visible()),
            "the triggerbot must park on a cursor nobody can see"
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

    // ---- module keybinds: the hook queue, the guard, and one dispatch per press ----------

    /// An input backend whose event queue is the test's to fill.
    ///
    /// The one thing a module keybind needs from the platform: `poll_events`. Everything else is
    /// accepted and unobservable, because a keybind must not synthesise anything.
    struct ScriptedInput {
        queue: Mutex<Vec<InputEvent>>,
    }

    impl ScriptedInput {
        fn push(&self, event: InputEvent) {
            self.queue.lock().expect("queue").push(event);
        }

        /// One physical press: down then up, the way the hook reports it.
        fn press(&self, key: Key) {
            self.push(InputEvent::KeyDown(key));
            self.push(InputEvent::KeyUp(key));
        }

        /// A held key's auto-repeat: another down with no up between them.
        fn repeat(&self, key: Key) {
            self.push(InputEvent::KeyDown(key));
        }

        fn release(&self, key: Key) {
            self.push(InputEvent::KeyUp(key));
        }

        /// One physical mouse press: down then up, the way the managed mouse hook reported it.
        fn press_button(&self, button: MouseButton) {
            self.push(InputEvent::ButtonDown(button));
            self.push(InputEvent::ButtonUp(button));
        }

        /// A held button's auto-repeat: another down with no up between them.
        fn repeat_button(&self, button: MouseButton) {
            self.push(InputEvent::ButtonDown(button));
        }

        fn release_button(&self, button: MouseButton) {
            self.push(InputEvent::ButtonUp(button));
        }
    }

    impl InputBackend for ScriptedInput {
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

        fn poll_events(&self, out: &mut Vec<InputEvent>) -> Result<(), PlatformError> {
            let mut queue = self.queue.lock().expect("queue");
            out.append(&mut queue);
            Ok(())
        }
    }

    /// A runtime over a scripted hook queue, the 1.8.9 module list, and a foreground the caller
    /// names — the four facts [`Runtime::poll_module_keys`] reads.
    fn keybind_runtime(
        foreground: Option<aoko_platform::WindowInfo>,
    ) -> (Runtime, Arc<ScriptedInput>, Arc<AppController>) {
        let controller = Arc::new(AppController::default());
        let mut runtime = Runtime::new(Arc::clone(&controller));
        // What the handshake publishes: without it `module_supported` answers `false` and the
        // managed `ToggleModule` guard would refuse every bind.
        *runtime.advertised.write().expect("cell") =
            BridgeCapabilities::for_version_fallback(Some("1.8.9"));

        let scripted = Arc::new(ScriptedInput {
            queue: Mutex::new(Vec::new()),
        });
        let input: Arc<dyn InputBackend> = scripted.clone();
        let windows: Arc<dyn WindowInspector> = Arc::new(FakeWindows {
            foreground,
            cursor: None,
        });
        runtime.platform = Some(PlatformSurfaces {
            input,
            windows: Some(windows),
        });
        (runtime, scripted, controller)
    }

    /// Writes one module bind the way `Shell::apply_captured_key` does.
    fn bind(controller: &AppController, module: &str, vk: i32) {
        let mut profile = controller.profile();
        profile.module_keys.insert(module.to_owned(), vk);
        controller.replace_profile(profile);
    }

    /// A bind is one toggle per physical press, and the queue is drained as it is read.
    #[test]
    fn a_bound_key_flips_its_module_once_per_press() {
        let (mut runtime, input, controller) =
            keybind_runtime(Some(window("Minecraft 1.8.9", "java")));
        bind(&controller, "autoclicker", 0x41);

        input.press(Key::Char('a'));
        assert_eq!(runtime.poll_module_keys(), 1);
        assert!(runtime.module_enabled("autoclicker"));

        // The events were consumed: nothing is left to dispatch a second time.
        assert_eq!(runtime.poll_module_keys(), 0);
        assert!(runtime.module_enabled("autoclicker"));

        // A second physical press is a second toggle, not a repeat.
        input.press(Key::Char('a'));
        assert_eq!(runtime.poll_module_keys(), 1);
        assert!(!runtime.module_enabled("autoclicker"));
    }

    /// Both spellings of the same key reach one bind: evdev says lower case, a synthesised press
    /// upper case, and the saved bind is the virtual-key code neither of them is.
    #[test]
    fn every_spelling_of_a_key_reaches_the_same_bind() {
        let (mut runtime, input, controller) =
            keybind_runtime(Some(window("Minecraft 1.8.9", "java")));
        bind(&controller, "autoclicker", 0x41);

        input.press(Key::Char('A'));
        assert_eq!(runtime.poll_module_keys(), 1, "the capital spelling");
        assert!(runtime.module_enabled("autoclicker"));

        input.press(Key::Char('a'));
        assert_eq!(runtime.poll_module_keys(), 1, "and the lower-case one");
        assert!(!runtime.module_enabled("autoclicker"));
    }

    /// The Windows hook reports every key as `Key::Other(vkCode)`, which is the spelling the
    /// comparison has to answer or every bind on Windows is dead.
    #[cfg(windows)]
    #[test]
    fn the_windows_hook_spelling_reaches_the_bind() {
        let (mut runtime, input, controller) =
            keybind_runtime(Some(window("Minecraft 1.8.9", "java")));
        bind(&controller, "autoclicker", 0x41);

        input.press(Key::Other(0x41));
        assert_eq!(runtime.poll_module_keys(), 1);
        assert!(runtime.module_enabled("autoclicker"));
    }

    /// Holding a bound key is one press: `WM_KEYDOWN` repeats and evdev sends `value == 2`, and a
    /// toggle that believed either would flicker the module dozens of times a second.
    #[test]
    fn the_auto_repeat_of_a_held_key_is_not_another_toggle() {
        let (mut runtime, input, controller) =
            keybind_runtime(Some(window("Minecraft 1.8.9", "java")));
        bind(&controller, "autoclicker", 0x41);

        input.repeat(Key::Char('a'));
        assert_eq!(runtime.poll_module_keys(), 1, "the press itself");

        for _ in 0..8 {
            input.repeat(Key::Char('a'));
        }
        assert_eq!(runtime.poll_module_keys(), 0, "eight repeats, no toggle");
        assert!(runtime.module_enabled("autoclicker"));

        // Its release re-arms the key, so the next press is a press again.
        input.release(Key::Char('a'));
        assert_eq!(runtime.poll_module_keys(), 0);
        input.repeat(Key::Char('a'));
        assert_eq!(runtime.poll_module_keys(), 1);
        assert!(!runtime.module_enabled("autoclicker"));
    }

    /// A mouse bind is a bind: the capture stores `VK_LBUTTON`..`VK_XBUTTON2` for the five
    /// buttons, so a button press has to take the same latch and the same lookup a keystroke
    /// does. The queue used to drop both button events, which left every mouse bind storable in
    /// the profile and dead at the press.
    #[test]
    fn a_mouse_bind_flips_its_module_once_per_press() {
        let (mut runtime, input, controller) =
            keybind_runtime(Some(window("Minecraft 1.8.9", "java")));
        bind(&controller, "autoclicker", 0x01);

        input.press_button(MouseButton::Left);
        assert_eq!(
            runtime.poll_module_keys(),
            1,
            "the press on the bound button"
        );
        assert!(runtime.module_enabled("autoclicker"));

        // A held button is one dispatch, exactly as a held key is.
        input.repeat_button(MouseButton::Left);
        assert_eq!(runtime.poll_module_keys(), 1, "the press itself");
        assert!(!runtime.module_enabled("autoclicker"));
        input.repeat_button(MouseButton::Left);
        assert_eq!(
            runtime.poll_module_keys(),
            0,
            "the auto-repeat of a held button"
        );
        input.release_button(MouseButton::Left);
        assert_eq!(runtime.poll_module_keys(), 0);

        // A different button is a different code, so it matches no bind.
        input.press_button(MouseButton::Middle);
        assert_eq!(
            runtime.poll_module_keys(),
            0,
            "the middle button is not the bind"
        );
        assert!(!runtime.module_enabled("autoclicker"));
    }

    /// The five buttons are the codes the capture stores, so the bind a user records is the bind
    /// this dispatches: `VK_LBUTTON`..`VK_XBUTTON2`, with `0x03` skipped.
    #[test]
    fn the_mouse_codes_are_the_ones_the_capture_stores() {
        assert_eq!(mouse_virtual_key(MouseButton::Left), 0x01);
        assert_eq!(mouse_virtual_key(MouseButton::Right), 0x02);
        assert_eq!(mouse_virtual_key(MouseButton::Middle), 0x04);
        assert_eq!(mouse_virtual_key(MouseButton::X1), 0x05);
        assert_eq!(mouse_virtual_key(MouseButton::X2), 0x06);
    }

    /// A key no module claims is a scan and nothing else: no toggle, and nothing repainted.
    #[test]
    fn an_unbound_key_toggles_nothing() {
        let (mut runtime, input, controller) =
            keybind_runtime(Some(window("Minecraft 1.8.9", "java")));
        bind(&controller, "autoclicker", 0x41);

        input.press(Key::Char('z'));
        assert_eq!(runtime.poll_module_keys(), 0);
        assert!(!runtime.module_enabled("autoclicker"));
    }

    /// The managed guard: a bind does nothing unless the game is the foreground window, which is
    /// what keeps a bound letter typed into this window's own search box from toggling a module.
    #[test]
    fn a_bind_does_nothing_while_this_window_is_in_front() {
        for foreground in [
            Some(window("aoko", "aoko-client.exe")),
            Some(window("Minecraft 1.8.9", "explorer.exe")),
            None,
        ] {
            let (mut runtime, input, controller) = keybind_runtime(foreground);
            bind(&controller, "autoclicker", 0x41);

            input.press(Key::Char('a'));
            assert_eq!(runtime.poll_module_keys(), 0);
            assert!(!runtime.module_enabled("autoclicker"));
        }
    }

    /// Without a window surface the guard cannot confirm the game, so it fails closed — the same
    /// answer the injection gate gives a session that cannot see windows.
    #[test]
    fn a_session_without_window_sense_dispatches_no_bind() {
        let (mut runtime, input, controller) =
            keybind_runtime(Some(window("Minecraft 1.8.9", "java")));
        bind(&controller, "autoclicker", 0x41);
        if let Some(platform) = runtime.platform.as_mut() {
            platform.windows = None;
        }

        input.press(Key::Char('a'));
        assert_eq!(runtime.poll_module_keys(), 0);
        assert!(!runtime.module_enabled("autoclicker"));
    }

    /// An open game screen is not gameplay, and the managed host blocked its module keybinds
    /// there: a bind pressed in a chest or an inventory does nothing.
    #[test]
    fn a_bind_behind_an_open_game_screen_does_nothing() {
        let (mut runtime, input, controller) =
            keybind_runtime(Some(window("Minecraft 1.8.9", "java")));
        bind(&controller, "autoclicker", 0x41);

        let store = runtime.controller().store();
        store.set_connected(true);
        store.publish(
            aoko_protocol::state::GameState {
                gui_open: true,
                screen_name: String::from("GuiChest"),
                ..aoko_protocol::state::GameState::default()
            },
            Instant::now(),
        );

        input.press(Key::Char('a'));
        assert_eq!(runtime.poll_module_keys(), 0, "a chest is not gameplay");
        assert!(!runtime.module_enabled("autoclicker"));

        // The screen closes; the same press now lands.
        runtime.controller().store().publish(
            aoko_protocol::state::GameState {
                gui_open: false,
                screen_name: String::from("none"),
                ..aoko_protocol::state::GameState::default()
            },
            Instant::now(),
        );
        input.press(Key::Char('a'));
        assert_eq!(runtime.poll_module_keys(), 1);
        assert!(runtime.module_enabled("autoclicker"));
    }

    /// The two `InputHooks.ToggleModule` refusals: a module the bridge does not advertise is
    /// ignored, and a dev-only module needs dev mode.
    #[test]
    fn a_bind_cannot_toggle_a_module_the_host_would_refuse() {
        let game = || Some(window("Minecraft 1.8.9", "java"));

        let (mut runtime, input, controller) = keybind_runtime(game());
        bind(&controller, "autoclicker", 0x41);
        *runtime.advertised.write().expect("cell") = BridgeCapabilities::empty();
        input.press(Key::Char('a'));
        assert_eq!(runtime.poll_module_keys(), 0, "unadvertised module");
        assert!(!runtime.module_enabled("autoclicker"));

        let (mut runtime, input, controller) = keybind_runtime(game());
        assert!(
            module_catalog::is_dev_only("killaura"),
            "the fixture is dev-only"
        );
        bind(&controller, "killaura", 0x42);
        input.press(Key::Char('b'));
        assert_eq!(runtime.poll_module_keys(), 0, "dev mode is off");
        assert!(!runtime.module_enabled("killaura"));

        let mut profile = controller.profile();
        profile.dev_mode = true;
        controller.replace_profile(profile);
        input.press(Key::Char('b'));
        assert_eq!(runtime.poll_module_keys(), 1, "dev mode is on");
        assert!(runtime.module_enabled("killaura"));
    }

    /// The confirmation needs both halves of the managed rule: a game title and a Java launcher.
    ///
    /// The pair is what makes it safe. `aoko_platform::is_game_window` is a title test, so it
    /// answers `true` for an editor literally titled `Minecraft`, and the JVM name test is what
    /// refuses that window; the name test alone answers `true` for any Java process, and the title
    /// rule is what refuses that one.
    ///
    /// The exception is the third case below: a window whose owning executable cannot be resolved
    /// is not evidence against the title, and treating it as a refusal is what stopped every module
    /// from acting in a session where the backend could not name the client's process.
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
        assert!(confirmed("Badlion Client 1.8.9", "javaw"));

        // An unreadable owner: the title is the only fact there is, and it matched.
        let unresolved = FakeWindows {
            foreground: Some(aoko_platform::WindowInfo {
                process_name: None,
                ..window("Minecraft 1.8.9", "")
            }),
            cursor: None,
        };
        assert!(
            game_is_foreground(&unresolved),
            "an unreadable process name must not turn a titled game window into a refusal"
        );

        assert!(
            !confirmed("Minecraft 1.8.9", "explorer.exe"),
            "a game title in a non-Java process is not the game"
        );
        assert!(
            !confirmed("Minecraft", "code"),
            "an editor titled Minecraft is exactly why the pair needs the JVM name test"
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
    /// them must be the seven bodies `ClickerLoops` starts, and the names must be the strings
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
                "cheststealer",
                "pixelpartyassist",
                "refill",
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
                "chest-stealer",
                "click-left",
                "click-right",
                "pixel-party",
                "refill",
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

#[cfg(test)]
mod link_capability_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A capabilities frame that arrives over an attached transport has to land in the cell
    /// `Runtime::setting_supported` reads, or the Settings card's reload-mappings press can
    /// never see the bridge agree to it.
    #[test]
    fn an_attached_capabilities_frame_reaches_the_setting_cell() {
        let mut runtime = Runtime::headless();
        let (transport, peer) = aoko_protocol::transport::memory::duplex_open();
        peer.feed(b"{\"type\":\"capabilities\",\"settings\":[\"reloadMappingsNonce\"]}\n");
        runtime.attach_ingest(transport);

        let deadline = Instant::now() + Duration::from_secs(5);
        while !runtime.setting_supported("reloadMappingsNonce") {
            assert!(Instant::now() < deadline, "the cell never filled");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
