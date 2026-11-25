# Aoko host `<->` JVM bridge wire contract

Definitive reference for reimplementing the host side in Rust. Extracted from the
shipped C# host (`Aoko/`) and the two native bridges (`McInjector/src/main/cpp/`).

Two bridges exist and they are **not** the same protocol:

| Bridge | File | Game versions | Host DLL |
| --- | --- | --- | --- |
| Legacy | `bridge.cpp` | 1.8.9 | `bridge.dll` |
| Modern | `bridge_261.cpp` | 1.21.x and 26.x | `bridge_261.dll` |

`GameStateClient.ResolveBridgeDllName` picks the DLL: `26.*` or `1.21*` -> `bridge_261.dll`,
everything else -> `bridge.dll`. Everything below states which bridge a fact belongs to.
Where they differ, both variants are given.

Editorial rules used here: JSON keys are quoted verbatim including casing. Numeric
constants are quoted as literals from source. Where a detail is not derivable from the
sources it is written `unknown`.

---

## Transport & framing

### Roles and address

- The **bridge is the TCP server**. The host is the client and dials in
  (`TcpClient.ConnectAsync("127.0.0.1", 25590)`).
- Port is hard-coded at both ends: `25590`. There is no negotiation, no discovery
  handshake, no fallback port, and no retry on a different port.
- Bind address is loopback only, `htonl(INADDR_LOOPBACK)` (`127.0.0.1`). The source comment
  states the reason: the port carries the live session, including the access token
  `setSession` writes into the game, so it must not be reachable from the LAN.
- `setsockopt(serverSocket, SOL_SOCKET, SO_REUSEADDR, ...)` is applied before bind.
  `bind()` and `listen()` return values are **not** checked in either bridge; a bind
  failure leaves the socket listening on nothing and `accept()` fails in the loop.
- `listen(serverSocket, 1)` in both bridges. **Backlog = 1.** The accept loop serves one
  client at a time; there is no concurrency, no `fork`, no thread per connection.

### Framing

- One JSON document per line, terminated by a single `\n` (LF). `BridgeActions.LineTerminator = "\n"`.
- Trailing `\r` **is stripped** on both sides:
  - bridge: `BoundedNewlineBuffer::Append` drops the last byte of the buffered line when it
    is `'\r'` before completing it.
  - host: `BoundedLineReader.TakeCompletedLine` decrements the emitted length by one when the
    last buffered char is `'\r'`.
- Therefore CRLF and LF framing are both accepted, and a `\r` never reaches a JSON parser.
- There is no length prefix, no content-length, no chunking, no keepalive frame, no
  handshake, and no authentication. TLS is not used.
- Encoding is UTF-8 on the host read path (`new StreamReader(..., Encoding.UTF8,
  detectEncodingFromByteOrderMarks: true, ...)`). The bridges write raw bytes with `send()`.
  A BOM on the wire would be consumed by the host reader; neither bridge writes one.
- Write side (host) is a `NetworkStream.WriteAsync` of the whole UTF-8 byte buffer, under a
  single send lock. A write that fails mid-buffer tears the connection down
  (`FailTornWrite`) rather than leaving a partial line on the socket.

### Inbound cap (host <- bridge)

`MaximumInboundMessageCharacters = 1024 * 1024` (1,048,576 **characters** on the host side).

Overflow behaviour, which must be reproduced exactly:

1. Bytes/chars beyond the cap accumulate nowhere; the reader enters a discard state.
2. It stays discarding until the next `\n`, then resets and returns `IsTooLong`.
3. The host logs `Ignored bridge message exceeding 1048576 characters.` and **continues**.
   No error frame is sent, no disconnect, no reconnect.
4. Quirk preserved on both sides: when the buffer is exactly at the cap and the next
   character is `'\r'`, that `\r` is retained (so a max-length line terminated by CRLF still
   completes and still has its `\r` stripped). Host: `current == '\r' &&
   _lineBuilder.Length == _maximumLineCharacters`. Bridge: `current == '\r' &&
   _buffer.size() == _maximumLineBytes`.
5. Bridge cap is in **bytes**, host cap in **characters**; with ASCII-dominant payloads they
   coincide. Multi-byte UTF-8 is counted differently on the two sides.

### Outbound cap (bridge <- host), command lines

Separate, stricter cap for command lines only:

- `AccountCommandRouter::kCommandLineLimit = 64 * 1024` (65,536 bytes).
- `AccountCommandRouter::kCommandHeadProbe = 256` — only the first 256 bytes are retained so
  an over-long line can still echo its `requestId`/`action`.
- Crossing the cap while the line head still starts with `{"type":"accountCommand"` or
  `{"type":"serverCommand"` (leading spaces/tabs/`\r` skipped) queues one synthetic reply
  with `"error":"line_too_long"`, exactly once per line.
- `ParseConfig` also refuses any already-received command line whose
  `line.size() > kCommandLineLimit`.

### Bridge read buffer sizes

- Legacy: `char buf[2048]` per `recv()`.
- Modern: `char buf[4096]` per `recv()`.
- Both: `lc::BoundedNewlineBuffer readBuf(1024 * 1024)` reassembles lines across reads, and
  both feed `AccountCommandRouter::InboundCommandTracker` the **raw** chunk before the frame
  buffer sees it.
- Client socket is set non-blocking on accept: `ioctlsocket(..., FIONBIO, &mode)` with
  `mode = 1`. Both bridges.

### Write path and backpressure (bridge -> host)

`SendJsonLine` (legacy) and `SendJsonLine121` (modern) are byte-identical in behaviour:

- Returns `1` when the entire line was queued.
- On `WSAEWOULDBLOCK` it waits in `select()` with `timeval wait = { 0, 500000 }` (500 ms).
- Returns `0` when **no byte** of the line had been sent yet — the caller retries the same
  line later.
- Returns `-1` when the socket failed, or when the wait expired after a partial write. The
  caller drops the line and, for the legacy state path, breaks the connection loop
  (`publishFailed`).

`TrySendCapabilities` is the one exception: it keeps a `pendingOffset` in a function-static
and resumes the remaining bytes on the next call instead of restarting. The loop will not
publish a state line until the capabilities line is fully written, because two logical lines
interleaved on one stream poison the host's line buffer.

### Cadence constants

Host (`GameStateClient.cs`):

| Constant | Value | Meaning |
| --- | --- | --- |
| `ConfigHeartbeatMs` | `2000` | Config is re-sent at least this often even with no change |
| `ConfigChangeCoalesceMs` | `25` | Delay applied after a change before the send loop rebuilds the payload |
| `StateNotificationIntervalMs` | `25` | Minimum interval between UI `StateUpdated` notifications. **Not a wire cadence** |
| `MaximumInboundMessageCharacters` | `1024 * 1024` | Inbound line cap |
| `SendTimeout` | `TimeSpan.FromSeconds(10)` | Ceiling for one command/config line write |

Bridge (`telemetry_schedule.h`):

| Constant | Value |
| --- | --- |
| `kStateReadIntervalMs` | `25` |
| `kEntityPublishIntervalMs` | `5` |
| `kModernStateFastIntervalMs` | `5` |
| `kModernStateNormalIntervalMs` | `25` |

State publish cadence actually observed:

- **Legacy**: loop sleep = min over all due jobs, seeded with `kStateReadIntervalMs` (25 ms).
  State publishes every `kStateReadIntervalMs` (25 ms) normally, or every
  `kEntityPublishIntervalMs` (5 ms) when entity telemetry is needed — i.e. when any of
  `nametags`, `closestPlayerInfo`, `aimAssist`, `fightStatus`, `fireballWarning`, `tntTimer`
  is enabled in config.
- **Modern**: fixed `Sleep(stateIntervalMs)` per iteration — 5 ms when any of `clicking`,
  `aimAssist`, `triggerbot`, `pixelPartyAutoLook`, `pixelPartyAutoWalk`, `hudEditor` is on
  **and** the host opted into patches (`supportsStatePatches`), otherwise 25 ms. A full state
  document is still emitted at least every 25 ms (`ModernFullStateDue`).

Config send cadence (host): the loop sends when `revision != lastSentRevision` **or**
`now - lastSentAt >= 2000`. On a change it first sleeps 25 ms and drains the coalescing
semaphore, so a burst of property changes produces one config line.

### Reconnect behaviour

- On read-loop exit with `_isInjected` still true and no cancellation: schedule an auto
  reconnect.
- Reconnect thread: initial `Task.Delay(500)`, then a loop of `ConnectAsync(maxAttempts: 1,
  reportFailure: false)` followed by `Task.Delay(1000)` between attempts.
- One connect attempt = `new TcpClient()` + `ConnectAsync` with no explicit connect timeout;
  fail -> `Dispose`.
- Inside `ConnectAsync` with `maxAttempts > 1` the retry delay is `Task.Delay(500, token)`.
  Attempt budgets used by the host: `20` (default), `8` (probe an existing bridge),
  `8` (reconnect to a selected PID), `30` (after injecting a DLL).
- Guarded by `Interlocked.CompareExchange` on `_isReconnecting`, so only one reconnect loop
  runs at a time.
- There is **no** application-level heartbeat or ping on either side. Liveness is inferred
  from the state stream itself. The host never times out a silent bridge.

### Port selection behaviour (`TcpPortHelper.cs`)

`TcpPortHelper` **does not select a port**. It is a diagnostic only:

- Runs `netstat -ano -p tcp` via `ProcessStartInfo` (`RedirectStandardOutput = true`,
  `CreateNoWindow = true`, `WaitForExit(5000)`).
- Regex, per line:
  `\s(?:0\.0\.0\.0|127\.0\.0\.1|\[::\]|\[::1\]):{port}(\s|$)`.
  `0.0.0.0` and `[::]` are matched because a wildcard listener also owns the port.
- Requires the line to contain `LISTENING` (case-insensitive).
- Splits on whitespace; requires `parts.Length >= 5` and parses `parts[^1]` as the PID,
  requiring `pid > 0`. Returns `null` on any failure or exception.
- Used for three decisions only: "is the selected PID's bridge already listening, so just
  reconnect", "is some *other* process holding the port, so refuse", and "did we end up
  connected to a PID other than the selected one, so disconnect".
- If the port is free the host proceeds to inject the DLL and then polls `ConnectAsync`
  with up to 30 attempts at 500 ms.

Consequence for the Rust host: the bind/connect target is the constant `127.0.0.1:25590`;
the netstat probe is a UX/diagnostic feature, not part of the wire contract.

---

## Message envelopes

Eight `type` values exist across the protocol. One additional framing form exists: the
legacy `state` line carries **no** `type` property at all.

| `type` | Direction | Trigger | Cadence |
| --- | --- | --- | --- |
| `config` | host -> bridge | Connection established; any tracked property change; heartbeat | On change (coalesced 25 ms) and at least every 2000 ms |
| `moduleAction` | host -> bridge | User presses the module keybind | On demand only |
| `accountCommand` | host -> bridge | Session switch / query / restore request | On demand only |
| `serverCommand` | host -> bridge | Server join request | On demand only |
| `capabilities` | bridge -> host | Every accepted connection | Exactly once per connection, before any state line |
| `state` | bridge -> host | Telemetry loop | Legacy 25 ms (5 ms with entity telemetry); modern 25 ms (5 ms when latency-sensitive) |
| `statePatch` | (host accepts) | Never emitted by either shipped bridge | Never |
| `commandResult` | bridge -> host | One accepted `accountCommand`/`serverCommand`, or one over-long command line | One reply per request, FIFO |

### `config` (host -> bridge)

Serialised by `System.Text.Json` from an anonymous object, `WriteIndented = false`, default
options (no camelCase policy — the keys are written literally as the property names in the
source, which are already camelCase). Terminated by `"\n"`. It is **one physical line**; the
block below is wrapped for readability only.

```json
{"type":"config","perfDiagnostics":false,"armed":true,"clicking":false,"minCPS":8,"maxCPS":12,
 "left":true,"right":false,"rightMinCPS":10,"rightMaxCPS":14,"rightBlock":false,
 "breakBlocks":false,"jitter":true,"randomizationMode":1,"clickInChests":false,
 "aimAssist":false,"aimAssistFov":30,"aimAssistRange":4.5,"aimAssistStrength":40,
 "triggerbot":false,"killAura":false,"killAuraCpsMode":"normal","killAuraMode":"switch",
 "killAuraSort":"health","killAuraAutoBlock":"hypixel","killAuraAttackTick":0,
 "killAuraAutoBlockRange":6,"killAuraSwingRange":3.5,"killAuraAttackRange":3,
 "killAuraFov":360,"killAuraMinCps":14,"killAuraMaxCps":14,"killAuraSwitchDelay":150,
 "killAuraRotations":"silent","killAuraDeadZone":0.5,"killAuraMaxTurnSpeed":25,
 "killAuraMinTurnSpeed":5,"speedBridge":false,"gtbHelper":false,"gtbHint":"","gtbCount":0,
 "gtbPreview":"","nametags":false,"nickHiderEnabled":false,"nickHiderAlias":"",
 "showModuleList":true,"moduleListStyle":0,"showLogo":true,"guiTheme":"Slate",
 "supportsStatePatches":true,"reloadMappingsNonce":0,"hudEditor":false,
 "hudLayout":{"modulelist":{"x":0.985,"y":0.02,"scale":1},"closestplayer":{"x":0.5,
 "y":0.78,"scale":1},"pixelparty":{"x":0.5,"y":0.66,"scale":1},"chestesplist":
 {"x":0.015,"y":0.3,"scale":1},"gtbhint":{"x":0.5,"y":0.62,"scale":1},"nametags":
 {"x":0,"y":0,"scale":1},"blockesplist":{"x":0.015,"y":0.56,"scale":1}}}
```

