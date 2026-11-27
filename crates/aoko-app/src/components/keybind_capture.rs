//! One-key capture for the keybind and action-bind buttons.
//!
//! This is the managed `InputHooks.StartKeyCapture` / `OnKeyCaptured` pair
//! (`Aoko/Core/InputHooks.cs:241-251`, `:364-369`) with a widget around it: a press
//! starts listening, the next press ends the capture and reports its key — a keystroke, or
//! for a mouse bind a second click on the same button — and Escape reports `0`, because
//! `InputHooks_OnKeyCaptured` writes `finalVk = (vkCode == 0x1B) ? 0 : vkCode` — *"ESC
//! unbinds"* (`MainWindow.xaml.cs:2042`). A capture that ends without a key (focus moved
//! away, or [`KeybindCapture::cancel`]) reports [`Captured::Abandoned`], which is the one
//! outcome the managed code cannot express: a low-level hook never loses focus.
//!
//! # What this can and cannot capture
//!
//! * **A mouse button is one of the managed pseudo-keys.** The managed capture passed
//!   `allowMouse: true` and its mouse hook reported `VK_LBUTTON`..`VK_XBUTTON2`
//!   (`Aoko/Core/InputHooks.cs:40-44`, `:429-448`), which is `0x01`, `0x02`, `0x04`, `0x05`
//!   and `0x06` — the five codes [`crate::views::keybind_text`] already names
//!   (`Mouse Left` … `Mouse X2`). A press on this button maps the same way:
//!   [`MouseButton::Left`] is `0x01`, `Right` `0x02`, `Middle` `0x04`, and the two side
//!   buttons `Navigate(Back)`/`Navigate(Forward)` are `0x05`/`0x06`.
//! * **The press that starts a capture is a gesture, not a key.** It is the one press that
//!   cannot also be reported, or the button could never start listening to a left click it
//!   would then have to call `Mouse Left`. So a mouse bind takes a *second* press, and
//!   [`KeybindCapture::handle_press`] is where the two meanings are separated: idle starts
//!   listening, listening reports the button. A press therefore does exactly one thing, which
//!   is what keeps it from starting and finishing the same capture. A double click is one
//!   gesture too, so its second click is neither a start nor a bind.
//!
//!   The managed hook captured only the middle and the two side buttons, because a left or
//!   right click still had to reach the game; here the press lands on this control, so all
//!   five are bindable and the starting press is the one excluded instead.
//! * **Modifier keys are not capturable**, and that is the platform rather than a choice:
//!   the Linux backends drop a modifier press before it becomes a `Keystroke`
//!   (`gpui_linux/src/linux/x11/client.rs:1163-1165`), so no event arrives to report.
//!   [`crate::views::keybind_text::virtual_key`] documents the same gap from the other
//!   side.
//! * **A press with no virtual key keeps listening and says so.** A bare modifier, a media
//!   key or `fn` has a name but no code to store, so it cannot end the capture. It used to be
//!   swallowed in silence, which is a button that looks broken: the press did nothing and
//!   nothing on screen said why. The capture still waits for a key it can store — the
//!   managed behaviour, where the hook only ever sent vk codes — and the button now reads the
//!   caller's own prefix followed by `[Not a bindable key]` until the next press.
//!
//! # The button is the component's, not a second one
//!
//! `components::button_sized` draws the box and the label; this type only wraps it with a
//! focus handle, the key listener and the mouse-press handling that both starts a capture and
//! reports a button as one. The width is the caller's: 140 for a card header's keybind button
//! (`MainWindow.xaml:359`), 210 for an action bind (`:525`, `:542`).
//!
//! # Mounting
//!
//! Mounted, all 40. `Shell::sync_keybind_captures` builds one capture entity per binding
//! before the tree is assembled, and `Shell::render_capture` paints it in the card header
//! (`Shell::render_card_header`) and in both arms of `Shell::render_control` that carry a
//! binding: `Control::Keybind` for the 38 module keys and `Control::ActionBind` for the two
//! action rows. The width is the XAML's, 140 for a header and 210 for an action bind.
//!
//! What a capture writes is not this type's business: it hands a [`Captured`] to the
//! caller's `on_capture` and keeps no key. `Shell::apply_captured_key` stores it on the
//! profile — `Profile::module_keys` for a module, `auto_rod_action_key` /
//! `throwpot_action_key` for an action bind — and `Shell::ensure_capture` rebuilds only the
//! button whose key changed, so the label on screen follows the profile rather than a
//! second copy of the number. `KeyTarget` names that field, which is what keeps the read
//! and the write on the same one.
//!
//! The capture's own scope is one key: it never reads or writes the profile, never decides
//! whether a key is free, and never keeps a key after the callback returns.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    div, App, Context, ElementId, FocusHandle, KeyDownEvent, MouseButton, NavigationDirection,
    Render, SharedString, Subscription, Window,
};

