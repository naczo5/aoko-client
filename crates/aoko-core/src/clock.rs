//! Determinism seams: monotonic time, sleeping, and randomness.
//!
//! These three traits are the contract from `MIGRATION_ARCHITECTURE.md` section 3
//! ("`aoko-core` (determinism seams)"). Every timing-sensitive controller in this
//! crate takes its time and its randomness through them, never from
//! `Instant::now()` or a thread-local RNG, so that a test can drive a controller
//! through millions of simulated clicks in microseconds.
//!
//! Production wires [`SystemClock`], [`ThreadSleeper`] and [`EntropyRng`].
//! Tests wire `crate::testing::VirtualClock` (a clock that only moves when the
//! sleeper is asked to sleep) and [`SplitMix64`] with a fixed seed.
//!
//! The C# originals call `Stopwatch.GetTimestamp()` directly (for example
//! `Aoko/Core/AdvancedClickTimingState.cs:95`) and `Random.Shared`
//! (`Aoko/Core/AdvancedClickTimingState.cs:110`). Those calls are exactly what
//! makes `Aoko.Tests/ClickerRandomizationTests.cs` only able to assert loose
//! ranges; the seams below remove that limitation without changing the formulas.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Monotonic time source. Never a wall clock: every deadline in this crate is an
/// absolute [`Instant`], so clock adjustments cannot stretch or compress a
/// schedule.
pub trait Clock: Send + Sync {
    /// Current monotonic instant.
    fn now(&self) -> Instant;
}

/// Blocking sleep until an absolute deadline.
///
/// Taking an absolute deadline rather than a duration is what keeps sleep error
/// from accumulating across ticks: the caller re-derives the deadline from the
/// previous deadline, and any overshoot is compensated by the next shorter wait.
pub trait Sleeper: Send + Sync {
    /// Returns immediately when `deadline` is already in the past.
    fn sleep_until(&self, deadline: Instant);
}

/// Uniform randomness source.
pub trait Rng: Send {
    /// Uniform in `[0.0, 1.0)`, the equivalent of `Random.Shared.NextDouble()`.
    fn next_f64(&mut self) -> f64;

    /// Uniform in `[low.0, high.0)`. Returns `low` when `high <= low`; a
    /// degenerate range must not produce `NaN` or a value above `high`.
    fn next_range(&mut self, low: f64, high: f64) -> f64;
}

/// `Random.Shared.Next(bound)`: a uniform integer in `0..bound`.
///
/// Kept as a free function rather than a third trait method so the seam stays
/// exactly the three functions the architecture document fixes.
pub fn next_below(rng: &mut dyn Rng, bound: u32) -> u32 {
    if bound <= 1 {
        return 0;
    }
    // `next_range` is half-open, but an implementation is allowed to be sloppy
    // at the top of the range, so clamp instead of trusting it.
    let drawn = rng.next_range(0.0, f64::from(bound));
    (drawn.max(0.0) as u32).min(bound - 1)
}

/// `Stopwatch.GetTimestamp()`: the real monotonic clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Spin window used by [`ThreadSleeper`] after its coarse sleep.
///
/// This is the only place in the crate that burns CPU on purpose. The C# click
/// loop sleeps in whole milliseconds (`Task.Delay(schedule.DelayMilliseconds)`,
/// `Aoko/Core/Clicker.cs:3282`), so a coarse sleep alone already matches its
/// accuracy; the spin exists so that a future sub-millisecond deadline (for
/// example an aim-assist tick that has to land between two 60 Hz frames) can be
/// honoured without overshooting. It is deliberately small: at most 250 us of
/// spinning per call, never a term in a polling loop.
const SPIN_WINDOW: Duration = Duration::from_micros(250);

/// Sleeper for the automation thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep_until(&self, deadline: Instant) {
        loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return;
            };

            if remaining > SPIN_WINDOW {
                // Leave the spin window on the table; if the OS overshoots the
                // sleep past the deadline the next iteration returns.
                std::thread::sleep(remaining - SPIN_WINDOW);
            } else {
                while Instant::now() < deadline {
                    std::hint::spin_loop();
                }
                return;
            }
        }
    }
}

/// Inverse of 2^53, the scale used to turn the top 53 bits of a `u64` into a
/// double without bias.
const INV_2_POW_53: f64 = 1.0 / 9_007_199_254_740_992.0;

/// Seeded, deterministic, allocation-free RNG.
///
/// The original derives from `Random.Shared` and is therefore unseeded. Tests and
/// any reproducible replay need a seed, so production randomness lives in
/// [`EntropyRng`] and this type is the deterministic counterpart.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Fixed-seed stream. The same seed always yields the same sequence.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// One full 64-bit step.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl Rng for SplitMix64 {
    fn next_f64(&mut self) -> f64 {
        // 53 significant bits is the exact precision of an f64 mantissa, so this
        // is uniform on [0, 1) with no rejection loop.
        (self.next_u64() >> 11) as f64 * INV_2_POW_53
    }

    fn next_range(&mut self, low: f64, high: f64) -> f64 {
        if high > low {
            low + (high - low) * self.next_f64()
        } else {
            low
        }
    }
}

