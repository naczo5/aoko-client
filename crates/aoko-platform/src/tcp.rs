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

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
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
        self.send_buffer.clear();
        self.send_buffer.extend_from_slice(line);
        if self.send_buffer.last() != Some(&b'\n') {
            self.send_buffer.push(b'\n');
        }
        // One write for one frame, so a torn line can never sit on the socket.
        self.stream.write_all(&self.send_buffer)?;
        self.stream.flush()?;
        Ok(())
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
