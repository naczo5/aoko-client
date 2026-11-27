//! Global input on Linux: XTest for synthesis and state, evdev for observation.
//!
//! Synthesis prefers X11 XTest because it needs no privileges and reaches every
//! X11/XWayland client. When there is no display (a pure Wayland session) it
//! falls back to `/dev/uinput`, which is what the compositor itself uses. When
//! neither is available, every synthesis call returns an actionable
//! [`PlatformError`]; nothing silently no-ops.
//!
//! State sampling uses `XQueryKeymap`/`XQueryPointer`, which are round trips but
//! cheap, and are what the managed host's `GetAsyncKeyState` equivalent costs. The one
//! question the pointer mask answers wrongly is *who* pressed: XTest and `uinput`
//! presses are in it too, so a held button is read from the evdev reader's own
//! transitions whenever it is watching a device, and the mask answers only when it is not.

use std::sync::Mutex;

use x11rb::connection::{Connection as _, RequestConnection as _};
use x11rb::protocol::xtest::ConnectionExt as _;

use crate::error::PlatformError;
use crate::input::{InputBackend, InputEvent, Key, MouseButton, MouseState};

use crate::evdev_table::{all_letter_codes, code_for_key};

use super::evdev::{EvdevObserver, VIRTUAL_DEVICE_NAME};
use super::uinput::{self, UinputDevice};
use super::x11::X11;

/// X11 keysyms this backend can name.
const XK_BACKSPACE: u32 = 0xff08;
const XK_TAB: u32 = 0xff09;
const XK_RETURN: u32 = 0xff0d;
const XK_ESCAPE: u32 = 0xff1b;
const XK_SPACE: u32 = 0x0020;
const XK_UP: u32 = 0xff52;
const XK_DOWN: u32 = 0xff54;
const XK_LEFT: u32 = 0xff51;
const XK_RIGHT: u32 = 0xff53;
const XK_HOME: u32 = 0xff50;
const XK_END: u32 = 0xff57;
/// `XK_Prior`, which is the keysym Page Up is on.
const XK_PRIOR: u32 = 0xff55;
/// `XK_Next`, which is the keysym Page Down is on.
const XK_NEXT: u32 = 0xff56;
const XK_INSERT: u32 = 0xff63;
const XK_DELETE: u32 = 0xffff;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_CONTROL_L: u32 = 0xffe3;
const XK_ALT_L: u32 = 0xffe9;
const XK_F1: u32 = 0xffbe;

/// XTest event types, from `X11/extensions/XTest.h`.
const XTEST_KEY_PRESS: u8 = 2;
const XTEST_KEY_RELEASE: u8 = 3;
const XTEST_BUTTON_PRESS: u8 = 4;
const XTEST_BUTTON_RELEASE: u8 = 5;
const XTEST_MOTION: u8 = 6;
/// `detail == 1` on a motion event means "relative to the current position".
const XTEST_MOTION_RELATIVE: u8 = 1;
/// `detail == 0` on a motion event means "absolute root-window coordinates".
const XTEST_MOTION_ABSOLUTE: u8 = 0;

/// X11 button numbers.
const BUTTON_LEFT: u8 = 1;
const BUTTON_MIDDLE: u8 = 2;
const BUTTON_RIGHT: u8 = 3;
/// Buttons 4 and 5 are the scroll wheel; a desktop numbers the two side buttons 8 and 9.
const BUTTON_X1: u8 = 8;
const BUTTON_X2: u8 = 9;

