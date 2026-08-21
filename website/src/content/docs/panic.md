---
title: Panic
description: The panic action instantly disables every module and hides the client.
---

**Panic** is a single action that immediately returns aoko to a clean, stealthy state. It is designed to be bound to a key you can hit instantly.

When triggered, panic:

- disarms the autoclicker and stops all clicking;
- disables all combat, movement, render, and utility modules;
- hides the in-game module list and logo;
- disables Discord Rich Presence;
- puts the loader window into stealth mode;
- disconnects the bridge and shuts down the loader.

## Usage

- Bind **panic** using the Panic keybind button in the external GUI (see [Profiles & keybinds](/aoko-client/profiles-keybinds/)).
- Panic can be triggered from the GUI or its bound key.

:::tip
Panic intentionally never persists a disabled left-click into your saved config. Your normal settings remain intact the next time you launch.
:::
