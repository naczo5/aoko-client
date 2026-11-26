//! The slider (`DarkSlider`, `App.xaml:219-300`).
//!
//! There is **no slider widget at the pinned revision** (`UI_INVENTORY.md` row 8),
//! so this is built from primitives: a 24 px transparent hit row, a 6 px
//! `SliderBackground` track with radius 3, an accent fill up to the thumb, a 4 px
//! `SliderForeground` end dot, and a 6×16 `Text`-coloured thumb in a 20 px grab zone.
//!
//! **Correction to the inventory:** row 8's role table says `SliderForeground` is
//! "used as a 6 px slider fill for the *unfilled* portion". It is not. The template
//! fills the *decrease* region with `AccentBrush` (`App.xaml:242`) and the base track
//! with `SliderBgBrush` (`:234`); `SliderFgBrush` is only the 4 px end dot (`:237`)
//! and, elsewhere, the checkbox edge. This component follows the source.
//!
//! # Coalescing
//!
//! The C# pushes config through `CoalescedLatestValue` at
//! `ConfigChangeCoalesceMs = 25` (`Aoko/Core/GameStateClient.cs:37`), so a drag emits
//! one update per 25 ms with the latest value winning, not one per pixel.
//! [`DragCoalescer`] is that rule as a value: [`slider`] owns one per instance and
//! calls `on_change` only when it fires. The final value is flushed on mouse-up, so
//! the last pixel of a drag is never dropped.
//!
//! # What this does not do yet
//!
//! Keyboard stepping (`SmallChange` 1 / `LargeChange` 5), the thumb's animated scale
//! (its hover pair is applied by size rather than a transform, and the 120/180 ms
//! curves are not run), and drag continuation outside the row.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    canvas, div, px, relative, AnyElement, App, Bounds, ElementId, IntoElement, MouseButton,
    Pixels, SharedString, Window,
};

use crate::theme::{metrics, Theme};

/// `ConfigChangeCoalesceMs` (`Aoko/Core/GameStateClient.cs:37`).
pub const COALESCE_INTERVAL: Duration = Duration::from_millis(25);

/// Value mapping for one slider: range plus tick snapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderGeometry {
    pub min: f64,
    pub max: f64,
    /// Tick size. `0.0` disables snapping, matching `IsSnapToTickEnabled` being off.
    pub step: f64,
}

impl SliderGeometry {
    #[must_use]
    pub const fn new(min: f64, max: f64, step: f64) -> Self {
        Self { min, max, step }
    }

    /// A slider with no snapping.
    #[must_use]
    pub const fn continuous(min: f64, max: f64) -> Self {
        Self::new(min, max, 0.0)
    }

    /// Where `value` sits in the range, 0..1.
    #[must_use]
    pub fn fraction(&self, value: f64) -> f32 {
        let span = self.max - self.min;
        if span.abs() < f64::EPSILON {
            return 0.0;
        }
        (((value - self.min) / span) as f32).clamp(0.0, 1.0)
    }

    /// Snaps to the tick grid anchored on `min`, then clamps. Order matters: rounding
    /// first would let a tick outside the range through.
    #[must_use]
    pub fn snap(&self, value: f64) -> f64 {
        let clamped = value.clamp(self.min, self.max);
        if self.step <= 0.0 {
            return clamped;
        }
        let ticks = ((clamped - self.min) / self.step).round();
        (self.min + ticks * self.step).clamp(self.min, self.max)
    }

    /// The value at a 0..1 position along the track.
    #[must_use]
    pub fn value_at(&self, fraction: f32) -> f64 {
        let span = self.max - self.min;
        self.snap(self.min + f64::from(fraction.clamp(0.0, 1.0)) * span)
    }

    /// Thumb offset in pixels, clamped so the thumb never leaves the track.
    #[must_use]
    pub fn thumb_offset(&self, track_width: f32, thumb_width: f32, value: f64) -> f32 {
        let travel = (track_width - thumb_width).max(0.0);
        travel * self.fraction(value)
    }
}

