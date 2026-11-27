//! Scrolling containers.
//!
//! `overflow_y_scroll()` comes from `StatefulInteractiveElement`, which needs an
//! `.id()` first, so the id is the component's argument rather than an afterthought
//! (`GPUI_API_NOTES.md` §5).
//!
//! # The scrollbar is not themed: the pin paints none
//!
//! `App.xaml:390-439` is a global `ScrollBar` style with both of its colours from the
//! palette — a 2 px track in `SliderBgBrush` at 45 % (`:399-403`) and a 4 px thumb in
//! `AccentBrush` at 92 % with a glow from `AccentColor` (`:415-422`), widening to 5 px and
//! full opacity on hover (`:431-434`). That is why a custom palette reaches the managed
//! scrollbar, and why its absence here is a palette-fidelity gap rather than a cosmetic
//! one.
//!
//! GPUI at rev 5526444 cannot draw it, and the gap is a missing *element*, not a missing
//! colour parameter:
//!
//! * `Styled::scrollbar_width` (`crates/gpui/src/styled.rs:68-71`) writes
//!   `Style::scrollbar_width` (`crates/gpui/src/style.rs:191-192`), and that field is read
//!   only by layout (`crates/gpui/src/taffy.rs:801`, `:876`). It reserves a gutter for a
//!   scrollbar; `Overflow::Scroll`'s own doc says the same (`style.rs:1219-1222`). Nothing
//!   paints in the reservation.
//! * `Style` has no scrollbar colour field at all — no track, thumb, hover or radius
//!   token — and no `Window` or `App` method supplies one, so there is no hook to fail to
//!   reach.
//! * GPUI itself draws no scrollbar anywhere at the pin. The only other scrollbar surface
//!   in the crate is `Platform::should_auto_hide_scrollbars`
//!   (`crates/gpui/src/platform.rs:629`; Windows answers it from
//!   `UISettings::AutoHideScrollBars`, `crates/gpui_windows/src/platform.rs:1041`,
//!   `:1719`), which is a preference query for a consumer that paints its own — zed's
//!   `ScrollManager` lives in its `editor` crate, not in `gpui`.
//!
//! So [`SCROLLBAR_WIDTH`] reserves the managed 4 px gutter and the gutter stays empty: no
//! thumb is drawn in the accent, a platform colour, or anything else. Matching
//! `App.xaml`'s bar needs a hand-drawn track and thumb positioned from a `ScrollHandle`'s
//! `offset`/`max_offset`/`bounds` (`crates/gpui/src/elements/div.rs:4355`, which is enough
//! geometry), with its own drag handling — and [`scroll`] takes no handle today, so that
//! is a change to this signature and to every caller. Recorded as a visible gap, not as a
//! match.

use gpui::prelude::*;
use gpui::{div, px, Div, ElementId, Stateful};

/// The managed bar's 4 px width (`App.xaml:391-392`), reserved as layout space.
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
