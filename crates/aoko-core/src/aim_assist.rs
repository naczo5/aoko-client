//! Aim-assist target selection and bounded relative-move generation.
//!
//! Port of `Aoko/Core/AimAssistMotionController.cs`. Read what the C# file
//! actually does before reading the name: it is **not** a turn-speed /
//! acceleration / deceleration / overshoot controller. It picks the nearest
//! eligible entity, converts its screen-space offset into a relative mouse move
//! scaled by a one-pole IIR filter and a strength curve, and clamps the step. There
//! is no velocity integration and no state beyond the filter, so there is no
//! overshoot or noise term to port. Anything else would be invented behaviour.
//!
//! The formulas, with their sources:
//!
//! | Step | Formula | Source |
//! | --- | --- | --- |
//! | Safe game FOV | valid in `10..=170`, otherwise `70` | `:49` |
//! | Focal length | `height / (2 * tan(fov * PI / 360))`, floored at `1` | `:50-51` |
//! | Angular radius | `atan2(hypot(dx, dy), focal) * 180 / PI` | `:71-73` |
//! | Eligibility | `0.01 < dist <= range`, on-screen, `angle <= fov/2` | `:65-75` |
//! | Filter | `filtered += (dx - filtered) * 0.40` | `:19`, `:102-103` |
//! | Strength | `0.08 + 0.36 * n^2`, `n = clamp(strength, 0, 100) / 100` | `:105-106` |
//! | Move | `round(filtered * strength)`, non-zero push, clamp to `±round(4 + 16n)` | `:107-121` |
//!
//! `Math.Round` in C# defaults to midpoint-to-even and the port keeps that, because
//! the movement tests compare exact integers.
//!
//! Two port decisions:
//!
//! - The C# reads `EntityInfo` (`Aoko/Core/GameState.cs:206`), which is a wire type
//!   owned by `aoko-protocol`. This module takes a borrowed [`AimEntity`] view
//!   instead, so the hot path never clones a string and this crate can be built
//!   before the protocol crate exists. Wave 2 adapts `EntityInfo` into it.
//! - The C# compares the *rendered* target label (`name`, or `#index` for an
//!   unnamed entity) to decide whether the filter must be cleared on a target
//!   handoff. That comparison is kept exactly, including the quirk that a named
//!   entity called `#3` is indistinguishable from the fourth unnamed entity, but it
//!   is done against a stack buffer so the steady state allocates nothing.
//!
//! One deviation, on an input the shipped app cannot produce: a `NaN` range is
//! treated as `0` (`f64::max` keeps the non-NaN operand) instead of propagating.
//! `Math.Max(NaN, 0)` returns `NaN` in C#, which makes every entity pass the range
//! filter. `Clicker.AimAssistRange` only ever holds `> 0 && <= 12`, so the two
//! readings are indistinguishable in practice, and the Rust one fails closed.

/// IIR smoothing factor applied to the target offset (`AimAssistMotionController.cs:19`).
const FILTER_ALPHA: f64 = 0.40;
/// Game FOV used when the reported one is out of range (`:49`).
const DEFAULT_GAME_FOV: f32 = 70.0;
/// Lower bound of a usable reported game FOV (`:49`).
const MIN_GAME_FOV: f32 = 10.0;
/// Upper bound of a usable reported game FOV (`:49`).
const MAX_GAME_FOV: f32 = 170.0;
/// Floor for the derived focal length, so a degenerate viewport cannot divide by
/// zero (`:51`).
const MIN_FOCAL_LENGTH_PX: f64 = 1.0;
/// Smallest distance that still counts as a target (`:65`).
const MIN_TARGET_DIST: f64 = 0.01;
/// Upper bound of the configured aim-assist FOV, halved into a threshold (`:55`).
const MAX_AIM_ASSIST_FOV: f32 = 180.0;

use aoko_protocol::state::EntityInfo;

/// One candidate entity, borrowed from whatever state snapshot the caller holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimEntity<'a> {
    /// Display name; empty means the entity is identified by its index.
    pub name: &'a str,
    /// Screen-space X in viewport pixels.
    pub sx: f32,
    /// Screen-space Y in viewport pixels.
    pub sy: f32,
    /// Distance in blocks.
    pub dist: f64,
}

impl<'a> AimEntity<'a> {
    /// Builds a candidate.
    #[must_use]
    pub const fn new(name: &'a str, sx: f32, sy: f32, dist: f64) -> Self {
        Self { name, sx, sy, dist }
    }
}

