//! Server address parsing, validation, and the SRV wire codec.
//!
//! Ported from `Aoko/Core/Servers/ServerAddress.cs` (parse/validate half) and
//! `Servers/ServerJoinService.cs`. The UDP lookup and the TTL cache live in
//! `aoko-platform`, which owns the socket; `build_srv_query` and `try_read_srv`
//! are pure byte codecs and stay here with the rest of the wire format.
//!
//! Parsing rules are ported from lunar-cookies `Core/MinecraftServerAddress.cs`
//! so saved text keeps the same meaning.

/// Port a bare host implies.
pub const DEFAULT_PORT: i32 = 25565;
/// Longest accepted address text.
pub const MAXIMUM_TEXT_LENGTH: usize = 256;
/// Longest host the bridge-side Java validation accepts.
pub const MAXIMUM_HOST_LENGTH: usize = 256;
/// Message the servers tab shows for unparsable text.
pub const INVALID_ADDRESS_MESSAGE: &str = "Enter a server as host or host:port.";

/// DNS transaction id every query carries (`SrvTransactionId`).
pub const SRV_TRANSACTION_ID: u16 = 0x1C0D;
/// DNS service type for Minecraft (`SRV`, 33).
const SRV_TYPE: u16 = 33;
/// DNS class `IN`.
const IN_CLASS: u16 = 1;
/// Port DNS listens on.
const DNS_PORT: i32 = 53;
/// Most resolvers a single lookup asks.
const MAXIMUM_SRV_SERVERS: usize = 2;
/// Fallback resolvers, matching the reference implementation.
pub const FALLBACK_DNS_SERVERS: [&str; 2] = ["8.8.8.8", "1.1.1.1"];

/// Resolvers to ask: the system list when it is non-empty, else the fallback.
#[must_use]
pub fn resolve_dns_servers(system: &[String]) -> Vec<String> {
    if system.is_empty() {
        return FALLBACK_DNS_SERVERS
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
    }
    system.iter().take(MAXIMUM_SRV_SERVERS).cloned().collect()
}

/// Query name for a host: `_minecraft._tcp.<host>`.
#[must_use]
pub fn srv_query_name(host: &str) -> String {
    format!("_minecraft._tcp.{}", host.trim_end_matches('.'))
}

/// A parsed target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerAddress {
    pub host: String,
    /// Kept as `i32` because the managed model is an `int`, and the "out of
    /// range" rejection path has to be representable.
    pub port: i32,
    pub port_specified: bool,
}

impl ServerAddress {
    #[must_use]
    pub fn new(host: impl Into<String>, port: i32, port_specified: bool) -> Self {
        Self {
            host: host.into(),
            port,
            port_specified,
        }
    }

    /// Parses user text. `None` means "reject, show
    /// [`INVALID_ADDRESS_MESSAGE`]".
    #[must_use]
    pub fn try_parse(text: Option<&str>) -> Option<Self> {
        let text = text?;
        let value = text.trim();
        if text.trim().is_empty() {
            return None;
        }
        if value.encode_utf16().count() > MAXIMUM_TEXT_LENGTH {
            return None;
        }

        let mut port = DEFAULT_PORT;
        let mut port_specified = false;
        let host;

        if let Some(rest) = value.strip_prefix('[') {
            let close = rest.find(']')?;
            // `close <= 1` in the managed code means an empty or one-character
            // address, i.e. the bracket pair is at the very start.
            if close == 0 {
                return None;
            }
            let inner = &rest[..close];
            let after = &rest[close + 1..];
            if !after.is_empty() {
                let port_text = after.strip_prefix(':')?;
                port = parse_port(port_text)?;
                port_specified = true;
            }
            host = inner;
        } else {
            let last_colon = value.rfind(':');
            let first_colon = value.find(':');
            match last_colon {
                Some(colon)
                    if colon > 0
                        && Some(colon) == first_colon
                        && colon + 1 < value.len()
                        && !value.contains('/') =>
                {
                    port = parse_port(&value[colon + 1..])?;
                    port_specified = true;
                    host = &value[..colon];
                }
                // No bracketed form and more than one colon: the whole value is
                // an IPv6 host.
                _ => host = value,
            }
        }

        let host = host.trim();
        if host.is_empty() || host.contains('/') || host.contains(' ') {
            return None;
        }

        Some(Self {
            host: host.to_owned(),
            port,
            port_specified,
        })
    }

    /// `host:port` when the port is explicit or non-default, else the bare host.
    #[must_use]
    pub fn display(&self) -> String {
        if self.port_specified || self.port != DEFAULT_PORT {
            format!("{}:{}", format_host(&self.host), self.port)
        } else {
            self.host.clone()
        }
    }

    /// Port as a wire-ready `u16`, or `None` when it is out of range.
    #[must_use]
    pub fn port_u16(&self) -> Option<u16> {
        if (1..=65535).contains(&self.port) {
            Some(self.port as u16)
        } else {
            None
        }
    }
}

fn format_host(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

/// `int.TryParse(text, NumberStyles.None, InvariantCulture)`: digits only, no
/// sign, no whitespace.
fn parse_port(text: &str) -> Option<i32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let port: i32 = text.parse().ok()?;
    (1..=65535).contains(&port).then_some(port)
}

/// Outcome of validating user-entered server text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerTarget {
    pub ok: bool,
    pub address: Option<ServerAddress>,
    pub error: &'static str,
}

/// `ServerJoinService.ValidateTarget`.
#[must_use]
pub fn validate_target(text: Option<&str>) -> ServerTarget {
    match ServerAddress::try_parse(text) {
        Some(address) => ServerTarget {
            ok: true,
            address: Some(address),
            error: "",
        },
        None => ServerTarget {
            ok: false,
            address: None,
            error: INVALID_ADDRESS_MESSAGE,
        },
    }
}

