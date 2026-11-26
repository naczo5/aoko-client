//! The command-search overlay view and its rows.
//!
//! Split by interface, as `UI_INVENTORY.md` §7 requires: **this module draws**, and
//! [`crate::views::command_search`] owns open/close, Ctrl+K, the query, ranking,
//! selection and the row actions. So there is no state here at all — the overlay is a
//! function of `&CommandSearchState` plus a click callback.
//!
//! Geometry comes from `views::command_search` (`card_width`, `card_top`,
//! `results_viewport_height`), which is the C# `LayoutCard` port, so the card's clamp
//! and its top ratio have one home rather than two.
//!
//! Three things are visible here and should not be "improved" back:
//!
//! - **Opaque card over a dark scrim** (UI decision 2). The C# card was `Panel` at 89 %
//!   alpha over a blurred tab control; GPUI cannot blur behind an element, so the card
//!   is the panel colour at full alpha and the blur is gone. `Theme::search_glass`
//!   keeps the C# value for the record; it is not what the card paints.
//! - **Two-stop scrim only.** The C# scrim is a two-stop vertical gradient
//!   (`CommandSearchOverlay.xaml:117-124`), which `Background::linear_gradient`
//!   reproduces exactly. The 760 px radial "moon haze" (`:132-140`) is dropped rather
//!   than faked with concentric divs, which bands.
//! - **Row washes are opaque blends.** The C# layered `TextBrush` at 8 % / 14 % over the
//!   card; over an opaque card that is exactly `blend(card, text, 0.08 / 0.14)`, so no
//!   extra element is needed and a light palette still darkens the row instead of
//!   washing it white.
//!
//! Copy is verbatim from the XAML: `up/down navigate`, `enter jump`, `f2 toggle` and
//! `UNAVAILABLE`. The keybind chip keeps `Consolas`'s role by using the bundled
//! monospace family through [`crate::assets::MONO_FAMILY`].

use gpui::prelude::*;
use gpui::{
    div, px, AnyElement, App, ClickEvent, Hsla, IntoElement, Pixels, SharedString, Size, Window,
};

use crate::assets::MONO_FAMILY;
use crate::components::field::FieldVariant;
use crate::components::{field, glyph, highlight_text, scroll, switch::switch_static};
use crate::theme::{fixed, metrics, motion::Motion, Theme};
use crate::views::command_search::{self, CommandSearchState, Row};

/// Header (accent line, query row, divider, footer) height taken off the results area.
const CHROME: f32 = 108.0;

