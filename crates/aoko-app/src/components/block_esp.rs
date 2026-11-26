//! One row of the Block ESP tracked-block list.
//!
//! The managed row is the `ItemsControl`'s item template at `MainWindow.xaml:695-712`: a
//! `DockPanel` at `Margin="6,3"` holding a bare `CheckBox` (`:698`), the block name
//! (`:708`), a 22×14 colour swatch with a 1 px `SliderFgBrush` edge (`:703`) and a 22 px
//! `DarkButton` whose content is `✕` (`:699`). The two tooltips are
//! [`copy::BLOCK_ESP_REMOVE_TOOLTIP`] and [`copy::BLOCK_ESP_COLOR_TOOLTIP`]; the glyph is
//! [`copy::REMOVE_GLYPH`]. No string is written here.
//!
//! **The row owns nothing.** `Profile::block_esp_targets` is the list, so the checkbox, the
//! swatch and the cap each report a [`RowAction`] naming their entry's `registry_id`, and the
//! caller writes it through to the profile. A row that mutated a local `enabled` would paint
//! the switch and forget it at the next frame; that is the defect this shape exists to
//! prevent.
//!
//! Order follows the `DockPanel`: the box is docked left, the cap and the swatch right, and
//! the name takes what is left. The flex row below paints that order directly, which is why
//! the name is `flex_1` between the box and the swatch rather than last.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, ClickEvent, IntoElement, SharedString, Window};

use std::rc::Rc;

use crate::components::{button, checkbox_bare, tooltip, ButtonVariant};
use crate::theme::{metrics, Color, Theme};
use crate::views::copy;

/// `BlockEspConfig.DefaultColorHex` (`Aoko/Core/BlockEsp.cs:161`): the colour a target carries
/// when its stored hex is missing or invalid, in `NormalizeColor` (`:217-238`).
pub const DEFAULT_COLOR: Color = Color::from_hex(0x00_E5_FF);

/// Which of a row's three controls was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowControl {
    /// The 14 px checkbox at the row's left.
    Toggle,
    /// The colour swatch, which opens the picker for this entry.
    Color,
    /// The `✕` cap at the row's right.
    Remove,
}

/// What a row asks its owner to do, always naming the entry it came from.
///
/// The caller owns the profile, so this is the whole write path: `Toggle` carries the value
/// the profile should take (the opposite of what the row paints), not the one it is leaving
/// behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowAction {
    /// Set `enabled` on the entry with this registry id.
    Toggle {
        /// `BlockEspTargetData::registry_id`, as stored.
        registry_id: String,
        /// The value to write: the checkbox's new state.
        enabled: bool,
    },
    /// Open the colour picker for the entry with this registry id.
    Color {
        /// `BlockEspTargetData::registry_id`, as stored.
        registry_id: String,
    },
    /// Drop the entry with this registry id.
    Remove {
        /// `BlockEspTargetData::registry_id`, as stored.
        registry_id: String,
    },
}

/// One tracked-block entry, borrowed from the profile for a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEspRow {
    /// `BlockEspTargetData::registry_id`, which is also the entry's identity in the list.
    pub registry_id: SharedString,
    /// `BlockEspTargetData::display_name`, the text the row shows.
    pub display_name: SharedString,
    /// `BlockEspTargetData::color_hex`, six uppercase digits with no `#`.
    pub color_hex: SharedString,
    /// `BlockEspTargetData::enabled`, which the checkbox paints and never writes.
    pub enabled: bool,
}

impl BlockEspRow {
    /// A row for one stored entry.
    #[must_use]
    pub fn new(
        registry_id: impl Into<SharedString>,
        display_name: impl Into<SharedString>,
        color_hex: impl Into<SharedString>,
        enabled: bool,
    ) -> Self {
        Self {
            registry_id: registry_id.into(),
            display_name: display_name.into(),
            color_hex: color_hex.into(),
            enabled,
        }
    }

    /// The swatch colour, through the protocol's own colour rule.
    ///
    /// `BlockEspConfig.NormalizeColor` (`Aoko/Core/BlockEsp.cs:217-238`) accepts one `#` and
    /// exactly six digits, and returns [`DEFAULT_COLOR`] otherwise. That rule lives in
    /// `aoko_protocol::block_esp::normalize_color`, so this calls it rather than restating it;
    /// `Color::parse_hex` only turns the result into channels.
    #[must_use]
    pub fn color(&self) -> Color {
        let normalized = aoko_protocol::block_esp::normalize_color(Some(self.color_hex.as_ref()));
        match Color::parse_hex(&normalized) {
            Some(color) => color,
            // Unreachable: `normalize_color` returns six hex digits or the default. The arm
            // exists because a panic in a row paint would take the window down.
            None => DEFAULT_COLOR,
        }
    }

    /// The action one of this row's controls produces.
    ///
    /// Pure, and the only place a row decides anything: the element below feeds its three
    /// controls through this, so what a press asks for can be asserted without a window.
    #[must_use]
    pub fn action(&self, control: RowControl) -> RowAction {
        let registry_id = self.registry_id.to_string();
        match control {
            RowControl::Toggle => RowAction::Toggle {
                registry_id,
                enabled: !self.enabled,
            },
            RowControl::Color => RowAction::Color { registry_id },
            RowControl::Remove => RowAction::Remove { registry_id },
        }
    }

