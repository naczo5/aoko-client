//! Design tokens: the ten-role palette, the derived command-search tints, and
//! the colour maths behind both.
//!
//! `MainWindow.GuiPalettes` is the normative table (MIGRATION_ARCHITECTURE §8.1),
//! and this module is its only Rust home. Every other module reads tokens from
//! here; no other module may write a colour literal. The WPF shell alone carries
//! 121 hex literals in `MainWindow.xaml`, and that dual source is the defect
//! arbitration 1 exists to prevent.
//!
//! Roles are named after the WPF properties so a reviewer can diff this file
//! against `MainWindow.xaml.cs:137-373` by eye.

pub mod metrics;
pub mod motion;

use std::sync::LazyLock;

use gpui::{Fill, Hsla, Rgba};

/// A colour token: 8-bit channels, opaque unless stated.
///
/// Tokens are stored as bytes rather than as GPUI's float [`Rgba`] so a palette
/// can be diffed against the C# hex table exactly, and so a derived tint is
/// reproducible bit for bit: the C# maths is integer channel maths with
/// `Math.Round`, not float blending.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    /// An opaque colour from `0xRRGGBB`.
    #[must_use]
    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
            a: 0xFF,
        }
    }

    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xFF }
    }

    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Same channels, new alpha. The C# writes these as
    /// `Color.FromArgb(0xE4, palette.Panel.R, …)` (`MainWindow.xaml.cs:1331`).
    #[must_use]
    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Parses `#RRGGBB` and `#AARRGGBB`.
    ///
    /// The 8-digit form is WPF's ARGB order, which is what the C# parses with
    /// `ColorConverter.ConvertFromString` for the overlay literals.
    #[must_use]
    pub fn parse_hex(text: &str) -> Option<Self> {
        let digits = text.trim().trim_start_matches('#');
        let parsed = u32::from_str_radix(digits, 16).ok()?;
        match digits.len() {
            6 => Some(Self::from_hex(parsed)),
            8 => Some(Self::rgba(
                (parsed >> 16) as u8,
                (parsed >> 8) as u8,
                parsed as u8,
                (parsed >> 24) as u8,
            )),
            _ => None,
        }
    }

    /// `#RRGGBB`, uppercase, no alpha channel.
    #[must_use]
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    /// Per-channel linear interpolation, `t = 0` returning `self`.
    ///
    /// Port of `MainWindow.Blend` (`:1268-1273`). `Math.Round` rounds halves to
    /// even in .NET, so this does too; half-away-from-zero would drift by one
    /// channel value on roughly one palette role in nine.
    #[must_use]
    pub fn blend(self, other: Self, t: f32) -> Self {
        let t = f64::from(t.clamp(0.0, 1.0));
        let mix = |x: u8, y: u8| round_ties_even(f64::from(x) + (f64::from(y) - f64::from(x)) * t);
        Self {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
            a: mix(self.a, other.a),
        }
    }

    /// Moves each channel towards 255. Port of `MainWindow.Lighten`
    /// (`:1262-1265`), including the 0..1 clamp.
    #[must_use]
    pub fn lighten(self, amount: f32) -> Self {
        let amount = f64::from(amount.clamp(0.0, 1.0));
        let lift = |v: u8| round_ties_even(f64::from(v) + (255.0 - f64::from(v)) * amount);
        Self {
            r: lift(self.r),
            g: lift(self.g),
            b: lift(self.b),
            a: self.a,
        }
    }

    /// WCAG relative luminance, the same curve `Aoko.Tests/PaletteContrastTests`
    /// implements: sRGB linearisation per channel, then 0.2126/0.7152/0.0722.
    #[must_use]
    pub fn luminance(self) -> f32 {
        fn linear(channel: u8) -> f32 {
            let s = f32::from(channel) / 255.0;
            if s <= 0.040_45 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG contrast ratio, 1.0 for identical colours.
    #[must_use]
    pub fn contrast_ratio(self, other: Self) -> f32 {
        let a = self.luminance();
        let b = other.luminance();
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
}

/// `Math.Round` on a double: ties go to even, then the result is clamped into a byte.
fn round_ties_even(value: f64) -> u8 {
    let rounded = value.round_ties_even().clamp(0.0, 255.0);
    rounded as u8
}

impl From<Color> for Rgba {
    fn from(color: Color) -> Self {
        let channel = |v: u8| f32::from(v) / 255.0;
        Rgba {
            r: channel(color.r),
            g: channel(color.g),
            b: channel(color.b),
            a: channel(color.a),
        }
    }
}

impl From<Color> for Hsla {
    fn from(color: Color) -> Self {
        Rgba::from(color).into()
    }
}

/// Fixed literals the C# keeps outside the palette.
///
/// They are not per-palette roles: the status brushes are fixed in
/// `Aoko/Views/TabStyles.xaml:10-11`, and the overlay's structural rim, scrim and
/// specular stops are fixed in `Aoko/Views/CommandSearchOverlay.xaml:17-18`,
/// `:118-123` and `:157`. They live here so no component invents a hex value.
pub mod fixed {
    use super::Color;

    /// `CriticalBrush` (`TabStyles.xaml:10`).
    pub const CRITICAL: Color = Color::from_hex(0xF4_3F_5E);
    /// `WarnBrush` (`TabStyles.xaml:11`).
    pub const WARN: Color = Color::from_hex(0xF5_BE_6A);
    /// Scrim top stop, `#9C020711` (`CommandSearchOverlay.xaml:119`).
    pub const SCRIM_TOP: Color = Color::rgba(0x02, 0x07, 0x11, 0x9C);
    /// Scrim bottom stop, `#D9050B17` (`CommandSearchOverlay.xaml:120`).
    pub const SCRIM_BOTTOM: Color = Color::rgba(0x05, 0x0B, 0x17, 0xD9);
    /// `GlassHairlineBrush`, `#2E8EB1CC` (`CommandSearchOverlay.xaml:17`).
    pub const GLASS_HAIRLINE: Color = Color::rgba(0x8E, 0xB1, 0xCC, 0x2E);
    /// `GlassSpecularBrush`, `#14FFFFFF` (`CommandSearchOverlay.xaml:18`).
    pub const GLASS_SPECULAR: Color = Color::rgba(0xFF, 0xFF, 0xFF, 0x14);
    /// White. Used only where the C# hardcodes `Foreground="White"`: the
    /// `DarkButton` hover label (`App.xaml:361`) and the colour picker's SV thumb
    /// ring (`ColorPickerDialog.xaml:43`). Kept as a token so components never write
    /// the literal.
    pub const CONTRAST_LABEL: Color = Color::from_hex(0xFF_FF_FF);
    /// The colour picker's SV square starts from pure red hue (`ColorPickerDialog.xaml:27`).
    pub const PICKER_HUE_ORIGIN: Color = Color::from_hex(0xFF_00_00);
}

/// The ten palette roles, in `MainWindow.GuiPalettes` order.
///
/// Nine are live. [`Theme::accent_secondary`] is carried and deliberately unread;
/// see its documentation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Theme {
    pub name: String,
    pub background: Color,
    pub panel: Color,
    pub slider_background: Color,
    pub slider_foreground: Color,
    pub accent: Color,
    /// INERT: carried for schema completeness, never rendered.
    ///
    /// `AccentSecondary` is assigned in all 18 palettes of both C# tables
    /// (`MainWindow.xaml.cs:35`, populated `:143-373`) and read at no render
    /// site: no `palette.AccentSecondary` read exists in `Aoko/` or
    /// `Aoko.Tests/`, no XAML resource key is fed from it, and the persisted
    /// `CustomPalette` has only nine colour fields, so a custom palette leaves it
    /// absent (`:1019-1032`). Arbitration 1 keeps the field for the JSON shape and
    /// says not to invent a visual use for it. Components must not read this.
    pub accent_secondary: Color,
    pub text: Color,
    pub dim_text: Color,
    pub tab_selected: Color,
    pub tab_hover: Color,
}

impl Theme {
    /// Looks a palette up by name, falling back to `Slate`.
    ///
    /// Port of `NormalizeThemeName` + the `TryGetValue` guard in `ApplyGuiTheme`
    /// (`MainWindow.xaml.cs:1288-1310`): case-insensitive, trimmed, and blank or
    /// unknown names normalise to `Slate`.
    #[must_use]
    pub fn by_name(name: &str) -> Theme {
        let trimmed = name.trim();
        builtins()
            .iter()
            .find(|theme| theme.name.eq_ignore_ascii_case(trimmed))
            .cloned()
            .unwrap_or_else(slate)
    }

    /// `Slate`, the fallback palette.
    #[must_use]
    pub fn slate() -> Theme {
        slate()
    }

    /// Palette names in table order, which is the order the Settings tab lists them.
    #[must_use]
    pub fn builtin_names() -> Vec<&'static str> {
        NAMES.to_vec()
    }

    /// The command-search card background: `Panel` at alpha `0xE4`
    /// (`CommandSearchGlassBrush`, `MainWindow.xaml.cs:1331`).
    ///
    /// Decision 2 replaces the C# blur behind it with an opaque card, so the
    /// 89 %-alpha value now sits over the scrim rather than over blurred content.
    #[must_use]
    pub fn search_glass(&self) -> Color {
        self.panel.with_alpha(0xE4)
    }

    /// Matched-run tint: `Blend(Text, Accent, 0.28)` (`:1336`).
    #[must_use]
    pub fn search_match(&self) -> Color {
        self.text.blend(self.accent, 0.28)
    }

    /// Pill behind a matched run: `Accent` at alpha `0x2E` (`:1337`).
    #[must_use]
    pub fn search_match_pill(&self) -> Color {
        self.accent.with_alpha(0x2E)
    }

    /// Query placeholder: `Blend(DimText, Text, 0.35)` (`:1338`).
    #[must_use]
    pub fn search_placeholder(&self) -> Color {
        self.dim_text.blend(self.text, 0.35)
    }

    /// Secondary overlay copy (`esc`, footer legend): `Blend(DimText, Text, 0.55)`
    /// (`:1339`).
    #[must_use]
    pub fn search_subtle_text(&self) -> Color {
        self.dim_text.blend(self.text, 0.55)
    }

    /// The palette a row/card lift uses on hover: `SliderForeground`, the same
    /// role `ControlCardStyle` uses for its rest border (`MainWindow.xaml:53`).
    #[must_use]
    pub fn card_border(&self) -> Color {
        self.slider_background
    }

    #[must_use]
    pub fn card_border_hover(&self) -> Color {
        self.slider_foreground
    }

    /// Derives a theme from a stored custom palette's nine hex roles, filling
    /// absent or invalid ones exactly as `MainWindow.RegisterPalette` does
    /// (`MainWindow.xaml.cs:1005-1032`).
    ///
    /// This takes the nine strings rather than `aoko_protocol::profile::CustomPalette`
    /// even though the crate depends on `aoko-protocol` (for the shell's state and the
    /// views). The palette is the one piece of protocol data this layer must not need:
    /// `theme/` is the bottom of the crate and is used by every widget, so keeping the
    /// wire type out of its signature is what lets `Theme` be built in a unit test with
    /// no profile in hand. The caller maps its fields in through [`CustomRoles`].
    #[must_use]
    pub fn from_custom(name: &str, roles: CustomRoles<'_>) -> Theme {
        let parse = |text: &str, fallback: Color| Color::parse_hex(text).unwrap_or(fallback);
        // RegisterPalette's fallback chain, in order: each role may default from an
        // earlier one, so these are evaluated top to bottom.
        let background = parse(roles.background, Color::rgb(0, 0, 0));
        let panel = parse(roles.panel, background);
        let slider_background = parse(roles.slider_background, panel.lighten(0.06));
        let slider_foreground = parse(roles.slider_foreground, panel.lighten(0.16));
        let accent = parse(roles.accent, Color::from_hex(0x80_80_80));
        let text = parse(roles.text, Color::from_hex(0xFF_FF_FF));
        let dim_text = parse(roles.dim_text, text.blend(background, 0.45));
        let tab_selected = parse(roles.tab_selected, panel.lighten(0.10));
        let tab_hover = parse(roles.tab_hover, panel.lighten(0.04));

        Theme {
            name: name.to_owned(),
            background,
            panel,
            slider_background,
            slider_foreground,
            accent,
            // A custom palette has no tenth role; the C# leaves it unset (`:1019-1032`).
            // Mirror the accent so the field stays populated without being read.
            accent_secondary: accent,
            text,
            dim_text,
            tab_selected,
            tab_hover,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        slate()
    }
}

/// The nine persisted hex roles of a custom palette, borrowed from its storage form.
pub struct CustomRoles<'a> {
    pub background: &'a str,
    pub panel: &'a str,
    pub slider_background: &'a str,
    pub slider_foreground: &'a str,
    pub accent: &'a str,
    pub text: &'a str,
    pub dim_text: &'a str,
    pub tab_selected: &'a str,
    pub tab_hover: &'a str,
}