/// The whole overlay: scrim, card, query row, results and legend.
///
/// `viewport` is `Window::viewport_size()`. Card width, card top and the results cap all
/// come from [`crate::views::command_search`], the C# `LayoutCard` port, so the clamp
/// has one home.
///
/// `on_row` receives the clicked row index; the caller owns what a click means (select,
/// jump, or toggle), and whether the overlay closes.
pub fn overlay(
    theme: &Theme,
    state: &CommandSearchState,
    motion: Motion,
    viewport: Size<Pixels>,
    on_row: impl Fn(usize, &ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let viewport_width = f32::from(viewport.width);
    let viewport_height = f32::from(viewport.height);
    let card_width = command_search::card_width(viewport_width);
    let card_top = command_search::card_top(viewport_height);
    let results_height = command_search::results_viewport_height(viewport_height, CHROME);

    let on_row = std::rc::Rc::new(on_row);
    let selected = state.selected_index();

    let mut results = scroll("command-search-results").max_h(px(results_height));
    for (index, row) in state.rows().iter().enumerate() {
        let callback = on_row.clone();
        results = results.child(row_element(
            theme,
            row,
            index,
            selected == Some(index),
            motion,
            move |event, window, cx| callback(index, event, window, cx),
        ));
    }

    if state.rows().is_empty() {
        results = results.child(
            div()
                .px(px(22.0))
                .py(px(20.0))
                .text_size(px(12.0))
                .text_color(theme.dim_text)
                .child(SharedString::from(state.empty_message())),
        );
    }

    let card = div()
        .flex()
        .flex_col()
        .w(px(card_width))
        .rounded(px(metrics::radius::OVERLAY_CARD))
        .border_1()
        .border_color(fixed::GLASS_HAIRLINE)
        // Decision 2: opaque card. `search_glass()` exists for the record only.
        .bg(theme.panel)
        .child(accent_line(theme))
        .child(query_row(theme, state))
        .child(
            div()
                .h(px(metrics::HAIRLINE))
                .w_full()
                .bg(theme.slider_background),
        )
        .child(results)
        .child(legend(theme, state));

    div()
        .size_full()
        .relative()
        // Scrim: the C# two-stop vertical gradient, darker at the bottom.
        .bg(gpui::linear_gradient(
            180.0,
            gpui::linear_color_stop(Hsla::from(fixed::SCRIM_TOP), 0.0),
            gpui::linear_color_stop(Hsla::from(fixed::SCRIM_BOTTOM), 1.0),
        ))
        .child(
            div()
                .absolute()
                .top(px(card_top))
                .left(px((viewport_width - card_width) / 2.0))
                .child(card),
        )
        .into_any_element()
}

/// The 2 px warm line at the top of the card: a 1 px specular highlight over a 1 px
/// accent line at 0.8 opacity (`CommandSearchOverlay.xaml:179-190`).
fn accent_line(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .child(div().h(px(1.0)).w_full().bg(fixed::GLASS_SPECULAR))
        .child(div().h(px(1.0)).w_full().bg(theme.accent).opacity(0.8))
        .into_any_element()
}

fn query_row(theme: &Theme, state: &CommandSearchState) -> AnyElement {
    let placeholder = if state.placeholder_visible() {
        "Search modules and settings"
    } else {
        ""
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .px(px(16.0))
        .py(px(14.0))
        .child(div().flex_1().child(field(
            "command-search-query",
            theme,
            FieldVariant::Query,
            state.query(),
            placeholder,
            true,
            |_, _, _| {},
        )))
        .child(
            // `EscChipStyle`: 4 px radius, slider-background fill, slider-foreground edge
            // that turns accent on hover, 10 px monospace cap.
            div()
                .ml(px(10.0))
                .rounded(px(metrics::radius::CHIP))
                .border_1()
                .border_color(theme.slider_foreground)
                .bg(theme.slider_background)
                .px(px(7.0))
                .py(px(2.0))
                .text_size(px(metrics::text::ESC))
                .font_family(MONO_FAMILY)
                .text_color(theme.search_subtle_text())
                .hover(move |style| style.border_color(theme.accent))
                .child(glyph::ESC),
        )
        .into_any_element()
}

/// One result row: 48 px, radius 7, inset 10×1, a hover wash and a stronger selected
/// wash, and an accent rule on the selected row.
pub fn row_element(
    theme: &Theme,
    row: &Row,
    index: usize,
    selected: bool,
    _motion: Motion,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let card = theme.panel;
    let selected_wash = card.blend(theme.text, metrics::overlay::ROW_SELECTED_WASH);
    let hover_wash = card.blend(theme.text, metrics::overlay::ROW_HOVER_WASH);
    let ranges: Vec<std::ops::Range<usize>> = row
        .name_ranges
        .iter()
        .map(|range| range.start..range.start + range.length)
        .collect();

    let keybind = row.state.keybind_text.clone();
    let unavailable = !row.is_available();

    let content = div()
        .flex()
        .flex_col()
        .flex_1()
        .overflow_hidden()
        .child(highlight_text(theme, row.display_name(), &ranges, 14.0))
        .child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.search_subtle_text())
                .truncate()
                .child(SharedString::from(row.entry.card_name)),
        );

    let mut element = div()
        .id(gpui::ElementId::Name(
            format!("command-search-row-{index}").into(),
        ))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(metrics::overlay::ROW_HEIGHT))
        .mx(px(metrics::overlay::ROW_INSET_X))
        .my(px(metrics::overlay::ROW_INSET_Y))
        .px(px(12.0))
        .rounded(px(metrics::radius::OVERLAY_ROW))
        .cursor_pointer()
        .bg(if selected { selected_wash } else { card })
        .hover(move |style| style.bg(hover_wash))
        .on_click(on_click);

    if selected {
        element = element.child(
            div()
                .w(px(2.0))
                .h(px(24.0))
                .rounded(px(1.0))
                .bg(theme.accent),
        );
    }

    element = element.child(if unavailable {
        div().flex_1().opacity(0.72).child(content)
    } else {
        div().flex_1().child(content)
    });

    if !unavailable && !keybind.is_empty() {
        element = element.child(
            div()
                .rounded(px(metrics::radius::CHIP))
                .border_1()
                .border_color(theme.slider_foreground)
                .bg(theme.slider_background)
                .px(px(6.0))
                .py(px(2.0))
                .text_size(px(metrics::text::ESC))
                .font_family(MONO_FAMILY)
                .text_color(theme.search_subtle_text())
                .child(SharedString::from(keybind)),
        );
    }

    if unavailable {
        element = element.child(
            div()
                .rounded(px(metrics::radius::CHIP))
                .border_1()
                .border_color(theme.slider_foreground)
                .px(px(6.0))
                .py(px(2.0))
                .text_size(px(9.0))
                .text_color(fixed::CRITICAL)
                .child("UNAVAILABLE"),
        );
    }

    if row.has_toggle() {
        element = element.child(
            div()
                .w(px(metrics::switch::TRACK_WIDTH))
                .flex()
                .justify_center()
                .child(switch_static(theme, row.is_on())),
        );
    }

    element.into_any_element()
}

