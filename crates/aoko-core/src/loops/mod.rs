//! The automation loops: real threads, cancellable, driven by injected time.
//!
//! `MIGRATION_ARCHITECTURE.md` section 4 assigns every blocking loop to its own
//! dedicated thread and every cross-thread hand-off to a lock-free snapshot. This
//! module is that contract:
//!
//! * [`LoopHandle`] owns one OS thread and a [`CancelToken`]. `stop` cancels and
//!   joins; `Drop` does the same, so a forgotten loop cannot outlive its owner.
//! * [`LoopRuntime`] is everything a body needs from the outside world: the
//!   [`Clock`], the [`Sleeper`], the token, and the [`Rng`]. Nothing in a body
//!   reads `Instant::now()` or a thread-local RNG.
//! * [`LoopSense`] is the read side: a snapshot of [`aoko_protocol::state::GameState`]
//!   plus the handful of facts that do not live in the state (foreground window,
//!   OS cursor visibility, which bridge modules are supported).
//! * Writing goes through `aoko_platform::InputBackend`.
//!
//! ## Cancellation latency
//!
//! The C# loops await `Task.Delay(ms, token)`, which wakes the continuation the
//! moment the token fires. A blocking `Sleeper` has no such channel, so
//! [`LoopRuntime::park`] splits a wait into [`CANCEL_SLICE`] pieces and re-checks
//! the token between them. A stop is therefore honoured within one slice, and a
//! slice is a real `sleep` — never a spin.
//!
//! ## Deadlines
//!
//! Waits are absolute by the time they reach the sleeper: `park(d)` computes
//! `now + d` once and then sleeps *until* that instant, so a slow wake-up shortens
//! the next piece instead of accumulating drift.

pub mod aim;
pub mod click;
pub mod pixel_party;
pub mod triggerbot;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::StateSnapshot;
use crate::clock::{next_below, Clock, Rng, Sleeper};
use crate::concurrency::CancelToken;

/// Longest single sleep a body takes while it has a cancellation check pending.
///
/// This bounds stop latency, not scheduling accuracy: every wait still targets an
/// absolute deadline, so splitting one wait into several does not move the deadline.
/// Every poll interval the loops use (4, 8, 10, 12, 16, 20, 25 ms) fits in one
/// slice, so only the 100 ms guard polls are split, into four.
pub const CANCEL_SLICE: Duration = Duration::from_millis(25);

/// Reads the outside world for a loop body.
///
/// Everything that is not in [`aoko_protocol::state::GameState`] but that the C#
/// loops query directly — `WindowDetection.IsMinecraftActive`, `IsCursorVisible`,
/// `GameStateClient.SupportsModule` — is here. The production implementation is
/// [`StoreSense`]; a test can implement the trait over fixed values.
pub trait LoopSense: Send + Sync {
    /// Latest state snapshot, `None` before the first bridge message.
    fn snapshot(&self) -> Option<Arc<StateSnapshot>>;

    /// `GameStateClient.IsConnected`.
    fn is_connected(&self) -> bool;

    /// `GameStateClient.IsInjected`.
    fn is_injected(&self) -> bool;

    /// `WindowDetection.IsMinecraftActive()`.
    fn minecraft_active(&self) -> bool;

    /// `WindowDetection.IsCursorVisible()`.
    fn cursor_visible(&self) -> bool;

    /// `GameStateClient.SupportsModule(id)`: whether the attached bridge
    /// advertises the module.
    fn supports_module(&self, module: &str) -> bool;

    /// `GameStateClient.SupportsStateField(field)`.
    fn supports_state_field(&self, field: &str) -> bool;

    /// Window size to fall back on when the state carries no viewport.
    fn framebuffer_size(&self) -> Option<(i32, i32)>;

    /// Age of the current snapshot at `now`.
    fn state_age_ms(&self, now: Instant) -> Option<u64> {
        self.snapshot()
            .map(|snapshot| now.saturating_duration_since(snapshot.at).as_millis() as u64)
    }

    /// The snapshot when it is younger than `max_age_ms`, else `None`.
    ///
    /// One atomic load, so the loop cannot observe a torn state/age pair.
    fn fresh_state(&self, now: Instant, max_age_ms: u64) -> Option<Arc<StateSnapshot>> {
        let snapshot = self.snapshot()?;
        let age = now.saturating_duration_since(snapshot.at).as_millis() as u64;
        if age > max_age_ms {
            return None;
        }
        Some(snapshot)
    }
}

