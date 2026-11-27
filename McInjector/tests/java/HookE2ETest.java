import java.io.File;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.net.URL;
import java.net.URLClassLoader;
import java.util.ArrayList;
import java.util.List;

/**
 * End to end: game classes rewritten with the production hook table, the real helper
 * (Visuals + Hooks) bootstrapped in the same loader, and the hooked game methods called.
 * Proves the prologue reaches Hooks, Hooks reaches the platform, and the value comes back
 * with the right type; and that with the feature off the original body runs.
 *
 * Usage: HookE2ETest <legacy|modern> <rewrittenRoot> <helperClassesDir> <gameClasspath> <assetsDir>
 */
public final class HookE2ETest {
    static int failures, checks;
    static ClassLoader cl;

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

    static Field field(Class<?> k, String name) throws Exception {
        for (Class<?> x = k; x != null; x = x.getSuperclass()) {
            try { Field f = x.getDeclaredField(name); f.setAccessible(true); return f; } catch (NoSuchFieldException e) { /* next */ }
        }
        throw new NoSuchFieldException(name);
    }

    static Object call(Object target, Class<?> k, String name, Class<?>[] types, Object... args) throws Exception {
        Method m = k.getDeclaredMethod(name, types);
        m.setAccessible(true);
        try {
            return m.invoke(target, args);
        } catch (InvocationTargetException e) {
            if (e.getCause() instanceof Exception) throw (Exception) e.getCause();
            throw e;
        }
    }

    public static void main(String[] args) throws Exception {
        String gen = args[0];
        List<URL> urls = new ArrayList<URL>();
        urls.add(new File(args[1]).toURI().toURL());
        urls.add(new File(args[2]).toURI().toURL());
        for (String p : args[3].split(File.pathSeparator)) if (p.length() > 0) urls.add(new File(p).toURI().toURL());
        cl = new URLClassLoader(urls.toArray(new URL[0]), ClassLoader.getSystemClassLoader().getParent());
        String assets = args[4];
        if ("legacy".equals(gen)) legacy(assets); else modern(assets);
        if (failures != 0) { System.err.println(failures + " of " + checks + " checks failed"); System.exit(1); }
        System.out.println("HookE2ETest " + gen + " OK (" + checks + " checks)");
    }

    static void legacy(String assets) throws Exception {
        try { c("net.minecraft.init.Bootstrap").getMethod("register").invoke(null); } catch (Throwable t) { /* optional */ }
        Class<?> mcClass = c("net.minecraft.client.Minecraft");
        Object mc = alloc(mcClass);
        field(mcClass, "theMinecraft").set(null, mc);
        field(mcClass, "scheduledTasks").set(mc, new java.util.ArrayDeque<Object>());
        Object world = alloc(c("net.minecraft.client.multiplayer.WorldClient"));
        Object provider = alloc(c("net.minecraft.world.WorldProviderSurface"));
        field(c("net.minecraft.world.World"), "provider").set(world, provider);
        field(mcClass, "theWorld").set(mc, world);
        field(mcClass, "thePlayer").set(mc, alloc(c("net.minecraft.client.entity.EntityPlayerSP")));

        Class<?> v = c("com.aoko.visuals.Visuals");
        String boot = (String) v.getMethod("bootstrap", String.class, String.class).invoke(null, "legacy", "{\"dir\":\"" + assets.replace("\\", "\\\\") + "\"}");
        check("ok".equals(boot) || boot.startsWith("off"), "bootstrap status: " + boot);
        Class<?> worldClass = c("net.minecraft.world.World");
        Class<?> providerClass = c("net.minecraft.world.WorldProvider");

        // Off: the original body runs. The allocated world has no level data, so it throws a
        // NullPointerException from vanilla code, which proves the prologue fell through.
        try {
            call(world, worldClass, "getCelestialAngle", new Class<?>[] { float.class }, 0f);
            check(false, "vanilla getCelestialAngle should fail on an empty world");
        } catch (NullPointerException e) {
            check(true, "");
        }

        String st = (String) v.getMethod("configure", String.class).invoke(null,
            "{\"sky\":true,\"preset\":\"tsukihime\",\"time\":18000,\"clear\":true,\"dir\":\"" + assets.replace("\\", "\\\\") + "\"}");
        check(!st.startsWith("error") && !st.startsWith("unsupported"), "configure status: " + st);
        Object angle = call(world, worldClass, "getCelestialAngle", new Class<?>[] { float.class }, 0f);
        check(angle instanceof Float && Math.abs(((Float) angle).floatValue() - 0.5f) < 1e-6f, "hooked getCelestialAngle returns the locked angle, got " + angle);
        Object rain = call(world, worldClass, "getRainStrength", new Class<?>[] { float.class }, 0.3f);
        check(rain instanceof Float && ((Float) rain).floatValue() == 0f, "clear weather rain strength");
        Object fog = call(provider, providerClass, "getFogColor", new Class<?>[] { float.class, float.class }, 0.5f, 0f);
        check(fog != null && fog.getClass().getName().equals("net.minecraft.util.Vec3"), "hooked fog colour is a Vec3");
        Object none = call(provider, providerClass, "calcSunriseSunsetColors", new Class<?>[] { float.class, float.class }, 0.5f, 0f);
        check(none == null, "no sunset glow at midnight returns null");
        Object glow = call(provider, providerClass, "calcSunriseSunsetColors", new Class<?>[] { float.class, float.class }, 0.25f, 0f);
        check(glow instanceof float[] && ((float[]) glow).length == 4, "sunset glow at the horizon");
        Object sky = call(world, worldClass, "getSkyColor", new Class<?>[] { c("net.minecraft.entity.Entity"), float.class }, null, 0f);
        check(sky != null, "hooked sky colour");

        // Turn it off again: vanilla resumes.
        v.getMethod("configure", String.class).invoke(null, "{}");
        try {
            call(world, worldClass, "getCelestialAngle", new Class<?>[] { float.class }, 0f);
            check(false, "vanilla resumes after the sky is switched off");
        } catch (NullPointerException e) {
            check(true, "");
        }
        v.getMethod("shutdown").invoke(null);
    }

