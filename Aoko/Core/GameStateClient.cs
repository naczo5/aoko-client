using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using Aoko;

namespace Aoko.Core;

/// <summary>
/// TCP client that connects to the injected Java agent running inside Minecraft.
/// Receives adaptive game-state updates and exposes the latest snapshot to the app.
/// </summary>
public class GameStateClient : INotifyPropertyChanged
{
    private static GameStateClient? _instance;
    public static GameStateClient Instance => _instance ??= new GameStateClient();

    private TcpClient? _client;
    private CancellationTokenSource? _cts;
    private Task? _configSenderTask;
    private Task? _readTask;
    private readonly int _port = 25590;
    private readonly SemaphoreSlim _sendLock = new(1, 1);
    private readonly SemaphoreSlim _configChangedSignal = new(0, 1);
    private readonly ManagedTransportDiagnostics _transportDiagnostics =
        ManagedTransportDiagnostics.FromEnvironment();
    private readonly CoalescedLatestValue<string> _actionBarDispatcher;
    private const int ConfigHeartbeatMs = 2000;
    private const int ConfigChangeCoalesceMs = 25;
    private const int StateNotificationIntervalMs = 25;
    private const int MaximumInboundMessageCharacters = 1024 * 1024;

    private volatile GameState _currentState = new();
    private readonly CoalescedCallback _stateNotification;
    private bool _isConnected;
    private bool _isInjected;
    private string _statusMessage = "Not injected";
    private string _injectedVersion = "1.8.9";
    private BridgeCapabilities _capabilities = BridgeCapabilities.ForVersionFallback("1.8.9");
    private int _injectionProgress;
    private bool _isInjectionInProgress;
    private long _lastUiActionBarDispatchTicks;
    private int _reloadMappingsNonce;
    private IntPtr _targetHwnd;
    private volatile bool _suppressConfigPush = false;
    private long _configRevision = 1;
    private int _configChangeTrackingAttached;

    public event PropertyChangedEventHandler? PropertyChanged;
    public event Action? StateUpdated;

    private GameStateClient()
    {
        _stateNotification = new CoalescedCallback(
            NotifyStateUpdated,
            StateNotificationIntervalMs);
        _actionBarDispatcher = new CoalescedLatestValue<string>(
            action =>
            {
                System.Windows.Threading.Dispatcher? dispatcher =
                    System.Windows.Application.Current?.Dispatcher;
                if (dispatcher == null)
                    throw new InvalidOperationException("WPF dispatcher is unavailable.");
                dispatcher.BeginInvoke(action);
            },
            actionBar => Clicker.Instance.UpdateGtbFromActionBar(actionBar));
    }

    // === Properties ===

    public GameState CurrentState
    {
        get => _currentState;
        private set
        {
            _currentState = value;
            _stateNotification.Signal();
        }
    }

    private void NotifyStateUpdated()
    {
        _transportDiagnostics.RecordStateNotification();
        OnPropertyChanged(nameof(CurrentState));
        StateUpdated?.Invoke();
    }

    public bool IsConnected
    {
        get => _isConnected;
        private set
        {
            if (_isConnected != value)
            {
                _isConnected = value;
                OnPropertyChanged(nameof(IsConnected));
                OnPropertyChanged(nameof(StatusMessage));
                if (value)
                {
                    _isInjectionInProgress = false;
                    _injectionProgress = 100;
                    OnPropertyChanged(nameof(IsInjectionInProgress));
                    OnPropertyChanged(nameof(InjectionProgress));
                }
            }
        }
    }

    public bool IsInjected
    {
        get => _isInjected;
        private set
        {
            if (_isInjected != value)
            {
                _isInjected = value;
                OnPropertyChanged(nameof(IsInjected));
                OnPropertyChanged(nameof(StatusMessage));
            }
        }
    }

    public string StatusMessage
    {
        get
        {
            if (IsConnected) return "Connected — receiving game state";
            if (IsInjected) return "Injected — waiting for connection...";
            return _statusMessage;
        }
        private set
        {
            _statusMessage = value;
            OnPropertyChanged(nameof(StatusMessage));
        }
    }

    public string InjectedVersion
    {
        get => _injectedVersion;
        private set
        {
            if (_injectedVersion != value)
            {
                _injectedVersion = value;
                OnPropertyChanged(nameof(InjectedVersion));
            }
        }
    }

    public int InjectionProgress
    {
        get => _injectionProgress;
        private set
        {
            int clamped = Math.Clamp(value, 0, 100);
            if (_injectionProgress != clamped)
            {
                _injectionProgress = clamped;
                OnPropertyChanged(nameof(InjectionProgress));
            }
        }
    }

    public bool IsInjectionInProgress
    {
        get => _isInjectionInProgress;
        private set
        {
            if (_isInjectionInProgress != value)
            {
                _isInjectionInProgress = value;
                OnPropertyChanged(nameof(IsInjectionInProgress));
            }
        }
    }

    private void SetInjectionStage(int progress, string stageText)
    {
        IsInjectionInProgress = true;
        InjectionProgress = progress;
        StatusMessage = $"{stageText} ({InjectionProgress}%)";
    }

    public void RequestBridgeMappingReload()
    {
        Interlocked.Increment(ref _reloadMappingsNonce);
        MarkBridgeConfigDirty();
        Log("Queued bridge mapping reload request.");
    }

    // === Injection ===

