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

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, RwLock};
use std::time::{Duration, Instant};

use aoko_core::app::{AppController, StateSnapshot};
use aoko_core::chest_stealer::FRESH_STATE_MS;
use aoko_core::loops::{ClickSink, ClickerLoopConfigs, ClickerLoops, LoopRuntime, LoopSense};
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

/// Seed the five loop bodies start from.
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
/// `aoko_core::loops::ClickSink::record_click` is the production seam, and the loops already
/// call it once per accepted click: the five bodies take an `Arc<dyn ClickSink>` when they
/// start, so handing them this one is the whole feed. That is also where the managed host
/// records a click — `Clicker.cs:3270`, `StatsTracker.Instance.RecordClick(cps, leftButton)`,
/// one line before `PerformClick`.
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
        self.advertised
            .read()
            .map(|advertised| advertised.supports_module(module_id))
            .unwrap_or(false)
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
        self.advertised
            .read()
            .map(|advertised| advertised.supports_setting(setting))
            .unwrap_or(false)
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
    /// retry and has to actually dial. In the deadline case the previous worker is still blocked
    /// in `connect`; it is detached rather than joined, it reports into the report cell of its own
    /// attempt, and the store's `connected` is a boolean either way — so the panel cannot be wrong
    /// about whether the link is up, which is the property that matters here.
    pub fn connect_bridge(&mut self) {
        if self.controller.store().is_connected() || self.attempt_is_live() {
            return;
        }

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
    /// from it, and a copy is exactly the stale second source this slice removes. The lock is
    /// held only while the caller reads.
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
        let sense: Arc<dyn LoopSense> =
            Arc::new(ControllerSense::new(Arc::clone(&self.controller), windows));
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

    /// Starts the five bodies on their own threads.
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
        let sense: Arc<dyn LoopSense> =
            Arc::new(ControllerSense::new(Arc::clone(&self.controller), windows));
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

/// Whether the game is the foreground window, confirmed strictly enough to gate injection.
///
/// The platform's rule composed with the launcher test, both required:
///
/// * `aoko_platform::is_game_window(&foreground)` — the managed `GameTitles` rule (`Minecraft`,
///   `Lunar Client`, `Badlion`, ordinal substring), now the one copy in the workspace:
///   `select_best_window` uses the same function;
/// * `foreground.is_jvm()` — the owning process is `java`, `javaw`, `java.exe` or `javaw.exe`,
///   `WindowDetection.WindowTarget.IsJvm`.
///
/// Neither half is safe alone, which is why the pair is the gate rather than either one. The title
/// rule answers `true` for an editor literally titled `Minecraft` — `window.rs` says so in the
/// builder's own note — and `is_jvm` is `false` there, so the pair refuses. The launcher test alone
/// answers `true` for any bare Java process, and the title rule refuses that.
///
/// The pair also replaces the score this module used to run:
/// `describe_target(..).is_likely_minecraft()` counts `fabric`, `forge`, `quilt` and `feather`, so
/// a focused IDE on a mod workspace passed it. `fabric-mod-template - IntelliJ IDEA` is a Java
/// process, so it passes the launcher half and fails the title half.
///
/// One residual is worth stating rather than hiding: this is still a title test, so a *Java* IDE
/// whose window title contains a game title — a project called `minecraft-plugin`, say — satisfies
/// both halves. Closing it needs the managed host's cached-handle test (the foreground equals a
/// known game window, i.e. `select_best_window` over a full enumeration), which no public platform
/// function offers today; an IDE blocklist here would be the drift `is_game_window` exists to
/// avoid, so it is reported instead.
///
/// A missing window surface, a foreground the backend cannot read, and a window with no process
/// name all answer `false`: this is the guard, so it fails closed.
#[must_use]
pub fn game_is_foreground(windows: &dyn WindowInspector) -> bool {
    let Ok(Some(foreground)) = windows.foreground() else {
        return false;
    };
    aoko_platform::is_game_window(&foreground) && foreground.is_jvm()
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
/// conservative sense.
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
/// exactly when that function refused the surface. Without it the game can never be confirmed
/// foreground, so `minecraft_active` answers `false`, and the cursor answers the conservative
/// "drawn" that parks every consumer rather than the permissive "hidden".
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

    // ---- playtime ---------------------------------------------------------------------

    /// The session timer counts the seconds the clock moves, and only those.
    ///
    /// The clock is [`StepClock`], so nothing here waits: `advance` is what crosses a second.
    /// The game is reported foreground through the same [`ControllerSense`] the click bodies
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

    /// The confirmation needs both halves of the managed rule: a game title and a Java launcher.
    ///
    /// The pair is what makes it safe. `aoko_platform::is_game_window` is a title test, so it
    /// answers `true` for an editor literally titled `Minecraft`, and `is_jvm` is what refuses
    /// that window; `is_jvm` alone answers `true` for any Java process, and the title rule is
    /// what refuses that one.
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

        assert!(
            !confirmed("Minecraft 1.8.9", "explorer.exe"),
            "a game title in a non-Java process is not the game"
        );
        assert!(
            !confirmed("Minecraft", "code"),
            "an editor titled Minecraft is exactly why the pair needs is_jvm"
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
