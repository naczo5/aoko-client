package com.aoko.visuals;

import java.awt.image.BufferedImage;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.lang.reflect.Proxy;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.function.Supplier;
import javax.imageio.ImageIO;

/**
 * Minecraft 26.1 to 26.3 (unobfuscated Mojang names, deferred submit rendering, environment
 * attributes). Sky values are replaced at the environment attribute probe; cosmetics are
 * submitted as custom geometry from inside the vanilla cape layer.
 */
final class ModernPlatform implements Platform {
    private static final int NO_OVERLAY = 655360;          // OverlayTexture.NO_OVERLAY = pack(0, 10)
    private static final int FULL_BRIGHT = 15728880;       // LightTexture.FULL_BRIGHT

    private static final class Entry {
        final Cosmetic cosmetic;
        final BufferedImage image;
        final String key;
        volatile Object textureId;      // Identifier once registered on the game thread
        volatile Object renderType;     // cached on the render thread
        volatile boolean registering;
        volatile boolean dropped;       // released before its registration finished
        Entry(Cosmetic c, BufferedImage image, String key) { this.cosmetic = c; this.image = image; this.key = key; }
    }

    private static final class Selection {
        static final Selection NONE = new Selection(null, null);
        final Entry hat, back;
        Selection(Entry hat, Entry back) { this.hat = hat; this.back = back; }
    }

    private static final class CapeTex {
        final String key;
        final Object identifier;
        final Object asset;             // ClientAsset.ResourceTexture
        CapeTex(String key, Object identifier, Object asset) { this.key = key; this.identifier = identifier; this.asset = asset; }
    }

    private static final class LevelCache {
        final Object level;
        final boolean overworld;
        LevelCache(Object level, boolean overworld) { this.level = level; this.overworld = overworld; }
    }

    private final ClassLoader loader;
    private Library lib;

    // Core.
    private Object mc;
    private Field fPlayer, fLevel;
    private Method mExecute, mEntityId, mTextureManager;
    private Class<?> clientLevelClass;

    // Sky.
    private boolean skyOk;
    private Object aSky, aFog, aSunset, aCloud, aSun, aMoon, aStar, aMoonPhase, aStarBright;
    private Method mGetValue, mRainLevel, mThunderLevel, mDimension, mSameThread;
    private Object overworldKey;
    private Constructor<?> vec3Ctor, vec4Ctor;
    private Object[] moonPhases;

    // Cosmetics.
    private boolean cosmeticsOk, capeOk;
    private Class<?> customGeometryClass;
    private Method mSubmitCustom, mPush, mPop, mTranslateAndRotate, mParentModel, mCutout, mTranslucent;
    private Method vAddVertex, vColor, vUv, vOverlay, vLight, vNormal;
    private Field fHead, fBody, fRightArm, fLeftArm;
    private Field fStateId, fStateInvisible, fStateSkin, fStateHead;
    private Method mItemEmpty;
    private Method mIdentifier, mRegister, mRelease, mNativeRead;
    private Constructor<?> dynamicTextureCtor, skinCtor, resourceTextureCtor;
    private Method sBody, sElytra, sModel, sSecure, rTexturePath;

    // State.
    private volatile Selection selection = Selection.NONE;
    private volatile CapeTex cape;
    private volatile String capePending = "";
    private volatile LevelCache levelCache = new LevelCache(null, false);
    private volatile String notes = "";
    private final ConcurrentLinkedQueue<Object> identifiersToRelease = new ConcurrentLinkedQueue<Object>();
    private Object lastOriginalSkin, lastSwappedSkin, lastCapeAsset;   // render thread only
    private int textureSeq;

    ModernPlatform(ClassLoader loader) { this.loader = loader; }

    private Class<?> cls(String name) { return Refl.cls(loader, name); }

    // ---------------------------------------------------------------------------------------
    // Resolution
    // ---------------------------------------------------------------------------------------

