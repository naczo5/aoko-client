//! The real TCP transport between the host and a bridge.
//!
//! Roles come from `docs/rust/WIRE_CONTRACT.md`: **the bridge is the server** and
//! the host dials in, so everything here is a client. Address is the constant
//! `127.0.0.1:25590`; there is no discovery, no fallback port and no retry on a
//! different port.
//!
//! Framing is the byte-level half of the contract and is shared by both OSes:
//! one UTF-8 JSON document per line, `\n` terminated, a single trailing `\r`
//! stripped on read, no length prefix, no keepalive, no TLS.
//!
//! One connection has two handles because the reader blocks in `recv_line` for
//! as long as the bridge is quiet: [`TcpBridgeTransport::try_clone_writer`]
//! hands the sender its own handle on the same socket, so a config write never
//! waits for the pump. The handles share one frame mutex, which is what keeps a
//! frame from being torn by two concurrent writers; see [`TcpBridgeWriter`].

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aoko_protocol::transport::{
    LineBuf, RecvOutcome, Transport, TransportError, MAX_INBOUND_LINE_BYTES, READ_BUFFER_BYTES,
};
use aoko_protocol::BRIDGE_PORT;

use crate::error::PlatformError;

/// Loopback address every bridge binds and every host dials.
///
/// Loopback only, by contract: the port carries the live session, including the
/// access token `setSession` writes into the game.
pub const BRIDGE_ADDRESS: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), BRIDGE_PORT);

/// `GameStateClient.SendTimeout`: ceiling for one command or config write.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// A blocking, newline-framed TCP client.
///
/// The read side keeps its own buffer and a consumed-prefix cursor, so a run of
/// short frames does not memmove the buffer for every line.
pub struct TcpBridgeTransport {
    stream: TcpStream,
    received: Vec<u8>,
    consumed: usize,
    /// Set after an oversized line was detected; stays set until its newline is
    /// found, mirroring `BoundedLineReader`'s discard state.
    discard_until_newline: bool,
    send_buffer: Vec<u8>,
    /// Serialises whole frames between this handle and every writer cloned from
    /// it. A duplicated handle is the same socket, and `write_all` can issue more
    /// than one `write` when a frame outgrows the send buffer, so two writers
    /// without this could interleave two frames into one torn line.
    frame_lock: Arc<Mutex<()>>,
}

impl TcpBridgeTransport {
    /// Dials `address` with no explicit connect timeout, matching the managed
    /// `TcpClient.ConnectAsync` call.
    pub fn connect(address: impl ToSocketAddrs) -> Result<Self, TransportError> {
        let stream = TcpStream::connect(address)?;
        Self::from_stream(stream)
    }

