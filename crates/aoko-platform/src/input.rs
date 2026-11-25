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
    /// Sampled mouse state. Cheap enough to call from an automation tick.
    fn mouse_state(&self) -> MouseState;
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
