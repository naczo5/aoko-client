---
title: Refill
description: Moves healing items from your backpack into empty hotbar slots while your inventory is open.
---

*Refill* keeps your hotbar stocked with healing items. When you open your survival inventory, it shift-clicks healing items from the backpack area into empty hotbar slots, one item at a time with a configurable delay.

Only actual healing counts: potions are identified by their effects, and only **Instant Health** and **Regeneration** potions are picked up (speed, strength, and other utility potions are ignored). Golden apples are always refilled.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Delay | Delay between slot interactions (milliseconds). | `50`–`500` / `120` |

## Usage notes

- The bridge reports which backpack slots hold healing items (Instant Health / Regeneration potions and golden apples) and whether the hotbar has a free slot; the loader drives the physical cursor over those slots.
- Only **empty** hotbar slots are filled — Refill never swaps out items you are holding.
- Because it uses the real cursor, keep the Minecraft window focused and the inventory open while it runs.
- A higher **Delay** is slower but looks more human and is more reliable on laggy servers; a lower delay is faster but more obvious.

:::caution
Rapid, perfectly-timed slot clicks are easy to spot. Increase the **Delay** on servers that monitor inventory actions.
:::
