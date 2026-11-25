//! SRV resolution: the UDP half of `Aoko/Core/Servers/ServerAddress.cs`.
//!
//! `aoko-protocol` owns the wire format (`srv_query_name`, `build_srv_query`,
//! `try_read_srv`, `resolve_dns_servers`); this module owns the socket, the
//! resolver loop and the TTL cache, which is the split the crate map assigns.
//!
//! Joining a server that publishes an SRV record is a parity-critical path:
//! without it, `mc.example.com` with no explicit port silently connects to
//! `mc.example.com:25565` instead of the host and port the record names.
//!
//! The managed decisions, each deliberate and each preserved here:
//!
//! 1. The query name is `_minecraft._tcp.<host>` with the trailing dot trimmed.
//! 2. Every configured resolver is asked, with a 2 s send and 2 s receive budget.
//!    A per-resolver failure is swallowed: SRV is optional and the explicit host
//!    stays usable.
//! 3. Only the resolver that was asked may answer. A reply from any other source
//!    is discarded, which is what makes the answer worth caching.
//! 4. An answer is accepted only when the protocol parser accepts it and the
//!    target is neither empty nor `"."`. The target stays a host *name*: resolving
//!    it here would put an address in the handshake where the server expects the
//!    name.
//! 5. Misses are cached too, for the shorter 10 minute TTL, so one unresponsive
//!    resolver at startup cannot disable SRV for the session.
//! 6. An expired entry is treated as absent and removed on read.
//! 7. A cancelled lookup is never cached. Caching it would send the next join to
//!    port 25565, which is exactly the failure this module exists to prevent.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use aoko_protocol::servers::{
    build_srv_query, resolve_dns_servers, srv_query_name, try_read_srv, ServerAddress,
};

use crate::error::PlatformError;

/// Port DNS listens on (`DnsPort`).
pub const DNS_PORT: u16 = 53;
/// Send and receive budget per resolver (`SrvTimeout`).
pub const SRV_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a negative answer is trusted (`SrvFailureTtl`).
pub const SRV_FAILURE_TTL: Duration = Duration::from_secs(10 * 60);
/// How long a positive answer is trusted (`SrvSuccessTtl`).
pub const SRV_SUCCESS_TTL: Duration = Duration::from_secs(60 * 60);

/// Largest datagram a UDP reply can carry, which is what the managed
/// `UdpClient.ReceiveAsync` buffer grew to.
const MAX_DATAGRAM: usize = 65_535;

/// The monotonic clock the cache reads.
///
/// The managed cache injected `Func<DateTimeOffset>` for the same reason: tests
/// drive expiry without sleeping. `Instant` is the Rust equivalent that an NTP
/// step cannot move backwards mid-TTL.
pub type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// A resolved target: the host stays a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrvTarget {
    /// Host name to connect to and to put in the handshake.
    pub host: String,
    /// Port the record names.
    pub port: i32,
}

/// The datagram exchange, injected so tests never open a socket.
pub trait SrvExchange: Send + Sync {
    /// Sends `query` to `server`, waits at most `timeout` for a reply, and returns
    /// the source address it arrived from together with its bytes.
    ///
    /// The source is returned rather than filtered because the caller must decide
    /// whether to trust it; a socket that dropped foreign datagrams in the kernel
    /// would hide the anti-spoofing rule instead of exercising it.
    fn exchange(
        &self,
        server: SocketAddr,
        query: &[u8],
        timeout: Duration,
    ) -> Result<(SocketAddr, Vec<u8>), PlatformError>;
}

/// The real exchange: one unconnected UDP socket per resolver.
pub struct UdpExchange;

