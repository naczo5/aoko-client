using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;

namespace Aoko.Core;

public class Profile
{
    public string Name { get; set; } = "Default";
    public string GuiTheme { get; set; } = "Slate";
    public string ModuleListStyle { get; set; } = "Default";
    public bool ShowLogo { get; set; } = true;
    public bool DiscordRpcEnabled { get; set; } = true;
    public bool IsArmed { get; set; } = false;
    public float MinCPS { get; set; } = 8.0f;
    public float MaxCPS { get; set; } = 12.0f;
    public bool LeftClickEnabled { get; set; } = true;
    
    public bool RightClickEnabled { get; set; } = false;
    public float RightMinCPS { get; set; } = 10.0f;
    public float RightMaxCPS { get; set; } = 14.0f;
    public bool RightClickOnlyBlock { get; set; } = false;
    public bool BreakBlocksEnabled { get; set; } = false;
    
    public bool JitterEnabled { get; set; } = true;
    public bool ClickInChests { get; set; } = false;
    public bool AimAssistEnabled { get; set; } = false;
    public float AimAssistFov { get; set; } = 30.0f;
    public float AimAssistRange { get; set; } = 4.5f;
    public int AimAssistStrength { get; set; } = 40;
    public bool TriggerbotEnabled { get; set; } = false;
    public bool TriggerbotOnlyCrosshair { get; set; } = true;
    public bool TriggerbotOnlyIfCanAttack { get; set; } = true;
    public int TriggerbotCooldownThreshold { get; set; } = 92;
    public int TriggerbotHitChance { get; set; } = 100;
    public bool TriggerbotRequireClick { get; set; } = true;
    public bool SilentAuraEnabled { get; set; } = false;
    public float SilentAuraRange { get; set; } = 3.0f;
    public float SilentAuraAimRange { get; set; } = 4.0f;
    public float SilentAuraRotSpeed { get; set; } = 35.0f;
    public string SilentAuraTargetMode { get; set; } = "distance";
    public int SilentAuraSwitchDelayMs { get; set; } = 400;
    public int SilentAuraAccuracy { get; set; } = 90;
    public bool SilentAuraSpamMode { get; set; } = true;
    public float SilentAuraSpamMinCps { get; set; } = 14.0f;
    public float SilentAuraSpamMaxCps { get; set; } = 18.0f;
    public bool SpeedBridgeEnabled { get; set; } = false;
    public bool SpeedBridgeBlockOnly { get; set; } = true;
    public int SpeedBridgeDelayMs { get; set; } = 85;
    public bool SpeedBridgeHoldingShiftOnly { get; set; } = true;
    public bool SpeedBridgeLookingDownOnly { get; set; } = true;
    public bool GtbHelperEnabled { get; set; } = false;
    public bool PixelPartyAssistEnabled { get; set; } = false;
    public int PixelPartyScanRadius { get; set; } = 28;
    public bool PixelPartyAutoLookEnabled { get; set; } = false;
    public bool PixelPartyAutoWalkEnabled { get; set; } = false;
    public bool NametagsEnabled { get; set; } = false;
    public bool ShowModuleList { get; set; } = true;
    public bool ClosestPlayerInfoEnabled { get; set; } = false;
    public bool NametagShowHealth { get; set; } = true;
    public bool NametagShowArmor { get; set; } = true;
    public bool NametagShowHeldItem { get; set; } = true;
    public bool NametagHideVanilla { get; set; } = false;
    public int NametagMaxCount { get; set; } = 8;

    public bool ChestEspEnabled { get; set; } = false;
    public int ChestEspMaxCount { get; set; } = 5;
    public bool ChestStealerEnabled { get; set; } = false;
    public int ChestStealerDelayMs { get; set; } = 120;

