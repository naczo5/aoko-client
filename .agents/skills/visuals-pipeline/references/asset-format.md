# Visuals asset format and wire contract

Single source of truth for the cosmetics and sky features. The C# loader, the Java
helper (`McInjector/src/main/java/com/aoko/visuals`), the native bridges, and the
asset generator all read this file. Change it first, then the code.

## Directory layout

Built-in assets live in the repo under `Aoko/Assets/Visuals/` and are embedded into
`Aoko.exe`. On start the loader copies them to `%AppData%\Aoko\visuals\` (see
"Installation"). The Java helper only ever reads from that directory.

```
visuals/
  capes/<id>.png
  skies/<id>/sky.json
  skies/<id>/sky.jpg            (optional cubemap atlas; .png also accepted)
  cosmetics/<id>/cosmetic.json
  cosmetics/<id>/model.geo.json
  cosmetics/<id>/texture.png
```

`<id>` matches `^[a-z0-9][a-z0-9_-]{0,31}$`. The display name of a cape is the id with
`-`/`_` replaced by spaces and each word capitalised unless a `name` is given elsewhere.
Skies and cosmetics carry an explicit `name`.

## Capes

`capes/<id>.png`: vanilla cape layout, width:height = 2:1, width a multiple of 64
(64x32, 128x64, 256x128). Scale all vanilla pixel coordinates by `width / 64`.

Vanilla (64x32) regions, in pixels:

| Region                      | x,y   | w x h  |
| :-------------------------- | :---- | :----- |
| top edge                    | 1,0   | 10 x 1 |
| bottom edge                 | 11,0  | 10 x 1 |
| left edge (wearer's right)  | 0,1   | 1 x 16 |
| outer face (seen from behind) | 1,1 | 10 x 16 |
| right edge                  | 11,1  | 1 x 16 |
| inner face (against back)   | 12,1  | 10 x 16 |

Everything outside those regions is unused. The outer face is the design. Keep the
edge strips the same colour family as the face border so seams are invisible.

## Skies: `skies/<id>/sky.json`

```json
{
  "name": "Tsukihime",
  "time": 18000,
  "clearWeather": true,
  "colors": {
    "day":   { "sky": "#78a7ff", "fog": "#c0d8ff", "cloud": "#ffffff", "sunset": "#ff9a4d" },
    "night": { "sky": "#05071a", "fog": "#0a0d28", "cloud": "#1a1d3a", "sunset": "#6a4cff" }
  },
  "stars": 1.0,
  "moonPhase": 0,
  "skybox": {
    "texture": "sky.jpg",
    "dayBrightness": 1.0,
    "nightBrightness": 1.0,
    "rotation": [0.0, 0.4, 0.0]
  }
}
```

* `name` required. Everything else optional.
* `time`: tick of day (0 to 23999) the preset shows by default. Absent means follow
  the server clock. The user's `skyTime` setting overrides it. On 1.8.9 a locked time also
  changes world brightness (the game derives lighting from it); on 26.x it only moves the sky.
* `clearWeather`: hide rain and thunder visuals. Default false.
* `colors.day` / `colors.night`: `#rrggbb`. `sunset` tints the horizon at dusk and
  dawn. Missing groups fall back to the built-in "classic" colours.
* `stars`: star brightness multiplier, 0 to 2, default 1.
* `moonPhase`: 0 to 7, fixed moon phase (0 is the full moon, then the vanilla order). Absent
  follows the world.
* `skybox`: textured cube sky, honoured on 1.8.9 only. `texture` names the atlas file
  (`.jpg` or `.png`) and defaults to `sky.jpg`. The 26.x renderer uses the
  preset colours and ignores it. `rotation` is degrees per second around x, y, z.
  `dayBrightness` / `nightBrightness` scale the texture by time of day (0 to 1.5).

### Cubemap atlas

A 3 x 2 grid of square faces. Image width = 3 * face, height = 2 * face. Recommended
face size 512 (1536 x 1024 JPEG at quality 88, under 600 KB).

| Column 0 | Column 1 | Column 2 |
| :------- | :------- | :------- |
| row 0: `+X` | `-X` | `+Y` |
| row 1: `-Y` | `+Z` | `-Z` |

World axes: `+X` east, `+Y` up, `+Z` south. Sampling convention is the OpenGL
cubemap table. For a direction `d`, let `ma` be the component with the largest
absolute value and pick the face by its sign:

| Face | sc  | tc  |
| :--- | :-- | :-- |
| `+X` | `-z` | `-y` |
| `-X` | `+z` | `-y` |
| `+Y` | `+x` | `+z` |
| `-Y` | `+x` | `-z` |
| `+Z` | `+x` | `-y` |
| `-Z` | `-x` | `-y` |

`u = (sc / |ma| + 1) / 2`, `v = (tc / |ma| + 1) / 2`. `u` runs left to right and `v`
top to bottom inside the face tile. A generator writes a pixel at `(u, v)` of a face
by inverting that table to get `d`, then evaluating the sky at `d`. The sky function
must depend only on `d` so faces join without seams. Up (`+Y`) of the world is the
sky's zenith.

