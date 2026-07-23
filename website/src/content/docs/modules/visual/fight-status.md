---
title: Fight Status
description: World-space fight prediction using effective survivability and recent combat telemetry.
---

*Fight Status* compares you with a nearby opponent and displays the likely fight outcome beside that player. It works independently of [Kill Aura](/aoko-client/modules/combat/killaura/); Kill Aura does not need to be enabled or targeting the player.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2

## Settings

This module is a single toggle and can also be assigned a keybind.

## Target selection

- Fight Status always tracks the closest eligible player within `16` blocks.
- A recently damaged opponent takes priority for `5` seconds while they remain eligible and in range. After that priority expires, tracking returns to the closest eligible player.

## Display and prediction

The world-space overlay is centered alongside the selected player's projected body. Its effective-survivability bar has a midpoint: green indicates your advantage and red indicates the opponent's advantage. The accompanying status reads **WINNING**, **EVEN**, or **LOSING**.

Before making a prediction, the overlay shows **Gathering data...** until it has observed at least `1.5` seconds of combat and at least `2` damage events. Once ready, it predicts the winner with a confidence value from `50%` to `95%`.

The prediction combines:

- current health and absorption;
- an armor-based effective-survivability estimate;
- observed damage rate;
- hit frequency; and
- recent combat momentum.

## Usage notes

- Fight Status observes combat telemetry; it does not attack, aim, or require another combat module.
- The bar follows the tracked player in world space rather than occupying a fixed HUD position.

:::caution
The overlay does not currently account for wall or depth occlusion, so it may remain visible when the selected player is behind geometry. Health, armor, and damage telemetry depend on version-specific mappings and observations; armor mitigation and the resulting prediction are estimates, not guaranteed outcomes.
:::
