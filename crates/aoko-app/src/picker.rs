//! Colour-picker widgets: the saturation/value square, the hue bar and the preview
//! swatch.
//!
//! `UI_INVENTORY.md` row 21 calls the picker the hardest widget after the overlay, and
//! §5.5 records why: WPF built the SV square from a `Canvas` with three stacked
//! gradients (`ColorPickerDialog.xaml:23-43`) and GPUI has **solid and two-stop linear
//! only**. The picker's numbers live in [`crate::dialogs::color_picker`], whose docs
//! say the painter is not modelled there and belongs to the view. This is that painter.
//!
//! The approach is custom paint, as the inventory recommends: one horizontal two-stop
//! gradient per pixel row. For row `i` of `n`, value is `1 - i/n`, so the row runs from
//! `hsv(hue, 0, value)` to `hsv(hue, 1, value)`. Stacking those rows reproduces the
//! WPF composite (hue base, white ramp across, black ramp down) without needing an
//! alpha blend mode or a multi-stop gradient: the two outer ramps are folded into each
//! row's endpoints.
//!
//! The hue bar is a column of solid rows for the same reason: six stops would band
//! visibly at 22 px wide and 180 px tall.
//!
//! Custom paint needs a paint callback, so both widgets are a `canvas` inside an
//! interactive wrapper. The wrapper owns the mouse events and the canvas captures the
//! element's rectangle for pointer-to-local conversion, because
//! `MouseDownEvent::position` is window-relative and there is no
//! `Window::element_bounds` at this pin.
//!
//! Both widgets take their events as data, the shape `components::slider` uses: the drag
//! callback receives the element-local position **and** the window and app, so the caller
//! can write its own state and repaint. That is what makes a press here a value — the
//! earlier shape (`Fn(f32, f32)`) gave the caller no way to notify an entity, so the
//! dialog could only paint a dead square. Nothing in this file knows what the caller does
//! with the position; the widget keeps the drag flag and the captured rectangle and no
//! colour state at all.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    canvas, div, fill, point, px, size, AnyElement, App, Bounds, IntoElement, MouseButton, Pixels,
    SharedString, Window,
};

use crate::dialogs::color_picker::{hsv_to_rgb, Hsv, Rgb, HUE_HEIGHT, SV_HEIGHT, SV_WIDTH};
use crate::theme::{fixed, Color, Theme};

/// The SV thumb's diameter (`ColorPickerDialog.xaml:43`).
pub const SV_THUMB: f32 = 12.0;
/// The hue bar's width. The markup gives the bar no width of its own; this is the
/// fixed 22 px the row next to the 220 px square leaves, and both the track and the
/// thumb's plate use it.
pub const HUE_WIDTH: f32 = 22.0;
/// The hue thumb's height (`:52-68`).
pub const HUE_THUMB: f32 = 4.0;

/// The square's drag callback: element-local `(x, y)`, plus the window and app so the
/// caller can write its own state and repaint.
///
/// The three callbacks are named rather than written out at each binding: the same shapes
/// appear in [`crate::components::slider`] and the colour dialog, and a reader comparing
/// them should see three documented types instead of a repeated `Rc<dyn Fn(..)>`.
pub type DragCallback = dyn Fn(f32, f32, &mut Window, &mut App);

/// The hue bar's drag callback: element-local `y`, plus the window and app.
pub type HueDragCallback = dyn Fn(f32, &mut Window, &mut App);

/// Either widget's drag-ended callback.
pub type ReleaseCallback = dyn Fn(&mut Window, &mut App);

