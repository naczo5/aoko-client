//! Global input: low-level hooks for observation, `SendInput` for synthesis.
//!
//! Hook installation matches `InputHooks.Install`: `WH_KEYBOARD_LL` and
//! `WH_MOUSE_LL` with the host module handle, delivering events on the thread that
//! installed them while it pumps messages. Events land in a fixed-capacity queue
//! that [`InputBackend::poll_events`] drains, so the hook thread never allocates
//! per keystroke and never blocks on the consumer.
//!
//! Synthesis matches `Clicker.PerformClick`, `Clicker.SetPixelPartyWalkKey` and
//! `Clicker.TryApplyAimAssist`: a click is one `SendInput` of two `INPUT`s, a
//! relative move is one `MOUSEEVENTF_MOVE` `INPUT`, and a key is a down or up
//! `INPUT_KEYBOARD` event. A down/up pair is one batch because the managed call was one
//! batch, and also because a pair that arrives as two calls can be split by a failure and
//! leave the button or key down; `send` reports a partial batch and the pair releases
//! what it can before reporting it, so a failed click cannot leave the game holding the
//! attack button.

use std::sync::Mutex;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, VkKeyScanW, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE,
    KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
    MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_F1, VK_LBUTTON,
    VK_LEFT, VK_MENU, VK_RBUTTON, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetSystemMetrics, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK,
    KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use crate::error::PlatformError;
use crate::input::{InputBackend, InputEvent, Key, MouseButton, MouseState};

/// Queue depth. A burst larger than this means the consumer stopped draining, and
/// dropping the overflow is better than growing without bound.
const EVENT_CAPACITY: usize = 4096;

/// `LLMHF_INJECTED`: our own synthesized events, which the managed hook ignored.
const LLMHF_INJECTED: u32 = 0x0000_0001;

static EVENTS: Mutex<Vec<InputEvent>> = Mutex::new(Vec::new());
static HOOKS: Mutex<Option<HookGuard>> = Mutex::new(None);

pub struct WindowsInputBackend;

impl WindowsInputBackend {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsInputBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// An installed pair of low-level hooks, unhooked on drop.
///
/// This is the only owner: a caller that installs and then returns early drops the
/// guard and the hooks with it, instead of leaking two `HHOOK`s for the life of the
/// process. The handles are kept as `isize` so the guard is `Send` without an
/// `unsafe impl`; a hook is a plain token, and `UnhookWindowsHookEx` may be called
/// from any thread.
pub struct HookGuard {
    keyboard: isize,
    mouse: isize,
}

impl HookGuard {
    /// Installs both hooks, or neither.
    ///
    /// The installing thread must pump messages for them to fire, which is what the
    /// managed host's WPF dispatcher provided.
    pub fn install() -> Result<Self, PlatformError> {
        // SAFETY: a null module name asks for this process's own module, which is
        // the module that owns the hook procedures below.
        let module = unsafe { GetModuleHandleW(windows::core::PCWSTR::null()) }
            .map_err(|err| PlatformError::os("GetModuleHandleW", err.to_string()))?;
        let instance = windows::Win32::Foundation::HINSTANCE(module.0);

        // SAFETY: the callback is a `'static` function with the exact `HOOKPROC`
        // signature, and the module handle outlives the hook.
        let keyboard =
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), instance, 0) }
                .map_err(|err| {
                    PlatformError::os("SetWindowsHookExW(WH_KEYBOARD_LL)", err.to_string())
                })?;

        // From here on the keyboard hook is owned by `guard`: if the mouse install
        // fails, `?` drops the guard and unhooks it rather than leaking it.
        let mut guard = Self {
            keyboard: keyboard.0 as isize,
            mouse: 0,
        };

        // SAFETY: as above.
        let mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), instance, 0) }
            .map_err(|err| PlatformError::os("SetWindowsHookExW(WH_MOUSE_LL)", err.to_string()))?;
        guard.mouse = mouse.0 as isize;
        Ok(guard)
    }

    /// Passes each live handle to `unhook` exactly once and clears it.
    ///
    /// The callback is the seam the tests observe: with a recorder in place of
    /// `UnhookWindowsHookEx`, "a zero handle is never unhooked" is an assertion
    /// rather than a comment. `Drop` is the only production caller.
    fn unhook(&mut self, mut unhook: impl FnMut(isize)) {
        if self.keyboard != 0 {
            unhook(self.keyboard);
            self.keyboard = 0;
        }
        if self.mouse != 0 {
            unhook(self.mouse);
            self.mouse = 0;
        }
    }
}

