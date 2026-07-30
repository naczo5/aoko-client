# Dev Performance and Reliability Improvement Plan

## Purpose

Improve Aoko's runtime performance, input responsiveness, client compatibility,
and maintainability from the proven `dev` baseline without adopting the
cross-cutting runtime registries from the archived architecture redesign.

The archived implementation remains available for reference:

- Branch: `archive/architecture-redesign`
- Snapshot commit: `62a2ee7`
- Tag: `architecture-redesign-archive-2026-07-30`

This plan favors measured, reversible changes. Each phase must preserve existing
module behavior on all supported bridge/runtime combinations before the next
phase starts.

## Implementation progress

### 2026-07-30 optimization slice

Completed on `dev` after the plan was created:

- Added opt-in managed transport counters behind
  `AOKO_PERF_DIAGNOSTICS=1`, covering inbound message rate/characters, parsing
  time, configuration serialization rate/time, and configuration sends.
- Added opt-in, allocation-bounded modern native counters behind the same
  switch, covering scan-loop work, player/chest/block scans, state publication
  count/time/bytes, and overlay render time. Summaries are emitted every five
  seconds rather than logging individual events.
- Extended those opt-in counters to the legacy bridge, covering its JNI state
  scan, player and chest render-thread scans, Block ESP production, state JSON
  publication, and OpenGL overlay render time. Native summaries now identify
  their bridge family explicitly.
- Replaced unconditional five-times-per-second configuration serialization
  with change-triggered sends, a 25 ms burst coalescing window, and a two-second
  recovery heartbeat.
- Added direct configuration wakeups for `Clicker` state, keybind state, and
  mapping reload changes. Both `PropertyChanged` and `StateChanged` are tracked
  so appearance properties and nested KillAura settings retain immediate sync.
- Changed modern full-state publication from a fixed 5 ms interval to:
  - 5 ms while clicking, Aim Assist, Triggerbot, Pixel Party managed input, or
    HUD editing needs low-latency state.
  - 25 ms otherwise.
- Moved managed Aim Assist body-point projection into the player snapshot
  producer, which already runs at the combat freshness budget. State
  publication consumes the prepared point and retains the previous projection
  path only as a fail-open fallback.
- Pre-escape published player names and reuse state JSON, escaped-string, and
  player snapshot buffers in the modern socket thread instead of reconstructing
  their capacities at 40–200 Hz.
- Decoupled Chest ESP and Block ESP production from the combat scan loop:
  - Chest ESP: 100 ms while enabled; Chest Stealer retains its active loop rate.
  - Block ESP: 150 ms while enabled.
- Changed Chest ESP rendering to copy a short producer snapshot and release its
  mutex before projection, sorting, and ImGui drawing.
- Replaced insertion-based Chest ESP candidate ordering with one sort and
  bounded resize.
- Snapshot configuration and HUD layout together once per render entry and
  resolve panel layouts from that snapshot without per-panel configuration
  locks.
- Reuse render-thread player, chest, block, target, and chest-candidate vector
  capacity through thread-local snapshots.
- Reuse Pixel Party keyboard input storage and cached native structure sizes
  instead of allocating an array and recomputing sizes for each input event.
- Replaced relative AutoClicker delay compensation with a tested absolute
  deadline scheduler. Minor scheduler overshoot is corrected on the following
  interval, while a missed interval resets from the current time to prevent
  catch-up click bursts.
- Added managed tests for diagnostics and config-send policy, plus a C++11
  native harness for adaptive state, ESP scheduling, and concurrent native
  performance-counter aggregation.
- Made profile persistence fail open on expected directory, access, and I/O
  failures so application shutdown and tests are not aborted by an unavailable
  `%APPDATA%` profile path.
- Replaced unbounded newline accumulation on both sides of the V1 TCP transport:
  - The managed reader caps an inbound bridge message at 1 MiB, discards only
    the oversized line, and resumes with the next message.
  - Both native bridges apply the same cap to loader configuration messages and
    release partial oversized data while waiting for the terminating newline.
