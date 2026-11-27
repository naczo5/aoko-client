//! Human-like inter-click delay generator.
//!
//! Port of `Aoko/Core/AdvancedClickTimingState.cs`, formula for formula. The C#
//! model is: engagement burst on start, periodic target-CPS drift, fatigue
//! accumulation on every click with recovery while idle, butterfly-click doublets,
//! a muscle-memory repeat chance, and micro-hesitation pauses.
//!
//! Constants are the C# private consts (`AdvancedClickTimingState.cs:13-16`):
//!
//! | Constant | Value |
//! | --- | --- |
//! | `MaxFatigue` | `0.25` |
//! | `FatiguePerClick` | `0.012` |
//! | `FatigueRecoveryPerSecond` | `0.35` |
//! | `IdleThresholdSeconds` | `0.8` |
//!
//! Every random draw keeps the *order* of the C# statements, so a seeded
//! [`Rng`] reproduces a run exactly. That is what
//! `ClickerRandomizationTests` could not do: it calls
//! `CalculateNextDelay(min, max)` with no timestamp and therefore reads
//! `Stopwatch.GetTimestamp()` and `Random.Shared`
//! (`AdvancedClickTimingState.cs:57`, `:77`). Here the timestamp is the injected
//! clock and the generator is the injected RNG, so the same two tests can assert
//! exact values instead of loose ranges.
//!
//! Documented deviation: a simulated timestamp that runs *backwards* is treated as
//! zero elapsed time instead of negative elapsed time. Both readings skip the idle
//! reset; negative elapsed would additionally subtract a negative fatigue recovery,
//! i.e. raise fatigue on a clock glitch. A monotonic [`Instant`] cannot go
//! backwards, so the branch only exists for a caller-supplied timestamp.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::clock::{next_below, Clock, Rng};

/// Up to 25% drop-off during continuous sustained clicking.
const MAX_FATIGUE: f64 = 0.25;
/// Fatigue added by one click.
const FATIGUE_PER_CLICK: f64 = 0.012;
/// Fatigue recovered per idle second.
const FATIGUE_RECOVERY_PER_SECOND: f64 = 0.35;
/// Gap after which a click counts as re-engaging.
const IDLE_THRESHOLD_SECONDS: f64 = 0.8;
/// Gap after which fatigue is dropped outright instead of decaying.
const FATIGUE_RESET_SECONDS: f64 = 2.0;
/// Floor applied to the very first click of a run.
const MIN_DELAY_MS: f64 = 10.0;

/// Converts a delay produced by [`AdvancedClickTimingState`] into a [`Duration`],
/// clamped to at least one millisecond.
///
/// `Duration::from_secs_f64` panics on a negative or non-finite value, so the
/// conversion goes through the checked constructor and falls back to 1 ms.
#[must_use]
pub fn delay_to_interval(delay_ms: f64) -> Duration {
    let seconds = delay_ms / 1000.0;
    Duration::try_from_secs_f64(seconds)
        .ok()
        .filter(|interval| !interval.is_zero())
        .unwrap_or(Duration::from_millis(1))
}

/// Stateful delay generator for one mouse button.
pub struct AdvancedClickTimingState {
    clock: Arc<dyn Clock>,
    rng: Box<dyn Rng>,
    last_click: Option<Instant>,
    previous_delay_ms: f64,
    current_cps: f64,
    target_cps: f64,
    next_drift: Option<Instant>,
    fatigue: f64,
    burst_clicks_remaining: u32,
    is_butterfly_second_click: bool,
    consecutive_butterfly_clicks: u32,
}

