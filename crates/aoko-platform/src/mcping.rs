//! Minecraft Server List Ping: the modern 1.7+ status exchange and the legacy
//! `0xFE 0x01` reply, ported from `Aoko/Core/Servers/ServerPing.cs`.
//!
//! One modern exchange is three packets:
//!
//! 1. a handshake (`0x00`) carrying the protocol number, the host, the big-endian
//!    port and next-state `1` (status),
//! 2. a status request, the empty `0x00` packet,
//! 3. one status response whose payload is `0x00` followed by a length-prefixed
//!    JSON document.
//!
//! Every packet is framed by a varint length prefix. That prefix is
//! attacker-controlled, so [`MAXIMUM_PACKET_LENGTH`] bounds it *before* anything is
//! allocated, and each read is charged against a deadline rather than against a
//! per-read socket timeout alone: a peer that dribbles one byte at a time cannot
//! hold a row on "Pinging" past [`PING_BUDGET`].
//!
//! A pre-1.7 server answers nothing to the modern exchange and understands only
//! `0xFE 0x01`, so a failed modern attempt is retried once in the legacy format
//! ([`LEGACY_REQUEST`] within [`LEGACY_BUDGET`]).
//!
//! SRV resolution is not duplicated: [`crate::srv::SrvResolver`] answers the
//! `_minecraft._tcp` record with its own TTL cache, and this module only asks it
//! which host and port to put in the handshake and dial.
//!
//! `players.sample` and `favicon` are in the status document but nothing in the
//! managed result carries them, so they are not read here either. The version name
//! and protocol are, because the managed result has a field for them.
//!
//! Nothing here runs on the caller's thread: [`PingService`] keeps a fixed pool of
//! workers ([`PING_PARALLELISM`], the managed `PingParallelism`) and reports
//! finished exchanges over a channel the tab drains once a frame.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aoko_protocol::servers::{ServerAddress, INVALID_ADDRESS_MESSAGE};
use serde_json::Value;

use crate::srv::SrvResolver;

/// Protocol number the handshake announces (`ProtocolVersion`).
pub const PROTOCOL_VERSION: i32 = 767;

/// Largest status packet accepted (`MaximumPacketLength`).
pub const MAXIMUM_PACKET_LENGTH: usize = 32_767;

/// Budget for one socket step (`OperationTimeout`).
pub const OPERATION_TIMEOUT: Duration = Duration::from_secs(4);

/// Budget for one whole modern exchange (`PingBudget`).
pub const PING_BUDGET: Duration = Duration::from_secs(10);

/// Budget for the legacy fallback, which is one request and one small reply.
pub const LEGACY_BUDGET: Duration = Duration::from_secs(4);

/// Largest legacy reply accepted: one kick byte, a 16-bit character count, then
/// that many UTF-16 big-endian units. This is the format's own ceiling, so a legal
/// reply is never truncated.
pub const MAXIMUM_LEGACY_RESPONSE: usize = 2 * 65_535 + 3;

/// Sockets one pass may keep open (`PingParallelism`).
pub const PING_PARALLELISM: usize = 4;

/// The pre-1.7 ping request.
pub const LEGACY_REQUEST: [u8; 2] = [0xFE, 0x01];

/// Chip text for a host that refused or dropped the connection.
pub const OFFLINE_LABEL: &str = "Offline";

/// Chip text for a reply that is not a status packet.
pub const UNEXPECTED_REPLY_LABEL: &str = "Unexpected ping response.";

/// Chip text for a pass that was abandoned.
pub const CANCELLED_LABEL: &str = "Cancelled";

/// Chip text for an exchange that ran out of budget.
pub const TIMED_OUT_LABEL: &str = "Timed out";

/// One server list ping outcome, already shaped for the Servers tab labels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerPingResult {
    pub online: bool,
    pub latency_ms: i32,
    /// MOTD with the legacy colour codes stripped, trimmed.
    pub description: String,
    pub online_players: i32,
    pub max_players: i32,
    /// `version.name`, empty when the server sent none.
    pub version: String,
    /// `version.protocol`, zero when the server sent none.
    pub protocol: i32,
    /// Failure text, empty when [`ServerPingResult::online`] holds.
    pub error: String,
}

impl ServerPingResult {
    /// A failed ping carrying `error`.
    #[must_use]
    pub fn offline(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            ..Self::default()
        }
    }

    /// `PlayersLabel`: `12/100` online, an em dash when the row is offline.
    #[must_use]
    pub fn players_label(&self) -> String {
        if self.online {
            format!("{}/{}", self.online_players, self.max_players)
        } else {
            "\u{2014}".to_owned()
        }
    }
}

/// Why a hand-framed read failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// The peer closed the connection, or the read failed, before the packet ended.
    Truncated,
    /// The budget ran out with the packet still incomplete.
    TimedOut,
    /// The varint was longer than five bytes, so it does not encode a 32-bit value.
    VarIntTooLong,
    /// The announced length is zero or beyond [`MAXIMUM_PACKET_LENGTH`].
    InvalidLength(i32),
}

