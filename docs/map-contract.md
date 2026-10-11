# Adapter map boundary and RCH compatibility

## Verified owners and current gap

Read-only source inspection on 2026-10-10:

- REM `ec6aadc81b3c18e2353211e97225a89d73af84dc`: the real Android SDK is API 1.1; Binder requests/responses use `protocolVersion`, `requestId`, `operation`, `payload`, `ok`, `result`/`error`. Host events use **`event`**, not `eventType`. The exhaustive Rust host operation switch still contains only `sensor.publish`, `lxmf.send`, `events.publish`, `notifications.raise`, and `operational.snapshot`.
- RCH `2a599275ee36ea809dbd5ff773ebda6bf9d2a8d1`: `crates/r3akt-rch-core/src/lib.rs::MarkerRecord` and the server mirror own marker identity, storage and normalization. HTTP routes include `/api/markers` and marker position updates. `marker_idempotency.rs` binds a creation request to its validated payload and leaves identity minting in the host.

The adapter follows RCH's record shape and host ownership. It does not create a separate RCH server, call its REST API, introduce a network transport, mint host marker identities, or impersonate operator telemetry. Both checkouts were inspected without editing them.

**The map operations below are a proposed REM extension, not an existing REM API.** The shipped adapter negotiates them; an unmodified REM host rejects the negotiation and the adapter retains raw observations in read-only mode. Synthetic test hosts exercise the contract but do not prove real REM map integration.

## Proposed version 1

Every operation travels through the real trusted Binder SDK envelope and its 65,536-byte UTF-8 size limit. The host must enforce publisher trust, plugin enablement, declared/granted map capabilities, observer ownership, and the selected-session boundary. Listing an operation in negotiation means it is available to this particular trusted plugin session, not merely compiled into the host. Future host capability declarations and the SDK API version increment belong to REM; this adapter's current manifest does not invent grants.

| Operation | Payload | Result |
|---|---|---|
| `map.adapter.describe` | `contractVersion: 1` | `contractVersion: 1`, `operations` including all six rows |
| `map.observer.get` | empty object | `observer`: explicit resolved measuring-device fix |
| `map.ple.get` | `pleId` | `marker`: RCH-compatible marker record |
| `map.ple.subscribe` | `sessionId`, exact `pleIds` | `accepted: true` |
| `map.ple.unsubscribe` | `sessionId` | `accepted: true` |
| `map.observation.draft.put` | `observationId`, `draft` with full provenance | `accepted: true` after the host retains the draft |

`draft.put` is a draft observation operation, not confirmation, replication or a claim of GNSS/survey accuracy. Host-owned confirmation and map projection must retain the observation and its reference evidence. Creation should be idempotent by observationId plus a canonical validated payload; a conflicting reuse must fail, following RCH's payload-bound creation principle. The adapter sends once and does not blindly retry.

Subscriptions must be replaced atomically on a new session from the same plugin, bounded by the requested selected set, removed by unsubscribe, and released on plugin stop/process death or host restart. The host must not replay stale updates into a new subscription. No selection/list API or automatic expansion is implied. Selection IDs and device slots are explicitly entered by the operator in the adapter's settings activity.

## Records and events

RCH marker fields are snake_case, matching the inspected source exactly:

```text
local_id, object_destination_hash, origin_rch, marker_type, symbol,
name, category, lat, lon, notes, created_ts_ms, updated_ts_ms
```

The adapter uses `object_destination_hash` as the PLE identity. The wall-clock `updated_ts_ms` is retained as provenance, not a guaranteed monotonic concurrency token. Older timestamps are refused; same-time changes can be legitimate and are compared by encoded marker content. Strong replay ordering would require a host-owned revision/event contract.

Observer JSON uses `position {latitude, longitude, altitudeM}`, `capturedAtMs`, `horizontalAccuracyM`, `source` (`devicePosition`, `verifiedColocated`, `verifiedOffsetResolved`, `unverified`), and `evidenceId`. Altitudes, when used, must be WGS84 ellipsoidal metres. The host must resolve co-location/offset evidence before returning an eligible fix; its latest unrelated telemetry record is unsuitable. Unknown source, stale/future time and excessive uncertainty block conversion. Older fixes cannot replace newer ones; conflicting same-time fixes are rejected.

Events use SDK envelope `{"protocolVersion":1,"event":"...","payload":{...}}`:

- `map.ple.updated`: `sessionId`, `marker`.
- `map.ple.removed`: `sessionId`, `pleId`; releases authorization without a device clear command.
- `map.observer.updated`: `sessionId`, `observer`.

The adapter ignores old-session/unselected events. It serializes writes, rechecks observer eligibility before each frame, records opaque replies independently, and requires explicit evidence for observed display confirmation.

## Native/platform contract

`org.freetak.atn.NativeCore.dispatch(String)` accepts JSON bounded to 393,216 UTF-8 bytes `{cmd, nowMs, ...}` (enough for a u16-sized PNG byte array); Binder envelopes remain capped at 65,536 bytes. It returns `{ok, result?, error?, actions}`. Android supplies the time, runs calls on one handler, and owns real GATT/Binder completion. Native commands include configure, hostStarted/Stopped, connected/disconnected, notification, hostResponse/Event, startSync/stopSync, setStyle, setIcon, transportResult, confirmDisplay, tick, status.

A native write action includes numeric operationId, one-based frameNumber, totalFrames, sessionId, connectionId, and bytes. Platform completion must echo operationId/frameNumber/sessionId/connectionId and completed. An old callback cannot advance a new authorization. Only one frame is issued at a time. A reply before the final transport callback is retained, but cannot establish transport completion. In-flight cancellation closes GATT; a fresh connection is explicit.

No session, selected-set authorization, queued write or marker cache resumes across reconnect/process restart. Configuration and an explicitly picked device may be saved privately; this is not authorization to reconnect or write.
