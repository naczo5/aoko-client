//! Measured layout constants and the layout maths that belongs to no widget.
//!
//! Every number here is quoted from the C# source, with the line that names it,
//! so a reviewer can check a dimension instead of trusting it. Colour tokens live
//! in [`super`]; this module is sizes, radii and text scale only.

/// Body text and the query field are the two sizes the C# sets explicitly.
pub mod text {
    /// `TabChipText` (`TabStyles.xaml:216`).
    pub const CHIP: f32 = 11.0;
    /// `TabCaption`, `TabStatusText` (`TabStyles.xaml:239`, `:246`).
    pub const CAPTION: f32 = 11.0;
    /// `TabSectionTitle` (`TabStyles.xaml:233`).
    pub const SECTION_TITLE: f32 = 12.0;
    /// `TabPrimaryButton` (`TabStyles.xaml:32`).
    pub const BUTTON: f32 = 12.0;
    /// Secondary/Danger buttons (`TabStyles.xaml:48`, `:94`).
    pub const BUTTON_SMALL: f32 = 11.0;
    /// `QueryBox` (`CommandSearchOverlay.xaml:216`).
    pub const QUERY: f32 = 18.0;
    /// The overlay's `esc` chip (`CommandSearchOverlay.xaml:240`).
    pub const ESC: f32 = 10.0;
}

/// Corner radii, in the order the styles declare them.
pub mod radius {
    /// `ControlCardStyle` (`MainWindow.xaml:53`).
    pub const CARD: f32 = 6.0;
    /// `DarkButton` (`App.xaml:337`).
    pub const BUTTON: f32 = 8.0;
    /// `TabPrimary/Secondary/DangerButton` shells (`TabStyles.xaml:43`, `:61`, `:125`).
    pub const BUTTON_TAB: f32 = 6.0;
    /// `TabChip` (`TabStyles.xaml:200`).
    pub const CHIP: f32 = 4.0;
    /// `TabChipTint` (`TabStyles.xaml:208`).
    pub const CHIP_TINT: f32 = 3.0;
    /// Control-center tab item (`MainWindow.xaml:82-86`).
    pub const TAB_ITEM: f32 = 10.0;
    /// Overlay card (`CommandSearchOverlay.xaml:162`).
    pub const OVERLAY_CARD: f32 = 12.0;
    /// Overlay result row (`CommandSearchOverlay.xaml:269`).
    pub const OVERLAY_ROW: f32 = 7.0;
    /// The slider track and its end dots (`App.xaml:232-234`).
    pub const SLIDER_TRACK: f32 = 3.0;
    /// The small checkbox (`App.xaml:127-130`).
    pub const CHECKBOX: f32 = 4.0;
}

/// The 1 px hairline.
///
/// §5.11 of the inventory: GPUI rounds at paint time, so a sub-pixel border can
/// vanish at 100 % and thicken at 150 %. Every 1 px border uses this token and
/// nothing writes `0.5`.
pub const HAIRLINE: f32 = 1.0;

/// Control-center chrome: rail 176 px + 10 px gap + content (`MainWindow.xaml:181-183`).
pub mod rail {
    pub const WIDTH: f32 = 176.0;
    pub const GAP: f32 = 10.0;
    /// Tab item width and minimum height (`MainWindow.xaml:82-86`).
    pub const ITEM_WIDTH: f32 = 150.0;
    pub const ITEM_MIN_HEIGHT: f32 = 50.0;
}

/// Window geometry.
///
/// Decision 4: one window at control geometry, so these are creation-time values
/// and the loader is an in-view state. `MIN_*` comes from the floor the C# sets
/// once it enters control mode (`MainWindow.xaml.cs:481-482`).
pub mod window {
    pub const WIDTH: f32 = 1020.0;
    pub const HEIGHT: f32 = 760.0;
    pub const MIN_WIDTH: f32 = 560.0;
    pub const MIN_HEIGHT: f32 = 460.0;
    /// Client-drawn title bar height. The C# had no client chrome and left this to
    /// DWM; this is our value, chosen to match the 32 px caption the shell strips.
    pub const TITLE_BAR_HEIGHT: f32 = 32.0;
}

/// Module toggle switch (`App.xaml:164-169`): 46×24 track, 18 px thumb inset 3,
/// travelling 22 px.
pub mod switch {
    pub const TRACK_WIDTH: f32 = 46.0;
    pub const TRACK_HEIGHT: f32 = 24.0;
    pub const THUMB: f32 = 18.0;
    pub const INSET: f32 = 3.0;
    pub const TRAVEL: f32 = 22.0;
}

/// Small checkbox (`App.xaml:127-131`).
pub mod checkbox {
    pub const SIZE: f32 = 14.0;
    pub const GAP: f32 = 6.0;
    pub const GLYPH: f32 = 10.0;
}

/// Slider (`App.xaml:232-298`): 6 px track, 4 px end dot, 6×16 thumb, and the two
/// scale pairs the template animates.
pub mod slider {
    pub const TRACK_HEIGHT: f32 = 6.0;
    pub const END_DOT: f32 = 4.0;
    pub const THUMB_WIDTH: f32 = 6.0;
    pub const THUMB_HEIGHT: f32 = 16.0;
    pub const HOVER_SCALE: f32 = 1.4;
    pub const HOVER_DOT_SCALE: f32 = 1.15;
    pub const DRAG_SCALE: f32 = 1.5;
    pub const DRAG_DOT_SCALE: f32 = 1.2;
}