## Cosmetics: `cosmetics/<id>/cosmetic.json`

```json
{
  "name": "Halo",
  "slot": "hat",
  "anchor": "head",
  "model": "model.geo.json",
  "texture": "texture.png",
  "scale": 1.0,
  "offset": [0, 0, 0],
  "fullbright": true,
  "translucent": false,
  "hideWithHelmet": false,
  "animations": [
    { "bone": "ring", "spin": [0, 60, 0], "bob": [0, 0.4, 0], "speed": 0.6, "phase": 0 }
  ]
}
```

* `slot`: `hat` or `back`. The user picks one cosmetic per slot.
* `anchor`: `head`, `body`, `rightArm`, `leftArm`. The model follows that body part,
  including sneaking and head rotation.
* `scale`: uniform, default 1. `offset`: Bedrock axes, in model pixels (1/16 block),
  applied after the model is placed on the anchor.
* `fullbright`: ignore world lighting. `translucent`: use alpha blending instead of
  alpha cutout. Both default false.
* `hideWithHelmet`: hide while the player wears a helmet. Default true for `hat`.
* `animations`: optional, up to 16 entries. Per entry: `bone` (required), `spin`
  (degrees per second per Bedrock axis, continuous), `swing` (rotation amplitude in
  degrees per Bedrock axis, sine wave), `bob` (translation amplitude in pixels per
  Bedrock axis, sine wave), `speed` (Hz for `swing` and `bob`, default 1) and `phase`
  (0 to 1 offset of the sine waves). A bone may appear more than once. Children
  inherit the motion. Time comes from the wall clock, not the game tick.

### Model: Bedrock `.geo.json`

Blockbench "Bedrock Entity" export, `format_version` `1.12.0`
(`minecraft:geometry`). Supported: bones with `name`, `parent`, `pivot`, `rotation`;
cubes with `origin`, `size`, `uv` (box UV array or per-face object with `uv` and
`uv_size`), `inflate`, `mirror`, `pivot`, `rotation`. Ignored: locators, `poly_mesh`,
animation controllers, Molang. Limits: 64 bones, 512 cubes. Reject anything else.

Coordinates follow the Bedrock player skeleton: pixels, y up, the model origin at the
player's feet, head pivot at `(0, 24, 0)`, the head box spans y 24 to 32, the face is
on the north (`-z`) side. A hat sits above y 32. The loader converts a Bedrock point
`(x, y, z)` to Java model space `(x, 24 - y, z)` and subtracts the anchor's rest pivot:

| Anchor     | Java rest pivot |
| :--------- | :-------------- |
| `head`     | `(0, 0, 0)`     |
| `body`     | `(0, 0, 0)`     |
| `rightArm` | `(-5, 2, 0)`    |
| `leftArm`  | `(5, 2, 0)`     |

Rotations are Bedrock degrees, applied as `Rz * Ry * Rx` (x first) about the pivot.

## Wire contract: C# to bridge

Extra keys in the existing config line (`GameStateClient` config payload). All keys
are optional on the bridge side; absent means the default.

| Key               | Type   | Default | Meaning |
| :---------------- | :----- | :------ | :------ |
| `cosmeticsEnabled`| bool   | false   | master toggle for cosmetics |
| `cosmeticsCape`   | string | ""      | cape id, "" = none (vanilla cape untouched) |
| `cosmeticsHat`    | string | ""      | `hat` slot cosmetic id |
| `cosmeticsBack`   | string | ""      | `back` slot cosmetic id |
| `skyEnabled`      | bool   | false   | master toggle for sky |
| `skyPreset`       | string | ""      | sky id, "" = none |
| `skyTime`         | int    | -2      | -2 preset default, -1 follow server, 0..23999 lock |
| `skyClearWeather` | bool   | false   | force clear weather visuals |
| `visualsDir`      | string | ""      | absolute path of the visuals directory |

Ids are lowercase `[a-z0-9_-]`, 1 to 32 characters, starting with a letter or digit.
`visualsDir` is an absolute Windows path (drive letter or UNC), at most 1024 characters.
The config line is JSON from System.Text.Json: backslashes and non-ASCII characters arrive
escaped, and the bridge decodes them. The loader validates ids before sending; the bridge validates
again and ignores a bad id.

Bridge to loader: a `visualsStatus` string in the state payload, one of
`off`, `ok`, `partial:<detail>`, `unsupported:<detail>`, `error:<detail>`.

Module ids: `cosmetics` (overlay probe `cfg.cosmeticsEnabled`) and `sky` (overlay
probe `cfg.skyEnabled`). Both appear in the capability `modules` list for the legacy
and modern bridges; the setting keys above go in `settings`; `visualsStatus` goes in
`state`.

## Installation of built-ins

The loader embeds `Aoko/Assets/Visuals/**` and extracts it to
`%AppData%\Aoko\visuals\` on start. Each built-in file is rewritten only when its
content hash differs from the embedded copy and the destination was not edited by the
user (tracked in `visuals\.builtin.json`: relative path to SHA-256 of the last
extracted content). A user file with the same name as a built-in that differs from
the recorded hash is left alone. Files the user adds are never touched or deleted.
