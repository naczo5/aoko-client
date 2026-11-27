import java.io.BufferedReader;
import java.io.FileReader;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;

/**
 * Every guard and value method the native hook table calls must exist in
 * com.aoko.visuals.Hooks as a public static method with exactly the descriptor the native side
 * derives from the shape. Input: lines "HOOKMETHOD <name> <descriptor>" printed by visuals_hooks_check.
 *
 * Usage: HooksContractTest <hookmethods.txt>
 */
public final class HooksContractTest {
    static String descriptor(Class<?> c) {
        if (c == void.class) return "V";
        if (c == boolean.class) return "Z";
        if (c == int.class) return "I";
        if (c == float.class) return "F";
        if (c == double.class) return "D";
        if (c == long.class) return "J";
        if (c == byte.class) return "B";
        if (c == short.class) return "S";
        if (c == char.class) return "C";
        if (c.isArray()) return "[" + descriptor(c.getComponentType());
        return "L" + c.getName().replace('.', '/') + ";";
    }

    static String descriptor(Method m) {
        StringBuilder sb = new StringBuilder("(");
        for (Class<?> p : m.getParameterTypes()) sb.append(descriptor(p));
        return sb.append(')').append(descriptor(m.getReturnType())).toString();
    }

    public static void main(String[] args) throws Exception {
        Class<?> hooks = Class.forName("com.aoko.visuals.Hooks");
        int checked = 0, failures = 0;
        BufferedReader in = new BufferedReader(new FileReader(args[0]));
        try {
            String line;
            while ((line = in.readLine()) != null) {
                if (!line.startsWith("HOOKMETHOD ")) continue;
                String[] p = line.split(" ");
                boolean found = false;
                for (Method m : hooks.getDeclaredMethods()) {
                    if (m.getName().equals(p[1]) && Modifier.isStatic(m.getModifiers()) && Modifier.isPublic(m.getModifiers())
                        && descriptor(m).equals(p[2])) found = true;
                }
                checked++;
                if (!found) { failures++; System.err.println("MISSING Hooks." + p[1] + p[2]); }
            }
        } finally {
            in.close();
        }
        if (checked == 0 || failures != 0) { System.err.println("hooks contract failed (" + checked + " checked)"); System.exit(1); }
        System.out.println("HooksContractTest OK (" + checked + " methods)");
    }
}
