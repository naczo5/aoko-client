---
title: Velocity
description: Reduces the knockback you take, on separate horizontal and vertical axes.
---

*Velocity* reduces the knockback applied to you when hit, with independent control over the horizontal and vertical components.

## Version support

1.8.9 · 1.21.x · 26.1

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Horizontal | Percentage of horizontal knockback retained (`100` = normal, lower = less pushback). | `1`–`100` / `100` |
| Vertical | Percentage of vertical knockback retained (`100` = normal, lower = less liftoff). | `1`–`100` / `100` |
| Chance | Probability (%) that the reduction is applied to a given knockback event. | `1`–`100` / `100` |

## Usage notes

- Set **Horizontal**/**Vertical** below 100 to take less knockback on that axis; both at 100 leaves knockback unchanged.
- **Chance** applies the reduction to only a fraction of hits, which looks less consistent than full cancellation.

:::danger
Knockback reduction is detectable. Partial values combined with a lower **Chance** are far less obvious than fully cancelling knockback.
:::
