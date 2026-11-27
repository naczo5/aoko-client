//! Port of `Aoko.Tests/BoundedLineReaderTests.cs`.
//!
//! The managed suite drives `BoundedLineReader` over a `MemoryStream`; here the
//! same cases run over `&[u8]`, plus the two quirks the managed tests never pinned
//! but `WIRE_CONTRACT.md` documents: CRLF exactly at the cap, and the oversized
//! report appearing once at end of stream.

use aoko_protocol::framing::BoundedLineReader;
use aoko_protocol::transport::{LineBuf, RecvOutcome, MAX_INBOUND_LINE_BYTES};

fn read_all(bytes: &[u8], maximum: usize) -> Vec<(RecvOutcome, String)> {
    let mut reader = BoundedLineReader::new(bytes, maximum);
    let mut out = LineBuf::new();
    let mut results = Vec::new();

    loop {
        let outcome = reader
            .read_line(&mut out)
            .expect("in-memory read cannot fail");
        results.push((outcome, out.as_str().to_owned()));
        if outcome == RecvOutcome::EndOfStream {
            return results;
        }
        // Safety valve: a logic error must fail the test, not hang it.
        assert!(results.len() < 64, "reader never reported end of stream");
    }
}

#[test]
fn reads_crlf_and_final_unterminated_line() {
    let results = read_all(b"first\r\nsecond", 32);

    assert_eq!(results[0], (RecvOutcome::Line, "first".to_owned()));
    assert_eq!(results[1], (RecvOutcome::Line, "second".to_owned()));
    assert_eq!(results[2].0, RecvOutcome::EndOfStream);
}

#[test]
fn accepts_line_at_configured_limit() {
    let results = read_all(b"12345\r\n", 5);

    assert_eq!(results[0], (RecvOutcome::Line, "12345".to_owned()));
    assert_ne!(results[0].0, RecvOutcome::TooLong);
}

#[test]
fn discards_oversized_line_and_continues() {
    let results = read_all(b"123456789\nvalid\n", 5);

    assert_eq!(results[0], (RecvOutcome::TooLong, String::new()));
    assert_eq!(results[1], (RecvOutcome::Line, "valid".to_owned()));
}

#[test]
fn reports_oversized_line_at_end_of_stream_once() {
    let results = read_all(b"123456789", 5);

    assert_eq!(results[0].0, RecvOutcome::TooLong);
    assert_eq!(results[1].0, RecvOutcome::EndOfStream);
    assert_eq!(results.len(), 2, "the oversized report must not repeat");
}

#[test]
fn preserves_utf8_text() {
    let results = read_all("zażółć 🐈\n".as_bytes(), 32);

    assert_eq!(results[0], (RecvOutcome::Line, "zażółć 🐈".to_owned()));
}

#[test]
fn keeps_the_carriage_return_at_the_limit_so_a_max_line_still_completes() {
    // Quirk from `WIRE_CONTRACT.md`: at exactly the cap the `\r` is retained, so a
    // maximum-length CRLF line completes and still has its `\r` stripped.
    let payload = vec![b'a'; 5];
    let mut wire = payload.clone();
    wire.extend_from_slice(b"\r\n");

    let results = read_all(&wire, 5);

    assert_eq!(results[0].0, RecvOutcome::Line);
    assert_eq!(results[0].1.len(), 5);
    assert_eq!(results[1].0, RecvOutcome::EndOfStream);
}

#[test]
fn base64_like_frame_at_the_wire_cap_round_trips() {
    let mut line = String::from("{\"type\":\"state\",\"actionBar\":\"");
    line.push_str(&"x".repeat(MAX_INBOUND_LINE_BYTES - line.len() - 2));
    line.push_str("\"}");
    assert_eq!(line.len(), MAX_INBOUND_LINE_BYTES);

    let mut wire = line.as_bytes().to_vec();
    wire.push(b'\n');

    let results = read_all(&wire, MAX_INBOUND_LINE_BYTES);
    assert_eq!(results[0].0, RecvOutcome::Line);
    assert_eq!(results[0].1, line);
}

#[test]
fn one_byte_past_the_wire_cap_is_discarded() {
    let mut wire = vec![b'a'; MAX_INBOUND_LINE_BYTES + 1];
    wire.push(b'\n');
    wire.extend_from_slice(b"ok\n");

    let results = read_all(&wire, MAX_INBOUND_LINE_BYTES);
    assert_eq!(results[0].0, RecvOutcome::TooLong);
    assert_eq!(results[1], (RecvOutcome::Line, "ok".to_owned()));
}

#[test]
fn line_buffer_is_reused_across_calls() {
    let mut reader = BoundedLineReader::new(&b"one\ntwo\n"[..], 32);
    let mut out = LineBuf::with_capacity(32);

    let first = reader.read_line(&mut out).expect("read");
    assert_eq!(first, RecvOutcome::Line);
    assert_eq!(out.as_str(), "one");

    let second = reader.read_line(&mut out).expect("read");
    assert_eq!(second, RecvOutcome::Line);
    assert_eq!(out.as_str(), "two");
}
