//! `netstat` line parsing for the bridge port diagnostic.
//!
//! Ported from the parse half of `Aoko/Core/TcpPortHelper.cs`. The process spawn
//! belongs to `aoko-platform`; nothing here runs a command. The managed helper is
//! diagnostics only: it decides "the selected PID's bridge is already listening,
//! so just reconnect", "some other process owns the port, so refuse", and "we are
//! connected to the wrong PID, so disconnect".

/// Address forms that own a listening port. `0.0.0.0` and `[::]` are included
/// because a wildcard listener also owns the port.
const LISTENER_ADDRESS_FORMS: [&str; 4] = ["0.0.0.0", "127.0.0.1", "[::]", "[::1]"];

/// Marker every `netstat` listener row carries.
const LISTENING_MARKER: &str = "LISTENING";

/// Minimum column count before the last column can be a PID.
const MINIMUM_COLUMNS: usize = 5;

/// Whether one `netstat` row names a listener on `port`.
///
/// Equivalent to the managed regex
/// `\s(?:0\.0\.0\.0|127\.0\.0\.1|\[::\]|\[::1\]):{port}(\s|$)` applied to the
/// trimmed line: the address must be preceded by whitespace and the port must be
/// followed by whitespace or end of line, so `:255901` never matches port 25590.
#[must_use]
pub fn line_reports_listener(line: &str, port: u16) -> bool {
    let trimmed = line.trim();
    let port_text = port.to_string();
    let mut previous_was_space = false;

    for (index, character) in trimmed.char_indices() {
        if previous_was_space {
            for form in LISTENER_ADDRESS_FORMS {
                let Some(after_form) = trimmed[index..].strip_prefix(form) else {
                    continue;
                };
                let Some(after_colon) = after_form.strip_prefix(':') else {
                    continue;
                };
                let Some(remainder) = after_colon.strip_prefix(port_text.as_str()) else {
                    continue;
                };

                if remainder.is_empty() || remainder.chars().next().is_some_and(char::is_whitespace)
                {
                    return true;
                }
            }
        }

        previous_was_space = character.is_whitespace();
    }

    false
}

/// Extracts the PID from one `netstat` row when it is a listener on `port`.
///
/// Requires the row to mention `LISTENING`, to match the listener pattern, to
/// have at least five columns, and to end in a positive PID.
#[must_use]
pub fn parse_listener_pid(line: &str, port: u16) -> Option<i32> {
    let trimmed = line.trim();
    if !contains_ignore_ascii_case(trimmed, LISTENING_MARKER) {
        return None;
    }
    if !line_reports_listener(trimmed, port) {
        return None;
    }

    let mut columns = trimmed.split_whitespace();
    let columns_count = columns.clone().count();
    if columns_count < MINIMUM_COLUMNS {
        return None;
    }

    let pid = columns.nth(columns_count - 1)?.parse::<i32>().ok()?;
    (pid > 0).then_some(pid)
}

/// Scans a whole `netstat -ano -p tcp` dump and returns the first matching PID.
#[must_use]
pub fn find_listening_process_id(netstat_output: &str, port: u16) -> Option<i32> {
    netstat_output
        .split('\n')
        .find_map(|line| parse_listener_pid(line, port))
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }

    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}
