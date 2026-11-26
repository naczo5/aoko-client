//! The module toggle switch (`ModuleSwitch`, `App.xaml:152-205`).
//!
//! Geometry: a 46×24 track, an 18 px thumb inset 3, travelling 22 px on a 200 ms
//! quadratic ease-out while a second layer that is the accent fades in over the
//! track. The accent is a *layer* in the C# because a storyboard can animate an
//! opacity but not a brush; over opaque colours an accent layer at opacity `p` is
//! exactly `blend(track, accent, p)`, so the layer collapses into a computed colour
//! and needs no extra element.
//!
//! The overlay's row variant (`Aoko/Views/CommandSearchOverlay.xaml:46-89`) is
//! deliberately un-animated in the C#: a storyboard on `IsChecked` replays
//! "switched on" for every already-on module whenever a container is rebuilt, which
//! happens on most keystrokes. [`switch_static`] is that variant.

use gpui::prelude::*;
use gpui::{div, px, Animation, AnimationExt, AnyElement, IntoElement, SharedString};

use crate::theme::{
    metrics,
    motion::{self, Motion},
    Color, Theme,
};

/// An animated switch. `is_on` is the end state; the transition runs when the state
/// changes because the animation id carries the state, so each toggle mounts a fresh
/// animation rather than resuming the previous one.
pub fn switch(
    name: impl Into<SharedString>,
    theme: &Theme,
    is_on: bool,
    motion: Motion,
) -> AnyElement {
    let name: SharedString = name.into();
    let track = theme.slider_background;
    let edge = theme.slider_foreground;
    let accent = theme.accent;
    // The thumb's fill flips with the state, as the C# trigger does; only its
    // position and the accent fade are animated.
    let thumb_color = if is_on {
        theme.text
    } else {
        theme.slider_foreground
    };

    let inset = metrics::switch::INSET;
    let travel = metrics::switch::TRAVEL;
    // Progress 0 is the state the switch is coming *from*, 1 the state it lands on.
    let progress = move |t: f32| if is_on { t } else { 1.0 - t };
    let start = progress(0.0);
    let landing = progress(1.0);

    if !motion.enabled() {
        return frame(
            track.blend(accent, landing),
            edge.blend(accent, landing),
            thumb_color,
            inset + travel * landing,
        );
    }

    let animation =
        Animation::new(motion.duration(motion::SWITCH)).with_easing(motion::quadratic_ease_out);
    let track_id: SharedString = format!("{name}-track").into();
    let thumb_id: SharedString = format!("{name}-thumb").into();

    let thumb_animation = animation.clone();
    div()
        .relative()
        .w(px(metrics::switch::TRACK_WIDTH))
        .h(px(metrics::switch::TRACK_HEIGHT))
        .rounded(px(metrics::switch::TRACK_HEIGHT / 2.0))
        .border_1()
        .border_color(edge.blend(accent, start))
        .bg(track.blend(accent, start))
        .with_animation(track_id, animation, move |element, t| {
            let p = progress(t);
            element
                .bg(track.blend(accent, p))
                .border_color(edge.blend(accent, p))
        })
        .child(
            div().h_full().flex().items_center().child(
                div()
                    .w(px(metrics::switch::THUMB))
                    .h(px(metrics::switch::THUMB))
                    .rounded_full()
                    .bg(thumb_color)
                    .ml(px(inset + travel * start))
                    .with_animation(thumb_id, thumb_animation, move |element, t| {
                        element.ml(px(inset + travel * progress(t)))
                    }),
            ),
        )
        .into_any_element()
}

/// The overlay's deliberately un-animated variant (`CommandSearchOverlay.xaml:46-89`).
pub fn switch_static(theme: &Theme, is_on: bool) -> AnyElement {
    switch("overlay-switch", theme, is_on, Motion::STILL)
}

/// The switch at a fixed progress, used for the reduced-motion path.
fn frame(track: Color, edge: Color, thumb: Color, offset: f32) -> AnyElement {
    div()
        .w(px(metrics::switch::TRACK_WIDTH))
        .h(px(metrics::switch::TRACK_HEIGHT))
        .rounded(px(metrics::switch::TRACK_HEIGHT / 2.0))
        .border_1()
        .border_color(edge)
        .bg(track)
        .flex()
        .items_center()
        .child(
            div()
                .w(px(metrics::switch::THUMB))
                .h(px(metrics::switch::THUMB))
                .rounded_full()
                .bg(thumb)
                .ml(px(offset)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transition_starts_where_the_switch_was() {
        // The arithmetic the animator runs, kept as a test so the direction cannot be
        // "fixed" by accident: turning on runs 0→1, turning off runs 1→0.
        let progress_for = |is_on: bool, t: f32| if is_on { t } else { 1.0 - t };
        assert_eq!(progress_for(true, 0.0), 0.0);
        assert_eq!(progress_for(true, 1.0), 1.0);
        assert_eq!(progress_for(false, 0.0), 1.0);
        assert_eq!(progress_for(false, 1.0), 0.0);

        // At rest the thumb is flush with the far side: inset, travel, thumb, inset.
        let inset = metrics::switch::INSET;
        let travel = metrics::switch::TRAVEL;
        assert_eq!(
            inset + travel + metrics::switch::THUMB + inset,
            metrics::switch::TRACK_WIDTH
        );
    }

    #[test]
    fn the_accent_layer_equals_a_blend() {
        // Over opaque colours, "accent at opacity p over the track" is a blend. That
        // equivalence is what lets this widget drop the C#'s second border element.
        let theme = Theme::slate();
        let half = theme.slider_background.blend(theme.accent, 0.5);
        assert_ne!(half, theme.slider_background);
        assert_ne!(half, theme.accent);
        assert_eq!(
            theme.slider_background.blend(theme.accent, 0.0),
            theme.slider_background
        );
        assert_eq!(
            theme.slider_background.blend(theme.accent, 1.0),
            theme.accent
        );
    }
}
