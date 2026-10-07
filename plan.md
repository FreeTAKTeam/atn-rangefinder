# ATN BinoX REM plugin implementation plan

## 1. Objective

Implement a separate Android REM plugin APK that connects to supported ATN BinoX binoculars over BLE, decodes range observations, and exchanges selected REM PLEs with the BinoX display.

The implementation is Rust-first:

- Rust owns the ATN codecs, validation, geographic calculations, synchronization state, device-slot ownership, command sequencing, and error model.
- A small Kotlin or Java shell owns Android-only APIs: BLE, Binder, foreground-service lifecycle, permissions, pairing UI, REM SDK callbacks, and JNI calls.
- The plugin uses REM's existing APK/Binder plugin model. It is not a Rust shared library loaded directly by REM.

This file is an implementation plan. It does not claim that a driver, APK, working plugin, or hardware test already exists.

## 2. Baselines

### ATN evidence

Use these repository documents as the protocol source:

1. [`atn-rangefinder-docs/docs/evidence.md`](atn-rangefinder-docs/docs/evidence.md)
2. [`atn-rangefinder-docs/docs/protocol.md`](atn-rangefinder-docs/docs/protocol.md)
3. [`atn-rangefinder-docs/docs/rem-contract.md`](atn-rangefinder-docs/docs/rem-contract.md)
4. [`atn-rangefinder-docs/docs/implementation-plan.md`](atn-rangefinder-docs/docs/implementation-plan.md)
5. [`atn-rangefinder-docs/fixtures/`](atn-rangefinder-docs/fixtures/)

Keep the evidence labels intact:

- `APK-DERIVED`
- `CAPTURE-REPORTED`
- `UNVERIFIED`
- `SYNTHETIC`

Do not turn an APK observation or a captured byte pattern into a general firmware guarantee.

### REM baseline

The REM plugin model was rechecked against `FreeTAKTeam/reticulum_mobile_emergency_management` commit:

`40c6081b566ee0c36035c841399f492c8a3ac55e`

At this baseline:

- Plugins are separate Android APKs.
- Services bind through the `rem-plugin-sdk` AAR and subclass `RemPluginService`.
- The public API is major 1, minor 1.
- JSON Binder messages are limited to 65,536 UTF-8 bytes.
- Existing host operations are `sensor.publish`, `events.publish`, `lxmf.send`, `notifications.raise`, and `operational.snapshot`.
- There is no map/PLE create, select, list, subscribe, or observer-fix operation.

Recheck REM `main` and record the selected commit before implementation starts.

## 3. Protocol facts used by the implementation

### BLE GATT

| Purpose | UUID |
|---|---|
| Service | `d973f2e0-b19e-11e2-9e96-0800200c9a66` |
| Device to host notifications | `d973f2e1-b19e-11e2-9e96-0800200c9a66` |
| Host to device writes | `d973f2e2-b19e-11e2-9e96-0800200c9a66` |
| CCCD | `00002902-0000-1000-8000-00805f9b34fb` |

ATN discovery data identifies model ID 3 as BinoX 4K and model ID 4 as BinoX 4T. Model detection must remain advisory until service discovery succeeds.

### Command `0x01`: BinoX measurement

Eight-byte minimum frame, with signed 16-bit big-endian fields:

| Offset | Width | Field |
|---:|---:|---|
| 0 | 1 | Signature, expected `0x01` |
| 1 | 1 | Command, expected `0x01` |
| 2 | 2 | Distance |
| 4 | 2 | Pitch |
| 6 | 2 | Compass |

Distance units, scaling, pitch polarity, compass reference, invalid values, and calibrated accuracy remain subject to field verification. The decoder must preserve raw values even when physical interpretation is unavailable.

### Command `0x02`: display marker position

Ten-byte record. Multi-byte fields are little-endian.

