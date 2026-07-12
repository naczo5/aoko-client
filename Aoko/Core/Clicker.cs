using System;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using System.Diagnostics;
using System.Linq;

namespace Aoko.Core;

public class Clicker : INotifyPropertyChanged
{
    private const int AimAssistStateFreshMs = 220;
    private const int TriggerbotStateFreshMs = 35;
    private static Clicker? _instance;
    public static Clicker Instance => _instance ??= new Clicker();

    // P/Invoke declarations
    [DllImport("user32.dll", SetLastError = true)]
    private static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
    [DllImport("user32.dll", SetLastError = true, EntryPoint = "SendInput")]
    private static extern uint SendInputKeyboard(uint nInputs, INPUT_KEY[] pInputs, int cbSize);
    [DllImport("user32.dll")]
    private static extern short GetAsyncKeyState(int vKey);

    [StructLayout(LayoutKind.Sequential)]
    private struct INPUT
    {
        public uint Type;
        public MOUSEINPUT Mi;
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

    [StructLayout(LayoutKind.Explicit)]
    private struct INPUTUNION
    {
        [FieldOffset(0)] public MOUSEINPUT Mi;
        [FieldOffset(0)] public KEYBDINPUT Ki;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct INPUT_KEY
    {
        public uint Type;
        public INPUTUNION U;
    }

    private const uint INPUT_MOUSE = 0;
    private const uint MOUSEEVENTF_MOVE = 0x0001;
    private const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
    private const uint MOUSEEVENTF_LEFTUP = 0x0004;
    private const uint MOUSEEVENTF_RIGHTDOWN = 0x0008;
    private const uint MOUSEEVENTF_RIGHTUP = 0x0010;
    private const int VK_LBUTTON = 0x01;
    private const int VK_W = 0x57;
    private const int VK_A = 0x41;
    private const int VK_D = 0x44;
    private const int VK_SPACE = 0x20;
    private const int PixelPartyJumpPulseHalfMs = 55;
    private const float PixelPartyWalkAlignMaxDeg = 18f;
    private const float PixelPartyPrecisionAlignMaxDeg = 55f;
    private const float PixelPartyPrecisionDist = 4f;
    private const float PixelPartyJumpMinDist = 5f;
    private const float PixelPartyMinWalkDist = 0.45f;
    // Coast on the last known target for a short window when the target momentarily
    // disappears (e.g. mid-jump) so we don't release keys/steering and stutter.
    private const int PixelPartyTargetGraceMs = 160;
    private const uint INPUT_KEYBOARD = 1;
    private const uint KEYEVENTF_KEYUP = 0x0002;
    
    // State
    private bool _isArmed;
    private bool _isClicking;
    private bool _useLeftButton = true;
    private CancellationTokenSource? _clickCts;
    private CancellationTokenSource? _aimAssistCts;
    private CancellationTokenSource? _triggerbotCts;
    private Task? _clickTask;
    private Task? _aimAssistTask;
    private Task? _triggerbotTask;
    private double _aimAssistFilteredDx;
    private double _aimAssistFilteredDy;
    private int _panicInProgress;
    
    // Settings
    private float _minCPS = 8.0f;
    private float _maxCPS = 12.0f;
    private bool _leftClickEnabled = false;
    private bool _rightClickEnabled = false;
    private bool _jitterEnabled = false;

    
    // Right Click Settings
    private float _rightMinCPS = 10.0f;
    private float _rightMaxCPS = 14.0f;
    private bool _rightClickOnlyBlock = false;
    private bool _breakBlocksEnabled = false;
    private bool _isMiningIntent = false;
    
    private readonly Random _random = new();
    private readonly object _sendInputLock = new();
    private readonly ChestStealerController _chestStealerController = new();
    private readonly INPUT[] _leftClickInputs;
    private readonly INPUT[] _rightClickInputs;
    private readonly INPUT[] _aimAssistMoveInput;
    
    public event PropertyChangedEventHandler? PropertyChanged;
    public event Action? StateChanged;
    
    private Clicker()
    {
        _leftClickInputs = new INPUT[2];
        _leftClickInputs[0].Type = INPUT_MOUSE;
        _leftClickInputs[0].Mi.DwFlags = MOUSEEVENTF_LEFTDOWN;
        _leftClickInputs[1].Type = INPUT_MOUSE;
        _leftClickInputs[1].Mi.DwFlags = MOUSEEVENTF_LEFTUP;

        _rightClickInputs = new INPUT[2];
        _rightClickInputs[0].Type = INPUT_MOUSE;
        _rightClickInputs[0].Mi.DwFlags = MOUSEEVENTF_RIGHTDOWN;
        _rightClickInputs[1].Type = INPUT_MOUSE;
        _rightClickInputs[1].Mi.DwFlags = MOUSEEVENTF_RIGHTUP;

        _aimAssistMoveInput = new INPUT[1];
        _aimAssistMoveInput[0].Type = INPUT_MOUSE;
        _aimAssistMoveInput[0].Mi.DwFlags = MOUSEEVENTF_MOVE;

        AttachBlockEspTargets(_blockEspTargets);
    }
    
    
    private string _guiTheme = "Slate";
    public string GuiTheme { get => _guiTheme; set { _guiTheme = value; OnPropertyChanged(nameof(GuiTheme)); } }

    private string _moduleListStyle = "Default";
    public string ModuleListStyle { get => _moduleListStyle; set { _moduleListStyle = value; OnPropertyChanged(nameof(ModuleListStyle)); } }

    private bool _showLogo = true;
    public bool ShowLogo { get => _showLogo; set { _showLogo = value; OnPropertyChanged(nameof(ShowLogo)); } }

    private bool _discordRpcEnabled = true;
    public bool DiscordRpcEnabled
    {
        get => _discordRpcEnabled;
        set
        {
            if (_discordRpcEnabled != value)
            {
                _discordRpcEnabled = value;
                OnPropertyChanged(nameof(DiscordRpcEnabled));
                OnPropertyChanged(nameof(DiscordRpcStatusText));
                StateChanged?.Invoke();
            }
        }
    }

    private string _discordRpcStatusText = "Ready";
    public string DiscordRpcStatusText
    {
        get => _discordRpcStatusText;
        internal set
        {
            if (_discordRpcStatusText != value)
            {
                _discordRpcStatusText = value;
                OnPropertyChanged(nameof(DiscordRpcStatusText));
            }
        }
    }

    public bool IsArmed
    {
        get => _isArmed;
        private set
        {
            if (_isArmed != value)
            {
                _isArmed = value;
                OnPropertyChanged(nameof(IsArmed));
                OnPropertyChanged(nameof(StatusText));
                StateChanged?.Invoke();
            }
        }
    }
    
    public bool IsClicking
    {
        get => _isClicking;
        private set
        {
            if (_isClicking != value)
            {
                _isClicking = value;
                OnPropertyChanged(nameof(IsClicking));
                OnPropertyChanged(nameof(StatusText));
                StateChanged?.Invoke();
            }
        }
    }
    
    public float MinCPS
    {
        get => _minCPS;
        set
        {
            if (value > 0 && value <= 25)
            {
                _minCPS = value;
                OnPropertyChanged(nameof(MinCPS));
                StateChanged?.Invoke();
            }
        }
    }
    
    public float MaxCPS
    {
        get => _maxCPS;
        set
        {
            if (value > 0 && value <= 25)
            {
                _maxCPS = value;
                OnPropertyChanged(nameof(MaxCPS));
                StateChanged?.Invoke();
            }
        }
    }
    
    public bool LeftClickEnabled
    {
        get => _leftClickEnabled;
        set
        {
            _leftClickEnabled = value;
            OnPropertyChanged(nameof(LeftClickEnabled));
            StateChanged?.Invoke();
        }
    }
    
    public bool RightClickEnabled
    {
        get => _rightClickEnabled;
        set
        {
            _rightClickEnabled = value;
            OnPropertyChanged(nameof(RightClickEnabled));
            StateChanged?.Invoke();
        }
    }
    
    public bool JitterEnabled
    {
        get => _jitterEnabled;
        set
        {
            _jitterEnabled = value;
            OnPropertyChanged(nameof(JitterEnabled));
            StateChanged?.Invoke();
        }
    }
    
    public float RightMinCPS
    {
        get => _rightMinCPS;
        set
        {
            if (value > 0 && value <= 25)
            {
                _rightMinCPS = value;
                OnPropertyChanged(nameof(RightMinCPS));
                StateChanged?.Invoke();
            }
        }
    }

    public float RightMaxCPS
    {
        get => _rightMaxCPS;
        set
        {
            if (value > 0 && value <= 25)
            {
                _rightMaxCPS = value;
                OnPropertyChanged(nameof(RightMaxCPS));
                StateChanged?.Invoke();
            }
        }
    }

    public bool RightClickOnlyBlock
    {
        get => _rightClickOnlyBlock;
        set
        {
            _rightClickOnlyBlock = value;
            OnPropertyChanged(nameof(RightClickOnlyBlock));
            StateChanged?.Invoke();
        }
    }

    public bool BreakBlocksEnabled
    {
        get => _breakBlocksEnabled;
        set
        {
            _breakBlocksEnabled = value;
            OnPropertyChanged(nameof(BreakBlocksEnabled));
            StateChanged?.Invoke();
        }
    }
    
