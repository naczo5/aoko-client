# AGENTS.md

## Purpose

Operating guide for coding agents working in `aoko-client` (`legoclickerC`). Build/test/lint commands, conventions, safety rules, and gotchas.

## Reference Priority

1. `.agents/skills/` — canonical procedural skills and architecture references:
   - `packet-and-call-hygiene`: Exclusive resources (hotbar, use, attack, inventory) for AutoRod, AutoTool, KillAura, and other mutating modules — not combat-only.
   - `java-to-jni-port`: Translate in-JVM Java (event buses, Mixins, mapping wrappers) into JNI/C#; unwrap to vanilla APIs.
   - `add-client-module`: `ModuleCatalog` first, then UI / Profile / TCP / string-list capabilities / bridges.
   - `diagnose-bridge-crash`: Root-cause runbook for JVM `hs_err` crash dumps and bridge debug logs.
   - `release-verification`: PR compile gate vs `New-GitHubRelease.ps1` on `main`.
   - `write-docs`: README, website/docs site, AGENTS.md, and skills stay aligned with current modules; silent on removals.
   - `karpathy-guidelines`: Behavioral guidelines for simpler, surgical code changes.
2. `README.md` — user-facing feature list and quick start.

## Repository Overview

- `Aoko/`: .NET 8 WPF loader + external GUI (publishes as `Aoko.exe`).
- `Aoko/Core/`: clicker engine, input hooks, profile persistence, TCP client, GTB solver.
- `McInjector/`: native bridge DLLs (`bridge.dll` for 1.8.9, `bridge_261.dll` for 26.1 / 26.2 / 1.21.x).
- `McInjector/src/main/cpp/`: JNI/Win32/OpenGL/Vulkan/ImGui/MinHook bridge sources.
- `McInjector/src/main/java/`: Unused/obsolete Java agent code (ignore). Bridges perform all JNI, rendering, and TCP duties themselves.

## Required Toolchain

- Windows 10/11 x64.
- .NET SDK 8.x.
- MinGW-w64 at `C:\mingw64\mingw64\bin\g++.exe`.
- JDK 17 headers at `C:\Program Files\Java\jdk-17\include`.

## Build Commands

Run from repository root. Prefer PowerShell for compound commands.

### Native bridge builds
- Build both bridges: `build_dll.bat`
- Build 1.8.9 bridge only: `McInjector\build.bat`
- Build 26.1 bridge only: `McInjector\build_261.bat`

