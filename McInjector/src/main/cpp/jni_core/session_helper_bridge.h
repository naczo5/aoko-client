#pragma once
// jni_core/session_helper_bridge.h
// Loads com.aoko.helper.SessionSwitcher.class into the game JVM and exposes typed
// wrappers for the account/server commands.
//
// Usage (once, as soon as a game ClassLoader is known — see
// AccountCommandRouter::LoadHelper):
//   SessionHelperBridge::Load(env, gameClassLoader);
//
// Usage (per inbound command, on the loader socket thread):
//   SessionHelperBridge::SessionInfo info;
//   if (!SessionHelperBridge::GetSessionInfo(env, info)) { /* info.error */ }
//
// All wrappers return the helper's own protocol string:
//   "ok", "ok:<detail>" or "error:<code>" (error:jni_exception on a JNI failure).
// Returned strings never contain the access token.

#include <jni.h>
#include <string>

namespace SessionHelperBridge {

// Decoded SessionSwitcher.getSessionInfo() payload.
struct SessionInfo {
    bool        ok = false;
    bool        inWorld = false;
    bool        ready = false;
    std::string username;
    std::string uuid;
    std::string error;   // machine-readable code or diagnostic when ok == false
};

// Define com.aoko.helper.SessionSwitcher in the JVM behind classLoader and cache
// its jmethodIDs. Idempotent: a second call is a no-op once loaded. A second call
// after Unload(), or after a first attempt that failed on a jmethodID, recovers
// the already-defined class through classLoader.loadClass() instead of trying to
// define it twice. Returns true when every jmethodID resolved.
bool Load(JNIEnv* env, jobject classLoader);

// True once Load() has succeeded and the class is still cached.
bool IsLoaded();

// Release the class global ref and cached IDs. Call during DLL detach, after the
// threads that can call into the helper have exited.
void Unload(JNIEnv* env);

// Optional class-name hints. Null/empty entries are ignored. Resets the helper's
// ready cache, so only call after Load() and when the names actually changed.
bool SetHints(JNIEnv* env, const char* mcClassName, const char* sessionClassName);
bool SetConnectHints(JNIEnv* env, const char* connectClassName,
                     const char* serverAddressClassName, const char* serverDataClassName);

// Minecraft/current-session discovery. "ok" or a diagnostic string.
// Deliberately not called at load time: init() also caches the launch session that
// restoreSession() restores to, so it must run at the first user command and not
// while the client may still be replacing its session. getSessionInfo() calls it.
std::string Init(JNIEnv* env);

// Fill out from getSessionInfo(). Returns out.ok.
bool GetSessionInfo(JNIEnv* env, SessionInfo& out);

// Decode the helper's getSessionInfo() string:
//   "ok|username|uuid|inWorld|ready"
//   "error|<diagnostic, may contain '|'>|inWorld|ready"
// Exposed so tests/session_helper_format_tests.cpp can guard the Java/C++ format
// contract without a JVM.
void ParseSessionInfo(const std::string& raw, SessionInfo& out);

// UTF-16 (a Java String) to UTF-8, the conversion every helper string goes
// through. Lone surrogates become U+FFFD. Exposed so the tests can guard it
// without a JVM.
std::string Utf16ToUtf8(const jchar* chars, jsize length);

// Replace the in-game session. "ok:<username>" or "error:<code>".
std::string SetSession(JNIEnv* env, const std::string& name,
                       const std::string& uuid, const std::string& token);

// Restore the session captured at first successful init(). "ok:<username>" or "error:<code>".
std::string RestoreSession(JNIEnv* env);

// Join host:port through ConnectScreen / GuiConnecting. "ok:<host>:<port>" or "error:<code>".
std::string JoinServer(JNIEnv* env, const std::string& host, int port);

// True when the helper resolved Minecraft and the current session.
bool IsReady(JNIEnv* env);

} // namespace SessionHelperBridge
