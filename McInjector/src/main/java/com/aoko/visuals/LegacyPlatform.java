package com.aoko.visuals;

import static com.aoko.visuals.Refl.field;
import static com.aoko.visuals.Refl.method;

import java.awt.image.BufferedImage;
import java.io.File;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ConcurrentLinkedQueue;

/**
 * Minecraft 1.8.9 (Forge or Lunar, MCP or SRG names). Everything is found by reflection
 * so the helper never links against game types. OpenGL goes through {@link LegacyGl}.
 */
final class LegacyPlatform implements Platform {
    private static final String[] N_PLAYER = { "thePlayer", "field_71439_g" };
    private static final String[] N_WORLD = { "theWorld", "field_71441_e" };
    private static final String[] N_ADD_TASK = { "addScheduledTask", "func_152344_a" };
    private static final String[] N_TEXTURE_MANAGER = { "getTextureManager", "func_110434_K" };
    private static final String[] N_LOAD_TEXTURE = { "loadTexture", "func_110579_a" };
    private static final String[] N_CELESTIAL = { "getCelestialAngle", "func_72826_c" };
    private static final String[] N_RAIN = { "getRainStrength", "func_72867_j" };
    private static final String[] N_THUNDER = { "getThunderStrength", "func_72819_i" };
    private static final String[] N_PROVIDER = { "provider", "field_73011_w" };
    private static final String[] N_DIMENSION = { "getDimensionId", "func_177502_q" };
    private static final String[] N_INVISIBLE = { "isInvisible", "func_82150_aj" };
    private static final String[] N_ARMOR = { "getCurrentArmor", "func_82169_q" };
    private static final String[] N_MAIN_MODEL = { "getMainModel", "func_177087_b" };
    private static final String[] N_POST_RENDER = { "postRender", "func_78794_c" };

    private static final int PART_HEAD = 0, PART_BODY = 2, PART_RIGHT_ARM = 3, PART_LEFT_ARM = 4;

    /** A cosmetic selected for a slot plus its decoded texture. {@code tex} is owned by the render thread. */
    private static final class Entry {
        final Cosmetic cosmetic;
        final LegacyGl.Pixels pixels;
        final String key;
        volatile int tex;
        Entry(Cosmetic c, LegacyGl.Pixels pixels, String key) { this.cosmetic = c; this.pixels = pixels; this.key = key; }
    }

    private static final class Selection {
        static final Selection NONE = new Selection(null, null);
        final Entry hat, back;
        Selection(Entry hat, Entry back) { this.hat = hat; this.back = back; }
    }

    private static final class CapeTex {
        final String key;
        final Object location;
        CapeTex(String key, Object location) { this.key = key; this.location = location; }
    }

    private static final class WorldCache {
        final Object world;
        final boolean overworldClient;
        WorldCache(Object world, boolean ok) { this.world = world; this.overworldClient = ok; }
    }

    private final ClassLoader loader;
    private Library lib;

    // Core handles.
    private Object mc;
    private Field fPlayer, fWorld;
    private Method mAddTask;
    private Class<?> worldClientClass;

    // Sky handles.
    private boolean skyOk;
    private Method mCelestial, mRain, mThunder, mDimension;
    private Field fProvider;
    private Constructor<?> vec3Ctor;

    // Cosmetic handles.
    private boolean cosmeticsOk, capeOk;
    private Method mInvisible, mArmor, mMainModel, mPostRender, mTextureManager, mLoadTexture;
    private Constructor<?> rlCtor, dynamicTextureCtor;
    private Class<?> modelRendererClass;
    private Field[] rotationFields;                 // rpx rpy rpz rx ry rz, manual postRender fallback
    private Field fSneak;                           // ModelBiped.isSneak: the whole model drops 0.2 while sneaking
    private volatile Field layerRendererField;
    private volatile Class<?> modelClass;
    private volatile Field[] partFields;

    // State.
    private volatile Selection selection = Selection.NONE;
    private volatile CapeTex cape;
    private volatile String capePending = "";
    private volatile WorldCache worldCache = new WorldCache(null, false);
    private volatile WorldCache providerCache = new WorldCache(null, false);
    private volatile String notes = "";
    private final ConcurrentLinkedQueue<Integer> texturesToDelete = new ConcurrentLinkedQueue<Integer>();
    private static final class SkySource {
        final LegacyGl.Pixels pixels;
        final String key;
        SkySource(LegacyGl.Pixels pixels, String key) { this.pixels = pixels; this.key = key; }
    }

