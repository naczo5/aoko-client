#pragma once

namespace lc {

inline const char* LegacyCapabilitiesJson()
{
    return
        "{\"type\":\"capabilities\","
        "\"modules\":[\"autoclicker\",\"rightclick\",\"jitter\",\"clickinchests\",\"breakblocks\",\"aimassist\",\"speedbridge\",\"gtbhelper\",\"nametags\",\"closestplayer\",\"chestesp\",\"cheststealer\",\"blockesp\",\"reach\",\"velocity\",\"antidebuff\",\"pixelpartyassist\",\"hudeditor\"],"
        "\"settings\":[\"mincps\",\"maxcps\",\"left\",\"right\",\"rightmincps\",\"rightmaxcps\",\"rightblock\",\"breakblocks\",\"jitter\",\"clickinchests\",\"aimassistfov\",\"aimassistrange\",\"aimassiststrength\",\"speedbridge\",\"speedbridgeblockonly\",\"speedbridgedelayms\",\"speedbridgeholdingshiftonly\",\"speedbridgelookingdownonly\",\"nametags\",\"closestplayerinfo\",\"nametagshowhealth\",\"nametagshowarmor\",\"nametaghidevanilla\",\"nametagmaxcount\",\"chestesp\",\"chestespmaxcount\",\"cheststealerenabled\",\"cheststealerdelayms\",\"blockespenabled\",\"blockespboxes\",\"blockesptracers\",\"blockesphud\",\"blockespmaxcount\",\"blockesprange\",\"blockespblocks\",\"keybindblockesp\",\"reachenabled\",\"reachmin\",\"reachmax\",\"reachchance\",\"velocityenabled\",\"velocityhorizontal\",\"velocityvertical\",\"velocitychance\",\"antidebuffenabled\",\"gtbhint\",\"gtbcount\",\"gtbpreview\",\"showmodulelist\",\"moduleliststyle\",\"showlogo\",\"guitheme\",\"keybindautoclicker\",\"keybindspeedbridge\",\"keybindnametags\",\"keybindclosestplayer\",\"keybindchestesp\",\"keybindcheststealer\",\"pixelpartyassist\",\"pixelpartyscanradius\",\"pixelpartyautolook\",\"pixelpartyautowalk\",\"hudeditor\"],"
        "\"state\":[\"actionbar\",\"holdingblock\",\"lookingatblock\",\"lookingatentity\",\"lookingatentitylatched\",\"breakingblock\",\"attackcooldown\",\"attackcooldownpertick\",\"statems\",\"cheststealerstate\",\"pitch\",\"pixelpartytargetfound\",\"pixelpartytargetyaw\",\"pixelpartytargetdist\",\"pixelpartyyawdelta\",\"hudlayout\"]}\n";
}

inline const char* ModernCapabilitiesJson()
{
    return
        "{\"type\":\"capabilities\","
        "\"modules\":[\"autoclicker\",\"rightclick\",\"jitter\",\"clickinchests\",\"breakblocks\",\"aimassist\",\"triggerbot\",\"speedbridge\",\"gtbhelper\",\"nametags\",\"closestplayer\",\"chestesp\",\"cheststealer\",\"blockesp\",\"reach\",\"velocity\",\"autototem\",\"antidebuff\",\"pixelpartyassist\",\"hudeditor\"],"
        "\"settings\":[\"mincps\",\"maxcps\",\"left\",\"right\",\"rightmincps\",\"rightmaxcps\",\"rightblock\",\"breakblocks\",\"jitter\",\"clickinchests\",\"triggerbot\",\"speedbridge\",\"speedbridgeblockonly\",\"speedbridgedelayms\",\"speedbridgeholdingshiftonly\",\"speedbridgelookingdownonly\",\"gtbhint\",\"gtbcount\",\"gtbpreview\",\"nametags\",\"closestplayerinfo\",\"nametagshowhealth\",\"nametagshowarmor\",\"nametaghidevanilla\",\"nametagmaxcount\",\"chestesp\",\"chestespmaxcount\",\"cheststealerenabled\",\"cheststealerdelayms\",\"blockespenabled\",\"blockespboxes\",\"blockesptracers\",\"blockesphud\",\"blockespmaxcount\",\"blockesprange\",\"blockespblocks\",\"keybindblockesp\",\"reachenabled\",\"reachmin\",\"reachmax\",\"reachchance\",\"velocityenabled\",\"velocityhorizontal\",\"velocityvertical\",\"velocitychance\",\"reloadmappingsnonce\",\"showmodulelist\",\"moduleliststyle\",\"showlogo\",\"guitheme\",\"autototemenabled\",\"autototemmode\",\"autototemhealth\",\"autototemelytra\",\"autototemexplosion\",\"autototemfall\",\"autototemdelay\",\"antidebuffenabled\",\"pixelpartyassist\",\"pixelpartyscanradius\",\"pixelpartyautolook\",\"pixelpartyautowalk\",\"hudeditor\"],"
        "\"state\":[\"actionbar\",\"holdingblock\",\"lookingatblock\",\"lookingatentity\",\"lookingatentitylatched\",\"breakingblock\",\"attackcooldown\",\"attackcooldownpertick\",\"statems\",\"cheststealerstate\",\"pixelpartytargetfound\",\"pixelpartytargetyaw\",\"pixelpartytargetdist\",\"pixelpartyyawdelta\",\"hudlayout\"]}\n";
}

} // namespace lc