| Offset | Width | Field |
|---:|---:|---|
| 0 | 1 | Signature `0x01` |
| 1 | 1 | Command `0x02` |
| 2 | 1 | `(slot_id << 1) + marker_type` |
| 3 | 1 | `(enabled << 4) + color` |
| 4 | 2 | Distance |
| 6 | 2 | Pitch |
| 8 | 2 | Compass |

All narrowing must be checked. Do not permit integer wrapping.

### Command `0x07`: display icon

A 14-byte little-endian header followed by a PNG fragment:

| Offset | Width | Field |
|---:|---:|---|
| 0 | 1 | Signature `0x01` |
| 1 | 1 | Command `0x07` |
| 2 | 2 | Total frame count |
| 4 | 2 | Total PNG length |
| 6 | 2 | Current frame number, starting at 1 |
| 8 | 2 | Fragment length |
| 10 | 2 | Slot ID |
| 12 | 2 | Field named CRC; observed encoder uses zero |
| 14 | variable | PNG bytes |

The codec must correctly handle exact-capacity and exact-multiple lengths. Do not reproduce the apparent Tactical Map final-fragment defect.

### Replies

Six-byte replies associated with commands `0x02` and `0x07` contain signature, command, an answer byte, and a three-byte tail. The meaning of the answer and tail is unresolved.

The implementation must distinguish:

1. Android accepted a write request.
2. The GATT callback reported completion or failure.
3. A device reply with a matching command was received.
4. The marker was actually visible on the BinoX.

Do not label these four states as the same success condition.

## 4. Scope

### Included

- User-selected connection to a BinoX 4K or BinoX 4T.
- BLE discovery, GATT connection, MTU handling, notification subscription, writes, and disconnect.
- Decode and retain raw range, pitch, and compass observations.
- Apply a verified device profile to interpret physical values.
- Obtain a suitable observer fix from REM through a new narrow host operation.
- Derive a draft target coordinate only when the required metadata is available.
- Create a draft PLE in REM with source and uncertainty metadata.
- Send explicitly selected REM PLEs to plugin-owned BinoX display slots.
- Keep selected PLE updates synchronized during an explicitly started session.
- Stop synchronization and disconnect cleanly.
- Send a bounded 40 x 40 PNG icon for each owned slot.

### Excluded

- LXMF changes or new network transport.
- Changes to Reticulum or the REM network core.
- Ballistics, aiming, fire control, or weapon integration.
- ATN firmware modification.
- General REM map rendering.
- Automatic synchronization of all REM PLEs.
- Clearing markers that the plugin does not own.
- Treating a derived coordinate as a BinoX GNSS fix.

## 5. Repository layout

Create one Cargo workspace and one Android application module:

```text
.
├── Cargo.toml
├── rust-toolchain.toml
├── crates/
│   ├── atn-protocol/
│   │   ├── Cargo.toml
│   │   └── src/
│   ├── atn-geo/
│   │   ├── Cargo.toml
│   │   └── src/
│   ├── atn-plugin-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   └── atn-jni/
│       ├── Cargo.toml
│       └── src/
├── android/
│   ├── settings.gradle.kts
│   ├── build.gradle.kts
│   ├── gradle.properties
│   └── app/
│       ├── build.gradle.kts
│       └── src/
│           ├── main/
│           │   ├── AndroidManifest.xml
│           │   ├── java/org/freetakteam/rem/plugin/atnrangefinder/
│           │   ├── jniLibs/
│           │   └── assets/rem-plugin-config/
│           ├── test/
│           └── androidTest/
├── tests/
│   └── fixtures/
├── tools/
│   └── packet-replay/
└── atn-rangefinder-docs/
```

### `atn-protocol`

Pure Rust with no Android dependency.

Responsibilities:

- UUID and command constants.
- Decode command `0x01`.
- Encode command `0x02`.
- Encode and reassemble command `0x07` fragments.
- Parse replies without assigning undocumented meaning.
- Validate lengths, signatures, command values, frame numbers, PNG bounds, and integer ranges.
- Preserve trailing bytes for diagnostics.
- Expose typed errors; never panic on input bytes.

### `atn-geo`