Bridge build scripts auto-copy output to `Aoko\bin\Debug\`, `Release\`, and `publish\` folders. The csproj also preserves newest bridge DLLs from project root.

### C# loader builds
- Debug build: `dotnet build Aoko\Aoko.csproj`
- Release build: `dotnet build -c Release Aoko\Aoko.csproj`
- Publish (self-contained single-file): `build_exe.bat`
- Full local package (bridges + publish): `build_release.bat` (invoked by the GitHub release script; do not run it extra when publishing)
- GitHub release: `.\scripts\New-GitHubRelease.ps1 -Version 0.x.y` from clean `main` (see `release-verification` skill)

### Run locally
- Run app: `dotnet run --project Aoko\Aoko.csproj` (ensure existing `Aoko.exe` is closed).

## Lint and Testing

- C# compile gate: `dotnet build Aoko\Aoko.csproj`
- Native compile gate: `McInjector\build_261.bat` and/or `McInjector\build.bat`
- Run all C# tests: `dotnet test Aoko.Tests\Aoko.Tests.csproj`
- Run native test harness: `McInjector\run_tests.bat`

## Debugging

- Bridge debug logs:
  - `bridge_debug.log` (legacy 1.8.9 bridge)
  - `bridge_261_debug.log` (modern 26.1 / 1.21 bridge)
- C# logging goes to `Debug.WriteLine`.
- For JVM crash dumps, check `hs_err_pid*.log` in Lunar's working directory, `%USERPROFILE%\.lunarclient\`, or `%TEMP%`.

## Architecture & Domain Safety Rules

- Loader and bridge communicate over TCP on port `25590`.
- **Safety Rule:** Keep bridge code read-first. Do NOT add raw packet spam or unrelated gameplay mutation.
- Input simulation normally happens in C# via Win32 `SendInput`. Controlled bridge-side JNI/game interaction is allowed only when explicitly owned by a module (consult `packet-and-call-hygiene` skill before touching combat/packet methods).
- `bridge_261.cpp` uses Yarn-first, Mojmap-fallback arrays for dual-version 1.21 / 26.1 support.
- **Renderer auto-detect (`bridge_261.dll`):** OpenGL (`wglSwapBuffers`) or Vulkan (`render_backend.cpp`). Whichever present path fires first claims the session (`RenderBackend_GetActiveKind()`). Kill-switch: `AOKO_BRIDGE261_VULKAN=0`.
- Preserve menu-injection compatibility (1.8.9): recover mappings when entering a world after injecting in title screen/lobby.

## Configuration Sync

When adding or modifying a module or setting, start in `Aoko/Core/ModuleCatalog.cs`, then update every surface that entry requires (see `add-client-module` skill). Typical layers:
1. `ModuleCatalog` registration (id, surfaces, overlay probe, DevOnly/ManagedOnly)
2. `Clicker` property in `Clicker.cs`
3. `Profile` save/load mapping in `Profile.cs`
4. `GameStateClient` TCP **config** payload (C# → bridge)
5. `BridgeCapabilities.cs` and `bridge_capabilities.h` **string lists** (not bitflags) if gating changes
6. `InputHooks` and the keybind button on the module card if the module is keybindable
7. Bridge `ParseConfig` / logic / overlay `pushMod` in `bridge.cpp` / `bridge_261.cpp`
8. Inbound JVM fields: bridge JSON → `GameState.cs` plus the capabilities `state` list

Hotbar, use-item, attack, or inventory modules must follow `packet-and-call-hygiene` (AutoRod/AutoTool exclusive slot lockouts). Overlay drawing stays draw-only unless the module owns mutation.

## Coding Style

- Match the touched file; do not reformat unrelated code. Keep changes minimal.
- **C#:** file-scoped namespaces, 4-space indent, `PascalCase` public API, `_camelCase` private fields. Bindable state raises `PropertyChanged`. `CancellationToken` for loops. Marshal UI updates to the dispatcher. Clamp TCP-driven values.
- **XAML:** `DynamicResource` theming, explicit `TwoWay` bindings.
- **C++:** C++11, existing include order, `Mutex`/`LockGuard` for shared globals. Keep the swap-buffer/present hook light; cache JNI IDs. Check nulls, clear JNI exceptions, manage local refs in loops.

## High-Signal Gotchas

- `JNIEnv*` is thread-local. Never use across threads. Always call `AttachCurrentThread` from non-render worker threads.
- Module JNI cadences are independent wall-clock intervals in `telemetry_schedule.h`. Never `Sleep(moduleA || moduleB ? 5 : 50)` and never tick modules from `wglSwapBuffers` / present. Enabling SpeedBridge must not accelerate overlay scans or `ReadGameState`.
- `bridge_261.cpp` fallback-array parsing: Yarn names MUST be checked first, then Mojmap.
- `build_release.bat` already runs `build_dll.bat` and copies DLLs from `McInjector\`. Everyday PRs do not need it. Publishing a GitHub release uses `scripts\New-GitHubRelease.ps1`, which calls `build_release.bat` itself.

## Git & Branch Policy

- **`dev` is the working branch.** Agents may stage, commit, create/switch local branches, and merge into `dev`.
- **`main` is protected.** Do NOT commit, merge, rebase, push, or force-push to `main` without explicit user permission.
- Always confirm the current branch (`git rev-parse --abbrev-ref HEAD`) before committing.
- Destructive git operations (`reset --hard`, `clean -f`, `branch -D`, `push --force`) require explicit permission on ANY branch.
- `.gitattributes` enforces LF repo / CRLF checkout (`* text=auto eol=crlf`). Use `git add --renormalize .` if diff churns line endings.

## Agent Workflow

- Run relevant build command(s) and test suites before finishing.
- Report exact commands run and results.
- Do not revert unrelated user changes.
