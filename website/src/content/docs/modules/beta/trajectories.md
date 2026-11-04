---
title: Trajectories
description: Draws the predicted flight path of the projectile you are holding.
---

> **Beta.** This module lives in the Beta tab of the external GUI. It was not thoroughly tested; use it at your own risk.

*Trajectories* draws the arc and landing spot of the projectile in your main hand.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Usage notes

- Supports a drawn bow (the arc follows the current pull strength), snowballs, eggs, ender pearls, splash and lingering potions, and experience bottles.
- The path stops at the first solid block. A green ring marks a block hit; an amber ring means the path left the simulation range.
- The prediction ignores your own movement speed and entity hits, so it can drift while you are running.
