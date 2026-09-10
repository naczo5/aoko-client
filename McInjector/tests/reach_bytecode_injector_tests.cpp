#include "../src/main/cpp/reach_bytecode_injector.h"

#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <string>
#include <vector>

static void Check(bool value, const char* message, const std::string& err = "") {
    if (!value) {
        printf("FAILED: %s (err: %s)\n", message, err.c_str());
        fflush(stdout);
        std::exit(1);
    }
}

static void U1(std::vector<unsigned char>& v, unsigned x) { v.push_back((unsigned char)x); }
static void U2(std::vector<unsigned char>& v, unsigned x) { U1(v, x >> 8); U1(v, x); }
static void U4(std::vector<unsigned char>& v, unsigned x) { U1(v, x >> 24); U1(v, x >> 16); U1(v, x >> 8); U1(v, x); }
static void Utf(std::vector<unsigned char>& v, const std::string& s) { U1(v, 1); U2(v, (unsigned)s.size()); v.insert(v.end(), s.begin(), s.end()); }

static std::vector<unsigned char> BuildTwoReturnFixture() {
    std::vector<unsigned char> v;
    U4(v, 0xCAFEBABE); U2(v, 0); U2(v, 52); U2(v, 9);
    Utf(v, "fixture/Renderer"); U1(v, 7); U2(v, 1);
    Utf(v, "java/lang/Object"); U1(v, 7); U2(v, 3);
    Utf(v, "getMouseOver"); Utf(v, "(F)V"); Utf(v, "Code"); Utf(v, "StackMapTable");
    U2(v, 0x0021); U2(v, 2); U2(v, 4); U2(v, 0); U2(v, 0);
    U2(v, 1);
    U2(v, 0x0001); U2(v, 5); U2(v, 6); U2(v, 1);
    U2(v, 7); U4(v, 27); // Code
    U2(v, 1); U2(v, 2); U4(v, 6);
    U1(v, 0x03); U1(v, 0x9a); U2(v, 4); U1(v, 0xb1); U1(v, 0xb1);
    U2(v, 0); U2(v, 1);
    U2(v, 8); U4(v, 3); U2(v, 1); U1(v, 5); // same_frame at offset 5
    U2(v, 0);
    return v;
}

static bool HasIfneTarget(const std::vector<unsigned char>& bytes, int expectedRel) {
    for (size_t i = 0; i + 3 <= bytes.size(); ++i) {
        if (bytes[i] == 0x9a) {
            int rel = (int)(int16_t)((bytes[i + 1] << 8) | bytes[i + 2]);
            if (rel == expectedRel) return true;
        }
    }
    return false;
}

static std::vector<unsigned char> BuildFixture(const std::string& method, const std::string& desc) {
    std::vector<unsigned char> v;
    U4(v, 0xCAFEBABE); U2(v, 0); U2(v, 52); U2(v, 9);
    Utf(v, "fixture/Renderer"); U1(v, 7); U2(v, 1);
    Utf(v, "java/lang/Object"); U1(v, 7); U2(v, 3);
    Utf(v, method); Utf(v, desc); Utf(v, "Code"); Utf(v, "StackMapTable");
    U2(v, 0x0021); U2(v, 2); U2(v, 4); U2(v, 0); U2(v, 0);
    U2(v, 1); // methods
    U2(v, 0x0001); U2(v, 5); U2(v, 6); U2(v, 1);
    U2(v, 7); U4(v, 22); // Code
    U2(v, 1); U2(v, 2); U4(v, 1); U1(v, 0xb1); // return
    U2(v, 0); U2(v, 1); // exceptions, nested attrs
    U2(v, 8); U4(v, 3); U2(v, 1); U1(v, 0); // first same_frame at offset 0
    U2(v, 0); // class attrs
    return v;
}

static bool HasInvokestaticIreturn(const std::vector<unsigned char>& bytes) {
    for (size_t i = 0; i + 4 <= bytes.size(); ++i) {
        if (bytes[i] == 0xb8 && bytes[i + 3] == 0xac)
            return true;
    }
    return false;
}

static bool HasInvokestaticAtEntry(const std::vector<unsigned char>& bytes) {
    for (size_t i = 0; i + 3 <= bytes.size(); ++i) {
        if (bytes[i] == 0xb8)
            return true;
    }
    return false;
}

