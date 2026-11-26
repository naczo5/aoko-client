//! Buttons: the four variants the shell uses.
//!
//! The C# has `DarkButton` (`App.xaml:329`) plus three tab variants
//! (`TabStyles.xaml:29`, `:68`, `:114`) and one inline style; the shared values are
//! ported here so a call site picks a variant instead of restyling a button.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, ClickEvent, FontWeight, SharedString, Window};

use crate::theme::{metrics, Color, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonVariant {
    /// `TabPrimaryButton`: accent fill, background-coloured label. The C# uses
    /// `BgBrush` for the label because white on the accent is 3.95:1 and fails AA.
    Primary,
    /// `TabSecondaryButton`: slider-background fill, text label, 1 px edge that
    /// turns accent on hover.
    Secondary,
    /// `TabDangerButton`: no fill at rest, critical label, a 0.10 critical wash on
    /// hover.
    Danger,
    /// `DarkButton` (`App.xaml:329`): the everyday button. Its hover is an accent overlay at full
    /// opacity with a white label, which is what the template's `IsMouseOver`
    /// trigger settles on.
    Dark,
}

/// Resolved colours and metrics for one variant.
pub struct Spec {
    pub background: Color,
    pub label: Color,
    pub border: Option<Color>,
    pub radius: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub font_size: f32,
    pub weight: FontWeight,
    pub hover_background: Color,
    pub hover_label: Color,
    pub hover_border: Option<Color>,
    pub pressed_opacity: f32,
}

impl ButtonVariant {
    #[must_use]
    pub fn spec(self, theme: &Theme) -> Spec {
        match self {
            Self::Primary => Spec {
                background: theme.accent,
                label: theme.background,
                border: None,
                radius: metrics::radius::BUTTON_TAB,
                padding_x: 14.0,
                padding_y: 7.0,
                font_size: metrics::text::BUTTON,
                weight: FontWeight::MEDIUM,
                hover_background: theme.accent,
                hover_label: theme.background,
                hover_border: None,
                // `TabStyles.xaml:25` hover 0.88, `:28` pressed 0.72.
                pressed_opacity: 0.72,
            },
            Self::Secondary => Spec {
                background: theme.slider_background,
                label: theme.text,
                border: Some(theme.slider_foreground),
                radius: metrics::radius::BUTTON_TAB,
                padding_x: 12.0,
                padding_y: 6.0,
                font_size: metrics::text::BUTTON_SMALL,
                weight: FontWeight::MEDIUM,
                hover_background: theme
                    .slider_background
                    .blend(theme.accent, metrics::wash::HOVER),
                hover_label: theme.text,
                hover_border: Some(theme.accent),
                pressed_opacity: 0.8,
            },
            Self::Danger => Spec {
                background: Color::TRANSPARENT,
                label: crate::theme::fixed::CRITICAL,
                border: None,
                radius: metrics::radius::BUTTON_TAB,
                padding_x: 12.0,
                padding_y: 6.0,
                font_size: metrics::text::BUTTON_SMALL,
                weight: FontWeight::SEMIBOLD,
                hover_background: crate::theme::fixed::CRITICAL.with_alpha(0x1A),
                hover_label: crate::theme::fixed::CRITICAL,
                hover_border: None,
                pressed_opacity: 0.8,
            },
            Self::Dark => Spec {
                background: theme.slider_background,
                label: theme.text,
                border: None,
                radius: metrics::radius::BUTTON,
                padding_x: 8.0,
                padding_y: 4.0,
                font_size: metrics::text::CAPTION,
                weight: FontWeight::MEDIUM,
                // The template's hover overlay is the accent at opacity 1 (`App.xaml:354`).
                hover_background: theme.accent,
                hover_label: crate::theme::fixed::CONTRAST_LABEL,
                hover_border: None,
                pressed_opacity: 0.8,
            },
        }
    }
}

/// A button. Behaviour is click-only; no focus ring, no keyboard activation yet.
pub fn button(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    variant: ButtonVariant,
    label: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let spec = variant.spec(theme);
    let mut element = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .rounded(px(spec.radius))
        .px(px(spec.padding_x))
        .py(px(spec.padding_y))
        .bg(spec.background)
        .text_size(px(spec.font_size))
        .font_weight(spec.weight)
        .text_color(spec.label);

    if let Some(border) = spec.border {
        element = element.border_1().border_color(border);
    }

    let hover_background = spec.hover_background;
    let hover_label = spec.hover_label;
    let hover_border = spec.hover_border;
    let pressed = spec.pressed_opacity;

    // `hover` and `active` take style closures, so the lift is declarative and the
    // element keeps its identity across frames.
    let element = element
        .hover(move |style| {
            let style = style.bg(hover_background).text_color(hover_label);
            match hover_border {
                Some(border) => style.border_color(border),
                None => style,
            }
        })
        .active(move |style| style.opacity(pressed))
        .on_click(on_click);

    element.child(label.into()).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_use_tokens_not_literals() {
        let theme = Theme::slate();
        // Primary's label is the background colour, for the AA reason above.
        let primary = ButtonVariant::Primary.spec(&theme);
        assert_eq!(primary.label, theme.background);
        assert_eq!(primary.background, theme.accent);

        // Secondary's edge is the card hairline and its hover edge is the accent.
        let secondary = ButtonVariant::Secondary.spec(&theme);
        assert_eq!(secondary.border, Some(theme.slider_foreground));
        assert_eq!(secondary.hover_border, Some(theme.accent));

        // Danger never fills at rest and keeps the critical label.
        let danger = ButtonVariant::Danger.spec(&theme);
        assert_eq!(danger.background, Color::TRANSPARENT);
        assert_eq!(danger.label, crate::theme::fixed::CRITICAL);

        // Dark hover is the accent fill the C# storyboard reaches at opacity 1.
        let dark = ButtonVariant::Dark.spec(&theme);
        assert_eq!(dark.hover_background, theme.accent);
    }

    #[test]
    fn every_variant_is_a_distinct_shape_or_colour() {
        let theme = Theme::slate();
        let specs = [
            ButtonVariant::Primary.spec(&theme),
            ButtonVariant::Secondary.spec(&theme),
            ButtonVariant::Danger.spec(&theme),
            ButtonVariant::Dark.spec(&theme),
        ];
        // No two variants share a shape *and* a fill: Secondary and Dark both sit on
        // the slider-background but differ in radius and padding, which is why the key
        // is the pair and not either alone.
        for (index, spec) in specs.iter().enumerate() {
            for other in specs.iter().skip(index + 1) {
                assert!(
                    spec.background != other.background || spec.radius != other.radius,
                    "two button variants are indistinguishable"
                );
            }
        }
    }
}