    @Override
    public String init(Library library) {
        this.lib = library;
        Class<?> mcClass = cls("net.minecraft.client.Minecraft");
        if (mcClass == null) return "unsupported:Minecraft class not found";
        if (cls("net.minecraft.client.renderer.SubmitNodeCollector") == null
            || cls("net.minecraft.world.attribute.EnvironmentAttributeProbe") == null
            || cls("net.minecraft.resources.Identifier") == null) {
            return "unsupported:needs Minecraft 26.1 or newer (1.21.x is not supported)";
        }
        String version = gameVersion();
        if (version != null && !modernVersion(version)) {
            return "unsupported:needs Minecraft 26.1 or newer (this is " + version + ")";
        }
        Method getter = null;
        for (Method m : mcClass.getDeclaredMethods()) {
            if (Modifier.isStatic(m.getModifiers()) && m.getParameterTypes().length == 0 && m.getReturnType() == mcClass) getter = m;
        }
        try {
            if (getter == null) return "unsupported:Minecraft accessor not found";
            getter.setAccessible(true);
            mc = getter.invoke(null);
        } catch (Throwable t) {
            return "error:Minecraft instance unavailable";
        }
        if (mc == null) return "error:waiting for Minecraft to start";
        fPlayer = Refl.field(mcClass, "player");
        fLevel = Refl.field(mcClass, "level");
        mExecute = Refl.publicMethod(mcClass, "execute", Runnable.class);
        mTextureManager = Refl.publicMethod(mcClass, "getTextureManager");
        if (fPlayer == null || fLevel == null || mExecute == null || mTextureManager == null)
            return "unsupported:Minecraft members not found";
        clientLevelClass = fLevel.getType();
        mEntityId = Refl.publicMethod(cls("net.minecraft.world.entity.Entity"), "getId");
        if (mEntityId == null) return "unsupported:Entity.getId not found";

        StringBuilder partial = new StringBuilder();
        try {
            resolveSky();
            skyOk = true;
        } catch (Throwable t) {
            skyOk = false;
            partial.append("sky (").append(Visuals.brief(t)).append(")");
            Visuals.log("modern sky resolution failed", t);
        }
        try {
            resolveCosmetics();
            cosmeticsOk = true;
        } catch (Throwable t) {
            cosmeticsOk = false;
            if (partial.length() > 0) partial.append(", ");
            partial.append("cosmetics (").append(Visuals.brief(t)).append(")");
            Visuals.log("modern cosmetics resolution failed", t);
        }
        try {
            resolveCapes();
            capeOk = true;
        } catch (Throwable t) {
            capeOk = false;
            if (partial.length() > 0) partial.append(", ");
            partial.append("capes (").append(Visuals.brief(t)).append(")");
            Visuals.log("modern cape resolution failed", t);
        }
        return partial.length() == 0 ? null : "partial:" + partial;
    }

    /** The game's version name, or null when it cannot be read (the class probes then decide). */
    private String gameVersion() {
        try {
            Class<?> constants = cls("net.minecraft.SharedConstants");
            Object v = constants.getMethod("getCurrentVersion").invoke(null);
            if (v == null) return null;
            for (String name : new String[] { "name", "getName" }) {
                try {
                    Object s = v.getClass().getMethod(name).invoke(v);
                    // A development build reports a name such as "Development Version": treat it as unknown.
                    if (s instanceof String && ((String) s).length() > 0 && Character.isDigit(((String) s).charAt(0))) return (String) s;
                } catch (NoSuchMethodException e) {
                    // try the other accessor
                }
            }
        } catch (Throwable t) {
            // fall through
        }
        return null;
    }

    /** 26.x and later: the year-style versions. 1.x names (1.21.11 and older) are not supported. */
    static boolean modernVersion(String version) {
        int end = 0;
        while (end < version.length() && Character.isDigit(version.charAt(end))) end++;
        if (end == 0) return false;
        try {
            return Integer.parseInt(version.substring(0, end)) >= 26;
        } catch (NumberFormatException e) {
            return false;
        }
    }

    private static Object need(Object o, String what) {
        if (o == null) throw new IllegalStateException(what + " not found");
        return o;
    }

    private Object staticField(Class<?> c, String name) throws Exception {
        Field f = Refl.field(c, name);
        if (f == null) throw new IllegalStateException(c.getSimpleName() + "." + name + " not found");
        return f.get(null);
    }

