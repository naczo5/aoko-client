package com.aoko.visuals;

import java.awt.image.BufferedImage;
import java.nio.ByteBuffer;
import org.lwjgl.BufferUtils;
import org.lwjgl.opengl.GL11;
import org.lwjgl.opengl.GL12;
import org.lwjgl.opengl.GL13;
import org.lwjgl.opengl.GL14;

/**
 * Fixed-function OpenGL for 1.8.9 (LWJGL 2). Every routine saves the state it touches and
 * puts it back exactly, so the game's GlStateManager cache stays truthful. Only call on the
 * render thread with a current context.
 */
final class LegacyGl {
    private LegacyGl() {}

    /** Render thread only. glGetFloat needs a 16 element buffer. */
    private static final java.nio.FloatBuffer COLOR_SCRATCH = BufferUtils.createFloatBuffer(16);

    /** Snapshot of the state a draw call changes. */
    private static final class Saved {
        boolean blend, alpha, cull, lighting, tex2d, fog, depthTest, depthMask, tex2dUnit1;
        int bound, activeUnit, alphaFunc, srcRgb, dstRgb, srcA, dstA;
        float alphaRef;
        final float[] color = new float[4];

        static Saved capture() {
            Saved s = new Saved();
            s.blend = GL11.glIsEnabled(GL11.GL_BLEND);
            s.alpha = GL11.glIsEnabled(GL11.GL_ALPHA_TEST);
            s.cull = GL11.glIsEnabled(GL11.GL_CULL_FACE);
            s.lighting = GL11.glIsEnabled(GL11.GL_LIGHTING);
            s.tex2d = GL11.glIsEnabled(GL11.GL_TEXTURE_2D);
            s.fog = GL11.glIsEnabled(GL11.GL_FOG);
            s.depthTest = GL11.glIsEnabled(GL11.GL_DEPTH_TEST);
            s.depthMask = GL11.glGetBoolean(GL11.GL_DEPTH_WRITEMASK);
            s.bound = GL11.glGetInteger(GL11.GL_TEXTURE_BINDING_2D);
            s.activeUnit = GL11.glGetInteger(GL13.GL_ACTIVE_TEXTURE);
            GL13.glActiveTexture(GL13.GL_TEXTURE1);
            s.tex2dUnit1 = GL11.glIsEnabled(GL11.GL_TEXTURE_2D);
            GL13.glActiveTexture(s.activeUnit);
            s.alphaFunc = GL11.glGetInteger(GL11.GL_ALPHA_TEST_FUNC);
            s.alphaRef = GL11.glGetFloat(GL11.GL_ALPHA_TEST_REF);
            s.srcRgb = GL11.glGetInteger(GL14.GL_BLEND_SRC_RGB);
            s.dstRgb = GL11.glGetInteger(GL14.GL_BLEND_DST_RGB);
            s.srcA = GL11.glGetInteger(GL14.GL_BLEND_SRC_ALPHA);
            s.dstA = GL11.glGetInteger(GL14.GL_BLEND_DST_ALPHA);
            java.nio.FloatBuffer buf = COLOR_SCRATCH;
            buf.clear();
            GL11.glGetFloat(GL11.GL_CURRENT_COLOR, buf);
            s.color[0] = buf.get(0); s.color[1] = buf.get(1); s.color[2] = buf.get(2); s.color[3] = buf.get(3);
            return s;
        }

        void restore() {
            GL13.glActiveTexture(GL13.GL_TEXTURE1);
            set(GL11.GL_TEXTURE_2D, tex2dUnit1);
            GL13.glActiveTexture(GL13.GL_TEXTURE0);
            GL11.glBindTexture(GL11.GL_TEXTURE_2D, bound);
            set(GL11.GL_TEXTURE_2D, tex2d);
            GL14.glBlendFuncSeparate(srcRgb, dstRgb, srcA, dstA);
            GL11.glAlphaFunc(alphaFunc, alphaRef);
            set(GL11.GL_BLEND, blend);
            set(GL11.GL_ALPHA_TEST, alpha);
            set(GL11.GL_CULL_FACE, cull);
            set(GL11.GL_LIGHTING, lighting);
            set(GL11.GL_FOG, fog);
            set(GL11.GL_DEPTH_TEST, depthTest);
            GL11.glDepthMask(depthMask);
            GL11.glColor4f(color[0], color[1], color[2], color[3]);
            GL13.glActiveTexture(activeUnit);
        }
    }

