#pragma once

#include <algorithm>
#include <string>

namespace lc {

class SimpleJsonConfigReader {
public:
    explicit SimpleJsonConfigReader(std::string line) : _line(std::move(line)) {}

    std::string GetString(const char* key) const
    {
        std::string marker = std::string("\"") + key + "\":";
        size_t pos = _line.find(marker);
        if (pos == std::string::npos) return "";

        pos += marker.length();
        if (pos >= _line.size()) return "";

        if (_line[pos] == '"') {
            size_t end = _line.find('"', pos + 1);
            return (end == std::string::npos) ? "" : _line.substr(pos + 1, end - pos - 1);
        }

        size_t end = _line.find_first_of(",}", pos);
        std::string value = (end == std::string::npos) ? _line.substr(pos) : _line.substr(pos, end - pos);
        while (!value.empty() && value[0] == ' ') value.erase(0, 1);
        return value;
    }

    bool GetBool(const char* key, bool defaultValue = false) const
    {
        std::string value = GetString(key);
        if (value.empty()) return defaultValue;
        return value == "true";
    }

    float GetFloat(const char* key, float defaultValue = 0.0f) const
    {
        std::string value = GetString(key);
        if (value.empty()) return defaultValue;
        try { return std::stof(value); } catch (...) { return defaultValue; }
    }

    int GetInt(const char* key, int defaultValue = 0) const
    {
        std::string value = GetString(key);
        if (value.empty()) return defaultValue;
        try { return std::stoi(value); } catch (...) { return defaultValue; }
    }

private:
    std::string _line;
};

inline int ClampInt(int value, int minValue, int maxValue)
{
    return (std::max)(minValue, (std::min)(maxValue, value));
}

inline float ClampFloat(float value, float minValue, float maxValue)
{
    return (std::max)(minValue, (std::min)(maxValue, value));
}

} // namespace lc