    /// Dials with a deadline. The managed host relied on the OS default; this is
    /// offered because a blocking UI cannot wait for a black-holed SYN.
    pub fn connect_timeout(
        address: impl ToSocketAddrs,
        timeout: Duration,
    ) -> Result<Self, TransportError> {
        let mut last_error = None;
        for candidate in address.to_socket_addrs()? {
            match TcpStream::connect_timeout(&candidate, timeout) {
                Ok(stream) => return Self::from_stream(stream),
                Err(err) => last_error = Some(err),
            }
        }
        Err(TransportError::Io(last_error.unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no socket address resolved",
            )
        })))
    }

    /// Dials the constant bridge endpoint.
    pub fn connect_bridge() -> Result<Self, TransportError> {
        Self::connect(BRIDGE_ADDRESS)
    }

    /// Adopts an already-connected socket.
    pub fn from_stream(stream: TcpStream) -> Result<Self, TransportError> {
        // `NetworkStream.WriteAsync` inherited the client's 10 s `SendTimeout`.
        stream.set_write_timeout(Some(SEND_TIMEOUT))?;
        // No read timeout: the managed host never times out a silent bridge and
        // infers liveness from the state stream.
        stream.set_read_timeout(None)?;
        Ok(Self {
            stream,
            received: Vec::with_capacity(READ_BUFFER_BYTES * 4),
            consumed: 0,
            discard_until_newline: false,
            send_buffer: Vec::with_capacity(READ_BUFFER_BYTES),
            frame_lock: Arc::new(Mutex::new(())),
        })
    }

    /// A second handle for a dedicated sender, so a config write does not wait
    /// for [`Transport::recv_line`] to return.
    ///
    /// Available and exported rather than unused: a caller that writes from the
    /// pump's own thread does not need it, because one writer cannot tear a
    /// frame, but a write that has to reach the wire while the reader is blocked
    /// does, and there is no other supported way to get one. Nothing here is a
    /// stub.
    ///
    /// `TcpStream::try_clone` is `dup(2)` on Unix and `WSADuplicateSocket` +
    /// `WSASocket` on Windows, so this is a genuinely separate handle with its
    /// own close: dropping it leaves the connection open, and the connection
    /// closes when every handle is gone. It is the *same* TCP connection and the
    /// same kernel send buffer, and `std::net::TcpStream` keeps no user-space
    /// buffer, so `flush` is a no-op on both platforms - there is no flush
    /// ordering between the handles to get right.
    ///
    /// What does need ordering is the frames: `write_all` can issue several
    /// `write` calls for one frame, so two writers on one socket can interleave
    /// two frames into a line neither side wrote. The new handle shares this
    /// transport's frame mutex, so [`TcpBridgeWriter::send_line`] and
    /// [`Transport::send_line`] cannot tear each other: many handles may write,
    /// one frame at a time.
    ///
    /// The handle starts with empty buffers. The inbound buffer, the consumed
    /// cursor and the discard flag stay with the reader, because a half-read
    /// frame belongs to exactly one of them.
    ///
    /// Socket options live on the socket, not the handle, so the 10 s
    /// [`SEND_TIMEOUT`] and the absent read timeout set by [`Self::from_stream`]
    /// apply to both. Nothing here adds a read timeout: a silent bridge stays
    /// silent rather than surfacing `WouldBlock` as a torn connection.
    pub fn try_clone_writer(&self) -> Result<TcpBridgeWriter, TransportError> {
        let stream = self.stream.try_clone()?;
        Ok(TcpBridgeWriter {
            stream,
            send_buffer: Vec::with_capacity(READ_BUFFER_BYTES),
            frame_lock: Arc::clone(&self.frame_lock),
        })
    }

    /// The peer's address, for logging which process answered.
    pub fn peer_addr(&self) -> Result<SocketAddr, PlatformError> {
        self.stream
            .peer_addr()
            .map_err(|err| PlatformError::os("TcpStream::peer_addr", err.to_string()))
    }

    #[must_use]
    pub fn is_discarding(&self) -> bool {
        self.discard_until_newline
    }

    fn pending(&self) -> &[u8] {
        // `consumed` only ever advances past bytes that were already handled.
        self.received.get(self.consumed..).unwrap_or_default()
    }

    fn compact(&mut self) {
        if self.consumed == 0 {
            return;
        }
        if self.consumed >= self.received.len() {
            self.received.clear();
            self.consumed = 0;
            return;
        }
        // Only pay the memmove once the dead prefix outweighs the live suffix.
        if self.consumed >= READ_BUFFER_BYTES {
            self.received.drain(..self.consumed);
            self.consumed = 0;
        }
    }
}

/// The write half of a bridge connection, for a sender that must not wait for
/// the pump to return from a read.
///
/// Obtained from [`TcpBridgeTransport::try_clone_writer`], which carries the
/// platform answer in full: a duplicated handle on the same socket, no
/// user-space buffering, and no flush ordering to respect.
///
/// The frame mutex is not redundant bookkeeping, and the next reader of a bare
/// `Mutex<()>` here should not delete it. `try_clone` is `dup(2)` on Unix and
/// `WSADuplicateSocket` on Windows, so both handles write into one kernel send
/// buffer, and one `write_all` can issue several `write` calls when that buffer
/// is full. Two handles writing at once therefore put bytes of two different
/// frames on the wire in an order neither side wrote: a corrupt JSON line the
/// bridge can only reject or misparse. No flush discipline prevents that,
/// because `std::net::TcpStream` has nothing to flush. Holding a single lock
/// across the whole `write_all` is what makes a frame whole, and the lock is
/// shared with the reader's handle so both send paths obey the same one.
///
/// It holds its own frame buffer and nothing else - no inbound bytes, no
/// consumed cursor, no discard state - so a half-read frame can never be
/// replayed onto the wire by this handle.
pub struct TcpBridgeWriter {
    stream: TcpStream,
    send_buffer: Vec<u8>,
    frame_lock: Arc<Mutex<()>>,
}

