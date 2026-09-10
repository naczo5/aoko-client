---
title: Autoclicker
description: Randomized left/right autoclicker with 3-way randomization (Basic, Medium, Advanced) and GUI/chest safety.
---

The *Autoclicker* fires the left and/or right mouse button at a randomized rate while the Minecraft window is focused. It includes GUI- and chest-aware safety checks so it pauses in menus and inventories.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Left click | Enables the left-button autoclicker (requires the module to be **Armed**). | On |
| Min / Max CPS (left) | Randomized clicks-per-second window for the left button. | `1`–`25` / `8`–`12` |
| Right click | Enables the right-button autoclicker. Runs independently of the armed/left state. | Off |
| Min / Max CPS (right) | Randomized CPS window for the right button. | `1`–`25` / `10`–`14` |
| Randomization | Randomization mode: **Basic** (uniform distribution), **Medium** (Gaussian jitter), or **Advanced** (humanized drift, butterfly clicking doublets, and stamina drop-off). | `Basic` / `Medium` / `Advanced` (`Medium`) |
| Right only when holding block | Right-click only fires while you are holding a placeable block. | Off |
| Break blocks | Treats held left-click as mining intent and pauses clicking while breaking a block. | Off |
| Click in chests | Allows clicking inside chest/container GUIs (otherwise paused there). | Off |

## Usage notes

- The left autoclicker only runs when the module is **Armed**; the right autoclicker is independent so it can be used on its own.
- Clicking automatically pauses in inventories, crafting/anvil/furnace screens, and (unless **Click in chests** is on) chest GUIs. It also pauses when the system cursor is visible.
- **Randomization modes**:
  - **Basic**: Flat uniform random selection within the configured Min/Max CPS window.
  - **Medium**: Gaussian distribution clustered around mid CPS, touching window edges naturally.
  - **Advanced**: Human-like click engine simulating physical PvP mechanics:
    - *Butterfly clicking doublets*: Alternates between rapid secondary finger strikes and recovery resets, averaging to target CPS.
    - *Stamina drop-off*: Prolonged sustained clicking builds up muscle fatigue (up to 25% drop-off), gradually lowering effective CPS until a brief pause allows recovery.
    - *Engagement rush*: Brief 2–5 burst clicks upon clicking after an idle pause.
    - *Macro drift & debounce repeats*: Wander target CPS smoothly every few seconds with occasional muscle-memory interval replication and micro-hesitations.

:::caution
A randomized autoclicker is the intended use case; very high or perfectly consistent CPS is easy for anti-cheats to flag. Keep the Min/Max window realistic and use **Medium** or **Advanced** randomization.
:::
