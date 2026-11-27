#pragma once
// visuals_bridge.h
//
// Native side of the cosmetics and sky modules, shared by bridge.dll (1.8.9) and
// bridge_261.dll (26.x). It defines the embedded com.aoko.visuals helper in the game's
// ClassLoader, installs the guard/value hooks through the shared JVMTI host, and pushes the
// loader's settings to the helper. All drawing and game-state work happens in Java on the
// game's own threads; nothing here ticks from the present hook.
//
// Call order: Install() once, SetSettings() whenever a config line arrives, Tick() from a
// JNI-attached worker thread, Shutdown() during detach.

#include "visuals_config.h"

#include <jni.h>
#include <string>

namespace visuals {

void Install(JavaVM* vm, Generation generation, void (*logger)(const std::string&));

// Any thread. Cheap; the work happens on the next Tick.
void SetSettings(const Settings& settings);

// Worker thread with a valid JNIEnv. Returns immediately when both modules are off and
// were never started. Safe to call every pass of the worker loop.
void Tick(JNIEnv* env);

// "off", "ok", "partial:...", "unsupported:..." or "error:..." (sanitised).
std::string Status();

// Turns every override off, unregisters the class-file broker and restores the game's
// vanilla bytecode. The hook callbacks are Java, so a failed restore is harmless.
// Returns true when nothing native still refers to this module.
bool Shutdown(JNIEnv* env);

} // namespace visuals