/// Latest-wins throttle for drag updates.
#[derive(Clone, Debug)]
pub struct DragCoalescer {
    interval: Duration,
    last_emit: Option<Duration>,
    pending: Option<f64>,
    emitted: Option<f64>,
}

impl DragCoalescer {
    /// The production 25 ms rule.
    #[must_use]
    pub fn new() -> Self {
        Self::with_interval(COALESCE_INTERVAL)
    }

    #[must_use]
    pub fn with_interval(interval: Duration) -> Self {
        Self {
            interval,
            last_emit: None,
            pending: None,
            emitted: None,
        }
    }

    /// Records a value and returns the value to publish, if this press may publish.
    pub fn push(&mut self, now: Duration, value: f64) -> Option<f64> {
        self.pending = Some(value);
        let due = match self.last_emit {
            None => true,
            Some(last) => now.saturating_sub(last) >= self.interval,
        };
        if !due {
            return None;
        }
        self.take(now)
    }

    /// Publishes a pending value that differs from the last published one, e.g. on
    /// mouse-up. Returns `None` when nothing changed, so a click that does not move
    /// the thumb stays silent.
    pub fn flush(&mut self) -> Option<f64> {
        self.take(Duration::ZERO)
    }

    fn take(&mut self, now: Duration) -> Option<f64> {
        let pending = self.pending.take()?;
        self.last_emit = Some(now);
        if self.emitted == Some(pending) {
            return None;
        }
        self.emitted = Some(pending);
        Some(pending)
    }
}

impl Default for DragCoalescer {
    fn default() -> Self {
        Self::new()
    }
}

/// The publish callback, behind an `Rc` because the drag and the release handler both
/// hold it. `dyn` keeps the component's own type from growing a second generic.
type OnChange = Rc<dyn Fn(f64, &mut Window, &mut App)>;