/// Keycodes the virtual uinput device declares up front: every letter, digit and
/// named key this backend can synthesize.
fn uinput_keys() -> Vec<u16> {
    let mut keys: Vec<u16> = all_letter_codes().collect();
    keys.extend_from_slice(&[
        crate::evdev_table::KEY_ESC,
        crate::evdev_table::KEY_BACKSPACE,
        crate::evdev_table::KEY_TAB,
        crate::evdev_table::KEY_ENTER,
        crate::evdev_table::KEY_LEFTCTRL,
        crate::evdev_table::KEY_RIGHTCTRL,
        crate::evdev_table::KEY_LEFTSHIFT,
        crate::evdev_table::KEY_RIGHTSHIFT,
        crate::evdev_table::KEY_LEFTALT,
        crate::evdev_table::KEY_RIGHTALT,
        crate::evdev_table::KEY_SPACE,
    ]);
    for number in 1..=12u16 {
        keys.push(crate::evdev_table::KEY_F1 + number - 1);
    }
    keys.extend_from_slice(&[
        crate::evdev_table::KEY_UP,
        crate::evdev_table::KEY_LEFT,
        crate::evdev_table::KEY_RIGHT,
        crate::evdev_table::KEY_DOWN,
        crate::evdev_table::KEY_HOME,
        crate::evdev_table::KEY_END,
        crate::evdev_table::KEY_PAGEUP,
        crate::evdev_table::KEY_PAGEDOWN,
        crate::evdev_table::KEY_INSERT,
        crate::evdev_table::KEY_DELETE,
    ]);
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// The `BTN_*` codes the virtual device declares: the three display buttons and the two side
/// ones, so no synthesised press names a code the device never advertised.
fn uinput_buttons() -> [u16; 5] {
    [
        super::uinput::BTN_LEFT,
        super::uinput::BTN_RIGHT,
        super::uinput::BTN_MIDDLE,
        super::evdev::BTN_SIDE,
        super::evdev::BTN_EXTRA,
    ]
}

/// The X11 button number one [`MouseButton`] is.
///
/// 1, 2 and 3 are the display buttons. Buttons 4 and 5 are the scroll wheel's two directions,
/// which leaves 8 and 9 for the side buttons - the numbering a desktop's back and forward
/// buttons arrive as.
fn x11_button(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => BUTTON_LEFT,
        MouseButton::Middle => BUTTON_MIDDLE,
        MouseButton::Right => BUTTON_RIGHT,
        MouseButton::X1 => BUTTON_X1,
        MouseButton::X2 => BUTTON_X2,
    }
}

/// The `linux/input-event-codes.h` `BTN_*` code one [`MouseButton`] is.
fn evdev_button(button: MouseButton) -> u16 {
    match button {
        MouseButton::Left => super::uinput::BTN_LEFT,
        MouseButton::Right => super::uinput::BTN_RIGHT,
        MouseButton::Middle => super::uinput::BTN_MIDDLE,
        MouseButton::X1 => super::evdev::BTN_SIDE,
        MouseButton::X2 => super::evdev::BTN_EXTRA,
    }
}

pub struct LinuxInputBackend {
    /// Present when a display was reachable; also provides state sampling.
    x11: Option<X11>,
    /// `(keysym, keycode)` for the user's layout, so a logical key resolves
    /// against the real keyboard rather than a hard-coded US keycode.
    keymap: Vec<(u32, u8)>,
    /// Whether the display offers the XTest extension, resolved once.
    xtest: bool,
    /// Created lazily, on the first synthesis call that needs it.
    uinput: Mutex<Option<UinputDevice>>,
    /// The classified failure from the last attempt, so a caller that retries does
    /// not have to re-derive why the device could not be created.
    uinput_error: Mutex<Option<PlatformError>>,
    observer: Option<EvdevObserver>,
    observer_error: Option<PlatformError>,
}

impl LinuxInputBackend {
    /// Builds a backend from whatever the session offers. Never fails: the
    /// capability report and per-call errors carry the gaps.
    #[must_use]
    pub fn new() -> Self {
        let x11 = X11::connect().ok();
        let keymap = x11.as_ref().map(X11::keysym_table).unwrap_or_default();
        let xtest = x11.as_ref().is_some_and(|connection| {
            connection
                .conn
                .extension_information(x11rb::protocol::xtest::X11_EXTENSION_NAME)
                .ok()
                .flatten()
                .is_some()
        });

        let (observer, observer_error) = match EvdevObserver::start(x11.as_ref()) {
            Ok(observer) => (Some(observer), None),
            Err(err) => (None, Some(err)),
        };

        Self {
            x11,
            keymap,
            xtest,
            uinput: Mutex::new(None),
            uinput_error: Mutex::new(None),
            observer,
            observer_error,
        }
    }

    /// Why global observation is unavailable, if it is.
    #[must_use]
    pub fn observation_error(&self) -> Option<&PlatformError> {
        self.observer_error.as_ref()
    }

    /// Whether synthesis goes through XTest rather than `/dev/uinput`.
    #[must_use]
    pub fn has_xtest(&self) -> bool {
        self.xtest
    }

    /// The physical buttons the evdev reader has seen held, or `None` when it is watching no
    /// device and so is maintaining nothing.
    fn observed_buttons(&self) -> Option<MouseState> {
        self.observer.as_ref()?.physical_mouse_state()
    }

    #[must_use]
    pub fn has_uinput(&self) -> bool {
        uinput::is_available()
    }

