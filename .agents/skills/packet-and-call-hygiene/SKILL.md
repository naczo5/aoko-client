---
name: packet-and-call-hygiene
description: >-
  Packet sequencing, JNI calling safety, and concurrency rules for any module that
  changes held items, uses items, attacks, opens inventories, or writes game state
  in McInjector and Aoko. Use whenever creating or modifying AutoRod, AutoTool,
  AutoTotem, KillAura, Triggerbot, Chest Stealer, SpeedBridge, Reach, Velocity,
  Hit Delay Fix, or any other network-interacting or JNI-mutating feature.
---

# Packet & Function Calling Hygiene

This skill is the mandatory gate for **any** module that can emit packets or mutate
gameplay state — not only KillAura. Grim/Vulcan/Polar **BadPackets** flags usually
come from two modules owning the same resource in one tick (held slot, use-item,
attack, window click), not from a single sequence being slightly wrong.

Read `references/conflict_matrix.md` before writing swap/use/attack code.
Read `references/packet_sequences.md` for class names and constructors.

---

## 1. Golden Architecture Rules

1. **Read-first bridge.** Observe state by default. Mutate only inside a module that
   explicitly owns that mutation.
2. **One owner per resource per tick.** Hotbar, use-item, attack/swing, window clicks,
   and silent rotations are exclusive resources. A second module must yield or queue.
3. **Prefer vanilla emission.** Changing `inventory.currentItem` and invalidating the
   controller cache so the **client tick** sends `C09` is safer than calling
   `syncCurrentPlayItem` / `sendUseItem` from a worker thread.
4. **No unbounded spam.** Tick-gate, rate-limit, and respect select-to-use delays.
5. **Input vs JNI.** Aim Assist / autoclicker / AutoRod *use* prefer Win32 `SendInput`
   from a path that Minecraft's normal input loop will see. Do not mix a synthetic
   packet use with a synthetic OS click for the same action in the same tick.
6. **Fail open.** Unresolved mappings skip the action. They must not crash the JVM.

---

## 2. Resource Model (apply to every module)

Classify the feature before coding. If it touches a row below, it must participate
in the conflict matrix.

| Resource | Typical packets / calls | Example owners | Typical BadPackets |
| :--- | :--- | :--- | :--- |
| **Held slot** | `C09` / `ServerboundSetCarriedItemPacket` | AutoRod, AutoTool, AutoTotem, KillAura weapons-only | Slot change during use/attack; two `C09`s; use on the previous item |
| **Use item / place** | `C08` / use-item / right-click | AutoRod, KillAura AutoBlock, FastPlace, right-clicker | Use while attacking; place while blocking; use after a stolen slot |
| **Attack / swing** | `C0A` then `C02` / interact | KillAura, Triggerbot | Attack while GUI open; attack while blocking; swing without synced item |
| **Window clicks** | `C0E` / click slot | Chest Stealer | Combat packets while `currentScreen != null` |
| **Look / move** | `C03` / move player | KillAura silent aim | OS `SendInput` mouse mixed into pre-motion yaw writes |
| **Break / start destroy** | digging packets | AutoTool (mining context), left clicker | `START_DESTROY` from a non-tick thread; tool swap mid-break |

**Exclusive transaction pattern** (AutoRod is the reference implementation):

```
[own hotbar] ──> [wait N ticks so C09 lands] ──> [use via vanilla input]
        ──> [hold for extension ticks] ──> [restore slot] ──> [settle 1 tick]
```

While the transaction is active (`HasPendingAutoRodLegacyTransaction` /
`IsAutoRodTransactionActive121`), **every other hotbar owner must no-op**.
AutoTool already does this with `input.pauseExclusive`. Copy that pattern; do not
add a second writer that “only swaps if the slot looks wrong.”

AutoTool itself must **not** call `syncCurrentPlayItem` or `clickBlock` from the
JNI worker. Write `currentItem`, set the controller’s cached slot to `-1`, and let
the next client tick emit `C09` in vanilla order. Off-thread held-item or
`START_DESTROY` packets crash and flag.

---

## 3. Canonical Sequences

Keep these short. Exact constructors live in `references/packet_sequences.md`.

### A. Combat attack (only modules that own attacks)

```
weapon/target check ──> held-item sync ──> swing ──> attack packet ──> local hit (1.8.9)
```