    public bool IsMiningIntent
    {
        get => _isMiningIntent;
        set
        {
            _isMiningIntent = value;
            OnPropertyChanged(nameof(IsMiningIntent));
            StateChanged?.Invoke();
        }
    }
    
    public string StatusText
    {
        get
        {
            if (IsClicking) return "Clicking";
            if (IsArmed) return "Armed";
            return "Disabled";
        }
    }
    
    public void ToggleArmed()
    {
        if (IsArmed)
            Disarm();
        else
            Arm();
    }
    
    public void Arm()
    {
        IsArmed = true;
    }
    
    public void Disarm()
    {
        IsArmed = false;
        StopClicking();
    }
    
    public void StartClicking(bool leftButton)
    {
        if (leftButton)
        {
            if (!IsArmed || !LeftClickEnabled) return;
        }
        else
        {
            // Right-click autoclick is intentionally independent from "Armed"
            // so it can work when left autoclicker is disabled.
            if (!RightClickEnabled) return;
        }
        if (IsClicking) return;
        
        _useLeftButton = leftButton;
        IsClicking = true;
        
        var cts = new CancellationTokenSource();
        _clickCts = cts;
        _clickTask = Task.Run(() => ClickLoop(cts.Token));
    }
    
    public void StopClicking()
    {
        var cts = _clickCts;
        var task = _clickTask;
        _clickCts = null;
        _clickTask = null;
        if (cts != null)
        {
            cts.Cancel();
            _ = DisposeCtsWhenDoneAsync(cts, task);
        }
        IsClicking = false;
    }
    
    public void Stop()
    {
        StopClicking();
        Disarm();
        StopAimAssistLoop();
        StopTriggerbotLoop();
        StopPixelPartyAssistInputLoop();
        _chestStealerController.Stop();
    }

    public void TriggerPanic()
    {
        _ = TriggerPanicAsync();
    }

    public async Task TriggerPanicAsync()
    {
        if (System.Windows.Application.Current?.Dispatcher is { } dispatcher && !dispatcher.CheckAccess())
        {
            dispatcher.Invoke(TriggerPanic);
            return;
        }

        if (Interlocked.Exchange(ref _panicInProgress, 1) == 1)
            return;

        try
        {
            Disarm();
            LeftClickEnabled = true;
            RightClickEnabled = false;
            JitterEnabled = false;
            ClickInChests = false;
            BreakBlocksEnabled = false;
            IsMiningIntent = false;

            AimAssistEnabled = false;
            TriggerbotEnabled = false;
            SilentAuraEnabled = false;
            SpeedBridgeEnabled = false;
            GtbHelperEnabled = false;
            PixelPartyAssistEnabled = false;
            PixelPartyAutoLookEnabled = false;
            PixelPartyAutoWalkEnabled = false;
            NametagsEnabled = false;
            NametagHideVanilla = false;
            ClosestPlayerInfoEnabled = false;
            ChestEspEnabled = false;
            ChestStealerEnabled = false;
            BlockEspEnabled = false;
            ReachEnabled = false;
            VelocityEnabled = false;
            AutoTotemEnabled = false;
            AntiDebuffEnabled = false;
            HitDelayFixEnabled = false;

            ShowModuleList = false;
            ShowLogo = false;
            DiscordRpcEnabled = false;

            if (System.Windows.Application.Current?.MainWindow is Aoko.MainWindow mainWindow)
                mainWindow.EnterPanicStealthMode();

            if (GameStateClient.Instance.IsConnected)
                await Task.Delay(250).ConfigureAwait(false);

            GameStateClient.Instance.Disconnect();

            var app = System.Windows.Application.Current;
            if (app != null)
                app.Shutdown();
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[Panic] Unexpected panic error: {ex}");
        }
        finally
        {
            Interlocked.Exchange(ref _panicInProgress, 0);
        }
    }

    private void StartAimAssistLoop()
    {
        if (_aimAssistCts != null) return;
        var cts = new CancellationTokenSource();
        _aimAssistCts = cts;
        _aimAssistTask = Task.Run(() => AimAssistLoop(cts.Token));
    }

    private CancellationTokenSource? _pixelPartyAssistInputCts;
    private Task? _pixelPartyAssistInputTask;
    private bool _pixelPartyKeyW;
    private bool _pixelPartyKeyA;
    private bool _pixelPartyKeyD;
    private bool _pixelPartyKeySpace;
    private long _pixelPartyLastTargetMs;
    private float _pixelPartyLastYawDelta;
    private float _pixelPartyLastDist;
    private const int PixelPartyAssistInputStateFreshMs = 120;

    private bool ShouldRunPixelPartyAssistInputLoop()
        => PixelPartyAssistEnabled && (PixelPartyAutoLookEnabled || PixelPartyAutoWalkEnabled);

    private void SyncPixelPartyAssistInputLoop()
    {
        if (ShouldRunPixelPartyAssistInputLoop())
            StartPixelPartyAssistInputLoop();
        else
            StopPixelPartyAssistInputLoop();
    }

    private void StartPixelPartyAssistInputLoop()
    {
        if (_pixelPartyAssistInputCts != null) return;
        var cts = new CancellationTokenSource();
        _pixelPartyAssistInputCts = cts;
        _pixelPartyAssistInputTask = Task.Run(() => PixelPartyAssistInputLoop(cts.Token));
    }

    private void StopPixelPartyAssistInputLoop()
    {
        var cts = _pixelPartyAssistInputCts;
        _pixelPartyAssistInputCts = null;
        if (cts == null) return;
        cts.Cancel();
        cts.Dispose();
        ReleasePixelPartyWalkKeys();
    }

    private void SendPixelPartyKey(int vk, bool down)
    {
        var input = new INPUT_KEY[1];
        input[0].Type = INPUT_KEYBOARD;
        input[0].U.Ki.WVk = (ushort)vk;
        input[0].U.Ki.DwFlags = down ? 0u : KEYEVENTF_KEYUP;
        lock (_sendInputLock)
        {
            SendInputKeyboard(1, input, Marshal.SizeOf<INPUT_KEY>());
        }
    }

    private void SetPixelPartyWalkKey(int vk, bool down, ref bool tracked)
    {
        if (down == tracked) return;
        SendPixelPartyKey(vk, down);
        tracked = down;
    }

    private void ReleasePixelPartyWalkKeys()
    {
        SetPixelPartyWalkKey(VK_W, false, ref _pixelPartyKeyW);
        SetPixelPartyWalkKey(VK_A, false, ref _pixelPartyKeyA);
        SetPixelPartyWalkKey(VK_D, false, ref _pixelPartyKeyD);
        SetPixelPartyWalkKey(VK_SPACE, false, ref _pixelPartyKeySpace);
    }

    /// <summary>Yaw snap via mouse — multiple SendInput passes per tick for large deltas.</summary>
    private void ApplyPixelPartySteer(float yawDeltaDeg, bool aggressive)
    {
        float remaining = yawDeltaDeg;
        if (Math.Abs(remaining) < 0.35f)
            return;

        float gain = aggressive ? 6.5f : 5.0f;
        int maxStep = aggressive ? 120 : 80;
        int maxPasses = aggressive ? 10 : 7;

        for (int pass = 0; pass < maxPasses && Math.Abs(remaining) > 0.4f; pass++)
        {
            int moveX = (int)Math.Round(remaining * gain);
            if (moveX == 0)
                moveX = Math.Sign(remaining);

            moveX = Math.Clamp(moveX, -maxStep, maxStep);
            _aimAssistMoveInput[0].Mi.Dx = moveX;
            _aimAssistMoveInput[0].Mi.Dy = 0;
            lock (_sendInputLock)
            {
                SendInput(1, _aimAssistMoveInput, Marshal.SizeOf<INPUT>());
            }

            remaining -= moveX / gain;
        }
    }

    private void StopAimAssistLoop()
    {
        var cts = _aimAssistCts;
        var task = _aimAssistTask;
        _aimAssistCts = null;
        _aimAssistTask = null;
        _aimAssistFilteredDx = 0.0;
        _aimAssistFilteredDy = 0.0;
        if (cts != null)
        {
            cts.Cancel();
            _ = DisposeCtsWhenDoneAsync(cts, task);
        }
    }

    public bool IsUsingLeftButton => _useLeftButton;

    private void StartTriggerbotLoop()
    {
        if (_triggerbotCts != null) return;
        var cts = new CancellationTokenSource();
        _triggerbotCts = cts;
        _triggerbotTask = Task.Run(() => TriggerbotLoop(cts.Token));
    }

    private void StopTriggerbotLoop()
    {
        var cts = _triggerbotCts;
        var task = _triggerbotTask;
        _triggerbotCts = null;
        _triggerbotTask = null;
        if (cts != null)
        {
            cts.Cancel();
            _ = DisposeCtsWhenDoneAsync(cts, task);
        }
    }

    private static async Task DisposeCtsWhenDoneAsync(CancellationTokenSource cts, Task? task)
    {
        if (task != null)
        {
            try
            {
                await task.ConfigureAwait(false);
            }
            catch
            {
            }
        }

        cts.Dispose();
    }
    