int main() {
    printf("Starting reach bytecode injector tests...\n");
    fflush(stdout);

    const std::vector<std::string> erMethods = { "extendedReach", "func_78749_i", "i" };
    std::vector<unsigned char> erFixture = BuildFixture("extendedReach", "()Z");
    reachinject::InjectResult erRes = reachinject::InjectExtendedReachHook(
        erFixture.data(), erFixture.size(), erMethods, "lc/aoko/NativeReachHook", "extendedReach");
    Check(erRes.ok && erRes.changed, "PlayerControllerMP extendedReach hook transformed", erRes.error);
    Check(erRes.injectedMethods == 1, "exactly one method transformed in PlayerControllerMP");
    Check(HasInvokestaticIreturn(erRes.bytes), "invokestatic + ireturn present");

    reachinject::InjectResult erRes2 = reachinject::InjectExtendedReachHook(
        erRes.bytes.data(), erRes.bytes.size(), erMethods, "lc/aoko/NativeReachHook", "extendedReach");
    Check(erRes2.ok && !erRes2.changed, "PlayerControllerMP extendedReach hook idempotent", erRes2.error);
    Check(erRes2.bytes == erRes.bytes, "idempotent bytes unchanged");

    const std::vector<std::string> mcMethods = { "clickMouse", "func_147116_af", "aw" };
    std::vector<unsigned char> mcFixture = BuildFixture("clickMouse", "()V");
    reachinject::InjectResult mcRes = reachinject::InjectReachHookAtEntry(
        mcFixture.data(), mcFixture.size(), mcMethods, "()V", "lc/aoko/NativeReachHook");
    Check(mcRes.ok && mcRes.changed, "Minecraft clickMouse entry hook transformed", mcRes.error);
    Check(mcRes.injectedMethods == 1, "exactly one method transformed in Minecraft");
    Check(HasInvokestaticAtEntry(mcRes.bytes), "invokestatic at entry present");

    reachinject::InjectResult mcRes2 = reachinject::InjectReachHookAtEntry(
        mcRes.bytes.data(), mcRes.bytes.size(), mcMethods, "()V", "lc/aoko/NativeReachHook");
    Check(mcRes2.ok && !mcRes2.changed, "Minecraft clickMouse entry hook idempotent", mcRes2.error);
    Check(mcRes2.bytes == mcRes.bytes, "idempotent bytes unchanged");

    std::vector<unsigned char> helper = reachinject::BuildNativeReachCallbackClass("lc/aoko/NativeReachHook");
    Check(helper.size() > 40 && helper[0] == 0xca && helper[1] == 0xfe, "helper class generated");

    const std::vector<std::string> moMethods = { "getMouseOver", "func_78473_a", "a" };
    std::vector<unsigned char> moFixture = BuildFixture("getMouseOver", "(F)V");
    reachinject::InjectResult moRes = reachinject::InjectReachHookBeforeReturns(
        moFixture.data(), moFixture.size(), moMethods, "(F)V", "lc/aoko/NativeReachHook", "onMouseOver");
    Check(moRes.ok && moRes.changed, "EntityRenderer getMouseOver return hook transformed", moRes.error);
    Check(moRes.injectedMethods == 1, "exactly one method transformed in EntityRenderer");
    Check(HasInvokestaticAtEntry(moRes.bytes), "invokestatic present for getMouseOver hook");

    reachinject::InjectResult moRes2 = reachinject::InjectReachHookBeforeReturns(
        moRes.bytes.data(), moRes.bytes.size(), moMethods, "(F)V", "lc/aoko/NativeReachHook", "onMouseOver");
    Check(moRes2.ok && !moRes2.changed, "EntityRenderer getMouseOver return hook idempotent", moRes2.error);
    Check(moRes2.bytes == moRes.bytes, "idempotent getMouseOver bytes unchanged");

    std::vector<unsigned char> branchFixture = BuildTwoReturnFixture();
    reachinject::InjectResult brRes = reachinject::InjectReachHookBeforeReturns(
        branchFixture.data(), branchFixture.size(), moMethods, "(F)V", "lc/aoko/NativeReachHook", "onMouseOver");
    Check(brRes.ok && brRes.changed, "getMouseOver two-return hook transformed", brRes.error);
    Check(HasIfneTarget(brRes.bytes, 7), "ifne target remapped past inserted callback");

    const char* bfkPaths[] = {
        "../scratch/vanilla_test/bfk.class",
        "../../scratch/vanilla_test/bfk.class",
        "scratch/vanilla_test/bfk.class",
        nullptr
    };
    FILE* bfkFile = nullptr;
    for (int i = 0; bfkPaths[i] && !bfkFile; ++i) bfkFile = std::fopen(bfkPaths[i], "rb");
    if (bfkFile) {
        std::fseek(bfkFile, 0, SEEK_END);
        long sz = std::ftell(bfkFile);
        std::fseek(bfkFile, 0, SEEK_SET);
        Check(sz > 100, "bfk.class has content");
        std::vector<unsigned char> bfk((size_t)sz);
        Check(std::fread(bfk.data(), 1, (size_t)sz, bfkFile) == (size_t)sz, "read bfk.class");
        std::fclose(bfkFile);
        const std::vector<std::string> obf = { "a" };
        reachinject::InjectResult bfkRes = reachinject::InjectReachHookBeforeReturns(
            bfk.data(), bfk.size(), obf, "(F)V", "lc/aoko/NativeReachHook", "onMouseOver");
        Check(bfkRes.ok && bfkRes.changed, "vanilla EntityRenderer.a(F)V return hook transformed", bfkRes.error);
        Check(bfkRes.injectedMethods >= 1, "at least one (F)V method hooked in bfk");
    }

    printf("All reach bytecode injector tests passed!\n");
    fflush(stdout);
    return 0;
}
