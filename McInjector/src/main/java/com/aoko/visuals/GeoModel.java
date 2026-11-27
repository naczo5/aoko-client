package com.aoko.visuals;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * Bedrock geometry (format 1.12) converted to Java model space.
 *
 * Cube faces follow {@code ModelPart.Cube} from the game exactly (vertex order, box UV
 * layout, mirroring), so a texture painted for a vanilla box lands the same way. A
 * Bedrock point (x, y, z) becomes the Java point (x, 24 - y, z); see {@link Mat}.
 */
final class GeoModel {
    static final int MAX_BONES = 64;
    static final int MAX_CUBES = 512;

    /** One cube: up to six quads in Java model space pixels. */
    static final class Cube {
        int faces;
        final float[] pos = new float[6 * 4 * 3];
        final float[] uv = new float[6 * 4 * 2];
        final float[] normal = new float[6 * 3];
    }

    static final class Bone {
        String name;
        int parent = -1;
        double px, py, pz;          // pivot, Java model space
        double rx, ry, rz;          // rest rotation, Bedrock degrees
        final List<Cube> cubes = new ArrayList<Cube>();
    }

    Bone[] bones;
    int[] order;                    // parents before children
    int texW = 64, texH = 64;
    int cubeCount;

    private static final int DOWN = 0, UP = 1, WEST = 2, NORTH = 3, EAST = 4, SOUTH = 5;
    private static final float[][] DIR = {
        { 0, -1, 0 }, { 0, 1, 0 }, { -1, 0, 0 }, { 0, 0, -1 }, { 1, 0, 0 }, { 0, 0, 1 }
    };

    static GeoModel parse(String json) throws Json.ParseException {
        Map<String, Object> root = Json.obj(Json.parse(json));
        if (root == null) throw new Json.ParseException("geometry root is not an object");
        List<Object> geos = Json.arr(root.get("minecraft:geometry"));
        if (geos == null || geos.isEmpty()) throw new Json.ParseException("missing minecraft:geometry (format 1.12.0 required)");
        Map<String, Object> geo = Json.obj(geos.get(0));
        if (geo == null) throw new Json.ParseException("bad geometry entry");
        GeoModel model = new GeoModel();
        Map<String, Object> desc = Json.obj(geo.get("description"));
        model.texW = clampInt(Json.num(desc, "texture_width", 64), 1, 4096);
        model.texH = clampInt(Json.num(desc, "texture_height", 64), 1, 4096);

        List<Object> rawBones = Json.arr(geo.get("bones"));
        if (rawBones == null || rawBones.isEmpty()) throw new Json.ParseException("model has no bones");
        if (rawBones.size() > MAX_BONES) throw new Json.ParseException("too many bones (max " + MAX_BONES + ")");

        model.bones = new Bone[rawBones.size()];
        Map<String, Integer> index = new HashMap<String, Integer>();
        String[] parents = new String[rawBones.size()];
        for (int b = 0; b < rawBones.size(); b++) {
            Map<String, Object> rb = Json.obj(rawBones.get(b));
            if (rb == null) throw new Json.ParseException("bad bone entry");
            Bone bone = new Bone();
            bone.name = Json.str(rb, "name", null);
            if (bone.name == null || bone.name.length() == 0 || bone.name.length() > 64)
                throw new Json.ParseException("bone without a valid name");
            if (index.containsKey(bone.name)) throw new Json.ParseException("duplicate bone " + bone.name);
            double[] pv = Json.nums(rb.get("pivot"), 3, new double[3]);
            bone.px = pv[0]; bone.py = 24 - pv[1]; bone.pz = pv[2];
            double[] rot = Json.nums(rb.get("rotation"), 3, new double[3]);
            bone.rx = rot[0]; bone.ry = rot[1]; bone.rz = rot[2];
            parents[b] = Json.str(rb, "parent", null);
            model.bones[b] = bone;
            index.put(bone.name, Integer.valueOf(b));

            boolean boneMirror = Json.bool(rb, "mirror", false);
            List<Object> cubes = Json.arr(rb.get("cubes"));
            if (cubes != null) {
                for (Object rc : cubes) {
                    if (++model.cubeCount > MAX_CUBES) throw new Json.ParseException("too many cubes (max " + MAX_CUBES + ")");
                    bone.cubes.add(parseCube(Json.obj(rc), boneMirror, model.texW, model.texH));
                }
            }
        }
        for (int b = 0; b < model.bones.length; b++) {
            if (parents[b] == null) continue;
            Integer p = index.get(parents[b]);
            if (p == null) throw new Json.ParseException("bone " + model.bones[b].name + " has unknown parent " + parents[b]);
            model.bones[b].parent = p.intValue();
        }
        model.order = topologicalOrder(model.bones);
        return model;
    }

