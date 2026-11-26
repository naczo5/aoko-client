//! Transport seam between the wire protocol and any byte-oriented socket.
//!
//! The protocol crate must not know what a socket is. It owns framing and
//! schema, and asks a [`Transport`] for newline-delimited lines. That keeps the
//! whole codec testable against an in-memory duplex peer, which is what the
//! `GameStateClientSeamTests` and `GameStateClientInjectionTests` fakes do in
//! the C# suite.

use std::fmt;

/// Hard inbound cap from `Aoko/Core/GameStateClient.cs`
/// (`MaximumInboundMessageCharacters = 1024 * 1024`). A peer must never be able
/// to grow our buffer past this.
pub const MAX_INBOUND_LINE_BYTES: usize = 1024 * 1024;

/// Read buffer size used by `BoundedLineReader` in the C# host.
pub const READ_BUFFER_BYTES: usize = 4096;

/// A reusable, capacity-bounded line buffer.
///
/// `clear` keeps the allocation so steady-state frames do not hit the allocator.
#[derive(Debug, Default)]
pub struct LineBuf {
    bytes: Vec<u8>,
}

impl LineBuf {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
        }
    }

    /// Drops the current contents but keeps the heap allocation.
    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    pub fn extend_from_slice(&mut self, slice: &[u8]) {
        self.bytes.extend_from_slice(slice);
    }

    pub fn push(&mut self, byte: u8) {
        self.bytes.push(byte);
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        // Frames are UTF-8 JSON; the codec validates before holding them here.
        std::str::from_utf8(&self.bytes).unwrap_or_default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Outcome of asking a transport for one line.
///
/// `TooLong` mirrors `BoundedLineReadResult.IsTooLong`: an oversized line is
/// consumed and discarded up to its newline so the stream stays aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecvOutcome {
    /// A complete line was written into the caller's buffer, trailing `\r`
    /// already stripped.
    Line,
    /// An oversized line was discarded up to and including its newline.
    TooLong,
    /// The peer closed and no partial line was pending.
    EndOfStream,
}

#[derive(Debug)]
pub enum TransportError {
    Io(std::io::Error),
    /// The peer closed mid-frame.
    Closed,
    /// A frame exceeded [`MAX_INBOUND_LINE_BYTES`].
    LineTooLong {
        limit: usize,
    },
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "transport io error: {err}"),
            Self::Closed => f.write_str("transport closed"),
            Self::LineTooLong { limit } => {
                write!(f, "line exceeded the {limit} byte inbound cap")
            }
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TransportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// A blocking, newline-framed byte channel.
///
/// Implementations are platform-owned; the protocol crate only sees this trait.
/// `recv_line` writes into the caller's buffer instead of returning an owned
/// `String`, which is what keeps steady-state frames allocation-free.
pub trait Transport: Send {
    /// Reads one line into `out`, appending to it. The caller is expected to
    /// `clear()` first if it wants only the newest line.
    ///
    /// Implementations must enforce [`MAX_INBOUND_LINE_BYTES`] and strip a
    /// single trailing `\r` before the `\n`.
    fn recv_line(&mut self, out: &mut LineBuf) -> Result<RecvOutcome, TransportError>;

    /// Writes one already-serialized line, appending `\n` if absent.
    fn send_line(&mut self, line: &[u8]) -> Result<(), TransportError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_buf_reuses_capacity_across_clear() {
        let mut buf = LineBuf::with_capacity(64);
        buf.extend_from_slice(b"{\"type\":\"state\"}");
        let capacity_before = buf.bytes.capacity();
        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.bytes.capacity(), capacity_before);
    }

    #[test]
    fn line_buf_len_and_slice_agree() {
        let mut buf = LineBuf::new();
        buf.extend_from_slice(b"abc");
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.as_slice(), b"abc");
        assert_eq!(buf.as_str(), "abc");
    }

    #[test]
    fn inbound_cap_matches_the_managed_client() {
        assert_eq!(MAX_INBOUND_LINE_BYTES, 1024 * 1024);
        assert_eq!(READ_BUFFER_BYTES, 4096);
    }
}