    private void resolveSky() throws Exception {
        Class<?> attrs = cls("net.minecraft.world.attribute.EnvironmentAttributes");
        need(attrs, "EnvironmentAttributes");
        aSky = staticField(attrs, "SKY_COLOR");
        aFog = staticField(attrs, "FOG_COLOR");
        aSunset = staticField(attrs, "SUNRISE_SUNSET_COLOR");
        aCloud = staticField(attrs, "CLOUD_COLOR");
        aSun = staticField(attrs, "SUN_ANGLE");
        aMoon = staticField(attrs, "MOON_ANGLE");
        aStar = staticField(attrs, "STAR_ANGLE");
        aMoonPhase = staticField(attrs, "MOON_PHASE");
        aStarBright = staticField(attrs, "STAR_BRIGHTNESS");
        Class<?> probe = cls("net.minecraft.world.attribute.EnvironmentAttributeProbe");
        Class<?> attrClass = cls("net.minecraft.world.attribute.EnvironmentAttribute");
        mGetValue = (Method) need(Refl.publicMethod(probe, "getValue", attrClass, float.class), "EnvironmentAttributeProbe.getValue");
        Class<?> levelClass = cls("net.minecraft.world.level.Level");
        mRainLevel = (Method) need(Refl.publicMethod(levelClass, "getRainLevel", float.class), "Level.getRainLevel");
        mThunderLevel = (Method) need(Refl.publicMethod(levelClass, "getThunderLevel", float.class), "Level.getThunderLevel");
        mDimension = (Method) need(Refl.publicMethod(levelClass, "dimension"), "Level.dimension");
        overworldKey = need(staticField(levelClass, "OVERWORLD"), "Level.OVERWORLD");
        // The attributes are declared as Vector3fc / Vector4fc; the values built here must implement them.
        Class<?> v3 = (Class<?>) need(cls("org.joml.Vector3f"), "Vector3f");
        Class<?> v4 = (Class<?>) need(cls("org.joml.Vector4f"), "Vector4f");
        Class<?> v3c = (Class<?>) need(cls("org.joml.Vector3fc"), "Vector3fc");
        Class<?> v4c = (Class<?>) need(cls("org.joml.Vector4fc"), "Vector4fc");
        if (!v3c.isAssignableFrom(v3) || !v4c.isAssignableFrom(v4)) throw new IllegalStateException("JOML vector types changed");
        vec3Ctor = (Constructor<?>) need(v3.getConstructor(float.class, float.class, float.class), "Vector3f");
        vec4Ctor = (Constructor<?>) need(v4.getConstructor(float.class, float.class, float.class, float.class), "Vector4f");
        mSameThread = Refl.publicMethod(cls("net.minecraft.client.Minecraft"), "isSameThread");
        Class<?> phase = cls("net.minecraft.world.level.MoonPhase");
        moonPhases = (Object[]) need(phase.getEnumConstants(), "MoonPhase");
        if (moonPhases.length != 8) throw new IllegalStateException("unexpected MoonPhase count " + moonPhases.length);
    }

