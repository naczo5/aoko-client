#pragma once

#include <algorithm>
#include <cerrno>
#include <cctype>
#include <cmath>
#include <cstdlib>
#include <limits>
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
        if (value == "true") return true;
        if (value == "false") return false;
        return defaultValue;
    }

    float GetFloat(const char* key, float defaultValue = 0.0f) const
    {
        std::string value = GetString(key);
        if (value.empty()) return defaultValue;

        errno = 0;
        char* end = NULL;
        const char* start = value.c_str();
        float parsed = std::strtof(start, &end);
        if (end == start || errno == ERANGE || !std::isfinite(parsed) || !OnlyWhitespaceRemains(end))
            return defaultValue;
        return parsed;
    }

    int GetInt(const char* key, int defaultValue = 0) const
    {
        std::string value = GetString(key);
        if (value.empty()) return defaultValue;

        errno = 0;
        char* end = NULL;
        const char* start = value.c_str();
        long parsed = std::strtol(start, &end, 10);
        if (end == start ||
            errno == ERANGE ||
            parsed < (std::numeric_limits<int>::min)() ||
            parsed > (std::numeric_limits<int>::max)() ||
            !OnlyWhitespaceRemains(end)) {
            return defaultValue;
        }
        return static_cast<int>(parsed);
    }

private:
    static bool OnlyWhitespaceRemains(const char* value)
    {
        while (*value != '\0') {
            if (!std::isspace(static_cast<unsigned char>(*value))) return false;
            ++value;
        }
        return true;
    }

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
