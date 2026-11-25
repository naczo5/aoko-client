package com.aoko.helper;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.Optional;
import java.util.UUID;

/**
 * Dependency-free structural test for com.aoko.helper.SessionSwitcher.
 *
 * <p>Runs on a plain JVM with no Minecraft on the classpath: the legacy 1.8.9
 * Session, the Forge SRG Session, the 1.21 five-argument Session, the 26.x User
 * shape, the Minecraft owner, the Screen and the ConnectScreen/GuiConnecting
 * classes are all faked here. Covers constructor scoring, argument mapping, UUID
 * normalization, accessor and field discovery, the in-world guard, invalid-input
 * rejection and the fail-closed paths for unknown layouts.</p>
 *
 * <p>Build and run through {@code McInjector/tests/java/run_tests.sh}.</p>
 */
public final class SessionSwitcherStructuralTest {

    // ── Fake session shapes (mirror SessionSwitcherCompatibilityTest) ──────────

    /** 1.8.9 vanilla / Forge SRG shape: four Strings, account type last. */
    public static final class LegacySession {
        final String name;
        final String uuid;
        final String token;
        final String type;

        public LegacySession(String name, String uuid, String token, String type) {
            this.name = name;
            this.uuid = uuid;
            this.token = token;
            this.type = type;
        }

        public String getUsername() { return name; }
        public String getAccessToken() { return token; }
        public String getPlayerID() { return uuid; }
    }

    public static final class ForgeSrgSession {
        final String name;
        final String uuid;
        final String token;
        final String type;

        public ForgeSrgSession(String name, String uuid, String token, String type) {
            this.name = name;
            this.uuid = uuid;
            this.token = token;
            this.type = type;
        }

        public String func_111285_a() { return name; }
        public String func_148254_d() { return token; }
        public String func_148255_b() { return uuid; }
    }

    public static final class Notch189Session {
        final String name;
        final String uuid;
        final String token;
        final String type;

        public Notch189Session(String name, String uuid, String token, String type) {
            this.name = name;
            this.uuid = uuid;
            this.token = token;
            this.type = type;
        }

        public String c() { return name; }
        public String d() { return token; }
        public String b() { return uuid; }
    }

    /** 1.21.x Yarn / Fabric and 26.x User shape. */
    public static final class ModernSession {
        final String name;
        final UUID uuid;
        final String token;
        final Optional<String> xuid;
        final Optional<String> clientId;

        public ModernSession(String name, UUID uuid, String token,
                             Optional<String> xuid, Optional<String> clientId) {
            this.name = name;
            this.uuid = uuid;
            this.token = token;
            this.xuid = xuid;
            this.clientId = clientId;
        }

        public String getName() { return name; }
        public String getAccessToken() { return token; }
        public UUID getUuid() { return uuid; }
    }

    /** Three Strings, no UUID parameter: name, uuid text, token. */
    public static final class LegacyThreeStringSession {
        final String name;
        final String uuid;
        final String token;

        public LegacyThreeStringSession(String name, String uuid, String token) {
            this.name = name;
            this.uuid = uuid;
            this.token = token;
        }

        public String getUsername() { return name; }
        public String getToken() { return token; }
    }

    /** Account-type enum instead of a String type. */
    public enum AccountType { MOJANG, MICROSOFT, MSA }

    public static final class EnumTypeSession {
        final String name;
        final String uuid;
        final String token;
        final AccountType type;

        public EnumTypeSession(String name, String uuid, String token, AccountType type) {
            this.name = name;
            this.uuid = uuid;
            this.token = token;
            this.type = type;
        }

        public String getUsername() { return name; }
        public String getAccessToken() { return token; }
    }

    /** No token-like accessor: must fail structural validation. */
    public static final class SessionWithoutToken {
        final String name;

        public SessionWithoutToken(String name, String uuid, String token, String type) {
            this.name = name;
        }

        public String getUsername() { return name; }
    }

    /** Unsupported constructor parameter: no argument mapping is safe. */
    public static final class UnsupportedSession {
        final Object payload;

        public UnsupportedSession(Object payload) { this.payload = payload; }

        public String getUsername() { return ""; }
        public String getAccessToken() { return ""; }
    }

