# Migration Architecture: Aoko managed host, C#/WPF -> Rust/GPUI

Status: Phase 0 (architecture + contract extraction). Branch `rust`, forked from local `HEAD` `f1a9f5f9362cd274a97531bc0c10a6e86d291ca0`.

This document is the decision record for replacing the managed host. It records what each Rust crate owns, which trait seams were chosen, why, and where the plan has limits. Behavioural parity detail lives in `MIGRATION_PARITY.md`; the byte-level wire contract lives in `docs/rust/WIRE_CONTRACT.md`; GPUI API facts pinned to the vendored revision live in `GPUI_API_NOTES.md`.

## 1. Scope and hard boundaries

Rewritten: the entire C# host under `Aoko/` (WPF views, dialogs, theming, theme manager, clicker engine, input hooks, profile persistence, TCP client, GTB solver, accounts, servers, Discord RPC, updater) and the test suite under `Aoko.Tests/` (52 test files).

Not rewritten, ever: `McInjector/` C++ JNI/JVMTI bridges. They are the immutable native counterpart and define the wire contract. `Aoko/` and `Aoko.Tests/` remain in-tree as executable ground truth until the final sign-off gate, then are removed in a dedicated commit.

Consequence to hold onto: only the *host* half of the protocol is ours. Any field spelling, unit, or ordering question is settled by reading `McInjector/src/main/cpp/`, never by preference.

## 2. Crate map

Four crates, layered strictly downward. A crate may depend on the layer below, never sideways, never upward.

```
aoko-app      GPUI binary, views, design system, screenshot harness
    |
aoko-core     controller state machines, schedulers, app coordination
    |
aoko-platform OS traits + Windows/Linux implementations
    |
aoko-protocol pure data: wire types, profile, catalog, solver. no OS, socket, process or UI dependency.
```

| Crate | Owns | Ported from |
| --- | --- | --- |
| `aoko-protocol` | Wire types, capability negotiation, bounded framing, game state, patch merging, profile + palette + HUD layout persistence, module catalog, stats, GTB solver, transport metrics, `netstat` line parsing | `GameState.cs`, `GameStatePatchMerger.cs`, `BoundedLineReader.cs`, `BridgeCapabilities.cs`, `ManagedTransportDiagnostics.cs`, `TcpPortHelper.cs` (parse half), `GameStateClient.cs` (transport seam), `HudLayout.cs`, `KillAuraSettings.cs`, `NickHiderConfig.cs`, `BlockEsp.cs`, `ModuleCatalog.cs`, `Profile.cs`, `CustomPalette.cs`, `GtbWordSolver.cs`, `BridgeCommandProtocol.cs`, `BridgeCommandResult.cs`, `Accounts/*` pure parsers, `Servers/ServerAddress.cs` (parse half), `StatsTracker.cs` (aggregation half) |
| `aoko-platform` | `ProcessInspector`, `WindowInspector`, `InputBackend`, `NativeBridgeInjector`, `DiscordRpcTransport`, `PlatformCapabilities`, `TcpPortProbe` | `ProcessCommandLine.cs`, `InjectionTargetDiscovery.cs`, `WindowDetection.cs`, `InputHooks.cs`, `NativeInjector.cs`, `DiscordRichPresenceService.cs` (transport half), `Servers/ServerPing.cs`, `Servers/ServerAddress.cs` (SRV lookup half), `TcpPortHelper.cs` (process spawn half) |
| `aoko-core` | `clicker/`, `aim_assist`, `chest_stealer`, `refill`, `auto_rod`, `auto_tool`, `concurrency/`, `services/` | `Clicker.cs`, `AdvancedClickTimingState.cs`, `ClickDeadlineScheduler.cs`, `ClickRandomizationMode.cs`, `AimAssistMotionController.cs`, `ChestStealerController.cs`, `ChestStealerCoordinateMapper.cs`, `RefillController.cs`, `CoalescedCallback.cs`, `CoalescedLatestValue.cs`, `OwnedAsyncLoop.cs`, `UpdateService.cs`, `DiscordRichPresenceService.cs` (state half), `Accounts/*` services, `Servers/*Store/Join*`, `ModuleSearchIndex.cs` |
| `aoko-app` | `theme/`, `assets/`, `components/`, `views/`, `dialogs/`, `capture/`, `main.rs` | `App.xaml`, `MainWindow.xaml(.cs)`, `ColorPickerDialog.*`, `WindowPickerDialog.*`, `ThemeManager.cs`, `StatsTracker.cs` (change-notification half), `Views/*` |

