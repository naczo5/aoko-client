//! An editable single-line text field.
//!
//! `TabTextBox` (`TabStyles.xaml:155`), the colour picker's `HexBox`
//! (`ColorPickerDialog.xaml:83`) and the command overlay's `QueryBox`
//! (`CommandSearchOverlay.xaml:215`) are all text boxes in the C#. The pin ships no
//! text-input widget, so this is built from the two primitives that do exist: an
//! entity that implements [`EntityInputHandler`] and an [`ElementInputHandler`]
//! registered during paint.
//!
//! # The seam, read from the pin
//!
//! - `EntityInputHandler` is declared at `gpui/src/input.rs:26`. Its required methods
//!   are `text_for_range`, `selected_text_range`, `marked_text_range`, `unmark_text`,
//!   `replace_text_in_range`, `replace_and_mark_text_in_range`, `bounds_for_range`
//!   and `character_index_for_point`; `paste`, `selection_bounds_for_range`,
//!   `set_selected_text_range`, `text_length_utf16`, `accepts_text_input`,
//!   `text_input_configuration` and `text_input_editable_range` have defaults.
//! - `ElementInputHandler::new(bounds, entity)` (`input.rs:160`) adapts an
//!   `Entity<V: EntityInputHandler>` to `InputHandler`, and `Window::handle_input`
//!   (`gpui/src/window.rs:5995`) stores it. That call must happen **during paint**:
//!   it debug-asserts paint, and it only stores the handler while the focus handle is
//!   focused, for the frame being drawn.
//!
//! # Why a key handler is not optional here
//!
//! Two routes can put a character into this field. They are not the same route, and an
//! earlier revision of this comment got the second one wrong, so both are spelled out
//! with what the pin actually does:
//!
//! - **The platform's post-callback fallback.** `x11/window.rs:1230-1243` and
//!   `wayland/window.rs:1622-1632` both call `replace_text_in_range(None, key_char)` on
//!   the window's stored input handler, *after* the app's input callback returns and
//!   only if it propagated. `gpui` installs that handler at the end of every draw
//!   (`window.rs:3917-3925`), so it is live on **both** Linux backends — not Wayland
//!   only. A handler-only field would insert typed characters under Xvfb as well.
//! - **This component's `on_key_down`.** `PlatformInput::KeyDown` reaches
//!   `Window::dispatch_event` (`window.rs:6220`) → `dispatch_key_event`
//!   (`window.rs:6647`) → `finish_dispatch_key_event` (`window.rs:6867`), which ends at
//!   key listeners and observers and never calls `dispatch_input`. The listener edits
//!   from `KeyDownEvent::keystroke.key_char`.
//!
//! The listener is what makes the field an *editor*: the fallback only ever sees
//! `key_char`, so Backspace, Delete, Left, Right, Home and End — none of which carry a
//! printable character — would do nothing under either backend without it. And because
//! the listener calls `stop_propagation`, the two routes never both edit one keystroke:
//! the fallback is skipped exactly when the listener took the key.
//!
//! `TestPlatform` implements no post-callback fallback (`platform/test/window.rs` has
//! no `handle_input` and no `replace_text_in_range`), so a raw `PlatformInput::KeyDown`
//! in a test reaches the listener and nothing else. Measured by deleting the
//! `on_key_down` handling and re-running: `a_raw_key_down_edits_through_the_key_listener`
//! fails on its first assertion — `propagate` comes back `true` and nothing was typed.
//! `Window::dispatch_keystroke` (`window.rs:6178-6201`) does run the handler itself
//! afterwards, so the `dispatch_keystroke` tests insert characters either way and only
//! the `key_char`-less keys distinguish the routes there; that is why the older
//! `a_keystroke_reaches_the_field_through_the_key_listener` does break in the same
//! experiment but only at its Backspace step, with a message that names the edit rather
//! than the missing route.
//!
//! # What this does not do yet
//!
//! - **No selection.** The caret is a point, not an anchor; shift-click, drag and
//!   shift-arrow selection are not implemented, and `selected_text_range` always
//!   reports an empty range.
//! - **No IME composition.** `marked_text_range` is `None` and
//!   `replace_and_mark_text_in_range` commits instead of marking, so a composition is
//!   inserted in one step rather than underlined as it is built.
//! - **No click-to-caret.** `character_index_for_point` returns `None`, so a click
//!   focuses the field and puts the caret at the end, not under the pointer. That
//!   needs a shaped line to map x to an index.
//! - **No caret blink and no horizontal scrolling** for text longer than the box.
//!
//! # Geometry against the C#
//!
//! The box matches [`super::field::field`]: `FieldVariant::Panel` fill, 1 px edge,
//! 6 px radius, 8×5 padding, 13 px text, accent edge while focused. The caret is
//! drawn by splitting the display text at the caret and putting a 1 px accent line
//! between the halves, so it needs no text measurement.

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    canvas, div, px, App, Bounds, Context, CursorStyle, ElementId, ElementInputHandler,
    EntityInputHandler, FocusHandle, IntoElement, KeyDownEvent, MouseButton, Pixels, Render,
    SharedString, UTF16Selection, Window,
};

