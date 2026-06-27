---
title: Aim Assist
description: Subtle mouse-movement aim assistance toward the closest target.
---

*Aim Assist* nudges your aim toward the closest entity inside a configurable field of view while you are attacking. It applies small `SendInput` mouse movements rather than snapping, so motion stays smooth.

## Version support

1.8.9 · 1.21.x · 26.1

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| FOV | Half-angle cone (in degrees) around your crosshair within which a target is considered. | `1`–`180` / `30` |
| Range | Maximum distance (in blocks) to a candidate target. | `1`–`12` / `4.5` |
| Strength | How aggressively your aim is pulled toward the target. | `1`–`100` / `40` |

## Usage notes

- Assistance only applies while you are holding the left mouse button **or** the left autoclicker is actively clicking.
- The target is the closest on-screen entity that falls within both the FOV cone and the range.
- A light temporal filter is applied to suppress jitter/orbiting around the target point.
- Movement strength and step size are tuned separately for the 1.8.9 bridge versus the modern bridge to account for different FOV/projection handling.

:::caution
Aim assistance is detectable. Lower **Strength** and a tighter **FOV** look more legitimate than aggressive values.
:::
