// jni_core/session_helper_bridge.cpp
#include "session_helper_bridge.h"
#include "session_switcher.inc"   // kSessionSwitcherClassBytes[], kSessionSwitcherClassLen
#include <cstddef>
#include <string>

namespace SessionHelperBridge {
namespace {

// ── State ─────────────────────────────────────────────────────────────────────

jclass    s_class             = nullptr; // global ref
jmethodID s_init              = nullptr;
jmethodID s_setSession        = nullptr;
jmethodID s_restoreSession    = nullptr;
jmethodID s_joinServer        = nullptr;
jmethodID s_getSessionInfo    = nullptr;
jmethodID s_isReady           = nullptr;
jmethodID s_hint              = nullptr;
jmethodID s_hintConnect       = nullptr;

// ClassLoader.defineClass(String,byte[],int,int) and loadClass(String) both take
// the binary (dotted) name: defineClass runs ClassLoader.checkName(), which throws
// NoClassDefFoundError: IllegalName for a '/'-separated name on every Java 9+ JVM.
// The binary name is also what the class file itself declares.
const char* const kClassNameDotted = "com.aoko.helper.SessionSwitcher";
const char* const kJniFailure = "error:jni_exception";

void ClearIds() {
    s_init = nullptr;
    s_setSession = nullptr;
    s_restoreSession = nullptr;
    s_joinServer = nullptr;
    s_getSessionInfo = nullptr;
    s_isReady = nullptr;
    s_hint = nullptr;
    s_hintConnect = nullptr;
}

// ── String marshalling ────────────────────────────────────────────────────────

// NewStringUTF() consumes modified UTF-8, so non-ASCII bytes would be mangled.
// Usernames, UUIDs, tokens and hostnames are ASCII; reject anything else instead
// of letting a corrupted string reach the JVM.
bool IsAsciiPrintable(const std::string& value) {
    for (std::size_t i = 0; i < value.size(); ++i) {
        const unsigned char c = static_cast<unsigned char>(value[i]);
        if (c < 0x20 || c > 0x7E) return false;
    }
    return true;
}

jstring NewAsciiString(JNIEnv* env, const std::string& value) {
    if (!IsAsciiPrintable(value)) return nullptr;
    jstring result = env->NewStringUTF(value.c_str());
    if (!result && env->ExceptionCheck()) env->ExceptionClear();
    return result;
}

// Copy a Java string and drop the local ref. Returns "" for null/exception.
// GetStringUTFChars would hand back modified UTF-8 (CESU-8, NUL as C0 80); the
// reply encoder expects real UTF-8, so the string is read as UTF-16 and converted.
std::string TakeString(JNIEnv* env, jstring value) {
    if (!value) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return std::string();
    }
    std::string result;
    const jchar* chars = env->GetStringChars(value, nullptr);
    if (chars) {
        result = Utf16ToUtf8(chars, env->GetStringLength(value));
        env->ReleaseStringChars(value, chars);
    } else if (env->ExceptionCheck()) {
        env->ExceptionClear();
    }
    env->DeleteLocalRef(value);
    return result;
}

// Shared tail for every "static String f(...)" call.
std::string CallString(JNIEnv* env, jstring result) {
    if (!result) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return std::string(kJniFailure);
    }
    const std::string text = TakeString(env, result);
    return text.empty() ? std::string(kJniFailure) : text;
}

} // namespace

// ── UTF-16 -> UTF-8 ───────────────────────────────────────────────────────────

namespace {

void AppendUtf8(std::string* out, unsigned int codePoint) {
    if (codePoint < 0x80) {
        *out += static_cast<char>(codePoint);
    } else if (codePoint < 0x800) {
        *out += static_cast<char>(0xC0 | (codePoint >> 6));
        *out += static_cast<char>(0x80 | (codePoint & 0x3F));
    } else if (codePoint < 0x10000) {
        *out += static_cast<char>(0xE0 | (codePoint >> 12));
        *out += static_cast<char>(0x80 | ((codePoint >> 6) & 0x3F));
        *out += static_cast<char>(0x80 | (codePoint & 0x3F));
    } else {
        *out += static_cast<char>(0xF0 | (codePoint >> 18));
        *out += static_cast<char>(0x80 | ((codePoint >> 12) & 0x3F));
        *out += static_cast<char>(0x80 | ((codePoint >> 6) & 0x3F));
        *out += static_cast<char>(0x80 | (codePoint & 0x3F));
    }
}

} // namespace

