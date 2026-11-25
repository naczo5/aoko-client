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
aoko-protocol pure data: wire types, profile, catalog, solver. zero deps.
```

| Crate | Owns | Ported from |
| --- | --- | --- |
| `aoko-protocol` | Wire types, capability negotiation, bounded framing, game state, patch merging, profile + palette + HUD layout persistence, module catalog, stats, GTB solver, transport metrics, TCP port selection | `GameState.cs`, `GameStatePatchMerger.cs`, `BoundedLineReader.cs`, `BridgeCapabilities.cs`, `ManagedTransportDiagnostics.cs`, `TcpPortHelper.cs`, `GameStateClient.cs` (transport seam), `HudLayout.cs`, `KillAuraSettings.cs`, `NickHiderConfig.cs`, `BlockEsp.cs`, `ModuleCatalog.cs`, `Profile.cs`, `CustomPalette.cs`, `StatsTracker.cs`, `GtbWordSolver.cs`, `BridgeCommandProtocol.cs`, `BridgeCommandResult.cs`, `Accounts/*` pure parsers, `Servers/ServerAddress.cs` |
| `aoko-platform` | `ProcessInspector`, `WindowInspector`, `InputBackend`, `NativeBridgeInjector`, `DiscordRpcTransport`, `PlatformCapabilities` | `ProcessCommandLine.cs`, `InjectionTargetDiscovery.cs`, `WindowDetection.cs`, `InputHooks.cs`, `NativeInjector.cs`, `DiscordRichPresenceService.cs` (transport half), `Servers/ServerPing.cs` (socket half) |
| `aoko-core` | `clicker/`, `aim_assist`, `chest_stealer`, `refill`, `auto_rod`, `auto_tool`, `concurrency/`, `services/` | `Clicker.cs`, `AdvancedClickTimingState.cs`, `ClickDeadlineScheduler.cs`, `ClickRandomizationMode.cs`, `AimAssistMotionController.cs`, `ChestStealerController.cs`, `ChestStealerCoordinateMapper.cs`, `RefillController.cs`, `CoalescedCallback.cs`, `CoalescedLatestValue.cs`, `OwnedAsyncLoop.cs`, `UpdateService.cs`, `DiscordRichPresenceService.cs` (state half), `Accounts/*` services, `Servers/*Store/Join*`, `ModuleSearchIndex.cs` |
| `aoko-app` | `theme/`, `assets/`, `components/`, `views/`, `dialogs/`, `capture/`, `main.rs` | `App.xaml`, `MainWindow.xaml(.cs)`, `ColorPickerDialog.*`, `WindowPickerDialog.*`, `ThemeManager.cs`, `Views/*` |

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

## 8. Documented trade-offs

Recorded here rather than hidden.

1. **Linux native bridge is a trait stub, by design.** `NativeBridgeInjector` reports unsupported. Data arrives only if a future `.so` bridge exists. The UI must therefore treat "no game state" as a normal state, not an error.
2. **Wayland window inspection is compositor-specific.** Hyprland/Sway expose IPC that makes it tractable; GNOME/KDE do not offer an equivalent portable path. Capability flags carry this, and XWayland windows are handled through the X11 path.
3. **`Aoko/` and `Aoko.Tests/` stay in the tree through every phase.** This costs repo size and means two implementations coexist. It also means every golden fixture is diffable against real C# behaviour, which is worth more than tidiness until sign-off.
4. **`Aoko/wndproc_new.cpp` is dead code.** 3.8 KB, absent from `Aoko.csproj`, referenced by no source file, script, or doc in the repo. It is not ported and not treated as a contract.
5. **Split of `GameStateClient`** trades a one-to-one file mapping for layer purity. Tracked in `MIGRATION_PARITY.md` so the mapping stays auditable.
6. **`aoko-platform` depends on `aoko-protocol`** for shared types, so "platform" is not importable by the protocol crate. This is intentional: the wire format must not learn OS concepts.

## 9. Verification commands

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo bench --workspace
cargo run -p aoko-app -- --capture-screenshots <out_dir>
```

CI runs the Linux and Windows matrices; `McInjector` DLL pipelines are untouched.

## 10. Non-goals for this migration

- Rewriting or re-deriving the C++ bridge.
- Shipping the Linux `.so` bridge.
- Changing any user-visible protocol field, key spelling, or default value.
- Deleting `Aoko/` or `Aoko.Tests/` before final sign-off.