    public bool BlockEspEnabled { get; set; } = false;
    public bool BlockEspBoxes { get; set; } = true;
    public bool BlockEspTracers { get; set; } = false;
    public bool BlockEspHud { get; set; } = true;
    public int BlockEspMaxCount { get; set; } = 64;
    public int BlockEspRange { get; set; } = 4;
    public List<BlockEspTargetData> BlockEspTargets { get; set; } = BlockEspTargetData.BuildDefaults();

    public bool ReachEnabled { get; set; } = false;
    public float ReachMin { get; set; } = 3.0f;
    public float ReachMax { get; set; } = 3.0f;
    public int ReachChance { get; set; } = 100;

    public bool VelocityEnabled { get; set; } = false;
    public int VelocityHorizontal { get; set; } = 100;
    public int VelocityVertical { get; set; } = 100;
    public int VelocityChance { get; set; } = 100;

    public bool AutoTotemEnabled { get; set; } = false;
    public int AutoTotemMode { get; set; } = 0;
    public int AutoTotemHealth { get; set; } = 10;
    public bool AutoTotemElytra { get; set; } = true;
    public int AutoTotemDelay { get; set; } = 0;
    public int AutoTotemBehaviorMode { get; set; } = 0;

    public bool AntiDebuffEnabled { get; set; } = false;
    public bool HitDelayFixEnabled { get; set; } = false;

    // Nullable so older JSON (without hudLayout) deserializes without error;
    // a null value is treated as canonical defaults in ApplyToClicker.
    public Dictionary<string, HudElementLayout>? HudLayout { get; set; }

    public Dictionary<string, int> ModuleKeys { get; set; } = new()
    {
        ["autoclicker"]   = 0xC0,
        ["rightclick"]    = 0,
        ["jitter"]        = 0,
        ["clickinchests"] = 0,
        ["breakblocks"]   = 0,
        ["aimassist"]     = 0,
        ["triggerbot"]    = 0,
        ["silentaura"]    = 0,
        ["speedbridge"]   = 0,
        ["gtbhelper"]     = 0,
        ["pixelpartyassist"] = 0,
        ["nametags"]      = 0,
        ["closestplayer"] = 0,
        ["chestesp"]      = 0,
        ["cheststealer"]  = 0,
        ["blockesp"]      = 0,
        ["reach"]         = 0,
        ["velocity"]      = 0,
        ["autototem"]     = 0,
        ["panic"]         = 0,
        ["hudeditor"]     = 0,
    };
    public string Theme { get; set; } = "Slate";
}

public static class ProfileManager
{
    private const string LegacyAppFolder = "Lego" + "Clicker";
    private static readonly string ProfilesDir;
    private static readonly JsonSerializerOptions JsonOptions;
    
    static ProfileManager()
    {
        string appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        ProfilesDir = InitializeProfileDirectory(appData);
        
        JsonOptions = new JsonSerializerOptions
        {
            WriteIndented = true,
            PropertyNamingPolicy = JsonNamingPolicy.CamelCase
        };
    }

    internal static string GetProfilesDirectory(string appData)
        => Path.Combine(appData, "Aoko", "profiles");

    internal static string GetLegacyProfilesDirectory(string appData)
        => Path.Combine(appData, LegacyAppFolder, "profiles");

    internal static string InitializeProfileDirectory(string appData)
    {
        string profilesDir = GetProfilesDirectory(appData);
        string legacyProfilesDir = GetLegacyProfilesDirectory(appData);

        try
        {
            Directory.CreateDirectory(profilesDir);
            CopyLegacyProfilesIfNeeded(profilesDir, legacyProfilesDir);
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }

        return profilesDir;
    }

