//! The Stats tab's pixels: `Aoko/MainWindow.xaml:1438-1521` over the runtime's live
//! [`StatsTracker`].
//!
//! # Mounted, and reading the one copy
//!
//! `shell.rs:1437-1448` is the Stats arm of the tab-body match, and it calls [`render`] with
//! `&self.runtime.stats()`. There is deliberately no `stats` field on `Shell`: the tracker
//! lives in [`crate::runtime::Runtime`], which the loop bodies write to through their click
//! sink and which `Runtime::sample_playtime` advances from the shell's per-frame tick, and
//! `Runtime::stats()` lends it as a `MutexGuard`. A field beside it would be the stale second
//! source this wiring exists to avoid.
//!
//! "Reset Stats" reports outward through `Shell::reset_stats` (`shell.rs:828`), which calls
//! `Runtime::reset_stats()`, repaints only when that cleared something, and leaves the next
//! frame to compose from the cleared tracker. This file keeps nothing between frames: every
//! number is read from the guard during `render`.
//!
//! Nothing here is waiting on a shell edit. The `TabBody::Stats` variant this needs is in
//! [`card_copy::TabBody`], and every exhaustive match over that enum already accounts for
//! it: the shell's render arm (`shell.rs:1438`), the two shell tests that skip the non-card
//! tabs, and the two coverage matches in [`crate::views::bindings`].
//!
//! # What is painted, and from where
//!
//! Every number is the tracker's, never a literal: [`StatsTexts::of`] is the single
//! place the six strings are composed, and it calls
//! `total_playtime_text`, `total_clicks`, `left_clicks`, `right_clicks`,
//! `average_cps_text`, `peak_cps_text` and `cps_sample_count` verbatim. The three
//! `Run`-composed lines in the markup (`L {left}  R {right}`, `Peak {peak}`,
//! `{n} samples`) are built from [`card_copy`]'s prefixes, so the spacing is the
//! table's and cannot drift.
//!
//! The histogram is the protocol's, too: the 49 buckets arrive with their label, count,
//! bar height and visibility already computed by `StatsTracker::refresh_cps_buckets`,
//! so this file decides where a bar goes and never how tall it is. The bar row is
//! bottom-aligned in a 124 px column, which is what makes a short bar sit on the axis,
//! and each column carries the markup's tooltip — the bucket's own `count_text()`
//! (`MainWindow.xaml:1488`).
//!
//! # Box rules, named rather than implied
//!
//! * **The chart box is `min_h(158)`, as the markup's `MinHeight="158"` is.** With no
//!   samples the box is the minimum and the centred "No samples yet" sits where the
//!   markup puts it; with samples the columns (124 px bar row plus a 5 px gap and a 9 px
//!   label) grow it past the minimum, so a label is never clipped. Every bar's height is
//!   the tracker's `bar_height()`, never a literal here.
//!
//! # Tests
//!
//! [`render`] has a GPUI window test: a probe view holds a populated tracker, is mounted
//! in a `TestAppContext` window, and the test asserts every part is laid out with a
//! non-empty box. GPUI at the pin exposes no readback of laid-out glyphs, so "the
//! visible text" is pinned in two halves that fail independently: [`StatsTexts::of`] is
//! asserted string-for-string against the tracker's own numbers (a wrong number fails
//! without a window), and each of those strings is drawn as the only child of the
//! element the geometry test finds (a wrong element fails without the model). What is
//! not proven — and cannot be, at the pin — is that a specific glyph was rasterised.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, App, ClickEvent, FontWeight, Window};

use aoko_protocol::stats::{StatsHistogramBucket, StatsTracker};

use crate::components::{self, ButtonVariant};
use crate::theme::metrics;
use crate::theme::Theme;
use crate::views::copy as card_copy;

/// The tab's copy that the tracker does not supply, verbatim from the markup.
///
/// The three captions and "No samples yet" are also in [`card_copy::STATS`] as `Note`s; the
/// drift test below asserts these spellings equal those, so the table stays the single source
/// and this module is the view's reading of it. The reset button's label is not here at all:
/// it is [`card_copy::STATS_RESET_LABEL`], which the table's `Button` and this view both read,
/// so there is nothing left to drift.
pub mod copy {
    /// `MainWindow.xaml:1444`.
    pub const PLAYTIME: &str = "Playtime";
    /// `:1450`.
    pub const CLICKS: &str = "Clicks";
    /// `:1459`.
    pub const AVERAGE: &str = "Avg CPS";
    /// `:1501`.
    pub const EMPTY: &str = "No samples yet";
}