impl SrvExchange for UdpExchange {
    fn exchange(
        &self,
        server: SocketAddr,
        query: &[u8],
        timeout: Duration,
    ) -> Result<(SocketAddr, Vec<u8>), PlatformError> {
        let bind: SocketAddr = if server.is_ipv4() {
            SocketAddr::from(([0, 0, 0, 0], 0))
        } else {
            SocketAddr::from(([0u16; 8], 0))
        };
        let socket = UdpSocket::bind(bind)
            .map_err(|err| PlatformError::os("bind SRV socket", err.to_string()))?;
        socket
            .set_write_timeout(Some(timeout))
            .map_err(|err| PlatformError::os("set SRV send timeout", err.to_string()))?;
        socket
            .set_read_timeout(Some(timeout))
            .map_err(|err| PlatformError::os("set SRV receive timeout", err.to_string()))?;

        socket
            .send_to(query, server)
            .map_err(|err| PlatformError::os("send SRV query", err.to_string()))?;

        let mut buffer = vec![0u8; MAX_DATAGRAM];
        let (length, source) = socket
            .recv_from(&mut buffer)
            .map_err(|err| PlatformError::os("receive SRV answer", err.to_string()))?;
        buffer.truncate(length);
        Ok((source, buffer))
    }
}

struct Entry {
    target: Option<SrvTarget>,
    expires_at: Instant,
}

/// TTL cache for SRV lookups, mirroring `SrvLookupCache`.
///
/// Keys are case-insensitive, as the managed `StringComparer.OrdinalIgnoreCase`
/// dictionary was, which is done by folding to lower case on the way in.
pub struct SrvLookupCache {
    entries: Mutex<HashMap<String, Entry>>,
    failure_ttl: Duration,
    success_ttl: Duration,
    now: Clock,
}

impl SrvLookupCache {
    /// The managed construction: `new SrvLookupCache(SrvFailureTtl, SrvSuccessTtl)`.
    #[must_use]
    pub fn new(failure_ttl: Duration, success_ttl: Duration) -> Self {
        Self::with_clock(failure_ttl, success_ttl, Arc::new(Instant::now))
    }

    /// The same cache over an injected clock.
    #[must_use]
    pub fn with_clock(failure_ttl: Duration, success_ttl: Duration, now: Clock) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            failure_ttl,
            success_ttl,
            now,
        }
    }

    /// `SrvLookupCache.TryGet`.
    ///
    /// `None` means there is no live entry; `Some(None)` is a cached miss and
    /// `Some(Some(target))` a cached hit. An expired entry is removed rather than
    /// left to be re-read.
    #[must_use]
    pub fn try_get(&self, host: &str) -> Option<Option<SrvTarget>> {
        let key = cache_key(host);
        let mut entries = lock(&self.entries);
        let entry = entries.get(&key)?;
        let live = entry.expires_at > (self.now)();
        let target = entry.target.clone();
        if live {
            return Some(target);
        }
        entries.remove(&key);
        None
    }

    /// `SrvLookupCache.Store`: the shorter TTL applies to a miss.
    pub fn store(&self, host: &str, target: Option<SrvTarget>) {
        let ttl = if target.is_some() {
            self.success_ttl
        } else {
            self.failure_ttl
        };
        let expires_at = (self.now)() + ttl;
        lock(&self.entries).insert(cache_key(host), Entry { target, expires_at });
    }

    /// Entries currently held, expired ones included until a read evicts them.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        lock(&self.entries).len()
    }
}

/// `TrySrvAsync` plus the socket: the cache in front of the resolver loop.
pub struct SrvResolver {
    cache: SrvLookupCache,
    exchange: Arc<dyn SrvExchange>,
    resolvers: Vec<String>,
    timeout: Duration,
}

impl SrvResolver {
    /// Builds a resolver over the system list, falling back to the public
    /// resolvers when it is empty, as `GetDnsServers()` did.
    #[must_use]
    pub fn new(system_resolvers: Vec<String>) -> Self {
        Self {
            cache: SrvLookupCache::new(SRV_FAILURE_TTL, SRV_SUCCESS_TTL),
            exchange: Arc::new(UdpExchange),
            resolvers: resolve_dns_servers(&system_resolvers),
            timeout: SRV_TIMEOUT,
        }
    }