### Placement rule

One rule settles every "which crate owns this file" question, so the two gating documents
cannot drift apart again:

- `aoko-protocol` may do computation, parsing and persistence. It may **not** open a socket,
  spawn a process, read a file outside an in-memory buffer it was handed, or call the OS.
  Its only dependencies are `serde`, `serde_json`, `thiserror` and `tracing`.
- `aoko-platform` owns anything that touches the OS: sockets, processes, windows, hooks,
  named pipes, `/proc`, X11.
- `aoko-core` owns state machines and coordination, and may call both of the above.

Where a C# file straddles the line, it is split and both halves are named in the table, rather
than being awarded to one crate and silently violating the rule. `GameStateClient.cs` splits
three ways and `TcpPortHelper.cs` splits two ways for exactly this reason: `TcpPortHelper`
runs `netstat -ano -p tcp` through `ProcessStartInfo` and regex-matches the output, so the
regex and line model live in `aoko-protocol` and the spawn lives in `aoko-platform`. That
split is also what keeps `TcpPortHelperTests`, which tests the regex and the null-on-no-match
behaviour, a pure protocol test.

Three more files split for the same reason, and are listed as halves in the table above:

- `Servers/ServerAddress.cs` is mostly pure — `TryParse`, `DefaultPort = 25565`, the 256-character
  cap — but `ResolveForPingAsync` opens a `UdpClient` for SRV lookup. Parsing and validation are
  `aoko-protocol`; the SRV query and its cache are `aoko-platform`.
- `StatsTracker.cs` is pure aggregation (CPS buckets, playtime, left/right counts) wrapped in
  `INotifyPropertyChanged` with a marshalled UI-thread notification. The aggregation and its
  persistence are `aoko-protocol`; turning a change into a scoped GPUI `cx.notify()` belongs to
  `aoko-app`, which is where the reactivity rules in section 6 already live.
- `Servers/ServerPing.cs` opens the socket, so it is `aoko-platform` in full. Its
  `ParseStatus`/`FlattenMotd` decode helpers are pure, but they are only useful beside the socket,
  so they stay with it rather than being split for their own sake.

### Why `GameStateClient` splits across two crates

C# `GameStateClient.cs` mixes three concerns: byte framing and schema (protocol), socket ownership and reconnect policy (platform), and publication of coalesced state to consumers (core). Keeping it whole would force `aoko-protocol` to take a socket dependency and stop it being pure. Split instead:

- `aoko-protocol::client` — the `Transport` trait plus decoding/encoding. Testable with an in-memory duplex transport, which is what `GameStateClientSeamTests` and `GameStateClientInjectionTests` already do through their own fakes.
- `aoko-platform` — supplies a `TcpTransport` implementation.
- `aoko-core::services` — reconnect policy, backoff, heartbeat cadence, coalescing, diagnostics counters.

## 3. Trait seams

Defined in Phase 0 so the three Phase 1 builders never block on each other.

### `aoko-protocol` (no OS, no runtime)

```rust
pub trait Transport: Send {
    fn recv_line(&mut self, out: &mut LineBuf) -> Result<RecvOutcome, TransportError>;
    fn send_line(&mut self, line: &[u8]) -> Result<(), TransportError>;
}
```

`LineBuf` is a reusable, capacity-bounded buffer. `recv_line` appends into it rather than returning `String`, which is what makes steady-state frames allocation-free.

### `aoko-platform` (one trait per OS concern)