    /// <summary>
    /// Injects the agent into the selected Java/Minecraft client process.
    /// </summary>
    /// <summary>
    /// Connects to the agent, which should be loaded at startup via -javaagent.
    /// Uses the same method name to keep compatibility with existing UI calls,
    /// but functionally it's now a "Connect" operation.
    /// </summary>
    public async Task<bool> InjectAsync(string version = "auto", int? targetPid = null, IntPtr? targetHwnd = null)
    {
        if (IsInjected || IsConnected)
        {
            StatusMessage = "Already connected/injected";
            IsInjectionInProgress = false;
            InjectionProgress = 100;
            return true;
        }

        InjectionTarget? automaticTarget = targetPid.HasValue
            ? null
            : InjectionTargetDiscovery.FindBestTarget();
        var mcProcess = targetPid.HasValue
            ? ResolveInjectionTarget(targetPid)
            : ResolveInjectionTarget(automaticTarget?.ProcessId);
        IntPtr? resolvedTargetHwnd = targetHwnd ?? automaticTarget?.Hwnd;
        string resolvedVersion = ResolveInjectionVersion(version, mcProcess);
        Log($"Resolved injection version: requested={version}, resolved={resolvedVersion}, title='{mcProcess?.MainWindowTitle ?? "<none>"}', pid={mcProcess?.Id.ToString() ?? "<none>"}");
        Capabilities = BridgeCapabilities.ForVersionFallback(resolvedVersion);

        int? bridgeListenerPid = TcpPortHelper.TryGetListeningProcessId(_port);

        if (targetPid.HasValue)
        {
            if (bridgeListenerPid == targetPid.Value)
            {
                SetInjectionStage(5, "Reconnecting to selected bridge");
                await ConnectAsync(
                    maxAttempts: 8,
                    onAttempt: (attempt, total) =>
                    {
                        int mapped = 5 + (attempt * 15 / total);
                        SetInjectionStage(mapped, $"Reconnecting to selected bridge ({attempt}/{total})");
                    },
                    reportFailure: false);

                if (IsConnected)
                {
                    IsInjected = true;
                    InjectedVersion = resolvedVersion;
                    ApplyInjectionTargetWindow(resolvedTargetHwnd);
                    IsInjectionInProgress = false;
                    InjectionProgress = 100;
                    return true;
                }
            }
            else if (bridgeListenerPid.HasValue)
            {
                StatusMessage = $"ERROR: Bridge port {_port} is already in use by PID {bridgeListenerPid.Value}. Close the other Java/Minecraft window first.";
                Log(StatusMessage);
                IsInjectionInProgress = false;
                return false;
            }
        }
        else
        {
            SetInjectionStage(5, "Checking existing bridge");

            // Auto inject: connect to any existing bridge before injecting.
            await ConnectAsync(
                maxAttempts: 8,
                onAttempt: (attempt, total) =>
                {
                    int mapped = 5 + (attempt * 15 / total);
                    SetInjectionStage(mapped, $"Checking existing bridge ({attempt}/{total})");
                },
                reportFailure: false);

            if (IsConnected)
            {
                IsInjected = true;
                InjectedVersion = resolvedVersion;
                ApplyInjectionTargetWindow(resolvedTargetHwnd);
                IsInjectionInProgress = false;
                InjectionProgress = 100;
                return true;
            }
        }

        // Inject Native Bridge
        SetInjectionStage(20, "Injecting bridge");
        if (mcProcess == null)
        {
            StatusMessage = targetPid.HasValue
                ? $"ERROR: Process PID {targetPid.Value} not found."
                : "ERROR: No compatible Java/Minecraft process found.";
            IsInjectionInProgress = false;
            return false;
        }
        
        string baseDir = AppDomain.CurrentDomain.BaseDirectory;
        string dllName = ResolveBridgeDllName(resolvedVersion);
        string dllPath = Path.Combine(baseDir, dllName);
        SetInjectionStage(20, $"Injecting {dllName}");
        
        Log($"Attempting to inject: {dllPath} into PID {mcProcess.Id}");

        if (!File.Exists(dllPath))
        {
             StatusMessage = $"ERROR: {dllName} not found.";
             Log($"{dllName} not found at " + dllPath);
             IsInjectionInProgress = false;
             return false;
        }

        bool injected = await Task.Run(() =>
            NativeInjector.Inject(mcProcess.Id, dllPath, (pct, msg) =>
            {
                int mapped = 20 + (pct * 60 / 100);
                SetInjectionStage(mapped, msg);
            }));
        if (!injected)
        {
             StatusMessage = "ERROR: Injection failed. Check logs.";
             Log("NativeInjector.Inject returned false.");
             IsInjectionInProgress = false;
             return false;
        }
        
        Log("Injection successful (ostensibly). Waiting for bridge...");
        SetInjectionStage(85, "Bridge injected, waiting for connection");

        Log("Attempting to connect to bridge...");
        await ConnectAsync(
            maxAttempts: 30,
            onAttempt: (attempt, total) =>
            {
                int mapped = 85 + (attempt * 14 / total);
                SetInjectionStage(mapped, $"Waiting for bridge startup ({attempt}/{total})");
            });

        if (IsConnected)
        {
            if (targetPid.HasValue)
            {
                int? listenerPid = TcpPortHelper.TryGetListeningProcessId(_port);
                if (listenerPid.HasValue && listenerPid.Value != targetPid.Value)
                {
                    Disconnect();
                    StatusMessage = $"ERROR: Connected to PID {listenerPid.Value}, not the selected PID {targetPid.Value}. Close the other Java/Minecraft window.";
                    Log(StatusMessage);
                    IsInjectionInProgress = false;
                    return false;
                }
            }

            IsInjected = true;
            InjectedVersion = resolvedVersion;
            ApplyInjectionTargetWindow(resolvedTargetHwnd);
            Log("Connected successfully!");
            IsInjectionInProgress = false;
            InjectionProgress = 100;
            return true;
        }

        StatusMessage = "ERROR: Connectivity failed after injection.";
        Log("Failed to connect to bridge TCP server.");
        IsInjectionInProgress = false;
        return false;
    }

    internal static Process? ResolveInjectionTarget(int? targetPid)
    {
        if (!targetPid.HasValue)
            return null;

        try
        {
            return Process.GetProcessById(targetPid.Value);
        }
        catch
        {
            return null;
        }
    }

    private void ApplyInjectionTargetWindow(IntPtr? targetHwnd)
    {
        if (!targetHwnd.HasValue || targetHwnd.Value == IntPtr.Zero)
            return;

        _targetHwnd = targetHwnd.Value;
        WindowDetection.SetTargetWindow(_targetHwnd);
    }

    private void Log(string message)
    {
        // Avoid file I/O on UI thread or frequent calls
        Debug.WriteLine($"[{DateTime.Now:HH:mm:ss}] [GameStateClient] {message}");
    }

    // === TCP Connection ===

    public async Task ConnectAsync(int maxAttempts = 20, Action<int, int>? onAttempt = null, bool reportFailure = true)
    {
        var previousCts = Interlocked.Exchange(ref _cts, null);
        var previousConfigTask = _configSenderTask;
        var previousReadTask = _readTask;
        _configSenderTask = null;
        _readTask = null;
        if (previousCts != null)
        {
            previousCts.Cancel();
            _ = DisposeCtsWhenDoneAsync(previousCts, previousConfigTask, previousReadTask);
        }

        _cts = new CancellationTokenSource();
        var token = _cts.Token;

        // Retry connection with configurable attempt count (500ms delay between attempts)
        for (int attempt = 0; attempt < maxAttempts; attempt++)
        {
            if (token.IsCancellationRequested) return;
            onAttempt?.Invoke(attempt + 1, maxAttempts);

            try
            {
                _client = new TcpClient();
                await _client.ConnectAsync("127.0.0.1", _port, token);
                IsConnected = true;
                StatusMessage = "Connected!";
                break;
            }
            catch
            {
                _client?.Dispose();
                _client = null;
                if (attempt + 1 < maxAttempts)
                    await Task.Delay(500, token);
            }
        }

        if (!IsConnected)
        {
            if (reportFailure)
                StatusMessage = "ERROR: Could not connect to agent on port " + _port;
            return;
        }

        EnsureConfigChangeTracking();
        MarkBridgeConfigDirty();

        // Start config sender task
        _configSenderTask = Task.Run(() => ConfigSenderLoop(token), token);
        _readTask = Task.Run(() => ReadLoop(token), token);
    }

