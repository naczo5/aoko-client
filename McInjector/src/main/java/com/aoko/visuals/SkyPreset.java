package com.aoko.visuals;

import java.util.Map;

/**
 * A sky preset from {@code skies/<id>/sky.json}. Colours follow the sun height
 * {@code h = cos(2 * pi * celestialAngle)} (1 at noon, -1 at midnight, 0 at the
 * horizon), the same convention the vanilla sky uses, so a preset reads the same on
 * 1.8.9 (celestial angle) and 26.x (sun angle in degrees).
 */
final class SkyPreset {
    /** Group order inside a colour set. */
    static final int SKY = 0, FOG = 1, CLOUD = 2, SUNSET = 3;

    final String id;
    final String name;
    final int time;                 // fixed tick, or -1 to follow the world
    final boolean clearWeather;
    final float[][] day;            // [SKY..SUNSET][rgb]
    final float[][] night;
    final float stars;
    final int moonPhase;            // -1 follows the world
    final String skyboxFile;        // null when there is no cube sky
    final float skyboxDay, skyboxNight;
    final float[] skyboxRotation;   // degrees per second around x, y, z

    // Classic fallbacks, close to the vanilla overworld.
    private static final int[] CLASSIC_DAY = { 0x78A7FF, 0xC0D8FF, 0xFFFFFF, 0xFF9A4D };
    private static final int[] CLASSIC_NIGHT = { 0x05060E, 0x0A0A14, 0x1A1A26, 0x5A3A66 };

    private SkyPreset(String id, String name, int time, boolean clear, float[][] day, float[][] night, float stars,
                      int moonPhase, String skyboxFile, float skyboxDay, float skyboxNight, float[] rot) {
        this.id = id; this.name = name; this.time = time; this.clearWeather = clear; this.day = day; this.night = night;
        this.stars = stars; this.moonPhase = moonPhase; this.skyboxFile = skyboxFile;
        this.skyboxDay = skyboxDay; this.skyboxNight = skyboxNight; this.skyboxRotation = rot;
    }

    /** Built-in fallback used when a time lock or weather override is on without a preset. */
    static SkyPreset classic() {
        return new SkyPreset("", "Classic", -1, false, new float[][] {
            rgb(CLASSIC_DAY[0]), rgb(CLASSIC_DAY[1]), rgb(CLASSIC_DAY[2]), rgb(CLASSIC_DAY[3]) },
            new float[][] { rgb(CLASSIC_NIGHT[0]), rgb(CLASSIC_NIGHT[1]), rgb(CLASSIC_NIGHT[2]), rgb(CLASSIC_NIGHT[3]) },
            1f, -1, null, 1f, 1f, new float[3]);
    }

    static SkyPreset parse(String id, String json) throws Json.ParseException {
        Map<String, Object> m = Json.obj(Json.parse(json));
        if (m == null) throw new Json.ParseException("sky.json is not an object");
        String name = Json.str(m, "name", null);
        if (name == null) throw new Json.ParseException("sky.json needs a name");
        if (name.length() > 64) name = name.substring(0, 64);
        int time = -1;
        if (m.containsKey("time")) {
            double t = Json.num(m, "time", -1);
            if (t < 0 || t > 23999) throw new Json.ParseException("time must be 0..23999");
            time = (int) Math.round(t);
        }
        Map<String, Object> colors = Json.obj(m.get("colors"));
        float[][] day = colorSet(Json.obj(colors == null ? null : colors.get("day")), CLASSIC_DAY);
        float[][] night = colorSet(Json.obj(colors == null ? null : colors.get("night")), CLASSIC_NIGHT);
        float stars = (float) Math.max(0, Math.min(2, Json.num(m, "stars", 1)));
        int moon = -1;
        if (m.containsKey("moonPhase")) {
            double mp = Json.num(m, "moonPhase", -1);
            if (mp < 0 || mp > 7) throw new Json.ParseException("moonPhase must be 0..7");
            moon = (int) Math.round(mp);
        }
        String file = null;
        float sbDay = 1, sbNight = 1;
        float[] rot = new float[3];
        Map<String, Object> sb = Json.obj(m.get("skybox"));
        if (sb != null) {
            file = Json.str(sb, "texture", "sky.jpg");
            if (file != null && !Cosmetic.safeFileName(file)) throw new Json.ParseException("bad skybox texture file name");
            sbDay = (float) Math.max(0, Math.min(1.5, Json.num(sb, "dayBrightness", 1)));
            sbNight = (float) Math.max(0, Math.min(1.5, Json.num(sb, "nightBrightness", 1)));
            double[] r = Json.nums(sb.get("rotation"), 3, new double[3]);
            for (int k = 0; k < 3; k++) rot[k] = (float) Math.max(-90, Math.min(90, r[k]));
        }
        return new SkyPreset(id, name, time, Json.bool(m, "clearWeather", false), day, night, stars, moon,
            file, sbDay, sbNight, rot);
    }

