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
//! Decision 4 is the managed two-mode window, restored here: the app opens at the
//! loader's 360×310 (`MainWindow.xaml:8-10`, `ResizeMode="NoResize"`) and grows to
//! 1020×760 when the bridge connects (`MainWindow.xaml.cs:469-493`). Only the
//! *floor* stays creation-time, because GPUI has no runtime min-size setter: the
//! window is created with the loader's size as its minimum, so a control-mode window
//! can be dragged smaller than the managed 560×460 floor. That is the one deviation,
//! and it is cheaper than a loader the size of the GUI.

use gpui::{point, px, size, Bounds, Pixels, Size, WindowBounds, WindowDecorations, WindowOptions};

use crate::theme::metrics;

/// The loader's window, 360×310 (`MainWindow.xaml:8-10`).
///
/// The splash that carries the inject button: fixed size in the managed host
/// (`ResizeMode="NoResize"`), and the size the app opens at.
#[must_use]
pub fn loader_size() -> Size<Pixels> {
    size(
        px(metrics::window::LOADER_WIDTH),
        px(metrics::window::LOADER_HEIGHT),
    )
}

/// The window's starting size, 1020×760 (`MainWindow.xaml.cs:474-475`).
#[must_use]
pub fn content_size() -> Size<Pixels> {
    size(px(metrics::window::WIDTH), px(metrics::window::HEIGHT))
}

/// The floor, 560×460 (`MainWindow.xaml.cs:481-482`).
///
/// The control-mode floor. It is not what the window is created with — see
/// [`options`] — because the loader's own window is smaller than this and the
/// creation-time minimum would refuse to draw it.
#[must_use]
pub fn min_size() -> Size<Pixels> {
    size(
        px(metrics::window::MIN_WIDTH),
        px(metrics::window::MIN_HEIGHT),
    )
}

/// Options for the single application window.
///
/// Opens at the loader's geometry and uses it as the minimum, because `window_min_size`
/// is read once at creation and GPUI exposes no setter afterwards: a window created
/// with the 560×460 control floor could never shrink to the 360×310 splash. The shell
/// grows the window when it enters control mode ([`crate::shell::Shell`]), which is the
/// managed `EnterControlMode` and the only runtime geometry change in the app.
#[must_use]
pub fn options() -> WindowOptions {
    let bounds = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: loader_size(),
    };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(loader_size()),
        window_decorations: Some(WindowDecorations::Client),
        titlebar: None,
        // The shell draws the whole client area, including its own title bar, so there
        // is nothing for a system title bar to contribute.
        focus: true,
        show: true,
        ..Default::default()
    }
}

/// Options at control geometry, for a caller that never shows the loader.
///
/// The capture harness is the one: every screen it renders is a control tab, it drives
/// no bridge, and `Shell`'s latch never fires there, so a window opened with
/// [`options`] would photograph a 360×310 splash-sized frame full of a 1020×760 layout.
/// Same chrome, same minimum — only the opening bounds differ.
#[must_use]
pub fn control_options() -> WindowOptions {
    let bounds = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: content_size(),
    };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(min_size()),
        ..options()
    }
}