/// Per-tick inputs, in the order the C# `Update` takes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimAssistParams {
    /// Viewport width in pixels.
    pub width: i32,
    /// Viewport height in pixels.
    pub height: i32,
    /// Game FOV as reported by the bridge.
    pub game_fov: f32,
    /// Configured aim-assist FOV cone in degrees.
    pub aim_assist_fov: f32,
    /// Configured maximum distance in blocks.
    pub range: f32,
    /// Configured strength, `0..=100`.
    pub strength: i32,
}

/// Which entity the move was aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AimTargetRef<'a> {
    /// The entity had a name.
    Named(&'a str),
    /// The entity was unnamed and is identified by its index, rendered `#index`.
    Indexed(usize),
}

/// Result of one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AimAssistMotion<'a> {
    /// A target was selected.
    pub has_target: bool,
    /// The crosshair was already on an entity, so movement is suppressed.
    pub suppressed: bool,
    /// Relative mouse X to inject.
    pub move_x: i32,
    /// Relative mouse Y to inject.
    pub move_y: i32,
    /// The selected target, when [`Self::has_target`] is set.
    pub target: Option<AimTargetRef<'a>>,
}

/// Stateful aim-assist controller. One per session, driven at the state cadence.
#[derive(Debug, Default)]
pub struct AimAssistMotionController {
    filtered_dx: f64,
    filtered_dy: f64,
    target_name: Option<Box<str>>,
}