    fn require_x11(&self) -> Result<&X11, PlatformError> {
        self.x11.as_ref().ok_or_else(|| {
            PlatformError::unsupported(
                "no X11 display: synthesis falls back to /dev/uinput, which is also unavailable",
            )
        })
    }

    fn keycode(&self, key: Key) -> Option<u8> {
        let keysym = keysym_for(key)?;
        self.keymap
            .iter()
            .find(|(mapped, _)| *mapped == keysym)
            .map(|(_, code)| *code)
    }

    fn xtest(&self, type_: u8, detail: u8, dx: i16, dy: i16) -> Result<(), PlatformError> {
        let x11 = self.require_x11()?;
        x11.conn
            .xtest_fake_input(type_, detail, 0, x11.root, dx, dy, 0)
            .map_err(|err| PlatformError::os("XTestFakeInput", err.to_string()))?;
        x11.conn
            .flush()
            .map_err(|err| PlatformError::os("XFlush", err.to_string()))
    }

    /// Lazily creates the virtual device; records why it failed so the error is
    /// reported once with the actionable text rather than per call.
    fn with_uinput<T>(
        &self,
        operation: impl FnOnce(&mut UinputDevice) -> Result<T, PlatformError>,
    ) -> Result<T, PlatformError> {
        let mut guard = self
            .uinput
            .lock()
            .map_err(|_| PlatformError::os("uinput mutex", "poisoned"))?;

        if guard.is_none() {
            // Return the stored failure exactly as it was classified. Re-wrapping it
            // here used to relabel a missing module (`Os`) as `PermissionDenied` on
            // every retry, which points the user at the wrong remedy.
            if let Ok(mut error) = self.uinput_error.lock() {
                if let Some(error) = error.take() {
                    return Err(error);
                }
            }
            let keys = uinput_keys();
            let device = UinputDevice::create(VIRTUAL_DEVICE_NAME, &keys, &uinput_buttons());
            match device {
                Ok(device) => *guard = Some(device),
                Err(err) => {
                    if let Ok(mut error) = self.uinput_error.lock() {
                        *error = Some(err.clone());
                    }
                    return Err(err);
                }
            }
        }

        let Some(device) = guard.as_mut() else {
            return Err(PlatformError::os("uinput", "device disappeared"));
        };
        operation(device)
    }

    fn synthesize_key(&self, key: Key, pressed: bool) -> Result<(), PlatformError> {
        if self.has_xtest() {
            let code = self.keycode(key).ok_or_else(|| {
                PlatformError::unsupported(format!("{key:?} has no keycode in the current layout"))
            })?;
            let type_ = if pressed {
                XTEST_KEY_PRESS
            } else {
                XTEST_KEY_RELEASE
            };
            return self.xtest(type_, code, 0, 0);
        }

        let code = code_for_key(key).ok_or_else(|| {
            PlatformError::unsupported(format!("no input-event code for {key:?}"))
        })?;
        self.with_uinput(|device| device.emit_key(code, pressed))
    }

    fn synthesize_button(&self, button: MouseButton, pressed: bool) -> Result<(), PlatformError> {
        if self.has_xtest() {
            let type_ = if pressed {
                XTEST_BUTTON_PRESS
            } else {
                XTEST_BUTTON_RELEASE
            };
            return self.xtest(type_, x11_button(button), 0, 0);
        }

        self.with_uinput(|device| device.emit_key(evdev_button(button), pressed))
    }
}

impl Default for LinuxInputBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InputBackend for LinuxInputBackend {
    fn probe_mouse_state(&self) -> Option<MouseState> {
        // The reader is the better observer: it saw every press and release of the user's and
        // none this process synthesised, because its own `uinput` node is filtered out by name.
        // The X11 mask answers for both, so a click of ours read back as the user holding the
        // button and the hold guards stood down over their own input.
        if let Some(held) = self.observed_buttons() {
            return Some(held);
        }
        // Nothing is watching a device, so the mask is all there is. No display means no way to
        // see buttons at all; that is `None`, not idle.
        let x11 = self.x11.as_ref()?;
        let (_, _, mask) = x11.pointer()?;
        Some(MouseState {
            left_down: mask & 0x0100 != 0,
            right_down: mask & 0x0400 != 0,
        })
    }

    fn mouse_state(&self) -> MouseState {
        self.probe_mouse_state().unwrap_or_default()
    }

