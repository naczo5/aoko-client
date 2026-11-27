package com.aoko.helper;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.List;
import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

/**
 * Version-adaptive Minecraft session/user swap and server join for the Aoko bridges.
 *
 * <p>Names are only fast paths. The resolver validates class structure, constructor
 * shape and the current field value before it permits a write, and unknown or
 * ambiguous layouts fail closed instead of writing to a guessed field. Covered
 * families: 1.8.9 vanilla {@code Session}, 1.8.9 Forge SRG, 1.21.x Fabric/Yarn
 * {@code Session} and the {@code User} class of official-name 26.x clients. Server
 * joins use Minecraft's own ConnectScreen / GuiConnecting path, never an
 * account-gated menu.</p>
 *
 * <p>Return protocol: {@code "ok"} or {@code "ok:<detail>"} on success,
 * {@code "error:<code>"} where {@code code} is one of {@code invalid_session_data},
 * {@code in_world}, {@code unsupported_session_constructor},
 * {@code session_write_not_observed}, {@code game_thread_timeout},
 * {@code no_launch_session}, {@code invalid_address}, {@code connect_path_not_found},
 * or a {@code describeThrowable} chain. Access tokens are never logged.</p>
 *
 * <p>Embedded as bytes in {@code jni_core/session_switcher.inc} by
 * {@code McInjector/gen_java_helper.sh}; keep the class Java 8 bytecode compatible.</p>
 */
public final class SessionSwitcher {
    private static Object launchSession;
    private static Object minecraftInstance;
    private static Class<?> minecraftClass;
    private static Field sessionField;
    private static Class<?> sessionClass;
    private static Method getUsername;
    private static Method getPlayerId;
    private static Field playerField;
    private static Field worldField;
    private static Field screenField;
    private static Method setScreenMethod;
    private static Method scheduleMethod;
    private static String hintedMcClass;
    private static String hintedSessionClass;
    private static String hintedConnectClass;
    // Stored for signature fidelity with the native discovery hints. Address objects
    // are always built through the validated createServerAddress path.
    private static String hintedServerAddressClass;
    private static String hintedServerDataClass;
    private static volatile boolean ready;

    private SessionSwitcher() {}

    /** Optional dot-name hints supplied by native discovery. Resets the ready cache. */
    public static synchronized void hint(String mcClassName, String sessionClassName) {
        if (mcClassName != null && !mcClassName.isEmpty()) hintedMcClass = mcClassName;
        if (sessionClassName != null && !sessionClassName.isEmpty()) hintedSessionClass = sessionClassName;
        ready = false;
    }

    /** Optional connect-path hints supplied by native discovery. */
    public static synchronized void hintConnect(
            String connectClassName, String serverAddressClassName, String serverDataClassName) {
        if (connectClassName != null && !connectClassName.isEmpty()) hintedConnectClass = connectClassName;
        if (serverAddressClassName != null && !serverAddressClassName.isEmpty())
            hintedServerAddressClass = serverAddressClassName;
        if (serverDataClassName != null && !serverDataClassName.isEmpty())
            hintedServerDataClass = serverDataClassName;
    }

    public static synchronized String init() {
        try {
            if (ready) return "ok";

            Class<?> mcClass = findMinecraftClass();
            if (mcClass == null) return "Minecraft class not found";

            minecraftInstance = findMinecraftInstance(mcClass);
            if (minecraftInstance == null) return "Minecraft instance is null";
            minecraftClass = mcClass;
            screenField = findScreenField(mcClass);
            setScreenMethod = findSetScreenMethod(mcClass, screenField);

            sessionField = findNamedSessionField(mcClass);
            if (sessionField == null) {
                sessionClass = findSessionClass();
                if (sessionClass != null) {
                    sessionField = findFieldOfType(mcClass, sessionClass);
                }
            }
            if (sessionField == null) sessionField = discoverSessionField(mcClass, minecraftInstance);
            if (sessionField == null) return "validated session/user field not found on Minecraft";

            sessionField.setAccessible(true);
            try {
                Field modifiersField = Field.class.getDeclaredField("modifiers");
                modifiersField.setAccessible(true);
                modifiersField.setInt(sessionField, sessionField.getModifiers() & ~Modifier.FINAL);
            } catch (Throwable ignored) {}
            sessionClass = sessionField.getType();
            if (!looksLikeSessionClass(sessionClass)) {
                return "session/user field failed structural validation";
            }

            getUsername = findValueMethod(sessionClass,
                    new String[]{"getUsername", "getName", "func_111285_a", "method_1676", "c"},
                    new String[]{"user", "name"});
            getPlayerId = findValueMethod(sessionClass,
                    new String[]{"getPlayerID", "getUuid", "getUuidOrNull", "getProfileId",
                            "func_148255_b", "method_44717", "b", "a"},
                    new String[]{"uuid", "profile", "player", "id"});

            playerField = findWorldStateField(mcClass, true);
            worldField = findWorldStateField(mcClass, false);
            scheduleMethod = findScheduleMethod(mcClass);

            Object current = sessionField.get(minecraftInstance);
            if (current == null || !sessionClass.isInstance(current)) {
                return "current session/user is null or has an unexpected type";
            }
            if (launchSession == null || !sessionClass.isInstance(launchSession)) {
                launchSession = current;
            }

            ready = true;
            return "ok";
        } catch (Throwable t) {
            ready = false;
            return "init failed: " + describeThrowable(t);
        }
    }