    /// The same resolver over injected parts, for tests and for a host that wants
    /// its own cache.
    #[must_use]
    pub fn with_parts(
        system_resolvers: Vec<String>,
        exchange: Arc<dyn SrvExchange>,
        cache: SrvLookupCache,
    ) -> Self {
        Self {
            cache,
            exchange,
            resolvers: resolve_dns_servers(&system_resolvers),
            timeout: SRV_TIMEOUT,
        }
    }

    /// The resolvers this resolver will ask, in order.
    #[must_use]
    pub fn resolvers(&self) -> &[String] {
        &self.resolvers
    }

    /// The cache, for reporting.
    #[must_use]
    pub fn cache(&self) -> &SrvLookupCache {
        &self.cache
    }

    /// `TrySrvAsync`: cache first, then one lookup, and cache the outcome unless
    /// the caller cancelled.
    ///
    /// `cancelled` is the stand-in for the managed `CancellationToken`. It is
    /// checked before each resolver and once more before storing, so a lookup that
    /// was abandoned mid-flight leaves no entry behind.
    pub fn resolve(&self, host: &str, cancelled: &AtomicBool) -> Option<SrvTarget> {
        let key = host.trim_end_matches('.');
        if key.is_empty() {
            return None;
        }

        if let Some(cached) = self.cache.try_get(key) {
            return cached;
        }

        let result = self.lookup(key, cancelled);
        // A cancelled lookup says nothing about the host: caching it would send the
        // next join to port 25565.
        if !cancelled.load(Ordering::Acquire) {
            self.cache.store(key, result.clone());
        }
        result
    }

    /// [`SrvResolver::resolve`] for a caller that has nothing to cancel.
    #[must_use]
    pub fn resolve_now(&self, host: &str) -> Option<SrvTarget> {
        self.resolve(host, &AtomicBool::new(false))
    }

    /// `ResolveForPingAsync`: an explicit port wins, otherwise the SRV record,
    /// otherwise the parsed address unchanged.
    #[must_use]
    pub fn resolve_for_ping(
        &self,
        parsed: &ServerAddress,
        cancelled: &AtomicBool,
    ) -> ServerAddress {
        if parsed.port_specified {
            return parsed.clone();
        }

        match self.resolve(&parsed.host, cancelled) {
            Some(target) => ServerAddress::new(target.host, target.port, true),
            None => parsed.clone(),
        }
    }

    fn lookup(&self, host: &str, cancelled: &AtomicBool) -> Option<SrvTarget> {
        let query_name = srv_query_name(host);
        // A label too long for the one-byte wire prefix makes `build_srv_query`
        // return `None`; sending it anyway would put a malformed packet on the
        // wire, so the whole lookup is abandoned and the explicit host is used.
        let query = build_srv_query(&query_name)?;

        for resolver in &self.resolvers {
            if cancelled.load(Ordering::Acquire) {
                return None;
            }

            // A resolver string that does not resolve to an address is one
            // resolver's problem, not the host's.
            let Ok(server) = resolver_address(resolver) else {
                continue;
            };

            let Ok((source, datagram)) = self.exchange.exchange(server, &query, self.timeout)
            else {
                continue;
            };

            // Only the resolver we asked may answer.
            if source != server {
                continue;
            }

            let Some(target) = accept_target(&datagram, &query_name) else {
                continue;
            };
            return Some(target);
        }

        None
    }
}

/// The acceptance rule the managed reader applied after parsing:
/// `TryReadSrv(...) && !string.IsNullOrWhiteSpace(target) && target != "."`.
fn accept_target(datagram: &[u8], query_name: &str) -> Option<SrvTarget> {
    let (target, port) = try_read_srv(datagram, Some(query_name))?;
    let host = target.trim_end_matches('.');
    if !is_usable_target(host) {
        return None;
    }
    Some(SrvTarget {
        host: host.to_string(),
        port,
    })
}

/// Whether a parsed target names a server at all. `"."` is the DNS root, which is
/// a valid record value meaning "no service here", and an empty name is the root
/// reached without a label; neither is a host to connect to.
fn is_usable_target(host: &str) -> bool {
    !host.is_empty() && host != "."
}

