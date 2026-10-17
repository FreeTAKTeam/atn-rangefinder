# ATN REM plugin on REM map API 1.3 (reviewed plan)

Reviewed 2026-10-17 against REM `91124cffd8f7fa7df94684b672c2228d97f0522d`
(`docs/plugins/android-plugin-system.md`, `crates/reticulum_mobile/src/jni_bridge/plugin_map_api.rs`,
`plugin_map_session_api.rs`, `app_state/plugin_map_sessions.rs`) and this repository's current
`crates/atn-adapter`, `crates/atn-domain`, `android/` and `docs/`.

## Outcome and boundaries

Deliver bidirectional ATN integration on REM's implemented map API (API major 1, minor 3):

- An explicitly started capture session creates one ordinary shared REM marker per eligible ATN
  measurement through `map.markers.create`.
- REM's operator-initiated **Share with ATN Rangefinder** session (`mapSelectionRead`) is the
  write authorization for displaying selected objects on the binocular, including later updates.
- ATN BLE operates alongside REM's RNode BLE or Bluetooth Classic link.
- A future goTenna plugin is a separate APK/Binder service and Rust core. This delivery only
  keeps ATN compatible with it; it designs nothing goTenna-specific beyond the constraints listed.

Rust keeps codecs, geometry, physical-semantic validation and synchronization authorization.
Android keeps Bluetooth, permissions, the trusted Binder boundary and scheduling. REM owns marker
identity, storage, replication, the observer fix and session membership. No REM, LXMF or
network-core change is required or made.

Contract facts this plan depends on (verified in REM source):

| Fact | Value |
|---|---|
| Session objects / sessions per plugin | 1–16 objects, at most 4 active sessions |
| Plugin payload text limits | `name` 128, `notes` 1024, other text 64 chars; `textTruncated` flags truncation |
| Envelope limit | 64 KiB UTF-8 JSON per request, response and event |
| Observer snapshot | `{available, observer{kind:"localDevice",identityHex,lxmfDestinationHex}, source:"device", datum:"WGS84", lat, lon, alt, horizontalAccuracyM, course, speed, updatedAtMs, ageMs}`; `alt` has no stated vertical datum |
| Marker object | RCH shape with `type`, `position:{lat,lon}`, `time`, `stale_at`, `created_at`, `updated_at` |
| `map.markers.create` | `idempotencyKey` required, plugin-scoped; replay returns the original marker with `created:false` |
| `map.session.stop` | returns `{stopped:true}`, no `map.session.stopped` event follows |
| Session end without event | `stop()`, unbind, Binder death, node stop, REM restart; sessions are never restored |
| Event delivery | best effort, backlog 256; `map.session.started` itself can be dropped |
| Version negotiation | a plugin declaring minor 3 on an older host is **Incompatible and never bound** |
| ATN display frame | command 2 carries slot (7 bits), type, colour, distance, pitch, compass relative to the observer, not lat/lon |

## Stage 1: migrate the host integration

- Build and pin the external `rem-plugin-sdk` AAR from REM `91124cf…` by commit and SHA-256, as
  today. No SDK source or binary is vendored.
- Manifest: `rem.plugin.apiMinor` = 3; declare `mapMarkersWrite`, `mapObserverRead`,
  `mapSelectionRead`, `mapMarkersRead` (needed to resolve indeterminate creates) and
  `notifications.raise` (operator-visible rejection reasons). Keep the existing
  `configurationEntrypoint`; decide whether the REM-hosted configuration page replaces or launches
  the current `SettingsActivity` through `actionResult.activity` (recommended: launch, keep the
  activity as the single validation path).
- Because declaring minor 3 means REM never binds on an older host, the plugin cannot "report"
  incompatibility itself. REM shows **Incompatible**; the plugin's settings activity shows
  "host not bound" with the required host version. Do not declare a lower minor and probe.
- Replace the proposed `map.adapter.describe`, `map.observer.get`, `map.ple.*` and
  `map.observation.draft.put` operations and the `map.ple.updated/removed`,
  `map.observer.updated` events in `crates/atn-adapter/src/runtime.rs` and `runtime/host.rs` with
  `map.symbols.list`, `map.markers.create`, `map.markers.list`, `map.observer.snapshot`,
  `map.session.list`, `map.session.objects.get`, `map.session.stop` and the four
  `map.session.*` events. Remove the capability negotiation handshake; API minor comes from the
  `start()` session JSON.