    fn probe_key_state(&self, key: Key) -> Option<bool> {
        let x11 = self.x11.as_ref()?;
        // A key the layout cannot name is unknowable, not up.
        let code = self.keycode(key)?;
        let keymap = x11.keymap()?;
        let index = usize::from(code) / 8;
        let bit = 1u8 << (code % 8);
        Some(keymap.get(index).is_some_and(|byte| *byte & bit != 0))
    }

    fn key_state(&self, key: Key) -> bool {
        self.probe_key_state(key).unwrap_or(false)
    }

    fn click(&self, button: MouseButton) -> Result<(), PlatformError> {
        self.synthesize_button(button, true)?;
        self.synthesize_button(button, false)
    }

    fn button_down(&self, button: MouseButton) -> Result<(), PlatformError> {
        self.synthesize_button(button, true)
    }

    fn button_up(&self, button: MouseButton) -> Result<(), PlatformError> {
        self.synthesize_button(button, false)
    }

    fn move_relative(&self, dx: i32, dy: i32) -> Result<(), PlatformError> {
        if (dx, dy) == (0, 0) {
            return Ok(());
        }
        if self.has_xtest() {
            let dx = i16::try_from(dx.clamp(i32::from(i16::MIN), i32::from(i16::MAX)))
                .map_err(|_| PlatformError::os("move_relative", "delta out of range"))?;
            let dy = i16::try_from(dy.clamp(i32::from(i16::MIN), i32::from(i16::MAX)))
                .map_err(|_| PlatformError::os("move_relative", "delta out of range"))?;
            return self.xtest(XTEST_MOTION, XTEST_MOTION_RELATIVE, dx, dy);
        }
        self.with_uinput(|device| device.emit_relative(dx, dy))
    }

    /// Absolute placement through XTest, which is the platform's `SetCursorPos`.
    ///
    /// XTest's motion event takes the *root window* coordinates directly when `detail` is 0
    /// (`XTEST_MOTION_ABSOLUTE`), so no pointer query and no delta accumulate: the pointer lands
    /// on the pixel. X11's protocol carries those coordinates as `i16`, which is the whole root
    /// window, so a point outside that range is refused as an argument error rather than wrapped
    /// into a different corner of the screen.
    ///
    /// With no XTest there is no absolute path: the uinput device declares relative axes only
    /// (`REL_X`/`REL_Y`), and a compositor applies its own pointer acceleration to them, so
    /// "absolute" through that device would be a guess. It refuses, which is the honest answer
    /// and the one [`InputBackend::move_to`]'s contract allows.
    fn move_to(&self, x: f32, y: f32) -> Result<(), PlatformError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(PlatformError::invalid_argument(format!(
                "move_to({x}, {y}) is not a screen coordinate"
            )));
        }
        if !self.has_xtest() {
            return Err(PlatformError::unsupported(
                "absolute pointer placement needs XTest; the uinput device declares relative axes \
                 only, so it cannot be aimed at a pixel",
            ));
        }

        let out_of_range = || {
            PlatformError::invalid_argument(format!(
                "move_to({x}, {y}) is outside the X11 root window's i16 coordinate range"
            ))
        };
        let x = i16::try_from(x.round() as i32).map_err(|_| out_of_range())?;
        let y = i16::try_from(y.round() as i32).map_err(|_| out_of_range())?;
        self.xtest(XTEST_MOTION, XTEST_MOTION_ABSOLUTE, x, y)
    }

    fn key_press(&self, key: Key) -> Result<(), PlatformError> {
        self.synthesize_key(key, true)?;
        self.synthesize_key(key, false)
    }

    fn key_down(&self, key: Key) -> Result<(), PlatformError> {
        self.synthesize_key(key, true)
    }

    fn key_up(&self, key: Key) -> Result<(), PlatformError> {
        self.synthesize_key(key, false)
    }

    fn poll_events(&self, out: &mut Vec<InputEvent>) -> Result<(), PlatformError> {
        match self.observer.as_ref() {
            Some(observer) => {
                observer.drain_into(out);
                Ok(())
            }
            // `EvdevObserver::start` already distinguishes a real permission
            // problem (no readable `/dev/input/event*`) from an `Os` failure such as
            // a thread that would not spawn, so the variant is preserved.
            None => Err(self
                .observer_error
                .clone()
                .unwrap_or_else(|| PlatformError::unsupported("no evdev observer"))),
        }
    }

    fn mouse_position(&self) -> Option<(i32, i32)> {
        self.x11
            .as_ref()
            .and_then(X11::pointer)
            .map(|(x, y, _)| (x, y))
    }
}

