---
name: diagnose-bridge-crash
description: >-
  Troubleshooting and root-cause analysis runbook for JVM crashes, JNI faults,
  hook failures, and rendering segmentation faults in McInjector bridges (1.8.9 and 26.1/1.21).
  Use when the game crashes upon injection, during gameplay, or when bridge logs report failures.
---

# Diagnosing Native Bridge & JVM Crashes

Locate logs first, then match the `hs_err` frame to a known JNI or renderer
signature. Mapping misses must fail open; they are not crashes.

---

## 1. Crash log discovery order

### Step 1: Bridge debug logs

Written next to the loaded DLL (`Aoko\bin\Debug\net8.0-windows\` or publish root):

* `bridge_debug.log` — 1.8.9
* `bridge_261_debug.log` — 1.21 / 26.1 / 26.2
* `loader_ui_debug.log` — C# UI lifecycle

```powershell
Get-Content ".\Aoko\bin\Debug\net8.0-windows\bridge_261_debug.log" -Tail 60 -ErrorAction SilentlyContinue
Get-Content ".\Aoko\bin\Debug\net8.0-windows\bridge_debug.log" -Tail 60 -ErrorAction SilentlyContinue
```

### Step 2: JVM crash dumps (`hs_err_pid*.log`)

Search Lunar’s working directory, `%USERPROFILE%\.lunarclient\`, `%TEMP%`, and
the user profile. Newest file wins.

```powershell
Get-ChildItem "$env:USERPROFILE\.lunarclient", "$env:TEMP", "$env:USERPROFILE" -Filter "hs_err_pid*.log" -Recurse -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 5 FullName, LastWriteTime
```

Also look beside the Lunar/Minecraft process cwd (often under
`%USERPROFILE%\.lunarclient\offline\` or the instance folder).

### Step 3: Read the dump

In `hs_err_pid*.log` note:

* `EXCEPTION_ACCESS_VIOLATION` / `0xC0000005` vs Java `EXCEPTION_ACCESS_VIOLATION` in a JNI stub
* **Problematic frame** — `bridge.dll` / `bridge_261.dll` vs `jvm.dll` vs `lwjgl` / `vulkan-1`
* Current thread name (Client thread vs named worker vs render)
* `JNI local refs table full` in the event or nearby log line

A crash in `jni_CallObjectMethod` / `jni_GetObjectField` almost always means a
pending Java exception or a stale local/global ref, not a “bad mapping string.”

---

## 2. Common signatures

### A. Access violation in `bridge*.dll` / JNI stubs

* **Unhandled Java exception:** `ExceptionCheck` + `ExceptionClear` before the next JNI call.
* **Stale local ref:** re-fetch each tick, or `NewGlobalRef` if it must survive frames.
* **Cross-thread `JNIEnv*`:** attach the worker; never cache the env pointer.

### B. Local reference table overflow

Loops over entities, chunks, or packets without `DeleteLocalRef` /
`PushLocalFrame` / `PopLocalFrame`.

### C. Thread not attached

```cpp
JNIEnv* env = nullptr;
jint res = g_jvm->GetEnv((void**)&env, JNI_VERSION_1_8);
if (res == JNI_EDETACHED) {
    g_jvm->AttachCurrentThread((void**)&env, nullptr);
}
```

Do not run packet dispatch or gameplay mutation on `wglSwapBuffers` /
`vkQueuePresentKHR`. Cache IDs there; mutate on tick / attached workers.
See packet-and-call-hygiene.

### D. Vulkan vs OpenGL (Lunar 26.2)

Both present paths must not initialize ImGui. `RenderBackend_GetActiveKind()`
in `render_backend.cpp`; first present wins. Kill-switch:
`AOKO_BRIDGE261_VULKAN=0`.

### E. Mapping / Yarn-Mojmap miss (not a crash)

Unresolved class/method/field → skip the feature and log. Returning a null
`jclass` into `GetMethodID` without a check **is** a crash. Yarn names first,
then Mojmap, in `bridge_261.cpp`.

---

## 3. Diagnostics checklist

- [ ] Latest `hs_err` problematic frame and module identified
- [ ] Bridge log tail around the crash timestamp
- [ ] JNI calls guarded; locals released in loops
- [ ] `JNIEnv*` only on the attached thread that created it
- [ ] Missing maps fail open instead of calling JNI on null IDs
- [ ] Renderer arbitration / Vulkan kill-switch checked if the frame is in present
