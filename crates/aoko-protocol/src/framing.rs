//! Bounded, newline-framed line assembly.
//!
//! Direct port of `Aoko/Core/BoundedLineReader.cs`. Three rules are subtle and
//! are reproduced byte for byte:
//!
//! 1. **CRLF at the limit.** When the buffer already holds exactly
//!    [`BoundedLineReader::maximum_line_bytes`] bytes and the next byte is `\r`,
//!    that `\r` is retained. A maximum-length line terminated by CRLF therefore
//!    still completes, and its `\r` is still stripped.
//! 2. **Oversized at EOF.** An oversized line that never sees a newline is
//!    reported as [`RecvOutcome::TooLong`] at end of stream, then the *next*
//!    call reports [`RecvOutcome::EndOfStream`]. It is never reported twice.
//! 3. **Discard alignment.** An oversized line is consumed up to and including
//!    its newline so the following frame still parses.
//!
//! The managed cap counts `char`s while this one counts bytes; the wire is
//! ASCII-dominant so the two coincide (`WIRE_CONTRACT.md`, inbound cap note 5).

use std::io::Read;

use crate::transport::{LineBuf, RecvOutcome, MAX_INBOUND_LINE_BYTES, READ_BUFFER_BYTES};

/// Reads newline-delimited bytes without letting one malformed frame grow an
/// unbounded buffer.
pub struct BoundedLineReader<R> {
    source: R,
    read_buffer: Vec<u8>,
    read_position: usize,
    read_length: usize,
    line: LineBuf,
    maximum_line_bytes: usize,
    discarding_oversized_line: bool,
}

impl<R: Read> BoundedLineReader<R> {
    /// `maximum_line_bytes` is the inbound cap for a single frame.
    #[must_use]
    pub fn new(source: R, maximum_line_bytes: usize) -> Self {
        Self {
            source,
            read_buffer: vec![0; READ_BUFFER_BYTES],
            read_position: 0,
            read_length: 0,
            line: LineBuf::with_capacity(maximum_line_bytes.min(READ_BUFFER_BYTES)),
            maximum_line_bytes,
            discarding_oversized_line: false,
        }
    }

    /// Reader using the wire's 1 MiB inbound cap.
    #[must_use]
    pub fn with_wire_cap(source: R) -> Self {
        Self::new(source, MAX_INBOUND_LINE_BYTES)
    }

    #[must_use]
    pub fn maximum_line_bytes(&self) -> usize {
        self.maximum_line_bytes
    }

    /// Reads the next frame into `out`, clearing it first.
    ///
    /// On [`RecvOutcome::Line`] `out` holds the line with a single trailing `\r`
    /// already removed. On the other outcomes `out` is empty.
    pub fn read_line(&mut self, out: &mut LineBuf) -> std::io::Result<RecvOutcome> {
        loop {
            if self.read_position >= self.read_length {
                self.read_length = self.source.read(&mut self.read_buffer)?;
                self.read_position = 0;

                if self.read_length == 0 {
                    if self.discarding_oversized_line {
                        self.reset_line();
                        out.clear();
                        return Ok(RecvOutcome::TooLong);
                    }

                    if self.line.is_empty() {
                        out.clear();
                        return Ok(RecvOutcome::EndOfStream);
                    }

                    self.take_completed_line(out);
                    return Ok(RecvOutcome::Line);
                }
            }

            let current = self.read_buffer[self.read_position];
            self.read_position += 1;

            if self.discarding_oversized_line {
                if current == b'\n' {
                    self.reset_line();
                    out.clear();
                    return Ok(RecvOutcome::TooLong);
                }
                continue;
            }

            if current == b'\n' {
                self.take_completed_line(out);
                return Ok(RecvOutcome::Line);
            }

            if self.line.len() >= self.maximum_line_bytes {
                if current == b'\r' && self.line.len() == self.maximum_line_bytes {
                    self.line.push(current);
                    continue;
                }

                self.discarding_oversized_line = true;
                continue;
            }

            self.line.push(current);
        }
    }

    fn take_completed_line(&mut self, out: &mut LineBuf) {
        let bytes = self.line.as_slice();
        let mut length = bytes.len();
        if length > 0 && bytes[length - 1] == b'\r' {
            length -= 1;
        }

        out.clear();
        out.extend_from_slice(&bytes[..length]);
        self.reset_line();
    }

    fn reset_line(&mut self) {
        self.line.clear();
        self.discarding_oversized_line = false;
    }
}

impl<R: Read> std::fmt::Debug for BoundedLineReader<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundedLineReader")
            .field("maximum_line_bytes", &self.maximum_line_bytes)
            .field("buffered", &self.line.len())
            .field("discarding", &self.discarding_oversized_line)
            .finish_non_exhaustive()
    }
}