/// Receives one click sample.
///
/// The managed loop calls `StatsTracker.Instance.RecordClick(cps, leftButton)`
/// inline; a trait keeps the counter off the automation thread's critical path.
pub trait ClickSink: Send + Sync {
    /// Records one accepted click at `cps`.
    fn record_click(&self, cps: f32, left_button: bool);
}

/// A sink that discards samples, for loops that run without statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoClickSink;

impl ClickSink for NoClickSink {
    fn record_click(&self, _cps: f32, _left_button: bool) {}
}

/// Time, randomness and cancellation for one body.
pub struct LoopRuntime {
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
    token: CancelToken,
    rng: parking_lot::Mutex<Box<dyn Rng>>,
    origin: Instant,
}

impl LoopRuntime {
    /// Wires a body to its seams. The taken `origin` is the zero for the
    /// millisecond counters the C# loops read from `Environment.TickCount64`.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>, sleeper: Arc<dyn Sleeper>, rng: Box<dyn Rng>) -> Self {
        let origin = clock.now();
        Self {
            clock,
            sleeper,
            token: CancelToken::new(),
            rng: parking_lot::Mutex::new(rng),
            origin,
        }
    }

    /// Share the body's token with a handle or a test.
    #[must_use]
    pub fn token(&self) -> &CancelToken {
        &self.token
    }

    /// The clock this body runs on, so per-loop state that needs time (the click
    /// timing model) shares the same source.
    #[must_use]
    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.clock)
    }

    /// Current instant from the injected clock.
    #[must_use]
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// Milliseconds since the runtime was created, the analogue of
    /// `Environment.TickCount64` (`Clicker.cs:2682`).
    #[must_use]
    pub fn now_ms(&self) -> i64 {
        i64::try_from(
            self.now()
                .saturating_duration_since(self.origin)
                .as_millis(),
        )
        .unwrap_or(i64::MAX)
    }

    /// Whether a stop was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Sleeps for at least `duration`, in cancellation-checkable slices.
    ///
    /// Returns `false` when a stop was requested, including when it arrives midway
    /// through; the caller must then return without touching the world again.
    pub fn park(&self, duration: Duration) -> bool {
        let Some(deadline) = self.now().checked_add(duration) else {
            return !self.is_cancelled();
        };
        self.park_until(deadline)
    }

    /// Sleeps until an absolute deadline, in cancellation-checkable slices.
    pub fn park_until(&self, deadline: Instant) -> bool {
        loop {
            if self.is_cancelled() {
                return false;
            }
            let now = self.now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return true;
            };
            // Landing exactly on the deadline must stop the loop, not ask the
            // sleeper for a zero-length wait: `sleep_until(now)` returns at once and
            // the slice loop would spin.
            if remaining.is_zero() {
                return true;
            }
            let slice = remaining.min(CANCEL_SLICE);
            let Some(next) = now.checked_add(slice) else {
                return !self.is_cancelled();
            };
            self.sleeper.sleep_until(next);
        }
    }

    /// Runs `f` with the body's RNG. The lock is released before any sleep, so a
    /// paused body never blocks another.
    pub fn with_rng<T>(&self, f: impl FnOnce(&mut dyn Rng) -> T) -> T {
        let mut guard = self.rng.lock();
        f(&mut **guard)
    }

    /// `Random.Shared.NextDouble()`.
    pub fn uniform(&self) -> f64 {
        self.with_rng(|rng| rng.next_f64())
    }

    /// `Random.Shared.Next(lowInclusive, highExclusive)`.
    pub fn next_i32(&self, low_inclusive: i32, high_exclusive: i32) -> i32 {
        if high_exclusive <= low_inclusive {
            return low_inclusive;
        }
        let span = (high_exclusive - low_inclusive) as u32;
        self.with_rng(|rng| next_below(rng, span) as i32 + low_inclusive)
    }

    /// `GaussianRandom(mean, stddev)` from `Clicker.cs:3355-3362` (Box-Muller).
    pub fn gaussian(&self, mean: f32, stddev: f32) -> f32 {
        let (u1, u2) = self.with_rng(|rng| (1.0 - rng.next_f64(), 1.0 - rng.next_f64()));
        // `u1 == 0` would make `ln(0)` infinite; the managed draw can only be 0
        // through a sloppy `NextDouble` implementation.
        let u1 = u1.max(f64::MIN_POSITIVE);
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        (f64::from(mean) + z * f64::from(stddev)) as f32
    }
}

