//! Thread-lifecycle tests for the loop executors.
//!
//! Every test here is deterministic: the loop thread coordinates with the test
//! through a condvar ([`GateSleeper`]), through its own token
//! ([`FakeInput::cancel_after_clicks`], [`CancelAfterSleeper`]) or through a
//! channel. No test asserts an elapsed duration, and no test sleeps.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aoko_platform::{InputBackend, Key, MouseButton};
use aoko_protocol::state::{EntityInfo, GameState};

use super::{ClickSink, LoopHandle, LoopRuntime, LoopSense};
use crate::clicker::{AdvancedClickTimingState, ClickRandomizationMode};
use crate::clock::{Clock, SplitMix64, SystemClock};
use crate::concurrency::CancelToken;
use crate::testing::{CancelAfterSleeper, FakeInput, GateSleeper, InputCall, VirtualClock};

/// A [`LoopSense`] over fixed values.
struct StaticSense {
    snapshot: Option<Arc<crate::app::StateSnapshot>>,
    connected: bool,
    injected: bool,
    active: bool,
    cursor_visible: bool,
    modules: Vec<&'static str>,
    fields: Vec<&'static str>,
    framebuffer: Option<(i32, i32)>,
    /// Module switches the bodies see through [`LoopSense::module_enabled`].
    ///
    /// `None` is "this fixture models no switches", which is the trait default and keeps a
    /// direct `run` test ungoverned. `Some(list)` answers from the list, so a body's own gate
    /// can be driven without adding a flag to every config.
    switches: Option<Vec<&'static str>>,
    /// Live settings handed over through [`LoopSense::loop_configs`]; `None` keeps the settings
    /// the body was started with.
    configs: Option<super::ClickerLoopConfigs>,
}

impl StaticSense {
    fn new(state: Option<GameState>) -> Self {
        Self {
            snapshot: state.map(|state| {
                Arc::new(crate::app::StateSnapshot {
                    state: Arc::new(state),
                    at: Instant::now(),
                })
            }),
            connected: true,
            injected: true,
            active: true,
            cursor_visible: false,
            modules: vec!["aimassist", "triggerbot", "pixelpartyassist"],
            fields: vec!["pixelpartyyawdelta"],
            framebuffer: Some((1600, 900)),
            switches: None,
            configs: None,
        }
    }

    fn inactive(mut self) -> Self {
        self.active = false;
        self
    }

    fn with_cursor_visible(mut self) -> Self {
        self.cursor_visible = true;
        self
    }

    /// Answers [`LoopSense::module_enabled`] from `switches`, so only the named modules are on.
    fn with_switches(mut self, switches: &[&'static str]) -> Self {
        self.switches = Some(switches.to_vec());
        self
    }

    /// Hands the bodies a live settings set instead of the one they were started with.
    fn with_configs(mut self, configs: super::ClickerLoopConfigs) -> Self {
        self.configs = Some(configs);
        self
    }
}

impl LoopSense for StaticSense {
    fn snapshot(&self) -> Option<Arc<crate::app::StateSnapshot>> {
        self.snapshot.clone()
    }
    fn is_connected(&self) -> bool {
        self.connected
    }
    fn is_injected(&self) -> bool {
        self.injected
    }
    fn minecraft_active(&self) -> bool {
        self.active
    }
    fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }
    fn supports_module(&self, module: &str) -> bool {
        self.modules.contains(&module)
    }
    fn supports_state_field(&self, field: &str) -> bool {
        self.fields.contains(&field)
    }
    fn framebuffer_size(&self) -> Option<(i32, i32)> {
        self.framebuffer
    }
    fn module_enabled(&self, module: &str) -> Option<bool> {
        self.switches
            .as_ref()
            .map(|switches| switches.contains(&module))
    }
    fn loop_configs(&self) -> Option<super::ClickerLoopConfigs> {
        self.configs
    }
}

/// A sense that behaves like a live bridge: every read produces a snapshot stamped
/// with the current virtual instant, so freshness is always satisfied.
struct LiveSense {
    state: GameState,
    clock: Arc<VirtualClock>,
}

impl LiveSense {
    fn new(state: GameState, clock: &Arc<VirtualClock>) -> Self {
        Self {
            state,
            clock: Arc::clone(clock),
        }
    }
}

impl LoopSense for LiveSense {
    fn snapshot(&self) -> Option<Arc<crate::app::StateSnapshot>> {
        Some(Arc::new(crate::app::StateSnapshot {
            state: Arc::new(self.state.clone()),
            at: self.clock.now(),
        }))
    }
    fn is_connected(&self) -> bool {
        true
    }
    fn is_injected(&self) -> bool {
        true
    }
    fn minecraft_active(&self) -> bool {
        true
    }
    fn cursor_visible(&self) -> bool {
        false
    }
    fn supports_module(&self, module: &str) -> bool {
        matches!(module, "aimassist" | "triggerbot" | "pixelpartyassist")
    }
    fn supports_state_field(&self, _field: &str) -> bool {
        true
    }
    fn framebuffer_size(&self) -> Option<(i32, i32)> {
        Some((1600, 900))
    }
}

#[derive(Default)]
struct CountingSink {
    clicks: AtomicUsize,
}

impl ClickSink for CountingSink {
    fn record_click(&self, _cps: f32, _left_button: bool) {
        let _ = self.clicks.fetch_add(1, Ordering::AcqRel);
    }
}

fn runtime(
    clock: &Arc<VirtualClock>,
    sleeper: Arc<dyn crate::clock::Sleeper>,
    seed: u64,
) -> LoopRuntime {
    LoopRuntime::new(clock.clone(), sleeper, Box::new(SplitMix64::new(seed)))
}

/// A timing state whose clock is never read: the tests use `Basic`, which is pure
/// RNG, so nothing here depends on real time.
/// Releases a gate and cancels its loop if the test panics, so a failed assertion
/// cannot leave a thread parked forever and turn a test failure into a hang.
struct GateGuard {
    gate: Arc<GateSleeper>,
    token: CancelToken,
}

