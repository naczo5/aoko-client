---
title: Jump Reset
description: Jumps on the first tick of a hit to take less knockback.
---

> **Beta.** This module lives in the Beta tab of the external GUI. It was not thoroughly tested; use it at your own risk.

*Jump Reset* presses your jump key for one tick when you take a hit while standing on the ground and holding W. Jumping on that tick cancels part of the knockback.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2 · 26.3

## Settings

- **Chance (%)**: how often a hit triggers the jump. 100 jumps on every hit.

## Usage notes

- Only triggers on the ground, with W held and no menu open.
- Sets the jump key binding for about 70 ms, then releases it unless you are holding jump yourself. It sends no packets.
- Works alongside Velocity and No Hurt Cam; No Hurt Cam waits until Jump Reset has seen the hit.