```rust
pub trait ProcessInspector: Send + Sync {
    fn list_processes(&self) -> Result<Vec<ProcessInfo>, PlatformError>;
    fn command_line(&self, pid: u32) -> Result<String, PlatformError>;
    fn discover_instances(&self) -> Result<Vec<InstanceCandidate>, PlatformError>;
}

pub trait WindowInspector: Send + Sync {
    fn enumerate(&self) -> Result<Vec<WindowInfo>, PlatformError>;
    fn foreground(&self) -> Result<Option<WindowInfo>, PlatformError>;
    fn client_bounds(&self, handle: WindowHandle) -> Result<Rect, PlatformError>;
}

pub trait InputBackend: Send + Sync {
    fn mouse_state(&self) -> MouseState;
    fn key_state(&self, key: Key) -> bool;
    fn click(&self, button: MouseButton) -> Result<(), PlatformError>;
    fn move_relative(&self, dx: i32, dy: i32) -> Result<(), PlatformError>;
    fn key_press(&self, key: Key) -> Result<(), PlatformError>;
    fn poll_events(&self, out: &mut Vec<InputEvent>) -> Result<(), PlatformError>;
}

pub trait NativeBridgeInjector: Send + Sync {
    fn inject(&self, pid: u32, dll: &Path) -> Result<InjectionReport, PlatformError>;
    fn supported(&self) -> BridgeSupport;
}

pub trait DiscordRpcTransport: Send {
    fn connect(&mut self, app_id: &str) -> Result<(), PlatformError>;
    fn write_frame(&mut self, op: u32, payload: &[u8]) -> Result<(), PlatformError>;
    fn read_frame(&mut self, out: &mut Vec<u8>) -> Result<Option<DiscordFrame>, PlatformError>;
}

pub trait PlatformCapabilities {
    fn report(&self) -> CapabilityReport;
}
```

`CapabilityReport` is a data struct, not a trait method per feature: the UI renders capability banners from it, so adding a capability must not change call sites.

### `aoko-core` (determinism seams)

```rust
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;          // monotonic
}
pub trait Sleeper: Send + Sync {
    fn sleep_until(&self, deadline: Instant);
}
pub trait Rng: Send {
    fn next_f64(&mut self) -> f64;
    fn next_range(&mut self, low: f64, high: f64) -> f64;
}
```

Every timing-sensitive controller takes `Arc<dyn Clock>`, `Arc<dyn Sleeper>`, and an owned `Box<dyn Rng>`. Production wires `SystemClock`/`ThreadSleeper`/`SplitMix64`; tests wire a virtual clock that advances only when the sleeper is asked to sleep. This is what makes `ClickDeadlineSchedulerTests`, `ClickerRandomizationTests`, and `AimAssistMotionControllerTests` deterministic rather than flaky under load.

## 4. Threading model

The single rule: the GPUI UI thread never blocks, and automation never runs on it.

| Thread | Owns | Blocking allowed |
| --- | --- | --- |
| GPUI main | `App`, windows, entities, `Render` | never |
| transport | socket read loop, decode, publish snapshot | yes, on socket |
| automation | clicker, aim assist, chest stealer, refill ticks | yes, on precise sleeps |
| platform-poller | process/window discovery, adaptive backoff | yes |
| discord | IPC socket | yes |

Cross-thread communication:

- Telemetry in: transport decodes into a `GameStatePatch`, applies it to a double-buffered state, then publishes an `Arc<GameStateSnapshot>` through an `arc-swap` cell. The UI reads the current snapshot without locking. UI entities are notified only when a field they display actually changed - not on every patch.
- Config out: slider drags coalesce through `CoalescedLatestValue` (25 ms, matching `ConfigChangeCoalesceMs`), so a drag emits one config push, not one per pixel.
- Automation commands: `CoalescedCallback` where latest-wins is correct; a bounded channel where every event matters.

Preserved semantics, verified against the C# originals rather than re-derived: `CoalescedCallback` and `CoalescedLatestValue` keep their drop-oldest/keep-latest behaviour and their flush-on-stop guarantee.

## 5. Platform capability matrix

Explicit, runtime-reported, and rendered in the UI. No silent degradation.

| Capability | Windows | Linux (Phase 1) |
| --- | --- | --- |
| Process enumeration + full command line | Win32 PEB read | `/proc/<pid>/cmdline`, `/comm`, `/exe` |
| Minecraft instance discovery | process + window match | `/proc` scan, cache-backed |
| Window enumeration + client bounds | `EnumWindows`, `GetClientRect` | X11 EWMH (`_NET_ACTIVE_WINDOW`, `_NET_WM_PID`); Wayland via compositor IPC where available; otherwise reported absent |
| Global input hook | `SetWindowsHookExW` | X11 XTest/XInput, or `evdev` + `uinput` |
| Synthetic click/move/key | `SendInput`, relative mouse | XTest, or `uinput` injection |
| Native bridge injection | `LoadLibrary` in target PID | `BridgeUnsupportedOnLinuxYet` unless a `.so` path is configured |
| Discord IPC | named pipe | `$XDG_RUNTIME_DIR/discord-ipc-{0..9}` incl. Flatpak/Snap paths |
| TCP bridge transport | yes | yes (contract identical) |