    public BridgeCapabilities Capabilities
    {
        get => _capabilities;
        private set
        {
            _capabilities = value;
            OnPropertyChanged(nameof(Capabilities));
        }
    }

    public bool SupportsModule(string moduleId)
        => Capabilities.SupportsModule(moduleId);

    public bool SupportsSetting(string settingName)
        => Capabilities.SupportsSetting(settingName);

    public bool SupportsStateField(string fieldName)
        => Capabilities.SupportsStateField(fieldName);

    private async Task ReadLoop(CancellationToken token)
    {
        try
        {
            if (_client == null) return;
            using var stream = _client.GetStream();
            using var reader = new BoundedLineReader(
                stream,
                MaximumInboundMessageCharacters,
                Encoding.UTF8);

            while (!token.IsCancellationRequested && _client.Connected)
            {
                BoundedLineReadResult readResult = await reader.ReadLineAsync(token);
                if (readResult.IsEndOfStream) break;
                if (readResult.IsTooLong)
                {
                    Log($"Ignored bridge message exceeding {MaximumInboundMessageCharacters} characters.");
                    continue;
                }

                string line = readResult.Line!;

                long parseStarted = Stopwatch.GetTimestamp();
                try
                {
                    // Check if it's a command from ClickGUI
                    if (line.Contains("\"type\":\"cmd\""))
                    {
                        HandleBridgeCommand(line);
                        continue;
                    }

                    if (line.Contains("\"type\":\"capabilities\""))
                    {
                        HandleBridgeCapabilities(line);
                        continue;
                    }

                    JsonNode? rawNode = JsonNode.Parse(line);
                    GameState? state = null;
                    if (GameStatePatchMerger.IsPartial(rawNode) && rawNode is JsonObject patch)
                        state = GameStatePatchMerger.Apply(CurrentState, patch);
                    else
                        state = rawNode?.Deserialize<GameState>();
                    if (state != null)
                    {
                        // Apply hudLayout from the same parsed document used for GameState.
                        try
                        {
                            JsonNode? hudLayoutNode = rawNode?["hudLayout"];
                            if (hudLayoutNode != null)
                                ApplyInboundHudLayout(hudLayoutNode);
                        }
                        catch { /* ignore JSON failures for hudLayout */ }

                        state.IsConnected = true;
                        state.LastUpdate = DateTime.Now;
                        CurrentState = state;
                        if (Capabilities.SupportsStateField("actionBar"))
                            QueueActionBarUpdate(state.ActionBar);
                    }
                }
                catch (JsonException)
                {
                    // Skip malformed lines
                }
                finally
                {
                    _transportDiagnostics.RecordInbound(
                        line.Length,
                        Stopwatch.GetTimestamp() - parseStarted);
                    LogTransportDiagnosticsIfReady();
                }
            }
        }
        catch (OperationCanceledException) { }
        catch (Exception ex)
        {
            Debug.WriteLine($"[GameStateClient] Read error: {ex.Message}");
        }
        finally
        {
            IsConnected = false;
            _client?.Dispose();
            _client = null;
            StatusMessage = "Disconnected from agent.";
            Capabilities = BridgeCapabilities.ForVersionFallback(InjectedVersion);
        }
    }


    // P/Invoke for Detach
    [System.Runtime.InteropServices.DllImport("bridge.dll", EntryPoint = "Detach", CallingConvention = System.Runtime.InteropServices.CallingConvention.Cdecl)]
    private static extern void DetachBridge();

    public async Task DetachAsync()
    {
        if (!_isInjected) return;

        var cts = Interlocked.Exchange(ref _cts, null);
        var configTask = _configSenderTask;
        var readTask = _readTask;
        _configSenderTask = null;
        _readTask = null;
        if (cts != null)
        {
            cts.Cancel();
            _ = DisposeCtsWhenDoneAsync(cts, configTask, readTask);
        }
        
        await Task.Run(() =>
        {
             try
             {
                 // Close socket first
                 if (_client != null)
                 {
                     _client.Close();
                     _client = null;
                 }
                 IsConnected = false;
                 
                 // Call native detach
                 try { DetachBridge(); } catch { } 
                 
                 IsInjected = false;
                 StatusMessage = "Detached";
             }
             catch
             {
                 StatusMessage = "Error detaching";
             }
        });
    }

    public void Disconnect()
    {
        var cts = Interlocked.Exchange(ref _cts, null);
        var configTask = _configSenderTask;
        var readTask = _readTask;
        _configSenderTask = null;
        _readTask = null;
        if (cts != null)
        {
            cts.Cancel();
            _ = DisposeCtsWhenDoneAsync(cts, configTask, readTask);
        }
        _client?.Dispose();
        _client = null;
        IsConnected = false;
        IsInjected = false;
        IsInjectionInProgress = false;
        InjectionProgress = 0;
        StatusMessage = "Not injected";
        _targetHwnd = IntPtr.Zero;
        WindowDetection.ClearTargetWindow();
        Capabilities = BridgeCapabilities.ForVersionFallback(InjectedVersion);
    }

    // === Helpers ===

    private static string ResolveInjectionVersion(string requestedVersion, Process? process)
    {
        if (!string.Equals(requestedVersion, "auto", StringComparison.OrdinalIgnoreCase))
        {
            string? explicitVersion = NormalizeDetectedVersion(requestedVersion);
            if (!string.IsNullOrEmpty(explicitVersion))
                return explicitVersion;
        }

        // Prefer the selected process command line (Prism jar paths, --version, etc.).
        // Do not use unrelated launcher history for a concrete PID target.
        string? fromCommandLine = ProcessCommandLine.TryParseVersion(ProcessCommandLine.TryGet(process));
        if (!string.IsNullOrEmpty(fromCommandLine))
            return fromCommandLine;

        string title = process?.MainWindowTitle?.ToLowerInvariant() ?? string.Empty;
        string? fromTitle = NormalizeDetectedVersion(title);
        if (!string.IsNullOrEmpty(fromTitle))
            return fromTitle;

        // Launcher-settings fallback only when there is no process to inspect
        // (avoids a selected Prism/vanilla process picking an unrelated Lunar history version).
        if (process == null)
        {
            string? fromLunarSettings = TryResolveVersionFromLunarSettings();
            if (!string.IsNullOrEmpty(fromLunarSettings))
                return fromLunarSettings;
        }

        // Preserve legacy behavior as safer fallback when auto-detection is inconclusive.
        return "1.8.9";
    }

