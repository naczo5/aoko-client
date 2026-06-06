using System.Text.RegularExpressions;
using Aoko.Core;

namespace Aoko.Tests;

public class TcpPortHelperTests
{
    [Theory]
    [InlineData("TCP    0.0.0.0:25590          0.0.0.0:0              LISTENING       12345", 25590, true)]
    [InlineData("TCP    127.0.0.1:25590        0.0.0.0:0              LISTENING       67890", 25590, true)]
    [InlineData("TCP    0.0.0.0:255901         0.0.0.0:0              LISTENING       11111", 25590, false)]
    public void ListeningPortRegex_MatchesExpectedLines(string line, int port, bool shouldMatch)
    {
        string pattern = $@"\s(?:0\.0\.0\.0|127\.0\.0\.1|\[::\]|\[::1\]):{port}(\s|$)";
        var regex = new Regex(pattern, RegexOptions.Compiled | RegexOptions.CultureInvariant);

        Assert.Equal(shouldMatch, regex.IsMatch(line));
    }

    [Fact]
    public void TryGetListeningProcessId_ReturnsNullOrPositivePid()
    {
        int? pid = TcpPortHelper.TryGetListeningProcessId(65534);

        if (pid.HasValue)
            Assert.True(pid.Value > 0);
    }
}