- Hardened the shared native configuration reader so partial numeric tokens,
  integer overflow, `NaN`, infinity, and invalid Boolean tokens fall back to
  validated defaults instead of entering module state.
- Completed an initial live smoke session on Lunar 26.2 with Vulkan. Normal
  gameplay, Aim Assist, Triggerbot, and ESP behaved correctly; the log contained
  no crashes, fatal errors, JNI exceptions, mapping failures, or socket errors.

Verification:

- All 190 managed tests pass, including `MainWindowStartupTests` in a restricted
  profile environment.
- All native harness tests pass, including bounded-reader recovery and strict
  configuration parsing cases.
- Both `bridge.dll` and `bridge_261.dll` compile successfully with the documented
  MinGW toolchain.

## Guiding principles

1. Measure before optimizing.
2. Keep render and input hot paths direct and allocation-light.
3. Run expensive work only at the frequency required by its consumer.
4. Keep JNI work on the attached worker that owns its `JNIEnv*`.
5. Publish prepared snapshots to render and managed consumers.
6. Put lifecycle cleanup on transition paths, not every tick or frame.
7. Prefer small commits with an isolated benchmark and rollback point.
8. Retain the V1 wire format until a demonstrated compatibility requirement
   justifies a protocol migration.
9. Do not remove functionality to obtain benchmark wins.
10. Do not merge a performance change based only on unit tests.

## Explicit non-goals

- No general native module registry in tick or render paths.
- No managed runtime that asynchronously mediates every module toggle.
- No wholesale protocol rewrite.
- No binary protocol or shared-memory transport in the first optimization
  cycle.
- No JNI discovery or reflection in render hooks.
- No packet spam or gameplay mutation outside a module that explicitly owns it.
- No cleanup of unrelated UI or feature code during performance work.

## Supported validation matrix

Every behavior-affecting phase must be checked against:

| Runtime | Bridge | Renderer | Required checks |
|---|---|---|---|
| Minecraft 1.8.9 | `bridge.dll` | OpenGL | Injection in menu and world, reconnect, input modules, overlays |
| Minecraft 1.21.x | `bridge_261.dll` | OpenGL | Yarn mappings, remap, combat/input state, overlays |
| Lunar 26.1 | `bridge_261.dll` | OpenGL | Mojmap fallback, reconnect, state mutation cleanup |
| Lunar 26.2 | `bridge_261.dll` | Vulkan | Backend arbitration, resize/reset, shutdown, overlay stability |
| Lunar 26.2 | `bridge_261.dll` | OpenGL fallback | Backend arbitration and feature equivalence |

## Success criteria

Final thresholds should be recorded from Phase 0 baselines. Until then, use
these minimum gates:

- No statistically meaningful FPS or frame-time regression against `dev`.
- At least 30% lower bridge-to-loader state bandwidth during normal play.
- At least 30% lower managed JSON parsing time during normal play.
- Unchanged settings produce no repeated full configuration serialization,
  except for a low-frequency recovery heartbeat.
- Overlay rendering performs no JNI lookup and holds no producer mutex while
  issuing ImGui draw calls.
- Combat/input freshness remains inside the existing stale-state limits.
- AutoClicker interval p95 and p99 jitter are no worse than the baseline.
- Disable, disconnect, world exit, remap, panic, and shutdown restore owned
  state deterministically.
- All automated build and test gates pass.
- The supported-runtime smoke matrix has a recorded result.

## Phase 0: Establish reproducible measurements

### P0.1 Add opt-in performance counters

- [ ] Add a compile-time or environment-gated native diagnostics collector.
- [ ] Measure durations using `QueryPerformanceCounter`.
- [ ] Keep the disabled path to one predictable branch.
- [ ] Record count, total, maximum, and bounded histogram buckets rather than
      logging every event.
