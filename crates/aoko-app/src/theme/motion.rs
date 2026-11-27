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
/// `DarkButton` hover overlay fade-in, `App.xaml:352`.
pub const BUTTON_HOVER: Duration = Duration::from_millis(150);
/// The same overlay fading back out, which the C# runs slower, `App.xaml:359`. Hover-out
/// being longer than hover-in is deliberate in the template and is why the pair exists
/// rather than one `BUTTON_HOVER`.
pub const BUTTON_HOVER_OUT: Duration = Duration::from_millis(200);
/// `DarkButton` press: the shell scales to 0.96, `App.xaml:369-370`.
pub const BUTTON_PRESS: Duration = Duration::from_millis(80);
/// Release back to 1, `App.xaml:377-378`.
pub const BUTTON_RELEASE: Duration = Duration::from_millis(120);
/// Control-center rail item hover: 1.035 scale plus a 0.12 accent glow,
/// `MainWindow.xaml:119-121`.
pub const TAB_HOVER: Duration = Duration::from_millis(150);
/// The rail item easing back out, `MainWindow.xaml:128-130`.
pub const TAB_HOVER_OUT: Duration = Duration::from_millis(200);
/// The active rail item's accent bar growing to full height, `MainWindow.xaml:141` —
/// `BackEase Amplitude="0.6"`, so this one goes with [`back_ease_out`].
pub const TAB_SELECT: Duration = Duration::from_millis(220);
/// The same bar's opacity, `MainWindow.xaml:143`. Shorter than [`TAB_SELECT`], so the bar
/// is already opaque while it is still growing.
pub const TAB_SELECT_FADE: Duration = Duration::from_millis(180);
/// Slider thumb scale on hover, `App.xaml:281-282`.
pub const SLIDER_HOVER: Duration = Duration::from_millis(120);
/// The thumb shrinking back, `App.xaml:289-290`.
pub const SLIDER_HOVER_OUT: Duration = Duration::from_millis(180);
/// The thumb's drag scale, `App.xaml:298-299`.
pub const SLIDER_DRAG: Duration = Duration::from_millis(100);
/// Overlay close fade, `CommandSearchOverlay.xaml.cs:104`.
pub const OVERLAY_CLOSE: Duration = Duration::from_millis(100);
/// Overlay card entrance, `CommandSearchOverlay.xaml.cs:150`.
pub const OVERLAY_CARD_IN: Duration = Duration::from_millis(140);
/// Overlay scrim fade-in, `CommandSearchOverlay.xaml.cs:153`.
pub const OVERLAY_SCRIM: Duration = Duration::from_millis(180);
/// Overlay card re-anchor when the result count changes, `:221`.
pub const OVERLAY_RECARD: Duration = Duration::from_millis(120);
/// Dropdown entrance. The C# asked WPF for `PopupAnimation="Slide"` (`App.xaml:72`), whose
/// built-in slide is the platform's own duration; 120 ms is ours, inside the
/// `design-aesthetic` skill's "under 250 ms" rule and the same pitch as the overlay card.
pub const POPUP_IN: Duration = Duration::from_millis(120);
/// Tooltip fade. WPF's `ToolTip` defaults to `PopupAnimation="Fade"`, so the managed
/// tooltips faded in without the XAML saying so, and a tooltip that appears in one frame
/// reads as a glitch next to every other animated surface.
pub const TOOLTIP_FADE: Duration = Duration::from_millis(120);
/// An expander body appearing. WPF's `Expander` has no animation at all
/// (`MainWindow.xaml:1212` sets only `Foreground`), so this is a client choice: a body that
/// pops into place reads as a broken layout, and the `design-aesthetic` skill's motion table
/// puts disclosure at the same 150 ms as a button hover.
pub const EXPANDER_BODY: Duration = Duration::from_millis(150);