/// How a modern attempt failed, before it is turned into chip text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    Offline,
    TimedOut,
    Unexpected,
}

impl Failure {
    const fn label(self) -> &'static str {
        match self {
            Self::Offline => OFFLINE_LABEL,
            Self::TimedOut => TIMED_OUT_LABEL,
            Self::Unexpected => UNEXPECTED_REPLY_LABEL,
        }
    }
}

/// One wall-clock budget, read before every read so a slow peer cannot outlast it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Deadline {
    expires: Instant,
}

impl Deadline {
    fn new(budget: Duration) -> Self {
        Self {
            expires: Instant::now() + budget,
        }
    }

    /// Time left, `None` once the budget is spent.
    fn remaining(self) -> Option<Duration> {
        let now = Instant::now();
        if now >= self.expires {
            return None;
        }
        self.expires.checked_duration_since(now)
    }

    fn spent(self) -> bool {
        self.remaining().is_none()
    }
}

/// Appends the varint encoding of `value`.
pub fn write_var_int(out: &mut Vec<u8>, value: i32) {
    let mut remaining = value as u32;
    loop {
        if remaining & !0x7F == 0 {
            out.push(remaining as u8);
            return;
        }
        out.push(((remaining & 0x7F) | 0x80) as u8);
        remaining >>= 7;
    }
}

/// Reads a varint, advancing `offset`.
///
/// `None` when the buffer ends first or the varint carries a sixth continuation
/// byte; neither can encode a 32-bit value.
#[must_use]
pub fn read_var_int(buffer: &[u8], offset: &mut usize) -> Option<i32> {
    let mut value: u32 = 0;
    let mut position: u32 = 0;
    let mut cursor = *offset;
    while cursor < buffer.len() {
        let current = buffer[cursor];
        cursor += 1;
        value |= u32::from(current & 0x7F) << position;
        if current & 0x80 == 0 {
            *offset = cursor;
            return Some(value as i32);
        }
        position += 7;
        if position >= 32 {
            return None;
        }
    }
    None
}

/// Appends a length-prefixed UTF-8 string.
pub fn write_string(out: &mut Vec<u8>, value: &str) {
    write_var_int(out, value.len() as i32);
    out.extend_from_slice(value.as_bytes());
}

/// Reads a length-prefixed UTF-8 string, advancing `offset`.
///
/// `None` when the length prefix or the body runs past the buffer, so a hostile
/// length cannot read outside the packet.
#[must_use]
pub fn read_string(buffer: &[u8], offset: &mut usize) -> Option<String> {
    let length = usize::try_from(read_var_int(buffer, offset)?).ok()?;
    let start = *offset;
    let end = start.checked_add(length)?;
    if end > buffer.len() {
        return None;
    }
    let text = String::from_utf8_lossy(&buffer[start..end]).into_owned();
    *offset = end;
    Some(text)
}

/// The handshake payload: protocol, host, big-endian port, next-state 1.
#[must_use]
pub fn build_handshake(host: &str, port: u16) -> Vec<u8> {
    let mut payload = Vec::with_capacity(host.len() + 8);
    write_var_int(&mut payload, 0x00);
    write_var_int(&mut payload, PROTOCOL_VERSION);
    write_string(&mut payload, host);
    payload.extend_from_slice(&port.to_be_bytes());
    write_var_int(&mut payload, 1);
    payload
}

/// Frames `payload` with its varint length prefix.
#[must_use]
pub fn frame_packet(payload: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(payload.len() + 5);
    write_var_int(&mut framed, payload.len() as i32);
    framed.extend_from_slice(payload);
    framed
}

/// Reads one framed packet, refusing a length the peer cannot justify.
pub fn read_packet<R: Read>(reader: &mut R, deadline: Deadline) -> Result<Vec<u8>, FrameError> {
    let length = read_var_int_from(reader, deadline)?;
    if length < 1 || length as usize > MAXIMUM_PACKET_LENGTH {
        return Err(FrameError::InvalidLength(length));
    }

    let mut payload = vec![0u8; length as usize];
    read_exactly(reader, &mut payload, deadline)?;
    Ok(payload)
}

fn read_var_int_from<R: Read>(reader: &mut R, deadline: Deadline) -> Result<i32, FrameError> {
    let mut value: u32 = 0;
    let mut position: u32 = 0;
    let mut one = [0u8; 1];
    loop {
        read_exactly(reader, &mut one, deadline)?;
        value |= u32::from(one[0] & 0x7F) << position;
        if one[0] & 0x80 == 0 {
            return Ok(value as i32);
        }
        position += 7;
        if position >= 32 {
            return Err(FrameError::VarIntTooLong);
        }
    }
}