    private bool _clickInChests = false;
    private bool _aimAssistEnabled = false;
    private float _aimAssistFov = 30.0f;
    private float _aimAssistRange = 4.5f;
    private int _aimAssistStrength = 40;
    private bool _gtbHelperEnabled = false;
    private string _gtbCurrentHint = "-";
    private int _gtbMatchCount = 0;
    private string _gtbMatchesPreview = "-";

    public bool ClickInChests
    {
        get => _clickInChests;
        set
        {
            _clickInChests = value;
            OnPropertyChanged(nameof(ClickInChests));
            StateChanged?.Invoke();
        }
    }

    public bool AimAssistEnabled
    {
        get => _aimAssistEnabled;
        set
        {
            _aimAssistEnabled = value;
            OnPropertyChanged(nameof(AimAssistEnabled));
            if (_aimAssistEnabled) StartAimAssistLoop();
            else StopAimAssistLoop();
            StateChanged?.Invoke();
        }
    }

    public float AimAssistFov
    {
        get => _aimAssistFov;
        set
        {
            float clamped = Math.Clamp(value, 1.0f, 180.0f);
            if (Math.Abs(_aimAssistFov - clamped) > float.Epsilon)
            {
                _aimAssistFov = clamped;
                OnPropertyChanged(nameof(AimAssistFov));
                StateChanged?.Invoke();
            }
        }
    }

    public float AimAssistRange
    {
        get => _aimAssistRange;
        set
        {
            float clamped = Math.Clamp(value, 1.0f, 12.0f);
            if (Math.Abs(_aimAssistRange - clamped) > float.Epsilon)
            {
                _aimAssistRange = clamped;
                OnPropertyChanged(nameof(AimAssistRange));
                StateChanged?.Invoke();
            }
        }
    }

    public int AimAssistStrength
    {
        get => _aimAssistStrength;
        set
        {
            int clamped = Math.Clamp(value, 1, 100);
            if (_aimAssistStrength != clamped)
            {
                _aimAssistStrength = clamped;
                OnPropertyChanged(nameof(AimAssistStrength));
                StateChanged?.Invoke();
            }
        }
    }

    public bool GtbHelperEnabled
    {
        get => _gtbHelperEnabled;
        set
        {
            _gtbHelperEnabled = value;
            OnPropertyChanged(nameof(GtbHelperEnabled));
            if (!value)
            {
                SetGtbState("", 0, "");
            }
            StateChanged?.Invoke();
        }
    }

    private bool _pixelPartyAssistEnabled = false;
    public bool PixelPartyAssistEnabled
    {
        get => _pixelPartyAssistEnabled;
        set
        {
            if (_pixelPartyAssistEnabled == value) return;
            _pixelPartyAssistEnabled = value;
            OnPropertyChanged(nameof(PixelPartyAssistEnabled));
            SyncPixelPartyAssistInputLoop();
            StateChanged?.Invoke();
        }
    }

    private int _pixelPartyScanRadius = 28;
    public int PixelPartyScanRadius
    {
        get => _pixelPartyScanRadius;
        set
        {
            int clamped = Math.Clamp(value, 8, 48);
            if (_pixelPartyScanRadius == clamped) return;
            _pixelPartyScanRadius = clamped;
            OnPropertyChanged(nameof(PixelPartyScanRadius));
            StateChanged?.Invoke();
        }
    }

    private bool _pixelPartyAutoLookEnabled = false;
    public bool PixelPartyAutoLookEnabled
    {
        get => _pixelPartyAutoLookEnabled;
        set
        {
            if (_pixelPartyAutoLookEnabled == value) return;
            _pixelPartyAutoLookEnabled = value;
            OnPropertyChanged(nameof(PixelPartyAutoLookEnabled));
            SyncPixelPartyAssistInputLoop();
            StateChanged?.Invoke();
        }
    }

    private bool _pixelPartyAutoWalkEnabled = false;
    public bool PixelPartyAutoWalkEnabled
    {
        get => _pixelPartyAutoWalkEnabled;
        set
        {
            if (_pixelPartyAutoWalkEnabled == value) return;
            _pixelPartyAutoWalkEnabled = value;
            OnPropertyChanged(nameof(PixelPartyAutoWalkEnabled));
            SyncPixelPartyAssistInputLoop();
            StateChanged?.Invoke();
        }
    }

    public string GtbCurrentHint
    {
        get => _gtbCurrentHint;
        private set
        {
            if (_gtbCurrentHint != value)
            {
                _gtbCurrentHint = value;
                OnPropertyChanged(nameof(GtbCurrentHint));
            }
        }
    }

    public int GtbMatchCount
    {
        get => _gtbMatchCount;
        private set
        {
            if (_gtbMatchCount != value)
            {
                _gtbMatchCount = value;
                OnPropertyChanged(nameof(GtbMatchCount));
            }
        }
    }

    public string GtbMatchesPreview
    {
        get => _gtbMatchesPreview;
        private set
        {
            if (_gtbMatchesPreview != value)
            {
                _gtbMatchesPreview = value;
                OnPropertyChanged(nameof(GtbMatchesPreview));
            }
        }
    }

    private bool _nametagsEnabled = false;
    public bool NametagsEnabled
    {
        get => _nametagsEnabled;
        set
        {
            _nametagsEnabled = value;
            OnPropertyChanged(nameof(NametagsEnabled));
            StateChanged?.Invoke();
        }
    }

    private bool _nametagShowHealth = true;
    public bool NametagShowHealth
    {
        get => _nametagShowHealth;
        set
        {
            _nametagShowHealth = value;
            OnPropertyChanged(nameof(NametagShowHealth));
            StateChanged?.Invoke();
        }
    }

    private bool _nametagShowArmor = true;
    public bool NametagShowArmor
    {
        get => _nametagShowArmor;
        set
        {
            _nametagShowArmor = value;
            OnPropertyChanged(nameof(NametagShowArmor));
            StateChanged?.Invoke();
        }
    }

    private bool _nametagShowHeldItem = true;
    public bool NametagShowHeldItem
    {
        get => _nametagShowHeldItem;
        set
        {
            _nametagShowHeldItem = value;
            OnPropertyChanged(nameof(NametagShowHeldItem));
            StateChanged?.Invoke();
        }
    }

    private bool _nametagHideVanilla = false;
    public bool NametagHideVanilla
    {
        get => _nametagHideVanilla;
        set
        {
            _nametagHideVanilla = value;
            OnPropertyChanged(nameof(NametagHideVanilla));
            StateChanged?.Invoke();
        }
    }

    private int _nametagMaxCount = 8;
    public int NametagMaxCount
    {
        get => _nametagMaxCount;
        set
        {
            int clamped = Math.Clamp(value, 1, 20);
            if (_nametagMaxCount != clamped)
            {
                _nametagMaxCount = clamped;
                OnPropertyChanged(nameof(NametagMaxCount));
                StateChanged?.Invoke();
            }
        }
    }

    private bool _chestEspEnabled = false;
    public bool ChestEspEnabled
    {
        get => _chestEspEnabled;
        set
        {
            _chestEspEnabled = value;
            OnPropertyChanged(nameof(ChestEspEnabled));
            StateChanged?.Invoke();
        }
    }

    private int _chestEspMaxCount = 5;
    public int ChestEspMaxCount
    {
        get => _chestEspMaxCount;
        set
        {
            int clamped = Math.Clamp(value, 1, 20);
            if (_chestEspMaxCount != clamped)
            {
                _chestEspMaxCount = clamped;
                OnPropertyChanged(nameof(ChestEspMaxCount));
                StateChanged?.Invoke();
            }
        }
    }

    private bool _chestStealerEnabled = false;
    public bool ChestStealerEnabled
    {
        get => _chestStealerEnabled;
        set
        {
            if (_chestStealerEnabled == value) return;
            _chestStealerEnabled = value;
            if (value)
                _chestStealerController.Start();
            else
                _chestStealerController.Stop();
            OnPropertyChanged(nameof(ChestStealerEnabled));
            StateChanged?.Invoke();
        }
    }

    private int _chestStealerDelayMs = 120;
    public int ChestStealerDelayMs
    {
        get => _chestStealerDelayMs;
        set
        {
            int clamped = Math.Clamp(value, 50, 500);
            if (_chestStealerDelayMs != clamped)
            {
                _chestStealerDelayMs = clamped;
                OnPropertyChanged(nameof(ChestStealerDelayMs));
                StateChanged?.Invoke();
            }
        }
    }

    // === Block ESP / X-ray ===

    private bool _blockEspEnabled = false;
    public bool BlockEspEnabled
    {
        get => _blockEspEnabled;
        set
        {
            if (_blockEspEnabled == value) return;
            _blockEspEnabled = value;
            OnPropertyChanged(nameof(BlockEspEnabled));
            StateChanged?.Invoke();
        }
    }

    private bool _blockEspBoxes = true;
    public bool BlockEspBoxes
    {
        get => _blockEspBoxes;
        set
        {
            if (_blockEspBoxes == value) return;
            _blockEspBoxes = value;
            OnPropertyChanged(nameof(BlockEspBoxes));
            StateChanged?.Invoke();
        }
    }

    private bool _blockEspTracers = false;
    public bool BlockEspTracers
    {
        get => _blockEspTracers;
        set
        {
            if (_blockEspTracers == value) return;
            _blockEspTracers = value;
            OnPropertyChanged(nameof(BlockEspTracers));
            StateChanged?.Invoke();
        }
    }

