#pragma once

namespace lc {

inline const char* LegacyCapabilitiesJson()
{
    return
        "{\"type\":\"capabilities\","
        "\"modules\":[\"autoclicker\",\"rightclick\",\"jitter\",\"clickinchests\",\"breakblocks\",\"aimassist\",\"speedbridge\",\"gtbhelper\",\"nametags\",\"closestplayer\",\"chestesp\",\"reach\",\"velocity\"],"
        "\"settings\":[\"mincps\",\"maxcps\",\"left\",\"right\",\"rightmincps\",\"rightmaxcps\",\"rightblock\",\"breakblocks\",\"jitter\",\"clickinchests\",\"aimassistfov\",\"aimassistrange\",\"aimassiststrength\",\"speedbridge\",\"speedbridgeblockonly\",\"speedbridgedelayms\",\"speedbridgeholdingshiftonly\",\"speedbridgelookingdownonly\",\"nametags\",\"closestplayerinfo\",\"nametagshowhealth\",\"nametagshowarmor\",\"nametaghidevanilla\",\"nametagmaxcount\",\"chestesp\",\"chestespmaxcount\",\"reachenabled\",\"reachmin\",\"reachmax\",\"reachchance\",\"velocityenabled\",\"velocityhorizontal\",\"velocityvertical\",\"velocitychance\",\"gtbhint\",\"gtbcount\",\"gtbpreview\",\"showmodulelist\",\"moduleliststyle\",\"showlogo\",\"guitheme\",\"keybindautoclicker\",\"keybindspeedbridge\",\"keybindnametags\",\"keybindclosestplayer\",\"keybindchestesp\"],"
        "\"state\":[\"actionbar\",\"holdingblock\",\"lookingatblock\",\"lookingatentity\",\"lookingatentitylatched\",\"breakingblock\",\"attackcooldown\",\"attackcooldownpertick\",\"statems\",\"pitch\"]}\n";
}

inline const char* ModernCapabilitiesJson()
{
    return
        "{\"type\":\"capabilities\","
        "\"modules\":[\"autoclicker\",\"rightclick\",\"jitter\",\"clickinchests\",\"breakblocks\",\"aimassist\",\"triggerbot\",\"speedbridge\",\"gtbhelper\",\"nametags\",\"closestplayer\",\"chestesp\",\"reach\",\"velocity\",\"autototem\"],"
        "\"settings\":[\"mincps\",\"maxcps\",\"left\",\"right\",\"rightmincps\",\"rightmaxcps\",\"rightblock\",\"breakblocks\",\"jitter\",\"clickinchests\",\"triggerbot\",\"speedbridge\",\"speedbridgeblockonly\",\"speedbridgedelayms\",\"speedbridgeholdingshiftonly\",\"speedbridgelookingdownonly\",\"gtbhint\",\"gtbcount\",\"gtbpreview\",\"nametags\",\"closestplayerinfo\",\"nametagshowhealth\",\"nametagshowarmor\",\"nametaghidevanilla\",\"nametagmaxcount\",\"chestesp\",\"chestespmaxcount\",\"reachenabled\",\"reachmin\",\"reachmax\",\"reachchance\",\"velocityenabled\",\"velocityhorizontal\",\"velocityvertical\",\"velocitychance\",\"reloadmappingsnonce\",\"showmodulelist\",\"moduleliststyle\",\"showlogo\",\"guitheme\",\"autototemenabled\",\"autototemmode\",\"autototemhealth\",\"autototemelytra\",\"autototemexplosion\",\"autototemfall\",\"autototemdelay\"],"
        "\"state\":[\"actionbar\",\"holdingblock\",\"lookingatblock\",\"lookingatentity\",\"lookingatentitylatched\",\"breakingblock\",\"attackcooldown\",\"attackcooldownpertick\",\"statems\"]}\n";
}

} // namespace lc
