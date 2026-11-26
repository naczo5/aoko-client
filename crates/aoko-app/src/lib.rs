//! The GPUI host: the shell, the design system, the views, the dialogs and the
//! screenshot harness.
//!
//! Layering inside the crate follows `MIGRATION_ARCHITECTURE.md` section 2:
//!
//! ```text
//! theme/ + assets/   tokens and font installation
//!     |
//! components/        the hand-built widgets (bare `gpui` only)
//!     |
//! views/ + dialogs/  tabs, cards and the two modal surfaces
//!     |
//! shell              the root view, tab rail and client-drawn title bar
//! ```
//!
//! Everything above `components` is testable without a GPU: the tab decisions,
//! the command-search state machine and the two dialog state machines are plain
//! structs with no GPUI type in their signatures, which is what the unit tests in
//! this crate exercise. GPUI has no widget test harness at the pin, so the pure
//! layer is the real test surface.
//!
//! This file is the crate's single module tree. `theme/` and `assets/` are the
//! component builder's; `views/` and `dialogs/` are the shell builder's. Declaring
//! both here is what keeps the two halves in one library target instead of two
//! binaries that cannot see each other.

// `assets/` and `theme/` are the component builder's and land here as they become
// compilable; `views/` is independent of both.
// pub mod assets;

pub mod dialogs;
pub mod views;