/// A slider. `on_change` fires at most once per [`COALESCE_INTERVAL`] during a drag,
/// plus once on release when the final value differs from the last published one.
///
/// # Calling it
///
/// The callback takes `&mut Window` and `&mut App` so a call site can reach the entity
/// that owns the value instead of faking one:
///
/// ```ignore
/// let handle = entity.clone();
/// components::slider(name, &theme, geometry, value, move |next, _window, cx| {
///     handle.update(cx, |shell, cx| shell.set_value(bind, next, cx));
/// })
/// ```
///
/// `handle.update` is the form that works from `render`, where the caller holds only an
/// [`Entity`](gpui::Entity). A method that already has a `Context<Self>` can pass
/// `cx.processor(|this: &mut Self, next: f64, _window, cx| ...)` instead, which produces
/// this exact signature (`Context::processor` takes its payload by value, unlike
/// `Context::listener`, which takes `&E`).
pub fn slider(
    name: impl Into<SharedString>,
    theme: &Theme,
    geometry: SliderGeometry,
    value: f64,
    on_change: impl Fn(f64, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let name: SharedString = name.into();
    let on_change: OnChange = Rc::new(on_change);
    let coalescer = Rc::new(RefCell::new(DragCoalescer::new()));
    let dragging = Rc::new(Cell::new(false));
    // The row's own rectangle, captured in a canvas prepaint: `MouseDownEvent::position`
    // is window-relative, and there is no `Window::element_bounds` at this revision.
    let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));

    let publish = {
        let bounds = bounds.clone();
        let coalescer = coalescer.clone();
        let on_change = on_change.clone();
        move |x: Pixels, window: &mut Window, cx: &mut App| {
            let rect = bounds.get();
            let width = rect.size.width.max(px(1.0));
            let local = (x - rect.origin.x) / width;
            if let Some(value) = coalescer
                .borrow_mut()
                .push(elapsed(), geometry.value_at(local))
            {
                on_change(value, window, cx);
            }
        }
    };

    let down = {
        let publish = publish.clone();
        let dragging = dragging.clone();
        move |event: &gpui::MouseDownEvent, window: &mut Window, cx: &mut App| {
            dragging.set(true);
            publish(event.position.x, window, cx);
        }
    };
    let moved = {
        let publish = publish.clone();
        let dragging = dragging.clone();
        move |event: &gpui::MouseMoveEvent, window: &mut Window, cx: &mut App| {
            if dragging.get() {
                publish(event.position.x, window, cx);
            }
        }
    };
    let up = {
        let coalescer = coalescer.clone();
        let dragging = dragging.clone();
        move |_event: &gpui::MouseUpEvent, window: &mut Window, cx: &mut App| {
            dragging.set(false);
            if let Some(value) = coalescer.borrow_mut().flush() {
                on_change(value, window, cx);
            }
        }
    };

    let fraction = geometry.fraction(value);
    let track = metrics::slider::TRACK_HEIGHT;
    let thumb = metrics::slider::THUMB_WIDTH;
    let colour_track = theme.slider_background;
    let colour_fill = theme.accent;
    let colour_thumb = theme.text;
    let colour_dot = theme.slider_foreground;
    let capture = bounds;

    div()
        .id(ElementId::Name(name))
        .relative()
        .flex()
        .items_center()
        .h(px(24.0))
        .w_full()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, down)
        .on_mouse_move(moved)
        .on_mouse_up(MouseButton::Left, up)
        // The two track segments and the thumb are a flex row, so the thumb sits on the
        // fill boundary by layout instead of by an absolute offset that would need the
        // rendered width at build time.
        .child(
            div()
                .h(px(track))
                .flex_shrink_0()
                .w(relative(fraction))
                .rounded(px(metrics::radius::SLIDER_TRACK))
                .bg(colour_fill),
        )
        .child(
            div()
                .flex_shrink_0()
                .w(px(thumb))
                .h(px(metrics::slider::THUMB_HEIGHT))
                .rounded(px(metrics::radius::SLIDER_TRACK))
                .bg(colour_thumb)
                .hover(|style| {
                    style.w(px(thumb * metrics::slider::HOVER_SCALE)).h(px(
                        metrics::slider::THUMB_HEIGHT * metrics::slider::HOVER_DOT_SCALE,
                    ))
                }),
        )
        .child(
            div()
                .relative()
                .flex_1()
                .h(px(track))
                .rounded(px(metrics::radius::SLIDER_TRACK))
                .bg(colour_track)
                // End dot, 1 px in from the right edge.
                .child(
                    div()
                        .absolute()
                        .top(px((track - metrics::slider::END_DOT) / 2.0))
                        .right(px(1.0))
                        .w(px(metrics::slider::END_DOT))
                        .h(px(metrics::slider::END_DOT))
                        .rounded_full()
                        .bg(colour_dot),
                ),
        )
        // Bounds capture, last so it paints over nothing and sits behind the pointer.
        .child(
            canvas(
                move |own_bounds, _window, _cx| {
                    capture.set(own_bounds);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .w_full()
            .h_full(),
        )
        .into_any_element()
}

fn elapsed() -> Duration {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraction_and_value_round_trip_with_snapping() {
        // Min CPS: 1..20 with a 1-tick grid, the shape of `MainWindow.xaml:368`.
        let geometry = SliderGeometry::new(1.0, 20.0, 1.0);
        assert_eq!(geometry.fraction(1.0), 0.0);
        assert_eq!(geometry.fraction(20.0), 1.0);
        assert!((geometry.fraction(10.5) - 0.5).abs() < 1e-6);

        // Snapping anchors on `min`, so a 1-tick grid gives whole numbers from 1.
        // A 19-wide span puts the midpoint 10.5 between ticks, where it rounds up.
        assert_eq!(geometry.value_at(0.0), 1.0);
        assert_eq!(geometry.value_at(0.5), 11.0);
        assert_eq!(geometry.value_at(1.0), 20.0);
        assert_eq!(geometry.snap(10.4), 10.0);
        assert_eq!(geometry.snap(10.6), 11.0);
    }

    #[test]
    fn snapping_clamps_after_rounding() {
        let geometry = SliderGeometry::new(0.0, 10.0, 3.0);
        // 9.9 snaps to 9 (the last tick) rather than overflowing to 12.
        assert_eq!(geometry.snap(9.9), 9.0);
        assert_eq!(geometry.snap(10.0), 9.0);
        assert_eq!(geometry.snap(-5.0), 0.0);
        assert_eq!(geometry.value_at(1.0), 9.0);

        // Step 0 is a continuous slider, not a division by zero.
        let continuous = SliderGeometry::continuous(0.0, 1.0);
        assert_eq!(continuous.snap(0.37), 0.37);
        assert!((continuous.value_at(0.5) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn degenerate_range_does_not_divide_by_zero() {
        let geometry = SliderGeometry::new(5.0, 5.0, 1.0);
        assert_eq!(geometry.fraction(5.0), 0.0);
        assert_eq!(geometry.value_at(0.7), 5.0);
        assert_eq!(geometry.thumb_offset(100.0, 6.0, 5.0), 0.0);
    }

    #[test]
    fn thumb_travel_keeps_the_thumb_inside_the_track() {
        let geometry = SliderGeometry::new(0.0, 100.0, 0.0);
        assert_eq!(geometry.thumb_offset(200.0, 6.0, 0.0), 0.0);
        assert_eq!(geometry.thumb_offset(200.0, 6.0, 100.0), 194.0);
        assert_eq!(geometry.thumb_offset(200.0, 6.0, 50.0), 97.0);
        // A thumb wider than the track cannot produce a negative offset.
        assert_eq!(geometry.thumb_offset(4.0, 6.0, 50.0), 0.0);
    }

    #[test]
    fn the_first_push_publishes_immediately() {
        let mut coalescer = DragCoalescer::new();
        assert_eq!(coalescer.push(Duration::ZERO, 12.0), Some(12.0));
    }

    #[test]
    fn pushes_inside_the_window_keep_only_the_latest() {
        let mut coalescer = DragCoalescer::new();
        assert_eq!(coalescer.push(Duration::ZERO, 1.0), Some(1.0));
        // Within 25 ms nothing is published...
        assert_eq!(coalescer.push(Duration::from_millis(5), 2.0), None);
        assert_eq!(coalescer.push(Duration::from_millis(10), 3.0), None);
        assert_eq!(coalescer.push(Duration::from_millis(20), 4.0), None);
        // ...and the next due push publishes the latest, not the first.
        assert_eq!(coalescer.push(Duration::from_millis(26), 5.0), Some(5.0));
    }

    #[test]
    fn the_window_is_25_milliseconds() {
        assert_eq!(COALESCE_INTERVAL, Duration::from_millis(25));
        assert_eq!(DragCoalescer::new().interval, COALESCE_INTERVAL);
    }

    #[test]
    fn flush_publishes_the_final_value_once() {
        let mut coalescer = DragCoalescer::new();
        assert_eq!(coalescer.push(Duration::ZERO, 1.0), Some(1.0));
        assert_eq!(coalescer.push(Duration::from_millis(2), 9.0), None);
        // The drag ends: the held value must not be lost.
        assert_eq!(coalescer.flush(), Some(9.0));
        // A second flush has nothing to say.
        assert_eq!(coalescer.flush(), None);
    }

    #[test]
    fn flush_is_silent_when_the_value_did_not_change() {
        let mut coalescer = DragCoalescer::new();
        assert_eq!(coalescer.push(Duration::ZERO, 7.0), Some(7.0));
        // A click with no movement records the same value and publishes nothing.
        assert_eq!(coalescer.push(Duration::from_millis(1), 7.0), None);
        assert_eq!(coalescer.flush(), None);
    }

    #[test]
    fn a_rewound_clock_defers_rather_than_panicking() {
        // `elapsed()` is process-relative and monotonic, but a caller passing a smaller
        // duration must not panic. `saturating_sub` yields 0, i.e. "not yet due", so the
        // value is held and published by the next queued push or by `flush`.
        let mut coalescer = DragCoalescer::new();
        assert_eq!(coalescer.push(Duration::from_millis(100), 1.0), Some(1.0));
        assert_eq!(coalescer.push(Duration::from_millis(90), 2.0), None);
        assert_eq!(coalescer.flush(), Some(2.0));
    }
}