Abbreviated (ellipses omitted mid-object) — the full key set is 216 scalar keys plus `type`
plus the nested `hudLayout`, enumerated in `## Outbound config fields`.

Optionality: every scalar key is always present. `hudLayout` is always present (the host
serialises all seven element ids). No field is omitted conditionally.

Bridge tolerance: both `ParseConfig` implementations read keys by substring search, so key
order is irrelevant, unknown keys are ignored, and missing keys fall back to the value
already held in the bridge's config struct (never to zero, except where the source passes an
explicit default).

### `moduleAction` (host -> bridge)

Two actions exist. Shapes as built by `GameStateClient`:

```json
{"type":"moduleAction","action":"autoRod","phase":"press","enabled":true,"requestId":7,
 "slotMode":3,"verifyForcedSlot":true,"extensionTicks":8,"holdToExtend":false}
```
```json
{"type":"moduleAction","action":"autoRod","phase":"release"}
```
```json
{"type":"moduleAction","action":"throwPot","phase":"press","enabled":true,"requestId":9}
```

- `requestId` comes from `Interlocked.Increment(ref _nextActionRequestId)` — a **separate**
  counter from the command `requestId` source. Ids start at 1.
- `slotMode` clamped host-side to `[0, 9]`; `extensionTicks` clamped host-side to `[1, 40]`.
- Bridge acknowledgement is not a `commandResult`. It is echoed in the next `state` line as
  `"lastActionId"` / `"lastActionResult"`. Results written by the bridges: `"queued"`,
  `"cancelled"`, `"rejected"`; the host also recognises `"received"`, `"started"`.
  `SetLastActionResult` ignores `requestId <= 0` and ignores an id lower than the last
  recorded one.
- `"phase":"release"` carries **no** `requestId`; it is not acknowledged.
- Field optionality: `phase` is always present; `enabled` is absent on the release frame;
  `slotMode`/`verifyForcedSlot`/`extensionTicks`/`holdToExtend` are absent on the release
  frame and absent for `throwPot`; `requestId` is optional in the bridge's reader
  (`GetInt("requestId", 0)`), and `requestId == 0` disables acknowledgement.

### `accountCommand` (host -> bridge)

```json
{"type":"accountCommand","action":"getSession","requestId":123}
```
```json
{"type":"accountCommand","action":"setSession","requestId":124,"name":"Notch",
 "uuid":"<uuid>","token":"<msa token>","refreshToken":"<optional>"}
```
```json
{"type":"accountCommand","action":"restoreSession","requestId":125}
```

Optionality: `name`, `uuid`, `token` are written only for `setSession` (empty string
substituted when null). `refreshToken` is written only when non-empty, and the bridge accepts
it but does not consume it. `requestId` is mandatory for correlation on the host side.

### `serverCommand` (host -> bridge)

```json
{"type":"serverCommand","action":"join","requestId":126,"host":"mc.example.com","port":25565}
```

Optionality: all four fields are always written. `host` is trimmed host-side and must be
non-empty; `port` must be in `[1, 65535]` or the host refuses locally with
`invalid_address` and never touches the socket.

### `capabilities` (bridge -> host)

Single line, sent first, before any state. Payload detail in `## Capability payload`.

```json
{"type":"capabilities","modules":["autoclicker","autorod","..."],"settings":["autorodenabled",
 "..."],"state":["inworld","actionbar","..."],
 "actions":["account.getSession","account.setSession","account.restoreSession","server.join"]}
```

Optionality: all four arrays are optional individually — a missing or empty array falls back
to the version table (`BridgeCapabilities.FromPayload`). `actions` is the newest array; older
payloads without it inherit the fallback set.

### `state` (bridge -> host)

Two structurally different documents share the name.

Modern (`bridge_261.cpp`) — starts with a type and may be flagged partial:

```json
{"type":"state","partial":true,"inWorld":true,"guiOpen":false,"holdingBlock":false,
 "lookingAtBlock":false,"lookingAtEntity":false,"lookingAtEntityLatched":false,
 "lookingAtEntityHurtTime":0,"breakingBlock":false,"stateMs":123456,
 "attackCooldown":1.000,"attackCooldownPerTick":0.080,"killAuraHasTarget":false,
 "killAuraBlocking":false,"lastActionId":0,"lastActionResult":"",
 "pixelPartyTargetFound":false,"pixelPartyTargetYaw":0.00,"pixelPartyTargetDist":-1.00,
 "pixelPartyYawDelta":0.00}
```

Legacy (`bridge.cpp`) — **no `type` property at all**, always a full document:

```json
{"mapped":true,"inWorld":true,"guiOpen":false,"screenName":"GuiIngameMenu",
 "actionBar":"","health":20.000000,"posX":0.500000,"posY":64.000000,"posZ":-12.300000,
 "pitch":12.340000,"holdingBlock":false,"lookingAtBlock":false,"lookingAtEntity":false,
 "lookingAtEntityLatched":false,"lookingAtEntityHurtTime":0,"moduleStatus":{},
 "breakingBlock":false,"attackCooldown":1.000000,"attackCooldownPerTick":0.080000,
 "killAuraUnavailableReason":"","killAuraHasTarget":false,"killAuraBlocking":false,
 "lastActionId":0,"lastActionResult":"","stateMs":123456,"chestStealerState":null,
 "refillState":null,"pixelPartyTargetFound":false,"pixelPartyTargetYaw":0.000000,
 "pixelPartyTargetDist":-1.000000,"pixelPartyYawDelta":0.000000,"entities":[]}
```

Optionality differs per bridge and per field, and is **not** free choice — see
`## Inbound state fields` for the exact conditional rules. Summary:

- Legacy: `hudLayout` is omitted unless the HUD editor dirtied the layout.
  `chestStealerState`/`refillState` are always present (`null` when absent). Every other key
  is unconditional.
- Modern: `screenName`, `actionBar`, `health`, `posX`, `posY`, `posZ`, `pitch`, `fov`,
  `viewportWidth`, `viewportHeight`, `moduleStatus`, `killAuraUnavailableReason` are emitted
  only when the frame is a **full** state. `chestStealerState`/`refillState` are emitted when
  the frame is full **or** the payload is non-null. `entities` is emitted when the frame is
  full **or** `aimAssist`/`triggerbot` is enabled. `hudLayout` only on full frames when
  dirtied. `partial` is present only on fast frames.

The host does not require `type` for a state line: `BridgeInboundClassifier` returns
`State` for any object that is not `commandResult` or `capabilities`, so the legacy untyped
document deserialises into `GameState` unchanged.

### `statePatch` (host-accepted, never emitted)

`GameStatePatchMerger.IsPartial` treats a line as a partial update when
`type == "statePatch"` **or** `"partial":true` is present. Neither shipped bridge writes
`type":"statePatch"`; `bridge_261.cpp` writes `{"type":"state","partial":true,...}`. Honour
`statePatch` for forward compatibility but do not emit it.

Partial-merge semantics that must be reproduced:

- Fields present are applied; fields absent keep the previous value.
- `type` and `partial` are skipped.
- A value that fails to parse is ignored (the previous value survives), never zeroed.
- `null` for `moduleStatus`/`entities` resets to empty; `null` for
  `chestStealerState`/`refillState` resets to `null`.
- **World-exit scrub**: if the patch carries `"inWorld": false`, then after applying,
  `entities` is emptied and `chestStealerState`/`refillState` are set to `null`, even when
  the patch omitted them. This prevents stale combat targets surviving a world change.

### `commandResult` (bridge -> host)

One line per request, written by the same socket thread that reads config:

```json
{"type":"commandResult","requestId":124,"ok":true,"action":"setSession",
 "username":"Notch","uuid":"<uuid>","inWorld":false,"ready":true}
```
```json
{"type":"commandResult","requestId":126,"ok":true,"action":"join",
 "address":"mc.example.com:25565","ready":true}
```
```json
{"type":"commandResult","requestId":127,"ok":false,"action":"getSession",
 "error":"in_world","inWorld":true}
```
```json
{"type":"commandResult","requestId":0,"ok":false,"action":"setSession",
 "error":"line_too_long"}
```

Field emission is conditional in `AccountCommandRouter::Serialize` — reproduce the ordering
and conditions exactly:

1. `"type":"commandResult"`, `"requestId"`, `"ok"`, `"action"` — always, in that order.
2. If `hasSession`: `"username"`, `"uuid"`, `"inWorld"`, `"ready"` — always all four together.
3. Else if `address` is non-empty: `"address"`, and `"ready"` only if `hasReady`.
4. If `!ok`: `"error"`, plus a second `"inWorld":true` when `error == "in_world"`.

Consequence: `inWorld` and `ready` can appear **twice** in one document (once from the
session block, once from the `in_world` failure tail). Last-wins parsers are required. The
host reads by property name, so duplicates are harmless to it.

Unknown/invalid replies (wrong type, non-object, `requestId` missing or `<= 0`) are dropped
by the host without touching state.

---

## Capability payload

The payload is four **string arrays**. They are string lists of lowercase identifiers, never
bitflags, never enum numerics, never a bitset. Both bridges build them as compile-time string
literals in `bridge_capabilities.h` returning a `const char*`; the host parses them into
`HashSet<string>` with `StringComparer.OrdinalIgnoreCase` after `Trim().ToLowerInvariant()`.

Consequence: matching is case-insensitive and whitespace-tolerant on the host; the bridge
always emits lowercase, so byte-for-byte output must be lowercase.

### Field semantics

| Key | Meaning | Empty-handling in `BridgeCapabilities.FromPayload` |
| --- | --- | --- |
| `modules` | Module ids the bridge can drive | Empty -> fallback table's modules |
| `settings` | Config key names the bridge reads | Empty -> fallback table's settings |
| `state` | State field names the bridge sends | Empty -> fallback table's state |
| `actions` | Session/server command names | Empty -> fallback `actions` |

If **all four** are empty the whole payload is rejected and the previous capability set is
kept. If some are present, present ones win and only the empty ones fall back.

`settings` is documented as "config key names the bridge reads", and three advertised
entries break that rule. Do not assume a capability string is a working config key:

| Advertised `settings` entry | What the bridges actually parse | Consequence |
| --- | --- | --- |
| `chesteprange` | `chestEspRange` (`bridge.cpp`, `bridge_261.cpp`) | `chesteprange` is neither the key nor `ToLowerInvariant` of it (`chestesprange` would be). |
| `nametagsrange` | `nametagRange` (`bridge.cpp`, `bridge_261.cpp`) | Same mismatch; a `supports_setting("nametagRange")` gate reads as unsupported. |
| `killaurarotmode` | Not parsed by either bridge | Advertised but inert. |

These are transcribed exactly as the headers publish them. When gating a write on a
capability, compare against the advertised string, and when actually sending config use the
key the bridge parses. The two sets are not identical.

### `actions` (identical in both bridges, 4 entries)

```
account.getSession,account.setSession,account.restoreSession,server.join
```

### `modules` — legacy, 39 entries

```
autoclicker,autorod,autotool,rightclick,jitter,clickinchests,breakblocks,aimassist,killaura,
speedbridge,gtbhelper,nametags,nickhider,closestplayer,fightstatus,chestesp,cheststealer,
refill,throwpot,autoheal,blockesp,bedplates,reach,velocity,antidebuff,hitdelayfix,
fireballwarning,fastplace,nohurtcam,sprint,tnttimer,wtap,jumpreset,hitselect,trajectories,
murdermystery,breakprogress,pixelpartyassist,hudeditor
```

### `modules` — modern, 41 entries

Same list with `triggerbot` inserted after `aimassist` and `autototem` after `velocity`:

```
autoclicker,autorod,autotool,rightclick,jitter,clickinchests,breakblocks,aimassist,triggerbot,
killaura,speedbridge,gtbhelper,nametags,nickhider,closestplayer,fightstatus,chestesp,
cheststealer,refill,throwpot,autoheal,blockesp,bedplates,reach,velocity,autototem,antidebuff,
hitdelayfix,fireballwarning,fastplace,nohurtcam,sprint,tnttimer,wtap,jumpreset,hitselect,
trajectories,murdermystery,breakprogress,pixelpartyassist,hudeditor
```