    public static synchronized String setSession(String name, String uuid, String token) {
        String initResult = init();
        if (!ready) return "error:" + initResult;
        if (isBlank(name) || isBlank(uuid) || isBlank(token)) return "error:invalid_session_data";
        if (isInWorld()) return "error:in_world";

        try {
            final Object replacement = createSession(name, uuid, token);
            if (replacement == null) return "error:unsupported_session_constructor";
            final Object previous = sessionField.get(minecraftInstance);
            final String[] failure = new String[1];

            boolean ran = runOnGameThread(() -> {
                try {
                    // Re-check after scheduling: the user may have joined a world.
                    if (isInWorld()) {
                        failure[0] = "in_world";
                        return;
                    }
                    sessionField.set(minecraftInstance, replacement);
                    if (sessionField.get(minecraftInstance) != replacement) {
                        failure[0] = "session_write_not_observed";
                    }
                } catch (Throwable t) {
                    try { sessionField.set(minecraftInstance, previous); } catch (Throwable ignored) {}
                    failure[0] = describeThrowable(t);
                }
            });

            if (!ran) return "error:game_thread_timeout";
            if (failure[0] != null) return "error:" + failure[0];
            return "ok:" + readUsername();
        } catch (Throwable t) {
            return "error:" + describeThrowable(t);
        }
    }

    public static synchronized String restoreSession() {
        String initResult = init();
        if (!ready) return "error:" + initResult;
        if (isInWorld()) return "error:in_world";
        if (launchSession == null || !sessionClass.isInstance(launchSession)) {
            return "error:no_launch_session";
        }

        final String[] failure = new String[1];
        boolean ran = runOnGameThread(() -> {
            try {
                if (isInWorld()) {
                    failure[0] = "in_world";
                    return;
                }
                sessionField.set(minecraftInstance, launchSession);
                if (sessionField.get(minecraftInstance) != launchSession) {
                    failure[0] = "session_write_not_observed";
                }
            } catch (Throwable t) {
                failure[0] = describeThrowable(t);
            }
        });
        if (!ran) return "error:game_thread_timeout";
        if (failure[0] != null) return "error:" + failure[0];
        try {
            return "ok:" + readUsername();
        } catch (Throwable t) {
            return "error:" + describeThrowable(t);
        }
    }

    public static synchronized String joinServer(String host, int port) {
        if (isBlank(host) || host.trim().length() > 256 || port < 1 || port > 65535) {
            return "error:invalid_address";
        }
        host = host.trim();
        String initResult = init();
        if (!ready) return "error:" + initResult;

        try {
            final String connectHost = host;
            final int connectPort = port;
            final String[] failure = new String[1];
            boolean ran = runOnGameThread(() -> {
                try {
                    if (!connectNow(connectHost, connectPort)) {
                        failure[0] = "connect_path_not_found";
                    }
                } catch (Throwable t) {
                    failure[0] = describeThrowable(t);
                }
            });
            if (!ran) return "error:game_thread_timeout";
            if (failure[0] != null) return "error:" + failure[0];
            return "ok:" + host + ":" + port;
        } catch (Throwable t) {
            return "error:" + describeThrowable(t);
        }
    }

    /** {@code "ok|username|uuid|inWorld|ready"} or {@code "error|message|false|false"}. */
    public static synchronized String getSessionInfo() {
        String initResult = init();
        if (!ready) return "error|" + initResult + "|false|false";
        try {
            return "ok|" + readUsername() + "|" + readUuid() + "|" + isInWorld() + "|true";
        } catch (Throwable t) {
            return "error|" + describeThrowable(t) + "|false|false";
        }
    }

