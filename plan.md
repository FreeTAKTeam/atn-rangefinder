# ATN BinoX REM plugin implementation plan

## 1. Objective

Implement ATN BinoX support as a separate Android REM plugin, but do not create a plugin-specific PLE or map-point subsystem.

The first product change is a general REM map-marker capability derived from the existing Reticulum Community Hub (RCH) implementation. REM must be able to create, persist, update, remove, select, and render point markers with different symbols independently of the ATN plugin. The plugin will then use that generic capability to add ranged observations to the map and to receive selected markers for display on the BinoX.

The required dependency direction is:

```text
RCH marker implementation
        |
        | extract/refactor reusable Rust code
        v
shared map-marker crates
        |
        +--------------------+
        |                    |
        v                    v
RCH HTTP/backend adapter   REM native backend and map
                                 |
                                 | REM plugin API
                                 v
                         ATN BinoX plugin
```

The ATN plugin must not own the REM map model, write directly into REM storage, create a parallel PLE schema, or reproduce RCH marker logic.

## 2. Architectural decision

### 2.1 RCH is the source implementation

RCH already implements the backend behavior needed for point markers:

- `GET /api/markers`
- `POST /api/markers`
- `GET /api/markers/symbols`
- `PATCH /api/markers/{object_destination_hash}`
- `PATCH /api/markers/{object_destination_hash}/position`
- `DELETE /api/markers/{object_destination_hash}`
- SQLite marker persistence
- idempotent marker creation
- marker normalization and validation
- multiple marker types and symbols
- marker API, persistence, restart, and parity tests

The RCH UI contract currently exposes fields including:

- `object_destination_hash`
- `origin_rch`
- `object_identity_storage_key`
- `marker_id`
- `type`
- `name`
- `category`
- `symbol`
- `notes`
- `position.lat`
- `position.lon`
- `time`
- `stale_at`
- `created_at`
- `updated_at`

The RCH symbol model supports a catalog rather than a single hard-coded dot. Existing examples include generic markers, friendly, hostile, neutral, unknown, vehicles, drones, people, sensors, radios, infrastructure, medical, alerts, and tasks.

### 2.2 Reuse means shared code, not copied code

Do not copy marker structs and CRUD functions from RCH into REM. Refactor the existing RCH implementation into reusable Rust crates and make RCH continue to consume those crates. REM then consumes the same pinned crates.

Transport-specific code remains outside the shared core:

- RCH keeps its Axum HTTP routes and API compatibility layer.
- REM exposes JNI/native methods, Vue stores, MapLibre rendering, and plugin Binder operations.
- The ATN project keeps BLE, BinoX codecs, geographic derivation, and synchronization state.

### 2.3 REM markers are a general product capability

The marker capability must be usable by:

- a human creating a point on the REM map;
- the ATN rangefinder plugin;
- later sensor or observation plugins;
- imported or synchronized operational data;
- future integrations that need typed symbols.

No ATN-specific field should be mandatory in the shared marker schema.

## 3. Source baselines

### 3.1 RCH baseline

Initial code-reuse analysis is based on:

- repository: `FreeTAKTeam/Reticulum-Community-Hub`
- commit: `4ac82730a4bf4b600b2fec111878c9d57ecbe70a`
- license declared by the Rust workspace: `EPL-2.0`

Relevant code includes:

- `crates/r3akt-rch-core/`
- `crates/r3akt-rch-core/src/sqlite_marker_idempotency.rs`
- `crates/r3akt-rch-core/src/sqlite_record_updates.rs`
- `crates/r3akt-rch-core/src/sqlite_read_snapshots.rs`
- `crates/r3akt-rch-core/migrations/0001_rch_core_snapshot.sql`
- `crates/r3akt-rch-server/src/marker_idempotency.rs`
- marker routes and validation currently located in `crates/r3akt-rch-server/src/lib.rs`
- `crates/r3akt-rch-server/tests/release_major_functionality.rs`
- `ui/src/api/types.ts`
- `ui/src/utils/markers.ts`
- `ui/src/stores/markers.ts`

Recheck RCH `main` before implementation and record the selected commit in both RCH and REM dependency documentation.