impl GateGuard {
    fn new(gate: &Arc<GateSleeper>, token: CancelToken) -> Self {
        Self {
            gate: Arc::clone(gate),
            token,
        }
    }
}

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.token.cancel();
        self.gate.release();
    }
}

fn timing() -> AdvancedClickTimingState {
    AdvancedClickTimingState::new(Arc::new(SystemClock), Box::new(SplitMix64::new(101)))
}

fn click_config(left: bool) -> super::click::ClickLoopConfig {
    super::click::ClickLoopConfig {
        mode: ClickRandomizationMode::Basic,
        min_cps: 10.0,
        max_cps: 10.0,
        ..if left {
            super::click::ClickLoopConfig::left()
        } else {
            super::click::ClickLoopConfig::right()
        }
    }
}

/// A state that lets the left click loop click immediately.
fn clicking_state() -> GameState {
    GameState {
        is_connected: true,
        in_world: true,
        health: 20.0,
        ..GameState::default()
    }
}

#[test]
fn a_started_loop_reports_running_and_joins_on_stop() {
    let clock = Arc::new(VirtualClock::new());
    let input = Arc::new(FakeInput::new());
    let sense = StaticSense::new(Some(clicking_state())).inactive();
    let sink = Arc::new(CountingSink::default());
    let loop_runtime = runtime(&clock, clock.clone(), 7);

    let input_for_loop = Arc::clone(&input);
    let sink_for_loop = Arc::clone(&sink);
    let mut handle = LoopHandle::spawn("click-lifecycle", loop_runtime, move |runtime| {
        let mut state = timing();
        super::click::run(
            runtime,
            &sense,
            input_for_loop.as_ref(),
            sink_for_loop.as_ref(),
            &click_config(true),
            &mut state,
        );
    })
    .expect("thread starts");

    assert!(handle.is_running());
    handle.stop();
    assert!(!handle.is_running());
    assert!(handle.token().is_cancelled());
    // The window was never active, so nothing was injected.
    assert_eq!(input.call_count(), 0);
    assert_eq!(sink.clicks.load(Ordering::Acquire), 0);
}

#[test]
fn the_click_loop_injects_exactly_until_it_is_cancelled() {
    let clock = Arc::new(VirtualClock::new());
    let input = Arc::new(FakeInput::new());
    let sense = StaticSense::new(Some(clicking_state()));
    let sink = Arc::new(CountingSink::default());
    let loop_runtime = runtime(&clock, clock.clone(), 3);

    // Cancelling from inside the backend is the only coordination point, so the
    // count is exact rather than best-effort.
    let token = loop_runtime.token().clone();
    input.cancel_after_clicks(3, token.clone());

    let input_for_loop = Arc::clone(&input);
    let sink_for_loop = Arc::clone(&sink);
    let mut handle = LoopHandle::spawn("click-budget", loop_runtime, move |runtime| {
        let mut state = timing();
        super::click::run(
            runtime,
            &sense,
            input_for_loop.as_ref(),
            sink_for_loop.as_ref(),
            &click_config(true),
            &mut state,
        );
    })
    .expect("thread starts");

    // The token is cancelled by the third click, so this wait ends exactly then. The
    // timeout is a safety net, not a schedule.
    assert!(
        token.wait_for(Duration::from_secs(10)),
        "loop did not self-cancel"
    );
    handle.stop();

    assert_eq!(input.click_count(), 3);
    assert_eq!(sink.clicks.load(Ordering::Acquire), 3);
    assert!(input
        .calls()
        .iter()
        .all(|call| matches!(call, InputCall::Click(MouseButton::Left))));
}

#[test]
fn the_right_click_engine_clicks_the_right_button() {
    let clock = Arc::new(VirtualClock::new());
    let input = Arc::new(FakeInput::new());
    let sense = StaticSense::new(Some(clicking_state()));
    let sink = Arc::new(CountingSink::default());
    let loop_runtime = runtime(&clock, clock.clone(), 9);
    let token = loop_runtime.token().clone();
    input.cancel_after_clicks(2, token.clone());

    let input_for_loop = Arc::clone(&input);
    let sink_for_loop = Arc::clone(&sink);
    let mut handle = LoopHandle::spawn("right-click", loop_runtime, move |runtime| {
        let mut state = timing();
        super::click::run(
            runtime,
            &sense,
            input_for_loop.as_ref(),
            sink_for_loop.as_ref(),
            &click_config(false),
            &mut state,
        );
    })
    .expect("thread starts");

    assert!(
        token.wait_for(Duration::from_secs(10)),
        "loop did not self-cancel"
    );
    handle.stop();
    assert_eq!(input.click_count(), 2);
    assert!(input
        .calls()
        .iter()
        .all(|call| matches!(call, InputCall::Click(MouseButton::Right))));
}

/// The AutoClicker's own switch gates the left body, not the bundle's.
///
/// The state here is the one that produced the shipped bug: another loop-driving module is on,
/// so the bundle (and this body) exists, but the AutoClicker is off. `Clicker.StartClicking`
/// refused to start such a stream (`Clicker.cs:470-476`), so nothing may be clicked.
#[test]
fn the_click_body_parks_while_its_own_switch_is_off() {
    let clock = Arc::new(VirtualClock::new());
    // `MODULE_OFF_POLL` is 100 ms: four 25 ms slices, twice.
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 8));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(151)),
    );
    sleeper.attach(loop_runtime.token().clone());
    let token = loop_runtime.token().clone();

    let sense = StaticSense::new(Some(clicking_state())).with_switches(&["aimassist"]);
    let input = FakeInput::new();
    let sink = CountingSink::default();
    let mut state = timing();

    super::click::run(
        &loop_runtime,
        &sense,
        &input,
        &sink,
        &click_config(true),
        &mut state,
    );

    assert!(token.is_cancelled());
    assert_eq!(input.click_count(), 0, "an off module must not click");
    assert_eq!(sink.clicks.load(Ordering::Acquire), 0);
    assert_eq!(
        clock.gaps(),
        vec![Duration::from_millis(25); 8],
        "the body parks at MODULE_OFF_POLL instead of clicking"
    );
}

