package com.aoko.visuals;

import java.awt.image.BufferedImage;
import java.io.File;
import java.util.ArrayList;
import java.util.List;

/**
 * Platform independent checks for the visuals helper: JSON reader, geometry and UV layout,
 * rotation convention, sky maths, cube sky layout, and the shipped built-in assets.
 * Plain JVM, no Minecraft. Run through run_visuals_tests.sh.
 */
public final class VisualsCoreTest {
    static int failures;
    static int checks;

    static void check(boolean ok, String what) {
        checks++;
        if (!ok) { failures++; System.err.println("FAIL " + what); }
    }

    static void near(double expected, double actual, double eps, String what) {
        check(Math.abs(expected - actual) <= eps, what + ": expected " + expected + " got " + actual);
    }

    public static void main(String[] args) throws Exception {
        File assets = new File(args.length > 0 ? args[0] : "../Aoko/Assets/Visuals");
        jsonTests();
        cubeMapTests();
        geometryTests();
        rotationTests();
        skyMathTests();
        configTests();
        shippedAssetTests(assets);
        if (failures != 0) {
            System.err.println(failures + " of " + checks + " checks failed");
            System.exit(1);
        }
        System.out.println("VisualsCoreTest OK (" + checks + " checks)");
    }

    // ---------------------------------------------------------------------------------------

    static void jsonTests() throws Exception {
        Object o = Json.parse("{\"a\":[1,2.5,-3e2],\"b\":{\"c\":\"x\\u0041\\n\"},\"d\":true,\"e\":null}");
        java.util.Map<String, Object> m = Json.obj(o);
        check(Json.arr(m.get("a")).size() == 3, "json array size");
        near(-300, ((Double) Json.arr(m.get("a")).get(2)).doubleValue(), 0, "json exponent");
        check("xA\n".equals(Json.str(Json.obj(m.get("b")), "c", null)), "json string escapes");
        check(Json.bool(m, "d", false) && m.containsKey("e") && m.get("e") == null, "json literals");
        String[] bad = { "", "{", "{\"a\":}", "[1,]", "{\"a\":1,}", "tru", "\"\\x\"", "1 2", "{\"a\":NaN}", "[1e999]" };
        for (String s : bad) {
            try { Json.parse(s); check(false, "json should reject " + s); } catch (Json.ParseException e) { check(true, ""); }
        }
        StringBuilder deep = new StringBuilder();
        for (int i = 0; i < 100; i++) deep.append('[');
        try { Json.parse(deep.toString()); check(false, "json depth limit"); } catch (Json.ParseException e) { check(true, ""); }
        check("\"a\\\"b\\\\\"".equals(Json.quote("a\"b\\")), "json quote");
    }

