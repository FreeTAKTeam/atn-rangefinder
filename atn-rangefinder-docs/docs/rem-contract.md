# REM integration evidence

Read-only source inspection. Pinned baseline: [FreeTAKTeam/reticulum_mobile_emergency_management](https://github.com/FreeTAKTeam/reticulum_mobile_emergency_management/tree/5a1275813a1c7037c21c68a4add1e862584a0551). Revalidate when implementation starts.

## Existing architecture

The [Android plugin guide](https://github.com/FreeTAKTeam/reticulum_mobile_emergency_management/blob/5a1275813a1c7037c21c68a4add1e862584a0551/docs/plugins/android-plugin-system.md) describes a separate APK, its own UID/process and an exported service discovered with action `network.reticulum.emergency.PLUGIN_V1`. The SDK supplies `RemPluginService`. Publisher trust, enabling the plugin and granting capabilities are separate decisions.

Actual `PluginProtocol.java` declares API 1.1; the SDK README's 1.0 statement is stale relative to that source. Maximum UTF-8 JSON envelope size is 65,536 bytes. Rust can implement the protocol core behind an Android service/JNI wrapper; this is not evidence of a pure-Rust dynamic-plugin loader.

The AIDL files under `packages/android-plugin-sdk/src/main/aidl/network/reticulum/emergency/plugin/api/` provide descriptor retrieval, `start(host, sessionJson)`, `stop(reason)`, and one-way host-event, response and configuration callbacks. The host accepts one-way `submitRequest(requestJson)`.

Requests use protocolVersion 1, requestId, operation and payload; the host determines pluginId from the connection. Responses correlate requestId and distinguish ok/result from error/code/message. Implement bounded pending requests and distinguish protocol rejection from Binder exception, timeout or process death.

`PluginCoordinator.java` starts compatible trusted/enabled plugins with the node foreground service and stops them when the node stops. UI visibility is not the lifecycle. The currently dispatched host event is `lxmf.received`, not map-point selection. This project does not need to adopt LXMF merely because that event exists.

## Map integration gap

The exhaustive operation switch in [plugin_host_api.rs](https://github.com/FreeTAKTeam/reticulum_mobile_emergency_management/blob/5a1275813a1c7037c21c68a4add1e862584a0551/crates/reticulum_mobile/src/jni_bridge/plugin_host_api.rs) implements:

- `sensor.publish`: plugin sensor projection; not map-point creation.
- `events.publish`: generic plugin event; not geographic event projection.
- `operational.snapshot`: latest operational context; not selected/listed PLEs and not guaranteed to be the local operator's GPS fix.
- `notifications.raise`.
- `lxmf.send`: outside this adapter's requested boundary.

There is no established map upsert/list/select/subscribe operation in this switch. Do not invent one as an existing API. Agree the required narrow REM extension before implementation.

The snapshot requires API minor >=1 plus the declared/granted `operationalRead` capability. It returns capturedAtMs, status, operationalSummary, eamReadiness, latestEvent and latestPosition. LatestPosition can be the newest telemetry record across records, so it is unsuitable as an assumed observer fix without ownership/source validation.

The internal `TelemetryPositionRecord` is keyed by callsign and includes lat/lon, optional altitude/course/speed/accuracy and timestamp. It is not a general PLE structure. Recording local telemetry may publish or replicate an operator fix; do not impersonate that fix to add a ranged observation. EventProjectionRecord likewise has no typed geographic point fields in the inspected contract.

## Implementation reference and security

The existing `plugins/ble-heart-rate` example demonstrates an Android BluetoothGatt service using the real plugin structure. Reuse architectural patterns, not its device UUIDs or measurement format.

Host package and certificate-fingerprint expectations are configured in the plugin build, and SDK CallerCertificateVerifier validates host Binder calls. Preserve this trust boundary. Never commit keystores, signing keys, keystore.properties, bugreports, device captures or proprietary APKs.

The next design decision is an explicit, narrow PLE/observer-location contract. This document is evidence and design input, not authorization to broaden networking core, map rendering or replication behavior.

## Build and licensing constraints

The inspected CI produces an SDK AAR as the `rem-plugin-sdk` Actions artifact. No published Maven coordinate was verified. A standalone build should accept/build a pinned AAR with a recorded checksum, subject to a licensing decision, rather than invent a dependency coordinate.

At this baseline the SDK uses compileSdk 35, minSdk 23 and Java 17; the REM application build uses minSdk 26, target/compile SDK 35 and NDK 27.0.12077973. Its Rust manifest requires 1.88; generic guidance mentioning 1.85 is not the authoritative manifest. Recheck tooling versions before implementation instead of treating this list as permanent requirements.

Android Bluetooth/location/foreground-service permissions are distinct from REM capability grants. Platform permission or pairing UI belongs to an explicit plugin-owned activity; an offline configuration WebView is not an automatic permission grant.

Source-tree inspection found no repository-level LICENSE/NOTICE. That is not a legal determination, but it is a reason not to assume copied SDK/source can be redistributed. Reference pinned sources and resolve licensing before vendoring. These documentation files add no license on behalf of the repository owner.
