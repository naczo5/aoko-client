---
title: Triggerbot
description: Attacks automatically when an entity is under your crosshair and the attack cooldown is ready.
---

*Triggerbot* automatically left-clicks when an entity is under your crosshair and the attack cooldown has recovered. It uses the bridge's attack-cooldown state to time hits and adds randomized reaction delays.

## Version support

1.21.x · 26.1 — **not available on 1.8.9**.

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Only on crosshair | Only fire when an entity is actually under the crosshair. | On |
| Only if can attack | Only fire when a valid, attackable target is in range. | On |
| Cooldown threshold | Minimum attack-cooldown recovery (%) required before firing — higher means it waits for a fuller charge. | `1`–`100` / `92` |
| Hit chance | Probability (%) that any given ready opportunity actually fires. | `1`–`100` / `100` |
| Require click | Only operate while you are physically holding the left mouse button. | On |

## Usage notes

- Triggerbot pauses while a GUI is open with the cursor visible, while breaking a block, and when the bridge state is stale.
- Timing combines the server-reported cooldown with a locally predicted cooldown, plus a randomized human-like reaction delay before each click.
- **Hit chance** below 100% randomly skips some opportunities for a less robotic pattern.

:::caution
Auto-attacking on cooldown is detectable. Keep **Require click** on so it only acts while you are genuinely fighting, and consider lowering **Hit chance**.
:::
