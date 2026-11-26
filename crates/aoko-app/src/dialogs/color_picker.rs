//! HSV colour picker state.
//!
//! Ported from `Aoko/ColorPickerDialog.xaml.cs`. The managed dialog is a separate
//! `Window`; this is the state it held, with the widget removed:
//!
//! * HSV is the source of truth for the saturation/value square and the hue bar.
//! * The RGB sliders are the one place a *byte* value is authoritative, because a
//!   round trip through HSV cannot reproduce every byte exactly. The managed
//!   `_updating` flag is what stopped that round trip recursing; here it is a
//!   borrow-free `settling` flag with the same job.
//! * An unparsable hex entry reverts to the last valid colour instead of being
//!   swallowed.
//!
//! The painter (a per-row quad fill) is not modelled: `Background` offers solid and
//! two-stop linear only at the pin, so the saturation/value square is custom paint
//! in the view and this module only supplies the numbers it paints from.

use crate::theme::Color;

/// Width of the saturation/value square, from `SvWidth`.
pub const SV_WIDTH: f32 = 220.0;
/// Height of the saturation/value square, from `SvHeight`.
pub const SV_HEIGHT: f32 = 180.0;
/// Height of the hue bar, from `HueHeight`.
pub const HUE_HEIGHT: f32 = 180.0;

/// An RGB triple, one byte per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// A colour from its three channels.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `#RRGGBB`, upper case, which is what the hex field showed.
    #[must_use]
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

impl From<Color> for Rgb {
    /// The picker has no alpha channel, so a [`Color`] maps to its three channels. Kept here
    /// rather than in `theme` because it exists for this dialog's delegation and nowhere else.
    fn from(color: Color) -> Self {
        Self::new(color.r, color.g, color.b)
    }
}

/// Hue in degrees (`0..360`), saturation and value in `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Hsv {
    pub hue: f64,
    pub sat: f64,
    pub val: f64,
}

/// `HsvToRgb`, round-half-away-from-zero like `Math.Round`.
#[must_use]
pub fn hsv_to_rgb(hue: f64, sat: f64, val: f64) -> Rgb {
    let hue = ((hue % 360.0) + 360.0) % 360.0;
    let sat = sat.clamp(0.0, 1.0);
    let val = val.clamp(0.0, 1.0);

    let chroma = val * sat;
    let x = chroma * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = val - chroma;

    let (r, g, b) = if hue < 60.0 {
        (chroma, x, 0.0)
    } else if hue < 120.0 {
        (x, chroma, 0.0)
    } else if hue < 180.0 {
        (0.0, chroma, x)
    } else if hue < 240.0 {
        (0.0, x, chroma)
    } else if hue < 300.0 {
        (x, 0.0, chroma)
    } else {
        (chroma, 0.0, x)
    };

    Rgb::new(
        clamp_channel((r + m) * 255.0),
        clamp_channel((g + m) * 255.0),
        clamp_channel((b + m) * 255.0),
    )
}

/// `RgbToHsv`.
#[must_use]
pub fn rgb_to_hsv(rgb: Rgb) -> Hsv {
    let r = f64::from(rgb.r) / 255.0;
    let g = f64::from(rgb.g) / 255.0;
    let b = f64::from(rgb.b) / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let val = max;
    let sat = if max <= 0.0 { 0.0 } else { delta / max };

    if delta <= 0.0 {
        return Hsv { hue: 0.0, sat, val };
    }

    let mut hue = if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };

    if hue < 0.0 {
        hue += 360.0;
    }

    Hsv { hue, sat, val }
}

/// `TryParseHex`: `#RGB`, `#RRGGBB`, with or without the hash, any case.
///
/// The rule is [`Color::parse_hex`], so this module expands nothing of its own and a change
/// to the tolerance cannot land on the picker's path alone. What stays here is the refusal of
/// the 8-digit ARGB form: the hex field writes back `#RRGGBB`, and `TryParseHex` refuses
/// anything that is not six digits once the shorthand is expanded, so `#FFAABBCC` is not an
/// entry this box accepts however opaque its alpha is.
#[must_use]
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let trimmed = text.trim();
    let body = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if body.chars().count() == 8 {
        return None;
    }

    Color::parse_hex(trimmed).map(Rgb::from)
}