Only difference: `+triggerbot`, `+autototem`.

### `state` — legacy, 25 entries

```
inworld,actionbar,holdingblock,lookingatblock,lookingatentity,lookingatentitylatched,
breakingblock,lookingatentityhurttime,modulestatus,attackcooldown,attackcooldownpertick,
killauraunavailablereason,killaurahastarget,killaurablocking,lastactionid,lastactionresult,
statems,cheststealerstate,refillstate,pitch,pixelpartytargetfound,pixelpartytargetyaw,
pixelpartytargetdist,pixelpartyyawdelta,hudlayout
```

### `state` — modern, 32 entries

Legacy list plus `fov`, `viewportwidth`, `viewportheight` (after `lookingatblock`) and
`health`, `posx`, `posy`, `posz` (after `pitch`). Literal array, in order:

```
inworld,actionbar,holdingblock,lookingatblock,fov,viewportwidth,viewportheight,
lookingatentity,lookingatentitylatched,breakingblock,lookingatentityhurttime,modulestatus,
attackcooldown,attackcooldownpertick,killauraunavailablereason,killaurahastarget,
killaurablocking,lastactionid,lastactionresult,statems,cheststealerstate,refillstate,pitch,
health,posx,posy,posz,pixelpartytargetfound,pixelpartytargetyaw,pixelpartytargetdist,
pixelpartyyawdelta,hudlayout
```

### `settings` — legacy, 194 entries

```
autorodenabled,autorodslotmode,autorodverifyforcedslot,autorodextensionticks,
autorodholdtoextend,keybindautorod,autotoolenabled,autotoolswapweapon,autotoolinstantswap,
autotoolswaptodelay,autotoolswapback,autotoolswapbackdelay,autotoolrequiremousedown,
autotoolonlysneaking,autotoolbedwarsmode,keybindautotool,mincps,maxcps,left,right,rightmincps,
rightmaxcps,rightblock,breakblocks,jitter,clickinchests,aimassistfov,aimassistrange,
aimassiststrength,<LC_KILLAURA_SETTINGS>,killaurarotmode,speedbridge,
speedbridgeblockonly,speedbridgedelayms,speedbridgeholdingshiftonly,speedbridgelookingdownonly,
nametags,nickhiderenabled,nickhideralias,closestplayerinfo,fightstatus,keybindfightstatus,
nametagshowhealth,nametagshowarmor,nametagshowhelditem,nametaghidevanilla,nametagsrange,
chestesp,chesteprange,cheststealerenabled,cheststealerdelayms,cheststealermenucheck,
cheststealertitlecheck,cheststealercustomitemscheck,cheststealerphysicalcheck,refillenabled,
refilldelayms,throwpotenabled,keybindthrowpot,autohealenabled,autohealhealth,autohealdelayms,
blockespenabled,blockespboxes,blockesptracers,blockesphud,blockespmaxcount,blockesprange,
blockespblocks,keybindblockesp,bedplatesenabled,bedplatesshowdistance,bedplatesrange,
keybindbedplates,reachenabled,reachmin,reachmax,reachchance,reachchancemode,
reachonlywhilesprinting,reachdisableinwater,reachverticalcheck,velocityenabled,
velocityhorizontal,velocityvertical,velocitychance,antidebuffenabled,hitdelayfixenabled,
fireballwarning,fireballwarningbox,fireballwarningarrow,fireballwarningsound,
keybindfireballwarning,fastplace,keybindfastplace,nohurtcam,keybindnohurtcam,sprint,
keybindsprint,tnttimer,keybindtnttimer,wtap,keybindwtap,jumpreset,keybindjumpreset,hitselect,
keybindhitselect,trajectories,keybindtrajectories,murdermystery,keybindmurdermystery,
breakprogress,keybindbreakprogress,jumpresetchance,gtbhint,gtbcount,gtbpreview,showmodulelist,
moduleliststyle,showlogo,guitheme,keybindautoclicker,keybindspeedbridge,keybindnametags,
keybindclosestplayer,keybindchestesp,keybindcheststealer,keybindrefill,pixelpartyassist,
pixelpartyscanradius,pixelpartyautolook,pixelpartyautowalk,hudeditor
```

`LC_KILLAURA_SETTINGS` expands in place (62 entries, from the `#define`), so the literal
array contains:

```
killaura,killauracpsmode,killauramode,killaurasort,killauraautoblock,killauraattacktick,
killauraautoblockrequirepress,killauraautoblockcps,killauraautoblockrange,killauraswingrange,
killauraattackrange,killaurafov,killauramincps,killauramaxcps,killauraswitchdelay,
killaurarotations,killauradeadzone,killauramaxturnspeed,killauraminturnspeed,
killauraacceleration,killauradeceleration,killaurauseovershoot,killauraovershootstrength,
killauraovershootrecovery,killauranoisestrength,killauravisualizeaim,killaurasmoothback,
killauramovefix,killaurasmoothing,killauraravensmoothing,killauraravenpredictticks,
killauraravenyawrandom,killauraanglestep,killaurathroughwalls,killaurarequirepress,
killauraallowmining,killauraweaponsonly,killauraallowtools,killaurainventorycheck,
killaurabotcheck,killauraplayers,killaurabosses,killauramobs,killauraanimals,killauragolems,
killaurasilverfish,killaurateams,killaurashowtarget,killauradebug,killaurarandomize,
killaurarandomizerange,killaurayrandomize,killauralbhorizontalspeed,killauralbverticalspeed,
killauralbsmooth,killauralbpredict,killauralbpredictsize,killauralbrandomize,
killauralbrandomrange,killauralbhorizontalsearch,killauralbbodymin,killauralbbodymax
```

The legacy array does **not** contain `triggerbot`, `autototem*`, `reloadmappingsnonce`, or
`keybindtriggerbot`.

### `settings` — modern, 200 entries

Differences from legacy:

- Removed: `aimassistfov`, `aimassistrange`, `aimassiststrength`.
- Added after `clickinchests`: `triggerbot`.
- Added after `velocitychance`: `reloadmappingsnonce`.
- Added after `keybindautoclicker`: `keybindtriggerbot`.
- Added after `keybindrefill`: `autototemenabled`, `autototemmode`, `autototemhealth`,
  `autototemelytra`, `autototembehaviormode`, `autototemdelay`.
- `gtbhint`, `gtbcount`, `gtbpreview` move from the tail (after `jumpresetchance`) to
  immediately after `speedbridgelookingdownonly`.
- The 11-entry block `showmodulelist`, `moduleliststyle`, `showlogo`, `guitheme`,
  `keybindautoclicker`, `keybindspeedbridge`, `keybindnametags`, `keybindclosestplayer`,
  `keybindchestesp`, `keybindcheststealer`, `keybindrefill` moves. In legacy it sits after
  `gtbpreview` and before `pixelpartyassist`; in modern it sits after `reloadmappingsnonce`
  and before `autototemenabled`. (In modern, `keybindtriggerbot` is inserted inside the
  moved block, directly after `keybindautoclicker`.)

Delta arithmetic: 194 - 3 + 1 + 1 + 1 + 6 = 200. Confirmed by count.

The bullet list above is a description, not a reconstruction recipe: the block move changes
order without changing membership, so applying only the add/remove bullets yields 200 entries
in the wrong order from that point on. For a byte-exact table, transcribe the modern array
from `bridge_capabilities.h` directly rather than deriving it.

### Host fallback tables (`BridgeCapabilities.ForVersionFallback`) and where they diverge

The host synthesises a capability set from the injected version before the `capabilities`
frame arrives (and again after every disconnect). Selection:

- `injectedVersion` starts with `1.21` (case-insensitive) **or** starts with `26.` -> modern
  table.
- Otherwise -> legacy table.

Divergences from `bridge_capabilities.h` — these are real and must be reproduced if the Rust
host is to behave identically:

| Array | Header modern | C# modern fallback | Header legacy | C# legacy fallback |
| --- | --- | --- | --- | --- |
| `modules` | 41 | 41 (same set) | 39 | 39 (same set) |
| `settings` | 200 | **155** | 194 | **143** |
| `state` | 32 | **27** (missing `pitch`, `health`, `posx`, `posy`, `posz`) | 25 | **29** (contains `pitch`, `health`, `posx`, `posy`, `posz`) |
| `actions` | 4 | 4 | 4 | 4 |

The C# fallback `settings` sets are stale: they advertise a mix of older names
(`aimassistfov`, `aimassistrange`, `aimassiststrength`, `killaurarange`, `killauraaimrange`,
`killauratargetmode`, `killauraswitchdelayms`, `killaurarandomization`, `killauraclickmode`)
and lack `gtbhint`, `reloadmappingsnonce`, `autototem*`, `keybindtriggerbot`, and several
keybinds. The legacy C# fallback also **inverts** one header decision: it advertises
`health`, `posx`, `posy`, `posz` (which the legacy bridge really does send) while the
header's legacy array does not. If the Rust host mirrors the *header* instead of the C#
tables, a user on a bridge that never sends `capabilities` sees different setting
availability. Match the C# tables to preserve behaviour.

