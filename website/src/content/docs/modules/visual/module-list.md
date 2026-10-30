---
title: Module List & Watermark
description: The in-game active-module list and the live client watermark.
---

aoko draws an in-game **module list** of your active modules under a live **watermark** with local latency and FPS. Both are styled to match your chosen GUI theme.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Settings

| Setting | Description | Options / Default |
| ------- | ----------- | ----------------- |
| Show module list | Toggles the active-module list overlay. | On |
| Module list style | Visual style of the list entries. | Default · Minimal · Outlined · Glass · Bold / `Default` |
| Show watermark | Toggles the segment above the list with latency, the aoko mark, and FPS. | On |

## Usage notes

- The module list shows which modules are currently active and updates live as you toggle them.
- The watermark shows tab-list latency and the current ImGui frame rate. It is tied to the module list visibility.
- Choose a **style** that matches your overlay preference, such as `Default`, `Glass`, or `Bold`.
- The module list position can be moved with the [HUD Editor](/aoko-client/modules/visual/hud-editor/).

:::tip
Both the module list and watermark are disabled automatically by [Panic](/aoko-client/panic/) for a clean screen.
:::
