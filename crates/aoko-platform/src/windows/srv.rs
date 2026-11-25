//! System DNS resolvers through `GetAdaptersAddresses`.
//!
//! This follows the managed `CollectSystemDnsServers`: walk the adapters, keep the
//! ones whose operational status is `Up`, take their DNS server addresses in the
//! order the OS lists them, skip the unspecified address, de-duplicate, and stop at
//! two resolvers. `GetNetworkParams` would be a shorter API but it reports one
//! machine-wide list without the Up filter, so it would not be the same answer.
//!
//! The address text comes from the raw `sockaddr` bytes rather than from
//! `SOCKADDR_IN`/`SOCKADDR_IN6`: the family is checked first and the address sits at
//! a fixed offset in each layout, so the two reads below need no further `WinSock`
//! types. A length that does not cover the address is refused instead of read.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    IP_ADAPTER_DNS_SERVER_ADDRESS_XP,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC};

/// Most resolvers a single lookup asks, matching `MaximumSrvServers`.
const MAXIMUM_SRV_SERVERS: usize = 2;

/// Starting buffer size; the call writes back the size it needs on overflow.
const INITIAL_BUFFER_BYTES: u32 = 16 * 1024;

/// `ERROR_BUFFER_OVERFLOW`: the buffer was too small, so retry with the size the
/// call wrote back.
const ERROR_BUFFER_OVERFLOW: u32 = 111;

/// How many times the buffer may be grown before giving up.
const MAXIMUM_ATTEMPTS: usize = 3;

/// Bytes `sockaddr_in` reserves before `sin_addr`.
const SOCKADDR_IN_ADDRESS_OFFSET: usize = 4;
/// Bytes `sockaddr_in6` reserves before `sin6_addr`.
const SOCKADDR_IN6_ADDRESS_OFFSET: usize = 8;

/// Resolvers this machine is configured with, in adapter order.
///
/// Any failure -- no adapters, a denied call, an allocation that would not grow --
/// yields an empty list, and the caller then uses the public fallback. That is what
/// the managed `catch` blocks achieved.
#[must_use]
pub fn collect_system_dns_servers() -> Vec<String> {
    let mut size = INITIAL_BUFFER_BYTES;
    let mut buffer: Vec<u8> = vec![0; usize::try_from(size).unwrap_or(0)];
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;

    for _ in 0..MAXIMUM_ATTEMPTS {
        // SAFETY: `buffer` is live for exactly `size` bytes and is handed over as an
        // adapter list; the call both reads the length and overwrites it with the
        // size it needs on overflow.
        let result = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC.0),
                flags,
                None,
                Some(buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()),
                &mut size,
            )
        };

        if result == 0 {
            // SAFETY: a zero return means the call filled `buffer` with a properly
            // aligned adapter list, which stays valid while `buffer` is alive.
            return unsafe { walk_adapters(buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()) };
        }
        if result != ERROR_BUFFER_OVERFLOW {
            return Vec::new();
        }
        buffer = vec![0; usize::try_from(size).unwrap_or(0)];
    }

    Vec::new()
}

/// Walks the adapter list, keeping the DNS addresses of adapters that are up.
///
/// # Safety
/// `first` must be null, or point at a valid `IP_ADAPTER_ADDRESSES_LH` list whose
/// `Next` chain and `FirstDnsServerAddress` chains stay readable for the duration of
/// the call.
unsafe fn walk_adapters(first: *const IP_ADAPTER_ADDRESSES_LH) -> Vec<String> {
    let mut servers: Vec<String> = Vec::with_capacity(MAXIMUM_SRV_SERVERS);
    let mut adapter = first;

    // SAFETY: the caller guarantees the chain is readable; each `Next` is either
    // null or the next element of the same list.
    while let Some(current) = unsafe { adapter.as_ref() } {
        adapter = current.Next;

        if current.OperStatus != IfOperStatusUp {
            continue;
        }

        let mut dns = current.FirstDnsServerAddress;
        // SAFETY: same list invariant as above, for the per-adapter DNS chain.
        while let Some(entry) = unsafe { dns.as_ref() } {
            dns = entry.Next;

            // SAFETY: `entry` is a live element of the chain, and `sockaddr_text`
            // re-checks the recorded sockaddr length before reading.
            if let Some(address) = unsafe { sockaddr_text(entry) } {
                if servers.contains(&address) {
                    continue;
                }
                servers.push(address);
                if servers.len() == MAXIMUM_SRV_SERVERS {
                    return servers;
                }
            }
        }
    }

    servers
}

/// The adapter's DNS address as text, for the two families this host can query.
///
/// Returns `None` for another family, for a null or short sockaddr, and for the
/// unspecified address, which is not a resolver.
///
/// # Safety
/// `entry` must point at a live `IP_ADAPTER_DNS_SERVER_ADDRESS_XP` whose
/// `Address.lpSockaddr` is either null or points at `iSockaddrLength` readable
/// bytes.
unsafe fn sockaddr_text(entry: &IP_ADAPTER_DNS_SERVER_ADDRESS_XP) -> Option<String> {
    let sockaddr = entry.Address.lpSockaddr;
    if sockaddr.is_null() {
        return None;
    }
    let available = usize::try_from(entry.Address.iSockaddrLength).ok()?;
    if available < SOCKADDR_IN_ADDRESS_OFFSET {
        return None;
    }

    let base = sockaddr.cast::<u8>();
    // SAFETY: at least two bytes are readable, which is the width of `sa_family`.
    let family = u16::from_ne_bytes(unsafe { [*base, *base.add(1)] });

    if family == AF_INET.0 {
        let octets = read_address_bytes(base, SOCKADDR_IN_ADDRESS_OFFSET, available)?;
        return resolver_text(IpAddr::V4(Ipv4Addr::from(octets)));
    }
    if family == AF_INET6.0 {
        let octets = read_address_bytes(base, SOCKADDR_IN6_ADDRESS_OFFSET, available)?;
        // A link-local address keeps its scope id in a separate field, which is not
        // part of the resolver string.
        return resolver_text(IpAddr::V6(Ipv6Addr::from(octets)));
    }

    None
}