### 3.2 REM baseline

Initial analysis is based on:

- repository: `FreeTAKTeam/reticulum_mobile_emergency_management`
- commit: `40c6081b566ee0c36035c841399f492c8a3ac55e`

At this baseline:

- REM plugins are separate Android APKs.
- Plugin services bind through the `rem-plugin-sdk` AAR and subclass `RemPluginService`.
- The public plugin API is major 1, minor 1.
- Binder JSON messages are limited to 65,536 UTF-8 bytes.
- Existing host operations are `sensor.publish`, `events.publish`, `lxmf.send`, `notifications.raise`, and `operational.snapshot`.
- There is no generic marker create/list/update/select/subscribe operation.
- The current map is `TelemetryMapView.vue`, implemented with MapLibre and focused on telemetry and SOS projections.

Recheck REM `main` before coding.

### 3.3 ATN evidence

Use these files as the protocol evidence source:

1. `atn-rangefinder-docs/docs/evidence.md`
2. `atn-rangefinder-docs/docs/protocol.md`
3. `atn-rangefinder-docs/docs/rem-contract.md`
4. `atn-rangefinder-docs/fixtures/`

Preserve the evidence labels `APK-DERIVED`, `CAPTURE-REPORTED`, `UNVERIFIED`, and `SYNTHETIC`.

## 4. Shared marker subsystem

### 4.1 Proposed crate split

Refactor RCH marker code into two reusable crates inside the RCH workspace. Names are proposals and may be adjusted before coding.

```text
crates/
├── r3akt-map-core/
│   ├── domain types
│   ├── symbol catalog
│   ├── validation and normalization
│   ├── commands and results
│   ├── service/repository traits
│   └── pure unit tests
└── r3akt-map-sqlite/
    ├── SQLite repository
    ├── migrations
    ├── idempotent creation
    ├── update/delete operations
    └── persistence/restart tests
```

`r3akt-map-core` must not depend on Axum, Android, JNI, Vue, MapLibre, or an RCH server state object.

`r3akt-map-sqlite` may depend on `rusqlite`, but it must expose a small repository implementation rather than the entire RCH database.

If a single crate with an optional `sqlite` feature is measurably simpler, that is acceptable, provided the pure domain module remains independently testable and does not pull server dependencies into Android builds.

### 4.2 Code to extract from RCH

Extract and retain behavior for:

- marker record and identifier types;
- latitude/longitude validation;
- marker type, category, and symbol normalization;
- symbol aliases;
- marker creation;
- idempotency-key validation and replay behavior;
- create, read, list, update, position update, and delete operations;
- created/updated/stale timestamps;
- serialization compatibility;
- SQLite persistence and restart behavior;
- typed errors and conflict handling.

Do not move HTTP request parsing or Axum response generation into the shared crate.

### 4.3 Identifier compatibility

RCH currently uses `object_destination_hash` as the marker's primary external identifier. The extraction must not break existing RCH clients.

Initial rule:

- preserve `object_destination_hash` in the serialized RCH contract;
- expose a typed Rust identifier such as `MarkerId` internally;
- do not create a second REM-only PLE identifier for the same object;
- REM may display a shorter local label, but the shared marker ID remains authoritative;
- any future identity redesign must include explicit migration and compatibility handling.

### 4.4 Symbol catalog

The catalog must become backend-owned shared data rather than a UI-only fallback list.

A symbol entry should contain at least:

```rust
pub struct MarkerSymbol {
    pub id: String,
    pub set: SymbolSet,
    pub description: String,
    pub category: String,
    pub mdi: Option<String>,
    pub tak: Option<String>,
}
```

The first shared catalog should preserve RCH symbol IDs and aliases. RCH and REM may render symbols differently, but they must use the same canonical keys.

The ATN plugin must request or receive the REM-supported symbol catalog. It must not invent arbitrary symbol strings and expect REM to render them.

### 4.5 Additive metadata

The first extraction should preserve the existing RCH marker contract. Add only fields required for general interoperability, and make them optional and backward compatible.

Candidate optional fields:

- source application/plugin ID;
- source observation ID;
- verification state;
- accuracy or uncertainty;
- generic bounded properties;
- owner/session information used for authorization, not display.

Do not place raw BinoX packets, unbounded JSON, icons, or private device identifiers in the shared marker record. ATN evidence can remain in plugin-owned bounded storage and be referenced by an observation ID.

### 4.6 RCH regression requirement

RCH must consume the extracted crates before REM adopts them.

RCH exit criteria:

- existing marker endpoints remain compatible;
- existing marker symbol results remain compatible;
- create/update/move/delete behavior is unchanged;
- idempotent replay and key-conflict behavior is unchanged;
- SQLite restart tests pass;
- release parity tests pass;
- no duplicate marker implementation remains in `r3akt-rch-server`.

This proves that the shared crate contains the real implementation rather than a new approximation.

## 5. General REM marker capability

### 5.1 Native backend

REM must add a native marker service backed by the shared RCH-derived crate.

Responsibilities:

- initialize the shared SQLite repository within REM storage;
- expose create, get, list, update, move, and delete operations;
- preserve typed symbols and categories;
- publish marker projection invalidations to the Vue application;
- maintain local authorization and source information;
- support idempotent plugin-originated creates;
- provide a local observer position through a separate, explicit operation;
- avoid treating arbitrary team telemetry as the local observer.

The REM backend should call the shared marker service directly. It must not start an embedded RCH HTTP server or make HTTP calls to RCH.

### 5.2 REM JNI/native bridge

Add narrow JNI methods or an existing structured bridge operation for:

- list marker symbols;
- list markers;
- create marker;
- update marker metadata;
- update marker position;
- delete marker;
- get the local observer fix;
- register or poll marker projection changes.

All JSON or typed bridge payloads must be bounded and validated. Prefer the same DTO serialization used by the shared Rust crate.

### 5.3 REM map UI

Extend the current MapLibre map rather than building a second map.

Initial implementation can extend `TelemetryMapView.vue`; a later rename to `OperationalMapView.vue` is optional and should not block the capability.

Add:

- a marker store separate from telemetry positions;
- a MapLibre marker/source layer for shared markers;
- symbol rendering using the shared catalog;
- marker details and source status;
- create-marker action from the map;
- symbol/category selection;
- rename, move, and delete actions;
- explicit selection of one or more markers for a plugin synchronization session;
- visual distinction between draft/unverified and confirmed markers where supported.

Telemetry positions, SOS points, and persistent map markers remain separate domain objects even when shown on the same map.

### 5.4 REM persistence and migration

REM must use a schema owned by the shared marker SQLite adapter or a documented compatible adapter.

Requirements:

- migration is versioned and transactional;
- no marker loss on application upgrade;
- plugin removal does not delete shared markers automatically;
- source-plugin removal may mark provenance unavailable but must not silently remove user-confirmed markers;
- tests reopen the database and verify all fields and symbol keys.

### 5.5 Marker replication

Initial scope is local REM persistence and rendering. Do not block local marker creation on network replication.

Before adding network propagation, decide whether markers use:

- existing REM mission/event replication;
- RCH/R3AKT distributed-object semantics;
- a dedicated versioned marker message.

Whichever mechanism is selected must reuse the same shared marker model. Do not create a second network-only marker schema.

## 6. Generic REM plugin map API

### 6.1 Principle

The plugin API exposes generic map markers, not ATN PLEs.

The ATN plugin is one consumer. Future plugins should be able to use the same operations without depending on ATN concepts.

### 6.2 Proposed capabilities

Names are proposals to be finalized in REM:

- `map.markers.read`
- `map.markers.write`
- `map.markers.select`
- `map.observer.read`

Capabilities must be declared and granted separately. A plugin that only creates observations does not automatically gain access to all markers.

### 6.3 Proposed operations

#### `map.symbols.list`

Returns the bounded shared symbol catalog supported by the installed REM version.

#### `map.markers.create`

Creates a marker through the shared marker service.

Required payload fields:

- idempotency key or source observation ID;
- name;
- marker type;
- symbol key;
- category;
- latitude and longitude;
- optional altitude if the shared schema supports it;
- optional notes;
- optional bounded provenance and uncertainty fields.

The host derives and records the plugin ID from the Binder connection. A plugin-supplied plugin ID is not trusted.

#### `map.markers.get`

Returns one marker only when the plugin has the required grant and scope.

#### `map.markers.list`

Returns a bounded, filtered list. It must not expose the full map by default to every plugin.

#### `map.markers.update`

Updates permitted metadata through the shared service and revision/conflict rules.

#### `map.markers.position.update`

Moves a marker through the shared coordinate validation path.

#### `map.markers.delete`

Deletes a marker only when ownership and grant rules permit it.

#### `map.markers.selection.snapshot`

Returns only markers explicitly selected for the requesting plugin's active synchronization session.

#### `map.observer.snapshot`

Returns the local observer fix chosen by REM, including source, timestamp, horizontal accuracy, datum, and optional altitude.

It must never substitute the latest arbitrary telemetry position.

### 6.4 Proposed host events

- `map.marker.created`
- `map.marker.updated`
- `map.marker.deleted`
- `map.marker.selection.changed`
- `map.marker.selection.stopped`
- `map.observer.changed`

Events must include marker ID and revision. Selection events include the session ID and only the selected set for that plugin.

### 6.5 API versioning

Implement the map capability as the next compatible REM plugin API minor version unless REM's current development has already allocated that version.

Do not hard-code `1.2` until the active REM branch is checked. Update the SDK, plugin guide, fixture plugin, capability grant UI, Binder tests, and AAR artifact together.

## 7. ATN plugin architecture

The implementation remains Rust-first:

- Rust owns ATN codecs, physical validation, geographic calculations, synchronization state, BinoX slot ownership, command sequencing, and errors.
- A small Kotlin or Java shell owns Android BLE, Binder, lifecycle, permissions, pairing UI, REM SDK callbacks, and JNI.
- The plugin uses the generic REM marker API.
- The plugin contains no REM marker database or map renderer.

### 7.1 Proposed repository layout

```text
.
├── Cargo.toml
├── rust-toolchain.toml
├── crates/
│   ├── atn-protocol/
│   ├── atn-geo/
│   ├── atn-plugin-core/
│   └── atn-jni/
├── android/
│   └── app/
├── tests/fixtures/
├── tools/packet-replay/
└── atn-rangefinder-docs/
```

The ATN crates may depend on `r3akt-map-core` for marker DTOs and symbol keys, pinned to the same revision used by REM. They must not depend on `r3akt-rch-server`.

### 7.2 `atn-protocol`

Pure Rust responsibilities:

- UUID and command constants;
- decode BinoX command `0x01`;
- encode marker command `0x02`;
- encode and reassemble icon command `0x07`;
- parse replies without assigning undocumented meaning;
- validate signatures, lengths, numeric bounds, fragments, and PNG structure;
- never panic on input bytes.

### 7.3 `atn-geo`

Pure Rust responsibilities:

- represent observer fixes, timestamps, accuracy, datum, and bearing reference;
- convert slant range and pitch into horizontal and vertical components;
- project a target with a tested WGS84 direct geodesic;
- refuse projection when semantics or observer data are insufficient;
- convert selected REM markers into relative BinoX distance/bearing values.

### 7.4 `atn-plugin-core`

Deterministic pure Rust state machine responsibilities:

- device profile selection;
- BLE connection/subscription state;
- raw observation handling;
- REM host request correlation;
- idempotent marker creation;
- active marker-selection session;
- plugin-owned BinoX slot allocation;
- marker revision tracking;
- position-before-icon sequencing;
- bounded write queue, retries, timers, and cancellation.

### 7.5 Android shell

Suggested application ID:

`org.freetakteam.rem.plugin.atnrangefinder`

Suggested plugin ID:

`org.freetakteam.rem.plugin.atn_rangefinder`

Main components:

- `AtnRangefinderPluginService extends RemPluginService`
- `AtnBleClient`
- `AtnPairingActivity`
- `NativeCore`
- `PluginPreferences`
- `PluginNotification`
- offline configuration assets

