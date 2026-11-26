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
//! Everything above `components` is testable without a GPU: the tab decisions, the
//! command-search state machine and the two dialog state machines are plain structs
//! with no GPUI type in their signatures, which is what the unit tests in this crate
//! exercise. GPUI has no widget test harness at the pin, so the pure layer is the real
//! test surface and the capture harness is the visual evidence.
//!
//! This file is the crate's single module tree, shared by both builders. `theme/`,
//! `components/` and `assets/` are the component builder's; `views/` and `dialogs/` are
//! the shell builder's. Declaring both here keeps the two halves in one library target
//! instead of two binaries that cannot see each other. Whoever adds a module adds its
//! line here and never removes another's.
//!
//! # Performance: section 6, measured
//!
//! The UI clause is *no per-frame cloning or style rebuilding in `Render::render`;
//! static tokens precomputed; filtered lists memoized; `cx.notify()` scoped to entities
//! whose displayed fields changed.* Allocation counts are deterministic and frame times
//! are not, so `perf` installs a counting global allocator in test builds and the
//! numbers below are assertions, not timings.
//!
//! A steady-state `Shell::render` — the whole tree for one tab, on the second and every
//! later frame — allocates **1006** times with the Combat tab showing. That is a budget,
//! not a target: the count is the pin's element model, which allocates twice per bare
//! `div()` and four more times per `into_any_element()`, plus an `Rc` and a `Vec` growth
//! per `on_click`. A zero-allocation frame is not reachable through the builder API
//! without hand-writing `Element` impls. Per tab, measured: Combat 1006, Render 868,
//! Utility 798, Risky 594, Beta 2228, Accounts 223, Servers 178, Stats 151,
//! Settings 415 — the count tracks how many controls a tab draws.
//!
//! What this crate stopped allocating per frame, each verified by the count moving:
//!
//! - Element ids: every `format!("{card}-{bind}")` in `render` was a `String` per id per
//!   frame. [`reuse`] memoises them from their `&'static str` pieces; ids that end in a
//!   number are `ElementId::NamedInteger` and allocate nothing even on the first frame.
//! - The chrome: the chip text was built twice and the title once per frame, and
//!   `render` cloned the `Theme` for its `String` name. The chrome is cached against the
//!   values it shows and the render body borrows the theme.
//! - Derived labels: keybind, action-bind and slider-value text are pure functions of
//!   one number and are memoised by it ([`reuse::by_number`]); the configs status line
//!   and the selected config name by their text ([`reuse::text`]).
//! - Style rebuilding: the palette preview rows built a whole `Theme` per row per frame
//!   to read one colour; `Theme::accent_by_name` reads the colour.
//!
//! The 1006 still includes **one** allocation this crate owns: `ensure_form_fields`
//! clones the `Theme` at the top of the frame before it checks whether it has anything
//! to create, and that clone is the theme's `String` name. Moving it inside the two
//! creation branches is mechanical and would make the frame 1005; it is not done here
//! because the number could not be re-measured under the build freeze, and a budget that
//! guesses is worse than one that admits a residue.
//!
//! Filtered lists: the command-search rows are computed by `refresh_rows` when the query
//! or the facts change and `rows()` hands `render` a stored slice, so no filter or sort
//! runs in a frame. `cx.notify()` is conditional in `set_value`, `set_checked`,
//! `set_selected_index`, `set_selected_text`, `select_next_config` and `set_animations`,
//! which is what makes a slider held at the end of its travel stop repainting.
//!
//! What is **not** covered: a frame *paint* (layout, text shaping, hitboxes) is not
//! measured here, only `Render::render`, which is what the clause names.

pub mod args;
pub mod assets;
pub mod capture;
pub mod components;
pub mod dialogs;
pub mod overlay;
/// Allocation accounting. Test-only: the module installs the global allocator used to
/// count a frame's allocations, and nothing in the shipped binary needs it.
#[cfg(test)]
pub mod perf;
pub mod picker;
pub mod reuse;
pub mod shell;
pub mod theme;
pub mod views;
pub mod window;