    private volatile SkySource skySource;
    private int skyTex;                              // render thread
    private String skyTexKey = "";
    private int skyW, skyH;
    private int capeSeq;

    LegacyPlatform(ClassLoader loader) { this.loader = loader; }

    // ---------------------------------------------------------------------------------------
    // Resolution
    // ---------------------------------------------------------------------------------------

    private Class<?> cls(String name) {
        return Refl.cls(loader, name);
    }

    @Override
    public String init(Library library) {
        this.lib = library;
        Class<?> mcClass = cls("net.minecraft.client.Minecraft");
        if (mcClass == null) return "unsupported:Minecraft class not found (obfuscated names are not supported)";
        // The singleton accessor is the only static no-argument method returning Minecraft.
        Method getter = null;
        for (Method m : mcClass.getDeclaredMethods()) {
            if (Modifier.isStatic(m.getModifiers()) && m.getParameterTypes().length == 0 && m.getReturnType() == mcClass) getter = m;
        }
        if (getter == null) return "unsupported:Minecraft accessor not found";
        try {
            getter.setAccessible(true);
            mc = getter.invoke(null);
        } catch (Throwable t) {
            return "error:Minecraft instance unavailable";
        }
        if (mc == null) return "error:waiting for Minecraft to start";
        fPlayer = field(mcClass, N_PLAYER);
        fWorld = field(mcClass, N_WORLD);
        mAddTask = method(mcClass, N_ADD_TASK, Runnable.class);
        if (fPlayer == null || fWorld == null || mAddTask == null)
            return "unsupported:Minecraft members not found (needs MCP or SRG names)";
        worldClientClass = fWorld.getType();
        if (cls("org.lwjgl.opengl.GL11") == null) return "unsupported:LWJGL not visible";

        StringBuilder partial = new StringBuilder();

        // Sky.
        Class<?> worldClass = cls("net.minecraft.world.World");
        Class<?> vec3 = cls("net.minecraft.util.Vec3");
        mCelestial = worldClass == null ? null : method(worldClass, N_CELESTIAL, float.class);
        mRain = worldClass == null ? null : method(worldClass, N_RAIN, float.class);
        mThunder = worldClass == null ? null : method(worldClass, N_THUNDER, float.class);
        fProvider = worldClass == null ? null : field(worldClass, N_PROVIDER);
        mDimension = fProvider == null ? null : method(fProvider.getType(), N_DIMENSION);
        try {
            vec3Ctor = vec3 == null ? null : vec3.getConstructor(double.class, double.class, double.class);
        } catch (Throwable t) {
            vec3Ctor = null;
        }
        skyOk = mCelestial != null && mRain != null && mThunder != null && fProvider != null && mDimension != null && vec3Ctor != null;
        if (!skyOk) partial.append("sky (world methods not found)");

        // Cosmetics.
        Class<?> entityClass = cls("net.minecraft.entity.Entity");
        Class<?> playerClass = cls("net.minecraft.entity.player.EntityPlayer");
        Class<?> rendererClass = cls("net.minecraft.client.renderer.entity.RendererLivingEntity");
        modelRendererClass = cls("net.minecraft.client.model.ModelRenderer");
        mInvisible = entityClass == null ? null : method(entityClass, N_INVISIBLE);
        mArmor = playerClass == null ? null : method(playerClass, N_ARMOR, int.class);
        mMainModel = rendererClass == null ? null : method(rendererClass, N_MAIN_MODEL);
        if (modelRendererClass != null) {
            mPostRender = method(modelRendererClass, N_POST_RENDER, float.class);
            List<Field> floats = new ArrayList<Field>();
            for (Field f : modelRendererClass.getDeclaredFields()) {
                if (f.getType() == float.class && Modifier.isPublic(f.getModifiers()) && !Modifier.isStatic(f.getModifiers())) floats.add(f);
            }
            if (floats.size() >= 8) {
                rotationFields = new Field[6];
                // Declaration order: textureWidth, textureHeight, rotationPointX/Y/Z, rotateAngleX/Y/Z, ...
                for (int i = 0; i < 6; i++) rotationFields[i] = floats.get(2 + i);
            }
        }
        Class<?> biped = cls("net.minecraft.client.model.ModelBiped");
        fSneak = biped == null ? null : field(biped, new String[] { "isSneak", "field_78117_n" });
        cosmeticsOk = mInvisible != null && mArmor != null && fSneak != null && mMainModel != null && modelRendererClass != null
            && (mPostRender != null || rotationFields != null);
        if (!cosmeticsOk) {
            if (partial.length() > 0) partial.append(", ");
            partial.append("cosmetics (model members not found)");
        }

        // Cape textures.
        Class<?> rl = cls("net.minecraft.util.ResourceLocation");
        Class<?> dyn = cls("net.minecraft.client.renderer.texture.DynamicTexture");
        Class<?> texObj = cls("net.minecraft.client.renderer.texture.ITextureObject");
        Class<?> texMgr = cls("net.minecraft.client.renderer.texture.TextureManager");
        mTextureManager = method(mcClass, N_TEXTURE_MANAGER);
        try {
            rlCtor = rl == null ? null : rl.getConstructor(String.class, String.class);
            dynamicTextureCtor = dyn == null ? null : dyn.getConstructor(BufferedImage.class);
        } catch (Throwable t) {
            rlCtor = null;
            dynamicTextureCtor = null;
        }
        mLoadTexture = (texMgr == null || rl == null || texObj == null) ? null : method(texMgr, N_LOAD_TEXTURE, rl, texObj);
        capeOk = rlCtor != null && dynamicTextureCtor != null && mTextureManager != null && mLoadTexture != null;
        if (!capeOk) {
            if (partial.length() > 0) partial.append(", ");
            partial.append("capes (texture manager not found)");
        }
        return partial.length() == 0 ? null : "partial:" + partial;
    }

