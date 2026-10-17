# Adapter map boundary: REM plugin map API 1.3

## Verified owners

Read-only source inspection on 2026-10-17 of REM `91124cffd8f7fa7df94684b672c2228d97f0522d`
(`docs/plugins/android-plugin-system.md`, `crates/reticulum_mobile/src/jni_bridge/plugin_map_api.rs`,
`plugin_map_session_api.rs`, `app_state/plugin_map_sessions.rs`, `packages/android-plugin-sdk`):

- REM implements plugin API minor 2 (`mapMarkersRead`, `mapMarkersWrite`, `mapObserverRead`) and
  minor 3 (`mapSelectionRead`, operator-selected sessions). Within API major 1 fields and
  operations are only added. Map objects are ordinary shared REM markers handled by the same
  native marker service (`r3akt-map-core`, RCH `19de27f419373c543fe6154aab2a3c027938b65d`) as the
  REM UI and replicated to peers and RCH. Plugins get no second marker store and cannot browse the map.
- REM negotiates the API minor at bind time: `start()` receives `apiMinor`, the lower of host and
  plugin. A plugin declaring a newer minor than the host is **Incompatible and never bound**, so an
  older REM shows the incompatibility itself; this plugin cannot report it.
- Binder requests, responses and events use `protocolVersion`, `requestId`, `operation`,
  `payload`, `ok`, `result`/`error{code,message}` and `event`, each at most 64 KiB UTF-8.

The adapter declares API 1.3 and the four map capabilities. REM owns marker identity, storage,
replication, the observer fix and session membership. The adapter never mints
`object_destination_hash` or `sessionId`, never impersonates operator telemetry, and never calls
RCH directly. Both checkouts were inspected without editing them.

## Operations used

| Operation | Payload | Result | Adapter use |
|---|---|---|---|
| `map.symbols.list` | `{}` | `{items:[{id,set,mdi?,description,tak?,category,aliases}]}` | once per host start; the configured `hostMarker.symbol` must be an `id` or alias, otherwise the adapter stays read-only |
| `map.markers.create` | `idempotencyKey`, `symbol`, `lat`, `lon`, optional `name`, `category`, `notes` | `{marker, created}` | one shared marker per eligible observation |
| `map.observer.snapshot` | `{}` | `{available, observer{kind:"localDevice",…}, source:"device", datum:"WGS84", lat, lon, alt, horizontalAccuracyM, course, speed, updatedAtMs, ageMs}` or `{available:false, observer}` | per observation, at session start, and refreshed while a session is active |
| `map.session.list` | `{}` | `{items:[session]}` | after the symbol check, on co-location confirmation; recovers a dropped `started` event |
| `map.session.objects.get` | `sessionId` | `{session, objects}` | authoritative snapshot of a listed session; omitted objects leave the selection |
| `map.session.stop` | `sessionId` | `{stopped:true}` | rejecting a session the adapter cannot serve; ending a session on disconnect, stop, profile change or local failure |

Errors are typed: `PermissionDenied` (minor too old, capability not declared/granted, plugin
disabled) makes the adapter read-only; `InvalidRequest` is recorded against the observation and
never retried; `NotFound` for a session ends the local session; `HostError`, Binder failures and
timeouts leave a marker creation **unknown**. `map.markers.update/position.update/delete/get/list`
are not used in this release.

## Markers created from observations

`idempotencyKey` is the connection-scoped observation identity (`<connectionId>/<sequence>`;
the connection ID is a fresh UUID per BLE connection, never a hardware address). REM scopes the key
to the plugin and answers a replay with the original marker and `created:false`, so the host can
never mint a second marker for one observation. Each decoded measurement is its own observation,
even when values repeat.

Creation requires a verified device profile, verified semantics, an eligible observer fix and the
operator's connection-scoped co-location confirmation. `lat`/`lon` are two-dimensional WGS84 from
the Vincenty direct solution; REM markers carry no altitude and REM's `alt` has no stated vertical
datum, so neither is used. `name` is the configured prefix plus the observation sequence (≤ 128
characters). `notes` (≤ 1024 characters, bounded before sending) carries raw values and configured
units, measurement time, horizontal distance and true bearing, observer position/accuracy/time,
profile and semantics evidence, north reference, co-location evidence and projection method. No
ATN hardware identifier and no REM identity hex is included.