Linux input is the honest risk: `uinput` needs group permissions or a udev rule, and a Wayland session may deny global input entirely. The design response is `PlatformCapabilities::report()` plus an actionable diagnostic string, and the UI stays fully usable with input modules disabled. It is not acceptable to panic, silently no-op, or pretend a click happened.

## 6. Performance engineering standards

Binding on every crate, measured in Phase 3.

- Framing: `BoundedLineReader` reuses its buffer; steady-state frames allocate only when a line must outlive the buffer.
- Patch merging: mutate in place, publish `Arc` snapshots. Do not deep-clone unchanged strings or maps per packet.
- Hot loops: allocation-free per tick, `Instant` absolute deadlines so sleep error cannot accumulate, hybrid sleep/spin only where sub-millisecond click scheduling needs it.
- GTB solver: index the wordlist once at startup; live queries touch no allocator beyond the output.
- UI: no per-frame cloning or style rebuilding in `Render::render`; static tokens precomputed; filtered lists memoized; `cx.notify()` scoped to entities whose displayed fields changed.
- Polling: adaptive backoff, cached PID metadata, no aggressive `/proc` sweeps when idle or unfocused.
- Budgets: obey `docs/performance/STATE_FIELD_BUDGETS.md` and `docs/DEV_PERFORMANCE_AND_RELIABILITY_PLAN.md`; never request a state field nothing reads.

## 7. GPUI pinning

GPUI is consumed as a git dependency pinned to a single zed revision, not as a floating branch and not from crates.io.

- crates.io `gpui 0.2.2` does not build against a current `libc`: its transitive `xattr` crate references `libc::ENOATTR`, which no longer exists on Linux. Verified by attempting the build.
- zed `main` pins `channel = "1.100.0"`, so the vendored revision builds on stable Rust with no nightly features - `rust-toolchain.toml` mirrors that pin.
- The revision is pinned because GPUI's API churns; `GPUI_API_NOTES.md` documents the exact surface at that revision so implementers work from fetched source, not from memory.

Cargo is configured with `net.git-fetch-with-cli = true` in `~/.cargo/config.toml` because libgit2's full-history clone of zed is prohibitively slow.

## 8. Contract decisions requiring arbitration

Phase 0 discovery surfaced places where the sources disagree with themselves. Each is decided
here so no builder invents semantics or "fixes" the C# behaviour. Deviating from these is a
parity bug, not an improvement.

