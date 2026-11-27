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
    /// `TabTextBox` (`TabStyles.xaml:155-201`): slider-background fill, a 1 px
    /// `SliderFgBrush` edge that turns accent on hover and on keyboard focus, 12 px text in
    /// 8,6 padding, radius 6.
    ///
    /// This is **not** the same box as the colour picker's hex entry, which is why
    /// [`Self::Hex`] exists: the two templates disagree on the fill, the edge, the padding
    /// and the font size, and an earlier revision of this file averaged them into one shape
    /// that matched neither — every tab field drew the picker's panel fill with a
    /// `SliderBgBrush` edge, which is a near-invisible hairline on a panel, instead of the
    /// C# field's visible `SliderFgBrush` edge.
    Panel,
    /// The colour picker's `HexBox` (`ColorPickerDialog.xaml:83-85`): `Padding="8,5"`,
    /// `FontSize="13"`, `PanelBrush` fill and a `SliderBgBrush` edge. It is deliberately the
    /// quieter of the two — the picker sits on the panel it matches, and the tab field sits
    /// on a card.
    Hex,
    /// The overlay's `QueryBox` (`CommandSearchOverlay.xaml:215-227`): no fill, no
    /// border, 18 px.
    Query,
}

impl FieldVariant {
    /// `pub(crate)` because [`super::text_input::TextInput`] paints the same shapes and must
    /// not restate the sizes.
    pub(crate) fn font_size(self) -> f32 {
        match self {
            Self::Panel => 12.0,
            Self::Hex => 13.0,
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
    // The two bordered variants differ in every one of these, which is the point of the
    // split: `TabStyles.xaml:155-201` against `ColorPickerDialog.xaml:83`.
    let rest_edge = match variant {
        FieldVariant::Panel => theme.slider_foreground,
        FieldVariant::Hex => theme.slider_background,
        FieldVariant::Query => theme.slider_background,
    };
    let fill = match variant {
        FieldVariant::Panel => theme.slider_background,
        FieldVariant::Hex => theme.panel,
        // `QueryBox` has no fill; the branch below never paints it, and a token keeps the
        // match total without inventing a colour.
        FieldVariant::Query => crate::theme::Color::TRANSPARENT,
    };
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
            .bg(fill)
            .px(px(8.0))
            // `Padding="8,6"` (`TabStyles.xaml:161`).
            .py(px(6.0))
            .border_color(if focused { focus_edge } else { rest_edge })
            .hover(move |style| style.border_color(focus_edge)),
        FieldVariant::Hex => element
            .rounded(px(metrics::radius::CARD))
            .border_1()
            .bg(fill)
            .px(px(8.0))
            // `Padding="8,5"` (`ColorPickerDialog.xaml:83`).
            .py(px(5.0))
            .border_color(if focused { focus_edge } else { rest_edge }),
        FieldVariant::Query => element.px(px(0.0)).py(px(0.0)),
    };

    if text.is_empty() {
        let placeholder_colour = match variant {
            FieldVariant::Panel | FieldVariant::Hex => theme.dim_text,
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
    fn the_three_variants_are_the_three_templates() {
        // `CommandSearchOverlay.xaml:216`: the query line is 18 px.
        assert_eq!(FieldVariant::Query.font_size(), 18.0);
        assert!(FieldVariant::Query.font_size() + 2.0 <= 24.0);
        // `TabStyles.xaml:159` is 12 px and `ColorPickerDialog.xaml:83` is 13; one number
        // cannot serve both, which is why the variants are two boxes.
        assert_eq!(FieldVariant::Panel.font_size(), 12.0);
        assert_eq!(FieldVariant::Hex.font_size(), 13.0);
    }
}
