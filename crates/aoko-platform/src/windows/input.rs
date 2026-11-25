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
//! `INPUT_KEYBOARD` event.

use std::sync::Mutex;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, VkKeyScanW, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE,
    KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DOWN,
    VK_ESCAPE, VK_F1, VK_LBUTTON, VK_LEFT, VK_MENU, VK_RBUTTON, VK_RETURN, VK_RIGHT, VK_SHIFT,
    VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
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
}

impl Drop for HookGuard {
    fn drop(&mut self) {
        // SAFETY: whichever handle is non-zero came from a successful
        // `SetWindowsHookExW` in `install`, is still installed, and is cleared here,
        // so each hook is unhooked exactly once.
        if self.keyboard != 0 {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(self.keyboard as *mut core::ffi::c_void));
            }
            self.keyboard = 0;
        }
        if self.mouse != 0 {
            // SAFETY: as above, for the mouse handle: it came from a successful
            // `SetWindowsHookExW`, is still installed, and is zeroed here.
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(self.mouse as *mut core::ffi::c_void));
            }
            self.mouse = 0;
        }
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
        send(&[mouse_input(down, 0, 0), mouse_input(up, 0, 0)])
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

    fn key_press(&self, key: Key) -> Result<(), PlatformError> {
        let vk = virtual_key(key);
        if vk.0 == 0 {
            return Err(PlatformError::unsupported(format!(
                "{key:?} has no virtual-key code"
            )));
        }
        send(&[keyboard_input(vk, false), keyboard_input(vk, true)])
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
    fn a_guard_without_handles_drops_without_calling_user32() {
        // A zero handle is what `install` leaves behind when the second hook fails,
        // so `Drop` must treat it as "nothing to unhook" rather than passing a
        // `NULL` `HHOOK` to `UnhookWindowsHookEx`.
        drop(HookGuard {
            keyboard: 0,
            mouse: 0,
        });
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
    fn mouse_state_is_readable() {
        let backend = WindowsInputBackend::new();
        let state = backend.mouse_state();
        // Values are whatever the desktop reports; the contract is that the read
        // is cheap and never fails.
        let _ = state.left_down || state.right_down;
    }
}
