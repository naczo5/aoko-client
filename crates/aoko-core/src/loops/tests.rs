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
fn the_click_loop_polls_at_one_hundred_milliseconds_without_a_state() {
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
fn the_triggerbot_polls_at_twenty_five_milliseconds_when_disabled() {
    let clock = Arc::new(VirtualClock::new());
    let sleeper = Arc::new(CancelAfterSleeper::new(clock.clone(), 3));
    let loop_runtime = LoopRuntime::new(
        clock.clone(),
        sleeper.clone(),
        Box::new(SplitMix64::new(59)),
    );
    sleeper.attach(loop_runtime.token().clone());
    let token = loop_runtime.token().clone();
    let sense = StaticSense::new(None);
    let input = FakeInput::new();

    let config = super::triggerbot::TriggerbotConfig {
        enabled: false,
        ..super::triggerbot::TriggerbotConfig::default()
    };

    super::triggerbot::run(&loop_runtime, &sense, &input, &config);

    assert!(token.is_cancelled());
    assert_eq!(
        clock.gaps(),
        vec![
            Duration::from_millis(25),
            Duration::from_millis(25),
            Duration::from_millis(25)
        ]
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

    let config = super::triggerbot::TriggerbotConfig {
        enabled: true,
        ..super::triggerbot::TriggerbotConfig::default()
    };

    super::triggerbot::run(&loop_runtime, &sense, &input, &config);
    assert_eq!(clock.gaps(), vec![Duration::from_millis(8)]);
    assert_eq!(input.call_count(), 0);
}

#[test]
fn the_aim_loop_never_moves_when_the_module_is_off() {
    let clock = Arc::new(VirtualClock::new());
    let gate = Arc::new(GateSleeper::new());
    let input = Arc::new(FakeInput::new());
    let sense = StaticSense::new(Some(clicking_state()));
    let loop_runtime = runtime(&clock, gate.clone(), 37);

    let input_for_loop = Arc::clone(&input);
    let mut handle = LoopHandle::spawn("aim-off", loop_runtime, move |runtime| {
        let mut controller = crate::aim_assist::AimAssistMotionController::new();
        super::aim::run(
            runtime,
            &sense,
            input_for_loop.as_ref(),
            &super::aim::AimLoopConfig {
                enabled: false,
                params: crate::aim_assist::AimAssistParams {
                    width: 1600,
                    height: 900,
                    game_fov: 70.0,
                    aim_assist_fov: 30.0,
                    range: 6.0,
                    strength: 50,
                },
                auto_left_clicking: false,
            },
            &mut controller,
        );
    })
    .expect("thread starts");

    gate.wait_until_entered();
    // Release before joining: `stop` joins, and the loop is parked inside the gate.
    gate.release();
    handle.stop();
    assert_eq!(input.move_count(), 0);
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
            enabled: true,
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
            enabled: true,
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
    let sense = StaticSense::new(Some(clicking_state()));
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
            enabled: true,
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
                enabled: true,
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
fn all_five_bodies_start_on_their_own_threads_and_stop_together() {
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

    let mut loops = super::ClickerLoops::start(
        sense,
        Arc::clone(&input),
        sink,
        super::ClickerLoopConfigs::defaults(),
        make_runtime,
    )
    .expect("all five bodies start");

    assert_eq!(
        loops.names(),
        vec![
            "click-left",
            "click-right",
            "aim-assist",
            "pixel-party",
            "triggerbot"
        ]
    );
    assert_eq!(loops.running(), 5);

    loops.stop();
    assert_eq!(loops.running(), 0);
    // The window was never active, so no body injected anything.
    assert_eq!(fake.call_count(), 0);

    // Stop is idempotent and dropping afterwards is harmless.
    loops.stop();
    drop(loops);
}