    private bool _blockEspHud = true;
    public bool BlockEspHud
    {
        get => _blockEspHud;
        set
        {
            if (_blockEspHud == value) return;
            _blockEspHud = value;
            OnPropertyChanged(nameof(BlockEspHud));
            StateChanged?.Invoke();
        }
    }

    private int _blockEspMaxCount = 64;
    public int BlockEspMaxCount
    {
        get => _blockEspMaxCount;
        set
        {
            int clamped = Math.Clamp(value, 1, 512);
            if (_blockEspMaxCount != clamped)
            {
                _blockEspMaxCount = clamped;
                OnPropertyChanged(nameof(BlockEspMaxCount));
                StateChanged?.Invoke();
            }
        }
    }

    private int _blockEspRange = 4;
    public int BlockEspRange
    {
        get => _blockEspRange;
        set
        {
            int clamped = Math.Clamp(value, 1, 8);
            if (_blockEspRange != clamped)
            {
                _blockEspRange = clamped;
                OnPropertyChanged(nameof(BlockEspRange));
                StateChanged?.Invoke();
            }
        }
    }

    private ObservableCollection<BlockEspTarget> _blockEspTargets = CreateDefaultBlockEspTargets();
    public ObservableCollection<BlockEspTarget> BlockEspTargets
    {
        get => _blockEspTargets;
        set
        {
            DetachBlockEspTargets(_blockEspTargets);
            _blockEspTargets = value ?? new ObservableCollection<BlockEspTarget>();
            AttachBlockEspTargets(_blockEspTargets);
            OnPropertyChanged(nameof(BlockEspTargets));
            StateChanged?.Invoke();
        }
    }

    private static ObservableCollection<BlockEspTarget> CreateDefaultBlockEspTargets()
    {
        var collection = new ObservableCollection<BlockEspTarget>(BlockEspPresets.BuildDefaultTargets());
        return collection;
    }

    /// <summary>Subscribes to collection + per-item changes so edits re-push config and persist.</summary>
    private void AttachBlockEspTargets(ObservableCollection<BlockEspTarget> targets)
    {
        targets.CollectionChanged += OnBlockEspTargetsChanged;
        foreach (BlockEspTarget t in targets)
            t.PropertyChanged += OnBlockEspTargetItemChanged;
    }

    private void DetachBlockEspTargets(ObservableCollection<BlockEspTarget> targets)
    {
        targets.CollectionChanged -= OnBlockEspTargetsChanged;
        foreach (BlockEspTarget t in targets)
            t.PropertyChanged -= OnBlockEspTargetItemChanged;
    }

    private void OnBlockEspTargetsChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        if (e.OldItems != null)
            foreach (BlockEspTarget t in e.OldItems)
                t.PropertyChanged -= OnBlockEspTargetItemChanged;
        if (e.NewItems != null)
            foreach (BlockEspTarget t in e.NewItems)
                t.PropertyChanged += OnBlockEspTargetItemChanged;
        OnPropertyChanged(nameof(BlockEspTargets));
        StateChanged?.Invoke();
    }

    private void OnBlockEspTargetItemChanged(object? sender, PropertyChangedEventArgs e)
        => StateChanged?.Invoke();

    /// <summary>Encodes the enabled targets as the delimited wire string sent to the bridge.</summary>
    public string BlockEspBlocksSerialized => BlockEspConfig.Serialize(_blockEspTargets);

    private bool _showModuleList = true;
    public bool ShowModuleList
    {
        get => _showModuleList;
        set
        {
            _showModuleList = value;
            OnPropertyChanged(nameof(ShowModuleList));
            StateChanged?.Invoke();
        }
    }

    private bool _reachEnabled = false;
    public bool ReachEnabled
    {
        get => _reachEnabled;
        set
        {
            _reachEnabled = value;
            OnPropertyChanged(nameof(ReachEnabled));
            StateChanged?.Invoke();
        }
    }

    private float _reachMin = 3.0f;
    public float ReachMin
    {
        get => _reachMin;
        set
        {
            float clamped = Math.Clamp(value, 3.0f, 6.0f);
            if (Math.Abs(_reachMin - clamped) > float.Epsilon)
            {
                _reachMin = clamped;
                OnPropertyChanged(nameof(ReachMin));
                StateChanged?.Invoke();
            }
        }
    }

    private float _reachMax = 3.0f;
    public float ReachMax
    {
        get => _reachMax;
        set
        {
            float clamped = Math.Clamp(value, 3.0f, 6.0f);
            if (Math.Abs(_reachMax - clamped) > float.Epsilon)
            {
                _reachMax = clamped;
                OnPropertyChanged(nameof(ReachMax));
                StateChanged?.Invoke();
            }
        }
    }

    private int _reachChance = 100;
    public int ReachChance
    {
        get => _reachChance;
        set
        {
            int clamped = Math.Clamp(value, 0, 100);
            if (_reachChance != clamped)
            {
                _reachChance = clamped;
                OnPropertyChanged(nameof(ReachChance));
                StateChanged?.Invoke();
            }
        }
    }

    private bool _velocityEnabled = false;
    public bool VelocityEnabled
    {
        get => _velocityEnabled;
        set
        {
            _velocityEnabled = value;
            OnPropertyChanged(nameof(VelocityEnabled));
            StateChanged?.Invoke();
        }
    }

    private int _velocityHorizontal = 100;
    public int VelocityHorizontal
    {
        get => _velocityHorizontal;
        set
        {
            int clamped = Math.Clamp(value, 1, 100);
            if (_velocityHorizontal != clamped)
            {
                _velocityHorizontal = clamped;
                OnPropertyChanged(nameof(VelocityHorizontal));
                StateChanged?.Invoke();
            }
        }
    }

    private int _velocityVertical = 100;
    public int VelocityVertical
    {
        get => _velocityVertical;
        set
        {
            int clamped = Math.Clamp(value, 1, 100);
            if (_velocityVertical != clamped)
            {
                _velocityVertical = clamped;
                OnPropertyChanged(nameof(VelocityVertical));
                StateChanged?.Invoke();
            }
        }
    }

    private int _velocityChance = 100;
    public int VelocityChance
    {
        get => _velocityChance;
        set
        {
            int clamped = Math.Clamp(value, 1, 100);
            if (_velocityChance != clamped)
            {
                _velocityChance = clamped;
                OnPropertyChanged(nameof(VelocityChance));
                StateChanged?.Invoke();
            }
        }
    }

    private bool _autoTotemEnabled = false;
    public bool AutoTotemEnabled
    {
        get => _autoTotemEnabled;
        set
        {
            _autoTotemEnabled = value;
            OnPropertyChanged(nameof(AutoTotemEnabled));
            StateChanged?.Invoke();
        }
    }

    private int _autoTotemMode = 0;
    public int AutoTotemMode
    {
        get => _autoTotemMode;
        set
        {
            int clamped = Math.Clamp(value, 0, 1);
            if (_autoTotemMode != clamped)
            {
                _autoTotemMode = clamped;
                OnPropertyChanged(nameof(AutoTotemMode));
                StateChanged?.Invoke();
            }
        }
    }

    private int _autoTotemHealth = 10;
    public int AutoTotemHealth
    {
        get => _autoTotemHealth;
        set
        {
            int clamped = Math.Clamp(value, 0, 36);
            if (_autoTotemHealth != clamped)
            {
                _autoTotemHealth = clamped;
                OnPropertyChanged(nameof(AutoTotemHealth));
                StateChanged?.Invoke();
            }
        }
    }

    private bool _autoTotemElytra = true;
    public bool AutoTotemElytra
    {
        get => _autoTotemElytra;
        set
        {
            _autoTotemElytra = value;
            OnPropertyChanged(nameof(AutoTotemElytra));
            StateChanged?.Invoke();
        }
    }

    private int _autoTotemDelay = 0;
    public int AutoTotemDelay
    {
        get => _autoTotemDelay;
        set
        {
            int clamped = Math.Clamp(value, 0, 20);
            if (_autoTotemDelay != clamped)
            {
                _autoTotemDelay = clamped;
                OnPropertyChanged(nameof(AutoTotemDelay));
                StateChanged?.Invoke();
            }
        }
    }

    private int _autoTotemBehaviorMode = 0; // 0 = Ghost (inventory only), 1 = Anarchy
    public int AutoTotemBehaviorMode
    {
        get => _autoTotemBehaviorMode;
        set
        {
            int clamped = Math.Clamp(value, 0, 1);
            if (_autoTotemBehaviorMode != clamped)
            {
                _autoTotemBehaviorMode = clamped;
                OnPropertyChanged(nameof(AutoTotemBehaviorMode));
                StateChanged?.Invoke();
            }
        }
    }

    private bool _antiDebuffEnabled = false;
    public bool AntiDebuffEnabled
    {
        get => _antiDebuffEnabled;
        set
        {
            _antiDebuffEnabled = value;
            OnPropertyChanged(nameof(AntiDebuffEnabled));
            StateChanged?.Invoke();
        }
    }