impl AimAssistMotionController {
    /// A controller with no latched target.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects a target and produces the next relative move.
    ///
    /// `looking_at_entity` is the live crosshair state. The C# signature also has a
    /// `LookingAtEntityLatched` on the state object, but it is never read here;
    /// `AimAssistMotionControllerTests.LatchedStateAlone_DoesNotSuppressImmediateReacquisition`
    /// pins that.
    pub fn update<'e>(
        &mut self,
        entities: &'e [AimEntity<'e>],
        params: &AimAssistParams,
        looking_at_entity: bool,
    ) -> AimAssistMotion<'e> {
        self.update_over(
            params,
            looking_at_entity,
            entities
                .iter()
                .enumerate()
                .map(|(index, entity)| (index, entity.name, entity.sx, entity.sy, entity.dist)),
        )
    }

    /// Same selection, fed straight from the bridge's `EntityInfo` list.
    ///
    /// The input loop runs at 125 Hz, so it must not build an adapter vector every
    /// tick; both entry points funnel into one monomorphised core instead.
    pub fn update_from_state<'e>(
        &mut self,
        entities: &'e [EntityInfo],
        params: &AimAssistParams,
        looking_at_entity: bool,
    ) -> AimAssistMotion<'e> {
        self.update_over(
            params,
            looking_at_entity,
            entities.iter().enumerate().map(|(index, entity)| {
                (
                    index,
                    entity.name.as_str(),
                    entity.sx,
                    entity.sy,
                    entity.dist,
                )
            }),
        )
    }

    /// The selection and filtering core shared by [`Self::update`] and
    /// [`Self::update_from_state`].
    ///
    /// The iterator yields `(index, name, screen_x, screen_y, distance)` so neither
    /// caller has to materialise a candidate list.
    fn update_over<'e, I>(
        &mut self,
        params: &AimAssistParams,
        looking_at_entity: bool,
        candidates: I,
    ) -> AimAssistMotion<'e>
    where
        I: Iterator<Item = (usize, &'e str, f32, f32, f64)>,
    {
        if looking_at_entity {
            self.reset();
            return AimAssistMotion {
                suppressed: true,
                ..AimAssistMotion::default()
            };
        }

        if params.width <= 0 || params.height <= 0 {
            self.reset();
            return AimAssistMotion::default();
        }

        let safe_game_fov = if (MIN_GAME_FOV..=MAX_GAME_FOV).contains(&params.game_fov) {
            f64::from(params.game_fov)
        } else {
            f64::from(DEFAULT_GAME_FOV)
        };

        let focal_length_px = (f64::from(params.height)
            / (2.0 * (safe_game_fov * std::f64::consts::PI / 360.0).tan()))
        .max(MIN_FOCAL_LENGTH_PX);

        let width = f64::from(params.width);
        let height = f64::from(params.height);
        let center_x = width * 0.5;
        let center_y = height * 0.5;
        let max_angle = f64::from(params.aim_assist_fov.clamp(1.0, MAX_AIM_ASSIST_FOV)) * 0.5;
        let range = f64::from(params.range).max(0.0);

        // `double bestScore = double.MaxValue;` with `if (score < bestScore)`
        // (`AimAssistMotionController.cs:57,78`). The sentinel matters: a candidate
        // whose score is NaN compares false against everything, so it is skipped
        // instead of becoming "the best" by being first. An `Option` with
        // `is_none_or` would accept it.
        let mut best_score = f64::MAX;
        let mut best: Option<BestTarget<'e>> = None;
        for (index, name, sx, sy, dist) in candidates {
            if dist <= MIN_TARGET_DIST || dist > range {
                continue;
            }
            if sx < 0.0 || sx > params.width as f32 || sy < 0.0 || sy > params.height as f32 {
                continue;
            }

            let dx = f64::from(sx) - center_x;
            let dy = f64::from(sy) - center_y;
            let radial = (dx * dx + dy * dy).sqrt();
            let angle = radial.atan2(focal_length_px) * 180.0 / std::f64::consts::PI;
            if angle > max_angle {
                continue;
            }

            let score = dx * dx + dy * dy;
            if score < best_score {
                best_score = score;
                best = Some(BestTarget {
                    dx,
                    dy,
                    index,
                    // `string.IsNullOrEmpty(entity.Name) ? $"#{index}" : entity.Name` (`:83-85`)
                    name: if name.is_empty() { None } else { Some(name) },
                });
            }
        }

        let Some(best) = best else {
            self.reset();
            return AimAssistMotion::default();
        };

        // The C# compares the rendered label, so an unnamed entity is `#index`
        // (`AimAssistMotionController.cs:95-100`).
        // Formatting into a stack buffer keeps this allocation-free; the boxed copy
        // is only replaced when the target actually changes.
        let index_label = IndexLabel::new(best.index);
        let candidate = best.name.unwrap_or_else(|| index_label.as_str());
        if self.target_name.as_deref() != Some(candidate) {
            self.filtered_dx = 0.0;
            self.filtered_dy = 0.0;
            self.target_name = Some(Box::from(candidate));
        }

        self.filtered_dx += (best.dx - self.filtered_dx) * FILTER_ALPHA;
        self.filtered_dy += (best.dy - self.filtered_dy) * FILTER_ALPHA;

        let strength_norm = f64::from(params.strength.clamp(0, 100)) / 100.0;
        let strength = 0.08 + 0.36 * strength_norm * strength_norm;
        let mut move_x = round_to_i32(self.filtered_dx * strength);
        let mut move_y = round_to_i32(self.filtered_dy * strength);

        // A sub-pixel offset must still produce a step once the target is more
        // than a pixel off centre, otherwise the crosshair sticks short of it.
        if move_x == 0 && best.dx.abs() > 1.0 {
            move_x = sign(best.dx);
        }
        if move_y == 0 && best.dy.abs() > 1.0 {
            move_y = sign(best.dy);
        }

        let max_step = round_to_i32(4.0 + 16.0 * strength_norm);
        let max_step = max_step.max(0);

        AimAssistMotion {
            has_target: true,
            suppressed: false,
            move_x: move_x.clamp(-max_step, max_step),
            move_y: move_y.clamp(-max_step, max_step),
            target: Some(match best.name {
                Some(name) => AimTargetRef::Named(name),
                None => AimTargetRef::Indexed(best.index),
            }),
        }
    }

    /// Drops the latched target and the filter state.
    pub fn reset(&mut self) {
        self.filtered_dx = 0.0;
        self.filtered_dy = 0.0;
        self.target_name = None;
    }

    /// Whether a target is currently latched. Diagnostics only.
    #[must_use]
    pub fn has_latched_target(&self) -> bool {
        self.target_name.is_some()
    }
}

/// Best candidate seen so far in one tick.
#[derive(Clone, Copy)]
struct BestTarget<'e> {
    dx: f64,
    dy: f64,
    index: usize,
    name: Option<&'e str>,
}

/// `(int)Math.Round(value)`: midpoint-to-even, saturating instead of wrapping.
fn round_to_i32(value: f64) -> i32 {
    value.round_ties_even() as i32
}

/// `Math.Sign(value)` as an `i32`.
fn sign(value: f64) -> i32 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

/// `#<index>` in a stack buffer, so the per-tick label comparison never allocates.
struct IndexLabel {
    buffer: [u8; 24],
    len: usize,
}

