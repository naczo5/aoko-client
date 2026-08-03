#include "../src/main/cpp/effect_check_rewriter.h"

#include <algorithm>
#include <cstdlib>
#include <iostream>
#include <string>
#include <vector>

static void Check(bool value, const char* message) {
    if (!value) {
        std::cerr << "FAILED: " << message << std::endl;
        std::exit(1);
    }
}

static void U1(std::vector<unsigned char>& v, unsigned x) { v.push_back((unsigned char)x); }
static void U2(std::vector<unsigned char>& v, unsigned x) { U1(v, x >> 8); U1(v, x); }
static void U4(std::vector<unsigned char>& v, unsigned x) {
    U1(v, x >> 24); U1(v, x >> 16); U1(v, x >> 8); U1(v, x);
}
static void Utf(std::vector<unsigned char>& v, const std::string& s) {
    U1(v, 1); U2(v, (unsigned)s.size()); v.insert(v.end(), s.begin(), s.end());
}

static std::vector<unsigned char> BuildFixture(
    const std::string& effectMethod,
    const std::string& owner = "net/minecraft/class_1309",
    const std::string& effectDescriptor = "(Lnet/minecraft/class_6880;)Z") {
    std::vector<unsigned char> v;
    U4(v, 0xCAFEBABE); U2(v, 0); U2(v, 52); U2(v, 14);
    Utf(v, "net/minecraft/class_758$class_7286"); U1(v, 7); U2(v, 1);
    Utf(v, "java/lang/Object"); U1(v, 7); U2(v, 3);
    Utf(v, "isActive");
    Utf(v, "(Ljava/lang/Object;Ljava/lang/Object;)Z");
    Utf(v, "Code");
    Utf(v, owner); U1(v, 7); U2(v, 8);
    Utf(v, effectMethod);
    Utf(v, effectDescriptor);
    U1(v, 12); U2(v, 10); U2(v, 11);
    U1(v, 10); U2(v, 9); U2(v, 12);

    U2(v, 0x0021); U2(v, 2); U2(v, 4); U2(v, 0); U2(v, 0);
    U2(v, 1);
    U2(v, 0x0009); U2(v, 5); U2(v, 6); U2(v, 1);
    U2(v, 7); U4(v, 18);
    U2(v, 2); U2(v, 2); U4(v, 6);
    U1(v, 0x2a); U1(v, 0x2b); U1(v, 0xb6); U2(v, 13); U1(v, 0xac);
    U2(v, 0); U2(v, 0);
    U2(v, 0);
    return v;
}

static bool Contains(const std::vector<unsigned char>& bytes, const std::string& text) {
    return std::search(bytes.begin(), bytes.end(), text.begin(), text.end()) != bytes.end();
}

int main() {
    std::vector<unsigned char> fixture = BuildFixture("method_6059");
    effectrewrite::RewriteResult rewritten = effectrewrite::RewriteRendererEffectChecks(
        fixture.data(), fixture.size());
    Check(rewritten.ok && rewritten.changed, "renderer effect check transformed");
    Check(rewritten.rewrittenCalls == 1, "exactly one effect check transformed");
    Check(Contains(rewritten.bytes, "lc/aoko/AntiDebuffRenderHook"), "helper owner appended");

    bool foundStaticCall = false;
    for (size_t i = 0; i < rewritten.bytes.size(); ++i) {
        if (rewritten.bytes[i] == 0xb8) { foundStaticCall = true; break; }
    }
    Check(foundStaticCall, "invokevirtual replaced with invokestatic");

    std::vector<unsigned char> legacyFixture = BuildFixture(
        "func_70644_a", "net/minecraft/entity/EntityLivingBase",
        "(Lnet/minecraft/potion/Potion;)Z");
    effectrewrite::RewriteResult legacy = effectrewrite::RewriteRendererEffectChecks(
        legacyFixture.data(), legacyFixture.size());
    Check(legacy.ok && legacy.rewrittenCalls == 1, "legacy potion check transformed");

    std::vector<unsigned char> wrong = BuildFixture("removeEffect");
    effectrewrite::RewriteResult rejected = effectrewrite::RewriteRendererEffectChecks(
        wrong.data(), wrong.size());
    Check(!rejected.ok, "unrelated LivingEntity method rejected");

    std::vector<unsigned char> helper = effectrewrite::BuildNativeEffectCheckClass();
    Check(helper.size() > 40 && helper[0] == 0xca && helper[1] == 0xfe,
          "native effect-check helper generated");

    std::cout << "effect_check_rewriter_tests: all passed" << std::endl;
    return 0;
}