/// The outcome of pressing OK or Cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickResult {
    /// The colour the caller should store.
    pub color: Rgb,
    /// Whether the user accepted (`true`) or cancelled (`false`).
    pub accepted: bool,
}

/// The picker's state.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorPickerState {
    hsv: Hsv,
    selected: Rgb,
    initial: Rgb,
    hex_text: String,
    /// Set while the state itself is writing the derived values, so a change
    /// handler cannot feed a rounded value back in. The managed `_updating`.
    settling: bool,
    dragging_sv: bool,
    dragging_hue: bool,
}

impl ColorPickerState {
    /// A picker opened on `initial`.
    #[must_use]
    pub fn new(initial: Rgb) -> Self {
        let mut state = Self {
            hsv: Hsv::default(),
            selected: initial,
            initial,
            hex_text: String::new(),
            settling: false,
            dragging_sv: false,
            dragging_hue: false,
        };

        state.load(initial);
        state
    }

    /// The colour the preview swatch and the hex field show.
    #[must_use]
    pub fn selected(&self) -> Rgb {
        self.selected
    }

    /// The HSV the square and the hue bar are drawn from.
    #[must_use]
    pub fn hsv(&self) -> Hsv {
        self.hsv
    }

    /// The hex field's text, which only commits on Enter or focus loss.
    #[must_use]
    pub fn hex_text(&self) -> &str {
        &self.hex_text
    }

