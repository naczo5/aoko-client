---
title: PixelParty Assist
description: Helps reach the correct block in the PixelParty minigame via auto-look and auto-walk.
---

*PixelParty Assist* helps you reach the correct safe block during the PixelParty minigame. The bridge locates the target block and reports the yaw delta and distance; the loader can then steer your view and walk you toward it.

## Version support

1.8.9 · 1.21.x · 26.1

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Scan radius | How far (in blocks) the bridge searches for the target block. | `8`–`48` / `28` |
| Auto look | Steers your view toward the target block using mouse movement. | Off |
| Auto walk | Walks (and jumps when needed) toward the target block. | Off |

## Usage notes

- **Auto look** snaps your yaw toward the target; **Auto walk** drives the `W`/jump keys and uses mouse-only steering to stay aligned.
- When the target is briefly lost (for example mid-jump), the assist coasts on the last known position for a short grace window so movement doesn't stutter.
- Walking stops automatically once you are close enough to the target block, switching to fine alignment.
- Both helpers pause when a GUI is open with the cursor visible or when the Minecraft window isn't focused.

:::tip
Enable **Auto look** alone first to get a feel for the steering before turning on **Auto walk**.
:::