- [ ] Add counters for:
  - [ ] Modern combat/aim scan duration.
  - [x] Player/nametag scan duration.
  - [x] Chest scan duration.
  - [x] Block ESP scan duration.
  - [x] Legacy state scan duration.
  - [x] OpenGL overlay render duration.
  - [x] Vulkan overlay render duration.
  - [x] State JSON construction duration and payload bytes.
  - [ ] State socket send duration and failures.
  - [ ] Relevant mutex acquisition wait time.
- [ ] Flush summaries at a bounded interval, such as every 5 seconds.
- [ ] Redact player names, aliases, server data, and configuration secrets.

Acceptance criteria:

- Diagnostics disabled: no log output and no measurable frame-time regression.
- Diagnostics enabled: one bounded summary per interval.
- Native harness tests cover histogram boundaries and reset behavior.

### P0.2 Add managed pipeline measurements

- [ ] Measure inbound messages per second and bytes per second.
- [ ] Measure JSON parse/deserialization duration.
- [ ] Measure `StateUpdated` invocations per second.
- [ ] Measure UI dispatcher updates per second.
- [ ] Measure configuration serialization and sends per second.
- [ ] Measure AutoClicker requested versus actual click intervals.
- [ ] Measure Aim Assist state age at input application time.
- [ ] Expose summaries only through debug logging or a dev-only diagnostics
      panel.

Acceptance criteria:

- Release behavior remains unchanged when diagnostics are disabled.
- Metrics collection is allocation-bounded.
- Tests verify counter rollover and disabled behavior.

### P0.3 Capture the baseline

- [ ] Define one repeatable scene per supported runtime:
  - [ ] Menu/idle.
  - [ ] Normal world with no modules.
  - [ ] Aim Assist enabled.
  - [ ] KillAura enabled in dev mode.
  - [ ] Nametags plus Chest ESP plus Block ESP.
  - [ ] HUD editor open and actively dragging.
- [ ] Record at least 60 seconds per scene.
- [ ] Capture FPS, p50/p95/p99 frame time, process CPU, working set, TCP
      bandwidth, message rate, parse time, scan time, and input jitter.
- [ ] Record hardware, resolution, renderer, client version, and Aoko commit.
- [ ] Store summarized results under `docs/performance/`.

Exit criteria:

- A reviewer can repeat the scenario and obtain comparable results.
- Optimization targets are updated with numeric baseline-relative thresholds.

## Phase 1: Low-risk transport and managed allocation reductions

### P1.1 Send configuration on change

- [ ] Add a monotonic managed configuration revision.
- [ ] Increment it when bridge-relevant state changes.
- [ ] Coalesce multiple changes made in the same UI operation.
- [ ] Serialize and send immediately when the revision changes.
- [ ] Cache the serialized payload for the current revision.
- [ ] Retain a one-to-two-second heartbeat for reconnect/recovery.
- [ ] Force a full send after connection, injection, version selection, mapping
      reload, and HUD editor completion.
- [ ] Ensure inbound HUD layout application cannot create an echo loop.

Acceptance criteria:

- Idle configuration serialization drops from five times per second to the
  heartbeat rate or lower.
- A setting change reaches the bridge no slower than the current 200 ms loop.
- Profiles, keybinds, and HUD editor changes still synchronize.
- Reconnect always sends a complete configuration.

### P1.2 Reuse managed input buffers

- [ ] Remove per-event `INPUT_KEY[]` allocation from Pixel Party key handling.
- [ ] Verify all reused buffers are protected by their existing input lock or
      are thread-confined.
- [ ] Audit other high-frequency `SendInput` callers for transient arrays,
      delegates, and closures.
- [ ] Cache structure sizes used by `Marshal.SizeOf`.

Acceptance criteria:

- No new shared-buffer race.
- Managed allocation profiles show no input-event array allocation.
- Pixel Party cleanup releases every owned key.

### P1.3 Coalesce UI-only state notifications

- [ ] Keep the latest game state available immediately to input loops.
- [ ] Rate-limit property change and visual UI updates independently from state
      ingestion.
