package com.aoko.visuals;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/** A cosmetic from {@code cosmetics/<id>/cosmetic.json}: manifest, model and animation. */
final class Cosmetic {
    static final int MAX_ANIMATIONS = 16;

    static final class Anim {
        int bone;
        double[] spin = new double[3];
        double[] swing = new double[3];
        double[] bob = new double[3];
        double speed = 1;
        double phase;
    }

    final String id;
    final String name;
    final String slot;          // "hat" or "back"
    final String anchor;        // head, body, rightArm, leftArm
    final String textureFile;
    final double scale;
    final double[] offset;      // Bedrock px
    final boolean fullbright;
    final boolean translucent;
    final boolean hideWithHelmet;
    final GeoModel model;
    final Anim[] anims;
    final double[] anchorPivot; // Java model space px

    private Cosmetic(String id, String name, String slot, String anchor, String textureFile, double scale,
                     double[] offset, boolean fullbright, boolean translucent, boolean hideWithHelmet,
                     GeoModel model, Anim[] anims) {
        this.id = id; this.name = name; this.slot = slot; this.anchor = anchor; this.textureFile = textureFile;
        this.scale = scale; this.offset = offset; this.fullbright = fullbright; this.translucent = translucent;
        this.hideWithHelmet = hideWithHelmet; this.model = model; this.anims = anims;
        this.anchorPivot = pivotOf(anchor);
    }

    static boolean validAnchor(String a) {
        return "head".equals(a) || "body".equals(a) || "rightArm".equals(a) || "leftArm".equals(a);
    }

    static double[] pivotOf(String anchor) {
        if ("rightArm".equals(anchor)) return new double[] { -5, 2, 0 };
        if ("leftArm".equals(anchor)) return new double[] { 5, 2, 0 };
        return new double[] { 0, 0, 0 };
    }

    /** Safe file name inside the cosmetic directory: no separators, no dots at the start. */
    static boolean safeFileName(String f) {
        return f != null && f.matches("[A-Za-z0-9_][A-Za-z0-9_.-]{0,63}");
    }

    static Cosmetic parse(String id, String manifestJson, String geoJson) throws Json.ParseException {
        Map<String, Object> m = Json.obj(Json.parse(manifestJson));
        if (m == null) throw new Json.ParseException("cosmetic.json is not an object");
        String name = Json.str(m, "name", id);
        if (name.length() > 64) name = name.substring(0, 64);
        String slot = Json.str(m, "slot", "hat");
        if (!"hat".equals(slot) && !"back".equals(slot)) throw new Json.ParseException("slot must be hat or back");
        String anchor = Json.str(m, "anchor", "hat".equals(slot) ? "head" : "body");
        if (!validAnchor(anchor)) throw new Json.ParseException("bad anchor " + anchor);
        String texture = Json.str(m, "texture", "texture.png");
        if (!safeFileName(texture)) throw new Json.ParseException("bad texture file name");
        double scale = Json.num(m, "scale", 1);
        if (!(scale > 0.05 && scale < 8)) throw new Json.ParseException("scale out of range");
        double[] offset = Json.nums(m.get("offset"), 3, new double[3]);
        for (double v : offset) if (Math.abs(v) > 256) throw new Json.ParseException("offset out of range");

        GeoModel model = GeoModel.parse(geoJson);
        List<Anim> anims = new ArrayList<Anim>();
        List<Object> rawAnims = Json.arr(m.get("animations"));
        if (rawAnims != null) {
            if (rawAnims.size() > MAX_ANIMATIONS) throw new Json.ParseException("too many animations");
            for (Object ra : rawAnims) {
                Map<String, Object> am = Json.obj(ra);
                if (am == null) throw new Json.ParseException("bad animation entry");
                String boneName = Json.str(am, "bone", null);
                int bone = -1;
                for (int b = 0; b < model.bones.length; b++) if (model.bones[b].name.equals(boneName)) bone = b;
                if (bone < 0) throw new Json.ParseException("animation targets unknown bone " + boneName);
                Anim a = new Anim();
                a.bone = bone;
                a.spin = clampVec(Json.nums(am.get("spin"), 3, a.spin), 3600);
                a.swing = clampVec(Json.nums(am.get("swing"), 3, a.swing), 180);
                a.bob = clampVec(Json.nums(am.get("bob"), 3, a.bob), 64);
                a.speed = Math.max(0, Math.min(8, Json.num(am, "speed", 1)));
                a.phase = Json.num(am, "phase", 0);
                anims.add(a);
            }
        }
        boolean hat = "hat".equals(slot);
        return new Cosmetic(id, name, slot, anchor, texture, scale, offset,
            Json.bool(m, "fullbright", false), Json.bool(m, "translucent", false),
            Json.bool(m, "hideWithHelmet", hat), model, anims.toArray(new Anim[0]));
    }

