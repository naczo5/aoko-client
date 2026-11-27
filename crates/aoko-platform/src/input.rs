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
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
    Other(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    /// The first side button: `XBUTTON1` on Windows, evdev `BTN_SIDE`, X11 button 8.
    X1,
    /// The second side button: `XBUTTON2` on Windows, evdev `BTN_EXTRA`, X11 button 9.
    X2,
}

/// Current mouse state, sampled by automation loops.
///
/// Left and right only, because that is the whole of what a hold guard asks: a loop that must
/// not act while the user is holding a button reads one of these two fields.
/// [`MouseButton::Middle`], [`MouseButton::X1`] and [`MouseButton::X2`] are **event-only** -
/// they arrive as [`InputEvent::ButtonDown`] and [`InputEvent::ButtonUp`] and are never part of
/// a sampled state, so a bind on one fires from the event stream while no hold guard can see
/// it held.
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

/// Whether [`Key::Other`] carries a virtual-key code on the target this build is for.
///
/// The two backends disagree about what that variant means, and each says so where it is
/// built: `windows/input.rs::keyboard_proc` stores the hook's `vkCode`, so every Windows key
/// arrives as `Key::Other(vk)`, while `evdev_table::key_from_code` stores the raw
/// `linux/input-event-codes.h` number, which is a different number for the same physical key.
const OTHER_IS_VIRTUAL_KEY: bool = cfg!(windows);

/// The [`Key`] a Win32 virtual-key code names, for the codes a module bind can carry.
///
/// A saved profile stores a module's key as the **virtual-key code** the managed host
/// wrote (`InputHooks.ModuleKeys`, and every `Profile.ModuleKeys` entry), because that is
/// what `GetAsyncKeyState`/the hook compared against. This is the code-to-key direction:
/// the letters and digits, whose virtual-key codes *are* their ASCII uppercase value
/// (`VK_A` = 0x41 ... `VK_0` = 0x30), plus every named key [`Key`] has a variant for.
///
/// Anything else is `None` rather than a guess: an unknown code must mean "no bind fires"
/// and not "some other key fires", because the result is a module being switched on. The
/// navigation cluster is named by the variants [`Key`] has for it (`VK_PRIOR`..`VK_DELETE`),
/// and the OEM punctuation codes answer the character the US layout types there
/// (`VK_OEM_1` is `;`), which [`Key::virtual_key`] reads back as the same code. Answering
/// `Key::Other(code)` for either group would read as a raw *evdev* code on every target but
/// Windows — a misfire waiting to happen, not a binding.
///
/// [`Key::virtual_key`] is the other direction, and the one a module keybind is compared
/// through: a [`Key`] is a spelling, and one physical key has more than one of those.
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
        // The navigation cluster: `VK_PRIOR` (Page Up) to `VK_DELETE`.
        0x21 => Some(Key::PageUp),
        0x22 => Some(Key::PageDown),
        0x23 => Some(Key::End),
        0x24 => Some(Key::Home),
        0x25 => Some(Key::Left),
        0x26 => Some(Key::Up),
        0x27 => Some(Key::Right),
        0x28 => Some(Key::Down),
        0x2D => Some(Key::Insert),
        0x2E => Some(Key::Delete),
        // The US layout's `VK_OEM_*` punctuation, named by the character it types there, which
        // is the spelling `Key::virtual_key` answers the same code from and the one
        // `evdev_table::key_from_code` gives the same physical key. `VK_OEM_102` is the extra
        // key left of `Z`, which types `\` where it exists and is `KEY_102ND` here.
        0xBA => Some(Key::Char(';')),
        0xBB => Some(Key::Char('=')),
        0xBC => Some(Key::Char(',')),
        0xBD => Some(Key::Char('-')),
        0xBE => Some(Key::Char('.')),
        0xBF => Some(Key::Char('/')),
        0xC0 => Some(Key::Char('`')),
        0xDB => Some(Key::Char('[')),
        0xDC => Some(Key::Char('\\')),
        0xDD => Some(Key::Char(']')),
        0xDE => Some(Key::Char('\'')),
        // VK_F1 = 0x70, VK_F24 = 0x87.
        0x70..=0x87 => Some(Key::F((code - 0x70 + 1) as u8)),
        _ => None,
    }
}