    public static synchronized boolean isReady() {
        return ready || "ok".equals(init());
    }

    private static Object findMinecraftInstance(Class<?> mcClass) throws Exception {
        Field singleton = findStaticFieldOfType(mcClass, mcClass);
        if (singleton != null) {
            singleton.setAccessible(true);
            Object result = singleton.get(null);
            if (result != null) return result;
        }

        Method getter = findMinecraftGetter(mcClass);
        if (getter == null) return null;
        getter.setAccessible(true);
        return getter.invoke(null);
    }

    private static boolean isInWorld() {
        try {
            return (playerField != null && playerField.get(minecraftInstance) != null)
                    || (worldField != null && worldField.get(minecraftInstance) != null);
        } catch (Throwable ignored) {
            // Failure to read the guard must fail closed for mutation callers.
            return true;
        }
    }

    private static boolean runOnGameThread(Runnable action) {
        if (scheduleMethod == null) {
            // Legacy/custom clients without an exposed scheduler are only mutated
            // at the main menu, where this direct write is quiescent.
            action.run();
            return true;
        }

        final CountDownLatch done = new CountDownLatch(1);
        Runnable wrapped = () -> {
            try { action.run(); }
            finally { done.countDown(); }
        };
        try {
            scheduleMethod.invoke(minecraftInstance, wrapped);
            if (done.await(4, TimeUnit.SECONDS)) return true;
        } catch (Throwable ignored) {}

        // Fallback: if scheduling timed out or failed while outside of a world
        // (for example the game is at the main menu, paused or the scheduler is
        // quiescent), perform the mutation directly.
        if (!isInWorld()) {
            action.run();
            return true;
        }
        return false;
    }

    private static Object createSession(String name, String uuidText, String token) throws Exception {
        UUID uuid = parseUuid(uuidText);
        Constructor<?>[] constructors = sessionClass.getDeclaredConstructors();
        Arrays.sort(constructors, Comparator.comparingInt(SessionSwitcher::constructorScore).reversed());

        for (Constructor<?> ctor : constructors) {
            Class<?>[] types = ctor.getParameterTypes();
            Object[] args = buildConstructorArgs(types, name, uuidText, uuid, token);
            if (args == null) continue;
            try {
                ctor.setAccessible(true);
                return ctor.newInstance(args);
            } catch (Throwable ignored) {
                // Try the next structurally supported shape.
            }
        }
        return null;
    }

    private static int constructorScore(Constructor<?> ctor) {
        Class<?>[] types = ctor.getParameterTypes();
        int score = types.length;
        for (Class<?> type : types) {
            if (type == UUID.class) score += 20;
            else if (type == Optional.class) score += 5;
            else if (type == String.class) score += 3;
            else if (type.isEnum()) score += 2;
            else score -= 100;
        }
        return score;
    }

    private static Object[] buildConstructorArgs(
            Class<?>[] types, String name, String uuidText, UUID uuid, String token) {
        if (types.length < 3 || types.length > 7) return null;
        boolean hasUuid = false;
        int stringCount = 0;
        for (Class<?> type : types) {
            if (type == UUID.class) hasUuid = true;
            if (type == String.class) stringCount++;
        }

        Object[] args = new Object[types.length];
        int stringIndex = 0;
        for (int i = 0; i < types.length; i++) {
            Class<?> type = types[i];
            if (type == String.class) {
                if (stringIndex == 0) args[i] = name;
                else if (!hasUuid && stringIndex == 1) args[i] = uuidText;
                else if (stringCount == 4 && stringIndex == 3) args[i] = "mojang";
                else args[i] = token;
                stringIndex++;
            } else if (type == UUID.class) {
                args[i] = uuid;
            } else if (type == Optional.class) {
                args[i] = Optional.empty();
            } else if (type.isEnum()) {
                args[i] = findAccountEnum(type);
                if (args[i] == null) return null;
            } else {
                return null;
            }
        }
        return args;
    }

    private static Object findAccountEnum(Class<?> type) {
        Object[] values = type.getEnumConstants();
        if (values == null || values.length == 0) return null;
        for (String wanted : new String[]{"MSA", "MICROSOFT", "MOJANG"}) {
            for (Object value : values) {
                if (((Enum<?>) value).name().equalsIgnoreCase(wanted)) return value;
            }
        }
        return values[0];
    }

