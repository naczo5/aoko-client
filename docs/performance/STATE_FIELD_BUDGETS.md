# GameState field budgets

This is the compatibility and scheduling contract for the V1 `state` message
received by `GameStateClient`.  The bridge still sends one newline-delimited
JSON object, so these budgets describe when a field needs to be refreshed; they
do not introduce a second wire format.  A producer may send a field less often
when its consumer is disabled, but must clear the published value on a world,
mapping, or feature transition where a stale value could cause input.

The age numbers are starting safety budgets, not benchmark results.  They are
measured at the loader when a state is consumed (`stateMs` is the bridge's
source timestamp).  A later benchmark may relax a UI-only budget, but must not
relax an input-critical budget without a characterization test and a live
smoke result.

## Scheduling classes

| Class | Maximum useful age | Initial producer cadence | Consumers |
|---|---:|---:|---|
| Input-critical | 25 ms while active; 100 ms while inactive | 5–10 ms while active | Triggerbot, Aim Assist, mining intent, Auto Rod guards |
| Combat snapshot | 50 ms while active; clear on disable | 20–50 ms | entity targets, attack cooldown, Kill Aura status |
| UI/visual | 250 ms | 100–200 ms | WPF status, Discord presence, HUD/ESP |
| Transition/debug | 1 s, or immediately on transition | event/heartbeat | mapping state, screen name, diagnostics |

The managed client always publishes the newest complete `GameState` object to
input loops immediately.  UI notifications are coalesced separately.  The
current `GameStatePatchMerger` only accepts an explicitly marked `statePatch`
or `partial:true` object; fields omitted from that patch retain the previous
complete snapshot, while fields present in the patch replace their previous
value.  New safety-critical fields should be added to that merge contract only
with a characterization test; an unmarked V1 `state` message is always a
complete snapshot and must not be merged.

## Top-level fields

| JSON field | Type/default | Class and age | Consumers and contract |
|---|---|---|---|
| `mapped` | bool / `false` | Transition; 1 s | Bridge readiness and mapping recovery. False on invalidation; never infer true from a socket connection. |
| `guiOpen` | bool / `false` | Input/UI; 25–100 ms | Blocks input while a game screen is open and drives presence. |
| `inWorld` | bool / `false` | Input-critical; 25 ms | Auto Rod and all input loops. Clear feature-owned state immediately on false. |
| `screenName` | string / `"unknown"` | Transition/UI; 250 ms | Chest GUI detection, menu status, Discord. Unknown is safe. |
| `actionBar` | string / `""` | UI; 250 ms | GTB hint extraction and Discord. Do not use as an input guard. |
| `health` | float / `-1` | UI; 250 ms | Player health display and presence. Negative means unavailable. |
| `fov` | float / `70` | Combat; 50 ms (10 ms during aim) | Aim Assist projection fallback. Clamp to a finite positive value before use. |
| `viewportWidth`, `viewportHeight` | int / `0` | Combat; 50 ms (10 ms during aim) | Screen projection and crosshair fallback. Non-positive values use the current window size. |
| `holdingBlock` | bool / `false` | Input-critical; 25 ms | Click-in-chests/mining guards. Both bridge families advertise and publish it; callers still retain the safe default for older payloads. |
| `lookingAtBlock` | bool / `false` | Input-critical; 25 ms | Mining intent and Break Blocks. False on stale/invalid state. |
| `lookingAtEntity` | bool / `false` | Input-critical; 25 ms | Triggerbot/Kill Aura crosshair guard. |
| `lookingAtEntityLatched` | bool / `false` | Input-critical; 25 ms | Short (12 ms) entity-hit grace window. Must be cleared on world exit. |
| `breakingBlock` | bool / `false` | Input-critical; 25 ms | Break Blocks guard. |
| `attackCooldown` | float / `1` | Input-critical; 25 ms | Triggerbot/Kill Aura click guard. Clamp to `[0,1]`; non-finite values are invalid. |
| `attackCooldownPerTick` | float / `0.08` | Combat; 50 ms | Cooldown normalization. Use a positive finite fallback when absent. |
| `killAuraUnavailableReason` | string / `""` | Transition/UI; 1 s | Dev-mode diagnostics only; never blocks input by itself. |
| `killAuraHasTarget` | bool / `false` | Combat; 50 ms | Managed input hook decides whether a native Kill Aura target is active. False is safe. |
| `killAuraBlocking` | bool / `false` | Combat; 50 ms | Managed input hook preserves block state while Kill Aura owns it. Clear on disable. |
| `stateMs` | unsigned integer / `0` | Input-critical; checked every read | Bridge source time used to reject stale snapshots. `0` means unknown; consumers apply their own connection/freshness guard. |
| `posX`, `posY`, `posZ` | double / `0` | UI/debug; 250 ms | Legacy position telemetry and diagnostics. Not currently required by managed input. |
| `pitch` | float / `0` | UI/visual; 250 ms | Legacy camera telemetry. Not an input source in the current managed client. |
| `entities` | array / `[]` | Combat; 25 ms while Aim Assist/Triggerbot, 100 ms visual-only | Target selection, crosshair fallback, Nametags. Modern bridge bounds the wire array to 32 entities; an empty array is a valid complete snapshot. |
| `chestStealerState` | object/null | Input/UI; 100 ms while Chest Stealer | Chest slot clicking and Chest GUI overlay. Null/`ready:false` means no actionable chest. |
| `pixelPartyTargetFound` | bool / `false` | Input-critical while Pixel Party; 50 ms | Managed Pixel Party loop. False clears the target grace timer. |
| `pixelPartyTargetYaw` | float / `0` | Input-critical while Pixel Party; 50 ms | Pixel Party camera steering; use only when `TargetFound` is true. |
| `pixelPartyTargetDist` | float / `-1` | Input-critical while Pixel Party; 50 ms | Pixel Party walk/target guard; negative means unavailable. |
| `pixelPartyYawDelta` | float / `0` | Input-critical while Pixel Party; 50 ms | Managed steering delta; normalized by the bridge to `[-180,180]`. |