    // ── Fake Minecraft owners ─────────────────────────────────────────────────

    public static final class MockForgeMinecraft {
        LegacySession field_71449_j;

        static final MockForgeMinecraft INSTANCE = new MockForgeMinecraft();
    }

    public static final class MockNotchMinecraft {
        Notch189Session ae;

        static final MockNotchMinecraft INSTANCE = new MockNotchMinecraft();
    }

    public static final class MockModernMinecraft {
        /** Wrong type under a fast-path name: the name must not win. */
        Object user;
        ModernSession real;

        static final MockModernMinecraft INSTANCE = new MockModernMinecraft();
    }

    public static final class MockAmbiguousMinecraft {
        /** Two live session-typed fields: discovery must refuse to guess. */
        LegacySession first;
        ForgeSrgSession second;

        static final MockAmbiguousMinecraft INSTANCE = new MockAmbiguousMinecraft();
    }

    public static final class MockSingleCandidateMinecraft {
        LegacySession only;

        static final MockSingleCandidateMinecraft INSTANCE = new MockSingleCandidateMinecraft();

        public String name() { return "x"; }
    }

    public static final class MockWorldMinecraft {
        LegacySession session;
        Object thePlayer;
        Object theWorld;

        public LegacySession launch;

        static final MockWorldMinecraft INSTANCE = new MockWorldMinecraft();

        public void setScreen(Object screen) { this.screen = screen; }

        public Object screen;

        public Object carried() { return session; }
    }

    /** Fake Screen for the screen-field/`setScreen` discovery paths. */
    public static final class FakeScreen {
        public String getName() { return "FakeScreen"; }
    }

    /** Unrelated instance used for the "wrong field owner" fail-closed check. */
    private static final class ForeignOwner {
        @SuppressWarnings("unused")
        LegacySession ae;
    }

    private static int checks;

    private SessionSwitcherStructuralTest() {}

    public static void main(String[] args) throws Exception {
        constructorScoring();
        constructorArgumentMapping();
        uuidNormalization();
        structuralValidation();
        namedFieldDiscovery();
        structuralFieldDiscovery();
        inWorldGuard();
        failClosedMutations();
        invalidAddresses();
        directWriteFallback();
        restoreLaunchSession();
        helpersAndHints();

        System.out.println("SessionSwitcher structural tests passed ("
                + checks + " assertions).");
    }

    // ── Cases ─────────────────────────────────────────────────────────────────

    private static void constructorScoring() throws Exception {
        int legacy = (Integer) invoke("constructorScore", new Class<?>[]{Constructor.class},
                new Object[]{LegacySession.class.getDeclaredConstructors()[0]});
        int modern = (Integer) invoke("constructorScore", new Class<?>[]{Constructor.class},
                new Object[]{ModernSession.class.getDeclaredConstructors()[0]});
        int unsupported = (Integer) invoke("constructorScore", new Class<?>[]{Constructor.class},
                new Object[]{UnsupportedSession.class.getDeclaredConstructors()[0]});
        require(legacy == 4 + 4 * 3, "legacy 4-String score = arity + 3 per String, got " + legacy);
        require(modern == 5 + 3 + 20 + 3 + 5 + 5,
                "modern score = arity + String/UUID/Optional weights, got " + modern);
        require(modern > legacy, "modern 5-arg shape must outrank the legacy shape");
        require(unsupported < 0, "unsupported parameter type must score negative, got " + unsupported);
    }

