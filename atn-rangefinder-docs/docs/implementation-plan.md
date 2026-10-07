# Phased implementation plan

This document is a handoff for future implementation, not authorization to change REM or operate a device. Keep the existing repository conventions and verify the current host contract before coding.

## Integration constraint

Read-only inspection of `FreeTAKTeam/reticulum_mobile_emergency_management` at main commit `5a1275813a1c7037c21c68a4add1e862584a0551` identifies an Android APK/Binder plugin model. A pure Rust dynamically loaded module is not the existing plugin packaging model. Plan for a Rust protocol/validation core plus an Android service/JNI boundary; the Android layer owns GATT lifecycle, permissions and platform interaction.

Current host operations observed in `crates/reticulum_mobile/src/jni_bridge/plugin_host_api.rs` are `sensor.publish`, `lxmf.send`, `events.publish`, `notifications.raise`, and `operational.snapshot`. These names do not imply a map-POI API. In particular:

- `events.publish` emits generic plugin events, not a map point or event-projection record.
- `operational.snapshot` exposes latest operational context, not selection/listing of all map PLEs.
- Bidirectional PLE integration therefore needs an explicitly designed and reviewed REM host extension, or another subsequently verified existing contract. Do not pretend that extension already exists.
- Use PLE as the user's map-point term without inventing a formal expansion.

The REM contract document provides additional source references. Revalidate against the target checkout before implementation.

## Phase 0: contract and supported-device decision

Deliver: agreed map-facing operations, data ownership, supported model/firmware matrix and a protocol evidence register.

Acceptance:
- Specify how a PLE is selected, how a received observation becomes a draft map point, and how the user authorizes a session and selects PLEs for device synchronization. Subsequent updates to those selected PLEs may synchronize automatically within that authorized session; repeated per-update confirmation is not a design requirement.
- Define observer-location source, age/accuracy criteria and reference metadata. Unavailable or stale fixes must block geographic derivation rather than substitute zeros.
- Agree device-slot ownership, cleanup and reconnect behavior. Do not clear unrelated device markers.
- Confirm required Android app/service permissions and plugin manifest against the actual SDK.
- No LXMF dependency or new network transport is introduced. No map renderer or ballistic/aiming logic is changed.

## Phase 1: pure Rust codecs and synthetic tests

Deliver: isolated, side-effect-free codecs with documented numeric bounds and typed raw observations.

Acceptance:
- Decode supported incoming measurements using big-endian signed fields; encode display markers using little-endian fields.
- Unknown command, malformed signature, short input and unsupported length produce explicit results; no panic or guessed measurement.
- Separate raw numeric interpretation from physical-semantic validation.
- Preserve PNG bytes through fragmentation/reassembly with header validation, bounded memory, order/duplicate handling and explicit incomplete-transfer errors.
- Test zero bytes, one byte, exact capacity, exact multiples and capacity plus one. The app's apparent exact-multiple fragmentation defect is not copied.
- Reject integer overflow before narrowing. Verify actual permitted device limits later.
- Treat ACK trailing bytes and status meaning as opaque. Capture a response without assigning undocumented success labels.
- Every public test vector is synthetic. Include no captured identifiers or location-derived values.

## Phase 2: Android BLE boundary, read-only first

Deliver: discovery and notification observation through the actual Android plugin architecture.

Acceptance:
- Discover characteristic handles by UUID; negotiate/obtain actual write capacity rather than hard-code handles or infer MTU from value length.
- Explicit user-selected device, permission and connection state; cancellation/disconnect are recoverable.
- Record timestamp, model/firmware if available, source identity scoped appropriately, raw bytes and decoder version without leaking identifiers into public logs.
- Compare repeated safe observations with displayed distance/bearing/pitch and independent references where practical. Document units, north reference, inclination convention and invalid-value behavior before claiming calibrated semantics.
- No command replay, global marker clearing, automatic reconnection writes or scope expansion during read-only acceptance.

## Phase 3: receive observations into REM

Deliver: integration through the agreed, real REM contract.

Acceptance:
- Raw observation remains accessible independently of its derived position.
- Location timestamp, uncertainty, datum and co-location assumptions are retained; unknown north reference is visibly unresolved or derivation is gated.
- Draft/confirmed map state is explicit. A Bluetooth notification alone does not become a verified GNSS point.
- Test missing/stale fix, unknown bearing reference, repeated observations, disconnect, profile/service restart and revoked permission.
- Demonstrate point visibility through the actual map path, not merely a generic event on the bus.

## Phase 4: explicitly selected REM PLE to device

Deliver: bounded display-marker transmission with optional synthetic/local icon.

Acceptance:
- An authorized synchronization session sends a selected PLE using an owned slot. Subsequent updates may flow automatically for the selected set within that session, with a clear stop/disconnect control. Conversion uses explicit observer position and reference assumptions; geographic coordinates are not mistaken for fields in command 2.
- Position and icon are separate operations; image framing preserves PNG content and validated lengths.
- Model transport completion, reply receipt and device-display confirmation separately. Status byte 3 is observed, but its full meaning and reply tail are unresolved.
- Test icon/color change, position change, selected-point removal, partial transfer, timeout, reconnect and cancellation. Do not infer fragment-level acknowledgment or blindly retry writes.
- Remove only the adapter's own temporary test marker. No clear-all initialization unless explicitly requested and safe.

## Completion bar

Working codecs alone are not a functioning REM integration. Completion requires passing codec tests, verified platform lifecycle tests, real REM PLE exchange in both directions, documented supported hardware/firmware and a retained list of unresolved semantics. Claims must distinguish software tests, captured wire behavior and user-observed device rendering.

## Safe field verification

Use stationary nonliving surfaces, normal manufacturer-approved operation and no weapon integration. Start with repeated identical observations; then vary range, heading and inclination while recording all displayed values and timestamps. Changing inclination often changes actual range, so do not claim perfect single-variable isolation without measurement.

Android full HCI logging and `adb bugreport` can capture the phone endpoint; production APKs need not be debuggable. Keep reports private, export only reviewed relevant observations, and disable temporary logging afterward. Never root or unlock a device merely to obtain a capture.
