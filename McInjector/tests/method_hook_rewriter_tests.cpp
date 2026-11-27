// method_hook_rewriter_tests.cpp
// Pure checks for the class-file rewriter and the visuals hook table. The JVM level checks
// (verification and behaviour) live in tests/java/run_hook_tests.sh and run_real_hook_checks.sh.
#include "../src/main/cpp/visuals_hooks.h"

#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

static int g_failures = 0;
static int g_checks = 0;

#define CHECK(cond) do { ++g_checks; if (!(cond)) { ++g_failures; std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #cond); } } while (0)

// fx.Tiny compiled with javac --release 8 -g:none (427 bytes):
//   public static int add(int a, long b)
//   public float scale(Object o, float x)
//   public void poke(String s)
static const unsigned char kTiny[] = {
    0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x00, 0x00, 0x34, 0x00, 0x14, 0x0A, 0x00,
    0x02, 0x00, 0x03, 0x07, 0x00, 0x04, 0x0C, 0x00, 0x05, 0x00, 0x06, 0x01,
    0x00, 0x10, 0x6A, 0x61, 0x76, 0x61, 0x2F, 0x6C, 0x61, 0x6E, 0x67, 0x2F,
    0x4F, 0x62, 0x6A, 0x65, 0x63, 0x74, 0x01, 0x00, 0x06, 0x3C, 0x69, 0x6E,
    0x69, 0x74, 0x3E, 0x01, 0x00, 0x03, 0x28, 0x29, 0x56, 0x07, 0x00, 0x08,
    0x01, 0x00, 0x22, 0x6A, 0x61, 0x76, 0x61, 0x2F, 0x6C, 0x61, 0x6E, 0x67,
    0x2F, 0x49, 0x6C, 0x6C, 0x65, 0x67, 0x61, 0x6C, 0x41, 0x72, 0x67, 0x75,
    0x6D, 0x65, 0x6E, 0x74, 0x45, 0x78, 0x63, 0x65, 0x70, 0x74, 0x69, 0x6F,
    0x6E, 0x0A, 0x00, 0x07, 0x00, 0x03, 0x07, 0x00, 0x0B, 0x01, 0x00, 0x07,
    0x66, 0x78, 0x2F, 0x54, 0x69, 0x6E, 0x79, 0x01, 0x00, 0x04, 0x43, 0x6F,
    0x64, 0x65, 0x01, 0x00, 0x03, 0x61, 0x64, 0x64, 0x01, 0x00, 0x05, 0x28,
    0x49, 0x4A, 0x29, 0x49, 0x01, 0x00, 0x0D, 0x53, 0x74, 0x61, 0x63, 0x6B,
    0x4D, 0x61, 0x70, 0x54, 0x61, 0x62, 0x6C, 0x65, 0x01, 0x00, 0x05, 0x73,
    0x63, 0x61, 0x6C, 0x65, 0x01, 0x00, 0x16, 0x28, 0x4C, 0x6A, 0x61, 0x76,
    0x61, 0x2F, 0x6C, 0x61, 0x6E, 0x67, 0x2F, 0x4F, 0x62, 0x6A, 0x65, 0x63,
    0x74, 0x3B, 0x46, 0x29, 0x46, 0x01, 0x00, 0x04, 0x70, 0x6F, 0x6B, 0x65,
    0x01, 0x00, 0x15, 0x28, 0x4C, 0x6A, 0x61, 0x76, 0x61, 0x2F, 0x6C, 0x61,
    0x6E, 0x67, 0x2F, 0x53, 0x74, 0x72, 0x69, 0x6E, 0x67, 0x3B, 0x29, 0x56,
    0x00, 0x21, 0x00, 0x0A, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
    0x00, 0x01, 0x00, 0x05, 0x00, 0x06, 0x00, 0x01, 0x00, 0x0C, 0x00, 0x00,
    0x00, 0x11, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x05, 0x2A, 0xB7,
    0x00, 0x01, 0xB1, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x00, 0x0D, 0x00,
    0x0E, 0x00, 0x01, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x37, 0x00, 0x03, 0x00,
    0x05, 0x00, 0x00, 0x00, 0x1B, 0x1A, 0x3E, 0x03, 0x36, 0x04, 0x15, 0x04,
    0x06, 0xA2, 0x00, 0x0E, 0x1D, 0x15, 0x04, 0x60, 0x3E, 0x84, 0x04, 0x01,
    0xA7, 0xFF, 0xF2, 0x1D, 0x1F, 0x88, 0x60, 0xAC, 0x00, 0x00, 0x00, 0x01,
    0x00, 0x0F, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x02, 0xFD, 0x00, 0x05, 0x01,
    0x01, 0xFA, 0x00, 0x10, 0x00, 0x01, 0x00, 0x10, 0x00, 0x11, 0x00, 0x01,
    0x00, 0x0C, 0x00, 0x00, 0x00, 0x23, 0x00, 0x02, 0x00, 0x03, 0x00, 0x00,
    0x00, 0x0C, 0x2B, 0xC7, 0x00, 0x07, 0x24, 0xA7, 0x00, 0x06, 0x24, 0x0D,
    0x6A, 0xAE, 0x00, 0x00, 0x00, 0x01, 0x00, 0x0F, 0x00, 0x00, 0x00, 0x05,
    0x00, 0x02, 0x08, 0x42, 0x02, 0x00, 0x01, 0x00, 0x12, 0x00, 0x13, 0x00,
    0x01, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x22, 0x00, 0x02, 0x00, 0x02, 0x00,
    0x00, 0x00, 0x0D, 0x2B, 0xC7, 0x00, 0x0B, 0xBB, 0x00, 0x07, 0x59, 0xB7,
    0x00, 0x09, 0xBF, 0xB1, 0x00, 0x00, 0x00, 0x01, 0x00, 0x0F, 0x00, 0x00,
    0x00, 0x03, 0x00, 0x01, 0x0C, 0x00, 0x00,
};