/// Copies `N` bytes at `offset` out of `base`, or `None` when the recorded sockaddr
/// length does not cover them.
fn read_address_bytes<const N: usize>(
    base: *const u8,
    offset: usize,
    available: usize,
) -> Option<[u8; N]> {
    if available < offset + N {
        return None;
    }
    let mut octets = [0u8; N];
    // SAFETY: the check above established that `offset + N` bytes are readable from
    // `base`, and `octets` is exactly `N` bytes of writable stack.
    unsafe {
        core::ptr::copy_nonoverlapping(base.add(offset), octets.as_mut_ptr(), N);
    }
    Some(octets)
}

/// Whether an address may be used as a resolver, matching the managed
/// `IPAddress.Any` / `IPv6Any` filter.
#[must_use]
pub fn is_usable_resolver(address: IpAddr) -> bool {
    !address.is_unspecified()
}

/// The resolver string for an address, or `None` when it cannot be a resolver.
fn resolver_text(address: IpAddr) -> Option<String> {
    is_usable_resolver(address).then(|| address.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Networking::WinSock::SOCKADDR;

    fn entry_for(storage: &mut Vec<u8>) -> IP_ADAPTER_DNS_SERVER_ADDRESS_XP {
        let mut entry = IP_ADAPTER_DNS_SERVER_ADDRESS_XP::default();
        entry.Address.lpSockaddr = storage.as_mut_ptr().cast::<SOCKADDR>();
        entry.Address.iSockaddrLength = i32::try_from(storage.len()).unwrap_or(0);
        entry
    }

    #[test]
    fn an_ipv4_sockaddr_is_read_as_a_dotted_quad() {
        // sockaddr_in: family, port, then sin_addr at offset 4.
        let mut storage = vec![0u8; 16];
        storage[0..2].copy_from_slice(&AF_INET.0.to_ne_bytes());
        storage[4..8].copy_from_slice(&[192, 168, 1, 1]);
        let entry = entry_for(&mut storage);

        // SAFETY: `storage` outlives the call and holds a well-formed sockaddr_in of
        // the length the entry records.
        assert_eq!(
            unsafe { sockaddr_text(&entry) },
            Some("192.168.1.1".to_string())
        );
    }

    #[test]
    fn an_ipv6_sockaddr_is_read_from_its_own_offset() {
        // sockaddr_in6: family, port, flowinfo, then sin6_addr at offset 8.
        let mut storage = vec![0u8; 28];
        storage[0..2].copy_from_slice(&AF_INET6.0.to_ne_bytes());
        storage[8..24].copy_from_slice(&[
            0x20, 0x01, 0x48, 0x60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x88,
        ]);
        let entry = entry_for(&mut storage);

        // SAFETY: as above, for a well-formed sockaddr_in6.
        assert_eq!(
            unsafe { sockaddr_text(&entry) },
            Some("2001:4860::88".to_string())
        );
    }

    #[test]
    fn the_unspecified_and_short_addresses_are_refused() {
        let mut any = vec![0u8; 16];
        any[0..2].copy_from_slice(&AF_INET.0.to_ne_bytes());
        let entry = entry_for(&mut any);
        // SAFETY: a well-formed sockaddr_in whose address is 0.0.0.0.
        assert_eq!(
            unsafe { sockaddr_text(&entry) },
            None,
            "0.0.0.0 is not a resolver"
        );

        // A length of 6 cannot cover sin_addr at offset 4 plus four bytes.
        let mut short = vec![0u8; 16];
        short[0..2].copy_from_slice(&AF_INET.0.to_ne_bytes());
        let mut entry = entry_for(&mut short);
        entry.Address.iSockaddrLength = 6;
        // SAFETY: the call must refuse on the recorded length before reading.
        assert_eq!(unsafe { sockaddr_text(&entry) }, None);
    }

    #[test]
    fn a_null_sockaddr_and_an_unknown_family_are_refused() {
        let entry = IP_ADAPTER_DNS_SERVER_ADDRESS_XP::default();
        // SAFETY: a null pointer is checked before any read.
        assert_eq!(unsafe { sockaddr_text(&entry) }, None);

        // Family 99 is neither stream this host can query.
        let mut storage = vec![0u8; 16];
        storage[0..2].copy_from_slice(&99u16.to_ne_bytes());
        let entry = entry_for(&mut storage);
        // SAFETY: a readable sockaddr of an unsupported family.
        assert_eq!(unsafe { sockaddr_text(&entry) }, None);
    }

    #[test]
    fn the_live_resolver_list_is_well_formed() {
        // Wine and CI may report no adapters at all; either way the result is a short
        // list of parseable, usable addresses, never a panic.
        let servers = collect_system_dns_servers();
        assert!(
            servers.len() <= MAXIMUM_SRV_SERVERS,
            "capped at MaximumSrvServers"
        );
        for server in &servers {
            let parsed: IpAddr = server.parse().expect("a resolver is an IP");
            assert!(is_usable_resolver(parsed));
        }
    }

    #[test]
    fn the_unspecified_address_is_never_a_resolver() {
        assert!(!is_usable_resolver(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
        assert!(!is_usable_resolver(IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
        assert!(is_usable_resolver(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    }
}
