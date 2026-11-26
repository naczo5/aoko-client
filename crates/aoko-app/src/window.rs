//! Window options, in one place because two callers need them.
//!
//! The live app and the capture harness must open the *same* window: same geometry,
//! same decorations, same minimum size. If they diverge, a screenshot stops being
//! evidence about the app.
//!
//! Decision 5 is `WindowDecorations::Client`: the title bar is drawn by the shell, so
//! `titlebar` stays `None` (that option only hides the system title bar on macOS and
//! Windows) and `window_decorations` asks the compositor for client-side decorations.
//!
//! Decision 4 is one window at control geometry, so `MIN_*` is a creation-time value.
//! GPUI has no runtime min-size setter, which is exactly why the two-mode window was
//! dropped instead of emulated.

use gpui::{point, px, size, Bounds, Pixels, Size, WindowBounds, WindowDecorations, WindowOptions};

use crate::theme::metrics;

/// The window's starting size, 1020×760 (`MainWindow.xaml.cs:474-475`).
#[must_use]
pub fn content_size() -> Size<Pixels> {
    size(px(metrics::window::WIDTH), px(metrics::window::HEIGHT))
}

/// The floor, 560×460 (`MainWindow.xaml.cs:481-482`).
#[must_use]
pub fn min_size() -> Size<Pixels> {
    size(
        px(metrics::window::MIN_WIDTH),
        px(metrics::window::MIN_HEIGHT),
    )
}

/// Options for the single application window.
#[must_use]
pub fn options() -> WindowOptions {
    let bounds = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: content_size(),
    };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(min_size()),
        window_decorations: Some(WindowDecorations::Client),
        titlebar: None,
        // The shell draws the whole client area, including its own title bar, so there
        // is nothing for a system title bar to contribute.
        focus: true,
        show: true,
        ..Default::default()
    }
}
