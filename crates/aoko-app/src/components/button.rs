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
///
/// Content-sized: the box is the label plus the variant's padding and nothing else. A
/// call site that needs a pinned box uses [`button_sized`] (width) or [`button_pinned`]
/// (either axis); Rust has no default arguments, so each pin is its own entry point rather
/// than a nullable parameter — adding one here would have rewritten all 32 call sites for
/// a property one of them wants.
pub fn button(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    variant: ButtonVariant,
    label: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    build_button(id, theme, variant, label, None, None, on_click)
}

/// The same button at a fixed outer width, which is what the managed UI does with
/// `Width="140"` on the card header's keybind button (`MainWindow.xaml:359`).
///
/// The width is the **border-box** width, as WPF's `Width` is: the padding is inside
/// it, so 140 means 140 on screen and the label is centred in what the padding leaves.
/// The element refuses to shrink, so a crowded header row cannot squeeze the button back
/// to its label; hover and press only restyle fill, label, edge and opacity, so the width
/// holds in every state and for every [`ButtonVariant`].
pub fn button_sized(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    variant: ButtonVariant,
    label: impl Into<SharedString>,
    width: f32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    build_button(id, theme, variant, label, Some(width), None, on_click)
}

/// The same button with either axis — or both — pinned to the markup's own number.
///
/// WPF sets `Height` on two of the shell's buttons and no width on them: the Stats tab's
/// "Reset Stats" is `Padding="14,6" Height="40"` with a content width
/// (`MainWindow.xaml:1466-1467`). A pinned axis is the **border-box** size, as WPF's is,
/// and it refuses to shrink, exactly like [`button_sized`]'s width. `None` leaves that axis
/// content-sized, so `button_pinned(.., None, Some(40.0), ..)` is a tall button with the
/// label's own width.
pub fn button_pinned(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    variant: ButtonVariant,
    label: impl Into<SharedString>,
    width: Option<f32>,
    height: Option<f32>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    build_button(id, theme, variant, label, width, height, on_click)
}

