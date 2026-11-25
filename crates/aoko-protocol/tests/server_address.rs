//! Port of `Aoko.Tests/ServerAddressTests.cs`.
//!
//! `ResolveForPingAsync` performs the UDP lookup, so those two cases become
//! `try_parse`/`validate_target` here; the lookup itself lives in `aoko-platform`, and
//! the SRV byte codec it will call is exercised directly.

use aoko_protocol::servers::{
    build_srv_query, dns_port, resolve_dns_servers, srv_query_name, try_build_join_request,
    try_read_srv, validate_target, ServerAddress, DEFAULT_PORT, INVALID_ADDRESS_MESSAGE,
    MAXIMUM_HOST_LENGTH, SRV_TRANSACTION_ID,
};

fn address(host: &str, port: i32, port_specified: bool) -> ServerAddress {
    ServerAddress::new(host, port, port_specified)
}

#[test]
fn try_parse_accepts_supported_forms() {
    let cases = [
        ("play.hypixel.net", "play.hypixel.net", 25565, false),
        ("  mc.example.com:25566  ", "mc.example.com", 25566, true),
        ("[::1]:25565", "::1", 25565, true),
        ("[2001:db8::1]", "2001:db8::1", 25565, false),
        ("::1", "::1", 25565, false),
        ("fe80::1:2", "fe80::1:2", 25565, false),
        ("example.com:1", "example.com", 1, true),
        ("example.com:65535", "example.com", 65535, true),
    ];

    for (text, host, port, port_specified) in cases {
        let parsed = ServerAddress::try_parse(Some(text)).unwrap_or_else(|| panic!("{text:?}"));

        assert_eq!(parsed.host, host, "{text:?}");
        assert_eq!(parsed.port, port, "{text:?}");
        assert_eq!(parsed.port_specified, port_specified, "{text:?}");
    }
}

#[test]
fn try_parse_rejects_invalid_forms() {
    let cases = [
        Some(""),
        Some("   "),
        None,
        Some("example.com:0"),
        Some("example.com:65536"),
        Some("example.com:70000"),
        Some("example.com:-1"),
        Some("example.com:abc"),
        Some("example.com: 25565"),
        Some("[::1"),
        Some("[]"),
        Some("[]:25565"),
        Some("[::1]x"),
        Some("[::1]:0"),
        Some("a b"),
        Some("host/path"),
    ];

    for text in cases {
        assert!(
            ServerAddress::try_parse(text).is_none(),
            "must reject {text:?}"
        );
    }
}

#[test]
fn try_parse_keeps_a_trailing_colon_as_a_bare_host() {
    // A colon at the last position never splits into host:port: the reference
    // `MinecraftServerAddress` requires a character after the colon.
    let parsed = ServerAddress::try_parse(Some("example.com:")).expect("accepted");

    assert_eq!(parsed.host, "example.com:");
    assert_eq!(parsed.port, 25565);
    assert!(!parsed.port_specified);
}

#[test]
fn try_parse_enforces_the_256_character_cap() {
    assert!(ServerAddress::try_parse(Some(&"a".repeat(256))).is_some());
    assert!(ServerAddress::try_parse(Some(&"a".repeat(257))).is_none());

    let at_cap = format!("{}:25565", "a".repeat(250));
    assert_eq!(at_cap.len(), 256);
    let parsed = ServerAddress::try_parse(Some(&at_cap)).expect("at cap");
    assert_eq!(parsed.host.len(), 250);

    assert!(ServerAddress::try_parse(Some(&format!("{}:25565", "a".repeat(251)))).is_none());
}

#[test]
fn display_formats_port_and_ipv6() {
    let cases = [
        ("host", 25565, true, "host:25565"),
        ("host", 25565, false, "host"),
        ("host", 25566, false, "host:25566"),
        ("::1", 25565, false, "::1"),
        ("::1", 25566, false, "[::1]:25566"),
    ];

    for (host, port, port_specified, expected) in cases {
        assert_eq!(
            address(host, port, port_specified).display(),
            expected,
            "{host}:{port}/{port_specified}"
        );
    }
}

#[test]
fn resolve_for_ping_keeps_an_explicit_port() {
    // The lookup half is `aoko-platform`; the parsed result is what it resolves to
    // when the host is not an SRV name.
    let resolved = ServerAddress::try_parse(Some("mc.example.com:25566")).expect("accepted");

    assert_eq!(resolved.host, "mc.example.com");
    assert_eq!(resolved.port, 25566);
    assert!(resolved.port_specified);
}

