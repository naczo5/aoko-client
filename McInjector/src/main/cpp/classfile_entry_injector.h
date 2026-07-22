#pragma once

#include <cstdint>
#include <string>
#include <vector>

namespace cfinject {

struct InjectResult {
    bool ok;
    bool changed;
    int injectedMethods;
    std::string error;
    std::vector<unsigned char> bytes;
    InjectResult() : ok(false), changed(false), injectedMethods(0) {}
};

namespace detail {

struct Reader {
    const unsigned char* p;
    size_t n;
    size_t at;
    Reader(const unsigned char* data, size_t size) : p(data), n(size), at(0) {}
    bool has(size_t count) const { return at <= n && count <= n - at; }
    bool u1(uint8_t& v) { if (!has(1)) return false; v = p[at++]; return true; }
    bool u2(uint16_t& v) { if (!has(2)) return false; v = (uint16_t)((p[at] << 8) | p[at + 1]); at += 2; return true; }
    bool u4(uint32_t& v) { if (!has(4)) return false; v = ((uint32_t)p[at] << 24) | ((uint32_t)p[at + 1] << 16) | ((uint32_t)p[at + 2] << 8) | p[at + 3]; at += 4; return true; }
    bool take(size_t count, const unsigned char*& out) { if (!has(count)) return false; out = p + at; at += count; return true; }
};

inline void PutU1(std::vector<unsigned char>& out, uint8_t v) { out.push_back(v); }
inline void PutU2(std::vector<unsigned char>& out, uint16_t v) { out.push_back((unsigned char)(v >> 8)); out.push_back((unsigned char)v); }
inline void PutU4(std::vector<unsigned char>& out, uint32_t v) { out.push_back((unsigned char)(v >> 24)); out.push_back((unsigned char)(v >> 16)); out.push_back((unsigned char)(v >> 8)); out.push_back((unsigned char)v); }
inline void PutBytes(std::vector<unsigned char>& out, const unsigned char* p, size_t n) { out.insert(out.end(), p, p + n); }

struct CpEntry {
    uint8_t tag;
    uint16_t a;
    uint16_t b;
    std::string utf8;
    CpEntry() : tag(0), a(0), b(0) {}
};

inline bool ParseConstantPool(Reader& r, uint16_t count, std::vector<CpEntry>& cp, std::string& error) {
    cp.assign(count, CpEntry());
    for (uint16_t i = 1; i < count; ++i) {
        uint8_t tag = 0;
        if (!r.u1(tag)) { error = "truncated constant-pool tag"; return false; }
        cp[i].tag = tag;
        uint16_t a = 0, b = 0;
        uint32_t u4 = 0;
        const unsigned char* raw = nullptr;
        switch (tag) {
        case 1:
            if (!r.u2(a) || !r.take(a, raw)) { error = "truncated Utf8 constant"; return false; }
            cp[i].utf8.assign((const char*)raw, (size_t)a);
            break;
        case 3: case 4:
            if (!r.u4(u4)) { error = "truncated numeric constant"; return false; }
            break;
        case 5: case 6:
            if (!r.u4(u4) || !r.u4(u4)) { error = "truncated wide constant"; return false; }
            if (++i < count) cp[i].tag = 0;
            break;
        case 7: case 8: case 16: case 19: case 20:
            if (!r.u2(a)) { error = "truncated single-index constant"; return false; }
            cp[i].a = a;
            break;
        case 9: case 10: case 11: case 12: case 17: case 18:
            if (!r.u2(a) || !r.u2(b)) { error = "truncated double-index constant"; return false; }
            cp[i].a = a; cp[i].b = b;
            break;
        case 15: {
            uint8_t kind = 0;
            if (!r.u1(kind) || !r.u2(a)) { error = "truncated method-handle constant"; return false; }
            cp[i].a = a;
            break;
        }
        default:
            error = "unsupported constant-pool tag " + std::to_string((int)tag);
            return false;
        }
    }
    return true;
}

inline const std::string& Utf8(const std::vector<CpEntry>& cp, uint16_t index) {
    static const std::string empty;
    return index < cp.size() && cp[index].tag == 1 ? cp[index].utf8 : empty;
}

inline bool IsCallbackRef(const std::vector<CpEntry>& cp, uint16_t ref,
                          const std::string& owner, const std::string& method,
                          const std::string& desc) {
    if (ref >= cp.size() || cp[ref].tag != 10) return false;
    uint16_t cls = cp[ref].a, nat = cp[ref].b;
    if (cls >= cp.size() || nat >= cp.size() || cp[cls].tag != 7 || cp[nat].tag != 12) return false;
    return Utf8(cp, cp[cls].a) == owner && Utf8(cp, cp[nat].a) == method && Utf8(cp, cp[nat].b) == desc;
}

inline bool ShiftStackMapFirstFrame(const unsigned char* data, size_t len, uint16_t delta,
                                    std::vector<unsigned char>& out, std::string& error) {
    Reader r(data, len);
    uint16_t entries = 0;
    if (!r.u2(entries)) { error = "truncated StackMapTable"; return false; }
    PutU2(out, entries);
    if (entries == 0) return r.at == len;
    uint8_t frame = 0;
    if (!r.u1(frame)) { error = "truncated first stack-map frame"; return false; }
    if (frame <= 63) {
        uint16_t shifted = (uint16_t)frame + delta;
        if (shifted <= 63) PutU1(out, (uint8_t)shifted);
        else { PutU1(out, 251); PutU2(out, shifted); }
    } else if (frame <= 127) {
        uint16_t shifted = (uint16_t)(frame - 64) + delta;
        if (shifted <= 63) PutU1(out, (uint8_t)(64 + shifted));
        else { PutU1(out, 247); PutU2(out, shifted); }
    } else if (frame == 247 || (frame >= 248 && frame <= 255)) {
        uint16_t oldDelta = 0;
        if (!r.u2(oldDelta)) { error = "truncated extended stack-map frame"; return false; }
        PutU1(out, frame);
        PutU2(out, (uint16_t)(oldDelta + delta));
    } else {
        error = "reserved stack-map frame type";
        return false;
    }
    if (!r.has(len - r.at)) { error = "invalid StackMapTable length"; return false; }
    PutBytes(out, data + r.at, len - r.at);
    return true;
}

inline bool ShiftCodeAttribute(const unsigned char* data, size_t len, uint16_t methodRef,
                               const std::vector<CpEntry>& cp,
                               const std::string& owner, const std::string& callback,
                               const std::string& callbackDesc,
                               std::vector<unsigned char>& out, bool& changed,
                               std::string& error) {
    Reader r(data, len);
    uint16_t maxStack = 0, maxLocals = 0;
    uint32_t codeLen = 0;
    if (!r.u2(maxStack) || !r.u2(maxLocals) || !r.u4(codeLen) || !r.has(codeLen)) {
        error = "truncated Code attribute"; return false;
    }
    const unsigned char* code = nullptr;
    if (!r.take(codeLen, code)) return false;
    if (codeLen >= 4 && code[0] == 0x2b && code[1] == 0xb8) {
        uint16_t existing = (uint16_t)((code[2] << 8) | code[3]);
        if (IsCallbackRef(cp, existing, owner, callback, callbackDesc)) {
            PutBytes(out, data, len);
            changed = false;
            return true;
        }
    }

    PutU2(out, maxStack < 1 ? 1 : maxStack);
    PutU2(out, maxLocals);
    PutU4(out, codeLen + 4);
    PutU1(out, 0x2b); // aload_1: first instance-method argument is the packet
    PutU1(out, 0xb8); // invokestatic helper.onPacket(Object)
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
        PutU2(out, (uint16_t)(start + 4)); PutU2(out, (uint16_t)(end + 4));
        PutU2(out, (uint16_t)(handler + 4)); PutU2(out, type);
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
                PutU2(shifted, (uint16_t)(start + 4)); PutU2(shifted, line);
            }
        } else if (name == "LocalVariableTable" || name == "LocalVariableTypeTable") {
            Reader ar(body, attrLen); uint16_t count = 0;
            if (!ar.u2(count)) { error = "truncated local-variable table"; return false; }
            PutU2(shifted, count);
            for (uint16_t j = 0; j < count; ++j) {
                uint16_t start = 0, span = 0, ni = 0, di = 0, slot = 0;
                if (!ar.u2(start) || !ar.u2(span) || !ar.u2(ni) || !ar.u2(di) || !ar.u2(slot)) { error = "truncated local-variable entry"; return false; }
                PutU2(shifted, (uint16_t)(start + 4)); PutU2(shifted, span);
                PutU2(shifted, ni); PutU2(shifted, di); PutU2(shifted, slot);
            }
        } else if (name == "StackMapTable") {
            if (!ShiftStackMapFirstFrame(body, attrLen, 4, shifted, error)) return false;
        } else {
            PutBytes(shifted, body, attrLen);
        }
        PutU2(out, nameIndex); PutU4(out, (uint32_t)shifted.size()); PutBytes(out, shifted.data(), shifted.size());
    }
    if (r.at != len) { error = "Code attribute length mismatch"; return false; }
    changed = true;
    return true;
}