    internal static void CopyLegacyProfilesIfNeeded(string profilesDir, string legacyProfilesDir)
    {
        try
        {
            if (!Directory.Exists(legacyProfilesDir))
                return;

            if (Directory.Exists(profilesDir) && Directory.GetFiles(profilesDir, "*.json").Length > 0)
                return;

            Directory.CreateDirectory(profilesDir);

            foreach (string legacyFile in Directory.GetFiles(legacyProfilesDir, "*.json"))
            {
                string targetFile = Path.Combine(profilesDir, Path.GetFileName(legacyFile));
                if (!File.Exists(targetFile))
                    File.Copy(legacyFile, targetFile, overwrite: false);
            }
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }
    
    public static List<string> GetProfileNames()
    {
        var names = new List<string>();
        
        if (Directory.Exists(ProfilesDir))
        {
            foreach (var file in Directory.GetFiles(ProfilesDir, "*.json"))
            {
                names.Add(Path.GetFileNameWithoutExtension(file));
            }
        }
        
        if (names.Count == 0)
            names.Add("Default");
        
        return names;
    }

    /// <summary>
    /// Name of the internal auto-save slot used to persist the live working state
    /// on close / restore it on launch. It is intentionally hidden from the user
    /// facing config list.
    /// </summary>
    public const string AutoSaveConfigName = "config";

    /// <summary>Absolute path of the folder that stores config (.json) files.</summary>
    public static string ConfigsDirectory => ProfilesDir;

    /// <summary>
    /// Returns the user-facing config names (every <c>*.json</c> file except the
    /// internal auto-save slot), sorted case-insensitively. Unlike
    /// <see cref="GetProfileNames"/> this never injects a synthetic "Default" entry.
    /// </summary>
    public static List<string> GetConfigNames()
    {
        var names = new List<string>();

        if (Directory.Exists(ProfilesDir))
        {
            foreach (var file in Directory.GetFiles(ProfilesDir, "*.json"))
            {
                string name = Path.GetFileNameWithoutExtension(file);
                if (string.Equals(name, AutoSaveConfigName, StringComparison.OrdinalIgnoreCase))
                    continue;
                names.Add(name);
            }
        }

        names.Sort(StringComparer.OrdinalIgnoreCase);
        return names;
    }

    /// <summary>Returns true if a config with the given (raw) name already exists.</summary>
    public static bool ConfigExists(string name)
    {
        string sanitized = SanitizeConfigName(name);
        if (sanitized.Length == 0)
            return false;
        return File.Exists(Path.Combine(ProfilesDir, $"{sanitized}.json"));
    }

    /// <summary>
    /// Produces a safe file-name stem from user input: trims whitespace, strips any
    /// path separators / invalid filename characters, and prevents traversal. Returns
    /// an empty string when nothing valid remains (caller should reject it).
    /// </summary>
    public static string SanitizeConfigName(string? name)
    {
        if (string.IsNullOrWhiteSpace(name))
            return string.Empty;

        string trimmed = name.Trim();

        // Drop anything that looks like a path component.
        trimmed = trimmed.Replace('\\', '_').Replace('/', '_');

        var sb = new System.Text.StringBuilder(trimmed.Length);
        foreach (char c in trimmed)
        {
            if (Array.IndexOf(Path.GetInvalidFileNameChars(), c) >= 0)
                continue;
            sb.Append(c);
        }

        string cleaned = sb.ToString().Trim().TrimEnd('.');

        // Reject reserved traversal stems.
        if (cleaned == "." || cleaned == "..")
            return string.Empty;

        // Keep file names reasonable.
        if (cleaned.Length > 64)
            cleaned = cleaned.Substring(0, 64);

        return cleaned;
    }
    
    public static void SaveProfile(Profile profile)
    {
        string filePath = Path.Combine(ProfilesDir, $"{profile.Name}.json");
        string json = JsonSerializer.Serialize(profile, JsonOptions);
        File.WriteAllText(filePath, json);
    }
    
    public static Profile? LoadProfile(string name)
    {
        string filePath = Path.Combine(ProfilesDir, $"{name}.json");
        
        if (!File.Exists(filePath))
            return null;
        
        try
        {
            string json = File.ReadAllText(filePath);
            return JsonSerializer.Deserialize<Profile>(json, JsonOptions);
        }
        catch
        {
            return null;
        }
    }
    
    public static void DeleteProfile(string name)
    {
        string filePath = Path.Combine(ProfilesDir, $"{name}.json");
        if (File.Exists(filePath))
            File.Delete(filePath);
    }
    
    public static Profile CreateFromClicker()
    {
        var clicker = Clicker.Instance;
        return new Profile
        {
            IsArmed = clicker.IsArmed,
            MinCPS = clicker.MinCPS,
            MaxCPS = clicker.MaxCPS,
            LeftClickEnabled = clicker.LeftClickEnabled,
            GuiTheme = clicker.GuiTheme,
            ModuleListStyle = clicker.ModuleListStyle,
            ShowLogo = clicker.ShowLogo,
            DiscordRpcEnabled = clicker.DiscordRpcEnabled,
            
            RightClickEnabled = clicker.RightClickEnabled,
            RightMinCPS = clicker.RightMinCPS,
            RightMaxCPS = clicker.RightMaxCPS,
            RightClickOnlyBlock = clicker.RightClickOnlyBlock,
            BreakBlocksEnabled = clicker.BreakBlocksEnabled,
            
            JitterEnabled = clicker.JitterEnabled,
            ClickInChests = clicker.ClickInChests,
            AimAssistEnabled = clicker.AimAssistEnabled,
            AimAssistFov = clicker.AimAssistFov,
            AimAssistRange = clicker.AimAssistRange,
            AimAssistStrength = clicker.AimAssistStrength,
            TriggerbotEnabled = clicker.TriggerbotEnabled,
            TriggerbotOnlyCrosshair = clicker.TriggerbotOnlyCrosshair,
            TriggerbotOnlyIfCanAttack = clicker.TriggerbotOnlyIfCanAttack,
            TriggerbotCooldownThreshold = clicker.TriggerbotCooldownThreshold,
            TriggerbotHitChance = clicker.TriggerbotHitChance,
            TriggerbotRequireClick = clicker.TriggerbotRequireClick,
            SilentAuraEnabled = clicker.SilentAuraEnabled,
            SilentAuraRange = clicker.SilentAuraRange,
            SilentAuraAimRange = clicker.SilentAuraAimRange,
            SilentAuraRotSpeed = clicker.SilentAuraRotSpeed,
            SilentAuraTargetMode = clicker.SilentAuraTargetMode,
            SilentAuraSwitchDelayMs = clicker.SilentAuraSwitchDelayMs,
            SilentAuraAccuracy = clicker.SilentAuraAccuracy,
            SilentAuraSpamMode = clicker.SilentAuraSpamMode,
            SilentAuraSpamMinCps = clicker.SilentAuraSpamMinCps,
            SilentAuraSpamMaxCps = clicker.SilentAuraSpamMaxCps,
            SpeedBridgeEnabled = clicker.SpeedBridgeEnabled,
            SpeedBridgeBlockOnly = clicker.SpeedBridgeBlockOnly,
            SpeedBridgeDelayMs = clicker.SpeedBridgeDelayMs,
            SpeedBridgeHoldingShiftOnly = clicker.SpeedBridgeHoldingShiftOnly,
            SpeedBridgeLookingDownOnly = clicker.SpeedBridgeLookingDownOnly,
            GtbHelperEnabled = clicker.GtbHelperEnabled,
            PixelPartyAssistEnabled = clicker.PixelPartyAssistEnabled,
            PixelPartyScanRadius = clicker.PixelPartyScanRadius,
            PixelPartyAutoLookEnabled = clicker.PixelPartyAutoLookEnabled,
            PixelPartyAutoWalkEnabled = clicker.PixelPartyAutoWalkEnabled,
            NametagsEnabled = clicker.NametagsEnabled,
            ShowModuleList = clicker.ShowModuleList,
            ClosestPlayerInfoEnabled = clicker.ClosestPlayerInfoEnabled,
            NametagShowHealth = clicker.NametagShowHealth,
            NametagShowArmor = clicker.NametagShowArmor,
            NametagShowHeldItem = clicker.NametagShowHeldItem,
            NametagHideVanilla = clicker.NametagHideVanilla,
            NametagMaxCount = clicker.NametagMaxCount,
            ChestEspEnabled = clicker.ChestEspEnabled,
            ChestEspMaxCount = clicker.ChestEspMaxCount,
            ChestStealerEnabled = clicker.ChestStealerEnabled,
            ChestStealerDelayMs = clicker.ChestStealerDelayMs,

            BlockEspEnabled = clicker.BlockEspEnabled,
            BlockEspBoxes = clicker.BlockEspBoxes,
            BlockEspTracers = clicker.BlockEspTracers,
            BlockEspHud = clicker.BlockEspHud,
            BlockEspMaxCount = clicker.BlockEspMaxCount,
            BlockEspRange = clicker.BlockEspRange,
            BlockEspTargets = BuildBlockEspTargetData(clicker.BlockEspTargets),

            ReachEnabled = clicker.ReachEnabled,
            ReachMin = clicker.ReachMin,
            ReachMax = clicker.ReachMax,
            ReachChance = clicker.ReachChance,

            VelocityEnabled = clicker.VelocityEnabled,
            VelocityHorizontal = clicker.VelocityHorizontal,
            VelocityVertical = clicker.VelocityVertical,
            VelocityChance = clicker.VelocityChance,

            AutoTotemEnabled = clicker.AutoTotemEnabled,
            AutoTotemMode = clicker.AutoTotemMode,
            AutoTotemHealth = clicker.AutoTotemHealth,
            AutoTotemElytra = clicker.AutoTotemElytra,
            AutoTotemDelay = clicker.AutoTotemDelay,
            AutoTotemBehaviorMode = clicker.AutoTotemBehaviorMode,

            AntiDebuffEnabled = clicker.AntiDebuffEnabled,
            HitDelayFixEnabled = clicker.HitDelayFixEnabled,

            HudLayout = BuildHudLayoutDict(clicker.HudLayout),

            ModuleKeys = new Dictionary<string, int>(InputHooks.ModuleKeys),
            Theme = ThemeManager.CurrentTheme
        };
    }
    
    public static void ApplyToClicker(Profile profile)
    {
        var clicker = Clicker.Instance;

        // Panic can force-disable left click for stealth in-memory; never persist that lockout.
        if (!profile.LeftClickEnabled)
            profile.LeftClickEnabled = true;

        if (profile.IsArmed)
            clicker.Arm();
        else
            clicker.Disarm();

        clicker.MinCPS = profile.MinCPS;
        clicker.MaxCPS = profile.MaxCPS;
        clicker.LeftClickEnabled = profile.LeftClickEnabled;
        if (profile.GuiTheme != null) clicker.GuiTheme = profile.GuiTheme;
        if (profile.ModuleListStyle != null) clicker.ModuleListStyle = profile.ModuleListStyle;
        clicker.ShowLogo = profile.ShowLogo;
        clicker.DiscordRpcEnabled = profile.DiscordRpcEnabled;
        
        clicker.RightClickEnabled = profile.RightClickEnabled;
        clicker.RightMinCPS = profile.RightMinCPS;
        clicker.RightMaxCPS = profile.RightMaxCPS;
        clicker.RightClickOnlyBlock = profile.RightClickOnlyBlock;
        clicker.BreakBlocksEnabled = profile.BreakBlocksEnabled;
        
        clicker.JitterEnabled = profile.JitterEnabled;
        clicker.ClickInChests = profile.ClickInChests;
        clicker.AimAssistEnabled = profile.AimAssistEnabled;
        clicker.AimAssistFov = profile.AimAssistFov;
        clicker.AimAssistRange = profile.AimAssistRange;
        clicker.AimAssistStrength = profile.AimAssistStrength;
        clicker.TriggerbotEnabled = profile.TriggerbotEnabled;
        clicker.TriggerbotOnlyCrosshair = profile.TriggerbotOnlyCrosshair;
        clicker.TriggerbotOnlyIfCanAttack = profile.TriggerbotOnlyIfCanAttack;
        clicker.TriggerbotCooldownThreshold = profile.TriggerbotCooldownThreshold;
        clicker.TriggerbotHitChance = profile.TriggerbotHitChance;
        clicker.TriggerbotRequireClick = profile.TriggerbotRequireClick;
        clicker.SilentAuraEnabled = profile.SilentAuraEnabled;
        clicker.SilentAuraRange = profile.SilentAuraRange;
        clicker.SilentAuraAimRange = profile.SilentAuraAimRange;
        clicker.SilentAuraRotSpeed = profile.SilentAuraRotSpeed;
        clicker.SilentAuraTargetMode = profile.SilentAuraTargetMode;
        clicker.SilentAuraSwitchDelayMs = profile.SilentAuraSwitchDelayMs;
        clicker.SilentAuraAccuracy = profile.SilentAuraAccuracy;
        clicker.SilentAuraSpamMode = profile.SilentAuraSpamMode;
        clicker.SilentAuraSpamMinCps = profile.SilentAuraSpamMinCps;
        clicker.SilentAuraSpamMaxCps = profile.SilentAuraSpamMaxCps;
        clicker.SpeedBridgeEnabled = profile.SpeedBridgeEnabled;
        clicker.SpeedBridgeBlockOnly = profile.SpeedBridgeBlockOnly;
        clicker.SpeedBridgeDelayMs = profile.SpeedBridgeDelayMs;
        clicker.SpeedBridgeHoldingShiftOnly = profile.SpeedBridgeHoldingShiftOnly;
        clicker.SpeedBridgeLookingDownOnly = profile.SpeedBridgeLookingDownOnly;
        clicker.GtbHelperEnabled = profile.GtbHelperEnabled;
        clicker.PixelPartyAssistEnabled = profile.PixelPartyAssistEnabled;
        clicker.PixelPartyScanRadius = profile.PixelPartyScanRadius;
        clicker.PixelPartyAutoLookEnabled = profile.PixelPartyAutoLookEnabled;
        clicker.PixelPartyAutoWalkEnabled = profile.PixelPartyAutoWalkEnabled;
        clicker.NametagsEnabled = profile.NametagsEnabled;
        clicker.ShowModuleList = profile.ShowModuleList;
        clicker.ClosestPlayerInfoEnabled = profile.ClosestPlayerInfoEnabled;
        clicker.NametagShowHealth = profile.NametagShowHealth;
        clicker.NametagShowArmor = profile.NametagShowArmor;
        clicker.NametagShowHeldItem = profile.NametagShowHeldItem;
        clicker.NametagHideVanilla = profile.NametagHideVanilla;
        clicker.NametagMaxCount = profile.NametagMaxCount;
        clicker.ChestEspEnabled = profile.ChestEspEnabled;
        clicker.ChestEspMaxCount = profile.ChestEspMaxCount;
        clicker.ChestStealerEnabled = profile.ChestStealerEnabled;
        clicker.ChestStealerDelayMs = profile.ChestStealerDelayMs;

        clicker.BlockEspEnabled = profile.BlockEspEnabled;
        clicker.BlockEspBoxes = profile.BlockEspBoxes;
        clicker.BlockEspTracers = profile.BlockEspTracers;
        clicker.BlockEspHud = profile.BlockEspHud;
        clicker.BlockEspMaxCount = profile.BlockEspMaxCount;
        clicker.BlockEspRange = profile.BlockEspRange;
        clicker.BlockEspTargets = BuildBlockEspTargets(profile.BlockEspTargets);

        clicker.ReachEnabled = profile.ReachEnabled;
        clicker.ReachMin = profile.ReachMin;
        clicker.ReachMax = profile.ReachMax;
        clicker.ReachChance = profile.ReachChance;

        clicker.VelocityEnabled = profile.VelocityEnabled;
        clicker.VelocityHorizontal = profile.VelocityHorizontal;
        clicker.VelocityVertical = profile.VelocityVertical;
        clicker.VelocityChance = profile.VelocityChance;

        clicker.AutoTotemEnabled = profile.AutoTotemEnabled;
        clicker.AutoTotemMode = profile.AutoTotemMode;
        clicker.AutoTotemHealth = profile.AutoTotemHealth;
        clicker.AutoTotemElytra = profile.AutoTotemElytra;
        clicker.AutoTotemDelay = profile.AutoTotemDelay;
        clicker.AutoTotemBehaviorMode = profile.AutoTotemBehaviorMode;

        clicker.AntiDebuffEnabled = profile.AntiDebuffEnabled;
        clicker.HitDelayFixEnabled = profile.HitDelayFixEnabled;

        clicker.HudLayout = BuildHudLayout(profile.HudLayout);

        foreach (var kvp in profile.ModuleKeys)
            InputHooks.SetModuleKey(kvp.Key, kvp.Value);
        ThemeManager.ApplyTheme(profile.Theme);
    }

    private static List<BlockEspTargetData> BuildBlockEspTargetData(
        System.Collections.ObjectModel.ObservableCollection<BlockEspTarget> targets)
    {
        var list = new List<BlockEspTargetData>(targets.Count);
        foreach (BlockEspTarget t in targets)
            list.Add(BlockEspTargetData.FromTarget(t));
        return list;
    }

    private static System.Collections.ObjectModel.ObservableCollection<BlockEspTarget> BuildBlockEspTargets(
        List<BlockEspTargetData>? data)
    {
        if (data == null || data.Count == 0)
            return new System.Collections.ObjectModel.ObservableCollection<BlockEspTarget>(
                BlockEspPresets.BuildDefaultTargets());

        var collection = new System.Collections.ObjectModel.ObservableCollection<BlockEspTarget>();
        foreach (BlockEspTargetData d in data)
        {
            if (d == null || string.IsNullOrWhiteSpace(d.RegistryId))
                continue;
            collection.Add(d.ToTarget());
        }
        if (collection.Count == 0)
            foreach (BlockEspTarget t in BlockEspPresets.BuildDefaultTargets())
                collection.Add(t);
        return collection;
    }

    /// <summary>
    /// Converts a <see cref="HudLayout"/> to a plain dictionary suitable for JSON
    /// serialization inside <see cref="Profile"/>.
    /// </summary>
    private static Dictionary<string, HudElementLayout> BuildHudLayoutDict(HudLayout hudLayout)    {
        var dict = new Dictionary<string, HudElementLayout>();
        foreach (string id in new[]
        {
            HudElementId.ModuleList, HudElementId.ClosestPlayer, HudElementId.PixelParty,
            HudElementId.ChestEspList, HudElementId.GtbHint, HudElementId.Nametags,
            HudElementId.BlockEspList,
        })
        {
            dict[id] = hudLayout.Get(id);
        }
        return dict;
    }

    /// <summary>
    /// Builds a <see cref="HudLayout"/> from the nullable dictionary stored in a
    /// <see cref="Profile"/>. Returns canonical defaults when the dictionary is null
    /// (covers older profiles that predate the hudLayout field).
    /// </summary>
    private static HudLayout BuildHudLayout(Dictionary<string, HudElementLayout>? dict)
    {
        if (dict == null)
            return new HudLayout();

        // Reconstruct via ToJson/FromJson so that clamping and defaults are applied
        // consistently, tolerating any invalid values that may have been stored on disk.
        var jsonObj = new JsonObject();
        foreach (var kv in dict)
        {
            jsonObj[kv.Key] = new JsonObject
            {
                ["x"]     = kv.Value.X,
                ["y"]     = kv.Value.Y,
                ["scale"] = kv.Value.Scale,
            };
        }
        return HudLayout.FromJson(jsonObj);
    }
}