fn build_button(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    variant: ButtonVariant,
    label: impl Into<SharedString>,
    width: Option<f32>,
    height: Option<f32>,
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

    if let Some(width) = width {
        element = element.w(px(width)).flex_shrink_0().overflow_hidden();
    }

    if let Some(height) = height {
        element = element.h(px(height)).flex_shrink_0().overflow_hidden();
    }

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

    /// Every rendered button, one per row, with the wrapper that measures it.
    ///
    /// The wrapper exists because `debug_bounds` is keyed by a debug selector and the
    /// component returns a type-erased `AnyElement` a test cannot decorate. The column
    /// uses `items_start`, so each wrapper is as wide as its button and nothing else —
    /// a shrink-to-fit parent cannot be wider than its only child, so the number below
    /// is the button's laid-out width, not the request.
    const CASES: [(&str, Option<f32>, Option<f32>, ButtonVariant, &str); 8] = [
        // The two content-sized cases use the variant the real call site uses
        // (`Shell::keybind_button`), so the un-sized number is the one the review
        // measured: 71 px of fill for "Keybind".
        ("natural", None, None, ButtonVariant::Secondary, "Keybind"),
        (
            "natural-longer",
            None,
            None,
            ButtonVariant::Secondary,
            "Keybind (F1)",
        ),
        (
            "sized",
            Some(140.0),
            None,
            ButtonVariant::Secondary,
            "Keybind",
        ),
        (
            "sized-primary",
            Some(140.0),
            None,
            ButtonVariant::Primary,
            "Keybind",
        ),
        (
            "sized-danger",
            Some(140.0),
            None,
            ButtonVariant::Danger,
            "Keybind",
        ),
        (
            "sized-dark",
            Some(140.0),
            None,
            ButtonVariant::Dark,
            "Keybind",
        ),
        (
            "sized-different-label",
            Some(140.0),
            None,
            ButtonVariant::Secondary,
            "Bind",
        ),
        // `MainWindow.xaml:1466-1467`: `Height="40"` with no width, the Stats tab's
        // "Reset Stats" button, which is the one call site that pins a height.
        (
            "pinned-height",
            None,
            Some(40.0),
            ButtonVariant::Dark,
            "Reset Stats",
        ),
    ];

    struct Probe;

    impl gpui::Render for Probe {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            let theme = Theme::slate();
            let mut column = div().flex().flex_col().items_start().gap(px(4.0));

            for (selector, width, height, variant, label) in CASES {
                let on_click = |_: &ClickEvent, _: &mut Window, _: &mut App| {};
                let element = match (width, height) {
                    (None, None) => button(selector, &theme, variant, label, on_click),
                    (Some(width), None) => {
                        button_sized(selector, &theme, variant, label, width, on_click)
                    }
                    (width, height) => {
                        button_pinned(selector, &theme, variant, label, width, height, on_click)
                    }
                };
                column = column.child(
                    div()
                        .debug_selector(move || selector.to_owned())
                        .flex()
                        .items_center()
                        .child(element),
                );
            }

            column
        }
    }

    /// The rendered width of one case, in the window the probe was drawn in.
    fn width_of(visual: &mut gpui::VisualTestContext, selector: &'static str) -> f32 {
        let bounds = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} was not painted"));
        f32::from(bounds.size.width)
    }

    /// The rendered height of one case.
    fn height_of(visual: &mut gpui::VisualTestContext, selector: &'static str) -> f32 {
        let bounds = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} was not painted"));
        f32::from(bounds.size.height)
    }

    fn probe_window() -> (gpui::VisualTestContext, gpui::TestAppContext) {
        let cx = gpui::TestAppContext::single();
        let window = cx.update(|cx| {
            let probe = cx.new(|_| Probe);
            cx.open_window(Default::default(), |_, _| probe.clone())
                .expect("test window")
        });
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &cx);
        // `debug_bounds` reads the rendered frame, so paint once before measuring.
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        (visual, cx)
    }

    #[test]
    fn a_sized_button_renders_at_its_width_and_a_plain_one_at_its_content() {
        let (mut visual, _cx) = probe_window();

        assert_eq!(
            width_of(&mut visual, "sized"),
            140.0,
            "the width is the border-box width: 140 on screen, padding inside"
        );
        // The same width whatever the label is doing inside it.
        assert_eq!(width_of(&mut visual, "sized-different-label"), 140.0);

        // Content-sized, unchanged from before this parameter existed: narrower than the
        // fixed one and wider when the label is longer.
        let natural = width_of(&mut visual, "natural");
        let longer = width_of(&mut visual, "natural-longer");
        assert!(
            natural < 140.0,
            "a plain button is its label, got {natural}"
        );
        assert!(
            longer > natural,
            "a plain button grows with its label: {natural} then {longer}"
        );
    }

    #[test]
    fn a_pinned_height_renders_at_its_height_and_a_plain_one_at_its_content() {
        let (mut visual, _cx) = probe_window();

        assert_eq!(
            height_of(&mut visual, "pinned-height"),
            40.0,
            "the height is the border-box height, as WPF's `Height` is"
        );

        // Content-sized on both axes, unchanged from before the height existed: shorter
        // than the pin, and a longer label does not move the height.
        let natural = height_of(&mut visual, "natural");
        let longer = height_of(&mut visual, "natural-longer");
        assert!(natural < 40.0, "a plain button is its label, got {natural}");
        assert_eq!(
            longer, natural,
            "a longer label changes the width only: {natural} then {longer}"
        );

        // And the pinned height holds for the dark variant that uses it, under the pointer.
        let bounds = visual
            .debug_bounds("pinned-height")
            .expect("pinned-height was not painted");
        let centre = gpui::point(
            bounds.origin.x + bounds.size.width / 2.0,
            bounds.origin.y + bounds.size.height / 2.0,
        );
        visual.simulate_mouse_move(centre, None, gpui::Modifiers::default());
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(
            height_of(&mut visual, "pinned-height"),
            40.0,
            "while hovered"
        );
    }

    #[test]
    fn the_width_holds_for_every_variant_and_under_the_pointer() {
        let (mut visual, _cx) = probe_window();

        for selector in ["sized-primary", "sized-danger", "sized-dark", "sized"] {
            assert_eq!(width_of(&mut visual, selector), 140.0, "{selector}");
        }

        // Hover and press are style states, not layout: the contract is that neither can
        // resize the button, so measure again with the pointer on it and held down.
        let bounds = visual.debug_bounds("sized").expect("sized button");
        let centre = gpui::point(
            bounds.origin.x + bounds.size.width / 2.0,
            bounds.origin.y + bounds.size.height / 2.0,
        );
        visual.simulate_mouse_move(centre, None, gpui::Modifiers::default());
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(width_of(&mut visual, "sized"), 140.0, "while hovered");

        visual.simulate_mouse_down(centre, gpui::MouseButton::Left, gpui::Modifiers::default());
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(width_of(&mut visual, "sized"), 140.0, "while pressed");
    }
}