    // ---------------------------------------------------------------------------------------
    // Configuration and background work
    // ---------------------------------------------------------------------------------------

    @Override
    public void onConfig(Config c, SkyState s) {
        refresh(c, s);
    }

    @Override
    public void maintain(Config c, SkyState s) {
        refresh(c, s);
    }

    @Override
    public String notes() { return notes; }

    @Override
    public void shutdown() {
        Selection old = selection;
        selection = Selection.NONE;
        cape = null;
        retire(old.hat, null);
        retire(old.back, null);
        runLater(new Runnable() {
            public void run() {
                try {
                    LegacyGl.delete(skyTex);
                    skyTex = 0;
                    drainDeletes();
                } catch (Throwable t) {
                    Visuals.log("legacy cleanup failed", t);
                }
            }
        });
    }

    private void runLater(Runnable r) {
        try {
            mAddTask.invoke(mc, r);
        } catch (Throwable t) {
            Visuals.log("scheduling failed", t);
        }
    }

    private void drainDeletes() {
        Integer id;
        while ((id = texturesToDelete.poll()) != null) LegacyGl.delete(id.intValue());
    }

    /** Runs on the bridge thread: does the file IO and parsing, never touches GL. */
    private void refresh(Config c, SkyState s) {
        StringBuilder n = new StringBuilder();
        try {
            // Cosmetics.
            Selection next = Selection.NONE;
            if (c.cosmetics && cosmeticsOk) {
                Entry hat = entry(c.hat, "hat", n, selection.hat);
                Entry back = entry(c.back, "back", n, selection.back);
                next = (hat == null && back == null) ? Selection.NONE : new Selection(hat, back);
            } else if (c.cosmetics) {
                n.append("cosmetics unavailable on this build");
            }
            if (next != selection) {
                Selection old = selection;
                selection = next;
                retire(old.hat, next.hat);
                retire(old.back, next.back);
            }

            // Cape.
            if (c.cosmetics && c.cape.length() > 0) {
                if (!capeOk) n.append("capes unavailable on this build; ");
                else ensureCape(c.cape, n);
            } else {
                cape = null;
                capePending = "";
            }

            // Sky box image.
            SkySource sky = null;
            if (s.skybox && skyOk && s.preset != null && s.preset.skyboxFile != null) {
                File f = new File(lib.skyDir(s.preset.id), s.preset.skyboxFile);
                String key = f.getPath() + "|" + lib.stamp(f);
                SkySource current = skySource;
                if (current != null && key.equals(current.key)) {
                    sky = current;
                } else {
                    try {
                        BufferedImage loaded = lib.loadImage(f);
                        if (Library.validAtlas(loaded)) sky = new SkySource(LegacyGl.prepare(loaded), key);
                        else n.append("sky texture must be a 3x2 grid of square faces; ");
                    } catch (Exception e) {
                        n.append("sky texture unreadable: ").append(Visuals.brief(e)).append("; ");
                    }
                }
            } else if (s.colors && !skyOk) {
                n.append("sky unavailable on this build; ");
            }
            skySource = sky;
        } catch (Throwable t) {
            n.append("refresh failed: ").append(Visuals.brief(t));
            Visuals.log("legacy refresh failed", t);
        }
        notes = n.toString().trim().replaceAll(";$", "");
    }

