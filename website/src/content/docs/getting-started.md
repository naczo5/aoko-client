---
title: Getting started
description: What aoko client is, supported versions, and how to inject it into Lunar Client.
---

aoko client is a free, open-source, injectable utility client for **Lunar Client** on Windows. It is made up of two parts:

- a **C# / .NET 8 WPF loader** that finds Lunar's `javaw.exe` and injects a native bridge, and hosts the external GUI, profiles, keybinds, and Discord Rich Presence; and
- a **C++ JNI bridge** that attaches to the JVM, reads game state, and renders ImGui overlays through OpenGL.

The loader and bridge talk to each other over TCP on port `25590`.

## Supported versions

aoko targets three Lunar Client versions through a single external GUI:

| Version  | Bridge        | Status      |
| -------- | ------------- | ----------- |
| 1.8.9    | `bridge.dll` (legacy) | Supported |
| 1.21.x   | `bridge_261.dll`      | Supported |
| 26.1     | `bridge_261.dll`      | Supported |

Some modules are version-gated — for example, **Triggerbot** and **AutoTotem** are only available on 1.21.x / 26.1. Each module page lists its version support.

## Requirements

- Windows 10/11 x64
- Lunar Client (1.8.9, 1.21.x, or 26.1)
- The release `Aoko.exe` is self-contained — no .NET install needed to run it.
- .NET 8 SDK + MinGW-w64 + JDK 17 headers are only needed to build from source.

## Quick start

1. Launch Lunar Client and join a server or world.
2. Run `Aoko.exe`.
3. Click **Inject**.
4. Use the external GUI to enable and configure modules.
5. Bind keys under the **Keybinds** tab to toggle modules in-game.

:::tip
It is generally recommended to inject while already in a server/world so that every module initializes correctly.
:::

:::caution
Use at your own risk. aoko is not affiliated with Mojang, Microsoft, or Lunar Client, and is provided for educational purposes only.
:::