- Rework the RCH-compatible record decoder for the nested `position`, the `type` field and the
  `time/stale_at/created_at/updated_at` timestamps. Keep the current rule: older `updated_at` is
  refused, equal timestamps are compared by content. Honour `textTruncated` as provenance, never
  as a reason to rewrite the marker.
- Preserve host identities as returned. Never mint `object_destination_hash` or `sessionId`.
- Rewrite `docs/map-contract.md` from "proposed extension" to the implemented API, and update
  `README.md`, `docs/configuration.md`, `docs/execution.md` and `docs/implementation-status.md`
  accordingly. The AGENTS.md rule "do not invent an implemented REM map API" stays.
- Update the synthetic host used by `atn-adapter` tests to the real envelopes, error codes
  (`PermissionDenied`, `InvalidRequest`, `NotFound`, `ResponseTooLarge`, `HostError`) and size
  limit; model REM's `map-fixture` reference consumer. Leave the browser console, `atn-host-service`
  and other untracked local work untouched; commit them separately or not at all, never as part of
  the migration commit.

## Stage 2: automatic measurement capture (inbound)

- Add **Start capture** / **Stop capture** in the plugin UI. Start requires a validated,
  verification-flagged profile, an active ATN connection and a fresh operator confirmation that the
  phone is at the binocular. The confirmation is session-scoped and never persisted.
- For each decoded measurement from the current connection generation and the measurement
  characteristic, create a separate observation even when values repeat. Replies and unknown
  frames never create markers. Observation identity = `{connectionGeneration, sequence}`, assigned
  in Rust; it is also the `idempotencyKey` (prefix with a per-process capture-session nonce so a
  restarted process can never collide with an earlier key).
- Fetch `map.observer.snapshot` per observation. Block conversion when `available` is false,
  `ageMs` exceeds the configured maximum, `horizontalAccuracyM` is absent or above the configured
  limit, `datum` is not `WGS84`, or `source` is not `device`. Ignore `course` and `speed`. Treat
  `alt` as informational only: placement is two-dimensional WGS84 and the adapter never claims an
  ellipsoidal height. Raw observations are retained in the bounded in-memory history regardless.
- Marker payload: configured catalog symbol validated against `map.symbols.list` at capture start,
  configured `category`/`markerType`, a bounded `name`, and `notes` built to fit 1024 characters
  **before** sending, so REM never truncates them: measured values and units as configured,
  measurement time, observer lat/lon/accuracy/age, profile identity and verification evidence
  reference, declination source, co-location confirmation time. Exclude ATN device identifiers and
  the REM `identityHex`/`lxmfDestinationHex`.
- Outcomes are kept distinct: `ok:true` created, `ok:true created:false` (already existed),
  `ok:false` typed error (no retry on `PermissionDenied`/`InvalidRequest`), Binder exception,
  timeout, Binder death. Timeout and `HostError` become **result unknown**. Resolution of result
  unknown is one bounded replay of the same `idempotencyKey` while the same Binder session is
  still bound, which by contract returns the original marker or creates it once; after that bound,
  or on rebind, it stays unknown. This is not write replay on reconnect: nothing is queued across
  rebind or process restart, and nothing is persisted.
- Capture authorization ends on ATN disconnect, host unbind/death, profile change or capability
  revocation. Restarting capture never publishes historical observations.
- Outbound display writes must not be re-read as measurements: verify on hardware that command-2
  writes do not provoke measurement-like notifications; in software, measurement decoding is bound
  to the notification characteristic and to the connection generation, never to a reply.

## Stage 3: selected-object display synchronization (outbound)

- Authorization is the REM session. No second confirmation when the connection, profile and the
  session-scoped co-location confirmation are current; the co-location confirmation is required
  because command 2 is observer-relative (distance/pitch/compass), so every frame is recomputed from
  a fresh `map.observer.snapshot` under the same gates as Stage 2.
- Discover sessions by both paths: `map.session.started` events **and** `map.session.list` on bind,
  on every ATN connect, and on the configured reconciliation interval, because events are best
  effort and the `started` event itself may be dropped.
