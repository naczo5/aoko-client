---
title: Cosmetics
description: Shows a cape, a hat and a back item on your own player.
---

> **Beta.** This module lives in the Beta tab of the external GUI. It was not thoroughly tested; use it at your own risk.

*Cosmetics* draws a cape, a hat item and a back item on your own player. It is visual only: nothing is sent to the server and other players do not see any of it.

## Version support

1.8.9 · 26.1 · 26.2 · 26.3

Cosmetics is not available on 1.21.x. The status line on the card says so when you inject there.

## Settings

| Setting | Description | Default |
| ------- | ----------- | ------- |
| Cape | Cape shown on your player. **None** leaves your vanilla cape alone. | None |
| Hat | Item worn on your head. | None |
| Back item | Item worn on your back. | None |
| Open folder | Opens the visuals folder in Explorer. | |

The status line under the pickers shows what the game side is doing.

| Status | Meaning |
| ------ | ------- |
| Not active. | The module is off or nothing is injected. |
| Active. | Everything you picked is on screen. |
| Partly applied: ... | Some picks could not be shown. The text says which. |
| Needs Minecraft 1.8.9 or 26.1 and newer. | The injected version does not support Cosmetics. |
| Something went wrong: ... | A pick failed to load. The text says why. |

## Built-in items

- Capes: Aoko Glass, Ember Crest, Sakura Drift, Tsukihime Moon, Void Star.
- Hats: Cat Ears, Halo.
- Back items: Angel Wings.

## Usage notes

- The pickers list what is in `%AppData%\Aoko\visuals`. The lists refresh when you come back to the Aoko window and after you press **Open folder**, so you can drop a file in and see it right away.
- Aoko copies its built-in capes and items into that folder when it starts. If you edit one of them, Aoko keeps your version.
- A saved pick whose files are gone stays selected and shows as *(missing)* until you choose something else.

## Add your own cape

1. Press **Open folder** and go to `capes`.
2. Copy a PNG in. The file name is the id: lowercase letters, digits, `-` and `_`, up to 32 characters, starting with a letter or digit. `my-cape.png` shows as *My Cape*.
3. Use the vanilla cape layout at a 2:1 ratio. The width must be a multiple of 64, for example 64x32, 128x64 or 256x128.
4. Pick it in the **Cape** list.

The [capes section of the asset format](https://github.com/naczo5/aoko-client/blob/main/.agents/skills/visuals-pipeline/references/asset-format.md#capes) lists where each face sits on the image.

## Add your own hat or back item

Each item is a folder under `cosmetics`, with a `cosmetic.json`, a Bedrock `model.geo.json` and a `texture.png`:

```
cosmetics\
  my-halo\
    cosmetic.json
    model.geo.json
    texture.png
```

The fields of `cosmetic.json` are defined in the [cosmetics section of the asset format](https://github.com/naczo5/aoko-client/blob/main/.agents/skills/visuals-pipeline/references/asset-format.md#cosmetics-cosmeticsidcosmeticjson). The ones you will use most:

| Field | What it does |
| ----- | ------------ |
| `name` | Name shown in the list. Required, up to 64 characters. |
| `slot` | `hat` or `back`. Decides which list the item appears in. Required. |
| `anchor` | Body part the model follows: `head`, `body`, `rightArm` or `leftArm`. |
| `model`, `texture` | File names inside the folder. |
| `scale`, `offset` | Size and position tweaks. |
| `fullbright`, `translucent`, `hideWithHelmet` | Lighting, blending and helmet behaviour. |
| `animations` | Optional spin, swing and bob per bone. |

An item whose `cosmetic.json` is broken, or whose folder name is not a valid id, is skipped and does not appear in the list.

## Safety

Cosmetics only changes what your own game draws. It sends nothing to servers, and nobody else sees your cape or items.