/// Every loop `Clicker.cs` owns, started together.
///
/// This is the assembly the managed host performed in `Clicker.StartAutomation`
/// (`Clicker.cs:470-500`): five bodies, one per thread — the left and right click
/// engines, aim assist, Pixel Party assist and the triggerbot. Owning this owns all
/// five; dropping it stops them.
pub struct ClickerLoops {
    handles: Vec<LoopHandle>,
}

/// One configuration set for [`ClickerLoops::start`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClickerLoopConfigs {
    /// Left click engine.
    pub left: click::ClickLoopConfig,
    /// Right click engine.
    pub right: click::ClickLoopConfig,
    /// Aim assist.
    pub aim: aim::AimLoopConfig,
    /// Pixel Party assist.
    pub pixel_party: pixel_party::PixelPartyConfig,
    /// Triggerbot.
    pub triggerbot: triggerbot::TriggerbotConfig,
}

impl ClickerLoopConfigs {
    /// The defaults: both click engines on `Medium`, everything else off.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            left: click::ClickLoopConfig::left(),
            right: click::ClickLoopConfig::right(),
            aim: aim::AimLoopConfig {
                enabled: false,
                params: crate::aim_assist::AimAssistParams {
                    width: 0,
                    height: 0,
                    game_fov: 70.0,
                    aim_assist_fov: 30.0,
                    range: 4.5,
                    strength: 50,
                },
                auto_left_clicking: false,
            },
            pixel_party: pixel_party::PixelPartyConfig::default(),
            triggerbot: triggerbot::TriggerbotConfig::default(),
        }
    }
}

/// Why one of the loops could not start. The loops that did start are stopped
/// before this is returned, so a partial start never leaks threads.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ClickerStartError(#[from] LoopSpawnError);

impl ClickerLoops {
    /// Starts all five bodies (two click engines and three assist loops).
    ///
    /// `make_runtime` is called once per loop so each body gets its own token and
    /// RNG; `seed` differentiates them.
    pub fn start(
        sense: Arc<dyn LoopSense>,
        input: Arc<dyn aoko_platform::InputBackend>,
        sink: Arc<dyn ClickSink>,
        configs: ClickerLoopConfigs,
        make_runtime: impl Fn(u64) -> LoopRuntime,
    ) -> Result<Self, ClickerStartError> {
        let mut handles: Vec<LoopHandle> = Vec::with_capacity(5);

        for (index, left_button) in [(0_u64, true), (1, false)] {
            let runtime = make_runtime(index);
            let clock = runtime.clock();
            let config = if left_button {
                configs.left
            } else {
                configs.right
            };
            let sense = Arc::clone(&sense);
            let input = Arc::clone(&input);
            let sink = Arc::clone(&sink);
            let started = LoopHandle::spawn(
                if left_button {
                    "click-left"
                } else {
                    "click-right"
                },
                runtime,
                move |runtime| {
                    let mut timing = crate::clicker::AdvancedClickTimingState::new(
                        clock,
                        Box::new(crate::clock::SplitMix64::new(0xC1 + index)),
                    );
                    click::run(
                        runtime,
                        sense.as_ref(),
                        input.as_ref(),
                        sink.as_ref(),
                        &config,
                        &mut timing,
                    );
                },
            );
            match started {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    for mut handle in handles {
                        handle.stop();
                    }
                    return Err(ClickerStartError(error));
                }
            }
        }