/// Command-search overlay geometry.
///
/// `CommandSearchOverlay.xaml.cs:24-32`. The inventory's correction matters here:
/// `340` is the card's **minimum width**, not a module-card width, and the edge
/// gutter is 72, not the 56 the C# comment claims.
pub mod overlay {
    pub const CARD_MIN_WIDTH: f32 = 340.0;
    pub const CARD_MAX_WIDTH: f32 = 620.0;
    pub const CARD_TOP_RATIO: f32 = 0.30;
    pub const CARD_EDGE_GUTTER: f32 = 72.0;
    pub const RESULTS_MAX_HEIGHT: f32 = 300.0;
    pub const RESULTS_MIN_HEIGHT: f32 = 100.0;
    pub const RESULTS_VERTICAL_MARGIN: f32 = 12.0;
    /// Result row height (`CommandSearchOverlay.xaml:269`), radius 7, inset 10×1.
    pub const ROW_HEIGHT: f32 = 48.0;
    pub const ROW_INSET_X: f32 = 10.0;
    pub const ROW_INSET_Y: f32 = 1.0;
    /// Row washes, as opacities of `TextBrush` over the card
    /// (`CommandSearchOverlay.xaml:281-300`). Selected is hover plus an accent rule.
    pub const ROW_HOVER_WASH: f32 = 0.08;
    pub const ROW_SELECTED_WASH: f32 = 0.14;
}

/// Card washes shared by every list-style surface (`TabCardWash`, and the
/// contrast test's 0.06 / 0.12 pair, `Aoko.Tests/PaletteContrastTests.cs:96-100`).
pub mod wash {
    pub const HOVER: f32 = 0.06;
    pub const SELECTED: f32 = 0.12;
    /// `TabChipTint` opacity (`TabStyles.xaml:208`) and the two status tones.
    pub const CHIP_TINT: f32 = 0.16;
    pub const CHIP_TINT_CRITICAL: f32 = 0.20;
    pub const CHIP_TINT_WARN: f32 = 0.16;
}

/// The card width for a given available width: `clamp(340, available - 2×72, 620)`.
///
/// Port of `LayoutCard` (`CommandSearchOverlay.xaml.cs:177`). Integer-free and pure,
/// so it is unit-tested rather than eyeballed in a screenshot.
#[must_use]
pub fn overlay_card_width(available: f32) -> f32 {
    (available - 2.0 * overlay::CARD_EDGE_GUTTER)
        .clamp(overlay::CARD_MIN_WIDTH, overlay::CARD_MAX_WIDTH)
}

/// `CardTopRatio` as pixels, clamped so the card never starts above the window or
/// pushes its results area out of the bottom.
#[must_use]
pub fn overlay_card_top(window_height: f32) -> f32 {
    let preferred = window_height * overlay::CARD_TOP_RATIO;
    let floor = window::TITLE_BAR_HEIGHT + 8.0;
    let ceiling = (window_height - overlay::RESULTS_MIN_HEIGHT).max(floor);
    preferred.clamp(floor, ceiling)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_width_clamps_to_the_measured_bounds() {
        // At the 560 px window floor: 560 - 144 = 416, comfortably inside the bounds.
        assert_eq!(overlay_card_width(560.0), 416.0);
        // A narrow viewport clamps up to 340 rather than clipping the footer.
        assert_eq!(overlay_card_width(420.0), overlay::CARD_MIN_WIDTH);
        assert_eq!(overlay_card_width(1020.0), overlay::CARD_MAX_WIDTH);
        assert_eq!(overlay_card_width(1_400.0), overlay::CARD_MAX_WIDTH);
        // 340 + 2×72 = 484 is the width at which the card stops being clamped up.
        assert_eq!(overlay_card_width(484.0), 340.0);
    }

    #[test]
    fn card_top_tracks_the_ratio_within_bounds() {
        assert!((overlay_card_top(760.0) - 228.0).abs() < 1e-3);
        // The floor keeps it under the title bar on a very short window: 60 × 0.30 would
        // be 18, so the floor's 32 + 8 wins.
        assert_eq!(overlay_card_top(60.0), window::TITLE_BAR_HEIGHT + 8.0);
        // A 160 px window prefers 48, which clears both bounds (40..=60) untouched.
        assert_eq!(overlay_card_top(160.0), 48.0);
        // Below ~133 px the ceiling (height − 100) falls under the floor, and the floor
        // still wins so the card never starts above the window.
        assert_eq!(overlay_card_top(120.0), window::TITLE_BAR_HEIGHT + 8.0,);
    }

    #[test]
    fn switch_geometry_adds_up() {
        // Thumb inset 3 each side plus 22 px of travel from an 18 px thumb in 46 px.
        assert_eq!(
            switch::INSET * 2.0 + switch::THUMB + switch::TRAVEL,
            switch::TRACK_WIDTH
        );
        // And vertically it is centred.
        assert_eq!((switch::TRACK_HEIGHT - switch::THUMB) / 2.0, switch::INSET);
    }
}