    /// Whether either canvas is being dragged.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.dragging_sv || self.dragging_hue
    }

    /// Whether the state is mid-update; a painter can skip work while it is set.
    #[must_use]
    pub fn is_settling(&self) -> bool {
        self.settling
    }

    /// Replaces the colour wholesale, as `SetColor` does.
    pub fn set_color(&mut self, color: Rgb) {
        self.load(color);
    }

    /// Types into the hex field.
    pub fn set_hex_text(&mut self, text: &str) {
        self.hex_text = text.to_owned();
    }

    /// `CommitHex`: adopts a parsable entry, otherwise reverts.
    ///
    /// Returns whether the entry was accepted, so the view can say so if it wants
    /// to; the revert already happened either way.
    pub fn commit_hex(&mut self) -> bool {
        if self.settling {
            return false;
        }

        match parse_hex(&self.hex_text) {
            Some(color) => {
                self.load(color);
                true
            }
            None => {
                // Back to the last valid colour, which is `selected` itself.
                self.refresh_derived();
                false
            }
        }
    }

    /// A press or drag on the saturation/value square, in square-local pixels.
    pub fn drag_sv(&mut self, x: f32, y: f32) {
        self.dragging_sv = true;
        self.set_sv(x, y);
    }

    /// A drag update on the square.
    pub fn move_sv(&mut self, x: f32, y: f32) {
        if self.dragging_sv {
            self.set_sv(x, y);
        }
    }

    /// The square's drag ended.
    pub fn end_sv(&mut self) {
        self.dragging_sv = false;
    }

    /// A press or drag on the hue bar, in bar-local pixels.
    pub fn drag_hue(&mut self, y: f32) {
        self.dragging_hue = true;
        self.set_hue(y);
    }

    /// A drag update on the hue bar.
    pub fn move_hue(&mut self, y: f32) {
        if self.dragging_hue {
            self.set_hue(y);
        }
    }

    /// The hue bar's drag ended.
    pub fn end_hue(&mut self) {
        self.dragging_hue = false;
    }

    /// One of the three RGB sliders moved.
    ///
    /// The channel is written as a byte and the HSV is re-derived from it, which
    /// is what keeps the slider at the value the user set instead of at whatever
    /// HSV round-trips back to.
    pub fn set_channel(&mut self, channel: Channel, value: u8) {
        if self.settling {
            return;
        }

        let mut color = self.selected;
        match channel {
            Channel::Red => color.r = value,
            Channel::Green => color.g = value,
            Channel::Blue => color.b = value,
        }

        self.hsv = rgb_to_hsv(color);
        self.selected = color;
        self.refresh_derived();
    }

    /// OK.
    #[must_use]
    pub fn accept(&self) -> PickResult {
        PickResult {
            color: self.selected,
            accepted: true,
        }
    }

    /// Cancel: the caller keeps the colour it passed in.
    #[must_use]
    pub fn cancel(&self) -> PickResult {
        PickResult {
            color: self.initial,
            accepted: false,
        }
    }

    /// The thumb position inside the square, in square-local pixels.
    #[must_use]
    pub fn sv_thumb(&self) -> (f32, f32) {
        (
            (self.hsv.sat as f32) * SV_WIDTH,
            (1.0 - self.hsv.val as f32) * SV_HEIGHT,
        )
    }

    /// The thumb offset down the hue bar.
    #[must_use]
    pub fn hue_thumb(&self) -> f32 {
        (self.hsv.hue as f32 / 360.0) * HUE_HEIGHT
    }

    /// The pure hue the square's backdrop is tinted with, at full saturation and
    /// value.
    #[must_use]
    pub fn hue_color(&self) -> Rgb {
        hsv_to_rgb(self.hsv.hue, 1.0, 1.0)
    }

    fn load(&mut self, color: Rgb) {
        self.hsv = rgb_to_hsv(color);
        self.selected = color;
        self.refresh_derived();
    }

    /// Recomputes the colour from HSV and refreshes the derived fields.
    fn refresh_from_hsv(&mut self) {
        self.selected = hsv_to_rgb(self.hsv.hue, self.hsv.sat, self.hsv.val);
        self.refresh_derived();
    }

    /// The managed `RefreshAll`: the hex text is written from the colour, with the
    /// settling flag up so nothing reads it back while it is being written.
    fn refresh_derived(&mut self) {
        self.settling = true;
        self.hex_text = self.selected.to_hex();
        self.settling = false;
    }

    fn set_sv(&mut self, x: f32, y: f32) {
        self.hsv.sat = f64::from((x / SV_WIDTH).clamp(0.0, 1.0));
        self.hsv.val = f64::from((1.0 - y / SV_HEIGHT).clamp(0.0, 1.0));
        self.refresh_from_hsv();
    }

    fn set_hue(&mut self, y: f32) {
        self.hsv.hue = f64::from((y / HUE_HEIGHT).clamp(0.0, 1.0)) * 360.0;
        self.refresh_from_hsv();
    }
}

/// Which RGB slider moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Red,
    Green,
    Blue,
}