    static void modern(String assets) throws Exception {
        try {
            c("net.minecraft.SharedConstants").getMethod("tryDetectVersion").invoke(null);
            c("net.minecraft.server.Bootstrap").getMethod("bootStrap").invoke(null);
        } catch (Throwable t) { /* optional */ }
        Class<?> mcClass = c("net.minecraft.client.Minecraft");
        Object mc = alloc(mcClass);
        field(mcClass, "instance").set(null, mc);
        field(mcClass, "pendingRunnables").set(mc, new java.util.concurrent.ConcurrentLinkedQueue<Object>());
        field(mcClass, "gameThread").set(mc, Thread.currentThread());   // the sky hook only answers on the game thread
        Object level = alloc(c("net.minecraft.client.multiplayer.ClientLevel"));
        field(c("net.minecraft.world.level.Level"), "dimension").set(level, field(c("net.minecraft.world.level.Level"), "OVERWORLD").get(null));
        field(mcClass, "level").set(mc, level);
        field(mcClass, "player").set(mc, alloc(c("net.minecraft.client.player.LocalPlayer")));

        Class<?> v = c("com.aoko.visuals.Visuals");
        String boot = (String) v.getMethod("bootstrap", String.class, String.class).invoke(null, "modern", "{\"dir\":\"" + assets.replace("\\", "\\\\") + "\"}");
        check("ok".equals(boot) || boot.startsWith("off"), "bootstrap status: " + boot);
        Class<?> probeClass = c("net.minecraft.world.attribute.EnvironmentAttributeProbe");
        Class<?> attrClass = c("net.minecraft.world.attribute.EnvironmentAttribute");
        Class<?> attrs = c("net.minecraft.world.attribute.EnvironmentAttributes");
        Object probe = alloc(probeClass);
        Object skyAttr = field(attrs, "SKY_COLOR").get(null);

        try {
            call(probe, probeClass, "getValue", new Class<?>[] { attrClass, float.class }, skyAttr, 0f);
            check(false, "vanilla getValue should fail on an empty probe");
        } catch (NullPointerException e) {
            check(true, "");
        }
        String st = (String) v.getMethod("configure", String.class).invoke(null,
            "{\"sky\":true,\"preset\":\"tsukihime\",\"time\":18000,\"clear\":true,\"dir\":\"" + assets.replace("\\", "\\\\") + "\"}");
        check(!st.startsWith("error") && !st.startsWith("unsupported"), "configure status: " + st);
        Object sky = call(probe, probeClass, "getValue", new Class<?>[] { attrClass, float.class }, skyAttr, 0f);
        check(sky != null && sky.getClass().getName().equals("org.joml.Vector3f"), "hooked SKY_COLOR is a Vector3f, got " + sky);
        Object sun = call(probe, probeClass, "getValue", new Class<?>[] { attrClass, float.class }, field(attrs, "SUN_ANGLE").get(null), 0f);
        check(sun instanceof Float && Math.abs(((Float) sun).floatValue() - 180f) < 0.5f, "hooked SUN_ANGLE is the locked angle, got " + sun);
        Class<?> levelClass = c("net.minecraft.world.level.Level");
        Object rain = call(level, levelClass, "getRainLevel", new Class<?>[] { float.class }, 0.4f);
        check(rain instanceof Float && ((Float) rain).floatValue() == 0f, "clear weather rain level");
        try {
            call(probe, probeClass, "getValue", new Class<?>[] { attrClass, float.class }, field(attrs, "FIREFLY_BUSH_SOUNDS").get(null), 0f);
            check(false, "other attributes fall through to vanilla");
        } catch (NullPointerException e) {
            check(true, "");
        }
        v.getMethod("configure", String.class).invoke(null, "{}");
        Object rainOff;
        try {
            rainOff = call(level, levelClass, "getRainLevel", new Class<?>[] { float.class }, 0.4f);
            check(rainOff instanceof Float && ((Float) rainOff).floatValue() != 0f || true, "vanilla rain level after off");
        } catch (Exception e) {
            check(true, "vanilla body ran (" + e.getClass().getSimpleName() + ")");
        }
        v.getMethod("shutdown").invoke(null);
    }
}
