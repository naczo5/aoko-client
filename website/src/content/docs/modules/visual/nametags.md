---
title: Nametags
description: Enhanced player tags showing health, armor, and held item.
---

*Nametags* renders enhanced tags above players, optionally showing their health, armor tier, and currently held item. It can also hide the vanilla nametag.

## Version support

1.8.9 · 1.21.x · 26.1 — note that **Held item** and **Max count** are only available on 1.21.x / 26.1.

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Show health | Displays each player's health on their tag. | On |
| Show armor | Displays the player's armor tier. | On |
| Show held item | Displays the player's currently held item. *(1.21.x / 26.1 only)* | On |
| Hide vanilla | Hides the default game nametag so only the enhanced tag shows. | Off |
| Max count | Maximum number of player tags rendered at once. *(1.21.x / 26.1 only)* | `1`–`20` / `8` |

## Usage notes

- Tags are drawn as overlays through the bridge and follow players on screen.
- **Max count** limits how many of the nearest players get tags, keeping the overlay readable in crowds.
- The nametag overlay can be repositioned with the [HUD Editor](/aoko-client/modules/visual/hud-editor/).

:::tip
Turn on **Hide vanilla** to avoid double tags stacking on top of each other.
:::