impl Key {
    /// The virtual-key code this key is, `None` when it names none.
    ///
    /// The rule a module keybind is dispatched through, and the inverse of
    /// [`key_for_virtual_key`] for every key that answers both. It is a **code** comparison
    /// and not a `Key` comparison on purpose: the managed hook tested
    /// `kb.VkCode == kvp.Value` (`InputHooks.cs:411`), and a [`Key`] is a spelling that
    /// the two observers spell differently. `Key::Char('a')` arrives from evdev
    /// (`evdev_table::key_from_code(30)`), `Key::Char('A')` from a capture, `Key::Other(0x41)`
    /// from the Windows hook (`windows/input.rs::keyboard_proc`), and all three are `VK_A`.
    /// Comparing the values would make a letter bind fire on Linux only for the wrong case
    /// and never at all on Windows.
    #[must_use]
    pub fn virtual_key(self) -> Option<i32> {
        match self {
            Key::Char(character) => {
                let upper = character.to_ascii_uppercase();
                match upper {
                    'A'..='Z' => Some(i32::from(upper as u8 - b'A') + 0x41),
                    '0'..='9' => Some(i32::from(upper as u8 - b'0') + 0x30),
                    // The US layout's punctuation, which is how `evdev_table` names the OEM
                    // keys: `KEY_SEMICOLON` is `Key::Char(';')` there, and the same table is
                    // what a bind is compared through. The managed hook reports these as
                    // `VK_OEM_*`, so a bound `;` is `VK_OEM_1` on either platform.
                    ';' => Some(0xBA),
                    '=' => Some(0xBB),
                    ',' => Some(0xBC),
                    '-' => Some(0xBD),
                    '.' => Some(0xBE),
                    '/' => Some(0xBF),
                    '`' => Some(0xC0),
                    '[' => Some(0xDB),
                    '\\' => Some(0xDC),
                    ']' => Some(0xDD),
                    '\'' => Some(0xDE),
                    _ => None,
                }
            }
            Key::Shift => Some(0x10),
            Key::Control => Some(0x11),
            Key::Alt => Some(0x12),
            Key::Space => Some(0x20),
            Key::Enter => Some(0x0D),
            Key::Escape => Some(0x1B),
            Key::Tab => Some(0x09),
            Key::Backspace => Some(0x08),
            Key::Left => Some(0x25),
            Key::Right => Some(0x27),
            Key::Up => Some(0x26),
            Key::Down => Some(0x28),
            // The navigation cluster, `VK_PRIOR` (Page Up) to `VK_DELETE`.
            Key::PageUp => Some(0x21),
            Key::PageDown => Some(0x22),
            Key::End => Some(0x23),
            Key::Home => Some(0x24),
            Key::Insert => Some(0x2D),
            Key::Delete => Some(0x2E),
            Key::F(number) if (1..=24).contains(&number) => Some(0x6F + i32::from(number)),
            Key::F(_) => None,
            // On Windows this variant *is* the hook's `vkCode`, so it answers for the whole
            // keyboard - every code the hook reports, named above or not. Elsewhere it is an
            // evdev code and names no virtual key.
            Key::Other(code) if OTHER_IS_VIRTUAL_KEY => i32::try_from(code).ok(),
            Key::Other(_) => None,
        }
    }
}

pub trait InputBackend: Send + Sync {
    /// Sampled mouse state, or `None` when this backend cannot see the mouse at all. On Linux
    /// that means no readable `/dev/input/event*` **and** no display to query: either one alone
    /// is an answer, the first from the evdev reader and the second from `XQueryPointer`.
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

    #[test]
    fn the_letters_and_digits_are_their_ascii_codes_from_either_spelling() {
        // A capture reports the lower-case name (`keybind_text::virtual_key("a")` is 0x41),
        // evdev reports `Key::Char('a')` and a synthesised press may carry `'A'`: all three are
        // `VK_A`, which is why the comparison is a code and not a `Key`.
        for character in ['a', 'A', 'z', 'Z'] {
            let code = Key::Char(character)
                .virtual_key()
                .expect("a letter has a code");
            assert_eq!(
                key_for_virtual_key(code).and_then(Key::virtual_key),
                Some(code),
                "{character} round trip"
            );
        }
        assert_eq!(Key::Char('a').virtual_key(), Some(0x41));
        assert_eq!(Key::Char('Z').virtual_key(), Some(0x5A));
        assert_eq!(Key::Char('0').virtual_key(), Some(0x30));
        assert_eq!(Key::Char('9').virtual_key(), Some(0x39));
        // `Key::Char` is not only letters: the OEM punctuation carries the code the US layout
        // puts it on, and a symbol that has none answers `None` rather than a guess.
        assert_eq!(Key::Char(';').virtual_key(), Some(0xBA), "VK_OEM_1");
        assert_eq!(Key::Char('\u{00e9}').virtual_key(), None);
    }

