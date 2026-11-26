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

/// Stats tab (`MainWindow.xaml:1440-1515`).
///
/// The tab is the one place the markup puts a `Width` on a `ControlCardStyle`
/// border and a fixed height on a chart row, so these are the numbers a pixel
/// diff of the tab would catch. The bar heights themselves are not here: they are
/// `aoko_protocol::stats`'s `MAX_BAR_HEIGHT` scaled by the tracker, because the
/// 124 px row is the space a bar is drawn in, not the bar.
pub mod stats {
    /// The three summary borders' `Width` (`:1442`, `:1448`, `:1457`).
    pub const SUMMARY_CARD: f32 = 150.0;
    /// The `0,0,8,8` margin each of them and the Reset button carries.
    pub const SUMMARY_GAP: f32 = 8.0;
    /// The summary value's `FontSize="20"` (`:1445`).
    pub const VALUE: f32 = 20.0;
    /// The value's `Margin="0,4,0,0"` (`:1445`).
    pub const VALUE_GAP: f32 = 4.0;
    /// The sub-line's `Margin="0,2,0,0"` (`:1452`, `:1461`).
    pub const SUBLINE_GAP: f32 = 2.0;
    /// "Reset Stats" `Height="40"`, `Padding="14,6"` (`:1466-1467`).
    pub const RESET_HEIGHT: f32 = 40.0;
    /// The header's `Margin="0,0,0,10"` (`:1472`).
    pub const HEADER_GAP: f32 = 10.0;
    /// "CPS Distribution" `FontSize="16"` (`:1473`).
    pub const HEADING: f32 = 16.0;
    /// The chart box: `CornerRadius="6" Padding="8" MinHeight="158"` (`:1478`).
    pub const CHART_RADIUS: f32 = 6.0;
    pub const CHART_PADDING: f32 = 8.0;
    pub const CHART_MIN_HEIGHT: f32 = 158.0;
    /// One column (`:1488-1496`): a 124 px bar row, an 18×h bar of radius 3, a
    /// 3 px side margin, and a 9 px label 5 px under the bar.
    pub const BAR_AREA: f32 = 124.0;
    pub const BAR_WIDTH: f32 = 18.0;
    pub const BAR_RADIUS: f32 = 3.0;
    pub const BUCKET_GAP: f32 = 3.0;
    pub const LABEL: f32 = 9.0;
    pub const LABEL_GAP: f32 = 5.0;
    /// "No samples yet" `FontSize="12"` (`:1501`).
    pub const EMPTY: f32 = 12.0;
}

/// Command-search overlay geometry.
///
/// `CommandSearchOverlay.xaml.cs:24-32`. The inventory's correction matters here:
/// `340` is the card's **minimum width**, not a module-card width, and the edge
/// gutter is 72, not the 56 the C# comment claims.
/// Command-search result rows.
///
/// The **card's** geometry is not here: `card_width`, `card_top` and
/// `results_viewport_height` live in [`crate::views::command_search`], which is the C#
/// `LayoutCard` port, so the 340/620/72/0.30 numbers have exactly one home. This module
/// keeps only what the row renderer needs.
pub mod overlay {
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

/// The Block ESP tracked-block row, the `ItemsControl` item template at
/// `MainWindow.xaml:695-712`.
///
/// The row is a `DockPanel` at `Margin="6,3"` holding a bare checkbox, the block name, a
/// 22×14 colour swatch and a 22 px `DarkButton`. Every number below is one of those
/// attributes; nothing here is inferred.
pub mod block_esp {
    /// The `DockPanel`'s `Margin="6,3"` (`:696`).
    pub const ROW_PAD_X: f32 = 6.0;
    pub const ROW_PAD_Y: f32 = 3.0;
    /// The checkbox's `Margin="0,0,8,0"` (`:698`).
    pub const CHECK_GAP: f32 = 8.0;
    /// The remove cap's `Width="22"` (`:699`).
    pub const REMOVE_WIDTH: f32 = 22.0;
    /// The colour swatch's `Width="22" Height="14"` (`:703`).
    pub const SWATCH_WIDTH: f32 = 22.0;
    pub const SWATCH_HEIGHT: f32 = 14.0;
    /// The swatch's `CornerRadius="3"` (`:703`).
    pub const SWATCH_RADIUS: f32 = 3.0;
    /// The swatch's `Margin="6,0"` (`:704`), which is also the 6 px the name leaves before
    /// it.
    pub const SWATCH_GAP: f32 = 6.0;
    /// The name's `FontSize="12"` (`:708`).
    pub const NAME_TEXT: f32 = 12.0;
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn the_three_summary_cards_stay_on_one_row_at_the_default_width() {
        // The wrap panel has room for content minus the rail and the tab's own 12 px
        // margin. Three 150 px cards and the two 8 px gaps have to fit, or the C#
        // layout (which never wraps at this width) stops matching.
        let content = window::WIDTH - rail::WIDTH - rail::GAP - 2.0 * 12.0;
        assert!(3.0 * stats::SUMMARY_CARD + 2.0 * stats::SUMMARY_GAP <= content);
    }

    #[test]
    fn the_row_fits_inside_the_results_cap_without_slicing() {
        // 300 / 48 is 6.25: six rows plus their 1 px insets fit, the seventh scrolls.
        let rows = ((300.0 / (overlay::ROW_HEIGHT + 2.0 * overlay::ROW_INSET_Y)).floor()) as usize;
        assert_eq!(rows, 6);
        assert!(rows as f32 * (overlay::ROW_HEIGHT + 2.0 * overlay::ROW_INSET_Y) <= 300.0);
    }
}