/// The footer legend and the live-region result count summary.
fn legend(theme: &Theme, state: &CommandSearchState) -> AnyElement {
    let key = |text: &'static str| {
        div()
            .text_size(px(10.5))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.text)
            .child(text)
    };
    let rest = |text: &'static str| {
        div()
            .text_size(px(10.5))
            .text_color(theme.dim_text)
            .child(text)
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(22.0))
        .py(px(10.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(16.0))
                .child(
                    div()
                        .flex()
                        .gap(px(4.0))
                        .child(key("up/down"))
                        .child(rest(" navigate")),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.0))
                        .child(key("enter"))
                        .child(rest(" jump")),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.0))
                        .child(key("f2"))
                        .child(rest(" toggle")),
                ),
        )
        .child(
            div()
                .text_size(px(10.5))
                .text_color(theme.dim_text)
                .child(SharedString::from(state.results_summary())),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_is_never_narrower_than_the_footer_needs() {
        // The card comes from the shell builder's LayoutCard port; this pins the numbers
        // the overlay actually asks for at the product's window size.
        let width = command_search::card_width(metrics::window::WIDTH);
        assert_eq!(width, 620.0);
        assert!(width >= 340.0);
        // At the 560 px floor the card is 416 wide, which still clears the 340 minimum.
        assert_eq!(command_search::card_width(560.0), 416.0);
    }

    #[test]
    fn the_results_area_is_capped_and_never_collapses() {
        let height = command_search::results_viewport_height(760.0, CHROME);
        assert!(height > 0.0);
        assert!(
            height <= 300.0,
            "results area is {height}, above the C# cap"
        );
        // A short window hits the floor rather than showing nothing.
        assert!(command_search::results_viewport_height(200.0, CHROME) >= 100.0);
    }

    #[test]
    fn washes_stay_ordered_on_every_palette() {
        for name in Theme::builtin_names() {
            let theme = Theme::by_name(name);
            let card = theme.panel;
            let hover = card.blend(theme.text, metrics::overlay::ROW_HOVER_WASH);
            let selected = card.blend(theme.text, metrics::overlay::ROW_SELECTED_WASH);
            assert_ne!(hover, card, "{name}: hover wash is invisible");
            assert_ne!(selected, hover, "{name}: selected wash is not stronger");
        }
    }
}