inline bool CopyMember(Reader& r, std::vector<unsigned char>& out, std::string& error) {
    uint16_t access = 0, name = 0, desc = 0, attrs = 0;
    if (!r.u2(access) || !r.u2(name) || !r.u2(desc) || !r.u2(attrs)) { error = "truncated member"; return false; }
    PutU2(out, access); PutU2(out, name); PutU2(out, desc); PutU2(out, attrs);
    for (uint16_t i = 0; i < attrs; ++i) {
        uint16_t ai = 0; uint32_t al = 0; const unsigned char* body = nullptr;
        if (!r.u2(ai) || !r.u4(al) || !r.take(al, body)) { error = "truncated member attribute"; return false; }
        PutU2(out, ai); PutU4(out, al); PutBytes(out, body, al);
    }
    return true;
}

inline bool NameMatches(const std::string& name, const std::vector<std::string>& names) {
    for (size_t i = 0; i < names.size(); ++i) if (name == names[i]) return true;
    return false;
}

inline bool PacketDescriptor(const std::string& desc) {
    if (desc.size() < 5 || desc[0] != '(' || desc[1] != 'L' || desc.substr(desc.size() - 2) != ")V") return false;
    size_t semi = desc.find(';', 2);
    if (semi == std::string::npos) return false;
    std::string first = desc.substr(2, semi - 2);
    return first.find("Packet") != std::string::npos || first.find("packet") != std::string::npos ||
           first.find("class_2596") != std::string::npos;
}

} // namespace detail

