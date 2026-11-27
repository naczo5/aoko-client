//! Port of `Aoko.Tests/BridgeCommandProtocolTests.cs`.
//!
//! The native bridge is written against these exact lines, so the assertions are
//! literal strings.

use aoko_protocol::commands::{
    build_account_command_line, build_server_command_line, AccountCommand, BridgeRequestIdSource,
};
use serde_json::Value;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

fn parse(line: &str) -> Value {
    serde_json::from_str(line.trim_end_matches('\n')).expect("one json document per line")
}

#[test]
fn get_session_builds_contract_line() {
    let line = build_account_command_line(123, &AccountCommand::get_session());

    assert_eq!(
        line,
        "{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":123}\n"
    );
}

#[test]
fn set_session_builds_contract_line_with_refresh_token() {
    let line = build_account_command_line(
        124,
        &AccountCommand::set_session(
            "Notch",
            "069a79f4-44e9-4726-a5be-fca90e38aaf5",
            "msa-token",
            Some("refresh-token".to_owned()),
        ),
    );

    assert_eq!(
        line,
        "{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":124,\
\"name\":\"Notch\",\"uuid\":\"069a79f4-44e9-4726-a5be-fca90e38aaf5\",\
\"token\":\"msa-token\",\"refreshToken\":\"refresh-token\"}\n"
    );
}

#[test]
fn set_session_omits_blank_refresh_token() {
    for refresh_token in [None, Some(String::new())] {
        let line = build_account_command_line(
            124,
            &AccountCommand::set_session("Notch", "uuid-1", "msa-token", refresh_token),
        );

        assert!(!line.contains("refreshToken"), "{line}");
        assert_eq!(
            line,
            "{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":124,\
\"name\":\"Notch\",\"uuid\":\"uuid-1\",\"token\":\"msa-token\"}\n"
        );
    }
}

#[test]
fn restore_session_builds_contract_line() {
    let line = build_account_command_line(125, &AccountCommand::restore_session());

    assert_eq!(
        line,
        "{\"type\":\"accountCommand\",\"action\":\"restoreSession\",\"requestId\":125}\n"
    );
}

#[test]
fn join_server_builds_contract_line() {
    let line = build_server_command_line(126, "mc.example.com", 25565);

    assert_eq!(
        line,
        "{\"type\":\"serverCommand\",\"action\":\"join\",\"requestId\":126,\
\"host\":\"mc.example.com\",\"port\":25565}\n"
    );
}

#[test]
fn command_lines_are_single_json_object_per_line() {
    let lines = [
        build_account_command_line(1, &AccountCommand::get_session()),
        build_account_command_line(
            2,
            &AccountCommand::set_session("Notch", "uuid", "token", None),
        ),
        build_account_command_line(3, &AccountCommand::restore_session()),
        build_server_command_line(4, "host", 25565),
    ];

    for line in lines {
        assert!(line.ends_with('\n'), "{line}");
        assert_eq!(line.matches('\n').count(), 1, "{line}");
        assert!(!line.contains('\r'), "{line}");
        assert!(parse(&line).is_object(), "{line}");
    }
}

#[test]
fn set_session_escapes_user_names_without_breaking_the_line() {
    let line = build_account_command_line(
        7,
        &AccountCommand::set_session("weird\"name", "uuid", "token", None),
    );

    assert_eq!(line.matches('\n').count(), 1);
    assert_eq!(parse(&line)["name"], "weird\"name");
}

#[test]
fn a_control_character_in_a_name_still_yields_one_line() {
    let line = build_account_command_line(
        8,
        &AccountCommand::set_session("bad\nname", "uuid", "token", None),
    );

    assert_eq!(
        line.matches('\n').count(),
        1,
        "the name is escaped, not raw"
    );
    assert_eq!(parse(&line)["name"], "bad\nname");
}

#[test]
fn account_command_display_never_prints_credentials() {
    let command = AccountCommand::set_session(
        "Notch",
        "uuid-1",
        "super-secret-token",
        Some("super-secret-refresh".to_owned()),
    );

    let text = command.to_string();

    assert_eq!(text, "AccountCommand { Action = setSession }");
    assert!(!text.contains("secret"));
    assert_eq!(format!("{command:?}"), text, "Debug is redacted too");
}

