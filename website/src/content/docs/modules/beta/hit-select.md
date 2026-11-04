---
title: Hit Select
description: Holds AutoClicker clicks while the target cannot take damage.
---

> **Beta.** This module lives in the Beta tab of the external GUI. It was not thoroughly tested; use it at your own risk.

*Hit Select* stops the AutoClicker from clicking while the player under your crosshair is still in its hurt window. The server ignores hits during that window, so those clicks only add swings.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Usage notes

- Needs the AutoClicker. It only filters left clicks while you are looking at an entity.
- The bridge reports the target's `hurtTime`; clicks resume once it drops to 1 tick or less.
- On a bridge that does not report the value, Hit Select does nothing.
