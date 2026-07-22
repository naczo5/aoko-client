---
title: Getting started
description: What aoko client is, supported versions, and how to inject it into Lunar Client.
---

aoko client is a free, open-source, injectable utility client for **Lunar Client** and **standalone Minecraft instances** on Windows. It is made up of two parts:

- a **C# / .NET 8 WPF loader** that detects Java (`javaw.exe`), injects a native bridge, and hosts the external GUI, profiles, keybinds, and Discord Rich Presence; and
- a **C++ JNI bridge** that attaches to the JVM, reads game state, and renders ImGui overlays natively through OpenGL or Vulkan.

The loader and bridge talk to each other over TCP on port `25590`.

## Supported versions & environments

aoko supports both **Lunar Client** and **standalone Minecraft instances** across four major version branches:

| Version | Bridge | Submodules / Environment | Status |
| :--- | :--- | :--- | :--- |
| **1.8.9** | `bridge.dll` (legacy) | Standalone Forge, OptiFine & Lunar | Supported |
| **1.21.x** | `bridge_261.dll` | Standalone Fabric (Yarn Mappings) & Lunar | Supported |
| **26.1** | `bridge_261.dll` | Standalone Fabric (Mojmap) & Lunar | Supported |
| **26.2** | `bridge_261.dll` | Standalone Fabric (OpenGL & Vulkan auto-detect) & Lunar | Supported |

Some modules are version-gated — for example, **Triggerbot**, **AutoTotem**, and **Vulkan overlay rendering** are available on modern versions (1.21.x / 26.1 / 26.2). Each module page details its version support.

## Requirements

- Windows 10/11 x64
- Lunar Client or a standalone Minecraft client (1.8.9 Forge, modern Fabric)
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
