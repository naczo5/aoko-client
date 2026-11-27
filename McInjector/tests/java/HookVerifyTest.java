import java.io.File;
import java.net.URL;
import java.net.URLClassLoader;
import java.util.ArrayList;
import java.util.List;

/**
 * Defines rewritten game classes in a child loader so the JVM verifier checks every method.
 * Any VerifyError, ClassFormatError or IncompatibleClassChangeError fails the run. Errors
 * from static initialisers (a headless JVM cannot initialise some game classes) happen after
 * linking and are reported but accepted.
 *
 * The loader is child-first over [rewritten classes, then the client jar and libraries], so every
 * game class (rewritten or not) is defined by the same loader and assignability checks between
 * them behave as they do in the game.
 *
 * Usage: HookVerifyTest <rewrittenRoot> <gameClasspath>
 */
public final class HookVerifyTest {
    public static void main(String[] args) throws Exception {
        File root = new File(args[0]);
        List<String> names = new ArrayList<String>();
        collect(root, root, names);
        int failures = 0;
        List<URL> urls = new ArrayList<URL>();
        urls.add(root.toURI().toURL());
        for (String p : args[1].split(File.pathSeparator)) if (p.length() > 0) urls.add(new File(p).toURI().toURL());
        URLClassLoader loader = new URLClassLoader(urls.toArray(new URL[0]), ClassLoader.getSystemClassLoader().getParent());
        for (String n : names) {
            try {
                Class.forName(n, true, loader);
                System.out.println("verified " + n);
            } catch (VerifyError | ClassFormatError | IncompatibleClassChangeError e) {
                failures++;
                System.err.println("FAIL " + n + ": " + e);
            } catch (Throwable t) {
                Throwable root0 = t;
                while (root0.getCause() != null) root0 = root0.getCause();
                if (root0 instanceof VerifyError || root0 instanceof ClassFormatError) {
                    failures++;
                    System.err.println("FAIL " + n + ": " + root0);
                } else {
                    System.out.println("verified " + n + " (initialiser stopped: " + root0.getClass().getSimpleName() + ")");
                }
            }
        }
        if (names.isEmpty()) { System.err.println("no classes found"); System.exit(1); }
        if (failures != 0) System.exit(1);
        System.out.println("HookVerifyTest OK (" + names.size() + " classes)");
    }

    static void collect(File root, File dir, List<String> out) {
        File[] files = dir.listFiles();
        if (files == null) return;
        for (File f : files) {
            if (f.isDirectory()) collect(root, f, out);
            else if (f.getName().endsWith(".class")) {
                String rel = root.toURI().relativize(f.toURI()).getPath();
                out.add(rel.substring(0, rel.length() - 6).replace('/', '.'));
            }
        }
    }
}
