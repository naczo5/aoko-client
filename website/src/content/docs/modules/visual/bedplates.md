---
title: BedPlates
description: Highlights player beds with in-world bounding boxes and distance indicators.
---

*BedPlates* renders in-world ESP overlays and bounding box plates directly on player beds, making bed defense and offense awareness seamless in Bedwars and related minigames.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Enable BedPlates | Toggles the BedPlates ESP overlay. | Toggle / `Off` |
| Show distance | Displays the distance in blocks above each detected bed. | Toggle / `On` |
| Scan range (chunks) | Chunk radius around the player to scan for beds. | `1`–`8` chunks / `4` chunks |

## Usage notes

- Scans loaded chunk sections for Bed blocks and combines bed head/foot pairs into a single clean plate.
- Works through walls and terrain, rendering cleanly using native OpenGL/Vulkan overlay backends.
- Keybindable directly from the module card in the external GUI.
