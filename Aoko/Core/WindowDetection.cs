using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;

namespace Aoko.Core;

public sealed record WindowTarget(IntPtr Hwnd, int ProcessId, string Title, string ProcessName, bool IsJvm)
{
    public string DisplayLabel => IsJvm
        ? $"[JVM] {Title} — {ProcessName} (PID {ProcessId})"
        : $"{Title} — {ProcessName} (PID {ProcessId})";
}

public static class WindowDetection
{
    [DllImport("user32.dll")]
    private static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern bool IsWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll")]
    private static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    private static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    private static extern bool ClientToScreen(IntPtr hWnd, ref POINT lpPoint);

    private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    private static readonly string[] GameTitles = { "Minecraft", "Lunar Client", "Badlion" };

    private static IntPtr _foundWindow = IntPtr.Zero;
    private static IntPtr _customTargetHwnd = IntPtr.Zero;
    private static readonly List<WindowTarget> _selectableWindows = new();

    public static void SetCustomTarget(IntPtr hwnd)
    {
        _customTargetHwnd = hwnd;
    }

    public static void ClearCustomTarget()
    {
        _customTargetHwnd = IntPtr.Zero;
    }

    public static IntPtr GetForegroundWindowHandle() => GetForegroundWindow();

    public static int GetWindowProcessId(IntPtr hwnd)
    {
        if (hwnd == IntPtr.Zero)
            return 0;

        GetWindowThreadProcessId(hwnd, out uint pid);
        return (int)pid;
    }

    public static IReadOnlyList<WindowTarget> ListSelectableWindows()
    {
        _selectableWindows.Clear();
        EnumWindows(SelectableWindowCallback, IntPtr.Zero);
        return SortWindowTargets(_selectableWindows);
    }

    internal static List<WindowTarget> SortWindowTargets(IEnumerable<WindowTarget> targets)
    {
        return targets
            .OrderByDescending(t => t.IsJvm)
            .ThenBy(t => t.Title, StringComparer.OrdinalIgnoreCase)
            .ToList();
    }

    private static bool SelectableWindowCallback(IntPtr hWnd, IntPtr lParam)
    {
        if (!IsWindowVisible(hWnd))
            return true;

        StringBuilder title = new(256);
        GetWindowText(hWnd, title, 256);
        string windowTitle = title.ToString().Trim();
        if (string.IsNullOrEmpty(windowTitle))
            return true;

        GetWindowThreadProcessId(hWnd, out uint pid);
        if (pid == 0 || pid == (uint)Environment.ProcessId)
            return true;

        string processName = ResolveProcessName((int)pid);
        bool isJvm = processName.Equals("java", StringComparison.OrdinalIgnoreCase)
                  || processName.Equals("javaw", StringComparison.OrdinalIgnoreCase);

        _selectableWindows.Add(new WindowTarget(hWnd, (int)pid, windowTitle, processName, isJvm));
        return true;
    }

    private static string ResolveProcessName(int processId)
    {
        try
        {
            using var proc = Process.GetProcessById(processId);
            return proc.ProcessName;
        }
        catch
        {
            return "unknown";
        }
    }

    public static bool IsMinecraftActive()
    {
        IntPtr hwnd = GetForegroundWindow();
        if (hwnd == IntPtr.Zero) return false;

        IntPtr target = FindMinecraftWindow();
        if (target != IntPtr.Zero && hwnd == target)
            return true;

        StringBuilder title = new(256);
        GetWindowText(hwnd, title, 256);
        string windowTitle = title.ToString();

        foreach (string gameTitle in GameTitles)
        {
            if (windowTitle.Contains(gameTitle, StringComparison.OrdinalIgnoreCase))
                return true;
        }

        return false;
    }

    /// <summary>
    /// Checks if Minecraft window is the FOREGROUND window (not just visible).
    /// This means no other window is on top of it.
    /// </summary>
    public static bool IsMinecraftForeground()
    {
        IntPtr foreground = GetForegroundWindow();
        if (foreground == IntPtr.Zero) return false;

        IntPtr minecraft = FindMinecraftWindow();
        if (minecraft == IntPtr.Zero) return false;

        return foreground == minecraft;
    }

    public static IntPtr FindMinecraftWindow()
    {
        if (_customTargetHwnd != IntPtr.Zero && IsWindow(_customTargetHwnd))
            return _customTargetHwnd;

        _foundWindow = IntPtr.Zero;
        EnumWindows(EnumWindowCallback, IntPtr.Zero);
        return _foundWindow;
    }

    private static bool EnumWindowCallback(IntPtr hWnd, IntPtr lParam)
    {
        if (!IsWindowVisible(hWnd)) return true;

        StringBuilder title = new(256);
        GetWindowText(hWnd, title, 256);
        string windowTitle = title.ToString();

        foreach (string gameTitle in GameTitles)
        {
            if (windowTitle.Contains(gameTitle, StringComparison.OrdinalIgnoreCase))
            {
                _foundWindow = hWnd;
                return false;
            }
        }

        return true;
    }

    public static RECT? GetMinecraftWindowRect()
    {
        IntPtr hwnd = FindMinecraftWindow();
        if (hwnd == IntPtr.Zero) return null;

        if (GetWindowRect(hwnd, out RECT rect))
            return rect;

        return null;
    }

    public static RECT? GetMinecraftClientRectOnScreen()
    {
        IntPtr hwnd = FindMinecraftWindow();
        if (hwnd == IntPtr.Zero) return null;

        if (!GetClientRect(hwnd, out RECT client))
            return null;

        POINT topLeft = new() { x = client.Left, y = client.Top };
        POINT bottomRight = new() { x = client.Right, y = client.Bottom };
        if (!ClientToScreen(hwnd, ref topLeft) || !ClientToScreen(hwnd, ref bottomRight))
            return null;

        return new RECT
        {
            Left = topLeft.x,
            Top = topLeft.y,
            Right = bottomRight.x,
            Bottom = bottomRight.y
        };
    }

    [DllImport("user32.dll")]
    private static extern bool GetCursorInfo(ref CURSORINFO pci);

    [StructLayout(LayoutKind.Sequential)]
    private struct CURSORINFO
    {
        public int cbSize;
        public int flags;
        public IntPtr hCursor;
        public POINT ptScreenPos;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT
    {
        public int x;
        public int y;
    }

    private const int CURSOR_SHOWING = 0x00000001;

    public static bool IsCursorVisible()
    {
        CURSORINFO pci = new CURSORINFO();
        pci.cbSize = Marshal.SizeOf(typeof(CURSORINFO));

        if (GetCursorInfo(ref pci))
        {
            return (pci.flags & CURSOR_SHOWING) != 0;
        }

        return false;
    }
}
