---
title: Kill Aura
description: Automated target selection, rotation calculation, auto-blocking, and attack execution.
---

Kill Aura automatically detects, tracks, and attacks valid targets within a configurable range and Field of View (FOV).

## Features

- **CPS Modes**: Static range (Min/Max CPS) or randomized CPS distribution.
- **Ranges**:
  - **Attack Range**: Maximum distance to register attack clicks against target entities.
  - **Swing Range**: Distance at which swing animations begin.
- **FOV Limit**: Restricts target acquisition to a configurable FOV cone in front of the player.
- **Auto-Block**:
  - Automatically holds block using swords/weapons when target entities are within block range.
  - **Require Press**: Requires holding right click to trigger auto-block.
- **Target Selection**:
  - Filters by entity type (Players, Bosses, Mobs, Animals, Golems, Silverfish).
  - Teams check & bot check options.
- **Rotations & Smoothing**:
  - Adjustable turn speeds (Min/Max turn speed).
  - Noise randomization and overshoot recovery.
  - Premotion tick alignment for safe rotation sending.

## Configuration Options

| Setting | Type | Description |
| :--- | :--- | :--- |
| `KillAura` | Toggle | Enables or disables Kill Aura module. |
| `Min CPS` / `Max CPS` | Slider | Sets the clicks per second range for attacks. |
| `Attack Range` | Slider | Maximum distance (in blocks) to attack targets. |
| `Swing Range` | Slider | Range (in blocks) to start swinging. |
| `FOV` | Slider | Field of View angle (1° to 360°) for acquiring targets. |
| `Auto Block` | Toggle | Enables automatic blocking when targets are in range. |
| `Through Walls` | Toggle | Allows targeting entities obscured by blocks. |
| `Weapons Only` | Toggle | Restricts attacks to when holding a weapon (sword/axe). |
