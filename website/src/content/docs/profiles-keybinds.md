---
title: Profiles & keybinds
description: How aoko stores configs and how per-module keybinds work.
---

## Profiles & configs

aoko saves your full module configuration — every toggle, slider, and the active theme — as JSON config files.

- Configs live in `%AppData%\Aoko\profiles\`.
- Each saved config is a `*.json` file named after the config.
- A hidden internal `config.json` slot auto-saves your live working state on close and restores it on launch, so your settings persist between sessions even without manually saving.
- Profiles from older builds stored under the legacy folder are migrated automatically the first time aoko runs.

Saving a config captures the current state of the loader (CPS values, enabled modules, ESP targets, HUD layout, theme, keybinds, and more).

## Keybinds

Every module can be bound to a key under the **Keybinds** tab. Bindings are stored per config in the `moduleKeys` map.

- Keybinds are **unbound by default**, with one exception: the **Autoclicker** toggle defaults to the backtick / grave key (`` ` ``).
- Bindable actions include: autoclicker, right-click, jitter, click-in-chests, break-blocks, aim assist, triggerbot, speedbridge, GTB helper, PixelParty assist, nametags, closest player, fight status, chest ESP, chest stealer, block ESP, reach, velocity, autototem, HUD editor, and **panic**.
- A key toggles its module on/off while the Minecraft window is focused.

:::tip
Bind **Panic** to a key you can reach instantly — it disables every module and hides the client in one press. See the [Panic](/aoko-client/panic/) page.
:::