    /// <summary>
    /// Maps a resolved injection version to the native bridge DLL that must be loaded.
    /// 26.x and 1.21.x share <c>bridge_261.dll</c>; everything else uses legacy <c>bridge.dll</c>.
    /// </summary>
    internal static string ResolveBridgeDllName(string resolvedVersion)
    {
        if (string.IsNullOrWhiteSpace(resolvedVersion))
            return "bridge.dll";

        if (resolvedVersion.StartsWith("26.", StringComparison.OrdinalIgnoreCase)
            || resolvedVersion.StartsWith("1.21", StringComparison.OrdinalIgnoreCase))
        {
            return "bridge_261.dll";
        }

        return "bridge.dll";
    }

    private static string? TryResolveVersionFromLunarSettings()
    {
        try
        {
            string lunarRoot = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".lunarclient");
            string cachePath = Path.Combine(lunarRoot, "settings", "cache.json");
            if (File.Exists(cachePath))
            {
                JsonNode? cache = JsonNode.Parse(File.ReadAllText(cachePath));
                JsonNode? historyNode = cache?["profileSelectHistory"]?["lunar"];
                if (historyNode is JsonArray history)
                {
                    foreach (JsonNode? entry in history)
                    {
                        string? version = entry?["version"]?.GetValue<string>();
                        string? normalized = NormalizeDetectedVersion(version);
                        if (!string.IsNullOrEmpty(normalized))
                            return normalized;
                    }
                }
            }

            string launcherPath = Path.Combine(lunarRoot, "settings", "launcher.json");
            if (File.Exists(launcherPath))
            {
                JsonNode? launcher = JsonNode.Parse(File.ReadAllText(launcherPath));
                string? gameProfile = launcher?["settings"]?["gameProfile"]?.GetValue<string>();
                string? fromProfile = ResolveVersionFromGameProfile(lunarRoot, gameProfile);
                if (!string.IsNullOrEmpty(fromProfile))
                    return fromProfile;
            }
        }
        catch
        {
        }

