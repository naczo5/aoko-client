//! The two modal surfaces, and the state machines behind them.
//!
//! **Not mounted in Phase 2** (section 12 decision 7): each dialog's state machine
//! and copy are built and unit-tested, but neither type implements `Render` and no
//! route opens one, so nothing appears on screen yet. This module is the state, not
//! a working surface.
//!
//! Both were product surfaces in the managed app — the colour picker's HSV
//! coupling and the picker's target/version selection each had real state — so
//! each is a plain struct here and the widget is a thin painter over it. Neither
//! opens a second window: `MIGRATION_ARCHITECTURE.md` section 12 decision 4 puts
//! every surface in the one window, and GPUI has no modal/owner relationship to
//! borrow anyway.

pub mod color_picker;
pub mod window_picker;