    private static void constructorArgumentMapping() throws Exception {
        String compact = "12345678123456781234567812345678";

        setStatic("sessionClass", LegacySession.class);
        Object legacy = invoke("createSession", new Class<?>[]{String.class, String.class, String.class},
                new Object[]{"LegacyUser", compact, "legacy-token"});
        require(legacy instanceof LegacySession, "legacy 4-String constructor selected");
        LegacySession ls = (LegacySession) legacy;
        require("LegacyUser".equals(ls.name), "legacy name");
        require(compact.equals(ls.uuid), "legacy uuid stays text");
        require("legacy-token".equals(ls.token), "legacy token");
        require("mojang".equals(ls.type), "legacy 4-String account type is mojang");

        setStatic("sessionClass", ForgeSrgSession.class);
        ForgeSrgSession srg = (ForgeSrgSession) invoke("createSession",
                new Class<?>[]{String.class, String.class, String.class},
                new Object[]{"SrgUser", compact, "srg-token"});
        require("mojang".equals(srg.type), "Forge SRG account type");

        setStatic("sessionClass", Notch189Session.class);
        Notch189Session notch = (Notch189Session) invoke("createSession",
                new Class<?>[]{String.class, String.class, String.class},
                new Object[]{"NotchUser", compact, "notch-token"});
        require("NotchUser".equals(notch.name), "1.8.9 Notch name");
        require(compact.equals(notch.uuid), "1.8.9 Notch uuid text");
        require("notch-token".equals(notch.token), "1.8.9 Notch token");
        require("mojang".equals(notch.type), "1.8.9 Notch account type");

        setStatic("sessionClass", LegacyThreeStringSession.class);
        LegacyThreeStringSession three = (LegacyThreeStringSession) invoke("createSession",
                new Class<?>[]{String.class, String.class, String.class},
                new Object[]{"ThreeUser", compact, "three-token"});
        require("ThreeUser".equals(three.name), "3-String name");
        require(compact.equals(three.uuid), "3-String uuid text slot");
        require("three-token".equals(three.token), "3-String token");

        setStatic("sessionClass", ModernSession.class);
        ModernSession modern = (ModernSession) invoke("createSession",
                new Class<?>[]{String.class, String.class, String.class},
                new Object[]{"ModernUser", compact, "modern-token"});
        require("ModernUser".equals(modern.name), "modern name");
        require("modern-token".equals(modern.token), "modern token");
        require(!modern.xuid.isPresent() && !modern.clientId.isPresent(),
                "modern Optional identifiers are empty");

        setStatic("sessionClass", EnumTypeSession.class);
        EnumTypeSession enumType = (EnumTypeSession) invoke("createSession",
                new Class<?>[]{String.class, String.class, String.class},
                new Object[]{"EnumUser", compact, "enum-token"});
        require(enumType.type == AccountType.MSA, "account enum prefers MSA, got " + enumType.type);
        setStatic("sessionClass", null);
    }

    private static void uuidNormalization() throws Exception {
        UUID parsed = (UUID) invoke("parseUuid", new Class<?>[]{String.class},
                new Object[]{"12345678123456781234567812345678"});
        require("12345678-1234-5678-1234-567812345678".equals(parsed.toString()),
                "compact 32-hex uuid normalized");
        UUID dashed = (UUID) invoke("parseUuid", new Class<?>[]{String.class},
                new Object[]{" 12345678-1234-5678-1234-567812345678 "});
        require(parsed.equals(dashed), "dashed uuid with padding accepted");
        boolean threw = false;
        try {
            invoke("parseUuid", new Class<?>[]{String.class}, new Object[]{"not-a-uuid"});
        } catch (Throwable expected) {
            threw = true;
        }
        require(threw, "invalid uuid text is rejected");
    }

    private static void structuralValidation() throws Exception {
        require((Boolean) invoke("looksLikeSessionClass", new Class<?>[]{Class.class},
                new Object[]{LegacySession.class}), "legacy session validates");
        require((Boolean) invoke("looksLikeSessionClass", new Class<?>[]{Class.class},
                new Object[]{ForgeSrgSession.class}), "Forge SRG session validates");
        require((Boolean) invoke("looksLikeSessionClass", new Class<?>[]{Class.class},
                new Object[]{Notch189Session.class}), "1.8.9 Notch session validates");
        require((Boolean) invoke("looksLikeSessionClass", new Class<?>[]{Class.class},
                new Object[]{ModernSession.class}), "modern session validates");
        require(!(Boolean) invoke("looksLikeSessionClass", new Class<?>[]{Class.class},
                new Object[]{SessionWithoutToken.class}), "session without token accessor rejected");
        require(!(Boolean) invoke("looksLikeSessionClass", new Class<?>[]{Class.class},
                new Object[]{String.class}), "java.* type rejected");
        require(!(Boolean) invoke("looksLikeSessionClass", new Class<?>[]{Class.class},
                new Object[]{int.class}), "primitive rejected");
        require(!(Boolean) invoke("looksLikeSessionClass", new Class<?>[]{Class.class},
                new Object[]{LegacySession[].class}), "array rejected");
    }