#[test]
fn credentials_are_only_attached_to_set_session() {
    assert!(!AccountCommand::get_session().carries_credentials());
    assert!(!AccountCommand::restore_session().carries_credentials());
    assert!(AccountCommand::set_session("a", "b", "c", None).carries_credentials());
}

#[test]
fn non_credential_commands_never_emit_credential_fields() {
    // Even when a caller stuffs them in, `getSession`/`restoreSession` are written as
    // bare envelopes: the native handler switches on `action` and would ignore them.
    let line = build_account_command_line(9, &AccountCommand::get_session());

    assert!(!line.contains("token"));
    assert!(!line.contains("uuid"));
    assert!(!line.contains("name"));
}

#[test]
fn request_id_source_is_monotonic() {
    let source = BridgeRequestIdSource::new();

    let first = source.next_id();
    let second = source.next_id();
    let third = source.next_id();

    assert_eq!(first, 1);
    assert!(second > first);
    assert!(third > second);
}

#[test]
fn request_id_source_is_unique_under_concurrency() {
    let source = Arc::new(BridgeRequestIdSource::new());
    let seen = Arc::new(std::sync::Mutex::new(Vec::with_capacity(256)));
    let peak = Arc::new(AtomicI32::new(0));

    let mut threads = Vec::with_capacity(8);
    for _ in 0..8 {
        let source = Arc::clone(&source);
        let seen = Arc::clone(&seen);
        let peak = Arc::clone(&peak);
        threads.push(std::thread::spawn(move || {
            for _ in 0..32 {
                let id = source.next_id();
                peak.fetch_max(id, Ordering::SeqCst);
                seen.lock().expect("lock").push(id);
            }
        }));
    }
    for thread in threads {
        thread.join().expect("worker thread");
    }

    let ids = seen.lock().expect("lock");
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();

    assert_eq!(ids.len(), 256);
    assert_eq!(unique.len(), 256, "every id is unique");
    assert_eq!(peak.load(Ordering::SeqCst), 256);
    assert_eq!(*unique.last().expect("max"), 256);
}

#[test]
fn set_session_with_a_2k_token_round_trips_unescaped_and_fits_the_legacy_buffer() {
    let token = jwt_like_token(2000);
    let line = build_account_command_line(
        200,
        &AccountCommand::set_session(
            "Notch",
            "069a79f4-44e9-4726-a5be-fca90e38aaf5",
            token.clone(),
            None,
        ),
    );

    let parsed = parse(&line);

    assert_eq!(parsed["token"], token.as_str());
    assert!(line.contains(&token));
    assert_eq!(line.matches('\n').count(), 1);
    assert!(line.len() > 2000, "unexpected line length {}", line.len());
    assert!(
        line.len() < 4096,
        "a 2 KB token produced {} characters; the legacy 4096-byte buffer cannot hold it",
        line.len()
    );
}

#[test]
fn set_session_with_access_and_refresh_tokens_exceeds_the_legacy_buffer() {
    let token = jwt_like_token(2000);
    let refresh_token = jwt_like_token(2000);
    let line = build_account_command_line(
        201,
        &AccountCommand::set_session(
            "Notch",
            "069a79f4-44e9-4726-a5be-fca90e38aaf5",
            token.clone(),
            Some(refresh_token.clone()),
        ),
    );

    let parsed = parse(&line);

    assert_eq!(parsed["token"], token.as_str());
    assert_eq!(parsed["refreshToken"], refresh_token.as_str());
    assert!(
        line.len() > 4096,
        "access+refresh token line was {} characters; native lines need 64 KiB",
        line.len()
    );
}

fn jwt_like_token(length: usize) -> String {
    const SEGMENT: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6IjEyMyJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.";
    let mut token = String::with_capacity(length + SEGMENT.len());
    while token.len() < length {
        token.push_str(SEGMENT);
    }
    token.truncate(length);
    token
}
