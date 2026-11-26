//! The small checkbox (`DarkCheckBox`, `App.xaml:119-153`).
//!
//! A 14×14 box, radius 4, `SliderBgBrush` fill, `SliderFgBrush` edge that turns
//! accent on hover, and a `✓` glyph in the accent colour when checked. There is no
//! `CheckBox` in `gpui`, so the glyph is a text child, exactly as the C# draws it.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, ClickEvent, IntoElement, SharedString, Window};

use crate::theme::{metrics, Theme};

pub fn checkbox(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    is_on: bool,
    label: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let rest_edge = theme.slider_foreground;
    let hover_edge = theme.accent;
    let fill = if is_on {
        theme.slider_foreground
    } else {
        theme.slider_background
    };
    let accent = theme.accent;

    let mut box_ = div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(metrics::checkbox::SIZE))
        .h(px(metrics::checkbox::SIZE))
        .rounded(px(metrics::radius::CHECKBOX))
        .border_1()
        .border_color(rest_edge)
        .bg(fill)
        .hover(move |style| style.border_color(hover_edge));

    if is_on {
        box_ = box_.child(
            div()
                .text_size(px(metrics::checkbox::GLYPH))
                .text_color(accent)
                .child(crate::components::glyph::CHECK),
        );
    }

    div()
        .id(id)
        .flex()
        .items_center()
        .cursor_pointer()
        .child(box_)
        .child(
            div()
                .ml(px(metrics::checkbox::GAP))
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.text)
                .child(label.into()),
        )
        .on_click(on_click)
        .into_any_element()
}