    private static double[] clampVec(double[] v, double limit) {
        double[] r = new double[3];
        for (int k = 0; k < 3; k++) r[k] = Math.max(-limit, Math.min(limit, v[k]));
        return r;
    }

    /**
     * Emits every quad for the given wall-clock time, in blocks relative to the anchor.
     * Allocation is limited to the per-bone matrices; the quad array is reused.
     */
    void emit(double seconds, QuadSink sink) {
        GeoModel.Bone[] bones = model.bones;
        Mat[] world = new Mat[bones.length];
        double[] p = new double[3];
        double[] n = new double[3];
        float[] q = new float[32];
        final double ox = offset[0], oy = -offset[1], oz = offset[2];   // Bedrock y up -> Java y down

        for (int oi = 0; oi < model.order.length; oi++) {
            int b = model.order[oi];
            GeoModel.Bone bone = bones[b];
            double rx = bone.rx, ry = bone.ry, rz = bone.rz;
            double bx = 0, by = 0, bz = 0;
            for (int a = 0; a < anims.length; a++) {
                Anim an = anims[a];
                if (an.bone != b) continue;
                double wave = Math.sin(2 * Math.PI * (an.speed * seconds + an.phase));
                rx += (an.spin[0] * seconds) % 360 + an.swing[0] * wave;
                ry += (an.spin[1] * seconds) % 360 + an.swing[1] * wave;
                rz += (an.spin[2] * seconds) % 360 + an.swing[2] * wave;
                bx += an.bob[0] * wave;
                by += an.bob[1] * wave;
                bz += an.bob[2] * wave;
            }
            Mat local = Mat.about(Mat.bedrockRotation(rx, ry, rz), bone.px, bone.py, bone.pz);
            if (bx != 0 || by != 0 || bz != 0) local = Mat.translation(bx, -by, bz).mul(local);
            world[b] = bone.parent >= 0 ? world[bone.parent].mul(local) : local;

            Mat w = world[b];
            for (int c = 0; c < bone.cubes.size(); c++) {
                GeoModel.Cube cube = bone.cubes.get(c);
                for (int f = 0; f < cube.faces; f++) {
                    w.dir(cube.normal[f * 3], cube.normal[f * 3 + 1], cube.normal[f * 3 + 2], n);
                    double len = Math.sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
                    if (len > 1e-9) { n[0] /= len; n[1] /= len; n[2] /= len; }
                    for (int k = 0; k < 4; k++) {
                        int pi = (f * 4 + k) * 3;
                        w.point(cube.pos[pi], cube.pos[pi + 1], cube.pos[pi + 2], p);
                        int o = k * 8;
                        q[o] = (float) (((p[0] - anchorPivot[0]) * scale + ox) / 16.0);
                        q[o + 1] = (float) (((p[1] - anchorPivot[1]) * scale + oy) / 16.0);
                        q[o + 2] = (float) (((p[2] - anchorPivot[2]) * scale + oz) / 16.0);
                        q[o + 3] = (float) n[0];
                        q[o + 4] = (float) n[1];
                        q[o + 5] = (float) n[2];
                        q[o + 6] = cube.uv[(f * 4 + k) * 2];
                        q[o + 7] = cube.uv[(f * 4 + k) * 2 + 1];
                    }
                    sink.quad(q);
                }
            }
        }
    }
}