impl Drop for HookGuard {
    fn drop(&mut self) {
        self.unhook(|handle| {
            // SAFETY: `unhook` only passes handles that came from a successful
            // `SetWindowsHookExW` in `install`, and clears each field before it
            // could be passed again, so this never sees a null or a stale `HHOOK`.
            let _ = unsafe { UnhookWindowsHookEx(HHOOK(handle as *mut core::ffi::c_void)) };
        });
    }
}

/// Installs both low-level hooks into a process-global owner that keeps them for
/// the life of the process.
///
/// Idempotent: the second call sees the existing guard and does nothing, and
/// concurrent callers serialise on the mutex instead of both passing the empty
/// check and leaking the handle that loses the race.
pub fn install_hooks() -> Result<(), PlatformError> {
    let mut installed = HOOKS
        .lock()
        .map_err(|_| PlatformError::os("input hooks", "poisoned"))?;
    if installed.is_some() {
        return Ok(());
    }
    *installed = Some(HookGuard::install()?);
    Ok(())
}

/// Removes both hooks. Safe to call when nothing is installed.
pub fn uninstall_hooks() {
    // A poisoned lock still holds the guard, so recover rather than leaking it.
    let taken = match HOOKS.lock() {
        Ok(mut slot) => slot.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    };
    drop(taken);
}

fn push_event(event: InputEvent) {
    if let Ok(mut queue) = EVENTS.lock() {
        if queue.len() < EVENT_CAPACITY {
            queue.push(event);
        }
    }
}

/// # Safety
/// Called by `user32` with a pointer to a valid `KBDLLHOOKSTRUCT` in `lparam`
/// while `code >= 0`, which is the documented low-level keyboard hook contract.
unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let data = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let key = Key::Other(data.vkCode);
        match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => push_event(InputEvent::KeyDown(key)),
            WM_KEYUP | WM_SYSKEYUP => push_event(InputEvent::KeyUp(key)),
            _ => {}
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