/// Each click engine answers to its own module id.
///
/// The two engines are one catalog pair, and a gate keyed on the wrong id would run the right
/// clicker whenever the left one was armed: `StartClicking(false)` checked
/// `RightClickEnabled` (`Clicker.cs:477-481`).
#[test]
fn the_right_click_body_does_not_run_on_the_left_modules_switch() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 4));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(157)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let sense = StaticSense::new(Some(clicking_state())).with_switches(&["autoclicker"]);
    let input = FakeInput::new();
    let sink = CountingSink::default();
    let mut state = timing();

    super::click::run(
        &loop_runtime,
        &sense,
        &input,
        &sink,
        &click_config(false),
        &mut state,
    );

    assert_eq!(input.click_count(), 0);
    assert_eq!(clock.gaps(), vec![Duration::from_millis(25); 4]);
}

/// The engine runs the settings the sense reports now, not the ones it was started with.
///
/// The state is a physical chest with the cursor visible, which the guard chain blocks unless
/// `ClickInChests` is on (`Clicker.cs:3145-3157`). The started config has it off, so a click can
/// only come from the live settings — which is the wiring that was missing: every clicker
/// setting a user changed in the GUI was ignored until the bundle restarted.
#[test]
fn the_click_body_reads_its_settings_live() {
    use aoko_protocol::state::ChestStealerState;

    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 1));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(163)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let mut state = clicking_state();
    state.chest_stealer_state = Some(ChestStealerState {
        ready: true,
        physical: true,
        ..ChestStealerState::default()
    });
    state.gui_open = true;
    state.screen_name = "GuiChest".to_owned();

    let mut live = click_config(true);
    live.guards.click_in_chests = true;
    let configs = super::ClickerLoopConfigs {
        left: live,
        ..super::ClickerLoopConfigs::defaults()
    };

    let sense = StaticSense::new(Some(state))
        .with_cursor_visible()
        .with_switches(&["autoclicker"])
        .with_configs(configs);
    let input = FakeInput::new();
    let sink = CountingSink::default();
    let mut state_timing = timing();

    // The starting config blocks chest clicks, so a click proves the live set was used.
    super::click::run(
        &loop_runtime,
        &sense,
        &input,
        &sink,
        &click_config(true),
        &mut state_timing,
    );

    assert_eq!(
        input.click_count(),
        1,
        "the live `ClickInChests` must reach the guard chain"
    );
}

#[test]
fn cancelling_mid_sleep_does_not_inject_another_click() {
    let clock = Arc::new(VirtualClock::new());
    let gate = Arc::new(GateSleeper::new());
    let input = Arc::new(FakeInput::new());
    let sense = StaticSense::new(Some(clicking_state()));
    let sink = Arc::new(CountingSink::default());
    let loop_runtime = runtime(&clock, gate.clone(), 5);
    let token = loop_runtime.token().clone();

    let input_for_loop = Arc::clone(&input);
    let sink_for_loop = Arc::clone(&sink);
    let mut handle = LoopHandle::spawn("click-mid-sleep", loop_runtime, move |runtime| {
        let mut state = timing();
        super::click::run(
            runtime,
            &sense,
            input_for_loop.as_ref(),
            sink_for_loop.as_ref(),
            &click_config(true),
            &mut state,
        );
    })
    .expect("thread starts");

    // The loop clicked once and then parked inside the gate: the wait is provably
    // in progress, not "probably in progress".
    gate.wait_until_entered();
    let clicks_before = input.click_count();
    assert_eq!(clicks_before, 1);

    token.cancel();
    gate.release();
    handle.stop();

    assert_eq!(
        input.click_count(),
        clicks_before,
        "a cancel that lands during the wait must not be followed by another click"
    );
}

#[test]
fn stopping_a_blocked_loop_cancels_before_the_thread_joins() {
    let clock = Arc::new(VirtualClock::new());
    let gate = Arc::new(GateSleeper::new());
    let input = Arc::new(FakeInput::new());
    let sense = StaticSense::new(Some(clicking_state()));
    let sink = Arc::new(CountingSink::default());
    let loop_runtime = runtime(&clock, gate.clone(), 13);

    let input_for_loop = Arc::clone(&input);
    let sink_for_loop = Arc::clone(&sink);
    let handle = LoopHandle::spawn("click-blocked", loop_runtime, move |runtime| {
        let mut state = timing();
        super::click::run(
            runtime,
            &sense,
            input_for_loop.as_ref(),
            sink_for_loop.as_ref(),
            &click_config(true),
            &mut state,
        );
    })
    .expect("thread starts");

    gate.wait_until_entered();
    let clicks_at_block = input.click_count();

    // `stop` joins, so it cannot run on the test thread while the gate is closed.
    // The channel makes "the token is already set" an ordering fact before the gate
    // opens, so the loop cannot slip another click in.
    let (tx, rx) = std::sync::mpsc::channel();
    let stopper = std::thread::spawn(move || {
        let token = handle.token().clone();
        token.cancel();
        tx.send(()).expect("receiver alive");
        let mut handle = handle;
        handle.stop();
        handle
    });

    rx.recv().expect("cancel reported");
    gate.release();
    let handle = stopper.join().expect("stopper thread");

    assert!(!handle.is_running(), "the handle must be joined after stop");
    assert!(handle.token().is_cancelled());
    assert_eq!(input.click_count(), clicks_at_block);
}

