//! Tooltips.
//!
//! The C# has 71 tooltip sites across three files (`UI_INVENTORY.md` row 12), and it
//! sets `ToolTipService.ShowOnDisabled` explicitly so a refused action still explains
//! itself (`TabStyles.xaml:37`, `:76`, `:122`; `AccountsView.xaml:498`).
//!
//! **GPUI has no disabled state.** The port is therefore: keep the element
//! interactive, attach the tooltip, and let the shell refuse the click. That is
//! recorded here because "disabled button with a tooltip" is a thing the C# got for
//! free and this layer cannot offer.
//!
//! Usage — the builder is a value so a caller chains it once per element:
//!
//! ```ignore
//! div().id("inject").tooltip(components::tooltip(&theme, "Inject (Ctrl+I)"))
//! ```

use gpui::prelude::*;
use gpui::{div, px, AnyView, App, Context, IntoElement, Render, SharedString, Window};

use crate::theme::Theme;

/// The tooltip surface: panel fill, 1 px edge, 11 px caption.
pub struct Tooltip {
    text: SharedString,
    theme: Theme,
}

impl Tooltip {
    #[must_use]
    pub fn new(theme: &Theme, text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            theme: theme.clone(),
        }
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .rounded(px(4.0))
            .border_1()
            .border_color(self.theme.card_border())
            .bg(self.theme.panel)
            .px(px(6.0))
            .py(px(3.0))
            .text_size(px(11.0))
            .text_color(self.theme.text)
            .child(self.text.clone())
    }
}

/// A tooltip builder for `InteractiveElement::tooltip`, which is only available on an
/// element with an id.
pub fn tooltip(
    theme: &Theme,
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let view = Tooltip::new(theme, text);
    move |_window, cx| {
        let theme = view.theme.clone();
        let text = view.text.clone();
        AnyView::from(cx.new(move |_| Tooltip { text, theme }))
    }
}