/// Fills `buffer` from `reader`, giving up when the budget is spent.
fn read_exactly<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
    deadline: Deadline,
) -> Result<(), FrameError> {
    let mut filled = 0;
    while filled < buffer.len() {
        if deadline.spent() {
            return Err(FrameError::TimedOut);
        }

        match reader.read(&mut buffer[filled..]) {
            Ok(0) => return Err(FrameError::Truncated),
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut => {}
            Err(_) => return Err(FrameError::Truncated),
        }
    }
    Ok(())
}

/// Reads the whole legacy reply, stopping at end of stream, at the cap, or at the
/// budget. A legacy server closes the connection after its kick packet.
fn read_legacy<R: Read>(reader: &mut R, deadline: Deadline) -> Vec<u8> {
    let mut reply = Vec::new();
    let mut chunk = [0u8; 512];
    while reply.len() < MAXIMUM_LEGACY_RESPONSE {
        if deadline.spent() {
            break;
        }

        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                let room = MAXIMUM_LEGACY_RESPONSE - reply.len();
                reply.extend_from_slice(&chunk[..count.min(room)]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    reply
}

/// Parses a pre-1.7 reply into an online result, less its latency.
///
/// Two shapes exist: the `§1` reply (`§1\0protocol\0version\0motd\0online\0max`,
/// the string as UTF-16 big-endian behind a two-byte character count) and the
/// beta-era `motd§online§max`. Anything else is not a reply this module can use.
#[must_use]
pub fn parse_legacy_response(payload: &[u8]) -> Option<ServerPingResult> {
    let (kick, body) = payload.split_first()?;
    if *kick != 0xFF || body.len() < 2 {
        return None;
    }

    let characters = usize::from(u16::from_be_bytes([body[0], body[1]]));
    let text = &body[2..];
    if text.len() < characters.checked_mul(2)? {
        return None;
    }

    let mut units = Vec::with_capacity(characters);
    for index in 0..characters {
        let high = text[index * 2];
        let low = text[index * 2 + 1];
        units.push(u16::from_be_bytes([high, low]));
    }
    let reply = String::from_utf16(&units).ok()?;

    if let Some(fields) = reply.strip_prefix("\u{a7}1\0") {
        let fields: Vec<&str> = fields.split('\u{0}').collect();
        if fields.len() < 5 {
            return None;
        }

        return Some(ServerPingResult {
            online: true,
            description: strip_colour_codes(fields[2]).trim().to_owned(),
            online_players: fields[3].trim().parse().unwrap_or(0),
            max_players: fields[4].trim().parse().unwrap_or(0),
            version: fields[1].to_owned(),
            protocol: fields[0].trim().parse().unwrap_or(0),
            ..ServerPingResult::default()
        });
    }

    let fields: Vec<&str> = reply.split('\u{a7}').collect();
    if fields.len() != 3 {
        return None;
    }

    Some(ServerPingResult {
        online: true,
        description: strip_colour_codes(fields[0]).trim().to_owned(),
        online_players: fields[1].trim().parse().unwrap_or(0),
        max_players: fields[2].trim().parse().unwrap_or(0),
        ..ServerPingResult::default()
    })
}

/// Joins the MOTD components of `description` into one line and strips its colour
/// codes.
#[must_use]
pub fn flatten_motd(value: &Value) -> String {
    let mut text = String::new();
    append_motd(value, &mut text);
    strip_colour_codes(&text).trim().to_owned()
}

fn append_motd(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => out.push_str(text),
        Value::Object(fields) => {
            if let Some(Value::String(text)) = fields.get("text") {
                out.push_str(text);
            }
            if let Some(Value::Array(extra)) = fields.get("extra") {
                for child in extra {
                    append_motd(child, out);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                append_motd(child, out);
            }
        }
        _ => {}
    }
}

/// Drops every `§` colour and format code, as `ColorCodes.Replace` did. A `§`
/// followed by anything else is text and stays.
#[must_use]
pub fn strip_colour_codes(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(current) = characters.next() {
        if current == '\u{a7}' {
            match characters.peek() {
                Some(&next) if is_colour_code(next) => {
                    characters.next();
                }
                _ => stripped.push(current),
            }
            continue;
        }
        stripped.push(current);
    }
    stripped
}

/// `[0-9A-FK-ORa-fk-or]`, the set in `ColorCodes`.
const fn is_colour_code(character: char) -> bool {
    matches!(
        character,
        '0'..='9' | 'A'..='F' | 'K'..='O' | 'R' | 'a'..='f' | 'k'..='o' | 'r'
    )
}

/// Reads a status document into the fields the tab paints.
///
/// `None` when `json` is not a JSON object: the managed reader's `TryGetProperty`
/// threw on anything else, so such a reply is a failed ping rather than a server
/// with no players.
#[must_use]
pub fn parse_status(json: &str, latency_ms: i32) -> Option<ServerPingResult> {
    let root: Value = serde_json::from_str(json).ok()?;
    if !root.is_object() {
        return None;
    }

    let players = root.get("players");
    let version = root.get("version");

    Some(ServerPingResult {
        online: true,
        latency_ms,
        description: root
            .get("description")
            .map(flatten_motd)
            .unwrap_or_default(),
        online_players: players
            .and_then(|value| value.get("online"))
            .and_then(int32_value)
            .unwrap_or(0),
        max_players: players
            .and_then(|value| value.get("max"))
            .and_then(int32_value)
            .unwrap_or(0),
        version: version
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        protocol: version
            .and_then(|value| value.get("protocol"))
            .and_then(int32_value)
            .unwrap_or(0),
        error: String::new(),
    })
}

/// A JSON number that fits the managed `int` field, `None` otherwise.
fn int32_value(value: &Value) -> Option<i32> {
    i32::try_from(value.as_i64()?).ok()
}

/// Pings `address` with the default budget. Never panics; a failure comes back in
/// [`ServerPingResult::error`].
#[must_use]
pub fn ping_address(address: &ServerAddress, cancelled: &AtomicBool) -> ServerPingResult {
    ping_address_with_budget(address, cancelled, PING_BUDGET)
}

/// [`ping_address`] over a caller-chosen budget.
///
/// The budget covers the whole modern exchange, not one step: the per-step socket
/// timeouts bound each read, and this bounds their sum.
#[must_use]
pub fn ping_address_with_budget(
    address: &ServerAddress,
    cancelled: &AtomicBool,
    budget: Duration,
) -> ServerPingResult {
    if cancelled.load(Ordering::Acquire) {
        return ServerPingResult::offline(CANCELLED_LABEL);
    }

    let failure = match query_modern(address, Deadline::new(budget)) {
        Ok(result) => return result,
        Err(failure) => failure,
    };

    if cancelled.load(Ordering::Acquire) {
        return ServerPingResult::offline(CANCELLED_LABEL);
    }

    if let Some(result) = query_legacy(address, Deadline::new(LEGACY_BUDGET)) {
        return result;
    }

    let label = failure.label();
    tracing::debug!("server list ping of {} failed: {label}", address.host);
    ServerPingResult::offline(label)
}

/// Resolves the text (SRV when no port was given), then pings. Never panics.
#[must_use]
pub fn ping_text(text: &str, srv: &SrvResolver, cancelled: &AtomicBool) -> ServerPingResult {
    let Some(parsed) = ServerAddress::try_parse(Some(text)) else {
        return ServerPingResult::offline(INVALID_ADDRESS_MESSAGE);
    };

    if cancelled.load(Ordering::Acquire) {
        return ServerPingResult::offline(CANCELLED_LABEL);
    }

    let resolved = srv.resolve_for_ping(&parsed, cancelled);
    ping_address(&resolved, cancelled)
}

/// The 1.7+ exchange: handshake, status request, one status response.
fn query_modern(address: &ServerAddress, deadline: Deadline) -> Result<ServerPingResult, Failure> {
    let port = address.port_u16().ok_or(Failure::Unexpected)?;
    let stream = connect(&address.host, port, deadline)?;
    stream.set_nodelay(true).map_err(|error| classify(&error))?;
    stream
        .set_write_timeout(Some(OPERATION_TIMEOUT))
        .map_err(|error| classify(&error))?;

    let started = Instant::now();
    write_packet(&stream, &build_handshake(&address.host, port))?;
    write_packet(&stream, &[0x00])?;

    let mut reader = BudgetReader {
        stream: &stream,
        deadline,
    };
    let payload = read_packet(&mut reader, deadline).map_err(failure_from)?;
    let latency_ms = i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX);

    if payload.first() != Some(&0x00) {
        return Err(Failure::Unexpected);
    }

    let mut offset = 1;
    let json = read_string(&payload, &mut offset).ok_or(Failure::Unexpected)?;
    parse_status(&json, latency_ms).ok_or(Failure::Unexpected)
}

/// The pre-1.7 exchange, `None` when the server does not answer in that format.
fn query_legacy(address: &ServerAddress, deadline: Deadline) -> Option<ServerPingResult> {
    let port = address.port_u16()?;
    let stream = connect(&address.host, port, deadline).ok()?;
    stream.set_nodelay(true).ok()?;
    stream.set_write_timeout(Some(OPERATION_TIMEOUT)).ok()?;

    let started = Instant::now();
    let mut handle = &stream;
    handle.write_all(&LEGACY_REQUEST).ok()?;
    handle.flush().ok()?;

    let mut reader = BudgetReader {
        stream: &stream,
        deadline,
    };
    let payload = read_legacy(&mut reader, deadline);
    let latency_ms = i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX);
    let mut result = parse_legacy_response(&payload)?;
    result.latency_ms = latency_ms;
    Some(result)
}