- [ ] Coalesce action-bar/UI dispatch to one pending dispatcher operation.
- [ ] Do not rate-limit state used by Triggerbot, Aim Assist, mining intent, or
      Auto Rod.

Acceptance criteria:

- WPF updates remain smooth at 30–60 Hz.
- The dispatcher queue does not grow during a 200 Hz inbound stream.
- Input modules always read the newest accepted state.

## Phase 2: Split full telemetry from latency-sensitive state

The modern bridge currently constructs and sends a large state document on a
5 ms loop. That combines latency-sensitive flags with expensive entity
projection and UI telemetry.

### P2.1 Inventory state consumers

- [ ] Map every `GameState` field to its consumers.
- [ ] Classify each field:
  - [ ] Fast input-critical.
  - [ ] Combat target data.
  - [ ] General UI.
  - [ ] Visual overlay only.
  - [ ] Debug-only.
- [ ] Record maximum acceptable age for every field.
- [ ] Add characterization tests for missing and stale optional fields.

Deliverable:

- `docs/performance/STATE_FIELD_BUDGETS.md`.

### P2.2 Introduce two scheduling classes without breaking V1

- [ ] Keep the existing newline-delimited JSON connection.
- [ ] Send a compact state shape at high frequency only when a managed consumer
      needs it.
- [ ] Send the complete state shape at a lower fixed rate.
- [ ] Preserve current field names and optional-field behavior.
- [ ] Teach the loader to merge partial state updates by field presence.
- [ ] Include a monotonic source timestamp so consumers can reject stale data.
- [ ] Bound all payload sizes and entity counts.

Suggested initial rates for measurement:

- Fast input state: 100 Hz while needed, otherwise disabled.
- Full state: 20 Hz normally, up to 50 Hz while HUD editing.

These are starting values, not final requirements.

Acceptance criteria:

- Aim Assist and Triggerbot freshness remain inside their current safety
  windows.
- Normal-play bandwidth and parse time improve by at least 30%.
- Full-state consumers remain behaviorally equivalent.
- Older loader/bridge pairings retain a documented fallback.

### P2.3 Avoid repeated projection in the socket loop

- [ ] Move world-to-screen/entity body-point projection to the producer that
      already owns the camera and entity snapshot.
- [ ] Project once per producer snapshot, not once per socket-send iteration.
- [ ] Publish prepared screen coordinates with the snapshot.
- [ ] Recompute when the camera changes beyond a measured threshold or at the
      required combat rate.
- [ ] Reuse and reserve state string buffers.

Acceptance criteria:

- Socket loop performs no entity body-point search.
- Projection results remain visually and functionally equivalent.
- Projection cost is visible as a separate diagnostic counter.

## Phase 3: Give workloads independent update budgets

### P3.1 Separate modern worker deadlines

- [ ] Replace the single scanner sleep decision with independent monotonic
      deadlines.
- [ ] Keep workers bounded; do not create a permanent thread per module.
- [ ] Schedule:
  - [ ] Auto Rod transaction polling.
  - [ ] Combat target/aim data.
  - [ ] General player list.
  - [ ] Fight Status.
  - [ ] Nametags suppression maintenance.
  - [ ] Chest ESP and Chest Stealer.
  - [ ] Block ESP.
  - [ ] Pixel Party scan.
- [ ] Skip disabled producers entirely.
- [ ] Reset the next deadline on enable to avoid a delayed first update.
- [ ] Prevent a slow producer from causing catch-up loops.

Suggested initial budgets:

| Workload | Active interval | Inactive behavior |
|---|---:|---|
| Auto Rod pending transaction | 5 ms | No polling |
| Combat target snapshot | 5–10 ms | No polling |
| Player/nametag snapshot | 40–50 ms | Clear on transition |
| Fight Status aggregation | 40–50 ms | Clear on disable |
| Chest Stealer | Feature-configured delay | No polling |
| Chest ESP | 100–200 ms | Clear on disable |
| Block ESP | 150–250 ms or movement-triggered | Clear on disable |
| Pixel Party | 20–50 ms while active | Clear on disable |