    private void retire(Entry old, Entry current) {
        if (old != null && old != current && old.tex != 0) texturesToDelete.add(Integer.valueOf(old.tex));
    }

    /** Keeps {@code old} when its files are unchanged, so textures are decoded and uploaded once. */
    private Entry entry(String id, String slot, StringBuilder n, Entry old) {
        if (id.length() == 0) return null;
        try {
            Cosmetic c = lib.loadCosmetic(id);
            if (!slot.equals(c.slot)) { n.append("'").append(id).append("' is not a ").append(slot).append(" cosmetic; "); return null; }
            File tf = new File(lib.cosmeticDir(id), c.textureFile);
            String key = id + "|" + lib.stamp(tf) + "|" + lib.stamp(new File(lib.cosmeticDir(id), "cosmetic.json"));
            if (old != null && old.key.equals(key)) return old;
            return new Entry(c, LegacyGl.prepare(lib.loadImage(tf)), key);
        } catch (Exception e) {
            n.append("cosmetic '").append(id).append("': ").append(Visuals.brief(e)).append("; ");
            return null;
        }
    }

    private void ensureCape(final String id, StringBuilder n) {
        File f = lib.capeFile(id);
        if (f == null) return;
        final String key;
        final BufferedImage img;
        try {
            key = id + "|" + lib.stamp(f);
            CapeTex cur = cape;
            if ((cur != null && cur.key.equals(key)) || key.equals(capePending)) return;
            img = lib.loadImage(f);
            if (!Library.validCape(img)) { n.append("cape '").append(id).append("' must be 2:1 with a width that is a multiple of 64; "); return; }
        } catch (Exception e) {
            n.append("cape '").append(id).append("': ").append(Visuals.brief(e)).append("; ");
            return;
        }
        capePending = key;
        runLater(new Runnable() {
            public void run() {
                try {
                    Object loc = rlCtor.newInstance("aoko", "cape/" + (++capeSeq));
                    Object tex = dynamicTextureCtor.newInstance(img);
                    Object mgr = mTextureManager.invoke(mc);
                    mLoadTexture.invoke(mgr, loc, tex);
                    cape = new CapeTex(key, loc);
                } catch (Throwable t) {
                    Visuals.log("cape registration failed", t);
                    capePending = "";
                }
            }
        });
    }

    // ---------------------------------------------------------------------------------------
    // World helpers
    // ---------------------------------------------------------------------------------------

    private boolean dimensionZero(Object provider) throws Exception {
        Object d = mDimension.invoke(provider);
        return d instanceof Integer && ((Integer) d).intValue() == 0;
    }

    /** True for the client world in the overworld; false for server worlds and other dimensions. */
    private boolean overworldClient(Object world) throws Exception {
        // Server worlds (singleplayer) call these hooks from another thread; do not let them evict the client entry.
        if (world == null || !worldClientClass.isInstance(world)) return false;
        WorldCache c = worldCache;
        if (c.world == world) return c.overworldClient;
        boolean ok = dimensionZero(fProvider.get(world));
        worldCache = new WorldCache(world, ok);
        return ok;
    }

    private boolean overworldProvider(Object provider) throws Exception {
        WorldCache c = providerCache;
        if (c.world == provider) return c.overworldClient;
        boolean ok = provider != null && fProvider.getType().isInstance(provider) && dimensionZero(provider);
        providerCache = new WorldCache(provider, ok);
        return ok;
    }

    private double heightNow(Object world, float pt, SkyState s) throws Exception {
        if (s.locked()) return SkyPreset.heightOfAngle(s.angle);
        return SkyPreset.heightOfAngle(((Float) mCelestial.invoke(world, Float.valueOf(pt))).floatValue());
    }

    private boolean skyActive() {
        return skyOk && !Visuals.skyFault;
    }

    // ---------------------------------------------------------------------------------------
    // Sky hooks (called from Hooks on the render or server thread). Each method returns the
    // replacement value, or null to leave vanilla alone, so the bytecode prologue's second
    // call can never fail.
    // ---------------------------------------------------------------------------------------