fn resolver_address(resolver: &str) -> Result<SocketAddr, PlatformError> {
    (resolver, DNS_PORT)
        .to_socket_addrs()
        .map_err(|err| PlatformError::os("resolve DNS server", err.to_string()))?
        .next()
        .ok_or_else(|| {
            PlatformError::os("resolve DNS server", format!("{resolver} has no address"))
        })
}

fn cache_key(host: &str) -> String {
    host.trim_end_matches('.').to_lowercase()
}

/// A poisoned cache lock still holds valid entries, so it is recovered rather than
/// turned into a panic.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A clock the test advances by hand.
    struct TestClock {
        base: Instant,
        offset: Mutex<Duration>,
    }

    impl TestClock {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                base: Instant::now(),
                offset: Mutex::new(Duration::ZERO),
            })
        }

        fn advance(&self, by: Duration) {
            *lock(&self.offset) += by;
        }

        fn clock(self: &Arc<Self>) -> Clock {
            let inner = Arc::clone(self);
            Arc::new(move || inner.base + *lock(&inner.offset))
        }
    }

    /// What the fake exchange does for one call.
    enum Reply {
        /// Arrives from the resolver it was sent to.
        Answer {
            datagram: Vec<u8>,
        },
        /// Arrives from somewhere else, which the driver must refuse.
        RewriteSource {
            source: SocketAddr,
            datagram: Vec<u8>,
        },
        Fail,
        CancelThenAnswer,
    }

    struct FakeExchange {
        replies: Mutex<Vec<Reply>>,
        calls: AtomicUsize,
        last_timeout: Mutex<Option<Duration>>,
        last_query: Mutex<Vec<u8>>,
        cancel: Option<Arc<AtomicBool>>,
    }

    impl FakeExchange {
        fn new(replies: Vec<Reply>) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(replies),
                calls: AtomicUsize::new(0),
                last_timeout: Mutex::new(None),
                last_query: Mutex::new(Vec::new()),
                cancel: None,
            })
        }

        fn with_cancel(replies: Vec<Reply>, cancel: Arc<AtomicBool>) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(replies),
                calls: AtomicUsize::new(0),
                last_timeout: Mutex::new(None),
                last_query: Mutex::new(Vec::new()),
                cancel: Some(cancel),
            })
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Acquire)
        }
    }

    impl SrvExchange for FakeExchange {
        fn exchange(
            &self,
            server: SocketAddr,
            query: &[u8],
            timeout: Duration,
        ) -> Result<(SocketAddr, Vec<u8>), PlatformError> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            *lock(&self.last_timeout) = Some(timeout);
            *lock(&self.last_query) = query.to_vec();

            let reply = {
                let mut replies = lock(&self.replies);
                if replies.is_empty() {
                    None
                } else {
                    Some(replies.remove(0))
                }
            };

            match reply {
                Some(Reply::Answer { datagram }) => Ok((server, datagram)),
                Some(Reply::RewriteSource { source, datagram }) => Ok((source, datagram)),
                Some(Reply::Fail) | None => {
                    Err(PlatformError::os("send SRV query", "fake failure"))
                }
                Some(Reply::CancelThenAnswer) => {
                    if let Some(cancel) = &self.cancel {
                        cancel.store(true, Ordering::Release);
                    }
                    Ok((server, Vec::new()))
                }
            }
        }
    }

    /// Builds a well-formed SRV reply for `query_name`.
    fn srv_response(query_name: &str, target: &str, port: u16) -> Vec<u8> {
        let mut rdata = Vec::new();
        rdata.extend_from_slice(&0u16.to_be_bytes()); // priority
        rdata.extend_from_slice(&0u16.to_be_bytes()); // weight
        rdata.extend_from_slice(&port.to_be_bytes());
        for label in target.split('.').filter(|label| !label.is_empty()) {
            rdata.push(u8::try_from(label.len()).unwrap_or(0));
            rdata.extend_from_slice(label.as_bytes());
        }
        rdata.push(0);

        let mut out = Vec::new();
        out.extend_from_slice(&0x1C0Du16.to_be_bytes());
        out.extend_from_slice(&0x8180u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());

        for label in query_name.split('.').filter(|label| !label.is_empty()) {
            out.push(u8::try_from(label.len()).unwrap_or(0));
            out.extend_from_slice(label.as_bytes());
        }
        out.push(0);
        out.extend_from_slice(&33u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());

        // The answer name is a compression pointer back to the question at offset 12.
        out.extend_from_slice(&0xC00Cu16.to_be_bytes());
        out.extend_from_slice(&33u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes()); // ttl
        out.extend_from_slice(&u16::try_from(rdata.len()).unwrap_or(0).to_be_bytes());
        out.extend_from_slice(&rdata);
        out
    }

    fn resolver(host: &str, exchange: Arc<FakeExchange>) -> SrvResolver {
        SrvResolver::with_parts(
            vec![host.to_string()],
            exchange,
            SrvLookupCache::new(SRV_FAILURE_TTL, SRV_SUCCESS_TTL),
        )
    }

    fn uncancelled() -> AtomicBool {
        AtomicBool::new(false)
    }

    const RESOLVER: &str = "127.0.0.1";

    #[test]
    fn a_positive_answer_is_cached_and_not_asked_again() {
        let exchange = FakeExchange::new(vec![Reply::Answer {
            datagram: srv_response("_minecraft._tcp.mc.example.com", "mc.example.com", 25566),
        }]);
        let clock = TestClock::new();
        let resolver = SrvResolver::with_parts(
            vec![RESOLVER.to_string()],
            Arc::clone(&exchange) as Arc<dyn SrvExchange>,
            SrvLookupCache::with_clock(SRV_FAILURE_TTL, SRV_SUCCESS_TTL, clock.clock()),
        );

        let first = resolver.resolve("mc.example.com", &uncancelled());
        assert_eq!(
            first,
            Some(SrvTarget {
                host: "mc.example.com".to_string(),
                port: 25566
            })
        );

        let second = resolver.resolve("mc.example.com", &uncancelled());
        assert_eq!(second, first);
        assert_eq!(exchange.calls(), 1, "the cache answered the second ask");
    }

    #[test]
    fn the_query_and_the_budget_are_the_managed_ones() {
        let exchange = FakeExchange::new(vec![Reply::Fail]);
        let resolver = resolver(RESOLVER, Arc::clone(&exchange));
        resolver.resolve("mc.example.com.", &uncancelled());

        assert_eq!(
            *lock(&exchange.last_timeout),
            Some(Duration::from_secs(2)),
            "both send and receive are bounded by SrvTimeout"
        );
        assert_eq!(
            *lock(&exchange.last_query),
            build_srv_query("_minecraft._tcp.mc.example.com").unwrap_or_default(),
            "the trailing dot is trimmed and the protocol encoder is used"
        );
        // C# interpolated the host as typed, so the case is preserved here too; DNS
        // comparison is case-insensitive at the server.
        assert_eq!(
            *lock(&exchange.last_query),
            build_srv_query(&srv_query_name("mc.example.com")).unwrap_or_default()
        );
    }

    #[test]
    fn a_positive_entry_expires_after_one_hour() {
        let exchange = FakeExchange::new(vec![
            Reply::Answer {
                datagram: srv_response("_minecraft._tcp.h", "h", 25566),
            },
            Reply::Answer {
                datagram: srv_response("_minecraft._tcp.h", "h", 25566),
            },
        ]);
        let clock = TestClock::new();
        let resolver = SrvResolver::with_parts(
            vec![RESOLVER.to_string()],
            Arc::clone(&exchange) as Arc<dyn SrvExchange>,
            SrvLookupCache::with_clock(SRV_FAILURE_TTL, SRV_SUCCESS_TTL, clock.clock()),
        );

        resolver.resolve("h", &uncancelled());
        clock.advance(SRV_SUCCESS_TTL - Duration::from_secs(1));
        resolver.resolve("h", &uncancelled());
        assert_eq!(
            exchange.calls(),
            1,
            "still live one second short of the TTL"
        );

        clock.advance(Duration::from_secs(2));
        resolver.resolve("h", &uncancelled());
        assert_eq!(
            exchange.calls(),
            2,
            "expired, so the resolver was asked again"
        );
    }

    #[test]
    fn a_miss_is_cached_for_ten_minutes_then_retried() {
        let exchange = FakeExchange::new(vec![Reply::Fail, Reply::Fail]);
        let clock = TestClock::new();
        let resolver = SrvResolver::with_parts(
            vec![RESOLVER.to_string()],
            Arc::clone(&exchange) as Arc<dyn SrvExchange>,
            SrvLookupCache::with_clock(SRV_FAILURE_TTL, SRV_SUCCESS_TTL, clock.clock()),
        );

        assert_eq!(resolver.resolve("h", &uncancelled()), None);
        assert_eq!(resolver.resolve("h", &uncancelled()), None);
        assert_eq!(exchange.calls(), 1, "the miss is cached for the short TTL");

        clock.advance(SRV_FAILURE_TTL + Duration::from_secs(1));
        assert_eq!(resolver.resolve("h", &uncancelled()), None);
        assert_eq!(exchange.calls(), 2, "a miss must not outlive its TTL");
    }

    #[test]
    fn a_later_success_replaces_a_cached_miss() {
        let exchange = FakeExchange::new(vec![
            Reply::Fail,
            Reply::Answer {
                datagram: srv_response("_minecraft._tcp.mc.example.com", "mc.example.com", 25566),
            },
        ]);
        let clock = TestClock::new();
        let resolver = SrvResolver::with_parts(
            vec![RESOLVER.to_string()],
            Arc::clone(&exchange) as Arc<dyn SrvExchange>,
            SrvLookupCache::with_clock(SRV_FAILURE_TTL, SRV_SUCCESS_TTL, clock.clock()),
        );
        let expected = SrvTarget {
            host: "mc.example.com".to_string(),
            port: 25566,
        };

        // 1. A miss, cached under the short TTL.
        assert_eq!(resolver.resolve("mc.example.com", &uncancelled()), None);
        assert_eq!(exchange.calls(), 1);
        assert_eq!(resolver.cache().try_get("mc.example.com"), Some(None));

        // 2. Once the miss lapses, the same host resolves: the store overwrites the
        //    negative entry instead of keeping it.
        clock.advance(SRV_FAILURE_TTL + Duration::from_secs(1));
        assert_eq!(
            resolver.resolve("mc.example.com", &uncancelled()),
            Some(expected.clone())
        );
        assert_eq!(exchange.calls(), 2);
        assert_eq!(
            resolver.cache().try_get("mc.example.com"),
            Some(Some(expected.clone())),
            "the miss is gone, the record replaced it"
        );

        // 3. The replacement carries the success TTL, not the remainder of the short
        //    one: nearly an hour later it is still served without a new query.
        clock.advance(SRV_SUCCESS_TTL - Duration::from_secs(60));
        assert_eq!(
            resolver.resolve("mc.example.com", &uncancelled()),
            Some(expected)
        );
        assert_eq!(exchange.calls(), 2, "the success is cached, not re-queried");
    }

    #[test]
    fn an_expired_entry_is_evicted_on_read() {
        let clock = TestClock::new();
        let cache = SrvLookupCache::with_clock(SRV_FAILURE_TTL, SRV_SUCCESS_TTL, clock.clock());
        cache.store("h", None);
        assert_eq!(cache.entry_count(), 1);

        clock.advance(SRV_FAILURE_TTL + Duration::from_secs(1));
        assert_eq!(cache.try_get("h"), None, "expired means absent");
        assert_eq!(cache.entry_count(), 0, "and it is removed, not left behind");
    }

    #[test]
    fn cache_keys_are_case_insensitive() {
        let cache = SrvLookupCache::new(SRV_FAILURE_TTL, SRV_SUCCESS_TTL);
        cache.store(
            "MC.Example.com",
            Some(SrvTarget {
                host: "mc.example.com".to_string(),
                port: 25566,
            }),
        );
        assert!(cache.try_get("mc.example.com.").is_some());
        assert!(cache.try_get("Mc.Example.COM").is_some());
    }

    #[test]
    fn a_reply_from_another_source_is_refused() {
        let exchange = FakeExchange::new(vec![Reply::RewriteSource {
            source: SocketAddr::from(([10, 0, 0, 9], DNS_PORT)),
            datagram: srv_response("_minecraft._tcp.mc.example.com", "evil.example.com", 1234),
        }]);
        let resolver = resolver(RESOLVER, Arc::clone(&exchange));
        let cancelled = uncancelled();

        assert_eq!(resolver.resolve("mc.example.com", &cancelled), None);
        // A refused answer is still a miss, and a miss is cached, so the count stays
        // at one round while the entry lives.
        assert_eq!(exchange.calls(), 1);
        assert_eq!(resolver.cache().try_get("mc.example.com"), Some(None));
    }

    #[test]
    fn a_spoofed_answer_does_not_stop_a_later_resolver_answering() {
        let exchange = FakeExchange::new(vec![
            Reply::RewriteSource {
                source: SocketAddr::from(([10, 0, 0, 9], DNS_PORT)),
                datagram: srv_response("_minecraft._tcp.h", "evil", 1234),
            },
            Reply::Answer {
                datagram: srv_response("_minecraft._tcp.h", "real.example.com", 25566),
            },
        ]);
        // Two resolvers: the loop must continue, not return.
        let resolver = SrvResolver::with_parts(
            vec!["127.0.0.1".to_string(), "127.0.0.2".to_string()],
            Arc::clone(&exchange) as Arc<dyn SrvExchange>,
            SrvLookupCache::new(SRV_FAILURE_TTL, SRV_SUCCESS_TTL),
        );

        let resolved = resolver.resolve("h", &uncancelled());
        assert_eq!(
            resolved,
            Some(SrvTarget {
                host: "real.example.com".to_string(),
                port: 25566
            })
        );
        assert_eq!(exchange.calls(), 2);
    }

    #[test]
    fn a_garbage_datagram_is_refused_without_caching_an_answer() {
        let exchange = FakeExchange::new(vec![Reply::Answer {
            datagram: vec![0xde, 0xad, 0xbe, 0xef],
        }]);
        let resolver = resolver(RESOLVER, Arc::clone(&exchange));
        assert_eq!(resolver.resolve("h", &uncancelled()), None);
        assert_eq!(
            resolver.cache().try_get("h"),
            Some(None),
            "a miss, not a hit"
        );
    }

    #[test]
    fn a_parsed_target_keeps_its_name_and_loses_only_the_root_dot() {
        let datagram = srv_response("_minecraft._tcp.mc.example.com", "mc.example.com", 25566);
        assert_eq!(
            accept_target(&datagram, "_minecraft._tcp.mc.example.com"),
            Some(SrvTarget {
                host: "mc.example.com".to_string(),
                port: 25566
            })
        );
        assert_eq!(
            accept_target(&[], "_minecraft._tcp.mc.example.com"),
            None,
            "a datagram the parser refuses is never an answer"
        );
    }

    #[test]
    fn the_root_and_empty_targets_are_never_accepted() {
        // `try_read_srv` already refuses a root name, so the managed
        // `target != "."` guard is asserted against the rule itself: no packet can
        // reach it, and it must still say no if one ever did.
        assert!(is_usable_target("mc.example.com"));
        assert!(!is_usable_target(""));
        assert!(!is_usable_target("."));
        assert_eq!(
            accept_target(
                &srv_response("_minecraft._tcp.h", ".", 25566),
                "_minecraft._tcp.h"
            ),
            None,
            "a record naming the root is not a server"
        );
    }

    #[test]
    fn a_cancelled_lookup_is_not_cached() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let exchange = FakeExchange::with_cancel(
            vec![Reply::CancelThenAnswer, Reply::Fail],
            Arc::clone(&cancelled),
        );
        let resolver = resolver(RESOLVER, Arc::clone(&exchange));

        // The exchange flips the flag mid-flight, so the empty reply is not cached.
        assert_eq!(resolver.resolve("h", &cancelled), None);
        assert_eq!(resolver.cache().entry_count(), 0, "nothing stored");

        cancelled.store(false, Ordering::Release);
        resolver.resolve("h", &cancelled);
        assert_eq!(exchange.calls(), 2, "the next attempt really re-queried");
    }

    #[test]
    fn a_cancel_before_the_loop_sends_nothing() {
        let cancelled = AtomicBool::new(true);
        let exchange = FakeExchange::new(vec![]);
        let resolver = resolver(RESOLVER, Arc::clone(&exchange));

        assert_eq!(resolver.resolve("h", &cancelled), None);
        assert_eq!(exchange.calls(), 0);
        assert_eq!(resolver.cache().entry_count(), 0);
    }

    #[test]
    fn an_empty_host_is_never_queried_or_cached() {
        let exchange = FakeExchange::new(vec![]);
        let resolver = resolver(RESOLVER, Arc::clone(&exchange));

        assert_eq!(resolver.resolve("", &uncancelled()), None);
        assert_eq!(resolver.resolve(".", &uncancelled()), None);
        assert_eq!(exchange.calls(), 0);
        assert_eq!(resolver.cache().entry_count(), 0);
    }

    #[test]
    fn an_unencodable_query_name_abandons_the_lookup() {
        let exchange = FakeExchange::new(vec![]);
        let resolver = resolver(RESOLVER, Arc::clone(&exchange));
        // A label longer than the one-byte length prefix: `build_srv_query` returns
        // `None` and nothing may go on the wire.
        let host = "a".repeat(300);

        assert_eq!(resolver.resolve(&host, &uncancelled()), None);
        assert_eq!(exchange.calls(), 0, "a malformed packet is never sent");
        // The managed reader treated a thrown encode as an ordinary miss, and a miss
        // is cached; the host cannot become encodable later, so that is harmless.
        assert_eq!(resolver.cache().entry_count(), 1);
    }

    #[test]
    fn resolve_now_is_the_uncancellable_entry_point() {
        let exchange = FakeExchange::new(vec![Reply::Answer {
            datagram: srv_response("_minecraft._tcp.h", "h", 25566),
        }]);
        let resolver = resolver(RESOLVER, Arc::clone(&exchange));
        assert_eq!(
            resolver.resolve_now("h"),
            Some(SrvTarget {
                host: "h".to_string(),
                port: 25566
            })
        );
    }

    #[test]
    fn no_system_resolvers_falls_back_to_the_public_pair() {
        let resolver = SrvResolver::new(Vec::new());
        assert_eq!(resolver.resolvers(), ["8.8.8.8", "1.1.1.1"]);
    }

    #[test]
    fn the_system_resolvers_are_asked_in_order_and_capped() {
        let resolver = SrvResolver::new(vec![
            "192.168.1.1".to_string(),
            "192.168.1.2".to_string(),
            "192.168.1.3".to_string(),
        ]);
        assert_eq!(resolver.resolvers(), ["192.168.1.1", "192.168.1.2"]);
    }

    #[test]
    fn an_explicit_port_is_never_overridden_by_srv() {
        let exchange = FakeExchange::new(vec![]);
        let resolver = resolver(RESOLVER, Arc::clone(&exchange));
        let parsed = ServerAddress::new("mc.example.com", 25566, true);

        let resolved = resolver.resolve_for_ping(&parsed, &uncancelled());
        assert_eq!(resolved, parsed);
        assert_eq!(exchange.calls(), 0);
    }

    #[test]
    fn a_host_without_a_record_keeps_its_parsed_address() {
        let exchange = FakeExchange::new(vec![Reply::Fail]);
        let resolver = resolver(RESOLVER, Arc::clone(&exchange));
        let parsed = ServerAddress::new("mc.example.com", 25565, false);

        let resolved = resolver.resolve_for_ping(&parsed, &uncancelled());
        assert_eq!(resolved, parsed);
        assert!(!resolved.port_specified);
    }
}