std::string Utf16ToUtf8(const jchar* chars, jsize length) {
    std::string out;
    if (!chars || length <= 0) return out;
    out.reserve(static_cast<std::size_t>(length));
    for (jsize i = 0; i < length; ++i) {
        unsigned int unit = chars[i];
        if (unit >= 0xD800 && unit <= 0xDBFF && i + 1 < length) {
            const unsigned int low = chars[i + 1];
            if (low >= 0xDC00 && low <= 0xDFFF) {
                AppendUtf8(&out, 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00));
                ++i;
                continue;
            }
        }
        // A lone surrogate has no UTF-8 form; the replacement character keeps the
        // reply valid instead of emitting an invalid sequence.
        if (unit >= 0xD800 && unit <= 0xDFFF) unit = 0xFFFD;
        AppendUtf8(&out, unit);
    }
    return out;
}

// ── getSessionInfo parsing ────────────────────────────────────────────────────

// "ok|username|uuid|inWorld|ready"
// "error|<diagnostic, may contain '|'>|inWorld|ready"
void ParseSessionInfo(const std::string& raw, SessionInfo& out) {
    out = SessionInfo();

    const std::size_t npos = std::string::npos;
    const std::size_t first = raw.find('|');
    if (first == npos) {
        out.error = raw.empty() ? kJniFailure : raw;
        return;
    }

    // Anchor the two flag fields from the right: ready is the last field and
    // inWorld the one before it, so a diagnostic containing '|' cannot shift them.
    const std::size_t last = raw.rfind('|');
    const std::size_t prev = (last != npos && last > 0) ? raw.rfind('|', last - 1) : npos;

    const std::string kind = raw.substr(0, first);
    if (prev == npos || prev < first) {
        // No flag fields: everything after the kind is the diagnostic.
        out.error = raw.substr(first + 1);
        return;
    }

    const std::string payload = (prev > first) ? raw.substr(first + 1, prev - first - 1)
                                               : std::string();
    out.inWorld = raw.substr(prev + 1, last - prev - 1) == "true";
    out.ready = raw.substr(last + 1) == "true";

    if (kind == "ok") {
        const std::size_t nameBar = payload.find('|');
        out.ok = true;
        if (nameBar == std::string::npos) {
            out.username = payload;
        } else {
            out.username = payload.substr(0, nameBar);
            out.uuid = payload.substr(nameBar + 1);
        }
    } else {
        out.error = payload;
    }
}

// ── Load / Unload ─────────────────────────────────────────────────────────────

namespace {

// ClassLoader.defineClass / loadClass ids, resolved once per Load() attempt.
bool ResolveLoaderIds(JNIEnv* env, jmethodID* defineClass, jmethodID* loadClass) {
    jclass clClass = env->FindClass("java/lang/ClassLoader");
    if (!clClass) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return false;
    }
    *defineClass = env->GetMethodID(clClass, "defineClass",
                                    "(Ljava/lang/String;[BII)Ljava/lang/Class;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); *defineClass = nullptr; }
    *loadClass = env->GetMethodID(clClass, "loadClass",
                                  "(Ljava/lang/String;)Ljava/lang/Class;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); *loadClass = nullptr; }
    env->DeleteLocalRef(clClass);
    return *defineClass != nullptr && *loadClass != nullptr;
}

// Resolve the helper through the same loader it would be defined into. FindClass
// cannot do this: it resolves against the class loader of the nearest Java frame,
// and this thread is a native thread attached without one, so it only ever sees
// the bootstrap loader and never the game's.
jclass LoadClassThroughLoader(JNIEnv* env, jobject classLoader, jmethodID loadClass) {
    jstring name = env->NewStringUTF(kClassNameDotted);
    if (!name) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        return nullptr;
    }
    jclass loaded = static_cast<jclass>(env->CallObjectMethod(classLoader, loadClass, name));
    if (env->ExceptionCheck()) { env->ExceptionClear(); loaded = nullptr; }
    env->DeleteLocalRef(name);
    return loaded;
}

} // namespace