#[test]
fn a_dropped_handle_stops_its_loop() {
    let clock = Arc::new(VirtualClock::new());
    let sense = StaticSense::new(Some(clicking_state())).inactive();
    let input = Arc::new(FakeInput::new());
    let sink = Arc::new(CountingSink::default());
    let loop_runtime = runtime(&clock, clock.clone(), 23);
    let token = loop_runtime.token().clone();

    let handle = LoopHandle::spawn("click-drop", loop_runtime, move |runtime| {
        let mut state = timing();
        super::click::run(
            runtime,
            &sense,
            input.as_ref(),
            sink.as_ref(),
            &click_config(true),
            &mut state,
        );
    })
    .expect("thread starts");

    assert!(!token.is_cancelled());
    drop(handle);
    assert!(
        token.is_cancelled(),
        "dropping the handle must stop the loop rather than leak the thread"
    );
}

#[test]
fn a_stopped_loop_never_touches_the_backend_again() {
    let clock = Arc::new(VirtualClock::new());
    let gate = Arc::new(GateSleeper::new());
    let input = Arc::new(FakeInput::new());
    let sense = StaticSense::new(Some(clicking_state()));
    let sink = Arc::new(CountingSink::default());
    let loop_runtime = runtime(&clock, gate.clone(), 47);

    let input_for_loop = Arc::clone(&input);
    let sink_for_loop = Arc::clone(&sink);
    let mut handle = LoopHandle::spawn("click-quiet", loop_runtime, move |runtime| {
        let mut state = timing();
        super::click::run(
            runtime,
            &sense,
            input_for_loop.as_ref(),
            sink_for_loop.as_ref(),
            &click_config(true),
            &mut state,
        );
    })
    .expect("thread starts");

    gate.wait_until_entered();
    gate.release();
    handle.stop();
    let after_stop = input.call_count();
    // The thread is joined, so a late tick is impossible; reading twice makes the
    // guarantee explicit.
    assert_eq!(input.call_count(), after_stop);
    assert!(!handle.is_running());
}

#[test]
fn the_click_loop_polls_at_one_hundred_milliseconds_while_not_connected() {
    let clock = Arc::new(VirtualClock::new());
    // 100 ms is four 25 ms slices, so the fourth sleep is the last.
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 4));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(31)),
    );
    sleeper.attach(loop_runtime.token().clone());
    let token = loop_runtime.token().clone();
    // Not connected *and* the OS cursor is visible: the only not-connected branch
    // that polls instead of clicking (`Clicker.cs:3175-3183`).
    let sense = StaticSense::new(None).with_cursor_visible();
    let input = FakeInput::new();
    let sink = CountingSink::default();
    let mut state = timing();

    super::click::run(
        &loop_runtime,
        &sense,
        &input,
        &sink,
        &click_config(true),
        &mut state,
    );

    assert!(token.is_cancelled());
    assert_eq!(
        clock.gaps(),
        vec![
            Duration::from_millis(25),
            Duration::from_millis(25),
            Duration::from_millis(25),
            Duration::from_millis(25)
        ]
    );
    assert_eq!(input.call_count(), 0);
}

#[test]
fn the_triggerbot_polls_at_one_hundred_milliseconds_when_disabled() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 8));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(59)),
    );
    sleeper.attach(loop_runtime.token().clone());
    let token = loop_runtime.token().clone();
    // `TriggerbotEnabled` off: an empty switch set, since the switch is the sense's now and
    // not a field of the settings (`Clicker.cs:2201` started and cancelled the loop on it).
    let sense = StaticSense::new(None).with_switches(&[]);
    let input = FakeInput::new();

    let config = super::triggerbot::TriggerbotConfig::default();

    super::triggerbot::run(&loop_runtime, &sense, &input, &config);

    assert!(token.is_cancelled());
    // `MODULE_OFF_POLL` is 100 ms, sliced into four 25 ms waits per tick.
    assert_eq!(
        clock.gaps(),
        vec![Duration::from_millis(25); 8],
        "a disabled body parks rather than polling at the enabled cadence"
    );
    assert_eq!(input.call_count(), 0);
}

#[test]
fn the_triggerbot_stops_at_the_stale_state_poll() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 1));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(63)),
    );
    sleeper.attach(loop_runtime.token().clone());
    // A state that has been sitting since the clock's origin: far older than 35 ms
    // after the first slice.
    let sense = StaticSense::new(Some(clicking_state()));
    let input = FakeInput::new();

    let config = super::triggerbot::TriggerbotConfig::default();

    super::triggerbot::run(&loop_runtime, &sense, &input, &config);
    assert_eq!(clock.gaps(), vec![Duration::from_millis(8)]);
    assert_eq!(input.call_count(), 0);
}

#[test]
fn the_aim_loop_never_moves_when_the_module_is_off() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 2));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(37)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let input = FakeInput::new();
    // `AimAssistEnabled` off. The body still exists — the bundle keeps seven threads alive —
    // so it has to park on its own switch instead of moving the crosshair.
    let sense = StaticSense::new(Some(aim_target_state())).with_switches(&[]);
    let mut controller = crate::aim_assist::AimAssistMotionController::new();

    super::aim::run(
        &loop_runtime,
        &sense,
        &input,
        &super::aim::AimLoopConfig {
            params: crate::aim_assist::AimAssistParams {
                width: 1600,
                height: 900,
                game_fov: 70.0,
                aim_assist_fov: 30.0,
                range: 6.0,
                strength: 100,
            },
            // A licensed tick: without the switch this configuration does move.
            auto_left_clicking: true,
        },
        &mut controller,
    );

    assert_eq!(input.move_count(), 0);
    assert_eq!(
        clock.gaps(),
        vec![Duration::from_millis(25); 2],
        "an off module parks at MODULE_OFF_POLL (100 ms, four 25 ms slices)"
    );
}