    private void resolveCosmetics() throws Exception {
        Class<?> renderLayer = cls("net.minecraft.client.renderer.entity.layers.RenderLayer");
        Class<?> ordered = cls("net.minecraft.client.renderer.OrderedSubmitNodeCollector");
        customGeometryClass = cls("net.minecraft.client.renderer.SubmitNodeCollector$CustomGeometryRenderer");
        Class<?> poseStack = cls("com.mojang.blaze3d.vertex.PoseStack");
        Class<?> pose = cls("com.mojang.blaze3d.vertex.PoseStack$Pose");
        Class<?> vertex = cls("com.mojang.blaze3d.vertex.VertexConsumer");
        Class<?> renderType = cls("net.minecraft.client.renderer.rendertype.RenderType");
        Class<?> renderTypes = cls("net.minecraft.client.renderer.rendertype.RenderTypes");
        Class<?> identifier = cls("net.minecraft.resources.Identifier");
        Class<?> modelPart = cls("net.minecraft.client.model.geom.ModelPart");
        Class<?> humanoid = cls("net.minecraft.client.model.HumanoidModel");
        Class<?> avatarState = cls("net.minecraft.client.renderer.entity.state.AvatarRenderState");
        Class<?> entityState = cls("net.minecraft.client.renderer.entity.state.EntityRenderState");
        Class<?> humanoidState = cls("net.minecraft.client.renderer.entity.state.HumanoidRenderState");
        Class<?> itemStack = cls("net.minecraft.world.item.ItemStack");
        need(renderLayer, "RenderLayer"); need(ordered, "OrderedSubmitNodeCollector"); need(customGeometryClass, "CustomGeometryRenderer");
        need(poseStack, "PoseStack"); need(pose, "PoseStack.Pose"); need(vertex, "VertexConsumer"); need(renderType, "RenderType");
        need(renderTypes, "RenderTypes"); need(identifier, "Identifier"); need(modelPart, "ModelPart"); need(humanoid, "HumanoidModel");
        need(avatarState, "AvatarRenderState"); need(entityState, "EntityRenderState"); need(humanoidState, "HumanoidRenderState");

        mSubmitCustom = (Method) need(Refl.publicMethod(ordered, "submitCustomGeometry", poseStack, renderType, customGeometryClass), "submitCustomGeometry");
        mPush = (Method) need(Refl.publicMethod(poseStack, "pushPose"), "PoseStack.pushPose");
        mPop = (Method) need(Refl.publicMethod(poseStack, "popPose"), "PoseStack.popPose");
        mTranslateAndRotate = (Method) need(Refl.publicMethod(modelPart, "translateAndRotate", poseStack), "ModelPart.translateAndRotate");
        mParentModel = (Method) need(Refl.publicMethod(renderLayer, "getParentModel"), "RenderLayer.getParentModel");
        mCutout = (Method) need(Refl.publicMethod(renderTypes, "entityCutout", identifier), "RenderTypes.entityCutout");
        mTranslucent = (Method) need(Refl.publicMethod(renderTypes, "entityTranslucent", identifier), "RenderTypes.entityTranslucent");
        vAddVertex = (Method) need(Refl.publicMethod(vertex, "addVertex", pose, float.class, float.class, float.class), "VertexConsumer.addVertex");
        vColor = (Method) need(Refl.publicMethod(vertex, "setColor", int.class), "VertexConsumer.setColor");
        vUv = (Method) need(Refl.publicMethod(vertex, "setUv", float.class, float.class), "VertexConsumer.setUv");
        vOverlay = (Method) need(Refl.publicMethod(vertex, "setOverlay", int.class), "VertexConsumer.setOverlay");
        vLight = (Method) need(Refl.publicMethod(vertex, "setLight", int.class), "VertexConsumer.setLight");
        vNormal = (Method) need(Refl.publicMethod(vertex, "setNormal", pose, float.class, float.class, float.class), "VertexConsumer.setNormal");
        fHead = (Field) need(Refl.field(humanoid, "head"), "HumanoidModel.head");
        fBody = (Field) need(Refl.field(humanoid, "body"), "HumanoidModel.body");
        fRightArm = (Field) need(Refl.field(humanoid, "rightArm"), "HumanoidModel.rightArm");
        fLeftArm = (Field) need(Refl.field(humanoid, "leftArm"), "HumanoidModel.leftArm");
        fStateId = (Field) need(Refl.field(avatarState, "id"), "AvatarRenderState.id");
        fStateInvisible = (Field) need(Refl.field(entityState, "isInvisible"), "EntityRenderState.isInvisible");
        fStateHead = (Field) need(Refl.field(humanoidState, "headEquipment"), "HumanoidRenderState.headEquipment");
        mItemEmpty = (Method) need(Refl.publicMethod(itemStack, "isEmpty"), "ItemStack.isEmpty");
        resolveTextures(identifier);
    }

