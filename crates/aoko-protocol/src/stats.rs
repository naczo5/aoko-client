//! CPS / playtime aggregation, ported from the aggregation half of
//! `Aoko/Core/StatsTracker.cs`.
//!
//! The managed type is a WPF `INotifyPropertyChanged` singleton with a one-second
//! `DispatcherTimer` that samples the foreground window. None of that belongs
//! here: this module owns the numbers, the histogram buckets, and the text
//! formatting. The loop that samples `WindowDetection` and `GameStateClient`
//! lives in `aoko-core`, and a UI binds to the values.

use std::time::Duration;

use crate::state::GameState;

/// Smallest CPS value that has a bucket.
pub const MIN_CPS_BUCKET: f64 = 1.0;
/// Largest CPS value that has a bucket.
pub const MAX_CPS_BUCKET: f64 = 25.0;
/// Width of one CPS bucket.
pub const CPS_BUCKET_SIZE: f64 = 0.5;
/// Buckets of margin kept visible on each side of the used window.
pub const CPS_WINDOW_PADDING: usize = 2;
/// Tallest histogram bar, in device-independent pixels.
pub const MAX_BAR_HEIGHT: f64 = 118.0;
/// Floor for a non-empty bar so a single click stays visible.
pub const MIN_VISIBLE_BAR_HEIGHT: f64 = 4.0;

/// Number of CPS buckets: `(25 - 1) / 0.5 + 1`.
pub const CPS_BUCKET_COUNT: usize = 49;

/// One histogram column.
#[derive(Debug, Clone, PartialEq)]
pub struct StatsHistogramBucket {
    label: String,
    count: i32,
    bar_height: f64,
    visible: bool,
}

impl StatsHistogramBucket {
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub fn count(&self) -> i32 {
        self.count
    }

    #[must_use]
    pub fn count_text(&self) -> String {
        self.count.to_string()
    }

    #[must_use]
    pub fn bar_height(&self) -> f64 {
        self.bar_height
    }

    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible
    }
}

/// Accumulated click statistics.
#[derive(Debug, Clone)]
pub struct StatsTracker {
    cps_counts: [i32; CPS_BUCKET_COUNT],
    buckets: Vec<StatsHistogramBucket>,
    total_clicks: i32,
    left_clicks: i32,
    right_clicks: i32,
    cps_total: f64,
    peak_cps: f64,
    total_playtime: Duration,
}

impl Default for StatsTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl StatsTracker {
    #[must_use]
    pub fn new() -> Self {
        let buckets = (0..CPS_BUCKET_COUNT)
            .map(|index| {
                let value = MIN_CPS_BUCKET + (index as f64 * CPS_BUCKET_SIZE);
                StatsHistogramBucket {
                    label: format!("{value:.1}"),
                    count: 0,
                    bar_height: 0.0,
                    visible: false,
                }
            })
            .collect();

        Self {
            cps_counts: [0; CPS_BUCKET_COUNT],
            buckets,
            total_clicks: 0,
            left_clicks: 0,
            right_clicks: 0,
            cps_total: 0.0,
            peak_cps: 0.0,
            total_playtime: Duration::ZERO,
        }
    }

    #[must_use]
    pub fn buckets(&self) -> &[StatsHistogramBucket] {
        &self.buckets
    }

    #[must_use]
    pub fn total_clicks(&self) -> i32 {
        self.total_clicks
    }

    #[must_use]
    pub fn left_clicks(&self) -> i32 {
        self.left_clicks
    }

    #[must_use]
    pub fn right_clicks(&self) -> i32 {
        self.right_clicks
    }

    #[must_use]
    pub fn cps_sample_count(&self) -> i32 {
        self.total_clicks
    }

    #[must_use]
    pub fn has_cps_samples(&self) -> bool {
        self.total_clicks > 0
    }

    #[must_use]
    pub fn total_playtime(&self) -> Duration {
        self.total_playtime
    }

