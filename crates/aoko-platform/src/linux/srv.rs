//! System DNS resolvers from `/etc/resolv.conf`.
//!
//! The managed `CollectSystemDnsServers` walked `NetworkInterface` objects; on
//! Linux the resolvers the system is actually configured with are the
//! `nameserver` lines, which is the same list `getaddrinfo` consults. Interfaces
//! are not enumerated because every resolver a `resolvconf`-managed system uses
//! ends up in this file, including the loopback stub of a local cache
//! (`systemd-resolved` publishes `127.0.0.53` here).
//!
//! A missing or unreadable file yields an empty list, which makes the caller fall
//! back to the public resolvers exactly as the managed code did when no adapter
//! reported an address.

use std::net::IpAddr;

/// Path the C library reads.
pub const RESOLV_CONF: &str = "/etc/resolv.conf";

/// Resolvers this system is configured with, in file order.
///
/// Unreadable or absent configuration is an empty list, never an error: SRV is
/// optional and the caller has a fallback.
#[must_use]
pub fn collect_system_dns_servers() -> Vec<String> {
    match std::fs::read_to_string(RESOLV_CONF) {
        Ok(text) => parse_resolv_conf(&text),
        Err(_) => Vec::new(),
    }
}

/// Every distinct `nameserver` address in `text`, in file order.
///
/// Blank lines, comments and every other directive (`search`, `options`, ...) are
/// skipped, as is the unspecified address, which matches the managed
/// `IPAddress.Any` / `IPv6Any` filter.
#[must_use]
pub fn parse_resolv_conf(text: &str) -> Vec<String> {
    let mut servers: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.split(['#', ';']).next().unwrap_or_default().trim();
        let mut parts = line.split_whitespace();
        if !parts
            .next()
            .is_some_and(|keyword| keyword.eq_ignore_ascii_case("nameserver"))
        {
            continue;
        }

        let Some(address) = parts.next() else {
            continue;
        };
        // `resolv.conf` may carry a scope suffix on link-local IPv6
        // (`fe80::1%eth0`), which is not part of the address.
        let address = address.split('%').next().unwrap_or(address);
        let Ok(parsed) = address.parse::<IpAddr>() else {
            continue;
        };
        if parsed.is_unspecified() {
            continue;
        }

        let text = parsed.to_string();
        if !servers.contains(&text) {
            servers.push(text);
        }
    }
    servers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nameserver_lines_are_collected_in_order() {
        let text = "\
# a comment
nameserver 192.168.1.1
nameserver 2001:4860:4860::8888
search lan
options edns0
";
        assert_eq!(
            parse_resolv_conf(text),
            ["192.168.1.1", "2001:4860:4860::8888"]
        );
    }

    #[test]
    fn a_trailing_comment_and_extra_whitespace_are_tolerated() {
        let text = "  nameserver\t10.0.0.1   # dhcp\nnameserver 10.0.0.2 ; second\n";
        assert_eq!(parse_resolv_conf(text), ["10.0.0.1", "10.0.0.2"]);
    }

    #[test]
    fn duplicates_and_the_unspecified_address_are_dropped() {
        let text = "\
nameserver 8.8.8.8
nameserver 8.8.8.8
nameserver 0.0.0.0
nameserver ::
";
        assert_eq!(parse_resolv_conf(text), ["8.8.8.8"]);
    }

    #[test]
    fn garbage_is_skipped_rather_than_guessed() {
        let text = "\
nameserver
nameserver not-an-address
nameserver 1.1.1.1
";
        assert_eq!(parse_resolv_conf(text), ["1.1.1.1"]);
    }

    #[test]
    fn an_ipv6_link_local_scope_is_stripped() {
        assert_eq!(
            parse_resolv_conf("nameserver fe80::1%eth0\n"),
            ["fe80::1".to_string()]
        );
    }

    #[test]
    fn an_empty_file_yields_nothing() {
        assert!(parse_resolv_conf("").is_empty());
        assert!(parse_resolv_conf("search only.example\n").is_empty());
    }

    #[test]
    fn the_live_file_parses_into_the_same_answer_the_parser_gives() {
        // Skipped, not passed, when there is no file: `.all()` over an empty list
        // is vacuously true, which is how this test used to look green on a box
        // with no /etc/resolv.conf. The parser itself is covered above; what only
        // the live file can prove is that `collect` adds nothing to it.
        let Ok(text) = std::fs::read_to_string(RESOLV_CONF) else {
            eprintln!("skipped: {RESOLV_CONF} is not readable in this environment");
            return;
        };

        let servers = collect_system_dns_servers();
        assert_eq!(
            servers,
            parse_resolv_conf(&text),
            "collect must be the read plus the parser, nothing else"
        );

        // And each answer is a real address, filtered and deduplicated.
        let mut seen = std::collections::HashSet::new();
        for server in &servers {
            let address: IpAddr = server
                .parse()
                .unwrap_or_else(|err| panic!("{server:?} is not an address: {err}"));
            assert!(!address.is_unspecified(), "{address} must be filtered");
            assert!(seen.insert(server.clone()), "{server} appears twice");
        }
    }
}
