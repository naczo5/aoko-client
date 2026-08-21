---
title: Auto Rod
description: Switches to a fishing rod, uses it once, and restores the exact previous hotbar slot.
---

*Auto Rod* performs one controlled item-use transaction when its dedicated action bind is pressed: it saves your exact selected hotbar slot, changes the selected-slot field to the chosen rod slot, lets Minecraft synchronize it through the normal tick path, submits one right-click to Minecraft's input loop, then restores the original slot field. It does not simulate number keys, so custom hotbar keybinds do not affect it.

## Version support

1.8.9 · 1.21.x · 26.1 · 26.2.

## Binds

Auto Rod has two independent binds, both unbound by default:

- **Auto Rod** in Settings is the general module bind. It only enables or disables the module.
- **Action** inside the Auto Rod card performs one transaction while the module is enabled.

The action bind accepts keyboard keys, left/right/middle mouse buttons, and X1/X2. One action is accepted per physical press; the key or button must be released before it can fire again. The action bind cannot share an input with any general module bind.

## Settings

| Setting | Description | Values / Default |
| ------- | ----------- | ---------------- |
| Slot mode | `Auto` chooses the lowest-numbered hotbar slot containing a fishing rod. A forced slot always targets that exact numbered slot. | `Auto`, `Slot 1`–`Slot 9` / `Auto` |
| Verify forced slot | Requires a fishing rod in a forced slot. If verification or rod identification is unavailable, the action silently does nothing. | On |
| Extension delay | Keeps the selected item active for the configured number of client ticks after the use input before restoring the original slot. One tick is approximately 50 ms. | `1`–`40` ticks / `4` |
| Hold action bind to extend cast | Ignores the fixed extension value and keeps the selected item active until the dedicated action bind is released. Restoration still waits at least one tick after use. | Off |

Disabling **Verify forced slot** allows the forced slot to contain another usable item. This is an advanced option: Auto Rod will use that slot regardless of its contents, then restore the exact slot selected before the action.

## Eligibility and input behavior

An accepted action input is consumed so Minecraft does not also process it. It passes through without performing an action when Auto Rod is disabled or unsupported, the bridge is disconnected, Minecraft is not focused, no world is active, or any screen is open, including chat, inventory, pause, and menus. A queued request is revalidated by the native bridge immediately before interaction.

The bridge changes the selected-slot field without emitting held-item or use packets from its worker. It waits two real client ticks for Minecraft to synchronize the selected item through its normal tick path, then submits one right-click to Minecraft's input loop. In fixed mode it restores the exact original slot after the configured 1–40 tick extension delay (four ticks by default). In hold mode it restores after the action bind is released, while always leaving at least one tick after the use input. A safety timeout restores the original slot if a release is lost. The existing one-tick restoration settle phase remains unchanged. It does not send raw packet spam and never searches for a sword or assumes the default `1`–`9` hotbar keybinds.

## Troubleshooting

- Confirm both the general module state and the separate action bind.
- In `Auto`, put a fishing rod in the hotbar; the first matching slot is used.
- In a forced mode with verification enabled, confirm that exact slot contains a fishing rod.
- Close chat and all other Minecraft screens and keep the game focused.
- For mapping or interaction failures, inspect `bridge_debug.log` on 1.8.9 or `bridge_261_debug.log` on 1.21.x/26.1/26.2.
