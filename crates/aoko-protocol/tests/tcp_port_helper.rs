//! Port of `Aoko.Tests/TcpPortHelperTests.cs`.
//!
//! The managed suite could only prove the regex behaves, because the helper spawns
//! `netstat`. Splitting the parse half out of the spawn lets these cases drive real
//! `netstat -ano -p tcp` rows. `line_reports_listener` is the regex; `parse_listener_pid`
//! adds the managed loop's `LISTENING` filter and the PID column.

use aoko_protocol::netstat::{
    find_listening_process_id, line_reports_listener, parse_listener_pid,
};

#[test]
fn listening_port_line_reports_the_expected_match() {
    let cases = [
        (
            "TCP    0.0.0.0:25590          0.0.0.0:0              LISTENING       12345",
            true,
        ),
        (
            "TCP    127.0.0.1:25590        0.0.0.0:0              LISTENING       67890",
            true,
        ),
        (
            "TCP    0.0.0.0:255901         0.0.0.0:0              LISTENING       11111",
            false,
        ),
    ];

    for (line, expected) in cases {
        assert_eq!(line_reports_listener(line, 25590), expected, "{line:?}");
    }
}

#[test]
fn wildcard_and_loopback_addresses_are_recognised() {
    for address in ["0.0.0.0", "127.0.0.1", "[::]", "[::1]"] {
        let line = format!("  TCP    {address}:25590   0.0.0.0:0   LISTENING   4242");
        assert!(line_reports_listener(&line, 25590), "{address}");
    }
}

#[test]
fn other_local_addresses_are_not_recognised() {
    // The managed pattern only accepts the wildcard and loopback forms, so a bridge
    // bound to a specific interface address is not reported as "already listening".
    for address in ["192.168.1.7", "10.0.0.4", "[::2]"] {
        let line = format!("  TCP    {address}:25590   0.0.0.0:0   LISTENING   4242");
        assert!(!line_reports_listener(&line, 25590), "{address}");
    }
}

#[test]
fn the_address_must_be_preceded_by_whitespace() {
    // `\s(?:0\.0\.0\.0|...):port` runs against the *trimmed* row, so a row whose
    // first column is the address cannot match: trimming removes the only whitespace
    // the pattern could have anchored on. Only a row with a protocol column in front
    // of the address matches.
    assert!(!line_reports_listener(
        "0.0.0.0:25590  0.0.0.0:0  LISTENING  4242",
        25590
    ));
    assert!(!line_reports_listener(" 0.0.0.0:25590", 25590));
    assert!(line_reports_listener("TCP 0.0.0.0:25590", 25590));
}

#[test]
fn the_port_must_end_the_token() {
    let cases = [
        ("  TCP    0.0.0.0:25590", 25590, true),
        ("  TCP    0.0.0.0:255900", 25590, false),
        ("  TCP    0.0.0.0:125590", 25590, false),
    ];

    for (line, port, expected) in cases {
        assert_eq!(line_reports_listener(line, port), expected, "{line:?}");
    }
}

#[test]
fn the_regex_alone_ignores_the_tcp_state() {
    // The managed regex carries no state check; the `LISTENING` filter lives in the
    // surrounding loop. Keeping the split means an ESTABLISHED row still "reports the
    // port" while never yielding a listener PID.
    let established = "  TCP    127.0.0.1:25590   127.0.0.1:51234   ESTABLISHED   4242";

    assert!(line_reports_listener(established, 25590));
    assert_eq!(parse_listener_pid(established, 25590), None);
}

#[test]
fn try_get_listening_process_id_returns_null_or_positive_pid() {
    let output = concat!(
        "\r\n",
        "Active Connections\r\n",
        "\r\n",
        "  Proto  Local Address          Foreign Address        State           PID\r\n",
        "  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1100\r\n",
        "  TCP    0.0.0.0:25590          0.0.0.0:0              LISTENING       12345\r\n",
        "  TCP    127.0.0.1:25591        0.0.0.0:0              LISTENING       12346\r\n",
    );

    let pid = find_listening_process_id(output, 25590);

    assert_eq!(pid, Some(12345));
    assert!(pid.is_some_and(|value| value > 0));
}

#[test]
fn find_listening_process_id_returns_none_when_no_row_listens() {
    let output = concat!(
        "  TCP    0.0.0.0:25591        0.0.0.0:0              LISTENING       12346\r\n",
        "  TCP    0.0.0.0:25590        0.0.0.0:0              ESTABLISHED     12347\r\n",
    );

    assert_eq!(find_listening_process_id(output, 25590), None);
    assert_eq!(find_listening_process_id("", 25590), None);
    assert_eq!(
        find_listening_process_id("Active Connections\r\n", 25590),
        None
    );
}

#[test]
fn a_row_without_enough_columns_yields_no_pid() {
    assert_eq!(parse_listener_pid("garbage", 25590), None);
    assert_eq!(parse_listener_pid("", 25590), None);
    assert_eq!(
        parse_listener_pid("TCP 0.0.0.0:25590 LISTENING", 25590),
        None
    );
}

#[test]
fn a_zero_or_negative_pid_is_rejected() {
    let zero = "  TCP    0.0.0.0:25590   0.0.0.0:0   LISTENING   0";
    let negative = "  TCP    0.0.0.0:25590   0.0.0.0:0   LISTENING   -1";

    assert_eq!(parse_listener_pid(zero, 25590), None);
    assert_eq!(parse_listener_pid(negative, 25590), None);
}

#[test]
fn the_listening_marker_is_matched_case_insensitively() {
    let lower = "  TCP    0.0.0.0:25590   0.0.0.0:0   listening   4242";

    assert_eq!(parse_listener_pid(lower, 25590), Some(4242));
}