/// Logical key to X11 keysym.
fn keysym_for(key: Key) -> Option<u32> {
    match key {
        Key::Char(character) => {
            let character = character.to_ascii_lowercase();
            if character.is_ascii_alphanumeric() {
                Some(u32::from(character))
            } else {
                None
            }
        }
        Key::Space => Some(XK_SPACE),
        Key::Enter => Some(XK_RETURN),
        Key::Escape => Some(XK_ESCAPE),
        Key::Tab => Some(XK_TAB),
        Key::Backspace => Some(XK_BACKSPACE),
        Key::Shift => Some(XK_SHIFT_L),
        Key::Control => Some(XK_CONTROL_L),
        Key::Alt => Some(XK_ALT_L),
        Key::Left => Some(XK_LEFT),
        Key::Right => Some(XK_RIGHT),
        Key::Up => Some(XK_UP),
        Key::Down => Some(XK_DOWN),
        Key::Home => Some(XK_HOME),
        Key::End => Some(XK_END),
        Key::PageUp => Some(XK_PRIOR),
        Key::PageDown => Some(XK_NEXT),
        Key::Insert => Some(XK_INSERT),
        Key::Delete => Some(XK_DELETE),
        Key::F(number) if (1..=12).contains(&number) => Some(XK_F1 + u32::from(number) - 1),
        Key::F(_) => None,
        Key::Other(code) => Some(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysym_mapping_covers_the_documented_keys() {
        assert_eq!(keysym_for(Key::Char('a')), Some(0x61));
        assert_eq!(keysym_for(Key::Char('A')), Some(0x61));
        assert_eq!(keysym_for(Key::Char('7')), Some(0x37));
        assert_eq!(keysym_for(Key::Space), Some(XK_SPACE));
        assert_eq!(keysym_for(Key::Enter), Some(XK_RETURN));
        assert_eq!(keysym_for(Key::F(1)), Some(XK_F1));
        assert_eq!(keysym_for(Key::F(12)), Some(XK_F1 + 11));
        assert_eq!(keysym_for(Key::F(13)), None);
        assert_eq!(keysym_for(Key::Char('!')), None);
        // The navigation cluster's keysyms are `XK_Prior` and `XK_Next` for the two paging
        // keys, and `XK_Delete` is 0xffff rather than next to the arrows.
        assert_eq!(keysym_for(Key::Home), Some(0xff50));
        assert_eq!(keysym_for(Key::End), Some(0xff57));
        assert_eq!(keysym_for(Key::PageUp), Some(0xff55));
        assert_eq!(keysym_for(Key::PageDown), Some(0xff56));
        assert_eq!(keysym_for(Key::Insert), Some(0xff63));
        assert_eq!(keysym_for(Key::Delete), Some(0xffff));
    }

    #[test]
    fn every_button_has_its_own_x11_number_and_evdev_code() {
        // The side buttons are the regression: a capture could store `Mouse X1`/`X2` while
        // nothing on either platform could produce the press.
        assert_eq!(x11_button(MouseButton::Left), 1);
        assert_eq!(x11_button(MouseButton::Middle), 2);
        assert_eq!(x11_button(MouseButton::Right), 3);
        assert_eq!(x11_button(MouseButton::X1), 8);
        assert_eq!(x11_button(MouseButton::X2), 9);
        assert_eq!(evdev_button(MouseButton::Left), 0x110);
        assert_eq!(evdev_button(MouseButton::Right), 0x111);
        assert_eq!(evdev_button(MouseButton::Middle), 0x112);
        assert_eq!(evdev_button(MouseButton::X1), 0x113);
        assert_eq!(evdev_button(MouseButton::X2), 0x114);
        // The virtual device declares every code a press can carry, or a synthesised click
        // would name a button the device never advertised.
        assert_eq!(uinput_buttons(), [0x110, 0x111, 0x112, 0x113, 0x114]);
    }

    #[test]
    fn the_uinput_device_declares_every_synthesizable_key() {
        let keys = uinput_keys();
        assert!(keys.len() > 40);
        assert!(
            keys.windows(2).all(|pair| pair[0] < pair[1]),
            "sorted and deduped"
        );
        assert!(keys.contains(&30), "letter codes are declared");
        assert!(keys.contains(&59), "F1 is declared");
        // The navigation cluster is synthesizable too: the Wayland path has to be able to
        // emit the keys a bind can now carry.
        for code in [102, 104, 107, 109, 110, 111] {
            assert!(keys.contains(&code), "code {code} is declared");
        }
    }

    #[test]
    fn unsupported_keys_report_instead_of_guessing() {
        assert!(code_for_key(Key::Char('!')).is_none());
        assert!(code_for_key(Key::Other(0x1_0000)).is_none());
    }

    #[test]
    fn a_backend_constructs_without_a_display_or_input_permissions() {
        let backend = LinuxInputBackend::new();
        // Sampling must not panic and must report a definite answer.
        let _ = backend.mouse_state();
        let _ = backend.key_state(Key::Char('a'));
        let _ = backend.mouse_position();

        if !backend.has_xtest() {
            // Without XTest every synthesis call must be a typed error or a
            // successful uinput write; never a panic.
            match backend.click(MouseButton::Left) {
                Ok(()) => assert!(backend.has_uinput()),
                Err(err) => assert!(!err.user_facing().is_empty()),
            }
        }
    }

    #[test]
    fn absolute_placement_refuses_without_a_display_and_never_panics() {
        let backend = LinuxInputBackend::new();

        // A non-finite point is refused before any X11 call, so this is safe headless.
        for (x, y) in [(f32::NAN, 0.0), (0.0, f32::INFINITY)] {
            let err = backend.move_to(x, y).expect_err("not a screen coordinate");
            assert!(
                matches!(err, PlatformError::InvalidArgument { .. }),
                "{err:?}"
            );
        }

        // With no XTest the placement is refused and the reason names the uinput device's
        // relative axes. The XTest path itself is not exercised here: it would move the pointer
        // of whichever desktop runs the suite, which the other synthesis tests avoid too.
        if !backend.has_xtest() {
            let err = backend
                .move_to(10.0, 20.0)
                .expect_err("uinput cannot place absolutely");
            assert!(
                matches!(err, PlatformError::Unsupported { .. }),
                "a session that cannot place must say why: {err:?}"
            );
        }
    }

    #[test]
    fn a_stored_failure_is_returned_as_is_rather_than_reclassified() {
        let backend = LinuxInputBackend::new();
        // A `Os` classification, which is what a missing uinput module produces. The
        // cached path used to rebuild this as `PermissionDenied`.
        let stored = PlatformError::os("open /dev/uinput", "No such file or directory");
        *backend
            .uinput_error
            .lock()
            .expect("the failure cache is not poisoned") = Some(stored.clone());

        let returned = backend
            .with_uinput(|_| Ok(()))
            .expect_err("the stored failure is handed back");
        assert_eq!(
            returned, stored,
            "the cached error must come back unchanged"
        );
    }

    #[test]
    fn a_cached_synthesis_failure_keeps_the_classification_it_was_given() {
        let backend = LinuxInputBackend::new();
        // Two calls reach both paths: the first attempt classifies, the second reads
        // the stored failure back.
        let first = backend.with_uinput(|_| Ok(()));
        let second = backend.with_uinput(|_| Ok(()));

        if first.is_ok() && second.is_ok() {
            // A writable /dev/uinput: there is no failure to classify here.
            return;
        }

        let first = first.expect_err("the first attempt failed");
        let second = second.expect_err("the cached attempt failed");
        assert_eq!(
            std::mem::discriminant(&first),
            std::mem::discriminant(&second),
            "the cached failure must not be re-classified: {first:?} then {second:?}"
        );
        assert!(
            !matches!(second, PlatformError::PermissionDenied { .. })
                || matches!(first, PlatformError::PermissionDenied { .. }),
            "a failure that was not a permission problem must not become one: {second:?}"
        );
    }

    #[test]
    fn sampling_reports_unknown_rather_than_idle_without_a_display() {
        let backend = LinuxInputBackend::new();
        match backend.probe_mouse_state() {
            Some(state) => {
                // A display answered, so the answer is definite and the lossy form
                // must agree with it rather than contradicting it.
                assert_eq!(backend.mouse_state(), state);
            }
            None => {
                // No display: the honest form is the one a hold guard can branch
                // on, and the lossy form still degrades to idle.
                assert_eq!(backend.mouse_state(), MouseState::default());
                assert_eq!(backend.probe_key_state(Key::Char('a')), None);
            }
        }
    }

    #[test]
    fn poll_events_reports_the_observation_gap() {
        let backend = LinuxInputBackend::new();
        let mut events = Vec::new();
        match backend.poll_events(&mut events) {
            Ok(()) => assert!(backend.observation_error().is_none()),
            Err(err) => {
                assert!(backend.observation_error().is_some());
                assert!(!err.user_facing().is_empty());
            }
        }
    }
}
