//! Window enumeration and geometry through `user32`.
//!
//! `WindowDetection.ListSelectableWindows` enumerates visible, titled top-level
//! windows and skips the host's own process, which is what [`WindowInspector`]
//! reproduces. Client bounds are `GetClientRect` mapped to the screen with
//! `ClientToScreen`, exactly like `GetMinecraftClientRectOnScreen`.

use windows::core::PWSTR;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetClientRect, GetCursorInfo, GetForegroundWindow, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsWindow, IsWindowVisible, CURSORINFO,
    CURSOR_SHOWING,
};

use crate::error::PlatformError;
use crate::window::{Rect, WindowHandle, WindowInfo, WindowInspector};

use super::handles::OwnedHandle;
use super::raw::ClientToScreen;

/// Titles are read into a stack buffer, matching the managed 256-character read.
const TITLE_CAPACITY: usize = 256;

pub struct WindowsWindowInspector;

impl WindowsWindowInspector {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn info(&self, window: HWND) -> Option<WindowInfo> {
        let title = window_title(window);
        if title.is_empty() {
            return None;
        }
        let pid = window_process_id(window).filter(|pid| *pid != 0)?;
        if pid == std::process::id() {
            return None;
        }
        Some(WindowInfo {
            handle: WindowHandle(window.0 as usize as u64),
            title,
            class_name: class_name(window),
            pid: Some(pid),
            process_name: process_image_name(pid),
            bounds: window_rect(window).unwrap_or_default(),
        })
    }
}

impl Default for WindowsWindowInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowInspector for WindowsWindowInspector {
    fn enumerate(&self) -> Result<Vec<WindowInfo>, PlatformError> {
        let mut context = EnumContext {
            windows: Vec::new(),
        };

        // SAFETY: `context` outlives the call, and the callback only mutates it
        // through the pointer handed to `EnumWindows`, on this same thread.
        let result = unsafe {
            EnumWindows(
                Some(enum_window_proc),
                LPARAM(std::ptr::from_mut(&mut context) as isize),
            )
        };

        result.map_err(|err| PlatformError::os("EnumWindows", err.to_string()))?;

        let mut windows = context.windows;
        crate::window::sort_windows(&mut windows);
        Ok(windows)
    }

    fn foreground(&self) -> Result<Option<WindowInfo>, PlatformError> {
        // SAFETY: `GetForegroundWindow` takes no arguments and returns a borrowed
        // window handle that is never owned by us.
        let window = unsafe { GetForegroundWindow() };
        if window.is_invalid() {
            return Ok(None);
        }
        Ok(self.info(window).or_else(|| {
            let pid = window_process_id(window);
            Some(WindowInfo {
                handle: WindowHandle(window.0 as usize as u64),
                title: window_title(window),
                class_name: class_name(window),
                pid,
                process_name: pid.and_then(process_image_name),
                bounds: window_rect(window).unwrap_or_default(),
            })
        }))
    }

    fn client_bounds(&self, handle: WindowHandle) -> Result<Rect, PlatformError> {
        let window = hwnd_from_handle(handle).ok_or_else(|| {
            PlatformError::not_found(format!("window {} is not a HWND", handle.0))
        })?;

        // SAFETY: `window` was validated by the caller and `rect` is a live local.
        let valid = unsafe { IsWindow(window) }.as_bool();
        if !valid {
            return Err(PlatformError::not_found(format!(
                "window {} no longer exists",
                handle.0
            )));
        }

        let mut client = RECT::default();
        // SAFETY: `client` is a live, correctly sized `RECT`.
        unsafe { GetClientRect(window, &mut client) }
            .map_err(|err| PlatformError::os("GetClientRect", err.to_string()))?;

        let mut top_left = POINT {
            x: client.left,
            y: client.top,
        };
        let mut bottom_right = POINT {
            x: client.right,
            y: client.bottom,
        };
        // SAFETY: both points are live locals with the client rect's coordinates,
        // which `ClientToScreen` maps in place.
        let mapped = unsafe {
            ClientToScreen(window, &mut top_left).as_bool()
                && ClientToScreen(window, &mut bottom_right).as_bool()
        };
        if !mapped {
            return Err(PlatformError::os(
                "ClientToScreen",
                windows::core::Error::from_win32().to_string(),
            ));
        }

        Ok(Rect {
            x: top_left.x,
            y: top_left.y,
            width: bottom_right.x - top_left.x,
            height: bottom_right.y - top_left.y,
        })
    }

    fn cursor_visible(&self) -> Option<bool> {
        let mut info = CURSORINFO {
            cbSize: u32::try_from(core::mem::size_of::<CURSORINFO>()).ok()?,
            ..Default::default()
        };
        // SAFETY: `info` is a live `CURSORINFO` with `cbSize` set, which is the
        // documented precondition of `GetCursorInfo`.
        if unsafe { GetCursorInfo(&mut info) }.is_err() {
            return None;
        }
        // The game hides the cursor with `SetCursor(NULL)` while the mouse is
        // captured, which leaves `hCursor` null even though `CURSOR_SHOWING` is set.
        Some(info.flags.0 & CURSOR_SHOWING.0 != 0 && !info.hCursor.is_invalid())
    }
}

