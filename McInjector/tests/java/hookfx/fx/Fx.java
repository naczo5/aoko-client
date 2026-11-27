package fx;

/** Fixture methods with the control-flow shapes a hook prologue must survive. */
public class Fx {
    public int field = 5;

    public static int sInt(int a, long b, Object c) {
        int x = a;
        for (int i = 0; i < 3; i++) x += i;
        return x + (int) b + (c == null ? 0 : 1);
    }

    public static float sFloat(float a, double d) {
        switch ((int) a) {
            case 1: return 10f;
            case 2: return 20f;
            case 100: return 30f;
            case 1000: return 40f;
            default: return a + (float) d;
        }
    }

    public static long sLong(long a, long b) {
        try {
            return a / b;
        } catch (ArithmeticException e) {
            return -1L;
        }
    }

    public static double sDouble(double a) {
        return a > 0 ? a * 2 : -a;
    }

    public Object iRef(Object o, int n) {
        // new StringBuilder(cond ? a : b) leaves an uninitialized object in a stack map frame.
        return o == null ? "null:" + n : new StringBuilder(n > 2 ? "big" : "small").append(o).toString();
    }

    public String iStr(String s) {
        return s == null ? "none" : s + field;
    }

    public void iVoid(int n) {
        for (int i = 0; i < n; i++) field += n;
    }

    public synchronized int iSync(int a) {
        return a + field;
    }

    public float[] iArr(float f) {
        return new float[] { f, f + 1 };
    }

    public int loopAtStart(int n) {
        // The first instruction is a loop head, so javac emits a frame at offset 0.
        int x = 0;
        while (true) {
            x += n;
            if (x > 20) return x;
        }
    }
}