/// The saturation/value square.
///
/// `on_drag` receives square-local pixels on press and on every move while the left button
/// is held, plus the window and app so the caller can write its own [`ColorPickerState`]
/// and repaint; `on_release` ends the drag. The widget keeps no state but the captured
/// bounds: whether a move belongs to a drag that *started here* is the caller's own flag,
/// which is why [`ColorPickerState::move_sv`] ignores a move it did not see a press for.
/// A widget-local flag could not work — the caller's repaint rebuilds this element, and
/// the flag would be gone before the next move arrives.
///
/// [`ColorPickerState`]: crate::dialogs::color_picker::ColorPickerState
/// [`ColorPickerState::move_sv`]: crate::dialogs::color_picker::ColorPickerState::move_sv
pub fn sv_square(
    name: impl Into<SharedString>,
    hsv: Hsv,
    on_drag: impl Fn(f32, f32, &mut Window, &mut App) + 'static,
    on_release: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let name: SharedString = name.into();
    let on_drag: Rc<DragCallback> = Rc::new(on_drag);
    let on_release: Rc<ReleaseCallback> = Rc::new(on_release);
    let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));

    let paint_bounds = bounds.clone();
    let hue = hsv.hue;

    let down = {
        let on_drag = on_drag.clone();
        let bounds = bounds.clone();
        move |event: &gpui::MouseDownEvent, window: &mut Window, cx: &mut App| {
            let (x, y) = local(&bounds.get(), event.position);
            on_drag(x, y, window, cx);
        }
    };
    let moved = {
        let bounds = bounds.clone();
        let on_drag = on_drag.clone();
        move |event: &gpui::MouseMoveEvent, window: &mut Window, cx: &mut App| {
            if event.pressed_button == Some(MouseButton::Left) {
                let (x, y) = local(&bounds.get(), event.position);
                on_drag(x, y, window, cx);
            }
        }
    };
    let up = move |_event: &gpui::MouseUpEvent, window: &mut Window, cx: &mut App| {
        on_release(window, cx);
    };

    let thumb_x = (hsv.sat as f32).clamp(0.0, 1.0) * SV_WIDTH - SV_THUMB / 2.0;
    let thumb_y = (1.0 - hsv.val as f32).clamp(0.0, 1.0) * SV_HEIGHT - SV_THUMB / 2.0;

    div()
        .id(gpui::ElementId::Name(name))
        .relative()
        .w(px(SV_WIDTH))
        .h(px(SV_HEIGHT))
        .overflow_hidden()
        .rounded(px(3.0))
        .cursor(gpui::CursorStyle::Crosshair)
        .on_mouse_down(MouseButton::Left, down)
        .on_mouse_move(moved)
        .on_mouse_up(MouseButton::Left, up)
        .child(
            canvas(
                move |own_bounds, _window, _cx| {
                    paint_bounds.set(own_bounds);
                },
                move |own_bounds, _, window, _cx| {
                    paint_sv_square(own_bounds, hue, window);
                },
            )
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .top(px(thumb_y))
                .left(px(thumb_x))
                .w(px(SV_THUMB))
                .h(px(SV_THUMB))
                .rounded_full()
                .border_2()
                .border_color(fixed::CONTRAST_LABEL),
        )
        .into_any_element()
}

/// The vertical hue bar.
///
/// `on_drag` receives bar-local `y` on press and on every move while the left button is
/// held, plus the window and app, so the caller can write its own
/// [`ColorPickerState`](crate::dialogs::color_picker::ColorPickerState) and repaint;
/// `on_release` ends the drag. Same rule as the square: the caller owns the flag that says
/// the drag started here.
pub fn hue_bar(
    name: impl Into<SharedString>,
    hsv: Hsv,
    on_drag: impl Fn(f32, &mut Window, &mut App) + 'static,
    on_release: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let name: SharedString = name.into();
    let on_drag: Rc<HueDragCallback> = Rc::new(on_drag);
    let on_release: Rc<ReleaseCallback> = Rc::new(on_release);
    let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));

    let paint_bounds = bounds.clone();

    let down = {
        let on_drag = on_drag.clone();
        let bounds = bounds.clone();
        move |event: &gpui::MouseDownEvent, window: &mut Window, cx: &mut App| {
            let (_, y) = local(&bounds.get(), event.position);
            on_drag(y, window, cx);
        }
    };
    let moved = {
        let bounds = bounds.clone();
        let on_drag = on_drag.clone();
        move |event: &gpui::MouseMoveEvent, window: &mut Window, cx: &mut App| {
            if event.pressed_button == Some(MouseButton::Left) {
                let (_, y) = local(&bounds.get(), event.position);
                on_drag(y, window, cx);
            }
        }
    };
    let up = move |_event: &gpui::MouseUpEvent, window: &mut Window, cx: &mut App| {
        on_release(window, cx);
    };

    let thumb_y = (hsv.hue as f32 / 360.0).clamp(0.0, 1.0) * HUE_HEIGHT - HUE_THUMB / 2.0;

    div()
        .id(gpui::ElementId::Name(name))
        .relative()
        .w(px(HUE_WIDTH))
        .h(px(HUE_HEIGHT))
        .overflow_hidden()
        .rounded(px(3.0))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, down)
        .on_mouse_move(moved)
        .on_mouse_up(MouseButton::Left, up)
        .child(
            canvas(
                move |own_bounds, _window, _cx| {
                    paint_bounds.set(own_bounds);
                },
                |own_bounds, _, window, _cx| {
                    paint_hue_bar(own_bounds, window);
                },
            )
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .top(px(thumb_y))
                .left(px(0.0))
                .w(px(HUE_WIDTH))
                .h(px(HUE_THUMB))
                .border_1()
                .border_color(fixed::CONTRAST_LABEL)
                // `#80FFFFFF`: the C# thumb is a half-transparent white plate.
                .bg(fixed::CONTRAST_LABEL.with_alpha(0x80)),
        )
        .into_any_element()
}

