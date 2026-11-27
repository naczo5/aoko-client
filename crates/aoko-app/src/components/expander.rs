//! Expanders.
//!
//! `Expander` with `IsExpanded` and a local `Style TargetType="Expander"`
//! (`MainWindow.xaml:1212`, used at `:1226`, `:1240`, `:1250`, `:1261`, `:1278`). This
//! is the one WPF control that maps onto GPUI directly: a `bool` plus conditional
//! children, which the inventory calls a "good fit".
//!
//! # Motion
//!
//! WPF's `Expander` has no animation: the style at `:1212` sets `Foreground` and nothing
//! else, so the managed body appeared in one frame. A disclosure that pops reads as a
//! broken layout next to the switch and the slider, so the body gets an entrance — a
//! fade plus a [`motion::EXPANDER_SLIDE`] rise over [`motion::EXPANDER_BODY`], the
//! client choice documented on that constant.
//!
//! The direction that could not be ported is the close: collapsing removes the body from
//! the tree, and an element that no longer exists cannot animate. Doing it properly needs
//! the shell to keep the body mounted for the exit and notify when it finishes — the same
//! shape as the overlay's close, which is likewise unported. The entrance is gated twice,
//! as every animated site is: [`motion::motion`] returns `Motion::STILL` when the client
//! setting is off, and `with_animation` collapses to its end state under GPUI's own
//! `App::reduce_motion`, which the capture harness sets.
//!
//! The marker is a glyph swap (`▸` → `▾`), not a rotation: there is no transform on a
//! text element at the pin, and a rotated glyph would need the drawn path.

use gpui::prelude::*;
use gpui::{
    div, px, Animation, AnimationExt, AnyElement, App, ClickEvent, ElementId, IntoElement,
    SharedString, Window,
};

use crate::theme::motion::{self, Motion};
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
    // The title is also the body's animation id: `with_animation` keys its clock on that id,
    // so two expanders on one tab must not share it. Titles are the unique thing a call site
    // passes ("Combat and targeting", "Auto block", …), and a container id is not available
    // here because the header already takes `id`.
    let title: SharedString = title.into();

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
            .child(title.clone()),
    );

    if is_expanded {
        element = element.child(body_frame(title, body));
    }

    element.into_any_element()
}

/// The body, inset and animated.
///
/// The inset is the call site's `Margin="8,6,0,0"` (`MainWindow.xaml:1227`, `:1241`,
/// `:1251`, `:1262`, `:1279`), which every managed expander writes identically, so it
/// belongs to the control rather than to five copies. It was 14 px on the left, which is
/// neither of the two numbers the XAML uses and left the body further in than its own
/// header's marker.
fn body_frame(title: SharedString, body: impl IntoElement) -> AnyElement {
    let frame = div().pl(px(8.0)).pt(px(6.0)).child(body);

    let gate: Motion = motion::motion();
    if !gate.enabled() {
        return frame.into_any_element();
    }

    frame
        .with_animation(
            ElementId::Name(format!("expander-{title}-body").into()),
            Animation::new(gate.duration(motion::EXPANDER_BODY)),
            |element, t| {
                element
                    .opacity(t)
                    // `relative().top()` moves the paint box without moving the layout, so the
                    // body rises into place instead of the card resizing under it.
                    .relative()
                    .top(px(motion::EXPANDER_SLIDE * (1.0 - t)))
            },
        )
        .into_any_element()
}
