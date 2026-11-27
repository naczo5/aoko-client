import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.InputStream;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.util.Arrays;

/**
 * Loads the original and the rewritten fx.Fx side by side (the JVM verifies the
 * rewritten bytes while defining the class) and checks that the guard/value prologue
 * behaves: the original path is untouched when the guard is false, and the helper value
 * is returned when the guard is true.
 *
 * Usage: HookFixtureTest <originalDir> <hookedDir>
 */
public final class HookFixtureTest {
    static final class Loader extends ClassLoader {
        private final byte[] fxBytes;
        Loader(ClassLoader parent, byte[] fxBytes) { super(parent); this.fxBytes = fxBytes; }
        @Override protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
            if (name.equals("fx.Fx")) {
                synchronized (getClassLoadingLock(name)) {
                    Class<?> c = findLoadedClass(name);
                    if (c == null) c = defineClass(name, fxBytes, 0, fxBytes.length);
                    if (resolve) resolveClass(c);
                    return c;
                }
            }
            return super.loadClass(name, resolve);
        }
    }

    static int failures;

    static void check(String what, Object expected, Object actual) {
        boolean same = expected == null ? actual == null
            : expected.getClass().isArray()
                ? actual != null && actual.getClass().isArray() && arrayEq(expected, actual)
                : expected.equals(actual);
        if (!same) {
            failures++;
            System.err.println("FAIL " + what + ": expected " + show(expected) + " got " + show(actual));
        }
    }

    static boolean arrayEq(Object a, Object b) {
        if (a instanceof float[] && b instanceof float[]) return Arrays.equals((float[]) a, (float[]) b);
        return false;
    }

    static String show(Object o) {
        if (o instanceof float[]) return Arrays.toString((float[]) o);
        return String.valueOf(o);
    }

    static byte[] read(File f) throws Exception { return Files.readAllBytes(f.toPath()); }

    static Method m(Class<?> c, String name, Class<?>... p) throws Exception { return c.getMethod(name, p); }

    public static void main(String[] args) throws Exception {
        byte[] orig = read(new File(args[0], "fx/Fx.class"));
        byte[] hooked = read(new File(args[1], "fx/Fx.class"));
        ClassLoader parent = HookFixtureTest.class.getClassLoader();
        Class<?> o = new Loader(parent, orig).loadClass("fx.Fx");
        Class<?> h = new Loader(parent, hooked).loadClass("fx.Fx");
        Class<?> hooks = parent.loadClass("fx.Hooks");
        java.lang.reflect.Field on = hooks.getField("on");
        java.lang.reflect.Field calls = hooks.getField("calls");
        Object oi = o.newInstance();
        Object hi = h.newInstance();

        for (int pass = 0; pass < 2; pass++) {
            boolean enabled = pass == 1;
            on.setBoolean(null, enabled);
            int before = calls.getInt(null);
            int invoked = 0;

            for (int a : new int[] { -3, 0, 7 }) for (long b : new long[] { 0L, 5L, -9L }) for (Object c : new Object[] { null, "x" }) {
                invoked++;
                check("sInt(" + a + "," + b + "," + c + ")",
                    enabled ? Integer.valueOf(777) : m(o, "sInt", int.class, long.class, Object.class).invoke(null, a, b, c),
                    m(h, "sInt", int.class, long.class, Object.class).invoke(null, a, b, c));
            }
            for (float f : new float[] { 1f, 2f, 100f, 1000f, 5.5f }) for (double d : new double[] { 0.5, -2 }) {
                invoked++;
                check("sFloat(" + f + "," + d + ")",
                    enabled ? Float.valueOf(3.5f) : m(o, "sFloat", float.class, double.class).invoke(null, f, d),
                    m(h, "sFloat", float.class, double.class).invoke(null, f, d));
            }
            for (long a : new long[] { 10L, -7L }) for (long b : new long[] { 0L, 3L }) {
                invoked++;
                check("sLong(" + a + "," + b + ")",
                    enabled ? Long.valueOf(99L) : m(o, "sLong", long.class, long.class).invoke(null, a, b),
                    m(h, "sLong", long.class, long.class).invoke(null, a, b));
            }
            for (double d : new double[] { -4.0, 3.0 }) {
                invoked++;
                check("sDouble(" + d + ")",
                    enabled ? Double.valueOf(1.25) : m(o, "sDouble", double.class).invoke(null, d),
                    m(h, "sDouble", double.class).invoke(null, d));
            }
            for (Object ob : new Object[] { null, "z" }) for (int n : new int[] { 1, 3 }) {
                invoked++;
                check("iRef(" + ob + "," + n + ")",
                    enabled ? "hooked" : m(o, "iRef", Object.class, int.class).invoke(oi, ob, n),
                    m(h, "iRef", Object.class, int.class).invoke(hi, ob, n));
            }
            for (String s : new String[] { null, "q" }) {
                invoked++;
                check("iStr(" + s + ")",
                    enabled ? "hookedString" : m(o, "iStr", String.class).invoke(oi, s),
                    m(h, "iStr", String.class).invoke(hi, s));
            }
            invoked++;
            m(o, "iVoid", int.class).invoke(oi, 3);
            m(h, "iVoid", int.class).invoke(hi, 3);
            java.lang.reflect.Field of = o.getField("field"), hf = h.getField("field");
            // Guard true makes the method return without running the body.
            check("iVoid field", enabled ? Integer.valueOf(hf.getInt(hi)) : Integer.valueOf(of.getInt(oi)), hf.getInt(hi));
            if (enabled) check("iVoid skipped", Integer.valueOf(5 + 3 * 3), Integer.valueOf(of.getInt(oi)) == 5 + 3 * 3 ? Integer.valueOf(5 + 3 * 3) : of.getInt(oi));
            of.setInt(oi, 5); hf.setInt(hi, 5);
            invoked++;
            check("iSync", enabled ? Integer.valueOf(-5) : m(o, "iSync", int.class).invoke(oi, 4),
                m(h, "iSync", int.class).invoke(hi, 4));
            invoked++;
            check("iArr", enabled ? new float[] { 42f } : m(o, "iArr", float.class).invoke(oi, 2f),
                m(h, "iArr", float.class).invoke(hi, 2f));
            for (int n : new int[] { 1, 4 }) {
                invoked++;
                check("loopAtStart(" + n + ")",
                    enabled ? Integer.valueOf(-9) : m(o, "loopAtStart", int.class).invoke(oi, n),
                    m(h, "loopAtStart", int.class).invoke(hi, n));
            }
            check("guard calls", Integer.valueOf(invoked), Integer.valueOf(calls.getInt(null) - before));
        }
        if (failures != 0) {
            System.err.println(failures + " failure(s)");
            System.exit(1);
        }
        System.out.println("HookFixtureTest OK");
    }
}