    private static float[][] colorSet(Map<String, Object> g, int[] fallback) throws Json.ParseException {
        String[] keys = { "sky", "fog", "cloud", "sunset" };
        float[][] out = new float[4][];
        for (int k = 0; k < 4; k++) {
            String hex = Json.str(g, keys[k], null);
            out[k] = hex == null ? rgb(fallback[k]) : rgb(parseHex(hex));
        }
        return out;
    }

    static int parseHex(String s) throws Json.ParseException {
        if (s.length() != 7 || s.charAt(0) != '#') throw new Json.ParseException("colour must be #rrggbb: " + s);
        try {
            return Integer.parseInt(s.substring(1), 16);
        } catch (NumberFormatException e) {
            throw new Json.ParseException("colour must be #rrggbb: " + s);
        }
    }

    private static float[] rgb(int c) {
        return new float[] { ((c >> 16) & 0xFF) / 255f, ((c >> 8) & 0xFF) / 255f, (c & 0xFF) / 255f };
    }

    // ---- Sun height ------------------------------------------------------------

    /** Vanilla celestial angle (0 = noon) for a tick of day. */
    static double celestialAngleForTick(long tick) {
        double a = (((tick % 24000L) + 24000L) % 24000L) / 24000.0 - 0.25;
        a -= Math.floor(a);
        double d1 = 0.5 - Math.cos(a * Math.PI) / 2.0;
        double angle = (a * 2.0 + d1) / 3.0;
        return angle - Math.floor(angle);
    }

    static double heightOfAngle(double celestialAngle) {
        return Math.cos(celestialAngle * 2 * Math.PI);
    }

    static double heightOfDegrees(double sunDegrees) {
        return Math.cos(Math.toRadians(sunDegrees));
    }

    private static double dayFactor(double h) {
        double f = h * 2 + 0.5;
        return f < 0 ? 0 : (f > 1 ? 1 : f);
    }

    private static double duskFactor(double h) {
        double d = 1 - Math.abs(h) / 0.3;
        if (d <= 0) return 0;
        return d * d * (3 - 2 * d);
    }

    private static float[] mix(float[] a, float[] b, double t) {
        float tf = (float) t;
        return new float[] { a[0] + (b[0] - a[0]) * tf, a[1] + (b[1] - a[1]) * tf, a[2] + (b[2] - a[2]) * tf };
    }

    private float[] group(int g, double h, double duskWeight) {
        double f = dayFactor(h);
        float[] base = mix(night[g], day[g], f);
        if (g == SUNSET) return base;
        float[] tint = mix(night[SUNSET], day[SUNSET], f);
        return mix(base, tint, duskFactor(h) * duskWeight);
    }

    float[] sky(double h) { return group(SKY, h, 0.30); }
    float[] fog(double h) { return group(FOG, h, 0.55); }
    float[] cloud(double h) { return group(CLOUD, h, 0.35); }

    /** Sunrise/sunset glow as {r, g, b, alpha}, or null away from the horizon (vanilla shape). */
    float[] sunset(double h) {
        if (h < -0.4 || h > 0.4) return null;
        double f4 = (h / 0.4) * 0.5 + 0.5;
        double f5 = 1.0 - (1.0 - Math.sin(f4 * Math.PI)) * 0.99;
        f5 *= f5;
        float[] c = group(SUNSET, h, 0);
        return new float[] { c[0], c[1], c[2], (float) f5 };
    }

    /** Star brightness 0..1 (vanilla 0..0.5 curve scaled by the preset). */
    float stars(double h) {
        double f = 1.0 - (h * 2.0 + 0.25);
        f = f < 0 ? 0 : (f > 1 ? 1 : f);
        double v = f * f * 0.5 * stars;
        return (float) (v > 1 ? 1 : v);
    }

    /** Vanilla's rain and thunder darkening: blend towards a grey of the colour's luminance. */
    static float[] weatherDim(float[] c, float rain, float thunder, float mix) {
        float r = c[0], g = c[1], b = c[2];
        if (rain > 0f) {
            float grey = (r * 0.3f + g * 0.59f + b * 0.11f) * 0.6f;
            float k = 1f - rain * mix;
            r = r * k + grey * (1f - k); g = g * k + grey * (1f - k); b = b * k + grey * (1f - k);
        }
        if (thunder > 0f) {
            float grey = (r * 0.3f + g * 0.59f + b * 0.11f) * 0.2f;
            float k = 1f - thunder * mix;
            r = r * k + grey * (1f - k); g = g * k + grey * (1f - k); b = b * k + grey * (1f - k);
        }
        return new float[] { r, g, b };
    }

    /** Cube sky brightness for the given sun height. */
    float skyboxBrightness(double h) {
        return (float) (skyboxNight + (skyboxDay - skyboxNight) * dayFactor(h));
    }
}