    private static void namedFieldDiscovery() throws Exception {
        Field forge = (Field) invoke("findNamedSessionField", new Class<?>[]{Class.class},
                new Object[]{MockForgeMinecraft.class});
        require(forge != null && "field_71449_j".equals(forge.getName()),
                "field_71449_j found on mock Forge Minecraft");

        Field notch = (Field) invoke("findNamedSessionField", new Class<?>[]{Class.class},
                new Object[]{MockNotchMinecraft.class});
        require(notch != null && "ae".equals(notch.getName()),
                "ae found on mock Notch Minecraft");

        Field wrongType = (Field) invoke("findNamedSessionField", new Class<?>[]{Class.class},
                new Object[]{MockModernMinecraft.class});
        require(wrongType == null,
                "name fast path must not accept a field whose type fails validation");
    }

    private static void structuralFieldDiscovery() throws Exception {
        MockSingleCandidateMinecraft single = MockSingleCandidateMinecraft.INSTANCE;
        single.only = new LegacySession("OnlyUser", "12345678123456781234567812345678", "t", "mojang");
        Field found = (Field) invoke("discoverSessionField", new Class<?>[]{Class.class, Object.class},
                new Object[]{MockSingleCandidateMinecraft.class, single});
        require(found != null && "only".equals(found.getName()), "unique live session field discovered");

        Field ambiguous = (Field) invoke("discoverSessionField", new Class<?>[]{Class.class, Object.class},
                new Object[]{MockAmbiguousMinecraft.class, MockAmbiguousMinecraft.INSTANCE});
        require(ambiguous == null, "ambiguous session fields fail closed");

        MockModernMinecraft modern = MockModernMinecraft.INSTANCE;
        modern.real = new ModernSession("ModernOwner", UUID.randomUUID(), "t",
                Optional.<String>empty(), Optional.<String>empty());
        Field typed = (Field) invoke("discoverSessionField", new Class<?>[]{Class.class, Object.class},
                new Object[]{MockModernMinecraft.class, modern});
        require(typed != null && "real".equals(typed.getName()),
                "null-valued structural field skipped while the live one is found");
    }

    private static void inWorldGuard() throws Exception {
        MockWorldMinecraft world = MockWorldMinecraft.INSTANCE;
        world.session = new LegacySession("WorldUser", "12345678123456781234567812345678", "t", "mojang");
        world.launch = world.session;
        setStatic("minecraftInstance", world);
        setStatic("sessionField", MockWorldMinecraft.class.getDeclaredField("session"));
        setStatic("sessionClass", LegacySession.class);
        setStatic("playerField", null);
        setStatic("worldField", null);
        require(!(Boolean) invoke("isInWorld", new Class<?>[0], new Object[0]),
                "no world field set means not in world");

        setStatic("worldField", MockWorldMinecraft.class.getDeclaredField("theWorld"));
        require(!(Boolean) invoke("isInWorld", new Class<?>[0], new Object[0]),
                "null world value means not in world");
        world.theWorld = new Object();
        require((Boolean) invoke("isInWorld", new Class<?>[0], new Object[0]),
                "non-null world value means in world");

        // Wrong field owner: the read throws and the guard must fail closed.
        setStatic("minecraftInstance", new ForeignOwner());
        require((Boolean) invoke("isInWorld", new Class<?>[0], new Object[0]),
                "unreadable player/world fields fail closed as in-world");
        setStatic("worldField", null);
    }