/// The id of the histogram card in [`card_copy`], whose heading the table owns.
const HISTOGRAM_CARD: &str = "CpsHistogramCard";

/// The six strings the tab draws, composed once per frame from the tracker.
///
/// A plain struct rather than getters: the render pass reads each field exactly once,
/// and the tests can compare a whole frame's text against one value. `EQ` holds because
/// every field is a `String` the tracker formatted — no floats reach this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsTexts {
    /// `{Binding TotalPlaytimeText}` (`MainWindow.xaml:1445`), `h:mm:ss` or `mm:ss`.
    pub playtime: String,
    /// `{Binding TotalClicks}` (`:1451`).
    pub clicks: String,
    /// `L {left}  R {right}` (`:1453`).
    pub clicks_split: String,
    /// `{Binding AverageCpsText}` (`:1460`), which is `n/a` before the first sample.
    pub average: String,
    /// `Peak {PeakCpsText}` (`:1462`).
    pub peak: String,
    /// `{CpsSampleCount} samples` (`:1475`).
    pub samples: String,
}

impl StatsTexts {
    /// Reads every number out of `tracker`.
    #[must_use]
    pub fn of(tracker: &StatsTracker) -> Self {
        let left = tracker.left_clicks();
        let right = tracker.right_clicks();
        Self {
            playtime: tracker.total_playtime_text(),
            clicks: tracker.total_clicks().to_string(),
            // `MainWindow.xaml:1453`: `<Run Text="L "/><Run LeftClicks/><Run Text="  R "/>`
            // `<Run RightClicks/>`, with no space between a prefix and its number.
            clicks_split: format!(
                "{}{left}{}{right}",
                card_copy::STATS_LEFT_PREFIX,
                card_copy::STATS_RIGHT_PREFIX
            ),
            average: tracker.average_cps_text(),
            peak: format!(
                "{}{}",
                card_copy::STATS_PEAK_PREFIX,
                tracker.peak_cps_text()
            ),
            samples: format!(
                "{}{}",
                tracker.cps_sample_count(),
                card_copy::STATS_SAMPLES_SUFFIX
            ),
        }
    }
}

/// The Stats tab body: a scrolling panel, the three summary boxes and the Reset button,
/// then the histogram.
///
/// `tab_index` is only the scroll container's element id, the same
/// `reuse::indexed("tab-scroll", ..)` the Accounts and Servers bodies use, so two tabs
/// scrolled in the same frame cannot share scroll state. `on_reset` is the "Reset Stats"
/// press; the managed handler (`ResetStatsButton_Click`) calls `StatsTracker.Reset`, and
/// this view deliberately does not own the tracker, so it cannot reset it itself.
pub fn render(
    tracker: &StatsTracker,
    theme: &Theme,
    tab_index: usize,
    on_reset: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let texts = StatsTexts::of(tracker);

    components::scroll(crate::reuse::indexed("tab-scroll", tab_index))
        .flex_col()
        .p(px(12.0))
        // `MainWindow.xaml:1441`: the wrap panel carries `Margin="0,0,0,4"` under it.
        .gap(px(4.0))
        .child(summary_row(&texts, theme, on_reset))
        .child(histogram(tracker, &texts, theme))
        .into_any_element()
}

/// The wrap panel of three summary boxes and the Reset button (`:1441-1468`).
fn summary_row(
    texts: &StatsTexts,
    theme: &Theme,
    on_reset: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_start()
        .gap(px(metrics::stats::SUMMARY_GAP))
        .child(summary_card(
            theme,
            copy::PLAYTIME,
            "stats-playtime",
            texts.playtime.clone(),
            None,
        ))
        .child(summary_card(
            theme,
            copy::CLICKS,
            "stats-clicks",
            texts.clicks.clone(),
            Some(("stats-clicks-split", texts.clicks_split.clone())),
        ))
        .child(summary_card(
            theme,
            copy::AVERAGE,
            "stats-average",
            texts.average.clone(),
            Some(("stats-peak", texts.peak.clone())),
        ))
        .child(reset_button(theme, on_reset))
        .into_any_element()
}

