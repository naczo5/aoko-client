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
//!   OS cursor visibility, which bridge modules are supported), and the two things the bodies
//!   must re-read every tick rather than capture: the module switch that owns the body
//!   ([`LoopSense::module_enabled`]) and the settings it runs with
//!   ([`LoopSense::loop_configs`]).
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
//!
//! ## Module switches
//!
//! A body runs only while its own module's switch is on. The managed client never needed a
//! gate inside a body because each module's arm path owned one: `Clicker.StartClicking`
//! (`Clicker.cs:462-486`) and `StopClicking` (`:488-509`) for the two click engines,
//! `AimAssistEnabled`'s setter (`:773-781`), `SyncPixelPartyAssistInputLoop` (`:626-636`),
//! the triggerbot's arm path (`:2201`), `ChestStealerEnabled` (`:1076-1090`) and
//! `RefillEnabled` (`:1174-1188`) for the rest. The port starts one bundle of seven
//! bodies, so the switch is read from [`LoopSense::module_enabled`] on every tick instead of
//! being frozen where the bundle was started; a disabled body parks at [`MODULE_OFF_POLL`].

pub mod aim;
pub mod chest_stealer;
pub mod click;
pub mod pixel_party;
pub mod refill;
pub mod triggerbot;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::{Duration, Instant};

use aoko_protocol::profile::Profile;

use crate::app::StateSnapshot;
use crate::chest_stealer::ScreenRect;
use crate::clock::{next_below, Clock, Rng, Sleeper};
use crate::concurrency::CancelToken;

/// Longest single sleep a body takes while it has a cancellation check pending.
///
/// This bounds stop latency, not scheduling accuracy: every wait still targets an
/// absolute deadline, so splitting one wait into several does not move the deadline.
/// Every poll interval the loops use (4, 8, 10, 12, 16, 20, 25 ms) fits in one
/// slice, so only the 100 ms guard polls are split, into four.
pub const CANCEL_SLICE: Duration = Duration::from_millis(25);

/// Catalog ids of the seven modules that own a loop body.
///
/// Each body asks [`LoopSense::module_enabled`] about exactly one of these, so the id has to
/// be the [`aoko_protocol::module_catalog`] id the switch set is keyed by — the production
/// sense resolves it through `module_catalog::flag_for_id` and an id that does not resolve
/// reads as off. Spelling each once means a body and the app's lookup cannot disagree about
/// which module keeps it running.
pub const AUTOCLICKER_MODULE: &str = "autoclicker";
/// `rightclick`.
pub const RIGHT_CLICK_MODULE: &str = "rightclick";
/// `aimassist`.
pub const AIM_ASSIST_MODULE: &str = "aimassist";
/// `pixelpartyassist`.
pub const PIXEL_PARTY_MODULE: &str = "pixelpartyassist";
/// `triggerbot`.
pub const TRIGGERBOT_MODULE: &str = "triggerbot";
/// `cheststealer`.
pub const CHEST_STEALER_MODULE: &str = "cheststealer";
/// `refill`.
pub const REFILL_MODULE: &str = "refill";
/// `hitselect`, read by the click bodies as a guard rather than as the owner of a loop.
///
/// `Clicker.HitSelectEnabled` was a property the managed `ClickLoop` read inside its `while`
/// (`Clicker.cs:3218-3227`), so the switch was live by construction. Here the module card
/// writes `EnabledFlag::HitSelect` and nothing else, so the guard has to read the switch;
/// [`ClickerLoopConfigs::from_profile`] only sees a `Profile`, whose `hitSelectEnabled` field is
/// written on save and therefore lags the switch until a reload.
pub const HIT_SELECT_MODULE: &str = "hitselect";