No ATN packet decoding or coordinate calculation belongs in Java/Kotlin.

## 8. ATN protocol facts

### 8.1 BLE GATT

| Purpose | UUID |
|---|---|
| Service | `d973f2e0-b19e-11e2-9e96-0800200c9a66` |
| Device to host notifications | `d973f2e1-b19e-11e2-9e96-0800200c9a66` |
| Host to device writes | `d973f2e2-b19e-11e2-9e96-0800200c9a66` |
| CCCD | `00002902-0000-1000-8000-00805f9b34fb` |

Model ID 3 is associated with BinoX 4K and model ID 4 with BinoX 4T in the analyzed application. Service discovery remains authoritative.

### 8.2 Command `0x01`: measurement

Eight-byte minimum frame with signed 16-bit big-endian values:

| Offset | Width | Field |
|---:|---:|---|
| 0 | 1 | Signature `0x01` |
| 1 | 1 | Command `0x01` |
| 2 | 2 | Distance |
| 4 | 2 | Pitch |
| 6 | 2 | Compass |

Units, scaling, pitch polarity, compass reference, invalid values, and calibrated accuracy require hardware verification. Preserve raw values when interpretation is blocked.

### 8.3 Command `0x02`: display marker

Ten-byte record with little-endian multi-byte fields:

| Offset | Width | Field |
|---:|---:|---|
| 0 | 1 | Signature `0x01` |
| 1 | 1 | Command `0x02` |
| 2 | 1 | `(slot_id << 1) + marker_type` |
| 3 | 1 | `(enabled << 4) + color` |
| 4 | 2 | Distance |
| 6 | 2 | Pitch |
| 8 | 2 | Compass |

Reject overflow before narrowing.

### 8.4 Command `0x07`: display icon

A 14-byte little-endian header followed by PNG bytes. Fragmentation must handle exact-capacity and exact-multiple lengths correctly and must not reproduce the Tactical Map defect.

### 8.5 Replies

Six-byte replies for commands `0x02` and `0x07` have unresolved answer and tail semantics.

Report separately:

1. platform write accepted;
2. GATT write completion/failure;
3. matching device reply received;
4. marker visually confirmed on the BinoX.

## 9. Observation-to-marker workflow

1. The user pairs and connects a supported BinoX.
2. A command `0x01` notification reaches the Rust decoder.
3. Rust retains the raw observation.
4. A verified device profile interprets distance, pitch, and compass.
5. The plugin requests `map.observer.snapshot` from REM.
6. Rust validates observer age, accuracy, datum, co-location/offset, and bearing reference.
7. Rust derives the geographic target.
8. The plugin requests the shared symbol catalog if not cached.
9. The plugin submits `map.markers.create` with:
   - a stable idempotency/source observation key;
   - a user-configured or default valid symbol key;
   - category such as `observation`;
   - derived position;
   - bounded uncertainty and provenance.
10. REM creates the marker through the shared RCH-derived marker service.
11. The normal REM map projection displays the marker.

The plugin does not send a special ATN PLE event and does not write directly into Vue state or SQLite.

## 10. Marker-to-BinoX workflow

1. The user selects one or more shared REM markers for the ATN plugin.
2. REM creates an authorized selection session scoped to the plugin.
3. The plugin receives `map.markers.selection.snapshot`.
4. Rust allocates plugin-owned BinoX slots.
5. Marker coordinates are converted to relative distance and bearing using the approved observer fix.
6. The shared symbol key is mapped to a bounded plugin-packaged BinoX icon and color.
7. Rust sends command `0x02`, followed by command `0x07` fragments.
8. Marker updates are propagated only while the selection session remains active.
9. Removing a selection affects only the corresponding plugin-owned slot.
10. Stopping the session cancels queued writes and prevents later replay.

Do not silently synchronize every REM marker.

## 11. Geographic gates

A BinoX observation can create a geographic marker only when:

- the exact model/firmware profile is verified;
- distance units and scale are known;
- pitch scale and polarity are known;
- compass scale and reference are known;
- the measurement is within verified bounds;
- the observer fix belongs to the local operator/device;
- fix age and accuracy are within configured limits;
- datum is known;
- BinoX/phone co-location or an offset is recorded.

