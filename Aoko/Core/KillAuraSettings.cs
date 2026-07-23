using System;
using System.ComponentModel;
using System.Runtime.CompilerServices;

namespace Aoko.Core;

/// <summary>
/// OpenMyau-Plus KillAura configuration, mirrored from commit fc7c95d.
/// Mode strings intentionally follow the upstream order/names; several upstream
/// controls are currently inert and are preserved as data for behavior parity.
/// </summary>
public sealed class KillAuraSettings : INotifyPropertyChanged
{
    public event PropertyChangedEventHandler? PropertyChanged;
    public event Action? Changed;

    private string _cpsMode = "normal";
    private string _mode = "switch";
    private string _sort = "health";
    private string _autoBlock = "hypixel";
    private int _attackTick;
    private bool _autoBlockRequirePress;
    private float _autoBlockCps = 8f;
    private float _autoBlockRange = 6f;
    private float _swingRange = 3.5f;
    private float _attackRange = 3f;
    private int _fov = 360;
    private int _minCps = 14;
    private int _maxCps = 14;
    private int _switchDelay = 150;
    private string _rotations = "silent";
    private float _deadZoneSize = 0.5f;
    private float _maxTurnSpeed = 25f;
    private float _minTurnSpeed = 5f;
    private float _acceleration = 2.5f;
    private float _deceleration = 1.5f;
    private bool _useOvershoot = true;
    private float _overshootStrength = 5f;
    private float _overshootRecovery = 0.2f;
    private float _noiseStrength = 0.2f;
    private bool _visualizeAim = true;
    private bool _smoothBack = true;
    private string _moveFix = "silent";
    private int _smoothing;
    private int _ravenSmoothing;
    private int _ravenPredictTicks;
    private int _ravenYawRandom;
    private float _grokMaxSkew = 12f;
    private int _angleStep = 90;
    private bool _throughWalls = true;
    private bool _requirePress;
    private bool _allowMining = true;
    private bool _weaponsOnly = true;
    private bool _allowTools;
    private bool _inventoryCheck = true;
    private bool _botCheck = true;
    private bool _players = true;
    private bool _bosses;
    private bool _mobs;
    private bool _animals;
    private bool _golems;
    private bool _silverfish;
    private bool _teams = true;
    private string _showTarget = "none";
    private string _debugLog = "none";
    private bool _randomize = true;
    private float _randomizeRange = 0.4f;
    private float _yRandomizeStrength = 0.3f;
    private float _liquidBounceHorizontalSpeed = 180f;
    private float _liquidBounceVerticalSpeed = 180f;
    private float _liquidBounceSmoothFactor = 0.5f;
    private bool _liquidBouncePredict = true;
    private float _liquidBouncePredictSize = 1f;
    private bool _liquidBounceRandomize = true;
    private float _liquidBounceRandomizeRange = 0.5f;
    private float _liquidBounceHorizontalSearch = 0.5f;
    private float _liquidBounceBodyPointMin = 0.1f;
    private float _liquidBounceBodyPointMax = 0.9f;

