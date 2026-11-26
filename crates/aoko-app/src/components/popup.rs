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

use gpui::prelude::*;
use gpui::{anchored, deferred, Anchor, AnyElement, Bounds, IntoElement, Pixels};

/// A popup anchored to the bottom-left of its trigger.
pub fn popup(trigger: Bounds<Pixels>, child: impl IntoElement) -> AnyElement {
    deferred(
        anchored()
            .anchor(Anchor::TopLeft)
            .attach(trigger, Anchor::BottomLeft)
            .child(child.into_any_element())
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
