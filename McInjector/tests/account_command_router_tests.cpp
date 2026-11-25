// tests/account_command_router_tests.cpp
// Wire-format test for jni_core/account_command_router.h.
//
// SessionHelperBridge is stubbed so the test needs no JVM: it only checks that
// the helper's "ok:<detail>" / "error:<code>" protocol strings become the exact
// commandResult lines the loader parses, that unknown packets are left alone, and
// that no reply ever carries token material.
#include <cstdio>
#include <cstring>
#include <string>

#include "jni_core/account_command_router.h"

namespace {

std::string gSetOutcome = "ok:Notch";
std::string gInfoOutcome = "ok|Notch|069a79f444e94726a5befca90e38aaf5|false|true";
std::string gJoinOutcome = "ok:mc.example.com:25565";
bool gLoaded = true;
bool gReady = true;

// Last credentials handed to the stub helper, so the parser test can prove a long
// token survives lc::SimpleJsonConfigReader intact.
std::string gLastToken;
std::string gLastUuid;
int gJoinCalls = 0;

int failures = 0;

void ExpectEq(const std::string& expected, const std::string& actual, const char* label) {
    if (expected == actual) return;
    std::fprintf(stderr, "FAIL %s:\n  expected: %s\n  actual:   %s\n",
                 label, expected.c_str(), actual.c_str());
    ++failures;
}

void ExpectTrue(bool condition, const char* label) {
    if (condition) return;
    std::fprintf(stderr, "FAIL %s\n", label);
    ++failures;
}

// Runs one inbound line and returns the reply (empty when not handled).
std::string Route(const char* line) {
    lc::SimpleJsonConfigReader reader(line);
    std::string reply;
    AccountCommandRouter::Handle(reinterpret_cast<JNIEnv*>(1), reader.GetString("type"), reader, &reply);
    return reply;
}

} // namespace

namespace SessionHelperBridge {

bool Load(JNIEnv*, jobject) { gLoaded = true; return true; }
bool IsLoaded() { return gLoaded; }
void Unload(JNIEnv*) {}
bool SetHints(JNIEnv*, const char*, const char*) { return true; }
bool SetConnectHints(JNIEnv*, const char*, const char*, const char*) { return true; }
std::string Init(JNIEnv*) { return "ok"; }

bool GetSessionInfo(JNIEnv*, SessionInfo& out) {
    out = SessionInfo();
    if (gInfoOutcome.compare(0, 3, "ok|") == 0) {
        // "ok|username|uuid|inWorld|ready", so a test can supply any username.
        const std::size_t nameAt = 3;
        const std::size_t uuidAt = gInfoOutcome.find('|', nameAt);
        const std::size_t flagsAt = gInfoOutcome.find('|', uuidAt + 1);
        out.ok = true;
        out.username = gInfoOutcome.substr(nameAt, uuidAt - nameAt);
        out.uuid = gInfoOutcome.substr(uuidAt + 1, flagsAt - uuidAt - 1);
        out.inWorld = false;
        out.ready = true;
    } else {
        out.error = gInfoOutcome.substr(6);
    }
    return out.ok;
}

std::string SetSession(JNIEnv*, const std::string&, const std::string& uuid, const std::string& token) {
    gLastUuid = uuid;
    gLastToken = token;
    return gSetOutcome;
}

std::string RestoreSession(JNIEnv*) { return gSetOutcome; }

std::string JoinServer(JNIEnv*, const std::string& host, int port) {
    (void)host;
    (void)port;
    ++gJoinCalls;
    return gJoinOutcome;
}

bool IsReady(JNIEnv*) { return gReady; }

} // namespace SessionHelperBridge

// Pop one queued reply from the single reply FIFO (empty when none).
static std::string TakeQueuedReply() {
    std::string reply;
    if (!AccountCommandRouter::PeekReply(&reply)) return std::string();
    AccountCommandRouter::PopReply();
    return reply;
}

