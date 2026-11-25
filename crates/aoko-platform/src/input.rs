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
    MouseMove { x: i32, y: i32 },
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
}
