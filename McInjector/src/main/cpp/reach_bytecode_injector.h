#pragma once

#include "classfile_entry_injector.h"

#include <cstdint>
#include <string>
#include <vector>

namespace reachinject {

struct InjectResult {
    bool ok;
    bool changed;
    int injectedMethods;
    std::string error;
    std::vector<unsigned char> bytes;
    InjectResult() : ok(false), changed(false), injectedMethods(0) {}
};

namespace detail {

inline bool IsVoidCallbackRef(const std::vector<cfinject::detail::CpEntry>& cp, uint16_t ref,
                              const std::string& owner, const std::string& method) {
    using namespace cfinject::detail;
    return IsCallbackRef(cp, ref, owner, method, "()V");
}

inline bool ReplaceExtendedReachCode(const unsigned char* data, size_t len, uint16_t methodRef,
                                     const std::vector<cfinject::detail::CpEntry>& cp,
                                     const std::string& owner, const std::string& callback,
                                     std::vector<unsigned char>& out, bool& changed,
                                     std::string& error) {
    using namespace cfinject::detail;
    Reader r(data, len);
    uint16_t maxStack = 0, maxLocals = 0;
    uint32_t codeLen = 0;
    if (!r.u2(maxStack) || !r.u2(maxLocals) || !r.u4(codeLen) || !r.has(codeLen)) {
        error = "truncated Code attribute"; return false;
    }
    const unsigned char* code = nullptr;
    if (!r.take(codeLen, code)) return false;

    // Check if already injected: 0xb8 <methodRef> 0xac (invokestatic helper.extendedReach; ireturn)
    if (codeLen == 4 && code[0] == 0xb8 && code[3] == 0xac) {
        uint16_t existing = (uint16_t)((code[1] << 8) | code[2]);
        if (IsCallbackRef(cp, existing, owner, callback, "()Z")) {
            PutBytes(out, data, len);
            changed = false;
            return true;
        }
    }

    // New code body:
    // 0xb8 <methodRef> (invokestatic helper.extendedReach()Z)
    // 0xac             (ireturn)
    PutU2(out, maxStack < 1 ? 1 : maxStack);
    PutU2(out, maxLocals < 1 ? 1 : maxLocals);
    PutU4(out, 4);
    PutU1(out, 0xb8);
    PutU2(out, methodRef);
    PutU1(out, 0xac);

    // Skip original exception table
    uint16_t exCount = 0;
    if (!r.u2(exCount)) { error = "truncated exception table"; return false; }
    r.at += exCount * 8;
    PutU2(out, 0); // 0 exceptions

    // Skip nested Code attributes; emitting 0 nested attributes is 100% valid and avoids StackMapTable issues
    PutU2(out, 0);

    changed = true;
    return true;
}

inline bool InjectCodeAtEntry(const unsigned char* data, size_t len, uint16_t methodRef,
                              const std::vector<cfinject::detail::CpEntry>& cp,
                              const std::string& owner, const std::string& callback,
                              std::vector<unsigned char>& out, bool& changed,
                              std::string& error) {
    using namespace cfinject::detail;
    Reader r(data, len);
    uint16_t maxStack = 0, maxLocals = 0;
    uint32_t codeLen = 0;
    if (!r.u2(maxStack) || !r.u2(maxLocals) || !r.u4(codeLen) || !r.has(codeLen)) {
        error = "truncated Code attribute"; return false;
    }
    const unsigned char* code = nullptr;
    if (!r.take(codeLen, code)) return false;

    // Check if already injected at offset 0: 0xb8 <methodRef>
    if (codeLen >= 3 && code[0] == 0xb8) {
        uint16_t existing = (uint16_t)((code[1] << 8) | code[2]);
        if (IsVoidCallbackRef(cp, existing, owner, callback)) {
            PutBytes(out, data, len);
            changed = false;
            return true;
        }
    }

    // Insert 0xb8 <methodRef> at offset 0 (3 bytes added)
    PutU2(out, maxStack < 1 ? 1 : maxStack);
    PutU2(out, maxLocals);
    PutU4(out, codeLen + 3);
    PutU1(out, 0xb8); // invokestatic helper.onMouseOver()
    PutU2(out, methodRef);
    PutBytes(out, code, codeLen);

    uint16_t exCount = 0;
    if (!r.u2(exCount)) { error = "truncated exception table"; return false; }
    PutU2(out, exCount);
    for (uint16_t i = 0; i < exCount; ++i) {
        uint16_t start = 0, end = 0, handler = 0, type = 0;
        if (!r.u2(start) || !r.u2(end) || !r.u2(handler) || !r.u2(type)) {
            error = "truncated exception-table entry"; return false;
        }
        PutU2(out, (uint16_t)(start + 3)); PutU2(out, (uint16_t)(end + 3));
        PutU2(out, (uint16_t)(handler + 3)); PutU2(out, type);
    }

    uint16_t attrCount = 0;
    if (!r.u2(attrCount)) { error = "truncated Code attribute table"; return false; }
    PutU2(out, attrCount);
    for (uint16_t i = 0; i < attrCount; ++i) {
        uint16_t nameIndex = 0; uint32_t attrLen = 0;
        if (!r.u2(nameIndex) || !r.u4(attrLen) || !r.has(attrLen)) { error = "truncated nested Code attribute"; return false; }
        const unsigned char* body = nullptr; r.take(attrLen, body);
        const std::string& name = Utf8(cp, nameIndex);
        std::vector<unsigned char> shifted;
        if (name == "LineNumberTable") {
            Reader ar(body, attrLen); uint16_t count = 0;
            if (!ar.u2(count)) { error = "truncated LineNumberTable"; return false; }
            PutU2(shifted, count);
            for (uint16_t j = 0; j < count; ++j) {
                uint16_t start = 0, line = 0;
                if (!ar.u2(start) || !ar.u2(line)) { error = "truncated line entry"; return false; }
                PutU2(shifted, (uint16_t)(start + 3)); PutU2(shifted, line);
            }
        } else if (name == "LocalVariableTable" || name == "LocalVariableTypeTable") {
            Reader ar(body, attrLen); uint16_t count = 0;
            if (!ar.u2(count)) { error = "truncated local-variable table"; return false; }
            PutU2(shifted, count);
            for (uint16_t j = 0; j < count; ++j) {
                uint16_t start = 0, span = 0, ni = 0, di = 0, slot = 0;
                if (!ar.u2(start) || !ar.u2(span) || !ar.u2(ni) || !ar.u2(di) || !ar.u2(slot)) { error = "truncated local-variable entry"; return false; }
                PutU2(shifted, (uint16_t)(start + 3)); PutU2(shifted, span);
                PutU2(shifted, ni); PutU2(shifted, di); PutU2(shifted, slot);
            }
        } else if (name == "StackMapTable") {
            if (!ShiftStackMapFirstFrame(body, attrLen, 3, shifted, error)) return false;
        } else {
            PutBytes(shifted, body, attrLen);
        }
        PutU2(out, nameIndex); PutU4(out, (uint32_t)shifted.size()); PutBytes(out, shifted.data(), shifted.size());
    }
    if (r.at != len) { error = "Code attribute length mismatch"; return false; }
    changed = true;
    return true;
}

inline bool NextInsnSize(const unsigned char* code, uint32_t codeLen, uint32_t pc, uint32_t& size, std::string& error) {
    if (pc >= codeLen) { error = "pc past end of code"; return false; }
    const unsigned char op = code[pc];
    if (op == 0xaa || op == 0xab) {
        error = "switch opcode; refusing rewrite";
        return false;
    }
    if (op == 0xc4) {
        if (pc + 1 >= codeLen) { error = "truncated wide opcode"; return false; }
        size = (code[pc + 1] == 0x84) ? 6u : 4u;
        if (pc + size > codeLen) { error = "truncated wide instruction"; return false; }
        return true;
    }
    static const unsigned char kSizes[256] = {
        /* 0x00-0x0f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
        /* 0x10-0x1f */ 2,3,2,3,3,2,2,2,2,2,1,1,1,1,1,1,
        /* 0x20-0x2f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
        /* 0x30-0x3f */ 1,1,1,1,1,1,2,2,2,2,2,1,1,1,1,1,
        /* 0x40-0x4f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
        /* 0x50-0x5f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
        /* 0x60-0x6f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
        /* 0x70-0x7f */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
        /* 0x80-0x8f */ 1,1,1,1,3,1,1,1,1,1,1,1,1,1,1,1,
        /* 0x90-0x9f */ 1,1,1,1,1,1,1,1,1,3,3,3,3,3,3,3,
        /* 0xa0-0xaf */ 3,3,3,3,3,3,3,3,3,2,0,0,1,1,1,1,
        /* 0xb0-0xbf */ 1,1,3,3,3,3,3,3,3,5,5,3,2,3,1,1,
        /* 0xc0-0xcf */ 3,3,1,1,0,4,3,3,5,5,1,1,1,1,1,1,
        /* 0xd0-0xdf */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
        /* 0xe0-0xef */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
        /* 0xf0-0xff */ 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,
    };
    size = kSizes[op];
    if (size < 1) { error = "unsupported or reserved opcode"; return false; }
    if (pc + size > codeLen) { error = "truncated instruction"; return false; }
    return true;
}

inline bool CollectInsnStarts(const unsigned char* code, uint32_t codeLen,
                              std::vector<uint32_t>& starts, std::string& error) {
    starts.clear();
    uint32_t pc = 0;
    while (pc < codeLen) {
        starts.push_back(pc);
        uint32_t size = 0;
        if (!NextInsnSize(code, codeLen, pc, size, error)) return false;
        pc += size;
    }
    if (pc != codeLen) { error = "instruction walk length mismatch"; return false; }
    return true;
}

inline uint32_t MappedOffset(uint32_t oldOff, const std::vector<uint32_t>& returns) {
    uint32_t extra = 0;
    for (size_t i = 0; i < returns.size(); ++i) {
        if (returns[i] < oldOff) extra += 3;
        else break;
    }
    return oldOff + extra;
}

inline bool CopyVerificationType(cfinject::detail::Reader& r, std::vector<unsigned char>& out,
                                 const std::vector<uint32_t>& returns, std::string& error) {
    using namespace cfinject::detail;
    uint8_t tag = 0;
    if (!r.u1(tag)) { error = "truncated verification type"; return false; }
    PutU1(out, tag);
    if (tag == 7) {
        uint16_t idx = 0;
        if (!r.u2(idx)) { error = "truncated Object verification type"; return false; }
        PutU2(out, idx);
        return true;
    }
    if (tag == 8) {
        uint16_t off = 0;
        if (!r.u2(off)) { error = "truncated Uninitialized verification type"; return false; }
        PutU2(out, (uint16_t)MappedOffset(off, returns));
        return true;
    }
    if (tag > 8) { error = "unknown verification type"; return false; }
    return true;
}

inline bool RemapStackMapTable(const unsigned char* data, size_t len,
                               const std::vector<uint32_t>& returns,
                               std::vector<unsigned char>& out, std::string& error) {
    using namespace cfinject::detail;
    Reader r(data, len);
    uint16_t entries = 0;
    if (!r.u2(entries)) { error = "truncated StackMapTable"; return false; }
    PutU2(out, entries);
    uint32_t absOff = 0;
    uint32_t prevNewAbs = 0;
    for (uint16_t i = 0; i < entries; ++i) {
        uint8_t frame = 0;
        if (!r.u1(frame)) { error = "truncated stack-map frame"; return false; }

        uint32_t oldDelta = 0;
        int kind = 0;
        if (frame <= 63) {
            kind = 0; oldDelta = frame;
        } else if (frame <= 127) {
            kind = 1; oldDelta = (uint32_t)(frame - 64);
        } else if (frame == 247) {
            kind = 2; uint16_t d = 0;
            if (!r.u2(d)) { error = "truncated stack-map delta"; return false; }
            oldDelta = d;
        } else if (frame >= 248 && frame <= 251) {
            kind = 3; uint16_t d = 0;
            if (!r.u2(d)) { error = "truncated stack-map delta"; return false; }
            oldDelta = d;
        } else if (frame >= 252 && frame <= 254) {
            kind = 4; uint16_t d = 0;
            if (!r.u2(d)) { error = "truncated stack-map delta"; return false; }
            oldDelta = d;
        } else if (frame == 255) {
            kind = 5; uint16_t d = 0;
            if (!r.u2(d)) { error = "truncated stack-map delta"; return false; }
            oldDelta = d;
        } else {
            error = "reserved stack-map frame type"; return false;
        }

        if (i == 0) absOff = oldDelta;
        else absOff += oldDelta + 1;
        const uint32_t newAbs = MappedOffset(absOff, returns);
        uint32_t newDelta = (i == 0) ? newAbs : (newAbs - prevNewAbs - 1);
        prevNewAbs = newAbs;
        if (newDelta > 0xffff) { error = "stack-map delta overflow"; return false; }

        auto emitDeltaFrame = [&](uint8_t tag) {
            PutU1(out, tag);
            PutU2(out, (uint16_t)newDelta);
        };

        if (kind == 0) {
            if (newDelta <= 63) PutU1(out, (uint8_t)newDelta);
            else emitDeltaFrame(251);
        } else if (kind == 1) {
            if (newDelta <= 63) PutU1(out, (uint8_t)(64 + newDelta));
            else emitDeltaFrame(247);
            if (!CopyVerificationType(r, out, returns, error)) return false;
        } else if (kind == 2) {
            emitDeltaFrame(247);
            if (!CopyVerificationType(r, out, returns, error)) return false;
        } else if (kind == 3) {
            emitDeltaFrame(frame);
        } else if (kind == 4) {
            emitDeltaFrame(frame);
            const int extras = (int)frame - 251;
            for (int n = 0; n < extras; ++n) {
                if (!CopyVerificationType(r, out, returns, error)) return false;
            }
        } else {
            emitDeltaFrame(255);
            uint16_t nlocals = 0, nstack = 0;
            if (!r.u2(nlocals)) { error = "truncated full_frame locals"; return false; }
            PutU2(out, nlocals);
            for (uint16_t n = 0; n < nlocals; ++n) {
                if (!CopyVerificationType(r, out, returns, error)) return false;
            }
            if (!r.u2(nstack)) { error = "truncated full_frame stack"; return false; }
            PutU2(out, nstack);
            for (uint16_t n = 0; n < nstack; ++n) {
                if (!CopyVerificationType(r, out, returns, error)) return false;
            }
        }
    }
    if (r.at != len) { error = "StackMapTable length mismatch"; return false; }
    return true;
}

inline bool InjectCallbackBeforeReturns(const unsigned char* data, size_t len, uint16_t methodRef,
                                        const std::vector<cfinject::detail::CpEntry>& cp,
                                        const std::string& owner, const std::string& callback,
                                        std::vector<unsigned char>& out, bool& changed,
                                        std::string& error) {
    using namespace cfinject::detail;
    Reader r(data, len);
    uint16_t maxStack = 0, maxLocals = 0;
    uint32_t codeLen = 0;
    if (!r.u2(maxStack) || !r.u2(maxLocals) || !r.u4(codeLen) || !r.has(codeLen)) {
        error = "truncated Code attribute"; return false;
    }
    const unsigned char* code = nullptr;
    if (!r.take(codeLen, code)) return false;

    std::vector<uint32_t> starts;
    if (!CollectInsnStarts(code, codeLen, starts, error)) return false;

    std::vector<uint32_t> returns;
    for (size_t i = 0; i < starts.size(); ++i) {
        if (code[starts[i]] == 0xb1) returns.push_back(starts[i]);
    }
    if (returns.empty()) { error = "no return instruction to hook"; return false; }

    bool already = true;
    for (size_t i = 0; i < returns.size(); ++i) {
        if (returns[i] < 3 || code[returns[i] - 3] != 0xb8) { already = false; break; }
        uint16_t existing = (uint16_t)((code[returns[i] - 2] << 8) | code[returns[i] - 1]);
        if (!IsVoidCallbackRef(cp, existing, owner, callback)) { already = false; break; }
    }
    if (already) {
        PutBytes(out, data, len);
        changed = false;
        return true;
    }

    std::vector<unsigned char> newCode;
    newCode.reserve(codeLen + returns.size() * 3);
    for (size_t i = 0; i < starts.size(); ++i) {
        const uint32_t pc = starts[i];
        uint32_t size = 0;
        if (!NextInsnSize(code, codeLen, pc, size, error)) return false;
        const unsigned char op = code[pc];
        if (op == 0xb1) {
            PutU1(newCode, 0xb8);
            PutU2(newCode, methodRef);
            PutU1(newCode, 0xb1);
            continue;
        }
        const bool isS2 = (op >= 0x99 && op <= 0xa8) || op == 0xc6 || op == 0xc7;
        const bool isS4 = (op == 0xc8 || op == 0xc9);
        if (isS2 || isS4) {
            int32_t oldRel = 0;
            if (isS2) oldRel = (int16_t)((code[pc + 1] << 8) | code[pc + 2]);
            else oldRel = (int32_t)(((uint32_t)code[pc + 1] << 24) | ((uint32_t)code[pc + 2] << 16) |
                                    ((uint32_t)code[pc + 3] << 8) | code[pc + 4]);
            const int32_t oldTarget = (int32_t)pc + oldRel;
            if (oldTarget < 0 || (uint32_t)oldTarget > codeLen) { error = "branch target out of range"; return false; }
            const int32_t newRel = (int32_t)MappedOffset((uint32_t)oldTarget, returns) - (int32_t)MappedOffset(pc, returns);
            PutU1(newCode, op);
            if (isS2) {
                if (newRel < -32768 || newRel > 32767) { error = "branch offset overflow"; return false; }
                PutU2(newCode, (uint16_t)(int16_t)newRel);
            } else {
                PutU4(newCode, (uint32_t)newRel);
            }
            continue;
        }
        PutBytes(newCode, code + pc, size);
    }

    PutU2(out, maxStack);
    PutU2(out, maxLocals);
    PutU4(out, (uint32_t)newCode.size());
    PutBytes(out, newCode.data(), newCode.size());

    uint16_t exCount = 0;
    if (!r.u2(exCount)) { error = "truncated exception table"; return false; }
    PutU2(out, exCount);
    for (uint16_t i = 0; i < exCount; ++i) {
        uint16_t start = 0, end = 0, handler = 0, type = 0;
        if (!r.u2(start) || !r.u2(end) || !r.u2(handler) || !r.u2(type)) {
            error = "truncated exception-table entry"; return false;
        }
        PutU2(out, (uint16_t)MappedOffset(start, returns));
        PutU2(out, (uint16_t)MappedOffset(end, returns));
        PutU2(out, (uint16_t)MappedOffset(handler, returns));
        PutU2(out, type);
    }

    uint16_t attrCount = 0;
    if (!r.u2(attrCount)) { error = "truncated Code attribute table"; return false; }
    PutU2(out, attrCount);
    for (uint16_t i = 0; i < attrCount; ++i) {
        uint16_t nameIndex = 0; uint32_t attrLen = 0;
        if (!r.u2(nameIndex) || !r.u4(attrLen) || !r.has(attrLen)) { error = "truncated nested Code attribute"; return false; }
        const unsigned char* body = nullptr; r.take(attrLen, body);
        const std::string& name = Utf8(cp, nameIndex);
        std::vector<unsigned char> shifted;
        if (name == "LineNumberTable") {
            Reader ar(body, attrLen); uint16_t count = 0;
            if (!ar.u2(count)) { error = "truncated LineNumberTable"; return false; }
            PutU2(shifted, count);
            for (uint16_t j = 0; j < count; ++j) {
                uint16_t start = 0, line = 0;
                if (!ar.u2(start) || !ar.u2(line)) { error = "truncated line entry"; return false; }
                PutU2(shifted, (uint16_t)MappedOffset(start, returns)); PutU2(shifted, line);
            }
        } else if (name == "LocalVariableTable" || name == "LocalVariableTypeTable") {
            Reader ar(body, attrLen); uint16_t count = 0;
            if (!ar.u2(count)) { error = "truncated local-variable table"; return false; }
            PutU2(shifted, count);
            for (uint16_t j = 0; j < count; ++j) {
                uint16_t start = 0, span = 0, ni = 0, di = 0, slot = 0;
                if (!ar.u2(start) || !ar.u2(span) || !ar.u2(ni) || !ar.u2(di) || !ar.u2(slot)) { error = "truncated local-variable entry"; return false; }
                const uint32_t newStart = MappedOffset(start, returns);
                const uint32_t newEnd = MappedOffset((uint32_t)start + span, returns);
                PutU2(shifted, (uint16_t)newStart);
                PutU2(shifted, (uint16_t)(newEnd - newStart));
                PutU2(shifted, ni); PutU2(shifted, di); PutU2(shifted, slot);
            }
        } else if (name == "StackMapTable") {
            if (!RemapStackMapTable(body, attrLen, returns, shifted, error)) return false;
        } else {
            PutBytes(shifted, body, attrLen);
        }
        PutU2(out, nameIndex); PutU4(out, (uint32_t)shifted.size()); PutBytes(out, shifted.data(), shifted.size());
    }
    if (r.at != len) { error = "Code attribute length mismatch"; return false; }
    changed = true;
    return true;
}

} // namespace detail

inline InjectResult InjectExtendedReachHook(const unsigned char* data, size_t len,
                                           const std::vector<std::string>& methodNames,
                                           const std::string& helperOwner,
                                           const std::string& callbackName = "extendedReach") {
    using namespace cfinject::detail;
    InjectResult result;
    if (!data || len < 10) { result.error = "class file is empty or truncated"; return result; }
    Reader r(data, len);
    uint32_t magic = 0; uint16_t minor = 0, major = 0, cpCount = 0;
    if (!r.u4(magic) || magic != 0xCAFEBABE || !r.u2(minor) || !r.u2(major) || !r.u2(cpCount) || cpCount == 0) {
        result.error = "invalid class-file header"; return result;
    }
    size_t cpStart = r.at;
    std::vector<CpEntry> cp;
    if (!ParseConstantPool(r, cpCount, cp, result.error)) return result;
    size_t cpEnd = r.at;

    const std::string callbackDesc = "()Z";
    const uint16_t ownerUtf = cpCount;
    const uint16_t ownerCls = cpCount + 1;
    const uint16_t nameUtf = cpCount + 2;
    const uint16_t descUtf = cpCount + 3;
    const uint16_t nat = cpCount + 4;
    const uint16_t methodRef = cpCount + 5;
    if ((uint32_t)methodRef + 1 > 65535) { result.error = "constant pool is full"; return result; }

    std::vector<unsigned char> appended;
    PutU1(appended, 1); PutU2(appended, (uint16_t)helperOwner.size()); PutBytes(appended, (const unsigned char*)helperOwner.data(), helperOwner.size());
    PutU1(appended, 7); PutU2(appended, ownerUtf);
    PutU1(appended, 1); PutU2(appended, (uint16_t)callbackName.size()); PutBytes(appended, (const unsigned char*)callbackName.data(), callbackName.size());
    PutU1(appended, 1); PutU2(appended, (uint16_t)callbackDesc.size()); PutBytes(appended, (const unsigned char*)callbackDesc.data(), callbackDesc.size());
    PutU1(appended, 12); PutU2(appended, nameUtf); PutU2(appended, descUtf);
    PutU1(appended, 10); PutU2(appended, ownerCls); PutU2(appended, nat);

    std::vector<unsigned char> out;
    PutU4(out, magic); PutU2(out, minor); PutU2(out, major); PutU2(out, cpCount + 6);
    PutBytes(out, data + cpStart, cpEnd - cpStart); PutBytes(out, appended.data(), appended.size());

    uint16_t access = 0, thisClass = 0, superClass = 0, interfaces = 0;
    if (!r.u2(access) || !r.u2(thisClass) || !r.u2(superClass) || !r.u2(interfaces)) { result.error = "truncated class declaration"; return result; }
    std::string thisClassName = Utf8(cp, cp[thisClass].a);
    PutU2(out, access); PutU2(out, thisClass); PutU2(out, superClass); PutU2(out, interfaces);
    for (uint16_t i = 0; i < interfaces; ++i) { uint16_t v = 0; if (!r.u2(v)) { result.error = "truncated interfaces"; return result; } PutU2(out, v); }
    uint16_t fields = 0;
    if (!r.u2(fields)) { result.error = "truncated fields"; return result; }
    PutU2(out, fields);
    for (uint16_t i = 0; i < fields; ++i) if (!CopyMember(r, out, result.error)) return result;

    uint16_t methods = 0;
    if (!r.u2(methods)) { result.error = "truncated methods"; return result; }
    PutU2(out, methods);
    bool matchedCode = false;
    std::vector<std::string> foundZMethods;
    for (uint16_t i = 0; i < methods; ++i) {
        uint16_t ma = 0, mn = 0, md = 0, attrs = 0;
        if (!r.u2(ma) || !r.u2(mn) || !r.u2(md) || !r.u2(attrs)) { result.error = "truncated method"; return result; }
        PutU2(out, ma); PutU2(out, mn); PutU2(out, md); PutU2(out, attrs);
        const std::string& mName = Utf8(cp, mn);
        const std::string& mDesc = Utf8(cp, md);
        if (mDesc == "()Z") {
            foundZMethods.push_back(mName);
        }
        const bool target = NameMatches(mName, methodNames) && mDesc == "()Z";
        for (uint16_t j = 0; j < attrs; ++j) {
            uint16_t ai = 0; uint32_t al = 0; const unsigned char* body = nullptr;
            if (!r.u2(ai) || !r.u4(al) || !r.take(al, body)) { result.error = "truncated method attribute"; return result; }
            if (target && Utf8(cp, ai) == "Code") {
                matchedCode = true;
                std::vector<unsigned char> shifted; bool changed = false;
                if (!detail::ReplaceExtendedReachCode(body, al, methodRef, cp, helperOwner, callbackName, shifted, changed, result.error)) return result;
                PutU2(out, ai); PutU4(out, (uint32_t)shifted.size()); PutBytes(out, shifted.data(), shifted.size());
                if (changed) { ++result.injectedMethods; result.changed = true; }
            } else {
                PutU2(out, ai); PutU4(out, al); PutBytes(out, body, al);
            }
        }
    }
    uint16_t classAttrs = 0;
    if (!r.u2(classAttrs)) { result.error = "truncated class attributes"; return result; }
    PutU2(out, classAttrs);
    for (uint16_t i = 0; i < classAttrs; ++i) {
        uint16_t ai = 0; uint32_t al = 0; const unsigned char* body = nullptr;
        if (!r.u2(ai) || !r.u4(al) || !r.take(al, body)) { result.error = "truncated class attribute"; return result; }
        PutU2(out, ai); PutU4(out, al); PutBytes(out, body, al);
    }
    if (r.at != len) { result.error = "trailing or malformed class data"; return result; }
    if (!result.changed && matchedCode) {
        result.ok = true;
        result.bytes.assign(data, data + len);
        return result;
    }
    if (!result.changed) {
        std::string msg = "no matching method found for extendedReach hook in " + thisClassName + ". Available ()Z methods: [";
        for (size_t k = 0; k < foundZMethods.size(); ++k) {
            if (k > 0) msg += ", ";
            msg += foundZMethods[k];
        }
        msg += "]";
        result.error = msg;
        return result;
    }
    result.ok = true;
    result.bytes.swap(out);
    return result;
}

inline InjectResult InjectReachHookAtEntry(const unsigned char* data, size_t len,
                                          const std::vector<std::string>& methodNames,
                                          const std::string& expectedDesc,
                                          const std::string& helperOwner,
                                          const std::string& callbackName = "onMouseOver") {
    using namespace cfinject::detail;
    InjectResult result;
    if (!data || len < 10) { result.error = "class file is empty or truncated"; return result; }
    Reader r(data, len);
    uint32_t magic = 0; uint16_t minor = 0, major = 0, cpCount = 0;
    if (!r.u4(magic) || magic != 0xCAFEBABE || !r.u2(minor) || !r.u2(major) || !r.u2(cpCount) || cpCount == 0) {
        result.error = "invalid class-file header"; return result;
    }
    size_t cpStart = r.at;
    std::vector<CpEntry> cp;
    if (!ParseConstantPool(r, cpCount, cp, result.error)) return result;
    size_t cpEnd = r.at;

    const std::string callbackDesc = "()V";
    const uint16_t ownerUtf = cpCount;
    const uint16_t ownerCls = cpCount + 1;
    const uint16_t nameUtf = cpCount + 2;
    const uint16_t descUtf = cpCount + 3;
    const uint16_t nat = cpCount + 4;
    const uint16_t methodRef = cpCount + 5;
    if ((uint32_t)methodRef + 1 > 65535) { result.error = "constant pool is full"; return result; }

    std::vector<unsigned char> appended;
    PutU1(appended, 1); PutU2(appended, (uint16_t)helperOwner.size()); PutBytes(appended, (const unsigned char*)helperOwner.data(), helperOwner.size());
    PutU1(appended, 7); PutU2(appended, ownerUtf);
    PutU1(appended, 1); PutU2(appended, (uint16_t)callbackName.size()); PutBytes(appended, (const unsigned char*)callbackName.data(), callbackName.size());
    PutU1(appended, 1); PutU2(appended, (uint16_t)callbackDesc.size()); PutBytes(appended, (const unsigned char*)callbackDesc.data(), callbackDesc.size());
    PutU1(appended, 12); PutU2(appended, nameUtf); PutU2(appended, descUtf);
    PutU1(appended, 10); PutU2(appended, ownerCls); PutU2(appended, nat);

    std::vector<unsigned char> out;
    PutU4(out, magic); PutU2(out, minor); PutU2(out, major); PutU2(out, cpCount + 6);
    PutBytes(out, data + cpStart, cpEnd - cpStart); PutBytes(out, appended.data(), appended.size());

    uint16_t access = 0, thisClass = 0, superClass = 0, interfaces = 0;
    if (!r.u2(access) || !r.u2(thisClass) || !r.u2(superClass) || !r.u2(interfaces)) { result.error = "truncated class declaration"; return result; }
    PutU2(out, access); PutU2(out, thisClass); PutU2(out, superClass); PutU2(out, interfaces);
    for (uint16_t i = 0; i < interfaces; ++i) { uint16_t v = 0; if (!r.u2(v)) { result.error = "truncated interfaces"; return result; } PutU2(out, v); }
    uint16_t fields = 0;
    if (!r.u2(fields)) { result.error = "truncated fields"; return result; }
    PutU2(out, fields);
    for (uint16_t i = 0; i < fields; ++i) if (!CopyMember(r, out, result.error)) return result;

    uint16_t methods = 0;
    if (!r.u2(methods)) { result.error = "truncated methods"; return result; }
    PutU2(out, methods);
    bool matchedCode = false;
    for (uint16_t i = 0; i < methods; ++i) {
        uint16_t ma = 0, mn = 0, md = 0, attrs = 0;
        if (!r.u2(ma) || !r.u2(mn) || !r.u2(md) || !r.u2(attrs)) { result.error = "truncated method"; return result; }
        PutU2(out, ma); PutU2(out, mn); PutU2(out, md); PutU2(out, attrs);
        const bool target = NameMatches(Utf8(cp, mn), methodNames) && (expectedDesc.empty() || Utf8(cp, md) == expectedDesc);
        for (uint16_t j = 0; j < attrs; ++j) {
            uint16_t ai = 0; uint32_t al = 0; const unsigned char* body = nullptr;
            if (!r.u2(ai) || !r.u4(al) || !r.take(al, body)) { result.error = "truncated method attribute"; return result; }
            if (target && Utf8(cp, ai) == "Code") {
                matchedCode = true;
                std::vector<unsigned char> shifted; bool changed = false;
                if (!detail::InjectCodeAtEntry(body, al, methodRef, cp, helperOwner, callbackName, shifted, changed, result.error)) return result;
                PutU2(out, ai); PutU4(out, (uint32_t)shifted.size()); PutBytes(out, shifted.data(), shifted.size());
                if (changed) { ++result.injectedMethods; result.changed = true; }
            } else {
                PutU2(out, ai); PutU4(out, al); PutBytes(out, body, al);
            }
        }
    }
    uint16_t classAttrs = 0;
    if (!r.u2(classAttrs)) { result.error = "truncated class attributes"; return result; }
    PutU2(out, classAttrs);
    for (uint16_t i = 0; i < classAttrs; ++i) {
        uint16_t ai = 0; uint32_t al = 0; const unsigned char* body = nullptr;
        if (!r.u2(ai) || !r.u4(al) || !r.take(al, body)) { result.error = "truncated class attribute"; return result; }
        PutU2(out, ai); PutU4(out, al); PutBytes(out, body, al);
    }
    if (r.at != len) { result.error = "trailing or malformed class data"; return result; }
    if (!result.changed && matchedCode) {
        result.ok = true;
        result.bytes.assign(data, data + len);
        return result;
    }
    if (!result.changed) {
        result.error = "no matching method found for entry hook";
        return result;
    }
    result.ok = true;
    result.bytes.swap(out);
    return result;
}

inline InjectResult InjectReachHookBeforeReturns(const unsigned char* data, size_t len,
                                                const std::vector<std::string>& methodNames,
                                                const std::string& expectedDesc,
                                                const std::string& helperOwner,
                                                const std::string& callbackName = "onMouseOver") {
    using namespace cfinject::detail;
    InjectResult result;
    if (!data || len < 10) { result.error = "class file is empty or truncated"; return result; }
    Reader r(data, len);
    uint32_t magic = 0; uint16_t minor = 0, major = 0, cpCount = 0;
    if (!r.u4(magic) || magic != 0xCAFEBABE || !r.u2(minor) || !r.u2(major) || !r.u2(cpCount) || cpCount == 0) {
        result.error = "invalid class-file header"; return result;
    }
    size_t cpStart = r.at;
    std::vector<CpEntry> cp;
    if (!ParseConstantPool(r, cpCount, cp, result.error)) return result;
    size_t cpEnd = r.at;

    const std::string callbackDesc = "()V";
    const uint16_t ownerUtf = cpCount;
    const uint16_t ownerCls = cpCount + 1;
    const uint16_t nameUtf = cpCount + 2;
    const uint16_t descUtf = cpCount + 3;
    const uint16_t nat = cpCount + 4;
    const uint16_t methodRef = cpCount + 5;
    if ((uint32_t)methodRef + 1 > 65535) { result.error = "constant pool is full"; return result; }

    std::vector<unsigned char> appended;
    PutU1(appended, 1); PutU2(appended, (uint16_t)helperOwner.size()); PutBytes(appended, (const unsigned char*)helperOwner.data(), helperOwner.size());
    PutU1(appended, 7); PutU2(appended, ownerUtf);
    PutU1(appended, 1); PutU2(appended, (uint16_t)callbackName.size()); PutBytes(appended, (const unsigned char*)callbackName.data(), callbackName.size());
    PutU1(appended, 1); PutU2(appended, (uint16_t)callbackDesc.size()); PutBytes(appended, (const unsigned char*)callbackDesc.data(), callbackDesc.size());
    PutU1(appended, 12); PutU2(appended, nameUtf); PutU2(appended, descUtf);
    PutU1(appended, 10); PutU2(appended, ownerCls); PutU2(appended, nat);

    std::vector<unsigned char> out;
    PutU4(out, magic); PutU2(out, minor); PutU2(out, major); PutU2(out, cpCount + 6);
    PutBytes(out, data + cpStart, cpEnd - cpStart); PutBytes(out, appended.data(), appended.size());

    uint16_t access = 0, thisClass = 0, superClass = 0, interfaces = 0;
    if (!r.u2(access) || !r.u2(thisClass) || !r.u2(superClass) || !r.u2(interfaces)) { result.error = "truncated class declaration"; return result; }
    PutU2(out, access); PutU2(out, thisClass); PutU2(out, superClass); PutU2(out, interfaces);
    for (uint16_t i = 0; i < interfaces; ++i) { uint16_t v = 0; if (!r.u2(v)) { result.error = "truncated interfaces"; return result; } PutU2(out, v); }
    uint16_t fields = 0;
    if (!r.u2(fields)) { result.error = "truncated fields"; return result; }
    PutU2(out, fields);
    for (uint16_t i = 0; i < fields; ++i) if (!CopyMember(r, out, result.error)) return result;

    uint16_t methods = 0;
    if (!r.u2(methods)) { result.error = "truncated methods"; return result; }
    PutU2(out, methods);
    bool matchedCode = false;
    for (uint16_t i = 0; i < methods; ++i) {
        uint16_t ma = 0, mn = 0, md = 0, attrs = 0;
        if (!r.u2(ma) || !r.u2(mn) || !r.u2(md) || !r.u2(attrs)) { result.error = "truncated method"; return result; }
        PutU2(out, ma); PutU2(out, mn); PutU2(out, md); PutU2(out, attrs);
        const bool target = NameMatches(Utf8(cp, mn), methodNames) && (expectedDesc.empty() || Utf8(cp, md) == expectedDesc);
        for (uint16_t j = 0; j < attrs; ++j) {
            uint16_t ai = 0; uint32_t al = 0; const unsigned char* body = nullptr;
            if (!r.u2(ai) || !r.u4(al) || !r.take(al, body)) { result.error = "truncated method attribute"; return result; }
            if (target && Utf8(cp, ai) == "Code") {
                matchedCode = true;
                std::vector<unsigned char> shifted; bool changed = false;
                if (!detail::InjectCallbackBeforeReturns(body, al, methodRef, cp, helperOwner, callbackName, shifted, changed, result.error)) return result;
                PutU2(out, ai); PutU4(out, (uint32_t)shifted.size()); PutBytes(out, shifted.data(), shifted.size());
                if (changed) { ++result.injectedMethods; result.changed = true; }
            } else {
                PutU2(out, ai); PutU4(out, al); PutBytes(out, body, al);
            }
        }
    }
    uint16_t classAttrs = 0;
    if (!r.u2(classAttrs)) { result.error = "truncated class attributes"; return result; }
    PutU2(out, classAttrs);
    for (uint16_t i = 0; i < classAttrs; ++i) {
        uint16_t ai = 0; uint32_t al = 0; const unsigned char* body = nullptr;
        if (!r.u2(ai) || !r.u4(al) || !r.take(al, body)) { result.error = "truncated class attribute"; return result; }
        PutU2(out, ai); PutU4(out, al); PutBytes(out, body, al);
    }
    if (r.at != len) { result.error = "trailing or malformed class data"; return result; }
    if (!result.changed && matchedCode) {
        result.ok = true;
        result.bytes.assign(data, data + len);
        return result;
    }
    if (!result.changed) {
        result.error = "no matching method found for return hook";
        return result;
    }
    result.ok = true;
    result.bytes.swap(out);
    return result;
}

inline std::vector<unsigned char> BuildNativeReachCallbackClass(const std::string& internalName = "lc/aoko/NativeReachHook") {
    using namespace cfinject::detail;
    std::vector<unsigned char> out;
    PutU4(out, 0xCAFEBABE); PutU2(out, 0); PutU2(out, 52); PutU2(out, 9);
    PutU1(out, 1); PutU2(out, (uint16_t)internalName.size()); PutBytes(out, (const unsigned char*)internalName.data(), internalName.size());
    PutU1(out, 7); PutU2(out, 1);
    const std::string objectName = "java/lang/Object";
    PutU1(out, 1); PutU2(out, (uint16_t)objectName.size()); PutBytes(out, (const unsigned char*)objectName.data(), objectName.size());
    PutU1(out, 7); PutU2(out, 3);
    const std::string m1Name = "onMouseOver";
    PutU1(out, 1); PutU2(out, (uint16_t)m1Name.size()); PutBytes(out, (const unsigned char*)m1Name.data(), m1Name.size());
    const std::string m1Desc = "()V";
    PutU1(out, 1); PutU2(out, (uint16_t)m1Desc.size()); PutBytes(out, (const unsigned char*)m1Desc.data(), m1Desc.size());
    const std::string m2Name = "extendedReach";
    PutU1(out, 1); PutU2(out, (uint16_t)m2Name.size()); PutBytes(out, (const unsigned char*)m2Name.data(), m2Name.size());
    const std::string m2Desc = "()Z";
    PutU1(out, 1); PutU2(out, (uint16_t)m2Desc.size()); PutBytes(out, (const unsigned char*)m2Desc.data(), m2Desc.size());

    PutU2(out, 0x0031); PutU2(out, 2); PutU2(out, 4); // public final super
    PutU2(out, 0); PutU2(out, 0); // interfaces, fields
    PutU2(out, 2); // 2 methods
    // Method 1: onMouseOver()V
    PutU2(out, 0x0109); PutU2(out, 5); PutU2(out, 6); PutU2(out, 0);
    // Method 2: extendedReach()Z
    PutU2(out, 0x0109); PutU2(out, 7); PutU2(out, 8); PutU2(out, 0);
    PutU2(out, 0); // class attributes
    return out;
}

} // namespace reachinject
