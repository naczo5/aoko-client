//! Status chips (`TabChip` / `TabChipTint` / `TabChipText`, `TabStyles.xaml:199-218`).
//!
//! A 4 px-radius pill with a 1 px `SliderFgBrush` edge and a 3 px-radius tint layer
//! at a fixed opacity, with the label at full strength over it. The tint opacities
//! are the ones `Aoko.Tests/PaletteContrastTests` checks: text on the tint must clear
//! AA, which is a claim about the label and not about the tone.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, IntoElement, SharedString};

use crate::theme::{fixed, metrics, Color, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipTone {
    /// No status: the panel's own hairline colour, label in text.
    Neutral,
    /// Accent tint, used for latency and source chips.
    Accent,
    /// Fixed critical tone (`CriticalBrush`).
    Critical,
    /// Fixed warning tone (`WarnBrush`).
    Warn,
}

impl ChipTone {
    /// The tint colour before opacity.
    #[must_use]
    pub fn tone(self, theme: &Theme) -> Color {
        match self {
            Self::Neutral => theme.slider_foreground,
            Self::Accent => theme.accent,
            Self::Critical => fixed::CRITICAL,
            Self::Warn => fixed::WARN,
        }
    }

    /// The tint layer's opacity, from `TabStyles.xaml:208` and the contrast test's
    /// 0.20 for the critical tone.
    ///
    /// `f64` because [`crate::theme::metrics::wash`] holds the managed `double`s: the blend
    /// that flattens the layer rounds a half to even, and an `f32` amount lands on the wrong
    /// side of one.
    #[must_use]
    pub fn tint_opacity(self) -> f64 {
        match self {
            Self::Neutral | Self::Accent => metrics::wash::CHIP_TINT,
            Self::Critical => metrics::wash::CHIP_TINT_CRITICAL,
            Self::Warn => metrics::wash::CHIP_TINT_WARN,
        }
    }

    /// The resolved background: the tint flattened over the panel, because a chip
    /// sits on a card and the C# achieves the layer with an `Opacity` on a `Border`.
    #[must_use]
    pub fn tint(self, theme: &Theme) -> Color {
        theme.panel.blend(self.tone(theme), self.tint_opacity())
    }

    /// The label colour: `TextBrush` for every tone (`TabChipText`).
    #[must_use]
    pub fn label(self, theme: &Theme) -> Color {
        theme.text
    }
}

pub fn chip(theme: &Theme, tone: ChipTone, label: impl Into<SharedString>) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .rounded(px(metrics::radius::CHIP))
        .border_1()
        .border_color(theme.slider_foreground)
        .bg(tone.tint(theme))
        .px(px(7.0))
        .py(px(2.0))
        .text_size(px(metrics::text::CHIP))
        .text_color(tone.label(theme))
        .child(label.into())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tone_clears_aa_on_its_tint_for_every_palette() {
        for name in Theme::builtin_names() {
            let theme = Theme::by_name(name);
            for tone in [
                ChipTone::Neutral,
                ChipTone::Accent,
                ChipTone::Critical,
                ChipTone::Warn,
            ] {
                let ratio = tone.label(&theme).contrast_ratio(tone.tint(&theme));
                assert!(ratio >= 4.5, "{name}/{tone:?}: chip label is {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn tones_are_distinguishable() {
        let theme = Theme::slate();
        let tints = [
            ChipTone::Neutral.tint(&theme),
            ChipTone::Accent.tint(&theme),
            ChipTone::Critical.tint(&theme),
            ChipTone::Warn.tint(&theme),
        ];
        for (index, tint) in tints.iter().enumerate() {
            for other in tints.iter().skip(index + 1) {
                assert_ne!(tint, other, "two chip tones resolve to the same tint");
            }
        }
    }
}
