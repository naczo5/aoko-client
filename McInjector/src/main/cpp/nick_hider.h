#pragma once

#include <cctype>
#include <string>

namespace lc {

inline bool IsNickHiderNameChar(unsigned char value)
{
    return std::isalnum(value) != 0 || value == '_';
}

inline bool IsNickHiderLeftBoundary(const std::string& text, size_t match)
{
    if (match == 0) return true;
    if (!IsNickHiderNameChar(static_cast<unsigned char>(text[match - 1]))) return true;

    // A formatting code is UTF-8 § (C2 A7) followed by one ASCII code byte.
    return match >= 3
        && static_cast<unsigned char>(text[match - 3]) == 0xC2
        && static_cast<unsigned char>(text[match - 2]) == 0xA7;
}

inline std::string TrimNickHiderAlias(const std::string& value)
{
    size_t first = 0;
    while (first < value.size() && std::isspace(static_cast<unsigned char>(value[first]))) ++first;
    size_t last = value.size();
    while (last > first && std::isspace(static_cast<unsigned char>(value[last - 1]))) --last;
    return value.substr(first, last - first);
}

inline bool IsValidNickHiderAlias(const std::string& value)
{
    if (value.empty()) return false;

    size_t codePoints = 0;
    for (size_t i = 0; i < value.size(); ++i) {
        unsigned char ch = static_cast<unsigned char>(value[i]);
        if (ch < 0x20 || ch == 0x7F) return false;
        // Minecraft's formatting marker (§ encoded as C2 A7 in UTF-8) is intentionally unsupported.
        if (ch == 0xC2 && i + 1 < value.size() && static_cast<unsigned char>(value[i + 1]) == 0xA7) return false;
        if ((ch & 0xC0) != 0x80) ++codePoints;
    }
    return codePoints > 0 && codePoints <= 32;
}

inline std::string NormalizeNickHiderAlias(const std::string& value)
{
    std::string alias = TrimNickHiderAlias(value);
    return IsValidNickHiderAlias(alias) ? alias : std::string();
}

/// Replaces only standalone Minecraft-name tokens. Text outside a match, including
/// surrounding § formatting codes, is copied byte-for-byte.
inline std::string ReplaceNickHiderText(const std::string& text, bool enabled,
                                        const std::string& accountName, const std::string& alias)
{
    if (!enabled || accountName.empty() || alias.empty() || text.empty()) return text;

    std::string output;
    size_t copyFrom = 0;
    size_t searchFrom = 0;
    bool changed = false;
    while (searchFrom < text.size()) {
        size_t match = text.find(accountName, searchFrom);
        if (match == std::string::npos) break;

        const bool leftBoundary = IsNickHiderLeftBoundary(text, match);
        const size_t end = match + accountName.size();
        const bool rightBoundary = end >= text.size() || !IsNickHiderNameChar(static_cast<unsigned char>(text[end]));
        if (!leftBoundary || !rightBoundary) {
            searchFrom = match + 1;
            continue;
        }

        output.append(text, copyFrom, match - copyFrom);
        output += alias;
        copyFrom = end;
        searchFrom = end;
        changed = true;
    }

    if (!changed) return text;
    output.append(text, copyFrom, std::string::npos);
    return output;
}

} // namespace lc
