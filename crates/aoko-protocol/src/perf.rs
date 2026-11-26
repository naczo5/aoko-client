//! Deterministic allocation accounting for the wire and solver paths.
//!
//! `MIGRATION_ARCHITECTURE.md` section 6 makes four claims about this crate, and
//! every one of them is about *how many allocations happen*, never about speed:
//!
//! - `BoundedLineReader` reuses its buffer, so a steady-state frame allocates only
//!   when the line has to leave that buffer.
//! - Patch merging mutates in place; a packet must not deep-copy the `String`s and
//!   maps it does not carry.
//! - The GTB solver indexes the wordlist once, and a repeated query touches no
//!   allocator at all.
//!
//! Allocation counts are deterministic, so they are asserted here. Timings are
//! noisy, so they live in `benches/protocol.rs` and are recorded, not asserted.
//!
//! # Why a global allocator
//!
//! A `#[global_allocator]` is the only way to see allocations made by code that
//! never names an allocator — `format!`, a `String` grown by `push`, `Vec`
//! internal growth, `serde_json` building a document. It is installed in test
//! builds only: this module is behind `#[cfg(test)]` in `lib.rs`, so the shipped
//! library and every dependent crate link `System` unchanged.
//!
//! The counter is a `Cell` in a `const`-initialised thread-local. That matters:
//! a lazily initialised thread-local would allocate on first touch, from inside
//! the allocator, and an allocator that allocates while counting would not
//! terminate. `const` initialisation registers no lazy slot and `Allocations` has
//! no destructor, so `try_with` is pure arithmetic.
//!
//! # What a number means
//!
//! [`measure`] reports the allocations the **calling thread** made between two
//! points, in both count and requested bytes, so a test reads as one line and is
//! unaffected by the other tests `cargo test` runs on its own threads. `alloc`,
//! `alloc_zeroed` and `realloc` each count once: a reallocation moves memory, so
//! it is an allocation. Frees are not counted — a path that allocates and
//! immediately drops is still a path that allocated.
//!
//! `aoko-core` carries its own copy of this module. A global allocator is
//! per-binary, so the two crates cannot share one.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::fmt;

/// One measurement: how many allocations, and how many bytes they asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Allocations {
    /// Allocation calls (`alloc`, `alloc_zeroed`, `realloc`).
    pub count: usize,
    /// Sum of the sizes those calls requested.
    pub bytes: usize,
}

impl Allocations {
    /// The count a path that must not allocate reports.
    pub const NONE: Self = Self { count: 0, bytes: 0 };

    /// Adds two measurements, for a path measured in phases.
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self {
            count: self.count + other.count,
            bytes: self.bytes + other.bytes,
        }
    }

    fn since(self, earlier: Self) -> Self {
        Self {
            count: self.count.saturating_sub(earlier.count),
            bytes: self.bytes.saturating_sub(earlier.bytes),
        }
    }
}

impl fmt::Display for Allocations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} allocation(s), {} byte(s)", self.count, self.bytes)
    }
}

thread_local! {
    /// Allocations made on this thread since it started.
    static ALLOCATED: Cell<Allocations> = const { Cell::new(Allocations::NONE) };
}

/// Adds one allocation of `bytes` to this thread's total.
fn record(bytes: usize) {
    // `try_with` rather than `with`: during thread teardown the slot can already be
    // gone, and an allocator that panics then would abort the whole test binary.
    let _ = ALLOCATED.try_with(|total| {
        let before = total.get();
        total.set(Allocations {
            count: before.count + 1,
            bytes: before.bytes + bytes,
        });
    });
}

fn allocated() -> Allocations {
    ALLOCATED.try_with(Cell::get).unwrap_or_default()
}

/// Runs `f` and returns its value with the allocations made on this thread while
/// it ran.
///
/// `f` must not start a thread; anything it does on this thread is counted.
pub fn measure<T>(f: impl FnOnce() -> T) -> (T, Allocations) {
    let before = allocated();
    let value = f();
    (value, allocated().since(before))
}

/// Counts every allocation, then hands it to `System`.
struct CountingAllocator;

