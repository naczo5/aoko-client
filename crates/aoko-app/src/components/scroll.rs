//! Scrolling containers.
//!
//! `overflow_y_scroll()` comes from `StatefulInteractiveElement`, which needs an
//! `.id()` first, so the id is the component's argument rather than an afterthought
//! (`GPUI_API_NOTES.md` §5).
//!
//! What is not ported: the global 4 px scrollbar with a 2 px track and 4 px accent
//! thumb (`App.xaml:390-439`). `scrollbar_width` exists as a style helper and is set
//! to the measured 4 px, but there is no documented way at the pin to colour the
//! thumb, so the thumb keeps the platform colour. That is a visible, accepted
//! difference and it is recorded here rather than implied to be a match.

use gpui::prelude::*;
use gpui::{div, px, Div, ElementId, Stateful};

/// 4 px scrollbar (`App.xaml:399`).
pub const SCROLLBAR_WIDTH: f32 = 4.0;

/// A vertical scroll container. Add children with `.child(..)`.
pub fn scroll(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .size_full()
        .overflow_y_scroll()
        .scrollbar_width(px(SCROLLBAR_WIDTH))
}