// Everything queued right now, in production order.
static std::vector<std::string> DrainReplies() {
    std::vector<std::string> out;
    std::string reply;
    while (AccountCommandRouter::PeekReply(&reply)) {
        out.push_back(reply);
        AccountCommandRouter::PopReply();
    }
    return out;
}

// A real setSession token is a 1-2 KB JWT/MSA blob of base64url characters. Feed
// the flat lc::SimpleJsonConfigReader one and prove it arrives byte-for-byte.
static void ParseLongToken() {
    std::string token;
    token.reserve(2048);
    const char* alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.";
    for (std::size_t i = 0; i < 2048; ++i) token += alphabet[i % 65];

    const std::string line =
        "{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":200,"
        "\"name\":\"Notch\",\"uuid\":\"069a79f4-44e9-4726-a5be-fca90e38aaf5\","
        "\"token\":\"" + token + "\"}";

    gLastToken.clear();
    gLastUuid.clear();
    const std::string reply = Route(line.c_str());

    ExpectTrue(gLastToken.size() == 2048, "2 KB token reaches the helper intact");
    ExpectTrue(gLastToken == token, "2 KB base64url token value matches byte for byte");
    ExpectTrue(gLastUuid == "069a79f4-44e9-4726-a5be-fca90e38aaf5",
               "uuid after a 2 KB token still parses");
    ExpectTrue(reply.find(token) == std::string::npos, "2 KB token never echoed in the reply");
    ExpectTrue(reply.find("\"requestId\":200") != std::string::npos,
               "long-line setSession still correlates by requestId");
    std::printf("long token: %zu bytes parsed, reply %zu bytes\n", gLastToken.size(), reply.size());
}

