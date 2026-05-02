#pragma once

namespace lc {

inline const char* LegacyCapabilitiesJson()
{
    return
        "{\"type\":\"capabilities\","
        "\"modules\":[\"autoclicker\",\"rightclick\",\"jitter\",\"clickinchests\",\"breakblocks\",\"aimassist\",\"gtbhelper\",\"nametags\",\"closestplayer\",\"chestesp\",\"reach\",\"velocity\"],"
        "\"settings\":[\"mincps\",\"maxcps\",\"left\",\"right\",\"rightmincps\",\"rightmaxcps\",\"rightblock\",\"breakblocks\",\"jitter\",\"clickinchests\",\"aimassistfov\",\"aimassistrange\",\"aimassiststrength\",\"nametags\",\"closestplayerinfo\",\"nametagshowhealth\",\"nametagshowarmor\",\"nametaghidevanilla\",\"nametagmaxcount\",\"chestesp\",\"chestespmaxcount\",\"reachenabled\",\"reachmin\",\"reachmax\",\"reachchance\",\"velocityenabled\",\"velocityhorizontal\",\"velocityvertical\",\"velocitychance\",\"gtbhint\",\"gtbcount\",\"gtbpreview\",\"showmodulelist\",\"moduleliststyle\",\"showlogo\",\"guitheme\",\"keybindautoclicker\",\"keybindnametags\",\"keybindclosestplayer\",\"keybindchestesp\"],"
        "\"state\":[\"actionbar\",\"holdingblock\",\"lookingatblock\",\"lookingatentity\",\"lookingatentitylatched\",\"breakingblock\",\"attackcooldown\",\"attackcooldownpertick\",\"statems\"]}\n";
}

inline const char* ModernCapabilitiesJson()
{
    return
        "{\"type\":\"capabilities\","
        "\"modules\":[\"autoclicker\",\"rightclick\",\"jitter\",\"clickinchests\",\"breakblocks\",\"aimassist\",\"triggerbot\",\"gtbhelper\",\"nametags\",\"closestplayer\",\"chestesp\",\"reach\",\"velocity\",\"autototem\"],"
        "\"settings\":[\"mincps\",\"maxcps\",\"left\",\"right\",\"rightmincps\",\"rightmaxcps\",\"rightblock\",\"breakblocks\",\"jitter\",\"clickinchests\",\"triggerbot\",\"gtbhint\",\"gtbcount\",\"gtbpreview\",\"nametags\",\"closestplayerinfo\",\"nametagshowhealth\",\"nametagshowarmor\",\"nametaghidevanilla\",\"nametagmaxcount\",\"chestesp\",\"chestespmaxcount\",\"reachenabled\",\"reachmin\",\"reachmax\",\"reachchance\",\"velocityenabled\",\"velocityhorizontal\",\"velocityvertical\",\"velocitychance\",\"reloadmappingsnonce\",\"showmodulelist\",\"moduleliststyle\",\"showlogo\",\"guitheme\",\"autototemenabled\",\"autototemmode\",\"autototemhealth\",\"autototemelytra\",\"autototemexplosion\",\"autototemfall\",\"autototemdelay\"],"
        "\"state\":[\"actionbar\",\"holdingblock\",\"lookingatblock\",\"lookingatentity\",\"lookingatentitylatched\",\"breakingblock\",\"attackcooldown\",\"attackcooldownpertick\",\"statems\"]}\n";
}

} // namespace lc