use crate::theme::{metrics, Theme};

/// The mask character. One per character, so a token's length is still visible while
/// its content is not — the same shape as a `PasswordBox`.
const MASK: char = '•';

/// The change callback: new value, plus the window and app the owner needs to reach
/// its own entity. Boxed behind an `Arc` because the entity and its events both hold it.
type ChangeHandler = Arc<dyn Fn(SharedString, &mut Window, &mut App)>;

/// Where the next character is unique per entity: element IDs must not collide between
/// two fields in one form.
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// The editing model: text plus a caret, in characters (not bytes, not UTF-16).
///
/// This is deliberately separate from the GPUI entity so the rules are unit-testable
/// without a window: inserting, deleting, moving, masking and the UTF-16 conversion
/// the input handler needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    text: String,
    /// Characters before the caret, `0..=char_len`.
    caret: usize,
}

impl TextEdit {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let caret = text.chars().count();
        Self { text, caret }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Caret position in characters.
    #[must_use]
    pub fn caret(&self) -> usize {
        self.caret
    }

    #[must_use]
    pub fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replaces the whole value and puts the caret at the end, which is what a dialog
    /// does when it seeds a field.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = self.text.chars().count();
    }

    /// Puts the caret at `char_index`, clamped to the value.
    pub fn set_caret(&mut self, char_index: usize) {
        self.caret = char_index.min(self.char_len());
    }

    pub fn insert_char(&mut self, character: char) {
        self.insert_str(character.encode_utf8(&mut [0; 4]));
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let at = self.byte_index(self.caret);
        self.text.insert_str(at, text);
        self.caret += text.chars().count();
    }

    /// Deletes the character before the caret. `false` at the start, so a handler can
    /// tell an edit from a no-op.
    pub fn backspace(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        let start = self.byte_index(self.caret - 1);
        let end = self.byte_index(self.caret);
        self.text.replace_range(start..end, "");
        self.caret -= 1;
        true
    }

    /// Deletes the character after the caret. `false` at the end.
    pub fn delete(&mut self) -> bool {
        if self.caret >= self.char_len() {
            return false;
        }
        let start = self.byte_index(self.caret);
        let end = self.byte_index(self.caret + 1);
        self.text.replace_range(start..end, "");
        true
    }

    /// Replaces a character range with `text`, which is what the input handler does for
    /// paste and IME. The caret ends after the inserted text.
    pub fn replace_range(&mut self, range: Range<usize>, text: &str) {
        let start_char = range.start.min(self.char_len());
        let end_char = range.end.clamp(start_char, self.char_len());
        let start = self.byte_index(start_char);
        let end = self.byte_index(end_char);
        self.text.replace_range(start..end, text);
        self.caret = start_char + text.chars().count();
    }

    /// Caret moves return whether the caret actually moved.
    pub fn move_left(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        self.caret -= 1;
        true
    }

    pub fn move_right(&mut self) -> bool {
        if self.caret >= self.char_len() {
            return false;
        }
        self.caret += 1;
        true
    }

    pub fn go_home(&mut self) -> bool {
        let moved = self.caret != 0;
        self.caret = 0;
        moved
    }

    pub fn go_end(&mut self) -> bool {
        let end = self.char_len();
        let moved = self.caret != end;
        self.caret = end;
        moved
    }

    /// The value as drawn, masked when the field hides its content.
    #[must_use]
    pub fn display(&self, masked: bool) -> String {
        if masked {
            MASK.to_string().repeat(self.char_len())
        } else {
            self.text.clone()
        }
    }

    /// The display text split at the caret, so the caret can be drawn between two text
    /// elements without measuring a shaped line.
    #[must_use]
    pub fn split_at_caret(&self, masked: bool) -> (String, String) {
        let byte = self.byte_index(self.caret);
        if masked {
            let before = self.text[..byte].chars().count();
            let after = self.text[byte..].chars().count();
            (
                MASK.to_string().repeat(before),
                MASK.to_string().repeat(after),
            )
        } else {
            (self.text[..byte].to_owned(), self.text[byte..].to_owned())
        }
    }

    fn byte_index(&self, char_index: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_index)
            .map_or(self.text.len(), |(index, _)| index)
    }
}

