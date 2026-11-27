//! Matched-run highlighting for the command search rows.
//!
//! A near one-to-one port of `Aoko/Views/HighlightTextBlock.cs`, which rebuilds
//! `Inlines` and **ignores out-of-range ranges**. GPUI's equivalent is
//! `StyledText::with_highlights(Vec<(Range<usize>, HighlightStyle)>)`
//! (`gpui/src/elements/text.rs:409`), whose ranges are **byte** offsets — so the same
//! discipline the C# control applies (never split a character) has to be applied
//! here, and [`sanitize_ranges`] is that discipline as a tested function.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{div, px, AnyElement, HighlightStyle, IntoElement, SharedString, StyledText};

use crate::theme::Theme;

/// Drops ranges that cannot be painted: inverted, empty, past the end, or inside a
/// multi-byte character.
///
/// Returns ranges in ascending order, which is what `StyledText` expects for byte
/// offsets. Overlaps are left alone rather than merged: a highlight is a style layer,
/// and merging would silently change which tint wins.
#[must_use]
pub fn sanitize_ranges(text: &str, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut kept: Vec<Range<usize>> = ranges
        .iter()
        .filter(|range| {
            range.start < range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end)
        })
        .cloned()
        .collect();
    kept.sort_by_key(|range| (range.start, range.end));
    kept
}

/// Text with matched runs tinted and pilled, the way `CommandSearchMatchBrush` and
/// `CommandSearchMatchPillBrush` do it (`MainWindow.xaml.cs:1336-1337`).
pub fn highlight_text(
    theme: &Theme,
    text: impl Into<SharedString>,
    ranges: &[Range<usize>],
    font_size: f32,
) -> AnyElement {
    let text: SharedString = text.into();
    let usable = sanitize_ranges(&text, ranges);
    let style = HighlightStyle {
        color: Some(theme.search_match().into()),
        background_color: Some(theme.search_match_pill().into()),
        ..Default::default()
    };

    div()
        .text_size(px(font_size))
        .text_color(theme.text)
        .child(
            StyledText::new(text).with_highlights(usable.into_iter().map(|range| (range, style))),
        )
        .into_any_element()
}

#[cfg(test)]
// `single_range_in_vec_init` fires on every one-element range list here. That shape is
// the subject of the tests — which single ranges a sanitiser must reject — so silencing
// it is more honest than writing a helper that hides the data.
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    #[test]
    fn ascii_ranges_pass_through_in_order() {
        let text = "Auto Clicker";
        let ranges = [5..12, 0..4];
        assert_eq!(sanitize_ranges(text, &ranges), vec![0..4, 5..12]);
    }

    #[test]
    fn out_of_range_ranges_are_ignored_like_the_csharp_control() {
        let text = "BlockEsp";
        // Built through `vec!` and a variable start so the "single range in an array" and
        // "reversed empty range" lints do not fire on what is deliberate test data.
        assert_eq!(sanitize_ranges(text, &[0..20]), Vec::<Range<usize>>::new());
        assert_eq!(sanitize_ranges(text, &[5..5]), Vec::<Range<usize>>::new());
        let (start, end) = (7_usize, 3_usize);
        assert_eq!(
            sanitize_ranges(text, &[start..end]),
            Vec::<Range<usize>>::new()
        );
        assert_eq!(sanitize_ranges("", &[0..1]), Vec::<Range<usize>>::new());
    }

    #[test]
    fn multi_byte_characters_are_never_split() {
        // "Pinging…" — the ellipsis is three bytes, and the C# copy rule keeps the
        // character. A byte range that lands inside it must be dropped, not painted.
        let text = "Pinging…";
        let ellipsis_start = text.len() - 3;
        assert!(text.is_char_boundary(ellipsis_start));

        // Inside the ellipsis: dropped.
        assert_eq!(
            sanitize_ranges(text, &[ellipsis_start + 1..text.len()]),
            Vec::<Range<usize>>::new()
        );
        // Covering it whole: kept.
        assert_eq!(
            sanitize_ranges(text, &[ellipsis_start..text.len()]),
            vec![ellipsis_start..text.len()]
        );
    }

    #[test]
    fn accents_and_emoji_are_handled_by_byte_offsets() {
        let text = "Café ✓";
        let check = text.find('✓').expect("glyph present");
        assert_eq!(
            sanitize_ranges(text, &[check..check + '✓'.len_utf8()]),
            vec![check..check + 3]
        );
        // A range ending mid-glyph is dropped.
        assert_eq!(
            sanitize_ranges(text, &[check..check + 1]),
            Vec::<Range<usize>>::new()
        );
    }
}
