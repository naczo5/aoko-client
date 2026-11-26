//! Compact relative timestamps for account and server cards (`just now`,
//! `3d ago`).
//!
//! Ported from `Aoko/Views/RelativeTime.cs`. The managed version takes a
//! `DateTimeOffset` because the stores deserialize one; the Rust stores keep the
//! raw ISO-8601 text (`StoredAccount::last_used_at` is `Option<String>`), so this
//! module also owns the parser that turns that text into epoch milliseconds.
//!
//! The sentence around the value stays the caller's job (`Last used 3d ago`,
//! `Last joined 3d ago`), exactly as in the managed source: one implementation so
//! the two tabs cannot drift.

/// One minute in milliseconds.
const MINUTE_MS: i64 = 60_000;
/// One hour in milliseconds.
const HOUR_MS: i64 = 3_600_000;
/// One day in milliseconds.
const DAY_MS: i64 = 86_400_000;

/// `RelativeTime.Format`: the elided form, from the elapsed time alone.
///
/// A negative elapsed time (a clock that moved, a timestamp in the future) reads
/// `just now`, matching the managed `elapsed < TimeSpan.Zero` branch.
#[must_use]
pub fn format_elapsed(elapsed_ms: i64) -> String {
    if elapsed_ms < 0 || elapsed_ms < MINUTE_MS {
        return "just now".to_owned();
    }

    if elapsed_ms < HOUR_MS {
        return format!("{}m ago", elapsed_ms / MINUTE_MS);
    }

    if elapsed_ms < DAY_MS {
        return format!("{}h ago", elapsed_ms / HOUR_MS);
    }

    let days = elapsed_ms / DAY_MS;
    if days < 30 {
        return format!("{days}d ago");
    }

    if days < 365 {
        return format!("{}mo ago", days / 30);
    }

    format!("{}y ago", days / 365)
}

