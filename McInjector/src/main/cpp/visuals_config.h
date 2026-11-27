#pragma once
// visuals_config.h
//
// Pure helpers shared by the bridges and the native tests: the settings the loader sends
// for the cosmetics and sky modules, id validation, the JSON handed to the Java helper,
// and status sanitising. No JNI, no Windows headers.

#include <cstddef>
#include <string>

namespace visuals {

enum class Generation { Legacy189, Modern26 };

struct Settings {
    bool cosmeticsEnabled;
    std::string cape, hat, back;
    bool skyEnabled;
    std::string skyPreset;
    int skyTime;                    // -2 preset default, -1 follow server, 0..23999 lock
    bool skyClearWeather;
    std::string dir;                // absolute visuals directory

    Settings() : cosmeticsEnabled(false), skyEnabled(false), skyTime(-2), skyClearWeather(false) {}

    bool Wanted() const { return cosmeticsEnabled || skyEnabled; }

    bool operator==(const Settings& o) const {
        return cosmeticsEnabled == o.cosmeticsEnabled && cape == o.cape && hat == o.hat && back == o.back &&
               skyEnabled == o.skyEnabled && skyPreset == o.skyPreset && skyTime == o.skyTime &&
               skyClearWeather == o.skyClearWeather && dir == o.dir;
    }
    bool operator!=(const Settings& o) const { return !(*this == o); }
};

// ^[a-z0-9][a-z0-9_-]{0,31}$ after the loader's normalisation. Anything else is dropped.
inline bool ValidId(const std::string& id) {
    if (id.empty() || id.size() > 32) return false;
    for (size_t i = 0; i < id.size(); ++i) {
        const char c = id[i];
        const bool alnum = (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9');
        if (alnum) continue;
        if (i > 0 && (c == '_' || c == '-')) continue;
        return false;
    }
    return true;
}

inline std::string CleanId(const std::string& id) { return ValidId(id) ? id : std::string(); }

inline int ClampSkyTime(int t) { return t < -2 ? -2 : (t > 23999 ? 23999 : t); }

// A directory path as the loader sends it: absolute (drive or UNC), no control characters, bounded.
inline bool ValidDir(const std::string& dir) {
    if (dir.empty() || dir.size() > 1024) return false;
    const bool drive = dir.size() >= 3 && ((dir[0] >= 'A' && dir[0] <= 'Z') || (dir[0] >= 'a' && dir[0] <= 'z')) &&
                       dir[1] == ':' && (dir[2] == '\\' || dir[2] == '/');
    const bool unc = dir.size() >= 3 && dir[0] == '\\' && dir[1] == '\\';
    if (!drive && !unc) return false;
    for (size_t i = 0; i < dir.size(); ++i) {
        const unsigned char c = static_cast<unsigned char>(dir[i]);
        if (c < 0x20 || c == 0x7F) return false;
    }
    return true;
}

// ---- JSON string handling ---------------------------------------------------------------
//
// The loader's config line is JSON written by System.Text.Json, which escapes backslashes
// and every non-ASCII character as \uXXXX. SimpleJsonConfigReader hands those strings back
// still escaped, so a path such as C:\Users\Jos\u00e9\... must be decoded before use.

inline void AppendUtf8(std::string& out, unsigned long cp) {
    if (cp < 0x80) {
        out += static_cast<char>(cp);
    } else if (cp < 0x800) {
        out += static_cast<char>(0xC0 | (cp >> 6));
        out += static_cast<char>(0x80 | (cp & 0x3F));
    } else if (cp < 0x10000) {
        out += static_cast<char>(0xE0 | (cp >> 12));
        out += static_cast<char>(0x80 | ((cp >> 6) & 0x3F));
        out += static_cast<char>(0x80 | (cp & 0x3F));
    } else {
        out += static_cast<char>(0xF0 | (cp >> 18));
        out += static_cast<char>(0x80 | ((cp >> 12) & 0x3F));
        out += static_cast<char>(0x80 | ((cp >> 6) & 0x3F));
        out += static_cast<char>(0x80 | (cp & 0x3F));
    }
}

inline int HexValue(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

// Decodes the body of a JSON string (no surrounding quotes) into UTF-8. Returns false on a
// malformed escape, a lone surrogate or a control character, so callers can reject the value.
inline bool UnescapeJsonString(const std::string& raw, std::string& out) {
    out.clear();
    for (size_t i = 0; i < raw.size(); ++i) {
        const unsigned char c = static_cast<unsigned char>(raw[i]);
        if (c < 0x20) return false;
        if (c != '\\') { out += static_cast<char>(c); continue; }
        if (++i >= raw.size()) return false;
        switch (raw[i]) {
        case '"': out += '"'; break;
        case '\\': out += '\\'; break;
        case '/': out += '/'; break;
        case 'b': out += '\b'; break;
        case 'f': out += '\f'; break;
        case 'n': out += '\n'; break;
        case 'r': out += '\r'; break;
        case 't': out += '\t'; break;
        case 'u': {
            unsigned long cp = 0;
            for (int k = 1; k <= 4; ++k) {
                if (i + k >= raw.size()) return false;
                const int h = HexValue(raw[i + k]);
                if (h < 0) return false;
                cp = cp * 16 + static_cast<unsigned long>(h);
            }
            i += 4;
            if (cp >= 0xD800 && cp <= 0xDBFF) {
                if (i + 6 >= raw.size()) return false;
                if (raw[i + 1] != '\\' || raw[i + 2] != 'u') return false;
                unsigned long lo = 0;
                for (int k = 3; k <= 6; ++k) {
                    const int h = HexValue(raw[i + k]);
                    if (h < 0) return false;
                    lo = lo * 16 + static_cast<unsigned long>(h);
                }
                if (lo < 0xDC00 || lo > 0xDFFF) return false;
                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                i += 6;
            } else if (cp >= 0xDC00 && cp <= 0xDFFF) {
                return false;
            }
            AppendUtf8(out, cp);
            break;
        }
        default: return false;
        }
    }
    return true;
}

// UTF-8 to a JSON string literal with every non-ASCII character as \uXXXX, so the result is
// pure ASCII and can cross JNI with NewStringUTF unchanged. Invalid UTF-8 becomes U+FFFD.
inline std::string JsonEscape(const std::string& v) {
    static const char* hex = "0123456789abcdef";
    std::string out;
    out.reserve(v.size() + 2);
    out += '"';
    size_t i = 0;
    while (i < v.size()) {
        const unsigned char c = static_cast<unsigned char>(v[i]);
        unsigned long cp = c;
        size_t len = 1;
        if (c >= 0xC2 && c <= 0xDF) { cp = c & 0x1F; len = 2; }
        else if (c >= 0xE0 && c <= 0xEF) { cp = c & 0x0F; len = 3; }
        else if (c >= 0xF0 && c <= 0xF4) { cp = c & 0x07; len = 4; }
        else if (c >= 0x80) { cp = 0xFFFD; }
        if (len > 1) {
            bool ok = i + len <= v.size();
            for (size_t k = 1; ok && k < len; ++k) {
                const unsigned char cc = static_cast<unsigned char>(v[i + k]);
                if ((cc & 0xC0) != 0x80) ok = false; else cp = (cp << 6) | (cc & 0x3F);
            }
            if (!ok || (len == 3 && (cp < 0x800 || (cp >= 0xD800 && cp <= 0xDFFF))) || (len == 4 && (cp < 0x10000 || cp > 0x10FFFF)) ||
                (len == 2 && cp < 0x80)) { cp = 0xFFFD; len = 1; }
        }
        i += len;
        if (cp == '"') out += "\\\"";
        else if (cp == '\\') out += "\\\\";
        else if (cp < 0x20) { out += "\\u00"; out += hex[cp >> 4]; out += hex[cp & 15]; }
        else if (cp < 0x80) out += static_cast<char>(cp);
        else if (cp < 0x10000) {
            out += "\\u";
            out += hex[(cp >> 12) & 15]; out += hex[(cp >> 8) & 15]; out += hex[(cp >> 4) & 15]; out += hex[cp & 15];
        } else {
            const unsigned long v2 = cp - 0x10000;
            const unsigned long hi = 0xD800 + (v2 >> 10), lo = 0xDC00 + (v2 & 0x3FF);
            out += "\\u"; out += hex[(hi >> 12) & 15]; out += hex[(hi >> 8) & 15]; out += hex[(hi >> 4) & 15]; out += hex[hi & 15];
            out += "\\u"; out += hex[(lo >> 12) & 15]; out += hex[(lo >> 8) & 15]; out += hex[(lo >> 4) & 15]; out += hex[lo & 15];
        }
    }
    out += '"';
    return out;
}

// Flat object read by com.aoko.visuals.Config.parse.
inline std::string BuildConfigJson(const Settings& s) {
    std::string j = "{";
    j += "\"cosmetics\":"; j += s.cosmeticsEnabled ? "true" : "false";
    j += ",\"cape\":" + JsonEscape(CleanId(s.cape));
    j += ",\"hat\":" + JsonEscape(CleanId(s.hat));
    j += ",\"back\":" + JsonEscape(CleanId(s.back));
    j += ",\"sky\":"; j += s.skyEnabled ? "true" : "false";
    j += ",\"preset\":" + JsonEscape(CleanId(s.skyPreset));
    j += ",\"time\":" + std::to_string(ClampSkyTime(s.skyTime));
    j += ",\"clear\":"; j += s.skyClearWeather ? "true" : "false";
    j += ",\"dir\":" + JsonEscape(ValidDir(s.dir) ? s.dir : std::string());
    j += "}";
    return j;
}

// Status text goes into the state JSON without further escaping, so keep it to a safe
// alphabet and a bounded length. Unknown characters become '_'.
inline std::string SanitizeStatus(const std::string& raw) {
    std::string out;
    out.reserve(raw.size() > 220 ? 220 : raw.size());
    for (size_t i = 0; i < raw.size() && out.size() < 220; ++i) {
        const unsigned char c = static_cast<unsigned char>(raw[i]);
        const bool ok = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') ||
                        c == ' ' || c == ':' || c == ';' || c == ',' || c == '.' || c == '(' || c == ')' ||
                        c == '_' || c == '-' || c == '\'' || c == '/' || c == '+';
        out += ok ? static_cast<char>(c) : '_';
    }
    return out.empty() ? std::string("off") : out;
}

} // namespace visuals
