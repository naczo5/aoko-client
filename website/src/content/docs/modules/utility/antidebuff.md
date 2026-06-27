---
title: AntiDebuff
description: Hides screen-obscuring potion effects client-side.
---

*AntiDebuff* hides screen-obscuring status effects on the client so they don't impair your view. The server still considers the effects active — only your local rendering is changed.

## Version support

1.8.9 · 1.21.x · 26.1

| Effect hidden | 1.8.9 | 1.21.x / 26.1 |
| ------------- | :---: | :-----------: |
| Blindness     | ✅ | ✅ |
| Nausea        | ✅ | ✅ |
| Darkness      | —  | ✅ |

## Settings

This module is a single toggle.

## Usage notes

- Removes the Blindness fog and Nausea (warping) overlay client-side; on 1.21.x / 26.1 it also suppresses Darkness.
- This is purely a visual/client-side change — it does not remove the effect on the server or grant any gameplay advantage beyond clearer vision.

:::tip
Useful against servers/maps that spam Nausea or Blindness purely to obstruct your screen.
:::
