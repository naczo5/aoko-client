//! Port of `Aoko.Tests/BridgeCommandResultTests.cs`.
//!
//! Reply decoding for `commandResult` plus the vetted sentence table that is allowed
//! to reach the status bar.

use aoko_protocol::command_result::{error, BridgeCommandResult};
use aoko_protocol::commands::wire_type;
use serde_json::Value;

const SET_SESSION_REPLY: &str = r#"{
  "type": "commandResult",
  "requestId": 124,
  "ok": true,
  "action": "setSession",
  "username": "Notch",
  "uuid": "069a79f4-44e9-4726-a5be-fca90e38aaf5",
  "inWorld": false,
  "ready": true
}"#;

fn parse(text: &str) -> (i32, BridgeCommandResult) {
    let node: Value = serde_json::from_str(text).expect("fixture json");
    BridgeCommandResult::try_parse(&node).expect("decodes as a command result")
}

#[test]
fn try_parse_reads_the_contract_reply() {
    let (request_id, result) = parse(SET_SESSION_REPLY);

    assert_eq!(request_id, 124);
    assert!(result.ok);
    assert_eq!(result.action, "setSession");
    assert_eq!(result.username, "Notch");
    assert_eq!(result.uuid, "069a79f4-44e9-4726-a5be-fca90e38aaf5");
    assert!(!result.in_world);
    assert!(result.ready);
    assert_eq!(result.error, "");
    assert_eq!(result.message(), "Switched to Notch.");
}

#[test]
fn try_parse_reads_the_join_failure_reply() {
    let (request_id, result) = parse(
        r#"{"type":"commandResult","requestId":126,"ok":false,"action":"join","error":"connect_path_not_found"}"#,
    );

    assert_eq!(request_id, 126);
    assert!(!result.ok);
    assert_eq!(result.action, "join");
    assert_eq!(result.error, "connect_path_not_found");
    assert_eq!(
        result.message(),
        "This client's connect screen could not be resolved, so the join cannot be started."
    );
}

