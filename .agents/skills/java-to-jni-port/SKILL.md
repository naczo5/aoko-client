---
name: java-to-jni-port
description: >-
  Port Minecraft modules from in-JVM Java (event buses, Mixins, mapping wrappers,
  obfuscated helpers) into Aoko C++ JNI bridges and C# SendInput. Use when
  reproducing or translating a Java module into the native bridge — not when only
  editing existing Aoko C# UI.
---

# Port Java Modules to Aoko JNI

In-JVM Java is an **algorithm reference**, not a drop-in source. Aoko has no Java
event bus, no Mixins, and none of the reference client’s wrapper types in the
target JVM. Copying Java call-for-call produces crashes, BadPackets, or a
1.8.9-only stub that never works on 26.1.

Local Java trees, if present, live under gitignored `_refs/`. Treat any index
markdown there as orientation only.

Then follow **add-client-module**. Mutating ports also follow **packet-and-call-hygiene**.
Event/API tables: `references/event_and_api_map.md`.

---

## 1. Unwrap before you translate

Read the Java until you can name **vanilla Minecraft** types and side effects.
Stop when the remaining names are the reference client’s own framework.

| If the Java says | It is | In Aoko |
| :--- | :--- | :--- |
| Mapping wrappers, `MMinecraft`-style stubs, obfuscated `g()` / `v()` | Façade over JNI/reflection | Resolve `net.minecraft.*` field/method IDs (Yarn then Mojmap on modern) |
| Obfuscated singleton (`thePlayer` via a renamed `Minecraft` instance), slot helper classes, `@EventLink` / tick listeners | Their runtime + MCP (sometimes obfuscated) | `thePlayer` JNI field; slot write + cache invalidate; Aoko tick/premotion |
| Mixin / ASM / `PacketEvent` send-queue inject | Bytecode hook | MinHook / premotion / vanilla emission — **not** a Java transformer |
| `BooleanValue` / `NumberValue` (or equivalent) | Their settings UI | `Clicker` + Profile + TCP JSON + capabilities strings |

**Worked example (already in tree):** a Java AutoTool tick loop (swap-to delay,
swap-back, weapon-on-entity, require mouse) became `auto_tool_core.h` + tests.
JNI only writes `currentItem` and sets controller cache to `-1`. It does **not**
call a wrapper method such as `InventoryPlayer.g()` — that method is not on
Minecraft’s class.

---

## 2. Port workflow (do this in order)

1. **Pick one source of truth.** If two Java trees describe the same module, use
   one for event order and one for the state machine — do not merge both into a
   single C++ function.
2. **Extract the algorithm on paper:** inputs, timers/ticks, outputs, what it
   writes (slot, velocity, overlay, keys). Ignore their GUI and obfuscated names.
3. **Choose the Aoko host** (see §3). Default to the *least* privileged host that
   can reproduce the behavior.
4. **Pull sequencing into a C++ header** when there is delay/restore/lockout
   (`auto_rod_core.h` / `auto_tool_core.h`) and unit-test it in `McInjector/tests`.
5. **Map types** with Yarn-first arrays in `bridge_261.cpp`; LaunchClassLoader +
   MCP/SRG names in `bridge.cpp`. Fail open if an ID is missing.
6. **Wire the product** via `ModuleCatalog` (add-client-module). Settings names
   stay stable JSON keys, not the reference client’s aliases.
7. **Conflict pass:** if it writes hotbar/use/attack/window/look, pause the other
   owners (AutoRod transaction vs AutoTool `pauseExclusive`).

Do not implement from a reference “import feasibility” blurb. Those notes often
recommend packet spam this repo forbids.

---

## 3. Where the logic lives

| Java pattern | Aoko host | Examples |
| :--- | :--- | :--- |
| Key priority, CPS, smooth aim, OS mouse | `Aoko/Core` `SendInput` / `InputHooks` | Autoclicker, Aim Assist |
| Read fields, overlay, ESP | JNI read + ImGui on present | BedPlates, nametags |
| Slot/use/attack/velocity/reach | JNI on **tick / premotion / attached worker**, not `wglSwapBuffers` | AutoTool, AutoRod, KillAura, Velocity |
| Silent rotations / pre-C03 attack | Premotion hook (`kill_aura_premotion.cpp`) | KillAura silent |
| Their `PacketEvent` send/cancel | Usually **do not port**. Prefer vanilla emission or an owned premotion path | Faster use via `itemUseCount`, not crafted `C08` spam |

If the Java runs on `PreUpdate` / `PrePlayerTick`, Aoko still uses a
game-tick-aligned path. Render thread only caches JNI IDs and draws.

---

## 4. Reproduction rules (why ports look “wrong”)

* **Timers:** some Java helpers are milliseconds (`GetTickCount`); others are
  client ticks (`ticksExisted`). Do not mix. AutoRod select-to-use is **ticks**.
* **Events are not functions.** “Block damage” means the player started breaking.
  Approximate with `objectMouseOver` + `breakingBlock` / LMB — do not invent a
  Java listener.
* **One version is not done.** 1.8.9 flat `loadedTileEntityList` vs modern chunk
  `blockEntities`; `ActiveRenderInfo` vs JOML `Matrix4f`; hit MISS on air.
* **Do not `FindClass` the reference client’s types** in the game JVM. Only
  Minecraft + JDK.
* **Do not `Call*` a helper that itself sends packets** from a worker
  (`syncCurrentPlayItem`, `sendUseItem`, `clickBlock`). Same rule as AutoTool.
* **Obfuscated one-letter methods are not the spec.** Decode via nearby MCP names
  or the mapping class the wrapper delegates to, then bind Yarn/Mojmap/SRG.
* **Read-first.** Skip disablers, crashers, and unbounded packet queues even if
  the Java implements them.

---

## 5. Checklist before coding JNI

- [ ] Vanilla types and side effects written down (no wrapper names left)
- [ ] Host chosen (C# input vs overlay vs tick JNI vs premotion)
- [ ] Dual-version mapping plan (Yarn then Mojmap; 1.8.9 loader)
- [ ] Exclusive resources listed vs AutoRod / AutoTool / KillAura / Chest Stealer
- [ ] Shared header + native test if there is state/delay
- [ ] `ModuleCatalog` and capabilities strings, not a one-off `bridge.cpp` toggle