Pure Rust.

Responsibilities:

- Represent observer fixes, bearing references, datums, timestamps, uncertainty, and device offset assumptions.
- Convert slant range and pitch to horizontal distance and vertical delta.
- Project a target using a tested WGS84 direct-geodesic calculation.
- Refuse geographic derivation when the compass reference is unknown or the observer fix is stale, inaccurate, or missing required metadata.
- Preserve both raw and interpreted values in the result.

Do not add magnetic declination correction until the BinoX compass reference is verified. If the device reports magnetic bearing, add an explicit, tested correction source later; never silently treat magnetic bearing as true bearing.

### `atn-plugin-core`

Pure Rust state machine.

Responsibilities:

- Device profile selection.
- Connection and subscription state.
- Authorized synchronization session state.
- Observation validation and duplicate handling.
- Host-request correlation and timeout state.
- Plugin-owned slot allocation.
- PLE revision tracking.
- Position-before-icon sequencing.
- Bounded write queue.
- Retry policy that excludes validation, authorization, and permission failures.
- Cancellation and disconnect behavior.
- Sanitized diagnostics.

The core should consume typed inputs and emit typed actions. It should not call Android APIs directly.

Example inputs:

```rust
CoreInput::BleConnected
CoreInput::BleDisconnected { reason }
CoreInput::BleNotification { bytes, received_at_ms }
CoreInput::BleWriteResult { operation_id, result }
CoreInput::HostStarted { session }
CoreInput::HostStopped { reason }
CoreInput::HostEvent { event }
CoreInput::HostResponse { response }
CoreInput::UserCommand { command }
CoreInput::Timer { timer_id, now_ms }
```

Example actions:

```rust
CoreAction::RequestBleWrite { operation_id, value }
CoreAction::SubmitHostRequest { request_id, operation, payload }
CoreAction::RequestObserverSnapshot
CoreAction::UpdatePluginState { state }
CoreAction::RaiseUserNotice { severity, message }
CoreAction::ScheduleTimer { timer_id, delay_ms }
CoreAction::CancelTimer { timer_id }
CoreAction::DisconnectBle
```

Keep this state machine deterministic so unit tests can reproduce every transition without Android or hardware.

### `atn-jni`

Rust `cdylib` exposing a narrow JNI surface.

Responsibilities:

- Create and destroy a core instance.
- Accept platform events and byte arrays.
- Return pending actions to the Android shell.
- Convert typed Rust errors into stable error codes and messages.
- Prevent Rust panics from crossing JNI.
- Validate handle ownership and reject use-after-destroy.

Prefer a small event/action interface rather than many JNI methods tied to internal modules. Keep unsafe code limited to the JNI boundary and enable `#![deny(unsafe_op_in_unsafe_fn)]`.

### Android shell

Suggested application ID:

`org.freetakteam.rem.plugin.atnrangefinder`

Suggested REM plugin ID:

`org.freetakteam.rem.plugin.atn_rangefinder`

The Android shell contains no ATN packet decoding or geographic calculations.

Main components:

- `AtnRangefinderPluginService extends RemPluginService`
- `AtnBleClient`
- `AtnPairingActivity`
- `NativeCore`
- `PluginPreferences`
- `PluginNotification`
- offline configuration assets under `assets/rem-plugin-config/`

The shell executes actions produced by Rust and sends results back to Rust.

## 6. Rust data model

The first implementation should include types equivalent to the following:

