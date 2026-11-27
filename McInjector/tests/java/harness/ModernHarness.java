package com.aoko.visuals;

import java.io.File;
import java.lang.reflect.Array;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.EnumSet;
import java.util.List;
import java.util.Set;

/**
 * Drives the ModernPlatform against the real Minecraft 26.x classes on a headless JVM.
 * Nothing here needs a GPU: the game model, pose stack, render state and skin classes are
 * plain Java. Texture registration and the vertex consumer are faked.
 *
 * Classpath: the client jar, its libraries, and the compiled com.aoko.visuals classes.
 * Usage: ModernHarness <assetsDir>
 */
public final class ModernHarness {
    static int failures, checks;
    static ClassLoader cl = ModernHarness.class.getClassLoader();

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

    static void set(Object o, String field, Object v) throws Exception {
        Field f = Refl.field(o.getClass(), field);
        if (f == null) throw new NoSuchFieldException(o.getClass().getName() + "." + field);
        f.set(o, v);
    }

    static Object get(Object o, String field) throws Exception {
        Field f = Refl.field(o instanceof Class ? (Class<?>) o : o.getClass(), field);
        if (f == null) throw new NoSuchFieldException(field);
        return f.get(o instanceof Class ? null : o);
    }

    public static void main(String[] args) throws Exception {
        File assets = new File(args[0]);
        System.out.println("Java " + System.getProperty("java.version"));
        bootstrapGame();
        cubeEquivalence();
        rotationEquivalence();
        platformChecks(assets);
        if (failures != 0) { System.err.println(failures + " of " + checks + " checks failed"); System.exit(1); }
        System.out.println("ModernHarness OK (" + checks + " checks)");
    }

    static void bootstrapGame() throws Exception {
        try {
            c("net.minecraft.SharedConstants").getMethod("tryDetectVersion").invoke(null);
            c("net.minecraft.server.Bootstrap").getMethod("bootStrap").invoke(null);
        } catch (Throwable t) {
            System.out.println("note: Bootstrap.bootStrap failed (" + t + "); continuing without registries");
        }
    }

    // ---- ModelPart.Cube equivalence -----------------------------------------------------------