struct EnumContext {
    windows: Vec<WindowInfo>,
}

/// # Safety
/// `lparam` must be a pointer produced from a live `&mut EnumContext` that
/// outlives this call, which is how [`WindowsWindowInspector::enumerate`] calls it.
unsafe extern "system" fn enum_window_proc(window: HWND, lparam: LPARAM) -> BOOL {
    let context = &mut *(lparam.0 as *mut EnumContext);
    let inspector = WindowsWindowInspector;

    let visible = IsWindowVisible(window).as_bool();
    if visible {
        if let Some(info) = inspector.info(window) {
            context.windows.push(info);
        }
    }
    BOOL(1)
}

#[must_use]
pub(crate) fn hwnd_from_handle(handle: WindowHandle) -> Option<HWND> {
    usize::try_from(handle.0)
        .ok()
        .map(|raw| HWND(raw as *mut core::ffi::c_void))
}

/// Window title, trimmed, in a stack buffer.
#[must_use]
pub fn window_title(window: HWND) -> String {
    let mut buffer = [0u16; TITLE_CAPACITY];
    // SAFETY: `buffer` is a live, correctly sized UTF-16 buffer; the call writes at
    // most its length in code units and returns the count written.
    let length = unsafe { GetWindowTextW(window, &mut buffer) };
    let length = usize::try_from(length).unwrap_or(0).min(TITLE_CAPACITY);
    String::from_utf16_lossy(&buffer[..length])
        .trim()
        .to_string()
}

fn class_name(window: HWND) -> Option<String> {
    let mut buffer = [0u16; TITLE_CAPACITY];
    // SAFETY: as above, with a buffer of the documented size.
    let length = unsafe { GetClassNameW(window, &mut buffer) };
    let length = usize::try_from(length).unwrap_or(0).min(TITLE_CAPACITY);
    if length == 0 {
        return None;
    }
    let name = String::from_utf16_lossy(&buffer[..length])
        .trim()
        .to_string();
    (!name.is_empty()).then_some(name)
}

/// Owning process id, or `None` when the window has no owner we can see.
#[must_use]
pub fn window_process_id(window: HWND) -> Option<u32> {
    let mut pid = 0u32;
    // SAFETY: `pid` is a live local; the call only writes to it.
    let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    (thread != 0 && pid != 0).then_some(pid)
}

fn window_rect(window: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live `RECT`.
    unsafe { GetWindowRect(window, &mut rect) }.ok()?;
    Some(Rect {
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    })
}

/// `WindowDetection.ResolveProcessName`: the executable basename, or `None` when
/// the process is protected.
#[must_use]
pub fn process_image_name(pid: u32) -> Option<String> {
    // SAFETY: the pid is caller-supplied data; `OpenProcess` either returns a owned
    // handle or an error, and the handle is closed by `OwnedHandle`.
    let process = OwnedHandle::from_result(unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
    })
    .ok()?;

    let mut buffer = [0u16; 1024];
    let mut size = u32::try_from(buffer.len()).ok()?;
    // SAFETY: `buffer` is live for `size` code units and `size` is the length we
    // hand over, which the call updates to the number written.
    let result = unsafe {
        QueryFullProcessImageNameW(
            process.get(),
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    };
    result.ok()?;

    let path = String::from_utf16_lossy(&buffer[..usize::try_from(size).ok()?]);
    let name = path.rsplit(['\\', '/']).next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_round_trips_through_hwnd() {
        let hwnd = hwnd_from_handle(WindowHandle(0x1234));
        assert!(hwnd.is_some());
        assert_eq!(hwnd.map(|window| window.0 as usize), Some(0x1234usize));
    }

    #[test]
    fn a_handle_too_large_for_a_pointer_is_rejected() {
        assert!(hwnd_from_handle(WindowHandle(u64::MAX)).is_none());
    }

    #[test]
    fn enumerating_windows_does_not_panic_without_a_desktop() {
        let inspector = WindowsWindowInspector::new();
        // CI may run without an interactive desktop; either an empty list or an
        // error is acceptable, a panic is not.
        match inspector.enumerate() {
            Ok(windows) => {
                for window in &windows {
                    assert!(!window.title.is_empty());
                    assert_ne!(window.pid, Some(std::process::id()));
                }
            }
            Err(err) => assert!(!err.user_facing().is_empty()),
        }
    }

    #[test]
    fn the_foreground_query_never_returns_an_empty_handle() {
        let inspector = WindowsWindowInspector::new();
        if let Ok(Some(window)) = inspector.foreground() {
            assert_ne!(window.handle.0, 0);
        }
    }

    #[test]
    fn client_bounds_of_an_invalid_handle_is_not_found() {
        let inspector = WindowsWindowInspector::new();
        let err = inspector
            .client_bounds(WindowHandle(0xdead_beef))
            .expect_err("no such window");
        assert!(matches!(err, PlatformError::NotFound { .. }));
    }
}