/// UTF-16 offset of a character offset, which is what the input protocol speaks.
#[must_use]
pub fn char_to_utf16(text: &str, char_index: usize) -> usize {
    text.chars().take(char_index).map(char::len_utf16).sum()
}

/// Character offset of a UTF-16 offset, clamped and snapped down to a boundary.
#[must_use]
pub fn utf16_to_char(text: &str, utf16_index: usize) -> usize {
    let mut utf16 = 0;
    for (chars, character) in text.chars().enumerate() {
        if utf16 >= utf16_index {
            return chars;
        }
        utf16 += character.len_utf16();
        if utf16 > utf16_index {
            // Landed inside a surrogate pair: snap to the character it belongs to.
            return chars;
        }
    }
    text.chars().count()
}

/// What one keystroke did, so the caller knows whether to fire `on_change`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// The value changed.
    Text,
    /// Only the caret moved.
    Caret,
    /// Not ours: let the key propagate to bindings.
    Ignored,
}

/// An editable field. Create one entity per field and render it as a child:
///
/// ```ignore
/// let token = cx.new(|cx| {
///     TextInput::new(cx, theme.clone(), "Paste a token")
///         .masked(true)
///         .on_change(move |text, _window, cx| {
///             handle.update(cx, |shell, cx| shell.set_token(text.to_string(), cx));
///         })
/// });
/// // in the view:
/// .child(token.clone())
/// // reading the value, e.g. to submit:
/// let value = token.read(cx).text().to_owned();
/// ```
///
/// `Entity<TextInput>` is an element because `impl<V: Render> IntoElement for Entity<V>`
/// exists at `gpui/src/view.rs:112`. The callback takes `&mut Window` and `&mut App` in
/// the same shape as [`super::slider::slider`], so it can reach the entity that owns the form.
/// `Context::processor` produces exactly this shape where a `Context<Self>` is in hand.
///
/// The field must be a **child** view, not a window's root: a key listener updates the
/// entity, and an entity that is already leased while it renders cannot be updated
/// again. Every real use nests it under the shell, so this only bites a test that makes
/// the field itself the root.
pub struct TextInput {
    theme: Theme,
    edit: TextEdit,
    placeholder: SharedString,
    masked: bool,
    id: ElementId,
    focus: FocusHandle,
    on_change: Option<ChangeHandler>,
}

