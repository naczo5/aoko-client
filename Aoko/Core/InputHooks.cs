using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Runtime.InteropServices;
using System.Windows;

namespace Aoko.Core;

public static class InputHooks
{
    // P/Invoke declarations
    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr SetWindowsHookEx(int idHook, LowLevelProc lpfn, IntPtr hMod, uint dwThreadId);
    
    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool UnhookWindowsHookEx(IntPtr hhk);
    
    [DllImport("user32.dll")]
    private static extern IntPtr CallNextHookEx(IntPtr hhk, int nCode, IntPtr wParam, IntPtr lParam);
    
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetModuleHandle(string? lpModuleName);
    
    private delegate IntPtr LowLevelProc(int nCode, IntPtr wParam, IntPtr lParam);
    
    private const int WH_KEYBOARD_LL = 13;
    private const int WH_MOUSE_LL = 14;
    private const int WM_KEYDOWN = 0x0100;
    private const int WM_KEYUP = 0x0101;
    private const int WM_LBUTTONDOWN = 0x0201;
    private const int WM_LBUTTONUP = 0x0202;
    private const int WM_RBUTTONDOWN = 0x0204;
    private const int WM_RBUTTONUP = 0x0205;
    private const int WM_MBUTTONDOWN = 0x0207;
    private const int WM_MBUTTONUP = 0x0208;
    private const int WM_XBUTTONDOWN = 0x020B;
    private const int WM_XBUTTONUP = 0x020C;
    private const int VK_LBUTTON = 0x01;
    private const int VK_RBUTTON = 0x02;
    private const int VK_MBUTTON = 0x04;
    private const int VK_XBUTTON1 = 0x05;
    private const int VK_XBUTTON2 = 0x06;
    private const int VK_OEM_3 = 0xC0; // Backtick key
    private const uint LLMHF_INJECTED = 0x00000001;
    
    [StructLayout(LayoutKind.Sequential)]
    private struct KBDLLHOOKSTRUCT
    {
        public uint VkCode;
        public uint ScanCode;
        public uint Flags;
        public uint Time;
        public IntPtr DwExtraInfo;
    }
    
    [StructLayout(LayoutKind.Sequential)]
    private struct MSLLHOOKSTRUCT
    {
        public int X;
        public int Y;
        public uint MouseData;
        public uint Flags;
        public uint Time;
        public IntPtr DwExtraInfo;
    }
    
    private static IntPtr _keyboardHook = IntPtr.Zero;
    private static IntPtr _mouseHook = IntPtr.Zero;
    
    // Keep delegates alive to prevent GC
    private static LowLevelProc? _keyboardProc;
    private static LowLevelProc? _mouseProc;
    
    // Per-module keybinds: moduleId -> VK code (0 = unbound).
    // Keep in sync with ModuleCatalog entries that require KeybindMaps.
    public static Dictionary<string, int> ModuleKeys { get; } = new()
    {
        ["autoclicker"]      = 0,
        ["rightclick"]       = 0,
        ["jitter"]           = 0,
        ["clickinchests"]    = 0,
        ["breakblocks"]      = 0,
        ["aimassist"]        = 0,
        ["triggerbot"]       = 0,
        ["killaura"]         = 0,
        ["speedbridge"]      = 0,
        ["gtbhelper"]        = 0,
        ["pixelpartyassist"] = 0,
        ["nametags"]         = 0,
        ["nickhider"]        = 0,
        ["closestplayer"]    = 0,
        ["fightstatus"]      = 0,
        ["chestesp"]         = 0,
        ["cheststealer"]     = 0,
        ["blockesp"]         = 0,
        ["reach"]            = 0,
        ["velocity"]         = 0,
        ["autototem"]        = 0,
        ["autorod"]          = 0,
        ["antidebuff"]       = 0,
        ["hitdelayfix"]     = 0,
        ["panic"]            = 0,
        ["hudeditor"]        = 0,
    };

    public static int AutoRodActionKey { get; private set; }