    /// The row's element. `on_action` is the caller's write-through: it receives the
    /// [`RowAction`] and updates the profile.
    pub fn element(
        self,
        theme: &Theme,
        on_action: impl Fn(RowAction, &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        let registry_id = self.registry_id.to_string();
        let color = self.color();

        let toggle_id = format!("blockesp-toggle-{registry_id}");
        let color_id = format!("blockesp-color-{registry_id}");
        let remove_id = format!("blockesp-remove-{registry_id}");

        // One owner for the callback, cloned per control: an `Fn` closure captures by move, so
        // three handlers cannot each take `on_action` unless it is shared.
        let callback: Rc<dyn Fn(RowAction, &mut Window, &mut App)> = Rc::new(on_action);
        let press = |action: RowAction| {
            let callback = Rc::clone(&callback);
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                (*callback)(action.clone(), window, cx);
            }
        };
        let toggled = press(self.action(RowControl::Toggle));
        let picked = press(self.action(RowControl::Color));
        let removed = press(self.action(RowControl::Remove));

        div()
            .id(SharedString::from(format!("blockesp-row-{registry_id}")))
            .flex()
            .items_center()
            .px(px(metrics::block_esp::ROW_PAD_X))
            .py(px(metrics::block_esp::ROW_PAD_Y))
            .child(
                div()
                    .mr(px(metrics::block_esp::CHECK_GAP))
                    .child(checkbox_bare(toggle_id, theme, self.enabled, toggled)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(metrics::block_esp::NAME_TEXT))
                    .text_color(theme.text)
                    .child(self.display_name.clone()),
            )
            .child(
                div()
                    .id(SharedString::from(color_id))
                    .ml(px(metrics::block_esp::SWATCH_GAP))
                    .mr(px(metrics::block_esp::SWATCH_GAP))
                    .w(px(metrics::block_esp::SWATCH_WIDTH))
                    .h(px(metrics::block_esp::SWATCH_HEIGHT))
                    .rounded(px(metrics::block_esp::SWATCH_RADIUS))
                    .border_1()
                    .border_color(theme.slider_foreground)
                    .bg(color)
                    .cursor_pointer()
                    .tooltip(tooltip(theme, copy::BLOCK_ESP_COLOR_TOOLTIP))
                    .on_click(picked),
            )
            .child(button::button_sized(
                remove_id,
                theme,
                ButtonVariant::Dark,
                copy::REMOVE_GLYPH,
                metrics::block_esp::REMOVE_WIDTH,
                removed,
            ))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(enabled: bool) -> BlockEspRow {
        BlockEspRow::new("minecraft:diamond_ore", "Diamond Ore", "00E5FF", enabled)
    }

    /// The write-through value, not the one being left behind: a row that reported its own
    /// `enabled` would make the checkbox paint the change and do nothing.
    #[test]
    fn the_checkbox_asks_for_the_opposite_of_what_it_paints() {
        assert_eq!(
            row(false).action(RowControl::Toggle),
            RowAction::Toggle {
                registry_id: "minecraft:diamond_ore".to_owned(),
                enabled: true,
            }
        );
        assert_eq!(
            row(true).action(RowControl::Toggle),
            RowAction::Toggle {
                registry_id: "minecraft:diamond_ore".to_owned(),
                enabled: false,
            }
        );
    }

    /// The id comes from the entry, so the caller can find the right one in the profile
    /// without the row holding an index that a reorder would invalidate.
    #[test]
    fn the_swatch_and_the_cap_name_their_entry() {
        let row = row(true);
        assert_eq!(
            row.action(RowControl::Color),
            RowAction::Color {
                registry_id: "minecraft:diamond_ore".to_owned()
            }
        );
        assert_eq!(
            row.action(RowControl::Remove),
            RowAction::Remove {
                registry_id: "minecraft:diamond_ore".to_owned()
            }
        );
    }

    /// The swatch reads the protocol's colour rule, so an invalid stored hex paints the
    /// default cyan rather than a colour invented here (`BlockEspConfig.NormalizeColor`,
    /// `Aoko/Core/BlockEsp.cs:217-238`).
    #[test]
    fn a_stored_colour_parses_through_the_protocol_rule() {
        assert_eq!(
            BlockEspRow::new("id", "Name", "00E5FF", true).color(),
            DEFAULT_COLOR
        );
        assert_eq!(
            BlockEspRow::new("id", "Name", "#ffd700", true).color(),
            Color::from_hex(0xFF_D7_00),
            "one leading #, any case"
        );
        assert_eq!(
            BlockEspRow::new("id", "Name", "not-a-colour", true).color(),
            DEFAULT_COLOR
        );
        assert_eq!(
            BlockEspRow::new("id", "Name", "abc", true).color(),
            DEFAULT_COLOR,
            "the shorthand is TryParseHex's rule, not NormalizeColor's"
        );
    }
}