        return null;
    }

    private static string? ResolveVersionFromGameProfile(string lunarRoot, string? gameProfile)
    {
        if (string.IsNullOrWhiteSpace(gameProfile) || !gameProfile.StartsWith("lunar-", StringComparison.OrdinalIgnoreCase))
            return null;

        string major = gameProfile.Substring("lunar-".Length);
        string profilePath = Path.Combine(lunarRoot, "profiles", "lunar", major, "profile.json");
        if (File.Exists(profilePath))
        {
            try
            {
                JsonNode? profile = JsonNode.Parse(File.ReadAllText(profilePath));
                string? gameVersion = profile?["gameVersion"]?.GetValue<string>();
                string? normalized = NormalizeDetectedVersion(gameVersion);
                if (!string.IsNullOrEmpty(normalized))
                    return normalized;
            }
            catch
            {
            }
        }

        return NormalizeDetectedVersion(major);
    }

    internal static string? NormalizeDetectedVersion(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
            return null;

        string value = raw.Trim().ToLowerInvariant();

        // Use token boundaries so library versions like log4j "2.26.0" do not match "26.2".
        if (ContainsVersionToken(value, "26.2"))
            return "26.2";
        if (ContainsVersionToken(value, "26.1")
            || value == "26"
            || value.Equals("minecraft 26", StringComparison.Ordinal)
            || value.StartsWith("26.", StringComparison.Ordinal))
        {
            return "26.1";
        }
        if (ContainsVersionToken(value, "1.21") || value == "1.21")
            return "1.21";
        if (ContainsVersionToken(value, "1.8.9") || value == "1.8" || value.StartsWith("1.8.", StringComparison.Ordinal))
            return "1.8.9";

        return null;
    }

    /// <summary>
    /// True when <paramref name="version"/> appears as a version token, not as a substring of another number.
    /// </summary>
    private static bool ContainsVersionToken(string value, string version)
    {
        string escaped = Regex.Escape(version);
        return Regex.IsMatch(
            value,
            $@"(?<![\d.]){escaped}(?![\d])",
            RegexOptions.CultureInvariant);
    }


    private string? FindJava()
    {
        // Check known JDK locations
        string[] jdkPaths = {
            @"C:\Program Files\Java\jdk-21.0.10\bin\java.exe",
            @"C:\Program Files\Java\jdk-17\bin\java.exe",
            @"C:\Program Files\Common Files\Oracle\Java\javapath\java.exe",
        };

        foreach (string path in jdkPaths)
        {
            if (File.Exists(path)) return path;
        }

        // Try PATH
        try
        {
            var psi = new ProcessStartInfo("java", "-version")
            {
                UseShellExecute = false,
                RedirectStandardError = true,
                CreateNoWindow = true
            };
            var p = Process.Start(psi);
            p?.WaitForExit(3000);
            if (p is { ExitCode: 0 }) return "java";
        }
        catch { }

        return null;
    }

    /// <summary>
    /// Quick check: is a GUI currently open in-game?
    /// </summary>
    public bool IsGuiOpen => IsConnected && CurrentState.GuiOpen;

    /// <summary>
    /// Quick check: current player health.
    /// </summary>
    public float PlayerHealth => IsConnected ? CurrentState.Health : -1;

    internal static int ModuleListStyleToIndex(string? styleName)
    {
        if (string.IsNullOrWhiteSpace(styleName))
            return 0;

        return styleName.Trim().ToLowerInvariant() switch
        {
            "default" => 0,
            "minimal" => 1,
            "outlined" => 2,
            "glass" => 3,
            "bold" => 4,
            _ => 0
        };
    }

    internal static string BuildAutoRodActionMessage(
        int slotMode, bool verifyForcedSlot, int extensionTicks, bool holdToExtend)
        => JsonSerializer.Serialize(new
        {
            type = "moduleAction",
            action = "autoRod",
            phase = "press",
            enabled = true,
            slotMode = Math.Clamp(slotMode, 0, 9),
            verifyForcedSlot,
            extensionTicks = Math.Clamp(extensionTicks, 1, 40),
            holdToExtend
        }) + "\n";

    internal static string BuildAutoRodReleaseMessage()
        => JsonSerializer.Serialize(new
        {
            type = "moduleAction",
            action = "autoRod",
            phase = "release"
        }) + "\n";

    public async Task<bool> SendAutoRodActionAsync(CancellationToken token = default)
    {
        var clicker = Clicker.Instance;
        if (!clicker.AutoRodEnabled)
            return false;

        string message = BuildAutoRodActionMessage(
            clicker.AutoRodSlotMode,
            clicker.AutoRodVerifyForcedSlot,
            clicker.AutoRodExtensionTicks,
            clicker.AutoRodHoldToExtend);
        try
        {
            return await SendMessageAsync(
                message,
                token,
                () => Clicker.Instance.AutoRodEnabled).ConfigureAwait(false);
        }
        catch (Exception ex) when (ex is IOException or InvalidOperationException or ObjectDisposedException or SocketException or OperationCanceledException)
        {
            Debug.WriteLine($"[GameStateClient] Auto Rod action send failed: {ex.Message}");
            return false;
        }
    }

    public async Task<bool> SendAutoRodReleaseAsync(CancellationToken token = default)
    {
        try
        {
            return await SendMessageAsync(BuildAutoRodReleaseMessage(), token).ConfigureAwait(false);
        }
        catch (Exception ex) when (ex is IOException or InvalidOperationException or ObjectDisposedException or SocketException or OperationCanceledException)
        {
            Debug.WriteLine($"[GameStateClient] Auto Rod release send failed: {ex.Message}");
            return false;
        }
    }

    private async Task<bool> SendMessageAsync(
        string message,
        CancellationToken token,
        Func<bool>? sendGuard = null)
    {
        byte[] data = Encoding.UTF8.GetBytes(message);
        return await SendMessageAsync(data, token, sendGuard).ConfigureAwait(false);
    }

    private void QueueActionBarUpdate(string actionBar)
    {
        long nowTicks = Environment.TickCount64;
        long last = Interlocked.Read(ref _lastUiActionBarDispatchTicks);
        bool allowSchedule = last == 0 || nowTicks - last >= StateNotificationIntervalMs;

        try
        {
            if (_actionBarDispatcher.Publish(actionBar, allowSchedule))
                Interlocked.Exchange(ref _lastUiActionBarDispatchTicks, nowTicks);
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"[GameStateClient] Action-bar dispatch unavailable: {ex.Message}");
        }
    }

    private async Task<bool> SendMessageAsync(
        ReadOnlyMemory<byte> data,
        CancellationToken token,
        Func<bool>? sendGuard = null)
    {
        await _sendLock.WaitAsync(token).ConfigureAwait(false);
        try
        {
            if (sendGuard != null && !sendGuard())
                return false;

            TcpClient? client = _client;
            if (client?.Connected != true)
                return false;

            NetworkStream stream = client.GetStream();
            await stream.WriteAsync(data, token).ConfigureAwait(false);
            return true;
        }
        finally
        {
            _sendLock.Release();
        }
    }

    // === Config Sending (C# -> Bridge for HUD display) ===

    private void HandleBridgeCapabilities(string json)
    {
        try
        {
            JsonNode? node = JsonNode.Parse(json);
            if (!string.Equals(node?["type"]?.GetValue<string>(), "capabilities", StringComparison.OrdinalIgnoreCase))
                return;

            BridgeCapabilities fallback = BridgeCapabilities.ForVersionFallback(InjectedVersion);
            BridgeCapabilities parsed = BridgeCapabilities.FromPayload(node, fallback);
            Capabilities = parsed;
            Log($"Bridge capabilities updated (modules={parsed.ModuleCount}, settings={parsed.SettingCount}, state={parsed.StateFieldCount}).");
        }
        catch (Exception ex)
        {
            Log($"Error parsing bridge capabilities: {ex.Message}");
        }
    }

    private async Task ConfigSenderLoop(CancellationToken token)
    {
        long lastSentRevision = -1;
        long lastSentAt = 0;
        long cachedRevision = -1;
        byte[]? cachedPayload = null;

        while (!token.IsCancellationRequested && _client?.Connected == true)
        {
            try
            {
                long revision = Volatile.Read(ref _configRevision);
                long now = Environment.TickCount64;
                long elapsedSinceSend = lastSentAt == 0 ? ConfigHeartbeatMs : now - lastSentAt;
                bool revisionChanged = revision != lastSentRevision;
                if (!IsConfigSendDue(
                    revision,
                    lastSentRevision,
                    elapsedSinceSend,
                    ConfigHeartbeatMs))
                {
                    int waitMs = (int)Math.Clamp(
                        ConfigHeartbeatMs - elapsedSinceSend,
                        1,
                        ConfigHeartbeatMs);
                    await _configChangedSignal.WaitAsync(waitMs, token).ConfigureAwait(false);
                    continue;
                }

                if (_suppressConfigPush)
                {
                    await Task.Delay(ConfigChangeCoalesceMs, token).ConfigureAwait(false);
                    continue;
                }

                if (revisionChanged && lastSentAt != 0)
                {
                    await Task.Delay(ConfigChangeCoalesceMs, token).ConfigureAwait(false);
                    while (_configChangedSignal.Wait(0))
                    {
                    }
                    revision = Volatile.Read(ref _configRevision);
                }

                byte[] payload;
                if (ShouldSerializeConfig(
                    revision,
                    cachedRevision,
                    cachedPayload != null))
                {
                    long serializationStarted = Stopwatch.GetTimestamp();
                    var clicker = Clicker.Instance;
                    var ka = clicker.KillAuraSettings;
                    var config = new
                    {
                    type = "config",
                    perfDiagnostics = _transportDiagnostics.Enabled,
                    armed = clicker.IsArmed,
                    clicking = clicker.IsClicking,
                    minCPS = clicker.MinCPS,
                    maxCPS = clicker.MaxCPS,
                    left = clicker.LeftClickEnabled,
                    right = clicker.RightClickEnabled,
                    rightMinCPS = clicker.RightMinCPS,
                    rightMaxCPS = clicker.RightMaxCPS,
                    rightBlock = clicker.RightClickOnlyBlock,
                    breakBlocks = clicker.BreakBlocksEnabled,
                    jitter = clicker.JitterEnabled,
                    clickInChests = clicker.ClickInChests,
                    aimAssist = clicker.AimAssistEnabled,
                    aimAssistFov = clicker.AimAssistFov,
                    aimAssistRange = clicker.AimAssistRange,
                    aimAssistStrength = clicker.AimAssistStrength,
                    triggerbot = clicker.TriggerbotEnabled,
                    killAura = clicker.KillAuraEnabled,
                    killAuraCpsMode = ka.CpsMode,
                    killAuraMode = ka.Mode,
                    killAuraSort = ka.Sort,
                    killAuraAutoBlock = ka.AutoBlock,
                    killAuraAttackTick = ka.AttackTick,
                    killAuraAutoBlockRequirePress = ka.AutoBlockRequirePress,
                    killAuraAutoBlockCps = ka.AutoBlockCps,
                    killAuraAutoBlockRange = ka.AutoBlockRange,
                    killAuraSwingRange = ka.SwingRange,
                    killAuraAttackRange = ka.AttackRange,
                    killAuraFov = ka.Fov,
                    killAuraMinCps = ka.MinCps,
                    killAuraMaxCps = ka.MaxCps,
                    killAuraSwitchDelay = ka.SwitchDelay,
                    killAuraRotations = ka.Rotations,
                    killAuraDeadZone = ka.DeadZoneSize,
                    killAuraMaxTurnSpeed = ka.MaxTurnSpeed,
                    killAuraMinTurnSpeed = ka.MinTurnSpeed,
                    killAuraAcceleration = ka.Acceleration,
                    killAuraDeceleration = ka.Deceleration,
                    killAuraUseOvershoot = ka.UseOvershoot,
                    killAuraOvershootStrength = ka.OvershootStrength,
                    killAuraOvershootRecovery = ka.OvershootRecovery,
                    killAuraNoiseStrength = ka.NoiseStrength,
                    killAuraVisualizeAim = ka.VisualizeAim,
                    killAuraSmoothBack = ka.SmoothBack,
                    killAuraMoveFix = ka.MoveFix,
                    killAuraSmoothing = ka.Smoothing,
                    killAuraRavenSmoothing = ka.RavenSmoothing,
                    killAuraRavenPredictTicks = ka.RavenPredictTicks,
                    killAuraRavenYawRandom = ka.RavenYawRandom,
                    killAuraGrokMaxSkew = ka.GrokMaxSkew,
                    killAuraAngleStep = ka.AngleStep,
                    killAuraThroughWalls = ka.ThroughWalls,
                    killAuraRequirePress = ka.RequirePress,
                    killAuraAllowMining = ka.AllowMining,
                    killAuraWeaponsOnly = ka.WeaponsOnly,
                    killAuraAllowTools = ka.AllowTools,
                    killAuraInventoryCheck = ka.InventoryCheck,
                    killAuraBotCheck = ka.BotCheck,
                    killAuraPlayers = ka.Players,
                    killAuraBosses = ka.Bosses,
                    killAuraMobs = ka.Mobs,
                    killAuraAnimals = ka.Animals,
                    killAuraGolems = ka.Golems,
                    killAuraSilverfish = ka.Silverfish,
                    killAuraTeams = ka.Teams,
                    killAuraShowTarget = ka.ShowTarget,
                    killAuraDebug = ka.DebugLog,
                    killAuraRandomize = ka.Randomize,
                    killAuraRandomizeRange = ka.RandomizeRange,
                    killAuraYRandomize = ka.YRandomizeStrength,
                    killAuraLbHorizontalSpeed = ka.LiquidBounceHorizontalSpeed,
                    killAuraLbVerticalSpeed = ka.LiquidBounceVerticalSpeed,
                    killAuraLbSmooth = ka.LiquidBounceSmoothFactor,
                    killAuraLbPredict = ka.LiquidBouncePredict,
                    killAuraLbPredictSize = ka.LiquidBouncePredictSize,
                    killAuraLbRandomize = ka.LiquidBounceRandomize,
                    killAuraLbRandomRange = ka.LiquidBounceRandomizeRange,
                    killAuraLbHorizontalSearch = ka.LiquidBounceHorizontalSearch,
                    killAuraLbBodyMin = ka.LiquidBounceBodyPointMin,
                    killAuraLbBodyMax = ka.LiquidBounceBodyPointMax,
                    speedBridge = clicker.SpeedBridgeEnabled,
                    speedBridgeBlockOnly = clicker.SpeedBridgeBlockOnly,
                    speedBridgeDelayMs = clicker.SpeedBridgeDelayMs,
                    speedBridgeHoldingShiftOnly = clicker.SpeedBridgeHoldingShiftOnly,
                    speedBridgeLookingDownOnly = clicker.SpeedBridgeLookingDownOnly,
                    gtbHelper = clicker.GtbHelperEnabled,
                    pixelPartyAssist = clicker.PixelPartyAssistEnabled,
                    pixelPartyScanRadius = clicker.PixelPartyScanRadius,
                    pixelPartyAutoLook = clicker.PixelPartyAutoLookEnabled,
                    pixelPartyAutoWalk = clicker.PixelPartyAutoWalkEnabled,
                    gtbHint = clicker.GtbCurrentHint,
                    gtbCount = clicker.GtbMatchCount,
                    gtbPreview = clicker.GtbMatchesPreview,
                    nametags = clicker.NametagsEnabled,
                    nickHiderEnabled = clicker.IsNickHiderActive,
                    nickHiderAlias = NickHiderConfig.NormalizeAlias(clicker.NickHiderAlias),
                    showModuleList = clicker.ShowModuleList,
                    moduleListStyle = ModuleListStyleToIndex(clicker.ModuleListStyle),
                    showLogo = clicker.ShowLogo,
                    guiTheme = clicker.GuiTheme,
                    // The modern bridge only emits partial fast-state packets
                    // after this explicit opt-in. Older loaders therefore keep
                    // receiving complete V1-compatible state documents.
                    supportsStatePatches = true,
                    closestPlayerInfo = clicker.ClosestPlayerInfoEnabled,
                    fightStatus = clicker.FightStatusEnabled,
                    nametagShowHealth = clicker.NametagShowHealth,
                    nametagShowArmor = clicker.NametagShowArmor,
                    nametagShowHeldItem = clicker.NametagShowHeldItem,
                    nametagHideVanilla = clicker.NametagHideVanilla,
                    reloadMappingsNonce = Volatile.Read(ref _reloadMappingsNonce),
                    nametagMaxCount = clicker.NametagMaxCount,
                    chestEsp = clicker.ChestEspEnabled,
                    chestEspMaxCount = clicker.ChestEspMaxCount,
                    chestStealerEnabled = clicker.ChestStealerEnabled,
                    chestStealerDelayMs = clicker.ChestStealerDelayMs,
                    blockEspEnabled = clicker.BlockEspEnabled,
                    blockEspBoxes = clicker.BlockEspBoxes,
                    blockEspTracers = clicker.BlockEspTracers,
                    blockEspHud = clicker.BlockEspHud,
                    blockEspMaxCount = clicker.BlockEspMaxCount,
                    blockEspRange = clicker.BlockEspRange,
                    blockEspBlocks = clicker.BlockEspBlocksSerialized,
                    reachEnabled = clicker.ReachEnabled,
                    reachMin = clicker.ReachMin,
                    reachMax = clicker.ReachMax,
                    reachChance = clicker.ReachChance,
                    velocityEnabled = clicker.VelocityEnabled,
                    velocityHorizontal = clicker.VelocityHorizontal,
                    velocityVertical = clicker.VelocityVertical,
                    velocityChance = clicker.VelocityChance,
                    autoTotemEnabled = clicker.AutoTotemEnabled,
                    autoTotemMode = clicker.AutoTotemMode,
                    autoTotemHealth = clicker.AutoTotemHealth,
                    autoTotemElytra = clicker.AutoTotemElytra,
                    autoTotemDelay = clicker.AutoTotemDelay,
                    autoTotemBehaviorMode = clicker.AutoTotemBehaviorMode,
                    autoRodEnabled = clicker.AutoRodEnabled,
                    autoRodSlotMode = clicker.AutoRodSlotMode,
                    autoRodVerifyForcedSlot = clicker.AutoRodVerifyForcedSlot,
                    autoRodExtensionTicks = clicker.AutoRodExtensionTicks,
                    autoRodHoldToExtend = clicker.AutoRodHoldToExtend,
                    antiDebuffEnabled = clicker.AntiDebuffEnabled,
                    hitDelayFixEnabled = clicker.HitDelayFixEnabled,
                    // Per-module keybinds
                    keybindAutoclicker   = InputHooks.GetModuleKey("autoclicker"),
                    keybindRightClick    = InputHooks.GetModuleKey("rightclick"),
                    keybindJitter        = InputHooks.GetModuleKey("jitter"),
                    keybindClickInChests = InputHooks.GetModuleKey("clickinchests"),
                    keybindBreakBlocks   = InputHooks.GetModuleKey("breakblocks"),
                    keybindAimAssist     = InputHooks.GetModuleKey("aimassist"),
                    keybindTriggerbot    = InputHooks.GetModuleKey("triggerbot"),
                    keybindKillAura      = InputHooks.GetModuleKey("killaura"),
                    keybindSpeedBridge   = InputHooks.GetModuleKey("speedbridge"),
                    keybindGtbHelper     = InputHooks.GetModuleKey("gtbhelper"),
                    keybindNametags      = InputHooks.GetModuleKey("nametags"),
                    keybindClosestPlayer = InputHooks.GetModuleKey("closestplayer"),
                    keybindFightStatus   = InputHooks.GetModuleKey("fightstatus"),
                    keybindChestEsp      = InputHooks.GetModuleKey("chestesp"),
                    keybindChestStealer  = InputHooks.GetModuleKey("cheststealer"),
                    keybindBlockEsp      = InputHooks.GetModuleKey("blockesp"),
                    keybindPixelPartyAssist = InputHooks.GetModuleKey("pixelpartyassist"),
                    keybindAutoRod = InputHooks.GetModuleKey("autorod"),
                    hudEditor = clicker.HudEditorActive,
                    hudLayout = clicker.HudLayout.ToJson()
                    };

                    string json = JsonSerializer.Serialize(config) + "\n";
                    payload = Encoding.UTF8.GetBytes(json);
                    cachedPayload = payload;
                    cachedRevision = revision;
                    _transportDiagnostics.RecordConfigSerialization(
                        json.Length,
                        Stopwatch.GetTimestamp() - serializationStarted);
                }
                else
                {
                    payload = cachedPayload!;
                }

                if (!await SendMessageAsync(payload, token).ConfigureAwait(false))
                    break;
                _transportDiagnostics.RecordConfigSend(payload.Length);
                lastSentRevision = revision;
                lastSentAt = Environment.TickCount64;
                LogTransportDiagnosticsIfReady();
            }
            catch (Exception)
            {
                break;
            }
        }
    }

    internal static bool IsConfigSendDue(
        long revision,
        long lastSentRevision,
        long elapsedSinceSendMs,
        int heartbeatMs)
        => revision != lastSentRevision || elapsedSinceSendMs >= heartbeatMs;

    internal static bool ShouldSerializeConfig(
        long revision,
        long cachedRevision,
        bool hasCachedPayload)
        => !hasCachedPayload || revision != cachedRevision;

    private void EnsureConfigChangeTracking()
    {
        if (Interlocked.Exchange(ref _configChangeTrackingAttached, 1) != 0)
            return;

        Clicker.Instance.PropertyChanged += OnClickerConfigPropertyChanged;
        Clicker.Instance.StateChanged += MarkBridgeConfigDirty;
        InputHooks.OnStateChanged += MarkBridgeConfigDirty;
    }

    private void OnClickerConfigPropertyChanged(
        object? sender,
        PropertyChangedEventArgs eventArgs)
        => MarkBridgeConfigDirty();

    private void MarkBridgeConfigDirty()
    {
        Interlocked.Increment(ref _configRevision);
        try
        {
            _configChangedSignal.Release();
        }
        catch (SemaphoreFullException)
        {
            // A pending signal already represents the latest revision.
        }
    }

    private void LogTransportDiagnosticsIfReady()
    {
        if (_transportDiagnostics.TryTakeSnapshot(
            Stopwatch.GetTimestamp(),
            out ManagedTransportSnapshot snapshot))
        {
            Log(snapshot.ToLogMessage());
        }
    }

    // === ClickGUI Command Handler ===

    private void ApplyInboundHudLayout(JsonNode node)
    {
        HudLayout inbound = HudLayout.FromJson(node);

        // Echo guard: if the inbound layout matches what we already have, do nothing.
        if (inbound.EqualsLayout(Clicker.Instance.HudLayout))
            return;

        // Suppress the config sender loop while we update the layout to avoid
        // an immediate echo back to the bridge.
        _suppressConfigPush = true;
        try
        {
            System.Windows.Application.Current?.Dispatcher.Invoke(() =>
            {
                Clicker.Instance.HudLayout = inbound;
            });

            ProfileManager.SaveProfile(ProfileManager.CreateFromClicker());
        }
        finally
        {
            _suppressConfigPush = false;
        }
    }

    private void HandleBridgeCommand(string json)
    {
        try
        {
            var node = JsonNode.Parse(json);
            string? action = node?["action"]?.GetValue<string>();
            if (action == null) return;

            var clicker = Clicker.Instance;
            switch (action)
            {
                case "toggleExternalGui":
                    var mw = System.Windows.Application.Current?.MainWindow as MainWindow;
                    if (mw != null)
                        mw.Dispatcher.Invoke(mw.ShowControlCenterFromBridge);
                    break;
                case "toggleArmed":
                    clicker.ToggleArmed();
                    break;
                case "toggleLeft":
                    clicker.LeftClickEnabled = !clicker.LeftClickEnabled;
                    break;
                case "toggleRight":
                    clicker.RightClickEnabled = !clicker.RightClickEnabled;
                    break;
                case "toggleJitter":
                    clicker.JitterEnabled = !clicker.JitterEnabled;
                    break;

                case "toggleClickInChests":
                    clicker.ClickInChests = !clicker.ClickInChests;
                    break;
                case "toggleAimAssist":
                    clicker.AimAssistEnabled = !clicker.AimAssistEnabled;
                    break;
                case "toggleTriggerbot":
                    clicker.TriggerbotEnabled = !clicker.TriggerbotEnabled;
                    break;
                case "toggleKillAura":
                    if (clicker.DevMode)
                        clicker.KillAuraEnabled = !clicker.KillAuraEnabled;
                    break;
                case "toggleSpeedBridge":
                    clicker.SpeedBridgeEnabled = !clicker.SpeedBridgeEnabled;
                    break;
                case "toggleSpeedBridgeBlockOnly":
                    clicker.SpeedBridgeBlockOnly = !clicker.SpeedBridgeBlockOnly;
                    break;
                case "toggleSpeedBridgeHoldingShiftOnly":
                    clicker.SpeedBridgeHoldingShiftOnly = !clicker.SpeedBridgeHoldingShiftOnly;
                    break;
                case "toggleSpeedBridgeLookingDownOnly":
                    clicker.SpeedBridgeLookingDownOnly = !clicker.SpeedBridgeLookingDownOnly;
                    break;
                case "toggleGtbHelper":
                    clicker.GtbHelperEnabled = !clicker.GtbHelperEnabled;
                    break;
                case "togglePixelPartyAssist":
                    clicker.PixelPartyAssistEnabled = !clicker.PixelPartyAssistEnabled;
                    break;
                case "toggleNametags":
                    clicker.NametagsEnabled = !clicker.NametagsEnabled;
                    break;
                case "toggleClosestPlayerInfo":
                    clicker.ClosestPlayerInfoEnabled = !clicker.ClosestPlayerInfoEnabled;
                    break;
                case "toggleFightStatus":
                    clicker.FightStatusEnabled = !clicker.FightStatusEnabled;
                    break;
                case "toggleNametagHealth":
                    clicker.NametagShowHealth = !clicker.NametagShowHealth;
                    break;
                case "toggleNametagArmor":
                    clicker.NametagShowArmor = !clicker.NametagShowArmor;
                    break;
                case "toggleNametagHeldItem":
                    clicker.NametagShowHeldItem = !clicker.NametagShowHeldItem;
                    break;
                case "toggleNametagHideVanilla":
                    clicker.NametagHideVanilla = !clicker.NametagHideVanilla;
                    break;
                case "toggleChestEsp":
                    clicker.ChestEspEnabled = !clicker.ChestEspEnabled;
                    break;
                case "toggleChestStealer":
                    clicker.ChestStealerEnabled = !clicker.ChestStealerEnabled;
                    break;
                case "toggleBlockEsp":
                    clicker.BlockEspEnabled = !clicker.BlockEspEnabled;
                    break;
                case "toggleBlockEspBoxes":
                    clicker.BlockEspBoxes = !clicker.BlockEspBoxes;
                    break;
                case "toggleBlockEspTracers":
                    clicker.BlockEspTracers = !clicker.BlockEspTracers;
                    break;
                case "toggleBlockEspHud":
                    clicker.BlockEspHud = !clicker.BlockEspHud;
                    break;
                case "setChestStealerDelayMs":
                    clicker.ChestStealerDelayMs = (int)(node?["value"]?.GetValue<float>() ?? 120f);
                    break;
                case "setKeybind":
                    string? moduleId = node?["module"]?.GetValue<string>();
                    int vkCode = node?["key"]?.GetValue<int>() ?? 0;
                    if (moduleId != null)
                        InputHooks.SetModuleKey(moduleId, vkCode);
                    break;
                case "setMinCPS":
                    float minVal = node?["value"]?.GetValue<float>() ?? 8;
                    clicker.MinCPS = minVal;
                    break;
                case "setMaxCPS":
                    float maxVal = node?["value"]?.GetValue<float>() ?? 12;
                    clicker.MaxCPS = maxVal;
                    break;
                case "setRightMinCPS":
                    float rMinVal = node?["value"]?.GetValue<float>() ?? 10;
                    clicker.RightMinCPS = rMinVal;
                    break;
                case "setRightMaxCPS":
                    float rMaxVal = node?["value"]?.GetValue<float>() ?? 14;
                    clicker.RightMaxCPS = rMaxVal;
                    break;
                case "toggleRightBlockOnly":
                    clicker.RightClickOnlyBlock = !clicker.RightClickOnlyBlock;
                    break;
                case "toggleBreakBlocks":
                    clicker.BreakBlocksEnabled = !clicker.BreakBlocksEnabled;
                    break;
                case "setAimAssistFov":
                    clicker.AimAssistFov = node?["value"]?.GetValue<float>() ?? 30;
                    break;
                case "setAimAssistRange":
                    clicker.AimAssistRange = node?["value"]?.GetValue<float>() ?? 4.5f;
                    break;
                case "setAimAssistStrength":
                    clicker.AimAssistStrength = node?["value"]?.GetValue<int>() ?? 40;
                    break;
                case "setSpeedBridgeDelayMs":
                    clicker.SpeedBridgeDelayMs = (int)(node?["value"]?.GetValue<float>() ?? 85f);
                    break;
                case "toggleReach":
                    clicker.ReachEnabled = !clicker.ReachEnabled;
                    break;
                case "setReachMin":
                    clicker.ReachMin = node?["value"]?.GetValue<float>() ?? 3.0f;
                    break;
                case "setReachMax":
                    clicker.ReachMax = node?["value"]?.GetValue<float>() ?? 6.0f;
                    break;
                case "setReachChance":
                    clicker.ReachChance = (int)(node?["value"]?.GetValue<float>() ?? 100f);
                    break;
                case "toggleVelocity":
                    clicker.VelocityEnabled = !clicker.VelocityEnabled;
                    break;
                case "toggleAutoTotem":
                    clicker.AutoTotemEnabled = !clicker.AutoTotemEnabled;
                    break;
                case "toggleAntiDebuff":
                    clicker.AntiDebuffEnabled = !clicker.AntiDebuffEnabled;
                    break;
                case "setAutoTotemMode":
                    clicker.AutoTotemMode = (int)(node?["value"]?.GetValue<float>() ?? 0f);
                    break;
                case "setAutoTotemHealth":
                    clicker.AutoTotemHealth = (int)(node?["value"]?.GetValue<float>() ?? 10f);
                    break;
                case "toggleAutoTotemElytra":
                    clicker.AutoTotemElytra = !clicker.AutoTotemElytra;
                    break;
                case "setAutoTotemDelay":
                    clicker.AutoTotemDelay = (int)(node?["value"]?.GetValue<float>() ?? 0f);
                    break;
                case "setAutoTotemBehaviorMode":
                    clicker.AutoTotemBehaviorMode = (int)(node?["value"]?.GetValue<float>() ?? 0f);
                    break;
                case "setVelocityHorizontal":
                    clicker.VelocityHorizontal = (int)(node?["value"]?.GetValue<float>() ?? 100f);
                    break;
                case "setVelocityVertical":
                    clicker.VelocityVertical = (int)(node?["value"]?.GetValue<float>() ?? 100f);
                    break;
                case "setVelocityChance":
                    clicker.VelocityChance = (int)(node?["value"]?.GetValue<float>() ?? 100f);
                    break;
            }

            Log($"Bridge command: {action}");
        }
        catch (Exception ex)
        {
            Log($"Error handling bridge command: {ex.Message}");
        }
    }

    private void OnPropertyChanged(string name)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
    }

    private static async Task DisposeCtsWhenDoneAsync(CancellationTokenSource cts, Task? configTask, Task? readTask)
    {
        try
        {
            if (configTask != null)
                await configTask.ConfigureAwait(false);
        }
        catch
        {
        }

        try
        {
            if (readTask != null)
                await readTask.ConfigureAwait(false);
        }
        catch
        {
        }

        cts.Dispose();
    }
}