    public static bool SetModuleKey(string moduleId, int vk)
    {
        if (vk > 0 && vk == AutoRodActionKey)
            return false;

        ModuleKeys[moduleId] = vk;
        OnStateChanged?.Invoke();
        return true;
    }

    public static bool SetAutoRodActionKey(int vk)
    {
        if (vk > 0 && ModuleKeys.Values.Contains(vk))
            return false;

        AutoRodActionLatch.End();
        AutoRodActionKey = vk;
        OnStateChanged?.Invoke();
        return true;
    }

    public static int GetModuleKey(string moduleId)
        => ModuleKeys.TryGetValue(moduleId, out int vk) ? vk : 0;

    internal static bool ShouldConsumeAutoRodAction(
        bool enabled, bool supported, bool connected, bool minecraftForeground,
        bool inWorld, bool anyScreenOpen)
        => enabled && supported && connected && minecraftForeground && inWorld && !anyScreenOpen;

    internal static bool IsAnyGameScreenOpen(GameState state)
    {
        if (state.GuiOpen)
            return true;

        string screenName = state.ScreenName?.Trim() ?? string.Empty;
        return screenName.Length > 0 &&
            !screenName.Equals("none", StringComparison.OrdinalIgnoreCase) &&
            !screenName.Equals("unknown", StringComparison.OrdinalIgnoreCase);
    }

    private static bool CanConsumeAutoRodAction()
    {
        var client = GameStateClient.Instance;
        GameState state = client.CurrentState;
        return ShouldConsumeAutoRodAction(
            Clicker.Instance.AutoRodEnabled,
            client.SupportsModule("autorod"),
            client.IsConnected,
            WindowDetection.IsMinecraftForeground(),
            state.InWorld,
            IsAnyGameScreenOpen(state));
    }

    internal sealed class PressLatch
    {
        private bool _isDown;
        private bool _consume;

        public bool Begin(bool canConsume, out bool trigger)
        {
            trigger = false;
            if (_isDown)
                return _consume;

            _isDown = true;
            _consume = canConsume;
            trigger = canConsume;
            return _consume;
        }

        public bool End()
        {
            bool consume = _consume;
            _isDown = false;
            _consume = false;
            return consume;
        }
    }

    private static readonly PressLatch AutoRodActionLatch = new();

    // Key capture mode for rebinding (reserved for future use)
    public static bool IsCapturingKey { get; private set; } = false;
    private static bool _captureAllowsMouse;
    public static event Action<int>? OnKeyCaptured;

    public static event Action? OnToggleRequested;
    public static event Action? OnStateChanged;

    public static bool IsPhysicalLeftButtonDown { get; private set; } = false;

    public static void StartKeyCapture(bool allowMouse = false)
    {
        _captureAllowsMouse = allowMouse;
        IsCapturingKey = true;
    }

    public static void StopKeyCapture()
    {
        IsCapturingKey = false;
        _captureAllowsMouse = false;
    }

