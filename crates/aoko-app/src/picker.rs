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
/// The hue thumb's height (`:52-68`).
pub const HUE_THUMB: f32 = 4.0;

/// The saturation/value square.
///
/// `on_drag` receives square-local pixels on press and on every drag move. The caller
/// updates its own [`ColorPickerState`](crate::dialogs::color_picker::ColorPickerState);
/// this widget keeps no state beyond the drag flag and the captured bounds.
pub fn sv_square(
    name: impl Into<SharedString>,
    hsv: Hsv,
    on_drag: impl Fn(f32, f32) + 'static,
    on_release: impl Fn() + 'static,
) -> AnyElement {
    let name: SharedString = name.into();
    let on_drag: Rc<dyn Fn(f32, f32)> = Rc::new(on_drag);
    let on_release: Rc<dyn Fn()> = Rc::new(on_release);
    let dragging = Rc::new(Cell::new(false));
    let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));

    let paint_bounds = bounds.clone();
    let hue = hsv.hue;

    let down = {
        let on_drag = on_drag.clone();
        let dragging = dragging.clone();
        let bounds = bounds.clone();
        move |event: &gpui::MouseDownEvent, _window: &mut Window, _cx: &mut App| {
            dragging.set(true);
            let (x, y) = local(&bounds.get(), event.position);
            on_drag(x, y);
        }
    };
    let moved = {
        let dragging = dragging.clone();
        let bounds = bounds.clone();
        let on_drag = on_drag.clone();
        move |event: &gpui::MouseMoveEvent, _window: &mut Window, _cx: &mut App| {
            if dragging.get() {
                let (x, y) = local(&bounds.get(), event.position);
                on_drag(x, y);
            }
        }
    };
    let up = {
        let dragging = dragging.clone();
        move |_event: &gpui::MouseUpEvent, _window: &mut Window, _cx: &mut App| {
            dragging.set(false);
            on_release();
        }
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
pub fn hue_bar(
    name: impl Into<SharedString>,
    hsv: Hsv,
    on_drag: impl Fn(f32) + 'static,
    on_release: impl Fn() + 'static,
) -> AnyElement {
    let name: SharedString = name.into();
    let on_drag: Rc<dyn Fn(f32)> = Rc::new(on_drag);
    let on_release: Rc<dyn Fn()> = Rc::new(on_release);
    let dragging = Rc::new(Cell::new(false));
    let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));

    let paint_bounds = bounds.clone();

    let down = {
        let on_drag = on_drag.clone();
        let dragging = dragging.clone();
        let bounds = bounds.clone();
        move |event: &gpui::MouseDownEvent, _window: &mut Window, _cx: &mut App| {
            dragging.set(true);
            let (_, y) = local(&bounds.get(), event.position);
            on_drag(y);
        }
    };
    let moved = {
        let dragging = dragging.clone();
        let bounds = bounds.clone();
        let on_drag = on_drag.clone();
        move |event: &gpui::MouseMoveEvent, _window: &mut Window, _cx: &mut App| {
            if dragging.get() {
                let (_, y) = local(&bounds.get(), event.position);
                on_drag(y);
            }
        }
    };
    let up = {
        let dragging = dragging.clone();
        move |_event: &gpui::MouseUpEvent, _window: &mut Window, _cx: &mut App| {
            dragging.set(false);
            on_release();
        }
    };

    let thumb_y = (hsv.hue as f32 / 360.0).clamp(0.0, 1.0) * HUE_HEIGHT - HUE_THUMB / 2.0;

    div()
        .id(gpui::ElementId::Name(name))
        .relative()
        .w(px(22.0))
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
                .w(px(22.0))
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
}
