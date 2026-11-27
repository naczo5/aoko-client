package com.aoko.visuals;

/**
 * Static callbacks the rewritten game bytecode calls (see method_hook_rewriter.h and
 * visuals_hooks.h). Every hook is a pair: {@code xg*} decides whether to override and
 * computes the replacement, {@code xv*} hands it back. The computation lives in the guard
 * so that a failure there simply leaves vanilla behaviour in place; the value half only
 * returns what the guard stored on the same thread.
 *
 * Prefix {@code l} is Minecraft 1.8.9, {@code m} is 26.x. These methods never throw.
 * Names and descriptors are matched by native tables, so changing a signature here means
 * changing visuals_hooks.h and the hook tests.
 */
public final class Hooks {
    private static final ThreadLocal<Object> STASH = new ThreadLocal<Object>();

    private Hooks() {}

    private static boolean put(Object value) {
        if (value == null) return false;
        STASH.set(value);
        return true;
    }

    private static Object take() {
        Object v = STASH.get();
        STASH.set(null);
        return v;
    }

    private static float takeFloat() {
        Object v = take();
        return v instanceof Float ? ((Float) v).floatValue() : 0f;
    }

    private static int takeInt() {
        Object v = take();
        return v instanceof Integer ? ((Integer) v).intValue() : 0;
    }

    // ---- 1.8.9 sky ---------------------------------------------------------------------

    public static boolean lgCelestial(Object world, float pt) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return put(p.celestial(world)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static float lvCelestial(Object world, float pt) { return takeFloat(); }

    public static boolean lgSkyColor(Object world, Object entity, float pt) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return put(p.skyColor(world, pt)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static Object lvSkyColor(Object world, Object entity, float pt) { return take(); }

    public static boolean lgCloud(Object world, float pt) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return put(p.cloud(world, pt)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static Object lvCloud(Object world, float pt) { return take(); }

    public static boolean lgStars(Object world, float pt) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return put(p.stars(world, pt)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static float lvStars(Object world, float pt) { return takeFloat(); }

    public static boolean lgMoon(Object world) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return put(p.moon(world)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static int lvMoon(Object world) { return takeInt(); }

    public static boolean lgRain(Object world, float pt) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return put(p.clearWeather(world)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static float lvRain(Object world, float pt) { return takeFloat(); }

    public static boolean lgThunder(Object world, float pt) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return put(p.clearWeather(world)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static float lvThunder(Object world, float pt) { return takeFloat(); }

    public static boolean lgFog(Object provider, float angle, float pt) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return put(p.fog(provider, angle)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static Object lvFog(Object provider, float angle, float pt) { return take(); }

    public static boolean lgSunset(Object provider, float angle, float pt) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return put(p.sunset(provider, angle)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static Object lvSunset(Object provider, float angle, float pt) {
        Object v = take();
        return (v instanceof float[] && ((float[]) v).length == 0) ? null : v;
    }

    /** RenderGlobal.renderSky(float, int): true skips vanilla's sky after the cube sky was drawn. */
    public static boolean lgSky(Object renderGlobal, float pt, int pass) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return p.renderSky(pt); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }

    // ---- 1.8.9 cosmetics -----------------------------------------------------------------

    public static boolean lgCape(Object player) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { return put(p.capeLocation(player)); } catch (Throwable t) { Visuals.fail("cosmetics", t); return false; }
    }
    public static Object lvCape(Object player) { return take(); }

    /** LayerCape.doRenderLayer: draws the hat and back items, always lets vanilla continue. */
    public static boolean lgLayer(Object layer, Object entity, float limbSwing, float limbSwingAmount, float partialTicks,
                                  float ageInTicks, float netHeadYaw, float headPitch, float scale) {
        LegacyPlatform p = Visuals.legacy;
        if (p == null) return false;
        try { p.drawLayer(layer, entity); } catch (Throwable t) { Visuals.fail("cosmetics", t); }
        return false;
    }

    // ---- 26.x ----------------------------------------------------------------------------

    public static boolean mgAttr(Object probe, Object attribute, float pt) {
        ModernPlatform p = Visuals.modern;
        if (p == null) return false;
        try { return put(p.attribute(probe, attribute, pt)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static Object mvAttr(Object probe, Object attribute, float pt) { return take(); }

    public static boolean mgRain(Object level, float pt) {
        ModernPlatform p = Visuals.modern;
        if (p == null) return false;
        try { return put(p.clearWeather(level)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static float mvRain(Object level, float pt) { return takeFloat(); }

    public static boolean mgThunder(Object level, float pt) {
        ModernPlatform p = Visuals.modern;
        if (p == null) return false;
        try { return put(p.clearWeather(level)); } catch (Throwable t) { Visuals.fail("sky", t); return false; }
    }
    public static float mvThunder(Object level, float pt) { return takeFloat(); }

    /** CapeLayer.submit: swaps the cape texture and submits the hat and back items. Never skips vanilla. */
    public static boolean mgCape(Object layer, Object poseStack, Object collector, int light, Object state, float yRot, float xRot) {
        ModernPlatform p = Visuals.modern;
        if (p == null) return false;
        try { p.capeLayer(layer, poseStack, collector, light, state); } catch (Throwable t) { Visuals.fail("cosmetics", t); }
        return false;
    }
}