    private static void failClosedMutations() throws Exception {
        // Guard: in world -> refuse before building anything.
        MockWorldMinecraft world = MockWorldMinecraft.INSTANCE;
        world.session = new LegacySession(
                "WorldUser", "12345678123456781234567812345678", "t", "mojang");
        world.launch = world.session;
        world.theWorld = new Object();
        setStatic("ready", Boolean.TRUE);
        setStatic("minecraftInstance", world);
        setStatic("sessionField", MockWorldMinecraft.class.getDeclaredField("session"));
        setStatic("sessionClass", LegacySession.class);
        setStatic("playerField", null);
        setStatic("worldField", MockWorldMinecraft.class.getDeclaredField("theWorld"));
        String inWorld = SessionSwitcher.setSession(
                "Name", "12345678123456781234567812345678", "token");
        require("error:in_world".equals(inWorld), "in-world setSession refused, got " + inWorld);
        String restoreInWorld = SessionSwitcher.restoreSession();
        require("error:in_world".equals(restoreInWorld), "in-world restoreSession refused");
        world.theWorld = null;

        // Blank credentials are rejected before any reflection write.
        String blankName = SessionSwitcher.setSession(
                "  ", "12345678123456781234567812345678", "token");
        require("error:invalid_session_data".equals(blankName), "blank name rejected");
        String blankToken = SessionSwitcher.setSession(
                "Name", "12345678123456781234567812345678", " ");
        require("error:invalid_session_data".equals(blankToken), "blank token rejected");

        // Unsupported constructor shape: refuse instead of guessing.
        setStatic("sessionClass", UnsupportedSession.class);
        setStatic("minecraftInstance", null);
        setStatic("worldField", null);
        String unsupported = SessionSwitcher.setSession(
                "Name", "12345678123456781234567812345678", "token");
        require("error:unsupported_session_constructor".equals(unsupported),
                "unsupported constructor rejected, got " + unsupported);

        // No launch session captured yet: restore must refuse.
        setStatic("launchSession", null);
        String noLaunch = SessionSwitcher.restoreSession();
        require("error:no_launch_session".equals(noLaunch), "missing launch session rejected");

        setStatic("ready", Boolean.FALSE);
        setStatic("sessionClass", null);
    }

    private static void invalidAddresses() throws Exception {
        require("error:invalid_address".equals(SessionSwitcher.joinServer(" ", 25565)),
                "blank host rejected");
        require("error:invalid_address".equals(SessionSwitcher.joinServer("localhost", 0)),
                "port 0 rejected");
        require("error:invalid_address".equals(SessionSwitcher.joinServer("localhost", 65536)),
                "port 65536 rejected");
        StringBuilder longHost = new StringBuilder();
        for (int i = 0; i < 300; i++) longHost.append('a');
        require("error:invalid_address".equals(SessionSwitcher.joinServer(longHost.toString(), 25565)),
                "host longer than 256 characters rejected");

        String noClient = SessionSwitcher.joinServer("localhost", 25565);
        require(noClient.startsWith("error:"), "join without Minecraft fails closed");
        require(!noClient.startsWith("error:invalid_address"),
                "uninitialized client is not reported as a bad address");

        String combined = (String) invoke("combinedAddress", new Class<?>[]{String.class, int.class},
                new Object[]{"2166:0:0:0:0:0:0:1", 25565});
        require("[2166:0:0:0:0:0:0:1]:25565".equals(combined), "IPv6 literal bracketed");
        String plain = (String) invoke("combinedAddress", new Class<?>[]{String.class, int.class},
                new Object[]{"mc.example.com", 25565});
        require("mc.example.com:25565".equals(plain), "host:port combined address");
    }

    private static void directWriteFallback() throws Exception {
        // No scheduler exposed: setSession writes directly at the main menu.
        MockWorldMinecraft world = new MockWorldMinecraft();
        LegacySession original = new LegacySession(
                "Original", "12345678123456781234567812345678", "original-token", "mojang");
        world.session = original;
        world.launch = original;

        setStatic("ready", Boolean.TRUE);
        setStatic("minecraftInstance", world);
        setStatic("sessionField", MockWorldMinecraft.class.getDeclaredField("session"));
        setStatic("sessionClass", LegacySession.class);
        setStatic("scheduleMethod", null);
        setStatic("playerField", null);
        setStatic("worldField", null);
        setStatic("launchSession", original);
        setStatic("getUsername", LegacySession.class.getDeclaredMethod("getUsername"));
        setStatic("getPlayerId", LegacySession.class.getDeclaredMethod("getPlayerID"));

        String result = SessionSwitcher.setSession(
                "Switched", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "switched-token");
        require("ok:Switched".equals(result), "direct write reports the new username, got " + result);
        require(world.session != original, "session field replaced");
        LegacySession written = world.session;
        require("Switched".equals(written.name), "written name");
        require("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".equals(written.uuid), "written uuid text");
        require("switched-token".equals(written.token), "written token");
        require("mojang".equals(written.type), "written account type");

        String info = SessionSwitcher.getSessionInfo();
        require(info.startsWith("ok|Switched|"), "getSessionInfo reports the swapped session, got " + info);
        require(info.endsWith("|false|true"), "getSessionInfo reports not-in-world and ready, got " + info);
        require((Boolean) invoke("isReady", new Class<?>[0], new Object[0]), "isReady after a swap");
    }