fn connect(host: &str, port: u16, deadline: Deadline) -> Result<TcpStream, Failure> {
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return Err(Failure::Offline);
    };

    let mut last: Option<io::Error> = None;
    for address in addresses {
        let Some(remaining) = deadline.remaining() else {
            return Err(Failure::TimedOut);
        };
        let attempt = remaining.min(OPERATION_TIMEOUT);
        if attempt.is_zero() {
            return Err(Failure::TimedOut);
        }

        match TcpStream::connect_timeout(&address, attempt) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = Some(error),
        }
    }

    Err(match last {
        Some(error) => classify(&error),
        None => Failure::Offline,
    })
}

fn write_packet(stream: &TcpStream, payload: &[u8]) -> Result<(), Failure> {
    let framed = frame_packet(payload);
    let mut handle = stream;
    handle
        .write_all(&framed)
        .map_err(|error| classify(&error))?;
    handle.flush().map_err(|error| classify(&error))
}

fn classify(error: &io::Error) -> Failure {
    match error.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Failure::TimedOut,
        _ => Failure::Offline,
    }
}

const fn failure_from(error: FrameError) -> Failure {
    match error {
        FrameError::TimedOut => Failure::TimedOut,
        FrameError::Truncated => Failure::Offline,
        FrameError::VarIntTooLong | FrameError::InvalidLength(_) => Failure::Unexpected,
    }
}

