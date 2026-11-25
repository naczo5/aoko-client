//! The triggerbot executor.
//!
//! Port of `Clicker.TriggerbotLoop(CancellationToken)` (`Aoko/Core/Clicker.cs:2837-2974`),
//! spawned by `Task.Run` at `:2820`.
//!
//! Delays, in evaluation order:
//!
//! | Condition | Delay |
//! | --- | --- |
//! | not enabled / unsupported / disconnected / inactive / cursor visible | 25 ms |
//! | state older than [`TRIGGERBOT_STATE_FRESH_MS`] | 8 ms |
//! | GUI open *and* OS cursor visible | 20 ms |
//! | break-blocks owns a block target | 12 ms |
//! | `TriggerbotRequireClick` and the button is up | 8 ms |
//! | no valid target | 8 ms |
//! | end of tick | 4 ms |
//!
//! The stale check runs *before* the GUI check here, unlike the aim-assist loop.
//! That ordering is preserved because it decides whether a stale GUI state can
//! hold a target.
//!
//! The cooldown model is four nested gates (`:2930-2966`): server cooldown,
//! predicted cooldown at the reaction deadline, a minimum interval derived from the
//! tick length, and a hard cap of one full swing. All four must be ready.

use std::time::Duration;

use aoko_platform::{InputBackend, MouseButton};
use aoko_protocol::state::{EntityInfo, GameState};

use super::{LoopRuntime, LoopSense};
use crate::clicker::guards::normalize_cooldown_per_tick;

/// `TriggerbotStateFreshMs` (`Clicker.cs:16`).
pub const TRIGGERBOT_STATE_FRESH_MS: u64 = 35;
/// Not-runnable poll (`:2856`).
pub const IDLE_POLL: Duration = Duration::from_millis(25);
/// Stale-state poll (`:2877`).
pub const STALE_POLL: Duration = Duration::from_millis(8);
/// GUI poll (`:2883`).
pub const GUI_POLL: Duration = Duration::from_millis(20);
/// Break-blocks poll (`:2891`).
pub const BREAK_BLOCKS_POLL: Duration = Duration::from_millis(12);
/// Require-click and no-target poll (`:2897`, `:2915`).
pub const NO_TARGET_POLL: Duration = Duration::from_millis(8);
/// Steady-state tick (`:2972`).
pub const TICK_POLL: Duration = Duration::from_millis(4);

/// Crosshair radius for the screen-space entity fallback (`Clicker.cs:3041`).
pub const CROSSHAIR_FALLBACK_RADIUS: f64 = 24.0;
/// Attack radius for `CanAttackTargetNow` (`:3001`).
pub const ATTACK_RADIUS: f64 = 30.0;
/// Maximum attack range in blocks (`:3013`).
pub const MAX_ATTACK_RANGE: f64 = 3.8;

/// Settings the loop reads each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriggerbotConfig {
    /// `TriggerbotEnabled`.
    pub enabled: bool,
    /// `BreakBlocksEnabled`, shared with the click loop.
    pub break_blocks_enabled: bool,
    /// `TriggerbotRequireClick`.
    pub require_click: bool,
    /// `TriggerbotOnlyCrosshair`.
    pub only_crosshair: bool,
    /// `TriggerbotOnlyIfCanAttack`.
    pub only_if_can_attack: bool,
    /// `TriggerbotCooldownThreshold`, percent.
    pub cooldown_threshold: i32,
    /// `TriggerbotHitChance`, percent.
    pub hit_chance: i32,
}

impl Default for TriggerbotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            break_blocks_enabled: false,
            require_click: true,
            only_crosshair: true,
            only_if_can_attack: true,
            cooldown_threshold: 90,
            hit_chance: 100,
        }
    }
}

/// `IsCrosshairEntityFallback` (`Clicker.cs:2978-3010`).
///
/// A bridge that does not report `lookingAtEntity` still gives screen positions, so
/// a living entity inside a small radius of the crosshair counts as "on target".
#[must_use]
pub fn is_crosshair_entity_fallback(entities: &[EntityInfo], width: i32, height: i32) -> bool {
    if entities.is_empty() || width <= 0 || height <= 0 {
        return false;
    }

    let cx = f64::from(width) * 0.5;
    let cy = f64::from(height) * 0.5;
    let radius_sq = CROSSHAIR_FALLBACK_RADIUS * CROSSHAIR_FALLBACK_RADIUS;

    for entity in entities {
        if entity.hp <= 0.1 {
            continue;
        }
        if entity.sx < 0.0
            || entity.sx > width as f32
            || entity.sy < 0.0
            || entity.sy > height as f32
        {
            continue;
        }
        let dx = f64::from(entity.sx) - cx;
        let dy = f64::from(entity.sy) - cy;
        if dx * dx + dy * dy <= radius_sq {
            return true;
        }
    }

    false
}