| # | Question | Decision | Why |
| --- | --- | --- | --- |
| 1 | Two palette tables coexist: `MainWindow.GuiPalettes` (10 roles) and `Core/ThemeManager.Themes` (8 roles). | **`GuiPalettes` is normative.** The Rust `Theme` has 10 roles. `ThemeManager.Themes` becomes a derived 8-role view, not a second source. | The persisted user type `CustomPalette.cs` carries `TabSelected`/`TabHover`, which only `GuiPalettes` defines. On `Slate` the two agree on all 8 shared roles, so nothing is lost. `MainWindow.ApplyGuiTheme` is the live render path (6 call sites); `ThemeManager.ApplyTheme` only fires from `Profile.cs` during load. `AccentSecondary` is assigned in all 18 palettes of both tables and read at no render site, so 9 of the 10 roles are live: carry the field for schema completeness and do not invent a visual use for it. |
| 2 | The capability `state` arrays disagree with what the bridges actually emit: `bridge.cpp` sends `health`/`posx`/`posy`/`posz` that its legacy array omits, and both bridges send `guiOpen`/`screenName`/`entities` that neither advertises. | **Never gate decoding on the `state` capability list.** Decode permissively: unknown fields ignored, absent fields defaulted. | `docs/performance/STATE_FIELD_BUDGETS.md` already mandates tolerance of unknown fields and defaults for absent ones. Capabilities gate *writes and UI affordances*, not deserialization. |
| 3 | `BridgeCapabilities.ForVersionFallback` is stale and partly inverted versus `bridge_capabilities.h` (155 vs 200 modern settings, 143 vs 194 legacy; the two disagree about `health`/`pos*`). | **Preserve the C# fallback exactly** as it is, quirks included. Do not reconcile it to the header. | The fallback is what the shipped C# host tells the UI before a `capabilities` frame arrives. Existing behaviour is the compatibility contract. The header is what a bridge advertises when it does send capabilities. The two intentionally differ; document, do not unify. |
| 4 | `hudLayout` is extracted by brace-matching in C#, not parsed, so a `"hudLayout":` sequence inside a string value could hijack it. | Parse the whole line as JSON first. Only if that fails, fall back to the legacy brace scan. | For every valid message both approaches agree, so parity holds. For adversarial input the new path is strictly safer. This is a documented, deliberate deviation. |
| 5 | `commandResult` can emit `inWorld` and `ready` twice (session block plus `in_world` failure tail). | **Last-wins**, matching `System.Text.Json` and therefore the C# host. Do not error on duplicate keys. | Divergence here would change which value the UI reads on the exact failure path users hit most. |
| 6 | The 1.8.9 bridge sends a state line with **no `type` field at all**. | The decoder accepts both a bare state object and a typed one. A missing `type` on an object that looks like state means state. | A strict `type` discriminator would reject the entire legacy wire format. |
| 7 | `ModuleCatalog.Entry.ManagedOnly` is declared but **no** entry sets it. | Keep the field, always `false`. Do not start honouring it. | Honouring a flag nothing sets would invent gating behaviour the shipped app never had. |
| 8 | Per-property ordering inside `BuildConfigPayload` is not documented. | JSON object key order is **not** part of the contract; TCP framing bytes are. | `ParseConfig` on both bridges is key-based. Framing (newline-delimited UTF-8, 1 MiB inbound cap, trailing `\r` trimmed) is byte-level and must match exactly. |
| 9 | `scripts/New-GuiScreenshots.ps1` drives the tab `SelectedIndex` from a seven-name array while the window has nine tabs. | The Phase 2 capture harness enumerates all nine tabs by name in the real order, and the existing artifacts are **not** screenshot parity evidence. | Real order: Combat, Render, Utility, Risky, Beta, Accounts, Servers, Stats, Settings. Index 5 writes `gui-stats.png` from Accounts and index 6 writes `gui-settings.png` from Servers, so two files are misnamed, and Stats and Settings were never captured at all. |

## 9. Documented trade-offs

Recorded here rather than hidden.

1. **Linux native bridge is a trait stub, by design.** `NativeBridgeInjector` reports unsupported. Data arrives only if a future `.so` bridge exists. The UI must therefore treat "no game state" as a normal state, not an error.
2. **Wayland window inspection is compositor-specific.** Hyprland/Sway expose IPC that makes it tractable; GNOME/KDE do not offer an equivalent portable path. Capability flags carry this, and XWayland windows are handled through the X11 path.
3. **`Aoko/` and `Aoko.Tests/` stay in the tree through every phase.** This costs repo size and means two implementations coexist. It also means every golden fixture is diffable against real C# behaviour, which is worth more than tidiness until sign-off.
4. **`Aoko/wndproc_new.cpp` is dead code.** 3.8 KB, absent from `Aoko.csproj`, referenced by no source file, script, or doc in the repo. It is not ported and not treated as a contract.
5. **Split of `GameStateClient`** trades a one-to-one file mapping for layer purity. Tracked in `MIGRATION_PARITY.md` so the mapping stays auditable.
6. **`aoko-platform` depends on `aoko-protocol`** for shared types, so "platform" is not importable by the protocol crate. This is intentional: the wire format must not learn OS concepts.
7. **Unobservable input is not "nothing held" (Linux).** `InputBackend::probe_mouse_state` and `probe_key_state` return `Option`, and `None` means the session cannot observe input, not that no button is down. The clicker hold guards refuse to act on `None` rather than assuming a free button. C# never faced this case, because `GetAsyncKeyState` always answers; it is therefore a new-platform decision, not a parity break. Coercing it with `unwrap_or(false)` is exactly the silent no-op that section 5 forbids.
8. **SRV resolution splits across two crates.** `aoko-protocol` owns the DNS wire codec (`srv_query_name`, `build_srv_query`, `try_read_srv`, `resolve_dns_servers`); `aoko-platform` owns the UDP exchange and the TTL cache, because only the platform crate may open a socket. The managed `SrvLookupCache` semantics are preserved exactly: 2 s per-resolver timeout, 10 min negative TTL, 1 h positive TTL, case-insensitive keys, and a cancelled lookup is never cached because caching it would send the next join to port 25565.