Outcomes per observation are shown in native status as `retained {reason}`, `observerRequested`,
`createRequested`, `created {hash}`, `alreadyExisted {hash}`, `rejected {code}` or
`unknown {reason}`. An unknown outcome is never retried automatically and nothing survives
reconnect or process restart.

## Observer

REM's snapshot is this phone's own latest fix: `source` must be `device`, `datum` must be `WGS84`,
`horizontalAccuracyM` must be present and within the configured maximum, `updatedAtMs` must be
within the configured observer age. `alt`, `course` and `speed` are ignored. REM cannot know the
phone is at the binocular; the operator confirms that for the current connection
(`confirmColocation` with evidence text), and the confirmation is cleared on disconnect, host loss,
configuration change or withdrawal. Without it the fix is `unverified` and conversion and display
writes are blocked. Older fixes never replace newer ones; conflicting same-time fixes are rejected.

## Selected-object sessions

Only the operator starts a session, from a marker's details sheet (**Share with ATN Rangefinder**).
A session holds 1–16 objects; a plugin has at most 4 active sessions. Events arrive through
`onHostEvent` as `{"protocolVersion":1,"event":…,"payload":…}`:

- `map.session.started`: session fields plus `objects` snapshots.
- `map.session.object.updated`: `sessionId`, `reason`, `object`.
- `map.session.object.removed`: `sessionId`, `objectDestinationHash`.
- `map.session.stopped`: `sessionId`, `reason` (`operator`, `capability-revoked`, `plugin-disabled`,
  `plugin-unavailable`, `objects-removed`). No event follows the plugin's own `map.session.stop`.

Delivery is best effort; sessions end without an event on `stop()`, unbind, Binder death, node
stop or REM restart and are never restored. The adapter therefore also lists sessions and treats
every unknown or previously ended `sessionId` as terminal (bounded set, never reused).

This release serves **one** display session at a time. A session is accepted only when the host
is ready, the device is connected, the profile is verified and co-location is confirmed, and
when the selection fits the configured ordered `slotPool`; slots are allocated in REM's
`objectDestinationHashes` order. Otherwise the whole session is rejected, nothing is allocated,
the reason is shown in status and `map.session.stop` is sent once. Serving several concurrent
sessions from disjoint slot allocations is planned (Stage 3 of
`docs/goals/atn-rem-plugin/plan.md`).

A map object uses the RCH representation REM emits: `object_destination_hash`, `origin_rch`,
`type`, `name`, `category`, `symbol`, `notes`, `position:{lat,lon}`, `time`, `stale_at`,
`created_at`, `updated_at` (RFC 3339) plus `textTruncated`. The adapter keys on
`object_destination_hash`, refuses an object older than the one it holds by `updated_at`, and
compares same-time objects by content. `textTruncated` is provenance only. Unknown fields are
tolerated because the contract only adds fields.

Relative device fields (distance, pitch, compass) are recomputed from the latest eligible observer
fix for every write. A stale or missing fix holds writes and requests a fresh snapshot rather than
sending old geometry; the lifecycle tick refreshes the fix at half the configured maximum age.
Removing an object or stopping a session releases its slot without a device clear command.

## Native/platform contract

`org.freetak.atn.NativeCore.dispatch(String)` accepts JSON bounded to 393,216 UTF-8 bytes
`{cmd, nowMs, ...}`; Binder envelopes remain capped at 65,536 bytes. It returns
`{ok, result?, error?, actions}`. Android supplies the time, runs calls on one handler, and owns
real GATT/Binder completion. Native commands: configure, hostStarted (`apiMinor` from the start
session JSON), hostStopped, connected/disconnected, confirmColocation/clearColocation,
notification, hostResponse/hostEvent, stopSync, setStyle, setIcon, transportResult,
confirmDisplay, tick, status.

Native actions: `hostRequest` (envelope), `cancelHostRequests` (`keepRequestIds`),
`sessionAuthorized` (`sessionId`, `connectionId`: Java admits writes only for that pair),
`write` (numeric `operationId`, one-based `frameNumber`, `totalFrames`, `sessionId`,
`connectionId`, `bytes`), `cancelWrites`, `disconnect`, `status`. Platform completion must echo
operationId/frameNumber/sessionId/connectionId and `completed`. An old callback cannot advance a
new authorization. Only one frame is issued at a time. In-flight cancellation closes GATT; a fresh
connection is explicit.

No session, slot allocation, co-location confirmation, queued write or marker cache resumes across
reconnect or process restart. Configuration and an explicitly picked device may be saved privately;
this is not authorization to reconnect or write.
