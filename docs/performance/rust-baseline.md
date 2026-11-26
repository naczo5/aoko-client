# Rust baseline: what section 6 costs, measured

Phase 3 record for `MIGRATION_ARCHITECTURE.md` section 6. That section makes four
claims about the Rust host that are about **how many allocations happen**, not about
speed:

| Clause | Claim | Measured here |
| --- | --- | --- |
| Framing | `BoundedLineReader` reuses its buffer; a steady-state frame allocates only when a line must outlive the buffer | `aoko-protocol/src/perf.rs` |
| Patch merging | Mutate in place, publish `Arc` snapshots; do not deep-clone unchanged strings or maps per packet | `aoko-protocol/src/perf.rs` |
| Hot loops | Allocation-free per tick, absolute `Instant` deadlines | `aoko-core/src/perf.rs` |
| GTB solver | Index the wordlist once; a live query touches no allocator beyond the output | `aoko-protocol/src/perf.rs` |

Allocation counts are deterministic, so they are **asserted in tests**. Timings are
noisy, so they are **recorded, not asserted**, in `benches/` and in the table below.

Measured on 2026-11-26 against the tree this document lands with, whose parent
commit is `be80ada`, with `rustc 1.100.0 (0ef1bc386 2026-11-09)`.

## The machine

| | |
| --- | --- |
| CPU | Intel(R) Core(TM) m3-8100Y CPU @ 1.10GHz |
| Cores | 2 physical, 4 threads |
| OS | Debian GNU/Linux 13 (trixie) |
| Profile | `bench` (`lto = "thin"`, `codegen-units = 1`, `opt-level` 3) |

This is a low-power laptop CPU running at roughly half its nominal clock. Read the
timing columns as one machine's baseline. They are not a budget and not a comparison
with the managed host, which was not measured under the same conditions.

## How to reproduce

```bash
cargo test -p aoko-protocol --lib perf
cargo test -p aoko-core --lib perf
cargo bench -p aoko-protocol
cargo bench -p aoko-core
```

The benches print one machine-readable line per case:

```text
bench case=framing.reused_buffer iterations=200000 total_ns=108827712 ns_per_op=544
```

## Allocation counts

One counter unit is one call into the allocator (`alloc`, `alloc_zeroed` or
`realloc`). Bytes are the sizes those calls requested. Counts are per calling thread,
so a test cannot be disturbed by the other tests `cargo test` runs in parallel.

| Path | Count | Bytes | Bound asserted | Test |
| --- | ---: | ---: | --- | --- |
| `BoundedLineReader`, line drained into a reused `LineBuf` | 0 | 0 | **exactly zero** | `draining_a_line_from_a_reused_buffer_allocates_nothing` |
| `BoundedLineReader`, first line into an empty `LineBuf` | 1 | 43 | **exactly one**, then zero | `a_line_that_leaves_the_reader_buffer_allocates_exactly_once` |
| `apply_into` with six scalar fields | 0 | 0 | **exactly zero** | `merging_a_scalar_patch_into_a_live_state_allocates_nothing` |
| `apply_into` with one string field | 1 | 12 | **exactly the string**, nothing else | `merging_a_patch_that_carries_one_string_allocates_only_that_string` |
| One partial frame through `Decoder::ingest` | 9 | 1008 | parse + **one** state copy + the frame box | `one_partial_frame_costs_its_parse_plus_exactly_one_state_copy` |
| One complete frame through `Decoder::ingest` | 20 | 1972 | parse + decode + **one** state copy + the frame box | `one_complete_frame_costs_its_parse_plus_exactly_one_state_copy` |
| GTB, repeated action-bar line | 0 | 0 | **exactly zero** | `a_repeated_gtb_mask_is_answered_without_touching_the_allocator` |
| GTB, new action-bar line | 24 | 521 | pipeline (19) + memo line + matches + vector | `a_new_action_bar_line_pays_the_mask_pipeline_and_its_matches` |
| One click-loop tick that clicks | 0 | 0 | **exactly zero** | `one_click_tick_allocates_nothing` |
| One click-loop tick that only waits | 0 | 0 | **exactly zero** | `one_waiting_tick_allocates_nothing` |