Use:

```text
horizontal_range = slant_range * cos(pitch)
vertical_delta = slant_range * sin(pitch)
```

Then apply a direct WGS84 geodesic with the verified bearing.

Unknown magnetic-versus-true reference blocks projection. Do not silently apply declination or treat magnetic bearing as true.

## 12. Implementation phases

## Phase 0: freeze contracts and baselines

Deliverables:

- record active RCH and REM commits;
- inventory all RCH marker structs, handlers, normalization, persistence, migrations, symbols, and tests;
- confirm RCH serialization compatibility requirements;
- define the shared crate boundary;
- decide the initial optional metadata fields;
- confirm REM plugin API version allocation;
- resolve EPL reuse, repository licensing, and NOTICE requirements.

Exit criteria:

- there is one agreed canonical marker model;
- no parallel REM PLE schema is proposed;
- extraction targets are linked to concrete RCH source files and tests.

## Phase 1: extract the RCH shared marker crates

Deliverables in RCH:

- `r3akt-map-core` and `r3akt-map-sqlite`, or an approved equivalent;
- marker domain, symbol catalog, validation, CRUD service, idempotency, and persistence moved from RCH-specific modules;
- compatibility adapters for the existing RCH API;
- crate documentation and versioning.

Exit criteria:

- RCH compiles against the extracted crates;
- no copied marker business logic remains in the server;
- all RCH marker and release tests pass;
- existing clients receive compatible responses.

## Phase 2: add general markers to the REM backend

Deliverables in REM:

- dependency pinned to the reviewed shared crate revision;
- REM marker repository and migrations;
- native CRUD methods;
- symbol catalog access;
- projection invalidation/event path;
- local observer-fix service with explicit ownership.

Exit criteria:

- markers persist across REM restart;
- create/update/move/delete use the shared service;
- invalid coordinates and symbols are rejected identically to RCH;
- REM does not run or call an RCH HTTP server.

## Phase 3: render and edit shared markers in REM

Deliverables in REM UI:

- marker store;
- MapLibre rendering in the existing map;
- shared symbol catalog rendering;
- marker create, inspect, rename, move, and delete UI;
- marker selection UI for plugin sessions;
- unit and end-to-end tests.

Exit criteria:

- a user can create several symbol types as persistent map dots;
- telemetry, SOS, and shared markers remain distinct;
- restart preserves markers and symbols;
- selection is explicit and visible.

## Phase 4: expose the generic plugin map API

Deliverables in REM and SDK:

- capability declarations and grants;
- generic marker operations;
- observer snapshot operation;
- marker and selection events;
- fixture-plugin coverage;
- updated plugin documentation and AAR.

Exit criteria:

- a fixture plugin can create a shared marker;
- the marker appears through the normal REM map path;
- a fixture plugin can receive only markers selected for it;
- capability revocation blocks access;
- all Binder messages remain below 64 KiB.

## Phase 5: create the ATN Rust workspace and codecs

Deliverables:

- Cargo workspace and Android skeleton;
- `atn-protocol`, `atn-geo`, `atn-plugin-core`, and `atn-jni`;
- command `0x01`, `0x02`, and `0x07` codecs;
- reply parser;
- synthetic tests, property tests, and fuzz targets.

Exit criteria:

- malformed input never panics;
- endian behavior is byte-exact;
- icon fragmentation/reassembly passes exact-multiple tests;
- no private capture bytes are committed.

## Phase 6: Android BLE read-only plugin

Deliverables:

- plugin service and signer verification;
- pairing activity and permissions;
- foreground connection notification;
- service discovery by UUID;
- notification subscription;
- raw measurement display and sanitized diagnostics.

Exit criteria:

- repeated BinoX observations reach Rust;
- disconnect and permission revocation are recoverable;
- no marker is created and no BinoX write is performed yet.

## Phase 7: validate physical semantics

Deliverables for each supported model/firmware:

- units and scales;
- pitch sign convention;
- compass north reference;
- invalid/no-range values;
- verified range limits;
- accuracy observations.