// SAFETY: every method forwards to `System` with the arguments it was given, so the
// global allocator's contract is `System`'s contract. The added work is a
// thread-local increment that neither allocates nor unwinds.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: `layout` is the caller's, forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: `layout` is the caller's, forwarded unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(new_size);
        // SAFETY: `ptr`, `layout` and `new_size` are the caller's, forwarded
        // unchanged, so the caller's invariants still hold.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` are the caller's, forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Read;

    use serde::Deserialize as _;
    use serde_json::Value;

    use super::*;
    use crate::codec::{Decoder, Inbound};
    use crate::framing::BoundedLineReader;
    use crate::gtb::{extract_mask, GtbWordSolver};
    use crate::state::{EntityInfo, GameState};
    use crate::state_patch::apply_into;
    use crate::transport::{LineBuf, RecvOutcome};

    /// One bridge-shaped frame, newline-terminated so it can repeat forever.
    const FRAME: &[u8] = b"{\"type\":\"state\",\"partial\":true,\"health\":20}\n";

    /// A source that never ends: it re-serves `pattern` from the start, so a reader
    /// over it sees an endless run of identical, perfectly aligned frames.
    struct RepeatingSource {
        pattern: &'static [u8],
        cursor: usize,
    }

    impl RepeatingSource {
        fn new(pattern: &'static [u8]) -> Self {
            Self { pattern, cursor: 0 }
        }
    }

    impl Read for RepeatingSource {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if buffer.is_empty() || self.pattern.is_empty() {
                return Ok(0);
            }
            for slot in buffer.iter_mut() {
                *slot = self.pattern[self.cursor];
                self.cursor = (self.cursor + 1) % self.pattern.len();
            }
            Ok(buffer.len())
        }
    }

    fn read_frame(reader: &mut BoundedLineReader<RepeatingSource>, out: &mut LineBuf) -> String {
        let outcome = reader
            .read_line(out)
            .expect("an in-memory read cannot fail");
        assert_eq!(outcome, RecvOutcome::Line);
        out.as_str().to_owned()
    }

    /// A state with every allocating field populated, so a merge that rebuilt the
    /// state would show up in the count.
    fn populated_state() -> GameState {
        GameState {
            screen_name: "GuiChest".to_owned(),
            action_bar: "The theme is C_T (1)".to_owned(),
            module_status: BTreeMap::from([("killaura".to_owned(), "no target".to_owned())]),
            entities: vec![EntityInfo {
                name: "Zombie".to_owned(),
                dist: 3.5,
                ..EntityInfo::default()
            }],
            is_connected: true,
            ..GameState::default()
        }
    }

    fn patch(json: &str) -> serde_json::Map<String, Value> {
        match serde_json::from_str::<Value>(json).expect("valid patch json") {
            Value::Object(object) => object,
            _ => panic!("a patch is a json object"),
        }
    }

    fn solver() -> GtbWordSolver {
        GtbWordSolver::from_words(["cat", "cot", "cut", "dog"])
    }

    #[test]
    fn draining_a_line_from_a_reused_buffer_allocates_nothing() {
        let mut reader = BoundedLineReader::new(RepeatingSource::new(FRAME), 4096);
        let line = String::from_utf8_lossy(&FRAME[..FRAME.len() - 1]).into_owned();
        let mut out = LineBuf::with_capacity(FRAME.len());

        // Warm-up: the first frame fills the caller's buffer. Every frame after it
        // must reuse both the reader's internals and `out`.
        assert_eq!(read_frame(&mut reader, &mut out), line);

        let (measured, allocations) = measure(|| reader.read_line(&mut out));

        assert_eq!(measured.expect("in-memory read"), RecvOutcome::Line);
        assert_eq!(out.as_str(), line);
        assert_eq!(
            allocations,
            Allocations::NONE,
            "a steady-state frame reuses the reader buffer and the caller buffer"
        );
    }

    #[test]
    fn a_line_that_leaves_the_reader_buffer_allocates_exactly_once() {
        // The only allocation section 6 permits on this path: the line has to sit in
        // the caller's buffer, so the very first frame into a zero-capacity
        // `LineBuf` buys that buffer. It is bought once and reused after that.
        let mut reader = BoundedLineReader::new(RepeatingSource::new(FRAME), 4096);
        let mut out = LineBuf::new();

        let (measured, allocations) = measure(|| reader.read_line(&mut out));

        assert_eq!(measured.expect("in-memory read"), RecvOutcome::Line);
        assert_eq!(out.len(), FRAME.len() - 1);
        assert_eq!(
            allocations.count, 1,
            "the line outliving the reader buffer costs exactly one allocation"
        );
        assert!(
            allocations.bytes >= FRAME.len() - 1,
            "the one allocation must be at least the line: {allocations}"
        );

        let (again, reused) = measure(|| reader.read_line(&mut out));
        assert_eq!(again.expect("in-memory read"), RecvOutcome::Line);
        assert_eq!(reused, Allocations::NONE, "the bought buffer is reused");
    }

    #[test]
    fn merging_a_scalar_patch_into_a_live_state_allocates_nothing() {
        let mut state = populated_state();
        let patch = patch(
            r#"{"partial":true,"health":19.5,"posX":12.0,"posZ":-3.5,"guiOpen":false,"stateMs":7}"#,
        );

        // Warm-up, then the same merge: unchanged strings, entities and module status
        // must keep their allocations, and a scalar needs none of its own.
        apply_into(&mut state, &patch);
        let ((), allocations) = measure(|| apply_into(&mut state, &patch));

        assert_eq!(state.health, 19.5);
        assert_eq!(state.screen_name, "GuiChest");
        assert_eq!(state.entities.len(), 1);
        assert_eq!(
            allocations,
            Allocations::NONE,
            "an in-place merge of scalars must not copy the fields it leaves alone"
        );
    }

    #[test]
    fn merging_a_patch_that_carries_one_string_allocates_only_that_string() {
        let mut state = populated_state();
        let patch = patch(r#"{"partial":true,"screenName":"GuiInventory"}"#);

        let ((), allocations) = measure(|| apply_into(&mut state, &patch));

        assert_eq!(state.screen_name, "GuiInventory");
        assert_eq!(state.action_bar, "The theme is C_T (1)");
        assert_eq!(
            allocations,
            Allocations {
                count: 1,
                bytes: "GuiInventory".len()
            },
            "a replaced string is the only thing the patch pays for"
        );
    }

    #[test]
    fn a_repeated_gtb_mask_is_answered_without_touching_the_allocator() {
        let mut solver = solver();
        let action_bar = "The theme is C_T (1)";

        // Warm-up fills the one-entry answer cache, which is what the live loop sees
        // on every frame after the first.
        let (warm, _) = measure(|| solver.solve(action_bar, 8).1.len());
        assert_eq!(warm, 3);

        let (matched, allocations) = measure(|| solver.solve(action_bar, 8).1.len());

        assert_eq!(matched, 3);
        assert_eq!(
            allocations,
            Allocations::NONE,
            "a repeated mask is answered from the index and the memo, with no allocator"
        );
    }

    #[test]
    fn a_new_action_bar_line_pays_the_mask_pipeline_and_its_matches() {
        // A line the solver has not seen is not steady state: it has to normalise the
        // action-bar text (a chain of temporary strings), remember the line, and clone
        // the words it returns. The bound is that decomposition and nothing else, so a
        // path that also copied the wordlist or the previous answer would fail here.
        let line = "The theme is C_T (1)";

        let (_, pipeline) = measure(|| extract_mask(line));
        assert_eq!(
            pipeline.count, 19,
            "the mask pipeline's temporary strings, as recorded in the baseline doc"
        );

        let mut solver = solver();
        let (matched, allocations) = measure(|| solver.solve(line, 8).1.len());

        assert_eq!(matched, 3);
        assert_eq!(
            allocations.count,
            pipeline.count + 1 + 3 + 1, // line + matches + vector
            "a new line pays the pipeline, the memo and the output, and nothing else"
        );
    }

    #[test]
    fn one_partial_frame_costs_its_parse_plus_exactly_one_state_copy() {
        // What the codec may charge per packet: parsing the document it was handed,
        // and the single copy that lets the decoder keep a state while the frame
        // carries one. Two deep copies would mean the merge rebuilt a snapshot it
        // already had.
        let line = r#"{"type":"state","partial":true,"health":19.5,"posX":12.0,"stateMs":7}"#;
        let mut decoder = Decoder::new();
        // Warm-up puts the decoder in the state the measurement sees: `is_connected`
        // set and `last_update` stamped, so both clones have the same content.
        let _ = decoder.ingest(line);

        let (_, parsed) = measure(|| serde_json::from_str::<Value>(line).expect("valid json"));
        let (_, copied) = measure(|| GameState::clone(decoder.state()));
        let (frame, total) = measure(|| decoder.ingest(line));

        assert!(matches!(frame, Inbound::State { .. }));
        assert_eq!(
            total,
            parsed.plus(copied).plus(Allocations {
                count: 1,
                bytes: std::mem::size_of::<GameState>(),
            }),
            "a partial frame costs its parse, one state copy and the frame box"
        );
    }

    #[test]
    fn one_complete_frame_costs_its_parse_plus_exactly_one_state_copy() {
        // The legacy bridge sends a complete, untyped document every interval, so this
        // path is as hot as the partial one. It must not pay a second copy of the
        // parsed tree on the way in, which is what `from_value(root.clone())` did.
        let line = r#"{"mapped":true,"inWorld":true,"screenName":"GuiChest","health":20,"viewportWidth":1600,"entities":[{"name":"Zombie","dist":3.5}]}"#;
        let mut decoder = Decoder::new();
        let _ = decoder.ingest(line);

        let (root, parsed) = measure(|| serde_json::from_str::<Value>(line).expect("valid json"));
        let (_, copied) = measure(|| GameState::clone(decoder.state()));
        let (frame, ingest) = measure(|| decoder.ingest(line));
        let (_, decoded) = measure(|| GameState::deserialize(&root));

        assert!(matches!(frame, Inbound::State { .. }));
        assert_eq!(
            ingest,
            parsed.plus(decoded).plus(copied).plus(Allocations {
                count: 1,
                bytes: std::mem::size_of::<GameState>(),
            }),
            "a complete frame costs one parse, one decode, one state copy and the box"
        );
    }
}
