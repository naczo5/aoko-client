//! Hand-built widgets.
//!
//! UI decision 1 is bare `gpui` only: there is no zed `ui` or `text` crate, and
//! the pinned revision has **no slider widget and no text-input widget** (the only
//! `TextInput` in the tree is the platform IME event type). So the two hardest
//! controls are from scratch here regardless of the dependency question, and the
//! rest are cheaper built than borrowed.
//!
//! Rules for this module, so the layer stays a design system rather than a second
//! theme system:
//!
//! - **No colour literals.** Every colour comes from a [`Theme`](crate::theme::Theme)
//!   token or from a `theme::fixed` token. The C# shell carries 121 hex literals in
//!   `MainWindow.xaml` and that dual source is the defect arbitration 1 exists to
//!   prevent.
//! - **No copy.** Call sites pass labels in; component-internal glyphs are limited
//!   to the ones the C# draws as glyphs (`✓`, `▾`, `▸`, `esc`).
//! - **Tokens for geometry.** Sizes come from [`crate::theme::metrics`], so a
//!   dimension has one home and a screenshot diff has one place to change.
//! - **Composition over configuration.** Containers return `Div` so callers add
//!   children; leaves return `AnyElement`.
//!
//! Behaviour is deliberately minimal in this layer: enough to be driven and to look
//! right in a capture, not enough to replace the shell's state machines. Each
//! widget's documentation says what it does **not** do yet.

pub mod button;
pub mod card;
pub mod checkbox;
pub mod chip;
pub mod combo;
pub mod expander;
pub mod field;
pub mod highlight;
pub mod popup;
pub mod progress;
pub mod scroll;
pub mod slider;
pub mod switch;
pub mod text_input;
pub mod tooltip;

pub use button::{button, ButtonVariant};
pub use card::card;
pub use checkbox::checkbox;
pub use chip::{chip, ChipTone};
pub use combo::combo;
pub use expander::expander;
pub use field::{field, FieldVariant};
pub use highlight::{highlight_text, sanitize_ranges};
pub use popup::popup;
pub use progress::{progress, progress_indeterminate};
pub use scroll::scroll;
pub use slider::{slider, DragCoalescer, SliderGeometry};
pub use switch::switch;
pub use text_input::{TextEdit, TextInput};
pub use tooltip::{tooltip, Tooltip};

/// Component-internal glyphs and labels.
///
/// The C# draws exactly these as text: `✓` for the small checkbox (`App.xaml:131`),
/// the combo chevron and expander markers, and the overlay's `esc` cap
/// (`CommandSearchOverlay.xaml:240`). They live here rather than at call sites so a
/// card never has to spell one, and they are the only copy this layer owns.
pub mod glyph {
    /// Checked box (`App.xaml:131`).
    pub const CHECK: &str = "✓";
    /// Collapsed / closed dropdown marker.
    pub const CHEVRON_DOWN: &str = "▾";
    /// Collapsed expander marker.
    pub const CHEVRON_RIGHT: &str = "▸";
    /// Expanded expander marker.
    pub const CHEVRON_EXPANDED: &str = "▾";
    /// The overlay's close cap.
    pub const ESC: &str = "esc";
}