#[test]
fn try_parse_missing_optional_fields_use_safe_defaults() {
    let (request_id, result) = parse(r#"{"type":"commandResult","requestId":9,"ok":true}"#);

    assert_eq!(request_id, 9);
    assert!(result.ok);
    assert_eq!(result.action, "");
    assert_eq!(result.username, "");
    assert_eq!(result.uuid, "");
    assert_eq!(result.error, "");
    assert!(!result.in_world);
    assert!(!result.ready);
    assert_eq!(result.message(), "The game completed the command.");
}

#[test]
fn try_parse_unknown_action_still_decodes() {
    let (_, result) = parse(
        r#"{"type":"commandResult","requestId":3,"ok":true,"action":"cosmetics.patch","ready":true}"#,
    );

    assert_eq!(result.action, "cosmetics.patch");
    assert_eq!(result.message(), "The game completed the command.");
}

#[test]
fn try_parse_rejects_non_command_documents() {
    let cases = [
        r#"{"type":"state","inWorld":true}"#,
        r#"{"requiredAction":"commandResult","requestId":1,"ok":true}"#,
        r#"{"type":"commandResult","ok":true}"#,
        r#"{"type":"commandResult","requestId":0,"ok":true}"#,
        r#"{"type":"commandResult","requestId":-4,"ok":true}"#,
        "[]",
    ];

    for json in cases {
        let node: Value = serde_json::from_str(json).expect("fixture json");
        assert!(
            BridgeCommandResult::try_parse(&node).is_none(),
            "must reject {json}"
        );
    }
}

#[test]
fn try_parse_rejects_a_non_numeric_request_id() {
    for json in [
        r#"{"type":"commandResult","requestId":"124","ok":true}"#,
        r#"{"type":"commandResult","requestId":null,"ok":true}"#,
        r#"{"type":"commandResult","requestId":1.5,"ok":true}"#,
        r#"{"type":"commandResult","requestId":2147483648,"ok":true}"#,
    ] {
        let node: Value = serde_json::from_str(json).expect("fixture json");
        assert!(
            BridgeCommandResult::try_parse(&node).is_none(),
            "must reject {json}"
        );
    }
}

#[test]
fn the_type_discriminator_is_case_insensitive() {
    let (request_id, result) = parse(r#"{"type":"COMMANDRESULT","requestId":11,"ok":true}"#);

    assert_eq!(request_id, 11);
    assert!(result.ok);
    assert_eq!(wire_type::COMMAND_RESULT, "commandResult");
}

#[test]
fn failure_carries_action_and_error() {
    let result = BridgeCommandResult::failure(Some("join"), Some(error::NOT_CONNECTED));

    assert!(!result.ok);
    assert_eq!(result.action, "join");
    assert_eq!(result.error, "not_connected");
    assert_eq!(
        result.message(),
        "The GUI is not connected to the game yet. Inject first, then wait for the main menu."
    );
}

#[test]
fn with_action_fills_only_a_missing_action() {
    let missing = BridgeCommandResult::failure(None, Some("in_world"));
    let present = BridgeCommandResult::new(true, Some("join"), None, None, false, false, None);

    assert_eq!(missing.with_action(Some("setSession")).action, "setSession");
    assert_eq!(present.with_action(Some("setSession")).action, "join");
    let unfilled = BridgeCommandResult::failure(None, Some("in_world")).with_action(None);
    assert_eq!(unfilled.action, "", "no action to fill in");
    let empty = BridgeCommandResult::failure(None, Some("in_world")).with_action(Some(""));
    assert_eq!(empty.action, "", "a blank action is not a fill");
}

#[test]
fn message_maps_known_error_codes() {
    let cases = [
        (
            "in_world",
            "Return to the main menu before switching accounts.",
        ),
        (
            "game_thread_timeout",
            "The game did not answer in time. Stay on the main menu and retry.",
        ),
        (
            "timeout",
            "The game did not answer in time. Stay on the main menu and retry.",
        ),
        (
            "not_ready",
            "Wait until the client reaches the main menu, then retry.",
        ),
        (
            "notready",
            "Wait until the client reaches the main menu, then retry.",
        ),
        (
            "no_launch_session",
            "The client has no launch session to restore.",
        ),
        ("invalid_address", "That server address is not valid."),
        (
            "invalid_session_data",
            "The account details were rejected. Re-import the account and retry.",
        ),
        (
            "session_write_not_observed",
            "The game did not apply the new session. Stay on the main menu and retry.",
        ),
        (
            "connection_lost",
            "The connection to the game dropped before the command finished.",
        ),
        ("send_failed", "The command could not be sent to the game."),
    ];

    for (code, expected) in cases {
        let result = BridgeCommandResult::failure(Some("join"), Some(code));
        assert_eq!(result.message(), expected, "code {code}");
    }
}

#[test]
fn message_maps_free_text_not_ready_reasons() {
    for text in [
        "Session helper not ready (wait on main menu, then retry)",
        "session helper NOT READY",
    ] {
        let result = BridgeCommandResult::failure(Some("setSession"), Some(text));
        assert_eq!(
            result.message(),
            "Wait until the client reaches the main menu, then retry.",
            "text {text}"
        );
    }
}

#[test]
fn message_maps_free_text_in_world_reasons() {
    let result = BridgeCommandResult::failure(
        Some("setSession"),
        Some("Disconnect from the world before switching"),
    );

    assert_eq!(
        result.message(),
        "Return to the main menu before switching accounts."
    );
}

#[test]
fn message_unknown_code_does_not_echo_bridge_text() {
    // A future describeThrowable string can carry Java exception text; only vetted
    // sentences may reach the status bar. The raw code stays on `error` for a log.
    let result = BridgeCommandResult::failure(Some("join"), Some("joinServer exception"));

    assert_eq!(result.error, "joinServer exception");
    assert!(!result.message().contains("joinServer exception"));
    assert_eq!(
        result.message(),
        "The game refused the request. Wait until the client reaches the main menu, then retry."
    );
}

#[test]
fn message_empty_error_still_explains_the_menu_requirement() {
    let result = BridgeCommandResult::failure(Some("join"), None);

    assert!(
        result.message().contains("main menu"),
        "{}",
        result.message()
    );
}

#[test]
fn message_success_depends_on_action() {
    let cases = [
        ("getSession", "Notch", "Signed in as Notch."),
        ("getSession", "", "No account is signed in right now."),
        ("setSession", "Notch", "Switched to Notch."),
        ("restoreSession", "Notch", "Restored Notch."),
        (
            "join",
            "Notch",
            "Join started. The client is connecting to the server.",
        ),
    ];

    for (action, username, expected) in cases {
        let result =
            BridgeCommandResult::new(true, Some(action), Some(username), None, false, false, None);
        assert_eq!(result.message(), expected, "{action}/{username}");
    }
}

#[test]
fn failure_message_lookup_is_case_insensitive() {
    let upper = BridgeCommandResult::failure(Some("join"), Some("IN_WORLD"));
    let mixed = BridgeCommandResult::failure(Some("join"), Some("In_World"));

    assert_eq!(upper.message(), mixed.message());
    assert_eq!(
        upper.message(),
        "Return to the main menu before switching accounts."
    );
}