/// Palette order is `MainWindow.xaml.cs:137-373`, which is also the Settings tab's
/// listing order (18 preview buttons, `MainWindow.xaml:1589-1901`).
const NAMES: [&str; 18] = [
    "Slate",
    "Ink",
    "Graphite",
    "Steel",
    "Blend",
    "Lush",
    "Water",
    "Lime Water",
    "Digital Horizon",
    "Coral",
    "Magic",
    "Blossom",
    "Pastel",
    "Sunkist",
    "Nord",
    "Cherry",
    "Aubergine",
    "Snowy Sky",
];

/// Builds one palette from its ten hex roles.
///
/// The array is positional in `MainWindow.GuiPalettes` order — background, panel,
/// slider background, slider foreground, accent, **accent secondary (inert)**, text,
/// dim text, tab selected, tab hover — so the table below reads as a table and a row
/// can be diffed against `MainWindow.xaml.cs:137-373` line for line.
fn palette(name: &str, roles: [u32; 10]) -> Theme {
    Theme {
        name: name.to_owned(),
        background: Color::from_hex(roles[0]),
        panel: Color::from_hex(roles[1]),
        slider_background: Color::from_hex(roles[2]),
        slider_foreground: Color::from_hex(roles[3]),
        accent: Color::from_hex(roles[4]),
        accent_secondary: Color::from_hex(roles[5]),
        text: Color::from_hex(roles[6]),
        dim_text: Color::from_hex(roles[7]),
        tab_selected: Color::from_hex(roles[8]),
        tab_hover: Color::from_hex(roles[9]),
    }
}