    Float celestial(Object world) throws Exception {
        SkyState s = Visuals.sky;
        if (!skyActive() || !s.locked() || !overworldClient(world)) return null;
        return Float.valueOf((float) s.angle);
    }

    Object skyColor(Object world, float pt) throws Exception {
        SkyState s = Visuals.sky;
        if (!skyActive() || !s.colors || !overworldClient(world)) return null;
        float[] c = s.preset.sky(heightNow(world, pt, s));
        float rain = ((Float) mRain.invoke(world, Float.valueOf(pt))).floatValue();
        float thunder = ((Float) mThunder.invoke(world, Float.valueOf(pt))).floatValue();
        c = SkyPreset.weatherDim(c, rain, thunder, 0.75f);
        return vec3Ctor.newInstance(Double.valueOf(c[0]), Double.valueOf(c[1]), Double.valueOf(c[2]));
    }

    Object cloud(Object world, float pt) throws Exception {
        SkyState s = Visuals.sky;
        if (!skyActive() || !s.colors || !overworldClient(world)) return null;
        float[] c = s.preset.cloud(heightNow(world, pt, s));
        float rain = ((Float) mRain.invoke(world, Float.valueOf(pt))).floatValue();
        float thunder = ((Float) mThunder.invoke(world, Float.valueOf(pt))).floatValue();
        c = SkyPreset.weatherDim(c, rain, thunder, 0.95f);
        return vec3Ctor.newInstance(Double.valueOf(c[0]), Double.valueOf(c[1]), Double.valueOf(c[2]));
    }

    Float stars(Object world, float pt) throws Exception {
        SkyState s = Visuals.sky;
        if (!skyActive() || !s.colors || !overworldClient(world)) return null;
        return Float.valueOf(s.preset.stars(heightNow(world, pt, s)));
    }

    Integer moon(Object world) throws Exception {
        SkyState s = Visuals.sky;
        if (!skyActive() || !s.colors || s.preset.moonPhase < 0 || !overworldClient(world)) return null;
        return Integer.valueOf(s.preset.moonPhase);
    }

    /** Zero for rain and thunder strength while clear weather is on. */
    Float clearWeather(Object world) throws Exception {
        if (!skyActive() || !Visuals.sky.clear || !overworldClient(world)) return null;
        return Float.valueOf(0f);
    }

    Object fog(Object provider, float celestialAngle) throws Exception {
        SkyState s = Visuals.sky;
        if (!skyActive() || !s.colors || !overworldProvider(provider)) return null;
        float[] c = s.preset.fog(SkyPreset.heightOfAngle(celestialAngle));
        return vec3Ctor.newInstance(Double.valueOf(c[0]), Double.valueOf(c[1]), Double.valueOf(c[2]));
    }

    private static final float[] NO_GLOW = new float[0];

    /** A float[4] glow, or {@link #NO_GLOW} when vanilla's null (no glow) should be returned. */
    Object sunset(Object provider, float celestialAngle) throws Exception {
        SkyState s = Visuals.sky;
        if (!skyActive() || !s.colors || !overworldProvider(provider)) return null;
        float[] glow = s.preset.sunset(SkyPreset.heightOfAngle(celestialAngle));
        return glow == null ? NO_GLOW : glow;
    }

    /** Draws the cube sky and reports whether vanilla's sky should be skipped. */
    boolean renderSky(float pt) throws Exception {
        SkyState s = Visuals.sky;
        if (!skyActive() || !s.colors || !s.skybox) return false;
        SkySource src = skySource;
        if (src == null) return false;
        LegacyGl.Pixels img = src.pixels;
        Object world = fWorld.get(mc);
        if (world == null || !overworldClient(world)) return false;
        drainDeletes();
        String key = src.key;
        if (skyTex == 0 || !key.equals(skyTexKey)) {
            LegacyGl.delete(skyTex);
            skyTex = LegacyGl.upload(img, true);
            skyTexKey = key;
            skyW = img.width;
            skyH = img.height;
        }
        if (skyTex == 0) return false;
        double h = heightNow(world, pt, s);
        LegacyGl.drawSkybox(skyTex, skyW, skyH, s.preset.skyboxBrightness(h), s.preset.skyboxRotation, Visuals.seconds());
        return true;
    }

