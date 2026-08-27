---
title: AutoHeal
description: Automatically throws a healing splash potion when your health drops below a configurable threshold.
---

*AutoHeal* watches your health and, the moment it drops to or below your configured threshold, swaps to the first healing splash potion in your hotbar, throws it, and restores your original held slot. It uses Auto Rod's transaction pipeline, so the held-item change is emitted by vanilla code before the throw.

Two safety rules decide when a throw is allowed: you must already be **looking steeply down** (about 75°), so the splash lands at your own feet instead of flying toward an opponent, and only **healing** splash potions (Instant Health / Regeneration) count — speed, strength, and other utility pots are ignored, and drinkable potions are never selected because they cannot be thrown.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Health threshold | Throw as soon as health is at or below this value (half-hearts). | `1`–`20` / `17` |
| Re-throw cooldown | Minimum time between automatic throws. | `50`–`2000` ms / `500` |

## Usage notes

- The module only acts while you are in a world, have no GUI open, and the Minecraft window is focused.
- **Look down to heal**: if you are not looking at least ~75° downward, the throw is held back until you do — your camera is never moved for you.
- If no healing splash potion is in the hotbar, nothing happens until one is available.
- AutoHeal never fights other hotbar modules: while Auto Rod or Throwpot owns the held slot, healing waits; AutoTool pauses while a heal is in flight.
- The threshold counts raw health only — absorption hearts are not included.

:::caution
Automatic combat responses are among the most flagged behaviors on anti-cheat servers. Keep the threshold conservative and the cooldown high enough to look deliberate.
:::