static const char* const kAddNames[] = { "add", nullptr };
static const char* const kScaleNames[] = { "scale", nullptr };
static const char* const kPokeNames[] = { "poke", nullptr };

static void TestDescriptors() {
    mhook::detail::Sig s = mhook::detail::ParseDesc("(IJLjava/lang/String;[FD)Ljava/lang/Object;");
    CHECK(s.ok && s.params.size() == 5 && s.params[0].kind == 'I' && s.params[1].kind == 'J' && s.params[2].kind == 'L'
          && s.params[3].kind == 'L' && s.params[4].kind == 'D' && s.ret == 'L' && s.retClass.empty());
    CHECK(mhook::detail::SlotsOf(s, true) == 1 + 1 + 2 + 1 + 1 + 2);
    CHECK(mhook::detail::ErasedDescriptor(s, true, 'Z') == "(Ljava/lang/Object;ILjava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;D)Z" ||
          mhook::detail::ErasedDescriptor(s, true, 'Z') == "(Ljava/lang/Object;IJLjava/lang/Object;Ljava/lang/Object;D)Z");
    mhook::detail::Sig v = mhook::detail::ParseDesc("(Z)Lnet/minecraft/util/Vec3;");
    CHECK(v.ok && v.params[0].kind == 'I' && v.ret == 'L' && v.retClass == "net/minecraft/util/Vec3");
    mhook::detail::Sig arr = mhook::detail::ParseDesc("(FF)[F");
    CHECK(arr.ok && arr.ret == 'L' && arr.retClass == "[F");
    CHECK(!mhook::detail::ParseDesc("(V)V").ok);
    CHECK(!mhook::detail::ParseDesc("(I").ok);
    CHECK(!mhook::detail::ParseDesc("()").ok);
    CHECK(!mhook::detail::ParseDesc("(Lfoo)V").ok);

    CHECK(mhook::detail::ShapeMatches("(Lnet/minecraft/entity/Entity;F)Lnet/minecraft/util/Vec3;", "(L*;F)L*;"));
    CHECK(mhook::detail::ShapeMatches("(FF)[F", "(FF)L*;"));
    CHECK(mhook::detail::ShapeMatches("(F)F", "(F)F"));
    CHECK(!mhook::detail::ShapeMatches("(D)F", "(F)F"));
    CHECK(!mhook::detail::ShapeMatches("(FI)V", "(F)V"));
    CHECK(!mhook::detail::ShapeMatches("(I)V", "(L*;)V"));
    CHECK(!mhook::detail::ShapeMatches("(F)V", "(F)F"));
}

static mhook::HookSpec Spec(const char* const* names, const char* shape, const char* guard, const char* value) {
    mhook::HookSpec s;
    s.methodNames = names; s.shape = shape; s.guardName = guard; s.valueName = value; s.forbidFlags = 0;
    return s;
}

