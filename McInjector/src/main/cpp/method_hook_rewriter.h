#pragma once
// method_hook_rewriter.h
//
// Pure class-file rewriter for the visuals hooks. No JVM, no Windows headers, so it
// is unit-testable and can be exercised against real Minecraft classes on any host.
//
// A hook prepends this to the body of one method (the only shape JVMTI
// RetransformClasses allows: no new or renamed methods, no signature changes):
//
//     if (Helper.guard(self?, args...)) {
//         return Helper.value(self?, args...);   // or just `return;` for void methods
//     }
//     <original body>
//
// Every reference-typed argument (and `this`) is passed to the helper as Object, so
// the helper never links against game types and works under any name mapping.
// Primitive arguments keep their type. For reference returns the helper's value is
// checkcast to the method's declared return type.
//
// The prologue is padded to a multiple of four bytes so switch padding in the original
// code stays aligned, every absolute code offset is shifted (exception table,
// LineNumberTable, LocalVariable[Type]Table, StackMapTable including Uninitialized
// offsets), and a StackMapTable frame is added for the first original instruction.
// Any attribute whose offsets this file cannot shift makes the rewrite fail closed.

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

namespace mhook {

struct HookSpec {
    // Null-terminated candidate method names (MCP, SRG, Mojmap, ...). First match wins.
    const char* const* methodNames;
    // Erased shape. Primitive descriptors must match exactly. "L*;" matches any
    // reference parameter or return type, including arrays. Example: "(L*;F)L*;".
    const char* shape;
    // Helper static method names. `guardName` is `(erased args)Z`. `valueName` is
    // `(erased args)<erased return>` and must be null for void methods.
    const char* guardName;
    const char* valueName;
    // Access-flag bits that disqualify a method. Use 0x1040 (ACC_SYNTHETIC | ACC_BRIDGE)
    // to skip the compiler generated bridge of a covariant override so a hook fires once.
    unsigned forbidFlags;
};

struct RewriteResult {
    bool ok;
    bool changed;
    int hooked;                 // methods rewritten by this call
    int alreadyHooked;          // methods that already carried this hook
    std::string error;
    std::vector<unsigned char> bytes;
    RewriteResult() : ok(false), changed(false), hooked(0), alreadyHooked(0) {}
};

namespace detail {

struct Cursor {
    const unsigned char* p;
    size_t n;
    size_t at;
    Cursor(const unsigned char* d, size_t len) : p(d), n(len), at(0) {}
    bool has(size_t c) const { return at <= n && c <= n - at; }
    bool u1(uint8_t& v) { if (!has(1)) return false; v = p[at++]; return true; }
    bool u2(uint16_t& v) { if (!has(2)) return false; v = (uint16_t)((p[at] << 8) | p[at + 1]); at += 2; return true; }
    bool u4(uint32_t& v) {
        if (!has(4)) return false;
        v = ((uint32_t)p[at] << 24) | ((uint32_t)p[at + 1] << 16) | ((uint32_t)p[at + 2] << 8) | p[at + 3];
        at += 4; return true;
    }
    bool skip(size_t c) { if (!has(c)) return false; at += c; return true; }
};

inline void P1(std::vector<unsigned char>& o, unsigned v) { o.push_back((unsigned char)v); }
inline void P2(std::vector<unsigned char>& o, unsigned v) { o.push_back((unsigned char)(v >> 8)); o.push_back((unsigned char)v); }
inline void P4(std::vector<unsigned char>& o, uint32_t v) {
    o.push_back((unsigned char)(v >> 24)); o.push_back((unsigned char)(v >> 16));
    o.push_back((unsigned char)(v >> 8)); o.push_back((unsigned char)v);
}
inline void PB(std::vector<unsigned char>& o, const unsigned char* d, size_t n) { o.insert(o.end(), d, d + n); }

struct Cp {
    uint8_t tag;
    uint16_t a, b;
    std::string utf8;
    Cp() : tag(0), a(0), b(0) {}
};

// Parses the constant pool. `end` receives the offset just after the last entry.
inline bool ParseCp(Cursor& r, uint16_t count, std::vector<Cp>& cp, std::string& err) {
    cp.assign(count, Cp());
    for (uint16_t i = 1; i < count; ++i) {
        uint8_t tag = 0;
        if (!r.u1(tag)) { err = "truncated constant pool"; return false; }
        cp[i].tag = tag;
        uint16_t a = 0, b = 0; uint32_t w = 0;
        switch (tag) {
        case 1: {
            if (!r.u2(a) || !r.has(a)) { err = "truncated Utf8"; return false; }
            cp[i].utf8.assign((const char*)(r.p + r.at), a);
            r.at += a;
            break;
        }
        case 3: case 4: if (!r.u4(w)) { err = "truncated number"; return false; } break;
        case 5: case 6:
            if (!r.u4(w) || !r.u4(w)) { err = "truncated wide"; return false; }
            if (++i < count) cp[i].tag = 0;
            break;
        case 7: case 8: case 16: case 19: case 20:
            if (!r.u2(a)) { err = "truncated ref"; return false; }
            cp[i].a = a; break;
        case 9: case 10: case 11: case 12: case 17: case 18:
            if (!r.u2(a) || !r.u2(b)) { err = "truncated ref2"; return false; }
            cp[i].a = a; cp[i].b = b; break;
        case 15: {
            uint8_t kind = 0;
            if (!r.u1(kind) || !r.u2(a)) { err = "truncated method handle"; return false; }
            cp[i].a = a; break;
        }
        default: err = "unsupported constant pool tag " + std::to_string((int)tag); return false;
        }
    }
    return true;
}

inline const std::string& CpUtf8(const std::vector<Cp>& cp, uint16_t i) {
    static const std::string empty;
    return (i < cp.size() && cp[i].tag == 1) ? cp[i].utf8 : empty;
}

// ---- Descriptor handling ---------------------------------------------------

struct Param { char kind; };   // 'L' reference (incl. arrays), else I J F D (Z B S C map to I)

struct Sig {
    std::vector<Param> params;
    char ret;                   // 'V', 'L', or I J F D
    std::string retClass;       // class/array name for checkcast when ret == 'L' (internal form), "" if Object
    bool ok;
    Sig() : ret('V'), ok(false) {}
};

inline bool ParseOne(const std::string& d, size_t& i, char& kind, std::string* cls) {
    if (i >= d.size()) return false;
    char c = d[i];
    switch (c) {
    case 'Z': case 'B': case 'S': case 'C': case 'I': kind = 'I'; ++i; return true;
    case 'J': kind = 'J'; ++i; return true;
    case 'F': kind = 'F'; ++i; return true;
    case 'D': kind = 'D'; ++i; return true;
    case 'V': kind = 'V'; ++i; return true;
    case 'L': {
        size_t e = d.find(';', i);
        if (e == std::string::npos) return false;
        if (cls) *cls = d.substr(i + 1, e - i - 1);
        i = e + 1; kind = 'L'; return true;
    }
    case '[': {
        size_t s = i;
        while (i < d.size() && d[i] == '[') ++i;
        if (i >= d.size()) return false;
        if (d[i] == 'L') { size_t e = d.find(';', i); if (e == std::string::npos) return false; i = e + 1; }
        else ++i;
        if (cls) *cls = d.substr(s, i - s);
        kind = 'L'; return true;
    }
    default: return false;
    }
}

inline Sig ParseDesc(const std::string& d) {
    Sig s;
    if (d.empty() || d[0] != '(') return s;
    size_t i = 1;
    while (i < d.size() && d[i] != ')') {
        char k = 0;
        if (!ParseOne(d, i, k, nullptr) || k == 'V') return s;
        Param p; p.kind = k; s.params.push_back(p);
    }
    if (i >= d.size() || d[i] != ')') return s;
    ++i;
    std::string cls;
    char rk = 0;
    if (!ParseOne(d, i, rk, &cls) || i != d.size()) return s;
    s.ret = rk;
    if (rk == 'L' && cls != "java/lang/Object") s.retClass = cls;
    s.ok = true;
    return s;
}

// Matches a method descriptor against the erased shape ("L*;" = any reference).
inline bool ShapeMatches(const std::string& desc, const std::string& shape) {
    size_t i = 0, j = 0;
    while (i < desc.size() && j < shape.size()) {
        if (shape.compare(j, 3, "L*;") == 0) {
            char k = 0;
            if (!ParseOne(desc, i, k, nullptr) || k != 'L') return false;
            j += 3;
        } else {
            if (desc[i] != shape[j]) return false;
            ++i; ++j;
        }
    }
    return i == desc.size() && j == shape.size();
}

inline std::string ErasedDescriptor(const Sig& s, bool self, char ret) {
    std::string d = "(";
    if (self) d += "Ljava/lang/Object;";
    for (size_t i = 0; i < s.params.size(); ++i) {
        switch (s.params[i].kind) {
        case 'L': d += "Ljava/lang/Object;"; break;
        case 'I': d += "I"; break;
        case 'J': d += "J"; break;
        case 'F': d += "F"; break;
        case 'D': d += "D"; break;
        }
    }
    d += ")";
    switch (ret) {
    case 'Z': d += "Z"; break;
    case 'V': d += "V"; break;
    case 'L': d += "Ljava/lang/Object;"; break;
    default: d += std::string(1, ret); break;
    }
    return d;
}

inline int SlotsOf(const Sig& s, bool self) {
    int n = self ? 1 : 0;
    for (size_t i = 0; i < s.params.size(); ++i) n += (s.params[i].kind == 'J' || s.params[i].kind == 'D') ? 2 : 1;
    return n;
}

// ---- Constant pool builder -------------------------------------------------

struct CpBuilder {
    std::vector<Cp>* cp;
    std::vector<unsigned char> added;   // serialized new entries
    uint16_t nextIndex;

