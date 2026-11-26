//! The combo trigger.
//!
//! `DarkComboBox` (`App.xaml:24-116`) is a `ToggleButton` plus a chevron, a `Popup`
//! with a `MaxHeight="200"` scroller and a separate item template. There is no
//! dropdown widget at the pin, so the trigger is here and the list is
//! [`super::popup`]; selection, keyboard movement and dismissal belong to the shell's
//! state.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, ClickEvent, IntoElement, SharedString, Window};

use crate::theme::{metrics, Theme};

/// Height cap of the dropdown list (`App.xaml:70-87`).
pub const LIST_MAX_HEIGHT: f32 = 200.0;
/// One item row's height, matching the tab-chip text metrics.
pub const ITEM_HEIGHT: f32 = 26.0;

pub fn combo(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    label: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let accent = theme.accent;
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .cursor_pointer()
        .rounded(px(metrics::radius::CARD))
        .border_1()
        .border_color(theme.slider_foreground)
        .bg(theme.slider_background)
        .px(px(12.0))
        .py(px(6.0))
        .text_size(px(metrics::text::BUTTON_SMALL))
        .text_color(theme.text)
        .hover(move |style| style.border_color(accent))
        .child(label.into())
        .child(
            div()
                .text_color(theme.dim_text)
                .text_size(px(metrics::text::ESC))
                .child(crate::components::glyph::CHEVRON_DOWN),
        )
        .on_click(on_click)
        .into_any_element()
}

/// How many rows a `MaxHeight="200"` list shows, given the row height.
#[must_use]
pub fn visible_rows(row_height: f32) -> usize {
    if row_height <= 0.0 {
        return 1;
    }
    ((LIST_MAX_HEIGHT / row_height).floor() as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_caps_at_two_hundred_pixels() {
        assert_eq!(visible_rows(ITEM_HEIGHT), 7);
        // A taller row shows fewer, never fewer than one.
        assert_eq!(visible_rows(200.0), 1);
        assert_eq!(visible_rows(500.0), 1);
        assert_eq!(visible_rows(0.0), 1);
        // The cap is the C# MaxHeight, not a guess.
        assert_eq!(LIST_MAX_HEIGHT, 200.0);
    }
}