/// # Safety
/// Called by `user32` with a pointer to a valid `MSLLHOOKSTRUCT` in `lparam` while
/// `code >= 0`, which is the documented low-level mouse hook contract.
unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let data = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        // Ignore injected events so our own clicks are not observed as input.
        if data.flags & LLMHF_INJECTED == 0 {
            let message = wparam.0 as u32;
            let event = match message {
                WM_MOUSEMOVE => Some(InputEvent::MouseMove {
                    x: data.pt.x,
                    y: data.pt.y,
                }),
                WM_LBUTTONDOWN => Some(InputEvent::ButtonDown(MouseButton::Left)),
                WM_LBUTTONUP => Some(InputEvent::ButtonUp(MouseButton::Left)),
                WM_RBUTTONDOWN => Some(InputEvent::ButtonDown(MouseButton::Right)),
                WM_RBUTTONUP => Some(InputEvent::ButtonUp(MouseButton::Right)),
                WM_MBUTTONDOWN => Some(InputEvent::ButtonDown(MouseButton::Middle)),
                WM_MBUTTONUP => Some(InputEvent::ButtonUp(MouseButton::Middle)),
                // `InputEvent` has no extra-button variant, so X1/X2 are not
                // surfaced rather than being reported as a keyboard key.
                WM_XBUTTONDOWN | WM_XBUTTONUP => None,
                _ => None,
            };
            if let Some(event) = event {
                push_event(event);
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

fn virtual_key(key: Key) -> VIRTUAL_KEY {
    match key {
        Key::Char(character) => {
            let mut units = [0u16; 2];
            let encoded = character.to_ascii_uppercase().encode_utf16(&mut units);
            match encoded.first() {
                Some(unit) if *unit < 0x80 => {
                    // SAFETY: `VkKeyScanW` maps a character to the current layout's
                    // virtual key; it takes a value, not a pointer.
                    let scan = unsafe { VkKeyScanW(*unit) };
                    if scan != -1 {
                        return VIRTUAL_KEY((scan as u16) & 0x00ff);
                    }
                    VIRTUAL_KEY(*unit)
                }
                _ => VIRTUAL_KEY(0),
            }
        }
        Key::Shift => VK_SHIFT,
        Key::Control => VK_CONTROL,
        Key::Alt => VK_MENU,
        Key::Space => VK_SPACE,
        Key::Enter => VK_RETURN,
        Key::Escape => VK_ESCAPE,
        Key::Tab => VK_TAB,
        Key::Backspace => VK_BACK,
        Key::Left => VK_LEFT,
        Key::Right => VK_RIGHT,
        Key::Up => VK_UP,
        Key::Down => VK_DOWN,
        Key::F(number) if (1..=24).contains(&number) => {
            VIRTUAL_KEY(VK_F1.0 + u16::from(number) - 1)
        }
        Key::F(_) => VIRTUAL_KEY(0),
        Key::Other(code) => VIRTUAL_KEY(u16::try_from(code).unwrap_or(0)),
    }
}

/// `(down, up)` mouse flags for one button. Each button has its own pair, so
/// reporting the middle button as a left click would make this backend and the
/// Linux one disagree about the same [`InputBackend`] method.
const fn mouse_flags(button: MouseButton) -> (MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS) {
    match button {
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    }
}

/// Flags for one absolute placement.
///
/// `MOUSEEVENTF_ABSOLUTE` makes `dx`/`dy` a position rather than a delta, and
/// `MOUSEEVENTF_VIRTUALDESK` normalises them over the whole virtual desktop instead of the
/// primary monitor, which is the coordinate space [`InputBackend::move_to`] documents.
///
/// Built per call rather than as a `const`: `MOUSE_EVENT_FLAGS` is a newtype whose `BitOr` is an
/// ordinary (non-`const`) operator, so the combination cannot be a constant item.
fn absolute_move_flags() -> MOUSE_EVENT_FLAGS {
    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK
}

/// The virtual desktop in pixels: the rectangle `MOUSEEVENTF_VIRTUALDESK` normalises into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VirtualScreen {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

/// Reads the virtual desktop metrics, or `None` when the session reports no usable area.
///
/// `GetSystemMetrics` has no error channel: a session with no display reports a zero-sized
/// desktop rather than failing, so the size is the check. A zero size would otherwise divide the
/// placement back to the origin, which is a click in the corner of the screen rather than a
/// refusal.
fn virtual_screen() -> Option<VirtualScreen> {
    // SAFETY: `GetSystemMetrics` takes an index constant by value, touches no caller memory, and
    // answers an `i32` for both the metrics below.
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(VirtualScreen {
        left,
        top,
        width,
        height,
    })
}

/// One axis of `SendInput`'s absolute coordinate: `0..=65535` across `span` pixels.
///
/// The documented mapping is normalised 0 to 65535 onto the desktop, where 0 is the first pixel
/// and 65535 the last, so the denominator is `span - 1`. A one-pixel axis has no interval to
/// normalise into and answers the only coordinate that exists.
fn normalise_axis(coordinate: i32, span: i32) -> i32 {
    if span <= 1 {
        return 0;
    }
    let clamped = coordinate.clamp(0, span - 1);
    // `i64` so a wide desktop cannot overflow the product; the quotient is bounded by 65535, so
    // the narrowing back to `i32` is exact.
    ((i64::from(clamped) * 65535) / i64::from(span - 1)) as i32
}

fn mouse_input(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn keyboard_input(key: VIRTUAL_KEY, release: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if release {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), PlatformError> {
    let expected = u32::try_from(inputs.len())
        .map_err(|_| PlatformError::os("SendInput", "too many inputs in one call"))?;
    let size = i32::try_from(core::mem::size_of::<INPUT>())
        .map_err(|_| PlatformError::os("SendInput", "INPUT does not fit the size parameter"))?;
    // SAFETY: `inputs` is a live slice of correctly initialised `INPUT` values and
    // `size` is `sizeof(INPUT)`, which is what `SendInput` requires.
    let inserted = unsafe { SendInput(inputs, size) };
    if inserted == expected {
        return Ok(());
    }
    Err(PlatformError::os(
        "SendInput",
        format!(
            "inserted {inserted} of {expected} events: {}",
            windows::core::Error::from_win32()
        ),
    ))
}

const MOUSE_MOVE_FLAGS: MOUSE_EVENT_FLAGS = MOUSEEVENTF_MOVE;

fn vk_down(vk: VIRTUAL_KEY) -> bool {
    // SAFETY: `GetAsyncKeyState` takes the virtual-key code by value; a high bit
    // means the key is down. It reports nothing for a code outside the map.
    let state = unsafe { GetAsyncKeyState(i32::from(vk.0)) };
    (state as u16) & 0x8000 != 0
}

impl InputBackend for WindowsInputBackend {
    fn probe_mouse_state(&self) -> Option<MouseState> {
        // `GetAsyncKeyState` always has a definite answer on Windows.
        Some(MouseState {
            left_down: vk_down(VK_LBUTTON),
            right_down: vk_down(VK_RBUTTON),
        })
    }

    fn mouse_state(&self) -> MouseState {
        self.probe_mouse_state().unwrap_or_default()
    }

    fn probe_key_state(&self, key: Key) -> Option<bool> {
        let vk = virtual_key(key);
        // A key with no virtual-key code cannot be queried at all, which is
        // unknown rather than up - the same answer the Linux backend gives.
        if vk.0 == 0 {
            return None;
        }
        Some(vk_down(vk))
    }

    fn key_state(&self, key: Key) -> bool {
        self.probe_key_state(key).unwrap_or(false)
    }

    fn click(&self, button: MouseButton) -> Result<(), PlatformError> {
        let (down, up) = mouse_flags(button);
        // One call for the pair, like `Clicker.PerformClick`.
        if let Err(error) = send(&[mouse_input(down, 0, 0), mouse_input(up, 0, 0)]) {
            // A batch can be *partly* queued: `SendInput` returns how many of the two events it
            // inserted, so a refused or half-inserted pair can leave `LEFTDOWN` in the input
            // stream with nothing to release it. The game would then see the attack button held
            // for as long as the process lives - the clicker keeps clicking with the module off,
            // because the loop is not what is pressing - and no later click repairs it, since a
            // second down on a held button is not a press/release pair either.
            //
            // The compensation is best effort and cannot make the stream worse: an `UP` for a
            // button that is already up is a release event the game has nothing to release,
            // while a missing `UP` is a held button. The original error is what the caller sees.
            let _ = send(&[mouse_input(up, 0, 0)]);
            return Err(error);
        }
        Ok(())
    }

    fn button_down(&self, button: MouseButton) -> Result<(), PlatformError> {
        send(&[mouse_input(mouse_flags(button).0, 0, 0)])
    }

    fn button_up(&self, button: MouseButton) -> Result<(), PlatformError> {
        send(&[mouse_input(mouse_flags(button).1, 0, 0)])
    }

    fn move_relative(&self, dx: i32, dy: i32) -> Result<(), PlatformError> {
        if (dx, dy) == (0, 0) {
            return Ok(());
        }
        send(&[mouse_input(MOUSE_MOVE_FLAGS, dx, dy)])
    }

    /// Absolute placement: the managed `SetCursorPos` (`ChestStealerController.cs:303-306`),
    /// spelled with `SendInput` so the placement goes through the same injector as every other
    /// synthetic event and cannot be separated from the click that follows it by a desktop
    /// switch.
    ///
    /// Unlike `SetCursorPos`, `SendInput` takes the position normalised, so the point is mapped
    /// through the **virtual desktop** rectangle (`MOUSEEVENTF_VIRTUALDESK`): the primary
    /// monitor alone would put a game on a second screen out of reach and clamp a negative
    /// coordinate on a left-hand monitor to the primary's origin.
    fn move_to(&self, x: f32, y: f32) -> Result<(), PlatformError> {
        // A `NaN` or an infinity is not a position, and the saturating `as i32` cast below would
        // otherwise turn it into the origin - a click in the corner of the desktop.
        if !x.is_finite() || !y.is_finite() {
            return Err(PlatformError::invalid_argument(format!(
                "move_to({x}, {y}) is not a screen coordinate"
            )));
        }

        let screen = virtual_screen().ok_or_else(|| {
            PlatformError::os(
                "GetSystemMetrics",
                "the session reports an empty virtual desktop",
            )
        })?;

        // Round once, here, at the OS boundary: the caller's point is a fractional mapping of a
        // slot through the client rect, and a pixel of error is a pixel the click can miss.
        let pixel_x = x.round() as i32;
        let pixel_y = y.round() as i32;

        send(&[mouse_input(
            absolute_move_flags(),
            normalise_axis(pixel_x.saturating_sub(screen.left), screen.width),
            normalise_axis(pixel_y.saturating_sub(screen.top), screen.height),
        )])
    }

    fn key_press(&self, key: Key) -> Result<(), PlatformError> {
        let vk = virtual_key(key);
        if vk.0 == 0 {
            return Err(PlatformError::unsupported(format!(
                "{key:?} has no virtual-key code"
            )));
        }
        if let Err(error) = send(&[keyboard_input(vk, false), keyboard_input(vk, true)]) {
            // Same partial-batch hazard as [`Self::click`], with a worse symptom: a key stuck
            // down is a held movement input, so the player walks, jumps or sprints with the
            // module off until the game loses focus. Release it before reporting the failure.
            let _ = send(&[keyboard_input(vk, true)]);
            return Err(error);
        }
        Ok(())
    }

    fn key_down(&self, key: Key) -> Result<(), PlatformError> {
        let vk = virtual_key(key);
        if vk.0 == 0 {
            return Err(PlatformError::unsupported(format!(
                "{key:?} has no virtual-key code"
            )));
        }
        send(&[keyboard_input(vk, false)])
    }

    fn key_up(&self, key: Key) -> Result<(), PlatformError> {
        let vk = virtual_key(key);
        if vk.0 == 0 {
            return Err(PlatformError::unsupported(format!(
                "{key:?} has no virtual-key code"
            )));
        }
        send(&[keyboard_input(vk, true)])
    }

    fn poll_events(&self, out: &mut Vec<InputEvent>) -> Result<(), PlatformError> {
        let mut queue = EVENTS
            .lock()
            .map_err(|_| PlatformError::os("input queue", "poisoned"))?;
        out.extend(queue.drain(..));
        Ok(())
    }

    fn mouse_position(&self) -> Option<(i32, i32)> {
        // The managed host read the pointer only through `GetCursorInfo`; there is
        // no cheap absolute-position sample in the current hook path, so report
        // nothing instead of a stale value.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_input_uses_the_documented_flags() {
        let input = mouse_input(MOUSEEVENTF_LEFTDOWN, 0, 0);
        assert_eq!(input.r#type, INPUT_MOUSE);
        // SAFETY: the value was built with `r#type == INPUT_MOUSE`, so reading the
        // `mi` arm of the union is the arm that was initialised.
        let mi = unsafe { input.Anonymous.mi };
        assert_eq!(mi.dwFlags, MOUSEEVENTF_LEFTDOWN);
    }

    #[test]
    fn each_button_maps_to_its_own_flag_pair() {
        // The middle row is the regression: it used to reuse the left pair, so a
        // `click(Middle)` sent a left click.
        assert_eq!(
            mouse_flags(MouseButton::Left),
            (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP)
        );
        assert_eq!(
            mouse_flags(MouseButton::Right),
            (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP)
        );
        assert_eq!(
            mouse_flags(MouseButton::Middle),
            (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP)
        );
    }

    #[test]
    fn the_absolute_axis_maps_the_ends_of_the_desktop() {
        // The two ends of the documented range, then a midpoint, on a 1920-pixel axis: 0 is the
        // first pixel and 65535 the last, so the span that is divided is `width - 1`.
        assert_eq!(normalise_axis(0, 1920), 0);
        assert_eq!(normalise_axis(1919, 1920), 65535);
        assert_eq!(normalise_axis(960, 1920), (960 * 65535) / 1919);
    }

    #[test]
    fn an_out_of_range_absolute_axis_clamps_instead_of_wrapping() {
        assert_eq!(normalise_axis(-50, 1920), 0);
        assert_eq!(normalise_axis(i32::MAX, 1920), 65535);
        // A one-pixel (or zero, or negative) axis has a single coordinate, not a division by zero.
        assert_eq!(normalise_axis(0, 1), 0);
        assert_eq!(normalise_axis(7, 0), 0);
        assert_eq!(normalise_axis(7, -3), 0);
    }

    #[test]
    fn the_absolute_move_flags_carry_absolute_and_virtual_desk() {
        let flags = absolute_move_flags();
        for expected in [
            MOUSEEVENTF_MOVE,
            MOUSEEVENTF_ABSOLUTE,
            MOUSEEVENTF_VIRTUALDESK,
        ] {
            assert_eq!(
                flags.0 & expected.0,
                expected.0,
                "flag {:#x} is missing",
                expected.0
            );
        }
        // A relative move must not pick these up: it stays one `MOUSEEVENTF_MOVE`.
        assert_eq!(MOUSE_MOVE_FLAGS.0 & MOUSEEVENTF_ABSOLUTE.0, 0);
    }

    #[test]
    fn a_non_finite_placement_is_refused_before_anything_is_injected() {
        // The refusal happens before `send`, so this is safe without a desktop.
        let backend = WindowsInputBackend::new();
        for (x, y) in [
            (f32::NAN, 0.0),
            (0.0, f32::INFINITY),
            (f32::NEG_INFINITY, 1.0),
        ] {
            let err = backend.move_to(x, y).expect_err("not a screen coordinate");
            assert!(
                matches!(err, PlatformError::InvalidArgument { .. }),
                "{err:?}"
            );
        }
    }

    #[test]
    fn keyboard_input_sets_the_key_up_flag_only_on_release() {
        let down = keyboard_input(VK_SPACE, false);
        let up = keyboard_input(VK_SPACE, true);
        // SAFETY: both values were built with `r#type == INPUT_KEYBOARD`, so the
        // `ki` arm of each union is the one that was initialised.
        let down_ki = unsafe { down.Anonymous.ki };
        let up_ki = unsafe { up.Anonymous.ki };
        assert_eq!(down_ki.wVk, VK_SPACE);
        assert_eq!(down_ki.dwFlags, KEYBD_EVENT_FLAGS(0));
        assert_eq!(up_ki.dwFlags, KEYEVENTF_KEYUP);
    }

    #[test]
    fn virtual_keys_cover_the_named_keys_the_host_uses() {
        assert_eq!(virtual_key(Key::Space), VK_SPACE);
        assert_eq!(virtual_key(Key::Enter), VK_RETURN);
        assert_eq!(virtual_key(Key::Escape), VK_ESCAPE);
        assert_eq!(virtual_key(Key::Tab), VK_TAB);
        assert_eq!(virtual_key(Key::Backspace), VK_BACK);
        assert_eq!(virtual_key(Key::Shift), VK_SHIFT);
        assert_eq!(virtual_key(Key::Control), VK_CONTROL);
        assert_eq!(virtual_key(Key::Alt), VK_MENU);
        assert_eq!(virtual_key(Key::Left), VK_LEFT);
        assert_eq!(virtual_key(Key::Down), VK_DOWN);
        assert_eq!(virtual_key(Key::F(1)), VK_F1);
        assert_eq!(virtual_key(Key::F(12)).0, VK_F1.0 + 11);
        assert_eq!(virtual_key(Key::Other(0x57)).0, 0x57);
    }

    #[test]
    fn an_unmappable_key_reports_unsupported_instead_of_pressing_nothing() {
        let backend = WindowsInputBackend::new();
        assert!(matches!(
            backend.key_press(Key::F(99)),
            Err(PlatformError::Unsupported { .. })
        ));
    }

    #[test]
    fn the_event_queue_respects_its_capacity() {
        if let Ok(mut queue) = EVENTS.lock() {
            queue.clear();
        }
        for _ in 0..(EVENT_CAPACITY + 8) {
            push_event(InputEvent::KeyDown(Key::Other(0x41)));
        }
        let drained = {
            let mut out = Vec::new();
            WindowsInputBackend::new()
                .poll_events(&mut out)
                .expect("drain");
            out
        };
        assert_eq!(drained.len(), EVENT_CAPACITY);
        assert!(drained
            .iter()
            .all(|event| *event == InputEvent::KeyDown(Key::Other(0x41))));
    }

    /// Serialises the tests that share the process-global hook slot, so they do not
    /// observe each other's installs.
    static HOOK_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn uninstalling_twice_is_idempotent_and_never_underflows() {
        let _serialised = HOOK_TEST_LOCK.lock().ok();
        uninstall_hooks();
        uninstall_hooks();
        let slot = HOOKS.lock().map(|slot| slot.is_none());
        assert!(slot.is_ok_and(|empty| empty));
    }

    #[test]
    fn a_guard_without_handles_unhooks_nothing() {
        // A zero handle is what `install` leaves behind when the second hook fails,
        // so `Drop` must treat it as "nothing to unhook" rather than passing a
        // `NULL` `HHOOK` to `UnhookWindowsHookEx`. The recorder stands in for
        // user32, so this asserts the call list instead of trusting a no-crash.
        let mut guard = HookGuard {
            keyboard: 0,
            mouse: 0,
        };
        let mut unhooked = Vec::new();

        guard.unhook(|handle| unhooked.push(handle));

        assert!(unhooked.is_empty(), "unhooked {unhooked:?}");
    }

    #[test]
    fn each_live_handle_is_unhooked_exactly_once() {
        let mut guard = HookGuard {
            keyboard: 0x11,
            mouse: 0x22,
        };
        let mut unhooked = Vec::new();

        guard.unhook(|handle| unhooked.push(handle));
        assert_eq!(unhooked, vec![0x11isize, 0x22]);

        // The fields are cleared as they are unhooked, so a second pass - `Drop`
        // after an explicit call, or any double teardown - touches no stale handle.
        guard.unhook(|handle| unhooked.push(handle));
        assert_eq!(
            unhooked,
            vec![0x11isize, 0x22],
            "a second pass must unhook nothing"
        );
    }

    #[test]
    fn concurrent_installs_never_leave_more_than_one_pair() {
        let _serialised = HOOK_TEST_LOCK.lock().ok();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    install_hooks().is_ok()
                })
            })
            .collect();

        let installed: Vec<bool> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap_or(false))
            .collect();

        // The guard is a single slot, so at most one pair can be owned however many
        // callers raced; the losers either saw it filled or failed outright.
        let count = match HOOKS.lock() {
            Ok(slot) => usize::from(slot.is_some()),
            Err(poisoned) => usize::from(poisoned.into_inner().is_some()),
        };
        assert!(count <= 1, "{count} hook pairs owned");
        if installed.iter().all(|ok| *ok) {
            assert_eq!(count, 1, "a successful install must be owned");
        }

        uninstall_hooks();
        let slot = HOOKS.lock().map(|slot| slot.is_none());
        assert!(slot.is_ok_and(|empty| empty));
    }

    #[test]
    fn the_mouse_sampler_always_answers_on_windows() {
        // `mouse_state` is `probe_mouse_state().unwrap_or_default()`, and a button
        // held down is not something a test can stage: with the desktop idle,
        // `default()` equals the truth, so asserting on the value cannot tell a
        // real read from a hardcoded one. What is observable is that the probe
        // answers at all, which is what this pins.
        assert!(
            WindowsInputBackend::new().probe_mouse_state().is_some(),
            "GetAsyncKeyState always has a definite answer on Windows"
        );
    }

    #[test]
    fn an_unqueryable_key_is_unknown_rather_than_up() {
        // A key with no virtual-key code cannot be queried at all. Reporting it as
        // "up" would tell the loops a key is released when nobody asked the OS.
        let backend = WindowsInputBackend::new();
        for key in [Key::F(0), Key::F(25), Key::Char('\u{100}')] {
            assert_eq!(backend.probe_key_state(key), None, "{key:?} has no code");
            assert!(
                !backend.key_state(key),
                "the fallback is `false`, not a panic"
            );
        }
    }
}