Exit criteria:

- versioned profiles can be marked verified;
- unknown firmware remains read-only;
- geographic projection remains blocked for unknown semantics.

## Phase 8: BinoX observation to shared REM marker

Deliverables:

- observer snapshot request;
- geographic derivation;
- shared symbol selection;
- `map.markers.create` request;
- idempotency and duplicate protection;
- bounded provenance and uncertainty;
- blocked/created/failed UI states.

Exit criteria:

- a valid observation creates a marker through the shared marker service;
- the marker is visible in the normal REM map;
- repeated packets do not create uncontrolled duplicates;
- missing or invalid observer data blocks creation.

## Phase 9: selected REM marker to BinoX

Deliverables:

- authorized selection session;
- selection snapshot and change events;
- geographic-to-relative conversion;
- slot allocation;
- command `0x02` and `0x07` transmission;
- update, removal, stop, and reconnect handling.

Exit criteria:

- a selected shared marker appears on the BinoX;
- symbol/icon mapping is deterministic;
- only plugin-owned slots are changed;
- stop prevents subsequent writes;
- stale commands are not replayed after reconnect.

## Phase 10: hardening and release

Deliverables:

- RCH/REM shared-crate compatibility matrix;
- supported BinoX model/firmware table;
- permission, privacy, dependency, and license review;
- lifecycle and crash tests;
- release signing and reproducible build notes;
- known limitations.

## 13. Test plan

### Shared marker crate tests

- valid and invalid coordinates;
- symbol normalization and aliases;
- create/read/list/update/move/delete;
- idempotent replay;
- idempotency-key conflict;
- revision/conflict handling where implemented;
- SQLite migration and restart;
- backward-compatible RCH serialization;
- bounded optional properties;
- no panic on malformed serialized input.

### RCH regression tests

- all current marker endpoints;
- symbol catalog compatibility;
- restart persistence;
- existing parity and major-functionality tests;
- TAK connector marker ingestion where applicable.

### REM native and UI tests

- migration and restart;
- JNI/native CRUD;
- symbol catalog;
- MapLibre marker rendering;
- multiple symbols;
- manual create/update/move/delete;
- separation from telemetry/SOS;
- selection-session isolation.

### REM plugin API tests

- capability grant and denial;
- generic marker creation through fixture plugin;
- invalid marker rejection identical to RCH;
- observer snapshot ownership;
- selected-set isolation;
- update/delete events;
- session stop;
- Binder death and 64 KiB limits.

### ATN Rust tests

- command `0x01` valid and malformed frames;
- signed fields and trailing bytes;
- command `0x02` packing and overflow rejection;
- command `0x07` zero, one, capacity boundaries, exact multiples, ordering, duplicates, and reassembly;
- PNG validation;
- reply parsing;
- geodesic calculations and derivation gates;
- state-machine cancellation, timeout, reconnect, duplicate observation, and slot ownership.

### Android tests

- manifest discovery;
- REM host package/certificate verification;
- permissions and pairing cancellation;
- GATT discovery and subscription;
- MTU fallback and serialized writes;
- plugin start/stop and Binder death;
- configuration validation.

### Hardware acceptance

For each supported BinoX model/firmware:

- pair and reconnect;
- repeat stationary nonliving target measurements;
- compare raw values with displayed distance, pitch, and compass;
- test no-range behavior;
- create a REM marker from an observation;
- select an existing REM marker and display it on the BinoX;
- update icon/position;
- remove only the plugin-owned slot;
- disconnect during icon transfer;
- restart REM and verify no stale write replay.

Keep captures private and redact locations and device identifiers.

## 14. Security and ownership rules

- REM remains the authority for map-marker persistence and authorization.
- The plugin cannot bypass the REM marker service.
- The host derives plugin identity from Binder.
- Marker write access is capability-gated.
- Selection sessions are explicit, scoped, bounded, and stoppable.
- Plugin removal does not silently delete confirmed shared markers.
- The ATN plugin only controls BinoX slots it allocated.
- No clear-all command is used without an explicit reviewed user action.
- No exact coordinates, BLE addresses, serial numbers, or raw captures appear in production logs.
- Every buffer, queue, JSON payload, fragment count, retry count, and property map is bounded.
- Signing keys and credentials remain outside git.

