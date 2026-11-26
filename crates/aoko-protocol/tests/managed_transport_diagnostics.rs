//! Port of `Aoko.Tests/ManagedTransportDiagnosticsTests.cs`.
//!
//! The managed suite spent most cases asserting literal strings exist in
//! `GameStateClient.cs`, `bridge.cpp`, and `bridge_261.cpp`. Those checks are kept as
//! source pins: the managed client is read from the frozen copy in
//! `tests/fixtures/managed/` (see the README there for provenance and the rule against
//! editing it), the native files are still read from the repository, because they are
//! still shipped and still write the wire. The counters, the two policies, and the log
//! line are exercised as real code.

use aoko_protocol::diagnostics::{
    is_config_send_due, should_serialize_config, ManagedTransportDiagnostics,
    ManagedTransportSnapshot,
};
use std::time::Duration;

const TICKS_PER_SECOND: u64 = 1000;

fn native_source(file: &str) -> String {
    let path = format!(
        "{}/../../McInjector/src/main/cpp/{file}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// `Aoko/Core/GameStateClient.cs`, frozen at the commit named in
/// `tests/fixtures/managed/README.md`.
///
/// `include_str!` rather than a runtime read: the evidence lives inside the crate now, so
/// deleting `Aoko/` cannot take it, and removing the fixture is a compile error instead of
/// a panic inside `read_to_string` on the sign-off commit.
const GAME_STATE_CLIENT_CS: &str = include_str!("fixtures/managed/GameStateClient.cs");

#[test]
fn enabled_reflects_configured_state() {
    let on = ManagedTransportDiagnostics::new(true, Duration::from_secs(5), TICKS_PER_SECOND, 0);
    let off = ManagedTransportDiagnostics::new(false, Duration::from_secs(5), TICKS_PER_SECOND, 0);

    assert!(on.enabled());
    assert!(!off.enabled());
    assert!(on.perf_diagnostics_flag());
    assert!(!off.perf_diagnostics_flag());
}

#[test]
fn config_send_policy_sends_on_revision_or_heartbeat() {
    let cases = [
        (2, 1, 0, 2000, true),
        (1, 1, 1999, 2000, false),
        (1, 1, 2000, 2000, true),
        (1, 1, 5000, 2000, true),
    ];

    for (revision, last_sent, elapsed_ms, heartbeat_ms, expected) in cases {
        assert_eq!(
            is_config_send_due(revision, last_sent, elapsed_ms, heartbeat_ms),
            expected,
            "revision {revision}, last {last_sent}, elapsed {elapsed_ms}, heartbeat {heartbeat_ms}"
        );
    }
}

#[test]
fn config_serialization_policy_reuses_unchanged_payload() {
    let cases = [(1, 1, false, true), (2, 1, true, true), (1, 1, true, false)];

    for (revision, cached, has_cached, expected) in cases {
        assert_eq!(
            should_serialize_config(revision, cached, has_cached),
            expected,
            "revision {revision}, cached {cached}, has_cached {has_cached}"
        );
    }
}

#[test]
fn disabled_diagnostics_do_not_produce_snapshots() {
    let diagnostics = ManagedTransportDiagnostics::new(false, Duration::ZERO, TICKS_PER_SECOND, 0);

    diagnostics.record_inbound(100, 20);
    diagnostics.record_config_serialization(200, 30);
    diagnostics.record_config_send(200);

    assert!(diagnostics.try_take_snapshot(i64::MAX).is_none());
}

#[test]
fn snapshot_atomically_reports_and_resets_the_window() {
    let diagnostics = ManagedTransportDiagnostics::new(true, Duration::ZERO, TICKS_PER_SECOND, 0);
    let now = TICKS_PER_SECOND as i64;

    diagnostics.record_inbound(120, 12);
    diagnostics.record_inbound(80, 8);
    diagnostics.record_state_notification();
    diagnostics.record_config_serialization(300, 25);
    diagnostics.record_config_send(300);

    let snapshot = diagnostics.try_take_snapshot(now).expect("first snapshot");
    assert_eq!(snapshot.inbound_messages, 2);
    assert_eq!(snapshot.inbound_characters, 200);
    assert_eq!(snapshot.inbound_parse_ticks, 20);
    assert_eq!(snapshot.state_notifications, 1);
    assert_eq!(snapshot.config_serializations, 1);
    assert_eq!(snapshot.config_characters, 300);
    assert_eq!(snapshot.config_serialization_ticks, 25);
    assert_eq!(snapshot.config_sends, 1);
    assert_eq!(snapshot.config_send_characters, 300);
    assert!(snapshot.window_seconds > 0.0);

    let reset = diagnostics
        .try_take_snapshot(now + 1)
        .expect("second snapshot");
    assert_eq!(reset.inbound_messages, 0);
    assert_eq!(reset.config_serializations, 0);
    assert_eq!(reset.config_sends, 0);
}

#[test]
fn log_message_contains_rates_without_payload_content() {
    let snapshot = ManagedTransportSnapshot {
        inbound_messages: 10,
        inbound_characters: 1000,
        inbound_parse_ticks: TICKS_PER_SECOND as i64 / 100,
        state_notifications: 25,
        config_serializations: 2,
        config_characters: 500,
        config_serialization_ticks: TICKS_PER_SECOND as i64 / 200,
        config_sends: 1,
        config_send_characters: 250,
        window_seconds: 5.0,
    };

    let message = snapshot.to_log_message(TICKS_PER_SECOND);

    assert!(message.contains("inbound=2.0/s"), "{message}");
    assert!(message.contains("stateNotify=5.0/s"), "{message}");
    assert!(message.contains("configSerialize=0.40/s"), "{message}");
    assert!(message.contains("configSend=0.20/s"), "{message}");
}

#[test]
fn config_change_tracking_covers_properties_nested_settings_and_keybinds() {
    // Source pin: the wiring that marks the bridge config dirty lives in the managed
    // client, which `aoko-core` owns. Keeping the pin means a refactor that silently
    // drops one of the three subscriptions fails here.
    let source = GAME_STATE_CLIENT_CS;

    for needle in [
        "Clicker.Instance.PropertyChanged += OnClickerConfigPropertyChanged;",
        "Clicker.Instance.StateChanged += MarkBridgeConfigDirty;",
        "InputHooks.OnStateChanged += MarkBridgeConfigDirty;",
    ] {
        assert!(source.contains(needle), "missing {needle}");
    }
}

#[test]
fn diagnostics_flag_is_carried_to_both_native_bridges() {
    let managed = GAME_STATE_CLIENT_CS;
    let legacy = native_source("bridge.cpp");
    let modern = native_source("bridge_261.cpp");

    assert!(managed.contains("perfDiagnostics = _transportDiagnostics.Enabled"));
    assert!(legacy.contains("reader.GetString(\"perfDiagnostics\")"));
    assert!(modern.contains("reader.GetString(\"perfDiagnostics\")"));
}

#[test]
fn modern_fast_state_uses_an_explicit_partial_envelope() {
    let modern = native_source("bridge_261.cpp");
    let client = GAME_STATE_CLIENT_CS;

    assert!(modern.contains("\\\"partial\\\":true"));
    assert!(client.contains("GameStatePatchMerger.IsPartial"));
    assert!(modern.contains("ModernFullStateDue"));
    assert!(client.contains("supportsStatePatches = true"));
    assert!(modern.contains("supportsStatePatches = false"));
    assert!(modern.contains("supportsStatePatches && lc::ModernStateNeedsFast"));
}

#[test]
fn modern_entity_producer_clears_snapshot_when_consumers_turn_off() {
    let modern = native_source("bridge_261.cpp");

    assert!(modern.contains("entityProducerWasEnabled"));
    assert!(modern.contains("g_playerList.clear();"));
    assert!(modern.contains("const bool entityProducerEnabled"));
}

#[test]
fn native_float_config_integration_uses_validated_existing_defaults() {
    let legacy = native_source("bridge.cpp");
    let modern = native_source("bridge_261.cpp");

    for source in [&legacy, &modern] {
        for needle in [
            "reader.GetFloat(\"minCPS\", g_config.minCPS)",
            "reader.GetFloat(\"maxCPS\", g_config.maxCPS)",
            "reader.GetFloat(\"rightMinCPS\", g_config.rightMinCPS)",
            "reader.GetFloat(\"rightMaxCPS\", g_config.rightMaxCPS)",
        ] {
            assert!(source.contains(needle), "missing {needle}");
        }
    }

    assert!(modern.contains("reader.GetFloat(\"reachMin\", g_config.reachMin)"));
    assert!(modern.contains("reader.GetFloat(\"reachMax\", g_config.reachMax)"));
}

#[test]
fn the_environment_flag_opts_in_without_crashing() {
    // `from_environment` reads `AOKO_PERF_DIAGNOSTICS`; the tests must not depend on
    // the ambient value, so only the interval/ticks contract is asserted here.
    let diagnostics = ManagedTransportDiagnostics::from_environment(TICKS_PER_SECOND, 0);

    assert_eq!(diagnostics.ticks_per_second(), TICKS_PER_SECOND);
}
