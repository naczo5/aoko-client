//! Text fields.
//!
//! **There is no text-field widget at the pin** (`UI_INVENTORY.md` row 9). This module
//! is the *visual* layer only: it draws the box, the text, the placeholder and — when
//! `focused` is passed in — a caret, and it reports clicks so an owner can move focus.
//!
//! For a field the user can actually type into, use [`super::text_input::TextInput`]:
//! an entity that implements `EntityInputHandler`, tracks its own focus and caret, and
//! reports edits through a callback. Reach for this module when the value is drawn but
//! not edited (a read-only row, the overlay's query line before the shell takes it
//! over, a capture).

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, ClickEvent, CursorStyle, IntoElement, SharedString, Window};

use crate::theme::{metrics, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldVariant {
    /// `TabTextBox` (`TabStyles.xaml:155`) and the colour picker's `HexBox`
    /// (`ColorPickerDialog.xaml:83`): panel fill, 1 px edge, accent edge on hover and
    /// on focus.
    Panel,
    /// The overlay's `QueryBox` (`CommandSearchOverlay.xaml:215-227`): no fill, no
    /// border, 18 px.
    Query,
}

impl FieldVariant {
    /// `pub(crate)` because [`super::text_input::TextInput`] paints the same two shapes
    /// and must not restate the sizes.
    pub(crate) fn font_size(self) -> f32 {
        match self {
            Self::Panel => 13.0,
            Self::Query => metrics::text::QUERY,
        }
    }
}

/// A field's visuals. `text` empty renders the placeholder.
pub fn field(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    variant: FieldVariant,
    text: &str,
    placeholder: &str,
    focused: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let rest_edge = theme.slider_background;
    let focus_edge = theme.accent;
    let accent = theme.accent;

    let mut element = div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(1.0))
        .text_size(px(variant.font_size()))
        .cursor(CursorStyle::IBeam)
        .on_click(on_click);

    element = match variant {
        FieldVariant::Panel => element
            .rounded(px(metrics::radius::CARD))
            .border_1()
            .bg(theme.panel)
            .px(px(8.0))
            .py(px(5.0))
            .border_color(if focused { focus_edge } else { rest_edge })
            .hover(move |style| style.border_color(focus_edge)),
        FieldVariant::Query => element.px(px(0.0)).py(px(0.0)),
    };

    if text.is_empty() {
        let placeholder_colour = match variant {
            FieldVariant::Panel => theme.dim_text,
            FieldVariant::Query => theme.search_placeholder(),
        };
        element = element.child(
            div()
                .text_color(placeholder_colour)
                .child(SharedString::from(placeholder.to_owned())),
        );
    } else {
        element = element.child(
            div()
                .text_color(theme.text)
                .child(SharedString::from(text.to_owned())),
        );
    }

    if focused {
        // The C# sets `CaretBrush` to the accent and an 18 px query field gets a tall
        // caret; blinking is a shell concern and is not implemented here.
        element = element.child(div().w(px(1.0)).h(px(variant.font_size() + 2.0)).bg(accent));
    }

    element.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_field_is_the_height_the_overlay_draws() {
        // 18 px is `CommandSearchOverlay.xaml:216`; the caret is two pixels taller.
        assert_eq!(FieldVariant::Query.font_size(), 18.0);
        assert_eq!(FieldVariant::Panel.font_size(), 13.0);
        assert!(FieldVariant::Query.font_size() + 2.0 <= 24.0);
    }
}
