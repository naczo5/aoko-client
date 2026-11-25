//! Port of `Aoko.Tests/GameStateClientSeamTests.cs` plus the decode seam itself.
//!
//! The managed suite pinned three pure helpers (`NormalizeDetectedVersion`,
//! `ResolveBridgeDllName`, `ModuleListStyleToIndex`). Its fourth case,
//! `ProcessCommandLine.TryParseVersion`, reads a live process command line and lives
//! in `aoko-platform`, so it is not here. Everything else drives the real
//! `TransportClient` over the in-memory transport, which is what the managed
//! `IGameStateTransport` seam used to do with a fake.

use aoko_protocol::capabilities::BridgeCapabilities;
use aoko_protocol::codec::{Decoder, Inbound, Pump, TransportClient};
use aoko_protocol::commands::AccountCommand;
use aoko_protocol::config_payload::{
    module_list_style_to_index, normalize_detected_version, resolve_bridge_dll_name,
};
use aoko_protocol::diagnostics::ManagedTransportDiagnostics;
use aoko_protocol::transport::memory;
use std::time::Duration;

#[test]
fn normalize_detected_version_returns_expected_value() {
    let cases: [(Option<&str>, Option<&str>); 13] = [
        (Some("26.1"), Some("26.1")),
        (Some("26.2"), Some("26.2")),
        (Some("26.3"), Some("26.3")),
        (Some("26.3.1"), Some("26.3")),
        (Some("26.1.2"), Some("26.1")),
        (Some("26"), Some("26.1")),
        (Some("minecraft 26"), Some("26.1")),
        (Some("1.21.4"), Some("1.21")),
        (Some("1.8.9"), Some("1.8.9")),
        (Some("1.8.8"), Some("1.8.9")),
        (Some("2.26.0"), None),
        (Some("unknown"), None),
        (Some(""), None),
    ];

    for (input, expected) in cases {
        assert_eq!(
            normalize_detected_version(input),
            expected,
            "input {input:?}"
        );
    }

    assert_eq!(normalize_detected_version(None), None);
}

#[test]
fn normalize_detected_version_ignores_casing_and_padding() {
    assert_eq!(normalize_detected_version(Some("  26.2  ")), Some("26.2"));
    assert_eq!(
        normalize_detected_version(Some("MINECRAFT 26")),
        Some("26.1")
    );
    assert_eq!(
        normalize_detected_version(Some("1.8.9-forge")),
        Some("1.8.9")
    );
}

#[test]
fn resolve_bridge_dll_name_maps_modern_versions_to_261() {
    let cases = [
        ("26.3", "bridge_261.dll"),
        ("26.2", "bridge_261.dll"),
        ("26.1", "bridge_261.dll"),
        ("1.21", "bridge_261.dll"),
        ("1.8.9", "bridge.dll"),
        ("auto", "bridge.dll"),
    ];

    for (version, expected) in cases {
        assert_eq!(
            resolve_bridge_dll_name(version),
            expected,
            "version {version}"
        );
    }
}

#[test]
fn resolve_bridge_dll_name_falls_back_on_empty_input() {
    assert_eq!(resolve_bridge_dll_name(""), "bridge.dll");
    assert_eq!(resolve_bridge_dll_name("   "), "bridge.dll");
}

#[test]
fn module_list_style_to_index_maps_styles_safely() {
    let cases = [
        ("Default", 0),
        ("Minimal", 1),
        ("Outlined", 2),
        ("Glass", 3),
        ("Bold", 4),
        ("unknown-style", 0),
        ("", 0),
    ];

    for (style, expected) in cases {
        assert_eq!(
            module_list_style_to_index(Some(style)),
            expected,
            "style {style}"
        );
    }

    assert_eq!(module_list_style_to_index(None), 0);
}

#[test]
fn module_list_style_lookup_ignores_casing() {
    assert_eq!(module_list_style_to_index(Some("glass")), 3);
    assert_eq!(module_list_style_to_index(Some(" BOLD ")), 4);
}

fn client() -> (MemoryPeerLike, TransportClient<memory::MemoryTransport>) {
    let (transport, peer) = memory::duplex();
    let client = TransportClient::new(
        transport,
        Decoder::with_injected_version("26.2"),
        ManagedTransportDiagnostics::new(false, Duration::from_secs(1), 20, 0),
    );
    (MemoryPeerLike(peer), client)
}

/// Thin wrapper so the helper above reads clearly at call sites.
struct MemoryPeerLike(memory::MemoryPeer);