impl AdvancedClickTimingState {
    /// Fresh state, equivalent to `new AdvancedClickTimingState()`.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>, rng: Box<dyn Rng>) -> Self {
        Self {
            clock,
            rng,
            last_click: None,
            previous_delay_ms: 0.0,
            current_cps: 0.0,
            target_cps: 0.0,
            next_drift: None,
            fatigue: 0.0,
            burst_clicks_remaining: 0,
            is_butterfly_second_click: false,
            consecutive_butterfly_clicks: 0,
        }
    }

    /// `CalculateNextDelay(minCps, maxCps)` using the injected clock.
    pub fn calculate_next_delay(&mut self, min_cps: f32, max_cps: f32) -> f64 {
        let now = self.clock.now();
        self.calculate_next_delay_at(min_cps, max_cps, now)
    }

    /// `CalculateNextDelay(minCps, maxCps, simulatedTimestamp)`: the same maths
    /// against a caller-supplied timestamp, which is what makes randomness,
    /// fatigue and burst behaviour reproducible in a test.
    pub fn calculate_next_delay_at(&mut self, min_cps: f32, max_cps: f32, now: Instant) -> f64 {
        // `f64::clamp` panics on a NaN bound, and the C# `Math.Clamp` would
        // silently return NaN. Neither is acceptable, so sanitise at the door.
        let min_cps = if min_cps.is_finite() { min_cps } else { 1.0 };
        let max_cps = if max_cps.is_finite() { max_cps } else { 1.0 };

        // `Math.Max(1.0f, Math.Min(minCps, maxCps))` then
        // `Math.Max(effectiveMin, maxCps)`, both in f32 (`:54-55`).
        let effective_min = min_cps.min(max_cps).max(1.0);
        let effective_max = max_cps.max(effective_min);
        let effective_min_f64 = f64::from(effective_min);
        let effective_max_f64 = f64::from(effective_max);

        // `double mid = (effectiveMin + effectiveMax) * 0.5;`: the sum happens in
        // f32, then widens (`:82-83`).
        let mid = f64::from(effective_min + effective_max) * 0.5;
        let half_span = (f64::from(effective_max - effective_min) * 0.5).max(0.5);

        let elapsed_seconds = match self.last_click {
            None => f64::INFINITY,
            Some(last) => now
                .checked_duration_since(last)
                .map_or(0.0, |elapsed| elapsed.as_secs_f64()),
        };

        if self.last_click.is_none() || elapsed_seconds >= IDLE_THRESHOLD_SECONDS {
            if elapsed_seconds.is_finite() {
                let recovery = elapsed_seconds * FATIGUE_RECOVERY_PER_SECOND;
                self.fatigue = (self.fatigue - recovery).max(0.0);
                if elapsed_seconds >= FATIGUE_RESET_SECONDS {
                    self.fatigue = 0.0;
                }
            } else {
                self.fatigue = 0.0;
            }

            // Engagement rush: 2 to 5 rapid burst clicks (`Next(2, 6)`, `:77`).
            self.burst_clicks_remaining = 2 + next_below(&mut *self.rng, 4);
            self.is_butterfly_second_click = false;
            self.consecutive_butterfly_clicks = 0;

            self.target_cps = clamp_cps(
                mid + sample_gaussian(&mut *self.rng) * (half_span * 0.4),
                effective_min_f64,
                effective_max_f64,
            );
            self.current_cps = self.target_cps;
            self.next_drift = Some(now + duration_from_seconds(1.5 + self.rng.next_f64() * 2.0));
        }

        // Periodic target-CPS drift (`:89-100`).
        if self.next_drift.is_none_or(|next| now >= next) {
            self.target_cps = clamp_cps(
                mid + (self.rng.next_f64() * 2.0 - 1.0) * (half_span * 0.7),
                effective_min_f64,
                effective_max_f64,
            );

            let drift = sample_gaussian(&mut *self.rng) * 0.4;
            self.current_cps = clamp_cps(
                self.current_cps + 0.3 * (self.target_cps - self.current_cps) + drift,
                effective_min_f64,
                effective_max_f64,
            );

            self.next_drift = Some(now + duration_from_seconds(1.2 + self.rng.next_f64() * 1.8));
        }

        let mut effective_cps = self.current_cps;

        // Initial burst rush bonus (`:104-108`).
        if self.burst_clicks_remaining > 0 {
            effective_cps *= 1.0 + (0.05 + self.rng.next_f64() * 0.07);
            self.burst_clicks_remaining -= 1;
        }

        // Stamina drop-off (`:111-112`).
        effective_cps *= 1.0 - self.fatigue;

        // Allow the dip below min CPS during sustained fatigue, but never a value
        // above max (`:115-116`).
        let lower_bound = (f64::from(effective_min) * 0.85).max(1.0);
        let upper_bound = lower_bound.max(effective_max_f64);
        effective_cps = effective_cps.clamp(lower_bound, upper_bound);

        let base_delay_ms = 1000.0 / effective_cps;
        let mut delay_ms = base_delay_ms;

        // Butterfly doublets (`:122-148`). The rhythm-break dice are rolled on
        // every call, in the condition, exactly as the C# `if` does.
        let rhythm_break_after = 8 + next_below(&mut *self.rng, 8);
        if self.consecutive_butterfly_clicks > rhythm_break_after {
            self.consecutive_butterfly_clicks = 0;
            self.is_butterfly_second_click = false;
        } else {
            self.consecutive_butterfly_clicks += 1;
            if self.is_butterfly_second_click {
                // Finger 1 -> finger 2: rapid tap.
                delay_ms = base_delay_ms * (0.72 + self.rng.next_f64() * 0.12);
                self.is_butterfly_second_click = false;
            } else {
                // Finger 2 -> finger 1: recovery reset. The pair averages 1.0x.
                delay_ms = base_delay_ms * (1.16 + self.rng.next_f64() * 0.12);
                self.is_butterfly_second_click = true;
            }
        }

        // Debounce / muscle-memory repeat, ~6% (`:150-154`). The draw is
        // short-circuited while no previous delay exists, as in C#.
        if self.previous_delay_ms > 0.0 && self.rng.next_f64() < 0.06 {
            delay_ms = self.previous_delay_ms + (self.rng.next_f64() * 4.0 - 2.0);
        }

        // Micro-hesitation, scaling with fatigue (`:156-160`).
        let pause_chance = 0.035 + 0.18 * self.fatigue;
        if self.rng.next_f64() < pause_chance {
            delay_ms += 35.0 + self.rng.next_f64() * 50.0;
        }

        // Accumulation happens after the recovery above, which is why a single
        // click following a long idle leaves exactly one increment behind.
        self.fatigue = (self.fatigue + FATIGUE_PER_CLICK).min(MAX_FATIGUE);

        delay_ms += sample_gaussian(&mut *self.rng) * 2.0;
        let delay_ms = delay_ms.max(MIN_DELAY_MS);

        self.last_click = Some(now);
        self.previous_delay_ms = delay_ms;
        delay_ms
    }

    /// Clears every accumulated behaviour, like `AdvancedClickTimingState.Reset`.
    pub fn reset(&mut self) {
        self.last_click = None;
        self.previous_delay_ms = 0.0;
        self.current_cps = 0.0;
        self.target_cps = 0.0;
        self.next_drift = None;
        self.fatigue = 0.0;
        self.burst_clicks_remaining = 0;
        self.is_butterfly_second_click = false;
        self.consecutive_butterfly_clicks = 0;
    }

    /// Current stamina drop-off, `0.0..=0.25`.
    #[must_use]
    pub fn fatigue(&self) -> f64 {
        self.fatigue
    }

    /// Burst clicks still owed by the engagement rush.
    #[must_use]
    pub fn burst_clicks_remaining(&self) -> u32 {
        self.burst_clicks_remaining
    }

    /// Whether the last generated delay was the second tap of a doublet, i.e.
    /// whether the next one is the shorter half.
    #[must_use]
    pub fn is_butterfly_second_click(&self) -> bool {
        self.is_butterfly_second_click
    }
}

