#pragma once
// jni_core/account_command_router.h
// Shared "accountCommand" / "serverCommand" handling for both bridges.
//
// The loader sends one JSON object per line over the existing control socket:
//   {"type":"accountCommand","action":"getSession","requestId":123}
//   {"type":"accountCommand","action":"setSession","requestId":124,
//    "name":"Notch","uuid":"<uuid>","token":"<msa token>","refreshToken":"<optional>"}
//   {"type":"accountCommand","action":"restoreSession","requestId":125}
//   {"type":"serverCommand","action":"join","requestId":126,
//    "host":"mc.example.com","port":25565}
//
// Each command produces exactly one reply, queued for the inbound socket thread
// to write with the bridge's own send path (never a second writer):
//   {"type":"commandResult","requestId":124,"ok":true,"action":"setSession",
//    "username":"Notch","uuid":"...","inWorld":false,"ready":true}
//
// Access tokens are never echoed or logged: only name/uuid/username/inWorld/ready
// and the helper's short error code leave this module.

#include <jni.h>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

#include "json_config_reader.h"
#include "session_helper_bridge.h"

namespace AccountCommandRouter {

// Shown verbatim when the helper class is not present in the JVM yet.
inline const char* NotReadyMessage() {
    return "Session helper not ready (wait on main menu, then retry)";
}

// Reply accumulator. Fields are only emitted when their flag is set so the
// commandResult shape stays stable for the managed side.
struct CommandResult {
    int         requestId = 0;
    std::string action;
    bool        ok = false;
    std::string error;
    std::string username;
    std::string uuid;
    std::string address;
    bool        hasSession = false;   // username/uuid/inWorld/ready present
    bool        inWorld = false;
    bool        ready = false;
    bool        hasReady = false;     // ready present without session details
};

inline std::string JsonEscape(const std::string& value) {
    std::string out;
    out.reserve(value.size() + 8);
    for (std::size_t i = 0; i < value.size(); ++i) {
        const unsigned char c = static_cast<unsigned char>(value[i]);
        switch (c) {
            case '"':  out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n";  break;
            case '\r': out += "\\r";  break;
            case '\t': out += "\\t";  break;
            default:
                if (c < 0x20 || c == 0x7F) {
                    char buffer[8];
                    std::snprintf(buffer, sizeof(buffer), "\\u%04x", c);
                    out += buffer;
                } else {
                    // >= 0x80 belongs to a UTF-8 sequence produced by TakeString()
                    // and JSON carries it verbatim. Escaping it per byte would turn
                    // one character into several Latin-1 code points.
                    out += static_cast<char>(c);
                }
                break;
        }
    }
    return out;
}

// Serialize one commandResult. The trailing newline is part of the frame.
inline std::string Serialize(const CommandResult& result) {
    std::string out = "{\"type\":\"commandResult\",\"requestId\":";
    out += std::to_string(result.requestId);
    out += ",\"ok\":";
    out += result.ok ? "true" : "false";
    out += ",\"action\":\"";
    out += JsonEscape(result.action);
    out += "\"";

    if (result.hasSession) {
        out += ",\"username\":\"";
        out += JsonEscape(result.username);
        out += "\",\"uuid\":\"";
        out += JsonEscape(result.uuid);
        out += "\",\"inWorld\":";
        out += result.inWorld ? "true" : "false";
        out += ",\"ready\":";
        out += result.ready ? "true" : "false";
    } else if (!result.address.empty()) {
        out += ",\"address\":\"";
        out += JsonEscape(result.address);
        out += "\"";
        if (result.hasReady) {
            out += ",\"ready\":";
            out += result.ready ? "true" : "false";
        }
    }

    if (!result.ok) {
        out += ",\"error\":\"";
        out += JsonEscape(result.error);
        out += "\"";
        // The in-world refusal is the one failure the caller can act on.
        if (result.error == "in_world") out += ",\"inWorld\":true";
    }

    out += "}\n";
    return out;
}

inline std::string StripErrorPrefix(const std::string& outcome) {
    if (outcome.compare(0, 6, "error:") == 0) return outcome.substr(6);
    return outcome;
}

// ── Reply queue ───────────────────────────────────────────────────────────────
// One FIFO for every commandResult, in the order the inbound lines produced them:
// InboundCommandTracker (over-long lines) and ParseConfig (routed commands) both
// push here. The bridge's inbound socket thread is the only reader, and it peeks,
// sends, and pops only once the socket accepted the whole line — so a
// backpressured socket delays a reply instead of dropping it. No other thread
// touches the queue, so it needs no lock and takes none.

// A stuck socket must not grow the queue without limit. At the cap the oldest
// reply is dropped: it has been unsendable the longest and its caller is closest
// to its own timeout.
const std::size_t kPendingReplyLimit = 64;

inline std::vector<std::string>& PendingReplies() {
    static std::vector<std::string> replies;
    return replies;
}

inline void PushReply(const std::string& line) {
    if (line.empty()) return;
    std::vector<std::string>& replies = PendingReplies();
    if (replies.size() >= kPendingReplyLimit) replies.erase(replies.begin());
    replies.push_back(line);
}

// Front of the queue without removing it. Pair with PopReply() only after the
// complete line was written.
inline bool PeekReply(std::string* line) {
    if (!line) return false;
    const std::vector<std::string>& replies = PendingReplies();
    if (replies.empty()) return false;
    *line = replies.front();
    return true;
}

inline void PopReply() {
    std::vector<std::string>& replies = PendingReplies();
    if (!replies.empty()) replies.erase(replies.begin());
}

inline void ClearReplies() {
    PendingReplies().clear();
}

// ── Inbound command line guard ────────────────────────────────────────────────
// A setSession frame carries a name, a UUID and a 1-2 KB access token, so it does
// not fit one recv() chunk. The bridges reassemble newline-framed lines across
// reads (BoundedNewlineBuffer keeps state between recv calls); this tracker also
// watches the raw stream so a command line above the reference tool's 64 KiB cap
// is answered with an error instead of being parsed from a partial buffer — or
// dropped silently by the transport when it exceeds the frame buffer.

// Reference cap for one inbound command line (64 KiB).
const std::size_t kCommandLineLimit = 64 * 1024;

// First bytes of the line kept so an over-long line can still echo its requestId
// and action. A truncated parse of this head never touches the credential fields.
const std::size_t kCommandHeadProbe = 256;

class InboundCommandTracker {
public:
    InboundCommandTracker() { ResetLine(); }