## 15. Main risks and decisions

| Item | Risk | Required mitigation |
|---|---|---|
| RCH marker logic is split between core and a large server module | Direct dependency would pull RCH-specific code into REM | Extract domain/service/persistence into small shared crates first |
| Copying instead of refactoring | RCH and REM behavior diverge | Make RCH consume the extracted crate before REM adoption |
| Identifier mismatch | Same marker receives RCH and REM IDs | Preserve the shared marker ID and RCH compatibility field |
| Symbol duplication | UI catalogs drift | Move canonical catalog/aliases into the shared backend crate |
| RCH/REM Rust edition difference | Dependency/build friction | Use a toolchain accepted by both; REM Rust 1.88 can consume edition 2024 crates |
| Mobile dependency size | Android binary grows or fails to build | Keep pure core small; isolate SQLite and server dependencies |
| REM has only a telemetry-focused map | Persistent markers could be mixed with live telemetry | Add a separate marker store/layer in the existing MapLibre view |
| Plugin overexposure | Plugin reads the entire operational map | Scope reads to grants and explicit selection sessions |
| Observer identity | Wrong team position used as local origin | Provide a dedicated local-observer operation |
| Compass reference | Derived target is rotated | Verify reference and block projection while unknown |
| Marker provenance growth | Unbounded plugin data bloats storage/Binder | Store bounded common metadata and plugin-owned detailed evidence |
| Licensing | Shared EPL code is incorporated into a repo without clear terms | Resolve REM license/NOTICE and preserve provenance before distribution |
| Network replication | A second marker wire model appears | Reuse the shared marker model for any later replication path |

## 16. Definition of done

The work is complete only when:

1. RCH marker behavior is implemented by shared Rust crates rather than duplicated server logic.
2. Existing RCH marker endpoints and tests remain compatible.
3. REM depends on the reviewed shared crate revision.
4. REM can create, persist, update, move, delete, and render shared markers with several symbol types without the ATN plugin installed.
5. REM exposes capability-gated generic marker operations to plugins.
6. A fixture plugin creates a normal shared marker visible on the REM map.
7. The ATN plugin installs as a separate signed REM plugin and verifies the REM host signer.
8. A user can connect a supported BinoX 4K or 4T.
9. Rust decodes raw distance, pitch, and compass values.
10. A verified observation creates a shared REM marker through `map.markers.create`.
11. An explicitly selected shared REM marker can be displayed on the BinoX.
12. Updates flow only during an authorized selection session.
13. Stop, disconnect, permission revocation, and Binder death cancel pending work.
14. The plugin never changes non-owned BinoX slots.
15. Tests cover shared marker code, RCH regression, REM persistence/UI/API, ATN codecs, JNI, BLE, and supported hardware.
16. Release documentation records shared-crate versions, supported firmware, unresolved semantics, and privacy limits.

## 17. First coding sequence

Execute in this order:

1. Pin and document the active RCH and REM commits.
2. Inventory the RCH marker implementation and map each behavior to a source file and test.
3. Define the canonical shared marker and symbol contracts.
4. Extract `r3akt-map-core` and the SQLite adapter from RCH.
5. Refactor RCH to consume the extracted crates and pass all regression tests.
6. Add the shared crates to REM at a pinned revision.
7. Implement REM marker persistence and native CRUD.
8. Render and edit shared markers in the existing REM MapLibre map.
9. Add explicit marker selection sessions.
10. Extend the REM plugin SDK with generic marker and observer operations.
11. Prove the API through a fixture plugin.
12. Build the ATN Rust workspace and protocol tests.
13. Add JNI and the minimal Android plugin service.
14. Implement pairing and read-only BLE.
15. Validate BinoX physical semantics.
16. Connect BinoX observations to `map.markers.create`.
17. Connect selected REM markers to BinoX command `0x02` and `0x07`.
18. Run lifecycle, security, privacy, licensing, and release checks.