```rust
pub struct RawMeasurement {
    pub signature: u8,
    pub command: u8,
    pub distance_raw: i16,
    pub pitch_raw: i16,
    pub compass_raw: i16,
    pub trailing: Vec<u8>,
    pub received_at_ms: i64,
}

pub enum BearingReference {
    TrueNorth,
    MagneticNorth,
    Unknown,
}

pub enum SemanticsStatus {
    Unverified,
    Verified,
}

pub struct DeviceProfile {
    pub model: DeviceModel,
    pub firmware: Option<String>,
    pub semantics_status: SemanticsStatus,
    pub distance_scale: f64,
    pub pitch_scale: f64,
    pub compass_scale: f64,
    pub bearing_reference: BearingReference,
    pub pitch_positive_is_up: Option<bool>,
    pub minimum_distance: Option<f64>,
    pub maximum_distance: Option<f64>,
    pub effective_att_value_capacity: usize,
}

pub struct ObserverFix {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: Option<f64>,
    pub horizontal_accuracy_m: f64,
    pub captured_at_ms: i64,
    pub datum: String,
    pub source: String,
}

pub struct InterpretedObservation {
    pub raw: RawMeasurement,
    pub slant_range_m: f64,
    pub pitch_deg: f64,
    pub bearing_deg: f64,
    pub bearing_reference: BearingReference,
    pub profile_id: String,
}

pub struct DerivedTarget {
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: Option<f64>,
    pub horizontal_range_m: f64,
    pub vertical_delta_m: f64,
    pub observer_fix: ObserverFix,
    pub observation: InterpretedObservation,
}
```

Raw observations must remain available even when interpretation or projection is blocked.

## 7. BLE implementation

### Device selection

- Scan only from an explicit pairing activity.
- Filter by the ATN service UUID and recognized model data where available.
- Show the user model, advertised name, and a redacted identifier.
- Require the user to select the device.
- Persist only the minimum information required to reconnect.
- Revalidate the service and characteristics after every connection.
- Never connect to the first device merely because its name starts with `ATN`.

### Android permissions

Handle Android API differences explicitly:

- API 31 and later: `BLUETOOTH_SCAN` and `BLUETOOTH_CONNECT`.
- Older supported APIs: the location permission required by Android BLE scanning rules.
- API 33 and later: notification permission where required for the foreground notification.

Android permission approval is separate from REM plugin trust and capability grants.

### Connection sequence

1. User selects a device.
2. Connect without automatic write replay.
3. Request the largest supported MTU.
4. Record the negotiated MTU callback.
5. Discover services.
6. Locate characteristics by UUID, never by captured ATT handle.
7. Enable local notification routing.
8. Write the CCCD.
9. Mark the connection subscribed only after descriptor success.
10. Start read-only observation handling.

### Effective write capacity

The Tactical Map evidence used full ATT characteristic values of 509 bytes. A new implementation should calculate:

```text
effective_value_capacity = min(platform_value_capacity, verified_device_profile_capacity)
icon_payload_capacity = effective_value_capacity - 14
```

Use 509 as the initial verified profile limit, not as a global BLE constant. Record negotiated MTU separately. Test smaller capacities and do not assume every Android stack or firmware accepts the same value.

### Write serialization

Android GATT operations must be serialized.

- Maintain one active write operation.
- Send command `0x02` before its command `0x07` icon transfer.
- Send icon fragments in order.
- Use write-without-response because that is what the analyzed application uses, but still process Android callback failures where delivered.
- Do not assume an acknowledgement for every icon fragment.
- Correlate a six-byte reply by command and retain the raw answer/tail.
- Apply bounded timeouts.
- Never blindly replay pending writes after reconnect.

### Reconnect policy

- Reconnect only to a user-selected device.
- Use bounded exponential delay.
- Stop retries when REM stops the plugin, permission is revoked, the user disconnects, or the device is marked unsupported.
- Reconnection does not restart an outbound synchronization session without explicit retained authorization.
- Reconnection does not clear all BinoX markers.

## 8. REM plugin integration

### Existing REM API use

The plugin service must:

- Export `network.reticulum.emergency.PLUGIN_V1`.
- Declare API major 1 and the supported minor version.
- Verify allowed REM package names and signing-certificate fingerprints on every Binder call through `RemPluginService`.
- Keep host package and certificate values in build configuration, not source secrets.
- Treat Binder exceptions, typed host errors, timeouts, and Binder death as separate outcomes.
- Stop BLE work when `onPluginStop` is called.
- Use an offline configuration page for status and session controls.