    private static UUID parseUuid(String value) {
        String clean = value == null ? "" : value.trim();
        if (clean.length() == 32) {
            clean = clean.substring(0, 8) + "-" + clean.substring(8, 12) + "-"
                    + clean.substring(12, 16) + "-" + clean.substring(16, 20)
                    + "-" + clean.substring(20);
        }
        return UUID.fromString(clean);
    }

    private static String readUsername() throws Exception {
        Object session = sessionField.get(minecraftInstance);
        if (session == null) return "";
        Object value = getUsername == null ? null : getUsername.invoke(session);
        return value == null ? "" : value.toString();
    }

    private static String readUuid() throws Exception {
        Object session = sessionField.get(minecraftInstance);
        if (session == null) return "";
        Object value = getPlayerId == null ? null : getPlayerId.invoke(session);
        return value == null ? "" : value.toString();
    }

    private static Class<?> findMinecraftClass() {
        String[] names = {
                hintedMcClass,
                "net.minecraft.client.Minecraft",
                "net.minecraft.client.MinecraftClient",
                "net.minecraft.class_310",
                "ave"
        };
        for (String name : names) {
            Class<?> result = tryLoad(name);
            if (result != null) return result;
        }
        return null;
    }

    private static Class<?> findSessionClass() {
        String[] names = {
                hintedSessionClass,
                "net.minecraft.client.User",
                "net.minecraft.client.session.Session",
                "net.minecraft.util.Session",
                "net.minecraft.class_320",
                "avm",
                "bhl",
                "bhm"
        };
        for (String name : names) {
            Class<?> result = tryLoad(name);
            if (result != null && looksLikeSessionClass(result)) return result;
        }
        return null;
    }

    private static Class<?> tryLoad(String name) {
        if (name == null || name.isEmpty()) return null;
        try {
            return Class.forName(name, false, SessionSwitcher.class.getClassLoader());
        } catch (Throwable ignored) {}
        try {
            ClassLoader context = Thread.currentThread().getContextClassLoader();
            if (context != null) return Class.forName(name, false, context);
        } catch (Throwable ignored) {}
        return null;
    }

    private static Field findNamedSessionField(Class<?> owner) {
        for (String name : new String[]{"user", "session", "field_71449_j", "ae", "field_1726", "field_1690"}) {
            Field field = findField(owner, name);
            if (field != null && !Modifier.isStatic(field.getModifiers())
                    && looksLikeSessionClass(field.getType())) {
                return field;
            }
        }
        return null;
    }

    private static Field discoverSessionField(Class<?> owner, Object instance) {
        Field only = null;
        for (Class<?> current = owner; current != null && current != Object.class;
             current = current.getSuperclass()) {
            for (Field field : current.getDeclaredFields()) {
                if (Modifier.isStatic(field.getModifiers()) || !looksLikeSessionClass(field.getType())) continue;
                try {
                    field.setAccessible(true);
                    Object value = field.get(instance);
                    if (value == null || !field.getType().isInstance(value)) continue;
                } catch (Throwable ignored) {
                    continue;
                }
                // Ambiguity is unsafe. Never write to a guessed field.
                if (only != null) return null;
                only = field;
            }
        }
        return only;
    }

    private static boolean looksLikeSessionClass(Class<?> type) {
        if (type == null || type.isPrimitive() || type.isArray()
                || type.getName().startsWith("java.")) return false;

        boolean constructor = false;
        for (Constructor<?> ctor : type.getDeclaredConstructors()) {
            if (constructorScore(ctor) > 0) {
                constructor = true;
                break;
            }
        }
        if (!constructor) return false;

        Method name = findValueMethod(type,
                new String[]{"getUsername", "getName", "func_111285_a", "method_1676", "c"},
                new String[]{"user", "name"});
        Method token = findValueMethod(type,
                new String[]{"getToken", "getAccessToken", "func_148254_d", "method_1674", "d"},
                new String[]{"token"});
        return name != null && token != null;
    }

    private static Method findValueMethod(Class<?> type, String[] names, String[] hints) {
        for (String name : names) {
            Method method = findNoArgMethod(type, name);
            if (method != null && isReadableValue(method.getReturnType())) {
                try { method.setAccessible(true); } catch (Throwable ignored) {}
                return method;
            }
        }
        for (Method method : type.getDeclaredMethods()) {
            if (method.getParameterTypes().length != 0 || !isReadableValue(method.getReturnType())) continue;
            String lower = method.getName().toLowerCase();
            for (String hint : hints) {
                if (lower.contains(hint)) {
                    try { method.setAccessible(true); } catch (Throwable ignored) {}
                    return method;
                }
            }
        }
        return null;
    }

