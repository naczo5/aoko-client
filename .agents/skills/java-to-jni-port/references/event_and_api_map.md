# Java Event & API Map (reference Java → Aoko)

Decode in-JVM Java to **vanilla Minecraft**, then to an Aoko host.
Do not `FindClass` the reference client’s packages inside the game JVM.

---

## 1. Event bus → Aoko timing

| Typical Java event | Meaning | Aoko |
| :--- | :--- | :--- |
| `PreUpdate` / `PreMotion` / `PrePlayerTick` | Before movement packets / player tick | Attached worker or tick loop; silent look and owned attacks use premotion (`onUpdateWalkingPlayer` / `sendMovementPackets`) |
| `PostMotion` | After movement packet | Restore local yaw/pitch before present |
| `BlockDamage` / `BlockBreak` | Started or finished mining | `objectMouseOver` type BLOCK + `breakingBlock` / LMB; no Java listener |
| `PacketEvent` (send/receive) | Intercept the netty/send queue | Do not add a send-queue interceptor by default. Owned packets only via premotion or vanilla emission |
| `Render2D` / `Render3D` | Overlay | ImGui on `wglSwapBuffers` / `vkQueuePresentKHR` (draw-only) |
| Key / mouse input events | Their input API | C# `InputHooks` / `GetAsyncKeyState` / game keybind fields |

---

## 2. Common Minecraft APIs

| Intent | 1.8.9 (MCP / SRG) | Modern (try Yarn `class_*` first, then Mojmap) | Aoko notes |
| :--- | :--- | :--- | :--- |
| Local player | `thePlayer` / `field_71439_g` | `player` / `class_746` | Global ref only if documented; re-fetch each tick when possible |
| World | `theWorld` / `WorldClient` | `world` / `class_638` | Abort exclusive txns on world identity change (AutoRod) |
| Held slot | `InventoryPlayer.currentItem` | selected slot on inventory | Write field + controller cache `-1`; do not `syncCurrentPlayItem` off-thread |
| Hit result | `objectMouseOver` / `MovingObjectPosition` | `crosshairTarget` / `HitResult` | Air is `MISS` on modern, not null |
| Use item | `sendUseItem` / keyBindUseItem | interaction manager / use item | Prefer `SendInput` right-click after slot settle (AutoRod) |
| Attack | `attackTargetEntityWithCurrentItem` + `C02` | `GameMode.attack` / interact packet | Premotion-owned; see packet-and-call-hygiene |
| Velocity | `motionX/Y/Z` | `getDeltaMovement` / `setDeltaMovement` | Field write on hurtTime spike |
| GUI open | `currentScreen != null` | `currentScreen` | Freeze AutoTool/AutoRod/combat |

---

## 3. Mapping wrappers

Obfuscated wrappers (`g(slot)`, `v()`, generated `V$src$...` names) usually
delegate to a mapping class that stores the real MCP/Yarn name. Those wrapper
types **are not in the game**. Open the mapping class, read the vanilla name it
binds, then duplicate that bind in `bridge.cpp` / `bridge_261.cpp`.

Inventory set-slot in Java (`inventory.setCurrentItem` / a one-letter setter) =
`currentItem`. Aoko: `SetIntField` + invalidate `currentPlayerItem`.

---

## 4. In-JVM helpers

Renamed singletons (`Minecraft.getMinecraft()` behind an obfuscated field) are
still the vanilla client instance. Slot/component helpers that live in the
reference client are **their** code. Reimplement the slot **policy**, not the
helper class.

Use one Java source for 1.8.9 event *order* (damage → update → break) and another
for a tick state machine (delays, swap-back, flags) when both exist. Keep that
split in the port; do not fuse them.
