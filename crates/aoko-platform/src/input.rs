//! Global input observation and synthesis.

use crate::error::PlatformError;

/// A physical key, named by logical function rather than OS scancode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Shift,
    Control,
    Alt,
    Space,
    Enter,
    Escape,
    Tab,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    F(u8),
    Other(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Current mouse state, sampled by automation loops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MouseState {
    pub left_down: bool,
    pub right_down: bool,
}

/// A discrete input event from the global hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    KeyDown(Key),
    KeyUp(Key),
    ButtonDown(MouseButton),
    ButtonUp(MouseButton),
    /// Absolute pointer position in screen coordinates.
    MouseMove {
        x: i32,
        y: i32,
    },
}

pub trait InputBackend: Send + Sync {
    /// Sampled mouse state, or `None` when this backend cannot see the mouse at
    /// all - no display, no `/dev/input` access, no session to query.
    ///
    /// The distinction matters to the hold guards: "no button is held" and "I
    /// cannot tell whether a button is held" must not be the same answer, or the
    /// clicker acts while the user is holding the button down.
    ///
    /// **The default is deliberately `None`.** Observability is a claim a backend
    /// has to make, so a backend that cannot see input must not inherit a confident
    /// answer from the lossy [`InputBackend::mouse_state`]. Every real backend
    /// overrides this, and a new one must override it too.
    fn probe_mouse_state(&self) -> Option<MouseState> {
        None
    }

    /// Sampled key state, `None` on the same "cannot observe" terms as
    /// [`InputBackend::probe_mouse_state`]. A key with no keycode in the current
    /// layout is also `None`: unknowable, not up.
    ///
    /// The default is `None` for the same fail-closed reason: override it to opt in.
    fn probe_key_state(&self, _key: Key) -> Option<bool> {
        None
    }

    /// Sampled mouse state. Cheap enough to call from an automation tick.
    ///
    /// Lossy: a backend that cannot observe returns an idle state. Use
    /// [`InputBackend::probe_mouse_state`] when the difference from "idle"
    /// matters.
    fn mouse_state(&self) -> MouseState;

    /// Sampled key state. Lossy in the same way as
    /// [`InputBackend::mouse_state`]; prefer [`InputBackend::probe_key_state`].
    fn key_state(&self, key: Key) -> bool;

    fn click(&self, button: MouseButton) -> Result<(), PlatformError>;
    fn move_relative(&self, dx: i32, dy: i32) -> Result<(), PlatformError>;
    fn key_press(&self, key: Key) -> Result<(), PlatformError>;

    /// Drains queued global hook events into `out`, which is reused so a hot
    /// loop allocates nothing.
    fn poll_events(&self, out: &mut Vec<InputEvent>) -> Result<(), PlatformError>;

    /// Presses without releasing, for the hold-modifier sequences the chest
    /// stealer and refill controllers drive (`SendInput` with only a down event).
    fn button_down(&self, button: MouseButton) -> Result<(), PlatformError> {
        Err(not_implemented(button, "button_down"))
    }

    fn button_up(&self, button: MouseButton) -> Result<(), PlatformError> {
        Err(not_implemented(button, "button_up"))
    }

    fn key_down(&self, key: Key) -> Result<(), PlatformError> {
        Err(not_implemented(key, "key_down"))
    }

    fn key_up(&self, key: Key) -> Result<(), PlatformError> {
        Err(not_implemented(key, "key_up"))
    }

    /// Absolute pointer position, when the backend can read it cheaply.
    fn mouse_position(&self) -> Option<(i32, i32)> {
        None
    }
}

/// Shared diagnostic for the optional half of [`InputBackend`].
fn not_implemented<T: core::fmt::Debug>(what: T, operation: &str) -> PlatformError {
    PlatformError::unsupported(format!(
        "{operation}({what:?}) is not implemented by this backend"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mouse_state_is_idle() {
        let state = MouseState::default();
        assert!(!state.left_down && !state.right_down);
    }

    #[test]
    fn key_variants_are_distinct() {
        assert_ne!(Key::Char('a'), Key::Char('b'));
        assert_eq!(Key::F(1), Key::F(1));
    }

    struct Minimal;

    impl InputBackend for Minimal {
        fn mouse_state(&self) -> MouseState {
            MouseState::default()
        }
        fn key_state(&self, _key: Key) -> bool {
            false
        }
        fn click(&self, _button: MouseButton) -> Result<(), PlatformError> {
            Ok(())
        }
        fn move_relative(&self, _dx: i32, _dy: i32) -> Result<(), PlatformError> {
            Ok(())
        }
        fn key_press(&self, _key: Key) -> Result<(), PlatformError> {
            Ok(())
        }
        fn poll_events(&self, _out: &mut Vec<InputEvent>) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    #[test]
    fn probing_is_opt_in_so_an_unsure_backend_cannot_claim_zero() {
        let backend = Minimal;
        // `Minimal` implements only the lossy pair, so it inherits the fail-closed
        // default: a caller must not read its `false` as "nothing is held".
        assert_eq!(backend.probe_mouse_state(), None);
        assert_eq!(backend.probe_key_state(Key::Char('a')), None);
        // The lossy forms still work for callers that accept the ambiguity.
        assert_eq!(backend.mouse_state(), MouseState::default());
        assert!(!backend.key_state(Key::Char('a')));
    }

    #[test]
    fn optional_operations_report_unsupported_instead_of_pretending() {
        let backend = Minimal;
        let err = backend
            .key_down(Key::Shift)
            .expect_err("no default support");
        assert!(matches!(err, PlatformError::Unsupported { .. }));
        assert!(backend.mouse_position().is_none());
        assert!(matches!(
            backend.button_up(MouseButton::Left),
            Err(PlatformError::Unsupported { .. })
        ));
    }
}
