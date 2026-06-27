---
title: GTB Helper
description: Solves Guess-The-Build hints and shows candidate words in an overlay.
---

*GTB Helper* reads the Guess-The-Build (GTB) hint from the action bar, matches it against a word list, and shows the candidate answers in an overlay.

## Version support

1.8.9 · 1.21.x · 26.1

## Settings

This module is a single toggle. When enabled it surfaces:

| Output | Description |
| ------ | ----------- |
| Current hint | The masked hint pattern parsed from the action bar (e.g. letters and blanks). |
| Match count | Number of dictionary words matching the current hint. |
| Matches preview | A preview list of the candidate words. |

## Usage notes

- The helper learns solved words it observes, improving future matches.
- Results update live as the action-bar hint reveals more letters, narrowing the candidate list.
- The hint overlay position can be moved with the [HUD Editor](/aoko-client/modules/visual/hud-editor/).

:::tip
This is an information-only overlay — it shows you likely answers but does not type or chat for you.
:::