`hudLayout` is an optional transition payload rather than a continuously
updated `GameState` property.  The loader applies it only when present and
uses an equality check to prevent a config echo loop.

## Nested and entity fields

### `EntityInfo`

| Field | Use | Budget/limit |
|---|---|---|
| `sx`, `sy` | Prepared screen position for Aim Assist, Triggerbot, and Nametags | Same age as `entities`; `-1` means not projected. |
| `dist` | Target ordering and range checks | Same age as `entities`; finite, non-negative values only. |
| `name`, `hp` | Nametags/visuals | Same age as `entities`; names are escaped by the bridge and must not be logged by diagnostics. |
| `x`, `y`, `z` | Legacy/debug world coordinates | Optional; UI/debug only and may be absent on modern. |

The renderer consumes a prepared producer snapshot and must not perform JNI
lookups or hold the producer mutex while drawing.  Entity arrays are bounded
before serialization; a deferred/incomplete producer snapshot must be marked
internally rather than represented as a stale non-empty array.

### `ChestStealerState`

`ready`, `physical`, `windowId`, `screenWidth`, and `screenHeight` are consumed
as one snapshot.  `slots[]` contains the screen coordinates used by the managed
controller.  The bridge should cap slot count to the active container's valid
range (currently a normal chest is at most 54 storage slots, with a small
allowance for player inventory); both bridges cap the published slot list at 90.
The managed mapper treats the reported dimensions as advisory, rejects slot
coordinates outside that source rectangle, and maps valid points into the
client rectangle before the controlled click jitter is applied.

## Bridge-family differences

Both bridges use the same property names and tolerate unknown fields.  The
following differences are intentional and must stay documented:

| Family | Additional/omitted fields | Compatibility rule |
|---|---|---|
| 1.8.9 `bridge.dll` | Includes `mapped`, `posX/Y/Z`, and `pitch`; entity telemetry may include world coordinates. `holdingBlock` is shared across both bridge families. | Loader fallback for 1.8.9 advertises these fields. Keep menu-to-world mapping recovery and Reach edge handling unchanged. |
| 1.21.x / Lunar 26.x `bridge_261.dll` | Uses prepared camera/entity projections and Vulkan or OpenGL rendering; it additionally publishes FOV/viewport fields for projection consumers. | Loader treats absent optional fields as defaults and uses capability checks before consuming version-specific values. |

Unknown top-level fields and unknown capability entries are ignored.  A newer
loader may therefore add optional settings without breaking an older bridge;
an older loader simply ignores those settings when it receives a newer
capability/state payload.  Any required field addition must first be covered by
a fixture in `Aoko.Tests/ProtocolCompatibilityTests.cs` and a fallback test.

## Measurement notes

Record actual age distributions and payload sizes under
`docs/performance/runs/` using the opt-in `AOKO_PERF_DIAGNOSTICS=1` mode.  The
budget table is a contract for scheduling and safety, not evidence that a
target has been met.  A phase is complete only after the corresponding live
runtime smoke result and automated tests are recorded in the main plan.
