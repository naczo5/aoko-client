---
title: AutoTool
description: Automatically selects the best weapon on hover and optimal tool while mining, with Bedwars chest protection.
---

*AutoTool* automatically selects the optimal tool from your hotbar when mining blocks and can instantly switch to your strongest weapon when aiming at an entity. It includes Bedwars mode to prevent accidental tool swaps when depositing resources into chests.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Enable AutoTool | Toggles automatic tool and weapon switching. | Toggle / `Off` |
| Swap weapon on entity hover | Automatically switches to your best weapon (swords over axes) when looking at an entity. | Toggle / `On` |
| Instant weapon swap | Skips the swap-to delay when targeting an entity for immediate combat readiness. | Toggle / `On` |
| Swap-to delay | Delay in milliseconds before switching to the optimal tool when targeting a block. | `0`–`200` ms / `50` ms |
| Swap back to original slot | Automatically restores your previously held item once you stop breaking blocks or hovering targets. | Toggle / `Off` |
| Swap-back delay | Delay in milliseconds before restoring the original slot after mining/combat stops. | `0`–`1000` ms / `350` ms |
| Require mouse down | Requires left-click to be physically held before swapping to mining tools. | Toggle / `On` |
| Only while sneaking | Restricts AutoTool activation to when the player is sneaking. | Toggle / `Off` |
| Bedwars mode | Prevents tool swaps when punching chests, allowing rapid resource deposits via left-click. | Toggle / `On` |

## Usage notes

- **Bedwars mode**: When enabled, left-clicking or punching chest blocks will not cause AutoTool to swap to an axe or fist tool. This ensures players can spam left-click to deposit iron, gold, diamonds, and emeralds into team chests without interruption.
- **Weapon Priority**: Swords are scored higher than axes by default. If an opponent entity is targeted while standing near or in front of blocks, weapon switching takes precedence.
- **Hotbar Safety**: AutoTool coordinates with Auto Rod and other mutating modules to avoid slot conflicts during active casts.