/// One 150 px `ControlCardStyle` border: caption, 20 px semibold value, optional sub-line.
///
/// The three lines go in a column of their own so the gaps are the markup's `4` and `2`
/// (`:1445`, `:1452`) rather than the card component's own `gap(6)`.
fn summary_card(
    theme: &Theme,
    caption: &'static str,
    value_selector: &'static str,
    value: String,
    sub: Option<(&'static str, String)>,
) -> AnyElement {
    let mut stack = div()
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(caption),
        )
        .child(
            div()
                .debug_selector(move || String::from(value_selector))
                .mt(px(metrics::stats::VALUE_GAP))
                .text_size(px(metrics::stats::VALUE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(value),
        );

    if let Some((selector, text)) = sub {
        stack = stack.child(
            div()
                .debug_selector(move || String::from(selector))
                .mt(px(metrics::stats::SUBLINE_GAP))
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(text),
        );
    }

    components::card(theme)
        .w(px(metrics::stats::SUMMARY_CARD))
        .child(stack)
        .into_any_element()
}

/// "Reset Stats": a [`ButtonVariant::Dark`] button whose height is the markup's 40 px.
///
/// `MainWindow.xaml:1466-1467` sets `Padding="14,6" Height="40"` and no `Width`, so the
/// height is pinned and the width stays the label's. The debug selector is on a wrapper
/// that is exactly the button's box, so a test click at its centre lands on the button.
fn reset_button(
    theme: &Theme,
    on_reset: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .flex()
        .debug_selector(|| String::from("stats-reset"))
        .child(components::button_pinned(
            "stats-reset",
            theme,
            ButtonVariant::Dark,
            card_copy::STATS_RESET_LABEL,
            None,
            Some(metrics::stats::RESET_HEIGHT),
            on_reset,
        ))
        .into_any_element()
}

/// The histogram card: heading, sample count, and the bars or the empty state
/// (`:1470-1517`).
fn histogram(tracker: &StatsTracker, texts: &StatsTexts, theme: &Theme) -> AnyElement {
    let mut header = div().flex().flex_row().items_center().justify_between();

    // `:1473`: the heading is the copy table's, so the two spellings cannot drift. A card
    // without one paints none, which is the rule `card_copy::heading` exists to keep.
    if let Some(heading) = card_copy::heading(HISTOGRAM_CARD) {
        header = header.child(
            div()
                .text_size(px(metrics::stats::HEADING))
                .font_weight(FontWeight::BOLD)
                .text_color(theme.text)
                .child(heading),
        );
    }

    header = header.child(
        div()
            .debug_selector(|| String::from("stats-samples"))
            .text_size(px(metrics::text::CAPTION))
            .text_color(theme.dim_text)
            .child(texts.samples.clone()),
    );

    components::card(theme)
        .child(
            div()
                .flex()
                .flex_col()
                // `:1472`: `DockPanel Margin="0,0,0,10"`, set here instead of on the
                // header so the card's own 6 px gap does not add to it.
                .gap(px(metrics::stats::HEADER_GAP))
                .child(header)
                .child(chart(tracker, theme)),
        )
        .into_any_element()
}

/// The `SliderBgBrush` box the bars sit in (`:1478-1515`).
///
/// `MinHeight="158"` is a floor, not a height: with no samples the centred empty text is
/// what the floor is for, and with samples the columns are taller than it, so the box
/// grows rather than clipping a label.
fn chart(tracker: &StatsTracker, theme: &Theme) -> AnyElement {
    let panel = div()
        .debug_selector(|| String::from("stats-chart"))
        .rounded(px(metrics::stats::CHART_RADIUS))
        .bg(theme.slider_background)
        .p(px(metrics::stats::CHART_PADDING))
        .min_h(px(metrics::stats::CHART_MIN_HEIGHT));

    if !tracker.has_cps_samples() {
        return panel
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .debug_selector(|| String::from("stats-empty"))
                    .text_size(px(metrics::stats::EMPTY))
                    .text_color(theme.dim_text)
                    .child(copy::EMPTY),
            )
            .into_any_element();
    }

    let mut bars = div()
        .flex()
        .flex_row()
        .items_end()
        .justify_center()
        .debug_selector(|| String::from("stats-bars"));

    // `:1488` `Visibility="{Binding Visibility}"`: the tracker keeps a two-bucket pad
    // around the populated range so the axis does not jump on every click.
    for (index, bucket) in tracker
        .buckets()
        .iter()
        .enumerate()
        .filter(|(_, bucket)| bucket.visible())
    {
        bars = bars.child(bucket_column(index, bucket, theme));
    }

    panel.child(bars).into_any_element()
}

/// One column: a bottom-aligned bar in the 124 px row, then its label (`:1488-1496`).
fn bucket_column(index: usize, bucket: &StatsHistogramBucket, theme: &Theme) -> AnyElement {
    let count = crate::reuse::by_number(
        "stats-bucket-count",
        u64::try_from(bucket.count()).unwrap_or(0),
        || bucket.count_text(),
    );
    let label = crate::reuse::by_number("stats-bucket-label", index as u64, || {
        bucket.label().to_owned()
    });

    div()
        .id(crate::reuse::indexed("stats-bucket", index))
        .mx(px(metrics::stats::BUCKET_GAP))
        .flex()
        .flex_col()
        .items_center()
        .tooltip(components::tooltip(theme, count))
        .child(
            div()
                .h(px(metrics::stats::BAR_AREA))
                .flex()
                .flex_col()
                .justify_end()
                .child(
                    div()
                        .w(px(metrics::stats::BAR_WIDTH))
                        .h(px(bucket.bar_height() as f32))
                        .rounded(px(metrics::stats::BAR_RADIUS))
                        .bg(theme.accent),
                ),
        )
        .child(
            div()
                .mt(px(metrics::stats::LABEL_GAP))
                .text_size(px(metrics::stats::LABEL))
                .text_color(theme.dim_text)
                .child(label),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use gpui::{Modifiers, Render, TestAppContext, VisualTestContext, WindowOptions};

    use super::*;
    use aoko_protocol::state::GameState;

    /// 3 clicks across two buckets and 62 seconds of playtime.
    fn populated() -> StatsTracker {
        let mut tracker = StatsTracker::new();
        tracker.record_click(2.0, true);
        tracker.record_click(4.0, false);
        tracker.record_click(4.0, false);
        let state = GameState::default();
        for _ in 0..62 {
            tracker.record_playtime_sample(true, false, &state, false);
        }
        tracker
    }

    /// The `Note` a Stats card opens with, out of the copy table.
    fn table_note(card_id: &str) -> Option<&'static str> {
        card_copy::STATS
            .iter()
            .find(|card| card.id == card_id)
            .and_then(|card| card.controls.first())
            .and_then(|control| match control {
                card_copy::Control::Note { text } => Some(*text),
                _ => None,
            })
    }

    #[test]
    fn the_texts_are_the_trackers_own_numbers() {
        let texts = StatsTexts::of(&populated());

        assert_eq!(texts.playtime, "01:02", "62 seconds is mm:ss below an hour");
        assert_eq!(texts.clicks, "3");
        assert_eq!(texts.clicks_split, "L 1  R 2");
        // (2.0 + 4.0 + 4.0) / 3 = 3.333.., formatted the protocol's way.
        assert_eq!(texts.average, "3.3");
        assert_eq!(texts.peak, "Peak 4.0");
        assert_eq!(texts.samples, "3 samples");
    }

    #[test]
    fn an_empty_tracker_says_so_instead_of_zeroes_it_does_not_have() {
        let texts = StatsTexts::of(&StatsTracker::new());

        assert_eq!(texts.playtime, "00:00");
        assert_eq!(texts.clicks, "0");
        assert_eq!(texts.clicks_split, "L 0  R 0");
        // `AverageCpsText` and `PeakCpsText` are `n/a` with no samples; the view paints
        // them rather than inventing `0.0`, which would read as a measured rate.
        assert_eq!(texts.average, "n/a");
        assert_eq!(texts.peak, "Peak n/a");
        assert_eq!(texts.samples, "0 samples");
    }

    #[test]
    fn the_view_copy_is_the_copy_tables_copy() {
        assert_eq!(table_note("PlaytimeSummaryCard"), Some(copy::PLAYTIME));
        assert_eq!(table_note("ClicksSummaryCard"), Some(copy::CLICKS));
        assert_eq!(table_note("AverageCpsSummaryCard"), Some(copy::AVERAGE));
        assert_eq!(table_note("CpsHistogramCard"), Some(copy::EMPTY));
        assert_eq!(
            card_copy::heading(HISTOGRAM_CARD),
            Some("CPS Distribution"),
            "the heading is the table's, so the view must read it there"
        );
        // The action row is the one card this view paints from the table's own control: the
        // label is a constant the table and the painter share, so the assertion is that the
        // card still carries it rather than that two spellings agree.
        let reset = card_copy::STATS
            .iter()
            .find(|card| card.id == "StatsActionsCard")
            .and_then(|card| card.controls.first())
            .and_then(|control| match control {
                card_copy::Control::Button { label, .. } => Some(*label),
                _ => None,
            });
        assert_eq!(reset, Some(card_copy::STATS_RESET_LABEL));
    }

    #[test]
    fn the_tracker_keeps_a_padded_window_of_buckets() {
        let tracker = populated();
        let visible: Vec<&str> = tracker
            .buckets()
            .iter()
            .filter(|bucket| bucket.visible())
            .map(StatsHistogramBucket::label)
            .collect();

        // 2.0 is bucket 2 and 4.0 is bucket 6; the window pads two on each side and the
        // low side saturates at zero, so 0..=8 is nine columns.
        assert_eq!(visible.len(), 9);
        assert_eq!(visible.first(), Some(&"1.0"));
        assert_eq!(visible.last(), Some(&"5.0"));
    }

    struct StatsProbe {
        tracker: StatsTracker,
        theme: Theme,
        resets: Arc<Mutex<usize>>,
    }

    impl Render for StatsProbe {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            let resets = self.resets.clone();
            div()
                .size_full()
                .child(render(&self.tracker, &self.theme, 0, move |_, _, _| {
                    *resets.lock().expect("resets lock") += 1;
                }))
        }
    }

    fn probe(
        cx: &mut TestAppContext,
        tracker: StatsTracker,
    ) -> (VisualTestContext, Arc<Mutex<usize>>) {
        let resets: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let recorded = resets.clone();
        let probe = cx.update(|cx| {
            cx.new(|_| StatsProbe {
                tracker,
                theme: Theme::slate(),
                resets: recorded,
            })
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| probe.clone())
                .expect("test window")
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        (visual, resets)
    }

    fn assert_laid_out(visual: &mut VisualTestContext, selector: &'static str) {
        let bounds = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is laid out"));
        assert!(
            f32::from(bounds.size.width) > 0.0 && f32::from(bounds.size.height) > 0.0,
            "{selector} has a non-empty box"
        );
    }

    #[gpui::test]
    fn every_part_of_the_tab_has_a_place_in_the_window(cx: &mut TestAppContext) {
        let (mut visual, resets) = probe(cx, populated());

        // Each of the six strings `StatsTexts` composed is drawn, as the only child of
        // the element found here.
        for selector in [
            "stats-playtime",
            "stats-clicks",
            "stats-clicks-split",
            "stats-average",
            "stats-peak",
            "stats-samples",
        ] {
            assert_laid_out(&mut visual, selector);
        }

        // The bars are the tracker's window, drawn in the 124 px row plus the label under
        // it, so the container is taller than the row alone.
        let bars = visual
            .debug_bounds("stats-bars")
            .expect("a populated tracker draws bars");
        assert!(
            f32::from(bars.size.height) >= metrics::stats::BAR_AREA,
            "the bar area is the markup's 124 px"
        );
        assert!(
            visual.debug_bounds("stats-empty").is_none(),
            "no empty state over real samples"
        );

        // The button reports its press to the caller that owns the tracker, and its box is
        // the markup's `Height="40"`.
        let button = visual
            .debug_bounds("stats-reset")
            .expect("the reset button is laid out");
        assert_eq!(
            f32::from(button.size.height),
            metrics::stats::RESET_HEIGHT,
            "`Height=\"40\"` is the border-box height"
        );
        visual.simulate_click(button.center(), Modifiers::default());
        assert_eq!(
            *resets.lock().expect("resets lock"),
            1,
            "Reset Stats reports its press"
        );
    }

    #[gpui::test]
    fn an_empty_tracker_draws_the_empty_state_at_its_minimum_height(cx: &mut TestAppContext) {
        let (mut visual, _) = probe(cx, StatsTracker::new());

        assert_laid_out(&mut visual, "stats-empty");
        assert_laid_out(&mut visual, "stats-samples");
        assert!(
            visual.debug_bounds("stats-bars").is_none(),
            "no bars over an empty tracker"
        );

        // `MinHeight="158"` with the 8 px padding: the floor holds, and whether the pin
        // counts the padding inside `min_h` or outside it the box is at most one padding
        // pair taller than the markup's number.
        let height = f32::from(
            visual
                .debug_bounds("stats-chart")
                .expect("the chart box is laid out")
                .size
                .height,
        );
        assert!(
            height >= metrics::stats::CHART_MIN_HEIGHT,
            "the empty box is at least the markup's minimum, got {height}"
        );
        assert!(
            height <= metrics::stats::CHART_MIN_HEIGHT + 2.0 * metrics::stats::CHART_PADDING,
            "and not more than the minimum plus its padding, got {height}"
        );
    }
}
