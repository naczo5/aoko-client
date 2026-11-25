---
title: Module search
description: Finds any module card or GUI tab from the Search button, Ctrl+K, or the first letter you type.
---

The overlay has a row for each module card, plus rows for the **Accounts**, **Servers**, **Settings**, and **Stats** tabs. Rows rank by name first, then by the aliases the index carries, so `ka` reaches Kill Aura and `esp` reaches the ESP modules.

## Open it

- Click **Search** at the top of the tab rail.
- Press `Ctrl+K` anywhere in the window.
- Type a character while the window is focused and the overlay opens with that character in the box. Text fields keep their own typing.

`Esc` closes the overlay and puts focus back where it was.

## Keys

| Key | Action |
| :-- | :----- |
| `Up` / `Down` | Move the selection |
| `Enter` | Open the row's tab, scroll its card into view, then flash the card. Tab rows only open the tab. |
| `Ctrl+Space` | Toggle the selected module without closing the overlay |
| `Space` | Types a space. Only `Ctrl+Space` toggles. |

## Limits

- Modules the connected bridge cannot run are listed as unavailable and carry no switch. The card in its tab shows the same gate.
- Rows cover module cards and the four non-module tabs. Individual settings stay on their module cards.
- A module toggled from a row goes through the same path as its keybind, so the overlay and the bound key never disagree.
