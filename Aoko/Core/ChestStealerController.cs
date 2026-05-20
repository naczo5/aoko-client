using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;

namespace Aoko.Core;

internal sealed class ChestStealerController
{
    [DllImport("user32.dll")]
    private static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);

    [DllImport("user32.dll")]
    private static extern bool GetCursorPos(out POINT lpPoint);

    [DllImport("user32.dll")]
    private static extern short GetAsyncKeyState(int vKey);

    [StructLayout(LayoutKind.Sequential)]
    private struct INPUT
    {
        public uint Type;
        public INPUTUNION U;
    }

    [StructLayout(LayoutKind.Explicit)]
    private struct INPUTUNION
    {
        [FieldOffset(0)]
        public MOUSEINPUT Mi;

        [FieldOffset(0)]
        public KEYBDINPUT Ki;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MOUSEINPUT
    {
        public int Dx;
        public int Dy;
        public uint MouseData;
        public uint DwFlags;
        public uint Time;
        public IntPtr DwExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct KEYBDINPUT
    {
        public ushort WVk;
        public ushort WScan;
        public uint DwFlags;
        public uint Time;
        public IntPtr DwExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT
    {
        public int X;
        public int Y;
    }

    private const uint INPUT_MOUSE = 0;
    private const uint INPUT_KEYBOARD = 1;
    private const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
    private const uint MOUSEEVENTF_LEFTUP = 0x0004;
    private const uint KEYEVENTF_KEYUP = 0x0002;
    private const uint KEYEVENTF_SCANCODE = 0x0008;
    private const int VK_SHIFT = 0x10;
    private const int VK_LSHIFT = 0xA0;
    private const ushort SC_LSHIFT = 0x2A;
    private const int FreshStateMs = 300;
    private const int SlotRetryCooldownMs = 85;

    private readonly object _lock = new();
    private readonly Random _random = new();
    private readonly INPUT[] _leftDown;
    private readonly INPUT[] _leftUp;
    private readonly INPUT[] _shiftDown;
    private readonly INPUT[] _shiftUp;
    private CancellationTokenSource? _cts;
    private Task? _task;
    private bool _syntheticShiftHeld;

    public ChestStealerController()
    {
        _leftDown = new INPUT[1];
        _leftDown[0].Type = INPUT_MOUSE;
        _leftDown[0].U.Mi.DwFlags = MOUSEEVENTF_LEFTDOWN;

        _leftUp = new INPUT[1];
        _leftUp[0].Type = INPUT_MOUSE;
        _leftUp[0].U.Mi.DwFlags = MOUSEEVENTF_LEFTUP;

        _shiftDown = new INPUT[1];
        _shiftDown[0].Type = INPUT_KEYBOARD;
        _shiftDown[0].U.Ki.WScan = SC_LSHIFT;
        _shiftDown[0].U.Ki.DwFlags = KEYEVENTF_SCANCODE;

        _shiftUp = new INPUT[1];
        _shiftUp[0].Type = INPUT_KEYBOARD;
        _shiftUp[0].U.Ki.WScan = SC_LSHIFT;
        _shiftUp[0].U.Ki.DwFlags = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;
    }

    public void Start()
    {
        lock (_lock)
        {
            if (_cts != null) return;
            _cts = new CancellationTokenSource();
            _task = Task.Run(() => RunAsync(_cts.Token));
        }
    }

    public void Stop()
    {
        CancellationTokenSource? cts;
        Task? task;
        lock (_lock)
        {
            cts = _cts;
            task = _task;
            _cts = null;
            _task = null;
        }

        if (cts == null) return;
        cts.Cancel();
        ReleaseSyntheticShift();
        _ = DisposeCtsWhenDoneAsync(cts, task);
    }

    private async Task RunAsync(CancellationToken token)
    {
        int activeWindowId = -1;
        Dictionary<int, long> slotRetryBlockedUntil = new();
        int clickCountThisWindow = 0;

        try
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    Clicker clicker = Clicker.Instance;
                    GameStateClient client = GameStateClient.Instance;
                    GameState state = client.CurrentState;
                    ChestStealerState? chest = state.ChestStealerState;

                    if (!clicker.ChestStealerEnabled ||
                        !client.IsConnected ||
                        !WindowDetection.IsMinecraftForeground() ||
                        !WindowDetection.IsCursorVisible() ||
                        state.LastUpdate == DateTime.MinValue ||
                        (DateTime.Now - state.LastUpdate).TotalMilliseconds > FreshStateMs ||
                        chest is not { Ready: true, Physical: true } ||
                        chest.Slots.Count == 0)
                    {
                        ReleaseSyntheticShift();
                        activeWindowId = -1;
                        slotRetryBlockedUntil.Clear();
                        clickCountThisWindow = 0;
                        await Task.Delay(45, token).ConfigureAwait(false);
                        continue;
                    }

                    if (chest.WindowId != activeWindowId)
                    {
                        ReleaseSyntheticShift();
                        activeWindowId = chest.WindowId;
                        slotRetryBlockedUntil.Clear();
                        clickCountThisWindow = 0;
                    }

                    long nowMs = Environment.TickCount64;
                    PruneRetryBlocks(chest, slotRetryBlockedUntil);
                    SlotSelection? selection = FindNearestLiveSlot(chest, slotRetryBlockedUntil, nowMs);
                    if (selection == null)
                    {
                        await Task.Delay(20, token).ConfigureAwait(false);
                        continue;
                    }

                    int clickElapsedMs = await ClickSlotAsync(chest, selection.Value.Slot, token).ConfigureAwait(false);
                    if (clickElapsedMs >= 0)
                    {
                        slotRetryBlockedUntil[selection.Value.Slot.SlotNumber] = Environment.TickCount64 + SlotRetryCooldownMs;
                        clickCountThisWindow++;
                    }

                    double nextDistance = DistanceToNearestLiveSlot(chest, slotRetryBlockedUntil, Environment.TickCount64);
                    int targetInterval = NextIntervalMs(clicker.ChestStealerDelayMs, nextDistance);
                    int delay = Math.Max(1, targetInterval - clickElapsedMs);
                    await Task.Delay(delay, token).ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    break;
                }
                catch (Exception ex)
                {
                    ReleaseSyntheticShift();
                    Debug.WriteLine($"[ChestStealer] {ex.Message}");
                    await Task.Delay(150, token).ConfigureAwait(false);
                }
            }
        }
        finally
        {
            ReleaseSyntheticShift();
        }
    }

    private readonly struct SlotSelection
    {
        public SlotSelection(ChestStealerSlot slot, double distance)
        {
            Slot = slot;
            Distance = distance;
        }

        public ChestStealerSlot Slot { get; }
        public double Distance { get; }
    }

    private SlotSelection? FindNearestLiveSlot(
        ChestStealerState state,
        Dictionary<int, long> slotRetryBlockedUntil,
        long nowMs)
    {
        WindowDetection.RECT? clientRect = WindowDetection.GetMinecraftClientRectOnScreen();
        if (clientRect == null) return null;

        POINT cursor = GetCursorOrClientCenter(clientRect.Value);

        ChestStealerSlot? best = null;
        double bestDistance = double.MaxValue;
        foreach (ChestStealerSlot slot in state.Slots)
        {
            if (slotRetryBlockedUntil.TryGetValue(slot.SlotNumber, out long blockedUntil) && blockedUntil > nowMs)
                continue;
            if (!TryGetSlotScreenPoint(state, slot, clientRect.Value, out int x, out int y))
                continue;

            long dx = x - cursor.X;
            long dy = y - cursor.Y;
            double distance = Math.Sqrt(dx * dx + dy * dy);
            if (distance < bestDistance)
            {
                bestDistance = distance;
                best = slot;
            }
        }

        return best == null ? null : new SlotSelection(best, bestDistance);
    }

    private double DistanceToNearestLiveSlot(ChestStealerState state, Dictionary<int, long> slotRetryBlockedUntil, long nowMs)
    {
        SlotSelection? selection = FindNearestLiveSlot(state, slotRetryBlockedUntil, nowMs);
        return selection?.Distance ?? 0.0;
    }

    private static void PruneRetryBlocks(ChestStealerState state, Dictionary<int, long> slotRetryBlockedUntil)
    {
        if (slotRetryBlockedUntil.Count == 0) return;

        HashSet<int> liveSlotNumbers = new();
        foreach (ChestStealerSlot slot in state.Slots)
            liveSlotNumbers.Add(slot.SlotNumber);

        List<int>? remove = null;
        foreach (int slotNumber in slotRetryBlockedUntil.Keys)
        {
            if (liveSlotNumbers.Contains(slotNumber)) continue;
            remove ??= new List<int>();
            remove.Add(slotNumber);
        }

        if (remove == null) return;
        foreach (int slotNumber in remove)
            slotRetryBlockedUntil.Remove(slotNumber);
    }

    private async Task<int> ClickSlotAsync(ChestStealerState state, ChestStealerSlot slot, CancellationToken token)
    {
        var stopwatch = Stopwatch.StartNew();
        WindowDetection.RECT? clientRect = WindowDetection.GetMinecraftClientRectOnScreen();
        if (clientRect == null) return -1;
        if (!TryGetSlotScreenPoint(state, slot, clientRect.Value, out int x, out int y))
            return -1;

        x += _random.Next(-2, 3);
        y += _random.Next(-2, 3);
        if (!SetCursorPos(x, y))
            return -1;

        EnsureShiftHeld();
        await Task.Delay(_random.Next(4, 10), token).ConfigureAwait(false);

        bool leftDownSent = false;
        try
        {
            SendInput(1, _leftDown, Marshal.SizeOf<INPUT>());
            leftDownSent = true;
            await Task.Delay(_random.Next(8, 18), token).ConfigureAwait(false);
        }
        finally
        {
            if (leftDownSent)
                SendInput(1, _leftUp, Marshal.SizeOf<INPUT>());
        }

        stopwatch.Stop();
        return (int)stopwatch.ElapsedMilliseconds;
    }

    private bool TryGetSlotScreenPoint(ChestStealerState state, ChestStealerSlot slot, WindowDetection.RECT clientRect, out int x, out int y)
        => ChestStealerCoordinateMapper.TryMapScaledPoint(state, slot, clientRect, out x, out y);

    private POINT GetCursorOrClientCenter(WindowDetection.RECT clientRect)
    {
        if (GetCursorPos(out POINT cursor))
            return cursor;

        return new POINT
        {
            X = (clientRect.Left + clientRect.Right) / 2,
            Y = (clientRect.Top + clientRect.Bottom) / 2
        };
    }

    private void EnsureShiftHeld()
    {
        bool shiftDown =
            (GetAsyncKeyState(VK_SHIFT) & unchecked((short)0x8000)) != 0 ||
            (GetAsyncKeyState(VK_LSHIFT) & unchecked((short)0x8000)) != 0;

        if (shiftDown || _syntheticShiftHeld)
        {
            if (_syntheticShiftHeld)
                SendInput(1, _shiftDown, Marshal.SizeOf<INPUT>());
            return;
        }

        SendInput(1, _shiftDown, Marshal.SizeOf<INPUT>());
        _syntheticShiftHeld = true;
    }

    private void ReleaseSyntheticShift()
    {
        if (!_syntheticShiftHeld)
            return;

        SendInput(1, _shiftUp, Marshal.SizeOf<INPUT>());
        _syntheticShiftHeld = false;
    }

    private int NextIntervalMs(int configuredDelayMs, double nextDistancePx)
    {
        int baseDelay = Math.Clamp(configuredDelayMs, 50, 500);
        double multiplier = nextDistancePx switch
        {
            <= 0.0 => 1.0,
            <= 24.0 => 0.78 + _random.NextDouble() * 0.14,
            <= 70.0 => 0.90 + _random.NextDouble() * 0.16,
            <= 140.0 => 1.00 + _random.NextDouble() * 0.18,
            <= 240.0 => 1.10 + _random.NextDouble() * 0.22,
            _ => 1.22 + _random.NextDouble() * 0.26
        };
        int jitter = Math.Max(8, baseDelay / 6);
        int interval = (int)Math.Round(baseDelay * multiplier) + _random.Next(-jitter, jitter + 1);
        return Math.Clamp(interval, 35, 750);
    }

    private static async Task DisposeCtsWhenDoneAsync(CancellationTokenSource cts, Task? task)
    {
        try
        {
            if (task != null)
                await task.ConfigureAwait(false);
        }
        catch (OperationCanceledException) { }
        finally
        {
            cts.Dispose();
        }
    }
}