/// Poll a body takes while its own module is switched off.
///
/// The managed host had no such interval: `Clicker.StopClicking` (`Clicker.cs:488-509`) and
/// the aim, Pixel Party, triggerbot, chest-stealer and refill arm paths cancelled the loop
/// instead, so a feature that was off owned no thread. The port keeps all seven bodies alive
/// under one supervisor, so an off module parks here; 100 ms is the clicker's slowest guard
/// poll (`Clicker.cs:3136`) and holds a disabled body to ten wake-ups a second.
pub const MODULE_OFF_POLL: Duration = Duration::from_millis(100);

/// Places the pointer at an absolute position in virtual-desktop pixels.
///
/// The chest stealer and refill both have to move the cursor onto a slot before pressing:
/// `SetCursorPos` in `ChestStealerController.ClickSlotAsync` (`ChestStealerController.cs:305`)
/// and `RefillController.ClickSlotAsync` (`RefillController.cs:247`). A relative move cannot do
/// that from an unknown cursor position, so this is `InputBackend::move_to` — the absolute
/// placement the platform backend offers — and it is the only place either body names it.
///
/// The `i32 -> f32` conversion is the signature's, not a precision choice: every client rect a
/// screen can present fits in an `f32`'s 24-bit mantissa exactly.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn place_cursor(
    input: &dyn aoko_platform::InputBackend,
    x: i32,
    y: i32,
) -> Result<(), aoko_platform::PlatformError> {
    input.move_to(x as f32, y as f32)
}

