---
title: Autoclicker
description: Randomized left/right autoclicker with jitter and GUI/chest safety.
---

The *Autoclicker* fires the left and/or right mouse button at a randomized rate while the Minecraft window is focused. It includes GUI- and chest-aware safety checks so it pauses in menus and inventories.

## Version support

1.8.9 · 1.21.x · 26.1

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Left click | Enables the left-button autoclicker (requires the module to be **Armed**). | On |
| Min / Max CPS (left) | Randomized clicks-per-second window for the left button. | `1`–`25` / `8`–`12` |
| Right click | Enables the right-button autoclicker. Runs independently of the armed/left state. | Off |
| Min / Max CPS (right) | Randomized CPS window for the right button. | `1`–`25` / `10`–`14` |
| Jitter | Uses a Gaussian distribution around the mid CPS for a more human click pattern. | On |
| Right only when holding block | Right-click only fires while you are holding a placeable block. | Off |
| Break blocks | Treats held left-click as mining intent and pauses clicking while breaking a block. | Off |
| Click in chests | Allows clicking inside chest/container GUIs (otherwise paused there). | Off |

## Usage notes

- The left autoclicker only runs when the module is **Armed**; the right autoclicker is independent so it can be used on its own.
- Clicking automatically pauses in inventories, crafting/anvil/furnace screens, and (unless **Click in chests** is on) chest GUIs. It also pauses when the system cursor is visible.
- CPS is drift-compensated each cycle so the real click rate stays close to the target.

:::caution
A randomized autoclicker is the intended use case; very high or perfectly consistent CPS is easy for anti-cheats to flag. Keep the Min/Max window realistic and leave **Jitter** on.
:::
