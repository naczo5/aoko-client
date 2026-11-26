//! The two modal surfaces, and the state machines behind them.
//!
//! Both were product surfaces in the managed app — the colour picker's HSV
//! coupling and the picker's target/version selection each had real state — so
//! each is a plain struct here and the widget is a thin painter over it. Neither
//! opens a second window: `MIGRATION_ARCHITECTURE.md` section 12 decision 4 puts
//! every surface in the one window, and GPUI has no modal/owner relationship to
//! borrow anyway.

pub mod color_picker;
pub mod window_picker;