/// Builds the host and port the bridge's join action expects.
#[must_use]
pub fn try_build_join_request(address: &ServerAddress) -> Option<(String, u16)> {
    let host = address.host.trim();
    if host.is_empty() || host.encode_utf16().count() > MAXIMUM_HOST_LENGTH {
        return None;
    }

    let port = address.port_u16()?;
    Some((host.to_owned(), port))
}

/// Builds the DNS SRV query for `name`.
///
/// Header `(0x1C0D, 0x0100, qdcount 1, 0, 0, 0)`, the label sequence, then
/// `qtype 33`, `qclass 1`.
///
/// `None` when a label does not fit the wire format: the length prefix is one byte,
/// so a label longer than 255 bytes cannot be encoded and the packet would otherwise
/// go out malformed. Nothing is sent in that case; the caller drops the SRV lookup and
/// falls back to the plain host and port.
#[must_use]
pub fn build_srv_query(name: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(64 + name.len());
    out.extend_from_slice(&SRV_TRANSACTION_ID.to_be_bytes());
    out.extend_from_slice(&0x0100u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());

    for label in name.split('.').filter(|label| !label.is_empty()) {
        out.push(u8::try_from(label.len()).ok()?);
        out.extend_from_slice(label.as_bytes());
    }

    out.push(0);
    out.extend_from_slice(&SRV_TYPE.to_be_bytes());
    out.extend_from_slice(&IN_CLASS.to_be_bytes());
    Some(out)
}

/// Reads a SRV answer.
///
/// When `expected_query_name` is given the datagram must be our own reply —
/// matching transaction id, the response bit, rcode 0, and the question we asked
/// — so a spoofed or stale packet cannot be cached as this server's target.
#[must_use]
pub fn try_read_srv(buffer: &[u8], expected_query_name: Option<&str>) -> Option<(String, i32)> {
    if buffer.len() < 12 {
        return None;
    }

    if u16::from_be_bytes([buffer[0], buffer[1]]) != SRV_TRANSACTION_ID {
        return None;
    }
    if buffer[2] & 0x80 == 0 || buffer[3] & 0x0F != 0 {
        return None;
    }

    let question_count = u16::from_be_bytes([buffer[4], buffer[5]]) as usize;
    let answer_count = u16::from_be_bytes([buffer[6], buffer[7]]) as usize;
    if expected_query_name.is_some() && question_count < 1 {
        return None;
    }

    let mut offset = 12usize;

    for index in 0..question_count {
        let mut name_offset = offset;
        offset = skip_name(buffer, offset)?;
        if offset + 4 > buffer.len() {
            return None;
        }

        if index == 0 {
            if let Some(expected) = expected_query_name {
                let question = read_name(buffer, &mut name_offset)?;
                if !question
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case(expected.trim_end_matches('.'))
                {
                    return None;
                }
                if u16::from_be_bytes([buffer[offset], buffer[offset + 1]]) != SRV_TYPE {
                    return None;
                }
            }
        }

        offset += 4;
    }

    for _ in 0..answer_count {
        offset = skip_name(buffer, offset)?;
        if offset + 10 > buffer.len() {
            return None;
        }

        let record_type = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]);
        let length = u16::from_be_bytes([buffer[offset + 8], buffer[offset + 9]]) as usize;
        offset += 10;
        if offset + length > buffer.len() {
            return None;
        }

        if record_type == SRV_TYPE && length >= 6 {
            // SRV rdata: priority, weight, port, then the target name (which may
            // be a compression pointer).
            let port = i32::from(u16::from_be_bytes([buffer[offset + 4], buffer[offset + 5]]));
            let mut name_offset = offset + 6;
            if let Some(target) = read_name(buffer, &mut name_offset) {
                if (1..=65535).contains(&port) {
                    return Some((target, port));
                }
            }
        }

        offset += length;
    }

    None
}

/// Advances past one (possibly compressed) name; returns the new offset.
fn skip_name(buffer: &[u8], mut offset: usize) -> Option<usize> {
    let mut hops = 0;
    while offset < buffer.len() && hops < 32 {
        hops += 1;
        let length = buffer[offset];
        if length == 0 {
            return Some(offset + 1);
        }
        if length & 0xC0 == 0xC0 {
            return Some(offset + 2);
        }
        offset += 1 + usize::from(length);
    }
    None
}

/// Reads one (possibly compressed) name; leaves `offset` past it.
fn read_name(buffer: &[u8], offset: &mut usize) -> Option<String> {
    let mut labels: Vec<String> = Vec::new();
    let mut hops = 0;
    let mut cursor = *offset;
    let mut end = *offset;
    let mut jumped = false;

    while cursor < buffer.len() && hops < 32 {
        hops += 1;
        let length = buffer[cursor];
        if length == 0 {
            if !jumped {
                end = cursor + 1;
            }
            break;
        }

        if length & 0xC0 == 0xC0 {
            if cursor + 1 >= buffer.len() {
                break;
            }
            if !jumped {
                end = cursor + 2;
            }
            cursor = (usize::from(length & 0x3F) << 8) | usize::from(buffer[cursor + 1]);
            jumped = true;
            continue;
        }

        cursor += 1;
        let label_end = cursor + usize::from(length);
        if label_end > buffer.len() {
            break;
        }
        labels.push(String::from_utf8_lossy(&buffer[cursor..label_end]).into_owned());
        cursor = label_end;
    }

    *offset = end;
    if labels.is_empty() {
        return None;
    }
    Some(labels.join("."))
}

/// DNS port the lookup dials.
#[must_use]
pub const fn dns_port() -> i32 {
    DNS_PORT
}