/// `CanAttackTargetNow` (`Clicker.cs:2986-3025`).
#[must_use]
pub fn can_attack_target_now(
    entities: &[EntityInfo],
    width: i32,
    height: i32,
    looking_at_entity: bool,
    looking_at_entity_latched: bool,
) -> bool {
    if entities.is_empty() {
        return false;
    }
    if looking_at_entity || looking_at_entity_latched {
        return true;
    }
    if !is_crosshair_entity_fallback(entities, width, height) {
        return false;
    }
    if width <= 0 || height <= 0 {
        return false;
    }

    let cx = f64::from(width) * 0.5;
    let cy = f64::from(height) * 0.5;
    let radius_sq = ATTACK_RADIUS * ATTACK_RADIUS;

    let mut best_dist = f64::MAX;
    let mut found = false;

    for entity in entities {
        if entity.hp <= 0.1 {
            continue;
        }
        if entity.dist <= 0.01 || entity.dist > MAX_ATTACK_RANGE {
            continue;
        }
        if entity.sx < 0.0
            || entity.sx > width as f32
            || entity.sy < 0.0
            || entity.sy > height as f32
        {
            continue;
        }

        let dx = f64::from(entity.sx) - cx;
        let dy = f64::from(entity.sy) - cy;
        if dx * dx + dy * dy > radius_sq {
            continue;
        }

        found = true;
        if entity.dist < best_dist {
            best_dist = entity.dist;
        }
    }

    found && best_dist <= MAX_ATTACK_RANGE
}

/// Per-loop timing state, mirroring the three locals in the managed method
/// (`Clicker.cs:2839-2841`).
#[derive(Debug, Default)]
struct Timing {
    had_target: bool,
    target_ready_at: i64,
    last_click_at: i64,
}