Acceptance criteria:

- Enabling Aim Assist or KillAura does not accelerate Chest ESP or Block ESP.
- Expensive scan p99 durations do not appear in the combat deadline budget.
- Disable and world exit clear published snapshots.

### P3.2 Add scan budgets and graceful degradation

- [ ] Bound entity, chest, block, and chunk work per cycle.
- [ ] Carry remaining work to the next cycle rather than producing a long stall.
- [ ] Prioritize nearest or currently displayed candidates.
- [ ] Track skipped/deferred work in diagnostics.
- [ ] Never defer owned-state restoration or safety cleanup.

Acceptance criteria:

- Pathological worlds do not create unbounded scan duration.
- The renderer can distinguish an empty snapshot from an incomplete snapshot
      where necessary.

### P3.3 Apply equivalent budgeting to the legacy bridge

- [ ] Separate general game-state publication from optional ESP scans.
- [ ] Avoid running every enabled state mutation through a generic dispatch
      loop.
- [ ] Preserve the legacy menu-injection mapping recovery behavior.
- [ ] Keep Reach click-edge handling on its validated narrow path.

Acceptance criteria:

- 1.8.9 input behavior and menu-to-world recovery remain equivalent.
- Idle and normal-play CPU usage do not regress.

## Phase 4: Make rendering consume immutable prepared snapshots

### P4.1 Capture one frame snapshot

- [ ] At frame start, copy or acquire:
  - [ ] Configuration needed for drawing.
  - [ ] HUD layout.
  - [ ] Theme.
  - [ ] Camera/viewport state.
  - [ ] Player/nametag snapshot.
  - [ ] Chest snapshot.
  - [ ] Block ESP snapshot.
  - [ ] Fight Status snapshot.
  - [ ] Pixel Party snapshot.
- [ ] Resolve HUD element layouts once from that frame snapshot.
- [ ] Do not reacquire configuration or HUD locks from individual panels.
- [ ] Do not hold a producer mutex while issuing ImGui calls.

Implementation options to benchmark:

1. Short lock plus copy into a reusable render-owned buffer.
2. Double-buffered producer snapshots with a short index swap.
3. Atomic `shared_ptr` publication using the C++11 free functions, only if
   allocation and reference-count costs are acceptable.

Choose by measurement rather than abstraction preference.

Acceptance criteria:

- Render code holds no chest/player/block producer mutex during drawing.
- A ThreadSanitizer-equivalent review or focused stress harness covers snapshot
  publication semantics where tooling permits.
- OpenGL and Vulkan output remain equivalent.

### P4.2 Remove per-frame heap churn

- [ ] Reserve known maximum sizes for render candidate vectors.
- [ ] Reuse row, line, aggregation, and smoothing buffers.
- [ ] Move sorting and nearest-N selection to background producers where the
      result does not depend on the current frame camera.
- [ ] Replace insertion-based sorting with bounded selection followed by one
      sort.
- [ ] Cache static labels and colors.
- [ ] Confirm no buffer retains unbounded capacity after pathological input.

Acceptance criteria:

- Allocation sampling shows no steady per-frame allocations in the overlay
  dispatch itself.
- p99 overlay frame time improves or remains neutral on both renderers.

### P4.3 Harden renderer reset and shutdown

- [ ] Port only the verified Vulkan reset/teardown fixes needed from the archive.
- [ ] Preserve backend arbitration.
- [ ] Test resize, minimize/restore, swapchain recreation, disconnect, panic,
      and process shutdown.
- [ ] Keep renderer cleanup separate from JNI cleanup ownership.

Acceptance criteria:

- No device loss or stale ImGui resource after repeated reset cycles.
- Shutdown is idempotent.

## Phase 5: Improve managed input timing without a general runtime

### P5.1 Add a small owned-loop helper

- [ ] Create a narrowly scoped helper for one loop's task, cancellation source,
      and awaited cleanup.