#[test]
fn the_aim_loop_uses_sixteen_milliseconds_while_a_gui_owns_the_cursor() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 1));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(61)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let mut state = clicking_state();
    state.gui_open = true;
    let sense = StaticSense::new(Some(state)).with_cursor_visible();
    let input = FakeInput::new();
    let mut controller = crate::aim_assist::AimAssistMotionController::new();

    super::aim::run(
        &loop_runtime,
        &sense,
        &input,
        &super::aim::AimLoopConfig {
            params: crate::aim_assist::AimAssistParams {
                width: 1600,
                height: 900,
                game_fov: 70.0,
                aim_assist_fov: 30.0,
                range: 6.0,
                strength: 50,
            },
            auto_left_clicking: true,
        },
        &mut controller,
    );

    assert_eq!(clock.gaps(), vec![Duration::from_millis(16)]);
    assert_eq!(input.move_count(), 0);
}

#[test]
fn the_aim_loop_injects_a_move_and_ticks_eight_milliseconds() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 1));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(67)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let mut state = clicking_state();
    state.viewport_width = 1600;
    state.viewport_height = 900;
    state.fov = 70.0;
    state.entities = vec![EntityInfo {
        sx: 900.0,
        sy: 450.0,
        dist: 3.0,
        name: "Target".to_owned(),
        ..EntityInfo::default()
    }];
    let sense = StaticSense::new(Some(state));
    let input = FakeInput::new();
    input.set_left_down(true);
    let mut controller = crate::aim_assist::AimAssistMotionController::new();

    super::aim::run(
        &loop_runtime,
        &sense,
        &input,
        &super::aim::AimLoopConfig {
            params: crate::aim_assist::AimAssistParams {
                width: 1600,
                height: 900,
                game_fov: 70.0,
                aim_assist_fov: 30.0,
                range: 6.0,
                strength: 100,
            },
            auto_left_clicking: false,
        },
        &mut controller,
    );

    assert!(
        input.move_count() >= 1,
        "expected an aim move, got {:?}",
        input.calls()
    );
    assert_eq!(clock.gaps(), vec![Duration::from_millis(8)]);
}

#[test]
fn the_triggerbot_never_clicks_when_it_is_not_enabled() {
    let clock = Arc::new(VirtualClock::new());
    let gate = Arc::new(GateSleeper::new());
    let input = Arc::new(FakeInput::new());
    // `TriggerbotEnabled` off: the switch is the sense's, see `TriggerbotConfig`.
    let sense = StaticSense::new(Some(clicking_state())).with_switches(&[]);
    let loop_runtime = runtime(&clock, gate.clone(), 41);

    let input_for_loop = Arc::clone(&input);
    let mut handle = LoopHandle::spawn("triggerbot-off", loop_runtime, move |runtime| {
        super::triggerbot::run(
            runtime,
            &sense,
            input_for_loop.as_ref(),
            &super::triggerbot::TriggerbotConfig::default(),
        );
    })
    .expect("thread starts");

    gate.wait_until_entered();
    input.clear();
    // Release before joining: `stop` joins, and the loop is parked inside the gate.
    gate.release();
    handle.stop();
    assert_eq!(input.call_count(), 0);
}

#[test]
fn the_triggerbot_clicks_once_the_reaction_and_cooldown_are_ready() {
    let clock = Arc::new(VirtualClock::new());
    // The first tick arms the target, the reaction is at most 220 ms, and the tick
    // is 4 ms: 120 ticks is past the deadline without depending on the draw.
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 120));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(71)),
    );
    sleeper.attach(loop_runtime.token().clone());
    let token = loop_runtime.token().clone();

    let mut state = clicking_state();
    state.viewport_width = 1600;
    state.viewport_height = 900;
    state.attack_cooldown = 1.0;
    state.attack_cooldown_per_tick = 0.05;
    state.looking_at_entity = true;
    // `CanAttackTargetNow` requires a live entity even when the bridge already
    // reports the crosshair as on a target (`Clicker.cs:2988-2990`).
    state.entities = vec![EntityInfo {
        sx: 800.0,
        sy: 450.0,
        dist: 3.0,
        hp: 20.0,
        ..EntityInfo::default()
    }];
    // A live bridge keeps the state fresh; a frozen snapshot would age out after
    // 35 ms and the loop would never reach the reaction deadline.
    let sense = LiveSense::new(state, &clock);
    let input = FakeInput::new();
    input.set_left_down(true);

    super::triggerbot::run(
        &loop_runtime,
        &sense,
        &input,
        &super::triggerbot::TriggerbotConfig {
            require_click: true,
            only_crosshair: true,
            only_if_can_attack: true,
            cooldown_threshold: 90,
            hit_chance: 100,
            break_blocks_enabled: false,
        },
    );

    assert!(token.is_cancelled());
    // A 90 % cooldown threshold with 0.05 s per tick means a 900 ms minimum
    // interval, so 480 ms of ticks admits exactly one click.
    assert_eq!(input.click_count(), 1, "one click per armed target");
    assert!(input
        .calls()
        .iter()
        .all(|call| matches!(call, InputCall::Click(MouseButton::Left))));
}