    private static void set(int cap, boolean on) {
        if (on) GL11.glEnable(cap); else GL11.glDisable(cap);
    }

    /** Turns lightmap texturing (unit 1) off; {@link Saved#restore()} puts it back. */
    private static void lightmapOff() {
        GL13.glActiveTexture(GL13.GL_TEXTURE1);
        GL11.glDisable(GL11.GL_TEXTURE_2D);
        GL13.glActiveTexture(GL13.GL_TEXTURE0);
    }

    static void push() { GL11.glPushMatrix(); }

    static void pop() { GL11.glPopMatrix(); }

    static void translate(float x, float y, float z) { GL11.glTranslatef(x, y, z); }

    static void rotate(float degrees, float x, float y, float z) { GL11.glRotatef(degrees, x, y, z); }

    // ---- Textures ----------------------------------------------------------------------

    /** RGBA pixels ready for upload. Built off the render thread; needs no GL context. */
    static final class Pixels {
        final int width, height;
        final ByteBuffer data;
        Pixels(int width, int height, ByteBuffer data) { this.width = width; this.height = height; this.data = data; }
    }

    static Pixels prepare(BufferedImage img) {
        int w = img.getWidth(), h = img.getHeight();
        int[] px = new int[w * h];
        img.getRGB(0, 0, w, h, px, 0, w);
        ByteBuffer buf = BufferUtils.createByteBuffer(w * h * 4);
        for (int i = 0; i < px.length; i++) {
            int p = px[i];
            buf.put((byte) (p >> 16)).put((byte) (p >> 8)).put((byte) p).put((byte) (p >>> 24));
        }
        buf.flip();
        return new Pixels(w, h, buf);
    }

    /** Uploads prepared pixels as an RGBA texture (clamped, no mipmaps). Returns the GL id, or 0 on failure. */
    static int upload(Pixels pixels, boolean linear) {
        ByteBuffer buf = pixels.data.duplicate();
        int prevUnit = GL11.glGetInteger(GL13.GL_ACTIVE_TEXTURE);
        GL13.glActiveTexture(GL13.GL_TEXTURE0);
        int prev = GL11.glGetInteger(GL11.GL_TEXTURE_BINDING_2D);
        // An error the game or another mod left pending must not make a good upload look failed.
        for (int i = 0; i < 16 && GL11.glGetError() != GL11.GL_NO_ERROR; i++) { /* drain */ }
        int id = GL11.glGenTextures();
        try {
            GL11.glBindTexture(GL11.GL_TEXTURE_2D, id);
            int filter = linear ? GL11.GL_LINEAR : GL11.GL_NEAREST;
            GL11.glTexParameteri(GL11.GL_TEXTURE_2D, GL11.GL_TEXTURE_MIN_FILTER, filter);
            GL11.glTexParameteri(GL11.GL_TEXTURE_2D, GL11.GL_TEXTURE_MAG_FILTER, filter);
            GL11.glTexParameteri(GL11.GL_TEXTURE_2D, GL11.GL_TEXTURE_WRAP_S, GL12.GL_CLAMP_TO_EDGE);
            GL11.glTexParameteri(GL11.GL_TEXTURE_2D, GL11.GL_TEXTURE_WRAP_T, GL12.GL_CLAMP_TO_EDGE);
            GL11.glPixelStorei(GL11.GL_UNPACK_ROW_LENGTH, 0);
            GL11.glPixelStorei(GL11.GL_UNPACK_SKIP_ROWS, 0);
            GL11.glPixelStorei(GL11.GL_UNPACK_SKIP_PIXELS, 0);
            GL11.glPixelStorei(GL11.GL_UNPACK_ALIGNMENT, 4);
            GL11.glTexImage2D(GL11.GL_TEXTURE_2D, 0, GL11.GL_RGBA8, pixels.width, pixels.height, 0, GL11.GL_RGBA,
                GL11.GL_UNSIGNED_BYTE, buf);
            if (GL11.glGetError() != GL11.GL_NO_ERROR) {
                GL11.glDeleteTextures(id);
                id = 0;
            }
        } finally {
            GL11.glBindTexture(GL11.GL_TEXTURE_2D, prev);
            GL13.glActiveTexture(prevUnit);
        }
        return id;
    }

    static void delete(int id) {
        if (id != 0) GL11.glDeleteTextures(id);
    }

