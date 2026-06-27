---
title: Reach
description: Extends your attack range with a configurable distance and hit chance.
---

*Reach* extends the distance from which you can attack entities.

## Version support

1.8.9 · 1.21.x · 26.1

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Min distance | Lower bound of the randomized attack distance (blocks). | `3.0`–`6.0` / `3.0` |
| Max distance | Upper bound of the randomized attack distance (blocks). | `3.0`–`6.0` / `3.0` |
| Chance | Probability (%) that reach is applied to a given hit. | `0`–`100` / `100` |

## Usage notes

- The effective reach for each hit is randomized between **Min** and **Max** distance.
- Leaving Min and Max equal gives a fixed reach; widening the gap randomizes per-hit distance.
- **Chance** lets only a fraction of hits use extended reach, reducing flags on servers that watch for it.

:::danger
Reach is detected on almost all anti-cheats. Use small distances (e.g. just above `3.0`) and a lower **Chance** on watched servers, or reserve it for private/anarchy use.
:::