    private static int[] topologicalOrder(Bone[] bones) throws Json.ParseException {
        int n = bones.length;
        int[] order = new int[n];
        int[] state = new int[n];       // 0 new, 1 visiting, 2 done
        int[] count = new int[1];
        for (int b = 0; b < n; b++) visit(bones, b, state, order, count);
        return order;
    }

    private static void visit(Bone[] bones, int b, int[] state, int[] order, int[] count) throws Json.ParseException {
        if (state[b] == 2) return;
        if (state[b] == 1) throw new Json.ParseException("bone parent cycle at " + bones[b].name);
        state[b] = 1;
        if (bones[b].parent >= 0) visit(bones, bones[b].parent, state, order, count);
        state[b] = 2;
        order[count[0]++] = b;
    }

    private static int clampInt(double v, int lo, int hi) {
        int i = (int) Math.round(v);
        return i < lo ? lo : (i > hi ? hi : i);
    }

    private static Cube parseCube(Map<String, Object> rc, boolean boneMirror, int texW, int texH) throws Json.ParseException {
        if (rc == null) throw new Json.ParseException("bad cube entry");
        double[] o = Json.nums(rc.get("origin"), 3, null);
        double[] s = Json.nums(rc.get("size"), 3, null);
        if (o == null || s == null) throw new Json.ParseException("cube needs origin and size");
        for (int k = 0; k < 3; k++) {
            if (Math.abs(o[k]) > 1024 || s[k] < 0 || s[k] > 1024) throw new Json.ParseException("cube out of range");
        }
        double grow = Json.num(rc, "inflate", 0);
        boolean mirror = Json.bool(rc, "mirror", boneMirror);
        // Bedrock origin is the min corner with y up; Java's min y is the old max y mirrored.
        double minX = o[0], minY = 24 - (o[1] + s[1]), minZ = o[2];
        double w = s[0], h = s[1], d = s[2];

        Cube cube = new Cube();
        Object uvRaw = rc.get("uv");
        boolean[] present = { true, true, true, true, true, true };
        double[][] rect = new double[6][];          // per Java face: u0, v0, u1, v1 in pixels
        if (uvRaw instanceof List) {
            double[] uv = Json.nums(uvRaw, 2, null);
            if (uv == null) throw new Json.ParseException("cube uv must have two numbers");
            double u0 = uv[0], v0 = uv[1];
            double u1 = u0 + d, u2 = u0 + d + w, u22 = u0 + d + w + w, u3 = u0 + d + w + d, u4 = u0 + d + w + d + w;
            double v1 = v0 + d, v2 = v0 + d + h;
            rect[DOWN] = new double[] { u1, v0, u2, v1 };
            rect[UP] = new double[] { u2, v1, u22, v0 };
            rect[WEST] = new double[] { u0, v1, u1, v2 };
            rect[NORTH] = new double[] { u1, v1, u2, v2 };
            rect[EAST] = new double[] { u2, v1, u3, v2 };
            rect[SOUTH] = new double[] { u3, v1, u4, v2 };
        } else if (uvRaw instanceof Map) {
            Map<String, Object> faces = Json.obj(uvRaw);
            // Bedrock up/down are Java DOWN/UP because the model is mirrored in y.
            String[] names = { "up", "down", "west", "north", "east", "south" };
            for (int f = 0; f < 6; f++) {
                Map<String, Object> fm = Json.obj(faces.get(names[f]));
                if (fm == null) { present[f] = false; continue; }
                double[] uv = Json.nums(fm.get("uv"), 2, null);
                double[] sz = Json.nums(fm.get("uv_size"), 2, null);
                if (uv == null || sz == null) throw new Json.ParseException("face " + names[f] + " needs uv and uv_size");
                rect[f] = new double[] { uv[0], uv[1], uv[0] + sz[0], uv[1] + sz[1] };
            }
        } else {
            throw new Json.ParseException("cube has no uv");
        }

        double x0 = minX - grow, y0 = minY - grow, z0 = minZ - grow;
        double x1 = minX + w + grow, y1 = minY + h + grow, z1 = minZ + d + grow;
        if (mirror) { double t = x1; x1 = x0; x0 = t; }
        // t = z0 side (north), l = z1 side (south); index 0..3 as in ModelPart.Cube.
        double[][] t = { { x0, y0, z0 }, { x1, y0, z0 }, { x1, y1, z0 }, { x0, y1, z0 } };
        double[][] l = { { x0, y0, z1 }, { x1, y0, z1 }, { x1, y1, z1 }, { x0, y1, z1 } };
        double[][][] quad = new double[6][][];
        quad[DOWN] = new double[][] { l[1], l[0], t[0], t[1] };
        quad[UP] = new double[][] { t[2], t[3], l[3], l[2] };
        quad[WEST] = new double[][] { t[0], l[0], l[3], t[3] };
        quad[NORTH] = new double[][] { t[1], t[0], t[3], t[2] };
        quad[EAST] = new double[][] { l[1], t[1], t[2], l[2] };
        quad[SOUTH] = new double[][] { l[0], l[1], l[2], l[3] };

        // Optional cube rotation about its own pivot.
        Mat cubeXf = null;
        double[] cr = Json.nums(rc.get("rotation"), 3, null);
        if (cr != null && (cr[0] != 0 || cr[1] != 0 || cr[2] != 0)) {
            double[] cp = Json.nums(rc.get("pivot"), 3, new double[3]);
            cubeXf = Mat.about(Mat.bedrockRotation(cr[0], cr[1], cr[2]), cp[0], 24 - cp[1], cp[2]);
        }

        int f = 0;
        double[] tmp = new double[3];
        for (int face = 0; face < 6; face++) {
            if (!present[face] || rect[face] == null) continue;
            double[] r = rect[face];
            double fu0 = r[0] / texW, fv0 = r[1] / texH, fu1 = r[2] / texW, fv1 = r[3] / texH;
            // Polygon(): vertex 0 gets (u1, v0), 1 gets (u0, v0), 2 gets (u0, v1), 3 gets (u1, v1).
            double[][] uvs = { { fu1, fv0 }, { fu0, fv0 }, { fu0, fv1 }, { fu1, fv1 } };
            double[][] vs = new double[4][];
            for (int k = 0; k < 4; k++) vs[k] = quad[face][k];
            int facing = face;
            if (mirror) {
                // Mirrored boxes swap the x-facing normals and reverse vertex order.
                if (face == WEST) facing = EAST; else if (face == EAST) facing = WEST;
                // ModelPart.Polygon reverses the vertex array after UVs are assigned, so each
                // UV travels with its vertex.
                double[][] rv = new double[4][];
                double[][] ru = new double[4][];
                for (int k = 0; k < 4; k++) { rv[k] = vs[3 - k]; ru[k] = uvs[3 - k]; }
                vs = rv;
                uvs = ru;
            }
            for (int k = 0; k < 4; k++) {
                double[] p = vs[k];
                double px = p[0], py = p[1], pz = p[2];
                if (cubeXf != null) { cubeXf.point(px, py, pz, tmp); px = tmp[0]; py = tmp[1]; pz = tmp[2]; }
                cube.pos[(f * 4 + k) * 3] = (float) px;
                cube.pos[(f * 4 + k) * 3 + 1] = (float) py;
                cube.pos[(f * 4 + k) * 3 + 2] = (float) pz;
                cube.uv[(f * 4 + k) * 2] = (float) uvs[k][0];
                cube.uv[(f * 4 + k) * 2 + 1] = (float) uvs[k][1];
            }
            double nx = DIR[facing][0], ny = DIR[facing][1], nz = DIR[facing][2];
            if (cubeXf != null) { cubeXf.dir(nx, ny, nz, tmp); nx = tmp[0]; ny = tmp[1]; nz = tmp[2]; }
            cube.normal[f * 3] = (float) nx;
            cube.normal[f * 3 + 1] = (float) ny;
            cube.normal[f * 3 + 2] = (float) nz;
            f++;
        }
        cube.faces = f;
        return cube;
    }
}