9. **Profile writes are atomic, unlike `File.WriteAllText`.** `ProfileManager.TryWriteProfile` truncated the target in place, so a process that died mid-write left a truncated profile that `LoadProfile` then silently reported as `null`. `aoko_platform::ProfileStore::write_profile` writes a sibling temporary file in the same directory, flushes it with `sync_all`, and renames it over the target. On success the observable result is identical - the same bytes at the same path - but a failure leaves the previous contents intact and removes the temporary. The fsync is deliberate: an unsynced rename can be reordered before the data is durable, which would turn a torn file into an empty one, and an empty profile is worse than a torn one because nothing signals the loss. Relatedly, where the managed host caught `IOException`/`UnauthorizedAccessException` and returned silently, every `ProfileStore` operation now returns a classified `PlatformError`; legacy migration stays best-effort exactly as it was, but logs at `debug` instead of vanishing. The legacy directory is read but never created, because inventing `LegoClicker/profiles` would leave state the old app never wrote.

10. **Deleting a config that is already gone says so.** `Profile.DeleteProfile` (`Profile.cs:556`) silently no-ops when the file is missing, and the card then reports "Deleted", so it claims success for something it did not do. The Rust card reports `Could not delete '{name}': it no longer exists` and refreshes the list. This is a deliberate, user-visible deviation on a failure path, because reporting success for a failure is the class of defect this migration exists to remove. Two smaller deviations in the same card: the confirmation buttons read Delete/Cancel rather than Yes/No, since a Yes/No pair only carries meaning beneath a dialog title and this is an in-card prompt; and deleting every named config is **not** a reset, because the auto-save slot is separate and the running settings still save on exit and restore on the next launch.

## 10. Verification commands

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
cargo bench --workspace
cargo run -p aoko-app --features capture -- --capture-screenshots <out_dir>
```

The capture run needs `--features capture`: without it the binary exits 1 and prints the correct form
itself, because `Window::render_to_image` and the headless renderer both sit behind gpui's
`test-support` at the pin. A documented command that cannot run is worse than an undocumented one,
because it reads as verified.

The Windows leg needs the runner variable, or the suite cross-compiles and never executes:

```bash
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine \
  cargo test -p aoko-platform --target x86_64-pc-windows-gnu
```

Without it the target still builds and never runs, so a suite that executed nothing reads as a pass. The doc gate needs `RUSTDOCFLAGS` set on the command line rather than in a manifest: plain `cargo doc` treats a broken intra-doc link as a warning, and on a crate with this many cross-references an unchecked link rots silently.

CI runs the Linux and Windows matrices; `McInjector` DLL pipelines are untouched.

### Source checks a reviewer also runs

The commands above do not cover these, and every gate so far has checked them from a brief rather than
from a document. Written down here so "the check as written" means one thing.

| Check | Permitted exception |
| --- | --- |
| No `todo!` or `unimplemented!` anywhere | none |
| No `unreachable!` outside `#[cfg(test)]` | none |
| No `unwrap` or `expect` outside `#[cfg(test)]` | the startup abort in `crates/aoko-app/src/main.rs`. Installing the bundled fonts and opening the first window cannot recover, and continuing with a silently substituted family would reflow every measurement in the app |
| Every `unsafe` block carries a `SAFETY:` comment naming the invariant it relies on | none, and the count is reported so a silent addition is visible |
| No `thread::sleep` in a test | `crates/aoko-platform/src/tcp.rs`, 20 ms, to force one message across two TCP reads. The failure mode is a weaker test, never a wrong assertion: if the writes coalesce, the reassembly path simply goes unexercised |
| No assertion on a duration the code chose — a schedule, a retry delay, a loop period | none: those are asserted against an injected clock, never against `Instant::elapsed`. A test may assert that a call **did not block**, provided the deadline is far enough in the past that a real sleep would be unmistakable and the ceiling is loose enough that a scheduler stall cannot trip it. `crates/aoko-core/src/clock.rs` is the only such test: a deadline one second in the past against a 250 ms ceiling |
| No lint suppression in a manifest (`[lints]`, `[workspace.lints]`, crate-level `allow`) | none. A dead declaration is removed, not hidden: a suppressed `unused_crate_dependencies` is an accurate signal |
| No colour literal outside `theme/` | none |
| A doc comment that describes behaviour must match the behaviour | none. This is the check that caught the switch rows described as "wired through `ModuleToggles`" while nothing wired them, and `card_title`'s description of a rule it does not implement |

