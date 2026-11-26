//! The injection target picker's pixels: `Aoko/WindowPickerDialog.xaml` over
//! [`WindowPickerState`].
//!
//! **Not mounted in Phase 2** (section 12 decision 7). The managed dialog is opened by
//! `SelectInjectionTarget` (`MainWindow.xaml.cs:1477-1489`) before `RunInjectionAsync`; the
//! Rust injection path has no picker call site yet, so this view is reachable only from its
//! own test. Mounting it needs a host — an overlay child of the shell — plus that call site,
//! which is the shell's wiring rather than this file's.
//!
//! # Deviations, stated rather than implied
//!
//! * **No `MessageBox`.** The managed refusal paths (`NoSelection`, `UnidentifiedProcess`)
//!   opened a modal box. A modal is a second window, which decision 4 rules out, so the
//!   same sentence is drawn inside the dialog and the confirmation becomes an inline
//!   "Inject anyway" button that calls
//!   [`confirm_inject`](WindowPickerState::confirm_inject). The state machine, not the
//!   view, still decides: the button only forwards `true`.
//! * **The version dropdown cycles on click.** `components::combo` at the pin is a
//!   click-only control with no popup list, which is the same shape the Settings tab's
//!   combos use. A press moves to the next of [`VERSION_CHOICES`].
//! * **No two-line label styling.** `DiscoveryTarget::display_label` contains a `\n`
//!   between the process line and its version line; the view splits on it and draws two
//!   Divs rather than relying on newline shaping inside one text run.
//!
//! The list, the selection, the version and every refusal come from the state machine in
//! [`crate::dialogs::window_picker`]; this file lays them out and forwards presses.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, Context, Entity, MouseButton, Render, SharedString, Window};

use crate::components::{button, button_sized, combo, scroll, ButtonVariant};
use crate::dialogs::window_picker::{
    InjectDecision, Refusal, WindowPickerState, CANCEL_LABEL, INJECT_LABEL, REFRESH_LABEL,
    SUBTITLE, TITLE, VERSION_CHOICES, VERSION_LABEL,
};
use crate::theme::{metrics, Theme};

/// `Width="620"` / `Height="500"` (`WindowPickerDialog.xaml:6-7`).
pub const DIALOG_WIDTH: f32 = 620.0;
pub const DIALOG_HEIGHT: f32 = 500.0;
/// The dialog's `Grid Margin="16"` (`:10`).
const MARGIN: f32 = 16.0;
/// `Width="90"` on Cancel and `Width="160"` on Inject (`:117-131`).
const CANCEL_WIDTH: f32 = 90.0;
const INJECT_WIDTH: f32 = 160.0;
/// `Width="160"` on the version combo (`:94`).
const VERSION_WIDTH: f32 = 160.0;

/// What a press in the dialog asks the host to do.
///
/// The view never injects, re-discovers or closes itself: it reports, and the host owns the
/// consequences. `Inject(InjectDecision::Accepted { .. })` is the one outcome that must
/// close the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowPickerAction {
    /// The user pressed Cancel: keep the caller's current target.
    Cancel,
    /// Re-enumerate: the host calls [`WindowPickerView::set_targets`] with the result.
    Refresh,
    /// Inject was pressed; the decision carries the refusal or the accepted target.
    Inject(InjectDecision),
}

type ActionHandler = Arc<dyn Fn(WindowPickerAction, &mut Window, &mut App)>;

/// The dialog, as a GPUI entity.
pub struct WindowPickerView {
    state: WindowPickerState,
    theme: Theme,
    /// The refusal currently drawn, if any. Cleared by any press that changes the selection.
    notice: Option<Refusal>,
    on_action: Option<ActionHandler>,
}

impl WindowPickerView {
    /// A picker over an already-discovered list, as the managed constructor was.
    pub fn new(theme: Theme, targets: Vec<aoko_platform::DiscoveryTarget>) -> Self {
        Self {
            state: WindowPickerState::new(targets),
            theme,
            notice: None,
            on_action: None,
        }
    }