No existing API 1.1 operation can safely replace a real PLE or observer-position operation. In particular, do not use `operational.snapshot.latestPosition` as the operator fix because it may be another telemetry record.

### Required REM extension

Before geographic integration, agree and implement a narrow REM API extension. The following names are proposals, not existing operations.

Proposed API minor: 1.2.

Proposed capabilities:

- `mapObserverRead`
- `mapPleRead`
- `mapPleWrite`

Proposed requests:

#### `map.observer.snapshot`

Returns the local observer fix selected by REM, including source, timestamp, accuracy, datum, and optional altitude.

The operation must never return the newest arbitrary team telemetry record as though it were the local observer.

#### `map.ple.draft.create`

Creates a local draft PLE from a ranged observation. The payload contains:

- plugin observation ID
- raw distance, pitch, and compass fields
- interpreted values and device profile ID
- observer fix and timestamp
- derived coordinate and uncertainty
- source plugin and redacted device identity
- verification state

The result returns the REM PLE ID and revision. The new point remains a draft until the user confirms it under REM rules.

#### `map.ple.sync.snapshot`

Returns only the PLEs explicitly selected for the active ATN synchronization session. It must not return the full map by default.

Proposed host events:

- `map.observer.changed`
- `map.ple.sync.changed`
- `map.ple.sync.stopped`

A PLE change event should include:

- session ID
- PLE ID
- revision
- latitude, longitude, and optional altitude
- display type or symbol key
- color key
- removed flag

Keep icon PNG bytes out of Binder messages. Map a bounded symbol/color set to plugin-packaged 40 x 40 PNG assets.

### Synchronization session

1. The user starts ATN synchronization in the plugin configuration or a REM map action.
2. REM supplies an authorized session ID and the selected PLE set.
3. Rust allocates plugin-owned BinoX slots.
4. Each PLE is converted from geographic coordinates to relative distance and bearing using the current approved observer fix.
5. Rust emits command `0x02`, then the icon transfer.
6. Updates to the selected set may flow automatically while the session remains active.
7. Removing a selected PLE disables or removes only its plugin-owned slot.
8. Stopping the session cancels queued writes and releases only plugin-owned slots according to verified device behavior.

Do not silently expand the selected set.

## 9. Geographic derivation rules

A BinoX observation can produce a derived target only when all gates pass:

- Device profile semantics are marked verified for the exact supported model/firmware family.
- Distance is valid and inside the verified range.
- Pitch is valid and its polarity is known.
- Compass value is valid.
- Compass reference is known.
- Observer fix is the local operator fix, not arbitrary telemetry.
- Observer fix age is below the configured limit.
- Horizontal accuracy is below the configured limit.
- Datum is known.
- Phone/BinoX co-location or a configured offset is explicitly recorded.

Calculate:

```text
horizontal_range = slant_range * cos(pitch)
vertical_delta = slant_range * sin(pitch)
```

Then apply the direct WGS84 geodesic from observer latitude/longitude using the verified bearing.

The resulting PLE metadata must retain the source observation and assumptions. A Bluetooth measurement is not a verified GNSS target.

## 10. Implementation phases

## Phase 0: freeze contracts and evidence

Deliverables:

- Record the REM commit used for implementation.
- Confirm final plugin/application IDs.
- Approve the proposed map/PLE host extension or replace it with a reviewed alternative.
- Define observer-fix age and accuracy limits.
- Define the initial supported BinoX model/firmware matrix.
- Define plugin-owned slot cleanup rules.

Exit criteria:

- No implementation relies on `operational.snapshot.latestPosition` for the local observer.
- Map operation names, capabilities, payloads, and events are agreed.
- Unverified ATN semantics remain explicitly marked.

## Phase 1: Rust workspace and build skeleton

Deliverables:

- Cargo workspace and four crates.
- Android Gradle project.
- Pinned Rust and Android build versions.
- JNI library loading.
- Minimal plugin service discovered by REM.
- Host signer verification configuration.
- Local build instructions.