inline InjectResult InjectPacketCallbackAtEntry(const unsigned char* data, size_t len,
                                                const std::vector<std::string>& methodNames,
                                                const std::string& helperOwner,
                                                const std::string& callbackName = "onPacket",
                                                const std::string& callbackDesc = "(Ljava/lang/Object;)V") {
    using namespace detail;
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
        const bool target = (ma & 0x0008) == 0 && NameMatches(Utf8(cp, mn), methodNames) && PacketDescriptor(Utf8(cp, md));
        for (uint16_t j = 0; j < attrs; ++j) {
            uint16_t ai = 0; uint32_t al = 0; const unsigned char* body = nullptr;
            if (!r.u2(ai) || !r.u4(al) || !r.take(al, body)) { result.error = "truncated method attribute"; return result; }
            if (target && Utf8(cp, ai) == "Code") {
                matchedCode = true;
                std::vector<unsigned char> shifted; bool changed = false;
                if (!ShiftCodeAttribute(body, al, methodRef, cp, helperOwner, callbackName, callbackDesc, shifted, changed, result.error)) return result;
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
        result.error = "no validated packet-send method matched";
        return result;
    }
    result.ok = true;
    result.bytes.swap(out);
    return result;
}

inline std::vector<unsigned char> BuildNativeCallbackClass(const std::string& internalName,
                                                           const std::string& callbackName = "onPacket") {
    using namespace detail;
    std::vector<unsigned char> out;
    PutU4(out, 0xCAFEBABE); PutU2(out, 0); PutU2(out, 52); PutU2(out, 7);
    PutU1(out, 1); PutU2(out, (uint16_t)internalName.size()); PutBytes(out, (const unsigned char*)internalName.data(), internalName.size());
    PutU1(out, 7); PutU2(out, 1);
    const std::string objectName = "java/lang/Object";
    PutU1(out, 1); PutU2(out, (uint16_t)objectName.size()); PutBytes(out, (const unsigned char*)objectName.data(), objectName.size());
    PutU1(out, 7); PutU2(out, 3);
    PutU1(out, 1); PutU2(out, (uint16_t)callbackName.size()); PutBytes(out, (const unsigned char*)callbackName.data(), callbackName.size());
    const std::string desc = "(Ljava/lang/Object;)V";
    PutU1(out, 1); PutU2(out, (uint16_t)desc.size()); PutBytes(out, (const unsigned char*)desc.data(), desc.size());
    PutU2(out, 0x0031); PutU2(out, 2); PutU2(out, 4); // public final super
    PutU2(out, 0); PutU2(out, 0); // interfaces, fields
    PutU2(out, 1); PutU2(out, 0x0109); PutU2(out, 5); PutU2(out, 6); PutU2(out, 0); // public static native
    PutU2(out, 0); // class attributes
    return out;
}

} // namespace cfinject