#[test]
fn the_pixel_party_loop_releases_its_keys_when_cancelled() {
    let clock = Arc::new(VirtualClock::new());
    let gate = Arc::new(GateSleeper::new());
    let input = Arc::new(FakeInput::new());
    let state = GameState {
        is_connected: true,
        in_world: true,
        pixel_party_target_found: true,
        pixel_party_target_dist: 8.0,
        pixel_party_yaw_delta: 5.0,
        ..GameState::default()
    };
    let sense = StaticSense::new(Some(state));

    let loop_runtime = runtime(&clock, gate.clone(), 43);
    let token = loop_runtime.token().clone();
    let input_for_loop = Arc::clone(&input);
    let handle = LoopHandle::spawn("pixel-party", loop_runtime, move |runtime| {
        let mut keys = super::pixel_party::WalkKeys::default();
        super::pixel_party::run(
            runtime,
            &sense,
            input_for_loop.as_ref(),
            &super::pixel_party::PixelPartyConfig {
                    auto_look: true,
                auto_walk: true,
            },
            &mut keys,
        );
    })
    .expect("thread starts");

    let _guard = GateGuard::new(&gate, token.clone());
    gate.wait_until_entered();
    let calls = input.calls();
    assert!(
        calls
            .iter()
            .any(|call| matches!(call, InputCall::KeyDown(Key::Char('w')))),
        "forward should be held while a distant target is in the walk cone: {calls:?}"
    );

    // `stop` joins, so it runs beside the gate: cancel first, prove it through the
    // channel, release the gate, and only then let the join complete.
    let (tx, rx) = std::sync::mpsc::channel();
    let stopper = std::thread::spawn(move || {
        handle.token().cancel();
        tx.send(()).expect("receiver alive");
        let mut handle = handle;
        handle.stop();
        handle
    });
    rx.recv().expect("cancel reported");
    gate.release();
    let handle = stopper.join().expect("stopper thread");
    assert!(!handle.is_running());

    let calls = input.calls();
    assert!(
        calls
            .iter()
            .any(|call| matches!(call, InputCall::KeyUp(Key::Char('w')))),
        "the loop must release forward on the way out: {calls:?}"
    );
    assert!(!super::pixel_party::WalkKeys::default().any_held());
}

#[test]
fn every_body_starts_on_its_own_thread_and_stops_together() {
    let clock = Arc::new(VirtualClock::new());
    let sense: Arc<dyn LoopSense> = Arc::new(StaticSense::new(Some(clicking_state())).inactive());
    let fake = Arc::new(FakeInput::new());
    let input: Arc<dyn InputBackend> = Arc::clone(&fake) as Arc<dyn InputBackend>;
    let sink: Arc<dyn ClickSink> = Arc::new(CountingSink::default());

    let make_runtime = {
        let clock = Arc::clone(&clock);
        move |seed: u64| {
            LoopRuntime::new(
                clock.clone(),
                clock.clone(),
                Box::new(SplitMix64::new(0x1000 + seed)),
            )
        }
    };

    let mut loops = super::ClickerLoops::start(sense, Arc::clone(&input), sink, make_runtime)
        .expect("all seven bodies start");

    assert_eq!(
        loops.names(),
        vec![
            "click-left",
            "click-right",
            "aim-assist",
            "pixel-party",
            "triggerbot",
            "chest-stealer",
            "refill"
        ]
    );
    assert_eq!(loops.running(), 7);

    loops.stop();
    assert_eq!(loops.running(), 0);
    // The window was never active, so no body injected anything.
    assert_eq!(fake.call_count(), 0);

    // Stop is idempotent and dropping afterwards is harmless.
    loops.stop();
    drop(loops);
}

// ---------------------------------------------------------------------------
// The not-connected branch (`Clicker.cs:3175-3183`).
//
// A missing snapshot is *not* "do nothing": managed falls through to the click
// logic when the cursor is hidden, which is what keeps standalone clicking alive.
// ---------------------------------------------------------------------------

#[test]
fn a_missing_snapshot_with_a_hidden_cursor_still_clicks() {
    let clock = Arc::new(VirtualClock::new());
    // One sleep: the click lands first, then the post-click wait cancels us.
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 1));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(71)),
    );
    sleeper.attach(loop_runtime.token().clone());
    let token = loop_runtime.token().clone();

    // No snapshot at all, cursor hidden, window active: before the fix this parked
    // forever and standalone clicking was dead.
    let sense = StaticSense::new(None);
    let input = FakeInput::new();
    let sink = CountingSink::default();
    let mut state = timing();

    super::click::run(
        &loop_runtime,
        &sense,
        &input,
        &sink,
        &click_config(true),
        &mut state,
    );

    assert!(token.is_cancelled());
    assert_eq!(
        input.click_count(),
        1,
        "an un-injected session with a hidden cursor must click"
    );
    assert_eq!(sink.clicks.load(Ordering::Acquire), 1);
}

#[test]
fn a_missing_snapshot_with_a_visible_cursor_never_clicks() {
    let clock = Arc::new(VirtualClock::new());
    // `NOT_FOREGROUND_POLL` is 100 ms, sliced into four 25 ms waits.
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 4));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(73)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let sense = StaticSense::new(None).with_cursor_visible();
    let input = FakeInput::new();
    let sink = CountingSink::default();
    let mut state = timing();

    super::click::run(
        &loop_runtime,
        &sense,
        &input,
        &sink,
        &click_config(true),
        &mut state,
    );

    assert_eq!(input.call_count(), 0);
    assert_eq!(sink.clicks.load(Ordering::Acquire), 0);
}

// ---------------------------------------------------------------------------
// GUI before freshness (`Clicker.cs:2676-2696` and `:2741-2768`).
//
// A stale state that also has a GUI open and the cursor visible must take the
// 16 ms GUI path, not the 8 ms stale path.
// ---------------------------------------------------------------------------

#[test]
fn the_aim_loop_checks_the_gui_before_a_stale_state() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 1));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(79)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let mut state = clicking_state();
    state.gui_open = true;
    let sense = StaticSense::new(Some(state)).with_cursor_visible();
    // The snapshot is stamped at the clock's origin; a second of virtual time makes
    // it far older than `AIM_STATE_FRESH_MS` (220 ms) without touching the wall
    // clock.
    clock.advance(Duration::from_secs(1));

    let input = FakeInput::new();
    let mut controller = crate::aim_assist::AimAssistMotionController::new();
    super::aim::run(
        &loop_runtime,
        &sense,
        &input,
        &super::aim::AimLoopConfig {
            params: crate::aim_assist::AimAssistParams {
                width: 1600,
                height: 900,
                game_fov: 70.0,
                aim_assist_fov: 30.0,
                range: 6.0,
                strength: 50,
            },
            auto_left_clicking: true,
        },
        &mut controller,
    );

    assert_eq!(
        clock.gaps(),
        vec![Duration::from_millis(16)],
        "the GUI gate runs before the staleness check, so the delay is 16 ms"
    );
    assert_eq!(input.move_count(), 0);
}