    CpBuilder(std::vector<Cp>& pool) : cp(&pool), nextIndex((uint16_t)pool.size()) {}

    bool Full() const { return nextIndex >= 0xFFF0; }

    uint16_t Find(uint8_t tag, const std::string& utf, uint16_t a, uint16_t b) const {
        for (size_t i = 1; i < cp->size(); ++i) {
            const Cp& e = (*cp)[i];
            if (e.tag != tag) continue;
            if (tag == 1) { if (e.utf8 == utf) return (uint16_t)i; }
            else if (tag == 7) { if (e.a == a) return (uint16_t)i; }
            else if (e.a == a && e.b == b) return (uint16_t)i;
        }
        return 0;
    }
    uint16_t Utf8(const std::string& s) {
        uint16_t f = Find(1, s, 0, 0);
        if (f) return f;
        if (Full() || s.size() > 0xFFFF) return 0;
        Cp e; e.tag = 1; e.utf8 = s; cp->push_back(e);
        P1(added, 1); P2(added, (unsigned)s.size());
        PB(added, (const unsigned char*)s.data(), s.size());
        return nextIndex++;
    }
    uint16_t Class(const std::string& internalName) {
        uint16_t u = Utf8(internalName);
        if (!u) return 0;
        uint16_t f = Find(7, "", u, 0);
        if (f) return f;
        if (Full()) return 0;
        Cp e; e.tag = 7; e.a = u; cp->push_back(e);
        P1(added, 7); P2(added, u);
        return nextIndex++;
    }
    uint16_t NameAndType(const std::string& name, const std::string& desc) {
        uint16_t n = Utf8(name), d = Utf8(desc);
        if (!n || !d) return 0;
        uint16_t f = Find(12, "", n, d);
        if (f) return f;
        if (Full()) return 0;
        Cp e; e.tag = 12; e.a = n; e.b = d; cp->push_back(e);
        P1(added, 12); P2(added, n); P2(added, d);
        return nextIndex++;
    }
    uint16_t Methodref(uint16_t cls, uint16_t nat) {
        if (!cls || !nat) return 0;
        uint16_t f = Find(10, "", cls, nat);
        if (f) return f;
        if (Full()) return 0;
        Cp e; e.tag = 10; e.a = cls; e.b = nat; cp->push_back(e);
        P1(added, 10); P2(added, cls); P2(added, nat);
        return nextIndex++;
    }
};

// ---- StackMapTable ---------------------------------------------------------

// Copies one verification_type_info, adding `delta` to Uninitialized offsets.
inline bool CopyVti(Cursor& r, std::vector<unsigned char>& out, unsigned delta, std::string& err) {
    uint8_t tag = 0;
    if (!r.u1(tag)) { err = "truncated verification type"; return false; }
    P1(out, tag);
    if (tag == 7) { uint16_t i = 0; if (!r.u2(i)) { err = "truncated object vti"; return false; } P2(out, i); }
    else if (tag == 8) {
        uint16_t off = 0;
        if (!r.u2(off)) { err = "truncated uninitialized vti"; return false; }
        if ((unsigned)off + delta > 0xFFFF) { err = "uninitialized offset overflow"; return false; }
        P2(out, off + delta);
    } else if (tag > 8) { err = "bad verification type"; return false; }
    return true;
}

// Rewrites an existing StackMapTable body (after the u2 count). The first entry's
// offset becomes `newFirstOffset`; `prependFrame` adds a same_frame at `prependOffset`.
// `entryCountOut` receives the new entry count.
inline bool RewriteStackMap(const unsigned char* data, size_t len, unsigned K, bool prependSameFrame,
                            std::vector<unsigned char>& out, std::string& err) {
    Cursor r(data, len);
    uint16_t count = 0;
    if (!r.u2(count)) { err = "truncated StackMapTable"; return false; }
    std::vector<unsigned char> body;
    unsigned entries = count;
    bool mergedFirst = false;
    for (uint16_t e = 0; e < count; ++e) {
        uint8_t type = 0;
        if (!r.u1(type)) { err = "truncated frame"; return false; }
        unsigned delta = 0;
        enum { SAME, SAME1, SAME1X, CHOP, SAMEX, APPEND, FULL } kind;
        unsigned nAppend = 0;
        if (type <= 63) { kind = SAME; delta = type; }
        else if (type <= 127) { kind = SAME1; delta = type - 64; }
        else if (type == 247) { kind = SAME1X; uint16_t d = 0; if (!r.u2(d)) { err = "truncated frame"; return false; } delta = d; }
        else if (type >= 248 && type <= 250) { kind = CHOP; uint16_t d = 0; if (!r.u2(d)) { err = "truncated frame"; return false; } delta = d; }
        else if (type == 251) { kind = SAMEX; uint16_t d = 0; if (!r.u2(d)) { err = "truncated frame"; return false; } delta = d; }
        else if (type >= 252 && type <= 254) { kind = APPEND; nAppend = type - 251; uint16_t d = 0; if (!r.u2(d)) { err = "truncated frame"; return false; } delta = d; }
        else if (type == 255) { kind = FULL; uint16_t d = 0; if (!r.u2(d)) { err = "truncated frame"; return false; } delta = d; }
        else { err = "reserved frame type"; return false; }

        unsigned newDelta = delta;
        if (e == 0) {
            if (prependSameFrame) {
                // Our frame sits at offset K. The original first frame was at `delta`
                // and is now at delta + K, i.e. delta + K - K - 1 after ours.
                if (delta == 0 && K > 0) {
                    // Original frame at offset 0 lands exactly on our target offset:
                    // keep only theirs, now at K.
                    mergedFirst = true;
                    newDelta = K;
                } else {
                    newDelta = delta - 1;
                }
            }
        }

        // Re-encode the header with newDelta.
        switch (kind) {
        case SAME:
            if (newDelta <= 63) P1(body, newDelta); else { P1(body, 251); P2(body, newDelta); }
            break;
        case SAME1:
            if (newDelta <= 63) P1(body, 64 + newDelta); else { P1(body, 247); P2(body, newDelta); }
            if (!CopyVti(r, body, K, err)) return false;
            break;
        case SAME1X:
            P1(body, 247); P2(body, newDelta);
            if (!CopyVti(r, body, K, err)) return false;
            break;
        case CHOP: P1(body, type); P2(body, newDelta); break;
        case SAMEX: P1(body, 251); P2(body, newDelta); break;
        case APPEND:
            P1(body, type); P2(body, newDelta);
            for (unsigned i = 0; i < nAppend; ++i) if (!CopyVti(r, body, K, err)) return false;
            break;
        case FULL: {
            P1(body, 255); P2(body, newDelta);
            uint16_t nl = 0;
            if (!r.u2(nl)) { err = "truncated full frame"; return false; }
            P2(body, nl);
            for (unsigned i = 0; i < nl; ++i) if (!CopyVti(r, body, K, err)) return false;
            uint16_t ns = 0;
            if (!r.u2(ns)) { err = "truncated full frame"; return false; }
            P2(body, ns);
            for (unsigned i = 0; i < ns; ++i) if (!CopyVti(r, body, K, err)) return false;
            break;
        }
        }
    }
    if (r.at != len) { err = "StackMapTable has trailing bytes"; return false; }
    if (prependSameFrame && !mergedFirst) ++entries;
    P2(out, entries);
    if (prependSameFrame && !mergedFirst) {
        if (K <= 63) P1(out, K); else { P1(out, 251); P2(out, K); }
    }
    PB(out, body.data(), body.size());
    return true;
}

inline bool IsHelperCall(const std::vector<Cp>& cp, uint16_t ref, const std::string& owner, const std::string& name) {
    if (ref >= cp.size() || cp[ref].tag != 10) return false;
    uint16_t c = cp[ref].a, nt = cp[ref].b;
    if (c >= cp.size() || nt >= cp.size() || cp[c].tag != 7 || cp[nt].tag != 12) return false;
    return CpUtf8(cp, cp[c].a) == owner && CpUtf8(cp, cp[nt].a) == name;
}

// True when the first bytes of the method already call `guardName` on the helper.
inline bool AlreadyHooked(const unsigned char* code, size_t len, const std::vector<Cp>& cp,
                          const std::string& owner, const std::string& guard) {
    size_t limit = len < 64 ? len : 64;
    for (size_t i = 0; i + 2 < limit; ++i) {
        if (code[i] == 0xB8) {
            uint16_t ref = (uint16_t)((code[i + 1] << 8) | code[i + 2]);
            if (IsHelperCall(cp, ref, owner, guard)) return true;
        }
    }
    return false;
}

inline void EmitLoads(std::vector<unsigned char>& o, const Sig& s, bool self) {
    unsigned slot = 0;
    if (self) { P1(o, 0x19); P1(o, 0); slot = 1; }
    for (size_t i = 0; i < s.params.size(); ++i) {
        char k = s.params[i].kind;
        unsigned op = k == 'L' ? 0x19 : k == 'I' ? 0x15 : k == 'J' ? 0x16 : k == 'F' ? 0x17 : 0x18;
        P1(o, op); P1(o, slot);
        slot += (k == 'J' || k == 'D') ? 2 : 1;
    }
}

struct MethodWork {
    const HookSpec* spec;
    Sig sig;
    bool isStatic;
};

// Rewrites one Code attribute body. `out` receives the new attribute body.
inline bool RewriteCode(const unsigned char* data, size_t len, const std::vector<Cp>& cp, CpBuilder& cb,
                        const MethodWork& w, unsigned major, const std::string& helper,
                        std::vector<unsigned char>& out, bool& already, std::string& err) {
    Cursor r(data, len);
    uint16_t maxStack = 0, maxLocals = 0;
    uint32_t codeLen = 0;
    if (!r.u2(maxStack) || !r.u2(maxLocals) || !r.u4(codeLen) || !r.has(codeLen)) { err = "truncated Code"; return false; }
    const unsigned char* code = data + r.at;
    r.at += codeLen;

    already = AlreadyHooked(code, codeLen, cp, helper, w.spec->guardName);
    if (already) { PB(out, data, len); return true; }

    const bool self = !w.isStatic;
    const bool isVoid = w.sig.ret == 'V';
    if (isVoid != (w.spec->valueName == nullptr)) { err = "valueName must be null exactly for void methods"; return false; }
    const int argSlots = SlotsOf(w.sig, self);
    if (argSlots > 250) { err = "too many argument slots"; return false; }

    uint16_t helperCls = cb.Class(helper);
    uint16_t guardRef = cb.Methodref(helperCls, cb.NameAndType(w.spec->guardName, ErasedDescriptor(w.sig, self, 'Z')));
    uint16_t valueRef = 0, castCls = 0;
    if (!isVoid) {
        valueRef = cb.Methodref(helperCls, cb.NameAndType(w.spec->valueName, ErasedDescriptor(w.sig, self, w.sig.ret)));
        if (w.sig.ret == 'L' && !w.sig.retClass.empty()) castCls = cb.Class(w.sig.retClass);
        if (!valueRef || (w.sig.ret == 'L' && !w.sig.retClass.empty() && !castCls)) { err = "constant pool full"; return false; }
    }
    if (!guardRef) { err = "constant pool full"; return false; }

    // Prologue (without padding). Branch offsets are relative to the ifeq opcode.
    std::vector<unsigned char> pro;
    EmitLoads(pro, w.sig, self);
    P1(pro, 0xB8); P2(pro, guardRef);
    const size_t ifeqAt = pro.size();
    P1(pro, 0x99); P2(pro, 0);                       // ifeq <patched>
    if (isVoid) {
        P1(pro, 0xB1);                                // return
    } else {
        EmitLoads(pro, w.sig, self);
        P1(pro, 0xB8); P2(pro, valueRef);
        if (castCls) { P1(pro, 0xC0); P2(pro, castCls); }
        unsigned ret = w.sig.ret == 'L' ? 0xB0 : w.sig.ret == 'I' ? 0xAC : w.sig.ret == 'J' ? 0xAD : w.sig.ret == 'F' ? 0xAE : 0xAF;
        P1(pro, ret);
    }
    const unsigned pad = (unsigned)((4 - (pro.size() % 4)) % 4);
    const unsigned K = (unsigned)pro.size() + pad;
    if ((uint64_t)codeLen + K > 65535) { err = "method too large to hook"; return false; }
    const int ifeqOffset = (int)(pro.size() - ifeqAt);   // target = end of prologue
    pro[ifeqAt + 1] = (unsigned char)((ifeqOffset >> 8) & 0xFF);
    pro[ifeqAt + 2] = (unsigned char)(ifeqOffset & 0xFF);

    unsigned newStack = maxStack;
    if ((unsigned)argSlots > newStack) newStack = (unsigned)argSlots;
    if (newStack < 2) newStack = 2;

    P2(out, newStack);
    P2(out, maxLocals);
    P4(out, codeLen + K);
    for (unsigned i = 0; i < pad; ++i) P1(out, 0x00);   // nop padding up front
    PB(out, pro.data(), pro.size());
    PB(out, code, codeLen);

    uint16_t exCount = 0;
    if (!r.u2(exCount)) { err = "truncated exception table"; return false; }
    P2(out, exCount);
    for (uint16_t i = 0; i < exCount; ++i) {
        uint16_t s = 0, e = 0, h = 0, t = 0;
        if (!r.u2(s) || !r.u2(e) || !r.u2(h) || !r.u2(t)) { err = "truncated exception entry"; return false; }
        P2(out, s + K); P2(out, e + K); P2(out, h + K); P2(out, t);
    }

    uint16_t attrCount = 0;
    if (!r.u2(attrCount)) { err = "truncated Code attributes"; return false; }
    // First pass: collect attributes and know whether a StackMapTable exists.
    struct Attr { uint16_t name; const unsigned char* body; uint32_t len; };
    std::vector<Attr> attrs;
    bool hasStackMap = false;
    for (uint16_t i = 0; i < attrCount; ++i) {
        Attr a; a.name = 0; a.body = nullptr; a.len = 0;
        if (!r.u2(a.name) || !r.u4(a.len) || !r.has(a.len)) { err = "truncated Code sub-attribute"; return false; }
        a.body = data + r.at;
        r.at += a.len;
        const std::string& nm = CpUtf8(cp, a.name);
        if (nm == "StackMapTable") hasStackMap = true;
        else if (nm != "LineNumberTable" && nm != "LocalVariableTable" && nm != "LocalVariableTypeTable") {
            err = "unsupported Code attribute " + nm; return false;
        }
        attrs.push_back(a);
    }
    if (r.at != len) { err = "Code attribute has trailing bytes"; return false; }

    const bool needFrames = major >= 50;
    uint16_t smName = 0;
    if (needFrames && !hasStackMap) {
        smName = cb.Utf8("StackMapTable");
        if (!smName) { err = "constant pool full"; return false; }
    }
    P2(out, (unsigned)attrs.size() + ((needFrames && !hasStackMap) ? 1 : 0));
    for (size_t i = 0; i < attrs.size(); ++i) {
        const Attr& a = attrs[i];
        const std::string& nm = CpUtf8(cp, a.name);
        std::vector<unsigned char> body;
        Cursor ar(a.body, a.len);
        if (nm == "LineNumberTable") {
            uint16_t n = 0;
            if (!ar.u2(n)) { err = "truncated LineNumberTable"; return false; }
            P2(body, n);
            for (uint16_t j = 0; j < n; ++j) {
                uint16_t pc = 0, line = 0;
                if (!ar.u2(pc) || !ar.u2(line)) { err = "truncated line entry"; return false; }
                P2(body, pc + K); P2(body, line);
            }
        } else if (nm == "LocalVariableTable" || nm == "LocalVariableTypeTable") {
            uint16_t n = 0;
            if (!ar.u2(n)) { err = "truncated local variable table"; return false; }
            P2(body, n);
            for (uint16_t j = 0; j < n; ++j) {
                uint16_t pc = 0, ln = 0, ni = 0, di = 0, sl = 0;
                if (!ar.u2(pc) || !ar.u2(ln) || !ar.u2(ni) || !ar.u2(di) || !ar.u2(sl)) { err = "truncated local variable entry"; return false; }
                P2(body, pc + K); P2(body, ln); P2(body, ni); P2(body, di); P2(body, sl);
            }
        } else {   // StackMapTable
            if (needFrames) {
                if (!RewriteStackMap(a.body, a.len, K, true, body, err)) return false;
            } else {
                PB(body, a.body, a.len);
            }
        }
        P2(out, a.name); P4(out, (uint32_t)body.size()); PB(out, body.data(), body.size());
    }
    if (needFrames && !hasStackMap) {
        std::vector<unsigned char> body;
        P2(body, 1);
        if (K <= 63) P1(body, K); else { P1(body, 251); P2(body, K); }
        P2(out, smName); P4(out, (uint32_t)body.size()); PB(out, body.data(), body.size());
    }
    return true;
}

} // namespace detail

// Rewrites `data`. `helperInternalName` is the slash-separated helper class
// (for example "com/aoko/visuals/Hooks"). Only methods matching a spec are touched;
// the result is `ok == true, changed == false` when nothing matched. A spec that
// matches a method but cannot be applied makes the whole rewrite fail closed.
inline RewriteResult Rewrite(const unsigned char* data, size_t len, const HookSpec* specs, size_t specCount,
                             const std::string& helperInternalName) {
    using namespace detail;
    RewriteResult res;
    Cursor r(data, len);
    uint32_t magic = 0; uint16_t minor = 0, major = 0, cpCount = 0;
    if (!r.u4(magic) || magic != 0xCAFEBABEu || !r.u2(minor) || !r.u2(major) || !r.u2(cpCount) || cpCount == 0) {
        res.error = "not a class file"; return res;
    }
    const size_t cpStart = r.at;
    std::vector<Cp> cp;
    if (!ParseCp(r, cpCount, cp, res.error)) return res;
    const size_t cpEnd = r.at;

    // access, this, super, interfaces, fields
    const size_t headerStart = r.at;
    uint16_t access = 0, thisC = 0, superC = 0, nIf = 0;
    if (!r.u2(access) || !r.u2(thisC) || !r.u2(superC) || !r.u2(nIf) || !r.skip((size_t)nIf * 2)) { res.error = "truncated class header"; return res; }
    uint16_t nFields = 0;
    if (!r.u2(nFields)) { res.error = "truncated fields"; return res; }
    for (uint16_t i = 0; i < nFields; ++i) {
        uint16_t fa = 0, fn = 0, fd = 0, fac = 0;
        if (!r.u2(fa) || !r.u2(fn) || !r.u2(fd) || !r.u2(fac)) { res.error = "truncated field"; return res; }
        for (uint16_t j = 0; j < fac; ++j) {
            uint16_t an = 0; uint32_t al = 0;
            if (!r.u2(an) || !r.u4(al) || !r.skip(al)) { res.error = "truncated field attribute"; return res; }
        }
    }
    const size_t methodsCountAt = r.at;
    uint16_t nMethods = 0;
    if (!r.u2(nMethods)) { res.error = "truncated methods"; return res; }

    CpBuilder cb(cp);
    std::vector<unsigned char> methodsOut;
    int hooked = 0, already = 0;
    for (uint16_t m = 0; m < nMethods; ++m) {
        uint16_t ma = 0, mn = 0, md = 0, mac = 0;
        if (!r.u2(ma) || !r.u2(mn) || !r.u2(md) || !r.u2(mac)) { res.error = "truncated method"; return res; }
        // Copies, not references: RewriteCode appends to `cp`, which may reallocate it.
        const std::string name = CpUtf8(cp, mn);
        const std::string desc = CpUtf8(cp, md);
        const HookSpec* match = nullptr;
        for (size_t s = 0; s < specCount && !match; ++s) {
            if ((ma & specs[s].forbidFlags) != 0) continue;
            if (!ShapeMatches(desc, specs[s].shape)) continue;
            for (const char* const* nn = specs[s].methodNames; nn && *nn; ++nn)
                if (name == *nn) { match = &specs[s]; break; }
        }
        P2(methodsOut, ma); P2(methodsOut, mn); P2(methodsOut, md); P2(methodsOut, mac);
        for (uint16_t a = 0; a < mac; ++a) {
            uint16_t an = 0; uint32_t al = 0;
            if (!r.u2(an) || !r.u4(al) || !r.has(al)) { res.error = "truncated method attribute"; return res; }
            const unsigned char* body = data + r.at;
            r.at += al;
            if (match && CpUtf8(cp, an) == "Code") {
                if (ma & 0x0500) { res.error = "target method is abstract or native"; return res; }   // ACC_ABSTRACT|ACC_NATIVE
                MethodWork w;
                w.spec = match;
                w.sig = ParseDesc(desc);
                w.isStatic = (ma & 0x0008) != 0;
                if (!w.sig.ok) { res.error = "bad descriptor " + desc; return res; }
                std::vector<unsigned char> nb;
                bool was = false;
                if (!RewriteCode(body, al, cp, cb, w, major, helperInternalName, nb, was, res.error)) {
                    res.error = name + desc + ": " + res.error;
                    return res;
                }
                if (was) ++already; else ++hooked;
                P2(methodsOut, an); P4(methodsOut, (uint32_t)nb.size()); PB(methodsOut, nb.data(), nb.size());
            } else {
                P2(methodsOut, an); P4(methodsOut, al); PB(methodsOut, body, al);
            }
        }
    }
    const size_t classAttrsStart = r.at;
    {
        // Validate the class attribute table so truncated or padded input fails closed.
        uint16_t attrCount = 0;
        if (!r.u2(attrCount)) { res.error = "truncated class attributes"; return res; }
        for (uint16_t a = 0; a < attrCount; ++a) {
            uint16_t an = 0; uint32_t al = 0;
            if (!r.u2(an) || !r.u4(al) || !r.skip(al)) { res.error = "truncated class attribute"; return res; }
        }
        if (r.at != len) { res.error = "trailing bytes after class attributes"; return res; }
    }

    res.hooked = hooked;
    res.alreadyHooked = already;
    if (hooked == 0) {
        res.ok = true; res.changed = false;
        return res;
    }

    std::vector<unsigned char>& o = res.bytes;
    PB(o, data, 8);                                   // magic, minor, major
    P2(o, cb.nextIndex);
    PB(o, data + cpStart, cpEnd - cpStart);
    PB(o, cb.added.data(), cb.added.size());
    PB(o, data + headerStart, methodsCountAt - headerStart);   // flags .. fields
    P2(o, nMethods);
    PB(o, methodsOut.data(), methodsOut.size());
    PB(o, data + classAttrsStart, len - classAttrsStart);       // class attributes
    res.ok = true;
    res.changed = true;
    return res;
}

} // namespace mhook
