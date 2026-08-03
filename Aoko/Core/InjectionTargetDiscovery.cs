using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;

namespace Aoko.Core;

/// <summary>
/// A Java process that can be selected as an injection target.
/// </summary>
public sealed record InjectionTarget(
    int ProcessId,
    IntPtr Hwnd,
    string Title,
    string ProcessName,
    string ClientType,
    string DetectedVersion,
    string DetectionReason,
    int Confidence)
{
    public bool IsLikelyMinecraft => Confidence >= 60;

    public string DisplayLabel
    {
        get
        {
            string window = string.IsNullOrWhiteSpace(Title) ? "No titled window" : Title;
            string version = string.IsNullOrWhiteSpace(DetectedVersion) ? "Auto-detect" : DetectedVersion;
            return $"{ClientType} — {window} ({ProcessName}, PID {ProcessId})\nVersion: {version} • {DetectionReason}";
        }
    }
}

/// <summary>
/// Finds Java processes that look like Minecraft clients and supplies enough metadata
/// for the loader to make a safe automatic choice or explain a manual choice.
/// </summary>
public static class InjectionTargetDiscovery
{
    private static readonly string[] JavaProcessNames = { "java", "javaw" };
    private static readonly string[] LunarMarkers = { "lunarclient", ".lunarclient", "lunar client" };
    private static readonly string[] BadlionMarkers = { "badlion" };
    private static readonly string[] MinecraftMarkers =
    {
        "minecraft",
        "net.minecraft",
        "mojang",
        "forge",
        "fabric",
        "quilt",
        "feather",
        "labymod"
    };

    public static IReadOnlyList<InjectionTarget> ListTargets()
    {
        try
        {
            var windowsByProcess = WindowDetection.ListSelectableWindows()
                .Where(window => window.IsJvm)
                .GroupBy(window => window.ProcessId)
                .ToDictionary(group => group.Key, SelectBestWindow);

            var targets = new List<InjectionTarget>();
            foreach (Process process in Process.GetProcesses())
            {
                using (process)
                {
                    try
                    {
                        if (process.Id == Environment.ProcessId
                            || !JavaProcessNames.Contains(process.ProcessName, StringComparer.OrdinalIgnoreCase))
                        {
                            continue;
                        }

                        windowsByProcess.TryGetValue(process.Id, out WindowTarget? window);
                        string title = window?.Title ?? TryGetMainWindowTitle(process);
                        IntPtr hwnd = window?.Hwnd ?? TryGetMainWindowHandle(process);
                        string? commandLine = ProcessCommandLine.TryGet(process);
                        string? executablePath = TryGetExecutablePath(process);

                        targets.Add(Describe(
                            process.Id,
                            hwnd,
                            title,
                            process.ProcessName,
                            commandLine,
                            executablePath));
                    }
                    catch (Exception ex)
                    {
                        Debug.WriteLine($"[InjectionTargetDiscovery] Could not inspect PID {process.Id}: {ex.Message}");
                    }
                }
            }

            // Keep every Java process in the picker. The user explicitly chooses when
            // more than one exists; confidence only controls the ordering and warning.
            return SortTargets(targets);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[InjectionTargetDiscovery] Error: {ex.Message}");
            return Array.Empty<InjectionTarget>();
        }
    }

    public static InjectionTarget? FindBestTarget()
    {
        var targets = ListTargets();
        return targets.FirstOrDefault(target => target.IsLikelyMinecraft) ?? targets.FirstOrDefault();
    }

    internal static List<InjectionTarget> SortTargets(IEnumerable<InjectionTarget> targets)
    {
        return targets
            .OrderByDescending(target => target.Confidence)
            .ThenByDescending(target => target.Hwnd != IntPtr.Zero)
            .ThenBy(target => target.ClientType, StringComparer.OrdinalIgnoreCase)
            .ThenBy(target => target.Title, StringComparer.OrdinalIgnoreCase)
            .ThenBy(target => target.ProcessId)
            .ToList();
    }

    internal static InjectionTarget Describe(
        int processId,
        IntPtr hwnd,
        string? title,
        string processName,
        string? commandLine,
        string? executablePath)
    {
        string safeTitle = title?.Trim() ?? string.Empty;
        string metadata = string.Join(
            " ",
            safeTitle,
            commandLine ?? string.Empty,
            executablePath ?? string.Empty).ToLowerInvariant();

        string clientType;
        string reason;
        int confidence;

        if (ContainsAny(metadata, LunarMarkers))
        {
            clientType = "Lunar Client";
            reason = "Lunar markers in process metadata";
            confidence = 100;
        }
        else if (ContainsAny(metadata, BadlionMarkers))
        {
            clientType = "Badlion";
            reason = "Badlion marker in process metadata";
            confidence = 95;
        }
        else if (ContainsAny(metadata, MinecraftMarkers))
        {
            clientType = "Minecraft Java";
            reason = "Minecraft/client marker in process metadata";
            confidence = 85;
        }
        else
        {
            clientType = "Java process";
            reason = string.IsNullOrWhiteSpace(safeTitle)
                ? "No client marker found"
                : "Java process with a visible window";
            confidence = string.IsNullOrWhiteSpace(safeTitle) ? 10 : 35;
        }

        string? detectedVersion = ProcessCommandLine.TryParseVersion(commandLine)
            ?? GameStateClient.NormalizeDetectedVersion(safeTitle);
        if (!string.IsNullOrWhiteSpace(detectedVersion) && confidence < 60)
        {
            confidence = 70;
            reason = "Minecraft version found in process metadata";
            clientType = "Minecraft Java";
        }

        return new InjectionTarget(
            processId,
            hwnd,
            safeTitle,
            processName,
            clientType,
            detectedVersion ?? string.Empty,
            reason,
            confidence);
    }

    private static WindowTarget SelectBestWindow(IEnumerable<WindowTarget> windows)
    {
        return windows
            .OrderByDescending(window => IsGameTitle(window.Title))
            .ThenByDescending(window => window.Hwnd != IntPtr.Zero)
            .First();
    }

    private static bool IsGameTitle(string title)
    {
        return title.Contains("minecraft", StringComparison.OrdinalIgnoreCase)
            || title.Contains("lunar", StringComparison.OrdinalIgnoreCase)
            || title.Contains("badlion", StringComparison.OrdinalIgnoreCase);
    }

    private static bool ContainsAny(string value, IEnumerable<string> markers)
    {
        return markers.Any(marker => value.Contains(marker, StringComparison.OrdinalIgnoreCase));
    }

    private static string TryGetMainWindowTitle(Process process)
    {
        try
        {
            return process.MainWindowTitle?.Trim() ?? string.Empty;
        }
        catch
        {
            return string.Empty;
        }
    }

    private static string? TryGetExecutablePath(Process process)
    {
        try
        {
            return process.MainModule?.FileName;
        }
        catch
        {
            return null;
        }
    }

    private static IntPtr TryGetMainWindowHandle(Process process)
    {
        try
        {
            return process.MainWindowHandle;
        }
        catch
        {
            return IntPtr.Zero;
        }
    }
}
