#pragma once
// Per-module runtime status shared by both bridges. A module that is supported
// by the bridge but cannot run (a mapping did not resolve, a class is missing)
// records a short reason here. The bridge sends the map as "moduleStatus" in the
// full state packet and the loader shows the reason on the module card.
// Callers own locking; the bridges wrap access with their own Mutex.

#include <map>
#include <string>

namespace lc {

class ModuleStatusMap {
public:
    // Returns true when the stored value changed.
    bool Set(const std::string& moduleId, const std::string& reason) {
        std::map<std::string, std::string>::iterator it = m_reasons.find(moduleId);
        if (reason.empty()) {
            if (it == m_reasons.end()) return false;
            m_reasons.erase(it);
            return true;
        }
        if (it != m_reasons.end() && it->second == reason) return false;
        m_reasons[moduleId] = reason;
        return true;
    }

    void Clear(const std::string& moduleId) { Set(moduleId, std::string()); }

    std::string Get(const std::string& moduleId) const {
        std::map<std::string, std::string>::const_iterator it = m_reasons.find(moduleId);
        return it == m_reasons.end() ? std::string() : it->second;
    }

    bool Empty() const { return m_reasons.empty(); }

    // {"fastplace":"Missing rightClickDelay mapping"}
    std::string ToJson() const {
        std::string out = "{";
        bool first = true;
        for (std::map<std::string, std::string>::const_iterator it = m_reasons.begin();
             it != m_reasons.end(); ++it) {
            if (!first) out += ',';
            first = false;
            AppendEscaped(out, it->first);
            out += ':';
            AppendEscaped(out, it->second);
        }
        out += '}';
        return out;
    }

private:
    static void AppendEscaped(std::string& out, const std::string& value) {
        out += '"';
        for (size_t i = 0; i < value.size(); ++i) {
            const unsigned char c = (unsigned char)value[i];
            if (c == '"' || c == '\\') { out += '\\'; out += (char)c; }
            else if (c < 0x20) out += ' ';
            else out += (char)c;
        }
        out += '"';
    }

    std::map<std::string, std::string> m_reasons;
};

} // namespace lc
