#include "../src/main/cpp/classfile_entry_injector.h"

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
static void U4(std::vector<unsigned char>& v, unsigned x) { U1(v, x >> 24); U1(v, x >> 16); U1(v, x >> 8); U1(v, x); }
static void Utf(std::vector<unsigned char>& v, const std::string& s) { U1(v, 1); U2(v, (unsigned)s.size()); v.insert(v.end(), s.begin(), s.end()); }

static std::vector<unsigned char> BuildFixture(const std::string& method, const std::string& desc) {
    std::vector<unsigned char> v;
    U4(v, 0xCAFEBABE); U2(v, 0); U2(v, 52); U2(v, 9);
    Utf(v, "fixture/Connection"); U1(v, 7); U2(v, 1);
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

static bool HasInjectedPrefix(const std::vector<unsigned char>& bytes) {
    for (size_t i = 0; i + 4 <= bytes.size(); ++i)
        if (bytes[i] == 0x2b && bytes[i + 1] == 0xb8)
            return true;
    return false;
}

int main() {
    const std::vector<std::string> names = { "addToSendQueue", "sendPacket", "send" };
    std::vector<unsigned char> fixture = BuildFixture("sendPacket", "(Lnet/minecraft/network/Packet;)V");
    cfinject::InjectResult first = cfinject::InjectPacketCallbackAtEntry(
        fixture.data(), fixture.size(), names, "lc/aoko/NativePacketHook");
    Check(first.ok && first.changed, "fixture transformed");
    Check(first.injectedMethods == 1, "exactly one method transformed");
    Check(HasInjectedPrefix(first.bytes), "aload_1/invokestatic prefix present");

    cfinject::InjectResult second = cfinject::InjectPacketCallbackAtEntry(
        first.bytes.data(), first.bytes.size(), names, "lc/aoko/NativePacketHook");
    Check(second.ok && !second.changed, "transform is idempotent");
    Check(second.bytes == first.bytes, "idempotent bytes unchanged");

    std::vector<unsigned char> wrong = BuildFixture("tick", "()V");
    cfinject::InjectResult rejected = cfinject::InjectPacketCallbackAtEntry(
        wrong.data(), wrong.size(), names, "lc/aoko/NativePacketHook");
    Check(!rejected.ok, "non-packet method rejected");

    std::vector<unsigned char> helper = cfinject::BuildNativeCallbackClass("lc/aoko/NativePacketHook");
    Check(helper.size() > 40 && helper[0] == 0xca && helper[1] == 0xfe, "runtime helper class generated");

    std::cout << "classfile_entry_injector_tests: all passed" << std::endl;
    return 0;
}