Exit criteria:

- Rust tests run on the development host.
- Android debug APK builds.
- REM discovers the APK as a plugin.
- The plugin starts and stops without BLE activity.
- No protocol implementation is claimed yet.

## Phase 2: ATN codecs

Deliverables:

- Command `0x01` decoder.
- Command `0x02` encoder.
- Command `0x07` fragmentation and reassembly.
- Six-byte reply parser.
- PNG length/dimension validation.
- Synthetic fixtures and byte-exact tests.
- Property tests and fuzz targets for byte parsers.

Exit criteria:

- Malformed, short, unknown, and oversized input returns typed errors without panic.
- Big-endian and little-endian behavior is byte-exact.
- Exact-capacity and exact-multiple icon cases pass.
- Reassembly reproduces every input PNG byte.
- No private capture bytes are committed.

## Phase 3: core state machine and JNI

Deliverables:

- Deterministic core input/action loop.
- Device profile storage.
- Host request correlation.
- Slot allocator.
- Position/icon operation sequencing.
- JNI create, destroy, input, and action-drain methods.
- Fake BLE and fake host adapters for tests.

Exit criteria:

- Full state transitions run in host-side tests.
- Disconnect and stop cancel pending work.
- No panic crosses JNI.
- Retry limits and timeout paths are tested.

## Phase 4: Android BLE read-only plugin

Deliverables:

- Pairing activity.
- Permission flow.
- Foreground notification while connected.
- GATT connection and subscription.
- Raw command `0x01` observation display.
- Sanitized diagnostic export without location or hardware identifiers.

Exit criteria:

- User can select a BinoX.
- Service and characteristics are discovered by UUID.
- Repeated measurements reach the Rust decoder.
- Disconnect and permission revocation are recoverable.
- No REM PLE is created.
- No data is written to the BinoX.

## Phase 5: field semantics validation

Deliverables:

- Test record for each supported model/firmware.
- Correlation of raw values with displayed distance, pitch, and compass.
- Verified units and scales.
- Pitch sign convention.
- Compass north reference.
- Invalid/no-range behavior.
- Accuracy observations and known limits.

Exit criteria:

- A device profile can be marked verified using recorded evidence.
- Unknown firmware remains read-only.
- Geographic derivation remains blocked when reference metadata is missing.

## Phase 6: REM map/PLE host extension

Deliverables in the REM repository:

- New capability declarations and grants.
- New request handlers.
- New host events.
- Local observer-fix source with clear ownership.
- Draft PLE creation path.
- Authorized PLE selection/synchronization path.
- Binder and permission tests.
- Updated plugin SDK and AAR artifact.

Exit criteria:

- A fixture plugin can request the local observer fix.
- A fixture plugin can create a draft PLE.
- REM can send only an explicitly selected PLE set to a plugin.
- Revoked capabilities block the operations.
- Payloads remain below the 64 KiB limit.

## Phase 7: BinoX observation to REM draft PLE

Deliverables:

- Observer snapshot request.
- Geographic derivation in Rust.
- `map.ple.draft.create` request.
- Provenance and uncertainty metadata.
- Duplicate observation protection.
- UI state for blocked, draft-created, and failed outcomes.

Exit criteria:

- A valid BinoX observation creates a visible draft PLE through the real REM map path.
- Missing/stale/inaccurate observer fixes block projection.
- Unknown compass reference blocks projection.
- Raw observation remains available when projection is blocked.
- Repeated packets do not create uncontrolled duplicates.

## Phase 8: REM PLE to BinoX

Deliverables:

- Authorized session start/stop.
- Selection snapshot and change events.
- Geographic-to-relative conversion.
- Plugin-owned slot allocation.
- Command `0x02` transmission.
- Command `0x07` icon transmission.
- Update and removal handling.
- Raw device reply recording.

Exit criteria:

- A selected REM PLE appears on the BinoX.
- Position and icon changes can be sent independently.
- Removed PLEs affect only plugin-owned slots.
- Stopping the session prevents later automatic writes.
- Reconnect does not replay stale commands.
- Write completion, device reply, and visible result are reported separately.

## Phase 9: hardening and release

Deliverables:

- Supported device/firmware table.
- Permission and privacy review.
- Dependency and license review.
- Crash and lifecycle tests.
- Release signing procedure.
- Reproducible build notes.
- APK checksum and release notes.
- Known limitations.

Exit criteria:

- All required tests pass locally and in approved automation.
- No secrets, APK analysis artifacts, locations, MAC addresses, raw private captures, or signing files are committed.
- Release APK can be installed, trusted, enabled, configured, connected, stopped, and removed cleanly.

## 11. Test plan

### Rust unit tests

Required cases:

- Command `0x01` valid minimum frame.
- Negative signed fields.
- Short frames from zero through seven bytes.
- Unknown signature.
- Unknown command.
- Trailing bytes retained.
- Command `0x02` minimum/maximum accepted values.
- Slot/type and enabled/color packing.
- Overflow rejection before narrowing.
- Command `0x07` zero bytes, one byte, capacity minus one, exact capacity, capacity plus one, exact multiples, and multiple-plus-one.
- Fragment numbering, total length, slot ID, and header validation.
- Out-of-order, duplicate, missing, and conflicting fragments.
- PNG signature, dimensions, declared length, and CRC validation.
- Six-byte reply parsing with tail preserved.
- Geographic projection at cardinal bearings and near longitude wrap.
- Stale fix and poor-accuracy rejection.
- State-machine cancellation, timeout, reconnect, and duplicate-event behavior.

### Property tests and fuzzing

- Decoder never panics for arbitrary bytes.
- Fragmentation followed by reassembly returns the original PNG.
- Encoded lengths never exceed configured capacity.
- Slot allocator never assigns one active slot to two PLEs.
- Stop/disconnect leaves no queued write action.

### Android tests

- Manifest discovery metadata.
- Host package and certificate verification.
- Plugin start/stop lifecycle.
- Permission denial and revocation.
- Pairing cancellation.
- Fake GATT service/characteristic discovery.
- Descriptor subscription failure.
- MTU fallback.
- Write queue ordering.
- Binder death and bounded reconnect.
- Configuration state validation.

### REM integration tests

- Capability grant and denial.
- Observer snapshot belongs to the local operator.
- Draft PLE creation through the real map store.
- Selected-set isolation.
- Update and remove events.
- Session stop.
- 64 KiB request/response limits.

### Hardware acceptance

For each supported BinoX model/firmware:

- Pair and reconnect.
- Repeat the same stationary nonliving target measurement.
- Vary range, heading, and inclination separately where practical.
- Compare raw values, BinoX display, plugin display, and derived REM point.
- Test no-range and invalid measurements.
- Send one PLE and verify position/icon.
- Update position and icon.
- Remove the plugin-owned marker.
- Disconnect during a multi-fragment icon transfer.
- Restart REM and confirm that no stale write is replayed.

Keep test captures private and redact exported reports.

## 12. Security and privacy requirements

- No internet permission unless a later reviewed requirement demands it.
- No telemetry leaves the device through this plugin.
- Do not log exact coordinates, BLE addresses, serial numbers, or raw private packet captures in production.
- Use a redacted device key in diagnostics.
- Enforce REM host certificate verification.
- Keep plugin signing keys and REM fingerprints outside committed secrets.
- Bound every byte buffer, JSON payload, queue, fragment count, retry count, and pending request.
- Reject malformed data before allocation where possible.
- Prevent panic and Java exception leakage across JNI.
- Clear in-memory pending location/observation records on session stop when no longer needed.
- Store only required preferences.
- Do not clear BinoX markers outside the plugin-owned slot set.

## 13. Build requirements

Initial alignment with the checked REM baseline:

- Rust 1.88
- Rust edition 2021 unless the selected toolchain and REM move together
- Java 17
- compile/target SDK 35
- minimum Android API 26 for parity with the REM application
- Android NDK 27.0.12077973, rechecked before coding
- arm64-v8a release build
- x86_64 debug/test build

Add other ABIs only after confirming the REM distribution matrix.

Suggested local verification:

```bash
cargo +1.88 fmt --all --check
cargo +1.88 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.88 test --workspace --all-features

cd android
./gradlew testDebugUnitTest assembleDebug
```

Add dependency-license and vulnerability checks after the initial workspace builds. Resolve REM SDK/AAR redistribution terms before vendoring an AAR.

## 14. Main risks and decisions still required

| Item | Risk | Required decision or mitigation |
|---|---|---|
| REM has no PLE API | Plugin cannot complete map exchange | Implement the narrow API extension before phases 7 and 8 |
| Observer identity | Wrong telemetry position could be used | Add a local-observer operation with explicit ownership |
| Compass reference | Target may be rotated by magnetic declination | Verify reference; block projection while unknown |
| Units and scaling | Raw values may be misinterpreted | Verify per model/firmware and use versioned profiles |
| Firmware variation | Commands may differ | Maintain a supported-device matrix and read-only fallback |
| Slot limits/persistence | Plugin may overwrite unrelated markers | Discover limits and track only owned slots |
| Reply semantics | False success reporting | Preserve raw reply and separate transport/device/display states |
| MTU/write pacing | Fragment transfer may fail on some phones | Use actual capacity, serialized writes, bounded timeouts, and device tests |
| Exact-multiple icon defect | Lost final bytes or empty fragment | Use independent Rust fragmentation tests |
| SDK distribution | Build may rely on an unpublished AAR | Pin checksum, document source, and resolve license before distribution |
| Android background limits | Connection may be stopped | Use the plugin service lifecycle and a visible low-priority foreground notification |
| JNI errors | Crash or corrupted state | Narrow interface, handle validation, panic containment, and tests |

## 15. Definition of done

The plugin is complete only when all of the following are true:

1. A signed APK installs as a separate REM plugin.
2. REM verifies the plugin publisher, enables it, and grants only required capabilities.
3. The plugin verifies the REM host package and signer.
4. The user can select and connect a supported BinoX 4K or 4T.
5. Raw distance, pitch, and compass observations are decoded by Rust.
6. Physical interpretation is tied to a verified model/firmware profile.
7. A suitable local observer fix is obtained through the reviewed REM operation.
8. A valid observation creates a draft PLE through the actual REM map store.
9. An explicitly selected REM PLE can be displayed on the BinoX with its icon.
10. PLE updates flow only during the authorized session.
11. Session stop, permission revocation, REM stop, and BLE disconnect cancel pending work.
12. The plugin never clears or overwrites non-owned BinoX markers.
13. Tests cover codecs, JNI, lifecycle, Binder operations, map exchange, and supported hardware.
14. Release documentation states supported hardware, firmware, unresolved semantics, and privacy limits.
15. No private evidence, real location, device identifier, proprietary binary, credential, or signing material is committed.

## 16. First coding sequence

Execute work in this order:

1. Record the active REM commit and SDK checksum.
2. Approve the REM map/PLE extension contract.
3. Create the Cargo workspace and Rust toolchain file.
4. Implement `atn-protocol` with synthetic tests.
5. Implement icon fragmentation/reassembly and PNG validation.
6. Implement `atn-geo` with derivation gates.
7. Implement the deterministic `atn-plugin-core` state machine.
8. Add fake BLE and fake REM host adapters.
9. Add the JNI layer and panic containment.
10. Build the minimal Android plugin service and REM discovery metadata.
11. Add pairing, permissions, foreground notification, and read-only BLE.
12. Validate BinoX measurement semantics on hardware.
13. Implement and test the REM host extension.
14. Connect observations to draft PLE creation.
15. Add authorized PLE-to-BinoX synchronization.
16. Run lifecycle, security, privacy, and release checks.