    private bool _hitDelayFixEnabled = false;
    public bool HitDelayFixEnabled
    {
        get => _hitDelayFixEnabled;
        set
        {
            _hitDelayFixEnabled = value;
            OnPropertyChanged(nameof(HitDelayFixEnabled));
            StateChanged?.Invoke();
        }
    }

    private bool _closestPlayerInfoEnabled = false;
    public bool ClosestPlayerInfoEnabled
    {
        get => _closestPlayerInfoEnabled;
        set
        {
            _closestPlayerInfoEnabled = value;
            OnPropertyChanged(nameof(ClosestPlayerInfoEnabled));
            StateChanged?.Invoke();
        }
    }

    private bool _triggerbotEnabled = false;
    public bool TriggerbotEnabled
    {
        get => _triggerbotEnabled;
        set
        {
            _triggerbotEnabled = value;
            OnPropertyChanged(nameof(TriggerbotEnabled));
            if (_triggerbotEnabled) StartTriggerbotLoop();
            else StopTriggerbotLoop();
            StateChanged?.Invoke();
        }
    }

    private bool _silentAuraEnabled = false;
    public bool SilentAuraEnabled
    {
        get => _silentAuraEnabled;
        set
        {
            if (_silentAuraEnabled == value) return;
            _silentAuraEnabled = value;
            OnPropertyChanged(nameof(SilentAuraEnabled));
            StateChanged?.Invoke();
        }
    }

    private float _silentAuraRange = 3.0f;
    public float SilentAuraRange
    {
        get => _silentAuraRange;
        set
        {
            float clamped = Math.Clamp(value, 2.5f, 4.0f);
            if (Math.Abs(_silentAuraRange - clamped) < 0.001f) return;
            _silentAuraRange = clamped;
            OnPropertyChanged(nameof(SilentAuraRange));
            StateChanged?.Invoke();
        }
    }

    private float _silentAuraAimRange = 4.0f;
    public float SilentAuraAimRange
    {
        get => _silentAuraAimRange;
        set
        {
            float clamped = Math.Clamp(value, 3.0f, 6.0f);
            if (Math.Abs(_silentAuraAimRange - clamped) < 0.001f) return;
            _silentAuraAimRange = clamped;
            OnPropertyChanged(nameof(SilentAuraAimRange));
            StateChanged?.Invoke();
        }
    }

    private float _silentAuraRotSpeed = 35.0f;
    public float SilentAuraRotSpeed
    {
        get => _silentAuraRotSpeed;
        set
        {
            float clamped = Math.Clamp(value, 10.0f, 90.0f);
            if (Math.Abs(_silentAuraRotSpeed - clamped) < 0.001f) return;
            _silentAuraRotSpeed = clamped;
            OnPropertyChanged(nameof(SilentAuraRotSpeed));
            StateChanged?.Invoke();
        }
    }

    private string _silentAuraTargetMode = "distance";
    public string SilentAuraTargetMode
    {
        get => _silentAuraTargetMode;
        set
        {
            string normalized = string.Equals(value, "health", StringComparison.OrdinalIgnoreCase) ? "health" : "distance";
            if (_silentAuraTargetMode == normalized) return;
            _silentAuraTargetMode = normalized;
            OnPropertyChanged(nameof(SilentAuraTargetMode));
            StateChanged?.Invoke();
        }
    }

    private int _silentAuraSwitchDelayMs = 400;
    public int SilentAuraSwitchDelayMs
    {
        get => _silentAuraSwitchDelayMs;
        set
        {
            int clamped = Math.Clamp(value, 0, 2000);
            if (_silentAuraSwitchDelayMs == clamped) return;
            _silentAuraSwitchDelayMs = clamped;
            OnPropertyChanged(nameof(SilentAuraSwitchDelayMs));
            StateChanged?.Invoke();
        }
    }

    private int _silentAuraAccuracy = 90;
    public int SilentAuraAccuracy
    {
        get => _silentAuraAccuracy;
        set
        {
            int clamped = Math.Clamp(value, 50, 100);
            if (_silentAuraAccuracy == clamped) return;
            _silentAuraAccuracy = clamped;
            OnPropertyChanged(nameof(SilentAuraAccuracy));
            StateChanged?.Invoke();
        }
    }

    private bool _silentAuraSpamMode = true;
    public bool SilentAuraSpamMode
    {
        get => _silentAuraSpamMode;
        set
        {
            if (_silentAuraSpamMode == value) return;
            _silentAuraSpamMode = value;
            OnPropertyChanged(nameof(SilentAuraSpamMode));
            StateChanged?.Invoke();
        }
    }

    private float _silentAuraSpamMinCps = 14.0f;
    public float SilentAuraSpamMinCps
    {
        get => _silentAuraSpamMinCps;
        set
        {
            float clamped = Math.Clamp(value, 8.0f, 20.0f);
            if (Math.Abs(_silentAuraSpamMinCps - clamped) < 0.001f) return;
            _silentAuraSpamMinCps = clamped;
            if (_silentAuraSpamMinCps > _silentAuraSpamMaxCps)
                _silentAuraSpamMaxCps = _silentAuraSpamMinCps;
            OnPropertyChanged(nameof(SilentAuraSpamMinCps));
            OnPropertyChanged(nameof(SilentAuraSpamMaxCps));
            StateChanged?.Invoke();
        }
    }

    private float _silentAuraSpamMaxCps = 18.0f;
    public float SilentAuraSpamMaxCps
    {
        get => _silentAuraSpamMaxCps;
        set
        {
            float clamped = Math.Clamp(value, 8.0f, 20.0f);
            if (Math.Abs(_silentAuraSpamMaxCps - clamped) < 0.001f) return;
            _silentAuraSpamMaxCps = clamped;
            if (_silentAuraSpamMinCps > _silentAuraSpamMaxCps)
                _silentAuraSpamMinCps = _silentAuraSpamMaxCps;
            OnPropertyChanged(nameof(SilentAuraSpamMinCps));
            OnPropertyChanged(nameof(SilentAuraSpamMaxCps));
            StateChanged?.Invoke();
        }
    }

    private bool _speedBridgeEnabled = false;
    public bool SpeedBridgeEnabled
    {
        get => _speedBridgeEnabled;
        set
        {
            if (_speedBridgeEnabled == value) return;
            _speedBridgeEnabled = value;
            OnPropertyChanged(nameof(SpeedBridgeEnabled));
            StateChanged?.Invoke();
        }
    }

    private bool _speedBridgeBlockOnly = true;
    public bool SpeedBridgeBlockOnly
    {
        get => _speedBridgeBlockOnly;
        set
        {
            if (_speedBridgeBlockOnly == value) return;
            _speedBridgeBlockOnly = value;
            OnPropertyChanged(nameof(SpeedBridgeBlockOnly));
            StateChanged?.Invoke();
        }
    }

    private int _speedBridgeDelayMs = 85;
    public int SpeedBridgeDelayMs
    {
        get => _speedBridgeDelayMs;
        set
        {
            int clamped = Math.Clamp(value, 20, 250);
            if (_speedBridgeDelayMs == clamped) return;
            _speedBridgeDelayMs = clamped;
            OnPropertyChanged(nameof(SpeedBridgeDelayMs));
            StateChanged?.Invoke();
        }
    }

    private bool _speedBridgeHoldingShiftOnly = true;
    public bool SpeedBridgeHoldingShiftOnly
    {
        get => _speedBridgeHoldingShiftOnly;
        set
        {
            if (_speedBridgeHoldingShiftOnly == value) return;
            _speedBridgeHoldingShiftOnly = value;
            OnPropertyChanged(nameof(SpeedBridgeHoldingShiftOnly));
            StateChanged?.Invoke();
        }
    }

    private bool _speedBridgeLookingDownOnly = true;
    public bool SpeedBridgeLookingDownOnly
    {
        get => _speedBridgeLookingDownOnly;
        set
        {
            if (_speedBridgeLookingDownOnly == value) return;
            _speedBridgeLookingDownOnly = value;
            OnPropertyChanged(nameof(SpeedBridgeLookingDownOnly));
            StateChanged?.Invoke();
        }
    }

    private bool _triggerbotOnlyCrosshair = true;
    public bool TriggerbotOnlyCrosshair
    {
        get => _triggerbotOnlyCrosshair;
        set
        {
            _triggerbotOnlyCrosshair = value;
            OnPropertyChanged(nameof(TriggerbotOnlyCrosshair));
            StateChanged?.Invoke();
        }
    }

    private bool _triggerbotOnlyIfCanAttack = true;
    public bool TriggerbotOnlyIfCanAttack
    {
        get => _triggerbotOnlyIfCanAttack;
        set
        {
            _triggerbotOnlyIfCanAttack = value;
            OnPropertyChanged(nameof(TriggerbotOnlyIfCanAttack));
            StateChanged?.Invoke();
        }
    }

    private int _triggerbotCooldownThreshold = 92;
    public int TriggerbotCooldownThreshold
    {
        get => _triggerbotCooldownThreshold;
        set
        {
            int clamped = Math.Clamp(value, 1, 100);
            if (_triggerbotCooldownThreshold != clamped)
            {
                _triggerbotCooldownThreshold = clamped;
                OnPropertyChanged(nameof(TriggerbotCooldownThreshold));
                StateChanged?.Invoke();
            }
        }
    }