    private static boolean isReadableValue(Class<?> type) {
        return type == String.class || type == UUID.class;
    }

    private static Method findNoArgMethod(Class<?> type, String name) {
        for (Class<?> current = type; current != null && current != Object.class;
             current = current.getSuperclass()) {
            try {
                return current.getDeclaredMethod(name);
            } catch (NoSuchMethodException ignored) {}
        }
        return null;
    }

    private static Field findFieldOfType(Class<?> owner, Class<?> type) {
        Field found = null;
        for (Class<?> current = owner; current != null && current != Object.class;
             current = current.getSuperclass()) {
            for (Field field : current.getDeclaredFields()) {
                if (!Modifier.isStatic(field.getModifiers()) && field.getType() == type) {
                    if (found != null) return null;
                    found = field;
                }
            }
        }
        return found;
    }

    private static Field findStaticFieldOfType(Class<?> owner, Class<?> type) {
        for (Class<?> current = owner; current != null && current != Object.class;
             current = current.getSuperclass()) {
            for (Field field : current.getDeclaredFields()) {
                if (Modifier.isStatic(field.getModifiers()) && field.getType() == type) return field;
            }
        }
        return null;
    }

    private static Field findWorldStateField(Class<?> owner, boolean player) {
        String[] names = player
                ? new String[]{"thePlayer", "player", "field_71439_g", "h", "field_1724"}
                : new String[]{"theWorld", "world", "level", "field_71441_e", "f", "field_1687"};
        for (String name : names) {
            Field field = findField(owner, name);
            if (field != null && !Modifier.isStatic(field.getModifiers())) {
                try { field.setAccessible(true); } catch (Throwable ignored) {}
                return field;
            }
        }

        String[] typeHints = player
                ? new String[]{"EntityPlayerSP", "LocalPlayer", "ClientPlayer", "class_746", "bew"}
                : new String[]{"WorldClient", "ClientLevel", "ClientWorld", "class_638", "bdb"};
        for (Class<?> current = owner; current != null && current != Object.class;
             current = current.getSuperclass()) {
            for (Field field : current.getDeclaredFields()) {
                String typeName = field.getType().getName();
                for (String hint : typeHints) {
                    if (typeName.contains(hint)) {
                        try { field.setAccessible(true); } catch (Throwable ignored) {}
                        return field;
                    }
                }
            }
        }
        return null;
    }

    private static Field findField(Class<?> owner, String name) {
        for (Class<?> current = owner; current != null && current != Object.class;
             current = current.getSuperclass()) {
            try {
                return current.getDeclaredField(name);
            } catch (NoSuchFieldException ignored) {}
        }
        return null;
    }

    private static Method findMinecraftGetter(Class<?> mcClass) {
        for (String name : new String[]{"getMinecraft", "getInstance", "func_71410_x", "method_1551", "A"}) {
            Method method = findNoArgMethod(mcClass, name);
            if (method != null && Modifier.isStatic(method.getModifiers())
                    && mcClass.isAssignableFrom(method.getReturnType())) return method;
        }
        for (Method method : mcClass.getDeclaredMethods()) {
            if (Modifier.isStatic(method.getModifiers())
                    && method.getParameterTypes().length == 0
                    && mcClass.isAssignableFrom(method.getReturnType())) return method;
        }
        return null;
    }

    private static Method findScheduleMethod(Class<?> mcClass) {
        for (String name : new String[]{"execute", "addScheduledTask", "func_152344_a", "method_18859", "a"}) {
            for (Class<?> current = mcClass; current != null && current != Object.class;
                 current = current.getSuperclass()) {
                try {
                    Method method = current.getDeclaredMethod(name, Runnable.class);
                    method.setAccessible(true);
                    return method;
                } catch (Throwable ignored) {}
            }
        }
        return null;
    }

    private static boolean connectNow(String host, int port) throws Exception {
        String combined = combinedAddress(host, port);
        Class<?>[] candidates = connectClassCandidates();
        for (int i = 0; i < candidates.length; i++) {
            if (tryModernConnect(candidates[i], host, port, combined)) return true;
        }
        for (int i = 0; i < candidates.length; i++) {
            if (tryLegacyConnect(candidates[i], host, port, combined)) return true;
        }
        return false;
    }

