---
title: Sky
description: Changes your sky colours, time of day, stars and moon with presets.
---

> **Beta.** This module lives in the Beta tab of the external GUI. It was not thoroughly tested; use it at your own risk.

*Sky* applies a sky preset to your game: sky, fog and cloud colours, a fixed time of day, clear weather, stars and the moon phase. It is visual only. Nothing is sent to the server, so the real time and weather do not change and other players see the normal sky.

A locked time of day also changes how bright your world is on 1.8.9, because the game derives lighting from it. A preset that locks midnight makes the world dark there. On 26.1 to 26.3 the lock only moves the sun, moon, stars and sky colours; lighting follows the server. Choose **Follow server** to keep normal lighting.

## Version support

1.8.9 · 26.1 · 26.2 · 26.3

Sky is not available on 1.21.x. The status line on the card says so when you inject there.

| Feature | 1.8.9 | 26.1 · 26.2 · 26.3 |
| ------- | ----- | ------------------ |
| Colours, time, clear weather, stars, moon | Yes | Yes |
| Textured skybox | Yes | No, the preset applies its colours only |

## Settings

| Setting | Description | Default |
| ------- | ----------- | ------- |
| Preset | Sky preset to apply. **None** leaves the sky alone. | None |
| Time of day | **Preset default**, **Follow server**, **Sunrise** (tick 0), **Noon** (6000), **Sunset** (12000), **Night** (13000) or **Midnight** (18000). | Preset default |
| Clear weather | Hides rain and thunder visuals. | Off |
| Open folder | Opens the visuals folder in Explorer. | |

**Preset default** uses the time stored in the preset. A preset without a time follows the server clock, the same as **Follow server**. The other choices lock the time of day whatever the preset says.

The status line shows what the game side is doing.

| Status | Meaning |
| ------ | ------- |
| Not active. | The module is off or nothing is injected. |
| Active. | The preset is applied. |
| Partly applied: ... | Part of the preset could not be shown. The text says which. |
| Needs Minecraft 1.8.9 or 26.1 and newer. | The injected version does not support Sky. |
| Something went wrong: ... | The preset failed to load. The text says why. |

## Built-in presets

Aurora, Ember, Glass, Nebula, Sakura Dusk and Tsukihime. Ember and Glass are colour-only presets without a skybox image.

## Usage notes

- The preset list is read from `%AppData%\Aoko\visuals\skies`. It refreshes when you come back to the Aoko window and after you press **Open folder**.
- Aoko copies its built-in presets into that folder when it starts. If you edit one of them, Aoko keeps your version.
- A saved preset whose files are gone stays selected and shows as *(missing)* until you choose something else.

## Add your own preset

Each preset is a folder under `skies`. The folder name is the id: lowercase letters, digits, `-` and `_`, up to 32 characters, starting with a letter or digit.

```
skies\
  my-sky\
    sky.json
    sky.jpg
```

`sky.jpg` is optional and only used on 1.8.9. Name it in `skybox.texture` (the default is `sky.jpg`). The fields of `sky.json` are defined in the [skies section of the asset format](https://github.com/naczo5/aoko-client/blob/main/.agents/skills/visuals-pipeline/references/asset-format.md#skies-skiesidskyjson). Only `name` is required.

| Field | What it does |
| ----- | ------------ |
| `name` | Name shown in the list, up to 64 characters. |
| `time` | Tick of day, `0` to `23999`, used when **Time of day** is **Preset default**. |
| `clearWeather` | Hides rain and thunder for this preset. |
| `colors.day`, `colors.night` | `#rrggbb` values for `sky`, `fog`, `cloud` and `sunset`. |
| `stars` | Star brightness, `0` to `2`. |
| `moonPhase` | Fixed moon phase, `0` (full moon) to `7`. |
| `skybox` | Textured sky for 1.8.9: `texture`, `dayBrightness`, `nightBrightness` and `rotation`. |

A preset with a broken `sky.json`, no `name`, or a folder name that is not a valid id is skipped and does not appear in the list.

### Textured skybox

The image is a 3 by 2 grid of square faces, saved as `sky.jpg` or `sky.png`. A face size of 512 gives a 1536 x 1024 image. The [cubemap atlas section](https://github.com/naczo5/aoko-client/blob/main/.agents/skills/visuals-pipeline/references/asset-format.md#cubemap-atlas) shows which face goes in which cell.

## Safety

Sky only changes what your own game draws. It sends nothing to servers, and nobody else sees your sky.
