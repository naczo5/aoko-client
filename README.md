# LegoClicker

LegoClicker is a Windows utility client for Lunar Client.

# Showcase
[![Watch the showcase video](docs/screenshots/thumbnail.png)](https://www.youtube.com/watch?v=eR7QKAWw8D4)

## Current status

- Supported versions: **26.1**, **1.21.x**, and **1.8.9**.
- All supported versions are used through the external GUI in `LegoClickerCS`.

## Features (current)

- Autoclicker (left/right, CPS range, jitter, block-only options)
- Aim Assist
- Triggerbot
- SpeedBridge (edge sneak assist with safety gates)
- Reach and Velocity controls
- AutoTotem (fall/explosion detection, Ghost and Anarchy modes)
- GTB Helper
- Discord Rich Presence
- Nametags, Closest Player panel, Chest ESP
- Per-module keybinds (all unbound by default)
- Profiles saved in `%AppData%\LegoClicker\profiles\`
- GUI customization (palette, module list style, show logo)

## Screenshots

![GUI Showcase GIF](screenshots/gui.gif)

![Gameplay HUD](screenshots/gameplay.png)

## Requirements

- Windows 10/11 x64
- Lunar Client
- .NET 8 SDK (build only)
- MinGW-w64 + JDK 17 headers (native build only)

## Quick start

1. Start Lunar Client.
2. Run `LegoClicker.exe`.
3. Click **Inject**.
4. Use the external GUI (bind keys under the Keybinds tab).

1.8.9 supports menu/lobby injection. Some JNI mappings are completed lazily after a world is joined, so modules may become available once the bridge sees in-world state.

## Build

Run from repository root unless noted.

### Native bridge DLLs

- Build both: `build_dll.bat`
- Build 26.1 only: `McInjector\build_261.bat`
- Build 1.8.9 only: `McInjector\build.bat`

### Loader (C#)

- Debug build: `dotnet build LegoClickerCS\LegoClickerCS.csproj`
- Release build: `dotnet build -c Release LegoClickerCS\LegoClickerCS.csproj`
- Run: `dotnet run --project LegoClickerCS\LegoClickerCS.csproj`
- Publish release exe: `build_exe.bat`

### Full release pipeline

- `build_release.bat`

## Tests

- Run C# tests: `dotnet test LegoClickerCS.Tests\LegoClickerCS.Tests.csproj`
- Run native harness tests: `McInjector\run_tests.bat`

## Notes on versions

- `bridge_261.dll` is the modern bridge used for both 26.1 and 1.21 injection.
- `bridge.dll` (1.8.9) is supported and now builds with the shared ImGui/OpenGL backend.
- Supported runtime bridges are configured through the external GUI.
- Discord Rich Presence is configured from the external GUI under the Utility tab.

## Project structure

```text
legoclickerC/
|- LegoClickerCS/              # WPF loader + external GUI (.NET 8)
|  |- Core/                    # Clicker, hooks, profile, TCP client
|  |- MainWindow.xaml(.cs)     # Main UI
|  |- bridge.dll               # 1.8.9 bridge (legacy)
|  `- bridge_261.dll           # 26.1 bridge
|- McInjector/
|  |- build.bat                # 1.8.9 bridge build (legacy)
|  |- build_261.bat            # 26.1 bridge build
|  `- src/main/cpp/            # Native bridge sources
|- docs/                       # Website
`- README.md
```

## Architecture (short)

- `LegoClickerCS` injects bridge DLL into Lunar and manages settings/UI.
- Bridge and loader communicate over TCP (`25590`).
- Bridge renders overlays through OpenGL/ImGui and reads game state via JNI.
- Input actions are sent through Win32 `SendInput`.
- Bridge capabilities gate version-specific modules and controls.

## Safety constraint used by this project

- Bridge-side logic is read-first. Limited ghost-safe JNI writes exist for modules such as Reach, Velocity, and nametag suppression.
- Do not add direct packet sending or in-game combat method calls.

## TODO

- Add Linux support