    static void cubeMapTests() {
        double[] d = new double[3];
        // Each face centre points along its axis and every direction maps back to the same face and texel.
        double[][] axes = { { 1, 0, 0 }, { -1, 0, 0 }, { 0, 1, 0 }, { 0, -1, 0 }, { 0, 0, 1 }, { 0, 0, -1 } };
        for (int f = 0; f < 6; f++) {
            CubeMap.direction(f, 0.5, 0.5, d);
            check(d[0] == axes[f][0] && d[1] == axes[f][1] && d[2] == axes[f][2], "face centre " + CubeMap.NAMES[f]);
        }
        java.util.Random r = new java.util.Random(7);
        for (int i = 0; i < 2000; i++) {
            double x = r.nextGaussian(), y = r.nextGaussian(), z = r.nextGaussian();
            double ax = Math.abs(x), ay = Math.abs(y), az = Math.abs(z);
            int face; double sc, tc, ma;
            if (ax >= ay && ax >= az) { face = x > 0 ? 0 : 1; ma = ax; sc = x > 0 ? -z : z; tc = -y; }
            else if (ay >= az) { face = y > 0 ? 2 : 3; ma = ay; sc = x; tc = y > 0 ? z : -z; }
            else { face = z > 0 ? 4 : 5; ma = az; sc = z > 0 ? x : -x; tc = -y; }
            double u = (sc / ma + 1) / 2, v = (tc / ma + 1) / 2;
            CubeMap.direction(face, u, v, d);
            double len = Math.sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]), n = Math.sqrt(x * x + y * y + z * z);
            check(Math.abs(d[0] / len - x / n) < 1e-9 && Math.abs(d[1] / len - y / n) < 1e-9 && Math.abs(d[2] / len - z / n) < 1e-9,
                "cube map round trip");
        }
        // Adjacent faces share an edge: the +X face's right edge (u=1) meets the -Z face's left edge (u=0).
        double[] a = new double[3], b = new double[3];
        CubeMap.direction(0, 1, 0.3, a);
        CubeMap.direction(5, 0, 0.3, b);
        near(a[0] / Math.abs(a[2]), b[0] / Math.abs(b[2]), 1e-9, "+X/-Z seam x");
        near(a[1] / Math.abs(a[2]), b[1] / Math.abs(b[2]), 1e-9, "+X/-Z seam y");
        float[] uv = new float[8];
        CubeMap.atlasUv(2, 1536, 1024, uv);          // +Y: column 2, row 0
        check(uv[0] > 2f / 3f && uv[2] < 1f && uv[1] > 0f && uv[5] < 0.5f, "atlas cell for +Y");
    }

    static void geometryTests() throws Exception {
        // A 2x3x4 cube at origin (1, 20, 3), uv (8, 16), texture 64x64.
        String geo = "{\"format_version\":\"1.12.0\",\"minecraft:geometry\":[{\"description\":{\"texture_width\":64,\"texture_height\":64},"
            + "\"bones\":[{\"name\":\"b\",\"pivot\":[0,24,0],\"cubes\":[{\"origin\":[1,20,3],\"size\":[2,3,4],\"uv\":[8,16]}]}]}]}";
        GeoModel m = GeoModel.parse(geo);
        GeoModel.Cube c = m.bones[0].cubes.get(0);
        check(c.faces == 6, "six faces");
        // Java min y = 24 - (20 + 3) = 1. DOWN face (index 0) sits at minY.
        double minY = 24 - (20 + 3);
        for (int k = 0; k < 4; k++) near(minY, c.pos[k * 3 + 1], 1e-6, "DOWN face y");
        // DOWN vertex 0 is l1 = (maxX, minY, maxZ) with uv (u2, v0)/64: u2 = 8 + d + w = 14, v0 = 16.
        near(1 + 2, c.pos[0], 1e-6, "DOWN v0 x");
        near(3 + 4, c.pos[2], 1e-6, "DOWN v0 z");
        near(14.0 / 64, c.uv[0], 1e-6, "DOWN v0 u");
        near(16.0 / 64, c.uv[1], 1e-6, "DOWN v0 v");
        // NORTH (index 3) at z = minZ, normal (0,0,-1); SOUTH at maxZ.
        near(-1, c.normal[3 * 3 + 2], 0, "NORTH normal");
        near(3, c.pos[(3 * 4) * 3 + 2], 1e-6, "NORTH z");
        near(7, c.pos[(5 * 4) * 3 + 2], 1e-6, "SOUTH z");
        // Mirror swaps the x-facing normals.
        GeoModel mm = GeoModel.parse(geo.replace("\"uv\":[8,16]", "\"uv\":[8,16],\"mirror\":true"));
        GeoModel.Cube mc = mm.bones[0].cubes.get(0);
        near(1, mc.normal[2 * 3], 0, "mirrored WEST normal flips to +x");
        near(-1, mc.normal[4 * 3], 0, "mirrored EAST normal flips to -x");

        // Per-face uv: only the faces given produce quads.
        String faceGeo = "{\"minecraft:geometry\":[{\"bones\":[{\"name\":\"b\",\"cubes\":[{\"origin\":[0,0,0],\"size\":[1,1,1],"
            + "\"uv\":{\"north\":{\"uv\":[0,0],\"uv_size\":[4,4]},\"up\":{\"uv\":[4,0],\"uv_size\":[4,4]}}}]}]}]}";
        GeoModel fm = GeoModel.parse(faceGeo);
        check(fm.bones[0].cubes.get(0).faces == 2, "per-face uv emits two faces");

        String[] bad = {
            "{}", "{\"minecraft:geometry\":[]}",
            "{\"minecraft:geometry\":[{\"bones\":[{\"name\":\"a\"},{\"name\":\"a\"}]}]}",
            "{\"minecraft:geometry\":[{\"bones\":[{\"name\":\"a\",\"parent\":\"zz\"}]}]}",
            "{\"minecraft:geometry\":[{\"bones\":[{\"name\":\"a\",\"parent\":\"b\"},{\"name\":\"b\",\"parent\":\"a\"}]}]}",
            "{\"minecraft:geometry\":[{\"bones\":[{\"name\":\"a\",\"cubes\":[{\"origin\":[0,0,0],\"size\":[1,1,1]}]}]}]}",
            "{\"minecraft:geometry\":[{\"bones\":[{\"name\":\"a\",\"cubes\":[{\"origin\":[9999,0,0],\"size\":[1,1,1],\"uv\":[0,0]}]}]}]}"
        };
        for (String s : bad) {
            try { GeoModel.parse(s); check(false, "geometry should reject " + s); } catch (Json.ParseException e) { check(true, ""); }
        }
        StringBuilder many = new StringBuilder("{\"minecraft:geometry\":[{\"bones\":[");
        for (int i = 0; i < 65; i++) many.append(i == 0 ? "" : ",").append("{\"name\":\"b").append(i).append("\"}");
        many.append("]}]}");
        try { GeoModel.parse(many.toString()); check(false, "bone limit"); } catch (Json.ParseException e) { check(true, ""); }
    }

    static void rotationTests() {
        // Rotating +x by 90 degrees about Bedrock z (y up): x -> y in Bedrock, i.e. y -> -y in Java space.
        Mat r = Mat.bedrockRotation(0, 0, 90);
        double[] p = new double[3];
        r.point(1, 0, 0, p);
        near(0, p[0], 1e-9, "rz90 x"); near(-1, p[1], 1e-9, "rz90 y (Java y is down)"); near(0, p[2], 1e-9, "rz90 z");
        // Rotating +z by 90 degrees about Bedrock y: z -> x.
        r = Mat.bedrockRotation(0, 90, 0);
        r.point(0, 0, 1, p);
        near(1, p[0], 1e-9, "ry90 x"); near(0, p[2], 1e-9, "ry90 z");
        // About a pivot the pivot stays put.
        Mat about = Mat.about(Mat.bedrockRotation(30, 40, 50), 3, -2, 5);
        about.point(3, -2, 5, p);
        near(3, p[0], 1e-9, "pivot x"); near(-2, p[1], 1e-9, "pivot y"); near(5, p[2], 1e-9, "pivot z");
        // Order: x first, then z. +y rotated 90 about x goes to +z (Bedrock), then 90 about z leaves +z alone.
        r = Mat.bedrockRotation(90, 0, 90);
        r.point(0, 1, 0, p);
        // Java space: Rx(-90) maps +y -> -z; Rz(-90) leaves z.
        near(0, p[0], 1e-9, "rxz order x"); near(-1, p[2], 1e-9, "rxz order z");
    }

    static void skyMathTests() {
        near(0.0, SkyPreset.celestialAngleForTick(6000), 1e-9, "noon angle");
        near(0.5, SkyPreset.celestialAngleForTick(18000), 1e-9, "midnight angle");
        check(Math.abs(SkyPreset.celestialAngleForTick(0) - 0.7845) < 0.001, "sunrise angle ~0.7845");
        near(SkyPreset.celestialAngleForTick(30000), SkyPreset.celestialAngleForTick(6000), 1e-9, "tick wraps");
        near(1.0, SkyPreset.heightOfAngle(0), 1e-12, "noon height");
        near(-1.0, SkyPreset.heightOfAngle(0.5), 1e-12, "midnight height");
        near(SkyPreset.heightOfAngle(0.25), SkyPreset.heightOfDegrees(90), 1e-12, "degrees match angle");

        SkyPreset c = SkyPreset.classic();
        float[] noon = c.sky(1), night = c.sky(-1), dusk = c.sky(0);
        check(noon[2] > noon[0], "classic noon sky is blue");
        check(night[0] < 0.1f && night[1] < 0.1f, "classic night sky is dark");
        check(dusk[0] > Math.min(noon[0], night[0]) - 1e-6, "dusk mixes towards the sunset colour");
        check(c.sunset(0.9) == null, "no sunset glow at noon");
        float[] glow = c.sunset(0);
        check(glow != null && glow[3] > 0.9f, "full glow at the horizon");
        check(c.stars(1) == 0f && c.stars(-1) > 0.4f, "stars only at night");
        float[] dim = SkyPreset.weatherDim(new float[] { 1, 1, 1 }, 1, 0, 0.75f);
        check(dim[0] < 1f, "rain dims the sky");
        // Colours are continuous in sun height.
        float[] prev = c.fog(1);
        double worst = 0;
        for (double h = 1; h >= -1; h -= 0.01) {
            float[] f = c.fog(h);
            for (int k = 0; k < 3; k++) worst = Math.max(worst, Math.abs(f[k] - prev[k]));
            prev = f;
        }
        check(worst < 0.08, "fog colour changes smoothly (max step " + worst + ")");
    }

    static void configTests() throws Exception {
        Config c = Config.parse("{\"cosmetics\":true,\"cape\":\" Aoko-Glass \",\"hat\":\"../x\",\"sky\":true,\"preset\":\"tsukihime\","
            + "\"time\":99999,\"clear\":true,\"dir\":\"C:\\\\a\"}");
        check(c.cosmetics && c.sky && c.clearWeather, "config flags");
        check("aoko-glass".equals(c.cape), "ids are trimmed and lower-cased");
        check(c.hat.length() == 0, "bad id is dropped");
        check(c.time == 23999, "time clamps");
        check("C:\\a".equals(c.dir), "dir passes through");
        check(Config.parse("{\"time\":-9}").time == -2, "time clamps low");
        check(Config.parse("{}").sameAs(Config.OFF), "empty config equals off");
        try { Config.parse("[]"); check(false, "config must be an object"); } catch (Json.ParseException e) { check(true, ""); }

        SkyState s = SkyState.of(Config.parse("{\"sky\":true,\"time\":6000}"), null);
        check(s.colors && s.locked() && s.preset != null, "time lock without a preset falls back to classic colours");
        near(0, s.angle, 1e-9, "locked angle");
        check(!SkyState.of(Config.parse("{\"sky\":true}"), null).active(), "sky on with nothing selected does nothing");
        check(SkyState.of(Config.parse("{\"sky\":true,\"clear\":true}"), null).clear, "clear weather alone is active");
        check(SkyState.OFF == SkyState.of(Config.parse("{\"time\":6000}"), null), "disabled sky is off");
    }

    // ---------------------------------------------------------------------------------------

    static void shippedAssetTests(File root) throws Exception {
        if (!root.isDirectory()) { check(false, "assets directory missing: " + root.getAbsolutePath()); return; }
        Library lib = new Library(root);
        File[] capes = new File(root, "capes").listFiles();
        check(capes != null && capes.length >= 5, "at least five capes ship");
        for (File f : capes) {
            String id = f.getName().replace(".png", "");
            check(Config.ID.matcher(id).matches(), "cape id valid: " + id);
            BufferedImage img = lib.loadImage(f);
            check(Library.validCape(img), "cape layout valid: " + id + " " + img.getWidth() + "x" + img.getHeight());
        }
        File[] skies = new File(root, "skies").listFiles();
        check(skies != null && skies.length >= 5, "at least five skies ship");
        int textured = 0;
        for (File d : skies) {
            SkyPreset p = lib.loadSky(d.getName());
            if (p.skyboxFile != null) {
                textured++;
                BufferedImage atlas = lib.loadImage(new File(d, p.skyboxFile));
                check(Library.validAtlas(atlas), "sky atlas valid: " + d.getName());
            }
            // Evaluate across the day: all colours finite and in range.
            for (double h = -1; h <= 1; h += 0.1) {
                for (float[] c : new float[][] { p.sky(h), p.fog(h), p.cloud(h) })
                    for (float v : c) check(v >= 0 && v <= 1.0001f, "sky colour in range: " + d.getName());
            }
        }
        check(textured >= 3, "textured skies ship");
        File[] cos = new File(root, "cosmetics").listFiles();
        check(cos != null && cos.length >= 3, "at least three cosmetics ship");
        List<String> slots = new ArrayList<String>();
        for (File d : cos) {
            Cosmetic c = lib.loadCosmetic(d.getName());
            slots.add(c.slot);
            BufferedImage tex = lib.loadImage(new File(d, c.textureFile));
            check(tex.getWidth() > 0, "cosmetic texture readable: " + c.id);
            final double[] min = { 1e9, 1e9, 1e9 }, max = { -1e9, -1e9, -1e9 };
            final int[] quads = { 0 };
            for (double t : new double[] { 0, 0.37, 1.9, 7.2 }) {
                c.emit(t, new QuadSink() {
                    public void quad(float[] q) {
                        quads[0]++;
                        for (int k = 0; k < 4; k++) {
                            for (int a = 0; a < 3; a++) { min[a] = Math.min(min[a], q[k * 8 + a]); max[a] = Math.max(max[a], q[k * 8 + a]); }
                            double nl = Math.sqrt(q[k * 8 + 3] * q[k * 8 + 3] + q[k * 8 + 4] * q[k * 8 + 4] + q[k * 8 + 5] * q[k * 8 + 5]);
                            check(Math.abs(nl - 1) < 1e-4, "unit normal in " + c.id);
                            check(q[k * 8 + 6] >= -1e-4 && q[k * 8 + 6] <= 1.0001 && q[k * 8 + 7] >= -1e-4 && q[k * 8 + 7] <= 1.0001,
                                "uv inside texture in " + c.id);
                        }
                    }
                });
            }
            check(quads[0] > 0, "cosmetic emits quads: " + c.id);
            for (int a = 0; a < 3; a++) check(min[a] > -3 && max[a] < 3, "cosmetic stays near the player: " + c.id + " axis " + a + " " + min[a] + ".." + max[a]);
            if ("head".equals(c.anchor)) {
                // Java y is down, head spans y -0.5..0 blocks; a hat must reach above the crown.
                check(min[1] < -0.45, "head cosmetic reaches the crown: " + c.id + " minY " + min[1]);
            }
            if ("body".equals(c.anchor)) {
                // The back of the body is +z; wings must sit behind it (z > 0.125).
                check(max[2] > 0.15, "back cosmetic sits behind the body: " + c.id + " maxZ " + max[2]);
            }
        }
        check(slots.contains("hat") && slots.contains("back"), "both slots covered by built-ins");
    }
}