/// Reads the outside world for a loop body.
///
/// Everything that is not in [`aoko_protocol::state::GameState`] but that the C#
/// loops query directly — `WindowDetection.IsMinecraftActive`, `IsCursorVisible`,
/// `GameStateClient.SupportsModule`, and the clicker's own switches and settings — is here.
/// [`crate::app::StoreSense`] is the store-only implementation; the shipped path is
/// `aoko_app::runtime::ControllerSense`, which adds the controller's switch set and profile on
/// top of these. A test implements the trait over fixed values.
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

    /// The focused game window's client area, in virtual-desktop pixels.
    ///
    /// `WindowDetection.GetMinecraftClientRectOnScreen()` (`WindowDetection.cs:256-276`). The
    /// chest-stealer and refill bodies map a slot from the game's GUI-scaled coordinates onto
    /// the screen through this rect (`ChestStealerCoordinateMapper.cs:7-27`), so it has to be
    /// the **client** area and not [`Self::framebuffer_size`]'s outer window rect: decorations
    /// would move every click up by the height of the title bar, which on a chest GUI is a
    /// different slot.
    ///
    /// `None` when this sense cannot answer — no window surface, no focused game window, or an
    /// empty client area. A body that cannot map a slot must park: clicking at a guessed
    /// position is worse than not clicking at all.
    fn client_rect(&self) -> Option<ScreenRect> {
        None
    }

    /// The switch behind one module, read fresh: `Some(on)` when this sense reads the same
    /// switch set the UI writes, `None` when it models no switches at all.
    ///
    /// The managed host never had to ask. A loop's existence *was* its switch: `Disarm`
    /// cancelled the click stream (`Clicker.cs:459`), `StartClicking` refused to start one
    /// whose module was off (`:462-486`), and `AimAssistEnabled = true`, the Pixel Party
    /// sync and the triggerbot's arm path each started or cancelled their own loop
    /// (`:617-620`, `:626-636`, `:725-728`). The port runs one supervisor with five
    /// long-lived bodies, so the switch has to be read *inside* the body: a value captured
    /// when the bundle started is what let the left clicker keep clicking after its own module
    /// was switched off, and let aim assist start a click stream while the clicker was never on.
    ///
    /// `None` is "this sense models no switches" — a test fixture or a capture harness — and
    /// the body then runs as its settings say. It must not mean "off": that would make every
    /// body dead on a sense that never had switches, which is the worse failure.
    fn module_enabled(&self, _module: &str) -> Option<bool> {
        None
    }

    /// The clicker settings, read fresh on every tick.
    ///
    /// `Clicker`'s properties were read inside each managed `while`, so a CPS band, a guard or
    /// an aim parameter took effect on the very next click. The port's bodies take their
    /// settings from here rather than from the value `ClickerLoops::start` was handed, which was
    /// a snapshot: every clicker setting a user changed in the GUI was ignored by the engine
    /// until the whole bundle restarted.
    ///
    /// `None` keeps the settings the body was started with (tests, captures).
    fn loop_configs(&self) -> Option<ClickerLoopConfigs> {
        None
    }

    /// Which physical mouse buttons are down, sampled: `InputHooks.IsPhysicalLeftButtonDown`
    /// and its right-button twin (`Aoko/Core/InputHooks.cs:239`, `:541-542`).
    ///
    /// **This is what makes the clicker a hold-to-click stream**, which is the managed
    /// behaviour and the reason the two click engines exist at all: the mouse hook called
    /// `StartClicking(button)` on the press and `StopClicking()` on the release
    /// (`InputHooks.cs:565-585`, `:606-625`), so arming the module started nothing by itself.
    /// A body that clicks on its own switch alone clicks with nobody touching the mouse, which
    /// is the defect this method exists to close.
    ///
    /// `None` means this sense does not model physical input at all (test fixtures, the capture
    /// harness), and the bodies then keep the switch-only behaviour they had. It is not
    /// "nothing is held": a real sense always answers, and answers `MouseState::default()` when
    /// it cannot see the mouse — fail-closed, because a clicker without a hold signal is a
    /// clicker that fires while the user is typing.
    fn physical_mouse(&self) -> Option<aoko_platform::MouseState> {
        None
    }

    /// Age of the current snapshot at `now`.
    fn state_age_ms(&self, now: Instant) -> Option<u64> {
        self.snapshot()
            .map(|snapshot| now.saturating_duration_since(snapshot.at).as_millis() as u64)
    }

    /// The snapshot when it is younger than `max_age_ms`, else `None`.
    ///
    /// One atomic load, so the loop cannot observe a torn state/age pair.
    ///
    /// # `stateMs` versus receive time
    ///
    /// The managed loops measure age two ways (`Clicker.cs:2686-2693`): from the
    /// bridge's own `StateMs` when it is non-zero, else from
    /// `DateTime.Now - state.LastUpdate`. This crate uses the third equivalent:
    /// the [`Instant`] the ingest side stamped the snapshot with.
    ///
    /// That is deliberate, not an oversight. `StateMs` is `Environment.TickCount64`
    /// *on the bridge's machine*: it shares no epoch with this process, so using it
    /// would require a clock-offset estimate and would inherit any skew between the
    /// two. The receive instant is always within one frame of the send instant, is
    /// the same quantity the `LastUpdate` fallback measures, and — being a
    /// monotonic [`Instant`] — cannot be moved by a wall-clock adjustment. The
    /// difference is bounded by the IPC latency, which is far below every freshness
    /// threshold in use (35 ms for the triggerbot is the tightest).
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
/// (`Clicker.cs:470-500`) plus the two container controllers it owned beside the clicker
/// (`_chestStealerController`, `_refillController`, `Clicker.cs:521-522`): seven bodies, one
/// per thread — the left and right click engines, aim assist, Pixel Party assist, the
/// triggerbot, the chest stealer and refill. Owning this owns all seven; dropping it stops
/// them.
///
/// Started is not the same as running: each body also asks its own module's switch on every
/// tick (see the module docs), so a bundle can outlive the module that raised it without that
/// module's body acting.
pub struct ClickerLoops {
    handles: Vec<LoopHandle>,
}

/// One configuration set for the seven bodies, as [`LoopSense::loop_configs`] hands it over.
///
/// Each body picks its own field; the two click engines are separate because the managed
/// client kept two independent CPS bands and guard sets for them.
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
    /// Chest stealer.
    pub chest_stealer: chest_stealer::ChestStealerLoopConfig,
    /// Refill.
    pub refill: refill::RefillLoopConfig,
}

