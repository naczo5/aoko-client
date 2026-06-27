---
title: SpeedBridge
description: Edge-sneak bridging assist with timing and condition gates.
---

*SpeedBridge* assists with bridging by automating the sneak-at-the-edge technique, gated by conditions so it only engages when you are actually bridging.

## Version support

1.8.9 · 1.21.x · 26.1

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Block only | Only engage while you are holding a placeable block. | On |
| Delay | Timing of the sneak assist (milliseconds). | `20`–`250` / `85` |
| Holding sneak only | Only engage while you are holding the sneak key. | On |
| Looking down only | Only engage while you are looking downward (as you do when bridging). | On |

## Usage notes

- The condition gates (**Block only**, **Holding sneak only**, **Looking down only**) keep the assist from triggering during normal movement.
- **Delay** tunes the sneak timing — adjust it to match your bridging speed and the server's tick behavior.

:::tip
The default gates are conservative on purpose. If the assist feels like it isn't engaging, relax one gate at a time rather than disabling all of them.
:::
