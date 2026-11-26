//! The command-search overlay view and its rows.
//!
//! **Not mounted in Phase 2** (section 12 decision 7): [`overlay`] and [`row_element`]
//! have no caller. The shell installs no `on_key_down`, so Ctrl+K and type-to-open do not
//! exist and nothing draws this view; it is the painting half of a surface whose state
//! half ([`crate::views::command_search`]) is equally unreachable. Kept because the
//! geometry, the copy and the row policy are the expensive part, and a later round
//! mounts it rather than rewriting it.
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
//!
//! # Motion: there is no animation here to gate
//!
//! Decision 3 makes one toggle govern every animated site, and this module paints no
//! animation: the card, the scrim and every row are drawn at their settled size, colour
//! and opacity on every frame. The managed overlay had four (`theme::motion`:
//! `OVERLAY_CARD_IN` 140 ms at `CommandSearchOverlay.xaml.cs:150`, `OVERLAY_SCRIM` 180 ms
//! at `:153`, `OVERLAY_CLOSE` 100 ms at `:104`, `OVERLAY_RECARD` 120 ms at `:221`), all
//! opacity on `CardVisual` / `Scrim` plus a 6 px `CardSlide.Y`, and none of them is
//! ported. The only animated sites in this crate are `components::switch` and
//! `components::progress`, and both consult the gate the same way: each branches on
//! `motion.enabled()` and returns a static frame when it is off.
//!
//! So [`overlay`] takes a `Motion` and does not read it, which is the seam for the
//! entrance rather than a gate being bypassed: a user with animations off needs no special
//! path here, because the settled paint is what `Motion::STILL` asks for — and that is the
//! managed answer too, where `!ClientAreaAnimation` returns straight to the settled state
//! instead of playing anything (`:98-101` for the close, `:148-149` for the open). The
//! mount that ports the entrance must spend the seam the way the switch does: branch on
//! `motion.enabled()` for a static frame, or wrap the scrim and card in
//! `with_animation(id, Animation::new(motion.duration(OVERLAY_SCRIM)), ..)` and read
//! `motion.progress(t)` inside.
//!
//! `the_overlay_paints_the_same_card_under_either_motion_setting` is what catches an
//! entrance that forgets the gate: the managed card slides 6 px, so an ungated slide moves
//! the painted box and the test's bounds comparison fails.
//!
//! [`row_element`] takes no `Motion` at all: rows never animated in the managed overlay,
//! and a parameter nothing reads is worse than a signature the mount has to extend.

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
///
/// `_motion` is the animation gate, accepted and not read: this view has no animation to
/// gate yet, and the module docs say what the mount must do with it. It is named with a
/// leading underscore so the signature does not promise a behaviour the file does not
/// have.
pub fn overlay(
    theme: &Theme,
    state: &CommandSearchState,
    _motion: Motion,
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
        // The one stable handle on the card's box: the window test compares it under both
        // motion settings, and the mount asserts against it once the entrance exists.
        .debug_selector(|| String::from("command-search-card"))
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
///
/// No `Motion`: the managed overlay animated the card and the scrim, never a row, so this
/// painter is static by construction rather than by a gate that always says no.
pub fn row_element(
    theme: &Theme,
    row: &Row,
    index: usize,
    selected: bool,
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
    use gpui::{Context, Render, TestAppContext, VisualTestContext, WindowOptions};

    /// The overlay in a window, with the motion setting a test can flip.
    struct OverlayProbe {
        state: CommandSearchState,
        motion: Motion,
    }

    impl Render for OverlayProbe {
        fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            overlay(
                &Theme::slate(),
                &self.state,
                self.motion,
                window.viewport_size(),
                |_, _, _| {},
            )
        }
    }

    #[gpui::test]
    fn the_overlay_paints_the_same_card_under_either_motion_setting(cx: &mut TestAppContext) {
        let probe = cx.update(|cx| {
            cx.new(|_| OverlayProbe {
                state: CommandSearchState::new(),
                motion: Motion::ANIMATED,
            })
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| probe.clone())
                .expect("test window")
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let animated = visual
            .debug_bounds("command-search-card")
            .expect("the card records its box in a debug build");
        assert!(
            f32::from(animated.size.width) > 0.0 && f32::from(animated.size.height) > 0.0,
            "the card has a box to compare"
        );

        // Flip the gate the way the settings toggle does. Nothing in this view animates, so
        // the settled card is the same card: an entrance that lands without consulting the
        // gate is what this comparison catches, and under either setting what is asserted
        // here is the frame a user with animations off sees. GPUI's test frames expose
        // geometry rather than opacity, which is the limit of what this can check.
        visual.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe.motion = Motion::STILL;
                cx.notify();
            });
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let still = visual
            .debug_bounds("command-search-card")
            .expect("the card is still laid out");

        assert_eq!(
            (
                f32::from(animated.origin.x),
                f32::from(animated.origin.y),
                f32::from(animated.size.width),
                f32::from(animated.size.height),
            ),
            (
                f32::from(still.origin.x),
                f32::from(still.origin.y),
                f32::from(still.size.width),
                f32::from(still.size.height),
            ),
            "the gate changes nothing, because there is no animation here to gate"
        );
    }

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
