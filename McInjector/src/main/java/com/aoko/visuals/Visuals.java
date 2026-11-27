package com.aoko.visuals;

import java.io.File;

/**
 * Entry point for the native bridge. Every public static method takes and returns only
 * JDK types, never throws, and returns the current status string:
 * {@code off}, {@code ok}, {@code partial:<detail>}, {@code unsupported:<detail>} or
 * {@code error:<detail>}.
 *
 * Threading: the bridge calls these from its own worker and socket threads. Anything that
 * touches game state or OpenGL is queued to the game thread by the platform. The hook
 * callbacks in {@link Hooks} run on the render thread and only read volatile snapshots.
 */
public final class Visuals {
    static volatile Platform platform;
    static volatile LegacyPlatform legacy;
    static volatile ModernPlatform modern;
    static volatile Library library;
    static volatile Config config = Config.OFF;
    static volatile SkyState sky = SkyState.OFF;
    static volatile boolean cosmeticsFault;
    static volatile boolean skyFault;

    private static String initStatus = "off";
    private static String lastFault = "";
    private static int skyFaults, cosmeticFaults;
    private static int logged;
    private static final Object LOCK = new Object();
    private static final long START = System.nanoTime();

    private Visuals() {}

    // ---- Native entry points ------------------------------------------------------

    /** {@code configJson} is the same object {@link #configure} takes; only its {@code dir} is read here. */
    public static String bootstrap(String generation, String configJson) {
        synchronized (LOCK) {
            try {
                if (platform != null) return statusLocked();
                String dir = Config.parse(configJson).dir;
                if (dir.length() == 0) return "error:no visuals directory";
                File root = new File(dir);
                Library lib = new Library(root);
                library = lib;
                Platform p;
                if ("legacy".equals(generation)) {
                    LegacyPlatform l = new LegacyPlatform(Visuals.class.getClassLoader());
                    legacy = l; p = l;
                } else if ("modern".equals(generation)) {
                    ModernPlatform m = new ModernPlatform(Visuals.class.getClassLoader());
                    modern = m; p = m;
                } else {
                    return "error:unknown generation";
                }
                String r = p.init(lib);
                initStatus = r == null ? "ok" : r;
                if (r != null && (r.startsWith("unsupported") || r.startsWith("error"))) {
                    // Nothing was published: the bridge retries "error" results and latches "unsupported".
                    legacy = null; modern = null;
                    platform = null;
                    return r;
                }
                platform = p;
                return statusLocked();
            } catch (Throwable t) {
                legacy = null; modern = null; platform = null;
                log("bootstrap failed", t);
                return "error:" + brief(t);
            }
        }
    }

    public static String configure(String json) {
        synchronized (LOCK) {
            try {
                Platform p = platform;
                if (p == null) return statusLocked();
                Config c = Config.parse(json);
                Config old = config;
                if (old.sameAs(c)) return statusLocked();
                config = c;
                if (!c.cosmetics) cosmeticsFault = false;
                if (!c.sky) skyFault = false;
                SkyPreset preset = null;
                String note = "";
                if (c.sky && c.preset.length() > 0) {
                    try {
                        preset = library.loadSky(c.preset);
                    } catch (Exception e) {
                        note = "sky preset '" + c.preset + "' not usable: " + brief(e);
                    }
                }
                sky = SkyState.of(c, preset);
                skyNote = note;
                p.onConfig(c, sky);
                return statusLocked();
            } catch (Throwable t) {
                log("configure failed", t);
                return "error:" + brief(t);
            }
        }
    }

    public static String maintain() {
        synchronized (LOCK) {
            try {
                Platform p = platform;
                if (p == null) return statusLocked();
                p.maintain(config, sky);
                return statusLocked();
            } catch (Throwable t) {
                log("maintain failed", t);
                return "error:" + brief(t);
            }
        }
    }

    public static String status() {
        synchronized (LOCK) { return statusLocked(); }
    }

    /** Turns every override off. The classes stay loaded; the bridge restores the game bytecode. */
    public static void shutdown() {
        synchronized (LOCK) {
            try {
                config = Config.OFF;
                sky = SkyState.OFF;
                Platform p = platform;
                if (p != null) { p.onConfig(Config.OFF, SkyState.OFF); p.shutdown(); }
            } catch (Throwable t) {
                log("shutdown failed", t);
            }
        }
    }

    // ---- Internals ------------------------------------------------------------------

    private static volatile String skyNote = "";

    private static String statusLocked() {
        Config c = config;
        if (platform == null) return initStatus.startsWith("unsupported") ? initStatus : "off";
        if (!c.cosmetics && !c.sky) return "off";
        StringBuilder parts = new StringBuilder();
        if (initStatus.startsWith("partial:")) parts.append(initStatus.substring(8));
        String n = platform.notes();
        if (n.length() > 0) { if (parts.length() > 0) parts.append("; "); parts.append(n); }
        if (c.sky && skyNote.length() > 0) { if (parts.length() > 0) parts.append("; "); parts.append(skyNote); }
        if (cosmeticsFault || skyFault) {
            if (parts.length() > 0) parts.append("; ");
            parts.append(lastFault);
        }
        if (parts.length() == 0) return "ok";
        String s = parts.toString();
        if (s.length() > 200) s = s.substring(0, 200);
        return (cosmeticsFault || skyFault ? "error:" : "partial:") + s;
    }

    static double seconds() {
        return (System.nanoTime() - START) / 1e9;
    }

    /** Records a hook failure. Three strikes turn that feature group off for the session. */
    static void fail(String group, Throwable t) {
        synchronized (LOCK) {
            lastFault = group + " disabled: " + brief(t);
            log(group + " hook failed", t);
            if ("sky".equals(group)) {
                if (++skyFaults >= 3) { skyFault = true; sky = SkyState.OFF; }
            } else if (++cosmeticFaults >= 3) {
                cosmeticsFault = true;
            }
        }
    }

    static String brief(Throwable t) {
        String m = t.getMessage();
        String s = t.getClass().getSimpleName() + (m == null ? "" : ": " + m);
        return s.length() > 120 ? s.substring(0, 120) : s;
    }

    static void log(String message, Throwable t) {
        if (logged++ > 60) return;
        System.err.println("[aoko-visuals] " + message + (t == null ? "" : " (" + brief(t) + ")"));
        if (t != null && logged < 12) t.printStackTrace();
    }
}