impl IndexLabel {
    fn new(index: usize) -> Self {
        let mut buffer = [0_u8; 24];
        buffer[0] = b'#';

        // Render the decimal digits in reverse, then copy them in order.
        let mut digits = [0_u8; 20];
        let mut count = 0;
        let mut value = index;
        loop {
            digits[count] = b'0' + (value % 10) as u8;
            count += 1;
            value /= 10;
            if value == 0 {
                break;
            }
        }

        let mut len = 1;
        while count > 0 {
            count -= 1;
            buffer[len] = digits[count];
            len += 1;
        }

        Self { buffer, len }
    }

    fn as_str(&self) -> &str {
        // Only ASCII digits and `#` are ever written, so this cannot fail; the
        // fallback keeps the function total.
        std::str::from_utf8(&self.buffer[..self.len]).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_WIDTH: i32 = 1920;
    const DEFAULT_HEIGHT: i32 = 1080;

    fn params() -> AimAssistParams {
        AimAssistParams {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            game_fov: 70.0,
            aim_assist_fov: 30.0,
            range: 4.5,
            strength: 40,
        }
    }

    fn entity(name: &str, sx: f32, sy: f32, dist: f64) -> AimEntity<'_> {
        AimEntity::new(name, sx, sy, dist)
    }

    /// `AimAssistMotionControllerTests.EquivalentTelemetry_ProducesEquivalentMovementWithoutVersionInput`.
    #[test]
    fn equivalent_telemetry_produces_equivalent_movement() {
        let entities = [entity("Player", 1030.0, 520.0, 3.0)];
        let mut legacy = AimAssistMotionController::new();
        let mut modern = AimAssistMotionController::new();

        let legacy_move = legacy.update(&entities, &params(), false);
        let modern_move = modern.update(&entities, &params(), false);

        assert_eq!(legacy_move, modern_move);
        assert!(legacy_move.has_target);
    }

    /// `AimAssistMotionControllerTests.Strength_AlwaysProducesBoundedModernBaselineMovement`.
    #[test]
    fn strength_always_produces_bounded_movement() {
        let entities = [entity("Player", 1400.0, 800.0, 3.0)];

        for strength in [10, 50, 100] {
            let mut controller = AimAssistMotionController::new();
            let mut tick = params();
            tick.aim_assist_fov = 120.0;
            tick.strength = strength;

            let movement = controller.update(&entities, &tick, false);
            let expected_max = round_to_i32(4.0 + 16.0 * f64::from(strength) / 100.0);

            assert!(movement.has_target);
            assert!(movement.move_x.abs() <= expected_max);
            assert!(movement.move_y.abs() <= expected_max);
        }
    }

    /// `AimAssistMotionControllerTests.ReportedGameFov_ControlsAngularEligibility`.
    #[test]
    fn reported_game_fov_controls_angular_eligibility() {
        let entities = [entity("Player", 1180.0, 540.0, 3.0)];

        for (game_fov, expected) in [(30.0_f32, true), (70.0, false), (110.0, false)] {
            let mut controller = AimAssistMotionController::new();
            let mut tick = params();
            tick.game_fov = game_fov;
            tick.aim_assist_fov = 30.0;

            assert_eq!(
                controller.update(&entities, &tick, false).has_target,
                expected,
                "game FOV {game_fov}"
            );
        }
    }

    /// `AimAssistMotionControllerTests.InvalidGameFov_UsesSafeDefault`.
    #[test]
    fn invalid_game_fov_uses_the_safe_default() {
        let entities = [entity("Player", 1040.0, 540.0, 3.0)];

        let mut invalid_controller = AimAssistMotionController::new();
        let mut invalid = params();
        invalid.game_fov = 0.0;
        let invalid_move = invalid_controller.update(&entities, &invalid, false);

        let mut fallback_controller = AimAssistMotionController::new();
        let mut fallback = params();
        fallback.game_fov = 70.0;
        let fallback_move = fallback_controller.update(&entities, &fallback, false);

        assert_eq!(fallback_move, invalid_move);
    }