    private static String combinedAddress(String host, int port) {
        boolean ipv6 = host.indexOf(':') >= 0;
        String wrapped = ipv6 ? "[" + host + "]" : host;
        return wrapped + ":" + port;
    }

    private static Class<?>[] connectClassCandidates() {
        List<Class<?>> found = new ArrayList<Class<?>>();
        String[] names = {
                hintedConnectClass,
                "net.minecraft.client.gui.screens.ConnectScreen",
                "net.minecraft.client.gui.screen.ConnectScreen",
                "net.minecraft.class_412",
                "net.minecraft.client.multiplayer.GuiConnecting",
                "net.minecraft.client.gui.GuiConnecting",
                "awz"
        };
        for (int i = 0; i < names.length; i++) {
            Class<?> type = tryLoad(names[i]);
            if (type != null && !found.contains(type)) found.add(type);
        }
        return found.toArray(new Class<?>[0]);
    }

    private static boolean tryModernConnect(Class<?> connectClass, String host, int port, String combined) {
        if (connectClass == null || minecraftClass == null) return false;
        Method[] methods = connectClass.getDeclaredMethods();
        Arrays.sort(methods, Comparator.comparingInt((Method method) -> method.getParameterTypes().length).reversed());

        Object parent = currentScreen();
        for (int m = 0; m < methods.length; m++) {
            Method method = methods[m];
            if (!Modifier.isStatic(method.getModifiers()) || method.getReturnType() != void.class) continue;
            Class<?>[] types = method.getParameterTypes();
            if (types.length < 4 || types.length > 6) continue;
            if (!types[1].isAssignableFrom(minecraftClass)) continue;

            Object[] args = buildConnectArgs(types, parent, host, port, combined);
            if (args == null) continue;
            try {
                method.setAccessible(true);
                method.invoke(null, args);
                return true;
            } catch (Throwable ignored) {
                // Try the next structurally matching connect method.
            }
        }
        return false;
    }

    private static Object[] buildConnectArgs(
            Class<?>[] types, Object parent, String host, int port, String combined) {
        Object[] args = new Object[types.length];
        args[0] = parent != null && types[0].isInstance(parent) ? parent : null;
        args[1] = minecraftInstance;

        int objectIndex = 0;
        for (int i = 2; i < types.length; i++) {
            Class<?> type = types[i];
            if (type == boolean.class || type == Boolean.class) {
                args[i] = Boolean.FALSE;
            } else if (type == int.class || type == Integer.class) {
                args[i] = Integer.valueOf(port);
            } else if (type == String.class) {
                args[i] = objectIndex == 0 ? host : combined;
                objectIndex++;
            } else if (type.isPrimitive()) {
                return null;
            } else {
                Object created = null;
                if (objectIndex == 0) {
                    created = createServerAddress(type, host, port, combined);
                    if (created == null) created = createServerData(type, host, combined);
                } else if (objectIndex == 1) {
                    created = createServerData(type, host, combined);
                    if (created == null) created = createServerAddress(type, host, port, combined);
                }
                if (created == null && i == types.length - 1) {
                    args[i] = null;
                } else if (created == null) {
                    return null;
                } else {
                    args[i] = created;
                    objectIndex++;
                }
            }
        }
        return args;
    }

    private static boolean tryLegacyConnect(Class<?> connectingClass, String host, int port, String combined)
            throws Exception {
        if (connectingClass == null || minecraftClass == null) return false;
        Object parent = currentScreen();
        Object screen = createLegacyConnectingScreen(connectingClass, parent, host, port, combined);
        if (screen == null) return false;
        Method setter = setScreenMethod;
        if (setter == null) setter = findSetScreenMethod(minecraftClass, screenField);
        if (setter == null) setter = findSetScreenFor(minecraftClass, screen.getClass());
        if (setter == null) return false;
        setter.setAccessible(true);
        setter.invoke(minecraftInstance, new Object[]{screen});
        return true;
    }

