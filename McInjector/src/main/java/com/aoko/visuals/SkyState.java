package com.aoko.visuals;

/** What the sky hooks should do right now. Immutable; swapped atomically by {@link Visuals}. */
final class SkyState {
    static final SkyState OFF = new SkyState(false, null, -1, false, false);

    /** Override sky, fog, cloud and star values from {@link #preset}. */
    final boolean colors;
    final SkyPreset preset;          // non-null when colors is true
    final int lockTick;              // -1 follows the world clock
    final boolean clear;             // suppress rain and thunder visuals
    final boolean skybox;            // the preset has a cube texture to draw (legacy only)
    final double angle;              // celestial angle of the locked time (0 = noon)

    private SkyState(boolean colors, SkyPreset preset, int lockTick, boolean clear, boolean skybox) {
        this.colors = colors; this.preset = preset; this.lockTick = lockTick; this.clear = clear; this.skybox = skybox;
        this.angle = lockTick >= 0 ? SkyPreset.celestialAngleForTick(lockTick) : 0;
    }

    boolean locked() { return lockTick >= 0; }

    boolean active() { return colors || clear || locked(); }

    static SkyState of(Config c, SkyPreset explicit) {
        if (!c.sky) return OFF;
        int tick = c.time >= 0 ? c.time : (c.time == -2 && explicit != null ? explicit.time : -1);
        boolean colors = explicit != null || tick >= 0;
        SkyPreset p = explicit != null ? explicit : (colors ? SkyPreset.classic() : null);
        boolean clear = c.clearWeather || (explicit != null && explicit.clearWeather);
        boolean box = explicit != null && explicit.skyboxFile != null;
        if (!colors && !clear) return OFF;
        return new SkyState(colors, p, tick, clear, box);
    }
}
