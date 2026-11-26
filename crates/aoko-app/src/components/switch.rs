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
use gpui::{
    div, px, Animation, AnimationExt, AnyElement, App, ClickEvent, ElementId, IntoElement,
    SharedString, Window,
};

use crate::theme::{
    metrics,
    motion::{self, Motion},
    Color, Theme,
};

/// An animated switch, and the click that flips it.
///
/// `on_click` is the same shape as `super::checkbox`'s: the element carries `.id()`,
/// `.cursor_pointer()` and the handler, so a caller cannot render a switch that looks
/// interactive and is not. The handler runs with `&mut Window` and `&mut App`, which is
/// what a call site needs to reach the entity that owns the state:
///
/// ```ignore
/// let handle = entity.clone();
/// let module = "aim_assist";
/// components::switch(name, &theme, is_on, motion, move |_, _, cx| {
///     handle.update(cx, |shell, cx| shell.toggle_module(module, cx));
/// })
/// ```
///
/// `is_on` is the end state; the transition runs when the state changes because the
/// animation id carries the state, so each toggle mounts a fresh animation rather than
/// resuming the previous one.
pub fn switch(
    name: impl Into<SharedString>,
    theme: &Theme,
    is_on: bool,
    motion: Motion,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let name: SharedString = name.into();
    let id = ElementId::Name(name.clone());

    div()
        .id(id)
        .cursor_pointer()
        .on_click(on_click)
        .child(visual(name, theme, is_on, motion))
        .into_any_element()
}

/// The switch's appearance with no interaction, for the two callers that must not arm
/// anything: the overlay's rows (toggled by `f2`, not by a click) and the reduced-motion
/// frame.
fn visual(name: SharedString, theme: &Theme, is_on: bool, motion: Motion) -> AnyElement {
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
///
/// Display only: the overlay toggles a row with `f2` rather than a click, so this keeps
/// the picture and deliberately carries no handler. It returns a static frame, which
/// also keeps its animation ids out of the tree when several rows are drawn.
#[must_use]
pub fn switch_static(theme: &Theme, is_on: bool) -> AnyElement {
    visual(
        SharedString::from("overlay-switch"),
        theme,
        is_on,
        Motion::STILL,
    )
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

/// The blocker's test: a click on the rendered switch must reach the handler, because a
/// switch that draws and does not click is exactly the defect this round fixes.
#[cfg(test)]
mod click_tests {
    use super::*;
    use gpui::{
        point, px, size, AnyWindowHandle, Bounds, Entity, Modifiers, MouseButton, MouseDownEvent,
        MouseUpEvent, PlatformInput, TestAppContext, WindowBounds, WindowOptions,
    };

    struct Probe {
        hits: usize,
    }

    impl Render for Probe {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let entity: Entity<Self> = cx.entity();
            div().size_full().child(switch(
                "probe",
                &Theme::slate(),
                false,
                Motion::STILL,
                move |_, _, cx| {
                    entity.update(cx, |probe, _| probe.hits += 1);
                },
            ))
        }
    }

    #[test]
    fn a_click_on_the_switch_reaches_the_handler() {
        let mut cx = TestAppContext::single();
        let probe = cx.update(|cx| cx.new(|_| Probe { hits: 0 }));
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    // The switch is the first child of a full-size root, so it occupies
                    // (0, 0)-(46, 24) and a click at (12, 12) is inside the track.
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.0), px(0.0)),
                        size(px(400.0), px(120.0)),
                    ))),
                    ..Default::default()
                },
                |_, _| probe.clone(),
            )
            .expect("test window")
        });
        let handle: AnyWindowHandle = window.into();

        cx.update_window(handle, |_, window, cx| {
            // Hit testing reads the rendered frame, so paint once before clicking.
            let _ = window.draw(cx);
            let position = point(px(12.0), px(12.0));
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    position,
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    position,
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                }),
                cx,
            );
        })
        .expect("dispatch");

        assert_eq!(
            cx.read(|cx| probe.read(cx).hits),
            1,
            "a click on the switch must run the handler that flips the module"
        );
    }
}