#[test]
fn the_pixel_party_loop_checks_the_gui_before_a_stale_state() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 1));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(83)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let mut state = clicking_state();
    state.gui_open = true;
    let sense = StaticSense::new(Some(state)).with_cursor_visible();
    // Far older than `PIXEL_PARTY_STATE_FRESH_MS` (120 ms).
    clock.advance(Duration::from_secs(1));

    let input = FakeInput::new();
    let mut keys = super::pixel_party::WalkKeys::default();
    super::pixel_party::run(
        &loop_runtime,
        &sense,
        &input,
        &super::pixel_party::PixelPartyConfig {
            auto_look: true,
            auto_walk: true,
        },
        &mut keys,
    );

    assert_eq!(
        clock.gaps(),
        vec![Duration::from_millis(16)],
        "the GUI gate runs before the staleness check, so the delay is 16 ms"
    );
    assert_eq!(input.call_count(), 0);
}

// ---------------------------------------------------------------------------
// Liveness (`ClickerLoops::running`).
// ---------------------------------------------------------------------------

#[test]
fn a_body_that_returned_early_is_not_reported_as_running() {
    let clock = Arc::new(VirtualClock::new());
    let loop_runtime = runtime(&clock, clock.clone(), 89);
    let (done_tx, done_rx) = std::sync::mpsc::channel();

    let mut handle = LoopHandle::spawn("early-exit", loop_runtime, move |runtime| {
        assert!(!runtime.is_cancelled());
        let _ = done_tx.send(());
        // Returns without ever consulting the token: a body that finished on its
        // own.
    })
    .expect("thread starts");

    done_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("body ran");

    // The flag clears as the body leaves its scope, so wait for that fact rather
    // than asserting an elapsed time.
    let mut spins = 0_u32;
    while handle.is_running() {
        std::hint::spin_loop();
        spins += 1;
        assert!(spins < 10_000_000, "the liveness flag never cleared");
    }

    assert!(!handle.is_running());
    assert!(
        handle.owns_thread(),
        "the thread handle is still owned until stop"
    );

    handle.stop();
    assert!(!handle.owns_thread());
    assert!(!handle.is_running());
}

// ---------------------------------------------------------------------------
// Unobservable input (`InputBackend::probe_mouse_state` returning `None`).
//
// `None` is "this session cannot see the mouse", not "no button is held". Both
// hold guards are licensed by proof of a hold, so unknown parks the tick without
// pretending to have observed a free button, and the loop keeps polling so it
// recovers the moment the session becomes observable.
// ---------------------------------------------------------------------------

/// Flips a [`FakeInput`] to observable (and held, if asked) on the `flip_at`-th
/// sleep and cancels `cancel_at` sleeps later, so the loop's own polling drives
/// the recovery instead of the test's timing.
struct FlipInputOnSleep {
    clock: Arc<VirtualClock>,
    input: Arc<FakeInput>,
    flip_at: usize,
    left_down: bool,
    cancel_at: usize,
    token: parking_lot::Mutex<Option<CancelToken>>,
    sleeps: AtomicUsize,
}

impl FlipInputOnSleep {
    fn new(
        clock: Arc<VirtualClock>,
        input: Arc<FakeInput>,
        flip_at: usize,
        left_down: bool,
        cancel_at: usize,
    ) -> Self {
        Self {
            clock,
            input,
            flip_at,
            left_down,
            cancel_at,
            token: parking_lot::Mutex::new(None),
            sleeps: AtomicUsize::new(0),
        }
    }

    fn attach(&self, token: CancelToken) {
        *self.token.lock() = Some(token);
    }
}

impl crate::clock::Sleeper for FlipInputOnSleep {
    fn sleep_until(&self, deadline: Instant) {
        self.clock.sleep_until(deadline);
        let count = self.sleeps.fetch_add(1, Ordering::AcqRel) + 1;
        if count == self.flip_at {
            self.input.set_mouse_unobservable(false);
            self.input.set_left_down(self.left_down);
        }
        if count >= self.cancel_at {
            if let Some(token) = self.token.lock().as_ref() {
                token.cancel();
            }
        }
    }
}

/// A state with one target a little right of centre, so a licensed aim tick moves.
fn aim_target_state() -> GameState {
    let mut state = clicking_state();
    state.viewport_width = 1600;
    state.viewport_height = 900;
    state.fov = 70.0;
    state.entities = vec![EntityInfo {
        sx: 900.0,
        sy: 450.0,
        dist: 3.0,
        name: "Target".to_owned(),
        ..EntityInfo::default()
    }];
    state
}

fn aim_config(auto_left_clicking: bool) -> super::aim::AimLoopConfig {
    super::aim::AimLoopConfig {
        params: crate::aim_assist::AimAssistParams {
            width: 1600,
            height: 900,
            game_fov: 70.0,
            aim_assist_fov: 30.0,
            range: 6.0,
            strength: 100,
        },
        auto_left_clicking,
    }
}

#[test]
fn the_aim_loop_does_not_move_while_the_mouse_is_unobservable() {
    let clock = Arc::new(VirtualClock::new());
    // Five 8 ms polls of the reset path: enough ticks that a mistaken "free
    // button" reading would have produced a move.
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 5));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(97)),
    );
    sleeper.attach(loop_runtime.token().clone());
    let token = loop_runtime.token().clone();

    let input = FakeInput::new();
    input.set_mouse_unobservable(true);
    let sense = StaticSense::new(Some(aim_target_state()));
    let mut controller = crate::aim_assist::AimAssistMotionController::new();

    super::aim::run(
        &loop_runtime,
        &sense,
        &input,
        &aim_config(false),
        &mut controller,
    );

    assert!(token.is_cancelled());
    assert_eq!(input.move_count(), 0, "unknown must not license a move");
    assert_eq!(
        clock.gaps(),
        vec![Duration::from_millis(8); 5],
        "the loop keeps polling rather than breaking out"
    );
    assert!(!controller.has_latched_target());
}