impl TcpBridgeWriter {
    /// Writes one line frame, appending `\n` when the line does not already end
    /// with one.
    ///
    /// A write failure is [`TransportError::Io`], exactly as
    /// [`Transport::send_line`] reports it, and it never panics: on Unix the
    /// runtime ignores `SIGPIPE`, so a closed peer surfaces as `EPIPE`; on
    /// Windows it surfaces as `WSAECONNRESET` or `WSAENOTCONN`. The caller owns
    /// the reaction, because a torn send means the connection is suspect and
    /// only the caller knows whether to redial.
    pub fn send_line(&mut self, line: &str) -> Result<(), TransportError> {
        frame_into(&mut self.send_buffer, line.as_bytes());
        write_frame(&mut self.stream, &self.send_buffer, &self.frame_lock)
    }
}

/// Builds one frame in place: the line, then exactly one `\n`.
fn frame_into(buffer: &mut Vec<u8>, line: &[u8]) {
    buffer.clear();
    buffer.extend_from_slice(line);
    if buffer.last() != Some(&b'\n') {
        buffer.push(b'\n');
    }
}

/// Writes one already-framed line while holding the connection's frame lock.
///
/// The lock is what makes the frame whole; `flush` is kept for symmetry with a
/// future buffered writer, and is a no-op for a raw `TcpStream` on both
/// platforms. A poisoned lock only means another handle panicked while holding
/// it - the mutex guards ordering, not data - so recovering the guard is
/// correct and no send is lost because of a panic elsewhere.
///
/// A writer blocked on a full send buffer holds the lock for up to
/// [`SEND_TIMEOUT`], which delays the next frame but can never interleave it.
fn write_frame(
    stream: &mut TcpStream,
    buffer: &[u8],
    frame_lock: &Arc<Mutex<()>>,
) -> Result<(), TransportError> {
    let _frame = frame_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    stream.write_all(buffer)?;
    stream.flush()?;
    Ok(())
}

impl Transport for TcpBridgeTransport {
    fn recv_line(&mut self, out: &mut LineBuf) -> Result<RecvOutcome, TransportError> {
        let mut chunk = [0u8; READ_BUFFER_BYTES];

        loop {
            if !self.discard_until_newline {
                if let Some(offset) = self.pending().iter().position(|byte| *byte == b'\n') {
                    let line_start = self.consumed;
                    let newline = self.consumed + offset;
                    // A single trailing `\r` is framing, not content, so it does
                    // not count against the cap (the preserved `BoundedLineReader`
                    // quirk: a max-length CRLF line still completes).
                    let trailing_carriage_return =
                        newline > line_start && self.received.get(newline - 1) == Some(&b'\r');
                    let content_len = offset - usize::from(trailing_carriage_return);

                    let outcome = if content_len > MAX_INBOUND_LINE_BYTES {
                        RecvOutcome::TooLong
                    } else {
                        let end = if trailing_carriage_return {
                            newline - 1
                        } else {
                            newline
                        };
                        out.extend_from_slice(
                            self.received.get(line_start..end).unwrap_or_default(),
                        );
                        RecvOutcome::Line
                    };

                    self.consumed = newline + 1;
                    self.compact();
                    return Ok(outcome);
                }

                let pending = self.pending();
                let trailing_carriage_return = pending.last().is_some_and(|byte| *byte == b'\r');
                let content_len = pending.len() - usize::from(trailing_carriage_return);
                if content_len > MAX_INBOUND_LINE_BYTES {
                    // Past the cap with no terminator: drop what we hold and stay
                    // in the discard state until the next newline.
                    self.discard_until_newline = true;
                    self.received.clear();
                    self.consumed = 0;
                }
            }

            let read = self.stream.read(&mut chunk)?;
            if read == 0 {
                return Ok(RecvOutcome::EndOfStream);
            }
            let read_chunk = chunk.get(..read).unwrap_or_default();

            if self.discard_until_newline {
                if let Some(offset) = read_chunk.iter().position(|byte| *byte == b'\n') {
                    self.discard_until_newline = false;
                    self.received.clear();
                    self.consumed = 0;
                    self.received
                        .extend_from_slice(read_chunk.get(offset + 1..).unwrap_or_default());
                    return Ok(RecvOutcome::TooLong);
                }
                // Still inside the oversized line; nothing to keep.
                continue;
            }

            self.received.extend_from_slice(read_chunk);
        }
    }