    private int _triggerbotHitChance = 100;
    public int TriggerbotHitChance
    {
        get => _triggerbotHitChance;
        set
        {
            int clamped = Math.Clamp(value, 1, 100);
            if (_triggerbotHitChance != clamped)
            {
                _triggerbotHitChance = clamped;
                OnPropertyChanged(nameof(TriggerbotHitChance));
                StateChanged?.Invoke();
            }
        }
    }

    private bool _triggerbotRequireClick = true;
    public bool TriggerbotRequireClick
    {
        get => _triggerbotRequireClick;
        set
        {
            _triggerbotRequireClick = value;
            OnPropertyChanged(nameof(TriggerbotRequireClick));
            StateChanged?.Invoke();
        }
    }

    private HudLayout _hudLayout = new HudLayout();
    public HudLayout HudLayout
    {
        get => _hudLayout;
        set
        {
            if (_hudLayout != value)
            {
                _hudLayout = value;
                OnPropertyChanged(nameof(HudLayout));
                StateChanged?.Invoke();
            }
        }
    }

    private bool _hudEditorActive = false;
    public bool HudEditorActive
    {
        get => _hudEditorActive;
        set
        {
            if (_hudEditorActive != value)
            {
                _hudEditorActive = value;
                OnPropertyChanged(nameof(HudEditorActive));
                StateChanged?.Invoke();
            }
        }
    }

    public void UpdateGtbFromActionBar(string actionBarText)
    {
        if (!GtbHelperEnabled)
        {
            SetGtbState("", 0, "");
            return;
        }

        GtbWordSolver.TryLearnSolvedWord(actionBarText);
        var (mask, matches) = GtbWordSolver.Solve(actionBarText, maxResults: 200);
        if (string.IsNullOrWhiteSpace(mask))
        {
            SetGtbState("", 0, "");
            return;
        }

        string preview = matches.Count == 0
            ? "No matches"
            : string.Join(", ", matches.Select(m => m));
        SetGtbState(mask, matches.Count, preview);
    }

    private void SetGtbState(string hint, int count, string preview)
    {
        GtbCurrentHint = string.IsNullOrWhiteSpace(hint) ? "-" : hint;
        GtbMatchCount = count;
        GtbMatchesPreview = string.IsNullOrWhiteSpace(preview) ? "-" : preview;
    }
    
