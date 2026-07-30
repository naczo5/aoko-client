using System;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace Aoko.Core;

internal readonly record struct BoundedLineReadResult(string? Line, bool IsTooLong)
{
    public bool IsEndOfStream => Line == null && !IsTooLong;
}

/// <summary>
/// Reads newline-delimited text without allowing one malformed message to grow
/// an unbounded <see cref="StringBuilder"/>.
/// </summary>
internal sealed class BoundedLineReader : IDisposable
{
    private const int ReadBufferCharacters = 4096;

    private readonly StreamReader _reader;
    private readonly int _maximumLineCharacters;
    private readonly char[] _readBuffer = new char[ReadBufferCharacters];
    private readonly StringBuilder _lineBuilder;
    private int _readPosition;
    private int _readLength;
    private bool _discardingOversizedLine;

    public BoundedLineReader(
        Stream stream,
        int maximumLineCharacters,
        Encoding? encoding = null,
        bool leaveOpen = false)
    {
        ArgumentNullException.ThrowIfNull(stream);
        ArgumentOutOfRangeException.ThrowIfNegativeOrZero(maximumLineCharacters);

        _maximumLineCharacters = maximumLineCharacters;
        _lineBuilder = new StringBuilder(Math.Min(maximumLineCharacters, ReadBufferCharacters));
        _reader = new StreamReader(
            stream,
            encoding ?? Encoding.UTF8,
            detectEncodingFromByteOrderMarks: true,
            bufferSize: ReadBufferCharacters,
            leaveOpen: leaveOpen);
    }

    public async ValueTask<BoundedLineReadResult> ReadLineAsync(CancellationToken cancellationToken)
    {
        while (true)
        {
            if (_readPosition >= _readLength)
            {
                _readLength = await _reader
                    .ReadAsync(_readBuffer.AsMemory(), cancellationToken)
                    .ConfigureAwait(false);
                _readPosition = 0;

                if (_readLength == 0)
                {
                    if (_discardingOversizedLine)
                    {
                        ResetLine();
                        return new BoundedLineReadResult(null, IsTooLong: true);
                    }

                    if (_lineBuilder.Length == 0)
                        return new BoundedLineReadResult(null, IsTooLong: false);

                    string finalLine = TakeCompletedLine();
                    return new BoundedLineReadResult(finalLine, IsTooLong: false);
                }
            }

            char current = _readBuffer[_readPosition++];
            if (_discardingOversizedLine)
            {
                if (current == '\n')
                {
                    ResetLine();
                    return new BoundedLineReadResult(null, IsTooLong: true);
                }

                continue;
            }

            if (current == '\n')
            {
                string completedLine = TakeCompletedLine();
                return new BoundedLineReadResult(completedLine, IsTooLong: false);
            }

            if (_lineBuilder.Length >= _maximumLineCharacters)
            {
                if (current == '\r' && _lineBuilder.Length == _maximumLineCharacters)
                {
                    _lineBuilder.Append(current);
                    continue;
                }

                _discardingOversizedLine = true;
                continue;
            }

            _lineBuilder.Append(current);
        }
    }

    public void Dispose()
        => _reader.Dispose();

    private string TakeCompletedLine()
    {
        int length = _lineBuilder.Length;
        if (length > 0 && _lineBuilder[length - 1] == '\r')
            length--;

        string line = _lineBuilder.ToString(0, length);
        ResetLine();
        return line;
    }

    private void ResetLine()
    {
        _lineBuilder.Clear();
        _discardingOversizedLine = false;
    }
}