impl TextInput {
    /// A field with an empty value. `cx` is the entity context, which supplies the
    /// focus handle.
    pub fn new(cx: &mut Context<Self>, theme: Theme, placeholder: impl Into<SharedString>) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            theme,
            edit: TextEdit::new(""),
            placeholder: placeholder.into(),
            masked: false,
            id: ElementId::Name(SharedString::from(format!("text-input-{id}"))),
            focus: cx.focus_handle(),
            on_change: None,
        }
    }

    /// Masks the value: one `•` per character, as a token field should.
    #[must_use]
    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    /// Called after every edit, with the new value. Not called by [`Self::set_text`],
    /// which is the owner writing in rather than the user typing.
    #[must_use]
    pub fn on_change(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Arc::new(handler));
        self
    }

    #[must_use]
    pub fn text(&self) -> &str {
        self.edit.text()
    }

    #[must_use]
    pub fn edit(&self) -> &TextEdit {
        &self.edit
    }

    #[must_use]
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Writes a value in from the owner, e.g. when a dialog opens. Does not fire
    /// `on_change`.
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.edit.set_text(text);
        cx.notify();
    }

    /// One keystroke. Returns whether it was ours.
    fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let keystroke = &event.keystroke;
        // Anything with a chord modifier belongs to a binding (Ctrl+K, Ctrl+V, …).
        if keystroke.modifiers.control
            || keystroke.modifiers.alt
            || keystroke.modifiers.platform
            || keystroke.modifiers.function
        {
            return false;
        }

        let outcome = match keystroke.key.as_str() {
            "backspace" => bool_outcome(self.edit.backspace()),
            "delete" => bool_outcome(self.edit.delete()),
            "left" => caret_outcome(self.edit.move_left()),
            "right" => caret_outcome(self.edit.move_right()),
            "home" => caret_outcome(self.edit.go_home()),
            "end" => caret_outcome(self.edit.go_end()),
            // Tab, Enter and Escape stay with the dialog: focus movement and submit.
            "tab" | "enter" | "escape" => Outcome::Ignored,
            _ => match keystroke.key_char.as_deref() {
                Some(text) if !text.is_empty() && !text.chars().any(char::is_control) => {
                    self.edit.insert_str(text);
                    Outcome::Text
                }
                _ => Outcome::Ignored,
            },
        };

        match outcome {
            Outcome::Text => {
                window.notify_text_input_changed(&self.focus);
                cx.notify();
                if let Some(handler) = self.on_change.clone() {
                    handler(SharedString::from(self.edit.text().to_owned()), window, cx);
                }
                true
            }
            Outcome::Caret => {
                window.notify_text_selection_changed(&self.focus);
                cx.notify();
                true
            }
            Outcome::Ignored => false,
        }
    }

    /// The display split, for the caller that wants to read it in tests.
    #[must_use]
    pub fn split_display(&self) -> (String, String) {
        self.edit.split_at_caret(self.masked)
    }
}

fn bool_outcome(changed: bool) -> Outcome {
    if changed {
        Outcome::Text
    } else {
        Outcome::Ignored
    }
}