    private void resolveTextures(Class<?> identifier) throws Exception {
        Class<?> manager = cls("net.minecraft.client.renderer.texture.TextureManager");
        Class<?> abstractTexture = cls("net.minecraft.client.renderer.texture.AbstractTexture");
        Class<?> dynamic = cls("net.minecraft.client.renderer.texture.DynamicTexture");
        Class<?> nativeImage = cls("com.mojang.blaze3d.platform.NativeImage");
        need(manager, "TextureManager"); need(abstractTexture, "AbstractTexture"); need(dynamic, "DynamicTexture"); need(nativeImage, "NativeImage");
        mRegister = (Method) need(Refl.publicMethod(manager, "register", identifier, abstractTexture), "TextureManager.register");
        mRelease = (Method) need(Refl.publicMethod(manager, "release", identifier), "TextureManager.release");
        mNativeRead = (Method) need(Refl.publicMethod(nativeImage, "read", byte[].class), "NativeImage.read");
        dynamicTextureCtor = (Constructor<?>) need(dynamic.getConstructor(Supplier.class, nativeImage), "DynamicTexture(Supplier, NativeImage)");
        mIdentifier = (Method) need(Refl.publicMethod(identifier, "fromNamespaceAndPath", String.class, String.class), "Identifier.fromNamespaceAndPath");
    }

    private void resolveCapes() throws Exception {
        Class<?> skin = cls("net.minecraft.world.entity.player.PlayerSkin");
        Class<?> texture = cls("net.minecraft.core.ClientAsset$Texture");
        Class<?> resource = cls("net.minecraft.core.ClientAsset$ResourceTexture");
        Class<?> model = cls("net.minecraft.world.entity.player.PlayerModelType");
        Class<?> avatarState = cls("net.minecraft.client.renderer.entity.state.AvatarRenderState");
        Class<?> identifier = cls("net.minecraft.resources.Identifier");
        need(skin, "PlayerSkin"); need(texture, "ClientAsset.Texture"); need(resource, "ClientAsset.ResourceTexture"); need(model, "PlayerModelType");
        need(mIdentifier, "Identifier factory (cosmetic textures)");
        skinCtor = (Constructor<?>) need(skin.getConstructor(texture, texture, texture, model, boolean.class), "PlayerSkin constructor");
        resourceTextureCtor = (Constructor<?>) need(resource.getConstructor(identifier), "ResourceTexture(Identifier)");
        sBody = (Method) need(Refl.publicMethod(skin, "body"), "PlayerSkin.body");
        sElytra = (Method) need(Refl.publicMethod(skin, "elytra"), "PlayerSkin.elytra");
        sModel = (Method) need(Refl.publicMethod(skin, "model"), "PlayerSkin.model");
        sSecure = (Method) need(Refl.publicMethod(skin, "secure"), "PlayerSkin.secure");
        rTexturePath = (Method) need(Refl.publicMethod(resource, "texturePath"), "ResourceTexture.texturePath");
        fStateSkin = (Field) need(Refl.field(avatarState, "skin"), "AvatarRenderState.skin");
    }

    // ---------------------------------------------------------------------------------------
    // Configuration and background work (bridge thread)
    // ---------------------------------------------------------------------------------------

    @Override
    public void onConfig(Config c, SkyState s) { refresh(c); }

    @Override
    public void maintain(Config c, SkyState s) { refresh(c); }

    @Override
    public String notes() { return notes; }

    @Override
    public void shutdown() {
        selection = Selection.NONE;
        cape = null;
    }

    private void runLater(Runnable r) {
        try {
            mExecute.invoke(mc, r);
        } catch (Throwable t) {
            Visuals.log("scheduling failed", t);
        }
    }

    private void refresh(Config c) {
        StringBuilder n = new StringBuilder();
        try {
            if (c.sky && !skyOk) n.append("sky unavailable on this build; ");
            Selection next = Selection.NONE;
            if (c.cosmetics && cosmeticsOk) {
                Entry hat = entry(c.hat, "hat", n);
                Entry back = entry(c.back, "back", n);
                Selection cur = selection;
                hat = same(cur.hat, hat);
                back = same(cur.back, back);
                if (hat != null || back != null) next = new Selection(hat, back);
                if (hat != null) ensureRegistered(hat);
                if (back != null) ensureRegistered(back);
            } else if (c.cosmetics) {
                n.append("cosmetics unavailable on this build; ");
            }
            Selection old = selection;
            if (next.hat != old.hat || next.back != old.back) {
                selection = next;
                release(old.hat, next);
                release(old.back, next);
            }

            if (c.cosmetics && c.cape.length() > 0) {
                if (!capeOk) n.append("capes unavailable on this build; ");
                else ensureCape(c.cape, n);
            } else {
                cape = null;
                capePending = "";
            }
        } catch (Throwable t) {
            n.append("refresh failed: ").append(Visuals.brief(t));
            Visuals.log("modern refresh failed", t);
        }
        notes = n.toString().trim().replaceAll(";$", "");
    }

