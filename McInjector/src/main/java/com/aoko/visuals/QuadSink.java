package com.aoko.visuals;

/** Receives textured quads in blocks, in the anchor's local Java model space. */
interface QuadSink {
    /** Four vertices of eight floats each: x, y, z, nx, ny, nz, u, v. The array is reused by the caller. */
    void quad(float[] vertices);
}