fn builtins() -> &'static [Theme] {
    static BUILTINS: LazyLock<Vec<Theme>> = LazyLock::new(|| {
        vec![
            palette(
                "Slate",
                [
                    0x0A0B0F, 0x12141A, 0x181B22, 0x2A2F38, 0xC7625A, 0xC7625A, 0xE8EAEE, 0x7A8290,
                    0x1F2229, 0x181B22,
                ],
            ),
            palette(
                "Ink",
                [
                    0x08090B, 0x101115, 0x16181C, 0x262830, 0xB0B6C0, 0xB0B6C0, 0xE8EAEE, 0x7A828F,
                    0x1D1F23, 0x16181C,
                ],
            ),
            palette(
                "Graphite",
                [
                    0x0B0B0D, 0x131316, 0x19191C, 0x2A2A2D, 0xB89B82, 0xB89B82, 0xE8E8EA, 0x82827E,
                    0x222226, 0x19191C,
                ],
            ),
            palette(
                "Steel",
                [
                    0x08090C, 0x0F1218, 0x161A21, 0x262C35, 0x6B8DAB, 0x6B8DAB, 0xE5E8EE, 0x7286A0,
                    0x1F232C, 0x161A21,
                ],
            ),
            palette(
                "Blend",
                [
                    0x070B14, 0x0D1424, 0x131E33, 0x1C2B49, 0x4794FD, 0x47FDA0, 0xEDF3F7, 0x7B94B5,
                    0x16233B, 0x10192A,
                ],
            ),
            palette(
                "Lush",
                [
                    0x080D09, 0x0F1710, 0x17241A, 0x223325, 0xA8E063, 0x56AB2F, 0xEEF5EE, 0x7A9C7D,
                    0x1B291D, 0x131D15,
                ],
            ),
            palette(
                "Water",
                [
                    0x060B12, 0x0B1624, 0x102136, 0x18304D, 0x0CE8C7, 0x0CA3E8, 0xEAF7F8, 0x709CB8,
                    0x12253D, 0x0E1D30,
                ],
            ),
            palette(
                "Lime Water",
                [
                    0x060E0E, 0x0B1B1B, 0x112828, 0x1A3A3A, 0x12FFF7, 0xB3FFAB, 0xF0FAF9, 0x70A5A0,
                    0x142E2E, 0x0E2222,
                ],
            ),
            palette(
                "Digital Horizon",
                [
                    0x090A14, 0x121324, 0x1B1D33, 0x262947, 0x5FC3E4, 0xE55D87, 0xF3EDF5, 0x8E7FA8,
                    0x1E213D, 0x15172C,
                ],
            ),
            palette(
                "Coral",
                [
                    0x0A0B0F, 0x14151C, 0x1E202B, 0x2D3040, 0xF4A896, 0x348597, 0xF7EDEB, 0xA08785,
                    0x222430, 0x191B24,
                ],
            ),
            palette(
                "Magic",
                [
                    0x060714, 0x0D0F24, 0x151838, 0x202554, 0x7F9EFF, 0x8E2DE2, 0xEDF1FF, 0x808EC7,
                    0x171B3D, 0x10132B,
                ],
            ),
            palette(
                "Blossom",
                [
                    0x0B0812, 0x161121, 0x221B30, 0x302742, 0xE2D0F9, 0x317773, 0xF7F2FD, 0x9E8CAE,
                    0x251D36, 0x1A1426,
                ],
            ),
            palette(
                "Pastel",
                [
                    0x0D0A10, 0x18131E, 0x241D2B, 0x332A3D, 0xF39BB2, 0xCFC4F3, 0xFBF3F6, 0xA8909D,
                    0x292131, 0x1C1722,
                ],
            ),
            palette(
                "Sunkist",
                [
                    0x0E0B07, 0x1A150D, 0x262014, 0x382F1E, 0xF2C94C, 0xF2994A, 0xFFF9ED, 0xA89675,
                    0x2E2517, 0x1E1810,
                ],
            ),
            palette(
                "Nord",
                [
                    0x0B0E12, 0x12181F, 0x1B242E, 0x263340, 0x8FBCBB, 0xA3BE8C, 0xECEFF4, 0x7B889B,
                    0x1E2833, 0x151C24,
                ],
            ),
            palette(
                "Cherry",
                [
                    0x0F080C, 0x1C1018, 0x2B1925, 0x3D2434, 0xBB377D, 0xFBD3E9, 0xFCF2F7, 0xA87C93,
                    0x301D2A, 0x21141D,
                ],
            ),
            palette(
                "Aubergine",
                [
                    0x0E060D, 0x1A0C18, 0x281325, 0x3B1D37, 0xAA076B, 0x61045F, 0xFAEBF7, 0x9E6E95,
                    0x2F162B, 0x200F1E,
                ],
            ),
            palette(
                "Snowy Sky",
                [
                    0x060C10, 0x0C1820, 0x132430, 0x1D3445, 0x01ABB3, 0x12E8E8, 0xEAF8FA, 0x75A2AC,
                    0x152836, 0x0E1B24,
                ],
            ),
        ]
    });

    &BUILTINS
}

