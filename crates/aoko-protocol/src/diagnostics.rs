//! Transport window metrics and the config-send policy.
//!
//! Ported from `Aoko/Core/ManagedTransportDiagnostics.cs` plus the two pure
//! predicates from `GameStateClient` (`IsConfigSendDue`,
//! `ShouldSerializeConfig`) that the same test suite pins.
//!
//! Timestamps are opaque integer ticks supplied by the caller, exactly like
//! `Stopwatch.GetTimestamp`, so this stays testable without a clock.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

/// Environment variable that turns the window metrics on.
pub const DIAGNOSTICS_ENV_VAR: &str = "AOKO_PERF_DIAGNOSTICS";

/// Default summary window (`TimeSpan.FromSeconds(5)` in the host).
pub const DEFAULT_SUMMARY_INTERVAL: Duration = Duration::from_secs(5);

/// One flushed metrics window.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ManagedTransportSnapshot {
    pub inbound_messages: i64,
    pub inbound_characters: i64,
    pub inbound_parse_ticks: i64,
    pub state_notifications: i64,
    pub config_serializations: i64,
    pub config_characters: i64,
    pub config_serialization_ticks: i64,
    pub config_sends: i64,
    pub config_send_characters: i64,
    pub window_seconds: f64,
}

impl ManagedTransportSnapshot {
    /// One log line of rates. Never contains payload content.
    #[must_use]
    pub fn to_log_message(&self, ticks_per_second: u64) -> String {
        let seconds = if self.window_seconds > 0.001 {
            self.window_seconds
        } else {
            0.001
        };
        let ticks_to_milliseconds = if ticks_per_second == 0 {
            0.0
        } else {
            1000.0 / ticks_per_second as f64
        };

        format!(
            "transport window={seconds:.1}s \
             inbound={:.1}/s chars={:.0}/s \
             parse={:.2}ms/s \
             stateNotify={:.1}/s \
             configSerialize={:.2}/s \
             configSerializeCpu={:.2}ms/s \
             configSend={:.2}/s chars={:.0}/s",
            self.inbound_messages as f64 / seconds,
            self.inbound_characters as f64 / seconds,
            self.inbound_parse_ticks as f64 * ticks_to_milliseconds / seconds,
            self.state_notifications as f64 / seconds,
            self.config_serializations as f64 / seconds,
            self.config_serialization_ticks as f64 * ticks_to_milliseconds / seconds,
            self.config_sends as f64 / seconds,
            self.config_send_characters as f64 / seconds,
        )
    }
}

const COUNTERS: usize = 9;
const INBOUND_MESSAGES: usize = 0;
const INBOUND_CHARACTERS: usize = 1;
const INBOUND_PARSE_TICKS: usize = 2;
const STATE_NOTIFICATIONS: usize = 3;
const CONFIG_SERIALIZATIONS: usize = 4;
const CONFIG_CHARACTERS: usize = 5;
const CONFIG_SERIALIZATION_TICKS: usize = 6;
const CONFIG_SENDS: usize = 7;
const CONFIG_SEND_CHARACTERS: usize = 8;

/// Windowed transport counters. A disabled instance records nothing and never
/// produces a snapshot.
#[derive(Debug)]
pub struct ManagedTransportDiagnostics {
    enabled: bool,
    ticks_per_second: u64,
    summary_interval_ticks: i64,
    window_started: AtomicI64,
    counters: [AtomicI64; COUNTERS],
}

impl ManagedTransportDiagnostics {
    #[must_use]
    pub fn new(enabled: bool, summary_interval: Duration, ticks_per_second: u64, now: i64) -> Self {
        let ticks_per_second = ticks_per_second.max(1);
        let interval_ticks = (summary_interval.as_secs_f64() * ticks_per_second as f64) as i64;

        let counters: [AtomicI64; COUNTERS] = std::array::from_fn(|_| AtomicI64::new(0));

        Self {
            enabled,
            ticks_per_second,
            summary_interval_ticks: interval_ticks.max(1),
            window_started: AtomicI64::new(now),
            counters,
        }
    }