    private static void restoreLaunchSession() throws Exception {
        MockWorldMinecraft world = MockWorldMinecraft.INSTANCE;
        LegacySession launch = new LegacySession(
                "LaunchUser", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "launch-token", "mojang");
        world.session = new LegacySession(
                "Other", "cccccccccccccccccccccccccccccccc", "other-token", "mojang");
        world.launch = launch;

        setStatic("ready", Boolean.TRUE);
        setStatic("minecraftInstance", world);
        setStatic("sessionField", MockWorldMinecraft.class.getDeclaredField("session"));
        setStatic("sessionClass", LegacySession.class);
        setStatic("scheduleMethod", null);
        setStatic("playerField", null);
        setStatic("worldField", null);
        setStatic("launchSession", launch);
        setStatic("getUsername", LegacySession.class.getDeclaredMethod("getUsername"));
        setStatic("getPlayerId", LegacySession.class.getDeclaredMethod("getPlayerID"));

        String restored = SessionSwitcher.restoreSession();
        require("ok:LaunchUser".equals(restored), "restore reports the launch username, got " + restored);
        require(world.session == launch, "restore reinstates the captured launch session");
    }

    private static void helpersAndHints() throws Exception {
        String described = (String) invoke("describeThrowable", new Class<?>[]{Throwable.class},
                new Object[]{new java.util.concurrent.CompletionException(
                        new IllegalStateException("boom"))});
        require(described.contains("CompletionException"), "describeThrowable names the outer type");
        require(described.contains("caused by IllegalStateException: boom"),
                "describeThrowable walks the cause chain, got " + described);

        require((Boolean) invoke("isBlank", new Class<?>[]{String.class}, new Object[]{null}),
                "null is blank");
        require((Boolean) invoke("isBlank", new Class<?>[]{String.class}, new Object[]{" \t"}),
                "whitespace is blank");
        require(!(Boolean) invoke("isBlank", new Class<?>[]{String.class}, new Object[]{"x"}),
                "text is not blank");

        SessionSwitcher.hint("net.minecraft.client.Minecraft", "net.minecraft.util.Session");
        SessionSwitcher.hintConnect("net.minecraft.client.gui.GuiConnecting",
                "net.minecraft.client.multiplayer.ServerAddress", "net.minecraft.client.multiplayer.ServerData");
        require(!SessionSwitcher.isReady(), "no Minecraft on the classpath means not ready");

        setStatic("ready", Boolean.FALSE);
        setStatic("sessionClass", null);
    }

    // ── Reflection plumbing ───────────────────────────────────────────────────

    private static void setStatic(String name, Object value) throws Exception {
        Field field = SessionSwitcher.class.getDeclaredField(name);
        field.setAccessible(true);
        if (value instanceof Boolean && field.getType() == boolean.class) {
            field.setBoolean(null, (Boolean) value);
            return;
        }
        field.set(null, value);
    }

    private static Object invoke(String name, Class<?>[] parameterTypes, Object[] args) throws Exception {
        Method method = SessionSwitcher.class.getDeclaredMethod(name, parameterTypes);
        method.setAccessible(true);
        try {
            return method.invoke(null, args);
        } catch (java.lang.reflect.InvocationTargetException e) {
            Throwable cause = e.getCause();
            if (cause instanceof Exception) throw (Exception) cause;
            throw e;
        }
    }

    private static void require(boolean condition, String label) {
        checks++;
        if (!condition) throw new AssertionError("FAILED: " + label);
    }
}
