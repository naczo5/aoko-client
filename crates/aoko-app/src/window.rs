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
//! 1020×760 when the bridge connects (`MainWindow.xaml.cs:469-493`). Only the *floor*
//! stays creation-time, because GPUI has no runtime min-size setter: the window is
//! created with the loader's size as its minimum, so a control-mode window can be
//! dragged smaller than the managed 560×460 floor. That is the one deviation, and it is
//! cheaper than a loader the size of the GUI.
//!
//! # Why the floor cannot be raised, precisely
//!
//! `EnterControlMode` swapped two numbers, not one: the window size and the resizable
//! window's minimum, 360×310 → 560×460 (`MainWindow.xaml.cs:474-482`). The port owns the
//! first half only. `window_min_size` is a `WindowOptions` field
//! (`crates/gpui/src/platform.rs:2769`), copied into `WindowParams` (`:2844`) and read
//! exactly once: `Window::new` destructures the options (`crates/gpui/src/window.rs:1643`,
//! `:1648`) and passes the value to `Platform::open_window` (`:1694-1721`). At rev 5526444
//! there is no runtime counterpart — no `Window::set_min_size`, nothing that rebuilds
//! `WindowOptions` after creation, and `Window::resize` (`:3295-3298`) takes a size and
//! forwards it to the platform with no minimum in it. The platform keeps the creation-time
//! number: on Windows it is the `WM_GETMINMAXINFO` `ptMinTrackSize` floor
//! (`crates/gpui_windows/src/events.rs:208-222`), from the `min_size` the window state
//! stored when the HWND was created (`crates/gpui_windows/src/window.rs:278`, `:534`).
//!
//! So [`crate::shell::Shell`]'s latch can grow the window to 1020×760 but cannot raise
//! what the OS lets a drag shrink it to: after the latch, the user can drag back down to
//! 360×310 and the rail and cards clip. The alternatives were a per-frame
//! `window.bounds()` check that resizes back (a drag is a stream of mouse-move resizes, so
//! that fights the hand and schedules a frame every frame) and opening at 560×460 or more
//! (which paints the loader as a splash floating in a control-sized window, the very
//! thing decision 4 rejects). Both are worse than a floor that is only too low.

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
/// The control-mode floor. [`control_options`] hands it to the platform as the window
/// minimum, and for the capture harness it stays in force for the whole run because that
/// window never shows the loader. It is deliberately *not* what [`options`] creates the
/// live window with: the loader's own window is smaller than this, and a creation-time
/// minimum would refuse to draw it. Since the pin cannot raise a window's minimum after
/// creation (see the module note), the live window's floor stays the loader's even in
/// control mode, where it can be dragged below this size and the rail and cards clip.
#[must_use]
pub fn min_size() -> Size<Pixels> {
    size(
        px(metrics::window::MIN_WIDTH),
        px(metrics::window::MIN_HEIGHT),
    )
}

/// Options for the single application window.
///
/// Opens at the loader's geometry and uses it as the minimum: `WindowOptions::window_min_size`
/// is read once, when the platform creates the window, and the pin exposes no setter
/// afterwards — the module note has the call sites. A window created with the 560×460
/// control floor could never shrink to the 360×310 splash, so the splash's own size is the
/// floor, and the price is that a control-mode window can be dragged below [`min_size`].
/// The shell grows the window when it enters control mode ([`crate::shell::Shell`]), which
/// is the managed `EnterControlMode` and the only runtime geometry change in the app.
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
