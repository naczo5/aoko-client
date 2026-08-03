#pragma once

#include <cstddef>
#include <string>
#include <vector>

namespace lc {

class BoundedNewlineBuffer {
public:
    explicit BoundedNewlineBuffer(std::size_t maximumLineBytes)
        : _maximumLineBytes(maximumLineBytes == 0 ? 1 : maximumLineBytes),
          _discardingOversizedLine(false)
    {
        _buffer.reserve((_maximumLineBytes < 4096) ? _maximumLineBytes : 4096);
    }

    std::size_t Append(
        const char* data,
        std::size_t length,
        std::vector<std::string>* completedLines)
    {
        if (!data || !completedLines) return 0;

        std::size_t discardedLines = 0;
        for (std::size_t i = 0; i < length; ++i) {
            const char current = data[i];
            if (_discardingOversizedLine) {
                if (current == '\n') {
                    _discardingOversizedLine = false;
                    ++discardedLines;
                }
                continue;
            }

            if (current == '\n') {
                if (!_buffer.empty() && _buffer[_buffer.size() - 1] == '\r')
                    _buffer.resize(_buffer.size() - 1);
                completedLines->push_back(_buffer);
                _buffer.clear();
                continue;
            }

            if (_buffer.size() >= _maximumLineBytes) {
                if (current == '\r' && _buffer.size() == _maximumLineBytes) {
                    _buffer.push_back(current);
                    continue;
                }

                _buffer.clear();
                _discardingOversizedLine = true;
                continue;
            }

            _buffer.push_back(current);
        }

        return discardedLines;
    }

    std::size_t BufferedBytes() const
    {
        return _buffer.size();
    }

    bool IsDiscardingOversizedLine() const
    {
        return _discardingOversizedLine;
    }

private:
    std::size_t _maximumLineBytes;
    std::string _buffer;
    bool _discardingOversizedLine;
};

} // namespace lc