    #[test]
    fn the_named_keys_and_function_keys_carry_the_windows_codes() {
        for (key, code) in [
            (Key::Backspace, 0x08),
            (Key::Tab, 0x09),
            (Key::Enter, 0x0D),
            (Key::Shift, 0x10),
            (Key::Control, 0x11),
            (Key::Alt, 0x12),
            (Key::Escape, 0x1B),
            (Key::Space, 0x20),
            (Key::Left, 0x25),
            (Key::Up, 0x26),
            (Key::Right, 0x27),
            (Key::Down, 0x28),
            (Key::F(1), 0x70),
            (Key::F(12), 0x7B),
            (Key::F(24), 0x87),
        ] {
            assert_eq!(key.virtual_key(), Some(code), "{key:?}");
            assert_eq!(
                key_for_virtual_key(code),
                Some(key),
                "{code:#04x} names the same key"
            );
        }
        // No code, no bind: a function key past F24 is not a key either backend reports.
        assert_eq!(Key::F(0).virtual_key(), None);
        assert_eq!(Key::F(25).virtual_key(), None);
        assert_eq!(key_for_virtual_key(0), None);
    }

    #[test]
    fn a_raw_code_is_a_virtual_key_only_where_the_backend_says_it_is() {
        // The Windows hook builds every event as `Key::Other(vkCode)`, so it must answer with
        // that code: on that target the raw variant is the spelling of the whole keyboard.
        let raw = Key::Other(0xBA);
        if OTHER_IS_VIRTUAL_KEY {
            assert_eq!(raw.virtual_key(), Some(0xBA));
            assert_eq!(Key::Other(0x24).virtual_key(), Some(0x24), "VK_HOME");
        } else {
            // On Linux the same variant is a `linux/input-event-codes.h` number, which is a
            // different number for the same physical key: answering with it would fire the
            // bind for whatever key happens to hold that virtual-key code.
            assert_eq!(raw.virtual_key(), None);
        }
        // A code that does not fit the profile's `i32` cannot be one either.
        assert_eq!(Key::Other(u32::MAX).virtual_key(), None);
    }

    #[test]
    fn the_navigation_cluster_round_trips_through_its_virtual_keys() {
        // The six the navigation cluster has, and the codes they are: answering `None` for
        // these kept a `Home` bind from ever firing on Linux, which has no other name for them.
        for (key, code) in [
            (Key::PageUp, 0x21),
            (Key::PageDown, 0x22),
            (Key::End, 0x23),
            (Key::Home, 0x24),
            (Key::Insert, 0x2D),
            (Key::Delete, 0x2E),
        ] {
            assert_eq!(key.virtual_key(), Some(code), "{key:?}");
            assert_eq!(key_for_virtual_key(code), Some(key), "{code:#04x}");
        }
    }

    #[test]
    fn the_oem_punctuation_round_trips_through_the_us_layout_codes() {
        // The eleven `VK_OEM_*` codes and the character a US layout types there. Both
        // directions are spelled out in the same block above, so a bind prints as its character
        // (`keybind_text::OEM_KEYS`) and an evdev `Key::Char(';')` still reaches it.
        for (key, code) in [
            (Key::Char(';'), 0xBA),
            (Key::Char('='), 0xBB),
            (Key::Char(','), 0xBC),
            (Key::Char('-'), 0xBD),
            (Key::Char('.'), 0xBE),
            (Key::Char('/'), 0xBF),
            (Key::Char('`'), 0xC0),
            (Key::Char('['), 0xDB),
            (Key::Char('\\'), 0xDC),
            (Key::Char(']'), 0xDD),
            (Key::Char('\''), 0xDE),
        ] {
            assert_eq!(key.virtual_key(), Some(code), "{key:?}");
            assert_eq!(key_for_virtual_key(code), Some(key), "{code:#04x}");
        }
        // A shifted symbol is not a key of its own: evdev names `KEY_SEMICOLON` `';'`, so `':'`
        // — which a capture stores as the same code (`VK_OEM_1`) — has no character here.
        assert_eq!(Key::Char(':').virtual_key(), None);
    }
}
