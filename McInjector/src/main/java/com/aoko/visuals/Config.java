package com.aoko.visuals;

import java.util.Map;
import java.util.regex.Pattern;

/** Immutable settings snapshot sent by the loader (see the wire contract in asset-format.md). */
final class Config {
    static final Pattern ID = Pattern.compile("[a-z0-9][a-z0-9_-]{0,31}");
    static final Config OFF = new Config(false, "", "", "", false, "", -2, false, "");

    final boolean cosmetics;
    final String cape, hat, back;
    final boolean sky;
    final String preset;
    final int time;                 // -2 preset default, -1 follow server, 0..23999 lock
    final boolean clearWeather;
    final String dir;

    Config(boolean cosmetics, String cape, String hat, String back, boolean sky, String preset, int time,
           boolean clearWeather, String dir) {
        this.cosmetics = cosmetics; this.cape = cape; this.hat = hat; this.back = back; this.sky = sky;
        this.preset = preset; this.time = time; this.clearWeather = clearWeather; this.dir = dir;
    }

    static String id(String v) {
        if (v == null) return "";
        String t = v.trim().toLowerCase(java.util.Locale.ROOT);
        return ID.matcher(t).matches() ? t : "";
    }

    static Config parse(String json) throws Json.ParseException {
        Map<String, Object> m = Json.obj(Json.parse(json));
        if (m == null) throw new Json.ParseException("config is not an object");
        double t = Json.num(m, "time", -2);
        int time = t < -2 ? -2 : (t > 23999 ? 23999 : (int) Math.round(t));
        String dir = Json.str(m, "dir", "");
        if (dir.length() > 1024) dir = "";
        return new Config(Json.bool(m, "cosmetics", false), id(Json.str(m, "cape", "")), id(Json.str(m, "hat", "")),
            id(Json.str(m, "back", "")), Json.bool(m, "sky", false), id(Json.str(m, "preset", "")), time,
            Json.bool(m, "clear", false), dir);
    }

    boolean sameAs(Config o) {
        return o != null && cosmetics == o.cosmetics && sky == o.sky && time == o.time && clearWeather == o.clearWeather
            && cape.equals(o.cape) && hat.equals(o.hat) && back.equals(o.back) && preset.equals(o.preset)
            && dir.equals(o.dir);
    }
}