    private Entry same(Entry old, Entry fresh) {
        return (old != null && fresh != null && old.key.equals(fresh.key)) ? old : fresh;
    }

    private void release(final Entry old, Selection keep) {
        if (old == null || old == keep.hat || old == keep.back) return;
        old.dropped = true;
        final Object id = old.textureId;
        if (id == null) return;
        runLater(new Runnable() {
            public void run() {
                try { mRelease.invoke(manager(), id); } catch (Throwable t) { Visuals.log("texture release failed", t); }
            }
        });
    }

    private Object manager() throws Exception { return mTextureManager.invoke(mc); }

    private Entry entry(String id, String slot, StringBuilder n) {
        if (id.length() == 0) return null;
        try {
            Cosmetic c = lib.loadCosmetic(id);
            if (!slot.equals(c.slot)) { n.append("'").append(id).append("' is not a ").append(slot).append(" cosmetic; "); return null; }
            File tf = new File(lib.cosmeticDir(id), c.textureFile);
            BufferedImage img = lib.loadImage(tf);
            return new Entry(c, img, id + "|" + lib.stamp(tf) + "|" + lib.stamp(new File(lib.cosmeticDir(id), "cosmetic.json")));
        } catch (Exception e) {
            n.append("cosmetic '").append(id).append("': ").append(Visuals.brief(e)).append("; ");
            return null;
        }
    }

    private static byte[] png(BufferedImage img) throws Exception {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        if (!ImageIO.write(img, "png", out)) throw new IllegalStateException("png encoder missing");
        return out.toByteArray();
    }

    /** Creates the GPU texture on the game thread and publishes its Identifier. */
    private Object register(String name, BufferedImage img) throws Exception {
        Object id = mIdentifier.invoke(null, "aoko", name);
        Object nativeImage = mNativeRead.invoke(null, (Object) png(img));
        Object texture = dynamicTextureCtor.newInstance(new Supplier<String>() {
            public String get() { return "aoko visuals"; }
        }, nativeImage);
        mRegister.invoke(manager(), id, texture);
        return id;
    }

    private void ensureRegistered(final Entry e) {
        if (e.textureId != null || e.registering) return;
        e.registering = true;
        final String name = "cosmetics/" + e.cosmetic.id + "_" + (++textureSeq);
        runLater(new Runnable() {
            public void run() {
                try {
                    Object id = register(name, e.image);
                    e.textureId = id;
                    if (e.dropped) mRelease.invoke(manager(), id);
                } catch (Throwable t) {
                    Visuals.log("cosmetic texture registration failed", t);
                    e.registering = false;
                }
            }
        });
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
        final String name = "capes/" + id + "_" + (++textureSeq);
        runLater(new Runnable() {
            public void run() {
                try {
                    // ResourceTexture(id) derives texturePath = textures/<path>.png; the cape model
                    // looks the texture up by that path, so register it there.
                    Object base = mIdentifier.invoke(null, "aoko", name);
                    Object asset = resourceTextureCtor.newInstance(base);
                    Object path = rTexturePath.invoke(asset);
                    Object nativeImage = mNativeRead.invoke(null, (Object) png(img));
                    Object texture = dynamicTextureCtor.newInstance(new Supplier<String>() {
                        public String get() { return "aoko cape"; }
                    }, nativeImage);
                    mRegister.invoke(manager(), path, texture);
                    CapeTex previous = cape;
                    cape = new CapeTex(key, path, asset);
                    if (previous != null) mRelease.invoke(manager(), previous.identifier);
                } catch (Throwable t) {
                    Visuals.log("cape registration failed", t);
                    capePending = "";
                }
            }
        });
    }

    // ---------------------------------------------------------------------------------------
    // Sky hooks
    // ---------------------------------------------------------------------------------------

    private boolean overworld() throws Exception {
        Object level = fLevel.get(mc);
        LevelCache c = levelCache;
        if (c.level == level) return c.overworld;
        boolean ok = level != null && overworldKey.equals(mDimension.invoke(level));
        levelCache = new LevelCache(level, ok);
        return ok;
    }

