//! Window enumeration through EWMH on X11.
//!
//! `_NET_CLIENT_LIST` gives the managed top-level windows, `_NET_ACTIVE_WINDOW`
//! the foreground one, `_NET_WM_PID` the owner and `WM_CLASS` the class. When a
//! window manager does not publish `_NET_CLIENT_LIST` the tree walk fallback
//! still finds viewable, titled windows.
//!
//! There is no portable Wayland equivalent: GNOME and KDE expose nothing a plain
//! client can query, so a Wayland-only session reports `window_inspection: false`
//! instead of pretending. XWayland windows are reached through this same path.

use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, MapState, Window, WindowClass};

use crate::error::PlatformError;
use crate::window::{Rect, WindowHandle, WindowInfo, WindowInspector};

use super::x11::X11;

pub struct LinuxWindowInspector {
    x11: Option<X11>,
    /// Why the X11 path is unavailable, for whichever method is called first.
    unavailable: Option<PlatformError>,
}

impl LinuxWindowInspector {
    /// Connects once. A missing display is recorded, not raised: the inspector
    /// stays constructible so [`crate::PlatformServices`] always has a value and
    /// the capability report carries the diagnostic.
    #[must_use]
    pub fn new() -> Self {
        match X11::connect() {
            Ok(x11) => Self {
                x11: Some(x11),
                unavailable: None,
            },
            Err(err) => Self {
                x11: None,
                unavailable: Some(err),
            },
        }
    }

    #[must_use]
    pub fn is_available(&self) -> bool {
        self.x11.is_some()
    }

    fn backend(&self) -> Result<&X11, PlatformError> {
        // The stored error is already the right one - `Unsupported` for a missing
        // display, `Os` for a failed atom intern - so it is returned unchanged
        // rather than relabelled, which would point a UI at the wrong remedy.
        self.x11.as_ref().ok_or_else(|| {
            self.unavailable
                .clone()
                .unwrap_or_else(|| PlatformError::unsupported("no X11 display"))
        })
    }

    fn info(&self, x11: &X11, window: Window) -> Option<WindowInfo> {
        // `WindowDetection` skipped `!IsWindowVisible`, which on X11 is the map
        // state; without this a minimised or withdrawn client still lists.
        let attributes = x11.conn.get_window_attributes(window).ok()?.reply().ok()?;
        if attributes.map_state != MapState::VIEWABLE {
            return None;
        }

        let title = x11
            .property_text(window, x11.atoms.wm_name, Some(x11.atoms.string))
            .unwrap_or_default();
        let title = title.trim().to_string();
        if title.is_empty() {
            return None;
        }

        let class_name = class_name(x11, window);
        let pid = x11
            .property_u32(window, x11.atoms.wm_pid)
            .and_then(|values| values.first().copied())
            .filter(|pid| *pid > 0);
        let bounds = x11.geometry(window)?;

        Some(WindowInfo {
            handle: WindowHandle(u64::from(window)),
            title,
            class_name,
            pid,
            // `WindowDetection.ResolveProcessName`: the picker labels entries with
            // the executable name and the `[JVM]` marker is a name test.
            process_name: pid.and_then(super::process::process_name),
            bounds,
        })
    }

    fn managed_windows(&self, x11: &X11) -> Vec<Window> {
        let managed = x11
            .property_u32(x11.root, x11.atoms.client_list)
            .unwrap_or_default();
        if !managed.is_empty() {
            return managed.into_iter().collect();
        }

        // Fallback for window managers without EWMH: walk the root's children.
        let Ok(cookie) = x11.conn.query_tree(x11.root) else {
            return Vec::new();
        };
        let Ok(reply) = cookie.reply() else {
            return Vec::new();
        };
        reply
            .children
            .into_iter()
            .filter(|child| {
                x11.conn
                    .get_window_attributes(*child)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .is_some_and(|attributes| {
                        attributes.map_state == MapState::VIEWABLE
                            && attributes.class == WindowClass::INPUT_OUTPUT
                    })
            })
            .collect()
    }
}

impl Default for LinuxWindowInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowInspector for LinuxWindowInspector {
    fn enumerate(&self) -> Result<Vec<WindowInfo>, PlatformError> {
        let x11 = self.backend()?;
        let mut windows: Vec<WindowInfo> = self
            .managed_windows(x11)
            .into_iter()
            .filter_map(|window| self.info(x11, window))
            .filter(|info| info.bounds.is_non_empty())
            .collect();
        crate::window::sort_windows(&mut windows);
        Ok(windows)
    }

    fn foreground(&self) -> Result<Option<WindowInfo>, PlatformError> {
        let x11 = self.backend()?;
        let active = x11
            .property_u32(x11.root, x11.atoms.active_window)
            .and_then(|values| values.first().copied())
            .filter(|window| *window != 0);
        let Some(active) = active else {
            return Ok(None);
        };

        // A foreground window may legitimately have no `_NET_WM_NAME` yet (a
        // splash or a client that lags), so report it with an empty title rather
        // than claiming nothing is focused.
        if let Some(info) = self.info(x11, active) {
            return Ok(Some(info));
        }
        let bounds = x11.geometry(active).unwrap_or_default();
        let pid = x11
            .property_u32(active, x11.atoms.wm_pid)
            .and_then(|values| values.first().copied())
            .filter(|pid| *pid > 0);
        Ok(Some(WindowInfo {
            handle: WindowHandle(u64::from(active)),
            title: String::new(),
            class_name: class_name(x11, active),
            pid,
            process_name: pid.and_then(super::process::process_name),
            bounds,
        }))
    }