/// In-memory duplex peer for tests and for the host's own seam tests.
///
/// The managed suite fakes the socket; this is the equivalent. Bytes fed into the
/// peer are framed by the same [`crate::framing::BoundedLineReader`], so the CRLF,
/// cap, and oversized-line rules are exercised end to end without a socket.
///
/// The client end appends to the caller's buffer, exactly as [`Transport::recv_line`]
/// requires. It used to clear that buffer first, which made a reader that forgot to
/// clear its own buffer look correct and let `TransportClient::pump` ship without the
/// clear; a fake that quietly satisfies a contract is worse than no fake.
pub mod memory {
    use std::collections::VecDeque;
    use std::io::Read;
    use std::sync::{Arc, Mutex, MutexGuard};

    use super::{LineBuf, RecvOutcome, Transport, TransportError};
    use crate::framing::BoundedLineReader;

    #[derive(Debug, Default)]
    struct Shared {
        inbound: VecDeque<u8>,
        sent: Vec<Vec<u8>>,
    }

    fn lock(shared: &Arc<Mutex<Shared>>) -> MutexGuard<'_, Shared> {
        match shared.lock() {
            Ok(guard) => guard,
            // A panicking test must not turn every later read into a poison error.
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// The peer end: feeds inbound bytes and inspects what the client wrote.
    #[derive(Debug, Clone)]
    pub struct MemoryPeer {
        shared: Arc<Mutex<Shared>>,
    }

    impl MemoryPeer {
        /// Appends bytes the client will read. Empty inbound behaves as EOF.
        pub fn feed(&self, bytes: &[u8]) {
            let mut shared = lock(&self.shared);
            shared.inbound.extend(bytes.iter().copied());
        }

        /// Every line the client has written, in order, without the trailing LF.
        #[must_use]
        pub fn sent_lines(&self) -> Vec<String> {
            let shared = lock(&self.shared);
            shared
                .sent
                .iter()
                .map(|line| String::from_utf8_lossy(line).into_owned())
                .collect()
        }

        /// The newest written line, if any.
        #[must_use]
        pub fn last_sent(&self) -> Option<String> {
            let shared = lock(&self.shared);
            shared
                .sent
                .last()
                .map(|line| String::from_utf8_lossy(line).into_owned())
        }
    }

    /// Reads from the peer's inbound queue; empty means end of stream.
    #[derive(Debug)]
    struct QueueReader {
        shared: Arc<Mutex<Shared>>,
    }

    impl Read for QueueReader {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let mut shared = lock(&self.shared);
            if shared.inbound.is_empty() {
                return Ok(0);
            }

            let take = out.len().min(shared.inbound.len());
            for slot in out.iter_mut().take(take) {
                *slot = shared.inbound.pop_front().unwrap_or_default();
            }
            Ok(take)
        }
    }

    /// Client end of the in-memory pair.
    #[derive(Debug)]
    pub struct MemoryTransport {
        reader: BoundedLineReader<QueueReader>,
        /// The line the reader produced, reused across calls. `BoundedLineReader`
        /// replaces the buffer it is given, so appending what the caller asked for means
        /// staging here first.
        line: LineBuf,
        shared: Arc<Mutex<Shared>>,
    }

    impl Transport for MemoryTransport {
        fn recv_line(&mut self, out: &mut LineBuf) -> Result<RecvOutcome, TransportError> {
            let outcome = self
                .reader
                .read_line(&mut self.line)
                .map_err(TransportError::Io)?;
            if outcome == RecvOutcome::Line {
                out.extend_from_slice(self.line.as_slice());
            }
            Ok(outcome)
        }

        fn send_line(&mut self, line: &[u8]) -> Result<(), TransportError> {
            let mut shared = lock(&self.shared);
            let mut stored = line.to_vec();
            if stored.last() != Some(&b'\n') {
                stored.push(b'\n');
            }
            shared.sent.push(stored);
            Ok(())
        }
    }

    /// Creates the pair, both ends honouring the 1 MiB inbound cap.
    #[must_use]
    pub fn duplex() -> (MemoryTransport, MemoryPeer) {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let transport = MemoryTransport {
            reader: BoundedLineReader::with_wire_cap(QueueReader {
                shared: Arc::clone(&shared),
            }),
            line: LineBuf::new(),
            shared: Arc::clone(&shared),
        };
        (transport, MemoryPeer { shared })
    }
}
