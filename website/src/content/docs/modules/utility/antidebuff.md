---
title: AntiDebuff
description: Hides screen-obscuring potion effects client-side.
---

*AntiDebuff* hides screen-obscuring status effects on the client so they don't impair your view. The server still considers the effects active. Only your local rendering changes.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

| Effect hidden | 1.8.9 | 1.21.x / 26.1 / 26.2 |
| ------------- | :---: | :------------------: |
| Blindness     | ✅ | ✅ |
| Nausea        | ✅ | ✅ |
| Darkness      | N/A | ✅ |

## Settings

This module is a single toggle.

## Usage notes

- Removes the Blindness fog and Nausea (warping) overlay client-side; on 1.21.x / 26.1 / 26.2 it also suppresses Darkness.
- This is purely a visual and client-side change. It does not remove the effect on the server.

:::tip
Useful against servers/maps that spam Nausea or Blindness purely to obstruct your screen.
:::