fn slate() -> Theme {
    builtins()[0].clone()
}

/// GPUI's styled helpers take `impl Into<Fill>`, so tokens get one conversion rather
/// than every call site wrapping itself.
impl From<Color> for gpui::Background {
    fn from(color: Color) -> Self {
        gpui::Background::from(Hsla::from(color))
    }
}

impl From<Color> for Fill {
    fn from(color: Color) -> Self {
        Fill::from(Rgba::from(color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_eighteen_palettes_in_csharp_order() {
        let names: Vec<&str> = builtins().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, NAMES.to_vec());
        assert_eq!(names.len(), 18);
    }

    #[test]
    fn every_role_matches_the_csharp_table() {
        // Spot-checked against `MainWindow.xaml.cs:137-373`; the full table is the
        // `builtins()` literal, and this pins the rows most quoted elsewhere.
        let slate = Theme::by_name("Slate");
        assert_eq!(slate.background.to_hex(), "#0A0B0F");
        assert_eq!(slate.panel.to_hex(), "#12141A");
        assert_eq!(slate.slider_background.to_hex(), "#181B22");
        assert_eq!(slate.slider_foreground.to_hex(), "#2A2F38");
        assert_eq!(slate.accent.to_hex(), "#C7625A");
        assert_eq!(slate.text.to_hex(), "#E8EAEE");
        assert_eq!(slate.dim_text.to_hex(), "#7A8290");
        assert_eq!(slate.tab_selected.to_hex(), "#1F2229");
        assert_eq!(slate.tab_hover.to_hex(), "#181B22");

        let snowy = Theme::by_name("Snowy Sky");
        assert_eq!(snowy.background.to_hex(), "#060C10");
        assert_eq!(snowy.accent.to_hex(), "#01ABB3");
        assert_eq!(snowy.tab_hover.to_hex(), "#0E1B24");
    }

    #[test]
    fn accent_secondary_is_populated_but_inert() {
        // Every built-in carries the field, which is what arbitration 1 asks for.
        for theme in builtins() {
            assert_ne!(
                theme.accent_secondary,
                Color::TRANSPARENT,
                "{} has no AccentSecondary",
                theme.name
            );
        }

        // Four palettes set it equal to the accent, and the rest do not, so the
        // field cannot be reproduced by aliasing Accent. Nothing may read it; this
        // test only pins the data, and no component references the field.
        assert_eq!(
            Theme::by_name("Slate").accent_secondary,
            Theme::by_name("Slate").accent
        );
        assert_eq!(
            Theme::by_name("Steel").accent_secondary,
            Theme::by_name("Steel").accent
        );
        assert_ne!(
            Theme::by_name("Blend").accent_secondary,
            Theme::by_name("Blend").accent
        );
        assert_ne!(
            Theme::by_name("Nord").accent_secondary,
            Theme::by_name("Nord").accent
        );
    }

    #[test]
    fn by_name_trims_and_ignores_case() {
        assert_eq!(Theme::by_name("  steel ").name, "Steel");
        assert_eq!(Theme::by_name("LIME WATER").name, "Lime Water");
        assert_eq!(Theme::by_name("snowy sky").name, "Snowy Sky");
    }

    #[test]
    fn by_name_falls_back_to_slate() {
        assert_eq!(Theme::by_name("Nope").name, "Slate");
        assert_eq!(Theme::by_name("").name, "Slate");
        assert_eq!(Theme::by_name("   ").name, "Slate");
        assert_eq!(Theme::default().name, "Slate");
        assert_eq!(Theme::slate().name, "Slate");
    }

    #[test]
    fn blend_endpoints_and_ties() {
        let a = Color::from_hex(0x000000);
        let b = Color::from_hex(0xFFFFFF);
        assert_eq!(a.blend(b, 0.0), a);
        assert_eq!(a.blend(b, 1.0), b);
        assert_eq!(a.blend(b, 0.5), Color::from_hex(0x808080));

        // 160.5 is a tie; .NET's Math.Round takes it to 160, not 161.
        let dim = Color::from_hex(0x7A8290);
        let text = Color::from_hex(0xE8EAEE);
        assert_eq!(dim.blend(text, 0.35).r, 160);
        // Out-of-range t is clamped, not wrapped.
        assert_eq!(a.blend(b, 2.0), b);
        assert_eq!(a.blend(b, -1.0), a);
    }

    #[test]
    fn lighten_moves_towards_white_and_clamps() {
        let panel = Color::from_hex(0x12141A);
        assert_eq!(panel.lighten(0.0), panel);
        assert_eq!(panel.lighten(1.0), Color::from_hex(0xFFFFFF));
        assert_eq!(panel.lighten(2.0), Color::from_hex(0xFFFFFF));
        assert_eq!(panel.lighten(-1.0), panel);
        // RegisterPalette's derived defaults (`MainWindow.xaml.cs:1013-1016`),
        // against the Slate panel's red channel of 18: 18 + 237×amount.
        assert_eq!(panel.lighten(0.06).r, 32); // 32.22
        assert_eq!(panel.lighten(0.10).r, 42); // 41.70
        assert_eq!(panel.lighten(0.16).r, 56); // 55.92
    }

    #[test]
    fn search_tints_match_the_csharp_derivation() {
        let slate = Theme::slate();
        assert_eq!(slate.search_glass(), slate.panel.with_alpha(0xE4));
        // Blend(#E8EAEE, #C7625A, 0.28)
        assert_eq!(slate.search_match().to_hex(), "#DFC4C5");
        assert_eq!(slate.search_match_pill(), slate.accent.with_alpha(0x2E));
        // Blend(#7A8290, #E8EAEE, 0.35) and 0.55. 110×0.55 is 60.500000000000005 in
        // binary floating point, so the .NET `Math.Round` here is 183, not the 182 a
        // hand computation lands on. The tie-to-even rule only bites at exact halves.
        assert_eq!(slate.search_placeholder().to_hex(), "#A0A6B1");
        assert_eq!(slate.search_subtle_text().to_hex(), "#B7BBC4");
    }

    #[test]
    fn hex_roundtrip_and_parse() {
        assert_eq!(Color::parse_hex("#C7625A"), Some(Color::from_hex(0xC7625A)));
        assert_eq!(Color::parse_hex("c7625a"), Some(Color::from_hex(0xC7625A)));
        assert_eq!(
            Color::parse_hex("  #0A0B0F "),
            Some(Color::from_hex(0x0A0B0F))
        );
        // WPF ARGB order.
        assert_eq!(
            Color::parse_hex("#9C020711"),
            Some(Color::rgba(0x02, 0x07, 0x11, 0x9C))
        );
        assert_eq!(Color::parse_hex("#12345"), None);
        assert_eq!(Color::parse_hex("not a colour"), None);
        for theme in builtins() {
            let role = theme.accent;
            assert_eq!(Color::parse_hex(&role.to_hex()), Some(role));
        }
    }

    #[test]
    fn from_custom_fills_missing_roles_like_register_palette() {
        let theme = Theme::from_custom(
            "Mine",
            CustomRoles {
                background: "#010203",
                panel: "#050607",
                slider_background: "",      // invalid -> Lighten(panel, 0.06)
                slider_foreground: "bogus", // invalid -> Lighten(panel, 0.16)
                accent: "#FF8800",
                text: "#FFFFFF",
                dim_text: "",     // invalid -> Blend(text, bg, 0.45)
                tab_selected: "", // invalid -> Lighten(panel, 0.10)
                tab_hover: "",    // invalid -> Lighten(panel, 0.04)
            },
        );
        let panel = Color::from_hex(0x050607);
        assert_eq!(theme.panel, panel);
        assert_eq!(theme.slider_background, panel.lighten(0.06));
        assert_eq!(theme.slider_foreground, panel.lighten(0.16));
        assert_eq!(theme.tab_selected, panel.lighten(0.10));
        assert_eq!(theme.tab_hover, panel.lighten(0.04));
        assert_eq!(
            theme.dim_text,
            Color::from_hex(0xFFFFFF).blend(Color::from_hex(0x010203), 0.45)
        );
        assert_eq!(theme.accent_secondary, theme.accent);
        assert_eq!(theme.name, "Mine");
    }

    // Ports of `Aoko.Tests/PaletteContrastTests.cs`, run against all 18 palettes
    // rather than only the live one: the claims are palette-independent in .NET
    // and must stay that way here.
    #[test]
    fn chip_labels_pass_aa_on_their_tint_layers() {
        for theme in builtins() {
            for (tone, opacity) in [
                (theme.accent, 0.16_f32),
                (fixed::CRITICAL, 0.20),
                (fixed::WARN, 0.16),
            ] {
                let tint = theme.panel.blend(tone, opacity);
                let ratio = theme.text.contrast_ratio(tint);
                assert!(
                    ratio >= 4.5,
                    "{}: text on a {tone:?} tint is {ratio:.2}:1",
                    theme.name
                );
            }
        }
    }

    #[test]
    fn slate_passes_the_csharp_contrast_claims() {
        // Faithful port of `StatusTones_PassAaOnTheTabBackground`. The C# reads the
        // *live* palette, which is Slate by default, so these four numbers are a
        // claim about Slate and are not asserted over the table.
        let slate = Theme::slate();
        assert!(slate.dim_text.contrast_ratio(slate.background) >= 4.5);
        assert!(slate.accent.contrast_ratio(slate.background) >= 4.5);
        assert!(fixed::CRITICAL.contrast_ratio(slate.background) >= 4.5);
        assert!(slate.accent.contrast_ratio(slate.panel) >= 4.5);
    }

    #[test]
    fn contrast_holds_across_the_whole_table() {
        // The claims that really are palette-independent: light-on-dark body copy and
        // the ordering of the wash layers. Values measured over all 18 palettes; the
        // worst cases are Ink's 1.28:1 card edge and Aubergine's 1.03:1 hover wash.
        for theme in builtins() {
            let border = theme.slider_foreground.contrast_ratio(theme.panel);
            let rest = theme.panel.contrast_ratio(theme.background);
            let hover = theme
                .panel
                .blend(theme.accent, 0.06)
                .contrast_ratio(theme.panel);
            let selected = theme
                .panel
                .blend(theme.accent, 0.12)
                .contrast_ratio(theme.panel);

            assert!(
                border > rest,
                "{}: edge is not stronger than the fill",
                theme.name
            );
            assert!(
                selected > hover,
                "{}: selected wash must beat hover",
                theme.name
            );
            assert!(
                hover > 1.0,
                "{}: hover wash is invisible ({hover:.3}:1)",
                theme.name
            );
            assert!(
                theme.text.contrast_ratio(theme.panel) >= 4.5,
                "{}",
                theme.name
            );
            assert!(
                theme.dim_text.contrast_ratio(theme.background) >= 4.5,
                "{}",
                theme.name
            );
            assert!(
                fixed::CRITICAL.contrast_ratio(theme.background) >= 4.5,
                "{}",
                theme.name
            );
        }
    }

    #[test]
    fn cherry_and_aubergine_accents_are_below_aa_by_design() {
        // These are shipped C# values, not a design claim: `#BB377D` is 3.73:1 and
        // `#AA076B` is 2.81:1 on their backgrounds, so a palette picker that offered
        // only AA-safe accents would have to drop two of the eighteen. Asserted so a
        // future "fix" is a deliberate decision rather than a silent palette edit.
        let cherry = Theme::by_name("Cherry");
        let aubergine = Theme::by_name("Aubergine");
        assert!(cherry.accent.contrast_ratio(cherry.background) < 4.5);
        assert!(aubergine.accent.contrast_ratio(aubergine.background) < 4.5);
        // Everything else clears AA on its background.
        for theme in builtins() {
            if theme.name == "Cherry" || theme.name == "Aubergine" {
                continue;
            }
            assert!(
                theme.accent.contrast_ratio(theme.background) >= 4.5,
                "{}: accent dropped below AA",
                theme.name
            );
        }
    }

    #[test]
    fn primary_label_uses_the_background_not_white() {
        // `PrimaryLabel_PassesAaWithTheBackgroundBrush`: white on the Slate accent is
        // 3.95:1 and fails, so the primary button label is the background colour.
        let slate = Theme::slate();
        assert!(slate.background.contrast_ratio(slate.accent) >= 4.5);
        assert!(Color::from_hex(0xFFFFFF).contrast_ratio(slate.accent) < 4.5);
    }

    #[test]
    fn search_match_tint_stays_readable_over_the_card() {
        for theme in builtins() {
            let card = theme.search_glass();
            let ratio = theme.search_match().contrast_ratio(card);
            assert!(
                ratio >= 4.5,
                "{}: matched run is {ratio:.2}:1 on the card",
                theme.name
            );
            let placeholder = theme.search_placeholder().contrast_ratio(card);
            assert!(
                placeholder >= 3.0,
                "{}: placeholder is only {placeholder:.2}:1",
                theme.name
            );
        }
    }
}