    private static void ToggleModule(string moduleId)
    {
        if (string.Equals(moduleId, "panic", StringComparison.OrdinalIgnoreCase))
        {
            Clicker.Instance.TriggerPanic();
            return;
        }

        if (!GameStateClient.Instance.SupportsModule(moduleId))
            return;

        if (ModuleCatalog.IsDevOnly(moduleId) && !Clicker.Instance.DevMode)
            return;

        var c = Clicker.Instance;
        switch (moduleId)
        {
            case "autoclicker":      c.ToggleArmed(); break;
            case "rightclick":       c.RightClickEnabled = !c.RightClickEnabled; break;
            case "jitter":           c.JitterEnabled = !c.JitterEnabled; break;
            case "clickinchests":    c.ClickInChests = !c.ClickInChests; break;
            case "breakblocks":      c.BreakBlocksEnabled = !c.BreakBlocksEnabled; break;
            case "aimassist":        c.AimAssistEnabled = !c.AimAssistEnabled; break;
            case "triggerbot":       c.TriggerbotEnabled = !c.TriggerbotEnabled; break;
            case "killaura":         c.KillAuraEnabled = !c.KillAuraEnabled; break;
            case "speedbridge":      c.SpeedBridgeEnabled = !c.SpeedBridgeEnabled; break;
            case "gtbhelper":        c.GtbHelperEnabled = !c.GtbHelperEnabled; break;
            case "pixelpartyassist": c.PixelPartyAssistEnabled = !c.PixelPartyAssistEnabled; break;
            case "nametags":         c.NametagsEnabled = !c.NametagsEnabled; break;
            case "nickhider":        c.NickHiderEnabled = !c.NickHiderEnabled; break;
            case "closestplayer":    c.ClosestPlayerInfoEnabled = !c.ClosestPlayerInfoEnabled; break;
            case "fightstatus":      c.FightStatusEnabled = !c.FightStatusEnabled; break;
            case "chestesp":         c.ChestEspEnabled = !c.ChestEspEnabled; break;
            case "cheststealer":     c.ChestStealerEnabled = !c.ChestStealerEnabled; break;
            case "blockesp":         c.BlockEspEnabled = !c.BlockEspEnabled; break;
            case "reach":            c.ReachEnabled = !c.ReachEnabled; break;
            case "velocity":         c.VelocityEnabled = !c.VelocityEnabled; break;
            case "autototem":        c.AutoTotemEnabled = !c.AutoTotemEnabled; break;
            case "autorod":          c.AutoRodEnabled = !c.AutoRodEnabled; break;
            case "antidebuff":       c.AntiDebuffEnabled = !c.AntiDebuffEnabled; break;
            case "hitdelayfix":     c.HitDelayFixEnabled = !c.HitDelayFixEnabled; break;
            case "hudeditor":        c.HudEditorActive = !c.HudEditorActive; break;
        }
    }

    private static bool ShouldBlockModuleKeybinds()
    {
        if (!WindowDetection.IsMinecraftForeground())
            return true;

        if (GameStateClient.Instance.IsConnected)
            return IsAnyGameScreenOpen(GameStateClient.Instance.CurrentState);

        return WindowDetection.IsCursorVisible();
    }
    
    public static void Install()
    {
        _keyboardProc = KeyboardProc;
        _mouseProc = MouseProc;
        
        using var curProcess = Process.GetCurrentProcess();
        using var curModule = curProcess.MainModule;
        IntPtr moduleHandle = GetModuleHandle(curModule?.ModuleName);
        
        _keyboardHook = SetWindowsHookEx(WH_KEYBOARD_LL, _keyboardProc, moduleHandle, 0);
        _mouseHook = SetWindowsHookEx(WH_MOUSE_LL, _mouseProc, moduleHandle, 0);
    }
    
    public static void Uninstall()
    {
        AutoRodActionLatch.End();

        if (_keyboardHook != IntPtr.Zero)
        {
            UnhookWindowsHookEx(_keyboardHook);
            _keyboardHook = IntPtr.Zero;
        }
        
        if (_mouseHook != IntPtr.Zero)
        {
            UnhookWindowsHookEx(_mouseHook);
            _mouseHook = IntPtr.Zero;
        }
    }
    
    private static IntPtr KeyboardProc(int nCode, IntPtr wParam, IntPtr lParam)
    {
        bool isDown = wParam == (IntPtr)WM_KEYDOWN;
        bool isUp = wParam == (IntPtr)WM_KEYUP;
        if (nCode >= 0 && (isDown || isUp))
        {
            var kb = Marshal.PtrToStructure<KBDLLHOOKSTRUCT>(lParam);
            int vkCode = (int)kb.VkCode;

            if (isDown && IsCapturingKey)
            {
                StopKeyCapture();
                Application.Current?.Dispatcher.BeginInvoke(() => OnKeyCaptured?.Invoke(vkCode));
                return (IntPtr)1;
            }

            if (vkCode == AutoRodActionKey && AutoRodActionKey > 0)
            {
                if (isDown)
                {
                    bool consume = AutoRodActionLatch.Begin(CanConsumeAutoRodAction(), out bool trigger);
                    if (trigger)
                        Application.Current?.Dispatcher.BeginInvoke(() => _ = GameStateClient.Instance.SendAutoRodActionAsync());
                    if (consume) return (IntPtr)1;
                }
                else if (AutoRodActionLatch.End())
                {
                    Application.Current?.Dispatcher.BeginInvoke(
                        () => _ = GameStateClient.Instance.SendAutoRodReleaseAsync());
                    return (IntPtr)1;
                }
            }

            if (isDown)
            {
                if (ShouldBlockModuleKeybinds())
                    return CallNextHookEx(_keyboardHook, nCode, wParam, lParam);

                foreach (var kvp in ModuleKeys)
                {
                    if (kvp.Value > 0 && kb.VkCode == (uint)kvp.Value)
                    {
                        string id = kvp.Key;
                        Application.Current?.Dispatcher.BeginInvoke(() =>
                        {
                            ToggleModule(id);
                            OnToggleRequested?.Invoke();
                            OnStateChanged?.Invoke();
                        });
                        return (IntPtr)1;
                    }
                }
            }
        }

        return CallNextHookEx(_keyboardHook, nCode, wParam, lParam);
    }
    
