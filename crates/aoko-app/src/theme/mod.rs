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
/// reproducible bit for bit: the C# maths is `Math.Round` over a `double` amount,
/// not float blending.
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

    /// Parses `#RGB`, `#RRGGBB` and `#AARRGGBB`, with or without the leading `#`.
    ///
    /// The 8-digit form is WPF's ARGB order, which is what the C# parses with
    /// `ColorConverter.ConvertFromString` for the overlay literals. The 3-digit shorthand is
    /// `TryParseHex`'s (`MainWindow.xaml.cs:1237-1241`), which doubles each digit before it
    /// parses; `ColorConverter` accepts it as well, so both authorities the C# consults
    /// agree and this is the workspace's only implementation of the rule.
    ///
    /// One leading `#` is stripped, not every one. `TryParseHex` removes a single `#`
    /// (`MainWindow.xaml.cs:1233-1234`) and then fails on a second, so repairing `##RRGGBB`
    /// here would paint a colour the managed side refuses from the same text — and this
    /// parser is reached from the dialogs' hex entries, where that typo arrives.
    ///
    /// `#RGBA`/`#ARGB` (the 4-digit form) is deliberately absent: no managed path accepts it,
    /// because `TryParseHex` only expands three digits and then insists on six.
    #[must_use]
    pub fn parse_hex(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);

        // `#RGB`, doubled into `#RRGGBB` the way `TryParseHex` does it. The recursion is on a
        // six-character string, so it terminates; every digit is one of the originals, which
        // is why the shorthand is always opaque.
        if digits.chars().count() == 3 {
            let mut doubled = String::with_capacity(6);
            for character in digits.chars() {
                doubled.push(character);
                doubled.push(character);
            }
            return Self::parse_hex(&doubled);
        }

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

    /// `#RGB` or `#RRGGBB`, with or without the leading `#`, and nothing else.
    ///
    /// Port of `TryParseHex` (`MainWindow.xaml.cs:1227-1253`), which expands three digits and
    /// then insists on six. The 8-digit `#AARRGGBB` form [`Color::parse_hex`] accepts for the
    /// overlay literals is **invalid** to it, so a palette role written that way takes its
    /// fallback exactly as `ParseHexOr` gives it (`RegisterPalette`, `:1009-1017`) instead of
    /// painting a translucent colour the managed app never shows. One leading `#` is stripped
    /// for the reason [`Color::parse_hex`] gives.
    ///
    /// This is the palette-entry rule and it is implemented once: [`Color::parse_hex`] does
    /// the parsing, and the digit count is the only extra check. A stored palette
    /// ([`Theme::from_custom`]) and the editor's hex boxes (`views::palette_store::parse_entry`,
    /// which delegates here) therefore accept exactly the same text.
    #[must_use]
    pub fn parse_rgb_hex(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
        match digits.chars().count() {
            3 | 6 => Self::parse_hex(trimmed),
            _ => None,
        }
    }

    /// Per-channel linear interpolation, `t = 0` returning `self`.
    ///
    /// Port of `MainWindow.Blend` (`:1268-1273`), which takes a **`double`**:
    /// `Math.Round(x + (y - x) * t)`, then `Color.FromRgb`. The amount is therefore
    /// `impl Into<f64>` and the arithmetic runs in `f64`, because the amount's precision
    /// is what decides a tie. A decimal literal written here (`0.28`) infers as `f64` and
    /// is bit for bit the value the C# holds; an `f32` amount is a *different* real
    /// number, one that can land on the other side of a half — `0.55f32` is
    /// `0.5500000119`, which takes `#7A8290`'s red channel of `182.5` to 183 where the
    /// C# takes it to 182 ([`Theme::search_subtle_text`]'s tint). So a ported amount is a
    /// literal or an `f64` constant and never an `f32`; the `f32` form exists for animated
    /// progress (the switch's eased `t`), which has no managed counterpart to match.
    ///
    /// `Math.Round` rounds halves to even in .NET, so this does too. The result is
    /// opaque, as `Color.FromRgb` is: alpha is not interpolated, and a translucent input
    /// does not make a translucent output. No palette role is translucent in the managed
    /// data (`TryParseHex` has no alpha digits), so every ported call is unaffected.
    #[must_use]
    pub fn blend(self, other: Self, t: impl Into<f64>) -> Self {
        let amount: f64 = t.into();
        let amount = amount.clamp(0.0, 1.0);
        let mix = |x: u8, y: u8| {
            round_ties_even(f64::from(x) + (f64::from(y) - f64::from(x)) * amount)
        };
        Self {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
            a: 0xFF,
        }
    }

    /// Moves each channel towards 255. Port of `MainWindow.Lighten` (`:1260-1265`),
    /// including the 0..1 clamp, the `double` amount and the opaque result.
    ///
    /// The amount's precision decides ties here too: Pastel's panel blue is 30, and
    /// `225 × 0.06` is exactly `13.5`, so `RegisterPalette` derives `#26212C` (44, even)
    /// from the `double` — `f64::from(0.06f32)` gives `#26212B` instead. See
    /// [`Color::blend`] for why the parameter is `impl Into<f64>`.
    #[must_use]
    pub fn lighten(self, amount: impl Into<f64>) -> Self {
        let amount: f64 = amount.into();
        let amount = amount.clamp(0.0, 1.0);
        let lift = |v: u8| round_ties_even(f64::from(v) + (255.0 - f64::from(v)) * amount);
        Self {
            r: lift(self.r),
            g: lift(self.g),
            b: lift(self.b),
            a: 0xFF,
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

/// `Math.Round` on a double, then the C#'s `(byte)` cast.
///
/// `Math.Round` returns a `double`; the managed code casts it, which truncates, but the
/// value is already integral so the cast is exact. Interpolation of two bytes cannot
/// leave `0..=255`, so the clamp only has a value to give for `NaN` (a `NaN` amount
/// clamps to `NaN` in both languages and casts to 0).
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

    /// One palette's accent, without building the palette.
    ///
    /// Port of the same lookup [`Theme::by_name`] does, for a caller that needs a single
    /// colour: the Settings tab's preview rows read only the accent, and cloning a whole
    /// `Theme` per row per frame allocated its `String` name eighteen times a frame.
    #[must_use]
    pub fn accent_by_name(name: &str) -> Color {
        let trimmed = name.trim();
        let table = builtins();
        table
            .iter()
            .find(|theme| theme.name.eq_ignore_ascii_case(trimmed))
            .or_else(|| table.first())
            .map_or(Color::TRANSPARENT, |theme| theme.accent)
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
    /// "Invalid" is [`Color::parse_rgb_hex`], i.e. `ParseHexOr`'s `TryParseHex`: the role must
    /// be three or six hex digits after one optional `#`. The 8-digit `#AARRGGBB` form is not
    /// one of the accepted spellings, because a managed palette has no alpha and refuses that
    /// text — so a hand-edited role carrying an alpha channel takes its derived fallback here
    /// for the same reason it takes one there, and the theme can never hold a translucent role
    /// the managed window could not paint.
    ///
    /// This takes the nine strings rather than `aoko_protocol::profile::CustomPalette`
    /// even though the crate depends on `aoko-protocol` (for the shell's state and the
    /// views). The palette is the one piece of protocol data this layer must not need:
    /// `theme/` is the bottom of the crate and is used by every widget, so keeping the
    /// wire type out of its signature is what lets `Theme` be built in a unit test with
    /// no profile in hand. The caller maps its fields in through [`CustomRoles`].
    #[must_use]
    pub fn from_custom(name: &str, roles: CustomRoles<'_>) -> Theme {
        let parse = |text: &str, fallback: Color| Color::parse_rgb_hex(text).unwrap_or(fallback);
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

        // Black and white are fixed points of themselves, and the amount's own ends are
        // exact in either direction.
        assert_eq!(a.blend(a, 0.5), a);
        assert_eq!(b.blend(b, 0.5), b);
        assert_eq!(b.blend(a, 0.0), b);
        assert_eq!(b.blend(a, 1.0), a);

        // `Math.Round`'s half-to-even rule, on channels where it decides the answer:
        // 0.5 -> 0, 1.5 -> 2 and 2.5 -> 2, so a plain `round()` (half away from zero)
        // fails two of these three.
        assert_eq!(Color::rgb(0, 0, 0).blend(Color::rgb(1, 1, 1), 0.5).r, 0);
        assert_eq!(Color::rgb(0, 0, 0).blend(Color::rgb(3, 3, 3), 0.5).r, 2);
        assert_eq!(Color::rgb(1, 1, 1).blend(Color::rgb(2, 2, 2), 0.5).r, 2);
        assert_eq!(Color::rgb(0, 0, 0).blend(Color::rgb(5, 5, 5), 0.5).r, 2);
        // 127.5 is the tie that does round up, because 128 is the even one.
        assert_eq!(a.blend(b, 0.5).r, 128);

        // 110 × 0.35 is exactly 160.5 in `double`, so 122 + 160.5 is a tie and .NET's
        // `Math.Round` takes it to 160, not 161.
        let dim = Color::from_hex(0x7A8290);
        let text = Color::from_hex(0xE8EAEE);
        assert_eq!(dim.blend(text, 0.35).r, 160);
        // Out-of-range t is clamped, not wrapped.
        assert_eq!(a.blend(b, 2.0), b);
        assert_eq!(a.blend(b, -1.0), a);

        // The amount is the C# `double`, and an `f32` is a different real number: 110 ×
        // 0.55 rounds to exactly 60.5 in `double` (a tie, so 182), while 110 × 0.55f32 is
        // 60.50000131, which rounds up to 183. This pair is why the parameter is
        // `impl Into<f64>` and why a ported amount is never written as an `f32`.
        assert_eq!(dim.blend(text, 0.55).to_hex(), "#B6BBC4");
        assert_eq!(dim.blend(text, 0.55_f32).to_hex(), "#B7BBC4");

        // `Color.FromRgb` has no alpha channel to carry, so the result is opaque whatever
        // the inputs were.
        let clear = Color::rgba(0x00, 0x00, 0x00, 0x40);
        let solid = Color::rgba(0xFF, 0xFF, 0xFF, 0x80);
        assert_eq!(clear.blend(solid, 0.5).a, 0xFF);
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

        // 255 is already white: 255 + 0 × amount, at both ends and in the middle.
        let white = Color::from_hex(0xFFFFFF);
        assert_eq!(white.lighten(0.0), white);
        assert_eq!(white.lighten(0.5), white);
        assert_eq!(white.lighten(1.0), white);
        // And 0 is black at amount 0 only, with 0.5 landing on the 127.5 tie that goes to
        // the even 128 rather than 127.
        let black = Color::from_hex(0x000000);
        assert_eq!(black.lighten(0.0), black);
        assert_eq!(black.lighten(0.5), Color::from_hex(0x808080));
        // 1 + 254 × 0.5 = 128 exactly, which is not a tie at all.
        let near_black = Color::from_hex(0x010101);
        assert_eq!(near_black.lighten(0.5), Color::from_hex(0x808080));

        // A tie on a real palette role: Pastel's panel blue is 30, so 225 × 0.06 is
        // exactly 13.5 and 225 × 0.10 is exactly 22.5 — both `RegisterPalette` amounts,
        // both half-to-even, and both moved by one by the `f32` spellings.
        let pastel = Color::from_hex(0x18131E);
        assert_eq!(pastel.lighten(0.06).b, 44); // 43.5 -> 44
        assert_eq!(pastel.lighten(0.10).b, 52); // 52.5 -> 52
        assert_eq!(pastel.lighten(0.06_f32).b, 43);
        assert_eq!(pastel.lighten(0.10_f32).b, 53);

        // Opaque, like the `Color.FromRgb` the C# returns.
        assert_eq!(Color::rgba(0x12, 0x14, 0x1A, 0x40).lighten(0.06).a, 0xFF);
    }

    #[test]
    fn search_tints_match_the_csharp_derivation() {
        let slate = Theme::slate();
        assert_eq!(slate.search_glass(), slate.panel.with_alpha(0xE4));
        // Blend(#E8EAEE, #C7625A, 0.28)
        assert_eq!(slate.search_match().to_hex(), "#DFC4C5");
        assert_eq!(slate.search_match_pill(), slate.accent.with_alpha(0x2E));
        // Blend(#7A8290, #E8EAEE, 0.35): 110 × 0.35 rounds to exactly 60.5, a tie, so
        // 122 + 60.5 goes to 160 (even) and the green and blue channels follow.
        assert_eq!(slate.search_placeholder().to_hex(), "#A0A6B1");
        // Blend(#7A8290, #E8EAEE, 0.55): the same 60.5 tie, i.e. 182 — 0xB6, not the
        // 0xB7 an `f32` amount produces. The f32 spelling is 0.5500000119, which is
        // 182.50000131 before rounding.
        assert_eq!(slate.search_subtle_text().to_hex(), "#B6BBC4");
        // A palette where the tie that bites is the placeholder's: Digital Horizon's dim
        // green is 127, 110 × 0.35 is again 60.5, and 127 + 60.5 goes to the even 166.
        let horizon = Theme::by_name("Digital Horizon");
        assert_eq!(horizon.search_placeholder().to_hex(), "#B1A6C3");
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

    /// One `#`, like `TryParseHex` (`MainWindow.xaml.cs:1233-1234`).
    ///
    /// The dialogs' hex entries hand user text straight to this parser, so a second `#` is
    /// the typo the managed side refuses; repairing it would make the same text a colour
    /// here and nothing there.
    #[test]
    fn exactly_one_leading_hash_is_stripped() {
        assert_eq!(Color::parse_hex("#C7625A"), Some(Color::from_hex(0xC7625A)));
        assert_eq!(Color::parse_hex("C7625A"), Some(Color::from_hex(0xC7625A)));
        assert_eq!(
            Color::parse_hex("  #C7625A  "),
            Some(Color::from_hex(0xC7625A))
        );
        assert_eq!(
            Color::parse_hex("#9C020711"),
            Some(Color::rgba(0x02, 0x07, 0x11, 0x9C)),
            "the 8-digit ARGB form the overlay literals use"
        );

        assert_eq!(
            Color::parse_hex("##C7625A"),
            None,
            "the second # is a typo, not something to repair"
        );
        assert_eq!(Color::parse_hex("##9C020711"), None);
        assert_eq!(Color::parse_hex("###C7625A"), None);

        // The 3-digit shorthand: `TryParseHex` doubles each digit (`MainWindow.xaml.cs:1237-1241`)
        // and `ColorConverter` accepts it, so a hand-edited role of `#abc` paints the same
        // colour on both hosts instead of falling back to a derived shade here.
        assert_eq!(Color::parse_hex("#abc"), Some(Color::from_hex(0xAA_BB_CC)));
        assert_eq!(Color::parse_hex("abc"), Some(Color::from_hex(0xAA_BB_CC)));
        assert_eq!(
            Color::parse_hex("  #fBd  "),
            Some(Color::from_hex(0xFF_BB_DD)),
            "shorthand is case-insensitive and trims like the long form"
        );
        assert_eq!(
            Color::parse_hex("#abc"),
            Color::parse_hex("#AABBCC"),
            "expanding shorthand is the same colour as spelling it out"
        );
        assert_eq!(Color::parse_hex("##abc"), None, "one #, as above");
        assert_eq!(
            Color::parse_hex("#abcd"),
            None,
            "the 4-digit ARGB form is not accepted"
        );
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

    /// The palette-entry form: `TryParseHex`'s three or six digits, and never the alpha one.
    ///
    /// The difference from [`Color::parse_hex`] is the digit count and nothing else, which is
    /// why the pair is asserted here: the permissive form is what the overlay literals use, and
    /// a palette role spelled with an alpha channel is text the managed `ParseHexOr` refuses.
    #[test]
    fn the_palette_entry_parser_refuses_the_argb_form() {
        let peach = Color::from_hex(0xC7625A);
        let teal = Color::from_hex(0xAA_BB_CC);

        assert_eq!(Color::parse_rgb_hex("#C7625A"), Some(peach));
        assert_eq!(Color::parse_rgb_hex("c7625a"), Some(peach));
        assert_eq!(Color::parse_rgb_hex("  #abc  "), Some(teal));

        // `TryParseHex` has no alpha digits: eight of them is not a palette entry, opaque
        // `#FFAABBCC` included.
        assert_eq!(Color::parse_rgb_hex("#9C020711"), None);
        assert_eq!(Color::parse_rgb_hex("9C020711"), None);
        assert_eq!(Color::parse_rgb_hex("#FFAABBCC"), None);
        // One `#`, three or six digits, and nothing else.
        assert_eq!(Color::parse_rgb_hex("##C7625A"), None);
        assert_eq!(Color::parse_rgb_hex("#12345"), None);
        assert_eq!(Color::parse_rgb_hex("#abcd"), None);
        assert_eq!(Color::parse_rgb_hex("#"), None);
        assert_eq!(Color::parse_rgb_hex(""), None);

        // Where the two agree, and where the permissive one goes further.
        for text in ["#C7625A", "abc", "  #0A0B0F "] {
            assert_eq!(Color::parse_rgb_hex(text), Color::parse_hex(text));
        }
        let scrim = Color::rgba(0x02, 0x07, 0x11, 0x9C);
        assert_eq!(Color::parse_hex("#9C020711"), Some(scrim));
    }

    /// An `#AARRGGBB` role is invalid to `RegisterPalette`, so it takes its fallback.
    ///
    /// `ParseHexOr` (`MainWindow.xaml.cs:1009-1017`, `:1254-1255`) is the whole story: a role
    /// that is not three or six digits is *absent* as far as the theme is concerned, so the
    /// fallback chain decides the colour — the background for a refused `panel`, the derived
    /// blend for a refused `dim_text`, `Colors.Gray` for a refused `accent`. Reading the alpha
    /// instead would paint a translucent role the managed window never shows.
    #[test]
    fn from_custom_refuses_the_argb_form_like_try_parse_hex() {
        let theme = Theme::from_custom(
            "Alpha",
            CustomRoles {
                background: "#010203",
                panel: "#80050607",             // ARGB -> the background
                slider_background: "#400A0B0C", // ARGB -> Lighten(background, 0.06)
                slider_foreground: "#0A0B0C",   // six digits -> taken
                accent: "#80FF8800",            // ARGB -> Colors.Gray
                text: "#FFFFFF",
                dim_text: "#80112233",    // ARGB -> Blend(text, background, 0.45)
                tab_selected: "#400A0B0C", // ARGB -> Lighten(background, 0.10)
                tab_hover: "#400A0B0C",    // ARGB -> Lighten(background, 0.04)
            },
        );

        let background = Color::from_hex(0x010203);
        assert_eq!(theme.panel, background, "an ARGB panel is the background");
        assert_eq!(theme.slider_background, background.lighten(0.06));
        assert_eq!(
            theme.slider_foreground,
            Color::from_hex(0x0A0B0C),
            "six digits are still a colour"
        );
        assert_eq!(theme.accent, Color::from_hex(0x808080), "Colors.Gray");
        assert_eq!(
            theme.dim_text,
            Color::from_hex(0xFFFFFF).blend(background, 0.45)
        );
        assert_eq!(theme.tab_selected, background.lighten(0.10));
        assert_eq!(theme.tab_hover, background.lighten(0.04));
        // And no role can come out of it translucent, which is the defect this closes.
        for (role, color) in [
            ("background", theme.background),
            ("panel", theme.panel),
            ("slider background", theme.slider_background),
            ("slider foreground", theme.slider_foreground),
            ("accent", theme.accent),
            ("text", theme.text),
            ("dim text", theme.dim_text),
            ("tab selected", theme.tab_selected),
            ("tab hover", theme.tab_hover),
        ] {
            assert_eq!(color.a, 0xFF, "the {role} role is translucent");
        }
    }

    // Ports of `Aoko.Tests/PaletteContrastTests.cs`, run against all 18 palettes
    // rather than only the live one: the claims are palette-independent in .NET
    // and must stay that way here.
    #[test]
    fn chip_labels_pass_aa_on_their_tint_layers() {
        // The amounts come from their one home rather than being repeated here, so this is a
        // claim about the layer [`crate::components::chip`] actually flattens — the C#
        // `TabChipTint` at 0.16 over the panel, and the critical tone's 0.20.
        for theme in builtins() {
            for (tone, opacity) in [
                (theme.accent, metrics::wash::CHIP_TINT),
                (fixed::CRITICAL, metrics::wash::CHIP_TINT_CRITICAL),
                (fixed::WARN, metrics::wash::CHIP_TINT_WARN),
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