fn caret_outcome(moved: bool) -> Outcome {
    if moved {
        Outcome::Caret
    } else {
        Outcome::Ignored
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let focused = self.focus.is_focused(window);

        let (before, after) = self.edit.split_at_caret(self.masked);
        let empty = self.edit.is_empty();
        let caret_colour = theme.accent;
        let text_colour = theme.text;
        let placeholder_colour = theme.dim_text;
        let edge = if focused {
            theme.accent
        } else {
            theme.slider_background
        };
        let placeholder = self.placeholder.clone();
        let focus = self.focus.clone();

        let key_entity = cx.entity();
        let focus_entity = cx.entity();

        div()
            .id(self.id.clone())
            .flex()
            .flex_row()
            .items_center()
            .relative()
            .gap(px(1.0))
            .text_size(px(13.0))
            .cursor(CursorStyle::IBeam)
            .rounded(px(metrics::radius::CARD))
            .border_1()
            .bg(theme.panel)
            .px(px(8.0))
            .py(px(5.0))
            .border_color(edge)
            .track_focus(&self.focus)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx);
                cx.stop_propagation();
            })
            .on_key_down(move |event, window, cx| {
                let handled =
                    key_entity.update(cx, |input, cx| input.handle_key(event, window, cx));
                if handled {
                    cx.stop_propagation();
                }
            })
            // Prefix, caret, suffix: the caret is drawn where it is by being between the
            // two halves of the display value, so no text measurement is needed. An
            // empty field has no prefix, so the caret sits left of the placeholder.
            .child(if empty {
                div().w(px(0.0)).into_any_element()
            } else {
                div()
                    .text_color(text_colour)
                    .child(SharedString::from(before))
                    .into_any_element()
            })
            .child(if focused {
                div()
                    .w(px(1.0))
                    .h(px(15.0))
                    .bg(caret_colour)
                    .into_any_element()
            } else {
                div().w(px(0.0)).into_any_element()
            })
            .child(if empty {
                div()
                    .text_color(placeholder_colour)
                    .child(placeholder)
                    .into_any_element()
            } else {
                div()
                    .text_color(text_colour)
                    .child(SharedString::from(after))
                    .into_any_element()
            })
            // Registers the IME/paste handler for this frame. It has to run during
            // paint: `Window::handle_input` debug-asserts paint and only stores the
            // handler while the field is focused.
            .child(
                canvas(
                    |bounds, _window, _cx| bounds,
                    move |bounds: Bounds<Pixels>, _, window, cx| {
                        window.handle_input(
                            &focus_entity.read(cx).focus,
                            ElementInputHandler::new(bounds, focus_entity.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .top(px(0.0))
                .left(px(0.0))
                .w_full()
                .h_full(),
            )
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let start = utf16_to_char(self.edit.text(), range_utf16.start);
        let end = utf16_to_char(self.edit.text(), range_utf16.end);
        Some(
            self.edit
                .text()
                .chars()
                .skip(start)
                .take(end.saturating_sub(start))
                .collect(),
        )
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        // No selection yet: the caret reports an empty range at its own position.
        let caret = char_to_utf16(self.edit.text(), self.edit.caret());
        Some(UTF16Selection {
            range: caret..caret,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (start, end) = match range_utf16 {
            Some(range) => (
                utf16_to_char(self.edit.text(), range.start),
                utf16_to_char(self.edit.text(), range.end),
            ),
            None => (self.edit.caret(), self.edit.caret()),
        };
        self.edit.replace_range(start..end, text);
        window.notify_text_input_changed(&self.focus);
        cx.notify();
        if let Some(handler) = self.on_change.clone() {
            handler(SharedString::from(self.edit.text().to_owned()), window, cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Composition is committed in one step: no marked range is reported, so the
        // platform has nothing to underline. Documented in the module header.
        self.replace_text_in_range(range_utf16, new_text, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Needs a shaped line to place a range; without it the IME candidate window
        // falls back to the field, which is acceptable for a single-line box.
        None
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        // Click-to-caret needs the same shaped line; a click focuses and appends.
        None
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(char_to_utf16(self.edit.text(), self.edit.char_len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_inserts_at_the_caret_and_advances_it() {
        let mut edit = TextEdit::new("");
        edit.insert_str("ac");
        edit.move_left();
        edit.insert_char('b');
        assert_eq!(edit.text(), "abc");
        assert_eq!(
            edit.caret(),
            2,
            "the caret sits after the inserted character"
        );
    }

    #[test]
    fn backspace_and_delete_stop_at_the_edges() {
        let mut edit = TextEdit::new("ab");
        // The constructor puts the caret at the end, so backspace removes the last one.
        assert!(edit.backspace());
        assert_eq!(edit.text(), "a");
        assert!(!edit.delete(), "nothing after the caret");
        edit.go_home();
        assert!(!edit.backspace(), "nothing before the caret");
        assert!(edit.delete(), "deletes the character after the caret");
        assert_eq!(edit.text(), "");
        assert_eq!(edit.caret(), 0);
        assert!(!edit.delete(), "an empty field has nothing to delete");
    }

    #[test]
    fn movement_clamps_instead_of_wrapping() {
        let mut edit = TextEdit::new("abc");
        assert!(edit.go_home());
        assert!(!edit.move_left());
        assert_eq!(edit.caret(), 0);
        assert!(!edit.go_home(), "already home");
        assert!(edit.move_right());
        assert!(edit.go_end());
        assert!(!edit.move_right());
        assert!(!edit.go_end(), "already at the end");
        assert_eq!(edit.caret(), 3);
    }

    #[test]
    fn a_multibyte_character_is_one_edit() {
        // Three bytes, one character: backspace must remove the whole thing and the
        // caret must be a character index, not a byte offset.
        let mut edit = TextEdit::new("héllo");
        edit.go_end();
        assert!(edit.backspace());
        assert_eq!(edit.text(), "héll");
        edit.insert_char('é');
        assert_eq!(edit.text(), "héllé");
        assert_eq!(edit.caret(), 5);
        assert!(edit.backspace());
        assert_eq!(edit.text(), "héll");
    }

    #[test]
    fn replication_of_an_empty_field_is_silent() {
        let mut edit = TextEdit::new("");
        assert!(!edit.backspace());
        assert!(!edit.delete());
        assert!(edit.is_empty());
    }

    #[test]
    fn masking_keeps_the_length_and_splits_at_the_caret() {
        let edit = {
            let mut edit = TextEdit::new("tok");
            edit.move_left();
            edit
        };
        assert_eq!(edit.display(true), "•••");
        let (before, after) = edit.split_at_caret(true);
        assert_eq!((before.as_str(), after.as_str()), ("••", "•"));
        // The two halves are the drawn value, in order, and unmasked they are the text.
        assert_eq!(format!("{before}{after}"), edit.display(true));
        assert_eq!(
            edit.split_at_caret(false),
            ("to".to_owned(), "k".to_owned())
        );
    }

    #[test]
    fn an_inserted_token_is_masked_in_full() {
        let mut edit = TextEdit::new("");
        edit.insert_str("abc");
        assert_eq!(edit.display(true), "•••");
        assert_eq!(edit.text(), "abc", "masking is a view, not a mutation");
    }

    #[test]
    fn replacing_a_range_puts_the_caret_after_the_insertion() {
        let mut edit = TextEdit::new("abcdef");
        edit.replace_range(1..4, "XY");
        assert_eq!(edit.text(), "aXYef");
        assert_eq!(edit.caret(), 3, "1 + 2 characters");
        // An out-of-range span clamps instead of panicking.
        edit.replace_range(0..99, "z");
        assert_eq!(edit.text(), "z");
        assert_eq!(edit.caret(), 1);
    }

    #[test]
    fn set_text_puts_the_caret_at_the_end() {
        let mut edit = TextEdit::new("a");
        edit.set_text("hello");
        assert_eq!(edit.caret(), 5);
        edit.set_text("");
        assert_eq!(edit.caret(), 0);
    }

    #[test]
    fn utf16_conversions_agree_across_planes() {
        // "a" 1 unit, "é" 1 unit, "😀" 2 units: the protocol counts UTF-16 code units.
        let text = "aé😀";
        assert_eq!(char_to_utf16(text, 0), 0);
        assert_eq!(char_to_utf16(text, 1), 1);
        assert_eq!(char_to_utf16(text, 2), 2);
        assert_eq!(char_to_utf16(text, 3), 4);
        assert_eq!(utf16_to_char(text, 0), 0);
        assert_eq!(utf16_to_char(text, 2), 2);
        assert_eq!(utf16_to_char(text, 4), 3);
        // An offset inside the surrogate pair snaps to the character that owns it.
        assert_eq!(utf16_to_char(text, 3), 2);
        // Past the end clamps to the character count.
        assert_eq!(utf16_to_char(text, 99), 3);
        assert_eq!(char_to_utf16(text, 99), 4);
    }

    #[test]
    fn the_display_split_is_the_display_value() {
        let mut edit = TextEdit::new("hello");
        for _ in 0..5 {
            let (before, after) = edit.split_at_caret(false);
            assert_eq!(format!("{before}{after}"), edit.display(false));
            edit.move_left();
        }
        let (before, after) = edit.split_at_caret(false);
        assert_eq!((before.as_str(), after.as_str()), ("", "hello"));
    }

    /// The end-to-end path that makes the field usable on X11: a real keystroke through
    /// the window's dispatch, into `on_key_down`, into the entity.
    ///
    /// `TestAppContext::dispatch_keystroke` (`gpui/src/app/test_context.rs:677`) is GPUI's
    /// own "as though the user typed it" entry point: it dispatches a `KeyDown` and then
    /// routes `key_char` to the registered input handler. It takes the window handle
    /// directly, so the root view is not leased while the field handles the key.
    #[test]
    fn a_keystroke_reaches_the_field_through_the_key_listener() {
        use gpui::{Keystroke, TestAppContext};

        let mut cx = TestAppContext::single();
        let (form, window) = form_window(&mut cx, "type", false);
        focus_field(&window, &form, &mut cx);
        let handle: gpui::AnyWindowHandle = window.into();

        cx.dispatch_keystroke(handle, Keystroke::parse("a").expect("keystroke"));
        cx.dispatch_keystroke(handle, Keystroke::parse("b").expect("keystroke"));
        assert_eq!(value(&cx, &form), "ab", "typed characters reach the value");
        assert_eq!(caret(&cx, &form), 2);

        cx.dispatch_keystroke(handle, Keystroke::parse("backspace").expect("keystroke"));
        assert_eq!(value(&cx, &form), "a", "backspace deletes behind the caret");

        cx.dispatch_keystroke(handle, Keystroke::parse("left").expect("keystroke"));
        assert_eq!(caret(&cx, &form), 0, "the caret moves left");

        cx.dispatch_keystroke(handle, Keystroke::parse("a").expect("keystroke"));
        assert_eq!(value(&cx, &form), "aa", "typing inserts at the caret");
        assert_eq!(caret(&cx, &form), 1);

        cx.dispatch_keystroke(handle, Keystroke::parse("home").expect("keystroke"));
        assert_eq!(caret(&cx, &form), 0);
        cx.dispatch_keystroke(handle, Keystroke::parse("delete").expect("keystroke"));
        assert_eq!(value(&cx, &form), "a", "delete removes ahead of the caret");
    }

    /// The masked field is what the account forms need: the owner reads the value, the
    /// drawn text does not show it.
    #[test]
    fn a_masked_field_still_reports_the_real_value() {
        use gpui::{Keystroke, TestAppContext};

        let mut cx = TestAppContext::single();
        let (form, window) = form_window(&mut cx, "token", true);
        focus_field(&window, &form, &mut cx);
        let handle: gpui::AnyWindowHandle = window.into();

        for key in ["s", "e", "c"] {
            cx.dispatch_keystroke(handle, Keystroke::parse(key).expect("keystroke"));
        }
        assert_eq!(value(&cx, &form), "sec", "the owner sees the value");
        let display = cx.read(|cx| form.read(cx).field.read(cx).split_display());
        // The caret sits after the last typed character, so the whole mask is prefix.
        assert_eq!(display, ("•••".to_owned(), String::new()));
    }

    /// The test that fails when the `on_key_down` handling is removed.
    ///
    /// It feeds `PlatformInput::KeyDown` straight into `Window::dispatch_event`
    /// (`window.rs:6220`), which is the shape the X11 backend delivers
    /// (`x11/client.rs:1207`) and which ends at key listeners: no `dispatch_input`, and
    /// `TestPlatform` has no post-callback fallback to fall back on, so only the listener
    /// can edit here. `Window::dispatch_keystroke` (`window.rs:6178-6201`) instead runs
    /// the handler itself after the listener propagates, which is why the
    /// `dispatch_keystroke` tests above fail late — at Backspace — rather than on the
    /// first character when this handling is removed: characters have a second route,
    /// the `key_char`-less keys do not.
    ///
    /// A character first, then the `key_char`-less keys — Left, Home, Backspace,
    /// Delete — which no platform fallback could ever produce.
    #[test]
    fn a_raw_key_down_edits_through_the_key_listener() {
        use gpui::TestAppContext;

        let mut cx = TestAppContext::single();
        let (form, window) = form_window(&mut cx, "raw", false);
        focus_field(&window, &form, &mut cx);
        let handle: gpui::AnyWindowHandle = window.into();

        let result = dispatch_key(&mut cx, handle, "a", Some("a"));
        assert!(
            !result.propagate,
            "the listener consumes the key it edits, so the platform fallback cannot insert \
             the same character a second time"
        );
        assert_eq!(
            value(&cx, &form),
            "a",
            "a typed character reaches the value through the listener"
        );

        dispatch_key(&mut cx, handle, "b", Some("b"));
        assert_eq!(value(&cx, &form), "ab");

        dispatch_key(&mut cx, handle, "left", None);
        assert_eq!(caret(&cx, &form), 1, "Left moves the caret");

        dispatch_key(&mut cx, handle, "x", Some("x"));
        assert_eq!(value(&cx, &form), "axb", "typing inserts at the caret");

        dispatch_key(&mut cx, handle, "backspace", None);
        assert_eq!(
            value(&cx, &form),
            "ab",
            "Backspace deletes behind the caret"
        );

        dispatch_key(&mut cx, handle, "home", None);
        dispatch_key(&mut cx, handle, "delete", None);
        assert_eq!(value(&cx, &form), "b", "Delete removes ahead of the caret");

        assert_eq!(
            changes(&cx, &form),
            ["a", "ab", "axb", "ab", "b"],
            "every edit reaches the owner through the callback, in order"
        );
    }

    /// The other route: what paste, IME and the platform fallback call. It has to edit
    /// and report through `on_change` with no keystroke listener involved at all.
    #[test]
    fn the_registered_input_handler_edits_on_the_paste_path() {
        use gpui::TestAppContext;

        let mut cx = TestAppContext::single();
        let (form, window) = form_window(&mut cx, "paste", false);
        focus_field(&window, &form, &mut cx);
        let field = cx.read(|cx| form.read(cx).field.clone());

        window
            .update(&mut cx, |_, window, cx| {
                field.update(cx, |field, cx| {
                    field.replace_text_in_range(None, "pasted", window, cx);
                });
            })
            .expect("handler edit");

        assert_eq!(value(&cx, &form), "pasted");
        assert_eq!(
            caret(&cx, &form),
            6,
            "the caret follows a committed insertion"
        );
        assert_eq!(
            changes(&cx, &form),
            ["pasted"],
            "the handler route reports to the owner too"
        );
    }

    /// One `KeyDown`, delivered the way the backend delivers it: a `key_char` for a
    /// character key, `None` for Backspace, arrows, Home and End. The keys are
    /// `&'static str` because `Keystroke::parse` requires it.
    ///
    /// `TestAppContext::update_window` is used rather than `WindowHandle::update`
    /// because the latter leases the root view for the whole closure, and dispatching a
    /// key event updates that root: `cannot update ... while it is already being
    /// updated`.
    fn dispatch_key(
        cx: &mut gpui::TestAppContext,
        window: gpui::AnyWindowHandle,
        key: &'static str,
        key_char: Option<&'static str>,
    ) -> gpui::DispatchEventResult {
        use gpui::{KeyDownEvent, Keystroke, PlatformInput};

        let mut keystroke = Keystroke::parse(key).expect("keystroke");
        keystroke.key_char = key_char.map(Into::into);
        cx.update_window(window, |_, window, cx| {
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke,
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            )
        })
        .expect("dispatch")
    }

    /// A window whose only content is the field under test, plus the values the field
    /// reported through `on_change` in the order it reported them.
    struct Form {
        field: gpui::Entity<TextInput>,
        changes: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Render for Form {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .items_center()
                .child(self.field.clone())
        }
    }

    fn form_window(
        cx: &mut gpui::TestAppContext,
        placeholder: &'static str,
        masked: bool,
    ) -> (gpui::Entity<Form>, gpui::WindowHandle<Form>) {
        use gpui::WindowOptions;

        let form = cx.update(|cx| {
            let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let recorded = changes.clone();
            let field = cx.new(|cx| {
                TextInput::new(cx, Theme::slate(), placeholder)
                    .masked(masked)
                    .on_change(move |value, _, _| {
                        recorded
                            .lock()
                            .expect("changes lock")
                            .push(value.to_string());
                    })
            });
            cx.new(|_| Form { field, changes })
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| form.clone())
                .expect("test window")
        });
        (form, window)
    }

    fn focus_field(
        window: &gpui::WindowHandle<Form>,
        form: &gpui::Entity<Form>,
        cx: &mut gpui::TestAppContext,
    ) {
        let handle = cx.read(|cx| form.read(cx).field.read(cx).focus_handle().clone());
        window
            .update(cx, |_, window, cx| window.focus(&handle, cx))
            .expect("focus");
    }

    fn value(cx: &gpui::TestAppContext, form: &gpui::Entity<Form>) -> String {
        cx.read(|cx| form.read(cx).field.read(cx).text().to_owned())
    }

    fn caret(cx: &gpui::TestAppContext, form: &gpui::Entity<Form>) -> usize {
        cx.read(|cx| form.read(cx).field.read(cx).edit().caret())
    }

    /// The values the field reported through `on_change`, oldest first.
    fn changes(cx: &gpui::TestAppContext, form: &gpui::Entity<Form>) -> Vec<String> {
        cx.read(|cx| form.read(cx).changes.lock().expect("changes lock").clone())
    }
}