    private static bool TryGetMouseBinding(int message, uint mouseData, out int vkCode, out bool isDown)
    {
        vkCode = 0;
        isDown = false;
        switch (message)
        {
            case WM_LBUTTONDOWN: vkCode = VK_LBUTTON; isDown = true; return true;
            case WM_LBUTTONUP: vkCode = VK_LBUTTON; return true;
            case WM_RBUTTONDOWN: vkCode = VK_RBUTTON; isDown = true; return true;
            case WM_RBUTTONUP: vkCode = VK_RBUTTON; return true;
            case WM_MBUTTONDOWN: vkCode = VK_MBUTTON; isDown = true; return true;
            case WM_MBUTTONUP: vkCode = VK_MBUTTON; return true;
            case WM_XBUTTONDOWN:
            case WM_XBUTTONUP:
                vkCode = ((mouseData >> 16) & 0xFFFF) == 1 ? VK_XBUTTON1 : VK_XBUTTON2;
                isDown = message == WM_XBUTTONDOWN;
                return true;
            default:
                return false;
        }
    }

    private static IntPtr MouseProc(int nCode, IntPtr wParam, IntPtr lParam)
    {
        if (nCode >= 0)
        {
            var ms = Marshal.PtrToStructure<MSLLHOOKSTRUCT>(lParam);
            
            // Ignore injected events (our own clicks)
            if ((ms.Flags & LLMHF_INJECTED) != 0)
            {
                return CallNextHookEx(_mouseHook, nCode, wParam, lParam);
            }
            
            int msg = wParam.ToInt32();
            if (TryGetMouseBinding(msg, ms.MouseData, out int mouseVk, out bool mouseDown))
            {
                if (mouseDown && IsCapturingKey && _captureAllowsMouse)
                {
                    StopKeyCapture();
                    Application.Current?.Dispatcher.BeginInvoke(() => OnKeyCaptured?.Invoke(mouseVk));
                    return (IntPtr)1;
                }

                if (mouseVk == AutoRodActionKey && AutoRodActionKey > 0)
                {
                    if (mouseDown)
                    {
                        bool consume = AutoRodActionLatch.Begin(CanConsumeAutoRodAction(), out bool trigger);
                        if (trigger)
                            Application.Current?.Dispatcher.BeginInvoke(() => _ = GameStateClient.Instance.SendAutoRodActionAsync());
                        if (consume) return (IntPtr)1;
                    }
                    else if (AutoRodActionLatch.End())
                    {
                        Application.Current?.Dispatcher.BeginInvoke(
                            () => _ = GameStateClient.Instance.SendAutoRodReleaseAsync());
                        return (IntPtr)1;
                    }
                }
            }

            if (Clicker.Instance.IsArmed && Clicker.Instance.KillAuraEnabled &&
                GameStateClient.Instance.IsConnected &&
                (GameStateClient.Instance.CurrentState.KillAuraHasTarget ||
                 GameStateClient.Instance.CurrentState.KillAuraBlocking) &&
                (msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP ||
                 msg == WM_RBUTTONDOWN || msg == WM_RBUTTONUP))
            {
                return (IntPtr)1;
            }

            if (msg == WM_LBUTTONDOWN) IsPhysicalLeftButtonDown = true;
            else if (msg == WM_LBUTTONUP) IsPhysicalLeftButtonDown = false;

            bool isLeftClickMessage = msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP;
            if (isLeftClickMessage && !Clicker.Instance.IsArmed)
                return CallNextHookEx(_mouseHook, nCode, wParam, lParam);
            
            if (msg == WM_LBUTTONDOWN)
            {
                Application.Current?.Dispatcher.BeginInvoke(() =>
                {
                    if (GameStateClient.Instance.IsConnected)
                    {
                        var state = GameStateClient.Instance.CurrentState;
                        bool chestGuiOpen =
                            state.GuiOpen &&
                            (state.ScreenName.Contains("GuiChest", StringComparison.OrdinalIgnoreCase) ||
                             state.ScreenName.Contains("ContainerScreen", StringComparison.OrdinalIgnoreCase) ||
                             state.ScreenName.Contains("class_481", StringComparison.OrdinalIgnoreCase) ||
                             state.ScreenName.Contains("GuiContainer", StringComparison.OrdinalIgnoreCase) ||
                             state.ScreenName.Contains("HopperScreen", StringComparison.OrdinalIgnoreCase) ||
                             state.ScreenName.Contains("class_488", StringComparison.OrdinalIgnoreCase) ||
                             state.ScreenName.Contains("ShulkerBox", StringComparison.OrdinalIgnoreCase) ||
                             state.ScreenName.Contains("class_495", StringComparison.OrdinalIgnoreCase) ||
                             state.ScreenName.Contains("HandledScreen", StringComparison.OrdinalIgnoreCase) ||
                             state.ScreenName.Contains("class_465", StringComparison.OrdinalIgnoreCase));

                        // In chest/container screens, a left click should not mark mining intent.
                        if (chestGuiOpen && Clicker.Instance.ClickInChests)
                            Clicker.Instance.IsMiningIntent = false;
                        else
                            Clicker.Instance.IsMiningIntent = state.LookingAtBlock;
                    }
                    else
                    {
                        Clicker.Instance.IsMiningIntent = false;
                    }
                    Clicker.Instance.StartClicking(true);
                    OnStateChanged?.Invoke();
                });
            }
            else if (msg == WM_LBUTTONUP)
            {
                if (Clicker.Instance.IsClicking && Clicker.Instance.IsUsingLeftButton)
                {
                    Application.Current?.Dispatcher.BeginInvoke(() =>
                    {
                        Clicker.Instance.StopClicking();
                        OnStateChanged?.Invoke();
                    });
                }
            }
            else if (msg == WM_RBUTTONDOWN)
            {
                Application.Current?.Dispatcher.BeginInvoke(() =>
                {
                    // Check if right-click-only-block is enabled
                    if (Clicker.Instance.RightClickOnlyBlock)
                    {
                        // Fail-open when state is unavailable; only block if connected and confirmed not holding a block.
                        if (GameStateClient.Instance.IsConnected && !GameStateClient.Instance.CurrentState.HoldingBlock)
                        {
                            // Don't start clicking - player isn't holding a block
                            return;
                        }
                    }
                    
                    if (Clicker.Instance.IsClicking && Clicker.Instance.IsUsingLeftButton)
                    {
                        // Keep left autoclick stream alive for blockhit sequences.
                        return;
                    }

                    Clicker.Instance.StartClicking(false);
                    OnStateChanged?.Invoke();
                });
            }
            else if (msg == WM_RBUTTONUP)
            {
                if (Clicker.Instance.IsClicking && !Clicker.Instance.IsUsingLeftButton)
                {
                    Application.Current?.Dispatcher.BeginInvoke(() =>
                    {
                        Clicker.Instance.StopClicking();
                        OnStateChanged?.Invoke();
                    });
                }
            }
        }
        
        return CallNextHookEx(_mouseHook, nCode, wParam, lParam);
    }
}
