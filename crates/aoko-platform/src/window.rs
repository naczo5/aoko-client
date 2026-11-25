//! Window enumeration, foreground detection and client-area geometry.

use crate::error::PlatformError;

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
    /// Outer bounds, including decorations.
    pub bounds: Rect,
}

pub trait WindowInspector: Send + Sync {
    fn enumerate(&self) -> Result<Vec<WindowInfo>, PlatformError>;
    fn foreground(&self) -> Result<Option<WindowInfo>, PlatformError>;
    fn client_bounds(&self, handle: WindowHandle) -> Result<Rect, PlatformError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_size_rect_is_empty() {
        assert!(!Rect::default().is_non_empty());
        assert!(Rect { x: 0, y: 0, width: 1, height: 1 }.is_non_empty());
    }

    #[test]
    fn negative_size_rect_is_empty() {
        let rect = Rect { x: 0, y: 0, width: -5, height: 10 };
        assert!(!rect.is_non_empty());
    }
}
