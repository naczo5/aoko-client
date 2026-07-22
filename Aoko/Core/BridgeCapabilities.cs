using System;
using System.Collections.Generic;
using System.Text.Json.Nodes;

namespace Aoko.Core;

public sealed class BridgeCapabilities
{
    private readonly HashSet<string> _modules;
    private readonly HashSet<string> _settings;
    private readonly HashSet<string> _stateFields;

    public int ModuleCount => _modules.Count;
    public int SettingCount => _settings.Count;
    public int StateFieldCount => _stateFields.Count;
    public IReadOnlyCollection<string> Modules => _modules;
    public IReadOnlyCollection<string> Settings => _settings;
    public IReadOnlyCollection<string> StateFields => _stateFields;

    private BridgeCapabilities(HashSet<string> modules, HashSet<string> settings, HashSet<string> stateFields)
    {
        _modules = modules;
        _settings = settings;
        _stateFields = stateFields;
    }

    public static BridgeCapabilities ForVersionFallback(string? injectedVersion)
    {
        bool is121 = !string.IsNullOrWhiteSpace(injectedVersion)
            && injectedVersion.StartsWith("1.21", StringComparison.OrdinalIgnoreCase);
        bool is261 = !string.IsNullOrWhiteSpace(injectedVersion)
            && injectedVersion.StartsWith("26.", StringComparison.OrdinalIgnoreCase);

        if (is121 || is261)
        {
            return new BridgeCapabilities(
                BuildSet(
                    "autoclicker",
                    "rightclick",
                    "jitter",
                    "clickinchests",
                    "breakblocks",
                    "aimassist",
                    "triggerbot",
                    "killaura",
                    "speedbridge",
                    "gtbhelper",
                    "nametags",
                    "nickhider",
                    "closestplayer",
                    "chestesp",
                    "cheststealer",
                    "blockesp",
                    "reach",
                    "velocity",
                    "autototem",
                    "antidebuff",
                    "hitdelayfix",
                    "pixelpartyassist",
                    "hudeditor"
                ),                BuildSet(
                    "mincps",
                    "maxcps",
                    "left",
                    "right",
                    "rightmincps",
                    "rightmaxcps",
                    "rightblock",
                    "breakblocks",
                    "jitter",
                    "clickinchests",
                    "aimassistfov",
                    "aimassistrange",
                    "aimassiststrength",
                    "triggerbot",
                    "killaura",
                    "killaurarange",
                    "killauraaimrange",
                    "killauraminturnspeed",
                    "killauramaxturnspeed",
                    "killauratargetmode",
                    "killauraswitchdelayms",
                    "killaurarandomization",
                    "killauraclickmode",
                    "killauramincps",
                    "killauramaxcps",
                    "killaurafov",
                    "killaurarotmode",
                    "killaurarequirepress",
                    "killauraweaponsonly",
                    "speedbridge",
                    "speedbridgeblockonly",
                    "speedbridgedelayms",
                    "speedbridgeholdingshiftonly",
                    "speedbridgelookingdownonly",
                    "gtbhint",
                    "gtbcount",
                    "gtbpreview",
                    "nametags",
                    "nickhiderenabled",
                    "nickhideralias",
                    "closestplayerinfo",
                    "nametagshowhealth",
                    "nametagshowarmor",
                    "nametagshowhelditem",
                    "nametaghidevanilla",
                    "nametagmaxcount",
                    "chestesp",
                    "chestespmaxcount",
                    "cheststealerenabled",
                    "cheststealerdelayms",
                    "blockespenabled",
                    "blockespboxes",
                    "blockesptracers",
                    "blockesphud",
                    "blockespmaxcount",
                    "blockesprange",
                    "blockespblocks",
                    "keybindblockesp",
                    "reachenabled",
                    "reachmin",
                    "reachmax",
                    "reachchance",
                    "velocityenabled",
                    "velocityhorizontal",
                    "velocityvertical",
                    "velocitychance",
                    "reloadmappingsnonce",
                    "showmodulelist",
                    "moduleliststyle",
                    "showlogo",
                    "guitheme",
                    "autototemenabled",
                    "autototemmode",
                    "autototemhealth",
                    "autototemelytra",
                    "autototemdelay",
                    "autototembehaviormode",
                    "antidebuffenabled",
                    "hitdelayfixenabled",
                    "pixelpartyassist",
                    "pixelpartyscanradius",
                    "pixelpartyautolook",
                    "pixelpartyautowalk",
                    "hudeditor"
                ),
                BuildSet(
                    "actionbar",
                    "lookingatentity",
                    "lookingatentitylatched",
                    "breakingblock",
                    "attackcooldown",
                    "attackcooldownpertick",
                    "killauraunavailablereason",
                    "killaurahastarget",
                    "killaurablocking",
                    "statems",
                    "cheststealerstate",
                    "pixelpartytargetfound",
                    "pixelpartytargetyaw",
                    "pixelpartytargetdist",
                    "pixelpartyyawdelta",
                    "hudlayout"
                )
            );
        }

        return new BridgeCapabilities(
            BuildSet(
                "autoclicker",
                "rightclick",
                "jitter",
                "clickinchests",
                "breakblocks",
                "aimassist",
                "killaura",
                "speedbridge",
                "gtbhelper",
                "nametags",
                "nickhider",
                "closestplayer",
                "chestesp",
                "cheststealer",
                "blockesp",
                "reach",
                "velocity",
                "antidebuff",
                "hitdelayfix",
                "pixelpartyassist",
                "hudeditor"
            ),
            BuildSet(
                "mincps",
                "maxcps",
                "left",
                "right",
                "rightmincps",
                "rightmaxcps",
                "rightblock",
                "breakblocks",
                "jitter",
                "clickinchests",
                "aimassistfov",
                "aimassistrange",
                "aimassiststrength",
                "killaura",
                "killaurarange",
                "killauraaimrange",
                "killauraminturnspeed",
                "killauramaxturnspeed",
                "killauratargetmode",
                "killauraswitchdelayms",
                "killaurarandomization",
                "killauraclickmode",
                "killauramincps",
                "killauramaxcps",
                "killaurafov",
                "killaurarotmode",
                "killaurarequirepress",
                "killauraweaponsonly",
                "speedbridge",
                "speedbridgeblockonly",
                "speedbridgedelayms",
                "speedbridgeholdingshiftonly",
                "speedbridgelookingdownonly",
                "nametags",
                "nickhiderenabled",
                "nickhideralias",
                "closestplayerinfo",
                "nametagshowhealth",
                "nametagshowarmor",
                "nametaghidevanilla",
                "chestesp",
                "cheststealerenabled",
                "cheststealerdelayms",
                "blockespenabled",
                "blockespboxes",
                "blockesptracers",
                "blockesphud",
                "blockespmaxcount",
                "blockesprange",
                "blockespblocks",
                "reachenabled",
                "reachmin",
                "reachmax",
                "reachchance",
                "velocityenabled",
                "velocityhorizontal",
                "velocityvertical",
                "velocitychance",
                "antidebuffenabled",
                "hitdelayfixenabled",
                "showmodulelist",
                "moduleliststyle",
                "showlogo",
                "guitheme",
                "keybindautoclicker",
                "keybindspeedbridge",
                "keybindnametags",
                "keybindclosestplayer",
                "keybindchestesp",
                "keybindcheststealer",
                "keybindblockesp",
                "pixelpartyassist",
                "pixelpartyscanradius",
                "pixelpartyautolook",
                "pixelpartyautowalk",
                "hudeditor"
            ),
            BuildSet(
                "actionbar",
                "holdingblock",
                "lookingatblock",
                "lookingatentity",
                "lookingatentitylatched",
                "breakingblock",
                "attackcooldown",
                "attackcooldownpertick",
                "killauraunavailablereason",
                "killaurahastarget",
                "killaurablocking",
                "statems",
                "cheststealerstate",
                "pitch",
                "pixelpartytargetfound",
                "pixelpartytargetyaw",
                "pixelpartytargetdist",
                "pixelpartyyawdelta",
                "hudlayout"
            )
        );
    }

