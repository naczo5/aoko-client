---
title: Chest Stealer
description: Quickly transfers items out of an open chest using the external cursor.
---

*Chest Stealer* empties an open chest/container by moving the external cursor over each slot and shift-clicking, with a configurable per-slot delay.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Delay | Delay between slot interactions (milliseconds). | `50`–`500` / `120` |
| Title check | Prevents stealing if the container title matches known menu keywords (e.g. shop, selector, lobby). | Toggle / `On` |
| Custom item check | Prevents stealing if over 50% of container items have custom display names (common in server GUI menus). | Toggle / `On` |
| Physical chest check | Requires an actual physical chest block near the player (within 6.5 blocks), preventing interaction with virtual or remote server GUIs. | Toggle / `On` |

## Usage notes

- The bridge reports the open container's slot layout and screen geometry; the loader drives the physical cursor over those slots.
- The three safety checks can be toggled independently:
  - **Title check** filters out GUIs whose title contains common shop or lobby keywords.
  - **Custom item check** detects GUIs using renamed items as clickable icons or buttons.
  - **Physical chest check** verifies that an actual chest block exists within 6.5 blocks. Disable this if playing on servers that open virtual storage via commands.
- A higher **Delay** is slower but looks more human and is more reliable on laggy servers; a lower delay is faster but more obvious.
- Because it uses the real cursor, keep the Minecraft window focused and the chest GUI open while it runs.

:::caution
Rapid, perfectly-timed slot clicks are easy to spot. Increase the **Delay** on servers that monitor inventory actions.
:::