    // ---- Cosmetics -----------------------------------------------------------------------

    private static final class GlSink implements QuadSink {
        public void quad(float[] q) {
            for (int k = 0; k < 4; k++) {
                int o = k * 8;
                GL11.glNormal3f(q[o + 3], q[o + 4], q[o + 5]);
                GL11.glTexCoord2f(q[o + 6], q[o + 7]);
                GL11.glVertex3f(q[o], q[o + 1], q[o + 2]);
            }
        }
    }

    /** Draws one cosmetic at the current matrix (already moved to its anchor). */
    static void drawCosmetic(Cosmetic c, int texture, double seconds) {
        Saved s = Saved.capture();
        try {
            GL13.glActiveTexture(GL13.GL_TEXTURE0);
            GL11.glEnable(GL11.GL_TEXTURE_2D);
            GL11.glBindTexture(GL11.GL_TEXTURE_2D, texture);
            GL11.glDisable(GL11.GL_CULL_FACE);
            GL11.glColor4f(1f, 1f, 1f, 1f);
            if (c.fullbright) {
                GL11.glDisable(GL11.GL_LIGHTING);
                lightmapOff();
            }
            if (c.translucent) {
                GL11.glEnable(GL11.GL_BLEND);
                GL14.glBlendFuncSeparate(GL11.GL_SRC_ALPHA, GL11.GL_ONE_MINUS_SRC_ALPHA, GL11.GL_ONE, GL11.GL_ZERO);
                GL11.glEnable(GL11.GL_ALPHA_TEST);
                GL11.glAlphaFunc(GL11.GL_GREATER, 0.003f);
                GL11.glDepthMask(false);
            } else {
                GL11.glDisable(GL11.GL_BLEND);
                GL11.glEnable(GL11.GL_ALPHA_TEST);
                GL11.glAlphaFunc(GL11.GL_GREATER, 0.5f);
            }
            GL11.glBegin(GL11.GL_QUADS);
            try {
                c.emit(seconds, new GlSink());
            } finally {
                GL11.glEnd();
            }
        } finally {
            s.restore();
        }
    }

    // ---- Sky -------------------------------------------------------------------------------

    /** Draws the cube sky around the origin of the current (rotation only) modelview matrix. */
    static void drawSkybox(int texture, int atlasW, int atlasH, float brightness, float[] spin, double seconds) {
        Saved s = Saved.capture();
        float[] corners = new float[12];
        float[] uvs = new float[8];
        try {
            GL13.glActiveTexture(GL13.GL_TEXTURE0);
            lightmapOff();
            GL11.glEnable(GL11.GL_TEXTURE_2D);
            GL11.glBindTexture(GL11.GL_TEXTURE_2D, texture);
            GL11.glDisable(GL11.GL_FOG);
            GL11.glDisable(GL11.GL_LIGHTING);
            GL11.glDisable(GL11.GL_ALPHA_TEST);
            GL11.glDisable(GL11.GL_BLEND);
            GL11.glDisable(GL11.GL_CULL_FACE);
            GL11.glDisable(GL11.GL_DEPTH_TEST);
            GL11.glDepthMask(false);
            GL11.glColor4f(brightness, brightness, brightness, 1f);
            GL11.glPushMatrix();
            try {
                if (spin[0] != 0) GL11.glRotatef((float) ((spin[0] * seconds) % 360.0), 1f, 0f, 0f);
                if (spin[1] != 0) GL11.glRotatef((float) ((spin[1] * seconds) % 360.0), 0f, 1f, 0f);
                if (spin[2] != 0) GL11.glRotatef((float) ((spin[2] * seconds) % 360.0), 0f, 0f, 1f);
                GL11.glBegin(GL11.GL_QUADS);
                try {
                    for (int face = 0; face < 6; face++) {
                        CubeMap.corners(face, 30f, corners);
                        CubeMap.atlasUv(face, atlasW, atlasH, uvs);
                        for (int k = 0; k < 4; k++) {
                            GL11.glTexCoord2f(uvs[k * 2], uvs[k * 2 + 1]);
                            GL11.glVertex3f(corners[k * 3], corners[k * 3 + 1], corners[k * 3 + 2]);
                        }
                    }
                } finally {
                    GL11.glEnd();
                }
            } finally {
                GL11.glPopMatrix();
            }
        } finally {
            s.restore();
        }
    }
}