    public string CpsMode { get => _cpsMode; set => SetMode(ref _cpsMode, value, "normal", "record"); }
    public string Mode { get => _mode; set => SetMode(ref _mode, value, "single", "switch"); }
    public string Sort { get => _sort; set => SetMode(ref _sort, value, "distance", "health", "hurttime", "fov"); }
    public string AutoBlock { get => _autoBlock; set { if (SetMode(ref _autoBlock, value, "none", "vanilla", "spoof", "hypixel", "blink", "interact", "swap", "legit", "fake", "morden")) NotifyConditions(); } }
    public int AttackTick { get => _attackTick; set => Set(ref _attackTick, Math.Clamp(value, 0, 5)); }
    public bool AutoBlockRequirePress { get => _autoBlockRequirePress; set => Set(ref _autoBlockRequirePress, value); }
    public float AutoBlockCps { get => _autoBlockCps; set => Set(ref _autoBlockCps, Math.Clamp(value, 1f, 10f)); }
    public float AutoBlockRange { get => _autoBlockRange; set => Set(ref _autoBlockRange, Math.Clamp(value, 3f, 8f)); }
    public float SwingRange { get => _swingRange; set => Set(ref _swingRange, Math.Clamp(value, 3f, 6f)); }
    public float AttackRange { get => _attackRange; set => Set(ref _attackRange, Math.Clamp(value, 3f, 6f)); }
    public int Fov { get => _fov; set => Set(ref _fov, Math.Clamp(value, 30, 360)); }
    public int MinCps { get => _minCps; set { int v = Math.Clamp(value, 1, 20); if (Set(ref _minCps, v) && _maxCps < v) MaxCps = v; } }
    public int MaxCps { get => _maxCps; set { int v = Math.Clamp(value, 1, 20); if (Set(ref _maxCps, v) && _minCps > v) MinCps = v; } }
    public int SwitchDelay { get => _switchDelay; set => Set(ref _switchDelay, Math.Clamp(value, 0, 1000)); }
    public string Rotations { get => _rotations; set { if (SetMode(ref _rotations, value, "none", "legit", "silent", "lockview", "liquidbounce", "hypixel", "grok")) NotifyConditions(); } }
    public float DeadZoneSize { get => _deadZoneSize; set => Set(ref _deadZoneSize, Math.Clamp(value, 0f, 2f)); }
    public float MaxTurnSpeed { get => _maxTurnSpeed; set => Set(ref _maxTurnSpeed, Math.Clamp(value, 5f, 180f)); }
    public float MinTurnSpeed { get => _minTurnSpeed; set => Set(ref _minTurnSpeed, Math.Clamp(value, 1f, 90f)); }
    public float Acceleration { get => _acceleration; set => Set(ref _acceleration, Math.Clamp(value, 0.1f, 10f)); }
    public float Deceleration { get => _deceleration; set => Set(ref _deceleration, Math.Clamp(value, 0.1f, 10f)); }
    public bool UseOvershoot { get => _useOvershoot; set { if (Set(ref _useOvershoot, value)) NotifyConditions(); } }
    public float OvershootStrength { get => _overshootStrength; set => Set(ref _overshootStrength, Math.Clamp(value, 0f, 20f)); }
    public float OvershootRecovery { get => _overshootRecovery; set => Set(ref _overshootRecovery, Math.Clamp(value, 0.01f, 1f)); }
    public float NoiseStrength { get => _noiseStrength; set => Set(ref _noiseStrength, Math.Clamp(value, 0f, 2f)); }
    public bool VisualizeAim { get => _visualizeAim; set => Set(ref _visualizeAim, value); }
    public bool SmoothBack { get => _smoothBack; set => Set(ref _smoothBack, value); }
    public string MoveFix { get => _moveFix; set => SetMode(ref _moveFix, value, "none", "silent", "strict"); }
    public int Smoothing { get => _smoothing; set => Set(ref _smoothing, Math.Clamp(value, 0, 100)); }
    public int RavenSmoothing { get => _ravenSmoothing; set => Set(ref _ravenSmoothing, Math.Clamp(value, 0, 10)); }
    public int RavenPredictTicks { get => _ravenPredictTicks; set => Set(ref _ravenPredictTicks, Math.Clamp(value, 0, 5)); }
    public int RavenYawRandom { get => _ravenYawRandom; set => Set(ref _ravenYawRandom, Math.Clamp(value, 0, 5)); }
    public float GrokMaxSkew { get => _grokMaxSkew; set => Set(ref _grokMaxSkew, Math.Clamp(value, 6f, 25f)); }
    public int AngleStep { get => _angleStep; set => Set(ref _angleStep, Math.Clamp(value, 30, 180)); }
    public bool ThroughWalls { get => _throughWalls; set => Set(ref _throughWalls, value); }
    public bool RequirePress { get => _requirePress; set => Set(ref _requirePress, value); }
    public bool AllowMining { get => _allowMining; set => Set(ref _allowMining, value); }
    public bool WeaponsOnly { get => _weaponsOnly; set { if (Set(ref _weaponsOnly, value)) NotifyConditions(); } }
    public bool AllowTools { get => _allowTools; set => Set(ref _allowTools, value); }
    public bool InventoryCheck { get => _inventoryCheck; set => Set(ref _inventoryCheck, value); }
    public bool BotCheck { get => _botCheck; set => Set(ref _botCheck, value); }
    public bool Players { get => _players; set => Set(ref _players, value); }
    public bool Bosses { get => _bosses; set => Set(ref _bosses, value); }
    public bool Mobs { get => _mobs; set => Set(ref _mobs, value); }
    public bool Animals { get => _animals; set => Set(ref _animals, value); }
    public bool Golems { get => _golems; set => Set(ref _golems, value); }
    public bool Silverfish { get => _silverfish; set => Set(ref _silverfish, value); }
    public bool Teams { get => _teams; set => Set(ref _teams, value); }
    public string ShowTarget { get => _showTarget; set => SetMode(ref _showTarget, value, "none", "default"); }
    public string DebugLog { get => _debugLog; set => SetMode(ref _debugLog, value, "none", "health"); }
    public bool Randomize { get => _randomize; set { if (Set(ref _randomize, value)) NotifyConditions(); } }
    public float RandomizeRange { get => _randomizeRange; set => Set(ref _randomizeRange, Math.Clamp(value, 0f, 1f)); }
    public float YRandomizeStrength { get => _yRandomizeStrength; set => Set(ref _yRandomizeStrength, Math.Clamp(value, 0f, 1f)); }
    public float LiquidBounceHorizontalSpeed { get => _liquidBounceHorizontalSpeed; set => Set(ref _liquidBounceHorizontalSpeed, Math.Clamp(value, 1f, 180f)); }
    public float LiquidBounceVerticalSpeed { get => _liquidBounceVerticalSpeed; set => Set(ref _liquidBounceVerticalSpeed, Math.Clamp(value, 1f, 180f)); }
    public float LiquidBounceSmoothFactor { get => _liquidBounceSmoothFactor; set => Set(ref _liquidBounceSmoothFactor, Math.Clamp(value, 0.1f, 1f)); }
    public bool LiquidBouncePredict { get => _liquidBouncePredict; set { if (Set(ref _liquidBouncePredict, value)) NotifyConditions(); } }
    public float LiquidBouncePredictSize { get => _liquidBouncePredictSize; set => Set(ref _liquidBouncePredictSize, Math.Clamp(value, 0f, 3f)); }
    public bool LiquidBounceRandomize { get => _liquidBounceRandomize; set { if (Set(ref _liquidBounceRandomize, value)) NotifyConditions(); } }
    public float LiquidBounceRandomizeRange { get => _liquidBounceRandomizeRange; set => Set(ref _liquidBounceRandomizeRange, Math.Clamp(value, 0f, 1f)); }
    public float LiquidBounceHorizontalSearch { get => _liquidBounceHorizontalSearch; set => Set(ref _liquidBounceHorizontalSearch, Math.Clamp(value, 0f, 1f)); }
    public float LiquidBounceBodyPointMin { get => _liquidBounceBodyPointMin; set => Set(ref _liquidBounceBodyPointMin, Math.Clamp(value, 0f, 1f)); }
    public float LiquidBounceBodyPointMax { get => _liquidBounceBodyPointMax; set => Set(ref _liquidBounceBodyPointMax, Math.Clamp(value, 0f, 1f)); }