/// A socket reader that re-arms the receive timeout from the remaining budget
/// before every read, so the last read cannot outlive the exchange.
struct BudgetReader<'a> {
    stream: &'a TcpStream,
    deadline: Deadline,
}

impl Read for BudgetReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let Some(remaining) = self.deadline.remaining() else {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the ping budget is spent",
            ));
        };

        let step = remaining.min(OPERATION_TIMEOUT);
        if step.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the ping budget is spent",
            ));
        }

        self.stream.set_read_timeout(Some(step))?;
        let mut handle = self.stream;
        handle.read(buffer)
    }
}

/// One row's ping request: the tab's row key and the address text to ping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingJob {
    /// Opaque caller key, echoed back in the [`PingReport`].
    pub id: String,
    /// Address text as the user saved it; resolution and parsing happen on the worker.
    pub address: String,
}

/// One finished ping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingReport {
    pub id: String,
    pub result: ServerPingResult,
}

/// The seam the tab drives: a real worker pool in the app, a canned queue in a test.
pub trait PingBackend: fmt::Debug + Send + Sync {
    /// Queues `jobs` and answers how many were accepted; zero once the pool is gone.
    fn enqueue(&self, jobs: Vec<PingJob>) -> usize;

    /// Drains every finished ping.
    fn take_finished(&self) -> Vec<PingReport>;

    /// How many workers the backend actually has.
    fn workers(&self) -> usize;

    /// Asks the backend to abandon the pass in flight.
    fn cancel(&self);
}

/// What the workers pull from and publish into.
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    results: Mutex<mpsc::Sender<PingReport>>,
    cancelled: AtomicBool,
    srv: SrvResolver,
}

impl fmt::Debug for Shared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shared")
            .field("queue", &self.queue)
            .field("cancelled", &self.cancelled.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct Queue {
    jobs: VecDeque<PingJob>,
    /// Jobs taken off the queue and not yet reported.
    active: usize,
    closed: bool,
}

/// A fixed pool of ping workers: a pass is queued, never one thread per row.
///
/// The pool is created once and answers every pass; the tab owns one and drains
/// [`PingService::take_finished`] once a frame. A worker that is mid-exchange when
/// the pool is dropped finishes it and exits, so dropping the tab never blocks.
pub struct PingService {
    shared: Arc<Shared>,
    results: Mutex<mpsc::Receiver<PingReport>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl fmt::Debug for PingService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PingService")
            .field("pending", &self.pending())
            .field("workers", &self.worker_count())
            .finish_non_exhaustive()
    }
}

impl PingService {
    /// Starts the pool over the public resolvers.
    #[must_use]
    pub fn start() -> Self {
        Self::with_resolvers(Vec::new())
    }

    /// Starts the pool over `resolvers`, which fall back to the public pair when
    /// empty, as `GetDnsServers()` did.
    ///
    /// A test build starts no worker, so no test here can open a socket; that keeps
    /// the shipped path one function rather than two.
    #[must_use]
    pub fn with_resolvers(resolvers: Vec<String>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            results: Mutex::new(sender),
            cancelled: AtomicBool::new(false),
            srv: SrvResolver::new(resolvers),
        });

        let service = Self {
            shared: Arc::clone(&shared),
            results: Mutex::new(receiver),
            workers: Mutex::new(Vec::new()),
        };

        if cfg!(test) {
            return service;
        }

        let mut workers = lock(&service.workers);
        for index in 0..PING_PARALLELISM {
            let shared = Arc::clone(&shared);
            let name = format!("aoko-server-ping-{index}");
            match std::thread::Builder::new()
                .name(name)
                .spawn(move || work(&shared))
            {
                Ok(worker) => workers.push(worker),
                Err(error) => tracing::warn!("cannot start a server-list ping worker: {error}"),
            }
        }
        drop(workers);

        service
    }

    /// Jobs queued or in flight.
    #[must_use]
    pub fn pending(&self) -> usize {
        let queue = lock(&self.shared.queue);
        queue.jobs.len() + queue.active
    }

    /// Whether the pool has nothing left to do.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.pending() == 0
    }

    /// Whether a cancel was asked for.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.shared.cancelled.load(Ordering::Acquire)
    }

    fn worker_count(&self) -> usize {
        lock(&self.workers).len()
    }
}

