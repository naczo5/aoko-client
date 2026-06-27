---
title: Chest Stealer
description: Quickly transfers items out of an open chest using the external cursor.
---

*Chest Stealer* empties an open chest/container by moving the external cursor over each slot and shift-clicking, with a configurable per-slot delay.

## Version support

1.8.9 · 1.21.x · 26.1

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Delay | Delay between slot interactions (milliseconds). | `50`–`500` / `120` |

## Usage notes

- The bridge reports the open container's slot layout and screen geometry; the loader drives the physical cursor over those slots.
- A higher **Delay** is slower but looks more human and is more reliable on laggy servers; a lower delay is faster but more obvious.
- Because it uses the real cursor, keep the Minecraft window focused and the chest GUI open while it runs.

:::caution
Rapid, perfectly-timed slot clicks are easy to spot. Increase the **Delay** on servers that monitor inventory actions.
:::
