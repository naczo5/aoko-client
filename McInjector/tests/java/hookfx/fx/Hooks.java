package fx;

/** Helper the rewritten fixture calls. Descriptors follow the erasure rule in method_hook_rewriter.h. */
public final class Hooks {
    public static volatile boolean on;
    public static int calls;

    public static boolean gSInt(int a, long b, Object c) { calls++; return on; }
    public static int vSInt(int a, long b, Object c) { return 777; }

    public static boolean gSFloat(float a, double d) { calls++; return on; }
    public static float vSFloat(float a, double d) { return 3.5f; }

    public static boolean gSLong(long a, long b) { calls++; return on; }
    public static long vSLong(long a, long b) { return 99L; }

    public static boolean gSDouble(double a) { calls++; return on; }
    public static double vSDouble(double a) { return 1.25; }

    public static boolean gIRef(Object self, Object o, int n) { calls++; return on; }
    public static Object vIRef(Object self, Object o, int n) { return "hooked"; }

    public static boolean gIStr(Object self, Object s) { calls++; return on; }
    public static Object vIStr(Object self, Object s) { return "hookedString"; }

    public static boolean gIVoid(Object self, int n) { calls++; return on; }

    public static boolean gISync(Object self, int a) { calls++; return on; }
    public static int vISync(Object self, int a) { return -5; }

    public static boolean gIArr(Object self, float f) { calls++; return on; }
    public static Object vIArr(Object self, float f) { return new float[] { 42f }; }

    public static boolean gLoop(Object self, int n) { calls++; return on; }
    public static int vLoop(Object self, int n) { return -9; }

    private Hooks() {}
}