#[test]
fn the_aim_loop_recovers_when_the_mouse_becomes_observable() {
    let clock = Arc::new(VirtualClock::new());
    let input = Arc::new(FakeInput::new());
    input.set_mouse_unobservable(true);
    let backend: &dyn InputBackend = input.as_ref();
    // Unobservable for four polls, then provably held.
    let sleeper = Arc::new(FlipInputOnSleep::new(
        clock.clone(),
        Arc::clone(&input),
        4,
        true,
        12,
    ));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(101)),
    );
    sleeper.attach(loop_runtime.token().clone());
    let token = loop_runtime.token().clone();

    let sense = StaticSense::new(Some(aim_target_state()));
    let mut controller = crate::aim_assist::AimAssistMotionController::new();

    super::aim::run(
        &loop_runtime,
        &sense,
        backend,
        &aim_config(false),
        &mut controller,
    );

    assert!(token.is_cancelled());
    assert!(
        input.move_count() >= 1,
        "the loop must resume once the sample exists again"
    );
}

#[test]
fn the_aim_loop_still_needs_the_hold_when_the_mouse_is_observable() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 5));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(103)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let input = FakeInput::new();
    // Observable and provably *not* held: the guard is unchanged.
    input.set_left_down(false);
    let sense = StaticSense::new(Some(aim_target_state()));
    let mut controller = crate::aim_assist::AimAssistMotionController::new();

    super::aim::run(
        &loop_runtime,
        &sense,
        &input,
        &aim_config(false),
        &mut controller,
    );

    assert_eq!(input.move_count(), 0);
}

#[test]
fn auto_left_clicking_licenses_the_aim_loop_without_an_observation() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 1));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(107)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let input = FakeInput::new();
    input.set_mouse_unobservable(true);
    let sense = StaticSense::new(Some(aim_target_state()));
    let mut controller = crate::aim_assist::AimAssistMotionController::new();

    super::aim::run(
        &loop_runtime,
        &sense,
        &input,
        &aim_config(true),
        &mut controller,
    );

    assert!(
        input.move_count() >= 1,
        "the hold the guard asks for comes from the clicker itself, so an \
         unobservable physical button is not part of the decision"
    );
}

/// The clicking triggerbot configuration and state used by the hold-guard tests.
fn triggerbot_target_state() -> GameState {
    let mut state = clicking_state();
    state.viewport_width = 1600;
    state.viewport_height = 900;
    state.attack_cooldown = 1.0;
    state.attack_cooldown_per_tick = 0.05;
    state.looking_at_entity = true;
    state.entities = vec![EntityInfo {
        sx: 800.0,
        sy: 450.0,
        dist: 3.0,
        hp: 20.0,
        ..EntityInfo::default()
    }];
    state
}

fn triggerbot_config(require_click: bool) -> super::triggerbot::TriggerbotConfig {
    super::triggerbot::TriggerbotConfig {
        require_click,
        only_crosshair: true,
        only_if_can_attack: true,
        cooldown_threshold: 90,
        hit_chance: 100,
        break_blocks_enabled: false,
    }
}

#[test]
fn the_triggerbot_does_not_fire_while_the_mouse_is_unobservable() {
    let clock = Arc::new(VirtualClock::new());
    // 30 polls is 240 ms of virtual time: far past the 45..220 ms reaction window,
    // so an unlicensed click would have happened.
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 30));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(109)),
    );
    sleeper.attach(loop_runtime.token().clone());
    let token = loop_runtime.token().clone();

    let input = FakeInput::new();
    input.set_mouse_unobservable(true);
    let sense = LiveSense::new(triggerbot_target_state(), &clock);

    super::triggerbot::run(&loop_runtime, &sense, &input, &triggerbot_config(true));

    assert!(token.is_cancelled());
    assert_eq!(input.click_count(), 0, "unknown must not fire the trigger");
    assert_eq!(
        clock.gaps(),
        vec![Duration::from_millis(8); 30],
        "the loop keeps polling at the hold guard's cadence"
    );
}

#[test]
fn the_triggerbot_recovers_when_the_mouse_becomes_observable() {
    let clock = Arc::new(VirtualClock::new());
    let input = Arc::new(FakeInput::new());
    input.set_mouse_unobservable(true);
    let backend: &dyn InputBackend = input.as_ref();
    // Unobservable for 30 polls, then observable and held for the remaining 200.
    let sleeper = Arc::new(FlipInputOnSleep::new(
        clock.clone(),
        Arc::clone(&input),
        30,
        true,
        230,
    ));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(113)),
    );
    sleeper.attach(loop_runtime.token().clone());
    let token = loop_runtime.token().clone();

    let sense = LiveSense::new(triggerbot_target_state(), &clock);

    super::triggerbot::run(&loop_runtime, &sense, backend, &triggerbot_config(true));

    assert!(token.is_cancelled());
    assert_eq!(
        input.click_count(),
        1,
        "exactly one click once the hold is observable, none before"
    );
}

#[test]
fn the_triggerbot_ignores_an_unobservable_mouse_when_the_hold_guard_is_off() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 120));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(127)),
    );
    sleeper.attach(loop_runtime.token().clone());

    let input = FakeInput::new();
    input.set_mouse_unobservable(true);
    let sense = LiveSense::new(triggerbot_target_state(), &clock);

    // `require_click` is what needs the observation; with it off the trigger does
    // not over-block.
    super::triggerbot::run(&loop_runtime, &sense, &input, &triggerbot_config(false));

    assert_eq!(input.click_count(), 1);
}
