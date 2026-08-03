#pragma once

#include "classfile_entry_injector.h"

#include <cstdint>
#include <string>
#include <vector>

namespace effectrewrite {

struct RewriteResult {
    bool ok;
    bool changed;
    int rewrittenCalls;
    std::string error;
    std::vector<unsigned char> bytes;
    RewriteResult() : ok(false), changed(false), rewrittenCalls(0) {}
};

namespace detail {

inline bool IsLivingEntityEffectCheck(const std::vector<cfinject::detail::CpEntry>& cp,
                                      uint16_t ref) {
    using namespace cfinject::detail;
    if (ref >= cp.size() || (cp[ref].tag != 10 && cp[ref].tag != 11)) return false;
    const uint16_t cls = cp[ref].a;
    const uint16_t nat = cp[ref].b;
    if (cls >= cp.size() || nat >= cp.size() || cp[cls].tag != 7 || cp[nat].tag != 12)
        return false;

    const std::string& owner = Utf8(cp, cp[cls].a);
    const std::string& name = Utf8(cp, cp[nat].a);
    const std::string& desc = Utf8(cp, cp[nat].b);
    const bool ownerMatch = owner == "net/minecraft/class_1309" ||
        owner == "net/minecraft/world/entity/LivingEntity" ||
        owner == "net/minecraft/entity/EntityLivingBase";
    const bool nameMatch = name == "method_6059" || name == "hasEffect" ||
        name == "hasStatusEffect" || name == "m_21023_" ||
        name == "isPotionActive" || name == "func_70644_a";
    const bool descriptorMatch = desc.size() > 4 && desc[0] == '(' && desc[1] == 'L' &&
        desc.substr(desc.size() - 3) == ";)Z" && desc.find(';') == desc.size() - 3;
    return ownerMatch && nameMatch && descriptorMatch;
}

inline size_t InstructionLength(const unsigned char* code, size_t length, size_t pc) {
    if (!code || pc >= length) return 0;
    const unsigned char op = code[pc];
    switch (op) {
    case 0x10: case 0x12:
    case 0x15: case 0x16: case 0x17: case 0x18: case 0x19:
    case 0x36: case 0x37: case 0x38: case 0x39: case 0x3a:
    case 0xa9: case 0xbc:
        return pc + 2 <= length ? 2 : 0;
    case 0x11: case 0x13: case 0x14: case 0x84:
    case 0x99: case 0x9a: case 0x9b: case 0x9c: case 0x9d: case 0x9e:
    case 0x9f: case 0xa0: case 0xa1: case 0xa2: case 0xa3: case 0xa4:
    case 0xa5: case 0xa6: case 0xa7: case 0xa8:
    case 0xb2: case 0xb3: case 0xb4: case 0xb5:
    case 0xb6: case 0xb7: case 0xb8:
    case 0xbb: case 0xbd: case 0xc0: case 0xc1:
    case 0xc6: case 0xc7:
        return pc + 3 <= length ? 3 : 0;
    case 0xb9: case 0xba: case 0xc8: case 0xc9:
        return pc + 5 <= length ? 5 : 0;
    case 0xc5:
        return pc + 4 <= length ? 4 : 0;
    case 0xc4: {
        if (pc + 2 > length) return 0;
        const size_t wideLength = code[pc + 1] == 0x84 ? 6 : 4;
        return pc + wideLength <= length ? wideLength : 0;
    }
    case 0xaa: { // tableswitch
        const size_t aligned = (pc + 4) & ~size_t(3);
        if (aligned + 12 > length) return 0;
        const int32_t low = (int32_t)((uint32_t(code[aligned + 4]) << 24) |
            (uint32_t(code[aligned + 5]) << 16) | (uint32_t(code[aligned + 6]) << 8) |
            uint32_t(code[aligned + 7]));
        const int32_t high = (int32_t)((uint32_t(code[aligned + 8]) << 24) |
            (uint32_t(code[aligned + 9]) << 16) | (uint32_t(code[aligned + 10]) << 8) |
            uint32_t(code[aligned + 11]));
        if (high < low) return 0;
        const uint64_t count = uint64_t(uint32_t(high - low)) + 1;
        const uint64_t end = uint64_t(aligned) + 12 + count * 4;
        return end <= length ? size_t(end - pc) : 0;
    }
    case 0xab: { // lookupswitch
        const size_t aligned = (pc + 4) & ~size_t(3);
        if (aligned + 8 > length) return 0;
        const int32_t pairs = (int32_t)((uint32_t(code[aligned + 4]) << 24) |
            (uint32_t(code[aligned + 5]) << 16) | (uint32_t(code[aligned + 6]) << 8) |
            uint32_t(code[aligned + 7]));
        if (pairs < 0) return 0;
        const uint64_t end = uint64_t(aligned) + 8 + uint64_t(pairs) * 8;
        return end <= length ? size_t(end - pc) : 0;
    }
    default:
        return 1;
    }
}

inline bool RewriteCode(const unsigned char* body, size_t bodyLength,
                        const std::vector<cfinject::detail::CpEntry>& cp,
                        uint16_t helperRef, std::vector<unsigned char>& out,
                        int& rewrittenCalls, std::string& error) {
    using namespace cfinject::detail;
    Reader r(body, bodyLength);
    uint16_t maxStack = 0, maxLocals = 0;
    uint32_t codeLength = 0;
    const unsigned char* code = nullptr;
    if (!r.u2(maxStack) || !r.u2(maxLocals) || !r.u4(codeLength) || !r.take(codeLength, code)) {
        error = "truncated Code attribute";
        return false;
    }

    std::vector<unsigned char> patched(code, code + codeLength);
    for (size_t pc = 0; pc < patched.size();) {
        const size_t instructionLength = InstructionLength(patched.data(), patched.size(), pc);
        if (instructionLength == 0) {
            error = "malformed bytecode while scanning effect checks";
            return false;
        }
        if (patched[pc] == 0xb6 && instructionLength == 3) {
            const uint16_t ref = uint16_t((patched[pc + 1] << 8) | patched[pc + 2]);
            if (IsLivingEntityEffectCheck(cp, ref)) {
                patched[pc] = 0xb8; // invokestatic; receiver becomes callback arg 0
                patched[pc + 1] = (unsigned char)(helperRef >> 8);
                patched[pc + 2] = (unsigned char)helperRef;
                ++rewrittenCalls;
            }
        }
        pc += instructionLength;
    }

    PutU2(out, maxStack);
    PutU2(out, maxLocals);
    PutU4(out, codeLength);
    PutBytes(out, patched.data(), patched.size());
    PutBytes(out, body + r.at, bodyLength - r.at);
    return true;
}

} // namespace detail

inline RewriteResult RewriteRendererEffectChecks(
    const unsigned char* data, size_t length,
    const std::string& helperOwner = "lc/aoko/AntiDebuffRenderHook") {
    using namespace cfinject::detail;
    RewriteResult result;
    if (!data || length < 10) {
        result.error = "class file is empty or truncated";
        return result;
    }

    Reader r(data, length);
    uint32_t magic = 0;
    uint16_t minor = 0, major = 0, cpCount = 0;
    if (!r.u4(magic) || magic != 0xCAFEBABE || !r.u2(minor) || !r.u2(major) ||
        !r.u2(cpCount) || cpCount == 0) {
        result.error = "invalid class-file header";
        return result;
    }
    const size_t cpStart = r.at;
    std::vector<CpEntry> cp;
    if (!ParseConstantPool(r, cpCount, cp, result.error)) return result;
    const size_t cpEnd = r.at;

    bool hasTargetRef = false;
    for (uint16_t i = 1; i < cpCount; ++i) {
        if (detail::IsLivingEntityEffectCheck(cp, i)) {
            hasTargetRef = true;
            break;
        }
    }
    if (!hasTargetRef) {
        result.error = "renderer class has no LivingEntity effect check";
        return result;
    }

    const uint16_t ownerUtf = cpCount;
    const uint16_t ownerCls = cpCount + 1;
    const uint16_t nameUtf = cpCount + 2;
    const uint16_t descUtf = cpCount + 3;
    const uint16_t nat = cpCount + 4;
    const uint16_t helperRef = cpCount + 5;
    if ((uint32_t)helperRef + 1 > 65535) {
        result.error = "constant pool is full";
        return result;
    }

    const std::string callbackName = "checkEffect";
    const std::string callbackDesc = "(Ljava/lang/Object;Ljava/lang/Object;)Z";
    std::vector<unsigned char> appended;
    PutU1(appended, 1); PutU2(appended, (uint16_t)helperOwner.size());
    PutBytes(appended, (const unsigned char*)helperOwner.data(), helperOwner.size());
    PutU1(appended, 7); PutU2(appended, ownerUtf);
    PutU1(appended, 1); PutU2(appended, (uint16_t)callbackName.size());
    PutBytes(appended, (const unsigned char*)callbackName.data(), callbackName.size());
    PutU1(appended, 1); PutU2(appended, (uint16_t)callbackDesc.size());
    PutBytes(appended, (const unsigned char*)callbackDesc.data(), callbackDesc.size());
    PutU1(appended, 12); PutU2(appended, nameUtf); PutU2(appended, descUtf);
    PutU1(appended, 10); PutU2(appended, ownerCls); PutU2(appended, nat);

    std::vector<unsigned char> out;
    PutU4(out, magic); PutU2(out, minor); PutU2(out, major); PutU2(out, cpCount + 6);
    PutBytes(out, data + cpStart, cpEnd - cpStart);
    PutBytes(out, appended.data(), appended.size());

    uint16_t access = 0, thisClass = 0, superClass = 0, interfaces = 0;
    if (!r.u2(access) || !r.u2(thisClass) || !r.u2(superClass) || !r.u2(interfaces)) {
        result.error = "truncated class declaration";
        return result;
    }
    PutU2(out, access); PutU2(out, thisClass); PutU2(out, superClass); PutU2(out, interfaces);
    for (uint16_t i = 0; i < interfaces; ++i) {
        uint16_t value = 0;
        if (!r.u2(value)) { result.error = "truncated interfaces"; return result; }
        PutU2(out, value);
    }

    uint16_t fields = 0;
    if (!r.u2(fields)) { result.error = "truncated fields"; return result; }
    PutU2(out, fields);
    for (uint16_t i = 0; i < fields; ++i) {
        if (!CopyMember(r, out, result.error)) return result;
    }

    uint16_t methods = 0;
    if (!r.u2(methods)) { result.error = "truncated methods"; return result; }
    PutU2(out, methods);
    for (uint16_t i = 0; i < methods; ++i) {
        uint16_t methodAccess = 0, methodName = 0, methodDesc = 0, attrs = 0;
        if (!r.u2(methodAccess) || !r.u2(methodName) || !r.u2(methodDesc) || !r.u2(attrs)) {
            result.error = "truncated method";
            return result;
        }
        PutU2(out, methodAccess); PutU2(out, methodName); PutU2(out, methodDesc); PutU2(out, attrs);
        for (uint16_t j = 0; j < attrs; ++j) {
            uint16_t attrName = 0;
            uint32_t attrLength = 0;
            const unsigned char* body = nullptr;
            if (!r.u2(attrName) || !r.u4(attrLength) || !r.take(attrLength, body)) {
                result.error = "truncated method attribute";
                return result;
            }
            if (Utf8(cp, attrName) == "Code") {
                std::vector<unsigned char> rewritten;
                if (!detail::RewriteCode(body, attrLength, cp, helperRef, rewritten,
                                         result.rewrittenCalls, result.error)) return result;
                PutU2(out, attrName); PutU4(out, (uint32_t)rewritten.size());
                PutBytes(out, rewritten.data(), rewritten.size());
            } else {
                PutU2(out, attrName); PutU4(out, attrLength); PutBytes(out, body, attrLength);
            }
        }
    }

    uint16_t classAttrs = 0;
    if (!r.u2(classAttrs)) { result.error = "truncated class attributes"; return result; }
    PutU2(out, classAttrs);
    for (uint16_t i = 0; i < classAttrs; ++i) {
        uint16_t attrName = 0;
        uint32_t attrLength = 0;
        const unsigned char* body = nullptr;
        if (!r.u2(attrName) || !r.u4(attrLength) || !r.take(attrLength, body)) {
            result.error = "truncated class attribute";
            return result;
        }
        PutU2(out, attrName); PutU4(out, attrLength); PutBytes(out, body, attrLength);
    }
    if (r.at != length) {
        result.error = "trailing or malformed class data";
        return result;
    }
    if (result.rewrittenCalls == 0) {
        result.error = "no effect-check invocation was rewritten";
        return result;
    }

    result.ok = true;
    result.changed = true;
    result.bytes.swap(out);
    return result;
}

inline std::vector<unsigned char> BuildNativeEffectCheckClass(
    const std::string& internalName = "lc/aoko/AntiDebuffRenderHook") {
    using namespace cfinject::detail;
    std::vector<unsigned char> out;
    PutU4(out, 0xCAFEBABE); PutU2(out, 0); PutU2(out, 52); PutU2(out, 7);
    PutU1(out, 1); PutU2(out, (uint16_t)internalName.size());
    PutBytes(out, (const unsigned char*)internalName.data(), internalName.size());
    PutU1(out, 7); PutU2(out, 1);
    const std::string objectName = "java/lang/Object";
    PutU1(out, 1); PutU2(out, (uint16_t)objectName.size());
    PutBytes(out, (const unsigned char*)objectName.data(), objectName.size());
    PutU1(out, 7); PutU2(out, 3);
    const std::string callbackName = "checkEffect";
    PutU1(out, 1); PutU2(out, (uint16_t)callbackName.size());
    PutBytes(out, (const unsigned char*)callbackName.data(), callbackName.size());
    const std::string desc = "(Ljava/lang/Object;Ljava/lang/Object;)Z";
    PutU1(out, 1); PutU2(out, (uint16_t)desc.size());
    PutBytes(out, (const unsigned char*)desc.data(), desc.size());
    PutU2(out, 0x0031); PutU2(out, 2); PutU2(out, 4);
    PutU2(out, 0); PutU2(out, 0);
    PutU2(out, 1); PutU2(out, 0x0109); PutU2(out, 5); PutU2(out, 6); PutU2(out, 0);
    PutU2(out, 0);
    return out;
}

} // namespace effectrewrite