    // ---------------------------------------------------------------------------------------
    // Cosmetic hooks
    // ---------------------------------------------------------------------------------------

    /** The cape texture location for the local player, or null for vanilla's. */
    Object capeLocation(Object player) throws Exception {
        CapeTex c = cape;
        if (c == null || !cosmeticsOk || Visuals.cosmeticsFault || !Visuals.config.cosmetics || player != fPlayer.get(mc)) return null;
        return c.location;
    }

    /** Called from inside the cape layer: draws the hat and back items for the local player. */
    void drawLayer(Object layer, Object entity) throws Exception {
        if (!cosmeticsOk || Visuals.cosmeticsFault || !Visuals.config.cosmetics) return;
        Selection sel = selection;
        if (sel == Selection.NONE || entity != fPlayer.get(mc)) return;
        if (Boolean.TRUE.equals(mInvisible.invoke(entity))) return;
        drainDeletes();

        Field lf = layerRendererField;
        if (lf == null) {
            for (Field f : layer.getClass().getDeclaredFields()) {
                if (!Modifier.isStatic(f.getModifiers())) { f.setAccessible(true); lf = f; break; }
            }
            layerRendererField = lf;
            if (lf == null) throw new IllegalStateException("cape layer has no renderer field");
        }
        Object model = mMainModel.invoke(lf.get(layer));
        Field[] parts = partsOf(model);
        boolean helmet = mArmor.invoke(entity, Integer.valueOf(3)) != null;
        double t = Visuals.seconds();
        drawEntry(sel.hat, model, parts, helmet, t);
        drawEntry(sel.back, model, parts, helmet, t);
    }

    private Field[] partsOf(Object model) {
        Class<?> k = model.getClass();
        Field[] cached = partFields;
        if (cached != null && modelClass == k) return cached;
        List<Class<?>> chain = new ArrayList<Class<?>>();
        for (Class<?> c = k; c != null && c != Object.class; c = c.getSuperclass()) chain.add(0, c);
        List<Field> out = new ArrayList<Field>();
        for (Class<?> c : chain) {
            for (Field f : c.getDeclaredFields()) {
                if (f.getType() == modelRendererClass && !Modifier.isStatic(f.getModifiers())) { f.setAccessible(true); out.add(f); }
            }
        }
        if (out.size() < 7) throw new IllegalStateException("player model has " + out.size() + " parts");
        Field[] arr = out.toArray(new Field[0]);
        modelClass = k;
        partFields = arr;
        return arr;
    }

    private void drawEntry(Entry e, Object model, Field[] parts, boolean helmet, double t) throws Exception {
        if (e == null) return;
        Cosmetic c = e.cosmetic;
        if (c.hideWithHelmet && helmet) return;
        if (e.tex == 0) {
            e.tex = LegacyGl.upload(e.pixels, false);
            if (e.tex == 0) return;
        }
        int idx = "head".equals(c.anchor) ? PART_HEAD : "body".equals(c.anchor) ? PART_BODY
            : "rightArm".equals(c.anchor) ? PART_RIGHT_ARM : PART_LEFT_ARM;
        Object part = parts[idx].get(model);
        LegacyGl.push();
        try {
            // ModelBiped.render drops the whole model by 0.2 while sneaking, before any part is drawn.
            if (fSneak.getBoolean(model)) LegacyGl.translate(0f, 0.2f, 0f);
            applyPart(part);
            LegacyGl.drawCosmetic(c, e.tex, t);
        } finally {
            LegacyGl.pop();
        }
    }

    /** Moves the matrix to a model part the way ModelRenderer.postRender does. */
    private void applyPart(Object part) throws Exception {
        final float s = 0.0625f;
        if (mPostRender != null) {
            mPostRender.invoke(part, Float.valueOf(s));
            return;
        }
        float px = rotationFields[0].getFloat(part), py = rotationFields[1].getFloat(part), pz = rotationFields[2].getFloat(part);
        float rx = rotationFields[3].getFloat(part), ry = rotationFields[4].getFloat(part), rz = rotationFields[5].getFloat(part);
        LegacyGl.translate(px * s, py * s, pz * s);
        if (rz != 0f) LegacyGl.rotate(rz * 57.295776f, 0f, 0f, 1f);
        if (ry != 0f) LegacyGl.rotate(ry * 57.295776f, 0f, 1f, 0f);
        if (rx != 0f) LegacyGl.rotate(rx * 57.295776f, 1f, 0f, 0f);
    }
}