impl MemoryPeerLike {
    fn feed(&self, bytes: &[u8]) {
        self.0.feed(bytes);
    }

    fn sent_lines(&self) -> Vec<String> {
        self.0.sent_lines()
    }
}

#[test]
fn pump_decodes_a_state_line_fed_by_the_peer() {
    let (peer, mut client) = client();
    peer.feed(b"{\"type\":\"state\",\"inWorld\":true,\"health\":20.0}\n");

    match client.pump().expect("pump") {
        Pump::Frame(Inbound::State { state, hud_layout }) => {
            assert!(state.in_world);
            assert_eq!(state.health, 20.0);
            assert!(hud_layout.is_none());
        }
        other => panic!("unexpected frame: {other:?}"),
    }
}

#[test]
fn pump_reports_end_of_stream_as_closed() {
    let (_peer, mut client) = client();

    assert_eq!(client.pump().expect("pump"), Pump::Closed);
}

#[test]
fn pump_reports_an_oversized_line_without_killing_the_client() {
    let (peer, mut client) = client();
    let mut line = b"{\"type\":\"state\",\"actionBar\":\"".to_vec();
    line.extend(std::iter::repeat_n(
        b'x',
        aoko_protocol::transport::MAX_INBOUND_LINE_BYTES,
    ));
    line.push(b'\n');
    peer.feed(&line);
    peer.feed(b"{\"type\":\"state\",\"health\":1.0}\n");

    assert_eq!(client.pump().expect("pump"), Pump::TooLong);
    match client.pump().expect("pump") {
        Pump::Frame(Inbound::State { state, .. }) => assert_eq!(state.health, 1.0),
        other => panic!("unexpected frame: {other:?}"),
    }
}

#[test]
fn sending_an_account_command_writes_one_line_to_the_peer() {
    let (peer, mut client) = client();

    client
        .send_account_command(7, &AccountCommand::get_session())
        .expect("send");

    assert_eq!(
        peer.sent_lines(),
        ["{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":7}\n"]
    );
}

#[test]
fn sending_a_server_command_writes_one_line_to_the_peer() {
    let (peer, mut client) = client();

    client
        .send_server_command(8, "mc.example.com", 25565)
        .expect("send");

    let sent = peer.sent_lines();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].starts_with("{\"type\":\"serverCommand\",\"action\":\"join\""));
    assert!(sent[0].ends_with("}\n"));
}

#[test]
fn the_client_keeps_the_last_state_between_pumps() {
    let (peer, mut client) = client();
    peer.feed(b"{\"type\":\"state\",\"health\":20.0}\n");
    peer.feed(b"{\"type\":\"statePatch\",\"health\":11.0}\n");

    assert!(matches!(
        client.pump().expect("pump"),
        Pump::Frame(Inbound::State { .. })
    ));
    assert!(matches!(
        client.pump().expect("pump"),
        Pump::Frame(Inbound::State { .. })
    ));

    assert_eq!(client.decoder().state().health, 11.0);
}

#[test]
fn capabilities_from_the_peer_replace_the_injected_fallback() {
    let (peer, mut client) = client();
    // The injected 26.2 fallback knows triggerbot but not a future module.
    assert!(!client
        .decoder()
        .capabilities()
        .supports_module("future_module"));

    peer.feed(b"{\"type\":\"capabilities\",\"modules\":[\"FUTURE_MODULE\"]}\n");

    match client.pump().expect("pump") {
        Pump::Frame(Inbound::Capabilities(caps)) => {
            assert!(caps.supports_module("future_module"))
        }
        other => panic!("unexpected frame: {other:?}"),
    }
    // The payload replaces the fallback wholesale, so a module the fallback knew and
    // the payload omits is gone.
    assert!(client
        .decoder()
        .capabilities()
        .supports_module("future_module"));
    assert!(!client
        .decoder()
        .capabilities()
        .supports_module("triggerbot"));
}

#[test]
fn the_injected_version_selects_the_bridge_family() {
    let modern = Decoder::with_injected_version("26.2");
    let legacy = Decoder::with_injected_version("1.8.9");

    assert!(modern.capabilities().supports_module("triggerbot"));
    assert!(!legacy.capabilities().supports_module("triggerbot"));
    assert_eq!(modern.injected_version(), "26.2");
    assert_eq!(legacy.injected_version(), "1.8.9");
}

#[test]
fn an_unknown_injected_version_falls_back_to_the_legacy_surface() {
    let unknown = Decoder::with_injected_version("not-a-version");

    assert_eq!(
        unknown.capabilities(),
        &BridgeCapabilities::for_version_fallback(None)
    );
}