        let runtime = make_runtime(2);
        {
            let sense = Arc::clone(&sense);
            let input = Arc::clone(&input);
            let config = configs.aim;
            let started = LoopHandle::spawn("aim-assist", runtime, move |runtime| {
                let mut controller = crate::aim_assist::AimAssistMotionController::new();
                aim::run(
                    runtime,
                    sense.as_ref(),
                    input.as_ref(),
                    &config,
                    &mut controller,
                );
            });
            match started {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    for mut handle in handles {
                        handle.stop();
                    }
                    return Err(ClickerStartError(error));
                }
            }
        }

        let runtime = make_runtime(3);
        {
            let sense = Arc::clone(&sense);
            let input = Arc::clone(&input);
            let config = configs.pixel_party;
            let started = LoopHandle::spawn("pixel-party", runtime, move |runtime| {
                let mut keys = pixel_party::WalkKeys::default();
                pixel_party::run(runtime, sense.as_ref(), input.as_ref(), &config, &mut keys);
            });
            match started {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    for mut handle in handles {
                        handle.stop();
                    }
                    return Err(ClickerStartError(error));
                }
            }
        }

        let runtime = make_runtime(4);
        {
            let sense = Arc::clone(&sense);
            let input = Arc::clone(&input);
            let config = configs.triggerbot;
            let started = LoopHandle::spawn("triggerbot", runtime, move |runtime| {
                triggerbot::run(runtime, sense.as_ref(), input.as_ref(), &config);
            });
            match started {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    for mut handle in handles {
                        handle.stop();
                    }
                    return Err(ClickerStartError(error));
                }
            }
        }

        Ok(Self { handles })
    }

    /// How many bodies are running.
    #[must_use]
    pub fn running(&self) -> usize {
        self.handles
            .iter()
            .filter(|handle| handle.is_running())
            .count()
    }

    /// Registered names, in start order.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.handles.iter().map(LoopHandle::name).collect()
    }

    /// Cancels every body and waits for all of them.
    pub fn stop(&mut self) {
        for handle in &mut self.handles {
            handle.stop();
        }
    }
}

impl Drop for ClickerLoops {
    fn drop(&mut self) {
        self.stop();
    }
}

impl std::fmt::Debug for ClickerLoops {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickerLoops")
            .field("names", &self.names())
            .field("running", &self.running())
            .finish()
    }
}

impl std::fmt::Debug for LoopRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopRuntime")
            .field("cancelled", &self.is_cancelled())
            .field("now_ms", &self.now_ms())
            .finish()
    }
}

/// The thread could not be created.
#[derive(Debug, thiserror::Error)]
#[error("could not start the {name} loop: {source}")]
pub struct LoopSpawnError {
    name: String,
    #[source]
    source: std::io::Error,
}

impl LoopSpawnError {
    /// Name the loop was registered under.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A running loop body and its cancellation handle.
///
/// Owning this owns the thread: `stop` cancels, joins, and reports nothing (a body
/// that panics is a programming error, and the panic is contained by the thread
/// boundary). `Drop` stops, so an early return cannot leak a loop.
pub struct LoopHandle {
    name: String,
    token: CancelToken,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LoopHandle {
    /// Starts `body` on its own thread.
    ///
    /// The body receives the same [`LoopRuntime`] the handle holds the token of, so
    /// a test can cancel through either path.
    pub fn spawn<F>(
        name: impl Into<String>,
        runtime: LoopRuntime,
        body: F,
    ) -> Result<Self, LoopSpawnError>
    where
        F: FnOnce(&LoopRuntime) + Send + 'static,
    {
        let name = name.into();
        let token = runtime.token().clone();
        let thread_name = format!("aoko-{name}");
        let thread = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || body(&runtime))
            .map_err(|source| LoopSpawnError {
                name: name.clone(),
                source,
            })?;
        Ok(Self {
            name,
            token,
            thread: Some(thread),
        })
    }

    /// The loop's cancellation token.
    #[must_use]
    pub fn token(&self) -> &CancelToken {
        &self.token
    }

    /// Registered name, for logs and test messages.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the thread is still owned, i.e. [`Self::stop`] has not run.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.thread.is_some()
    }

    /// Cancels and joins. Idempotent, and safe to call from any thread except the
    /// loop's own.
    pub fn stop(&mut self) {
        self.token.cancel();
        if let Some(thread) = self.thread.take() {
            // A panic in the body already surfaced on its own thread; joining only
            // waits for the thread to finish.
            let _ = thread.join();
        }
    }
}

impl Drop for LoopHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

impl std::fmt::Debug for LoopHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopHandle")
            .field("name", &self.name)
            .field("running", &self.is_running())
            .field("cancelled", &self.token.is_cancelled())
            .finish()
    }
}
