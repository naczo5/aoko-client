//! The two modal surfaces, the state machines behind them, and their views.
//!
//! **The views are built and tested, but nothing mounts them** (section 12 decision 7):
//! [`color_picker_view::ColorPickerView`] and [`window_picker_view::WindowPickerView`] each
//! implement `Render` and are exercised in a window by their own tests, but no route in the
//! running app opens either one — the managed call sites
//! (`MainWindow.xaml.cs:1075-1085`, `:2030-2036` for the colour picker and `:1477-1489`
//! for the target picker) have no Rust equivalent yet. Mounting one means an overlay child
//! of the shell, which is the shell's wiring.
//!
//! Each view's header says what is and is not interactive: the colour picker's square and
//! hue bar paint but do not take a press, because [`crate::picker`]'s callbacks carry no
//! `App`; the target picker's message box is drawn in place rather than opening a second
//! window.
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
