//! Port of `Aoko.Tests/StatsTrackerTests.cs`.
//!
//! The managed suite read `StatsHistogramBucket.Visibility`; here the same
//! information is the `visible()` predicate, so the assertions are on labels.

use aoko_protocol::state::GameState;
use aoko_protocol::stats::{format_duration, is_blocking_gui, StatsTracker, CPS_BUCKET_COUNT};
use std::time::Duration;

#[test]
fn record_click_updates_cps_buckets_average_and_peak() {
    let mut stats = StatsTracker::new();

    stats.record_click(8.4, true);
    stats.record_click(12.6, true);
    stats.record_click(12.1, true);

    assert_eq!(stats.total_clicks(), 3);
    assert_eq!(stats.left_clicks(), 3);
    assert_eq!(stats.right_clicks(), 0);
    assert_eq!(stats.average_cps_text(), "11.0");
    assert_eq!(stats.peak_cps_text(), "12.6");
    assert_eq!(stats.buckets()[14].count(), 1);
    assert_eq!(stats.buckets()[22].count(), 1);
    assert_eq!(stats.buckets()[23].count(), 1);
    assert!(stats.has_cps_samples());
}

#[test]
fn record_click_shows_tight_detailed_cps_window() {
    let mut stats = StatsTracker::new();

    stats.record_click(12.1, true);
    stats.record_click(12.6, true);

    let visible: Vec<&str> = stats
        .buckets()
        .iter()
        .filter(|bucket| bucket.visible())
        .map(|bucket| bucket.label())
        .collect();

    assert_eq!(visible, ["11.0", "11.5", "12.0", "12.5", "13.0", "13.5"]);
}

#[test]
fn record_click_separates_left_and_right_counts() {
    let mut stats = StatsTracker::new();

    stats.record_click(10.0, true);
    stats.record_click(11.0, false);
    stats.record_click(12.0, false);

    assert_eq!(stats.total_clicks(), 3);
    assert_eq!(stats.left_clicks(), 1);
    assert_eq!(stats.right_clicks(), 2);
}

#[test]
fn record_click_ignores_non_positive_and_non_finite_rates() {
    let mut stats = StatsTracker::new();

    stats.record_click(0.0, true);
    stats.record_click(-4.0, true);
    stats.record_click(f32::NAN, true);
    stats.record_click(f32::INFINITY, true);

    assert_eq!(stats.total_clicks(), 0);
    assert!(!stats.has_cps_samples());
    assert_eq!(stats.average_cps_text(), "n/a");
}

#[test]
fn record_click_clamps_rates_outside_the_histogram() {
    let mut stats = StatsTracker::new();

    stats.record_click(0.1, true);
    stats.record_click(500.0, true);

    assert_eq!(
        stats.buckets()[0].count(),
        1,
        "below the window clamps to 0"
    );
    assert_eq!(
        stats.buckets()[CPS_BUCKET_COUNT - 1].count(),
        1,
        "above the window clamps to the last bucket"
    );
}

#[test]
fn record_playtime_sample_only_advances_for_active_play() {
    let mut stats = StatsTracker::new();
    let in_game = GameState::default();
    let visible_menu = GameState {
        gui_open: true,
        screen_name: "GuiMainMenu".to_owned(),
        ..GameState::default()
    };
    let ghost_gui = GameState {
        gui_open: true,
        screen_name: "GuiIngameMenu".to_owned(),
        ..GameState::default()
    };

    stats.record_playtime_sample(true, true, &in_game, false);
    stats.record_playtime_sample(false, true, &in_game, false);
    stats.record_playtime_sample(true, true, &visible_menu, true);
    stats.record_playtime_sample(true, true, &ghost_gui, false);

    assert_eq!(stats.total_playtime(), Duration::from_secs(2));
    assert_eq!(stats.total_playtime_text(), "00:02");
}

#[test]
fn a_cursor_visible_gui_only_blocks_while_the_bridge_is_connected() {
    let menu = GameState {
        gui_open: true,
        ..GameState::default()
    };

    assert!(is_blocking_gui(&menu, true));
    assert!(!is_blocking_gui(&menu, false));

    let mut stats = StatsTracker::new();
    stats.record_playtime_sample(true, false, &menu, true);
    assert_eq!(
        stats.total_playtime(),
        Duration::from_secs(1),
        "without a bridge the state is stale, so playtime keeps accruing"
    );
}

#[test]
fn reset_clears_all_stats_and_buckets() {
    let mut stats = StatsTracker::new();
    stats.record_click(9.0, true);
    stats.record_click(13.0, false);
    stats.record_playtime_sample(true, false, &GameState::default(), false);

    stats.reset();

    assert_eq!(stats.total_clicks(), 0);
    assert_eq!(stats.left_clicks(), 0);
    assert_eq!(stats.right_clicks(), 0);
    assert_eq!(stats.total_playtime(), Duration::ZERO);
    assert!(!stats.has_cps_samples());
    assert_eq!(stats.average_cps_text(), "n/a");
    assert!(stats.buckets().iter().all(|bucket| bucket.count() == 0));
    assert!(stats.buckets().iter().all(|bucket| !bucket.visible()));
}

#[test]
fn bucket_labels_span_the_documented_window() {
    let stats = StatsTracker::new();

    assert_eq!(stats.buckets().len(), CPS_BUCKET_COUNT);
    assert_eq!(stats.buckets()[0].label(), "1.0");
    assert_eq!(stats.buckets()[1].label(), "1.5");
    assert_eq!(stats.buckets()[CPS_BUCKET_COUNT - 1].label(), "25.0");
}

#[test]
fn bar_heights_are_relative_to_the_tallest_bucket() {
    let mut stats = StatsTracker::new();
    stats.record_click(10.0, true);
    stats.record_click(10.2, true);
    stats.record_click(12.0, true);

    let tallest = stats
        .buckets()
        .iter()
        .map(|bucket| bucket.bar_height())
        .fold(0.0_f64, f64::max);
    let quiet = stats.buckets()[30].bar_height();

    assert!(tallest > 0.0);
    assert_eq!(quiet, 0.0, "an empty bucket has no bar");
}

#[test]
fn duration_formatting_matches_the_managed_host() {
    assert_eq!(format_duration(Duration::ZERO), "00:00");
    assert_eq!(format_duration(Duration::from_secs(2)), "00:02");
    assert_eq!(format_duration(Duration::from_secs(75)), "01:15");
    assert_eq!(format_duration(Duration::from_secs(3600)), "1:00:00");
    assert_eq!(format_duration(Duration::from_secs(3672)), "1:01:12");
}