    private async Task AimAssistLoop(CancellationToken token)
    {
        try
        {
            while (!token.IsCancellationRequested)
            {
                bool supportedVersion = GameStateClient.Instance.SupportsModule("aimassist");
                bool shouldRun =
                    AimAssistEnabled &&
                    supportedVersion &&
                    GameStateClient.Instance.IsConnected &&
                    WindowDetection.IsMinecraftActive();

                if (!shouldRun)
                {
                    await Task.Delay(8, token).ConfigureAwait(false);
                    continue;
                }

                var state = GameStateClient.Instance.CurrentState;
                if (state.GuiOpen && WindowDetection.IsCursorVisible())
                {
                    await Task.Delay(16, token).ConfigureAwait(false);
                    continue;
                }

                long nowMs = Environment.TickCount64;
                long stateAgeMs;
                if (state.StateMs > 0)
                {
                    long bridgeMs = unchecked((long)state.StateMs);
                    stateAgeMs = nowMs - bridgeMs;
                }
                else
                {
                    stateAgeMs = (long)(DateTime.Now - state.LastUpdate).TotalMilliseconds;
                }
                if (stateAgeMs < 0) stateAgeMs = 0;
                if (stateAgeMs > AimAssistStateFreshMs)
                {
                    await Task.Delay(8, token).ConfigureAwait(false);
                    continue;
                }

                bool leftHeld = (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0;
                bool autoLeftClicking = IsClicking && _useLeftButton;
                if (leftHeld || autoLeftClicking)
                    TryApplyAimAssist();

                await Task.Delay(8, token).ConfigureAwait(false);
            }
        }
        catch (TaskCanceledException) { }
    }

    private async Task PixelPartyAssistInputLoop(CancellationToken token)
    {
        try
        {
            while (!token.IsCancellationRequested)
            {
                bool moduleOk = PixelPartyAssistEnabled
                    && GameStateClient.Instance.SupportsModule("pixelpartyassist")
                    && GameStateClient.Instance.SupportsStateField("pixelpartyyawdelta")
                    && GameStateClient.Instance.IsConnected
                    && WindowDetection.IsMinecraftActive();

                bool walkEnabled = moduleOk && PixelPartyAutoWalkEnabled;
                // Auto-walk steers with mouse only (no A/D); enable look when walk or look is on.
                bool steerEnabled = moduleOk && (PixelPartyAutoLookEnabled || walkEnabled);

                if (!steerEnabled && !walkEnabled)
                {
                    ReleasePixelPartyWalkKeys();
                    _pixelPartyLastTargetMs = 0;
                    await Task.Delay(16, token).ConfigureAwait(false);
                    continue;
                }

                var state = GameStateClient.Instance.CurrentState;
                if (state.GuiOpen && WindowDetection.IsCursorVisible())
                {
                    ReleasePixelPartyWalkKeys();
                    _pixelPartyLastTargetMs = 0;
                    await Task.Delay(16, token).ConfigureAwait(false);
                    continue;
                }

                long nowMs = Environment.TickCount64;
                long stateAgeMs;
                if (state.StateMs > 0)
                {
                    long bridgeMs = unchecked((long)state.StateMs);
                    stateAgeMs = nowMs - bridgeMs;
                }
                else
                {
                    stateAgeMs = (long)(DateTime.Now - state.LastUpdate).TotalMilliseconds;
                }
                if (stateAgeMs < 0) stateAgeMs = 0;
                if (stateAgeMs > PixelPartyAssistInputStateFreshMs)
                {
                    ReleasePixelPartyWalkKeys();
                    _pixelPartyLastTargetMs = 0;
                    await Task.Delay(8, token).ConfigureAwait(false);
                    continue;
                }

                bool targetFound = state.PixelPartyTargetFound;
                float dist = state.PixelPartyTargetDist;
                float yawDelta = state.PixelPartyYawDelta;

                if (targetFound)
                {
                    _pixelPartyLastTargetMs = nowMs;
                    _pixelPartyLastYawDelta = yawDelta;
                    _pixelPartyLastDist = dist;
                }
                else if (_pixelPartyLastTargetMs != 0 &&
                         (nowMs - _pixelPartyLastTargetMs) <= PixelPartyTargetGraceMs)
                {
                    // Target briefly lost (typically mid-jump): coast on last known values
                    // so keys/steering stay engaged and movement doesn't stutter.
                    targetFound = true;
                    yawDelta = _pixelPartyLastYawDelta;
                    dist = _pixelPartyLastDist;
                }

                bool precisionRange = targetFound && dist < PixelPartyPrecisionDist;
                bool canWalk = targetFound && dist > PixelPartyMinWalkDist;

                if (steerEnabled && targetFound)
                    ApplyPixelPartySteer(yawDelta, walkEnabled);

                if (walkEnabled)
                {
                    if (!canWalk)
                    {
                        ReleasePixelPartyWalkKeys();
                    }
                    else
                    {
                        SetPixelPartyWalkKey(VK_A, false, ref _pixelPartyKeyA);
                        SetPixelPartyWalkKey(VK_D, false, ref _pixelPartyKeyD);

                        float yawAbs = Math.Abs(yawDelta);
                        float alignMax = precisionRange ? PixelPartyPrecisionAlignMaxDeg : PixelPartyWalkAlignMaxDeg;
                        bool alignedForward = yawAbs < alignMax;
                        // Close range: walk onto the block even when not perfectly aligned.
                        bool walkForward = alignedForward
                            || (precisionRange && dist < 2.2f && yawAbs < 75f);
                        SetPixelPartyWalkKey(VK_W, walkForward, ref _pixelPartyKeyW);

                        bool jumpDown = walkForward
                            && !precisionRange
                            && dist >= PixelPartyJumpMinDist
                            && ((Environment.TickCount64 / PixelPartyJumpPulseHalfMs) % 2) == 0;
                        SetPixelPartyWalkKey(VK_SPACE, jumpDown, ref _pixelPartyKeySpace);
                    }
                }
                else
                {
                    ReleasePixelPartyWalkKeys();
                }

                int loopDelayMs = (steerEnabled || walkEnabled) && targetFound ? 4 : 8;
                await Task.Delay(loopDelayMs, token).ConfigureAwait(false);
            }
        }
        catch (TaskCanceledException)
        {
            ReleasePixelPartyWalkKeys();
        }
    }

    private async Task TriggerbotLoop(CancellationToken token)
    {
        bool hadTarget = false;
        long targetReadyAt = 0;
        long lastTriggerbotClickAt = 0;

        try
        {
            while (!token.IsCancellationRequested)
            {
                bool supportedVersion = GameStateClient.Instance.SupportsModule("triggerbot");
                bool shouldRun =
                    TriggerbotEnabled &&
                    supportedVersion &&
                    GameStateClient.Instance.IsConnected &&
                    WindowDetection.IsMinecraftActive() &&
                    !WindowDetection.IsCursorVisible();

                if (!shouldRun)
                {
                    hadTarget = false;
                    await Task.Delay(25, token).ConfigureAwait(false);
                    continue;
                }

                var state = GameStateClient.Instance.CurrentState;
                long nowMs = Environment.TickCount64;
                long stateAgeMs;
                if (state.StateMs > 0)
                {
                    long bridgeMs = unchecked((long)state.StateMs);
                    stateAgeMs = nowMs - bridgeMs;
                }
                else
                {
                    stateAgeMs = (long)(DateTime.Now - state.LastUpdate).TotalMilliseconds;
                }
                if (stateAgeMs < 0) stateAgeMs = 0;
                bool staleState = stateAgeMs > TriggerbotStateFreshMs;
                if (staleState)
                {
                    hadTarget = false;
                    await Task.Delay(8, token).ConfigureAwait(false);
                    continue;
                }

                if (state.GuiOpen && WindowDetection.IsCursorVisible())
                {
                    hadTarget = false;
                    await Task.Delay(20, token).ConfigureAwait(false);
                    continue;
                }

                if (BreakBlocksEnabled && (state.BreakingBlock || IsMiningIntent))
                {
                    hadTarget = false;
                    await Task.Delay(12, token).ConfigureAwait(false);
                    continue;
                }

                bool leftHeld = InputHooks.IsPhysicalLeftButtonDown;
                if (TriggerbotRequireClick && !leftHeld)
                {
                    hadTarget = false;
                    await Task.Delay(8, token).ConfigureAwait(false);
                    continue;
                }

                bool crosshairOnEntity = state.LookingAtEntity || state.LookingAtEntityLatched || IsCrosshairEntityFallback(state);
                bool canAttackTarget = CanAttackTargetNow(state);
                bool hasTarget =
                    (!TriggerbotOnlyCrosshair || crosshairOnEntity) &&
                    (!TriggerbotOnlyIfCanAttack || canAttackTarget);

                if (!hasTarget)
                {
                    hadTarget = false;
                    await Task.Delay(8, token).ConfigureAwait(false);
                    continue;
                }

                if (!hadTarget)
                {
                    hadTarget = true;
                    float cooldownThreshold = TriggerbotCooldownThreshold / 100.0f;
                    float thresholdFactor = 1.10f - Math.Clamp(cooldownThreshold, 0.10f, 1.00f);
                    float baseIntervalMs = 40.0f + (thresholdFactor * 110.0f);
                    double reactionFactor = 0.7 + (Random.Shared.NextDouble() * 1.1);
                    int reactionMs = (int)Math.Clamp(baseIntervalMs * reactionFactor, 45.0f, 220.0f);
                    targetReadyAt = nowMs + reactionMs;
                }

                float serverCooldown = Math.Clamp(state.AttackCooldown, 0.0f, 1.0f);
                float cooldownPerTick = NormalizeCooldownPerTick(state.AttackCooldownPerTick);
                float localCooldown = lastTriggerbotClickAt == 0
                    ? 1.0f
                    : Math.Clamp(((nowMs - lastTriggerbotClickAt) / 50.0f) * cooldownPerTick, 0.0f, 1.0f);
                float cooldownBase = Math.Min(serverCooldown, localCooldown);
                float predictedCooldown = cooldownBase;
                if (targetReadyAt > nowMs)
                {
                    double msAhead = targetReadyAt - nowMs;
                    float localAhead = Math.Clamp(localCooldown + (float)(msAhead / 50.0) * cooldownPerTick, 0.0f, 1.0f);
                    predictedCooldown = Math.Min(serverCooldown, localAhead);
                }

                float threshold = TriggerbotCooldownThreshold / 100.0f;
                bool cooldownReady = serverCooldown >= threshold && predictedCooldown >= threshold;
                long minIntervalMs = (long)Math.Ceiling((threshold / Math.Max(0.01f, cooldownPerTick)) * 50.0f);
                bool intervalReady = lastTriggerbotClickAt == 0 || (nowMs - lastTriggerbotClickAt) >= minIntervalMs;
                long fullCooldownMs = (long)Math.Ceiling(50.0f / Math.Max(0.01f, cooldownPerTick));
                bool hardCapReady = lastTriggerbotClickAt == 0 || (nowMs - lastTriggerbotClickAt) >= fullCooldownMs;

                if (cooldownReady && intervalReady && hardCapReady && nowMs >= targetReadyAt)
                {
                    bool didClick = false;
                    if (Random.Shared.Next(1, 101) <= TriggerbotHitChance)
                    {
                        PerformClick(true);
                        didClick = true;
                    }

                    if (didClick)
                    {
                        lastTriggerbotClickAt = nowMs;
                        int postClickDelay = Random.Shared.Next(18, 46);
                        if (Random.Shared.NextDouble() < 0.10)
                            postClickDelay += Random.Shared.Next(20, 65);
                        targetReadyAt = nowMs + postClickDelay;
                    }
                    else
                    {
                        targetReadyAt = nowMs + Random.Shared.Next(18, 34);
                    }
                }

                await Task.Delay(4, token).ConfigureAwait(false);
            }
        }
        catch (TaskCanceledException) { }
    }

    private bool IsCrosshairEntityFallback(GameState state)
    {
        if (state.Entities.Count == 0) return false;
        var rect = WindowDetection.GetMinecraftWindowRect();
        if (!rect.HasValue) return false;

        int width = state.ViewportWidth > 0
            ? state.ViewportWidth
            : rect.Value.Right - rect.Value.Left;
        int height = state.ViewportHeight > 0
            ? state.ViewportHeight
            : rect.Value.Bottom - rect.Value.Top;
        if (width <= 0 || height <= 0) return false;

        double cx = width * 0.5;
        double cy = height * 0.5;
        const double radiusSq = 24.0 * 24.0;

        foreach (var entity in state.Entities)
        {
            if (entity.Hp <= 0.1f) continue;
            if (entity.Sx < 0 || entity.Sx > width || entity.Sy < 0 || entity.Sy > height) continue;
            double dx = entity.Sx - cx;
            double dy = entity.Sy - cy;
            if ((dx * dx + dy * dy) <= radiusSq)
                return true;
        }

        return false;
    }

    private bool CanAttackTargetNow(GameState state)
    {
        if (state.Entities.Count == 0) return false;

        if (state.LookingAtEntity || state.LookingAtEntityLatched) return true;
        if (!IsCrosshairEntityFallback(state)) return false;

        var rect = WindowDetection.GetMinecraftWindowRect();
        if (!rect.HasValue) return false;

        int width = state.ViewportWidth > 0
            ? state.ViewportWidth
            : rect.Value.Right - rect.Value.Left;
        int height = state.ViewportHeight > 0
            ? state.ViewportHeight
            : rect.Value.Bottom - rect.Value.Top;
        if (width <= 0 || height <= 0) return false;

        double cx = width * 0.5;
        double cy = height * 0.5;
        const double radiusSq = 30.0 * 30.0;

        double bestDist = double.MaxValue;
        bool foundInCrosshair = false;

        foreach (var entity in state.Entities)
        {
            if (entity.Hp <= 0.1f) continue;
            if (entity.Dist <= 0.01 || entity.Dist > 3.8) continue;
            if (entity.Sx < 0 || entity.Sx > width || entity.Sy < 0 || entity.Sy > height) continue;

            double dx = entity.Sx - cx;
            double dy = entity.Sy - cy;
            double distSq = dx * dx + dy * dy;
            if (distSq > radiusSq) continue;

            foundInCrosshair = true;
            if (entity.Dist < bestDist)
                bestDist = entity.Dist;
        }

        if (!foundInCrosshair) return false;
        return bestDist <= 3.8;
    }

    private static float NormalizeCooldownPerTick(float raw)
    {
        if (!float.IsFinite(raw) || raw <= 0.0f) return 0.08f;
        float normalized;
        if (raw > 1.0f)
        {
            float asTicks = 1.0f / raw;
            float asAttackSpeed = raw / 20.0f;
            normalized = Math.Min(asTicks, asAttackSpeed);
        }
        else
        {
            normalized = raw;
        }
        return Math.Clamp(normalized, 0.025f, 0.25f);
    }

    private static bool IsAlwaysBlockedGuiScreen(string screen)
    {
        return screen.Contains("GuiInventory", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("InventoryScreen", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("class_490", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("GuiCrafting", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("CraftingScreen", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("class_479", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("GuiFurnace", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("FurnaceScreen", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("class_3871", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("AbstractFurnace", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("GuiRepair", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("AnvilScreen", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("class_471", StringComparison.OrdinalIgnoreCase);
    }

    private static bool IsChestGuiScreen(string screen)
    {
        return screen.Contains("GuiChest", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("ContainerScreen", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("class_481", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("GuiContainer", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("HopperScreen", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("class_488", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("ShulkerBox", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("class_495", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("HandledScreen", StringComparison.OrdinalIgnoreCase) ||
               screen.Contains("class_465", StringComparison.OrdinalIgnoreCase);
    }

    private async Task ClickLoop(CancellationToken token)
    {
        var stopwatch = new Stopwatch();
        
        while (!token.IsCancellationRequested)
        {
            if (!WindowDetection.IsMinecraftActive())
            {
                await Task.Delay(100, token).ConfigureAwait(false);
                continue;
            }

            // Menu & Inventory Safety Checks
            if (GameStateClient.Instance.IsConnected)
            {
                var state = GameStateClient.Instance.CurrentState;
                if (state.GuiOpen)
                {
                    string screen = state.ScreenName;
                    bool alwaysBlock = IsAlwaysBlockedGuiScreen(screen);
                    bool isChest = IsChestGuiScreen(screen);

                    bool shouldBlock = false;

                    // Universal Ghost UI Check:
                    // If the bridge reports a GUI is open, but the system cursor is HIDDEN,
                    // it implies we are actually in-game (Crosshair mode) and the GUI state is 'ghosted'.
                    if (WindowDetection.IsCursorVisible())
                    {
                        if (alwaysBlock)
                        {
                            shouldBlock = true;
                        }
                        else if (isChest)
                        {
                            // Block in chests UNLESS "Click In Chests" is enabled
                            if (!ClickInChests) shouldBlock = true;
                        }
                        else
                        {
                            // Menu (Escape, Settings, Chat, etc.)
                            // Always block in menus if cursor is visible
                            shouldBlock = true;
                        }
                    }

                    if (shouldBlock)
                    {
                        await Task.Delay(100, token).ConfigureAwait(false);
                        continue;
                    }
                }
            }
            else
            {
                // Fallback if not connected: Cursor check
                if (WindowDetection.IsCursorVisible())
                {
                     await Task.Delay(100, token).ConfigureAwait(false);
                     continue;
                }
            }
            
            // Right Click Logic: "Only hold block" check
            if (!_useLeftButton && RightClickOnlyBlock)
            {
                // Fail-open when state is unavailable; only pause when connected and confirmed not holding a block.
                if (GameStateClient.Instance.IsConnected && !GameStateClient.Instance.CurrentState.HoldingBlock)
                {
                    await Task.Delay(100, token).ConfigureAwait(false);
                    continue;
                }
            }
            
            // Break Blocks Logic: 
            if (_useLeftButton && BreakBlocksEnabled)
            {
                if (GameStateClient.Instance.IsConnected)
                {
                    var state = GameStateClient.Instance.CurrentState;
                    bool allowChestClicks =
                        state.GuiOpen &&
                        WindowDetection.IsCursorVisible() &&
                        ClickInChests &&
                        IsChestGuiScreen(state.ScreenName);

                    if (allowChestClicks)
                    {
                        // Chest GUI clicks should never be treated as block-mining intent.
                        IsMiningIntent = false;
                    }
                    else if (GameStateClient.Instance.SupportsStateField("breakingBlock"))
                    {
                        // Modern state payload: pause when we are actually breaking a block.
                        if (!state.LookingAtBlock)
                            IsMiningIntent = false;

                        if (state.BreakingBlock || IsMiningIntent)
                        {
                            await Task.Delay(25, token).ConfigureAwait(false);
                            continue;
                        }
                    }
                    else
                    {
                        // Legacy fallback behavior (intent-based).
                        if (!state.LookingAtBlock)
                            IsMiningIntent = false;

                        if (IsMiningIntent)
                        {
                            await Task.Delay(50, token).ConfigureAwait(false);
                            continue;
                        }
                    }
                }
            }
            
            stopwatch.Restart();
            
            float minCps = _useLeftButton ? MinCPS : RightMinCPS;
            float maxCps = _useLeftButton ? MaxCPS : RightMaxCPS;
            if (minCps > maxCps) minCps = maxCps;
            
            // Calculate CPS with optional jitter (gaussian distribution)
            float cps;
            if (JitterEnabled)
            {
                float midCps = (minCps + maxCps) / 2.0f;
                // Slightly widen range for Gaussian to touch edges
                float range = (maxCps - minCps) / 4.0f; 
                cps = GaussianRandom(midCps, range);
                cps = Math.Clamp(cps, minCps, maxCps);
            }
            else
            {
                cps = minCps + (float)_random.NextDouble() * (maxCps - minCps);
            }
            
            double targetInterval = 1000.0 / cps; // in milliseconds

            // Perform click
            StatsTracker.Instance.RecordClick(cps, _useLeftButton);
            PerformClick(_useLeftButton);
            
            // Drift Compensation
            // Stop stopwatch to see how long click + logic took
            stopwatch.Stop();
            double elapsed = stopwatch.Elapsed.TotalMilliseconds;
            
            // Calculate remaining wait time, compensating for elapsed time
            int waitTime = (int)(targetInterval - elapsed);
            if (waitTime < 1) waitTime = 1; // Always yield at least a bit
            
            try
            {
                await Task.Delay(waitTime, token).ConfigureAwait(false);
            }
            catch (TaskCanceledException)
            {
                break;
            }
        }
    }
    
    private void PerformClick(bool leftButton)
    {
        INPUT[] inputs = leftButton ? _leftClickInputs : _rightClickInputs;
        lock (_sendInputLock)
        {
            SendInput(2, inputs, Marshal.SizeOf<INPUT>());
        }
    }

    private void TryApplyAimAssist()
    {
        var state = GameStateClient.Instance.CurrentState;
        if (state.Entities.Count == 0)
        {
            _aimAssistFilteredDx = 0.0;
            _aimAssistFilteredDy = 0.0;
            return;
        }
        bool isLegacyBridge = GameStateClient.Instance.InjectedVersion.StartsWith("1.8", StringComparison.OrdinalIgnoreCase);

        var rect = WindowDetection.GetMinecraftWindowRect();
        if (!rect.HasValue) return;

        int width = state.ViewportWidth > 0
            ? state.ViewportWidth
            : rect.Value.Right - rect.Value.Left;
        int height = state.ViewportHeight > 0
            ? state.ViewportHeight
            : rect.Value.Bottom - rect.Value.Top;
        if (width <= 0 || height <= 0) return;

        double centerX = width * 0.5;
        double centerY = height * 0.5;
        double maxAngle = AimAssistFov * 0.5;
        double legacyFocalLengthPx = 1.0;
        if (isLegacyBridge)
        {
            double gameFov = state.Fov;
            if (gameFov < 10.0 || gameFov > 170.0)
                gameFov = 70.0;
            legacyFocalLengthPx = height / (2.0 * Math.Tan((gameFov * Math.PI / 180.0) * 0.5));
            if (legacyFocalLengthPx < 1.0)
                legacyFocalLengthPx = 1.0;
        }
        double bestScore = double.MaxValue;
        double bestDx = 0;
        double bestDy = 0;

        foreach (var entity in state.Entities)
        {
            if (entity.Dist <= 0.01 || entity.Dist > AimAssistRange) continue;
            if (entity.Sx < 0 || entity.Sx > width || entity.Sy < 0 || entity.Sy > height) continue;

            double dx = entity.Sx - centerX;
            double dy = entity.Sy - centerY;
            double radial = Math.Sqrt(dx * dx + dy * dy);
            double angle = isLegacyBridge
                ? Math.Atan2(radial, legacyFocalLengthPx) * (180.0 / Math.PI)
                : Math.Atan2(radial, centerX) * (180.0 / Math.PI);
            if (angle > maxAngle) continue;

            double score = dx * dx + dy * dy;
            if (score < bestScore)
            {
                bestScore = score;
                bestDx = dx;
                bestDy = dy;
            }
        }

        if (bestScore == double.MaxValue) return;

        // Light temporal filter keeps closest-point targeting, but suppresses endpoint orbiting.
        const double filterAlpha = 0.40;
        _aimAssistFilteredDx += (bestDx - _aimAssistFilteredDx) * filterAlpha;
        _aimAssistFilteredDy += (bestDy - _aimAssistFilteredDy) * filterAlpha;

        double strengthNorm = AimAssistStrength / 100.0;
        double strength = isLegacyBridge
            ? (0.16 + 0.54 * strengthNorm * strengthNorm)
            : (0.08 + 0.36 * strengthNorm * strengthNorm);

        int moveX = (int)Math.Round(_aimAssistFilteredDx * strength);
        int moveY = (int)Math.Round(_aimAssistFilteredDy * strength);

        if (moveX == 0 && Math.Abs(bestDx) > 1) moveX = Math.Sign(bestDx);
        if (moveY == 0 && Math.Abs(bestDy) > 1) moveY = Math.Sign(bestDy);

        int maxStep = isLegacyBridge
            ? (int)Math.Round(8 + 14 * strengthNorm)
            : (int)Math.Round(4 + 16 * strengthNorm);
        moveX = Math.Clamp(moveX, -maxStep, maxStep);
        moveY = Math.Clamp(moveY, -maxStep, maxStep);

        if (moveX == 0 && moveY == 0) return;

        _aimAssistMoveInput[0].Mi.Dx = moveX;
        _aimAssistMoveInput[0].Mi.Dy = moveY;
        lock (_sendInputLock)
        {
            SendInput(1, _aimAssistMoveInput, Marshal.SizeOf<INPUT>());
        }
    }
     
    private float GaussianRandom(float mean, float stddev)
    {
        // Box-Muller transform
        double u1 = 1.0 - _random.NextDouble();
        double u2 = 1.0 - _random.NextDouble();
        double z = Math.Sqrt(-2.0 * Math.Log(u1)) * Math.Cos(2.0 * Math.PI * u2);
        return (float)(mean + z * stddev);
    }
    
    private void OnPropertyChanged(string name)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
    }
}
