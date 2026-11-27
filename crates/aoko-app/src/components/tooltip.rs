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
//! The surface itself is a deliberate departure from the managed one: the C# declared no
//! `ToolTip` style at all, so every one of those 71 sites painted WPF's default light
//! tooltip over a midnight UI. That is the C# being the bug, and the design brief's rule
//! is a dark glass surface with a hairline edge, which is what this paints.
//!
//! # Motion
//!
//! WPF's `ToolTip` defaults to `PopupAnimation="Fade"`, so the managed tooltips faded in
//! without a line of XAML saying so — a fade that the port owed and did not have. A
//! tooltip view is created when the tooltip is shown and dropped when it hides, so the
//! fade is an entrance on a fresh element and needs no state anywhere: the id is fixed,
//! the animation runs once, and [`Motion`] /
//! `App::reduce_motion` both collapse it to the settled frame.
//!
//! Usage — the builder is a value so a caller chains it once per element:
//!
//! ```ignore
//! div().id("inject").tooltip(components::tooltip(&theme, "Inject (Ctrl+I)"))
//! ```

use gpui::prelude::*;
use gpui::{
    div, px, Animation, AnimationExt, AnyView, App, Context, ElementId, IntoElement, Render,
    SharedString, Window,
};

use crate::theme::motion::{self, Motion};
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

    /// The painted box, without the entrance.
    fn surface(&self) -> gpui::Div {
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

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let surface = self.surface();
        let gate: Motion = motion::motion();
        if !gate.enabled() {
            return surface.into_any_element();
        }

        // Named after the text so two tooltips in one tree cannot share a clock. The
        // animator only touches opacity: a tooltip that also slid would move its own hit
        // area while the pointer is deciding whether to stay on the trigger.
        surface
            .with_animation(
                ElementId::Name(format!("tooltip-{}", self.text).into()),
                Animation::new(gate.duration(motion::TOOLTIP_FADE)),
                |element, t| element.opacity(t),
            )
            .into_any_element()
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