    // Feed one raw recv() chunk, before the frame buffer sees it. Returns true
    // when this chunk answered an over-long line.
    bool Observe(const char* data, std::size_t length) {
        if (!data) return false;
        bool answered = false;
        for (std::size_t i = 0; i < length; ++i) {
            const char current = data[i];
            if (current == '\n') {
                // The CR of a CRLF pair is framing, not content: the frame buffer
                // strips it before the line reaches ParseConfig, so counting it
                // here would reject a line that is still within the cap there.
                _pendingCr = false;
                ResetLine();
                continue;
            }
            if (_pendingCr) {
                _pendingCr = false;
                AppendByte('\r', answered);
            }
            if (current == '\r') {
                _pendingCr = true;
                continue;
            }
            AppendByte(current, answered);
        }
        return answered;
    }

private:
    void ResetLine() {
        _head.clear();
        _lineBytes = 0;
        _couldBeCommand = true;
        _reported = false;
        _pendingCr = false;
    }

    // Account for one content byte, in the same order the bridge sees it: the
    // tracker runs before the frame buffer, so the cap is checked here first.
    void AppendByte(char current, bool& answered) {
        ++_lineBytes;
        if (_head.size() < kCommandHeadProbe) _head += current;
        // The head is capped at kCommandHeadProbe, so once it is full the answer
        // cannot change any more; recomputing it per byte made long lines
        // quadratic on the thread that also publishes state.
        if (_couldBeCommand && _head.size() < kCommandHeadProbe) {
            _couldBeCommand = HeadCouldBeCommand(_head);
        }
        // Report once per line, as soon as the cap is crossed.
        if (!_reported && _lineBytes > kCommandLineLimit && _couldBeCommand) {
            _reported = true;
            PushReply(BuildOversizeReply());
            answered = true;
        }
    }

    // True while the line head could still begin an accountCommand/serverCommand.
    // Compares in place: the head is re-examined on every byte of a long line, and
    // a fresh substring per call made that quadratic in allocations.
    static bool HeadCouldBeCommand(const std::string& head) {
        static const char* const kOpenings[] = {
            "{\"type\":\"accountCommand\"",
            "{\"type\":\"serverCommand\""
        };
        std::size_t at = 0;
        while (at < head.size() && (head[at] == ' ' || head[at] == '\t' || head[at] == '\r')) ++at;
        if (at == head.size()) return true;   // nothing compared yet
        const std::size_t length = head.size() - at;
        for (std::size_t i = 0; i < sizeof(kOpenings) / sizeof(kOpenings[0]); ++i) {
            const char* opening = kOpenings[i];
            const std::size_t openingLength = std::strlen(opening);
            if (length <= openingLength) {
                if (std::memcmp(opening, head.data() + at, length) == 0) return true;
            } else if (std::memcmp(opening, head.data() + at, openingLength) == 0) {
                return true;
            }
        }
        return false;
    }

    std::string BuildOversizeReply() const {
        CommandResult result;
        // The head stops well before the token, so this never sees credential data.
        const lc::SimpleJsonConfigReader reader(_head);
        result.requestId = reader.GetInt("requestId", 0);
        const std::string action = reader.GetString("action");
        result.action = action.empty() ? "unknown" : action;
        result.error = "line_too_long";
        return Serialize(result);
    }

