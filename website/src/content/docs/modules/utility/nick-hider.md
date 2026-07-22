---
title: Nick Hider
description: Client-side local username spoofing via native JVMTI string filtering.
---

Nick Hider replaces your in-game username with a custom alias locally across rendered text, nametags, scoreboards, and chat messages.

## How It Works

Nick Hider operates via a native JVMTI agent injected alongside the bridge DLL. It inspects string allocations and replaces occurrences of your actual player name with your configured alias before text is rendered on screen.

- **Local Only**: Changes are purely client-side and do not affect what other players or the server see.
- **Streamer Safety**: Useful for hiding your real account name during recordings or livestreams.

## Configuration Options

| Setting | Type | Description |
| :--- | :--- | :--- |
| `Nick Hider` | Toggle | Enables or disables local username replacement. |
| `Alias` | Text | The custom name displayed in place of your player name. |