impl ClickerLoopConfigs {
    /// The defaults: both click engines on `Medium`, everything else off.
    ///
    /// Derived through [`Self::from_profile`] rather than restated, so the fallback a
    /// profile-less sense gets cannot drift from `Profile::default()`. It had drifted: the
    /// right engine started at `8/12` where the managed field initialisers are `10/14`
    /// (`Clicker.cs:128-129`), aim strength was `50` against `40` (`:755`) and the triggerbot's
    /// cooldown threshold was `90` against `92` (`:2554`).
    #[must_use]
    pub fn defaults() -> Self {
        Self::from_profile(&Profile::default(), false)
    }

    /// `Profile` -> the seven bodies' settings, clamped the way the managed setters clamped.
    ///
    /// `left_clicking` is `Clicker.IsUsingLeftButton` (`Clicker.cs:253`): the aim-assist hold
    /// guard runs when the left click engine is clicking (`:2704-2705`), and the port's
    /// equivalent of "the left engine is running" is the AutoClicker switch.
    ///
    /// Every clamp below is a managed property setter's, not an invention. `MinCPS`/`MaxCPS`
    /// accepted only `(0, 25]` and left a refused value at the field initialiser
    /// (`:257-293`, `:362-398`), so a hand-edited `maxCPS: 500` has to fall back to the default
    /// rather than become a 2 ms click interval. `AimAssistFov` clamps to `1..=180`,
    /// `AimAssistRange` to `1..=12`, `AimAssistStrength` to `1..=100` (`:787-828`), and
    /// `TriggerbotCooldownThreshold` / `TriggerbotHitChance` to `1..=100` (`:2555-2584`).
    ///
    /// The Chest Stealer menu check is the OR of the three sub-checks, which is the invariant
    /// the managed sub-check setters maintain — each one recomputed
    /// `_chestStealerMenuCheck = _chestStealerTitleCheck || _chestStealerCustomItemsCheck ||
    /// _chestStealerPhysicalCheck` (`:1135`, `:1150`, `:1165`). Those are the three rows the GUI
    /// paints, so the OR is what the user's checkboxes mean; the stored `menuCheck` boolean is
    /// not recomputed by this port's profile setters.
    #[must_use]
    pub fn from_profile(profile: &Profile, left_clicking: bool) -> Self {
        let mode = profile.randomization_mode();
        let (left_min, left_max) = cps_band(
            profile.min_cps,
            profile.max_cps,
            click::DEFAULT_MIN_CPS,
            click::DEFAULT_MAX_CPS,
        );
        let (right_min, right_max) = cps_band(
            profile.right_min_cps,
            profile.right_max_cps,
            click::DEFAULT_RIGHT_MIN_CPS,
            click::DEFAULT_RIGHT_MAX_CPS,
        );
        let chest_menu_check = profile.chest_stealer_menu_check()
            || profile.chest_stealer_title_check()
            || profile.chest_stealer_custom_items_check()
            || profile.chest_stealer_physical_check();

        // The managed menu and inventory guard was engine-agnostic: `ClickLoop` evaluated
        // `ClickInChests` / `ChestStealerMenuCheck` before it branched on `leftButton`
        // (`Clicker.cs:3130-3178`). Only the "only hold block", break-blocks and hit-select terms
        // are `leftButton`-gated (`:3183`, `:3195`, `:3218`), and `decide` already applies that
        // test. So both engines start from the same guard set, which is why the right engine used
        // to be blocked in every chest GUI with "Click in Chests" on.
        let guards = crate::clicker::tick::ClickConfig {
            click_in_chests: profile.click_in_chests,
            chest_stealer_menu_check: chest_menu_check,
            break_blocks_enabled: profile.break_blocks_enabled,
            hit_select_enabled: profile.hit_select_enabled,
            // Set per engine below; the left engine's guard never reads it.
            right_click_only_block: false,
            // Filled from the live link state inside the body.
            injected: false,
        };

        Self {
            left: click::ClickLoopConfig {
                left_button: true,
                guards,
                mode,
                min_cps: left_min,
                max_cps: left_max,
            },
            right: click::ClickLoopConfig {
                left_button: false,
                guards: crate::clicker::tick::ClickConfig {
                    right_click_only_block: profile.right_click_only_block,
                    ..guards
                },
                mode,
                min_cps: right_min,
                max_cps: right_max,
            },
            aim: aim::AimLoopConfig {
                params: crate::aim_assist::AimAssistParams {
                    // Both are overwritten from the state each tick; the state's viewport wins
                    // and `LoopSense::framebuffer_size` is the fallback.
                    width: 0,
                    height: 0,
                    game_fov: 70.0,
                    aim_assist_fov: profile.aim_assist_fov.clamp(1.0, 180.0),
                    range: profile.aim_assist_range.clamp(1.0, 12.0),
                    strength: profile.aim_assist_strength.clamp(1, 100),
                },
                auto_left_clicking: left_clicking,
            },
            pixel_party: pixel_party::PixelPartyConfig {
                auto_look: profile.pixel_party_auto_look_enabled,
                auto_walk: profile.pixel_party_auto_walk_enabled,
            },
            triggerbot: triggerbot::TriggerbotConfig {
                break_blocks_enabled: profile.break_blocks_enabled,
                require_click: profile.triggerbot_require_click,
                only_crosshair: profile.triggerbot_only_crosshair,
                only_if_can_attack: profile.triggerbot_only_if_can_attack,
                cooldown_threshold: profile.triggerbot_cooldown_threshold.clamp(1, 100),
                hit_chance: profile.triggerbot_hit_chance.clamp(1, 100),
            },
            chest_stealer: chest_stealer::ChestStealerLoopConfig {
                // `ChestStealerDelayMs`'s setter clamps to `50..=500` (`Clicker.cs:1093-1106`);
                // `next_interval_ms` clamps again, so an out-of-range profile value is harmless
                // either way, but the field is what the GUI shows.
                delay_ms: profile.chest_stealer_delay_ms.clamp(50, 500),
                // `clicker.ChestStealerMenuCheck` (`Clicker.cs:1108-1125`): the OR of the three
                // sub-checks, as above.
                menu_check: chest_menu_check,
            },
            refill: refill::RefillLoopConfig {
                // `RefillDelayMs`'s setter clamps to `50..=500` (`Clicker.cs:1191-1204`).
                delay_ms: crate::refill::normalize_delay_ms(profile.refill_delay_ms),
            },
        }
    }
}