    static void cubeEquivalence() throws Exception {
        Class<?> cube = c("net.minecraft.client.model.geom.ModelPart$Cube");
        Class<?> dir = c("net.minecraft.core.Direction");
        Constructor<?> ctor = cube.getConstructors()[0];
        Set<Object> all = new java.util.HashSet<Object>();
        for (Object d : dir.getEnumConstants()) all.add(d);
        java.util.Random r = new java.util.Random(11);
        for (int n = 0; n < 300; n++) {
            double ox = r.nextInt(40) - 20, oy = r.nextInt(40) - 5, oz = r.nextInt(40) - 20;
            double w = 1 + r.nextInt(9), h = 1 + r.nextInt(9), d = 1 + r.nextInt(9);
            double grow = r.nextBoolean() ? 0 : r.nextInt(3) * 0.25;
            boolean mirror = r.nextBoolean();
            int u = r.nextInt(40), v = r.nextInt(40);
            String json = "{\"minecraft:geometry\":[{\"description\":{\"texture_width\":64,\"texture_height\":64},\"bones\":[{\"name\":\"b\",\"cubes\":[{"
                + "\"origin\":[" + ox + "," + oy + "," + oz + "],\"size\":[" + w + "," + h + "," + d + "],\"uv\":[" + u + "," + v + "],"
                + "\"inflate\":" + grow + ",\"mirror\":" + mirror + "}]}]}]}";
            GeoModel m = GeoModel.parse(json);
            GeoModel.Cube mine = m.bones[0].cubes.get(0);
            double minY = 24 - (oy + h);
            Object real = ctor.newInstance(u, v, (float) ox, (float) minY, (float) oz, (float) w, (float) h, (float) d,
                (float) grow, (float) grow, (float) grow, mirror, 64f, 64f, all);
            Object[] polys = (Object[]) get(real, "polygons");
            boolean ok = polys.length == mine.faces;
            for (int f = 0; f < polys.length && ok; f++) {
                Object[] verts = (Object[]) polys[f].getClass().getMethod("vertices").invoke(polys[f]);
                Object normal = polys[f].getClass().getMethod("normal").invoke(polys[f]);
                for (int k = 0; k < 4 && ok; k++) {
                    Object vx = verts[k];
                    float[] real5 = new float[5];
                    String[] names = { "x", "y", "z", "u", "v" };
                    for (int i = 0; i < 5; i++) real5[i] = ((Float) vx.getClass().getMethod(names[i]).invoke(vx)).floatValue();
                    int pi = (f * 4 + k) * 3, ui = (f * 4 + k) * 2;
                    ok = Math.abs(real5[0] - mine.pos[pi]) < 1e-4 && Math.abs(real5[1] - mine.pos[pi + 1]) < 1e-4
                        && Math.abs(real5[2] - mine.pos[pi + 2]) < 1e-4 && Math.abs(real5[3] - mine.uv[ui]) < 1e-5
                        && Math.abs(real5[4] - mine.uv[ui + 1]) < 1e-5;
                    if (!ok) System.err.println("cube " + n + " face " + f + " vertex " + k + " real " + java.util.Arrays.toString(real5)
                        + " mine " + mine.pos[pi] + "," + mine.pos[pi + 1] + "," + mine.pos[pi + 2] + " uv " + mine.uv[ui] + "," + mine.uv[ui + 1]);
                }
                float nx = ((Float) normal.getClass().getMethod("x").invoke(normal)).floatValue();
                float ny = ((Float) normal.getClass().getMethod("y").invoke(normal)).floatValue();
                float nz = ((Float) normal.getClass().getMethod("z").invoke(normal)).floatValue();
                ok = ok && Math.abs(nx - mine.normal[f * 3]) < 1e-5 && Math.abs(ny - mine.normal[f * 3 + 1]) < 1e-5 && Math.abs(nz - mine.normal[f * 3 + 2]) < 1e-5;
                if (!ok) System.err.println("cube " + n + " face " + f + " normal real " + nx + "," + ny + "," + nz + " mine "
                    + mine.normal[f * 3] + "," + mine.normal[f * 3 + 1] + "," + mine.normal[f * 3 + 2]);
            }
            check(ok, "cube " + n + " matches ModelPart.Cube (mirror=" + mirror + ", grow=" + grow + ")");
            if (!ok) break;
        }
    }

    static void rotationEquivalence() throws Exception {
        Class<?> m4 = c("org.joml.Matrix4f");
        Class<?> v3 = c("org.joml.Vector3f");
        java.util.Random r = new java.util.Random(5);
        for (int n = 0; n < 200; n++) {
            double rx = r.nextInt(360) - 180, ry = r.nextInt(360) - 180, rz = r.nextInt(360) - 180;
            Mat mine = Mat.bedrockRotation(rx, ry, rz);
            // ModelPart.translateAndRotate uses rotateZYX(zRot, yRot, xRot) with the Java angles (-rx, ry, -rz).
            Object mat = m4.getConstructor().newInstance();
            Object rotated = m4.getMethod("rotateZYX", float.class, float.class, float.class)
                .invoke(mat, (float) Math.toRadians(-rz), (float) Math.toRadians(ry), (float) Math.toRadians(-rx));
            Object vec = v3.getConstructor(float.class, float.class, float.class).newInstance(1f, 2f, 3f);
            Object out = m4.getMethod("transformPosition", v3).invoke(rotated, vec);
            double[] p = new double[3];
            mine.point(1, 2, 3, p);
            double ex = ((Float) v3.getMethod("x").invoke(out)).doubleValue(), ey = ((Float) v3.getMethod("y").invoke(out)).doubleValue(),
                ez = ((Float) v3.getMethod("z").invoke(out)).doubleValue();
            check(Math.abs(ex - p[0]) < 1e-4 && Math.abs(ey - p[1]) < 1e-4 && Math.abs(ez - p[2]) < 1e-4,
                "rotation matches JOML rotateZYX for " + rx + "," + ry + "," + rz);
        }
    }

    // ---- Platform ------------------------------------------------------------------------------

    static Object newPoseStack() throws Exception { return c("com.mojang.blaze3d.vertex.PoseStack").getConstructor().newInstance(); }

