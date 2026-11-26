//! One-key capture for the keybind and action-bind buttons.
//!
//! This is the managed `InputHooks.StartKeyCapture` / `OnKeyCaptured` pair
//! (`Aoko/Core/InputHooks.cs:241-251`, `:364-369`) with a widget around it: a press
//! starts listening, the next key press ends the capture and reports its virtual key,
//! and Escape reports `0`, because `InputHooks_OnKeyCaptured` writes
//! `finalVk = (vkCode == 0x1B) ? 0 : vkCode` — *"ESC unbinds"* (`MainWindow.xaml.cs:2042`).
//! A capture that ends without a key (focus moved away, or [`KeybindCapture::cancel`])
//! reports [`Captured::Abandoned`], which is the one outcome the managed code cannot
//! express: a low-level hook never loses focus.
//!
//! # What this can and cannot capture
//!
//! * **Keyboard only.** The managed capture passed `allowMouse: true` and the hook
//!   reported `VK_LBUTTON`..`VK_XBUTTON2` (`0x01`-`0x06`). A mouse press here is the press
//!   that *starts* the capture, and there is no second mouse press to report without
//!   guessing which click the user meant, so mouse binds stay unproducible and the
//!   formatter keeps naming them.
//! * **Modifier keys are not capturable**, and that is the platform rather than a choice:
//!   the Linux backends drop a modifier press before it becomes a `Keystroke`
//!   (`gpui_linux/src/linux/x11/client.rs:1163-1165`), so no event arrives to report.
//!   [`crate::views::keybind_text::virtual_key`] documents the same gap from the other
//!   side.
//! * **An unmapped key name is swallowed and the capture continues.** A name with no
//!   virtual key (dead keys, media keys, `fn`) would otherwise end the capture with
//!   nothing to store; waiting for a mappable key is closer to the managed behaviour,
//!   where the hook only ever sent vk codes.
//!
//! # The button is the component's, not a second one
//!
//! `components::button_sized` draws the box and the label; this type only wraps it with a
//! focus handle, the key listener and the press that starts a capture. The width is the
//! caller's: 140 for a card header's keybind button (`MainWindow.xaml:359`), 210 for an
//! action bind (`:525`, `:542`).
//!
//! # Mounting
//!
//! Nothing in the shell mounts this yet (decision 7): the 38 keybind buttons and the two
//! action binds still render their documented-inert handler. The call site is one line per
//! button, in `Shell::render_card_header` / `render_control`'s `ActionBind` arm: build the
//! entity with `KeybindCapture::new`, `.on_capture(...)` into the runtime's
//! `set_module_enabled`-style write, and pass `.focus_handle()` where the shell currently
//! draws `keybind_button`.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    div, App, Context, ElementId, FocusHandle, KeyDownEvent, MouseButton, Render, SharedString,
    Subscription, Window,
};

use crate::components::{button_sized, ButtonVariant};
use crate::theme::Theme;
use crate::views::keybind_text;

/// What a finished capture produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Captured {
    /// A key was pressed: the profile's virtual key. `0` is Escape, which unbinds.
    Key(i32),
    /// The capture ended without a key, so the stored binding is unchanged.
    Abandoned,
}

impl Captured {
    /// The virtual key, when there is one to store.
    ///
    /// `Some(0)` is a real value — the managed unbind — so a caller that writes the
    /// profile must treat it as "clear this key", not as "no change".
    #[must_use]
    pub fn virtual_key(self) -> Option<i32> {
        match self {
            Self::Key(vk) => Some(vk),
            Self::Abandoned => None,
        }
    }
}

type CaptureHandler = Arc<dyn Fn(Captured, &mut Window, &mut App)>;

/// A button that captures one keystroke when pressed.
pub struct KeybindCapture {
    theme: Theme,
    focus: FocusHandle,
    name: SharedString,
    width: f32,
    rest: SharedString,
    pending: SharedString,
    capturing: bool,
    handler: Option<CaptureHandler>,
    /// The blur listener, replaced on every `start`. It is not dropped when a capture
    /// finishes: dropping a subscription from inside its own callback would be the
    /// re-entrancy this deliberately avoids, and the `capturing` guard already makes a
    /// stale listener a no-op.
    blur: Option<Subscription>,
}