    fn send_line(&mut self, line: &[u8]) -> Result<(), TransportError> {
        // One write for one frame, under the same lock the cloned writer uses, so
        // a torn line can never sit on the socket from either handle.
        frame_into(&mut self.send_buffer, line);
        write_frame(&mut self.stream, &self.send_buffer, &self.frame_lock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    fn listener() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind(SocketAddr::new(
            std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
            0,
        ))
        .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        (listener, addr)
    }

    fn read_line(outcome: &mut TcpBridgeTransport) -> String {
        let mut line = LineBuf::new();
        let result = outcome.recv_line(&mut line).expect("recv");
        assert_eq!(result, RecvOutcome::Line, "expected a complete line");
        line.as_str().to_string()
    }

    #[test]
    fn reads_a_single_lf_terminated_line() {
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            socket.write_all(b"{\"type\":\"state\"}\n").expect("write");
        });

        let mut transport = TcpBridgeTransport::connect(addr).expect("connect");
        assert_eq!(read_line(&mut transport), "{\"type\":\"state\"}");
        server.join().expect("server thread");
    }

    #[test]
    fn strips_a_single_trailing_carriage_return() {
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            socket
                .write_all(b"{\"a\":1}\r\n{\"b\":2}\n")
                .expect("write");
        });

        let mut transport = TcpBridgeTransport::connect(addr).expect("connect");
        assert_eq!(read_line(&mut transport), "{\"a\":1}");
        assert_eq!(read_line(&mut transport), "{\"b\":2}");
        server.join().expect("server thread");
    }

    #[test]
    fn reassembles_a_line_split_across_reads() {
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            socket.write_all(b"{\"ty").expect("write part 1");
            socket.flush().expect("flush");
            std::thread::sleep(Duration::from_millis(20));
            socket
                .write_all(b"pe\":\"state\"}\n")
                .expect("write part 2");
        });

        let mut transport = TcpBridgeTransport::connect(addr).expect("connect");
        assert_eq!(read_line(&mut transport), "{\"type\":\"state\"}");
        server.join().expect("server thread");
    }

    #[test]
    fn reports_end_of_stream_after_the_peer_closes() {
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            socket.write_all(b"{\"type\":\"state\"}\n").expect("write");
            // Dropping the socket closes the connection.
        });

        let mut transport = TcpBridgeTransport::connect(addr).expect("connect");
        assert_eq!(read_line(&mut transport), "{\"type\":\"state\"}");
        let mut line = LineBuf::new();
        assert_eq!(
            transport.recv_line(&mut line).expect("recv eof"),
            RecvOutcome::EndOfStream
        );
        server.join().expect("server thread");
    }

    #[test]
    fn discards_an_oversized_line_and_resynchronises_on_the_next_one() {
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let oversized = vec![b'x'; MAX_INBOUND_LINE_BYTES + 64];
            socket.write_all(&oversized).expect("write oversized");
            socket
                .write_all(b"\n{\"type\":\"state\"}\n")
                .expect("write next");
        });

        let mut transport = TcpBridgeTransport::connect(addr).expect("connect");
        let mut line = LineBuf::new();
        assert_eq!(
            transport.recv_line(&mut line).expect("recv oversized"),
            RecvOutcome::TooLong
        );
        assert!(line.is_empty(), "a discarded line must not leak bytes");
        // The stream stays aligned: the following frame still arrives intact.
        line.clear();
        assert_eq!(
            transport.recv_line(&mut line).expect("recv next"),
            RecvOutcome::Line
        );
        assert_eq!(line.as_str(), "{\"type\":\"state\"}");
        server.join().expect("server thread");
    }

    #[test]
    fn accepts_a_line_exactly_at_the_cap() {
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut payload = vec![b'a'; MAX_INBOUND_LINE_BYTES];
            payload.push(b'\n');
            socket.write_all(&payload).expect("write at cap");
        });

        let mut transport = TcpBridgeTransport::connect(addr).expect("connect");
        let mut line = LineBuf::with_capacity(MAX_INBOUND_LINE_BYTES);
        assert_eq!(
            transport.recv_line(&mut line).expect("recv at cap"),
            RecvOutcome::Line
        );
        assert_eq!(line.len(), MAX_INBOUND_LINE_BYTES);
        server.join().expect("server thread");
    }

    #[test]
    fn accepts_a_max_length_line_terminated_with_crlf() {
        // The preserved quirk: at exactly the cap, a following `\r` is kept so
        // the line completes and the `\r` is then stripped.
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut payload = vec![b'a'; MAX_INBOUND_LINE_BYTES - 1];
            payload.extend_from_slice(b"\r\n");
            socket.write_all(&payload).expect("write crlf at cap");
        });

        let mut transport = TcpBridgeTransport::connect(addr).expect("connect");
        let mut line = LineBuf::with_capacity(MAX_INBOUND_LINE_BYTES);
        assert_eq!(
            transport.recv_line(&mut line).expect("recv crlf"),
            RecvOutcome::Line
        );
        assert_eq!(line.len(), MAX_INBOUND_LINE_BYTES - 1);
        server.join().expect("server thread");
    }

    #[test]
    fn send_line_appends_exactly_one_newline() {
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().expect("accept");
            let mut reader = std::io::BufReader::new(socket);
            let mut first = String::new();
            let mut second = String::new();
            reader.read_line(&mut first).expect("read first");
            reader.read_line(&mut second).expect("read second");
            (first, second)
        });

        let mut transport = TcpBridgeTransport::connect(addr).expect("connect");
        transport.send_line(b"{\"type\":\"config\"}").expect("send");
        transport
            .send_line(b"{\"type\":\"moduleAction\"}\n")
            .expect("send");
        let (first, second) = server.join().expect("server thread");
        assert_eq!(first, "{\"type\":\"config\"}\n");
        assert_eq!(second, "{\"type\":\"moduleAction\"}\n");
    }

    #[test]
    fn a_writer_handle_sends_while_the_transport_is_blocked_in_a_read() {
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().expect("accept");
            let mut reader = std::io::BufReader::new(socket);
            let mut config = String::new();
            reader.read_line(&mut config).expect("read config");
            // The reader stays blocked until this line exists, so the write below
            // can only have come from the cloned handle.
            reader
                .get_mut()
                .write_all(b"{\"type\":\"state\"}\n")
                .expect("write state");
            config
        });

        let transport = TcpBridgeTransport::connect(addr).expect("connect");
        let mut writer = transport.try_clone_writer().expect("clone writer");

        let (at_read, wait_at_read) = std::sync::mpsc::sync_channel(0);
        let reader_thread = std::thread::spawn(move || {
            let mut transport = transport;
            // Rendezvous: one statement from blocking in `recv_line`, with the
            // server still silent.
            at_read.send(()).expect("rendezvous");
            read_line(&mut transport)
        });

        wait_at_read.recv().expect("reader reached recv_line");
        writer
            .send_line("{\"type\":\"config\"}")
            .expect("send config");

        assert_eq!(
            reader_thread.join().expect("reader thread"),
            "{\"type\":\"state\"}"
        );
        assert_eq!(
            server.join().expect("server thread"),
            "{\"type\":\"config\"}\n"
        );
    }

    /// Whether this process currently ignores `SIGPIPE`.
    ///
    /// The closed-peer test depends on it: under `SIG_DFL` the write kills the
    /// whole test binary, which CI reports as a crash rather than as a failed
    /// assertion. Reading the disposition back is the difference between
    /// assuming that and checking it.
    #[cfg(target_os = "linux")]
    fn sigpipe_is_ignored() -> bool {
        // SAFETY: all-zero is a valid `sigaction` - every field is an integer or
        // a pointer - and the call below only writes the current disposition.
        let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: `previous` is live and correctly aligned for the duration of
        // the call, and a null `act` installs no handler.
        let queried =
            unsafe { libc::sigaction(libc::SIGPIPE, std::ptr::null(), &mut previous) } == 0;
        queried && previous.sa_sigaction == libc::SIG_IGN
    }

    #[test]
    fn a_send_to_a_closed_peer_is_a_classified_error_not_a_panic() {
        // Guard the assumption this test rests on rather than inheriting it from
        // the runtime: a SIGPIPE handler installed by a harness would turn the
        // write below into a dead process.
        #[cfg(target_os = "linux")]
        assert!(
            sigpipe_is_ignored(),
            "SIGPIPE is not ignored, so writing to a closed peer would kill the test binary"
        );
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (_socket, _) = listener.accept().expect("accept");
            // Dropping sends a FIN; the reset arrives once the peer writes again.
        });

        let transport = TcpBridgeTransport::connect(addr).expect("connect");
        let mut writer = transport.try_clone_writer().expect("clone writer");
        server.join().expect("server thread");

        // The write that races the reset can still be accepted by the local send
        // buffer, so this asserts that the error arrives, not that the first call
        // returns it.
        let mut error = None;
        for _ in 0..64 {
            if let Err(err) = writer.send_line("{\"type\":\"config\"}") {
                error = Some(err);
                break;
            }
        }

        let err = error.expect("a closed peer must surface an error");
        assert!(
            matches!(&err, TransportError::Io(_)),
            "classified like any other transport failure: {err:?}"
        );
    }

    #[test]
    fn two_writer_sends_arrive_as_two_lines_not_one_frame() {
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().expect("accept");
            let mut reader = std::io::BufReader::new(socket);
            let mut first = String::new();
            let mut second = String::new();
            reader.read_line(&mut first).expect("read first");
            reader.read_line(&mut second).expect("read second");
            (first, second)
        });

        let transport = TcpBridgeTransport::connect(addr).expect("connect");
        let mut writer = transport.try_clone_writer().expect("clone writer");
        writer
            .send_line("{\"type\":\"config\"}")
            .expect("first send");
        writer
            .send_line("{\"type\":\"moduleAction\"}")
            .expect("second send");

        let (first, second) = server.join().expect("server thread");
        assert_eq!(first, "{\"type\":\"config\"}\n");
        assert_eq!(second, "{\"type\":\"moduleAction\"}\n");
    }

    #[test]
    fn a_writer_handle_does_not_inherit_the_readers_buffered_frame() {
        let (listener, addr) = listener();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            // One complete line, then the start of a second one: reading the
            // first leaves the second pending in the reader.
            socket
                .write_all(b"{\"type\":\"state\"}\n{\"ty")
                .expect("write partial");

            let mut received = Vec::new();
            let mut chunk = [0u8; 64];
            while !received.contains(&b'\n') {
                let read = socket.read(&mut chunk).expect("read config");
                assert_ne!(read, 0, "the peer closed mid-test");
                received.extend_from_slice(chunk.get(..read).expect("chunk"));
            }

            // Finish the line the reader is still holding a prefix of.
            socket.write_all(b"pe\":\"state\"}\n").expect("complete");
            received
        });

        let mut transport = TcpBridgeTransport::connect(addr).expect("connect");
        assert_eq!(read_line(&mut transport), "{\"type\":\"state\"}");

        let mut writer = transport.try_clone_writer().expect("clone writer");
        writer
            .send_line("{\"type\":\"config\"}")
            .expect("send config");

        // The peer sees the config frame alone, so no byte the reader is holding
        // was replayed onto the writer's handle.
        assert_eq!(
            server.join().expect("server thread"),
            b"{\"type\":\"config\"}\n".to_vec()
        );
        // And the reader still owns its half-read frame.
        assert_eq!(read_line(&mut transport), "{\"type\":\"state\"}");
    }

    #[test]
    fn the_bridge_endpoint_is_loopback_on_the_contract_port() {
        assert_eq!(
            BRIDGE_ADDRESS.ip(),
            std::net::IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(BRIDGE_ADDRESS.port(), 25590);
        assert_eq!(BRIDGE_ADDRESS.port(), BRIDGE_PORT);
    }

    #[test]
    fn connect_reports_a_closed_port_as_an_io_error() {
        // Bind, learn the port, drop the listener, then dial: nobody is listening.
        let addr = {
            let (listener, addr) = listener();
            drop(listener);
            addr
        };
        let result = TcpBridgeTransport::connect_timeout(addr, Duration::from_millis(200));
        assert!(result.is_err(), "a closed port must not connect");
    }
}