- Slot pool: an explicitly configured, verified, ordered list of slot IDs within 0–127. Support up
  to REM's 4 sessions × 16 objects; allocate a new session atomically in the host's
  `objectDestinationHashes` order to the first free slots. The same object in two sessions gets
  two slots. If capacity or readiness is insufficient, allocate nothing, raise a notification via
  `notifications.raise` with the reason, call `map.session.stop` once, and mark the ID terminal
  whatever the stop result. `sessionId`s are never reused.
- Before each dispatch fetch `map.session.objects.get` only when the local snapshot is older than
  the reconciliation interval or an event arrived; an unchanged object (by `updated_at` and
  content) never produces a write. Objects missing from `objects.get` are treated as removed.
- One ATN write queue serialises all position and icon frames across sessions. Frames carry
  `connectionGeneration`, `sessionId`, `operationId`, `frameNumber/totalFrames`; platform
  completions must echo them and a stale echo cannot advance a newer state. Coalesce only unsent
  position updates for the same slot.
- `map.session.stopped`, `object.removed`, capability revocation, host unbind and ATN disconnect
  cancel unsent work for the affected scope. Cancellation during a partial icon transfer closes
  ATN GATT, invalidates every display session and allocation, and requires new Share actions after
  an explicit reconnect. Releasing a slot does not clear the binocular display; the UI shows
  "released, display not cleared" distinctly from transport completion, opaque reply receipt and
  operator-confirmed rendering.
- A dispatched frame cannot be recalled; frames stop at the first locally observed invalidation.
  REM snapshots are not Bluetooth leases.

## Stage 4: Bluetooth coexistence and goTenna compatibility

- ATN GATT object, queue and lifecycle stay independent of REM's RNode link. Never toggle the
  adapter, never disconnect unrelated devices, keep discovery bounded and explicit.
- Transport boundary keeps characteristic identity, connection generation, negotiated capacity and
  typed completion/error so a goTenna plugin can add its own reply-indication and status channels
  without changing this one. No shared transport library, no Linux-console port in this release.
- Document disconnect-before-connect RNode/goTenna switching as operator-managed; switching must
  leave the REM node and the ATN plugin running. Write the goTenna architecture note as a separate
  short document (`docs/goals/gotenna-plugin/`), not as part of this delivery.

## Configuration and acceptance

Structured settings with profile import/export; no qualification wizard. Profiles supply device
semantics, evidence, operational limits, the slot pool, reconciliation interval, observer
freshness/accuracy limits and resource bounds. Settings persist privately; authorizations,
co-location confirmations, allocations and write queues never persist.

Support stays qualification-driven: only model/firmware/phone/REM-version combinations with
completed evidence are marked verified; others remain raw-observation-only.

Validation must cover:

- Real API 1.3 envelopes, `apiMinor` from `start()`, each typed error, `PermissionDenied` on
  revocation mid-session, 64 KiB limit, `textTruncated`, host-owned identities.
- Observer: unavailable, stale, poor accuracy, non-WGS84 datum, unexpected `source`; absent or
  expired co-location confirmation; unknown reference; repeated identical measurements; every
  create outcome including replay returning `created:false`.
- Sessions: four concurrent, overlapping selections, exhausted pool, dropped `started` event
  discovered by list, reordered/duplicate events, `objects.get` omitting objects, terminal stopped
  IDs, late echoes from an old connection generation.
- Partial icon transfer followed by session stop, revocation, unbind or disconnect: no further
  frames, no replay, no early slot reuse.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, JNI smoke, SDK-pinned Android unit tests, lint and assembly.
- Hardware: RNode BLE + ATN BLE and RNode Classic + ATN BLE with real REM marker visibility,
  real session events, binocular rendering, and the no-echo check. Recorded as unperformed until
  done. ATN + goTenna coexistence is an explicitly unperformed gate until that plugin exists.

## Delivery

Order: Stage 1, Stage 2, Stage 3, Stage 4 documentation, then hardware qualification. Keep this
document and an execution handoff under `docs/goals/atn-rem-plugin/`; record exact software results
and open host/hardware gates in `docs/implementation-status.md`.

Completion means visible REM markers and verified binocular rendering while the chosen RNode link
stays operational. Compilation and synthetic tests alone mean **implemented but unproven**.