use crate::components::{button_sized, tooltip, ButtonVariant};
use crate::theme::Theme;
use crate::views::{copy, keybind_text};

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

/// What a capture reads when the press it just received has no virtual key to store.
///
/// No managed sentence exists for this: the low-level hook only ever sent vk codes, so a name it
/// could not map never arrived and the managed app never had to say anything. The string lives
/// here rather than with the callers because it is this state machine's own outcome. It is kept
/// as short as the pending copy it replaces — every button is fixed-width and clips its label —
/// so it reads whole on both the 140 px and the 210 px button.
const NOT_BINDABLE: &str = "[Not a bindable key]";

/// The managed virtual key of one mouse button, `VK_LBUTTON`..`VK_XBUTTON2`
/// (`Aoko/Core/InputHooks.cs:40-44`, `:429-448`).
///
/// The codes are not contiguous — `0x03` is the managed `Cancel` and names no button — which is
/// why [`crate::views::keybind_text::format_virtual_key`] names these five one by one instead of
/// computing them.
fn mouse_virtual_key(button: MouseButton) -> i32 {
    match button {
        MouseButton::Left => 0x01,
        MouseButton::Right => 0x02,
        MouseButton::Middle => 0x04,
        MouseButton::Navigate(NavigationDirection::Back) => 0x05,
        MouseButton::Navigate(NavigationDirection::Forward) => 0x06,
    }
}

/// The refusal label for a button whose pending copy is `pending`: the same prefix the caller
/// writes (`Bind` / `Action`) and [`NOT_BINDABLE`] after it.
///
/// The prefix is taken from the pending label rather than being a fourth argument, so the two
/// halves of one button's copy cannot drift apart. A label with no `": "` in it is used whole,
/// which is the only honest answer for copy this function was not written against.
fn refusal_label(pending: &str) -> String {
    let prefix = pending
        .split_once(": ")
        .map_or(pending, |(prefix, _)| prefix);
    format!("{prefix}: {NOT_BINDABLE}")
}

