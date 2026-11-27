---
name: visuals-pipeline
description: >-
  Cosmetics (capes, hats, back items) and sky presets across 1.8.9 and 26.x. Use when changing
  anything under McInjector/src/main/java/com/aoko/visuals, the method hook rewriter, the
  visuals bridge glue, the built-in assets, or the Cosmetics and Sky modules.
---

# Visuals pipeline

Two modules, `cosmetics` and `sky`, draw inside the game's own render path instead of from the
present hook. Read `references/asset-format.md` first: it is the contract for asset files and the
wire keys between the loader and the bridges.

## How it fits together

```
Aoko (C#)  --config line-->  bridge.cpp / bridge_261.cpp  --visuals::SetSettings-->  visuals_bridge.cpp
                                                                                      |
                              embedded com.aoko.visuals classes  <--DefineClass-------+
                              game bytecode + guard/value prologues  <--JVMTI retransform (method_hook_rewriter.h)
                                                                                      |
                              Visuals.configure(json) / maintain()  -->  LegacyPlatform | ModernPlatform
```

* `method_hook_rewriter.h` prepends `if (Hooks.guard(args)) return Hooks.value(args);` to chosen
  game methods. Retransformation may not add or rename methods, so the prologue is the only
  shape that works. The guard computes the replacement and the value half returns it, so a
  failure leaves vanilla behaviour. Hook tables live in `visuals_hooks.h`; the Java side is
  `Hooks.java`. Prefix `l` = 1.8.9, `m` = 26.x. Names and descriptors must match on both sides.
* Everything the helper does to the game is reflection, so the helper never links against game
  types. Names: MCP and SRG on 1.8.9, Mojang names on 26.x. 1.21.x is reported as
  `unsupported` (its names are obfuscated and its renderer differs).
* 1.8.9 draws with fixed-function OpenGL (`LegacyGl`) inside `LayerCape.doRenderLayer`, saving and
  restoring every state bit it touches so GlStateManager's cache stays true.
* 26.x submits `submitCustomGeometry` from inside `CapeLayer.submit` and swaps the cape by
  replacing `AvatarRenderState.skin`. Sky values replace environment attributes at
  `EnvironmentAttributeProbe.getValue`. Textured skyboxes are not supported there; the preset
  colours apply instead.
* Time lock changes world brightness on 1.8.9 (the celestial angle feeds lighting) and is
  visual only on 26.x. Say so in user docs.

## Changing things

1. Edit the Java under `McInjector/src/main/java/com/aoko/visuals`, then run
   `McInjector/gen_visuals_helper.sh` to refresh `jni_core/visuals_helper.inc` (CI-style check:
   `--check`). LWJGL 2 is compiled against `McInjector/tools/lwjgl2-stubs`; never embed it.
2. A new hook needs: a spec in `visuals_hooks.h`, a `Hooks` pair, a platform method that returns
   the replacement or null, and a case in the harnesses. Run the checks below against real jars.
3. Asset or schema changes start in `references/asset-format.md`, then
   `scripts/visuals/generate_assets.py` (`--check` verifies the committed art), then the C#
   catalog and installer.
4. A user visible change also follows `add-client-module` and `write-docs`.

## Verification (no game needed)

| Check | Command |
| :--- | :--- |
| Rewriter fixtures, Java 6 and 8 class files | `McInjector/tests/java/run_hook_tests.sh` |
| Core Java and shipped assets | `LWJGL_JAR=... McInjector/tests/java/run_visuals_tests.sh` |
| Hook table on real classes, verifier, end to end | `McInjector/tests/java/run_real_hook_checks.sh legacy|modern <jar> <libs> [java]` |
| Helper against real 26.x classes | `run_modern_harness.sh` (needs `MC_JAR MC_LIBS JAVA25 LWJGL_JAR`) |
| Helper against real 1.8.9 classes (MCP names) | `run_legacy_harness.sh` (needs `MC_JAR MC_LIBS LWJGL_JAR`) |
| Native helpers | `McInjector\run_tests.bat` (`method_hook_rewriter_tests`, `visuals_config_tests`) |

The 1.8.9 MCP jar is the vanilla jar run through SpecialSource with `joined.srg` plus the MCP
`methods.csv` and `fields.csv` (MCP stable 22). 26.x jars are unobfuscated Mojang downloads.
Not covered without a game: OpenGL output, GPU texture upload and the final look.

## Gotchas

* `SimpleJsonConfigReader` does not unescape. Decode strings with `visuals::UnescapeJsonString`;
  System.Text.Json writes `\\` and `\uXXXX` for paths.
* `ImageIO.createImageInputStream(InputStream)` spools to a temp file. `Library` reads from memory.
* Hooked classes in a signed jar and unsigned replacements cannot share a package in tests;
  the scripts extract the jar first.
* Never hook a bridge method twice: pass `kForbidBridge` for covariant overrides.
* Server worlds share classes with client worlds in singleplayer. Every sky guard checks for the
  client world and the overworld first.
