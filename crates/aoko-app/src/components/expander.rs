//! Expanders.
//!
//! `Expander` with `IsExpanded` and a local `Style TargetType="Expander"`
//! (`MainWindow.xaml:1212`, used at `:1226`, `:1240`, `:1250`, `:1261`, `:1278`). This
//! is the one WPF control that maps onto GPUI directly: a `bool` plus conditional
//! children, which the inventory calls a "good fit".

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, ClickEvent, IntoElement, SharedString, Window};

use crate::theme::{metrics, Theme};

/// A header row that shows or hides its body. `on_toggle` flips the caller's `bool`;
/// the body is built unconditionally, so keep it cheap.
pub fn expander(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    title: impl Into<SharedString>,
    is_expanded: bool,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    body: impl IntoElement,
) -> AnyElement {
    let marker = if is_expanded {
        crate::components::glyph::CHEVRON_EXPANDED
    } else {
        crate::components::glyph::CHEVRON_RIGHT
    };

    let mut element = div().flex().flex_col().gap(px(6.0)).child(
        div()
            .id(id)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .cursor_pointer()
            .text_size(px(metrics::text::SECTION_TITLE))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.text)
            .hover(|style| style.text_color(theme.accent))
            .on_click(on_toggle)
            .child(
                div()
                    .text_size(px(metrics::text::ESC))
                    .text_color(theme.dim_text)
                    .child(marker),
            )
            .child(title.into()),
    );

    if is_expanded {
        element = element.child(div().pl(px(14.0)).child(body));
    }

    element.into_any_element()
}
