//! Anchored popups.
//!
//! `anchored()` gives an element that repositions itself towards the window when its
//! natural placement does not fit, which covers what the inventory flagged as
//! missing (§5.7). `Anchored::attach(target, corner)` is preferred over
//! `Anchored::position` at this revision: with `attach`, a dropdown that does not fit
//! below its trigger moves above it, and the offset flips with it. The inventory's
//! recommendation to "clamp to the window rect yourself" is therefore not needed for
//! the dropdown case, and doing it by hand would fight the built-in flip.
//!
//! `deferred(..)` wraps the result so a popup paints above later siblings, which is
//! the same z-order trick the C# got from `Popup` being a separate layer.
//!
//! # Motion: the slide the C# asked for
//!
//! `DarkComboBox` declares `PopupAnimation="Slide"` on its `Popup` (`App.xaml:72`), so the
//! managed dropdown slid in as it opened and this one has to as well. The entrance is an
//! opacity fade plus a [`motion::POPUP_SLIDE`] drop over [`motion::POPUP_IN`]: the popup is
//! built only while the list is open, so the animation is an entrance on a fresh element
//! and needs no state from the caller. `relative().top()` moves the paint box rather than
//! the layout, which matters because the element is `anchored()` — a real margin would move
//! the anchor's own bounds and fight the flip.
//!
//! **Unmounted at the time of writing:** [`popup`] has no caller. The shell's
//! `Control::Combo` arm cycles to the next option on a click instead of opening the
//! `MaxHeight="200"` list (`shell.rs`, the `Control::Combo` arm), so the entrance here is
//! ready for that wiring rather than visible today.

use gpui::prelude::*;
use gpui::{
    anchored, deferred, div, px, Anchor, Animation, AnimationExt, AnyElement, Bounds, ElementId,
    IntoElement, Pixels,
};

use crate::theme::motion::{self, Motion};

/// A popup anchored to the bottom-left of its trigger.
pub fn popup(trigger: Bounds<Pixels>, child: impl IntoElement) -> AnyElement {
    let content = div().child(child);
    let gate: Motion = motion::motion();

    let content: AnyElement = if gate.enabled() {
        content
            .with_animation(
                // One dropdown at a time: a second `Popup` cannot be open while this one
                // is, so a constant id cannot collide.
                ElementId::Name("combo-popup-in".into()),
                Animation::new(gate.duration(motion::POPUP_IN)),
                |element, t| {
                    element
                        .opacity(t)
                        .relative()
                        .top(px(motion::POPUP_SLIDE * (1.0 - t)))
                },
            )
            .into_any_element()
    } else {
        content.into_any_element()
    };

    deferred(
        anchored()
            .anchor(Anchor::TopLeft)
            .attach(trigger, Anchor::BottomLeft)
            .child(content)
            .into_any_element(),
    )
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use crate::components::combo::{visible_rows, ITEM_HEIGHT};

    #[test]
    fn a_popup_never_shows_a_partial_row() {
        // 200 / 26 is 7.7, so seven rows fit and the eighth would be sliced.
        let rows = visible_rows(ITEM_HEIGHT);
        assert!(rows as f32 * ITEM_HEIGHT <= 200.0);
        assert!((rows + 1) as f32 * ITEM_HEIGHT > 200.0);
    }
}