    #[must_use]
    pub fn total_playtime_text(&self) -> String {
        format_duration(self.total_playtime)
    }

    #[must_use]
    pub fn average_cps_text(&self) -> String {
        if self.has_cps_samples() {
            format!("{:.1}", self.cps_total / f64::from(self.total_clicks))
        } else {
            "n/a".to_owned()
        }
    }

    #[must_use]
    pub fn peak_cps_text(&self) -> String {
        if self.has_cps_samples() {
            format!("{:.1}", self.peak_cps)
        } else {
            "n/a".to_owned()
        }
    }

    /// Records one click sample. Zero, negative, NaN, and infinite rates are
    /// ignored, matching the managed guard.
    pub fn record_click(&mut self, cps: f32, left_button: bool) {
        let cps = f64::from(cps);
        if !cps.is_finite() || cps <= 0.0 {
            return;
        }

        self.total_clicks += 1;
        if left_button {
            self.left_clicks += 1;
        } else {
            self.right_clicks += 1;
        }

        self.cps_total += cps;
        if cps > self.peak_cps {
            self.peak_cps = cps;
        }

        let raw_bucket = ((cps - MIN_CPS_BUCKET) / CPS_BUCKET_SIZE).floor();
        let bucket = (raw_bucket as i64).clamp(0, CPS_BUCKET_COUNT as i64 - 1) as usize;
        self.cps_counts[bucket] += 1;
        self.refresh_cps_buckets();
    }

    /// Adds one second of playtime when the client is focused and not blocked by
    /// a cursor-visible GUI.
    pub fn record_playtime_sample(
        &mut self,
        minecraft_active: bool,
        bridge_connected: bool,
        state: &GameState,
        cursor_visible: bool,
    ) {
        if !minecraft_active {
            return;
        }
        if bridge_connected && is_blocking_gui(state, cursor_visible) {
            return;
        }

        self.total_playtime += Duration::from_secs(1);
    }

    /// Clears every counter and bucket.
    pub fn reset(&mut self) {
        self.total_clicks = 0;
        self.left_clicks = 0;
        self.right_clicks = 0;
        self.cps_total = 0.0;
        self.peak_cps = 0.0;
        self.total_playtime = Duration::ZERO;
        self.cps_counts = [0; CPS_BUCKET_COUNT];
        self.refresh_cps_buckets();
    }

    fn refresh_cps_buckets(&mut self) {
        let mut first: Option<usize> = None;
        let mut last: usize = 0;
        let mut max = 0;

        for (index, count) in self.cps_counts.iter().enumerate() {
            if *count <= 0 {
                continue;
            }
            if first.is_none() {
                first = Some(index);
            }
            last = index;
            if *count > max {
                max = *count;
            }
        }

        let (visible_start, visible_end) = match first {
            None => (0, 0),
            Some(first) => (
                first.saturating_sub(CPS_WINDOW_PADDING),
                (last + CPS_WINDOW_PADDING).min(CPS_BUCKET_COUNT - 1),
            ),
        };

        for (index, bucket) in self.buckets.iter_mut().enumerate() {
            let count = self.cps_counts[index];
            bucket.count = count.max(0);
            bucket.visible = first.is_some() && index >= visible_start && index <= visible_end;

            let mut height = if max <= 0 {
                0.0
            } else {
                MAX_BAR_HEIGHT * f64::from(count) / f64::from(max)
            };
            if count > 0 && height < MIN_VISIBLE_BAR_HEIGHT {
                height = MIN_VISIBLE_BAR_HEIGHT;
            }
            bucket.bar_height = height.max(0.0);
        }
    }
}

/// A GUI that owns the cursor blocks playtime accrual.
#[must_use]
pub fn is_blocking_gui(state: &GameState, cursor_visible: bool) -> bool {
    state.gui_open && cursor_visible
}

/// `StatsTracker.FormatDuration`: `h:mm:ss` past an hour, `mm:ss` below it.
#[must_use]
pub fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours >= 1 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}
