using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;

namespace Aoko.Core;

/// <summary>
/// Moves healing items (pots, golden apples) from the backpack into empty
/// hotbar slots while the survival inventory screen is open. The bridge
/// reports candidate slot coordinates via <see cref="GameState.RefillState"/>;
/// this controller performs the shift-clicks with the physical mouse so the
/// game's own input path emits the window clicks.
/// </summary>
internal sealed class RefillController
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
    private const int SlotRetryCooldownMs = 150;

    private readonly object _lock = new();
    private readonly Random _random = new();
    private readonly INPUT[] _leftDown;
    private readonly INPUT[] _leftUp;
    private readonly INPUT[] _shiftDown;
    private readonly INPUT[] _shiftUp;
    private CancellationTokenSource? _cts;
    private Task? _task;
    private bool _syntheticShiftHeld;

    public RefillController()
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

        try
        {
            while (!token.IsCancellationRequested)
            {
                try
                {
                    Clicker clicker = Clicker.Instance;
                    GameStateClient client = GameStateClient.Instance;
                    GameState state = client.CurrentState;
                    RefillState? refill = state.RefillState;

                    bool guiActive = state.GuiOpen || WindowDetection.IsCursorVisible();
                    if (!clicker.RefillEnabled ||
                        !client.IsConnected ||
                        !WindowDetection.IsMinecraftForeground() ||
                        !guiActive ||
                        state.LastUpdate == DateTime.MinValue ||
                        (DateTime.Now - state.LastUpdate).TotalMilliseconds > FreshStateMs ||
                        refill is not { Ready: true } ||
                        refill.Slots.Count == 0)
                    {
                        ReleaseSyntheticShift();
                        activeWindowId = -1;
                        slotRetryBlockedUntil.Clear();
                        await Task.Delay(45, token).ConfigureAwait(false);
                        continue;
                    }

                    if (refill.WindowId != activeWindowId)
                    {
                        activeWindowId = refill.WindowId;
                        slotRetryBlockedUntil.Clear();
                    }

                    PruneRetryBlocks(refill, slotRetryBlockedUntil);

                    WindowDetection.RECT? clientRect = WindowDetection.GetMinecraftClientRectOnScreen();
                    if (clientRect == null)
                    {
                        await Task.Delay(20, token).ConfigureAwait(false);
                        continue;
                    }

                    long nowMs = Environment.TickCount64;
                    ChestStealerSlot? target = null;
                    double bestDistance = double.MaxValue;
                    POINT cursor = GetCursorOrClientCenter(clientRect.Value);
                    foreach (ChestStealerSlot slot in refill.Slots)
                    {
                        if (slotRetryBlockedUntil.TryGetValue(slot.SlotNumber, out long blockedUntil) && blockedUntil > nowMs)
                            continue;
                        if (!ChestStealerCoordinateMapper.TryMapScaledPoint(refill.ToChestStealerState(), slot, clientRect.Value, out int x, out int y))
                            continue;

                        long dx = x - cursor.X;
                        long dy = y - cursor.Y;
                        double distance = Math.Sqrt(dx * dx + dy * dy);
                        if (distance < bestDistance)
                        {
                            bestDistance = distance;
                            target = slot;
                        }
                    }

                    if (target == null)
                    {
                        // Every reported pot was recently clicked; wait for the
                        // bridge to re-scan the inventory before trying again.
                        await Task.Delay(30, token).ConfigureAwait(false);
                        continue;
                    }

                    int clickElapsedMs = await ClickSlotAsync(refill, target, clientRect.Value, token).ConfigureAwait(false);
                    if (clickElapsedMs >= 0)
                        slotRetryBlockedUntil[target.SlotNumber] = Environment.TickCount64 + SlotRetryCooldownMs;

                    await Task.Delay(Math.Max(1, clicker.RefillDelayMs - clickElapsedMs), token).ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    break;
                }
                catch (Exception ex)
                {
                    ReleaseSyntheticShift();
                    Debug.WriteLine($"[Refill] {ex.Message}");
                    await Task.Delay(150, token).ConfigureAwait(false);
                }
            }
        }
        finally
        {
            ReleaseSyntheticShift();
        }
    }

    private async Task<int> ClickSlotAsync(RefillState state, ChestStealerSlot slot, WindowDetection.RECT clientRect, CancellationToken token)
    {
        var stopwatch = Stopwatch.StartNew();
        if (!ChestStealerCoordinateMapper.TryMapScaledPoint(state.ToChestStealerState(), slot, clientRect, out int x, out int y))
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

    private static void PruneRetryBlocks(RefillState state, Dictionary<int, long> slotRetryBlockedUntil)
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
