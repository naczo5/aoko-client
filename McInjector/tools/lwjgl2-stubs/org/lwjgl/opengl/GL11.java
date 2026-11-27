package org.lwjgl.opengl;

import java.nio.ByteBuffer;
import java.nio.FloatBuffer;

/** Compile-time stub of the LWJGL 2 members the visuals helper uses. Never embedded. */
public final class GL11 {
    private GL11() {}
    public static final int GL_ALPHA_TEST = 0x0BC0;
    public static final int GL_ALPHA_TEST_FUNC = 0x0BC1;
    public static final int GL_ALPHA_TEST_REF = 0x0BC2;
    public static final int GL_BLEND = 0x0BE2;
    public static final int GL_CULL_FACE = 0x0B44;
    public static final int GL_CURRENT_COLOR = 0x0B00;
    public static final int GL_DEPTH_TEST = 0x0B71;
    public static final int GL_DEPTH_WRITEMASK = 0x0B72;
    public static final int GL_FOG = 0x0B60;
    public static final int GL_GREATER = 0x0204;
    public static final int GL_LIGHTING = 0x0B50;
    public static final int GL_LINEAR = 0x2601;
    public static final int GL_NEAREST = 0x2600;
    public static final int GL_NO_ERROR = 0;
    public static final int GL_ONE = 1;
    public static final int GL_ONE_MINUS_SRC_ALPHA = 0x0303;
    public static final int GL_QUADS = 0x0007;
    public static final int GL_RGBA = 0x1908;
    public static final int GL_RGBA8 = 0x8058;
    public static final int GL_SRC_ALPHA = 0x0302;
    public static final int GL_TEXTURE_2D = 0x0DE1;
    public static final int GL_TEXTURE_BINDING_2D = 0x8069;
    public static final int GL_TEXTURE_MAG_FILTER = 0x2800;
    public static final int GL_TEXTURE_MIN_FILTER = 0x2801;
    public static final int GL_TEXTURE_WRAP_S = 0x2802;
    public static final int GL_TEXTURE_WRAP_T = 0x2803;
    public static final int GL_UNPACK_ALIGNMENT = 0x0CF5;
    public static final int GL_UNPACK_ROW_LENGTH = 0x0CF2;
    public static final int GL_UNPACK_SKIP_PIXELS = 0x0CF4;
    public static final int GL_UNPACK_SKIP_ROWS = 0x0CF3;
    public static final int GL_UNSIGNED_BYTE = 0x1401;
    public static final int GL_ZERO = 0;

    public static void glAlphaFunc(int func, float ref) { throw new UnsupportedOperationException(); }
    public static void glBegin(int mode) { throw new UnsupportedOperationException(); }
    public static void glBindTexture(int target, int texture) { throw new UnsupportedOperationException(); }
    public static void glColor4f(float r, float g, float b, float a) { throw new UnsupportedOperationException(); }
    public static void glDeleteTextures(int texture) { throw new UnsupportedOperationException(); }
    public static void glDepthMask(boolean flag) { throw new UnsupportedOperationException(); }
    public static void glDisable(int cap) { throw new UnsupportedOperationException(); }
    public static void glEnable(int cap) { throw new UnsupportedOperationException(); }
    public static void glEnd() { throw new UnsupportedOperationException(); }
    public static int glGenTextures() { throw new UnsupportedOperationException(); }
    public static boolean glGetBoolean(int pname) { throw new UnsupportedOperationException(); }
    public static int glGetError() { throw new UnsupportedOperationException(); }
    public static float glGetFloat(int pname) { throw new UnsupportedOperationException(); }
    public static void glGetFloat(int pname, FloatBuffer params) { throw new UnsupportedOperationException(); }
    public static int glGetInteger(int pname) { throw new UnsupportedOperationException(); }
    public static boolean glIsEnabled(int cap) { throw new UnsupportedOperationException(); }
    public static void glNormal3f(float x, float y, float z) { throw new UnsupportedOperationException(); }
    public static void glPixelStorei(int pname, int param) { throw new UnsupportedOperationException(); }
    public static void glPopMatrix() { throw new UnsupportedOperationException(); }
    public static void glPushMatrix() { throw new UnsupportedOperationException(); }
    public static void glRotatef(float angle, float x, float y, float z) { throw new UnsupportedOperationException(); }
    public static void glTexCoord2f(float s, float t) { throw new UnsupportedOperationException(); }
    public static void glTexImage2D(int target, int level, int internalformat, int width, int height, int border, int format, int type, ByteBuffer pixels) { throw new UnsupportedOperationException(); }
    public static void glTexParameteri(int target, int pname, int param) { throw new UnsupportedOperationException(); }
    public static void glTranslatef(float x, float y, float z) { throw new UnsupportedOperationException(); }
    public static void glVertex3f(float x, float y, float z) { throw new UnsupportedOperationException(); }
}
