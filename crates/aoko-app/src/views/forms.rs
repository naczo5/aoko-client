//! The shell's six text fields: the widgets, and the seam that keeps them apart
//! from the state that decides what a submit means.
//!
//! `components::text_input` is an entity, so it has to be created where a
//! `Context` is in hand. The shell creates them on the first render
//! ([`Shell::ensure_form_fields`](crate::shell::Shell)) because `Shell::new` has no
//! context: `main.rs` calls `cx.new(|_| Shell::new(theme))` and that file belongs to
//! the components owner.
//!
//! The state stays where it was — `AccountsState::paste_text`, the servers state's
//! boxes, and `Shell::config_name` — so every submit path and every refusal is still
//! unit-testable without a window. The widgets are a view of that state: a change in
//! the field writes through to the state, and a change in the state is pushed back
//! into the field only while the field is **not** focused, so typing never has its
//! caret reset by a repaint.

use gpui::{div, AnyElement, App, Entity, IntoElement, Window};

use crate::components::text_input::TextInput;

/// One entity per text field the shell renders. `None` until the first render that
/// had a `Context`.
#[derive(Default)]
pub struct FormFields {
    /// The token/cookie paste box (`AccountsView.xaml:219`).
    pub token: Option<Entity<TextInput>>,
    /// The offline-mode username (`:271`).
    pub offline_name: Option<Entity<TextInput>>,
    /// The add form's display name (`ServersView.xaml`, `Name`).
    pub server_name: Option<Entity<TextInput>>,
    /// The add form's address.
    pub server_address: Option<Entity<TextInput>>,
    /// The direct-join box.
    pub direct_address: Option<Entity<TextInput>>,
    /// The Settings card's new-config name (`NewConfigNameBox`, `:1552`).
    pub config_name: Option<Entity<TextInput>>,
}

impl FormFields {
    /// Drops every field, so the next render recreates them against the current
    /// theme. The values survive because they live in the state, not here.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Renders one field. A missing entity means the shell has not rendered yet,
    /// which no laid-out view can hit; an empty box keeps the layout stable instead
    /// of panicking if it ever does.
    #[must_use]
    pub fn element(field: &Option<Entity<TextInput>>) -> AnyElement {
        match field {
            Some(field) => field.clone().into_any_element(),
            None => div().into_any_element(),
        }
    }
}

/// Pushes `wanted` into `field` unless the user is typing in it.
///
/// `TextInput::set_text` does not fire `on_change` (it is the owner writing in, not a
/// keystroke), so this cannot loop. Skipping a focused field is what lets a repaint
/// happen mid-word without moving the caret.
pub fn sync(field: &Option<Entity<TextInput>>, wanted: &str, window: &Window, cx: &mut App) {
    let Some(field) = field.as_ref() else {
        return;
    };
    if field.read(cx).focus_handle().is_focused(window) {
        return;
    }
    if field.read(cx).text() != wanted {
        let text = wanted.to_owned();
        field.update(cx, |input, cx| input.set_text(text, cx));
    }
}
