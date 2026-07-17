using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;

namespace Aoko.Core;

/// <summary>
/// Reads another process's command line (via PEB) and extracts Minecraft version tokens.
/// Works for Prism/vanilla/Fabric/Forge (jar paths, --version) as well as Lunar Genesis.
/// </summary>
internal static class ProcessCommandLine
{
    private const uint ProcessQueryInformation = 0x0400;
    private const uint ProcessQueryLimitedInformation = 0x1000;
    private const uint ProcessVmRead = 0x0010;
    private const int ProcessBasicInformationClass = 0;

    // Prism/vanilla game arg, Lunar Genesis: --version 26.2
    private static readonly Regex VersionArgRegex = new(
        @"--version(?:=|\s+)(?<ver>[\w.\-]+)",
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant | RegexOptions.Compiled);

    // Prism/Fabric: .../minecraft/26.2/minecraft-26.2-client.jar
    private static readonly Regex MinecraftClientJarRegex = new(
        @"minecraft-(?<ver>\d+(?:\.\d+)+)-client\.jar",
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant | RegexOptions.Compiled);

    // Fabric intermediary: .../intermediary/26.2/intermediary-26.2.jar
    private static readonly Regex IntermediaryRegex = new(
        @"intermediary[/-](?<ver>\d+(?:\.\d+)+)",
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant | RegexOptions.Compiled);

    // Official launcher layout: .../versions/26.2/...
    private static readonly Regex VersionsDirRegex = new(
        @"[\\/]versions[\\/](?<ver>\d+(?:\.\d+)+)[\\/]",
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant | RegexOptions.Compiled);

    // Lunar natives zip markers: v26_2, v1_8, v1_21_11
    private static readonly Regex NativesMarkerRegex = new(
        @"[vV](?<ver>26_\d+(?:_\d+)?|1_21(?:_\d+)?|1_8(?:_\d+)?)\b",
        RegexOptions.CultureInvariant | RegexOptions.Compiled);

    public static string? TryGet(int processId)
    {
        if (processId <= 0)
            return null;

        IntPtr handle = OpenProcess(ProcessQueryInformation | ProcessVmRead, false, processId);
        if (handle == IntPtr.Zero)
            handle = OpenProcess(ProcessQueryLimitedInformation | ProcessVmRead, false, processId);
        if (handle == IntPtr.Zero)
            return null;

        try
        {
            if (IntPtr.Size != 8)
                return null;

            if (IsWow64Process(handle, out bool isWow64) && isWow64)
                return null;

            var pbi = new ProcessBasicInformation();
            int status = NtQueryInformationProcess(handle, ProcessBasicInformationClass, ref pbi, Marshal.SizeOf<ProcessBasicInformation>(), out _);
            if (status != 0 || pbi.PebBaseAddress == IntPtr.Zero)
                return null;

            // PEB->ProcessParameters is at offset 0x20 on x64.
            if (!TryReadIntPtr(handle, pbi.PebBaseAddress + 0x20, out IntPtr processParams) || processParams == IntPtr.Zero)
                return null;

            // RTL_USER_PROCESS_PARAMETERS.CommandLine (UNICODE_STRING) at offset 0x70 on x64.
            if (!TryReadUInt16(handle, processParams + 0x70, out ushort lengthBytes) || lengthBytes == 0)
                return null;
            if (!TryReadIntPtr(handle, processParams + 0x78, out IntPtr buffer) || buffer == IntPtr.Zero)
                return null;

            int charCount = lengthBytes / 2;
            if (charCount <= 0 || charCount > 32768)
                return null;

            byte[] raw = new byte[lengthBytes];
            if (!ReadProcessMemory(handle, buffer, raw, (uint)raw.Length, out IntPtr read) || read.ToInt64() != raw.Length)
                return null;

            return Encoding.Unicode.GetString(raw);
        }
        catch
        {
            return null;
        }
        finally
        {
            CloseHandle(handle);
        }
    }

    public static string? TryGet(Process? process)
    {
        if (process == null)
            return null;

        try
        {
            return TryGet(process.Id);
        }
        catch
        {
            return null;
        }
    }

    /// <summary>
    /// Parses a JVM command line for Minecraft version markers from any launcher.
    /// Does not scan the full string with loose substring checks (avoids log4j <c>2.26.0</c> false hits).
    /// </summary>
    public static string? TryParseVersion(string? commandLine)
    {
        if (string.IsNullOrWhiteSpace(commandLine))
            return null;

        string? fromArg = NormalizeGroup(VersionArgRegex.Match(commandLine));
        if (!string.IsNullOrEmpty(fromArg))
            return fromArg;

        string? fromClientJar = NormalizeGroup(MinecraftClientJarRegex.Match(commandLine));
        if (!string.IsNullOrEmpty(fromClientJar))
            return fromClientJar;

        string? fromIntermediary = NormalizeGroup(IntermediaryRegex.Match(commandLine));
        if (!string.IsNullOrEmpty(fromIntermediary))
            return fromIntermediary;

        string? fromVersionsDir = NormalizeGroup(VersionsDirRegex.Match(commandLine));
        if (!string.IsNullOrEmpty(fromVersionsDir))
            return fromVersionsDir;

        Match natives = NativesMarkerRegex.Match(commandLine);
        if (natives.Success)
        {
            string raw = natives.Groups["ver"].Value.Replace('_', '.');
            string? normalized = GameStateClient.NormalizeDetectedVersion(raw);
            if (!string.IsNullOrEmpty(normalized))
                return normalized;
        }

        return null;
    }

    private static string? NormalizeGroup(Match match)
    {
        if (!match.Success)
            return null;
        return GameStateClient.NormalizeDetectedVersion(match.Groups["ver"].Value);
    }

    private static bool TryReadIntPtr(IntPtr process, IntPtr address, out IntPtr value)
    {
        value = IntPtr.Zero;
        byte[] buf = new byte[IntPtr.Size];
        if (!ReadProcessMemory(process, address, buf, (uint)buf.Length, out IntPtr read) || read.ToInt64() != buf.Length)
            return false;
        value = (IntPtr)BitConverter.ToInt64(buf, 0);
        return true;
    }

    private static bool TryReadUInt16(IntPtr process, IntPtr address, out ushort value)
    {
        value = 0;
        byte[] buf = new byte[2];
        if (!ReadProcessMemory(process, address, buf, 2, out IntPtr read) || read.ToInt64() != 2)
            return false;
        value = BitConverter.ToUInt16(buf, 0);
        return true;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessBasicInformation
    {
        public IntPtr Reserved1;
        public IntPtr PebBaseAddress;
        public IntPtr Reserved2_0;
        public IntPtr Reserved2_1;
        public IntPtr UniqueProcessId;
        public IntPtr Reserved3;
    }

    [DllImport("ntdll.dll")]
    private static extern int NtQueryInformationProcess(
        IntPtr processHandle,
        int processInformationClass,
        ref ProcessBasicInformation processInformation,
        int processInformationLength,
        out int returnLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr OpenProcess(uint desiredAccess, bool inheritHandle, int processId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool ReadProcessMemory(
        IntPtr hProcess,
        IntPtr lpBaseAddress,
        [Out] byte[] lpBuffer,
        uint nSize,
        out IntPtr lpNumberOfBytesRead);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool CloseHandle(IntPtr hObject);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool IsWow64Process(IntPtr hProcess, out bool wow64Process);
}
