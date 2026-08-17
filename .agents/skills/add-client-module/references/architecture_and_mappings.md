# Aoko Architecture & Version Mappings Reference

This document serves as the canonical technical reference for the dual-architecture design of **aoko-client** and the deep version splits between Minecraft 1.8.9 (legacy) and Minecraft 1.21 / 26.1 / 26.2 (modern).

---

## 1. Project Architecture

* **Aoko (C# / .NET 8 WPF):**
  * External GUI, profile manager (`%AppData%\Aoko\profiles\`), and process injector.
  * Cheat engines (`Clicker.cs`), input simulation via Win32 `SendInput`.
  * TCP Client communicating with the native bridge on `127.0.0.1:25590` via `GameStateClient.cs`.
* **McInjector (C++ Native Bridge DLLs):**
  * `bridge.dll`: Injected into Minecraft 1.8.9 (Lunar / Forge / Vanilla).
  * `bridge_261.dll`: Injected into Minecraft 1.21.x / Lunar 26.1 / Lunar 26.2.
  * Uses JNI to inspect game state directly from the hosting HotSpot JVM.
  * Native / JNI-backed engines (not a complete product list): **KillAura** (premotion, silent rotations, auto-block), **AutoRod** (exclusive hotbar transaction + vanilla right-click), **AutoTool** (slot write, yields to AutoRod via `pauseExclusive`), **AutoTotem**, **Reach**, **Velocity**, **AntiDebuff**, **Nick Hider** (JVMTI), **Hit Delay Fix** (cooldown field write, not a packet sequencer), plus overlay modules (ESP, nametags, BedPlates). See `ModuleCatalog.cs` for the full registry.
  * Hosts TCP server on port `25590`.
  * Hooks OpenGL (`wglSwapBuffers`) and Vulkan (`vkQueuePresentKHR`) with ImGui overlay rendering.

---

## 2. Version Differences & Mapping Systems

### Legacy Bridge (`bridge.dll` - Minecraft 1.8.9)
* **Mappings:** Obfuscated without official Mojang maps. The bridge uses reflection, heuristic string/signature scanning, and LaunchClassLoader lookups.
* **World Structure:** Entities and TileEntities (chests) are stored in flat lists (`playerEntities`, `loadedTileEntityList`) on the `WorldClient` object.
* **Camera:** Projection and ModelView matrices are read from `ActiveRenderInfo` (`MODELVIEW` and `PROJECTION` FloatBuffers). Viewer position is retrieved from `RenderManager`.
* **Menu-Injection Compatibility:** Mappings and features must recover cleanly when injected in the title screen or lobbies, dynamically initializing upon entering a world.

### Modern Bridge (`bridge_261.dll` - Minecraft 1.21 / Lunar 26.1 / Lunar 26.2)
* **Yarn vs. Mojmap Dual Target:**
  * **Minecraft 1.21 (Obfuscated / Fabric):** Uses **Yarn Intermediary** mappings (e.g. `net.minecraft.class_1657`, `method_18798`).
  * **Minecraft 26.1+ (Unobfuscated):** Mojang stopped obfuscating client distributions; natively uses **Official Mojang Mappings** (e.g. `net.minecraft.world.entity.player.Player`, `getDeltaMovement`).
  * **Resolution Pattern:** `bridge_261.cpp` checks Yarn class names first, falling back to Official Mojang names:
    ```cpp
    const char* names[] = {
        "net.minecraft.class_1657",                    // 1.21 Yarn
        "net.minecraft.world.entity.player.Player",     // 26.1 Mojmap
        nullptr
    };
    ```
* **World Structure:** Block entities (chests) are chunk-based. The bridge queries `WorldChunk.blockEntities` (Yarn `field_12833`) Map.
* **Camera Matrices:** `ActiveRenderInfo` is replaced by JOML `Matrix4f` retrieved from `GameRenderer` (Yarn `class_757`) and `Camera` (Yarn `class_4184`).
* **HitResults:** Looking at air returns a `BlockHitResult` with `Type.MISS`.

---

## 3. Internal Game State Modules (Reach & Velocity)

| Module | Modern (1.21 / 26.1) Implementation | Legacy (1.8.9) Implementation |
| :--- | :--- | :--- |
| **Reach** | Exploits `ENTITY_INTERACTION_RANGE` entity attribute. Fetches the player's attribute instance and dynamically invokes `setBaseValue()` via JNI. | Performs math-based raycasting. On mouse click, if an entity is within the extended reach range, overwrites `objectMouseOver` and `pointedEntity`. |
| **Velocity** | Monitors player `hurtTime`. On damage spike, fetches `Vec3` velocity via `getVelocity()`, scales components by configured modifiers, and writes back via `setVelocity()` (`setDeltaMovement`). | Directly scales primitive `motionX`, `motionY`, and `motionZ` fields on `EntityPlayer` upon `hurtTime` spike. |

---

## 4. Renderer Auto-Detection (Vulkan vs. OpenGL in `bridge_261.dll`)

* **OpenGL Path:** Hooks `wglSwapBuffers` in `opengl32.dll`.
* **Vulkan Path:** Hooks `vulkan-1.dll` entry points (`vkCreateInstance`, `vkCreateDevice`, `vkCreateSwapchainKHR`, `vkQueuePresentKHR`, `vkDestroySwapchainKHR`, `vkDestroyDevice`) using a vendored dynamic loader (`-DIMGUI_IMPL_VULKAN_NO_PROTOTYPES`).
* **Arbitration:** Whichever present path fires first claims the session (`RenderBackend_GetActiveKind()`); the alternate path becomes a no-op overlay.
* **Kill-Switch:** Environment variable `AOKO_BRIDGE261_VULKAN=0` forces OpenGL mode.
