---
title: Fireball Warning
description: Warns when a nearby player holds a fireball, with box, arrow, and sound alerts.
---

*Fireball Warning* watches nearby players' held items and warns you when someone holds a fireball / fire charge.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Enable Fireball Warning | Toggles the warning overlay. | Toggle / `Off` |
| Box warning | Draws a box around the holder with their name and distance. | Toggle / `On` |
| Arrow warning | Renders a directional arrow pointing toward the holder. | Toggle / `On` |
| Sound warning | Plays an in-game note-block pling (client-side only, never a Windows ding). | Toggle / `On` |

## Usage notes

- Detection is read-only: it inspects held-item names, never swaps slots or sends packets.
- Works through the existing player overlay scan, so no extra range setting is needed.
- Keybindable directly from the module card in the external GUI.