impl PingBackend for PingService {
    fn enqueue(&self, jobs: Vec<PingJob>) -> usize {
        if jobs.is_empty() {
            return 0;
        }

        let count = jobs.len();
        // A new pass is a new decision: an earlier cancel does not apply to it.
        self.shared.cancelled.store(false, Ordering::Release);
        {
            let mut queue = lock(&self.shared.queue);
            if queue.closed {
                return 0;
            }
            queue.jobs.extend(jobs);
        }
        self.shared.wake.notify_all();
        count
    }

    fn take_finished(&self) -> Vec<PingReport> {
        let receiver = lock(&self.results);
        let mut finished = Vec::new();
        while let Ok(report) = receiver.try_recv() {
            finished.push(report);
        }
        finished
    }

    fn workers(&self) -> usize {
        self.worker_count()
    }

    fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::Release);
    }
}

impl Drop for PingService {
    fn drop(&mut self) {
        {
            let mut queue = lock(&self.shared.queue);
            queue.closed = true;
            queue.jobs.clear();
        }
        self.shared.wake.notify_all();
        // The handles are dropped rather than joined: a worker in the middle of a
        // ten second exchange must not hold up anything, and it exits on its own once
        // it sees the pool closed.
        lock(&self.workers).clear();
    }
}

fn work(shared: &Arc<Shared>) {
    loop {
        let job = {
            let mut queue = lock(&shared.queue);
            while queue.jobs.is_empty() && !queue.closed {
                queue = shared
                    .wake
                    .wait(queue)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            if queue.closed {
                return;
            }

            let Some(job) = queue.jobs.pop_front() else {
                continue;
            };
            queue.active += 1;
            job
        };

        let result = ping_text(&job.address, &shared.srv, &shared.cancelled);
        {
            let sender = lock(&shared.results);
            let _ = sender.send(PingReport { id: job.id, result });
        }

        {
            let mut queue = lock(&shared.queue);
            queue.active = queue.active.saturating_sub(1);
        }
        shared.wake.notify_all();
    }
}

/// A poisoned lock still holds a usable queue, so it is recovered rather than
/// turned into a panic on a worker thread.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn deadline() -> Deadline {
        Deadline::new(Duration::from_secs(1))
    }

    /// Serves at most one byte per read, so the partial-read loop is exercised.
    struct ChunkedReader {
        inner: Cursor<Vec<u8>>,
        chunk: usize,
    }

    impl ChunkedReader {
        fn new(bytes: Vec<u8>, chunk: usize) -> Self {
            Self {
                inner: Cursor::new(bytes),
                chunk,
            }
        }
    }

    impl Read for ChunkedReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let limit = buffer.len().min(self.chunk);
            if limit == 0 {
                return Ok(0);
            }
            self.inner.read(&mut buffer[..limit])
        }
    }

    #[test]
    fn a_varint_survives_the_round_trip_through_the_buffer_reader() {
        for value in [0, 1, 127, 128, 255, 767, 25565, 32767] {
            let mut bytes = Vec::new();
            write_var_int(&mut bytes, value);

            let mut offset = 0;
            assert_eq!(read_var_int(&bytes, &mut offset), Some(value));
            assert_eq!(offset, bytes.len(), "the reader consumes the whole varint");
        }
    }

    #[test]
    fn a_varint_with_a_sixth_continuation_byte_is_rejected() {
        let bytes = [0x80, 0x80, 0x80, 0x80, 0x80, 0x00];

        let mut offset = 0;
        assert_eq!(read_var_int(&bytes, &mut offset), None);
    }

    #[test]
    fn a_truncated_varint_is_rejected() {
        let bytes = [0x80, 0x80];

        let mut offset = 0;
        assert_eq!(read_var_int(&bytes, &mut offset), None);
    }

    #[test]
    fn the_handshake_announces_protocol_767_and_the_big_endian_port() {
        let payload = build_handshake("mc.example.com", 25565);

        assert_eq!(payload[0], 0x00);
        assert_eq!(payload[1], 0xFF);
        assert_eq!(payload[2], 0x05);
        assert_eq!(payload.len(), 1 + 2 + 1 + "mc.example.com".len() + 2 + 1);

        let port = payload.len() - 3;
        assert_eq!(payload[port], 0x63);
        assert_eq!(payload[port + 1], 0xDD);
        assert_eq!(payload[port + 2], 0x01, "next state is status");
    }

    #[test]
    fn a_packet_is_read_out_of_its_frame() {
        let mut reader = Cursor::new(vec![0x01, 0x42]);

        let payload = read_packet(&mut reader, deadline()).expect("a framed packet");

        assert_eq!(payload, vec![0x42]);
    }

    #[test]
    fn a_packet_delivered_in_pieces_is_reassembled() {
        let mut reader = ChunkedReader::new(vec![0x05, 0x00, 0x01, 0x02, 0x03, 0x04], 1);

        let payload = read_packet(&mut reader, deadline()).expect("a framed packet");

        assert_eq!(payload, vec![0x00, 0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn a_zero_or_oversized_length_is_refused_before_anything_is_allocated() {
        for framed in [vec![0x00], vec![0x80, 0x80, 0x02, 0x00]] {
            let mut reader = Cursor::new(framed);
            match read_packet(&mut reader, deadline()) {
                Err(FrameError::InvalidLength(_)) => {}
                other => panic!("expected an invalid length, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_truncated_packet_is_reported_rather_than_padded() {
        let mut reader = Cursor::new(vec![0x05, 0x00]);

        assert_eq!(
            read_packet(&mut reader, deadline()),
            Err(FrameError::Truncated)
        );
    }

    #[test]
    fn a_status_document_reports_players_version_and_motd() {
        let result = parse_status(
            "{\"version\":{\"name\":\"1.21.4\",\"protocol\":769},\"players\":{\"max\":100,\"online\":12},\"description\":{\"text\":\"Test\"}}",
            42,
        )
        .expect("a status document");

        assert!(result.online);
        assert_eq!(result.latency_ms, 42);
        assert_eq!(result.online_players, 12);
        assert_eq!(result.max_players, 100);
        assert_eq!(result.description, "Test");
        assert_eq!(result.version, "1.21.4");
        assert_eq!(result.protocol, 769);
        assert_eq!(result.players_label(), "12/100");
        assert!(result.error.is_empty());
    }

    #[test]
    fn a_minimal_status_document_still_counts_as_online() {
        let result = parse_status("{}", 7).expect("a status document");

        assert!(result.online);
        assert_eq!(result.latency_ms, 7);
        assert_eq!(result.description, "");
        assert_eq!(result.online_players, 0);
        assert_eq!(result.max_players, 0);
        assert_eq!(result.version, "");
        assert_eq!(result.players_label(), "0/0");
    }

    #[test]
    fn a_status_document_that_is_not_an_object_is_not_a_ping_we_can_use() {
        assert!(parse_status("null", 1).is_none());
        assert!(parse_status("[1, 2]", 1).is_none());
        assert!(parse_status("not json", 1).is_none());
    }

    #[test]
    fn the_offline_players_label_is_the_managed_em_dash() {
        let offline = ServerPingResult::offline(OFFLINE_LABEL);
        assert_eq!(offline.players_label(), "\u{2014}");
        assert!(!offline.online);

        let online = ServerPingResult {
            online: true,
            ..ServerPingResult::default()
        };
        assert_eq!(online.players_label(), "0/0");
    }

    #[test]
    fn the_motd_is_joined_and_its_colours_stripped() {
        let cases: [(&str, &str); 7] = [
            ("\"Hello World\"", "Hello World"),
            ("{\"text\":\"Hello World\"}", "Hello World"),
            (
                "{\"text\":\"Hello \",\"extra\":[{\"text\":\"\u{a7}aWorld\"}]}",
                "Hello World",
            ),
            (
                "{\"text\":\"\u{a7}c\u{a7}lShop \u{a7}r\u{a7}7| \u{a7}aOpen\"}",
                "Shop | Open",
            ),
            ("[{\"text\":\"A\"},\"B\",{\"text\":\"C\"}]", "ABC"),
            ("{\"text\":\"\u{a7}zX\"}", "\u{a7}zX"),
            ("\"  padded  \"", "padded"),
        ];

        for (json, expected) in cases {
            let value: Value = serde_json::from_str(json).expect("valid json");
            assert_eq!(flatten_motd(&value), expected, "for {json}");
        }
    }

    #[test]
    fn every_colour_code_is_dropped_and_anything_else_after_the_marker_stays() {
        assert_eq!(strip_colour_codes("\u{a7}0\u{a7}1\u{a7}2\u{a7}3"), "");
        assert_eq!(
            strip_colour_codes("\u{a7}aG\u{a7}lL\u{a7}rR"),
            "GLR",
            "colours and formats are both dropped"
        );
        assert_eq!(strip_colour_codes("\u{a7}z"), "\u{a7}z");
        assert_eq!(strip_colour_codes("100\u{a7}"), "100\u{a7}");
    }

    #[test]
    fn a_section_one_legacy_reply_reads_as_online() {
        let reply = legacy_reply("\u{a7}1\u{0}100\u{0}1.6.4\u{0}A Minecraft Server\u{0}12\u{0}20");

        let result = parse_legacy_response(&reply).expect("a legacy reply");

        assert!(result.online);
        assert_eq!(result.protocol, 100);
        assert_eq!(result.version, "1.6.4");
        assert_eq!(result.description, "A Minecraft Server");
        assert_eq!(result.online_players, 12);
        assert_eq!(result.max_players, 20);
        assert_eq!(result.players_label(), "12/20");
    }

    #[test]
    fn a_beta_era_legacy_reply_reads_as_online() {
        let reply = legacy_reply("A Minecraft Server\u{a7}3\u{a7}8");

        let result = parse_legacy_response(&reply).expect("a legacy reply");

        assert!(result.online);
        assert_eq!(result.description, "A Minecraft Server");
        assert_eq!(result.online_players, 3);
        assert_eq!(result.max_players, 8);
    }

    #[test]
    fn a_modern_reply_is_not_mistaken_for_a_legacy_one() {
        assert!(parse_legacy_response(&[0x00, 0x01, 0x02]).is_none());
        assert!(parse_legacy_response(&[0xFF]).is_none());
        assert!(
            parse_legacy_response(&[0xFF, 0x00, 0x40, 0x00]).is_none(),
            "the character count outruns the body"
        );
    }

    #[test]
    fn the_address_rules_are_the_managed_ones() {
        let parsed = |text: &str| ServerAddress::try_parse(Some(text));

        assert_eq!(
            parsed("  play.example.net  "),
            Some(ServerAddress::new("play.example.net", 25565, false)),
            "whitespace is trimmed and a bare host defaults to 25565"
        );
        assert_eq!(
            parsed("play.example.net:25566"),
            Some(ServerAddress::new("play.example.net", 25566, true))
        );
        assert_eq!(
            parsed("[2001:db8::1]:25565"),
            Some(ServerAddress::new("2001:db8::1", 25565, true)),
            "IPv6 is bracketed"
        );
        assert_eq!(
            parsed("[2001:db8::1]"),
            Some(ServerAddress::new("2001:db8::1", 25565, false))
        );
        assert_eq!(
            parsed("2001:db8::1"),
            Some(ServerAddress::new("2001:db8::1", 25565, false)),
            "more than one colon means the whole text is the host"
        );
        assert_eq!(
            parsed("spielplatz.example"),
            Some(ServerAddress::new("spielplatz.example", 25565, false)),
            "a non-ASCII host is kept as typed; IDN resolution is the resolver's job"
        );

        assert_eq!(parsed(""), None);
        assert_eq!(parsed("   "), None);
        assert_eq!(parsed("host:0"), None, "port zero is not a port");
        assert_eq!(parsed("host:65536"), None);
        assert_eq!(parsed("host:port"), None);
        assert_eq!(parsed("[]"), None);
        assert_eq!(parsed("[2001:db8::1]:"), None);
        assert_eq!(parsed("a b"), None);
        assert_eq!(parsed("http://example.net"), None);
        assert_eq!(parsed(&"h".repeat(257)), None, "the text cap is 256");
    }

    #[test]
    fn an_unparsable_address_never_reaches_a_socket() {
        let cancelled = AtomicBool::new(false);
        let result = ping_text("bad host", &SrvResolver::new(Vec::new()), &cancelled);

        assert!(!result.online);
        assert_eq!(result.error, INVALID_ADDRESS_MESSAGE);
    }

    #[test]
    fn a_cancelled_pass_is_reported_as_cancelled_before_any_resolution() {
        let cancelled = AtomicBool::new(true);
        let result = ping_text(
            "play.example.com",
            &SrvResolver::new(Vec::new()),
            &cancelled,
        );

        assert!(!result.online);
        assert_eq!(result.error, CANCELLED_LABEL);
    }

    #[test]
    fn a_test_build_starts_no_worker_and_still_queues() {
        let service = PingService::start();
        assert_eq!(service.workers(), 0, "no test may open a socket");
        assert!(service.is_idle());

        let accepted = service.enqueue(vec![PingJob {
            id: "row".to_owned(),
            address: "play.example.net".to_owned(),
        }]);

        assert_eq!(accepted, 1);
        assert_eq!(service.pending(), 1);
        assert!(!service.is_idle());
        assert!(service.take_finished().is_empty());
        assert!(!service.is_cancelled());

        service.cancel();
        assert!(service.is_cancelled());
    }

    #[test]
    fn an_empty_pass_queues_nothing() {
        let service = PingService::start();
        assert_eq!(service.enqueue(Vec::new()), 0);
        assert!(service.is_idle());
    }

    #[test]
    fn a_dropped_pool_refuses_new_jobs() {
        let service = PingService::start();
        let shared = Arc::clone(&service.shared);
        drop(service);

        assert!(lock(&shared.queue).closed);
    }

    /// A legacy reply: the kick byte, the character count, then UTF-16 big-endian.
    fn legacy_reply(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut reply = vec![0xFF];
        reply.extend_from_slice(&(units.len() as u16).to_be_bytes());
        for unit in units {
            reply.extend_from_slice(&unit.to_be_bytes());
        }
        reply
    }
}