impl KeybindCapture {
    /// A button labelled `rest`, reading `pending` while it is capturing.
    ///
    /// The two labels are the caller's copy: `Bind: Unbound` / `Bind: [Press key or
    /// mouse...]` for a module, `Action: Unbound` / `Action: [Press key or mouse...]` for an
    /// action bind (`MainWindow.xaml.cs:2135-2154`).
    pub fn new(
        cx: &mut Context<Self>,
        theme: Theme,
        name: impl Into<SharedString>,
        width: f32,
        rest: impl Into<SharedString>,
        pending: impl Into<SharedString>,
    ) -> Self {
        Self {
            theme,
            focus: cx.focus_handle(),
            name: name.into(),
            width,
            rest: rest.into(),
            pending: pending.into(),
            capturing: false,
            handler: None,
            blur: None,
        }
    }

    /// Where a finished capture is reported.
    ///
    /// The callback takes the window and app so it can reach the entity that owns the
    /// binding, the same shape as `components::slider`'s and `text_input`'s.
    #[must_use]
    pub fn on_capture(
        mut self,
        handler: impl Fn(Captured, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.handler = Some(Arc::new(handler));
        self
    }

    /// Whether a capture is listening.
    #[must_use]
    pub fn is_capturing(&self) -> bool {
        self.capturing
    }

    /// The focus handle, for a caller that needs to move focus explicitly.
    #[must_use]
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The label this button draws right now.
    #[must_use]
    pub fn label(&self) -> &SharedString {
        if self.capturing {
            &self.pending
        } else {
            &self.rest
        }
    }

    /// Starts listening, and focuses the button so keys reach it.
    pub fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.capturing {
            return;
        }

        self.capturing = true;
        window.focus(&self.focus, cx);
        // Registered after the focus move, so the listener exists for the blur that ends
        // this capture rather than for the one that preceded it.
        self.blur = Some(cx.on_blur(&self.focus, window, |capture, window, cx| {
            capture.finish(Captured::Abandoned, window, cx);
        }));
    }

    /// Ends the capture without a key: the binding is unchanged.
    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish(Captured::Abandoned, window, cx);
    }

    /// Handles one key press. Returns whether the key was consumed.
    ///
    /// Consumed means the capture owned the key: it either finished on it or swallowed a
    /// name it cannot map, so a letter typed while capturing cannot fall through to a
    /// shortcut behind the button.
    pub fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.capturing {
            return false;
        }

        let key = event.keystroke.key.as_str();
        if key.eq_ignore_ascii_case("escape") {
            // `MainWindow.xaml.cs:2042`: ESC unbinds rather than cancelling.
            self.finish(Captured::Key(0), window, cx);
            return true;
        }

        if let Some(vk) = keybind_text::virtual_key(key) {
            self.finish(Captured::Key(vk), window, cx);
        }
        true
    }

    fn finish(&mut self, result: Captured, window: &mut Window, cx: &mut Context<Self>) {
        if !self.capturing {
            return;
        }

        self.capturing = false;
        if let Some(handler) = self.handler.clone() {
            handler(result, window, cx);
        }
    }
}

impl Render for KeybindCapture {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let label = self.label().clone();
        let width = self.width;
        let name = self.name.clone();
        let focus = self.focus.clone();

        let press = cx.entity();
        let keys = cx.entity();
        let selector = name.clone();