Three of those numbers need the decomposition to be readable:

- **The partial frame's 9** is 7 for parsing the document (665 B), 1 for the state
  copy the frame carries (7 B) and 1 for the `Box<GameState>` that copy sits in
  (336 B, which is `size_of::<GameState>()`). The test asserts exactly that sum, so a
  second deep copy appears as a failure rather than as a slightly worse number.
- **The complete frame's 20** is 13 for the JSON parse (1464 B), 3 for the decode out of
  the parsed tree (86 B), 3 for the state copy the frame carries (86 B) and 1 for the
  `Box<GameState>` (336 B).
- **The GTB new line's 24** is 19 for the mask pipeline's temporary strings, 1 to
  remember the line, 3 for the matching words and 1 for the vector holding them. A
  repeated line never reaches any of it.

For scale: cloning a populated `GameState` (two strings, a module-status map with one
entry, one named entity) costs 7 allocations and 667 bytes. Before this phase,
`ingest_state` paid that clone twice per partial packet plus the frame copy; the
in-place merge pays it once, and `apply_into` itself pays nothing for a field the
patch does not carry. Both codec rows above are dominated by `serde_json`, not by the
port: the parse is what allocates, and the copy that remains is the frame shape, not
the merge.

## Bench figures

Five passes of each bench, in **ns per operation**. The passes ran on the machine
described above while other work was building on it — load average 3.5 to 5.6 on four
threads — so each row reports the **fastest** pass, which is the least contaminated
estimate, and the **median** with the spread beside it. A faster machine would move
every figure; nothing here is a target.

| Case | Passes | Fastest | Median | Worst |
| --- | --- | ---: | ---: | ---: |
| `framing.reused_buffer` | 509, 560, 553, 615, 547 | **509** | 553 | 615 |
| `patch.scalar_merge` | 195, 194, 396, 381, 196 | **194** | 196 | 396 |
| `gtb.repeated_line` | 9, 9, 11, 11, 8 | **8** | 9 | 11 |
| `gtb.new_line` | 1247, 1174, 1459, 1397, 1142 | **1142** | 1247 | 1459 |
| `codec.partial_frame` | 1465, 1399, 1496, 2051, 1423 | **1399** | 1465 | 2051 |
| `click.tick` | 477, 441, 466, 448, 437 | **437** | 448 | 477 |
| `click.deadline` | 27, 21, 33, 21, 22 | **21** | 22 | 33 |

Each case warms up once outside the timed region and then runs a fixed iteration
count, so the iteration number in the output is the sample size for that figure.
`click.tick` runs 32 ticks per measured call and divides, so the loop's own start-up
(`LoopRuntime`, one RNG box) is amortised to well under a percent of the figure instead
of dominating a single-tick one. `click.deadline` is
`ClickDeadlineScheduler::schedule_next` walked forward one interval per call, which is
the absolute-deadline rule section 6 asks for.

Two figures deserve a second look, because they are larger than the rest:
`framing.reused_buffer` is a byte-at-a-time copy loop over a 54-byte line, and
`codec.partial_frame` includes a full `serde_json` parse of the frame. Neither is a
hot-loop claim: the codec path runs once per bridge frame, and the allocation count,
not the duration, is what section 6 binds. The `patch.scalar_merge` spread across the
five passes (194 to 396 ns) is the clearest warning in this table about reading a single
pass as a result.

## What the measurements caught

Two paths violated the contract and were fixed, both without changing behaviour:

1. **The GTB solver allocated on every steady-state query.** The memo was keyed on the
   extracted mask, but extracting the mask is what allocates, so a repeated action-bar
   line cost 19 allocations before the memo could answer. The memo is now keyed on the
   line as well, which is the input the bridge actually repeats:
   `a_repeated_gtb_mask_is_answered_without_touching_the_allocator` measures zero.
   `try_learn_solved_word` clears both halves, because a learned word invalidates the
   answer as well as the mask.