- [ ] Make start/stop state changes synchronous to the caller.
- [ ] Serialize start and stop for the same loop only.
- [ ] Do not maintain a global module registry.
- [ ] Do not permanently poison a module after one unexpected loop failure.
- [ ] Log and expose unexpected completion in dev diagnostics.

Candidates:

- AutoClicker.
- Right Clicker.
- Aim Assist.
- Triggerbot.
- Pixel Party managed input.

Migrate one loop at a time with characterization tests.

Acceptance criteria:

- Rapid enable/disable/enable ends in the last requested state.
- Stop prevents new input immediately and awaits cleanup asynchronously where
  the caller permits.
- A loop can be deliberately restarted after a handled failure.
- Panic and application exit await all owned-loop cleanup.

### P5.2 Use deadline-based AutoClicker scheduling

- [ ] Measure existing requested-versus-actual interval error.
- [ ] Schedule the next click against an absolute `Stopwatch` deadline to avoid
      cumulative delay drift.
- [ ] Preserve randomized CPS distribution and existing input guards.
- [ ] Avoid busy-waiting.
- [ ] Evaluate a high-resolution waitable timer only if measured `Task.Delay`
      jitter remains unacceptable.
- [ ] Do not raise global Windows timer resolution permanently.

Acceptance criteria:

- CPS distribution remains inside configured bounds.
- Click interval p95/p99 jitter improves or remains neutral.
- Idle CPU and battery impact do not regress.

### P5.3 Preserve dispatcher affinity

- [ ] Keep WPF shutdown and stealth-mode operations on the dispatcher.
- [ ] Audit `ConfigureAwait(false)` across methods that resume into UI work.
- [ ] Add regression tests for panic invoked from both UI and worker threads.

Acceptance criteria:

- Panic reliably disables features, releases input, disconnects, enters stealth
  mode, and shuts down without cross-thread access.

## Phase 6: Port correctness improvements selectively

Review the archived snapshot commit `62a2ee7` and port fixes as independent
changes. Do not cherry-pick architecture-dependent commits wholesale.

### P6.1 JNI ownership and lifetime

- [ ] Port the KillAura packet callback lifetime gate if it can be isolated.
- [ ] Verify remap, world exit, and shutdown drain callbacks before deleting JNI
      references.
- [ ] Ensure every worker uses only its attached thread's `JNIEnv*`.
- [ ] Remove any shared owner object that can publish a foreign or dangling
      `JNIEnv*`.
- [ ] Add stress tests for callback entry racing disable/remap/shutdown.

### P6.2 Mapping probe control

- [ ] Cache successful method and field IDs.
- [ ] Record unsupported optional mapping probes per mapping generation.
- [ ] Retry only after a generation change or explicit reload.
- [ ] Keep Yarn-first and Mojmap-fallback ordering.
- [ ] Do not treat optional mapping failures as core mapping failure.

Acceptance criteria:

- Unsupported optional mappings do not generate repeated JNI exceptions in a
  scan loop.
- Explicit Reload Mappings resets the appropriate attempt state.

### P6.3 Direct lifecycle cleanup

- [ ] Define explicit cold-path functions for:
  - [ ] World exit.
  - [ ] Mapping invalidation.
  - [ ] Renderer reset.
  - [ ] Client disconnect.
  - [ ] Panic.
  - [ ] Shutdown.
- [ ] Call feature-owned cleanup directly from these transition points.
- [ ] Make each cleanup function idempotent.
- [ ] Keep JNI reference deletion on the owning attached worker.
- [ ] Add state-restoration tests for Reach, Velocity, AutoTotem, Auto Rod,
      SpeedBridge, AntiDebuff, Hit Delay Fix, and KillAura.

### P6.4 Small compatibility fixes

- [ ] Verify and port legacy `nametagShowHeldItem` parsing.
- [ ] Port strict finite/integer bounds where they apply to the existing V1
      parser.
- [ ] Port bounded crash/config diagnostics only if disabled or change-triggered
      overhead is negligible.
- [ ] Port Vulkan helper fixes with their focused native tests.

## Phase 7: Improve maintainability through static composition

