package com.aoko.visuals;

import java.io.File;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;

/**
 * Drives the LegacyPlatform against the real Minecraft 1.8.9 classes (MCP names) on a
 * headless JVM. OpenGL calls are not exercised: this covers name resolution, the world and
 * provider checks, the colour hooks, selection loading and player model part discovery.
 *
 * Usage: LegacyHarness <assetsDir>
 */
public final class LegacyHarness {
    static int failures, checks;
    static ClassLoader cl = LegacyHarness.class.getClassLoader();

    static void check(boolean ok, String what) {
        checks++;
        if (!ok) { failures++; System.err.println("FAIL " + what); }
    }

    static Class<?> c(String n) throws Exception { return Class.forName(n, true, cl); }

    static Object alloc(Class<?> k) throws Exception {
        Field f = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        f.setAccessible(true);
        Object u = f.get(null);
        return u.getClass().getMethod("allocateInstance", Class.class).invoke(u, k);
    }

    static void set(Object o, String name, Object v) throws Exception {
        Field f = Refl.field(o instanceof Class ? (Class<?>) o : o.getClass(), name);
        if (f == null) throw new NoSuchFieldException(name);
        if (Modifier.isStatic(f.getModifiers())) f.set(null, v); else f.set(o, v);
    }

    public static void main(String[] args) throws Exception {
        File assets = new File(args[0]);
        try {
            c("net.minecraft.init.Bootstrap").getMethod("register").invoke(null);
        } catch (Throwable t) {
            System.out.println("note: Bootstrap.register failed: " + t);
        }
        Class<?> mcClass = c("net.minecraft.client.Minecraft");
        Object mc = alloc(mcClass);
        set(mcClass, "theMinecraft", mc);
        set(mc, "scheduledTasks", new java.util.ArrayDeque<Object>());
        Object player = alloc(c("net.minecraft.client.entity.EntityPlayerSP"));
        Object world = alloc(c("net.minecraft.client.multiplayer.WorldClient"));
        Object provider = alloc(c("net.minecraft.world.WorldProviderSurface"));
        set(world, "provider", provider);
        set(mc, "thePlayer", player);
        set(mc, "theWorld", world);

        Library lib = new Library(assets);
        LegacyPlatform p = new LegacyPlatform(cl);
        String status = p.init(lib);
        check(status == null, "LegacyPlatform.init resolves everything (got " + status + ")");
        if (status != null) { System.exit(1); }
        Visuals.legacy = p;

        // Sky: locked midnight.
        Config cfg = Config.parse("{\"sky\":true,\"preset\":\"tsukihime\",\"time\":18000,\"clear\":true}");
        Visuals.config = cfg;
        Visuals.sky = SkyState.of(cfg, lib.loadSky("tsukihime"));
        Float angle = p.celestial(world);
        check(angle != null && Math.abs(angle.floatValue() - 0.5f) < 1e-6f, "locked midnight celestial angle, got " + angle);
        Object sky = p.skyColor(world, 0f);
        check(sky != null && sky.getClass().getName().equals("net.minecraft.util.Vec3"), "sky colour is a Vec3");
        double r = ((Double) Refl.field(sky.getClass(), "xCoord").get(sky)).doubleValue();
        check(r >= 0 && r <= 1, "sky colour component in range");
        check(p.cloud(world, 0f) != null, "cloud colour override");
        Float stars = p.stars(world, 0f);
        check(stars != null && stars.floatValue() > 0.4f, "midnight stars, got " + stars);
        Float rain = p.clearWeather(world);
        check(rain != null && rain.floatValue() == 0f, "clear weather returns 0");
        check(p.moon(world) == null, "preset without a moon phase leaves the world's");
        check(p.fog(provider, 0.5f) != null, "fog override on the overworld provider");
        Object glowNone = p.sunset(provider, 0.5f);
        check(glowNone instanceof float[] && ((float[]) glowNone).length == 0, "no sunset glow at midnight returns the null sentinel");
        Object glow = p.sunset(provider, 0.25f);
        check(glow instanceof float[] && ((float[]) glow).length == 4, "sunset glow at the horizon");

        // Server world, other dimension and disabled state leave vanilla alone.
        Object server = alloc(c("net.minecraft.world.WorldServer"));
        set(server, "provider", provider);
        check(p.celestial(server) == null && p.clearWeather(server) == null, "server worlds are never touched");
        Object nether = alloc(c("net.minecraft.world.WorldProviderHell"));
        set(nether, "dimensionId", Integer.valueOf(-1));
        Object netherWorld = alloc(c("net.minecraft.client.multiplayer.WorldClient"));
        set(netherWorld, "provider", nether);
        check(p.celestial(netherWorld) == null && p.skyColor(netherWorld, 0f) == null, "other dimensions are never touched");
        check(p.fog(nether, 0.5f) == null, "nether provider untouched");
        Visuals.sky = SkyState.OFF;
        check(p.celestial(world) == null && p.skyColor(world, 0f) == null, "disabled sky leaves vanilla");

        // Hooks glue: guard stores, value returns, never throws.
        Visuals.sky = SkyState.of(cfg, lib.loadSky("tsukihime"));
        check(Hooks.lgCelestial(world, 0f) && Math.abs(Hooks.lvCelestial(world, 0f) - 0.5f) < 1e-6f, "celestial hook pair");
        check(Hooks.lgSkyColor(world, player, 0f) && Hooks.lvSkyColor(world, player, 0f) != null, "sky colour hook pair");
        check(!Hooks.lgCelestial(null, 0f), "null world is ignored");
        check(Hooks.lgSunset(provider, 0.5f, 0f) && Hooks.lvSunset(provider, 0.5f, 0f) == null, "sunset sentinel becomes null");
        check(!Hooks.lgLayer(null, null, 0, 0, 0, 0, 0, 0, 0), "layer hook never skips vanilla");

        // Model parts and layer renderer discovery on real classes.
        Object model = c("net.minecraft.client.model.ModelPlayer").getConstructor(float.class, boolean.class).newInstance(0f, Boolean.FALSE);
        java.lang.reflect.Method partsOf = LegacyPlatform.class.getDeclaredMethod("partsOf", Object.class);
        partsOf.setAccessible(true);
        Field[] parts = (Field[]) partsOf.invoke(p, model);
        String[] expect = { "bipedHead", "bipedHeadwear", "bipedBody", "bipedRightArm", "bipedLeftArm", "bipedRightLeg", "bipedLeftLeg" };
        for (int i = 0; i < expect.length; i++) check(parts[i].getName().equals(expect[i]), "part " + i + " is " + expect[i] + ", got " + parts[i].getName());
        Object layer = alloc(c("net.minecraft.client.renderer.entity.layers.LayerCape"));
        Field only = null;
        for (Field f : layer.getClass().getDeclaredFields()) if (!Modifier.isStatic(f.getModifiers())) only = f;
        check(only != null && only.getType().getName().equals("net.minecraft.client.renderer.entity.RenderPlayer"), "cape layer holds the RenderPlayer");

        // Selection loading (no GL involved).
        Config cos = Config.parse("{\"cosmetics\":true,\"hat\":\"halo\",\"back\":\"angel-wings\",\"cape\":\"aoko-glass\"}");
        Visuals.config = cos;
        p.onConfig(cos, SkyState.OFF);
        Object sel = Refl.field(p.getClass(), "selection").get(p);
        check(Refl.field(sel.getClass(), "hat").get(sel) != null && Refl.field(sel.getClass(), "back").get(sel) != null,
            "hat and back selected (notes: " + p.notes() + ")");
        check(p.notes().length() == 0, "no notes for valid selection: " + p.notes());
        Cosmetic wrongSlot = lib.loadCosmetic("halo");
        check("hat".equals(wrongSlot.slot), "halo is a hat");
        Config bad = Config.parse("{\"cosmetics\":true,\"back\":\"halo\",\"hat\":\"missing-thing\"}");
        p.onConfig(bad, SkyState.OFF);
        check(p.notes().contains("not a back") && p.notes().contains("missing-thing"), "mis-slotted and missing cosmetics are reported: " + p.notes());

        // Cape location: queued registration cannot run headless, so publish one by hand.
        Class<?> capeClass = c("com.aoko.visuals.LegacyPlatform$CapeTex");
        java.lang.reflect.Constructor<?> cc = capeClass.getDeclaredConstructors()[0];
        cc.setAccessible(true);
        Object loc = c("net.minecraft.util.ResourceLocation").getConstructor(String.class, String.class).newInstance("aoko", "cape/1");
        Refl.field(p.getClass(), "cape").set(p, cc.newInstance("k", loc));
        Visuals.config = cos;
        check(p.capeLocation(player) == loc, "cape location for the local player");
        check(p.capeLocation(alloc(c("net.minecraft.client.entity.EntityOtherPlayerMP"))) == null, "other players keep vanilla capes");
        check(Hooks.lgCape(player) && Hooks.lvCape(player) == loc, "cape hook pair");
        Visuals.legacy = null;

        if (failures != 0) { System.err.println(failures + " of " + checks + " checks failed"); System.exit(1); }
        System.out.println("LegacyHarness OK (" + checks + " checks)");
    }
}
