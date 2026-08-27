---
title: Throwpot
description: Instantly throws the first healing splash potion in your hotbar via an action bind.
---

*Throwpot* is a one-shot action: press the action bind and the client swaps to the first healing splash potion in your hotbar, throws it, then restores your original held slot. It uses Auto Rod's transaction pipeline, so the held-item change is emitted by vanilla code before the throw.

Only **healing** splash potions (Instant Health / Regeneration) are thrown — speed, strength, and other utility pots are ignored, and drinkable potions are never selected because they cannot be thrown.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Settings

| Setting | Description | Default |
| ------- | ----------- | ------- |
| Enable Throwpot | Arms the module; the action bind only fires while enabled. | Off |
| Action bind | The dedicated key or mouse button that triggers a throw. | Unbound |

## Usage notes

- The action fires once per press while enabled, connected, in-game, and no GUI is open.
- The bridge writes the hotbar slot, waits two ticks so the server sees the held-item change, then issues a physical right-click — the same safe sequence as Auto Rod.
- Throwpot and Auto Rod never run at the same time: whichever module owns the held slot wins, and presses made during the other's transaction are dropped instead of firing late.
- If no healing splash potion is in the hotbar, nothing happens.

:::caution
Splash potions are visible throws. On servers that monitor combat automation, keep the potion count you carry believable for the situation.
:::