`SupportsAction` gates the account/server UI. Because both shipped bridges advertise all four
actions, the fallback always includes all four (source comment: leaving `actions` out "told
the user to rebuild the DLLs for a feature the bridge supports").

---

## Inbound state fields

Keys are **lowercase identifiers in the capability array** (`attackcooldownpertick`) but
**camelCase JSON keys on the wire** (`attackCooldownPerTick`). The host compares capability
names case-insensitively after lowercasing, so the mismatch is benign there; a Rust
implementation must keep the two spellings distinct mapping targets.

- Wire keys: camelCase (`inWorld`, `screenName`, `posX`, `attackCooldownPerTick`).
- Capability entries: all-lowercase (`inworld`, `attackcooldownpertick`, `pixelpartyyawdelta`).
  The transform is a plain `ToLowerInvariant` on the wire key, so `attackCooldownPerTick`
  corresponds to `attackcooldownpertick`. Note that some wire keys have no advertised
  counterpart at all: `screenName`, `guiOpen`, `entities`.

Union across both bridges: **36** top-level state fields. 32 shared, 1 legacy-only
(`mapped`), 3 modern-only (`fov`, `viewportWidth`, `viewportHeight`).

`legacy` = emitted by `bridge.cpp`. `modern` = emitted by `bridge_261.cpp`. "cap" = appears
in that bridge's `state` capability array.

| Wire key | Type | Units / domain | Legacy | Modern | Cap legacy | Cap modern |
| --- | --- | --- | --- | --- | --- | --- |
| `mapped` | bool | Class mapping resolved | yes | no | no | no |
| `inWorld` | bool | A world is loaded | yes | yes | `inworld` | `inworld` |
| `guiOpen` | bool | Any screen open (modern excludes `ChatScreen`) | yes | yes | no | no |
| `screenName` | string | Current screen class/simple name | yes | full only | no | no |
| `actionBar` | string | Action-bar text, unescaped content | yes | full only | `actionbar` | `actionbar` |
| `health` | float | HP, 1 unit = half heart. Legacy 6 dp, modern 1 dp | yes | full only | no | `health` |
| `posX` | double | World block X. Legacy 6 dp, modern 3 dp | yes | full only | no | `posx` |
| `posY` | double | World block Y | yes | full only | no | `posy` |
| `posZ` | double | World block Z | yes | full only | no | `posz` |
| `pitch` | float | Camera pitch, degrees. Legacy 6 dp, modern 2 dp | yes | full only | `pitch` | `pitch` |
| `fov` | float | Vertical FOV, degrees, modern 2 dp | no | full only | no | `fov` |
| `viewportWidth` | int | Client window width in pixels | no | full only | no | `viewportwidth` |
| `viewportHeight` | int | Client window height in pixels | no | full only | no | `viewportheight` |
| `holdingBlock` | bool | Held item is a block | yes | yes | `holdingblock` | `holdingblock` |
| `lookingAtBlock` | bool | Crosshair over a block | yes | yes | `lookingatblock` | `lookingatblock` |
| `lookingAtEntity` | bool | Crosshair over a living entity | yes | yes | `lookingatentity` | `lookingatentity` |
| `lookingAtEntityLatched` | bool | Sticky variant of the above | yes | yes | `lookingatentitylatched` | `lookingatentitylatched` |
| `lookingAtEntityHurtTime` | int | `hurtTime` ticks of the target, 0 = none | yes | yes | `lookingatentityhurttime` | `lookingatentityhurttime` |
| `moduleStatus` | object | `{moduleId: reason}`, absent id = healthy | yes | full only | `modulestatus` | `modulestatus` |
| `breakingBlock` | bool | Currently breaking | yes | yes | `breakingblock` | `breakingblock` |
| `attackCooldown` | float | Progress toward next hit. Legacy 6 dp, modern 3 dp | yes | yes | `attackcooldown` | `attackcooldown` |
| `attackCooldownPerTick` | float | Per-tick increment | yes | yes | `attackcooldownpertick` | `attackcooldownpertick` |
| `killAuraUnavailableReason` | string | Why kill aura is idle; `""` when fine | yes | full only | `killauraunavailablereason` | `killauraunavailablereason` |
| `killAuraHasTarget` | bool | Kill aura holds a target | yes | yes | `killaurahastarget` | `killaurahastarget` |
| `killAuraBlocking` | bool | Kill aura is autoblocking | yes | yes | `killaurablocking` | `killaurablocking` |
| `lastActionId` | int | Last accepted `moduleAction.requestId`, 0 = none | yes | yes | `lastactionid` | `lastactionid` |
| `lastActionResult` | string | `queued`/`cancelled`/`rejected`/`""` | yes | yes | `lastactionresult` | `lastactionresult` |
| `stateMs` | u64 | Bridge monotonic ms since boot (`GetTickCount64`, cast to `unsigned long long`) | yes | yes | `statems` | `statems` |
| `chestStealerState` | object\|null | See below | yes | full or non-null | `cheststealerstate` | `cheststealerstate` |
| `refillState` | object\|null | See below | yes | full or non-null | `refillstate` | `refillstate` |
| `pixelPartyTargetFound` | bool | Target block located | yes | yes | `pixelpartytargetfound` | `pixelpartytargetfound` |
| `pixelPartyTargetYaw` | float | Degrees. Legacy 6 dp, modern 2 dp | yes | yes | `pixelpartytargetyaw` | `pixelpartytargetyaw` |
| `pixelPartyTargetDist` | float | Blocks, `-1` when no target | yes | yes | `pixelpartytargetdist` | `pixelpartytargetdist` |
| `pixelPartyYawDelta` | float | Normalised to `(-180, 180]`; 0 when no target | yes | yes | `pixelpartyyawdelta` | `pixelpartyyawdelta` |
| `entities` | array | See below | yes | full or aim/trigger | no | no |
| `hudLayout` | object | Only when dirtied. See `## HUD layout sync` | yes | full only | `hudlayout` | `hudlayout` |

Advertised-but-sent mismatch, both directions:

- Legacy advertises `pitch` but not `health`/`posx`/`posy`/`posz`, yet sends all five.
- Neither bridge advertises `guiOpen`, `screenName`, or `entities`, yet all three are sent
  (modern sends `entities` conditionally).

### Nested objects

`moduleStatus` — flat string map, keys are lowercase module ids, values are short human
strings. Built by `lc::ModuleStatusMap::ToJson`: `{"fastplace":"Missing rightClickDelay
mapping"}`. Empty map serialises as `{}`. Only `"` and `\` are escaped; control characters
below `0x20` are replaced with a space. Entries are written in `std::map` order, i.e. sorted
by key.

`chestStealerState`:

```json
{"ready":true,"physical":true,"windowId":42,"screenWidth":1920,"screenHeight":1080,
 "slots":[{"index":0,"slotNumber":53,"x":512,"y":340}]}
```

- `ready` — bool, at least one slot was emitted.
- `physical` — bool, a physical chest is nearby (blocked container reports `false`).
- `windowId` — int, container sync id.
- `screenWidth`, `screenHeight` — int, GUI space dimensions.
- `slots[]` — `index` (loop index), `slotNumber` (container slot id), `x`/`y` (absolute GUI
  pixel centre = `guiLeft + slotX + 8`, `guiTop + slotY + 8`).
- A blocked container emits `{"ready":false,"physical":false,"windowId":N,"screenWidth":W,
  "screenHeight":H,"slots":[]}` — `slots` present but empty.

`refillState` — same schema **without** `physical`:

```json
{"ready":true,"windowId":0,"screenWidth":1920,"screenHeight":1080,
 "slots":[{"index":0,"slotNumber":36,"x":512,"y":340}]}
```

`entities[]` — identical key set in both bridges, different caps and precision:

```json
[{"sx":640.500000,"sy":360.250000,"dist":12.480000,"name":"Steve","hp":20.000000}]
```

- `sx`, `sy` — projected screen pixels; `-1` when not projected.
- `dist` — blocks from the camera.
- `name` — display name, JSON-escaped.
- `hp` — health, clamped.
- Legacy cap `kEntityJsonCap = 20`; modern cap `32`.
- Legacy `entities` is `[]` unless entity telemetry was requested in config; modern emits it
  only when the frame is full or `aimAssist`/`triggerbot` is enabled.

### Exact emission order (required for byte-for-byte parity)

**Legacy** (`bridge.cpp`, one string append per field, floats via `std::to_string`, i.e. 6
decimal places, `double` likewise):

```
{"mapped":B,"inWorld":B,"guiOpen":B,"screenName":"s","actionBar":"s","health":F,
"posX":F,"posY":F,"posZ":F,"pitch":F,"holdingBlock":B,"lookingAtBlock":B,
"lookingAtEntity":B,"lookingAtEntityLatched":B,"lookingAtEntityHurtTime":I,
"moduleStatus":O,"breakingBlock":B,"attackCooldown":F,"attackCooldownPerTick":F,
"killAuraUnavailableReason":"s","killAuraHasTarget":B,"killAuraBlocking":B,
"lastActionId":I,"lastActionResult":"s","stateMs":I,"chestStealerState":O|null,
"refillState":O|null,"pixelPartyTargetFound":B,"pixelPartyTargetYaw":F,
"pixelPartyTargetDist":F,"pixelPartyYawDelta":F,"entities":[E],
[hudLayout O,]}\n
```

`hudLayout` is appended as `,"hudLayout":{...}` only when `g_hudEditor.layoutDirty`.

**Modern** (`bridge_261.cpp`):

```
{"type":"state"[,"partial":true],"
inWorld":B,"guiOpen":B
[,"screenName":"s","actionBar":"s","health":H1,"posX":H3,"posY":H3,"posZ":H3,"pitch":H2,
 "fov":H2,"viewportWidth":I,"viewportHeight":I]
,"holdingBlock":B,"lookingAtBlock":B,"lookingAtEntity":B,"lookingAtEntityLatched":B,
"lookingAtEntityHurtTime":I[,"moduleStatus":O],"breakingBlock":B,"stateMs":U,
"attackCooldown":H3,"attackCooldownPerTick":H3[,"killAuraUnavailableReason":"s"],
"killAuraHasTarget":B,"killAuraBlocking":B,"lastActionId":I,"lastActionResult":"s"
[,"chestStealerState":O|null][,"refillState":O|null],"pixelPartyTargetFound":B,
"pixelPartyTargetYaw":H2,"pixelPartyTargetDist":H2,"pixelPartyYawDelta":H2
[,"entities":[E]][,"hudLayout":O]}\n
```

`H1` = `%.1f`, `H2` = `%.2f`, `H3` = `%.3f`, `I` = integer, `U` = `%llu`.

Numeric formatting differs between the bridges and is part of the byte contract: legacy
`"health":20.000000` vs modern `"health":20.0`; legacy `"posX":0.500000` vs modern
`"posX":0.500`; legacy `"attackCooldown":1.000000` vs modern `"attackCooldown":1.000`. The
modern bridge writes `attackCooldownPerTick` in the same block as `attackCooldown` but
`stateMs` *before* them, and `killAuraUnavailableReason` *after* them.

### String escaping differences

- Legacy `JsonEscape` escapes `\`, `"`, `\n`, `\r`, `\t` and passes every other byte through,
  including control bytes `0x00`-`0x1F` (which produces invalid JSON if such a byte appears in
  `screenName`/`actionBar`/entity names).
- Modern escapes only `"` and `\` for `screenName`, `actionBar`, `killAuraUnavailableReason`
  and entity names (inline loops, not a shared helper).
- Modern entity names are pre-escaped into `p.jsonEscapedName` off the socket thread.
- `lastActionResult` is never escaped in either bridge; its values are internal constants.
- `moduleStatus` uses the third escaper from `module_status.h` described above.

---

## Outbound config fields

216 scalar keys plus `type` and the nested `hudLayout`, all at the top level of one object.

Parsing model that must be reproduced (this is the contract, not an implementation hint):

- `lc::SimpleJsonConfigReader` does **not** tokenise JSON. It searches the raw line for
  `"<key>":` and reads from there.
- `GetString`:
  - Marker not found -> `""`.
  - If the byte right after the colon is `"` -> returns bytes up to the **next** `"`. No
    unescaping, so a `\"` inside a value terminates it early.
  - Otherwise returns bytes up to the next `,` or `}` with leading spaces trimmed.
- `GetBool` is exact string comparison against `true` / `false`; anything else -> the supplied
  default. So `true ` (trailing space) is `false`.
- `GetFloat` / `GetInt` use `strtof` / `strtol`, then require the remainder to be
  all-whitespace, `errno != ERANGE`, finite, and (for `GetInt`) within `int` range; otherwise
  the supplied default.
- The first occurrence of `"<key>":` anywhere in the line wins — including an occurrence
  inside a string value. Key order is irrelevant; unknown keys are ignored; missing keys give
  the caller's default.
- A value containing `"` cannot be transmitted verbatim for any string key (`nickHiderAlias`,
  `gtbHint`, `gtbPreview`, `guiTheme`, `blockEspBlocks`).

Notation below: `bool` = `GetBool`, `int` = `GetInt`, `float` = `GetFloat`,
`string` = `GetString`. The clamp column gives the effective accepted domain. `keep` means an
out-of-range value leaves the previously stored value unchanged (not clamped, not defaulted).
`prev` in a default column means the current value of the bridge's config field is used as the
default, so an absent key is a no-op.

### Session / transport

| Key | Type | Legacy | Modern |
| --- | --- | --- | --- |
| `perfDiagnostics` | string then bool | read as string first; applied only when non-empty | same |
| `armed` | bool | plain | plain |
| `clicking` | bool | plain | plain |
| `supportsStatePatches` | bool | not read | plain, default `false`; forced `false` on every accept |
| `reloadMappingsNonce` | int | not read | default `prev` |

### Autoclicker / right click

| Key | Type | Legacy | Modern |
| --- | --- | --- | --- |
| `minCPS` | float | `[1.0, 25.0]`, default `prev` | same |
| `maxCPS` | float | `[1.0, 25.0]`, then `max < min -> max = min` | same |
| `left` | bool | plain | **not read** |
| `right` | bool | plain | plain |
| `rightMinCPS` | float | `[1.0, 25.0]`, default `prev` | same |
| `rightMaxCPS` | float | `[1.0, 25.0]`, then `< min -> = min` | same |
| `rightBlock` | bool | plain | plain |
| `breakBlocks` | bool | plain | plain |
| `jitter` | bool | plain | plain |
| `randomizationMode` | int | **not read** | **not read** |
| `clickInChests` | bool | plain | plain |

### Aim assist / triggerbot

| Key | Type | Legacy | Modern |
| --- | --- | --- | --- |
| `aimAssist` | bool | plain | plain |
| `aimAssistFov` | float | **not read** | **not read** |
| `aimAssistRange` | float | **not read** | **not read** |
| `aimAssistStrength` | int | **not read** | **not read** |
| `triggerbot` | bool | **not read** | plain |

### Kill aura (all keys read by both bridges)

`killAuraCpsMode`, `killAuraMode`, `killAuraSort`, `killAuraRotations`, `killAuraMoveFix`,
`killAuraShowTarget`, `killAuraDebug` and `killAuraAutoBlock` are string enums mapped to
integers inside the bridge; an unrecognised string selects the miss branch.

| Key | Type | Legacy | Modern | Enum mapping / notes |
| --- | --- | --- | --- | --- |
| `killAura` | bool | plain | plain | |
| `killAuraCpsMode` | string | plain | plain | `"record"` -> record mode, else normal |
| `killAuraMode` | string | plain | plain | `"single"` -> single, else switch |
| `killAuraSort` | string | plain | plain | `distance`=0, `hurttime`=2, `fov`=3, miss=1 |
| `killAuraAutoBlock` | string | plain | plain | index in `none,vanilla,spoof,hypixel,blink,interact,swap,legit,fake,morden`; miss -> `hypixel` |
| `killAuraAttackTick` | int | `[0,5]` default `0` | same | |
| `killAuraAutoBlockRequirePress` | bool | plain | plain | |
| `killAuraAutoBlockCps` | float | `[1,10]` default `8` | same | |
| `killAuraAutoBlockRange` | float | `[3,8]` default `6` | same | |
| `killAuraSwingRange` | float | `[3,6]` default `3.5` | same | |
| `killAuraAttackRange` | float | `[3,6]` default `3` | same | |
| `killAuraFov` | int | `[30,360]` default `360` | same | |
| `killAuraMinCps` | int | `[1,20]` default `14` | same | `min > max -> min = max` |
| `killAuraMaxCps` | int | `[1,20]` default `14` | same | |
| `killAuraSwitchDelay` | int | `[0,1000]` default `150` | same | ms |
| `killAuraRotations` | string | plain | plain | `none`=0, `legit`=1, `lockview`=3, `liquidbounce`=4, `hypixel`=5, `grok`=6, miss=silent(2) |
| `killAuraDeadZone` | float | `[0,2]` default `0.5` | same | |
| `killAuraMaxTurnSpeed` | float | `[5,180]` default `25` | same | |
| `killAuraMinTurnSpeed` | float | `[1,90]` default `5` | same | |
| `killAuraAcceleration` | float | `[0.1,10]` default `2.5` | same | |
| `killAuraDeceleration` | float | `[0.1,10]` default `1.5` | same | |
| `killAuraUseOvershoot` | bool | default `true` | same | |
| `killAuraOvershootStrength` | float | `[0,20]` default `5` | same | |
| `killAuraOvershootRecovery` | float | `[0.01,1]` default `0.2` | same | |
| `killAuraNoiseStrength` | float | `[0,2]` default `0.2` | same | |
| `killAuraVisualizeAim` | bool | default `true` | same | |
| `killAuraSmoothBack` | bool | default `true` | same | |
| `killAuraMoveFix` | string | plain | plain | `none`=0, `strict`=2, miss=silent(1) |
| `killAuraSmoothing` | int | `[0,100]` default `0` | same | |
| `killAuraRavenSmoothing` | int | `[0,10]` default `0` | same | |
| `killAuraRavenPredictTicks` | int | `[0,5]` default `0` | same | |
| `killAuraRavenYawRandom` | int | `[0,5]` default `0` | same | |
| `killAuraGrokMaxSkew` | float | `[6.0,25.0]` default `12.0` | same | |
| `killAuraAngleStep` | int | `[30,180]` default `90` | same | |
| `killAuraThroughWalls` | bool | default `true` | same | |
| `killAuraRequirePress` | bool | plain | plain | |
| `killAuraAllowMining` | bool | default `true` | same | |
| `killAuraWeaponsOnly` | bool | default `true` | same | |
| `killAuraAllowTools` | bool | plain | plain | |
| `killAuraInventoryCheck` | bool | default `true` | same | |
| `killAuraBotCheck` | bool | default `true` | same | |
| `killAuraPlayers` | bool | default `true` | same | |
| `killAuraBosses` | bool | plain | plain | |
| `killAuraMobs` | bool | plain | plain | |
| `killAuraAnimals` | bool | plain | plain | |
| `killAuraGolems` | bool | plain | plain | |
| `killAuraSilverfish` | bool | plain | plain | |
| `killAuraTeams` | bool | default `true` | same | |
| `killAuraShowTarget` | string | plain | plain | `"default"` enables |
| `killAuraDebug` | string | plain | plain | `"health"` enables |
| `killAuraRandomize` | bool | default `true` | same | |
| `killAuraRandomizeRange` | float | `[0,1]` default `0.4` | same | |
| `killAuraYRandomize` | float | `[0,1]` default `0.3` | same | |
| `killAuraLbHorizontalSpeed` | float | `[1,180]` default `180` | same | |
| `killAuraLbVerticalSpeed` | float | `[1,180]` default `180` | same | |
| `killAuraLbSmooth` | float | `[0.1,1]` default `0.5` | same | |
| `killAuraLbPredict` | bool | default `true` | same | |
| `killAuraLbPredictSize` | float | `[0,3]` default `1` | same | |
| `killAuraLbRandomize` | bool | default `true` | same | |
| `killAuraLbRandomRange` | float | `[0,1]` default `0.5` | same | |
| `killAuraLbHorizontalSearch` | float | `[0,1]` default `0.5` | same | |
| `killAuraLbBodyMin` | float | `[0,1]` default `0.1` | same | |
| `killAuraLbBodyMax` | float | `[0,1]` default `0.9` | same | |

### Speed bridge / GTB / pixel party

| Key | Type | Legacy | Modern |
| --- | --- | --- | --- |
| `speedBridge` | bool | plain | plain |
| `speedBridgeBlockOnly` | bool | plain | plain |
| `speedBridgeDelayMs` | int | `< 20` -> keep; `> 250` -> `250` | `[20,250]`, default `prev` |
| `speedBridgeHoldingShiftOnly` | bool | plain | plain |
| `speedBridgeLookingDownOnly` | bool | plain | plain |
| `gtbHelper` | bool | plain | plain |
| `gtbHint` | string | empty -> keep previous | assigned verbatim |
| `gtbCount` | int | default `prev`; `< 0` -> keep | default `0` |
| `gtbPreview` | string | empty -> keep previous | assigned verbatim |
| `pixelPartyAssist` | bool | plain | plain |
| `pixelPartyScanRadius` | int | `< 8` -> `8`; `> 48` -> `48`; default `prev` | `[8,48]`, default `prev` |
| `pixelPartyAutoLook` | bool | plain | plain |
| `pixelPartyAutoWalk` | bool | plain | plain |

### Nametags / nick hider / HUD chrome

| Key | Type | Legacy | Modern |
| --- | --- | --- | --- |
| `nametags` | bool | plain | plain |
| `nickHiderEnabled` | bool | plain; forced `false` when the alias is empty | same |
| `nickHiderAlias` | string | normalised (`lc::NormalizeNickHiderAlias`) | same |
| `closestPlayerInfo` | bool | plain | plain |
| `fightStatus` | bool | plain | plain |
| `nametagShowHealth` | bool | read twice, plain | plain |
| `nametagShowArmor` | bool | read twice, plain | plain |
| `nametagShowHeldItem` | bool | read twice, default `true` | default `true` |
| `nametagHideVanilla` | bool | read twice, plain | plain |
| `nametagRange` | int | `< 1` -> keep; `> 8` -> `8` | `[1,8]`, default `prev` |
| `showModuleList` | string | empty -> `true`, else `== "true"` | same |
| `moduleListStyle` | int | default `-1`; `>= 0` -> `[0,6]`; `< 0` -> keep | `[0,6]`, default `0` |
| `showLogo` | string | empty -> `true`, else `== "true"` | same |
| `guiTheme` | string | empty -> `"Default"` | same |
| `hudEditor` | bool | plain | plain |
| `hudLayout` | object | `lc::ParseHudLayout` — see `## HUD layout sync` | same |

### Chest ESP / chest stealer / refill / block ESP / bed plates

| Key | Type | Legacy | Modern |
| --- | --- | --- | --- |
| `chestEsp` | bool | plain | plain |
| `chestEspRange` | int | `< 1` -> keep; `> 8` -> `8` | `[1,8]`, default `prev` |
| `chestStealerEnabled` | bool | plain | plain |
| `chestStealerDelayMs` | int | `< 50` -> keep; `> 500` -> `500` | `[50,500]`, default `prev` |
| `chestStealerMenuCheck` | bool | default `true`; afterwards recomputed as OR of the three checks | same |
| `chestStealerTitleCheck` | bool | default = `chestStealerMenuCheck` | same |
| `chestStealerCustomItemsCheck` | bool | default = `chestStealerMenuCheck` | same |
| `chestStealerPhysicalCheck` | bool | default = `chestStealerMenuCheck` | same |
| `refillEnabled` | bool | plain | plain |
| `refillDelayMs` | int | `[50,500]`, default `prev` | same |
| `blockEspEnabled` | bool | plain | plain |
| `blockEspBoxes` | bool | default `true` | same |
| `blockEspTracers` | bool | plain | plain |
| `blockEspHud` | bool | default `true` | same |
| `blockEspMaxCount` | int | default `-1`; `>= 1` -> `min(v,512)`; `< 1` -> keep | `[1,512]`, default `prev` |
| `blockEspRange` | int | default `-1`; `>= 1` -> `min(v,8)`; `< 1` -> keep | `[1,8]`, default `prev` |
| `blockEspBlocks` | string | `lc::ParseBlockEspTargets`, re-parsed every config line; the cache version bumps only when the target set actually changes | same |
| `bedPlatesEnabled` | bool | plain | plain |
| `bedPlatesShowDistance` | bool | default `true` | same |
| `bedPlatesRange` | int | default `-1`; `>= 1` -> `min(v,8)`; `< 1` -> keep | `[1,8]`, default `prev` |

### Reach / velocity / anti-debuff / hit-delay

| Key | Type | Legacy | Modern |
| --- | --- | --- | --- |
| `reachEnabled` | bool | plain | plain |
| `reachMin` | float | plain; `<= 0` -> keep previous | `[3.0,6.0]`, default `prev` |
| `reachMax` | float | plain; `< min` -> `= min` | `[3.0,6.0]`; then `< min -> = min` |
| `reachChance` | int | `< 1` -> keep; `> 100` -> `100` | default `100`, **unclamped** |
| `reachChanceMode` | int | default `prev`, unclamped | `[0,1]`, default `0` |
| `reachOnlyWhileSprinting` | bool | default `prev` | plain |
| `reachDisableInWater` | bool | default `prev` | plain |
| `reachVerticalCheck` | bool | default `prev` | plain |
| `velocityEnabled` | bool | plain | plain |
| `velocityHorizontal` | int | `< 1` -> keep; `> 100` -> `100` | `[1,100]`, default `100` |
| `velocityVertical` | int | `< 1` -> keep; `> 100` -> `100` | `[1,100]`, default `100` |
| `velocityChance` | int | `< 1` -> keep; `> 100` -> `100` | `[1,100]`, default `100` |
| `antiDebuffEnabled` | bool | plain | plain |
| `hitDelayFixEnabled` | bool | plain | plain |

### Auto totem (modern only)

| Key | Type | Legacy | Modern |
| --- | --- | --- | --- |
| `autoTotemEnabled` | bool | not read | plain |
| `autoTotemMode` | int | not read | `[0,1]`, default `0` |
| `autoTotemHealth` | int | not read | `[0,36]`, default `10` |
| `autoTotemElytra` | bool | not read | plain |
| `autoTotemDelay` | int | not read | `[0,20]`, default `0` |
| `autoTotemBehaviorMode` | int | not read | `[0,1]`, default `0` |

### Auto rod / throwpot / auto heal / auto tool

| Key | Type | Legacy | Modern |
| --- | --- | --- | --- |
| `autoRodEnabled` | bool | plain; `false` cancels queued rod requests | plain |
| `autoRodSlotMode` | int | `[0,9]`, default `prev` | same |
| `autoRodVerifyForcedSlot` | bool | default `true` | same |
| `autoRodExtensionTicks` | int | `[1,40]` (`autorod::kMinExtensionTicks`..`kMaxExtensionTicks`), default `prev` | same |
| `autoRodHoldToExtend` | bool | default `false` | same |
| `throwpotEnabled` | bool | plain; `false` clears the pending request | plain |
| `autoHealEnabled` | bool | plain | plain |
| `autoHealHealth` | int | `[1,20]`, default `prev` | same |
| `autoHealDelayMs` | int | `[50,2000]`, default `prev` | same |
| `autoToolEnabled` | bool | plain | plain |
| `autoToolSwapWeapon` | bool | default `true` | same |
| `autoToolInstantSwap` | bool | default `true` | same |
| `autoToolSwapToDelay` | int | `[0,500]` default `50` | same |
| `autoToolSwapBack` | bool | default `false` | same |
| `autoToolSwapBackDelay` | int | `[0,1000]` default `350` | same |
| `autoToolRequireMouseDown` | bool | default `true` | same |
| `autoToolOnlySneaking` | bool | default `false` | same |
| `autoToolBedwarsMode` | bool | default `true` | same |

### Fireball warning and single-toggle modules

| Key | Type | Legacy | Modern |
| --- | --- | --- | --- |
| `fireballWarning` | bool | plain | plain |
| `fireballWarningBox` | bool | default `true` | same |
| `fireballWarningArrow` | bool | default `true` | same |
| `fireballWarningSound` | bool | default `true` | same |
| `fastPlace` | bool | plain | plain |
| `noHurtCam` | bool | plain | plain |
| `sprint` | bool | plain | plain |
| `tntTimer` | bool | plain | plain |
| `wTap` | bool | plain | plain |
| `jumpReset` | bool | plain | plain |
| `jumpResetChance` | int | `[0,100]`, default `prev` | same |
| `hitSelect` | bool | plain | plain |
| `trajectories` | bool | plain | plain |
| `murderMystery` | bool | plain | plain |
| `breakProgress` | bool | plain | plain |

### Keybinds

`keybind*` values are virtual-key codes. `0` means "unbound". `-1` is the legacy host-side
sentinel for "absent", which is why the legacy bridge reads with default `-1` and applies only
when `>= 0` — an absent key leaves the previous binding intact. The modern bridge uses
`ClampInt(..., 0, 255)`, so a missing key re-writes the current value.

| Key | Legacy | Modern |
| --- | --- | --- |
| `keybindAutoclicker` | `>=0` apply, unclamped | **not read** |
| `keybindRightClick` | **not read** | **not read** |
| `keybindAimAssist` | **not read** | **not read** |
| `keybindTriggerbot` | **not read** | `[0,255]` |
| `keybindKillAura` | **not read** | **not read** |
| `keybindSpeedBridge` | `>=0` apply | **not read** |
| `keybindGtbHelper` | **not read** | **not read** |
| `keybindNametags` | `>=0` apply | **not read** |
| `keybindClosestPlayer` | `>=0` apply | **not read** |
| `keybindFightStatus` | `>=0` apply | default `0`, no clamp |
| `keybindChestEsp` | `>=0` apply | **not read** |
| `keybindChestStealer` | `>=0` apply | **not read** |
| `keybindRefill` | `>=0` apply | **not read** |
| `keybindBlockEsp` | `>=0` apply | **not read** |
| `keybindBedPlates` | `>=0` apply | **not read** |
| `keybindPixelPartyAssist` | **not read** | **not read** |
| `keybindAutoRod` | `[0,255]` | `[0,255]` |
| `keybindThrowpot` | `[0,255]` | `[0,255]` |
| `keybindAutoheal` | **not read** | **not read** |
| `keybindAutoTool` | `[0,255]` | `[0,255]` |
| `keybindFireballWarning` | `<0` ignore, else apply (read twice) | `[0,255]` |
| `keybindFastPlace` | `[0,255]` | `[0,255]` |
| `keybindNoHurtCam` | `[0,255]` | `[0,255]` |
| `keybindSprint` | `[0,255]` | `[0,255]` |
| `keybindTntTimer` | `[0,255]` | `[0,255]` |
| `keybindWTap` | `[0,255]` | `[0,255]` |
| `keybindJumpReset` | `[0,255]` | `[0,255]` |
| `keybindHitSelect` | `[0,255]` | `[0,255]` |
| `keybindTrajectories` | `[0,255]` | `[0,255]` |
| `keybindMurderMystery` | `[0,255]` | `[0,255]` |
| `keybindBreakProgress` | `[0,255]` | `[0,255]` |

### Host-sent keys that no bridge reads

`randomizationMode`, `aimAssistFov`, `aimAssistRange`, `aimAssistStrength`;
`supportsStatePatches` and `reloadMappingsNonce` and `autoTotem*` on the legacy bridge only;
and every keybind marked "not read" above. These must still be sent unchanged: the payload
shape is part of the contract and a future bridge may consume them.

---

## HUD layout sync

### Structure

`hudLayout` is a flat object keyed by element id; each value is
`{"x":<double>,"y":<double>,"scale":<double>}`.

- `x`, `y` are **normalised screen fractions in `[0,1]`**, multiplied by the window dimensions
  to obtain pixels. `0,0` is the top-left.
- `scale` is a size multiplier clamped to `[0.5, 2.0]` (`HudLayout.MinScale` / `MaxScale`;
  `lc::ClampHudScale`).
- `movable` / `resizable` are **bridge-authoritative and never on the wire**. `hud_layout.h`
  states this explicitly: interaction constraints come from `DefaultLayout()` and gate the
  editor, so a config can never make a non-movable element draggable. `nametags` is the one
  non-movable element.
- Coordinate clamps: `lc::ClampHudCoord` = `[0,1]`; `lc::ClampHudScale` = `[0.5,2.0]` with
  non-finite or `<= 0` becoming `1.0`. Host equivalents: `ClampCoord` (`[0,1]`, NaN/Inf ->
  element default) and `ClampScale` (`[0.5,2.0]`, NaN/Inf/`0` -> element default).
- Pixel placement (`HudElementPixelPos`) is `floor(x * winW)`, `floor(y * winH)`, then clamped
  to `[0, winW - contentW]` / `[0, winH - contentH]`; a zero or negative window returns
  `(0,0)`.

### Element ids and canonical defaults

Order below is the emission order in both the host (`HudLayout._knownIds`) and the bridge
(`SerializeHudLayout`'s `ids[]`) — the same order on both sides.

| id | default `x` | default `y` | default `scale` | movable |
| --- | --- | --- | --- | --- |
| `modulelist` | `0.985` | `0.020` | `1.0` | yes |
| `closestplayer` | `0.500` | `0.780` | `1.0` | yes |
| `pixelparty` | `0.500` | `0.660` | `1.0` | yes |
| `chestesplist` | `0.015` | `0.300` | `1.0` | yes |
| `gtbhint` | `0.500` | `0.620` | `1.0` | yes |
| `nametags` | `0.000` | `0.000` | `1.0` | **no** |
| `blockesplist` | `0.015` | `0.560` | `1.0` | yes |

Seven ids. Two comments in `HudLayout.cs` say "six"; the constant list and `BuildDefaults()`
both contain seven.

### Outbound (host -> bridge)

`clicker.HudLayout.ToJson()` produces every id, always, in the order above:

```json
{"modulelist":{"x":0.985,"y":0.02,"scale":1},"closestplayer":{"x":0.5,"y":0.78,"scale":1},
 "pixelparty":{"x":0.5,"y":0.66,"scale":1},"chestesplist":{"x":0.015,"y":0.3,"scale":1},
 "gtbhint":{"x":0.5,"y":0.62,"scale":1},"nametags":{"x":0,"y":0,"scale":1},
 "blockesplist":{"x":0.015,"y":0.56,"scale":1}}
```

Values are `double`s written by `System.Text.Json` shortest-round-trip formatting (`0.02`,
not `0.020000`). The bridge parses them with `std::stof` on the substring after the marker, so
any JSON number format it can parse is accepted.

### Inbound (bridge -> host)

Emitted inside the state document as the **last** member, only when
`g_hudEditor.layoutDirty` was set, and only once — the flag is consumed:

```json
,"hudLayout":{"modulelist":{"x":0.985000,"y":0.020000,"scale":1.000000},...}
```

`lc::SerializeHudLayout` uses `snprintf("%.6f")` for all three fields and writes all seven ids
in the order above, substituting a default-constructed `HudElementLayout{}` (`x`=0, `y`=0,
`scale`=1) for an id missing from the map. Emitted on the legacy publish tick whenever
dirtied; on the modern path only inside the `sendFullState` block.

### Schema agreement between the two sides

- The id set and key names are duplicated constants on both sides (`ELEM_*` in
  `hud_layout.h`, `HudElementId` in `HudLayout.cs`). Nothing negotiates them; a mismatch
  silently drops an element.
- The bridge's parser is position-based, not a JSON parser: `ParseHudLayout` searches the raw
  config line for `"hudLayout":`, requires the next byte to be `{`, then brace-matches to the
  closing `}`. It then searches that substring for `"<id>":{` per id and brace-matches again.
  Consequences: key order is irrelevant; unknown keys inside an element are fine; a missing
  `x`/`y`/`scale` keeps the element default for that field; a `"hudLayout":` occurrence
  earlier in the line (e.g. inside a string value) is taken as the start; and an unknown
  element id is simply never looked up.
- Host `HudLayout.FromJson` ignores unknown ids (forward compatibility) and fills missing
  values from the canonical defaults.
- Host echo guard: `ApplyInboundHudLayout` computes `HudLayout.FromJson(node)` and drops the
  update when `inbound.EqualsLayout(Clicker.Instance.HudLayout)` with tolerance `1e-9` per
  field. When it accepts, it sets `_suppressConfigPush = true` around the UI update and the
  profile save so the config loop does not immediately echo the layout back.
- Drag protection (bridge side): while the user is dragging an element
  (`g_hudEditor.grabbedId` non-empty), the incoming layout for that id is replaced by the live
  value so a periodic config push cannot reset it mid-gesture. The final layout is sent back
  when the gesture ends.

---

## Profile & config backward compatibility

`Profile` is the on-disk user profile. Requirement: existing profiles must deserialize and
re-serialise compatibly, so the key spellings and defaults below are load-bearing.

### Serialiser settings (`ProfileManager.JsonOptions`)

```csharp
new JsonSerializerOptions { WriteIndented = true, PropertyNamingPolicy = JsonNamingPolicy.CamelCase }
```

- No `JsonPropertyName` attributes exist anywhere in `Profile.cs`; the JSON name is always the
  camelCase conversion of the C# property name.
- The naming policy applies to **both** write and read, and matching is case-sensitive. A file
  containing the JSON key `MinCPS` does **not** bind to the C# property `MinCPS`; the policy-produced key is `minCPS`.
- No `PropertyNameCaseInsensitive`, no `NumberHandling`, no `DefaultIgnoreCondition`, no enum
  converter, no `JsonExtensionData`, no `[JsonRequired]`.
- Location: `%AppData%\Aoko\profiles\<Name>.json`; a one-time copy migrates
  `%AppData%\LegoClicker\profiles\*.json` when the new directory holds no `*.json`.
- `WriteIndented = true`, so files are pretty-printed. A Rust writer must not depend on the
  exact whitespace, but a round-trip diff test will see indentation.

### Tolerance rules (what "compatible" means)

| Situation | Behaviour |
| --- | --- |
| Unknown property in file | Ignored on read; **dropped** on the next save (no extension data) |
| Missing property | Property initialiser default applies |
| Unknown element id inside `hudLayout` | Ignored |
| Missing element id inside `hudLayout`, or `hudLayout:null` | Canonical defaults for that element |
| Malformed JSON | `LoadProfile` catches everything and returns `null` |
| File missing | Returns `null` |
| Nested object of the wrong type (e.g. `"killAuraSettings": 5`) | `JsonException` -> whole load returns `null` |
| Write failure (IO / ACL / NotSupported) | Reported to debug output only; profile unchanged |
| Out-of-range value in a `KillAuraSettings` property | Clamped by the property setter on assign |

### Top-level properties (143)

Format: JSON key — C# type — default. Every key is written on every save.

Two setter behaviours must survive the port, because they are observable through a
load/save round trip and through the GUI:

- `ChestStealerMenuCheck` is not a plain flag. Its setter assigns the menu check, then,
  while the private `_hasExplicitSubChecks` field is still `false`, copies the value into
  `chestStealerTitleCheck`, `chestStealerCustomItemsCheck` and `chestStealerPhysicalCheck`.
  Each of those three setters sets `_hasExplicitSubChecks = true`. `_hasExplicitSubChecks`
  is private and therefore **not serialised**, so it resets to `false` on every load and
  the cascade therefore depends on JSON property order within the object.
- `AutoRodSlotMode` clamps to `0..=9` and `AutoRodExtensionTicks` clamps to `1..=40` in
  their setters, so an out-of-range value read from an old profile is coerced on load and
  the coerced value is what gets written back.

| Key | Type | Default |
| --- | --- | --- |
| `name` | string | `"Default"` |
| `guiTheme` | string | `"Slate"` |
| `moduleListStyle` | string | `"Default"` |
| `showLogo` | bool | `true` |
| `devMode` | bool | `false` |
| `discordRpcEnabled` | bool | `true` |
| `isArmed` | bool | `false` |
| `minCPS` | float | `8.0` |
| `maxCPS` | float | `12.0` |
| `leftClickEnabled` | bool | `true` |
| `rightClickEnabled` | bool | `false` |
| `rightMinCPS` | float | `10.0` |
| `rightMaxCPS` | float | `14.0` |
| `rightClickOnlyBlock` | bool | `false` |
| `breakBlocksEnabled` | bool | `false` |
| `randomizationMode` | enum `ClickRandomizationMode` | `1` (`Medium`) |
| `jitterEnabled` | bool | `true` |
| `clickInChests` | bool | `false` |
| `aimAssistEnabled` | bool | `false` |
| `aimAssistFov` | float | `30.0` |
| `aimAssistRange` | float | `4.5` |
| `aimAssistStrength` | int | `40` |
| `triggerbotEnabled` | bool | `false` |
| `triggerbotOnlyCrosshair` | bool | `true` |
| `triggerbotOnlyIfCanAttack` | bool | `true` |
| `triggerbotCooldownThreshold` | int | `92` |
| `triggerbotHitChance` | int | `100` |
| `triggerbotRequireClick` | bool | `true` |
| `killAuraEnabled` | bool | `false` |
| `killAuraSettings` | object | see below |
| `killAuraRange` | float | `2.9` (legacy, migration only) |
| `killAuraAimRange` | float | `3.5` (legacy) |
| `killAuraMinTurnSpeed` | float | `10.0` (legacy) |
| `killAuraMaxTurnSpeed` | float | `20.0` (legacy) |
| `killAuraTargetMode` | string | `"distance"` (legacy) |
| `killAuraSwitchDelayMs` | int | `350` (legacy) |
| `killAuraRandomization` | int | `18` (legacy) |
| `killAuraClickMode` | string | `"cooldown"` (legacy) |
| `killAuraMinCps` | float | `10.0` (legacy) |
| `killAuraMaxCps` | float | `14.0` (legacy) |
| `killAuraFov` | int | `120` (legacy) |
| `killAuraRotMode` | string | `"legit"` (legacy) |
| `killAuraRequirePress` | bool | `false` (legacy) |
| `killAuraWeaponsOnly` | bool | `true` (legacy) |
| `speedBridgeEnabled` | bool | `false` |
| `speedBridgeBlockOnly` | bool | `true` |
| `speedBridgeDelayMs` | int | `85` |
| `speedBridgeHoldingShiftOnly` | bool | `true` |
| `speedBridgeLookingDownOnly` | bool | `true` |
| `gtbHelperEnabled` | bool | `false` |
| `pixelPartyAssistEnabled` | bool | `false` |
| `pixelPartyScanRadius` | int | `28` |
| `pixelPartyAutoLookEnabled` | bool | `false` |
| `pixelPartyAutoWalkEnabled` | bool | `false` |
| `nametagsEnabled` | bool | `false` |
| `nickHiderEnabled` | bool | `false` |
| `nickHiderAlias` | string | `""` |
| `showModuleList` | bool | `true` |
| `closestPlayerInfoEnabled` | bool | `false` |
| `fightStatusEnabled` | bool | `false` |
| `nametagShowHealth` | bool | `true` |
| `nametagShowArmor` | bool | `true` |
| `nametagShowHeldItem` | bool | `true` |
| `nametagHideVanilla` | bool | `false` |
| `nametagRange` | int | `4` |
| `chestEspEnabled` | bool | `false` |
| `chestEspRange` | int | `4` |
| `chestStealerEnabled` | bool | `false` |
| `chestStealerDelayMs` | int | `120` |
| `chestStealerMenuCheck` | bool | `true` |
| `chestStealerTitleCheck` | bool | `true` |
| `chestStealerCustomItemsCheck` | bool | `true` |
| `chestStealerPhysicalCheck` | bool | `true` |
| `refillEnabled` | bool | `false` |
| `refillDelayMs` | int | `120` |
| `blockEspEnabled` | bool | `false` |
| `blockEspBoxes` | bool | `true` |
| `blockEspTracers` | bool | `false` |
| `blockEspHud` | bool | `true` |
| `blockEspMaxCount` | int | `64` |
| `blockEspRange` | int | `4` |
| `blockEspTargets` | array | preset list, see below |
| `bedPlatesEnabled` | bool | `false` |
| `bedPlatesShowDistance` | bool | `true` |
| `bedPlatesRange` | int | `4` |
| `reachEnabled` | bool | `false` |
| `reachMin` | float | `3.0` |
| `reachMax` | float | `3.0` |
| `reachChance` | int | `100` |
| `reachChanceMode` | int | `0` |
| `reachOnlyWhileSprinting` | bool | `false` |
| `reachDisableInWater` | bool | `false` |
| `reachVerticalCheck` | bool | `false` |
| `velocityEnabled` | bool | `false` |
| `velocityHorizontal` | int | `100` |
| `velocityVertical` | int | `100` |
| `velocityChance` | int | `100` |
| `autoTotemEnabled` | bool | `false` |
| `autoTotemMode` | int | `0` |
| `autoTotemHealth` | int | `10` |
| `autoTotemElytra` | bool | `true` |
| `autoTotemDelay` | int | `0` |
| `autoTotemBehaviorMode` | int | `0` |
| `autoRodEnabled` | bool | `false` |
| `throwpotEnabled` | bool | `false` |
| `autoHealEnabled` | bool | `false` |
| `autoHealHealth` | int | `17` |
| `autoHealDelayMs` | int | `500` |
| `autoRodSlotMode` | int | `0` |
| `autoRodVerifyForcedSlot` | bool | `true` |
| `autoRodExtensionTicks` | int | `4` |
| `autoRodHoldToExtend` | bool | `false` |
| `autoRodActionKey` | int | `0` |
| `throwpotActionKey` | int | `0` |
| `autoToolEnabled` | bool | `false` |
| `autoToolSwapWeapon` | bool | `true` |
| `autoToolInstantSwap` | bool | `true` |
| `autoToolSwapToDelay` | int | `50` |
| `autoToolSwapBack` | bool | `false` |
| `autoToolSwapBackDelay` | int | `350` |
| `autoToolRequireMouseDown` | bool | `true` |
| `autoToolOnlySneaking` | bool | `false` |
| `autoToolBedwarsMode` | bool | `true` |
| `antiDebuffEnabled` | bool | `false` |
| `hitDelayFixEnabled` | bool | `false` |
| `fireballWarningEnabled` | bool | `false` |
| `fireballWarningBox` | bool | `true` |
| `fireballWarningArrow` | bool | `true` |
| `fireballWarningSound` | bool | `true` |
| `fastPlaceEnabled` | bool | `false` |
| `noHurtCamEnabled` | bool | `false` |
| `sprintEnabled` | bool | `false` |
| `tntTimerEnabled` | bool | `false` |
| `wTapEnabled` | bool | `false` |
| `jumpResetEnabled` | bool | `false` |
| `hitSelectEnabled` | bool | `false` |
| `trajectoriesEnabled` | bool | `false` |
| `murderMysteryEnabled` | bool | `false` |
| `breakProgressEnabled` | bool | `false` |
| `jumpResetChance` | int | `80` |
| `hudLayout` | object, nullable | `null` |
| `moduleKeys` | object | 39 entries, all `0` |
| `theme` | string | `"Slate"` |

Notes on individual keys:

- `randomizationMode` serialises as a **number**: `0` = Basic, `1` = Medium, `2` = Advanced.
  No `JsonStringEnumConverter` is registered.
- `jitterEnabled`'s setter has a side effect: while no explicit randomization mode has been
  assigned, `true` implies `Medium` and `false` implies `Basic`. `System.Text.Json` binds
  properties in document order, so a file where `jitterEnabled` precedes `randomizationMode`
  can produce a different final mode than the reverse order. Preserve the side effect if the
  Rust reader is to round-trip identically.
- `guiTheme` and `theme` are two independent properties, both defaulting to `"Slate"`, both
  persisted. Do not collapse them.
- `minCPS`/`maxCPS` and `rightMinCPS`/`rightMaxCPS` keep their internal capitalisation.

### `moduleKeys`

A `Dictionary<string, int>` with 39 fixed keys, all initialised to `0`. Keys, in source order:

```
autoclicker, rightclick, aimassist, triggerbot, killaura, speedbridge, gtbhelper,
pixelpartyassist, nametags, nickhider, closestplayer, fightstatus, chestesp, cheststealer,
refill, blockesp, bedplates, reach, velocity, autototem, autorod, throwpot, autoheal,
autotool, antidebuff, hitdelayfix, fireballwarning, fastplace, nohurtcam, sprint, tnttimer,
wtap, jumpreset, hitselect, trajectories, murdermystery, breakprogress, panic, hudeditor
```

A pre-existing file may contain `silentaura`; the loader removes it during migration. Unknown
keys in `moduleKeys` are preserved through deserialisation into the dictionary and are
re-serialised, because a `Dictionary` keeps them — unlike unknown top-level properties, which
are dropped.

### Nested `killAuraSettings`

62 persisted properties, camelCase of the C# names, no attributes. Every setter clamps, so a
hand-edited out-of-range value is clamped rather than rejected.

```
cpsMode "normal" | mode "switch" | sort "health" | autoBlock "hypixel" | attackTick 0
autoBlockRequirePress false | autoBlockCps 8 | autoBlockRange 6 | swingRange 3.5
attackRange 3 | fov 360 | minCps 14 | maxCps 14 | switchDelay 150 | rotations "silent"
deadZoneSize 0.5 | maxTurnSpeed 25 | minTurnSpeed 5 | acceleration 2.5 | deceleration 1.5
useOvershoot true | overshootStrength 5 | overshootRecovery 0.2 | noiseStrength 0.2
visualizeAim true | smoothBack true | moveFix "silent" | smoothing 0 | ravenSmoothing 0
ravenPredictTicks 0 | ravenYawRandom 0 | grokMaxSkew 12 | angleStep 90 | throughWalls true
requirePress false | allowMining true | weaponsOnly true | allowTools false
inventoryCheck true | botCheck true | players true | bosses false | mobs false | animals false
golems false | silverfish false | teams true | showTarget "none" | debugLog "none"
randomize true | randomizeRange 0.4 | yRandomizeStrength 0.3
liquidBounceHorizontalSpeed 180 | liquidBounceVerticalSpeed 180
liquidBounceSmoothFactor 0.5 | liquidBouncePredict true | liquidBouncePredictSize 1
liquidBounceRandomize true | liquidBounceRandomizeRange 0.5
liquidBounceHorizontalSearch 0.5 | liquidBounceBodyPointMin 0.1
liquidBounceBodyPointMax 0.9
```

String-valued properties are validated against an allow-list on assign (`SetMode`); an
unknown string falls back to the first allowed value.

### Nested `hudLayout` in a profile

`Dictionary<string, HudElementLayout>?` — nullable precisely so a pre-HUD profile without the
key deserialises; `null` is then treated as canonical defaults by `ApplyToClicker`. Keys are
the seven element ids; values are `{"x":double,"y":double,"scale":double}` under the same
clamp rules as the wire format. `ProfileManager.BuildHudLayoutDict` / `BuildHudLayout` convert
between the dictionary and `HudLayout`.

### Nested `blockEspTargets`

`List<BlockEspTargetData>`, each item:

| Key | Type | Default |
| --- | --- | --- |
| `registryId` | string | `""` |
| `displayName` | string | `""` |
| `colorHex` | string | `BlockEspConfig.DefaultColorHex` |
| `enabled` | bool | `true` |

`colorHex` is `RRGGBB` **without** a leading `#`. The wire key `blockEspBlocks` carries the
serialised list (`BlockEspBlocksSerialized` in the host).

### Not part of `Profile`

- Accounts live in `Aoko/Core/Accounts/` and servers in `Aoko/Core/Servers/`; neither is a
  `Profile` property, and no profile file references an account or a server.
- Custom palettes are a separate file, `%AppData%\Aoko\palettes.json`, written by
  `PaletteStore` with the same camelCase policy plus
  `DefaultIgnoreCondition = JsonIgnoreCondition.Never`:

  | Key | Default |
  | --- | --- |
  | `name` | `"Custom"` |
  | `background` | `"#0A0B0F"` |
  | `panel` | `"#12141A"` |
  | `sliderBackground` | `"#181B22"` |
  | `sliderForeground` | `"#2A2F38"` |
  | `accent` | `"#C7625A"` |
  | `text` | `"#E8EAEE"` |
  | `dimText` | `"#7A8290"` |
  | `tabSelected` | `"#1F2229"` |
  | `tabHover` | `"#181B22"` |

  Colours here **do** carry a leading `#`, unlike `blockEspTargets.colorHex`.

### Migrations performed on load

`LoadProfile` parses the file, then re-parses it with `JsonDocument` and runs:

1. `MigrateSilentAuraToKillAura` — always removes `moduleKeys["silentaura"]`. If
   `silentAuraEnabled` is exactly `true` and `killAuraEnabled` is false, enables kill aura and
   copies `silentAuraRange` -> `killAuraSettings.attackRange`, `silentAuraMinCps` ->
   `killAuraSettings.minCps` (rounded), `silentAuraMaxCps` -> `killAuraSettings.maxCps`
   (rounded).
2. `MigrateFlatKillAuraSettings` — **only when the file has no `killAuraSettings` key**: copies
   the flat legacy fields into `killAuraSettings` (`attackRange` <- `killAuraRange`, `sort` <-
   `killAuraTargetMode`, `switchDelay` <- `killAuraSwitchDelayMs`, `minCps`/`maxCps` <-
   rounded `killAuraMinCps`/`killAuraMaxCps`, `fov` <- `killAuraFov`, plus `requirePress` and
   `weaponsOnly`), and maps `killAuraRotMode`: `"legit"` -> `"legit"`, `"lock"` ->
   `"lockview"`, anything else -> `"silent"`.

The legacy flat keys remain in the file and keep being written after migration, so a
round-trip preserves both representations.

---

## `commandResult` correlation

### Outbound request shapes

Built by `BridgeCommandWire` with `JsonSerializer.Serialize(JsonObject)` + `"\n"`:

```json
{"type":"accountCommand","action":"getSession","requestId":1}
{"type":"accountCommand","action":"setSession","requestId":2,"name":"Notch","uuid":"...","token":"...","refreshToken":"..."}
{"type":"accountCommand","action":"restoreSession","requestId":3}
{"type":"serverCommand","action":"join","requestId":4,"host":"mc.example.com","port":25565}
```

- `refreshToken` is emitted only when non-empty.
- `token` is the Minecraft services access token; it is written to the socket and never to logs.
  `AccountCommand.ToString()` deliberately omits credentials.
- The **wire action strings** are `getSession`, `setSession`, `restoreSession`, `join`. The
  **capability strings** are `account.getSession`, `account.setSession`,
  `account.restoreSession`, `server.join`. Do not confuse the sets: the bridge compares against
  the short forms, and the host gates UI on the dotted forms.

### `requestId` source

- `BridgeRequestIdSource` wraps `Interlocked.Increment(ref _last)`, so ids are `1, 2, 3, ...`
  monotonically, never reused within a process, shared by all four commands.
- Completely independent of the `moduleAction` id counter (`_nextActionRequestId`).
- The host rejects an inbound `commandResult` whose `requestId` is missing or `<= 0`.

### Timeout

- `BridgeCommandDispatcher.DefaultTimeoutMs = 20000` (20 s), overridable through the
  constructor's `timeoutMs` (clamped `>= 1`).
- The wait is `completion.Task.WaitAsync(TimeSpan.FromMilliseconds(_timeoutMs), token)`; a
  `TimeoutException` becomes `BridgeCommandResult.Failure(action, "timeout")`.
- The pending entry is always removed in `finally`.
- The clock starts **after** the send completes, not when the caller enters.

### Dispatcher lifecycle

- `_pending` is a `ConcurrentDictionary<int, PendingCommand>`.
- Insert before send; remove on completion, timeout, cancellation, or failure.
- `_send` returning `false` (socket gone, guard failed) short-circuits to `send_failed`
  **without** waiting.
- `!_isConnected()` short-circuits to `not_connected` before allocating an id.
- `IOException` / `ObjectDisposedException` / `SocketException` -> `connection_lost`.
- `FailAll(error)` runs on read-loop exit, on `ConnectAsync` entry, and after a torn write; it
  completes every pending request with that error.
- `TryComplete(node)` returns `false` for a malformed document or an unknown id, so the read
  loop ignores it silently.
- Cancelling the token throws `OperationCanceledException` (no result) and leaves no entry.

### Error vocabulary

Produced by the host, not the bridge:

| Code | Meaning |
| --- | --- |
| `not_connected` | No socket |
| `send_failed` | Write refused |
| `timeout` | 20 s elapsed |
| `connection_lost` | Socket error mid-wait |
| `invalid_address` | Host-side host/port validation failed |

Observed bridge-side codes (`AccountCommandRouter`, `SessionHelperBridge`, host mapping table):
`invalid_session_data`, `in_world`, `unsupported_session_constructor`,
`session_write_not_observed`, `game_thread_timeout`, `no_launch_session`, `invalid_address`,
`connect_path_not_found`, `not_ready` / `notready`, `line_too_long`, `unknown_action`,
`getSession_failed`, plus free text such as
`Session helper not ready (wait on main menu, then retry)` and
`Disconnect from the world before switching`.

The host maps known codes to user-facing sentences and collapses everything else to one generic
refusal message, logging the raw code locally only. A Rust host should retain the code, not just
the sentence.

### Capability gating

- UI affordances are gated on `Capabilities.SupportsAction("account.setSession")`,
  `"account.getSession"`, `"account.restoreSession"`, `"server.join"` — dotted names,
  case-insensitive after normalisation.
- `actions` comes from the payload when non-empty, else from the version fallback. Since both
  shipped bridges advertise all four, gating only matters for a hypothetical older bridge.
- `SupportsModule` / `SupportsSetting` / `SupportsStateField` gate the rest of the UI.
  `SupportsStateField("actionBar")` is the one gate that changes behaviour rather than
  presentation: the host skips dispatching action-bar text to the GTB solver when the bridge
  does not advertise it.

### Bridge-side handling order

1. A line arrives in a `recv()` chunk.
2. `InboundCommandTracker.Observe` runs on the raw bytes first and may queue a `line_too_long`
   reply.
3. `BoundedNewlineBuffer` splits the chunk into complete lines.
4. `ParseConfig` dispatches on `type`: `accountCommand`/`serverCommand` ->
   `HandleAccountCommand`, which attaches JNI, lazily loads
   `com.aoko.helper.SessionSwitcher`, calls `AccountCommandRouter::Handle`, and pushes the
   reply.
5. The reply waits in a `std::vector<std::string>` FIFO capped at
   `AccountCommandRouter::kPendingReplyLimit = 64`; at the cap the **oldest** entry is dropped.
6. Every loop iteration the bridge peeks, sends, and only then pops — so a backpressured socket
   delays a reply instead of dropping it.
7. `ClearReplies()` runs on accept and on terminal socket failure.

`autoRod` / `throwPot` never use this path; their acknowledgement is the
`lastActionId`/`lastActionResult` state pair, plus a 1500 ms host-side diagnostic window before
logging that an action went unacknowledged.

---

## Source index

Line ranges are from the `rust` branch working tree.

### Native bridges (`McInjector/src/main/cpp/`)

| File | Lines | Used for |
| --- | --- | --- |
| `bridge_capabilities.h` | 1-25 | `LC_KILLAURA_SETTINGS`, `LegacyCapabilitiesJson`, `ModernCapabilitiesJson` |
| `hud_layout.h` | 1-186 | Element ids, defaults, clamps, `ParseHudLayout`, `SerializeHudLayout`, `HudElementPixelPos` |
| `json_config_reader.h` | 1-102 | `SimpleJsonConfigReader` semantics, `ClampInt`, `ClampFloat` |
| `bounded_newline_buffer.h` | 1-77 | Framing, CR stripping, over-long line discard, CR-at-cap quirk |
| `telemetry_schedule.h` | 1-125 | `kStateReadIntervalMs`, `kEntityPublishIntervalMs`, `kModernState*IntervalMs`, `ModernFullStateDue` |
| `module_status.h` | 1-68 | `moduleStatus` JSON shape and escaping |
| `auto_rod_core.h` | 5-11 | `kHotbarSlots`, `kUseToRestoreTicks`, `kMinExtensionTicks`, `kMaxExtensionTicks` |
| `jni_core/account_command_router.h` | 1-432 | `CommandResult`, `Serialize`, reply FIFO, `kPendingReplyLimit`, `kCommandLineLimit`, `kCommandHeadProbe`, `InboundCommandTracker`, `Handle` and the per-action handlers |
| `bridge.cpp` | 640-670 | `SetLastActionResult`, `ResetLastActionState`, `LastActionJson` |
| `bridge.cpp` | 1541-1555 | `JsonEscape` (legacy escaping rules) |
| `bridge.cpp` | 8685-8698 | `ModuleStatusJsonLegacy` |
| `bridge.cpp` | 9460-9905 | Chest stealer / refill slot JSON (`windowId`, `screenWidth`, `slotNumber`, `x`, `y`) |
| `bridge.cpp` | 10160-10176 | Refill state JSON (no `physical`) |
| `bridge.cpp` | 14131-14136, 14430-14563 | Entity JSON cap (20) and `{sx,sy,dist,name,hp}` construction |
| `bridge.cpp` | 17297-17320 | `HandleAccountCommand` |
| `bridge.cpp` | 17322-17728 | `ParseConfig` legacy: keys, clamps, defaults |
| `bridge.cpp` | 17730-17784 | `TrySendCapabilities`, `SendJsonLine` |
| `bridge.cpp` | 17786-17810 | Server loop: WSA, `SO_REUSEADDR`, loopback bind, `listen(..., 1)`, `setlocale` |
| `bridge.cpp` | 17815-17900 | Accept loop, `BoundedNewlineBuffer(1024 * 1024)`, `InboundCommandTracker`, capabilities-before-state rule |
| `bridge.cpp` | 18062-18145 | Legacy state JSON: key order, numeric formatting, conditional `hudLayout` |
| `bridge.cpp` | 18154-18205 | `recv` config path and reply drain |
| `bridge_261.cpp` | 409-418 | `HandleAccountCommand` forward declaration |
| `bridge_261.cpp` | 419-762 | `ParseConfig` modern: keys, clamps, defaults |
| `bridge_261.cpp` | 763-792 | `TrySendCapabilities` (modern variant) |
| `bridge_261.cpp` | 793-820 | `SendJsonLine121` |
| `bridge_261.cpp` | 1126-1133 | `LastActionJson` |
| `bridge_261.cpp` | 14095-14122 | `HandleAccountCommand` |
| `bridge_261.cpp` | 15541 | `BuildModernRefillStateJson` call site |
| `bridge_261.cpp` | 18194-18206 | `ModuleStatusJson121` |
| `bridge_261.cpp` | 21462-21500 | Server loop: bind, `listen(..., 1)`, accept, per-connection `supportsStatePatches` reset |
| `bridge_261.cpp` | 21505-21560 | Fast/normal interval selection, full-state cadence, capabilities-before-state rule |
| `bridge_261.cpp` | 21636-21836 | Modern state JSON: `type`, `partial`, key order, conditional blocks, `hudLayout` |
| `bridge_261.cpp` | 21840-21875 | `recv` config path and reply drain |

### Managed host (`Aoko/Core/`)

| File | Lines | Used for |
| --- | --- | --- |
| `GameStateClient.cs` | 29, 36-45 | Port, cadence constants, `MaximumInboundMessageCharacters`, `SendTimeout` |
| `GameStateClient.cs` | 255-410 | Injection flow, attempt budgets, `ResolveBridgeDllName` |
| `GameStateClient.cs` | 449-505 | `ConnectAsync`, 500 ms retry delay |
| `GameStateClient.cs` | 511-622 | `ReadLoop`: `BoundedLineReader`, 1 MiB cap, type classification, patch merge, inbound `hudLayout`, action-bar gate |
| `GameStateClient.cs` | 626-662 | `ScheduleAutoReconnectAsync` (500 ms then 1000 ms) |
| `GameStateClient.cs` | 760-782 | Version normalisation and DLL selection |
| `GameStateClient.cs` | 935-1030 | `ModuleListStyleToIndex`, `moduleAction` builders, `_nextActionRequestId` |
| `GameStateClient.cs` | 1100-1128 | `RestoreSessionAsync`, `JoinServerAsync` host-side validation |
| `GameStateClient.cs` | 1156-1200, 1236-1252 | `SendMessageAsync`, `FailTornWrite` |
| `GameStateClient.cs` | 1266-1580 | `ConfigSenderLoop`: heartbeat, coalesce, the full `config` object, `IsConfigSendDue`, `ShouldSerializeConfig` |
| `GameStateClient.cs` | 1620-1648 | `ApplyInboundHudLayout` echo guard |
| `BoundedLineReader.cs` | 1-129 | CR stripping, character cap, CR-at-cap quirk, BOM detection |
| `BridgeCapabilities.cs` | 1-590 | Payload parse, normalisation, `ForVersionFallback`, `Supports*` gates |
| `BridgeCommandProtocol.cs` | 1-189 | `BridgeActions`, `AccountCommand`, `BridgeRequestIdSource`, `BridgeCommandWire`, `BridgeInboundClassifier` |
| `BridgeCommandDispatcher.cs` | 1-120 | 20 s timeout, pending registry, `FailAll`, `TryComplete` |
| `BridgeCommandResult.cs` | 1-204 | `commandResult` decode, error vocabulary, user-facing mapping |
| `GameState.cs` | 1-233 | `GameState`, `ChestStealerState`, `ChestStealerSlot`, `RefillState`, `EntityInfo` property names |
| `GameStatePatchMerger.cs` | 1-180 | `statePatch`/`partial` detection, merge rules, world-exit scrub |
| `HudLayout.cs` | 1-269 | Element ids, defaults, clamps, `ToJson`, `FromJson`, `EqualsLayout` |
| `Profile.cs` | 10-275 | `Profile` properties and defaults |
| `Profile.cs` | 277-295 | `ProfileManager.JsonOptions` (camelCase, indented) |
| `Profile.cs` | 445-505 | `SaveProfile`, `LoadProfile` tolerance |
| `Profile.cs` | 512-554 | `MigrateSilentAuraToKillAura`, `MigrateFlatKillAuraSettings` |
| `Profile.cs` | 906-977 | `BuildBlockEspTargetData`, `BuildHudLayoutDict`, `BuildHudLayout` |
| `KillAuraSettings.cs` | 12-95 | 60 nested properties, defaults, setter clamps |
| `CustomPalette.cs` | 1-109 | `palettes.json` schema and store |
| `ClickRandomizationMode.cs` | 1-10 | Enum order (`Basic = 0`, `Medium = 1`, `Advanced = 2`) |
| `TcpPortHelper.cs` | 1-52 | `netstat` probe, regex, PID extraction |