// Inbound framing: a command line may span recv() chunks, and anything above the
// 64 KiB cap must be answered with line_too_long instead of being parsed.
static void InboundLineGuard() {
    using AccountCommandRouter::ClearReplies;
    using AccountCommandRouter::InboundCommandTracker;
    using AccountCommandRouter::kCommandLineLimit;

    // 1. A setSession line longer than one 4096-byte recv chunk, split exactly the
    //    way the bridge loop sees it: no rejection, no truncation.
    {
        std::string token;
        for (std::size_t i = 0; i < 4000; ++i) token += "abcdefghij"[i % 10];
        const std::string line =
            "{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":201,"
            "\"name\":\"Notch\",\"uuid\":\"069a79f444e94726a5befca90e38aaf5\","
            "\"token\":\"" + token + "\"}\n";
        ExpectTrue(line.size() > 4096, "test line really is longer than one recv chunk");

        ClearReplies();
        InboundCommandTracker tracker;
        for (std::size_t offset = 0; offset < line.size(); offset += 4096) {
            const std::size_t chunk = (line.size() - offset < 4096) ? line.size() - offset : 4096;
            tracker.Observe(line.data() + offset, chunk);
        }
        ExpectTrue(DrainReplies().empty(), "4096-byte chunking of a 2 KB-token line is not rejected");
    }

    // 2. An over-long accountCommand line: exactly one rejection, echoing the
    //    requestId and action recovered from the head.
    {
        ClearReplies();
        InboundCommandTracker tracker;
        std::string line =
            "{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":202,"
            "\"name\":\"Notch\",\"uuid\":\"069a79f4-44e9-4726-a5be-fca90e38aaf5\","
            "\"token\":\"";
        line.append(kCommandLineLimit, 'a');
        line += "\"}\n";

        for (std::size_t offset = 0; offset < line.size(); offset += 4096) {
            const std::size_t chunk = (line.size() - offset < 4096) ? line.size() - offset : 4096;
            tracker.Observe(line.data() + offset, chunk);
        }
        const std::vector<std::string> replies = DrainReplies();
        ExpectTrue(replies.size() == 1, "over-long line is rejected exactly once");
        ExpectEq("{\"type\":\"commandResult\",\"requestId\":202,\"ok\":false,"
                 "\"action\":\"setSession\",\"error\":\"line_too_long\"}\n",
                 replies.empty() ? std::string() : replies[0],
                 "over-long command line rejected with line_too_long");
    }

    // 3. A config line that big must stay untouched by the command guard.
    {
        ClearReplies();
        InboundCommandTracker tracker;
        std::string line = "{\"type\":\"config\",\"armed\":false,\"pad\":\"";
        line.append(kCommandLineLimit * 2, 'x');
        line += "\"}\n";
        for (std::size_t offset = 0; offset < line.size(); offset += 4096) {
            const std::size_t chunk = (line.size() - offset < 4096) ? line.size() - offset : 4096;
            tracker.Observe(line.data() + offset, chunk);
        }
        ExpectTrue(DrainReplies().empty(), "large config line is never answered by the command guard");
    }

    // 4. State resets at the newline: the next line is judged on its own.
    {
        ClearReplies();
        InboundCommandTracker tracker;
        std::string big = "{\"type\":\"serverCommand\",\"action\":\"join\",\"requestId\":203,\"host\":\"";
        big.append(kCommandLineLimit, 'h');
        big += "\"}\n";
        tracker.Observe(big.data(), big.size());
        ExpectTrue(DrainReplies().size() == 1, "over-long join line rejected");
        const char* next = "{\"type\":\"serverCommand\",\"action\":\"join\",\"requestId\":204,"
                           "\"host\":\"h\",\"port\":25565}\n";
        tracker.Observe(next, std::strlen(next));
        ExpectTrue(DrainReplies().empty(), "following short line is not rejected");
    }

    // 5. Leading whitespace before the opening brace is tolerated.
    {
        ClearReplies();
        InboundCommandTracker tracker;
        std::string line = "  {\"type\":\"serverCommand\",\"action\":\"join\",\"requestId\":205,\"host\":\"";
        line.append(kCommandLineLimit, 'h');
        line += "\"}\n";
        tracker.Observe(line.data(), line.size());
        ExpectTrue(DrainReplies().size() == 1, "leading whitespace still classified");
    }

    // 6. Exactly at the cap is still allowed; one byte over is not.
    {
        const std::string head =
            "{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":206,\"pad\":\"";
        const std::size_t tail = 2;   // closing quote + closing brace
        std::string atCap = head;
        atCap.append(kCommandLineLimit - head.size() - tail, 'p');
        atCap += "\"}";
        ExpectTrue(atCap.size() == kCommandLineLimit, "guard boundary line built at the cap");

        ClearReplies();
        InboundCommandTracker tracker;
        tracker.Observe(atCap.data(), atCap.size());
        ExpectTrue(DrainReplies().empty(), "line at the 64 KiB cap is allowed");
        tracker.Observe("\n", 1);

        std::string overCap = head;
        overCap.append(kCommandLineLimit - head.size() - tail + 1, 'p');
        overCap += "\"}";
        ClearReplies();
        InboundCommandTracker tracker2;
        tracker2.Observe(overCap.data(), overCap.size());
        ExpectTrue(DrainReplies().size() == 1, "line one byte over the cap is rejected");
    }

    // 7. The rejection reply never carries the token-shaped payload.
    {
        ClearReplies();
        InboundCommandTracker tracker;
        std::string line = "{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":207,"
                           "\"name\":\"N\",\"uuid\":\"u\",\"token\":\"";
        line.append(kCommandLineLimit, 'z');
        line += "\"}\n";
        tracker.Observe(line.data(), line.size());
        const std::string reply = TakeQueuedReply();
        ExpectTrue(reply.find('z') == std::string::npos, "rejection reply carries no payload bytes");
    }

    // 8. Two over-long command lines are both answered, in order.
    {
        ClearReplies();
        InboundCommandTracker tracker;
        std::string both;
        for (int i = 0; i < 2; ++i) {
            both += "{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":2";
            both += static_cast<char>('0' + i);
            both += ",\"name\":\"N\",\"uuid\":\"u\",\"token\":\"";
            both.append(kCommandLineLimit, 'y');
            both += "\"}\n";
        }
        tracker.Observe(both.data(), both.size());

        const std::vector<std::string> replies = DrainReplies();
        ExpectTrue(replies.size() == 2, "queue drains exactly two replies");
        if (replies.size() == 2) {
            ExpectTrue(replies[0].find("\"requestId\":20") != std::string::npos, "first reply correlates");
            ExpectTrue(replies[1].find("\"requestId\":21") != std::string::npos, "second reply correlates");
        }
    }

    // 9. CRLF framing is not content: a line at the cap followed by CRLF is within
    //    the cap for ParseConfig too, so it must not be rejected as well (#6).
    {
        const std::string head =
            "{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":209,\"pad\":\"";
        const std::size_t tail = 2;
        std::string atCap = head;
        atCap.append(kCommandLineLimit - head.size() - tail, 'p');
        atCap += "\"}";

        ClearReplies();
        InboundCommandTracker tracker;
        tracker.Observe(atCap.data(), atCap.size());
        ExpectTrue(tracker.Observe("\r\n", 2) == false, "Observe reports no rejection for CRLF at the cap");
        ExpectTrue(DrainReplies().empty(), "line at the cap + CRLF is not rejected");
    }

    // 10. The CR may land in the next recv chunk and must be dropped the same way.
    {
        const std::string head =
            "{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":210,\"pad\":\"";
        const std::size_t tail = 2;
        std::string atCap = head;
        atCap.append(kCommandLineLimit - head.size() - tail, 'p');
        atCap += "\"}";

        ClearReplies();
        InboundCommandTracker tracker;
        tracker.Observe(atCap.data(), atCap.size());
        tracker.Observe("\r", 1);
        tracker.Observe("\n", 1);
        ExpectTrue(DrainReplies().empty(), "CRLF split across recv chunks is not counted");
    }

    // 11. One byte over the cap with CRLF is still rejected ...
    {
        const std::string head =
            "{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":211,\"pad\":\"";
        const std::size_t tail = 2;
        std::string overCap = head;
        overCap.append(kCommandLineLimit - head.size() - tail + 1, 'p');
        overCap += "\"}";

        ClearReplies();
        InboundCommandTracker tracker;
        tracker.Observe(overCap.data(), overCap.size());
        tracker.Observe("\r\n", 2);
        ExpectTrue(DrainReplies().size() == 1, "line one byte over the cap + CRLF is rejected");
    }

    // 12. ... and a lone CR is content, because the frame buffer only strips the CR
    //     that precedes LF.
    {
        const std::string head =
            "{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":212,\"pad\":\"";
        const std::size_t tail = 2;
        std::string atCap = head;
        atCap.append(kCommandLineLimit - head.size() - tail, 'p');
        atCap += "\"}";
        std::string withCr = atCap.substr(0, atCap.size() - 2) + "\r}";

        ClearReplies();
        InboundCommandTracker tracker;
        tracker.Observe(withCr.data(), withCr.size());
        ExpectTrue(DrainReplies().empty(), "line at the cap with a lone CR is still at the cap");
        tracker.Observe("x", 1);
        ExpectTrue(DrainReplies().size() == 1, "the lone CR then counts as content");
    }
}