    std::string _head;
    std::size_t _lineBytes = 0;
    bool        _couldBeCommand = true;
    bool        _reported = false;
    bool        _pendingCr = false;
};

// "ok:<username>" / "error:<code>" helpers.
inline bool IsOk(const std::string& outcome) {
    return outcome.compare(0, 3, "ok:") == 0;
}

// Fill username/uuid/inWorld/ready from the helper after a successful write.
inline void FillSessionDetails(JNIEnv* env, CommandResult& result) {
    SessionHelperBridge::SessionInfo info;
    if (SessionHelperBridge::GetSessionInfo(env, info)) {
        result.ok = true;
        result.hasSession = true;
        result.username = info.username;
        result.uuid = info.uuid;
        result.inWorld = info.inWorld;
        result.ready = info.ready;
    } else {
        result.error = info.error.empty() ? "getSession_failed" : info.error;
    }
}

inline void HandleGetSession(JNIEnv* env, CommandResult& result) {
    FillSessionDetails(env, result);
}

inline void HandleSetSession(JNIEnv* env, const lc::SimpleJsonConfigReader& reader,
                             CommandResult& result) {
    // refreshToken is accepted on the wire but not consumed: the in-game session
    // only ever needs the access token.
    const std::string outcome = SessionHelperBridge::SetSession(
        env, reader.GetString("name"), reader.GetString("uuid"), reader.GetString("token"));
    if (!IsOk(outcome)) {
        result.error = StripErrorPrefix(outcome);
        return;
    }

    const std::string writtenName = outcome.substr(3);
    FillSessionDetails(env, result);
    if (!result.ok) {
        // The write was observed but the follow-up read failed: still report the
        // swap instead of turning a success into an error.
        result.ok = true;
        result.error.clear();
        result.hasSession = true;
        result.username = writtenName;
        result.inWorld = false;
        result.ready = SessionHelperBridge::IsReady(env);
    }
}

inline void HandleRestoreSession(JNIEnv* env, CommandResult& result) {
    const std::string outcome = SessionHelperBridge::RestoreSession(env);
    if (!IsOk(outcome)) {
        result.error = StripErrorPrefix(outcome);
        return;
    }

    const std::string restoredName = outcome.substr(3);
    FillSessionDetails(env, result);
    if (!result.ok) {
        result.ok = true;
        result.error.clear();
        result.hasSession = true;
        result.username = restoredName;
        result.inWorld = false;
        result.ready = SessionHelperBridge::IsReady(env);
    }
}

inline void HandleJoin(JNIEnv* env, const lc::SimpleJsonConfigReader& reader,
                       CommandResult& result) {
    const int port = reader.GetInt("port", 25565);
    if (port < 1 || port > 65535) {
        result.error = "invalid_address";
        return;
    }

    const std::string outcome = SessionHelperBridge::JoinServer(env, reader.GetString("host"), port);
    if (!IsOk(outcome)) {
        result.error = StripErrorPrefix(outcome);
        return;
    }
    result.ok = true;
    result.address = outcome.substr(3);
    result.hasReady = true;
    result.ready = SessionHelperBridge::IsReady(env);
}

// Handle one inbound line. Returns false when the packet type is not ours.
// *reply receives the complete commandResult frame (with '\n') on true.
inline bool Handle(JNIEnv* env, const std::string& packetType,
                   const lc::SimpleJsonConfigReader& reader, std::string* reply) {
    const bool isJoin = packetType == "serverCommand";
    if (!isJoin && packetType != "accountCommand") return false;
    if (!reply) return true;

    CommandResult result;
    result.requestId = reader.GetInt("requestId", 0);
    result.action = reader.GetString("action");
    if (result.action.empty()) result.action = isJoin ? "join" : "unknown";

    if (!env || !SessionHelperBridge::IsLoaded()) {
        result.error = NotReadyMessage();
    } else if (isJoin) {
        // Only the one action this bridge implements: anything else falls through
        // to unknown_action instead of being executed as a join.
        if (result.action == "join") HandleJoin(env, reader, result);
        else result.error = "unknown_action";
    } else if (result.action == "getSession") {
        HandleGetSession(env, result);
    } else if (result.action == "setSession") {
        HandleSetSession(env, reader, result);
    } else if (result.action == "restoreSession") {
        HandleRestoreSession(env, result);
    } else {
        result.error = "unknown_action";
    }

    *reply = Serialize(result);
    return true;
}

// Load the helper once a game ClassLoader is known. mcClassNameHint is the exact
// binary name of the resolved Minecraft class (optional, improves resolution).
inline bool LoadHelper(JNIEnv* env, jobject classLoader, const char* mcClassNameHint) {
    if (SessionHelperBridge::IsLoaded()) return true;
    if (!env || !classLoader) return false;
    if (!SessionHelperBridge::Load(env, classLoader)) return false;
    if (mcClassNameHint && mcClassNameHint[0]) {
        SessionHelperBridge::SetHints(env, mcClassNameHint, nullptr);
    }
    return true;
}

// Hand the helper the connect-path class names this client actually resolves, so
// an obfuscated or modded client does not have to rely on its own guesses. Empty
// names are ignored; call after LoadHelper().
inline void HintConnectPath(JNIEnv* env, const char* connectClassName,
                            const char* serverAddressClassName,
                            const char* serverDataClassName) {
    if (!SessionHelperBridge::IsLoaded()) return;
    SessionHelperBridge::SetConnectHints(env, connectClassName,
                                         serverAddressClassName, serverDataClassName);
}

} // namespace AccountCommandRouter
