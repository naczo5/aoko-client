//! User-defined GUI palettes, ported from `Aoko/Core/CustomPalette.cs`.
//!
//! All colours are stored as `#RRGGBB` hex strings so a palette is fully
//! self-contained on disk and forward-compatible with future editors that expose
//! more of the individual roles.
//!
//! Ten roles: this is `MainWindow.GuiPalettes`, which arbitration decision 1 makes
//! normative over the derived eight-role `ThemeManager.Themes` view.

use serde::{Deserialize, Serialize};

/// A user-defined GUI colour palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomPalette {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_background")]
    pub background: String,
    #[serde(default = "default_panel")]
    pub panel: String,
    #[serde(rename = "sliderBackground", default = "default_slider_background")]
    pub slider_background: String,
    #[serde(rename = "sliderForeground", default = "default_slider_foreground")]
    pub slider_foreground: String,
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_text")]
    pub text: String,
    #[serde(rename = "dimText", default = "default_dim_text")]
    pub dim_text: String,
    #[serde(rename = "tabSelected", default = "default_tab_selected")]
    pub tab_selected: String,
    #[serde(rename = "tabHover", default = "default_tab_hover")]
    pub tab_hover: String,
}

fn default_name() -> String {
    "Custom".to_owned()
}
fn default_background() -> String {
    "#0A0B0F".to_owned()
}
fn default_panel() -> String {
    "#12141A".to_owned()
}
fn default_slider_background() -> String {
    "#181B22".to_owned()
}
fn default_slider_foreground() -> String {
    "#2A2F38".to_owned()
}
fn default_accent() -> String {
    "#C7625A".to_owned()
}
fn default_text() -> String {
    "#E8EAEE".to_owned()
}
fn default_dim_text() -> String {
    "#7A8290".to_owned()
}
fn default_tab_selected() -> String {
    "#1F2229".to_owned()
}
fn default_tab_hover() -> String {
    "#181B22".to_owned()
}

impl Default for CustomPalette {
    fn default() -> Self {
        Self {
            name: default_name(),
            background: default_background(),
            panel: default_panel(),
            slider_background: default_slider_background(),
            slider_foreground: default_slider_foreground(),
            accent: default_accent(),
            text: default_text(),
            dim_text: default_dim_text(),
            tab_selected: default_tab_selected(),
            tab_hover: default_tab_hover(),
        }
    }
}

/// Serialises the palette set the way `PaletteStore.JsonOptions` does: camelCase,
/// indented.
#[must_use]
pub fn to_palettes_json(palettes: &[CustomPalette]) -> String {
    serde_json::to_string_pretty(palettes).unwrap_or_default()
}

/// Parses a palette file. Malformed input degrades to an empty set, matching the
/// managed best-effort loader.
#[must_use]
pub fn from_palettes_json(json: &str) -> Vec<CustomPalette> {
    if json.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str(json).unwrap_or_default()
}

/// Inserts or replaces a palette by name (case-insensitive).
pub fn upsert(palettes: &mut Vec<CustomPalette>, palette: CustomPalette) {
    match palettes
        .iter()
        .position(|existing| existing.name.eq_ignore_ascii_case(&palette.name))
    {
        Some(index) => palettes[index] = palette,
        None => palettes.push(palette),
    }
}

/// Removes every palette with the given name (case-insensitive).
pub fn delete(palettes: &mut Vec<CustomPalette>, name: &str) {
    palettes.retain(|existing| !existing.name.eq_ignore_ascii_case(name));
}
