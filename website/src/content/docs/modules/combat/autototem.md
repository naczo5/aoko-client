---
title: AutoTotem
description: Keeps a Totem of Undying equipped, with inventory-only and anarchy modes.
---

*AutoTotem* keeps a Totem of Undying ready in your off-hand. It offers a safe inventory-only mode and a more aggressive anarchy mode.

## Version support

1.21.x · 26.1 — **not available on 1.8.9**.

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Behavior mode | `Ghost` (inventory only) re-equips from inventory; `Anarchy` is the more aggressive replacement mode. | Ghost / Anarchy / `Ghost` |
| Mode | Secondary mode selector for totem handling. | `0`–`1` / `0` |
| Health | Health threshold (in half-hearts) that triggers re-equipping. | `0`–`36` / `10` |
| Elytra | Allows swapping around an equipped Elytra. | On |
| Delay | Added delay (ticks) before re-equipping. | `0`–`20` / `0` |

## Usage notes

- **Ghost (inventory only)** keeps a totem in the off-hand by pulling from your inventory and avoids riskier interactions — the recommended default.
- **Anarchy** mode is intended for anarchy/private servers where more aggressive totem replacement is acceptable.
- The **Health** threshold controls how low you must drop before a re-equip is attempted; **Delay** adds a small human-like pause.

:::caution
Anarchy mode performs more aggressive inventory interactions. Prefer **Ghost** mode on normal servers.
:::