    private static Object createLegacyConnectingScreen(
            Class<?> connectingClass, Object parent, String host, int port, String combined) {
        Constructor<?>[] constructors = connectingClass.getDeclaredConstructors();
        Arrays.sort(constructors, Comparator.comparingInt((Constructor<?> ctor) -> ctor.getParameterTypes().length).reversed());

        for (int i = 0; i < constructors.length; i++) {
            Constructor<?> ctor = constructors[i];
            Class<?>[] types = ctor.getParameterTypes();
            if (types.length < 3 || types.length > 4) continue;
            if (!types[1].isAssignableFrom(minecraftClass)) continue;
            Object[] args = new Object[types.length];
            args[0] = parent != null && types[0].isInstance(parent) ? parent : null;
            args[1] = minecraftInstance;
            boolean built = true;
            for (int p = 2; p < types.length; p++) {
                Class<?> type = types[p];
                if (type == String.class) args[p] = host;
                else if (type == int.class || type == Integer.class) args[p] = Integer.valueOf(port);
                else if (!type.isPrimitive()) {
                    Object data = createServerData(type, host, combined);
                    if (data == null) {
                        built = false;
                        break;
                    }
                    args[p] = data;
                } else {
                    built = false;
                    break;
                }
            }
            if (!built) continue;
            try {
                ctor.setAccessible(true);
                return ctor.newInstance(args);
            } catch (Throwable ignored) {
                // Try the next structurally matching constructor.
            }
        }
        return null;
    }

    private static Object createServerAddress(Class<?> type, String host, int port, String combined) {
        Object parsed = invokeStaticParser(type, combined);
        if (parsed != null) return parsed;
        parsed = invokeStaticParser(type, host);
        if (parsed != null) return parsed;

        Constructor<?>[] constructors = type.getDeclaredConstructors();
        for (int i = 0; i < constructors.length; i++) {
            Class<?>[] types = constructors[i].getParameterTypes();
            try {
                constructors[i].setAccessible(true);
                if (types.length == 2 && types[0] == String.class
                        && (types[1] == int.class || types[1] == Integer.class)) {
                    return constructors[i].newInstance(new Object[]{host, Integer.valueOf(port)});
                }
                if (types.length == 1 && types[0] == String.class) {
                    Object created = constructors[i].newInstance(new Object[]{combined});
                    if (created != null) return created;
                }
            } catch (Throwable ignored) {}
        }
        return null;
    }

    private static Object invokeStaticParser(Class<?> type, String value) {
        String[] names = {"parseString", "parse", "method_29545", "a"};
        for (int i = 0; i < names.length; i++) {
            try {
                Method method = type.getDeclaredMethod(names[i], String.class);
                if (!Modifier.isStatic(method.getModifiers())) continue;
                if (!type.isAssignableFrom(method.getReturnType()) && method.getReturnType() != type) continue;
                method.setAccessible(true);
                Object parsed = method.invoke(null, new Object[]{value});
                if (parsed != null) return parsed;
            } catch (Throwable ignored) {}
        }
        Method[] methods = type.getDeclaredMethods();
        for (int i = 0; i < methods.length; i++) {
            Method method = methods[i];
            if (!Modifier.isStatic(method.getModifiers()) || method.getParameterTypes().length != 1) continue;
            if (method.getParameterTypes()[0] != String.class) continue;
            if (!type.isAssignableFrom(method.getReturnType()) && method.getReturnType() != type) continue;
            try {
                method.setAccessible(true);
                Object parsed = method.invoke(null, new Object[]{value});
                if (parsed != null) return parsed;
            } catch (Throwable ignored) {}
        }
        return null;
    }

    private static Object createServerData(Class<?> type, String host, String combined) {
        Constructor<?>[] constructors = type.getDeclaredConstructors();
        Arrays.sort(constructors, Comparator.comparingInt((Constructor<?> ctor) -> ctor.getParameterTypes().length));
        String name = host;
        for (int i = 0; i < constructors.length; i++) {
            Class<?>[] types = constructors[i].getParameterTypes();
            if (types.length < 2 || types.length > 3) continue;
            if (types[0] != String.class || types[1] != String.class) continue;
            Object[] args = new Object[types.length];
            args[0] = name;
            args[1] = combined;
            if (types.length == 3) {
                if (types[2] == boolean.class || types[2] == Boolean.class) {
                    args[2] = Boolean.FALSE;
                } else if (types[2].isEnum()) {
                    args[2] = findServerTypeEnum(types[2]);
                    if (args[2] == null) continue;
                } else {
                    continue;
                }
            }
            try {
                constructors[i].setAccessible(true);
                return constructors[i].newInstance(args);
            } catch (Throwable ignored) {}
        }
        if (hintedServerDataClass != null) {
            Class<?> hinted = tryLoad(hintedServerDataClass);
            if (hinted != null && hinted != type) return createServerData(hinted, host, combined);
        }
        return null;
    }

