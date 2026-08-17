# Minecraft Packet Sequences Reference

Class names, constructors, and the select-then-use flow used by AutoRod / AutoTool.
Conflict rules live in `conflict_matrix.md`.

---

## 1. Held-item change (`C09`)

Any hotbar module (AutoRod, AutoTool, AutoTotem, KillAura weapons-only) eventually
produces a carried-item packet. Prefer letting the **client tick** send it.

### Minecraft 1.8.9

* Class: `net.minecraft.network.play.client.C09PacketHeldItemChange`
* Vanilla path: set `InventoryPlayer.currentItem`, then
  `PlayerControllerMP.syncCurrentPlayItem()` on the client tick.
* Bridge pattern for AutoTool: write `currentItem`, set the controller’s cached
  slot field to `-1`, **do not** call `syncCurrentPlayItem` from the JNI worker.

### Minecraft 1.21 / 26.1

* Yarn: `net.minecraft.class_2863` (`UpdateSelectedSlotC2SPacket` /
  `ServerboundSetCarriedItemPacket`)
* Mojmap: `net.minecraft.network.protocol.game.ServerboundSetCarriedItemPacket`

---

## 2. Combat & interaction

### Minecraft 1.8.9

#### `C02PacketUseEntity` (`net.minecraft.network.play.client.C02PacketUseEntity`)
* **Constructor:** `(Lnet/minecraft/entity/Entity;Lnet/minecraft/network/play/client/C02PacketUseEntity$Action;)V`
* **Action Enum:** `INTERACT` (0), `ATTACK` (1), `INTERACT_AT` (2)
* **Fields:** `entityId`, `action`, `hitVec`

#### `C0APacketAnimation`
* **Constructor:** `()V`
* Dispatch **before** `C02 ATTACK`.

KillAura 1.8.9 order (pre-`C03` premotion): swing → `syncCurrentPlayItem` → `C02`
ATTACK → `attackTargetEntityWithCurrentItem`.

### Minecraft 1.21.x (Yarn) & 26.1+ (Mojmap)

#### Attack / interact
* Yarn: `net.minecraft.class_2824` — `method_34206(Entity, sneaking)`
* Mojmap: `net.minecraft.network.protocol.game.ServerboundInteractPacket.attack(Entity, sneaking)`

#### Hand swing
* Yarn: `net.minecraft.class_2879` (`HandSwingC2SPacket`)
* Mojmap: `net.minecraft.network.protocol.game.ServerboundSwingPacket`

---

## 3. Use item / place (AutoRod, AutoBlock, FastPlace)

### AutoRod (both versions) — do not packet-spam this

```
write rod slot ──> wait kSelectToUseTicks (2) ──> SendInput right-click
        ──> wait extension ticks ──> restore original slot ──> settle 1 tick
```

The worker must not emit interaction or held-item packets. Minecraft’s input loop
sends the use after `C09` has had time to land. Constants: `auto_rod_core.h`.

### Minecraft 1.8.9 AutoBlock / place

#### `C07PacketPlayerDigging` (release use)
* Action: `RELEASE_USE_ITEM` (ordinal 5)
* `BlockPos.ORIGIN`, `EnumFacing.DOWN`

#### `C08PacketPlayerBlockPlacement` (reblock / place)
* Sword block: `BlockPos(-1, -1, -1)`, facing `255`, held `ItemSword`

### Modern use / release
* Yarn use: player `interactItem` / `interactBlock` equivalents (`class_1713` click
  types for inventories; item use goes through client interaction manager).
* Mojmap: `ServerboundUseItemPacket` / `ServerboundUseItemOnPacket` /
  `ServerboundPlayerActionPacket` release.

Prefer vanilla `SendInput` or the client interaction manager on the game thread
over constructing these packets from a worker.

---

## 4. Movement & rotation (`C03`)

### Minecraft 1.8.9 subclasses
* `C03PacketPlayer` (onGround)
* `C04PacketPlayerPosition` (`x, y, z, onGround`)
* `C05PacketPlayerLook` (`yaw, pitch, onGround`)
* `C06PacketPlayerPosLook` (`x, y, z, yaw, pitch, onGround`)

Silent rotation fields: `yaw` (`field_149476_e`), `pitch` (`field_149473_f`),
`rotating` (`field_149481_i`).

### Modern
* Yarn: `net.minecraft.class_2828` (`PlayerMoveC2SPacket`)
* Mojmap: `net.minecraft.network.protocol.game.ServerboundMovePlayerPacket`
