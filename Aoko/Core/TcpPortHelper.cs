using System;
using System.Diagnostics;
using System.Text.RegularExpressions;

namespace Aoko.Core;

internal static class TcpPortHelper
{
    public static int? TryGetListeningProcessId(int port)
    {
        try
        {
            var psi = new ProcessStartInfo
            {
                FileName = "netstat",
                Arguments = "-ano -p tcp",
                RedirectStandardOutput = true,
                UseShellExecute = false,
                CreateNoWindow = true
            };

            using var process = Process.Start(psi);
            if (process == null)
                return null;

            string output = process.StandardOutput.ReadToEnd();
            process.WaitForExit(5000);

            var regex = new Regex(
                $@"\s(?:0\.0\.0\.0|127\.0\.0\.1|\[::\]|\[::1\]):{port}(\s|$)",
                RegexOptions.Compiled | RegexOptions.CultureInvariant);

            foreach (string rawLine in output.Split('\n'))
            {
                string line = rawLine.Trim();
                if (!line.Contains("LISTENING", StringComparison.OrdinalIgnoreCase))
                    continue;
                if (!regex.IsMatch(line))
                    continue;

                string[] parts = line.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
                if (parts.Length >= 5 && int.TryParse(parts[^1], out int pid) && pid > 0)
                    return pid;
            }
        }
        catch
        {
        }

        return null;
    }
}
