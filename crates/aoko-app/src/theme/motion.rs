//! Animation tokens and the animation gate.
//!
//! Decision 3 (§12 of `MIGRATION_ARCHITECTURE.md`) settles the one question the
//! inventory flagged as blocking (§5.3): the C# gated four animation sites on
//! `SystemParameters.ClientAreaAnimation`, which GPUI does not expose, so the
//! preference becomes a **client setting we own** and one toggle governs every
//! animated site on every platform. Do not probe the OS preference.
//!
//! Durations are the C# values, not round numbers invented here. The pitch of a
//! 200 ms switch and a 150 ms button hover is part of how the app feels, and the
//! capture harness has to be able to freeze it (`Motion::still`) for a
//! deterministic screenshot.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Module switch thumb travel and accent fade, `App.xaml:184-188`.
pub const SWITCH: Duration = Duration::from_millis(200);
/// `DarkButton` hover overlay, `App.xaml:337-360`.
pub const BUTTON_HOVER: Duration = Duration::from_millis(150);
/// Overlay close fade, `CommandSearchOverlay.xaml.cs:104`.
pub const OVERLAY_CLOSE: Duration = Duration::from_millis(100);
/// Overlay card entrance, `CommandSearchOverlay.xaml.cs:150`.
pub const OVERLAY_CARD_IN: Duration = Duration::from_millis(140);
/// Overlay scrim fade-in, `CommandSearchOverlay.xaml.cs:153`.
pub const OVERLAY_SCRIM: Duration = Duration::from_millis(180);
/// Overlay card re-anchor when the result count changes, `:221`.
pub const OVERLAY_RECARD: Duration = Duration::from_millis(120);

/// Card pulse keyframe times, `MainWindow.xaml.cs:1797-1810`.
pub const PULSE_RISE: Duration = Duration::from_millis(160);
pub const PULSE_HOLD: Duration = Duration::from_millis(820);
pub const PULSE_TOTAL: Duration = Duration::from_millis(1200);

/// The servers tab's indeterminate bar sweep: one 2.2 s `RepeatBehavior="Forever"`
/// pass (`ServersView.xaml:158-164`).
pub const PROGRESS_SWEEP: Duration = Duration::from_millis(2200);

/// `BackEase` amplitude from `MainWindow.xaml:143`.
pub const BACK_AMPLITUDE: f32 = 0.6;

/// Linear progress. The fourth easing in the C# set, used by the pulse's fall.
#[must_use]
pub fn linear(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

/// `QuadraticEase EasingMode="EaseOut"` (`App.xaml:185`): `1 - (1 - t)²`.
#[must_use]
pub fn quadratic_ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// `CubicEase EasingMode="EaseOut"` (`MainWindow.xaml.cs:1800`): `(t - 1)³ + 1`.
#[must_use]
pub fn cubic_ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let inverse = 1.0 - t;
    1.0 - inverse * inverse * inverse
}

/// `BackEase` with [`BACK_AMPLITUDE`], eased out.
///
/// WPF's `BackEase` core is `t³ - t·A·sin(πt)` and `EaseOut` mirrors `EaseIn`, so
/// this is `1 - EaseIn(1 - t)`. It overshoots past 1 before settling, which is the
/// whole reason to keep the named easing rather than approximating it.
#[must_use]
pub fn back_ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let inverse = 1.0 - t;
    let ease_in = inverse * inverse * inverse
        - inverse * BACK_AMPLITUDE * (std::f32::consts::PI * inverse).sin();
    1.0 - ease_in
}

/// The card pulse's rise–hold–fall opacity, over normalised progress.
///
/// `with_animation` hands out 0→1 progress, so the three linear keyframes from
/// `MainWindow.xaml.cs:1797-1810` have to be reconstructed by clamping `t` rather
/// than by asking for three separate animations.
#[must_use]
pub fn card_pulse(progress: f32) -> f32 {
    let rise = PULSE_RISE.as_secs_f32() / PULSE_TOTAL.as_secs_f32();
    let hold = PULSE_HOLD.as_secs_f32() / PULSE_TOTAL.as_secs_f32();
    let t = progress.clamp(0.0, 1.0);
    if t < rise {
        cubic_ease_out(t / rise)
    } else if t < hold {
        1.0
    } else {
        1.0 - linear((t - hold) / (1.0 - hold))
    }
}

/// The animation gate.
///
/// Two ways in, both from decision 3: [`Motion::enabled`] carries the setting
/// explicitly (the shape components take, and the only shape that is testable in
/// isolation), and the process-wide [`animations_enabled`] / [`set_animations_enabled`]
/// pair is where the shell puts the client setting so leaf widgets can read it
/// without threading a flag through every call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Motion {
    enabled: bool,
}

impl Motion {
    /// Animated, the default (the C# default is `ClientAreaAnimation = true`).
    pub const ANIMATED: Self = Self { enabled: true };
    /// Reduced motion: every animation collapses to its end state.
    pub const STILL: Self = Self { enabled: false };

    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    #[must_use]
    pub const fn enabled(self) -> bool {
        self.enabled
    }

