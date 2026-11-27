package com.aoko.visuals;

/** Affine 3x4 transform (row major: three rows of a 3x3 linear part plus a translation column). */
final class Mat {
    final double[] m = new double[12];

    private Mat() {}

    static Mat identity() {
        Mat r = new Mat();
        r.m[0] = 1; r.m[5] = 1; r.m[10] = 1;
        return r;
    }

    static Mat translation(double x, double y, double z) {
        Mat r = identity();
        r.m[3] = x; r.m[7] = y; r.m[11] = z;
        return r;
    }

    /**
     * Rotation for Bedrock Euler degrees expressed in Java model space.
     *
     * Bedrock composes Rz * Ry * Rx (x applied first) in a y-up frame. Java model space
     * is the same frame with y mirrored (y' = 24 - y), and conjugating by that mirror
     * turns Rx(a) into Rx(-a), Rz(a) into Rz(-a) and leaves Ry alone. So the matrix
     * returned here is Rz(-rz) * Ry(ry) * Rx(-rx).
     */
    static Mat bedrockRotation(double rxDeg, double ryDeg, double rzDeg) {
        double ax = Math.toRadians(-rxDeg), ay = Math.toRadians(ryDeg), az = Math.toRadians(-rzDeg);
        double cx = Math.cos(ax), sx = Math.sin(ax);
        double cy = Math.cos(ay), sy = Math.sin(ay);
        double cz = Math.cos(az), sz = Math.sin(az);
        Mat rx = identity(); rx.m[5] = cx; rx.m[6] = -sx; rx.m[9] = sx; rx.m[10] = cx;
        Mat ry = identity(); ry.m[0] = cy; ry.m[2] = sy; ry.m[8] = -sy; ry.m[10] = cy;
        Mat rz = identity(); rz.m[0] = cz; rz.m[1] = -sz; rz.m[4] = sz; rz.m[5] = cz;
        return rz.mul(ry).mul(rx);
    }

    /** this * o: o is applied first. */
    Mat mul(Mat o) {
        Mat r = new Mat();
        for (int row = 0; row < 3; row++) {
            for (int col = 0; col < 3; col++) {
                r.m[row * 4 + col] = m[row * 4] * o.m[col] + m[row * 4 + 1] * o.m[4 + col] + m[row * 4 + 2] * o.m[8 + col];
            }
            r.m[row * 4 + 3] = m[row * 4] * o.m[3] + m[row * 4 + 1] * o.m[7] + m[row * 4 + 2] * o.m[11] + m[row * 4 + 3];
        }
        return r;
    }

    /** Rotation {@code rot} about the pivot point: T(p) * rot * T(-p). */
    static Mat about(Mat rot, double px, double py, double pz) {
        return translation(px, py, pz).mul(rot).mul(translation(-px, -py, -pz));
    }

    void point(double x, double y, double z, double[] out) {
        out[0] = m[0] * x + m[1] * y + m[2] * z + m[3];
        out[1] = m[4] * x + m[5] * y + m[6] * z + m[7];
        out[2] = m[8] * x + m[9] * y + m[10] * z + m[11];
    }

    void dir(double x, double y, double z, double[] out) {
        out[0] = m[0] * x + m[1] * y + m[2] * z;
        out[1] = m[4] * x + m[5] * y + m[6] * z;
        out[2] = m[8] * x + m[9] * y + m[10] * z;
    }
}