/// `Math.Round` on a `0..255` channel, saturating instead of wrapping.
fn clamp_channel(value: f64) -> u8 {
    let rounded = value.round();
    if rounded.is_nan() || rounded <= 0.0 {
        return 0;
    }

    if rounded >= 255.0 {
        return 255;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        rounded as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_primaries_round_trip_through_hsv() {
        for color in [
            Rgb::new(255, 0, 0),
            Rgb::new(0, 255, 0),
            Rgb::new(0, 0, 255),
            Rgb::new(255, 255, 255),
            Rgb::new(0, 0, 0),
            Rgb::new(0xBB, 0x37, 0x7D),
        ] {
            let hsv = rgb_to_hsv(color);
            assert_eq!(
                hsv_to_rgb(hsv.hue, hsv.sat, hsv.val),
                color,
                "{color:?} did not survive the round trip"
            );
        }
    }

    #[test]
    fn hue_degrees_match_the_managed_mapping() {
        assert!((rgb_to_hsv(Rgb::new(255, 0, 0)).hue - 0.0).abs() < 1e-9);
        assert!((rgb_to_hsv(Rgb::new(0, 255, 0)).hue - 120.0).abs() < 1e-9);
        assert!((rgb_to_hsv(Rgb::new(0, 0, 255)).hue - 240.0).abs() < 1e-9);
        assert!((rgb_to_hsv(Rgb::new(255, 0, 255)).hue - 300.0).abs() < 1e-9);

        // A grey has no hue; the managed code leaves it at zero rather than NaN.
        let grey = rgb_to_hsv(Rgb::new(128, 128, 128));
        assert_eq!(grey.hue, 0.0);
        assert_eq!(grey.sat, 0.0);
    }

    #[test]
    fn black_has_zero_saturation_rather_than_a_divide_by_zero() {
        let black = rgb_to_hsv(Rgb::new(0, 0, 0));
        assert_eq!(black.sat, 0.0);
        assert_eq!(black.val, 0.0);
    }

    #[test]
    fn hex_parsing_accepts_short_forms_and_rejects_junk() {
        assert_eq!(parse_hex("#BB377D"), Some(Rgb::new(0xBB, 0x37, 0x7D)));
        assert_eq!(parse_hex("bb377d"), Some(Rgb::new(0xBB, 0x37, 0x7D)));
        assert_eq!(parse_hex("#abc"), Some(Rgb::new(0xAA, 0xBB, 0xCC)));
        assert_eq!(parse_hex("  #FBD3E9  "), Some(Rgb::new(0xFB, 0xD3, 0xE9)));

        for text in [
            "",
            "   ",
            "#12345",
            "#1234567",
            "#gggggg",
            "rgb(1,2,3)",
            "\u{2014}",
        ] {
            assert_eq!(parse_hex(text), None, "accepted {text:?}");
        }
    }

    #[test]
    fn the_hex_box_agrees_with_the_theme_parser_except_on_argb() {
        for text in ["#abc", "#AABBCC", "abc", "  #BB377D  "] {
            assert_eq!(
                parse_hex(text),
                Color::parse_hex(text).map(Rgb::from),
                "{text:?} must parse the same through both"
            );
        }

        assert_eq!(parse_hex("#abc"), Some(Rgb::new(0xAA, 0xBB, 0xCC)));
        assert_eq!(parse_hex("#AABBCC"), Some(Rgb::new(0xAA, 0xBB, 0xCC)));
        assert_eq!(parse_hex("##abc"), None, "the second # is a typo here too");

        assert_eq!(
            parse_hex("#FF1A2B3C"),
            None,
            "the one difference: this box writes back #RRGGBB and has no alpha"
        );
        assert!(
            Color::parse_hex("#FF1A2B3C").is_some(),
            "which the theme parser does accept — the guard is the whole difference"
        );
    }

    #[test]
    fn the_hex_field_shows_the_selected_colour_in_upper_case() {
        let state = ColorPickerState::new(Rgb::new(0xBB, 0x37, 0x7D));
        assert_eq!(state.hex_text(), "#BB377D");
        assert_eq!(state.selected(), Rgb::new(0xBB, 0x37, 0x7D));
    }

    #[test]
    fn an_unparsable_hex_entry_reverts_instead_of_being_swallowed() {
        let mut state = ColorPickerState::new(Rgb::new(0xBB, 0x37, 0x7D));

        state.set_hex_text("#zzzzzz");
        assert!(!state.commit_hex());
        assert_eq!(state.selected(), Rgb::new(0xBB, 0x37, 0x7D));
        assert_eq!(
            state.hex_text(),
            "#BB377D",
            "the field shows the real value again"
        );
    }

    #[test]
    fn a_parsable_hex_entry_is_adopted() {
        let mut state = ColorPickerState::new(Rgb::new(0, 0, 0));

        state.set_hex_text("#0CE8C7");
        assert!(state.commit_hex());
        assert_eq!(state.selected(), Rgb::new(0x0C, 0xE8, 0xC7));
    }

    #[test]
    fn dragging_the_square_clamps_to_its_edges() {
        let mut state = ColorPickerState::new(Rgb::new(0xBB, 0x37, 0x7D));

        state.drag_sv(0.0, 0.0);
        assert_eq!(state.hsv().sat, 0.0);
        assert_eq!(state.hsv().val, 1.0);
        assert_eq!(state.selected(), Rgb::new(255, 255, 255));

        state.move_sv(SV_WIDTH * 2.0, SV_HEIGHT * 2.0);
        assert_eq!(state.hsv().sat, 1.0);
        assert_eq!(state.hsv().val, 0.0);
        assert_eq!(state.selected(), Rgb::new(0, 0, 0));

        state.end_sv();
        assert!(!state.is_dragging());

        // A move after the drag ended is ignored, as the managed handler checks.
        state.move_sv(SV_WIDTH / 2.0, 0.0);
        assert_eq!(state.selected(), Rgb::new(0, 0, 0));
    }

    #[test]
    fn dragging_the_hue_bar_sets_the_degree_range() {
        let mut state = ColorPickerState::new(Rgb::new(255, 0, 0));

        state.drag_hue(HUE_HEIGHT / 3.0);
        assert!((state.hsv().hue - 120.0).abs() < 1e-3);
        assert_eq!(
            state.selected(),
            Rgb::new(0, 255, 0),
            "120 degrees is pure green"
        );

        state.move_hue(HUE_HEIGHT);
        assert!((state.hsv().hue - 360.0).abs() < 1e-3);
        state.end_hue();
        assert!(!state.is_dragging());
    }

    #[test]
    fn the_square_thumb_tracks_saturation_and_value() {
        let mut state = ColorPickerState::new(Rgb::new(255, 0, 0));
        let (x, y) = state.sv_thumb();
        assert!((x - SV_WIDTH).abs() < 1e-3, "red is fully saturated");
        assert!(y.abs() < 1e-3, "red is fully bright");

        state.drag_sv(0.0, SV_HEIGHT);
        let (x, y) = state.sv_thumb();
        assert!(x.abs() < 1e-3);
        assert!((y - SV_HEIGHT).abs() < 1e-3);
    }

    #[test]
    fn an_rgb_slider_is_authoritative_over_the_hsv_round_trip() {
        let mut state = ColorPickerState::new(Rgb::new(0xBB, 0x37, 0x7D));

        state.set_channel(Channel::Green, 0x40);
        assert_eq!(
            state.selected(),
            Rgb::new(0xBB, 0x40, 0x7D),
            "the byte set stays set"
        );

        // Every one of the 256 values survives being set on its own channel.
        for value in 0..=255_u8 {
            state.set_channel(Channel::Blue, value);
            assert_eq!(state.selected().b, value);
        }
    }

    #[test]
    fn setting_a_channel_moves_the_hsv_the_square_draws() {
        let mut state = ColorPickerState::new(Rgb::new(0, 0, 0));
        assert_eq!(state.hsv().hue, 0.0);

        state.set_channel(Channel::Red, 0);
        state.set_channel(Channel::Green, 255);
        assert!((state.hsv().hue - 120.0).abs() < 1e-9);
        assert_eq!(state.hsv().sat, 1.0);
    }

    #[test]
    fn accept_and_cancel_report_the_right_colour() {
        let mut state = ColorPickerState::new(Rgb::new(0x11, 0x22, 0x33));
        state.set_hex_text("#010203");
        assert!(state.commit_hex());

        assert_eq!(
            state.accept(),
            PickResult {
                color: Rgb::new(1, 2, 3),
                accepted: true
            }
        );
        assert_eq!(
            state.cancel(),
            PickResult {
                color: Rgb::new(0x11, 0x22, 0x33),
                accepted: false
            },
            "cancel returns the colour the dialog opened on"
        );
    }

    #[test]
    fn the_hue_backdrop_is_the_pure_hue_not_the_selection() {
        let mut state = ColorPickerState::new(Rgb::new(0x55, 0x55, 0x55));
        state.drag_hue(HUE_HEIGHT / 2.0);

        assert_eq!(state.hue_color(), Rgb::new(0, 255, 255));
    }
}