    public bool IsLiquidBounce => Rotations == "liquidbounce";
    public bool IsHypixelRotation => Rotations == "hypixel";
    public bool IsGrokRotation => Rotations == "grok";
    public bool IsSwapAutoBlock => AutoBlock == "swap";
    public bool OvershootControlsEnabled => IsLiquidBounce && UseOvershoot;
    public bool RandomizeControlsEnabled => IsLiquidBounce && Randomize;
    public bool PredictControlsEnabled => IsLiquidBounce && LiquidBouncePredict;
    public bool LiquidRandomizeControlsEnabled => IsLiquidBounce && LiquidBounceRandomize;
    public bool AllowToolsEnabled => WeaponsOnly;

    public KillAuraSettings Clone()
    {
        var clone = new KillAuraSettings();
        clone.CopyFrom(this);
        return clone;
    }

    public void CopyFrom(KillAuraSettings? source)
    {
        if (source == null) return;
        foreach (var property in typeof(KillAuraSettings).GetProperties())
        {
            if (property.CanRead && property.CanWrite)
                property.SetValue(this, property.GetValue(source));
        }
    }

    private bool SetMode(ref string field, string? value, params string[] allowed)
    {
        string normalized = value?.Trim().ToLowerInvariant() ?? allowed[0];
        if (Array.IndexOf(allowed, normalized) < 0) normalized = allowed[0];
        return Set(ref field, normalized);
    }

    private bool Set<T>(ref T field, T value, [CallerMemberName] string? name = null)
    {
        if (Equals(field, value)) return false;
        field = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
        Changed?.Invoke();
        return true;
    }

    private void NotifyConditions()
    {
        foreach (string name in new[] { nameof(IsLiquidBounce), nameof(IsHypixelRotation), nameof(IsGrokRotation), nameof(IsSwapAutoBlock), nameof(OvershootControlsEnabled), nameof(RandomizeControlsEnabled), nameof(PredictControlsEnabled), nameof(LiquidRandomizeControlsEnabled), nameof(AllowToolsEnabled) })
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
    }
}