/// Production randomness: `rand`'s thread-local generator.
#[derive(Debug, Clone, Copy, Default)]
pub struct EntropyRng;

impl Rng for EntropyRng {
    fn next_f64(&mut self) -> f64 {
        rand::random::<f64>()
    }

    fn next_range(&mut self, low: f64, high: f64) -> f64 {
        if high > low {
            low + (high - low) * rand::random::<f64>()
        } else {
            low
        }
    }
}

/// A clock and a sleeper handed to one controller together.
///
/// Controllers take a single `Timing` instead of two `Arc`s so that a caller
/// cannot wire a real clock to a virtual sleeper by accident.
#[derive(Clone)]
pub struct Timing {
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
}

impl Timing {
    /// Wire an explicit clock/sleeper pair.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>, sleeper: Arc<dyn Sleeper>) -> Self {
        Self { clock, sleeper }
    }

    /// [`SystemClock`] plus [`ThreadSleeper`].
    #[must_use]
    pub fn system() -> Self {
        Self::new(Arc::new(SystemClock), Arc::new(ThreadSleeper))
    }

    /// The clock half, for callers that only need time.
    #[must_use]
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// The sleeper half.
    #[must_use]
    pub fn sleeper(&self) -> &Arc<dyn Sleeper> {
        &self.sleeper
    }

    /// Current instant from the wired clock.
    #[must_use]
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// `now + duration`, saturating instead of panicking on absurd durations.
    #[must_use]
    pub fn deadline_in(&self, duration: Duration) -> Instant {
        self.clock
            .now()
            .checked_add(duration)
            .unwrap_or_else(|| self.clock.now())
    }

    /// Block until `deadline` through the wired sleeper.
    pub fn sleep_until(&self, deadline: Instant) {
        self.sleeper.sleep_until(deadline);
    }

    /// Block for `duration` through the wired sleeper.
    pub fn sleep_for(&self, duration: Duration) {
        self.sleeper.sleep_until(self.deadline_in(duration));
    }
}

impl Default for Timing {
    fn default() -> Self {
        Self::system()
    }
}

impl fmt::Debug for Timing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Timing").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_mix64_repeats_for_a_fixed_seed() {
        let mut a = SplitMix64::new(7);
        let mut b = SplitMix64::new(7);
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }

        let mut c = SplitMix64::new(8);
        assert_ne!(SplitMix64::new(7).next_u64(), c.next_u64());
    }

    #[test]
    fn next_f64_stays_inside_the_half_open_unit_interval() {
        let mut rng = SplitMix64::new(0x1234_5678);
        for _ in 0..10_000 {
            let value = rng.next_f64();
            assert!((0.0..1.0).contains(&value), "{value} escaped [0, 1)");
        }
    }

    #[test]
    fn next_below_covers_the_half_open_integer_range() {
        let mut rng = SplitMix64::new(99);
        let mut seen = [false; 6];
        for _ in 0..1_000 {
            let value = next_below(&mut rng, 6);
            assert!(value < 6);
            seen[value as usize] = true;
        }
        assert!(seen.iter().all(|hit| *hit), "not every bucket was drawn");
        assert_eq!(next_below(&mut rng, 0), 0);
        assert_eq!(next_below(&mut rng, 1), 0);
    }

    #[test]
    fn next_range_holds_a_degenerate_range_at_the_low_bound() {
        let mut rng = SplitMix64::new(5);
        assert_eq!(rng.next_range(4.0, 4.0), 4.0);
        assert_eq!(rng.next_range(4.0, 1.0), 4.0);
        for _ in 0..1_000 {
            let value = rng.next_range(-3.0, 2.0);
            assert!((-3.0..2.0).contains(&value), "{value} escaped [-3, 2)");
        }
    }

    #[test]
    fn thread_sleeper_does_not_block_for_a_past_deadline() {
        // A deadline one second in the past, so an implementation that slept the
        // unsigned distance instead of returning would block for about a second.
        // The ceiling is 250 ms: wide enough that a scheduler stall cannot fail
        // the test, narrow enough that a spurious one-second sleep still does.
        // `checked_sub` is `Some` on any host that has been up for a second.
        let deadline = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("the host has been up for at least a second");
        let start = Instant::now();
        ThreadSleeper.sleep_until(deadline);
        assert!(start.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn timing_derives_deadlines_from_the_wired_clock() {
        let clock = Arc::new(crate::testing::VirtualClock::new());
        let timing = Timing::new(
            Arc::clone(&clock) as Arc<dyn Clock>,
            Arc::clone(&clock) as Arc<dyn Sleeper>,
        );
        let base = timing.now();
        let deadline = timing.deadline_in(Duration::from_millis(5));
        assert_eq!(deadline, base + Duration::from_millis(5));

        // Sleeping through the timing seam advances the injected clock.
        timing.sleep_until(deadline);
        assert_eq!(timing.now(), deadline);
    }
}
