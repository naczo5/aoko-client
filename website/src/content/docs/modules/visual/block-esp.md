---
title: Block ESP
description: Highlights configurable block types through walls, with boxes, tracers, and a HUD list.
---

*Block ESP* highlights chosen block types through walls. You pick which blocks to search for, and aoko draws boxes and/or tracers to nearby matches and lists them on a HUD panel.

## Version support

1.8.9 · 1.21.x · 26.1

## Settings

| Setting | Description | Range / Default |
| ------- | ----------- | --------------- |
| Boxes | Draws a box around each matching block. | On |
| Tracers | Draws lines toward matching blocks. | Off |
| HUD list | Shows a HUD panel listing found blocks. | On |
| Max count | Maximum number of blocks highlighted at once. | `1`–`512` / `64` |
| Range | Search radius in chunks/blocks around you. | `1`–`8` / `4` |
| Blocks | The set of block types to search for (configurable target list). | Default preset |

## Usage notes

- The **Blocks** list defines which block registry IDs are highlighted; it ships with a default preset and is fully editable.
- **Range** trades coverage for performance — a larger range scans more area but costs more each frame.
- **Max count** caps how many matches render so dense areas stay readable.
- The block list HUD panel can be moved with the [HUD Editor](/aoko-client/modules/visual/hud-editor/).

:::caution
This effectively works as a targeted X-ray for the block types you choose. Expect it to be against the rules on most public servers — best reserved for private worlds.
:::
