//! Timing pass over the wire and solver paths `MIGRATION_ARCHITECTURE.md`
//! section 6 names.
//!
//! There is no bench framework here on purpose: five cases do not justify a
//! dependency, and the workspace carries none. Each case is a fixed iteration
//! count around a closure whose state is built once, so the number is the cost of
//! one steady-state call and not the cost of constructing a harness.
//!
//! One line per case, machine-readable and stable in key order:
//!
//! ```text
//! bench case=framing.reused_buffer iterations=200000 total_ns=… ns_per_op=…
//! ```
//!
//! **These numbers prove nothing about correctness.** The allocation claims of
//! section 6 are asserted in `src/perf.rs`; this is the companion figure, and
//! `docs/performance/rust-baseline.md` records both, with the machine they came
//! from and what they do not support.

use std::hint::black_box;
use std::io::Read;
use std::time::Instant;

use serde_json::Value;

use aoko_protocol::codec::Decoder;
use aoko_protocol::framing::BoundedLineReader;
use aoko_protocol::gtb::GtbWordSolver;
use aoko_protocol::state::{EntityInfo, GameState};
use aoko_protocol::state_patch::apply_into;
use aoko_protocol::transport::LineBuf;

/// One bridge-shaped frame, newline-terminated.
const FRAME: &[u8] = b"{\"type\":\"state\",\"partial\":true,\"health\":20,\"posX\":1.5}\n";

/// A partial frame, as `bridge_261.cpp` writes it on fast frames.
const PARTIAL_FRAME: &str =
    r#"{"type":"state","partial":true,"health":19.5,"posX":12.0,"posZ":-3.5,"stateMs":7}"#;

/// A GTB action-bar line, as the bridges relay it.
const ACTION_BAR: &str = "The theme is C_T (1)";

/// A source that re-serves `pattern` forever, so the reader sees endless identical,
/// aligned frames.
struct RepeatingSource {
    pattern: &'static [u8],
    cursor: usize,
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

/// Times `body` once untimed (to warm whatever it builds lazily) and then
/// `iterations` times.
fn time(name: &str, iterations: u64, mut body: impl FnMut()) {
    body();
    let started = Instant::now();
    for _ in 0..iterations {
        body();
    }
    let total_nanos = started.elapsed().as_nanos();
    let per_op = total_nanos / u128::from(iterations.max(1));
    println!("bench case={name} iterations={iterations} total_ns={total_nanos} ns_per_op={per_op}");
}

fn populated_state() -> GameState {
    GameState {
        screen_name: "GuiChest".to_owned(),
        action_bar: ACTION_BAR.to_owned(),
        module_status: [
            ("killaura".to_owned(), "no target".to_owned()),
            ("triggerbot".to_owned(), "ready".to_owned()),
        ]
        .into_iter()
        .collect(),
        entities: (0..8)
            .map(|index| EntityInfo {
                name: format!("Mob{index}"),
                dist: f64::from(index),
                ..EntityInfo::default()
            })
            .collect(),
        is_connected: true,
        ..GameState::default()
    }
}

fn patch() -> serde_json::Map<String, Value> {
    match serde_json::from_str::<Value>(
        r#"{"partial":true,"health":19.5,"posX":12.0,"posZ":-3.5,"stateMs":7,"guiOpen":false}"#,
    ) {
        Ok(Value::Object(object)) => object,
        _ => serde_json::Map::new(),
    }
}

fn main() {
    // 1. Framing: drain one complete line from a reader whose buffer and caller
    //    buffer are both already sized.
    {
        let mut reader = BoundedLineReader::new(
            RepeatingSource {
                pattern: FRAME,
                cursor: 0,
            },
            4096,
        );
        let mut out = LineBuf::with_capacity(FRAME.len());
        time("framing.reused_buffer", 200_000, || {
            let _ = black_box(reader.read_line(&mut out));
            black_box(out.len());
        });
    }

    // 2. Patch merging: six scalars into a live snapshot that owns strings, a map
    //    and a vector none of the scalars touch.
    {
        let mut state = populated_state();
        let patch = patch();
        time("patch.scalar_merge", 200_000, || {
            apply_into(black_box(&mut state), black_box(&patch));
        });
        black_box(state.health);
    }

    // 3. GTB steady state: the same action-bar line the bridge keeps re-sending.
    {
        let mut solver = GtbWordSolver::from_words(["cat", "cot", "cut", "dog", "duck"]);
        time("gtb.repeated_line", 2_000_000, || {
            black_box(solver.solve(ACTION_BAR, 8).1.len());
        });
    }

    // 4. GTB new line: mask pipeline, wordlist scan and the returned matches. The
    //    lines are built up front so the harness itself never allocates inside the
    //    timed region, and each one carries a different mask so the memo misses.
    {
        let mut solver = GtbWordSolver::from_words(["cat", "cot", "cut", "dog", "duck"]);
        let lines: Vec<String> = (0..26)
            .map(|index| format!("The theme is {}_T (1)", (b'A' + index) as char))
            .collect();
        let mut next = 0usize;
        time("gtb.new_line", 100_000, || {
            let line = &lines[next % lines.len()];
            next += 1;
            black_box(solver.solve(line, 8).1.len());
        });
    }

    // 5. Codec: one partial frame end to end, parse included.
    {
        let mut decoder = Decoder::new();
        time("codec.partial_frame", 100_000, || {
            black_box(decoder.ingest(black_box(PARTIAL_FRAME)));
        });
    }
}