impl std::fmt::Debug for AdvancedClickTimingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdvancedClickTimingState")
            .field("fatigue", &self.fatigue)
            .field("current_cps", &self.current_cps)
            .field("target_cps", &self.target_cps)
            .field("burst_clicks_remaining", &self.burst_clicks_remaining)
            .field("is_butterfly_second_click", &self.is_butterfly_second_click)
            .finish_non_exhaustive()
    }
}

/// `Math.Clamp(value, min, max)` without the NaN panic.
///
/// The bounds are already ordered by the caller (`effective_min <= effective_max`),
/// so this only has to be total, not defensive.
fn clamp_cps(value: f64, low: f64, high: f64) -> f64 {
    if value.is_nan() {
        return low;
    }
    value.max(low).min(high)
}

/// Box-Muller, exactly as `SampleGaussian` (`AdvancedClickTimingState.cs:175-180`):
/// `sqrt(-2 ln u1) * cos(2 pi u2)` with `u1` floored away from zero.
fn sample_gaussian(rng: &mut dyn Rng) -> f64 {
    let u1 = rng.next_f64().max(1e-7);
    let u2 = rng.next_f64();
    (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
}

/// `(long)(x * Stopwatch.Frequency)` ticks, in nanoseconds: truncating toward
/// zero, saturating instead of wrapping.
fn duration_from_seconds(seconds: f64) -> Duration {
    let nanos = (seconds * 1_000_000_000.0).max(0.0);
    // A saturating cast truncates toward zero, matching the C# `(long)` cast.
    Duration::from_nanos(nanos as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SplitMix64;
    use crate::testing::VirtualClock;

    fn state(seed: u64) -> AdvancedClickTimingState {
        AdvancedClickTimingState::new(
            Arc::new(VirtualClock::new()),
            Box::new(SplitMix64::new(seed)),
        )
    }

    /// `ClickerRandomizationTests.AdvancedClickTimingState_GeneratesPositiveDelaysWithinBounds`.
    #[test]
    fn generated_delays_stay_positive_and_bounded() {
        let mut timing = state(0x5EED);
        for _ in 0..50 {
            let delay_ms = timing.calculate_next_delay(10.0, 14.0);
            assert!(
                delay_ms >= MIN_DELAY_MS,
                "delay {delay_ms} ms is below the floor"
            );
            assert!(delay_ms < 300.0, "delay {delay_ms} ms is out of bounds");
        }
    }

    /// `ClickerRandomizationTests.AdvancedClickTimingState_DemonstratesFatigueAccumulationAndIdleRecovery`.
    #[test]
    fn fatigue_accumulates_while_clicking_and_recovers_while_idle() {
        let mut timing = state(11);
        let base = Instant::now();
        let step = Duration::from_millis(70);

        assert!(
            timing.fatigue().abs() < f64::EPSILON,
            "fresh state is rested"
        );

        let mut now = base;
        for _ in 0..25 {
            now += step;
            timing.calculate_next_delay_at(12.0, 16.0, now);
        }
        assert!(
            timing.fatigue() > 0.1,
            "fatigue should have accumulated, was {}",
            timing.fatigue()
        );

        // 2.5 s idle: the reset branch clears fatigue, then the click itself adds
        // one increment back.
        now += Duration::from_millis(2_500);
        timing.calculate_next_delay_at(12.0, 16.0, now);
        assert!(
            timing.fatigue() <= 0.02,
            "fatigue should have recovered after idle, was {}",
            timing.fatigue()
        );
    }

    /// `ClickerRandomizationTests.AdvancedClickTimingState_DemonstratesButterflyDoubletAlternation`.
    #[test]
    fn the_butterfly_flag_alternates_between_clicks() {
        let mut timing = state(23);
        let base = Instant::now();

        timing.calculate_next_delay_at(10.0, 14.0, base);
        let first = timing.is_butterfly_second_click();

        timing.calculate_next_delay_at(10.0, 14.0, base + Duration::from_millis(70));
        let second = timing.is_butterfly_second_click();

        assert_ne!(first, second, "the doublet must alternate");
    }

    #[test]
    fn the_same_seed_reproduces_the_same_delays() {
        let base = Instant::now();
        let mut left = state(0xC0FFEE);
        let mut right = state(0xC0FFEE);

        for click in 0..64_u32 {
            let at = base + Duration::from_millis(u64::from(click) * 70);
            assert_eq!(
                left.calculate_next_delay_at(9.0, 13.0, at),
                right.calculate_next_delay_at(9.0, 13.0, at)
            );
        }
    }

    #[test]
    fn a_fresh_engagement_owes_a_burst_of_two_to_five_clicks() {
        for seed in 0..64 {
            // 64 seeds is enough to cover every burst length at least once.
            let mut timing = state(seed);
            timing.calculate_next_delay_at(10.0, 14.0, Instant::now());
            // The current click consumed one of them.
            assert!((1..=4).contains(&timing.burst_clicks_remaining()));
        }
    }

    #[test]
    fn reset_clears_every_accumulated_behaviour() {
        let mut timing = state(31);
        let base = Instant::now();
        for click in 0..20_u32 {
            timing.calculate_next_delay_at(
                12.0,
                16.0,
                base + Duration::from_millis(70 * u64::from(click)),
            );
        }
        assert!(timing.fatigue() > 0.0);

        timing.reset();
        assert!(timing.fatigue().abs() < f64::EPSILON);
        assert_eq!(timing.burst_clicks_remaining(), 0);
        assert!(!timing.is_butterfly_second_click());

        // A reset generator re-engages exactly like a fresh one: a new burst is
        // owed and the click that starts it leaves one fatigue increment behind.
        let delay_ms = timing.calculate_next_delay_at(10.0, 14.0, base + Duration::from_secs(10));
        assert!((1..=4).contains(&timing.burst_clicks_remaining()));
        assert!((timing.fatigue() - FATIGUE_PER_CLICK).abs() < f64::EPSILON);
        assert!(delay_ms >= MIN_DELAY_MS);
    }

    #[test]
    fn an_invalid_cps_window_is_clamped_instead_of_panicking() {
        let mut timing = state(5);
        let at = Instant::now();

        // min > max, NaN and a zero window must all produce a usable delay.
        for (min_cps, max_cps) in [
            (20.0_f32, 5.0_f32),
            (f32::NAN, 10.0),
            (10.0, f32::NAN),
            (0.0, 0.0),
        ] {
            let delay_ms = timing.calculate_next_delay_at(min_cps, max_cps, at);
            assert!(
                delay_ms.is_finite(),
                "{min_cps}/{max_cps} produced {delay_ms}"
            );
            assert!(delay_ms >= MIN_DELAY_MS);
        }
    }

    #[test]
    fn delay_to_interval_never_panics_or_returns_zero() {
        assert_eq!(delay_to_interval(83.25), Duration::from_micros(83_250));
        assert_eq!(delay_to_interval(0.0), Duration::from_millis(1));
        assert_eq!(delay_to_interval(-5.0), Duration::from_millis(1));
        assert_eq!(delay_to_interval(f64::NAN), Duration::from_millis(1));
        assert_eq!(delay_to_interval(f64::INFINITY), Duration::from_millis(1));
    }
}