    static void platformChecks(File assets) throws Exception {
        Class<?> mcClass = c("net.minecraft.client.Minecraft");
        Object mc = alloc(mcClass);
        Field inst = Refl.field(mcClass, "instance");
        check(inst != null, "Minecraft.instance field exists");
        inst.set(null, mc);

        Object player = alloc(c("net.minecraft.client.player.LocalPlayer"));
        set(player, "id", Integer.valueOf(42));
        Object level = alloc(c("net.minecraft.client.multiplayer.ClientLevel"));
        set(level, "dimension", get(c("net.minecraft.world.level.Level"), "OVERWORLD"));
        set(mc, "player", player);
        set(mc, "level", level);
        // Game-thread tasks queue up here instead of running (they need a GPU).
        set(mc, "gameThread", Thread.currentThread());
        set(mc, "pendingRunnables", new java.util.concurrent.ConcurrentLinkedQueue<Object>());

        Library lib = new Library(assets);
        ModernPlatform p = new ModernPlatform(cl);
        String status = p.init(lib);
        check(status == null, "ModernPlatform.init resolves everything (got " + status + ")");
        if (status != null) return;
        Visuals.modern = p;

        // The preset's moonPhase number is the ordinal of the game's enum (0 = full moon).
        Class<?> moonPhase = c("net.minecraft.world.level.MoonPhase");
        Object[] phases = moonPhase.getEnumConstants();
        for (int i = 0; i < phases.length; i++) {
            check(((Integer) moonPhase.getMethod("index").invoke(phases[i])).intValue() == i, "MoonPhase ordinal " + i + " equals index()");
        }
        check(phases.length == 8 && phases[0].toString().equals("FULL_MOON"), "phase 0 is the full moon");

        // ---- sky attributes
        Object attrs = c("net.minecraft.world.attribute.EnvironmentAttributes");
        String cfg = "{\"sky\":true,\"preset\":\"tsukihime\",\"time\":18000,\"clear\":true}";
        Config cfgObj = Config.parse(cfg);
        Visuals.config = cfgObj;
        Visuals.sky = SkyState.of(cfgObj, lib.loadSky("tsukihime"));
        Object probe = alloc(c("net.minecraft.world.attribute.EnvironmentAttributeProbe"));
        Object sky = p.attribute(probe, get(attrs, "SKY_COLOR"), 0f);
        check(sky != null && sky.getClass().getName().equals("org.joml.Vector3f"), "SKY_COLOR override is a Vector3f");
        // The attributes are declared Vector3fc / Vector4fc, so the values must implement those.
        check(c("org.joml.Vector3fc").isInstance(sky), "SKY_COLOR value implements Vector3fc");
        check(c("org.joml.Vector3fc").isInstance(p.attribute(probe, get(attrs, "FOG_COLOR"), 0f)), "FOG_COLOR value implements Vector3fc");
        Object sun = p.attribute(probe, get(attrs, "SUN_ANGLE"), 0f);
        check(sun instanceof Float && Math.abs(((Float) sun).floatValue() - 180f) < 0.5f, "locked midnight sun angle is 180 degrees, got " + sun);
        Object moon = p.attribute(probe, get(attrs, "MOON_ANGLE"), 0f);
        check(moon instanceof Float && Math.abs(((Float) moon).floatValue() - 360f) < 0.5f, "moon is opposite the sun, got " + moon);
        Object sunset = p.attribute(probe, get(attrs, "SUNRISE_SUNSET_COLOR"), 0f);
        check(sunset != null && sunset.getClass().getName().equals("org.joml.Vector4f") && c("org.joml.Vector4fc").isInstance(sunset), "sunset override is a Vector4fc");
        check(c("org.joml.Vector4fc").isInstance(p.attribute(probe, get(attrs, "CLOUD_COLOR"), 0f)), "CLOUD_COLOR value implements Vector4fc");
        // Another thread gets vanilla values.
        final Object[] other = new Object[1];
        final Object fProbe = probe, fAttr = get(attrs, "SKY_COLOR");
        Thread t = new Thread(new Runnable() { public void run() { try { other[0] = p.attribute(fProbe, fAttr, 0f); } catch (Exception e) { other[0] = e; } } });
        t.start(); t.join();
        check(other[0] == null, "attribute hook ignores other threads, got " + other[0]);
        check(ModernPlatform.modernVersion("26.1") && ModernPlatform.modernVersion("26.4-pre-2") && !ModernPlatform.modernVersion("1.21.11") && !ModernPlatform.modernVersion("") ,
            "version gate separates 26.x from 1.x");
        Object stars = p.attribute(probe, get(attrs, "STAR_BRIGHTNESS"), 0f);
        check(stars instanceof Float && ((Float) stars).floatValue() > 0.4f, "midnight star brightness is high, got " + stars);
        check(p.attribute(probe, get(attrs, "FIREFLY_BUSH_SOUNDS"), 0f) == null, "unrelated attributes stay vanilla");
        check(p.attribute(probe, get(attrs, "CLOUD_COLOR"), 0f) != null, "cloud colour overridden");
        check(p.clearWeather(level) != null && ((Float) p.clearWeather(level)).floatValue() == 0f, "clear weather returns 0");
        check(p.clearWeather(alloc(c("net.minecraft.server.level.ServerLevel"))) == null, "server levels are never touched");
        Visuals.sky = SkyState.OFF;
        check(p.attribute(probe, get(attrs, "SKY_COLOR"), 0f) == null, "disabled sky leaves vanilla");

        // From here on tasks must queue, not run inline (they need a GPU).
        set(mc, "gameThread", new Thread());

        // ---- cosmetics through the cape layer
        Config cos = Config.parse("{\"cosmetics\":true,\"hat\":\"halo\",\"back\":\"angel-wings\",\"cape\":\"aoko-glass\"}");
        Visuals.config = cos;
        p.onConfig(cos, SkyState.OFF);
        Selection: {
            Object sel = get(p, "selection");
            check(get(sel, "hat") != null && get(sel, "back") != null, "hat and back selected (notes: " + p.notes() + ")");
            // Registration needs a GPU; publish the identifiers the game thread would have.
            Object idClass = c("net.minecraft.resources.Identifier");
            for (String slot : new String[] { "hat", "back" }) {
                Object e = get(sel, slot);
                Object id = idClass.getClass() == Class.class ? ((Class<?>) idClass).getMethod("fromNamespaceAndPath", String.class, String.class).invoke(null, "aoko", "test/" + slot) : null;
                set(e, "textureId", id);
            }
        }
        Object assetClass = c("net.minecraft.core.ClientAsset$ResourceTexture");
        Object tid = ((Class<?>) c("net.minecraft.resources.Identifier")).getMethod("fromNamespaceAndPath", String.class, String.class).invoke(null, "aoko", "capes/test");
        Object asset = ((Class<?>) assetClass).getConstructor(c("net.minecraft.resources.Identifier")).newInstance(tid);
        Class<?> capeTexClass = c("com.aoko.visuals.ModernPlatform$CapeTex");
        Constructor<?> capeCtor = capeTexClass.getDeclaredConstructors()[0];
        capeCtor.setAccessible(true);
        Object path = ((Class<?>) assetClass).getMethod("texturePath").invoke(asset);
        set(p, "cape", capeCtor.newInstance("k", path, asset));

        Object state = c("net.minecraft.client.renderer.entity.state.AvatarRenderState").getConstructor().newInstance();
        set(state, "id", Integer.valueOf(42));
        Class<?> idCls = c("net.minecraft.resources.Identifier");
        Object steve = ((Class<?>) assetClass).getConstructor(idCls).newInstance(((Class<?>) idCls).getMethod("withDefaultNamespace", String.class).invoke(null, "steve"));
        Class<?> skinCls = c("net.minecraft.world.entity.player.PlayerSkin");
        Object wide = c("net.minecraft.world.entity.player.PlayerModelType").getEnumConstants()[0];
        Object skin = skinCls.getMethod("insecure", c("net.minecraft.core.ClientAsset$Texture"), c("net.minecraft.core.ClientAsset$Texture"),
            c("net.minecraft.core.ClientAsset$Texture"), c("net.minecraft.world.entity.player.PlayerModelType")).invoke(null, steve, null, null, wide);
        set(state, "skin", skin);
        Object empty = c("net.minecraft.world.item.ItemStack").getField("EMPTY").get(null);
        set(state, "headEquipment", empty);

        // A real baked player model behind a fake RenderLayerParent.
        Object model = buildPlayerModel();
        Class<?> parentIface = c("net.minecraft.client.renderer.entity.RenderLayerParent");
        Object parent = Proxy.newProxyInstance(cl, new Class<?>[] { parentIface }, new InvocationHandler() {
            final Object m = model;
            public Object invoke(Object proxy, Method me, Object[] a) { return me.getName().equals("getModel") ? m : null; }
        });
        Object layer = alloc(c("net.minecraft.client.renderer.entity.layers.CapeLayer"));
        Field renderer = Refl.field(c("net.minecraft.client.renderer.entity.layers.RenderLayer"), "renderer");
        renderer.set(layer, parent);

        final List<Object[]> submitted = new ArrayList<Object[]>();
        Class<?> collectorIface = c("net.minecraft.client.renderer.SubmitNodeCollector");
        Object collector = Proxy.newProxyInstance(cl, new Class<?>[] { collectorIface }, new InvocationHandler() {
            public Object invoke(Object proxy, Method me, Object[] a) {
                if (me.getName().equals("submitCustomGeometry")) submitted.add(a);
                return null;
            }
        });
        Object poseStack = newPoseStack();
        p.capeLayer(layer, poseStack, collector, 0xF000F0, state);
        Object swapped = get(state, "skin");
        Object capeAsset = skinCls.getMethod("cape").invoke(swapped);
        check(capeAsset == asset, "cape texture swapped in the render state");
        check(submitted.size() == 2, "hat and back submitted as custom geometry, got " + submitted.size());
        check(((Boolean) poseStack.getClass().getMethod("isEmpty").invoke(poseStack)).booleanValue(), "pose stack balanced after submit");

        // Run the deferred geometry callbacks with a recording vertex consumer.
        Class<?> vcIface = c("com.mojang.blaze3d.vertex.VertexConsumer");
        for (Object[] sub : submitted) {
            final int[] verts = { 0 };
            final float[] top = { Float.MAX_VALUE };
            Object vc = Proxy.newProxyInstance(cl, new Class<?>[] { vcIface }, new InvocationHandler() {
                public Object invoke(Object proxy, Method me, Object[] a) {
                    if (me.getName().equals("addVertex") && a.length == 4) {
                        verts[0]++;
                        top[0] = Math.min(top[0], ((Float) a[2]).floatValue());
                    }
                    return proxy;
                }
            });
            Object pose = poseStack.getClass().getMethod("last").invoke(poseStack);
            Object geometry = sub[2];
            geometry.getClass().getMethod("render", c("com.mojang.blaze3d.vertex.PoseStack$Pose"), vcIface).invoke(geometry, pose, vc);
            check(verts[0] > 0 && verts[0] % 4 == 0, "geometry emits whole quads, got " + verts[0] + " vertices");
        }

        // Other players and invisible players are left alone.
        set(state, "id", Integer.valueOf(7));
        submitted.clear();
        p.capeLayer(layer, newPoseStack(), collector, 0, state);
        check(submitted.isEmpty(), "other players get nothing");
        set(state, "id", Integer.valueOf(42));
        set(state, "isInvisible", Boolean.TRUE);
        p.capeLayer(layer, newPoseStack(), collector, 0, state);
        check(submitted.isEmpty(), "invisible local player gets no items");
        Visuals.modern = null;
    }

    static Object buildPlayerModel() throws Exception {
        Class<?> pm = c("net.minecraft.client.model.player.PlayerModel");
        Class<?> deform = c("net.minecraft.client.model.geom.builders.CubeDeformation");
        Object none = deform.getField("NONE").get(null);
        Object mesh = pm.getMethod("createMesh", deform, boolean.class).invoke(null, none, Boolean.FALSE);
        Class<?> layerDef = c("net.minecraft.client.model.geom.builders.LayerDefinition");
        Object def = layerDef.getMethod("create", c("net.minecraft.client.model.geom.builders.MeshDefinition"), int.class, int.class).invoke(null, mesh, 64, 64);
        Object root = layerDef.getMethod("bakeRoot").invoke(def);
        return pm.getConstructor(c("net.minecraft.client.model.geom.ModelPart"), boolean.class).newInstance(root, Boolean.FALSE);
    }
}
