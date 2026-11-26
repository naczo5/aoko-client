//! Module and panel cards.
//!
//! `ControlCardStyle` (`MainWindow.xaml:53`): `PanelBrush` fill, radius 6, padding
//! 12, a 1 px `SliderBgBrush` edge that lifts to `SliderFgBrush` on hover. There are
//! 49 card call sites, so this is the most reused container in the app.

use gpui::prelude::*;
use gpui::{div, px, Div};

use crate::theme::{metrics, Theme};

/// A card container. Add children with `.child(..)`.
#[must_use]
pub fn card(theme: &Theme) -> Div {
    let rest = theme.card_border();
    let hover = theme.card_border_hover();
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .rounded(px(metrics::radius::CARD))
        .border_1()
        .border_color(rest)
        .bg(theme.panel)
        .p(px(12.0))
        .hover(move |style| style.border_color(hover))
}