2. **`click::run` allocated once per loop start, inside the loop body.** The
   not-connected `GameState::default()` was rebuilt on every call, carrying one 7-byte
   string clone. It is now built once per process via `OnceLock`. The negative case is
   on record: with the old line restored, both tick tests fail with
   `Allocations { count: 1, bytes: 7 }`.

Three paths were restructured rather than loosened:

3. **Patch merging deep-cloned the snapshot per packet.** `state_patch::apply_into`
   merges into the live state and parses each carried field straight out of the borrowed
   JSON, so a scalar field costs nothing and a string field costs only the string.
   `apply` keeps its signature and is now one clone plus `apply_into`, which is what the
   existing `GameStatePatchMergerTests` port already asserts.

4. **The codec merged into a copy and then copied again.** `ingest_state` now merges
   into the decoder's own state and hands out the single copy the frame shape requires.

5. **A complete frame deep-copied the parsed document before decoding it.**
   `from_value::<GameState>(root.clone())` cloned the whole parsed tree and then decoded
   it, on the path the legacy bridge uses for every frame. It decodes from the borrowed
   document now, which is what
   `one_complete_frame_costs_its_parse_plus_exactly_one_state_copy` pins: one parse, one
   decode, one copy, one box.

## What the numbers do not prove

- **They do not prove speed, and no comparison with the C# host is implied.** Section 6
  governs allocation behaviour; a slower allocation-free path would still satisfy it.
  The managed host was not measured on this machine.
- **They do not prove a wire-format or behaviour change was safe.** The parity evidence
  is `MIGRATION_PARITY.md`, `docs/rust/WIRE_CONTRACT.md` and the test suite. Four
  behavioural paths changed shape here — the GTB memo, the codec's merge, the
  complete-frame decode and the loop's default state — and every one of them is
  covered by tests that predate this phase.
- **They do not prove the numbers on another machine.** Allocation counts will match on
  any machine, since they are properties of the code and the allocator API. Timings will
  not.
- **They are not a budget for the whole app.** Nothing in `aoko-app` was measured here;
  the UI clause of section 6 belongs to whichever builder owns that crate.
- **The bench is not statistically sound.** Three passes of a fixed-iteration loop on a
  shared machine bound the order of magnitude and nothing finer; do not use the last
  digit of any figure as a regression gate.

## What section 6 asks for that the port does not have yet

Named here rather than quietly omitted, because a measured gap is auditable and an
unmentioned one is not.

1. **`Inbound::State` carries `Box<GameState>`, not an `Arc`.** The contract says
   "publish `Arc` snapshots", and `aoko-core::app::StateStore` does publish `Arc`, but the
   codec has to hand the caller a value it also keeps, so every state frame — complete or
   partial — still costs exactly one deep copy: 1 allocation for the partial frame
   measured above, 3 for the complete one, and 7 (667 B) for a fully populated state.
   Removing it means changing
   `Inbound::State { state: Box<GameState> }` to share the decoder's snapshot, which is a
   public shape change and outside a phase that measures. It is recorded as the next
   Phase 4 candidate, not as delivered work.
2. **A new GTB line still allocates through the mask pipeline** (19 allocations,
   measured). The clause's strict reading — a live query touches no allocator beyond the
   output — holds for the steady state, which is what the per-frame loop actually sees,
   and does not hold for a first-time line. Fixing it means threading reusable scratch
   buffers through `extract_mask`'s normalisation chain: a redesign of code that has
   parity tests, so it is measured and deferred rather than attempted here.
3. **No bench framework.** `crates/aoko-protocol/benches/protocol.rs` and
   `crates/aoko-core/benches/loops.rs` are hand-rolled `harness = false` binaries that
   print one line per case. There is no regression gate on the numbers, by design:
   nothing here is stable enough on this machine to fail a build.