/// A button that captures one keystroke when pressed.
pub struct KeybindCapture {
    theme: Theme,
    focus: FocusHandle,
    name: SharedString,
    width: f32,
    rest: SharedString,
    pending: SharedString,
    /// [`refusal_label`] of `pending`: what the button reads after a press it cannot map.
    refused: SharedString,
    capturing: bool,
    /// Whether the press just received had no virtual key, so the pending label would leave the
    /// user guessing why nothing happened. Cleared when the capture ends or restarts.
    last_refused: bool,
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
        let pending = pending.into();
        Self {
            theme,
            focus: cx.focus_handle(),
            name: name.into(),
            width,
            rest: rest.into(),
            refused: SharedString::from(refusal_label(&pending)),
            pending,
            capturing: false,
            last_refused: false,
            handler: None,
            blur: None,
        }
    }

    /// Repaints the button in a new palette.
    ///
    /// The entity owns its own copy of the theme, taken when the shell built it, so a palette
    /// applied while the window is up reaches this button only through here.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
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
    ///
    /// Three states, not two: at rest the stored binding, while listening the caller's pending
    /// copy, and after a press with no virtual key the refusal that says why nothing was stored.
    #[must_use]
    pub fn label(&self) -> &SharedString {
        if !self.capturing {
            &self.rest
        } else if self.last_refused {
            &self.refused
        } else {
            &self.pending
        }
    }

    /// Whether the last press could not be mapped to a virtual key.
    #[must_use]
    pub fn is_refused(&self) -> bool {
        self.last_refused
    }

    /// Starts listening, and focuses the button so keys reach it.
    pub fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.capturing {
            return;
        }

        self.capturing = true;
        self.last_refused = false;
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

    /// Handles one mouse press, which either starts a capture or is the capture's input.
    ///
    /// The press that starts listening is a gesture and is not reported; a press that arrives
    /// while the button is listening is the binding, so it reports the pseudo-key of the button
    /// it was made with (`0x01`-`0x06`) and ends the capture. One press is one of the two, never
    /// both, so a click can neither bind itself by starting a capture nor start a second one.
    ///
    /// `click_count` is the event's, and the second or later click of one gesture is not a bind:
    /// a control people habitually double-click would otherwise leave the capture holding
    /// `Mouse Left`, which nobody asked for. A mouse bind is a click, then a separate click.
    pub fn handle_press(
        &mut self,
        button: MouseButton,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if click_count > 1 {
            return;
        }

        if self.capturing {
            self.finish(Captured::Key(mouse_virtual_key(button)), window, cx);
        } else {
            self.start(window, cx);
        }
    }

    /// Handles one key press. Returns whether the key was consumed.
    ///
    /// Consumed means the capture owned the key: it either finished on it or refused a name it
    /// cannot map, so a letter typed while capturing cannot fall through to a shortcut behind
    /// the button.
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
            return true;
        }

        // A name with no virtual key — a bare modifier, a media key, `fn` — has nothing to
        // store, so the capture keeps waiting instead of ending on an empty value. Saying so is
        // the difference between "waiting for the next key" and "this button is broken": it was
        // silent, which is a press that visibly did nothing. `last_refused` is a gate as well as
        // a label, so an auto-repeating refused key notifies once rather than every frame.
        if !self.last_refused {
            self.last_refused = true;
            cx.notify();
        }
        true
    }

    fn finish(&mut self, result: Captured, window: &mut Window, cx: &mut Context<Self>) {
        if !self.capturing {
            return;
        }

        self.capturing = false;
        self.last_refused = false;
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

        let keys = cx.entity();
        let press = cx.entity();
        let selector = name.clone();

        div()
            // The id is required before `track_focus`: focus tracking and key listeners live
            // on `StatefulInteractiveElement`, the same ordering `components::scroll` needs
            // for `overflow_y_scroll`.
            .id(ElementId::Name(SharedString::from(format!("{name}-focus"))))
            .track_focus(&self.focus)
            .cursor_pointer()
            // The only place the managed UI said Escape unbinds (`MainWindow.xaml:1005`), and it
            // says it on the control rather than in a help panel: the key that unbinds is not one
            // a user guesses, and a button that silently starts listening owes the instruction.
            // Attached here rather than by each caller so all 40 bindings get it from one place.
            .tooltip(tooltip(&theme, copy::KEYBIND_TOOLTIP))
            .debug_selector(move || format!("{selector}-capture"))
            // Any button, one listener: the handler reads which one from the event and decides
            // which of the two meanings the press has — start listening, or be the captured
            // button — so one press cannot be both.
            .on_any_mouse_down(move |event, window, cx| {
                press.update(cx, |capture, cx| {
                    capture.handle_press(event.button, event.click_count, window, cx);
                });
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
    //! Why every test here activates its window, and what a focus listener that never fires
    //! actually looks like.
    //!
    //! A focus listener needs three things at once, and the third is invisible when it is
    //! missing: the subscription must be active, the frame must be drawn, and the **window
    //! must be active**. The first two are what the obvious failure modes are about — a
    //! listener registered during a draw is `Pending` until its deferred activation runs
    //! (`crates/gpui/src/app/context.rs:576-587`, `src/subscription.rs:194-231`), and
    //! `Window::blur` only clears the focus and marks the frame dirty (`src/window.rs:2785-2800`),
    //! so the blur is delivered by the next draw rather than by the call. The third is the
    //! one that costs an afternoon, because a listener that is registered, active and
    //! invoked still reports nothing:
    //!
    //! * `Frame::new` starts with `window_active: false` (`crates/gpui/src/window.rs:1109`).
    //! * Each frame copies the platform's bit in: `next_frame.window_active = self.active.get()`
    //!   (`:3910`), and `self.active` is written only by the platform's activation callback
    //!   (`:2238`), not by opening a window.
    //! * The focus dispatch blanks both paths of the event for an inactive window
    //!   (`:3973-3981`): `previous_focus_path` and `current_focus_path` are each replaced by
    //!   `Default::default()` when their frame was not active.
    //! * `Context::on_blur` filters on `event.previous_focus_path.last() == Some(&focus_id)`
    //!   (`crates/gpui/src/app/context.rs:579-580`), so an empty path can never match.
    //!
    //! The result is indistinguishable, from the assertion's side, from a listener that was
    //! never registered: both produce an empty result list. Drawing more frames cannot help,
    //! which is why `Probe::activate` exists and why every test calls it from `probe()`.

    use std::sync::{Arc, Mutex};

    use gpui::{
        AnyWindowHandle, Entity, Keystroke, Modifiers, MouseDownEvent, TestAppContext,
        VisualTestContext, WindowOptions,
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

        /// The label the button paints, which is how a refusal reaches the user.
        fn label(&self) -> String {
            self.visual
                .cx
                .read(|cx| self.capture.read(cx).label().to_string())
        }

        /// A press on the rendered button, which is what starts a capture.
        fn press(&mut self) {
            self.press_button(MouseButton::Left);
        }

        /// One mouse-down on the rendered button with `button`: the press that starts a
        /// capture for a left press, or the captured input for any press while listening.
        fn press_button(&mut self, button: MouseButton) {
            let bounds = self
                .visual
                .debug_bounds("test-bind-capture")
                .expect("the capture button records its bounds in a debug build");
            self.visual
                .simulate_mouse_down(bounds.center(), button, Modifiers::default());
        }

        /// The second `MouseDown` of a double click: same point, `click_count` 2.
        fn second_click(&mut self) {
            let bounds = self
                .visual
                .debug_bounds("test-bind-capture")
                .expect("the capture button records its bounds in a debug build");
            self.visual.simulate_event(MouseDownEvent {
                position: bounds.center(),
                modifiers: Modifiers::default(),
                button: MouseButton::Left,
                click_count: 2,
                first_mouse: false,
            });
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

        /// Activates the test window and confirms it took, which is what a focus listener
        /// needs. The chain that makes this necessary is in the module docs.
        ///
        /// Activation goes through the platform callback on the foreground executor, hence
        /// the pump: `Window::activate_window` (`crates/gpui/src/window.rs:7275`) reaches
        /// `TestPlatform::set_active_window`, which spawns the task that calls
        /// `on_active_status_change` and so writes `self.active`.
        fn activate(&mut self) {
            self.visual.update(|window, _| window.activate_window());
            self.visual.run_until_parked();
            // The tripwire, and its whole purpose is the error message: `is_window_active`
            // reads the flag the callback writes rather than the frame, so a failed
            // activation fails here *by name* instead of surfacing two frames later as an
            // empty result list, which looks exactly like a missing listener.
            self.visual.update(|window, _| {
                assert!(
                    window.is_window_active(),
                    "the platform activation callback did not run"
                );
            });
            self.draw();
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
        let mut probe = Probe {
            visual,
            window: handle,
            capture,
            results,
        };
        probe.activate();
        probe
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
    fn the_press_that_starts_a_capture_is_not_a_binding(cx: &mut TestAppContext) {
        let mut probe = probe(cx);

        probe.press();

        assert!(probe.capturing(), "the press started a capture");
        assert!(
            probe.results().is_empty(),
            "the press that starts listening is the gesture, not a key: reporting it would bind \
             Mouse Left on the way in and leave nothing listening"
        );
    }

    #[gpui::test]
    fn a_press_while_listening_binds_the_button_it_was_made_with(cx: &mut TestAppContext) {
        let mut probe = probe(cx);
        probe.press();

        probe.press_button(MouseButton::Middle);

        assert_eq!(
            probe.results(),
            [Captured::Key(0x04)],
            "VK_MBUTTON, which the profile stores and the label reads as `Mouse Middle`"
        );
        assert!(!probe.capturing(), "one press ends the capture");
    }

    #[gpui::test]
    fn a_left_press_while_listening_binds_mouse_left(cx: &mut TestAppContext) {
        // The second left press is the binding; only the first one is the gesture. Without
        // that split a left button could never be bound, and with the split reversed the
        // starting press would bind itself.
        let mut probe = probe(cx);
        probe.press();
        probe.press_button(MouseButton::Left);

        assert_eq!(probe.results(), [Captured::Key(0x01)]);
        assert!(!probe.capturing());
    }

    #[gpui::test]
    fn the_second_click_of_a_double_click_is_not_a_bind(cx: &mut TestAppContext) {
        // The button is the kind of control people double-click. Both clicks are one gesture, so
        // the second one must not read as "bind Mouse Left" — a capture that ended by accident
        // also stops listening, so the key the user meant would go nowhere.
        let mut probe = probe(cx);
        probe.press();

        probe.second_click();

        assert!(probe.results().is_empty(), "one gesture is not two presses");
        assert!(probe.capturing(), "the capture is still listening");

        probe.press_button(MouseButton::Right);
        assert_eq!(
            probe.results(),
            [Captured::Key(0x02)],
            "and a separate press still binds"
        );
    }

    #[gpui::test]
    fn a_press_with_no_virtual_key_says_so_and_keeps_listening(cx: &mut TestAppContext) {
        let mut probe = probe(cx);
        probe.press();

        // Both a bare modifier and a name outside the vocabulary: neither has a code to store,
        // and both used to leave the capture listening with nothing on screen to say why the
        // press did nothing.
        for name in ["shift", "f25"] {
            probe.key(name);

            assert!(probe.results().is_empty(), "{name} has no key to store");
            assert!(
                probe.capturing(),
                "the capture waits for a key it can store, as the managed hook did"
            );
            assert!(
                probe
                    .visual
                    .cx
                    .read(|cx| probe.capture.read(cx).is_refused()),
                "{name} is recorded as refused"
            );
            assert_eq!(
                probe.label(),
                "Bind: [Not a bindable key]",
                "{name} says why the press did nothing"
            );
        }

        probe.key("f1");

        assert_eq!(
            probe.results(),
            [Captured::Key(0x70)],
            "the next mappable key still ends the capture"
        );
        assert_eq!(
            probe.label(),
            "Bind: Unbound",
            "and the button reads the profile again"
        );
    }

    #[gpui::test]
    fn losing_focus_abandons_the_capture(cx: &mut TestAppContext) {
        let mut probe = probe(cx);
        probe.press();
        assert!(probe.capturing());

        // Focus listeners are dispatched inside `Window::draw`
        // (`crates/gpui/src/window.rs:3938-3993`), not by `Window::blur` (`:2785-2800`),
        // which only clears the focus and marks the frame dirty. So the press's focus move
        // needs a frame to be recorded, and the blur needs one to be delivered — and the
        // window must be active, since the event's paths are blanked otherwise
        // (`Probe::activate`).
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
    fn the_mouse_buttons_are_the_managed_pseudo_keys() {
        // The managed `VK_LBUTTON`..`VK_XBUTTON2`, which is what its mouse hook reported
        // (`InputHooks.cs:40-44`), and the codes the label already names. The two properties
        // that matter are both asserted: the code the profile stores, and that the label reads
        // it back as a mouse button rather than as `VK 0x1`.
        for (button, vk, label) in [
            (MouseButton::Left, 0x01, "Mouse Left"),
            (MouseButton::Right, 0x02, "Mouse Right"),
            (MouseButton::Middle, 0x04, "Mouse Middle"),
            (
                MouseButton::Navigate(NavigationDirection::Back),
                0x05,
                "Mouse X1",
            ),
            (
                MouseButton::Navigate(NavigationDirection::Forward),
                0x06,
                "Mouse X2",
            ),
        ] {
            assert_eq!(mouse_virtual_key(button), vk, "{button:?}");
            assert_eq!(keybind_text::format_virtual_key(vk), label, "{button:?}");
        }
    }

    #[test]
    fn a_refusal_keeps_the_prefix_of_the_label_it_replaces() {
        // The button has one voice: the refusal reads as the control that refused, whether that
        // is a card header's `Bind: ` or an action row's `Action: `.
        let module = refusal_label("Bind: [Press key or mouse...]");
        let action = refusal_label("Action: [Press key or mouse...]");

        assert_eq!(module, "Bind: [Not a bindable key]");
        assert_eq!(action, "Action: [Not a bindable key]");
        assert_eq!(refusal_label("Unbound"), "Unbound: [Not a bindable key]");
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