### P7.1 Split bridge implementation files

- [ ] Establish native folders for combat, movement, visuals, utility, protocol,
      and lifecycle helpers.
- [ ] Move one feature at a time without changing its call frequency or thread.
- [ ] Keep bridge composition roots responsible for hooks, worker startup,
      socket ownership, and renderer arbitration.
- [ ] Use direct function calls or concrete feature objects.
- [ ] Avoid virtual dispatch and string lookup in tick/render paths.
- [ ] Keep mapping-family adapters explicit.

Suggested extraction order:

1. Pure helpers and serializers.
2. Visual snapshot producers.
3. Visual renderers.
4. Auto Rod.
5. SpeedBridge.
6. Reach/Velocity/AutoTotem.
7. KillAura last.

Acceptance criteria for each extraction:

- Functional diff is zero except for required include/build wiring.
- Native harness and both bridge builds pass.
- Measured performance remains neutral.
- The extraction can be reverted independently.

### P7.2 Use typed snapshots at boundaries

- [ ] Define plain structs for configuration and prepared state.
- [ ] Validate and clamp only when accepting configuration.
- [ ] Publish a complete validated configuration snapshot.
- [ ] Avoid repeated string-based setting lookup after parsing.
- [ ] Keep structs version/family-specific where layouts genuinely differ.

### P7.3 Add compile-time/catalog consistency checks

- [ ] Retain existing `ModuleCatalog` as the UI/profile compatibility source.
- [ ] Add tests that enumerate profile, keybind, capability, and payload
      coverage.
- [ ] Avoid a second runtime descriptor system unless it generates an existing
      surface.
- [ ] Prefer source-generated or test-time validation over runtime lookup.

## Phase 8: Protocol and configuration hardening without a rewrite

### P8.1 Harden the existing V1 transport

- [x] Enforce maximum line and receive-buffer sizes.
- [x] Reject non-finite numbers and out-of-range integer conversions.
- [ ] Preserve unknown-field tolerance.
- [x] Keep malformed-line handling fail-open for the connection.
- [ ] Add representative fixtures for both bridge families.
- [ ] Add loader-newer/bridge-older and bridge-newer/loader-older tests.

### P8.2 Revisit protocol versioning only with evidence

A V2 protocol should be reconsidered only if at least one of these becomes
necessary:

- Partial fast/full state messages cannot be represented safely in V1.
- Released loader/bridge compatibility requires negotiated capabilities.
- The flat configuration prevents a measured optimization.
- A new command needs acknowledgment or ordering guarantees.

If reconsidered:

- [ ] Start with one optional message, not a complete migration.
- [ ] Keep the V1 fallback.
- [ ] Measure serialization, parsing, and bandwidth.
- [ ] Define rollback behavior before enabling it by default.

## Phase 9: Verification and release discipline

### P9.1 Automated gates

Run before every behavior-affecting merge:

```powershell
dotnet build Aoko\Aoko.csproj
dotnet test Aoko.Tests\Aoko.Tests.csproj
McInjector\run_tests.bat
McInjector\build.bat
McInjector\build_261.bat
```

Additional requirements:

- [ ] Add targeted managed tests for every changed scheduling or merge rule.
- [ ] Add native harness tests for pure timing, snapshot, mapping, and cleanup
      helpers.
- [ ] Keep test-only tracing disabled in release DLLs.
- [ ] Resolve C++ standard warnings where C++17 inline variables are compiled
      under the documented C++11 target.

### P9.2 Live smoke checklist

For each supported runtime:

- [ ] Inject from menu.
- [ ] Enter and leave a world.
- [ ] Reload mappings.
- [ ] Disconnect and reconnect the loader.
- [ ] Enable and disable each changed module repeatedly.
- [ ] Exercise panic.
- [ ] Close the loader before the client.
- [ ] Close the client before the loader.
- [ ] Verify owned key/button/state restoration.
- [ ] Check bridge logs for repeated mapping failures or exceptions.
- [ ] Compare performance capture with the Phase 0 baseline.