// The reply FIFO is ordered, lossless on peek, and bounded.
static void ReplyQueue() {
    using AccountCommandRouter::ClearReplies;
    using AccountCommandRouter::kPendingReplyLimit;
    using AccountCommandRouter::PeekReply;
    using AccountCommandRouter::PopReply;
    using AccountCommandRouter::PushReply;

    ClearReplies();
    PushReply("first\n");
    PushReply("second\n");
    std::string front;
    ExpectTrue(PeekReply(&front) && front == "first\n", "peek returns the front without consuming");
    ExpectTrue(PeekReply(&front) && front == "first\n", "a backpressured retry sees the same reply");
    PopReply();
    ExpectTrue(PeekReply(&front) && front == "second\n", "pop advances to the next reply");
    PopReply();
    ExpectTrue(!PeekReply(&front), "queue is empty after both pops");
    PopReply();   // must be a no-op, not a crash
    ExpectTrue(!PeekReply(&front), "popping an empty queue stays empty");

    // A stuck socket must not grow the queue without limit: the oldest replies go.
    ClearReplies();
    for (std::size_t i = 0; i < kPendingReplyLimit + 3; ++i)
        PushReply("r" + std::to_string(i) + "\n");
    const std::vector<std::string> replies = DrainReplies();
    ExpectTrue(replies.size() == kPendingReplyLimit, "reply queue is bounded at the cap");
    if (replies.size() == kPendingReplyLimit) {
        ExpectTrue(replies.front() == "r3\n", "at the cap the oldest replies are dropped");
        ExpectTrue(replies.back() == "r" + std::to_string(kPendingReplyLimit + 2) + "\n",
                   "the newest reply is kept");
    }
}