    private boolean isClientLevel(Object level) throws Exception {
        return level != null && clientLevelClass.isInstance(level) && overworld();
    }

    /** The replacement value for an environment attribute, or null to keep vanilla's. */
    Object attribute(Object probe, Object attribute, float pt) throws Exception {
        if (!skyOk || Visuals.skyFault) return null;
        SkyState s = Visuals.sky;
        if (!s.active()) return null;
        final boolean angle = s.locked() && (attribute == aSun || attribute == aMoon || attribute == aStar);
        final boolean color = s.colors && (attribute == aSky || attribute == aFog || attribute == aCloud || attribute == aSunset
            || attribute == aStarBright);
        final boolean phase = s.colors && attribute == aMoonPhase && s.preset.moonPhase >= 0;
        if (!angle && !color && !phase) return null;
        // Only the game thread's probes, and only in the overworld.
        if (mSameThread != null && !((Boolean) mSameThread.invoke(mc)).booleanValue()) return null;
        if (!overworld()) return null;
        if (angle) {
            double deg = s.angle * 360.0;
            return Float.valueOf((float) (attribute == aMoon ? deg + 180.0 : deg));
        }
        if (phase) return moonPhases[s.preset.moonPhase];
        double h = sunHeight(probe, pt, s);
        if (attribute == aSky) {
            float[] c = SkyPreset.weatherDim(s.preset.sky(h), rain(pt), thunder(pt), 0.75f);
            return vec3Ctor.newInstance(Float.valueOf(c[0]), Float.valueOf(c[1]), Float.valueOf(c[2]));
        }
        if (attribute == aFog) {
            float[] c = s.preset.fog(h);
            return vec3Ctor.newInstance(Float.valueOf(c[0]), Float.valueOf(c[1]), Float.valueOf(c[2]));
        }
        if (attribute == aCloud) {
            float[] c = SkyPreset.weatherDim(s.preset.cloud(h), rain(pt), thunder(pt), 0.95f);
            return vec4Ctor.newInstance(Float.valueOf(c[0]), Float.valueOf(c[1]), Float.valueOf(c[2]), Float.valueOf(1f));
        }
        if (attribute == aSunset) {
            float[] c = s.preset.sunset(h);
            if (c == null) c = new float[] { 0f, 0f, 0f, 0f };
            return vec4Ctor.newInstance(Float.valueOf(c[0]), Float.valueOf(c[1]), Float.valueOf(c[2]), Float.valueOf(c[3]));
        }
        return Float.valueOf(s.preset.stars(h));    // aStarBright
    }

    private double sunHeight(Object probe, float pt, SkyState s) throws Exception {
        if (s.locked()) return SkyPreset.heightOfAngle(s.angle);
        Object deg = mGetValue.invoke(probe, aSun, Float.valueOf(pt));
        return SkyPreset.heightOfDegrees(((Float) deg).doubleValue());
    }

    private float rain(float pt) throws Exception {
        Object level = fLevel.get(mc);
        return level == null ? 0f : ((Float) mRainLevel.invoke(level, Float.valueOf(pt))).floatValue();
    }

    private float thunder(float pt) throws Exception {
        Object level = fLevel.get(mc);
        return level == null ? 0f : ((Float) mThunderLevel.invoke(level, Float.valueOf(pt))).floatValue();
    }

    /** Zero for rain and thunder level while clear weather is on. */
    Float clearWeather(Object level) throws Exception {
        if (!skyOk || Visuals.skyFault || !Visuals.sky.clear || !isClientLevel(level)) return null;
        return Float.valueOf(0f);
    }

    // ---------------------------------------------------------------------------------------
    // Cosmetic hook: runs inside CapeLayer.submit for every rendered player
    // ---------------------------------------------------------------------------------------