#[test]
fn resolve_for_ping_rejects_garbage_with_the_user_facing_message() {
    // Exact match: the framework's "(Parameter 'text')" suffix must not reach the
    // status bar, so the message is this crate's constant, not a formatted error.
    let target = validate_target(Some("bad host"));

    assert!(!target.ok);
    assert!(target.address.is_none());
    assert_eq!(target.error, INVALID_ADDRESS_MESSAGE);
    assert_eq!(
        INVALID_ADDRESS_MESSAGE,
        "Enter a server as host or host:port."
    );
}

#[test]
fn build_srv_query_encodes_the_minecraft_service_name() {
    let query = build_srv_query("_minecraft._tcp.play.hypixel.net");

    assert_eq!(query[0], 0x1C);
    assert_eq!(query[1], 0x0D);
    assert_eq!(query[2], 0x01);
    assert_eq!(query[3], 0x00);
    assert_eq!(query[5], 1);
    assert_eq!(query[7], 0);
    assert_eq!(query[12], 10);
    assert_eq!(&query[13..23], b"_minecraft");
    assert_eq!(query[23], 4);
    assert_eq!(&query[24..28], b"_tcp");

    let tail = query.len() - 5;
    assert_eq!(query[tail], 0);
    assert_eq!(query[tail + 1], 0x00);
    assert_eq!(query[tail + 2], 0x21);
    assert_eq!(query[tail + 3], 0x00);
    assert_eq!(query[tail + 4], 0x01);
    assert_eq!(SRV_TRANSACTION_ID, 0x1C0D);
}

#[test]
fn try_read_srv_parses_a_literal_target() {
    let response = dns_response("proxy.example.com", 25566, false);

    let (target, port) = try_read_srv(&response, None).expect("parsed");

    assert_eq!(target, "proxy.example.com");
    assert_eq!(port, 25566);
}

#[test]
fn try_read_srv_skips_a_cname_and_follows_a_compression_pointer() {
    let response = dns_response("proxy.example.com", 25565, true);

    let (target, port) = try_read_srv(&response, None).expect("parsed");

    assert_eq!(target, "proxy.example.com");
    assert_eq!(port, 25565);
}

#[test]
fn try_read_srv_rejects_truncated_responses() {
    assert!(try_read_srv(&[], None).is_none());
    assert!(try_read_srv(&[0; 11], None).is_none());

    let response = dns_response("proxy.example.com", 25566, false);
    assert!(try_read_srv(&response[..response.len() - 4], None).is_none());
}

#[test]
fn try_read_srv_accepts_only_the_answer_to_our_own_question() {
    let response = dns_response("proxy.example.com", 25566, false);
    let query_name = "_minecraft._tcp.example.com";

    let (target, port) = try_read_srv(&response, Some(query_name)).expect("parsed");
    assert_eq!(target, "proxy.example.com");
    assert_eq!(port, 25566);

    // Somebody else's answer, or one for another name, must not be cached as this server.
    assert!(try_read_srv(&response, Some("_minecraft._tcp.other.net")).is_none());

    let mut foreign_transaction = response.clone();
    foreign_transaction[0] ^= 0xFF;
    assert!(try_read_srv(&foreign_transaction, Some(query_name)).is_none());

    let mut not_a_response = response.clone();
    not_a_response[2] &= 0x7F;
    assert!(try_read_srv(&not_a_response, Some(query_name)).is_none());
}

#[test]
fn srv_query_name_builds_the_minecraft_service_name() {
    assert_eq!(
        srv_query_name("play.hypixel.net"),
        "_minecraft._tcp.play.hypixel.net"
    );
}

#[test]
fn validate_target_returns_the_user_facing_message() {
    let ok = validate_target(Some("play.hypixel.net"));
    assert!(ok.ok);
    assert_eq!(ok.error, "");
    assert_eq!(ok.address.expect("address").display(), "play.hypixel.net");

    let bad = validate_target(Some("bad host"));
    assert!(!bad.ok);
    assert_eq!(bad.error, INVALID_ADDRESS_MESSAGE);

    let empty = validate_target(None);
    assert!(!empty.ok);
    assert_eq!(empty.error, INVALID_ADDRESS_MESSAGE);
}