    public static BridgeCapabilities FromPayload(JsonNode? node, BridgeCapabilities fallback)
    {
        var modules = ParseStringArray(node?["modules"]);
        var settings = ParseStringArray(node?["settings"]);
        var stateFields = ParseStringArray(node?["state"]);

        if (modules.Count == 0 && settings.Count == 0 && stateFields.Count == 0)
            return fallback;

        if (modules.Count == 0) modules = new HashSet<string>(fallback._modules, StringComparer.OrdinalIgnoreCase);
        if (settings.Count == 0) settings = new HashSet<string>(fallback._settings, StringComparer.OrdinalIgnoreCase);
        if (stateFields.Count == 0) stateFields = new HashSet<string>(fallback._stateFields, StringComparer.OrdinalIgnoreCase);

        return new BridgeCapabilities(modules, settings, stateFields);
    }

    public bool SupportsModule(string moduleId)
        => _modules.Contains(Normalize(moduleId));

    public bool SupportsSetting(string settingName)
        => _settings.Contains(Normalize(settingName));

    public bool SupportsStateField(string fieldName)
        => _stateFields.Contains(Normalize(fieldName));

    private static HashSet<string> BuildSet(params string[] values)
    {
        var set = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (string value in values)
        {
            string normalized = Normalize(value);
            if (!string.IsNullOrWhiteSpace(normalized))
                set.Add(normalized);
        }
        return set;
    }

    private static HashSet<string> ParseStringArray(JsonNode? node)
    {
        var set = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (node is not JsonArray arr) return set;

        foreach (JsonNode? item in arr)
        {
            string? raw = item?.GetValue<string>();
            string normalized = Normalize(raw);
            if (!string.IsNullOrWhiteSpace(normalized))
                set.Add(normalized);
        }

        return set;
    }

    private static string Normalize(string? value)
        => string.IsNullOrWhiteSpace(value)
            ? string.Empty
            : value.Trim().ToLowerInvariant();
}
