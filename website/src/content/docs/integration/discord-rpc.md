---
title: Discord Rich Presence
description: Surfaces aoko's state, version, and active module count in Discord.
---

*Discord Rich Presence* (RPC) displays your aoko status in Discord, including current state, injected version, and active module count.

## Version support

Loader feature. Works across all supported versions.

## Settings

| Setting | Description | Default |
| ------- | ----------- | ------- |
| Discord RPC | Toggles the rich presence integration. | On |

## Usage notes

- When enabled, the loader publishes a live presence with the client state, target version, and active module count.
- The presence status text reflects the current loader state (e.g. ready / connected).
- [Panic](/aoko-client/panic/) disables Discord RPC as part of its stealth shutdown.

:::tip
Turn this off if you'd rather not advertise that you're running aoko in your Discord status.
:::