/// How far the overlay card rises as it fades in, `CommandSearchOverlay.xaml.cs:163`:
/// `CardSlide.Y` runs 8 → 0. The close runs 0 → 6, not 8 (`:109`), so the two are not
/// symmetric and must not share a constant.
pub const OVERLAY_CARD_SLIDE: f32 = 8.0;
/// Dropdown entrance offset, ours. It exists only while [`POPUP_IN`] runs, which is why it
/// lives here rather than in `metrics`, whose numbers are laid-out geometry.
pub const POPUP_SLIDE: f32 = 4.0;
/// Expander body rise, ours; see [`EXPANDER_BODY`].
pub const EXPANDER_SLIDE: f32 = 4.0;

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

/// `QuarticEase EasingMode="EaseOut"` (`CommandSearchOverlay.xaml.cs:151`): `1 - (1 - t)⁴`.
///
/// The overlay card and its slide use this rather than the cubic the pulse uses, and the
/// difference is visible on a 140 ms run: quartic is already 94 % of the way at the halfway
/// point, so the card snaps into place and settles instead of gliding.
#[must_use]
pub fn quartic_ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let inverse = 1.0 - t;
    1.0 - inverse * inverse * inverse * inverse
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
        for easing in [
            linear,
            quadratic_ease_out,
            cubic_ease_out,
            quartic_ease_out,
            back_ease_out,
        ] {
            assert!((easing(0.0) - 0.0).abs() < 1e-6);
            assert!((easing(1.0) - 1.0).abs() < 1e-6);
            assert!((easing(-2.0) - easing(0.0)).abs() < 1e-6);
            assert!((easing(3.0) - easing(1.0)).abs() < 1e-6);
        }
    }

    #[test]
    fn the_durations_are_the_csharp_numbers() {
        // One home for every duration the C# storyboards use, so a component cannot
        // round 140 to "about 150" and call it the port.
        assert_eq!(BUTTON_HOVER.as_millis(), 150);
        assert_eq!(BUTTON_HOVER_OUT.as_millis(), 200);
        assert_eq!(BUTTON_PRESS.as_millis(), 80);
        assert_eq!(BUTTON_RELEASE.as_millis(), 120);
        assert_eq!(TAB_HOVER.as_millis(), 150);
        assert_eq!(TAB_HOVER_OUT.as_millis(), 200);
        assert_eq!(TAB_SELECT.as_millis(), 220);
        assert_eq!(TAB_SELECT_FADE.as_millis(), 180);
        assert_eq!(SLIDER_HOVER.as_millis(), 120);
        assert_eq!(SLIDER_HOVER_OUT.as_millis(), 180);
        assert_eq!(SLIDER_DRAG.as_millis(), 100);
        assert_eq!(OVERLAY_CARD_IN.as_millis(), 140);
        assert_eq!(OVERLAY_SCRIM.as_millis(), 180);
        // The card slide is 8 px in and 6 px out (`CommandSearchOverlay.xaml.cs:163`, `:109`).
        assert_eq!(OVERLAY_CARD_SLIDE, 8.0);
        // Hover-out is slower than hover-in wherever the C# bothered to say so.
        for (enter, exit) in [
            (BUTTON_HOVER, BUTTON_HOVER_OUT),
            (TAB_HOVER, TAB_HOVER_OUT),
            (SLIDER_HOVER, SLIDER_HOVER_OUT),
        ] {
            assert!(exit > enter, "hover-out must be the slower half");
        }
        // The one rule the design system adds on top of the C# set: interaction feedback
        // is short. The pulse and the sweep are the only long ones, and they are not
        // feedback, so they are deliberately absent from this list.
        let ceiling = Duration::from_millis(250);
        for duration in [
            SWITCH,
            BUTTON_HOVER,
            BUTTON_HOVER_OUT,
            BUTTON_PRESS,
            BUTTON_RELEASE,
            TAB_HOVER,
            TAB_HOVER_OUT,
            TAB_SELECT,
            TAB_SELECT_FADE,
            SLIDER_HOVER,
            SLIDER_HOVER_OUT,
            SLIDER_DRAG,
            OVERLAY_CARD_IN,
            OVERLAY_SCRIM,
            OVERLAY_CLOSE,
            POPUP_IN,
            TOOLTIP_FADE,
            EXPANDER_BODY,
        ] {
            assert!(
                duration < ceiling,
                "a micro-interaction is {duration:?}, above the design system's 250 ms ceiling"
            );
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