// An over-long line's answer is queued before the result of the line after it, so
// one queue keeps the wire order deterministic (#12).
static void ReplyQueueOrder() {
    using AccountCommandRouter::ClearReplies;
    using AccountCommandRouter::InboundCommandTracker;
    using AccountCommandRouter::kCommandLineLimit;

    ClearReplies();
    InboundCommandTracker tracker;
    std::string big = "{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":213,\"pad\":\"";
    big.append(kCommandLineLimit, 'q');
    big += "\"}\n";
    tracker.Observe(big.data(), big.size());

    // What the bridge does next: ParseConfig routes the following line.
    AccountCommandRouter::PushReply(
        Route("{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":213}"));

    const std::vector<std::string> replies = DrainReplies();
    ExpectTrue(replies.size() == 2, "two replies queued for two lines");
    if (replies.size() == 2) {
        ExpectTrue(replies[0].find("line_too_long") != std::string::npos,
                   "over-long answer is queued first");
        ExpectTrue(replies[1].find("\"ok\":true") != std::string::npos,
                   "routed result follows it");
    }
}

// Only the one serverCommand action this bridge implements is executed (#7).
static void ServerCommandActions() {
    gJoinCalls = 0;
    ExpectEq("{\"type\":\"commandResult\",\"requestId\":130,\"ok\":false,\"action\":\"ping\","
             "\"error\":\"unknown_action\"}\n",
             Route("{\"type\":\"serverCommand\",\"action\":\"ping\",\"requestId\":130,\"host\":\"h\"}"),
             "unknown serverCommand action is not executed as a join");
    ExpectTrue(gJoinCalls == 0, "unknown serverCommand action never reaches JoinServer");

    ExpectEq("{\"type\":\"commandResult\",\"requestId\":131,\"ok\":false,\"action\":\"list\","
             "\"error\":\"unknown_action\"}\n",
             Route("{\"type\":\"serverCommand\",\"action\":\"list\",\"requestId\":131}"),
             "a future serverCommand action is refused, not joined");
    ExpectTrue(gJoinCalls == 0, "still no join for a future action");

    gJoinOutcome = "ok:mc.example.com:25565";
    ExpectTrue(Route("{\"type\":\"serverCommand\",\"action\":\"join\",\"requestId\":132,\"host\":\"h\"}")
                   .find("\"ok\":true") != std::string::npos,
               "join action still routes");
    ExpectTrue(gJoinCalls == 1, "join action reaches JoinServer exactly once");
}

// Non-ASCII replies are UTF-8, not per-byte \u00XX escapes (#8).
static void Utf8Reply() {
    // "Ünïcødé" as UTF-8, never as the mojibake the per-byte escaping produced.
    const char* utf8Name = "\xc3\x9c" "n\xc3\xaf\xc3\xa7\xc3\xb8" "d\xc3\xa9";
    const std::string saved = gInfoOutcome;
    gInfoOutcome = std::string("ok|") + utf8Name + "|069a79f444e94726a5befca90e38aaf5|false|true";

    const std::string reply =
        Route("{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":133}");
    ExpectTrue(reply.find(utf8Name) != std::string::npos,
               "UTF-8 username is carried verbatim in the reply");
    ExpectTrue(reply.find("\\u00c3") == std::string::npos,
               "reply does not escape each UTF-8 byte as a Latin-1 code point");
    gInfoOutcome = saved;

    // Control characters still have to be escaped for the frame to stay valid JSON.
    const std::string escaped = AccountCommandRouter::JsonEscape(std::string("a\x01\x7f", 3));
    ExpectEq("a\\u0001\\u007f", escaped, "control bytes are still \\u escaped");
    ExpectEq(utf8Name, AccountCommandRouter::JsonEscape(utf8Name),
             "multi-byte UTF-8 passes through JsonEscape untouched");
}

int main() {
    // Contract shapes, byte for byte.
    ExpectEq("{\"type\":\"commandResult\",\"requestId\":123,\"ok\":true,\"action\":\"getSession\","
             "\"username\":\"Notch\",\"uuid\":\"069a79f444e94726a5befca90e38aaf5\","
             "\"inWorld\":false,\"ready\":true}\n",
             Route("{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":123}"),
             "getSession reply");

    ExpectEq("{\"type\":\"commandResult\",\"requestId\":124,\"ok\":true,\"action\":\"setSession\","
             "\"username\":\"Notch\",\"uuid\":\"069a79f444e94726a5befca90e38aaf5\","
             "\"inWorld\":false,\"ready\":true}\n",
             Route("{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":124,"
                   "\"name\":\"Notch\",\"uuid\":\"069a79f4-44e9-4726-a5be-fca90e38aaf5\","
                   "\"token\":\"secret-token-value\"}"),
             "setSession reply");

    ExpectEq("{\"type\":\"commandResult\",\"requestId\":125,\"ok\":true,\"action\":\"restoreSession\","
             "\"username\":\"Notch\",\"uuid\":\"069a79f444e94726a5befca90e38aaf5\","
             "\"inWorld\":false,\"ready\":true}\n",
             Route("{\"type\":\"accountCommand\",\"action\":\"restoreSession\",\"requestId\":125}"),
             "restoreSession reply");

    ExpectEq("{\"type\":\"commandResult\",\"requestId\":126,\"ok\":true,\"action\":\"join\","
             "\"address\":\"mc.example.com:25565\",\"ready\":true}\n",
             Route("{\"type\":\"serverCommand\",\"action\":\"join\",\"requestId\":126,"
                   "\"host\":\"mc.example.com\",\"port\":25565}"),
             "join reply");

    // Failure codes pass through untouched.
    gJoinOutcome = "error:connect_path_not_found";
    ExpectEq("{\"type\":\"commandResult\",\"requestId\":127,\"ok\":false,\"action\":\"join\","
             "\"error\":\"connect_path_not_found\"}\n",
             Route("{\"type\":\"serverCommand\",\"action\":\"join\",\"requestId\":127,"
                   "\"host\":\"mc.example.com\"}"),
             "join failure reply");

    gSetOutcome = "error:in_world";
    ExpectEq("{\"type\":\"commandResult\",\"requestId\":9,\"ok\":false,\"action\":\"setSession\","
             "\"error\":\"in_world\",\"inWorld\":true}\n",
             Route("{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":9,"
                   "\"name\":\"a\",\"uuid\":\"b\",\"token\":\"c\"}"),
             "in-world refusal reply");
    gSetOutcome = "ok:Notch";

    // Port bounds are enforced before the helper is called.
    ExpectEq("{\"type\":\"commandResult\",\"requestId\":128,\"ok\":false,\"action\":\"join\","
             "\"error\":\"invalid_address\"}\n",
             Route("{\"type\":\"serverCommand\",\"action\":\"join\",\"requestId\":128,"
                   "\"host\":\"h\",\"port\":0}"),
             "out-of-range port rejected");

    // Unknown actions and missing helper answers.
    ExpectEq("{\"type\":\"commandResult\",\"requestId\":1,\"ok\":false,\"action\":\"nope\","
             "\"error\":\"unknown_action\"}\n",
             Route("{\"type\":\"accountCommand\",\"action\":\"nope\",\"requestId\":1}"),
             "unknown action reply");

    gLoaded = false;
    ExpectEq("{\"type\":\"commandResult\",\"requestId\":2,\"ok\":false,\"action\":\"getSession\","
             "\"error\":\"Session helper not ready (wait on main menu, then retry)\"}\n",
             Route("{\"type\":\"accountCommand\",\"action\":\"getSession\",\"requestId\":2}"),
             "helper not loaded guard");
    gLoaded = true;

    // A missing JNIEnv is the same guard, not a crash.
    {
        lc::SimpleJsonConfigReader reader(
            "{\"type\":\"serverCommand\",\"action\":\"join\",\"requestId\":3,\"host\":\"h\"}");
        std::string reply;
        AccountCommandRouter::Handle(nullptr, reader.GetString("type"), reader, &reply);
        ExpectEq("{\"type\":\"commandResult\",\"requestId\":3,\"ok\":false,\"action\":\"join\","
                 "\"error\":\"Session helper not ready (wait on main menu, then retry)\"}\n",
                 reply, "null env guard");
    }

    // Non-command packets must stay untouched.
    {
        lc::SimpleJsonConfigReader reader("{\"type\":\"config\",\"armed\":false}");
        std::string reply;
        bool handled = AccountCommandRouter::Handle(
            reinterpret_cast<JNIEnv*>(1), reader.GetString("type"), reader, &reply);
        ExpectTrue(!handled, "config packet not handled");
        ExpectTrue(reply.empty(), "config packet produces no reply");

        lc::SimpleJsonConfigReader module("{\"type\":\"moduleAction\",\"action\":\"autoRod\"}");
        handled = AccountCommandRouter::Handle(
            reinterpret_cast<JNIEnv*>(1), module.GetString("type"), module, &reply);
        ExpectTrue(!handled, "moduleAction packet not handled");
    }

    // requestId 0 is allowed and echoed.
    ExpectEq("{\"type\":\"commandResult\",\"requestId\":0,\"ok\":true,\"action\":\"getSession\","
             "\"username\":\"Notch\",\"uuid\":\"069a79f444e94726a5befca90e38aaf5\","
             "\"inWorld\":false,\"ready\":true}\n",
             Route("{\"type\":\"accountCommand\",\"action\":\"getSession\"}"),
             "missing requestId defaults to 0");

    // No reply may leak the access token that was supplied.
    {
        const std::string reply = Route(
            "{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":11,"
            "\"name\":\"Notch\",\"uuid\":\"069a79f4-44e9-4726-a5be-fca90e38aaf5\","
            "\"token\":\"eyJhbGciOiJub25lIn0.secret\"}");
        ExpectTrue(reply.find("eyJhbGciOiJub25lIn0") == std::string::npos,
                   "reply never carries the token");
        ExpectTrue(reply.find("secret") == std::string::npos, "reply never carries token material");
    }

    // Quoted diagnostics stay valid JSON-escaped text.
    gSetOutcome = "error:bad \"quote\" \\ here";
    ExpectEq("{\"type\":\"commandResult\",\"requestId\":12,\"ok\":false,\"action\":\"setSession\","
             "\"error\":\"bad \\\"quote\\\" \\\\ here\"}\n",
             Route("{\"type\":\"accountCommand\",\"action\":\"setSession\",\"requestId\":12,"
                   "\"name\":\"a\",\"uuid\":\"b\",\"token\":\"c\"}"),
             "diagnostic escaping");

    ParseLongToken();
    InboundLineGuard();
    ReplyQueue();
    ReplyQueueOrder();
    ServerCommandActions();
    Utf8Reply();

    if (failures == 0) {
        std::printf("account_command_router_tests: all checks passed.\n");
        return 0;
    }
    std::fprintf(stderr, "account_command_router_tests: %d failure(s).\n", failures);
    return 1;
}