#[test]
fn try_build_join_request_returns_the_bridge_host_and_port() {
    let (host, port) =
        try_build_join_request(&address("mc.example.com", 25566, true)).expect("accepted");
    assert_eq!(host, "mc.example.com");
    assert_eq!(port, 25566);

    let (trimmed, _) =
        try_build_join_request(&address("  padded.example.com ", 25565, false)).expect("accepted");
    assert_eq!(trimmed, "padded.example.com");
}

#[test]
fn try_build_join_request_rejects_out_of_range_targets() {
    assert!(try_build_join_request(&address("", 0, false)).is_none());
    assert!(try_build_join_request(&address("host", 0, true)).is_none());
    assert!(try_build_join_request(&address("host", 65536, true)).is_none());
    assert!(try_build_join_request(&address(&"a".repeat(257), 25565, false)).is_none());
    assert!(try_build_join_request(&address(&"a".repeat(256), 25565, false)).is_some());
    assert_eq!(MAXIMUM_HOST_LENGTH, 256);
    assert_eq!(DEFAULT_PORT, 25565);
}

#[test]
fn dns_servers_fall_back_when_the_system_list_is_empty() {
    assert_eq!(resolve_dns_servers(&[]), ["8.8.8.8", "1.1.1.1"]);
    assert_eq!(
        resolve_dns_servers(&["10.0.0.1".to_owned()]),
        ["10.0.0.1".to_owned()]
    );
    assert_eq!(dns_port(), 53);
}

#[test]
fn port_u16_reports_out_of_range_ports() {
    assert_eq!(address("host", 25565, false).port_u16(), Some(25565));
    assert_eq!(address("host", 1, true).port_u16(), Some(1));
    assert_eq!(address("host", 65535, true).port_u16(), Some(65535));
    assert_eq!(address("host", 0, true).port_u16(), None);
    assert_eq!(address("host", 65536, true).port_u16(), None);
    assert_eq!(address("host", -1, true).port_u16(), None);
}

/// Builds a DNS response with one question and an optional CNAME ahead of the SRV
/// answer — the same generator the managed suite used.
fn dns_response(target: &str, port: u16, leading_cname: bool) -> Vec<u8> {
    struct Writer {
        bytes: Vec<u8>,
    }

    impl Writer {
        fn position(&self) -> usize {
            self.bytes.len()
        }

        fn u16(&mut self, value: u16) {
            self.bytes.extend_from_slice(&value.to_be_bytes());
        }

        fn u32(&mut self, value: u32) {
            self.bytes.extend_from_slice(&value.to_be_bytes());
        }

        fn name(&mut self, labels: &[&str]) {
            for label in labels {
                self.bytes.push(label.len() as u8);
                self.bytes.extend_from_slice(label.as_bytes());
            }
            self.bytes.push(0);
        }

        fn pointer(&mut self, offset: usize) {
            self.u16(0xC000 | (offset as u16 & 0x3FFF));
        }

        fn reserve_u16(&mut self) -> usize {
            let position = self.bytes.len();
            self.u16(0);
            position
        }

        fn patch_u16(&mut self, position: usize, value: usize) {
            let bytes = (value as u16).to_be_bytes();
            self.bytes[position] = bytes[0];
            self.bytes[position + 1] = bytes[1];
        }
    }

    let mut writer = Writer { bytes: Vec::new() };
    writer.u16(SRV_TRANSACTION_ID);
    writer.u16(0x8180);
    writer.u16(1);
    writer.u16(if leading_cname { 2 } else { 1 });
    writer.u16(0);
    writer.u16(0);

    let name_start = writer.position();
    writer.name(&["_minecraft", "_tcp", "example.com"]);
    writer.u16(33);
    writer.u16(1);

    let mut cname_start = 0;
    if leading_cname {
        writer.pointer(name_start);
        writer.u16(5);
        writer.u16(1);
        writer.u32(0);
        let cname_length = writer.reserve_u16();
        cname_start = writer.position();
        writer.name(&["proxy", "example.com"]);
        let length = writer.position() - cname_start;
        writer.patch_u16(cname_length, length);
    }

    writer.pointer(name_start);
    writer.u16(33);
    writer.u16(1);
    writer.u32(0);
    let rdata_length = writer.reserve_u16();
    let rdata_start = writer.position();
    writer.u16(0);
    writer.u16(0);
    writer.u16(port);
    if leading_cname {
        writer.pointer(cname_start);
    } else {
        let labels: Vec<&str> = target.split('.').collect();
        writer.name(&labels);
    }
    let length = writer.position() - rdata_start;
    writer.patch_u16(rdata_length, length);

    writer.bytes
}