    /// The duration to use, `Duration::ZERO` when motion is off.
    #[must_use]
    pub fn duration(self, duration: Duration) -> Duration {
        if self.enabled {
            duration
        } else {
            Duration::ZERO
        }
    }

    /// Progress to render: `1.0` when motion is off, so the widget lands on the
    /// animated end state instead of jumping to the start.
    #[must_use]
    pub fn progress(self, t: f32) -> f32 {
        if self.enabled {
            t.clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// Card pulse opacity: fully faded out when motion is off, because the C#
    /// leaves the card unmarked rather than drawing a static ring
    /// (`MainWindow.xaml.cs:1786-1789`).
    #[must_use]
    pub fn pulse(self, t: f32) -> f32 {
        if self.enabled {
            card_pulse(t)
        } else {
            0.0
        }
    }
}

impl Default for Motion {
    fn default() -> Self {
        match animations_enabled() {
            true => Self::ANIMATED,
            false => Self::STILL,
        }
    }
}

static ANIMATIONS_ENABLED: AtomicBool = AtomicBool::new(true);

/// Reads the client animation setting (default: on).
#[must_use]
pub fn animations_enabled() -> bool {
    ANIMATIONS_ENABLED.load(Ordering::Relaxed)
}

/// Writes the client animation setting. The shell calls this once when it loads
/// the profile and again whenever the user flips the toggle.
pub fn set_animations_enabled(enabled: bool) {
    ANIMATIONS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// The current gate as a value.
#[must_use]
pub fn motion() -> Motion {
    Motion::new(animations_enabled())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easings_hit_their_endpoints_and_clamp() {
        for easing in [linear, quadratic_ease_out, cubic_ease_out, back_ease_out] {
            assert!((easing(0.0) - 0.0).abs() < 1e-6);
            assert!((easing(1.0) - 1.0).abs() < 1e-6);
            assert!((easing(-2.0) - easing(0.0)).abs() < 1e-6);
            assert!((easing(3.0) - easing(1.0)).abs() < 1e-6);
        }
    }

    #[test]
    fn ease_out_is_ahead_of_linear_and_monotonic() {
        for easing in [quadratic_ease_out, cubic_ease_out] {
            let mut previous = -1.0;
            for step in 0..=20 {
                let t = step as f32 / 20.0;
                let value = easing(t);
                assert!(value >= previous, "easing went backwards at {t}");
                assert!(value >= t - 1e-6, "ease-out is behind linear at {t}");
                previous = value;
            }
        }
        // At the halfway point a quadratic ease-out is three quarters of the way.
        assert!((quadratic_ease_out(0.5) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn back_ease_overshoots_then_settles() {
        let peak = (1..100)
            .map(|step| back_ease_out(step as f32 / 100.0))
            .fold(f32::MIN, f32::max);
        assert!(peak > 1.0, "BackEase 0.6 should overshoot, peak was {peak}");
        assert!(
            peak < 1.2,
            "overshoot is not plausibly 0.6 amplitude: {peak}"
        );
    }

    #[test]
    fn card_pulse_follows_the_three_keyframes() {
        assert!((card_pulse(0.0) - 0.0).abs() < 1e-6);
        // 160 ms of a 1200 ms run is the top of the rise.
        let rise = 160.0 / 1200.0;
        assert!((card_pulse(rise) - 1.0).abs() < 1e-6);
        assert!(
            (card_pulse(0.5) - 1.0).abs() < 1e-6,
            "the hold must stay at 1"
        );
        assert!((card_pulse(820.0 / 1200.0) - 1.0).abs() < 1e-6);
        assert!((card_pulse(1.0) - 0.0).abs() < 1e-6);
        // Monotone rise, monotone fall.
        assert!(card_pulse(0.05) < card_pulse(0.1));
        assert!(card_pulse(0.9) > card_pulse(0.99));
    }

    #[test]
    fn still_motion_collapses_to_the_end_state() {
        let still = Motion::STILL;
        assert_eq!(still.duration(SWITCH), Duration::ZERO);
        assert_eq!(still.progress(0.0), 1.0);
        assert_eq!(still.pulse(0.4), 0.0);
        assert!(!still.enabled());

        let animated = Motion::ANIMATED;
        assert_eq!(animated.duration(SWITCH), SWITCH);
        assert_eq!(animated.progress(0.25), 0.25);
        assert!(animated.pulse(0.4) > 0.9);
    }

    #[test]
    fn the_client_setting_drives_the_gate() {
        // One test owns the global so parallel tests cannot observe a torn state.
        let original = animations_enabled();
        assert!(original, "animations default to on");

        set_animations_enabled(false);
        assert!(!animations_enabled());
        assert_eq!(motion(), Motion::STILL);

        set_animations_enabled(true);
        assert!(animations_enabled());
        assert_eq!(motion(), Motion::ANIMATED);
        assert_eq!(Motion::default(), Motion::ANIMATED);
    }
}