    /// Reads `AOKO_PERF_DIAGNOSTICS`; `1` and `true` enable it, case-insensitive.
    #[must_use]
    pub fn from_environment(ticks_per_second: u64, now: i64) -> Self {
        let enabled = std::env::var(DIAGNOSTICS_ENV_VAR).is_ok_and(|value| {
            value.eq_ignore_ascii_case("1") || value.eq_ignore_ascii_case("true")
        });
        Self::new(enabled, DEFAULT_SUMMARY_INTERVAL, ticks_per_second, now)
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub fn ticks_per_second(&self) -> u64 {
        self.ticks_per_second
    }

    /// The config payload's `perfDiagnostics` flag is this value.
    #[must_use]
    pub fn perf_diagnostics_flag(&self) -> bool {
        self.enabled
    }

    pub fn record_inbound(&self, characters: i64, parse_ticks: i64) {
        if !self.enabled {
            return;
        }
        self.counters[INBOUND_MESSAGES].fetch_add(1, Ordering::Relaxed);
        self.counters[INBOUND_CHARACTERS].fetch_add(characters.max(0), Ordering::Relaxed);
        self.counters[INBOUND_PARSE_TICKS].fetch_add(parse_ticks.max(0), Ordering::Relaxed);
    }

    pub fn record_config_serialization(&self, characters: i64, serialization_ticks: i64) {
        if !self.enabled {
            return;
        }
        self.counters[CONFIG_SERIALIZATIONS].fetch_add(1, Ordering::Relaxed);
        self.counters[CONFIG_CHARACTERS].fetch_add(characters.max(0), Ordering::Relaxed);
        self.counters[CONFIG_SERIALIZATION_TICKS]
            .fetch_add(serialization_ticks.max(0), Ordering::Relaxed);
    }

    pub fn record_state_notification(&self) {
        if !self.enabled {
            return;
        }
        self.counters[STATE_NOTIFICATIONS].fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_config_send(&self, characters: i64) {
        if !self.enabled {
            return;
        }
        self.counters[CONFIG_SENDS].fetch_add(1, Ordering::Relaxed);
        self.counters[CONFIG_SEND_CHARACTERS].fetch_add(characters.max(0), Ordering::Relaxed);
    }

    /// Flushes the window when `summary_interval` has elapsed, resetting it.
    ///
    /// Returns `None` when diagnostics are off, the window is not due yet, or
    /// another thread already took it.
    pub fn try_take_snapshot(&self, now_timestamp: i64) -> Option<ManagedTransportSnapshot> {
        if !self.enabled {
            return None;
        }

        let started = self.window_started.load(Ordering::Relaxed);
        let elapsed = now_timestamp - started;
        if elapsed < self.summary_interval_ticks {
            return None;
        }

        if self
            .window_started
            .compare_exchange(started, now_timestamp, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }

        let take = |index: usize| self.counters[index].swap(0, Ordering::Relaxed);

        Some(ManagedTransportSnapshot {
            inbound_messages: take(INBOUND_MESSAGES),
            inbound_characters: take(INBOUND_CHARACTERS),
            inbound_parse_ticks: take(INBOUND_PARSE_TICKS),
            state_notifications: take(STATE_NOTIFICATIONS),
            config_serializations: take(CONFIG_SERIALIZATIONS),
            config_characters: take(CONFIG_CHARACTERS),
            config_serialization_ticks: take(CONFIG_SERIALIZATION_TICKS),
            config_sends: take(CONFIG_SENDS),
            config_send_characters: take(CONFIG_SEND_CHARACTERS),
            window_seconds: elapsed as f64 / self.ticks_per_second as f64,
        })
    }
}

/// `GameStateClient.IsConfigSendDue`: send on revision change or heartbeat.
#[must_use]
pub fn is_config_send_due(
    revision: i64,
    last_sent_revision: i64,
    elapsed_since_send_ms: i64,
    heartbeat_ms: i64,
) -> bool {
    revision != last_sent_revision || elapsed_since_send_ms >= heartbeat_ms
}

/// `GameStateClient.ShouldSerializeConfig`: reuse the cached payload when the
/// revision is unchanged.
#[must_use]
pub fn should_serialize_config(
    revision: i64,
    cached_revision: i64,
    has_cached_payload: bool,
) -> bool {
    !has_cached_payload || revision != cached_revision
}
