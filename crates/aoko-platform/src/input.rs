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

/// The [`Key`] a Win32 virtual-key code names, for the codes a module bind can carry.
///
/// A saved profile stores a module's key as the **virtual-key code** the managed host
/// wrote (`InputHooks.ModuleKeys`, and every `Profile.ModuleKeys` entry), because that is
/// what `GetAsyncKeyState`/the hook compared against. The hook now reports a [`Key`], so
/// the two have to meet somewhere, and this is that place: the letters and digits, whose
/// virtual-key codes *are* their ASCII uppercase value (`VK_A` = 0x41 ... `VK_0` = 0x30),
/// plus the handful of named keys a bind realistically uses.
///
/// Anything else is `None` rather than a guess: an unknown code must mean "no bind fires"
/// and not "some other key fires", because the result is a module being switched on.
#[must_use]
pub fn key_for_virtual_key(code: i32) -> Option<Key> {
    match code {
        // VK 0x30..=0x39 are '0'..='9' and 0x41..=0x5A are 'A'..='Z'.
        0x30..=0x39 => char::from_u32(code as u32).map(Key::Char),
        0x41..=0x5A => char::from_u32(code as u32).map(Key::Char),
        // The named keys, `VK_*` values from `windows::Win32::UI::Input::KeyboardAndMouse`.
        0x08 => Some(Key::Backspace),
        0x09 => Some(Key::Tab),
        0x0D => Some(Key::Enter),
        0x10 | 0xA0 | 0xA1 => Some(Key::Shift),
        0x11 | 0xA2 | 0xA3 => Some(Key::Control),
        0x12 | 0xA4 | 0xA5 => Some(Key::Alt),
        0x1B => Some(Key::Escape),
        0x20 => Some(Key::Space),
        0x25 => Some(Key::Left),
        0x26 => Some(Key::Up),
        0x27 => Some(Key::Right),
        0x28 => Some(Key::Down),
        // VK_F1 = 0x70.
        0x70..=0x87 => Some(Key::F((code - 0x70 + 1) as u8)),
        _ => None,
    }
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

    /// Places the pointer at an absolute position, in virtual-desktop pixels.
    ///
    /// `(x, y)` is a point in the coordinate space the window inspectors report:
    /// **physical pixels of the virtual desktop, origin at its top-left corner**, y growing
    /// downwards. That is the same space `WindowInspector::client_bounds` returns the game's
    /// client area in, so a caller maps a `chestStealerState` slot through that rect and hands
    /// the result here without a second conversion. Negative coordinates are legitimate on a
    /// multi-monitor desktop whose leftmost monitor sits left of or above the primary one.
    ///
    /// **Why this is not [`InputBackend::move_relative`].** A relative move is a *delta*: the OS
    /// applies pointer ballistics to it, and with the game's mouse captured it becomes camera
    /// yaw/pitch rather than pointer motion at all. Neither is a position a caller can aim at, so
    /// accumulating deltas can only ever converge on a target by trial and feedback. A chest
    /// slot has to be hit on a pixel: the managed host's chest stealer computed the screen point
    /// once (`ChestStealerCoordinateMapper.TryMapScaledPoint`, `ChestStealerController.cs:296-308`)
    /// and then called `SetCursorPos` - absolute placement - before each click, which is exactly
    /// this call. Keep the two operations separate rather than expressing one through the other:
    /// aiming a camera and pointing at a menu item are different intents with different failure
    /// modes, and a backend can plausibly support one without the other.
    ///
    /// The default refuses with the same `unsupported` shape as the other optional capabilities,
    /// so a session that cannot place the cursor says so instead of reporting a click at a
    /// position nothing moved to.
    ///
    /// # Errors
    /// [`PlatformError::Unsupported`] from a backend without absolute placement,
    /// [`PlatformError::InvalidArgument`] for a coordinate that is not a finite number, and
    /// whatever the OS call reports.
    fn move_to(&self, x: f32, y: f32) -> Result<(), PlatformError> {
        Err(not_implemented((x, y), "move_to"))
    }

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
        // Absolute placement is optional in the same sense: a backend that cannot put the
        // pointer on a pixel must refuse, not report a click at a position nothing moved to.
        let err = backend
            .move_to(10.0, 20.0)
            .expect_err("no absolute placement");
        assert!(matches!(err, PlatformError::Unsupported { .. }), "{err:?}");
        assert!(
            err.user_facing().contains("move_to"),
            "the refusal names the operation: {err:?}"
        );
    }
}
