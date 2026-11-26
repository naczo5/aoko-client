//! Determinate and indeterminate progress bars.
//!
//! Three surfaces use one: the injection bar (`MainWindow.xaml:333`, 0–100), the
//! accounts tab's 80×3 indeterminate busy line (`AccountsView.xaml:429`), and the
//! servers tab's re-templated bar with a looping 2.2 s storyboard
//! (`ServersView.xaml:158-164`). There is no `ProgressBar` in `gpui`, so both shapes
//! are a filled div over a track.

use gpui::prelude::*;
use gpui::{
    div, px, relative, Animation, AnimationExt, AnyElement, Div, IntoElement, SharedString,
};

use crate::theme::{
    motion::{self, Motion},
    Theme,
};

/// Bar geometry. The accounts busy line is 3 px tall and 80 wide; the injection bar
/// fills its card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressStyle {
    pub height: f32,
    pub width: Option<f32>,
    pub radius: f32,
}

impl ProgressStyle {
    /// The standard 6 px bar filling its container.
    #[must_use]
    pub const fn bar() -> Self {
        Self {
            height: 6.0,
            width: None,
            radius: 3.0,
        }
    }

    /// The accounts tab's 80×3 indeterminate line (`AccountsView.xaml:429`).
    #[must_use]
    pub const fn busy() -> Self {
        Self {
            height: 3.0,
            width: Some(80.0),
            radius: 1.0,
        }
    }
}

impl Default for ProgressStyle {
    fn default() -> Self {
        Self::bar()
    }
}

/// The sweeping segment's share of the track and the distance it travels, leaving a
/// gap so the segment is never flush with both ends at once.
const SEGMENT: f32 = 0.35;
const TRAVEL: f32 = 1.0 - SEGMENT;

/// A determinate bar. `fraction` is clamped to 0..1.
pub fn progress(
    _name: impl Into<SharedString>,
    theme: &Theme,
    style: ProgressStyle,
    fraction: f32,
) -> AnyElement {
    let fraction = fraction.clamp(0.0, 1.0);
    track(theme, style)
        .child(
            div()
                .h_full()
                .w(relative(fraction))
                .rounded(px(style.radius))
                .bg(theme.accent),
        )
        .into_any_element()
}

/// An indeterminate bar: a short accent segment sliding across the track, looping.
///
/// With motion off it renders a static half-track fill instead of freezing mid-slide,
/// which is the readable state when the client animation setting is disabled.
pub fn progress_indeterminate(
    name: impl Into<SharedString>,
    theme: &Theme,
    style: ProgressStyle,
    motion: Motion,
) -> AnyElement {
    let name: SharedString = name.into();
    if !motion.enabled() {
        return progress(name, theme, style, 0.5);
    }

    let animation = Animation::new(motion.duration(motion::PROGRESS_SWEEP)).repeat();
    let segment = div()
        .h_full()
        .w(relative(SEGMENT))
        .rounded(px(style.radius))
        .bg(theme.accent);

    track(theme, style)
        .child(
            segment.with_animation(format!("{name}-slide"), animation, |element, t| {
                element.ml(relative(TRAVEL * t))
            }),
        )
        .into_any_element()
}

fn track(theme: &Theme, style: ProgressStyle) -> Div {
    let mut element = div()
        .h(px(style.height))
        .rounded(px(style.radius))
        .bg(theme.slider_background)
        .overflow_hidden();
    element = match style.width {
        Some(width) => element.w(px(width)),
        None => element.w_full(),
    };
    element
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_geometry_matches_the_accounts_line() {
        let busy = ProgressStyle::busy();
        assert_eq!(busy.height, 3.0);
        assert_eq!(busy.width, Some(80.0));

        let bar = ProgressStyle::bar();
        assert_eq!(bar.width, None);
        assert_eq!(bar.height, 6.0);

        // Radius never exceeds half the height, or the ends render as a lens.
        for style in [busy, bar] {
            assert!(style.radius <= style.height / 2.0);
        }
    }

    #[test]
    fn the_sweep_stays_inside_the_track() {
        for step in 0..=20 {
            let t = step as f32 / 20.0;
            assert!(SEGMENT + TRAVEL * t <= 1.0 + 1e-6);
        }
        // And it reaches the end exactly.
        assert!((SEGMENT + TRAVEL - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_sweep_is_the_csharp_storyboard_duration() {
        assert_eq!(motion::PROGRESS_SWEEP.as_millis(), 2200);
    }
}
