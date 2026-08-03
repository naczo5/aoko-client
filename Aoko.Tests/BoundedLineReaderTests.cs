using System.Text;
using Aoko.Core;

namespace Aoko.Tests;

public class BoundedLineReaderTests
{
    [Fact]
    public async Task ReadLineAsync_ReadsCrLfAndFinalUnterminatedLine()
    {
        using var stream = Stream("first\r\nsecond");
        using var reader = new BoundedLineReader(stream, maximumLineCharacters: 32);

        BoundedLineReadResult first = await reader.ReadLineAsync(CancellationToken.None);
        BoundedLineReadResult second = await reader.ReadLineAsync(CancellationToken.None);
        BoundedLineReadResult end = await reader.ReadLineAsync(CancellationToken.None);

        Assert.Equal("first", first.Line);
        Assert.Equal("second", second.Line);
        Assert.True(end.IsEndOfStream);
    }

    [Fact]
    public async Task ReadLineAsync_AcceptsLineAtConfiguredLimit()
    {
        using var stream = Stream("12345\r\n");
        using var reader = new BoundedLineReader(stream, maximumLineCharacters: 5);

        BoundedLineReadResult result = await reader.ReadLineAsync(CancellationToken.None);

        Assert.Equal("12345", result.Line);
        Assert.False(result.IsTooLong);
    }

    [Fact]
    public async Task ReadLineAsync_DiscardsOversizedLineAndContinues()
    {
        using var stream = Stream("123456789\nvalid\n");
        using var reader = new BoundedLineReader(stream, maximumLineCharacters: 5);

        BoundedLineReadResult oversized = await reader.ReadLineAsync(CancellationToken.None);
        BoundedLineReadResult valid = await reader.ReadLineAsync(CancellationToken.None);

        Assert.True(oversized.IsTooLong);
        Assert.Null(oversized.Line);
        Assert.Equal("valid", valid.Line);
    }

    [Fact]
    public async Task ReadLineAsync_ReportsOversizedLineAtEndOfStreamOnce()
    {
        using var stream = Stream("123456789");
        using var reader = new BoundedLineReader(stream, maximumLineCharacters: 5);

        BoundedLineReadResult oversized = await reader.ReadLineAsync(CancellationToken.None);
        BoundedLineReadResult end = await reader.ReadLineAsync(CancellationToken.None);

        Assert.True(oversized.IsTooLong);
        Assert.True(end.IsEndOfStream);
    }

    [Fact]
    public async Task ReadLineAsync_PreservesUtf8Text()
    {
        using var stream = Stream("zażółć 🐈\n");
        using var reader = new BoundedLineReader(stream, maximumLineCharacters: 32);

        BoundedLineReadResult result = await reader.ReadLineAsync(CancellationToken.None);

        Assert.Equal("zażółć 🐈", result.Line);
    }

    private static MemoryStream Stream(string value)
        => new(Encoding.UTF8.GetBytes(value));
}