/// The 36×36 preview swatch (`ColorPickerDialog.xaml:78-79`).
pub fn preview_swatch(theme: &Theme, rgb: Rgb) -> AnyElement {
    div()
        .w(px(36.0))
        .h(px(36.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(theme.slider_background)
        .bg(Color::rgb(rgb.r, rgb.g, rgb.b))
        .into_any_element()
}

/// Paints the square as one two-stop horizontal gradient per pixel row.
fn paint_sv_square(bounds: Bounds<Pixels>, hue: f64, window: &mut Window) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let rows = height.ceil().max(1.0) as usize;
    let row_height = height / rows as f32;

    for row in 0..rows {
        let value = 1.0 - (row as f32 + 0.5) / rows as f32;
        let left = hsv_to_rgb(hue, 0.0, f64::from(value));
        let right = hsv_to_rgb(hue, 1.0, f64::from(value));
        let rect = Bounds {
            origin: point(
                bounds.origin.x,
                bounds.origin.y + px(row as f32 * row_height),
            ),
            size: size(px(width), px(row_height)),
        };
        window.paint_quad(fill(
            rect,
            gpui::linear_gradient(
                90.0,
                gpui::linear_color_stop(Color::rgb(left.r, left.g, left.b), 0.0),
                gpui::linear_color_stop(Color::rgb(right.r, right.g, right.b), 1.0),
            ),
        ));
    }
}

/// Paints the hue bar as one solid row per pixel.
fn paint_hue_bar(bounds: Bounds<Pixels>, window: &mut Window) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let rows = height.ceil().max(1.0) as usize;
    let row_height = height / rows as f32;

    for row in 0..rows {
        let hue = (row as f32 / rows as f32) * 360.0;
        let colour = hsv_to_rgb(f64::from(hue), 1.0, 1.0);
        let rect = Bounds {
            origin: point(
                bounds.origin.x,
                bounds.origin.y + px(row as f32 * row_height),
            ),
            size: size(px(width), px(row_height)),
        };
        window.paint_quad(fill(rect, Color::rgb(colour.r, colour.g, colour.b)));
    }
}