    void capeLayer(Object layer, Object poseStack, Object collector, int light, Object state) throws Exception {
        if (!cosmeticsOk || Visuals.cosmeticsFault || !Visuals.config.cosmetics) return;
        Object player = fPlayer.get(mc);
        if (player == null) return;
        int localId = ((Integer) mEntityId.invoke(player)).intValue();
        if (fStateId.getInt(state) != localId) return;

        CapeTex ct = cape;
        if (capeOk && ct != null) swapCape(state, ct);

        Selection sel = selection;
        if (sel == Selection.NONE || fStateInvisible.getBoolean(state)) return;
        boolean helmet = !((Boolean) mItemEmpty.invoke(fStateHead.get(state))).booleanValue();
        Object model = mParentModel.invoke(layer);
        double t = Visuals.seconds();
        submit(sel.hat, model, poseStack, collector, light, helmet, t);
        submit(sel.back, model, poseStack, collector, light, helmet, t);
    }

    private void swapCape(Object state, CapeTex ct) throws Exception {
        Object skin = fStateSkin.get(state);
        if (skin == null || skin == lastSwappedSkin) return;
        if (skin != lastOriginalSkin || ct.asset != lastCapeAsset) {
            lastOriginalSkin = skin;
            lastCapeAsset = ct.asset;
            lastSwappedSkin = skinCtor.newInstance(sBody.invoke(skin), ct.asset, sElytra.invoke(skin), sModel.invoke(skin), sSecure.invoke(skin));
        }
        fStateSkin.set(state, lastSwappedSkin);
    }

    private void submit(Entry e, Object model, Object poseStack, Object collector, int light, boolean helmet, double t) throws Exception {
        if (e == null) return;
        final Cosmetic c = e.cosmetic;
        if (c.hideWithHelmet && helmet) return;
        Object id = e.textureId;
        if (id == null) return;
        Object rt = e.renderType;
        if (rt == null) {
            rt = (c.translucent ? mTranslucent : mCutout).invoke(null, id);
            e.renderType = rt;
        }
        Field partField = "head".equals(c.anchor) ? fHead : "body".equals(c.anchor) ? fBody : "rightArm".equals(c.anchor) ? fRightArm : fLeftArm;
        Object part = partField.get(model);
        final int lightCoords = c.fullbright ? FULL_BRIGHT : light;
        final double seconds = t;
        InvocationHandler handler = new InvocationHandler() {
            public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
                if (m.getDeclaringClass() == Object.class) {
                    if ("hashCode".equals(m.getName())) return Integer.valueOf(System.identityHashCode(proxy));
                    if ("equals".equals(m.getName())) return Boolean.valueOf(proxy == args[0]);
                    return "aoko cosmetic geometry";
                }
                try {
                    emit(c, seconds, lightCoords, args[0], args[1]);
                } catch (Throwable ex) {
                    Visuals.fail("cosmetics", ex);
                }
                return null;
            }
        };
        Object renderer = Proxy.newProxyInstance(loader, new Class<?>[] { customGeometryClass }, handler);
        mPush.invoke(poseStack);
        try {
            mTranslateAndRotate.invoke(part, poseStack);
            mSubmitCustom.invoke(collector, poseStack, rt, renderer);
        } finally {
            mPop.invoke(poseStack);
        }
    }

    /** Runs when the renderer flushes the submission: writes the vertices. */
    private void emit(Cosmetic c, double seconds, final int light, final Object pose, final Object consumer) throws Exception {
        final Object white = Integer.valueOf(-1);
        final Object overlay = Integer.valueOf(NO_OVERLAY);
        final Object lightArg = Integer.valueOf(light);
        c.emit(seconds, new QuadSink() {
                public void quad(float[] q) {
                    try {
                        for (int k = 0; k < 4; k++) {
                            int o = k * 8;
                            vAddVertex.invoke(consumer, pose, Float.valueOf(q[o]), Float.valueOf(q[o + 1]), Float.valueOf(q[o + 2]));
                            vColor.invoke(consumer, white);
                            vUv.invoke(consumer, Float.valueOf(q[o + 6]), Float.valueOf(q[o + 7]));
                            vOverlay.invoke(consumer, overlay);
                            vLight.invoke(consumer, lightArg);
                            vNormal.invoke(consumer, pose, Float.valueOf(q[o + 3]), Float.valueOf(q[o + 4]), Float.valueOf(q[o + 5]));
                        }
                    } catch (InvocationTargetException ex) {
                        throw new RuntimeException(ex.getCause());
                    } catch (IllegalAccessException ex) {
                        throw new RuntimeException(ex);
                    }
                }
        });
    }
}