/// Runs the triggerbot loop until cancelled.
pub fn run(
    runtime: &LoopRuntime,
    sense: &dyn LoopSense,
    input: &dyn InputBackend,
    config: &TriggerbotConfig,
) {
    let mut timing = Timing::default();

    while !runtime.is_cancelled() {
        let should_run = config.enabled
            && sense.supports_module("triggerbot")
            && sense.is_connected()
            && sense.minecraft_active()
            && !sense.cursor_visible();

        if !should_run {
            timing.had_target = false;
            if !runtime.park(IDLE_POLL) {
                break;
            }
            continue;
        }

        let now = runtime.now();
        let now_ms = runtime.now_ms();

        // Stale before GUI, unlike the aim loop (`:2865-2871`).
        let age = sense.state_age_ms(now);
        let stale = age.is_none_or(|age| age > TRIGGERBOT_STATE_FRESH_MS);
        if stale {
            timing.had_target = false;
            if !runtime.park(STALE_POLL) {
                break;
            }
            continue;
        }

        let Some(snapshot) = sense.snapshot() else {
            timing.had_target = false;
            if !runtime.park(STALE_POLL) {
                break;
            }
            continue;
        };
        let state = &snapshot.state;

        if state.gui_open && sense.cursor_visible() {
            timing.had_target = false;
            if !runtime.park(GUI_POLL) {
                break;
            }
            continue;
        }

        if config.break_blocks_enabled
            && (state.looking_at_block || state.breaking_block)
            && !state.looking_at_entity
            && !state.looking_at_entity_latched
        {
            timing.had_target = false;
            if !runtime.park(BREAK_BLOCKS_POLL) {
                break;
            }
            continue;
        }

        // `InputHooks.IsPhysicalLeftButtonDown` (`Clicker.cs:2897-2898`). The
        // trigger only fires on proof that the user is holding the button, so an
        // unobservable session (`None`: Wayland plus uinput, or no `/dev/input`
        // access) must not be read as "not held" and must not be read as "held"
        // either: it parks, exactly like a proven "not held", but for the recorded
        // reason. C# only had the always-answering `GetAsyncKeyState`, so the
        // unknown state is a new-platform decision rather than a parity difference.
        // The UI explains the inactivity from
        // `CapabilityReport::pointer_state_sampling` - the flag that agrees with
        // `probe_mouse_state`, not `input_observation`, which only says events are
        // delivered. Do not rewrite this as `unwrap_or(false)`.
        let left_held = input.probe_mouse_state().map(|mouse| mouse.left_down);
        if config.require_click && left_held != Some(true) {
            timing.had_target = false;
            if !runtime.park(NO_TARGET_POLL) {
                break;
            }
            continue;
        }

        let (width, height) = viewport(sense, state);
        let crosshair_on_entity = state.looking_at_entity
            || state.looking_at_entity_latched
            || is_crosshair_entity_fallback(&state.entities, width, height);
        let can_attack = can_attack_target_now(
            &state.entities,
            width,
            height,
            state.looking_at_entity,
            state.looking_at_entity_latched,
        );

        let has_target = (!config.only_crosshair || crosshair_on_entity)
            && (!config.only_if_can_attack || can_attack);

        if !has_target {
            timing.had_target = false;
            if !runtime.park(NO_TARGET_POLL) {
                break;
            }
            continue;
        }

        if !timing.had_target {
            timing.had_target = true;
            // Reaction time: 45..=220 ms, shorter with a lower cooldown threshold
            // (`:2918-2926`).
            let cooldown_threshold = config.cooldown_threshold as f32 / 100.0;
            let threshold_factor = 1.10 - cooldown_threshold.clamp(0.10, 1.00);
            let base_interval_ms = 40.0 + (threshold_factor * 110.0);
            let reaction_factor = 0.7 + (runtime.uniform() as f32 * 1.1);
            let reaction_ms = (base_interval_ms * reaction_factor).clamp(45.0, 220.0) as i64;
            timing.target_ready_at = now_ms + reaction_ms;
        }

        let server_cooldown = state.attack_cooldown.clamp(0.0, 1.0);
        let cooldown_per_tick = normalize_cooldown_per_tick(state.attack_cooldown_per_tick);
        let local_cooldown = if timing.last_click_at == 0 {
            1.0
        } else {
            (((now_ms - timing.last_click_at) as f32 / 50.0) * cooldown_per_tick).clamp(0.0, 1.0)
        };

        let mut predicted_cooldown = server_cooldown.min(local_cooldown);
        if timing.target_ready_at > now_ms {
            let ms_ahead = (timing.target_ready_at - now_ms) as f64;
            let local_ahead =
                (local_cooldown + (ms_ahead / 50.0) as f32 * cooldown_per_tick).clamp(0.0, 1.0);
            predicted_cooldown = server_cooldown.min(local_ahead);
        }

        let threshold = config.cooldown_threshold as f32 / 100.0;
        let cooldown_ready = server_cooldown >= threshold && predicted_cooldown >= threshold;
        let per_tick = cooldown_per_tick.max(0.01);
        let min_interval_ms = ((threshold / per_tick) * 50.0).ceil() as i64;
        let interval_ready =
            timing.last_click_at == 0 || (now_ms - timing.last_click_at) >= min_interval_ms;
        let full_cooldown_ms = (50.0 / per_tick).ceil() as i64;
        let hard_cap_ready =
            timing.last_click_at == 0 || (now_ms - timing.last_click_at) >= full_cooldown_ms;

        if cooldown_ready && interval_ready && hard_cap_ready && now_ms >= timing.target_ready_at {
            let hit_chance = config.hit_chance.clamp(0, 100);
            if runtime.next_i32(1, 101) <= hit_chance {
                // The managed triggerbot injects without a statistics sample: only
                // the two click loops call `StatsTracker.RecordClick`.
                if let Err(error) = input.click(MouseButton::Left) {
                    tracing::debug!(%error, "triggerbot click failed");
                }
                timing.last_click_at = now_ms;
                let mut post_click_delay = runtime.next_i32(18, 46);
                if runtime.uniform() < 0.10 {
                    post_click_delay += runtime.next_i32(20, 65);
                }
                timing.target_ready_at = now_ms + i64::from(post_click_delay);
            } else {
                timing.target_ready_at = now_ms + i64::from(runtime.next_i32(18, 34));
            }
        }

        if !runtime.park(TICK_POLL) {
            break;
        }
    }
}

/// Viewport size, falling back to the window size the sense reports.
fn viewport(sense: &dyn LoopSense, state: &GameState) -> (i32, i32) {
    if state.viewport_width > 0 && state.viewport_height > 0 {
        return (state.viewport_width, state.viewport_height);
    }
    sense.framebuffer_size().unwrap_or((0, 0))
}