    /// Called with what the press asked for.
    #[must_use]
    pub fn on_action(
        mut self,
        handler: impl Fn(WindowPickerAction, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_action = Some(Arc::new(handler));
        self
    }

    /// The state under the view.
    #[must_use]
    pub fn state(&self) -> &WindowPickerState {
        &self.state
    }

    /// Replaces the list after a refresh, which is `RefreshWindowList`.
    pub fn set_targets(
        &mut self,
        targets: Vec<aoko_platform::DiscoveryTarget>,
        cx: &mut Context<Self>,
    ) {
        self.state.refresh(targets);
        self.notice = None;
        cx.notify();
    }

    fn report(&mut self, action: WindowPickerAction, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handler) = self.on_action.clone() {
            handler(action, window, cx);
        }
    }

    fn inject(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let decision = self.state.request_inject();
        self.notice = match &decision {
            InjectDecision::Refused(refusal) | InjectDecision::NeedsConfirmation(refusal) => {
                Some(*refusal)
            }
            InjectDecision::Accepted { .. } => None,
        };
        self.report(WindowPickerAction::Inject(decision), window, cx);
        cx.notify();
    }

    /// The inline "yes" for an `UnidentifiedProcess` question: the managed
    /// `MessageBoxResult.OK` path.
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let decision = self.state.confirm_inject(true);
        self.notice = None;
        self.report(WindowPickerAction::Inject(decision), window, cx);
        cx.notify();
    }

    fn render_row(&self, index: usize, theme: &Theme, entity: &Entity<Self>) -> AnyElement {
        let Some(target) = self.state.targets().get(index) else {
            return div().into_any_element();
        };

        let label = target.display_label();
        let (first, second) = match label.split_once('\n') {
            Some((first, second)) => (first.to_owned(), second.to_owned()),
            None => (label, String::new()),
        };
        let selected = self.state.selected_index() == Some(index);
        let fill = if selected {
            theme.tab_selected
        } else {
            theme.panel
        };
        let handle = entity.clone();

        div()
            .id(SharedString::from(format!("window-picker-row-{index}")))
            .debug_selector(move || format!("window-picker-row-{index}"))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .w_full()
            .px(px(4.0))
            .py(px(4.0))
            .bg(fill)
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                handle.update(cx, |view, cx| {
                    view.state.select(Some(index));
                    view.notice = None;
                    cx.notify();
                });
            })
            .child(
                div()
                    .text_size(px(metrics::text::SECTION_TITLE))
                    .text_color(theme.text)
                    .child(SharedString::from(first)),
            )
            .child(
                div()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(SharedString::from(second)),
            )
            .into_any_element()
    }
}

