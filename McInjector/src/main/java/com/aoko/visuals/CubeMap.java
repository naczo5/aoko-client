package com.aoko.visuals;

/**
 * Cube sky atlas layout and face geometry. Atlas is 3 x 2 faces:
 * row 0 = +X, -X, +Y; row 1 = -Y, +Z, -Z. A face pixel (u, v) maps to the direction
 * given by the OpenGL cubemap table (see asset-format.md), with +X east, +Y up, +Z south.
 */
final class CubeMap {
    /** Face order matches the atlas cell order. */
    static final String[] NAMES = { "+X", "-X", "+Y", "-Y", "+Z", "-Z" };
    static final int[] COL = { 0, 1, 2, 0, 1, 2 };
    static final int[] ROW = { 0, 0, 0, 1, 1, 1 };

    private CubeMap() {}

    /** Direction for face {@code face} at (u, v) in [0,1], u left to right, v top to bottom. Not normalised. */
    static void direction(int face, double u, double v, double[] out) {
        double sc = 2 * u - 1, tc = 2 * v - 1;
        switch (face) {
            case 0: out[0] = 1;  out[1] = -tc; out[2] = -sc; break;   // +X: sc=-z, tc=-y
            case 1: out[0] = -1; out[1] = -tc; out[2] = sc;  break;   // -X: sc=+z, tc=-y
            case 2: out[0] = sc; out[1] = 1;   out[2] = tc;  break;   // +Y: sc=+x, tc=+z
            case 3: out[0] = sc; out[1] = -1;  out[2] = -tc; break;   // -Y: sc=+x, tc=-z
            case 4: out[0] = sc; out[1] = -tc; out[2] = 1;   break;   // +Z: sc=+x, tc=-y
            default: out[0] = -sc; out[1] = -tc; out[2] = -1; break;  // -Z: sc=-x, tc=-y
        }
    }

    /**
     * Quad corners (a cube of half-size {@code r} around the origin) for a face, in the
     * order (u,v) = (0,0), (1,0), (1,1), (0,1). Writes 12 floats.
     */
    static void corners(int face, float r, float[] out12) {
        double[] d = new double[3];
        double[][] uv = { { 0, 0 }, { 1, 0 }, { 1, 1 }, { 0, 1 } };
        for (int k = 0; k < 4; k++) {
            direction(face, uv[k][0], uv[k][1], d);
            out12[k * 3] = (float) (d[0] * r);
            out12[k * 3 + 1] = (float) (d[1] * r);
            out12[k * 3 + 2] = (float) (d[2] * r);
        }
    }

    /** Atlas texture coordinates of the same four corners, with a half-texel inset against bleeding. */
    static void atlasUv(int face, int atlasW, int atlasH, float[] out8) {
        double cw = atlasW / 3.0, ch = atlasH / 2.0;
        double x0 = COL[face] * cw + 0.5, x1 = (COL[face] + 1) * cw - 0.5;
        double y0 = ROW[face] * ch + 0.5, y1 = (ROW[face] + 1) * ch - 0.5;
        double[][] c = { { x0, y0 }, { x1, y0 }, { x1, y1 }, { x0, y1 } };
        for (int k = 0; k < 4; k++) {
            out8[k * 2] = (float) (c[k][0] / atlasW);
            out8[k * 2 + 1] = (float) (c[k][1] / atlasH);
        }
    }
}
