# Packet Conflict & Module Concurrency Matrix

BadPackets on Grim/Vulcan/Polar is usually two modules writing the same resource
in one tick. Apply these rules to **every** module, not only KillAura.

---

## 1. How to use this matrix

1. List the resources the new module writes (held slot, use-item, attack, window,
   look, break).
2. Find every existing owner of those resources.
3. Pick a single winner for the overlapping window (pause, queue, or disable).
4. Encode the pause as an explicit flag (AutoRod transaction, `pauseExclusive`,
   `s_inStamp`, `g_killAuraBlocking`) — not as “hopefully the other loop is idle.”

---

## 2. Concurrency Conflict Matrix

| Active Module A | Active Module B | Collision Hazard | Arbitration |
| :--- | :--- | :--- | :--- |
| **AutoRod** | **AutoTool** | AutoRod writes the rod slot and waits ~2 ticks before a vanilla right-click. AutoTool weapon-swap (entity hover or held LMB) snaps the slot back, so the use/attack lands on the sword/tool. Grim treats that as BadPackets (wrong held item). | **Exclusive hotbar:** `HasPendingAutoRodLegacyTransaction()` / `IsAutoRodTransactionActive121()`. AutoTool sets `input.pauseExclusive` and must not swap-to or swap-back until the rod restore has settled. |
| **AutoRod** | **Autoclicker / Triggerbot / KillAura** | Left-click or attack during the select-to-use window uses the previous item or attacks while the server still has the old `C09`. | Pause combat and mining owners for the whole AutoRod transaction. AutoRod use is OS right-click on the game loop, not an off-thread `sendUseItem`. |
| **AutoRod** | **AutoTotem** | Totem offhand/hotbar swap during rod select/use/restore. | AutoTotem yields while any exclusive hotbar transaction is active. |
| **AutoTool** | **KillAura / Triggerbot** | AutoTool swaps to a tool or weapon while KillAura is mid swing/`C02` or AutoBlock `C08`. | Do not swap during `s_inStamp` / `g_killAuraBlocking`. Invalidate controller cache (`currentPlayerItem = -1`) and let the client tick emit `C09`; never `syncCurrentPlayItem` from the worker. |
| **AutoTotem** | **KillAura AutoBlock** | Totem swap while the server still sees a sword-block `C08`. | KillAura holds `s_inStamp` / `g_killAuraBlocking`. AutoTotem yields mid-attack and while blocking. |
| **KillAura / Triggerbot** | **Chest Stealer** | `C0A`/`C02` while a container is open (`C0E` clicks). Instant inventory-combat flag. | Combat (and AutoRod) require `currentScreen == null` unless a documented inventory-combat option is on (disallowed on strict servers). |
| **KillAura silent aim** | **Aim Assist `SendInput`** | Network yaw and OS mouse both try to own look. | Aim Assist only moves the physical camera. KillAura only patches outbound `C03` in pre-motion and restores before present. |
| **KillAura AutoBlock** | **FastPlace / Scaffold / SpeedBridge** | Two `C08`s in one tick (air-block vs real place) or sneak/place races. | Placement yields while `g_killAuraBlocking`. SpeedBridge owns sneak; place only after sneak is true for that tick. |
| **Hit Delay Fix** | **KillAura / Triggerbot** | Cooldown reset without a matching raycast/attack this tick. | Hit Delay Fix is not a packet sequencer. Keep resets aligned with a real attack tick. |

---

## 3. Shared State Synchronization Rules

1. **Active item / hotbar lock**
   - At most one exclusive transaction (`AutoRodLegacyTransaction` phases:
     Selected → Used → Restoring → Idle).
   - Other modules read that flag every tick. “Correcting” a slot that looks
     wrong during the wait window is how AutoTool caused rod BadPackets.
   - Prefer field write + cache invalidate over calling `syncCurrentPlayItem`
     off-thread.

2. **Use-item lock**
   - Do not emit use/place while another module owns the hand (AutoRod cast,
     AutoBlock, or player eating).
   - AutoRod must not call `sendUseItem` from the JNI worker; the synthetic
     right-click is handled by Minecraft’s normal input loop.

3. **Window / container lock**
   - Combat, AutoRod, and AutoTool suspend when a GUI is open.

4. **Rotation lockout**
   - `s_silentEngaged` / `s_combatYaw` / `s_combatPitch` only during movement
     packet serialization. Restore local camera before the swap-buffer/present.
