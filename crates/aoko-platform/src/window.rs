//! Window enumeration, foreground detection and client-area geometry.

use crate::error::PlatformError;
use crate::process::compare_ignore_case;

/// Opaque native window identifier (HWND on Windows, X11 window id elsewhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowHandle(pub u64);

/// Axis-aligned rectangle in screen coordinates, physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    #[must_use]
    pub fn is_non_empty(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

/// One top-level window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub handle: WindowHandle,
    pub title: String,
    pub class_name: Option<String>,
    /// Owning process, when the backend can resolve it.
    pub pid: Option<u32>,
    /// Basename of the owning executable, e.g. `javaw.exe`.
    ///
    /// `WindowDetection.WindowTarget.ProcessName` in the managed host. Kept
    /// separate from [`WindowInfo::pid`] because the picker labels entries with
    /// it and the JVM test is a name test.
    pub process_name: Option<String>,
    /// Outer bounds, including decorations.
    pub bounds: Rect,
}

impl WindowInfo {
    /// `WindowTarget.IsJvm`: the window belongs to a `java`/`javaw` process.
    #[must_use]
    pub fn is_jvm(&self) -> bool {
        self.process_name
            .as_deref()
            .is_some_and(crate::process::is_java_process_name)
    }

    /// `WindowTarget.DisplayLabel`, including the `[JVM]` prefix.
    #[must_use]
    pub fn display_label(&self) -> String {
        let prefix = if self.is_jvm() { "[JVM] " } else { "" };
        let process = self.process_name.as_deref().unwrap_or("unknown");
        let pid = self
            .pid
            .map_or_else(|| "0".to_string(), |pid| pid.to_string());
        format!(
            "{prefix}{title} \u{2014} {process} (PID {pid})",
            title = self.title
        )
    }
}

/// `WindowDetection.SortWindowTargets`: JVM entries first, then alphabetical by
/// title. Stable, so equal titles keep enumeration order.
pub fn sort_windows(windows: &mut [WindowInfo]) {
    windows.sort_by(|left, right| {
        right
            .is_jvm()
            .cmp(&left.is_jvm())
            .then_with(|| compare_ignore_case(&left.title, &right.title))
    });
}

pub trait WindowInspector: Send + Sync {
    fn enumerate(&self) -> Result<Vec<WindowInfo>, PlatformError>;
    fn foreground(&self) -> Result<Option<WindowInfo>, PlatformError>;
    fn client_bounds(&self, handle: WindowHandle) -> Result<Rect, PlatformError>;

    /// Whether the OS is currently drawing the pointer.
    ///
    /// The managed host uses `GetCursorInfo` for this on the keybind hot path.
    /// `None` means "this backend cannot tell", which is not an error; callers
    /// must treat it as "assume visible".
    fn cursor_visible(&self) -> Option<bool> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(title: &str, process: &str, pid: u32) -> WindowInfo {
        WindowInfo {
            handle: WindowHandle(0),
            title: title.to_string(),
            class_name: None,
            pid: Some(pid),
            process_name: Some(process.to_string()),
            bounds: Rect::default(),
        }
    }

    #[test]
    fn zero_size_rect_is_empty() {
        assert!(!Rect::default().is_non_empty());
        assert!(Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1
        }
        .is_non_empty());
    }

    #[test]
    fn negative_size_rect_is_empty() {
        let rect = Rect {
            x: 0,
            y: 0,
            width: -5,
            height: 10,
        };
        assert!(!rect.is_non_empty());
    }

    // `WindowDetectionTests.SortWindowTargets_PlacesJvmEntriesFirst_ThenAlphabeticalByTitle`
    #[test]
    fn sort_windows_places_jvm_entries_first_then_alphabetical_by_title() {
        let mut windows = vec![
            window("Zulu App", "chrome", 100),
            window("Alpha Game", "javaw", 200),
            window("Beta Tool", "notepad", 300),
            window("Minecraft", "java", 400),
        ];

        sort_windows(&mut windows);

        assert!(windows[0].is_jvm());
        assert!(windows[1].is_jvm());
        assert!(!windows[2].is_jvm());
        assert!(!windows[3].is_jvm());
        assert_eq!(windows[0].title, "Alpha Game");
        assert_eq!(windows[1].title, "Minecraft");
        assert_eq!(windows[2].title, "Beta Tool");
        assert_eq!(windows[3].title, "Zulu App");
    }

    // `WindowDetectionTests.WindowTarget_DisplayLabel_MarksJvmTargets`
    #[test]
    fn window_info_display_label_marks_jvm_targets() {
        let jvm = window("Lunar Client", "javaw", 42);
        let other = window("Notepad", "notepad", 99);

        assert!(jvm.display_label().starts_with("[JVM]"));
        assert!(!other.display_label().contains("[JVM]"));
        assert!(jvm.display_label().contains("PID 42"));
    }

    #[test]
    fn display_label_uses_the_managed_separator_and_falls_back_to_unknown() {
        let mut info = window("Minecraft 1.8.9", "javaw", 7);
        assert_eq!(
            info.display_label(),
            "[JVM] Minecraft 1.8.9 \u{2014} javaw (PID 7)"
        );
        info.process_name = None;
        info.pid = None;
        assert_eq!(
            info.display_label(),
            "Minecraft 1.8.9 \u{2014} unknown (PID 0)"
        );
    }
}
