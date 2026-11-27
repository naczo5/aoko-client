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
    debug_selector: Option<&'static str>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let text = theme.text;

    let mut row = div()
        .id(id)
        .flex()
        .items_center()
        .cursor_pointer()
        .child(box_(theme, is_on))
        .child(
            div()
                .ml(px(metrics::checkbox::GAP))
                .text_size(px(metrics::text::CAPTION))
                .text_color(text)
                .child(label.into()),
        )
        .on_click(on_click);

    // On the node that already exists, never a wrapper: a wrapper is a paint node, and the
    // shell's frame budget counts those. `debug_selector` is a no-op outside a debug build,
    // so production pays nothing either.
    if let Some(selector) = debug_selector {
        row = row.debug_selector(move || selector.to_owned());
    }

    row.into_any_element()
}

/// The 14×14 box on its own, for a row whose label is a sibling instead of the checkbox's
/// own child.
///
/// The tracked-block row is that shape: the `CheckBox` at `MainWindow.xaml:698` has no
/// `Content`, and the block name is the next element in the `DockPanel`. Painting it through
/// [`checkbox`] with an empty label would add the 6 px label gap on top of the row's own
/// `Margin="0,0,8,0"`.
pub fn checkbox_bare(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    is_on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .cursor_pointer()
        .child(box_(theme, is_on))
        .on_click(on_click)
        .into_any_element()
}

/// The box itself, shared by [`checkbox`] and [`checkbox_bare`].
fn box_(theme: &Theme, is_on: bool) -> gpui::Div {
    let rest_edge = theme.slider_foreground;
    let hover_edge = theme.accent;
    let fill = if is_on {
        theme.slider_foreground
    } else {
        theme.slider_background
    };
    let accent = theme.accent;

    let mut square = div()
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
        square = square.child(
            div()
                .text_size(px(metrics::checkbox::GLYPH))
                .text_color(accent)
                .child(crate::components::glyph::CHECK),
        );
    }

    square
}