### P9.3 Merge policy

- [ ] One optimization concern per commit or tightly related commit series.
- [ ] Include baseline and result numbers in the commit or verification record.
- [ ] Do not combine source extraction with behavioral optimization.
- [ ] Land on `dev`.
- [ ] Soak on `dev` before promotion to `main`.
- [ ] Keep the previous release artifacts available for rollback.

## Recommended implementation sequence

The first execution cycle should be:

1. P0.1–P0.3: instrumentation and baseline.
2. P1.1: configuration dirty tracking.
3. P1.3: UI notification coalescing.
4. P2.1: state consumer/freshness inventory.
5. P2.2–P2.3: split fast/full state and remove socket-loop projection.
6. P3.1: independent producer deadlines.
7. P4.1–P4.2: render snapshots and allocation reduction.
8. Repeat the complete baseline capture.
9. Only then begin managed timing and source-organization work.

This ordering targets the largest known overhead first: oversized 5 ms state
publication, repeated JSON work, coupled scan frequencies, and render snapshot
contention.

## Initial task backlog

These tasks are ready to become small implementation issues:

- [ ] PERF-001: Add native bounded timing counters.
- [ ] PERF-002: Add managed transport and click-jitter counters.
- [ ] PERF-003: Capture baseline results for all supported runtime scenes.
- [ ] PERF-004: Add bridge-config revision and dirty-send behavior.
- [ ] PERF-005: Cache serialized configuration and add recovery heartbeat.
- [ ] PERF-006: Coalesce WPF state notifications while retaining latest input
      state.
- [ ] PERF-007: Document every `GameState` consumer and freshness requirement.
- [ ] PERF-008: Add merge-by-presence support for partial state.
- [ ] PERF-009: Split fast input state from full UI/entity state.
- [ ] PERF-010: Move entity body projection out of the socket loop.
- [ ] PERF-011: Add independent modern producer deadlines.
- [ ] PERF-012: Decouple ESP scanning frequency from Aim Assist/KillAura.
- [ ] PERF-013: Capture one immutable HUD/config snapshot per frame.
- [ ] PERF-014: Remove producer locks from ImGui drawing.
- [ ] PERF-015: Reuse render candidate and text buffers.
- [ ] PERF-016: Replace insertion sorting with bounded selection plus sort.
- [ ] PERF-017: Add a per-loop managed ownership helper.
- [ ] PERF-018: Add deadline-based AutoClicker scheduling and jitter tests.
- [ ] SAFE-001: Port verified JNI callback lifetime hardening.
- [ ] SAFE-002: Port mapping-generation attempt caching.
- [ ] SAFE-003: Add direct lifecycle cleanup and restoration tests.
- [ ] SAFE-004: Port and test Vulkan reset/shutdown fixes.
- [ ] COMPAT-001: Verify legacy held-item nametag configuration.
- [ ] COMPAT-002: Harden V1 numeric and message-size validation.
- [ ] MAINT-001: Extract pure native helpers without behavior changes.
- [ ] MAINT-002: Extract one visual producer and renderer with direct dispatch.
- [ ] QA-001: Automate benchmark summary comparison.
- [ ] QA-002: Maintain the supported-runtime smoke verification record.

## Definition of done

The improvement program is complete when:

- Performance measurements show a meaningful improvement over the original
  `dev` baseline across normal, combat, and visual-heavy scenes.
- Full state, fast input state, and producer scan rates match consumer needs.
- Unchanged configuration is not repeatedly serialized and parsed.
- The render thread consumes prepared snapshots without JNI discovery,
  producer-lock drawing, or steady heap churn.
- Managed input loops start, stop, fail, and restart deterministically.
- JNI ownership and lifecycle cleanup pass automated and live transition tests.
- Bridge source is easier to navigate through static feature extraction without
  a runtime registry in hot paths.
- All supported runtime and renderer combinations pass the smoke matrix.
- `dev` has soaked successfully and is ready for a normal promotion decision.
