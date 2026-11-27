//! The two modal surfaces, the state machines behind them, and their views.
//!
//! **The colour picker is mounted; the target picker is not.** `Shell::open_color_picker`
//! (`shell.rs:993`) builds a [`color_picker_view::ColorPickerView`], `shell.rs:2954` renders it
//! as an overlay child of the shell's one window, and the swatch on a palette row or a
//! block-ESP row opens it (`shell.rs:1508`). [`window_picker_view::WindowPickerView`] still has
//! no route into the running app: managed opened it from `MainWindow.xaml.cs:1470-1486` when
//! more than one client was running, and the port lists those targets inside the loader panel
//! instead (`shell.rs`'s `render_loader`), which is why that view is built and tested but
//! unreachable.
//!
//! Each view's header says what is and is not interactive: the colour picker's square and
//! hue bar take a press and a drag through [`crate::picker`]'s window-and-app callbacks;
//! the target picker's message box is drawn in place rather than opening a second window.
//!
//! Both were product surfaces in the managed app — the colour picker's HSV coupling and the
//! picker's target/version selection each had real state — so each state machine is a plain
//! struct and the widget is a thin painter over it. Neither opens a second window:
//! `MIGRATION_ARCHITECTURE.md` section 12 decision 4 puts every surface in the one window,
//! and GPUI has no modal/owner relationship to borrow anyway.

pub mod color_picker;
pub mod color_picker_view;
pub mod window_picker;
pub mod window_picker_view;
