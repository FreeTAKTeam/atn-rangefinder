# atn-rangefinder

A Rust core and Android plugin for connecting ATN rangefinders and binoculars to [Reticulum Emergency Management (REM)](https://github.com/FreeTAKTeam/reticulum_mobile_emergency_management), with protocol research and original synthetic fixtures.

## Aim

The aim of this project is to connect ATN rangefinders and binoculars to REM so users can exchange observations and selected map points between the device and the REM map over Bluetooth.

In the planned workflow, a user measures a point with the ATN device and receives its range, bearing, and inclination in REM. With a suitable observer-position fix and verified reference information, the adapter can turn that observation into a map point. In the other direction, the user selects REM map points (PLEs) to show as markers on the device display. Updates to those selected points may synchronize during an explicitly authorized session, with a clear way to stop or disconnect.

This repository provides the adapter implementation, protocol evidence, integration contracts, and synthetic fixtures needed to validate that workflow. The intended outcome is a tested Android REM plugin backed by a Rust protocol core, with documented support for verified device models and firmware versions. End-to-end integration still requires the host contract and hardware acceptance.

## Project status

The repository now contains a Rust protocol core, configurable geographic validation and synchronization rules, a JSON/JNI adapter, and an Android REM plugin with explicit device selection and BLE lifecycle handling.

The adapter follows the inspected RCH marker record shape and uses the real REM APK/Binder SDK. **The current REM host still lacks the required map/observer API.** Codecs and synthetic adapter exchange are tested; real REM map visibility, supported firmware semantics, and physical-device rendering remain acceptance gates. See the [implementation status](docs/implementation-status.md).

## Implementation

- [`atn-protocol`](crates/atn-protocol): strict codecs, bounded icon reassembly and PNG validation.
- [`atn-domain`](crates/atn-domain): explicit observer/reference eligibility, WGS84 geometry and selected-slot sessions.
- [`atn-adapter`](crates/atn-adapter): bounded host requests, RCH marker compatibility and JNI actions.
- [Android plugin](android/README.md): trusted Binder service, permissions/settings activity, BLE notifications and serialized display writes.
- [Map contract](docs/map-contract.md), [configuration](docs/configuration.md), and [synthetic configuration template](examples/synthetic-config.json).

Build and validate the Rust workspace with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Android builds require an external pinned REM SDK AAR, its SHA-256, the trusted host package/certificate, and built JNI libraries. See [Android build instructions](android/README.md). No SDK source, proprietary binary, signing key or field capture is vendored.

## Intended scope

- Receive device range, bearing, and inclination observations, preserving their raw values and reference metadata.
- Derive map observations only when suitable observer-position and coordinate-reference information is available.
- Send selected REM map points (PLEs) to the device display within an explicitly authorized synchronization session.

LXMF, network-core changes, map rendering, ballistics, aiming, fire control, and firmware modification are outside scope.

## Documentation

The supplied research folder is preserved under [`atn-rangefinder-docs/`](atn-rangefinder-docs/):

1. [Research overview](atn-rangefinder-docs/README.md)
2. [Evidence and limitations](atn-rangefinder-docs/docs/evidence.md)
3. [Protocol observations](atn-rangefinder-docs/docs/protocol.md)
4. [REM integration contract](atn-rangefinder-docs/docs/rem-contract.md)
5. [Implementation plan and acceptance criteria](atn-rangefinder-docs/docs/implementation-plan.md)
6. [Synthetic fixtures](atn-rangefinder-docs/fixtures/README.md)

The fixtures are invented codec examples, not field evidence. No APK, decompiled proprietary source, private packet capture, actual location, or device identifier is included. The original fixtures now feed executable codec and integration tests.

## Contributing

Read the [handoff instructions](atn-rangefinder-docs/AGENTS.md) and evidence limitations before proposing implementation work. Keep synthetic examples separate from device observations, and do not commit credentials, raw private captures, proprietary binaries, or real location data.

## License

Licensed under the [Eclipse Public License 2.0](LICENSE) (`EPL-2.0`). This license covers the original material in this repository; it does not grant rights to external SDKs, applications, or other third-party material referenced by the research.