    /// `AimAssistMotionControllerTests.ResolutionScaling_PreservesEquivalentAngularTargeting`.
    #[test]
    fn resolution_scaling_preserves_angular_targeting() {
        let full_hd_entities = [entity("Player", 1060.0, 540.0, 3.0)];
        let qhd_entities = [entity("Player", 1413.333, 720.0, 3.0)];

        let mut full_hd_controller = AimAssistMotionController::new();
        let mut full_hd_params = params();
        full_hd_params.aim_assist_fov = 20.0;
        let full_hd = full_hd_controller.update(&full_hd_entities, &full_hd_params, false);

        let mut qhd_controller = AimAssistMotionController::new();
        let mut qhd_params = params();
        qhd_params.width = 2560;
        qhd_params.height = 1440;
        qhd_params.aim_assist_fov = 20.0;
        let qhd = qhd_controller.update(&qhd_entities, &qhd_params, false);

        assert!(full_hd.has_target);
        assert!(qhd.has_target);
        assert_eq!(sign(f64::from(full_hd.move_x)), sign(f64::from(qhd.move_x)));
    }

    /// `AimAssistMotionControllerTests.RangeAndScreenBounds_ExcludeInvalidTargets`.
    #[test]
    fn range_and_screen_bounds_exclude_invalid_targets() {
        let entities = [
            entity("TooFar", 970.0, 540.0, 8.0),
            entity("Offscreen", -1.0, 540.0, 3.0),
        ];
        let mut controller = AimAssistMotionController::new();

        let movement = controller.update(&entities, &params(), false);

        assert!(!movement.has_target);
        assert_eq!(movement.move_x, 0);
        assert_eq!(movement.move_y, 0);
    }

    /// `AimAssistMotionControllerTests.LookingAtEntity_SuppressesAndClearsResidualMovement`.
    #[test]
    fn looking_at_entity_suppresses_and_clears_residual_movement() {
        let entities = [entity("Player", 1080.0, 540.0, 3.0)];
        let mut controller = AimAssistMotionController::new();
        let _ = controller.update(&entities, &params(), false);
        let _ = controller.update(&entities, &params(), false);

        let suppressed = controller.update(&entities, &params(), true);
        let reacquired = controller.update(&entities, &params(), false);
        let fresh = AimAssistMotionController::new().update(&entities, &params(), false);

        assert!(suppressed.suppressed);
        assert!(!suppressed.has_target);
        assert_eq!(suppressed.move_x, 0);
        assert_eq!(suppressed.move_y, 0);
        assert_eq!(fresh, reacquired);
    }

    /// `AimAssistMotionControllerTests.LatchedStateAlone_DoesNotSuppressImmediateReacquisition`.
    #[test]
    fn a_latched_crosshair_does_not_suppress_reacquisition() {
        // `lookingAtEntity: false` with a latched state on the snapshot: the
        // controller reads only the live flag, so a reacquisition is immediate.
        let entities = [entity("Player", 1080.0, 540.0, 3.0)];
        let mut controller = AimAssistMotionController::new();

        let movement = controller.update(&entities, &params(), false);

        assert!(!movement.suppressed);
        assert!(movement.has_target);
        assert_ne!(movement.move_x, 0);
    }

    /// `AimAssistMotionControllerTests.TargetHandoff_DoesNotCarryPreviousDirection`.
    #[test]
    fn target_handoff_does_not_carry_the_previous_direction() {
        let first = [entity("Alpha", 1160.0, 540.0, 3.0)];
        let second = [entity("Beta", 900.0, 540.0, 3.0)];
        let mut controller = AimAssistMotionController::new();
        let _ = controller.update(&first, &params(), false);
        let _ = controller.update(&first, &params(), false);
        let _ = controller.update(&first, &params(), false);

        let switched = controller.update(&second, &params(), false);
        let fresh = AimAssistMotionController::new().update(&second, &params(), false);

        assert_eq!(switched.target, Some(AimTargetRef::Named("Beta")));
        assert_eq!(switched.move_x, fresh.move_x);
        assert!(switched.move_x < 0);
    }

    /// `AimAssistMotionControllerTests.SameMovingTarget_PreservesSmoothingDirection`.
    #[test]
    fn a_moving_target_keeps_the_smoothing_direction() {
        let first = [entity("Player", 1080.0, 540.0, 3.0)];
        let moved = [entity("Player", 1100.0, 540.0, 3.0)];
        let mut controller = AimAssistMotionController::new();

        let first_move = controller.update(&first, &params(), false);
        let moved_move = controller.update(&moved, &params(), false);

        assert_eq!(moved_move.target, Some(AimTargetRef::Named("Player")));
        assert!(first_move.move_x > 0);
        assert!(moved_move.move_x >= first_move.move_x);
    }