static void TestRewrite() {
    mhook::HookSpec specs[3] = {
        Spec(kAddNames, "(IJ)I", "gAdd", "vAdd"),
        Spec(kScaleNames, "(L*;F)F", "gScale", "vScale"),
        Spec(kPokeNames, "(L*;)V", "gPoke", nullptr),
    };
    mhook::RewriteResult r = mhook::Rewrite(kTiny, sizeof(kTiny), specs, 3, "com/aoko/visuals/Hooks");
    CHECK(r.ok && r.changed && r.hooked == 3 && r.alreadyHooked == 0);
    CHECK(r.bytes.size() > sizeof(kTiny));
    CHECK(r.bytes.size() >= 4 && r.bytes[0] == 0xCA && r.bytes[1] == 0xFE && r.bytes[2] == 0xBA && r.bytes[3] == 0xBE);

    // A second pass over the output is a no-op.
    mhook::RewriteResult again = mhook::Rewrite(r.bytes.data(), r.bytes.size(), specs, 3, "com/aoko/visuals/Hooks");
    CHECK(again.ok && !again.changed && again.hooked == 0 && again.alreadyHooked == 3);

    // The helper owner and the method names are in the new constant pool.
    std::string blob(reinterpret_cast<const char*>(r.bytes.data()), r.bytes.size());
    CHECK(blob.find("com/aoko/visuals/Hooks") != std::string::npos);
    CHECK(blob.find("(IJ)Z") != std::string::npos);
    CHECK(blob.find("(Ljava/lang/Object;F)F") != std::string::npos);
    CHECK(blob.find("(Ljava/lang/Object;Ljava/lang/Object;)V") == std::string::npos);
    CHECK(blob.find("(Ljava/lang/Object;Ljava/lang/Object;)Z") != std::string::npos);

    // Nothing matches: unchanged, still ok.
    mhook::HookSpec none = Spec(kAddNames, "(FF)F", "g", "v");
    mhook::RewriteResult miss = mhook::Rewrite(kTiny, sizeof(kTiny), &none, 1, "x/Y");
    CHECK(miss.ok && !miss.changed && miss.hooked == 0 && miss.alreadyHooked == 0);

    // Mismatched value/void configuration fails closed.
    mhook::HookSpec bad = Spec(kAddNames, "(IJ)I", "g", nullptr);
    mhook::RewriteResult badRes = mhook::Rewrite(kTiny, sizeof(kTiny), &bad, 1, "x/Y");
    CHECK(!badRes.ok && !badRes.error.empty());
    mhook::HookSpec bad2 = Spec(kPokeNames, "(L*;)V", "g", "v");
    CHECK(!mhook::Rewrite(kTiny, sizeof(kTiny), &bad2, 1, "x/Y").ok);

    // Truncated and non-class input fails closed instead of reading out of bounds.
    for (size_t cut : { (size_t)0, (size_t)3, (size_t)10, (size_t)60, sizeof(kTiny) / 2, sizeof(kTiny) - 1 }) {
        mhook::RewriteResult t = mhook::Rewrite(kTiny, cut, specs, 3, "com/aoko/visuals/Hooks");
        CHECK(!t.ok || !t.changed);
    }
    const unsigned char junk[16] = { 1, 2, 3 };
    CHECK(!mhook::Rewrite(junk, sizeof(junk), specs, 3, "x/Y").ok);

    // The forbidden-flags mask skips methods carrying those access bits (public static = 0x0009).
    mhook::HookSpec masked = Spec(kAddNames, "(IJ)I", "g", "v");
    masked.forbidFlags = 0x0008;
    mhook::RewriteResult skipped = mhook::Rewrite(kTiny, sizeof(kTiny), &masked, 1, "x/Y");
    CHECK(skipped.ok && !skipped.changed && skipped.hooked == 0);
}

static void TestHookTable() {
    size_t count = 0;
    for (int g = 0; g < 2; ++g) {
        const visuals::HookClass* classes = visuals::HookClasses(g == 0 ? visuals::Generation::Legacy189 : visuals::Generation::Modern26, &count);
        CHECK(count >= 3);
        for (size_t i = 0; i < count; ++i) {
            CHECK(classes[i].internalName != nullptr && std::strncmp(classes[i].internalName, "net/minecraft/", 14) == 0);
            CHECK(classes[i].count >= 1);
            for (size_t k = 0; k < classes[i].count; ++k) {
                const mhook::HookSpec& s = classes[i].specs[k];
                CHECK(s.methodNames != nullptr && s.methodNames[0] != nullptr);
                CHECK(s.shape != nullptr && s.shape[0] == '(');
                CHECK(s.guardName != nullptr && s.guardName[0] == (g == 0 ? 'l' : 'm') && s.guardName[1] == 'g');
                // Value hooks mirror the guard name with a v, and exist exactly for non-void shapes.
                const bool isVoid = s.shape[std::strlen(s.shape) - 1] == 'V';
                CHECK(isVoid == (s.valueName == nullptr));
                if (s.valueName) CHECK(s.valueName[0] == s.guardName[0] && s.valueName[1] == 'v' && std::strcmp(s.valueName + 2, s.guardName + 2) == 0);
            }
        }
    }
}

int main() {
    TestDescriptors();
    TestRewrite();
    TestHookTable();
    if (g_failures != 0) { std::printf("%d of %d checks failed\n", g_failures, g_checks); return 1; }
    std::printf("method_hook_rewriter_tests OK (%d checks)\n", g_checks);
    return 0;
}