/// The CPS band as `Clicker.MinCPS` then `Clicker.MaxCPS` would have applied it.
///
/// `Profile.ApplyToClicker` assigns the pair in that order (`Profile.cs:740-741`,
/// `:750-751`), and each setter accepted only `(0, 25]` while cross-clamping the other end.
/// So both steps are reproduced in sequence: a valid minimum raises a lower maximum, then a
/// valid maximum pulls a higher minimum down. Anything a setter would have refused leaves the
/// field at its initialiser.
fn cps_band(min: f32, max: f32, default_min: f32, default_max: f32) -> (f32, f32) {
    let accepted = |value: f32| value > 0.0 && value <= 25.0;
    let mut low = default_min;
    let mut high = default_max;

    if accepted(min) {
        low = min;
        if low > high {
            high = low;
        }
    }
    if accepted(max) {
        high = max;
        if high < low {
            low = high;
        }
    }

    (low, high)
}

/// Why one of the loops could not start. The loops that did start are stopped
/// before this is returned, so a partial start never leaks threads.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ClickerStartError(#[from] LoopSpawnError);

impl ClickerLoops {
    /// Starts all seven bodies: two click engines, three assist loops and the two container
    /// controllers.
    ///
    /// `make_runtime` is called once per loop so each body gets its own token and
    /// RNG; `seed` differentiates them.
    ///
    /// No settings are handed in. A body resolves its own on every tick from
    /// [`LoopSense::loop_configs`] and falls back to [`ClickerLoopConfigs::defaults`] for a
    /// sense with no profile behind it, which is the shape the managed loops had: they read
    /// `Clicker`'s properties inside their `while`, so an edit took effect on the next click.
    /// The set this function used to take was a snapshot, and every clicker setting a user
    /// changed in the GUI was ignored until the whole bundle restarted.
    ///
    /// Whether a body may act is not decided here either: it asks
    /// [`LoopSense::module_enabled`] about its own module every tick, and the two click engines
    /// also ask [`LoopSense::physical_mouse`] whether their button is held.
    pub fn start(
        sense: Arc<dyn LoopSense>,
        input: Arc<dyn aoko_platform::InputBackend>,
        sink: Arc<dyn ClickSink>,
        make_runtime: impl Fn(u64) -> LoopRuntime,
    ) -> Result<Self, ClickerStartError> {
        let mut handles: Vec<LoopHandle> = Vec::with_capacity(7);
        // The started settings, resolved once: a `Profile::default()` clone per body would be
        // seven allocations of the same strings for the same values.
        let fallback = ClickerLoopConfigs::defaults();
        // One cell for both click engines: `Clicker`'s `_activity`, so the button pressed first
        // keeps the mouse and the other engine's press is refused (`Clicker.cs:470`).
        let stream = Arc::new(crate::clicker::ClickStream::new());

        for (index, left_button) in [(0_u64, true), (1, false)] {
            let runtime = make_runtime(index);
            let clock = runtime.clock();
            let config = if left_button {
                fallback.left
            } else {
                fallback.right
            };
            let sense = Arc::clone(&sense);
            let input = Arc::clone(&input);
            let sink = Arc::clone(&sink);
            let stream = Arc::clone(&stream);
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
                    click::run_with_stream(
                        runtime,
                        sense.as_ref(),
                        input.as_ref(),
                        sink.as_ref(),
                        &config,
                        &mut timing,
                        Some(stream.as_ref()),
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
            let config = fallback.aim;
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
            let config = fallback.pixel_party;
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
            let config = fallback.triggerbot;
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

        let runtime = make_runtime(5);
        {
            let sense = Arc::clone(&sense);
            let input = Arc::clone(&input);
            let config = fallback.chest_stealer;
            let started = LoopHandle::spawn("chest-stealer", runtime, move |runtime| {
                chest_stealer::run(runtime, sense.as_ref(), input.as_ref(), &config);
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

        let runtime = make_runtime(6);
        {
            let sense = Arc::clone(&sense);
            let input = Arc::clone(&input);
            let config = fallback.refill;
            let started = LoopHandle::spawn("refill", runtime, move |runtime| {
                refill::run(runtime, sense.as_ref(), input.as_ref(), &config);
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

    /// How many thread handles this bundle still owns, whether or not their bodies
    /// are executing.
    #[must_use]
    pub fn owned(&self) -> usize {
        self.handles
            .iter()
            .filter(|handle| handle.owns_thread())
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
    /// Cleared the instant the body returns, so a body that finished on its own is
    /// not reported as running.
    live: Arc<std::sync::atomic::AtomicBool>,
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
        let live = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let body_live = Arc::clone(&live);
        let thread_name = format!("aoko-{name}");
        let thread = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                // A drop guard, so a body that panics is not reported as running
                // either.
                let _guard = LiveGuard(body_live);
                body(&runtime);
            })
            .map_err(|source| LoopSpawnError {
                name: name.clone(),
                source,
            })?;
        Ok(Self {
            name,
            token,
            thread: Some(thread),
            live,
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

    /// Whether the body is still executing.
    ///
    /// This is liveness, not ownership: a body that returned on its own (for
    /// example one whose loop condition was already false) reports `false` even
    /// though its thread handle is still held until [`Self::stop`].
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.live.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Whether this handle still owns a thread handle, i.e. [`Self::stop`] has not
    /// run yet.
    #[must_use]
    pub fn owns_thread(&self) -> bool {
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

/// Clears a liveness flag when the body leaves its scope.
struct LiveGuard(Arc<std::sync::atomic::AtomicBool>);

impl Drop for LiveGuard {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
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
