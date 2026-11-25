// tests/session_helper_format_tests.cpp
// Guards the SessionSwitcher.getSessionInfo() string contract between
// src/main/java/com/aoko/helper/SessionSwitcher.java and
// src/main/cpp/jni_core/session_helper_bridge.cpp.
//
// Links the real session_helper_bridge.cpp (no JVM is needed: every JNI call goes
// through the JNIEnv function table, which is never dereferenced here) so a format
// drift on either side fails this test instead of silently reporting an empty
// username or the wrong in-world flag.
#include <cstdio>
#include <string>

#include "jni_core/session_helper_bridge.h"

namespace {

int failures = 0;

void Check(const char* raw, bool ok, const char* username, const char* uuid,
           bool inWorld, bool ready, const char* error, bool expectedOk) {
    SessionHelperBridge::SessionInfo info;
    SessionHelperBridge::ParseSessionInfo(raw, info);

    const bool good = info.ok == ok
        && info.username == username
        && info.uuid == uuid
        && info.inWorld == inWorld
        && info.ready == ready
        && info.error == error
        && info.ok == expectedOk;

    std::printf("%s %-58s ok=%d user=[%s] uuid=[%s] inWorld=%d ready=%d err=[%s]\n",
                good ? "PASS" : "FAIL", raw, info.ok ? 1 : 0, info.username.c_str(),
                info.uuid.c_str(), info.inWorld ? 1 : 0, info.ready ? 1 : 0,
                info.error.c_str());
    if (!good) ++failures;
}

// The helper's strings are read as UTF-16 and must reach the reply encoder as real
// UTF-8: GetStringUTFChars (modified UTF-8) plus per-byte escaping produced
// mojibake for any non-ASCII username or diagnostic.
void CheckUtf8Name(const jchar* chars, jsize length, const std::string& expected,
                   const char* label) {
    const std::string actual = SessionHelperBridge::Utf16ToUtf8(chars, length);
    const bool good = actual == expected;
    std::printf("%s %-52s [%s]\n", good ? "PASS" : "FAIL", label, actual.c_str());
    if (!good) ++failures;
}

void CheckUtf8() {
    // ASCII is one byte per unit.
    const jchar ascii[] = { 'N', 'o', 't', 'c', 'h' };
    CheckUtf8Name(ascii, 5, "Notch", "ASCII stays one byte per unit");

    // 'Ü' U+00DC -> C3 9C (two bytes), 'n' U+006E.
    const jchar latin[] = { 0x00DC, 'n' };
    CheckUtf8Name(latin, 2, "\xc3\x9cn", "U+00DC becomes two UTF-8 bytes");

    // U+20AC '€' -> E2 82 AC (three bytes).
    const jchar euro[] = { 0x20AC };
    CheckUtf8Name(euro, 1, "\xe2\x82\xac", "U+20AC becomes three UTF-8 bytes");

    // U+1F600 as a surrogate pair -> F0 9F 98 80 (four bytes, one character).
    const jchar emoji[] = { 0xD83D, 0xDE00 };
    CheckUtf8Name(emoji, 2, "\xf0\x9f\x98\x80", "surrogate pair becomes one 4-byte sequence");

    // A lone high surrogate has no UTF-8 form; U+FFFD instead of invalid bytes.
    const jchar lone[] = { 0xD83D };
    CheckUtf8Name(lone, 1, "\xef\xbf\xbd", "lone surrogate becomes U+FFFD");

    // U+0000 stays a single NUL byte (modified UTF-8 would emit C0 80).
    const jchar nul[] = { 'a', 0x0000, 'b' };
    CheckUtf8Name(nul, 3, std::string("a\0b", 3), "NUL stays one byte");

    CheckUtf8Name(nullptr, 0, "", "null input is empty");
}

} // namespace

int main() {
    // getSessionInfo() success: "ok|username|uuid|inWorld|ready".
    Check("ok|Notch|069a79f444e94726a5befca90e38aaf5|false|true",
          true, "Notch", "069a79f444e94726a5befca90e38aaf5", false, true, "", true);
    Check("ok|Notch|069a79f4-44e9-4726-a5be-fca90e38aaf5|true|true",
          true, "Notch", "069a79f4-44e9-4726-a5be-fca90e38aaf5", true, true, "", true);
    // Null session object: empty name and uuid, flags intact.
    Check("ok|||false|true", true, "", "", false, true, "", true);

    // getSessionInfo() failure: "error|message|inWorld|ready".
    Check("error|Minecraft class not found|false|false",
          false, "", "", false, false, "Minecraft class not found", false);
    Check("error|session/user field failed structural validation|false|false",
          false, "", "", false, false, "session/user field failed structural validation", false);
    // A describeThrowable chain may itself contain '|'.
    Check("error|init failed: Foo: a|b caused by Bar|false|false",
          false, "", "", false, false, "init failed: Foo: a|b caused by Bar", false);
    // Too few fields: everything after the kind is the diagnostic.
    Check("error|no flags here", false, "", "", false, false, "no flags here", false);
    // No separator at all: fail closed, the whole string becomes the diagnostic.
    Check("ok", false, "", "", false, false, "ok", false);
    Check("", false, "", "", false, false, "error:jni_exception", false);

    CheckUtf8();

    if (failures == 0) {
        std::printf("session_helper_format_tests: all checks passed.\n");
        return 0;
    }
    std::fprintf(stderr, "session_helper_format_tests: %d failure(s).\n", failures);
    return 1;
}