1.8.9 KillAura fires this **pre-`C03`** via premotion (`s_inStamp` /
`s_silentEngaged`). Do not invent a second attack path from the render thread.

### B. Auto-block (unblock → attack → re-block)

Attack while the server still thinks the sword is blocking drops or flags.
Release (`C07 RELEASE_USE_ITEM`) before the attack; re-place (`C08` air-click)
only with a sword still in hand.

### C. Hotbar swap + use (AutoRod, and any future “switch, then interact”)

```
record original slot ──> write target slot ──> wait ≥2 ticks ──> use
        ──> wait extension / release ──> restore original ──> settle
```

If AutoTool, AutoTotem, KillAura, or the autoclicker writes the slot during the
wait window, the server sees **use/attack on the wrong item** (Grim BadPackets).
That conflict is why AutoRod pauses AutoTool for the whole cast, including a held
LMB.

### D. Silent rotations

Patch yaw/pitch only on the outbound movement packet (pre-motion). Restore the
local camera before the frame presents. Never drag the OS mouse inside that window.

---

## 4. Concurrency Rules

Full table: `references/conflict_matrix.md`. Non-negotiable rules:

1. **Hotbar lock.** One exclusive owner. AutoRod transaction beats AutoTool swap-to
   and swap-back. AutoTotem must not swap mid-attack or mid-block.
2. **Screen guard.** Combat and AutoRod do not run with a container GUI open.
3. **Do not steal a slot to “help.”** Weapon-swap-on-entity-hover (AutoTool) will
   cancel a rod cast if it is not paused.
4. **KillAura AutoBlock vs placement.** FastPlace/Scaffold/SpeedBridge yield while
   `g_killAuraBlocking` is set.
5. **C# `SendInput` vs native silent aim.** Physical camera (Aim Assist) and
   network yaw (KillAura) stay on separate paths.
6. **Independent wall-clock Hz.** Each module's JNI cadence is its own interval in
   `telemetry_schedule.h`. Do not `Sleep(moduleA || moduleB ? 5 : 50)` — enabling
   one module must not run unrelated scans or `ReadGameState` faster. Do not bind
   module ticks to `wglSwapBuffers` / present. Overlay drawing stays per-frame.

When adding a module, list every resource it writes and every module that already
writes those resources. If that list is empty, the design is unfinished.

---

## 5. JNI Thread & Memory Hygiene

Every `Call*`, `Get*Field`, `NewObject`, `FindClass` is followed by
`ExceptionCheck()` / `ExceptionClear()`.

```cpp
jobject netHandler = env->CallObjectMethod(mc, g_getNetHandlerMethod);
if (env->ExceptionCheck() || !netHandler) {
    env->ExceptionClear();
    return false;
}
```

* `DeleteLocalRef` in loops; `PushLocalFrame` / `PopLocalFrame` around scans.
* `JNIEnv*` is thread-local. Attach worker threads; never share the pointer.
* Do not dispatch packets or gameplay mutations from `wglSwapBuffers` /
  `vkQueuePresentKHR`. Cache field/method IDs on the render thread; mutate on the
  tick / attached worker, or via `Minecraft.addScheduledTask()`.

---

## 6. Dual-Version Mapping

Yarn-first, then Mojmap, in `bridge_261.cpp`:

```cpp
const char* packetNames[] = {
    "net.minecraft.class_2828",                    // 1.21 Yarn (PlayerMoveC2SPacket)
    "net.minecraft.network.protocol.game.ServerboundMovePlayerPacket", // 26.1 Mojmap
    nullptr
};
```

Legacy 1.8.9: `LoadClassWithLoader` then `FindClass`. Prefer shared headers
(`auto_rod_core.h`, `auto_tool_core.h`) plus `McInjector/tests` for sequencing
logic that must stay identical on both bridges.

---

## 7. Pre-Implementation Checklist

- [ ] Which exclusive resources does this module write (slot, use, attack, window, look)?
- [ ] Which existing modules write the same resources, and who yields?
- [ ] If this is a select-then-use flow, is there a tick delay so `C09` lands before use?
- [ ] Are worker-thread JNI writes limited to fields, with vanilla code emitting packets?
- [ ] `ExceptionCheck` / local-ref cleanup / Yarn-then-Mojmap / fail-open on missing maps?
- [ ] Native unit tests for the lockout (see AutoTool `pauseExclusive` tests)?
