//! The root view.
//!
//! **Builder 5 owns the body of this type.** The brief's boundary puts `main.rs` and an
//! explicit root type here so the binary has one window option set and one mount, and
//! the shell builder fills in the tab rail, the title bar, the views and the overlay
//! layer without also owning the process lifecycle.
//!
//! What is already settled for the shell, so it does not get re-decided in the body:
//!
//! - **One window at control geometry** (UI decision 4). The loader is an in-view
//!   state, not a second window; there is no destroy-and-recreate transition.
//! - **Client decorations we draw** (decision 5). The title bar is part of this view:
//!   a themed 32 px strip (see [`crate::theme::metrics::window::TITLE_BAR_HEIGHT`]),
//!   a drag region, and our own minimise/maximise/close hit-testing. It is client
//!   area, so it appears in a capture.
//! - **The palette comes from [`crate::theme::Theme`]**, never from a literal.
//! - **The animation gate is [`crate::theme::motion`]** (decision 3). The shell writes
//!   the client setting once and may also push it into
//!   `App::set_reduce_motion`, which exists at this pin and makes `AnimationExt`
//!   widgets render their end state even if a call site forgets the token.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context, IntoElement, Render, Window};

use crate::theme::Theme;

/// The root view.
pub struct Shell {
    theme: Theme,
}

impl Shell {
    /// A shell with the given starting palette.
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        Self { theme }
    }

    /// The active palette.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Replaces the palette, e.g. when the Settings tab changes it.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // A placeholder body: the background token, the title-bar strip and a marker so
        // a capture is not mistaken for a blank window. Builder 5 replaces this.
        let theme = self.theme.clone();
        let title_bar = crate::theme::metrics::window::TITLE_BAR_HEIGHT;
        let body: AnyElement = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .child(
                div()
                    .h(px(title_bar))
                    .w_full()
                    .bg(theme.panel)
                    .border_b(px(crate::theme::metrics::HAIRLINE))
                    .border_color(theme.card_border())
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .text_size(px(crate::theme::metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child("Aoko"),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(crate::theme::metrics::text::SECTION_TITLE))
                    .text_color(theme.dim_text)
                    .child("shell placeholder — Builder 5 owns this view"),
            )
            .into_any_element();
        body
    }
}