    fn client_bounds(&self, handle: WindowHandle) -> Result<Rect, PlatformError> {
        let x11 = self.backend()?;
        let window = X11::window_from_handle(handle).ok_or_else(|| {
            PlatformError::not_found(format!("window {} is not a 32-bit X11 id", handle.0))
        })?;
        x11.geometry(window)
            .ok_or_else(|| PlatformError::not_found(format!("window {} disappeared", handle.0)))
    }

    /// Linux cannot answer this, and says so instead of guessing.
    ///
    /// The managed host asks `GetCursorInfo` for `CURSOR_SHOWING` plus a non-null
    /// `hCursor`. X11 has no equivalent request: `XQueryPointer` returns
    /// position, child and `same_screen`, never visibility, and while `XFixes`
    /// can hide and show a cursor it exposes no getter for that state. The way
    /// the game actually hides the pointer - GLFW's transparent cursor or SDL's
    /// relative mode - is a client-side `XDefineCursor`, which only the pixels of
    /// `XFixesGetCursorImage` would reveal: inference, not a state query, and
    /// `XFixes` is not even linked here.
    ///
    /// Wayland exposes nothing about the pointer to clients at all, so the
    /// answer is the same on GLFW-native and XWayland sessions.
    ///
    /// This override is explicit rather than an inherited default so the limit
    /// is greppable here, is named in [`crate::CapabilityReport`]'s notes, and is
    /// pinned by `cursor_visibility_is_reported_as_unobservable`.
    /// Callers must not read `None` as "the cursor is hidden".
    fn cursor_visible(&self) -> Option<bool> {
        None
    }
}

/// `WM_CLASS` carries two NUL-terminated strings: instance then class. The
/// picker shows the class, which is what WindowDetection compares against.
fn class_name(x11: &X11, window: Window) -> Option<String> {
    let reply = x11
        .conn
        .get_property(
            false,
            window,
            x11.atoms.wm_class,
            AtomEnum::STRING,
            0,
            u32::MAX,
        )
        .ok()?
        .reply()
        .ok()?;
    let mut parts = reply.value.split(|byte| *byte == 0);
    let _instance = parts.next();
    let class = parts.next()?;
    let class = String::from_utf8_lossy(class).trim().to_string();
    (!class.is_empty()).then_some(class)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unavailable_display_reports_unsupported_not_a_panic() {
        // Force the unavailable branch without touching the real session.
        let inspector = LinuxWindowInspector {
            x11: None,
            unavailable: Some(PlatformError::unsupported("test: no display")),
        };
        assert!(!inspector.is_available());

        let err = inspector.enumerate().expect_err("no display");
        assert!(matches!(err, PlatformError::Unsupported { .. }));
        assert!(inspector.foreground().is_err());
        assert!(inspector.client_bounds(WindowHandle(1)).is_err());
    }

    #[test]
    fn the_unavailable_message_is_preserved_for_the_banner() {
        let inspector = LinuxWindowInspector {
            x11: None,
            unavailable: Some(PlatformError::permission_denied("DISPLAY not authorized")),
        };
        let err = inspector.enumerate().expect_err("no display");
        assert!(err.user_facing().contains("DISPLAY not authorized"));
    }

    #[test]
    fn the_constructor_stores_a_diagnostic_exactly_when_it_has_no_backend() {
        // Holds with and without DISPLAY, which is the point: the previous version
        // asserted only on the headless path, so a capable session ran zero
        // assertions here. `backend()` trusts exactly this invariant - it returns
        // the stored error only when there is no connection - and `enumerate`
        // reaches the OS only through it.
        let inspector = LinuxWindowInspector::new();

        assert_eq!(
            inspector.is_available(),
            inspector.unavailable.is_none(),
            "a usable inspector must not carry a stored error, and vice versa"
        );

        match (inspector.is_available(), inspector.enumerate()) {
            (false, result) => assert!(result.is_err(), "no backend must not look like no windows"),
            (true, result) => assert!(result.is_ok(), "a live display must enumerate: {result:?}"),
        }
    }

    // `WindowInspector::cursor_visible` is a platform limit on Linux, not a gap:
    // the refusal is explicit, identical with and without a display, and never
    // claims the cursor is hidden.
    #[test]
    fn cursor_visibility_is_reported_as_unobservable() {
        let without_display = LinuxWindowInspector {
            x11: None,
            unavailable: Some(PlatformError::unsupported("test: no display")),
        };
        assert_eq!(without_display.cursor_visible(), None);

        let live = LinuxWindowInspector::new();
        assert_eq!(live.cursor_visible(), None);

        // `None` must not be mistaken for the managed `false`, which would let a
        // `!cursor_visible` gate act.
        assert_ne!(live.cursor_visible(), Some(false));
        assert_ne!(live.cursor_visible(), Some(true));
    }
}
