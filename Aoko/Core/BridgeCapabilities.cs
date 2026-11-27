using System;
using System.Collections.Generic;
using System.Text.Json.Nodes;

namespace Aoko.Core;

public sealed class BridgeCapabilities
{
    private readonly HashSet<string> _modules;
    private readonly HashSet<string> _settings;
    private readonly HashSet<string> _stateFields;
    private readonly HashSet<string> _actions;

    public int ModuleCount => _modules.Count;
    public int SettingCount => _settings.Count;
    public int StateFieldCount => _stateFields.Count;
    public int ActionCount => _actions.Count;
    public IReadOnlyCollection<string> Modules => _modules;
    public IReadOnlyCollection<string> Settings => _settings;
    public IReadOnlyCollection<string> StateFields => _stateFields;

    /// <summary>
    /// Session/server command names advertised after the "state" array. A payload without the
    /// list inherits the version fallback, which mirrors <c>bridge_capabilities.h</c>.
    /// </summary>
    public IReadOnlyCollection<string> Actions => _actions;

    private BridgeCapabilities(
        HashSet<string> modules,
        HashSet<string> settings,
        HashSet<string> stateFields,
        HashSet<string>? actions = null)
    {
        _modules = modules;
        _settings = settings;
        _stateFields = stateFields;
        _actions = actions ?? new HashSet<string>(StringComparer.OrdinalIgnoreCase);
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
                    "fightstatus",
                    "chestesp",
                    "cheststealer",
                    "refill",
                    "throwpot",
                    "autoheal",
                    "blockesp",
                    "bedplates",
                    "reach",
                    "velocity",
                    "autototem",
                    "autorod",
                    "autotool",
                    "antidebuff",
                    "hitdelayfix",
                    "fireballwarning",
                    "fastplace",
                    "nohurtcam",
                    "sprint",
                    "tnttimer",
                    "wtap",
                    "jumpreset",
                    "hitselect",
                    "trajectories",
                    "murdermystery",
                    "breakprogress",
                    "cosmetics",
                    "sky",
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
                    "fightstatus",
                    "keybindfightstatus",
                    "nametagshowhealth",
                    "nametagshowarmor",
                    "nametagshowhelditem",
                    "nametaghidevanilla",
                    "nametagsrange",
                    "chestesp",
                    "chesteprange",
                    "cheststealerenabled",
                    "cheststealerdelayms",
                    "cheststealermenucheck",
                    "cheststealertitlecheck",
                    "cheststealercustomitemscheck",
                    "cheststealerphysicalcheck",
                    "refillenabled",
                    "refilldelayms",
                    "throwpotenabled",
                    "keybindthrowpot",
                    "autohealenabled",
                    "autohealhealth",
                    "autohealdelayms",
                    "blockespenabled",
                    "blockespboxes",
                    "blockesptracers",
                    "blockesphud",
                    "blockespmaxcount",
                    "blockesprange",
                    "blockespblocks",
                    "keybindblockesp",
                    "bedplatesenabled",
                    "bedplatesshowdistance",
                    "bedplatesrange",
                    "keybindbedplates",
                    "reachenabled",
                    "reachmin",
                    "reachmax",
                    "reachchance",
                    "reachchancemode",
                    "reachonlywhilesprinting",
                    "reachdisableinwater",
                    "reachverticalcheck",
                    "velocityenabled",
                    "velocityhorizontal",
                    "velocityvertical",
                    "velocitychance",
                    "reloadmappingsnonce",
                    "showmodulelist",
                    "moduleliststyle",
                    "showlogo",
                    "guitheme",
                    "keybindautoclicker",
                    "keybindtriggerbot",
                    "keybindspeedbridge",
                    "keybindnametags",
                    "keybindclosestplayer",
                    "keybindchestesp",
                    "keybindcheststealer",
                    "keybindrefill",
                    "autototemenabled",
                    "autototemmode",
                    "autototemhealth",
                    "autototemelytra",
                    "autototemdelay",
                    "autototembehaviormode",
                    "autorodenabled",
                    "autorodslotmode",
                    "autorodverifyforcedslot",
                    "autorodextensionticks",
                    "autorodholdtoextend",
                    "keybindautorod",
                    "autotoolenabled",
                    "autotoolswapweapon",
                    "autotoolinstantswap",
                    "autotoolswaptodelay",
                    "autotoolswapback",
                    "autotoolswapbackdelay",
                    "autotoolrequiremousedown",
                    "autotoolonlysneaking",
                    "autotoolbedwarsmode",
                    "keybindautotool",
                    "antidebuffenabled",
                    "hitdelayfixenabled",
                    "fireballwarning",
                    "fireballwarningbox",
                    "fireballwarningarrow",
                    "fireballwarningsound",
                    "keybindfireballwarning",
                    "fastplace",
                    "keybindfastplace",
                    "nohurtcam",
                    "keybindnohurtcam",
                    "sprint",
                    "keybindsprint",
                    "tnttimer",
                    "keybindtnttimer",
                    "wtap",
                    "keybindwtap",
                    "jumpreset",
                    "keybindjumpreset",
                    "hitselect",
                    "keybindhitselect",
                    "trajectories",
                    "keybindtrajectories",
                    "murdermystery",
                    "keybindmurdermystery",
                    "breakprogress",
                    "keybindbreakprogress",
                    "jumpresetchance",
                    "pixelpartyassist",
                    "pixelpartyscanradius",
                    "pixelpartyautolook",
                    "pixelpartyautowalk",
                    "cosmeticsenabled",
                    "cosmeticscape",
                    "cosmeticshat",
                    "cosmeticsback",
                    "skyenabled",
                    "skypreset",
                    "skytime",
                    "skyclearweather",
                    "visualsdir",
                    "hudeditor"
                ),
                BuildSet(
                    "inworld",
                    "actionbar",
                    "holdingblock",
                    "lookingatblock",
                    "fov",
                    "viewportwidth",
                    "viewportheight",
                    "lookingatentity",
                    "lookingatentitylatched",
                    "breakingblock",
                    "lookingatentityhurttime",
                    "modulestatus",
                    "attackcooldown",
                    "attackcooldownpertick",
                    "killauraunavailablereason",
                    "killaurahastarget",
                    "killaurablocking",
                    "lastactionid",
                    "lastactionresult",
                    "statems",
                    "cheststealerstate",
                    "refillstate",
                    "pixelpartytargetfound",
                    "pixelpartytargetyaw",
                    "pixelpartytargetdist",
                    "pixelpartyyawdelta",
                    "hudlayout",
                    "visualsstatus"
                ),
                BuildBridgeActionSet()
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
                "fightstatus",
                "chestesp",
                "cheststealer",
                "refill",
                "throwpot",
                "autoheal",
                "blockesp",
                "bedplates",
                "reach",
                "velocity",
                "autorod",
                "autotool",
                "antidebuff",
                "hitdelayfix",
                "fireballwarning",
                "fastplace",
                "nohurtcam",
                "sprint",
                "tnttimer",
                "wtap",
                "jumpreset",
                "hitselect",
                "trajectories",
                "murdermystery",
                "breakprogress",
                "cosmetics",
                "sky",
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
                "fightstatus",
                "keybindfightstatus",
                "nametagshowhealth",
                "nametagshowarmor",
                "nametagshowhelditem",
                "nametaghidevanilla",
                "nametagsrange",
                "chestesp",
                "chesteprange",
                "cheststealerenabled",
                "cheststealerdelayms",
                "cheststealermenucheck",
                "cheststealertitlecheck",
                "cheststealercustomitemscheck",
                "cheststealerphysicalcheck",
                "refillenabled",
                "refilldelayms",
                "throwpotenabled",
                "keybindthrowpot",
                "autohealenabled",
                "autohealhealth",
                "autohealdelayms",
                "blockespenabled",
                "blockespboxes",
                "blockesptracers",
                "blockesphud",
                "blockespmaxcount",
                "blockesprange",
                "blockespblocks",
                "bedplatesenabled",
                "bedplatesshowdistance",
                "bedplatesrange",
                "keybindbedplates",
                "reachenabled",
                "reachmin",
                "reachmax",
                "reachchance",
                "reachchancemode",
                "reachonlywhilesprinting",
                "reachdisableinwater",
                "reachverticalcheck",
                "velocityenabled",
                "velocityhorizontal",
                "velocityvertical",
                "velocitychance",
                "autorodenabled",
                "autorodslotmode",
                "autorodverifyforcedslot",
                "autorodextensionticks",
                "autorodholdtoextend",
                "keybindautorod",
                "autotoolenabled",
                "autotoolswapweapon",
                "autotoolinstantswap",
                "autotoolswaptodelay",
                "autotoolswapback",
                "autotoolswapbackdelay",
                "autotoolrequiremousedown",
                "autotoolonlysneaking",
                "autotoolbedwarsmode",
                "keybindautotool",
                "antidebuffenabled",
                "hitdelayfixenabled",
                "fireballwarning",
                "fireballwarningbox",
                "fireballwarningarrow",
                "fireballwarningsound",
                "keybindfireballwarning",
                "fastplace",
                "keybindfastplace",
                "nohurtcam",
                "keybindnohurtcam",
                "sprint",
                "keybindsprint",
                "tnttimer",
                "keybindtnttimer",
                "wtap",
                "keybindwtap",
                "jumpreset",
                "keybindjumpreset",
                "hitselect",
                "keybindhitselect",
                "trajectories",
                "keybindtrajectories",
                "murdermystery",
                "keybindmurdermystery",
                "breakprogress",
                "keybindbreakprogress",
                "jumpresetchance",
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
                "keybindrefill",
                "keybindblockesp",
                "pixelpartyassist",
                "pixelpartyscanradius",
                "pixelpartyautolook",
                "pixelpartyautowalk",
                "cosmeticsenabled",
                "cosmeticscape",
                "cosmeticshat",
                "cosmeticsback",
                "skyenabled",
                "skypreset",
                "skytime",
                "skyclearweather",
                "visualsdir",
                "hudeditor"
            ),
            BuildSet(
                "inworld",
                "actionbar",
                "holdingblock",
                "lookingatblock",
                "lookingatentity",
                "lookingatentitylatched",
                "breakingblock",
                "lookingatentityhurttime",
                "modulestatus",
                "attackcooldown",
                "attackcooldownpertick",
                "killauraunavailablereason",
                "killaurahastarget",
                "killaurablocking",
                "lastactionid",
                "lastactionresult",
                "statems",
                "cheststealerstate",
                "refillstate",
                "pitch",
                "health",
                "posx",
                "posy",
                "posz",
                "pixelpartytargetfound",
                "pixelpartytargetyaw",
                "pixelpartytargetdist",
                "pixelpartyyawdelta",
                "hudlayout",
                "visualsstatus"
            ),
            BuildBridgeActionSet()
        );
    }

    /// <summary>
    /// The session/server commands both shipped bridges advertise in
    /// <c>bridge_capabilities.h</c>. The fallback is used between connect and the capabilities
    /// frame (and for a bridge whose frame never arrives), so leaving actions out told the
    /// user to rebuild the DLLs for a feature the bridge supports.
    /// </summary>
    private static HashSet<string> BuildBridgeActionSet() => BuildSet(
        BridgeActions.AccountGetSession,
        BridgeActions.AccountSetSession,
        BridgeActions.AccountRestoreSession,
        BridgeActions.ServerJoin);

    public static BridgeCapabilities FromPayload(JsonNode? node, BridgeCapabilities fallback)
    {
        var modules = ParseStringArray(node?["modules"]);
        var settings = ParseStringArray(node?["settings"]);
        var stateFields = ParseStringArray(node?["state"]);
        var actions = ParseStringArray(node?["actions"]);

        if (modules.Count == 0 && settings.Count == 0 && stateFields.Count == 0 && actions.Count == 0)
            return fallback;

        if (modules.Count == 0) modules = new HashSet<string>(fallback._modules, StringComparer.OrdinalIgnoreCase);
        if (settings.Count == 0) settings = new HashSet<string>(fallback._settings, StringComparer.OrdinalIgnoreCase);
        if (stateFields.Count == 0) stateFields = new HashSet<string>(fallback._stateFields, StringComparer.OrdinalIgnoreCase);
        // A payload from a bridge that predates the actions list must not report the session
        // and join commands as unsupported: both shipped bridges advertise them, so the
        // fallback set is the better answer than "rebuild the bridge DLLs".
        if (actions.Count == 0) actions = new HashSet<string>(fallback._actions, StringComparer.OrdinalIgnoreCase);

        return new BridgeCapabilities(modules, settings, stateFields, actions);
    }

    public bool SupportsModule(string moduleId)
        => _modules.Contains(Normalize(moduleId));

    public bool SupportsSetting(string settingName)
        => _settings.Contains(Normalize(settingName));

    public bool SupportsStateField(string fieldName)
        => _stateFields.Contains(Normalize(fieldName));

    /// <summary>
    /// Whether the connected bridge advertises a session/server command such as
    /// <c>account.setSession</c> or <c>server.join</c>. Actions are not modules,
    /// settings, or state fields.
    /// </summary>
    public bool SupportsAction(string actionName)
        => _actions.Contains(Normalize(actionName));

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
            // A non-string element means a malformed payload; skip it instead of throwing
            // out of the capability parse and leaving the previous set in place.
            if (item is not JsonValue value || !value.TryGetValue<string>(out string? raw))
                continue;

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