impl Render for WindowPickerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let entity = cx.entity();
        let version = VERSION_CHOICES
            .get(self.state.version_index())
            .map_or("Auto-detect", |(label, _)| *label);

        let mut list = scroll("window-picker-list-scroll")
            .flex_col()
            .gap(px(2.0))
            .p(px(4.0));
        for index in 0..self.state.targets().len() {
            list = list.child(self.render_row(index, &theme, &entity));
        }

        let refresh = {
            let handle = entity.clone();
            div()
                .debug_selector(|| String::from("window-picker-refresh"))
                .child(button(
                    "window-picker-refresh",
                    &theme,
                    ButtonVariant::Dark,
                    SharedString::from(REFRESH_LABEL),
                    move |_, window, cx| {
                        handle.update(cx, |view, cx| {
                            view.notice = None;
                            view.report(WindowPickerAction::Refresh, window, cx);
                            cx.notify();
                        });
                    },
                ))
        };

        let version_combo = {
            let handle = entity.clone();
            div()
                .w(px(VERSION_WIDTH))
                .debug_selector(|| String::from("window-picker-version"))
                .child(combo(
                    "window-picker-version",
                    &theme,
                    SharedString::from(version),
                    move |_, _, cx| {
                        handle.update(cx, |view, cx| {
                            let next = (view.state.version_index() + 1) % VERSION_CHOICES.len();
                            view.state.set_version_index(next);
                            cx.notify();
                        });
                    },
                ))
        };

        let cancel = {
            let handle = entity.clone();
            div()
                .debug_selector(|| String::from("window-picker-cancel"))
                .child(button_sized(
                    "window-picker-cancel",
                    &theme,
                    ButtonVariant::Dark,
                    SharedString::from(CANCEL_LABEL),
                    CANCEL_WIDTH,
                    move |_, window, cx| {
                        handle.update(cx, |view, cx| {
                            view.report(WindowPickerAction::Cancel, window, cx);
                        });
                    },
                ))
        };
        let inject = {
            let handle = entity.clone();
            div()
                .debug_selector(|| String::from("window-picker-inject"))
                .child(button_sized(
                    "window-picker-inject",
                    &theme,
                    ButtonVariant::Dark,
                    SharedString::from(INJECT_LABEL),
                    INJECT_WIDTH,
                    move |_, window, cx| {
                        handle.update(cx, |view, cx| view.inject(window, cx));
                    },
                ))
        };

        // The managed MessageBox, drawn in place: the refusal's own sentence, and the
        // confirmation's "yes" when the question is the unidentified-process one.
        let notice = self.notice.map(|refusal| {
            let mut column = div()
                .debug_selector(|| String::from("window-picker-notice"))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(metrics::text::CAPTION))
                        .text_color(theme.text)
                        .child(SharedString::from(refusal.title())),
                )
                .child(
                    div()
                        .text_size(px(metrics::text::CAPTION))
                        .text_color(theme.dim_text)
                        .child(SharedString::from(refusal.message())),
                );

            if refusal == Refusal::UnidentifiedProcess {
                let handle = entity.clone();
                column = column.child(
                    div()
                        .debug_selector(|| String::from("window-picker-confirm"))
                        .child(button(
                            "window-picker-confirm",
                            &theme,
                            ButtonVariant::Danger,
                            SharedString::from("Inject anyway"),
                            move |_, window, cx| {
                                handle.update(cx, |view, cx| view.confirm(window, cx));
                            },
                        )),
                );
            }

            column
        });

        let mut root = div()
            .id("window-picker")
            .debug_selector(|| String::from("window-picker"))
            .flex()
            .flex_col()
            .w(px(DIALOG_WIDTH))
            .h(px(DIALOG_HEIGHT))
            .p(px(MARGIN))
            .bg(theme.background)
            .child(
                div()
                    .debug_selector(|| String::from("window-picker-title"))
                    .text_size(px(16.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.text)
                    .mb(px(6.0))
                    .child(SharedString::from(TITLE)),
            )
            .child(
                div()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .mb(px(10.0))
                    .child(SharedString::from(SUBTITLE)),
            )
            .child(
                div()
                    .debug_selector(|| String::from("window-picker-list"))
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .rounded(px(metrics::radius::CARD))
                    .border_1()
                    .border_color(theme.slider_background)
                    .bg(theme.panel)
                    .p(px(4.0))
                    .child(list),
            )
            .child(div().mt(px(10.0)).child(refresh))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .mt(px(12.0))
                    .child(
                        div()
                            .text_size(px(metrics::text::SECTION_TITLE))
                            .text_color(theme.text)
                            .child(SharedString::from(VERSION_LABEL)),
                    )
                    .child(version_combo),
            );

        if let Some(notice) = notice {
            root = root.child(div().mt(px(12.0)).child(notice));
        }

        root.child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap(px(8.0))
                .mt(px(14.0))
                .child(cancel)
                .child(inject),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use aoko_platform::{ClientType, DiscoveryTarget};
    use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowOptions};

    use super::*;

    fn target(pid: u32, client: ClientType, confidence: u8) -> DiscoveryTarget {
        DiscoveryTarget {
            pid,
            window: None,
            title: format!("Minecraft {pid}"),
            process_name: "javaw.exe".to_owned(),
            client_type: client,
            detected_version: None,
            detection_reason: "test".to_owned(),
            confidence,
        }
    }

    /// A window whose content is the dialog, plus every action it reported.
    struct PickerProbe {
        view: Entity<WindowPickerView>,
    }

    impl Render for PickerProbe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.view.clone())
        }
    }

    struct Probe {
        visual: VisualTestContext,
        view: Entity<WindowPickerView>,
        actions: Arc<Mutex<Vec<WindowPickerAction>>>,
    }

    impl Probe {
        fn click(&mut self, selector: &'static str) {
            let bounds = self
                .visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} renders"));
            self.visual
                .simulate_click(bounds.center(), Modifiers::default());
        }

        fn selected_index(&self) -> Option<usize> {
            self.visual
                .cx
                .read(|cx| self.view.read(cx).state().selected_index())
        }

        fn version_index(&self) -> usize {
            self.visual
                .cx
                .read(|cx| self.view.read(cx).state().version_index())
        }

        fn actions(&self) -> Vec<WindowPickerAction> {
            self.actions.lock().expect("actions lock").clone()
        }
    }

    fn probe(cx: &mut TestAppContext, targets: Vec<DiscoveryTarget>) -> Probe {
        let actions: Arc<Mutex<Vec<WindowPickerAction>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = actions.clone();
        let (probe, view) = cx.update(|cx| {
            let view = cx.new(|_| {
                WindowPickerView::new(Theme::slate(), targets).on_action(move |action, _, _| {
                    recorded.lock().expect("actions lock").push(action);
                })
            });
            let probe = cx.new(|_| PickerProbe { view: view.clone() });
            (probe, view)
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| probe.clone())
                .expect("test window")
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        Probe {
            visual,
            view,
            actions,
        }
    }

    fn two_targets() -> Vec<DiscoveryTarget> {
        vec![
            target(11, ClientType::LunarClient, 95),
            target(12, ClientType::JavaProcess, 20),
        ]
    }

    /// The bare JVM first, so the default selection is the row that asks before injecting.
    fn unidentified_first() -> Vec<DiscoveryTarget> {
        vec![
            target(11, ClientType::JavaProcess, 20),
            target(12, ClientType::LunarClient, 95),
        ]
    }

    #[gpui::test]
    fn every_part_of_the_dialog_has_a_place_in_the_window(cx: &mut TestAppContext) {
        let mut probe = probe(cx, two_targets());

        for selector in [
            "window-picker",
            "window-picker-title",
            "window-picker-list",
            "window-picker-row-0",
            "window-picker-row-1",
            "window-picker-refresh",
            "window-picker-version",
            "window-picker-cancel",
            "window-picker-inject",
        ] {
            let bounds = probe
                .visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is laid out"));
            assert!(
                f32::from(bounds.size.width) > 0.0 && f32::from(bounds.size.height) > 0.0,
                "{selector} has a non-empty box"
            );
        }

        assert_eq!(probe.selected_index(), Some(0), "the first row is selected");
    }

    #[gpui::test]
    fn a_row_press_selects_and_the_version_combo_cycles(cx: &mut TestAppContext) {
        let mut probe = probe(cx, two_targets());

        probe.click("window-picker-row-1");
        assert_eq!(
            probe.selected_index(),
            Some(1),
            "the pressed row becomes the selection"
        );

        probe.click("window-picker-version");
        assert_eq!(
            probe.version_index(),
            1,
            "a press moves to the next version"
        );

        probe.click("window-picker-refresh");
        probe.click("window-picker-cancel");

        assert_eq!(
            probe.actions(),
            [WindowPickerAction::Refresh, WindowPickerAction::Cancel],
            "Refresh and Cancel are reported to the host, not acted on here"
        );
    }

    #[gpui::test]
    fn injecting_an_unidentified_process_asks_inline_then_accepts(cx: &mut TestAppContext) {
        let mut probe = probe(cx, unidentified_first());

        probe.click("window-picker-inject");

        assert_eq!(
            probe.actions(),
            [WindowPickerAction::Inject(
                InjectDecision::NeedsConfirmation(Refusal::UnidentifiedProcess)
            )],
            "the state machine asks rather than injecting"
        );
        assert!(
            probe.visual.debug_bounds("window-picker-notice").is_some(),
            "the question is drawn in the dialog, since a message box would be a second window"
        );

        probe.click("window-picker-confirm");

        assert_eq!(
            probe.actions().last().cloned(),
            Some(WindowPickerAction::Inject(InjectDecision::Accepted {
                target_index: 0,
                pid: 11,
                version: "auto",
            })),
            "confirming accepts the selected process and the default version"
        );
        assert!(
            probe.visual.debug_bounds("window-picker-notice").is_none(),
            "and the question goes away"
        );
    }
}