bool Load(JNIEnv* env, jobject classLoader) {
    if (IsLoaded()) return true;
    if (!env || !classLoader) return false;

    jmethodID defineClass = nullptr;
    jmethodID loadClass = nullptr;
    if (!ResolveLoaderIds(env, &defineClass, &loadClass)) return false;

    jclass defined = nullptr;
    jbyteArray bytes = env->NewByteArray(static_cast<jsize>(kSessionSwitcherClassLen));
    if (bytes) {
        env->SetByteArrayRegion(bytes, 0, static_cast<jsize>(kSessionSwitcherClassLen),
                                reinterpret_cast<const jbyte*>(kSessionSwitcherClassBytes));
        if (env->ExceptionCheck()) {
            env->ExceptionClear();
            env->DeleteLocalRef(bytes);
            bytes = nullptr;
        }
    } else if (env->ExceptionCheck()) {
        env->ExceptionClear();
    }

    jstring className = bytes ? env->NewStringUTF(kClassNameDotted) : nullptr;
    if (className) {
        defined = static_cast<jclass>(env->CallObjectMethod(
            classLoader, defineClass, className, bytes,
            static_cast<jint>(0), static_cast<jint>(kSessionSwitcherClassLen)));
        if (env->ExceptionCheck()) { env->ExceptionClear(); defined = nullptr; }
        env->DeleteLocalRef(className);
    } else if (env->ExceptionCheck()) {
        env->ExceptionClear();
    }
    if (bytes) env->DeleteLocalRef(bytes);

    if (!defined) {
        // Already defined behind this loader: a retry after an earlier attempt
        // failed past defineClass, or a load after Unload(). Defining twice throws
        // LinkageError, so recover through the same loader instead of retrying the
        // definition forever.
        defined = LoadClassThroughLoader(env, classLoader, loadClass);
    }
    if (!defined) return false;

    // The eight lookups double as the identity check for the embedded class: these
    // exact signatures only resolve on the SessionSwitcher the .inc was built from.
    jmethodID init = env->GetStaticMethodID(defined, "init", "()Ljava/lang/String;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); init = nullptr; }
    jmethodID setSession = env->GetStaticMethodID(defined, "setSession",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); setSession = nullptr; }
    jmethodID restoreSession = env->GetStaticMethodID(defined, "restoreSession",
        "()Ljava/lang/String;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); restoreSession = nullptr; }
    jmethodID joinServer = env->GetStaticMethodID(defined, "joinServer",
        "(Ljava/lang/String;I)Ljava/lang/String;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); joinServer = nullptr; }
    jmethodID getSessionInfo = env->GetStaticMethodID(defined, "getSessionInfo",
        "()Ljava/lang/String;");
    if (env->ExceptionCheck()) { env->ExceptionClear(); getSessionInfo = nullptr; }
    jmethodID isReady = env->GetStaticMethodID(defined, "isReady", "()Z");
    if (env->ExceptionCheck()) { env->ExceptionClear(); isReady = nullptr; }
    jmethodID hint = env->GetStaticMethodID(defined, "hint",
        "(Ljava/lang/String;Ljava/lang/String;)V");
    if (env->ExceptionCheck()) { env->ExceptionClear(); hint = nullptr; }
    jmethodID hintConnect = env->GetStaticMethodID(defined, "hintConnect",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V");
    if (env->ExceptionCheck()) { env->ExceptionClear(); hintConnect = nullptr; }

    if (!init || !setSession || !restoreSession || !joinServer || !getSessionInfo || !isReady) {
        // Cache nothing on a partial load: the next attempt recovers the class
        // through loadClass() and resolves the ids again.
        env->DeleteLocalRef(defined);
        return false;
    }

    s_class = static_cast<jclass>(env->NewGlobalRef(defined));
    env->DeleteLocalRef(defined);
    if (!s_class) return false;

    s_init           = init;
    s_setSession     = setSession;
    s_restoreSession = restoreSession;
    s_joinServer     = joinServer;
    s_getSessionInfo = getSessionInfo;
    s_isReady        = isReady;
    s_hint           = hint;
    s_hintConnect    = hintConnect;
    return true;
}

bool IsLoaded() {
    return s_class != nullptr && s_init != nullptr && s_getSessionInfo != nullptr;
}

void Unload(JNIEnv* env) {
    if (s_class && env) env->DeleteGlobalRef(s_class);
    s_class = nullptr;
    ClearIds();
}

// ── Hints ─────────────────────────────────────────────────────────────────────

bool SetHints(JNIEnv* env, const char* mcClassName, const char* sessionClassName) {
    if (!IsLoaded() || !env || !s_hint) return false;
    if (!mcClassName && !sessionClassName) return false;

    jstring mc = mcClassName ? NewAsciiString(env, mcClassName) : nullptr;
    jstring session = sessionClassName ? NewAsciiString(env, sessionClassName) : nullptr;
    if ((mcClassName && !mc) || (sessionClassName && !session)) {
        if (mc) env->DeleteLocalRef(mc);
        if (session) env->DeleteLocalRef(session);
        return false;
    }

    env->CallStaticVoidMethod(s_class, s_hint, mc, session);
    bool ok = !env->ExceptionCheck();
    if (!ok) env->ExceptionClear();
    if (mc) env->DeleteLocalRef(mc);
    if (session) env->DeleteLocalRef(session);
    return ok;
}

bool SetConnectHints(JNIEnv* env, const char* connectClassName,
                     const char* serverAddressClassName, const char* serverDataClassName) {
    if (!IsLoaded() || !env || !s_hintConnect) return false;
    if (!connectClassName && !serverAddressClassName && !serverDataClassName) return false;

    jstring connect = connectClassName ? NewAsciiString(env, connectClassName) : nullptr;
    jstring address = serverAddressClassName ? NewAsciiString(env, serverAddressClassName) : nullptr;
    jstring data = serverDataClassName ? NewAsciiString(env, serverDataClassName) : nullptr;
    if ((connectClassName && !connect) || (serverAddressClassName && !address)
            || (serverDataClassName && !data)) {
        if (connect) env->DeleteLocalRef(connect);
        if (address) env->DeleteLocalRef(address);
        if (data) env->DeleteLocalRef(data);
        return false;
    }

    env->CallStaticVoidMethod(s_class, s_hintConnect, connect, address, data);
    bool ok = !env->ExceptionCheck();
    if (!ok) env->ExceptionClear();
    if (connect) env->DeleteLocalRef(connect);
    if (address) env->DeleteLocalRef(address);
    if (data) env->DeleteLocalRef(data);
    return ok;
}

// ── Calls ─────────────────────────────────────────────────────────────────────

std::string Init(JNIEnv* env) {
    if (!IsLoaded() || !env) return kJniFailure;
    return CallString(env, static_cast<jstring>(
        env->CallStaticObjectMethod(s_class, s_init)));
}

bool GetSessionInfo(JNIEnv* env, SessionInfo& out) {
    out = SessionInfo();
    if (!IsLoaded() || !env) {
        out.error = kJniFailure;
        return false;
    }
    jstring raw = static_cast<jstring>(env->CallStaticObjectMethod(s_class, s_getSessionInfo));
    if (!raw) {
        if (env->ExceptionCheck()) env->ExceptionClear();
        out.error = kJniFailure;
        return false;
    }
    ParseSessionInfo(TakeString(env, raw), out);
    return out.ok;
}

std::string SetSession(JNIEnv* env, const std::string& name,
                       const std::string& uuid, const std::string& token) {
    if (!IsLoaded() || !env) return kJniFailure;
    if (!IsAsciiPrintable(name) || !IsAsciiPrintable(uuid) || !IsAsciiPrintable(token))
        return "error:invalid_session_data";

    jstring jname = NewAsciiString(env, name);
    jstring juuid = NewAsciiString(env, uuid);
    jstring jtoken = NewAsciiString(env, token);
    if (!jname || !juuid || !jtoken) {
        if (jname) env->DeleteLocalRef(jname);
        if (juuid) env->DeleteLocalRef(juuid);
        if (jtoken) env->DeleteLocalRef(jtoken);
        return "error:invalid_session_data";
    }

    jstring result = static_cast<jstring>(
        env->CallStaticObjectMethod(s_class, s_setSession, jname, juuid, jtoken));

    // Drop the credential strings as soon as the call returns.
    env->DeleteLocalRef(jname);
    env->DeleteLocalRef(juuid);
    env->DeleteLocalRef(jtoken);

    return CallString(env, result);
}

std::string RestoreSession(JNIEnv* env) {
    if (!IsLoaded() || !env) return kJniFailure;
    return CallString(env, static_cast<jstring>(
        env->CallStaticObjectMethod(s_class, s_restoreSession)));
}

std::string JoinServer(JNIEnv* env, const std::string& host, int port) {
    if (!IsLoaded() || !env) return kJniFailure;
    if (port < 1 || port > 65535) return "error:invalid_address";

    jstring jhost = NewAsciiString(env, host);
    if (!jhost) return "error:invalid_address";

    jstring result = static_cast<jstring>(
        env->CallStaticObjectMethod(s_class, s_joinServer, jhost, static_cast<jint>(port)));
    env->DeleteLocalRef(jhost);
    return CallString(env, result);
}

bool IsReady(JNIEnv* env) {
    if (!IsLoaded() || !env) return false;
    jboolean ready = env->CallStaticBooleanMethod(s_class, s_isReady);
    if (env->ExceptionCheck()) { env->ExceptionClear(); return false; }
    return ready == JNI_TRUE;
}

} // namespace SessionHelperBridge