An exception is not a pass: each one above is a named, justified deviation that the reviewer confirms is
still justified rather than silently waived. If a check needs a new exception, the exception is argued in
the review and added here in the same change, so the record cannot drift from the practice.

## 11. Non-goals for this migration

- Rewriting or re-deriving the C++ bridge.
- Shipping the Linux `.so` bridge.
- Changing any user-visible protocol field, key spelling, or default value.
- Deleting `Aoko/` or `Aoko.Tests/` before final sign-off.

## 12. Phase 2 UI decisions

Settled by the product owner before any Phase 2 code, because each one changes layout, capture
semantics or workspace dependencies. Neither builder re-opens these.

| # | Decision | Consequence |
| --- | --- | --- |
| 1 | **Bare `gpui` only.** No zed `ui`/`text` crates. | Every widget is hand-built against the pin. One theme system, no second settings/schema tree. The slider and the text field must be written from scratch either way: the pinned tree contains no slider widget and no text-input widget, since the only `TextInput` is the platform IME event type. |
| 2 | **Opaque card over a dark scrim.** Frosted glass is dropped. | The overlay stays readable without backdrop blur, which GPUI cannot do at the pin: `Background` offers solid and 2-stop linear only. |
| 3 | **Animations stay, gated by a client setting we own.** | Do not probe `ClientAreaAnimation`. Expose an in-app preference through `theme::motion` so one toggle governs every animated site on every platform. |
| 4 | **One window at control geometry, loader as an in-view state.** | No destroy-and-recreate transition, so window position survives and there is no flicker. The loader is a shell state, not a second window. Not built yet: only `LOADER_TITLE` and `LOADER_INJECT` exist, and nothing reads them. Decision 7 records it as debt, so this is a decision about where a loader will live when one exists, not a claim that it does. |
| 5 | **Client decorations we draw.** | The title bar is ours: themed background, drag region, and our own minimise/maximise/close hit-testing, plus resize edges. It is client area, so it appears in headless captures; that is the better visual-parity outcome and it must be captured. |
| 6 | **Bundle open font families** for UI and for monospace, shipped as assets. | Identical metrics on Windows and Linux. The drift versus the `Segoe UI` / `Consolas` baseline is accepted once rather than reflowing per platform. The families are **Liberation Sans** for UI and **Liberation Mono** for monospace — Open, and redistributable under SIL OFL 1.1, which is what makes bundling them in a GPL-3 binary lawful. The record of why, with the licence citation, is the module header at `crates/aoko-app/src/assets/mod.rs`; §3 of `docs/rust/UI_INVENTORY.md` lists the same two and defers to it. Neither builder re-picks them. |
| 7 | **Unmounted surfaces and inert controls are named debt, not delivered work.** | Two kinds, and they are not the same claim. *Built but unmounted:* the command-search overlay, both dialogs (colour picker, window picker) and pulse-on-jump exist and are unit-tested, but no route reaches them — nothing calls `open_search`, neither dialog type implements `Render`, the shell installs no key listener for search, and the overlay ignores the motion value handed to it. *Not built, and deliberately inert:* keybind capture (the 38 keybind buttons and the search rail button render with a click handler that does nothing, following `Control::Button`'s rule that a control stays inert rather than pretending) and the loader, of which only two unread copy constants exist and no state machine. The consequence is procedural: no builder may describe a surface as working without a test that reaches it from a window, and no doc comment may claim it is wired. Deliberate debt is auditable; a doc comment that says "wired" is not — and a register that lists more than exists is the same defect in the other direction. |

The existing `gui-*.png` artifacts are **not** parity evidence: `scripts/New-GuiScreenshots.ps1`
misnamed two of them and never captured Stats or Settings (section 8 item 9).