/// Window-space point to element-local `(x, y)` in logical pixels.
fn local(bounds: &Bounds<Pixels>, position: gpui::Point<Pixels>) -> (f32, f32) {
    (
        f32::from(position.x - bounds.origin.x),
        f32::from(position.y - bounds.origin.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_squares_endpoints_are_white_and_the_pure_hue() {
        // Row geometry: the top row is value 1 (white at sat 0, pure hue at sat 1) and the
        // bottom row is value ~0 (black at both ends). That is the WPF composite's
        // behaviour and the reason the per-row approximation is exact rather than a guess.
        let rows = 180usize;
        let hue = 210.0;
        let top_value = 1.0 - 0.5 / rows as f32;
        let top_left = hsv_to_rgb(hue, 0.0, f64::from(top_value));
        assert!(top_left.r > 250 && top_left.g > 250 && top_left.b > 250);

        let bottom_value = 1.0 - (rows as f32 - 0.5) / rows as f32;
        let bottom = hsv_to_rgb(hue, 1.0, f64::from(bottom_value));
        assert!(bottom.r <= 2 && bottom.g <= 2 && bottom.b <= 2);
    }

    #[test]
    fn the_hue_bar_ends_where_the_csharp_gradient_does() {
        // The XAML gradient runs red → yellow → green → cyan → blue → magenta → red, so
        // both ends must be red and the middle cyan-ish.
        let top = hsv_to_rgb(0.0, 1.0, 1.0);
        assert_eq!((top.r, top.g, top.b), (255, 0, 0));
        let half = hsv_to_rgb(180.0, 1.0, 1.0);
        assert_eq!((half.r, half.g, half.b), (0, 255, 255));
    }

    #[test]
    fn thumb_positions_stay_inside_their_tracks() {
        for (sat, val) in [(0.0, 1.0), (1.0, 0.0), (0.5, 0.5)] {
            let x = (sat as f32) * SV_WIDTH - SV_THUMB / 2.0;
            let y = (1.0 - val as f32) * SV_HEIGHT - SV_THUMB / 2.0;
            assert!((-SV_THUMB / 2.0..=SV_WIDTH - SV_THUMB / 2.0).contains(&x));
            assert!((-SV_THUMB / 2.0..=SV_HEIGHT - SV_THUMB / 2.0).contains(&y));
        }
    }

    #[test]
    fn the_pointer_maps_to_element_local_pixels() {
        let bounds = Bounds {
            origin: point(px(40.0), px(60.0)),
            size: size(px(220.0), px(180.0)),
        };
        assert_eq!(local(&bounds, point(px(40.0), px(60.0))), (0.0, 0.0));
        assert_eq!(local(&bounds, point(px(150.0), px(150.0))), (110.0, 90.0));
    }

    // ── The press becomes a value ───────────────────────────────────────────────
    //
    // These are the tests the wiring was missing: the painters were geometry and the
    // callback carried no app, so nothing could turn a press into a colour. The state
    // machine is real, so the value asserted is the one a caller's `drag_sv`/`drag_hue`
    // would produce, not a number this file made up.

    use std::sync::{Arc, Mutex};

    use gpui::{Modifiers, MouseButton, Render, TestAppContext, VisualTestContext, WindowOptions};

    use crate::dialogs::color_picker::{rgb_to_hsv, ColorPickerState};

    /// Which widget the probe draws.
    #[derive(Clone, Copy)]
    enum Widget {
        Square,
        Hue,
    }

    /// One widget over a real state machine, recording every colour it produced.
    struct PickerProbe {
        state: ColorPickerState,
        widget: Widget,
        colours: Arc<Mutex<Vec<Rgb>>>,
        releases: Arc<Mutex<usize>>,
    }

    impl Render for PickerProbe {
        fn render(
            &mut self,
            _window: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            let hsv = self.state.hsv();
            let colours = self.colours.clone();
            let releases = self.releases.clone();
            let pressed = cx.entity();
            let released = pressed.clone();

            let widget = match self.widget {
                Widget::Square => sv_square(
                    "test-sv",
                    hsv,
                    move |x, y, _window, cx| {
                        pressed.update(cx, |probe, cx| {
                            probe.state.drag_sv(x, y);
                            colours
                                .lock()
                                .expect("colours")
                                .push(probe.state.selected());
                            cx.notify();
                        });
                    },
                    move |_window, cx| {
                        released.update(cx, |probe, cx| {
                            probe.state.end_sv();
                            *releases.lock().expect("releases") += 1;
                            cx.notify();
                        });
                    },
                ),
                Widget::Hue => hue_bar(
                    "test-hue",
                    hsv,
                    move |y, _window, cx| {
                        pressed.update(cx, |probe, cx| {
                            probe.state.drag_hue(y);
                            colours
                                .lock()
                                .expect("colours")
                                .push(probe.state.selected());
                            cx.notify();
                        });
                    },
                    move |_window, cx| {
                        released.update(cx, |probe, cx| {
                            probe.state.end_hue();
                            *releases.lock().expect("releases") += 1;
                            cx.notify();
                        });
                    },
                ),
            };

            // The wrapper is the widget's own box, so the bounds a test reads are the
            // rectangle the press lands in.
            let (width, height) = match self.widget {
                Widget::Square => (SV_WIDTH, SV_HEIGHT),
                Widget::Hue => (HUE_WIDTH, HUE_HEIGHT),
            };
            div()
                .w(px(width))
                .h(px(height))
                .debug_selector(|| String::from("test-picker-widget"))
                .child(widget)
        }
    }

    struct Probe {
        visual: VisualTestContext,
        colours: Arc<Mutex<Vec<Rgb>>>,
        releases: Arc<Mutex<usize>>,
    }

    impl Probe {
        fn widget_box(&mut self) -> Bounds<Pixels> {
            self.visual
                .debug_bounds("test-picker-widget")
                .expect("the widget records its bounds in a debug build")
        }

        fn colours(&self) -> Vec<Rgb> {
            self.colours.lock().expect("colours").clone()
        }

        fn releases(&self) -> usize {
            *self.releases.lock().expect("releases")
        }
    }

    /// A point at a fraction of the widget's own box.
    fn at(bounds: Bounds<Pixels>, x: f32, y: f32) -> gpui::Point<Pixels> {
        point(
            bounds.origin.x + px(f32::from(bounds.size.width) * x),
            bounds.origin.y + px(f32::from(bounds.size.height) * y),
        )
    }

    fn probe(cx: &mut TestAppContext, widget: Widget) -> Probe {
        let colours: Arc<Mutex<Vec<Rgb>>> = Arc::new(Mutex::new(Vec::new()));
        let releases: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let (recorded, released) = (colours.clone(), releases.clone());
        let entity = cx.update(|cx| {
            cx.new(|_| PickerProbe {
                // Pure red: hue 0, full saturation, full value.
                state: ColorPickerState::new(Rgb::new(255, 0, 0)),
                widget,
                colours: recorded,
                releases: released,
            })
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| entity.clone())
                .expect("test window")
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        Probe {
            visual,
            colours,
            releases,
        }
    }

    #[gpui::test]
    fn a_press_inside_the_square_reports_a_colour(cx: &mut TestAppContext) {
        let mut probe = probe(cx, Widget::Square);
        let widget = probe.widget_box();

        probe.visual.simulate_mouse_down(
            at(widget, 0.25, 0.75),
            MouseButton::Left,
            Modifiers::default(),
        );
        probe.visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let colours = probe.colours();
        assert_eq!(colours.len(), 1, "one press, one value");
        assert_ne!(
            colours[0],
            Rgb::new(255, 0, 0),
            "the press is a colour, not the one the state already had"
        );
        // Read back through the state: a quarter across and three quarters down is a
        // quarter saturated and a quarter bright.
        let hsv = rgb_to_hsv(colours[0]);
        assert!(
            hsv.sat < 0.5,
            "a press near the left edge desaturates: {}",
            hsv.sat
        );
        assert!(
            hsv.val < 0.5,
            "a press near the bottom darkens: {}",
            hsv.val
        );
    }

    #[gpui::test]
    fn a_drag_reports_a_sequence_and_a_release(cx: &mut TestAppContext) {
        let mut probe = probe(cx, Widget::Square);
        let widget = probe.widget_box();

        probe.visual.simulate_mouse_down(
            at(widget, 0.1, 0.5),
            MouseButton::Left,
            Modifiers::default(),
        );
        probe.visual.simulate_mouse_move(
            at(widget, 0.5, 0.5),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        probe.visual.simulate_mouse_move(
            at(widget, 0.9, 0.5),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        probe.visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        probe.visual.simulate_mouse_up(
            at(widget, 0.9, 0.5),
            MouseButton::Left,
            Modifiers::default(),
        );

        let colours = probe.colours();
        assert_eq!(
            colours.len(),
            3,
            "the press and both moves are values, not one: {colours:?}"
        );
        let saturations: Vec<f64> = colours.iter().map(|rgb| rgb_to_hsv(*rgb).sat).collect();
        assert!(
            saturations.windows(2).all(|pair| pair[1] > pair[0]),
            "moving right saturates: {saturations:?}"
        );
        assert!(saturations[0] < 0.2, "the press was at the left edge");
        assert_eq!(probe.releases(), 1, "the release ends the drag");
    }

    #[gpui::test]
    fn a_press_on_the_hue_bar_reports_a_new_hue(cx: &mut TestAppContext) {
        let mut probe = probe(cx, Widget::Hue);
        let widget = probe.widget_box();

        probe.visual.simulate_mouse_down(
            at(widget, 0.5, 0.75),
            MouseButton::Left,
            Modifiers::default(),
        );
        probe.visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let colours = probe.colours();
        assert_eq!(colours.len(), 1, "one press, one value");
        assert_ne!(
            colours[0],
            Rgb::new(255, 0, 0),
            "three quarters down the bar is not hue 0"
        );
        let hue = rgb_to_hsv(colours[0]).hue;
        assert!(
            (hue - 270.0).abs() < 5.0,
            "y = 0.75 * 180 is 270 degrees, got {hue}"
        );
    }
}