        div()
            // The id is required before `track_focus`: focus tracking and key listeners live
            // on `StatefulInteractiveElement`, the same ordering `components::scroll` needs
            // for `overflow_y_scroll`.
            .id(ElementId::Name(SharedString::from(format!("{name}-focus"))))
            .track_focus(&self.focus)
            .cursor_pointer()
            .debug_selector(move || format!("{selector}-capture"))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx);
                press.update(cx, |capture, cx| capture.start(window, cx));
                cx.stop_propagation();
            })
            .on_key_down(move |event, window, cx| {
                let handled = keys.update(cx, |capture, cx| capture.handle_key(event, window, cx));
                if handled {
                    cx.stop_propagation();
                }
            })
            .child(button_sized(
                ElementId::Name(name),
                &theme,
                ButtonVariant::Dark,
                label,
                width,
                // The press that starts a capture is the wrapper's mouse-down; the button's
                // own click is inert on purpose, so a click cannot report a key.
                |_, _, _| {},
            ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use gpui::{
        AnyWindowHandle, Entity, Keystroke, Modifiers, TestAppContext, VisualTestContext,
        WindowOptions,
    };

    use super::*;

    /// A window whose only content is one capture button.
    struct CaptureProbe {
        capture: Entity<KeybindCapture>,
    }

    impl Render for CaptureProbe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .items_center()
                .child(self.capture.clone())
        }
    }

    /// The window under test plus everything the capture reported, so a test needs no
    /// lookups by type.
    struct Probe {
        visual: VisualTestContext,
        window: AnyWindowHandle,
        capture: Entity<KeybindCapture>,
        results: Arc<Mutex<Vec<Captured>>>,
    }

    impl Probe {
        fn capturing(&self) -> bool {
            self.visual
                .cx
                .read(|cx| self.capture.read(cx).is_capturing())
        }

        fn results(&self) -> Vec<Captured> {
            self.results.lock().expect("results lock").clone()
        }

        /// A press on the rendered button, which is what starts a capture.
        fn press(&mut self) {
            let bounds = self
                .visual
                .debug_bounds("test-bind-capture")
                .expect("the capture button records its bounds in a debug build");
            self.visual
                .simulate_click(bounds.center(), Modifiers::default());
        }

        fn key(&mut self, name: &'static str) {
            let keystroke = Keystroke::parse(name).expect("keystroke");
            self.visual.cx.dispatch_keystroke(self.window, keystroke);
        }

        fn draw(&mut self) {
            self.visual.update(|window, cx| {
                let _ = window.draw(cx);
            });
        }
    }

    fn probe(cx: &mut TestAppContext) -> Probe {
        let results: Arc<Mutex<Vec<Captured>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = results.clone();
        let (probe, capture) = cx.update(|cx| {
            let capture = cx.new(|cx| {
                KeybindCapture::new(
                    cx,
                    Theme::slate(),
                    "test-bind",
                    140.0,
                    "Bind: Unbound",
                    "Bind: [Press key or mouse...]",
                )
                .on_capture(move |result, _, _| {
                    recorded.lock().expect("results lock").push(result);
                })
            });
            let probe = cx.new(|_| CaptureProbe {
                capture: capture.clone(),
            });
            (probe, capture)
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| probe.clone())
                .expect("test window")
        });
        let handle: AnyWindowHandle = window.into();
        let mut visual = VisualTestContext::from_window(handle, cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        Probe {
            visual,
            window: handle,
            capture,
            results,
        }
    }

    #[gpui::test]
    fn a_press_then_a_keystroke_reports_the_virtual_key(cx: &mut TestAppContext) {
        let mut probe = probe(cx);

        assert!(!probe.capturing(), "a fresh button is not listening");
        probe.press();
        assert!(probe.capturing(), "the press started a capture");
        assert_eq!(
            probe
                .visual
                .cx
                .read(|cx| probe.capture.read(cx).label().to_string()),
            "Bind: [Press key or mouse...]",
            "the managed pending copy is what the button reads while capturing"
        );

        probe.key("f1");

        assert_eq!(
            probe.results(),
            [Captured::Key(0x70)],
            "the module key F1 is reported once"
        );
        assert!(!probe.capturing(), "one key ends the capture");
    }

    #[gpui::test]
    fn escape_reports_the_managed_unbind(cx: &mut TestAppContext) {
        let mut probe = probe(cx);
        probe.press();

        probe.key("escape");

        assert_eq!(
            probe.results(),
            [Captured::Key(0)],
            "escape is the managed `finalVk = 0`, which the label reads as Unbound"
        );
        assert!(!probe.capturing());
    }

    #[gpui::test]
    fn losing_focus_abandons_the_capture(cx: &mut TestAppContext) {
        let mut probe = probe(cx);
        probe.press();
        assert!(probe.capturing());

        // Focus listeners are dispatched inside `Window::draw`
        // (`crates/gpui/src/window.rs:3938-3993`), not by `Window::blur` (`:2785-2800`),
        // which only clears the focus and marks the frame dirty. So the press's focus move
        // needs a frame to be recorded, and the blur needs one to be delivered.
        probe.draw();
        probe.visual.update(|window, cx| window.blur(cx));
        probe.draw();

        assert_eq!(
            probe.results(),
            [Captured::Abandoned],
            "a capture that loses focus stores nothing"
        );
        assert!(!probe.capturing());
    }

    #[gpui::test]
    fn a_keystroke_without_a_capture_reports_nothing(cx: &mut TestAppContext) {
        let mut probe = probe(cx);

        probe.key("a");

        assert!(
            probe.results().is_empty(),
            "an unfocused button must not report the window's keys"
        );
    }

    #[test]
    fn a_captured_key_is_the_value_the_profile_stores() {
        // The callback's payload is what the shell will write, so `Some(0)` is the unbind
        // and `None` is "nothing to write" rather than an empty key.
        assert_eq!(Captured::Key(0x70).virtual_key(), Some(0x70));
        assert_eq!(Captured::Key(0).virtual_key(), Some(0));
        assert_eq!(Captured::Abandoned.virtual_key(), None);
    }
}
