---
title: HUD Editor
description: Drag and scale aoko's on-screen overlays into your preferred layout.
---

The *HUD Editor* lets you reposition and scale aoko's on-screen overlay elements, saving the layout into your config.

## Version support

1.8.9 · 1.21.x · 26.1

## Settings

When the HUD Editor is active you can move and scale these elements:

| Element | Module |
| ------- | ------ |
| Module list | [Module List & Logo](/aoko-client/modules/visual/module-list/) |
| Closest player panel | [Closest Player](/aoko-client/modules/visual/closest-player/) |
| PixelParty panel | [PixelParty Assist](/aoko-client/modules/movement/pixelparty-assist/) |
| Chest ESP list | [Chest ESP](/aoko-client/modules/visual/chest-esp/) |
| GTB hint | [GTB Helper](/aoko-client/modules/utility/gtb-helper/) |
| Nametags | [Nametags](/aoko-client/modules/visual/nametags/) |
| Block ESP list | [Block ESP](/aoko-client/modules/visual/block-esp/) |

## Usage notes

- Each element stores its own position and scale; the layout is saved with your config (see [Profiles & keybinds](/aoko-client/profiles-keybinds/)).
- Positions and scales are clamped to sane values on load, so an older or hand-edited config can't push an element off-screen.
- The editor can be toggled with its keybind under the **Keybinds** tab.

:::tip
Set up your layout once and it travels with your saved config.
:::