/// Formats a stored ISO-8601 timestamp against `now_ms`.
///
/// `None` when the text is absent or unparsable, so the caller can use the same
/// wording it uses for "never" instead of inventing a comparison against zero.
#[must_use]
pub fn format_stored(value: Option<&str>, now_ms: i64) -> Option<String> {
    let then = parse_iso8601_utc_millis(value?)?;
    Some(format_elapsed(now_ms.saturating_sub(then)))
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fff](Z|+-HH:MM)` into milliseconds since the
/// Unix epoch.
///
/// The stores write `System.Text.Json`'s round-trip format, which is always this
/// shape; hand-edited files are the only other source. A malformed value returns
/// `None` rather than a defaulted date, so nothing claims a card was used in 1970.
#[must_use]
pub fn parse_iso8601_utc_millis(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, rest) = text.split_once(['T', 't', ' '])?;

    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    // Offset suffix: `Z`, `z`, empty, `+HH:MM`, `-HH:MM`, `+HHMM`, `-HHMM`.
    let (clock, offset_minutes) = split_offset(rest)?;
    let mut clock_parts = clock.split(':');
    let hour: i64 = clock_parts.next()?.parse().ok()?;
    let minute: i64 = clock_parts.next()?.parse().ok()?;
    let seconds = clock_parts.next()?.to_owned();
    if clock_parts.next().is_some() || !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        return None;
    }

    let (second_text, fraction_ms) = match seconds.split_once('.') {
        Some((whole, fraction)) => (whole, parse_fraction_millis(fraction)?),
        None => (seconds.as_str(), 0),
    };
    let second: i64 = second_text.parse().ok()?;
    if !(0..=59).contains(&second) {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let millis = days
        .checked_mul(DAY_MS)?
        .checked_add(hour * HOUR_MS)?
        .checked_add(minute * MINUTE_MS)?
        .checked_add(second * 1000)?
        .checked_add(i64::from(fraction_ms))?;

    millis.checked_sub(i64::from(offset_minutes) * MINUTE_MS)
}

/// Splits the trailing zone designator off a clock, returning the clock and the
/// offset in minutes east of UTC.
fn split_offset(rest: &str) -> Option<(&str, i32)> {
    if let Some(clock) = rest.strip_suffix(['Z', 'z']) {
        return Some((clock, 0));
    }

    // Find a sign that starts the offset. A leading `-` cannot occur: the date
    // part already consumed the year, so any sign here belongs to the zone.
    let sign_index = rest.rfind(['+', '-']);
    let Some(index) = sign_index else {
        return Some((rest, 0));
    };

    let (clock, zone) = rest.split_at(index);
    let negative = zone.starts_with('-');
    let digits = zone[1..].replace(':', "");
    if digits.len() != 4 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    let hours: i32 = digits[0..2].parse().ok()?;
    let minutes: i32 = digits[2..4].parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }

    let total = hours * 60 + minutes;
    Some((clock, if negative { -total } else { total }))
}

/// Sub-second digits to milliseconds, truncated or padded to three places.
fn parse_fraction_millis(fraction: &str) -> Option<i32> {
    if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    let mut millis = 0_i32;
    for index in 0..3 {
        millis *= 10;
        if let Some(byte) = fraction.as_bytes().get(index) {
            millis += i32::from(byte - b'0');
        }
    }

    Some(millis)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`), so no date crate is pulled into a UI-only layer.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = i64::from((month + 9) % 12);
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW_MS: i64 = 1_700_000_000_000;

    #[test]
    fn elapsed_buckets_match_the_managed_switch() {
        assert_eq!(format_elapsed(0), "just now");
        assert_eq!(format_elapsed(-1), "just now");
        assert_eq!(format_elapsed(59_999), "just now");
        assert_eq!(format_elapsed(60_000), "1m ago");
        assert_eq!(format_elapsed(5 * MINUTE_MS), "5m ago");
        assert_eq!(format_elapsed(HOUR_MS), "1h ago");
        assert_eq!(format_elapsed(3 * HOUR_MS), "3h ago");
        assert_eq!(format_elapsed(DAY_MS), "1d ago");
        assert_eq!(format_elapsed(60 * DAY_MS), "2mo ago");
        assert_eq!(format_elapsed(400 * DAY_MS), "1y ago");
    }

    #[test]
    fn the_last_used_sentence_never_says_never_used_twice() {
        // Port of `LastUsedLabel_UsesSentenceCaseAndNeverSaysNeverUsedTwice`: a
        // three-day-old stamp reads `3d ago`, and the surrounding sentence is the
        // caller's. The fixed epoch is 2023-11-14T22:13:20Z, so this is exactly
        // three days earlier.
        let label = format_stored(Some("2023-11-11T22:13:20Z"), NOW_MS)
            .map(|relative| format!("Last used {relative}"));
        assert_eq!(label.as_deref(), Some("Last used 3d ago"));
        assert!(
            format_stored(None, NOW_MS).is_none(),
            "absent text is the caller's `Never used`, not a timestamp"
        );
    }

    #[test]
    fn iso_timestamps_round_trip_through_the_parser() {
        // 2023-11-14T22:13:20Z is 1_700_000_000 seconds after the epoch.
        assert_eq!(
            parse_iso8601_utc_millis("2023-11-14T22:13:20Z"),
            Some(NOW_MS)
        );
        assert_eq!(
            parse_iso8601_utc_millis("2023-11-14T22:13:20+00:00"),
            Some(NOW_MS)
        );
        assert_eq!(
            parse_iso8601_utc_millis("2023-11-14T22:13:20.500Z"),
            Some(NOW_MS + 500)
        );
        // An offset is subtracted, not added: 23:13:20+01:00 is the same instant.
        assert_eq!(
            parse_iso8601_utc_millis("2023-11-14T23:13:20+01:00"),
            Some(NOW_MS)
        );
        assert_eq!(
            parse_iso8601_utc_millis("2023-11-14T21:13:20-0100"),
            Some(NOW_MS)
        );
        assert_eq!(parse_iso8601_utc_millis("1970-01-01T00:00:00Z"), Some(0));
    }

    #[test]
    fn malformed_timestamps_are_refused_rather_than_defaulted() {
        for text in [
            "",
            "not a date",
            "2023-11-14",
            "2023-13-14T22:13:20Z",
            "2023-11-32T22:13:20Z",
            "2023-11-14T24:13:20Z",
            "2023-11-14T22:60:20Z",
            "2023-11-14T22:13:60Z",
            "2023-11-14T22:13:20+1:00",
            "2023-11-14T22:13:Z",
            "2023-11-14T22:13:20.abcZ",
        ] {
            assert_eq!(parse_iso8601_utc_millis(text), None, "accepted {text:?}");
        }
    }

    #[test]
    fn a_future_stamp_reads_just_now() {
        let future = "2030-01-01T00:00:00Z";
        assert_eq!(
            format_stored(Some(future), NOW_MS).as_deref(),
            Some("just now")
        );
    }
}