    /// `AimAssistMotionControllerTests.NoEligibleTarget_ResetsTrackingState`.
    #[test]
    fn no_eligible_target_resets_the_tracking_state() {
        let target = [entity("Player", 1080.0, 540.0, 3.0)];
        let mut controller = AimAssistMotionController::new();
        let _ = controller.update(&target, &params(), false);
        let _ = controller.update(&target, &params(), false);

        let none = controller.update(&[], &params(), false);
        let reacquired = controller.update(&target, &params(), false);
        let fresh = AimAssistMotionController::new().update(&target, &params(), false);

        assert!(!none.has_target);
        assert_eq!(fresh, reacquired);
    }

    #[test]
    fn an_unnamed_target_is_identified_by_its_index() {
        let entities = [
            entity("", 1080.0, 540.0, 3.0),
            entity("", 500.0, 540.0, 3.0),
        ];
        let mut controller = AimAssistMotionController::new();

        let movement = controller.update(&entities, &params(), false);

        assert!(movement.has_target);
        assert_eq!(movement.target, Some(AimTargetRef::Indexed(0)));
        assert!(controller.has_latched_target());
    }

    #[test]
    fn a_target_change_clears_the_filter_but_a_stable_target_does_not() {
        // 100 px off centre, so the strength clamp does not saturate the first
        // step and the ramp is observable.
        let alpha = [entity("Alpha", 1060.0, 540.0, 3.0)];
        let beta = [entity("Beta", 1060.0, 540.0, 3.0)];
        let mut controller = AimAssistMotionController::new();

        // Same geometry, different name: the filter restarts, so the first move is
        // the same as a cold start.
        let first = controller.update(&alpha, &params(), false);
        let second = controller.update(&alpha, &params(), false);
        assert!(
            second.move_x > first.move_x,
            "a stable target keeps ramping"
        );

        let switched = controller.update(&beta, &params(), false);
        assert_eq!(switched.move_x, first.move_x);
    }

    #[test]
    fn a_reset_controller_behaves_like_a_fresh_one() {
        let entities = [entity("Player", 1080.0, 540.0, 3.0)];
        let mut controller = AimAssistMotionController::new();
        let _ = controller.update(&entities, &params(), false);
        let _ = controller.update(&entities, &params(), false);

        controller.reset();
        assert!(!controller.has_latched_target());

        let after_reset = controller.update(&entities, &params(), false);
        let fresh = AimAssistMotionController::new().update(&entities, &params(), false);
        assert_eq!(after_reset, fresh);
    }

    #[test]
    fn a_degenerate_viewport_is_rejected_without_panicking() {
        let entities = [entity("Player", 0.0, 0.0, 3.0)];
        let mut controller = AimAssistMotionController::new();

        for (width, height) in [(0, 1080), (1920, 0), (-5, -5)] {
            let mut tick = params();
            tick.width = width;
            tick.height = height;
            let movement = controller.update(&entities, &tick, false);
            assert!(!movement.has_target);
            assert_eq!(movement.move_x, 0);
        }
    }

    /// A NaN-scored candidate must be skipped, not selected: the managed code
    /// compares against `double.MaxValue` (`AimAssistMotionController.cs:57,78`),
    /// and every NaN comparison is false.
    #[test]
    fn a_nan_candidate_never_wins_over_a_real_one() {
        let entities = [
            entity("nan", f32::NAN, 540.0, 3.0),
            entity("second", 1030.0, 540.0, 3.0),
        ];
        let mut controller = AimAssistMotionController::new();
        let movement = controller.update(&entities, &params(), false);

        assert!(movement.has_target);
        assert_eq!(movement.target, Some(AimTargetRef::Named("second")));
        assert_ne!(movement.move_x, 0, "the real target is still aimed at");
    }

    /// With only the NaN candidate every candidate is rejected, so the tick reports
    /// no target instead of emitting a NaN-derived move.
    #[test]
    fn a_nan_only_candidate_yields_no_target() {
        let entities = [entity("nan", f32::NAN, f32::NAN, 3.0)];
        let mut controller = AimAssistMotionController::new();
        let movement = controller.update(&entities, &params(), false);

        assert!(!movement.has_target);
        assert_eq!(movement.target, None);
        assert_eq!(movement.move_x, 0);
        assert_eq!(movement.move_y, 0);
        assert!(!controller.has_latched_target());
    }

    #[test]
    fn the_index_label_formats_without_allocating_digits_wrong() {
        for index in [0_usize, 7, 42, 8_675_309, usize::MAX] {
            assert_eq!(IndexLabel::new(index).as_str(), format!("#{index}"));
        }
    }
}