    private static Object findServerTypeEnum(Class<?> type) {
        Object[] values = type.getEnumConstants();
        if (values == null || values.length == 0) return null;
        for (int i = 0; i < values.length; i++) {
            String name = ((Enum<?>) values[i]).name();
            if ("OTHER".equalsIgnoreCase(name) || "NORMAL".equalsIgnoreCase(name)) return values[i];
        }
        return values[values.length - 1];
    }

    private static Object currentScreen() {
        if (screenField == null || minecraftInstance == null) return null;
        try {
            return screenField.get(minecraftInstance);
        } catch (Throwable ignored) {
            return null;
        }
    }

    private static Field findScreenField(Class<?> owner) {
        String[] names = {"currentScreen", "screen", "field_71462_r", "m", "field_1752"};
        for (int i = 0; i < names.length; i++) {
            Field field = findField(owner, names[i]);
            if (field != null && !Modifier.isStatic(field.getModifiers())
                    && looksLikeScreenClass(field.getType())) {
                try { field.setAccessible(true); } catch (Throwable ignored) {}
                return field;
            }
        }
        Field only = null;
        for (Class<?> current = owner; current != null && current != Object.class;
             current = current.getSuperclass()) {
            Field[] fields = current.getDeclaredFields();
            for (int i = 0; i < fields.length; i++) {
                Field field = fields[i];
                if (Modifier.isStatic(field.getModifiers()) || !looksLikeScreenClass(field.getType())) continue;
                if (only != null) return null;
                only = field;
            }
        }
        if (only != null) {
            try { only.setAccessible(true); } catch (Throwable ignored) {}
        }
        return only;
    }

    private static boolean looksLikeScreenClass(Class<?> type) {
        if (type == null || type.isPrimitive() || type.isArray()
                || type.getName().startsWith("java.")) return false;
        String name = type.getName();
        return name.endsWith("Screen") || name.endsWith("GuiScreen")
                || name.endsWith("class_437") || name.equals("axu");
    }

    private static Method findSetScreenMethod(Class<?> mcClass, Field screen) {
        Class<?> screenType = screen == null ? null : screen.getType();
        String[] names = {"setScreen", "displayGuiScreen", "func_147108_a", "openScreen", "method_1507", "a"};
        for (int i = 0; i < names.length; i++) {
            Method method = findOneArgMethod(mcClass, names[i], screenType);
            if (method != null) return method;
        }
        if (screenType == null) return null;
        return findSetScreenFor(mcClass, screenType);
    }

    private static Method findSetScreenFor(Class<?> mcClass, Class<?> screenType) {
        Method found = null;
        for (Class<?> current = mcClass; current != null && current != Object.class;
             current = current.getSuperclass()) {
            Method[] methods = current.getDeclaredMethods();
            for (int i = 0; i < methods.length; i++) {
                Method method = methods[i];
                if (Modifier.isStatic(method.getModifiers()) || method.getParameterTypes().length != 1) continue;
                if (method.getReturnType() != void.class) continue;
                Class<?> param = method.getParameterTypes()[0];
                if (!param.isAssignableFrom(screenType) && param != screenType) continue;
                if (found != null) return null;
                found = method;
            }
        }
        if (found != null) {
            try { found.setAccessible(true); } catch (Throwable ignored) {}
        }
        return found;
    }

    private static Method findOneArgMethod(Class<?> owner, String name, Class<?> preferredParam) {
        for (Class<?> current = owner; current != null && current != Object.class;
             current = current.getSuperclass()) {
            Method[] methods = current.getDeclaredMethods();
            for (int i = 0; i < methods.length; i++) {
                Method method = methods[i];
                if (!method.getName().equals(name) || method.getParameterTypes().length != 1) continue;
                if (preferredParam != null
                        && !method.getParameterTypes()[0].isAssignableFrom(preferredParam)
                        && method.getParameterTypes()[0] != preferredParam) continue;
                try { method.setAccessible(true); } catch (Throwable ignored) {}
                return method;
            }
        }
        return null;
    }

    private static String describeThrowable(Throwable throwable) {
        StringBuilder result = new StringBuilder();
        Throwable current = throwable;
        for (int depth = 0; current != null && depth < 5; depth++) {
            if (depth > 0) result.append(" caused by ");
            result.append(current.getClass().getSimpleName());
            String message = current.getMessage();
            if (message != null && !message.isEmpty()) result.append(": ").append(message);
            Throwable next = current.getCause();
            if (next == current) break;
            current = next;
        }
        return result.toString();
    }

    private static boolean isBlank(String value) {
        return value == null || value.trim().isEmpty();
    }
}
